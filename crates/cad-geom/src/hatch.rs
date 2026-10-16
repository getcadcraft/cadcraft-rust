//! hatch.js (patterns and closed boundaries): `{ type: 'hatch', loops (even-odd), solid, pattern, pscale, pangle,
//! piso, patlines?, ox?, oy? }`. `boundaryAt` (the HATCH command's pick-inside search) is not ported in M4.

use crate::jsmath::JsMath;
use cc_cad::entity::{Entity, Geo};
use cc_cad::{Doc, js};
use serde_json::Value;

use crate::geom::*;

struct Fam {
    angle: f64,
    x: f64,
    y: f64,
    dx: f64,
    dy: f64,
    dashes: Vec<f64>,
}

/// The standard metric patterns (acadiso sizes): (name, [(angle°, x, y, dx, dy, dashes)]).
#[rustfmt::skip]
#[allow(clippy::type_complexity)]
const PATTERNS: [(&str, &[(f64, f64, f64, f64, f64, &[f64])]); 16] = [
    ("ANSI31", &[(45.0, 0.0, 0.0, 0.0, 3.175, &[])]),
    ("ANSI32", &[(45.0, 0.0, 0.0, 0.0, 9.525, &[]), (45.0, 4.490128, 0.0, 0.0, 9.525, &[])]),
    ("ANSI33", &[(45.0, 0.0, 0.0, 0.0, 6.35, &[]), (45.0, 4.490128, 0.0, 0.0, 6.35, &[3.175, -1.5875])]),
    ("ANSI34", &[(45.0, 0.0, 0.0, 0.0, 19.05, &[]), (45.0, 4.490128, 0.0, 0.0, 19.05, &[]), (45.0, 8.980256, 0.0, 0.0, 19.05, &[]), (45.0, 13.470384, 0.0, 0.0, 19.05, &[])]),
    ("ANSI35", &[(45.0, 0.0, 0.0, 0.0, 6.35, &[]), (45.0, 4.490128, 0.0, 0.0, 6.35, &[7.9375, -1.5875, 0.0, -1.5875])]),
    ("ANSI36", &[(45.0, 0.0, 0.0, 5.55625, 3.175, &[7.9375, -1.5875, 0.0, -1.5875])]),
    ("ANSI37", &[(45.0, 0.0, 0.0, 0.0, 3.175, &[]), (135.0, 0.0, 0.0, 0.0, 3.175, &[])]),
    ("ANSI38", &[(45.0, 0.0, 0.0, 0.0, 3.175, &[]), (135.0, 0.0, 0.0, 6.35, 3.175, &[7.9375, -4.7625])]),
    ("LINE", &[(0.0, 0.0, 0.0, 0.0, 3.175, &[])]),
    ("NET", &[(0.0, 0.0, 0.0, 0.0, 3.175, &[]), (90.0, 0.0, 0.0, 0.0, 3.175, &[])]),
    ("NET3", &[(0.0, 0.0, 0.0, 0.0, 3.175, &[]), (60.0, 0.0, 0.0, 0.0, 3.175, &[]), (120.0, 0.0, 0.0, 0.0, 3.175, &[])]),
    ("DASH", &[(0.0, 0.0, 0.0, 3.175, 3.175, &[3.175, -3.175])]),
    ("SQUARE", &[(0.0, 0.0, 0.0, 0.0, 3.175, &[3.175, -3.175]), (90.0, 0.0, 0.0, 0.0, 3.175, &[3.175, -3.175])]),
    ("BRICK", &[(0.0, 0.0, 0.0, 0.0, 6.35, &[]), (90.0, 0.0, 0.0, 6.35, 6.35, &[6.35, -6.35]), (90.0, 3.175, 0.0, 6.35, 6.35, &[-6.35, 6.35])]),
    ("DOTS", &[(0.0, 0.0, 0.0, 0.79375, 1.5875, &[0.0, -1.5875])]),
    ("EARTH", &[
        (0.0, 0.0, 0.0, 6.35, 6.35, &[6.35, -6.35]), (0.0, 0.0, 2.38125, 6.35, 6.35, &[6.35, -6.35]), (0.0, 0.0, 4.7625, 6.35, 6.35, &[6.35, -6.35]),
        (90.0, 0.79375, 5.55625, 6.35, 6.35, &[6.35, -6.35]), (90.0, 3.175, 5.55625, 6.35, 6.35, &[6.35, -6.35]), (90.0, 5.55625, 5.55625, 6.35, 6.35, &[6.35, -6.35])]),
];
pub fn pattern_names() -> Vec<&'static str> {
    PATTERNS.iter().map(|(n, _)| *n).collect()
}

const MAX_LINES: f64 = 6000.0;
const MAX_SEGS: usize = 60000;
/// Dash periods walked along one pattern line at most (guards patterns of gaps only; the JS has no cap).
const MAX_PERIODS: usize = 10_000_000;

/// Metric (acadiso) or inch (acad) definitions: the hatch's `piso`, else from the drawing's units.
pub fn hatch_is_metric_doc(e: Option<&Entity>, doc: &Doc) -> bool {
    if let Some(Value::Bool(b)) = e.and_then(|e| e.get("piso")) {
        return *b;
    }
    let u = doc.meta.get("units").and_then(Value::as_str);
    let lu = doc.meta.get("lunits").and_then(Value::as_f64);
    !(u == Some("inches") || u == Some("feet") || lu == Some(3.0) || lu == Some(4.0))
}

/// The pattern's lines as L primitives, clipped even-odd to the loops; None = solid, unknown pattern, or too dense.
pub fn hatch_lines(e: &Entity, doc: &Doc) -> Option<Vec<Prim>> {
    let Geo::Hatch(h) = &e.geo else { return None };
    if h.solid == Some(true) {
        return None;
    }
    let mut fams: Option<Vec<Fam>> = None;
    if let Some(Value::Array(pl)) = e.get("patlines")
        && !pl.is_empty()
    {
        let n = |f: &Value, k: &str| js::to_number_opt(f.get(k));
        fams = Some(
            pl.iter()
                .map(|f| Fam {
                    angle: n(f, "angle"),
                    x: n(f, "x"),
                    y: n(f, "y"),
                    dx: n(f, "dx"),
                    dy: n(f, "dy"),
                    dashes: f.get("dashes").and_then(Value::as_array).map(|a| a.iter().map(js::to_number).collect()).unwrap_or_default(),
                })
                .collect(),
        );
    } else {
        let name = match e.get("pattern") {
            Some(v) if js::truthy(Some(v)) => js::to_js_string(v).to_uppercase(),
            _ => String::new(),
        };
        if let Some((_, lines)) = PATTERNS.iter().find(|(n, _)| *n == name) {
            let pscale = e.num("pscale").filter(|v| *v > 0.0).unwrap_or(1.0);
            let k = pscale / if hatch_is_metric_doc(Some(e), doc) { 1.0 } else { 25.4 };
            let turn = js::or(e.num("pangle"), 0.0);
            let (c, s) = (turn.jcos(), turn.jsin());
            fams = Some(
                lines
                    .iter()
                    .map(|(angle, x, y, dx, dy, dashes)| Fam {
                        angle: angle + turn * 180.0 / PI,
                        x: (x * c - y * s) * k,
                        y: (x * s + y * c) * k,
                        dx: dx * k,
                        dy: dy * k,
                        dashes: dashes.iter().map(|d| d * k).collect(),
                    })
                    .collect(),
            );
        }
    }
    let ox = js::finite(e.get("ox")).unwrap_or(0.0);
    let oy = js::finite(e.get("oy")).unwrap_or(0.0);
    let mut fams = fams?;
    if ox != 0.0 || oy != 0.0 {
        for f in &mut fams {
            f.x += ox;
            f.y += oy;
        }
    }
    clip_families(&fams, &h.loops)
}

fn loop_edges(loops: &[Vec<f64>]) -> Vec<[f64; 4]> {
    let mut edges = Vec::new();
    for l in loops {
        let n = l.len() / 2;
        if n < 3 {
            continue;
        }
        for i in 0..n {
            let j = (i + 1) % n;
            let g = |k: usize| l.get(k).copied().unwrap_or(f64::NAN);
            let (x1, y1, x2, y2) = (g(2 * i), g(2 * i + 1), g(2 * j), g(2 * j + 1));
            if x1 != x2 || y1 != y2 {
                edges.push([x1, y1, x2, y2]);
            }
        }
    }
    edges
}

fn seg(ox: f64, oy: f64, ux: f64, uy: f64, t0: f64, t1: f64) -> Prim {
    Prim::l(ox + ux * t0, oy + uy * t0, ox + ux * t1, oy + uy * t1)
}

fn clip_families(fams: &[Fam], loops: &[Vec<f64>]) -> Option<Vec<Prim>> {
    let edges = loop_edges(loops);
    if edges.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    let mut line_count = 0.0;
    for fm in fams {
        let a = fm.angle * PI / 180.0;
        let (ux, uy) = (a.jcos(), a.jsin());
        let (nx, ny) = (-uy, ux);
        let dy = fm.dy;
        if !(dy.abs() > 1e-12) {
            continue;
        }
        let mut n_min = f64::INFINITY;
        let mut n_max = f64::NEG_INFINITY;
        for [x1, y1, x2, y2] in &edges {
            for (x, y) in [(x1, y1), (x2, y2)] {
                let v = (x - fm.x) * nx + (y - fm.y) * ny;
                if v < n_min {
                    n_min = v;
                }
                if v > n_max {
                    n_max = v;
                }
            }
        }
        let k0 = (js::min(n_min / dy, n_max / dy) - 1e-9).ceil();
        let k1 = (js::max(n_min / dy, n_max / dy) + 1e-9).floor();
        line_count += js::max(0.0, k1 - k0 + 1.0);
        if line_count > MAX_LINES {
            return None;
        }
        let period = fm.dashes.iter().fold(0.0, |s, d| s + d.abs());
        let mut k = k0;
        while k <= k1 {
            let ox = fm.x + k * (fm.dx * ux + dy * nx);
            let oy = fm.y + k * (fm.dx * uy + dy * ny);
            let mut ts = Vec::new();
            for [x1, y1, x2, y2] in &edges {
                let d1 = (x1 - ox) * nx + (y1 - oy) * ny;
                let d2 = (x2 - ox) * nx + (y2 - oy) * ny;
                if (d1 > 0.0) == (d2 > 0.0) {
                    continue;
                }
                let tt = d1 / (d1 - d2);
                let px = x1 + (x2 - x1) * tt;
                let py = y1 + (y2 - y1) * tt;
                ts.push((px - ox) * ux + (py - oy) * uy);
            }
            k += 1.0;
            if ts.len() < 2 {
                continue;
            }
            ts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            for pair in ts.chunks_exact(2) {
                let [t0, t1] = [pair.first().copied().unwrap_or(0.0), pair.get(1).copied().unwrap_or(0.0)];
                if t1 - t0 < 1e-12 {
                    continue;
                }
                if fm.dashes.is_empty() || !(period > 1e-12) {
                    out.push(seg(ox, oy, ux, uy, t0, t1));
                    continue;
                }
                let mut start = (t0 / period).floor() * period;
                let mut guard = 0usize;
                while start < t1 {
                    guard += 1;
                    if guard > MAX_PERIODS {
                        return None;
                    }
                    let mut pp = start;
                    for &d in &fm.dashes {
                        let len = d.abs();
                        if d >= 0.0 {
                            let a0 = js::max(pp, t0);
                            let a1 = js::min(pp + if d == 0.0 { 0.0 } else { len }, t1);
                            if if d == 0.0 { pp >= t0 && pp <= t1 } else { a1 > a0 } {
                                out.push(if d == 0.0 {
                                    seg(ox, oy, ux, uy, pp - period * 0.02, pp + period * 0.02)
                                } else {
                                    seg(ox, oy, ux, uy, a0, a1)
                                });
                            }
                        }
                        pp += len;
                    }
                    if out.len() > MAX_SEGS {
                        return None;
                    }
                    start += period;
                }
            }
            if out.len() > MAX_SEGS {
                return None;
            }
        }
    }
    Some(out)
}

/// Points along an arc primitive (CCW a0 → a1, or the other way when rev), as a flat list ending at its end.
fn arc_points(q: &Prim, rev: bool) -> Vec<f64> {
    let sweep = q.a1 - q.a0;
    let n = steps(4.0, sweep.abs() / (TAU / 128.0));
    let mut flat = Vec::with_capacity(2 * n + 2);
    for i in 0..=n {
        let a = if rev { q.a1 - sweep * f(i) / f(n) } else { q.a0 + sweep * f(i) / f(n) };
        flat.push(q.cx + q.r * a.jcos());
        flat.push(q.cy + q.r * a.jsin());
    }
    flat
}

/// A closed object as a flat loop (arcs and ellipses as short straight pieces), else None.
pub fn loops_of(e: &Entity) -> Option<Vec<Vec<f64>>> {
    match &e.geo {
        Geo::Circle(c) => Some(vec![arc_points(&Prim::a(c.cx, c.cy, c.r, 0.0, TAU), false)]),
        Geo::Poly(pl) if pl.is_closed() && pl.pts.len() >= 2 => {
            let mut flat = Vec::new();
            for q in poly_prims(&pl.pts, true) {
                if q.is_l() {
                    flat.push(q.x1);
                    flat.push(q.y1);
                } else {
                    let ap = arc_points(&q, q.rev());
                    flat.extend(ap.iter().take(ap.len().saturating_sub(2)));
                }
            }
            (flat.len() >= 6).then(|| vec![flat])
        }
        Geo::Ellipse(el) if el.t1 - el.t0 >= TAU - 1e-9 => {
            let pts = ellipse_points(e, 144.0);
            let mut flat = Vec::new();
            for q in pts.iter().take(pts.len().saturating_sub(1)) {
                flat.push(q.x);
                flat.push(q.y);
            }
            Some(vec![flat])
        }
        _ => None,
    }
}
