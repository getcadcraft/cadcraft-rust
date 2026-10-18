//! MarkupCraft for assistants — the app's 0.5 `agents.js` (markupRow, filterMarkups) and tools/cli/headless/markup.mjs
//! on cc-io-markup's reader (the Rust pdf-read.js): the sheets of a PDF and its markups as MarkupCraft reads them.

use cc_markup::model::Markup;
use serde_json::{Map, Value, json};

/// One markup as the assistant sees it (agents.js `markupRow`, without a measurement: headless).
pub fn markup_row(m: &Markup, sheet_title: &dyn Fn(&str) -> String) -> Value {
    let mut o = Map::new();
    let opt = |s: &str| if s.is_empty() { Value::Null } else { Value::String(s.to_string()) };
    o.insert("id".into(), json!(m.id));
    o.insert("sheet".into(), json!(sheet_title(m.sheet())));
    o.insert("type".into(), json!(m.kind));
    if !m.subject.is_empty() {
        o.insert("subject".into(), json!(m.subject));
    }
    o.insert("text".into(), opt(&m.text));
    o.insert("comment".into(), opt(&m.comment));
    if !m.label.is_empty() {
        o.insert("label".into(), json!(m.label));
    }
    o.insert("status".into(), json!(m.status));
    o.insert("author".into(), opt(&m.author));
    o.insert("created".into(), opt(&m.created));
    o.insert("modified".into(), opt(&m.modified));
    if m.kind == "count" {
        o.insert("count".into(), json!(m.pts().len()));
    }
    if let Some(p) = &m.punch {
        o.insert("punch".into(), json!({"n": cc_cad::num(p.n), "assignee": p.assignee, "due": p.due, "status": p.status}));
    }
    if !m.layer.is_empty() {
        o.insert("layer".into(), json!(m.layer));
    }
    o.insert("readOnly".into(), json!(m.kind == "foreign" || m.locked));
    let replies: Vec<Value> = m
        .replies
        .iter()
        .map(|r| json!({"author": opt(&r.author), "date": opt(&r.date), "text": r.text, "kind": if r.kind.is_empty() { "comment" } else { r.kind.as_str() }}))
        .collect();
    o.insert("replies".into(), Value::Array(replies));
    Value::Object(o)
}

/// agents.js `filterMarkups(list, {sheet, status, type, author}, sheetTitle)`.
pub fn filter_markups<'a>(list: &'a [Markup], a: &Value, sheet_title: &dyn Fn(&str) -> String) -> Vec<&'a Markup> {
    let q = |k: &str| a.get(k).and_then(Value::as_str).filter(|x| !x.is_empty()).map(str::to_lowercase);
    let (sheet, status, ty, author) = (q("sheet"), q("status"), q("type"), q("author"));
    list.iter()
        .filter(|m| {
            if sheet.as_ref().is_some_and(|s| !sheet_title(m.sheet()).to_lowercase().contains(s.as_str())) {
                return false;
            }
            if status.as_ref().is_some_and(|s| m.status.to_lowercase() != *s) {
                return false;
            }
            if ty.as_ref().is_some_and(|s| m.kind.to_lowercase() != *s) {
                return false;
            }
            if author.as_ref().is_some_and(|s| !m.author.to_lowercase().contains(s.as_str())) {
                return false;
            }
            true
        })
        .collect()
}

/// `markups_list`: `{sheets, file, total, offset, markups}` (markup.mjs `sheets()` + `markups()`).
pub fn list(bytes: &[u8], name: &str, a: &Value) -> Result<Value, String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).to_string();
    let mut env = cc_markup::model::Env::default();
    let rec =
        cc_io_markup::read::open_pdf(&mut env, bytes, &cc_io_markup::read::OpenOpts { file_id: "f1".into(), name: base, read_text: true })?;
    let title_of = |key: &str| -> String {
        match rec.pages.iter().find(|p| p.key == key) {
            Some(p) if !p.info.title.is_empty() => p.info.title.clone(),
            Some(p) if !p.info.name.is_empty() => p.info.name.clone(),
            Some(p) => format!("Page {}", p.index.saturating_add(1)),
            None => key.to_string(),
        }
    };
    let r2 = |x: f64| cc_cad::num(cc_cad::js::round(x / 72.0 * 100.0) / 100.0);
    let sheets: Vec<Value> = rec
        .pages
        .iter()
        .map(|p| {
            let n = rec.markups.iter().filter(|m| m.sheet() == p.key).count();
            let opt = |s: &str| if s.is_empty() { Value::Null } else { Value::String(s.to_string()) };
            json!({
                "index": p.index, "sheet": p.info.title, "number": opt(&p.info.sheet_no), "name": opt(&p.info.name), "from": opt(p.info.from),
                "sizeIn": [r2(p.view[2] - p.view[0]), r2(p.view[3] - p.view[1])], "rotate": cc_cad::num(p.rotate), "markups": n,
                "calibrated": rec.calibrations.iter().any(|(k, _)| *k == p.key),
            })
        })
        .collect();
    let hits = filter_markups(&rec.markups, a, &title_of);
    let offset = usize::try_from(a.get("offset").and_then(Value::as_u64).unwrap_or(0)).unwrap_or(usize::MAX);
    let limit = usize::try_from(a.get("limit").and_then(Value::as_u64).unwrap_or(200)).unwrap_or(usize::MAX);
    let rows: Vec<Value> = hits.iter().skip(offset).take(limit).map(|m| markup_row(m, &title_of)).collect();
    Ok(
        json!({"sheets": sheets, "file": name, "total": hits.len(), "offset": a.get("offset").cloned().unwrap_or(json!(0)), "markups": rows}),
    )
}
