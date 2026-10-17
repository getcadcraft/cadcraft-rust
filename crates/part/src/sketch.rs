//! sketch.js — the constrained 2D sketch as plain JSON `{seq, entities, constraints}`: building (points, lines, circles,
//! arcs, rectangles, splines), relations and dimensions (`CONSTRAINTS`, signature matching, measuring), removal, the
//! sketch fillet, profiles (closed loops for the kernel), even-odd insideness and the untrusted-file reader.
//! The residual equations live with the solver ([`crate::solver`]), compiled once per solve.
//!
//! The origin is the virtual fixed point `'O'`. Reading a field a broken in-memory sketch lacks gives NaN (the JS reads
//! `undefined`); files go through [`read_sketch`] first, which checks every reference.

use crate::i18n::{F, t};
use crate::js;
use crate::jv;
use crate::spline::{self as sp, Curve, P2};
use serde_json::Value;
use std::collections::HashSet;

pub const ORIGIN: &str = "O";

/// The names `Object.prototype` holds: a plain-object table "has" them in the JS (`CONSTRAINTS.constructor` is truthy).
pub fn proto_key(s: &str) -> bool {
    matches!(
        s,
        "constructor"
            | "__defineGetter__"
            | "__defineSetter__"
            | "hasOwnProperty"
            | "__lookupGetter__"
            | "__lookupSetter__"
            | "isPrototypeOf"
            | "propertyIsEnumerable"
            | "toString"
            | "valueOf"
            | "__proto__"
            | "toLocaleString"
    )
}

/// `String(obj[key])` for an `Object.prototype` name: V8's text of the inherited value.
pub fn proto_text(s: &str) -> String {
    match s {
        "constructor" => "function Object() { [native code] }".into(),
        "__proto__" => "[object Object]".into(),
        k => format!("function {k}() {{ [native code] }}"),
    }
}

/// One row of `CONSTRAINTS`.
#[derive(Clone, Copy, Debug)]
pub struct Def {
    pub key: &'static str,
    pub label_key: &'static str,
    pub sigs: &'static [&'static str],
    pub dim: bool,
    pub auto: bool,
}

/// `CONSTRAINTS`, in the JS table's order.
pub const CONSTRAINTS: [Def; 19] = [
    Def { key: "coincident", label_key: "model.sketch.rel.coincident", sigs: &["PP", "PL", "PR"], dim: false, auto: false },
    Def { key: "horizontal", label_key: "model.sketch.rel.horizontal", sigs: &["L", "PP"], dim: false, auto: false },
    Def { key: "vertical", label_key: "model.sketch.rel.vertical", sigs: &["L", "PP"], dim: false, auto: false },
    Def { key: "parallel", label_key: "model.sketch.rel.parallel", sigs: &["LL"], dim: false, auto: false },
    Def { key: "perpendicular", label_key: "model.sketch.rel.perpendicular", sigs: &["LL"], dim: false, auto: false },
    Def { key: "tangent", label_key: "model.sketch.rel.tangent", sigs: &["LR", "RR", "LS", "RS", "SS"], dim: false, auto: false },
    Def { key: "equal", label_key: "model.sketch.rel.equal", sigs: &["LL", "RR"], dim: false, auto: false },
    Def { key: "concentric", label_key: "model.sketch.rel.concentric", sigs: &["RR", "PR"], dim: false, auto: false },
    Def { key: "midpoint", label_key: "model.sketch.rel.midpoint", sigs: &["PL"], dim: false, auto: false },
    Def { key: "fix", label_key: "model.sketch.rel.fix", sigs: &["P", "L", "R", "S"], dim: false, auto: false },
    Def { key: "symmetric", label_key: "model.sketch.rel.symmetric", sigs: &["PPL"], dim: false, auto: false },
    Def { key: "offsetlink", label_key: "model.sketch.rel.offsetlink", sigs: &["LL", "RR"], dim: false, auto: true },
    Def { key: "distance", label_key: "model.sketch.rel.distance", sigs: &["PP", "PL", "L", "LL"], dim: true, auto: false },
    Def { key: "hdistance", label_key: "model.sketch.rel.hdistance", sigs: &["PP", "L"], dim: true, auto: false },
    Def { key: "vdistance", label_key: "model.sketch.rel.vdistance", sigs: &["PP", "L"], dim: true, auto: false },
    Def { key: "radius", label_key: "model.sketch.rel.radius", sigs: &["R"], dim: true, auto: false },
    Def { key: "diameter", label_key: "model.sketch.rel.diameter", sigs: &["R"], dim: true, auto: false },
    Def { key: "angle", label_key: "model.sketch.rel.angle", sigs: &["LL"], dim: true, auto: false },
    Def { key: "offset", label_key: "model.sketch.rel.offset", sigs: &["LL", "RR"], dim: true, auto: true },
];
pub const RELATIONS: [&str; 11] =
    ["coincident", "horizontal", "vertical", "parallel", "perpendicular", "tangent", "equal", "concentric", "midpoint", "symmetric", "fix"];
pub const DIMENSIONS: [&str; 6] = ["distance", "hdistance", "vdistance", "radius", "diameter", "angle"];

/// `CONSTRAINTS[type]`: Some(Ok(def)), Some(Err(())) for an `Object.prototype` name (truthy, no fields), None.
pub fn def_of(ty: &str) -> Option<Result<&'static Def, ()>> {
    if let Some(d) = CONSTRAINTS.iter().find(|d| d.key == ty) {
        return Some(Ok(d));
    }
    if proto_key(ty) { Some(Err(())) } else { None }
}
/// `CONSTRAINTS[type].dim` (truthy).
pub fn is_dim(ty: &str) -> bool {
    matches!(def_of(ty), Some(Ok(d)) if d.dim)
}
/// `CONSTRAINTS[type]?.label`.
pub fn label_of(ty: &str) -> Option<String> {
    match def_of(ty) {
        Some(Ok(d)) => Some(t(d.label_key, &[])),
        _ => None,
    }
}

/// `newSketch()`.
pub fn new_sketch() -> Value {
    jv::obj(vec![("seq", 1.into()), ("entities", Value::Array(Vec::new())), ("constraints", Value::Array(Vec::new()))])
}

// ---------------------------------------------------------------- lookup

/// An entity as `byId` returns it: the virtual origin, or a stored entity.
#[derive(Clone, Copy, Debug)]
pub enum Ent<'a> {
    Origin,
    E(&'a Value),
}
impl<'a> Ent<'a> {
    pub fn ty(&self) -> &'a str {
        match self {
            Ent::Origin => "point",
            Ent::E(e) => jv::s(e, "type"),
        }
    }
    pub fn id(&self) -> &'a str {
        match self {
            Ent::Origin => ORIGIN,
            Ent::E(e) => jv::s(e, "id"),
        }
    }
    pub fn num(&self, k: &str) -> f64 {
        match self {
            Ent::Origin => match k {
                "x" | "y" => 0.0,
                _ => f64::NAN,
            },
            Ent::E(e) => jv::n(e, k),
        }
    }
    pub fn str(&self, k: &str) -> &'a str {
        match self {
            Ent::Origin => "",
            Ent::E(e) => jv::s(e, k),
        }
    }
    pub fn val(&self, k: &str) -> Option<&'a Value> {
        match self {
            Ent::Origin => None,
            Ent::E(e) => e.get(k),
        }
    }
    pub fn construction(&self) -> bool {
        jv::truthy(self.val("construction"))
    }
    pub fn value(&self) -> Value {
        match self {
            Ent::Origin => {
                jv::obj(vec![("id", ORIGIN.into()), ("type", "point".into()), ("x", 0.into()), ("y", 0.into()), ("origin", true.into())])
            }
            Ent::E(e) => (*e).clone(),
        }
    }
}

pub fn entities(sk: &Value) -> &[Value] {
    jv::arr(sk.get("entities"))
}
pub fn constraints(sk: &Value) -> &[Value] {
    jv::arr(sk.get("constraints"))
}
pub fn entities_mut(sk: &mut Value) -> Option<&mut Vec<Value>> {
    sk.get_mut("entities").and_then(Value::as_array_mut)
}
pub fn constraints_mut(sk: &mut Value) -> Option<&mut Vec<Value>> {
    sk.get_mut("constraints").and_then(Value::as_array_mut)
}

/// `byId(sk, id)`.
pub fn by_id<'a>(sk: &'a Value, id: &str) -> Option<Ent<'a>> {
    if id == ORIGIN {
        return Some(Ent::Origin);
    }
    entities(sk).iter().find(|e| jv::is_str(e.get("id"), id)).map(Ent::E)
}
pub fn by_id_mut<'a>(sk: &'a mut Value, id: &str) -> Option<&'a mut Value> {
    entities_mut(sk)?.iter_mut().find(|e| jv::is_str(e.get("id"), id))
}
/// `kindOf(e)`: P / L / S / R.
pub fn kind_of(e: &Ent) -> char {
    match e.ty() {
        "point" => 'P',
        "line" => 'L',
        "spline" => 'S',
        _ => 'R',
    }
}
/// `pointXY(sk, id)`.
pub fn point_xy(sk: &Value, id: &str) -> Option<P2> {
    by_id(sk, id).map(|p| [p.num("x"), p.num("y")])
}
/// `pointXY` with NaN for a missing point.
pub fn pxy(sk: &Value, id: &str) -> P2 {
    point_xy(sk, id).unwrap_or([f64::NAN, f64::NAN])
}
/// `radiusOf(sk, e)`.
pub fn radius_of(sk: &Value, e: &Ent) -> f64 {
    if e.ty() == "circle" {
        return e.num("r");
    }
    let c = pxy(sk, e.str("c"));
    let s = pxy(sk, e.str("s"));
    js::hypot2(s[0] - c[0], s[1] - c[1])
}
/// `pointsOf(e)`.
pub fn points_of(e: &Ent) -> Vec<String> {
    let st = |k: &str| e.str(k).to_string();
    match e.ty() {
        "point" => vec![e.id().to_string()],
        "line" => vec![st("p1"), st("p2")],
        "circle" => vec![st("c")],
        "spline" => jv::arr(e.val("pts")).iter().map(|v| js::str_of(Some(v))).collect(),
        _ => vec![st("c"), st("s"), st("e")],
    }
}
/// `splineMin(e)`.
pub fn spline_min(e: &Ent) -> usize {
    if jv::truthy(e.val("closed")) { 3 } else { 2 }
}

fn spline_pts(e: &Ent) -> Vec<String> {
    jv::arr(e.val("pts")).iter().map(|v| js::str_of(Some(v))).collect()
}

/// `splineCurve(sk, e, Pget)`.
pub fn spline_curve_with(e: &Ent, pget: &dyn Fn(&str) -> P2, aget: Option<&dyn Fn(&str) -> Option<f64>>) -> Result<Curve, String> {
    let q: Vec<P2> = spline_pts(e).iter().map(|id| pget(id)).collect();
    let closed = jv::truthy(e.val("closed"));
    if e.str("kind") == "control" {
        let knots: Option<Vec<f64>> = if jv::truthy(e.val("knots")) {
            Some(jv::arr(e.val("knots")).iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect())
        } else {
            None
        };
        return Ok(sp::control_curve(&q, closed, knots.as_deref()));
    }
    let ang = |k: &str| -> Option<f64> {
        match aget {
            Some(f) => f(k),
            None => jv::finite(e.val(k)),
        }
        .filter(|x| x.is_finite())
    };
    sp::interp_fit(&q, closed, ang("a0"), ang("a1"))
}
/// `splineCurve(sk, e)`.
pub fn spline_curve(sk: &Value, e: &Ent) -> Result<Curve, String> {
    spline_curve_with(e, &|id| pxy(sk, id), None)
}

/// `splineEndTangent(sk, e, end, Pget, Aget)`.
pub fn spline_end_tangent_with(
    e: &Ent,
    end: char,
    pget: &dyn Fn(&str) -> P2,
    aget: Option<&dyn Fn(&str) -> Option<f64>>,
) -> Result<P2, String> {
    let key = if end == 's' { "a0" } else { "a1" };
    let ang = match aget {
        Some(f) => f(key),
        None => jv::as_num(e.val(key)),
    };
    if e.str("kind") == "fit"
        && let Some(a) = ang.filter(|x| x.is_finite())
    {
        return Ok([js::cos(a), js::sin(a)]);
    }
    if e.str("kind") == "control" {
        let p: Vec<P2> = spline_pts(e).iter().map(|id| pget(id)).collect();
        let n = p.len();
        let g = |i: usize| p.get(i).copied().unwrap_or([f64::NAN, f64::NAN]);
        let d = if end == 's' {
            [g(1)[0] - g(0)[0], g(1)[1] - g(0)[1]]
        } else {
            [g(n.saturating_sub(1))[0] - g(n.saturating_sub(2))[0], g(n.saturating_sub(1))[1] - g(n.saturating_sub(2))[1]]
        };
        let l = js::hypot2(d[0], d[1]);
        let l = if l == 0.0 || l.is_nan() { 1e-300 } else { l };
        return Ok([d[0] / l, d[1] / l]);
    }
    let c = spline_curve_with(e, pget, aget)?;
    let dom = sp::domain(&c);
    let d = sp::derivs_at(&c, if end == 's' { dom[0] } else { dom[1] }, 1);
    let tt = d.get(1).copied().unwrap_or([f64::NAN, f64::NAN]);
    let l = js::hypot2(tt[0], tt[1]);
    let l = if l == 0.0 || l.is_nan() { 1e-300 } else { l };
    Ok([tt[0] / l, tt[1] / l])
}
/// `splineEndTangent(sk, e, end)`.
pub fn spline_end_tangent(sk: &Value, e: &Ent, end: char) -> Result<P2, String> {
    spline_end_tangent_with(e, end, &|id| pxy(sk, id), None)
}

/// `tessSpline(sk, e, perSpan)`.
pub fn tess_spline(sk: &Value, e: &Ent, per_span: usize) -> Result<Vec<P2>, String> {
    Ok(sp::tessellate(&spline_curve(sk, e)?, per_span))
}

/// `loosePoints(sk)`: points no curve uses.
pub fn loose_points(sk: &Value) -> Vec<&Value> {
    let mut used: HashSet<String> = HashSet::new();
    for e in entities(sk) {
        let en = Ent::E(e);
        if en.ty() != "point" {
            used.extend(points_of(&en));
        }
    }
    entities(sk).iter().filter(|e| jv::s(e, "type") == "point" && !used.contains(jv::s(e, "id"))).collect()
}

/// `arcAngles(sk, a)` → (c, r, a0, sweep).
pub fn arc_angles(sk: &Value, a: &Ent) -> (P2, f64, f64, f64) {
    let c = pxy(sk, a.str("c"));
    let s = pxy(sk, a.str("s"));
    let e = pxy(sk, a.str("e"));
    let a0 = js::atan2(s[1] - c[1], s[0] - c[0]);
    let a1 = js::atan2(e[1] - c[1], e[0] - c[0]);
    let mut sweep = a1 - a0;
    let tau = 2.0 * std::f64::consts::PI;
    let mut guard = 0u32;
    while sweep <= 1e-12 && guard < 1000 {
        sweep += tau;
        guard = guard.saturating_add(1);
    }
    while sweep > tau && guard < 2000 {
        sweep -= tau;
        guard = guard.saturating_add(1);
    }
    (c, js::hypot2(s[0] - c[0], s[1] - c[1]), a0, sweep)
}

// ---------------------------------------------------------------- building

pub fn next_id(sk: &mut Value, prefix: &str) -> String {
    let n = jv::n(sk, "seq");
    jv::set(sk, "seq", jv::num(n + 1.0));
    format!("{prefix}{}", js::num_str(n))
}

fn push_entity(sk: &mut Value, e: Value) {
    if let Some(v) = entities_mut(sk) {
        v.push(e);
    }
}

/// `addPoint(sk, x, y, opts)`.
pub fn add_point(sk: &mut Value, x: f64, y: f64, construction: bool) -> String {
    let id = next_id(sk, "p");
    let mut p = jv::obj(vec![("id", id.as_str().into()), ("type", "point".into()), ("x", jv::num(x)), ("y", jv::num(y))]);
    if construction {
        jv::set(&mut p, "construction", true.into());
    }
    push_entity(sk, p);
    id
}

/// A position: [x, y] (a new point) or an existing point id.
#[derive(Clone, Debug)]
pub enum At {
    Xy(P2),
    Id(String),
}
impl At {
    pub fn from_json(v: &Value) -> At {
        match v {
            Value::String(s) => At::Id(s.clone()),
            _ => At::Xy([
                jv::arr(Some(v)).first().and_then(Value::as_f64).unwrap_or(f64::NAN),
                jv::arr(Some(v)).get(1).and_then(Value::as_f64).unwrap_or(f64::NAN),
            ]),
        }
    }
    fn xy(&self, sk: &Value) -> P2 {
        match self {
            At::Xy(p) => *p,
            At::Id(id) => pxy(sk, id),
        }
    }
}

fn end_point(sk: &mut Value, v: &At, made: &mut Vec<String>) -> Result<String, String> {
    match v {
        At::Id(src) => {
            let q = pxy(sk, src);
            let id = add_point(sk, q[0], q[1], false);
            let c = add_constraint(sk, "coincident", &[id.clone(), src.clone()], None, None)?;
            made.push(jv::s(&c, "id").to_string());
            Ok(id)
        }
        At::Xy(p) => Ok(add_point(sk, p[0], p[1], false)),
    }
}

/// `addLine(sk, a, b, opts)` → { id, p1, p2, constraints }.
pub fn add_line(sk: &mut Value, a: &At, b: &At, construction: bool) -> Result<Value, String> {
    let mut made = Vec::new();
    let p1 = end_point(sk, a, &mut made)?;
    let p2 = end_point(sk, b, &mut made)?;
    let id = next_id(sk, "l");
    let mut l = jv::obj(vec![("id", id.as_str().into()), ("type", "line".into()), ("p1", p1.as_str().into()), ("p2", p2.as_str().into())]);
    if construction {
        jv::set(&mut l, "construction", true.into());
    }
    push_entity(sk, l);
    Ok(jv::obj(vec![("id", id.into()), ("p1", p1.into()), ("p2", p2.into()), ("constraints", jv::strs(&made))]))
}

/// `addCircle(sk, c, r, opts)` → { id, c, constraints }.
pub fn add_circle(sk: &mut Value, c: &At, r: f64, construction: bool) -> Result<Value, String> {
    let mut made = Vec::new();
    let cid = end_point(sk, c, &mut made)?;
    let id = next_id(sk, "c");
    let mut e = jv::obj(vec![("id", id.as_str().into()), ("type", "circle".into()), ("c", cid.as_str().into()), ("r", jv::num(r.abs()))]);
    if construction {
        jv::set(&mut e, "construction", true.into());
    }
    push_entity(sk, e);
    Ok(jv::obj(vec![("id", id.into()), ("c", cid.into()), ("constraints", jv::strs(&made))]))
}

/// `addArc(sk, c, s, e, opts)`: counter-clockwise about c from s; e is put on the radius through it.
pub fn add_arc(sk: &mut Value, c: &At, s: &At, e: &At, construction: bool) -> Result<Value, String> {
    let mut made = Vec::new();
    let cxy = c.xy(sk);
    let sxy = s.xy(sk);
    let r = js::hypot2(sxy[0] - cxy[0], sxy[1] - cxy[1]);
    let exy0 = e.xy(sk);
    let ang = js::atan2(exy0[1] - cxy[1], exy0[0] - cxy[0]);
    let exy = [cxy[0] + r * js::cos(ang), cxy[1] + r * js::sin(ang)];
    let cid = end_point(sk, c, &mut made)?;
    let sid = end_point(sk, s, &mut made)?;
    let eid = match e {
        At::Id(_) => end_point(sk, e, &mut made)?,
        At::Xy(_) => add_point(sk, exy[0], exy[1], false),
    };
    let id = next_id(sk, "a");
    let mut a = jv::obj(vec![
        ("id", id.as_str().into()),
        ("type", "arc".into()),
        ("c", cid.as_str().into()),
        ("s", sid.as_str().into()),
        ("e", eid.as_str().into()),
    ]);
    if construction {
        jv::set(&mut a, "construction", true.into());
    }
    push_entity(sk, a);
    Ok(jv::obj(vec![("id", id.into()), ("c", cid.into()), ("s", sid.into()), ("e", eid.into()), ("constraints", jv::strs(&made))]))
}

/// `addRect(sk, a, b, opts)` → { lines, constraints }.
pub fn add_rect(sk: &mut Value, a: P2, b: P2, construction: bool) -> Result<Value, String> {
    let pts = [[a[0], a[1]], [b[0], a[1]], [b[0], b[1]], [a[0], b[1]]];
    let mut lines = Vec::new();
    for i in 0..4 {
        let p = pts.get(i).copied().unwrap_or([0.0; 2]);
        let q = pts.get(js::rem(i.saturating_add(1), 4)).copied().unwrap_or([0.0; 2]);
        lines.push(add_line(sk, &At::Xy(p), &At::Xy(q), construction)?);
    }
    let g = |i: usize, k: &str| lines.get(i).map(|l| jv::s(l, k).to_string()).unwrap_or_default();
    let mut made = Vec::new();
    for i in 0..4 {
        let c = add_constraint(sk, "coincident", &[g(i, "p2"), g(js::rem(i.saturating_add(1), 4), "p1")], None, None)?;
        made.push(jv::s(&c, "id").to_string());
    }
    for (ty, i) in [("horizontal", 0), ("horizontal", 2), ("vertical", 1), ("vertical", 3)] {
        let c = add_constraint(sk, ty, &[g(i, "id")], None, None)?;
        made.push(jv::s(&c, "id").to_string());
    }
    let ids: Vec<String> = (0..4).map(|i| g(i, "id")).collect();
    Ok(jv::obj(vec![("lines", jv::strs(&ids)), ("constraints", jv::strs(&made))]))
}

/// `addSpline(sk, pts, opts)` → { id, pts }.
pub fn add_spline(sk: &mut Value, pts: &[At], opts: &Value) -> Result<Value, String> {
    let kind = if jv::is_str(opts.get("kind"), "control") { "control" } else { "fit" };
    let closed = jv::truthy(opts.get("closed"));
    let mut ids = Vec::new();
    for v in pts {
        ids.push(match v {
            At::Id(s) => s.clone(),
            At::Xy(p) => add_point(sk, p[0], p[1], false),
        });
    }
    if ids.len() < if closed { 3 } else { 2 } {
        return Err(if closed { t("model.sketch_spline.closed_three", &[]) } else { t("model.sketch_spline.open_two", &[]) });
    }
    let id = next_id(sk, "s");
    let mut e = jv::obj(vec![("id", id.as_str().into()), ("type", "spline".into()), ("kind", kind.into()), ("pts", jv::strs(&ids))]);
    if closed {
        jv::set(&mut e, "closed", true.into());
    }
    if jv::truthy(opts.get("construction")) {
        jv::set(&mut e, "construction", true.into());
    }
    if kind == "fit" && !closed {
        if let Some(a) = jv::finite(opts.get("a0")) {
            jv::set(&mut e, "a0", jv::num(a));
        }
        if let Some(a) = jv::finite(opts.get("a1")) {
            jv::set(&mut e, "a1", jv::num(a));
        }
    }
    if kind == "control" && !closed && jv::truthy(opts.get("knots")) {
        jv::set(&mut e, "knots", opts.get("knots").cloned().unwrap_or(Value::Null));
    }
    spline_curve(sk, &Ent::E(&e))?;
    push_entity(sk, e);
    Ok(jv::obj(vec![("id", id.into()), ("pts", jv::strs(&ids))]))
}

/// `matchSig(sk, type, refs)` → (sig, refs ordered P < L < R < S).
pub fn match_sig(sk: &Value, ty: &str, refs: &[String]) -> Result<Option<(String, Vec<String>)>, String> {
    let Some(def) = def_of(ty) else { return Ok(None) };
    let mut pairs: Vec<(String, char)> = Vec::with_capacity(refs.len());
    for r in refs {
        match by_id(sk, r) {
            Some(e) => pairs.push((r.clone(), kind_of(&e))),
            None => return Ok(None),
        }
    }
    let order = |k: char| match k {
        'P' => 0,
        'L' => 1,
        'R' => 2,
        _ => 3,
    };
    pairs.sort_by_key(|p| order(p.1));
    let sig: String = pairs.iter().map(|p| p.1).collect();
    let Ok(def) = def else { return Err("Cannot read properties of undefined (reading 'includes')".into()) };
    if !def.sigs.contains(&sig.as_str()) {
        return Ok(None);
    }
    let uniq: HashSet<&String> = refs.iter().collect();
    if uniq.len() != refs.len() {
        return Ok(None);
    }
    Ok(Some((sig, pairs.into_iter().map(|p| p.0).collect())))
}
fn match_sig_ok(sk: &Value, ty: &str, refs: &[String]) -> Option<(String, Vec<String>)> {
    match_sig(sk, ty, refs).ok().flatten()
}

/// `applicable(sk, refs)`.
pub fn applicable(sk: &Value, refs: &[String]) -> Vec<&'static str> {
    let mut out = Vec::new();
    for d in &CONSTRAINTS {
        if d.auto {
            continue;
        }
        let Some((sig, mr)) = match_sig_ok(sk, d.key, refs) else { continue };
        let r = |i: usize| mr.get(i).map_or("", String::as_str);
        if d.key == "tangent" && sig.contains('S') {
            let (Some(a), Some(b)) = (by_id(sk, r(0)), by_id(sk, r(1))) else { continue };
            if end_meet(sk, &a, &b).is_none() {
                continue;
            }
        }
        if d.key == "symmetric" && r(0) == ORIGIN && r(1) == ORIGIN {
            continue;
        }
        if d.key == "distance" && sig == "LL" && !nearly_parallel(sk, r(0), r(1)) {
            continue;
        }
        if d.key == "angle" && nearly_parallel(sk, r(0), r(1)) {
            continue;
        }
        out.push(d.key);
    }
    out
}

fn line_vec(sk: &Value, id: &str) -> P2 {
    let l = by_id(sk, id);
    let (a, b) = match l {
        Some(l) => (pxy(sk, l.str("p1")), pxy(sk, l.str("p2"))),
        None => ([f64::NAN; 2], [f64::NAN; 2]),
    };
    [b[0] - a[0], b[1] - a[1]]
}
fn nearly_parallel(sk: &Value, l1: &str, l2: &str) -> bool {
    let u = line_vec(sk, l1);
    let v = line_vec(sk, l2);
    let den = js::hypot2(u[0], u[1]) * js::hypot2(v[0], v[1]);
    let den = if den == 0.0 || den.is_nan() { 1.0 } else { den };
    let c = (u[0] * v[1] - u[1] * v[0]) / den;
    c.abs() < 1e-6
}

/// What `measure` returns: `{ value?, sign?, kind? }`.
#[derive(Clone, Debug, Default)]
pub struct Meas {
    pub value: Option<MVal>,
    pub sign: Option<f64>,
    pub kind: Option<&'static str>,
}
#[derive(Clone, Debug)]
pub enum MVal {
    N(f64),
    A(Vec<f64>),
}
impl MVal {
    pub fn to_json(&self) -> Value {
        match self {
            MVal::N(x) => jv::num(*x),
            MVal::A(v) => jv::nums(v),
        }
    }
}
impl Meas {
    pub fn to_json(&self) -> Value {
        let mut o = jv::obj(vec![]);
        if let Some(v) = &self.value {
            jv::set(&mut o, "value", v.to_json());
        }
        if let Some(s) = self.sign {
            jv::set(&mut o, "sign", jv::num(s));
        }
        if let Some(k) = self.kind {
            jv::set(&mut o, "kind", k.into());
        }
        o
    }
}

fn line_pts(sk: &Value, id: &str) -> [P2; 2] {
    match by_id(sk, id) {
        Some(l) => [pxy(sk, l.str("p1")), pxy(sk, l.str("p2"))],
        None => [[f64::NAN; 2]; 2],
    }
}

/// `signedDist(p, [a, b])`.
pub fn signed_dist(p: P2, l: [P2; 2]) -> f64 {
    let [a, b] = l;
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = js::hypot2(dx, dy);
    let len = if len == 0.0 || len.is_nan() { 1e-300 } else { len };
    (dx * (p[1] - a[1]) - dy * (p[0] - a[0])) / len
}

/// `measure(sk, type, refs, sig)`.
pub fn measure(sk: &Value, ty: &str, refs: &[String], sig: Option<&str>) -> Meas {
    let owned;
    let sig = match sig {
        Some(s) => s,
        None => {
            owned = match_sig_ok(sk, ty, refs).map(|m| m.0).unwrap_or_default();
            owned.as_str()
        }
    };
    let r = |i: usize| refs.get(i).map_or("", String::as_str);
    let sgn = |v: f64| if v < 0.0 { -1.0 } else { 1.0 };
    match ty {
        "fix" => {
            let Some(e) = by_id(sk, r(0)) else { return Meas::default() };
            let v = match e.ty() {
                "point" => vec![e.num("x"), e.num("y")],
                "line" => {
                    let (a, b) = (pxy(sk, e.str("p1")), pxy(sk, e.str("p2")));
                    vec![a[0], a[1], b[0], b[1]]
                }
                "circle" => {
                    let c = pxy(sk, e.str("c"));
                    vec![c[0], c[1], e.num("r")]
                }
                "spline" => spline_pts(&e).iter().flat_map(|id| pxy(sk, id)).collect(),
                _ => {
                    let (c, s, en) = (pxy(sk, e.str("c")), pxy(sk, e.str("s")), pxy(sk, e.str("e")));
                    vec![c[0], c[1], s[0], s[1], en[0], en[1]]
                }
            };
            Meas { value: Some(MVal::A(v)), ..Meas::default() }
        }
        "distance" => {
            if sig == "PP" {
                let (a, b) = (pxy(sk, r(0)), pxy(sk, r(1)));
                return Meas { value: Some(MVal::N(js::hypot2(b[0] - a[0], b[1] - a[1]))), ..Meas::default() };
            }
            if sig == "L" {
                let [a, b] = line_pts(sk, r(0));
                return Meas { value: Some(MVal::N(js::hypot2(b[0] - a[0], b[1] - a[1]))), ..Meas::default() };
            }
            let p = if sig == "PL" { pxy(sk, r(0)) } else { line_pts(sk, r(1))[0] };
            let s = signed_dist(p, line_pts(sk, if sig == "PL" { r(1) } else { r(0) }));
            Meas { value: Some(MVal::N(s.abs())), sign: Some(sgn(s)), kind: None }
        }
        "hdistance" | "vdistance" => {
            let [a, b] = if sig == "L" { line_pts(sk, r(0)) } else { [pxy(sk, r(0)), pxy(sk, r(1))] };
            let d = if ty == "hdistance" { b[0] - a[0] } else { b[1] - a[1] };
            Meas { value: Some(MVal::N(d.abs())), sign: Some(sgn(d)), kind: None }
        }
        "offset" | "offsetlink" => {
            let s = if sig == "LL" {
                signed_dist(line_pts(sk, r(1))[0], line_pts(sk, r(0)))
            } else {
                let rr = |i: usize| by_id(sk, r(i)).map_or(f64::NAN, |e| radius_of(sk, &e));
                rr(1) - rr(0)
            };
            Meas { value: Some(MVal::N(s.abs())), sign: Some(sgn(s)), kind: None }
        }
        "radius" => Meas { value: Some(MVal::N(by_id(sk, r(0)).map_or(f64::NAN, |e| radius_of(sk, &e)))), ..Meas::default() },
        "diameter" => Meas { value: Some(MVal::N(2.0 * by_id(sk, r(0)).map_or(f64::NAN, |e| radius_of(sk, &e)))), ..Meas::default() },
        "angle" => {
            let [a1, b1] = line_pts(sk, r(0));
            let [a2, b2] = line_pts(sk, r(1));
            let u = [b1[0] - a1[0], b1[1] - a1[1]];
            let v = [b2[0] - a2[0], b2[1] - a2[1]];
            let ang = js::atan2(u[0] * v[1] - u[1] * v[0], u[0] * v[0] + u[1] * v[1]);
            Meas { value: Some(MVal::N(ang.abs() * 180.0 / std::f64::consts::PI)), sign: Some(sgn(ang)), kind: None }
        }
        "tangent" => {
            if sig.contains('S') {
                return Meas::default();
            }
            if sig == "LR" {
                let Some(rc) = by_id(sk, r(1)) else { return Meas::default() };
                return Meas { sign: Some(sgn(signed_dist(pxy(sk, rc.str("c")), line_pts(sk, r(0))))), ..Meas::default() };
            }
            let (Some(a), Some(b)) = (by_id(sk, r(0)), by_id(sk, r(1))) else { return Meas::default() };
            let (ca, cb) = (pxy(sk, a.str("c")), pxy(sk, b.str("c")));
            let (ra, rb) = (radius_of(sk, &a), radius_of(sk, &b));
            let d = js::hypot2(cb[0] - ca[0], cb[1] - ca[1]);
            if (d - (ra + rb)).abs() <= (d - (ra - rb).abs()).abs() {
                Meas { kind: Some("ext"), ..Meas::default() }
            } else {
                Meas { kind: Some("int"), sign: Some(sgn(ra - rb)), value: None }
            }
        }
        _ => Meas::default(),
    }
}

/// `addConstraint(sk, type, refs, value, extra)`.
pub fn add_constraint(sk: &mut Value, ty: &str, refs: &[String], value: Option<&Value>, of: Option<&str>) -> Result<Value, String> {
    let Some((sig, mrefs)) = match_sig(sk, ty, refs)? else {
        let rel = label_of(ty).filter(|s| !s.is_empty()).unwrap_or_else(|| ty.to_string());
        return Err(t("model.sketch.not_apply", &[("relation", F::S(rel))]));
    };
    let r = |i: usize| mrefs.get(i).cloned().unwrap_or_default();
    if ty == "tangent" && sig.contains('S') {
        let (a, b) = (r(0), r(1));
        let meet = {
            let (Some(ea), Some(eb)) = (by_id(sk, &a), by_id(sk, &b)) else { return Err(t("model.sketch.tangent_spline_end", &[])) };
            end_meet(sk, &ea, &eb)
        };
        let Some(meet) = meet else { return Err(t("model.sketch.tangent_spline_end", &[])) };
        for (eid, pid) in [(a.clone(), meet.0.clone()), (b.clone(), meet.1.clone())] {
            let (is_fit, first, has) = {
                let Some(e) = by_id(sk, &eid) else { continue };
                let pts = spline_pts(&e);
                let first = pts.first().cloned().unwrap_or_default();
                let end = if pid == first { 's' } else { 'e' };
                let key = if end == 's' { "a0" } else { "a1" };
                (e.ty() == "spline" && e.str("kind") == "fit", end, jv::finite(e.val(key)).is_some())
            };
            if !is_fit || has {
                continue;
            }
            let tt = {
                let Some(e) = by_id(sk, &eid) else { continue };
                spline_end_tangent(sk, &e, first)?
            };
            let key = if first == 's' { "a0" } else { "a1" };
            if let Some(e) = by_id_mut(sk, &eid) {
                jv::set(e, key, jv::num(js::atan2(tt[1], tt[0])));
            }
        }
    }
    if ty == "symmetric" && r(0) == r(1) {
        return Err(t("model.sketch.symmetric_two_points", &[]));
    }
    if ty == "offsetlink" && !constraints(sk).iter().any(|k| jv::is_str(k.get("id"), of.unwrap_or("\u{0}")) && jv::s(k, "type") == "offset")
    {
        return Err(t("model.sketch.offsetlink_needs_dim", &[]));
    }
    let id = next_id(sk, "k");
    let mut c = jv::obj(vec![("id", id.into()), ("type", ty.into()), ("refs", jv::strs(&mrefs))]);
    if ty == "offsetlink" {
        jv::set(&mut c, "of", of.map_or(Value::Null, |s| s.into()));
    }
    let meas = measure(sk, ty, &mrefs, Some(&sig));
    if let Some(s) = meas.sign {
        jv::set(&mut c, "sign", jv::num(s));
    }
    if let Some(k) = meas.kind {
        jv::set(&mut c, "kind", k.into());
    }
    if ty == "fix" {
        jv::set(&mut c, "value", meas.value.as_ref().map_or(Value::Null, MVal::to_json));
    } else if is_dim(ty) {
        let v = match value {
            None => match &meas.value {
                Some(MVal::N(x)) => *x,
                _ => f64::NAN,
            },
            Some(v) => cc_cad::js::to_number(v),
        };
        if !v.is_finite() {
            return Err(t("model.sketch.not_number", &[]));
        }
        if if ty == "angle" { !(0.0..=180.0).contains(&v) } else { v < 0.0 } {
            return Err(if ty == "angle" { t("model.sketch.angle_range", &[]) } else { t("model.sketch.dimension_negative", &[]) });
        }
        if (ty == "radius" || ty == "diameter") && v <= 0.0 {
            return Err(t("model.sketch.radius_zero", &[]));
        }
        jv::set(&mut c, "value", jv::num(v));
    }
    if let Some(v) = constraints_mut(sk) {
        v.push(c.clone());
    }
    Ok(c)
}

/// `describe(c)`.
pub fn describe(c: &Value) -> String {
    let ty = jv::s(c, "type");
    let fmt = |v: f64| js::fmt_fixed(v, 4);
    match def_of(ty) {
        None => ty.to_string(),
        Some(Err(())) => String::from("undefined"),
        Some(Ok(d)) => {
            if !d.dim {
                return t(d.label_key, &[]);
            }
            let v = fmt(jv::n(c, "value"));
            match ty {
                "angle" => t("model.sketch.describe.angle", &[("value", F::S(v))]),
                "diameter" => t("model.sketch.describe.diameter", &[("value", F::S(v))]),
                "radius" => t("model.sketch.describe.radius", &[("value", F::S(v))]),
                "offset" => t("model.sketch.describe.offset", &[("value", F::S(v))]),
                _ => t("model.sketch.describe.dim", &[("label", F::S(t(d.label_key, &[]))), ("value", F::S(v))]),
            }
        }
    }
}

fn refs_of(c: &Value) -> Vec<String> {
    jv::arr(c.get("refs")).iter().map(|v| js::str_of(Some(v))).collect()
}

/// `removeEntities(sk, ids)` → (entities removed, constraints removed).
pub fn remove_entities(sk: &mut Value, ids: &[String]) -> (usize, usize) {
    let mut gone: Vec<String> = Vec::new();
    let add = |g: &mut Vec<String>, s: &str| {
        if !g.iter().any(|x| x == s) {
            g.push(s.to_string());
        }
    };
    for id in ids {
        add(&mut gone, id);
    }
    gone.retain(|x| x != ORIGIN);
    for id in ids {
        if let Some(e) = by_id(sk, id)
            && e.ty() != "point"
        {
            for p in points_of(&e) {
                add(&mut gone, &p);
            }
        }
    }
    // keep points still used by curves that stay
    for e in entities(sk) {
        let en = Ent::E(e);
        if gone.iter().any(|g| g == en.id()) || en.ty() == "point" {
            continue;
        }
        for p in points_of(&en) {
            if gone.contains(&p) && !ids.contains(&p) {
                gone.retain(|g| *g != p);
            }
        }
    }
    // removing a point removes curves built on it — a spline just loses that point while it keeps enough
    let n = entities(sk).len();
    for i in 0..n {
        let Some(e) = entities(sk).get(i) else { continue };
        let en = Ent::E(e);
        if en.ty() == "point" || gone.iter().any(|g| g == en.id()) || !points_of(&en).iter().any(|p| gone.contains(p)) {
            continue;
        }
        if en.ty() == "spline" {
            let pts = points_of(&en);
            let keep: Vec<String> = pts.iter().filter(|p| !gone.contains(p)).cloned().collect();
            if keep.len() >= spline_min(&en) {
                let drop_a0 = keep.first() != pts.first();
                let drop_a1 = keep.last() != pts.last();
                if let Some(e) = entities_mut(sk).and_then(|v| v.get_mut(i)) {
                    if drop_a0 {
                        jv::del(e, "a0");
                    }
                    if drop_a1 {
                        jv::del(e, "a1");
                    }
                    jv::set(e, "pts", jv::strs(&keep));
                    jv::del(e, "knots");
                }
                continue;
            }
        }
        let id = en.id().to_string();
        add(&mut gone, &id);
    }
    let gset: HashSet<&String> = gone.iter().collect();
    if let Some(v) = entities_mut(sk) {
        v.retain(|e| !gset.contains(&js::str_of(e.get("id"))));
    }
    let before = constraints(sk).len();
    if let Some(v) = constraints_mut(sk) {
        v.retain(|c| !refs_of(c).iter().any(|r| gset.contains(r)));
    }
    drop_orphan_links(sk);
    (gone.len(), before.saturating_sub(constraints(sk).len()))
}

/// `removeConstraints(sk, ids)`.
pub fn remove_constraints(sk: &mut Value, ids: &[String]) {
    if let Some(v) = constraints_mut(sk) {
        v.retain(|c| !ids.iter().any(|i| jv::is_str(c.get("id"), i)));
    }
    drop_orphan_links(sk);
}

fn drop_orphan_links(sk: &mut Value) {
    let masters: HashSet<String> =
        constraints(sk).iter().filter(|c| jv::s(c, "type") == "offset").map(|c| js::str_of(c.get("id"))).collect();
    if let Some(v) = constraints_mut(sk) {
        v.retain(|c| jv::s(c, "type") != "offsetlink" || c.get("of").and_then(Value::as_str).is_some_and(|o| masters.contains(o)));
    }
}

/// `filletCorner(sk, pointId, r)` → { arc, removed }.
pub fn fillet_corner(sk: &mut Value, point_id: &str, r: f64) -> Result<Value, String> {
    if !(r > 0.0) {
        return Err(t("model.sketch.fillet_radius_zero", &[]));
    }
    let Some(at) = point_xy(sk, point_id) else { return Err(t("model.sketch.fillet_pick_corner", &[])) };
    let tol = 1e-6 * js::max(1.0, extent(sk));
    let mut ends: Vec<(String, String, String)> = Vec::new();
    for e in entities(sk) {
        let en = Ent::E(e);
        if en.ty() != "line" || en.construction() {
            continue;
        }
        let (p1, p2) = (en.str("p1").to_string(), en.str("p2").to_string());
        for (mine, other) in [(p1.clone(), p2.clone()), (p2.clone(), p1.clone())] {
            let q = pxy(sk, &mine);
            if js::hypot2(q[0] - at[0], q[1] - at[1]) <= tol {
                ends.push((en.id().to_string(), mine, other));
            }
        }
    }
    if ends.len() != 2 || ends.first().map(|e| &e.0) == ends.get(1).map(|e| &e.0) {
        return Err(t("model.sketch.fillet_two_lines", &[]));
    }
    let (la_id, a_mine, a_other) = ends.first().cloned().unwrap_or_default();
    let (lb_id, b_mine, b_other) = ends.get(1).cloned().unwrap_or_default();
    let p = at;
    let fa = pxy(sk, &a_other);
    let fb = pxy(sk, &b_other);
    let la = js::hypot2(fa[0] - p[0], fa[1] - p[1]);
    let lb = js::hypot2(fb[0] - p[0], fb[1] - p[1]);
    let u1 = [(fa[0] - p[0]) / la, (fa[1] - p[1]) / la];
    let u2 = [(fb[0] - p[0]) / lb, (fb[1] - p[1]) / lb];
    let cos_t = js::max(-1.0, js::min(1.0, u1[0] * u2[0] + u1[1] * u2[1]));
    let th = js::acos(cos_t);
    if th < 1e-6 || std::f64::consts::PI - th < 1e-6 {
        return Err(t("model.sketch.fillet_in_line", &[]));
    }
    let d = r / js::tan(th / 2.0);
    if d >= la - tol || d >= lb - tol {
        return Err(t("model.sketch.fillet_too_big", &[]));
    }
    let t1 = [p[0] + u1[0] * d, p[1] + u1[1] * d];
    let t2 = [p[0] + u2[0] * d, p[1] + u2[1] * d];
    let bis = [u1[0] + u2[0], u1[1] + u2[1]];
    let bl = js::hypot2(bis[0], bis[1]);
    let h = r / js::sin(th / 2.0);
    let c = [p[0] + bis[0] / bl * h, p[1] + bis[1] / bl * h];
    let ida = a_mine.clone();
    let mut idb = b_mine.clone();
    if ida == idb {
        idb = add_point(sk, p[0], p[1], false);
        if let Some(l) = by_id_mut(sk, &lb_id) {
            if jv::s(l, "p1") == ida {
                jv::set(l, "p1", idb.as_str().into());
            } else {
                jv::set(l, "p2", idb.as_str().into());
            }
        }
    }
    let removed = constraints(sk).iter().filter(|k| refs_of(k).iter().any(|x| *x == ida || *x == idb)).count();
    if let Some(v) = constraints_mut(sk) {
        v.retain(|k| !refs_of(k).iter().any(|x| *x == ida || *x == idb));
    }
    set_pt(sk, &ida, t1);
    set_pt(sk, &idb, t2);
    let cid = add_point(sk, c[0], c[1], false);
    let ccw = (t1[0] - c[0]) * (t2[1] - c[1]) - (t1[1] - c[1]) * (t2[0] - c[0]) > 0.0;
    let aid = next_id(sk, "a");
    let arc = jv::obj(vec![
        ("id", aid.as_str().into()),
        ("type", "arc".into()),
        ("c", cid.as_str().into()),
        ("s", if ccw { ida.as_str() } else { idb.as_str() }.into()),
        ("e", if ccw { idb.as_str() } else { ida.as_str() }.into()),
    ]);
    push_entity(sk, arc);
    add_constraint(sk, "tangent", &[la_id, aid.clone()], None, None)?;
    add_constraint(sk, "tangent", &[lb_id, aid.clone()], None, None)?;
    add_constraint(sk, "radius", std::slice::from_ref(&aid), Some(&jv::num(r)), None)?;
    Ok(jv::obj(vec![("arc", aid.into()), ("removed", jv::unum(removed))]))
}

fn set_pt(sk: &mut Value, id: &str, p: P2) {
    if let Some(e) = by_id_mut(sk, id) {
        jv::set(e, "x", jv::num(p[0]));
        jv::set(e, "y", jv::num(p[1]));
    }
}

/// `extent(sk)`.
pub fn extent(sk: &Value) -> f64 {
    let mut m: f64 = 0.0;
    for e in entities(sk) {
        let en = Ent::E(e);
        if en.ty() == "point" {
            m = js::max(js::max(m, en.num("x").abs()), en.num("y").abs());
        }
        if en.ty() == "circle" {
            let c = pxy(sk, en.str("c"));
            let r = en.num("r");
            m = js::max(js::max(m, c[0].abs() + r), c[1].abs() + r);
        }
    }
    m
}

/// `bounds(sk)` → (x0, y0, x1, y1) or None.
pub fn bounds(sk: &Value) -> Option<[f64; 4]> {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut inc = |x: f64, y: f64| {
        x0 = js::min(x0, x);
        y0 = js::min(y0, y);
        x1 = js::max(x1, x);
        y1 = js::max(y1, y);
    };
    for e in entities(sk) {
        let en = Ent::E(e);
        match en.ty() {
            "point" => inc(en.num("x"), en.num("y")),
            "circle" => {
                let c = pxy(sk, en.str("c"));
                let r = en.num("r");
                inc(c[0] - r, c[1] - r);
                inc(c[0] + r, c[1] + r);
            }
            "arc" => {
                for p in tess_arc(sk, &en, 32.0) {
                    inc(p[0], p[1]);
                }
            }
            "spline" => {
                if let Ok(ps) = tess_spline(sk, &en, 8) {
                    for p in ps {
                        inc(p[0], p[1]);
                    }
                }
            }
            _ => {}
        }
    }
    if x0 == f64::INFINITY { None } else { Some([x0, y0, x1, y1]) }
}

/// `endsOf(e)`.
pub fn ends_of(e: &Ent) -> Vec<String> {
    match e.ty() {
        "line" => vec![e.str("p1").to_string(), e.str("p2").to_string()],
        "arc" => vec![e.str("s").to_string(), e.str("e").to_string()],
        "spline" if !jv::truthy(e.val("closed")) => {
            let pts = spline_pts(e);
            vec![pts.first().cloned().unwrap_or_else(|| "undefined".into()), pts.last().cloned().unwrap_or_else(|| "undefined".into())]
        }
        _ => Vec::new(),
    }
}

/// `endMeet(sk, a, b)` → (pa, pb).
pub fn end_meet(sk: &Value, a: &Ent, b: &Ent) -> Option<(String, String)> {
    let ea = ends_of(a);
    let eb = ends_of(b);
    for p in &ea {
        if eb.contains(p) {
            return Some((p.clone(), p.clone()));
        }
    }
    for k in constraints(sk) {
        if jv::s(k, "type") != "coincident" {
            continue;
        }
        let refs = jv::arr(k.get("refs"));
        if refs.len() != 2 {
            continue;
        }
        let u = js::str_of(refs.first());
        let v = js::str_of(refs.get(1));
        if ea.contains(&u) && eb.contains(&v) {
            return Some((u, v));
        }
        if ea.contains(&v) && eb.contains(&u) {
            return Some((v, u));
        }
    }
    None
}

/// `sharedEnd(sk, a, b)`.
pub fn shared_end(sk: &Value, a: &Ent, b: &Ent) -> Option<String> {
    if a.ty() == "spline" || b.ty() == "spline" {
        return None;
    }
    end_meet(sk, a, b).map(|m| m.0)
}

// ---------------------------------------------------------------- profiles

/// `tessArc(sk, a, segFull)`.
pub fn tess_arc(sk: &Value, a: &Ent, seg_full: f64) -> Vec<P2> {
    let (c, r, a0, sweep) = arc_angles(sk, a);
    let n = js::max(1.0, (sweep / (2.0 * std::f64::consts::PI) * seg_full - 1e-9).ceil());
    let n = js::count(n, 1_000_000).max(1);
    let nf = js::f(n);
    let mut pts = Vec::with_capacity(n.saturating_add(1));
    for i in 0..=n {
        let tt = a0 + sweep * js::f(i) / nf;
        pts.push([c[0] + r * js::cos(tt), c[1] + r * js::sin(tt)]);
    }
    pts
}

/// `tessCircle(c, r, segFull)`.
pub fn tess_circle(c: P2, r: f64, seg_full: f64) -> Vec<P2> {
    let n = js::count(seg_full.ceil(), 1_000_000);
    let mut pts = Vec::with_capacity(n);
    for i in 0..n {
        let tt = 2.0 * std::f64::consts::PI * js::f(i) / seg_full;
        pts.push([c[0] + r * js::cos(tt), c[1] + r * js::sin(tt)]);
    }
    pts
}

/// A profile: closed loops and which curve went into which loop.
#[derive(Clone, Debug, Default)]
pub struct Profile {
    pub loops: Vec<Vec<P2>>,
    pub curves: Vec<(String, usize)>,
}
/// A profile problem: the message and where (an open or branching end).
#[derive(Clone, Debug)]
pub struct ProfileError {
    pub error: String,
    pub at: Option<P2>,
}

/// `profile(sk, segFull)`.
pub fn profile(sk: &Value, seg_full: f64) -> Result<Profile, ProfileError> {
    let tol = 1e-6 * js::max(1.0, extent(sk));
    let mut out = Profile::default();
    struct Seg {
        id: String,
        pts: Vec<P2>,
        n0: usize,
        n1: usize,
    }
    let mut segs: Vec<Seg> = Vec::new();
    let err = |e: String| ProfileError { error: e, at: None };
    for e in entities(sk) {
        let en = Ent::E(e);
        if en.construction() {
            continue;
        }
        match en.ty() {
            "circle" => {
                let r = en.num("r");
                if !(r > tol) {
                    return Err(err(t("model.sketch.circle_no_size", &[])));
                }
                out.loops.push(tess_circle(pxy(sk, en.str("c")), r, seg_full));
                out.curves.push((en.id().to_string(), out.loops.len().saturating_sub(1)));
            }
            "line" => {
                let a = pxy(sk, en.str("p1"));
                let b = pxy(sk, en.str("p2"));
                if js::hypot2(b[0] - a[0], b[1] - a[1]) <= tol {
                    return Err(err(t("model.sketch.line_no_length", &[])));
                }
                segs.push(Seg { id: en.id().to_string(), pts: vec![a, b], n0: 0, n1: 0 });
            }
            "arc" => segs.push(Seg { id: en.id().to_string(), pts: tess_arc(sk, &en, seg_full), n0: 0, n1: 0 }),
            "spline" => {
                let per = js::count(js::max(4.0, (seg_full / 8.0).ceil()), 100_000);
                let mut pts = match tess_spline(sk, &en, per) {
                    Ok(p) => p,
                    Err(m) => return Err(err(m)),
                };
                if jv::truthy(en.val("closed")) {
                    pts.pop();
                    out.loops.push(pts);
                    out.curves.push((en.id().to_string(), out.loops.len().saturating_sub(1)));
                } else {
                    segs.push(Seg { id: en.id().to_string(), pts, n0: 0, n1: 0 });
                }
            }
            _ => {}
        }
    }
    // join ends by position
    let mut nodes: Vec<(P2, Vec<usize>)> = Vec::new();
    let node_of = |nodes: &mut Vec<(P2, Vec<usize>)>, p: P2| -> usize {
        for (i, n) in nodes.iter().enumerate() {
            if js::hypot2(n.0[0] - p[0], n.0[1] - p[1]) <= tol * 10.0 {
                return i;
            }
        }
        nodes.push((p, Vec::new()));
        nodes.len().saturating_sub(1)
    };
    for (si, s) in segs.iter_mut().enumerate() {
        let first = s.pts.first().copied().unwrap_or([f64::NAN; 2]);
        let last = s.pts.last().copied().unwrap_or([f64::NAN; 2]);
        s.n0 = node_of(&mut nodes, first);
        s.n1 = node_of(&mut nodes, last);
        if let Some(n) = nodes.get_mut(s.n0) {
            n.1.push(si);
        }
        if let Some(n) = nodes.get_mut(s.n1) {
            n.1.push(si);
        }
    }
    let fx = |v: f64| js::fmt_fixed(v, 3);
    for n in &nodes {
        if n.1.len() == 1 {
            return Err(ProfileError {
                error: t("model.sketch.profile_open", &[("x", F::S(fx(n.0[0]))), ("y", F::S(fx(n.0[1])))]),
                at: Some(n.0),
            });
        }
        if n.1.len() > 2 {
            return Err(ProfileError {
                error: t("model.sketch.profile_branches", &[("x", F::S(fx(n.0[0]))), ("y", F::S(fx(n.0[1])))]),
                at: Some(n.0),
            });
        }
    }
    let mut used = vec![false; segs.len()];
    for s0 in 0..segs.len() {
        if used.get(s0).copied().unwrap_or(true) {
            continue;
        }
        let mut lp: Vec<P2> = Vec::new();
        let Some(start) = segs.get(s0).map(|s| s.n0) else { continue };
        let mut s = s0;
        let mut from = start;
        let li = out.loops.len();
        for _guard in 0..=segs.len() {
            if let Some(u) = used.get_mut(s) {
                *u = true;
            }
            let Some(seg) = segs.get(s) else { break };
            out.curves.push((seg.id.clone(), li));
            let fwd = seg.n0 == from;
            let n = seg.pts.len();
            for i in 0..n.saturating_sub(1) {
                let idx = if fwd { i } else { n.saturating_sub(1).saturating_sub(i) };
                if let Some(p) = seg.pts.get(idx) {
                    lp.push(*p);
                }
            }
            let to = if fwd { seg.n1 } else { seg.n0 };
            if to == start {
                break;
            }
            let nx = nodes.get(to).and_then(|nd| nd.1.iter().find(|&&q| q != s).copied());
            let Some(nx) = nx else { break };
            if used.get(nx).copied().unwrap_or(true) {
                break;
            }
            from = to;
            s = nx;
        }
        if lp.len() < 3 {
            return Err(err(t("model.sketch.loop_three_sides", &[])));
        }
        out.loops.push(lp);
    }
    Ok(out)
}

/// `insideLoops(loops, p)`: even-odd.
pub fn inside_loops(loops: &[Vec<P2>], p: P2) -> bool {
    let mut inside = false;
    for lp in loops {
        let n = lp.len();
        if n == 0 {
            continue;
        }
        let mut j = n.saturating_sub(1);
        for i in 0..n {
            let (Some(a), Some(b)) = (lp.get(i), lp.get(j)) else { break };
            if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
                inside = !inside;
            }
            j = i;
        }
    }
    inside
}

/// `loopArea(loop)`.
pub fn loop_area(lp: &[P2]) -> f64 {
    let n = lp.len();
    if n == 0 {
        return 0.0;
    }
    let mut s = 0.0;
    let mut j = n.saturating_sub(1);
    for i in 0..n {
        let (Some(a), Some(b)) = (lp.get(j), lp.get(i)) else { break };
        s += a[0] * b[1] - b[0] * a[1];
        j = i;
    }
    s / 2.0
}

// ---------------------------------------------------------------- validation of file data

/// ID = /^[A-Za-z][A-Za-z0-9_:-]{0,40}$/.
fn id_ok(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 41
        && b.first().is_some_and(u8::is_ascii_alphabetic)
        && b.iter().all(|c| c.is_ascii_alphanumeric() || b"_:-".contains(c))
}
fn num_ok(v: Option<&Value>) -> Option<f64> {
    jv::as_num(v).filter(|x| x.is_finite() && x.abs() < 1e9)
}

const TYPE_ERR_NULL: &str = "Cannot read properties of null (reading 'construction')";

/// `readSketch(raw)`: untrusted sketch JSON → a clean copy, or the message.
pub fn read_sketch(raw: Option<&Value>) -> Result<Value, String> {
    let Some(raw) = raw.filter(|r| jv::truthy(Some(r)) && jv::is_obj_like(Some(r))) else {
        return Err(t("model.sketch.file.no_entities", &[]));
    };
    let (Some(ents), Some(cons)) = (jv::as_arr(raw.get("entities")), jv::as_arr(raw.get("constraints"))) else {
        return Err(t("model.sketch.file.no_entities", &[]));
    };
    let seq = match jv::int(raw.get("seq")) {
        Some(s) if s > 0.0 => s,
        _ => 1.0,
    };
    let mut entities: Vec<Value> = Vec::new();
    let mut ids: HashSet<String> = HashSet::new();
    ids.insert(ORIGIN.to_string());
    let take = |ids: &mut HashSet<String>, id: Option<&Value>| -> Result<String, String> {
        match id.and_then(Value::as_str) {
            Some(s) if id_ok(s) && !ids.contains(s) => {
                ids.insert(s.to_string());
                Ok(s.to_string())
            }
            _ => Err(t("model.sketch.file.bad_id", &[])),
        }
    };
    let ents = ents.get(..ents.len().min(5000)).unwrap_or(&[]);
    let mut point_ids: HashSet<String> = HashSet::new();
    for e in ents {
        if e.is_null() {
            return Err(TYPE_ERR_NULL.into());
        }
        let cons_flag = jv::is_true(e.get("construction"));
        if jv::is_str(e.get("type"), "point") {
            let (Some(x), Some(y)) = (num_ok(e.get("x")), num_ok(e.get("y"))) else { return Err(t("model.sketch.file.bad_point", &[])) };
            let id = take(&mut ids, e.get("id"))?;
            let mut p = jv::obj(vec![("id", id.as_str().into()), ("type", "point".into()), ("x", jv::num(x)), ("y", jv::num(y))]);
            if cons_flag {
                jv::set(&mut p, "construction", true.into());
            }
            point_ids.insert(id);
            entities.push(p);
        }
    }
    let is_pt = |v: Option<&Value>| v.and_then(Value::as_str).is_some_and(|s| s == ORIGIN || point_ids.contains(s));
    let not_origin = |v: Option<&Value>| !jv::is_str(v, ORIGIN);
    for e in ents {
        let cons_flag = jv::is_true(e.get("construction"));
        let ty = e.get("type").and_then(Value::as_str);
        let mut g = match ty {
            Some("line") => {
                if !is_pt(e.get("p1")) || !is_pt(e.get("p2")) || !not_origin(e.get("p1")) || !not_origin(e.get("p2")) {
                    return Err(t("model.sketch.file.bad_line", &[]));
                }
                let id = take(&mut ids, e.get("id"))?;
                jv::obj(vec![("id", id.into()), ("type", "line".into()), ("p1", jv::s(e, "p1").into()), ("p2", jv::s(e, "p2").into())])
            }
            Some("circle") => {
                let r = num_ok(e.get("r"));
                if !is_pt(e.get("c")) || !not_origin(e.get("c")) || r.is_none_or(|r| r <= 0.0) {
                    return Err(t("model.sketch.file.bad_circle", &[]));
                }
                let id = take(&mut ids, e.get("id"))?;
                jv::obj(vec![("id", id.into()), ("type", "circle".into()), ("c", jv::s(e, "c").into()), ("r", jv::num(r.unwrap_or(0.0)))])
            }
            Some("arc") => {
                if !["c", "s", "e"].iter().all(|k| is_pt(e.get(*k)) && not_origin(e.get(*k))) {
                    return Err(t("model.sketch.file.bad_arc", &[]));
                }
                let id = take(&mut ids, e.get("id"))?;
                jv::obj(vec![
                    ("id", id.into()),
                    ("type", "arc".into()),
                    ("c", jv::s(e, "c").into()),
                    ("s", jv::s(e, "s").into()),
                    ("e", jv::s(e, "e").into()),
                ])
            }
            Some("spline") => read_spline(e, &is_pt, &mut ids, &take)?,
            Some("point") => continue,
            _ => return Err(t("model.sketch.file.unknown_geometry", &[])),
        };
        if cons_flag {
            jv::set(&mut g, "construction", true.into());
        }
        entities.push(g);
    }
    let mut sk = jv::obj(vec![("seq", jv::num(seq)), ("entities", Value::Array(entities)), ("constraints", Value::Array(Vec::new()))]);
    for e in self::entities(&sk) {
        let en = Ent::E(e);
        if en.ty() == "spline" && spline_curve(&sk, &en).is_err() {
            return Err(t("model.sketch.file.spline_coincident", &[]));
        }
    }
    let cons = cons.get(..cons.len().min(10000)).unwrap_or(&[]);
    let mut out_cons: Vec<Value> = Vec::new();
    for c in cons {
        let ty = c.get("type").and_then(Value::as_str);
        let def = ty.and_then(def_of);
        if !jv::truthy(Some(c)) || def.is_none() || jv::as_arr(c.get("refs")).is_none() {
            return Err(t("model.sketch.file.unknown_relation", &[]));
        }
        let ty = ty.unwrap_or("");
        let id = take(&mut ids, c.get("id"))?;
        let refs: Vec<String> = jv::arr(c.get("refs")).iter().map(cc_cad::js::to_js_string).collect();
        let mut k = jv::obj(vec![("id", id.into()), ("type", ty.into()), ("refs", jv::strs(&refs))]);
        // matchSig against the sketch so far (relations are checked against the geometry only)
        let m = match_sig(&sk, ty, &refs)?;
        let Some((sig, _)) = m else {
            let Some(label) = label_of(ty) else { return Err("Cannot read properties of undefined (reading 'toLowerCase')".into()) };
            return Err(t("model.sketch.file.relation_wrong", &[("relation", F::S(js::lower(&label)))]));
        };
        if ty == "fix" {
            let ok = jv::as_arr(c.get("value")).is_some_and(|a| a.iter().all(|v| num_ok(Some(v)).is_some()));
            if !ok {
                return Err(t("model.sketch.file.bad_fix", &[]));
            }
            jv::set(&mut k, "value", c.get("value").cloned().unwrap_or(Value::Null));
        } else if is_dim(ty) {
            match num_ok(c.get("value")) {
                Some(v) if v >= 0.0 => jv::set(&mut k, "value", jv::num(v)),
                _ => return Err(t("model.sketch.file.bad_dimension", &[])),
            }
        }
        if let Some(s) = jv::as_num(c.get("sign"))
            && (s == 1.0 || s == -1.0)
        {
            jv::set(&mut k, "sign", jv::num(s));
        }
        if let Some(kd @ ("ext" | "int")) = c.get("kind").and_then(Value::as_str) {
            jv::set(&mut k, "kind", kd.into());
        }
        for key in ["at", "off"] {
            if let Some(a) = jv::as_arr(c.get(key))
                && a.len() == 2
                && a.iter().all(|v| num_ok(Some(v)).is_some())
            {
                jv::set(&mut k, key, Value::Array(a.clone()));
            }
        }
        if ty == "offsetlink" {
            jv::set(&mut k, "of", js::string_of(c.get("of")).into());
        }
        if ty == "tangent" && sig.contains('S') {
            let r0 = refs.first().map_or("", String::as_str);
            let r1 = refs.get(1).map_or("", String::as_str);
            let meet = match (by_id(&sk, r0), by_id(&sk, r1)) {
                (Some(a), Some(b)) => end_meet(&sk, &a, &b),
                _ => None,
            };
            if meet.is_none() {
                return Err(t("model.sketch.file.tangent_spline_end", &[]));
            }
        }
        if ty == "symmetric" && refs.first() == refs.get(1) {
            return Err(t("model.sketch.file.symmetric_wrong", &[]));
        }
        out_cons.push(k.clone());
        if let Some(v) = constraints_mut(&mut sk) {
            v.push(k);
        }
    }
    for k in &out_cons {
        if jv::s(k, "type") == "offsetlink" {
            let of = jv::s(k, "of");
            if !out_cons.iter().any(|m| jv::s(m, "id") == of && jv::s(m, "type") == "offset") {
                return Err(t("model.sketch.file.offset_lost", &[]));
            }
        }
    }
    let mut seq_v = seq;
    for id in &ids {
        let b = id.as_bytes();
        if b.len() >= 2 && b.first().is_some_and(u8::is_ascii_lowercase) && b.iter().skip(1).all(u8::is_ascii_digit) {
            let n = js::str_num(id.get(1..).unwrap_or("0"));
            seq_v = js::max(seq_v, n + 1.0);
        }
    }
    jv::set(&mut sk, "seq", jv::num(seq_v));
    Ok(sk)
}

fn read_spline(
    e: &Value,
    is_pt: &dyn Fn(Option<&Value>) -> bool,
    ids: &mut HashSet<String>,
    take: &dyn Fn(&mut HashSet<String>, Option<&Value>) -> Result<String, String>,
) -> Result<Value, String> {
    let bad = |k: &str| t(k, &[]);
    let kind = e.get("kind").and_then(Value::as_str);
    if kind != Some("fit") && kind != Some("control") {
        return Err(bad("model.sketch.spline_bad.kind"));
    }
    let kind = kind.unwrap_or("fit");
    let Some(pts) = jv::as_arr(e.get("pts")).filter(|a| a.len() <= 500) else { return Err(bad("model.sketch.spline_bad.points")) };
    let closed = jv::is_true(e.get("closed"));
    if pts.len() < if closed { 3 } else { 2 } {
        return Err(bad("model.sketch.spline_bad.too_few"));
    }
    let uniq: HashSet<String> = pts.iter().map(|v| v.to_string()).collect();
    if !pts.iter().all(|p| p.is_string() && is_pt(Some(p)) && !jv::is_str(Some(p), ORIGIN)) || uniq.len() != pts.len() {
        return Err(bad("model.sketch.spline_bad.point_twice"));
    }
    let id = take(ids, e.get("id"))?;
    let mut out = jv::obj(vec![("id", id.into()), ("type", "spline".into()), ("kind", kind.into()), ("pts", Value::Array(pts.clone()))]);
    if closed {
        jv::set(&mut out, "closed", true.into());
    }
    for key in ["a0", "a1"] {
        let Some(v) = e.get(key) else { continue };
        if kind != "fit" || closed || num_ok(Some(v)).is_none() {
            return Err(bad("model.sketch.spline_bad.end_angle"));
        }
        jv::set(&mut out, key, v.clone());
    }
    if let Some(k) = e.get("knots") {
        if kind != "control" || closed {
            return Err(bad("model.sketch.spline_bad.knots_kind"));
        }
        if let Some(why) = sp::knot_problem(Some(k), pts.len()) {
            return Err(why);
        }
        jv::set(&mut out, "knots", k.clone());
    }
    Ok(out)
}
