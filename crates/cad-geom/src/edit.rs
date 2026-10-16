//! geom.js, part 4: OFFSET, TRIM / EXTEND, FILLET of two lines, REVCLOUD, BREAK, LENGTHEN, JOIN, STRETCH, AREA.
//!
//! Where the JS compares objects by identity (`c !== e`), the Rust compares references (`std::ptr::eq`): pass the
//! picked object as a reference into the same slice as the cutters / boundaries when it is one of them.

use crate::jsmath::JsMath;
use cc_cad::entity::{Circle, Color, Entity, Geo, Inf, Poly, Vertex};
use cc_cad::js;
use cc_geom::P2;
use serde_json::Value;

use crate::geom::*;
use crate::pick::{empty_bounds, point_in_poly, prim_bounds};
use crate::tx::tx_entity;

fn set_xy(v: &mut [Vertex], i: usize, x: f64, y: f64) {
    if let Some(q) = v.get_mut(i) {
        q.x = x;
        q.y = y;
    }
}
fn same(a: &Entity, b: &Entity) -> bool {
    std::ptr::eq(a, b)
}
fn same_id(c: &Entity, e: &Entity) -> bool {
    c.id.is_some() && c.id == e.id
}
fn is_poly(e: &Entity) -> bool {
    matches!(e.geo, Geo::Poly(_))
}
fn closed_poly(e: &Entity) -> bool {
    matches!(&e.geo, Geo::Poly(pl) if pl.is_closed())
}
fn pts_len(e: &Entity) -> usize {
    match &e.geo {
        Geo::Poly(pl) => pl.pts.len(),
        _ => 0,
    }
}
/// `[...new Set(list)]` for numbers (SameValueZero), first occurrence kept.
fn dedupe(list: Vec<f64>) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::with_capacity(list.len());
    for t in list {
        if !out.iter().any(|u| *u == t || (u.is_nan() && t.is_nan())) {
            out.push(t);
        }
    }
    out
}
fn sort_nums(v: &mut [f64]) {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
}

// ---------------------------------------------------------------------------------------------------------------
// Offset

/// An entity offset by d toward the side of `side`, or None.
pub fn offset_entity(e: &Entity, d: f64, side: P2) -> Option<Entity> {
    match &e.geo {
        Geo::Line(l) => {
            let len = js::hypot(l.x2 - l.x1, l.y2 - l.y1);
            if len < EPS {
                return None;
            }
            let mut nx = -(l.y2 - l.y1) / len;
            let mut ny = (l.x2 - l.x1) / len;
            if (side.x - l.x1) * nx + (side.y - l.y1) * ny < 0.0 {
                nx = -nx;
                ny = -ny;
            }
            Some(with_geo(e, line_ent(l.x1 + nx * d, l.y1 + ny * d, l.x2 + nx * d, l.y2 + ny * d)))
        }
        Geo::Circle(c) => {
            let out = dist(p(c.cx, c.cy), side) > c.r;
            let r = c.r + if out { d } else { -d };
            (r > EPS).then(|| with_geo(e, Geo::Circle(Circle { r, ..c.clone() })))
        }
        Geo::Arc(a) => {
            let out = dist(p(a.cx, a.cy), side) > a.r;
            let r = a.r + if out { d } else { -d };
            (r > EPS).then(|| with_geo(e, arc_ent(a.cx, a.cy, r, a.a0, a.a1)))
        }
        Geo::Poly(_) => offset_poly(e, d, side),
        Geo::Xline(i) | Geo::Ray(i) => {
            let mut nx = -i.dy;
            let mut ny = i.dx;
            if (side.x - i.x) * nx + (side.y - i.y) * ny < 0.0 {
                nx = -nx;
                ny = -ny;
            }
            let ni = Inf { x: i.x + nx * d, y: i.y + ny * d, ..i.clone() };
            Some(with_geo(e, if matches!(e.geo, Geo::Ray(_)) { Geo::Ray(ni) } else { Geo::Xline(ni) }))
        }
        _ => None,
    }
}

fn offset_poly(e: &Entity, d: f64, side: P2) -> Option<Entity> {
    let Geo::Poly(pl) = &e.geo else { return None };
    let closed = pl.is_closed();
    let ps = poly_prims(&pl.pts, closed);
    let mut near = *ps.first()?;
    let mut nd = f64::INFINITY;
    for q in &ps {
        let x = dist_to_prim(q, side);
        if x < nd {
            nd = x;
            near = *q;
        }
    }
    let left_of = |q: &Prim, x: P2| {
        if q.is_l() {
            return (q.x2 - q.x1) * (x.y - q.y1) - (q.y2 - q.y1) * (x.x - q.x1) > 0.0;
        }
        let outside = dist(p(q.cx, q.cy), x) > q.r;
        if q.rev() { outside } else { !outside }
    };
    let sgn = if left_of(&near, side) { 1.0 } else { -1.0 };
    let mut off = Vec::with_capacity(ps.len());
    for q in &ps {
        if q.is_l() {
            let l = js::or(Some(js::hypot(q.x2 - q.x1, q.y2 - q.y1)), 1.0);
            let nx = -(q.y2 - q.y1) / l * d * sgn;
            let ny = (q.x2 - q.x1) / l * d * sgn;
            off.push(Prim { x1: q.x1 + nx, y1: q.y1 + ny, x2: q.x2 + nx, y2: q.y2 + ny, ..*q });
        } else {
            let to_left_inside = !q.rev();
            let r = q.r + if (sgn > 0.0) == to_left_inside { -d } else { d };
            if !(r > EPS) {
                return None;
            }
            off.push(Prim { r, ..*q });
        }
    }
    let start = |q: &Prim| if q.is_l() { p(q.x1, q.y1) } else { prim_point(q, if q.rev() { 1.0 } else { 0.0 }) };
    let end = |q: &Prim| if q.is_l() { p(q.x2, q.y2) } else { prim_point(q, if q.rev() { 0.0 } else { 1.0 }) };
    let n = off.len();
    let mut pts = Vec::with_capacity(n + 1);
    for (i, q) in off.iter().enumerate() {
        let mut s = start(q);
        if (i > 0 || closed)
            && let Some(prev) = off.get((i + n - 1) % n)
            && prev.is_l()
            && q.is_l()
            && let Some(x) = intersect(prev, q, true, true).first()
        {
            s = *x;
        }
        pts.push(Vertex::new(s.x, s.y, pl.pts.get(i).map_or(0.0, Vertex::bulge)));
    }
    if !closed && let Some(last) = off.last() {
        let l = end(last);
        pts.push(Vertex::new(l.x, l.y, 0.0));
    }
    Some(with_geo(e, Geo::Poly(Poly { pts, closed: pl.closed })))
}

// ---------------------------------------------------------------------------------------------------------------
// Trim and extend

/// Parameter along the whole entity: poly → segment index + local t; circle/arc → fraction of sweep.
fn entity_params(e: &Entity, cutters: &[&Entity], blocks: Option<&Blocks>, edge_extend: bool) -> (Vec<Prim>, Vec<f64>) {
    let own = if is_poly(e) { poly_prims_of(e) } else { prims(e, None) };
    let poly = is_poly(e);
    let mut ts = Vec::new();
    for (i, q) in own.iter().enumerate() {
        for c in cutters {
            let ext = edge_extend && matches!(c.geo, Geo::Line(_) | Geo::Arc(_));
            for r in prims(c, blocks) {
                for x in intersect(q, &r, false, ext) {
                    let mut t = prim_param(q, x, true);
                    if q.rev() {
                        t = 1.0 - t;
                    }
                    ts.push(if poly { f(i) } else { 0.0 } + t);
                }
            }
        }
    }
    if e.unbounded() {
        sort_nums(&mut ts);
        let mut u = Vec::with_capacity(ts.len());
        for (i, t) in ts.iter().enumerate() {
            if i == 0 || ts.get(i - 1).is_some_and(|prev| t - prev > 1e-15) {
                u.push(*t);
            }
        }
        return (own, u);
    }
    let mut r = dedupe(ts.into_iter().map(|t| js::round(t * 1e9) / 1e9).collect());
    sort_nums(&mut r);
    (own, r)
}

fn pick_param(e: &Entity, own: &[Prim], q: P2) -> f64 {
    let poly = is_poly(e);
    let mut best = 0.0;
    let mut bd = f64::INFINITY;
    for (i, pr) in own.iter().enumerate() {
        let d = dist_to_prim(pr, q);
        if d < bd {
            bd = d;
            let mut t = prim_param(pr, q, true);
            if pr.rev() {
                t = 1.0 - t;
            }
            best = if poly { f(i) } else { 0.0 } + t;
        }
    }
    best
}

fn trimmable(e: &Entity) -> bool {
    matches!(e.geo, Geo::Line(_) | Geo::Arc(_) | Geo::Circle(_) | Geo::Poly(_) | Geo::Xline(_) | Geo::Ray(_))
}
fn max_t(e: &Entity) -> f64 {
    match &e.geo {
        Geo::Poly(pl) => {
            let n = f(pl.pts.len());
            if pl.is_closed() { n } else { n - 1.0 }
        }
        _ => 1.0,
    }
}
fn tiny(e: &Entity) -> f64 {
    if e.unbounded() { 1e-15 } else { 1e-9 }
}

/// Trim e at the cutting edges around the picked point: the pieces that stay (empty = all gone, None = nothing to cut).
pub fn trim_entity(e: &Entity, cutters: &[Entity], pick: P2, blocks: Option<&Blocks>) -> Option<Vec<Entity>> {
    if !trimmable(e) {
        return None;
    }
    let cs: Vec<&Entity> = cutters.iter().filter(|c| !same(c, e)).collect();
    let (own, ts) = entity_params(e, &cs, blocks, false);
    let tp = pick_param(e, &own, pick);
    if let Geo::Circle(c) = &e.geo {
        if ts.len() < 2 {
            return None;
        }
        let angs: Vec<f64> = ts.iter().map(|t| t * TAU).collect();
        let ap = tp * TAU;
        let (first, last) = (*angs.first()?, *angs.last()?);
        let mut lo = last - TAU;
        let mut hi = first;
        for w in angs.windows(2) {
            if let [a, b] = w
                && ap >= *a
                && ap <= *b
            {
                lo = *a;
                hi = *b;
            }
        }
        if ap > last {
            lo = last;
            hi = first + TAU;
        }
        return Some(vec![with_geo(e, arc_ent(c.cx, c.cy, c.r, norm_angle(hi), norm_angle(lo)))]);
    }
    let mt = max_t(e);
    let tn = tiny(e);
    let inner: Vec<f64> = ts.iter().copied().filter(|t| *t > tn && *t < mt - tn).collect();
    if closed_poly(e) {
        return trim_closed_poly(e, &ts, tp);
    }
    if inner.is_empty() {
        return None;
    }
    let mut lo = 0.0;
    let mut hi = mt;
    for t in &inner {
        if *t < tp {
            lo = *t;
        } else if *t > tp {
            hi = *t;
            break;
        }
    }
    let mut keep = Vec::new();
    if lo > tn {
        keep.extend(sub_entity(e, 0.0, lo));
    }
    if hi < mt - tn {
        keep.extend(sub_entity(e, hi, mt));
    }
    Some(keep)
}

/// A closed polyline as an open one walking the ring twice (and back to the start).
fn doubled(e: &Entity) -> Entity {
    let Geo::Poly(pl) = &e.geo else { return e.clone() };
    let mut pts = pl.pts.clone();
    pts.extend(pl.pts.iter().cloned());
    if let Some(f) = pl.pts.first() {
        pts.push(f.clone());
    }
    with_geo(e, Geo::Poly(Poly { pts, closed: Some(false) }))
}

fn trim_closed_poly(e: &Entity, ts: &[f64], tp: f64) -> Option<Vec<Entity>> {
    let n = f(pts_len(e));
    if ts.len() < 2 {
        return None;
    }
    let (first, last) = (*ts.first()?, *ts.last()?);
    let mut lo = last - n;
    let mut hi = first;
    for w in ts.windows(2) {
        if let [a, b] = w
            && tp >= *a
            && tp <= *b
        {
            lo = *a;
            hi = *b;
        }
    }
    if tp > last {
        lo = last;
        hi = first + n;
    }
    Some(sub_entity(&doubled(e), hi, lo + n).into_iter().collect())
}

/// float → index (floor/ceil results; None when negative or not finite).
fn ix(x: f64) -> Option<i64> {
    if x.is_finite() && x.abs() < 9e15 {
        // finite, integral after floor/ceil, inside i64
        #[allow(clippy::cast_possible_truncation)]
        let i = x as i64;
        Some(i)
    } else {
        None
    }
}

/// The piece of a line / arc / open poly (or construction line) between parameters t0 < t1.
pub fn sub_entity(e: &Entity, t0: f64, t1: f64) -> Option<Entity> {
    if t1 - t0 < tiny(e) {
        return None;
    }
    if let Geo::Xline(i) | Geo::Ray(i) = &e.geo {
        let pr = *prims(e, None).first()?;
        let at = |t: f64| prim_point(&pr, t);
        let open0 = matches!(e.geo, Geo::Xline(_)) && t0 <= 1e-12;
        let open1 = t1 >= 1.0 - 1e-12;
        if open0 && open1 {
            return Some(e.clone());
        }
        if open1 {
            let a = at(t0);
            return Some(with_geo(e, Geo::Ray(Inf { x: a.x, y: a.y, ..i.clone() })));
        }
        if open0 {
            let b = at(t1);
            return Some(with_geo(e, Geo::Ray(Inf { x: b.x, y: b.y, dx: -i.dx, dy: -i.dy })));
        }
        let (a, b) = (at(t0), at(t1));
        return Some(with_geo(e, line_ent(a.x, a.y, b.x, b.y)));
    }
    match &e.geo {
        Geo::Line(l) => {
            let pp = p(l.x1 + (l.x2 - l.x1) * t0, l.y1 + (l.y2 - l.y1) * t0);
            let q = p(l.x1 + (l.x2 - l.x1) * t1, l.y1 + (l.y2 - l.y1) * t1);
            Some(with_geo(e, line_ent(pp.x, pp.y, q.x, q.y)))
        }
        Geo::Arc(a) => {
            let (a0, a1) = arc_range(a.a0, a.a1);
            let s = a1 - a0;
            Some(with_geo(e, arc_ent(a.cx, a.cy, a.r, norm_angle(a0 + s * t0), norm_angle(a0 + s * t1))))
        }
        Geo::Poly(pl) => {
            let v = &pl.pts;
            let i0 = ix(t0.floor())?;
            let i1 = ix(t1.ceil())?.saturating_sub(1).min(i64::try_from(v.len()).unwrap_or(0) - 2);
            let at = |i: i64, t: f64| -> Option<P2> {
                let iu = usize::try_from(i).ok()?;
                let (a, b) = (v.get(iu)?, v.get(iu + 1)?);
                let bb = a.bulge();
                if bb.abs() < 1e-12 {
                    return Some(p(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t));
                }
                let arc = bulge_arc(a.p(), b.p(), bb);
                let u = if arc.rev() { 1.0 - t } else { t };
                Some(prim_point(&arc, u))
            };
            let part_bulge = |i: i64, ta: f64, tb: f64| {
                let b = usize::try_from(i).ok().and_then(|iu| v.get(iu)).map_or(0.0, Vertex::bulge);
                if b.abs() < 1e-12 { 0.0 } else { (b.jatan() * (tb - ta)).jtan() }
            };
            let mut pts = Vec::new();
            let mut i = i0;
            while i <= i1 {
                let fi = i as f64;
                let ta = if i == i0 { t0 - fi } else { 0.0 };
                let tb = if i == i1 { t1 - fi } else { 1.0 };
                let q = at(i, ta)?;
                pts.push(Vertex::new(q.x, q.y, part_bulge(i, ta, tb)));
                if i == i1 {
                    let q = at(i, tb)?;
                    pts.push(Vertex::new(q.x, q.y, 0.0));
                }
                i += 1;
            }
            Some(with_geo(e, Geo::Poly(Poly { pts, closed: Some(false) })))
        }
        _ => None,
    }
}

/// One piece of a trimSplit: [t0, t1] in entityParams' parameter; `whole` = the uncut object.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub t0: f64,
    pub t1: f64,
    pub whole: bool,
}
/// TRIM in pieces (0.4 T.1–T.3): e split at its cutting edges.
#[derive(Clone, Debug, PartialEq)]
pub struct Split {
    pub e: Entity,
    pub own: Vec<Prim>,
    pub pieces: Vec<Piece>,
    pub max_t: f64,
    pub closed: bool,
}
impl Split {
    pub fn to_value(&self) -> Value {
        serde_json::json!({
            "e": self.e.to_value(),
            "own": self.own.iter().map(Prim::to_value).collect::<Vec<_>>(),
            "pieces": self.pieces.iter().map(|pc| {
                let mut v = serde_json::json!({ "t0": cc_cad::num(pc.t0), "t1": cc_cad::num(pc.t1) });
                if pc.whole && let Some(o) = v.as_object_mut() { o.insert("whole".into(), true.into()); }
                v
            }).collect::<Vec<_>>(),
            "maxT": cc_cad::num(self.max_t),
            "closed": self.closed,
        })
    }
}

/// e split at its cutting edges → the pieces in order, or None for objects TRIM does not cut. `edge_extend`:
/// cutting lines and arcs extended (EDGEMODE 1).
pub fn trim_split(e: &Entity, cutters: &[Entity], blocks: Option<&Blocks>, edge_extend: bool) -> Option<Split> {
    if !trimmable(e) {
        return None;
    }
    if is_poly(e) && pts_len(e) < 2 {
        return None;
    }
    let cs: Vec<&Entity> = cutters.iter().filter(|c| !same(c, e) && !same_id(c, e)).collect();
    let (own, ts) = entity_params(e, &cs, blocks, edge_extend);
    let closed = matches!(e.geo, Geo::Circle(_)) || closed_poly(e);
    let mt = max_t(e);
    let mut pieces = Vec::new();
    if closed {
        let mut cuts = dedupe(
            ts.iter()
                .map(|t| {
                    let u = ((t % mt) + mt) % mt;
                    if (u - mt).abs() < 1e-9 { 0.0 } else { u }
                })
                .collect(),
        );
        sort_nums(&mut cuts);
        if cuts.len() < 2 {
            pieces.push(Piece { t0: 0.0, t1: mt, whole: true });
        } else {
            for w in cuts.windows(2) {
                if let [a, b] = w
                    && b - a > 1e-9
                {
                    pieces.push(Piece { t0: *a, t1: *b, whole: false });
                }
            }
            if let (Some(first), Some(last)) = (cuts.first(), cuts.last()) {
                pieces.push(Piece { t0: *last, t1: first + mt, whole: false });
            }
        }
    } else {
        let tn = tiny(e);
        let mut at = vec![0.0];
        at.extend(ts.iter().copied().filter(|t| *t > tn && *t < mt - tn));
        at.push(mt);
        for w in at.windows(2) {
            if let [a, b] = w
                && b - a > tn
            {
                pieces.push(Piece { t0: *a, t1: *b, whole: false });
            }
        }
    }
    Some(Split { e: e.clone(), own, pieces, max_t: mt, closed })
}

/// The index of the piece of a trimSplit that holds the point nearest q.
pub fn piece_at(split: &Split, q: P2) -> usize {
    let mut t = pick_param(&split.e, &split.own, q);
    if split.closed && split.pieces.len() > 1 && split.pieces.first().is_some_and(|p0| t < p0.t0) {
        t += split.max_t;
    }
    let mut best = 0;
    let mut bd = f64::INFINITY;
    for (i, pc) in split.pieces.iter().enumerate() {
        let d = if t < pc.t0 {
            pc.t0 - t
        } else if t > pc.t1 {
            t - pc.t1
        } else {
            0.0
        };
        if d < bd {
            bd = d;
            best = i;
        }
    }
    best
}

/// One piece of a trimSplit as an object of its own.
pub fn piece_entity(split: &Split, i: usize) -> Option<Entity> {
    let pc = split.pieces.get(i)?;
    if pc.whole {
        return Some(split.e.clone());
    }
    span_entity(split, pc.t0, pc.t1)
}

fn span_entity(split: &Split, t0: f64, t1: f64) -> Option<Entity> {
    let e = &split.e;
    if let Geo::Circle(c) = &e.geo {
        return Some(with_geo(e, arc_ent(c.cx, c.cy, c.r, norm_angle(t0 * TAU), norm_angle(t1 * TAU))));
    }
    if closed_poly(e) {
        return sub_entity(&doubled(e), t0, if t1 > t0 { t1 } else { t1 + split.max_t });
    }
    sub_entity(e, t0, t1)
}

/// What stays of a trimSplit when the pieces in `gone` are removed: kept runs joined back into one object each
/// (empty = nothing left), or None when nothing is removed.
pub fn trim_result(split: &Split, gone: &[usize]) -> Option<Vec<Entity>> {
    let g = |i: usize| gone.contains(&i);
    let pieces = &split.pieces;
    if !(0..pieces.len()).any(g) {
        return None;
    }
    let mut runs: Vec<(f64, f64)> = Vec::new();
    if split.closed {
        if pieces.len() == 1 {
            return Some(Vec::new());
        }
        let n = pieces.len();
        let start = (0..n).find(|i| g(*i)).unwrap_or(0);
        let mut run: Option<(f64, f64)> = None;
        for k in 1..=n {
            let i = (start + k) % n;
            let Some(pc) = pieces.get(i) else { continue };
            if g(i) {
                if let Some(r) = run.take() {
                    runs.push(r);
                }
                continue;
            }
            let t0 = run.map_or(pc.t0, |r| r.0);
            let mut t1 = pc.t1;
            let mut guard = 0;
            while t1 < t0 - 1e-12 && guard < 1000 {
                t1 += split.max_t;
                guard += 1;
            }
            run = Some((t0, t1));
        }
        if let Some(r) = run {
            runs.push(r);
        }
    } else {
        let mut run: Option<(f64, f64)> = None;
        for (i, pc) in pieces.iter().enumerate() {
            if g(i) {
                if let Some(r) = run.take() {
                    runs.push(r);
                }
                continue;
            }
            run = Some(run.map_or((pc.t0, pc.t1), |r| (r.0, pc.t1)));
        }
        if let Some(r) = run {
            runs.push(r);
        }
    }
    Some(runs.into_iter().filter_map(|(a, b)| span_entity(split, a, b)).collect())
}

/// Points where an open run of points (a freehand path or a fence) crosses e.
pub fn path_crossings(e: &Entity, pts: &[P2], blocks: Option<&Blocks>) -> Vec<P2> {
    let own = if is_poly(e) { poly_prims_of(e) } else { prims(e, blocks) };
    let mut out = Vec::new();
    for (a, b) in pts.iter().zip(pts.iter().skip(1)) {
        if dist(*a, *b) <= EPS {
            continue;
        }
        let s = Prim::l(a.x, a.y, b.x, b.y);
        for q in &own {
            out.extend(intersect(q, &s, false, false));
        }
    }
    out
}

/// The part an EXTEND adds (old → new), for the preview: a line, an arc or the moved end segment of a polyline.
pub fn extension_part(old: &Entity, nu: &Entity) -> Option<Entity> {
    match (&old.geo, &nu.geo) {
        (Geo::Line(o), Geo::Line(n)) => {
            let s = dist(p(o.x1, o.y1), p(n.x1, n.y1)) > EPS;
            Some(Entity::new(if s { line_ent(o.x1, o.y1, n.x1, n.y1) } else { line_ent(o.x2, o.y2, n.x2, n.y2) }))
        }
        (Geo::Arc(o), Geo::Arc(n)) => {
            let d = norm_angle(o.a0 - n.a0);
            if d.abs() > 1e-12 && (d - TAU).abs() > 1e-12 {
                return Some(with_geo(old, arc_ent(o.cx, o.cy, o.r, n.a0, o.a0)));
            }
            Some(with_geo(old, arc_ent(o.cx, o.cy, o.r, o.a1, n.a1)))
        }
        (Geo::Poly(o), Geo::Poly(n)) => {
            let len = o.pts.len();
            let (o0, n0) = (o.pts.first()?, n.pts.first()?);
            let i = if dist(o0.p(), n0.p()) > EPS { 0 } else { len.checked_sub(1)? };
            let (a, b) = (o.pts.get(i)?, n.pts.get(i)?);
            Some(Entity::new(line_ent(a.x, a.y, b.x, b.y)))
        }
        _ => None,
    }
}

/// Extend the end of e nearest `pick` to the closest boundary → the new entity, or None.
pub fn extend_entity(e: &Entity, boundaries: &[Entity], pick: P2, blocks: Option<&Blocks>, edge_extend: bool) -> Option<Entity> {
    let bps: Vec<(Prim, bool)> = boundaries
        .iter()
        .filter(|b| !same(b, e) && !same_id(b, e))
        .flat_map(|b| {
            let ext = edge_extend && matches!(b.geo, Geo::Line(_) | Geo::Arc(_));
            prims(b, blocks).into_iter().map(move |q| (q, ext))
        })
        .collect();
    match &e.geo {
        Geo::Line(l) => {
            let at_start = dist(pick, p(l.x1, l.y1)) < dist(pick, p(l.x2, l.y2));
            let (from, to) = if at_start { (p(l.x2, l.y2), p(l.x1, l.y1)) } else { (p(l.x1, l.y1), p(l.x2, l.y2)) };
            if dist(from, to) < EPS {
                return None;
            }
            let ray = Prim::l(from.x, from.y, to.x, to.y);
            let mut best: Option<(f64, P2)> = None;
            for (q, ext) in &bps {
                for x in intersect(&ray, q, true, *ext) {
                    let t = prim_param(&ray, x, false);
                    if t > 1.0 + 1e-9 && best.is_none_or(|b| t < b.0) {
                        best = Some((t, x));
                    }
                }
            }
            let (_, x) = best?;
            Some(with_geo(e, if at_start { line_ent(x.x, x.y, l.x2, l.y2) } else { line_ent(l.x1, l.y1, x.x, x.y) }))
        }
        Geo::Arc(a) => {
            let (a0, a1) = arc_range(a.a0, a.a1);
            let c = p(a.cx, a.cy);
            let at_start = dist(pick, polar(c, a0, a.r)) < dist(pick, polar(c, a1, a.r));
            let full = Prim::circle(a.cx, a.cy, a.r);
            let mut best: Option<(f64, f64)> = None;
            for (q, ext) in &bps {
                for x in intersect(&full, q, true, *ext) {
                    let ang = norm_angle((x.y - a.cy).jatan2(x.x - a.cx));
                    let gap = if at_start { norm_angle(a0 - ang) } else { norm_angle(ang - a1) };
                    if gap > 1e-9 && gap < TAU - (a1 - a0) && best.is_none_or(|b| gap < b.0) {
                        best = Some((gap, ang));
                    }
                }
            }
            let (_, ang) = best?;
            Some(with_geo(e, if at_start { arc_ent(a.cx, a.cy, a.r, ang, a.a1) } else { arc_ent(a.cx, a.cy, a.r, a.a0, ang) }))
        }
        Geo::Poly(pl) if !pl.is_closed() && pl.pts.len() >= 2 => {
            let n = pl.pts.len();
            let (first, last) = (pl.pts.first()?, pl.pts.last()?);
            let at_start = dist(pick, first.p()) < dist(pick, last.p());
            let (pp, q) = if at_start { (pl.pts.get(1)?, first) } else { (pl.pts.get(n - 2)?, last) };
            let b = if at_start { first.bulge() } else { pl.pts.get(n - 2)?.bulge() };
            if b.abs() > 1e-12 {
                return None;
            }
            let tmp = Entity::new(line_ent(pp.x, pp.y, q.x, q.y));
            let r = extend_entity(&tmp, boundaries, q.p(), blocks, edge_extend)?;
            let Geo::Line(rl) = &r.geo else { return None };
            let mut pts = pl.pts.clone();
            let k = if at_start { 0 } else { n - 1 };
            if let Some(v) = pts.get_mut(k) {
                v.x = rl.x2;
                v.y = rl.y2;
            }
            Some(with_geo(e, Geo::Poly(Poly { pts, closed: pl.closed })))
        }
        _ => None,
    }
}

/// FILLET of two lines.
#[derive(Clone, Debug, PartialEq)]
pub struct Fillet {
    pub a: Entity,
    pub b: Entity,
    pub arc: Option<Entity>,
}
impl Fillet {
    pub fn to_value(&self) -> Value {
        serde_json::json!({ "a": self.a.to_value(), "b": self.b.to_value(), "arc": self.arc.as_ref().map_or(Value::Null, Entity::to_value) })
    }
}

/// Fillet two lines with radius r (0 = sharp corner); the picks say which side of each line to keep.
pub fn fillet_lines(a: &Entity, b: &Entity, r: f64, pick_a: P2, pick_b: P2) -> Option<Fillet> {
    let (Geo::Line(la), Geo::Line(lb)) = (&a.geo, &b.geo) else { return None };
    let pa_ = Prim::l(la.x1, la.y1, la.x2, la.y2);
    let pb_ = Prim::l(lb.x1, lb.y1, lb.x2, lb.y2);
    let x = *intersect(&pa_, &pb_, true, true).first()?;
    let keep_dir = |l: &Prim, pick: P2| {
        let mut ends = [p(l.x1, l.y1), p(l.x2, l.y2)];
        if dist(ends[1], x) - dist(ends[0], x) > 0.0 {
            ends.swap(0, 1);
        }
        let far = ends[0];
        let tp = prim_param(&Prim::l(x.x, x.y, far.x, far.y), pick, false);
        let target = if tp >= 0.0 { far } else { ends[1] };
        let d = js::or(Some(dist(x, target)), 1.0);
        ((target.x - x.x) / d, (target.y - x.y) / d, target)
    };
    let (aux, auy, afar) = keep_dir(&pa_, pick_a);
    let (bux, buy, bfar) = keep_dir(&pb_, pick_b);
    if r <= EPS {
        return Some(Fillet {
            a: with_geo(a, line_ent(x.x, x.y, afar.x, afar.y)),
            b: with_geo(b, line_ent(x.x, x.y, bfar.x, bfar.y)),
            arc: None,
        });
    }
    let half = js::max(-1.0, js::min(1.0, aux * bux + auy * buy)).jacos() / 2.0;
    if half < 1e-6 || half > PI / 2.0 - 1e-6 {
        return None;
    }
    let t = r / half.jtan();
    let pa = p(x.x + aux * t, x.y + auy * t);
    let pb = p(x.x + bux * t, x.y + buy * t);
    let bx = aux + bux;
    let by = auy + buy;
    let bl = js::hypot(bx, by);
    let c = p(x.x + bx / bl * (r / half.jsin()), x.y + by / bl * (r / half.jsin()));
    let mut a0 = (pa.y - c.y).jatan2(pa.x - c.x);
    let mut a1 = (pb.y - c.y).jatan2(pb.x - c.x);
    if norm_angle(a1 - a0) > PI {
        std::mem::swap(&mut a0, &mut a1);
    }
    let arc = Entity::with(arc_ent(c.x, c.y, r, norm_angle(a0), norm_angle(a1)), a.layer.clone(), Some(Color::or_null(&a.color)));
    Some(Fillet {
        a: with_geo(a, line_ent(pa.x, pa.y, afar.x, afar.y)),
        b: with_geo(b, line_ent(pb.x, pb.y, bfar.x, bfar.y)),
        arc: Some(arc),
    })
}

// ---------------------------------------------------------------------------------------------------------------
// Revision clouds

/// An object's outline as points (arcs and curves in short pieces): (pts, closed), or None.
pub fn path_of(e: &Entity) -> Option<(Vec<P2>, bool)> {
    match &e.geo {
        Geo::Circle(c) => {
            let n = 96;
            Some(((0..n).map(|i| polar(p(c.cx, c.cy), (f(i) / f(n)) * TAU, c.r)).collect(), true))
        }
        Geo::Ellipse(el) => {
            let mut pts = ellipse_points(e, 144.0);
            let closed = el.t1 - el.t0 >= TAU - 1e-9;
            if closed {
                pts.pop();
            }
            Some((pts, closed))
        }
        Geo::Line(l) => Some((vec![p(l.x1, l.y1), p(l.x2, l.y2)], false)),
        Geo::Arc(_) => {
            let pr = *prims(e, None).first()?;
            let n = steps(8.0, (pr.a1 - pr.a0) / (TAU / 96.0));
            Some(((0..=n).map(|i| prim_point(&pr, f(i) / f(n))).collect(), false))
        }
        Geo::Poly(pl) if pl.pts.len() >= 2 => {
            let mut pts = Vec::new();
            for pr in poly_prims(&pl.pts, pl.is_closed()) {
                if pr.is_l() {
                    pts.push(p(pr.x1, pr.y1));
                    continue;
                }
                let n = steps(4.0, (pr.a1 - pr.a0) / (TAU / 96.0));
                for i in 0..n {
                    let t = f(i) / f(n);
                    pts.push(prim_point(&pr, if pr.rev() { 1.0 - t } else { t }));
                }
            }
            if !pl.is_closed()
                && let Some(l) = pl.pts.last()
            {
                pts.push(l.p());
            }
            Some((pts, pl.is_closed()))
        }
        _ => None,
    }
}

/// Points spaced evenly along a path (about `step` apart).
pub fn resample_path(pts: &[P2], closed: bool, step: f64) -> Vec<P2> {
    let mut ring = pts.to_vec();
    if closed {
        let Some(f0) = pts.first() else { return Vec::new() };
        ring.push(*f0);
    }
    let lens: Vec<f64> = ring.iter().zip(ring.iter().skip(1)).map(|(a, b)| dist(*a, *b)).collect();
    let total = lens.iter().fold(0.0, |s, d| s + d);
    if !(total > EPS) || !(step > EPS) {
        return pts.to_vec();
    }
    let nf = js::max(if closed { 3.0 } else { 1.0 }, js::round(total / step));
    let n = steps(0.0, nf);
    let mut out = Vec::new();
    let mut seg = 0usize;
    let mut acc = 0.0;
    for k in 0..=n {
        if closed && k == n {
            break;
        }
        let target = (total * f(k)) / f(n);
        while seg + 1 < lens.len() && acc + lens.get(seg).copied().unwrap_or(0.0) < target - 1e-12 {
            acc += lens.get(seg).copied().unwrap_or(0.0);
            seg += 1;
        }
        let ls = lens.get(seg).copied().unwrap_or(0.0);
        let t = if ls > 0.0 { (target - acc) / ls } else { 0.0 };
        let (Some(a), Some(b)) = (ring.get(seg), ring.get(seg + 1)) else { break };
        out.push(p(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t));
    }
    out
}

/// The arc shape of a revision cloud: bulge of each scallop (an included angle of 120°).
pub fn cloud_bulge() -> f64 {
    (TAU / 3.0 / 4.0).jtan()
}

/// A revision cloud along a path: a polyline of arcs about `arc_len` long that bulge outward (inward with
/// `reverse`) → `{ type: 'poly', closed, pts }`.
pub fn revcloud(pts: &[P2], closed: bool, arc_len: f64, reverse: bool) -> Entity {
    let q = resample_path(pts, closed, arc_len);
    let n = q.len();
    let mut a = 0.0;
    for i in 0..n {
        let (Some(u), Some(v)) = (q.get(i), q.get((i + 1) % n)) else { continue };
        a += u.x * v.y - v.x * u.y;
    }
    let sign = (if closed { if a >= 0.0 { 1.0 } else { -1.0 } } else { 1.0 }) * if reverse { -1.0 } else { 1.0 };
    let cb = cloud_bulge();
    let vs = q.iter().enumerate().map(|(i, pt)| Vertex::new(pt.x, pt.y, if closed || i + 1 < n { sign * cb } else { 0.0 })).collect();
    Entity::new(Geo::Poly(Poly { pts: vs, closed: Some(closed) }))
}

// ---------------------------------------------------------------------------------------------------------------
// BREAK and LENGTHEN

/// Where q is along e: line/arc → 0…1, polyline → segment index + 0…1.
pub fn param_on(e: &Entity, q: P2) -> f64 {
    let own = if is_poly(e) { poly_prims_of(e) } else { prims(e, None) };
    pick_param(e, &own, q)
}

/// BREAK: e without the part between p1 and p2 → the pieces, or Err('type' | 'circle').
pub fn break_entity(e: &Entity, p1: P2, p2: P2) -> Result<Vec<Entity>, &'static str> {
    let same_pt = dist(p1, p2) <= EPS * js::max(1.0, js::hypot(p1.x, p1.y));
    if let Geo::Circle(c) = &e.geo {
        if same_pt {
            return Err("circle");
        }
        let a1 = (p1.y - c.cy).jatan2(p1.x - c.cx);
        let a2 = (p2.y - c.cy).jatan2(p2.x - c.cx);
        return Ok(vec![with_geo(e, arc_ent(c.cx, c.cy, c.r, norm_angle(a2), norm_angle(a1)))]);
    }
    if !matches!(e.geo, Geo::Line(_) | Geo::Arc(_) | Geo::Poly(_) | Geo::Xline(_) | Geo::Ray(_)) {
        return Err("type");
    }
    let mut t1 = param_on(e, p1);
    let mut t2 = if same_pt { t1 } else { param_on(e, p2) };
    if closed_poly(e) {
        let n = f(pts_len(e));
        let open = doubled(e);
        if same_pt {
            return Ok(sub_entity(&open, t1, t1 + n).into_iter().collect());
        }
        if t2 < t1 {
            t2 += n;
        }
        return Ok(sub_entity(&open, t2, t1 + n).into_iter().collect());
    }
    if t2 < t1 {
        std::mem::swap(&mut t1, &mut t2);
    }
    let mt = if is_poly(e) { f(pts_len(e)) - 1.0 } else { 1.0 };
    let tn = tiny(e);
    let mut out = Vec::new();
    if t1 > tn || matches!(e.geo, Geo::Xline(_)) {
        out.extend(sub_entity(e, 0.0, t1));
    }
    if t2 < mt - tn {
        out.extend(sub_entity(e, t2, mt));
    }
    Ok(out)
}

/// Length of a line, arc or polyline (None for others).
pub fn curve_length(e: &Entity) -> Option<f64> {
    match &e.geo {
        Geo::Line(l) => Some(js::hypot(l.x2 - l.x1, l.y2 - l.y1)),
        Geo::Arc(a) => Some(a.r * js::or(Some(norm_angle(a.a1 - a.a0)), TAU)),
        Geo::Poly(_) => Some(poly_prims_of(e).iter().fold(0.0, |s, q| s + prim_length(q))),
        _ => None,
    }
}

/// LENGTHEN: e with the end nearest `pick` moved so it gets longer or shorter by `delta`, or (an arc) its included
/// angle by `d_angle` → the entity, or Err('type' | 'short' | 'full').
pub fn lengthen_entity(e: &Entity, pick: P2, delta: f64, d_angle: Option<f64>) -> Result<Entity, &'static str> {
    match &e.geo {
        Geo::Line(l) => {
            let len = js::hypot(l.x2 - l.x1, l.y2 - l.y1);
            let nl = len + delta;
            if !(nl > EPS) {
                return Err("short");
            }
            let at_start = dist(pick, p(l.x1, l.y1)) < dist(pick, p(l.x2, l.y2));
            let ux = (l.x2 - l.x1) / len;
            let uy = (l.y2 - l.y1) / len;
            Ok(with_geo(
                e,
                if at_start {
                    line_ent(l.x2 - ux * nl, l.y2 - uy * nl, l.x2, l.y2)
                } else {
                    line_ent(l.x1, l.y1, l.x1 + ux * nl, l.y1 + uy * nl)
                },
            ))
        }
        Geo::Arc(a) => {
            let sw = js::or(Some(norm_angle(a.a1 - a.a0)), TAU);
            let ns = sw + d_angle.unwrap_or(delta / a.r);
            if !(ns > 1e-9) {
                return Err("short");
            }
            if ns >= TAU - 1e-9 {
                return Err("full");
            }
            let c = p(a.cx, a.cy);
            let at_start = dist(pick, polar(c, a.a0, a.r)) < dist(pick, polar(c, a.a1, a.r));
            Ok(with_geo(
                e,
                if at_start {
                    arc_ent(a.cx, a.cy, a.r, norm_angle(a.a1 - ns), a.a1)
                } else {
                    arc_ent(a.cx, a.cy, a.r, a.a0, norm_angle(a.a0 + ns))
                },
            ))
        }
        Geo::Poly(pl) if !pl.is_closed() && pl.pts.len() >= 2 => {
            let n = pl.pts.len();
            let mut pts = pl.pts.clone();
            let (Some(first), Some(last)) = (pts.first(), pts.last()) else { return Err("type") };
            let at_start = dist(pick, first.p()) < dist(pick, last.p());
            let i = if at_start { 0 } else { n - 2 };
            let (Some(pp), Some(q)) = (pts.get(i).cloned(), pts.get(i + 1).cloned()) else { return Err("type") };
            let b = pp.bulge();
            if b.abs() < 1e-12 {
                let len = dist(pp.p(), q.p());
                let nl = len + delta;
                if !(nl > EPS) {
                    return Err("short");
                }
                let ux = (q.x - pp.x) / len;
                let uy = (q.y - pp.y) / len;
                if at_start {
                    set_xy(&mut pts, 0, q.x - ux * nl, q.y - uy * nl);
                } else {
                    set_xy(&mut pts, n - 1, pp.x + ux * nl, pp.y + uy * nl);
                }
                return Ok(with_geo(e, Geo::Poly(Poly { pts, closed: pl.closed })));
            }
            let a = bulge_arc(pp.p(), q.p(), b);
            let sw = a.a1 - a.a0;
            let ns = sw + d_angle.unwrap_or(delta / a.r);
            if !(ns > 1e-9) {
                return Err("short");
            }
            if ns >= TAU - 1e-9 {
                return Err("full");
            }
            let c = p(a.cx, a.cy);
            let dir = if b > 0.0 { 1.0 } else { -1.0 };
            if at_start {
                let aq = (q.y - c.y).jatan2(q.x - c.x);
                let np = polar(c, aq - dir * ns, a.r);
                set_xy(&mut pts, 0, np.x, np.y);
            } else {
                let ap = (pp.y - c.y).jatan2(pp.x - c.x);
                let nq = polar(c, ap + dir * ns, a.r);
                set_xy(&mut pts, n - 1, nq.x, nq.y);
            }
            if let Some(v) = pts.get_mut(i) {
                v.b = Some(dir * (ns / 4.0).jtan());
            }
            Ok(with_geo(e, Geo::Poly(Poly { pts, closed: pl.closed })))
        }
        _ => Err("type"),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// JOIN

/// One JOIN result: `to` replaces the objects in `from`.
#[derive(Clone, Debug, PartialEq)]
pub struct Joined {
    pub from: Vec<Option<u64>>,
    pub to: Entity,
}

fn piece_verts(e: &Entity) -> Option<Vec<Vertex>> {
    match &e.geo {
        Geo::Line(l) => Some(vec![Vertex::new(l.x1, l.y1, 0.0), Vertex::new(l.x2, l.y2, 0.0)]),
        Geo::Arc(a) => {
            let c = p(a.cx, a.cy);
            let sw = js::or(Some(norm_angle(a.a1 - a.a0)), TAU);
            let s = polar(c, a.a0, a.r);
            let t = polar(c, a.a1, a.r);
            Some(vec![Vertex::new(s.x, s.y, (sw / 4.0).jtan()), Vertex::new(t.x, t.y, 0.0)])
        }
        Geo::Poly(pl) if !pl.is_closed() && pl.pts.len() >= 2 => Some(pl.pts.iter().map(|v| Vertex::new(v.x, v.y, v.bulge())).collect()),
        _ => None,
    }
}
fn reverse_verts(v: &[Vertex]) -> Vec<Vertex> {
    let prev = std::iter::once(None).chain(v.iter().map(Some));
    let mut out: Vec<Vertex> = v.iter().zip(prev).map(|(q, pb)| Vertex::new(q.x, q.y, pb.map_or(0.0, |b| -b.bulge()))).collect();
    out.reverse();
    out
}

/// Join objects → (results, left ids). Lines on one line join into one line; arcs on one circle into one arc (or
/// the circle); otherwise pieces meeting end to end become polylines.
pub fn join_entities(ents: &[Entity], tol: Option<f64>) -> (Vec<Joined>, Vec<Option<u64>>) {
    let joinable = |e: &Entity| matches!(e.geo, Geo::Line(_) | Geo::Arc(_) | Geo::Poly(_)) && !closed_poly(e);
    let list: Vec<&Entity> = ents.iter().filter(|e| joinable(e)).collect();
    let mut left: Vec<Option<u64>> = ents.iter().filter(|e| !joinable(e)).map(|e| e.id).collect();
    if list.len() < 2 {
        return (Vec::new(), ents.iter().map(|e| e.id).collect());
    }
    let mut b = empty_bounds();
    for e in &list {
        for q in prims(e, None) {
            prim_bounds(&q, &mut b);
        }
    }
    let t = tol.unwrap_or_else(|| js::max(js::max(b.max_x - b.min_x, b.max_y - b.min_y), 1.0) * 1e-9);
    let from_all = || list.iter().map(|e| e.id).collect::<Vec<_>>();
    // all lines on one line → one line from the extremes
    if list.iter().all(|e| matches!(e.geo, Geo::Line(_)))
        && let Some(a0) = list.first()
        && let Geo::Line(a) = &a0.geo
    {
        let (ux, uy) = (a.x2 - a.x1, a.y2 - a.y1);
        let l = js::hypot(ux, uy);
        if l > EPS {
            let on = |x: f64, y: f64| ((x - a.x1) * uy - (y - a.y1) * ux).abs() / l <= t * 1e3;
            let lines: Vec<&cc_cad::entity::Line> =
                list.iter().filter_map(|e| if let Geo::Line(x) = &e.geo { Some(x) } else { None }).collect();
            if lines.iter().all(|e| on(e.x1, e.y1) && on(e.x2, e.y2)) {
                let all: Vec<f64> = lines
                    .iter()
                    .flat_map(|e| {
                        [((e.x1 - a.x1) * ux + (e.y1 - a.y1) * uy) / (l * l), ((e.x2 - a.x1) * ux + (e.y2 - a.y1) * uy) / (l * l)]
                    })
                    .collect();
                let t0 = js::min_of(all.iter().copied());
                let t1 = js::max_of(all.iter().copied());
                let to = with_geo(a0, line_ent(a.x1 + ux * t0, a.y1 + uy * t0, a.x1 + ux * t1, a.y1 + uy * t1));
                return (vec![Joined { from: from_all(), to }], left);
            }
        }
    }
    // all arcs on one circle → one arc counter-clockwise from the first, or the circle
    if list.iter().all(|e| matches!(e.geo, Geo::Arc(_)))
        && let Some(a0) = list.first()
        && let Geo::Arc(a) = &a0.geo
    {
        let arcs: Vec<&cc_cad::entity::Arc> = list.iter().filter_map(|e| if let Geo::Arc(x) = &e.geo { Some(x) } else { None }).collect();
        if arcs.iter().all(|e| js::hypot(e.cx - a.cx, e.cy - a.cy) <= t * 1e3 && (e.r - a.r).abs() <= t * 1e3) {
            let mut spans: Vec<(f64, f64)> = arcs.iter().map(|e| arc_range(e.a0, e.a1)).collect();
            spans.sort_by(|x, y| (x.0 - y.0).partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal));
            let mut merged: Vec<(f64, f64)> = Vec::new();
            for (s0, s1) in spans {
                match merged.last_mut() {
                    Some(m) if s0 <= m.1 + 1e-9 => m.1 = js::max(m.1, s1),
                    _ => merged.push((s0, s1)),
                }
            }
            if merged.len() > 1
                && let (Some(&last), Some(&first)) = (merged.last(), merged.first())
                && last.1 - TAU >= first.0 - 1e-9
            {
                if let Some(m0) = merged.first_mut() {
                    *m0 = (last.0 - TAU, js::max(first.1, last.1 - TAU));
                }
                merged.pop();
            }
            let whole = merged.len() == 1 && merged.first().is_some_and(|m| m.1 - m.0 >= TAU - 1e-9);
            if whole {
                let to =
                    Entity::with(Geo::Circle(Circle { cx: a.cx, cy: a.cy, r: a.r }), a0.layer.clone(), Some(Color::or_null(&a0.color)));
                return (vec![Joined { from: from_all(), to }], left);
            }
            let n = merged.len();
            let mut best: Option<(f64, f64, f64)> = None;
            for (k, m) in merged.iter().enumerate() {
                let Some(&nx) = merged.get((k + 1) % n) else { continue };
                let gap = (if k + 1 < n { nx.0 } else { nx.0 + TAU }) - m.1;
                if best.is_none_or(|b| gap > b.0) {
                    best = Some((gap, nx.0, m.1));
                }
            }
            if let Some((_, b0, b1)) = best {
                let to = with_geo(a0, arc_ent(a.cx, a.cy, a.r, norm_angle(b0), norm_angle(b1)));
                return (vec![Joined { from: from_all(), to }], left);
            }
        }
    }
    // chains of pieces meeting end to end
    let pieces: Vec<(&Entity, Vec<Vertex>)> = list.iter().filter_map(|e| piece_verts(e).map(|v| (*e, v))).collect();
    let near = |a: &Vertex, b: &Vertex| (a.x - b.x).abs() <= t * 1e3 && (a.y - b.y).abs() <= t * 1e3;
    let mut used = vec![false; pieces.len()];
    let mut results = Vec::new();
    for (si, (start_e, start_v)) in pieces.iter().enumerate() {
        if used.get(si).copied().unwrap_or(true) {
            continue;
        }
        if let Some(u) = used.get_mut(si) {
            *u = true;
        }
        let mut chain: Vec<Vertex> = start_v.clone();
        let mut from = vec![start_e.id];
        let mut grew = true;
        while grew {
            grew = false;
            for (pi, (pe_ent, pv)) in pieces.iter().enumerate() {
                if used.get(pi).copied().unwrap_or(true) {
                    continue;
                }
                let (Some(head), Some(tail), Some(ps), Some(pe)) = (chain.first(), chain.last(), pv.first(), pv.last()) else { continue };
                let (add, at_end) = if near(tail, ps) {
                    (pv.clone(), true)
                } else if near(tail, pe) {
                    (reverse_verts(pv), true)
                } else if near(head, pe) {
                    (pv.clone(), false)
                } else if near(head, ps) {
                    (reverse_verts(pv), false)
                } else {
                    continue;
                };
                if let Some(u) = used.get_mut(pi) {
                    *u = true;
                }
                from.push(pe_ent.id);
                grew = true;
                if at_end {
                    chain.pop();
                    chain.extend(add);
                } else {
                    let mut nc: Vec<Vertex> = add.get(..add.len().saturating_sub(1)).unwrap_or(&[]).to_vec();
                    nc.extend(chain);
                    chain = nc;
                }
            }
        }
        if from.len() < 2 {
            left.push(start_e.id);
            continue;
        }
        let closed = chain.len() > 2 && matches!((chain.first(), chain.last()), (Some(a), Some(b)) if near(a, b));
        if closed {
            chain.pop();
        }
        let pts = chain.iter().map(|q| Vertex::new(q.x, q.y, q.bulge())).collect();
        let to = Entity::with(Geo::Poly(Poly { pts, closed: Some(closed) }), start_e.layer.clone(), Some(Color::or_null(&start_e.color)));
        results.push(Joined { from, to });
    }
    (results, left)
}

// ---------------------------------------------------------------------------------------------------------------
// STRETCH

/// The bulge that keeps an arc's height (sagitta) when its chord changes from c0 to c1.
fn keep_sagitta(b: f64, c0: f64, c1: f64) -> f64 {
    if !(b.abs() > 1e-12) || !(c1 > EPS) {
        return b;
    }
    (b * c0 / 2.0) * 2.0 / c1
}

/// e stretched by (dx, dy): what `inside(point)` says is in the window moves → a new entity, or None when nothing
/// in it moves.
pub fn stretch_entity(e: &Entity, inside: &dyn Fn(P2) -> bool, dx: f64, dy: f64) -> Option<Entity> {
    let mv = |q: P2| p(q.x + dx, q.y + dy);
    let tr = m_translate(dx, dy);
    match &e.geo {
        Geo::Xline(i) | Geo::Ray(i) => {
            if !inside(p(i.x, i.y)) {
                return None;
            }
            let ni = Inf { x: i.x + dx, y: i.y + dy, ..i.clone() };
            Some(with_geo(e, if matches!(e.geo, Geo::Ray(_)) { Geo::Ray(ni) } else { Geo::Xline(ni) }))
        }
        Geo::Line(l) => {
            let a = inside(p(l.x1, l.y1));
            let b = inside(p(l.x2, l.y2));
            if !a && !b {
                return None;
            }
            let (x1, y1) = if a { (l.x1 + dx, l.y1 + dy) } else { (l.x1, l.y1) };
            let (x2, y2) = if b { (l.x2 + dx, l.y2 + dy) } else { (l.x2, l.y2) };
            Some(with_geo(e, line_ent(x1, y1, x2, y2)))
        }
        Geo::Poly(pl) => {
            let n = pl.pts.len();
            let hit: Vec<bool> = pl.pts.iter().map(|v| inside(v.p())).collect();
            if !hit.iter().any(|h| *h) {
                return None;
            }
            let mut pts: Vec<Vertex> = pl
                .pts
                .iter()
                .zip(&hit)
                .map(|(v, h)| {
                    let mut c = v.clone();
                    if *h {
                        let q = mv(v.p());
                        c.x = q.x;
                        c.y = q.y;
                    }
                    c
                })
                .collect();
            let segs = if pl.is_closed() { n } else { n.saturating_sub(1) };
            for i in 0..segs {
                let j = (i + 1) % n;
                let (Some(hi), Some(hj), Some(vi), Some(vj), Some(ni), Some(nj)) =
                    (hit.get(i), hit.get(j), pl.pts.get(i), pl.pts.get(j), pts.get(i), pts.get(j))
                else {
                    continue;
                };
                if hi != hj && vi.bulge().abs() > 1e-12 {
                    let nb = keep_sagitta(vi.b.unwrap_or(f64::NAN), dist(vi.p(), vj.p()), dist(ni.p(), nj.p()));
                    if let Some(x) = pts.get_mut(i) {
                        x.b = Some(nb);
                    }
                }
            }
            Some(with_geo(e, Geo::Poly(Poly { pts, closed: pl.closed })))
        }
        Geo::Arc(a) => {
            let c = p(a.cx, a.cy);
            let s = polar(c, a.a0, a.r);
            let t = polar(c, a.a1, a.r);
            let (ia, ib) = (inside(s), inside(t));
            if !ia && !ib {
                return inside(c).then(|| tx_entity(e, &tr));
            }
            if ia && ib {
                return Some(tx_entity(e, &tr));
            }
            let sweep = js::or(Some(norm_angle(a.a1 - a.a0)), TAU);
            let b0 = (sweep / 4.0).jtan();
            let s1 = if ia { mv(s) } else { s };
            let t1 = if ib { mv(t) } else { t };
            let nb = keep_sagitta(b0, dist(s, t), dist(s1, t1));
            if dist(s1, t1) <= EPS {
                return None;
            }
            let g = bulge_arc(s1, t1, nb);
            Some(with_geo(e, arc_ent(g.cx, g.cy, g.r, norm_angle(g.a0), norm_angle(g.a1))))
        }
        Geo::Circle(c) => inside(p(c.cx, c.cy)).then(|| tx_entity(e, &tr)),
        Geo::Ellipse(c) => inside(p(c.cx, c.cy)).then(|| tx_entity(e, &tr)),
        Geo::Text(t) => inside(p(t.x, t.y)).then(|| tx_entity(e, &tr)),
        Geo::Point(t) => inside(p(t.x, t.y)).then(|| tx_entity(e, &tr)),
        Geo::Insert(t) => inside(p(t.x, t.y)).then(|| tx_entity(e, &tr)),
        Geo::Mleader(l) => {
            let hit: Vec<bool> = l.pts.iter().map(|q| inside(*q)).collect();
            if !hit.iter().any(|h| *h) {
                return None;
            }
            let mut nl = l.clone();
            nl.pts = l.pts.iter().zip(&hit).map(|(q, h)| if *h { mv(*q) } else { *q }).collect();
            Some(with_geo(e, Geo::Mleader(nl)))
        }
        Geo::Dim(d) => {
            let keys: Vec<&str> = ["p1", "p2", "c", "loc"].into_iter().filter(|k| d.point(k).is_some_and(inside)).collect();
            if keys.is_empty() {
                return None;
            }
            let mut nd = d.clone();
            nd.block = None;
            for k in keys {
                if let Some(q) = d.point(k) {
                    nd.set_point(k, mv(q));
                }
            }
            Some(with_geo(e, Geo::Dim(nd)))
        }
        Geo::Hatch(h) => {
            let mut any = false;
            let loops = h
                .loops
                .iter()
                .map(|l| {
                    let mut o = l.clone();
                    for c in o.chunks_exact_mut(2) {
                        if let [x, y] = c
                            && inside(p(*x, *y))
                        {
                            *x += dx;
                            *y += dy;
                            any = true;
                        }
                    }
                    o
                })
                .collect();
            any.then(|| with_geo(e, Geo::Hatch(cc_cad::entity::Hatch { loops, solid: h.solid })))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------------------------
// AREA

/// Signed area of a closed run of points (counter-clockwise positive) and its perimeter, closing segment included.
pub fn polygon_measure(pts: &[P2]) -> (f64, f64) {
    let n = pts.len();
    let mut a = 0.0;
    let mut per = 0.0;
    for i in 0..n {
        let (Some(&u), Some(&v)) = (pts.get(i), pts.get((i + 1) % n)) else { continue };
        a += u.x * v.y - v.x * u.y;
        per += dist(u, v);
    }
    (a / 2.0, if n >= 2 { per } else { 0.0 })
}

fn poly_measure(v: &[Vertex], closed: bool) -> (f64, f64) {
    let n = v.len();
    let mut a = 0.0;
    let mut len = 0.0;
    for i in 0..n {
        let (Some(u), Some(w)) = (v.get(i), v.get((i + 1) % n)) else { continue };
        a += u.x * w.y - w.x * u.y;
        let last = i == n - 1;
        let b = if last && !closed { 0.0 } else { u.bulge() };
        let c = dist(u.p(), w.p());
        if b.abs() > 1e-12 && c > EPS {
            let th = 4.0 * b.abs().jatan();
            let r = c / (2.0 * (th / 2.0).jsin());
            a += js::sign(b) * r * r * (th - th.jsin());
            if !last || closed {
                len += r * th;
            }
        } else if !last || closed {
            len += c;
        }
    }
    ((a / 2.0).abs(), len)
}

/// Area and perimeter of an object: (area, perimeter, kind 'circle' | 'closed' | 'open'), or None.
pub fn entity_area(e: &Entity) -> Option<(f64, f64, &'static str)> {
    match &e.geo {
        Geo::Circle(c) => Some((PI * c.r * c.r, TAU * c.r, "circle")),
        Geo::Ellipse(el) => {
            let a = js::hypot(el.mx, el.my);
            let b = a * el.ratio;
            if el.t1 - el.t0 >= TAU - EPS || norm_angle(el.t1 - el.t0).abs() < EPS {
                let h = ((a - b) / (a + b)).powi(2);
                return Some((PI * a * b, PI * (a + b) * (1.0 + 3.0 * h / (10.0 + (4.0 - 3.0 * h).sqrt())), "closed"));
            }
            let pts = ellipse_points(e, 720.0);
            let (area, _) = polygon_measure(&pts);
            let len = pts.iter().zip(pts.iter().skip(1)).fold(0.0, |s, (u, v)| s + dist(*u, *v));
            Some((area.abs(), len, "open"))
        }
        Geo::Poly(pl) => {
            if pl.pts.len() < 2 {
                return None;
            }
            let (area, len) = poly_measure(&pl.pts, pl.is_closed());
            Some((area, len, if pl.is_closed() { "closed" } else { "open" }))
        }
        Geo::Hatch(h) => {
            let rings: Vec<Vec<P2>> = h.loops.iter().map(|l| crate::pick::flat_points(l)).filter(|r| r.len() >= 3).collect();
            if rings.is_empty() {
                return None;
            }
            let mut area = 0.0;
            let mut per = 0.0;
            for (i, r) in rings.iter().enumerate() {
                let (ma, mp) = polygon_measure(r);
                let depth = rings.iter().enumerate().filter(|(j, o)| *j != i && r.first().is_some_and(|r0| point_in_poly(*r0, o))).count();
                area += (if depth % 2 == 1 { -1.0 } else { 1.0 }) * ma.abs();
                per += mp;
            }
            Some((area.abs(), per, "closed"))
        }
        _ => None,
    }
}
