//! model.js — the DetailCraft model and its one edit path. The state is plain JSON data, as in the JS: every edit goes
//! through [`Model::change`] (JSON-text snapshots, 200 deep; an edit that fails changes nothing), the edit vocabulary is
//! [`Tx`], and [`read_model`] checks an untrusted file field by field (controls stripped, bad parts dropped with
//! warnings, ids made unique, a newer version refused). Millimetres, z up.

use crate::connections::defaults_of;
use crate::i18n::{F, t, t0};
use crate::js::{
    Res, arr, clean_text, hypot3, is_integer, num, num_or0, num_to_string, parse, round, s, spread, str_to_number, stringify,
    stringify_indent, to_js_string, truthy, upper,
};
use crate::profiles::{clean_custom, get_profile};
use crate::rebar::{clean_footing, footing_defaults};
use crate::stair::{clean_stair, stair_defaults};
use crate::state::{is_beam_end_v, members_of_v};
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
pub fn field<'a>(v: Option<&'a Value>, k: &str) -> Option<&'a Value> {
    v.and_then(Value::as_object).and_then(|m| m.get(k))
}
/// `a === b` (absent = undefined; objects never equal: they are distinct references here).
pub fn strict_eq(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (None, None) | (Some(Value::Null), Some(Value::Null)) => true,
        (Some(Value::Bool(x)), Some(Value::Bool(y))) => x == y,
        (Some(Value::Number(x)), Some(Value::Number(y))) => x.as_f64() == y.as_f64(),
        (Some(Value::String(x)), Some(Value::String(y))) => x == y,
        _ => false,
    }
}
/// `new Set(list).has(v)`.
fn set_has(list: &[Value], v: Option<&Value>) -> bool {
    v.is_some_and(|v| list.iter().any(|x| strict_eq(Some(x), Some(v))))
}
/// `[...list].includes(v)` for a string list.
fn includes(list: &[&str], v: Option<&Value>) -> bool {
    v.and_then(Value::as_str).is_some_and(|s| list.contains(&s))
}
/// `Array.isArray(v) ? v : []`, capped.
fn list_of(v: Option<&Value>, cap: usize) -> &[Value] {
    let a = arr(v);
    a.get(..a.len().min(cap)).unwrap_or(a)
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
fn arr_mut<'a>(m: &'a mut Map<String, Value>, k: &str) -> &'a mut Vec<Value> {
    let e = m.entry(k.to_string()).or_insert_with(|| Value::Array(vec![]));
    if !e.is_array() {
        *e = Value::Array(vec![]);
    }
    match e {
        Value::Array(a) => a,
        _ => unreachable_vec(),
    }
}
#[allow(clippy::unreachable)]
fn unreachable_vec() -> &'static mut Vec<Value> {
    unreachable!("arr_mut makes the value an array first")
}
/// `Object.entries(v)` for a value spread or walked as entries (objects; strings and arrays by index).
fn entries(v: Option<&Value>) -> Vec<(String, Value)> {
    spread(v, None).into_iter().collect()
}
/// `s[k] = v` on an object state.
fn put(s: &mut Value, k: &str, v: Value) {
    m_of(s).insert(k.to_string(), v);
}
fn n(x: f64) -> Value {
    Value::from(x)
}
/// A fill from a JS value: undefined and null leave the `{name}` in the text, as i18n.js `fill` does.
fn fill_of<'a>(k: &'a str, v: Option<&Value>) -> Option<(&'a str, F)> {
    match v {
        None | Some(Value::Null) => None,
        Some(x) => Some((k, F::S(to_js_string(x)))),
    }
}

// ---------------------------------------------------------------------------------------------------------------

/// `emptyState(name)`.
pub fn empty_state(name: &str) -> Value {
    json!({
        "format": FORMAT, "version": 2, "name": name,
        "grids": { "x": [], "y": [] }, "levels": [{ "name": t("model.level", &[("n", F::N(1.0))]), "z": 0 }],
        "members": [], "connections": [], "footings": [], "stairs": [], "bolts": [], "profiles": [], "seq": 1,
        "phases": [{ "n": 1, "name": "" }], "numbering": { "perPhase": true }, "issues": [],
    })
}
/// `emptyState()` with the default name.
pub fn empty_state_default() -> Value {
    empty_state(&t0("model.name.untitled"))
}
/// `cleanPhase(v)`.
pub fn clean_phase(v: Option<&Value>) -> Option<f64> {
    let x = num(v);
    if is_integer(x) && (1.0..=MAX_PHASE).contains(&x) { Some(x) } else { None }
}
fn with_used_phases(phases: &[Value], state: &Value) -> Vec<Value> {
    let mut out: Vec<Value> = phases.iter().map(|p| Value::Object(spread(Some(p), None))).collect();
    let all = arr(state.get("members")).iter().chain(arr(state.get("footings"))).chain(arr(state.get("stairs")));
    for x in all {
        let ph = x.get("phase");
        if !out.iter().any(|p| strict_eq(p.get("n"), ph)) {
            let mut o = Map::new();
            if let Some(v) = ph {
                o.insert("n".into(), v.clone());
            }
            o.insert("name".into(), Value::from(""));
            out.push(Value::Object(o));
        }
    }
    out.sort_by(|a, b| crate::js::ord(num(a.get("n")) - num(b.get("n"))));
    out
}
/// `connPhase(state, c)`.
pub fn conn_phase(state: &Value, c: &Value) -> f64 {
    let main = members_of_v(c).first().copied().flatten();
    let m = arr(state.get("members")).iter().find(|m| strict_eq(m.get("id"), main));
    crate::js::or(m.map_or(f64::NAN, |m| crate::js::f(m.get("phase"))), 1.0)
}

/// `starterState(name)`.
pub fn starter_state(name: &str) -> Value {
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
pub fn nodes(state: &Value) -> Vec<Value> {
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
pub fn node_name(state: &Value, p: [f64; 3]) -> String {
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
pub fn point_at(state: &Value, gy: &Value, gx: &Value, level: &Value) -> Option<[f64; 3]> {
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
    pub state: Value,
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
    pub fn new(state: Value) -> Model {
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
        let before = stringify(&self.state);
        let result = {
            let mut tx = Tx { s: &mut self.state };
            f(&mut tx)
        };
        let result = match result {
            Ok(r) => r,
            Err(e) => {
                self.state = parse(&before).unwrap_or(Value::Null);
                return Err(e);
            }
        };
        let after = stringify(&self.state);
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
        self.redo_stack.push(Step { label: s.label.clone(), json: stringify(&self.state) });
        self.state = parse(&s.json).unwrap_or(Value::Null);
        self.version = self.version.saturating_add(1);
        Some(s.label)
    }
    pub fn redo(&mut self) -> Option<String> {
        let s = self.redo_stack.pop()?;
        self.undo_stack.push(Step { label: s.label.clone(), json: stringify(&self.state) });
        self.state = parse(&s.json).unwrap_or(Value::Null);
        self.version = self.version.saturating_add(1);
        Some(s.label)
    }
    /// `toJSON()` = `JSON.stringify(state, null, 1)`.
    pub fn to_json(&self) -> String {
        stringify_indent(&self.state, 1)
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
    pub fn reset(&mut self, state: Value) {
        self.state = state;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.version = self.version.saturating_add(1);
        self.saved_version = self.version;
    }
}

/// The edit vocabulary; each checks its input so a bad edit fails before anything is kept.
pub struct Tx<'a> {
    pub s: &'a mut Value,
}

fn vec3(p: Option<&Value>) -> Res<Value> {
    let a = match p {
        Some(Value::Array(a)) if a.len() == 3 => a,
        _ => return Err(t0("model.err.point")),
    };
    let v: Vec<f64> = a.iter().map(|x| num(Some(x))).collect();
    if v.iter().any(|x| !x.is_finite() || x.abs() > 1e7) {
        return Err(t0("model.err.coords"));
    }
    Ok(Value::Array(v.into_iter().map(n).collect()))
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
fn clean_hole(h: Option<&Value>) -> Option<Map<String, Value>> {
    let h = h.filter(|h| truthy(Some(h)))?;
    if !includes(&["web", "top", "bottom", "plate"], field(Some(h), "face")) {
        return None;
    }
    let (x, y, d) = (num(field(Some(h), "x")), num(field(Some(h), "y")), num(field(Some(h), "d")));
    if ![x, y, d].iter().all(|v| v.is_finite()) || d <= 0.0 || d > 200.0 || x.abs() > 1e6 || y.abs() > 1e4 {
        return None;
    }
    let mut o = Map::new();
    o.insert("face".into(), field(Some(h), "face").cloned().unwrap_or(Value::Null));
    o.insert("x".into(), n(x));
    o.insert("y".into(), n(y));
    o.insert("d".into(), n(d));
    Some(o)
}
fn clean_cut(c: Option<&Value>) -> Option<Map<String, Value>> {
    let c = c.filter(|c| truthy(Some(c)))?;
    let co = Some(c);
    if field(co, "kind").and_then(Value::as_str) != Some("cope")
        || !includes(&["start", "end"], field(co, "end"))
        || !includes(&["top", "bottom"], field(co, "side"))
    {
        return None;
    }
    let (length, depth) = (num(field(co, "length")), num(field(co, "depth")));
    if !(length > 0.0 && length < 1e5 && depth > 0.0 && depth < 5000.0) {
        return None;
    }
    let mut o = Map::new();
    o.insert("kind".into(), Value::from("cope"));
    o.insert("end".into(), field(co, "end").cloned().unwrap_or(Value::Null));
    o.insert("side".into(), field(co, "side").cloned().unwrap_or(Value::Null));
    o.insert("length".into(), n(length));
    o.insert("depth".into(), n(depth));
    Some(o)
}

impl Tx<'_> {
    fn st(&mut self) -> &mut Map<String, Value> {
        m_of(self.s)
    }
    fn list(&self, k: &str) -> &[Value] {
        arr(self.s.get(k))
    }
    fn profiles(&self) -> Vec<Value> {
        self.list("profiles").to_vec()
    }
    /// `nextId(prefix)`.
    pub fn next_id(&mut self, prefix: &str) -> String {
        let st = self.st();
        let seq = st.get("seq").cloned().unwrap_or(Value::Null);
        let x = num(Some(&seq));
        // `this.s.seq++`: the old value as a number, then one more
        st.insert("seq".into(), n(x + 1.0));
        format!("{prefix}{}", num_to_string(x))
    }
    pub fn set_name(&mut self, name: Option<&Value>) {
        let c = clean_text(name, 80);
        let v = if c.is_empty() { t0("model.name.untitled") } else { c };
        self.st().insert("name".into(), Value::from(v));
    }
    pub fn set_grids(&mut self, grids: Option<&Value>) -> Res<()> {
        let x = field(grids, "x").and_then(Value::as_array).ok_or_else(|| "grids.x.map is not a function".to_string())?;
        let y = field(grids, "y").and_then(Value::as_array).ok_or_else(|| "grids.y.map is not a function".to_string())?;
        let cp = |a: &Vec<Value>| Value::Array(a.iter().take(MAX_LIST).map(|g| Value::Object(spread(Some(g), None))).collect());
        let g = json!({ "x": cp(x), "y": cp(y) });
        self.st().insert("grids".into(), g);
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
        self.st().insert("levels".into(), Value::Array(out));
        Ok(())
    }
    /// `ensurePhase(n)`.
    pub fn ensure_phase(&mut self, ph: f64) {
        let st = self.st();
        if !st.get("phases").is_some_and(Value::is_array) {
            st.insert("phases".into(), json!([{ "n": 1, "name": "" }]));
        }
        if !truthy(st.get("numbering")) {
            st.insert("numbering".into(), json!({ "perPhase": true }));
        }
        let phases = arr_mut(st, "phases");
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
        arr_mut(self.st(), "phases").push(json!({ "n": nn, "name": nm }));
        Ok(nn)
    }
    pub fn rename_phase(&mut self, ph: Option<&Value>, name: Option<&Value>) -> Res<()> {
        let nm = clean_text(name, 40);
        let phases = arr_mut(self.st(), "phases");
        let p = phases.iter_mut().find(|x| strict_eq(x.get("n"), ph)).ok_or_else(|| t0("phase.err.gone"))?;
        m_of(p).insert("name".into(), Value::from(nm));
        Ok(())
    }
    pub fn remove_phase(&mut self, ph: Option<&Value>) -> Res<()> {
        let used = self
            .list("members")
            .iter()
            .chain(self.list("footings"))
            .chain(self.list("stairs"))
            .filter(|x| strict_eq(x.get("phase"), ph))
            .count();
        if used > 0 {
            let mut vars = vec![("n", F::N(used as f64))];
            vars.extend(fill_of("phase", ph));
            return Err(t("phase.err.used", &vars));
        }
        if self.list("phases").len() <= 1 {
            return Err(t0("phase.err.last"));
        }
        arr_mut(self.st(), "phases").retain(|p| !strict_eq(p.get("n"), ph));
        Ok(())
    }
    pub fn set_numbering(&mut self, o: Option<&Value>) {
        let pp = truthy(field(o, "perPhase"));
        self.st().insert("numbering".into(), json!({ "perPhase": pp }));
    }
    pub fn add_issues(&mut self, list: Option<&Value>) -> Res<()> {
        let items = list.and_then(Value::as_array).cloned().unwrap_or_default();
        let st = self.st();
        if !st.get("issues").is_some_and(Value::is_array) {
            st.insert("issues".into(), json!([]));
        }
        for x in items.iter().take(MAX_LIST) {
            let c = clean_issue(x).ok_or_else(|| t0("issue.err.bad"))?;
            arr_mut(self.st(), "issues").push(Value::Object(c));
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
        for (key, pre) in [("members", "member:"), ("footings", "footing:"), ("stairs", "stair:")] {
            if let Some(Value::Array(list)) = st.get_mut(key) {
                for x in list.iter_mut() {
                    let k = format!("{pre}{}", crate::js::string_of(x.get("id")));
                    if ids.contains(&k) {
                        m_of(x).insert("phase".into(), n(p));
                    }
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
        arr_mut(self.st(), "members").push(Value::Object(mem));
        Ok(Value::from(id))
    }
    pub fn update_member(&mut self, id: Option<&Value>, patch: Option<&Value>) -> Res<()> {
        let custom = self.profiles();
        let idx = self.list("members").iter().position(|x| strict_eq(x.get("id"), id)).ok_or_else(|| t0("model.err.member-gone"))?;
        let cur = self.list("members").get(idx).cloned().unwrap_or(Value::Null);
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
        if let Some(Value::Array(list)) = self.st().get_mut("members")
            && let Some(m) = list.get_mut(idx)
        {
            let mm = m_of(m);
            for (k, v) in next {
                mm.insert(k, v);
            }
        }
        Ok(())
    }
    pub fn remove_members(&mut self, ids: Option<&Value>) {
        let set = arr(ids).to_vec();
        let st = self.st();
        arr_mut(st, "members").retain(|m| !set_has(&set, m.get("id")));
        arr_mut(st, "connections").retain(|c| !members_of_v(c).iter().any(|id| set_has(&set, *id)));
    }
    pub fn add_hole(&mut self, member: Option<&Value>, h: Option<&Value>) -> Res<Value> {
        let idx = self.list("members").iter().position(|x| strict_eq(x.get("id"), member)).ok_or_else(|| t0("model.err.member-gone"))?;
        let mut hole = clean_hole(h).ok_or_else(|| t0("model.err.hole"))?;
        let id = self.next_id("h");
        hole.insert("id".into(), Value::from(id.clone()));
        if let Some(Value::Array(list)) = self.st().get_mut("members")
            && let Some(m) = list.get_mut(idx)
        {
            arr_mut(m_of(m), "holes").push(Value::Object(hole));
        }
        Ok(Value::from(id))
    }
    pub fn remove_hole(&mut self, member: Option<&Value>, hole: Option<&Value>) {
        if let Some(Value::Array(list)) = self.st().get_mut("members")
            && let Some(m) = list.iter_mut().find(|x| strict_eq(x.get("id"), member))
        {
            arr_mut(m_of(m), "holes").retain(|h| !strict_eq(h.get("id"), hole));
        }
    }
    pub fn add_cut(&mut self, member: Option<&Value>, c: Option<&Value>) -> Res<Value> {
        let idx = self.list("members").iter().position(|x| strict_eq(x.get("id"), member)).ok_or_else(|| t0("model.err.member-gone"))?;
        let mut cut = clean_cut(c).ok_or_else(|| t0("model.err.cope"))?;
        let id = self.next_id("k");
        cut.insert("id".into(), Value::from(id.clone()));
        if let Some(Value::Array(list)) = self.st().get_mut("members")
            && let Some(m) = list.get_mut(idx)
        {
            arr_mut(m_of(m), "cuts").push(Value::Object(cut));
        }
        Ok(Value::from(id))
    }
    pub fn remove_cut(&mut self, member: Option<&Value>, cut: Option<&Value>) {
        if let Some(Value::Array(list)) = self.st().get_mut("members")
            && let Some(m) = list.iter_mut().find(|x| strict_eq(x.get("id"), member))
        {
            arr_mut(m_of(m), "cuts").retain(|h| !strict_eq(h.get("id"), cut));
        }
    }

    pub fn add_connection(&mut self, c: Option<&Value>) -> Res<Value> {
        let id = self.next_id("j");
        let ty = field(c, "type");
        let params_with = |kind: &str| -> Value {
            let base = defaults_of(kind).unwrap_or(Value::Null);
            let extra = field(c, "params").filter(|v| truthy(Some(v)));
            Value::Object(spread(Some(&base), extra))
        };
        let conns = self.list("connections").to_vec();
        let mut o = Map::new();
        let cv = c.cloned().unwrap_or(Value::Null);
        if is_beam_end_v(&cv) {
            if conns
                .iter()
                .any(|x| is_beam_end_v(x) && strict_eq(x.get("beam"), field(c, "beam")) && strict_eq(x.get("end"), field(c, "end")))
            {
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
            if conns
                .iter()
                .any(|x| x.get("type").and_then(Value::as_str) == Some("baseplate") && strict_eq(x.get("column"), field(c, "column")))
            {
                return Err(t0("model.err.base-taken"));
            }
            o.insert("id".into(), Value::from(id.clone()));
            o.insert("type".into(), Value::from("baseplate"));
            if let Some(b) = field(c, "column") {
                o.insert("column".into(), b.clone());
            }
            o.insert("params".into(), params_with("baseplate"));
        } else if ty.and_then(Value::as_str) == Some("splice") {
            let ms = self.list("members");
            let (a, b) = (field(c, "a"), field(c, "b"));
            if !ms.iter().any(|m| strict_eq(m.get("id"), a)) || !ms.iter().any(|m| strict_eq(m.get("id"), b)) || strict_eq(a, b) {
                return Err(t0("model.err.conn-gone"));
            }
            let a_end = if field(c, "aEnd").and_then(Value::as_str) == Some("start") { "start" } else { "end" };
            let a_end_v = Value::from(a_end);
            if conns.iter().any(|x| {
                x.get("type").and_then(Value::as_str) == Some("splice")
                    && ((strict_eq(x.get("a"), a) && strict_eq(x.get("aEnd"), Some(&a_end_v)))
                        || (strict_eq(x.get("a"), b) && strict_eq(x.get("b"), a))
                        || (strict_eq(x.get("a"), a) && strict_eq(x.get("b"), b)))
            }) {
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
            let ms = self.list("members");
            let (br, sp) = (field(c, "brace"), field(c, "support"));
            let brm = ms.iter().find(|m| strict_eq(m.get("id"), br));
            if brm.is_none_or(|m| m.get("type").and_then(Value::as_str) != Some("brace"))
                || !ms.iter().any(|m| strict_eq(m.get("id"), sp))
                || strict_eq(br, sp)
            {
                return Err(t0("model.err.conn-gone"));
            }
            let end = if field(c, "end").and_then(Value::as_str) == Some("start") { "start" } else { "end" };
            let end_v = Value::from(end);
            if conns.iter().any(|x| {
                x.get("type").and_then(Value::as_str) == Some("gusset")
                    && strict_eq(x.get("brace"), br)
                    && strict_eq(x.get("end"), Some(&end_v))
            }) {
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
        arr_mut(self.st(), "connections").push(Value::Object(o));
        Ok(Value::from(id))
    }
    pub fn update_connection(&mut self, id: Option<&Value>, params: Option<&Value>) -> Res<()> {
        let idx = self.list("connections").iter().position(|x| strict_eq(x.get("id"), id)).ok_or_else(|| t0("model.err.conn-gone"))?;
        let cur = self.list("connections").get(idx).cloned().unwrap_or(Value::Null);
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
        if let Some(Value::Array(list)) = self.st().get_mut("connections")
            && let Some(c) = list.get_mut(idx)
        {
            m_of(c).insert("params".into(), Value::Object(next));
        }
        Ok(())
    }
    pub fn remove_connections(&mut self, ids: Option<&Value>) {
        let set = arr(ids).to_vec();
        arr_mut(self.st(), "connections").retain(|c| !set_has(&set, c.get("id")));
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
        arr_mut(self.st(), "footings").push(Value::Object(o));
        Ok(Value::from(id))
    }
    pub fn update_footing(&mut self, id: Option<&Value>, patch: Option<&Value>) -> Res<()> {
        let i = self.list("footings").iter().position(|x| strict_eq(x.get("id"), id)).ok_or_else(|| t0("model.err.footing-gone"))?;
        let f = self.list("footings").get(i).cloned().unwrap_or(Value::Null);
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
        if let Some(slot) = arr_mut(self.st(), "footings").get_mut(i) {
            *slot = Value::Object(o);
        }
        Ok(())
    }
    pub fn remove_footings(&mut self, ids: Option<&Value>) {
        let set = arr(ids).to_vec();
        arr_mut(self.st(), "footings").retain(|c| !set_has(&set, c.get("id")));
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
            st.insert("stairs".into(), json!([]));
        }
        let mut o = Map::new();
        o.insert("id".into(), Value::from(id.clone()));
        o.extend(r.fields());
        o.insert("phase".into(), n(phase));
        arr_mut(self.st(), "stairs").push(Value::Object(o));
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
        if let Some(slot) = arr_mut(self.st(), "stairs").get_mut(i) {
            *slot = Value::Object(o);
        }
        Ok(())
    }
    pub fn add_bolt(&mut self, b: Option<&Value>) -> Res<Value> {
        let c = b.and_then(clean_bolt).ok_or_else(|| t0("model.err.bolt"))?;
        let st = self.st();
        if !st.get("bolts").is_some_and(Value::is_array) {
            st.insert("bolts".into(), json!([]));
        }
        let id = self.next_id("k");
        let mut o = Map::new();
        o.insert("id".into(), Value::from(id.clone()));
        o.extend(c);
        arr_mut(self.st(), "bolts").push(Value::Object(o));
        Ok(Value::from(id))
    }
    pub fn remove_bolts(&mut self, ids: Option<&Value>) {
        let set = ids.filter(|v| truthy(Some(v))).map(|v| arr(Some(v)).to_vec());
        let st = self.st();
        match set {
            Some(set) => {
                let keep: Vec<Value> = arr(st.get("bolts")).iter().filter(|b| !set_has(&set, b.get("id"))).cloned().collect();
                st.insert("bolts".into(), Value::Array(keep));
            }
            None => {
                st.insert("bolts".into(), json!([]));
            }
        }
    }
    pub fn remove_stairs(&mut self, ids: Option<&Value>) {
        let set = arr(ids).to_vec();
        let st = self.st();
        let keep: Vec<Value> = arr(st.get("stairs")).iter().filter(|x| !set_has(&set, x.get("id"))).cloned().collect();
        st.insert("stairs".into(), Value::Array(keep));
    }
    pub fn add_profile(&mut self, p: Option<&Value>) -> Res<()> {
        let r = clean_custom(p.unwrap_or(&Value::Null), false)?;
        let name = upper(s(r.get("name")));
        if self.list("profiles").iter().any(|x| upper(s(x.get("name"))) == name) {
            return Err(t("model.err.profile-taken", &[("name", F::from(s(r.get("name"))))]));
        }
        arr_mut(self.st(), "profiles").push(r);
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
pub fn read_model(input: &Value) -> Res<(Value, Vec<String>)> {
    let parsed;
    let j: &Value = match input {
        Value::String(text) => {
            parsed = parse(text).map_err(|_| t0("model.err.not-json"))?;
            &parsed
        }
        v => v,
    };
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
    let mut ids: Vec<String> = Vec::new();
    let mut id_set: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut uid = |id: Option<&Value>, prefix: &str| -> String {
        let mut v = clean_text(id, 20);
        let ok = {
            let mut cs = v.chars();
            cs.next().is_some_and(|c| c.is_ascii_lowercase()) && {
                let rest: Vec<char> = cs.collect();
                !rest.is_empty() && rest.iter().all(char::is_ascii_digit)
            }
        };
        if !ok || id_set.contains(&v) {
            v = format!("{prefix}{}", 100_000usize.saturating_add(id_set.len()));
        }
        if id_set.insert(v.clone()) {
            ids.push(v.clone());
        }
        v
    };
    let mut members: Vec<Value> = Vec::new();
    let mut dropped = 0usize;
    for m in list_of(j.get("members"), 20000) {
        if m.is_null() {
            dropped = dropped.saturating_add(1);
            continue;
        }
        let mo = Some(m);
        let mut mem = Map::new();
        mem.insert("id".into(), Value::from(uid(field(mo, "id"), "m")));
        let ty = if includes(&MEMBER_TYPES, field(mo, "type")) {
            field(mo, "type").cloned().unwrap_or(Value::Null)
        } else {
            Value::from("beam")
        };
        mem.insert("type".into(), ty);
        mem.insert("profile".into(), Value::from(clean_text(field(mo, "profile"), 40)));
        let mat = clean_text(field(mo, "material"), 30);
        mem.insert("material".into(), Value::from(if mat.is_empty() { "A992".to_string() } else { mat }));
        let (Ok(start), Ok(end)) = (vec3(field(mo, "start")), vec3(field(mo, "end"))) else {
            // the end's uid calls never happen when start or end throws
            dropped = dropped.saturating_add(1);
            continue;
        };
        mem.insert("start".into(), start);
        mem.insert("end".into(), end);
        mem.insert("rotation".into(), n(num_or0(field(mo, "rotation"))));
        let pos = if includes(&["top", "middle", "bottom"], field(mo, "position")) {
            field(mo, "position").cloned().unwrap_or(Value::Null)
        } else {
            Value::from("middle")
        };
        mem.insert("position".into(), pos);
        mem.insert("name".into(), Value::from(clean_text(field(mo, "name"), 40)));
        let mut cuts = Vec::new();
        for c in list_of(field(mo, "cuts"), 50) {
            if let Some(mut k) = clean_cut(Some(c)) {
                k.insert("id".into(), Value::from(uid(field(Some(c), "id"), "k")));
                cuts.push(Value::Object(k));
            }
        }
        mem.insert("cuts".into(), Value::Array(cuts));
        let mut holes = Vec::new();
        for h in list_of(field(mo, "holes"), 2000) {
            if let Some(mut k) = clean_hole(Some(h)) {
                k.insert("id".into(), Value::from(uid(field(Some(h), "id"), "h")));
                holes.push(Value::Object(k));
            }
        }
        mem.insert("holes".into(), Value::Array(holes));
        mem.insert("phase".into(), n(clean_phase(field(mo, "phase")).unwrap_or(1.0)));
        match check_member(&mut mem, &profiles) {
            Ok(()) => members.push(Value::Object(mem)),
            Err(_) => dropped = dropped.saturating_add(1),
        }
    }
    if dropped > 0 {
        warnings.push(t("model.warn.members", &[("n", F::N(dropped as f64))]));
    }
    let member_ids: Vec<String> = members.iter().map(|m| s_(m.get("id")).to_string()).collect();
    let has_member = |v: Option<&Value>| v.and_then(Value::as_str).is_some_and(|x| member_ids.iter().any(|m| m == x));
    let mut conns = Vec::new();
    let mut dropped_c = 0usize;
    for c in list_of(j.get("connections"), 20000) {
        let ty = field(Some(c), "type").and_then(Value::as_str).unwrap_or("");
        let base = field(Some(c), "type").and_then(Value::as_str).and_then(defaults_of);
        let Some(base) = base else {
            dropped_c = dropped_c.saturating_add(1);
            continue;
        };
        if !members_of_v(c).iter().all(|id| has_member(*id))
            || (ty == "splice" && strict_eq(c.get("a"), c.get("b")))
            || (ty == "gusset" && strict_eq(c.get("brace"), c.get("support")))
        {
            dropped_c = dropped_c.saturating_add(1);
            continue;
        }
        let mut params = base.as_object().cloned().unwrap_or_default();
        let given = field(Some(c), "params");
        if let Some(bo) = base.as_object() {
            for (k, bv) in bo {
                let Some(v) = field(given, k) else { continue };
                if bv.is_number() {
                    let x = num(Some(v));
                    if x.is_finite() && (0.0..=10000.0).contains(&x) {
                        params.insert(k.clone(), n(x));
                    }
                } else {
                    let cv = clean_text(Some(v), 30);
                    params.insert(k.clone(), if cv.is_empty() { bv.clone() } else { Value::from(cv) });
                }
            }
        }
        let mut o = Map::new();
        let end_of = |k: &str| Value::from(if c.get(k).and_then(Value::as_str) == Some("start") { "start" } else { "end" });
        o.insert("id".into(), Value::from(uid(c.get("id"), "j")));
        match ty {
            "splice" => {
                o.insert("type".into(), Value::from("splice"));
                o.insert("a".into(), c.get("a").cloned().unwrap_or(Value::Null));
                o.insert("aEnd".into(), end_of("aEnd"));
                o.insert("b".into(), c.get("b").cloned().unwrap_or(Value::Null));
                o.insert("params".into(), Value::Object(params));
            }
            "gusset" => {
                o.insert("type".into(), Value::from("gusset"));
                o.insert("brace".into(), c.get("brace").cloned().unwrap_or(Value::Null));
                o.insert("end".into(), end_of("end"));
                o.insert("support".into(), c.get("support").cloned().unwrap_or(Value::Null));
                o.insert("params".into(), Value::Object(params));
            }
            _ => {
                o.insert("type".into(), Value::from(ty));
                if let Some(v) = c.get("column") {
                    o.insert("column".into(), v.clone());
                }
                o.insert("params".into(), Value::Object(params));
            }
        }
        if is_beam_end_v(c) {
            o.insert("beam".into(), c.get("beam").cloned().unwrap_or(Value::Null));
            o.insert("end".into(), end_of("end"));
        }
        conns.push(Value::Object(o));
    }
    if dropped_c > 0 {
        warnings.push(t("model.warn.connections", &[("n", F::N(dropped_c as f64))]));
    }
    let mut footings = Vec::new();
    for f in list_of(j.get("footings"), 5000) {
        match clean_footing(f) {
            Ok(r) => {
                let mut o = Map::new();
                o.insert("id".into(), Value::from(uid(field(Some(f), "id"), "f")));
                o.extend(r.fields());
                o.insert("phase".into(), n(clean_phase(field(Some(f), "phase")).unwrap_or(1.0)));
                footings.push(Value::Object(o));
            }
            Err(e) => warnings.push(t("model.warn.footing", &[("error", F::from(e))])),
        }
    }
    let mut bolts = Vec::new();
    for b in list_of(j.get("bolts"), 200_000) {
        if let Some(c) = clean_bolt(b) {
            let mut o = Map::new();
            o.insert("id".into(), Value::from(uid(field(Some(b), "id"), "k")));
            o.extend(c);
            bolts.push(Value::Object(o));
        }
    }
    let mut stairs = Vec::new();
    for x in list_of(j.get("stairs"), 2000) {
        match clean_stair(x) {
            Ok(r) => {
                let mut o = Map::new();
                o.insert("id".into(), Value::from(uid(field(Some(x), "id"), "s")));
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
    put(&mut s, "members", Value::Array(members));
    put(&mut s, "connections", Value::Array(conns));
    put(&mut s, "footings", Value::Array(footings));
    put(&mut s, "bolts", Value::Array(bolts));
    put(&mut s, "stairs", Value::Array(stairs));
    let base_ph = if ph.is_empty() { vec![json!({ "n": 1, "name": "" })] } else { ph };
    let phases = with_used_phases(&base_ph, &s);
    put(&mut s, "phases", Value::Array(phases));
    let pp = field(j.get("numbering"), "perPhase").and_then(Value::as_bool) != Some(false);
    put(&mut s, "numbering", json!({ "perPhase": pp }));
    put(&mut s, "issues", Value::Array(list_of(j.get("issues"), 20000).iter().filter_map(clean_issue).map(Value::Object).collect()));
    let mut max_seq = 0.0;
    for id in &ids {
        let rest: String = id.chars().skip(1).collect();
        max_seq = crate::js::max(max_seq, crate::js::or(str_to_number(&rest), 0.0));
    }
    let seq0 = crate::js::or(num(j.get("seq")), 1.0);
    put(&mut s, "seq", n(crate::js::max(seq0, max_seq + 1.0)));
    Ok((s, warnings))
}

fn s_(v: Option<&Value>) -> &str {
    s(v)
}
