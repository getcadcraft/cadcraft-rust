//! dwg-write.js — CivilCraft's DWG export: the entity list (scene.js / dxf.js shapes) mapped to a cc-dwg model by
//! `fromCivilList` (lib/dwg civil.js, ported here: it is the one part of the writer's import closure cc-io-dwg does not
//! have) and written by cc-io-dwg's writer, which is byte-identical to the JS writer (`forks/tools/iodiff.mjs`).
//! Map pictures become IMAGE + IMAGEDEF (0.4 V8).

use cc_civil::js::{J, hypot, round};
use cc_io_dwg::w_write::{GuidMode, WriteOpts, write_dwg_opts};
use serde_json::{Map, Value, json};

fn aci(name: &str) -> Option<f64> {
    Some(match name {
        "red" => 1.0,
        "yellow" => 2.0,
        "green" => 3.0,
        "cyan" => 4.0,
        "blue" => 5.0,
        "magenta" => 6.0,
        "white" => 7.0,
        "gray" => 8.0,
        "orange" => 30.0,
        "brown" => 34.0,
        _ => return None,
    })
}
fn num(v: Option<&Value>) -> f64 {
    cc_civil::js::numf(v)
}
fn fin(xs: &[f64]) -> bool {
    xs.iter().all(|x| x.is_finite())
}
fn or0(v: f64) -> f64 {
    if v != 0.0 && !v.is_nan() { v } else { 0.0 }
}
fn color_of(c: Option<&Value>) -> Value {
    match c.and_then(Value::as_f64) {
        Some(0.0) => json!({ "index": 0 }),
        Some(x) if cc_civil::js::is_integer(x) && (1.0..=255.0).contains(&x) => json!({ "index": x }),
        _ => json!({ "index": 256 }),
    }
}

/// `fromCivilList({ units, layers, entities, created })` → the cc-dwg model as JSON.
pub fn from_civil_list(units: Option<&str>, layers: &[Value], entities: &[Value]) -> Value {
    let insunits = match units {
        Some("m") => 6.0,
        Some("ft") => 2.0,
        _ => 0.0,
    };
    let metric = match units {
        Some(u) => u == "m",
        None => ![1.0, 2.0, 3.0, 8.0, 9.0, 10.0].contains(&insunits),
    };
    let header = json!({ "INSUNITS": insunits, "MEASUREMENT": if metric { 1 } else { 0 }, "LUPREC": if metric { 3 } else { 2 } });
    let mut table_layers: Vec<Value> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let add_layer = |l: &Value, table_layers: &mut Vec<Value>, seen: &mut Vec<String>| {
        let name = match l.get("name") {
            None | Some(Value::Null) => "0".to_string(),
            Some(v) => cc_civil::js::to_js_string(v),
        };
        if seen.contains(&name) {
            return;
        }
        seen.push(name.clone());
        let c = match l.get("color") {
            Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
            Some(Value::String(s)) => aci(s).unwrap_or(7.0),
            _ => 7.0,
        };
        let r = round(c.abs());
        let idx = if r != 0.0 && !r.is_nan() { r } else { 7.0 };
        let idx = cc_civil::js::min(255.0, cc_civil::js::max(1.0, idx));
        table_layers.push(json!({
            "name": name, "color": { "index": idx }, "off": l.get("on") == Some(&Value::Bool(false)), "frozen": cc_civil::js::truthy(l.get("frozen")),
            "locked": cc_civil::js::truthy(l.get("locked")), "plot": l.get("plot") != Some(&Value::Bool(false)),
        }));
    };
    let zero = layers
        .iter()
        .find(|l| l.get("name").and_then(Value::as_str) == Some("0"))
        .cloned()
        .unwrap_or_else(|| json!({ "name": "0", "color": 7 }));
    add_layer(&zero, &mut table_layers, &mut seen);
    for l in layers {
        add_layer(l, &mut table_layers, &mut seen);
    }
    let text = |e: &Value| -> Map<String, Value> {
        let ha_v = e.get("ha").filter(|v| !v.is_null()).or_else(|| e.get("align"));
        let ha = match ha_v.and_then(Value::as_str) {
            Some("left") => Some(0.0),
            Some("center") => Some(1.0),
            Some("right") => Some(2.0),
            _ => None,
        }
        .or_else(|| e.get("ha").and_then(Value::as_f64).filter(|x| cc_civil::js::is_integer(*x)))
        .unwrap_or(0.0);
        let va = match e.get("va").and_then(Value::as_str) {
            Some("baseline") => Some(0.0),
            Some("bottom") => Some(1.0),
            Some("middle") => Some(2.0),
            Some("top") => Some(3.0),
            _ => None,
        }
        .or_else(|| e.get("va").and_then(Value::as_f64).filter(|x| cc_civil::js::is_integer(*x)))
        .unwrap_or(0.0);
        let at = json!({ "x": e.get("x"), "y": e.get("y"), "z": 0 });
        let mut m = Map::new();
        m.insert("insert".into(), at.clone());
        if ha != 0.0 || va != 0.0 {
            m.insert("align".into(), at);
        }
        m.insert("hAlign".into(), ha.j());
        m.insert("vAlign".into(), va.j());
        m.insert("height".into(), e.get("h").cloned().unwrap_or(Value::Null));
        let rot = num(e.get("rot"));
        m.insert("rotation".into(), if rot.is_finite() { rot.j() } else { json!(0) });
        let wf = num(e.get("wf"));
        m.insert("widthFactor".into(), if wf > 0.0 { wf.j() } else { json!(1) });
        m.insert("value".into(), Value::String(cc_civil::js::string_or_empty(e.get("text"))));
        m
    };
    let conv = |e: &Value, table_layers: &mut Vec<Value>, seen: &mut Vec<String>| -> Option<Value> {
        let layer = match e.get("layer") {
            Some(v) if cc_civil::js::truthy(Some(v)) => cc_civil::js::to_js_string(v),
            _ => "0".into(),
        };
        if !seen.contains(&layer) {
            add_layer(&json!({ "name": layer, "color": 7 }), table_layers, seen);
        }
        let mut base = Map::new();
        base.insert("layer".into(), Value::String(layer));
        base.insert("color".into(), color_of(e.get("color")));
        let pts_ok = |e: &Value| -> Vec<Value> {
            cc_civil::js::arr(e.get("pts")).iter().filter(|p| fin(&[num(p.get("x")), num(p.get("y"))])).cloned().collect()
        };
        let ty = e.get("type").and_then(Value::as_str).unwrap_or("");
        let mut out = base;
        match ty {
            "poly" => {
                let pts = pts_ok(e);
                if pts.len() < 2 {
                    return None;
                }
                out.insert("type".into(), json!("LWPOLYLINE"));
                out.insert(
                    "vertices".into(),
                    Value::Array(
                        pts.iter()
                            .map(|p| {
                                let b = num(p.get("b"));
                                json!({ "x": p.get("x"), "y": p.get("y"), "bulge": if b.is_finite() { b.j() } else { json!(0) } })
                            })
                            .collect(),
                    ),
                );
                out.insert("closed".into(), Value::Bool(cc_civil::js::truthy(e.get("closed"))));
                let el = num(e.get("elev"));
                out.insert("elevation".into(), if el.is_finite() { el.j() } else { json!(0) });
            }
            "poly3d" => {
                let pts = pts_ok(e);
                if pts.len() < 2 {
                    return None;
                }
                out.insert("type".into(), json!("POLYLINE_3D"));
                out.insert(
                    "vertices".into(),
                    Value::Array(pts.iter().map(|p| json!({ "x": p.get("x"), "y": p.get("y"), "z": or0(num(p.get("z"))).j() })).collect()),
                );
                out.insert("closed".into(), Value::Bool(cc_civil::js::truthy(e.get("closed"))));
            }
            "line" => {
                let (a, b) = (e.get("a"), e.get("b"));
                let g = |p: Option<&Value>, k: &str| num(p.and_then(|p| p.get(k)));
                if !fin(&[g(a, "x"), g(a, "y"), g(b, "x"), g(b, "y")]) {
                    return None;
                }
                out.insert("type".into(), json!("LINE"));
                out.insert("start".into(), json!({ "x": g(a, "x").j(), "y": g(a, "y").j(), "z": or0(g(a, "z")).j() }));
                out.insert("end".into(), json!({ "x": g(b, "x").j(), "y": g(b, "y").j(), "z": or0(g(b, "z")).j() }));
            }
            "arc" | "circle" => {
                let c = e.get("c");
                let (cx, cy, r) = (num(c.and_then(|c| c.get("x"))), num(c.and_then(|c| c.get("y"))), num(e.get("r")));
                if ty == "arc" {
                    let (a0, a1) = (num(e.get("a0")), num(e.get("a1")));
                    if !fin(&[cx, cy, r, a0, a1]) || !(r > 0.0) {
                        return None;
                    }
                    out.insert("type".into(), json!("ARC"));
                    out.insert("center".into(), json!({ "x": cx.j(), "y": cy.j(), "z": 0 }));
                    out.insert("radius".into(), r.j());
                    out.insert("startAngle".into(), a0.j());
                    out.insert("endAngle".into(), a1.j());
                } else {
                    if !fin(&[cx, cy, r]) || !(r > 0.0) {
                        return None;
                    }
                    out.insert("type".into(), json!("CIRCLE"));
                    out.insert("center".into(), json!({ "x": cx.j(), "y": cy.j(), "z": 0 }));
                    out.insert("radius".into(), r.j());
                }
            }
            "point" => {
                let (x, y, z) = (num(e.get("x")), num(e.get("y")), num(e.get("z")));
                if !fin(&[x, y]) {
                    return None;
                }
                out.insert("type".into(), json!("POINT"));
                out.insert("position".into(), json!({ "x": x.j(), "y": y.j(), "z": if z.is_finite() { z.j() } else { json!(0) } }));
            }
            "text" => {
                let (x, y, h) = (num(e.get("x")), num(e.get("y")), num(e.get("h")));
                if !fin(&[x, y]) || !(h > 0.0) || cc_civil::js::string_or_empty(e.get("text")).is_empty() {
                    return None;
                }
                out.insert("type".into(), json!("TEXT"));
                out.extend(text(e));
            }
            _ => return None,
        }
        // move "type" after layer/color like the JS spread ({ ...base, type: … })
        Some(Value::Object(out))
    };
    let ms: Vec<Value> = entities
        .iter()
        .map(|e| {
            conv(e, &mut table_layers, &mut seen).unwrap_or_else(|| {
                let layer = match e.get("layer") {
                    Some(v) if cc_civil::js::truthy(Some(v)) => cc_civil::js::to_js_string(v),
                    _ => "0".into(),
                };
                json!({ "type": "LINE", "layer": layer })
            })
        })
        .collect();
    json!({
        "version": "AC1015", "codepage": "ANSI_1252", "header": header,
        "tables": { "layers": table_layers },
        "blocks": [{ "name": "*Model_Space", "entities": ms }, { "name": "*Paper_Space", "entities": [] }],
        "layouts": [], "objects": [],
    })
}

/// A map picture beside the drawing (0.4 V8).
pub struct Image {
    pub file_name: String,
    pub px_w: f64,
    pub px_h: f64,
    /// (minX, minY, maxX, maxY)
    pub bbox: Option<(f64, f64, f64, f64)>,
    pub layer: Option<String>,
}

/// `writeDwg({ units, layers, entities, created, version, images })` → (bytes, report) or the writer's error.
pub fn write_dwg(
    units: Option<&str>,
    layers: &[Value],
    entities: &[Value],
    created_ms: f64,
    version: &str,
    images: &[Image],
    guid_seq: bool,
) -> Result<(Vec<u8>, Value), String> {
    let mut model = from_civil_list(units, layers, entities);
    let mut objects: Vec<Value> = Vec::new();
    let mut new_layers: Vec<Value> = Vec::new();
    let mut unshift: Vec<Value> = Vec::new();
    let existing: Vec<String> = cc_civil::js::arr(model.get("tables").and_then(|t| t.get("layers")))
        .iter()
        .filter_map(|l| l.get("name").and_then(Value::as_str).map(str::to_string))
        .collect();
    for (i, im) in images.iter().enumerate() {
        let Some((x0, y0, x1, y1)) = im.bbox else { continue };
        if !(im.px_w > 0.0 && im.px_h > 0.0) {
            continue;
        }
        let handle = format!("C1A0{:X}", i.saturating_add(1));
        let du = (x1 - x0) / im.px_w;
        let dv = (y1 - y0) / im.px_h;
        objects.push(json!({ "type": "IMAGEDEF", "handle": handle, "fileName": im.file_name, "size": { "x": im.px_w.j(), "y": im.px_h.j() }, "pixelSize": { "x": du.j(), "y": dv.j() }, "resUnits": 0, "loaded": true, "classVersion": 0 }));
        let layer = im.layer.clone().filter(|l| !l.is_empty()).unwrap_or_else(|| "G-MAP-IMAG".into());
        if !existing.contains(&layer) && !new_layers.iter().any(|l| l.get("name").and_then(Value::as_str) == Some(layer.as_str())) {
            new_layers
                .push(json!({ "name": layer, "color": { "index": 8 }, "off": false, "frozen": false, "locked": false, "plot": true }));
        }
        unshift.push(
            json!({ "type": "IMAGE", "layer": layer, "color": { "index": 256 }, "linetype": "ByLayer", "lineweight": -1, "insert": { "x": x0.j(), "y": y0.j(), "z": 0 },
                "u": { "x": du.j(), "y": 0, "z": 0 }, "v": { "x": 0, "y": dv.j(), "z": 0 }, "size": { "x": im.px_w.j(), "y": im.px_h.j() }, "def": handle, "displayFlags": 7,
                "brightness": 50, "contrast": 50, "fade": 0, "clipping": false, "clip": [], "classVersion": 0 }),
        );
    }
    if let Some(m) = model.as_object_mut() {
        if let Some(Value::Array(o)) = m.get_mut("objects") {
            o.extend(objects);
        }
        if let Some(Value::Array(l)) = m.get_mut("tables").and_then(|t| t.get_mut("layers")) {
            l.extend(new_layers);
        }
        if let Some(Value::Array(b)) = m.get_mut("blocks")
            && let Some(Value::Array(ents)) = b.first_mut().and_then(|ms| ms.get_mut("entities"))
        {
            for e in unshift {
                ents.insert(0, e);
            }
        }
    }
    let v = cc_io_dwg::w_js::from_json(&model, 0);
    let cc_io_dwg::V::Obj(o) = v else { return Err("model".into()) };
    let opts = WriteOpts {
        version: version.to_string(),
        created_ms: Some(created_ms),
        guid: if guid_seq { GuidMode::Sequence } else { GuidMode::Random },
    };
    let (bytes, report) = write_dwg_opts(&o, &opts)?;
    let rj: Value = serde_json::from_str(&cc_io_dwg::V::Obj(report).to_json()).unwrap_or(Value::Null);
    let mut counts = Map::new();
    if let Some(c) = rj.get("counts").and_then(Value::as_object) {
        for (k, n) in c {
            let key = match k.as_str() {
                "LWPOLYLINE" => "lwpolyline",
                "POLYLINE_3D" => "polyline3d",
                "LINE" => "line",
                "ARC" => "arc",
                "CIRCLE" => "circle",
                "POINT" => "point",
                "TEXT" => "text",
                "MTEXT" => "mtext",
                "ELLIPSE" => "ellipse",
                "SOLID" => "solid",
                "HATCH" => "hatch",
                "INSERT" => "insert",
                "ATTDEF" => "attdef",
                "ATTRIB" => "attrib",
                _ => "",
            };
            let key = if key.is_empty() { k.to_lowercase() } else { key.to_string() };
            counts.insert(key, n.clone());
        }
    }
    let report = json!({ "version": version, "counts": Value::Object(counts), "notes": rj.get("notes"), "layers": rj.get("layers"), "objects": rj.get("objects"), "entities": rj.get("entities") });
    let _ = hypot;
    Ok((bytes, report))
}
