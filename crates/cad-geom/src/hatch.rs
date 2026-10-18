//! hatch.js (patterns and closed boundaries): `{ type: 'hatch', loops (even-odd), solid, pattern, pscale, pangle,
//! piso, patlines?, ox?, oy? }`. [`boundary_at`] is the HATCH command's pick-inside search (M4.10).

use crate::jsmath::JsMath;
use cc_cad::entity::{Entity, Geo};
use cc_cad::{Doc, js};
use cc_geom::P2;
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

// ---------------------------------------------------------------------------------------------------------------
// boundaryAt: the HATCH command's pick-inside search (M4.10)

const MAX_BOUNDARY_SEGS: usize = 80000;

/// Why no boundary was found round a point (the HATCH command prints `cmd.hatch.err.<code>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundaryError {
    None,
    Open,
    Large,
}
impl BoundaryError {
    pub fn code(self) -> &'static str {
        match self {
            BoundaryError::None => "none",
            BoundaryError::Open => "open",
            BoundaryError::Large => "large",
        }
    }
}

struct Half {
    from: usize,
    to: usize,
    ang: f64,
    twin: usize,
    used: bool,
}

/// Floor of a cell coordinate as a grid index (the JS `Math.floor` feeds a 32-bit hash: wraps the same way).
fn cell_index(v: f64) -> i32 {
    let f = v.floor();
    if !f.is_finite() {
        return 0;
    }
    // |f| is far below 2^31 in practice (≈ √segments); clamp keeps the cast defined for hostile input
    #[allow(clippy::cast_possible_truncation)]
    let i = f.clamp(-2_147_483_648.0, 2_147_483_647.0) as i32;
    i
}

/// `Math.round(v)` as a grid key (NaN and ±Infinity keep their own keys).
fn round_key(v: f64) -> i64 {
    let r = js::round(v);
    if r.is_nan() {
        return i64::MIN;
    }
    #[allow(clippy::cast_possible_truncation)]
    let i = r.clamp(-9.0e18, 9.0e18) as i64;
    i
}

fn in_ring(q: P2, ring: &[P2]) -> bool {
    let mut inside = false;
    let n = ring.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (Some(a), Some(b)) = (ring.get(i), ring.get(j)) else { break };
        if (a.y > q.y) != (b.y > q.y) && q.x < (b.x - a.x) * (q.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn flat_of(ring: &[P2]) -> Vec<f64> {
    ring.iter().flat_map(|p| [p.x, p.y]).collect()
}

#[allow(clippy::too_many_arguments)]
fn on_seg(px: f64, py: f64, x1: f64, y1: f64, vx: f64, vy: f64, l: f64, tol: f64) -> Option<f64> {
    let u = ((px - x1) * vx + (py - y1) * vy) / (l * l);
    let d = ((px - x1) * vy - (py - y1) * vx).abs() / l;
    (d <= tol && u > -tol / l && u < 1.0 + tol / l).then(|| js::max(0.0, js::min(1.0, u)))
}

fn cross_split(s: &[f64; 4], t: &[f64; 4], ss: &mut Vec<f64>, ts: &mut Vec<f64>, tol: f64) {
    let [ax, ay, bx, by] = *s;
    let [cx, cy, dx, dy] = *t;
    let (rx, ry, qx, qy) = (bx - ax, by - ay, dx - cx, dy - cy);
    let den = rx * qy - ry * qx;
    let lr = js::hypot(rx, ry);
    let lq = js::hypot(qx, qy);
    if den.abs() <= 1e-12 * lr * lq {
        for (px, py) in [(cx, cy), (dx, dy)] {
            if let Some(u) = on_seg(px, py, ax, ay, rx, ry, lr, tol) {
                ss.push(u);
            }
        }
        for (px, py) in [(ax, ay), (bx, by)] {
            if let Some(u) = on_seg(px, py, cx, cy, qx, qy, lq, tol) {
                ts.push(u);
            }
        }
        return;
    }
    let u = ((cx - ax) * qy - (cy - ay) * qx) / den;
    let v = ((cx - ax) * ry - (cy - ay) * rx) / den;
    let eu = tol / lr;
    let ev = tol / lq;
    if u < -eu || u > 1.0 + eu || v < -ev || v > 1.0 + ev {
        return;
    }
    ss.push(js::max(0.0, js::min(1.0, u)));
    ts.push(js::max(0.0, js::min(1.0, v)));
}

/// hatch.js `boundaryAt`: the smallest closed area round `pt` made by the objects, and the islands inside it (other
/// pieces of the graph, so even-odd leaves them empty). Arcs and curves become short straight pieces. `within`
/// limits the search to what is on screen.
pub fn boundary_at(entities: &[&Entity], pt: P2, blocks: Option<&Blocks>, within: Option<&Bounds>) -> Result<Vec<Vec<f64>>, BoundaryError> {
    use std::collections::{HashMap, HashSet};
    // 1. every boundary-making object as straight pieces
    let mut segs: Vec<[f64; 4]> = Vec::new();
    let add = |segs: &mut Vec<[f64; 4]>, x1: f64, y1: f64, x2: f64, y2: f64| {
        if (x1 - x2).abs() > EPS || (y1 - y2).abs() > EPS {
            segs.push([x1, y1, x2, y2]);
        }
    };
    for e in entities {
        if matches!(e.typ(), "text" | "point" | "hatch" | "dim" | "mleader" | "viewport") {
            continue;
        }
        if let Some(w) = within {
            let b = crate::pick::entity_bounds(e, blocks);
            if !crate::pick::valid_bounds(&b) || b.max_x < w.min_x || b.min_x > w.max_x || b.max_y < w.min_y || b.min_y > w.max_y {
                continue;
            }
        }
        for q in prims(e, blocks) {
            if q.boxed {
                continue;
            }
            if q.is_l() {
                add(&mut segs, q.x1, q.y1, q.x2, q.y2);
            } else {
                let fl = arc_points(&q, false);
                let mut i = 0usize;
                while let (Some(a), Some(b), Some(c), Some(d)) = (fl.get(i), fl.get(i + 1), fl.get(i + 2), fl.get(i + 3)) {
                    add(&mut segs, *a, *b, *c, *d);
                    i += 2;
                }
            }
        }
        if segs.len() > MAX_BOUNDARY_SEGS {
            return Err(BoundaryError::Large);
        }
    }
    if segs.is_empty() {
        return Err(BoundaryError::None);
    }
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for s in &segs {
        min_x = js::min(min_x, js::min(s[0], s[2]));
        max_x = js::max(max_x, js::max(s[0], s[2]));
        min_y = js::min(min_y, js::min(s[1], s[3]));
        max_y = js::max(max_y, js::max(s[1], s[3]));
    }
    let tol = js::max(js::max(max_x - min_x, max_y - min_y), 1.0) * 1e-9;

    // 2. split every piece where others cross or touch it (a grid keeps this near linear)
    let n = segs.len();
    let mut splits: Vec<Vec<f64>> = vec![vec![0.0, 1.0]; n];
    let cs = js::max(js::max(max_x - min_x, max_y - min_y), 1e-9) / js::max(1.0, f(n).sqrt());
    let mut cell_of: HashMap<i32, usize> = HashMap::new();
    let mut cells: Vec<Vec<usize>> = Vec::new();
    for (idx, s) in segs.iter().enumerate() {
        let i0 = cell_index((js::min(s[0], s[2]) - min_x) / cs);
        let i1 = cell_index((js::max(s[0], s[2]) - min_x) / cs);
        let j0 = cell_index((js::min(s[1], s[3]) - min_y) / cs);
        let j1 = cell_index((js::max(s[1], s[3]) - min_y) / cs);
        let mut i = i0;
        while i <= i1 {
            let mut j = j0;
            while j <= j1 {
                let k = i.wrapping_mul(73_856_093) ^ j.wrapping_mul(19_349_663);
                let slot = *cell_of.entry(k).or_insert_with(|| {
                    cells.push(Vec::new());
                    cells.len() - 1
                });
                if let Some(c) = cells.get_mut(slot) {
                    c.push(idx);
                }
                j = j.saturating_add(1);
                if j == i32::MAX {
                    break;
                }
            }
            i = i.saturating_add(1);
            if i == i32::MAX {
                break;
            }
        }
    }
    let mut done: HashSet<(usize, usize)> = HashSet::new();
    for list in &cells {
        for (a, &i) in list.iter().enumerate() {
            for &j in list.iter().skip(a + 1) {
                let pk = if i < j { (i, j) } else { (j, i) };
                if i == j || !done.insert(pk) {
                    continue;
                }
                let (Some(si), Some(sj)) = (segs.get(i), segs.get(j)) else { continue };
                let mut ti = splits.get_mut(i).map(std::mem::take).unwrap_or_default();
                let mut tj = splits.get_mut(j).map(std::mem::take).unwrap_or_default();
                cross_split(si, sj, &mut ti, &mut tj, tol);
                if let Some(x) = splits.get_mut(i) {
                    *x = ti;
                }
                if let Some(x) = splits.get_mut(j) {
                    *x = tj;
                }
            }
        }
    }

    // 3. a planar graph: vertices merged within the tolerance, edges once each
    let snap = tol * 1000.0;
    let mut vid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    let mut verts: Vec<P2> = Vec::new();
    let mut vertex = |x: f64, y: f64, verts: &mut Vec<P2>| -> usize {
        let gx = round_key(x / snap);
        let gy = round_key(y / snap);
        for di in -1i64..=1 {
            for dj in -1i64..=1 {
                if let Some(list) = vid.get(&(gx.wrapping_add(di), gy.wrapping_add(dj))) {
                    for &v in list {
                        if let Some(p) = verts.get(v)
                            && (p.x - x).abs() <= snap
                            && (p.y - y).abs() <= snap
                        {
                            return v;
                        }
                    }
                }
            }
        }
        let v = verts.len();
        verts.push(P2::new(x, y));
        vid.entry((gx, gy)).or_default().push(v);
        v
    };
    let mut edge_set: HashSet<(usize, usize)> = HashSet::new();
    let mut edges: Vec<(usize, usize)> = Vec::new();
    for (s, sp) in segs.iter().zip(&splits) {
        let mut ts = sp.clone();
        // `[...new Set(ts)].sort((p, q) => p - q)`: values are in 0…1
        ts.sort_by(|p, q| p.partial_cmp(q).unwrap_or(std::cmp::Ordering::Equal));
        ts.dedup_by(|a, b| a == b);
        let Some(&t0) = ts.first() else { continue };
        let mut prev = vertex(s[0] + (s[2] - s[0]) * t0, s[1] + (s[3] - s[1]) * t0, &mut verts);
        for &t in ts.iter().skip(1) {
            let v = vertex(s[0] + (s[2] - s[0]) * t, s[1] + (s[3] - s[1]) * t, &mut verts);
            if v != prev {
                let ek = if prev < v { (prev, v) } else { (v, prev) };
                if edge_set.insert(ek) {
                    edges.push((prev, v));
                }
            }
            prev = v;
        }
    }
    let nv = verts.len();
    let vat = |v: usize| verts.get(v).copied().unwrap_or(P2::new(f64::NAN, f64::NAN));
    // drop dangling pieces: they bound nothing
    let mut deg: Vec<i64> = vec![0; nv];
    let bump = |deg: &mut Vec<i64>, v: usize, d: i64| {
        if let Some(x) = deg.get_mut(v) {
            *x += d;
        }
    };
    let deg_of = |deg: &Vec<i64>, v: usize| deg.get(v).copied().unwrap_or(0);
    for &(a, b) in &edges {
        bump(&mut deg, a, 1);
        bump(&mut deg, b, 1);
    }
    let mut alive = vec![true; edges.len()];
    let mut changed = true;
    while changed {
        changed = false;
        for (&(a, b), al) in edges.iter().zip(alive.iter_mut()) {
            if *al && (deg_of(&deg, a) < 2 || deg_of(&deg, b) < 2) {
                *al = false;
                bump(&mut deg, a, -1);
                bump(&mut deg, b, -1);
                changed = true;
            }
        }
    }
    let mut half: Vec<Half> = Vec::new();
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); nv];
    for (&(a, b), al) in edges.iter().zip(&alive) {
        if !*al {
            continue;
        }
        let (pa, pb) = (vat(a), vat(b));
        let h1 = half.len();
        let h2 = h1 + 1;
        half.push(Half { from: a, to: b, ang: (pb.y - pa.y).jatan2(pb.x - pa.x), twin: h2, used: false });
        half.push(Half { from: b, to: a, ang: (pa.y - pb.y).jatan2(pa.x - pb.x), twin: h1, used: false });
        if let Some(o) = out.get_mut(a) {
            o.push(h1);
        }
        if let Some(o) = out.get_mut(b) {
            o.push(h2);
        }
    }
    if half.is_empty() {
        return Err(BoundaryError::Open);
    }
    let ang_of = |h: usize| half.get(h).map_or(f64::NAN, |x| x.ang);
    for o in &mut out {
        o.sort_by(|p, q| {
            let d = ang_of(*p) - ang_of(*q);
            if d < 0.0 {
                std::cmp::Ordering::Less
            } else if d > 0.0 {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
    }

    // 4. every face: from a half-edge u→v, turn to the edge leaving v just clockwise of v→u (face on the left)
    let next = |h: usize, half: &[Half]| -> usize {
        let Some(hh) = half.get(h) else { return h };
        let Some(o) = out.get(hh.to) else { return h };
        let len = o.len();
        if len == 0 {
            return h;
        }
        // indexOf → -1 when absent: (−1 − 1 + len) % len
        let k = match o.iter().position(|x| *x == hh.twin) {
            Some(i) => (i + len - 1) % len,
            None => (len + len - 2) % len,
        };
        o.get(k).copied().unwrap_or(h)
    };
    struct Face {
        ring: Vec<usize>,
        area: f64,
        comp: i64,
    }
    let mut faces: Vec<Face> = Vec::new();
    for h0 in 0..half.len() {
        if half.get(h0).is_none_or(|x| x.used) {
            continue;
        }
        let mut ring: Vec<usize> = Vec::new();
        let mut h = h0;
        let mut guard = 0usize;
        while let Some(hh) = half.get_mut(h) {
            hh.used = true;
            ring.push(hh.from);
            h = next(h, &half);
            guard += 1;
            if h == h0 || guard >= half.len() + 2 {
                break;
            }
        }
        let mut area = 0.0;
        for (i, v) in ring.iter().enumerate() {
            let p = vat(*v);
            let q = vat(ring.get((i + 1) % ring.len()).copied().unwrap_or(*v));
            area += p.x * q.y - q.x * p.y;
        }
        faces.push(Face { ring, area: area / 2.0, comp: -1 });
    }
    // components (connected pieces of the graph), for islands
    let mut comp: Vec<i64> = vec![-1; nv];
    let comp_of = |comp: &Vec<i64>, v: usize| comp.get(v).copied().unwrap_or(0);
    let mut n_comp = 0i64;
    for i in 0..nv {
        if comp_of(&comp, i) >= 0 || out.get(i).is_none_or(Vec::is_empty) {
            continue;
        }
        let mut stack = vec![i];
        if let Some(x) = comp.get_mut(i) {
            *x = n_comp;
        }
        while let Some(u) = stack.pop() {
            for &h in out.get(u).map_or(&[][..], Vec::as_slice) {
                let Some(to) = half.get(h).map(|x| x.to) else { continue };
                if comp_of(&comp, to) < 0 {
                    if let Some(x) = comp.get_mut(to) {
                        *x = n_comp;
                    }
                    stack.push(to);
                }
            }
        }
        n_comp += 1;
    }
    for fc in &mut faces {
        fc.comp = fc.ring.first().map_or(-1, |v| comp_of(&comp, *v));
    }
    let pts_of = |ring: &[usize]| -> Vec<P2> { ring.iter().map(|v| vat(*v)).collect() };

    // 5. the smallest bounded face round the point
    let mut outer: Option<&Face> = None;
    for fc in &faces {
        if fc.area > 0.0 && outer.is_none_or(|o| fc.area < o.area) && in_ring(pt, &pts_of(&fc.ring)) {
            outer = Some(fc);
        }
    }
    let Some(o) = outer else { return Err(BoundaryError::Open) };
    let oring = pts_of(&o.ring);
    let mut loops = vec![flat_of(&oring)];
    for fc in &faces {
        if fc.area >= 0.0 || fc.comp == o.comp {
            continue;
        }
        let ring = pts_of(&fc.ring);
        if let Some(first) = ring.first()
            && in_ring(*first, &oring)
            && !in_ring(pt, &ring)
        {
            loops.push(flat_of(&ring));
        }
    }
    Ok(loops)
}
