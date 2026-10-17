//! `roomfill.js` — the room around a point by flood fill (T21): lines thickened by half the door gap, flood fill, grow
//! back to the lines' faces, close slits, trace the outer boundary along pixel edges, Douglas–Peucker.
// Integer arithmetic here is index arithmetic over rasters whose size `raster_ok` checked at every entry point (≤ 40 M px): it cannot overflow, so the crate-wide
// `arithmetic_side_effects` deny is relaxed for this module only.
#![allow(clippy::arithmetic_side_effects)]

use crate::geom::P;
use crate::matcher::{Img, raster_ok};

/// Binary max filter with a (2r+1)² square, in O(n) whatever r (van Herk / Gil–Werman, by rows then columns).
pub fn dilate_bin(src: &[u8], w: usize, h: usize, r: usize) -> Vec<u8> {
    if !raster_ok(w, h) {
        return Vec::new();
    }
    let r = r.min(w.max(h));
    if r == 0 {
        return src.to_vec();
    }
    let k = 2 * r + 1;
    let pass = |get: &dyn Fn(usize, usize) -> u8, set: &mut dyn FnMut(usize, usize, u8), len: usize, count: usize| {
        let n = len + 2 * r;
        let mut g = vec![0u8; len + 2 * r + k];
        let mut hh = vec![0u8; len + 2 * r + k];
        for line in 0..count {
            let at = |i: usize| if i >= r && i < r + len { get(line, i - r) } else { 0 };
            for i in 0..n {
                let v = if i % k == 0 { at(i) } else { g.get(i - 1).copied().unwrap_or(0) | at(i) };
                if let Some(o) = g.get_mut(i) {
                    *o = v;
                }
            }
            for i in (0..n).rev() {
                let v = if i == n - 1 || (i + 1) % k == 0 { at(i) } else { hh.get(i + 1).copied().unwrap_or(0) | at(i) };
                if let Some(o) = hh.get_mut(i) {
                    *o = v;
                }
            }
            for x in 0..len {
                let a = x;
                let b = x + 2 * r;
                let v = hh.get(a).copied().unwrap_or(0) | if b < n { g.get(b).copied().unwrap_or(0) } else { 0 };
                set(line, x, v);
            }
        }
    };
    let mut tmp = vec![0u8; w * h];
    let mut out = vec![0u8; w * h];
    pass(
        &|y, x| src.get(y * w + x).copied().unwrap_or(0),
        &mut |y, x, v| {
            if let Some(o) = tmp.get_mut(y * w + x) {
                *o = v;
            }
        },
        w,
        h,
    );
    let tmp2 = tmp;
    pass(
        &|x, y| tmp2.get(y * w + x).copied().unwrap_or(0),
        &mut |x, y, v| {
            if let Some(o) = out.get_mut(y * w + x) {
                *o = v;
            }
        },
        h,
        w,
    );
    out
}

/// `roomAt` → the ring (pixel corners) and its area in pixels, or why not ('wall' | 'open' | 'small').
pub fn room_at(ink: &Img, sx0: f64, sy0: f64, gap_px: f64, on: f64, max_share: f64) -> Result<(Vec<P>, usize), &'static str> {
    let (w, h) = (ink.w, ink.h);
    if !raster_ok(w, h) {
        return Err("open");
    }
    let (sxf, syf) = (sx0.floor(), sy0.floor());
    if !(sxf >= 0.0 && syf >= 0.0 && sxf < w as f64 && syf < h as f64) {
        return Err("open");
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (sx, sy) = (sxf as usize, syf as usize);
    let wall: Vec<u8> = (0..w * h).map(|i| u8::from(f64::from(ink.data.get(i).copied().unwrap_or(0.0)) >= on)).collect();
    if wall.get(sy * w + sx).copied().unwrap_or(0) != 0 {
        return Err("wall");
    }
    let rf = cc_m3d::js::max(1.0, cc_m3d::js::round(gap_px / 2.0));
    let rf = if rf.is_finite() { rf.min(1e6) } else { 1.0 };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let r = rf as usize;
    let closed = dilate_bin(&wall, w, h, r);
    let mut start = sy * w + sx;
    if closed.get(start).copied().unwrap_or(0) != 0 {
        let mut best: Option<usize> = None;
        let mut bd = f64::INFINITY;
        for y in sy.saturating_sub(r)..=(sy + r).min(h - 1) {
            for x in sx.saturating_sub(r)..=(sx + r).min(w - 1) {
                let dx = x as f64 - sx as f64;
                let dy = y as f64 - sy as f64;
                let d = dx * dx + dy * dy;
                if closed.get(y * w + x).copied().unwrap_or(1) == 0 && d < bd {
                    bd = d;
                    best = Some(y * w + x);
                }
            }
        }
        let Some(b) = best else { return Err("small") };
        start = b;
    }
    let mut region = vec![0u8; w * h];
    let mut stack: Vec<usize> = Vec::with_capacity(1024);
    let (mut n, mut edge) = (0usize, false);
    if let Some(v) = region.get_mut(start) {
        *v = 1;
    }
    stack.push(start);
    while let Some(i) = stack.pop() {
        n += 1;
        let x = i % w;
        let y = i / w;
        if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
            edge = true;
        }
        let visit = |j: usize, region: &mut Vec<u8>, stack: &mut Vec<usize>| {
            if region.get(j).copied().unwrap_or(1) == 0 && closed.get(j).copied().unwrap_or(1) == 0 {
                if let Some(v) = region.get_mut(j) {
                    *v = 1;
                }
                stack.push(j);
            }
        };
        if x > 0 {
            visit(i - 1, &mut region, &mut stack);
        }
        if x + 1 < w {
            visit(i + 1, &mut region, &mut stack);
        }
        if y > 0 {
            visit(i - w, &mut region, &mut stack);
        }
        if y + 1 < h {
            visit(i + w, &mut region, &mut stack);
        }
    }
    if edge || n as f64 > max_share * w as f64 * h as f64 {
        return Err("open");
    }
    let mut grown = dilate_bin(&region, w, h, r);
    for (g, wl) in grown.iter_mut().zip(&wall) {
        if *wl != 0 {
            *g = 0;
        }
    }
    let mut keep = vec![0u8; w * h];
    if let Some(v) = keep.get_mut(start) {
        *v = 1;
    }
    stack.clear();
    stack.push(start);
    let mut px = 0usize;
    while let Some(i) = stack.pop() {
        px += 1;
        let x = i % w;
        let y = i / w;
        let nb = [
            if x > 0 { Some(i - 1) } else { None },
            if x + 1 < w { Some(i + 1) } else { None },
            if y > 0 { Some(i - w) } else { None },
            if y + 1 < h { Some(i + w) } else { None },
        ];
        for j in nb.into_iter().flatten() {
            if grown.get(j).copied().unwrap_or(0) != 0 && keep.get(j).copied().unwrap_or(1) == 0 {
                if let Some(v) = keep.get_mut(j) {
                    *v = 1;
                }
                stack.push(j);
            }
        }
    }
    if (px as f64) < 4.0 * rf * rf {
        return Err("small");
    }
    let cf = cc_m3d::js::max(1.0, cc_m3d::js::round(rf / 2.0));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let c = cf as usize;
    let mut dil = dilate_bin(&keep, w, h, c);
    for v in &mut dil {
        *v = u8::from(*v == 0);
    }
    let outside = dilate_bin(&dil, w, h, c);
    for i in 0..keep.len() {
        let v = u8::from(outside.get(i).copied().unwrap_or(1) == 0 && wall.get(i).copied().unwrap_or(1) == 0);
        if let Some(o) = keep.get_mut(i) {
            *o = v;
        }
    }
    let ring = simplify(&trace(&keep, w, h), 1.0);
    if ring.len() < 3 {
        return Err("small");
    }
    Ok((ring, px))
}

/// The outer boundary of a mask, along pixel edges, keeping the region on the right. Corners only.
pub fn trace(mask: &[u8], w: usize, h: usize) -> Vec<P> {
    if !raster_ok(w, h) {
        return Vec::new();
    }
    let Some(s) = mask.iter().position(|v| *v != 0) else { return Vec::new() };
    let (wi, hi) = (i64::try_from(w).unwrap_or(i64::MAX), i64::try_from(h).unwrap_or(i64::MAX));
    let inside =
        |x: i64, y: i64| x >= 0 && y >= 0 && x < wi && y < hi && usize::try_from(y * wi + x).is_ok_and(|k| mask.get(k) == Some(&1));
    const DX: [i64; 4] = [1, 0, -1, 0];
    const DY: [i64; 4] = [0, 1, 0, -1];
    const AR: [[i64; 2]; 4] = [[0, 0], [-1, 0], [-1, -1], [0, -1]];
    const AL: [[i64; 2]; 4] = [[0, -1], [0, 0], [-1, 0], [-1, -1]];
    let x0 = i64::try_from(s % w).unwrap_or(0);
    let y0 = i64::try_from(s / w).unwrap_or(0);
    let (mut x, mut y, mut d) = (x0, y0, 0usize);
    let mut out: Vec<P> = Vec::new();
    let guard = 4u64.saturating_mul((w + h) as u64).saturating_mul(64).saturating_add(16);
    let mut gi = 0u64;
    while gi < guard {
        let ar = AR.get(d).copied().unwrap_or([0, 0]);
        let al = AL.get(d).copied().unwrap_or([0, 0]);
        let r_in = inside(x + ar[0], y + ar[1]);
        let l_in = inside(x + al[0], y + al[1]);
        let nd = if !r_in {
            (d + 1) % 4
        } else if l_in {
            (d + 3) % 4
        } else {
            d
        };
        if x == x0 && y == y0 && nd == 0 && !out.is_empty() {
            break;
        }
        if nd != d || out.is_empty() {
            out.push(P::new(x as f64, y as f64));
        }
        d = nd;
        x += DX.get(d).copied().unwrap_or(0);
        y += DY.get(d).copied().unwrap_or(0);
        gi += 1;
    }
    out
}

/// Douglas–Peucker on an open polyline (iterative: the kept points of the JS recursion, in order).
fn dp(pts: &[P], eps: f64) -> Vec<P> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut kept = vec![false; pts.len()];
    if let Some(f) = kept.first_mut() {
        *f = true;
    }
    if let Some(l) = kept.last_mut() {
        *l = true;
    }
    let mut stack = vec![(0usize, pts.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        if b - a + 1 < 3 {
            continue;
        }
        let (Some(p), Some(q)) = (pts.get(a), pts.get(b)) else { continue };
        let dx = q.x - p.x;
        let dy = q.y - p.y;
        let l0 = cc_m3d::js::hypot2(dx, dy);
        let l = if l0 == 0.0 || l0.is_nan() { 1.0 } else { l0 };
        let (mut k, mut m) = (usize::MAX, -1.0f64);
        for i in a + 1..b {
            let Some(pi) = pts.get(i) else { continue };
            let d = ((pi.x - p.x) * dy - (pi.y - p.y) * dx).abs() / l;
            if d > m {
                m = d;
                k = i;
            }
        }
        if m <= eps || k == usize::MAX {
            continue;
        }
        if let Some(v) = kept.get_mut(k) {
            *v = true;
        }
        stack.push((k, b));
        stack.push((a, k));
    }
    pts.iter().zip(&kept).filter(|(_, k)| **k).map(|(p, _)| *p).collect()
}

/// Douglas–Peucker on a closed ring.
pub fn simplify(ring: &[P], eps: f64) -> Vec<P> {
    if ring.len() < 4 {
        return ring.to_vec();
    }
    let Some(r0) = ring.first().copied() else { return Vec::new() };
    let (mut b, mut best) = (0usize, -1.0f64);
    for (i, p) in ring.iter().enumerate() {
        let d = (p.x - r0.x) * (p.x - r0.x) + (p.y - r0.y) * (p.y - r0.y);
        if d > best {
            best = d;
            b = i;
        }
    }
    let first = dp(ring.get(..=b).unwrap_or(&[]), eps);
    let mut tail: Vec<P> = ring.get(b..).unwrap_or(&[]).to_vec();
    tail.push(r0);
    let second = dp(&tail, eps);
    let mut out: Vec<P> = first.get(..first.len().saturating_sub(1)).unwrap_or(&[]).to_vec();
    out.extend_from_slice(second.get(..second.len().saturating_sub(1)).unwrap_or(&[]));
    out
}
