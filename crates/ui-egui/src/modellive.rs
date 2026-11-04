//! ModelCraft in a Studio Live session (0.5 modelcraft/dev/live.js, LV.22): the model's native JSON (groups and
//! components as `defs`, each with its own vertices, edges, faces and placements); every step's ops are the difference of
//! its copies before and after, through the shared state core (livestate.rs):
//!
//! - a group's (or component's) geometry — vertices, edges, faces, curves — is one structural op: when two people reshape
//!   the same group at once the second is refused and taken back; different groups merge freely;
//! - placements, names of groups, guides, dimensions and section planes merge per item and field;
//! - scenes, tags, storeys, units, the drawing axes travel whole (the later one wins).
//!
//! Ids: each person's new definitions, guides and placed things number from their own range (c × 1e6 + n); `nextId` and
//! each definition's `next` are personal.

use cc_live::doc::{Adapter, TouchReq};
use cc_live::msgpack::Value as Mp;
use cc_m3d::model::{Model, SnapRef};
use serde_json::Value as J;

use crate::livestate::{Cfg, ops_json, to_mp};

pub const SCHEMA: u64 = 1;
const ID_BLOCK: f64 = 1e6;

/// live.js `refs`: a placement names the definition it places; that one must still be there.
fn refs(op: &J, path: &[String], out: &mut Vec<String>) {
    let v = op.get("v");
    if path.first().map(String::as_str) == Some("defs")
        && let Some(d) = v.and_then(|v| v.get("def")).and_then(J::as_str)
        && v.is_some_and(J::is_object)
    {
        out.push(format!("e:defs/#{d}"));
    } else if path.last().map(String::as_str) == Some("def")
        && let Some(d) = v.and_then(J::as_str)
    {
        out.push(format!("e:defs/#{d}"));
    }
}

/// live.js `CORE` for ModelCraft.
pub static CORE: Cfg = Cfg {
    lists: &[("defs", "id"), ("defs/*/insts", "id"), ("guides", "id"), ("dims", "id"), ("sections", "id")],
    atomic: &[("defs/*", "geo", &["verts", "edges", "faces", "splines"])],
    personal: &["nextId", "defs/*/next"],
    depth: 1,
    refs: Some(refs),
};

fn json_of(m: &mut Model) -> J {
    serde_json::from_str(&m.snapshot()).unwrap_or(J::Null)
}
fn num_id(s: &str) -> f64 {
    let d: String = s.chars().skip_while(|c| !c.is_ascii_digit()).collect();
    if d.trim().is_empty() {
        return 0.0;
    }
    d.parse::<f64>().unwrap_or(f64::NAN)
}

/// live.js `claimIds()`: the model's nextId and every definition's next from this client's range.
pub fn claim_ids(m: &mut Model, c: u64) {
    #[allow(clippy::cast_precision_loss)]
    let lo = c as f64 * ID_BLOCK;
    let hi = lo + ID_BLOCK;
    let in_r = |n: f64| n > lo && n < hi;
    let mut top = lo + 1.0;
    for d in m.defs.values_mut() {
        let n = num_id(&d.id);
        if in_r(n) {
            top = top.max(n + 1.0);
        }
        let mut nx = lo + 1.0;
        let keys: Vec<f64> = d
            .verts
            .keys()
            .chain(d.edges.keys())
            .chain(d.faces.keys())
            .chain(d.insts.keys())
            .chain(d.splines.keys().map(|k| k.parse::<f64>().unwrap_or(f64::NAN)))
            .collect();
        for k in keys {
            if in_r(k) {
                nx = nx.max(k + 1.0);
            }
        }
        d.next = nx.max(if in_r(d.next) { d.next } else { 0.0 });
    }
    let keys: Vec<f64> = m.guides.keys().chain(m.dims.keys()).chain(m.sections.keys()).collect();
    for k in keys {
        if in_r(k) {
            top = top.max(k + 1.0);
        }
    }
    m.next_id = top.max(if in_r(m.next_id) { m.next_id } else { 0.0 });
}

/// live.js `releaseIds()`: numbering goes on above every id in the model (as opening a file does).
pub fn release_ids(m: &mut Model) {
    let mut top = m.next_id;
    for d in m.defs.values_mut() {
        let n = num_id(&d.id) + 1.0;
        if n.is_finite() {
            top = top.max(n);
        }
        let mut nx = d.next;
        let keys: Vec<f64> = d
            .verts
            .keys()
            .chain(d.edges.keys())
            .chain(d.faces.keys())
            .chain(d.insts.keys())
            .chain(d.splines.keys().map(|k| k.parse::<f64>().unwrap_or(f64::NAN)))
            .collect();
        for k in keys {
            if k.is_finite() {
                nx = nx.max(k + 1.0);
            }
        }
        d.next = nx;
    }
    let keys: Vec<f64> = m.guides.keys().chain(m.dims.keys()).chain(m.sections.keys()).collect();
    for k in keys {
        top = top.max(k + 1.0);
    }
    m.next_id = top;
}

/// live.js `put(j)`: the model from its JSON, each window's own counters kept.
fn put(m: &mut Model, j: &J, c: u64) -> Result<(), String> {
    let keep: Vec<(String, f64)> = m.defs.iter().map(|(id, d)| (id.clone(), d.next)).collect();
    let next_id = m.next_id;
    m.restore(&SnapRef::Str(cc_detail::js::stringify(j).into())).map_err(|e| format!("{e:?}"))?;
    m.next_id = next_id;
    for (id, n) in keep {
        if let Some(d) = m.defs.get_mut(id.as_str()) {
            d.next = n;
        }
    }
    claim_ids(m, c);
    m.bump_version();
    m.dirty = true;
    Ok(())
}

/// The model without its personal counters (live.js `hash()`).
pub fn shared(m: &mut Model) -> J {
    let mut j = json_of(m);
    if let Some(o) = j.as_object_mut() {
        o.shift_remove("nextId");
        if let Some(J::Array(ds)) = o.get_mut("defs") {
            for d in ds {
                if let Some(d) = d.as_object_mut() {
                    d.shift_remove("next");
                }
            }
        }
    }
    j
}

/// live.js `validate(ops)`: no placement of a definition that is gone, and no group placed inside itself.
pub fn validate(j: &J) -> Option<String> {
    let defs = j.get("defs").and_then(J::as_array).map_or(&[][..], Vec::as_slice);
    let ids: Vec<String> = defs.iter().map(|d| crate::livestate::id_text(d.get("id").unwrap_or(&J::Null))).collect();
    if !ids.iter().any(|i| i == "root") {
        return Some("invalid".into());
    }
    let kids: Vec<(String, Vec<String>)> = defs
        .iter()
        .zip(&ids)
        .map(|(d, id)| {
            let l = d.get("insts").and_then(J::as_array).map_or(&[][..], Vec::as_slice);
            (id.clone(), l.iter().map(|i| crate::livestate::id_text(i.get("def").unwrap_or(&J::Null))).collect())
        })
        .collect();
    for (_, l) in &kids {
        if l.iter().any(|d| !ids.contains(d)) {
            return Some("invalid".into());
        }
    }
    // a cycle: walk with an explicit stack (a deep or cyclic model never overflows)
    let mut state: std::collections::HashMap<&str, u8> = std::collections::HashMap::new();
    for start in &ids {
        if state.get(start.as_str()).copied() == Some(2) {
            continue;
        }
        let mut stack: Vec<(&str, usize)> = vec![(start.as_str(), 0)];
        state.insert(start.as_str(), 1);
        while let Some((id, i)) = stack.last().copied() {
            let ks = kids.iter().find(|(k, _)| k == id).map(|(_, l)| l.as_slice()).unwrap_or(&[]);
            match ks.get(i) {
                Some(k) => {
                    if let Some(top) = stack.last_mut() {
                        top.1 = i.saturating_add(1);
                    }
                    match state.get(k.as_str()).copied() {
                        Some(1) => return Some("invalid".into()),
                        Some(2) => {}
                        _ => {
                            state.insert(k.as_str(), 1);
                            stack.push((k.as_str(), 0));
                        }
                    }
                }
                None => {
                    state.insert(id, 2);
                    stack.pop();
                }
            }
        }
    }
    None
}

/// The adapter over a model (live.js `createLive`).
pub struct McAdapter<'a> {
    pub m: &'a mut Model,
    pub c: u64,
}

impl Adapter for McAdapter<'_> {
    fn apply(&mut self, ops: &Mp, _origin: &str) -> Result<(), String> {
        let ops = ops_json(ops);
        // a tool previewing redraws from its starting copy on every frame: others' edits go into that copy too
        let base = self.m.base().map_err(|e| format!("{e:?}"))?;
        let keep = match base {
            Some(b) => {
                let mut bj: J = serde_json::from_str(&b).unwrap_or(J::Null);
                CORE.apply(&mut bj, &ops);
                Some(cc_detail::js::stringify(&bj))
            }
            None => None,
        };
        let mut j = json_of(self.m);
        CORE.apply(&mut j, &ops);
        put(self.m, &j, self.c)?;
        if let Some(k) = keep {
            self.m.set_base(Some(&k));
        }
        Ok(())
    }
    fn invert(&mut self, ops: &Mp) -> Result<Mp, String> {
        let j = json_of(self.m);
        Ok(Mp::Arr(CORE.invert(&j, &ops_json(ops)).iter().map(to_mp).collect()))
    }
    fn touched(&self, ops: &Mp) -> Vec<TouchReq> {
        CORE.touched(&ops_json(ops))
    }
    fn snapshot(&mut self) -> Vec<u8> {
        let v = Mp::map(vec![("v", Mp::from(SCHEMA)), ("model", Mp::Str(self.m.snapshot()))]);
        cc_live::msgpack::pack(&v).unwrap_or_default()
    }
    fn load(&mut self, bytes: &[u8], _q: u64) -> Result<(), String> {
        let x = cc_live::msgpack::unpack(bytes).map_err(|e| e.0.to_string())?;
        if x.get("v").and_then(Mp::as_u64) != Some(SCHEMA) {
            return Err("live snapshot: unknown format".into());
        }
        let text = x.get("model").and_then(Mp::as_str).ok_or("live snapshot: unknown format")?;
        self.m.restore(&SnapRef::Str(text.into())).map_err(|e| format!("{e:?}"))?;
        self.m.undo_stack.clear();
        self.m.redo_stack.clear();
        claim_ids(self.m, self.c);
        self.m.bump_version();
        Ok(())
    }
    fn validate(&mut self, ops: &Mp) -> Option<String> {
        let mut j = json_of(self.m);
        CORE.apply(&mut j, &ops_json(ops));
        validate(&j)
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The window's side (liveapps.rs)

/// ModelCraft's [`crate::live::LiveView`] over the window's model.
pub struct McLive<'a>(pub &'a mut crate::UiApp);

impl McLive<'_> {
    /// The model view (a joiner with nothing open gets an empty model to take the snapshot).
    fn mv(&mut self) -> Option<&mut crate::modelcraft::ModelView> {
        if self.0.mc.is_none() {
            self.0.show_model(Model::new(), "Live.modelcraft");
        }
        self.0.mc.as_mut()
    }
    /// Undo / Redo pressed while live: the person's own steps.
    pub fn reverse_asked(&mut self) {
        let Some(redo) = self.0.mc.as_mut().and_then(|m| m.live_req.take()) else { return };
        #[cfg(target_arch = "wasm32")]
        let _ = redo;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(l) = crate::live::reverse(self, !redo) {
            let Some(mv) = self.mv() else { return };
            mv.message = match (l, redo) {
                (Some(w), false) => crate::modelcraft::mt("app.undid", &[("what", &w)]),
                (Some(w), true) => crate::modelcraft::mt("app.redid", &[("what", &w)]),
                (None, false) => crate::modelcraft::mt("app.undo.nothing", &[]),
                (None, true) => crate::modelcraft::mt("app.redo.nothing", &[]),
            };
            mv.after_history();
        }
    }
}

impl crate::live::LiveView for McLive<'_> {
    const APP: &'static str = "modelcraft";
    const PREFIX: &'static str = "model";
    fn live_slot(&mut self) -> &mut Option<crate::live::LiveSession> {
        &mut self.0.live
    }
    fn live_window_open(&mut self) -> &mut bool {
        &mut self.0.live_window
    }
    fn with_adapter(&mut self, c: u64, f: &mut dyn FnMut(&mut dyn Adapter)) {
        if let Some(mv) = self.mv() {
            f(&mut McAdapter { m: &mut mv.tools.host.model, c });
        }
    }
    fn claim_ids(&mut self, c: u64) {
        if let Some(mv) = self.mv() {
            claim_ids(&mut mv.tools.host.model, c);
        }
    }
    fn release_ids(&mut self) {
        let Some(mv) = self.mv() else { return };
        release_ids(&mut mv.tools.host.model);
        mv.tools.host.model.undo_stack.clear();
        mv.tools.host.model.redo_stack.clear();
        mv.live_on = false;
    }
    fn live_bind(&mut self) {
        let Some(mv) = self.mv() else { return };
        mv.tools.host.model.undo_stack.clear();
        mv.tools.host.model.redo_stack.clear();
        mv.live_on = true;
    }
    fn model_version(&self) -> u64 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let v = self.0.mc.as_ref().map_or(0.0, |m| m.tools.host.model.version()).max(0.0) as u64;
        v
    }
    fn after_remote(&mut self) {
        let Some(mv) = self.mv() else { return };
        mv.live_on = true;
        mv.after_history();
    }
    fn tr(&self, key: &str, vars: &[(&str, String)]) -> String {
        let v: Vec<(&str, &str)> = vars.iter().map(|(k, s)| (*k, s.as_str())).collect();
        crate::modelcraft::mt(key, &v)
    }
    fn say(&mut self, msg: String) {
        if let Some(m) = self.0.mc.as_mut() {
            m.message = msg.clone();
        }
        self.0.message = msg;
    }
    fn live_state(&mut self) -> Option<J> {
        self.mv().map(|mv| shared(&mut mv.tools.host.model))
    }
    fn local_steps(&mut self, g: &mut dyn FnMut() -> String) -> Vec<(String, Mp, Mp)> {
        let Some(mv) = self.mv() else { return vec![] };
        let m = &mut mv.tools.host.model;
        if m.undo_stack.is_empty() {
            return vec![];
        }
        let steps = std::mem::take(&mut m.undo_stack);
        let mut out = vec![];
        for st in &steps {
            let (a, b) = (st.b.s(), st.a.s());
            if a == b {
                continue;
            }
            let (a, b): (J, J) = (serde_json::from_str(&a).unwrap_or(J::Null), serde_json::from_str(&b).unwrap_or(J::Null));
            let mark = g();
            if let Some((ops, inverse)) = CORE.step(&mark, &a, &b) {
                out.push((st.label.clone(), ops, inverse));
            }
        }
        out
    }
}
