//! `overlay.js` — revision overlay (T13): line an old sheet up with its revision (a similarity from two point pairs),
//! see what changed on ink rasters (`Float32` 0 = paper … 1 = ink), and how much changed under each mark.
// Integer arithmetic here is index arithmetic over rasters whose size `raster_ok` checked at every entry point (≤ 40 M px): it cannot overflow, so the crate-wide
// `arithmetic_side_effects` deny is relaxed for this module only.
#![allow(clippy::arithmetic_side_effects)]

use crate::geom::{self, M6, P, apply, flatten, invert, multiply};
use crate::matcher::{Img, raster_ok};
use cc_m3d::js::{hypot2, max, min};

pub const IDENTITY: M6 = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// The similarity taking a1 → b1 and a2 → b2 (no mirror), or None.
pub fn fit_two_points(a1: &P, a2: &P, b1: &P, b2: &P) -> Option<M6> {
    let (ax, ay, bx, by) = (a2.x - a1.x, a2.y - a1.y, b2.x - b1.x, b2.y - b1.y);
    let la = ax * ax + ay * ay;
    if !(la > 0.0) || !(bx * bx + by * by > 0.0) {
        return None;
    }
    let c = (bx * ax + by * ay) / la;
    let s = (by * ax - bx * ay) / la;
    Some([c, s, -s, c, b1.x - (c * a1.x - s * a1.y), b1.y - (s * a1.x + c * a1.y)])
}
/// The uniform scale of a similarity.
pub fn scale_of(m: &M6) -> f64 {
    hypot2(m[0], m[1])
}

/// Pixel map: NEW raster pixel → OLD raster pixel.
pub fn pixel_map(tn: &M6, to: &M6, a: &M6, px: f64) -> M6 {
    let s = [px, 0.0, 0.0, px, 0.0, 0.0];
    let si = [1.0 / px, 0.0, 0.0, 1.0 / px, 0.0, 0.0];
    multiply(&s, &multiply(to, &multiply(&invert(a), &multiply(&invert(tn), &si))))
}

fn at(src: &Img, xx: f64, yy: f64) -> f64 {
    if xx >= 0.0 && yy >= 0.0 && xx < src.w as f64 && yy < src.h as f64 {
        let i = yy * src.w as f64 + xx;
        if i >= 0.0 && i < src.data.len() as f64 {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            return src.data.get(i as usize).map_or(0.0, |v| f64::from(*v));
        }
    }
    0.0
}

/// The old raster resampled into the new raster's pixels.
pub fn warp(src: &Img, m: &M6, w: usize, h: usize) -> Img {
    if !raster_ok(w, h) || !raster_ok(src.w, src.h) {
        return Img::default();
    }
    let mut out = vec![0f32; w.saturating_mul(h)];
    let (sw, sh) = (src.w as f64, src.h as f64);
    for y in 0..h {
        let cy = y as f64 + 0.5;
        for x in 0..w {
            let cx = x as f64 + 0.5;
            let u = m[0] * cx + m[2] * cy + m[4] - 0.5;
            let v = m[1] * cx + m[3] * cy + m[5] - 0.5;
            let x0 = u.floor();
            let y0 = v.floor();
            if x0 < -1.0 || y0 < -1.0 || x0 >= sw || y0 >= sh {
                continue;
            }
            let fx = u - x0;
            let fy = v - y0;
            let val = (at(src, x0, y0) * (1.0 - fx) + at(src, x0 + 1.0, y0) * fx) * (1.0 - fy)
                + (at(src, x0, y0 + 1.0) * (1.0 - fx) + at(src, x0 + 1.0, y0 + 1.0) * fx) * fy;
            if let Some(o) = out.get_mut(y * w + x) {
                #[allow(clippy::cast_possible_truncation)]
                let f = val as f32;
                *o = f;
            }
        }
    }
    Img { data: out, w, h }
}

/// Max filter with a square of radius r (separable).
pub fn dilate(img: &Img, r: usize) -> Img {
    let (w, h) = (img.w, img.h);
    if !raster_ok(w, h) {
        return Img::default();
    }
    let r = r.min(w.max(h));
    if r == 0 {
        return img.clone();
    }
    let d = |i: usize| img.data.get(i).copied().unwrap_or(0.0);
    let mut tmp = vec![0f32; w * h];
    let mut out = vec![0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut m = 0f32;
            for k in x.saturating_sub(r)..=w.saturating_sub(1).min(x.saturating_add(r)) {
                let v = d(y * w + k);
                if v > m {
                    m = v;
                }
            }
            if let Some(o) = tmp.get_mut(y * w + x) {
                *o = m;
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            let mut m = 0f32;
            for k in y.saturating_sub(r)..=h.saturating_sub(1).min(y.saturating_add(r)) {
                let v = tmp.get(k * w + x).copied().unwrap_or(0.0);
                if v > m {
                    m = v;
                }
            }
            if let Some(o) = out.get_mut(y * w + x) {
                *o = m;
            }
        }
    }
    Img { data: out, w, h }
}

/// Where two sheets differ: 1 = ink only in the old (removed), 2 = only in the new (added).
#[derive(Debug, Clone)]
pub struct ChangeMask {
    pub mask: Vec<u8>,
    pub w: usize,
    pub h: usize,
    pub removed: usize,
    pub added: usize,
    pub sum: Vec<u32>,
}
pub fn change_mask(old: &Img, new: &Img, r: usize, on: f64, off: f64) -> ChangeMask {
    let (w, h) = (new.w, new.h);
    if !raster_ok(w, h) || !raster_ok(old.w, old.h) {
        return ChangeMask { mask: Vec::new(), w: 0, h: 0, removed: 0, added: 0, sum: Vec::new() };
    }
    let d_old = dilate(old, r).data;
    let d_new = dilate(new, r).data;
    let mut mask = vec![0u8; w * h];
    let (mut removed, mut added) = (0usize, 0usize);
    for (i, m) in mask.iter_mut().enumerate() {
        let a = f64::from(old.data.get(i).copied().unwrap_or(0.0));
        let b = f64::from(new.data.get(i).copied().unwrap_or(0.0));
        let dn = f64::from(d_new.get(i).copied().unwrap_or(0.0));
        let dold = f64::from(d_old.get(i).copied().unwrap_or(0.0));
        if a >= on && dn < off {
            *m = 1;
            removed += 1;
        } else if b >= on && dold < off {
            *m = 2;
            added += 1;
        }
    }
    let sum = sum_of(&mask, w, h);
    ChangeMask { mask, w, h, removed, added, sum }
}

/// How much the drawing changed under a mark.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarkChange {
    pub removed: usize,
    pub added: usize,
    pub total: usize,
    pub share: f64,
    pub corner: bool,
}

/// `markChange(m, cm, toPx, { buffer })` for a mark of `kind` with its points and cut-outs.
pub fn mark_change(kind: &str, pts: &[P], holes: &[Vec<P>], cm: &ChangeMask, to_px: &dyn Fn(&P) -> P, buffer: f64) -> MarkChange {
    let (w, h) = (cm.w, cm.h);
    let any = |x: f64, y: f64| box_sum(&cm.sum, w, h, x - buffer, y - buffer, x + buffer, y + buffer) > 0;
    let mut rings: Vec<(&[P], bool)> = Vec::new();
    if kind == "area" {
        rings.push((pts, true));
        for r in holes {
            rings.push((r, true));
        }
    } else {
        rings.push((pts, false));
    }
    let mut segs: Vec<(P, P)> = Vec::new();
    let mut corners: Vec<P> = Vec::new();
    for (ring, closed) in rings {
        for p in ring {
            corners.push(to_px(p));
        }
        let fp: Vec<P> = flatten(ring, closed).iter().map(to_px).collect();
        if let [one] = fp.as_slice() {
            segs.push((*one, *one));
        }
        for wv in fp.windows(2) {
            if let [a, b] = wv {
                segs.push((*a, *b));
            }
        }
        if closed
            && fp.len() > 2
            && let (Some(l), Some(f)) = (fp.last(), fp.first())
        {
            segs.push((*l, *f));
        }
    }
    if segs.is_empty() {
        return MarkChange { removed: 0, added: 0, total: 0, share: 0.0, corner: false };
    }
    let (mut samples, mut hit) = (0.0f64, 0.0f64);
    for (a, b) in &segs {
        let n = max(1.0, hypot2(b.x - a.x, b.y - a.y).ceil());
        let n = if n.is_finite() && n < 1e7 { n } else { 1.0 };
        let mut k = 0.0;
        while k <= n {
            samples += 1.0;
            if any(a.x + (b.x - a.x) * k / n, a.y + (b.y - a.y) * k / n) {
                hit += 1.0;
            }
            k += 1.0;
        }
    }
    let corner = corners.iter().any(|c| any(c.x, c.y));
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for (p, q) in &segs {
        for s in [p, q] {
            x0 = min(x0, s.x);
            y0 = min(y0, s.y);
            x1 = max(x1, s.x);
            y1 = max(y1, s.y);
        }
    }
    let bx0 = max(0.0, (x0 - buffer).floor());
    let by0 = max(0.0, (y0 - buffer).floor());
    let bx1 = min(w as f64 - 1.0, (x1 + buffer).ceil());
    let by1 = min(h as f64 - 1.0, (y1 + buffer).ceil());
    let b2 = buffer * buffer;
    let (mut removed, mut added) = (0usize, 0usize);
    if bx0.is_finite() && by0.is_finite() && bx1.is_finite() && by1.is_finite() {
        let mut y = by0;
        while y <= by1 {
            let mut x = bx0;
            while x <= bx1 {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let v = cm.mask.get((y * w as f64 + x) as usize).copied().unwrap_or(0);
                if v != 0 {
                    let p = P::new(x + 0.5, y + 0.5);
                    if segs.iter().any(|(a, b)| d2seg(&p, a, b) <= b2) {
                        if v == 1 {
                            removed += 1;
                        } else {
                            added += 1;
                        }
                    }
                }
                x += 1.0;
            }
            y += 1.0;
        }
    }
    MarkChange { removed, added, total: removed + added, share: if samples > 0.0 { hit / samples } else { 0.0 }, corner }
}

/// The flag rule.
pub fn changed(r: &MarkChange, kind: &str, share: f64) -> bool {
    if kind == "count" { r.total > 0 } else { r.corner || r.share >= share }
}

/// Summed-area table of a mask (nonzero = 1), (w + 1) × (h + 1).
pub fn sum_of(mask: &[u8], w: usize, h: usize) -> Vec<u32> {
    if !raster_ok(w.saturating_add(1), h.saturating_add(1)) {
        return Vec::new();
    }
    let ww = w + 1;
    let mut s = vec![0u32; ww * (h + 1)];
    for y in 0..h {
        let mut row = 0u32;
        for x in 0..w {
            row = row.wrapping_add(u32::from(mask.get(y * w + x).is_some_and(|v| *v != 0)));
            let up = s.get(y * ww + x + 1).copied().unwrap_or(0);
            if let Some(o) = s.get_mut((y + 1) * ww + x + 1) {
                *o = up.wrapping_add(row);
            }
        }
    }
    s
}
/// How many nonzero mask pixels in the box [x0, x1] × [y0, y1] (clipped).
pub fn box_sum(s: &[u32], w: usize, h: usize, x0: f64, y0: f64, x1: f64, y1: f64) -> i64 {
    let x0 = max(0.0, x0.floor());
    let y0 = max(0.0, y0.floor());
    let x1 = min(w as f64 - 1.0, x1.floor());
    let y1 = min(h as f64 - 1.0, y1.floor());
    if !(x1 >= x0) || !(y1 >= y0) {
        return 0;
    }
    let ww = (w + 1) as f64;
    let g = |i: f64| -> i64 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let v = s.get(i as usize).copied().unwrap_or(0);
        i64::from(v)
    };
    g((y1 + 1.0) * ww + x1 + 1.0) - g(y0 * ww + x1 + 1.0) - g((y1 + 1.0) * ww + x0) + g(y0 * ww + x0)
}

fn d2seg(p: &P, a: &P, b: &P) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let l = dx * dx + dy * dy;
    let t = if l != 0.0 && !l.is_nan() { max(0.0, min(1.0, ((p.x - a.x) * dx + (p.y - a.y) * dy) / l)) } else { 0.0 };
    let x = a.x + dx * t - p.x;
    let y = a.y + dy * t - p.y;
    x * x + y * y
}

/// A point list moved by A (bulges kept).
pub fn move_pts(pts: &[P], a: &M6) -> Vec<P> {
    pts.iter()
        .map(|p| {
            let q = apply(a, p);
            if p.has_b() { P::arc(q.x, q.y, p.b) } else { geom::P::new(q.x, q.y) }
        })
        .collect()
}
