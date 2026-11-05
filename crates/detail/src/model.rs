//! model.js — the DetailCraft model and its one edit path. The state is the JS's plain data ([`State`]: members,
//! connections and footings as compact records, the rest JSON; its text the JS's): every edit goes
//! through [`Model::change`] (JSON-text snapshots, 200 deep; an edit that fails changes nothing), the edit vocabulary is
//! [`Tx`], and [`read_model`] checks an untrusted file field by field (controls stripped, bad parts dropped with
//! warnings, ids made unique, a newer version refused). Millimetres, z up.

use crate::connections::defaults_of;
use crate::i18n::{F, t, t0};
use crate::js::{
    Res, arr, clean_text, hypot3, is_integer, num, num_or0, num_to_string, parse, round, s, spread, str_to_number, to_js_string, truthy,
    upper,
};
use crate::jv::{JObj, JStr, JVal, Jsv};
use crate::profiles::{clean_custom, get_profile, get_profile_str};
use crate::read_typed::{ConnSrc, MemberSrc, Src};
use crate::rebar::{clean_footing, footing_defaults};
use crate::stair::{clean_stair, stair_defaults};
use crate::state::{is_beam_end_v, members_of_v};
use crate::store::State;
use serde_json::{Map, Value, json};

pub const FORMAT: &str = "detailcraft";
pub const VERSION: f64 = 2.0;
pub const MAX_PHASE: f64 = 999.0;
pub const MEMBER_TYPES: [&str; 4] = ["beam", "column", "brace", "plate"];
pub const MATERIALS: [&str; 7] = ["A992", "A36", "A572 Gr 50", "A500 Gr C", "S235", "S275", "S355"];
const UNDO_DEPTH: usize = 200;
/// Caps on what an edit takes from its caller (the JS has none; a list this long is refused instead of walked).
const MAX_LIST: usize = 1_000_000;

// ---------------------------------------------------------------------------------------------------------------
// JS value helpers

/// `o.k` for any JS value (only objects have the fields asked for here).
#[inline]
pub fn field<'a, T: Jsv>(v: Option<&'a T>, k: &str) -> Option<&'a T::Item> {
    v.and_then(|v| v.js_get(k))
}
/// `a === b` (absent = undefined; objects never equal: they are distinct references here).
pub fn strict_eq<A: Jsv, B: Jsv>(a: Option<&A>, b: Option<&B>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => {
            if let (Some(p), Some(q)) = (x.js_str(), y.js_str()) {
                p == q
            } else if let (Some(p), Some(q)) = (x.js_f64(), y.js_f64()) {
                p == q
            } else if x.js_null() && y.js_null() {
                true
            } else if let (Some(p), Some(q)) = (x.js_bool(), y.js_bool()) {
                p == q
            } else {
                false
            }
        }
        _ => false,
    }
}
/// A value as `===` tells values apart (None: equal to nothing, as NaN or an object).
fn eq_key<T: Jsv>(v: Option<&T>) -> Option<String> {
    let Some(v) = v else { return Some("u".into()) };
    Some(if let Some(s) = v.js_str() {
        format!("s{s}")
    } else if let Some(f) = v.js_f64() {
        if f.is_nan() {
            return None;
        }
        format!("n{}", if f == 0.0 { 0.0f64 } else { f }.to_bits())
    } else if v.js_null() {
        "z".into()
    } else if v.js_bool()? {
        "b1".into()
    } else {
        "b0".into()
    })
}

/// What [`Tx::add_connection_in`] checks a new connection against: the beam ends, base plates, splices and gussets
/// taken, and the members (their types) — made once for a whole paste instead of scanning the lists for each.
#[derive(Debug, Default)]
pub struct ConnIndex {
    beam_ends: std::collections::HashSet<(String, String)>,
    bases: std::collections::HashSet<String>,
    splice_a_end: std::collections::HashSet<(String, String)>,
    splice_ab: std::collections::HashSet<(String, String)>,
    gussets: std::collections::HashSet<(String, String)>,
    /// member id → type of the first member with it
    members: std::collections::HashMap<String, String>,
}

impl ConnIndex {
    /// The index of a state's connections and members.
    pub fn of(state: &State) -> ConnIndex {
        let mut ix = ConnIndex::default();
        for m in state.list("members") {
            if let Some(k) = eq_key(m.get("id")) {
                ix.members.entry(k).or_insert_with(|| m.get("type").and_then(JVal::as_str).unwrap_or("\u{0}").to_string());
            }
        }
        for c in state.list("connections") {
            ix.learn(c);
        }
        ix
    }
    fn learn<T: Jsv>(&mut self, x: &T) {
        let k2 = |a: &str, b: &str| eq_key(x.js_get(a)).zip(eq_key(x.js_get(b)));
        if is_beam_end_v(x)
            && let Some(p) = k2("beam", "end")
        {
            self.beam_ends.insert(p);
        }
        match x.js_get("type").and_then(Jsv::js_str) {
            Some("baseplate") => {
                if let Some(k) = eq_key(x.js_get("column")) {
                    self.bases.insert(k);
                }
            }
            Some("splice") => {
                if let Some(p) = k2("a", "aEnd") {
                    self.splice_a_end.insert(p);
                }
                if let Some(p) = k2("a", "b") {
                    self.splice_ab.insert(p);
                }
            }
            Some("gusset") => {
                if let Some(p) = k2("brace", "end") {
                    self.gussets.insert(p);
                }
            }
            _ => {}
        }
    }
    fn pair(a: Option<&Value>, b: Option<&Value>) -> Option<(String, String)> {
        eq_key(a).zip(eq_key(b))
    }
    fn beam_end_taken(&self, beam: Option<&Value>, end: Option<&Value>) -> bool {
        Self::pair(beam, end).is_some_and(|p| self.beam_ends.contains(&p))
    }
    fn base_taken(&self, column: Option<&Value>) -> bool {
        eq_key(column).is_some_and(|k| self.bases.contains(&k))
    }
    fn splice_taken(&self, a: Option<&Value>, a_end: Option<&Value>, b: Option<&Value>) -> bool {
        Self::pair(a, a_end).is_some_and(|p| self.splice_a_end.contains(&p))
            || Self::pair(b, a).is_some_and(|p| self.splice_ab.contains(&p))
            || Self::pair(a, b).is_some_and(|p| self.splice_ab.contains(&p))
    }
    fn gusset_taken(&self, brace: Option<&Value>, end: Option<&Value>) -> bool {
        Self::pair(brace, end).is_some_and(|p| self.gussets.contains(&p))
    }
    fn member_type(&self, id: Option<&Value>) -> Option<&str> {
        eq_key(id).and_then(|k| self.members.get(&k)).map(String::as_str)
    }
}

/// `new Set(list).has(v)`.
fn set_has<T: Jsv>(list: &[Value], v: Option<&T>) -> bool {
    v.is_some_and(|v| list.iter().any(|x| strict_eq(Some(x), Some(v))))
}
/// `[...list].includes(v)` for a string list.
fn includes<T: Jsv>(list: &[&str], v: Option<&T>) -> bool {
    v.and_then(Jsv::js_str).is_some_and(|s| list.contains(&s))
}
/// `Array.isArray(v) ? v : []`, capped.
fn list_of<T: Jsv>(v: Option<&T>, cap: usize) -> &[T::Item] {
    let a = arr(v);
    a.get(..a.len().min(cap)).unwrap_or(a)
}
/// A record as an object to change (anything else becomes `{}` first, as `m_of`).
fn jm_of(v: &mut JVal) -> &mut JObj {
    if !v.is_object() {
        *v = JVal::Obj(JObj::new());
    }
    match v {
        JVal::Obj(m) => m,
        // replaced by an object just above
        _ => unreachable_jmap(),
    }
}
#[allow(clippy::unreachable)]
fn unreachable_jmap() -> &'static mut JObj {
    unreachable!("jm_of makes the value an object first")
}
/// A record's list field to change (made an array first, as `arr_mut`).
fn jarr_mut<'a>(m: &'a mut JObj, k: &str) -> &'a mut Vec<JVal> {
    if !m.get(k).is_some_and(JVal::is_array) {
        m.insert(k, JVal::Arr(Vec::new()));
    }
    match m.get_mut(k) {
        Some(JVal::Arr(a)) => a,
        _ => unreachable_jvec(),
    }
}
#[allow(clippy::unreachable)]
fn unreachable_jvec() -> &'static mut Vec<JVal> {
    unreachable!("jarr_mut makes the value an array first")
}
fn m_of(v: &mut Value) -> &mut Map<String, Value> {
    if !v.is_object() {
        *v = Value::Object(Map::new());
    }
    match v {
        Value::Object(m) => m,
        // replaced by an object just above
        _ => unreachable_map(),
    }
}
#[allow(clippy::unreachable)]
fn unreachable_map() -> &'static mut Map<String, Value> {
    unreachable!("m_of makes the value an object first")
}
/// `Object.entries(v)` for a value spread or walked as entries (objects; strings and arrays by index).
fn entries(v: Option<&Value>) -> Vec<(String, Value)> {
    spread(v, None).into_iter().collect()
}
/// `s[k] = v` on an object state.
fn put(s: &mut State, k: &str, v: Value) {
    s.insert(k, v);
}
fn n(x: f64) -> Value {
    Value::from(x)
}
/// A fill from a JS value: undefined and null leave the `{name}` in the text, as i18n.js `fill` does.
fn fill_of<'a, T: Jsv>(k: &'a str, v: Option<&T>) -> Option<(&'a str, F)> {
    match v {
        None => None,
        Some(x) if x.js_null() => None,
        Some(x) => Some((k, F::S(to_js_string(x)))),
    }
}

// ---------------------------------------------------------------------------------------------------------------

/// `emptyState(name)`.
pub fn empty_state(name: &str) -> State {
    State::from_value(json!({
        "format": FORMAT, "version": 2, "name": name,
        "grids": { "x": [], "y": [] }, "levels": [{ "name": t("model.level", &[("n", F::N(1.0))]), "z": 0 }],
        "members": [], "connections": [], "footings": [], "stairs": [], "bolts": [], "profiles": [], "seq": 1,
        "phases": [{ "n": 1, "name": "" }], "numbering": { "perPhase": true }, "issues": [],
    }))
}
/// `emptyState()` with the default name.
pub fn empty_state_default() -> State {
    empty_state(&t0("model.name.untitled"))
}
/// `cleanPhase(v)`.
pub fn clean_phase<T: Jsv>(v: Option<&T>) -> Option<f64> {
    let x = num(v);
    if is_integer(x) && (1.0..=MAX_PHASE).contains(&x) { Some(x) } else { None }
}
fn with_used_phases(phases: &[Value], state: &State) -> Vec<Value> {
    let mut out: Vec<Value> = phases.iter().map(|p| Value::Object(spread(Some(p), None))).collect();
    // each phase's `n`, beside it (a member's phase is looked up in these, not in the objects' maps)
    let mut ns: Vec<Option<Value>> = out.iter().map(|p| p.get("n").cloned()).collect();
    fn add<T: Jsv>(x: &T, out: &mut Vec<Value>, ns: &mut Vec<Option<Value>>) {
        let ph = x.js_get("phase");
        if !ns.iter().any(|n| strict_eq(n.as_ref(), ph)) {
            let mut o = Map::new();
            let v = ph.map(|v| v.js_value().into_owned());
            if let Some(v) = &v {
                o.insert("n".into(), v.clone());
            }
            o.insert("name".into(), Value::from(""));
            out.push(Value::Object(o));
            ns.push(v);
        }
    }
    for x in state.list("members").iter().chain(state.list("footings")) {
        add(x, &mut out, &mut ns);
    }
    for x in arr(state.get("stairs")) {
        add(x, &mut out, &mut ns);
    }
    out.sort_by(|a, b| crate::js::ord(num(a.get("n")) - num(b.get("n"))));
    out
}
/// `connPhase(state, c)`.
pub fn conn_phase<T: Jsv>(state: &State, c: &T) -> f64 {
    let main = members_of_v(c).first().copied().flatten();
    let m = state.list("members").iter().find(|m| strict_eq(m.get("id"), main));
    crate::js::or(m.map_or(f64::NAN, |m| crate::js::f(m.get("phase"))), 1.0)
}

/// `starterState(name)`.
pub fn starter_state(name: &str) -> State {
    let mut s = empty_state(name);
    if let Ok(g) = grids_from_spacings("1 2 3", "6000 6000", "A B", "6000") {
        put(&mut s, "grids", g);
    }
    put(
        &mut s,
        "levels",
        json!([{ "name": t("model.level", &[("n", F::N(1.0))]), "z": 0 }, { "name": t("model.level", &[("n", F::N(2.0))]), "z": 4000 }]),
    );
    s
}

fn split_ws_comma(s: &str) -> Vec<String> {
    s.split(|c: char| cc_cad::js::is_ws(c) || c == ',').filter(|x| !x.is_empty()).map(str::to_string).collect()
}
/// `gridsFromSpacings(xLabels, xSpacings, yLabels, ySpacings)`.
pub fn grids_from_spacings(xl: &str, xs: &str, yl: &str, ys: &str) -> Res<Value> {
    let axis = |labels: &str, spacings: &str, which: String| -> Res<Value> {
        let ls = split_ws_comma(labels);
        let ss: Vec<f64> = split_ws_comma(spacings).iter().map(|x| str_to_number(x)).collect();
        if ls.len() > 60 {
            return Err(t("model.err.grid-max", &[("max", F::N(60.0))]));
        }
        if !ls.is_empty() && ss.len() != ls.len().saturating_sub(1) {
            return Err(t(
                "model.err.grid-spacings",
                &[("which", F::from(which.as_str())), ("n", F::N(ls.len().saturating_sub(1) as f64))],
            ));
        }
        if ss.iter().any(|v| !v.is_finite() || *v <= 0.0 || *v > 200_000.0) {
            return Err(t("model.err.grid-positive", &[("which", F::from(which.as_str()))]));
        }
        let mut uniq: Vec<&String> = Vec::new();
        for l in &ls {
            if !uniq.contains(&l) {
                uniq.push(l);
            }
        }
        if uniq.len() != ls.len() {
            return Err(t("model.err.grid-labels", &[("which", F::from(which.as_str()))]));
        }
        let mut at = 0.0;
        let mut out = Vec::new();
        for (i, label) in ls.iter().enumerate() {
            if i > 0 {
                at += ss.get(i - 1).copied().unwrap_or(f64::NAN);
            }
            out.push(json!({ "label": crate::js::slice16(label, 8), "at": at }));
        }
        Ok(Value::Array(out))
    };
    let x = axis(xl, xs, t0("model.grids.numbered"))?;
    let y = axis(yl, ys, t0("model.grids.lettered"))?;
    Ok(json!({ "x": x, "y": y }))
}
/// `spacingsOf(lines)`.
pub fn spacings_of(lines: &[Value]) -> String {
    let mut out = Vec::new();
    for (i, g) in lines.iter().enumerate().skip(1) {
        let prev = lines.get(i - 1).map_or(f64::NAN, |p| num(p.get("at")));
        out.push(num_to_string(crate::js::fixed_num(num(g.get("at")) - prev, 3)));
    }
    out.join(" ")
}
/// `nodes(state)`.
pub fn nodes(state: &State) -> Vec<Value> {
    let mut out = Vec::new();
    let grids = state.get("grids");
    for lv in arr(state.get("levels")) {
        for gx in arr(field(grids, "x")) {
            for gy in arr(field(grids, "y")) {
                out.push(json!({ "p": [gx.get("at"), gy.get("at"), lv.get("z")], "gx": gx.get("label"), "gy": gy.get("label"), "level": lv.get("name") }));
            }
        }
    }
    out
}
/// `nodeName(state, p)`.
pub fn node_name(state: &State, p: [f64; 3]) -> String {
    let grids = state.get("grids");
    let gx = arr(field(grids, "x")).iter().find(|g| (num(g.get("at")) - p[0]).abs() < 0.5);
    let gy = arr(field(grids, "y")).iter().find(|g| (num(g.get("at")) - p[1]).abs() < 0.5);
    let lv = arr(state.get("levels")).iter().find(|l| (num(l.get("z")) - p[2]).abs() < 0.5);
    let xy = match (gx, gy) {
        (Some(gx), Some(gy)) => format!("{}/{}", crate::js::string_of(gy.get("label")), crate::js::string_of(gx.get("label"))),
        _ => format!("({}, {})", num_to_string(round(p[0])), num_to_string(round(p[1]))),
    };
    match lv {
        Some(lv) => {
            let mut vars = vec![("xy", F::from(xy))];
            vars.extend(fill_of("level", lv.get("name")));
            t("model.node.at", &vars)
        }
        None => t("model.node.atz", &[("xy", F::from(xy)), ("z", F::N(round(p[2])))]),
    }
}
/// `pointAt(state, gy, gx, level)`.
pub fn point_at(state: &State, gy: &Value, gx: &Value, level: &Value) -> Option<[f64; 3]> {
    let grids = state.get("grids");
    let x = arr(field(grids, "x")).iter().find(|g| strict_eq(g.get("label"), Some(gx)))?;
    let y = arr(field(grids, "y")).iter().find(|g| strict_eq(g.get("label"), Some(gy)))?;
    let l = arr(state.get("levels")).iter().find(|l| strict_eq(l.get("name"), Some(level)))?;
    Some([num(x.get("at")), num(y.get("at")), num(l.get("z"))])
}
/// `defaultsFor(type)`.
pub fn defaults_for(kind: &str) -> (&'static str, &'static str, &'static str) {
    match kind {
        "column" => ("W12x50", "A992", "middle"),
        "brace" => ("HSS6x6x1/4", "A500 Gr C", "middle"),
        "plate" => ("PL20x200", "A36", "middle"),
        _ => ("W16x26", "A992", "top"),
    }
}

/// One undo / redo step.
#[derive(Clone, Debug)]
pub struct Step {
    pub label: String,
    pub json: String,
}

/// `new Model(state)`.
#[derive(Clone, Debug)]
pub struct Model {
    pub state: State,
    pub undo_stack: Vec<Step>,
    pub redo_stack: Vec<Step>,
    pub version: u64,
    pub saved_version: u64,
}
impl Default for Model {
    fn default() -> Self {
        Model::new(starter_state(&t0("model.name.untitled")))
    }
}
impl Model {
    pub fn new(state: State) -> Model {
        Model { state, undo_stack: vec![], redo_stack: vec![], version: 0, saved_version: 0 }
    }
    pub fn dirty(&self) -> bool {
        self.version != self.saved_version
    }
    pub fn mark_saved(&mut self) {
        self.saved_version = self.version;
    }
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
    /// One undo step: `f(tx)` edits through the Tx; an error restores the state and is returned.
    pub fn change<T>(&mut self, label: &str, f: impl FnOnce(&mut Tx) -> Res<T>) -> Res<T> {
        let before = self.state.stringify();
        let result = {
            let mut tx = Tx { s: &mut self.state };
            f(&mut tx)
        };
        let result = match result {
            Ok(r) => r,
            Err(e) => {
                self.state = State::parse(&before).unwrap_or_else(|_| State::from_value(Value::Null));
                return Err(e);
            }
        };
        let after = self.state.stringify();
        if after == before {
            return Ok(result);
        }
        self.undo_stack.push(Step { label: label.to_string(), json: before });
        if self.undo_stack.len() > UNDO_DEPTH {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
        self.version = self.version.saturating_add(1);
        Ok(result)
    }
    pub fn undo(&mut self) -> Option<String> {
        let s = self.undo_stack.pop()?;
        self.redo_stack.push(Step { label: s.label.clone(), json: self.state.stringify() });
        self.state = State::parse(&s.json).unwrap_or_else(|_| State::from_value(Value::Null));
        self.version = self.version.saturating_add(1);
        Some(s.label)
    }
    pub fn redo(&mut self) -> Option<String> {
        let s = self.redo_stack.pop()?;
        self.undo_stack.push(Step { label: s.label.clone(), json: self.state.stringify() });
        self.state = State::parse(&s.json).unwrap_or_else(|_| State::from_value(Value::Null));
        self.version = self.version.saturating_add(1);
        Some(s.label)
    }
    /// `toJSON()` = `JSON.stringify(state, null, 1)`.
    pub fn to_json(&self) -> String {
        self.state.stringify_indent(1)
    }
    /// `load(text)` → warnings.
    pub fn load(&mut self, text: &Value) -> Res<Vec<String>> {
        let (state, warnings) = read_model(text)?;
        self.state = state;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.version = self.version.saturating_add(1);
        self.saved_version = self.version;
        Ok(warnings)
    }
    pub fn reset(&mut self, state: State) {
        self.state = state;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.version = self.version.saturating_add(1);
        self.saved_version = self.version;
    }
}

/// The edit vocabulary; each checks its input so a bad edit fails before anything is kept.
pub struct Tx<'a> {
    pub s: &'a mut State,
}

fn vec3(p: Option<&Value>) -> Res<Value> {
    let a = match p {
        Some(Value::Array(a)) if a.len() == 3 => a,
        _ => return Err(t0("model.err.point")),
    };
    let v = [num(a.first()), num(a.get(1)), num(a.get(2))];
    if v.iter().any(|x| !x.is_finite() || x.abs() > 1e7) {
        return Err(t0("model.err.coords"));
    }
    Ok(Value::Array(v.into_iter().map(n).collect()))
}
/// [`vec3`]'s point as numbers.
fn vec3_p<T: Jsv>(p: Option<&T>) -> Res<[f64; 3]> {
    let a = match p.and_then(Jsv::js_arr) {
        Some(a) if a.len() == 3 => a,
        _ => return Err(t0("model.err.point")),
    };
    let v = [num(a.first()), num(a.get(1)), num(a.get(2))];
    if v.iter().any(|x| !x.is_finite() || x.abs() > 1e7) {
        return Err(t0("model.err.coords"));
    }
    Ok(v)
}
/// `checkMember(m, custom)` on a member object (fixes material and rotation in place).
fn check_member(m: &mut Map<String, Value>, custom: &[Value]) -> Res<()> {
    let prof = m.get("profile").cloned().unwrap_or(Value::Null);
    if get_profile(&prof, custom).is_none() {
        let vars: Vec<(&str, F)> = fill_of("name", m.get("profile")).into_iter().collect();
        return Err(t("model.err.profile-unknown", &vars));
    }
    let mat = m.get("material");
    if !includes(&MATERIALS, mat) && clean_text(mat, 30).is_empty() {
        return Err(t0("model.err.material"));
    }
    let cm = clean_text(mat, 30);
    m.insert("material".into(), Value::from(cm));
    if !includes(&["top", "middle", "bottom"], m.get("position")) {
        return Err(t0("model.err.position"));
    }
    let rot = match m.get("rotation") {
        Some(Value::Number(x)) => x.as_f64().unwrap_or(f64::NAN),
        _ => f64::NAN,
    };
    if !rot.is_finite() {
        return Err(t0("model.err.rotation"));
    }
    m.insert("rotation".into(), n(((rot % 360.0) + 360.0) % 360.0));
    let st = crate::js::arr(m.get("start"));
    let en = crate::js::arr(m.get("end"));
    let g = |a: &[Value], i: usize| num(a.get(i));
    let (dx, dy, dz) = (g(en, 0) - g(st, 0), g(en, 1) - g(st, 1), g(en, 2) - g(st, 2));
    if hypot3(dx, dy, dz) < 1.0 {
        return Err(t0("model.err.same-points"));
    }
    Ok(())
}
/// `cleanText` of a JS value.
pub fn clean(v: Option<&Value>, max: usize) -> String {
    clean_text(v, max)
}
fn regex_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10 && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() })
}
fn regex_hash(s: &str) -> bool {
    s.len() == 8 && s.bytes().all(|c| c.is_ascii_hexdigit())
}
fn regex_rev(s: &str) -> bool {
    !s.is_empty() && s.len() <= 5 && s.bytes().all(|c| c.is_ascii_uppercase())
}
fn regex_part_key(k: &str) -> bool {
    let rest = k.strip_prefix("part:").or_else(|| k.strip_prefix("footing:"));
    rest.is_some_and(|r| {
        let n = r.encode_utf16().count();
        (1..=80).contains(&n) && r.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == ':' || c == '-')
    })
}
/// `cleanBolt(b)`.
pub fn clean_bolt(b: &Value) -> Option<Map<String, Value>> {
    if !(b.is_object() || b.is_array()) {
        return None;
    }
    let bo = Some(b);
    let d = num(field(bo, "d"));
    let l = num(field(bo, "length"));
    let head: Vec<f64> = arr(field(bo, "head")).iter().map(|x| num(Some(x))).collect();
    let ax: Vec<f64> = arr(field(bo, "axis")).iter().map(|x| num(Some(x))).collect();
    if !(3.0..=100.0).contains(&d)
        || !(5.0..=5000.0).contains(&l)
        || head.len() != 3
        || ax.len() != 3
        || head.iter().chain(&ax).any(|v| !v.is_finite())
        || head.iter().any(|v| v.abs() > 1e7)
    {
        return None;
    }
    let nn = crate::js::hypot_n(&ax);
    if nn < 1e-9 {
        return None;
    }
    let r = |v: f64| round(v * 1000.0) / 1000.0;
    let mut o = Map::new();
    o.insert("d".into(), n(r(d)));
    o.insert("length".into(), n(r(l)));
    o.insert("grade".into(), Value::from(clean_text(field(bo, "grade"), 20)));
    o.insert(
        "kind".into(),
        Value::from(if field(bo, "kind").and_then(Value::as_str) == Some("ANCHORBOLT") { "ANCHORBOLT" } else { "BOLT" }),
    );
    o.insert("head".into(), Value::Array(head.iter().map(|&v| n(r(v))).collect()));
    o.insert("axis".into(), Value::Array(ax.iter().map(|&v| n(round(v / nn * 1e6) / 1e6)).collect()));
    o.insert("phase".into(), n(clean_phase(field(bo, "phase")).unwrap_or(1.0)));
    Some(o)
}
/// `cleanIssue(x)`.
pub fn clean_issue(x: &Value) -> Option<Map<String, Value>> {
    if !(x.is_object() || x.is_array()) {
        return None;
    }
    let xo = Some(x);
    let sheet = clean_text(field(xo, "sheet"), 120);
    let number = clean_text(field(xo, "number"), 30);
    let rev = upper(&clean_text(field(xo, "rev"), 5));
    let ds = crate::js::string_of(field(xo, "date"));
    let date = if regex_date(&ds) { ds } else { String::new() };
    let hs = crate::js::string_of(field(xo, "hash"));
    let hash = if regex_hash(&hs) { upper(&hs) } else { String::new() };
    if sheet.is_empty() || number.is_empty() || !regex_rev(&rev) || hash.is_empty() {
        return None;
    }
    let mut out = Map::new();
    out.insert("sheet".into(), Value::from(sheet));
    out.insert("number".into(), Value::from(number));
    out.insert("rev".into(), Value::from(rev));
    out.insert("date".into(), Value::from(date));
    out.insert("note".into(), Value::from(clean_text(field(xo, "note"), 60)));
    out.insert("hash".into(), Value::from(hash));
    if let Some(Value::Object(parts)) = field(xo, "parts") {
        let mut p = Map::new();
        for (k, v) in parts.iter().take(5000) {
            let vs = to_js_string(v);
            if regex_part_key(k) && regex_hash(&vs) {
                p.insert(k.clone(), Value::from(upper(&vs)));
            }
        }
        out.insert("parts".into(), Value::Object(p));
    }
    Some(out)
}
fn clean_hole_s<S: Src>(h: &S) -> Option<JObj> {
    if !h.truthy() {
        return None;
    }
    if !includes(&["web", "top", "bottom", "plate"], h.get("face")) {
        return None;
    }
    let (x, y, d) = (num(h.get("x")), num(h.get("y")), num(h.get("d")));
    if ![x, y, d].iter().all(|v| v.is_finite()) || d <= 0.0 || d > 200.0 || x.abs() > 1e6 || y.abs() > 1e4 {
        return None;
    }
    let mut o = JObj::with_capacity(5);
    o.0.push((JStr::S("face"), h.get("face").map_or(JVal::Null, Jsv::js_jval)));
    o.0.push((JStr::S("x"), JVal::num(x)));
    o.0.push((JStr::S("y"), JVal::num(y)));
    o.0.push((JStr::S("d"), JVal::num(d)));
    Some(o)
}
fn clean_cut_s<S: Src>(c: &S) -> Option<JObj> {
    if !c.truthy() {
        return None;
    }
    if c.get("kind").and_then(Jsv::js_str) != Some("cope")
        || !includes(&["start", "end"], c.get("end"))
        || !includes(&["top", "bottom"], c.get("side"))
    {
        return None;
    }
    let (length, depth) = (num(c.get("length")), num(c.get("depth")));
    if !(length > 0.0 && length < 1e5 && depth > 0.0 && depth < 5000.0) {
        return None;
    }
    let mut o = JObj::with_capacity(6);
    o.0.push((JStr::S("kind"), JVal::Str(JStr::S("cope"))));
    o.0.push((JStr::S("end"), c.get("end").map_or(JVal::Null, Jsv::js_jval)));
    o.0.push((JStr::S("side"), c.get("side").map_or(JVal::Null, Jsv::js_jval)));
    o.0.push((JStr::S("length"), JVal::num(length)));
    o.0.push((JStr::S("depth"), JVal::num(depth)));
    Some(o)
}

impl Tx<'_> {
    fn st(&mut self) -> &mut State {
        self.s.obj_mut()
    }
    /// A field's list that is not one of the records' (`Array.isArray(s[k]) ? s[k] : []`).
    fn list(&self, k: &str) -> &[Value] {
        arr(self.s.get(k))
    }
    /// The members, connections or footings.
    fn recs(&self, k: &str) -> &[JVal] {
        self.s.list(k)
    }
    fn profiles(&self) -> Vec<Value> {
        self.list("profiles").to_vec()
    }
    /// `nextId(prefix)`.
    pub fn next_id(&mut self, prefix: &str) -> String {
        let st = self.st();
        let x = num(st.get("seq").or(Some(&Value::Null)));
        // `this.s.seq++`: the old value as a number, then one more
        st.insert("seq", n(x + 1.0));
        format!("{prefix}{}", num_to_string(x))
    }
    pub fn set_name(&mut self, name: Option<&Value>) {
        let c = clean_text(name, 80);
        let v = if c.is_empty() { t0("model.name.untitled") } else { c };
        self.st().insert("name", Value::from(v));
    }
    pub fn set_grids(&mut self, grids: Option<&Value>) -> Res<()> {
        let x = field(grids, "x").and_then(Value::as_array).ok_or_else(|| "grids.x.map is not a function".to_string())?;
        let y = field(grids, "y").and_then(Value::as_array).ok_or_else(|| "grids.y.map is not a function".to_string())?;
        let cp = |a: &Vec<Value>| Value::Array(a.iter().take(MAX_LIST).map(|g| Value::Object(spread(Some(g), None))).collect());
        let g = json!({ "x": cp(x), "y": cp(y) });
        self.st().insert("grids", g);
        Ok(())
    }
    pub fn set_levels(&mut self, levels: Option<&Value>) -> Res<()> {
        let ls = levels.and_then(Value::as_array).ok_or_else(|| "levels.map is not a function".to_string())?;
        if ls.is_empty() {
            return Err(t0("model.err.level-keep"));
        }
        let mut names: Vec<String> = Vec::new();
        let mut out = Vec::new();
        for l in ls.iter().take(MAX_LIST) {
            if l.is_null() {
                return Err("Cannot read properties of null (reading 'name')".into());
            }
            let name = clean_text(field(Some(l), "name"), 40);
            let z = num(field(Some(l), "z"));
            if name.is_empty() {
                return Err(t0("model.err.level-name"));
            }
            if names.contains(&name) {
                return Err(t("model.err.level-twice", &[("name", F::from(name.as_str()))]));
            }
            if !z.is_finite() || z.abs() > 1e6 {
                return Err(t0("model.err.level-z"));
            }
            names.push(name.clone());
            out.push(json!({ "name": name, "z": z }));
        }
        out.sort_by(|a, b| crate::js::ord(num(a.get("z")) - num(b.get("z"))));
        self.st().insert("levels", Value::Array(out));
        Ok(())
    }
    /// `ensurePhase(n)`.
    pub fn ensure_phase(&mut self, ph: f64) {
        let st = self.st();
        if !st.get("phases").is_some_and(Value::is_array) {
            st.insert("phases", json!([{ "n": 1, "name": "" }]));
        }
        if !truthy(st.get("numbering")) {
            st.insert("numbering", json!({ "perPhase": true }));
        }
        let phases = st.arr_mut("phases");
        if !phases.iter().any(|p| num(p.get("n")) == ph && p.get("n").is_some_and(Value::is_number)) {
            phases.push(json!({ "n": ph, "name": "" }));
            phases.sort_by(|a, b| crate::js::ord(num(a.get("n")) - num(b.get("n"))));
        }
    }
    pub fn add_phase(&mut self, name: Option<&Value>) -> Res<f64> {
        let mut mx = 0.0;
        for p in self.list("phases") {
            mx = crate::js::max(mx, num(p.get("n")));
        }
        let nn = mx + 1.0;
        if nn > MAX_PHASE {
            return Err(t("phase.err.max", &[("max", F::N(MAX_PHASE))]));
        }
        let nm = clean_text(Some(name.unwrap_or(&Value::from(""))), 40);
        self.st().arr_mut("phases").push(json!({ "n": nn, "name": nm }));
        Ok(nn)
    }
    pub fn rename_phase(&mut self, ph: Option<&Value>, name: Option<&Value>) -> Res<()> {
        let nm = clean_text(name, 40);
        let phases = self.st().arr_mut("phases");
        let p = phases.iter_mut().find(|x| strict_eq(x.get("n"), ph)).ok_or_else(|| t0("phase.err.gone"))?;
        m_of(p).insert("name".into(), Value::from(nm));
        Ok(())
    }
    pub fn remove_phase(&mut self, ph: Option<&Value>) -> Res<()> {
        let used = self.recs("members").iter().chain(self.recs("footings")).filter(|x| strict_eq(x.get("phase"), ph)).count()
            + self.list("stairs").iter().filter(|x| strict_eq(x.get("phase"), ph)).count();
        if used > 0 {
            let mut vars = vec![("n", F::N(used as f64))];
            vars.extend(fill_of("phase", ph));
            return Err(t("phase.err.used", &vars));
        }
        if self.list("phases").len() <= 1 {
            return Err(t0("phase.err.last"));
        }
        self.st().arr_mut("phases").retain(|p| !strict_eq(p.get("n"), ph));
        Ok(())
    }
    pub fn set_numbering(&mut self, o: Option<&Value>) {
        let pp = truthy(field(o, "perPhase"));
        self.st().insert("numbering", json!({ "perPhase": pp }));
    }
    pub fn add_issues(&mut self, list: Option<&Value>) -> Res<()> {
        let items = list.and_then(Value::as_array).cloned().unwrap_or_default();
        let st = self.st();
        if !st.get("issues").is_some_and(Value::is_array) {
            st.insert("issues", json!([]));
        }
        for x in items.iter().take(MAX_LIST) {
            let c = clean_issue(x).ok_or_else(|| t0("issue.err.bad"))?;
            self.st().arr_mut("issues").push(Value::Object(c));
        }
        Ok(())
    }
    pub fn set_phase_of(&mut self, refs: Option<&Value>, ph: Option<&Value>) -> Res<()> {
        let p = clean_phase(ph).ok_or_else(|| t("phase.err.number", &[("max", F::N(MAX_PHASE))]))?;
        self.ensure_phase(p);
        let ids: Vec<String> = arr(refs)
            .iter()
            .map(|r| format!("{}:{}", crate::js::string_of(field(Some(r), "kind")), crate::js::string_of(field(Some(r), "id"))))
            .collect();
        let st = self.st();
        for (key, pre) in [("members", "member:"), ("footings", "footing:")] {
            if let Some(list) = st.held_mut(key) {
                for x in list.iter_mut() {
                    let k = format!("{pre}{}", crate::js::string_of(x.get("id")));
                    if ids.contains(&k) {
                        jm_of(x).insert("phase", JVal::num(p));
                    }
                }
            }
        }
        if let Some(Value::Array(list)) = st.get_mut("stairs") {
            for x in list.iter_mut() {
                let k = format!("stair:{}", crate::js::string_of(x.get("id")));
                if ids.contains(&k) {
                    m_of(x).insert("phase".into(), n(p));
                }
            }
        }
        Ok(())
    }

    pub fn add_member(&mut self, m: Option<&Value>) -> Res<Value> {
        let ty = if includes(&MEMBER_TYPES, field(m, "type")) {
            field(m, "type").and_then(Value::as_str).unwrap_or("beam").to_string()
        } else {
            "beam".into()
        };
        let (dp, dmat, dpos) = defaults_for(&ty);
        let id = self.next_id("m");
        let or_d = |k: &str, d: &str| match field(m, k) {
            Some(v) if truthy(Some(v)) => v.clone(),
            _ => Value::from(d),
        };
        let start = vec3(field(m, "start"))?;
        let end = vec3(field(m, "end"))?;
        let rot = {
            let v = match field(m, "rotation") {
                None | Some(Value::Null) => 0.0,
                Some(v) => num(Some(v)),
            };
            if v == 0.0 || v.is_nan() { 0.0 } else { v }
        };
        let name = clean_text(Some(crate::js::nullish(field(m, "name"), &Value::from(""))), 40);
        let phase = clean_phase(field(m, "phase")).unwrap_or(1.0);
        let mut mem = Map::new();
        mem.insert("id".into(), Value::from(id.clone()));
        mem.insert("type".into(), Value::from(ty));
        mem.insert("profile".into(), or_d("profile", dp));
        mem.insert("material".into(), or_d("material", dmat));
        mem.insert("start".into(), start);
        mem.insert("end".into(), end);
        mem.insert("rotation".into(), n(rot));
        mem.insert("position".into(), or_d("position", dpos));
        mem.insert("name".into(), Value::from(name));
        mem.insert("cuts".into(), json!([]));
        mem.insert("holes".into(), json!([]));
        mem.insert("phase".into(), n(phase));
        let custom = self.profiles();
        check_member(&mut mem, &custom)?;
        self.ensure_phase(phase);
        self.st().members_mut().push(JVal::Obj(JObj::from_map_owned(mem)));
        Ok(Value::from(id))
    }
    pub fn update_member(&mut self, id: Option<&Value>, patch: Option<&Value>) -> Res<()> {
        let custom = self.profiles();
        let idx = self.recs("members").iter().position(|x| strict_eq(x.get("id"), id)).ok_or_else(|| t0("model.err.member-gone"))?;
        let cur = self.recs("members").get(idx).map_or(Value::Null, JVal::to_value);
        let mut next = spread(Some(&cur), patch);
        if truthy(field(patch, "start")) {
            next.insert("start".into(), vec3(field(patch, "start"))?);
        }
        if truthy(field(patch, "end")) {
            next.insert("end".into(), vec3(field(patch, "end"))?);
        }
        if field(patch, "name").is_some() {
            next.insert("name".into(), Value::from(clean_text(field(patch, "name"), 40)));
        }
        if field(patch, "rotation").is_some() {
            next.insert("rotation".into(), n(num(field(patch, "rotation"))));
        }
        let mut ph = None;
        if field(patch, "phase").is_some() {
            let p = clean_phase(field(patch, "phase"));
            match p {
                Some(p) => {
                    next.insert("phase".into(), n(p));
                    ph = Some(p);
                }
                None => {
                    return Err(t("phase.err.number", &[("max", F::N(MAX_PHASE))]));
                }
            }
        }
        if let Some(p) = ph {
            self.ensure_phase(p);
        }
        check_member(&mut next, &custom)?;
        if let Some(list) = self.st().held_mut("members")
            && let Some(m) = list.get_mut(idx)
        {
            let mm = jm_of(m);
            for (k, v) in next {
                mm.insert(JStr::from_string(k), JVal::from(v));
            }
        }
        Ok(())
    }
    pub fn remove_members(&mut self, ids: Option<&Value>) {
        let set = arr(ids).to_vec();
        let st = self.st();
        st.members_mut().retain(|m| !set_has(&set, m.get("id")));
        st.connections_mut().retain(|c| !members_of_v(c).iter().any(|id| set_has(&set, *id)));
    }
    pub fn add_hole(&mut self, member: Option<&Value>, h: Option<&Value>) -> Res<Value> {
        let idx = self.recs("members").iter().position(|x| strict_eq(x.get("id"), member)).ok_or_else(|| t0("model.err.member-gone"))?;
        let mut hole = h.and_then(clean_hole_s).ok_or_else(|| t0("model.err.hole"))?;
        let id = self.next_id("h");
        hole.insert("id", JVal::from(id.clone()));
        if let Some(list) = self.st().held_mut("members")
            && let Some(m) = list.get_mut(idx)
        {
            jarr_mut(jm_of(m), "holes").push(JVal::Obj(hole));
        }
        Ok(Value::from(id))
    }
    pub fn remove_hole(&mut self, member: Option<&Value>, hole: Option<&Value>) {
        if let Some(list) = self.st().held_mut("members")
            && let Some(m) = list.iter_mut().find(|x| strict_eq(x.get("id"), member))
        {
            jarr_mut(jm_of(m), "holes").retain(|h| !strict_eq(h.get("id"), hole));
        }
    }
    pub fn add_cut(&mut self, member: Option<&Value>, c: Option<&Value>) -> Res<Value> {
        let idx = self.recs("members").iter().position(|x| strict_eq(x.get("id"), member)).ok_or_else(|| t0("model.err.member-gone"))?;
        let mut cut = c.and_then(clean_cut_s).ok_or_else(|| t0("model.err.cope"))?;
        let id = self.next_id("k");
        cut.insert("id", JVal::from(id.clone()));
        if let Some(list) = self.st().held_mut("members")
            && let Some(m) = list.get_mut(idx)
        {
            jarr_mut(jm_of(m), "cuts").push(JVal::Obj(cut));
        }
        Ok(Value::from(id))
    }
    pub fn remove_cut(&mut self, member: Option<&Value>, cut: Option<&Value>) {
        if let Some(list) = self.st().held_mut("members")
            && let Some(m) = list.iter_mut().find(|x| strict_eq(x.get("id"), member))
        {
            jarr_mut(jm_of(m), "cuts").retain(|h| !strict_eq(h.get("id"), cut));
        }
    }

    pub fn add_connection(&mut self, c: Option<&Value>) -> Res<Value> {
        self.add_connection_in(c, None)
    }

    /// [`Tx::add_connection`] checking against `idx` (what is taken, the members) instead of scanning the lists: the same
    /// decisions, for pasting thousands at once; `idx` learns the new one.
    pub fn add_connection_in(&mut self, c: Option<&Value>, idx: Option<&mut ConnIndex>) -> Res<Value> {
        let id = self.next_id("j");
        let ty = field(c, "type");
        let params_with = |kind: &str| -> Value {
            let base = defaults_of(kind).unwrap_or(Value::Null);
            let extra = field(c, "params").filter(|v| truthy(Some(v)));
            Value::Object(spread(Some(&base), extra))
        };
        let conns = self.recs("connections");
        let mut o = Map::new();
        let cv = c.cloned().unwrap_or(Value::Null);
        if is_beam_end_v(&cv) {
            let taken = match idx.as_deref() {
                Some(ix) => ix.beam_end_taken(field(c, "beam"), field(c, "end")),
                None => conns
                    .iter()
                    .any(|x| is_beam_end_v(x) && strict_eq(x.get("beam"), field(c, "beam")) && strict_eq(x.get("end"), field(c, "end"))),
            };
            if taken {
                return Err(t0("model.err.end-taken"));
            }
            let kind = ty.and_then(Value::as_str).unwrap_or("");
            o.insert("id".into(), Value::from(id.clone()));
            o.insert("type".into(), Value::from(kind));
            if let Some(b) = field(c, "beam") {
                o.insert("beam".into(), b.clone());
            }
            o.insert("end".into(), Value::from(if field(c, "end").and_then(Value::as_str) == Some("start") { "start" } else { "end" }));
            if let Some(b) = field(c, "column") {
                o.insert("column".into(), b.clone());
            }
            o.insert("params".into(), params_with(kind));
        } else if ty.and_then(Value::as_str) == Some("baseplate") {
            let taken = match idx.as_deref() {
                Some(ix) => ix.base_taken(field(c, "column")),
                None => conns
                    .iter()
                    .any(|x| x.get("type").and_then(JVal::as_str) == Some("baseplate") && strict_eq(x.get("column"), field(c, "column"))),
            };
            if taken {
                return Err(t0("model.err.base-taken"));
            }
            o.insert("id".into(), Value::from(id.clone()));
            o.insert("type".into(), Value::from("baseplate"));
            if let Some(b) = field(c, "column") {
                o.insert("column".into(), b.clone());
            }
            o.insert("params".into(), params_with("baseplate"));
        } else if ty.and_then(Value::as_str) == Some("splice") {
            let ms = self.recs("members");
            let (a, b) = (field(c, "a"), field(c, "b"));
            let has_m = |v: Option<&Value>| match idx.as_deref() {
                Some(ix) => ix.member_type(v).is_some(),
                None => ms.iter().any(|m| strict_eq(m.get("id"), v)),
            };
            if !has_m(a) || !has_m(b) || strict_eq(a, b) {
                return Err(t0("model.err.conn-gone"));
            }
            let a_end = if field(c, "aEnd").and_then(Value::as_str) == Some("start") { "start" } else { "end" };
            let a_end_v = Value::from(a_end);
            let taken = match idx.as_deref() {
                Some(ix) => ix.splice_taken(a, Some(&a_end_v), b),
                None => conns.iter().any(|x| {
                    x.get("type").and_then(JVal::as_str) == Some("splice")
                        && ((strict_eq(x.get("a"), a) && strict_eq(x.get("aEnd"), Some(&a_end_v)))
                            || (strict_eq(x.get("a"), b) && strict_eq(x.get("b"), a))
                            || (strict_eq(x.get("a"), a) && strict_eq(x.get("b"), b)))
                }),
            };
            if taken {
                return Err(t0("model.err.splice-taken"));
            }
            o.insert("id".into(), Value::from(id.clone()));
            o.insert("type".into(), Value::from("splice"));
            if let Some(v) = a {
                o.insert("a".into(), v.clone());
            }
            o.insert("aEnd".into(), a_end_v);
            if let Some(v) = b {
                o.insert("b".into(), v.clone());
            }
            o.insert("params".into(), params_with("splice"));
        } else if ty.and_then(Value::as_str) == Some("gusset") {
            let ms = self.recs("members");
            let (br, sp) = (field(c, "brace"), field(c, "support"));
            let (br_type, sp_ok) = match idx.as_deref() {
                Some(ix) => (ix.member_type(br).map(|t| t.to_string()), ix.member_type(sp).is_some()),
                None => (
                    ms.iter()
                        .find(|m| strict_eq(m.get("id"), br))
                        .map(|m| m.get("type").and_then(JVal::as_str).unwrap_or("\u{0}").to_string()),
                    ms.iter().any(|m| strict_eq(m.get("id"), sp)),
                ),
            };
            if br_type.as_deref() != Some("brace") || !sp_ok || strict_eq(br, sp) {
                return Err(t0("model.err.conn-gone"));
            }
            let end = if field(c, "end").and_then(Value::as_str) == Some("start") { "start" } else { "end" };
            let end_v = Value::from(end);
            let taken = match idx.as_deref() {
                Some(ix) => ix.gusset_taken(br, Some(&end_v)),
                None => conns.iter().any(|x| {
                    x.get("type").and_then(JVal::as_str) == Some("gusset")
                        && strict_eq(x.get("brace"), br)
                        && strict_eq(x.get("end"), Some(&end_v))
                }),
            };
            if taken {
                return Err(t0("model.err.end-taken"));
            }
            o.insert("id".into(), Value::from(id.clone()));
            o.insert("type".into(), Value::from("gusset"));
            if let Some(v) = br {
                o.insert("brace".into(), v.clone());
            }
            o.insert("end".into(), end_v);
            if let Some(v) = sp {
                o.insert("support".into(), v.clone());
            }
            o.insert("params".into(), params_with("gusset"));
        } else {
            return Err(t0("model.err.conn-unknown"));
        }
        let made = Value::Object(o);
        if let Some(ix) = idx {
            ix.learn(&made);
        }
        self.st().connections_mut().push(JVal::from(made));
        Ok(Value::from(id))
    }
    pub fn update_connection(&mut self, id: Option<&Value>, params: Option<&Value>) -> Res<()> {
        let idx = self.recs("connections").iter().position(|x| strict_eq(x.get("id"), id)).ok_or_else(|| t0("model.err.conn-gone"))?;
        let cur = self.recs("connections").get(idx).map_or(Value::Null, JVal::to_value);
        let mut next = spread(cur.get("params"), None);
        for (k, v) in entries(params) {
            let Some(old) = next.get(&k) else { continue };
            if old.is_number() {
                let x = num(Some(&v));
                if !x.is_finite() || x < 0.0 || x > 10000.0 {
                    return Err(t("model.err.param-mm", &[("name", F::from(k.as_str()))]));
                }
                next.insert(k, n(x));
            } else {
                next.insert(k, Value::from(clean_text(Some(&v), 30)));
            }
        }
        if let Some(list) = self.st().held_mut("connections")
            && let Some(c) = list.get_mut(idx)
        {
            jm_of(c).insert("params", JVal::Obj(JObj::from_map_owned(next)));
        }
        Ok(())
    }
    pub fn remove_connections(&mut self, ids: Option<&Value>) {
        let set = arr(ids).to_vec();
        self.st().connections_mut().retain(|c| !set_has(&set, c.get("id")));
    }

    fn footing_input(base: &Value, f: Option<&Value>) -> Value {
        let mut m = spread(Some(base), f);
        let sub = |k: &str| {
            let extra = field(f, k).filter(|v| truthy(Some(v)));
            Value::Object(spread(base.get(k), extra))
        };
        m.insert("mat".into(), sub("mat"));
        m.insert("starters".into(), sub("starters"));
        Value::Object(m)
    }
    pub fn add_footing(&mut self, f: Option<&Value>) -> Res<Value> {
        let id = self.next_id("f");
        if f.is_none_or(Value::is_null) {
            return Err(format!("Cannot read properties of {} (reading 'mat')", if f.is_none() { "undefined" } else { "null" }));
        }
        let r = clean_footing(&Self::footing_input(&footing_defaults(), f))?;
        let phase = clean_phase(field(f, "phase")).unwrap_or(1.0);
        self.ensure_phase(phase);
        let mut o = Map::new();
        o.insert("id".into(), Value::from(id.clone()));
        o.extend(r.fields());
        o.insert("phase".into(), n(phase));
        self.st().footings_mut().push(JVal::Obj(JObj::from_map_owned(o)));
        Ok(Value::from(id))
    }
    pub fn update_footing(&mut self, id: Option<&Value>, patch: Option<&Value>) -> Res<()> {
        let i = self.recs("footings").iter().position(|x| strict_eq(x.get("id"), id)).ok_or_else(|| t0("model.err.footing-gone"))?;
        let f = self.recs("footings").get(i).map_or(Value::Null, JVal::to_value);
        let r = clean_footing(&Self::footing_input(&f, patch))?;
        let phase = if field(patch, "phase").is_some() {
            clean_phase(field(patch, "phase"))
        } else {
            Some(crate::js::or(num(f.get("phase")), 1.0))
        };
        let phase = phase.ok_or_else(|| t("phase.err.number", &[("max", F::N(MAX_PHASE))]))?;
        self.ensure_phase(phase);
        let mut o = Map::new();
        o.insert("id".into(), id.cloned().unwrap_or(Value::Null));
        o.extend(r.fields());
        o.insert("phase".into(), n(phase));
        if let Some(slot) = self.st().footings_mut().get_mut(i) {
            *slot = JVal::Obj(JObj::from_map_owned(o));
        }
        Ok(())
    }
    pub fn remove_footings(&mut self, ids: Option<&Value>) {
        let set = arr(ids).to_vec();
        self.st().footings_mut().retain(|c| !set_has(&set, c.get("id")));
    }
    fn stair_input(base: &Value, x: Option<&Value>) -> Value {
        let mut m = spread(Some(base), x);
        let extra = field(x, "tread").filter(|v| truthy(Some(v)));
        m.insert("tread".into(), Value::Object(spread(base.get("tread"), extra)));
        Value::Object(m)
    }
    pub fn add_stair(&mut self, x: Option<&Value>) -> Res<Value> {
        let id = self.next_id("s");
        if x.is_none_or(Value::is_null) {
            return Err(format!("Cannot read properties of {} (reading 'tread')", if x.is_none() { "undefined" } else { "null" }));
        }
        let r = clean_stair(&Self::stair_input(&stair_defaults(), x)).map_err(|e| t("stair.err.field", &[("name", F::from(e))]))?;
        let phase = clean_phase(field(x, "phase")).unwrap_or(1.0);
        self.ensure_phase(phase);
        let st = self.st();
        if !st.get("stairs").is_some_and(Value::is_array) {
            st.insert("stairs", json!([]));
        }
        let mut o = Map::new();
        o.insert("id".into(), Value::from(id.clone()));
        o.extend(r.fields());
        o.insert("phase".into(), n(phase));
        self.st().arr_mut("stairs").push(Value::Object(o));
        Ok(Value::from(id))
    }
    pub fn update_stair(&mut self, id: Option<&Value>, patch: Option<&Value>) -> Res<()> {
        let i = self.list("stairs").iter().position(|x| strict_eq(x.get("id"), id)).ok_or_else(|| t0("stair.err.gone"))?;
        let x = self.list("stairs").get(i).cloned().unwrap_or(Value::Null);
        let r = clean_stair(&Self::stair_input(&x, patch)).map_err(|e| t("stair.err.field", &[("name", F::from(e))]))?;
        let phase = if field(patch, "phase").is_some() {
            clean_phase(field(patch, "phase"))
        } else {
            Some(crate::js::or(num(x.get("phase")), 1.0))
        };
        let phase = phase.ok_or_else(|| t("phase.err.number", &[("max", F::N(MAX_PHASE))]))?;
        self.ensure_phase(phase);
        let mut o = Map::new();
        o.insert("id".into(), id.cloned().unwrap_or(Value::Null));
        o.extend(r.fields());
        o.insert("phase".into(), n(phase));
        if let Some(slot) = self.st().arr_mut("stairs").get_mut(i) {
            *slot = Value::Object(o);
        }
        Ok(())
    }
    pub fn add_bolt(&mut self, b: Option<&Value>) -> Res<Value> {
        let c = b.and_then(clean_bolt).ok_or_else(|| t0("model.err.bolt"))?;
        let st = self.st();
        if !st.get("bolts").is_some_and(Value::is_array) {
            st.insert("bolts", json!([]));
        }
        let id = self.next_id("k");
        let mut o = Map::new();
        o.insert("id".into(), Value::from(id.clone()));
        o.extend(c);
        self.st().arr_mut("bolts").push(Value::Object(o));
        Ok(Value::from(id))
    }
    pub fn remove_bolts(&mut self, ids: Option<&Value>) {
        let set = ids.filter(|v| truthy(Some(v))).map(|v| arr(Some(v)).to_vec());
        let st = self.st();
        match set {
            Some(set) => {
                let keep: Vec<Value> = arr(st.get("bolts")).iter().filter(|b| !set_has(&set, b.get("id"))).cloned().collect();
                st.insert("bolts", Value::Array(keep));
            }
            None => {
                st.insert("bolts", json!([]));
            }
        }
    }
    pub fn remove_stairs(&mut self, ids: Option<&Value>) {
        let set = arr(ids).to_vec();
        let st = self.st();
        let keep: Vec<Value> = arr(st.get("stairs")).iter().filter(|x| !set_has(&set, x.get("id"))).cloned().collect();
        st.insert("stairs", Value::Array(keep));
    }
    pub fn add_profile(&mut self, p: Option<&Value>) -> Res<()> {
        let r = clean_custom(p.unwrap_or(&Value::Null), false)?;
        let name = upper(s(r.get("name")));
        if self.list("profiles").iter().any(|x| upper(s(x.get("name"))) == name) {
            return Err(t("model.err.profile-taken", &[("name", F::from(s(r.get("name"))))]));
        }
        self.st().arr_mut("profiles").push(r);
        Ok(())
    }

    /// Runs one Tx method by name with JSON arguments (the scripts' and the harness's way in).
    pub fn call(&mut self, op: &str, a: &[Value]) -> Res<Option<Value>> {
        let g = |i: usize| a.get(i);
        Ok(match op {
            "nextId" => Some(Value::from(self.next_id(&crate::js::string_of(g(0))))),
            "setName" => {
                self.set_name(g(0));
                None
            }
            "setGrids" => {
                self.set_grids(g(0))?;
                None
            }
            "setLevels" => {
                self.set_levels(g(0))?;
                None
            }
            "ensurePhase" => {
                self.ensure_phase(num(g(0)));
                None
            }
            "addPhase" => Some(n(self.add_phase(g(0))?)),
            "renamePhase" => {
                self.rename_phase(g(0), g(1))?;
                None
            }
            "removePhase" => {
                self.remove_phase(g(0))?;
                None
            }
            "setNumbering" => {
                self.set_numbering(g(0));
                None
            }
            "addIssues" => {
                self.add_issues(g(0))?;
                None
            }
            "setPhaseOf" => {
                self.set_phase_of(g(0), g(1))?;
                None
            }
            "addMember" => Some(self.add_member(g(0))?),
            "updateMember" => {
                self.update_member(g(0), g(1))?;
                None
            }
            "removeMembers" => {
                self.remove_members(g(0));
                None
            }
            "addHole" => Some(self.add_hole(g(0), g(1))?),
            "removeHole" => {
                self.remove_hole(g(0), g(1));
                None
            }
            "addCut" => Some(self.add_cut(g(0), g(1))?),
            "removeCut" => {
                self.remove_cut(g(0), g(1));
                None
            }
            "addConnection" => Some(self.add_connection(g(0))?),
            "updateConnection" => {
                self.update_connection(g(0), g(1))?;
                None
            }
            "removeConnections" => {
                self.remove_connections(g(0));
                None
            }
            "addFooting" => Some(self.add_footing(g(0))?),
            "updateFooting" => {
                self.update_footing(g(0), g(1))?;
                None
            }
            "removeFootings" => {
                self.remove_footings(g(0));
                None
            }
            "addStair" => Some(self.add_stair(g(0))?),
            "updateStair" => {
                self.update_stair(g(0), g(1))?;
                None
            }
            "addBolt" => Some(self.add_bolt(g(0))?),
            "removeBolts" => {
                self.remove_bolts(g(0));
                None
            }
            "removeStairs" => {
                self.remove_stairs(g(0));
                None
            }
            "addProfile" => {
                self.add_profile(g(0))?;
                None
            }
            _ => return Err(format!("tx.{op} is not a function")),
        })
    }
}

/// `readModel(text)` → (state, warnings), or the refusal.
pub fn read_model(input: &Value) -> Res<(State, Vec<String>)> {
    // S.15: a file's text is read typed (members and connections straight into their fields); text that is not a
    // JSON object, or that the typed read refuses, is read as JSON below, which gives the same result or error
    if let Value::String(text) = input
        && let Some(f) = crate::read_typed::parse_file(text)
    {
        let j = Value::Object(f.rest);
        return read_core(&j, &f.members.0, &f.connections.0, &f.footings.0);
    }
    match input {
        Value::String(text) => read_model_json(text),
        j => read_value(j),
    }
}

/// readModel of a file's text through the JSON tree (the whole text parsed into values, then read): what the typed
/// read is checked against (tests, fuzz target `detail_read_typed`), and the route for text it refuses.
pub fn read_model_json(text: &str) -> Res<(State, Vec<String>)> {
    let j = parse(text).map_err(|_| t0("model.err.not-json"))?;
    read_value(&j)
}

fn read_value(j: &Value) -> Res<(State, Vec<String>)> {
    read_core(j, list_of(j.get("members"), 20000), list_of(j.get("connections"), 20000), list_of(j.get("footings"), 5000))
}

/// readModel after the parse: `j` the file's fields (members and connections given apart, already capped).
fn read_core<M: MemberSrc, C: ConnSrc, FT: Jsv>(
    j: &Value,
    members_in: &[M],
    conns_in: &[C],
    footings_in: &[FT],
) -> Res<(State, Vec<String>)> {
    if !truthy(Some(j)) || j.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err(t0("model.err.not-model"));
    }
    let ver = match j.get("version") {
        Some(Value::Number(x)) => x.as_f64().unwrap_or(f64::NAN),
        _ => f64::NAN,
    };
    if !is_integer(ver) || ver < 1.0 {
        return Err(t0("model.err.no-version"));
    }
    if ver > VERSION {
        return Err(t("model.err.newer", &[("version", F::N(ver))]));
    }
    let mut warnings = Vec::new();
    let nm = clean_text(j.get("name"), 80);
    let mut s = empty_state(&if nm.is_empty() { t0("model.name.untitled") } else { nm });
    let gl = |a: Option<&Value>| -> Vec<Value> {
        let mut out: Vec<Value> = list_of(a, 60)
            .iter()
            .map(|g| (clean_text(field(Some(g), "label"), 8), num(field(Some(g), "at"))))
            .filter(|(l, at)| !l.is_empty() && at.is_finite())
            .map(|(l, at)| json!({ "label": l, "at": at }))
            .collect();
        out.sort_by(|a, b| crate::js::ord(num(a.get("at")) - num(b.get("at"))));
        out
    };
    let grids = j.get("grids");
    put(&mut s, "grids", json!({ "x": gl(field(grids, "x")), "y": gl(field(grids, "y")) }));
    let mut lv: Vec<Value> = list_of(j.get("levels"), 200)
        .iter()
        .map(|l| (clean_text(field(Some(l), "name"), 40), num(field(Some(l), "z"))))
        .filter(|(nm, z)| !nm.is_empty() && z.is_finite())
        .map(|(nm, z)| json!({ "name": nm, "z": z }))
        .collect();
    if !lv.is_empty() {
        lv.sort_by(|a, b| crate::js::ord(num(a.get("z")) - num(b.get("z"))));
        put(&mut s, "levels", Value::Array(lv));
    }
    let mut profiles: Vec<Value> = Vec::new();
    for p in list_of(j.get("profiles"), 500) {
        match clean_custom(p, true) {
            Ok(r) if !profiles.iter().any(|x| upper(s_(x.get("name"))) == upper(s_(r.get("name")))) => profiles.push(r),
            _ => warnings.push(t0("model.warn.profile")),
        }
    }
    // the largest number after an id's letter (readModel's seq scan over the ids kept, made as they are kept)
    let mut max_seq = 0.0;
    // every kept id is a letter and digits (the check below, or made so): kept as a number, no copy of the text
    let mut id_set: std::collections::HashSet<u128, cc_m3d::omap::FxBuild> = std::collections::HashSet::default();
    id_set.reserve(members_in.len().saturating_add(conns_in.len()));
    // `uid(cleanText(id, 20), prefix)`
    let mut uid = |cleaned: String, prefix: &str| -> String {
        let mut v = cleaned;
        // /^[a-z]\d+$/ (a letter, then ASCII digits: bytes are enough)
        let ok = match v.as_bytes() {
            [c, rest @ ..] => c.is_ascii_lowercase() && !rest.is_empty() && rest.iter().all(u8::is_ascii_digit),
            [] => false,
        };
        if !ok || id_key(&v).is_none_or(|k| id_set.contains(&k)) {
            v = format!("{prefix}{}", 100_000usize.saturating_add(id_set.len()));
        }
        if let Some(k) = id_key(&v)
            && id_set.insert(k)
        {
            // every kept id starts with one ASCII letter
            let rest = v.get(1..).unwrap_or("");
            max_seq = crate::js::max(max_seq, crate::js::or(str_to_number(rest), 0.0));
        }
        v
    };
    let mut members: Vec<JVal> = Vec::with_capacity(members_in.len());
    let mut dropped = 0usize;
    // whether a profile name is one (getProfile against the file's own profiles, which stay as they are here)
    let mut known: std::collections::HashMap<String, bool, cc_m3d::omap::FxBuild> = std::collections::HashMap::default();
    for m in members_in {
        if Src::is_null(m) {
            dropped = dropped.saturating_add(1);
            continue;
        }
        let id = JStr::from_string(uid(clean_text(Src::get(m, "id"), 20), "m"));
        let ty = match Src::get(m, "type").and_then(Jsv::js_str) {
            Some(t) if MEMBER_TYPES.contains(&t) => JStr::new(t),
            _ => JStr::S("beam"),
        };
        let profile = crate::js::clean_jstr(Src::get(m, "profile"), 40);
        let mat = crate::js::clean_jstr(Src::get(m, "material"), 30);
        let mat = if mat.is_empty() { JStr::S("A992") } else { mat };
        let (Ok(start), Ok(end)) = (vec3_p(Src::get(m, "start")), vec3_p(Src::get(m, "end"))) else {
            // the end's uid calls never happen when start or end throws
            dropped = dropped.saturating_add(1);
            continue;
        };
        let rot = num_or0(Src::get(m, "rotation"));
        let pos = match Src::get(m, "position").and_then(Jsv::js_str) {
            Some(p) if ["top", "middle", "bottom"].contains(&p) => JStr::new(p),
            _ => JStr::S("middle"),
        };
        let name = crate::js::clean_jstr(Src::get(m, "name"), 40);
        let mut cuts = Vec::new();
        for c in m.list("cuts", 50) {
            if let Some(mut k) = clean_cut_s(c) {
                k.0.push((JStr::S("id"), JVal::Str(JStr::from_string(uid(clean_text(Src::get(c, "id"), 20), "k")))));
                cuts.push(JVal::Obj(k));
            }
        }
        let mut holes = Vec::new();
        for h in m.list("holes", 2000) {
            if let Some(mut k) = clean_hole_s(h) {
                k.0.push((JStr::S("id"), JVal::Str(JStr::from_string(uid(clean_text(Src::get(h, "id"), 20), "h")))));
                holes.push(JVal::Obj(k));
            }
        }
        let phase = clean_phase(Src::get(m, "phase")).unwrap_or(1.0);
        // checkMember: the profile is one, the material and position are (made so above), the rotation a number
        // (num || 0 is one unless it is ±∞), start and end apart
        let ok_profile = match known.get(profile.as_str()) {
            Some(k) => *k,
            None => {
                let k = get_profile_str(&profile, &profiles).is_some();
                known.insert(profile.as_str().to_string(), k);
                k
            }
        };
        if !ok_profile || !rot.is_finite() || hypot3(end[0] - start[0], end[1] - start[1], end[2] - start[2]) < 1.0 {
            dropped = dropped.saturating_add(1);
            continue;
        }
        let p3 = |p: [f64; 3]| JVal::Arr(vec![JVal::num(p[0]), JVal::num(p[1]), JVal::num(p[2])]);
        let mut mem = JObj::with_capacity(12);
        mem.0.push((JStr::S("id"), JVal::Str(id)));
        mem.0.push((JStr::S("type"), JVal::Str(ty)));
        mem.0.push((JStr::S("profile"), JVal::Str(profile)));
        mem.0.push((JStr::S("material"), JVal::Str(mat)));
        mem.0.push((JStr::S("start"), p3(start)));
        mem.0.push((JStr::S("end"), p3(end)));
        mem.0.push((JStr::S("rotation"), JVal::num(((rot % 360.0) + 360.0) % 360.0)));
        mem.0.push((JStr::S("position"), JVal::Str(pos)));
        mem.0.push((JStr::S("name"), JVal::Str(name)));
        mem.0.push((JStr::S("cuts"), JVal::Arr(cuts)));
        mem.0.push((JStr::S("holes"), JVal::Arr(holes)));
        mem.0.push((JStr::S("phase"), JVal::num(phase)));
        members.push(JVal::Obj(mem));
    }
    if dropped > 0 {
        warnings.push(t("model.warn.members", &[("n", F::N(dropped as f64))]));
    }
    let member_ids: std::collections::HashSet<&str, cc_m3d::omap::FxBuild> = members.iter().map(|m| crate::js::s(m.get("id"))).collect();
    let has_member = |v: Option<&str>| v.is_some_and(|x| member_ids.contains(x));
    let mut conns: Vec<JVal> = Vec::with_capacity(conns_in.len());
    let mut dropped_c = 0usize;
    for c in conns_in {
        let ty = Src::get(c, "type").and_then(Jsv::js_str).unwrap_or("");
        let base = Src::get(c, "type").and_then(Jsv::js_str).and_then(crate::connections::defaults_ref);
        let Some(base) = base else {
            dropped_c = dropped_c.saturating_add(1);
            continue;
        };
        if !members_of_s(c).iter().all(|id| has_member(id.and_then(Jsv::js_str)))
            || (ty == "splice" && strict_eq(Src::get(c, "a"), Src::get(c, "b")))
            || (ty == "gusset" && strict_eq(Src::get(c, "brace"), Src::get(c, "support")))
        {
            dropped_c = dropped_c.saturating_add(1);
            continue;
        }
        let kp = crate::state::kind_params(ty);
        let mut params = JObj::with_capacity(kp.len());
        // params: the defaults' keys in their order, each the file's value when it is a good one
        let _ = base;
        for &(k, bv, i) in kp {
            let v = match c.param_i(k, i) {
                None => JVal::from_value(bv),
                Some(v) if bv.is_number() => {
                    let x = num(Some(v));
                    if x.is_finite() && (0.0..=10000.0).contains(&x) { JVal::num(x) } else { JVal::from_value(bv) }
                }
                Some(v) => {
                    let cv = crate::js::clean_jstr(Some(v), 30);
                    if cv.is_empty() { JVal::from_value(bv) } else { JVal::Str(cv) }
                }
            };
            params.0.push((JStr::S(k), v));
        }
        let mut o = JObj::with_capacity(7);
        let g = |k: &str| Src::get(c, k);
        let gv = |k: &str| g(k).map_or(JVal::Null, Jsv::js_jval);
        let end_of = |k: &str| JVal::Str(JStr::S(if g(k).and_then(Jsv::js_str) == Some("start") { "start" } else { "end" }));
        o.0.push((JStr::S("id"), JVal::Str(JStr::from_string(uid(clean_text(g("id"), 20), "j")))));
        match ty {
            "splice" => {
                o.0.push((JStr::S("type"), JVal::Str(JStr::S("splice"))));
                o.0.push((JStr::S("a"), gv("a")));
                o.0.push((JStr::S("aEnd"), end_of("aEnd")));
                o.0.push((JStr::S("b"), gv("b")));
                o.0.push((JStr::S("params"), JVal::Obj(params)));
            }
            "gusset" => {
                o.0.push((JStr::S("type"), JVal::Str(JStr::S("gusset"))));
                o.0.push((JStr::S("brace"), gv("brace")));
                o.0.push((JStr::S("end"), end_of("end")));
                o.0.push((JStr::S("support"), gv("support")));
                o.0.push((JStr::S("params"), JVal::Obj(params)));
            }
            _ => {
                o.0.push((JStr::S("type"), JVal::str(ty)));
                if let Some(v) = g("column") {
                    o.0.push((JStr::S("column"), v.js_jval()));
                }
                o.0.push((JStr::S("params"), JVal::Obj(params)));
            }
        }
        if is_beam_end_s(c) {
            o.0.push((JStr::S("beam"), gv("beam")));
            o.0.push((JStr::S("end"), end_of("end")));
        }
        conns.push(JVal::Obj(o));
    }
    if dropped_c > 0 {
        warnings.push(t("model.warn.connections", &[("n", F::N(dropped_c as f64))]));
    }
    let mut footings: Vec<JVal> = Vec::new();
    for f in footings_in {
        match clean_footing(f) {
            Ok(r) => {
                let mut o = JObj::with_capacity(12);
                o.0.push((JStr::S("id"), JVal::Str(JStr::from_string(uid(clean_text(field(Some(f), "id"), 20), "f")))));
                r.fields_j(&mut o);
                o.insert("phase", JVal::num(clean_phase(field(Some(f), "phase")).unwrap_or(1.0)));
                footings.push(JVal::Obj(o));
            }
            Err(e) => warnings.push(t("model.warn.footing", &[("error", F::from(e))])),
        }
    }
    let mut bolts = Vec::new();
    for b in list_of(j.get("bolts"), 200_000) {
        if let Some(c) = clean_bolt(b) {
            let mut o = Map::new();
            o.insert("id".into(), Value::from(uid(clean_text(field(Some(b), "id"), 20), "k")));
            o.extend(c);
            bolts.push(Value::Object(o));
        }
    }
    let mut stairs = Vec::new();
    for x in list_of(j.get("stairs"), 2000) {
        match clean_stair(x) {
            Ok(r) => {
                let mut o = Map::new();
                o.insert("id".into(), Value::from(uid(clean_text(field(Some(x), "id"), 20), "s")));
                o.extend(r.fields());
                o.insert("phase".into(), n(clean_phase(field(Some(x), "phase")).unwrap_or(1.0)));
                stairs.push(Value::Object(o));
            }
            Err(e) => warnings.push(t("model.warn.stair", &[("name", F::from(e))])),
        }
    }
    let mut ph: Vec<Value> = Vec::new();
    for p in list_of(j.get("phases"), 999) {
        if let Some(nn) = clean_phase(field(Some(p), "n"))
            && !ph.iter().any(|x| num(x.get("n")) == nn)
        {
            ph.push(json!({ "n": nn, "name": clean_text(field(Some(p), "name"), 40) }));
        }
    }
    put(&mut s, "profiles", Value::Array(profiles));
    s.set_records("members", members);
    s.set_records("connections", conns);
    s.set_records("footings", footings);
    put(&mut s, "bolts", Value::Array(bolts));
    put(&mut s, "stairs", Value::Array(stairs));
    let base_ph = if ph.is_empty() { vec![json!({ "n": 1, "name": "" })] } else { ph };
    let phases = with_used_phases(&base_ph, &s);
    put(&mut s, "phases", Value::Array(phases));
    let pp = field(j.get("numbering"), "perPhase").and_then(Value::as_bool) != Some(false);
    put(&mut s, "numbering", json!({ "perPhase": pp }));
    put(&mut s, "issues", Value::Array(list_of(j.get("issues"), 20000).iter().filter_map(clean_issue).map(Value::Object).collect()));
    let seq0 = crate::js::or(num(j.get("seq")), 1.0);
    put(&mut s, "seq", n(crate::js::max(seq0, max_seq + 1.0)));
    Ok((s, warnings))
}

/// An id of a letter and at most 19 digits as one number (the letter, the digit count, the digits), else `None`.
fn id_key(v: &str) -> Option<u128> {
    let (c, rest) = v.as_bytes().split_first()?;
    if rest.is_empty() || rest.len() > 19 || !rest.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let digits = rest.iter().try_fold(0u64, |a, d| a.checked_mul(10)?.checked_add(u64::from(d.checked_sub(b'0')?)))?;
    let len = u128::try_from(rest.len()).ok()?;
    Some((u128::from(*c) << 72) | (len << 64) | u128::from(digits))
}

/// `isBeamEnd(c)` of a connection being read.
fn is_beam_end_s<S: Src>(c: &S) -> bool {
    matches!(c.get("type").and_then(Jsv::js_str), Some("endplate" | "sheartab"))
}
/// `membersOf(c)` of a connection being read (as state.rs `members_of_v`).
fn members_of_s<S: Src>(c: &S) -> Vec<Option<&S::V>> {
    let g = |k: &str| c.get(k);
    match c.get("type").and_then(Jsv::js_str) {
        Some("splice") => vec![g("a"), g("b")],
        Some("gusset") => vec![g("support"), g("brace")],
        _ if is_beam_end_s(c) => vec![g("beam"), g("column")],
        _ => vec![g("column")],
    }
}

fn s_<T: Jsv>(v: Option<&T>) -> &str {
    s(v)
}

#[cfg(test)]
mod conn_index_tests {
    use super::*;

    /// The index decides as the scans do: every existing connection again is refused the same way, and a fresh one
    /// on a free joint is made by both.
    #[test]
    fn the_index_decides_as_the_scans() {
        let m = crate::sample::sample_model("Sample frame").expect("sample");
        let conns: Vec<Value> = m.state.list("connections").iter().map(JVal::to_value).collect();
        assert!(!conns.is_empty());
        for c in &conns {
            let mut a = m.state.clone();
            let mut b = m.state.clone();
            let slow = Tx { s: &mut a }.add_connection(Some(c));
            let mut ix = ConnIndex::of(&b);
            let fast = Tx { s: &mut b }.add_connection_in(Some(c), Some(&mut ix));
            assert_eq!(slow, fast, "{c}");
            assert_eq!(a.stringify(), b.stringify());
        }
        // a splice of two members that don't meet still goes through the same member checks
        let s = json!({"type": "splice", "a": "m7", "aEnd": "end", "b": "m404"});
        let (mut a, mut b) = (m.state.clone(), m.state.clone());
        let mut ix = ConnIndex::of(&b);
        assert_eq!(Tx { s: &mut a }.add_connection(Some(&s)), Tx { s: &mut b }.add_connection_in(Some(&s), Some(&mut ix)));
        assert_eq!(eq_key(Some(&json!(-0.0))), eq_key(Some(&json!(0))));
    }
}
/// S.15: the typed read of a file's text gives what the JSON read gives (same state and warnings, or the same error)
/// on the sample and 20,000 files made from it: fields of members, holes, copes, connections and params replaced by
/// values of every kind, dropped or added, lists of the wrong kind, members and connections duplicated or dropped,
/// then a third of them damaged as text (duplicated and escaped keys, cut files, stray bytes, deep nesting, numbers
/// out of range, lone surrogates, trailing text).
#[cfg(test)]
mod typed_read_tests {
    use super::*;

    #[test]
    fn typed_read_reads_as_the_json_does() {
        let mut typed = 0usize;
        let mut ok = 0usize;
        for t in corpus(20_000).iter().chain([r#""p""#.to_string(), "[]".into(), "{}".into(), "nul".into()].iter()) {
            let (a, b) = (digest(t), digest_json(t));
            assert_eq!(a, b, "{}", t.get(..t.len().min(400)).unwrap_or(""));
            if parse(t).is_ok_and(|v| v.is_object()) {
                assert!(crate::read_typed::parse_file(t).is_some(), "the typed read took every object");
                typed += 1;
            }
            ok += usize::from(a.starts_with("ok"));
        }
        assert!(typed > 17_000 && ok > 15_000, "{typed} typed, {ok} read");
    }

    /// S.16: the state held as records reads every file as the model before it did (one JSON value): the same state
    /// text and warnings, or the same error, on 24,000 random and broken files.
    #[test]
    fn state_reads_as_the_reference() {
        for t in corpus(24_000).iter().chain([r#""p""#.to_string(), "[]".into(), "{}".into(), "nul".into(), "5".into()].iter()) {
            if let Err(e) = crate::reference::check_read(t) {
                panic!("{e}\n{}", t.get(..t.len().min(400)).unwrap_or(""));
            }
        }
    }
    /// S.16: random edits (every Tx method, with arguments of every kind; several to a step) with undo and redo, from
    /// the sample, the starter state, an empty and a broken state and 400 states read from random files: the results,
    /// errors and state text after every step are the old model's.
    #[test]
    fn edits_as_the_reference() {
        let mut starts: Vec<Value> = vec![
            crate::sample::sample_model("Sample frame").expect("sample").state.to_value(),
            starter_state("S").to_value(),
            json!({}),
            json!({ "members": 5, "connections": [null, 3, {"id": "j1", "type": "endplate"}], "footings": "x", "seq": "a" }),
            Value::Null,
        ];
        for t in corpus(2_000).iter().step_by(5) {
            if let Ok((s, _)) = crate::reference::read_model(&Value::String(t.clone())) {
                starts.push(s);
            }
        }
        let mut steps = 0usize;
        for (i, st) in starts.iter().enumerate() {
            for k in 0..4u64 {
                let mut p = crate::reference::Picks::seeded((i as u64) * 7919 + k * 104_729 + 1);
                if let Err(e) = crate::reference::check_edits(st, &mut p, 40) {
                    panic!("start {i} seed {k}: {e}");
                }
                steps += 40;
            }
        }
        assert!(steps > 50_000, "{steps}");
    }

    /// A small deterministic generator (xorshift64*).
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
        }
        fn below(&mut self, n: usize) -> usize {
            usize::try_from(self.next() % (n.max(1) as u64)).unwrap_or(0)
        }
        fn chance(&mut self, k: usize) -> bool {
            self.below(k) == 0
        }
    }

    /// A value of any kind, the kinds a hostile file puts where the reader expects a number, a word or a list.
    fn junk(r: &mut Rng, depth: usize) -> Value {
        match r.below(if depth > 3 { 9 } else { 12 }) {
            0 => Value::Null,
            1 => json!(true),
            2 => json!(false),
            3 => json!(0),
            4 => json!(-0.0),
            5 => json!(r.below(5000) as f64 - 500.0),
            6 => json!(1.5e300),
            7 => json!(["", "web", "top", "start", "end", "cope", "A36", " W14x90 ", "m1", "beam", "1e3", "\u{7}x"][r.below(12)]),
            8 => json!(u64::MAX),
            9 => json!([junk(r, depth + 1), junk(r, depth + 1)]),
            10 => json!({ "x": junk(r, depth + 1) }),
            _ => json!([1, 2, 3]),
        }
    }

    /// The sample and the frames model, then fields of members, holes, copes, connections and their params replaced
    /// by junk, dropped or added; lists of the wrong kind; members and connections of other kinds.
    fn mutate(r: &mut Rng, base: &Value) -> Value {
        let mut v = base.clone();
        let n = 1 + r.below(12);
        for _ in 0..n {
            let list = ["members", "connections"][r.below(2)];
            let Some(a) = v.get_mut(list).and_then(Value::as_array_mut) else { continue };
            if a.is_empty() {
                continue;
            }
            let i = r.below(a.len());
            let Some(x) = a.get_mut(i) else { continue };
            match r.below(10) {
                0 => *x = junk(r, 0),
                1 | 2 => {
                    if let Some(o) = x.as_object_mut() {
                        let keys: Vec<String> = o.keys().cloned().collect();
                        if let Some(k) = keys.get(r.below(keys.len())) {
                            if r.chance(3) {
                                o.remove(k);
                            } else {
                                o.insert(k.clone(), junk(r, 0));
                            }
                        }
                    }
                }
                3 => {
                    if let Some(o) = x.as_object_mut() {
                        let ks = [
                            "id", "type", "profile", "material", "start", "end", "rotation", "position", "name", "phase", "cuts", "holes",
                            "params", "a", "b", "aEnd", "beam", "column", "brace", "support", "zz",
                        ];
                        o.insert(ks[r.below(ks.len())].to_string(), junk(r, 0));
                    }
                }
                4 => {
                    // a hole or a cope changed
                    for k in ["holes", "cuts"] {
                        if let Some(h) = x.get_mut(k).and_then(Value::as_array_mut) {
                            if r.chance(2) {
                                h.push(junk(r, 0));
                            }
                            if let Some(e) = h.first_mut().and_then(Value::as_object_mut) {
                                let ks = ["id", "face", "x", "y", "d", "kind", "end", "side", "length", "depth"];
                                e.insert(ks[r.below(ks.len())].to_string(), junk(r, 0));
                            }
                        }
                    }
                    if let Some(o) = x.as_object_mut()
                        && r.chance(3)
                    {
                        o.insert("holes".into(), json!([{ "face": "web", "x": 100, "y": 0, "d": 22, "id": "h7" }, null, 5]));
                        o.insert("cuts".into(), json!([{ "kind": "cope", "end": "start", "side": "top", "length": 100, "depth": 30 }]));
                    }
                }
                5 => {
                    if let Some(p) = x.get_mut("params").and_then(Value::as_object_mut) {
                        let keys: Vec<String> = p.keys().cloned().collect();
                        if let Some(k) = keys.get(r.below(keys.len())) {
                            p.insert(k.clone(), junk(r, 0));
                        }
                    } else if let Some(o) = x.as_object_mut() {
                        o.insert("params".into(), junk(r, 0));
                    }
                }
                6 => {
                    let dup = x.clone();
                    a.push(dup);
                }
                7 => {
                    a.remove(i);
                }
                8 => {
                    if let Some(o) = x.as_object_mut() {
                        o.insert(
                            "id".into(),
                            [json!("m1"), json!("j1"), json!("x"), json!("m100000"), json!("k3"), json!(7)][r.below(6)].clone(),
                        );
                    }
                }
                _ => {
                    if let Some(o) = x.as_object_mut() {
                        o.insert(
                            "type".into(),
                            json!(
                                ["beam", "column", "brace", "plate", "splice", "gusset", "endplate", "sheartab", "baseplate", "x"]
                                    [r.below(10)]
                            ),
                        );
                    }
                }
            }
        }
        if r.chance(20)
            && let Some(o) = v.as_object_mut()
        {
            let k = ["members", "connections"][r.below(2)];
            o.insert(k.into(), junk(r, 0));
        }
        v
    }

    /// Text-level damage: duplicated keys, escaped keys, cut files, a stray byte, deep nesting, a number too big.
    fn damage(r: &mut Rng, text: String) -> String {
        match r.below(14) {
            0 => {
                let cut = r.below(text.len());
                let mut c = cut;
                while !text.is_char_boundary(c) {
                    c -= 1;
                }
                text[..c].to_string()
            }
            1 => text.replacen("\"members\"", "\"m\\u0065mbers\"", 1),
            2 => text.replacen("{\"id\"", "{\"id\":\"m5\",\"id\"", 1 + r.below(3)),
            3 => text.replacen("\"params\":{", "\"params\":{\"bolts\":3,\"bolts\":\"x\",", 1 + r.below(3)),
            4 => text.replacen("\"members\":[", "\"members\":[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]],", 1),
            5 => text.replacen("\"d\":22", "\"d\":1e400", 1),
            6 => text.replacen("\"connections\":[", "\"connections\":[{\"zz\":[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]}],", 1),
            7 => {
                let mut b = text.into_bytes();
                let i = r.below(b.len());
                if let Some(x) = b.get_mut(i) {
                    *x = b"{}[],:\"\\ 0e-x"[r.below(13)];
                }
                String::from_utf8_lossy(&b).into_owned()
            }
            8 => format!("{text} x"),
            9 => format!("[{text}]"),
            10 => text.replacen("\"members\":", "\"members\":[],\"members\":", 1),
            11 => text.replacen("\"name\":\"", "\"name\":\"\\ud800", 1),
            _ => text,
        }
    }

    pub(super) fn corpus(n: usize) -> Vec<String> {
        let mut out = Vec::with_capacity(n);
        let sample = parse(&crate::sample::sample_model("Sample frame").expect("sample").to_json()).expect("sample json");
        let mut r = Rng(0x9e37_79b9_7f4a_7c15);
        for i in 0..n {
            let v = mutate(&mut r, &sample);
            let t = if i % 3 == 0 { crate::js::stringify_indent(&v, 1) } else { crate::js::stringify(&v) };
            out.push(if r.chance(3) { damage(&mut r, t) } else { t });
        }
        out
    }

    fn show(r: Res<(State, Vec<String>)>) -> String {
        match r {
            Ok((s, w)) => format!("ok {} {}", s.stringify(), w.join("|")),
            Err(e) => format!("err {e}"),
        }
    }
    /// The typed read of the text.
    pub(super) fn digest(text: &str) -> String {
        show(read_model(&Value::String(text.to_string())))
    }
    /// The JSON read: the text parsed, then read as a value.
    pub(super) fn digest_json(text: &str) -> String {
        show(read_model_json(text))
    }
}
