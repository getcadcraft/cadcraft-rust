//! The state-diff core of 0.5's Studio Live adapters (`stateCore(cfg)` in detailcraft/, modelcraft/ and civilcraft/
//! dev/live.js — the same function in all three; session live-apps). A document is plain JSON; an edit's ops are the
//! difference of the document before and after it:
//!
//! - `{o:'add', path, id, v, at}` an item of a list created at its place — key `e:<path>/#<id>` (c)
//! - `{o:'del', path, id}` an item removed (d)
//! - `{o:'put', path, v | x:1}` one value — in an item `e:<item>` field `<rest>` (p), else `meta:<path>` (p)
//! - `{o:'atom', path, v}` an atomic group of an item's fields — `e:<item>` (s)
//! - `{o:'ord', path, v:[ids]}` a list's order — `meta:<path>.order` (p)
//!
//! [`Cfg`] says which arrays are lists merged per item (path pattern → id field, `*` = an item), which fields only make
//! sense together (atomic groups), which are personal (never sent), how deep plain objects merge field by field, and
//! which items an op's value names (reads).

use cc_detail::js::stringify;
use cc_live::doc::{Cond, TouchReq};
use cc_live::msgpack::Value as Mp;
use serde_json::{Map, Value as J};

/// `cfg.refs(op)`: the item keys an op's value names (op, its path, out).
pub type Refs = fn(&J, &[String], &mut Vec<String>);

/// The core's configuration (live.js `stateCore(cfg)`).
pub struct Cfg {
    /// list path patterns and their id fields
    pub lists: &'static [(&'static str, &'static str)],
    /// (item pattern, group name, fields)
    pub atomic: &'static [(&'static str, &'static str, &'static [&'static str])],
    pub personal: &'static [&'static str],
    pub depth: usize,
    /// item keys an op's value names (`cfg.refs(op)`)
    pub refs: Option<Refs>,
}

/// JSON ↔ MessagePack as live-client.js packs JS values (integers that are safe as int, other numbers float64).
pub fn to_mp(v: &J) -> Mp {
    match v {
        J::Null => Mp::Nil,
        J::Bool(b) => Mp::Bool(*b),
        J::Number(n) => {
            if let Some(i) = n.as_i64() {
                Mp::Int(i128::from(i))
            } else if let Some(u) = n.as_u64() {
                Mp::Int(i128::from(u))
            } else {
                let f = n.as_f64().unwrap_or(0.0);
                if f.fract() == 0.0 && f.abs() <= 9_007_199_254_740_991.0 {
                    #[allow(clippy::cast_possible_truncation)]
                    let i = f as i64;
                    Mp::Int(i128::from(i))
                } else {
                    Mp::F64(f)
                }
            }
        }
        J::String(s) => Mp::Str(s.clone()),
        J::Array(a) => Mp::Arr(a.iter().map(to_mp).collect()),
        J::Object(o) => Mp::Map(o.iter().map(|(k, v)| (k.clone(), to_mp(v))).collect()),
    }
}

/// MessagePack → JSON (bytes become an array of numbers; the adapters that carry bytes read them from `Mp` directly).
pub fn to_json(v: &Mp) -> J {
    match v {
        Mp::Nil | Mp::Ext(..) => J::Null,
        Mp::Bool(b) => J::Bool(*b),
        Mp::Int(i) => i64::try_from(*i).map_or_else(
            |_| {
                #[allow(clippy::cast_precision_loss)]
                let f = *i as f64;
                serde_json::Number::from_f64(f).map_or(J::Null, J::Number)
            },
            |x| J::Number(x.into()),
        ),
        Mp::F64(f) => serde_json::Number::from_f64(*f).map_or(J::Null, J::Number),
        Mp::Str(s) => J::String(s.clone()),
        Mp::Bin(b) => J::Array(b.iter().map(|x| J::from(*x)).collect()),
        Mp::Arr(a) => J::Array(a.iter().map(to_json).collect()),
        Mp::Map(m) => J::Object(m.iter().map(|(k, v)| (k.clone(), to_json(v))).collect()),
    }
}

/// The ops of a MessagePack array as JSON.
pub fn ops_json(ops: &Mp) -> Vec<J> {
    ops.as_arr().iter().map(to_json).collect()
}
/// `[{o:'g', g}, …ops]` as MessagePack.
pub fn with_mark(g: &str, ops: &[J]) -> Mp {
    let mark = serde_json::json!({"o": "g", "g": g});
    Mp::Arr(std::iter::once(&mark).chain(ops.iter()).map(to_mp).collect())
}

/// Stable text of a document (object keys sorted): live.js `canonical`, what the convergence checks compare.
pub fn canonical(v: &J) -> String {
    fn walk(x: &J) -> J {
        match x {
            J::Array(a) => J::Array(a.iter().map(walk).collect()),
            J::Object(o) => {
                let mut ks: Vec<&String> = o.keys().collect();
                ks.sort();
                J::Object(ks.into_iter().map(|k| (k.clone(), walk(o.get(k).unwrap_or(&J::Null)))).collect())
            }
            o => o.clone(),
        }
    }
    stringify(&walk(v))
}

pub fn same(a: Option<&J>, b: Option<&J>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => stringify(x) == stringify(y),
        _ => false,
    }
}
/// `String(id)` as live.js writes an item into a path ('#' + id).
pub fn id_text(v: &J) -> String {
    match v {
        J::String(s) => s.clone(),
        J::Null => "null".into(),
        J::Bool(b) => b.to_string(),
        _ => stringify(v),
    }
}
fn id_of<'a>(x: &'a J, f: &str) -> Option<&'a J> {
    x.as_object().and_then(|o| o.get(f))
}
fn pj(path: &[String]) -> J {
    J::Array(path.iter().cloned().map(J::String).collect())
}
fn op_put(path: Vec<String>, v: Option<J>) -> J {
    let mut o = Map::new();
    o.insert("o".into(), J::from("put"));
    o.insert("path".into(), J::Array(path.into_iter().map(J::String).collect()));
    match v {
        Some(v) => o.insert("v".into(), v),
        None => o.insert("x".into(), J::from(1)),
    };
    J::Object(o)
}
fn seg(path: &J) -> Vec<String> {
    path.as_array().map(|a| a.iter().map(|s| s.as_str().map_or_else(|| stringify(s), str::to_string)).collect()).unwrap_or_default()
}
fn idx(op: &J) -> Option<usize> {
    op.get("at").and_then(J::as_u64).and_then(|a| usize::try_from(a).ok())
}

struct Out {
    mid: Vec<J>,
    post: Vec<J>,
}

impl Cfg {
    fn pat(path: &[String]) -> String {
        path.iter().map(|s| if s.starts_with('#') { "*" } else { s.as_str() }).collect::<Vec<_>>().join("/")
    }
    fn list_id(&self, path: &[String]) -> Option<&'static str> {
        let p = Self::pat(path);
        self.lists.iter().find(|(k, _)| *k == p).map(|(_, f)| *f)
    }
    fn is_personal(&self, path: &[String]) -> bool {
        let p = Self::pat(path);
        self.personal.contains(&p.as_str())
    }
    fn groups(&self, item: &[String]) -> Vec<(&'static str, &'static [&'static str])> {
        let p = Self::pat(item);
        self.atomic.iter().filter(|(k, _, _)| *k == p).map(|(_, g, f)| (*g, *f)).collect()
    }
    fn unique_ids(arr: &[J], f: &str) -> bool {
        let mut seen = std::collections::HashSet::new();
        arr.iter().all(|x| match id_of(x, f) {
            None | Some(J::Null) => false,
            Some(k) => seen.insert(stringify(k)),
        })
    }
    /// An item's value without its personal fields.
    pub fn strip(&self, path: &[String], v: &J) -> J {
        match v {
            J::Object(o) => {
                let mut out = Map::new();
                for (k, x) in o {
                    let mut p = path.to_vec();
                    p.push(k.clone());
                    if !self.is_personal(&p) {
                        out.insert(k.clone(), if x.is_object() || x.is_array() { self.strip(&p, x) } else { x.clone() });
                    }
                }
                J::Object(out)
            }
            other => other.clone(),
        }
    }

    fn diff_into(&self, a: Option<&J>, b: Option<&J>, path: &[String], out: &mut Out, lvl: usize) {
        if let (Some(f), Some(J::Array(aa)), Some(J::Array(bb))) = (self.list_id(path), a, b)
            && Self::unique_ids(aa, f)
            && Self::unique_ids(bb, f)
        {
            let key = |x: &J| id_of(x, f).map(stringify).unwrap_or_default();
            let ak: Vec<String> = aa.iter().map(key).collect();
            let bk: Vec<String> = bb.iter().map(key).collect();
            let aset: std::collections::HashMap<&String, usize> = ak.iter().enumerate().map(|(i, k)| (k, i)).collect();
            let bset: std::collections::HashSet<&String> = bk.iter().collect();
            for (x, k) in aa.iter().zip(&ak) {
                if !bset.contains(k) {
                    out.mid.push(serde_json::json!({"o": "del", "path": pj(path), "id": id_of(x, f).cloned().unwrap_or(J::Null)}));
                }
            }
            for (at, (x, k)) in bb.iter().zip(&bk).enumerate() {
                if !aset.contains_key(k) {
                    let mut p = path.to_vec();
                    p.push(format!("#{}", id_text(id_of(x, f).unwrap_or(&J::Null))));
                    out.mid.push(serde_json::json!({"o": "add", "path": pj(path), "id": id_of(x, f).cloned().unwrap_or(J::Null), "v": self.strip(&p, x), "at": at}));
                }
            }
            for (x, k) in bb.iter().zip(&bk) {
                if let Some(&i) = aset.get(k)
                    && let Some(ax) = aa.get(i)
                {
                    let mut p = path.to_vec();
                    p.push(format!("#{}", id_text(id_of(x, f).unwrap_or(&J::Null))));
                    self.item(&p, ax, x, out);
                }
            }
            let was: Vec<&String> = ak.iter().filter(|k| bset.contains(k)).collect();
            let now: Vec<&String> = bk.iter().filter(|k| aset.contains_key(k)).collect();
            if was != now {
                out.post.push(serde_json::json!({"o": "ord", "path": pj(path), "v": bb.iter().map(|x| id_of(x, f).cloned().unwrap_or(J::Null)).collect::<Vec<_>>()}));
            }
            return;
        }
        if let (Some(J::Object(ao)), Some(J::Object(bo))) = (a, b)
            && lvl < self.depth
        {
            let mut keys: Vec<&String> = ao.keys().collect();
            keys.extend(bo.keys().filter(|k| !ao.contains_key(*k)));
            for k in keys {
                let mut p = path.to_vec();
                p.push(k.clone());
                let (x, y) = (ao.get(k), bo.get(k));
                if self.is_personal(&p) || same(x, y) {
                    continue;
                }
                match (x, y) {
                    (_, None) => out.mid.push(op_put(p, None)),
                    (None, Some(y)) => {
                        let v = self.strip(&p, y);
                        out.mid.push(op_put(p, Some(v)));
                    }
                    (Some(_), Some(_)) => self.diff_into(x, y, &p, out, lvl.saturating_add(1)),
                }
            }
            return;
        }
        if !same(a, b) {
            out.mid.push(match b {
                None => op_put(path.to_vec(), None),
                Some(y) => op_put(path.to_vec(), Some(self.strip(path, y))),
            });
        }
    }

    /// One item of a list, both sides: its atomic groups, then its fields (lists inside it merge per item too).
    fn item(&self, path: &[String], a: &J, b: &J, out: &mut Out) {
        let (Some(ao), Some(bo)) = (a.as_object(), b.as_object()) else {
            if !same(Some(a), Some(b)) {
                out.mid.push(op_put(path.to_vec(), Some(self.strip(path, b))));
            }
            return;
        };
        let groups = self.groups(path);
        let mut in_group: Vec<&str> = Vec::new();
        for (name, fields) in &groups {
            in_group.extend(fields.iter().copied());
            if fields.iter().any(|k| !same(ao.get(*k), bo.get(*k))) {
                let mut v = Map::new();
                for k in fields.iter() {
                    if let Some(x) = bo.get(*k) {
                        v.insert((*k).to_string(), x.clone());
                    }
                }
                let mut p = path.to_vec();
                p.push((*name).to_string());
                out.mid.push(serde_json::json!({"o": "atom", "path": pj(&p), "v": J::Object(v)}));
            }
        }
        let mut keys: Vec<&String> = ao.keys().collect();
        keys.extend(bo.keys().filter(|k| !ao.contains_key(*k)));
        for k in keys {
            let mut p = path.to_vec();
            p.push(k.clone());
            let (x, y) = (ao.get(k), bo.get(k));
            if in_group.contains(&k.as_str()) || self.is_personal(&p) || same(x, y) {
                continue;
            }
            if self.list_id(&p).is_some() {
                let empty = J::Array(vec![]);
                let falsy = |v: Option<&J>| {
                    matches!(v, None | Some(J::Null | J::Bool(false))) || v.is_some_and(|v| v == &J::from(0) || v == &J::from(""))
                };
                let xa = if falsy(x) { &empty } else { x.unwrap_or(&empty) };
                let ya = if falsy(y) { &empty } else { y.unwrap_or(&empty) };
                self.diff_into(Some(xa), Some(ya), &p, out, 0);
            } else {
                out.mid.push(match y {
                    None => op_put(p, None),
                    Some(y) => {
                        let v = self.strip(&p, y);
                        op_put(p, Some(v))
                    }
                });
            }
        }
    }

    /// live.js `CORE.diff(a, b)`: the ops that turn document a into document b.
    pub fn diff(&self, a: &J, b: &J) -> Vec<J> {
        let mut out = Out { mid: vec![], post: vec![] };
        self.diff_into(Some(a), Some(b), &[], &mut out, 0);
        out.mid.extend(out.post);
        out.mid
    }

    fn find_mut<'a>(&self, root: &'a mut J, path: &[String]) -> Option<&'a mut J> {
        let mut cur = root;
        for (i, s) in path.iter().enumerate() {
            cur = if let Some(id) = s.strip_prefix('#') {
                let f = self.list_id(path.get(..i)?)?;
                cur.as_array_mut()?.iter_mut().find(|x| id_of(x, f).is_some_and(|v| id_text(v) == id))?
            } else {
                cur.as_object_mut()?.get_mut(s)?
            };
        }
        Some(cur)
    }
    pub fn get<'a>(&self, root: &'a J, path: &[String]) -> Option<&'a J> {
        let mut cur = root;
        for (i, s) in path.iter().enumerate() {
            cur = if let Some(id) = s.strip_prefix('#') {
                let f = self.list_id(path.get(..i)?)?;
                cur.as_array()?.iter().find(|x| id_of(x, f).is_some_and(|v| id_text(v) == id))?
            } else {
                cur.as_object()?.get(s)?
            };
        }
        Some(cur)
    }

    fn apply_one(&self, root: &mut J, op: &J) {
        let path = seg(op.get("path").unwrap_or(&J::Null));
        let o = op.get("o").and_then(J::as_str).unwrap_or("");
        let f = self.list_id(&path);
        match o {
            "add" => {
                let Some(f) = f else { return };
                let Some(id) = op.get("id") else { return };
                if !self.get(root, &path).is_some_and(J::is_array) {
                    let Some((last, up)) = path.split_last() else { return };
                    if last.starts_with('#') {
                        return;
                    }
                    let Some(J::Object(box_)) = self.find_mut(root, up) else { return };
                    box_.insert(last.clone(), J::Array(vec![]));
                }
                let Some(J::Array(l)) = self.find_mut(root, &path) else { return };
                let v = op.get("v").cloned().unwrap_or(J::Null);
                match l.iter().position(|x| id_of(x, f).is_some_and(|k| stringify(k) == stringify(id))) {
                    Some(i) => {
                        if let Some(slot) = l.get_mut(i) {
                            *slot = v;
                        }
                    }
                    None => {
                        let at = idx(op).unwrap_or(l.len()).min(l.len());
                        l.insert(at, v);
                    }
                }
            }
            "del" => {
                let (Some(f), Some(id)) = (f, op.get("id")) else { return };
                if let Some(J::Array(l)) = self.find_mut(root, &path) {
                    let id = stringify(id);
                    if let Some(i) = l.iter().position(|x| id_of(x, f).is_some_and(|k| stringify(k) == id)) {
                        l.remove(i);
                    }
                }
            }
            "put" => {
                let Some((last, up)) = path.split_last() else { return };
                if last.starts_with('#') {
                    return;
                }
                let Some(J::Object(box_)) = self.find_mut(root, up) else { return };
                if op.get("x").is_some_and(|x| !x.is_null() && x != &J::from(0) && x != &J::Bool(false)) {
                    box_.shift_remove(last);
                } else {
                    box_.insert(last.clone(), op.get("v").cloned().unwrap_or(J::Null));
                }
            }
            "atom" => {
                let Some((name, up)) = path.split_last() else { return };
                let fields: Vec<&str> = self.groups(up).into_iter().find(|(g, _)| g == name).map(|(_, f)| f.to_vec()).unwrap_or_default();
                let v = op.get("v").cloned().unwrap_or(J::Null);
                let Some(J::Object(it)) = self.find_mut(root, up) else { return };
                for k in fields {
                    match v.get(k) {
                        None => {
                            it.shift_remove(k);
                        }
                        Some(x) => {
                            it.insert(k.to_string(), x.clone());
                        }
                    }
                }
            }
            "ord" => {
                let Some(f) = f else { return };
                let ids: Vec<String> = op.get("v").and_then(J::as_array).map(|a| a.iter().map(stringify).collect()).unwrap_or_default();
                if let Some(J::Array(l)) = self.find_mut(root, &path) {
                    let mut rest: Vec<Option<J>> = std::mem::take(l).into_iter().map(Some).collect();
                    let keys: Vec<String> =
                        rest.iter().map(|x| x.as_ref().and_then(|x| id_of(x, f)).map(stringify).unwrap_or_default()).collect();
                    let mut next: Vec<J> = Vec::with_capacity(rest.len());
                    for id in &ids {
                        if let Some(i) = keys.iter().position(|k| k == id)
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

    /// live.js `CORE.apply(root, ops)`: ops on objects that are gone are skipped.
    pub fn apply(&self, root: &mut J, ops: &[J]) {
        for op in ops.iter().take(10_000_000) {
            self.apply_one(root, op);
        }
    }

    fn ids_of(&self, root: &J, path: &[String]) -> Vec<J> {
        let Some(f) = self.list_id(path) else { return vec![] };
        self.get(root, path)
            .and_then(J::as_array)
            .map(|l| l.iter().map(|x| id_of(x, f).cloned().unwrap_or(J::Null)).collect())
            .unwrap_or_default()
    }

    /// live.js `CORE.invert(root, ops)`: the inverse of ops on `root` as it is now (list positions walked op by op).
    pub fn invert(&self, root: &J, ops: &[J]) -> Vec<J> {
        let mut lists: Vec<(Vec<String>, Vec<J>)> = Vec::new();
        let mut out: Vec<Option<J>> = Vec::new();
        for op in ops {
            let o = op.get("o").and_then(J::as_str).unwrap_or("");
            let path = seg(op.get("path").unwrap_or(&J::Null));
            let inv = match o {
                "g" => Some(op.clone()),
                "add" | "del" => {
                    let id = op.get("id").cloned().unwrap_or(J::Null);
                    if !lists.iter().any(|(p, _)| *p == path) {
                        lists.push((path.clone(), self.ids_of(root, &path)));
                    }
                    let Some(l) = lists.iter_mut().find(|(p, _)| *p == path).map(|(_, l)| l) else { continue };
                    let i = l.iter().position(|x| stringify(x) == stringify(&id));
                    let mut ip = path.clone();
                    ip.push(format!("#{}", id_text(&id)));
                    let x = i.and_then(|_| self.get(root, &ip));
                    let inv = match (x, i) {
                        (Some(x), Some(i)) => {
                            serde_json::json!({"o": "add", "path": pj(&path), "id": id, "v": self.strip(&ip, x), "at": i})
                        }
                        _ => serde_json::json!({"o": "del", "path": pj(&path), "id": id}),
                    };
                    if o == "add" {
                        if i.is_none() {
                            let at = idx(op).unwrap_or(l.len()).min(l.len());
                            l.insert(at, id);
                        }
                    } else if let Some(i) = i {
                        l.remove(i);
                    }
                    Some(inv)
                }
                "put" => Some(match self.get(root, &path) {
                    None => op_put(path, None),
                    Some(v) => op_put(path, Some(v.clone())),
                }),
                "atom" => {
                    let Some((name, up)) = path.split_last() else { continue };
                    let fields: Vec<&str> =
                        self.groups(up).into_iter().find(|(g, _)| g == name).map(|(_, f)| f.to_vec()).unwrap_or_default();
                    self.get(root, up).map(|it| {
                        let mut v = Map::new();
                        for k in fields {
                            if let Some(x) = it.get(k) {
                                v.insert(k.to_string(), x.clone());
                            }
                        }
                        serde_json::json!({"o": "atom", "path": pj(&path), "v": J::Object(v)})
                    })
                }
                "ord" => {
                    if !lists.iter().any(|(p, _)| *p == path) {
                        lists.push((path.clone(), self.ids_of(root, &path)));
                    }
                    let Some(l) = lists.iter_mut().find(|(p, _)| *p == path).map(|(_, l)| l) else { continue };
                    let inv = serde_json::json!({"o": "ord", "path": pj(&path), "v": l.clone()});
                    let want: Vec<String> =
                        op.get("v").and_then(J::as_array).map(|a| a.iter().map(stringify).collect()).unwrap_or_default();
                    let mut next: Vec<J> = want.iter().filter_map(|w| l.iter().find(|x| &stringify(x) == w).cloned()).collect();
                    for x in l.iter() {
                        if !next.iter().any(|y| stringify(y) == stringify(x)) {
                            next.push(x.clone());
                        }
                    }
                    *l = next;
                    Some(inv)
                }
                _ => None,
            };
            out.push(inv);
        }
        let mut r: Vec<J> = out.into_iter().rev().flatten().collect();
        if let Some(gi) = r.iter().position(|o| o.get("o").and_then(J::as_str) == Some("g"))
            && gi > 0
        {
            let g = r.remove(gi);
            r.insert(0, g);
        }
        r
    }

    /// Where an op lands: the innermost list item on its path (key e:…) and the field below it, else a meta key.
    fn key_of(path: &[String]) -> (String, Option<String>) {
        match path.iter().rposition(|s| s.starts_with('#')) {
            None => (format!("meta:{}", path.join("/")), None),
            Some(last) => (
                format!("e:{}", path.get(..=last).unwrap_or(&[]).join("/")),
                Some(path.get(last.saturating_add(1)..).unwrap_or(&[]).join("/")),
            ),
        }
    }

    /// live.js `CORE.touched(ops)`.
    pub fn touched(&self, ops: &[J]) -> Vec<TouchReq> {
        let mut out: Vec<TouchReq> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut add = |out: &mut Vec<TouchReq>, k: String, m: char, f: Option<String>| {
            if seen.insert(format!("{k}\u{0}{m}\u{0}{}", f.clone().unwrap_or_default())) {
                out.push(TouchReq { k, m, f, r: Cond::Auto, g: false });
            }
        };
        for op in ops {
            let o = op.get("o").and_then(J::as_str).unwrap_or("");
            if o == "g" {
                continue;
            }
            let path = seg(op.get("path").unwrap_or(&J::Null));
            match o {
                "add" | "del" => {
                    let id = id_text(op.get("id").unwrap_or(&J::Null));
                    let mut p = path.clone();
                    p.push(format!("#{id}"));
                    add(&mut out, format!("e:{}", p.join("/")), if o == "add" { 'c' } else { 'd' }, None);
                }
                "ord" => add(&mut out, format!("meta:{}.order", path.join("/")), 'p', Some("v".into())),
                "atom" => {
                    let up = path.get(..path.len().saturating_sub(1)).unwrap_or(&[]);
                    add(&mut out, Self::key_of(up).0, 's', None);
                }
                "put" => {
                    let (k, f) = Self::key_of(&path);
                    add(&mut out, k, 'p', Some(f.filter(|f| !f.is_empty()).unwrap_or_else(|| "v".into())));
                }
                _ => {}
            }
            let mut rs: Vec<String> = Vec::new();
            let upto = if o == "add" || o == "del" { path.len() } else { path.len().saturating_sub(1) };
            for i in 0..path.len() {
                if path.get(i).is_some_and(|s| s.starts_with('#')) && i < upto {
                    rs.push(format!("e:{}", path.get(..=i).unwrap_or(&[]).join("/")));
                }
            }
            if let Some(refs) = self.refs
                && op.get("v").is_some()
            {
                refs(op, &path, &mut rs);
            }
            for k in rs {
                if !out.iter().any(|x| x.k == k && x.m != 'r') {
                    add(&mut out, k, 'r', None);
                }
            }
        }
        out
    }

    /// An edit's wire ops and inverse (`[{o:'g'}, …diff]`), or None when nothing travels.
    pub fn step(&self, g: &str, before: &J, after: &J) -> Option<(Mp, Mp)> {
        let ops = self.diff(before, after);
        if ops.is_empty() {
            return None;
        }
        let inverse = self.diff(after, before);
        Some((with_mark(g, &ops), with_mark(g, &inverse)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    static CFG: Cfg = Cfg {
        lists: &[("defs", "id"), ("defs/*/insts", "id")],
        atomic: &[("defs/*", "geo", &["verts", "faces"])],
        personal: &["nextId", "defs/*/next"],
        depth: 1,
        refs: None,
    };

    #[test]
    fn atomic_groups_travel_whole_and_invert() {
        let a = serde_json::json!({"nextId": 4, "defs": [{"id": "root", "next": 3, "verts": [1], "faces": [2], "name": "a", "insts": [{"id": 1, "def": "g"}]}], "units": "mm"});
        let b = serde_json::json!({"nextId": 9, "defs": [{"id": "root", "next": 7, "verts": [1, 5], "faces": [2], "name": "b", "insts": [{"id": 1, "def": "g", "x": 3}, {"id": 2, "def": "g"}]}], "units": "in"});
        let ops = CFG.diff(&a, &b);
        assert!(ops.iter().any(|o| o["o"] == "atom" && o["v"]["verts"] == serde_json::json!([1, 5])));
        assert!(!ops.iter().any(|o| stringify(o).contains("next")), "{ops:?}");
        let mut x = a.clone();
        let inv = CFG.invert(&x, &ops);
        CFG.apply(&mut x, &ops);
        let strip = |v: &J| {
            let mut v = v.clone();
            v.as_object_mut().unwrap().remove("nextId");
            v["defs"][0].as_object_mut().unwrap().remove("next");
            canonical(&v)
        };
        assert_eq!(strip(&x), strip(&b));
        CFG.apply(&mut x, &inv);
        assert_eq!(strip(&x), strip(&a));
        let t = CFG.touched(&ops);
        assert!(t.iter().any(|t| t.k == "e:defs/#root" && t.m == 's'), "{t:?}");
        assert!(t.iter().any(|t| t.k == "e:defs/#root/insts/#2" && t.m == 'c'));
        assert!(t.iter().any(|t| t.k == "meta:units"));
    }
}
