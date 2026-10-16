//! The transcendental functions exactly as V8 computes them. Measured against Node 24 (V8 13.6) on 500 000 arguments
//! (2026-10-16): `Math.tan` / `atan` / `atan2` / `acos` are FreeBSD msun ports and match the `libm` crate (also msun)
//! bit for bit; `Math.pow` matched the platform `powf` bit for bit; `Math.sin` / `cos` use the classic fdlibm 5.3
//! kernels (Horner polynomials, the `qx` trick in `__kernel_cos`), not msun's re-ordered ones, so they are ported
//! here ([`fd_sin`] / [`fd_cos`]): 0 differences for |x| < 823 549 (2^19·π/2); above that the argument reduction is
//! msun's large one and this falls back to `libm` (≈ 1.8 % of such huge angles differ in the last bit). The platform
//! libm alone rounded differently often enough to flip a `ceil` or a nearest-candidate choice (a semicircle bulge
//! cut into 48 or 49 pieces, a polyline corner target picking the other segment). Every call in this crate goes
//! through [`JsMath`]. On wasm `powf` is msun's `pow`, not glibc's: last-bit differences are possible there.

// fdlibm's constants are written as in its sources (Rust reads them to the same doubles)
#![allow(clippy::excessive_precision, clippy::approx_constant)]

/// `x.jsin()` = `Math.sin(x)` etc.
pub trait JsMath {
    fn jsin(self) -> f64;
    fn jcos(self) -> f64;
    fn jtan(self) -> f64;
    fn jatan(self) -> f64;
    fn jacos(self) -> f64;
    /// `Math.atan2(self, x)`
    fn jatan2(self, x: f64) -> f64;
    /// `Math.pow(self, y)`
    fn jpow(self, y: f64) -> f64;
}
impl JsMath for f64 {
    fn jsin(self) -> f64 {
        fd_sin(self)
    }
    fn jcos(self) -> f64 {
        fd_cos(self)
    }
    fn jtan(self) -> f64 {
        libm::tan(self)
    }
    fn jatan(self) -> f64 {
        libm::atan(self)
    }
    fn jacos(self) -> f64 {
        libm::acos(self)
    }
    fn jatan2(self, x: f64) -> f64 {
        libm::atan2(self, x)
    }
    fn jpow(self, y: f64) -> f64 {
        self.powf(y)
    }
}

// -------------------------------------------------------------------------------------------------------------------
// fdlibm 5.3 sin / cos (the kernels and the small / medium argument reduction), as V8's base/ieee754.cc has them.
// Constants as in fdlibm (and the `libm` crate):
// ====================================================
// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
//
// Developed at SunSoft, a Sun Microsystems, Inc. business.
// Permission to use, copy, modify, and distribute this
// software is freely granted, provided that this notice
// is preserved.
// ====================================================

const C1: f64 = 4.16666666666666019037e-02; // 0x3FA55555, 0x5555554C
const C2: f64 = -1.38888888888741095749e-03; // 0xBF56C16C, 0x16C15177
const C3: f64 = 2.48015872894767294178e-05; // 0x3EFA01A0, 0x19CB1590
const C4: f64 = -2.75573143513906633035e-07; // 0xBE927E4F, 0x809C52AD
const C5: f64 = 2.08757232129817482790e-09; // 0x3E21EE9E, 0xBDB4B1C4
const C6: f64 = -1.13596475577881948265e-11; // 0xBDA8FAE9, 0xBE8838D4
const S1: f64 = -1.66666666666666324348e-01; // 0xBFC55555, 0x55555549
const S2: f64 = 8.33333333332248946124e-03; // 0x3F811111, 0x1110F8A6
const S3: f64 = -1.98412698298579493134e-04; // 0xBF2A01A0, 0x19C161D5
const S4: f64 = 2.75573137070700676789e-06; // 0x3EC71DE3, 0x57B1FE7D
const S5: f64 = -2.50507602534068634195e-08; // 0xBE5AE5E6, 0x8A2B9CEB
const S6: f64 = 1.58969099521155010221e-10; // 0x3DE5D93A, 0x5ACFD57C
const INV_PIO2: f64 = 6.36619772367581382433e-01; // 0x3FE45F30, 0x6DC9C883
const PIO2_1: f64 = 1.57079632673412561417e+00; // 0x3FF921FB, 0x54400000
const PIO2_1T: f64 = 6.07710050650619224932e-11; // 0x3DD0B461, 0x1A626331
const PIO2_2: f64 = 6.07710050630396597660e-11; // 0x3DD0B461, 0x1A600000
const PIO2_2T: f64 = 2.02226624879595063154e-21; // 0x3BA3198A, 0x2E037073
const PIO2_3: f64 = 2.02226624871116645580e-21; // 0x3BA3198A, 0x2E000000
const PIO2_3T: f64 = 8.47842766036889956997e-32; // 0x397B839A, 0x252049C1

/// The high 32 bits of a double.
fn hi(x: f64) -> u32 {
    u32::try_from(x.to_bits() >> 32).unwrap_or(0)
}
/// `(int32_t)x` for |x| < 2^31 (the callers check the range).
fn trunc_i32(x: f64) -> i32 {
    let t = x.trunc();
    if t.abs() < 2_147_483_648.0 {
        // in range by the check above
        #[allow(clippy::cast_possible_truncation)]
        let i = t as i32;
        i
    } else {
        0
    }
}

/// __kernel_cos(x, y): cos on [-π/4, π/4], y the tail of x.
fn k_cos(x: f64, y: f64) -> f64 {
    let ix = hi(x) & 0x7fff_ffff;
    if ix < 0x3e40_0000 && trunc_i32(x) == 0 {
        return 1.0;
    }
    let z = x * x;
    let r = z * (C1 + z * (C2 + z * (C3 + z * (C4 + z * (C5 + z * C6)))));
    if ix < 0x3fd3_3333 {
        return 1.0 - (0.5 * z - (z * r - x * y));
    }
    let qx = if ix > 0x3fe9_0000 { 0.28125 } else { f64::from_bits(u64::from(ix - 0x0020_0000) << 32) };
    let iz = 0.5 * z - qx;
    let a = 1.0 - qx;
    a - (iz - (z * r - x * y))
}

/// __kernel_sin(x, y, iy): sin on [-π/4, π/4]; iy = 0 when y is 0.
fn k_sin(x: f64, y: f64, iy: bool) -> f64 {
    let ix = hi(x) & 0x7fff_ffff;
    if ix < 0x3e40_0000 && trunc_i32(x) == 0 {
        return x;
    }
    let z = x * x;
    let v = z * x;
    let r = S2 + z * (S3 + z * (S4 + z * (S5 + z * S6)));
    if !iy { x + v * (S1 + z * r) } else { x - ((z * (0.5 * y - v * r) - y) - v * S1) }
}

/// __ieee754_rem_pio2 for |x| ≤ 2^19·π/2: (n, y0, y1) with x = n·π/2 + y0 + y1; None for larger |x|.
fn rem_pio2(x: f64) -> Option<(i32, f64, f64)> {
    let hx = hi(x);
    let neg = hx >> 31 != 0;
    let ix = hx & 0x7fff_ffff;
    if ix <= 0x3fe9_21fb {
        return Some((0, x, 0.0));
    }
    if ix < 0x4002_d97c {
        // |x| < 3π/4: n = ±1
        return Some(if !neg {
            let mut z = x - PIO2_1;
            if ix != 0x3ff9_21fb {
                let y0 = z - PIO2_1T;
                (1, y0, (z - y0) - PIO2_1T)
            } else {
                z -= PIO2_2;
                let y0 = z - PIO2_2T;
                (1, y0, (z - y0) - PIO2_2T)
            }
        } else {
            let mut z = x + PIO2_1;
            if ix != 0x3ff9_21fb {
                let y0 = z + PIO2_1T;
                (-1, y0, (z - y0) + PIO2_1T)
            } else {
                z += PIO2_2;
                let y0 = z + PIO2_2T;
                (-1, y0, (z - y0) + PIO2_2T)
            }
        });
    }
    if ix > 0x4139_21fb {
        return None;
    }
    let t = x.abs();
    let n = trunc_i32(t * INV_PIO2 + 0.5);
    let fnn = f64::from(n);
    let mut r = t - fnn * PIO2_1;
    let mut w = fnn * PIO2_1T;
    let mut y0 = r - w;
    // npio2_hw[n - 1]: the high word of n·π/2
    let quick = n < 32 && ix != hi(fnn * std::f64::consts::FRAC_PI_2);
    if !quick {
        let j = (ix >> 20) & 0x7ff;
        let i = j.wrapping_sub((hi(y0) >> 20) & 0x7ff);
        if i > 16 && i < 0x800 {
            let t = r;
            w = fnn * PIO2_2;
            r = t - w;
            w = fnn * PIO2_2T - ((t - r) - w);
            y0 = r - w;
            let i = j.wrapping_sub((hi(y0) >> 20) & 0x7ff);
            if i > 49 && i < 0x800 {
                let t = r;
                w = fnn * PIO2_3;
                r = t - w;
                w = fnn * PIO2_3T - ((t - r) - w);
                y0 = r - w;
            }
        }
    }
    let y1 = (r - y0) - w;
    Some(if neg { (n.wrapping_neg(), -y0, -y1) } else { (n, y0, y1) })
}

/// fdlibm `sin` (V8's `Math.sin`).
pub fn fd_sin(x: f64) -> f64 {
    let ix = hi(x) & 0x7fff_ffff;
    if ix <= 0x3fe9_21fb {
        return k_sin(x, 0.0, false);
    }
    if ix >= 0x7ff0_0000 {
        return f64::NAN; // x - x: NaN for ±Infinity and NaN
    }
    match rem_pio2(x) {
        None => libm::sin(x),
        Some((n, a, b)) => match n & 3 {
            0 => k_sin(a, b, true),
            1 => k_cos(a, b),
            2 => -k_sin(a, b, true),
            _ => -k_cos(a, b),
        },
    }
}

/// fdlibm `cos` (V8's `Math.cos`).
pub fn fd_cos(x: f64) -> f64 {
    let ix = hi(x) & 0x7fff_ffff;
    if ix <= 0x3fe9_21fb {
        return k_cos(x, 0.0);
    }
    if ix >= 0x7ff0_0000 {
        return f64::NAN; // x - x: NaN for ±Infinity and NaN
    }
    match rem_pio2(x) {
        None => libm::cos(x),
        Some((n, a, b)) => match n & 3 {
            0 => k_cos(a, b),
            1 => -k_sin(a, b, true),
            2 => -k_cos(a, b),
            _ => k_sin(a, b, true),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn npio2_high_words() {
        // the fdlibm table npio2_hw[0..32], which rem_pio2 computes from n·π/2
        const T: [u32; 32] = [
            0x3FF921FB, 0x400921FB, 0x4012D97C, 0x401921FB, 0x401F6A7A, 0x4022D97C, 0x4025FDBB, 0x402921FB, 0x402C463A, 0x402F6A7A,
            0x4031475C, 0x4032D97C, 0x40346B9C, 0x4035FDBB, 0x40378FDB, 0x403921FB, 0x403AB41B, 0x403C463A, 0x403DD85A, 0x403F6A7A,
            0x40407E4C, 0x4041475C, 0x4042106C, 0x4042D97C, 0x4043A28C, 0x40446B9C, 0x404534AC, 0x4045FDBB, 0x4046C6CB, 0x40478FDB,
            0x404858EB, 0x404921FB,
        ];
        for (i, w) in T.iter().enumerate() {
            assert_eq!(hi(f64::from(u32::try_from(i + 1).unwrap()) * std::f64::consts::FRAC_PI_2), *w, "n = {}", i + 1);
        }
    }
    #[test]
    fn known_values() {
        // V8 (Node 24): Math.sin / Math.cos of these arguments
        assert_eq!(fd_sin(0.5), 0.479425538604203);
        assert_eq!(fd_cos(1.0), 0.5403023058681398);
        assert_eq!(fd_sin(std::f64::consts::PI), 1.2246467991473532e-16);
        assert_eq!(fd_cos(100.0), 0.8623188722876839);
    }
}
