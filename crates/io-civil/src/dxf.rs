//! dxf.js — R12 (AC1009) ASCII DXF of the plan linework (2D polylines at their elevation, 3D polylines, lines, arcs,
//! text, points). The entity list is the JSON list scene.js's `toDxfEntities` makes.

use cc_civil::i18n::t;
use cc_civil::js::{J, num_to_string, pad_start, str_to_number};
use cc_civil::tr;
use serde_json::{Map, Value, json};

/// `x.toPrecision(p)` with a fast path: the exact value is rounded half up; only a near-tie needs the full expansion.
pub fn to_precision(x: f64, p: usize) -> String {
    if !x.is_finite() || x == 0.0 {
        return cc_m3d::js::to_precision(x, p);
    }
    let s = format!("{:.*e}", p.saturating_add(2), x.abs());
    let (mant, _) = s.split_once('e').unwrap_or((s.as_str(), "0"));
    let digits: Vec<u8> = mant.bytes().filter(u8::is_ascii_digit).collect();
    let extra = digits.get(p..).unwrap_or(&[]);
    if extra == b"500" || extra == b"499" {
        return cc_m3d::js::to_precision(x, p);
    }
    // not a near-tie:
    let p3 = format!("{:.*e}", p.saturating_sub(1), x.abs());
    // Rust rounds the exact value half to even, which only differs from half up on an exact tie (excluded above)
    let (m, e) = p3.split_once('e').unwrap_or((p3.as_str(), "0"));
    let e: i64 = e.parse().unwrap_or(0);
    let dg: String = m.chars().filter(char::is_ascii_digit).collect();
    let mut out = String::new();
    if x < 0.0 {
        out.push('-');
    }
    let pi = i64::try_from(p).unwrap_or(1);
    if e < -6 || e >= pi {
        out.push_str(dg.get(..1).unwrap_or(""));
        if p > 1 {
            out.push('.');
            out.push_str(dg.get(1..).unwrap_or(""));
        }
        out.push('e');
        out.push(if e < 0 { '-' } else { '+' });
        out.push_str(&e.unsigned_abs().to_string());
        return out;
    }
    if e >= 0 {
        let k = usize::try_from(e).unwrap_or(0).saturating_add(1);
        out.push_str(dg.get(..k).unwrap_or(""));
        if k < dg.len() {
            out.push('.');
            out.push_str(dg.get(k..).unwrap_or(""));
        }
    } else {
        out.push_str("0.");
        for _ in 0..e.unsigned_abs().saturating_sub(1) {
            out.push('0');
        }
        out.push_str(&dg);
    }
    out
}

/// `f(n)`: '0.0' for |n| < 1e-12, else `Number(n.toPrecision(15)).toString()`.
pub fn f(n: f64) -> String {
    if n.abs() < 1e-12 { "0.0".into() } else { num_to_string(str_to_number(&to_precision(n, 15))) }
}

fn aci(name: &str) -> Option<i64> {
    Some(match name {
        "red" => 1,
        "yellow" => 2,
        "green" => 3,
        "cyan" => 4,
        "blue" => 5,
        "magenta" => 6,
        "white" => 7,
        "gray" => 8,
        "orange" => 30,
        "brown" => 34,
        _ => return None,
    })
}

fn deg(a: f64) -> f64 {
    let mut d = a * 180.0 / std::f64::consts::PI;
    d %= 360.0;
    if d < 0.0 {
        d += 360.0;
    }
    d
}

/// Plain text → R12 DXF text: non-ASCII as \U+XXXX (per UTF-16 unit), ° ± as %% codes, line breaks as spaces.
fn encode_text(s: &str) -> String {
    let s = s.replace('°', "%%d").replace('±', "%%p");
    let mut out = String::with_capacity(s.len());
    let mut in_break = false;
    for c in s.chars() {
        if c == '\r' || c == '\n' {
            if !in_break {
                out.push(' ');
            }
            in_break = true;
            continue;
        }
        in_break = false;
        if (' '..='~').contains(&c) {
            out.push(c);
        } else {
            let mut buf = [0u16; 2];
            for u in c.encode_utf16(&mut buf) {
                out.push_str(&format!("\\U+{:04X}", u));
            }
        }
    }
    out
}

/// `writeDxf({ units, layers, entities })` → (text, report).
#[allow(clippy::arithmetic_side_effects)]
pub fn write_dxf(units: &str, layers: &[(String, Value)], entities: &[Value]) -> (String, Value) {
    let mut notes: Vec<String> = Vec::new();
    let mut counts: Vec<(String, u64)> = Vec::new();
    let mut lines: Vec<String> = Vec::with_capacity(entities.len().saturating_mul(16));
    let w = |code: i64, v: String, lines: &mut Vec<String>| {
        lines.push(pad_start(&code.to_string(), 3, ' '));
        lines.push(v);
    };
    let mut used: Vec<(String, String)> = Vec::new();
    let mut used_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut renamed = 0u64;
    let mut lname = |n: &str| -> String {
        if let Some(r) = used_map.get(n) {
            return r.clone();
        }
        let up = n.to_uppercase();
        let mut r: String = up
            .chars()
            .map(|c| if c.is_ascii_uppercase() || c.is_ascii_digit() || c == '$' || c == '_' || c == '-' { c } else { '_' })
            .collect();
        r = cc_civil::js::slice16(&r, 31);
        if r.is_empty() {
            r = "UNNAMED".into();
        }
        if r != n {
            renamed += 1;
        }
        used_map.insert(n.to_string(), r.clone());
        used.push((n.to_string(), r.clone()));
        r
    };
    let mut layer_list: Vec<(String, Value)> = Vec::new();
    let set_layer = |list: &mut Vec<(String, Value)>, k: String, v: Value| match list.iter_mut().find(|(n, _)| *n == k) {
        Some(slot) => slot.1 = v,
        None => list.push((k, v)),
    };
    for (name, color) in layers {
        let c = match color {
            Value::Number(n) => Value::Number(n.clone()),
            Value::String(s) => json!(aci(s).unwrap_or(7)),
            _ => json!(7),
        };
        let n = lname(name);
        set_layer(&mut layer_list, n, c);
    }
    let layer_of = |e: &Value| -> String {
        match e.get("layer") {
            Some(v) if cc_civil::js::truthy(Some(v)) => cc_civil::js::to_js_string(v),
            _ => "0".into(),
        }
    };
    for e in entities {
        let n = lname(&layer_of(e));
        if !layer_list.iter().any(|(k, _)| *k == n) {
            layer_list.push((n, json!(7)));
        }
    }
    if !layer_list.iter().any(|(k, _)| k == "0") {
        layer_list.push(("0".into(), json!(7)));
    }
    let num = |v: Option<&Value>| cc_civil::js::numf(v);
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut grow = |x: f64, y: f64| {
        if x < min_x {
            min_x = x;
        }
        if x > max_x {
            max_x = x;
        }
        if y < min_y {
            min_y = y;
        }
        if y > max_y {
            max_y = y;
        }
    };
    for e in entities {
        let ty = e.get("type").and_then(Value::as_str).unwrap_or("");
        if let Some(pts) = e.get("pts").filter(|p| cc_civil::js::truthy(Some(p))) {
            for p in cc_civil::js::arr(Some(pts)) {
                grow(num(p.get("x")), num(p.get("y")));
            }
        } else if ty == "line" {
            let (a, b) = (e.get("a"), e.get("b"));
            grow(num(a.and_then(|a| a.get("x"))), num(a.and_then(|a| a.get("y"))));
            grow(num(b.and_then(|b| b.get("x"))), num(b.and_then(|b| b.get("y"))));
        } else if ty == "arc" {
            let c = e.get("c");
            let (cx, cy, r) = (num(c.and_then(|c| c.get("x"))), num(c.and_then(|c| c.get("y"))), num(e.get("r")));
            grow(cx - r, cy - r);
            grow(cx + r, cy + r);
        } else if num(e.get("x")).is_finite() {
            grow(num(e.get("x")), num(e.get("y")));
        }
    }
    if !min_x.is_finite() {
        min_x = 0.0;
        min_y = 0.0;
        max_x = 1.0;
        max_y = 1.0;
    }
    let s = |x: &str| x.to_string();
    w(0, s("SECTION"), &mut lines);
    w(2, s("HEADER"), &mut lines);
    w(9, s("$ACADVER"), &mut lines);
    w(1, s("AC1009"), &mut lines);
    w(9, s("$DWGCODEPAGE"), &mut lines);
    w(3, s("ANSI_1252"), &mut lines);
    w(9, s("$INSBASE"), &mut lines);
    w(10, s("0.0"), &mut lines);
    w(20, s("0.0"), &mut lines);
    w(30, s("0.0"), &mut lines);
    w(9, s("$EXTMIN"), &mut lines);
    w(10, f(min_x), &mut lines);
    w(20, f(min_y), &mut lines);
    w(30, s("0.0"), &mut lines);
    w(9, s("$EXTMAX"), &mut lines);
    w(10, f(max_x), &mut lines);
    w(20, f(max_y), &mut lines);
    w(30, s("0.0"), &mut lines);
    w(9, s("$LUNITS"), &mut lines);
    w(70, s("2"), &mut lines);
    w(9, s("$MEASUREMENT"), &mut lines);
    w(70, s(if units == "m" { "1" } else { "0" }), &mut lines);
    w(0, s("ENDSEC"), &mut lines);
    w(0, s("SECTION"), &mut lines);
    w(2, s("TABLES"), &mut lines);
    w(0, s("TABLE"), &mut lines);
    w(2, s("LTYPE"), &mut lines);
    w(70, s("1"), &mut lines);
    w(0, s("LTYPE"), &mut lines);
    w(2, s("CONTINUOUS"), &mut lines);
    w(70, s("0"), &mut lines);
    w(3, s("Solid line"), &mut lines);
    w(72, s("65"), &mut lines);
    w(73, s("0"), &mut lines);
    w(40, s("0.0"), &mut lines);
    w(0, s("ENDTAB"), &mut lines);
    w(0, s("TABLE"), &mut lines);
    w(2, s("LAYER"), &mut lines);
    w(70, layer_list.len().to_string(), &mut lines);
    for (n, c) in &layer_list {
        w(0, s("LAYER"), &mut lines);
        w(2, n.clone(), &mut lines);
        w(70, s("0"), &mut lines);
        w(62, cc_civil::js::to_js_string(c), &mut lines);
        w(6, s("CONTINUOUS"), &mut lines);
    }
    w(0, s("ENDTAB"), &mut lines);
    w(0, s("TABLE"), &mut lines);
    w(2, s("STYLE"), &mut lines);
    w(70, s("1"), &mut lines);
    w(0, s("STYLE"), &mut lines);
    w(2, s("STANDARD"), &mut lines);
    w(70, s("0"), &mut lines);
    w(40, s("0.0"), &mut lines);
    w(41, s("1.0"), &mut lines);
    w(50, s("0.0"), &mut lines);
    w(71, s("0"), &mut lines);
    w(42, s("2.5"), &mut lines);
    w(3, s("txt"), &mut lines);
    w(4, String::new(), &mut lines);
    w(0, s("ENDTAB"), &mut lines);
    w(0, s("ENDSEC"), &mut lines);
    w(0, s("SECTION"), &mut lines);
    w(2, s("BLOCKS"), &mut lines);
    w(0, s("ENDSEC"), &mut lines);
    w(0, s("SECTION"), &mut lines);
    w(2, s("ENTITIES"), &mut lines);
    let count = |k: &str, counts: &mut Vec<(String, u64)>| match counts.iter_mut().find(|(n, _)| n == k) {
        Some(c) => c.1 += 1,
        None => counts.push((k.to_string(), 1)),
    };
    for e in entities {
        let l = lname(&layer_of(e));
        let ty = e.get("type").and_then(Value::as_str).unwrap_or("");
        match ty {
            "poly" | "poly3d" => {
                let pts = cc_civil::js::arr(e.get("pts"));
                if pts.len() < 2 {
                    continue;
                }
                let three = ty == "poly3d";
                let elev = {
                    let v = num(e.get("elev"));
                    if v != 0.0 && !v.is_nan() { v } else { 0.0 }
                };
                let closed = cc_civil::js::truthy(e.get("closed"));
                w(0, s("POLYLINE"), &mut lines);
                w(8, l.clone(), &mut lines);
                w(66, s("1"), &mut lines);
                w(10, s("0.0"), &mut lines);
                w(20, s("0.0"), &mut lines);
                w(30, if three { s("0.0") } else { f(elev) }, &mut lines);
                w(70, ((if closed { 1 } else { 0 }) | (if three { 8 } else { 0 })).to_string(), &mut lines);
                for p in pts {
                    w(0, s("VERTEX"), &mut lines);
                    w(8, l.clone(), &mut lines);
                    w(10, f(num(p.get("x"))), &mut lines);
                    w(20, f(num(p.get("y"))), &mut lines);
                    let z = {
                        let v = num(p.get("z"));
                        if v != 0.0 && !v.is_nan() { v } else { 0.0 }
                    };
                    w(30, if three { f(z) } else { f(elev) }, &mut lines);
                    if three {
                        w(70, s("32"), &mut lines);
                    }
                }
                w(0, s("SEQEND"), &mut lines);
                w(8, l.clone(), &mut lines);
                count(ty, &mut counts);
            }
            "line" => {
                let (a, b) = (e.get("a"), e.get("b"));
                w(0, s("LINE"), &mut lines);
                w(8, l.clone(), &mut lines);
                w(10, f(num(a.and_then(|a| a.get("x")))), &mut lines);
                w(20, f(num(a.and_then(|a| a.get("y")))), &mut lines);
                w(30, s("0.0"), &mut lines);
                w(11, f(num(b.and_then(|b| b.get("x")))), &mut lines);
                w(21, f(num(b.and_then(|b| b.get("y")))), &mut lines);
                w(31, s("0.0"), &mut lines);
                count("line", &mut counts);
            }
            "arc" => {
                let c = e.get("c");
                w(0, s("ARC"), &mut lines);
                w(8, l.clone(), &mut lines);
                w(10, f(num(c.and_then(|c| c.get("x")))), &mut lines);
                w(20, f(num(c.and_then(|c| c.get("y")))), &mut lines);
                w(30, s("0.0"), &mut lines);
                w(40, f(num(e.get("r"))), &mut lines);
                w(50, f(deg(num(e.get("a0")))), &mut lines);
                w(51, f(deg(num(e.get("a1")))), &mut lines);
                count("arc", &mut counts);
            }
            "point" => {
                let z = {
                    let v = num(e.get("z"));
                    if v != 0.0 && !v.is_nan() { v } else { 0.0 }
                };
                w(0, s("POINT"), &mut lines);
                w(8, l.clone(), &mut lines);
                w(10, f(num(e.get("x"))), &mut lines);
                w(20, f(num(e.get("y"))), &mut lines);
                w(30, f(z), &mut lines);
                count("point", &mut counts);
            }
            "text" => {
                let align = e.get("align").and_then(Value::as_str).unwrap_or("");
                let ha = if align == "center" {
                    1
                } else if align == "right" {
                    2
                } else {
                    0
                };
                let (x, y) = (num(e.get("x")), num(e.get("y")));
                w(0, s("TEXT"), &mut lines);
                w(8, l.clone(), &mut lines);
                w(10, f(x), &mut lines);
                w(20, f(y), &mut lines);
                w(30, s("0.0"), &mut lines);
                w(40, f(num(e.get("h"))), &mut lines);
                w(1, encode_text(&cc_civil::js::string_of(e.get("text"))), &mut lines);
                let rot = num(e.get("rot"));
                if rot != 0.0 && !rot.is_nan() {
                    w(50, f(deg(rot)), &mut lines);
                }
                if ha != 0 {
                    w(72, ha.to_string(), &mut lines);
                    w(11, f(x), &mut lines);
                    w(21, f(y), &mut lines);
                    w(31, s("0.0"), &mut lines);
                }
                count("text", &mut counts);
            }
            _ => {}
        }
    }
    w(0, s("ENDSEC"), &mut lines);
    w(0, s("EOF"), &mut lines);
    if renamed > 0 {
        notes.push(tr!("dxf.renamed", n = renamed as f64));
    }
    let _ = t;
    let mut cm = Map::new();
    for (k, v) in counts {
        cm.insert(k, json!(v));
    }
    (format!("{}\r\n", lines.join("\r\n")), json!({ "notes": notes, "counts": Value::Object(cm) }))
}

/// The scene's layers for the DXF/DWG writers: (name, ACI).
pub fn scene_layers() -> Vec<(String, Value)> {
    crate::scene::LAYERS.iter().map(|(n, _, a, _)| (n.to_string(), a.j())).collect()
}
