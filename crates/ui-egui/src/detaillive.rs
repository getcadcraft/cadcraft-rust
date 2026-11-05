//! DetailCraft in a Studio Live session (0.5 detailcraft/dev/live.js, the app adapter, LV.22): the model's state is
//! plain JSON and every edit's ops are the difference of the state before and after it (the shared state-diff core,
//! livestate.rs: members, connections, footings, stairs, bolts and issues merge per item and field, profiles per name, the
//! rest as values; a connection, bolt group or stair reads the members it names). Built geometry, marks and schedules are
//! never sent: every window works them out. Each person's new ids come from their own range (client c: c × 1e6 + n).
//! The session, the commands `detail.live.*` and the Live window are live.rs's.

use cc_detail::js::parse;
#[cfg(test)]
use cc_detail::js::stringify;
use cc_detail::store::State;
use cc_live::doc::{Adapter, TouchReq};
use cc_live::msgpack::Value as Mp;
use serde_json::Value as J;

use crate::detailview::DetailView;
pub use crate::live::LiveSession;
use crate::livestate::{Cfg, ops_json, to_mp};

pub const SCHEMA: u64 = 1;
const ID_BLOCK: f64 = 1e6;

fn is_member_id(s: &str) -> bool {
    s.len() > 1 && s.starts_with('m') && s.get(1..).is_some_and(|d| d.bytes().all(|c| c.is_ascii_digit()))
}
/// live.js `refs`: a connection, bolt group or stair reads the members it is made on.
fn refs(op: &J, path: &[String], out: &mut Vec<String>) {
    if path.first().map(String::as_str) == Some("members") {
        return;
    }
    fn walk(v: &J, out: &mut Vec<String>) {
        match v {
            J::String(s) if is_member_id(s) => {
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

/// live.js `CORE` for DetailCraft.
pub static CORE: Cfg = Cfg {
    lists: &[
        ("members", "id"),
        ("members/*/holes", "id"),
        ("members/*/cuts", "id"),
        ("connections", "id"),
        ("footings", "id"),
        ("stairs", "id"),
        ("bolts", "id"),
        ("profiles", "name"),
        ("issues", "guid"),
    ],
    atomic: &[],
    personal: &["seq"],
    depth: 2,
    refs: Some(refs),
};

pub fn diff(a: &J, b: &J) -> Vec<J> {
    CORE.diff(a, b)
}
pub fn apply(root: &mut J, ops: &[J]) {
    CORE.apply(root, ops);
}
pub fn invert(root: &J, ops: &[J]) -> Vec<J> {
    CORE.invert(root, ops)
}
pub fn touched(ops: &[J]) -> Vec<TouchReq> {
    CORE.touched(ops)
}
fn id_text(v: &J) -> String {
    crate::livestate::id_text(v)
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

/// live.js `releaseIds()`: after the session, numbering goes on above every id in the model.
pub fn release_ids(state: &mut J) {
    let mut top = state.get("seq").and_then(J::as_f64).filter(|x| *x != 0.0 && x.is_finite()).unwrap_or(1.0);
    for k in ["members", "connections", "footings", "stairs", "bolts"] {
        for x in state.get(k).and_then(J::as_array).map(Vec::as_slice).unwrap_or(&[]) {
            let n = num_id(&id_text(x.get("id").unwrap_or(&J::Null))) + 1.0;
            if n.is_finite() {
                top = top.max(n);
            }
            for h in ["holes", "cuts"].iter().flat_map(|f| x.get(*f).and_then(J::as_array).map(Vec::as_slice).unwrap_or(&[])) {
                let n = num_id(&id_text(h.get("id").unwrap_or(&J::Null))) + 1.0;
                if n.is_finite() {
                    top = top.max(n);
                }
            }
        }
    }
    if let (Some(o), Some(n)) = (state.as_object_mut(), serde_json::Number::from_f64(top)) {
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
        // the state's JSON is the boundary: the ops are paths into it
        let mut st = self.model.state.to_value();
        CORE.apply(&mut st, &ops_json(ops));
        self.model.state = State::from_value(st);
        self.model.version = self.model.version.saturating_add(1);
        Ok(())
    }
    fn invert(&mut self, ops: &Mp) -> Result<Mp, String> {
        Ok(Mp::Arr(CORE.invert(&self.model.state.to_value(), &ops_json(ops)).iter().map(to_mp).collect()))
    }
    fn touched(&self, ops: &Mp) -> Vec<TouchReq> {
        CORE.touched(&ops_json(ops))
    }
    fn snapshot(&mut self) -> Vec<u8> {
        let v = Mp::map(vec![("v", Mp::from(SCHEMA)), ("state", Mp::Str(self.model.state.stringify()))]);
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
        self.model.state = State::from_value(st);
        self.model.undo_stack.clear();
        self.model.redo_stack.clear();
        self.model.version = self.model.version.saturating_add(1);
        Ok(())
    }
    fn validate(&mut self, ops: &Mp) -> Option<String> {
        let mut s = self.model.state.to_value();
        CORE.apply(&mut s, &ops_json(ops));
        let ids: Vec<String> = s
            .get("members")
            .and_then(J::as_array)
            .map(|a| a.iter().map(|m| id_text(m.get("id").unwrap_or(&J::Null))).collect())
            .unwrap_or_default();
        fn bad(v: &J, ids: &[String]) -> bool {
            match v {
                J::String(x) if is_member_id(x) => !ids.contains(x),
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

impl crate::live::LiveView for DetailView {
    const APP: &'static str = "detailcraft";
    const PREFIX: &'static str = "detail";
    fn live_slot(&mut self) -> &mut Option<LiveSession> {
        &mut self.live
    }
    fn live_window_open(&mut self) -> &mut bool {
        &mut self.live_window
    }
    fn with_adapter(&mut self, c: u64, f: &mut dyn FnMut(&mut dyn Adapter)) {
        f(&mut DcAdapter { model: &mut self.model, c });
    }
    fn claim_ids(&mut self, c: u64) {
        let mut st = self.model.state.to_value();
        claim_ids(&mut st, c);
        self.model.state = State::from_value(st);
    }
    fn release_ids(&mut self) {
        let mut st = self.model.state.to_value();
        release_ids(&mut st);
        self.model.state = State::from_value(st);
        self.model.undo_stack.clear();
        self.model.redo_stack.clear();
    }
    fn live_bind(&mut self) {
        self.model.undo_stack.clear();
        self.model.redo_stack.clear();
    }
    fn model_version(&self) -> u64 {
        self.model.version
    }
    fn after_remote(&mut self) {
        self.recompute();
    }
    fn tr(&self, key: &str, vars: &[(&str, String)]) -> String {
        let v: Vec<(&str, cc_detail::i18n::F)> = vars.iter().map(|(k, s)| (*k, cc_detail::i18n::F::S(s.clone()))).collect();
        cc_detail::i18n::t(key, &v)
    }
    fn say(&mut self, msg: String) {
        self.message = msg;
    }
}

impl DetailView {
    /// Undo / redo can act: the session's own steps while live, else the model's.
    pub fn can_undo_now(&self) -> bool {
        self.live.as_ref().map_or_else(|| self.model.can_undo(), |s| s.doc.can_undo())
    }
    pub fn can_redo_now(&self) -> bool {
        self.live.as_ref().map_or_else(|| self.model.can_redo(), |s| s.doc.can_redo())
    }
    /// Carry what arrived from the session into the model (every frame).
    pub fn live_poll(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        crate::live::poll(self);
    }
    /// A step this window made (`model.change` grew the version): its ops go out (live.js's wrapped `change`).
    pub fn live_local(&mut self, label: &str, version_before: u64) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.model.version == version_before {
                return;
            }
            let Some(s) = self.live.as_mut() else { return };
            let Some(before) = self.model.undo_stack.last().and_then(|t| parse(&t.json).ok()) else { return };
            let g = s.next_group();
            if let Some((ops, inverse)) = CORE.step(&g, &before, &self.model.state.to_value()) {
                crate::live::local_change(self, label, ops, inverse);
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (label, version_before);
    }
    /// Undo / redo in a live session: the person's own steps. → the label.
    pub fn live_reverse(&mut self, undo: bool) -> Option<Option<String>> {
        #[cfg(not(target_arch = "wasm32"))]
        return crate::live::reverse(self, undo);
        #[cfg(target_arch = "wasm32")]
        {
            let _ = undo;
            None
        }
    }
}

/// `detail.live.*` commands (live.rs).
pub fn run(dv: &mut DetailView, id: &str, name: &str) -> Result<(), String> {
    crate::live::run(dv, id, name)
}

/// The Live window (live.rs), its buttons run `detail.live.*`.
#[cfg(not(target_arch = "wasm32"))]
pub fn window(dv: &mut DetailView, ctx: &egui::Context, tk: &crate::theme::Tokens, acts: &mut Vec<crate::detailpanel::Act>) {
    let mut la: Vec<(String, String)> = vec![];
    crate::live::window(dv, ctx, tk, &mut la);
    let _ = acts;
    for (id, n) in la {
        if let Err(e) = crate::live::run(dv, &id, &n) {
            dv.message = e;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::livestate::to_json;

    fn sample_state() -> J {
        cc_detail::sample::sample_model("Sample frame").expect("sample").state.to_value()
    }

    #[test]
    fn diff_applies_and_inverts_back() {
        let a = sample_state();
        let mut m = cc_detail::model::Model::new(State::from_value(a.clone()));
        m.change("edit", |tx| {
            let id = tx.s.list("members").get(6).and_then(|x| x.get("id")).map_or(J::Null, cc_detail::jv::JVal::to_value);
            tx.update_member(Some(&id), Some(&serde_json::json!({"profile": "W18x35"})))?;
            tx.set_name(Some(&J::from("Changed")));
            Ok(())
        })
        .expect("change");
        let b = m.state.to_value();
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
