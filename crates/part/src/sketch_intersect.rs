//! sketch-intersect.js — curve–curve intersections for sketch curves, used by trim, extend and offset: line × line,
//! line × circle and circle × circle in closed form; a spline against a line or circle by sampling the implicit
//! distance per knot span (24 samples), bisection on sign changes and a golden-section look at near touches; spline ×
//! spline by crossing fine polylines (48 per span) and 2-D Newton. Results sort along the first curve with V8's
//! TimSort (cc-m3d), as `Array.prototype.sort` orders them.

use crate::js;
use crate::sketch::{self as s, Ent};
use crate::spline::{self as sp, P2};

const TAU: f64 = 2.0 * std::f64::consts::PI;

/// An intersectable curve (`curveOf`).
#[derive(Clone, Debug)]
pub enum Curve {
    Line { id: String, a: P2, b: P2 },
    Circle { id: String, c: P2, r: f64 },
    Arc { id: String, c: P2, r: f64, a0: f64, sweep: f64 },
    Spline { id: String, bs: sp::Curve, closed: bool },
}

impl Curve {
    pub fn id(&self) -> &str {
        match self {
            Curve::Line { id, .. } | Curve::Circle { id, .. } | Curve::Arc { id, .. } | Curve::Spline { id, .. } => id,
        }
    }
    fn round(&self) -> bool {
        matches!(self, Curve::Circle { .. } | Curve::Arc { .. })
    }
    fn centre(&self) -> (P2, f64) {
        match self {
            Curve::Circle { c, r, .. } | Curve::Arc { c, r, .. } => (*c, *r),
            _ => ([f64::NAN; 2], f64::NAN),
        }
    }
}

/// `curveOf(sk, e)`: None for points (and other kinds).
pub fn curve_of(sk: &serde_json::Value, e: &Ent) -> Result<Option<Curve>, String> {
    let id = e.id().to_string();
    Ok(Some(match e.ty() {
        "line" => Curve::Line { id, a: s::pxy(sk, e.str("p1")), b: s::pxy(sk, e.str("p2")) },
        "circle" => Curve::Circle { id, c: s::pxy(sk, e.str("c")), r: e.num("r") },
        "arc" => {
            let (c, r, a0, sweep) = s::arc_angles(sk, e);
            Curve::Arc { id, c, r, a0, sweep }
        }
        "spline" => Curve::Spline { id, bs: s::spline_curve(sk, e)?, closed: crate::jv::truthy(e.val("closed")) },
        _ => return Ok(None),
    }))
}

/// `normAng(a)`.
pub fn norm_ang(a: f64) -> f64 {
    let a = a % TAU;
    if a < 0.0 { a + TAU } else { a }
}

/// `pointOn(cv, t)`.
pub fn point_on(cv: &Curve, t: f64) -> P2 {
    match cv {
        Curve::Line { a, b, .. } => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t],
        Curve::Circle { c, r, .. } => [c[0] + r * js::cos(t), c[1] + r * js::sin(t)],
        Curve::Arc { c, r, a0, .. } => [c[0] + r * js::cos(a0 + t), c[1] + r * js::sin(a0 + t)],
        Curve::Spline { bs, .. } => sp::point_at(bs, t),
    }
}

/// `tangentOn(cv, t)`: unit tangent along the parameter.
pub fn tangent_on(cv: &Curve, t: f64) -> P2 {
    let d = match cv {
        Curve::Line { a, b, .. } => [b[0] - a[0], b[1] - a[1]],
        Curve::Circle { .. } => [-js::sin(t), js::cos(t)],
        Curve::Arc { a0, .. } => [-js::sin(a0 + t), js::cos(a0 + t)],
        Curve::Spline { bs, .. } => sp::derivs_at(bs, t, 1).get(1).copied().unwrap_or([f64::NAN; 2]),
    };
    let l = js::hypot2(d[0], d[1]);
    let l = if l == 0.0 || l.is_nan() { 1e-300 } else { l };
    [d[0] / l, d[1] / l]
}

/// `rangeOf(cv)` → (lo, hi, periodic).
pub fn range_of(cv: &Curve) -> (f64, f64, bool) {
    match cv {
        Curve::Line { .. } => (0.0, 1.0, false),
        Curve::Circle { .. } => (0.0, TAU, true),
        Curve::Arc { sweep, .. } => (0.0, *sweep, false),
        Curve::Spline { bs, closed, .. } => {
            let [lo, hi] = sp::domain(bs);
            (lo, hi, *closed)
        }
    }
}

/// `paramOf(cv, p)`.
pub fn param_of(cv: &Curve, p: P2) -> f64 {
    match cv {
        Curve::Line { a, b, .. } => {
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let den = dx * dx + dy * dy;
            let den = if den == 0.0 || den.is_nan() { 1e-300 } else { den };
            ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / den
        }
        Curve::Circle { c, .. } => norm_ang(js::atan2(p[1] - c[1], p[0] - c[0])),
        Curve::Arc { c, a0, .. } => norm_ang(js::atan2(p[1] - c[1], p[0] - c[0]) - a0),
        Curve::Spline { bs, .. } => sp::closest_param(bs, p, 12),
    }
}

/// `sizeOf(cv)`.
pub fn size_of(cv: &Curve) -> f64 {
    match cv {
        Curve::Line { a, b, .. } => js::max_of([a[0].abs(), a[1].abs(), b[0].abs(), b[1].abs(), js::hypot2(b[0] - a[0], b[1] - a[1])]),
        Curve::Circle { c, r, .. } | Curve::Arc { c, r, .. } => c[0].abs() + c[1].abs() + r,
        Curve::Spline { bs, .. } => js::max_of(bs.poles.iter().map(|q| q[0].abs() + q[1].abs())),
    }
}

fn line_line(a0: P2, a1: P2, b0: P2, b1: P2) -> Option<(f64, f64)> {
    let d1 = [a1[0] - a0[0], a1[1] - a0[1]];
    let d2 = [b1[0] - b0[0], b1[1] - b0[1]];
    let den = d1[0] * d2[1] - d1[1] * d2[0];
    if den.abs() <= 1e-14 * js::hypot2(d1[0], d1[1]) * js::hypot2(d2[0], d2[1]) {
        return None;
    }
    let w = [b0[0] - a0[0], b0[1] - a0[1]];
    Some(((w[0] * d2[1] - w[1] * d2[0]) / den, (w[0] * d1[1] - w[1] * d1[0]) / den))
}

fn line_circle_t(a: P2, b: P2, c: P2, r: f64) -> Vec<f64> {
    let d = [b[0] - a[0], b[1] - a[1]];
    let f = [a[0] - c[0], a[1] - c[1]];
    let aa = d[0] * d[0] + d[1] * d[1];
    let bq = 2.0 * (f[0] * d[0] + f[1] * d[1]);
    let cc = f[0] * f[0] + f[1] * f[1] - r * r;
    let disc = bq * bq - 4.0 * aa * cc;
    let scale = bq * bq + (4.0 * aa * cc).abs();
    if disc < -1e-12 * scale {
        return Vec::new();
    }
    if disc <= 1e-12 * scale {
        return vec![-bq / (2.0 * aa)];
    }
    let sq = disc.sqrt();
    let sb = if bq == 0.0 || bq.is_nan() { 1.0 } else { js::sign(bq) };
    let q = -0.5 * (bq + sb * sq);
    let (t1, t2) = (q / aa, cc / q);
    if t1 < t2 { vec![t1, t2] } else { vec![t2, t1] }
}

fn circle_circle_p(c1: P2, r1: f64, c2: P2, r2: f64) -> Vec<P2> {
    let (dx, dy) = (c2[0] - c1[0], c2[1] - c1[1]);
    let d = js::hypot2(dx, dy);
    if d <= 1e-14 * (r1 + r2) {
        return Vec::new();
    }
    let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
    let h2 = r1 * r1 - a * a;
    let m = [c1[0] + a * dx / d, c1[1] + a * dy / d];
    if h2 < -1e-12 * r1 * r1 {
        return Vec::new();
    }
    if h2 <= 1e-12 * r1 * r1 {
        return vec![m];
    }
    let h = h2.sqrt();
    vec![[m[0] - h * dy / d, m[1] + h * dx / d], [m[0] + h * dy / d, m[1] - h * dx / d]]
}

fn roots1(f: &dyn Fn(f64) -> f64, lo: f64, hi: f64, n: usize, tol_f: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let mut u0 = lo;
    let mut f0 = f(lo);
    if f0.abs() <= tol_f * 1e-3 {
        out.push(lo);
    }
    let mut f_prev: Option<f64> = None;
    let nf = js::f(n);
    for i in 1..=n {
        let u1 = lo + (hi - lo) * js::f(i) / nf;
        let f1 = f(u1);
        if f1 == 0.0 {
            out.push(u1);
        } else if f0 != 0.0 && (f0 < 0.0) != (f1 < 0.0) {
            let (mut a, mut b, mut fa) = (u0, u1, f0);
            let mut it: u32 = 0;
            while it < 200 && b - a > 1e-16 * js::max(1.0, a.abs()) {
                let m = (a + b) / 2.0;
                let fm = f(m);
                if fm == 0.0 {
                    a = m;
                    b = m;
                    break;
                }
                if (fm < 0.0) == (fa < 0.0) {
                    a = m;
                    fa = fm;
                } else {
                    b = m;
                }
                it = it.saturating_add(1);
            }
            out.push((a + b) / 2.0);
        } else if let Some(fp) = f_prev
            && f0.abs() < fp.abs()
            && f0.abs() < f1.abs()
            && f0.abs() <= tol_f
        {
            let mut a = lo + (hi - lo) * (js::f(i) - 2.0) / nf;
            let mut b = u1;
            for _ in 0..100 {
                let m1 = b - (b - a) * 0.618;
                let m2 = a + (b - a) * 0.618;
                if f(m1).abs() < f(m2).abs() {
                    b = m2;
                } else {
                    a = m1;
                }
            }
            let m = (a + b) / 2.0;
            if f(m).abs() <= tol_f * 1e-3 {
                out.push(m);
            }
        }
        f_prev = Some(f0);
        u0 = u1;
        f0 = f1;
    }
    out
}

fn spline_vs_implicit(bs: &sp::Curve, ff: &dyn Fn(P2) -> f64, tol_f: f64) -> Vec<f64> {
    let mut out = Vec::new();
    for [u0, u1] in sp::spans(bs) {
        out.extend(roots1(&|u| ff(sp::point_at(bs, u)), u0, u1, 24, tol_f));
    }
    out
}

fn spline_spline(a: &sp::Curve, b: &sp::Curve, scale: f64) -> Vec<(f64, f64)> {
    let sample = |cv: &sp::Curve| -> Vec<(f64, P2)> {
        let mut pts = Vec::new();
        for [u0, u1] in sp::spans(cv) {
            for i in 0..48 {
                let u = u0 + (u1 - u0) * js::f(i) / 48.0;
                pts.push((u, sp::point_at(cv, u)));
            }
        }
        let hi = sp::domain(cv)[1];
        pts.push((hi, sp::point_at(cv, hi)));
        pts
    };
    let (pa, pb) = (sample(a), sample(b));
    let mut out = Vec::new();
    let [la, ha] = sp::domain(a);
    let [lb, hb] = sp::domain(b);
    for wa in pa.windows(2) {
        let [(ua0, a0), (ua1, a1)] = [wa.first().copied().unwrap_or((0.0, [0.0; 2])), wa.get(1).copied().unwrap_or((0.0, [0.0; 2]))];
        let (ax0, ax1) = (js::min(a0[0], a1[0]), js::max(a0[0], a1[0]));
        let (ay0, ay1) = (js::min(a0[1], a1[1]), js::max(a0[1], a1[1]));
        for wb in pb.windows(2) {
            let [(ub0, b0), (ub1, b1)] = [wb.first().copied().unwrap_or((0.0, [0.0; 2])), wb.get(1).copied().unwrap_or((0.0, [0.0; 2]))];
            if js::max(b0[0], b1[0]) < ax0 || js::min(b0[0], b1[0]) > ax1 || js::max(b0[1], b1[1]) < ay0 || js::min(b0[1], b1[1]) > ay1 {
                continue;
            }
            let Some((ta, tb)) = line_line(a0, a1, b0, b1) else { continue };
            if ta < -0.01 || ta > 1.01 || tb < -0.01 || tb > 1.01 {
                continue;
            }
            let mut u = ua0 + (ua1 - ua0) * ta;
            let mut v = ub0 + (ub1 - ub0) * tb;
            for _ in 0..40 {
                let da = sp::derivs_at(a, u, 1);
                let db = sp::derivs_at(b, v, 1);
                let g = |d: &Vec<P2>, i: usize| d.get(i).copied().unwrap_or([f64::NAN; 2]);
                let (p, dp, q, dq) = (g(&da, 0), g(&da, 1), g(&db, 0), g(&db, 1));
                let r = [p[0] - q[0], p[1] - q[1]];
                let det = -dp[0] * dq[1] + dp[1] * dq[0];
                if det == 0.0 || det.is_nan() {
                    break;
                }
                let du = (-r[0] * -dq[1] + r[1] * -dq[0]) / det;
                let dv = (dp[0] * -r[1] - dp[1] * -r[0]) / det;
                u = js::max(la, js::min(ha, u + du));
                v = js::max(lb, js::min(hb, v + dv));
                if du.abs() + dv.abs() < 1e-15 * js::max(1.0, u.abs() + v.abs()) {
                    break;
                }
            }
            let p = sp::point_at(a, u);
            let q = sp::point_at(b, v);
            if js::hypot2(p[0] - q[0], p[1] - q[1]) <= 1e-9 * scale {
                out.push((u, v));
            }
        }
    }
    out
}

fn within(cv: &Curve, t: f64, eps_len: f64) -> Option<f64> {
    match cv {
        Curve::Line { a, b, .. } => {
            let l = js::hypot2(b[0] - a[0], b[1] - a[1]);
            let l = if l == 0.0 || l.is_nan() { 1e-300 } else { l };
            let e = eps_len / l;
            if t >= -e && t <= 1.0 + e { Some(js::max(0.0, js::min(1.0, t))) } else { None }
        }
        Curve::Circle { .. } => Some(norm_ang(t)),
        Curve::Arc { r, sweep, .. } => {
            let e = eps_len / r;
            let a = norm_ang(t);
            if a <= sweep + e {
                return Some(js::min(a, *sweep));
            }
            if TAU - a <= e {
                return Some(0.0);
            }
            None
        }
        Curve::Spline { .. } => Some(t),
    }
}

/// A crossing: the point and both parameters.
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub p: P2,
    pub ta: f64,
    pub tb: f64,
}

fn sort_by_ta(v: &mut [Hit]) {
    cc_m3d::sort::timsort(v, |x, y| x.ta - y.ta);
}

/// `intersect(A, B, { infA, infB })`.
pub fn intersect(a: &Curve, b: &Curve, inf_a: bool, inf_b: bool) -> Vec<Hit> {
    if matches!(a, Curve::Spline { .. }) && !matches!(b, Curve::Spline { .. }) {
        let mut v: Vec<Hit> = intersect(b, a, inf_b, inf_a).into_iter().map(|x| Hit { p: x.p, ta: x.tb, tb: x.ta }).collect();
        sort_by_ta(&mut v);
        return v;
    }
    let scale = js::max(js::max(1.0, size_of(a)), size_of(b));
    let eps = 1e-9 * scale;
    let raw: Vec<(f64, f64)> = match (a, b) {
        (Curve::Line { a: a0, b: a1, .. }, Curve::Line { a: b0, b: b1, .. }) => line_line(*a0, *a1, *b0, *b1).into_iter().collect(),
        (Curve::Line { a: a0, b: a1, .. }, _) if b.round() => {
            let (c, r) = b.centre();
            line_circle_t(*a0, *a1, c, r).into_iter().map(|t| (t, param_of(b, point_on(a, t)))).collect()
        }
        (_, Curve::Line { a: b0, b: b1, .. }) if a.round() => {
            let (c, r) = a.centre();
            line_circle_t(*b0, *b1, c, r).into_iter().map(|t| (param_of(a, point_on(b, t)), t)).collect()
        }
        _ if a.round() && b.round() => {
            let (c1, r1) = a.centre();
            let (c2, r2) = b.centre();
            circle_circle_p(c1, r1, c2, r2).into_iter().map(|p| (param_of(a, p), param_of(b, p))).collect()
        }
        (Curve::Line { a: la, b: lb, .. }, Curve::Spline { bs, .. }) => {
            let d = [lb[0] - la[0], lb[1] - la[1]];
            let l = js::hypot2(d[0], d[1]);
            let l = if l == 0.0 || l.is_nan() { 1e-300 } else { l };
            let la = *la;
            spline_vs_implicit(bs, &|q| (d[0] * (q[1] - la[1]) - d[1] * (q[0] - la[0])) / l, eps)
                .into_iter()
                .map(|u| (param_of(a, sp::point_at(bs, u)), u))
                .collect()
        }
        (_, Curve::Spline { bs, .. }) if a.round() => {
            let (c, r) = a.centre();
            spline_vs_implicit(bs, &|q| js::hypot2(q[0] - c[0], q[1] - c[1]) - r, eps)
                .into_iter()
                .map(|u| (param_of(a, sp::point_at(bs, u)), u))
                .collect()
        }
        (Curve::Spline { bs: ba, .. }, Curve::Spline { bs: bb, .. }) => spline_spline(ba, bb, scale),
        _ => Vec::new(),
    };
    let mut out: Vec<Hit> = Vec::new();
    for (xa, xb) in raw {
        let ta = if inf_a { if matches!(a, Curve::Arc { .. }) { Some(norm_ang(xa)) } else { Some(xa) } } else { within(a, xa, eps) };
        let tb = if inf_b { if matches!(b, Curve::Arc { .. }) { Some(norm_ang(xb)) } else { Some(xb) } } else { within(b, xb, eps) };
        let (Some(ta), Some(tb)) = (ta, tb) else { continue };
        let p = point_on(a, ta);
        if out.iter().any(|y| js::hypot2(y.p[0] - p[0], y.p[1] - p[1]) <= 10.0 * eps) {
            continue;
        }
        out.push(Hit { p, ta, tb });
    }
    sort_by_ta(&mut out);
    out
}

/// The curve as the JS object (for the parity harness).
pub fn curve_json(cv: &Curve) -> serde_json::Value {
    use crate::jv;
    match cv {
        Curve::Line { id, a, b } => {
            jv::obj(vec![("kind", "line".into()), ("id", id.as_str().into()), ("a", jv::pt2(*a)), ("b", jv::pt2(*b))])
        }
        Curve::Circle { id, c, r } => {
            jv::obj(vec![("kind", "circle".into()), ("id", id.as_str().into()), ("c", jv::pt2(*c)), ("r", jv::num(*r))])
        }
        Curve::Arc { id, c, r, a0, sweep } => jv::obj(vec![
            ("kind", "arc".into()),
            ("id", id.as_str().into()),
            ("c", jv::pt2(*c)),
            ("r", jv::num(*r)),
            ("a0", jv::num(*a0)),
            ("sweep", jv::num(*sweep)),
        ]),
        Curve::Spline { id, bs, closed } => {
            jv::obj(vec![("kind", "spline".into()), ("id", id.as_str().into()), ("bs", sp::curve_json(bs)), ("closed", (*closed).into())])
        }
    }
}
/// A curve from its JS object (the harness hands curves in).
pub fn curve_from_json(v: &serde_json::Value, bs: Option<sp::Curve>) -> Option<Curve> {
    use crate::jv;
    let p = |k: &str| {
        let a = jv::arr(v.get(k));
        [jv::as_num(a.first()).unwrap_or(f64::NAN), jv::as_num(a.get(1)).unwrap_or(f64::NAN)]
    };
    let id = jv::s(v, "id").to_string();
    Some(match jv::s(v, "kind") {
        "line" => Curve::Line { id, a: p("a"), b: p("b") },
        "circle" => Curve::Circle { id, c: p("c"), r: jv::n(v, "r") },
        "arc" => Curve::Arc { id, c: p("c"), r: jv::n(v, "r"), a0: jv::n(v, "a0"), sweep: jv::n(v, "sweep") },
        "spline" => Curve::Spline { id, bs: bs?, closed: jv::truthy(v.get("closed")) },
        _ => return None,
    })
}
