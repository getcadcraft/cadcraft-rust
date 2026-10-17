//! compare.js — sheet compare: mix two renderings of the same view into one picture, and the alignment transform.

/// Colors of the mix, as [r, g, b] 0–255.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MixColors {
    pub bg: [f64; 3],
    pub same: [f64; 3],
    pub old: [f64; 3],
    pub new: [f64; 3],
}

/// Pixel counts of a mix.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MixStats {
    pub changed_old: u64,
    pub changed_new: u64,
    pub same: u64,
}

/// `Uint8ClampedArray` store: round half to even, clamp to 0–255 (NaN → 0).
pub fn clamp_u8(v: f64) -> u8 {
    if v.is_nan() || v <= 0.0 {
        return 0;
    }
    if v >= 255.0 {
        return 255;
    }
    let f = v.floor();
    let d = v - f;
    let r = if d > 0.5 || (d == 0.5 && f % 2.0 != 0.0) { f + 1.0 } else { f };
    // 0 ≤ r ≤ 255 here
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let b = r as u8;
    b
}

/// `combine(a, b, out, colors, balance)`: a, b are the old and new RGBA renderings; out gets the mix.
pub fn combine(a: &[u8], b: &[u8], out: &mut [u8], colors: &MixColors, balance: f64) -> MixStats {
    let wa = crate::js::min(1.0, 2.0 * (1.0 - balance));
    let wb = crate::js::min(1.0, 2.0 * balance);
    let [br, bg, bb] = colors.bg;
    let [sr, sg, sb] = colors.same;
    let [or, og, ob] = colors.old;
    let [nr, ng, nb] = colors.new;
    let mut st = MixStats::default();
    let px = |s: &[u8], i: usize| -> [f64; 4] {
        // reads past the end are `undefined` → NaN in the JS
        let g = |k: usize| s.get(i.saturating_add(k)).map_or(f64::NAN, |v| f64::from(*v));
        [g(0), g(1), g(2), g(3)]
    };
    let n = out.len();
    let mut i = 0usize;
    while i < n {
        let [a0, a1, a2, a3] = px(a, i);
        let [b0, b1, b2, b3] = px(b, i);
        let da = (1.0 - (a0 * 0.299 + a1 * 0.587 + a2 * 0.114) / 255.0) * (a3 / 255.0);
        let db = (1.0 - (b0 * 0.299 + b1 * 0.587 + b2 * 0.114) / 255.0) * (b3 / 255.0);
        let (aa, bv) = (da * wa, db * wb);
        let common = crate::js::min(aa, bv);
        let only_a = crate::js::max(0.0, aa - common - 0.06) / 0.94;
        let only_b = crate::js::max(0.0, bv - common - 0.06) / 0.94;
        let (mut r, mut g, mut bl) = (br, bg, bb);
        r += (sr - r) * common;
        g += (sg - g) * common;
        bl += (sb - bl) * common;
        r += (or - r) * only_a;
        g += (og - g) * only_a;
        bl += (ob - bl) * only_a;
        r += (nr - r) * only_b;
        g += (ng - g) * only_b;
        bl += (nb - bl) * only_b;
        for (k, v) in [r, g, bl, 255.0].into_iter().enumerate() {
            if let Some(slot) = out.get_mut(i.saturating_add(k)) {
                *slot = clamp_u8(v);
            }
        }
        if only_a > 0.35 {
            st.changed_old = st.changed_old.saturating_add(1);
        } else if only_b > 0.35 {
            st.changed_new = st.changed_new.saturating_add(1);
        } else if common > 0.35 {
            st.same = st.same.saturating_add(1);
        }
        i = i.saturating_add(4);
    }
    st
}

/// `alignment({fit, dx, dy})`: old user space → new.
pub fn alignment(fit: Option<[f64; 6]>, dx: f64, dy: f64) -> [f64; 6] {
    let b = fit.unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    let or0 = |v: f64| if v == 0.0 || v.is_nan() { 0.0 } else { v };
    [b[0], b[1], b[2], b[3], b[4] + or0(dx), b[5] + or0(dy)]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mix() {
        let px = |r: u8, g: u8, b: u8| [r, g, b, 255u8];
        let a: Vec<u8> = [px(0, 0, 0), px(0, 0, 0), px(255, 255, 255), px(255, 255, 255)].concat();
        let b: Vec<u8> = [px(0, 0, 0), px(255, 255, 255), px(0, 0, 0), px(255, 255, 255)].concat();
        let mut out = vec![0u8; 16];
        let c = MixColors { bg: [255.0; 3], same: [100.0; 3], old: [200.0, 0.0, 0.0], new: [0.0, 0.0, 200.0] };
        let st = combine(&a, &b, &mut out, &c, 0.5);
        assert_eq!(&out[0..3], &[100, 100, 100]);
        assert_eq!(&out[4..7], &[200, 0, 0]);
        assert_eq!(&out[8..11], &[0, 0, 200]);
        assert_eq!(&out[12..15], &[255, 255, 255]);
        assert_eq!(st, MixStats { changed_old: 1, changed_new: 1, same: 1 });
        combine(&a, &b, &mut out, &c, 1.0);
        assert_eq!(&out[4..7], &[255, 255, 255]);
        assert_eq!(&out[0..3], &[0, 0, 200]);
        assert_eq!(alignment(Some([1.0, 0.0, 0.0, 1.0, 5.0, 5.0]), 1.0, -2.0), [1.0, 0.0, 0.0, 1.0, 6.0, 3.0]);
    }
}
