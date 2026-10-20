//! xclip.js: clipped block references and linked drawings (0.5 WL.16, XCLIP; WL.17 binds only the clipped part).
//!
//! A reference's clip is `insert.clip = { pts: [{x, y}…] }`: a closed polygon (≥ 3 points; a rectangle is 4) in the
//! BLOCK's own coordinates, so it moves, turns and scales with the reference.
//!
//! * [`ClipOut`] — a walk output that passes on only what lies inside a polygon (world coordinates): lines and arcs cut
//!   at the boundary, fills cut to it, texts and points kept when their insertion point is inside — so the screen,
//!   plots and extents all clip without knowing about clips (walk.js wraps a clipped reference's contents in it);
//! * [`clip_entities`] — a block's objects cut to a clip in its own coordinates (XREF Bind, clipped part only).
//!
//! `clipFromFilter` / `filterOf` (the SPATIAL_FILTER a file keeps) live with the readers and savers in cc-io-cad.

use cc_cad::entity::{Entity, Geo, Obj};
use cc_geom::P2;
use serde_json::Value;

use crate::geom::{Blocks, K, Prim, TAU, p, prims};
use crate::jsmath::JsMath;
use crate::pick::{entity_bounds, valid_bounds};
use crate::walk::{Opts, PicOut, WalkOut};

const EPS: f64 = 1e-12;

/// Even–odd point in polygon.
pub fn inside_poly(q: P2, poly: &[P2]) -> bool {
    let mut c = false;
    let n = poly.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (Some(a), Some(b)) = (poly.get(i), poly.get(j)) else { break };
        if (a.y > q.y) != (b.y > q.y) && q.x < ((b.x - a.x) * (q.y - a.y)) / (b.y - a.y) + a.x {
            c = !c;
        }
        j = i;
    }
    c
}

/// Pairs (previous, this) round the polygon, as `for (i = 0, j = n − 1; i < n; j = i++)` visits them.
fn edges(poly: &[P2]) -> impl Iterator<Item = (P2, P2)> + '_ {
    let last = poly.last().copied();
    poly.iter().scan(last, |prev, q| {
        let pr = (*prev)?;
        *prev = Some(*q);
        Some((pr, *q))
    })
}

/// Where segment a→b crosses the polygon's edges: parameters t in (0, 1).
fn seg_crossings(a: P2, b: P2, poly: &[P2]) -> Vec<f64> {
    let mut out = Vec::new();
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    for (pp, q) in edges(poly) {
        let (ex, ey) = (q.x - pp.x, q.y - pp.y);
        let den = dx * ey - dy * ex;
        if den.abs() < EPS {
            continue;
        }
        let tt = ((pp.x - a.x) * ey - (pp.y - a.y) * ex) / den;
        let u = ((pp.x - a.x) * dy - (pp.y - a.y) * dx) / den;
        if tt > EPS && tt < 1.0 - EPS && (-EPS..=1.0 + EPS).contains(&u) {
            out.push(tt);
        }
    }
    out
}

/// JS `arr.sort((x, y) => x - y)` on finite numbers.
fn sort_nums(v: &mut [f64]) {
    v.sort_by(f64::total_cmp);
}

/// A line primitive cut to the polygon → the pieces inside.
pub fn clip_line(q: &Prim, poly: &[P2]) -> Vec<Prim> {
    let a = p(q.x1, q.y1);
    let b = p(q.x2, q.y2);
    let mut cr = seg_crossings(a, b, poly);
    sort_nums(&mut cr);
    let mut ts = Vec::with_capacity(cr.len().saturating_add(2));
    ts.push(0.0);
    ts.extend(cr);
    ts.push(1.0);
    let mut out: Vec<(f64, f64)> = Vec::new();
    for w in ts.windows(2) {
        let [t0, t1] = [w.first().copied().unwrap_or(0.0), w.get(1).copied().unwrap_or(0.0)];
        if t1 - t0 < EPS {
            continue;
        }
        let m = (t0 + t1) / 2.0;
        if !inside_poly(p(a.x + (b.x - a.x) * m, a.y + (b.y - a.y) * m), poly) {
            continue;
        }
        match out.last_mut() {
            Some(prev) if (prev.1 - t0).abs() < EPS => prev.1 = t1,
            _ => out.push((t0, t1)),
        }
    }
    out.into_iter()
        .map(|(t0, t1)| Prim {
            x1: a.x + (b.x - a.x) * t0,
            y1: a.y + (b.y - a.y) * t0,
            x2: a.x + (b.x - a.x) * t1,
            y2: a.y + (b.y - a.y) * t1,
            ..*q
        })
        .collect()
}

/// `while (v <= 0) v += TAU` (and its mirror images), bounded: a value too far off for that comes back as it is.
fn up_to_positive(mut v: f64) -> f64 {
    let mut n = 0;
    while v <= 0.0 && n < 64 {
        v += TAU;
        n += 1;
    }
    v
}

/// An arc primitive (counter-clockwise a0 → a1) cut to the polygon → the sub-arcs inside.
pub fn clip_arc(q: &Prim, poly: &[P2]) -> Vec<Prim> {
    let sweep = up_to_positive(q.a1 - q.a0);
    let mut ang = Vec::new();
    for (pp, r) in edges(poly) {
        let (dx, dy, fx, fy) = (r.x - pp.x, r.y - pp.y, pp.x - q.cx, pp.y - q.cy);
        let aa = dx * dx + dy * dy;
        let bb = 2.0 * (fx * dx + fy * dy);
        let cc = fx * fx + fy * fy - q.r * q.r;
        let d = bb * bb - 4.0 * aa * cc;
        if aa < EPS || d < 0.0 {
            continue;
        }
        for s in [(-bb - d.sqrt()) / (2.0 * aa), (-bb + d.sqrt()) / (2.0 * aa)] {
            if s < -EPS || s > 1.0 + EPS {
                continue;
            }
            let mut a = (pp.y + dy * s - q.cy).jatan2(pp.x + dx * s - q.cx) - q.a0;
            let mut n = 0;
            while a < 0.0 && n < 64 {
                a += TAU;
                n += 1;
            }
            while a >= TAU && n < 128 {
                a -= TAU;
                n += 1;
            }
            if a > EPS && a < sweep - EPS {
                ang.push(a);
            }
        }
    }
    sort_nums(&mut ang);
    let mut ts = Vec::with_capacity(ang.len().saturating_add(2));
    ts.push(0.0);
    ts.extend(ang);
    ts.push(sweep);
    let mut out: Vec<(f64, f64)> = Vec::new();
    for w in ts.windows(2) {
        let [s0, s1] = [w.first().copied().unwrap_or(0.0), w.get(1).copied().unwrap_or(0.0)];
        if s1 - s0 < 1e-9 {
            continue;
        }
        let m = q.a0 + (s0 + s1) / 2.0;
        if !inside_poly(p(q.cx + q.r * m.jcos(), q.cy + q.r * m.jsin()), poly) {
            continue;
        }
        match out.last_mut() {
            Some(prev) if (prev.1 - s0).abs() < 1e-9 => prev.1 = s1,
            _ => out.push((s0, s1)),
        }
    }
    out.into_iter().map(|(s0, s1)| Prim { a0: q.a0 + s0, a1: q.a0 + s1, ..*q }).collect()
}

fn js_sign(z: f64) -> f64 {
    if z > 0.0 {
        1.0
    } else if z < 0.0 {
        -1.0
    } else {
        z
    }
}

fn convex(poly: &[P2]) -> bool {
    let n = poly.len();
    let mut sign = 0.0;
    for i in 0..n {
        let (Some(a), Some(b), Some(c)) = (poly.get(i), poly.get((i + 1) % n), poly.get((i + 2) % n)) else { continue };
        let z = (b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x);
        if z.abs() < EPS {
            continue;
        }
        if sign != 0.0 && js_sign(z) != sign {
            return false;
        }
        sign = js_sign(z);
    }
    true
}

/// A fill ring [x, y, …] cut to the polygon (Sutherland–Hodgman; for a concave clip, rings across it are kept whole).
pub fn clip_ring(l: &[f64], poly: &[P2]) -> Option<Vec<f64>> {
    let mut pts: Vec<P2> = l.chunks_exact(2).map(|c| p(c.first().copied().unwrap_or(0.0), c.get(1).copied().unwrap_or(0.0))).collect();
    if pts.len() > 1
        && let (Some(f), Some(z)) = (pts.first(), pts.last())
        && (f.x - z.x).abs() < EPS
        && (f.y - z.y).abs() < EPS
    {
        pts.pop();
    }
    let ins = pts.iter().filter(|q| inside_poly(**q, poly)).count();
    if ins == pts.len() && !poly.iter().any(|q| inside_poly(*q, &pts)) {
        return Some(l.to_vec());
    }
    if !convex(poly) {
        return if ins > 0 { Some(l.to_vec()) } else { None };
    }
    let n = poly.len();
    let mut ccw = 0.0;
    for (a, b) in poly.iter().zip(poly.iter().cycle().skip(1)).take(n) {
        ccw += a.x * b.y - b.x * a.y;
    }
    let dir = if ccw >= 0.0 { 1.0 } else { -1.0 };
    let side = |q: P2, a: P2, b: P2| ((b.x - a.x) * (q.y - a.y) - (b.y - a.y) * (q.x - a.x)) * dir >= -EPS;
    for i in 0..n {
        if pts.is_empty() {
            break;
        }
        let (Some(&a), Some(&b)) = (poly.get(i), poly.get((i + 1) % n)) else { break };
        let inp = std::mem::take(&mut pts);
        let m = inp.len();
        for k in 0..m {
            let (Some(&q0), Some(&q1)) = (inp.get(k), inp.get((k + 1) % m)) else { break };
            let (pi, qi) = (side(q0, a, b), side(q1, a, b));
            if pi {
                pts.push(q0);
            }
            if pi != qi {
                let (dx, dy, ex, ey) = (q1.x - q0.x, q1.y - q0.y, b.x - a.x, b.y - a.y);
                let den = dx * ey - dy * ex;
                if den.abs() > EPS {
                    let tt = ((a.x - q0.x) * ey - (a.y - q0.y) * ex) / den;
                    pts.push(p(q0.x + dx * tt, q0.y + dy * tt));
                }
            }
        }
    }
    if pts.len() < 3 {
        return None;
    }
    let mut out = Vec::with_capacity(pts.len().saturating_mul(2).saturating_add(2));
    for q in &pts {
        out.push(q.x);
        out.push(q.y);
    }
    if let Some(f) = pts.first() {
        out.push(f.x);
        out.push(f.y);
    }
    Some(out)
}

/// The primitives cut to the polygon (lines and arcs; anything else as it is).
fn clip_prims(ps: &[Prim], poly: &[P2]) -> Vec<Prim> {
    ps.iter()
        .flat_map(|q| match q.k {
            K::L => clip_line(q, poly),
            K::A => clip_arc(q, poly),
        })
        .collect()
}

/// `clipOut(out, poly)`: a walk output that passes on only what lies inside `poly` (world coordinates).
pub struct ClipOut<'a> {
    pub out: &'a mut dyn WalkOut,
    pub poly: Vec<P2>,
}

impl WalkOut for ClipOut<'_> {
    fn stroke(&mut self, color: &str, prims: Vec<Prim>, lw: f64, o: Option<Opts>) {
        let c = clip_prims(&prims, &self.poly);
        if !c.is_empty() {
            self.out.stroke(color, c, lw, o);
        }
    }
    fn fill(&mut self, color: &str, loops: Vec<Vec<f64>>, solid: bool, o: Option<Opts>) {
        let c: Vec<Vec<f64>> = loops.iter().filter_map(|l| clip_ring(l, &self.poly)).collect();
        if !c.is_empty() {
            self.out.fill(color, c, solid, o);
        }
    }
    fn text(&mut self, color: &str, e: &Entity, o: Option<Opts>) {
        let at = match &e.geo {
            Geo::Text(t) => Some(p(t.x, t.y)),
            _ => None,
        };
        if at.is_some_and(|q| inside_poly(q, &self.poly)) {
            self.out.text(color, e, o);
        }
    }
    fn point(&mut self, color: &str, x: f64, y: f64) {
        if inside_poly(p(x, y), &self.poly) {
            self.out.point(color, x, y);
        }
    }
    fn viewport(&mut self, e: &Entity) {
        self.out.viewport(e);
    }
    fn begin(&mut self, index: usize) {
        self.out.begin(index);
    }
    fn has_mask(&self) -> bool {
        self.out.has_mask()
    }
    fn mask(&mut self, color: &str, ring: Vec<f64>, wipe: bool) {
        if let Some(c) = clip_ring(&ring, &self.poly) {
            self.out.mask(color, c, wipe);
        }
    }
    fn has_infinite(&self) -> bool {
        self.out.has_infinite()
    }
    fn infinite(&mut self, color: &str, e: &Entity, lw: f64, o: Option<Opts>) {
        self.out.infinite(color, e, lw, o);
    }
    fn picture(&mut self, pic: &PicOut) {
        self.out.picture(pic);
    }
}

/// `clipPoly(ref)`: a usable clip polygon of a reference (block coordinates), or None.
pub fn clip_poly(e: &Entity) -> Option<Vec<P2>> {
    let pts = e.get("clip").and_then(|c| c.get("pts")).and_then(Value::as_array)?;
    let v: Vec<P2> = pts
        .iter()
        .filter_map(|q| {
            let x = q.get("x").and_then(Value::as_f64).filter(|x| x.is_finite())?;
            let y = q.get("y").and_then(Value::as_f64).filter(|y| y.is_finite())?;
            Some(p(x, y))
        })
        .collect();
    if v.len() >= 3 { Some(v) } else { None }
}

/// What [`clip_entities`] made.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Clipped {
    pub entities: Vec<Entity>,
    pub cut: usize,
    pub kept: usize,
}

/// The keys that are geometry (`clipEntities`' GEOM): everything else (layer, colour, linetype, lineweight…) carries
/// over to the pieces.
const GEOM: &[&str] =
    &["id", "type", "x1", "y1", "x2", "y2", "cx", "cy", "r", "a0", "a1", "pts", "closed", "mx", "my", "ratio", "t0", "t1", "w", "z1", "z2"];

fn finite_xy(v: &Obj) -> Option<P2> {
    let x = v.get("x").and_then(Value::as_f64).filter(|x| x.is_finite())?;
    let y = v.get("y").and_then(Value::as_f64).filter(|y| y.is_finite())?;
    Some(p(x, y))
}

/// `clipEntities(doc, entities, poly)`: a block's objects cut to a clip polygon in the block's coordinates (WL.17, XREF
/// Bind "clipped part only"). Objects wholly inside stay as they are; lines, arcs, circles, polylines and ellipses
/// crossing the boundary become their inside pieces as lines and arcs (with the object's layer, colour, linetype,
/// lineweight); fills are cut; texts, points, attribute definitions, dimensions, leaders and nested blocks crossing the
/// boundary are kept when their insertion point (or centre) is inside.
pub fn clip_entities(blocks: &Blocks, entities: &[Entity], poly: &[P2]) -> Clipped {
    let mut r = Clipped::default();
    for e in entities {
        let b = entity_bounds(e, Some(blocks));
        let ok = valid_bounds(&b);
        let corners =
            if ok { vec![p(b.min_x, b.min_y), p(b.max_x, b.min_y), p(b.max_x, b.max_y), p(b.min_x, b.max_y)] } else { Vec::new() };
        let all_in = !corners.is_empty()
            && corners.iter().all(|q| inside_poly(*q, poly))
            && !poly.iter().any(|q| q.x > b.min_x && q.x < b.max_x && q.y > b.min_y && q.y < b.max_y);
        if all_in {
            r.entities.push(e.clone());
            continue;
        }
        match &e.geo {
            Geo::Line(_) | Geo::Arc(_) | Geo::Circle(_) | Geo::Poly(_) | Geo::Ellipse(_) => {
                let ps = clip_prims(&prims(e, None), poly);
                let mut props = e.to_obj();
                for k in GEOM {
                    props.remove(*k);
                }
                for q in &ps {
                    let mut o = Obj::new();
                    match q.k {
                        K::L => {
                            o.insert("type".into(), "line".into());
                            o.extend(props.clone());
                            for (k, v) in [("x1", q.x1), ("y1", q.y1), ("x2", q.x2), ("y2", q.y2)] {
                                o.insert(k.into(), cc_cad::num(v));
                            }
                        }
                        K::A => {
                            o.insert("type".into(), "arc".into());
                            o.extend(props.clone());
                            for (k, v) in [("cx", q.cx), ("cy", q.cy), ("r", q.r), ("a0", q.a0), ("a1", q.a1)] {
                                o.insert(k.into(), cc_cad::num(v));
                            }
                        }
                    }
                    r.entities.push(Entity::from_value(&Value::Object(o)));
                }
                if !ps.is_empty() {
                    r.cut = r.cut.saturating_add(1);
                }
            }
            Geo::Hatch(h) => {
                let loops: Vec<Vec<f64>> = h.loops.iter().filter_map(|l| clip_ring(l, poly)).collect();
                if !loops.is_empty() {
                    let mut c = e.clone();
                    if let Geo::Hatch(hh) = &mut c.geo {
                        hh.loops = loops;
                    }
                    r.entities.push(c);
                    r.cut = r.cut.saturating_add(1);
                }
            }
            _ => {
                let at = finite_xy(&e.to_obj()).or(if ok { Some(p((b.min_x + b.max_x) / 2.0, (b.min_y + b.max_y) / 2.0)) } else { None });
                if at.is_some_and(|q| inside_poly(q, poly)) {
                    r.entities.push(e.clone());
                    r.kept = r.kept.saturating_add(1);
                }
            }
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(x: f64, y: f64, w: f64) -> Vec<P2> {
        vec![p(x, y), p(x + w, y), p(x + w, y + w), p(x, y + w)]
    }

    #[test]
    fn a_line_across_a_square_keeps_its_middle() {
        let r = clip_line(&Prim::l(-5.0, 5.0, 15.0, 5.0), &sq(0.0, 0.0, 10.0));
        assert_eq!(r.len(), 1);
        let q = r.first().copied().unwrap_or(Prim::l(0.0, 0.0, 0.0, 0.0));
        assert!((q.x1 - 0.0).abs() < 1e-12 && (q.x2 - 10.0).abs() < 1e-12);
    }

    #[test]
    fn a_ring_is_cut_to_a_convex_clip_and_kept_whole_inside() {
        let ring = [-5.0, -5.0, 5.0, -5.0, 5.0, 5.0, -5.0, 5.0, -5.0, -5.0];
        let c = clip_ring(&ring, &sq(0.0, 0.0, 10.0)).unwrap_or_default();
        assert_eq!(c.len(), 10);
        let inner = [1.0, 1.0, 2.0, 1.0, 2.0, 2.0];
        assert_eq!(clip_ring(&inner, &sq(0.0, 0.0, 10.0)), Some(inner.to_vec()));
        assert_eq!(clip_ring(&[20.0, 20.0, 21.0, 20.0, 21.0, 21.0], &sq(0.0, 0.0, 10.0)), None);
    }

    #[test]
    fn hostile_angles_do_not_hang() {
        let mut q = Prim::l(0.0, 0.0, 0.0, 0.0);
        q.k = K::A;
        q.r = 5.0;
        q.a0 = 0.0;
        q.a1 = -1e300;
        let _ = clip_arc(&q, &sq(-1.0, -1.0, 2.0));
        q.a1 = f64::NAN;
        let _ = clip_arc(&q, &sq(-1.0, -1.0, 2.0));
    }
}
