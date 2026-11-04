//! PartCraft in a Studio Live session (0.5 partcraft/dev/live.js, LV.20): the part is the feature tree; an edit's ops are
//! the difference between the part before and after it:
//!
//! - `{o:'add', id, v, at}` a feature created (at = its place in the tree) — key `e:f/<id>` (c)
//! - `{o:'del', id}` a feature removed (d); `{o:'set', id, f, v | x:1}` one field of a feature (a sketch travels whole:
//!   two people in one sketch, the later edit wins) (p)
//! - `{o:'ord', v:[id…]}` the tree's order — `meta:features.order` (s: a reorder is refused if the order changed since)
//! - `{o:'top', f, v | x:1}` the part's other fields (name, rollback bar, material, parameters …) — `meta:<f>` (p)
//!
//! Never meshes: every window rebuilds the solid from the tree with its own kernel. A feature names the features it builds
//! on (its sketch "f3", a face "f2.end", a body "@f4"): those are reads, and removing one still built on is refused.
//! Ids: client c's new features are f<c × 1e6 + n>; `seq` is personal.

use cc_live::doc::{Adapter, Cond, TouchReq};
use cc_live::msgpack::Value as Mp;
use serde_json::{Value as J, json};

use crate::livestate::{ops_json, same, to_json, to_mp};

pub const SCHEMA: u64 = 1;
const ID_BLOCK: f64 = 1e6;

fn feats(p: &J) -> &[J] {
    p.get("features").and_then(J::as_array).map_or(&[], Vec::as_slice)
}
fn fid(x: &J) -> J {
    x.get("id").cloned().unwrap_or(J::Null)
}
fn op_s<'a>(op: &'a J, k: &str) -> &'a str {
    op.get(k).and_then(J::as_str).unwrap_or("")
}
fn x_set(op: &J) -> bool {
    op.get("x").is_some_and(|x| !x.is_null() && x != &J::from(0) && x != &J::Bool(false))
}

/// Feature ids named anywhere in a value (live.js `idsIn`: an imported STEP's own text left out).
pub fn ids_in(v: &J) -> Vec<String> {
    let J::Object(o) = v else { return vec![] };
    let mut rest = o.clone();
    rest.shift_remove("step");
    let text = cc_detail::js::stringify(&J::Object(rest));
    let b = text.as_bytes();
    let mut out = vec![];
    let mut i = 0usize;
    while i < b.len() {
        let prev_ok = i == 0 || !b.get(i - 1).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_');
        if b.get(i) == Some(&b'f') && prev_ok {
            let mut j = i + 1;
            while b.get(j).is_some_and(u8::is_ascii_digit) {
                j += 1;
            }
            if j > i + 1 && !b.get(j).is_some_and(u8::is_ascii_digit) {
                out.push(text.get(i..j).unwrap_or("").to_string());
                i = j;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// live.js `diff(a, b)`.
pub fn diff(a: &J, b: &J) -> Vec<J> {
    let (mut out, mut tail) = (vec![], vec![]);
    let (fa, fb) = (feats(a), feats(b));
    let key = |x: &J| cc_detail::js::stringify(&fid(x));
    let ka: Vec<String> = fa.iter().map(key).collect();
    let kb: Vec<String> = fb.iter().map(key).collect();
    for (x, k) in fa.iter().zip(&ka) {
        if !kb.contains(k) {
            out.push(json!({"o": "del", "id": fid(x)}));
        }
    }
    for (at, (x, k)) in fb.iter().zip(&kb).enumerate() {
        if !ka.contains(k) {
            out.push(json!({"o": "add", "id": fid(x), "v": x, "at": at}));
        }
    }
    for (x, k) in fb.iter().zip(&kb) {
        let Some(was) = ka.iter().position(|y| y == k).and_then(|i| fa.get(i)) else { continue };
        let (Some(wo), Some(xo)) = (was.as_object(), x.as_object()) else { continue };
        let mut fs: Vec<&String> = wo.keys().collect();
        fs.extend(xo.keys().filter(|f| !wo.contains_key(*f)));
        for f in fs {
            if f != "id" && !same(wo.get(f), xo.get(f)) {
                out.push(match xo.get(f) {
                    None => json!({"o": "set", "id": fid(x), "f": f, "x": 1}),
                    Some(v) => json!({"o": "set", "id": fid(x), "f": f, "v": v}),
                });
            }
        }
    }
    let before: Vec<&String> = ka.iter().filter(|k| kb.contains(k)).collect();
    let after: Vec<&String> = kb.iter().filter(|k| ka.contains(k)).collect();
    if before != after {
        tail.push(json!({"o": "ord", "v": fb.iter().map(fid).collect::<Vec<_>>()}));
    }
    if let (Some(ao), Some(bo)) = (a.as_object(), b.as_object()) {
        let mut fs: Vec<&String> = ao.keys().collect();
        fs.extend(bo.keys().filter(|k| !ao.contains_key(*k)));
        for f in fs {
            if f == "features" || f == "seq" || same(ao.get(f), bo.get(f)) {
                continue;
            }
            out.push(match bo.get(f) {
                None => json!({"o": "top", "f": f, "x": 1}),
                Some(v) => json!({"o": "top", "f": f, "v": v}),
            });
        }
    }
    out.extend(tail);
    out
}

/// live.js `applyOp(op, p)`.
pub fn apply_one(p: &mut J, op: &J) {
    let Some(po) = p.as_object_mut() else { return };
    if op_s(op, "o") == "top" {
        let f = op_s(op, "f");
        if !f.is_empty() && f != "features" {
            if x_set(op) {
                po.shift_remove(f);
            } else {
                po.insert(f.to_string(), op.get("v").cloned().unwrap_or(J::Null));
            }
        }
        return;
    }
    let l = po.entry("features").or_insert_with(|| J::Array(vec![]));
    let Some(list) = l.as_array_mut() else { return };
    let id = op.get("id");
    match op_s(op, "o") {
        "add" => {
            let v = op.get("v").cloned().unwrap_or(J::Null);
            match list.iter().position(|x| same(x.get("id"), id)) {
                Some(i) => {
                    if let Some(s) = list.get_mut(i) {
                        *s = v;
                    }
                }
                None => {
                    let at = op.get("at").and_then(J::as_u64).and_then(|a| usize::try_from(a).ok()).unwrap_or(list.len()).min(list.len());
                    list.insert(at, v);
                }
            }
        }
        "del" => {
            if let Some(i) = list.iter().position(|x| same(x.get("id"), id)) {
                list.remove(i);
            }
        }
        "set" => {
            if let Some(J::Object(x)) = list.iter_mut().find(|y| same(y.get("id"), id)) {
                let f = op_s(op, "f");
                if x_set(op) {
                    x.shift_remove(f);
                } else {
                    x.insert(f.to_string(), op.get("v").cloned().unwrap_or(J::Null));
                }
            }
        }
        "ord" => {
            let want = op.get("v").and_then(J::as_array).cloned().unwrap_or_default();
            let mut rest: Vec<Option<J>> = std::mem::take(list).into_iter().map(Some).collect();
            let mut next = vec![];
            for w in &want {
                if let Some(i) = rest.iter().position(|x| x.as_ref().is_some_and(|x| same(x.get("id"), Some(w))))
                    && let Some(x) = rest.get_mut(i).and_then(Option::take)
                {
                    next.push(x);
                }
            }
            next.extend(rest.into_iter().flatten());
            *list = next;
        }
        _ => {}
    }
}

fn walk(op: &J, list: &mut Vec<J>) {
    let id = op.get("id").cloned().unwrap_or(J::Null);
    match op_s(op, "o") {
        "add" => {
            if !list.iter().any(|x| same(Some(x), Some(&id))) {
                let at = op.get("at").and_then(J::as_u64).and_then(|a| usize::try_from(a).ok()).unwrap_or(list.len()).min(list.len());
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

/// live.js `invert(ops)`.
pub fn invert(p: &J, ops: &[J]) -> Vec<J> {
    let mut order: Vec<J> = feats(p).iter().map(fid).collect();
    let mut inv = vec![];
    for op in ops {
        let find = || feats(p).iter().find(|y| same(y.get("id"), op.get("id")));
        let r = match op_s(op, "o") {
            "add" | "del" => {
                let i = order.iter().position(|x| same(Some(x), op.get("id")));
                match i.and_then(|i| find().map(|x| (i, x))) {
                    Some((i, x)) => Some(json!({"o": "add", "id": op.get("id"), "v": x, "at": i})),
                    None => Some(json!({"o": "del", "id": op.get("id")})),
                }
            }
            "set" => find().map(|x| {
                let f = op_s(op, "f");
                match x.get(f) {
                    None => json!({"o": "set", "id": op.get("id"), "f": f, "x": 1}),
                    Some(v) => json!({"o": "set", "id": op.get("id"), "f": f, "v": v}),
                }
            }),
            "ord" => Some(json!({"o": "ord", "v": order.clone()})),
            "top" => {
                let f = op_s(op, "f");
                Some(match p.get(f) {
                    None => json!({"o": "top", "f": f, "x": 1}),
                    Some(v) => json!({"o": "top", "f": f, "v": v}),
                })
            }
            "g" => Some(json!({"o": "g", "g": op.get("g")})),
            _ => None,
        };
        inv.push(r);
        walk(op, &mut order);
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

/// live.js `touched(ops)` (the features a feature builds on, from the part as it is).
pub fn touched(p: &J, ops: &[J]) -> Vec<TouchReq> {
    let ids: Vec<String> = feats(p).iter().map(|x| crate::livestate::id_text(&fid(x))).collect();
    let refs = |v: &J| -> Vec<String> {
        let mut out: Vec<String> = vec![];
        for id in ids_in(v) {
            if ids.contains(&id) && !out.contains(&id) {
                out.push(id);
            }
        }
        out
    };
    let mut out: Vec<TouchReq> = vec![];
    let mut seen = std::collections::HashSet::new();
    let mut add = |out: &mut Vec<TouchReq>, k: String, m: char, f: Option<String>| {
        if seen.insert(format!("{k}\u{0}{m}\u{0}{}", f.clone().unwrap_or_default())) {
            out.push(TouchReq { k, m, f, r: Cond::Auto, g: false });
        }
    };
    for op in ops {
        let id = crate::livestate::id_text(op.get("id").unwrap_or(&J::Null));
        match op_s(op, "o") {
            "add" => {
                add(&mut out, format!("e:f/{id}"), 'c', None);
                for r in refs(op.get("v").unwrap_or(&J::Null)) {
                    if r != id {
                        add(&mut out, format!("e:f/{r}"), 'r', None);
                    }
                }
            }
            "del" => add(&mut out, format!("e:f/{id}"), 'd', None),
            "set" => {
                let f = op_s(op, "f").to_string();
                add(&mut out, format!("e:f/{id}"), 'p', Some(f.clone()));
                if !x_set(op) {
                    let mut o = serde_json::Map::new();
                    o.insert(f, op.get("v").cloned().unwrap_or(J::Null));
                    for r in refs(&J::Object(o)) {
                        if r != id {
                            add(&mut out, format!("e:f/{r}"), 'r', None);
                        }
                    }
                }
            }
            "ord" => add(&mut out, "meta:features.order".into(), 's', None),
            "top" => add(&mut out, format!("meta:{}", op_s(op, "f")), 'p', Some("v".into())),
            _ => {}
        }
    }
    out
}

/// live.js `validate(ops)`: a feature removed while another still builds on it, or one added on a feature that is gone.
pub fn validate(p: &J, ops: &[J]) -> Option<String> {
    let now: Vec<String> = feats(p).iter().map(|x| crate::livestate::id_text(&fid(x))).collect();
    let mut after: Vec<(String, J)> = feats(p).iter().map(|x| (crate::livestate::id_text(&fid(x)), x.clone())).collect();
    for op in ops {
        let id = crate::livestate::id_text(op.get("id").unwrap_or(&J::Null));
        match op_s(op, "o") {
            "add" => {
                let v = op.get("v").cloned().unwrap_or(J::Null);
                match after.iter_mut().find(|(k, _)| *k == id) {
                    Some(slot) => slot.1 = v,
                    None => after.push((id, v)),
                }
            }
            "del" => after.retain(|(k, _)| *k != id),
            "set" => {
                if let Some((_, J::Object(x))) = after.iter_mut().find(|(k, _)| *k == id) {
                    let f = op_s(op, "f").to_string();
                    // { ...x, [f]: op.x ? undefined : op.v }: an undefined field drops out of the JSON
                    if x_set(op) {
                        x.shift_remove(&f);
                    } else {
                        x.insert(f, op.get("v").cloned().unwrap_or(J::Null));
                    }
                }
            }
            _ => {}
        }
    }
    let gone: Vec<&String> = now.iter().filter(|id| !after.iter().any(|(k, _)| k == *id)).collect();
    for (id, x) in &after {
        for r in ids_in(x) {
            if r != *id && gone.contains(&&r) {
                return Some("feature-in-use".into());
            }
        }
    }
    None
}

fn num_id(v: &J) -> f64 {
    let s = crate::livestate::id_text(v);
    let d: String = s.chars().skip_while(|c| !c.is_ascii_digit()).collect();
    if d.trim().is_empty() {
        return 0.0;
    }
    d.parse::<f64>().unwrap_or(f64::NAN)
}

/// live.js `claimIds()`.
pub fn claim_ids(p: &mut J, c: u64) {
    #[allow(clippy::cast_precision_loss)]
    let lo = c as f64 * ID_BLOCK;
    let hi = lo + ID_BLOCK;
    let mut next = lo + 1.0;
    for x in feats(p) {
        let n = num_id(&fid(x));
        if n >= next && n < hi {
            next = n + 1.0;
        }
    }
    let seq = p.get("seq").and_then(J::as_f64);
    let v = match seq {
        Some(q) if q > lo && q < hi => q.max(next),
        _ => next,
    };
    if let (Some(o), Some(n)) = (p.as_object_mut(), serde_json::Number::from_f64(v)) {
        o.insert("seq".into(), J::Number(n));
    }
}

/// live.js `releaseIds()`.
pub fn release_ids(p: &mut J) {
    let mut max: f64 = 0.0;
    for x in feats(p) {
        let n = num_id(&fid(x));
        if n.is_finite() && n > max {
            max = n;
        }
    }
    let cur = p.get("seq").and_then(J::as_f64).filter(|x| *x != 0.0 && x.is_finite()).unwrap_or(1.0);
    if let (Some(o), Some(n)) = (p.as_object_mut(), serde_json::Number::from_f64(cur.max(max + 1.0))) {
        o.insert("seq".into(), J::Number(n));
    }
}

/// The part without its personal fields (live.js `shared()` / `hash()`).
pub fn shared(p: &J) -> J {
    let mut s = p.clone();
    if let Some(o) = s.as_object_mut() {
        o.shift_remove("seq");
    }
    s
}

/// The adapter over a part (live.js `createLive`); `changed` tells the window to rebuild.
pub struct PartAdapter<'a> {
    pub part: &'a mut J,
    pub c: u64,
    pub changed: &'a mut bool,
}

impl Adapter for PartAdapter<'_> {
    fn apply(&mut self, ops: &Mp, _origin: &str) -> Result<(), String> {
        for op in ops_json(ops) {
            apply_one(self.part, &op);
        }
        *self.changed = true;
        Ok(())
    }
    fn invert(&mut self, ops: &Mp) -> Result<Mp, String> {
        Ok(Mp::Arr(invert(self.part, &ops_json(ops)).iter().map(to_mp).collect()))
    }
    fn touched(&self, ops: &Mp) -> Vec<TouchReq> {
        touched(self.part, &ops_json(ops))
    }
    fn snapshot(&mut self) -> Vec<u8> {
        let v = Mp::map(vec![("v", Mp::from(SCHEMA)), ("part", to_mp(&shared(self.part)))]);
        cc_live::msgpack::pack(&v).unwrap_or_default()
    }
    fn load(&mut self, bytes: &[u8], _q: u64) -> Result<(), String> {
        let x = cc_live::msgpack::unpack(bytes).map_err(|e| e.0.to_string())?;
        if x.get("v").and_then(Mp::as_u64) != Some(SCHEMA) {
            return Err("live snapshot: unknown format".into());
        }
        let mut p = x.get("part").map(to_json).filter(J::is_object).ok_or("live snapshot: unknown format")?;
        let seq = self.part.get("seq").and_then(J::as_f64).filter(|q| *q != 0.0).unwrap_or(1.0);
        if let (Some(o), Some(n)) = (p.as_object_mut(), serde_json::Number::from_f64(seq)) {
            o.insert("seq".into(), J::Number(n));
        }
        claim_ids(&mut p, self.c);
        *self.part = p;
        *self.changed = true;
        Ok(())
    }
    fn validate(&mut self, ops: &Mp) -> Option<String> {
        validate(self.part, &ops_json(ops))
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The window's side (liveapps.rs): live.js `watch(app, a, client)` — every history step goes out once the part settles
// (at once after a command, when a sketch drag stops), and a change with no step of its own (a drag) goes out as one.

/// PartCraft's [`crate::live::LiveView`] over the window's part (a joiner with nothing open gets a new part).
pub struct PartLive<'a>(pub &'a mut crate::UiApp);

impl PartLive<'_> {
    fn pv(&mut self) -> &mut crate::partview::PartView {
        self.0.part.get_or_insert_with(crate::partview::PartView::new_part)
    }
    /// Undo / Redo pressed while live: the person's own steps.
    pub fn reverse_asked(&mut self) {
        let Some(undo) = self.0.part.as_mut().and_then(|p| p.live_req.take()) else { return };
        #[cfg(target_arch = "wasm32")]
        let _ = undo;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(l) = crate::live::reverse(self, undo) {
            let pv = self.pv();
            pv.note = match l {
                Some(l) => crate::partview::trv(if undo { "app.undo.undid" } else { "app.undo.redid" }, &[("what", l.to_lowercase())]),
                None => crate::partview::tr(if undo { "app.undo.nothing_undo" } else { "app.undo.nothing_redo" }),
            };
        }
    }
}

impl crate::live::LiveView for PartLive<'_> {
    const APP: &'static str = "partcraft";
    const PREFIX: &'static str = "part";
    fn live_slot(&mut self) -> &mut Option<crate::live::LiveSession> {
        &mut self.0.live
    }
    fn live_window_open(&mut self) -> &mut bool {
        &mut self.0.live_window
    }
    fn with_adapter(&mut self, c: u64, f: &mut dyn FnMut(&mut dyn Adapter)) {
        let pv = self.pv();
        let mut changed = false;
        f(&mut PartAdapter { part: &mut pv.part, c, changed: &mut changed });
        if changed {
            pv.live_applied();
        }
    }
    fn claim_ids(&mut self, c: u64) {
        claim_ids(&mut self.pv().part, c);
    }
    fn release_ids(&mut self) {
        let pv = self.pv();
        release_ids(&mut pv.part);
        pv.undo.clear();
        pv.redo.clear();
        pv.live_on = false;
        pv.live_known = None;
    }
    fn live_bind(&mut self) {
        let pv = self.pv();
        pv.undo.clear();
        pv.redo.clear();
        pv.live_on = true;
        pv.live_known = Some(pv.part.clone());
    }
    fn model_version(&self) -> u64 {
        self.0.part.as_ref().map_or(0, |p| p.live_ver)
    }
    fn after_remote(&mut self) {
        let pv = self.pv();
        pv.live_on = true;
        // the rebuild re-solves sketches in place: what it writes is not an edit of ours (live.js `resync`)
        pv.live_known = Some(pv.part.clone());
    }
    fn tr(&self, key: &str, vars: &[(&str, String)]) -> String {
        let v: Vec<(&str, cc_part::i18n::F)> = vars.iter().map(|(k, s)| (*k, cc_part::i18n::F::S(s.clone()))).collect();
        cc_part::i18n::t(key, &v)
    }
    fn say(&mut self, msg: String) {
        if let Some(p) = self.0.part.as_mut() {
            p.message = msg.clone();
        }
        self.0.message = msg;
    }
    fn live_state(&mut self) -> Option<J> {
        Some(shared(&self.pv().part))
    }
    fn local_steps(&mut self, g: &mut dyn FnMut() -> String) -> Vec<(String, Mp, Mp)> {
        let pv = self.pv();
        // a sketch drag goes out once it stops
        if pv.sk.dragging() {
            return vec![];
        }
        let steps = std::mem::take(&mut pv.undo);
        let known = pv.live_known.take().unwrap_or_else(|| pv.part.clone());
        let mut out = vec![];
        let mut push = |label: &str, a: &J, b: &J, out: &mut Vec<(String, Mp, Mp)>| {
            let ops = diff(a, b);
            if ops.is_empty() {
                return;
            }
            let inverse = diff(b, a);
            let mark = g();
            out.push((label.to_string(), crate::livestate::with_mark(&mark, &ops), crate::livestate::with_mark(&mark, &inverse)));
        };
        if steps.is_empty() {
            // a change with no history step of its own (live.js: the part changed since it was last sent)
            if !same(Some(&known), Some(&pv.part)) {
                let label = pv.live_label.clone();
                push(&label, &known, &pv.part, &mut out);
            }
        } else {
            // anything that changed before the first step (a drag that ended in one) goes with it
            let first_before = steps.first().map(|s| s.1.clone()).unwrap_or_default();
            if !same(Some(&known), Some(&first_before)) {
                let label = pv.live_label.clone();
                push(&label, &known, &first_before, &mut out);
            }
            for (i, (label, before)) in steps.iter().enumerate() {
                let after = steps.get(i + 1).map_or(&pv.part, |n| &n.1);
                push(label, before, after, &mut out);
            }
            if let Some((l, _)) = steps.last() {
                pv.live_label = l.clone();
            }
        }
        pv.live_known = Some(pv.part.clone());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids_named_in_a_feature() {
        assert_eq!(
            ids_in(&json!({"sketch": "f3", "face": "f2.end", "body": "@f4", "x": "ff1", "step": "f9", "n": "f12a"})),
            vec!["f3", "f2", "f4", "f12"]
        );
        let a = json!({"name": "P", "seq": 3, "features": [{"id": "f1", "type": "sketch"}, {"id": "f2", "type": "extrude", "sketch": "f1", "d": 10}]});
        let b = json!({"name": "P", "seq": 4, "features": [{"id": "f1", "type": "sketch"}, {"id": "f2", "type": "extrude", "sketch": "f1", "d": 20}, {"id": "f3", "type": "fillet", "edges": ["f2.e1"]}]});
        let ops = diff(&a, &b);
        let mut x = a.clone();
        let inv = invert(&x, &ops);
        for o in &ops {
            apply_one(&mut x, o);
        }
        assert_eq!(shared(&x), shared(&b));
        for o in &inv {
            apply_one(&mut x, o);
        }
        assert_eq!(shared(&x), shared(&a));
        let t = touched(&a, &ops);
        assert!(t.iter().any(|t| t.k == "e:f/f3" && t.m == 'c') && t.iter().any(|t| t.k == "e:f/f2" && t.m == 'r'));
        assert_eq!(validate(&a, &[json!({"o": "del", "id": "f1"})]).as_deref(), Some("feature-in-use"));
        let _ = to_json(&Mp::Nil);
    }
}
