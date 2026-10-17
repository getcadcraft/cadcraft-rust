//! scene.js — the plan drawing as plain linework and labels in model coordinates; the screen, the DXF, the DWG and
//! the PDF all draw from it. Items are kept as the JSON objects the JS makes (point lists are the objects given).

use cc_civil::alignment::{Alignment, Element};
use cc_civil::i18n::t;
use cc_civil::js::{J, JsMath, hypot, max, num_to_string, round};
use cc_civil::pipes::{Analysis, Network};
use cc_civil::tin::{Contour, Surface};
use cc_civil::tr;
use cc_civil::units::{fixed, format_station};
use serde_json::{Value, json};
use std::f64::consts::PI;

/// `LAYERS`: (name, token, ACI, label key).
pub const LAYERS: [(&str, &str, i64, &str); 24] = [
    ("C-TOPO-MINR", "minor", 8, "scene.layer.minr"),
    ("C-TOPO-MAJR", "major", 7, "scene.layer.majr"),
    ("C-TOPO-TEXT", "major", 7, "scene.layer.topotext"),
    ("C-TOPO-TINN", "tin", 8, "scene.layer.tin"),
    ("C-TOPO-BRKL", "orange", 30, "scene.layer.brkl"),
    ("C-TOPO-BNDY", "ink2", 7, "scene.layer.bndy"),
    ("C-TOPO-SLOP", "cyan", 4, "scene.layer.slop"),
    ("V-NODE", "green", 3, "scene.layer.node"),
    ("V-NODE-TEXT", "green", 3, "scene.layer.nodetext"),
    ("V-NODE-LINE", "green", 3, "scene.layer.figure"),
    ("C-ROAD-CNTR", "red", 1, "scene.layer.cntr"),
    ("C-ROAD-STAN", "red", 1, "scene.layer.stan"),
    ("C-ROAD-CORR", "ochre", 2, "scene.layer.corr"),
    ("C-ROAD-SCTN", "ink2", 8, "scene.layer.sctn"),
    ("C-GRAD-PADS", "blue", 5, "scene.layer.pads"),
    ("C-GRAD-DAYL", "magenta", 6, "scene.layer.dayl"),
    ("C-GRAD-SLOP", "magenta", 6, "scene.layer.gradslop"),
    ("C-GRAD-FEAT", "green", 3, "scene.layer.feat"),
    ("V-PROP", "ink2", 7, "scene.layer.prop"),
    ("V-PROP-TEXT", "ink2", 7, "scene.layer.proptext"),
    ("C-STRM-PIPE", "blue", 5, "scene.layer.pipe"),
    ("C-STRM-STRC", "blue", 5, "scene.layer.strc"),
    ("C-STRM-TEXT", "blue", 5, "scene.layer.strmtext"),
    ("C-STRM-PROB", "red", 1, "scene.layer.prob"),
];
/// The layers as JSON (`{ name, token, aci, label }`).
pub fn layers_json() -> Value {
    Value::Array(LAYERS.iter().map(|(n, tk, a, l)| json!({ "name": n, "token": tk, "aci": a, "label": t(l, &[]) })).collect())
}

/// A pad to draw.
pub struct ScenePad {
    pub name: String,
    /// the pad's corner objects as given
    pub pts: Vec<Value>,
    pub z: f64,
    pub daylight: Vec<Value>,
    /// rays: (x0, y0, x, y, d)
    pub rays: Vec<(f64, f64, f64, f64, f64)>,
}
/// A corridor to draw: its lines (name → point objects) and sections' L / R lists.
pub struct SceneCorridor {
    pub lines: Vec<(String, Vec<Value>)>,
    pub sections: Vec<(Vec<Value>, Vec<Value>)>,
}
/// An alignment to draw.
pub struct SceneAlignment<'a> {
    pub name: String,
    pub alignment: Option<&'a Alignment>,
    pub pis: Vec<Value>,
}
/// `planScene`'s state.
pub struct SceneState<'a> {
    pub units: String,
    pub points: Vec<cc_civil::points::Pt>,
    pub surface: Option<&'a Surface>,
    pub contours: Option<(f64, f64, Vec<Contour>)>,
    pub breaklines: Vec<Vec<Value>>,
    pub alignments: Vec<SceneAlignment<'a>>,
    pub pads: Vec<ScenePad>,
    pub corridors: Vec<SceneCorridor>,
    pub network: Option<(Network, Option<Analysis>)>,
}
/// `show` switches (None = the default).
#[derive(Default, Clone)]
pub struct Show {
    pub triangles: Option<bool>,
    pub contours: Option<bool>,
    pub labels: Option<bool>,
    pub breaklines: Option<bool>,
    pub outline: Option<bool>,
    pub points: Option<bool>,
    pub point_labels: Option<bool>,
    pub slopes: Option<bool>,
    pub alignments: Option<bool>,
    pub pads: Option<bool>,
    pub pipes: Option<bool>,
    pub point_label_text: Option<String>,
}

fn xy(x: f64, y: f64) -> Value {
    json!({ "x": x.j(), "y": y.j() })
}
#[allow(clippy::too_many_arguments)]
fn text(layer: &str, x: f64, y: f64, rot: f64, mm: f64, align: &str, middle: bool, s: String, mask: bool) -> Value {
    let mut o = json!({ "layer": layer, "kind": "text", "x": x.j(), "y": y.j(), "rot": rot.j(), "mm": mm.j(), "align": align, "text": s });
    if let Some(m) = o.as_object_mut() {
        if middle {
            m.insert("middle".into(), Value::Bool(true));
        }
        if mask {
            m.insert("mask".into(), Value::Bool(true));
        }
    }
    o
}
fn pxy(v: &Value) -> (f64, f64) {
    (cc_civil::js::numf(v.get("x")), cc_civil::js::numf(v.get("y")))
}

#[allow(clippy::arithmetic_side_effects)] // bounded counters and indices (page numbers, padding, digit math)
fn decimals_of(v: f64) -> usize {
    let s = num_to_string(v);
    match s.find('.') {
        None => 0,
        Some(i) => (s.len() - i - 1).min(3),
    }
}
fn centroid(pts: &[Value]) -> (f64, f64) {
    let (mut x, mut y) = (0.0, 0.0);
    for p in pts {
        let (a, b) = pxy(p);
        x += a;
        y += b;
    }
    (x / pts.len() as f64, y / pts.len() as f64)
}

/// `labelsAlong(pts, spacing, minLen)` → (x, y, rot).
pub fn labels_along(pts: &[(f64, f64)], spacing: f64, min_len: f64) -> Vec<(f64, f64, f64)> {
    let mut out = Vec::new();
    let mut total = 0.0;
    for w in pts.windows(2) {
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
        total += hypot(b.0 - a.0, b.1 - a.1);
    }
    if total < min_len {
        return out;
    }
    let n = max(1.0, (total / spacing).floor());
    let step = total / n;
    let mut want = step / 2.0;
    let mut acc = 0.0;
    for w in pts.windows(2) {
        if (out.len() as f64) >= n {
            break;
        }
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
        let l = hypot(b.0 - a.0, b.1 - a.1);
        let mut guard = 0u32;
        while acc + l >= want && (out.len() as f64) < n && guard < 1_000_000 {
            let tt = if l != 0.0 { (want - acc) / l } else { 0.0 };
            let mut rot = (b.1 - a.1).jatan2(b.0 - a.0);
            if rot > PI / 2.0 {
                rot -= PI;
            } else if rot < -PI / 2.0 {
                rot += PI;
            }
            out.push((a.0 + (b.0 - a.0) * tt, a.1 + (b.1 - a.1) * tt, rot));
            want += step;
            guard = guard.saturating_add(1);
        }
        acc += l;
    }
    out
}

/// `planScene(state, { mmPerUnit, show })` → items (the `layers` are [`LAYERS`]).
#[allow(clippy::arithmetic_side_effects)]
pub fn plan_scene(state: &SceneState, mm_per_unit: f64, show: &Show) -> Vec<Value> {
    let mut items: Vec<Value> = Vec::new();
    let u = if state.units.is_empty() { "ft" } else { state.units.as_str() };
    let s_triangles = show.triangles.unwrap_or(false);
    let s_contours = show.contours.unwrap_or(true);
    let s_labels = show.labels.unwrap_or(true);
    let s_breaklines = show.breaklines.unwrap_or(true);
    let s_outline = show.outline.unwrap_or(true);
    let s_points = show.points.unwrap_or(true);
    let s_point_labels = show.point_labels.unwrap_or(false);
    let s_slopes = show.slopes.unwrap_or(false);
    let s_alignments = show.alignments.unwrap_or(true);
    let s_pads = show.pads.unwrap_or(true);
    let plt = show.point_label_text.as_deref().filter(|t| matches!(*t, "n" | "nz" | "nzd")).unwrap_or("nzd");
    let unit_per_mm = 1.0 / mm_per_unit;
    let sf = state.surface;

    if let Some(sf) = sf.filter(|_| s_triangles) {
        let e = sf.edges();
        for pair in e.chunks(2) {
            let (Some(&a), Some(&b)) = (pair.first(), pair.get(1)) else { continue };
            let (a, b) = (a as usize, b as usize);
            items.push(json!({ "layer": "C-TOPO-TINN", "kind": "poly", "pts": [xy(sf.x.g(a) + sf.ox, sf.y.g(a) + sf.oy), xy(sf.x.g(b) + sf.ox, sf.y.g(b) + sf.oy)] }));
        }
    }
    if let Some(sf) = sf.filter(|_| s_outline) {
        for ring in sf.outline() {
            items.push(json!({ "layer": "C-TOPO-BNDY", "kind": "poly", "pts": ring.j(), "closed": true }));
        }
    }
    if let (Some(_), Some((interval, major_n, list))) = (sf.filter(|_| s_contours), state.contours.as_ref()) {
        let major = interval * if *major_n != 0.0 && !major_n.is_nan() { *major_n } else { 5.0 };
        let is_major = |z: f64| (z / major - round(z / major)).abs() < 1e-6;
        let dec = decimals_of(*interval);
        for c in list {
            let maj = is_major(c.z);
            items.push(json!({ "layer": if maj { "C-TOPO-MAJR" } else { "C-TOPO-MINR" }, "kind": "poly", "pts": c.pts.j(), "closed": c.closed, "elev": c.z.j() }));
            if maj && s_labels {
                let pts: Vec<(f64, f64)> = c.pts.iter().map(|p| (p.x, p.y)).collect();
                for (x, y, rot) in labels_along(&pts, 60.0 * unit_per_mm, 25.0 * unit_per_mm) {
                    items.push(text("C-TOPO-TEXT", x, y, rot, 2.0, "center", true, fixed(c.z, dec), true));
                }
            }
        }
    }
    if let Some(sf) = sf.filter(|_| s_slopes) {
        let len = 4.0 * unit_per_mm;
        for a in sf.slopes() {
            if a.size < len * 0.8 || !(a.slope > 1e-6) {
                continue;
            }
            let dx = a.dir.jcos() * len / 2.0;
            let dy = a.dir.jsin() * len / 2.0;
            let tip = (a.x + dx, a.y + dy);
            let h = len * 0.3;
            let ang = a.dir;
            items.push(json!({ "layer": "C-TOPO-SLOP", "kind": "poly", "pts": [xy(a.x - dx, a.y - dy), xy(tip.0, tip.1)] }));
            items.push(json!({ "layer": "C-TOPO-SLOP", "kind": "poly", "pts": [
                xy(tip.0 - (ang - 0.45).jcos() * h, tip.1 - (ang - 0.45).jsin() * h), xy(tip.0, tip.1), xy(tip.0 - (ang + 0.45).jcos() * h, tip.1 - (ang + 0.45).jsin() * h)] }));
        }
    }
    if s_breaklines {
        for b in &state.breaklines {
            items.push(json!({ "layer": "C-TOPO-BRKL", "kind": "poly", "pts": b, "z": true }));
        }
    }
    if s_points {
        for p in &state.points {
            items.push(json!({ "layer": "V-NODE", "kind": "point", "x": p.x.j(), "y": p.y.j(), "z": p.z.unwrap_or(0.0).j() }));
            if s_point_labels {
                let mut s = p.n.clone();
                if plt != "n"
                    && let Some(z) = p.z
                {
                    s.push_str("  ");
                    s.push_str(&fixed(z, 2));
                }
                if plt == "nzd" && !p.d.is_empty() {
                    s.push_str("  ");
                    s.push_str(&p.d);
                }
                items.push(text("V-NODE-TEXT", p.x + 1.2 * unit_per_mm, p.y + 0.4 * unit_per_mm, 0.0, 1.5, "left", false, s, false));
            }
        }
    }
    if s_pads {
        for pd in &state.pads {
            items.push(json!({ "layer": "C-GRAD-PADS", "kind": "poly", "pts": pd.pts, "closed": true, "elev": pd.z.j() }));
            if !pd.daylight.is_empty() {
                items.push(json!({ "layer": "C-GRAD-DAYL", "kind": "poly", "pts": pd.daylight, "closed": true, "z": true }));
                for r in &pd.rays {
                    if r.4 > 1e-6 {
                        items.push(json!({ "layer": "C-GRAD-SLOP", "kind": "poly", "pts": [xy(r.0, r.1), xy(r.2, r.3)] }));
                    }
                }
            }
            let c = centroid(&pd.pts);
            let name = if pd.name.is_empty() { t("scene.pad.default", &[]) } else { pd.name.clone() };
            items.push(text(
                "C-GRAD-PADS",
                c.0,
                c.1,
                0.0,
                2.2,
                "center",
                true,
                tr!("scene.pad.label", name = name, z = fixed(pd.z, 2)),
                true,
            ));
        }
        for c in &state.corridors {
            let line = |k: &str| c.lines.iter().find(|(n, _)| n == k).map(|(_, v)| v);
            for k in ["EOP_L", "EOP_R", "SH_L", "SH_R"] {
                if let Some(l) = line(k).filter(|l| l.len() >= 2) {
                    items.push(json!({ "layer": "C-ROAD-CORR", "kind": "poly", "pts": l, "z": true }));
                }
            }
            for k in ["DL_L", "DL_R"] {
                if let Some(l) = line(k).filter(|l| l.len() >= 2) {
                    items.push(json!({ "layer": "C-GRAD-DAYL", "kind": "poly", "pts": l, "z": true }));
                }
            }
            for (sl, sr) in &c.sections {
                for q in [sl, sr] {
                    let (Some(q2), Some(q3)) = (q.get(2), q.get(3)) else { continue };
                    let (a, b) = (pxy(q2), pxy(q3));
                    if hypot(b.0 - a.0, b.1 - a.1) > 1e-6 {
                        items.push(json!({ "layer": "C-GRAD-SLOP", "kind": "poly", "pts": [q2, q3] }));
                    }
                }
            }
        }
    }
    if s_alignments {
        for a in &state.alignments {
            let Some(al) = a.alignment.filter(|al| al.ok()) else {
                if a.pis.len() >= 2 {
                    items.push(json!({ "layer": "C-ROAD-CNTR", "kind": "poly", "pts": a.pis, "dashed": true }));
                }
                continue;
            };
            for e in &al.elements {
                match e {
                    Element::Line { p0, p1, .. } => items.push(json!({ "layer": "C-ROAD-CNTR", "kind": "poly", "pts": [p0.j(), p1.j()] })),
                    Element::Arc(e) => {
                        let a1 = e.a0 + if e.ccw { e.delta } else { -e.delta };
                        items.push(json!({ "layer": "C-ROAD-CNTR", "kind": "arc", "c": e.c.j(), "r": e.r.j(), "a0": if e.ccw { e.a0 } else { a1 }.j(), "a1": if e.ccw { a1 } else { e.a0 }.j() }));
                    }
                }
            }
            let major = 100.0;
            let minor = if u == "m" { 20.0 } else { 50.0 };
            let tick = 1.5 * unit_per_mm;
            for (sta, p) in al.ticks(minor) {
                let is_maj = (sta / major - round(sta / major)).abs() < 1e-9;
                let nx = p.dir.jsin();
                let ny = -p.dir.jcos();
                let k = if is_maj { tick * 1.6 } else { tick };
                items.push(json!({ "layer": "C-ROAD-STAN", "kind": "poly", "pts": [xy(p.x - nx * k, p.y - ny * k), xy(p.x + nx * k, p.y + ny * k)] }));
                if is_maj {
                    let mut rot = p.dir - PI / 2.0;
                    let off = -(k + 0.8 * unit_per_mm);
                    let mut align = "right";
                    if rot.jcos() < -1e-9 || (rot.jcos().abs() <= 1e-9 && rot.jsin() < 0.0) {
                        rot += PI;
                        align = "left";
                    }
                    items.push(text(
                        "C-ROAD-STAN",
                        p.x + nx * off,
                        p.y + ny * off,
                        rot,
                        1.8,
                        align,
                        true,
                        format_station(sta, u, Some(0)),
                        false,
                    ));
                }
            }
            for e in &al.elements {
                let Element::Arc(e) = e else { continue };
                for (p, sta, pc) in [(e.p0, e.sta, true), (e.p1, e.sta + e.len, false)] {
                    let Some(pt) = al.point_at(sta) else { continue };
                    let nx = pt.dir.jsin();
                    let ny = -pt.dir.jcos();
                    let k = 3.0 * unit_per_mm;
                    items.push(json!({ "layer": "C-ROAD-STAN", "kind": "poly", "pts": [xy(p.x, p.y), xy(p.x + nx * k, p.y + ny * k)] }));
                    let mut rot = pt.dir - PI / 2.0;
                    let mut align = "left";
                    if rot.jcos() < -1e-9 {
                        rot += PI;
                        align = "right";
                    }
                    let label = tr!(if pc { "scene.sta.pc" } else { "scene.sta.pt" }, sta = format_station(sta, u, None));
                    items.push(text(
                        "C-ROAD-STAN",
                        p.x + nx * (k + 0.8 * unit_per_mm),
                        p.y + ny * (k + 0.8 * unit_per_mm),
                        rot,
                        1.6,
                        align,
                        true,
                        label,
                        false,
                    ));
                }
            }
            if let Some(p0) = al.point_at(al.start) {
                let name = if a.name.is_empty() { t("scene.al.default", &[]) } else { a.name.clone() };
                items.push(text(
                    "C-ROAD-CNTR",
                    p0.x - p0.dir.jcos() * 3.0 * unit_per_mm,
                    p0.y - p0.dir.jsin() * 3.0 * unit_per_mm,
                    0.0,
                    2.2,
                    "right",
                    true,
                    name,
                    false,
                ));
            }
        }
    }
    if show.pipes != Some(false)
        && let Some((net, check)) = &state.network
    {
        let per = if u == "m" { 1000.0 } else { 12.0 };
        let dia_txt = |d: f64| if u == "m" { format!("{} mm", fixed(d, 0)) } else { format!("{}\"", fixed(d, 0)) };
        let bad: Vec<Option<String>> = check.as_ref().map(|c| c.problems.iter().map(|p| p.id.clone()).collect()).unwrap_or_default();
        let is_bad = |id: &str| bad.iter().any(|b| b.as_deref() == Some(id));
        let by_id = |id: &str| net.structures.iter().rev().find(|x| x.id == id);
        let dia0 = |d: f64| if d != 0.0 && !d.is_nan() { d } else { 0.0 };
        for p in &net.pipes {
            let (Some(a), Some(b)) = (by_id(&p.from), by_id(&p.to)) else { continue };
            let l = hypot(b.x - a.x, b.y - a.y);
            if l < 1e-9 {
                continue;
            }
            let ux = (b.x - a.x) / l;
            let uy = (b.y - a.y) / l;
            let ra = max(dia0(a.dia) / per / 2.0, 1.2 * unit_per_mm);
            let rb = max(dia0(b.dia) / per / 2.0, 1.2 * unit_per_mm);
            items.push(json!({ "layer": if is_bad(&p.id) { "C-STRM-PROB" } else { "C-STRM-PIPE" }, "kind": "poly", "pts": [xy(a.x + ux * ra, a.y + uy * ra), xy(b.x - ux * rb, b.y - uy * rb)] }));
            let m = ((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
            let h = 1.6 * unit_per_mm;
            let tip = (m.0 + ux * h, m.1 + uy * h);
            items.push(json!({ "layer": "C-STRM-PIPE", "kind": "poly", "pts": [
                xy(tip.0 - ux * h * 1.6 + uy * h * 0.7, tip.1 - uy * h * 1.6 - ux * h * 0.7), xy(tip.0, tip.1), xy(tip.0 - ux * h * 1.6 - uy * h * 0.7, tip.1 - uy * h * 1.6 + ux * h * 0.7)] }));
            let info = check.as_ref().and_then(|c| c.pipes.iter().find(|(k, _)| *k == p.id).map(|(_, v)| v));
            if l * mm_per_unit > 18.0 {
                let mut rot = uy.jatan2(ux);
                if rot > PI / 2.0 {
                    rot -= PI;
                } else if rot < -PI / 2.0 {
                    rot += PI;
                }
                let off = 1.8 * unit_per_mm;
                let mut s = format!("{} {}", p.name, dia_txt(p.dia));
                if let Some(sl) = info.and_then(|i| i.slope) {
                    s.push_str(&format!(" @ {}%", fixed(sl * 100.0, 2)));
                }
                items.push(text("C-STRM-TEXT", m.0 - rot.jsin() * off, m.1 + rot.jcos() * off, rot, 1.6, "center", true, s, false));
            }
        }
        for x in &net.structures {
            let r = max(dia0(x.dia) / per / 2.0, 1.2 * unit_per_mm);
            let mut pts = Vec::new();
            for k in 0..=24 {
                let a = f64::from(k) * PI / 12.0;
                pts.push(xy(x.x + a.jcos() * r, x.y + a.jsin() * r));
            }
            items.push(
                json!({ "layer": if is_bad(&x.id) { "C-STRM-PROB" } else { "C-STRM-STRC" }, "kind": "poly", "pts": pts, "closed": true }),
            );
            let st = check.as_ref().and_then(|c| c.structs.iter().find(|(k, _)| *k == x.id).map(|(_, v)| v));
            let label = match st.and_then(|s| s.rim) {
                Some(rim) => tr!("scene.strc.rim", name = x.name.as_str(), rim = fixed(rim, 2)),
                None => x.name.clone(),
            };
            items.push(text("C-STRM-TEXT", x.x + r + unit_per_mm, x.y + r, 0.0, 1.6, "left", false, label, false));
        }
    }
    items
}

/// `toDxfEntities(scene, mmPerUnit)`.
pub fn to_dxf_entities(items: &[Value], mm_per_unit: f64) -> Vec<Value> {
    let mut out = Vec::with_capacity(items.len());
    let n = |it: &Value, k: &str| cc_civil::js::numf(it.get(k));
    for it in items {
        let layer = it.get("layer").cloned().unwrap_or(Value::Null);
        match it.get("kind").and_then(Value::as_str) {
            Some("poly") => {
                let mut o = serde_json::Map::new();
                let three = cc_civil::js::truthy(it.get("z"));
                o.insert("type".into(), Value::String(if three { "poly3d" } else { "poly" }.into()));
                o.insert("layer".into(), layer);
                o.insert("pts".into(), it.get("pts").cloned().unwrap_or(Value::Null));
                if let Some(c) = it.get("closed") {
                    o.insert("closed".into(), c.clone());
                }
                if !three {
                    let e = n(it, "elev");
                    o.insert("elev".into(), if e != 0.0 && !e.is_nan() { e.j() } else { json!(0) });
                }
                out.push(Value::Object(o));
            }
            Some("arc") => out
                .push(json!({ "type": "arc", "layer": layer, "c": it.get("c"), "r": it.get("r"), "a0": it.get("a0"), "a1": it.get("a1") })),
            Some("point") => out.push(json!({ "type": "point", "layer": layer, "x": it.get("x"), "y": it.get("y"), "z": it.get("z") })),
            Some("text") => {
                let h = n(it, "mm") / mm_per_unit;
                let rot = n(it, "rot");
                let middle = cc_civil::js::truthy(it.get("middle"));
                let dx = if middle { rot.jsin() * h / 2.0 } else { 0.0 };
                let dy = if middle { -rot.jcos() * h / 2.0 } else { 0.0 };
                out.push(json!({ "type": "text", "layer": layer, "x": (n(it, "x") + dx).j(), "y": (n(it, "y") + dy).j(), "h": h.j(), "rot": rot.j(), "text": it.get("text"), "align": it.get("align") }));
            }
            _ => {}
        }
    }
    out
}
