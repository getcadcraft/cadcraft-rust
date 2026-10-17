//! issues.js — the drawing issue log (0.4 D17): revision letters (A … skipping I and O; after Z: AA), each sheet's
//! status against its last issue, planning an issue, the revision base for clouds (0.4 D19), revisions by number.

use serde_json::{Map, Value, json};

const LETTERS: &str = "ABCDEFGHJKLMNPQRSTUVWXYZ";

/// `nextRev(rev)`.
pub fn next_rev(rev: &str) -> String {
    if rev.is_empty() {
        return "A".into();
    }
    let up = rev.to_uppercase();
    let mut digits: Vec<usize> = Vec::new();
    for c in up.chars() {
        match LETTERS.chars().position(|l| l == c) {
            Some(d) => digits.push(d),
            None => return "A".into(),
        }
    }
    let last = LETTERS.len().saturating_sub(1);
    let mut i = digits.len();
    while i > 0 && digits.get(i - 1) == Some(&last) {
        if let Some(d) = digits.get_mut(i - 1) {
            *d = 0;
        }
        i -= 1;
    }
    if i == 0 {
        digits.insert(0, 0);
    } else if let Some(d) = digits.get_mut(i - 1) {
        *d = d.saturating_add(1);
    }
    digits.iter().filter_map(|d| LETTERS.chars().nth(*d)).collect()
}

/// What issues.js needs of a sheet.
#[derive(Clone, Copy, Debug)]
pub struct SheetRef<'a> {
    pub id: &'a str,
    pub number: &'a str,
    pub hash: &'a str,
    pub prints: Option<&'a Map<String, Value>>,
}

fn s<'a>(v: &'a Value, k: &str) -> Option<&'a Value> {
    v.get(k)
}
fn eq_str(v: Option<&Value>, x: &str) -> bool {
    v.and_then(Value::as_str) == Some(x)
}

/// `sheetLog(issues, id)`.
pub fn sheet_log<'a>(issues: &'a [Value], id: &str) -> Vec<&'a Value> {
    issues.iter().filter(|x| eq_str(s(x, "sheet"), id)).collect()
}

/// A sheet's status.
#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub issued: bool,
    pub rev: Value,
    pub date: Value,
    pub note: Value,
    pub changed: bool,
    pub count: usize,
}
impl Status {
    pub fn j(&self) -> Value {
        json!({ "issued": self.issued, "rev": self.rev, "date": self.date, "note": self.note, "changed": self.changed, "count": self.count })
    }
}

/// `sheetStatus(sheets, issues)` → (id, status) in sheet order (a later sheet of the same id replaces the earlier).
pub fn sheet_status(sheets: &[SheetRef], issues: &[Value]) -> Vec<(String, Status)> {
    let mut out: Vec<(String, Status)> = Vec::new();
    for sh in sheets {
        let log = sheet_log(issues, sh.id);
        let st = match log.last() {
            Some(last) => Status {
                issued: true,
                rev: last.get("rev").cloned().unwrap_or(Value::Null),
                date: last.get("date").cloned().unwrap_or(Value::Null),
                note: last.get("note").cloned().unwrap_or(Value::Null),
                changed: !eq_str(last.get("hash"), sh.hash),
                count: log.len(),
            },
            None => Status { issued: false, rev: json!(""), date: json!(""), note: json!(""), changed: true, count: 0 },
        };
        match out.iter_mut().find(|(k, _)| k == sh.id) {
            Some(slot) => slot.1 = st,
            None => out.push((sh.id.to_string(), st)),
        }
    }
    out
}

/// `planIssue(sheets, issues, ids, { date, note, force })`.
pub fn plan_issue(sheets: &[SheetRef], issues: &[Value], ids: &[String], date: &Value, note: &str, force: bool) -> Vec<Value> {
    let st = sheet_status(sheets, issues);
    let mut out = Vec::new();
    for id in ids {
        let Some(sh) = sheets.iter().find(|x| x.id == id) else { continue };
        let Some((_, s)) = st.iter().find(|(k, _)| k == id) else { continue };
        if !force && s.issued && !s.changed {
            continue;
        }
        let rev = s.rev.as_str().map_or_else(|| crate::js::to_js_string(&s.rev), str::to_string);
        let mut o = Map::new();
        o.insert("sheet".into(), Value::from(id.clone()));
        o.insert("number".into(), Value::from(sh.number));
        o.insert("rev".into(), Value::from(next_rev(&rev)));
        o.insert("date".into(), date.clone());
        o.insert("note".into(), Value::from(crate::js::slice16(note, 60)));
        o.insert("hash".into(), Value::from(sh.hash));
        if let Some(p) = sh.prints {
            o.insert("parts".into(), Value::Object(p.clone()));
        }
        out.push(Value::Object(o));
    }
    out
}

/// `cloudBase(log, hash)` → (parts, rev).
pub fn cloud_base(log: &[&Value], hash: &str) -> Option<(Map<String, Value>, String)> {
    let last = *log.last()?;
    let changed = !eq_str(last.get("hash"), hash);
    let base = if changed { Some(last) } else { log.len().checked_sub(2).and_then(|i| log.get(i)).copied() }?;
    let parts = base.get("parts").filter(|p| crate::js::truthy(Some(p)))?;
    let last_rev = crate::js::string_of(last.get("rev"));
    let rev = if changed { next_rev(&last_rev) } else { last_rev };
    Some((parts.as_object().cloned().unwrap_or_default(), rev))
}

/// `revsByNumber(issues)` → (number, rev), a later issue replacing the earlier.
pub fn revs_by_number(issues: &[Value]) -> Vec<(String, String)> {
    let mut m: Vec<(String, String)> = Vec::new();
    for x in issues {
        let k = crate::js::string_of(x.get("number"));
        let v = crate::js::string_of(x.get("rev"));
        match m.iter_mut().find(|(a, _)| *a == k) {
            Some(slot) => slot.1 = v,
            None => m.push((k, v)),
        }
    }
    m
}
