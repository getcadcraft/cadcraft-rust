//! bom.js (+ the material list of materials.js) — part properties, materials and the bill of materials of an
//! assembly: `MATERIALS`, `presetOf`, `readMaterial`, `readPartProps`, `massOf`, `bomRows`, `csvCell`, `bomCsv`.

use crate::i18n::{F, t};
use crate::js;
use crate::jv;
use serde_json::Value;

pub const MAX_DENSITY: f64 = 25.0;

/// Simulation's materials (materials.js: id, label key, density kg/m³).
pub const SIM_MATERIALS: [(&str, &str, f64); 7] = [
    ("steel", "study.mat.steel", 7850.0),
    ("ss304", "study.mat.ss304", 8000.0),
    ("al6061", "study.mat.al6061", 2700.0),
    ("ti64", "study.mat.ti64", 4430.0),
    ("abs", "study.mat.abs", 1050.0),
    ("pla", "study.mat.pla", 1240.0),
    ("nylon", "study.mat.nylon", 1140.0),
];
const MORE: [(&str, f64); 6] = [
    ("xch.bom.mat.cast_iron", 7.2),
    ("xch.bom.mat.brass", 8.5),
    ("xch.bom.mat.copper", 8.96),
    ("xch.bom.mat.acetal", 1.41),
    ("xch.bom.mat.polycarbonate", 1.2),
    ("xch.bom.mat.oak", 0.75),
];

/// `MATERIALS`: (name now, density g/cm³).
pub fn materials() -> Vec<(String, f64)> {
    let mut v: Vec<(String, f64)> = SIM_MATERIALS.iter().map(|(_, k, d)| (t(k, &[]), d / 1000.0)).collect();
    v.extend(MORE.iter().map(|(k, d)| (t(k, &[]), *d)));
    v
}

/// `presetOf(material)`.
pub fn preset_of(m: Option<&Value>) -> i64 {
    let Some(m) = m.filter(|m| jv::truthy(Some(m))) else { return -1 };
    let list = materials();
    let name = m.get("name");
    let dens = m.get("density");
    let by_name = list.iter().position(|(n, d)| jv::is_str(name, n) && jv::as_num(dens) == Some(*d));
    if let Some(i) = by_name {
        return i64::try_from(i).unwrap_or(-1);
    }
    list.iter()
        .position(|(_, d)| jv::as_num(dens) == Some(*d) && !jv::is_str(name, "Custom"))
        .map_or(-1, |i| i64::try_from(i).unwrap_or(-1))
}

fn clean(v: Option<&Value>, max: usize) -> String {
    js::slice16(js::trim(&js::strip_ctrl(&js::str_of(v))), max)
}

/// `readMaterial(m)`: None for null / absent.
pub fn read_material(m: Option<&Value>) -> Result<Option<Value>, String> {
    let Some(m) = m.filter(|m| !m.is_null()) else { return Ok(None) };
    if !jv::is_obj_like(Some(m)) {
        return Err(t("xch.bom.bad_material", &[]));
    }
    match jv::finite(m.get("density")) {
        Some(d) if d > 0.0 && d <= MAX_DENSITY => {
            let name = clean(m.get("name"), 60);
            let name = if name.is_empty() { "Custom".to_string() } else { name };
            Ok(Some(jv::obj(vec![("name", name.into()), ("density", m.get("density").cloned().unwrap_or(Value::Null))])))
        }
        _ => Err(t("xch.bom.bad_density", &[("max", F::N(MAX_DENSITY))])),
    }
}

/// `readPartProps(p)`.
pub fn read_part_props(p: Option<&Value>) -> Result<Value, String> {
    let mut out = jv::obj(vec![]);
    let Some(p) = p.filter(|p| jv::truthy(Some(p)) && jv::is_obj_like(Some(p))) else { return Ok(out) };
    if p.get("number").is_some() {
        let v = clean(p.get("number"), 40);
        if !v.is_empty() {
            jv::set(&mut out, "number", v.into());
        }
    }
    if p.get("description").is_some() {
        let v = clean(p.get("description"), 200);
        if !v.is_empty() {
            jv::set(&mut out, "description", v.into());
        }
    }
    if let Some(m) = read_material(p.get("material"))? {
        jv::set(&mut out, "material", m);
    }
    Ok(out)
}

/// `massOf(volume, density)`: grams from mm³ and g/cm³.
pub fn mass_of(volume: f64, density: f64) -> f64 {
    volume * density / 1000.0
}

/// `bomRows(asm, builds)`; `volume_of(pid)` = the built part's (volume, exact) when it built.
pub fn bom_rows(asm: &Value, volume_of: &dyn Fn(&str) -> Option<(f64, bool)>) -> Vec<Value> {
    let mut rows: Vec<Value> = Vec::new();
    let mut by_pid: Vec<(String, usize)> = Vec::new();
    let parts = asm.get("parts").cloned().unwrap_or(Value::Null);
    for inst in jv::arr(asm.get("instances")) {
        let pid = js::str_of(inst.get("part"));
        let idx = match by_pid.iter().find(|(k, _)| *k == pid) {
            Some((_, i)) => *i,
            None => {
                let p = parts.get(&pid).cloned().unwrap_or(Value::Null);
                let st = volume_of(&pid);
                let volume = st.map(|s| s.0);
                let mat = p.get("material").filter(|m| jv::truthy(Some(m)));
                let density = mat.and_then(|m| jv::as_num(m.get("density")));
                let mass_each = match (volume, density) {
                    (Some(v), Some(d)) => Some(mass_of(v, d)),
                    _ => None,
                };
                let opt = |x: Option<f64>| x.map_or(Value::Null, jv::num);
                rows.push(jv::obj(vec![
                    ("item", jv::unum(rows.len().saturating_add(1))),
                    ("pid", pid.as_str().into()),
                    ("qty", 0.into()),
                    ("number", p.get("number").filter(|v| jv::truthy(Some(v))).cloned().unwrap_or_else(|| "".into())),
                    ("name", p.get("name").cloned().unwrap_or(Value::Null)),
                    ("description", p.get("description").filter(|v| jv::truthy(Some(v))).cloned().unwrap_or_else(|| "".into())),
                    ("material", mat.and_then(|m| m.get("name").cloned()).unwrap_or_else(|| "".into())),
                    ("density", opt(density)),
                    ("volume", opt(volume)),
                    ("massEach", opt(mass_each)),
                    ("massTotal", Value::Null),
                    ("exact", st.is_some_and(|s| s.1).into()),
                ]));
                by_pid.push((pid.clone(), rows.len().saturating_sub(1)));
                rows.len().saturating_sub(1)
            }
        };
        if let Some(r) = rows.get_mut(idx) {
            let q = jv::n(r, "qty") + 1.0;
            jv::set(r, "qty", jv::num(q));
        }
    }
    for r in &mut rows {
        let me = jv::as_num(r.get("massEach"));
        let q = jv::n(r, "qty");
        jv::set(r, "massTotal", me.map_or(Value::Null, |m| jv::num(m * q)));
    }
    rows
}

/// `csvCell(v, { text })`.
pub fn csv_cell(v: &Value, text: bool) -> String {
    let mut s = match v {
        Value::Null => String::new(),
        x => cc_cad::js::to_js_string(x),
    };
    if text && s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        s.insert(0, '\'');
    }
    let needs = s.contains(['"', ',', '\r', '\n']) || s.chars().next().is_some_and(js::is_ws) || s.chars().last().is_some_and(js::is_ws);
    if needs { format!("\"{}\"", s.replace('"', "\"\"")) } else { s }
}

fn n3(v: Option<f64>) -> Value {
    match v {
        None => "".into(),
        Some(x) => js::num_str(js::fixed_num(x, 3)).into(),
    }
}

pub const BOM_HEADER: [&str; 10] = [
    "Item",
    "Quantity",
    "Part number",
    "Name",
    "Description",
    "Material",
    "Density (g/cm³)",
    "Volume each (mm³)",
    "Mass each (g)",
    "Mass total (g)",
];

/// `bomCsv(rows)`.
pub fn bom_csv(rows: &[Value]) -> String {
    let mut lines = vec![BOM_HEADER.iter().map(|h| csv_cell(&Value::String((*h).into()), true)).collect::<Vec<_>>().join(",")];
    for r in rows {
        let g = |k: &str| r.get(k).cloned().unwrap_or(Value::Null);
        let density = match r.get("density") {
            None | Some(Value::Null) => Value::String(String::new()),
            Some(v) => v.clone(),
        };
        lines.push(
            [
                csv_cell(&g("item"), false),
                csv_cell(&g("qty"), false),
                csv_cell(&g("number"), true),
                csv_cell(&g("name"), true),
                csv_cell(&g("description"), true),
                csv_cell(&g("material"), true),
                csv_cell(&density, false),
                csv_cell(&n3(jv::as_num(r.get("volume"))), false),
                csv_cell(&n3(jv::as_num(r.get("massEach"))), false),
                csv_cell(&n3(jv::as_num(r.get("massTotal"))), false),
            ]
            .join(","),
        );
    }
    let total = if !rows.is_empty() && rows.iter().all(|r| jv::as_num(r.get("massTotal")).is_some()) {
        Some(rows.iter().fold(0.0, |s, r| s + jv::n(r, "massTotal")))
    } else {
        None
    };
    let qty = rows.iter().fold(0.0, |s, r| s + jv::n(r, "qty"));
    lines.push(
        [
            "".into(),
            csv_cell(&jv::num(qty), false),
            "".into(),
            csv_cell(&"Total".into(), true),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            csv_cell(&n3(total), false),
        ]
        .join(","),
    );
    format!("\u{feff}{}\r\n", lines.join("\r\n"))
}
