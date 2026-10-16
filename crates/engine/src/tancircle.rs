//! tancircle.js — circles tangent to picked objects (CIRCLE TTR, Tan Tan Tan). Ported here (it is not part of
//! cc-cad-geom); the same math in the same order as the JS.

use cc_cad::entity::Entity;
use cc_cad::js;
use cc_cad_geom::geom::{Blocks, EPS, FAR, dist, prim_param, prim_point, prims};
use cc_cad_geom::jsmath::JsMath;
use cc_geom::P2;

/// A straight line (unit direction) or a full circle under a pick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    L { x: f64, y: f64, dx: f64, dy: f64 },
    C { cx: f64, cy: f64, r: f64 },
}

/// A tangent circle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Circle {
    pub cx: f64,
    pub cy: f64,
    pub r: f64,
}

/// V8's `Math.hypot` over any number of arguments (Kahan-compensated, scaled by the largest).
pub fn hypot_n(xs: &[f64]) -> f64 {
    let mut max = 0.0f64;
    let mut nan = false;
    for x in xs {
        let a = x.abs();
        if a.is_infinite() {
            return f64::INFINITY;
        }
        if a.is_nan() {
            nan = true;
        } else if a > max {
            max = a;
        }
    }
    if nan {
        return f64::NAN;
    }
    if max == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut comp = 0.0f64;
    for x in xs {
        let n = x.abs() / max;
        let summand = n * n - comp;
        let pre = sum + summand;
        comp = (pre - sum) - summand;
        sum = pre;
    }
    sum.sqrt() * max
}

/// `curveAt(e, pick, blocks)`.
pub fn curve_at(e: Option<&Entity>, pick: P2, blocks: Option<&Blocks>) -> Option<Curve> {
    let e = e?;
    let mut best = None;
    let mut bd = f64::INFINITY;
    for q in prims(e, blocks) {
        if q.boxed {
            continue;
        }
        let d = dist(prim_point(&q, prim_param(&q, pick, true)), pick);
        if d < bd {
            bd = d;
            best = Some(q);
        }
    }
    let best = best?;
    if best.is_l() {
        let l = js::hypot(best.x2 - best.x1, best.y2 - best.y1);
        return if l > EPS {
            Some(Curve::L { x: best.x1, y: best.y1, dx: (best.x2 - best.x1) / l, dy: (best.y2 - best.y1) / l })
        } else {
            None
        };
    }
    if best.r > EPS { Some(Curve::C { cx: best.cx, cy: best.cy, r: best.r }) } else { None }
}

/// `touchPoint(o, c)`: where the circle touches o.
pub fn touch_point(o: &Curve, c: &Circle) -> P2 {
    match *o {
        Curve::L { x, y, dx, dy } => {
            let t = (c.cx - x) * dx + (c.cy - y) * dy;
            P2::new(x + dx * t, y + dy * t)
        }
        Curve::C { cx, cy, r } => {
            let d = js::hypot(c.cx - cx, c.cy - cy);
            if d < EPS {
                return P2::new(cx + r, cy);
            }
            let k = (c.cx - cx) / d;
            let m = (c.cy - cy) / d;
            let s = if d > r || (d - (r - c.r)).abs() < (d - (r + c.r)).abs() { 1.0 } else { -1.0 };
            P2::new(cx + k * r * s, cy + m * r * s)
        }
    }
}

fn tangent(o: &Curve, c: &Circle, tol: f64) -> bool {
    match *o {
        Curve::L { x, y, dx, dy } => (((c.cx - x) * dy - (c.cy - y) * dx).abs() - c.r).abs() <= tol,
        Curve::C { cx, cy, r } => {
            let d = js::hypot(c.cx - cx, c.cy - cy);
            (d - (r + c.r)).abs() <= tol || (d - (r - c.r).abs()).abs() <= tol
        }
    }
}

fn offsets(o: &Curve, r: f64) -> Vec<Curve> {
    match *o {
        Curve::L { x, y, dx, dy } => {
            let (nx, ny) = (-dy, dx);
            [1.0, -1.0].iter().map(|s| Curve::L { x: x + nx * r * s, y: y + ny * r * s, dx, dy }).collect()
        }
        Curve::C { cx, cy, r: or } => {
            [or + r, (or - r).abs()].into_iter().filter(|rr| *rr > EPS).map(|rr| Curve::C { cx, cy, r: rr }).collect()
        }
    }
}

fn cross(a: &Curve, b: &Curve) -> Vec<P2> {
    match (*a, *b) {
        (Curve::L { x: ax, y: ay, dx: adx, dy: ady }, Curve::L { x: bx, y: by, dx: bdx, dy: bdy }) => {
            let den = adx * bdy - ady * bdx;
            if den.abs() < 1e-12 {
                return Vec::new();
            }
            let t = ((bx - ax) * bdy - (by - ay) * bdx) / den;
            vec![P2::new(ax + adx * t, ay + ady * t)]
        }
        (Curve::C { .. }, Curve::L { .. }) => cross(b, a),
        (Curve::L { x, y, dx, dy }, Curve::C { cx, cy, r }) => {
            let (fx, fy) = (x - cx, y - cy);
            let bb = fx * dx + fy * dy;
            let cc = fx * fx + fy * fy - r * r;
            let d = bb * bb - cc;
            if d < -1e-12 * js::max(1.0, r * r) {
                return Vec::new();
            }
            let s = js::max(0.0, d).sqrt();
            let ts = if s < 1e-12 { vec![-bb] } else { vec![-bb + s, -bb - s] };
            ts.into_iter().map(|t| P2::new(x + dx * t, y + dy * t)).collect()
        }
        (Curve::C { cx: acx, cy: acy, r: ar }, Curve::C { cx: bcx, cy: bcy, r: br }) => {
            let (dx, dy) = (bcx - acx, bcy - acy);
            let d = js::hypot(dx, dy);
            if d < EPS || d > ar + br + 1e-9 * (ar + br) || d < (ar - br).abs() - 1e-9 * (ar + br) {
                return Vec::new();
            }
            let l = (ar * ar - br * br + d * d) / (2.0 * d);
            let h = js::max(0.0, ar * ar - l * l).sqrt();
            let mx = acx + dx * l / d;
            let my = acy + dy * l / d;
            if h < 1e-12 {
                vec![P2::new(mx, my)]
            } else {
                vec![P2::new(mx - dy * h / d, my + dx * h / d), P2::new(mx + dy * h / d, my - dx * h / d)]
            }
        }
    }
}

fn scale(os: &[Curve]) -> f64 {
    js::max_of(std::iter::once(1.0).chain(os.iter().map(|o| match *o {
        Curve::L { x, y, .. } => js::hypot(x, y),
        Curve::C { cx, cy, r } => js::hypot(cx, cy) + r,
    })))
}

fn nearest_to_picks(cands: &[Circle], list: &[(&Curve, P2)]) -> Option<Circle> {
    let mut best = None;
    let mut bd = f64::INFINITY;
    for c in cands {
        let sc = list.iter().fold(0.0, |s, (o, p)| s + dist(touch_point(o, c), *p));
        if sc < bd - 1e-12 {
            bd = sc;
            best = Some(*c);
        }
    }
    best
}

/// `circleTTR(a, pa, b, pb, r)`: tangent to a and b with radius r, the one touching nearest the picks.
pub fn circle_ttr(a: &Curve, pa: P2, b: &Curve, pb: P2, r: f64) -> Option<Circle> {
    if !(r > EPS) {
        return None;
    }
    let tol = 1e-7 * scale(&[*a, *b, Curve::C { cx: 0.0, cy: 0.0, r }]);
    let mut cands = Vec::new();
    for oa in offsets(a, r) {
        for ob in offsets(b, r) {
            for q in cross(&oa, &ob) {
                let c = Circle { cx: q.x, cy: q.y, r };
                if q.x.abs() < FAR && q.y.abs() < FAR && tangent(a, &c, tol) && tangent(b, &c, tol) {
                    cands.push(c);
                }
            }
        }
    }
    nearest_to_picks(&cands, &[(a, pa), (b, pb)])
}

fn residual(o: &Curve, s: f64, x: f64, y: f64, r: f64) -> f64 {
    match *o {
        Curve::L { x: ox, y: oy, dx, dy } => (x - ox) * dy - (y - oy) * dx - s * r,
        Curve::C { cx, cy, r: or } => js::hypot(x - cx, y - cy) - (or + s * r),
    }
}
fn residual2(o: &Curve, s: f64, x: f64, y: f64, r: f64) -> f64 {
    match *o {
        Curve::C { cx, cy, r: or } if s == -1.0 => js::hypot(x - cx, y - cy) - (or - r).abs(),
        _ => residual(o, s, x, y, r),
    }
}
type M3 = [[f64; 3]; 3];
fn det(m: &M3) -> f64 {
    let [[a, b, c], [d, e, f], [g, h, i]] = *m;
    a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)
}
fn solve3(os: &[Curve; 3], ss: [f64; 3], x0: f64, y0: f64, r0: f64) -> Option<Circle> {
    let (mut x, mut y, mut r) = (x0, y0, r0);
    for _ in 0..60 {
        let f: [f64; 3] = [residual2(&os[0], ss[0], x, y, r), residual2(&os[1], ss[1], x, y, r), residual2(&os[2], ss[2], x, y, r)];
        let lim = 1e-13 * js::max_of([1.0, x.abs(), y.abs(), r]);
        if f.iter().all(|v| v.abs() < lim) {
            break;
        }
        let h = 1e-7 * js::max_of([1.0, x.abs(), y.abs(), r.abs()]);
        let mut j: M3 = [[0.0; 3]; 3];
        for (i, row) in j.iter_mut().enumerate() {
            let (o, s, fi) = match i {
                0 => (&os[0], ss[0], f[0]),
                1 => (&os[1], ss[1], f[1]),
                _ => (&os[2], ss[2], f[2]),
            };
            *row =
                [(residual2(o, s, x + h, y, r) - fi) / h, (residual2(o, s, x, y + h, r) - fi) / h, (residual2(o, s, x, y, r + h) - fi) / h];
        }
        let d = det(&j);
        if !d.is_finite() || d.abs() < 1e-18 {
            return None;
        }
        let col = |k: usize| -> M3 {
            let mut m = j;
            for (i, row) in m.iter_mut().enumerate() {
                let fi = match i {
                    0 => f[0],
                    1 => f[1],
                    _ => f[2],
                };
                match k {
                    0 => row[0] = -fi,
                    1 => row[1] = -fi,
                    _ => row[2] = -fi,
                }
            }
            m
        };
        let mut dx = det(&col(0)) / d;
        let mut dy = det(&col(1)) / d;
        let mut dr = det(&col(2)) / d;
        let step = hypot_n(&[dx, dy, dr]);
        let lim = js::max(1.0, r.abs()) * 2.0;
        if step > lim {
            dx *= lim / step;
            dy *= lim / step;
            dr *= lim / step;
        }
        x += dx;
        y += dy;
        r += dr;
        if !(x + y + r).is_finite() {
            return None;
        }
    }
    if r > EPS { Some(Circle { cx: x, cy: y, r }) } else { None }
}

/// `circleTTT(a, pa, b, pb, c, pc)`: tangent to three, the one touching nearest the picks.
pub fn circle_ttt(a: &Curve, pa: P2, b: &Curve, pb: P2, c: &Curve, pc: P2) -> Option<Circle> {
    let os = [*a, *b, *c];
    let tol = 1e-7 * scale(&os);
    let pp = [pa, pb, pc];
    let m = P2::new((pa.x + pb.x + pc.x) / 3.0, (pa.y + pb.y + pc.y) / 3.0);
    let spread = js::max_of(std::iter::once(1e-6).chain(pp.iter().map(|p| dist(*p, m))));
    let mut seeds = Vec::new();
    for k in [0.5, 1.0, 2.0] {
        for i in 0..6u8 {
            let t = f64::from(i) * std::f64::consts::PI / 3.0;
            seeds.push((m.x + t.jcos() * spread * (k - 0.5), m.y + t.jsin() * spread * (k - 0.5), spread * k * 0.6));
        }
    }
    let mut found: Vec<Circle> = Vec::new();
    for s0 in [1.0, -1.0] {
        for s1 in [1.0, -1.0] {
            for s2 in [1.0, -1.0] {
                for &(x, y, r) in &seeds {
                    let Some(sol) = solve3(&os, [s0, s1, s2], x, y, r) else { continue };
                    if !os.iter().all(|o| tangent(o, &sol, tol * 10.0)) {
                        continue;
                    }
                    if !found.iter().any(|f| (f.cx - sol.cx).abs() + (f.cy - sol.cy).abs() + (f.r - sol.r).abs() < tol * 100.0) {
                        found.push(sol);
                    }
                }
            }
        }
    }
    nearest_to_picks(&found, &[(a, pa), (b, pb), (c, pc)])
}
