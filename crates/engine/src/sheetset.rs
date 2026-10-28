//! sheetset.js — sheet sets (0.4 part 2, track Z): one set brings together sheets (layouts) from many drawing files,
//! numbered and titled, in subsets, published as one PDF. The file `<name>.ccsheets` is JSON; paths are relative to
//! the set's folder with "/" between folders ("\" read as "/"); an absolute path or a drive letter is refused, so a set
//! moves with its folder. Reading treats the file as untrusted: unknown keys dropped, strings checked (no control
//! characters, names ≤ 255, descriptions ≤ 2000), at most 2000 sheets, 200 subsets, 8 levels, 100 properties; ids made
//! unique. The set is kept as the JSON value the JS keeps (so it writes back byte for byte); edits return a new set.
//!
//! Also the title-block fields (Z.3: `fieldValues`, `fillTitleBlocks`), numbering from fields (WL.10) and the sheet
//! list table (Z.6, block CC_SHEETLIST).

use cc_cad::entity::Entity;
use cc_cad::{Doc, js};
use serde_json::{Map, Value, json};

use crate::lang::Lang;

pub const FORMAT: &str = "cadcraft-sheetset";
pub const VERSION: i64 = 1;
pub const EXT: &str = ".ccsheets";
pub const LAYOUT_NAMES: [&str; 4] = ["number-title", "number", "title", "keep"];
const L_SHEETS: usize = 2000;
const L_SUBSETS: usize = 200;
const L_DEPTH: usize = 8;
const L_PROPS: usize = 100;
const L_NAME: usize = 255;
const L_TEXT: usize = 2000;
const L_PATH: usize = 1024;

/// A sheet set that could not be read: the reason, in the reader's words.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetSetError(pub String);

// ---------------------------------------------------------------------------- strings and paths

fn ctrl(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' | '\u{7f}')
}

/// sheetset.js `str(v, max, multi)`.
pub fn str_of(v: Option<&Value>, max: usize, multi: bool) -> String {
    let s = match v {
        None | Some(Value::Null) => return String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => js::num_to_string(n.as_f64().unwrap_or(f64::NAN)),
        _ => return String::new(),
    };
    let mut s: String = s.chars().filter(|c| !ctrl(*c)).collect();
    if !multi {
        let mut o = String::with_capacity(s.len());
        let mut run = false;
        for c in s.chars() {
            if matches!(c, '\r' | '\n' | '\t') {
                if !run {
                    o.push(' ');
                }
                run = true;
            } else {
                o.push(c);
                run = false;
            }
        }
        s = o;
    }
    js::slice16(js::trim(&s), max)
}
fn s(v: Option<&Value>) -> String {
    str_of(v, L_NAME, false)
}

/// `cleanPath(p)`: "/" separators, no absolute path, no drive letter, no URL; '' when it is not one.
pub fn clean_path(p: Option<&Value>) -> String {
    let raw = str_of(p, L_PATH, false).replace('\\', "/");
    let mut s = String::with_capacity(raw.len());
    for c in raw.chars() {
        if c == '/' && s.ends_with('/') {
            continue;
        }
        s.push(c);
    }
    let lower = s.to_ascii_lowercase();
    let b = lower.as_bytes();
    let drive = b.first().is_some_and(u8::is_ascii_lowercase) && b.get(1) == Some(&b':');
    let url = {
        // /^[a-z][a-z0-9+.-]*:\/\//i
        let mut it = lower.char_indices();
        match it.next() {
            Some((_, c)) if c.is_ascii_lowercase() => {
                let rest: String = lower
                    .chars()
                    .skip(1)
                    .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '+' | '.' | '-'))
                    .collect();
                lower.get(rest.len().saturating_add(1)..).is_some_and(|t| t.starts_with("://"))
            }
            _ => false,
        }
    };
    if s.is_empty() || s.starts_with('/') || drive || url {
        return String::new();
    }
    s.split('/').filter(|x| *x != "." && !x.is_empty()).collect::<Vec<_>>().join("/")
}

fn prop_key_ok(k: &str) -> bool {
    let mut cs = k.chars();
    matches!(cs.next(), Some(c) if c.is_ascii_uppercase())
        && k.len() <= 64
        && cs.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// `props(o, notes, where)`.
fn props(o: Option<&Value>, notes: &mut Vec<String>, at: &str, l: &Lang) -> Map<String, Value> {
    let mut out = Map::new();
    let Some(Value::Object(o)) = o else { return out };
    let mut n = 0usize;
    for (k0, v) in o {
        let k: String =
            k0.to_uppercase().chars().map(|c| if c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_' { c } else { '_' }).collect();
        if !prop_key_ok(&k) {
            continue;
        }
        n = n.saturating_add(1);
        if n > L_PROPS {
            notes.push(l.t("sheetset.read.props", &[("where", at.to_string())]));
            break;
        }
        out.insert(k, Value::String(str_of(Some(v), L_TEXT, false)));
    }
    out
}

// ---------------------------------------------------------------------------- the set

/// `newSheetSet(name)`.
pub fn new_sheet_set(name: &str, l: &Lang) -> Value {
    let n = s(Some(&json!(name)));
    json!({ "format": FORMAT, "version": VERSION, "name": if n.is_empty() { l.s("sheetset.h") } else { n }, "properties": {}, "layoutNames": "number-title", "template": null, "sheets": [], "subsets": [] })
}

fn id_ok(v: Option<&Value>) -> Option<String> {
    let s = v?.as_str()?;
    (!s.is_empty() && s.chars().count() <= 40 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .then(|| s.to_string())
}

/// `readSheetSet(text)` → (set, notes); Err with the reason when it is not a sheet set CadCraft can read.
pub fn read_sheet_set(text: &str, l: &Lang) -> Result<(Value, Vec<String>), SheetSetError> {
    let fail = |k: &str| SheetSetError(l.s(k));
    let o: Value = serde_json::from_str(text.strip_prefix('\u{feff}').unwrap_or(text)).map_err(|_| fail("sheetset.read.json"))?;
    if o.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err(fail("sheetset.read.format"));
    }
    let v =
        o.get("version").and_then(|v| v.as_f64()).filter(|v| v.fract() == 0.0 && *v >= 1.0).ok_or_else(|| fail("sheetset.read.format"))?;
    if v > VERSION as f64 {
        return Err(SheetSetError(l.t("sheetset.read.newer", &[("v", js::num_to_string(v))])));
    }
    let mut st = Reader { ids: Vec::new(), n_sheets: 0, n_subsets: 0, dropped: 0, notes: Vec::new() };
    let props_v = props(o.get("properties"), &mut st.notes, "set", l);
    let template = o.get("template").filter(|t| js::truthy(Some(t))).and_then(|t| {
        let f = clean_path(t.get("file"));
        let lay = s(t.get("layout"));
        (!f.is_empty() && !lay.is_empty()).then(|| json!({ "file": f, "layout": lay }))
    });
    let name = s(o.get("name"));
    let layout_names = o.get("layoutNames").and_then(Value::as_str).filter(|x| LAYOUT_NAMES.contains(x)).unwrap_or("number-title");
    let sheets = st.sheets(o.get("sheets"), l);
    let subsets = st.subsets(o.get("subsets"), 1, l);
    let mut set = Map::new();
    set.insert("format".into(), json!(FORMAT));
    set.insert("version".into(), json!(VERSION));
    set.insert("name".into(), json!(if name.is_empty() { l.s("sheetset.h") } else { name }));
    set.insert("properties".into(), Value::Object(props_v));
    set.insert("layoutNames".into(), json!(layout_names));
    set.insert("template".into(), template.unwrap_or(Value::Null));
    set.insert("sheets".into(), Value::Array(sheets));
    set.insert("subsets".into(), Value::Array(subsets));
    if let Some(nb) = clean_numbering(o.get("numbering")) {
        set.insert("numbering".into(), nb);
    }
    if st.dropped > 0 {
        st.notes.push(l.t("sheetset.read.dropped", &[("n", st.dropped.to_string())]));
    }
    Ok((Value::Object(set), st.notes))
}

struct Reader {
    ids: Vec<String>,
    n_sheets: usize,
    n_subsets: usize,
    dropped: usize,
    notes: Vec<String>,
}
impl Reader {
    fn uid(&mut self, want: Option<&Value>, pre: &str) -> String {
        let mut id = id_ok(want).unwrap_or_default();
        if id.is_empty() || self.ids.contains(&id) {
            let mut k = self.ids.len().saturating_add(1);
            loop {
                id = format!("{pre}{k}");
                k = k.saturating_add(1);
                if !self.ids.contains(&id) {
                    break;
                }
            }
        }
        self.ids.push(id.clone());
        id
    }
    fn sheet(&mut self, v: &Value, l: &Lang) -> Option<Value> {
        if !v.is_object() {
            self.dropped = self.dropped.saturating_add(1);
            return None;
        }
        let file = clean_path(v.get("file"));
        let layout = s(v.get("layout"));
        if file.is_empty() || layout.is_empty() {
            self.dropped = self.dropped.saturating_add(1);
            return None;
        }
        self.n_sheets = self.n_sheets.saturating_add(1);
        if self.n_sheets > L_SHEETS {
            self.dropped = self.dropped.saturating_add(1);
            return None;
        }
        let id = self.uid(v.get("id"), "s");
        let p = props(v.get("properties"), &mut self.notes, &layout, l);
        Some(json!({ "id": id, "file": file, "layout": layout, "number": s(v.get("number")), "title": s(v.get("title")),
            "description": str_of(v.get("description"), L_TEXT, true), "properties": p }))
    }
    fn sheets(&mut self, v: Option<&Value>, l: &Lang) -> Vec<Value> {
        v.and_then(Value::as_array).map(|a| a.iter().filter_map(|x| self.sheet(x, l)).collect()).unwrap_or_default()
    }
    fn subsets(&mut self, v: Option<&Value>, depth: usize, l: &Lang) -> Vec<Value> {
        let Some(list) = v.and_then(Value::as_array) else { return Vec::new() };
        if depth > L_DEPTH {
            if !list.is_empty() {
                self.notes.push(l.s("sheetset.read.deep"));
            }
            return Vec::new();
        }
        let mut out = Vec::new();
        for g in list {
            if !g.is_object() {
                continue;
            }
            self.n_subsets = self.n_subsets.saturating_add(1);
            if self.n_subsets > L_SUBSETS {
                self.notes.push(l.s("sheetset.read.subsets"));
                break;
            }
            let id = self.uid(g.get("id"), "g");
            let name = s(g.get("name"));
            let sheets = self.sheets(g.get("sheets"), l);
            let subs = self.subsets(g.get("subsets"), depth.saturating_add(1), l);
            out.push(json!({ "id": id, "name": if name.is_empty() { l.s("sheetset.subset.unnamed") } else { name }, "sheets": sheets, "subsets": subs }));
        }
        out
    }
}

fn arr(v: &Value, k: &str) -> Vec<Value> {
    v.get(k).and_then(Value::as_array).cloned().unwrap_or_default()
}
fn gs(v: &Value, k: &str) -> String {
    match v.get(k) {
        Some(Value::String(s)) => s.clone(),
        None | Some(Value::Null) => String::new(),
        Some(x) => js::to_js_string(x),
    }
}
fn truthy(v: &Value, k: &str) -> bool {
    js::truthy(v.get(k))
}

/// JSON.stringify(v, null, 2).
pub fn stringify2(v: &Value) -> String {
    fn go(v: &Value, ind: usize, out: &mut String, depth: usize) {
        let pad = |n: usize| "  ".repeat(n);
        match v {
            Value::Array(a) if !a.is_empty() && depth < 128 => {
                out.push_str("[\n");
                for (i, x) in a.iter().enumerate() {
                    out.push_str(&pad(ind.saturating_add(1)));
                    go(x, ind.saturating_add(1), out, depth.saturating_add(1));
                    if i.saturating_add(1) < a.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&pad(ind));
                out.push(']');
            }
            Value::Object(m) if !m.is_empty() && depth < 128 => {
                out.push_str("{\n");
                for (i, (k, x)) in m.iter().enumerate() {
                    out.push_str(&pad(ind.saturating_add(1)));
                    out.push_str(&serde_json::to_string(&Value::String(k.clone())).unwrap_or_default());
                    out.push_str(": ");
                    go(x, ind.saturating_add(1), out, depth.saturating_add(1));
                    if i.saturating_add(1) < m.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&pad(ind));
                out.push('}');
            }
            Value::Number(n) => out.push_str(&match n.as_f64() {
                Some(f) => js::num_to_string(f),
                None => n.to_string(),
            }),
            other => out.push_str(&serde_json::to_string(other).unwrap_or_default()),
        }
    }
    let mut out = String::new();
    go(v, 0, &mut out, 0);
    out
}

/// `writeSheetSet(set)`: stable key order, two-space indent, a newline at the end.
pub fn write_sheet_set(set: &Value) -> String {
    fn sheet(x: &Value) -> Value {
        let mut m = Map::new();
        m.insert("id".into(), x.get("id").cloned().unwrap_or(Value::Null));
        m.insert("file".into(), x.get("file").cloned().unwrap_or(Value::Null));
        m.insert("layout".into(), x.get("layout").cloned().unwrap_or(Value::Null));
        m.insert("number".into(), if truthy(x, "number") { x.get("number").cloned().unwrap_or(Value::Null) } else { json!("") });
        m.insert("title".into(), if truthy(x, "title") { x.get("title").cloned().unwrap_or(Value::Null) } else { json!("") });
        if truthy(x, "description") {
            m.insert("description".into(), x.get("description").cloned().unwrap_or(Value::Null));
        }
        if let Some(p) = x.get("properties").and_then(Value::as_object).filter(|p| !p.is_empty()) {
            m.insert("properties".into(), Value::Object(p.clone()));
        }
        Value::Object(m)
    }
    fn sub(g: &Value) -> Value {
        let mut m = Map::new();
        m.insert("id".into(), g.get("id").cloned().unwrap_or(Value::Null));
        m.insert("name".into(), g.get("name").cloned().unwrap_or(Value::Null));
        m.insert("sheets".into(), Value::Array(arr(g, "sheets").iter().map(sheet).collect()));
        let subs = arr(g, "subsets");
        if !subs.is_empty() {
            m.insert("subsets".into(), Value::Array(subs.iter().map(sub).collect()));
        }
        Value::Object(m)
    }
    let mut m = Map::new();
    m.insert("format".into(), json!(FORMAT));
    m.insert("version".into(), json!(VERSION));
    m.insert("name".into(), set.get("name").cloned().unwrap_or(Value::Null));
    m.insert(
        "properties".into(),
        match set.get("properties") {
            Some(p) if js::truthy(Some(p)) => p.clone(),
            _ => json!({}),
        },
    );
    m.insert(
        "layoutNames".into(),
        if truthy(set, "layoutNames") { set.get("layoutNames").cloned().unwrap_or(Value::Null) } else { json!("number-title") },
    );
    if truthy(set, "template") {
        m.insert("template".into(), set.get("template").cloned().unwrap_or(Value::Null));
    }
    if truthy(set, "numbering") {
        m.insert("numbering".into(), set.get("numbering").cloned().unwrap_or(Value::Null));
    }
    m.insert("sheets".into(), Value::Array(arr(set, "sheets").iter().map(sheet).collect()));
    m.insert("subsets".into(), Value::Array(arr(set, "subsets").iter().map(sub).collect()));
    stringify2(&Value::Object(m)) + "\n"
}

// ---------------------------------------------------------------------------- reading the tree

/// One sheet in set order: the sheet, the names / ids of the subsets it is in (outermost first), its index.
#[derive(Clone, Debug, PartialEq)]
pub struct SheetAt {
    pub sheet: Value,
    pub path: Vec<Value>,
    pub index: usize,
}

/// `allSheets(set)`: top-level sheets first, then each subset depth-first.
pub fn all_sheets(set: &Value) -> Vec<SheetAt> {
    fn walk(sheets: &[Value], subsets: &[Value], path: &[Value], out: &mut Vec<SheetAt>, depth: usize) {
        for s in sheets {
            let index = out.len();
            out.push(SheetAt { sheet: s.clone(), path: path.to_vec(), index });
        }
        if depth > 64 {
            return;
        }
        for g in subsets {
            let mut p = path.to_vec();
            p.push(g.clone());
            walk(&arr(g, "sheets"), &arr(g, "subsets"), &p, out, depth.saturating_add(1));
        }
    }
    let mut out = Vec::new();
    walk(&arr(set, "sheets"), &arr(set, "subsets"), &[], &mut out, 0);
    out
}

/// `findSheet(set, id)`.
pub fn find_sheet(set: &Value, id: &str) -> Option<Value> {
    all_sheets(set).into_iter().find(|x| x.sheet.get("id").and_then(Value::as_str) == Some(id)).map(|x| x.sheet)
}

/// `sheetLabel(s)`: "A-101 — Floor plan".
pub fn sheet_label(s: &Value) -> String {
    let n = js::trim(&gs(s, "number")).to_string();
    let t = js::trim(&gs(s, "title")).to_string();
    if !n.is_empty() && !t.is_empty() {
        format!("{n} — {t}")
    } else if !n.is_empty() {
        n
    } else if !t.is_empty() {
        t
    } else {
        gs(s, "layout")
    }
}

/// `layoutNameFor(set, s)`: the tab name the set gives a sheet.
pub fn layout_name_for(set: &Value, s: &Value) -> String {
    let n = js::trim(&gs(s, "number")).to_string();
    let t = js::trim(&gs(s, "title")).to_string();
    let lay = gs(s, "layout");
    match set.get("layoutNames").and_then(Value::as_str).unwrap_or("") {
        "number" => {
            if n.is_empty() {
                lay
            } else {
                n
            }
        }
        "title" => {
            if t.is_empty() {
                lay
            } else {
                t
            }
        }
        "keep" => lay,
        _ => js::slice16(
            &if !n.is_empty() && !t.is_empty() {
                format!("{n} {t}")
            } else if !n.is_empty() {
                n
            } else if !t.is_empty() {
                t
            } else {
                lay
            },
            255,
        ),
    }
}

// ---------------------------------------------------------------------------- edits (each returns a new set)

fn sub_ids(list: &[Value], out: &mut Vec<String>, depth: usize) {
    if depth > 64 {
        return;
    }
    for g in list {
        out.push(gs(g, "id"));
        sub_ids(&arr(g, "subsets"), out, depth.saturating_add(1));
    }
}

/// `freshId(set, pre)`.
fn fresh_id(set: &Value, pre: &str) -> String {
    let mut used: Vec<String> = all_sheets(set).iter().map(|x| gs(&x.sheet, "id")).collect();
    sub_ids(&arr(set, "subsets"), &mut used, 0);
    let mut uniq: Vec<String> = Vec::new();
    for u in used {
        if !uniq.contains(&u) {
            uniq.push(u);
        }
    }
    let mut k = uniq.len().saturating_add(1);
    loop {
        let id = format!("{pre}{k}");
        if !uniq.contains(&id) {
            return id;
        }
        k = k.saturating_add(1);
    }
}

/// The subset with this id (depth-first; the last match wins, as findSubset's walk keeps overwriting).
fn subset_mut<'a>(list: &'a mut Vec<Value>, id: &str) -> Option<&'a mut Value> {
    // findSubset: `if (g.id === id) hit = g; else walk(g.subsets)` — the last one met wins
    let mut path: Option<Vec<usize>> = None;
    fn find(list: &[Value], id: &str, cur: &mut Vec<usize>, hit: &mut Option<Vec<usize>>) {
        for (i, g) in list.iter().enumerate() {
            cur.push(i);
            if g.get("id").and_then(Value::as_str) == Some(id) {
                *hit = Some(cur.clone());
            } else {
                find(g.get("subsets").and_then(Value::as_array).map_or(&[][..], Vec::as_slice), id, cur, hit);
            }
            cur.pop();
        }
    }
    find(list, id, &mut Vec::new(), &mut path);
    let path = path?;
    let mut cur = list;
    let (last, head) = path.split_last()?;
    for i in head {
        cur = cur.get_mut(*i)?.get_mut("subsets")?.as_array_mut()?;
    }
    cur.get_mut(*last)
}

fn sheets_mut<'a>(set: &'a mut Value, subset: Option<&str>) -> Option<&'a mut Vec<Value>> {
    match subset {
        Some(id) => {
            let subs = set.get_mut("subsets")?.as_array_mut()?;
            subset_mut(subs, id)?.get_mut("sheets")?.as_array_mut()
        }
        None => set.get_mut("sheets")?.as_array_mut(),
    }
}

/// `locate(set, id)` → (subset id or None for the set's own list, index).
fn locate(set: &Value, id: &str) -> Option<(Option<String>, usize)> {
    if let Some(i) = arr(set, "sheets").iter().position(|s| s.get("id").and_then(Value::as_str) == Some(id)) {
        return Some((None, i));
    }
    fn walk(list: &[Value], id: &str, hit: &mut Option<(Option<String>, usize)>) {
        for g in list {
            if let Some(i) = arr(g, "sheets").iter().position(|s| s.get("id").and_then(Value::as_str) == Some(id)) {
                *hit = Some((Some(gs(g, "id")), i));
            } else {
                walk(&arr(g, "subsets"), id, hit);
            }
        }
    }
    let mut hit = None;
    walk(&arr(set, "subsets"), id, &mut hit);
    hit
}

fn insert_at(list: &mut Vec<Value>, at: Option<f64>, v: Value) {
    let n = list.len();
    let i = match at {
        None => n,
        Some(a) if a.is_nan() => 0,
        // clamped to 0 … n before the cast
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
        Some(a) => a.clamp(0.0, n as f64) as usize,
    };
    list.insert(i.min(n), v);
}

/// `addSheet(set, sheet, { subset, at })` → (new set, the sheet's id); Err when it has no file or layout.
pub fn add_sheet(set: &Value, sheet: &Value, subset: Option<&str>, at: Option<f64>, l: &Lang) -> Result<(Value, String), SheetSetError> {
    let mut n = set.clone();
    let id = fresh_id(&n, "s");
    let p = sheet.get("properties").and_then(Value::as_object).cloned().unwrap_or_default();
    let sv = json!({ "id": id, "file": clean_path(sheet.get("file")), "layout": s(sheet.get("layout")), "number": s(sheet.get("number")), "title": s(sheet.get("title")),
        "description": str_of(sheet.get("description"), L_TEXT, true), "properties": p });
    if gs(&sv, "file").is_empty() || gs(&sv, "layout").is_empty() {
        return Err(SheetSetError(l.s("sheetset.err.sheet")));
    }
    let has_sub = subset.is_some_and(|g| sheets_mut(&mut n, Some(g)).is_some());
    let list = sheets_mut(&mut n, if has_sub { subset } else { None }).ok_or_else(|| SheetSetError(l.s("sheetset.err.sheet")))?;
    insert_at(list, at, sv);
    Ok((n, id))
}

/// `removeSheet(set, id)`.
pub fn remove_sheet(set: &Value, id: &str) -> Value {
    let mut n = set.clone();
    if let Some((g, i)) = locate(&n, id)
        && let Some(list) = sheets_mut(&mut n, g.as_deref())
        && i < list.len()
    {
        list.remove(i);
    }
    n
}

/// `moveSheet(set, id, { subset, at })`.
pub fn move_sheet(set: &Value, id: &str, subset: Option<&str>, at: Option<f64>) -> Value {
    let mut n = set.clone();
    let Some((g, i)) = locate(&n, id) else { return n };
    let Some(s) = sheets_mut(&mut n, g.as_deref()).filter(|l| i < l.len()).map(|l| l.remove(i)) else { return n };
    let has_sub = subset.is_some_and(|g| sheets_mut(&mut n, Some(g)).is_some());
    if let Some(list) = sheets_mut(&mut n, if has_sub { subset } else { None }) {
        insert_at(list, at, s);
    }
    n
}

/// `updateSheet(set, id, patch)`.
pub fn update_sheet(set: &Value, id: &str, patch: &Value, l: &Lang) -> Value {
    let mut n = set.clone();
    let Some((g, i)) = locate(&n, id) else { return n };
    let Some(sh) = sheets_mut(&mut n, g.as_deref()).and_then(|list| list.get_mut(i)) else { return n };
    let Some(o) = sh.as_object_mut() else { return n };
    for k in ["number", "title", "layout"] {
        if let Some(v) = patch.get(k) {
            o.insert(k.into(), json!(s(Some(v))));
        }
    }
    if let Some(v) = patch.get("file") {
        let f = clean_path(Some(v));
        if !f.is_empty() {
            o.insert("file".into(), json!(f));
        }
    }
    if let Some(v) = patch.get("description") {
        o.insert("description".into(), json!(str_of(Some(v), L_TEXT, true)));
    }
    if let Some(v) = patch.get("properties") {
        let lay = o.get("layout").and_then(Value::as_str).unwrap_or("").to_string();
        o.insert("properties".into(), Value::Object(props(Some(v), &mut Vec::new(), &lay, l)));
    }
    n
}

/// `addSubset(set, name, { parent })` → (new set, the subset's id).
pub fn add_subset(set: &Value, name: &str, parent: Option<&str>, l: &Lang) -> (Value, String) {
    let mut n = set.clone();
    let id = fresh_id(&n, "g");
    let nm = s(Some(&json!(name)));
    let g = json!({ "id": id, "name": if nm.is_empty() { l.s("sheetset.subset.unnamed") } else { nm }, "sheets": [], "subsets": [] });
    let into_parent = parent.and_then(|p| {
        let subs = n.get_mut("subsets")?.as_array_mut()?;
        subset_mut(subs, p)?.get_mut("subsets")?.as_array_mut()
    });
    match into_parent {
        Some(list) => list.push(g),
        None => {
            if let Some(list) = n.get_mut("subsets").and_then(Value::as_array_mut) {
                list.push(g);
            }
        }
    }
    (n, id)
}

/// `renameSubset(set, id, name)`.
pub fn rename_subset(set: &Value, id: &str, name: &str) -> Value {
    let mut n = set.clone();
    let nm = s(Some(&json!(name)));
    if !nm.is_empty()
        && let Some(subs) = n.get_mut("subsets").and_then(Value::as_array_mut)
        && let Some(g) = subset_mut(subs, id).and_then(Value::as_object_mut)
    {
        g.insert("name".into(), json!(nm));
    }
    n
}

/// `removeSubset(set, id)`: its sheets and subsets move up to where it was.
pub fn remove_subset(set: &Value, id: &str) -> Value {
    fn walk(owner: &mut Value, id: &str, depth: usize) -> bool {
        if depth > 64 {
            return false;
        }
        let subs = arr(owner, "subsets");
        if let Some(i) = subs.iter().position(|g| g.get("id").and_then(Value::as_str) == Some(id)) {
            let g = subs.get(i).cloned().unwrap_or(Value::Null);
            if let Some(list) = owner.get_mut("subsets").and_then(Value::as_array_mut) {
                list.splice(i..=i, arr(&g, "subsets"));
            }
            if let Some(list) = owner.get_mut("sheets").and_then(Value::as_array_mut) {
                list.extend(arr(&g, "sheets"));
            }
            return true;
        }
        if let Some(list) = owner.get_mut("subsets").and_then(Value::as_array_mut) {
            for g in list.iter_mut() {
                if walk(g, id, depth.saturating_add(1)) {
                    return true;
                }
            }
        }
        false
    }
    let mut n = set.clone();
    walk(&mut n, id, 0);
    n
}

/// Every sheet of the set, mutably, in set order.
fn for_each_sheet_mut(set: &mut Value, f: &mut dyn FnMut(&mut Value, &[String])) {
    fn walk(owner: &mut Value, path: &mut Vec<String>, f: &mut dyn FnMut(&mut Value, &[String]), depth: usize) {
        if let Some(list) = owner.get_mut("sheets").and_then(Value::as_array_mut) {
            for s in list.iter_mut() {
                f(s, path);
            }
        }
        if depth > 64 {
            return;
        }
        if let Some(list) = owner.get_mut("subsets").and_then(Value::as_array_mut) {
            for g in list.iter_mut() {
                path.push(gs(g, "name"));
                walk(g, path, f, depth.saturating_add(1));
                path.pop();
            }
        }
    }
    walk(set, &mut Vec::new(), f, 0);
}

/// `renumber(set, ids, { prefix, start, step, digits })`.
pub fn renumber(set: &Value, ids: Option<&[String]>, prefix: &str, start: f64, step: f64, digits: f64) -> Value {
    let mut n = set.clone();
    let mut k = if start.is_finite() { start } else { 1.0 };
    let st = if step.is_finite() && step != 0.0 { step } else { 1.0 };
    // `digits | 0` (ToInt32), then at least 0
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]
    let pad = if digits.is_finite() { js::max(0.0, f64::from(digits.rem_euclid(4_294_967_296.0) as u32 as i32)) as usize } else { 0 };
    for_each_sheet_mut(&mut n, &mut |sh, _| {
        if let Some(want) = ids
            && !want.contains(&gs(sh, "id"))
        {
            return;
        }
        let ks = js::num_to_string(k);
        let padded =
            if ks.encode_utf16().count() < pad { format!("{}{ks}", "0".repeat(pad.saturating_sub(ks.encode_utf16().count()))) } else { ks };
        if let Some(o) = sh.as_object_mut() {
            o.insert("number".into(), json!(s(Some(&json!(format!("{prefix}{padded}"))))));
        }
        k += st;
    });
    n
}

// ---------------------------------------------------------------------------- numbering from fields (0.5 WL.10)

/// The `{NAME}` / `{NAME:w}` tokens of a rule: (start, end, name, width).
fn tokens(rule: &str) -> Vec<(usize, usize, String, Option<u32>)> {
    // /\{([A-Za-z][A-Za-z0-9_]{0,40})(?::(\d{1,2}))?\}/g over bytes (every byte it matches is ASCII)
    let b = rule.as_bytes();
    let at = |i: usize| b.get(i).copied().unwrap_or(0);
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        if at(i) == b'{' && at(i.saturating_add(1)).is_ascii_alphabetic() {
            let ns = i.saturating_add(1);
            let mut j = ns.saturating_add(1);
            while (at(j).is_ascii_alphanumeric() || at(j) == b'_') && j.saturating_sub(ns) < 41 {
                j = j.saturating_add(1);
            }
            let name = rule.get(ns..j).unwrap_or("").to_string();
            let mut w = None;
            let mut ok = true;
            if at(j) == b':' {
                let ds = j.saturating_add(1);
                let mut k = ds;
                while at(k).is_ascii_digit() && k.saturating_sub(ds) < 2 {
                    k = k.saturating_add(1);
                }
                if k == ds {
                    ok = false;
                } else {
                    w = rule.get(ds..k).and_then(|x| x.parse().ok());
                    j = k;
                }
            }
            if ok && at(j) == b'}' {
                out.push((i, j.saturating_add(1), name, w));
                i = j.saturating_add(1);
                continue;
            }
        }
        i = i.saturating_add(1);
    }
    out
}

/// `ruleProblem(rule)` → a message key, or None.
pub fn rule_problem(rule: Option<&Value>) -> Option<&'static str> {
    let r = match rule {
        None | Some(Value::Null) => String::new(),
        Some(v) => js::to_js_string(v),
    };
    let r = js::trim(&r);
    if r.is_empty() {
        return Some("sheetset.rule.empty");
    }
    if r.encode_utf16().count() > 120 {
        return Some("sheetset.rule.long");
    }
    if !tokens(r).iter().any(|t| t.2.to_uppercase() == "SEQ") {
        return Some("sheetset.rule.seq");
    }
    None
}

/// `cleanNumbering(v)`.
pub fn clean_numbering(v: Option<&Value>) -> Option<Value> {
    let v = v.filter(|v| v.is_object())?;
    if rule_problem(v.get("rule")).is_some() {
        return None;
    }
    let st = js::to_number_opt(v.get("start"));
    let start = if st.is_finite() { js::max(0.0, js::min(1e6, js::round(st))) } else { 1.0 };
    Some(
        json!({ "rule": js::trim(&js::to_js_string(v.get("rule").unwrap_or(&Value::Null))), "start": cc_cad::entity::num(start), "auto": js::truthy(v.get("auto")) }),
    )
}

fn replace_tokens(rule: &str, f: &mut dyn FnMut(&str, Option<u32>) -> String) -> String {
    let mut out = String::new();
    let mut last = 0usize;
    for (a, b, name, w) in tokens(rule) {
        out.push_str(rule.get(last..a).unwrap_or(""));
        out.push_str(&f(&name, w));
        last = b;
    }
    out.push_str(rule.get(last..).unwrap_or(""));
    out
}

/// `numberByRule(set, numbering)`: every sheet numbered by the rule (sheets keep their number when it can't be used).
pub fn number_by_rule(set: &Value, numbering: Option<&Value>) -> Value {
    let nb = clean_numbering(numbering.or(set.get("numbering")));
    let Some(nb) = nb else { return set.clone() };
    let mut n = set.clone();
    if let Some(o) = n.as_object_mut() {
        o.insert("numbering".into(), nb.clone());
    }
    let rule = gs(&nb, "rule");
    let start = nb.get("start").and_then(Value::as_f64).unwrap_or(1.0);
    let set_props = n.get("properties").and_then(Value::as_object).cloned().unwrap_or_default();
    let mut seq: Vec<(String, f64)> = Vec::new();
    for_each_sheet_mut(&mut n, &mut |sh, path| {
        let own = sh.get("properties").and_then(Value::as_object).cloned().unwrap_or_default();
        let field = |name: &str| -> String {
            let k = name.to_uppercase();
            if k == "SUBSET" {
                return path.last().cloned().unwrap_or_default();
            }
            if let Some((_, v)) = own.iter().find(|(x, _)| x.to_uppercase() == k) {
                return js::to_js_string(v);
            }
            set_props.iter().find(|(x, _)| x.to_uppercase() == k).map(|(_, v)| js::to_js_string(v)).unwrap_or_default()
        };
        let key = replace_tokens(&rule, &mut |name, _| if name.to_uppercase() == "SEQ" { "\u{0}".into() } else { field(name) });
        let k = match seq.iter_mut().find(|(x, _)| *x == key) {
            Some(slot) => {
                slot.1 += 1.0;
                slot.1
            }
            None => {
                seq.push((key, start));
                start
            }
        };
        let text = replace_tokens(&rule, &mut |name, w| {
            if name.to_uppercase() == "SEQ" {
                let ks = js::num_to_string(k);
                let w = w.unwrap_or(0) as usize;
                let len = ks.encode_utf16().count();
                if len < w { format!("{}{ks}", "0".repeat(w.saturating_sub(len))) } else { ks }
            } else {
                field(name)
            }
        });
        if let Some(o) = sh.as_object_mut() {
            o.insert("number".into(), json!(s(Some(&json!(text)))));
        }
    });
    n
}

/// `setProperties(set, p)`.
pub fn set_properties(set: &Value, p: &Value, l: &Lang) -> Value {
    let mut n = set.clone();
    if let Some(o) = n.as_object_mut() {
        o.insert("properties".into(), Value::Object(props(Some(p), &mut Vec::new(), "set", l)));
    }
    n
}

// ---------------------------------------------------------------------------- fields (Z.3)

pub const FIELD_NAMES: [&str; 8] =
    ["SHEETNUMBER", "SHEETTITLE", "SHEETDESC", "SHEETSETNAME", "SHEETCOUNT", "SHEETINDEX", "PROJECTNAME", "PROJECTNUMBER"];
const ALIASES: [(&str, &[&str]); 8] = [
    ("SHEETNUMBER", &["SHEET_NO", "SHEETNO", "SHEET_NUMBER", "DWG_NO", "DWGNO", "DRAWING_NO", "SHEET"]),
    ("SHEETTITLE", &["SHEET_TITLE", "DWG_TITLE", "DRAWING_TITLE", "TITLE"]),
    ("SHEETDESC", &["SHEET_DESC", "DESCRIPTION"]),
    ("SHEETSETNAME", &["SHEET_SET", "SET_NAME", "SETNAME"]),
    ("PROJECTNAME", &["PROJECT", "PROJECT_NAME", "PROJ_NAME", "JOB_NAME"]),
    ("PROJECTNUMBER", &["PROJECT_NO", "PROJECTNO", "PROJECT_NUMBER", "JOB_NO", "JOBNO"]),
    ("SHEETCOUNT", &["SHEETS", "SHEET_COUNT", "TOTAL_SHEETS"]),
    ("SHEETINDEX", &["SHEET_OF", "SHEET_INDEX"]),
];

/// `fieldTag(tag)`: the field an attribute tag stands for ('SHEET_NO' → 'SHEETNUMBER').
pub fn field_tag(tag: &str) -> String {
    let mut k = String::new();
    let mut run = false;
    for c in tag.to_uppercase().chars() {
        if c.is_whitespace() || c == '-' {
            if !run {
                k.push('_');
            }
            run = true;
        } else {
            k.push(c);
            run = false;
        }
    }
    for (f, list) in ALIASES {
        if k == f || list.contains(&k.as_str()) {
            return f.to_string();
        }
    }
    k
}

/// `fieldValues(set, s)`: the built-in fields, then the set's properties, then the sheet's own (they win).
pub fn field_values(set: &Value, sh: &Value, l: &Lang) -> Map<String, Value> {
    let list = all_sheets(set);
    let i = list.iter().position(|x| x.sheet.get("id") == sh.get("id"));
    let props = set.get("properties").and_then(Value::as_object).cloned().unwrap_or_default();
    let mut m = Map::new();
    let name = gs(set, "name");
    m.insert("SHEETSETNAME".into(), json!(name.clone()));
    m.insert("PROJECTNAME".into(), props.get("PROJECTNAME").cloned().unwrap_or_else(|| set.get("name").cloned().unwrap_or(json!(""))));
    m.insert("PROJECTNUMBER".into(), props.get("PROJECTNUMBER").cloned().unwrap_or(json!("")));
    for (k, v) in props {
        m.insert(k, v);
    }
    m.insert("SHEETNUMBER".into(), json!(gs(sh, "number")));
    m.insert("SHEETTITLE".into(), json!(gs(sh, "title")));
    m.insert("SHEETDESC".into(), json!(gs(sh, "description")));
    m.insert("SHEETCOUNT".into(), json!(list.len().to_string()));
    m.insert(
        "SHEETINDEX".into(),
        json!(match i {
            Some(i) => l.t("sheetset.field.index", &[("i", i.saturating_add(1).to_string()), ("n", list.len().to_string())]),
            None => String::new(),
        }),
    );
    for (k, v) in sh.get("properties").and_then(Value::as_object).cloned().unwrap_or_default() {
        m.insert(k, v);
    }
    m
}

/// `fillTitleBlocks(doc, layoutName, values)`: every block reference on that sheet with an attribute whose tag is a
/// field (or an alias) gets the field's value. One undo step → how many attribute values changed.
pub fn fill_title_blocks(doc: &mut Doc, layout: &str, values: &Map<String, Value>, l: &Lang) -> usize {
    let Some(space) = doc.layouts.iter().find(|x| x.name == layout).map(|x| x.space) else { return 0 };
    let mut todo: Vec<(u64, Entity, usize)> = Vec::new();
    for e in doc.space_entities(space) {
        let cc_cad::entity::Geo::Insert(ins) = &e.geo else { continue };
        let Some(blk) = doc.blocks.get(&ins.block) else { continue };
        let Some(defs) = blk.attdefs.as_ref().filter(|a| !a.is_empty()) else { continue };
        let mut o = e.to_obj();
        let mut attrs = o.get("attrs").and_then(Value::as_object).cloned().unwrap_or_default();
        let mut n = 0usize;
        for d in defs {
            if js::truthy(d.get("constant")) {
                continue;
            }
            let tag = gs(d, "tag");
            let f = field_tag(&tag);
            let Some(v) = values.get(&f) else { continue };
            let v = match v {
                Value::Null => String::new(),
                x => js::to_js_string(x),
            };
            let have = match attrs.get(&tag) {
                Some(Value::Null) | None => match d.get("def") {
                    Some(Value::Null) | None => String::new(),
                    Some(x) => js::to_js_string(x),
                },
                Some(x) => js::to_js_string(x),
            };
            if have != v {
                attrs.insert(tag, json!(v));
                n = n.saturating_add(1);
            }
        }
        if n > 0 {
            o.insert("attrs".into(), Value::Object(attrs));
            todo.push((e.id.unwrap_or(0), Entity::from_value(&Value::Object(o)), n));
        }
    }
    if todo.is_empty() {
        return 0;
    }
    let total = todo.iter().map(|x| x.2).sum();
    let _ = doc.change(&l.s("sheetset.undo.fields"), |d| {
        for (id, e, _) in todo {
            d.update_in(id, e, space)?;
        }
        Ok(())
    });
    total
}

// ---------------------------------------------------------------------------- the sheet list table (Z.6)

pub const SHEET_LIST_BLOCK: &str = "CC_SHEETLIST";

/// `sheetListEntities(set, { inch, height })`: the table in block coordinates (top-left at 0,0, rows going down).
pub fn sheet_list_entities(set: &Value, inch: bool, height: Option<f64>, l: &Lang) -> Vec<Value> {
    let k = if inch { 1.0 / 25.4 } else { 1.0 };
    let h = match height {
        Some(x) if x > 0.0 => x,
        _ => 3.5 * k,
    };
    let row_h = h * 2.0;
    let (w1, w2) = (30.0 * k, 110.0 * k);
    let ww = w1 + w2;
    let pad = h * 0.6;
    // (kind, a, b, depth)
    let mut rows: Vec<(&str, String, String, usize)> = vec![("head", l.s("sheetset.list.number"), l.s("sheetset.list.title"), 0)];
    fn walk(sheets: &[Value], subsets: &[Value], depth: usize, rows: &mut Vec<(&str, String, String, usize)>) {
        for sh in sheets {
            let t = gs(sh, "title");
            rows.push(("sheet", gs(sh, "number"), if t.is_empty() { gs(sh, "layout") } else { t }, 0));
        }
        if depth > 64 {
            return;
        }
        for g in subsets {
            rows.push(("subset", String::new(), gs(g, "name"), depth));
            walk(&arr(g, "sheets"), &arr(g, "subsets"), depth.saturating_add(1), rows);
        }
    }
    walk(&arr(set, "sheets"), &arr(set, "subsets"), 0, &mut rows);
    let mut out = Vec::new();
    let line = |out: &mut Vec<Value>, x1: f64, y1: f64, x2: f64, y2: f64| {
        out.push(json!({ "type": "line", "layer": "0", "color": 0, "x1": x1, "y1": y1, "x2": x2, "y2": y2 }))
    };
    let text = |out: &mut Vec<Value>, x: f64, y: f64, s: &str, hh: f64| {
        out.push(
            json!({ "type": "text", "layer": "0", "color": 0, "x": x, "y": y, "h": hh, "rot": 0, "s": s, "ha": "left", "va": "baseline" }),
        )
    };
    text(&mut out, 0.0, row_h * 0.6, &gs(set, "name"), h * 1.4);
    for (i, (kind, a, b, depth)) in rows.iter().enumerate() {
        let y = -(i as f64) * row_h;
        let base = y - row_h + pad;
        line(&mut out, 0.0, y, ww, y);
        if *kind == "subset" {
            text(&mut out, pad + *depth as f64 * h, base, b, h * 1.1);
        } else {
            if !a.is_empty() {
                text(&mut out, pad, base, a, h);
            }
            text(&mut out, w1 + pad, base, b, h);
        }
    }
    let bottom = -(rows.len() as f64) * row_h;
    line(&mut out, 0.0, bottom, ww, bottom);
    line(&mut out, 0.0, 0.0, 0.0, bottom);
    line(&mut out, ww, 0.0, ww, bottom);
    for (i, (kind, ..)) in rows.iter().enumerate() {
        if *kind != "subset" {
            line(&mut out, w1, -(i as f64) * row_h, w1, -((i.saturating_add(1)) as f64) * row_h);
        }
    }
    out
}

fn has_list_insert(doc: &Doc, space: cc_cad::SpaceId) -> bool {
    doc.space_entities(space).any(|e| matches!(&e.geo, cc_cad::entity::Geo::Insert(i) if i.block == SHEET_LIST_BLOCK))
}

/// `placeSheetList(doc, layoutName, set)`: the table on a sheet (top-left of its paper inside a margin) when it is not
/// there yet, else redefined. One undo step → false when there is no such sheet.
pub fn place_sheet_list(doc: &mut Doc, layout: &str, set: &Value, at: Option<(f64, f64)>, l: &Lang) -> bool {
    let Some(lay) = doc.layouts.iter().find(|x| x.name == layout).cloned() else { return false };
    let inch = lay.unit() == Some("in");
    let m = if inch { 0.75 } else { 20.0 };
    let has = has_list_insert(doc, lay.space);
    let ents: Vec<Entity> = sheet_list_entities(set, inch, None, l).iter().map(Entity::from_value).collect();
    let block = cc_cad::doc::Block::new(SHEET_LIST_BLOCK, cc_geom::P2::new(0.0, 0.0), ents);
    let p = at.unwrap_or_else(|| {
        let n = |k: &str| js::to_number_opt(lay.paper.get(k));
        (n("x0") + m, n("y0") + n("h") - m * 2.0)
    });
    let _ = doc.change(&l.s("sheetset.undo.list"), |d| {
        d.define_block(&block)?;
        if !has {
            let e = Entity::from_value(&json!({ "type": "insert", "layer": "0", "color": null, "block": SHEET_LIST_BLOCK, "x": p.0, "y": p.1, "sx": 1, "sy": 1, "rot": 0 }));
            d.add_in(e, lay.space)?;
        }
        Ok(())
    });
    true
}

/// `refreshSheetList(doc, set)`: a drawing holding the sheet list gets its block redefined from the set → changed?
pub fn refresh_sheet_list(doc: &mut Doc, set: &Value, l: &Lang) -> bool {
    let Some(b) = doc.blocks.get(SHEET_LIST_BLOCK).cloned() else { return false };
    let uses = doc.layouts.iter().find(|x| has_list_insert(doc, x.space)).cloned();
    let ents = sheet_list_entities(set, uses.as_ref().is_some_and(|u| u.unit() == Some("in")), None, l);
    let sig_v = |list: &[Value]| {
        let t: Vec<String> = list.iter().filter(|e| e.get("type").and_then(Value::as_str) == Some("text")).map(|e| gs(e, "s")).collect();
        format!("{}|{}", t.join("\n"), list.iter().filter(|e| e.get("type").and_then(Value::as_str) == Some("line")).count())
    };
    let have: Vec<Value> = b.entities.iter().map(Entity::to_value).collect();
    if sig_v(&ents) == sig_v(&have) {
        return false;
    }
    let block = cc_cad::doc::Block::new(SHEET_LIST_BLOCK, cc_geom::P2::new(0.0, 0.0), ents.iter().map(Entity::from_value).collect());
    let _ = doc.change(&l.s("sheetset.undo.list"), |d| d.define_block(&block).map(|_| ()));
    true
}
