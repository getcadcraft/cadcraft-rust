//! geom.js, part 5: corners — FILLET and CHAMFER on two objects, on two segments of one polyline, or on every
//! corner of a polyline — and EXPLODE.
//!
//! Results are `{ ok: true, edits: [entity with id], add: [new entities] }` or `{ ok: false, error }` where error is
//! 'type' 'same' 'parallel' 'collinear' 'large' 'none' 'middle' 'angle' ([`Corner`]).

use crate::jsmath::JsMath;
use cc_cad::entity::{Color, Entity, Geo, Poly, Vertex};
use cc_cad::js;
use cc_geom::P2;
use serde_json::Value;

use crate::geom::*;
use crate::tx::tx_entity;

fn unit(dx: f64, dy: f64) -> Option<P2> {
    let l = js::hypot(dx, dy);
    if l > 0.0 { Some(p(dx / l, dy / l)) } else { None }
}
fn dot(a: P2, b: P2) -> f64 {
    a.x * b.x + a.y * b.y
}
fn clamp1(x: f64) -> f64 {
    js::max(-1.0, js::min(1.0, x))
}

/// What a pick on e is, for a corner: the object, the polyline segment nearest the pick, its primitive, the pick.
#[derive(Clone, Copy, Debug)]
pub struct Target<'a> {
    pub e: &'a Entity,
    pub seg: Option<usize>,
    pub prim: Prim,
    pub pick: P2,
}

pub fn corner_target(e: &Entity, pick: P2) -> Option<Target<'_>> {
    match &e.geo {
        Geo::Line(l) => (dist(p(l.x1, l.y1), p(l.x2, l.y2)) > EPS).then(|| Target {
            e,
            seg: None,
            prim: prims(e, None).first().copied().unwrap_or(Prim::l(l.x1, l.y1, l.x2, l.y2)),
            pick,
        }),
        Geo::Arc(a) if a.r > EPS => Some(Target { e, seg: None, prim: *prims(e, None).first()?, pick }),
        Geo::Circle(c) if c.r > EPS => Some(Target { e, seg: None, prim: *prims(e, None).first()?, pick }),
        Geo::Poly(_) => {
            let mut best: Option<Prim> = None;
            let mut bd = f64::INFINITY;
            for q in poly_prims_of(e) {
                if prim_length(&q) <= EPS {
                    continue;
                }
                let d = dist_to_prim(&q, pick);
                if d < bd {
                    bd = d;
                    best = Some(q);
                }
            }
            best.map(|q| Target { e, seg: q.seg, prim: q, pick })
        }
        _ => None,
    }
}

/// Which way from a cut point the kept part runs: a unit vector (straight pieces) or 'ccw' / 'cw' (arcs).
#[derive(Clone, Copy, Debug)]
pub enum Keep {
    Dir(P2),
    Ccw,
    Cw,
}

/// Move one end of a target's piece to tt, keeping the part that runs from tt in the direction `keep`.
/// Ok(None) = unchanged (`{ e }` of the object passed).
fn cut_target(t: &Target, tt: P2, keep: Keep, e: &Entity) -> Result<Option<Entity>, &'static str> {
    if matches!(e.geo, Geo::Circle(_)) {
        return Ok(None);
    }
    let q = t.prim;
    let start_end: bool; // true = 'start'
    if q.is_l() {
        let k = match keep {
            Keep::Dir(d) => d,
            _ => p(f64::NAN, f64::NAN),
        };
        let ds = (q.x1 - tt.x) * k.x + (q.y1 - tt.y) * k.y;
        let de = (q.x2 - tt.x) * k.x + (q.y2 - tt.y) * k.y;
        if js::max(ds, de) <= 1e-9 * js::max(1.0, prim_length(&q)) {
            return Err("large");
        }
        start_end = de >= ds;
    } else {
        let a = (tt.y - q.cy).jatan2(tt.x - q.cx);
        start_end = matches!(keep, Keep::Ccw);
        if on_sweep(a, q.a0, q.a1, 1e-9) {
            if (if start_end { norm_angle(q.a1 - a) } else { norm_angle(a - q.a0) }) < 1e-9 {
                return Err("large");
            }
        } else {
            let to_start = norm_angle(q.a0 - a);
            let to_end = norm_angle(a - q.a1);
            if if start_end { to_start > to_end } else { to_end > to_start } {
                return Err("large");
            }
        }
    }
    match &e.geo {
        Geo::Line(l) => Ok(Some(with_geo(e, if start_end { line_ent(tt.x, tt.y, l.x2, l.y2) } else { line_ent(l.x1, l.y1, tt.x, tt.y) }))),
        Geo::Arc(a) => {
            let ang = norm_angle((tt.y - a.cy).jatan2(tt.x - a.cx));
            Ok(Some(with_geo(e, if start_end { arc_ent(a.cx, a.cy, a.r, ang, a.a1) } else { arc_ent(a.cx, a.cy, a.r, a.a0, ang) })))
        }
        Geo::Poly(pl) => {
            let n = pl.pts.len();
            let i = t.seg.unwrap_or(usize::MAX);
            let segs = if pl.is_closed() { n } else { n.saturating_sub(1) };
            let at_start = if q.is_l() || !q.rev() { start_end } else { !start_end };
            if pl.is_closed() || (if at_start { i != 0 } else { i.checked_add(1) != Some(segs) }) {
                return Err("middle");
            }
            let mut pts = pl.pts.clone();
            let k = if at_start { i } else { i + 1 };
            if let Some(v) = pts.get_mut(k) {
                v.x = tt.x;
                v.y = tt.y;
            }
            if !q.is_l() {
                let a = (tt.y - q.cy).jatan2(tt.x - q.cx);
                let a0 = if start_end { a } else { q.a0 };
                let a1 = if !start_end { a } else { q.a1 };
                if let Some(v) = pts.get_mut(i) {
                    v.b = Some((norm_angle(a1 - a0) / 4.0).jtan() * if q.rev() { -1.0 } else { 1.0 });
                }
            }
            Ok(Some(with_geo(e, Geo::Poly(Poly { pts, closed: pl.closed }))))
        }
        _ => Err("type"),
    }
}

struct LlFillet {
    t: f64,
    a: P2,
    b: P2,
    c: P2,
}
/// Corner of two straight pieces at x, along the unit rays ua and ub: fillet points for radius r.
fn ll_fillet(x: P2, ua: P2, ub: P2, r: f64) -> Option<LlFillet> {
    let half = clamp1(dot(ua, ub)).jacos() / 2.0;
    if half < 1e-9 || half > PI / 2.0 - 1e-9 {
        return None;
    }
    let t = r / half.jtan();
    let bis = unit(ua.x + ub.x, ua.y + ub.y)?;
    Some(LlFillet {
        t,
        a: polar(x, ua.y.jatan2(ua.x), t),
        b: polar(x, ub.y.jatan2(ub.x), t),
        c: p(x.x + bis.x * r / half.jsin(), x.y + bis.y * r / half.jsin()),
    })
}

/// layer / colour a corner's new object takes.
#[derive(Clone, Debug, PartialEq)]
pub struct Like {
    pub layer: Option<String>,
    pub color: Color,
}
/// The current layer / colour (`opts.current`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Current {
    pub layer: Option<String>,
    pub color: Option<Color>,
}

/// The arc of circle c (radius r) between a and b, the short way round.
fn corner_arc(c: P2, r: f64, a: P2, b: P2, like: &Like) -> Entity {
    let mut a0 = (a.y - c.y).jatan2(a.x - c.x);
    let mut a1 = (b.y - c.y).jatan2(b.x - c.x);
    if norm_angle(a1 - a0) > PI {
        std::mem::swap(&mut a0, &mut a1);
    }
    Entity::with(arc_ent(c.x, c.y, r, norm_angle(a0), norm_angle(a1)), like.layer.clone(), Some(like.color.clone()))
}

/// The ray from corner x along a straight piece toward the part its pick keeps: (u, reach).
fn keep_ray(q: &Prim, x: P2, pick: P2) -> Option<(P2, f64)> {
    let mut ends = [p(q.x1, q.y1), p(q.x2, q.y2)];
    if dist(ends[1], x) - dist(ends[0], x) > 0.0 {
        ends.swap(0, 1);
    }
    let far = ends[0];
    let u0 = unit(far.x - x.x, far.y - x.y)?;
    if (pick.x - x.x) * u0.x + (pick.y - x.y) * u0.y >= 0.0 || dist(ends[1], x) <= EPS {
        return Some((u0, dist(far, x)));
    }
    Some((unit(ends[1].x - x.x, ends[1].y - x.y)?, dist(ends[1], x)))
}

fn line_dir(q: &Prim) -> Option<P2> {
    unit(q.x2 - q.x1, q.y2 - q.y1)
}

struct Shared {
    k: usize,
    in_seg: usize,
    out_seg: usize,
}
/// Two targets on one polyline: the vertex between adjacent segments, or None.
fn shared_vertex(a: &Target, b: &Target) -> Option<Shared> {
    let Geo::Poly(pl) = &a.e.geo else { return None };
    let n = pl.pts.len();
    let segs = if pl.is_closed() { n } else { n.saturating_sub(1) };
    let (i, j) = (a.seg?, b.seg?);
    if segs < 2 || n == 0 {
        return None;
    }
    if (i + 1) % n == j && (pl.is_closed() || i + 1 < n - 1) {
        return Some(Shared { k: j, in_seg: i, out_seg: j });
    }
    if (j + 1) % n == i && (pl.is_closed() || j + 1 < n - 1) {
        return Some(Shared { k: i, in_seg: j, out_seg: i });
    }
    None
}

/// Replace vertex k of a polyline by the corner points a (bulge toward the next) and b.
fn splice_corner(e: &Entity, k: usize, a: P2, bulge: f64, b: P2) -> Entity {
    let Geo::Poly(pl) = &e.geo else { return e.clone() };
    let mut pts = pl.pts.clone();
    let bk = pts.get(k).map_or(0.0, Vertex::bulge);
    if k < pts.len() {
        pts.splice(k..=k, [Vertex::new(a.x, a.y, bulge), Vertex::new(b.x, b.y, bk)]);
    }
    with_geo(e, Geo::Poly(Poly { pts: dedupe(pts, pl.is_closed()), closed: pl.closed }))
}

/// Drop vertices on top of the one before (zero-length segments a corner can leave).
fn dedupe(pts: Vec<Vertex>, closed: bool) -> Vec<Vertex> {
    let mut out: Vec<Vertex> = Vec::new();
    for v in pts {
        if let Some(last) = out.last_mut()
            && dist(last.p(), v.p()) <= 1e-9
        {
            last.b = Some(v.bulge());
            continue;
        }
        out.push(v);
    }
    if closed && out.len() > 1 && matches!((out.first(), out.last()), (Some(a), Some(b)) if dist(a.p(), b.p()) <= 1e-9) {
        out.pop();
    }
    out
}

struct VCorner {
    x: P2,
    ua: P2,
    ub: P2,
    la: f64,
    lb: f64,
    turn: f64,
}
/// The vertex corner of two adjacent straight segments of one polyline.
fn vertex_corner(v: &[Vertex], k: usize, in_seg: usize, out_seg: usize) -> Option<VCorner> {
    let n = v.len();
    if n == 0 {
        return None;
    }
    let (vk, pin, pout) = (v.get(k)?.p(), v.get(in_seg)?.p(), v.get((out_seg + 1) % n)?.p());
    let ua = unit(pin.x - vk.x, pin.y - vk.y)?;
    let ub = unit(pout.x - vk.x, pout.y - vk.y)?;
    Some(VCorner { x: vk, ua, ub, la: dist(pin, vk), lb: dist(pout, vk), turn: js::sign(-ua.x * ub.y + ua.y * ub.x) })
}

fn layer_of(a: &Entity, b: &Entity, current: Option<&Current>) -> Like {
    if a.layer == b.layer && Color::or_null(&a.color) == Color::or_null(&b.color) {
        return Like { layer: a.layer.clone(), color: Color::or_null(&a.color) };
    }
    Like {
        layer: current.and_then(|c| c.layer.clone()).or_else(|| a.layer.clone()),
        color: current.and_then(|c| c.color.clone()).unwrap_or(Color::Null),
    }
}

/// A corner's outcome.
#[derive(Clone, Debug, PartialEq)]
pub struct Corner {
    pub error: Option<&'static str>,
    pub edits: Vec<Entity>,
    pub add: Vec<Entity>,
    pub semicircle: bool,
}
impl Corner {
    fn err(e: &'static str) -> Corner {
        Corner { error: Some(e), edits: Vec::new(), add: Vec::new(), semicircle: false }
    }
    fn ok(edits: Vec<Entity>, add: Vec<Entity>) -> Corner {
        Corner { error: None, edits, add, semicircle: false }
    }
    pub fn to_value(&self) -> Value {
        let mut v = match self.error {
            Some(e) => serde_json::json!({ "ok": false, "error": e }),
            None => serde_json::json!({
                "ok": true,
                "edits": self.edits.iter().map(Entity::to_value).collect::<Vec<_>>(),
                "add": self.add.iter().map(Entity::to_value).collect::<Vec<_>>(),
            }),
        };
        if self.semicircle
            && let Some(o) = v.as_object_mut()
        {
            o.insert("semicircle".into(), true.into());
        }
        v
    }
}

fn same_obj(a: &Target, b: &Target) -> bool {
    std::ptr::eq(a.e, b.e) || (a.e.id.is_some() && a.e.id == b.e.id)
}

/// FILLET between two picked targets with radius r. `trim` false leaves the objects as they are. Parallel lines get
/// a half circle across the gap, at the end of the first line nearest its pick.
pub fn fillet_pair(a: Option<&Target>, b: Option<&Target>, r: f64, trim: bool, current: Option<&Current>) -> Corner {
    let (Some(a), Some(b)) = (a, b) else { return Corner::err("type") };
    let like = layer_of(a.e, b.e, current);
    if same_obj(a, b) {
        let Geo::Poly(pl) = &a.e.geo else { return Corner::err("same") };
        if a.seg == b.seg {
            return Corner::err("same");
        }
        let Some(sv) = shared_vertex(a, b) else { return cut_both(a, b, None, None, trim, Vec::new(), true) };
        let pp = poly_prims_of(a.e);
        if !pp.get(sv.in_seg).is_some_and(Prim::is_l) || !pp.get(sv.out_seg).is_some_and(Prim::is_l) {
            return Corner::err("type");
        }
        let Some(vc) = vertex_corner(&pl.pts, sv.k, sv.in_seg, sv.out_seg) else { return Corner::err("none") };
        if r <= EPS {
            return Corner::ok(Vec::new(), Vec::new());
        }
        let Some(fl) = ll_fillet(vc.x, vc.ua, vc.ub, r) else { return Corner::err("collinear") };
        if fl.t > vc.la + 1e-9 || fl.t > vc.lb + 1e-9 {
            return Corner::err("large");
        }
        let sweep = PI - 2.0 * clamp1(dot(vc.ua, vc.ub)).jacos() / 2.0;
        return Corner::ok(vec![splice_corner(a.e, sv.k, fl.a, (sweep / 4.0).jtan() * vc.turn, fl.b)], Vec::new());
    }
    let (pa, pb) = (a.prim, b.prim);
    if pa.is_l() && pb.is_l() {
        let Some(x) = intersect(&pa, &pb, true, true).first().copied() else {
            // parallel lines: a half circle across the gap
            let ua = line_dir(&pa).unwrap_or(p(f64::NAN, f64::NAN));
            let ub = line_dir(&pb).unwrap_or(p(f64::NAN, f64::NAN));
            let mut es = [p(pa.x1, pa.y1), p(pa.x2, pa.y2)];
            if dist(es[1], a.pick) - dist(es[0], a.pick) < 0.0 {
                es.swap(0, 1);
            }
            let e0 = es[0];
            let s = (e0.x - pb.x1) * ub.x + (e0.y - pb.y1) * ub.y;
            let f0 = p(pb.x1 + ub.x * s, pb.y1 + ub.y * s);
            let gap = dist(e0, f0);
            if gap <= 1e-9 * js::max(1.0, prim_length(&pa)) {
                return Corner::err("collinear");
            }
            let other = if dist(e0, p(pa.x1, pa.y1)) < dist(e0, p(pa.x2, pa.y2)) { p(pa.x2, pa.y2) } else { p(pa.x1, pa.y1) };
            let k = unit(other.x - e0.x, other.y - e0.y).unwrap_or(ua);
            let m = p((e0.x + f0.x) / 2.0, (e0.y + f0.y) / 2.0);
            let rr = gap / 2.0;
            let ae = (e0.y - m.y).jatan2(e0.x - m.x);
            let af = (f0.y - m.y).jatan2(f0.x - m.x);
            let ccw_from_e = (e0.x - m.x) * -k.y - (e0.y - m.y) * -k.x > 0.0;
            let arc = Entity::with(
                arc_ent(m.x, m.y, rr, norm_angle(if ccw_from_e { ae } else { af }), norm_angle(if ccw_from_e { af } else { ae })),
                like.layer.clone(),
                Some(like.color.clone()),
            );
            let mut out = cut_both(a, b, Some((e0, Keep::Dir(k))), Some((f0, Keep::Dir(k))), trim, vec![arc], false);
            out.semicircle = true;
            return out;
        };
        let (Some(ra), Some(rb)) = (keep_ray(&pa, x, a.pick), keep_ray(&pb, x, b.pick)) else { return Corner::err("none") };
        if r <= EPS {
            return cut_both(a, b, Some((x, Keep::Dir(ra.0))), Some((x, Keep::Dir(rb.0))), trim, Vec::new(), false);
        }
        let Some(fl) = ll_fillet(x, ra.0, rb.0, r) else { return Corner::err("collinear") };
        if fl.t >= ra.1 - 1e-9 || fl.t >= rb.1 - 1e-9 {
            return Corner::err("large");
        }
        let arc = corner_arc(fl.c, r, fl.a, fl.b, &like);
        return cut_both(a, b, Some((fl.a, Keep::Dir(ra.0))), Some((fl.b, Keep::Dir(rb.0))), trim, vec![arc], false);
    }
    // a line, arc or circle with an arc or circle
    let full = |q: &Prim| Prim::circle(q.cx, q.cy, q.r);
    let inf = |q: &Prim| if q.is_l() { *q } else { full(q) };
    if r <= EPS {
        let mut best: Option<(f64, P2)> = None;
        for x in intersect(&inf(&pa), &inf(&pb), true, true) {
            let s = dist(x, a.pick) + dist(x, b.pick);
            if best.is_none_or(|bb| s < bb.0) {
                best = Some((s, x));
            }
        }
        let Some((_, x)) = best else { return Corner::err("none") };
        return cut_both(a, b, Some((x, sharp_keep(&pa, x, a.pick))), Some((x, sharp_keep(&pb, x, b.pick))), trim, Vec::new(), false);
    }
    let offsets = |q: &Prim| -> Vec<Prim> {
        if q.is_l() {
            let u = line_dir(q).unwrap_or(p(f64::NAN, f64::NAN));
            let n = p(-u.y * r, u.x * r);
            return [1.0, -1.0].iter().map(|s| Prim::l(q.x1 + n.x * s, q.y1 + n.y * s, q.x2 + n.x * s, q.y2 + n.y * s)).collect();
        }
        [q.r + r, q.r - r].into_iter().filter(|rr| *rr > EPS).map(|rr| Prim { r: rr, ..full(q) }).collect()
    };
    let touch = |q: &Prim, c: P2| -> Option<P2> {
        if q.is_l() {
            let t = prim_param(q, c, false);
            return Some(prim_point(q, t));
        }
        let u = unit(c.x - q.cx, c.y - q.cy)?;
        Some(p(q.cx + u.x * q.r, q.cy + u.y * q.r))
    };
    let mut best: Option<(f64, P2, P2, P2)> = None;
    for oa in offsets(&pa) {
        for ob in offsets(&pb) {
            for c in intersect(&oa, &ob, true, true) {
                let (Some(ta), Some(tb)) = (touch(&pa, c), touch(&pb, c)) else { continue };
                if dist(ta, tb) <= 1e-9 {
                    continue;
                }
                let s = dist(ta, a.pick) + dist(tb, b.pick);
                if best.is_none_or(|bb| s < bb.0) {
                    best = Some((s, c, ta, tb));
                }
            }
        }
    }
    let Some((_, c, ta, tb)) = best else { return Corner::err("none") };
    let away = |q: &Prim, at: P2, to: P2| -> Keep {
        if q.is_l() {
            let u = line_dir(q).unwrap_or(p(f64::NAN, f64::NAN));
            let s = dot(u, p(to.x - at.x, to.y - at.y));
            return Keep::Dir(if s > 0.0 { p(-u.x, -u.y) } else { u });
        }
        let ang = (at.y - q.cy).jatan2(at.x - q.cx);
        if (to.x - at.x) * -ang.jsin() + (to.y - at.y) * ang.jcos() > 0.0 { Keep::Cw } else { Keep::Ccw }
    };
    let arc = corner_arc(c, r, ta, tb, &like);
    cut_both(a, b, Some((ta, away(&pa, ta, tb))), Some((tb, away(&pb, tb, ta))), trim, vec![arc], false)
}

/// For a sharp corner on a curve: which way from x the picked part lies.
fn sharp_keep(q: &Prim, x: P2, pick: P2) -> Keep {
    if q.is_l() {
        let u = line_dir(q).unwrap_or(p(f64::NAN, f64::NAN));
        return Keep::Dir(if dot(u, p(pick.x - x.x, pick.y - x.y)) >= 0.0 { u } else { p(-u.x, -u.y) });
    }
    let a = (x.y - q.cy).jatan2(x.x - q.cx);
    let ap = (pick.y - q.cy).jatan2(pick.x - q.cx);
    if on_sweep(a, q.a0, q.a1, 1e-9) && !q.full {
        return if norm_angle(ap - q.a0) <= norm_angle(a - q.a0) { Keep::Cw } else { Keep::Ccw };
    }
    if q.full {
        return if norm_angle(ap - a) <= PI { Keep::Ccw } else { Keep::Cw };
    }
    if norm_angle(q.a0 - a) <= norm_angle(a - q.a1) { Keep::Ccw } else { Keep::Cw }
}

/// Cut both targets ([point, keep] each; None = leave it) and collect the edits. Two cuts on one polyline chain.
fn cut_both(a: &Target, b: &Target, ca: Option<(P2, Keep)>, cb: Option<(P2, Keep)>, trim: bool, add: Vec<Entity>, same: bool) -> Corner {
    if same && (ca.is_none() || cb.is_none()) {
        return Corner::err("middle");
    }
    if !trim {
        return Corner::ok(Vec::new(), add);
    }
    let ra = match ca {
        Some((pt, k)) => match cut_target(a, pt, k, a.e) {
            Ok(x) => x,
            Err(e) => return Corner::err(e),
        },
        None => None,
    };
    let base_b: &Entity = if same { ra.as_ref().unwrap_or(a.e) } else { b.e };
    let rb = match cb {
        Some((pt, k)) => match cut_target(b, pt, k, base_b) {
            Ok(x) => x,
            Err(e) => return Corner::err(e),
        },
        None => None,
    };
    let edits = if same { vec![rb.unwrap_or_else(|| base_b.clone())] } else { ra.into_iter().chain(rb).collect() };
    Corner::ok(edits, add)
}

/// A CHAMFER spec: method 'D' (distances d1 from the first piece, d2 from the second) or 'A' (len from the first at
/// ang to it, radians).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChamferSpec {
    pub method: Option<String>,
    pub d1: Option<f64>,
    pub d2: Option<f64>,
    pub len: Option<f64>,
    pub ang: Option<f64>,
}
impl ChamferSpec {
    pub fn from_value(v: &Value) -> ChamferSpec {
        let g = |k: &str| v.get(k).and_then(Value::as_f64);
        ChamferSpec {
            method: v.get("method").and_then(Value::as_str).map(str::to_string),
            d1: g("d1"),
            d2: g("d2"),
            len: g("len"),
            ang: g("ang"),
        }
    }
}

/// The chamfer cut on each side of a corner: (da, db), or None when the angle does not close the corner.
fn chamfer_cuts(spec: &ChamferSpec, ua: P2, ub: P2) -> Option<(f64, f64)> {
    if spec.method.as_deref() == Some("A") {
        let theta = clamp1(dot(ua, ub)).jacos();
        let al = js::or(spec.ang, 0.0);
        if !spec.len.is_some_and(|l| l > EPS) {
            return Some((0.0, 0.0));
        }
        let len = spec.len.unwrap_or(0.0);
        if !(al > 1e-9) || theta + al >= PI - 1e-9 {
            return None;
        }
        return Some((len, len * al.jsin() / (theta + al).jsin()));
    }
    Some((js::max(0.0, js::or(spec.d1, 0.0)), js::max(0.0, spec.d2.or(spec.d1).unwrap_or(0.0))))
}

/// CHAMFER between two picked straight pieces.
pub fn chamfer_pair(a: Option<&Target>, b: Option<&Target>, spec: &ChamferSpec, trim: bool, current: Option<&Current>) -> Corner {
    let (Some(a), Some(b)) = (a, b) else { return Corner::err("type") };
    if !a.prim.is_l() || !b.prim.is_l() {
        return Corner::err("type");
    }
    let like = layer_of(a.e, b.e, current);
    if same_obj(a, b) {
        let Geo::Poly(pl) = &a.e.geo else { return Corner::err("same") };
        if a.seg == b.seg {
            return Corner::err("same");
        }
        let Some(sv) = shared_vertex(a, b) else { return Corner::err("middle") };
        let Some(vc) = vertex_corner(&pl.pts, sv.k, sv.in_seg, sv.out_seg) else { return Corner::err("none") };
        let first_in = a.seg == Some(sv.in_seg);
        let Some((da, db)) = chamfer_cuts(spec, if first_in { vc.ua } else { vc.ub }, if first_in { vc.ub } else { vc.ua }) else {
            return Corner::err("angle");
        };
        let (d_in, d_out) = if first_in { (da, db) } else { (db, da) };
        if dot(vc.ua, vc.ub).abs() > 1.0 - 1e-12 {
            return Corner::err("collinear");
        }
        if d_in > vc.la + 1e-9 || d_out > vc.lb + 1e-9 {
            return Corner::err("large");
        }
        if d_in <= EPS && d_out <= EPS {
            return Corner::ok(Vec::new(), Vec::new());
        }
        let pa = polar(vc.x, vc.ua.y.jatan2(vc.ua.x), d_in);
        let pb = polar(vc.x, vc.ub.y.jatan2(vc.ub.x), d_out);
        return Corner::ok(vec![splice_corner(a.e, sv.k, pa, 0.0, pb)], Vec::new());
    }
    let Some(x) = intersect(&a.prim, &b.prim, true, true).first().copied() else { return Corner::err("parallel") };
    let (Some(ra), Some(rb)) = (keep_ray(&a.prim, x, a.pick), keep_ray(&b.prim, x, b.pick)) else { return Corner::err("none") };
    if dot(ra.0, rb.0).abs() > 1.0 - 1e-12 {
        return Corner::err("collinear");
    }
    let Some((da, db)) = chamfer_cuts(spec, ra.0, rb.0) else { return Corner::err("angle") };
    if da > ra.1 + 1e-9 || db > rb.1 + 1e-9 {
        return Corner::err("large");
    }
    let pa = polar(x, ra.0.y.jatan2(ra.0.x), da);
    let pb = polar(x, rb.0.y.jatan2(rb.0.x), db);
    let add = if dist(pa, pb) > EPS {
        vec![Entity::with(line_ent(pa.x, pa.y, pb.x, pb.y), like.layer.clone(), Some(like.color.clone()))]
    } else {
        Vec::new()
    };
    cut_both(a, b, Some((pa, Keep::Dir(ra.0))), Some((pb, Keep::Dir(rb.0))), trim, add, false)
}

/// Undo the fillet arcs a polyline already has (an arc segment tangent to the straight segments on both sides).
fn unfillet_poly(e: &Entity) -> (Entity, usize) {
    let Geo::Poly(pl) = &e.geo else { return (e.clone(), 0) };
    let v = &pl.pts;
    let n = v.len();
    let closed = pl.is_closed();
    if n < 4 {
        return (e.clone(), 0);
    }
    let dir = |a: &Vertex, b: &Vertex| (b.y - a.y).jatan2(b.x - a.x);
    let same = |a: f64, b: f64| {
        let d = norm_angle(a - b);
        d < 1e-6 || d > TAU - 1e-6
    };
    let mut drop: Vec<usize> = Vec::new();
    let mut at: Vec<(usize, P2)> = Vec::new();
    for k in 0..n {
        let (pk, k1, k2) = ((k + n - 1) % n, (k + 1) % n, (k + 2) % n);
        let (Some(vk), Some(vpk), Some(vk1), Some(vk2)) = (v.get(k), v.get(pk), v.get(k1), v.get(k2)) else { continue };
        let b = vk.bulge();
        if b.abs() < 1e-12 {
            continue;
        }
        if !closed && (k < 1 || k + 2 > n - 1) {
            continue;
        }
        if vpk.bulge().abs() > 1e-12
            || vk1.bulge().abs() > 1e-12
            || drop.contains(&k)
            || drop.contains(&k1)
            || at.iter().any(|(i, _)| *i == k1)
        {
            continue;
        }
        let chord = dir(vk, vk1);
        let th = 4.0 * b.jatan();
        if !same(dir(vpk, vk), chord - th / 2.0) || !same(dir(vk1, vk2), chord + th / 2.0) {
            continue;
        }
        let Some(x) = intersect(&Prim::l(vpk.x, vpk.y, vk.x, vk.y), &Prim::l(vk1.x, vk1.y, vk2.x, vk2.y), true, true).first().copied()
        else {
            continue;
        };
        at.push((k, x));
        drop.push(k1);
    }
    if at.is_empty() {
        return (e.clone(), 0);
    }
    let mut pts = Vec::new();
    for (k, q) in v.iter().enumerate() {
        if drop.contains(&k) {
            continue;
        }
        match at.iter().find(|(i, _)| *i == k) {
            Some((_, x)) => pts.push(Vertex::new(x.x, x.y, 0.0)),
            None => pts.push(q.clone()),
        }
    }
    (with_geo(e, Geo::Poly(Poly { pts, closed: pl.closed })), at.len())
}

/// polyCorners' spec: a fillet radius, or a chamfer spec.
#[derive(Clone, Debug)]
pub enum CornerSpec {
    Fillet(f64),
    Chamfer(ChamferSpec),
}

/// Every corner of a polyline where two straight segments meet.
#[derive(Clone, Debug, PartialEq)]
pub struct PolyCorners {
    pub e: Entity,
    pub done: usize,
    pub short: usize,
    pub arcs: usize,
    pub removed: usize,
}
impl PolyCorners {
    pub fn to_value(&self) -> Value {
        serde_json::json!({ "e": self.e.to_value(), "done": self.done, "short": self.short, "arcs": self.arcs, "removed": self.removed })
    }
}

/// Fillet ('fillet': old fillet arcs are taken out first) or chamfer every corner of a polyline.
pub fn poly_corners(e: &Entity, spec: &CornerSpec) -> PolyCorners {
    let (src, removed) = if matches!(spec, CornerSpec::Fillet(_)) { unfillet_poly(e) } else { (e.clone(), 0) };
    let Geo::Poly(pl) = &src.geo else { return PolyCorners { e: e.clone(), done: 0, short: 0, arcs: 0, removed } };
    let v = &pl.pts;
    let n = v.len();
    let closed = pl.is_closed();
    let segs = if closed { n } else { n.saturating_sub(1) };
    let is_line = |i: usize| v.get(i).is_some_and(|q| q.bulge().abs() < 1e-12);
    let mut used = vec![0.0f64; segs];
    let seg_len = |i: usize| match (v.get(i), v.get((i + 1) % n.max(1))) {
        (Some(a), Some(b)) => dist(a.p(), b.p()),
        _ => f64::NAN,
    };
    let mut repl: Vec<(usize, P2, P2, f64)> = Vec::new();
    let (mut short, mut arcs) = (0usize, 0usize);
    let (k0, k1) = if closed { (0, n) } else { (1, n.saturating_sub(1)) };
    for k in k0..k1 {
        let i = (k + n - 1) % n;
        if !is_line(i) || !is_line(k) {
            arcs += 1;
            continue;
        }
        let Some(vc) = vertex_corner(v, k, i, k) else { continue };
        if dot(vc.ua, vc.ub).abs() > 1.0 - 1e-12 {
            continue;
        }
        let (a, b, bulge);
        match spec {
            CornerSpec::Fillet(r) => {
                if !(*r > EPS) {
                    continue;
                }
                let Some(fl) = ll_fillet(vc.x, vc.ua, vc.ub, *r) else { continue };
                a = fl.a;
                b = fl.b;
                let sweep = PI - clamp1(dot(vc.ua, vc.ub)).jacos();
                bulge = (sweep / 4.0).jtan() * vc.turn;
            }
            CornerSpec::Chamfer(cs) => {
                let Some((da, db)) = chamfer_cuts(cs, vc.ua, vc.ub) else {
                    short += 1;
                    continue;
                };
                if da <= EPS && db <= EPS {
                    continue;
                }
                a = polar(vc.x, vc.ua.y.jatan2(vc.ua.x), da);
                b = polar(vc.x, vc.ub.y.jatan2(vc.ub.x), db);
                bulge = 0.0;
            }
        }
        let ta = dist(vc.x, a);
        let tb = dist(vc.x, b);
        let ui = used.get(i).copied().unwrap_or(0.0);
        let uk = used.get(k).copied().unwrap_or(0.0);
        if ta > seg_len(i) - ui + 1e-9 || tb > seg_len(k) - uk + 1e-9 {
            short += 1;
            continue;
        }
        if let Some(x) = used.get_mut(i) {
            *x += ta;
        }
        if let Some(x) = used.get_mut(k) {
            *x += tb;
        }
        repl.push((k, a, b, bulge));
    }
    if repl.is_empty() {
        return PolyCorners { e: if removed > 0 { src.clone() } else { e.clone() }, done: 0, short, arcs, removed };
    }
    let mut pts = Vec::new();
    for (k, q) in v.iter().enumerate() {
        match repl.iter().find(|r| r.0 == k) {
            Some((_, a, b, bulge)) => {
                pts.push(Vertex::new(a.x, a.y, *bulge));
                pts.push(Vertex::new(b.x, b.y, q.bulge()));
            }
            None => pts.push(q.clone()),
        }
    }
    let done = repl.len();
    PolyCorners { e: with_geo(&src, Geo::Poly(Poly { pts: dedupe(pts, closed), closed: pl.closed })), done, short, arcs, removed }
}

/// Explode: a polyline into lines/arcs, an insert into its (transformed) block entities and attribute texts, a
/// dimension or multileader into what it draws. None for other types (or an insert of a missing block).
pub fn explode_entity(e: &Entity, blocks: Option<&Blocks>) -> Option<Vec<Entity>> {
    let col = Some(Color::or_null(&e.color));
    match &e.geo {
        Geo::Poly(_) => Some(
            poly_prims_of(e)
                .iter()
                .map(|q| {
                    let g = if q.is_l() {
                        line_ent(q.x1, q.y1, q.x2, q.y2)
                    } else {
                        arc_ent(q.cx, q.cy, q.r, norm_angle(q.a0), norm_angle(q.a1))
                    };
                    Entity::with(g, e.layer.clone(), col.clone())
                })
                .collect(),
        ),
        Geo::Mleader(_) => Some(
            crate::leader::leader_geometry(e)
                .into_iter()
                .map(|mut c| {
                    c.layer = e.layer.clone();
                    c.color = col.clone();
                    c
                })
                .collect(),
        ),
        Geo::Insert(_) | Geo::Dim(_) => {
            let (parts, m) = match &e.geo {
                Geo::Insert(ins) => {
                    let blk = blocks.and_then(|b| b.get(&ins.block))?;
                    let mut parts = blk.entities.clone();
                    for mut t in attr_texts(ins, Some(blk), false) {
                        t.extra.remove("tag");
                        t.extra.remove("invisible");
                        parts.push(t);
                    }
                    (parts, Some(insert_matrix(ins, blk)))
                }
                Geo::Dim(d) => (dim_parts(e, d.block.as_deref(), blocks), None),
                _ => return None,
            };
            Some(
                parts
                    .iter()
                    .map(|c| {
                        let mut t = match &m {
                            Some(m) => tx_entity(c, m),
                            None => c.clone(),
                        };
                        if t.layer.as_deref() == Some("0") {
                            t.layer = e.layer.clone();
                        }
                        if t.color == Some(Color::Num(0.0)) {
                            t.color = col.clone();
                        }
                        t.id = None;
                        t.extra.remove("id");
                        t
                    })
                    .collect(),
            )
        }
        _ => None,
    }
}
