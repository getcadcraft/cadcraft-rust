//! geom.js, part 2: bounds, picking, window / crossing / fence selection, object snaps and tangents.

use crate::jsmath::JsMath;
use cc_cad::entity::{Entity, Geo};
use cc_cad::js;
use cc_geom::P2;
use serde_json::Value;

use crate::geom::*;
use crate::tx::tx_entity;

pub fn empty_bounds() -> Bounds {
    Bounds { min_x: f64::INFINITY, min_y: f64::INFINITY, max_x: f64::NEG_INFINITY, max_y: f64::NEG_INFINITY }
}
pub fn grow_pt(b: &mut Bounds, x: f64, y: f64) {
    if x < b.min_x {
        b.min_x = x;
    }
    if y < b.min_y {
        b.min_y = y;
    }
    if x > b.max_x {
        b.max_x = x;
    }
    if y > b.max_y {
        b.max_y = y;
    }
}
pub fn valid_bounds(b: &Bounds) -> bool {
    b.min_x <= b.max_x && b.min_y <= b.max_y
}

pub fn prim_bounds(q: &Prim, b: &mut Bounds) {
    if q.is_l() {
        grow_pt(b, q.x1, q.y1);
        grow_pt(b, q.x2, q.y2);
        return;
    }
    let s = prim_point(q, 0.0);
    let e = prim_point(q, 1.0);
    grow_pt(b, s.x, s.y);
    grow_pt(b, e.x, e.y);
    for k in 0..4 {
        let a = f(k) * PI / 2.0;
        if on_sweep_d(a, q.a0, q.a1) {
            grow_pt(b, q.cx + q.r * a.jcos(), q.cy + q.r * a.jsin());
        }
    }
}

pub fn entity_bounds(e: &Entity, blocks: Option<&Blocks>) -> Bounds {
    let mut b = empty_bounds();
    if e.unbounded() {
        return b;
    }
    if let Geo::Point(pt) = &e.geo {
        grow_pt(&mut b, pt.x, pt.y);
        return b;
    }
    for q in prims(e, blocks) {
        prim_bounds(&q, &mut b);
    }
    if let Geo::Insert(i) = &e.geo
        && !valid_bounds(&b)
    {
        grow_pt(&mut b, i.x, i.y);
    }
    b
}

/// Distance from q to an entity (text and hatch count as hit anywhere inside).
pub fn dist_to_entity(e: &Entity, q: P2, blocks: Option<&Blocks>) -> f64 {
    match &e.geo {
        Geo::Point(pt) => return dist(p(pt.x, pt.y), q),
        Geo::Text(t) if point_in_poly(q, &text_corners(t)) => return 0.0,
        Geo::Hatch(h) if h.loops.iter().any(|l| point_in_flat(q, l)) => return 0.0,
        Geo::Insert(ins) if ins.attrs.is_some() => {
            if let Some(blk) = blocks.and_then(|b| b.get(&ins.block)) {
                let m = insert_matrix(ins, blk);
                let hit = attr_texts(ins, Some(blk), false).iter().any(|t| match &tx_entity(t, &m).geo {
                    Geo::Text(tt) => point_in_poly(q, &text_corners(tt)),
                    _ => false,
                });
                if hit {
                    return 0.0;
                }
            }
        }
        _ => {}
    }
    prims(e, blocks).iter().fold(f64::INFINITY, |best, pr| js::min(best, dist_to_prim(pr, q)))
}

pub fn point_in_poly(q: P2, pts: &[P2]) -> bool {
    let mut inside = false;
    let n = pts.len();
    if n == 0 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (Some(a), Some(b)) = (pts.get(i), pts.get(j)) else { break };
        if (a.y > q.y) != (b.y > q.y) && q.x < (b.x - a.x) * (q.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}
pub fn flat_points(fl: &[f64]) -> Vec<P2> {
    fl.chunks_exact(2).map(|c| p(c.first().copied().unwrap_or(0.0), c.get(1).copied().unwrap_or(0.0))).collect()
}
fn point_in_flat(q: P2, fl: &[f64]) -> bool {
    point_in_poly(q, &flat_points(fl))
}

/// Window (fully inside) or crossing (inside or touching) selection test.
pub fn in_window(e: &Entity, w: &Bounds, crossing: bool, blocks: Option<&Blocks>) -> bool {
    if let Geo::Xline(i) | Geo::Ray(i) = &e.geo {
        return crossing && clip_infinite(p(i.x, i.y), p(i.dx, i.dy), w, matches!(e.geo, Geo::Ray(_))).is_some();
    }
    let b = entity_bounds(e, blocks);
    if !valid_bounds(&b) {
        return false;
    }
    let inside = b.min_x >= w.min_x && b.max_x <= w.max_x && b.min_y >= w.min_y && b.max_y <= w.max_y;
    if inside || !crossing {
        return inside;
    }
    if b.max_x < w.min_x || b.min_x > w.max_x || b.max_y < w.min_y || b.min_y > w.max_y {
        return false;
    }
    let edges = [
        Prim::l(w.min_x, w.min_y, w.max_x, w.min_y),
        Prim::l(w.max_x, w.min_y, w.max_x, w.max_y),
        Prim::l(w.max_x, w.max_y, w.min_x, w.max_y),
        Prim::l(w.min_x, w.max_y, w.min_x, w.min_y),
    ];
    for q in prims(e, blocks) {
        let s = prim_point(&q, 0.0);
        if s.x >= w.min_x && s.x <= w.max_x && s.y >= w.min_y && s.y <= w.max_y {
            return true;
        }
        if edges.iter().any(|ed| !intersect(&q, ed, false, false).is_empty()) {
            return true;
        }
    }
    matches!(e.geo, Geo::Text(_) | Geo::Hatch(_))
}

/// Objects a fence (an open run of points) crosses or touches → their ids (None where an object has no id).
pub fn fence_hits(entities: &[Entity], pts: &[P2], blocks: Option<&Blocks>) -> Vec<Option<u64>> {
    let segs: Vec<Prim> =
        pts.iter().zip(pts.iter().skip(1)).filter(|(a, b)| dist(**a, **b) > EPS).map(|(a, b)| Prim::l(a.x, a.y, b.x, b.y)).collect();
    if segs.is_empty() {
        return Vec::new();
    }
    let mut fb = empty_bounds();
    for s in &segs {
        prim_bounds(s, &mut fb);
    }
    let crosses = |ps: &[Prim]| ps.iter().any(|q| segs.iter().any(|s| !intersect(q, s, false, false).is_empty()));
    let mut out = Vec::new();
    for e in entities {
        if let Geo::Point(pt) = &e.geo {
            if segs.iter().any(|s| dist_to_prim(s, p(pt.x, pt.y)) <= 1e-9) {
                out.push(e.id);
            }
            continue;
        }
        if e.unbounded() {
            if crosses(&prims(e, None)) {
                out.push(e.id);
            }
            continue;
        }
        let b = entity_bounds(e, blocks);
        if !valid_bounds(&b) || b.max_x < fb.min_x || b.min_x > fb.max_x || b.max_y < fb.min_y || b.min_y > fb.max_y {
            continue;
        }
        if crosses(&prims(e, blocks)) {
            out.push(e.id);
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------
// Object snaps

pub const SNAP_KINDS: [&str; 10] = ["end", "mid", "cen", "quad", "int", "perp", "tan", "near", "node", "ins"];

/// A circle {cx, cy, r}.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Circ {
    pub cx: f64,
    pub cy: f64,
    pub r: f64,
}

/// The points on circle c where a line from q touches it (none when q is inside).
pub fn tangent_from(q: P2, c: &Circ) -> Vec<P2> {
    let dx = q.x - c.cx;
    let dy = q.y - c.cy;
    let d = js::hypot(dx, dy);
    if d < c.r - EPS {
        return Vec::new();
    }
    if d <= c.r + EPS {
        return vec![q];
    }
    let a = dy.jatan2(dx);
    let b = (c.r / d).jacos();
    [a + b, a - b].iter().map(|t| p(c.cx + c.r * t.jcos(), c.cy + c.r * t.jsin())).collect()
}
/// The lines touching both circles (outer pair first, then the inner pair).
pub fn common_tangents(c1: &Circ, c2: &Circ) -> Vec<[P2; 2]> {
    let dx = c2.cx - c1.cx;
    let dy = c2.cy - c1.cy;
    let d = js::hypot(dx, dy);
    if d < EPS {
        return Vec::new();
    }
    let mut out = Vec::new();
    let base = dy.jatan2(dx);
    for sgn in [1.0, -1.0] {
        let k = (c1.r - sgn * c2.r) / d;
        if k.abs() > 1.0 + 1e-12 {
            continue;
        }
        let g = js::max(-1.0, js::min(1.0, k)).jacos();
        for t in [base + g, base - g] {
            let n = p(t.jcos(), t.jsin());
            out.push([p(c1.cx + c1.r * n.x, c1.cy + c1.r * n.y), p(c2.cx + sgn * c2.r * n.x, c2.cy + sgn * c2.r * n.y)]);
        }
    }
    out
}
/// The other end of a deferred tangent: a point, or another deferred tangent.
#[derive(Clone, Copy, Debug)]
pub enum TanOther {
    Point(P2),
    Tan(Circ, P2),
}
/// The touch point of a deferred tangent (c, near) once the other end is known: one point, or [a, b] for two
/// deferred tangents; None when there is none.
pub fn resolve_tangent(c: &Circ, near: P2, other: TanOther) -> Option<Vec<P2>> {
    match other {
        TanOther::Tan(c2, near2) => {
            let pairs = common_tangents(c, &c2);
            let mut best = None;
            let mut bd = f64::INFINITY;
            for [a, b] in pairs {
                let sc = dist(a, near) + dist(b, near2);
                if sc < bd {
                    bd = sc;
                    best = Some(vec![a, b]);
                }
            }
            best
        }
        TanOther::Point(q) => {
            let cands = tangent_from(q, c);
            let mut it = cands.into_iter();
            let first = it.next()?;
            Some(vec![it.fold(first, |m, x| if dist(x, near) < dist(m, near) { x } else { m })])
        }
    }
}

/// One snap candidate.
#[derive(Clone, Debug, PartialEq)]
pub struct Snap {
    pub kind: &'static str,
    pub x: f64,
    pub y: f64,
    /// a deferred tangent's circle (no base point yet)
    pub defer: Option<Circ>,
}
impl Snap {
    pub fn to_value(&self) -> Value {
        let mut v = serde_json::json!({ "kind": self.kind, "x": cc_cad::num(self.x), "y": cc_cad::num(self.y) });
        if let (Some(c), Some(o)) = (self.defer, v.as_object_mut()) {
            o.insert("defer".into(), serde_json::json!({ "cx": cc_cad::num(c.cx), "cy": cc_cad::num(c.cy), "r": cc_cad::num(c.r) }));
        }
        v
    }
}

fn kind_name(k: &str) -> &'static str {
    SNAP_KINDS.iter().find(|x| **x == k).copied().unwrap_or("")
}

/// Snap candidates of one entity near q (within r world units).
pub fn snap_points(e: &Entity, q: P2, r: f64, kinds: &[&str], blocks: Option<&Blocks>, base: Option<P2>) -> Vec<Snap> {
    let has = |k: &str| kinds.contains(&k);
    let mut out: Vec<Snap> = Vec::new();
    let add = |out: &mut Vec<Snap>, kind: &str, pt: P2| {
        if has(kind) && (pt.x - q.x).abs() <= r && (pt.y - q.y).abs() <= r {
            out.push(Snap { kind: kind_name(kind), x: pt.x, y: pt.y, defer: None });
        }
    };
    match &e.geo {
        Geo::Point(pt) => {
            add(&mut out, "node", p(pt.x, pt.y));
            return out;
        }
        Geo::Text(t) => {
            add(&mut out, "ins", p(t.x, t.y));
            return out;
        }
        Geo::Insert(i) => add(&mut out, "ins", p(i.x, i.y)),
        _ => {}
    }
    if e.unbounded() {
        let Some(pr) = prims(e, None).into_iter().next() else { return out };
        if let Some(b) = base
            && has("perp")
        {
            let t = prim_param(&pr, b, false);
            if (0.0..=1.0).contains(&t) {
                add(&mut out, "perp", prim_point(&pr, t));
            }
        }
        if has("near") {
            let n = prim_point(&pr, prim_param(&pr, q, true));
            if dist(n, q) <= r {
                out.push(Snap { kind: "near", x: n.x, y: n.y, defer: None });
            }
        }
        return out;
    }
    for pr in prims(e, blocks) {
        if pr.boxed {
            continue;
        }
        if pr.is_l() {
            add(&mut out, "end", p(pr.x1, pr.y1));
            add(&mut out, "end", p(pr.x2, pr.y2));
            add(&mut out, "mid", p((pr.x1 + pr.x2) / 2.0, (pr.y1 + pr.y2) / 2.0));
        } else {
            add(&mut out, "cen", p(pr.cx, pr.cy));
            if !pr.full {
                add(&mut out, "end", prim_point(&pr, 0.0));
                add(&mut out, "end", prim_point(&pr, 1.0));
                add(&mut out, "mid", prim_point(&pr, 0.5));
            }
            for k in 0..4 {
                let a = f(k) * PI / 2.0;
                if on_sweep_d(a, pr.a0, pr.a1) {
                    add(&mut out, "quad", p(pr.cx + pr.r * a.jcos(), pr.cy + pr.r * a.jsin()));
                }
            }
        }
        if let Some(b) = base
            && has("perp")
        {
            if pr.is_l() {
                let t = prim_param(&pr, b, false);
                if (0.0..=1.0).contains(&t) {
                    add(&mut out, "perp", prim_point(&pr, t));
                }
            } else {
                let a = (b.y - pr.cy).jatan2(b.x - pr.cx);
                if on_sweep_d(a, pr.a0, pr.a1) {
                    add(&mut out, "perp", p(pr.cx + pr.r * a.jcos(), pr.cy + pr.r * a.jsin()));
                }
            }
        }
        if has("tan") && !pr.is_l() {
            let c = Circ { cx: pr.cx, cy: pr.cy, r: pr.r };
            if let Some(b) = base {
                for t in tangent_from(b, &c) {
                    if on_sweep_d((t.y - pr.cy).jatan2(t.x - pr.cx), pr.a0, pr.a1) {
                        add(&mut out, "tan", t);
                    }
                }
            } else {
                let a = (q.y - pr.cy).jatan2(q.x - pr.cx);
                let n = p(pr.cx + pr.r * a.jcos(), pr.cy + pr.r * a.jsin());
                if on_sweep_d(a, pr.a0, pr.a1) && (js::hypot(q.x - pr.cx, q.y - pr.cy) - pr.r).abs() <= r {
                    out.push(Snap { kind: "tan", x: n.x, y: n.y, defer: Some(c) });
                }
            }
        }
        if has("near") {
            let n = prim_point(&pr, prim_param(&pr, q, true));
            if dist(n, q) <= r {
                out.push(Snap { kind: "near", x: n.x, y: n.y, defer: None });
            }
        }
    }
    out
}
