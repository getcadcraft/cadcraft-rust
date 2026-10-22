//! A VP8 encoder of key frames only (RFC 6386): every macroblock predicted by DC_PRED from the pixels already
//! reconstructed (Y 16 × 16, U and V 8 × 8), its residue through the forward DCT (and the WHT of the sixteen luma DCs),
//! quantized at one index, tokens coded with the default probabilities, no loop filter. The encoder reconstructs each
//! macroblock with the decoder's own inverse transforms (RFC 6386 14.3, 14.4), so its predictions match the decoder's
//! bit for bit. Simple and robust rather than small: a key frame each frame.

use crate::tables::{AC_Q, COEFF_UPDATE_PROBS, DC_Q, DEFAULT_COEFF_PROBS};

/// The boolean entropy encoder (RFC 6386 7.3).
struct BoolEnc {
    out: Vec<u8>,
    range: u32,
    bottom: u32,
    bit_count: i32,
}

impl BoolEnc {
    fn new() -> BoolEnc {
        BoolEnc { out: Vec::new(), range: 255, bottom: 0, bit_count: 24 }
    }
    fn add_one(&mut self) {
        for b in self.out.iter_mut().rev() {
            if *b == 255 {
                *b = 0;
            } else {
                *b += 1;
                return;
            }
        }
    }
    fn put(&mut self, prob: u8, bit: bool) {
        let split = 1 + (((self.range - 1) * u32::from(prob)) >> 8);
        if bit {
            self.bottom = self.bottom.wrapping_add(split);
            self.range -= split;
        } else {
            self.range = split;
        }
        while self.range < 128 {
            self.range <<= 1;
            if self.bottom & (1 << 31) != 0 {
                self.add_one();
            }
            self.bottom <<= 1;
            self.bit_count -= 1;
            if self.bit_count == 0 {
                self.out.push((self.bottom >> 24) as u8);
                self.bottom &= (1 << 24) - 1;
                self.bit_count = 8;
            }
        }
    }
    /// L(n): an unsigned n-bit literal, most significant bit first, each at probability 128.
    fn lit(&mut self, v: u32, n: u32) {
        for k in (0..n).rev() {
            self.put(128, (v >> k) & 1 == 1);
        }
    }
    fn finish(mut self) -> Vec<u8> {
        let mut c = self.bit_count;
        let mut v = self.bottom;
        if c < 32 && v & (1u32 << (32 - c)) != 0 {
            self.add_one();
        }
        v = v.checked_shl((c & 7) as u32).unwrap_or(0);
        c >>= 3;
        while c > 0 {
            v = v.checked_shl(8).unwrap_or(0);
            c -= 1;
        }
        for _ in 0..4 {
            self.out.push((v >> 24) as u8);
            v <<= 8;
        }
        self.out
    }
}

const ZIGZAG: [usize; 16] = [0, 1, 4, 8, 5, 2, 3, 6, 9, 12, 13, 10, 7, 11, 14, 15];
const BANDS: [usize; 17] = [0, 1, 2, 3, 6, 4, 5, 6, 6, 6, 6, 6, 6, 6, 6, 7, 0];
const PCAT1: [u8; 1] = [159];
const PCAT2: [u8; 2] = [165, 145];
const PCAT3: [u8; 3] = [173, 148, 140];
const PCAT4: [u8; 4] = [176, 155, 140, 135];
const PCAT5: [u8; 5] = [180, 157, 141, 134, 130];
const PCAT6: [u8; 11] = [254, 254, 243, 230, 196, 177, 153, 140, 133, 130, 129];

/// The six dequantization factors at a quantizer index (dixie.c): Y DC / AC, Y2 DC / AC, chroma DC / AC.
#[derive(Clone, Copy, Debug)]
struct Quant {
    y1: [i32; 2],
    y2: [i32; 2],
    uv: [i32; 2],
}

impl Quant {
    fn at(q: u8) -> Quant {
        let i = usize::from(q.min(127));
        let dc = i32::from(DC_Q.get(i).copied().unwrap_or(4));
        let ac = i32::from(AC_Q.get(i).copied().unwrap_or(4));
        Quant { y1: [dc, ac], y2: [dc * 2, (ac * 155 / 100).max(8)], uv: [dc.min(132), ac] }
    }
}

fn get(b: &[i32; 16], i: usize) -> i32 {
    b.get(i).copied().unwrap_or(0)
}

/// libvpx's forward 4 × 4 DCT (vp8_short_fdct4x4_c) of a residue block in raster order.
fn fdct(input: &[i32; 16]) -> [i32; 16] {
    let mut t = [0i32; 16];
    for (row, o) in input.as_chunks::<4>().0.iter().zip(t.as_chunks_mut::<4>().0.iter_mut()) {
        {
            let [i0, i1, i2, i3] = row;
            let [o0, o1, o2, o3] = o;
            let a1 = (i0 + i3) * 8;
            let b1 = (i1 + i2) * 8;
            let c1 = (i1 - i2) * 8;
            let d1 = (i0 - i3) * 8;
            *o0 = a1 + b1;
            *o2 = a1 - b1;
            *o1 = (c1 * 2217 + d1 * 5352 + 14500) >> 12;
            *o3 = (d1 * 2217 - c1 * 5352 + 7500) >> 12;
        }
    }
    let mut out = [0i32; 16];
    for c in 0..4 {
        let (i0, i4, i8, i12) = (get(&t, c), get(&t, c + 4), get(&t, c + 8), get(&t, c + 12));
        let a1 = i0 + i12;
        let b1 = i4 + i8;
        let c1 = i4 - i8;
        let d1 = i0 - i12;
        let v = [
            (a1 + b1 + 7) >> 4,
            ((c1 * 2217 + d1 * 5352 + 12000) >> 16) + i32::from(d1 != 0),
            (a1 - b1 + 7) >> 4,
            (d1 * 2217 - c1 * 5352 + 51000) >> 16,
        ];
        for (k, x) in v.into_iter().enumerate() {
            if let Some(s) = out.get_mut(c + 4 * k) {
                *s = x;
            }
        }
    }
    out
}

/// libvpx's forward WHT (vp8_short_walsh4x4_c) of the sixteen luma DCs.
fn fwht(input: &[i32; 16]) -> [i32; 16] {
    let mut t = [0i32; 16];
    for (row, o) in input.as_chunks::<4>().0.iter().zip(t.as_chunks_mut::<4>().0.iter_mut()) {
        {
            let [i0, i1, i2, i3] = row;
            let [o0, o1, o2, o3] = o;
            let a1 = (i0 + i2) * 4;
            let d1 = (i1 + i3) * 4;
            let c1 = (i1 - i3) * 4;
            let b1 = (i0 - i2) * 4;
            *o0 = a1 + d1 + i32::from(a1 != 0);
            *o1 = b1 + c1;
            *o2 = b1 - c1;
            *o3 = a1 - d1;
        }
    }
    let mut out = [0i32; 16];
    for c in 0..4 {
        let (i0, i4, i8, i12) = (get(&t, c), get(&t, c + 4), get(&t, c + 8), get(&t, c + 12));
        let a1 = i0 + i8;
        let d1 = i4 + i12;
        let c1 = i4 - i12;
        let b1 = i0 - i8;
        let mut v = [a1 + d1, b1 + c1, b1 - c1, a1 - d1];
        for x in &mut v {
            *x += i32::from(*x < 0);
            *x = (*x + 3) >> 3;
        }
        for (k, x) in v.into_iter().enumerate() {
            if let Some(s) = out.get_mut(c + 4 * k) {
                *s = x;
            }
        }
    }
    out
}

/// i16 as the decoder stores its intermediate values.
fn s16(x: i32) -> i32 {
    i32::from(x as i16)
}

/// The decoder's inverse WHT (RFC 6386 14.3).
fn iwht(input: &[i32; 16]) -> [i32; 16] {
    let mut t = [0i32; 16];
    for c in 0..4 {
        let (i0, i4, i8, i12) = (get(input, c), get(input, c + 4), get(input, c + 8), get(input, c + 12));
        let a1 = i0 + i12;
        let b1 = i4 + i8;
        let c1 = i4 - i8;
        let d1 = i0 - i12;
        for (k, x) in [a1 + b1, c1 + d1, a1 - b1, d1 - c1].into_iter().enumerate() {
            if let Some(s) = t.get_mut(c + 4 * k) {
                *s = s16(x);
            }
        }
    }
    let mut out = [0i32; 16];
    for (row, o) in t.as_chunks::<4>().0.iter().zip(out.as_chunks_mut::<4>().0.iter_mut()) {
        {
            let [i0, i1, i2, i3] = row;
            let [o0, o1, o2, o3] = o;
            let a1 = i0 + i3;
            let b1 = i1 + i2;
            let c1 = i1 - i2;
            let d1 = i0 - i3;
            *o0 = s16((a1 + b1 + 3) >> 3);
            *o1 = s16((c1 + d1 + 3) >> 3);
            *o2 = s16((a1 - b1 + 3) >> 3);
            *o3 = s16((d1 - c1 + 3) >> 3);
        }
    }
    out
}

/// The decoder's inverse DCT (RFC 6386 14.4).
fn idct(input: &[i32; 16]) -> [i32; 16] {
    const C1: i32 = 20091; // cos(pi/8) * sqrt(2) - 1, 16.16
    const S1: i32 = 35468; // sin(pi/8) * sqrt(2), 16.16
    let mut t = [0i32; 16];
    for c in 0..4 {
        let (i0, i4, i8, i12) = (get(input, c), get(input, c + 4), get(input, c + 8), get(input, c + 12));
        let a1 = i0 + i8;
        let b1 = i0 - i8;
        let c1 = ((i4 * S1) >> 16) - (i12 + ((i12 * C1) >> 16));
        let d1 = (i4 + ((i4 * C1) >> 16)) + ((i12 * S1) >> 16);
        for (k, x) in [a1 + d1, b1 + c1, b1 - c1, a1 - d1].into_iter().enumerate() {
            if let Some(s) = t.get_mut(c + 4 * k) {
                *s = s16(x);
            }
        }
    }
    let mut out = [0i32; 16];
    for (row, o) in t.as_chunks::<4>().0.iter().zip(out.as_chunks_mut::<4>().0.iter_mut()) {
        {
            let [i0, i1, i2, i3] = row;
            let [o0, o1, o2, o3] = o;
            let a1 = i0 + i2;
            let b1 = i0 - i2;
            let c1 = ((i1 * S1) >> 16) - (i3 + ((i3 * C1) >> 16));
            let d1 = (i1 + ((i1 * C1) >> 16)) + ((i3 * S1) >> 16);
            *o0 = s16((a1 + d1 + 4) >> 3);
            *o3 = s16((a1 - d1 + 4) >> 3);
            *o1 = s16((b1 + c1 + 4) >> 3);
            *o2 = s16((b1 - c1 + 4) >> 3);
        }
    }
    out
}

/// A coefficient divided by its factor, rounded to the nearest, within the token range.
fn quant(c: i32, f: i32) -> i32 {
    let f = f.max(1);
    let a = (c.abs() + f / 2) / f;
    a.min(2048) * c.signum()
}

/// One plane, padded to whole macroblocks.
struct Plane {
    w: usize,
    px: Vec<u8>,
}

impl Plane {
    fn at(&self, x: usize, y: usize) -> i32 {
        i32::from(self.px.get(y * self.w + x).copied().unwrap_or(0))
    }
    fn set(&mut self, x: usize, y: usize, v: i32) {
        if let Some(p) = self.px.get_mut(y * self.w + x) {
            *p = v.clamp(0, 255) as u8;
        }
    }
}

/// The token writer for one block: coefficients in raster order, the first position coded, the plane type and the
/// neighbours' context. → whether the block has a non-zero coefficient.
fn write_block(bc: &mut BoolEnc, q: &[i32; 16], first: usize, plane: usize, ctx: usize) -> bool {
    let Some(probs) = DEFAULT_COEFF_PROBS.get(plane) else { return false };
    let at = |i: usize| ZIGZAG.get(i).map_or(0, |z| get(q, *z));
    let last = (first..16).rev().find(|i| at(*i) != 0);
    let p_of = |i: usize, c: usize| -> [u8; 11] {
        probs.get(BANDS.get(i).copied().unwrap_or(0)).and_then(|b| b.get(c)).copied().unwrap_or([128; 11])
    };
    let Some(last) = last else {
        let p = p_of(first, ctx);
        bc.put(p[0], false);
        return false;
    };
    let mut ctx = ctx;
    let mut prev_zero = false;
    for i in first..=last {
        let p = p_of(i, ctx);
        let v = at(i);
        let a = v.abs();
        if !prev_zero {
            bc.put(p[0], true);
        }
        if a == 0 {
            bc.put(p[1], false);
            prev_zero = true;
            ctx = 0;
            continue;
        }
        bc.put(p[1], true);
        if a == 1 {
            bc.put(p[2], false);
        } else {
            bc.put(p[2], true);
            if a <= 4 {
                bc.put(p[3], false);
                if a == 2 {
                    bc.put(p[4], false);
                } else {
                    bc.put(p[4], true);
                    bc.put(p[5], a == 4);
                }
            } else {
                bc.put(p[3], true);
                let extra = |bc: &mut BoolEnc, v: i32, pc: &[u8]| {
                    let n = pc.len();
                    for (k, pr) in pc.iter().enumerate() {
                        bc.put(*pr, (v >> (n - 1 - k)) & 1 == 1);
                    }
                };
                if a <= 10 {
                    bc.put(p[6], false);
                    if a <= 6 {
                        bc.put(p[7], false);
                        extra(bc, a - 5, &PCAT1);
                    } else {
                        bc.put(p[7], true);
                        extra(bc, a - 7, &PCAT2);
                    }
                } else {
                    bc.put(p[6], true);
                    if a <= 34 {
                        bc.put(p[8], false);
                        if a <= 18 {
                            bc.put(p[9], false);
                            extra(bc, a - 11, &PCAT3);
                        } else {
                            bc.put(p[9], true);
                            extra(bc, a - 19, &PCAT4);
                        }
                    } else {
                        bc.put(p[8], true);
                        if a <= 66 {
                            bc.put(p[10], false);
                            extra(bc, a - 35, &PCAT5);
                        } else {
                            bc.put(p[10], true);
                            extra(bc, a - 67, &PCAT6);
                        }
                    }
                }
            }
        }
        bc.put(128, v < 0);
        prev_zero = false;
        ctx = if a == 1 { 1 } else { 2 };
    }
    if last < 15 {
        let p = p_of(last + 1, ctx);
        bc.put(p[0], false);
    }
    true
}

/// DC_PRED of an n × n block at (x0, y0) from the reconstructed plane (128 at the frame's top left corner).
fn dc_pred(p: &Plane, x0: usize, y0: usize, n: usize) -> i32 {
    let (up, left) = (y0 > 0, x0 > 0);
    let mut sum = 0;
    if up {
        sum += (0..n).map(|i| p.at(x0 + i, y0 - 1)).sum::<i32>();
    }
    if left {
        sum += (0..n).map(|i| p.at(x0 - 1, y0 + i)).sum::<i32>();
    }
    let shift = if n == 16 { 3 } else { 2 } + i32::from(up) + i32::from(left);
    if !up && !left { 128 } else { (sum + (1 << (shift - 1))) >> shift }
}

/// Code one n × n chroma (or the luma) block's 4 × 4 pieces without Y2: → (quantized blocks, reconstructed into `rec`).
fn chroma_blocks(src: &Plane, rec: &mut Plane, x0: usize, y0: usize, f: [i32; 2]) -> [[i32; 16]; 4] {
    let pred = dc_pred(rec, x0, y0, 8);
    let mut out = [[0i32; 16]; 4];
    for (b, qb) in out.iter_mut().enumerate() {
        let (bx, by) = (x0 + (b % 2) * 4, y0 + (b / 2) * 4);
        let mut res = [0i32; 16];
        for (k, r) in res.iter_mut().enumerate() {
            *r = src.at(bx + k % 4, by + k / 4) - pred;
        }
        let c = fdct(&res);
        let mut dq = [0i32; 16];
        for k in 0..16 {
            let fk = if k == 0 { f[0] } else { f[1] };
            let v = quant(get(&c, k), fk);
            if let (Some(a), Some(d)) = (qb.get_mut(k), dq.get_mut(k)) {
                *a = v;
                *d = s16(v * fk);
            }
        }
        let r = idct(&dq);
        for (k, x) in r.iter().enumerate() {
            rec.set(bx + k % 4, by + k / 4, pred + x);
        }
    }
    out
}

/// RGBA (rows top to bottom) → Y, U, V planes padded to whole macroblocks (BT.601, studio range, 4:2:0).
fn yuv(rgba: &[u8], w: usize, h: usize, mbw: usize, mbh: usize) -> (Plane, Plane, Plane) {
    let (pw, ph) = (mbw * 16, mbh * 16);
    let px = |x: usize, y: usize| -> [i32; 3] {
        let (x, y) = (x.min(w.saturating_sub(1)), y.min(h.saturating_sub(1)));
        let o = (y * w + x) * 4;
        match rgba.get(o..o + 3) {
            Some([r, g, b]) => [i32::from(*r), i32::from(*g), i32::from(*b)],
            _ => [0; 3],
        }
    };
    let mut yp = Plane { w: pw, px: vec![0; pw * ph] };
    for y in 0..ph {
        for x in 0..pw {
            let [r, g, b] = px(x, y);
            yp.set(x, y, ((66 * r + 129 * g + 25 * b + 128) >> 8) + 16);
        }
    }
    let (cw, ch) = (pw / 2, ph / 2);
    let mut up = Plane { w: cw, px: vec![0; cw * ch] };
    let mut vp = Plane { w: cw, px: vec![0; cw * ch] };
    for y in 0..ch {
        for x in 0..cw {
            let mut s = [0i32; 3];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let c = px(2 * x + dx, 2 * y + dy);
                for k in 0..3 {
                    if let (Some(a), Some(b)) = (s.get_mut(k), c.get(k)) {
                        *a += b;
                    }
                }
            }
            let [r, g, b] = s.map(|v| (v + 2) / 4);
            up.set(x, y, ((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128);
            vp.set(x, y, ((112 * r - 94 * g - 18 * b + 128) >> 8) + 128);
        }
    }
    (yp, up, vp)
}

/// One RGBA frame (`w` × `h`, rows top to bottom) as a VP8 key frame at quantizer index `q` (0 best … 127 smallest).
/// Sizes over 16383 pixels are refused (None).
pub fn key_frame(rgba: &[u8], w: u32, h: u32, q: u8) -> Option<Vec<u8>> {
    if w == 0 || h == 0 || w > 16383 || h > 16383 || rgba.len() < (w as usize) * (h as usize) * 4 {
        return None;
    }
    let (w, h) = (w as usize, h as usize);
    let (mbw, mbh) = (w.div_ceil(16), h.div_ceil(16));
    let (sy, su, sv) = yuv(rgba, w, h, mbw, mbh);
    let mut ry = Plane { w: sy.w, px: vec![0; sy.px.len()] };
    let mut ru = Plane { w: su.w, px: vec![0; su.px.len()] };
    let mut rv = Plane { w: sv.w, px: vec![0; sv.px.len()] };
    let qf = Quant::at(q);
    // the first partition: the frame header and every macroblock's modes
    let mut hd = BoolEnc::new();
    hd.lit(0, 1); // colour space
    hd.lit(0, 1); // clamping type
    hd.lit(0, 1); // segmentation
    hd.lit(0, 1); // filter type
    hd.lit(0, 6); // loop filter level: none
    hd.lit(0, 3); // sharpness
    hd.lit(0, 1); // loop filter adjustments
    hd.lit(0, 2); // one token partition
    hd.lit(u32::from(q.min(127)), 7);
    for _ in 0..5 {
        hd.lit(0, 1); // no quantizer deltas
    }
    hd.lit(1, 1); // refresh entropy probabilities
    for p in COEFF_UPDATE_PROBS.iter().flatten().flatten().flatten() {
        hd.put(*p, false);
    }
    hd.lit(0, 1); // mb_no_skip_coeff: every macroblock codes its tokens
    // the token partition
    let mut tk = BoolEnc::new();
    // contexts: per macroblock column (above) and for the row (left): Y 4, U 2, V 2, Y2 1
    let mut above = vec![[false; 9]; mbw];
    for my in 0..mbh {
        let mut left = [false; 9];
        for mx in 0..mbw {
            // modes: Y DC_PRED (kf_ymode_tree "100"), chroma DC_PRED ("0")
            hd.put(145, true);
            hd.put(156, false);
            hd.put(163, false);
            hd.put(142, false);
            let (x0, y0) = (mx * 16, my * 16);
            // luma: 16 DCTs, their DCs through the WHT (Y2)
            let pred = dc_pred(&ry, x0, y0, 16);
            let mut coefs = [[0i32; 16]; 16];
            let mut dcs = [0i32; 16];
            for (b, c) in coefs.iter_mut().enumerate() {
                let (bx, by) = (x0 + (b % 4) * 4, y0 + (b / 4) * 4);
                let mut res = [0i32; 16];
                for (k, r) in res.iter_mut().enumerate() {
                    *r = sy.at(bx + k % 4, by + k / 4) - pred;
                }
                *c = fdct(&res);
                if let Some(d) = dcs.get_mut(b) {
                    *d = c[0];
                }
            }
            let y2 = fwht(&dcs);
            let mut y2q = [0i32; 16];
            let mut y2d = [0i32; 16];
            for k in 0..16 {
                let f = if k == 0 { qf.y2[0] } else { qf.y2[1] };
                let v = quant(get(&y2, k), f);
                if let (Some(a), Some(d)) = (y2q.get_mut(k), y2d.get_mut(k)) {
                    *a = v;
                    *d = s16(v * f);
                }
            }
            let dc_rec = iwht(&y2d);
            let mut yq = [[0i32; 16]; 16];
            for (b, (c, qb)) in coefs.iter().zip(yq.iter_mut()).enumerate() {
                let mut dq = [0i32; 16];
                if let Some(d) = dq.get_mut(0) {
                    *d = get(&dc_rec, b);
                }
                for k in 1..16 {
                    let v = quant(get(c, k), qf.y1[1]);
                    if let (Some(a), Some(d)) = (qb.get_mut(k), dq.get_mut(k)) {
                        *a = v;
                        *d = s16(v * qf.y1[1]);
                    }
                }
                let r = idct(&dq);
                let (bx, by) = (x0 + (b % 4) * 4, y0 + (b / 4) * 4);
                for (k, x) in r.iter().enumerate() {
                    ry.set(bx + k % 4, by + k / 4, pred + x);
                }
            }
            let uq = chroma_blocks(&su, &mut ru, mx * 8, my * 8, qf.uv);
            let vq = chroma_blocks(&sv, &mut rv, mx * 8, my * 8, qf.uv);
            // tokens: Y2 (type 1), Y after Y2 (type 0), U and V (type 2)
            let a = above.get_mut(mx)?;
            let nz = write_block(&mut tk, &y2q, 0, 1, usize::from(a[8]) + usize::from(left[8]));
            a[8] = nz;
            left[8] = nz;
            for (b, qb) in yq.iter().enumerate() {
                let (c, r) = (b % 4, b / 4);
                let ctx = usize::from(*a.get(c)?) + usize::from(*left.get(r)?);
                let nz = write_block(&mut tk, qb, 1, 0, ctx);
                *a.get_mut(c)? = nz;
                *left.get_mut(r)? = nz;
            }
            for (base, blocks) in [(4usize, &uq), (6usize, &vq)] {
                for (b, qb) in blocks.iter().enumerate() {
                    let (c, r) = (base + b % 2, base + b / 2);
                    let ctx = usize::from(*a.get(c)?) + usize::from(*left.get(r)?);
                    let nz = write_block(&mut tk, qb, 0, 2, ctx);
                    *a.get_mut(c)? = nz;
                    *left.get_mut(r)? = nz;
                }
            }
        }
    }
    let first = hd.finish();
    let tokens = tk.finish();
    let size = u32::try_from(first.len()).ok().filter(|s| *s < (1 << 19))?;
    // frame tag: key frame (0), version 0, shown, the first partition's size
    let tag = (size << 5) | (1 << 4);
    let mut out = Vec::with_capacity(10 + first.len() + tokens.len());
    out.extend(tag.to_le_bytes().into_iter().take(3));
    out.extend_from_slice(&[0x9d, 0x01, 0x2a]);
    out.extend_from_slice(&(w as u16).to_le_bytes());
    out.extend_from_slice(&(h as u16).to_le_bytes());
    out.extend_from_slice(&first);
    out.extend_from_slice(&tokens);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_transforms_round_trip() {
        // a residue through the forward DCT and the decoder's inverse comes back within rounding
        let mut res = [0i32; 16];
        for (k, r) in res.iter_mut().enumerate() {
            *r = ((k as i32) * 7 % 23) - 11;
        }
        let back = idct(&fdct(&res));
        assert!(res.iter().zip(back.iter()).all(|(a, b)| (a - b).abs() <= 1), "{res:?} {back:?}");
        let dcs: [i32; 16] = std::array::from_fn(|k| (k as i32) * 40 - 300);
        let back = iwht(&fwht(&dcs));
        assert!(dcs.iter().zip(back.iter()).all(|(a, b)| (a - b).abs() <= 1), "{dcs:?} {back:?}");
    }

    #[test]
    fn a_frame_has_the_key_frame_header() {
        let rgba: Vec<u8> = (0..40 * 30).flat_map(|i| [(i % 255) as u8, 80, 200, 255]).collect();
        let f = key_frame(&rgba, 40, 30, 20).unwrap();
        assert_eq!(f[0] & 1, 0, "a key frame");
        assert_eq!(&f[3..6], &[0x9d, 0x01, 0x2a]);
        assert_eq!(u16::from_le_bytes([f[6], f[7]]), 40);
        assert_eq!(u16::from_le_bytes([f[8], f[9]]), 30);
        assert!(key_frame(&rgba, 0, 30, 20).is_none());
        assert!(key_frame(&rgba[..10], 40, 30, 20).is_none());
    }
}
