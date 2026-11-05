//! A VP8 encoder (RFC 6386). Key frames: every macroblock predicted by DC_PRED from the pixels already reconstructed
//! (Y 16 × 16, U and V 8 × 8). Inter frames (M6.40): every macroblock predicted from the last frame at a whole-pixel
//! motion vector of even length (a diamond search from the neighbours' and the last frame's vectors; chroma then moves
//! by whole pixels too, so no sub-pixel filter is needed), coded as ZEROMV, NEARESTMV, NEARMV or NEWMV against the
//! decoder's own census of the neighbours (find_near_mvs), and skipped when its residue quantizes to nothing. The
//! residue goes through the forward DCT (and the WHT of the sixteen luma DCs), quantized at one index, tokens coded
//! with the default probabilities, no loop filter. The encoder reconstructs each macroblock with the decoder's own
//! inverse transforms (RFC 6386 14.3, 14.4), so its references match the decoder's bit for bit.

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

/// A coefficient divided by its factor, rounded up from `f / round` (2: to the nearest; more: a dead zone round zero),
/// within the token range. Inter frames use a dead
/// zone: what is left of a good prediction is mostly the last frame's own rounding, not worth coding again.
fn quant_at(c: i32, f: i32, round: i32) -> i32 {
    let f = f.max(1);
    let a = (c.abs() + f / round.max(2)) / f;
    a.min(2048) * c.signum()
}

/// One plane, padded to whole macroblocks.
#[derive(Clone)]
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

// ------------------------------------------------------------------------------------------- inter frames (M6.40)

/// Inter frames round coefficients up from a third of their step (see [`quant_at`]).
const INTER_ROUND: i32 = 3;

/// RFC 6386 16.3: the mode probabilities from the neighbours' census (vp8_mode_contexts).
const MODE_CONTEXTS: [[u8; 4]; 6] =
    [[7, 1, 1, 143], [14, 18, 14, 107], [135, 64, 57, 68], [60, 56, 128, 65], [159, 134, 128, 34], [234, 188, 128, 28]];
/// RFC 6386 17.2: the motion vectors' default probabilities, row then column (is short, sign, short tree, long bits).
const MV_PROBS: [[u8; 19]; 2] = [
    [162, 128, 225, 146, 172, 147, 214, 39, 156, 128, 129, 132, 75, 145, 178, 206, 239, 254, 254],
    [164, 128, 204, 170, 119, 235, 140, 230, 228, 128, 130, 130, 74, 148, 180, 203, 236, 254, 254],
];
/// RFC 6386 17.2: the probabilities that a motion vector probability is updated (never, here).
const MV_UPDATE_PROBS: [[u8; 19]; 2] = [
    [237, 246, 253, 253, 254, 254, 254, 254, 254, 254, 254, 254, 254, 254, 250, 250, 252, 254, 254],
    [231, 243, 245, 253, 254, 254, 254, 254, 254, 254, 254, 254, 254, 254, 251, 251, 254, 254, 254],
];
/// small_mv_tree (RFC 6386 17.2).
const SMALL_MV_TREE: [i32; 14] = [2, 8, 4, 6, -0, -1, -2, -3, 10, 12, -4, -5, -6, -7];

impl BoolEnc {
    /// Write the leaf `v` of `tree` (dixie's bool_read_tree in reverse: node i's probability is probs[i >> 1]).
    fn tree(&mut self, tree: &[i32], probs: &[u8], v: i32) {
        // the path from the root to the leaf, found by a walk of the (small) tree
        fn path(tree: &[i32], at: usize, v: i32, out: &mut Vec<(usize, bool)>) -> bool {
            for bit in [false, true] {
                let Some(&n) = tree.get(at + usize::from(bit)) else { return false };
                out.push((at, bit));
                if n <= 0 {
                    if -n == v {
                        return true;
                    }
                } else if let Ok(next) = usize::try_from(n)
                    && path(tree, next, v, out)
                {
                    return true;
                }
                out.pop();
            }
            false
        }
        let mut p = Vec::new();
        if path(tree, 0, v, &mut p) {
            for (at, bit) in p {
                self.put(probs.get(at >> 1).copied().unwrap_or(128), bit);
            }
        }
    }
}

/// One motion vector component (a quarter-pel difference, |v| ≤ 1023) with its probabilities (dixie read_mv_component
/// in reverse).
fn write_mv_component(bc: &mut BoolEnc, v: i32, p: &[u8; 19]) {
    let x = v.abs().min(1023);
    let pr = |i: usize| p.get(i).copied().unwrap_or(128);
    if x < 8 {
        bc.put(pr(0), false);
        bc.tree(&SMALL_MV_TREE, p.get(2..9).unwrap_or(&[]), x);
    } else {
        bc.put(pr(0), true);
        for i in 0..3 {
            bc.put(pr(9 + i), (x >> i) & 1 == 1);
        }
        for i in (4..10).rev() {
            bc.put(pr(9 + i), (x >> i) & 1 == 1);
        }
        // bit 3 is implied (set) when no higher bit is
        if x & 0xFFF0 != 0 {
            bc.put(pr(9 + 3), (x >> 3) & 1 == 1);
        }
    }
    if x != 0 {
        bc.put(pr(1), v < 0);
    }
}

/// A motion vector as the decoder keeps it: eighth-pel units (the bitstream's quarter-pels doubled), x then y.
type Mv = [i32; 2];

/// dixie find_near_mvs for a frame whose macroblocks are all inter-coded from the last frame without split vectors:
/// `coded` holds the vectors of the macroblocks coded so far (raster order); outside the frame counts as intra.
/// → ([best, nearest, near], the census).
fn near_mvs(coded: &[Mv], mbw: usize, mx: usize, my: usize) -> ([Mv; 3], [usize; 4]) {
    let at = |x: Option<usize>, y: Option<usize>| -> Option<Mv> {
        let (x, y) = (x?, y?);
        if x >= mbw {
            return None;
        }
        coded.get(y.checked_mul(mbw)?.checked_add(x)?).copied()
    };
    let (left_x, up_y) = (mx.checked_sub(1), my.checked_sub(1));
    let mut mvs = [[0i32; 2]; 4];
    let mut cnt = [0usize; 4];
    let mut i = 0usize;
    for (m, weight) in [(at(Some(mx), up_y), 2usize), (at(left_x, Some(my)), 2), (at(left_x, up_y), 1)] {
        let Some(m) = m else { continue };
        if m != [0, 0] {
            if mvs.get(i) != Some(&m) {
                i = (i + 1).min(3);
                if let Some(s) = mvs.get_mut(i) {
                    *s = m;
                }
            }
            if let Some(c) = cnt.get_mut(i) {
                *c += weight;
            }
        } else if let Some(c) = cnt.get_mut(0) {
            *c += weight;
        }
    }
    // three distinct vectors: the above-left one may merge with the nearest
    if cnt[3] > 0 && mvs.get(i) == mvs.get(1) {
        cnt[1] += 1;
    }
    cnt[3] = 0; // no neighbour splits
    if cnt[2] > cnt[1] {
        cnt.swap(1, 2);
        mvs.swap(1, 2);
    }
    if cnt[1] >= cnt[0] {
        mvs[0] = mvs[1];
    }
    ([mvs[0], mvs[1], mvs[2]], cnt)
}

/// A frame's three planes, padded to whole macroblocks: the source, or the reconstruction the decoder will hold.
#[derive(Clone)]
struct Frame3 {
    y: Plane,
    u: Plane,
    v: Plane,
}

/// One macroblock's quantized coefficients in token order of use: Y2, the sixteen Y, four U, four V.
struct MbCoefs {
    y2: [i32; 16],
    y: [[i32; 16]; 16],
    u: [[i32; 16]; 4],
    v: [[i32; 16]; 4],
}

impl MbCoefs {
    fn all_zero(&self) -> bool {
        self.y2.iter().chain(self.y.iter().flatten()).chain(self.u.iter().flatten()).chain(self.v.iter().flatten()).all(|c| *c == 0)
    }
}

/// The luma of a 16 × 16 macroblock at (x0, y0) against its prediction `pred` (raster order): sixteen DCTs whose DCs
/// go through the WHT (Y2), quantized; the decoder's reconstruction written into `rec`. → (Y2, Y).
fn luma_mb(src: &Plane, rec: &mut Plane, x0: usize, y0: usize, pred: &[i32; 256], qf: &Quant, round: i32) -> ([i32; 16], [[i32; 16]; 16]) {
    let pr = |k: usize| pred.get(k).copied().unwrap_or(128);
    let mut coefs = [[0i32; 16]; 16];
    let mut dcs = [0i32; 16];
    for (b, c) in coefs.iter_mut().enumerate() {
        let (bx, by) = ((b % 4) * 4, (b / 4) * 4);
        let mut res = [0i32; 16];
        for (k, r) in res.iter_mut().enumerate() {
            let (px, py) = (bx + k % 4, by + k / 4);
            *r = src.at(x0 + px, y0 + py) - pr(py * 16 + px);
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
        let v = quant_at(get(&y2, k), f, round);
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
            let v = quant_at(get(c, k), qf.y1[1], round);
            if let (Some(a), Some(d)) = (qb.get_mut(k), dq.get_mut(k)) {
                *a = v;
                *d = s16(v * qf.y1[1]);
            }
        }
        let r = idct(&dq);
        let (bx, by) = ((b % 4) * 4, (b / 4) * 4);
        for (k, x) in r.iter().enumerate() {
            let (px, py) = (bx + k % 4, by + k / 4);
            rec.set(x0 + px, y0 + py, pr(py * 16 + px) + x);
        }
    }
    (y2q, yq)
}

/// An 8 × 8 chroma block at (x0, y0) against its prediction (raster order): four DCTs quantized at `f`, reconstructed
/// into `rec`.
fn chroma_mb(src: &Plane, rec: &mut Plane, x0: usize, y0: usize, pred: &[i32; 64], f: [i32; 2], round: i32) -> [[i32; 16]; 4] {
    let pr = |k: usize| pred.get(k).copied().unwrap_or(128);
    let mut out = [[0i32; 16]; 4];
    for (b, qb) in out.iter_mut().enumerate() {
        let (bx, by) = ((b % 2) * 4, (b / 2) * 4);
        let mut res = [0i32; 16];
        for (k, r) in res.iter_mut().enumerate() {
            let (px, py) = (bx + k % 4, by + k / 4);
            *r = src.at(x0 + px, y0 + py) - pr(py * 8 + px);
        }
        let c = fdct(&res);
        let mut dq = [0i32; 16];
        for k in 0..16 {
            let fk = if k == 0 { f[0] } else { f[1] };
            let v = quant_at(get(&c, k), fk, round);
            if let (Some(a), Some(d)) = (qb.get_mut(k), dq.get_mut(k)) {
                *a = v;
                *d = s16(v * fk);
            }
        }
        let r = idct(&dq);
        for (k, x) in r.iter().enumerate() {
            let (px, py) = (bx + k % 4, by + k / 4);
            rec.set(x0 + px, y0 + py, pr(py * 8 + px) + x);
        }
    }
    out
}

/// A macroblock's tokens with the neighbours' non-zero flags (above for its column, left for its row: Y 0–3, U 4–5,
/// V 6–7, Y2 8). → None when a context index is out of range (cannot happen).
fn write_mb_tokens(tk: &mut BoolEnc, a: &mut [bool; 9], left: &mut [bool; 9], c: &MbCoefs) -> Option<()> {
    let nz = write_block(tk, &c.y2, 0, 1, usize::from(a[8]) + usize::from(left[8]));
    a[8] = nz;
    left[8] = nz;
    for (b, qb) in c.y.iter().enumerate() {
        let (cx, r) = (b % 4, b / 4);
        let ctx = usize::from(*a.get(cx)?) + usize::from(*left.get(r)?);
        let nz = write_block(tk, qb, 1, 0, ctx);
        *a.get_mut(cx)? = nz;
        *left.get_mut(r)? = nz;
    }
    for (base, blocks) in [(4usize, &c.u), (6usize, &c.v)] {
        for (b, qb) in blocks.iter().enumerate() {
            let (cx, r) = (base + b % 2, base + b / 2);
            let ctx = usize::from(*a.get(cx)?) + usize::from(*left.get(r)?);
            let nz = write_block(tk, qb, 0, 2, ctx);
            *a.get_mut(cx)? = nz;
            *left.get_mut(r)? = nz;
        }
    }
    Some(())
}

/// The frame header fields both kinds share, from segmentation to the quantizer (RFC 6386 19.2).
fn header_common(hd: &mut BoolEnc, q: u8) {
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
}

/// The partitions after the frame tag: the first partition's size checked (19 bits).
fn assemble(tag_low: u32, extra: &[u8], first: Vec<u8>, tokens: Vec<u8>) -> Option<Vec<u8>> {
    let size = u32::try_from(first.len()).ok().filter(|s| *s < (1 << 19))?;
    let tag = (size << 5) | tag_low;
    let mut out = Vec::with_capacity(3 + extra.len() + first.len() + tokens.len());
    out.extend(tag.to_le_bytes().into_iter().take(3));
    out.extend_from_slice(extra);
    out.extend_from_slice(&first);
    out.extend_from_slice(&tokens);
    Some(out)
}

/// A key frame of the planes `src` (`w` × `h` shown, `mbw` × `mbh` macroblocks) → (the frame, its reconstruction).
fn encode_key(src: &Frame3, w: usize, h: usize, mbw: usize, mbh: usize, q: u8) -> Option<(Vec<u8>, Frame3)> {
    let mut rec = Frame3 {
        y: Plane { w: src.y.w, px: vec![0; src.y.px.len()] },
        u: Plane { w: src.u.w, px: vec![0; src.u.px.len()] },
        v: Plane { w: src.v.w, px: vec![0; src.v.px.len()] },
    };
    let qf = Quant::at(q);
    // the first partition: the frame header and every macroblock's modes
    let mut hd = BoolEnc::new();
    hd.lit(0, 1); // colour space
    hd.lit(0, 1); // clamping type
    header_common(&mut hd, q);
    hd.lit(1, 1); // refresh entropy probabilities
    for p in COEFF_UPDATE_PROBS.iter().flatten().flatten().flatten() {
        hd.put(*p, false);
    }
    hd.lit(0, 1); // mb_no_skip_coeff: every macroblock codes its tokens
    let mut tk = BoolEnc::new();
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
            let pred = [dc_pred(&rec.y, x0, y0, 16); 256];
            let (y2, y) = luma_mb(&src.y, &mut rec.y, x0, y0, &pred, &qf, 2);
            let (cx, cy) = (mx * 8, my * 8);
            let pu = [dc_pred(&rec.u, cx, cy, 8); 64];
            let u = chroma_mb(&src.u, &mut rec.u, cx, cy, &pu, qf.uv, 2);
            let pv = [dc_pred(&rec.v, cx, cy, 8); 64];
            let v = chroma_mb(&src.v, &mut rec.v, cx, cy, &pv, qf.uv, 2);
            write_mb_tokens(&mut tk, above.get_mut(mx)?, &mut left, &MbCoefs { y2, y, u, v })?;
        }
    }
    let mut start = vec![0x9d, 0x01, 0x2a];
    start.extend_from_slice(&u16::try_from(w).ok()?.to_le_bytes());
    start.extend_from_slice(&u16::try_from(h).ok()?.to_le_bytes());
    // frame tag: key frame (0), version 0, shown
    let out = assemble(1 << 4, &start, hd.finish(), tk.finish())?;
    Some((out, rec))
}

/// Sum of absolute differences of the 16 × 16 luma block at (x0, y0) in `src` and at (x0 + dx, y0 + dy) in `refp`,
/// stopping once past `stop`.
fn sad16(src: &Plane, refp: &Plane, x0: usize, y0: usize, dx: i32, dy: i32, stop: u32) -> u32 {
    let (rx, ry) = ((x0 as i64 + i64::from(dx)) as usize, (y0 as i64 + i64::from(dy)) as usize);
    let mut s = 0u32;
    for y in 0..16 {
        let (Some(a), Some(b)) = (
            src.px.get((y0 + y) * src.w + x0..(y0 + y) * src.w + x0 + 16),
            refp.px.get((ry + y) * refp.w + rx..(ry + y) * refp.w + rx + 16),
        ) else {
            return u32::MAX;
        };
        s += a.iter().zip(b).map(|(p, q)| u32::from(p.abs_diff(*q))).sum::<u32>();
        if s > stop {
            return s;
        }
    }
    s
}

/// An inter frame of `src` predicted from `refp` (the last frame's reconstruction) with whole-pixel motion vectors of
/// even length (so chroma moves by whole pixels too: no sub-pixel filter anywhere). `prev` = the last frame's vectors
/// (one per macroblock, eighth-pels), searched from. → (the frame, its reconstruction, its vectors).
fn encode_inter(src: &Frame3, refp: &Frame3, mbw: usize, mbh: usize, q: u8, prev: &[Mv]) -> Option<(Vec<u8>, Frame3, Vec<Mv>)> {
    let qf = Quant::at(q);
    let mut rec = refp.clone();
    let (pw, ph) = (mbw * 16, mbh * 16);
    // the search's price of a vector: about the bits of its difference from the predicted one, in SAD units
    let lambda = (qf.y1[1] / 4).max(1) as u32;
    struct Mb {
        mv: Mv,
        coefs: MbCoefs,
        skip: bool,
    }
    let mut mbs: Vec<Mb> = Vec::with_capacity(mbw * mbh);
    let mut coded: Vec<Mv> = Vec::with_capacity(mbw * mbh);
    for my in 0..mbh {
        for mx in 0..mbw {
            let (x0, y0) = (mx * 16, my * 16);
            let ([best, _, _], _) = near_mvs(&coded, mbw, mx, my);
            // displacement bounds: the whole block inside the frame, ±64 pixels
            let (lo_x, hi_x) = ((-(x0 as i32)).max(-64), ((pw - 16 - x0) as i32).min(64));
            let (lo_y, hi_y) = ((-(y0 as i32)).max(-64), ((ph - 16 - y0) as i32).min(64));
            let ok = |d: [i32; 2]| d[0] % 2 == 0 && d[1] % 2 == 0 && (lo_x..=hi_x).contains(&d[0]) && (lo_y..=hi_y).contains(&d[1]);
            let pred_px = [best[0] / 8, best[1] / 8];
            let price = |d: [i32; 2]| lambda * ((d[0] - pred_px[0]).unsigned_abs() + (d[1] - pred_px[1]).unsigned_abs()).min(4096) / 2;
            let cost = |d: [i32; 2], stop: u32| sad16(&src.y, &refp.y, x0, y0, d[0], d[1], stop).saturating_add(price(d));
            // starting points: no motion, the predicted vector, the neighbours', this block's last frame
            let mut cands: Vec<[i32; 2]> = vec![[0, 0], pred_px];
            if let Some(m) = coded.last().filter(|_| mx > 0) {
                cands.push([m[0] / 8, m[1] / 8]);
            }
            if let Some(m) = my.checked_sub(1).and_then(|py| coded.get(py * mbw + mx)) {
                cands.push([m[0] / 8, m[1] / 8]);
            }
            if let Some(m) = prev.get(my * mbw + mx) {
                cands.push([m[0] / 8, m[1] / 8]);
            }
            let mut at = [0i32, 0];
            let mut c_at = cost(at, u32::MAX);
            for c in cands {
                let c = [c[0] & !1, c[1] & !1];
                if ok(c) && c != at {
                    let v = cost(c, c_at);
                    if v < c_at {
                        (at, c_at) = (c, v);
                    }
                }
            }
            // a diamond search down to two-pixel steps
            for step in [8, 4, 2] {
                for _ in 0..16 {
                    let mut moved = false;
                    for d in [[step, 0], [-step, 0], [0, step], [0, -step]] {
                        let c = [at[0] + d[0], at[1] + d[1]];
                        if ok(c) {
                            let v = cost(c, c_at);
                            if v < c_at {
                                (at, c_at) = (c, v);
                                moved = true;
                            }
                        }
                    }
                    if !moved {
                        break;
                    }
                }
            }
            // the prediction: the reference's pixels at the vector (chroma at half of it)
            let mut py = [0i32; 256];
            for (k, p) in py.iter_mut().enumerate() {
                let (x, y) = (x0 as i32 + at[0] + (k % 16) as i32, y0 as i32 + at[1] + (k / 16) as i32);
                *p = refp.y.at(x.max(0) as usize, y.max(0) as usize);
            }
            let (cx0, cy0) = (mx * 8, my * 8);
            let chroma_pred = |p: &Plane| {
                let mut out = [0i32; 64];
                for (k, o) in out.iter_mut().enumerate() {
                    let (x, y) = (cx0 as i32 + at[0] / 2 + (k % 8) as i32, cy0 as i32 + at[1] / 2 + (k / 8) as i32);
                    *o = p.at(x.max(0) as usize, y.max(0) as usize);
                }
                out
            };
            let (pu, pv) = (chroma_pred(&refp.u), chroma_pred(&refp.v));
            let (y2, y) = luma_mb(&src.y, &mut rec.y, x0, y0, &py, &qf, INTER_ROUND);
            let u = chroma_mb(&src.u, &mut rec.u, cx0, cy0, &pu, qf.uv, INTER_ROUND);
            let v = chroma_mb(&src.v, &mut rec.v, cx0, cy0, &pv, qf.uv, INTER_ROUND);
            let coefs = MbCoefs { y2, y, u, v };
            let skip = coefs.all_zero();
            let mv = [at[0] * 8, at[1] * 8];
            coded.push(mv);
            mbs.push(Mb { mv, coefs, skip });
        }
    }
    // the first partition: the header, then each macroblock's skip flag, reference and vector
    let skips = mbs.iter().filter(|m| m.skip).count();
    let total = mbs.len().max(1);
    let prob_skip_false = u32::try_from(((total - skips) * 256 / total).clamp(1, 255)).unwrap_or(128);
    let mut hd = BoolEnc::new();
    header_common(&mut hd, q);
    hd.lit(0, 1); // refresh golden
    hd.lit(0, 1); // refresh alternate
    hd.lit(0, 2); // no copy to golden
    hd.lit(0, 2); // no copy to alternate
    hd.lit(0, 1); // golden sign bias
    hd.lit(0, 1); // alternate sign bias
    hd.lit(1, 1); // refresh entropy probabilities
    hd.lit(1, 1); // refresh last
    for p in COEFF_UPDATE_PROBS.iter().flatten().flatten().flatten() {
        hd.put(*p, false);
    }
    hd.lit(1, 1); // mb_no_skip_coeff: a skip flag per macroblock
    hd.lit(prob_skip_false, 8);
    hd.lit(1, 8); // prob_intra: every macroblock inter
    hd.lit(255, 8); // prob_last: every macroblock from the last frame
    hd.lit(128, 8); // prob_gf
    hd.lit(0, 1); // no intra 16 × 16 probability update
    hd.lit(0, 1); // no intra chroma probability update
    for p in MV_UPDATE_PROBS.iter().flatten() {
        hd.put(*p, false);
    }
    let ps = u8::try_from(prob_skip_false).unwrap_or(128);
    let mut tk = BoolEnc::new();
    let mut above = vec![[false; 9]; mbw];
    let mut done: Vec<Mv> = Vec::with_capacity(mbs.len());
    for my in 0..mbh {
        let mut left = [false; 9];
        for mx in 0..mbw {
            let m = mbs.get(my * mbw + mx)?;
            hd.put(ps, m.skip);
            hd.put(1, true); // inter
            hd.put(255, false); // the last frame
            let ([best, nearest, near], cnt) = near_mvs(&done, mbw, mx, my);
            let p: [u8; 4] =
                std::array::from_fn(|i| MODE_CONTEXTS.get(cnt.get(i).copied().unwrap_or(0)).and_then(|r| r.get(i)).copied().unwrap_or(128));
            // the decoder's bounds for nearest / near / best: one macroblock past the frame's edges (eighth-pels)
            let clamp = |v: Mv| -> Mv {
                let (l, r) = (-(((mx + 1) as i32) << 7), ((mbw - mx) as i32) << 7);
                let (t, b) = (-(((my + 1) as i32) << 7), ((mbh - my) as i32) << 7);
                [v[0].clamp(l, r), v[1].clamp(t, b)]
            };
            if m.mv == [0, 0] {
                hd.put(p[0], false); // ZEROMV
            } else if m.mv == clamp(nearest) {
                hd.put(p[0], true);
                hd.put(p[1], false); // NEARESTMV
            } else if m.mv == clamp(near) {
                hd.put(p[0], true);
                hd.put(p[1], true);
                hd.put(p[2], false); // NEARMV
            } else {
                hd.put(p[0], true);
                hd.put(p[1], true);
                hd.put(p[2], true);
                hd.put(p[3], false); // NEWMV: the difference from the best, in quarter-pels, row first
                let b = clamp(best);
                write_mv_component(&mut hd, (m.mv[1] - b[1]) / 2, &MV_PROBS[0]);
                write_mv_component(&mut hd, (m.mv[0] - b[0]) / 2, &MV_PROBS[1]);
            }
            done.push(m.mv);
            let a = above.get_mut(mx)?;
            if m.skip {
                *a = [false; 9];
                left = [false; 9];
            } else {
                write_mb_tokens(&mut tk, a, &mut left, &m.coefs)?;
            }
        }
    }
    // frame tag: an inter frame (1), version 0, shown
    let out = assemble(1 | (1 << 4), &[], hd.finish(), tk.finish())?;
    Some((out, rec, coded))
}

/// RGBA (rows top to bottom) as the encoder's planes.
fn planes(rgba: &[u8], w: usize, h: usize, mbw: usize, mbh: usize) -> Frame3 {
    let (y, u, v) = yuv(rgba, w, h, mbw, mbh);
    Frame3 { y, u, v }
}

/// One RGBA frame (`w` × `h`, rows top to bottom) as a VP8 key frame at quantizer index `q` (0 best … 127 smallest).
/// Sizes over 16383 pixels are refused (None).
pub fn key_frame(rgba: &[u8], w: u32, h: u32, q: u8) -> Option<Vec<u8>> {
    let mut e = Encoder::new(w, h, q, 1)?;
    e.frame(rgba).map(|(f, _)| f)
}

/// A film's coder: a key frame first and every `key_every` frames (where a player can start or seek), inter frames
/// predicted from the frame before in between.
pub struct Encoder {
    w: usize,
    h: usize,
    mbw: usize,
    mbh: usize,
    q: u8,
    key_every: usize,
    n: usize,
    last: Option<Frame3>,
    mvs: Vec<Mv>,
}

impl std::fmt::Debug for Encoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Encoder({} × {}, q {}, frame {})", self.w, self.h, self.q, self.n)
    }
}

impl Encoder {
    /// A coder of `w` × `h` frames (1 … 16383 pixels) at quantizer index `q`.
    pub fn new(w: u32, h: u32, q: u8, key_every: usize) -> Option<Encoder> {
        if w == 0 || h == 0 || w > 16383 || h > 16383 {
            return None;
        }
        let (w, h) = (w as usize, h as usize);
        Some(Encoder {
            w,
            h,
            mbw: w.div_ceil(16),
            mbh: h.div_ceil(16),
            q: q.min(127),
            key_every: key_every.max(1),
            n: 0,
            last: None,
            mvs: Vec::new(),
        })
    }
    /// The next frame (RGBA, rows top to bottom, `w` × `h`) → (its VP8 data, whether it is a key frame).
    pub fn frame(&mut self, rgba: &[u8]) -> Option<(Vec<u8>, bool)> {
        if rgba.len() < self.w * self.h * 4 {
            return None;
        }
        let src = planes(rgba, self.w, self.h, self.mbw, self.mbh);
        let key = self.n.is_multiple_of(self.key_every) || self.last.is_none();
        let out = match (&self.last, key) {
            (Some(refp), false) => {
                let (f, rec, mvs) = encode_inter(&src, refp, self.mbw, self.mbh, self.q, &self.mvs)?;
                self.last = Some(rec);
                self.mvs = mvs;
                (f, false)
            }
            _ => {
                let (f, rec) = encode_key(&src, self.w, self.h, self.mbw, self.mbh, self.q)?;
                self.last = Some(rec);
                self.mvs.clear();
                (f, true)
            }
        };
        self.n += 1;
        Some(out)
    }
    /// The last frame's reconstruction, Y then U then V planes cropped to the frame (what a decoder shows; tests).
    pub fn last_i420(&self) -> Option<Vec<u8>> {
        let f = self.last.as_ref()?;
        let mut out = Vec::with_capacity(self.w * self.h * 3 / 2);
        for (p, w, h) in
            [(&f.y, self.w, self.h), (&f.u, self.w.div_ceil(2), self.h.div_ceil(2)), (&f.v, self.w.div_ceil(2), self.h.div_ceil(2))]
        {
            for y in 0..h {
                out.extend_from_slice(p.px.get(y * p.w..y * p.w + w)?);
            }
        }
        Some(out)
    }
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
