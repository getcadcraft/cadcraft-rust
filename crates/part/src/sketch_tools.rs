//! sketch-tools.js — sketch editing tools; each mutates the sketch it is given (one undo step in the editor) and says
//! how many existing relations it dropped: trim (pieces between crossings; lines and arcs keep their direction
//! relations, a circle becomes an arc, a spline becomes exact control-point pieces), extend, chains and offsets
//! (lines and arcs exactly with mitred corners, splines by a fitted spline within `OFFSET_TOL`), mirror, linear and
//! circular patterns, polygons, slots and projected model edges.
//!
//! Entities are found again by id after every change (the JS holds live object references; ids are unique).

use crate::i18n::{F, t};
use crate::js;
use crate::jv;
use crate::omap::OMap;
use crate::sketch::{self as s, At, Ent, ORIGIN};
use crate::sketch_intersect::{self as si, Curve};
use crate::spline::{self as sp, P2};
use serde_json::Value;
use std::collections::HashSet;

pub const OFFSET_TOL: f64 = 0.001;

fn dist(a: P2, b: P2) -> f64 {
    js::hypot2(b[0] - a[0], b[1] - a[1])
}
fn tol_of(sk: &Value) -> f64 {
    1e-7 * js::max(1.0, s::extent(sk))
}
fn set_pt(sk: &mut Value, id: &str, p: P2) {
    if let Some(e) = s::by_id_mut(sk, id) {
        jv::set(e, "x", jv::num(p[0]));
        jv::set(e, "y", jv::num(p[1]));
    }
}
fn used_by(sk: &Value, pid: &str, except: &str) -> bool {
    s::entities(sk).iter().any(|e| {
        let en = Ent::E(e);
        en.ty() != "point" && en.id() != except && s::points_of(&en).iter().any(|p| p == pid)
    })
}
fn cids(sk: &Value) -> Vec<String> {
    s::constraints(sk).iter().map(|c| js::str_of(c.get("id"))).collect()
}
fn dropped_since(sk: &Value, before: &[String]) -> usize {
    let now: HashSet<String> = cids(sk).into_iter().collect();
    before.iter().filter(|id| !now.contains(*id)).count()
}
fn refs_of(c: &Value) -> Vec<String> {
    jv::arr(c.get("refs")).iter().map(|v| js::str_of(Some(v))).collect()
}

fn line_keeps(sk: &Value, c: &Value) -> bool {
    let ty = jv::s(c, "type");
    if matches!(
        ty,
        "horizontal"
            | "vertical"
            | "parallel"
            | "perpendicular"
            | "angle"
            | "coincident"
            | "tangent"
            | "symmetric"
            | "offset"
            | "offsetlink"
    ) {
        return true;
    }
    if ty == "distance" {
        return s::match_sig(sk, ty, &refs_of(c)).ok().flatten().is_some_and(|m| m.0 != "L");
    }
    false
}
fn round_keeps(_sk: &Value, c: &Value) -> bool {
    matches!(jv::s(c, "type"), "radius" | "diameter" | "concentric" | "tangent" | "equal" | "coincident" | "offset" | "offsetlink")
}

fn drop_on_point(sk: &mut Value, pid: &str) {
    if let Some(v) = s::constraints_mut(sk) {
        v.retain(|c| !refs_of(c).iter().any(|r| r == pid));
    }
}
fn filter_curve(sk: &mut Value, id: &str, keep: fn(&Value, &Value) -> bool) {
    let ids: Vec<String> =
        s::constraints(sk).iter().filter(|c| refs_of(c).iter().any(|r| r == id) && !keep(sk, c)).map(|c| js::str_of(c.get("id"))).collect();
    s::remove_constraints(sk, &ids);
}

/// `ownPoint(sk, e, key)`: the point of the curve that must move (a new one when another curve shares it).
fn own_point(sk: &mut Value, eid: &str, key: &str) -> String {
    let pid = s::by_id(sk, eid).map(|e| e.str(key).to_string()).unwrap_or_default();
    if !used_by(sk, &pid, eid) {
        return pid;
    }
    let q = s::pxy(sk, &pid);
    let id = s::add_point(sk, q[0], q[1], false);
    if let Some(e) = s::by_id_mut(sk, eid) {
        jv::set(e, key, id.as_str().into());
    }
    id
}

/// `tieTo(sk, pid, cutter)`.
fn tie_to(sk: &mut Value, pid: &str, cutter: Option<&str>) {
    let Some(cid) = cutter else { return };
    let Some(cut) = s::by_id(sk, cid).map(|e| e.value()) else { return };
    let ce = Ent::E(&cut);
    let at = s::pxy(sk, pid);
    let tol = 1e3 * tol_of(sk);
    for q in s::ends_of(&ce) {
        if dist(s::pxy(sk, &q), at) <= tol {
            let _ = s::add_constraint(sk, "coincident", &[pid.to_string(), q], None, None);
            return;
        }
    }
    if matches!(ce.ty(), "line" | "circle" | "arc") {
        let _ = s::add_constraint(sk, "coincident", &[pid.to_string(), cid.to_string()], None, None);
    }
}

/// A crossing of a curve with another: parameter, point, the other curve's id.
#[derive(Clone, Debug)]
pub struct Cut {
    pub t: f64,
    pub p: P2,
    pub by: String,
}

/// `crossings(sk, e)` → (curve, cuts sorted along it, (lo, hi, periodic)).
pub fn crossings(sk: &Value, eid: &str) -> Result<(Curve, Vec<Cut>, (f64, f64, bool)), String> {
    let e = s::by_id(sk, eid).ok_or_else(|| "Cannot read properties of null".to_string())?;
    let cv = si::curve_of(sk, &e)?.ok_or_else(|| "Cannot read properties of null".to_string())?;
    let tol = 1e3 * tol_of(sk);
    let r = si::range_of(&cv);
    let ends: Vec<P2> = if r.2 { Vec::new() } else { vec![si::point_on(&cv, r.0), si::point_on(&cv, r.1)] };
    let mut out: Vec<Cut> = Vec::new();
    for o in s::entities(sk) {
        let oe = Ent::E(o);
        if oe.id() == eid || oe.ty() == "point" {
            continue;
        }
        let Ok(Some(co)) = si::curve_of(sk, &oe) else { continue };
        for x in si::intersect(&cv, &co, false, false) {
            if ends.iter().any(|q| dist(*q, x.p) <= tol) {
                continue;
            }
            if out.iter().any(|y| dist(y.p, x.p) <= tol) {
                continue;
            }
            out.push(Cut { t: x.ta, p: x.p, by: oe.id().to_string() });
        }
    }
    cc_m3d::sort::timsort(&mut out, |a, b| a.t - b.t);
    Ok((cv, out, r))
}

/// `trimPiece(sk, id, at)` → (a, b, hi, period), or None.
pub fn trim_piece(sk: &Value, id: &str, at: P2) -> Option<(f64, f64, f64, f64)> {
    let e = s::by_id(sk, id)?;
    if e.ty() == "point" {
        return None;
    }
    let (cv, cuts, range) = crossings(sk, id).ok()?;
    let mut tc = si::param_of(&cv, at);
    if range.2 {
        let tt = range.1 - range.0;
        if cuts.len() < 2 {
            return Some((range.0, range.1, range.1, 0.0));
        }
        let (mut lo, mut hi): (Option<&Cut>, Option<&Cut>) = (None, None);
        for c in &cuts {
            if c.t < tc {
                lo = Some(c);
            }
            if c.t > tc && hi.is_none() {
                hi = Some(c);
            }
        }
        let a = lo.or(cuts.last()).map_or(f64::NAN, |c| c.t);
        let b0 = hi.or(cuts.first()).map_or(f64::NAN, |c| c.t);
        return Some((a, if b0 > a { b0 } else { b0 + tt }, range.1, tt));
    }
    if let Curve::Arc { sweep, .. } = cv
        && tc > sweep
    {
        tc = if tc - sweep < 2.0 * std::f64::consts::PI - tc { sweep } else { 0.0 };
    }
    tc = js::max(range.0, js::min(range.1, tc));
    let (mut a, mut b) = (range.0, range.1);
    for c in &cuts {
        if c.t < tc {
            a = c.t;
        }
        if c.t > tc && b == range.1 {
            b = js::min(b, c.t);
        }
    }
    Some((a, b, range.1, 0.0))
}

/// What trim / extend / offset / mirror / patterns made.
#[derive(Clone, Debug, Default)]
pub struct Made {
    pub removed: Option<&'static str>,
    pub made: Vec<String>,
    pub dropped: usize,
}
impl Made {
    pub fn to_json(&self) -> Value {
        let mut o = jv::obj(vec![]);
        if let Some(r) = self.removed {
            jv::set(&mut o, "removed", r.into());
        }
        jv::set(&mut o, "made", jv::strs(&self.made));
        jv::set(&mut o, "dropped", jv::unum(self.dropped));
        o
    }
}

fn push_entity(sk: &mut Value, e: Value) {
    if let Some(v) = s::entities_mut(sk) {
        v.push(e);
    }
}

/// `trim(sk, id, at)`.
pub fn trim(sk: &mut Value, id: &str, at: P2) -> Result<Made, String> {
    let Some(e) = s::by_id(sk, id).map(|e| (e.value(), matches!(e, Ent::Origin))) else {
        return Err(t("model.sketch_tools.trim_pick", &[]));
    };
    let (ev, origin) = e;
    if origin || jv::s(&ev, "type") == "point" {
        return Err(t("model.sketch_tools.trim_pick", &[]));
    }
    let ety = jv::s(&ev, "type").to_string();
    let construction = jv::truthy(ev.get("construction"));
    let before = cids(sk);
    let (cv, cuts, range) = crossings(sk, id)?;
    let mut tc = si::param_of(&cv, at);
    if !range.2 {
        let v = match cv {
            Curve::Arc { sweep, .. } if tc > sweep => {
                if tc - sweep < 2.0 * std::f64::consts::PI - tc {
                    sweep
                } else {
                    0.0
                }
            }
            _ => tc,
        };
        tc = js::max(range.0, js::min(range.1, v));
    }
    let done =
        |sk: &Value, removed: &'static str, made: Vec<String>| Made { removed: Some(removed), made, dropped: dropped_since(sk, &before) };
    if range.2 {
        if cuts.len() < 2 {
            s::remove_entities(sk, &[id.to_string()]);
            return Ok(done(sk, "curve", Vec::new()));
        }
        let (mut a, mut b): (Option<Cut>, Option<Cut>) = (None, None);
        for c in &cuts {
            if c.t < tc {
                a = Some(c.clone());
            }
            if c.t > tc && b.is_none() {
                b = Some(c.clone());
            }
        }
        let a = a.or_else(|| cuts.last().cloned()).unwrap_or(Cut { t: f64::NAN, p: [f64::NAN; 2], by: String::new() });
        let b = b.or_else(|| cuts.first().cloned()).unwrap_or(Cut { t: f64::NAN, p: [f64::NAN; 2], by: String::new() });
        if ety == "circle" {
            let s_id = s::add_point(sk, b.p[0], b.p[1], false);
            let e_id = s::add_point(sk, a.p[0], a.p[1], false);
            let aid = s::next_id(sk, "a");
            let mut arc = jv::obj(vec![
                ("id", aid.as_str().into()),
                ("type", "arc".into()),
                ("c", ev.get("c").cloned().unwrap_or(Value::Null)),
                ("s", s_id.as_str().into()),
                ("e", e_id.as_str().into()),
            ]);
            if construction {
                jv::set(&mut arc, "construction", true.into());
            }
            push_entity(sk, arc);
            filter_curve(sk, id, round_keeps);
            if let Some(cs) = s::constraints_mut(sk) {
                for c in cs.iter_mut() {
                    let refs: Vec<Value> = refs_of(c).into_iter().map(|r| Value::String(if r == id { aid.clone() } else { r })).collect();
                    jv::set(c, "refs", Value::Array(refs));
                }
            }
            if let Some(v) = s::entities_mut(sk) {
                v.retain(|x| !jv::is_str(x.get("id"), id));
            }
            tie_to(sk, &s_id, Some(&b.by));
            tie_to(sk, &e_id, Some(&a.by));
            return Ok(done(sk, "piece", vec![aid]));
        }
        let ub = b.t;
        let ua = if a.t <= ub { a.t + (range.1 - range.0) } else { a.t };
        let made = spline_pieces(sk, &ev, &cv, &[(ub, ua, Some(b), Some(a))])?;
        return Ok(done(sk, "piece", made));
    }
    let (mut lo, mut hi): (Option<Cut>, Option<Cut>) = (None, None);
    for c in &cuts {
        if c.t < tc {
            lo = Some(c.clone());
        }
        if c.t > tc && hi.is_none() {
            hi = Some(c.clone());
        }
    }
    if lo.is_none() && hi.is_none() {
        s::remove_entities(sk, &[id.to_string()]);
        return Ok(done(sk, "curve", Vec::new()));
    }
    if ety == "line" || ety == "arc" {
        let (k1, k2) = if ety == "line" { ("p1", "p2") } else { ("s", "e") };
        filter_curve(sk, id, if ety == "line" { line_keeps } else { round_keeps });
        let (Some(lo_c), Some(hi_c)) = (lo.clone(), hi.clone()) else {
            let (key, c) = match (&lo, &hi) {
                (None, Some(h)) => (k1, h.clone()),
                (Some(l), None) => (k2, l.clone()),
                _ => return Ok(done(sk, "piece", vec![id.to_string()])),
            };
            let p = own_point(sk, id, key);
            drop_on_point(sk, &p);
            set_pt(sk, &p, c.p);
            tie_to(sk, &p, Some(&c.by));
            return Ok(done(sk, "piece", vec![id.to_string()]));
        };
        // split: this curve keeps its start → lo; a new one runs hi → its end
        let end = s::by_id(sk, id).map(|e| e.str(k2).to_string()).unwrap_or_default();
        let n_a = s::add_point(sk, lo_c.p[0], lo_c.p[1], false);
        let n_b = s::add_point(sk, hi_c.p[0], hi_c.p[1], false);
        if let Some(e) = s::by_id_mut(sk, id) {
            jv::set(e, k2, n_a.as_str().into());
        }
        if ety == "line" {
            let lid = s::next_id(sk, "l");
            let mut l2 = jv::obj(vec![
                ("id", lid.as_str().into()),
                ("type", "line".into()),
                ("p1", n_b.as_str().into()),
                ("p2", end.as_str().into()),
            ]);
            if construction {
                jv::set(&mut l2, "construction", true.into());
            }
            push_entity(sk, l2);
            for q in [n_b.clone(), end.clone()] {
                let _ = s::add_constraint(sk, "coincident", &[q, id.to_string()], None, None);
            }
            tie_to(sk, &n_a, Some(&lo_c.by));
            tie_to(sk, &n_b, Some(&hi_c.by));
            return Ok(done(sk, "piece", vec![id.to_string(), lid]));
        }
        let aid = s::next_id(sk, "a");
        let mut a2 = jv::obj(vec![
            ("id", aid.as_str().into()),
            ("type", "arc".into()),
            ("c", ev.get("c").cloned().unwrap_or(Value::Null)),
            ("s", n_b.as_str().into()),
            ("e", end.as_str().into()),
        ]);
        if construction {
            jv::set(&mut a2, "construction", true.into());
        }
        push_entity(sk, a2);
        s::add_constraint(sk, "equal", &[id.to_string(), aid.clone()], None, None)?;
        tie_to(sk, &n_a, Some(&lo_c.by));
        tie_to(sk, &n_b, Some(&hi_c.by));
        return Ok(done(sk, "piece", vec![id.to_string(), aid]));
    }
    // open spline
    let mut keep: Vec<(f64, f64, Option<Cut>, Option<Cut>)> = Vec::new();
    if let Some(l) = lo {
        keep.push((range.0, l.t, None, Some(l)));
    }
    if let Some(h) = hi {
        keep.push((h.t, range.1, Some(h), None));
    }
    let made = spline_pieces(sk, &ev, &cv, &keep)?;
    Ok(done(sk, "piece", made))
}

/// `splinePieces(sk, e, cv, ranges)`.
fn spline_pieces(sk: &mut Value, ev: &Value, cv: &Curve, ranges: &[(f64, f64, Option<Cut>, Option<Cut>)]) -> Result<Vec<String>, String> {
    let Curve::Spline { bs, .. } = cv else { return Ok(Vec::new()) };
    let eid = jv::s(ev, "id").to_string();
    let pts: Vec<String> = jv::arr(ev.get("pts")).iter().map(|v| js::str_of(Some(v))).collect();
    let first = pts.first().cloned().unwrap_or_default();
    let last = pts.last().cloned().unwrap_or_default();
    let closed = jv::truthy(ev.get("closed"));
    let mut made = Vec::new();
    let mut remap: OMap<String> = OMap::new();
    for (ua, ub, ca, cb) in ranges {
        let seg = sp::normalize_knots(&sp::segment(bs, *ua, *ub));
        let n = seg.poles.len();
        let mut ids: Vec<At> = Vec::with_capacity(n);
        let mut id_strs: Vec<String> = Vec::with_capacity(n);
        for (i, q) in seg.poles.iter().enumerate() {
            let id = if i == 0 && ca.is_none() && !closed {
                first.clone()
            } else if i == n.saturating_sub(1) && cb.is_none() && !closed {
                last.clone()
            } else {
                s::add_point(sk, q[0], q[1], false)
            };
            id_strs.push(id.clone());
            ids.push(At::Id(id));
        }
        let mut opts = jv::obj(vec![("kind", "control".into()), ("knots", jv::nums(&seg.knots))]);
        if let Some(c) = ev.get("construction") {
            jv::set(&mut opts, "construction", c.clone());
        }
        let r = s::add_spline(sk, &ids, &opts)?;
        let rid = jv::s(&r, "id").to_string();
        made.push(rid.clone());
        if ca.is_none() {
            remap.set(&first, rid.clone());
        }
        if cb.is_none() {
            remap.set(&last, rid.clone());
        }
        if let Some(c) = ca {
            tie_to(sk, id_strs.first().map_or("", String::as_str), Some(&c.by));
        }
        if let Some(c) = cb {
            tie_to(sk, id_strs.get(n.saturating_sub(1)).map_or("", String::as_str), Some(&c.by));
        }
    }
    let ent = Ent::E(ev);
    let n = s::constraints(sk).len();
    for i in 0..n {
        let Some(c) = s::constraints(sk).get(i) else { continue };
        if jv::s(c, "type") != "tangent" {
            continue;
        }
        let refs = refs_of(c);
        if !refs.contains(&eid) {
            continue;
        }
        let other = refs.iter().find(|r| **r != eid).cloned();
        let meet = match other.as_deref().and_then(|o| s::by_id(sk, o)) {
            Some(o) => s::end_meet(sk, &ent, &o),
            None => s::end_meet(sk, &ent, &Ent::E(&Value::Null)),
        };
        let to = meet.and_then(|m| remap.get(&m.0).cloned());
        if let Some(to) = to
            && let Some(cm) = s::constraints_mut(sk).and_then(|v| v.get_mut(i))
        {
            let nr: Vec<Value> = refs.into_iter().map(|r| Value::String(if r == eid { to.clone() } else { r })).collect();
            jv::set(cm, "refs", Value::Array(nr));
        }
    }
    s::remove_entities(sk, &[eid]);
    Ok(made)
}

/// `extend(sk, id, at)`.
pub fn extend(sk: &mut Value, id: &str, at: P2) -> Result<Made, String> {
    let ety = s::by_id(sk, id).map(|e| e.ty().to_string()).unwrap_or_default();
    if ety != "line" && ety != "arc" {
        return Err(t("model.sketch_tools.extend_pick", &[]));
    }
    let before = cids(sk);
    let cv = {
        let e = s::by_id(sk, id).ok_or_else(|| t("model.sketch_tools.extend_pick", &[]))?;
        si::curve_of(sk, &e)?.ok_or_else(|| t("model.sketch_tools.extend_pick", &[]))?
    };
    let tol = 1e3 * tol_of(sk);
    let key = {
        let e = s::by_id(sk, id).ok_or_else(|| t("model.sketch_tools.extend_pick", &[]))?;
        if ety == "line" {
            if dist(at, s::pxy(sk, e.str("p1"))) < dist(at, s::pxy(sk, e.str("p2"))) { "p1" } else { "p2" }
        } else if dist(at, s::pxy(sk, e.str("s"))) < dist(at, s::pxy(sk, e.str("e"))) {
            "s"
        } else {
            "e"
        }
    };
    let mut best: Option<(f64, P2, String)> = None;
    for o in s::entities(sk) {
        let oe = Ent::E(o);
        if oe.id() == id || oe.ty() == "point" {
            continue;
        }
        let Ok(Some(co)) = si::curve_of(sk, &oe) else { continue };
        for x in si::intersect(&cv, &co, true, false) {
            let gap = match &cv {
                Curve::Line { a, b, .. } => {
                    let l = dist(*a, *b);
                    if key == "p2" { (x.ta - 1.0) * l } else { -x.ta * l }
                }
                Curve::Arc { r, sweep, .. } => {
                    if key == "e" {
                        (x.ta - sweep) * r
                    } else {
                        (2.0 * std::f64::consts::PI - x.ta) * r
                    }
                }
                _ => f64::NAN,
            };
            if let Curve::Arc { sweep, .. } = &cv
                && x.ta <= sweep + 1e-12
            {
                continue;
            }
            if !(gap > tol) {
                continue;
            }
            if best.as_ref().is_none_or(|b| gap < b.0) {
                best = Some((gap, x.p, oe.id().to_string()));
            }
        }
    }
    let Some((_, p, by)) = best else { return Err(t("model.sketch_tools.extend_nothing", &[])) };
    let pid = own_point(sk, id, key);
    drop_on_point(sk, &pid);
    filter_curve(sk, id, if ety == "line" { line_keeps } else { round_keeps });
    set_pt(sk, &pid, p);
    tie_to(sk, &pid, Some(&by));
    Ok(Made { removed: None, made: vec![id.to_string()], dropped: dropped_since(sk, &before) })
}

// ---------------------------------------------------------------- chains and offsets

fn end_xy(sk: &Value, e: &Ent) -> Option<Vec<P2>> {
    let ends = s::ends_of(e);
    if ends.is_empty() { None } else { Some(ends.iter().map(|p| s::pxy(sk, p)).collect()) }
}

/// A chain of curves: (id, forward) items and whether it closes.
#[derive(Clone, Debug, Default)]
pub struct Chain {
    pub items: Vec<(String, bool)>,
    pub closed: bool,
}
impl Chain {
    pub fn to_json(&self) -> Value {
        jv::obj(vec![
            (
                "items",
                Value::Array(self.items.iter().map(|(id, f)| jv::obj(vec![("id", id.as_str().into()), ("fwd", (*f).into())])).collect()),
            ),
            ("closed", self.closed.into()),
        ])
    }
    pub fn from_json(v: &Value) -> Chain {
        Chain {
            items: jv::arr(v.get("items")).iter().map(|i| (js::str_of(i.get("id")), jv::truthy(i.get("fwd")))).collect(),
            closed: jv::truthy(v.get("closed")),
        }
    }
}

/// `chainOf(sk, id, ids)`.
pub fn chain_of(sk: &Value, id: &str, ids: Option<&[String]>) -> Result<Chain, String> {
    let Some(e0) = s::by_id(sk, id).filter(|e| e.ty() != "point") else { return Err(t("model.sketch_tools.offset_pick", &[])) };
    if end_xy(sk, &e0).is_none() {
        return Ok(Chain { items: vec![(id.to_string(), true)], closed: true });
    }
    let tol = 1e3 * tol_of(sk);
    let cons0 = e0.construction();
    let pool: Vec<&Value> = s::entities(sk)
        .iter()
        .filter(|e| {
            let en = Ent::E(e);
            en.ty() != "point"
                && end_xy(sk, &en).is_some()
                && en.construction() == cons0
                && ids.is_none_or(|l| l.iter().any(|x| x == en.id()))
        })
        .collect();
    let at_node = |p: P2, except: &str| -> Vec<&Value> {
        pool.iter()
            .filter(|e| {
                let en = Ent::E(e);
                en.id() != except && end_xy(sk, &en).is_some_and(|ends| ends.iter().any(|q| dist(p, *q) <= tol))
            })
            .copied()
            .collect()
    };
    let mut items: std::collections::VecDeque<(String, bool)> = std::collections::VecDeque::new();
    items.push_back((id.to_string(), true));
    let mut used: HashSet<String> = HashSet::new();
    used.insert(id.to_string());
    let mut walk = |forward: bool, items: &mut std::collections::VecDeque<(String, bool)>| -> bool {
        let mut cur: Ent = e0;
        let mut fwd = true;
        loop {
            let ends = end_xy(sk, &cur).unwrap_or_default();
            let (a, b) = (ends.first().copied().unwrap_or([f64::NAN; 2]), ends.get(1).copied().unwrap_or([f64::NAN; 2]));
            let p = if forward == fwd { b } else { a };
            let all = at_node(p, cur.id());
            if all.len() != 1 {
                return false;
            }
            let Some(nx) = all.first().map(|v| Ent::E(v)) else { return false };
            if used.contains(nx.id()) {
                return nx.id() == id;
            }
            let na = end_xy(sk, &nx).and_then(|v| v.first().copied()).unwrap_or([f64::NAN; 2]);
            let nf = if forward { dist(na, p) <= tol } else { dist(na, p) > tol };
            used.insert(nx.id().to_string());
            if forward {
                items.push_back((nx.id().to_string(), nf));
            } else {
                items.push_front((nx.id().to_string(), nf));
            }
            cur = nx;
            fwd = nf;
        }
    };
    let closed = walk(true, &mut items);
    if !closed {
        walk(false, &mut items);
    }
    Ok(Chain { items: items.into_iter().collect(), closed })
}

/// `offsetSide(sk, chain, p)` → (distance, side).
pub fn offset_side(sk: &Value, chain: &Chain, p: P2) -> Result<Option<(f64, f64)>, String> {
    let mut best: Option<(f64, f64)> = None;
    for (id, fwd) in &chain.items {
        let e = s::by_id(sk, id).ok_or_else(|| "Cannot read properties of null (reading 'type')".to_string())?;
        let Some(cv) = si::curve_of(sk, &e)? else { continue };
        let r = si::range_of(&cv);
        let mut tt = si::param_of(&cv, p);
        if !r.2 {
            tt = match cv {
                Curve::Arc { sweep, .. } => {
                    if tt > sweep {
                        if tt - sweep < 2.0 * std::f64::consts::PI - tt { sweep } else { 0.0 }
                    } else {
                        tt
                    }
                }
                _ => js::max(r.0, js::min(r.1, tt)),
            };
        }
        let q = si::point_on(&cv, tt);
        let d = dist(p, q);
        if best.is_none_or(|b| d < b.0) {
            let mut tg = si::tangent_on(&cv, tt);
            if !fwd {
                tg = [-tg[0], -tg[1]];
            }
            let side = if tg[0] * (p[1] - q[1]) - tg[1] * (p[0] - q[0]) >= 0.0 { 1.0 } else { -1.0 };
            best = Some((d, side));
        }
    }
    Ok(best)
}

#[derive(Clone, Debug)]
enum Kind {
    Line { dir: P2 },
    Circle,
    Arc { ccw: bool, sweep0: f64 },
    CSpline,
    Spline { a0: Option<f64>, a1: Option<f64> },
}
#[derive(Clone, Debug)]
struct El {
    eid: String,
    construction: bool,
    kind: Kind,
    s: P2,
    e: P2,
    c: P2,
    r: f64,
    q: Vec<P2>,
    err: f64,
    id: String,
}

/// What `offset` made.
#[derive(Clone, Debug, Default)]
pub struct OffsetResult {
    pub made: Vec<String>,
    pub master: Option<String>,
    pub parametric: bool,
    pub err: f64,
}
impl OffsetResult {
    pub fn to_json(&self) -> Value {
        jv::obj(vec![
            ("made", jv::strs(&self.made)),
            ("master", self.master.as_deref().map_or(Value::Null, Into::into)),
            ("parametric", self.parametric.into()),
            ("err", jv::num(self.err)),
        ])
    }
}

/// `offset(sk, chain, d, { link })`.
pub fn offset(sk: &mut Value, chain: &Chain, d: f64, link: Option<&str>) -> Result<OffsetResult, String> {
    if !(d.abs() > 0.0) {
        return Err(t("model.sketch_tools.offset_zero", &[]));
    }
    let tol = 1e3 * tol_of(sk);
    let mut els: Vec<El> = Vec::new();
    for (id, fwd) in &chain.items {
        let ev = s::by_id(sk, id).map(|e| e.value()).ok_or_else(|| "Cannot read properties of null (reading 'type')".to_string())?;
        let e = Ent::E(&ev);
        let construction = e.construction();
        let base = |kind: Kind| El {
            eid: id.clone(),
            construction,
            kind,
            s: [f64::NAN; 2],
            e: [f64::NAN; 2],
            c: [f64::NAN; 2],
            r: f64::NAN,
            q: Vec::new(),
            err: 0.0,
            id: String::new(),
        };
        match e.ty() {
            "line" => {
                let (mut a, mut b) = (s::pxy(sk, e.str("p1")), s::pxy(sk, e.str("p2")));
                if !fwd {
                    std::mem::swap(&mut a, &mut b);
                }
                let l = dist(a, b);
                let n = [-(b[1] - a[1]) / l, (b[0] - a[0]) / l];
                let mut el = base(Kind::Line { dir: [b[0] - a[0], b[1] - a[1]] });
                el.s = [a[0] + d * n[0], a[1] + d * n[1]];
                el.e = [b[0] + d * n[0], b[1] + d * n[1]];
                els.push(el);
            }
            "circle" => {
                let r = e.num("r") - d;
                if !(r > tol) {
                    return Err(t("model.sketch_tools.offset_circle", &[]));
                }
                let mut el = base(Kind::Circle);
                el.c = s::pxy(sk, e.str("c"));
                el.r = r;
                els.push(el);
            }
            "arc" => {
                let (c, r0, a0, sweep) = s::arc_angles(sk, &e);
                let r = if *fwd { r0 - d } else { r0 + d };
                if !(r > tol) {
                    return Err(t("model.sketch_tools.offset_arc", &[]));
                }
                let at = |ang: f64| [c[0] + r * js::cos(ang), c[1] + r * js::sin(ang)];
                let mut el = base(Kind::Arc { ccw: *fwd, sweep0: sweep });
                el.c = c;
                el.r = r;
                el.s = at(if *fwd { a0 } else { a0 + sweep });
                el.e = at(if *fwd { a0 + sweep } else { a0 });
                els.push(el);
            }
            _ => {
                let mut bs = s::spline_curve(sk, &e)?;
                if jv::truthy(e.val("closed")) {
                    let f = sp::offset_fit(&bs, d, OFFSET_TOL)?;
                    let mut el = base(Kind::CSpline);
                    el.q = f.q;
                    el.err = f.err;
                    els.push(el);
                } else {
                    if !fwd {
                        bs = sp::reverse(&bs);
                    }
                    let f = sp::offset_fit(&bs, d, OFFSET_TOL)?;
                    let mut el = base(Kind::Spline { a0: f.a0, a1: f.a1 });
                    el.s = f.q.first().copied().unwrap_or([f64::NAN; 2]);
                    el.e = f.q.last().copied().unwrap_or([f64::NAN; 2]);
                    el.q = f.q;
                    el.err = f.err;
                    els.push(el);
                }
            }
        }
    }
    let has_spline = els.iter().any(|x| matches!(x.kind, Kind::Spline { .. } | Kind::CSpline));
    let n = els.len();
    let first_closed = els.first().is_some_and(|x| matches!(x.kind, Kind::Circle | Kind::CSpline));
    let open = !chain.closed || first_closed;
    let full = |x: &El| -> Curve {
        match x.kind {
            Kind::Line { .. } => Curve::Line { id: String::new(), a: x.s, b: x.e },
            _ => Curve::Circle { id: String::new(), c: x.c, r: x.r },
        }
    };
    if !first_closed {
        let m = if open { n.saturating_sub(1) } else { n };
        for i in 0..m {
            let j = js::rem(i.saturating_add(1), n);
            let (Some(aa), Some(bb)) = (els.get(i).cloned(), els.get(j).cloned()) else { continue };
            if dist(aa.e, bb.s) <= tol {
                if let Some(b) = els.get_mut(j) {
                    b.s = aa.e;
                }
                continue;
            }
            if matches!(aa.kind, Kind::Spline { .. }) || matches!(bb.kind, Kind::Spline { .. }) {
                return Err(t("model.sketch_tools.offset_spline_joins", &[]));
            }
            let mid = [(aa.e[0] + bb.s[0]) / 2.0, (aa.e[1] + bb.s[1]) / 2.0];
            let xs = si::intersect(&full(&aa), &full(&bb), true, true);
            let Some(first) = xs.first() else {
                return Err(t(
                    "model.sketch_tools.offset_corner",
                    &[("x", F::N(js::fixed_num(mid[0], 3))), ("y", F::N(js::fixed_num(mid[1], 3)))],
                ));
            };
            let mut best = *first;
            for x in xs.iter().skip(1) {
                if dist(x.p, mid) < dist(best.p, mid) {
                    best = *x;
                }
            }
            if let Some(a) = els.get_mut(i) {
                a.e = best.p;
            }
            if let Some(b) = els.get_mut(j) {
                b.s = best.p;
            }
        }
    }
    for x in &els {
        if let Kind::Line { dir } = x.kind
            && (x.e[0] - x.s[0]) * dir[0] + (x.e[1] - x.s[1]) * dir[1] <= 0.0
        {
            return Err(t("model.sketch_tools.offset_side_inside_out", &[]));
        }
        if let Kind::Arc { ccw, sweep0 } = x.kind {
            let ang = |p: P2| js::atan2(p[1] - x.c[1], p[0] - x.c[0]);
            let sw = if ccw { si::norm_ang(ang(x.e) - ang(x.s)) } else { si::norm_ang(ang(x.s) - ang(x.e)) };
            if (sw - sweep0).abs() > std::f64::consts::PI || sw < 1e-9 {
                return Err(t("model.sketch_tools.offset_arc_inside_out", &[]));
            }
        }
    }
    let mut made = Vec::new();
    let mut ends: Vec<Option<(String, String)>> = Vec::new();
    for x in &mut els {
        let cons = x.construction;
        match &x.kind {
            Kind::Line { .. } => {
                let r = s::add_line(sk, &At::Xy(x.s), &At::Xy(x.e), cons)?;
                x.id = jv::s(&r, "id").to_string();
                ends.push(Some((jv::s(&r, "p1").to_string(), jv::s(&r, "p2").to_string())));
            }
            Kind::Circle => {
                let r = s::add_circle(sk, &At::Xy(x.c), x.r, cons)?;
                x.id = jv::s(&r, "id").to_string();
                ends.push(None);
            }
            Kind::Arc { ccw, .. } => {
                let r = if *ccw {
                    s::add_arc(sk, &At::Xy(x.c), &At::Xy(x.s), &At::Xy(x.e), cons)?
                } else {
                    s::add_arc(sk, &At::Xy(x.c), &At::Xy(x.e), &At::Xy(x.s), cons)?
                };
                x.id = jv::s(&r, "id").to_string();
                let (rs, re) = (jv::s(&r, "s").to_string(), jv::s(&r, "e").to_string());
                ends.push(Some(if *ccw { (rs, re) } else { (re, rs) }));
            }
            Kind::CSpline => {
                let mut o = jv::obj(vec![("kind", "fit".into()), ("closed", true.into())]);
                if cons {
                    jv::set(&mut o, "construction", true.into());
                }
                let pts: Vec<At> = x.q.iter().map(|p| At::Xy(*p)).collect();
                let r = s::add_spline(sk, &pts, &o)?;
                x.id = jv::s(&r, "id").to_string();
                ends.push(None);
            }
            Kind::Spline { a0, a1 } => {
                if let Some(q0) = x.q.first_mut() {
                    *q0 = x.s;
                }
                if let Some(ql) = x.q.last_mut() {
                    *ql = x.e;
                }
                let mut o = jv::obj(vec![("kind", "fit".into())]);
                jv::set(&mut o, "a0", a0.map_or(Value::Null, jv::num));
                jv::set(&mut o, "a1", a1.map_or(Value::Null, jv::num));
                if cons {
                    jv::set(&mut o, "construction", true.into());
                }
                let pts: Vec<At> = x.q.iter().map(|p| At::Xy(*p)).collect();
                let r = s::add_spline(sk, &pts, &o)?;
                x.id = jv::s(&r, "id").to_string();
                let rp: Vec<String> = jv::arr(r.get("pts")).iter().map(|v| js::str_of(Some(v))).collect();
                ends.push(Some((rp.first().cloned().unwrap_or_default(), rp.last().cloned().unwrap_or_default())));
            }
        }
        made.push(x.id.clone());
    }
    if ends.first().is_some_and(Option::is_some) {
        let m = if open { n.saturating_sub(1) } else { n };
        for i in 0..m {
            let a = ends.get(i).cloned().flatten().map(|e| e.1).unwrap_or_default();
            let b = ends.get(js::rem(i.saturating_add(1), n)).cloned().flatten().map(|e| e.0).unwrap_or_default();
            s::add_constraint(sk, "coincident", &[a, b], None, None)?;
        }
    }
    let mut master: Option<String> = None;
    if !has_spline {
        for (i, x) in els.iter().enumerate() {
            if i == 0 && link.is_none() {
                let m = s::add_constraint(sk, "offset", &[x.eid.clone(), x.id.clone()], None, None)?;
                let mid = jv::s(&m, "id").to_string();
                if let Some(c) = s::constraints_mut(sk).and_then(|v| v.iter_mut().find(|c| jv::is_str(c.get("id"), &mid))) {
                    jv::set(c, "value", jv::num(d.abs()));
                }
                master = Some(mid);
            } else {
                let of = link.map(str::to_string).or_else(|| master.clone());
                s::add_constraint(sk, "offsetlink", &[x.eid.clone(), x.id.clone()], None, of.as_deref())?;
            }
        }
    }
    Ok(OffsetResult {
        made,
        master: master.or_else(|| link.map(str::to_string)),
        parametric: !has_spline,
        err: js::max_of(std::iter::once(0.0).chain(els.iter().map(|x| if x.err == 0.0 || x.err.is_nan() { 0.0 } else { x.err }))),
    })
}

// ---------------------------------------------------------------- copies

fn copy_entities(
    sk: &mut Value,
    ids: &[String],
    f: &dyn Fn(P2) -> P2,
    flip: bool,
    ang: &dyn Fn(f64) -> f64,
    same: &dyn Fn(&Value, &str) -> bool,
) -> (OMap<String>, Vec<String>) {
    let mut take: HashSet<String> = HashSet::new();
    for id in ids {
        let Some(e) = s::by_id(sk, id) else { continue };
        if matches!(e, Ent::Origin) {
            continue;
        }
        take.insert(id.clone());
        if e.ty() != "point" {
            take.extend(s::points_of(&e));
        }
    }
    let mut map: OMap<String> = OMap::new();
    map.set(ORIGIN, ORIGIN.to_string());
    let mut made = Vec::new();
    let src: Vec<Value> = s::entities(sk).iter().filter(|e| take.contains(jv::s(e, "id"))).cloned().collect();
    for e in src.iter().filter(|x| jv::s(x, "type") == "point") {
        let id = jv::s(e, "id").to_string();
        if same(sk, &id) {
            map.set(&id, id.clone());
            continue;
        }
        let q = f([jv::n(e, "x"), jv::n(e, "y")]);
        let nid = s::add_point(sk, q[0], q[1], jv::truthy(e.get("construction")));
        map.set(&id, nid.clone());
        made.push(nid);
    }
    for e in src.iter().filter(|x| jv::s(x, "type") != "point") {
        let mut n = e.clone();
        let prefix = match jv::s(e, "type") {
            "line" => "l",
            "circle" => "c",
            "arc" => "a",
            "spline" => "s",
            _ => "undefined",
        };
        let nid = s::next_id(sk, prefix);
        jv::set(&mut n, "id", nid.as_str().into());
        for k in ["p1", "p2", "c", "s", "e"] {
            if let Some(old) = n.get(k).and_then(Value::as_str).map(str::to_string) {
                match map.get(&old) {
                    Some(m) => jv::set(&mut n, k, m.as_str().into()),
                    None => jv::del(&mut n, k),
                }
            }
        }
        if let Some(pts) = jv::as_arr(n.get("pts")).cloned() {
            let np: Vec<Value> = pts.iter().map(|p| map.get(&js::str_of(Some(p))).map_or(Value::Null, |m| m.as_str().into())).collect();
            jv::set(&mut n, "pts", Value::Array(np));
        }
        if flip && jv::s(&n, "type") == "arc" {
            let (sv, ev) = (n.get("s").cloned(), n.get("e").cloned());
            match ev {
                Some(v) => jv::set(&mut n, "s", v),
                None => jv::del(&mut n, "s"),
            }
            match sv {
                Some(v) => jv::set(&mut n, "e", v),
                None => jv::del(&mut n, "e"),
            }
        }
        for k in ["a0", "a1"] {
            if let Some(a) = jv::finite(n.get(k)) {
                jv::set(&mut n, k, jv::num(ang(a)));
            }
        }
        push_entity(sk, n);
        map.set(jv::s(e, "id"), nid.clone());
        made.push(nid);
    }
    (map, made)
}

fn copy_relations(sk: &mut Value, map: &mut OMap<String>, keep: fn(&Value) -> bool, f: Option<&dyn Fn(P2) -> P2>) {
    let src: Vec<Value> = s::constraints(sk).to_vec();
    for c in &src {
        let refs = refs_of(c);
        if !refs.iter().all(|r| map.has(r)) || refs.iter().all(|r| map.get(r).is_some_and(|m| m == r)) || !keep(c) {
            continue;
        }
        let mut n = c.clone();
        let nid = s::next_id(sk, "k");
        jv::set(&mut n, "id", nid.as_str().into());
        jv::set(&mut n, "refs", jv::strs(&refs.iter().map(|r| map.get(r).cloned().unwrap_or_default()).collect::<Vec<_>>()));
        if jv::s(c, "type") == "offsetlink" {
            let key = format!("k:{}", js::string_of(c.get("of")));
            let Some(m) = map.get(&key).cloned() else { continue };
            jv::set(&mut n, "of", m.into());
        }
        if jv::s(&n, "type") == "fix"
            && let Some(f) = f
        {
            let r0 = jv::arr(n.get("refs")).first().map(|v| js::str_of(Some(v))).unwrap_or_default();
            let is_circle = s::by_id(sk, &r0).is_some_and(|e| e.ty() == "circle");
            let mut vals: Vec<f64> = jv::arr(n.get("value")).iter().map(|v| jv::as_num(Some(v)).unwrap_or(f64::NAN)).collect();
            let pairs = if is_circle { 1.0 } else { js::f(vals.len()) / 2.0 };
            let mut i = 0usize;
            while js::f(i) < pairs {
                let (a, b) = (i.saturating_mul(2), i.saturating_mul(2).saturating_add(1));
                let q = f([vals.get(a).copied().unwrap_or(f64::NAN), vals.get(b).copied().unwrap_or(f64::NAN)]);
                while vals.len() <= b {
                    vals.push(f64::NAN);
                }
                if let Some(x) = vals.get_mut(a) {
                    *x = q[0];
                }
                if let Some(x) = vals.get_mut(b) {
                    *x = q[1];
                }
                i = i.saturating_add(1);
            }
            jv::set(&mut n, "value", jv::nums(&vals));
        }
        jv::del(&mut n, "at");
        if let Some(v) = s::constraints_mut(sk) {
            v.push(n);
        }
        map.set(&format!("k:{}", js::str_of(c.get("id"))), nid);
    }
}

/// `mirror(sk, ids, axis)` → (made, relations).
pub fn mirror(sk: &mut Value, ids: &[String], axis: &str) -> Result<(Vec<String>, usize), String> {
    let Some(l) = s::by_id(sk, axis).filter(|l| l.ty() == "line").map(|l| (l.str("p1").to_string(), l.str("p2").to_string())) else {
        return Err(t("model.sketch_tools.mirror_line", &[]));
    };
    let (a, b) = (s::pxy(sk, &l.0), s::pxy(sk, &l.1));
    let len = dist(a, b);
    if !(len > 0.0) {
        return Err(t("model.sketch_tools.mirror_no_length", &[]));
    }
    let u = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
    let refl = move |p: P2| {
        let tt = (p[0] - a[0]) * u[0] + (p[1] - a[1]) * u[1];
        let q = [a[0] + tt * u[0], a[1] + tt * u[1]];
        [2.0 * q[0] - p[0], 2.0 * q[1] - p[1]]
    };
    let phi = js::atan2(u[1], u[0]);
    let tol = 1e3 * tol_of(sk);
    let on_axis = move |sk: &Value, pid: &str| {
        let p = s::pxy(sk, pid);
        ((p[0] - a[0]) * u[1] - (p[1] - a[1]) * u[0]).abs() <= tol
    };
    let sel: Vec<String> = ids.iter().filter(|id| **id != axis && **id != l.0 && **id != l.1 && *id != ORIGIN).cloned().collect();
    if sel.is_empty() {
        return Err(t("model.sketch_tools.mirror_select", &[]));
    }
    let (map, made) = copy_entities(sk, &sel, &refl, true, &|tt| 2.0 * phi - tt, &on_axis);
    let mut relations = 0usize;
    for (o, n) in map.iter() {
        let Some(e) = s::by_id(sk, o).map(|e| e.ty().to_string()) else { continue };
        if o == ORIGIN {
            continue;
        }
        if e == "point" {
            if o == n {
                let has = s::constraints(sk)
                    .iter()
                    .any(|c| jv::s(c, "type") == "coincident" && refs_of(c).iter().any(|r| r == o) && refs_of(c).iter().any(|r| r == axis));
                if !has && s::add_constraint(sk, "coincident", &[o.to_string(), axis.to_string()], None, None).is_ok() {
                    relations = relations.saturating_add(1);
                }
            } else {
                s::add_constraint(sk, "symmetric", &[o.to_string(), n.clone(), axis.to_string()], None, None)?;
                relations = relations.saturating_add(1);
            }
        } else if e == "circle" {
            s::add_constraint(sk, "equal", &[o.to_string(), n.clone()], None, None)?;
            relations = relations.saturating_add(1);
        }
    }
    Ok((made, relations))
}

fn translation_keeps(c: &Value) -> bool {
    jv::s(c, "type") != "fix"
}
fn rotation_keeps(c: &Value) -> bool {
    !matches!(jv::s(c, "type"), "fix" | "horizontal" | "vertical" | "hdistance" | "vdistance")
}

/// `patternLinear(sk, ids, [dx, dy], count)`.
pub fn pattern_linear(sk: &mut Value, ids: &[String], dxy: P2, count: f64) -> Result<Vec<String>, String> {
    if !(count >= 2.0) || count > 200.0 || !js::is_integer(count) {
        return Err(t("model.sketch_tools.pattern_count", &[]));
    }
    let mut made = Vec::new();
    let n = js::count(count, 200);
    for i in 1..n {
        let fi = js::f(i);
        let f = move |p: P2| [p[0] + dxy[0] * fi, p[1] + dxy[1] * fi];
        let (mut map, m) = copy_entities(sk, ids, &f, false, &|a| a, &|_, _| false);
        copy_relations(sk, &mut map, translation_keeps, Some(&f));
        made.extend(m);
    }
    Ok(made)
}

/// `patternCircular(sk, ids, c, count, angle)`.
pub fn pattern_circular(sk: &mut Value, ids: &[String], c: P2, count: f64, angle: f64) -> Result<Vec<String>, String> {
    if !(count >= 2.0) || count > 200.0 || !js::is_integer(count) {
        return Err(t("model.sketch_tools.pattern_count", &[]));
    }
    if !(angle > 0.0 && angle <= 360.0) {
        return Err(t("model.sketch_tools.pattern_angle", &[]));
    }
    let step = (if angle >= 360.0 { 360.0 / count } else { angle / (count - 1.0) }) * std::f64::consts::PI / 180.0;
    let mut made = Vec::new();
    let n = js::count(count, 200);
    for i in 1..n {
        let th = step * js::f(i);
        let (co, si_) = (js::cos(th), js::sin(th));
        let f = move |p: P2| [c[0] + co * (p[0] - c[0]) - si_ * (p[1] - c[1]), c[1] + si_ * (p[0] - c[0]) + co * (p[1] - c[1])];
        let (mut map, m) = copy_entities(sk, ids, &f, false, &|tt| tt + th, &|_, _| false);
        copy_relations(sk, &mut map, rotation_keeps, Some(&f));
        made.extend(m);
    }
    Ok(made)
}

/// `addPolygon(sk, c, v, n)` → { circle, lines, made }.
pub fn add_polygon(sk: &mut Value, c: P2, v: P2, n: f64) -> Result<Value, String> {
    if !js::is_integer(n) || !(3.0..=64.0).contains(&n) {
        return Err(t("model.sketch_tools.polygon_sides", &[]));
    }
    let r = dist(c, v);
    if !(r > 0.0) {
        return Err(t("model.sketch_tools.polygon_size", &[]));
    }
    let a0 = js::atan2(v[1] - c[1], v[0] - c[0]);
    let circ = s::add_circle(sk, &At::Xy(c), r, true)?;
    let cid = jv::s(&circ, "id").to_string();
    let k = js::count(n, 64);
    let corners: Vec<P2> = (0..k)
        .map(|i| {
            let a = a0 + 2.0 * std::f64::consts::PI * js::f(i) / n;
            [c[0] + r * js::cos(a), c[1] + r * js::sin(a)]
        })
        .collect();
    let mut lines: Vec<Value> = Vec::new();
    for i in 0..k {
        let p = corners.get(i).copied().unwrap_or([0.0; 2]);
        let q = corners.get(js::rem(i.saturating_add(1), k)).copied().unwrap_or([0.0; 2]);
        lines.push(s::add_line(sk, &At::Xy(p), &At::Xy(q), false)?);
    }
    let g = |i: usize, key: &str| lines.get(i).map(|l| jv::s(l, key).to_string()).unwrap_or_default();
    for i in 0..k {
        s::add_constraint(sk, "coincident", &[g(i, "p2"), g(js::rem(i.saturating_add(1), k), "p1")], None, None)?;
        s::add_constraint(sk, "coincident", &[g(i, "p1"), cid.clone()], None, None)?;
        if i > 0 {
            s::add_constraint(sk, "equal", &[g(0, "id"), g(i, "id")], None, None)?;
        }
    }
    let lids: Vec<String> = (0..k).map(|i| g(i, "id")).collect();
    let mut made = vec![cid.clone()];
    made.extend(lids.iter().cloned());
    Ok(jv::obj(vec![("circle", cid.into()), ("lines", jv::strs(&lids)), ("made", jv::strs(&made))]))
}

/// `addSlot(sk, a, b, w)` → { made, centres, arcs }.
pub fn add_slot(sk: &mut Value, a: P2, b: P2, w: f64) -> Result<Value, String> {
    let l = dist(a, b);
    let r = w / 2.0;
    if !(l > 0.0) {
        return Err(t("model.sketch_tools.slot_centres", &[]));
    }
    if !(r > 0.0) {
        return Err(t("model.sketch_tools.slot_width", &[]));
    }
    let u = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
    let n = [-u[1], u[0]];
    let pp = |c: P2, sg: f64| [c[0] + n[0] * r * sg, c[1] + n[1] * r * sg];
    let ca = s::add_point(sk, a[0], a[1], false);
    let cb = s::add_point(sk, b[0], b[1], false);
    let q1 = pp(a, 1.0);
    let p1 = s::add_point(sk, q1[0], q1[1], false);
    let q2 = pp(b, 1.0);
    let p2 = s::add_point(sk, q2[0], q2[1], false);
    let q3 = pp(b, -1.0);
    let p3 = s::add_point(sk, q3[0], q3[1], false);
    let q4 = pp(a, -1.0);
    let p4 = s::add_point(sk, q4[0], q4[1], false);
    let arc_b = s::next_id(sk, "a");
    let arc_a = s::next_id(sk, "a");
    let top = s::next_id(sk, "l");
    let bot = s::next_id(sk, "l");
    let st = |x: &str| Value::String(x.to_string());
    push_entity(sk, jv::obj(vec![("id", st(&arc_b)), ("type", "arc".into()), ("c", st(&cb)), ("s", st(&p3)), ("e", st(&p2))]));
    push_entity(sk, jv::obj(vec![("id", st(&arc_a)), ("type", "arc".into()), ("c", st(&ca)), ("s", st(&p1)), ("e", st(&p4))]));
    push_entity(sk, jv::obj(vec![("id", st(&top)), ("type", "line".into()), ("p1", st(&p2)), ("p2", st(&p1))]));
    push_entity(sk, jv::obj(vec![("id", st(&bot)), ("type", "line".into()), ("p1", st(&p4)), ("p2", st(&p3))]));
    for (ln, ar) in [(&top, &arc_a), (&top, &arc_b), (&bot, &arc_a), (&bot, &arc_b)] {
        s::add_constraint(sk, "tangent", &[ln.clone(), ar.clone()], None, None)?;
    }
    s::add_constraint(sk, "equal", &[arc_a.clone(), arc_b.clone()], None, None)?;
    Ok(jv::obj(vec![
        ("made", jv::strs(&[arc_a.clone(), arc_b.clone(), top, bot])),
        ("centres", jv::strs(&[ca, cb])),
        ("arcs", jv::strs(&[arc_a, arc_b])),
    ]))
}

/// `projectEdge(sk, frame, edge)` → { made, dropped, kind }.
pub fn project_edge(sk: &mut Value, frame: &Value, edge: Option<&Value>) -> Result<Value, String> {
    let Some(edge) = edge.filter(|e| jv::truthy(Some(e))) else { return Err(t("model.sketch_tools.project_pick", &[])) };
    let v3 = |v: Option<&Value>| -> [f64; 3] {
        let a = jv::arr(v);
        [jv::as_num(a.first()).unwrap_or(f64::NAN), jv::as_num(a.get(1)).unwrap_or(f64::NAN), jv::as_num(a.get(2)).unwrap_or(f64::NAN)]
    };
    let dot = |u: [f64; 3], v: [f64; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
    let (fo, fx, fy, fnn) = (v3(frame.get("o")), v3(frame.get("x")), v3(frame.get("y")), v3(frame.get("n")));
    let loc = |p: [f64; 3]| {
        let d = [p[0] - fo[0], p[1] - fo[1], p[2] - fo[2]];
        [dot(d, fx), dot(d, fy)]
    };
    let tol = 1e3 * tol_of(sk);
    let done = |id: String, kind: &str| jv::obj(vec![("made", jv::strs(&[id])), ("dropped", 0.into()), ("kind", kind.into())]);
    if jv::is_str(edge.get("type"), "line") {
        let (pa, pb) = (loc(v3(edge.get("a"))), loc(v3(edge.get("b"))));
        if dist(pa, pb) <= tol {
            return Err(t("model.sketch_tools.project_point", &[]));
        }
        let l = s::add_line(sk, &At::Xy(pa), &At::Xy(pb), false)?;
        let id = jv::s(&l, "id").to_string();
        s::add_constraint(sk, "fix", std::slice::from_ref(&id), None, None)?;
        return Ok(done(id, "line"));
    }
    if jv::is_str(edge.get("type"), "circle")
        && let Some(circ) = edge.get("circ").filter(|c| jv::truthy(Some(c)))
    {
        let (c, n, r) = (v3(circ.get("c")), v3(circ.get("n")), jv::n(circ, "r"));
        let cn = dot(n, fnn);
        if (cn.abs() - 1.0).abs() > 1e-9 {
            return Err(t("model.sketch_tools.project_ellipse", &[]));
        }
        let cc = loc(c);
        let (pa, pb) = (loc(v3(edge.get("a"))), loc(v3(edge.get("b"))));
        if dist(pa, pb) <= tol {
            let k = s::add_circle(sk, &At::Xy(cc), r, false)?;
            let id = jv::s(&k, "id").to_string();
            s::add_constraint(sk, "fix", std::slice::from_ref(&id), None, None)?;
            return Ok(done(id, "circle"));
        }
        let a = if cn > 0.0 {
            s::add_arc(sk, &At::Xy(cc), &At::Xy(pa), &At::Xy(pb), false)?
        } else {
            s::add_arc(sk, &At::Xy(cc), &At::Xy(pb), &At::Xy(pa), false)?
        };
        let id = jv::s(&a, "id").to_string();
        s::add_constraint(sk, "fix", std::slice::from_ref(&id), None, None)?;
        return Ok(done(id, "arc"));
    }
    Err(t("model.sketch_tools.project_freeform", &[]))
}
