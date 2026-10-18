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

/// One sheet of the open plan set, as the window knows it.
#[derive(Debug, Clone)]
pub struct LiveSheet {
    pub key: String,
    pub title: String,
    pub width: f64,
    pub height: f64,
    /// the page box [x0, y0, x1, y1]
    pub view: [f64; 4],
    pub file: Option<String>,
}

/// The markup types an assistant may add and the geometry each takes (agents.js `ADDABLE`).
fn addable(t: &str) -> Option<&'static str> {
    Some(match t {
        "rect" | "cloudrect" | "ellipse" | "text" => "rect",
        "note" => "point",
        "line" | "arrow" => "two",
        _ => return None,
    })
}

fn find_sheet<'a>(sheets: &'a [LiveSheet], current: Option<&str>, q: Option<&Value>) -> Result<&'a LiveSheet, String> {
    match q {
        None | Some(Value::Null) => current.and_then(|k| sheets.iter().find(|s| s.key == k)).ok_or_else(|| "No sheet is open.".to_string()),
        Some(Value::String(s)) if s.is_empty() => {
            current.and_then(|k| sheets.iter().find(|x| x.key == k)).ok_or_else(|| "No sheet is open.".to_string())
        }
        Some(Value::Number(n)) => {
            let i = n.as_u64().unwrap_or(0);
            usize::try_from(i)
                .ok()
                .and_then(|i| i.checked_sub(1))
                .and_then(|i| sheets.get(i))
                .ok_or_else(|| format!("Sheets are numbered 1 to {}.", sheets.len()))
        }
        Some(v) => {
            let q = cc_cad::js::to_js_string(v);
            let lc = q.to_lowercase();
            sheets
                .iter()
                .find(|s| s.key == q)
                .or_else(|| sheets.iter().find(|s| s.title.to_lowercase() == lc))
                .or_else(|| sheets.iter().find(|s| s.title.to_lowercase().contains(&lc)))
                .ok_or_else(|| format!("No sheet \"{q}\". sheets_list shows them."))
        }
    }
}

/// MarkupCraft's live tools (agents.js `installAgents`) on the window's store. → (answer, the sheet to show).
#[allow(clippy::too_many_arguments)]
pub fn live_run(
    store: &mut cc_markup::model::Store,
    sheets: &[LiveSheet],
    current: Option<&str>,
    author: &str,
    client: Option<&str>,
    name: &str,
    a: &Value,
    allowed: &[String],
) -> Result<(Value, Option<String>), String> {
    let title = |k: &str| sheets.iter().find(|s| s.key == k).map_or_else(|| k.to_string(), |s| s.title.clone());
    let r2 = |x: f64| cc_cad::num(cc_cad::js::round(x / 72.0 * 100.0) / 100.0);
    match name {
        "app_info" => Ok((
            json!({"app": "markupcraft", "title": "MarkupCraft", "allowed": allowed, "sheets": sheets.len(), "markups": store.all().len(),
                "sheet": current.map(title), "author": if author.is_empty() { Value::Null } else { json!(author) }}),
            None,
        )),
        "sheets_list" => {
            let list: Vec<Value> = sheets
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    json!({"n": i.saturating_add(1), "sheet": s.title, "file": s.file, "sizeIn": [r2(s.width), r2(s.height)],
                        "markups": store.on_sheet(&s.key).len(), "shown": Some(s.key.as_str()) == current})
                })
                .collect();
            Ok((json!({"sheets": list}), None))
        }
        "markups_list" => {
            let all: Vec<Markup> = store.all().into_iter().cloned().collect();
            let hits = filter_markups(&all, a, &title);
            let offset = usize::try_from(a.get("offset").and_then(Value::as_u64).unwrap_or(0)).unwrap_or(usize::MAX);
            let limit = usize::try_from(a.get("limit").and_then(Value::as_u64).unwrap_or(200)).unwrap_or(usize::MAX);
            let rows: Vec<Value> = hits.iter().skip(offset).take(limit).map(|m| markup_row(m, &title)).collect();
            Ok((json!({"total": hits.len(), "offset": a.get("offset").cloned().unwrap_or(json!(0)), "markups": rows}), None))
        }
        "markup_add" => {
            let s = find_sheet(sheets, current, a.get("sheet"))?;
            let ty = a.get("type").and_then(Value::as_str).unwrap_or("");
            let kind = addable(ty).ok_or_else(|| format!("{ty} cannot be added by an assistant."))?;
            let [x0, y0, _, _] = s.view;
            let (w, h) = (s.width, s.height);
            let pt = a.get("units").and_then(Value::as_str) == Some("pt");
            let p = |v: &[f64]| -> [f64; 2] {
                let (x, y) = (v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0));
                if pt { [x0 + x, y0 + y] } else { [x0 + x * w, y0 + y * h] }
            };
            let nums =
                |k: &str| -> Option<Vec<f64>> { a.get(k).and_then(Value::as_array).map(|v| v.iter().filter_map(Value::as_f64).collect()) };
            let text = a.get("text").and_then(Value::as_str).unwrap_or("");
            let who = if author.is_empty() { format!("{} (assistant)", client.unwrap_or("Assistant")) } else { author.to_string() };
            let mut f = serde_json::Map::new();
            f.insert("sheet".into(), json!(s.key));
            f.insert("type".into(), json!(ty));
            f.insert("text".into(), json!(text));
            f.insert("comment".into(), json!(a.get("comment").and_then(Value::as_str).unwrap_or("")));
            f.insert("status".into(), json!(a.get("status").and_then(Value::as_str).unwrap_or("Open")));
            f.insert("author".into(), json!(who));
            if let Some(c) = a.get("color").and_then(Value::as_str) {
                f.insert("style".into(), json!({"color": c}));
            }
            match kind {
                "rect" => {
                    let r = nums("rect").filter(|r| r.len() == 4).ok_or_else(|| format!("{ty} needs rect [x0, y0, x1, y1]."))?;
                    let (pa, pb) = (p(r.get(0..2).unwrap_or(&[])), p(r.get(2..4).unwrap_or(&[])));
                    if (pb[0] - pa[0]).abs() < 1.0 || (pb[1] - pa[1]).abs() < 1.0 {
                        if ty == "text" && text.is_empty() {
                            return Err("A text box needs text.".into());
                        }
                        return Err("The rectangle is too small.".into());
                    }
                    f.insert("rect".into(), json!([pa[0], pa[1], pb[0], pb[1]]));
                }
                "point" => {
                    let at = nums("at").filter(|v| v.len() == 2).ok_or("note needs at [x, y].")?;
                    f.insert("pts".into(), json!([p(&at)]));
                }
                _ => {
                    let (Some(fr), Some(to)) = (nums("from").filter(|v| v.len() == 2), nums("to").filter(|v| v.len() == 2)) else {
                        return Err(format!("{ty} needs from [x, y] and to [x, y]."));
                    };
                    f.insert("pts".into(), json!([p(&fr), p(&to)]));
                }
            }
            if ty == "text" && text.is_empty() {
                return Err("A text box needs text.".into());
            }
            let label = format!("Assistant: add {ty}");
            let made = store.change(&label, |tx| tx.add_fields(&f)).map_err(|e| format!("{e:?}"))?;
            let go = (Some(s.key.as_str()) != current).then(|| s.key.clone());
            Ok((json!({"id": made.id, "sheet": s.title, "type": made.kind, "undo": label}), go))
        }
        other => Err(format!("{other} is not one of MarkupCraft's tools.")),
    }
}
