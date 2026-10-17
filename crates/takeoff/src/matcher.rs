//! `match.js` — symbol search for automatic counting: normalized cross-correlation (NCC) on "ink" rasters
//! (`Float32`, 0 = paper … 1 = black) at one working scale; the template at 0/90/180/270° (exact) or any angle
//! (bilinear, then refined); integral images; blank windows skipped; 3 × 3 local maxima with a parabola fit.
//! Float32 storage is kept where the JS stores into `Float32Array`s, so scores round the same way.
// Integer arithmetic here is index arithmetic over rasters whose size `raster_ok` checked at every entry point (≤ 40 M px): it cannot overflow, so the crate-wide
// `arithmetic_side_effects` deny is relaxed for this module only.
#![allow(clippy::arithmetic_side_effects)]

use crate::jsmath::{cos, sin};
use cc_m3d::js::{max, min, round};
use std::f64::consts::PI;

/// An ink raster.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Img {
    pub data: Vec<f32>,
    pub w: usize,
    pub h: usize,
}
impl Img {
    #[inline]
    fn get(&self, i: usize) -> f64 {
        f64::from(self.data.get(i).copied().unwrap_or(0.0))
    }
}

/// Largest raster a search takes (the JS skips a sheet over 40 M px at the working scale).
pub const MAX_PIXELS: usize = 40_000_000;

/// A raster of `w` × `h` px is one a search, warp or fill takes: its size computes without overflow (on 32-bit
/// targets too) and stays within [`MAX_PIXELS`]. Every raster entry point checks it first, which is what keeps the
/// index arithmetic of these modules in range.
/// A raster coordinate as a signed number (rasters are ≤ [`MAX_PIXELS`], so it always fits).
fn ix(v: usize) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}
fn isz(v: usize) -> isize {
    isize::try_from(v).unwrap_or(isize::MAX)
}

pub fn raster_ok(w: usize, h: usize) -> bool {
    w.checked_mul(h).is_some_and(|n| n <= MAX_PIXELS)
}

#[inline]
fn f32of(v: f64) -> f32 {
    #[allow(clippy::cast_possible_truncation)]
    let f = v as f32;
    f
}

/// RGBA → ink darkness 0..1.
pub fn to_ink(rgba: &[u8], w: usize, h: usize) -> Img {
    if !raster_ok(w, h) {
        return Img::default();
    }
    let n = w.saturating_mul(h);
    let mut out = vec![0f32; n];
    for (i, o) in out.iter_mut().enumerate() {
        let j = i.saturating_mul(4);
        let px = |k: usize| f64::from(rgba.get(j.saturating_add(k)).copied().unwrap_or(0));
        let a = px(3) / 255.0;
        let lum = (0.299 * px(0) + 0.587 * px(1) + 0.114 * px(2)) / 255.0;
        *o = f32of(1.0 - (lum * a + (1.0 - a)));
    }
    Img { data: out, w, h }
}

fn to_isize(v: f64) -> isize {
    if v.is_nan() {
        return 0;
    }
    #[allow(clippy::cast_possible_truncation)]
    let i = v.clamp(-1e15, 1e15) as isize;
    i
}

/// `crop(img, x0, y0, cw, ch)`. Spec gap: where the JS would throw (a box left of / above the raster with rows to
/// copy), the crop is empty.
pub fn crop(img: &Img, x0: f64, y0: f64, cw: f64, ch: f64) -> Img {
    let x0 = max(0.0, round(x0));
    let y0 = max(0.0, round(y0));
    let cw = min(img.w as f64 - x0, round(cw));
    let ch = min(img.h as f64 - y0, round(ch));
    let (wi, hi) = (to_isize(max(0.0, cw)), to_isize(max(0.0, ch)));
    let (wu, hu) = (usize::try_from(wi).unwrap_or(0), usize::try_from(hi).unwrap_or(0));
    let (xu, yu) = (usize::try_from(to_isize(x0)).unwrap_or(0), usize::try_from(to_isize(y0)).unwrap_or(0));
    let mut data = vec![0f32; wu.saturating_mul(hu)];
    for y in 0..hu {
        let src = (yu + y) * img.w + xu;
        if let (Some(s), Some(d)) = (img.data.get(src..src + wu), data.get_mut(y * wu..(y + 1) * wu)) {
            d.copy_from_slice(s);
        }
    }
    Img { data, w: wu, h: hu }
}

/// Rotate a raster 90° clockwise (y down).
pub fn rot90(t: &Img) -> Img {
    if !raster_ok(t.w, t.h) {
        return Img::default();
    }
    let mut out = vec![0f32; t.data.len()];
    for y in 0..t.h {
        for x in 0..t.w {
            if let Some(o) = out.get_mut(x * t.h + (t.h - 1 - y)) {
                *o = t.data.get(y * t.w + x).copied().unwrap_or(0.0);
            }
        }
    }
    Img { data: out, w: t.h, h: t.w }
}

/// Rotate a raster by `deg` clockwise about its middle, into the box that holds it all (bilinear, paper outside).
pub fn rotate(t: &Img, deg: f64) -> Img {
    if !raster_ok(t.w, t.h) {
        return Img::default();
    }
    let r = ((deg % 360.0) + 360.0) % 360.0;
    if r == 0.0 {
        return t.clone();
    }
    if r % 90.0 == 0.0 {
        let mut o = t.clone();
        let mut k = 0.0;
        while k < r / 90.0 {
            o = rot90(&o);
            k += 1.0;
        }
        return o;
    }
    let a = r * PI / 180.0;
    let (c, s) = (cos(a), sin(a));
    let wf = ((t.w as f64 * c).abs() + (t.h as f64 * s).abs()).ceil();
    let hf = ((t.w as f64 * s).abs() + (t.h as f64 * c).abs()).ceil();
    let w = usize::try_from(to_isize(wf)).unwrap_or(0);
    let h = usize::try_from(to_isize(hf)).unwrap_or(0);
    let mut out = vec![0f32; w.saturating_mul(h)];
    let (cx0, cy0, cx1, cy1) = (t.w as f64 / 2.0, t.h as f64 / 2.0, w as f64 / 2.0, h as f64 / 2.0);
    let at = |xx: f64, yy: f64| -> f64 {
        if xx >= 0.0 && yy >= 0.0 && xx < t.w as f64 && yy < t.h as f64 {
            t.get(usize::try_from(to_isize(yy * t.w as f64 + xx)).unwrap_or(0))
        } else {
            0.0
        }
    };
    for y in 0..h {
        for x in 0..w {
            let dx = x as f64 + 0.5 - cx1;
            let dy = y as f64 + 0.5 - cy1;
            let u = c * dx + s * dy + cx0 - 0.5;
            let v = -s * dx + c * dy + cy0 - 0.5;
            let (x0, y0) = (u.floor(), v.floor());
            let (fx, fy) = (u - x0, v - y0);
            let val = (at(x0, y0) * (1.0 - fx) + at(x0 + 1.0, y0) * fx) * (1.0 - fy)
                + (at(x0, y0 + 1.0) * (1.0 - fx) + at(x0 + 1.0, y0 + 1.0) * fx) * fy;
            if let Some(o) = out.get_mut(y * w + x) {
                *o = f32of(val);
            }
        }
    }
    Img { data: out, w, h }
}

/// Every 15°.
pub fn any_angles() -> Vec<f64> {
    (0..24).map(|i| f64::from(i) * 15.0).collect()
}

/// A 3 × 3 binomial blur.
pub fn blur(t: &Img, passes: usize) -> Img {
    if !raster_ok(t.w, t.h) {
        return Img::default();
    }
    let mut a = t.data.clone();
    let (w, h) = (t.w, t.h);
    for _ in 0..passes {
        let mut b = vec![0f32; a.len()];
        let mut c = vec![0f32; a.len()];
        let g = |v: &[f32], i: usize| f64::from(v.get(i).copied().unwrap_or(0.0));
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                let l = if x > 0 { g(&a, i - 1) } else { g(&a, i) };
                let r = if x + 1 < w { g(&a, i + 1) } else { g(&a, i) };
                if let Some(o) = b.get_mut(i) {
                    *o = f32of((l + 2.0 * g(&a, i) + r) / 4.0);
                }
            }
        }
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                let u = if y > 0 { g(&b, i - w) } else { g(&b, i) };
                let d = if y + 1 < h { g(&b, i + w) } else { g(&b, i) };
                if let Some(o) = c.get_mut(i) {
                    *o = f32of((u + 2.0 * g(&b, i) + d) / 4.0);
                }
            }
        }
        a = c;
    }
    Img { data: a, w, h }
}

/// A trimmed template and where it sat in the box.
#[derive(Debug, Clone, PartialEq)]
pub struct Trimmed {
    pub img: Img,
    pub ox: usize,
    pub oy: usize,
}
/// Trim blank paper around the template (keeps a 1 px margin).
pub fn trim(t: &Img, ink: f64) -> Trimmed {
    let (mut x0, mut y0, mut x1, mut y1) = (ix(t.w), ix(t.h), -1i64, -1i64);
    for y in 0..t.h {
        for x in 0..t.w {
            if t.get(y * t.w + x) > ink {
                let (xi, yi) = (ix(x), ix(y));
                if xi < x0 {
                    x0 = xi;
                }
                if xi > x1 {
                    x1 = xi;
                }
                if yi < y0 {
                    y0 = yi;
                }
                if yi > y1 {
                    y1 = yi;
                }
            }
        }
    }
    if x1 < 0 {
        return Trimmed { img: t.clone(), ox: 0, oy: 0 };
    }
    let x0 = (x0 - 1).max(0);
    let y0 = (y0 - 1).max(0);
    let x1 = (x1 + 1).min(ix(t.w) - 1);
    let y1 = (y1 + 1).min(ix(t.h) - 1);
    let img = crop(t, x0 as f64, y0 as f64, (x1 - x0 + 1) as f64, (y1 - y0 + 1) as f64);
    Trimmed { img, ox: usize::try_from(x0).unwrap_or(0), oy: usize::try_from(y0).unwrap_or(0) }
}

fn stats(t: &Img) -> (f64, f64) {
    let n = t.data.len() as f64;
    let (mut s, mut s2) = (0.0, 0.0);
    for v in &t.data {
        let v = f64::from(*v);
        s += v;
        s2 += v * v;
    }
    let mean = s / n;
    let var = s2 / n - mean * mean;
    (mean, max(0.0, var).sqrt())
}

/// Why a template can't be searched for, or None.
pub fn template_problem(tpl: &Img) -> Option<String> {
    if tpl.w < 4 || tpl.h < 4 {
        return Some(crate::i18n::t("ac.err.small"));
    }
    if stats(tpl).1 < 0.03 {
        return Some(crate::i18n::t("ac.err.empty"));
    }
    None
}

/// Integral images of the values and their squares.
#[derive(Debug, Clone)]
pub struct Integrals {
    s: Vec<f64>,
    s2: Vec<f64>,
    w: usize,
}
pub fn integrals(img: &Img) -> Integrals {
    let ww = img.w + 1;
    let size = ww * (img.h + 1);
    let mut s = vec![0f64; size];
    let mut s2 = vec![0f64; size];
    for y in 0..img.h {
        let (mut r, mut r2) = (0.0, 0.0);
        for x in 0..img.w {
            let v = img.get(y * img.w + x);
            r += v;
            r2 += v * v;
            let up = s.get(y * ww + x + 1).copied().unwrap_or(0.0);
            let up2 = s2.get(y * ww + x + 1).copied().unwrap_or(0.0);
            if let Some(o) = s.get_mut((y + 1) * ww + x + 1) {
                *o = up + r;
            }
            if let Some(o) = s2.get_mut((y + 1) * ww + x + 1) {
                *o = up2 + r2;
            }
        }
    }
    Integrals { s, s2, w: ww }
}

/// A local peak of the score.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Peak {
    pub x: usize,
    pub y: usize,
    pub dx: f64,
    pub dy: f64,
    pub score: f64,
    pub raw: f64,
}

/// Progress and stop hooks of a search.
pub struct Hooks<'a> {
    pub tick: Option<&'a mut dyn FnMut(usize, usize)>,
    pub stop: Option<&'a dyn Fn() -> bool>,
}

/// NCC of one template over the image → local peaks ≥ floor. None when stopped.
pub fn ncc_peaks(img: &Img, t: &Img, floor: f64, min_std: f64, ints: Option<&Integrals>, hooks: &mut Hooks<'_>) -> Option<Vec<Peak>> {
    if !raster_ok(img.w, img.h) || !raster_ok(t.w, t.h) {
        return Some(Vec::new());
    }
    let (tm, ts) = stats(t);
    let n = t.w * t.h;
    let nf = n as f64;
    let tz: Vec<f32> = t.data.iter().map(|v| f32of(f64::from(*v) - tm)).collect();
    let tnorm = ts * nf.sqrt();
    if img.w < t.w || img.h < t.h {
        return Some(Vec::new());
    }
    let ww = img.w - t.w + 1;
    let hh = img.h - t.h + 1;
    let own;
    let iv = match ints {
        Some(i) => i,
        None => {
            own = integrals(img);
            &own
        }
    };
    let (s, s2, iw) = (&iv.s, &iv.s2, iv.w);
    let mut score = vec![0f32; ww * hh];
    let g = |v: &Vec<f64>, i: usize| v.get(i).copied().unwrap_or(0.0);
    let d = &img.data;
    let dw = img.w;
    for y in 0..hh {
        for x in 0..ww {
            let a = y * iw + x;
            let b = a + t.w;
            let c = (y + t.h) * iw + x;
            let e = c + t.w;
            let sum = g(s, e) - g(s, b) - g(s, c) + g(s, a);
            let sum2 = g(s2, e) - g(s2, b) - g(s2, c) + g(s2, a);
            let v = sum2 / nf - (sum / nf) * (sum / nf);
            if v < min_std * min_std {
                continue;
            }
            let mut dot = 0.0f64;
            let mut k = 0usize;
            for ty in 0..t.h {
                let row = (y + ty) * dw + x;
                let (Some(trow), Some(drow)) = (tz.get(k..k + t.w), d.get(row..row + t.w)) else { break };
                for (tv, dv) in trow.iter().zip(drow) {
                    dot += f64::from(*tv) * f64::from(*dv);
                }
                k += t.w;
            }
            if let Some(o) = score.get_mut(y * ww + x) {
                *o = f32of(dot / (tnorm * (v * nf).sqrt()));
            }
        }
        if hooks.tick.is_some() && (y & 15) == 15 {
            if hooks.stop.is_some_and(|st| st()) {
                return None;
            }
            if let Some(tk) = hooks.tick.as_mut() {
                tk(y + 1, hh);
            }
        }
    }
    let sc = |x: isize, y: isize| -> Option<f64> {
        let (Ok(xu), Ok(yu)) = (usize::try_from(x), usize::try_from(y)) else { return None };
        if xu >= ww || yu >= hh {
            return None;
        }
        score.get(yu * ww + xu).map(|v| f64::from(*v))
    };
    let mut peaks = Vec::new();
    for y in 0..hh {
        for x in 0..ww {
            let v = f64::from(score.get(y * ww + x).copied().unwrap_or(0.0));
            if v < floor {
                continue;
            }
            let mut best = true;
            'outer: for dy in -1isize..=1 {
                for dx in -1isize..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let Some(u) = sc(isz(x) + dx, isz(y) + dy) else { continue };
                    if u > v || (u == v && (dy < 0 || (dy == 0 && dx < 0))) {
                        best = false;
                        break 'outer;
                    }
                }
            }
            if best {
                let at = |xx: isize, yy: isize| sc(xx, yy).unwrap_or(v);
                let fit = |a: f64, c: f64| -> (f64, f64) {
                    let den = a - 2.0 * v + c;
                    if den >= 0.0 {
                        return (0.0, 0.0);
                    }
                    let dd = max(-0.5, min(0.5, (a - c) / (2.0 * den)));
                    (dd, -(a - c) * dd / 4.0)
                };
                let (xi, yi) = (isz(x), isz(y));
                let (dx, gx) = fit(at(xi - 1, yi), at(xi + 1, yi));
                let (dy, gy) = fit(at(xi, yi - 1), at(xi, yi + 1));
                peaks.push(Peak { x, y, dx, dy, score: min(1.0, v + max(0.0, gx) + max(0.0, gy)), raw: v });
            }
        }
    }
    Some(peaks)
}

/// A match candidate (image px).
#[derive(Debug, Clone, PartialEq)]
pub struct Cand {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub rot: f64,
    pub score: f64,
    pub sheet: Option<String>,
}

/// The default blur (off: it cost precision on Carey).
pub const BLUR: usize = 0;

/// Search one image for a template at each rotation. None when stopped.
pub fn find_symbol(
    img0: &Img,
    template0: &Img,
    rotations: &[f64],
    floor: f64,
    smooth: usize,
    progress: Option<&mut dyn FnMut(f64)>,
    stop: Option<&dyn Fn() -> bool>,
) -> Option<Vec<Cand>> {
    if !raster_ok(img0.w, img0.h) || !raster_ok(template0.w, template0.h) {
        return Some(Vec::new());
    }
    let (img, template) = if smooth > 0 { (blur(img0, smooth), blur(template0, smooth)) } else { (img0.clone(), template0.clone()) };
    let ints = integrals(&img);
    let mut out = Vec::new();
    let mut list: Vec<f64> = Vec::new();
    for r in rotations {
        let v = ((round(*r) % 360.0) + 360.0) % 360.0;
        if !list.iter().any(|x| *x == v || (x.is_nan() && v.is_nan())) {
            list.push(v);
        }
    }
    let fine = list.iter().any(|r| r % 90.0 != 0.0);
    let coarse_floor = if fine { max(0.3, floor - 0.2) } else { floor };
    let mut progress = progress;
    let nl = list.len() as f64;
    for (i, r) in list.iter().enumerate() {
        let tt = rotate(&template, *r);
        let has_progress = progress.is_some();
        let peaks = {
            let mut tick_fn = |done: usize, total: usize| {
                if let Some(p) = progress.as_mut() {
                    p((i as f64 + done as f64 / total as f64) / nl * if fine { 0.8 } else { 1.0 });
                }
            };
            let mut hooks = Hooks { tick: if has_progress { Some(&mut tick_fn) } else { None }, stop };
            ncc_peaks(&img, &tt, coarse_floor, 0.02, Some(&ints), &mut hooks)?
        };
        for p in peaks {
            out.push(Cand { x: p.x as f64, y: p.y as f64, w: tt.w as f64, h: tt.h as f64, rot: *r, score: p.score, sheet: None });
        }
    }
    if !fine {
        if let Some(p) = progress.as_mut() {
            p(1.0);
        }
        return Some(out);
    }
    let best: Vec<Cand> = suppress(&out, coarse_floor, 0.25).into_iter().take(400).collect();
    let mut refined = Vec::new();
    let nb = best.len() as f64;
    for (k, c) in best.iter().enumerate() {
        if stop.is_some_and(|s| s()) {
            return None;
        }
        let r = refine_angle(&img, &template, c);
        if r.score >= floor {
            refined.push(r);
        }
        if let Some(p) = progress.as_mut() {
            p(0.8 + 0.2 * (k as f64 + 1.0) / nb);
        }
    }
    if let Some(p) = progress.as_mut() {
        p(1.0);
    }
    Some(refined)
}

/// Try angles within ±7.5° of a candidate's (1.5° steps) on a window around it; the best fit.
pub fn refine_angle(img: &Img, template: &Img, c: &Cand) -> Cand {
    let cx = c.x + c.w / 2.0;
    let cy = c.y + c.h / 2.0;
    let mut best = c.clone();
    let mut d = -7.5f64;
    while d <= 7.5 + 1e-9 {
        let r = ((c.rot + d) % 360.0 + 360.0) % 360.0;
        let tt = rotate(template, r);
        let pad = 3.0;
        let x0 = max(0.0, (cx - tt.w as f64 / 2.0).floor() - pad);
        let y0 = max(0.0, (cy - tt.h as f64 / 2.0).floor() - pad);
        let win = crop(img, x0, y0, tt.w as f64 + 2.0 * pad, tt.h as f64 + 2.0 * pad);
        d += 1.5;
        if win.w < tt.w || win.h < tt.h {
            continue;
        }
        let mut hooks = Hooks { tick: None, stop: None };
        for p in ncc_peaks(&win, &tt, 0.0, 0.02, None, &mut hooks).unwrap_or_default() {
            if p.score > best.score + 1e-9 {
                best = Cand {
                    x: x0 + p.x as f64,
                    y: y0 + p.y as f64,
                    w: tt.w as f64,
                    h: tt.h as f64,
                    rot: r,
                    score: p.score,
                    sheet: best.sheet.clone(),
                };
            }
        }
    }
    best
}

fn iou(a: &Cand, b: &Cand) -> f64 {
    let ix = max(0.0, min(a.x + a.w, b.x + b.w) - max(a.x, b.x));
    let iy = max(0.0, min(a.y + a.h, b.y + b.h) - max(a.y, b.y));
    let i = ix * iy;
    i / (a.w * a.h + b.w * b.h - i)
}

/// Keep candidates ≥ threshold, best first, dropping any that overlaps a kept one by IoU > maxOverlap.
pub fn suppress(cands: &[Cand], threshold: f64, max_overlap: f64) -> Vec<Cand> {
    let mut list: Vec<Cand> = cands.iter().filter(|c| c.score >= threshold).cloned().collect();
    cc_m3d::js::sort_by(&mut list, |a, b| b.score - a.score);
    let mut keep: Vec<Cand> = Vec::new();
    for c in list {
        if !keep.iter().any(|k| k.sheet == c.sheet && iou(k, &c) > max_overlap) {
            keep.push(c);
        }
    }
    keep
}

/// Working scale (px per PDF unit) that makes the template's longer side about `target` px.
pub fn work_scale(box_w: f64, box_h: f64, target: f64) -> f64 {
    min(4.0, max(0.2, target / max(box_w, box_h)))
}
