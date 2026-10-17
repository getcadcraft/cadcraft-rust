//! dwgout.js — drawing sheets (display lists: mm on paper, y up) as DWG and DXF (0.4 D4): a layer per pen with its
//! lineweight and linetype, LINE / LWPOLYLINE / CIRCLE / solid HATCH / TEXT on an Arial style at the capital height;
//! `sheetModel` → a cc-dwg model for cc-io-dwg's writer, `sheetDxf` → DXF R12 text of our own.

use cc_detail::js::{arr, cos, jnum, num_to_string, round, sin, string_of, trim, truthy};
use serde_json::{Map, Value, json};
use std::f64::consts::PI;

const CAP: f64 = 0.718;

fn r6(v: f64) -> f64 {
    round(v * 1e6) / 1e6
}

/// A pen: layer name, lineweight (1/100 mm), dash pattern, linetype name.
#[derive(Clone, Debug, PartialEq)]
pub struct Pen {
    pub name: String,
    pub w: f64,
    pub dash: Option<Vec<f64>>,
    pub lt: String,
}

fn known_dashed() -> [(&'static [f64], &'static str, &'static str); 6] {
    [
        (&[1.5, 1.0], "DC-HIDDEN", "DC_HIDDEN"),
        (&[4.0, 1.0, 0.6, 1.0], "DC-CENTER", "DC_CENTER"),
        (&[8.0, 1.5, 1.0, 1.5], "DC-GRID", "DC_GRID"),
        (&[2.0, 1.2], "DC-FOOTING", "DC_FOOTING"),
        (&[5.0, 1.5], "DC-LEVEL", "DC_LEVEL"),
        (&[3.0, 1.5], "DC-SLOPED", "DC_SLOPED"),
    ]
}
fn same_dash(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
}
fn num(v: Option<&Value>) -> f64 {
    cc_detail::js::num(v)
}

/// `penOf(it)`; None where the JS would throw (a width no solid pen takes, e.g. NaN).
pub fn pen_of(it: &Value) -> Option<Pen> {
    let w = match it.get("w") {
        None | Some(Value::Null) => 0.25,
        Some(v) => num(Some(v)),
    };
    let dash: Option<Vec<f64>> = match it.get("dash") {
        Some(Value::Array(a)) if !a.is_empty() => Some(a.iter().map(|x| num(Some(x))).collect()),
        _ => None,
    };
    let wr = round(w * 100.0);
    let Some(dash) = dash else {
        let name = if w >= 0.45 {
            "DC-THICK"
        } else if w >= 0.3 {
            "DC-OUTLINE"
        } else if w >= 0.0 {
            "DC-THIN"
        } else {
            return None;
        };
        return Some(Pen { name: name.into(), w: wr, dash: None, lt: "CONTINUOUS".into() });
    };
    if let Some((_, name, lt)) = known_dashed().iter().find(|(d, _, _)| same_dash(d, &dash)) {
        return Some(Pen { name: (*name).into(), w: wr, dash: Some(dash), lt: (*lt).into() });
    }
    let key = format!("DC_DASH_{}", dash.iter().map(|v| num_to_string(r6(*v)).replacen('.', "p", 1)).collect::<Vec<_>>().join("_"));
    Some(Pen { name: key.replace('_', "-"), w: wr, dash: Some(dash), lt: key })
}

fn is_text(it: &Value) -> bool {
    it.get("k").and_then(Value::as_str) == Some("text")
}

/// `layersOf(sheet)`.
pub fn layers_of(sheet: &Value) -> Result<Vec<Pen>, String> {
    let mut out: Vec<Pen> = Vec::new();
    for it in arr(sheet.get("items")) {
        let p = if is_text(it) {
            Pen { name: "DC-TEXT".into(), w: 25.0, dash: None, lt: "CONTINUOUS".into() }
        } else {
            pen_of(it).ok_or("Cannot read properties of undefined (reading 'name')")?
        };
        match out.iter_mut().find(|x| x.name == p.name) {
            Some(x) => x.w = cc_detail::js::max(x.w, p.w),
            None => out.push(p),
        }
    }
    Ok(out)
}

fn dash_text(d: &[f64]) -> String {
    d.iter().map(|v| num_to_string(*v)).collect::<Vec<_>>().join(" ")
}
fn sum(d: &[f64]) -> f64 {
    d.iter().fold(0.0, |s, v| s + v)
}
fn pts_of(it: &Value) -> Vec<[f64; 2]> {
    arr(it.get("pts")).iter().map(|p| [num(p.get(0)), num(p.get(1))]).collect()
}
fn xy(x: f64, y: f64) -> Value {
    json!({ "x": jnum(x), "y": jnum(y) })
}
fn text_of(it: &Value) -> String {
    string_of(it.get("s"))
}

/// `sheetModel(sheet, { title })` → the cc-dwg model as JSON.
pub fn sheet_model(sheet: &Value, title: &str) -> Result<Value, String> {
    let layers = layers_of(sheet)?;
    let linetypes: Vec<Value> = layers
        .iter()
        .filter_map(|l| l.dash.as_ref().map(|d| (l, d)))
        .enumerate()
        .map(|(i, (l, d))| {
            json!({
                "handle": format!("A{:X}", i.saturating_add(1)), "name": l.lt, "description": format!("{} {}", l.name, dash_text(d)),
                "patternLength": jnum(sum(d)),
                "elements": d.iter().enumerate().map(|(k, v)| json!({ "length": jnum(if k % 2 == 1 { -v } else { *v }) })).collect::<Vec<_>>(),
            })
        })
        .collect();
    let base = |name: &str| -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("layer".into(), Value::from(name));
        m.insert("color".into(), json!({ "index": 256 }));
        m.insert("linetype".into(), Value::from("ByLayer"));
        m.insert("lineweight".into(), Value::from(-1));
        m
    };
    let mut ents: Vec<Value> = Vec::new();
    let hatch = |name: &str, vertices: Vec<Value>| -> Value {
        let mut e = base(name);
        e.insert("type".into(), Value::from("HATCH"));
        e.insert("solid".into(), Value::from(true));
        e.insert("pattern".into(), Value::from("SOLID"));
        e.insert("style".into(), Value::from(0));
        e.insert("patternType".into(), Value::from(1));
        e.insert("loops".into(), json!([{ "flags": 1, "polyline": { "vertices": vertices, "closed": true } }]));
        Value::Object(e)
    };
    for it in arr(sheet.get("items")) {
        if is_text(it) {
            let txt = text_of(it);
            if trim(&txt).is_empty() {
                continue;
            }
            let ha = match it.get("align").and_then(Value::as_str) {
                Some("center") => 1,
                Some("right") => 2,
                _ => 0,
            };
            let at = json!({ "x": jnum(r6(num(it.get("x")))), "y": jnum(r6(num(it.get("y")))), "z": 0 });
            let mut e = base("DC-TEXT");
            e.insert("type".into(), Value::from("TEXT"));
            e.insert("insert".into(), at.clone());
            if ha != 0 {
                e.insert("align".into(), at);
            }
            e.insert("hAlign".into(), Value::from(ha));
            e.insert("vAlign".into(), Value::from(0));
            e.insert("height".into(), jnum(r6(num(it.get("h")) * CAP)));
            let rot = num(it.get("rot"));
            e.insert(
                "rotation".into(),
                if truthy(it.get("rot")) && !rot.is_nan() { it.get("rot").cloned().unwrap_or(Value::from(0)) } else { Value::from(0) },
            );
            e.insert("widthFactor".into(), Value::from(1));
            e.insert("oblique".into(), Value::from(0));
            e.insert("style".into(), Value::from("DetailCraft"));
            e.insert("value".into(), Value::from(txt));
            ents.push(Value::Object(e));
            continue;
        }
        let pen = pen_of(it).ok_or("Cannot read properties of undefined (reading 'name')")?;
        let fill = truthy(it.get("fill"));
        if it.get("k").and_then(Value::as_str) == Some("circle") {
            let c = arr(it.get("c"));
            let (cx, cy, rr) = (num(c.first()), num(c.get(1)), num(it.get("r")));
            if fill {
                let mut v = Vec::new();
                for i in 0..48 {
                    let a = 2.0 * PI * f64::from(i) / 48.0;
                    v.push(xy(r6(cx + rr * cos(a)), r6(cy + rr * sin(a))));
                }
                ents.push(hatch(&pen.name, v));
            }
            let mut e = base(&pen.name);
            e.insert("type".into(), Value::from("CIRCLE"));
            e.insert("center".into(), json!({ "x": jnum(r6(cx)), "y": jnum(r6(cy)), "z": 0 }));
            e.insert("radius".into(), jnum(r6(rr)));
            ents.push(Value::Object(e));
            continue;
        }
        let pts: Vec<[f64; 2]> = pts_of(it).into_iter().map(|[x, y]| [r6(x), r6(y)]).collect();
        if pts.len() < 2 {
            continue;
        }
        let closed = truthy(it.get("closed"));
        if fill && pts.len() >= 3 {
            ents.push(hatch(&pen.name, pts.iter().map(|p| xy(p[0], p[1])).collect()));
        }
        let mut e = base(&pen.name);
        if let ([a, b], false) = (pts.as_slice(), closed) {
            e.insert("type".into(), Value::from("LINE"));
            e.insert("start".into(), json!({ "x": jnum(a[0]), "y": jnum(a[1]), "z": 0 }));
            e.insert("end".into(), json!({ "x": jnum(b[0]), "y": jnum(b[1]), "z": 0 }));
        } else {
            e.insert("type".into(), Value::from("LWPOLYLINE"));
            e.insert(
                "vertices".into(),
                Value::Array(pts.iter().map(|p| json!({ "x": jnum(p[0]), "y": jnum(p[1]), "bulge": 0 })).collect()),
            );
            e.insert("closed".into(), Value::from(closed));
            e.insert("elevation".into(), Value::from(0));
        }
        ents.push(Value::Object(e));
    }
    let (w, h) = (sheet.get("w").cloned().unwrap_or(Value::Null), sheet.get("h").cloned().unwrap_or(Value::Null));
    let mut header = Map::new();
    header.insert("INSUNITS".into(), Value::from(4));
    header.insert("MEASUREMENT".into(), Value::from(1));
    header.insert("LUNITS".into(), Value::from(2));
    header.insert("LUPREC".into(), Value::from(2));
    header.insert("LTSCALE".into(), Value::from(1));
    header.insert("LIMMIN".into(), json!({ "x": 0, "y": 0 }));
    header.insert("LIMMAX".into(), json!({ "x": w, "y": h }));
    header.insert("EXTMIN".into(), json!({ "x": 0, "y": 0, "z": 0 }));
    header.insert("EXTMAX".into(), json!({ "x": w, "y": h, "z": 0 }));
    if !title.is_empty() {
        header.insert("PROJECTNAME".into(), Value::from(title));
    }
    let layer_rows: Vec<Value> = layers
        .iter()
        .map(|l| json!({ "name": l.name, "color": { "index": 7 }, "linetype": if l.dash.is_some() { l.lt.clone() } else { "Continuous".into() }, "lineweight": jnum(l.w), "plot": true }))
        .collect();
    Ok(json!({
        "version": "AC1015", "codepage": "ANSI_1252", "header": Value::Object(header),
        "tables": { "linetypes": linetypes, "styles": [{ "name": "DetailCraft", "height": 0, "widthFactor": 1, "oblique": 0, "font": "arial.ttf", "bigFont": "" }], "layers": layer_rows },
        "blocks": [{ "name": "*Model_Space", "entities": ents }, { "name": "*Paper_Space", "entities": [] }],
        "layouts": [], "objects": [],
    }))
}

/// A DXF R12 string value: one line, Windows-1252 range only (any other UTF-16 unit as \U+XXXX).
fn dxf_text(s: &str) -> String {
    let mut one = String::with_capacity(s.len());
    let mut in_run = false;
    for c in s.chars() {
        if c == '\r' || c == '\n' {
            if !in_run {
                one.push(' ');
            }
            in_run = true;
        } else {
            in_run = false;
            one.push(c);
        }
    }
    let mut out = String::with_capacity(one.len());
    for c in one.chars() {
        let u = u32::from(c);
        if (0x20..=0x7e).contains(&u) || (0xa0..=0xff).contains(&u) {
            out.push(c);
        } else {
            let mut buf = [0u16; 2];
            for unit in c.encode_utf16(&mut buf).iter() {
                out.push_str(&format!("\\U+{unit:04X}"));
            }
        }
    }
    out
}
fn fnum(v: f64) -> String {
    let n = r6(v);
    if cc_detail::js::is_integer(n) { crate::num::fixed(n, 1) } else { num_to_string(n) }
}

/// `sheetDxf(sheet, { title })` → { text, counts, notes, layers, title }.
pub fn sheet_dxf(sheet: &Value, title: &str) -> Result<Value, String> {
    let mut out: Vec<String> = Vec::new();
    let pad3 = |c: i32| format!("{c:>3}");
    let g = |out: &mut Vec<String>, code: i32, v: &str| {
        out.push(pad3(code));
        out.push(v.to_string());
    };
    let gn = |out: &mut Vec<String>, code: i32, v: f64| {
        out.push(pad3(code));
        out.push(fnum(v));
    };
    let gi = |out: &mut Vec<String>, code: i32, v: f64| {
        out.push(pad3(code));
        out.push(format!("{:>6}", num_to_string(round(v))));
    };
    // `g(code, v)` with a value straight from the sheet: a number is formatted, anything else is `String(v)` (a NaN the
    // sheet carried arrives as null and prints "null", as in the JS)
    let gv = |out: &mut Vec<String>, code: i32, v: Option<&Value>| {
        out.push(pad3(code));
        out.push(match v {
            Some(Value::Number(n)) => fnum(n.as_f64().unwrap_or(f64::NAN)),
            None => "undefined".into(),
            Some(x) => cc_detail::js::to_js_string(x),
        });
    };
    let layers = layers_of(sheet)?;
    g(&mut out, 0, "SECTION");
    g(&mut out, 2, "HEADER");
    g(&mut out, 9, "$ACADVER");
    g(&mut out, 1, "AC1009");
    g(&mut out, 9, "$INSBASE");
    gn(&mut out, 10, 0.0);
    gn(&mut out, 20, 0.0);
    gn(&mut out, 30, 0.0);
    g(&mut out, 9, "$EXTMIN");
    gn(&mut out, 10, 0.0);
    gn(&mut out, 20, 0.0);
    gn(&mut out, 30, 0.0);
    g(&mut out, 9, "$EXTMAX");
    gv(&mut out, 10, sheet.get("w"));
    gv(&mut out, 20, sheet.get("h"));
    gn(&mut out, 30, 0.0);
    g(&mut out, 9, "$LIMMIN");
    gn(&mut out, 10, 0.0);
    gn(&mut out, 20, 0.0);
    g(&mut out, 9, "$LIMMAX");
    gv(&mut out, 10, sheet.get("w"));
    gv(&mut out, 20, sheet.get("h"));
    g(&mut out, 9, "$LTSCALE");
    gn(&mut out, 40, 1.0);
    g(&mut out, 9, "$TEXTSTYLE");
    g(&mut out, 7, "DETAILCRAFT");
    g(&mut out, 9, "$CLAYER");
    g(&mut out, 8, "0");
    g(&mut out, 0, "ENDSEC");
    g(&mut out, 0, "SECTION");
    g(&mut out, 2, "TABLES");
    let lts: Vec<&Pen> = layers.iter().filter(|l| l.dash.is_some()).collect();
    g(&mut out, 0, "TABLE");
    g(&mut out, 2, "LTYPE");
    gi(&mut out, 70, lts.len() as f64 + 1.0);
    g(&mut out, 0, "LTYPE");
    g(&mut out, 2, "CONTINUOUS");
    gi(&mut out, 70, 0.0);
    g(&mut out, 3, "Solid line");
    // Spec gap: the JS line ends in a comment that swallows `gi(73, 0); g(40, 0);` — CONTINUOUS is written without them
    gi(&mut out, 72, 65.0);
    for l in &lts {
        let d = l.dash.clone().unwrap_or_default();
        g(&mut out, 0, "LTYPE");
        g(&mut out, 2, &l.lt);
        gi(&mut out, 70, 0.0);
        g(&mut out, 3, &format!("{} {}", l.name, dash_text(&d)));
        gi(&mut out, 72, 65.0);
        gi(&mut out, 73, d.len() as f64);
        gn(&mut out, 40, sum(&d));
        for (i, v) in d.iter().enumerate() {
            gn(&mut out, 49, if i % 2 == 1 { -v } else { *v });
        }
    }
    g(&mut out, 0, "ENDTAB");
    g(&mut out, 0, "TABLE");
    g(&mut out, 2, "LAYER");
    gi(&mut out, 70, layers.len() as f64 + 1.0);
    g(&mut out, 0, "LAYER");
    g(&mut out, 2, "0");
    gi(&mut out, 70, 0.0);
    gi(&mut out, 62, 7.0);
    g(&mut out, 6, "CONTINUOUS");
    for l in &layers {
        g(&mut out, 0, "LAYER");
        g(&mut out, 2, &l.name);
        gi(&mut out, 70, 0.0);
        gi(&mut out, 62, 7.0);
        g(&mut out, 6, if l.dash.is_some() { &l.lt } else { "CONTINUOUS" });
    }
    g(&mut out, 0, "ENDTAB");
    g(&mut out, 0, "TABLE");
    g(&mut out, 2, "STYLE");
    gi(&mut out, 70, 2.0);
    for (name, font) in [("STANDARD", "txt"), ("DETAILCRAFT", "arial.ttf")] {
        g(&mut out, 0, "STYLE");
        g(&mut out, 2, name);
        gi(&mut out, 70, 0.0);
        gn(&mut out, 40, 0.0);
        gn(&mut out, 41, 1.0);
        gn(&mut out, 50, 0.0);
        gi(&mut out, 71, 0.0);
        gn(&mut out, 42, 2.5);
        g(&mut out, 3, font);
        g(&mut out, 4, "");
    }
    g(&mut out, 0, "ENDTAB");
    g(&mut out, 0, "ENDSEC");
    g(&mut out, 0, "SECTION");
    g(&mut out, 2, "ENTITIES");
    let mut fills = 0usize;
    let mut counts: Vec<(&str, usize)> = Vec::new();
    let mut count = |t: &'static str| match counts.iter_mut().find(|(k, _)| *k == t) {
        Some(c) => c.1 = c.1.saturating_add(1),
        None => counts.push((t, 1)),
    };
    for it in arr(sheet.get("items")) {
        if is_text(it) {
            let txt = text_of(it);
            if trim(&txt).is_empty() {
                continue;
            }
            let ha = match it.get("align").and_then(Value::as_str) {
                Some("center") => 1.0,
                Some("right") => 2.0,
                _ => 0.0,
            };
            g(&mut out, 0, "TEXT");
            g(&mut out, 8, "DC-TEXT");
            gv(&mut out, 10, it.get("x"));
            gv(&mut out, 20, it.get("y"));
            gn(&mut out, 30, 0.0);
            gn(&mut out, 40, num(it.get("h")) * CAP);
            g(&mut out, 1, &dxf_text(&txt));
            let rot = num(it.get("rot"));
            if truthy(it.get("rot")) {
                gn(&mut out, 50, rot * 180.0 / PI);
            }
            g(&mut out, 7, "DETAILCRAFT");
            if ha != 0.0 {
                gi(&mut out, 72, ha);
                gv(&mut out, 11, it.get("x"));
                gv(&mut out, 21, it.get("y"));
                gn(&mut out, 31, 0.0);
            }
            count("TEXT");
            continue;
        }
        let pen = pen_of(it).ok_or("Cannot read properties of undefined (reading 'name')")?;
        if truthy(it.get("fill")) {
            fills = fills.saturating_add(1);
        }
        if it.get("k").and_then(Value::as_str) == Some("circle") {
            let c = arr(it.get("c"));
            g(&mut out, 0, "CIRCLE");
            g(&mut out, 8, &pen.name);
            gv(&mut out, 10, c.first());
            gv(&mut out, 20, c.get(1));
            gn(&mut out, 30, 0.0);
            gv(&mut out, 40, it.get("r"));
            count("CIRCLE");
            continue;
        }
        let raw = arr(it.get("pts"));
        if raw.len() < 2 {
            continue;
        }
        let closed = truthy(it.get("closed"));
        fn pv(p: &Value, i: usize) -> Option<&Value> {
            p.get(i)
        }
        if let ([a, b], false) = (raw, closed) {
            g(&mut out, 0, "LINE");
            g(&mut out, 8, &pen.name);
            gv(&mut out, 10, pv(a, 0));
            gv(&mut out, 20, pv(a, 1));
            gn(&mut out, 30, 0.0);
            gv(&mut out, 11, pv(b, 0));
            gv(&mut out, 21, pv(b, 1));
            gn(&mut out, 31, 0.0);
            count("LINE");
            continue;
        }
        g(&mut out, 0, "POLYLINE");
        g(&mut out, 8, &pen.name);
        gi(&mut out, 66, 1.0);
        gn(&mut out, 10, 0.0);
        gn(&mut out, 20, 0.0);
        gn(&mut out, 30, 0.0);
        gi(&mut out, 70, if closed { 1.0 } else { 0.0 });
        for p in raw {
            g(&mut out, 0, "VERTEX");
            g(&mut out, 8, &pen.name);
            gv(&mut out, 10, pv(p, 0));
            gv(&mut out, 20, pv(p, 1));
            gn(&mut out, 30, 0.0);
        }
        g(&mut out, 0, "SEQEND");
        g(&mut out, 8, &pen.name);
        count("POLYLINE");
    }
    g(&mut out, 0, "ENDSEC");
    g(&mut out, 0, "EOF");
    let notes = if fills > 0 { json!([{ "key": "dxf.note.fills", "n": fills }]) } else { json!([]) };
    let mut cm = Map::new();
    for (k, n) in counts {
        cm.insert(k.into(), Value::from(n));
    }
    Ok(json!({
        "text": format!("{}\r\n", out.join("\r\n")), "counts": Value::Object(cm), "notes": notes,
        "layers": layers.iter().map(|l| l.name.clone()).collect::<Vec<_>>(), "title": title,
    }))
}

/// The JSON of a pen.
pub fn pen_json(p: &Pen) -> Value {
    json!({ "name": p.name, "w": jnum(p.w), "dash": p.dash.as_ref().map(|d| d.iter().map(|x| jnum(*x)).collect::<Vec<_>>()), "lt": p.lt })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pens_and_r12() {
        assert_eq!(pen_of(&json!({ "w": 0.5 })).unwrap().name, "DC-THICK");
        assert_eq!(pen_of(&json!({ "dash": [2.5, 0.75] })).unwrap().name, "DC-DASH-2p5-0p75");
        assert!(pen_of(&json!({ "w": -1 })).is_none());
        let d = sheet_dxf(&json!({ "w": 420, "h": 297, "items": [{ "k": "path", "pts": [[0, 0], [null, 1]] }] }), "").unwrap();
        let text = d["text"].as_str().unwrap();
        // the JS writes no 73 / 40 for CONTINUOUS (a comment swallowed them) and String(null) for a null coordinate
        assert!(text.contains("Solid line\r\n 72\r\n    65\r\n  0\r\nENDTAB"));
        assert!(text.contains(" 11\r\nnull"));
    }
}
