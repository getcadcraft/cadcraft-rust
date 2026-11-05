//! TakeoffCraft in a Studio Live session (0.5 takeoffcraft/dev/live.js, LV.16): the model keeps a whole copy of the
//! takeoff before each step, so a step's ops are the difference between that copy and the takeoff after it, per object:
//!
//! - `{o:'add', c, id, v, at}` an item / mark / bid area / typical group / note created — key `e:<c>/<id>` (c)
//! - `{o:'del', c, id}` …removed (d); `{o:'set', c, id, f, v | x:1}` one of its fields (p)
//! - `{o:'key', c, k, v | x:1}` one entry of a keyed table (a sheet's scale and settings) — `meta:<c>/<k>` (p)
//! - `{o:'top', f, v | x:1}` the takeoff's name, the bid … — `meta:<f>` (p); `{o:'ord', c, v}` a list's order (p)
//! - `{o:'file', v}` a PDF listed in the takeoff (never removed while live) — `meta:file/<hash>` (p)
//!
//! A mark reads its item, bid area and group, so a mark made on an item someone deleted is refused, not orphaned.
//! Totals, quantities and the estimate are never sent. Ids: each person's new objects number from their own range
//! (client c: c × 1e6 + 1 …); `seq` is personal. The snapshot carries the takeoff and the open PDFs; a joiner opens them.

use cc_live::doc::{Adapter, Cond, TouchReq};
use cc_live::msgpack::Value as Mp;
use serde_json::{Map, Value as J, json};

use crate::livestate::{ops_json, same, to_json, to_mp};

pub const SCHEMA: u64 = 1;
const ID_BLOCK: f64 = 1e6;
const LISTS: [&str; 5] = ["items", "marks", "areas", "groups", "notes"];
const TABLES: [&str; 1] = ["sheets"];

fn arr<'a>(s: &'a J, c: &str) -> &'a [J] {
    s.get(c).and_then(J::as_array).map_or(&[], Vec::as_slice)
}
fn id_of(x: &J) -> J {
    x.get("id").cloned().unwrap_or(J::Null)
}
fn key(x: &J) -> String {
    cc_detail::js::stringify(&id_of(x))
}
/// What a mark depends on (`dep(c, x)`): its item, bid area and group (absent ones left out, as JS packs undefined).
fn dep(c: &str, x: &J, o: &mut Map<String, J>) {
    if c != "marks" {
        return;
    }
    for (k, f) in [("it", "item"), ("ar", "area"), ("gr", "group")] {
        if let Some(v) = x.get(f) {
            o.insert(k.into(), v.clone());
        }
    }
}
fn obj(v: J) -> Map<String, J> {
    match v {
        J::Object(o) => o,
        _ => Map::new(),
    }
}

/// live.js `diff(a, b)`.
pub fn diff(a: &J, b: &J) -> Vec<J> {
    let (mut head, mut out, mut tail) = (vec![], vec![], vec![]);
    let had: Vec<String> = arr(a, "files").iter().map(|f| cc_detail::js::stringify(f.get("hash").unwrap_or(&J::Null))).collect();
    for f in arr(b, "files") {
        if !had.contains(&cc_detail::js::stringify(f.get("hash").unwrap_or(&J::Null))) {
            head.push(json!({"o": "file", "v": f}));
        }
    }
    for c in LISTS {
        let (la, lb) = (arr(a, c), arr(b, c));
        let ka: Vec<String> = la.iter().map(key).collect();
        let kb: Vec<String> = lb.iter().map(key).collect();
        let ia: std::collections::HashMap<&String, usize> = ka.iter().enumerate().map(|(i, k)| (k, i)).collect();
        let ib: std::collections::HashSet<&String> = kb.iter().collect();
        for (x, k) in la.iter().zip(&ka) {
            if !ib.contains(k) {
                let mut o = obj(json!({"o": "del", "c": c, "id": id_of(x)}));
                dep(c, x, &mut o);
                out.push(J::Object(o));
            }
        }
        for (at, (x, k)) in lb.iter().zip(&kb).enumerate() {
            if !ia.contains_key(k) {
                out.push(json!({"o": "add", "c": c, "id": id_of(x), "v": x, "at": at}));
            }
        }
        for (x, k) in lb.iter().zip(&kb) {
            let Some(was) = ia.get(k).and_then(|i| la.get(*i)) else { continue };
            let (Some(wo), Some(xo)) = (was.as_object(), x.as_object()) else { continue };
            let mut fields: Vec<&String> = wo.keys().collect();
            fields.extend(xo.keys().filter(|f| !wo.contains_key(*f)));
            for f in fields {
                if f == "id" || same(wo.get(f), xo.get(f)) {
                    continue;
                }
                let mut o = match xo.get(f) {
                    None => obj(json!({"o": "set", "c": c, "id": id_of(x), "f": f, "x": 1})),
                    Some(v) => obj(json!({"o": "set", "c": c, "id": id_of(x), "f": f, "v": v})),
                };
                dep(c, x, &mut o);
                out.push(J::Object(o));
            }
        }
        let before: Vec<&String> = ka.iter().filter(|k| ib.contains(k)).collect();
        let after: Vec<&String> = kb.iter().filter(|k| ia.contains_key(k)).collect();
        if before != after {
            tail.push(json!({"o": "ord", "c": c, "v": lb.iter().map(id_of).collect::<Vec<_>>()}));
        }
    }
    let empty = J::Object(Map::new());
    for c in TABLES {
        let (ta, tb) = (a.get(c).filter(|v| v.is_object()).unwrap_or(&empty), b.get(c).filter(|v| v.is_object()).unwrap_or(&empty));
        let (Some(ta), Some(tb)) = (ta.as_object(), tb.as_object()) else { continue };
        let mut ks: Vec<&String> = ta.keys().collect();
        ks.extend(tb.keys().filter(|k| !ta.contains_key(*k)));
        for k in ks {
            if !same(ta.get(k), tb.get(k)) {
                out.push(match tb.get(k) {
                    None => json!({"o": "key", "c": c, "k": k, "x": 1}),
                    Some(v) => json!({"o": "key", "c": c, "k": k, "v": v}),
                });
            }
        }
    }
    if let (Some(ao), Some(bo)) = (a.as_object(), b.as_object()) {
        let mut fs: Vec<&String> = ao.keys().collect();
        fs.extend(bo.keys().filter(|k| !ao.contains_key(*k)));
        for f in fs {
            if LISTS.contains(&f.as_str()) || TABLES.contains(&f.as_str()) || f == "files" || f == "seq" || same(ao.get(f), bo.get(f)) {
                continue;
            }
            out.push(match bo.get(f) {
                None => json!({"o": "top", "f": f, "x": 1}),
                Some(v) => json!({"o": "top", "f": f, "v": v}),
            });
        }
    }
    head.extend(out);
    head.extend(tail);
    head
}

fn op_s<'a>(op: &'a J, k: &str) -> &'a str {
    op.get(k).and_then(J::as_str).unwrap_or("")
}
fn is_list(c: &str) -> bool {
    LISTS.contains(&c)
}
fn x_set(op: &J) -> bool {
    op.get("x").is_some_and(|x| !x.is_null() && x != &J::from(0) && x != &J::Bool(false))
}
fn at_of(op: &J) -> Option<usize> {
    op.get("at").and_then(J::as_u64).and_then(|a| usize::try_from(a).ok())
}

/// live.js `applyOp(op)` on the takeoff's state.
pub fn apply_one(s: &mut J, op: &J) {
    let Some(so) = s.as_object_mut() else { return };
    let c = op_s(op, "c");
    match op_s(op, "o") {
        "file" => {
            let Some(v) = op.get("v") else { return };
            let files = so.entry("files").or_insert_with(|| J::Array(vec![]));
            if let J::Array(l) = files
                && !l.iter().any(|f| same(f.get("hash"), v.get("hash")))
            {
                l.push(v.clone());
            }
        }
        "add" if is_list(c) => {
            let Some(id) = op.get("id") else { return };
            let l = so.entry(c).or_insert_with(|| J::Array(vec![]));
            if !l.is_array() {
                *l = J::Array(vec![]);
            }
            let Some(l) = l.as_array_mut() else { return };
            let v = op.get("v").cloned().unwrap_or(J::Null);
            match l.iter().position(|x| same(x.get("id"), Some(id))) {
                Some(i) => {
                    if let Some(slot) = l.get_mut(i) {
                        *slot = v;
                    }
                }
                None => {
                    let at = at_of(op).unwrap_or(l.len()).min(l.len());
                    l.insert(at, v);
                }
            }
        }
        "del" if is_list(c) => {
            if let Some(J::Array(l)) = so.get_mut(c)
                && let Some(i) = l.iter().position(|x| same(x.get("id"), op.get("id")))
            {
                l.remove(i);
            }
        }
        "set" if is_list(c) => {
            let f = op_s(op, "f");
            if let Some(J::Array(l)) = so.get_mut(c)
                && let Some(J::Object(x)) = l.iter_mut().find(|y| same(y.get("id"), op.get("id")))
            {
                if x_set(op) {
                    x.shift_remove(f);
                } else {
                    x.insert(f.to_string(), op.get("v").cloned().unwrap_or(J::Null));
                }
            }
        }
        "key" => {
            let k = op_s(op, "k");
            let tb = so.entry(c).or_insert_with(|| J::Object(Map::new()));
            if let J::Object(tb) = tb {
                if x_set(op) {
                    tb.shift_remove(k);
                } else {
                    tb.insert(k.to_string(), op.get("v").cloned().unwrap_or(J::Null));
                }
            }
        }
        "top" => {
            let f = op_s(op, "f");
            if f.is_empty() {
                return;
            }
            if x_set(op) {
                so.shift_remove(f);
            } else {
                so.insert(f.to_string(), op.get("v").cloned().unwrap_or(J::Null));
            }
        }
        "ord" if is_list(c) => {
            let want: Vec<String> =
                op.get("v").and_then(J::as_array).map(|a| a.iter().map(cc_detail::js::stringify).collect()).unwrap_or_default();
            if let Some(J::Array(l)) = so.get_mut(c) {
                let mut rest: Vec<Option<J>> = std::mem::take(l).into_iter().map(Some).collect();
                let ks: Vec<String> = rest.iter().map(|x| x.as_ref().map(key).unwrap_or_default()).collect();
                let mut next = vec![];
                for w in &want {
                    if let Some(i) = ks.iter().position(|k| k == w)
                        && let Some(x) = rest.get_mut(i).and_then(Option::take)
                    {
                        next.push(x);
                    }
                }
                next.extend(rest.into_iter().flatten());
                *l = next;
            }
        }
        _ => {}
    }
}

fn walk(op: &J, list: &mut Vec<J>) {
    let id = op.get("id").cloned().unwrap_or(J::Null);
    match op_s(op, "o") {
        "add" => {
            if !list.iter().any(|x| same(Some(x), Some(&id))) {
                let at = at_of(op).unwrap_or(list.len()).min(list.len());
                list.insert(at, id);
            }
        }
        "del" => {
            if let Some(i) = list.iter().position(|x| same(Some(x), Some(&id))) {
                list.remove(i);
            }
        }
        "ord" => {
            let want = op.get("v").and_then(J::as_array).cloned().unwrap_or_default();
            let mut next: Vec<J> = want.into_iter().filter(|w| list.iter().any(|x| same(Some(x), Some(w)))).collect();
            for x in list.iter() {
                if !next.iter().any(|y| same(Some(y), Some(x))) {
                    next.push(x.clone());
                }
            }
            *list = next;
        }
        _ => {}
    }
}

/// live.js `invert(ops)` on the state as it is now.
pub fn invert(s: &J, ops: &[J]) -> Vec<J> {
    let mut lists: Vec<(String, Vec<J>)> = vec![];
    let mut inv: Vec<Option<J>> = vec![];
    for op in ops {
        let c = op_s(op, "c").to_string();
        if is_list(&c) && !lists.iter().any(|(k, _)| *k == c) {
            lists.push((c.clone(), arr(s, &c).iter().map(id_of).collect()));
        }
        let find = |id: Option<&J>| arr(s, &c).iter().find(|x| same(x.get("id"), id));
        let r = match op_s(op, "o") {
            "add" | "del" if is_list(&c) => {
                let ids = lists.iter().find(|(k, _)| *k == c).map(|(_, l)| l.as_slice()).unwrap_or(&[]);
                let i = ids.iter().position(|x| same(Some(x), op.get("id")));
                match i.and_then(|i| find(op.get("id")).map(|x| (i, x))) {
                    Some((i, x)) => Some(json!({"o": "add", "c": c, "id": op.get("id"), "v": x, "at": i})),
                    None => {
                        let mut o = obj(json!({"o": "del", "c": c, "id": op.get("id")}));
                        if let Some(v) = op.get("v") {
                            dep(&c, v, &mut o);
                        }
                        Some(J::Object(o))
                    }
                }
            }
            "set" if is_list(&c) => find(op.get("id")).map(|x| {
                let f = op_s(op, "f");
                let mut o = match x.get(f) {
                    None => obj(json!({"o": "set", "c": c, "id": op.get("id"), "f": f, "x": 1})),
                    Some(v) => obj(json!({"o": "set", "c": c, "id": op.get("id"), "f": f, "v": v})),
                };
                dep(&c, x, &mut o);
                J::Object(o)
            }),
            "key" => {
                let k = op_s(op, "k");
                Some(match s.get(&c).and_then(|t| t.get(k)) {
                    None => json!({"o": "key", "c": c, "k": k, "x": 1}),
                    Some(v) => json!({"o": "key", "c": c, "k": k, "v": v}),
                })
            }
            "top" => {
                let f = op_s(op, "f");
                Some(match s.get(f) {
                    None => json!({"o": "top", "f": f, "x": 1}),
                    Some(v) => json!({"o": "top", "f": f, "v": v}),
                })
            }
            "ord" if is_list(&c) => {
                let ids = lists.iter().find(|(k, _)| *k == c).map(|(_, l)| l.clone()).unwrap_or_default();
                Some(json!({"o": "ord", "c": c, "v": ids}))
            }
            "g" => Some(json!({"o": "g", "g": op.get("g")})),
            _ => None,
        };
        inv.push(r);
        if is_list(&c)
            && let Some((_, l)) = lists.iter_mut().find(|(k, _)| *k == c)
        {
            walk(op, l);
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

/// Adds one `touched` entry (key, mode, field) unless it is there already.
type AddTouch<'a> = dyn FnMut(&mut Vec<TouchReq>, String, char, Option<&str>) + 'a;

/// live.js `touched(ops)`.
pub fn touched(ops: &[J]) -> Vec<TouchReq> {
    let mut out: Vec<TouchReq> = vec![];
    let mut seen = std::collections::HashSet::new();
    let mut add = |out: &mut Vec<TouchReq>, k: String, m: char, f: Option<&str>| {
        if seen.insert(format!("{k}\u{0}{m}\u{0}{}", f.unwrap_or(""))) {
            out.push(TouchReq { k, m, f: f.map(str::to_string), r: Cond::Auto, g: false });
        }
    };
    let txt = |v: &J| crate::livestate::id_text(v);
    let truthy = |v: Option<&J>| v.is_some_and(|v| !matches!(v, J::Null | J::Bool(false)) && v != &J::from(0) && v != &J::from(""));
    for op in ops {
        let c = op_s(op, "c");
        let reads = |out: &mut Vec<TouchReq>, add: &mut AddTouch<'_>| {
            let mv = |f: &str| if c == "marks" { op.get("v").and_then(|v| v.get(f)) } else { None };
            let it = op.get("it").or_else(|| mv("item"));
            let ar = op.get("ar").or_else(|| mv("area"));
            let gr = op.get("gr").or_else(|| mv("group"));
            if truthy(it) {
                add(out, format!("e:items/{}", txt(it.unwrap_or(&J::Null))), 'r', None);
            }
            if truthy(ar) {
                add(out, format!("e:areas/{}", txt(ar.unwrap_or(&J::Null))), 'r', None);
            }
            if truthy(gr) {
                add(out, format!("e:groups/{}", txt(gr.unwrap_or(&J::Null))), 'r', None);
            }
        };
        let id = txt(op.get("id").unwrap_or(&J::Null));
        match op_s(op, "o") {
            "file" => {
                add(&mut out, format!("meta:file/{}", txt(op.get("v").and_then(|v| v.get("hash")).unwrap_or(&J::Null))), 'p', Some("v"))
            }
            "add" => {
                add(&mut out, format!("e:{c}/{id}"), 'c', None);
                reads(&mut out, &mut add);
            }
            "del" => {
                add(&mut out, format!("e:{c}/{id}"), 'd', None);
                reads(&mut out, &mut add);
            }
            "set" => {
                add(&mut out, format!("e:{c}/{id}"), 'p', Some(op_s(op, "f")));
                reads(&mut out, &mut add);
                if c == "marks" && op_s(op, "f") == "item" && truthy(op.get("v")) {
                    add(&mut out, format!("e:items/{}", txt(op.get("v").unwrap_or(&J::Null))), 'r', None);
                }
            }
            "key" => add(&mut out, format!("meta:{c}/{}", op_s(op, "k")), 'p', Some("v")),
            "top" => add(&mut out, format!("meta:{}", op_s(op, "f")), 'p', Some("v")),
            "ord" => add(&mut out, format!("meta:{c}.order"), 'p', Some("v")),
            _ => {}
        }
    }
    out
}

/// live.js `validate(ops)`: a mark needs its item, and its kind must be one the item takes.
pub fn validate(s: &J, ops: &[J]) -> Option<String> {
    let mut items: Vec<(String, J)> = arr(s, "items").iter().map(|x| (key(x), x.clone())).collect();
    for op in ops {
        if op_s(op, "c") != "items" {
            continue;
        }
        let k = cc_detail::js::stringify(op.get("id").unwrap_or(&J::Null));
        match op_s(op, "o") {
            "add" => {
                items.retain(|(i, _)| *i != k);
                items.push((k, op.get("v").cloned().unwrap_or(J::Null)));
            }
            "del" => items.retain(|(i, _)| *i != k),
            "set" => {
                if let Some((_, x)) = items.iter_mut().find(|(i, _)| *i == k)
                    && let J::Object(o) = x
                {
                    o.insert(op_s(op, "f").to_string(), op.get("v").cloned().unwrap_or(J::Null));
                }
            }
            _ => {}
        }
    }
    let item = |id: Option<&J>| items.iter().find(|(k, _)| Some(k.as_str()) == id.map(cc_detail::js::stringify).as_deref()).map(|(_, x)| x);
    for op in ops {
        let (o, c) = (op_s(op, "o"), op_s(op, "c"));
        if o == "add" && c == "marks" {
            let m = op.get("v");
            let it = m.and_then(|m| item(m.get("item")));
            let pts_ok = m.and_then(|m| m.get("pts")).and_then(J::as_array).is_some_and(|p| !p.is_empty());
            let (Some(m), Some(it)) = (m, it) else { return Some("invalid".into()) };
            if !pts_ok || !m.is_object() {
                return Some("invalid".into());
            }
            let kind = cc_takeoff::jv::to_string(m.get("kind"));
            if !cc_takeoff::model::kinds_of(Some(it)).contains(&kind) {
                return Some("invalid".into());
            }
        }
        if o == "set" && c == "marks" && op_s(op, "f") == "item" && item(op.get("v")).is_none() {
            return Some("invalid".into());
        }
        if o == "del" && c == "items" {
            let gone = op.get("id");
            let in_use = arr(s, "marks").iter().any(|m| {
                same(m.get("item"), gone)
                    && !ops.iter().any(|x| op_s(x, "o") == "del" && op_s(x, "c") == "marks" && same(x.get("id"), m.get("id")))
            });
            if in_use {
                return Some("invalid".into());
            }
        }
    }
    None
}

fn num_id(v: &J) -> f64 {
    let s = crate::livestate::id_text(v);
    // +String(id).replace(/^\D+/, ''): leading non-digits dropped, the rest as a JS number ('' is 0)
    let d: String = s.chars().skip_while(|c| !c.is_ascii_digit()).collect();
    if d.trim().is_empty() {
        return 0.0;
    }
    d.parse::<f64>().unwrap_or(f64::NAN)
}

/// live.js `claimIds()`.
pub fn claim_ids(s: &mut J, c: u64) {
    #[allow(clippy::cast_precision_loss)]
    let lo = c as f64 * ID_BLOCK;
    let hi = lo + ID_BLOCK;
    let mut next = lo;
    for l in LISTS {
        for x in arr(s, l) {
            let n = num_id(&id_of(x));
            if n > next && n < hi {
                next = n;
            }
            for k in x.get("components").and_then(J::as_array).map_or(&[][..], Vec::as_slice) {
                let m = num_id(&id_of(k));
                if m > next && m < hi {
                    next = m;
                }
            }
        }
    }
    let seq = s.get("seq").and_then(J::as_f64);
    let v = match seq {
        Some(q) if q >= lo && q < hi => q.max(next),
        _ => next,
    };
    if let (Some(o), Some(n)) = (s.as_object_mut(), serde_json::Number::from_f64(v)) {
        o.insert("seq".into(), J::Number(n));
    }
}

/// live.js `releaseIds()`.
pub fn release_ids(s: &mut J) {
    let mut max: f64 = 0.0;
    for l in LISTS {
        for x in arr(s, l) {
            let n = num_id(&id_of(x));
            if n.is_finite() && n > max {
                max = n;
            }
        }
    }
    let cur = s.get("seq").and_then(J::as_f64).filter(|x| *x != 0.0 && x.is_finite()).unwrap_or(0.0);
    if let (Some(o), Some(n)) = (s.as_object_mut(), serde_json::Number::from_f64(cur.max(max))) {
        o.insert("seq".into(), J::Number(n));
    }
}

/// The takeoff without its personal fields (live.js `shared()`), and its canonical text (`hash()`).
pub fn shared(s: &J) -> J {
    let mut s = s.clone();
    if let Some(o) = s.as_object_mut() {
        o.shift_remove("seq");
    }
    s
}

/// The adapter over a takeoff (live.js `createLive`). `files` = the open PDFs (name, bytes) for the snapshot;
/// `opened` gets the PDFs a snapshot brings, for the window to open.
pub struct TkAdapter<'a> {
    pub tk: &'a mut cc_takeoff::model::Takeoff,
    pub c: u64,
    pub files: Vec<(String, std::sync::Arc<Vec<u8>>)>,
    pub opened: &'a mut Vec<(String, Vec<u8>)>,
}

impl Adapter for TkAdapter<'_> {
    fn apply(&mut self, ops: &Mp, _origin: &str) -> Result<(), String> {
        for op in ops_json(ops) {
            apply_one(&mut self.tk.state, &op);
        }
        self.tk.version += 1.0;
        Ok(())
    }
    fn invert(&mut self, ops: &Mp) -> Result<Mp, String> {
        Ok(Mp::Arr(invert(&self.tk.state, &ops_json(ops)).iter().map(to_mp).collect()))
    }
    fn touched(&self, ops: &Mp) -> Vec<TouchReq> {
        touched(&ops_json(ops))
    }
    fn snapshot(&mut self) -> Vec<u8> {
        let pdfs: Vec<Mp> =
            self.files.iter().map(|(n, b)| Mp::map(vec![("name", Mp::Str(n.clone())), ("bytes", Mp::Bin(b.to_vec()))])).collect();
        let v = Mp::map(vec![("v", Mp::from(SCHEMA)), ("takeoff", to_mp(&shared(&self.tk.state))), ("pdfs", Mp::Arr(pdfs))]);
        cc_live::msgpack::pack(&v).unwrap_or_default()
    }
    fn load(&mut self, bytes: &[u8], _q: u64) -> Result<(), String> {
        let x = cc_live::msgpack::unpack(bytes).map_err(|e| e.0.to_string())?;
        if x.get("v").and_then(Mp::as_u64) != Some(SCHEMA) {
            return Err("live snapshot: unknown format".into());
        }
        let mut st = x.get("takeoff").map(to_json).filter(J::is_object).ok_or("live snapshot: unknown format")?;
        let seq = self.tk.state.get("seq").and_then(J::as_f64).filter(|q| *q != 0.0).unwrap_or(0.0);
        if let (Some(o), Some(n)) = (st.as_object_mut(), serde_json::Number::from_f64(seq)) {
            o.insert("seq".into(), J::Number(n));
        }
        claim_ids(&mut st, self.c);
        self.tk.state = st;
        self.tk.undo_stack.clear();
        self.tk.redo_stack.clear();
        self.tk.version += 1.0;
        for p in x.get("pdfs").map(Mp::as_arr).unwrap_or(&[]) {
            if let (Some(n), Some(Mp::Bin(b))) = (p.get("name").and_then(Mp::as_str), p.get("bytes")) {
                self.opened.push((n.to_string(), b.clone()));
            }
        }
        Ok(())
    }
    fn validate(&mut self, ops: &Mp) -> Option<String> {
        validate(&self.tk.state, &ops_json(ops))
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The window's side (liveapps.rs): the takeoff of the open plan set, or (a joiner with nothing open) one held until the
// snapshot's PDFs open.

/// TakeoffCraft's [`crate::live::LiveView`] over the window.
pub struct TkLive<'a>(pub &'a mut crate::UiApp);

impl TkLive<'_> {
    fn tk(&mut self) -> &mut cc_takeoff::model::Takeoff {
        match self.0.pdf.as_mut() {
            Some(p) => &mut p.takeoff.tk,
            None => self.0.live_hold_tk.get_or_insert_with(Default::default),
        }
    }
    /// The PDFs a snapshot brought: opened under the takeoff (a joiner with nothing open gets a new plan set).
    fn open_pdfs(&mut self, list: Vec<(String, Vec<u8>)>) {
        let mut it = list.into_iter();
        if self.0.pdf.is_none() {
            let Some((name, bytes)) = it.next() else { return };
            match crate::pdfview::PdfView::open(&name, &bytes) {
                Ok(mut p) => {
                    p.mode = "takeoff";
                    p.zone = self.0.zone;
                    if let Some(tk) = self.0.live_hold_tk.take() {
                        p.takeoff.tk = tk;
                    }
                    self.0.pdf = Some(p);
                    self.0.seed_tk_snap();
                }
                Err(e) => {
                    self.0.message = format!("{name}: {e}");
                    return;
                }
            }
        }
        let Some(p) = self.0.pdf.as_mut() else { return };
        for (name, bytes) in it {
            if p.files().iter().any(|f| f.bytes.as_slice() == bytes.as_slice()) {
                continue;
            }
            let keep = p.takeoff.tk.state.clone();
            if let Err(e) = p.add_file(&name, &bytes) {
                p.takeoff.message = format!("{name}: {e}");
            }
            // opening a file is not an edit: the takeoff stays as the snapshot made it
            p.takeoff.tk.state = keep;
        }
        p.sync_takeoff();
    }
    /// Undo / Redo pressed while live (TakeoffMode::history asks): the person's own steps.
    pub fn reverse_asked(&mut self) {
        let Some(redo) = self.0.pdf.as_mut().and_then(|p| p.takeoff.live_req.take()) else { return };
        #[cfg(target_arch = "wasm32")]
        let _ = redo;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(l) = crate::live::reverse(self, !redo) {
            let m = match (l, redo) {
                (Some(w), false) => cc_takeoff::i18n::tf("undo.did", &[("what", cc_takeoff::i18n::F::S(w))]),
                (Some(w), true) => cc_takeoff::i18n::tf("redo.did", &[("what", cc_takeoff::i18n::F::S(w))]),
                (None, false) => cc_takeoff::i18n::t("undo.nothing"),
                (None, true) => cc_takeoff::i18n::t("redo.nothing"),
            };
            if let Some(p) = self.0.pdf.as_mut() {
                p.takeoff.message = m;
                p.takeoff.prune_sel();
            }
        }
    }
}

impl crate::live::LiveView for TkLive<'_> {
    const APP: &'static str = "takeoffcraft";
    const PREFIX: &'static str = "takeoff";
    fn live_slot(&mut self) -> &mut Option<crate::live::LiveSession> {
        &mut self.0.live
    }
    fn live_window_open(&mut self) -> &mut bool {
        &mut self.0.live_window
    }
    fn with_adapter(&mut self, c: u64, f: &mut dyn FnMut(&mut dyn Adapter)) {
        let files: Vec<(String, std::sync::Arc<Vec<u8>>)> =
            self.0.pdf.as_ref().map(|p| p.files().iter().map(|f| (f.name.clone(), f.bytes.clone())).collect()).unwrap_or_default();
        let mut opened = vec![];
        {
            let tk = self.tk();
            f(&mut TkAdapter { tk, c, files, opened: &mut opened });
        }
        if !opened.is_empty() {
            self.open_pdfs(opened);
        }
    }
    fn claim_ids(&mut self, c: u64) {
        claim_ids(&mut self.tk().state, c);
    }
    fn release_ids(&mut self) {
        let tk = self.tk();
        release_ids(&mut tk.state);
        tk.undo_stack.clear();
        tk.redo_stack.clear();
        if let Some(p) = self.0.pdf.as_mut() {
            p.takeoff.live_on = false;
        }
    }
    fn live_bind(&mut self) {
        let tk = self.tk();
        tk.undo_stack.clear();
        tk.redo_stack.clear();
        if let Some(p) = self.0.pdf.as_mut() {
            p.takeoff.live_on = true;
            p.mode = "takeoff";
        }
    }
    fn model_version(&self) -> u64 {
        let v = self.0.pdf.as_ref().map_or_else(|| self.0.live_hold_tk.as_ref().map_or(0.0, |t| t.version), |p| p.takeoff.tk.version);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let n = v.max(0.0) as u64;
        n
    }
    fn after_remote(&mut self) {
        if let Some(p) = self.0.pdf.as_mut() {
            p.takeoff.live_on = true;
            p.takeoff.prune_sel();
        }
    }
    fn tr(&self, key: &str, vars: &[(&str, String)]) -> String {
        let v: Vec<(&str, cc_takeoff::i18n::F)> = vars.iter().map(|(k, s)| (*k, cc_takeoff::i18n::F::S(s.clone()))).collect();
        cc_takeoff::i18n::tf(key, &v)
    }
    fn say(&mut self, msg: String) {
        if let Some(p) = self.0.pdf.as_mut() {
            p.takeoff.message = msg.clone();
        }
        self.0.message = msg;
    }
    fn live_state(&mut self) -> Option<J> {
        Some(shared(&self.tk().state))
    }
    fn local_steps(&mut self, g: &mut dyn FnMut() -> String) -> Vec<(String, Mp, Mp)> {
        let tk = self.tk();
        if tk.undo_stack.is_empty() {
            return vec![];
        }
        let steps = std::mem::take(&mut tk.undo_stack);
        let mut out = vec![];
        for (i, st) in steps.iter().enumerate() {
            let after = steps.get(i + 1).map_or(&tk.state, |n| &n.before);
            let ops = diff(&st.before, after);
            if ops.is_empty() {
                continue;
            }
            let inverse = diff(after, &st.before);
            let mark = g();
            out.push((st.label.clone(), crate::livestate::with_mark(&mark, &ops), crate::livestate::with_mark(&mark, &inverse)));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_round_trip_and_marks_read_their_item() {
        let a = json!({"name": "T", "seq": 3, "items": [{"id": "i1", "type": "count", "name": "Doors"}], "marks": [], "sheets": {}, "files": []});
        let b = json!({"name": "T2", "seq": 5, "items": [{"id": "i1", "type": "count", "name": "Doors 2"}],
            "marks": [{"id": "k4", "item": "i1", "kind": "count", "pts": [[1, 2]], "sheet": "h:0"}], "sheets": {"h:0": {"scale": 1}}, "files": [{"hash": "h"}]});
        let ops = diff(&a, &b);
        assert_eq!(op_s(&ops[0], "o"), "file");
        let mut x = a.clone();
        let inv = invert(&x, &ops);
        for o in &ops {
            apply_one(&mut x, o);
        }
        assert_eq!(crate::livestate::canonical(&shared(&x)), crate::livestate::canonical(&shared(&b)));
        for o in &inv {
            apply_one(&mut x, o);
        }
        // the file stays listed (opening one is not an edit); the rest is back
        assert_eq!(x["name"], "T");
        assert!(x["marks"].as_array().unwrap().is_empty());
        let t = touched(&ops);
        assert!(t.iter().any(|t| t.k == "e:marks/k4" && t.m == 'c'));
        assert!(t.iter().any(|t| t.k == "e:items/i1" && t.m == 'r'));
        assert!(t.iter().any(|t| t.k == "meta:sheets/h:0"));
        assert_eq!(validate(&a, &ops), None);
        let bad = vec![json!({"o": "add", "c": "marks", "id": "k9", "v": {"item": "nope", "kind": "count", "pts": [[0, 0]]}})];
        assert_eq!(validate(&a, &bad).as_deref(), Some("invalid"));
        let mut s = b.clone();
        claim_ids(&mut s, 2);
        assert_eq!(s["seq"], json!(2_000_000.0));
        let _ = to_json(&Mp::Nil);
    }
}
