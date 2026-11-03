//! dwg-props.js: what Properties shows for an object picked in a drawing's 3D view. Reads the drawing model's own
//! entity (never the compiled 3D batches), so what it says is what the file says.
//!
//! `entity_rows(e, units, dyn)` → rows { key (a BimCraft catalog key), value (plain text, or a key when `value_key`) };
//! `attribute_rows(e)` → a block reference's attribute values in file order; `color_text(color)`.
//! Angles in the model are radians, shown in degrees; lengths in drawing units (the units name added to sizes).

use cc_cad::js;
use cc_io_cad::jsv::{self, Ov};
use cc_io_dwg::value::{Obj, V};

use crate::dynamic::DynInfo;

/// One property row.
#[derive(Clone, Debug, PartialEq)]
pub struct PropRow {
    pub key: String,
    pub value: String,
    /// the value is a catalog key (yes / no, "not known", "none")
    pub value_key: bool,
}

/// `num(v)`: '' unless a finite number; 0 decimals from 1e5, 2 from 100, else 4; trailing zeros dropped; "-0" → "0".
pub fn num(v: Ov) -> String {
    match jsv::fin(v) {
        Some(x) => num_f(x),
        None => String::new(),
    }
}
/// [`num`] of a number.
pub fn num_f(v: f64) -> String {
    if !v.is_finite() {
        return String::new();
    }
    let a = v.abs();
    let s = js::to_fixed(
        v,
        if a >= 1e5 {
            0
        } else if a >= 100.0 {
            2
        } else {
            4
        },
    );
    // .replace(/(\.\d*?)0+$/, '$1').replace(/\.$/, '').replace(/^-0$/, '0')
    let s = match s.find('.') {
        Some(_) => {
            let t = s.trim_end_matches('0');
            t.strip_suffix('.').unwrap_or(t).to_string()
        }
        None => s,
    };
    if s == "-0" { "0".into() } else { s }
}

/// `pt(p)`: "x, y, z" (z ?? 0); '' when there is no point.
fn pt(p: Ov) -> String {
    if !jsv::truthy(p) {
        return String::new();
    }
    let z = jsv::get(p, "z");
    let zv = if jsv::nullish(z) { num_f(0.0) } else { num(z) };
    [num(jsv::get(p, "x")), num(jsv::get(p, "y")), zv].join(", ")
}

/// `deg(r)`: degrees with a ° sign; '' unless finite.
fn deg(r: f64) -> String {
    if r.is_finite() { format!("{}°", num_f(r * 180.0 / std::f64::consts::PI)) } else { String::new() }
}

/// `colorText(c)`: 'ByLayer' | 'ByBlock' | 'ACI n' | '#rrggbb'.
pub fn color_text(c: Ov) -> String {
    if !jsv::truthy(c) {
        return "ByLayer".into();
    }
    let rgb = jsv::get(c, "rgb");
    if rgb.is_some() {
        let n = jsv::to_num(rgb);
        // (c.rgb >>> 0) & 0xffffff
        let u = js::int32(n) as i64 as u32 & 0x00ff_ffff;
        return format!("#{u:06x}");
    }
    let idx = jsv::get(c, "index");
    match idx {
        Some(V::Num(n)) if *n == 0.0 => "ByBlock".into(),
        None => "ByLayer".into(),
        Some(V::Num(n)) if *n == 256.0 => "ByLayer".into(),
        other => format!("ACI {}", jsv::to_str(other)),
    }
}

/// `attributeRows(e)`: (tag, value) of each attribute with a tag, in file order.
pub fn attribute_rows(e: &Obj) -> Vec<(String, String)> {
    e.arr("attribs")
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .filter(|a| jsv::truthy(Some(a)) && jsv::truthy(jsv::get(Some(a), "tag")))
        .map(|a| (jsv::to_str(jsv::get(Some(a), "tag")), jsv::str_nn(jsv::get(Some(a), "value"))))
        .collect()
}

/// `entityRows(e, { units, dyn })`.
pub fn entity_rows(e: &Obj, units: &str, dyn_: Option<&DynInfo>) -> Vec<PropRow> {
    let mut rows: Vec<PropRow> = Vec::new();
    let mut add = |key: &str, value: String, value_key: bool| {
        if !value.is_empty() {
            rows.push(PropRow { key: key.to_string(), value, value_key });
        }
    };
    // add(key, v) where v may be undefined / null / '' (skipped) or any value (String(v))
    let addv = |add: &mut dyn FnMut(&str, String, bool), key: &str, v: Ov| {
        if !jsv::nullish(v) {
            add(key, jsv::to_str(v), false);
        }
    };
    let len = |v: f64| -> String {
        if v.is_finite() {
            format!("{}{}", num_f(v), if units.is_empty() { String::new() } else { format!(" {units}") })
        } else {
            String::new()
        }
    };
    let lenv = |v: Ov| -> String { jsv::fin(v).map_or(String::new(), len) };
    let ty = e.str("type").unwrap_or("");
    let g = |k: &str| e.get(k);
    if ty == "UNKNOWN" {
        let n = if jsv::truthy(g("dxfName")) {
            jsv::to_str(g("dxfName"))
        } else if jsv::truthy(g("className")) {
            jsv::to_str(g("className"))
        } else {
            "UNKNOWN".into()
        };
        add("dwg.prop.type", n, false);
    } else {
        addv(&mut add, "dwg.prop.type", g("type"));
    }
    add("dwg.prop.layer", if jsv::truthy(g("layer")) { jsv::to_str(g("layer")) } else { "0".into() }, false);
    add("dwg.prop.color", color_text(g("color")), false);
    if jsv::truthy(g("linetype")) {
        add("dwg.prop.linetype", jsv::to_str(g("linetype")), false);
    }
    addv(&mut add, "dwg.prop.handle", g("handle"));
    if let Some(d) = dyn_ {
        add("dwg.prop.dyn", d.name.clone(), false);
        if !d.states.is_empty() {
            match &d.state {
                Some(s) => add("dwg.prop.dyn.state", s.clone(), false),
                None => add("dwg.prop.dyn.state", "dwg.dyn.unknown".into(), true),
            }
            if d.others.is_empty() {
                add("dwg.prop.dyn.others", "dwg.dyn.none".into(), true);
            } else {
                add("dwg.prop.dyn.others", d.others.join(", "), false);
            }
        }
    }
    let count = |k: &str| e.arr(k).filter(|_| jsv::truthy(g(k))).map_or(0, Vec::len).to_string();
    match ty {
        "LINE" => {
            add("dwg.prop.start", pt(g("start")), false);
            add("dwg.prop.end", pt(g("end")), false);
            if jsv::truthy(g("start")) && jsv::truthy(g("end")) {
                let (s, en) = (g("start"), g("end"));
                let c = |p: Ov, k: &str| jsv::to_num(jsv::get(p, k));
                let cz = |p: Ov| jsv::or(jsv::get(p, "z"), 0.0);
                let l = cc_io_cad::small::hypot3(c(en, "x") - c(s, "x"), c(en, "y") - c(s, "y"), cz(en) - cz(s));
                add("dwg.prop.length", len(l), false);
            }
        }
        "CIRCLE" | "ARC" => {
            add("dwg.prop.center", pt(g("center")), false);
            add("dwg.prop.radius", lenv(g("radius")), false);
            if ty == "ARC" {
                add("dwg.prop.startangle", deg(jsv::to_num(g("startAngle"))), false);
                add("dwg.prop.endangle", deg(jsv::to_num(g("endAngle"))), false);
            }
        }
        "ELLIPSE" => {
            add("dwg.prop.center", pt(g("center")), false);
            add("dwg.prop.majoraxis", pt(g("majorAxis")), false);
            add("dwg.prop.ratio", num(g("ratio")), false);
        }
        "LWPOLYLINE" | "POLYLINE_2D" | "POLYLINE_3D" => {
            add("dwg.prop.vertices", count("vertices"), false);
            add("dwg.prop.closed", if jsv::truthy(g("closed")) { "dwg.yes" } else { "dwg.no" }.into(), true);
            if ty != "POLYLINE_3D" {
                add("dwg.prop.elevation", len(jsv::or(g("elevation"), 0.0)), false);
            }
        }
        "POLYLINE_PFACE" => {
            add("dwg.prop.vertices", count("vertices"), false);
            add("dwg.prop.faces", count("faces"), false);
        }
        "POLYLINE_MESH" => {
            let mn = |k: &str| if jsv::truthy(g(k)) { jsv::to_str(g(k)) } else { "0".into() };
            add("dwg.prop.mesh", format!("{} × {}", mn("m"), mn("n")), false);
        }
        "3DFACE" | "SOLID" | "TRACE" => {
            let pts = e.arr("points").filter(|_| jsv::truthy(g("points"))).map(Vec::as_slice).unwrap_or(&[]);
            for (i, p) in pts.iter().take(4).enumerate() {
                add(&format!("dwg.prop.corner{}", i.saturating_add(1)), pt(Some(p)), false);
            }
        }
        "SPLINE" => {
            addv(&mut add, "dwg.prop.degree", g("degree"));
            add("dwg.prop.control", count("control"), false);
            add("dwg.prop.fit", count("fit"), false);
        }
        "INSERT" | "MINSERT" => {
            addv(&mut add, "dwg.prop.block", g("block"));
            add("dwg.prop.insert", pt(g("insert")), false);
            if jsv::truthy(g("scale")) {
                let s = g("scale");
                let c = |k: &str| {
                    let v = jsv::get(s, k);
                    if jsv::nullish(v) { num_f(1.0) } else { num(v) }
                };
                add("dwg.prop.scale", [c("x"), c("y"), c("z")].join(", "), false);
            }
            add("dwg.prop.rotation", deg(jsv::or(g("rotation"), 0.0)), false);
            let cols = if jsv::truthy(g("cols")) { jsv::to_num(g("cols")) } else { 1.0 };
            let rws = if jsv::truthy(g("rows")) { jsv::to_num(g("rows")) } else { 1.0 };
            if cols * rws > 1.0 {
                let t = |k: &str| if jsv::truthy(g(k)) { jsv::to_str(g(k)) } else { "1".into() };
                add("dwg.prop.array", format!("{} × {}", t("cols"), t("rows")), false);
            }
        }
        "HATCH" => {
            if jsv::truthy(g("solid")) {
                add("dwg.prop.pattern", "SOLID".into(), false);
            } else {
                addv(&mut add, "dwg.prop.pattern", g("pattern"));
            }
            add("dwg.prop.loops", count("loops"), false);
        }
        _ => {
            if ty.starts_with("DIMENSION") || ty.ends_with("_DIMENSION") {
                addv(&mut add, "dwg.prop.block", g("block"));
                add("dwg.prop.measurement", lenv(g("measurement")), false);
            }
        }
    }
    if jsv::truthy(g("thickness")) {
        add("dwg.prop.thickness", lenv(g("thickness")), false);
    }
    let x = g("extrusion");
    if jsv::truthy(x) {
        let c = |k: &str| jsv::or(jsv::get(x, k), 0.0).abs();
        let z = jsv::get(x, "z");
        let zn = if jsv::nullish(z) { 1.0 } else { jsv::to_num(z) };
        if c("x") > 1e-9 || c("y") > 1e-9 || zn < 0.0 {
            add("dwg.prop.extrusion", pt(x), false);
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn o(json: &str) -> Obj {
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        match cc_io_cad::jsv::from_json(&v) {
            V::Obj(o) => o,
            _ => Obj::new(),
        }
    }

    #[test]
    fn numbers_and_rows() {
        assert_eq!(num_f(12.5), "12.5");
        assert_eq!(num_f(-0.00001), "0");
        assert_eq!(num_f(123_456.7), "123457");
        assert_eq!(num_f(150.0), "150");
        assert_eq!(num_f(1.23456), "1.2346");
        let e = o(
            r#"{"type":"LINE","handle":"1F","layer":"A","color":{"index":1},"start":{"x":0,"y":0,"z":0},"end":{"x":3,"y":4,"z":0},"extrusion":{"x":0,"y":0,"z":-1}}"#,
        );
        let rows = entity_rows(&e, "inches", None);
        let get = |k: &str| rows.iter().find(|r| r.key == k).map(|r| r.value.clone());
        assert_eq!(get("dwg.prop.color").as_deref(), Some("ACI 1"));
        assert_eq!(get("dwg.prop.length").as_deref(), Some("5 inches"));
        assert_eq!(get("dwg.prop.extrusion").as_deref(), Some("0, 0, -1"));
        assert_eq!(color_text(Some(&cc_io_cad::jsv::from_json(&serde_json::json!({"rgb": -1})))), "#ffffff");
        let i = o(
            r#"{"type":"INSERT","block":"B","insert":{"x":1,"y":2},"scale":{"x":2},"rotation":1.5707963267948966,"cols":2,"rows":3,"attribs":[{"tag":"T","value":5},{"value":"x"}]}"#,
        );
        let rows = entity_rows(&i, "", None);
        assert!(rows.iter().any(|r| r.key == "dwg.prop.scale" && r.value == "2, 1, 1"));
        assert!(rows.iter().any(|r| r.key == "dwg.prop.rotation" && r.value == "90°"));
        assert!(rows.iter().any(|r| r.key == "dwg.prop.array" && r.value == "2 × 3"));
        assert_eq!(attribute_rows(&i), vec![("T".to_string(), "5".to_string())]);
    }
}
