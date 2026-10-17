//! sketch-spline.js — cubic B-spline math for sketch splines: de Boor evaluation (The NURBS Book A2.2/A2.3), chord-length
//! interpolation (GeomAPI_Interpolate's parametrisation), clamped / periodic control splines, tessellation, exact area
//! (3-point Gauss), closest point, knot insertion (Boehm), exact segments, reversal, curvature and the offset fit.
//!
//! An index read past an array gives NaN (the JS reads `undefined`); index arithmetic saturates.

use crate::i18n::{F, t};
use crate::js;

pub const DEG: usize = 3;
pub type P2 = [f64; 2];

/// A flat B-spline `{ deg, knots, poles, periodic?, n?, P?, tau? }`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Curve {
    pub deg: usize,
    pub knots: Vec<f64>,
    pub poles: Vec<P2>,
    pub periodic: bool,
    pub n: usize,
    pub pp: Vec<P2>,
    pub tau: Vec<f64>,
}

#[inline]
fn kv(u: &[f64], i: usize) -> f64 {
    u.get(i).copied().unwrap_or(f64::NAN)
}
#[inline]
fn pv(p: &[P2], i: usize) -> P2 {
    p.get(i).copied().unwrap_or([f64::NAN, f64::NAN])
}
#[inline]
fn a(x: usize, y: usize) -> usize {
    x.saturating_add(y)
}
#[inline]
fn s(x: usize, y: usize) -> usize {
    x.saturating_sub(y)
}

/// `findSpan(U, p, m, u)`.
pub fn find_span(uu: &[f64], p: usize, m: usize, u: f64) -> usize {
    if u >= kv(uu, m) {
        let mut k = s(m, 1);
        while k > p && kv(uu, k) >= kv(uu, a(k, 1)) {
            k = s(k, 1);
        }
        return k;
    }
    if u <= kv(uu, p) {
        let mut k = p;
        while k < s(m, 1) && kv(uu, a(k, 1)) <= u {
            k = a(k, 1);
        }
        return k;
    }
    let (mut lo, mut hi) = (p, s(m, 1));
    while lo < hi {
        let mid = a(a(lo, hi), 1) >> 1;
        if kv(uu, mid) <= u {
            lo = mid;
        } else {
            hi = s(mid, 1);
        }
    }
    lo
}

/// `basisFuns(k, u, p, U)`.
pub fn basis_funs(k: usize, u: f64, p: usize, uu: &[f64]) -> Vec<f64> {
    let mut nn = vec![0.0; a(p, 1)];
    if let Some(x) = nn.first_mut() {
        *x = 1.0;
    }
    let mut left = vec![0.0; a(p, 1)];
    let mut right = vec![0.0; a(p, 1)];
    for j in 1..=p {
        if let Some(x) = left.get_mut(j) {
            *x = u - kv(uu, s(a(k, 1), j));
        }
        if let Some(x) = right.get_mut(j) {
            *x = kv(uu, a(k, j)) - u;
        }
        let mut saved = 0.0;
        for r in 0..j {
            let den = kv(&right, a(r, 1)) + kv(&left, s(j, r));
            let tt = if den == 0.0 { 0.0 } else { kv(&nn, r) / den };
            if let Some(x) = nn.get_mut(r) {
                *x = saved + kv(&right, a(r, 1)) * tt;
            }
            saved = kv(&left, s(j, r)) * tt;
        }
        if let Some(x) = nn.get_mut(j) {
            *x = saved;
        }
    }
    nn
}

/// `dersBasisFuns(k, u, p, n, U)`: ders[i][j] = i-th derivative of basis function k − p + j.
pub fn ders_basis_funs(k: usize, u: f64, p: usize, n: usize, uu: &[f64]) -> Vec<Vec<f64>> {
    let w = a(p, 1);
    let mut ndu = vec![vec![0.0; w]; w];
    if let Some(r) = ndu.first_mut()
        && let Some(x) = r.first_mut()
    {
        *x = 1.0;
    }
    let get = |m: &Vec<Vec<f64>>, i: usize, j: usize| m.get(i).and_then(|r| r.get(j)).copied().unwrap_or(f64::NAN);
    let put = |m: &mut Vec<Vec<f64>>, i: usize, j: usize, v: f64| {
        if let Some(x) = m.get_mut(i).and_then(|r| r.get_mut(j)) {
            *x = v;
        }
    };
    let mut left = vec![0.0; w];
    let mut right = vec![0.0; w];
    for j in 1..=p {
        if let Some(x) = left.get_mut(j) {
            *x = u - kv(uu, s(a(k, 1), j));
        }
        if let Some(x) = right.get_mut(j) {
            *x = kv(uu, a(k, j)) - u;
        }
        let mut saved = 0.0;
        for r in 0..j {
            let v = kv(&right, a(r, 1)) + kv(&left, s(j, r));
            put(&mut ndu, j, r, v);
            let tt = if v == 0.0 { 0.0 } else { get(&ndu, r, s(j, 1)) / v };
            put(&mut ndu, r, j, saved + kv(&right, a(r, 1)) * tt);
            saved = kv(&left, s(j, r)) * tt;
        }
        put(&mut ndu, j, j, saved);
    }
    let mut ders = vec![vec![0.0; w]; a(n, 1)];
    for j in 0..=p {
        let v = get(&ndu, j, p);
        put(&mut ders, 0, j, v);
    }
    let pi = |x: usize| -> i64 { i64::try_from(x).unwrap_or(i64::MAX) };
    for r in 0..=p {
        let (mut s1, mut s2) = (0usize, 1usize);
        let mut aa = vec![vec![0.0; w]; 2];
        put(&mut aa, 0, 0, 1.0);
        for kk in 1..=n {
            let mut d = 0.0;
            let rk = pi(r).saturating_sub(pi(kk));
            let pk = pi(p).saturating_sub(pi(kk));
            let ix = |x: i64| usize::try_from(x).unwrap_or(usize::MAX);
            if pi(r) >= pi(kk) {
                let den = get(&ndu, ix(pk.saturating_add(1)), ix(rk));
                let v = if den == 0.0 { 0.0 } else { get(&aa, s1, 0) / den };
                put(&mut aa, s2, 0, v);
                d = v * get(&ndu, ix(rk), ix(pk));
            }
            let j1: i64 = if rk >= -1 { 1 } else { rk.saturating_neg() };
            let j2: i64 = if pi(r).saturating_sub(1) <= pk { pi(kk).saturating_sub(1) } else { pi(p).saturating_sub(pi(r)) };
            let mut j = j1;
            while j <= j2 {
                let den = get(&ndu, ix(pk.saturating_add(1)), ix(rk.saturating_add(j)));
                let v = if den == 0.0 { 0.0 } else { (get(&aa, s1, ix(j)) - get(&aa, s1, ix(j.saturating_sub(1)))) / den };
                put(&mut aa, s2, ix(j), v);
                d += v * get(&ndu, ix(rk.saturating_add(j)), ix(pk));
                j = j.saturating_add(1);
            }
            if pi(r) <= pk {
                let den = get(&ndu, ix(pk.saturating_add(1)), r);
                let v = if den == 0.0 { 0.0 } else { -get(&aa, s1, s(kk, 1)) / den };
                put(&mut aa, s2, kk, v);
                d += v * get(&ndu, r, ix(pk));
            }
            put(&mut ders, kk, r, d);
            std::mem::swap(&mut s1, &mut s2);
        }
    }
    let mut f = js::f(p);
    for kk in 1..=n {
        for j in 0..=p {
            let v = get(&ders, kk, j) * f;
            put(&mut ders, kk, j, v);
        }
        f *= js::f(p) - js::f(kk);
    }
    ders
}

/// `domain(c)`.
pub fn domain(c: &Curve) -> [f64; 2] {
    [kv(&c.knots, c.deg), kv(&c.knots, c.poles.len())]
}

/// `pointAt(c, u)`.
pub fn point_at(c: &Curve, u: f64) -> P2 {
    let p = c.deg;
    let k = find_span(&c.knots, p, c.poles.len(), u);
    let nn = basis_funs(k, u, p, &c.knots);
    let (mut x, mut y) = (0.0, 0.0);
    for j in 0..=p {
        let q = pv(&c.poles, a(s(k, p), j));
        let b = kv(&nn, j);
        x += b * q[0];
        y += b * q[1];
    }
    [x, y]
}

/// `derivsAt(c, u, n)`: [C(u), C′(u), …].
pub fn derivs_at(c: &Curve, u: f64, n: usize) -> Vec<P2> {
    let p = c.deg;
    let k = find_span(&c.knots, p, c.poles.len(), u);
    let d = ders_basis_funs(k, u, p, n.min(p), &c.knots);
    let mut out = Vec::with_capacity(a(n, 1));
    for i in 0..=n {
        let (mut x, mut y) = (0.0, 0.0);
        if i <= p {
            for j in 0..=p {
                let q = pv(&c.poles, a(s(k, p), j));
                let b = d.get(i).and_then(|r| r.get(j)).copied().unwrap_or(f64::NAN);
                x += b * q[0];
                y += b * q[1];
            }
        }
        out.push([x, y]);
    }
    out
}

/// `spans(c)`: the non-empty knot spans.
pub fn spans(c: &Curve) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    for k in c.deg..c.poles.len() {
        let (u0, u1) = (kv(&c.knots, k), kv(&c.knots, a(k, 1)));
        if u1 > u0 {
            out.push([u0, u1]);
        }
    }
    out
}

/// `periodicCurve(P, tau, periods)`.
pub fn periodic_curve(pp: &[P2], tau: &[f64], periods: usize) -> Curve {
    let n = pp.len();
    let tt = kv(tau, n) - kv(tau, 0);
    let ni = i64::try_from(n).unwrap_or(1).max(1);
    let tk = |i: i64| -> f64 {
        let q = i.div_euclid(ni);
        let r = usize::try_from(i.saturating_sub(q.saturating_mul(ni))).unwrap_or(0);
        kv(tau, r) + (q as f64) * tt
    };
    let m = a(periods.saturating_mul(n), DEG);
    let mut knots = Vec::with_capacity(a(m, a(DEG, 1)));
    let d = i64::try_from(DEG).unwrap_or(3);
    for j in 0..=a(m, DEG) {
        knots.push(tk(i64::try_from(j).unwrap_or(0).saturating_sub(d)));
    }
    let mut poles = Vec::with_capacity(m);
    for j in 0..m {
        poles.push(if n == 0 { [f64::NAN, f64::NAN] } else { pv(pp, js::rem(j, n)) });
    }
    Curve { deg: DEG, knots, poles, periodic: true, n, pp: pp.to_vec(), tau: tau.to_vec() }
}

/// `clampedUniform(m, p)`.
pub fn clamped_uniform(m: usize, p: usize) -> Vec<f64> {
    let mut uu = vec![0.0; a(p, 1)];
    for i in 1..=s(s(m, p), 1) {
        uu.push(js::f(i));
    }
    uu.extend(std::iter::repeat_n(js::f(s(m, p)), a(p, 1)));
    uu
}

/// `controlCurve(P, { closed, knots })`.
pub fn control_curve(pp: &[P2], closed: bool, knots: Option<&[f64]>) -> Curve {
    if closed {
        let tau: Vec<f64> = (0..=pp.len()).map(js::f).collect();
        return periodic_curve(pp, &tau, 1);
    }
    let p = DEG.min(s(pp.len(), 1));
    Curve { deg: p, knots: knots.map_or_else(|| clamped_uniform(pp.len(), p), <[f64]>::to_vec), poles: pp.to_vec(), ..Curve::default() }
}

/// `knotProblem(U, m)`: the file reader's message, or None.
pub fn knot_problem(uu: Option<&serde_json::Value>, m: usize) -> Option<String> {
    let p = DEG.min(s(m, 1));
    let list: Option<Vec<f64>> = uu.and_then(|v| v.as_array()).and_then(|arr| {
        if arr.len() != a(a(m, p), 1) {
            return None;
        }
        arr.iter().map(|x| x.as_f64().filter(|v| v.is_finite() && v.abs() < 1e9)).collect()
    });
    let Some(uu) = list else { return Some(t("model.sketch_spline.knots.mismatch", &[])) };
    for i in 1..uu.len() {
        if kv(&uu, i) < kv(&uu, s(i, 1)) {
            return Some(t("model.sketch_spline.knots.backwards", &[]));
        }
    }
    let last = s(uu.len(), 1);
    for i in 1..=p {
        if kv(&uu, i) != kv(&uu, 0) || kv(&uu, s(last, i)) != kv(&uu, last) {
            return Some(t("model.sketch_spline.knots.unclamped", &[]));
        }
    }
    if !(kv(&uu, last) > kv(&uu, 0)) {
        return Some(t("model.sketch_spline.knots.no_length", &[]));
    }
    let mut run = 1usize;
    let end = s(s(uu.len(), p), 1);
    for i in a(p, 1)..end {
        run = if kv(&uu, i) == kv(&uu, s(i, 1)) { a(run, 1) } else { 1 };
        if run > p {
            return Some(t("model.sketch_spline.knots.repeats", &[]));
        }
    }
    None
}

fn chord_params(q: &[P2], closed: bool) -> Result<Vec<f64>, String> {
    let mut tt = vec![0.0];
    let n = q.len();
    let end = a(n, usize::from(closed));
    for i in 1..end {
        let x = pv(q, s(i, 1));
        let y = pv(q, js::rem(i, n));
        let d = js::hypot2(y[0] - x[0], y[1] - x[1]);
        if !(d > 0.0) {
            return Err(t("model.sketch_spline.fit_points_coincide", &[]));
        }
        tt.push(kv(&tt, s(i, 1)) + d);
    }
    Ok(tt)
}

/// `solveDense(A, B)`: Gaussian elimination with partial pivoting (B: columns).
fn solve_dense(aa: &[Vec<f64>], bb: &[Vec<f64>]) -> Result<Vec<Vec<f64>>, String> {
    let n = aa.len();
    let w = bb.first().map_or(0, Vec::len);
    let mut m: Vec<Vec<f64>> = aa
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let mut row = r.clone();
            row.extend(bb.get(i).cloned().unwrap_or_default());
            row
        })
        .collect();
    let g = |m: &Vec<Vec<f64>>, i: usize, j: usize| m.get(i).and_then(|r| r.get(j)).copied().unwrap_or(f64::NAN);
    for c in 0..n {
        let mut piv = c;
        for r in a(c, 1)..n {
            if g(&m, r, c).abs() > g(&m, piv, c).abs() {
                piv = r;
            }
        }
        if !(g(&m, piv, c).abs() > 1e-300) {
            return Err(t("model.sketch_spline.cannot_pass", &[]));
        }
        m.swap(c, piv);
        for r in 0..n {
            if r == c || g(&m, r, c) == 0.0 {
                continue;
            }
            let f = g(&m, r, c) / g(&m, c, c);
            for k in c..a(n, w) {
                let v = g(&m, r, k) - f * g(&m, c, k);
                if let Some(x) = m.get_mut(r).and_then(|row| row.get_mut(k)) {
                    *x = v;
                }
            }
        }
    }
    Ok(m.iter().enumerate().map(|(i, r)| r.iter().skip(n).map(|v| v / g(&m, i, i)).collect()).collect())
}

/// `interpFit(Q, { closed, a0, a1 })`.
pub fn interp_fit(q: &[P2], closed: bool, a0: Option<f64>, a1: Option<f64>) -> Result<Curve, String> {
    if closed {
        let n = q.len();
        if n < 3 {
            return Err(t("model.sketch_spline.closed_three", &[]));
        }
        let tau = chord_params(q, true)?;
        let flat = periodic_curve(q, &tau, 1);
        let mut aa = vec![vec![0.0; n]; n];
        for (i, row) in aa.iter_mut().enumerate() {
            let k = a(i, DEG);
            let nn = basis_funs(k, kv(&tau, i), DEG, &flat.knots);
            for j in 0..=DEG {
                if let Some(x) = row.get_mut(js::rem(a(s(k, DEG), j), n)) {
                    *x += kv(&nn, j);
                }
            }
        }
        let bb: Vec<Vec<f64>> = q.iter().map(|p| vec![p[0], p[1]]).collect();
        let sol = solve_dense(&aa, &bb)?;
        let pp: Vec<P2> = sol.iter().map(|r| [kv(r, 0), kv(r, 1)]).collect();
        return Ok(periodic_curve(&pp, &tau, 1));
    }
    let n1 = q.len();
    if n1 < 2 {
        return Err(t("model.sketch_spline.open_two", &[]));
    }
    let tt = chord_params(q, false)?;
    let t0 = kv(&tt, 0);
    let tl = kv(&tt, s(n1, 1));
    let mut uu = vec![t0, t0, t0, t0];
    uu.extend(tt.iter().skip(1).take(s(n1, 2)).copied());
    uu.extend([tl, tl, tl, tl]);
    let m = a(n1, 2);
    let p = DEG;
    let mut aa: Vec<Vec<f64>> = Vec::new();
    let mut bb: Vec<Vec<f64>> = Vec::new();
    let mut row = |u: f64, d: usize, rhs: Vec<f64>| {
        let k = find_span(&uu, p, m, u);
        let dd = ders_basis_funs(k, u, p, d, &uu);
        let mut r = vec![0.0; m];
        for j in 0..=p {
            if let Some(x) = r.get_mut(a(s(k, p), j)) {
                *x = dd.get(d).and_then(|row| row.get(j)).copied().unwrap_or(f64::NAN);
            }
        }
        aa.push(r);
        bb.push(rhs);
    };
    let end = |row: &mut dyn FnMut(f64, usize, Vec<f64>), u: f64, ang: Option<f64>| match ang.filter(|x| x.is_finite()) {
        Some(g) => row(u, 1, vec![js::cos(g), js::sin(g)]),
        None => row(u, 2, vec![0.0, 0.0]),
    };
    let q0 = pv(q, 0);
    row(t0, 0, vec![q0[0], q0[1]]);
    end(&mut row, t0, a0);
    for i in 1..s(n1, 1) {
        let qi = pv(q, i);
        row(kv(&tt, i), 0, vec![qi[0], qi[1]]);
    }
    end(&mut row, tl, a1);
    let ql = pv(q, s(n1, 1));
    row(tl, 0, vec![ql[0], ql[1]]);
    let sol = solve_dense(&aa, &bb)?;
    let mut pp: Vec<P2> = sol.iter().map(|r| [kv(r, 0), kv(r, 1)]).collect();
    if let Some(x) = pp.first_mut() {
        *x = q0;
    }
    if let Some(x) = pp.get_mut(s(m, 1)) {
        *x = ql;
    }
    Ok(Curve { deg: p, knots: uu, poles: pp, ..Curve::default() })
}

/// `occForm(c)`: distinct knots + multiplicities.
pub fn occ_form(c: &Curve) -> (Vec<P2>, Vec<f64>, Vec<usize>, usize, bool) {
    if c.periodic {
        return (c.pp.clone(), c.tau.clone(), vec![1; c.tau.len()], c.deg, true);
    }
    let mut knots: Vec<f64> = Vec::new();
    let mut mults: Vec<usize> = Vec::new();
    for &u in &c.knots {
        if knots.last() == Some(&u) {
            if let Some(m) = mults.last_mut() {
                *m = a(*m, 1);
            }
        } else {
            knots.push(u);
            mults.push(1);
        }
    }
    (c.poles.clone(), knots, mults, c.deg, false)
}

/// `tessellate(c, perSpan)`.
pub fn tessellate(c: &Curve, per_span: usize) -> Vec<P2> {
    let mut out = Vec::new();
    let ps = js::f(per_span);
    for [u0, u1] in spans(c) {
        for i in 0..per_span {
            out.push(point_at(c, u0 + (u1 - u0) * js::f(i) / ps));
        }
    }
    out.push(point_at(c, domain(c)[1]));
    out
}

/// `tessellateTol(c, tol)`.
pub fn tessellate_tol(c: &Curve, tol: f64) -> Vec<P2> {
    let mut out = Vec::new();
    fn rec(c: &Curve, tol: f64, out: &mut Vec<P2>, u0: f64, p0: P2, u1: f64, p1: P2, depth: u32) {
        let mut worst: f64 = 0.0;
        for f in [0.25, 0.5, 0.75] {
            let q = point_at(c, u0 + (u1 - u0) * f);
            let (dx, dy) = (p1[0] - p0[0], p1[1] - p0[1]);
            let l = js::hypot2(dx, dy);
            let d = if l > 0.0 { (dx * (q[1] - p0[1]) - dy * (q[0] - p0[0])).abs() / l } else { js::hypot2(q[0] - p0[0], q[1] - p0[1]) };
            worst = js::max(worst, d);
        }
        if worst <= tol || depth > 14 {
            out.push(p1);
            return;
        }
        let um = (u0 + u1) / 2.0;
        let pm = point_at(c, um);
        rec(c, tol, out, u0, p0, um, pm, depth.saturating_add(1));
        rec(c, tol, out, um, pm, u1, p1, depth.saturating_add(1));
    }
    let sp = spans(c);
    let Some(first) = sp.first() else { return out };
    out.push(point_at(c, first[0]));
    for [u0, u1] in sp {
        for i in 0..4 {
            let ua = u0 + (u1 - u0) * js::f(i) / 4.0;
            let ub = u0 + (u1 - u0) * js::f(a(i, 1)) / 4.0;
            rec(c, tol, &mut out, ua, point_at(c, ua), ub, point_at(c, ub), 0);
        }
    }
    out
}

fn gl3() -> [[f64; 2]; 3] {
    let r = (3.0f64 / 5.0).sqrt();
    [[-r, 5.0 / 9.0], [0.0, 8.0 / 9.0], [r, 5.0 / 9.0]]
}

/// `areaTerm(c)`: ½∫(x y′ − y x′) du (exact for a cubic).
pub fn area_term(c: &Curve) -> f64 {
    let mut sum = 0.0;
    for [u0, u1] in spans(c) {
        let h = (u1 - u0) / 2.0;
        let m = (u0 + u1) / 2.0;
        for [g, w] in gl3() {
            let d = derivs_at(c, m + h * g, 1);
            let (p, dd) = (pv(&d, 0), pv(&d, 1));
            sum += w * h * (p[0] * dd[1] - p[1] * dd[0]) / 2.0;
        }
    }
    sum
}

/// `arcLength(c)`.
pub fn arc_length(c: &Curve) -> f64 {
    let s70 = 70.0f64.sqrt();
    let g5 = [
        [0.0, 128.0 / 225.0],
        [(5.0 - 2.0 * (10.0f64 / 7.0).sqrt()).sqrt() / 3.0, (322.0 + 13.0 * s70) / 900.0],
        [(5.0 + 2.0 * (10.0f64 / 7.0).sqrt()).sqrt() / 3.0, (322.0 - 13.0 * s70) / 900.0],
    ];
    let mut sum = 0.0;
    for [u0, u1] in spans(c) {
        for i in 0..8 {
            let aa = u0 + (u1 - u0) * js::f(i) / 8.0;
            let b = u0 + (u1 - u0) * js::f(a(i, 1)) / 8.0;
            let h = (b - aa) / 2.0;
            let m = (aa + b) / 2.0;
            for [g, w] in g5 {
                let signs: &[f64] = if g != 0.0 { &[-1.0, 1.0] } else { &[1.0] };
                for &sg in signs {
                    let d = pv(&derivs_at(c, m + sg * h * g, 1), 1);
                    sum += w * h * js::hypot2(d[0], d[1]);
                }
            }
        }
    }
    sum
}

/// `closestParam(c, p, perSpan)`.
pub fn closest_param(c: &Curve, p: P2, per_span: usize) -> f64 {
    let [lo, hi] = domain(c);
    let mut best = lo;
    let mut bd = f64::INFINITY;
    let ps = js::f(per_span);
    for [u0, u1] in spans(c) {
        for i in 0..=per_span {
            let u = u0 + (u1 - u0) * js::f(i) / ps;
            let q = point_at(c, u);
            let d = js::pow(q[0] - p[0], 2.0) + js::pow(q[1] - p[1], 2.0);
            if d < bd {
                bd = d;
                best = u;
            }
        }
    }
    let mut u = best;
    for _ in 0..30 {
        let d = derivs_at(c, u, 2);
        let (q, d1, d2) = (pv(&d, 0), pv(&d, 1), pv(&d, 2));
        let r = [q[0] - p[0], q[1] - p[1]];
        let f = r[0] * d1[0] + r[1] * d1[1];
        let df = d1[0] * d1[0] + d1[1] * d1[1] + r[0] * d2[0] + r[1] * d2[1];
        if !(df > 0.0) {
            break;
        }
        let mut un = u - f / df;
        if c.periodic {
            let tt = hi - lo;
            let mut guard = 0u32;
            while un < lo && guard < 100_000 {
                un += tt;
                guard = guard.saturating_add(1);
            }
            while un > hi && guard < 200_000 {
                un -= tt;
                guard = guard.saturating_add(1);
            }
        } else {
            un = js::max(lo, js::min(hi, un));
        }
        if (un - u).abs() < 1e-15 * js::max(1.0, u.abs()) {
            u = un;
            break;
        }
        u = un;
    }
    u
}

/// `curvatureAt(c, u)` (left turn positive).
pub fn curvature_at(c: &Curve, u: f64) -> f64 {
    let d = derivs_at(c, u, 2);
    let (d1, d2) = (pv(&d, 1), pv(&d, 2));
    let sp = js::hypot2(d1[0], d1[1]);
    if sp > 0.0 { (d1[0] * d2[1] - d1[1] * d2[0]) / (sp * sp * sp) } else { 0.0 }
}

/// `insertKnot(c, u)` (Boehm), an open flat curve.
pub fn insert_knot(c: &Curve, u: f64) -> Curve {
    let p = c.deg;
    let m = c.poles.len();
    let k = find_span(&c.knots, p, m, u);
    let mut q: Vec<P2> = Vec::with_capacity(a(m, 1));
    for i in 0..=s(k, p) {
        q.push(pv(&c.poles, i));
    }
    for i in a(s(k, p), 1)..=k {
        let al = (u - kv(&c.knots, i)) / (kv(&c.knots, a(i, p)) - kv(&c.knots, i));
        let (pa, pb) = (pv(&c.poles, s(i, 1)), pv(&c.poles, i));
        q.push([(1.0 - al) * pa[0] + al * pb[0], (1.0 - al) * pa[1] + al * pb[1]]);
    }
    for i in k..m {
        q.push(pv(&c.poles, i));
    }
    let mut kk: Vec<f64> = c.knots.iter().take(a(k, 1)).copied().collect();
    kk.push(u);
    kk.extend(c.knots.iter().skip(a(k, 1)).copied());
    Curve { deg: p, knots: kk, poles: q, ..Curve::default() }
}

fn mult_of(uu: &[f64], u: f64) -> usize {
    uu.iter().filter(|&&v| v == u).count()
}

fn raise(c: &Curve, u0: f64, eps: f64) -> (Curve, f64) {
    let mut u = u0;
    if let Some(near) = c.knots.iter().find(|&&v| (v - u).abs() <= eps) {
        u = *near;
    }
    let mut cur = Curve { deg: c.deg, knots: c.knots.clone(), poles: c.poles.clone(), ..Curve::default() };
    let mut guard = 0usize;
    while mult_of(&cur.knots, u) < c.deg && guard < 64 {
        cur = insert_knot(&cur, u);
        guard = a(guard, 1);
    }
    (cur, u)
}

/// `segment(c, ua, ub)`: the exact piece between ua < ub, open clamped.
pub fn segment(c: &Curve, ua: f64, ub: f64) -> Curve {
    let [lo, hi] = domain(c);
    let src = if c.periodic && ub > hi + 1e-12 * js::max(1.0, hi.abs()) { periodic_curve(&c.pp, &c.tau, 2) } else { c.clone() };
    let eps = 1e-12 * js::max(js::max(js::max(1.0, lo.abs()), hi.abs()), hi - lo);
    let (r1, aa) = raise(&src, ua, eps);
    let (r2, bu) = raise(&r1, ub, eps);
    let (kk, pp, p) = (&r2.knots, &r2.poles, c.deg);
    let mut last_a: i64 = -1;
    let mut first_b: i64 = -1;
    for (j, &v) in kk.iter().enumerate() {
        let jj = i64::try_from(j).unwrap_or(-1);
        if v == aa {
            last_a = jj;
        }
        if v == bu && first_b < 0 {
            first_b = jj;
        }
    }
    let pi = i64::try_from(p).unwrap_or(3);
    let i0 = last_a.saturating_sub(pi).saturating_add(1);
    let i1 = first_b.saturating_add(pi).saturating_sub(1);
    let mut knots = vec![aa];
    if i0 >= 0 && i1 >= i0 {
        let (b0, b1) = (usize::try_from(i0).unwrap_or(0), usize::try_from(i1).unwrap_or(0));
        knots.extend(kk.iter().skip(b0).take(a(s(b1, b0), 1)).copied());
    }
    knots.push(bu);
    let np = s(s(knots.len(), p), 1);
    let start = usize::try_from(i0.saturating_sub(1)).unwrap_or(usize::MAX);
    let poles: Vec<P2> = (0..np).map(|k| pv(pp, a(start, k))).collect();
    Curve { deg: p, knots, poles, ..Curve::default() }
}

/// `reverse(c)`.
pub fn reverse(c: &Curve) -> Curve {
    let uu = &c.knots;
    let (u0, u1) = (kv(uu, 0), kv(uu, s(uu.len(), 1)));
    let n = uu.len();
    let knots = (0..n).map(|j| u0 + u1 - kv(uu, s(s(n, 1), j))).collect();
    let mut poles = c.poles.clone();
    poles.reverse();
    Curve { deg: c.deg, knots, poles, ..Curve::default() }
}

/// `normalizeKnots(c)`.
pub fn normalize_knots(c: &Curve) -> Curve {
    let u0 = kv(&c.knots, 0);
    let l = kv(&c.knots, s(c.knots.len(), 1)) - u0;
    let sc = if l > 0.0 { (js::f(c.poles.len()) - js::f(c.deg)) / l } else { 1.0 };
    Curve { deg: c.deg, knots: c.knots.iter().map(|u| (u - u0) * sc).collect(), poles: c.poles.clone(), ..Curve::default() }
}

/// `offsetFit(c, d, tol)`'s result.
#[derive(Clone, Debug)]
pub struct OffsetFit {
    pub q: Vec<P2>,
    pub a0: Option<f64>,
    pub a1: Option<f64>,
    pub closed: bool,
    pub err: f64,
    pub curve: Curve,
}

/// `offsetFit(c, d, tol)`.
pub fn offset_fit(c: &Curve, d: f64, tol: f64) -> Result<OffsetFit, String> {
    let sp = spans(c);
    let off = |u: f64| -> P2 {
        let dd = derivs_at(c, u, 1);
        let (q, tt) = (pv(&dd, 0), pv(&dd, 1));
        let l = js::hypot2(tt[0], tt[1]);
        [q[0] - d * tt[1] / l, q[1] + d * tt[0] / l]
    };
    for &[u0, u1] in &sp {
        for i in 0..=32 {
            let k = curvature_at(c, u0 + (u1 - u0) * js::f(i) / 32.0);
            if k * d > 0.0 && d.abs() >= 1.0 / k.abs() - 1e-12 {
                return Err(t("model.sketch_spline.offset_too_big", &[("r", F::N(js::fixed_num(1.0 / k.abs(), 3)))]));
            }
        }
    }
    let closed = c.periodic;
    let mut truth = Vec::new();
    for &[u0, u1] in &sp {
        for i in 0..40 {
            truth.push(off(u0 + (u1 - u0) * (js::f(i) + 0.5) / 40.0));
        }
    }
    let angle = |u: f64| {
        let tt = pv(&derivs_at(c, u, 1), 1);
        js::atan2(tt[1], tt[0])
    };
    let [lo, hi] = domain(c);
    let mut per = 2usize;
    while per <= 64 {
        let mut q = Vec::new();
        for &[u0, u1] in &sp {
            for i in 0..per {
                q.push(off(u0 + (u1 - u0) * js::f(i) / js::f(per)));
            }
        }
        if !closed {
            q.push(off(hi));
        }
        let (a0, a1) = if closed { (None, None) } else { (Some(angle(lo)), Some(angle(hi))) };
        if let Ok(fit) = interp_fit(&q, closed, a0, a1) {
            let mut err: f64 = 0.0;
            for p in &truth {
                let qq = point_at(&fit, closest_param(&fit, *p, 6));
                err = js::max(err, js::hypot2(qq[0] - p[0], qq[1] - p[1]));
            }
            if err <= tol {
                return Ok(OffsetFit { q, a0, a1, closed, err, curve: fit });
            }
        }
        per = per.saturating_mul(2);
    }
    Err(t("model.sketch_spline.offset_tolerance", &[("tol", F::N(tol))]))
}

/// The curve as the JS object (for the parity harness).
pub fn curve_json(c: &Curve) -> serde_json::Value {
    use crate::jv;
    let poles = serde_json::Value::Array(c.poles.iter().map(|p| jv::pt2(*p)).collect());
    let mut o = jv::obj(vec![("deg", jv::unum(c.deg)), ("knots", jv::nums(&c.knots)), ("poles", poles)]);
    if c.periodic {
        jv::set(&mut o, "periodic", true.into());
        jv::set(&mut o, "n", jv::unum(c.n));
        jv::set(&mut o, "P", serde_json::Value::Array(c.pp.iter().map(|p| jv::pt2(*p)).collect()));
        jv::set(&mut o, "tau", jv::nums(&c.tau));
    }
    o
}
