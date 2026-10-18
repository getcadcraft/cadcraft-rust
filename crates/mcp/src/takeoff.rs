//! TakeoffCraft for assistants — the app's 0.5 `agents.js` (takeoffSummary, takeoffQuantities, takeoffEstimate,
//! itemsList) and tools/cli/headless/takeoff.mjs on cc-takeoff: the same answers as the live window.

use cc_takeoff::model::{self, QOpts};
use serde_json::{Map, Value, json};

/// An open takeoff (a validated project and the file it came from).
#[derive(Debug, Clone)]
pub struct TakeoffDoc {
    pub state: Value,
    pub name: String,
    pub warnings: Vec<String>,
}

/// `Math.round(n * 1e4) / 1e4` for finite numbers.
fn r4(n: f64) -> Value {
    if n.is_finite() { cc_cad::num(cc_cad::js::round(n * 1e4) / 1e4) } else { Value::Null }
}

/// `(cents < 0 ? '-' : '') + '$' + (|cents| / 100).toLocaleString('en-US', {min 2, max 2})`.
pub fn money(cents: f64) -> String {
    let neg = if cents < 0.0 { "-" } else { "" };
    let fixed = cc_cad::js::to_fixed(cents.abs() / 100.0, 2);
    let (int, frac) = fixed.split_once('.').unwrap_or((&fixed, "00"));
    let mut grouped = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && int.len().saturating_sub(i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("{neg}${grouped}.{frac}")
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

/// takeoff.mjs `sheetLabel(key)`.
pub fn sheet_label(state: &Value, key: &str) -> String {
    let (hash, page) = key.split_once(':').unwrap_or((key, ""));
    let files = state.get("files").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    let p = cc_cad::js::str_to_number(page);
    let n = if p.is_finite() && p != 0.0 { p } else { 0.0 } + 1.0;
    let n = cc_cad::js::num_to_string(n);
    match files.iter().find(|f| s(f, "hash") == hash) {
        Some(f) => {
            let pages = f.get("pages").and_then(Value::as_f64).filter(|x| *x != 0.0).unwrap_or(1.0);
            if files.len() > 1 || pages > 1.0 { format!("{}, page {n}", s(f, "name")) } else { s(f, "name").to_string() }
        }
        None => format!("Sheet {}, page {n}", hash.chars().take(8).collect::<String>()),
    }
}

/// `{file, ...takeoffSummary(state, sheetLabel), warnings}`.
pub fn summary(t: &TakeoffDoc) -> Value {
    let mut v = summary_with(&t.state, &|k| sheet_label(&t.state, k));
    if let Some(o) = v.as_object_mut() {
        let mut out = Map::new();
        out.insert("file".into(), Value::String(t.name.clone()));
        out.extend(std::mem::take(o));
        out.insert("warnings".into(), json!(t.warnings));
        return Value::Object(out);
    }
    v
}

/// agents.js `itemsList(state)`.
pub fn items_list(state: &Value) -> Value {
    let items: Vec<Value> = state
        .get("items")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .map(|it| {
            let f = |k: &str| it.get(k).and_then(Value::as_f64).unwrap_or(0.0);
            let comps: Vec<Value> = it
                .get("components")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or(&[])
                .iter()
                .map(|c| json!({"name": c.get("name"), "unit": c.get("unit"), "formula": c.get("formula")}))
                .collect();
            json!({
                "id": it.get("id"), "name": it.get("name"), "type": it.get("type"), "unit": cc_takeoff::units::unit_label(s(it, "unit")),
                "color": it.get("color"), "heightFt": r4(f("heightFt")), "wastePercent": r4(f("waste") / 100.0),
                "materialPerUnit": cc_takeoff::units::format_rate(f("mat")), "laborPerUnit": cc_takeoff::units::format_rate(f("lab")), "components": comps,
            })
        })
        .collect();
    Value::Array(items)
}

/// agents.js `takeoffSummary(state, sheetLabel)`.
pub fn summary_with(st: &Value, label: &dyn Fn(&str) -> String) -> Value {
    let q = model::quantities(st, &QOpts::default());
    let unscaled: f64 = q.list.iter().map(|(_, x)| x.unscaled).sum();
    let arr = |k: &str| st.get(k).and_then(Value::as_array).cloned().unwrap_or_default();
    let files: Vec<Value> =
        arr("files").iter().map(|f| json!({"name": f.get("name"), "pages": f.get("pages"), "sha256": f.get("hash")})).collect();
    let scaled: Vec<Value> = st
        .get("sheets")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter(|(_, v)| v.get("scale").is_some_and(|x| cc_cad::js::truthy(Some(x))))
                .map(|(k, v)| {
                    let sc = &v["scale"];
                    let sc_label = sc
                        .get("label")
                        .filter(|x| cc_cad::js::truthy(Some(x)))
                        .or_else(|| sc.get("preset").filter(|x| cc_cad::js::truthy(Some(x))))
                        .cloned();
                    json!({"sheet": label(k), "scale": sc_label})
                })
                .collect()
        })
        .unwrap_or_default();
    let areas: Vec<Value> = arr("areas").iter().map(|a| a.get("name").cloned().unwrap_or(Value::Null)).collect();
    json!({
        "name": st.get("name"), "items": arr("items").len(), "marks": arr("marks").len(), "files": files,
        "scaledSheets": scaled, "unscaledMarks": unscaled, "bidAreas": areas,
    })
}

/// agents.js `takeoffQuantities(state, sheetLabel, {by})` with takeoff.mjs's sheet labels.
pub fn quantities(t: &TakeoffDoc, by: &str) -> Value {
    quantities_with(&t.state, &|k| sheet_label(&t.state, k), by)
}

/// agents.js `takeoffQuantities(state, sheetLabel, {by})`.
pub fn quantities_with(st: &Value, sheet_label: &dyn Fn(&str) -> String, by: &str) -> Value {
    let q = model::quantities(st, &QOpts::default());
    if by == "sheet" {
        let rows: Vec<Value> = model::sheet_breakdown(st, &q)
            .iter()
            .map(|r| {
                json!({"sheet": sheet_label(s(r, "sheet")), "item": r.get("name"), "quantity": r4(r.get("qty").and_then(Value::as_f64).unwrap_or(f64::NAN)),
                    "unit": r.get("unit"), "marks": r.get("marks")})
            })
            .collect();
        return json!({"rows": rows});
    }
    let items: Vec<Value> = st
        .get("items")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .map(|it| {
            let id = s(it, "id");
            let ty = s(it, "type");
            let mut o = Map::new();
            o.insert("id".into(), json!(id));
            o.insert("name".into(), it.get("name").cloned().unwrap_or(Value::Null));
            o.insert("type".into(), json!(ty));
            o.insert("unit".into(), json!(cc_takeoff::units::unit_label(s(it, "unit"))));
            match q.get(id) {
                Some(t) => {
                    o.insert("quantity".into(), r4(model::item_qty(it, &t.tot)));
                    o.insert("marks".into(), cc_cad::num(t.tot.marks));
                    o.insert("unscaledMarks".into(), cc_cad::num(t.unscaled));
                    o.insert("sheets".into(), json!(t.by_sheet.iter().map(|(k, _)| sheet_label(k)).collect::<Vec<_>>()));
                    if ty == "area" {
                        o.insert("perimeterFt".into(), r4(t.tot.perimeter_ft));
                    }
                }
                None => {
                    o.insert("quantity".into(), r4(0.0));
                }
            }
            let h = it.get("heightFt").and_then(Value::as_f64).unwrap_or(0.0);
            if ty == "linear" && h > 0.0 {
                o.insert("heightFt".into(), r4(h));
            }
            Value::Object(o)
        })
        .collect();
    json!({"items": items})
}

/// agents.js `takeoffEstimate(state)`.
pub fn estimate(t: &TakeoffDoc) -> Value {
    estimate_of(&t.state)
}

/// agents.js `takeoffEstimate(state)`.
pub fn estimate_of(state: &Value) -> Value {
    let e = model::estimate_all(state);
    let rate = |r: &Value, k: &str| cc_takeoff::units::format_rate(r.get(k).and_then(Value::as_f64).unwrap_or(0.0));
    let rows: Vec<Value> = e
        .rows
        .iter()
        .map(|r| {
            let f = |k: &str| r.get(k).and_then(Value::as_f64).unwrap_or(0.0);
            if r.get("error").is_some_and(|x| cc_cad::js::truthy(Some(x))) {
                return json!({"kind": r.get("kind"), "name": r.get("name"), "unit": r.get("unit"), "formula": r.get("formula"), "error": r.get("error")});
            }
            let mut o = Map::new();
            for k in ["kind", "name", "unit"] {
                o.insert(k.into(), r.get(k).cloned().unwrap_or(Value::Null));
            }
            if let Some(fm) = r.get("formula").filter(|x| cc_cad::js::truthy(Some(x))) {
                o.insert("formula".into(), fm.clone());
            }
            o.insert("quantity".into(), r4(f("qty")));
            o.insert("wastePercent".into(), r4(f("waste") / 100.0));
            o.insert("quantityWithWaste".into(), r4(f("qtyW")));
            o.insert("materialPerUnit".into(), json!(rate(r, "mat")));
            o.insert("laborPerUnit".into(), json!(rate(r, "lab")));
            o.insert("materialCents".into(), r.get("matExt").cloned().unwrap_or(Value::Null));
            o.insert("laborCents".into(), r.get("labExt").cloned().unwrap_or(Value::Null));
            o.insert("totalCents".into(), r.get("total").cloned().unwrap_or(Value::Null));
            o.insert("total".into(), json!(money(f("total"))));
            if let Some(u) = r.get("unscaled").filter(|x| cc_cad::js::truthy(Some(x))) {
                o.insert("unscaledMarks".into(), u.clone());
            }
            Value::Object(o)
        })
        .collect();
    json!({
        "rows": rows, "materialCents": cc_cad::num(e.mat_total), "laborCents": cc_cad::num(e.lab_total), "totalCents": cc_cad::num(e.total),
        "total": money(e.total), "errors": cc_cad::num(e.errors), "unscaledMarks": cc_cad::num(e.unscaled), "incomplete": e.incomplete,
    })
}

/// takeoff.mjs `export(kind)`: a CSV (with the BOM, as the app writes it) or the XLSX workbook.
pub fn export(t: &TakeoffDoc, kind: &str, now_ms: f64) -> Result<Vec<u8>, String> {
    match kind {
        "csv" => {
            let e = model::base_estimate(&t.state);
            Ok(format!("\u{feff}{}", cc_io_takeoff::export::estimate_csv(&t.state, &e)).into_bytes())
        }
        "xlsx" => {
            let label = |k: &str| sheet_label(&t.state, k);
            let stamp = cc_io_takeoff::xlsx::Stamp::from_ms(now_ms);
            Ok(cc_io_takeoff::export::estimate_xlsx(&t.state, &label, true, &stamp))
        }
        _ => Err("Exports: csv, xlsx.".into()),
    }
}

/// TakeoffCraft's live tools (agents.js `installAgents`) on the window's takeoff. `sheets` = (key, title) of the open
/// PDFs' pages; `takeoff_export` is the window's own (it writes a file).
pub fn live_run(
    state: &Value,
    sheets: &[(String, String)],
    current: Option<&str>,
    name: &str,
    a: &Value,
    allowed: &[String],
) -> Result<Value, String> {
    let label = |k: &str| sheets.iter().find(|(x, _)| x == k).map_or_else(|| sheet_label(state, k), |(_, t)| t.clone());
    match name {
        "app_info" => {
            let mut o = Map::new();
            o.insert("app".into(), json!("takeoffcraft"));
            o.insert("title".into(), json!("TakeoffCraft"));
            o.insert("allowed".into(), json!(allowed));
            o.insert("sheet".into(), current.map_or(Value::Null, |k| Value::String(label(k))));
            if let Value::Object(s) = summary_with(state, &label) {
                o.extend(s);
            }
            Ok(Value::Object(o))
        }
        "sheets_list" => {
            let marks = state.get("marks").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
            let list: Vec<Value> = sheets
                .iter()
                .map(|(k, t)| {
                    let scale = state.get("sheets").and_then(|s| s.get(k)).and_then(|s| s.get("scale")).and_then(|s| s.get("label")).filter(|x| cc_cad::js::truthy(Some(x))).cloned();
                    json!({"key": k, "sheet": t, "scale": scale, "marks": marks.iter().filter(|m| m.get("sheet").and_then(Value::as_str) == Some(k.as_str())).count()})
                })
                .collect();
            Ok(json!({"sheets": list}))
        }
        "items_list" => Ok(json!({"items": items_list(state)})),
        "takeoff_quantities" => Ok(quantities_with(state, &label, a.get("by").and_then(Value::as_str).unwrap_or("item"))),
        "takeoff_estimate" => Ok(estimate_of(state)),
        other => Err(format!("{other} is not one of TakeoffCraft's tools.")),
    }
}
