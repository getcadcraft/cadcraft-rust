//! DetailCraft in a Studio Live session (0.5 detailcraft/dev/live.js, the app adapter, LV.22): the model's state is
//! plain JSON and every edit's ops are the difference of the state before and after it (live.js's state-diff core:
//! members, connections, footings, stairs, bolts and issues merge per item and field, profiles per name, the rest as
//! values; a connection, bolt group or stair reads the members it names). Built geometry, marks and schedules are never
//! sent: every window works them out. Each person's new ids come from their own range (client c: c × 1e6 + n).
//!
//! The session itself is cc-live's: the host's window runs the session server on the LAN (`detail.live.host`, a short code
//! for joiners, approval of each), a joiner connects with the code (`detail.live.join`), and both go through
//! [`cc_live::doc::LiveDoc`] (pending edits, rebase, refusals, per-person undo). The ops and snapshots are the bytes 0.5's
//! live.js makes, so a 0.5 DetailCraft page and this window edit one model together.
//!
//! Commands: `detail.live.host` = `[bind address]` (default the LAN on a free port), `detail.live.join` = `<host:port> <code | plain>`,
//! `detail.live.approve` = `<c> [editor|viewer]`, `detail.live.deny` = `<c>`, `detail.live.invite`, `detail.live.leave`, `detail.live.pump` = `<ms>` (headless:
//! carry messages until everything sent is decided, at most that long).

#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::{Receiver, channel};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

use cc_detail::js::{parse, stringify};
use cc_live::doc::{Adapter, Cond, TouchReq};
#[cfg(not(target_arch = "wasm32"))]
use cc_live::doc::{DocEvent, LiveDoc};
#[cfg(not(target_arch = "wasm32"))]
use cc_live::envelope::Envelope;
use cc_live::msgpack::Value as Mp;
use serde_json::{Map, Value as J};

use crate::detailview::DetailView;

pub const SCHEMA: u64 = 1;
const ID_BLOCK: f64 = 1e6;

// ---------------------------------------------------------------------------------------------------------------------
// JSON ↔ MessagePack as live-client.js packs JS values (integers that are safe as int, other numbers float64)

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

// ---------------------------------------------------------------------------------------------------------------------
// live.js's state core, configured for DetailCraft

/// The id field of the list at a path pattern (`members/*/holes` …).
fn list_field(pat: &str) -> Option<&'static str> {
    Some(match pat {
        "members" | "members/*/holes" | "members/*/cuts" | "connections" | "footings" | "stairs" | "bolts" => "id",
        "profiles" => "name",
        "issues" => "guid",
        _ => return None,
    })
}
const DEPTH: usize = 2;

fn pat(path: &[String]) -> String {
    path.iter().map(|s| if s.starts_with('#') { "*" } else { s.as_str() }).collect::<Vec<_>>().join("/")
}
fn list_id(path: &[String]) -> Option<&'static str> {
    list_field(&pat(path))
}
fn is_personal(path: &[String]) -> bool {
    pat(path) == "seq"
}
fn same(a: Option<&J>, b: Option<&J>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => stringify(x) == stringify(y),
        _ => false,
    }
}
/// `String(id)` as live.js writes an item into a path ('#' + id).
fn id_text(v: &J) -> String {
    match v {
        J::String(s) => s.clone(),
        J::Number(_) => stringify(v),
        J::Null => "null".into(),
        J::Bool(b) => b.to_string(),
        _ => stringify(v),
    }
}
fn id_of<'a>(x: &'a J, f: &str) -> Option<&'a J> {
    x.as_object().and_then(|o| o.get(f))
}
fn unique_ids(arr: &[J], f: &str) -> bool {
    let mut seen = std::collections::HashSet::new();
    arr.iter().all(|x| match id_of(x, f) {
        None | Some(J::Null) => false,
        Some(k) => seen.insert(stringify(k)),
    })
}
/// An item's value without its personal fields.
fn strip(path: &[String], v: &J) -> J {
    match v {
        J::Object(o) => {
            let mut out = Map::new();
            for (k, x) in o {
                let mut p = path.to_vec();
                p.push(k.clone());
                if !is_personal(&p) {
                    out.insert(k.clone(), if x.is_object() || x.is_array() { strip(&p, x) } else { x.clone() });
                }
            }
            J::Object(out)
        }
        other => other.clone(),
    }
}

fn op_put(path: Vec<String>, v: Option<J>) -> J {
    let mut o = Map::new();
    o.insert("o".into(), J::from("put"));
    o.insert("path".into(), J::Array(path.into_iter().map(J::String).collect()));
    match v {
        Some(v) => {
            o.insert("v".into(), v);
        }
        None => {
            o.insert("x".into(), J::from(1));
        }
    }
    J::Object(o)
}
fn pj(path: &[String]) -> J {
    J::Array(path.iter().cloned().map(J::String).collect())
}

struct Out {
    mid: Vec<J>,
    post: Vec<J>,
}

fn diff_into(a: Option<&J>, b: Option<&J>, path: &[String], out: &mut Out, lvl: usize) {
    if let (Some(f), Some(J::Array(aa)), Some(J::Array(bb))) = (list_id(path), a, b)
        && unique_ids(aa, f)
        && unique_ids(bb, f)
    {
        let key = |x: &J| id_of(x, f).map(stringify).unwrap_or_default();
        let ak: Vec<String> = aa.iter().map(key).collect();
        let bk: Vec<String> = bb.iter().map(key).collect();
        for (x, k) in aa.iter().zip(&ak) {
            if !bk.contains(k) {
                out.mid.push(serde_json::json!({"o": "del", "path": pj(path), "id": id_of(x, f).cloned().unwrap_or(J::Null)}));
            }
        }
        for (at, (x, k)) in bb.iter().zip(&bk).enumerate() {
            if !ak.contains(k) {
                let mut p = path.to_vec();
                p.push(format!("#{}", id_text(id_of(x, f).unwrap_or(&J::Null))));
                out.mid.push(serde_json::json!({"o": "add", "path": pj(path), "id": id_of(x, f).cloned().unwrap_or(J::Null), "v": strip(&p, x), "at": at}));
            }
        }
        for (x, k) in bb.iter().zip(&bk) {
            if let Some(i) = ak.iter().position(|y| y == k)
                && let Some(ax) = aa.get(i)
            {
                let mut p = path.to_vec();
                p.push(format!("#{}", id_text(id_of(x, f).unwrap_or(&J::Null))));
                item(&p, ax, x, out);
            }
        }
        let was: Vec<&String> = ak.iter().filter(|k| bk.contains(k)).collect();
        let now: Vec<&String> = bk.iter().filter(|k| ak.contains(k)).collect();
        if was != now {
            out.post.push(serde_json::json!({"o": "ord", "path": pj(path), "v": bb.iter().map(|x| id_of(x, f).cloned().unwrap_or(J::Null)).collect::<Vec<_>>()}));
        }
        return;
    }
    if let (Some(J::Object(ao)), Some(J::Object(bo))) = (a, b)
        && lvl < DEPTH
    {
        let mut keys: Vec<&String> = ao.keys().collect();
        for k in bo.keys() {
            if !ao.contains_key(k) {
                keys.push(k);
            }
        }
        for k in keys {
            let mut p = path.to_vec();
            p.push(k.clone());
            let (x, y) = (ao.get(k), bo.get(k));
            if is_personal(&p) || same(x, y) {
                continue;
            }
            match (x, y) {
                (_, None) => out.mid.push(op_put(p, None)),
                (None, Some(y)) => {
                    let v = strip(&p, y);
                    out.mid.push(op_put(p, Some(v)));
                }
                (Some(_), Some(_)) => diff_into(x, y, &p, out, lvl.saturating_add(1)),
            }
        }
        return;
    }
    if !same(a, b) {
        out.mid.push(match b {
            None => op_put(path.to_vec(), None),
            Some(y) => op_put(path.to_vec(), Some(strip(path, y))),
        });
    }
}

/// One item of a list, both sides: its fields (lists inside it merge per item too). DetailCraft has no atomic groups.
fn item(path: &[String], a: &J, b: &J, out: &mut Out) {
    let (Some(ao), Some(bo)) = (a.as_object(), b.as_object()) else {
        if !same(Some(a), Some(b)) {
            out.mid.push(op_put(path.to_vec(), Some(strip(path, b))));
        }
        return;
    };
    let mut keys: Vec<&String> = ao.keys().collect();
    for k in bo.keys() {
        if !ao.contains_key(k) {
            keys.push(k);
        }
    }
    for k in keys {
        let mut p = path.to_vec();
        p.push(k.clone());
        let (x, y) = (ao.get(k), bo.get(k));
        if is_personal(&p) || same(x, y) {
            continue;
        }
        if list_id(&p).is_some() {
            let empty = J::Array(vec![]);
            diff_into(Some(x.unwrap_or(&empty)), Some(y.unwrap_or(&empty)), &p, out, 0);
        } else {
            out.mid.push(match y {
                None => op_put(p, None),
                Some(y) => {
                    let v = strip(&p, y);
                    op_put(p, Some(v))
                }
            });
        }
    }
}

/// live.js `CORE.diff(a, b)`: the ops that turn state a into state b.
pub fn diff(a: &J, b: &J) -> Vec<J> {
    let mut out = Out { mid: vec![], post: vec![] };
    diff_into(Some(a), Some(b), &[], &mut out, 0);
    out.mid.extend(out.post);
    out.mid
}

fn seg(path: &J) -> Vec<String> {
    path.as_array().map(|a| a.iter().map(|s| s.as_str().map_or_else(|| stringify(s), str::to_string)).collect()).unwrap_or_default()
}

/// The value at `path` (items found by '#'+id).
fn get_mut<'a>(root: &'a mut J, path: &[String]) -> Option<&'a mut J> {
    let mut cur = root;
    for (i, s) in path.iter().enumerate() {
        cur = if let Some(id) = s.strip_prefix('#') {
            let f = list_id(path.get(..i)?)?;
            cur.as_array_mut()?.iter_mut().find(|x| id_of(x, f).is_some_and(|v| id_text(v) == id))?
        } else {
            cur.as_object_mut()?.get_mut(s)?
        };
    }
    Some(cur)
}
fn get<'a>(root: &'a J, path: &[String]) -> Option<&'a J> {
    let mut cur = root;
    for (i, s) in path.iter().enumerate() {
        cur = if let Some(id) = s.strip_prefix('#') {
            let f = list_id(path.get(..i)?)?;
            cur.as_array()?.iter().find(|x| id_of(x, f).is_some_and(|v| id_text(v) == id))?
        } else {
            cur.as_object()?.get(s)?
        };
    }
    Some(cur)
}

fn apply_one(root: &mut J, op: &J) {
    let path = seg(op.get("path").unwrap_or(&J::Null));
    let o = op.get("o").and_then(J::as_str).unwrap_or("");
    let f = list_id(&path);
    match o {
        "add" => {
            let Some(f) = f else { return };
            let Some(id) = op.get("id") else { return };
            if get(root, &path).is_none() {
                // the list is missing: make it where its container is
                let Some((last, up)) = path.split_last() else { return };
                if last.starts_with('#') {
                    return;
                }
                let Some(J::Object(box_)) = get_mut(root, up) else { return };
                box_.insert(last.clone(), J::Array(vec![]));
            }
            let Some(J::Array(l)) = get_mut(root, &path) else { return };
            let v = op.get("v").cloned().unwrap_or(J::Null);
            match l.iter().position(|x| id_of(x, f).is_some_and(|k| stringify(k) == stringify(id))) {
                Some(i) => {
                    if let Some(slot) = l.get_mut(i) {
                        *slot = v;
                    }
                }
                None => {
                    let at = op.get("at").and_then(J::as_u64).and_then(|a| usize::try_from(a).ok()).unwrap_or(l.len()).min(l.len());
                    l.insert(at, v);
                }
            }
        }
        "del" => {
            let (Some(f), Some(id)) = (f, op.get("id")) else { return };
            if let Some(J::Array(l)) = get_mut(root, &path) {
                l.retain(|x| !id_of(x, f).is_some_and(|k| stringify(k) == stringify(id)));
            }
        }
        "put" => {
            let Some((last, up)) = path.split_last() else { return };
            if last.starts_with('#') {
                return;
            }
            let Some(J::Object(box_)) = get_mut(root, up) else { return };
            if op.get("x").is_some_and(|x| !x.is_null() && x != &J::from(0) && x != &J::Bool(false)) {
                box_.remove(last);
            } else {
                box_.insert(last.clone(), op.get("v").cloned().unwrap_or(J::Null));
            }
        }
        "ord" => {
            let Some(f) = f else { return };
            let ids: Vec<String> = op.get("v").and_then(J::as_array).map(|a| a.iter().map(stringify).collect()).unwrap_or_default();
            if let Some(J::Array(l)) = get_mut(root, &path) {
                let mut rest: Vec<J> = std::mem::take(l);
                let mut next: Vec<J> = Vec::with_capacity(rest.len());
                for id in &ids {
                    if let Some(i) = rest.iter().position(|x| id_of(x, f).is_some_and(|k| &stringify(k) == id)) {
                        next.push(rest.remove(i));
                    }
                }
                next.extend(rest);
                *l = next;
            }
        }
        _ => {}
    }
}

/// live.js `CORE.apply(root, ops)`.
pub fn apply(root: &mut J, ops: &[J]) {
    for op in ops.iter().take(1_000_000) {
        apply_one(root, op);
    }
}

fn ids_of(root: &J, path: &[String]) -> Vec<J> {
    let Some(f) = list_id(path) else { return vec![] };
    get(root, path).and_then(J::as_array).map(|l| l.iter().map(|x| id_of(x, f).cloned().unwrap_or(J::Null)).collect()).unwrap_or_default()
}

/// live.js `CORE.invert(root, ops)`: the inverse of ops on `root` as it is now.
pub fn invert(root: &J, ops: &[J]) -> Vec<J> {
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
                    lists.push((path.clone(), ids_of(root, &path)));
                }
                let l = lists.iter_mut().find(|(p, _)| *p == path).map(|(_, l)| l);
                let Some(l) = l else { continue };
                let i = l.iter().position(|x| stringify(x) == stringify(&id));
                let mut ip = path.clone();
                ip.push(format!("#{}", id_text(&id)));
                let x = i.and_then(|_| get(root, &ip));
                let inv = match (x, i) {
                    (Some(x), Some(i)) => serde_json::json!({"o": "add", "path": pj(&path), "id": id, "v": strip(&ip, x), "at": i}),
                    _ => serde_json::json!({"o": "del", "path": pj(&path), "id": id}),
                };
                if o == "add" {
                    if i.is_none() {
                        let at = op.get("at").and_then(J::as_u64).and_then(|a| usize::try_from(a).ok()).unwrap_or(l.len()).min(l.len());
                        l.insert(at, id);
                    }
                } else if let Some(i) = i {
                    l.remove(i);
                }
                Some(inv)
            }
            "put" => Some(match get(root, &path) {
                None => op_put(path, None),
                Some(v) => op_put(path, Some(v.clone())),
            }),
            "ord" => {
                if !lists.iter().any(|(p, _)| *p == path) {
                    lists.push((path.clone(), ids_of(root, &path)));
                }
                let Some(l) = lists.iter_mut().find(|(p, _)| *p == path).map(|(_, l)| l) else { continue };
                let inv = serde_json::json!({"o": "ord", "path": pj(&path), "v": l.clone()});
                let want: Vec<String> = op.get("v").and_then(J::as_array).map(|a| a.iter().map(stringify).collect()).unwrap_or_default();
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
        Some(last) => {
            (format!("e:{}", path.get(..=last).unwrap_or(&[]).join("/")), Some(path.get(last.saturating_add(1)..).unwrap_or(&[]).join("/")))
        }
    }
}

/// Item keys a value names (live.js `refs`: a connection, bolt group or stair reads the members it is made on).
fn refs(op: &J, path: &[String], out: &mut Vec<String>) {
    if path.first().map(String::as_str) == Some("members") {
        return;
    }
    fn walk(v: &J, out: &mut Vec<String>) {
        match v {
            J::String(s) if s.len() > 1 && s.starts_with('m') && s.get(1..).is_some_and(|d| d.bytes().all(|c| c.is_ascii_digit())) => {
                let k = format!("e:members/#{s}");
                if !out.contains(&k) {
                    out.push(k);
                }
            }
            J::Array(a) => a.iter().for_each(|x| walk(x, out)),
            J::Object(o) => o.values().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    if let Some(v) = op.get("v") {
        walk(v, out);
    }
}

/// live.js `CORE.touched(ops)`.
pub fn touched(ops: &[J]) -> Vec<TouchReq> {
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
            "put" => {
                let (k, f) = key_of(&path);
                add(&mut out, k, 'p', Some(f.filter(|f| !f.is_empty()).unwrap_or_else(|| "v".into())));
            }
            _ => {}
        }
        // reads: the items the op sits inside, and the members its value names
        let mut rs: Vec<String> = Vec::new();
        let upto = if o == "add" || o == "del" { path.len() } else { path.len().saturating_sub(1) };
        for i in 0..path.len() {
            if path.get(i).is_some_and(|s| s.starts_with('#')) && i < upto {
                rs.push(format!("e:{}", path.get(..=i).unwrap_or(&[]).join("/")));
            }
        }
        refs(op, &path, &mut rs);
        for k in rs {
            if !out.iter().any(|x| x.k == k && x.m != 'r') {
                add(&mut out, k, 'r', None);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------------
// The adapter on the window's model

fn num_id(s: &str) -> f64 {
    let d: String = s.chars().skip_while(|c| !c.is_ascii_digit()).collect();
    d.parse::<f64>().unwrap_or(f64::NAN)
}

/// live.js `claimIds()`: new ids from this client's own range (c × 1e6 + n).
pub fn claim_ids(state: &mut J, c: u64) {
    #[allow(clippy::cast_precision_loss)]
    let lo = c as f64 * ID_BLOCK;
    let hi = lo + ID_BLOCK;
    let mut next = lo + 1.0;
    let mut see = |id: &J| {
        let n = num_id(&id_text(id));
        if n > lo && n < hi && n >= next {
            next = n + 1.0;
        }
    };
    for k in ["members", "connections", "footings", "stairs", "bolts"] {
        for x in state.get(k).and_then(J::as_array).map(Vec::as_slice).unwrap_or(&[]) {
            see(x.get("id").unwrap_or(&J::Null));
            for h in x.get("holes").and_then(J::as_array).map(Vec::as_slice).unwrap_or(&[]) {
                see(h.get("id").unwrap_or(&J::Null));
            }
            for h in x.get("cuts").and_then(J::as_array).map(Vec::as_slice).unwrap_or(&[]) {
                see(h.get("id").unwrap_or(&J::Null));
            }
        }
    }
    let cur = state.get("seq").and_then(J::as_f64).unwrap_or(0.0);
    let seq = next.max(if cur > lo && cur < hi { cur } else { 0.0 });
    if let Some(o) = state.as_object_mut()
        && let Some(n) = serde_json::Number::from_f64(seq)
    {
        o.insert("seq".into(), J::Number(n));
    }
}

/// The adapter over the window's model (live.js `createLive`).
pub struct DcAdapter<'a> {
    pub model: &'a mut cc_detail::model::Model,
    pub c: u64,
}

impl Adapter for DcAdapter<'_> {
    fn apply(&mut self, ops: &Mp, _origin: &str) -> Result<(), String> {
        let ops: Vec<J> = ops.as_arr().iter().map(to_json).collect();
        apply(&mut self.model.state, &ops);
        self.model.version = self.model.version.saturating_add(1);
        Ok(())
    }
    fn invert(&mut self, ops: &Mp) -> Result<Mp, String> {
        let ops: Vec<J> = ops.as_arr().iter().map(to_json).collect();
        Ok(Mp::Arr(invert(&self.model.state, &ops).iter().map(to_mp).collect()))
    }
    fn touched(&self, ops: &Mp) -> Vec<TouchReq> {
        let ops: Vec<J> = ops.as_arr().iter().map(to_json).collect();
        touched(&ops)
    }
    fn snapshot(&mut self) -> Vec<u8> {
        let v = Mp::map(vec![("v", Mp::from(SCHEMA)), ("state", Mp::Str(stringify(&self.model.state)))]);
        cc_live::msgpack::pack(&v).unwrap_or_default()
    }
    fn load(&mut self, bytes: &[u8], _q: u64) -> Result<(), String> {
        let x = cc_live::msgpack::unpack(bytes).map_err(|e| e.0.to_string())?;
        if x.get("v").and_then(Mp::as_u64) != Some(SCHEMA) {
            return Err("live snapshot: unknown format".into());
        }
        let text = x.get("state").and_then(Mp::as_str).ok_or("live snapshot: unknown format")?;
        let mut st = parse(text)?;
        if !st.is_object() {
            return Err("live snapshot: unknown format".into());
        }
        let seq = self.model.state.get("seq").cloned();
        if let (Some(o), Some(seq)) = (st.as_object_mut(), seq) {
            o.insert("seq".into(), seq);
        }
        claim_ids(&mut st, self.c);
        self.model.state = st;
        self.model.undo_stack.clear();
        self.model.redo_stack.clear();
        self.model.version = self.model.version.saturating_add(1);
        Ok(())
    }
    fn validate(&mut self, ops: &Mp) -> Option<String> {
        let mut s = self.model.state.clone();
        let ops: Vec<J> = ops.as_arr().iter().map(to_json).collect();
        apply(&mut s, &ops);
        let ids: Vec<String> = s
            .get("members")
            .and_then(J::as_array)
            .map(|a| a.iter().map(|m| id_text(m.get("id").unwrap_or(&J::Null))).collect())
            .unwrap_or_default();
        fn bad(v: &J, ids: &[String]) -> bool {
            match v {
                J::String(x) if x.len() > 1 && x.starts_with('m') && x.get(1..).is_some_and(|d| d.bytes().all(|c| c.is_ascii_digit())) => {
                    !ids.contains(x)
                }
                J::Array(a) => a.iter().any(|y| bad(y, ids)),
                J::Object(o) => o.iter().any(|(k, y)| k != "id" && bad(y, ids)),
                _ => false,
            }
        }
        for k in ["connections", "bolts", "stairs", "footings"] {
            for x in s.get(k).and_then(J::as_array).map(Vec::as_slice).unwrap_or(&[]) {
                if bad(x, &ids) {
                    return Some("invalid".into());
                }
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The window's session

/// How this window talks to the session.
#[cfg(not(target_arch = "wasm32"))]
pub enum Link {
    /// the host's own window, in-process with the server it started
    Local(cc_live::net::LocalClient),
    /// a joiner's connection; envelopes arrive through a reader thread
    Remote { tx: std::sync::Arc<std::sync::Mutex<cc_live::net::Joiner>>, rx: Receiver<Result<Envelope, String>> },
}

/// A live session in this window.
#[cfg(not(target_arch = "wasm32"))]
pub struct LiveSession {
    pub doc: LiveDoc,
    pub link: Link,
    pub server: Option<cc_live::net::LiveHandle>,
    pub addr: String,
    pub code: String,
    /// joiners waiting for the host: (c, name)
    pub knocks: Vec<(u64, String)>,
    g: u64,
    closed: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for LiveSession {
    fn drop(&mut self) {
        if let Some(h) = &self.server {
            h.stop();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl LiveSession {
    fn send(&mut self, env: &Envelope) {
        match &self.link {
            Link::Local(l) => l.send(env),
            Link::Remote { tx, .. } => {
                let r = tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).send(env);
                if r.is_err() {
                    self.closed = true;
                }
            }
        }
    }
    fn flush_out(&mut self) {
        let out = std::mem::take(&mut self.doc.outbox);
        for e in &out {
            self.send(e);
        }
    }
}

/// A word of 0.5 Studio's Live window (`window.live.*` in Studio's catalog, the shell's `lang/`).
#[cfg(not(target_arch = "wasm32"))]
fn sw(key: &str, vars: &[(&str, String)]) -> String {
    crate::lang::t(&crate::lang::STUDIO, key, vars)
}

/// The people in the session, this window counted once.
#[cfg(not(target_arch = "wasm32"))]
fn people_in(s: &LiveSession) -> usize {
    let mut n = s.doc.peers.values().filter(|p| p.state != "gone" && p.state != "removed").count();
    if !s.doc.peers.contains_key(&s.doc.c) {
        n = n.saturating_add(1);
    }
    n
}

/// The session's line, in 0.5 Studio's words: "Live, 3 people" (or "Viewing live"), and for the host the code and the
/// address ("The code: K7-Q4M2X · Address 192.168.1.20:4545").
#[cfg(not(target_arch = "wasm32"))]
pub fn hosting_line(s: &LiveSession) -> String {
    let n = people_in(s).to_string();
    let key = if s.doc.role == "viewer" { "window.live.status.viewing" } else { "window.live.status.live" };
    let status = sw(key, &[("n", n)]);
    if s.server.is_some() {
        let code = if s.code.is_empty() { "…".to_string() } else { s.code.clone() };
        format!(
            "{status} · {} · {}",
            sw("window.live.code.label", &[("code", code)]),
            sw("window.live.code.address", &[("address", s.addr.clone())])
        )
    } else {
        status
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl DetailView {
    /// Undo / redo can act: the session's own steps while live, else the model's.
    pub fn can_undo_now(&self) -> bool {
        self.live.as_ref().map_or_else(|| self.model.can_undo(), |s| s.doc.can_undo())
    }
    pub fn can_redo_now(&self) -> bool {
        self.live.as_ref().map_or_else(|| self.model.can_redo(), |s| s.doc.can_redo())
    }

    /// Joined or hosting: this window's undo is the session's (per person), the model's own stacks start empty.
    pub fn live_bind(&mut self) {
        self.model.undo_stack.clear();
        self.model.redo_stack.clear();
    }

    /// Carry what arrived from the session into the model and send what the session waits for (every frame).
    pub fn live_poll(&mut self) {
        let Some(s) = self.live.as_mut() else { return };
        let mut got: Vec<Envelope> = Vec::new();
        let mut lost = false;
        match &s.link {
            Link::Local(l) => {
                while let Ok(w) = l.rx.try_recv() {
                    match w {
                        cc_live::net::Wire::Bytes(b) => {
                            if let Ok(e) = Envelope::decode(&b) {
                                got.push(e);
                            }
                        }
                        cc_live::net::Wire::Close(_) => lost = true,
                    }
                }
            }
            Link::Remote { rx, .. } => {
                while let Ok(r) = rx.try_recv() {
                    match r {
                        Ok(e) => got.push(e),
                        Err(_) => lost = true,
                    }
                }
            }
        }
        let v0 = self.model.version;
        let mut a = DcAdapter { model: &mut self.model, c: s.doc.c };
        for e in &got {
            s.doc.message(&mut a, e);
        }
        s.doc.flush(&mut a);
        if lost {
            s.closed = true;
            s.doc.dropped();
        }
        let events = std::mem::take(&mut s.doc.events);
        let mut say: Option<String> = None;
        for ev in events {
            match ev {
                DocEvent::Code { code } => s.code = code,
                DocEvent::Join { c, name } => s.knocks.push((c, name)),
                DocEvent::Welcome { c, .. } => {
                    let mut st = std::mem::take(&mut a.model.state);
                    claim_ids(&mut st, c);
                    a.model.state = st;
                    a.c = c;
                    say = Some(cc_detail::i18n::t0("live.status.live"));
                }
                DocEvent::Status(st) if st == "waiting" => say = Some(cc_detail::i18n::t0("live.status.waiting")),
                DocEvent::Status(st) if st == "reconnecting" => say = Some(cc_detail::i18n::t0("live.status.reconnecting")),
                DocEvent::Refused { label, reason, by_name, .. } => {
                    let who = if by_name.is_empty() { cc_detail::i18n::t0("live.someone") } else { by_name };
                    let key = match reason.as_str() {
                        "locked" => "live.refused.locked",
                        "role" => "live.refused.role",
                        _ => "live.refused",
                    };
                    say = Some(cc_detail::i18n::t(key, &[("what", cc_detail::i18n::F::S(label)), ("name", cc_detail::i18n::F::S(who))]));
                }
                DocEvent::UndoSkipped { label, by_name } => {
                    let who = if by_name.is_empty() { cc_detail::i18n::t0("live.someone") } else { by_name };
                    say = Some(cc_detail::i18n::t(
                        "live.undo_skipped",
                        &[("what", cc_detail::i18n::F::S(label)), ("name", cc_detail::i18n::F::S(who))],
                    ));
                }
                DocEvent::Ended { .. } => say = Some(cc_detail::i18n::t0("live.ended")),
                _ => {}
            }
        }
        s.flush_out();
        if let Some(m) = say {
            self.message = m;
        }
        if self.model.version != v0 {
            self.recompute();
        }
        if self.live.as_ref().is_some_and(|s| s.doc.final_()) {
            self.live = None;
        }
    }

    /// A step this window made (`model.change` grew the version): its ops go out (live.js's wrapped `change`).
    pub fn live_local(&mut self, label: &str, version_before: u64) {
        if self.model.version == version_before {
            return;
        }
        let Some(s) = self.live.as_mut() else { return };
        let Some(before) = self.model.undo_stack.last().and_then(|t| parse(&t.json).ok()) else { return };
        let ops = diff(&before, &self.model.state);
        if ops.is_empty() {
            return;
        }
        let inverse = diff(&self.model.state, &before);
        s.g = s.g.saturating_add(1);
        let mark = serde_json::json!({"o": "g", "g": format!("s{}", s.g)});
        let ops = Mp::Arr(std::iter::once(&mark).chain(ops.iter()).map(to_mp).collect());
        let inverse = Mp::Arr(std::iter::once(&mark).chain(inverse.iter()).map(to_mp).collect());
        let mut a = DcAdapter { model: &mut self.model, c: s.doc.c };
        s.doc.local_change(&mut a, label, ops, inverse);
        s.flush_out();
    }

    /// Undo / redo in a live session: the person's own steps (live.js's `m.undo` = `client.undo()`). → the label.
    pub fn live_reverse(&mut self, undo: bool) -> Option<Option<String>> {
        let s = self.live.as_mut()?;
        let label = if undo { s.doc.undo_label() } else { s.doc.redo_label() };
        let mut a = DcAdapter { model: &mut self.model, c: s.doc.c };
        let ok = if undo { s.doc.undo(&mut a) } else { s.doc.redo(&mut a) };
        s.flush_out();
        self.recompute();
        Some(if ok { label } else { None })
    }
}

/// `live.*` commands.
#[cfg(not(target_arch = "wasm32"))]
pub fn run(dv: &mut DetailView, id: &str, name: &str) -> Result<(), String> {
    match id {
        "detail.live.host" => {
            if dv.live.is_some() {
                return Err("live.host: this window is in a live session already".into());
            }
            let bind = if name.trim().is_empty() { "0.0.0.0:0" } else { name.trim() };
            let bind: std::net::SocketAddr = bind.parse().map_err(|_| format!("live.host: an address host:port, not {bind}"))?;
            let cfg = cc_live::server::Config { app: "detailcraft".into(), schema: 1, ..Default::default() };
            let h = cc_live::net::start(cfg, bind).map_err(|e| format!("live.host: {e}"))?;
            let local = h.attach_local();
            let addr = h.addr.to_string();
            let mut s = LiveSession {
                doc: LiveDoc::new(),
                link: Link::Local(local),
                server: Some(h),
                addr,
                code: String::new(),
                knocks: vec![],
                g: 0,
                closed: false,
            };
            let hello = s.doc.hello("detailcraft", SCHEMA, "Host", Some(("host", "")));
            s.send(&hello);
            s.send(&Envelope::new("invite"));
            dv.live = Some(s);
            dv.live_bind();
            pump(dv, 2000, |s| !s.code.is_empty() && s.doc.is_live());
            if let Some(s) = dv.live.as_ref() {
                dv.message = hosting_line(s);
            }
        }
        "detail.live.join" => {
            if dv.live.is_some() {
                return Err("live.join: this window is in a live session already".into());
            }
            let (addr, code) = name.trim().split_once(' ').ok_or("live.join: <host:port> <code>")?;
            let secret =
                if code.trim() == "plain" { cc_live::net::Secret::Plain } else { cc_live::net::Secret::Code(code.trim().to_string()) };
            let mut j = cc_live::net::Joiner::connect(addr.trim(), &secret)?;
            let doc = LiveDoc::new();
            j.send(&doc.hello("detailcraft", SCHEMA, &cc_detail::i18n::t0("live.guest"), None))?;
            let tx = std::sync::Arc::new(std::sync::Mutex::new(j));
            let (etx, rx) = channel();
            let reader = tx.clone();
            std::thread::Builder::new()
                .name("cc-live-join".into())
                .spawn(move || {
                    loop {
                        let r = reader.lock().unwrap_or_else(std::sync::PoisonError::into_inner).recv(Duration::from_millis(20));
                        match r {
                            Ok(Some(e)) => {
                                if etx.send(Ok(e)).is_err() {
                                    break;
                                }
                            }
                            Ok(None) => std::thread::sleep(Duration::from_millis(2)),
                            Err(e) => {
                                let _ = etx.send(Err(e));
                                break;
                            }
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
            dv.live = Some(LiveSession {
                doc,
                link: Link::Remote { tx, rx },
                server: None,
                addr: addr.trim().to_string(),
                code: String::new(),
                knocks: vec![],
                g: 0,
                closed: false,
            });
            dv.live_bind();
            pump(dv, 10_000, |s| s.doc.is_live() || s.doc.final_() || s.doc.status == "waiting");
        }
        "detail.live.approve" | "detail.live.deny" => {
            let s = dv.live.as_mut().ok_or("live: not in a live session")?;
            let mut it = name.split_whitespace();
            let c: u64 = it.next().and_then(|x| x.parse().ok()).ok_or("live.approve: <client> [editor|viewer]")?;
            let b = if id == "detail.live.approve" {
                cc_live::msgpack::Value::map(vec![("c", Mp::from(c)), ("role", Mp::from(it.next().unwrap_or("editor")))])
            } else {
                cc_live::msgpack::Value::map(vec![("c", Mp::from(c))])
            };
            s.send(&Envelope::new(if id == "detail.live.approve" { "approve" } else { "deny" }).body(b));
            s.knocks.retain(|k| k.0 != c);
        }
        "detail.live.window" => dv.live_window = name.trim() != "close",
        "detail.live.invite" => {
            let s = dv.live.as_mut().ok_or("live: not in a live session")?;
            s.send(&Envelope::new("invite"));
        }
        "detail.live.leave" => {
            if let Some(mut s) = dv.live.take() {
                s.send(&Envelope::new(if s.server.is_some() { "end" } else { "bye" }));
                if let Link::Remote { tx, .. } = &s.link {
                    tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).close();
                }
            }
            dv.message = cc_detail::i18n::t0("live.ended");
        }
        "detail.live.pump" => {
            // headless: `<ms> [wait] [approve]` — until what this window sent is decided (or `wait`: the whole time);
            // `approve` lets every joiner in as an editor meanwhile (test runs; the window's host approves by hand)
            let mut it = name.split_whitespace();
            let ms: u64 = it.next().and_then(|x| x.parse().ok()).unwrap_or(2000).min(120_000);
            let flags: Vec<&str> = it.collect();
            let (wait, approve) = (flags.contains(&"wait"), flags.contains(&"approve"));
            let t0 = web_time::Instant::now();
            loop {
                dv.live_poll();
                let Some(s) = dv.live.as_mut() else { break };
                if approve {
                    for (c, _) in std::mem::take(&mut s.knocks) {
                        s.send(&Envelope::new("approve").body(Mp::map(vec![("c", Mp::from(c)), ("role", Mp::from("editor"))])));
                    }
                }
                let done = !wait && s.doc.settled() && s.doc.outbox.is_empty();
                if done || t0.elapsed() > Duration::from_millis(ms) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(3));
            }
            if let Some(s) = dv.live.as_ref() {
                dv.message = hosting_line(s);
            }
        }
        o => return Err(format!("unknown command {o}")),
    }
    Ok(())
}

/// Carry messages until `done` (or `ms` passed): headless runs and the first moments of a session.
#[cfg(not(target_arch = "wasm32"))]
pub fn pump(dv: &mut DetailView, ms: u64, done: impl Fn(&LiveSession) -> bool) {
    let t0 = web_time::Instant::now();
    loop {
        dv.live_poll();
        let Some(s) = dv.live.as_ref() else { return };
        if done(s) || t0.elapsed() > Duration::from_millis(ms) {
            return;
        }
        std::thread::sleep(Duration::from_millis(3));
    }
}

/// The browser edition joins through its own WebSocket (not built yet): no session in this window there.
#[cfg(target_arch = "wasm32")]
pub struct LiveSession;

#[cfg(target_arch = "wasm32")]
impl DetailView {
    pub fn can_undo_now(&self) -> bool {
        self.model.can_undo()
    }
    pub fn can_redo_now(&self) -> bool {
        self.model.can_redo()
    }
    pub fn live_poll(&mut self) {}
    pub fn live_local(&mut self, _label: &str, _version_before: u64) {}
    pub fn live_reverse(&mut self, _undo: bool) -> Option<Option<String>> {
        None
    }
}

#[cfg(target_arch = "wasm32")]
pub fn run(_dv: &mut DetailView, id: &str, _name: &str) -> Result<(), String> {
    Err(format!("{id}: Studio Live is not in the browser edition of this version yet"))
}

/// The Live window (Studio's hosting and joining dialogs, in plain English: their words are Studio's, not in the 0.5
/// app catalogs): host on the LAN and show the code, let joiners in, or join with an address and a code; leave.
#[cfg(not(target_arch = "wasm32"))]
pub fn window(dv: &mut DetailView, ctx: &egui::Context, tk: &crate::theme::Tokens, acts: &mut Vec<crate::detailpanel::Act>) {
    use egui::RichText;
    let mut open = dv.live_window;
    if !open {
        return;
    }
    let run = |acts: &mut Vec<crate::detailpanel::Act>, id: &'static str, name: String| acts.push(crate::detailpanel::Act::Run(id, name));
    crate::shell::Dialog::new(RichText::new(sw("window.live.title", &[])).strong())
        .modeless()
        .default_width(380.0)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .open(&mut open)
        .show(ctx, |ui| match dv.live.as_ref() {
            Some(s) => {
                ui.label(hosting_line(s));
                if s.server.is_some() {
                    if !s.knocks.is_empty() {
                        ui.add_space(4.0);
                        ui.label(RichText::new(sw("window.live.waiting.title", &[])).strong());
                    }
                    for (c, name) in &s.knocks {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(sw("window.live.knock", &[("name", name.clone())]));
                            if ui.add(crate::shell::Btn::primary(sw("window.live.approve.edit", &[]))).clicked() {
                                run(acts, "detail.live.approve", format!("{c} editor"));
                            }
                            if ui.add(crate::shell::Btn::new(sw("window.live.approve.view", &[]))).clicked() {
                                run(acts, "detail.live.approve", format!("{c} viewer"));
                            }
                            if ui.add(crate::shell::Btn::new(sw("window.live.approve.deny", &[]))).clicked() {
                                run(acts, "detail.live.deny", c.to_string());
                            }
                        });
                    }
                }
                ui.add_space(4.0);
                let n = people_in(s).to_string();
                ui.label(RichText::new(sw("window.live.people.title", &[("n", n)])).strong());
                for p in s.doc.peers.values().filter(|p| p.state != "gone" && p.state != "removed") {
                    let role = match p.role.as_str() {
                        "host" => sw("window.live.role.host", &[]),
                        "viewer" => sw("window.live.role.viewer", &[]),
                        "assistant" => sw("window.live.role.assistant", &[]),
                        _ => sw("window.live.role.editor", &[]),
                    };
                    let name = if p.name.is_empty() { sw("window.live.noname", &[]) } else { p.name.clone() };
                    let you = if p.c == s.doc.c { format!(" {}", sw("window.live.people.you", &[])) } else { String::new() };
                    ui.label(RichText::new(format!("{name}{you} · {role}")).color(tk.text_dim));
                }
                ui.add_space(4.0);
                let (end, hint) = if s.server.is_some() {
                    ("window.live.end", "window.live.end.hint")
                } else {
                    ("window.live.leave", "window.live.leave.hint")
                };
                if ui.add(crate::shell::Btn::new(sw(end, &[]))).on_hover_text(sw(hint, &[])).clicked() {
                    run(acts, "detail.live.leave", String::new());
                }
            }
            None => {
                ui.label(RichText::new(sw("window.live.link.noshell", &[])).color(tk.text_dim));
                if ui.add(crate::shell::Btn::primary(sw("window.live.share", &[]))).clicked() {
                    run(acts, "detail.live.host", String::new());
                }
                ui.separator();
                // 0.5's "Join with the drawing open here": one field, the host's address and the code after it
                ui.label(RichText::new(sw("window.live.here.title", &[])).strong());
                let fid = egui::Id::unique("dc_live_join");
                let mut f: (String, String) = ctx.data(|d| d.get_temp(fid)).unwrap_or_default();
                let l = ui.label(sw("window.live.here.label", &[]));
                ui.add(crate::shell::field(&mut f.0).desired_width(260.0).hint_text("192.168.1.20:4545 K7-Q4M2X")).labelled_by(l.id);
                if !f.1.is_empty() {
                    ui.label(RichText::new(&f.1).color(tk.accent));
                }
                if ui.add_enabled(!f.0.trim().is_empty(), crate::shell::Btn::new(sw("window.live.here.join", &[]))).clicked() {
                    match join_words(&f.0) {
                        Ok(spec) => {
                            f.1.clear();
                            run(acts, "detail.live.join", spec);
                        }
                        Err(key) => f.1 = sw(key, &[]),
                    }
                }
                ctx.data_mut(|d| d.insert_temp(fid, f));
            }
        });
    dv.live_window = open;
}

/// The join field (an address with a code after it, as 0.5's "Join here") → `detail.live.join`'s `<host:port> <code>`,
/// or the Studio key saying what is missing. A code alone needs the network search (mDNS), which the fork has not.
#[cfg(not(target_arch = "wasm32"))]
pub fn join_words(text: &str) -> Result<String, &'static str> {
    let mut it = text.split_whitespace();
    let (Some(a), b, None) = (it.next(), it.next(), it.next()) else { return Err("window.live.here.bad") };
    let is_addr = |x: &str| x.rsplit_once(':').is_some_and(|(h, p)| !h.is_empty() && p.parse::<u16>().is_ok());
    match b {
        Some(code) if is_addr(a) => Ok(format!("{a} {code}")),
        None if is_addr(a) => Err("window.live.here.needcode"),
        None => Err("window.live.here.needcode"),
        Some(_) => Err("window.live.here.bad"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_join_field_reads_an_address_and_a_code() {
        use super::join_words;
        assert_eq!(join_words(" 192.168.1.20:4545  K7-Q4M2X "), Ok("192.168.1.20:4545 K7-Q4M2X".into()));
        assert_eq!(join_words("192.168.1.20:4545"), Err("window.live.here.needcode"));
        assert_eq!(join_words("K7-Q4M2X"), Err("window.live.here.needcode"));
        assert_eq!(join_words("a b c"), Err("window.live.here.bad"));
        assert_eq!(join_words("host:99999 K7"), Err("window.live.here.bad"));
        assert_eq!(join_words(""), Err("window.live.here.bad"));
        // every word the window uses is in Studio's catalog, en and es-MX
        for k in [
            "window.live.title",
            "window.live.share",
            "window.live.link.noshell",
            "window.live.code.label",
            "window.live.code.address",
            "window.live.knock",
            "window.live.approve.edit",
            "window.live.approve.view",
            "window.live.approve.deny",
            "window.live.waiting.title",
            "window.live.people.you",
            "window.live.role.host",
            "window.live.role.editor",
            "window.live.role.viewer",
            "window.live.role.assistant",
            "window.live.noname",
            "window.live.end",
            "window.live.end.hint",
            "window.live.leave",
            "window.live.leave.hint",
            "window.live.here.title",
            "window.live.here.label",
            "window.live.here.join",
            "window.live.here.needcode",
            "window.live.here.bad",
            "window.live.button",
            "window.live.button.title",
            "window.live.status.live",
            "window.live.status.viewing",
            "window.live.people.title",
        ] {
            for l in [cc_lang::Locale::En, cc_lang::Locale::EsMx] {
                assert!(crate::lang::STUDIO.get_in(l, k).is_some(), "{k} {l:?}");
            }
        }
    }

    use super::*;

    fn sample_state() -> J {
        cc_detail::sample::sample_model("Sample frame").expect("sample").state
    }

    #[test]
    fn diff_applies_and_inverts_back() {
        let a = sample_state();
        let mut m = cc_detail::model::Model::new(a.clone());
        m.change("edit", |tx| {
            let id = tx.s.get("members").and_then(J::as_array).and_then(|l| l.get(6)).and_then(|x| x.get("id")).cloned().unwrap_or(J::Null);
            tx.update_member(Some(&id), Some(&serde_json::json!({"profile": "W18x35"})))?;
            tx.set_name(Some(&J::from("Changed")));
            Ok(())
        })
        .expect("change");
        let b = m.state.clone();
        let ops = diff(&a, &b);
        assert!(!ops.is_empty());
        let mut x = a.clone();
        let inv = invert(&x, &ops);
        apply(&mut x, &ops);
        assert_eq!(stringify(&x), stringify(&b));
        apply(&mut x, &inv);
        assert_eq!(stringify(&x), stringify(&a));
        let t = touched(&ops);
        assert!(t.iter().any(|t| t.k == "meta:name" && t.m == 'p'), "{t:?}");
        assert!(t.iter().any(|t| t.k.starts_with("e:members/#") && t.f.as_deref() == Some("profile")), "{t:?}");
        // through MessagePack and back unchanged
        let back: Vec<J> = ops.iter().map(|o| to_json(&to_mp(o))).collect();
        assert_eq!(stringify(&J::Array(back)), stringify(&J::Array(ops)));
    }

    #[test]
    fn items_added_and_removed_are_creates_and_deletes_with_their_reads() {
        let a = sample_state();
        let mut b = a.clone();
        if let Some(l) = b.get_mut("connections").and_then(J::as_array_mut) {
            l.remove(0);
            l.push(serde_json::json!({"id": "j99", "type": "baseplate", "column": "m1"}));
        }
        let ops = diff(&a, &b);
        let t = touched(&ops);
        assert!(t.iter().any(|t| t.m == 'd' && t.k.starts_with("e:connections/#")));
        assert!(t.iter().any(|t| t.m == 'c' && t.k == "e:connections/#j99"));
        assert!(t.iter().any(|t| t.m == 'r' && t.k == "e:members/#m1"), "{t:?}");
        let mut x = a.clone();
        apply(&mut x, &ops);
        assert_eq!(stringify(&x), stringify(&b));
        // hostile ops are skipped, never a crash
        apply(
            &mut x,
            &[serde_json::json!({"o": "put", "path": [1, 2]}), serde_json::json!({"o": "add", "path": ["nope"], "id": 1}), J::Null],
        );
    }
}
