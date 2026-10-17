//! `Math.*` exactly as V8 computes it, for the functions proj4js uses. Measured against Node (V8) on 200 000 arguments
//! each (2026-10-16): `asin`, `acos`, `atan`, `atan2`, `tan`, `exp` match the `libm` crate bit for bit and `pow` the
//! platform `powf`; `sin` / `cos` are fdlibm 5.3 (cc-m3d's `jsmath`); `log` differs from `libm` (musl's newer algorithm)
//! in about 0.3 % of arguments, so fdlibm's `e_log.c` — what V8's `ieee754::log` is — is ported here ([`log`]): 0 of 200 000
//! differ. ("Developed at SunSoft, a Sun Microsystems, Inc. business. Permission to use, copy, modify, and distribute this
//! software is freely granted, provided that this notice is preserved." — see NOTICE.)

#![allow(clippy::excessive_precision, clippy::approx_constant)]

use cc_m3d::jsmath::JsMath;

#[inline]
pub fn sin(x: f64) -> f64 {
    x.jsin()
}
#[inline]
pub fn cos(x: f64) -> f64 {
    x.jcos()
}
#[inline]
pub fn tan(x: f64) -> f64 {
    libm::tan(x)
}
#[inline]
pub fn asin(x: f64) -> f64 {
    libm::asin(x)
}
#[inline]
pub fn acos(x: f64) -> f64 {
    libm::acos(x)
}
#[inline]
pub fn atan(x: f64) -> f64 {
    libm::atan(x)
}
/// `Math.atan2(y, x)`.
#[inline]
pub fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}
#[inline]
pub fn exp(x: f64) -> f64 {
    libm::exp(x)
}
/// `Math.pow(x, y)`: V8's ieee754::pow is fdlibm's e_pow, which answers a few exponents exactly before its general path
/// (y = 2 → x·x, y = 0.5 → √x for x ≥ 0, y = ±1); the platform `powf` (correctly rounded) can differ from x·x in the
/// last bit (found by cvdiff: `Math.pow(b, 2)` in proj4's hypot, UTM zone 34). Other arguments: `powf` matched V8 on
/// 200 000 random pairs.
#[inline]
pub fn pow(x: f64, y: f64) -> f64 {
    if y == 2.0 {
        x * x
    } else if y == 1.0 {
        x
    } else if y == -1.0 {
        1.0 / x
    } else if y == 0.5 && x >= 0.0 && x.is_finite() {
        x.sqrt()
    } else {
        x.jpow(y)
    }
}
#[inline]
pub fn sqrt(x: f64) -> f64 {
    x.sqrt()
}
#[inline]
pub fn abs(x: f64) -> f64 {
    x.abs()
}
#[inline]
pub fn floor(x: f64) -> f64 {
    x.floor()
}
/// `Math.round`.
#[inline]
pub fn round(x: f64) -> f64 {
    cc_cad::js::round(x)
}
/// `Math.max(a, b)` (NaN wins).
#[inline]
pub fn max(a: f64, b: f64) -> f64 {
    cc_cad::js::max(a, b)
}
/// `Math.min(a, b)` (NaN wins).
#[inline]
pub fn min(a: f64, b: f64) -> f64 {
    cc_cad::js::min(a, b)
}
/// `Math.hypot(a, b)` (V8).
#[inline]
pub fn hypot(a: f64, b: f64) -> f64 {
    cc_cad::js::hypot(a, b)
}

#[inline]
fn hi(x: f64) -> i32 {
    // the high 32 bits as fdlibm's signed int
    i32::from_ne_bytes(((x.to_bits() >> 32) as u32).to_ne_bytes())
}

/// fdlibm 5.3 `__ieee754_log` (V8's `Math.log`).
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
    let lx = (x.to_bits() & 0xffff_ffff) as u32;
    let mut k: i32 = 0;
    if hx < 0x0010_0000 {
        if ((hx & 0x7fff_ffff) as u32 | lx) == 0 {
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
    let high = (hx | (i ^ 0x3ff0_0000)) as u32;
    x = f64::from_bits((u64::from(high) << 32) | (x.to_bits() & 0xffff_ffff));
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

#[cfg(test)]
mod tests {
    #[test]
    fn logs() {
        assert_eq!(super::log(1.0), 0.0);
        assert_eq!(super::log(std::f64::consts::E), 1.0);
        assert!(super::log(-1.0).is_nan());
        assert_eq!(super::log(0.0), f64::NEG_INFINITY);
    }
}
