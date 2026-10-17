//! vecdiff.js — compare two sheets by their vector paths (0.4 K12).
//!
//! `pageSegments(opList)` (line segments of every path of a pdf.js operator list, transforms applied, curves cut in
//! eight), `diffSegments(old, new, {tol, T, step})` and `clusters(segs, {gap, minLen})`. The operator list is the
//! part of pdf.js's that the JS reads ([`Op`]); `cc-io-markup` builds it from a page the way pdf.js's evaluator does
//! (path data in 32-bit floats).

use crate::js::{hypot, max, min};

/// `DrawOPS` codes in a path's data.
pub const MOVE_TO: f64 = 0.0;
pub const LINE_TO: f64 = 1.0;
pub const CURVE_TO: f64 = 2.0;
pub const QUAD_TO: f64 = 3.0;
pub const CLOSE: f64 = 4.0;

/// The operators `pageSegments` looks at.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Save,
    Restore,
    /// `cm`
    Transform([f64; 6]),
    /// `paintFormXObjectBegin` with the form's /Matrix (as 32-bit floats), when it has one
    FormBegin(Option<[f64; 6]>),
    FormEnd,
    /// `constructPath`: the path data (None = an empty path)
    Path(Option<Vec<f64>>),
}

pub type Seg = [f64; 4];
type M = [f64; 6];

fn mul(a: &M, b: &M) -> M {
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]
}

fn ap(m: &M, x: f64, y: f64) -> [f64; 2] {
    [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]]
}

/// `pageSegments(opList, OPS, {max})`.
pub fn page_segments(ops: &[Op], max_segs: usize) -> Vec<Seg> {
    let mut out: Vec<Seg> = Vec::new();
    let mut ctm: M = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut stack: Vec<M> = Vec::new();
    for op in ops {
        if out.len() >= max_segs {
            break;
        }
        match op {
            Op::Save => stack.push(ctm),
            Op::Restore => ctm = stack.pop().unwrap_or(ctm),
            Op::Transform(a) => ctm = mul(&ctm, a),
            Op::FormBegin(m) => {
                stack.push(ctm);
                if let Some(m) = m {
                    ctm = mul(&ctm, m);
                }
            }
            Op::FormEnd => ctm = stack.pop().unwrap_or(ctm),
            Op::Path(None) => {}
            Op::Path(Some(data)) => {
                let d = |k: usize| data.get(k).copied().unwrap_or(f64::NAN);
                let (mut x, mut y, mut sx, mut sy) = (0.0, 0.0, 0.0, 0.0);
                let seg = |x1: f64, y1: f64, x2: f64, y2: f64, out: &mut Vec<Seg>| {
                    let p = ap(&ctm, x1, y1);
                    let q = ap(&ctm, x2, y2);
                    if p[0] != q[0] || p[1] != q[1] {
                        out.push([p[0], p[1], q[0], q[1]]);
                    }
                };
                let mut k = 0usize;
                while k < data.len() {
                    let c = d(k);
                    if c == MOVE_TO {
                        x = d(k.saturating_add(1));
                        sx = x;
                        y = d(k.saturating_add(2));
                        sy = y;
                        k = k.saturating_add(3);
                    } else if c == LINE_TO {
                        let (nx, ny) = (d(k.saturating_add(1)), d(k.saturating_add(2)));
                        seg(x, y, nx, ny, &mut out);
                        x = nx;
                        y = ny;
                        k = k.saturating_add(3);
                    } else if c == CURVE_TO || c == QUAD_TO {
                        let cubic = c == CURVE_TO;
                        let n = if cubic { 6 } else { 4 };
                        let pts: Vec<f64> = (1..=n).map(|i| d(k.saturating_add(i))).collect();
                        let g = |i: usize| pts.get(i).copied().unwrap_or(f64::NAN);
                        let (ex, ey) = if cubic { (g(4), g(5)) } else { (g(2), g(3)) };
                        let (mut px, mut py) = (x, y);
                        for j in 1..=8 {
                            let t = f64::from(j) / 8.0;
                            let u = 1.0 - t;
                            let (nx, ny) = if cubic {
                                (
                                    u * u * u * x + 3.0 * u * u * t * g(0) + 3.0 * u * t * t * g(2) + t * t * t * ex,
                                    u * u * u * y + 3.0 * u * u * t * g(1) + 3.0 * u * t * t * g(3) + t * t * t * ey,
                                )
                            } else {
                                (u * u * x + 2.0 * u * t * g(0) + t * t * ex, u * u * y + 2.0 * u * t * g(1) + t * t * ey)
                            };
                            seg(px, py, nx, ny, &mut out);
                            px = nx;
                            py = ny;
                        }
                        x = ex;
                        y = ey;
                        k = k.saturating_add(if cubic { 7 } else { 5 });
                    } else if c == CLOSE {
                        seg(x, y, sx, sy, &mut out);
                        x = sx;
                        y = sy;
                        k = k.saturating_add(1);
                    } else {
                        break;
                    }
                }
            }
        }
    }
    out
}

/// A grid key from two floored cell numbers (None when not finite: the JS loops never run for those).
fn key(gx: f64, gy: f64) -> Option<(i64, i64)> {
    if gx.is_finite() && gy.is_finite() && gx.abs() < 9.0e15 && gy.abs() < 9.0e15 {
        #[allow(clippy::cast_possible_truncation)]
        Some((gx as i64, gy as i64))
    } else {
        None
    }
}

type Grid = std::collections::HashMap<(i64, i64), Vec<usize>>;

fn index(segs: &[Seg], cell: f64) -> Grid {
    let mut g: Grid = Grid::new();
    for (i, s) in segs.iter().enumerate() {
        let x0 = (min(s[0], s[2]) / cell).floor();
        let x1 = (max(s[0], s[2]) / cell).floor();
        let y0 = (min(s[1], s[3]) / cell).floor();
        let y1 = (max(s[1], s[3]) / cell).floor();
        if (x1 - x0 + 1.0) * (y1 - y0 + 1.0) > 4000.0 {
            continue;
        }
        let mut gx = x0;
        while gx <= x1 {
            let mut gy = y0;
            while gy <= y1 {
                if let Some(k) = key(gx, gy) {
                    g.entry(k).or_default().push(i);
                }
                gy += 1.0;
            }
            gx += 1.0;
        }
    }
    g
}

fn dist_seg(px: f64, py: f64, s: &Seg) -> f64 {
    let dx = s[2] - s[0];
    let dy = s[3] - s[1];
    let l = dx * dx + dy * dy;
    let mut t = if l != 0.0 && !l.is_nan() { ((px - s[0]) * dx + (py - s[1]) * dy) / l } else { 0.0 };
    t = max(0.0, min(1.0, t));
    hypot(px - (s[0] + t * dx), py - (s[1] + t * dy))
}

fn near(px: f64, py: f64, segs: &[Seg], grid: &Grid, cell: f64, tol: f64, big: &[usize]) -> bool {
    let gx = (px / cell).floor();
    let gy = (py / cell).floor();
    let mut ix = gx - 1.0;
    while ix <= gx + 1.0 {
        let mut iy = gy - 1.0;
        while iy <= gy + 1.0 {
            if let Some(list) = key(ix, iy).and_then(|k| grid.get(&k)) {
                for i in list {
                    if segs.get(*i).is_some_and(|s| dist_seg(px, py, s) <= tol) {
                        return true;
                    }
                }
            }
            iy += 1.0;
        }
        ix += 1.0;
    }
    big.iter().any(|i| segs.get(*i).is_some_and(|s| dist_seg(px, py, s) <= tol))
}

/// Most sample points one segment is checked at (a hostile file's mile-long line is not sampled every 2 pt —
/// deviation; real sheets need at most a few thousand).
const MAX_SAMPLES: f64 = 1.0e6;

fn unmatched(segs: &[Seg], other: &[Seg], tol: f64, step: f64) -> (Vec<Seg>, usize) {
    let cell = max(tol * 4.0, 8.0);
    let grid = index(other, cell);
    let big: Vec<usize> = other
        .iter()
        .enumerate()
        .filter(|(_, s)| ((s[2] - s[0]).abs() / cell + 1.0) * ((s[3] - s[1]).abs() / cell + 1.0) > 4000.0)
        .map(|(i, _)| i)
        .collect();
    let mut out = Vec::new();
    let mut kept = 0usize;
    for s in segs {
        let len = hypot(s[2] - s[0], s[3] - s[1]);
        let n = max(1.0, (len / step).ceil()).min(MAX_SAMPLES);
        let mut run_from = -1.0;
        let mut j = 0.0;
        while j <= n {
            let t = j / n;
            let x = s[0] + (s[2] - s[0]) * t;
            let y = s[1] + (s[3] - s[1]) * t;
            let miss = !near(x, y, other, &grid, cell, tol, &big);
            if miss && run_from < 0.0 {
                run_from = j;
            }
            if (!miss || j == n) && run_from >= 0.0 {
                let end = if miss { j } else { j - 1.0 };
                if end > run_from || n == 1.0 {
                    let t0 = run_from / n;
                    let t1 = max(end, run_from) / n;
                    let te = if t1 == t0 { min(1.0, t0 + 1.0 / n) } else { t1 };
                    out.push([s[0] + (s[2] - s[0]) * t0, s[1] + (s[3] - s[1]) * t0, s[0] + (s[2] - s[0]) * te, s[1] + (s[3] - s[1]) * te]);
                }
                run_from = -1.0;
            }
            if !miss {
                kept = kept.saturating_add(1);
            }
            j += 1.0;
        }
    }
    (out, kept)
}

/// What [`diff_segments`] gives.
#[derive(Debug, Clone, PartialEq)]
pub struct Diff {
    pub removed: Vec<Seg>,
    pub added: Vec<Seg>,
    pub kept: usize,
}

/// `diffSegments(oldSegs, newSegs, {tol, T, step})`.
pub fn diff_segments(old: &[Seg], new: &[Seg], tol: f64, t: Option<&[f64; 6]>, step: f64) -> Diff {
    let olds: Vec<Seg> = match t {
        Some(m) => old
            .iter()
            .map(|s| {
                let a = ap(m, s[0], s[1]);
                let b = ap(m, s[2], s[3]);
                [a[0], a[1], b[0], b[1]]
            })
            .collect(),
        None => old.to_vec(),
    };
    let (removed, k1) = unmatched(&olds, new, tol, step);
    let (added, k2) = unmatched(new, &olds, tol, step);
    Diff { removed, added, kept: k1.saturating_add(k2) }
}

struct Bx {
    b: [f64; 4],
    n: f64,
}

/// Most grid cells one box is filed under in `clusters` (deviation for hostile sizes only).
const MAX_CELLS: f64 = 4.0e6;

/// `clusters(segs, {gap, minLen})`.
pub fn clusters(segs: &[Seg], gap: f64, min_len: f64) -> Vec<[f64; 4]> {
    let mut list: Vec<Bx> = segs
        .iter()
        .map(|s| Bx { b: [min(s[0], s[2]), min(s[1], s[3]), max(s[0], s[2]), max(s[1], s[3])], n: hypot(s[2] - s[0], s[3] - s[1]) })
        .collect();
    let mut changed = true;
    while changed {
        changed = false;
        let cell = gap * 4.0;
        let mut grid: Grid = Grid::new();
        let mut out: Vec<Bx> = Vec::new();
        for bx in list {
            let mut host: Option<usize> = None;
            let cx0 = ((bx.b[0] - gap) / cell).floor();
            let cx1 = ((bx.b[2] + gap) / cell).floor();
            let cy0 = ((bx.b[1] - gap) / cell).floor();
            let cy1 = ((bx.b[3] + gap) / cell).floor();
            let mut gx = cx0;
            'search: while gx <= cx1 {
                let mut gy = cy0;
                while gy <= cy1 {
                    if let Some(l) = key(gx, gy).and_then(|k| grid.get(&k)) {
                        for &oi in l {
                            let Some(o) = out.get(oi) else { continue };
                            if bx.b[0] - gap <= o.b[2] && o.b[0] - gap <= bx.b[2] && bx.b[1] - gap <= o.b[3] && o.b[1] - gap <= bx.b[3] {
                                host = Some(oi);
                                break 'search;
                            }
                        }
                    }
                    gy += 1.0;
                }
                gx += 1.0;
            }
            if let Some(h) = host.and_then(|h| out.get_mut(h)) {
                h.b = [min(h.b[0], bx.b[0]), min(h.b[1], bx.b[1]), max(h.b[2], bx.b[2]), max(h.b[3], bx.b[3])];
                h.n += bx.n;
                changed = true;
            } else {
                let idx = out.len();
                let (fx0, fx1) = ((bx.b[0] / cell).floor(), (bx.b[2] / cell).floor());
                let (fy0, fy1) = ((bx.b[1] / cell).floor(), (bx.b[3] / cell).floor());
                if (fx1 - fx0 + 1.0) * (fy1 - fy0 + 1.0) <= MAX_CELLS {
                    let mut gx = fx0;
                    while gx <= fx1 {
                        let mut gy = fy0;
                        while gy <= fy1 {
                            if let Some(k) = key(gx, gy) {
                                grid.entry(k).or_default().push(idx);
                            }
                            gy += 1.0;
                        }
                        gx += 1.0;
                    }
                }
                out.push(bx);
            }
        }
        list = out;
    }
    list.into_iter().filter(|x| x.n >= min_len).map(|x| x.b).collect()
}
