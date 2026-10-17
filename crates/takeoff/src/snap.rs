//! `snap.js` — snapping to the PDF's own linework (T1). The page's vector paths come as a pdf.js-style operator list
//! (`fnArray` + `argsArray`, pdf.js's own OPS numbers and path-data codes 0 move, 1 line, 2 cubic, 3 quad, 4 close);
//! cc-io-takeoff builds the same list from a PDF page. Curves are flattened (8 chords); clipping paths are skipped;
//! form XObjects are followed with their matrix.
// Integer arithmetic here is segment indices below the segment cap (MAX_SEGS) and grid cells from finite, clamped coordinates: it cannot overflow, so the crate-wide
// `arithmetic_side_effects` deny is relaxed for this module only.
#![allow(clippy::arithmetic_side_effects)]

use std::collections::HashMap;

use cc_m3d::js::hypot2;

const CURVE_STEPS: usize = 8;
pub const MAX_SEGS: usize = 400_000;

/// pdf.js's operator numbers this module reads.
#[derive(Debug, Clone, Copy)]
pub struct Ops {
    pub save: i64,
    pub restore: i64,
    pub transform: i64,
    pub construct_path: i64,
    pub end_path: i64,
    pub paint_form_begin: i64,
    pub paint_form_end: i64,
}
/// The values of pdf.js 6 (`pdfjs.OPS`).
pub const PDFJS_OPS: Ops =
    Ops { save: 10, restore: 11, transform: 12, construct_path: 91, end_path: 28, paint_form_begin: 74, paint_form_end: 75 };

/// One operator: its number and its arguments (as pdf.js gives them, numbers or arrays).
#[derive(Debug, Clone)]
pub enum Arg {
    Num(f64),
    List(Vec<f64>),
    /// constructPath: [op, [path data], minMax]
    Path(i64, Vec<f64>),
    Other,
}
#[derive(Debug, Clone)]
pub struct Op {
    pub f: i64,
    pub args: Vec<Arg>,
}

/// The linework: segments `[x1, y1, x2, y2, …]`, the curve flags, the count and whether it was cut at the limit.
#[derive(Debug, Clone, Default)]
pub struct Linework {
    pub segs: Vec<f64>,
    /// 0 straight; else 1 (a chord of a flattened curve) | 2 (it starts the curve) | 4 (it ends it)
    pub curve: Vec<u8>,
    pub n: usize,
    pub truncated: bool,
}

type M6 = [f64; 6];
fn mul(m: &M6, k: &M6) -> M6 {
    [
        m[0] * k[0] + m[2] * k[1],
        m[1] * k[0] + m[3] * k[1],
        m[0] * k[2] + m[2] * k[3],
        m[1] * k[2] + m[3] * k[3],
        m[0] * k[4] + m[2] * k[5] + m[4],
        m[1] * k[4] + m[3] * k[5] + m[5],
    ]
}
fn m6(v: &[f64]) -> M6 {
    let g = |i: usize| v.get(i).copied().unwrap_or(f64::NAN);
    [g(0), g(1), g(2), g(3), g(4), g(5)]
}

/// Segments of an operator list.
pub fn extract_segments(ops_def: &Ops, ops: &[Op], max_segs: usize) -> Linework {
    let mut lw = Linework::default();
    let mut ctm: M6 = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut stack: Vec<M6> = Vec::new();
    let push = |lw: &mut Linework, ax: f64, ay: f64, bx: f64, by: f64, flags: u8| {
        if lw.n >= max_segs {
            lw.truncated = true;
            return;
        }
        if !(ax.is_finite() && ay.is_finite() && bx.is_finite() && by.is_finite()) {
            return;
        }
        if ax == bx && ay == by {
            return;
        }
        lw.segs.extend_from_slice(&[ax, ay, bx, by]);
        lw.curve.push(flags);
        lw.n += 1;
    };
    for op in ops {
        if lw.truncated {
            break;
        }
        let f = op.f;
        if f == ops_def.save {
            stack.push(ctm);
        } else if f == ops_def.restore {
            ctm = stack.pop().unwrap_or(ctm);
        } else if f == ops_def.transform {
            let v: Vec<f64> = op.args.iter().map(|a| if let Arg::Num(x) = a { *x } else { f64::NAN }).collect();
            ctm = mul(&ctm, &m6(&v));
        } else if f == ops_def.paint_form_begin {
            stack.push(ctm);
            if let Some(Arg::List(m)) = op.args.first() {
                ctm = mul(&ctm, &m6(m));
            }
        } else if f == ops_def.paint_form_end {
            ctm = stack.pop().unwrap_or(ctm);
        } else if f == ops_def.construct_path {
            let Some(Arg::Path(kind, d)) = op.args.first() else { continue };
            if *kind == ops_def.end_path {
                continue;
            }
            let m = ctm;
            let xf = |x: f64, y: f64| m[0] * x + m[2] * y + m[4];
            let yf = |x: f64, y: f64| m[1] * x + m[3] * y + m[5];
            let (mut px, mut py, mut sx, mut sy) = (f64::NAN, f64::NAN, f64::NAN, f64::NAN);
            let at = |k: usize| d.get(k).copied().unwrap_or(f64::NAN);
            let mut k = 0usize;
            while k < d.len() {
                let c = at(k);
                if c == 0.0 {
                    px = xf(at(k + 1), at(k + 2));
                    py = yf(at(k + 1), at(k + 2));
                    sx = px;
                    sy = py;
                    k += 3;
                } else if c == 1.0 {
                    let qx = xf(at(k + 1), at(k + 2));
                    let qy = yf(at(k + 1), at(k + 2));
                    push(&mut lw, px, py, qx, qy, 0);
                    px = qx;
                    py = qy;
                    k += 3;
                } else if c == 2.0 || c == 3.0 {
                    let pts: Vec<f64> = if c == 2.0 { (1..=6).map(|i| at(k + i)).collect() } else { (1..=4).map(|i| at(k + i)).collect() };
                    let ln = pts.len();
                    let (ex0, ey0) = (pts.get(ln - 2).copied().unwrap_or(f64::NAN), pts.get(ln - 1).copied().unwrap_or(f64::NAN));
                    let ex = xf(ex0, ey0);
                    let ey = yf(ex0, ey0);
                    let (mut lx, mut ly) = (px, py);
                    let g = |i: usize| pts.get(i).copied().unwrap_or(f64::NAN);
                    for st in 1..=CURVE_STEPS {
                        let t = st as f64 / CURVE_STEPS as f64;
                        let u = 1.0 - t;
                        let (mut x, mut y);
                        if c == 2.0 {
                            let (c1x, c1y, c2x, c2y) = (xf(g(0), g(1)), yf(g(0), g(1)), xf(g(2), g(3)), yf(g(2), g(3)));
                            x = u * u * u * px + 3.0 * u * u * t * c1x + 3.0 * u * t * t * c2x + t * t * t * ex;
                            y = u * u * u * py + 3.0 * u * u * t * c1y + 3.0 * u * t * t * c2y + t * t * t * ey;
                        } else {
                            let (cx, cy) = (xf(g(0), g(1)), yf(g(0), g(1)));
                            x = u * u * px + 2.0 * u * t * cx + t * t * ex;
                            y = u * u * py + 2.0 * u * t * cy + t * t * ey;
                        }
                        if st == CURVE_STEPS {
                            x = ex;
                            y = ey;
                        }
                        let flags = 1 | if st == 1 { 2 } else { 0 } | if st == CURVE_STEPS { 4 } else { 0 };
                        push(&mut lw, lx, ly, x, y, flags);
                        lx = x;
                        ly = y;
                    }
                    px = ex;
                    py = ey;
                    k += if c == 2.0 { 7 } else { 5 };
                } else if c == 4.0 {
                    if sx.is_finite() {
                        push(&mut lw, px, py, sx, sy, 0);
                    }
                    px = sx;
                    py = sy;
                    k += 1;
                } else {
                    break;
                }
            }
        }
    }
    lw
}

/// A snap point: where, what kind, how far.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub x: f64,
    pub y: f64,
    pub kind: &'static str,
    pub d: f64,
}

/// Which kinds of snap are offered.
#[derive(Debug, Clone, Copy)]
pub struct Kinds {
    pub end: bool,
    pub mid: bool,
    pub int: bool,
    pub near: bool,
}
impl Default for Kinds {
    fn default() -> Self {
        Kinds { end: true, mid: true, int: true, near: true }
    }
}

/// A uniform grid over the segments.
#[derive(Debug, Clone)]
pub struct SnapIndex {
    segs: Vec<f64>,
    curve: Vec<u8>,
    n: usize,
    cell: f64,
    x0: f64,
    y0: f64,
    cols: f64,
    rows: f64,
    grid: HashMap<i64, Vec<usize>>,
}

fn seg(g: &[f64], s: usize) -> (f64, f64, f64, f64) {
    let at = |i: usize| g.get(s.saturating_mul(4).saturating_add(i)).copied().unwrap_or(f64::NAN);
    (at(0), at(1), at(2), at(3))
}

impl SnapIndex {
    pub fn new(lw: &Linework) -> Self {
        let n = lw.n;
        let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        let mut i = 0usize;
        while i < n * 4 {
            let x = lw.segs.get(i).copied().unwrap_or(f64::NAN);
            let y = lw.segs.get(i + 1).copied().unwrap_or(f64::NAN);
            if x < x0 {
                x0 = x;
            }
            if x > x1 {
                x1 = x;
            }
            if y < y0 {
                y0 = y;
            }
            if y > y1 {
                y1 = y;
            }
            i += 2;
        }
        if n == 0 {
            x0 = 0.0;
            y0 = 0.0;
            x1 = 1.0;
            y1 = 1.0;
        }
        let span = cc_m3d::js::max(cc_m3d::js::max(x1 - x0, y1 - y0), 1.0);
        let cols0 = cc_m3d::js::max(1.0, cc_m3d::js::min(1024.0, cc_m3d::js::round((n as f64).sqrt() * 2.0)));
        let cell = span / cols0;
        let cols = cc_m3d::js::max(1.0, ((x1 - x0) / cell).ceil() + 1.0);
        let rows = cc_m3d::js::max(1.0, ((y1 - y0) / cell).ceil() + 1.0);
        let curve = if lw.curve.len() >= n { lw.curve.clone() } else { vec![0; n] };
        let mut idx = SnapIndex { segs: lw.segs.clone(), curve, n, cell, x0, y0, cols, rows, grid: HashMap::new() };
        for s in 0..n {
            idx.insert(s);
        }
        idx
    }
    fn cell_of(&self, x: f64, y: f64) -> (f64, f64) {
        (((x - self.x0) / self.cell).floor(), ((y - self.y0) / self.cell).floor())
    }
    fn key(&self, cx: f64, cy: f64) -> i64 {
        let k = cy * self.cols + cx;
        // keys are small integers (cells of a grid at most 1024 × 1024 cells wide plus margins)
        if k.is_finite() && k.abs() < 9e15 {
            #[allow(clippy::cast_possible_truncation)]
            let v = k as i64;
            v
        } else {
            i64::MIN
        }
    }
    fn insert(&mut self, s: usize) {
        let (ax, ay, bx, by) = seg(&self.segs, s);
        let len = hypot2(bx - ax, by - ay);
        let steps_f = cc_m3d::js::max(1.0, (len / (self.cell * 0.5)).ceil());
        let steps = if steps_f.is_finite() && steps_f < 1e7 { steps_f } else { 1.0 };
        let mut last = i64::MIN + 1;
        let mut k = 0.0;
        while k <= steps {
            let t = k / steps;
            let (cx, cy) = self.cell_of(ax + (bx - ax) * t, ay + (by - ay) * t);
            let key = self.key(cx, cy);
            k += 1.0;
            if key == last {
                continue;
            }
            last = key;
            let list = self.grid.entry(key).or_default();
            if list.last() != Some(&s) {
                list.push(s);
            }
        }
    }
    /// Segment numbers whose cells are within `r` of p (deduped, in grid order).
    pub fn near(&self, px: f64, py: f64, r: f64) -> Vec<usize> {
        let (c0, r0) = self.cell_of(px - r, py - r);
        let (c1, r1) = self.cell_of(px + r, py + r);
        let mut seen: Vec<usize> = Vec::new();
        let mut seen_set = std::collections::HashSet::new();
        let mut cy = cc_m3d::js::max(0.0, r0);
        let cy_end = cc_m3d::js::min(self.rows - 1.0, r1);
        let cx_start = cc_m3d::js::max(0.0, c0);
        let cx_end = cc_m3d::js::min(self.cols - 1.0, c1);
        // a pick far outside the drawing asks about no cells; a huge tolerance is cut to the grid
        while cy <= cy_end {
            let mut cx = cx_start;
            while cx <= cx_end {
                if let Some(list) = self.grid.get(&self.key(cx, cy)) {
                    for s in list {
                        if seen_set.insert(*s) {
                            seen.push(*s);
                        }
                    }
                }
                cx += 1.0;
            }
            cy += 1.0;
        }
        seen
    }
    /// The snap point for p within tol, or None.
    pub fn snap(&self, px: f64, py: f64, tol: f64, want: Kinds) -> Option<Hit> {
        let g = &self.segs;
        let cand: Vec<usize> = self.near(px, py, tol).into_iter().filter(|s| dist_seg(px, py, g, *s) <= tol).collect();
        if cand.is_empty() {
            return None;
        }
        let rank = |k: &str| match k {
            "end" => 0,
            "int" => 1,
            _ => 2,
        };
        let mut best: Option<Hit> = None;
        let mut offer = |x: f64, y: f64, kind: &'static str| {
            let d = hypot2(x - px, y - py);
            if d > tol {
                return;
            }
            let better = match &best {
                None => true,
                Some(b) => d < b.d - 1e-9 || ((d - b.d).abs() <= 1e-9 && rank(kind) < rank(b.kind)),
            };
            if better {
                best = Some(Hit { x, y, kind, d });
            }
        };
        for &s in &cand {
            let (ax, ay, bx, by) = seg(g, s);
            let cur = self.curve.get(s).copied().unwrap_or(0);
            if want.end {
                if cur == 0 || cur & 2 != 0 {
                    offer(ax, ay, "end");
                }
                if cur == 0 || cur & 4 != 0 {
                    offer(bx, by, "end");
                }
            }
            if want.mid && cur == 0 {
                offer((ax + bx) / 2.0, (ay + by) / 2.0, "mid");
            }
        }
        if want.int {
            let list: Vec<usize> = cand.iter().take(80).copied().collect();
            for i in 0..list.len() {
                for j in i + 1..list.len() {
                    if let (Some(a), Some(b)) = (list.get(i), list.get(j))
                        && let Some((x, y)) = intersect(g, *a, *b)
                    {
                        offer(x, y, "int");
                    }
                }
            }
        }
        if best.is_some() {
            return best;
        }
        if !want.near {
            return None;
        }
        let mut best: Option<Hit> = None;
        for &s in &cand {
            let (qx, qy) = closest_on_seg(px, py, g, s);
            let d = hypot2(qx - px, qy - py);
            if best.is_none_or(|b| d < b.d) {
                best = Some(Hit { x: qx, y: qy, kind: "near", d });
            }
        }
        best.filter(|b| b.d <= tol)
    }
    pub fn len(&self) -> usize {
        self.n
    }
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }
}

fn closest_on_seg(px: f64, py: f64, g: &[f64], s: usize) -> (f64, f64) {
    let (ax, ay, bx, by) = seg(g, s);
    let dx = bx - ax;
    let dy = by - ay;
    let l = dx * dx + dy * dy;
    let t = if l != 0.0 && !l.is_nan() { cc_m3d::js::max(0.0, cc_m3d::js::min(1.0, ((px - ax) * dx + (py - ay) * dy) / l)) } else { 0.0 };
    (ax + dx * t, ay + dy * t)
}
fn dist_seg(px: f64, py: f64, g: &[f64], s: usize) -> f64 {
    let (qx, qy) = closest_on_seg(px, py, g, s);
    hypot2(qx - px, qy - py)
}

/// Where two segments cross (ends included), or None.
pub fn intersect(g: &[f64], s: usize, t: usize) -> Option<(f64, f64)> {
    let (ax, ay, bx, by) = seg(g, s);
    let (cx, cy, dx, dy) = seg(g, t);
    let (rx, ry, sx, sy) = (bx - ax, by - ay, dx - cx, dy - cy);
    let den = rx * sy - ry * sx;
    if den.abs() < 1e-12 * cc_m3d::js::max(1.0, hypot2(rx, ry) * hypot2(sx, sy)) {
        return None;
    }
    let u = ((cx - ax) * sy - (cy - ay) * sx) / den;
    let v = ((cx - ax) * ry - (cy - ay) * rx) / den;
    let e = 1e-9;
    if u < -e || u > 1.0 + e || v < -e || v > 1.0 + e {
        return None;
    }
    let at_end = |w: f64| w < e || w > 1.0 - e;
    if at_end(u) && at_end(v) {
        return None;
    }
    Some((ax + rx * u, ay + ry * u))
}
