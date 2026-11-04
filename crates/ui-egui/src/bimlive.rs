//! BimCraft in a Studio Live session (0.5 bimcraft/dev/live.js, LV.21): an IFC model is lines (#id = TYPE(params)); every
//! edit writes lines through the file (`set` / `remove`), and the file's write watch (cc-io-ifc `IfcFile::watch`) keeps
//! how each line was before its first write; when the edit is done the lines go out:
//!
//! - `{o:'line', id, t, p, n?}` the line as it is now (type + raw parameters) — key `e:<id>`: a new line (c, `n:1`) or a
//!   rewritten one (p, field `line`: the later one wins)
//! - `{o:'gone', id}` the line removed (d)
//!
//! The inverse is the same lines as they were before. New lines take ids from the client's own range
//! (1e9 + c × 1e7 + n), so two people's new lines never share an id; the file's own ids stay. The model shown is rebuilt
//! from the edited file (geometry only when a write needs it). A joiner opens the host's file from the snapshot.

use cc_io_ifc::value::Value as V;
use cc_live::doc::{Adapter, Cond, TouchReq};
use cc_live::msgpack::Value as Mp;
use serde_json::Value as J;

use crate::livestate::to_json;

pub const SCHEMA: u64 = 1;
const ID_BASE: f64 = 1e9;
const ID_BLOCK: f64 = 1e7;
/// Attribute positions an attribute edit touches on an element (Name, Description, ObjectType, Tag): no new geometry.
const ATTR_AT: [usize; 4] = [2, 3, 4, 7];
const MAX_DEPTH: usize = 64;

fn prop_type(t: &str) -> bool {
    matches!(
        t,
        "IFCPROPERTYSINGLEVALUE" | "IFCPROPERTYSET" | "IFCRELDEFINESBYPROPERTIES" | "IFCPROPERTYENUMERATEDVALUE" | "IFCELEMENTQUANTITY"
    ) || t.strip_prefix("IFCQUANTITY").is_some_and(|r| !r.is_empty() && r.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'))
}

/// cc-ifc's raw parameter → the wire value (live.js `enc`: DERIVED as `{derived: 1}`; MessagePack carries plain data).
pub fn enc(v: &V) -> Mp {
    match v {
        V::Null => Mp::Nil,
        V::Derived => Mp::map(vec![("derived", Mp::from(1u64))]),
        V::Num(n) => num(*n),
        V::Str(s) => Mp::Str(s.lossy().into_owned()),
        V::Ref(n) => Mp::map(vec![("ref", num(*n))]),
        V::Enum(e) => Mp::map(vec![("enum", Mp::Str(e.to_string()))]),
        V::Bin(b) => Mp::map(vec![("bin", Mp::Str(b.to_string()))]),
        V::Typed(t) => Mp::map(vec![("typed", Mp::Str(t.0.to_string())), ("value", enc(&t.1))]),
        V::List(l) => Mp::Arr(l.iter().map(enc).collect()),
    }
}
fn num(n: f64) -> Mp {
    if n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_991.0 {
        #[allow(clippy::cast_possible_truncation)]
        let i = n as i64;
        Mp::Int(i128::from(i))
    } else {
        Mp::F64(n)
    }
}
fn mp_num(v: &Mp) -> Option<f64> {
    match v {
        #[allow(clippy::cast_precision_loss)]
        Mp::Int(i) => Some(*i as f64),
        Mp::F64(f) => Some(*f),
        _ => None,
    }
}

/// The wire value → cc-ifc's raw parameter (live.js `dec`), None when it is no parameter value.
pub fn dec(v: &Mp, depth: usize) -> Option<V> {
    if depth > MAX_DEPTH {
        return None;
    }
    Some(match v {
        Mp::Nil => V::Null,
        Mp::Bool(b) => V::Num(if *b { 1.0 } else { 0.0 }),
        Mp::Int(_) | Mp::F64(_) => V::Num(mp_num(v)?),
        Mp::Str(s) => V::Str(cc_io_ifc::value::JsStr::Utf8(s.as_str().into())),
        Mp::Arr(a) => V::List(a.iter().map(|x| dec(x, depth + 1)).collect::<Option<Vec<_>>>()?.into_boxed_slice()),
        Mp::Map(m) => {
            let get = |k: &str| m.iter().find(|(x, _)| x == k).map(|(_, v)| v);
            if m.len() == 1 && get("derived").is_some() {
                V::Derived
            } else if let Some(r) = get("ref") {
                V::Ref(mp_num(r)?)
            } else if let Some(e) = get("enum").and_then(Mp::as_str) {
                V::Enum(e.into())
            } else if let Some(b) = get("bin").and_then(Mp::as_str) {
                V::Bin(b.into())
            } else {
                let t = get("typed").and_then(Mp::as_str)?;
                V::Typed(Box::new((t.into(), dec(get("value").unwrap_or(&Mp::Nil), depth + 1)?)))
            }
        }
        Mp::Bin(_) | Mp::Ext(..) => return None,
    })
}

fn same(a: &[V], b: &[V]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| enc(x) == enc(y))
}
fn line_op(id: f64, t: &str, p: &[V], new: bool) -> Mp {
    let mut m = vec![("o", Mp::from("line")), ("id", num(id)), ("t", Mp::Str(t.to_string())), ("p", Mp::Arr(p.iter().map(enc).collect()))];
    if new {
        m.push(("n", Mp::from(1u64)));
    }
    Mp::map(m)
}
fn gone_op(id: f64) -> Mp {
    Mp::map(vec![("o", Mp::from("gone")), ("id", num(id))])
}
fn line_now(f: &cc_io_ifc::model::IfcFile<'_>, id: f64) -> Option<(String, Vec<V>)> {
    let t = f.ty(id).filter(|t| !t.is_empty())?.to_string();
    let p = f.params(id).ok().flatten()?.to_vec();
    Some((t, p))
}

/// The lines an edit wrote (the watch's record of how each was before) → (ops, inverse), or None when nothing changed.
pub fn step_ops(f: &cc_io_ifc::model::IfcFile<'_>, log: &[cc_io_ifc::model::Watched]) -> Option<(Vec<Mp>, Vec<Mp>)> {
    let (mut ops, mut inv) = (vec![], vec![]);
    for (id, was) in log {
        let now = line_now(f, *id);
        match (was, &now) {
            (None, None) => continue,
            (Some((wt, wp)), Some((nt, np))) if wt == nt && same(wp, np) => continue,
            _ => {}
        }
        ops.push(match &now {
            Some((t, p)) => line_op(*id, t, p, was.is_none()),
            None => gone_op(*id),
        });
        inv.push(match was {
            Some((t, p)) => line_op(*id, t, p, now.is_none()),
            None => gone_op(*id),
        });
    }
    if ops.is_empty() {
        return None;
    }
    inv.reverse();
    Some((ops, inv))
}

fn op_s<'a>(op: &'a Mp, k: &str) -> &'a str {
    op.get(k).and_then(Mp::as_str).unwrap_or("")
}
fn op_id(op: &Mp) -> Option<f64> {
    op.get("id").and_then(mp_num).filter(|x| *x > 0.0 && x.fract() == 0.0 && x.is_finite())
}

/// live.js `needsGeometry(f, id, type, p)`: does writing this line change what the model looks like?
fn needs_geometry(f: &cc_io_ifc::model::IfcFile<'_>, id: f64, t: &str, p: &[V]) -> bool {
    if !f.has(id) {
        return !prop_type(t);
    }
    if prop_type(t) {
        return false;
    }
    let Some((ot, old)) = line_now(f, id) else { return true };
    if ot != t || old.len() != p.len() {
        return true;
    }
    old.iter().zip(p).enumerate().any(|(i, (a, b))| !ATTR_AT.contains(&i) && enc(a) != enc(b))
}

/// Others' lines into the file → whether the geometry needs rebuilding.
pub fn apply_ops(f: &mut cc_io_ifc::model::IfcFile<'_>, ops: &Mp) -> bool {
    let mut geometry = false;
    for op in ops.as_arr() {
        let Some(id) = op_id(op) else { continue };
        match op_s(op, "o") {
            "line" => {
                let t = op_s(op, "t").to_ascii_uppercase();
                let Some(V::List(p)) = op.get("p").and_then(|p| dec(p, 0)) else { continue };
                if !geometry {
                    geometry = needs_geometry(f, id, &t, &p);
                }
                let _ = f.set(id, &t, p.into_vec());
            }
            "gone" => {
                f.remove(id);
                geometry = true;
            }
            _ => {}
        }
    }
    geometry
}

/// live.js `invert(ops)`: the lines as they are now, last op first.
pub fn invert(f: &cc_io_ifc::model::IfcFile<'_>, ops: &Mp) -> Mp {
    let mut out: Vec<Mp> = vec![];
    for op in ops.as_arr().iter().rev() {
        let o = op_s(op, "o");
        if o == "g" {
            out.push(op.clone());
            continue;
        }
        if o != "line" && o != "gone" {
            continue;
        }
        let Some(id) = op_id(op) else { continue };
        out.push(match line_now(f, id) {
            Some((t, p)) => line_op(id, &t, &p, o == "gone"),
            None => gone_op(id),
        });
    }
    if let Some(gi) = out.iter().position(|o| op_s(o, "o") == "g")
        && gi > 0
    {
        let g = out.remove(gi);
        out.insert(0, g);
    }
    Mp::Arr(out)
}

/// live.js `touched(ops)`.
pub fn touched(ops: &Mp) -> Vec<TouchReq> {
    let mut out: Vec<TouchReq> = vec![];
    let mut seen = std::collections::HashSet::new();
    for op in ops.as_arr() {
        let Some(id) = op.get("id").and_then(mp_num) else { continue };
        let k = format!("e:{}", cc_detail::js::stringify(&crate::livestate::to_json(&num(id))));
        let (m, f) = match op_s(op, "o") {
            "line" if op.get("n").is_some_and(|n| !matches!(n, Mp::Nil | Mp::Bool(false))) => ('c', None),
            "line" => ('p', Some("line".to_string())),
            "gone" => ('d', None),
            _ => continue,
        };
        if seen.insert(format!("{k}{m}")) {
            out.push(TouchReq { k, m, f, r: Cond::Auto, g: false });
        }
    }
    out
}

/// live.js `validate(ops)`: every line an edit writes points only at lines that exist once it is applied.
pub fn validate(f: &cc_io_ifc::model::IfcFile<'_>, ops: &Mp) -> Option<String> {
    let mut born: Vec<f64> = vec![];
    let mut gone: Vec<f64> = vec![];
    for op in ops.as_arr() {
        let Some(id) = op.get("id").and_then(mp_num) else { continue };
        match op_s(op, "o") {
            "line" => born.push(id),
            "gone" => gone.push(id),
            _ => {}
        }
    }
    fn refs(v: &Mp, out: &mut Vec<f64>, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        match v {
            Mp::Arr(a) => a.iter().for_each(|x| refs(x, out, depth + 1)),
            Mp::Map(m) => {
                if let Some((_, r)) = m.iter().find(|(k, _)| k == "ref") {
                    out.push(mp_num(r).unwrap_or(f64::NAN));
                }
            }
            _ => {}
        }
    }
    for op in ops.as_arr() {
        if op_s(op, "o") != "line" {
            continue;
        }
        let t = op_s(op, "t");
        let ok_t = t.len() > 3 && t.starts_with("IFC") && t.bytes().skip(3).all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
        if !ok_t || !matches!(op.get("p"), Some(Mp::Arr(_))) || f.schema.entity(t).is_none() {
            return Some("invalid".into());
        }
        let mut rs = vec![];
        refs(op.get("p").unwrap_or(&Mp::Nil), &mut rs, 0);
        for r in rs {
            if gone.contains(&r) || (!born.contains(&r) && !f.has(r)) {
                return Some("invalid".into());
            }
        }
    }
    None
}

/// live.js `hash()`: every line as `id=TYPE[params]`, sorted (what the convergence checks compare).
pub fn hash_lines(f: &cc_io_ifc::model::IfcFile<'_>) -> Vec<String> {
    let mut lines: Vec<String> = f
        .ids()
        .into_iter()
        .filter_map(|id| {
            let (t, p) = line_now(f, id)?;
            let pj = to_json(&Mp::Arr(p.iter().map(enc).collect()));
            Some(format!("{}={t}{}", cc_detail::js::stringify(&to_json(&num(id))), cc_detail::js::stringify(&pj)))
        })
        .collect();
    lines.sort();
    lines
}

fn lo(c: u64) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let c = c as f64;
    ID_BASE + c * ID_BLOCK
}

/// live.js `claimIds()`: new lines number from this client's range (cc-ifc gives a new line maxId + 1). → the file's
/// own maximum before the claim (to give back after the session).
pub fn claim_ids(f: &mut cc_io_ifc::model::IfcFile<'_>, c: u64, true_max: f64) -> f64 {
    let l = lo(c);
    let mut m = l;
    for id in f.new_ids() {
        if id > l && id < l + ID_BLOCK && id > m {
            m = id;
        }
    }
    let t = true_max.max(if f.max_id >= l && f.max_id < l + ID_BLOCK { 0.0 } else { f.max_id });
    f.max_id = m;
    t
}

// ---------------------------------------------------------------------------------------------------------------------
// The window's side (liveapps.rs)

/// BimCraft's [`crate::live::LiveView`] over the window's IFC model (its editor made on first use).
pub struct BimLive<'a>(pub &'a mut crate::UiApp);

impl BimLive<'_> {
    /// The open model's file, its write watch on.
    fn file(&mut self) -> Option<&mut cc_io_ifc::model::IfcFile<'static>> {
        crate::bimedit::doc(self.0).ok()?;
        let d = self.0.bimh.ed.as_mut()?;
        let f = &mut d.ed.api.f;
        if f.watch.is_none() {
            f.watch = Some(vec![]);
        }
        Some(f)
    }
    /// Undo / Redo while live (bim.undo / bim.redo): the person's own steps.
    pub fn reverse(&mut self, undo: bool) -> Result<(), String> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(l) = crate::live::reverse(self, undo) {
            self.0.message = match l {
                Some(w) => cc_bim::draw::tr::t(if undo { "edit.undid.step" } else { "edit.redid.step" }, &[("what", &w)]),
                None => cc_bim::draw::tr::t0(if undo { "edit.undo.nothing" } else { "edit.redo.nothing" }),
            };
        }
        #[cfg(target_arch = "wasm32")]
        let _ = undo;
        Ok(())
    }
}

struct BimAdapter<'a, 'f> {
    /// the open model's file (None: a joiner before the snapshot opened one)
    f: Option<&'a mut cc_io_ifc::model::IfcFile<'f>>,
    c: u64,
    true_max: &'a mut f64,
    geometry: &'a mut Option<bool>,
    snapshot: Option<(String, Vec<u8>)>,
    opened: &'a mut Option<(String, Vec<u8>)>,
}

impl Adapter for BimAdapter<'_, '_> {
    fn apply(&mut self, ops: &Mp, _origin: &str) -> Result<(), String> {
        let Some(f) = self.f.as_deref_mut() else { return Err("live: no model open yet".into()) };
        let keep = f.watch.take();
        let g = apply_ops(f, ops);
        f.watch = keep.map(|_| vec![]);
        *self.true_max = self.true_max.max(f.max_id);
        if self.c > 0 {
            let l = lo(self.c);
            let mine = f.new_ids().into_iter().filter(|id| *id > l && *id < l + ID_BLOCK).fold(l, f64::max);
            f.max_id = mine;
        }
        *self.geometry = Some(self.geometry.unwrap_or(false) || g);
        Ok(())
    }
    fn invert(&mut self, ops: &Mp) -> Result<Mp, String> {
        let f = self.f.as_deref().ok_or("live: no model open yet")?;
        Ok(invert(f, ops))
    }
    fn touched(&self, ops: &Mp) -> Vec<TouchReq> {
        touched(ops)
    }
    fn snapshot(&mut self) -> Vec<u8> {
        let (name, bytes) = self.snapshot.take().unwrap_or_default();
        let v = Mp::map(vec![("v", Mp::from(SCHEMA)), ("name", Mp::Str(name)), ("bytes", Mp::Bin(bytes))]);
        cc_live::msgpack::pack(&v).unwrap_or_default()
    }
    fn load(&mut self, bytes: &[u8], _q: u64) -> Result<(), String> {
        let x = cc_live::msgpack::unpack(bytes).map_err(|e| e.0.to_string())?;
        if x.get("v").and_then(Mp::as_u64) != Some(SCHEMA) {
            return Err("live snapshot: unknown format".into());
        }
        let Some(Mp::Bin(b)) = x.get("bytes") else { return Err("live snapshot: unknown format".into()) };
        let name = x.get("name").and_then(Mp::as_str).filter(|n| !n.is_empty()).unwrap_or("model.ifc");
        *self.opened = Some((name.to_string(), b.clone()));
        Ok(())
    }
    fn validate(&mut self, ops: &Mp) -> Option<String> {
        self.f.as_deref().and_then(|f| validate(f, ops))
    }
}

impl crate::live::LiveView for BimLive<'_> {
    const APP: &'static str = "bimcraft";
    const PREFIX: &'static str = "bim";
    fn live_slot(&mut self) -> &mut Option<crate::live::LiveSession> {
        &mut self.0.live
    }
    fn live_window_open(&mut self) -> &mut bool {
        &mut self.0.live_window
    }
    fn with_adapter(&mut self, c: u64, f: &mut dyn FnMut(&mut dyn Adapter)) {
        // the snapshot a joiner may ask for: the edited file as Save writes it
        let ts = cc_bim::draw::editor::iso_seconds(self.0.bimh.now());
        let snap = self
            .0
            .bimh
            .ed
            .as_ref()
            .and_then(|d| d.ed.save(&ts).ok())
            .map(|b| (self.0.bimh.ed.as_ref().map(|d| d.name.clone()).unwrap_or_default(), b));
        let snap = snap.or_else(|| self.0.ifc3_bytes().map(|b| (self.0.ifc_name().to_string(), b.to_vec())));
        let mut geometry: Option<bool> = None;
        let mut opened: Option<(String, Vec<u8>)> = None;
        let mut true_max = self.0.live_bim_max;
        {
            let f_opt = self.file();
            let mut a = BimAdapter { f: f_opt, c, true_max: &mut true_max, geometry: &mut geometry, snapshot: snap, opened: &mut opened };
            f(&mut a);
        }
        self.0.live_bim_max = true_max;
        if let Some((name, bytes)) = opened {
            // a joiner opens the host's file; the editor is made on it with the watch on (a pick stays)
            let keep = self.0.picked_product();
            self.0.open_bytes(&name, &bytes);
            if let Some(id) = keep {
                self.0.select_product(id);
            }
            self.0.live_bim_ver = self.0.live_bim_ver.wrapping_add(1);
            let tm = self.0.live_bim_max;
            if let Some(file) = self.file() {
                let t = claim_ids(file, c, tm);
                self.0.live_bim_max = t;
            }
        } else if let Some(g) = geometry {
            self.0.live_bim_ver = self.0.live_bim_ver.wrapping_add(1);
            if let Some(d) = self.0.bimh.ed.as_mut() {
                d.ed.reindex();
            }
            // the model shown is made again from the file; what this window has picked stays picked (bimcraft.js refresh)
            let keep = self.0.picked_product();
            crate::bimedit::after_change(self.0, keep, g);
        }
    }
    fn claim_ids(&mut self, c: u64) {
        let tm = self.0.live_bim_max;
        if let Some(f) = self.file() {
            let t = claim_ids(f, c, tm);
            self.0.live_bim_max = t;
        }
    }
    fn release_ids(&mut self) {
        let tm = self.0.live_bim_max;
        if let Some(d) = self.0.bimh.ed.as_mut() {
            let f = &mut d.ed.api.f;
            f.watch = None;
            f.max_id = f.max_id.max(tm);
        }
        self.0.live_bim_max = 0.0;
    }
    fn live_bind(&mut self) {
        let _ = self.file();
    }
    fn model_version(&self) -> u64 {
        self.0.live_bim_ver
    }
    fn after_remote(&mut self) {}
    fn tr(&self, key: &str, vars: &[(&str, String)]) -> String {
        let v: Vec<(&str, &str)> = vars.iter().map(|(k, s)| (*k, s.as_str())).collect();
        cc_bim::draw::tr::t(key, &v)
    }
    fn say(&mut self, msg: String) {
        self.0.message = msg;
    }
    fn live_state(&mut self) -> Option<J> {
        let f = self.file()?;
        Some(J::Array(hash_lines(f).into_iter().map(J::String).collect()))
    }
    fn local_steps(&mut self, g: &mut dyn FnMut() -> String) -> Vec<(String, Mp, Mp)> {
        let label = self.0.bimh.ed.as_ref().and_then(|d| d.ed.changes.last()).map(|c| {
            let l = c.label();
            if l.is_empty() { c.kind().to_string() } else { l.to_string() }
        });
        let Some(f) = self.file() else { return vec![] };
        let log = f.watch.replace(vec![]).unwrap_or_default();
        if log.is_empty() {
            return vec![];
        }
        let Some((ops, inv)) = step_ops(f, &log) else { return vec![] };
        let mark = Mp::map(vec![("o", Mp::from("g")), ("g", Mp::Str(g()))]);
        let ops = Mp::Arr(std::iter::once(mark.clone()).chain(ops).collect());
        let inv = Mp::Arr(std::iter::once(mark).chain(inv).collect());
        vec![(label.unwrap_or_default(), ops, inv)]
    }
}
