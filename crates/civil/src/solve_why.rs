//! solve-why.js — when an alignment's curves or a profile's vertical curves have no solution, why, and the radius or
//! length that would fit (0.5 WL.23).

use crate::geo::P2;
use crate::js::{JsMath, hypot, max, min};
use crate::profile::Pvi;
use serde_json::Value;
use std::f64::consts::PI;

fn ang(a: P2, b: P2, c: P2) -> f64 {
    let a1 = (b.y - a.y).jatan2(b.x - a.x);
    let a2 = (c.y - b.y).jatan2(c.x - b.x);
    let mut d = a2 - a1;
    let mut g = 0;
    while d > PI && g < 1000 {
        d -= 2.0 * PI;
        g += 1;
    }
    while d < -PI && g < 2000 {
        d += 2.0 * PI;
        g += 1;
    }
    d
}
fn down(v: f64) -> f64 {
    (v * 100.0 + 1e-9).floor() / 100.0
}
fn up(v: f64) -> f64 {
    (v * 100.0 - 1e-9).ceil() / 100.0
}
fn dist(p: P2, q: P2) -> f64 {
    hypot(q.x - p.x, q.y - p.y)
}

/// A PI with its radius as given (`Number(p.r) > 0 ? Number(p.r) : 0` is applied here).
#[derive(Clone, Copy, Debug)]
pub struct WPi {
    pub p: P2,
    pub r: f64,
}
#[derive(Clone, Copy, Debug, Default)]
struct C {
    d: f64,
    r: f64,
    t: f64,
    tan_half: f64,
    back: bool,
}
fn curves_of(pis: &[WPi]) -> Vec<C> {
    let n = pis.len();
    (0..n)
        .map(|i| {
            if i == 0 || i + 1 == n {
                return C::default();
            }
            let (a, b, c) = (pis.get(i - 1), pis.get(i), pis.get(i + 1));
            let (Some(a), Some(b), Some(c)) = (a, b, c) else { return C::default() };
            let d = ang(a.p, b.p, c.p);
            let r = if b.r > 0.0 { b.r } else { 0.0 };
            let tan_half = (d.abs() / 2.0).jtan();
            let back = (d.abs() - PI).abs() < 1e-6;
            C { d, r, t: if back || d.abs() < 1e-9 { 0.0 } else { r * tan_half }, tan_half, back }
        })
        .collect()
}

/// `maxRadius(pis, i)`.
pub fn max_radius(pis: &[WPi], i: usize) -> f64 {
    let cs = curves_of(pis);
    let Some(c) = cs.get(i) else { return 0.0 };
    if i == 0 || i + 1 == pis.len() || c.back {
        return 0.0;
    }
    if c.d.abs() < 1e-9 {
        return f64::INFINITY;
    }
    let p = |k: usize| pis.get(k).map_or(P2 { x: f64::NAN, y: f64::NAN }, |w| w.p);
    let t = |k: usize| cs.get(k).map_or(f64::NAN, |c| c.t);
    let room = min(dist(p(i - 1), p(i)) - t(i - 1), dist(p(i), p(i + 1)) - t(i + 1));
    if room <= 0.0 { 0.0 } else { room / c.tan_half }
}

/// `curveWhy(pis, { minR })` → issues (JSON, as the JS objects).
pub fn curve_why(pis_in: &[WPi], min_r: f64) -> Vec<Value> {
    let p: Vec<WPi> = pis_in.iter().filter(|w| w.p.x.is_finite() && w.p.y.is_finite()).copied().collect();
    if p.len() < 3 {
        return Vec::new();
    }
    let cs = curves_of(&p);
    let n = p.len();
    let mut out = Vec::new();
    for (i, c) in cs.iter().enumerate() {
        if c.back && c.r > 0.0 {
            out.push(crate::jo! { "kind" => "back", "pi" => i });
        }
    }
    let pp = |k: usize| p.get(k).map_or(P2 { x: f64::NAN, y: f64::NAN }, |w| w.p);
    let cc = |k: usize| cs.get(k).copied().unwrap_or_default();
    for a in 0..n - 1 {
        let b = a + 1;
        let l = dist(pp(a), pp(b));
        let (ta, tb) = (cc(a).t, cc(b).t);
        if ta + tb <= l + 1e-9 {
            continue;
        }
        let inner = |i: usize| i > 0 && i + 1 < n && !cc(i).back && cc(i).d.abs() > 1e-9;
        let fix = |i: usize, other: f64| -> Option<f64> {
            if !inner(i) {
                return None;
            }
            let here = l - other;
            let far = if i == a { dist(pp(a - 1), pp(a)) - cc(a - 1).t } else { dist(pp(b), pp(b + 1)) - cc(b + 1).t };
            let room = min(here, far);
            if room > 0.0 { Some(down(room / cc(i).tan_half)) } else { None }
        };
        let f = l / (ta + tb);
        out.push(crate::jo! {
            "kind" => "overlap", "a" => a, "b" => b, "dist" => l, "need" => ta + tb, "Ta" => ta, "Tb" => tb,
            "fixA" => fix(a, tb), "fixB" => fix(b, ta),
            "both" => crate::jo! { "ra" => if inner(a) { down(cc(a).r * f) } else { 0.0 }, "rb" => if inner(b) { down(cc(b).r * f) } else { 0.0 } },
        });
    }
    if min_r > 0.0 {
        for (i, c) in cs.iter().enumerate() {
            if i == 0 || i + 1 == n || !(c.r > 0.0) || c.r >= min_r - 1e-9 || c.back {
                continue;
            }
            let max_r = max_radius(&p, i);
            out.push(crate::jo! {
                "kind" => "min", "pi" => i, "r" => c.r, "minR" => min_r, "maxR" => max_r,
                "fix" => if min_r <= max_r + 1e-9 { Some(up(min_r)) } else { None },
            });
        }
    }
    out
}

/// `verticalWhy(pvis)` → issues (JSON).
pub fn vertical_why(pvis_in: &[Pvi]) -> Vec<Value> {
    let mut pvis: Vec<(Pvi, usize)> =
        pvis_in.iter().filter(|p| p.sta.is_finite() && p.z.is_finite()).copied().enumerate().map(|(i, p)| (p, i)).collect();
    crate::js::sort_by(&mut pvis, |a, b| a.0.sta - b.0.sta);
    let mut out = Vec::new();
    let mut prev_end = pvis.first().map_or(0.0, |p| p.0.sta);
    let g = |k: usize| pvis.get(k).map(|p| p.0);
    for i in 1..pvis.len().saturating_sub(1) {
        let (Some(v), Some(a), Some(b)) = (g(i), g(i - 1), g(i + 1)) else { continue };
        let i0 = pvis.get(i).map_or(0, |p| p.1);
        let g1 = (v.z - a.z) / (v.sta - a.sta);
        let g2 = (b.z - v.z) / (b.sta - v.sta);
        let aa = (g2 - g1) * 100.0;
        let lv = v.l.unwrap_or(f64::NAN);
        let mut l = if lv != 0.0 && !lv.is_nan() { lv } else { 0.0 };
        if l == 0.0 && v.k.is_some_and(|k| k > 0.0) {
            l = v.k.unwrap_or(0.0) * aa.abs();
        }
        if !(l > 0.0) || aa.abs() < 1e-9 {
            continue;
        }
        let back = v.sta - max(a.sta, prev_end);
        let fwd = b.sta - v.sta;
        let room = 2.0 * min(back, fwd);
        if l > room + 1e-9 {
            let fix = if room > 0.0 { Some(down(room)) } else { None };
            let k = fix.filter(|f| *f != 0.0).map(|f| f / aa.abs());
            out.push(crate::jo! {
                "kind" => "vc", "pvi" => i, "index" => i0, "L" => l, "room" => room, "fix" => fix, "K" => k,
                "side" => if back < fwd { "before" } else { "past" },
            });
            prev_end = v.sta + fix.unwrap_or(0.0) / 2.0;
        } else {
            prev_end = v.sta + l / 2.0;
        }
    }
    out
}
