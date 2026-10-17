//! gis-layers.js — the GIS layer model of a project: picking, extents, the plan scene, labels from an attribute, and
//! what "Use for the surface" offers. These read `state.gis` + the features (cc-civil's gis_state); the edits that are
//! undo steps on the project (addLayers, removeLayer, setLayer, transformLayers, moveLayers, useForSurface) sit on the
//! project model.

use crate::gis_geom::{BBox, bounds, point_in_ring, ring_area};
use cc_civil::gis_state::{Feature, Geom, Layer, Pos};
use cc_civil::js::{G, J, JsMath, hypot, max, min, num_to_string, slice16, str_to_number, to_fixed};
use cc_civil::tr;
use serde_json::{Map, Value, json};

pub const MAX_LAYER_FEATURES: usize = 500_000;
pub const MAX_LABELS: usize = 5000;

/// `tokenLabel(token)`.
pub fn token_label(token: &str) -> String {
    match token {
        "blue" | "orange" | "magenta" | "cyan" | "ochre" | "green" | "red" => tr!(&format!("layers.color.{token}")),
        _ => token.to_string(),
    }
}

/// A feature list of a layer (the project's Map).
pub fn features_of<'a>(features: &'a cc_civil::gis_state::Features, id: &str) -> &'a [Feature] {
    features.iter().find(|(k, _)| k == id).map_or(&[], |(_, f)| f.as_slice())
}

/// `layerBounds(features)`.
pub fn layer_bounds(features: &[Feature]) -> Option<BBox> {
    let mut out = BBox::default();
    for f in features {
        let b = bounds(f.g.as_ref());
        if !b.min_x.is_finite() {
            continue;
        }
        out.min_x = min(out.min_x, b.min_x);
        out.min_y = min(out.min_y, b.min_y);
        out.max_x = max(out.max_x, b.max_x);
        out.max_y = max(out.max_y, b.max_y);
    }
    if out.min_x.is_finite() { Some(out) } else { None }
}

/// `gisExtents(project)`: visible layers.
pub fn gis_extents(layers: &[Layer], features: &cc_civil::gis_state::Features) -> Option<BBox> {
    let mut out: Option<BBox> = None;
    for l in layers {
        if !l.visible {
            continue;
        }
        let Some(b) = layer_bounds(features_of(features, &l.id)) else { continue };
        out = Some(match out {
            Some(o) => BBox {
                min_x: min(o.min_x, b.min_x),
                min_y: min(o.min_y, b.min_y),
                max_x: max(o.max_x, b.max_x),
                max_y: max(o.max_y, b.max_y),
            },
            None => b,
        });
    }
    out
}

fn seg_dist(px: f64, py: f64, a: &Pos, b: &Pos) -> f64 {
    let dx = b.g(0) - a.g(0);
    let dy = b.g(1) - a.g(1);
    let l = dx * dx + dy * dy;
    let t = if l != 0.0 && !l.is_nan() { max(0.0, min(1.0, ((px - a.g(0)) * dx + (py - a.g(1)) * dy) / l)) } else { 0.0 };
    hypot(px - a.g(0) - dx * t, py - a.g(1) - dy * t)
}

/// `pickFeature(project, x, y, tol)` → (layer id, index).
pub fn pick_feature(layers: &[Layer], features: &cc_civil::gis_state::Features, x: f64, y: f64, tol: f64) -> Option<(String, usize)> {
    let mut inside: Option<(String, usize, f64)> = None;
    let mut pt: Option<(String, usize, f64)> = None;
    let mut edge: Option<(String, usize)> = None;
    for l in layers.iter().rev() {
        if !l.visible {
            continue;
        }
        let fs = features_of(features, &l.id);
        let mut best: Option<usize> = None;
        let mut bd = tol;
        for (i, f) in fs.iter().enumerate() {
            let b = bounds(f.g.as_ref());
            if x < b.min_x - tol || x > b.max_x + tol || y < b.min_y - tol || y > b.max_y + tol {
                continue;
            }
            let Some(g) = &f.g else { continue };
            if let Geom::Pt(c) = g {
                let d = hypot(c.g(0) - x, c.g(1) - y);
                if d <= tol && pt.as_ref().is_none_or(|p| d < p.2) {
                    pt = Some((l.id.clone(), i, d));
                }
                continue;
            }
            let mut hit = false;
            let rings: Vec<&Vec<Pos>> = match g {
                Geom::Ln(c) => c.iter().collect(),
                Geom::Pg(c) => c.iter().flatten().collect(),
                Geom::Pt(_) => Vec::new(),
            };
            for r in rings {
                for k in 1..r.len() {
                    let d = seg_dist(x, y, r.get(k.saturating_sub(1)).unwrap_or(&Vec::new()), r.get(k).unwrap_or(&Vec::new()));
                    if d <= bd {
                        bd = d;
                        best = Some(i);
                        hit = true;
                    }
                }
            }
            if let Geom::Pg(c) = g
                && !hit
            {
                for poly in c {
                    let Some(outer) = poly.first() else { continue };
                    if !point_in_ring(x, y, outer) || poly.iter().skip(1).any(|h| point_in_ring(x, y, h)) {
                        continue;
                    }
                    let a = ring_area(outer).abs();
                    if inside.as_ref().is_none_or(|ins| a < ins.2) {
                        inside = Some((l.id.clone(), i, a));
                    }
                }
            }
        }
        if let Some(b) = best
            && edge.is_none()
        {
            edge = Some((l.id.clone(), b));
        }
    }
    if let Some((l, i, _)) = pt {
        return Some((l, i));
    }
    if let Some(e) = edge {
        return Some(e);
    }
    inside.map(|(l, i, _)| (l, i))
}

// ---------------------------------------------------------------- the plan scene

fn to_xy(r: &[Pos]) -> Vec<Value> {
    r.iter()
        .map(|p| {
            if p.len() > 2 {
                json!({ "x": p.g(0).j(), "y": p.g(1).j(), "z": p.g(2).j() })
            } else {
                json!({ "x": p.g(0).j(), "y": p.g(1).j() })
            }
        })
        .collect()
}

/// `gisScene(project, mmPerUnit)` → { layers, items }.
pub fn gis_scene(layers: &[Layer], features: &cc_civil::gis_state::Features, mm_per_unit: f64) -> Value {
    let mut out_layers = Vec::new();
    let mut items: Vec<Value> = Vec::new();
    let h = 0.6 / mm_per_unit;
    for l in layers {
        if !l.visible {
            continue;
        }
        let name = format!("GIS-{}", l.id);
        out_layers.push(json!({ "name": name, "token": l.color, "aci": 7, "label": l.name }));
        let fs = features_of(features, &l.id);
        if l.geom == "pt" {
            for f in fs {
                let Some(g) = &f.g else { continue };
                let c: &[f64] = match g {
                    Geom::Pt(c) => c,
                    // `const [x, y] = f.g.c` of a line or polygon: its first part / polygon (not numbers)
                    _ => &[],
                };
                let (x, y) = (c.g(0), c.g(1));
                items.push(json!({ "layer": name, "kind": "poly", "pts": [
                    { "x": (x - h).j(), "y": (y - h).j() }, { "x": (x + h).j(), "y": (y - h).j() }, { "x": (x + h).j(), "y": (y + h).j() }, { "x": (x - h).j(), "y": (y + h).j() }
                ], "closed": true }));
            }
            if l.label.is_some() {
                for it in label_items(l, fs, mm_per_unit) {
                    items.push(with_layer(it, &name));
                }
            }
            continue;
        }
        for f in fs {
            match &f.g {
                Some(Geom::Ln(c)) => {
                    for part in c {
                        items.push(
                            json!({ "layer": name, "kind": "poly", "pts": to_xy(part), "z": l.has_z && part.iter().all(|p| p.len() > 2) }),
                        );
                    }
                }
                Some(Geom::Pg(c)) => {
                    for poly in c {
                        for r in poly {
                            let r2 = r.get(..r.len().saturating_sub(1)).unwrap_or(&[]);
                            items.push(json!({ "layer": name, "kind": "poly", "pts": to_xy(r2), "closed": true, "z": l.has_z && r.iter().all(|p| p.len() > 2) }));
                        }
                    }
                }
                _ => {}
            }
        }
        if l.label.is_some() {
            for it in label_items(l, fs, mm_per_unit) {
                items.push(with_layer(it, &name));
            }
        }
    }
    json!({ "layers": out_layers, "items": items })
}
fn with_layer(mut it: Value, name: &str) -> Value {
    if let Some(o) = it.as_object_mut() {
        let mut m = Map::new();
        // { ...it, layer: name }: the existing keys keep their place, layer goes last
        for (k, v) in o.iter() {
            m.insert(k.clone(), v.clone());
        }
        m.insert("layer".into(), Value::String(name.into()));
        return Value::Object(m);
    }
    it
}

// ---------------------------------------------------------------- labels from an attribute (0.4 V7)

/// `labelText(v)`.
pub fn label_text(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) if s.is_empty() => String::new(),
        Some(Value::Number(n)) => {
            let x = n.as_f64().unwrap_or(f64::NAN);
            if x.is_finite() { num_to_string(str_to_number(&to_fixed(x, 3))) } else { String::new() }
        }
        Some(Value::Bool(b)) => tr!(if *b { "gis.label.yes" } else { "gis.label.no" }),
        Some(x) => {
            let s = cc_civil::js::to_js_string(x);
            let mut out = String::new();
            let mut ws = false;
            for c in s.chars() {
                if cc_civil::js::is_ws(c) {
                    if !ws {
                        out.push(' ');
                    }
                    ws = true;
                } else {
                    out.push(c);
                    ws = false;
                }
            }
            slice16(cc_civil::js::trim(&out), 80)
        }
    }
}

/// `labelItems(layer, features, mmPerUnit)`.
pub fn label_items(layer: &Layer, features: &[Feature], mm_per_unit: f64) -> Vec<Value> {
    let Some(field) = layer.label.as_deref() else { return Vec::new() };
    let u = 1.0 / mm_per_unit;
    let mut out = Vec::new();
    for f in features {
        if out.len() >= MAX_LABELS {
            break;
        }
        let text = label_text(f.p.get(field));
        let Some(g) = &f.g else { continue };
        if text.is_empty() {
            continue;
        }
        match g {
            Geom::Pt(c) => out.push(json!({ "kind": "text", "x": (c.g(0) + 1.2 * u).j(), "y": (c.g(1) + 0.6 * u).j(), "rot": 0, "mm": 1.8, "align": "left", "middle": true, "text": text, "mask": true })),
            Geom::Ln(c) => {
                let mut best: Option<&Vec<Pos>> = None;
                let mut bl = -1.0;
                for part in c {
                    let mut l = 0.0;
                    for i in 1..part.len() {
                        let (a, b) = (part.get(i.saturating_sub(1)), part.get(i));
                        l += hypot(b.map_or(f64::NAN, |b| b.g(0)) - a.map_or(f64::NAN, |a| a.g(0)), b.map_or(f64::NAN, |b| b.g(1)) - a.map_or(f64::NAN, |a| a.g(1)));
                    }
                    if l > bl {
                        bl = l;
                        best = Some(part);
                    }
                }
                let Some(best) = best.filter(|b| b.len() >= 2) else { continue };
                let p = |i: usize| best.get(i).cloned().unwrap_or_default();
                let mut half = bl / 2.0;
                let mut i = 1usize;
                while i < best.len() {
                    let (a, b) = (p(i.saturating_sub(1)), p(i));
                    let d = hypot(b.g(0) - a.g(0), b.g(1) - a.g(1));
                    if half <= d {
                        break;
                    }
                    half -= d;
                    i = i.saturating_add(1);
                }
                let k = i.min(best.len().saturating_sub(1));
                let (a, b) = (p(k.saturating_sub(1)), p(k));
                let dd = hypot(b.g(0) - a.g(0), b.g(1) - a.g(1));
                let d = if dd != 0.0 && !dd.is_nan() { dd } else { 1.0 };
                let mut rot = (b.g(1) - a.g(1)).jatan2(b.g(0) - a.g(0));
                if rot.jcos() < -1e-9 {
                    rot += std::f64::consts::PI;
                }
                if rot > std::f64::consts::PI {
                    rot -= 2.0 * std::f64::consts::PI;
                }
                let x = a.g(0) + ((b.g(0) - a.g(0)) * half) / d;
                let y = a.g(1) + ((b.g(1) - a.g(1)) * half) / d;
                out.push(json!({ "kind": "text", "x": (x - rot.jsin() * 1.4 * u).j(), "y": (y + rot.jcos() * 1.4 * u).j(), "rot": rot.j(), "mm": 1.8, "align": "center", "middle": true, "text": text, "mask": true }));
            }
            Geom::Pg(c) => {
                let mut ring: Option<&Vec<Pos>> = None;
                let mut ba = -1.0;
                for poly in c {
                    let Some(o) = poly.first() else { continue };
                    let a = ring_area(o).abs();
                    if a > ba {
                        ba = a;
                        ring = Some(o);
                    }
                }
                let Some(ring) = ring else { continue };
                let p = |i: usize| ring.get(i).cloned().unwrap_or_default();
                let (mut cx, mut cy, mut a2) = (0.0, 0.0, 0.0);
                let mut k = 0usize;
                while k.saturating_add(1) < ring.len() {
                    let (q0, q1) = (p(k), p(k.saturating_add(1)));
                    let cr = q0.g(0) * q1.g(1) - q1.g(0) * q0.g(1);
                    a2 += cr;
                    cx += (q0.g(0) + q1.g(0)) * cr;
                    cy += (q0.g(1) + q1.g(1)) * cr;
                    k = k.saturating_add(1);
                }
                if a2.abs() > 1e-12 {
                    cx /= 3.0 * a2;
                    cy /= 3.0 * a2;
                } else {
                    cx = p(0).g(0);
                    cy = p(0).g(1);
                }
                if !point_in_ring(cx, cy, ring) {
                    cx = p(0).g(0);
                    cy = p(0).g(1);
                }
                out.push(json!({ "kind": "text", "x": cx.j(), "y": cy.j(), "rot": 0, "mm": 1.8, "align": "center", "middle": true, "text": text, "mask": true }));
            }
        }
    }
    out
}

// ---------------------------------------------------------------- use for the surface

fn z_count(g: Option<&Geom>) -> bool {
    match g {
        None => false,
        Some(Geom::Pt(c)) => c.len() > 2,
        Some(Geom::Ln(c)) => c.iter().flatten().any(|p| p.len() > 2),
        Some(Geom::Pg(c)) => c.iter().flatten().flatten().any(|p| p.len() > 2),
    }
}

/// `useChoices(layer, features)`.
pub fn use_choices(layer: &Layer, features: &[Feature]) -> Vec<Value> {
    let mut out = Vec::new();
    let nf: Vec<String> = layer.fields.iter().filter(|f| f.ty == "number").map(|f| f.name.clone()).collect();
    let with_z = features.iter().any(|f| z_count(f.g.as_ref()));
    match layer.geom.as_str() {
        "ln" => {
            if with_z {
                out.push(json!({ "use": "breaklines", "from": "z", "label": tr!("layers.use.brk.z") }));
            }
            if !nf.is_empty() {
                out.push(json!({ "use": "breaklines", "from": "field", "fields": nf, "label": tr!("layers.use.brk.field") }));
            }
        }
        "pg" => {
            out.push(json!({ "use": "boundary", "label": tr!("layers.use.boundary") }));
            if with_z {
                out.push(json!({ "use": "breaklines", "from": "z", "label": tr!("layers.use.pgbrk.z") }));
            }
            if !nf.is_empty() {
                out.push(json!({ "use": "breaklines", "from": "field", "fields": nf, "label": tr!("layers.use.pgbrk.field") }));
            }
        }
        _ => {
            if with_z {
                out.push(json!({ "use": "points", "from": "z", "label": tr!("layers.use.pts.z") }));
            }
            if !nf.is_empty() {
                out.push(json!({ "use": "points", "from": "field", "fields": nf, "label": tr!("layers.use.pts.field") }));
            }
        }
    }
    out
}
