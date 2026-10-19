//! CadCraft for assistants — the CadCraft app's 0.5 `agents.js` (the DOM-free part every surface shares) on the
//! Rust document and command line: describeDrawing, plainObject, listObjects, objectInFull, checkDrawing, runInput,
//! undoSteps, and the pieces of plot.js / sheets.js / commands.js they use (extentsOf, scaleLabel, describe).
//!
//! The headless tools ([`crate::headless`]) and the window's live tools (apps/cadcraft/src/control.rs) both answer
//! through these functions, as 0.5's CLI, MCP server and live window all answer through agents.js.

use std::collections::HashMap;

use cc_cad::entity::{Entity, Geo};
use cc_cad::{Doc, js};
use cc_cad_geom::geom::{Bounds, deg, norm_angle, prim_length, prims, text_corners};
use cc_cad_geom::pick::{empty_bounds, entity_bounds, grow_pt, prim_bounds, valid_bounds};
use cc_cad_geom::walk::{Opts as WOpts, WalkOpts, WalkOut, walk};
use cc_engine::{Engine, Host, Lang};
use cc_geom::P2;
use serde_json::{Map, Value, json};

/// `num()`: a finite number rounded to 1e-9 (agents.js), anything else as it is.
pub fn num(v: f64) -> Value {
    if v.is_finite() { cc_cad::num(js::round(v * 1e9) / 1e9) } else { Value::Null }
}
fn num_v(v: &Value) -> Value {
    match v.as_f64() {
        Some(f) if v.is_number() => num(f),
        _ => v.clone(),
    }
}
fn rnd6(v: f64) -> Value {
    num(js::round(v * 1e6) / 1e6)
}

// ---------------------------------------------------------------- plot.js extentsOf

/// The bounds plot.js `extentsOf` takes: everything the model draws (strokes, fills, texts, points), from the walk.
struct Ext {
    b: Bounds,
}
impl WalkOut for Ext {
    fn stroke(&mut self, _c: &str, ps: Vec<cc_cad_geom::geom::Prim>, _lw: f64, _o: Option<WOpts>) {
        for q in &ps {
            prim_bounds(q, &mut self.b);
        }
    }
    fn fill(&mut self, _c: &str, loops: Vec<Vec<f64>>, _solid: bool, _o: Option<WOpts>) {
        for l in &loops {
            for [x, y] in l.as_chunks::<2>().0 {
                grow_pt(&mut self.b, *x, *y);
            }
        }
    }
    fn text(&mut self, _c: &str, e: &Entity, _o: Option<WOpts>) {
        if let Geo::Text(t) = &e.geo {
            for p in text_corners(t) {
                grow_pt(&mut self.b, p.x, p.y);
            }
        }
    }
    fn point(&mut self, _c: &str, x: f64, y: f64) {
        grow_pt(&mut self.b, x, y);
    }
    // masks and construction lines are not part of the extents (plot.js keeps them apart; the sheet is 0 × 0)
    fn has_mask(&self) -> bool {
        true
    }
    fn has_infinite(&self) -> bool {
        true
    }
}

/// plot.js `extentsOf(doc)`: the model's drawn extents, or None.
pub fn extents_of(doc: &Doc) -> Option<Bounds> {
    let ents: Vec<Entity> = doc.model().cloned().collect();
    let mut out = Ext { b: empty_bounds() };
    walk(doc, &ents, &mut out, &WalkOpts { m: None, plot: true, frozen: None, max_dashes: None });
    valid_bounds(&out.b).then_some(out.b)
}

// ---------------------------------------------------------------- sheets.js scales

fn unit_mm(u: &str) -> Option<f64> {
    Some(match u {
        "millimeters" | "mm" => 1.0,
        "centimeters" => 10.0,
        "meters" => 1000.0,
        "kilometers" => 1e6,
        "inches" | "in" => 25.4,
        "feet" => 304.8,
        "yards" => 914.4,
        "miles" => 1_609_344.0,
        "microinches" => 25.4e-6,
        "mils" => 0.0254,
        _ => return None,
    })
}
fn meta_units(doc: &Doc) -> &str {
    doc.meta.get("units").and_then(Value::as_str).unwrap_or("")
}
fn is_imperial(doc: &Doc) -> bool {
    matches!(meta_units(doc), "inches" | "feet" | "yards" | "miles" | "microinches" | "mils")
}
/// `Number(n.toPrecision(6)).toString()`.
fn fmt_n(n: f64) -> String {
    if !n.is_finite() {
        return js::num_to_string(n);
    }
    js::num_to_string(to_precision(n, 6).parse::<f64>().unwrap_or(n))
}
fn ratio_label(n: f64) -> String {
    if n >= 1.0 { format!("1:{}", fmt_n(n)) } else { format!("{}:1", fmt_n(1.0 / n)) }
}
const METRIC: [f64; 20] =
    [0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 25.0, 50.0, 100.0, 200.0, 250.0, 500.0, 1000.0, 1250.0, 2000.0, 2500.0, 5000.0, 10000.0];
const ARCH: [(&str, f64); 17] = [
    ("1/16\" = 1'-0\"", 192.0),
    ("3/32\" = 1'-0\"", 128.0),
    ("1/8\" = 1'-0\"", 96.0),
    ("3/16\" = 1'-0\"", 64.0),
    ("1/4\" = 1'-0\"", 48.0),
    ("3/8\" = 1'-0\"", 32.0),
    ("1/2\" = 1'-0\"", 24.0),
    ("3/4\" = 1'-0\"", 16.0),
    ("1\" = 1'-0\"", 12.0),
    ("1-1/2\" = 1'-0\"", 8.0),
    ("3\" = 1'-0\"", 4.0),
    ("1\" = 10'", 120.0),
    ("1\" = 20'", 240.0),
    ("1\" = 30'", 360.0),
    ("1\" = 40'", 480.0),
    ("1\" = 50'", 600.0),
    ("1\" = 100'", 1200.0),
];
/// sheets.js `scaleList(doc)`: (n, label), architectural and engineering ones first for imperial drawings.
pub fn scale_list(doc: &Doc) -> Vec<(f64, String)> {
    let mut list: Vec<(f64, String)> = Vec::new();
    if is_imperial(doc) {
        let mut a: Vec<(f64, String)> = ARCH.iter().map(|(l, n)| (*n, (*l).to_string())).collect();
        a.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
        list.extend(a);
    }
    list.extend(METRIC.iter().map(|n| (*n, ratio_label(*n))));
    list
}
/// sheets.js `scaleLabel(doc, paperUnit, s)`.
pub fn scale_label(doc: &Doc, paper_unit: &str, s: f64) -> String {
    let model_mm = unit_mm(meta_units(doc)).or_else(|| unit_mm(paper_unit)).unwrap_or(1.0);
    let n = model_mm / unit_mm(paper_unit).unwrap_or(f64::NAN) / s;
    match scale_list(doc).into_iter().find(|(x, _)| (x - n).abs() < 1e-6 * n) {
        Some((_, l)) => l,
        None => ratio_label(n),
    }
}

// ---------------------------------------------------------------- commands.js describe

fn lang_en() -> &'static Lang {
    static L: std::sync::OnceLock<Lang> = std::sync::OnceLock::new();
    L.get_or_init(|| Lang::new(&serde_json::from_str(include_str!("../../../assets/lang/cadcraft/en.json")).unwrap_or(Value::Null)))
}

/// The catalog CadCraft prints from (lang/en.json of the 0.5 app).
pub fn lang() -> Lang {
    lang_en().clone()
}

/// commands.js `describe(e, units)`: one line about an object, lengths in the drawing's format.
pub fn describe(doc: &Doc, e: &Entity) -> String {
    let (lu, prec) = cc_cad_geom::units::units_of(Some(&doc.meta));
    let pv = json!(prec);
    let l = |n: f64| cc_cad_geom::units::format_length(n, f64::from(lu), Some(&pv));
    let p = |x: f64, y: f64| format!("{}, {}", l(x), l(y));
    let fmt = cc_engine::input::fmt;
    let lang = lang_en();
    let t = |k: &str, v: &[(&str, String)]| lang.t(k, v);
    let fo = e.to_obj();
    let n = |k: &str| fo.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
    match e.typ() {
        "line" => t(
            "cmd.desc.line",
            &[("from", p(n("x1"), n("y1"))), ("to", p(n("x2"), n("y2"))), ("length", l(js::hypot(n("x2") - n("x1"), n("y2") - n("y1"))))],
        ),
        "circle" => t("cmd.desc.circle", &[("center", p(n("cx"), n("cy"))), ("r", l(n("r")))]),
        "arc" => {
            t("cmd.desc.arc", &[("center", p(n("cx"), n("cy"))), ("r", l(n("r"))), ("a0", fmt(deg(n("a0")))), ("a1", fmt(deg(n("a1"))))])
        }
        "poly" => {
            let closed = fo.get("closed").is_some_and(|c| js::truthy(Some(c)));
            let count = fo.get("pts").and_then(Value::as_array).map_or(0, Vec::len);
            t(if closed { "cmd.desc.poly.closed" } else { "cmd.desc.poly" }, &[("n", count.to_string())])
        }
        "text" => {
            let s = fo.get("s").map(js::to_js_string).unwrap_or_default();
            t("cmd.desc.text", &[("s", s), ("at", p(n("x"), n("y"))), ("h", l(n("h")))])
        }
        "insert" => {
            let block = fo.get("block").map(js::to_js_string).unwrap_or_default();
            let na = fo.get("attrs").and_then(Value::as_object).map_or(0, Map::len);
            if na > 0 {
                t("cmd.desc.insert.attrs", &[("block", block), ("at", p(n("x"), n("y"))), ("n", na.to_string())])
            } else {
                t("cmd.desc.insert", &[("block", block), ("at", p(n("x"), n("y")))])
            }
        }
        "point" => p(n("x"), n("y")),
        "ellipse" => t("cmd.desc.ellipse", &[("center", p(n("cx"), n("cy"))), ("ratio", fmt(n("ratio")))]),
        "hatch" => t("cmd.desc.hatch", &[("n", fo.get("loops").and_then(Value::as_array).map_or(0, Vec::len).to_string())]),
        "dim" => {
            let kind_key = fo.get("kind").and_then(Value::as_str).filter(|k| !k.is_empty());
            let kind = kind_key
                .filter(|k| cc_cad_geom::dims::DIM_KINDS.contains(k))
                .map(|k| t(&format!("dimkind.{k}"), &[]))
                .unwrap_or_else(|| t("cmd.desc.dim.imported", &[]))
                .to_lowercase();
            if kind_key.is_none() {
                let block = fo.get("block").map(js::to_js_string).unwrap_or_default();
                return t("cmd.desc.dim.block", &[("kind", kind), ("block", block)]);
            }
            let value = cc_cad_geom::dims::format_measure(e, None);
            match fo.get("text").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                Some(text) => t("cmd.desc.dim.text", &[("kind", kind), ("value", value), ("text", text.to_string())]),
                None => t("cmd.desc.dim", &[("kind", kind), ("value", value)]),
            }
        }
        "viewport" => t(
            "cmd.desc.viewport",
            &[("w", l(n("w"))), ("h", l(n("h"))), ("at", p(n("x"), n("y"))), ("view", p(n("cx"), n("cy"))), ("s", fmt(n("s")))],
        ),
        ty @ ("xline" | "ray") => {
            t(&format!("cmd.desc.{ty}"), &[("at", p(n("x"), n("y"))), ("angle", fmt(deg(norm_angle(n("dy").atan2(n("dx"))))))])
        }
        "mleader" => {
            let s = cc_io_cad::text::one_line(fo.get("s").and_then(Value::as_str).unwrap_or(""));
            let pts = fo.get("pts").and_then(Value::as_array);
            let first = pts.and_then(|a| a.first());
            let at = p(
                first.and_then(|q| q.get("x")).and_then(Value::as_f64).unwrap_or(f64::NAN),
                first.and_then(|q| q.get("y")).and_then(Value::as_f64).unwrap_or(f64::NAN),
            );
            t("cmd.mld.describe", &[("s", s), ("at", at), ("n", pts.map_or(0, Vec::len).to_string())])
        }
        _ => String::new(),
    }
}

/// `x.toPrecision(p)` (finite x, 1 ≤ p ≤ 21): p significant digits, ties to the larger magnitude as ECMAScript says.
// digit arithmetic on 0–9 and exponents a double can have (|e| < 400): nothing can overflow
#[allow(clippy::arithmetic_side_effects)]
pub fn to_precision(x: f64, p: usize) -> String {
    if x == 0.0 {
        return if p > 1 { format!("0.{}", "0".repeat(p - 1)) } else { "0".into() };
    }
    let p = p.clamp(1, 21);
    // the exact digits, far enough past the cut to see a tie
    let exact = format!("{:.*e}", 60, x.abs());
    let (mant, exp) = exact.split_once('e').unwrap_or((&exact, "0"));
    let mut e: i32 = exp.parse().unwrap_or(0);
    let digits: Vec<u8> = mant.bytes().filter(u8::is_ascii_digit).map(|b| b - b'0').collect();
    let mut kept: Vec<u8> = digits.iter().take(p).copied().collect();
    let rest = digits.get(p..).unwrap_or(&[]);
    let up = match rest.first() {
        Some(d) if *d > 5 => true,
        Some(5) => true, // a tie or above: ECMAScript picks the larger n
        _ => false,
    };
    if up {
        let mut i = kept.len();
        loop {
            if i == 0 {
                kept.insert(0, 1);
                kept.pop();
                e = e.saturating_add(1);
                break;
            }
            i -= 1;
            if let Some(d) = kept.get_mut(i) {
                if *d == 9 {
                    *d = 0;
                } else {
                    *d += 1;
                    break;
                }
            }
        }
    }
    let ds: String = kept.iter().map(|d| char::from(b'0' + d)).collect();
    let neg = if x < 0.0 { "-" } else { "" };
    let pi = i32::try_from(p).unwrap_or(21);
    if e < -6 || e >= pi {
        let (a, b) = ds.split_at(1);
        let m = if b.is_empty() { a.to_string() } else { format!("{a}.{b}") };
        return format!("{neg}{m}e{}{}", if e >= 0 { "+" } else { "-" }, e.abs());
    }
    if e >= 0 {
        let k = usize::try_from(e).unwrap_or(0).saturating_add(1);
        let (a, b) = ds.split_at(k.min(ds.len()));
        return if b.is_empty() { format!("{neg}{a}") } else { format!("{neg}{a}.{b}") };
    }
    format!("{neg}0.{}{ds}", "0".repeat(usize::try_from(-e - 1).unwrap_or(0)))
}

/// model3d.js `fromFile`'s notes for the 3D faces doc.load took out. `model3d` = the drawing's saved CadCraft 3D model
/// text (`input.model3d`): when it reads, its own faces (layer CC-3D) are part of it and not counted as loose ones.
pub fn loose_faces_note(doc: &Doc, model3d: Option<&str>) -> Option<Vec<String>> {
    let lang = lang_en();
    let mut out = Vec::new();
    let mut restored = false;
    if let Some(text) = model3d.filter(|t| !t.is_empty()) {
        match serde_json::from_str::<Value>(text) {
            Ok(_) => restored = true,
            Err(e) => out.push(lang.t("model3d.read.not_restored", &[("why", crate::json::slice16(&e.to_string(), 160))])),
        }
    }
    let faces: Vec<&Value> = doc
        .faces3d
        .iter()
        .filter(|f| !restored || f.get("layer").map(js::to_js_string).unwrap_or_default().to_uppercase() != "CC-3D")
        .collect();
    if faces.is_empty() {
        return (!out.is_empty()).then_some(out);
    }
    let k = match meta_units(doc) {
        "millimeters" => 0.001,
        "centimeters" => 0.01,
        "meters" => 1.0,
        "kilometers" => 1000.0,
        "inches" => 0.0254,
        "feet" => 0.3048,
        "yards" => 0.9144,
        "miles" => 1609.344,
        "microinches" => 2.54e-8,
        "mils" => 2.54e-5,
        _ => 0.001,
    };
    let mut polys = 0usize;
    for f in faces.iter().take(200_000) {
        let mut pts: Vec<[f64; 3]> = Vec::new();
        let mut bad = false;
        for p in f.get("pts").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]) {
            let Some(a) = p.as_array() else {
                bad = true;
                break;
            };
            let xs: Vec<f64> = a.iter().filter_map(Value::as_f64).collect();
            if a.len() < 3 || xs.len() != a.len() || !xs.iter().all(|x| x.is_finite() && (x * k).abs() < 1e7) {
                bad = true;
                break;
            }
            let q = [xs.first().copied().unwrap_or(0.0) * k, xs.get(1).copied().unwrap_or(0.0) * k, xs.get(2).copied().unwrap_or(0.0) * k];
            if pts.last().is_none_or(|l| l.iter().zip(q.iter()).any(|(a, b)| (a - b).abs() > 1e-9)) {
                pts.push(q);
            }
        }
        if bad {
            continue;
        }
        if pts.len() > 1
            && let (Some(a), Some(b)) = (pts.first(), pts.last())
            && a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() <= 1e-9)
        {
            pts.pop();
        }
        if pts.len() >= 3 {
            polys = polys.saturating_add(1);
        }
    }
    if polys > 0 {
        out.push(lang.t("model3d.read.loose_faces", &[("n", polys.to_string())]));
    }
    if faces.len() > 200_000 {
        out.push(lang.t("model3d.read.faces_left_out", &[("n", faces.len().saturating_sub(200_000).to_string())]));
    }
    (!out.is_empty()).then_some(out)
}

// ---------------------------------------------------------------- reading (agents.js)

/// agents.js `describeDrawing(doc, {report, format})`.
pub fn describe_drawing(doc: &Doc, report: Option<&Value>, format: Option<&str>) -> Value {
    let mut counts = Map::new();
    let mut per_layer: HashMap<String, u64> = HashMap::new();
    let mut uses: HashMap<String, u64> = HashMap::new();
    let mut objects = 0u64;
    for e in doc.model() {
        objects = objects.saturating_add(1);
        let c = counts.entry(e.typ().to_string()).or_insert(json!(0));
        *c = json!(c.as_u64().unwrap_or(0).saturating_add(1));
        let n = per_layer.entry(e.layer_str().to_string()).or_insert(0);
        *n = n.saturating_add(1);
        if let Geo::Insert(i) = &e.geo {
            let u = uses.entry(i.block.clone()).or_insert(0);
            *u = u.saturating_add(1);
        }
    }
    let b = extents_of(doc);
    let (lu, prec) = cc_cad_geom::units::units_of(Some(&doc.meta));
    let lunits_name = cc_cad_geom::units::LUNITS.iter().find(|(c, _)| *c == lu).map(|(_, n)| *n);
    let meta_s = |k: &str| doc.meta.get(k).filter(|v| js::truthy(Some(v))).cloned();
    let layers: Vec<Value> = doc
        .layers
        .values()
        .map(|l| {
            let v = l.to_value();
            json!({
                "name": l.name, "color": v.get("color").cloned().unwrap_or(Value::Null), "on": l.on, "frozen": l.frozen, "locked": l.locked, "plot": l.plot,
                "linetype": l.linetype.clone().filter(|s| !s.is_empty()), "lineweight": l.lineweight.map_or(Value::Null, cc_cad::num),
                "objects": per_layer.get(&l.name).copied().unwrap_or(0)
            })
        })
        .collect();
    let blocks: Vec<Value> = doc
        .blocks
        .values()
        .filter(|bk| !bk.name.starts_with('*'))
        .map(|bk| {
            let tags: Vec<Value> =
                bk.attdefs.as_deref().unwrap_or(&[]).iter().map(|a| a.get("tag").cloned().unwrap_or(Value::Null)).collect();
            json!({"name": bk.name, "objects": bk.entities.len(), "uses": uses.get(&bk.name).copied().unwrap_or(0), "attributes": tags})
        })
        .collect();
    let layouts: Vec<Value> = doc
        .layouts
        .iter()
        .map(|l| {
            let unit = l.unit().unwrap_or("mm").to_string();
            let paper = if l.paper.is_empty() {
                Value::Null
            } else {
                json!({"w": rnd6(l.w().unwrap_or(f64::NAN)), "h": rnd6(l.h().unwrap_or(f64::NAN)), "unit": l.paper.get("unit").cloned().unwrap_or(Value::Null)})
            };
            let ents: Vec<&Entity> = doc.space_entities(l.space).collect();
            let vps: Vec<Value> = ents
                .iter()
                .filter(|e| e.typ() == "viewport")
                .map(|v| {
                    let vo = v.to_obj();
                    let vn = |k: &str| vo.get(k).and_then(Value::as_f64);
                    let s = vn("s").filter(|s| *s != 0.0 && !s.is_nan());
                    json!({"id": v.id, "w": num(vn("w").unwrap_or(f64::NAN)), "h": num(vn("h").unwrap_or(f64::NAN)), "scale": s.map(|s| scale_label(doc, &unit, s))})
                })
                .collect();
            json!({"name": l.name, "paper": paper, "objects": ents.len(), "viewports": vps})
        })
        .collect();
    let styles: Vec<Value> = doc
        .text_styles
        .iter()
        .take(200)
        // `(s && s.name) || s`: doc.js keeps each style as { font, h, wf, obl } under its name, so the style itself
        .map(|(_, s)| s.get("name").filter(|n| js::truthy(Some(n))).cloned().unwrap_or_else(|| Value::Object(s.clone())))
        .collect();
    let notes: Vec<Value> =
        report.and_then(|r| r.get("notes")).and_then(Value::as_array).map(|a| a.iter().take(50).cloned().collect()).unwrap_or_default();
    json!({
        "name": doc.meta.get("name").cloned().unwrap_or(Value::Null),
        "source": meta_s("source"),
        "format": meta_s("format").or_else(|| format.map(|f| Value::String(f.into()))),
        "units": {"insunits": meta_s("units").unwrap_or_else(|| json!("unitless")), "lengthFormat": lunits_name, "precision": prec},
        "extents": b.map(|b| json!({"minX": num(b.min_x), "minY": num(b.min_y), "maxX": num(b.max_x), "maxY": num(b.max_y)})),
        "objects": objects,
        "byType": Value::Object(counts),
        "layers": layers,
        "blocks": blocks,
        "layouts": layouts,
        "textStyles": styles,
        "views": doc.views.iter().map(|v| v.name.clone()).collect::<Vec<_>>(),
        "unsaved": doc.version != doc.saved_version,
        "notes": notes,
    })
}

/// agents.js `plainObject(doc, e, {full})`: the object's fields (+ length / area, a one-line description).
pub fn plain_object(doc: &Doc, e: &Entity, full: bool) -> Value {
    let mut o = Map::new();
    for (k, v) in e.to_obj() {
        if k.starts_with('_') {
            continue;
        }
        if !full
            && let Value::Array(a) = &v
            && a.len() > 200
        {
            o.insert(k.clone(), Value::Array(a.iter().take(200).cloned().collect()));
            o.insert(format!("{k}Total"), json!(a.len()));
            continue;
        }
        o.insert(k, num_v(&v));
    }
    if matches!(e.typ(), "line" | "arc" | "poly" | "circle" | "ellipse") {
        let ps = prims(e, None);
        if !ps.is_empty() {
            o.insert("length".into(), num(ps.iter().map(prim_length).sum()));
        }
    }
    if matches!(e.typ(), "circle" | "poly" | "ellipse" | "hatch")
        && let Some((a, _, kind)) = cc_cad_geom::edit::entity_area(e)
        && a.is_finite()
        && kind != "open"
    {
        o.insert("area".into(), num(a));
    }
    o.insert("description".into(), Value::String(describe(doc, e)));
    Value::Object(o)
}

/// The space a name picks: "model" (or nothing) = the model, else a sheet by name (any case).
fn space_of(doc: &Doc, name: Option<&str>) -> Result<cc_cad::SpaceId, String> {
    match name {
        None | Some("") | Some("model") => Ok(cc_cad::MODEL),
        Some(n) => doc.layouts.iter().find(|l| l.name.to_lowercase() == n.to_lowercase()).map(|l| l.space).ok_or_else(|| {
            let names: Vec<&str> = doc.layouts.iter().map(|l| l.name.as_str()).collect();
            format!("There is no sheet \"{n}\". Sheets: {}.", if names.is_empty() { "none".to_string() } else { names.join(", ") })
        }),
    }
}

/// agents.js `listObjects(doc, filters)`.
pub fn list_objects(doc: &Doc, a: &Value) -> Result<Value, String> {
    let s = |k: &str| a.get(k).and_then(Value::as_str).filter(|x| !x.is_empty());
    let ty = s("type");
    let lay = s("layer").map(str::to_lowercase);
    let block = s("block");
    let txt = s("text").map(str::to_lowercase);
    let within = a.get("within").filter(|w| w.is_object());
    let only: Option<Vec<u64>> = a.get("ids").and_then(Value::as_array).map(|v| v.iter().filter_map(Value::as_u64).collect());
    let offset = usize::try_from(a.get("offset").and_then(Value::as_u64).unwrap_or(0)).unwrap_or(usize::MAX);
    let limit = usize::try_from(a.get("limit").and_then(Value::as_u64).unwrap_or(100)).unwrap_or(usize::MAX);
    let sp = space_of(doc, a.get("space").and_then(Value::as_str))?;
    let mut hits: Vec<&Entity> = Vec::new();
    for e in doc.space_entities(sp) {
        if let Some(o) = &only
            && !e.id.is_some_and(|id| o.contains(&id))
        {
            continue;
        }
        if ty.is_some_and(|t| e.typ() != t) {
            continue;
        }
        if lay.as_ref().is_some_and(|l| e.layer_str().to_lowercase() != *l) {
            continue;
        }
        if let Some(bn) = block
            && !matches!(&e.geo, Geo::Insert(i) if i.block == bn)
        {
            continue;
        }
        if let Some(t) = &txt {
            let eo = e.to_obj();
            let in_s = eo.get("s").map(js::to_js_string).unwrap_or_default().to_lowercase().contains(t.as_str());
            let in_attrs = eo
                .get("attrs")
                .and_then(Value::as_object)
                .is_some_and(|m| m.values().any(|v| js::to_js_string(v).to_lowercase().contains(t.as_str())));
            if !(in_s || in_attrs) {
                continue;
            }
        }
        if let Some(w) = within {
            let f = |k: &str| w.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
            let b = entity_bounds(e, Some(&doc.blocks));
            // `!b || b.minX < w.minX …` (an empty box has ±Infinity sides, which fail the test the same way)
            if !(b.min_x >= f("minX") && b.min_y >= f("minY") && b.max_x <= f("maxX") && b.max_y <= f("maxY")) {
                continue;
            }
        }
        hits.push(e);
    }
    let objects: Vec<Value> = hits.iter().skip(offset).take(limit).map(|e| plain_object(doc, e, false)).collect();
    Ok(json!({"total": hits.len(), "offset": a.get("offset").cloned().unwrap_or(json!(0)), "objects": objects}))
}

/// agents.js `objectInFull(doc, id)`: any space; a reference with its block definition.
pub fn object_in_full(doc: &Doc, id: u64) -> Option<Value> {
    let spaces = std::iter::once(cc_cad::MODEL).chain(doc.layouts.iter().map(|l| l.space));
    for sp in spaces {
        let Some(e) = doc.space_map(sp).and_then(|m| m.get(&id)) else { continue };
        let mut o = plain_object(doc, e, true);
        if let Geo::Insert(i) = &e.geo
            && let Some(bk) = doc.blocks.get(&i.block)
            && let Some(m) = o.as_object_mut()
        {
            let atts: Vec<Value> = bk
                .attdefs
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .map(|a| {
                    json!({"tag": a.get("tag").cloned().unwrap_or(Value::Null), "prompt": a.get("prompt").cloned().unwrap_or(Value::Null),
                        "invisible": js::truthy(a.get("invisible")), "constant": js::truthy(a.get("constant"))})
                })
                .collect();
            m.insert("blockDefinition".into(), json!({"name": bk.name, "objects": bk.entities.len(), "attributes": atts}));
        }
        return Some(o);
    }
    None
}

// ---------------------------------------------------------------- standards check (AGENTS-SPEC § 5.3)

/// A layer / style name rule: a name (any case) or `/regex/` with an optional `i` flag.
type NameTest = Box<dyn Fn(&str) -> bool>;

fn matcher(list: &[Value]) -> Result<Vec<NameTest>, String> {
    let mut out: Vec<NameTest> = Vec::new();
    for p in list {
        let s = js::to_js_string(p);
        let re = s.strip_prefix('/').and_then(|r| {
            let (body, flags) = r.rsplit_once('/')?;
            flags.chars().all(|c| c == 'i').then(|| (body.to_string(), !flags.is_empty()))
        });
        match re {
            Some((body, ci)) => {
                let rx = regex::RegexBuilder::new(&body)
                    .case_insensitive(ci)
                    .size_limit(1 << 20)
                    .dfa_size_limit(1 << 20)
                    .build()
                    .map_err(|e| format!("Invalid regular expression: /{body}/: {e}"))?;
                out.push(Box::new(move |n: &str| rx.is_match(n)));
            }
            None => {
                let low = s.to_lowercase();
                out.push(Box::new(move |n: &str| n.to_lowercase() == low));
            }
        }
    }
    Ok(out)
}

/// agents.js `checkDrawing(doc, rules)`.
pub fn check_drawing(doc: &Doc, rules: &Value) -> Result<Value, String> {
    let mut findings: Vec<Value> = Vec::new();
    let mut add = |rule: &str, severity: &str, message: String, ids: Vec<u64>| {
        findings.push(json!({"rule": rule, "severity": severity, "message": message, "ids": ids.iter().take(200).collect::<Vec<_>>(), "count": ids.len()}));
    };
    let all: Vec<&Entity> = doc.model().chain(doc.layouts.iter().flat_map(|l| doc.space_entities(l.space))).collect();
    let ids_where = |f: &dyn Fn(&Entity) -> bool| -> Vec<u64> { all.iter().filter(|e| f(e)).filter_map(|e| e.id).collect() };
    let arr = |v: Option<&Value>| v.and_then(Value::as_array).cloned();
    let l = rules.get("layers");
    if let Some(mut allowed) = arr(l.and_then(|l| l.get("allowed"))) {
        allowed.push(json!("0"));
        let ok = matcher(&allowed)?;
        for layer in doc.layers.values() {
            if !ok.iter().any(|f| f(&layer.name)) {
                let ids = ids_where(&|e: &Entity| e.layer_str() == layer.name);
                add("layers.allowed", "error", format!("Layer \"{}\" is not in the allowed list.", layer.name), ids);
            }
        }
    }
    for r in arr(l.and_then(|l| l.get("required"))).unwrap_or_default() {
        let r = js::to_js_string(&r);
        if !doc.layers.keys().any(|n| n.to_lowercase() == r.to_lowercase()) {
            add("layers.required", "error", format!("Layer \"{r}\" is missing."), Vec::new());
        }
    }
    for r in arr(l.and_then(|l| l.get("noObjectsOn"))).unwrap_or_default() {
        let r = js::to_js_string(&r);
        let ids = ids_where(&|e: &Entity| e.layer_str().to_lowercase() == r.to_lowercase());
        if !ids.is_empty() {
            let msg = format!("{} object{} on layer \"{r}\".", ids.len(), if ids.len() == 1 { " is" } else { "s are" });
            add("layers.noObjectsOn", "error", msg, ids);
        }
    }
    let o = rules.get("objects");
    let flag = |k: &str| js::truthy(o.and_then(|o| o.get(k)));
    if flag("colorByLayer") {
        let ids = ids_where(&|e: &Entity| e.color.as_ref().is_some_and(|c| !matches!(c, cc_cad::Color::Null)) && e.typ() != "viewport");
        if !ids.is_empty() {
            add("objects.colorByLayer", "error", format!("{} object(s) have their own colour instead of ByLayer.", ids.len()), ids);
        }
    }
    if flag("linetypeByLayer") {
        let ids = ids_where(&|e: &Entity| matches!(&e.lt, Some(Some(lt)) if !lt.is_empty() && !lt.eq_ignore_ascii_case("bylayer")));
        if !ids.is_empty() {
            add("objects.linetypeByLayer", "error", format!("{} object(s) have their own linetype.", ids.len()), ids);
        }
    }
    if flag("lineweightByLayer") {
        let ids = ids_where(&|e: &Entity| matches!(e.lw, Some(Some(w)) if w != -1.0));
        if !ids.is_empty() {
            add("objects.lineweightByLayer", "error", format!("{} object(s) have their own lineweight.", ids.len()), ids);
        }
    }
    if let Some(hs) = arr(o.and_then(|o| o.get("textHeights"))) {
        let ok_h: Vec<f64> = hs.iter().map(js::to_number).collect();
        let ids = ids_where(&|e: &Entity| {
            matches!(e.typ(), "text" | "mleader")
                && e.to_obj()
                    .get("h")
                    .and_then(Value::as_f64)
                    .is_some_and(|h| h.is_finite() && !ok_h.iter().any(|k| (k - h).abs() <= 1e-6 * k.max(1.0)))
        });
        if !ids.is_empty() {
            let shown: Vec<String> = ok_h.iter().map(|h| js::num_to_string(*h)).collect();
            add("objects.textHeights", "error", format!("{} text(s) use a height not in {}.", ids.len(), shown.join(", ")), ids);
        }
    }
    if let Some(ss) = arr(o.and_then(|o| o.get("textStyles"))) {
        let ok_s = matcher(&ss)?;
        let ids = ids_where(&|e: &Entity| {
            e.typ() == "text"
                && e.to_obj()
                    .get("style")
                    .cloned()
                    .filter(|s| js::truthy(Some(s)))
                    .is_some_and(|s| !ok_s.iter().any(|f| f(&js::to_js_string(&s))))
        });
        if !ids.is_empty() {
            add("objects.textStyles", "error", format!("{} text(s) use a style not in the list.", ids.len()), ids);
        }
    }
    if js::truthy(rules.get("blocks").and_then(|b| b.get("noUnused"))) {
        let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut walk_used = |ents: &mut dyn Iterator<Item = &Entity>| {
            for e in ents {
                if let Some(b) = cc_cad::doc::block_ref(e) {
                    used.insert(b.to_string());
                }
            }
        };
        walk_used(&mut all.iter().copied());
        for bk in doc.blocks.values() {
            walk_used(&mut bk.entities.iter());
        }
        for bk in doc.blocks.values() {
            if !bk.name.starts_with('*') && !bk.name.starts_with('_') && !used.contains(&bk.name) {
                add("blocks.noUnused", "warning", format!("Block \"{}\" is defined but not used.", bk.name), Vec::new());
            }
        }
    }
    if let Some(u) = rules.get("units").filter(|u| js::truthy(Some(u))) {
        let (lu, _) = cc_cad_geom::units::units_of(Some(&doc.meta));
        let name = |c: f64| cc_cad_geom::units::LUNITS.iter().find(|(k, _)| f64::from(*k) == c).map(|(_, n)| (*n).to_string());
        if let Some(want) = u.get("lunits")
            && js::to_number(want) != f64::from(lu)
        {
            let shown = name(js::to_number(want)).unwrap_or_else(|| js::to_js_string(want));
            add(
                "units.lunits",
                "error",
                format!("The length format is {}, not {shown}.", name(f64::from(lu)).unwrap_or_default()),
                Vec::new(),
            );
        }
        if let Some(want) = u.get("insunits") {
            let have = doc.meta.get("units").filter(|v| js::truthy(Some(v))).map_or_else(|| "unitless".to_string(), js::to_js_string);
            if js::to_js_string(want).to_lowercase() != have.to_lowercase() {
                add("units.insunits", "error", format!("The drawing units are {have}, not {}.", js::to_js_string(want)), Vec::new());
            }
        }
    }
    let ok = !findings.iter().any(|f| f["severity"] == "error");
    Ok(json!({"ok": ok, "findings": findings}))
}

// ---------------------------------------------------------------- the command line (agents.js runInput)

/// A script's lines: one per prompt answer; a blank line is Enter, but the newline that ends the file is not a line.
pub fn script_lines(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l).to_string()).collect();
    if lines.len() > 1 && lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

#[derive(Default)]
struct RecState {
    printed: Vec<String>,
    cmds: Vec<String>,
    shift: Option<bool>,
    cursor: Option<P2>,
    force_pick: Option<u64>,
}

/// The host while an assistant's input runs: everything goes to the window's own host, and what is printed and which
/// commands start are recorded (agents.js `capture` and `host.onCommand`).
struct Rec {
    inner: std::rc::Rc<std::cell::RefCell<Option<Box<dyn Host>>>>,
    st: std::rc::Rc<std::cell::RefCell<RecState>>,
}

impl Rec {
    fn with<R>(&self, d: R, f: impl FnOnce(&mut Box<dyn Host>) -> R) -> R {
        match self.inner.borrow_mut().as_mut() {
            Some(h) => f(h),
            None => d,
        }
    }
}

impl Host for Rec {
    fn print(&mut self, s: &str) {
        {
            let mut st = self.st.borrow_mut();
            for l in s.split('\n') {
                if st.printed.len() < 2000 {
                    st.printed.push(l.to_string());
                }
            }
        }
        self.with((), |h| h.print(s));
    }
    fn prompt(&mut self, s: &str) {
        self.with((), |h| h.prompt(s));
    }
    fn cursor(&self) -> Option<P2> {
        let c = self.st.borrow().cursor;
        c.or_else(|| self.with(None, |h| h.cursor()))
    }
    fn shift(&self) -> bool {
        let s = self.st.borrow().shift;
        s.unwrap_or_else(|| self.with(false, |h| h.shift()))
    }
    fn selection(&self) -> Vec<u64> {
        self.with(Vec::new(), |h| h.selection())
    }
    fn set_selection(&mut self, ids: &[u64]) {
        self.with((), |h| h.set_selection(ids));
    }
    fn pick_at(&self, doc: &Doc, p: P2) -> Option<u64> {
        if let Some(id) = self.st.borrow().force_pick {
            return Some(id);
        }
        self.with(None, |h| h.pick_at(doc, p))
    }
    fn pick_window(&self, doc: &Doc, w: &Bounds, crossing: bool) -> Vec<u64> {
        self.with(Vec::new(), |h| h.pick_window(doc, w, crossing))
    }
    fn preview(&mut self, ents: Vec<Entity>) {
        self.with((), |h| h.preview(ents));
    }
    fn on_command(&mut self, name: Option<&str>) {
        if let Some(n) = name {
            self.st.borrow_mut().cmds.push(n.to_string());
        }
        self.with((), |h| h.on_command(name));
    }
    fn zoom_extents(&mut self) {
        self.with((), |h| h.zoom_extents());
    }
    fn zoom_window(&mut self, a: P2, b: P2, exact: bool) {
        self.with((), |h| h.zoom_window(a, b, exact));
    }
    fn zoom_previous(&mut self) -> bool {
        self.with(false, |h| h.zoom_previous())
    }
    fn zoom_by(&mut self, f: f64) {
        self.with((), |h| h.zoom_by(f));
    }
    fn view_rect(&self) -> Option<(f64, f64)> {
        self.with(None, |h| h.view_rect())
    }
}

/// A stand-in while the window's host is lent to the recorder.
struct Lent;
impl Host for Lent {
    fn print(&mut self, _s: &str) {}
    fn prompt(&mut self, _s: &str) {}
    fn cursor(&self) -> Option<P2> {
        None
    }
    fn selection(&self) -> Vec<u64> {
        Vec::new()
    }
    fn set_selection(&mut self, _ids: &[u64]) {}
    fn pick_at(&self, _doc: &Doc, _p: P2) -> Option<u64> {
        None
    }
    fn pick_window(&self, _doc: &Doc, _w: &Bounds, _crossing: bool) -> Vec<u64> {
        Vec::new()
    }
    fn preview(&mut self, _ents: Vec<Entity>) {}
    fn zoom_extents(&mut self) {}
    fn zoom_window(&mut self, _a: P2, _b: P2, _exact: bool) {}
}

fn kind_name(k: &cc_engine::prompt::Kind) -> &'static str {
    use cc_engine::prompt::Kind;
    match k {
        Kind::Point => "point",
        Kind::Distance => "distance",
        Kind::Angle => "angle",
        Kind::Number => "number",
        Kind::Text => "string",
        Kind::Keyword => "keyword",
        Kind::Select => "select",
        Kind::Entity => "entity",
    }
}

fn snapshot(doc: &Doc) -> Vec<(u64, Entity)> {
    doc.model().chain(doc.layouts.iter().flat_map(|l| doc.space_entities(l.space))).filter_map(|e| e.id.map(|id| (id, e.clone()))).collect()
}

/// agents.js `anchor(doc, e)`: where to "click" an object given by id.
fn anchor(doc: &Doc, e: &Entity) -> P2 {
    let ps = prims(e, Some(&doc.blocks));
    if let Some(q) = ps.first() {
        return if q.k == cc_cad_geom::geom::K::L {
            P2::new((q.x1 + q.x2) / 2.0, (q.y1 + q.y2) / 2.0)
        } else {
            let a = (q.a0 + q.a1) / 2.0;
            P2::new(q.cx + q.r * a.cos(), q.cy + q.r * a.sin())
        };
    }
    let b = entity_bounds(e, Some(&doc.blocks));
    if valid_bounds(&b) { P2::new((b.min_x + b.max_x) / 2.0, (b.min_y + b.max_y) / 2.0) } else { P2::new(0.0, 0.0) }
}

fn pt_of(v: Option<&Value>) -> Option<P2> {
    let a = v?.as_array()?;
    if a.len() < 2 || !a.iter().all(|n| js::to_number(n).is_finite()) {
        return None;
    }
    Some(P2::new(js::to_number(a.first()?), js::to_number(a.get(1)?)))
}

/// One input item → None, or an error message (agents.js `feed`).
fn feed(eng: &mut Engine, st: &std::rc::Rc<std::cell::RefCell<RecState>>, raw: &Value) -> Option<String> {
    let o = match raw {
        Value::String(s) => {
            eng.text(s);
            return None;
        }
        Value::Object(o) => o,
        _ => return Some("Each input is a string, or an object {pick}, {window}, {entity} or {escape}.".into()),
    };
    let saved = st.borrow().shift;
    st.borrow_mut().shift = Some(js::truthy(o.get("shift")));
    let out = (|| {
        if js::truthy(o.get("escape")) {
            eng.escape();
            return None;
        }
        if o.contains_key("pick") {
            let Some(p) = pt_of(o.get("pick")) else { return Some("pick needs [x, y].".into()) };
            let xy = format!("{},{}", js::num_to_string(p.x), js::num_to_string(p.y));
            if eng.expects().is_none() {
                return Some(format!("Nothing is asking for a point or an object (pick {xy})."));
            }
            st.borrow_mut().cursor = Some(p);
            if !eng.click(p, None, p) && eng.expects() == Some(cc_engine::prompt::Kind::Select) {
                eng.host_mut().print(&format!("Nothing found at {xy}."));
            }
            return None;
        }
        if o.contains_key("window") {
            let w = o.get("window").and_then(Value::as_array);
            let nums: Option<Vec<f64>> = w.filter(|a| a.len() == 4).map(|a| a.iter().map(js::to_number).collect());
            let Some([x1, y1, x2, y2]) = nums.filter(|n| n.iter().all(|x| x.is_finite())).and_then(|n| <[f64; 4]>::try_from(n).ok()) else {
                return Some("window needs [x1, y1, x2, y2].".into());
            };
            if eng.expects() != Some(cc_engine::prompt::Kind::Select) {
                return Some("A window is only taken where objects are being selected.".into());
            }
            let b = Bounds { min_x: x1.min(x2), min_y: y1.min(y2), max_x: x1.max(x2), max_y: y1.max(y2) };
            eng.window(b, js::truthy(o.get("crossing")));
            return None;
        }
        if o.contains_key("entity") {
            let id = o.get("entity").map(js::to_number).unwrap_or(f64::NAN);
            let found = if id.is_finite() && id >= 0.0 && id.fract() == 0.0 {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let key = id as u64;
                eng.doc().get(key).cloned()
            } else {
                None
            };
            let Some(e) = found else {
                return Some(format!("There is no object {}.", o.get("entity").map(js::to_js_string).unwrap_or_default()));
            };
            let kind = eng.expects();
            let Some(kind) = kind.filter(|k| matches!(k, cc_engine::prompt::Kind::Entity | cc_engine::prompt::Kind::Select)) else {
                return Some("Nothing is asking for an object.".into());
            };
            let at = pt_of(o.get("at")).unwrap_or_else(|| anchor(&eng.doc(), &e));
            st.borrow_mut().force_pick = e.id;
            if kind == cc_engine::prompt::Kind::Select {
                st.borrow_mut().shift = Some(false);
                eng.click(at, None, at);
                eng.host_mut().print("1 found");
            } else {
                eng.click(at, None, at);
            }
            st.borrow_mut().force_pick = None;
            return None;
        }
        Some(format!("Unknown input {}.", crate::json::slice16(&crate::json::stringify(raw), 80)))
    })();
    st.borrow_mut().shift = saved;
    out
}

/// agents.js `runInput`: every line (or {pick}/{window}/{entity}/{escape}) answers the next prompt; a command still
/// waiting after the last input is ended with Esc. All of it = ONE undo step, labelled `prefix` + the commands run (or
/// `label`). → {printed, commands, added, changed, removed, waiting, undo, error}.
pub fn run_input(eng: &mut Engine, input: &Value, select: Option<&[u64]>, label: Option<&str>, prefix: &str) -> Value {
    let lines: Vec<Value> = match input {
        Value::Array(a) => a.clone(),
        Value::String(s) => script_lines(s).into_iter().map(Value::String).collect(),
        other => script_lines(&js::to_js_string(other)).into_iter().map(Value::String).collect(),
    };
    // lend the window's host to the recorder for the length of the call
    let inner = std::rc::Rc::new(std::cell::RefCell::new(Some(std::mem::replace(&mut *eng.host_mut(), Box::new(Lent)))));
    let st = std::rc::Rc::new(std::cell::RefCell::new(RecState::default()));
    *eng.host_mut() = Box::new(Rec { inner: inner.clone(), st: st.clone() });
    if eng.busy() {
        eng.escape();
    }
    if let Some(sel) = select {
        let ok: Vec<u64> = sel.iter().copied().filter(|id| eng.doc().get(*id).is_some()).collect();
        eng.host_mut().set_selection(&ok);
    }
    let before = snapshot(&eng.doc());
    let mark = eng.doc().mark();
    let mut error: Option<String> = None;
    for raw in &lines {
        if let Some(e) = feed(eng, &st, raw) {
            error = Some(e);
            break;
        }
    }
    let mut waiting = Value::Null;
    if eng.busy() {
        let kind = eng.expects();
        let prompt = if kind.is_some() { eng.prompt_line() } else { String::new() };
        waiting = json!({"command": eng.running(), "kind": kind.as_ref().map(kind_name), "prompt": prompt});
        eng.escape();
        if eng.busy() {
            eng.escape();
        }
    }
    let cmds = st.borrow().cmds.clone();
    let mut uniq: Vec<&str> = Vec::new();
    for c in &cmds {
        if !c.starts_with('_') && !uniq.contains(&c.as_str()) {
            uniq.push(c);
        }
    }
    let joined = crate::json::slice16(&uniq.join(", "), 60);
    let what = label.map(str::to_string).unwrap_or(if joined.is_empty() { "command".into() } else { joined });
    eng.doc_mut().squash(mark, &format!("{prefix}{what}"));
    let after = snapshot(&eng.doc());
    let before_map: HashMap<u64, &Entity> = before.iter().map(|(k, v)| (*k, v)).collect();
    let after_ids: std::collections::HashSet<u64> = after.iter().map(|(k, _)| *k).collect();
    let (mut added, mut changed, mut removed) = (Vec::new(), Vec::new(), Vec::new());
    for (id, e) in &after {
        match before_map.get(id) {
            None => added.push(*id),
            Some(b) if *b != e => changed.push(*id),
            _ => {}
        }
    }
    for (id, _) in &before {
        if !after_ids.contains(id) {
            removed.push(*id);
        }
    }
    let undo = {
        let d = eng.doc();
        if d.undo_stack.len() > mark { d.undo_stack.last().map(|s| Value::String(s.label.clone())) } else { None }
    };
    // give the window its host back
    if let Some(h) = inner.borrow_mut().take() {
        *eng.host_mut() = h;
    }
    let printed = st.borrow().printed.clone();
    let err_line = printed.iter().find_map(|l| l.strip_prefix("Error: ").map(str::to_string));
    json!({
        "printed": printed, "commands": cmds, "added": added, "changed": changed, "removed": removed, "waiting": waiting,
        "undo": undo, "error": error.or(err_line),
    })
}

/// agents.js `undoSteps`: up to `steps` steps; only those labelled with `prefix` unless `any`.
pub fn undo_steps(doc: &mut Doc, steps: u64, any: bool, prefix: &str) -> Vec<Value> {
    let mut undone = Vec::new();
    for _ in 0..steps {
        let Some(top) = doc.undo_stack.last() else { break };
        if !any && !prefix.is_empty() && !top.label.starts_with(prefix) {
            break;
        }
        undone.push(doc.undo().map_or(Value::Null, Value::String));
    }
    undone
}

/// What the window knows about its 2D view (for `view_describe`).
#[derive(Debug, Clone, Copy)]
pub struct ViewInfo {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub zoom: f64,
}

/// `drawing_plot`'s arguments → plot.js options (agents.js plotPdf defaults: iso-a3, fitted, mono, pen 0.25).
pub fn plot_opts(a: &Value, date_ms: f64) -> cc_io_cad::plot::PlotOpts {
    let window = a.get("window").filter(|w| w.is_object()).map(|w| {
        let g = |k: &str| w.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
        cc_cad_geom::geom::Bounds { min_x: g("minX"), min_y: g("minY"), max_x: g("maxX"), max_y: g("maxY") }
    });
    cc_io_cad::plot::PlotOpts {
        layout: a.get("layout").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string),
        window,
        paper: a.get("paper").and_then(Value::as_str).unwrap_or("iso-a3").to_string(),
        portrait: a.get("portrait").and_then(Value::as_bool).unwrap_or(false),
        scale: a.get("scale").and_then(Value::as_str).map(str::to_string),
        mono: a.get("mono").and_then(Value::as_bool) != Some(false),
        pen: a.get("pen").and_then(Value::as_f64).filter(|p| *p != 0.0 && !p.is_nan()).unwrap_or(0.25),
        date_ms,
        deflate: true,
    }
}

/// The space shown: a sheet's name, or "model".
fn space_name(doc: &Doc) -> String {
    doc.layout().map_or_else(|| "model".to_string(), |l| l.name.clone())
}

/// CadCraft's live tools (agents.js `installAgents`) on the window's drawing and command line. `drawing_save`,
/// `drawing_plot` and `view_screenshot` are the window's own (they write files or need the GPU).
pub fn live_run(eng: &mut Engine, name: &str, a: &Value, allowed: &[String], view: Option<ViewInfo>) -> Result<Value, String> {
    const PREFIX: &str = "Assistant: ";
    match name {
        "app_info" => {
            let d = eng.doc();
            Ok(json!({
                "app": "cadcraft", "title": "CadCraft", "allowed": allowed, "drawing": d.meta.get("name").cloned().unwrap_or(Value::Null),
                "source": d.meta.get("source").filter(|v| js::truthy(Some(v))).cloned().unwrap_or(Value::Null), "unsaved": d.version != d.saved_version,
                "space": space_name(&d), "objects": d.count(), "busy": if eng.busy() { eng.running().map(Value::from).unwrap_or(Value::Null) } else { Value::Null },
            }))
        }
        "drawing_describe" => Ok(describe_drawing(&eng.doc(), None, None)),
        "objects_list" => list_objects(&eng.doc(), a),
        "object_get" => {
            let id = a.get("id").and_then(Value::as_u64).unwrap_or(0);
            object_in_full(&eng.doc(), id).ok_or_else(|| format!("There is no object {id}."))
        }
        "drawing_check" => check_drawing(&eng.doc(), a.get("rules").unwrap_or(&Value::Null)),
        "selection_get" => {
            let sel = eng.host().selection();
            let d = eng.doc();
            let ids: Vec<u64> = sel.into_iter().filter(|id| d.get(*id).is_some()).collect();
            let objects: Vec<Value> = ids
                .iter()
                .take(1000)
                .filter_map(|id| d.get(*id))
                .map(|e| json!({"id": e.id, "type": e.typ(), "layer": e.layer_str(), "description": describe(&d, e)}))
                .collect();
            Ok(json!({"count": ids.len(), "objects": objects}))
        }
        "view_describe" => {
            let d = eng.doc();
            let shown: Vec<String> = d.layers.values().filter(|l| l.on && !l.frozen).map(|l| l.name.clone()).collect();
            Ok(json!({
                "space": space_name(&d),
                "visible": view.map(|v| json!({"minX": num(v.min_x), "minY": num(v.min_y), "maxX": num(v.max_x), "maxY": num(v.max_y)})),
                "zoom": view.map(|v| num(v.zoom)), "panes": null, "layersShown": shown,
            }))
        }
        "command_run" => {
            let input = a.get("input").cloned().unwrap_or(Value::Null);
            let text: Vec<String> = match &input {
                Value::Array(l) => l.iter().map(|x| x.as_str().map_or_else(|| crate::json::stringify(x), str::to_string)).collect(),
                Value::String(s) => script_lines(s),
                _ => Vec::new(),
            };
            let echo = lang_en().t("agents.echo", &[("input", crate::json::slice16(&text.join(" · "), 300))]);
            eng.host_mut().print(&echo);
            let select: Option<Vec<u64>> = a.get("select").and_then(Value::as_array).map(|v| v.iter().filter_map(Value::as_u64).collect());
            let mut r = run_input(eng, &input, select.as_deref(), None, PREFIX);
            let mut counts = Map::new();
            for k in ["added", "changed", "removed"] {
                let n = r.get(k).and_then(Value::as_array).map_or(0, Vec::len);
                counts.insert(k.into(), json!(n));
                if let Some(arr) = r.get_mut(k).and_then(Value::as_array_mut) {
                    arr.truncate(2000);
                }
            }
            if let Some(o) = r.as_object_mut() {
                o.insert("counts".into(), Value::Object(counts));
            }
            Ok(r)
        }
        "selection_set" => {
            let ids: Vec<u64> =
                a.get("ids").and_then(Value::as_array).map(|v| v.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
            let bad: Vec<String> = {
                let d = eng.doc();
                ids.iter().filter(|id| d.get(**id).is_none()).map(u64::to_string).collect()
            };
            if !bad.is_empty() {
                let more = if bad.len() > 10 { ", …" } else { "" };
                return Err(format!(
                    "No objects {}{more} in the space shown.",
                    bad.iter().take(10).cloned().collect::<Vec<_>>().join(", ")
                ));
            }
            let mode = a.get("mode").and_then(Value::as_str).unwrap_or("replace");
            let mut s: Vec<u64> = if mode == "replace" { Vec::new() } else { eng.host().selection() };
            for id in ids {
                if mode == "remove" {
                    s.retain(|x| *x != id);
                } else if !s.contains(&id) {
                    s.push(id);
                }
            }
            eng.host_mut().set_selection(&s);
            Ok(json!({"count": s.len()}))
        }
        "undo" => {
            let steps = a.get("steps").and_then(Value::as_u64).unwrap_or(1);
            let any = a.get("any").and_then(Value::as_bool).unwrap_or(false);
            Ok(json!({"undone": undo_steps(&mut eng.doc_mut(), steps, any, PREFIX)}))
        }
        "drawing_plot" => Err("drawing_plot writes a file: the window runs it (live_plot).".into()),
        "view_screenshot" => Err("A picture of the view is not available in this version of CadCraft yet.".into()),
        other => Err(format!("{other} is not one of CadCraft's tools.")),
    }
}
