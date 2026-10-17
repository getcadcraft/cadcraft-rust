//! `Math.log`, `Math.log10` and `Math.exp` exactly as V8 computes them. V8's `base::ieee754` is fdlibm 5.3; the `libm`
//! crate follows musl's newer `log` / `log10`, which differ from it in the last bit for a fraction of arguments (found by
//! pcdiff: `log(pi)` in a parameter expression), so fdlibm's `e_log.c` and `e_log10.c` are ported here. `exp` matches
//! `libm` except that V8 answers `Math.exp(1)` with `Math.E` (fdlibm is one ulp above it there).
//! ("Developed at SunSoft, a Sun Microsystems, Inc. business. Permission to use, copy, modify, and distribute this
//! software is freely granted, provided that this notice is preserved.")
//! Merge note: cc-proj carries the same `log` (layer 2, so not reachable from here); one shared copy in cc-m3d's
//! jsmath would serve both.

#![allow(clippy::excessive_precision, clippy::approx_constant)]

/// The high 32 bits as fdlibm's signed int.
fn hi(x: f64) -> i32 {
    i32::from_ne_bytes(u32::try_from(x.to_bits() >> 32).unwrap_or(0).to_ne_bytes())
}
/// The low 32 bits.
fn lo(x: f64) -> u32 {
    u32::try_from(x.to_bits() & 0xffff_ffff).unwrap_or(0)
}
/// `x` with its high word replaced.
fn with_hi(x: f64, h: i32) -> f64 {
    let hb = u64::from(u32::from_ne_bytes(h.to_ne_bytes()));
    f64::from_bits((hb << 32) | u64::from(lo(x)))
}

/// fdlibm `__ieee754_log`.
pub fn log(mut x: f64) -> f64 {
    const LN2_HI: f64 = 6.93147180369123816490e-01;
    const LN2_LO: f64 = 1.90821492927058770002e-10;
    const TWO54: f64 = 1.80143985094819840000e+16;
    const LG1: f64 = 6.666666666666735130e-01;
    const LG2: f64 = 3.999999999940941908e-01;
    const LG3: f64 = 2.857142874366239149e-01;
    const LG4: f64 = 2.222219843214978396e-01;
    const LG5: f64 = 1.818357216161805012e-01;
    const LG6: f64 = 1.531383769920937332e-01;
    const LG7: f64 = 1.479819860511658591e-01;
    let mut hx = hi(x);
    let lx = lo(x);
    let mut k: i32 = 0;
    if hx < 0x0010_0000 {
        if (hx & 0x7fff_ffff) == 0 && lx == 0 {
            return f64::NEG_INFINITY;
        }
        if hx < 0 {
            return f64::NAN;
        }
        k = k.wrapping_sub(54);
        x *= TWO54;
        hx = hi(x);
    }
    if hx >= 0x7ff0_0000 {
        return x + x;
    }
    k = k.wrapping_add((hx >> 20).wrapping_sub(1023));
    hx &= 0x000f_ffff;
    let i = hx.wrapping_add(0x95f64) & 0x10_0000;
    x = with_hi(x, hx | (i ^ 0x3ff0_0000));
    k = k.wrapping_add(i >> 20);
    let f = x - 1.0;
    if (0x000f_ffff & hx.wrapping_add(2)) < 3 {
        if f == 0.0 {
            if k == 0 {
                return 0.0;
            }
            let dk = f64::from(k);
            return dk * LN2_HI + dk * LN2_LO;
        }
        let r = f * f * (0.5 - 0.33333333333333333 * f);
        if k == 0 {
            return f - r;
        }
        let dk = f64::from(k);
        return dk * LN2_HI - ((r - dk * LN2_LO) - f);
    }
    let s = f / (2.0 + f);
    let dk = f64::from(k);
    let z = s * s;
    let mut i = hx.wrapping_sub(0x6147a);
    let w = z * z;
    let j = 0x6b851_i32.wrapping_sub(hx);
    let t1 = w * (LG2 + w * (LG4 + w * LG6));
    let t2 = z * (LG1 + w * (LG3 + w * (LG5 + w * LG7)));
    i |= j;
    let r = t2 + t1;
    if i > 0 {
        let hfsq = 0.5 * f * f;
        if k == 0 { f - (hfsq - s * (hfsq + r)) } else { dk * LN2_HI - ((hfsq - (s * (hfsq + r) + dk * LN2_LO)) - f) }
    } else if k == 0 {
        f - s * (f - r)
    } else {
        dk * LN2_HI - ((s * (f - r) - dk * LN2_LO) - f)
    }
}

/// fdlibm `__ieee754_log10`.
pub fn log10(mut x: f64) -> f64 {
    const TWO54: f64 = 1.80143985094819840000e+16;
    const IVLN10: f64 = 4.34294481903251816668e-01;
    const LOG10_2HI: f64 = 3.01029995663611771306e-01;
    const LOG10_2LO: f64 = 3.69423907715893078616e-13;
    let mut hx = hi(x);
    let lx = lo(x);
    let mut k: i32 = 0;
    if hx < 0x0010_0000 {
        if (hx & 0x7fff_ffff) == 0 && lx == 0 {
            return f64::NEG_INFINITY;
        }
        if hx < 0 {
            return f64::NAN;
        }
        k = k.wrapping_sub(54);
        x *= TWO54;
        hx = hi(x);
    }
    if hx >= 0x7ff0_0000 {
        return x + x;
    }
    k = k.wrapping_add((hx >> 20).wrapping_sub(1023));
    let i: i32 = i32::from(k < 0);
    hx = (hx & 0x000f_ffff) | (0x3ff_i32.wrapping_sub(i) << 20);
    let y = f64::from(k.wrapping_add(i));
    x = with_hi(x, hx);
    let z = y * LOG10_2LO + IVLN10 * log(x);
    z + y * LOG10_2HI
}

/// `Math.exp`.
pub fn exp(x: f64) -> f64 {
    if x == 1.0 { std::f64::consts::E } else { libm::exp(x) }
}

#[cfg(test)]
mod tests {
    #[test]
    fn logs() {
        assert_eq!(super::log(1.0), 0.0);
        assert_eq!(super::log(std::f64::consts::E), 1.0);
        assert!(super::log(-1.0).is_nan());
        assert_eq!(super::log(0.0), f64::NEG_INFINITY);
        assert_eq!(super::log10(1000.0), 3.0);
        assert_eq!(super::log10(std::f64::consts::PI), 0.4971498726941338);
        assert_eq!(super::exp(1.0), 2.718281828459045);
        // optional sweep against Node: PC_FDLIBM_VECTORS = file of "x log log10 exp" bit patterns (hex) per line
        if let Ok(p) = std::env::var("PC_FDLIBM_VECTORS") {
            let text = std::fs::read_to_string(p).unwrap_or_default();
            let mut bad = [0usize; 3];
            for l in text.lines() {
                let v: Vec<u64> = l.split(' ').filter_map(|h| u64::from_str_radix(h, 16).ok()).collect();
                let x = f64::from_bits(v[0]);
                for (k, got) in [super::log(x), super::log10(x), super::exp(x)].iter().enumerate() {
                    if got.to_bits() != v[k + 1] && !(got.is_nan() && f64::from_bits(v[k + 1]).is_nan()) {
                        bad[k] += 1;
                    }
                }
            }
            assert_eq!(bad, [0, 0, 0], "log / log10 / exp mismatches");
        }
    }
}
