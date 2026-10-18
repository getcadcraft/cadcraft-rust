//! model-rooms.js — rooms and the building's outline found from the walls' joined outlines: every edge split where
//! others cross or touch it, the pieces with a wall on one side only walked into loops (anticlockwise: a room;
//! clockwise: a building's outside or an island in a room).

use std::collections::HashMap;

use crate::draw::m::{self, V2, at, cross, dot, nx, pv, sub};

pub use crate::draw::m::{area, in_poly, perim};

const EPS: f64 = 1e-6;

/// A room: its loop, holes (free-standing walls inside), area, perimeter, centroid.
#[derive(Clone, Debug, PartialEq)]
pub struct Room {
    pub poly: Vec<V2>,
    pub holes: Vec<Vec<V2>>,
    pub area: f64,
    pub perimeter: f64,
    pub centroid: V2,
}

/// A building outline (anticlockwise outer faces).
#[derive(Clone, Debug, PartialEq)]
pub struct Outline {
    pub poly: Vec<V2>,
    pub area: f64,
    pub perimeter: f64,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Loops {
    pub rooms: Vec<Room>,
    pub outlines: Vec<Outline>,
}

/// A loop `loopAt` can pick: poly, holes, area.
pub trait AsLoop {
    fn poly(&self) -> &[V2];
    fn holes(&self) -> &[Vec<V2>];
    fn area(&self) -> f64;
}
impl AsLoop for Room {
    fn poly(&self) -> &[V2] {
        &self.poly
    }
    fn holes(&self) -> &[Vec<V2>] {
        &self.holes
    }
    fn area(&self) -> f64 {
        self.area
    }
}
impl AsLoop for Outline {
    fn poly(&self) -> &[V2] {
        &self.poly
    }
    fn holes(&self) -> &[Vec<V2>] {
        &[]
    }
    fn area(&self) -> f64 {
        self.area
    }
}

/// `Math.round(x)` as an integer key part (JS keys are its decimal text).
fn rkey(x: f64) -> String {
    crate::js::num_str(m::round(x))
}

struct OutEdge {
    to: String,
    ang: f64,
    used: bool,
}

/// `planLoops(polys, { snap })`
pub fn plan_loops(polys: &[Vec<V2>]) -> Loops {
    plan_loops_snap(polys, 1e-3)
}

pub fn plan_loops_snap(polys: &[Vec<V2>], snap: f64) -> Loops {
    let mut edges: Vec<(V2, V2)> = Vec::new();
    for p in polys {
        let n = p.len();
        for i in 0..n {
            let (a, b) = (at(p, i), at(p, nx(i, n)));
            if m::hypot(b[0] - a[0], b[1] - a[1]) > EPS {
                edges.push((a, b));
            }
        }
    }
    let mut cuts: Vec<Vec<f64>> = edges.iter().map(|_| vec![0.0, 1.0]).collect();
    let ne = edges.len();
    for i in 0..ne {
        for j in i.wrapping_add(1)..ne {
            let (Some(&(a, b)), Some(&(c, d))) = (edges.get(i), edges.get(j)) else { continue };
            let r = sub(b, a);
            let s = sub(d, c);
            let den = cross(r, s);
            let ca = sub(c, a);
            if den.abs() < 1e-12 * m::len(r) * m::len(s) {
                if cross(ca, r).abs() > 1e-6 * m::len(r) {
                    continue;
                }
                let rr = dot(r, r);
                let ss = dot(s, s);
                for q in [c, d] {
                    let t = dot(sub(q, a), r) / rr;
                    if t > EPS
                        && t < 1.0 - EPS
                        && let Some(cu) = cuts.get_mut(i)
                    {
                        cu.push(t);
                    }
                }
                for q in [a, b] {
                    let t = dot(sub(q, c), s) / ss;
                    if t > EPS
                        && t < 1.0 - EPS
                        && let Some(cu) = cuts.get_mut(j)
                    {
                        cu.push(t);
                    }
                }
                continue;
            }
            let t = cross(ca, s) / den;
            let u = cross(ca, r) / den;
            if t < -EPS || t > 1.0 + EPS || u < -EPS || u > 1.0 + EPS {
                continue;
            }
            if t > EPS
                && t < 1.0 - EPS
                && let Some(cu) = cuts.get_mut(i)
            {
                cu.push(t);
            }
            if u > EPS
                && u < 1.0 - EPS
                && let Some(cu) = cuts.get_mut(j)
            {
                cu.push(u);
            }
        }
    }
    let key = |p: V2| format!("{},{}", rkey(p[0] / snap), rkey(p[1] / snap));
    let mut pts: HashMap<String, V2> = HashMap::new();
    let mut pieces_order: Vec<(String, String)> = Vec::new();
    let mut pieces_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (i, &(a, b)) in edges.iter().enumerate() {
        let mut ts: Vec<f64> = Vec::new();
        for &x in cuts.get(i).map(Vec::as_slice).unwrap_or(&[]) {
            let r = m::round(x * 1e9) / 1e9;
            if !ts.contains(&r) {
                ts.push(r);
            }
        }
        ts.sort_by(|x, y| crate::js::sign_ord(x - y));
        for k in 0..ts.len().saturating_sub(1) {
            let (Some(&t0), Some(&t1)) = (ts.get(k), ts.get(k.wrapping_add(1))) else { continue };
            let p = [a[0] + (b[0] - a[0]) * t0, a[1] + (b[1] - a[1]) * t0];
            let q = [a[0] + (b[0] - a[0]) * t1, a[1] + (b[1] - a[1]) * t1];
            let kp = key(p);
            pts.entry(kp.clone()).or_insert(p);
            let kq = key(q);
            pts.entry(kq.clone()).or_insert(q);
            if kp == kq {
                continue;
            }
            let id = if kp < kq { format!("{kp}|{kq}") } else { format!("{kq}|{kp}") };
            if pieces_seen.insert(id) {
                pieces_order.push((kp, kq));
            }
        }
    }
    let inside = |x: f64, y: f64| polys.iter().any(|p| in_poly(x, y, p));
    let mut out_order: Vec<String> = Vec::new();
    let mut out: HashMap<String, Vec<OutEdge>> = HashMap::new();
    for (kp, kq) in &pieces_order {
        let (Some(&p), Some(&q)) = (pts.get(kp), pts.get(kq)) else { continue };
        let d = sub(q, p);
        let l = m::len(d);
        let mid = [(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0];
        let n = [-d[1] / l, d[0] / l];
        let h = m::min(0.5, l / 4.0);
        let left = inside(mid[0] + n[0] * h, mid[1] + n[1] * h);
        let right = inside(mid[0] - n[0] * h, mid[1] - n[1] * h);
        if left == right {
            continue;
        }
        let (from, to) = if left { (kq.clone(), kp.clone()) } else { (kp.clone(), kq.clone()) };
        if !out.contains_key(&from) {
            out_order.push(from.clone());
            out.insert(from.clone(), Vec::new());
        }
        let (Some(&a), Some(&b)) = (pts.get(&from), pts.get(&to)) else { continue };
        if let Some(v) = out.get_mut(&from) {
            v.push(OutEdge { to, ang: m::atan2(b[1] - a[1], b[0] - a[0]), used: false });
        }
    }
    // walk the loops
    let mut loops: Vec<Vec<V2>> = Vec::new();
    let two_pi = 2.0 * std::f64::consts::PI;
    for start in &out_order {
        let count = out.get(start).map_or(0, Vec::len);
        for e0 in 0..count {
            if out.get(start).and_then(|v| v.get(e0)).is_none_or(|e| e.used) {
                continue;
            }
            let mut lp: Vec<String> = vec![start.clone()];
            let mut cur = start.clone();
            let mut e: Option<(String, usize)> = Some((start.clone(), e0));
            let mut guard = 0u32;
            let mut closed = false;
            while let Some((ek, ei)) = e.clone() {
                let Some(edge) = out.get_mut(&ek).and_then(|v| v.get_mut(ei)) else { break };
                if edge.used || guard >= 100_000 {
                    break;
                }
                guard = guard.saturating_add(1);
                edge.used = true;
                let at_k = edge.to.clone();
                if at_k == *start {
                    closed = true;
                    break;
                }
                lp.push(at_k.clone());
                let (Some(&pc), Some(&pa)) = (pts.get(&cur), pts.get(&at_k)) else { break };
                let back = m::atan2(pc[1] - pa[1], pc[0] - pa[0]);
                let mut best: Option<usize> = None;
                let mut best_a = f64::INFINITY;
                if let Some(list) = out.get(&at_k) {
                    for (ci, c) in list.iter().enumerate() {
                        if c.used {
                            continue;
                        }
                        let mut a = back - c.ang;
                        let mut g = 0u32;
                        while a <= 1e-9 && g < 64 {
                            a += two_pi;
                            g = g.saturating_add(1);
                        }
                        while a > two_pi + 1e-9 && g < 128 {
                            a -= two_pi;
                            g = g.saturating_add(1);
                        }
                        if a < best_a {
                            best_a = a;
                            best = Some(ci);
                        }
                    }
                }
                cur = at_k.clone();
                e = best.map(|b| (at_k, b));
            }
            // the JS checks `e && e.to === start` after the walk: the edge that closed it
            if lp.len() >= 3 && closed {
                let poly: Vec<V2> = lp.iter().filter_map(|k| pts.get(k).copied()).collect();
                loops.push(clean(&poly));
            }
        }
    }
    let mut rooms: Vec<Room> = Vec::new();
    let mut islands: Vec<Vec<V2>> = Vec::new();
    let mut outlines: Vec<Outline> = Vec::new();
    for l in loops {
        if l.len() < 3 {
            continue;
        }
        let a = area(&l);
        if a > 1.0 {
            let (p, c) = (perim(&l), centroid(&l));
            rooms.push(Room { poly: l, holes: Vec::new(), area: a, perimeter: p, centroid: c });
        } else if a < -1.0 {
            islands.push(l);
        }
    }
    for l in islands {
        let p0 = at(&l, 0);
        let mut hosts: Vec<usize> = (0..rooms.len()).filter(|&i| rooms.get(i).is_some_and(|r| in_poly(p0[0], p0[1], &r.poly))).collect();
        hosts.sort_by(|&x, &y| crate::js::sign_ord(rooms.get(x).map_or(0.0, |r| r.area) - rooms.get(y).map_or(0.0, |r| r.area)));
        let host = hosts.first().copied();
        let la = area(&l).abs();
        match host.and_then(|h| rooms.get_mut(h)).filter(|r| la < r.area) {
            Some(r) => {
                let mut rev = l.clone();
                rev.reverse();
                r.holes.push(rev);
            }
            None => {
                let mut o = l.clone();
                o.reverse();
                let (a, p) = (area(&o), perim(&o));
                outlines.push(Outline { poly: o, area: a, perimeter: p });
            }
        }
    }
    for r in &mut rooms {
        if r.holes.is_empty() {
            continue;
        }
        r.area -= r.holes.iter().fold(0.0, |s, h| s + area(h));
        r.perimeter += r.holes.iter().fold(0.0, |s, h| s + perim(h));
    }
    rooms.sort_by(|a, b| {
        let d = a.centroid[1] - b.centroid[1];
        let d = if d != 0.0 && !d.is_nan() { d } else { a.centroid[0] - b.centroid[0] };
        crate::js::sign_ord(d)
    });
    Loops { rooms, outlines }
}

/// `loopAt(loops, p)`: the smallest loop containing p (not inside one of its holes).
pub fn loop_at<T: AsLoop>(loops: &[T], p: V2) -> Option<&T> {
    let mut hits: Vec<&T> =
        loops.iter().filter(|l| in_poly(p[0], p[1], l.poly()) && !l.holes().iter().any(|h| in_poly(p[0], p[1], h))).collect();
    hits.sort_by(|a, b| crate::js::sign_ord(a.area().abs() - b.area().abs()));
    hits.first().copied()
}

/// The index of `loopAt`'s loop (rooms are told apart by identity in the JS).
pub fn loop_index<T: AsLoop>(loops: &[T], p: V2) -> Option<usize> {
    let mut hits: Vec<usize> = (0..loops.len())
        .filter(|&i| loops.get(i).is_some_and(|l| in_poly(p[0], p[1], l.poly()) && !l.holes().iter().any(|h| in_poly(p[0], p[1], h))))
        .collect();
    hits.sort_by(|&a, &b| crate::js::sign_ord(loops.get(a).map_or(0.0, |l| l.area().abs()) - loops.get(b).map_or(0.0, |l| l.area().abs())));
    hits.first().copied()
}

pub fn centroid(p: &[V2]) -> V2 {
    let n = p.len();
    let (mut a, mut x, mut y) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let (pa, pb) = (at(p, i), at(p, nx(i, n)));
        let c = pa[0] * pb[1] - pb[0] * pa[1];
        a += c;
        x += (pa[0] + pb[0]) * c;
        y += (pa[1] + pb[1]) * c;
    }
    if a.abs() < 1e-9 { at(p, 0) } else { [x / (3.0 * a), y / (3.0 * a)] }
}

/// A point well inside a polygon (for a label).
pub fn label_point(p: &[V2]) -> V2 {
    let c = centroid(p);
    if in_poly(c[0], c[1], p) {
        return c;
    }
    let n = p.len();
    let mut xs: Vec<f64> = Vec::new();
    for i in 0..n {
        let (a, b) = (at(p, i), at(p, nx(i, n)));
        if (a[1] > c[1]) != (b[1] > c[1]) {
            xs.push(a[0] + (c[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]));
        }
    }
    xs.sort_by(|a, b| crate::js::sign_ord(a - b));
    let mut best: Option<(f64, f64)> = None;
    let mut i = 0usize;
    while i.wrapping_add(1) < xs.len() {
        let (Some(&x0), Some(&x1)) = (xs.get(i), xs.get(i.wrapping_add(1))) else { break };
        if best.is_none_or(|b| x1 - x0 > b.1 - b.0) {
            best = Some((x0, x1));
        }
        i = i.wrapping_add(2);
    }
    match best {
        Some(b) => [(b.0 + b.1) / 2.0, c[1]],
        None => c,
    }
}

/// Drop repeated and collinear points of a loop.
fn clean(p: &[V2]) -> Vec<V2> {
    let mut q: Vec<V2> = Vec::new();
    for (i, v) in p.iter().enumerate() {
        if i == 0 {
            q.push(*v);
            continue;
        }
        let pr = at(p, i.wrapping_sub(1));
        if m::hypot(v[0] - pr[0], v[1] - pr[1]) > 1e-6 {
            q.push(*v);
        }
    }
    if q.len() > 1 {
        let (f, l) = (at(&q, 0), at(&q, q.len().wrapping_sub(1)));
        if m::hypot(f[0] - l[0], f[1] - l[1]) <= 1e-6 {
            q.pop();
        }
    }
    let mut changed = true;
    while changed && q.len() > 3 {
        changed = false;
        let n = q.len();
        for i in 0..n {
            let a = at(&q, pv(i, n));
            let b = at(&q, i);
            let c = at(&q, nx(i, n));
            let ab = sub(b, a);
            let bc = sub(c, b);
            if cross(ab, bc).abs() <= 1e-7 * (m::len(ab) * m::len(bc) + 1e-12) && dot(ab, bc) > 0.0 {
                q.remove(i);
                changed = true;
                break;
            }
        }
    }
    q
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ring_of_walls_is_one_room_and_one_outline() {
        // a 6 × 4 m room of 200 mm walls (outlines as the joins make them)
        let o = crate::draw::walls::wall_outlines(
            &[[0.0, 0.0], [6000.0, 0.0], [6000.0, 4000.0], [0.0, 4000.0]]
                .iter()
                .enumerate()
                .map(|(i, p)| crate::draw::walls::Seg {
                    key: format!("W{i}"),
                    a: *p,
                    b: [[0.0, 0.0], [6000.0, 0.0], [6000.0, 4000.0], [0.0, 4000.0]][(i + 1) % 4],
                    t: 200.0,
                })
                .collect::<Vec<_>>(),
            1.0,
        );
        let polys: Vec<Vec<V2>> = o.into_iter().map(|(_, p)| p).collect();
        let l = plan_loops(&polys);
        assert_eq!(l.rooms.len(), 1);
        assert_eq!(l.outlines.len(), 1);
        assert!((l.rooms[0].area - 5800.0 * 3800.0).abs() < 1e-6);
        assert!((l.outlines[0].area - 6200.0 * 4200.0).abs() < 1e-6);
    }
}
