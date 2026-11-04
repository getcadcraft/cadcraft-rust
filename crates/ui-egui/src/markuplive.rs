//! MarkupCraft in a Studio Live session (0.5 markupcraft/dev/live.js, LV.16): ops come from the Store's own records
//! (each undo step's `add` / `update` / `remove` / `cal` / `name` / `punch`):
//!
//! - `{o:'add', id, m, at?}` a markup created — key `e:<id>` (c); `{o:'del', id, ly, at?}` removed (d)
//! - `{o:'set', id, ly, f, v | x:1}` one field (style fields one by one: `style.color`) (p)
//! - `{o:'text', id, ly, v}` its text: structural without a version (the typist's hold protects it) (s)
//! - `{o:'reply', id, ly, r}` / `{o:'unreply', id, ly, rid}` one reply of a thread — `e:<id>` field `reply.<rid>` (p)
//! - `{o:'cal', sheet, v}` a sheet's scale — `meta:cal/<sheet>`; `{o:'name', sheet, v}` — `meta:name/<sheet>`
//! - `{o:'punch', v}` the next punch number: structural, so two new punch items never share one
//!
//! A markup's layer is a read (`layer:<name>`). Markups name sheets `<fileId>:<page>`: a joiner opens the host's PDFs
//! (they come in the snapshot) under the host's file ids. Personal and never sent: layers' shown / locked state, custom
//! columns, the view, the tool, the author name.

use cc_live::doc::{Adapter, Cond, TouchReq};
use cc_live::msgpack::Value as Mp;
use cc_markup::json::{markup_from_value, markup_value};
use cc_markup::model::{Op, SheetName, Store, geometry_ok};
use cc_markup::units::Cal;
use serde_json::{Map, Value as J, json};

use crate::livestate::{ops_json, same, to_json, to_mp};

pub const SCHEMA: u64 = 1;

fn cal_json(c: &Cal) -> J {
    let mut s = cc_m3d::json::Out::new(false);
    cc_markup::json::write_cal(&mut s, c);
    serde_json::from_str(&s.s).unwrap_or(J::Null)
}
fn name_json(n: &SheetName) -> J {
    json!({"number": n.number, "title": n.title})
}
fn name_of(v: &J) -> Option<SheetName> {
    let o = v.as_object()?;
    let s = |k: &str| o.get(k).and_then(J::as_str).unwrap_or("").to_string();
    Some(SheetName { number: s("number"), title: s("title") })
}
fn truthy(v: Option<&J>) -> bool {
    v.is_some_and(|v| !matches!(v, J::Null | J::Bool(false)) && v != &J::from(0) && v != &J::from(""))
}
fn op_s<'a>(op: &'a J, k: &str) -> &'a str {
    op.get(k).and_then(J::as_str).unwrap_or("")
}
fn layer_of(m: Option<&J>) -> String {
    m.and_then(|m| m.get("layer")).filter(|l| truthy(Some(l))).and_then(J::as_str).unwrap_or("").to_string()
}

/// live.js `diff(id, a, b)` on two markups' JSON.
fn diff_markup(id: &str, a: &J, b: &J, out: &mut Vec<J>) {
    let ly = {
        let l = layer_of(Some(b));
        if l.is_empty() { layer_of(Some(a)) } else { l }
    };
    let (Some(ao), Some(bo)) = (a.as_object(), b.as_object()) else { return };
    let mut fs: Vec<&String> = ao.keys().collect();
    fs.extend(bo.keys().filter(|k| !ao.contains_key(*k)));
    for f in fs {
        let (x, y) = (ao.get(f), bo.get(f));
        if f == "id" || same(x, y) {
            continue;
        }
        if f == "style"
            && let (Some(J::Object(sa)), Some(J::Object(sb))) = (x, y)
        {
            let mut ks: Vec<&String> = sa.keys().collect();
            ks.extend(sb.keys().filter(|k| !sa.contains_key(*k)));
            for k in ks {
                if !same(sa.get(k), sb.get(k)) {
                    out.push(match sb.get(k) {
                        None => json!({"o": "set", "id": id, "ly": ly, "f": format!("style.{k}"), "x": 1}),
                        Some(v) => json!({"o": "set", "id": id, "ly": ly, "f": format!("style.{k}"), "v": v}),
                    });
                }
            }
            continue;
        }
        if f == "replies" {
            let rid = |r: &J| r.get("id").cloned().unwrap_or(J::Null);
            let was: Vec<&J> = x.and_then(J::as_array).map(|a| a.iter().collect()).unwrap_or_default();
            let now: Vec<&J> = y.and_then(J::as_array).map(|a| a.iter().collect()).unwrap_or_default();
            // a Map keeps the last of equal ids
            fn last<'a>(l: &[&'a J], id: &J) -> Option<&'a J> {
                l.iter().rev().find(|r| same(r.get("id").or(Some(&J::Null)), Some(id))).copied()
            }
            let mut seen: Vec<J> = vec![];
            for r in &now {
                let k = rid(r);
                if seen.iter().any(|s| same(Some(s), Some(&k))) {
                    continue;
                }
                seen.push(k.clone());
                let r = last(&now, &k).unwrap_or(r);
                if last(&was, &k).is_none_or(|w| !same(Some(w), Some(r))) {
                    out.push(json!({"o": "reply", "id": id, "ly": ly, "r": r}));
                }
            }
            let mut seen: Vec<J> = vec![];
            for r in &was {
                let k = rid(r);
                if seen.iter().any(|s| same(Some(s), Some(&k))) {
                    continue;
                }
                seen.push(k.clone());
                if last(&now, &k).is_none() {
                    out.push(json!({"o": "unreply", "id": id, "ly": ly, "rid": k}));
                }
            }
            continue;
        }
        if f == "text" {
            let v = match y {
                None | Some(J::Null) => J::from(""),
                Some(v) => v.clone(),
            };
            out.push(json!({"o": "text", "id": id, "ly": ly, "v": v}));
            continue;
        }
        out.push(match y {
            None => json!({"o": "set", "id": id, "ly": ly, "f": f, "x": 1}),
            Some(v) => json!({"o": "set", "id": id, "ly": ly, "f": f, "v": v}),
        });
    }
}

/// live.js `opsOf(records, reverse)`: a Store step's records as wire ops (reverse = the inverse).
pub fn ops_of(records: &[Op], reverse: bool) -> Vec<J> {
    let mut out = vec![];
    let list: Vec<&Op> = if reverse { records.iter().rev().collect() } else { records.iter().collect() };
    for r in list {
        match r {
            Op::Add { id, after, .. } => {
                if reverse {
                    // the record's `before` is nothing: the layer the JS reads from it is ''
                    out.push(json!({"o": "del", "id": id, "ly": ""}));
                } else {
                    out.push(json!({"o": "add", "id": id, "m": markup_value(after)}));
                }
            }
            Op::Remove { id, before, order, .. } => {
                if reverse {
                    out.push(json!({"o": "add", "id": id, "m": markup_value(before), "at": order}));
                } else {
                    out.push(json!({"o": "del", "id": id, "ly": before.layer, "at": order}));
                }
            }
            Op::Update { id, before, after, .. } => {
                let (a, b) = if reverse { (after, before) } else { (before, after) };
                diff_markup(id, &markup_value(a), &markup_value(b), &mut out);
            }
            Op::Cal { sheet, before, after } => {
                let v = if reverse { before } else { after };
                out.push(json!({"o": "cal", "sheet": sheet, "v": v.as_ref().map_or(J::Null, cal_json)}));
            }
            Op::Name { sheet, before, after } => {
                let v = if reverse { before } else { after };
                out.push(json!({"o": "name", "sheet": sheet, "v": v.as_ref().map_or(J::Null, name_json)}));
            }
            Op::Punch { before, after } => {
                out.push(json!({"o": "punch", "v": cc_detail::js::n(if reverse { *before } else { *after })}));
            }
        }
    }
    out
}

fn reply_sort(rs: &mut [J]) {
    let k = |r: &J, f: &str| r.get(f).map(crate::livestate::id_text).unwrap_or_default();
    rs.sort_by(|x, y| {
        let (dx, dy) = (k(x, "date"), k(y, "date"));
        if dx < dy {
            std::cmp::Ordering::Less
        } else if dx > dy {
            std::cmp::Ordering::Greater
        } else if k(x, "id") < k(y, "id") {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }
    });
}

/// live.js `applyOp(op)` on the store.
pub fn apply_one(store: &mut Store, op: &J) {
    let id = op.get("id").map(crate::livestate::id_text);
    let cur = id.as_ref().and_then(|i| store.markups.get(i)).map(markup_value);
    match op_s(op, "o") {
        "add" => {
            let Some(m) = op.get("m").filter(|m| m.is_object()) else { return };
            let mk = markup_from_value(m);
            let key = id.unwrap_or_else(|| mk.id.clone());
            if let Some(s) = mk.sheet.clone() {
                store.dirty_sheets.insert(s);
            }
            match op.get("at").and_then(J::as_u64).and_then(|a| usize::try_from(a).ok()) {
                Some(at) if cur.is_none() => {
                    let at = at.min(store.markups.len());
                    store.markups.shift_insert(at, key, mk);
                }
                _ => {
                    store.markups.insert(key, mk);
                }
            }
        }
        "del" => {
            if let Some(i) = id
                && let Some(m) = store.markups.shift_remove(&i)
            {
                store.dirty_sheets.insert(m.sheet().to_string());
            }
        }
        "set" | "text" | "reply" | "unreply" => {
            let (Some(i), Some(J::Object(mut m))) = (id, cur) else { return };
            match op_s(op, "o") {
                "text" => {
                    let v = match op.get("v") {
                        None | Some(J::Null) => J::from(""),
                        Some(v) => v.clone(),
                    };
                    m.insert("text".into(), v);
                }
                "reply" => {
                    let Some(r) = op.get("r") else { return };
                    let rs = m.entry("replies").or_insert_with(|| J::Array(vec![]));
                    if !rs.is_array() {
                        *rs = J::Array(vec![]);
                    }
                    if let J::Array(l) = rs {
                        match l.iter().position(|x| same(x.get("id"), r.get("id"))) {
                            Some(k) => {
                                if let Some(s) = l.get_mut(k) {
                                    *s = r.clone();
                                }
                            }
                            None => l.push(r.clone()),
                        }
                        reply_sort(l);
                    }
                }
                "unreply" => {
                    if let Some(J::Array(l)) = m.get_mut("replies") {
                        l.retain(|x| !same(x.get("id"), op.get("rid")));
                    } else {
                        m.insert("replies".into(), J::Array(vec![]));
                    }
                }
                _ => {
                    let f = op_s(op, "f");
                    let x = truthy(op.get("x"));
                    if let Some(k) = f.strip_prefix("style.") {
                        let st = m.entry("style").or_insert_with(|| J::Object(Map::new()));
                        if !st.is_object() {
                            *st = J::Object(Map::new());
                        }
                        if let J::Object(st) = st {
                            if x {
                                st.shift_remove(k);
                            } else {
                                st.insert(k.to_string(), op.get("v").cloned().unwrap_or(J::Null));
                            }
                        }
                    } else if x {
                        m.shift_remove(f);
                    } else {
                        m.insert(f.to_string(), op.get("v").cloned().unwrap_or(J::Null));
                    }
                }
            }
            let mk = markup_from_value(&J::Object(m));
            store.dirty_sheets.insert(mk.sheet().to_string());
            store.markups.insert(i, mk);
        }
        "cal" => {
            let sheet = op_s(op, "sheet").to_string();
            match op.get("v").filter(|v| truthy(Some(v))).and_then(cc_markup::json::cal_of) {
                Some(c) => {
                    store.calibrations.insert(sheet.clone(), c);
                }
                None => {
                    store.calibrations.shift_remove(&sheet);
                }
            }
            store.dirty_sheets.insert(sheet);
        }
        "name" => {
            let sheet = op_s(op, "sheet").to_string();
            match op.get("v").filter(|v| truthy(Some(v))).and_then(name_of) {
                Some(n) => {
                    store.sheet_names.insert(sheet.clone(), n);
                }
                None => {
                    store.sheet_names.shift_remove(&sheet);
                }
            }
            store.dirty_sheets.insert(sheet);
        }
        "punch" => {
            store.punch_next = op.get("v").and_then(J::as_f64).unwrap_or(f64::NAN);
        }
        _ => {}
    }
}

/// live.js `invert(ops)`, op by op on the markups as they are before it.
pub fn invert(store: &Store, ops: &[J]) -> Vec<J> {
    let mut order: Vec<String> = store.markups.keys().cloned().collect();
    let mut inv: Vec<Option<J>> = vec![];
    for op in ops {
        let id = op.get("id").map(crate::livestate::id_text);
        let cur = id.as_ref().and_then(|i| store.markups.get(i)).map(markup_value);
        let ly = layer_of(cur.as_ref());
        let pos = id.as_ref().and_then(|i| order.iter().position(|x| x == i));
        let r = match op_s(op, "o") {
            "add" => Some(match (&cur, pos) {
                (Some(m), Some(i)) => json!({"o": "add", "id": op.get("id"), "m": m, "at": i}),
                _ => json!({"o": "del", "id": op.get("id"), "ly": layer_of(op.get("m"))}),
            }),
            "del" => match (&cur, pos) {
                (Some(m), Some(i)) => Some(json!({"o": "add", "id": op.get("id"), "m": m, "at": i})),
                _ => None,
            },
            "set" => cur.as_ref().map(|m| {
                let f = op_s(op, "f");
                let v = match f.strip_prefix("style.") {
                    Some(k) => m.get("style").and_then(|s| s.get(k)),
                    None => m.get(f),
                };
                match v {
                    None => json!({"o": "set", "id": op.get("id"), "ly": ly, "f": f, "x": 1}),
                    Some(v) => json!({"o": "set", "id": op.get("id"), "ly": ly, "f": f, "v": v}),
                }
            }),
            "text" => cur.as_ref().map(|m| {
                let v = match m.get("text") {
                    None | Some(J::Null) => J::from(""),
                    Some(v) => v.clone(),
                };
                json!({"o": "text", "id": op.get("id"), "ly": ly, "v": v})
            }),
            "reply" => cur.as_ref().map(|m| {
                let rid = op.get("r").and_then(|r| r.get("id"));
                let was = m.get("replies").and_then(J::as_array).and_then(|l| l.iter().find(|r| same(r.get("id"), rid)));
                match was {
                    Some(w) => json!({"o": "reply", "id": op.get("id"), "ly": ly, "r": w}),
                    None => json!({"o": "unreply", "id": op.get("id"), "ly": ly, "rid": rid}),
                }
            }),
            "unreply" => cur.as_ref().and_then(|m| {
                let was = m.get("replies").and_then(J::as_array).and_then(|l| l.iter().find(|r| same(r.get("id"), op.get("rid"))));
                was.map(|w| json!({"o": "reply", "id": op.get("id"), "ly": ly, "r": w}))
            }),
            "cal" => {
                let sheet = op_s(op, "sheet");
                Some(json!({"o": "cal", "sheet": sheet, "v": store.calibrations.get(sheet).map_or(J::Null, cal_json)}))
            }
            "name" => {
                let sheet = op_s(op, "sheet");
                Some(json!({"o": "name", "sheet": sheet, "v": store.sheet_names.get(sheet).map_or(J::Null, name_json)}))
            }
            "punch" => Some(json!({"o": "punch", "v": cc_detail::js::n(store.punch_next)})),
            "g" => Some(json!({"o": "g", "g": op.get("g")})),
            _ => None,
        };
        inv.push(r);
        if let Some(i) = id {
            match op_s(op, "o") {
                "add" if !order.contains(&i) => {
                    let at = op.get("at").and_then(J::as_u64).and_then(|a| usize::try_from(a).ok()).unwrap_or(order.len()).min(order.len());
                    order.insert(at, i);
                }
                "del" => order.retain(|x| *x != i),
                _ => {}
            }
        }
    }
    let mut out: Vec<J> = inv.into_iter().rev().flatten().collect();
    if let Some(gi) = out.iter().position(|o| op_s(o, "o") == "g")
        && gi > 0
    {
        let g = out.remove(gi);
        out.insert(0, g);
    }
    out
}

/// Adds one `touched` entry (key, mode, field, condition) unless it is there already.
type AddTouch<'a> = dyn FnMut(&mut Vec<TouchReq>, String, char, Option<String>, Cond) + 'a;

/// live.js `touched(ops)`.
pub fn touched(ops: &[J]) -> Vec<TouchReq> {
    let mut out: Vec<TouchReq> = vec![];
    let mut seen = std::collections::HashSet::new();
    let mut add = |out: &mut Vec<TouchReq>, k: String, m: char, f: Option<String>, r: Cond| {
        if seen.insert(format!("{k}\u{0}{m}\u{0}{}", f.clone().unwrap_or_default())) {
            out.push(TouchReq { k, m, f, r, g: false });
        }
    };
    let txt = crate::livestate::id_text;
    for op in ops {
        let id = txt(op.get("id").unwrap_or(&J::Null));
        let layer = |out: &mut Vec<TouchReq>, add: &mut AddTouch<'_>, ly: Option<&J>| {
            if truthy(ly) {
                add(out, format!("layer:{}", txt(ly.unwrap_or(&J::Null))), 'r', None, Cond::Auto);
            }
        };
        match op_s(op, "o") {
            "add" => {
                add(&mut out, format!("e:{id}"), 'c', None, Cond::Auto);
                layer(&mut out, &mut add, op.get("m").and_then(|m| m.get("layer")));
            }
            "del" => {
                add(&mut out, format!("e:{id}"), 'd', None, Cond::Auto);
                layer(&mut out, &mut add, op.get("ly"));
            }
            "set" => {
                add(&mut out, format!("e:{id}"), 'p', Some(op_s(op, "f").to_string()), Cond::Auto);
                layer(&mut out, &mut add, op.get("ly"));
                if op_s(op, "f") == "layer" && !truthy(op.get("x")) {
                    layer(&mut out, &mut add, op.get("v"));
                }
            }
            "text" => {
                add(&mut out, format!("e:{id}"), 's', None, Cond::None);
                layer(&mut out, &mut add, op.get("ly"));
            }
            "reply" => {
                let rid = txt(op.get("r").and_then(|r| r.get("id")).unwrap_or(&J::Null));
                add(&mut out, format!("e:{id}"), 'p', Some(format!("reply.{rid}")), Cond::Auto);
            }
            "unreply" => {
                add(&mut out, format!("e:{id}"), 'p', Some(format!("reply.{}", txt(op.get("rid").unwrap_or(&J::Null)))), Cond::Auto)
            }
            "cal" => add(&mut out, format!("meta:cal/{}", op_s(op, "sheet")), 'p', Some("v".into()), Cond::Auto),
            "name" => add(&mut out, format!("meta:name/{}", op_s(op, "sheet")), 'p', Some("v".into()), Cond::Auto),
            "punch" => add(&mut out, "meta:punch".into(), 's', None, Cond::Auto),
            _ => {}
        }
    }
    out
}

/// live.js `validate(ops)`: geometry that makes sense, read-only annotations stay read-only.
pub fn validate(store: &Store, ops: &[J]) -> Option<String> {
    let mut after: Vec<(String, J)> = vec![];
    for op in ops {
        let o = op_s(op, "o");
        let id = op.get("id").map(crate::livestate::id_text).unwrap_or_default();
        if o == "add" {
            let Some(m) = op.get("m").filter(|m| m.is_object()) else { return Some("invalid".into()) };
            if !truthy(m.get("type")) || !m.get("sheet").is_some_and(J::is_string) || !geometry_ok(&markup_from_value(m)) {
                return Some("invalid".into());
            }
            after.push((id, m.clone()));
        } else if o == "set" && matches!(op_s(op, "f"), "pts" | "rect" | "holes") {
            let base =
                after.iter().rev().find(|(k, _)| *k == id).map(|(_, m)| m.clone()).or_else(|| store.markups.get(&id).map(markup_value));
            if let Some(J::Object(mut b)) = base {
                let f = op_s(op, "f");
                if truthy(op.get("x")) {
                    b.shift_remove(f);
                } else {
                    b.insert(f.to_string(), op.get("v").cloned().unwrap_or(J::Null));
                }
                if !geometry_ok(&markup_from_value(&J::Object(b))) {
                    return Some("invalid".into());
                }
            }
        } else if o == "set" && op_s(op, "f") == "locked" {
            return Some("invalid".into());
        }
    }
    for op in ops {
        let o = op_s(op, "o");
        if o != "add"
            && o != "g"
            && o != "reply"
            && let Some(id) = op.get("id").map(crate::livestate::id_text)
            && store.markups.get(&id).is_some_and(|m| m.locked)
        {
            return Some("invalid".into());
        }
    }
    None
}

/// The shared state (live.js `state()` without the files): what the convergence checks compare.
pub fn state_json(store: &Store) -> J {
    json!({
        "v": SCHEMA,
        "markups": store.markups.values().map(markup_value).collect::<Vec<_>>(),
        "calibrations": store.calibrations.iter().map(|(k, c)| json!([k, cal_json(c)])).collect::<Vec<_>>(),
        "sheetNames": store.sheet_names.iter().map(|(k, n)| json!([k, name_json(n)])).collect::<Vec<_>>(),
        "punchNext": cc_detail::js::n(store.punch_next),
    })
}

/// The adapter over a plan set's store. `files` = the open PDFs (id, name, bytes) for the snapshot; `opened` gets the
/// PDFs a snapshot brings (under the host's ids).
pub struct MkAdapter<'a> {
    pub store: &'a mut Store,
    pub files: Vec<(String, String, std::sync::Arc<Vec<u8>>)>,
    pub opened: &'a mut Vec<(String, String, Vec<u8>)>,
}

impl Adapter for MkAdapter<'_> {
    fn apply(&mut self, ops: &Mp, _origin: &str) -> Result<(), String> {
        for op in ops_json(ops) {
            apply_one(self.store, &op);
        }
        self.store.version = self.store.version.saturating_add(1);
        Ok(())
    }
    fn invert(&mut self, ops: &Mp) -> Result<Mp, String> {
        Ok(Mp::Arr(invert(self.store, &ops_json(ops)).iter().map(to_mp).collect()))
    }
    fn touched(&self, ops: &Mp) -> Vec<TouchReq> {
        touched(&ops_json(ops))
    }
    fn snapshot(&mut self) -> Vec<u8> {
        let files: Vec<Mp> = self
            .files
            .iter()
            .map(|(id, n, b)| Mp::map(vec![("id", Mp::Str(id.clone())), ("name", Mp::Str(n.clone())), ("bytes", Mp::Bin(b.to_vec()))]))
            .collect();
        let st = state_json(self.store);
        let mut m = vec![("v".to_string(), Mp::from(SCHEMA)), ("files".to_string(), Mp::Arr(files))];
        if let Some(o) = st.as_object() {
            for (k, v) in o {
                if k != "v" {
                    m.push((k.clone(), to_mp(v)));
                }
            }
        }
        cc_live::msgpack::pack(&Mp::Map(m)).unwrap_or_default()
    }
    fn load(&mut self, bytes: &[u8], _q: u64) -> Result<(), String> {
        let x = cc_live::msgpack::unpack(bytes).map_err(|e| e.0.to_string())?;
        if x.get("v").and_then(Mp::as_u64) != Some(SCHEMA) {
            return Err("live snapshot: unknown format".into());
        }
        let s = &mut *self.store;
        s.markups.clear();
        for m in x.get("markups").map(Mp::as_arr).unwrap_or(&[]) {
            let mk = markup_from_value(&to_json(m));
            s.markups.insert(mk.id.clone(), mk);
        }
        s.calibrations.clear();
        for p in x.get("calibrations").map(Mp::as_arr).unwrap_or(&[]) {
            let p = to_json(p);
            if let (Some(k), Some(c)) = (p.get(0).map(crate::livestate::id_text), p.get(1).and_then(cc_markup::json::cal_of)) {
                s.calibrations.insert(k, c);
            }
        }
        s.sheet_names.clear();
        for p in x.get("sheetNames").map(Mp::as_arr).unwrap_or(&[]) {
            let p = to_json(p);
            if let (Some(k), Some(n)) = (p.get(0).map(crate::livestate::id_text), p.get(1).and_then(name_of)) {
                s.sheet_names.insert(k, n);
            }
        }
        let pn = x.get("punchNext").map(to_json).and_then(|v| v.as_f64()).filter(|v| *v != 0.0 && !v.is_nan()).unwrap_or(1.0);
        s.punch_next = pn;
        s.undo_stack.clear();
        s.redo_stack.clear();
        s.version = s.version.saturating_add(1);
        for f in x.get("files").map(Mp::as_arr).unwrap_or(&[]) {
            if let (Some(id), Some(n), Some(Mp::Bin(b))) =
                (f.get("id").and_then(Mp::as_str), f.get("name").and_then(Mp::as_str), f.get("bytes"))
            {
                self.opened.push((id.to_string(), n.to_string(), b.clone()));
            }
        }
        Ok(())
    }
    fn validate(&mut self, ops: &Mp) -> Option<String> {
        validate(self.store, &ops_json(ops))
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The window's side (liveapps.rs)

/// MarkupCraft's [`crate::live::LiveView`] over the window: the open plan set's store, or (a joiner with nothing open)
/// one held until the snapshot's PDFs open.
pub struct MkLive<'a>(pub &'a mut crate::UiApp);

impl MkLive<'_> {
    fn store(&mut self) -> &mut Store {
        match self.0.pdf.as_mut() {
            Some(p) => &mut p.store,
            None => self.0.live_hold_mk.get_or_insert_with(Store::new),
        }
    }
    fn open_pdfs(&mut self, list: Vec<(String, String, Vec<u8>)>) {
        if self.0.pdf.is_none() {
            match crate::pdfview::PdfView::open_shared(&list) {
                Ok(mut p) => {
                    p.mode = "markup";
                    p.zone = self.0.zone;
                    if let Some(h) = self.0.live_hold_mk.take() {
                        p.store.markups = h.markups;
                        p.store.calibrations = h.calibrations;
                        p.store.sheet_names = h.sheet_names;
                        p.store.punch_next = h.punch_next;
                        p.store.undo_stack.clear();
                        p.store.redo_stack.clear();
                        p.store.version = p.store.version.saturating_add(1);
                    }
                    p.live_on = true;
                    self.0.pdf = Some(p);
                }
                Err(e) => self.0.message = e,
            }
            return;
        }
        let Some(p) = self.0.pdf.as_mut() else { return };
        for (id, name, bytes) in &list {
            if let Err(e) = p.add_shared(id, name, bytes) {
                self.0.message = format!("{name}: {e}");
            }
        }
    }
    /// Undo / Redo pressed while live: the person's own steps.
    pub fn reverse_asked(&mut self) {
        let Some(redo) = self.0.pdf.as_mut().and_then(|p| p.live_req.take()) else { return };
        #[cfg(target_arch = "wasm32")]
        let _ = redo;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(l) = crate::live::reverse(self, !redo) {
            let which = if redo { "redo" } else { "undo" };
            let m = match l {
                Some(l) => {
                    let mut c = l.chars();
                    let low = c.next().map(|f| f.to_lowercase().collect::<String>() + c.as_str()).unwrap_or_default();
                    cc_markup::i18n::t(&format!("history.{which}"), &[("what", cc_markup::i18n::Fill::S(low))])
                }
                None => cc_markup::i18n::t0(&format!("history.{which}.none")),
            };
            if let Some(p) = self.0.pdf.as_mut() {
                let store = &p.store;
                p.tools.sel.retain(|id| store.get(id).is_some());
                p.tools.message = m.clone();
            }
            self.0.message = m;
        }
    }
}

impl crate::live::LiveView for MkLive<'_> {
    const APP: &'static str = "markupcraft";
    const PREFIX: &'static str = "markup";
    fn live_slot(&mut self) -> &mut Option<crate::live::LiveSession> {
        &mut self.0.live
    }
    fn live_window_open(&mut self) -> &mut bool {
        &mut self.0.live_window
    }
    fn with_adapter(&mut self, _c: u64, f: &mut dyn FnMut(&mut dyn Adapter)) {
        let files: Vec<(String, String, std::sync::Arc<Vec<u8>>)> = self
            .0
            .pdf
            .as_ref()
            .map(|p| p.files().iter().map(|f| (f.id.clone(), f.name.clone(), f.bytes.clone())).collect())
            .unwrap_or_default();
        let mut opened = vec![];
        {
            let store = self.store();
            f(&mut MkAdapter { store, files, opened: &mut opened });
        }
        if !opened.is_empty() {
            self.open_pdfs(opened);
        }
    }
    fn claim_ids(&mut self, _c: u64) {}
    fn release_ids(&mut self) {
        let s = self.store();
        s.undo_stack.clear();
        s.redo_stack.clear();
        if let Some(p) = self.0.pdf.as_mut() {
            p.live_on = false;
        }
    }
    fn live_bind(&mut self) {
        let s = self.store();
        s.undo_stack.clear();
        s.redo_stack.clear();
        if let Some(p) = self.0.pdf.as_mut() {
            p.live_on = true;
            p.mode = "markup";
        }
    }
    fn model_version(&self) -> u64 {
        self.0.pdf.as_ref().map_or_else(|| self.0.live_hold_mk.as_ref().map_or(0, |s| s.version), |p| p.store.version)
    }
    fn after_remote(&mut self) {
        if let Some(p) = self.0.pdf.as_mut() {
            p.live_on = true;
            let store = &p.store;
            p.tools.sel.retain(|id| store.get(id).is_some());
            p.tex = None;
        }
    }
    fn tr(&self, key: &str, vars: &[(&str, String)]) -> String {
        let v: Vec<(&str, cc_markup::i18n::Fill)> = vars.iter().map(|(k, s)| (*k, cc_markup::i18n::Fill::S(s.clone()))).collect();
        cc_markup::i18n::t(key, &v)
    }
    fn say(&mut self, msg: String) {
        if let Some(p) = self.0.pdf.as_mut() {
            p.tools.message = msg.clone();
        }
        self.0.message = msg;
    }
    fn live_state(&mut self) -> Option<J> {
        Some(state_json(self.store()))
    }
    fn local_steps(&mut self, g: &mut dyn FnMut() -> String) -> Vec<(String, Mp, Mp)> {
        let s = self.store();
        if s.undo_stack.is_empty() {
            return vec![];
        }
        let steps = std::mem::take(&mut s.undo_stack);
        let mut out = vec![];
        for st in &steps {
            let ops = ops_of(&st.ops, false);
            if ops.is_empty() {
                continue;
            }
            let inverse = ops_of(&st.ops, true);
            let mark = g();
            out.push((st.label.clone(), crate::livestate::with_mark(&mark, &ops), crate::livestate::with_mark(&mark, &inverse)));
        }
        out
    }
}
