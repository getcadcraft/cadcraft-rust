//! res-skeleton.js — the roof over any outline: a weighted straight skeleton whose edges may wait before they start
//! to move ("time" is height). Events: an edge shrinks away, a reflex corner reaches another edge (the loop splits), an
//! edge starts to move; the polygon is cleaned after each. Faces are each moving edge's swept area, lines the corners'
//! paths and the ridges left when loops close.

use std::collections::HashMap;

use crate::draw::m::{self, V2, at, cross, dot, nx, pv, sub};

fn hyp(a: V2, b: V2) -> f64 {
    m::hypot(b[0] - a[0], b[1] - a[1])
}

/// An edge's speed (inward per unit of height; 0 = it never moves) and the height it starts at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spec {
    pub speed: f64,
    pub start: f64,
}

/// A face: the edge it belongs to (the input polygon's index), its plan polygon and plane.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub edge: i64,
    pub poly: Vec<V2>,
    pub speed: f64,
    pub start: f64,
    pub n: V2,
    pub c: f64,
    pub u: V2,
}

/// A skeleton line between two faces (3D ends).
#[derive(Clone, Debug, PartialEq)]
pub struct SkLine {
    pub a: [f64; 3],
    pub b: [f64; 3],
    pub f: [i64; 2],
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Skeleton {
    pub faces: Vec<Face>,
    pub lines: Vec<SkLine>,
    pub top: f64,
    pub error: Option<&'static str>,
}

/// `faceZ(face, p)`
pub fn face_z(f: &Face, p: V2) -> f64 {
    f.start + (f.n[0] * p[0] + f.n[1] * p[1] - f.c) / f.speed
}

#[derive(Clone, Debug)]
struct Edge {
    u: V2,
    n: V2,
    c: f64,
    speed: f64,
    start: f64,
    orig: i64,
}

#[derive(Clone, Copy, Debug)]
struct Nd {
    x: f64,
    y: f64,
    e: usize,
    v: V2,
}

struct Sk {
    e: Vec<Edge>,
    eps: f64,
    scale: f64,
    pieces_order: Vec<usize>,
    pieces: HashMap<usize, Vec<[f64; 6]>>,
    lines: Vec<(V2, f64, V2, f64, [usize; 2])>,
}

impl Sk {
    fn edge(&self, i: usize) -> &Edge {
        static NONE: Edge = Edge { u: [1.0, 0.0], n: [0.0, 1.0], c: 0.0, speed: 0.0, start: 0.0, orig: -1 };
        self.e.get(i).unwrap_or(&NONE)
    }
    fn moving(&self, e: usize, t: f64) -> bool {
        let x = self.edge(e);
        x.speed > 0.0 && t >= x.start - 1e-9 * self.scale
    }
    fn piece(&mut self, e: usize, a: V2, az: f64, b: V2, bz: f64) {
        let ed = self.edge(e);
        if ed.orig < 0 || ed.speed <= 0.0 || hyp(a, b) <= self.eps {
            return;
        }
        let list = self.pieces.entry(e).or_insert_with(|| {
            self.pieces_order.push(e);
            Vec::new()
        });
        list.push([a[0], a[1], az, b[0], b[1], bz]);
    }
    fn vel_of(&self, a_i: usize, b_i: usize, t1: f64) -> V2 {
        let (a, b) = (self.edge(a_i), self.edge(b_i));
        let va = if self.moving(a_i, t1) { a.speed } else { 0.0 };
        let vb = if self.moving(b_i, t1) { b.speed } else { 0.0 };
        let det = cross(a.n, b.n);
        if det.abs() > 1e-9 {
            return [(va * b.n[1] - vb * a.n[1]) / det, (a.n[0] * vb - b.n[0] * va) / det];
        }
        if dot(a.n, b.n) > 0.0 {
            let v = (va + vb) / 2.0;
            return [a.n[0] * v, a.n[1] * v];
        }
        [0.0, 0.0]
    }

    fn close_loop(&mut self, cyc: &[Nd], t1: f64) {
        let n = cyc.len();
        for i in 0..n {
            let (Some(a), Some(b)) = (cyc.get(i), cyc.get(nx(i, n))) else { continue };
            if self.moving(a.e, t1) {
                self.piece(a.e, [a.x, a.y], t1, [b.x, b.y], t1);
            }
        }
        for i in 0..n {
            let (Some(a), Some(b)) = (cyc.get(i).copied(), cyc.get(nx(i, n)).copied()) else { continue };
            if hyp([a.x, a.y], [b.x, b.y]) <= self.eps {
                continue;
            }
            for j in i.wrapping_add(1)..n {
                let (Some(c), Some(d)) = (cyc.get(j).copied(), cyc.get(nx(j, n)).copied()) else { continue };
                if dot(self.edge(a.e).u, self.edge(c.e).u) > -0.999999 {
                    continue;
                }
                if let Some(ov) = overlap([a.x, a.y], [b.x, b.y], [c.x, c.y], [d.x, d.y], self.eps) {
                    self.lines.push((ov[0], t1, ov[1], t1, [a.e, c.e]));
                }
            }
        }
    }

    fn clean(&mut self, cs: Vec<Vec<Nd>>, t1: f64) -> Vec<Vec<Nd>> {
        let eps = self.eps;
        let mut out = Vec::new();
        let mut work = cs;
        let mut count = 0u32;
        while let Some(mut cyc) = work.pop() {
            if count >= 10_000 {
                break;
            }
            count = count.saturating_add(1);
            let mut again = true;
            let mut emptied = false;
            while again {
                again = false;
                if cyc.len() < 3 {
                    break;
                }
                // 1. corners closer than eps merge
                let mut i = 0usize;
                while i < cyc.len() && cyc.len() >= 3 {
                    let n = cyc.len();
                    let j = nx(i, n);
                    let (Some(a), Some(b)) = (cyc.get(i).copied(), cyc.get(j).copied()) else { break };
                    if hyp([a.x, a.y], [b.x, b.y]) <= eps * 4.0 {
                        if let Some(aa) = cyc.get_mut(i) {
                            aa.x = (a.x + b.x) / 2.0;
                            aa.y = (a.y + b.y) / 2.0;
                            aa.e = b.e;
                        }
                        cyc.remove(j);
                        again = true;
                        break;
                    }
                    i = i.wrapping_add(1);
                }
                if again || cyc.len() < 3 {
                    continue;
                }
                // 2. a spike: two edges folded onto one line
                let n = cyc.len();
                for i in 0..n {
                    let (p_i, n_i) = (pv(i, n), nx(i, n));
                    let (Some(p), Some(nd), Some(q)) = (cyc.get(p_i).copied(), cyc.get(i).copied(), cyc.get(n_i).copied()) else {
                        continue;
                    };
                    let (ea, eb) = (p.e, nd.e);
                    if dot(self.edge(ea).u, self.edge(eb).u) > -1.0 + 1e-9 {
                        continue;
                    }
                    let an = self.edge(ea).n;
                    let dq = (an[0] * (q.x - p.x) + an[1] * (q.y - p.y)).abs();
                    if dq > eps * 4.0 {
                        continue;
                    }
                    let lpn = hyp([p.x, p.y], [nd.x, nd.y]);
                    let lnq = hyp([nd.x, nd.y], [q.x, q.y]);
                    let x = if lpn <= lnq { [p.x, p.y] } else { [q.x, q.y] };
                    self.piece(ea, x, t1, [nd.x, nd.y], t1);
                    self.piece(eb, [nd.x, nd.y], t1, x, t1);
                    if hyp(x, [nd.x, nd.y]) > eps {
                        self.lines.push((x, t1, [nd.x, nd.y], t1, [ea, eb]));
                    }
                    if lpn < lnq - eps * 4.0 {
                        if let Some(pp) = cyc.get_mut(p_i) {
                            pp.e = eb;
                        }
                        cyc.remove(i);
                    } else if lnq < lpn - eps * 4.0 {
                        cyc.remove(i);
                    } else {
                        if let Some(pp) = cyc.get_mut(p_i) {
                            pp.x = (p.x + q.x) / 2.0;
                            pp.y = (p.y + q.y) / 2.0;
                            pp.e = q.e;
                        }
                        let qi = n_i;
                        let (hi, lo) = (i.max(qi), i.min(qi));
                        cyc.remove(hi);
                        cyc.remove(lo);
                    }
                    again = true;
                    break;
                }
                if again || cyc.len() < 3 {
                    continue;
                }
                // 3. a corner on another edge (or on another corner): the loop splits in two
                let m_len = cyc.len();
                'outer: for r in 0..m_len {
                    let Some(rr) = cyc.get(r).copied() else { continue };
                    for j in 0..m_len {
                        if j == r || nx(j, m_len) == r {
                            continue;
                        }
                        let (Some(q), Some(q1)) = (cyc.get(j).copied(), cyc.get(nx(j, m_len)).copied()) else { continue };
                        let e = self.edge(q.e).clone();
                        let d = e.n[0] * (rr.x - q.x) + e.n[1] * (rr.y - q.y);
                        if d.abs() > eps * 4.0 {
                            continue;
                        }
                        let s = e.u[0] * (rr.x - q.x) + e.u[1] * (rr.y - q.y);
                        let l = e.u[0] * (q1.x - q.x) + e.u[1] * (q1.y - q.y);
                        if s < -eps * 4.0 || s > l + eps * 4.0 {
                            continue;
                        }
                        let (mut c1, mut c2): (Vec<Nd>, Vec<Nd>);
                        if s <= eps * 4.0 || s >= l - eps * 4.0 {
                            let k = if s <= eps * 4.0 { j } else { nx(j, m_len) };
                            if k == r || nx(k, m_len) == r || nx(r, m_len) == k {
                                continue;
                            }
                            let Some(kk) = cyc.get(k).copied() else { continue };
                            c1 = Vec::new();
                            let mut x = r;
                            let mut g = 0usize;
                            while x != k && g <= m_len {
                                if let Some(v) = cyc.get(x) {
                                    c1.push(*v);
                                }
                                x = nx(x, m_len);
                                g = g.wrapping_add(1);
                            }
                            c2 = Vec::new();
                            let mut x = k;
                            let mut g = 0usize;
                            while x != r && g <= m_len {
                                if let Some(v) = cyc.get(x) {
                                    c2.push(*v);
                                }
                                x = nx(x, m_len);
                                g = g.wrapping_add(1);
                            }
                            let r1 = rr;
                            let k2 = Nd { x: rr.x, y: rr.y, ..kk };
                            if let Some(f) = c1.first_mut() {
                                *f = r1;
                            }
                            if let Some(f) = c2.first_mut() {
                                *f = k2;
                            }
                        } else {
                            let r1 = rr;
                            let r2 = Nd { e: q.e, ..rr };
                            c1 = vec![r1];
                            let jn = nx(j, m_len);
                            let mut x = nx(r, m_len);
                            let mut g = 0usize;
                            while x != jn && g <= m_len {
                                if let Some(v) = cyc.get(x) {
                                    c1.push(*v);
                                }
                                x = nx(x, m_len);
                                g = g.wrapping_add(1);
                            }
                            c2 = vec![r2];
                            let mut x = jn;
                            let mut g = 0usize;
                            while x != r && g <= m_len {
                                if let Some(v) = cyc.get(x) {
                                    c2.push(*v);
                                }
                                x = nx(x, m_len);
                                g = g.wrapping_add(1);
                            }
                        }
                        work.push(c1);
                        work.push(c2);
                        cyc.clear();
                        emptied = true;
                        break 'outer;
                    }
                }
                if cyc.is_empty() {
                    break;
                }
            }
            if emptied || cyc.is_empty() {
                continue;
            }
            let pts: Vec<V2> = cyc.iter().map(|q| [q.x, q.y]).collect();
            if cyc.len() < 3 || poly_area(&pts).abs() <= eps * self.scale {
                if cyc.len() >= 2 {
                    self.close_loop(&cyc, t1);
                }
                continue;
            }
            out.push(cyc);
        }
        out
    }
}

/// The part two antiparallel collinear segments share.
fn overlap(a: V2, b: V2, c: V2, d: V2, eps: f64) -> Option<[V2; 2]> {
    let l = hyp(a, b);
    if l <= eps {
        return None;
    }
    let u = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
    if cross(u, sub(c, a)).abs() > eps * 4.0 || cross(u, sub(d, a)).abs() > eps * 4.0 {
        return None;
    }
    let s0 = m::max(0.0, m::min(dot(u, sub(c, a)), dot(u, sub(d, a))));
    let s1 = m::min(l, m::max(dot(u, sub(c, a)), dot(u, sub(d, a))));
    if s1 - s0 <= eps {
        return None;
    }
    Some([[a[0] + u[0] * s0, a[1] + u[1] * s0], [a[0] + u[0] * s1, a[1] + u[1] * s1]])
}

/// `polyArea(p)`
pub fn poly_area(p: &[V2]) -> f64 {
    m::area(p)
}

/// `skeleton(poly, specs, { eps })`
pub fn skeleton(poly: &[V2], specs: &[Spec]) -> Skeleton {
    let p0: Vec<V2> = poly.to_vec();
    let s0: Vec<Spec> = specs
        .iter()
        .map(|s| Spec { speed: if s.speed > 0.0 { s.speed } else { 0.0 }, start: if s.start.is_finite() { s.start } else { 0.0 } })
        .collect();
    let xs: Vec<f64> = p0.iter().map(|q| q[0]).collect();
    let ys: Vec<f64> = p0.iter().map(|q| q[1]).collect();
    let scale = m::max(
        m::max(1.0, m::max_of(xs.iter().copied()) - m::min_of(xs.iter().copied())),
        m::max_of(ys.iter().copied()) - m::min_of(ys.iter().copied()),
    );
    let eps = scale * 1e-7;
    let spec = |i: usize| s0.get(i).copied().unwrap_or(Spec { speed: 0.0, start: 0.0 });
    let mut pts: Vec<V2> = Vec::new();
    let mut sp: Vec<Spec> = Vec::new();
    let mut ix: Vec<i64> = Vec::new();
    let n0 = p0.len();
    for i in 0..n0 {
        if hyp(at(&p0, i), at(&p0, nx(i, n0))) > eps {
            pts.push(at(&p0, i));
            sp.push(spec(i));
            ix.push(i64::try_from(i).unwrap_or(-1));
        }
    }
    let mut changed = true;
    while changed && pts.len() > 3 {
        changed = false;
        let n = pts.len();
        for i in 0..n {
            let h = pv(i, n);
            let (a, b, c) = (at(&pts, h), at(&pts, i), at(&pts, nx(i, n)));
            let u = sub(b, a);
            let v = sub(c, b);
            let (Some(sh), Some(si)) = (sp.get(h).copied(), sp.get(i).copied()) else { continue };
            if cross(u, v).abs() <= 1e-9 * m::len(u) * m::len(v)
                && dot(u, v) > 0.0
                && sh.speed == si.speed
                && (sh.start - si.start).abs() < 1e-9
            {
                pts.remove(i);
                sp.remove(i);
                ix.remove(i);
                changed = true;
                break;
            }
        }
    }
    if pts.len() < 3 {
        return Skeleton { error: Some("res.err.shape"), ..Skeleton::default() };
    }
    let n = pts.len();
    let mut e: Vec<Edge> = Vec::new();
    for i in 0..n {
        let (a, b) = (at(&pts, i), at(&pts, nx(i, n)));
        let l = hyp(a, b);
        let u = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
        let nn = [-u[1], u[0]];
        let s = sp.get(i).copied().unwrap_or(Spec { speed: 0.0, start: 0.0 });
        e.push(Edge { u, n: nn, c: dot(nn, a), speed: s.speed, start: s.start, orig: ix.get(i).copied().unwrap_or(-1) });
    }
    let mut order: Vec<Nd> = Vec::new();
    for i in 0..n {
        let h = pv(i, n);
        let (Some(ea), Some(eb)) = (e.get(h).cloned(), e.get(i).cloned()) else { continue };
        let p = at(&pts, i);
        if cross(ea.u, eb.u).abs() < 1e-9 && dot(ea.u, eb.u) > 0.0 {
            let faster = ea.speed > eb.speed || (ea.speed == eb.speed && ea.start < eb.start);
            let u = if faster { [-ea.n[0], -ea.n[1]] } else { [ea.n[0], ea.n[1]] };
            let nn = [-u[1], u[0]];
            e.push(Edge { u, n: nn, c: dot(nn, p), speed: 0.0, start: f64::INFINITY, orig: -1 });
            order.push(Nd { x: p[0], y: p[1], e: e.len().wrapping_sub(1), v: [0.0; 2] });
        }
        order.push(Nd { x: p[0], y: p[1], e: i, v: [0.0; 2] });
    }
    let mut sk = Sk { e, eps, scale, pieces_order: Vec::new(), pieces: HashMap::new(), lines: Vec::new() };
    let mut started: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut t = m::min_of(sk.e.iter().filter(|x| x.speed > 0.0).map(|x| x.start));
    if !t.is_finite() {
        return Skeleton { error: Some("res.err.nopitch"), ..Skeleton::default() };
    }
    let mut cycles: Vec<Vec<Nd>> = vec![order];
    let mut guard = 0u32;
    while !cycles.is_empty() && guard < 20_000 {
        guard = guard.saturating_add(1);
        // edges that start now: their segment is where their face begins
        for ci in 0..cycles.len() {
            let Some(cyc) = cycles.get(ci).cloned() else { continue };
            let k = cyc.len();
            for i in 0..k {
                let (Some(q), Some(nxq)) = (cyc.get(i), cyc.get(nx(i, k))) else { continue };
                if sk.moving(q.e, t) && !started.contains(&q.e) {
                    sk.piece(q.e, [q.x, q.y], t, [nxq.x, nxq.y], t);
                }
            }
        }
        for cyc in &cycles {
            for q in cyc {
                if sk.moving(q.e, t) {
                    started.insert(q.e);
                }
            }
        }
        // velocities
        for cyc in &mut cycles {
            let k = cyc.len();
            for i in 0..k {
                let pe = cyc.get(pv(i, k)).map_or(0, |q| q.e);
                let ee = cyc.get(i).map_or(0, |q| q.e);
                let v = sk.vel_of(pe, ee, t);
                if let Some(q) = cyc.get_mut(i) {
                    q.v = v;
                }
            }
        }
        // the next event
        let mut tn = f64::INFINITY;
        for x in &sk.e {
            if x.speed > 0.0 && x.start > t + 1e-9 * scale && x.start < tn {
                tn = x.start;
            }
        }
        for cyc in &cycles {
            let mm = cyc.len();
            for i in 0..mm {
                let (Some(a), Some(b)) = (cyc.get(i), cyc.get(nx(i, mm))) else { continue };
                let u = sk.edge(a.e).u;
                let len = u[0] * (b.x - a.x) + u[1] * (b.y - a.y);
                let rate = u[0] * (b.v[0] - a.v[0]) + u[1] * (b.v[1] - a.v[1]);
                if rate < -1e-12 {
                    let tt = t + m::max(0.0, len) / -rate;
                    if tt < tn {
                        tn = tt;
                    }
                }
            }
            for r in 0..mm {
                let Some(rr) = cyc.get(r) else { continue };
                let pa = cyc.get(pv(r, mm)).map_or(0, |q| q.e);
                let (au, bu) = (sk.edge(pa).u, sk.edge(rr.e).u);
                if cross(au, bu) > -1e-9 {
                    continue;
                }
                for j in 0..mm {
                    if j == r || nx(j, mm) == r {
                        continue;
                    }
                    let (Some(q), Some(q1)) = (cyc.get(j), cyc.get(nx(j, mm))) else { continue };
                    let ed = sk.edge(q.e);
                    let ve = if sk.moving(q.e, t) { ed.speed } else { 0.0 };
                    let d0 = ed.n[0] * (rr.x - q.x) + ed.n[1] * (rr.y - q.y);
                    let rate = ed.n[0] * rr.v[0] + ed.n[1] * rr.v[1] - ve;
                    if !(d0 > eps && rate < -1e-12) {
                        continue;
                    }
                    let th = t + d0 / -rate;
                    if th >= tn {
                        continue;
                    }
                    let dt = th - t;
                    let pr = [rr.x + rr.v[0] * dt, rr.y + rr.v[1] * dt];
                    let pq = [q.x + q.v[0] * dt, q.y + q.v[1] * dt];
                    let pq1 = [q1.x + q1.v[0] * dt, q1.y + q1.v[1] * dt];
                    let s = dot(ed.u, sub(pr, pq));
                    let l = dot(ed.u, sub(pq1, pq));
                    if s > -eps && s < l + eps {
                        tn = th;
                    }
                }
            }
        }
        if !tn.is_finite() {
            break;
        }
        // advance
        let dt = tn - t;
        for cyc in &mut cycles {
            let k = cyc.len();
            for i in 0..k {
                let pe = cyc.get(pv(i, k)).map_or(0, |q| q.e);
                let Some(q) = cyc.get(i).copied() else { continue };
                let a = [q.x, q.y];
                let b = [q.x + q.v[0] * dt, q.y + q.v[1] * dt];
                if dt > 0.0 && hyp(a, b) > eps {
                    let (mp, mq) = (sk.moving(pe, t), sk.moving(q.e, t));
                    if mp {
                        sk.piece(pe, a, t, b, tn);
                    }
                    if mq {
                        sk.piece(q.e, a, t, b, tn);
                    }
                    if mp || mq {
                        sk.lines.push((a, t, b, tn, [pe, q.e]));
                    }
                }
                if let Some(qq) = cyc.get_mut(i) {
                    qq.x = b[0];
                    qq.y = b[1];
                }
            }
        }
        t = tn;
        cycles = sk.clean(cycles, t);
    }
    // faces: each moving edge's pieces chained into polygons
    let mut faces = Vec::new();
    for &ei in &sk.pieces_order {
        let Some(list) = sk.pieces.get(&ei) else { continue };
        for lp in chain(list, eps * 20.0) {
            let mut poly2 = tidy(&lp, eps * 4.0);
            if poly2.len() < 3 {
                continue;
            }
            let ar = poly_area(&poly2);
            if ar.abs() <= eps * scale * 10.0 {
                continue;
            }
            if ar < 0.0 {
                poly2.reverse();
            }
            let ed = sk.edge(ei);
            faces.push(Face { edge: ed.orig, poly: poly2, speed: ed.speed, start: ed.start, n: ed.n, c: ed.c, u: ed.u });
        }
    }
    let raw: Vec<SkLine> = sk
        .lines
        .iter()
        .filter(|l| sk.edge(l.4[0]).orig >= 0 || sk.edge(l.4[1]).orig >= 0)
        .map(|l| SkLine { a: [l.0[0], l.0[1], l.1], b: [l.2[0], l.2[1], l.3], f: [sk.edge(l.4[0]).orig, sk.edge(l.4[1]).orig] })
        .collect();
    let lines = merge_lines(raw, eps * 20.0);
    Skeleton { faces, lines, top: t, error: None }
}

/// Segments → closed loops of plan points (ends within tol are one point).
fn chain(segs: &[[f64; 6]], tol: f64) -> Vec<Vec<V2>> {
    let mut p: Vec<V2> = Vec::new();
    let mut id_of = |x: f64, y: f64| -> usize {
        for (i, q) in p.iter().enumerate() {
            if (q[0] - x).abs() <= tol && (q[1] - y).abs() <= tol {
                return i;
            }
        }
        p.push([x, y]);
        p.len().wrapping_sub(1)
    };
    let mut adj: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut es: Vec<(usize, usize, (usize, usize))> = Vec::new();
    for s in segs {
        let a = id_of(s[0], s[1]);
        let b = id_of(s[3], s[4]);
        if a == b {
            continue;
        }
        let k = if a < b { (a, b) } else { (b, a) };
        if es.iter().any(|x| x.2 == k) {
            continue;
        }
        let id = es.len();
        es.push((a, b, k));
        for v in [a, b] {
            adj.entry(v).or_default().push(id);
        }
    }
    let mut used = vec![false; es.len()];
    let mut loops = Vec::new();
    for s in 0..es.len() {
        if used.get(s).copied().unwrap_or(true) {
            continue;
        }
        if let Some(u) = used.get_mut(s) {
            *u = true;
        }
        let Some(&(start, b0, _)) = es.get(s) else { continue };
        let mut lp = vec![start];
        let mut cur = b0;
        let mut prev = start;
        let mut n = 0usize;
        while n < es.len().saturating_add(2) && cur != start {
            n = n.wrapping_add(1);
            lp.push(cur);
            let opts: Vec<usize> =
                adj.get(&cur).map(|v| v.iter().copied().filter(|&id| !used.get(id).copied().unwrap_or(true)).collect()).unwrap_or_default();
            let Some(&first) = opts.first() else { break };
            let mut best = first;
            if opts.len() > 1 {
                let din = sub(at(&p, cur), at(&p, prev));
                let mut best_ang = f64::INFINITY;
                for &id in &opts {
                    let Some(&(ea, eb, _)) = es.get(id) else { continue };
                    let nxv = if ea == cur { eb } else { ea };
                    let dout = sub(at(&p, nxv), at(&p, cur));
                    let ang = m::atan2(cross(din, dout), dot(din, dout));
                    if -ang < best_ang {
                        best_ang = -ang;
                        best = id;
                    }
                }
            }
            if let Some(u) = used.get_mut(best) {
                *u = true;
            }
            let Some(&(ea, eb, _)) = es.get(best) else { break };
            let nxv = if ea == cur { eb } else { ea };
            prev = cur;
            cur = nxv;
        }
        if cur == start && lp.len() >= 3 {
            loops.push(lp.iter().map(|&i| at(&p, i)).collect());
        }
    }
    loops
}

/// Collinear points and back-and-forth spikes out.
fn tidy(poly: &[V2], eps: f64) -> Vec<V2> {
    let mut p = poly.to_vec();
    let mut changed = true;
    while changed && p.len() >= 3 {
        changed = false;
        let n = p.len();
        for i in 0..n {
            let (a, b, c) = (at(&p, pv(i, n)), at(&p, i), at(&p, nx(i, n)));
            let u = sub(b, a);
            let v = sub(c, b);
            let (lu, lv) = (m::len(u), m::len(v));
            if lu <= eps || lv <= eps || cross(u, v).abs() / m::max(lu, lv) <= eps {
                p.remove(i);
                changed = true;
                break;
            }
        }
    }
    p
}

/// Collinear, end-to-end lines between the same two faces joined into one.
fn merge_lines(ls: Vec<SkLine>, tol: f64) -> Vec<SkLine> {
    let key = |l: &SkLine| if l.f[0] < l.f[1] { (l.f[0], l.f[1]) } else { (l.f[1], l.f[0]) };
    let mut order: Vec<(i64, i64)> = Vec::new();
    let mut groups: HashMap<(i64, i64), Vec<SkLine>> = HashMap::new();
    for l in ls {
        let d = hyp([l.a[0], l.a[1]], [l.b[0], l.b[1]]);
        if d <= tol && (l.a[2] - l.b[2]).abs() <= tol {
            continue;
        }
        let k = key(&l);
        if !groups.contains_key(&k) {
            order.push(k);
        }
        groups.entry(k).or_default().push(l);
    }
    let mut out = Vec::new();
    for k in order {
        let Some(mut list) = groups.remove(&k) else { continue };
        let mut changed = true;
        while changed {
            changed = false;
            'scan: for i in 0..list.len() {
                for j in i.wrapping_add(1)..list.len() {
                    let (Some(a), Some(b)) = (list.get(i).cloned(), list.get(j).cloned()) else { continue };
                    for (p, q, r, s) in [(a.a, a.b, b.a, b.b), (a.a, a.b, b.b, b.a), (a.b, a.a, b.a, b.b), (a.b, a.a, b.b, b.a)] {
                        if m::hypot3(q[0] - r[0], q[1] - r[1], q[2] - r[2]) > tol {
                            continue;
                        }
                        let d1 = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
                        let d2 = [s[0] - r[0], s[1] - r[1], s[2] - r[2]];
                        let l1 = m::hypot3(d1[0], d1[1], d1[2]);
                        let l2 = m::hypot3(d2[0], d2[1], d2[2]);
                        if l1 == 0.0 || l2 == 0.0 || l1.is_nan() || l2.is_nan() {
                            continue;
                        }
                        let c = (d1[0] * d2[0] + d1[1] * d2[1] + d1[2] * d2[2]) / (l1 * l2);
                        if c < 1.0 - 1e-9 {
                            continue;
                        }
                        if let Some(x) = list.get_mut(i) {
                            *x = SkLine { a: p, b: s, f: a.f };
                        }
                        list.remove(j);
                        changed = true;
                        break 'scan;
                    }
                }
            }
        }
        out.extend(list);
    }
    out
}
