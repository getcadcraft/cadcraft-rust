//! CivilCraft in a Studio Live session (0.5 civilcraft/dev/live.js, LV.22): the document is the project's state as its
//! own undo copies keep it (without the surface's TIN and the display settings) plus its points; every edit's ops are
//! the difference, through the shared state core (livestate.rs): alignments, pads, feature lines, parcels, GIS layers
//! and maps merge per item and field; points per point number; the surface's own settings per field. The TIN is never
//! sent: every window builds it from the points and breaklines. Not shared: GIS layers' features, map pictures, grid
//! shift files and a grid surface loaded from a file. Ids: each person's new objects number from their own range
//! (client c: c × 1e6 + n); `seq` is personal.

use cc_civil::project::Project;
use cc_live::doc::{Adapter, TouchReq};
use cc_live::msgpack::Value as Mp;
use serde_json::{Value as J, json};

use crate::livestate::{Cfg, ops_json, to_json, to_mp};

pub const SCHEMA: u64 = 1;
const ID_BLOCK: f64 = 1e6;

/// live.js `CORE` for CivilCraft.
pub static CORE: Cfg = Cfg {
    lists: &[
        ("state/alignments", "id"),
        ("state/pads", "id"),
        ("state/features", "id"),
        ("state/parcels", "id"),
        ("state/gis/layers", "id"),
        ("state/gis/maps", "id"),
        ("state/surface/breaklines", "id"),
        ("points", "n"),
    ],
    atomic: &[],
    personal: &["state/seq"],
    depth: 3,
    refs: None,
};

fn points_json(p: &Project) -> J {
    J::Array(p.points.iter().map(cc_civil::js::J::j).collect())
}
/// live.js `doc()`: {state (the undo copy's form), points}.
pub fn doc(p: &Project) -> J {
    let state: J = serde_json::from_str(&p.snapshot()).unwrap_or(J::Null);
    json!({"state": state, "points": points_json(p)})
}
fn seq_of(p: &Project) -> J {
    p.state.get("seq").cloned().unwrap_or(J::Null)
}
/// The state with the window's own `seq`, as restore() takes it.
fn with_seq(mut st: J, seq: J) -> String {
    if let Some(o) = st.as_object_mut() {
        if seq.is_null() {
            o.shift_remove("seq");
        } else {
            o.insert("seq".into(), seq);
        }
    }
    cc_detail::js::stringify(&st)
}

/// live.js `put(ops)`: ops into the project, its state through restore() (which keeps the TIN and the display), the
/// points as a new list.
pub fn put(p: &mut Project, ops: &[J]) {
    let mut d = doc(p);
    CORE.apply(&mut d, ops);
    let seq = seq_of(p);
    let st = d.get("state").cloned().unwrap_or(J::Null);
    p.live_restore(&with_seq(st, seq));
    if ops.iter().any(|o| o.get("path").and_then(|x| x.get(0)).and_then(J::as_str) == Some("points")) {
        p.points = d.get("points").and_then(J::as_array).map(|a| a.iter().map(cc_civil::points::read_pt).collect()).unwrap_or_default();
        p.points_version = p.points_version.saturating_add(1);
    }
    p.touch();
}

fn num_id(v: &J) -> f64 {
    let s = crate::livestate::id_text(v);
    let d: String = s.chars().skip_while(|c| !c.is_ascii_digit()).collect();
    if d.trim().is_empty() {
        return 0.0;
    }
    d.parse::<f64>().unwrap_or(f64::NAN)
}
fn ids(s: &J) -> impl Iterator<Item = &J> {
    ["alignments", "pads", "features", "parcels"]
        .into_iter()
        .flat_map(move |k| s.get(k).and_then(J::as_array).map_or(&[][..], Vec::as_slice).iter())
        .chain(s.get("gis").and_then(|g| g.get("layers")).and_then(J::as_array).map_or(&[][..], Vec::as_slice).iter())
        .filter_map(|x| x.get("id"))
}

/// live.js `claimIds()`.
pub fn claim_ids(p: &mut Project, c: u64) {
    #[allow(clippy::cast_precision_loss)]
    let lo = c as f64 * ID_BLOCK;
    let hi = lo + ID_BLOCK;
    let mut next = lo + 1.0;
    for id in ids(&p.state) {
        let n = num_id(id);
        if n > lo && n < hi && n >= next {
            next = n + 1.0;
        }
    }
    let cur = p.state.get("seq").and_then(J::as_f64).unwrap_or(0.0);
    let v = next.max(if cur > lo && cur < hi { cur } else { 0.0 });
    if let (Some(o), Some(n)) = (p.state.as_object_mut(), serde_json::Number::from_f64(v)) {
        o.insert("seq".into(), J::Number(n));
    }
}

/// live.js `releaseIds()`.
pub fn release_ids(p: &mut Project) {
    let mut top = p.state.get("seq").and_then(J::as_f64).filter(|x| *x != 0.0 && x.is_finite()).unwrap_or(1.0);
    let s = p.state.clone();
    for k in ["alignments", "pads", "features", "parcels"] {
        for x in s.get(k).and_then(J::as_array).map_or(&[][..], Vec::as_slice) {
            let n = num_id(x.get("id").unwrap_or(&J::Null)) + 1.0;
            if n.is_finite() {
                top = top.max(n);
            }
        }
    }
    if let (Some(o), Some(n)) = (p.state.as_object_mut(), serde_json::Number::from_f64(top)) {
        o.insert("seq".into(), J::Number(n));
    }
}

/// live.js `hash()`'s document: the state without `seq`, the points.
pub fn shared(p: &Project) -> J {
    let mut d = doc(p);
    if let Some(s) = d.get_mut("state").and_then(J::as_object_mut) {
        s.shift_remove("seq");
    }
    d
}

/// The adapter over a project (live.js `createLive`).
pub struct CvAdapter<'a> {
    pub p: &'a mut Project,
    pub c: u64,
}

impl Adapter for CvAdapter<'_> {
    fn apply(&mut self, ops: &Mp, _origin: &str) -> Result<(), String> {
        put(self.p, &ops_json(ops));
        Ok(())
    }
    fn invert(&mut self, ops: &Mp) -> Result<Mp, String> {
        Ok(Mp::Arr(CORE.invert(&doc(self.p), &ops_json(ops)).iter().map(to_mp).collect()))
    }
    fn touched(&self, ops: &Mp) -> Vec<TouchReq> {
        CORE.touched(&ops_json(ops))
    }
    fn snapshot(&mut self) -> Vec<u8> {
        let v = Mp::map(vec![("v", Mp::from(SCHEMA)), ("state", Mp::Str(self.p.snapshot())), ("points", to_mp(&points_json(self.p)))]);
        cc_live::msgpack::pack(&v).unwrap_or_default()
    }
    fn load(&mut self, bytes: &[u8], _q: u64) -> Result<(), String> {
        let x = cc_live::msgpack::unpack(bytes).map_err(|e| e.0.to_string())?;
        if x.get("v").and_then(Mp::as_u64) != Some(SCHEMA) {
            return Err("live snapshot: unknown format".into());
        }
        let text = x.get("state").and_then(Mp::as_str).ok_or("live snapshot: unknown format")?;
        let st: J = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let seq = seq_of(self.p);
        self.p.live_restore(&with_seq(st, seq));
        // built here from the points, as every window does
        if let Some(s) = self.p.state.get_mut("surface").and_then(J::as_object_mut) {
            s.insert("tin".into(), J::Null);
        }
        self.p.points = x
            .get("points")
            .map(to_json)
            .and_then(|v| v.as_array().map(|a| a.iter().map(cc_civil::points::read_pt).collect()))
            .unwrap_or_default();
        self.p.points_version = self.p.points_version.saturating_add(1);
        self.p.undo_stack.clear();
        self.p.redo_stack.clear();
        claim_ids(self.p, self.c);
        self.p.touch();
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The window's side (liveapps.rs)

/// CivilCraft's [`crate::live::LiveView`] over the window's project (a joiner with nothing open gets an empty one).
pub struct CvLive<'a>(pub &'a mut crate::UiApp);

impl CvLive<'_> {
    fn cv(&mut self) -> &mut crate::civilview::CivilView {
        self.0.civil.get_or_insert_with(|| crate::civilview::CivilView::new(Project::new(), "Live"))
    }
    /// Undo / Redo pressed while live: the person's own steps.
    pub fn reverse_asked(&mut self) {
        let Some(redo) = self.0.civil.as_mut().and_then(|c| c.live_req.take()) else { return };
        #[cfg(target_arch = "wasm32")]
        let _ = redo;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(l) = crate::live::reverse(self, !redo) {
            let m = match (l, redo) {
                (Some(w), false) => cc_civil::i18n::t("cv.undo.done", &[("what", cc_civil::i18n::F::S(w))]),
                (Some(w), true) => cc_civil::i18n::t("cv.redo.done", &[("what", cc_civil::i18n::F::S(w))]),
                (None, false) => cc_civil::i18n::t("cv.undo.none", &[]),
                (None, true) => cc_civil::i18n::t("cv.redo.none", &[]),
            };
            self.cv().message = m;
        }
    }
}

impl crate::live::LiveView for CvLive<'_> {
    const APP: &'static str = "civilcraft";
    const PREFIX: &'static str = "civil";
    fn live_slot(&mut self) -> &mut Option<crate::live::LiveSession> {
        &mut self.0.live
    }
    fn live_window_open(&mut self) -> &mut bool {
        &mut self.0.live_window
    }
    fn with_adapter(&mut self, c: u64, f: &mut dyn FnMut(&mut dyn Adapter)) {
        let cv = self.cv();
        f(&mut CvAdapter { p: &mut cv.project, c });
    }
    fn claim_ids(&mut self, c: u64) {
        claim_ids(&mut self.cv().project, c);
    }
    fn release_ids(&mut self) {
        let cv = self.cv();
        release_ids(&mut cv.project);
        cv.project.undo_stack.clear();
        cv.project.redo_stack.clear();
        cv.live_on = false;
    }
    fn live_bind(&mut self) {
        let cv = self.cv();
        cv.project.undo_stack.clear();
        cv.project.redo_stack.clear();
        cv.live_on = true;
    }
    fn model_version(&self) -> u64 {
        self.0.civil.as_ref().map_or(0, |c| c.project.version)
    }
    fn after_remote(&mut self) {
        let cv = self.cv();
        // the plan, the surface and the panels follow the project's version, as after any edit
        cv.live_on = true;
    }
    fn tr(&self, key: &str, vars: &[(&str, String)]) -> String {
        let v: Vec<(&str, cc_civil::i18n::F)> = vars.iter().map(|(k, s)| (*k, cc_civil::i18n::F::S(s.clone()))).collect();
        cc_civil::i18n::t(key, &v)
    }
    fn say(&mut self, msg: String) {
        if let Some(c) = self.0.civil.as_mut() {
            c.message = msg.clone();
        }
        self.0.message = msg;
    }
    fn live_state(&mut self) -> Option<J> {
        Some(shared(&self.cv().project))
    }
    fn local_steps(&mut self, g: &mut dyn FnMut() -> String) -> Vec<(String, Mp, Mp)> {
        let p = &mut self.cv().project;
        if p.undo_stack.is_empty() {
            return vec![];
        }
        let steps = std::mem::take(&mut p.undo_stack);
        // each step's document after it: the next step's copy, the last one the project now
        let now = doc(p);
        let mut afters: Vec<J> = vec![now];
        for st in steps.iter().skip(1).rev() {
            let next_pts = afters.last().and_then(|d| d.get("points")).cloned().unwrap_or(J::Null);
            let state: J = serde_json::from_str(&st.json).unwrap_or(J::Null);
            let pts = st.pts.as_ref().map_or(next_pts, |v| J::Array(v.iter().map(cc_civil::js::J::j).collect()));
            afters.push(json!({"state": state, "points": pts}));
        }
        afters.reverse();
        let mut out = vec![];
        for (st, b) in steps.iter().zip(&afters) {
            let state: J = serde_json::from_str(&st.json).unwrap_or(J::Null);
            let pts = st
                .pts
                .as_ref()
                .map_or_else(|| b.get("points").cloned().unwrap_or(J::Null), |v| J::Array(v.iter().map(cc_civil::js::J::j).collect()));
            let a = json!({"state": state, "points": pts});
            let g = g();
            if let Some((ops, inverse)) = CORE.step(&g, &a, b) {
                out.push((st.label.clone(), ops, inverse));
            }
        }
        out
    }
}
