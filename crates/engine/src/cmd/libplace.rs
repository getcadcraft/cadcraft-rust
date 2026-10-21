//! libplace.js: a library item (docs/LIBRARY-SPEC.md §1, "cadcraft-library-item" format 1) as a CadCraft block
//! (INSERT, option Library): `LIB_<slug>_v<version>`, drawn in the drawing's units (items are in mm), with the item
//! number (CC-ITEM, asked when placed), the quantity and the library fields as attributes. Item data is untrusted:
//! every number is checked (finite, at most 1e7), unknown shapes are skipped and counted, and at most 50 000 objects
//! are taken.

use std::f64::consts::PI;

use cc_cad::Doc;
use cc_cad::doc::Block;
use cc_cad::entity::{Entity, Obj};
use cc_cad::js;
use cc_cad_geom::geom::{norm_angle, norm_tag};
use cc_cad_geom::pick::{empty_bounds, entity_bounds, grow_pt, valid_bounds};
use serde_json::{Map, Value, json};
use unicode_normalization::UnicodeNormalization;

use crate::cmd::block::one_line;
use crate::cmd::xref::mm_per_unit;
use crate::lang::Lang;

pub const TAG_ITEM: &str = "CC-ITEM";
const MAX_ENTITIES: usize = 50_000;
const LIM: f64 = 1e7;

fn fin(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|x| x.is_finite() && x.abs() <= LIM)
}

/// `str(v, n)`: text, control characters as one space each run, trimmed, at most n UTF-16 units.
fn str_(v: Option<&Value>, n: usize) -> String {
    let s = match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(x) => js::to_js_string(x),
    };
    let mut out = String::with_capacity(s.len());
    let mut in_ctl = false;
    for ch in s.chars() {
        if ch <= '\u{1f}' || ch == '\u{7f}' {
            if !in_ctl {
                out.push(' ');
            }
            in_ctl = true;
        } else {
            out.push(ch);
            in_ctl = false;
        }
    }
    js::slice16(js::trim(&out), n)
}

/// `slugify(s, max)`.
pub fn slugify(s: &str, max: usize) -> String {
    let low: String = s.nfkd().filter(|c| !('\u{300}'..='\u{36f}').contains(c)).collect::<String>().to_lowercase();
    let mut out = String::new();
    let mut dash = false;
    for ch in low.chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            out.push(ch);
            dash = false;
        } else if !dash {
            out.push('-');
            dash = true;
        }
    }
    let t = out.trim_matches('-');
    let cut: String = t.chars().take(max).collect();
    let cut = cut.trim_end_matches('-');
    if cut.is_empty() { "item".into() } else { cut.to_string() }
}

/// `Number.isInteger(v) && v > 0 ? v : 1`
fn version_of(item: &Value) -> String {
    match item.get("version").and_then(Value::as_f64) {
        Some(v) if v > 0.0 && v.fract() == 0.0 && v.is_finite() => js::num_to_string(v),
        _ => "1".into(),
    }
}

/// `libBlockName(item, slug)`.
pub fn lib_block_name(item: &Value, slug: Option<&str>) -> String {
    let src = match slug.filter(|s| !s.is_empty()) {
        Some(s) => s.to_string(),
        None => [item.get("name"), item.get("uuid")]
            .into_iter()
            .flatten()
            .find(|v| js::truthy(Some(v)))
            .map(js::to_js_string)
            .unwrap_or_default(),
    };
    format!("LIB_{}_v{}", slugify(&src, 20), version_of(item))
}

fn is_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36 && b.iter().enumerate().all(|(i, c)| if matches!(i, 8 | 13 | 18 | 23) { *c == b'-' } else { c.is_ascii_hexdigit() })
}

/// `itemProblem(item)`: None, or a plain sentence why it cannot be placed.
pub fn item_problem(item: &Value, lang: &Lang) -> Option<String> {
    let Some(o) = item.as_object() else { return Some(lang.t("lib.place.not_library_item", &[])) };
    if o.get("format").and_then(Value::as_str) != Some("cadcraft-library-item") {
        return Some(lang.t("lib.place.not_cadcraft_library_item", &[]));
    }
    if o.get("format_version").and_then(Value::as_f64) != Some(1.0) {
        return Some(lang.t("lib.place.format_version", &[("version", str_(o.get("format_version"), 20))]));
    }
    let Some(ents) = o.get("plan").and_then(|p| p.get("entities")).and_then(Value::as_array) else {
        return Some(lang.t("lib.place.library_item_has_no", &[]));
    };
    if ents.len() > MAX_ENTITIES {
        return Some(lang.t("lib.place.library_item_has_more", &[]));
    }
    if let Some(u) = o.get("uuid").filter(|u| !u.is_null())
        && !is_uuid(&js::to_js_string(u))
    {
        return Some(lang.t("lib.place.library_item_has_malformed", &[]));
    }
    None
}

fn color_of(c: Option<&Value>) -> Value {
    match c {
        Some(Value::Number(n)) => match n.as_f64() {
            Some(0.0) => json!(0),
            Some(v) if v.fract() == 0.0 && (1.0..=255.0).contains(&v) => json!(v),
            _ => Value::Null,
        },
        Some(Value::String(s))
            if s.len() == 7 && s.starts_with('#') && s.get(1..).is_some_and(|h| h.chars().all(|c| c.is_ascii_hexdigit())) =>
        {
            Value::String(s.to_lowercase())
        }
        _ => Value::Null,
    }
}

fn rad(d: f64) -> f64 {
    d * PI / 180.0
}

/// `o[a] ?? o['c' + a]`
fn xy_of(o: &Value, a: &str) -> Option<f64> {
    match o.get(a) {
        Some(v) if !v.is_null() => fin(Some(v)),
        _ => fin(o.get(format!("c{a}").as_str())),
    }
}

/// `convert(e, k)`: one plan object (mm, degrees) → a CadCraft object in drawing units (×k), or None.
fn convert(e: &Value, k: f64) -> Option<Value> {
    let o = e.as_object()?;
    let layer = {
        let l = str_(o.get("layer"), 255);
        if l.is_empty() { "0".to_string() } else { l }
    };
    let color = color_of(o.get("color"));
    let p = |v: f64| v * k;
    let f = |key: &str| fin(o.get(key));
    let ty = o.get("type").and_then(Value::as_str).unwrap_or("");
    let mut out = match ty {
        "line" => json!({ "type": "line", "x1": p(f("x1")?), "y1": p(f("y1")?), "x2": p(f("x2")?), "y2": p(f("y2")?) }),
        "circle" => {
            let (x, y, r) = (xy_of(e, "x")?, xy_of(e, "y")?, f("r")?);
            if !(r > 0.0) {
                return None;
            }
            json!({ "type": "circle", "cx": p(x), "cy": p(y), "r": p(r) })
        }
        "arc" => {
            let (x, y, r, a0, a1) = (xy_of(e, "x")?, xy_of(e, "y")?, f("r")?, f("a0")?, f("a1")?);
            if !(r > 0.0) {
                return None;
            }
            json!({ "type": "arc", "cx": p(x), "cy": p(y), "r": p(r), "a0": norm_angle(rad(a0)), "a1": norm_angle(rad(a1)) })
        }
        "poly" => {
            let pts = o.get("pts").and_then(Value::as_array)?;
            if pts.len() < 2 || pts.len() > 100_000 {
                return None;
            }
            let mut out = Vec::with_capacity(pts.len());
            for q in pts {
                let (x, y, b) = match q {
                    Value::Array(a) => (a.first(), a.get(1), a.get(2)),
                    _ => (q.get("x"), q.get("y"), q.get("b")),
                };
                let (x, y) = (fin(x)?, fin(y)?);
                let b = fin(b).filter(|b| b.abs() < 1e3).unwrap_or(0.0);
                out.push(json!({ "x": p(x), "y": p(y), "b": b }));
            }
            json!({ "type": "poly", "pts": out, "closed": js::truthy(o.get("closed")) })
        }
        "ellipse" => {
            let (x, y, mx, my, ratio) = (xy_of(e, "x")?, xy_of(e, "y")?, f("mx")?, f("my")?, f("ratio")?);
            if !(ratio > 0.0) || !(js::hypot(mx, my) > 0.0) {
                return None;
            }
            let t0 = f("t0").map_or(0.0, rad);
            let t1 = f("t1").map_or(2.0 * PI, rad);
            json!({ "type": "ellipse", "cx": p(x), "cy": p(y), "mx": p(mx), "my": p(my), "ratio": ratio, "t0": t0, "t1": t1 })
        }
        "text" => {
            let v = match o.get("value") {
                Some(v) if !v.is_null() => Some(v),
                _ => o.get("s"),
            };
            let s = str_(v, 500);
            if s.is_empty() {
                return None;
            }
            let (x, y) = (f("x")?, f("y")?);
            let h = f("h").filter(|h| *h > 0.0).unwrap_or(100.0);
            let ha = o.get("ha").and_then(Value::as_str).filter(|v| ["left", "center", "right"].contains(v)).unwrap_or("left");
            let va =
                o.get("va").and_then(Value::as_str).filter(|v| ["baseline", "bottom", "middle", "top"].contains(v)).unwrap_or("baseline");
            json!({ "type": "text", "x": p(x), "y": p(y), "h": p(h), "rot": norm_angle(rad(f("rot").unwrap_or(0.0))), "s": s, "ha": ha, "va": va })
        }
        "point" => json!({ "type": "point", "x": p(f("x")?), "y": p(f("y")?) }),
        "hatch" => {
            let ls = o.get("loops").and_then(Value::as_array).filter(|l| !l.is_empty())?;
            let mut loops = Vec::with_capacity(ls.len());
            for l in ls {
                let l = l.as_array()?;
                let flat: Vec<&Value> = if l.first().is_some_and(Value::is_array) {
                    l.iter().flat_map(|q| q.as_array().map(|a| a.iter().collect::<Vec<_>>()).unwrap_or_else(|| vec![q])).collect()
                } else {
                    l.iter().collect()
                };
                if flat.len() < 6 || !flat.len().is_multiple_of(2) {
                    return None;
                }
                let mut nums = Vec::with_capacity(flat.len());
                for v in flat {
                    nums.push(p(fin(Some(v))?));
                }
                loops.push(nums);
            }
            json!({ "type": "hatch", "solid": true, "loops": loops })
        }
        _ => return None,
    };
    if let Some(m) = out.as_object_mut() {
        m.insert("layer".into(), Value::String(layer));
        m.insert("color".into(), color);
    }
    Some(out)
}

/// geom.js `attdef(t, opts)` (serde values).
#[allow(clippy::too_many_arguments)]
fn attdef(x: f64, y: f64, h: f64, ha: &str, va: &str, tag: &str, prompt: &str, def: &str, hidden: bool) -> Value {
    let mut d = Map::new();
    d.insert("tag".into(), Value::String(norm_tag(tag)));
    d.insert("prompt".into(), Value::String(prompt.into()));
    d.insert("def".into(), Value::String(one_line(def)));
    d.insert("x".into(), json!(if x.is_nan() { 0.0 } else { x }));
    d.insert("y".into(), json!(if y.is_nan() { 0.0 } else { y }));
    d.insert("h".into(), json!(if h > 0.0 { h } else { 2.5 }));
    d.insert("rot".into(), json!(0));
    d.insert("ha".into(), Value::String(ha.into()));
    d.insert("va".into(), Value::String(va.into()));
    d.insert("wf".into(), json!(1));
    d.insert("layer".into(), Value::String("0".into()));
    d.insert("color".into(), Value::Null);
    if hidden {
        d.insert("invisible".into(), Value::Bool(true));
        d.insert("preset".into(), Value::Bool(true));
    }
    Value::Object(d)
}

/// `Number(v.toPrecision(10))` as text.
fn prec10(v: f64) -> String {
    js::num_to_string(js::str_to_number(&js::to_exponential(v, 9)))
}

/// `paramAttrs(params)`.
fn param_attrs(params: Option<&Value>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(p) = params.and_then(Value::as_object) else { return out };
    for (k, tag) in [("width", "CC-WIDTH-MM"), ("depth", "CC-DEPTH-MM"), ("height", "CC-HEIGHT-MM"), ("weight_kg", "CC-WEIGHT-KG")] {
        if let Some(v) = fin(p.get(k)).filter(|v| *v != 0.0) {
            out.push((tag.to_string(), prec10(v)));
        }
    }
    for (group, prefix) in [("elec", "CC-ELEC-"), ("plumb", "CC-PLUMB-")] {
        // Spec gap: JS lists integer-like keys first; items use word keys, kept here in their order
        if let Some(g) = p.get(group).and_then(Value::as_object) {
            for (k, v) in g {
                let s = str_(Some(v), 120);
                if !s.is_empty() && !k.is_empty() && k.len() <= 20 && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    out.push((format!("{prefix}{}", k.to_uppercase()), s));
                }
            }
        }
    }
    let r = str_(p.get("remarks"), 500);
    if !r.is_empty() {
        out.push(("CC-REMARKS".into(), r));
    }
    out
}

const PART_ATTR_TAGS: [&str; 5] = ["CC-PART-TYPE", "CC-PART-STD", "CC-PART-SIZE", "CC-PART-LENGTH", "CC-PART-MATERIAL"];

fn part_attrs(src: Option<&Value>) -> Vec<(String, String)> {
    let Some(o) = src.and_then(Value::as_object) else { return Vec::new() };
    PART_ATTR_TAGS
        .iter()
        .filter_map(|t| o.get(*t).and_then(Value::as_str).map(|s| ((*t).to_string(), str_(Some(&Value::String(s.into())), 255))))
        .collect()
}

/// `attributionOf(item)`.
pub fn attribution_of(item: &Value) -> String {
    let lic = item.get("license");
    let a = str_(lic.and_then(|l| l.get("attribution")), 500);
    if !a.is_empty() {
        return a;
    }
    let holder = str_(lic.and_then(|l| l.get("holder")), 200);
    let id = str_(lic.and_then(|l| l.get("id")), 40);
    format!(
        "“{}”{}{}",
        str_(item.get("name"), 200),
        if holder.is_empty() { String::new() } else { format!(" by {holder}") },
        if id.is_empty() { String::new() } else { format!(", {id}") }
    )
}

/// What `blockFromItem` makes.
pub struct Made {
    pub name: String,
    pub block: Block,
    pub layers: Vec<Value>,
    pub skipped: usize,
    /// the drawing's units, or "" when it has none (taken as mm)
    pub units: String,
}

/// `blockFromItem(doc, item, { slug, library })`.
pub fn block_from_item(doc: &Doc, item: &Value, slug: Option<&str>, lang: &Lang) -> Result<Made, String> {
    if let Some(why) = item_problem(item, lang) {
        return Err(why);
    }
    let units = doc.meta.get("units").cloned();
    let mm = mm_per_unit(units.as_ref()).filter(|m| *m != 0.0);
    let k = mm.map_or(1.0, |m| 1.0 / m);
    let name = lib_block_name(item, slug);
    let mut entities: Vec<Entity> = Vec::new();
    let mut skipped = 0usize;
    for e in item.get("plan").and_then(|p| p.get("entities")).and_then(Value::as_array).map_or(&[][..], Vec::as_slice) {
        match convert(e, k) {
            Some(c) => entities.push(Entity::from_value(&c)),
            None => skipped = skipped.saturating_add(1),
        }
    }
    let mut layers = Vec::new();
    for l in item.get("plan").and_then(|p| p.get("layers")).and_then(Value::as_array).map_or(&[][..], Vec::as_slice).iter().take(1000) {
        let n = str_(l.get("name"), 255);
        if !n.is_empty() {
            let c = color_of(l.get("color"));
            layers.push(json!({ "name": n, "color": if js::truthy(Some(&c)) { c } else { json!(7) } }));
        }
    }
    let mut b = empty_bounds();
    for e in &entities {
        let eb = entity_bounds(e, None);
        if valid_bounds(&eb) {
            grow_pt(&mut b, eb.min_x, eb.min_y);
            grow_pt(&mut b, eb.max_x, eb.max_y);
        }
    }
    let (min_x, min_y, max_x, max_y) =
        if valid_bounds(&b) { (b.min_x, b.min_y, b.max_x, b.max_y) } else { (0.0, 0.0, 600.0 * k, 600.0 * k) };
    let small = {
        let s = js::min(max_x - min_x, max_y - min_y);
        if s == 0.0 || s.is_nan() { 600.0 * k } else { s }
    };
    let h = js::min(js::max(small * 0.15, 20.0 * k), 150.0 * k);
    let (cx, cy) = ((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);
    let lic = item.get("license");
    let pv = item.get("provenance");
    let lib = {
        let v = pv.and_then(|p| p.get("library")).filter(|v| js::truthy(Some(v)));
        str_(Some(v.unwrap_or(&Value::String("librarycraft".into()))), 120)
    };
    let ver = match item.get("version").and_then(Value::as_f64) {
        Some(v) if v.fract() == 0.0 && v.is_finite() => js::num_to_string(v),
        _ => "1".into(),
    };
    let mut fields: Vec<(String, String)> = vec![
        ("CC-QTY".into(), "1".into()),
        ("CC-ITEM-UUID".into(), str_(item.get("uuid"), 36)),
        ("CC-ITEM-VER".into(), ver),
        ("CC-ITEM-NAME".into(), str_(item.get("name"), 255)),
        ("CC-LIBRARY".into(), lib),
        ("CC-SHA256".into(), str_(pv.and_then(|p| p.get("sha256")), 64)),
        ("CC-LICENSE".into(), str_(lic.and_then(|l| l.get("id")), 40)),
        ("CC-ATTRIB".into(), attribution_of(item)),
        ("CC-LICENSE-URL".into(), str_(lic.and_then(|l| l.get("url")), 255)),
        ("CC-MAKER".into(), str_(item.get("maker"), 200)),
        ("CC-MODEL".into(), str_(item.get("model"), 200)),
        ("CC-CATEGORY".into(), str_(item.get("category"), 120)),
        ("CC-KIND".into(), str_(item.get("kind"), 40)),
        ("CC-SOURCE-FILE".into(), str_(pv.and_then(|p| p.get("source_file")), 255)),
    ];
    fields.extend(param_attrs(item.get("params")));
    fields.extend(part_attrs(item.get("source_attrs")));
    let mut attdefs = vec![attdef(cx, cy, h, "center", "middle", TAG_ITEM, &lang.t("lib.place.item_number", &[]), "", false)];
    for (i, (tag, v)) in fields.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let fi = i as f64;
        attdefs.push(attdef(min_x, min_y - h * (1.6 + fi * 1.4), h * 0.6, "left", "baseline", tag, tag, v, true));
    }
    let mut block = Block::new(&name, cc_geom::P2::new(0.0, 0.0), entities);
    block.attdefs = Some(attdefs);
    Ok(Made {
        name,
        block,
        layers,
        skipped,
        units: if mm.is_some() { units.as_ref().map(js::to_js_string).unwrap_or_default() } else { String::new() },
    })
}

/// `libraryFieldsOfBlock(blk)` → (uuid, name, version).
fn library_fields(blk: &Block) -> Option<(String, String, f64)> {
    let d = |tag: &str| {
        blk.attdefs
            .iter()
            .flatten()
            .find(|x| x.get("tag").and_then(Value::as_str) == Some(tag))
            .and_then(|x| x.get("def"))
            .filter(|v| js::truthy(Some(v)))
            .map(js::to_js_string)
    };
    let (uuid, name) = (d("CC-ITEM-UUID"), d("CC-ITEM-NAME"));
    if uuid.is_none() && name.is_none() {
        return None;
    }
    let v = crate::cmd::qselect::parse_int(&d("CC-ITEM-VER").unwrap_or_default());
    Some((uuid.unwrap_or_default(), name.unwrap_or_default(), if v == 0.0 || v.is_nan() { 1.0 } else { v }))
}

/// What `ensureLibraryBlock` returns.
pub struct Ensured {
    pub name: String,
    pub reused: bool,
    pub skipped: usize,
    pub units: String,
}

/// `ensureLibraryBlock(doc, item, { slug })` inside a change: define it, or reuse the same uuid@version.
pub fn ensure_library_block(
    doc: &mut Doc,
    item: &Value,
    slug: Option<&str>,
    lang: &Lang,
) -> Result<Result<Ensured, String>, cc_cad::DocError> {
    let made = match block_from_item(doc, item, slug, lang) {
        Ok(m) => m,
        Err(e) => return Ok(Err(e)),
    };
    let uuid = item.get("uuid").filter(|v| js::truthy(Some(v))).map(js::to_js_string).unwrap_or_default();
    // `item.version || 1`, compared strictly with a number
    let want_ver: Option<f64> = match item.get("version") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => Some(1.0),
        Some(Value::Number(n)) => n.as_f64().map(|v| if v == 0.0 { 1.0 } else { v }),
        Some(Value::String(s)) if s.is_empty() => Some(1.0),
        _ => None,
    };
    let item_name = str_(item.get("name"), 255);
    let mut name = made.name.clone();
    let mut n = 2u64;
    while let Some(blk) = doc.blocks.get(&name) {
        if let Some((fu, fname, fv)) = library_fields(blk)
            && fu == uuid
            && want_ver == Some(fv)
            && (!fu.is_empty() || fname == item_name)
        {
            return Ok(Ok(Ensured { name, reused: true, skipped: made.skipped, units: made.units }));
        }
        name = match made.name.rfind("_v") {
            Some(i) if made.name.get(i.saturating_add(2)..).is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit())) => {
                format!("{}-{n}{}", made.name.get(..i).unwrap_or(""), made.name.get(i..).unwrap_or(""))
            }
            _ => made.name.clone(),
        };
        n = n.saturating_add(1);
        if n > 100_000 {
            break;
        }
    }
    for l in &made.layers {
        let ln = l.get("name").and_then(Value::as_str).unwrap_or("");
        if !doc.layers.contains_key(ln) {
            doc.add_layer(l);
        }
    }
    let mut b = made.block;
    b.name.clone_from(&name);
    doc.define_block_owned(b)?;
    Ok(Ok(Ensured { name, reused: false, skipped: made.skipped, units: made.units }))
}

/// attrs.js `nextNumber(doc, 'CC-ITEM')`: the highest whole item number used + 1.
pub fn next_number(doc: &Doc) -> String {
    let mut max = 0.0f64;
    for e in doc.every_entity() {
        if let cc_cad::entity::Geo::Insert(i) = &e.geo
            && let Some(v) = i.attrs.as_ref().and_then(|a| a.get(TAG_ITEM)).filter(|v| !v.is_null())
        {
            let s = js::to_js_string(v);
            let t = js::trim(&s);
            if !t.is_empty() && t.chars().all(|c| c.is_ascii_digit()) {
                max = js::max(max, crate::cmd::qselect::parse_int(&s));
            }
        }
    }
    js::num_to_string(max + 1.0)
}

/// `libraryAttrs(blk, n)`: every default, CC-ITEM as given.
pub fn library_attrs(blk: Option<&Block>, n: &str) -> Obj {
    let mut a = crate::cmd::block::default_attrs(blk);
    a.insert(TAG_ITEM.into(), Value::String(n.into()));
    a
}

/// One library item a drawing uses (`libraryUses`): grouped by uuid@version.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LibUse {
    pub uuid: String,
    pub version: f64,
    pub block: String,
    pub name: String,
    pub library: String,
    pub license: String,
    pub attribution: String,
    pub license_url: String,
    pub maker: String,
    pub model: String,
    pub category: String,
    pub count: usize,
    pub quantity: f64,
    pub numbers: Vec<String>,
}

/// `libraryUses(doc)`: every library item the drawing's references use (model and sheets).
pub fn library_uses(doc: &Doc) -> Vec<LibUse> {
    let mut out: Vec<LibUse> = Vec::new();
    for e in doc.every_entity() {
        let cc_cad::entity::Geo::Insert(i) = &e.geo else { continue };
        let Some(a) = i.attrs.as_ref() else { continue };
        let g = |k: &str| a.get(k).filter(|v| js::truthy(Some(v))).map(js::to_js_string).unwrap_or_default();
        let uuid = g("CC-ITEM-UUID");
        if uuid.is_empty() {
            continue;
        }
        let v = crate::cmd::qselect::parse_int(&g("CC-ITEM-VER"));
        let version = if v == 0.0 || v.is_nan() { 1.0 } else { v };
        let k = match out.iter().position(|u| u.uuid == uuid && u.version == version) {
            Some(k) => k,
            None => {
                out.push(LibUse {
                    uuid: uuid.clone(),
                    version,
                    block: i.block.clone(),
                    name: g("CC-ITEM-NAME"),
                    library: g("CC-LIBRARY"),
                    license: g("CC-LICENSE"),
                    attribution: g("CC-ATTRIB"),
                    license_url: g("CC-LICENSE-URL"),
                    maker: g("CC-MAKER"),
                    model: g("CC-MODEL"),
                    category: g("CC-CATEGORY"),
                    ..LibUse::default()
                });
                out.len().saturating_sub(1)
            }
        };
        let Some(u) = out.get_mut(k) else { continue };
        u.count = u.count.saturating_add(1);
        let q = js::str_to_number(&g("CC-QTY"));
        u.quantity += if q > 0.0 { q } else { 1.0 };
        let n = a.get(TAG_ITEM).filter(|v| !v.is_null()).map(js::to_js_string).unwrap_or_default();
        let n = js::trim(&n).to_string();
        if !n.is_empty() && !u.numbers.contains(&n) {
            u.numbers.push(n);
        }
    }
    for u in &mut out {
        u.numbers.sort_by(|x, y| crate::cmd::qselect::locale_cmp(x, y));
    }
    out.sort_by(|x, y| {
        let fx = x.numbers.first().map_or("~", String::as_str);
        let fy = y.numbers.first().map_or("~", String::as_str);
        crate::cmd::qselect::locale_cmp(fx, fy).then_with(|| crate::cmd::qselect::locale_cmp_plain(&x.name, &y.name))
    });
    out
}

/// `creditsText(uses)`: one line per item.
pub fn credits_text(uses: &[LibUse]) -> String {
    uses.iter()
        .map(|u| {
            format!(
                "{}{}{}{}",
                if u.numbers.is_empty() { String::new() } else { format!("Item {}: ", u.numbers.join(", ")) },
                u.attribution,
                if !u.license.is_empty() && !u.attribution.contains(&u.license) { format!(" ({})", u.license) } else { String::new() },
                if u.license_url.is_empty() { String::new() } else { format!(" {}", u.license_url) }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `creditsCsv(uses)`: BOM + CRLF; cells starting = + - @ get a leading '.
pub fn credits_csv(uses: &[LibUse], lang: &Lang) -> String {
    let cell = |v: &str| {
        let mut s = v.to_string();
        if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
            s.insert(0, '\'');
        }
        if s.contains(['"', ',', '\r', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s }
    };
    let t = |k: &str| lang.t(k, &[]);
    let mut rows: Vec<Vec<String>> = vec![
        [t("lib.place.item_numbers")]
            .into_iter()
            .chain(["name", "maker", "model", "category", "placed", "quantity"].iter().map(|k| t(&format!("lib.place.col.{k}"))))
            .chain([t("lib.place.item_id")])
            .chain(["version", "license", "attribution"].iter().map(|k| t(&format!("lib.place.col.{k}"))))
            .chain([t("lib.place.license_link"), t("lib.place.col.library")])
            .collect(),
    ];
    for u in uses {
        rows.push(vec![
            u.numbers.join(" "),
            u.name.clone(),
            u.maker.clone(),
            u.model.clone(),
            u.category.clone(),
            u.count.to_string(),
            js::num_to_string(u.quantity),
            u.uuid.clone(),
            js::num_to_string(u.version),
            u.license.clone(),
            u.attribution.clone(),
            u.license_url.clone(),
            u.library.clone(),
        ]);
    }
    format!("\u{feff}{}\r\n", rows.iter().map(|r| r.iter().map(|c| cell(c)).collect::<Vec<_>>().join(",")).collect::<Vec<_>>().join("\r\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_and_names() {
        assert_eq!(slugify("Évier double — 1200 mm", 20), "evier-double-1200-mm");
        assert_eq!(slugify("***", 20), "item");
        assert_eq!(lib_block_name(&json!({ "name": "Hand Sink", "version": 3 }), None), "LIB_hand-sink_v3");
        assert_eq!(lib_block_name(&json!({ "name": "X", "version": 2.5 }), Some("my slug")), "LIB_my-slug_v1");
    }

    #[test]
    fn hostile_items_are_refused_or_trimmed() {
        let l = Lang::new(&Value::Null);
        let d = Doc::new();
        for bad in [json!(null), json!([]), json!({ "format": "x" }), json!({ "format": "cadcraft-library-item", "format_version": 2 })] {
            assert!(block_from_item(&d, &bad, None, &l).is_err());
        }
        let it = json!({ "format": "cadcraft-library-item", "format_version": 1, "name": "T", "plan": { "entities": [
            { "type": "line", "x1": 0, "y1": 0, "x2": 1e9, "y2": 0 }, { "type": "circle", "x": 0, "y": 0, "r": -1 },
            { "type": "poly", "pts": [[0, 0], [1, 1, 1e9]] }, { "type": "hatch", "loops": [[[0, 0], [1, 0], [1, 1]]] }, { "type": "zz" } ] } });
        let m = block_from_item(&d, &it, None, &l).unwrap();
        assert_eq!((m.block.entities.len(), m.skipped), (2, 3));
    }
}
