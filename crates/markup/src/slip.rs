//! slip.js — slip-sheeting (0.4 K13): match a revised set's sheets to the old ones, carry the old markups across,
//! stamp the old sheet "Superseded".

use crate::js::{max, min, round};
use crate::json::markup_value;
use crate::model::Markup;
use serde_json::{Map, Value};

/// Statuses `carryMarkups` can leave behind.
pub const CLOSED: [&str; 3] = ["Accepted", "Rejected", "Completed"];

/// `sheetNoKey(s)`: upper case, spaces / dots / underscores / dashes dropped.
pub fn sheet_no_key(s: &str) -> String {
    s.to_uppercase().chars().filter(|c| !(crate::js::is_ws(*c) || matches!(c, '.' | '_' | '-'))).collect()
}

/// A sheet as `matchSheets` / `carryMarkups` / `supersededStamp` see it.
#[derive(Debug, Clone, PartialEq)]
pub struct SlipSheet {
    pub key: String,
    pub no: String,
    pub file: f64,
    pub page: f64,
    pub width: f64,
    pub height: f64,
    pub view: [f64; 4],
    pub rotate: f64,
}

/// One pair from [`match_sheets`].
#[derive(Debug, Clone, PartialEq)]
pub struct SheetPair {
    pub new_key: String,
    pub old_key: Option<String>,
    /// "number" | "order"
    pub by: Option<&'static str>,
}

fn cmp_num(a: f64, b: f64) -> std::cmp::Ordering {
    a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
}

/// `matchSheets(oldSheets, newSheets)`.
pub fn match_sheets(olds: &[SlipSheet], news: &[SlipSheet]) -> Vec<SheetPair> {
    let mut by_no: Vec<(String, &SlipSheet)> = Vec::new();
    for s in olds {
        let k = sheet_no_key(&s.no);
        if k.is_empty() {
            continue;
        }
        match by_no.iter_mut().find(|(kk, _)| *kk == k) {
            Some(slot) => {
                if s.file > slot.1.file {
                    slot.1 = s;
                }
            }
            None => by_no.push((k, s)),
        }
    }
    let mut out: Vec<SheetPair> = news
        .iter()
        .map(|n| {
            let k = sheet_no_key(&n.no);
            let o = by_no.iter().find(|(kk, _)| *kk == k).map(|(_, s)| *s);
            SheetPair { new_key: n.key.clone(), old_key: o.map(|o| o.key.clone()), by: o.map(|_| "number") }
        })
        .collect();
    let mut files: Vec<f64> = Vec::new();
    for n in news {
        if !files.contains(&n.file) {
            files.push(n.file);
        }
    }
    for f in files {
        let mut pages: Vec<&SlipSheet> = news.iter().filter(|n| n.file == f).collect();
        if pages.iter().any(|n| !sheet_no_key(&n.no).is_empty()) {
            continue;
        }
        let mut old_files: Vec<f64> = Vec::new();
        for s in olds {
            if !old_files.contains(&s.file) {
                old_files.push(s.file);
            }
        }
        old_files.sort_by(|a, b| cmp_num(*b, *a));
        let Some(of) = old_files.iter().copied().find(|x| olds.iter().filter(|s| s.file == *x).count() == pages.len()) else { continue };
        let mut old_pages: Vec<&SlipSheet> = olds.iter().filter(|s| s.file == of).collect();
        old_pages.sort_by(|a, b| cmp_num(a.page, b.page));
        pages.sort_by(|a, b| cmp_num(a.page, b.page));
        for (i, n) in pages.iter().enumerate() {
            if let Some(r) = out.iter_mut().find(|x| x.new_key == n.key)
                && r.old_key.is_none()
            {
                r.old_key = old_pages.get(i).map(|o| o.key.clone());
                r.by = Some("order");
            }
        }
    }
    out
}

/// What [`carry_markups`] gives: markup fields (as the JS `clone` leaves them: JSON values), their sources and counts.
#[derive(Debug, Clone, PartialEq)]
pub struct Carried {
    pub items: Vec<Map<String, Value>>,
    pub sources: Vec<String>,
    pub left: usize,
    pub scaled: usize,
    pub closed: usize,
}

fn same_size(a: &SlipSheet, b: &SlipSheet) -> bool {
    (a.width - b.width).abs() <= 1.0 && (a.height - b.height).abs() <= 1.0
}

/// A number as a JSON value; NaN / ±Infinity as the parity kit's "$NaN" / "$Inf" / "$-Inf" (what the canonical
/// writer prints for them).
pub fn num_value(x: f64) -> Value {
    match serde_json::Number::from_f64(x) {
        Some(n) => Value::Number(n),
        None if x.is_nan() => Value::String("$NaN".into()),
        None if x > 0.0 => Value::String("$Inf".into()),
        None => Value::String("$-Inf".into()),
    }
}

fn js_num(v: Option<&Value>) -> f64 {
    // a value of the JSON clone in arithmetic: null → 0, a number → itself
    match v {
        Some(Value::Null) => 0.0,
        Some(x) => x.as_f64().unwrap_or(f64::NAN),
        None => f64::NAN,
    }
}

/// `carryMarkups(markups, toKey, {from, to, skipClosed})`.
pub fn carry_markups(markups: &[&Markup], to_key: &str, from: Option<&SlipSheet>, to: Option<&SlipSheet>, skip_closed: bool) -> Carried {
    let mut c = Carried { items: Vec::new(), sources: Vec::new(), left: 0, scaled: 0, closed: 0 };
    let (same, k, fv, tv) = match (from, to) {
        (Some(f), Some(t)) if !same_size(f, t) => (false, min(t.width / f.width, t.height / f.height), f.view, t.view),
        _ => (true, 1.0, [0.0; 4], [0.0; 4]),
    };
    let fx = |p: &Value| -> Value {
        if same {
            return p.clone();
        }
        let a = p.as_array();
        let g = |i: usize| js_num(a.and_then(|a| a.get(i)));
        Value::Array(vec![num_value(tv[0] + (g(0) - fv[0]) * k), num_value(tv[1] + (g(1) - fv[1]) * k)])
    };
    for m in markups {
        if m.kind == "foreign" || m.locked {
            c.left = c.left.saturating_add(1);
            continue;
        }
        if skip_closed && CLOSED.contains(&m.status.as_str()) {
            c.closed = c.closed.saturating_add(1);
            continue;
        }
        let Value::Object(mut o) = markup_value(m) else { continue };
        o.remove("id");
        o.insert("sheet".into(), Value::String(to_key.to_string()));
        o.insert("src".into(), Value::Null);
        if let Some(Value::Array(pts)) = o.get("pts") {
            let mapped: Vec<Value> = pts.iter().map(&fx).collect();
            o.insert("pts".into(), Value::Array(mapped));
        }
        if let Some(Value::Array(r)) = o.get("rect") {
            let g = |i: usize| r.get(i).cloned().unwrap_or(Value::Null);
            let a = fx(&Value::Array(vec![g(0), g(1)]));
            let b = fx(&Value::Array(vec![g(2), g(3)]));
            let p = |v: &Value, i: usize| v.get(i).cloned().unwrap_or(Value::Null);
            o.insert("rect".into(), Value::Array(vec![p(&a, 0), p(&a, 1), p(&b, 0), p(&b, 1)]));
        }
        if !same {
            c.scaled = c.scaled.saturating_add(1);
            if let Some(Value::Object(st)) = o.get_mut("style") {
                let w = js_num(st.get("width"));
                let fs = js_num(st.get("fontSize"));
                st.insert("width".into(), num_value(round(w * k * 100.0) / 100.0));
                st.insert("fontSize".into(), num_value(max(4.0, round(fs * k * 10.0) / 10.0)));
            }
        }
        if let Some(Value::Array(reps)) = o.get_mut("replies") {
            for r in reps.iter_mut() {
                if let Value::Object(ro) = r {
                    ro.remove("id");
                }
            }
        }
        c.items.push(o);
        c.sources.push(m.id.clone());
    }
    c
}

/// `supersededStamp(page, {label, author, sheet})` → stamp markup fields.
pub fn superseded_stamp(page: &SlipSheet, label: &str, author: &str, sheet: Option<&str>) -> Map<String, Value> {
    let [x0, y0, x1, y1] = page.view;
    let (w0, h0) = (x1 - x0, y1 - y0);
    let rot = if page.rotate != 0.0 && !page.rotate.is_nan() { page.rotate } else { 0.0 };
    let r = rot % 180.0;
    let turned = r != 0.0 && !r.is_nan();
    let across = if turned { h0 } else { w0 };
    let w = min(across * 0.4, 432.0);
    let h = max(36.0, w * 0.18);
    let (dx, dy) = if turned { (h, w) } else { (w, h) };
    let m = min(w0, h0) * 0.04 + (if turned { dx } else { dy }) / 2.0;
    let c = if rot == 90.0 {
        [x0 + m, (y0 + y1) / 2.0]
    } else if rot == 180.0 {
        [(x0 + x1) / 2.0, y0 + m]
    } else if rot == 270.0 {
        [x1 - m, (y0 + y1) / 2.0]
    } else {
        [(x0 + x1) / 2.0, y1 - m]
    };
    let mut o = Map::new();
    if let Some(s) = sheet {
        o.insert("sheet".into(), Value::String(s.to_string()));
    }
    o.insert("type".into(), Value::String("stamp".into()));
    o.insert("rot".into(), num_value(rot));
    o.insert("author".into(), Value::String(author.to_string()));
    o.insert("label".into(), Value::String(label.to_string()));
    o.insert(
        "rect".into(),
        Value::Array(vec![num_value(c[0] - dx / 2.0), num_value(c[1] - dy / 2.0), num_value(c[0] + dx / 2.0), num_value(c[1] + dy / 2.0)]),
    );
    let mut st = Map::new();
    st.insert("color".into(), Value::String("#c4122f".into()));
    st.insert("width".into(), num_value(3.0));
    st.insert("fill".into(), Value::Null);
    st.insert("fillOpacity".into(), num_value(0.25));
    st.insert("opacity".into(), num_value(1.0));
    st.insert("fontSize".into(), num_value(round(h * 0.45)));
    o.insert("style".into(), Value::Object(st));
    o
}
