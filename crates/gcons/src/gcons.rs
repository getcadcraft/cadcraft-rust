//! gcons.js — 2D constraints on a CadCraft drawing. The constraints live in the undoable document beside the parameter
//! table (`doc.constraints`, `doc.params`, replaced together by `Doc::set_parametric` inside a change: one undo step
//! with whatever geometry moved).
//!
//! Geometric: coincident (PP, PL, PR), collinear (LL), concentric (RR), fix (P, E), parallel, perpendicular (LL),
//! horizontal, vertical (L, PP), tangent (LR, RR), symmetric (PPL, LLL, RRL), equal (LL, RR). Dimensional: hdist,
//! vdist (PP), dist (PP, PL, LL), radius, diameter (R), angle (LL, R): each has a name (d1, rad1, dia1, ang1), its last
//! resolved value and where its label sits; its expression is the parameter of the same name, linked `c:<id>` → name.
//!
//! Moving constrained geometry goes through `Doc::change`, which calls [`after_edit`] (installed as the drawing's
//! edit hook): what the edit moved is pinned and the rest follows; if that cannot be met the pins come off; if it still
//! cannot be met the edit is refused (rolled back) and the drawing notes why. Erasing an object drops its constraints.

use cc_cad::entity::{Dim, Entity, Geo};
use cc_cad::js;
use cc_cad::{Doc, DocError};
use cc_cad_geom::jsmath::JsMath;
use cc_geom::P2;
use cc_part::expr;
use cc_part::params::{self, Params};
use serde_json::{Map, Value, json};

use crate::con::{Con, Ref};
use crate::gsolve::{
    self, Pins, RK, analyze, components, components_idx, fields_of, get_field, ref_circle, ref_kind, ref_line, ref_point, residual_ok,
    seg_count, solvable, solve_system,
};
use crate::tr::Tr;

const TAU: f64 = std::f64::consts::PI * 2.0;
const PI: f64 = std::f64::consts::PI;

pub const LIMIT_CONSTRAINTS: usize = 20_000;
pub const LIMIT_PAYLOAD: usize = 8_000_000;
pub const PARAM_BLOCK: &str = "CADCRAFT_PARAMETRIC";

/// The geometric types: (type, label key, signatures, glyph).
pub const GEOMETRIC: &[(&str, &str, &[&str], &str)] = &[
    ("horizontal", "gcons.type.horizontal", &["L", "PP"], "H"),
    ("vertical", "gcons.type.vertical", &["L", "PP"], "V"),
    ("perpendicular", "gcons.type.perpendicular", &["LL"], "⊥"),
    ("parallel", "gcons.type.parallel", &["LL"], "∥"),
    ("tangent", "gcons.type.tangent", &["LR", "RR"], "T"),
    ("coincident", "gcons.type.coincident", &["PP", "PL", "PR"], "•"),
    ("concentric", "gcons.type.concentric", &["RR"], "◎"),
    ("collinear", "gcons.type.collinear", &["LL"], "⫽"),
    ("symmetric", "gcons.type.symmetric", &["PPL", "LLL", "RRL"], "⇔"),
    ("equal", "gcons.type.equal", &["LL", "RR"], "="),
    ("fix", "gcons.type.fix", &["P", "E"], "⊕"),
];
/// The dimensional types: (type, label key, name prefix, signatures, kind).
pub const DIMENSIONAL: &[(&str, &str, &str, &[&str], &str)] = &[
    ("hdist", "gcons.dimtype.hdist", "d", &["PP"], "length"),
    ("vdist", "gcons.dimtype.vdist", "d", &["PP"], "length"),
    ("dist", "gcons.dimtype.dist", "d", &["PP", "PL", "LL"], "length"),
    ("radius", "gcons.dimtype.radius", "rad", &["R"], "length"),
    ("diameter", "gcons.dimtype.diameter", "dia", &["R"], "length"),
    ("angle", "gcons.dimtype.angle", "ang", &["LL", "R"], "angle"),
];

pub fn is_dim_type(t: &str) -> bool {
    DIMENSIONAL.iter().any(|d| d.0 == t)
}
pub fn is_dim(c: &Con) -> bool {
    is_dim_type(&c.typ)
}
pub fn is_type(t: &str) -> bool {
    is_dim_type(t) || GEOMETRIC.iter().any(|g| g.0 == t)
}
/// `TYPES[type].label`
pub fn type_label(tr: &Tr, t: &str) -> String {
    GEOMETRIC
        .iter()
        .find(|g| g.0 == t)
        .map(|g| g.1)
        .or_else(|| DIMENSIONAL.iter().find(|d| d.0 == t).map(|d| d.1))
        .map_or_else(String::new, |k| tr.s(k))
}
pub fn type_sigs(t: &str) -> &'static [&'static str] {
    GEOMETRIC.iter().find(|g| g.0 == t).map(|g| g.2).or_else(|| DIMENSIONAL.iter().find(|d| d.0 == t).map(|d| d.3)).unwrap_or(&[])
}
pub fn dim_prefix(t: &str) -> &'static str {
    DIMENSIONAL.iter().find(|d| d.0 == t).map_or("d", |d| d.2)
}
pub fn dim_kind(t: &str) -> &'static str {
    DIMENSIONAL.iter().find(|d| d.0 == t).map_or("length", |d| d.4)
}
pub fn glyph(t: &str) -> &'static str {
    GEOMETRIC.iter().find(|g| g.0 == t).map_or("", |g| g.3)
}
/// `targetKey(c)`
pub fn target_key(c: &Con) -> String {
    format!("c:{}", c.id)
}

// ---------------------------------------------------------------- the drawing's constraints

/// The drawing's constraints (shared, not copied).
pub fn cons(doc: &Doc) -> std::sync::Arc<Vec<Con>> {
    std::sync::Arc::clone(&doc.constraints)
}
/// The list `set_parametric` takes.
pub fn cons_values(cs: &[Con]) -> Vec<Con> {
    cs.to_vec()
}
/// The drawing's parameters.
pub fn params_of(doc: &Doc) -> Params {
    Params::from_json(&doc.params)
}
pub fn model_get(doc: &Doc, id: u64) -> Option<Entity> {
    doc.space_map(cc_cad::doc::MODEL).and_then(|m| m.get(&id)).cloned()
}
fn model_ref(doc: &Doc, id: u64) -> Option<&Entity> {
    doc.space_map(cc_cad::doc::MODEL).and_then(|m| m.get(&id))
}

// ---------------------------------------------------------------- references

/// `constraintPoints(e)`: where a constraint can grab an object.
pub fn constraint_points(e: &Entity) -> Vec<(String, f64, f64)> {
    let mut out = Vec::new();
    let mut add = |k: &str| {
        if let Some(p) = ref_point(e, k) {
            out.push((k.to_string(), p[0], p[1]));
        }
    };
    match &e.geo {
        Geo::Line(_) => {
            add("p1");
            add("p2");
            add("mid");
        }
        Geo::Circle(_) => add("c"),
        Geo::Arc(_) => {
            add("s");
            add("e");
            add("c");
            add("mid");
        }
        Geo::Point(_) => add("p"),
        Geo::Poly(p) => {
            for i in 0..p.pts.len() {
                add(&format!("v{i}"));
            }
        }
        _ => {}
    }
    out
}
/// `nearestPointRef(e, p)` → (key, distance)
pub fn nearest_point_ref(e: &Entity, p: P2) -> Option<(String, f64)> {
    let mut best: Option<(String, f64)> = None;
    for (k, x, y) in constraint_points(e) {
        let d = js::hypot(x - p.x, y - p.y);
        if best.as_ref().is_none_or(|b| d < b.1) {
            best = Some((k, d));
        }
    }
    best
}
/// `curveRef(e, p)`
pub fn curve_ref(e: &Entity, p: P2) -> Option<String> {
    match &e.geo {
        Geo::Line(_) => Some("L".into()),
        Geo::Circle(_) | Geo::Arc(_) => Some("R".into()),
        Geo::Poly(_) => {
            let mut best = None;
            let mut bd = f64::INFINITY;
            for i in 0..seg_count(e) {
                let d = seg_dist(e, i, p);
                if d < bd {
                    bd = d;
                    best = Some(format!("s{i}"));
                }
            }
            best
        }
        _ => None,
    }
}
fn seg_dist(e: &Entity, i: usize, p: P2) -> f64 {
    let Geo::Poly(pl) = &e.geo else { return f64::INFINITY };
    let n = pl.pts.len().max(1);
    let (Some(a), Some(b)) = (pl.pts.get(i), pl.pts.get(i.saturating_add(1) % n)) else { return f64::INFINITY };
    if a.b.filter(|v| !v.is_nan()).unwrap_or(0.0).abs() > 1e-12 {
        let k = ref_circle(e, &format!("s{i}"));
        return (js::hypot(p.x - k[0], p.y - k[1]) - k[2]).abs();
    }
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l2 = dx * dx + dy * dy;
    let l2 = if l2 == 0.0 || l2.is_nan() { 1e-300 } else { l2 };
    let t = js::max(0.0, js::min(1.0, ((p.x - a.x) * dx + (p.y - a.y) * dy) / l2));
    js::hypot(p.x - (a.x + t * dx), p.y - (a.y + t * dy))
}
/// `signature(doc, refs)`
pub fn signature(doc: &Doc, refs: &[Ref]) -> Option<String> {
    let mut s = String::new();
    for r in refs {
        s.push(ref_kind(model_ref(doc, r.e)?, &r.k)?);
    }
    Some(s)
}
fn type_word(tr: &Tr, e: &Entity) -> String {
    let t = e.typ();
    if ["line", "circle", "arc", "poly", "point"].contains(&t) { tr.s(&format!("gcons.word.{t}")) } else { t.to_string() }
}
/// `describeRef(doc, r)`
pub fn describe_ref(doc: &Doc, tr: &Tr, r: &Ref) -> String {
    let Some(e) = model_ref(doc, r.e) else { return tr.s("gcons.deleted_object") };
    let n = tr.t("gcons.ref.object", &[("type", type_word(tr, e)), ("id", r.e.to_string())]);
    match r.k.as_str() {
        "*" | "L" | "R" | "p" => n,
        "p1" | "s" => tr.t("gcons.ref.start", &[("obj", n)]),
        "p2" | "e" => tr.t("gcons.ref.end", &[("obj", n)]),
        "mid" => tr.t("gcons.ref.mid", &[("obj", n)]),
        "c" => tr.t("gcons.ref.centre", &[("obj", n)]),
        k => {
            let i = js::str_to_number(k.get(1..).unwrap_or("")) + 1.0;
            tr.t(if k.starts_with('v') { "gcons.ref.vertex" } else { "gcons.ref.segment" }, &[("obj", n), ("i", js::num_to_string(i))])
        }
    }
}
/// `describeConstraint(doc, c)`
pub fn describe_constraint(doc: &Doc, tr: &Tr, c: &Con) -> String {
    let what = c.refs.iter().map(|r| describe_ref(doc, tr, r)).collect::<Vec<_>>().join(", ");
    let label = type_label(tr, &c.typ);
    if is_dim(c) {
        tr.t("gcons.describe.dim", &[("name", c.name().unwrap_or_default().to_string()), ("kind", label.to_lowercase()), ("what", what)])
    } else {
        tr.t("gcons.describe", &[("label", label), ("what", what)])
    }
}

// ---------------------------------------------------------------- making constraints

/// `nextId(cs)`: 'k' + (the largest number after 'k' + 1).
pub fn next_id(cs: &[Con]) -> String {
    // `parseInt(id.slice(1), 10) || 0` without making a string per id
    let m = cs.iter().fold(0.0f64, |m, c| {
        let tail = c.id.get(1..).unwrap_or("");
        let t = js::trim(tail);
        let (neg, digits) = match t.strip_prefix('-') {
            Some(r) => (true, r),
            None => (false, t.strip_prefix('+').unwrap_or(t)),
        };
        let mut v = 0.0f64;
        let mut any = false;
        for b in digits.bytes().take_while(u8::is_ascii_digit) {
            v = v * 10.0 + f64::from(b - b'0');
            any = true;
        }
        let n = if !any {
            0.0
        } else if v > 9.0e15 {
            gsolve::parse_int(tail)
        } else if neg {
            -v
        } else {
            v
        };
        js::max(m, if n.is_nan() { 0.0 } else { n })
    });
    format!("k{}", js::num_to_string(m + 1.0))
}
/// `nextName(params, prefix, taken)`
pub fn next_name(p: &Params, prefix: &str, taken: &[String]) -> String {
    let mut i = 1u64;
    loop {
        let n = format!("{prefix}{i}");
        if p.var(&n).is_none() && !taken.contains(&n) {
            return n;
        }
        i = i.saturating_add(1);
    }
}

/// `makeConstraint`'s options.
#[derive(Clone, Debug, Default)]
pub struct MakeOpts {
    pub id: Option<String>,
    pub name: Option<String>,
    pub loc: Option<P2>,
    pub value: Option<f64>,
    /// `space: 'model'` (paste / mirror work on model objects whatever space is open)
    pub model: bool,
}

fn sig_words(tr: &Tr, sigs: &[&str]) -> String {
    let w = |k: char| match k {
        'P' => tr.s("gcons.sig.point"),
        'L' => tr.s("gcons.sig.line"),
        'R' => tr.s("gcons.circle_arc"),
        _ => tr.s("gcons.an_object"),
    };
    let one = |s: &str| {
        let mut it = s.chars().map(w);
        let first = it.next().unwrap_or_default();
        it.fold(first, |a, b| tr.t("gcons.sig.and", &[("a", a), ("b", b)]))
    };
    let mut it = sigs.iter().map(|s| one(s));
    let first = it.next().unwrap_or_default();
    it.fold(first, |a, b| tr.t("gcons.sig.or", &[("a", a), ("b", b)]))
}

/// Signed distance of a point (or a circle's centre) from the infinite line l = [x1, y1, x2, y2].
fn sdist(px: f64, py: f64, l: [f64; 4]) -> f64 {
    let (ux, uy) = (l[2] - l[0], l[3] - l[1]);
    let h = js::hypot(ux, uy);
    (ux * (py - l[1]) - uy * (px - l[0])) / if h == 0.0 || h.is_nan() { 1e-300 } else { h }
}
fn mirror_gap(a: P2, b: P2, l: [f64; 4]) -> [f64; 2] {
    let (ux, uy) = (l[2] - l[0], l[3] - l[1]);
    let l2 = ux * ux + uy * uy;
    let l2 = if l2 == 0.0 || l2.is_nan() { 1e-300 } else { l2 };
    let t = ((a.x - l[0]) * ux + (a.y - l[1]) * uy) / l2;
    let (fx, fy) = (l[0] + t * ux, l[1] + t * uy);
    [2.0 * fx - a.x - b.x, 2.0 * fy - a.y - b.y]
}
pub fn intersect_lines(l1: [f64; 4], l2: [f64; 4]) -> P2 {
    let (d1x, d1y, d2x, d2y) = (l1[2] - l1[0], l1[3] - l1[1], l2[2] - l2[0], l2[3] - l2[1]);
    let den = d1x * d2y - d1y * d2x;
    if den.abs() < 1e-300 {
        return P2::new(l1[0], l1[1]);
    }
    let t = ((l2[0] - l1[0]) * d2y - (l2[1] - l1[1]) * d2x) / den;
    P2::new(l1[0] + d1x * t, l1[1] + d1y * t)
}
fn hyp2(v: [f64; 2]) -> f64 {
    js::hypot(v[0], v[1])
}

/// `makeConstraint(doc, type, refs, opts)` → the constraint (not added) or why not.
pub fn make_constraint(doc: &Doc, tr: &Tr, typ: &str, refs: &[Ref], o: &MakeOpts) -> Result<Con, String> {
    if !is_type(typ) {
        return Err(tr.t("gcons.unknown", &[("type", typ.to_string())]));
    }
    if !doc.in_model() && !o.model {
        return Err(tr.s("gcons.constraints_work_model_tab"));
    }
    for r in refs {
        match model_ref(doc, r.e) {
            Some(e) if solvable(e) => {}
            _ => return Err(tr.s("gcons.constraints_work_lines_polylines")),
        }
    }
    let sigs = type_sigs(typ);
    let sig = signature(doc, refs);
    let Some(sig) = sig.filter(|s| sigs.contains(&s.as_str())) else {
        return Err(tr.t("gcons.needs", &[("what", type_label(tr, typ)), ("sigs", sig_words(tr, sigs))]));
    };
    let mut keys: Vec<String> = refs.iter().map(|r| format!("{}:{}", r.e, r.k)).collect();
    keys.sort();
    keys.dedup();
    if keys.len() != refs.len() {
        return Err(tr.s("gcons.pick_two_different_things"));
    }
    let cs = cons(doc);
    let id = o.id.clone().unwrap_or_else(|| next_id(&cs));
    let mut c = Con::new(&id, typ, refs.to_vec());
    let ent = |i: usize| refs.get(i).and_then(|r| model_ref(doc, r.e));
    let key = |i: usize| refs.get(i).map_or("", |r| r.k.as_str());
    let pp = |i: usize| ent(i).and_then(|e| ref_point(e, key(i))).unwrap_or([f64::NAN, f64::NAN]);
    let ll = |i: usize| ent(i).map_or([f64::NAN; 4], |e| ref_line(e, key(i)));
    let cc = |i: usize| ent(i).map_or([f64::NAN; 3], |e| ref_circle(e, key(i)));
    let jn = cc_cad::entity::num;
    match typ {
        "fix" => {
            if sig == "P" {
                let p = pp(0);
                c.set("value", json!([jn(p[0]), jn(p[1])]));
            } else if let Some(e) = ent(0) {
                let mut fixed = Map::new();
                for f in fields_of(e) {
                    fixed.insert(f.clone(), jn(get_field(e, &f)));
                }
                c.set("fixed", Value::Object(fixed));
            }
        }
        "tangent" => {
            if sig == "LR" {
                let k = cc(1);
                c.set("sign", json!(if sdist(k[0], k[1], ll(0)) >= 0.0 { 1 } else { -1 }));
            } else {
                let (a, b) = (cc(0), cc(1));
                let d = js::hypot(b[0] - a[0], b[1] - a[1]);
                if d < js::max(a[2], b[2]) - 1e-12 {
                    c.set("kind", json!("int"));
                    c.set("sign", json!(if a[2] >= b[2] { 1 } else { -1 }));
                }
            }
        }
        "symmetric" => {
            if sig == "LLL" {
                let (a, b, m) = (ll(0), ll(1), ll(2));
                let p = |x: f64, y: f64| P2::new(x, y);
                let same = hyp2(mirror_gap(p(a[0], a[1]), p(b[0], b[1]), m)) + hyp2(mirror_gap(p(a[2], a[3]), p(b[2], b[3]), m));
                let swap = hyp2(mirror_gap(p(a[0], a[1]), p(b[2], b[3]), m)) + hyp2(mirror_gap(p(a[2], a[3]), p(b[0], b[1]), m));
                c.set("pair", json!(if swap < same { 1 } else { 0 }));
            }
        }
        "hdist" | "vdist" => {
            let (a, b) = (pp(0), pp(1));
            let d = if typ == "hdist" { b[0] - a[0] } else { b[1] - a[1] };
            c.set("sign", json!(if d >= 0.0 { 1 } else { -1 }));
        }
        "dist" => {
            if sig == "PL" {
                let p = pp(0);
                c.set("sign", json!(if sdist(p[0], p[1], ll(1)) >= 0.0 { 1 } else { -1 }));
            } else if sig == "LL" {
                let l2 = ll(1);
                c.set("sign", json!(if sdist(l2[0], l2[1], ll(0)) >= 0.0 { 1 } else { -1 }));
            }
        }
        "angle" => {
            if sig == "LL" {
                let (l1, l2) = (ll(0), ll(1));
                let u = [l1[2] - l1[0], l1[3] - l1[1]];
                let v = [l2[2] - l2[0], l2[3] - l2[1]];
                if (u[0] * v[1] - u[1] * v[0]).abs() < 1e-9 * hyp2(u) * hyp2(v) {
                    return Err(tr.s("gcons.those_lines_parallel_there"));
                }
                let (mut s1, mut s2) = (1.0, 1.0);
                if let Some(loc) = o.loc {
                    let x = intersect_lines(l1, l2);
                    let w = [loc.x - x.x, loc.y - x.y];
                    let bis = |a: [f64; 2], b: [f64; 2]| [a[0] / hyp2(a) + b[0] / hyp2(b), a[1] / hyp2(a) + b[1] / hyp2(b)];
                    let mut best = f64::NEG_INFINITY;
                    for a in [1.0, -1.0] {
                        for b in [1.0, -1.0] {
                            let m = bis([u[0] * a, u[1] * a], [v[0] * b, v[1] * b]);
                            let hm = hyp2(m);
                            let sc = (m[0] * w[0] + m[1] * w[1]) / if hm == 0.0 || hm.is_nan() { 1.0 } else { hm };
                            if sc > best {
                                best = sc;
                                s1 = a;
                                s2 = b;
                            }
                        }
                    }
                }
                c.set("dirs", json!([s1 as i64, s2 as i64]));
                let uu = [u[0] * s1, u[1] * s1];
                let vv = [v[0] * s2, v[1] * s2];
                c.set("sign", json!(if (uu[0] * vv[1] - uu[1] * vv[0]).jatan2(uu[0] * vv[0] + uu[1] * vv[1]) >= 0.0 { 1 } else { -1 }));
            } else if ent(0).is_some_and(|e| !matches!(e.geo, Geo::Arc(_))) {
                return Err(tr.s("gcons.angle_needs_two_lines"));
            }
        }
        _ => {}
    }
    if is_dim(&c) {
        let name = o.name.clone().unwrap_or_else(|| next_name(&params_of(doc), dim_prefix(typ), &[]));
        c.set("name", json!(name));
        let get = |id: u64| model_get(doc, id);
        let v = o.value.unwrap_or_else(|| measure(&get, &c));
        c.set("value", jn(v));
        let off = label_offset(&get, &c, o.loc);
        c.set("off", json!([jn(off[0]), jn(off[1])]));
    }
    Ok(c)
}

type Get<'a> = &'a dyn Fn(u64) -> Option<Entity>;

fn rp(get: Get, c: &Con, i: usize) -> [f64; 2] {
    c.refs.get(i).and_then(|r| get(r.e).and_then(|e| ref_point(&e, &r.k))).unwrap_or([f64::NAN, f64::NAN])
}
fn rl(get: Get, c: &Con, i: usize) -> [f64; 4] {
    c.refs.get(i).and_then(|r| get(r.e).map(|e| ref_line(&e, &r.k))).unwrap_or([f64::NAN; 4])
}
fn rc(get: Get, c: &Con, i: usize) -> [f64; 3] {
    c.refs.get(i).and_then(|r| get(r.e).map(|e| ref_circle(&e, &r.k))).unwrap_or([f64::NAN; 3])
}
fn rk(get: Get, c: &Con, i: usize) -> Option<char> {
    c.refs.get(i).and_then(|r| get(r.e).and_then(|e| ref_kind(&e, &r.k)))
}
/// An arc's sweep in degrees ((a1 − a0) mod 2π, a full turn for 0).
fn arc_sweep(e: &Entity) -> Option<(f64, f64, f64, f64, f64)> {
    let Geo::Arc(a) = &e.geo else { return None };
    let mut sw = (a.a1 - a.a0) % TAU;
    if sw <= 0.0 {
        sw += TAU;
    }
    Some((sw, a.cx, a.cy, a.r, a.a0))
}

/// `measure(doc, c, get)`: what a dimensional constraint measures now (degrees for angles).
pub fn measure(get: Get, c: &Con) -> f64 {
    let s = c.sign_or_1();
    match c.typ.as_str() {
        "hdist" => s * (rp(get, c, 1)[0] - rp(get, c, 0)[0]),
        "vdist" => s * (rp(get, c, 1)[1] - rp(get, c, 0)[1]),
        "dist" => {
            if rk(get, c, 1) == Some('P') {
                let (a, b) = (rp(get, c, 0), rp(get, c, 1));
                js::hypot(b[0] - a[0], b[1] - a[1])
            } else if rk(get, c, 0) == Some('P') {
                let p = rp(get, c, 0);
                s * sdist(p[0], p[1], rl(get, c, 1))
            } else {
                let l2 = rl(get, c, 1);
                s * sdist(l2[0], l2[1], rl(get, c, 0))
            }
        }
        "radius" => rc(get, c, 0)[2],
        "diameter" => 2.0 * rc(get, c, 0)[2],
        "angle" => {
            if rk(get, c, 0) == Some('R') {
                let e = c.refs.first().and_then(|r| get(r.e));
                return e.as_ref().and_then(arc_sweep).map_or(f64::NAN, |a| a.0 * 180.0 / PI);
            }
            let (l1, l2) = (rl(get, c, 0), rl(get, c, 1));
            let [s1, s2] = c.dirs().unwrap_or([1.0, 1.0]);
            let u = [(l1[2] - l1[0]) * s1, (l1[3] - l1[1]) * s1];
            let v = [(l2[2] - l2[0]) * s2, (l2[3] - l2[1]) * s2];
            s * (u[0] * v[1] - u[1] * v[0]).jatan2(u[0] * v[0] + u[1] * v[1]) * 180.0 / PI
        }
        _ => 0.0,
    }
}

/// `anchor(doc, c)`: where a dimension's label hangs from.
struct Anchor {
    x: f64,
    y: f64,
    n: Option<[f64; 2]>,
    size: f64,
    r: f64,
}

fn foot(p: [f64; 2], l: [f64; 4]) -> [f64; 2] {
    let (ux, uy) = (l[2] - l[0], l[3] - l[1]);
    let l2 = ux * ux + uy * uy;
    let l2 = if l2 == 0.0 || l2.is_nan() { 1e-300 } else { l2 };
    let t = ((p[0] - l[0]) * ux + (p[1] - l[1]) * uy) / l2;
    [l[0] + t * ux, l[1] + t * uy]
}
/// `dimPoints`: the two points a linear dimensional constraint measures between.
fn dim_points(get: Get, c: &Con) -> ([f64; 2], [f64; 2]) {
    if rk(get, c, 1) == Some('P') {
        return (rp(get, c, 0), rp(get, c, 1));
    }
    if rk(get, c, 0) == Some('P') {
        let p = rp(get, c, 0);
        return (p, foot(p, rl(get, c, 1)));
    }
    let l2 = rl(get, c, 1);
    let p = [l2[0], l2[1]];
    (foot(p, rl(get, c, 0)), p)
}
fn anchor(get: Get, c: &Con) -> Option<Anchor> {
    match c.typ.as_str() {
        "hdist" | "vdist" | "dist" => {
            let (p, q) = dim_points(get, c);
            let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
            let h = js::hypot(dx, dy);
            let l2 = if h == 0.0 || h.is_nan() { 1.0 } else { h };
            let n = if c.typ == "hdist" {
                [0.0, 1.0]
            } else if c.typ == "vdist" {
                [-1.0, 0.0]
            } else {
                [-dy / l2, dx / l2]
            };
            Some(Anchor { x: (p[0] + q[0]) / 2.0, y: (p[1] + q[1]) / 2.0, n: Some(n), size: js::max(l2, 1e-6), r: f64::NAN })
        }
        "radius" | "diameter" => {
            let k = rc(get, c, 0);
            Some(Anchor { x: k[0], y: k[1], n: None, size: f64::NAN, r: k[2] })
        }
        "angle" => {
            if rk(get, c, 0) == Some('R') {
                let e = c.refs.first().and_then(|r| get(r.e))?;
                let Geo::Arc(a) = &e.geo else { return None };
                return Some(Anchor { x: a.cx, y: a.cy, n: None, size: f64::NAN, r: a.r });
            }
            let (l1, l2) = (rl(get, c, 0), rl(get, c, 1));
            let x = intersect_lines(l1, l2);
            Some(Anchor {
                x: x.x,
                y: x.y,
                n: None,
                size: f64::NAN,
                r: js::min(js::hypot(l1[2] - l1[0], l1[3] - l1[1]), js::hypot(l2[2] - l2[0], l2[3] - l2[1])),
            })
        }
        _ => None,
    }
}
/// `labelOffset(doc, c, loc)`
fn label_offset(get: Get, c: &Con, loc: Option<P2>) -> [f64; 2] {
    let Some(a) = anchor(get, c) else { return [0.0, 0.0] };
    if c.typ == "angle" {
        let r = match loc {
            Some(l) => js::hypot(l.x - a.x, l.y - a.y),
            None => a.r * 0.6,
        };
        return [js::max(r, 1e-9), 0.0];
    }
    match loc {
        None => {
            if c.typ == "radius" || c.typ == "diameter" {
                // 0.7071 as the JS writes it (not 1/√2)
                #[allow(clippy::approx_constant)]
                let k = 0.7071;
                return [a.r * k * 1.6, a.r * k * 1.6];
            }
            let n = a.n.unwrap_or([0.0, 1.0]);
            [n[0] * a.size * 0.25, n[1] * a.size * 0.25]
        }
        Some(l) => [l.x - a.x, l.y - a.y],
    }
}

// ---------------------------------------------------------------- display

fn round6(v: f64) -> f64 {
    js::round(v * 1e6) / 1e6
}
/// `labelOf(c, params)`: "d1=40", or "fx: d1=width/2" when an expression drives it.
pub fn label_of(c: &Con, p: Option<&Params>) -> String {
    let name = c.name().unwrap_or_default();
    let ex = match p.and_then(|p| p.var(name)) {
        Some(v) => v.expr.clone(),
        None => expr::format_number(round6(c.value_num())),
    };
    if expr::literal(&ex).is_some() {
        return format!("{name}={}", fmt_value(c, c.value_num()));
    }
    format!("fx: {name}={ex}")
}
/// `fmtValue(c, v)`
pub fn fmt_value(c: &Con, v: f64) -> String {
    if c.typ == "angle" { format!("{}°", expr::format_number(round6(v))) } else { expr::format_number(round6(v)) }
}

/// `dimEntityFor(doc, c, st)`: a dimension entity drawing a dimensional constraint where it is now, labelled. None when
/// the geometry makes it undrawable.
pub fn dim_entity_for(doc: &Doc, c: &Con, st: Option<&cc_cad::entity::Obj>) -> Option<Entity> {
    let get = |id: u64| model_get(doc, id);
    let g: Get = &get;
    if !c.refs.iter().all(|r| get(r.e).is_some_and(|e| ref_kind(&e, &r.k).is_some())) {
        return None;
    }
    let a = anchor(g, c)?;
    let off = c.off();
    let st = st.cloned().unwrap_or_else(|| doc.current.dim.clone());
    let p = |v: [f64; 2]| P2::new(v[0], v[1]);
    let mk = |kind: &str, p1: Option<P2>, p2: Option<P2>, cen: Option<P2>, loc: P2, rot: Option<f64>| {
        let d = Dim {
            kind: Some(kind.into()),
            p1,
            p2,
            c: cen,
            loc: Some(loc),
            rot,
            text: Some(label_of(c, Some(&params_of(doc)))),
            st: Some(st.clone()),
            block: None,
        };
        Entity::with(Geo::Dim(d), Some("0".into()), Some(cc_cad::entity::Color::Null))
    };
    match c.typ.as_str() {
        "hdist" | "vdist" | "dist" => {
            let (p1, p2) = dim_points(g, c);
            if js::hypot(p2[0] - p1[0], p2[1] - p1[1]) < 1e-12 {
                return None;
            }
            let loc = P2::new(a.x + off[0], a.y + off[1]);
            if c.typ == "dist" {
                return Some(mk("aligned", Some(p(p1)), Some(p(p2)), None, loc, None));
            }
            Some(mk("linear", Some(p(p1)), Some(p(p2)), None, loc, Some(if c.typ == "hdist" { 0.0 } else { PI / 2.0 })))
        }
        "radius" | "diameter" => {
            let k = rc(g, c, 0);
            let h = js::hypot(off[0], off[1]);
            let d = if h == 0.0 || h.is_nan() { 1.0 } else { h };
            let u = [off[0] / d, off[1] / d];
            Some(mk(
                &c.typ,
                Some(P2::new(k[0] + u[0] * k[2], k[1] + u[1] * k[2])),
                None,
                Some(P2::new(k[0], k[1])),
                P2::new(k[0] + off[0], k[1] + off[1]),
                None,
            ))
        }
        "angle" => {
            let rr = if off[0] != 0.0 && !off[0].is_nan() { off[0] } else { a.r * 0.6 };
            if rk(g, c, 0) == Some('R') {
                let e = c.refs.first().and_then(|r| get(r.e))?;
                let (sw, cx, cy, r, a0) = arc_sweep(&e)?;
                let Geo::Arc(ar) = &e.geo else { return None };
                let m = a0 + sw / 2.0;
                let cen = P2::new(cx, cy);
                return Some(mk(
                    "angular",
                    Some(P2::new(cen.x + ar.a0.jcos() * r, cen.y + ar.a0.jsin() * r)),
                    Some(P2::new(cen.x + ar.a1.jcos() * r, cen.y + ar.a1.jsin() * r)),
                    Some(cen),
                    P2::new(cen.x + m.jcos() * rr, cen.y + m.jsin() * rr),
                    None,
                ));
            }
            let (l1, l2) = (rl(g, c, 0), rl(g, c, 1));
            let [s1, s2] = c.dirs().unwrap_or([1.0, 1.0]);
            let u = [(l1[2] - l1[0]) * s1, (l1[3] - l1[1]) * s1];
            let v = [(l2[2] - l2[0]) * s2, (l2[3] - l2[1]) * s2];
            let (lu, lv) = (hyp2(u), hyp2(v));
            if (u[0] * v[1] - u[1] * v[0]).abs() < 1e-9 * lu * lv {
                return None;
            }
            let b = [u[0] / lu + v[0] / lv, u[1] / lu + v[1] / lv];
            let hb = hyp2(b);
            let lb = if hb == 0.0 || hb.is_nan() { 1.0 } else { hb };
            let cen = P2::new(a.x, a.y);
            Some(mk(
                "angular",
                Some(P2::new(cen.x + u[0] / lu * rr, cen.y + u[1] / lu * rr)),
                Some(P2::new(cen.x + v[0] / lv * rr, cen.y + v[1] / lv * rr)),
                Some(cen),
                P2::new(cen.x + b[0] / lb * rr, cen.y + b[1] / lb * rr),
                None,
            ))
        }
        _ => None,
    }
}

/// `barsFor(doc)`: each object's constraint bar (coincident and dimensional ones left out).
pub fn bars_for(doc: &Doc) -> Vec<(u64, Vec<Con>)> {
    let mut out: Vec<(u64, Vec<Con>)> = Vec::new();
    for c in cons(doc).iter() {
        if is_dim(c) || c.typ == "coincident" {
            continue;
        }
        for r in c.refs.iter() {
            if model_ref(doc, r.e).is_none() {
                continue;
            }
            match out.iter_mut().find(|(id, _)| *id == r.e) {
                Some((_, l)) => {
                    if !l.iter().any(|k| k.id == c.id) {
                        l.push(c.clone());
                    }
                }
                None => out.push((r.e, vec![c.clone()])),
            }
        }
    }
    out
}

// ---------------------------------------------------------------- solving

/// What `solveConstraints` returns.
pub struct SolveOut {
    pub ok: bool,
    pub updates: Vec<(u64, Entity)>,
    pub conflicts: Vec<String>,
    pub freed: bool,
}

/// `solveConstraints(doc, constraints, { pinned, only, strict, get })`
pub fn solve_constraints(get: Get, cs: &[Con], pinned: Option<&Pins>, only: Option<&[u64]>, strict: bool) -> SolveOut {
    let mut updates: Vec<(u64, Entity)> = Vec::new();
    let mut freed = false;
    let mut conflicts: Vec<String> = Vec::new();
    // only the groups that touch `only` are copied out
    let groups: Vec<Vec<Con>> = components_idx(cs)
        .into_iter()
        .filter(|g| only.is_none_or(|o| g.iter().filter_map(|i| cs.get(*i)).any(|c| c.refs.iter().any(|r| o.contains(&r.e)))))
        .map(|g| g.into_iter().filter_map(|i| cs.get(i).cloned()).collect())
        .collect();
    for g in groups {
        let up = updates.clone();
        let cur = move |id: u64| up.iter().find(|(i, _)| *i == id).map(|(_, e)| e.clone()).or_else(|| get(id));
        let mut r = solve_system(&g, &cur, pinned, 100);
        if !r.ok && pinned.is_some_and(|p| !p.is_empty()) && !strict {
            r = solve_system(&g, &cur, None, 100);
            freed = true;
        }
        if r.ok {
            let flat: Vec<u64> =
                r.updates.iter().filter(|(id, e)| cur(*id).is_some_and(|was| collapsed(&was, e))).map(|(id, _)| *id).collect();
            if !flat.is_empty() {
                conflicts.extend(g.iter().filter(|c| c.refs.iter().any(|q| flat.contains(&q.e))).map(|c| c.id.clone()));
                continue;
            }
        }
        if !r.ok {
            if r.worst.is_empty() {
                conflicts.extend(g.iter().map(|c| c.id.clone()));
            } else {
                conflicts.extend(r.worst.iter().cloned());
            }
            continue;
        }
        for (id, e) in r.updates {
            match updates.iter_mut().find(|(i, _)| *i == id) {
                Some(slot) => slot.1 = e,
                None => updates.push((id, e)),
            }
        }
    }
    SolveOut { ok: conflicts.is_empty(), updates, conflicts, freed }
}

/// `collapsed(was, now)`: did solving shrink a line, a polyline segment or a radius to (almost) nothing?
fn collapsed(was: &Entity, now: &Entity) -> bool {
    let tiny = |a: f64, b: f64| b < 1e-6 * js::max(a, 1e-300) && a > 1e-9;
    match (&was.geo, &now.geo) {
        (Geo::Line(w), Geo::Line(n)) => tiny(js::hypot(w.x2 - w.x1, w.y2 - w.y1), js::hypot(n.x2 - n.x1, n.y2 - n.y1)),
        (Geo::Circle(w), Geo::Circle(n)) => tiny(w.r, n.r),
        (Geo::Arc(w), Geo::Arc(n)) => tiny(w.r, n.r),
        (Geo::Poly(w), Geo::Poly(n)) => {
            let len = n.pts.len().max(1);
            for i in 0..seg_count(now) {
                let j = i.saturating_add(1) % len;
                let (Some(a), Some(b), Some(c), Some(d)) = (w.pts.get(i), w.pts.get(j), n.pts.get(i), n.pts.get(j)) else { continue };
                if tiny(js::hypot(b.x - a.x, b.y - a.y), js::hypot(d.x - c.x, d.y - c.y)) {
                    return true;
                }
            }
            false
        }
        _ => false,
    }
}

/// `status(doc)`: degrees of freedom, conflicts and redundancies of the constrained geometry.
#[derive(Clone, Debug, Default)]
pub struct Status {
    pub count: usize,
    pub dof: usize,
    pub conflicts: Vec<String>,
    pub redundant: Vec<String>,
    pub determined: Vec<u64>,
    pub objects: usize,
}
pub fn status(doc: &Doc) -> Status {
    let all = cons(doc);
    let mut s = Status { count: all.len(), ..Status::default() };
    let cs: Vec<Con> = all.iter().filter(|c| c.refs.iter().all(|r| model_ref(doc, r.e).is_some())).cloned().collect();
    let get = |id: u64| model_get(doc, id);
    for g in components(&cs) {
        let a = analyze(&g, &get);
        s.dof = s.dof.saturating_add(a.dof);
        s.conflicts.extend(a.conflicts);
        s.redundant.extend(a.redundant);
        for id in a.determined {
            if !s.determined.contains(&id) {
                s.determined.push(id);
            }
        }
    }
    let mut objs: Vec<u64> = cs.iter().flat_map(|c| c.refs.iter().map(|r| r.e)).collect();
    objs.sort_unstable();
    objs.dedup();
    s.objects = objs.len();
    s
}
pub fn status_text(doc: &Doc, tr: &Tr) -> String {
    let s = status(doc);
    if s.count == 0 {
        return tr.s("gcons.no_constraints");
    }
    let count =
        tr.t("gcons.status.count", &[("n", s.count.to_string()), ("objects", tr.t("gcons.n.objects", &[("n", s.objects.to_string())]))]);
    let state = if !s.conflicts.is_empty() {
        tr.t("gcons.status.notmet", &[("n", s.conflicts.len().to_string())])
    } else if !s.redundant.is_empty() {
        tr.t("gcons.status.over", &[("n", s.redundant.len().to_string())])
    } else if s.dof == 0 {
        tr.s("gcons.fully_constrained")
    } else {
        tr.t("gcons.status.dof", &[("n", s.dof.to_string())])
    };
    tr.t("gcons.status.line", &[("count", count), ("state", state)])
}

/// `dropDimParams(params, dropped)`: parameters of dimensional constraints going away; their values are written into
/// anything using them.
pub fn drop_dim_params(p: &Params, dropped: &[Con]) -> Params {
    let mut p = p.clone();
    for c in dropped {
        if !is_dim(c) {
            continue;
        }
        params::remove_link(&mut p, &target_key(c));
        let name = c.name().unwrap_or_default();
        if p.var(name).is_some() {
            let _ = params::remove_var(&mut p, name, true);
        }
    }
    p
}

/// `fits(doc, c)`: does a constraint still fit the drawing?
pub fn fits(doc: &Doc, c: &Con) -> bool {
    signature(doc, &c.refs).is_some_and(|s| type_sigs(&c.typ).contains(&s.as_str()))
}

/// `afterEdit(doc, ops)`: before a change is recorded, drop constraints whose objects went, then re-solve what moved
/// with the moved fields pinned. → None, or the message the change is refused with (then rolled back).
pub fn after_edit(doc: &mut Doc, tr: &Tr, before: &[(u64, Option<Entity>)]) -> Result<Option<String>, DocError> {
    if doc.solving || doc.constraints.is_empty() || before.is_empty() {
        return Ok(None);
    }
    let all = cons(doc);
    let ref_ids: Vec<u64> = all.iter().flat_map(|c| c.refs.iter().map(|r| r.e)).collect();
    let hit: Vec<u64> = before.iter().map(|(id, _)| *id).filter(|id| ref_ids.contains(id)).collect();
    if hit.is_empty() {
        return Ok(None);
    }
    let was_of = |id: u64| before.iter().find(|(i, _)| *i == id).and_then(|(_, e)| e.clone());
    let (mut keep, mut dropped): (Vec<Con>, Vec<Con>) = (Vec::new(), Vec::new());
    for c in all.iter().cloned() {
        if fits(doc, &c) { keep.push(c) } else { dropped.push(c) }
    }
    let mut pinned: Pins = Pins::new();
    let mut moved_ids: Vec<u64> = Vec::new();
    for &id in &hit {
        let (Some(was), Some(now)) = (was_of(id), model_get(doc, id)) else { continue };
        if was.typ() != now.typ() {
            continue;
        }
        if let (Geo::Poly(w), Geo::Poly(n)) = (&was.geo, &now.geo)
            && (w.pts.len() != n.pts.len() || w.closed != n.closed)
        {
            continue;
        }
        for f in fields_of(&now) {
            let (a, b) = (get_field(&was, &f), get_field(&now, &f));
            if (a - b).abs() > 1e-12 * js::max(1.0, a.abs()) {
                if !moved_ids.contains(&id) {
                    moved_ids.push(id);
                }
                pinned.insert((id, f));
            }
        }
    }
    let mut notes: Vec<String> = Vec::new();
    if !dropped.is_empty() {
        let p = drop_dim_params(&params_of(doc), &dropped);
        doc.set_parametric(cons_values(&keep), p.to_json())?;
        notes.push(tr.t("gcons.note.dropped", &[("n", dropped.len().to_string())]));
    }
    if !pinned.is_empty() {
        let was_get = |id: u64| if before.iter().any(|(i, _)| *i == id) { was_of(id) } else { model_get(doc, id) };
        let groups: Vec<Vec<Con>> = components_idx(&keep)
            .into_iter()
            .filter(|g| g.iter().filter_map(|i| keep.get(*i)).any(|c| c.refs.iter().any(|r| moved_ids.contains(&r.e))))
            .map(|g| g.into_iter().filter_map(|i| keep.get(i).cloned()).collect::<Vec<Con>>())
            .filter(|g| residual_ok(g, &was_get))
            .collect();
        if !groups.is_empty() {
            let flat: Vec<Con> = groups.into_iter().flatten().collect();
            let get = |id: u64| model_get(doc, id);
            let r = solve_constraints(&get, &flat, Some(&pinned), Some(&moved_ids), false);
            if !r.ok {
                let mut seen: Vec<String> = Vec::new();
                for id in &r.conflicts {
                    if !seen.contains(id) {
                        seen.push(id.clone());
                    }
                }
                let names: Vec<String> = seen
                    .iter()
                    .filter_map(|id| keep.iter().find(|c| &c.id == id))
                    .take(3)
                    .map(|c| describe_constraint(doc, tr, c))
                    .collect();
                return Ok(Some(tr.t("gcons.refuse.follow", &[("names", names.join("; "))])));
            }
            if r.freed
                && moved_ids.iter().all(|id| {
                    let e = r.updates.iter().find(|(i, _)| i == id).map(|(_, e)| e);
                    let was = was_of(*id);
                    match (was, e) {
                        (Some(was), Some(e)) => fields_of(e).iter().all(|f| {
                            let w = get_field(&was, f);
                            (get_field(e, f) - w).abs() <= 1e-7 * js::max(1.0, w.abs())
                        }),
                        _ => true,
                    }
                })
            {
                let held: Vec<String> = keep
                    .iter()
                    .filter(|c| c.refs.iter().any(|q| moved_ids.contains(&q.e)) && (c.typ == "fix" || is_dim(c)))
                    .take(3)
                    .map(|c| describe_constraint(doc, tr, c))
                    .collect();
                let key = if held.is_empty() { "gcons.refuse.hold" } else { "gcons.refuse.hold.list" };
                return Ok(Some(tr.t(key, &[("n", moved_ids.len().to_string()), ("list", held.join("; "))])));
            }
            let followers = r.updates.iter().filter(|(id, _)| !moved_ids.contains(id)).count();
            for (id, e) in r.updates {
                doc.update_in(id, e, cc_cad::doc::MODEL)?;
            }
            if r.freed {
                notes.push(tr.s("gcons.constraints_held_what_you"));
            } else if followers > 0 {
                notes.push(tr.t("gcons.note.followed", &[("n", followers.to_string())]));
            }
        }
    }
    if !notes.is_empty() {
        doc.note(&notes.join(" "));
    }
    Ok(None)
}

// ---------------------------------------------------------------- files

/// `forFile(doc)`: the parametric data of a drawing for a file → None, or (entity id → key, the point strings, text).
pub struct ForFile {
    pub refs: Vec<(u64, String)>,
    pub points: Vec<Vec<String>>,
    pub text: String,
}
pub fn for_file(doc: &Doc) -> Option<ForFile> {
    let cs: Vec<Con> = cons(doc).iter().filter(|c| c.refs.iter().all(|r| model_ref(doc, r.e).is_some())).cloned().collect();
    let p = params_of(doc);
    if cs.is_empty() && !params::has_params(Some(&p)) {
        return None;
    }
    let mut refs: Vec<(u64, String)> = Vec::new();
    let mut types = Map::new();
    for c in &cs {
        for r in c.refs.iter() {
            if !refs.iter().any(|(id, _)| *id == r.e) {
                let key = (refs.len().saturating_add(1)).to_string();
                types.insert(key.clone(), json!(model_ref(doc, r.e).map_or("", |e| e.typ())));
                refs.push((r.e, key));
            }
        }
    }
    let key_of = |id: u64| refs.iter().find(|(i, _)| *i == id).map(|(_, k)| k.clone()).unwrap_or_default();
    let list: Vec<Value> = cs
        .iter()
        .map(|c| {
            let mut o = (*c.obj).clone();
            o.insert("refs".into(), Value::Array(c.refs.iter().map(|r| json!({ "r": key_of(r.e), "k": r.k })).collect()));
            Value::Object(o)
        })
        .collect();
    let payload =
        json!({ "format": "cadcraft-parametric", "version": 1, "refs": Value::Object(types), "constraints": list, "params": p.to_json() });
    let raw = cc_cad_geom::walk::js_stringify(&strip_nonfinite(&payload));
    let mut text = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ('\u{21}'..='\u{7e}').contains(&ch) {
            text.push(ch);
        } else {
            let mut buf = [0u16; 2];
            for u in ch.encode_utf16(&mut buf) {
                text.push_str(&format!("\\u{:04x}", u));
            }
        }
    }
    // 230 UTF-16 units (the text is ASCII now) a point
    let bytes = text.as_bytes();
    let mut points: Vec<Vec<String>> = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let end = i.saturating_add(230).min(bytes.len());
        let piece = text.get(i..end).unwrap_or_default();
        points.push(vec![format!("CCPARAM:{}:{piece}", points.len().saturating_add(1))]);
        i = end;
    }
    Some(ForFile { refs, points, text })
}
/// The parity spelling "$NaN" / "$Inf" back to what JSON.stringify writes for them (null).
fn strip_nonfinite(v: &Value) -> Value {
    match v {
        Value::String(s) if s == "$NaN" || s == "$Inf" || s == "$-Inf" => Value::Null,
        Value::Array(a) => Value::Array(a.iter().map(strip_nonfinite).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), strip_nonfinite(v))).collect()),
        x => x.clone(),
    }
}

/// `parametricText(ents)`: the payload text from the CADCRAFT_PARAMETRIC block's points' first strings.
pub fn parametric_text(firsts: &[String]) -> String {
    let mut parts: Vec<(u64, &str)> = Vec::new();
    for s in firsts {
        let Some(rest) = s.strip_prefix("CCPARAM:") else { continue };
        let Some(colon) = rest.find(':') else { continue };
        let (n, tail) = rest.split_at(colon);
        if n.is_empty() || n.len() > 7 || !n.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        parts.push((n.parse().unwrap_or(0), tail.get(1..).unwrap_or("")));
    }
    parts.sort_by_key(|p| p.0);
    parts.iter().map(|p| p.1).collect()
}
/// `refKeyIn(strings)`: the CCREF key in an entity's CADCRAFT xdata strings.
pub fn ref_key_in(strings: &[String]) -> Option<String> {
    strings.iter().find_map(|s| {
        let d = s.strip_prefix("CCREF:")?;
        (!d.is_empty() && d.len() <= 9 && d.bytes().all(|b| b.is_ascii_digit())).then(|| d.to_string())
    })
}

fn ref_key_ok(k: &str) -> bool {
    matches!(k, "p1" | "p2" | "mid" | "L" | "c" | "R" | "s" | "e" | "p" | "*") || matches!(RK::parse(k), RK::V(_) | RK::Seg(_))
}
/// `FIELD_KEY` = /^(x1|y1|x2|y2|cx|cy|r|a0|a1|x|y|\d{1,6}[xy])$/
fn field_key_ok(f: &str) -> bool {
    matches!(f, "x1" | "y1" | "x2" | "y2" | "cx" | "cy" | "r" | "a0" | "a1" | "x" | "y") || {
        let d = f.strip_suffix('x').or_else(|| f.strip_suffix('y')).unwrap_or("");
        !d.is_empty() && d.len() <= 6 && d.bytes().all(|b| b.is_ascii_digit())
    }
}
fn fin(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|x| x.is_finite())
}

/// What the references of a file / clipboard constraint resolve to: key → (entity id, the entity when known).
pub type RefMap = Vec<(String, u64, Option<Entity>)>;

/// `cleanConstraint(c, refMap)`
fn clean_constraint(c: &Value, ref_map: Option<&RefMap>) -> Option<Con> {
    let o = c.as_object()?;
    let typ = o.get("type")?.as_str()?;
    if !is_type(typ) {
        return None;
    }
    let id = o.get("id")?.as_str()?;
    let digits = id.strip_prefix('k')?;
    if digits.is_empty() || digits.len() > 9 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let rs = o.get("refs")?.as_array()?;
    if rs.is_empty() || rs.len() > 3 {
        return None;
    }
    let mut refs = Vec::new();
    let mut sig = String::new();
    for r in rs {
        let k = r.get("k")?.as_str()?;
        if !ref_key_ok(k) {
            return None;
        }
        let e = match ref_map {
            Some(m) => {
                let key = match r.get("r") {
                    Some(Value::String(s)) => s.clone(),
                    Some(x) if !x.is_null() => js::to_js_string(x),
                    // String(undefined) / String(null) are no key the map has
                    _ => return None,
                };
                let (_, id, ent) = m.iter().find(|(k2, _, _)| *k2 == key)?;
                if let Some(ent) = ent {
                    sig.push(ref_kind(ent, k)?);
                }
                *id
            }
            None => {
                let v = r.get("e")?;
                let f = v.as_f64()?;
                if f.fract() != 0.0 || f < 0.0 {
                    return None;
                }
                v.as_u64()?
            }
        };
        refs.push(Ref { e, k: k.to_string() });
    }
    if !sig.is_empty() && !type_sigs(typ).contains(&sig.as_str()) {
        return None;
    }
    let mut k = Con::new(id, typ, refs);
    match o.get("sign").and_then(Value::as_f64) {
        Some(s) if s == 1.0 || s == -1.0 => k.set("sign", json!(s as i64)),
        _ => {}
    }
    if o.get("kind").and_then(Value::as_str) == Some("int") {
        k.set("kind", json!("int"));
    }
    match o.get("pair").and_then(Value::as_f64) {
        Some(p) if p == 0.0 || p == 1.0 => k.set("pair", json!(p as i64)),
        _ => {}
    }
    if let Some(d) = o.get("dirs").and_then(Value::as_array)
        && d.len() == 2
        && d.iter().all(|x| matches!(x.as_f64(), Some(v) if v == 1.0 || v == -1.0))
    {
        k.set("dirs", Value::Array(d.iter().map(|x| json!(x.as_f64().unwrap_or(1.0) as i64)).collect()));
    }
    if typ == "fix" {
        if let Some(v) = o.get("value").and_then(Value::as_array).filter(|a| a.len() == 2 && a.iter().all(|x| fin(Some(x)).is_some())) {
            k.set("value", Value::Array(v.clone()));
        } else {
            let fx = o.get("fixed").and_then(Value::as_object)?;
            let mut m = Map::new();
            for (f, v) in fx {
                if !field_key_ok(f) || fin(Some(v)).is_none() {
                    return None;
                }
                m.insert(f.clone(), v.clone());
            }
            k.set("fixed", Value::Object(m));
        }
    } else if is_dim_type(typ) {
        let v = fin(o.get("value"))?;
        let name = o.get("name")?.as_str()?;
        if !expr::name_ok(name) {
            return None;
        }
        k.set("value", o.get("value").cloned().unwrap_or_else(|| cc_cad::entity::num(v)));
        k.set("name", json!(name));
        let off = o.get("off").and_then(Value::as_array).filter(|a| a.len() == 2 && a.iter().all(|x| fin(Some(x)).is_some()));
        k.set("off", off.map_or_else(|| json!([0, 0]), |a| Value::Array(a.clone())));
    }
    Some(k)
}

/// `resolveParametric(raw, refMap)`: untrusted parametric data (text) + the file's entities (key → id, entity) →
/// (constraints, params, notes).
pub fn resolve_parametric(tr: &Tr, raw: &Value, ref_map: &RefMap) -> (Vec<Con>, Params, Vec<String>) {
    let mut notes: Vec<String> = Vec::new();
    let p: Value = match raw {
        Value::String(s) => {
            let len16: usize = s.chars().map(char::len_utf16).sum();
            if len16 > LIMIT_PAYLOAD {
                notes.push(tr.s("gcons.file.too_large"));
                return (Vec::new(), Params::default(), notes);
            }
            match serde_json::from_str::<Value>(s) {
                Ok(v) => v,
                Err(_) => {
                    notes.push(tr.s("gcons.file.unreadable"));
                    return (Vec::new(), Params::default(), notes);
                }
            }
        }
        x => x.clone(),
    };
    let list = p.get("constraints").and_then(Value::as_array);
    let good = p.is_object()
        && p.get("format").and_then(Value::as_str) == Some("cadcraft-parametric")
        && p.get("version").and_then(Value::as_f64) == Some(1.0)
        && list.is_some();
    let Some(list) = list.filter(|_| good) else {
        notes.push(tr.s("gcons.file.unknown_form"));
        return (Vec::new(), Params::default(), notes);
    };
    if list.len() > LIMIT_CONSTRAINTS {
        notes.push(tr.s("gcons.file.too_many"));
        return (Vec::new(), Params::default(), notes);
    }
    let mut out: Vec<Con> = Vec::new();
    let mut lost = 0usize;
    for c in list {
        match clean_constraint(c, Some(ref_map)) {
            Some(k) if !out.iter().any(|x| x.id == k.id) => out.push(k),
            _ => lost = lost.saturating_add(1),
        }
    }
    let mut names: Vec<String> = Vec::new();
    let mut kept: Vec<Con> = Vec::new();
    for c in out {
        if is_dim(&c) {
            let n = c.name().unwrap_or_default().to_string();
            if names.contains(&n) {
                lost = lost.saturating_add(1);
                continue;
            }
            names.push(n);
        }
        kept.push(c);
    }
    let mut prm = Params::default();
    if let Some(pp) = p.get("params").filter(|v| js::truthy(Some(v))) {
        let links_in = pp.get("links").and_then(Value::as_array);
        let keys: Vec<String> = kept.iter().filter(|c| is_dim(c)).map(target_key).collect();
        let links: Vec<Value> = links_in
            .map(|a| {
                a.iter().filter(|l| l.get("target").and_then(Value::as_str).is_some_and(|t| keys.iter().any(|k| k == t))).cloned().collect()
            })
            .unwrap_or_default();
        let dropped_links = links_in.map_or(0, Vec::len).saturating_sub(links.len());
        let vars = pp.get("vars").filter(|v| !v.is_null()).cloned().unwrap_or_else(|| json!([]));
        let set: std::collections::HashSet<String> = keys.iter().cloned().collect();
        match params::read_params(&json!({ "vars": vars, "links": links }), Some(&set)) {
            Ok(x) => prm = x,
            Err(why) => {
                notes.push(tr.t("gcons.note.params_unreadable", &[("why", why)]));
                prm = Params::default();
            }
        }
        if dropped_links > 0 {
            notes.push(tr.t("gcons.note.links_dropped", &[("n", dropped_links.to_string())]));
        }
    }
    for c in &mut kept {
        if !is_dim(c) {
            continue;
        }
        let name = c.name().unwrap_or_default().to_string();
        if prm.var(&name).is_none() {
            let mut nm = name.clone();
            if params::name_problem(Some(&json!(name)), Some(&prm), None).is_some() {
                nm = next_name(&prm, dim_prefix(&c.typ), &[]);
                c.set("name", json!(nm));
            }
            prm.vars.push(params::Var { name: nm, expr: expr::format_number(round6(c.value_num())), note: None });
        }
        let tk = target_key(c);
        if !prm.links.iter().any(|l| l.target == tk) {
            prm.links.push(params::Link { target: tk, expr: c.name().unwrap_or_default().to_string() });
        }
    }
    if lost > 0 {
        notes.push(tr.t("gcons.note.lost", &[("n", lost.to_string())]));
    }
    (kept, prm, notes)
}

/// `fitsIn(model, c)`
fn fits_in(doc: &Doc, c: &Con) -> bool {
    fits(doc, c)
}

// ---------------------------------------------------------------- clipboard

/// `clipConstraints(doc, index)`: constraints among copied objects, refs as indexes (`index`: id → index).
pub fn clip_constraints(doc: &Doc, index: &[(u64, usize)]) -> Vec<Value> {
    let mut out = Vec::new();
    if !doc.in_model() {
        return out;
    }
    let p = params_of(doc);
    for c in cons(doc).iter() {
        let idx: Option<Vec<usize>> = c.refs.iter().map(|r| index.iter().find(|(id, _)| *id == r.e).map(|(_, i)| *i)).collect();
        let Some(idx) = idx else { continue };
        let mut o = (*c.obj).clone();
        o.insert("refs".into(), Value::Array(c.refs.iter().zip(&idx).map(|(r, i)| json!({ "i": i, "k": r.k })).collect()));
        if is_dim(c) {
            let e = match p.var(c.name().unwrap_or_default()) {
                Some(v) => v.expr.clone(),
                None => expr::format_number(c.value_num()),
            };
            o.insert("expr".into(), json!(e));
        }
        out.push(Value::Object(o));
    }
    out
}

/// `readClipConstraints(list, nEntities)`: untrusted clipboard constraints → clean ones (refs by index), or Err.
#[allow(clippy::result_unit_err)]
pub fn read_clip_constraints(list: Option<&Value>, n_entities: usize) -> Result<Vec<Value>, ()> {
    let Some(list) = list else { return Ok(Vec::new()) };
    let Some(a) = list.as_array().filter(|a| a.len() <= LIMIT_CONSTRAINTS) else { return Err(()) };
    let map: RefMap = (0..n_entities).map(|i| (i.to_string(), u64::try_from(i).unwrap_or(u64::MAX), None)).collect();
    let mut out = Vec::with_capacity(a.len());
    for c in a {
        let refs = c.get("refs").and_then(Value::as_array).map(|rs| {
            Value::Array(
                rs.iter()
                    .map(|r| {
                        let i = r.get("i").and_then(Value::as_f64).filter(|v| v.fract() == 0.0 && *v >= 0.0 && *v < n_entities as f64);
                        json!({ "r": i.map_or(Value::Null, |v| json!(js::num_to_string(v))), "k": r.get("k").cloned().unwrap_or(Value::Null) })
                    })
                    .collect(),
            )
        });
        let mut o = c.as_object().cloned().unwrap_or_default();
        o.insert("refs".into(), refs.unwrap_or(Value::Null));
        let mut k = clean_constraint(&Value::Object(o), Some(&map)).ok_or(())?;
        if is_dim(&k) {
            let e = c
                .get("expr")
                .and_then(Value::as_str)
                .filter(|e| e.chars().map(char::len_utf16).sum::<usize>() <= 1000)
                .map_or_else(|| expr::format_number(k.value_num()), str::to_string);
            k.set("expr", json!(e));
        }
        let mut obj = (*k.obj).clone();
        obj.insert("refs".into(), Value::Array(k.refs.iter().map(|r| json!({ "i": r.e, "k": r.k })).collect()));
        out.push(Value::Object(obj));
    }
    Ok(out)
}

/// `pasteConstraints(doc, list, ids, dx, dy)`: inside the paste's change → how many were added.
pub fn paste_constraints(doc: &mut Doc, list: &[Value], ids: &[Option<u64>], dx: f64, dy: f64) -> Result<usize, DocError> {
    if list.is_empty() || !doc.in_model() {
        return Ok(0);
    }
    let mut cs = cons(doc).to_vec();
    let mut prm = params_of(doc);
    let mut n = 0usize;
    for c in list {
        let Some(o) = c.as_object() else { continue };
        let refs_in: Vec<(usize, String)> = o
            .get("refs")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter().filter_map(|r| Some((usize::try_from(r.get("i")?.as_u64()?).ok()?, r.get("k")?.as_str()?.to_string()))).collect()
            })
            .unwrap_or_default();
        let new_ids: Option<Vec<u64>> =
            refs_in.iter().map(|(i, _)| ids.get(*i).copied().flatten().filter(|id| model_ref(doc, *id).is_some())).collect();
        let Some(new_ids) = new_ids else { continue };
        let mut k = Con {
            id: String::new(),
            typ: o.get("type").and_then(Value::as_str).unwrap_or_default().to_string(),
            refs: std::sync::Arc::new(Vec::new()),
            obj: std::sync::Arc::new(o.clone()),
        };
        k.set_id(&next_id(&cs));
        k.set_refs(refs_in.iter().zip(&new_ids).map(|((_, kk), id)| Ref { e: *id, k: kk.clone() }).collect());
        let cexpr = o.get("expr").and_then(Value::as_str).unwrap_or_default().to_string();
        k.remove("expr");
        if k.typ == "fix" {
            if let Some(v) = k.value_pair() {
                k.set("value", json!([cc_cad::entity::num(v[0] + dx), cc_cad::entity::num(v[1] + dy)]));
            }
            if let Some(f) = k.get("fixed").and_then(Value::as_object).cloned() {
                let mut m = Map::new();
                for (key, v) in f {
                    let x = v.as_f64().unwrap_or(f64::NAN);
                    let is_x = matches!(key.as_str(), "x1" | "x2" | "cx" | "x")
                        || (key.ends_with('x')
                            && key.len() > 1
                            && key.get(..key.len() - 1).is_some_and(|d| d.bytes().all(|b| b.is_ascii_digit())));
                    let is_y = matches!(key.as_str(), "y1" | "y2" | "cy" | "y")
                        || (key.ends_with('y')
                            && key.len() > 1
                            && key.get(..key.len() - 1).is_some_and(|d| d.bytes().all(|b| b.is_ascii_digit())));
                    let nv = if is_x {
                        x + dx
                    } else if is_y {
                        x + dy
                    } else {
                        x
                    };
                    m.insert(key, if is_x || is_y { cc_cad::entity::num(nv) } else { v });
                }
                k.set("fixed", Value::Object(m));
            }
        }
        if !fits_in(doc, &k) {
            continue;
        }
        if is_dim(&k) {
            let nm = next_name(&prm, dim_prefix(&k.typ), &[]);
            k.set("name", json!(nm));
            let uses: Vec<String> = words(&cexpr).into_iter().filter(|w| !literal_word(w)).collect();
            let keep_expr = uses.iter().all(|w| prm.var(w).is_some()) && expr::literal(&cexpr).is_none();
            prm.vars.push(params::Var {
                name: nm.clone(),
                expr: if keep_expr { cexpr.clone() } else { expr::format_number(round6(k.value_num())) },
                note: None,
            });
            prm.links.push(params::Link { target: target_key(&k), expr: nm });
        }
        cs.push(k);
        n = n.saturating_add(1);
    }
    if n > 0 {
        doc.set_parametric(cons_values(&cs), prm.to_json())?;
    }
    Ok(n)
}
/// `/[A-Za-z_][A-Za-z0-9_]*/g` matches
fn words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in s.chars() {
        let start = ch.is_ascii_alphabetic() || ch == '_';
        let cont = start || ch.is_ascii_digit();
        if cur.is_empty() {
            if start {
                cur.push(ch);
            }
        } else if cont {
            cur.push(ch);
        } else {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}
fn literal_word(w: &str) -> bool {
    matches!(
        w.to_ascii_lowercase().as_str(),
        "mm" | "cm"
            | "m"
            | "in"
            | "ft"
            | "deg"
            | "rad"
            | "pi"
            | "sin"
            | "cos"
            | "tan"
            | "asin"
            | "acos"
            | "atan"
            | "atan2"
            | "sqrt"
            | "pow"
            | "exp"
            | "ln"
            | "log"
            | "abs"
            | "sign"
            | "ceil"
            | "floor"
            | "int"
            | "round"
            | "min"
            | "max"
            | "if"
            | "e"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file's fixed fields may hold any text: a key ending in a multi-byte character was cut inside it (fuzz,
    /// cad_parametric).
    #[test]
    fn field_keys_with_multibyte_text() {
        assert!(field_key_ok("12x") && field_key_ok("cy") && !field_key_ok("x\u{fffd}") && !field_key_ok("1\u{fffd}y\u{fffd}"));
        let raw = json!({ "format": "cadcraft-parametric", "version": 1, "constraints": [
            { "id": "k1", "type": "fix", "refs": [{ "r": "1", "k": "*" }], "fixed": { "x\u{fffd}": 1, "1\u{e9}": 2 } }] });
        let map: RefMap = vec![("1".into(), 1, None)];
        let (cs, _, notes) = resolve_parametric(&Tr::default(), &raw, &map);
        assert!(cs.is_empty() && !notes.is_empty());
    }
}
