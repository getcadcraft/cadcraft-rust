// Copyright 2026 The Manifold Authors; Rust port 2026 CadCraft.
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
//
// Adapted (by the Manifold Authors, math.h) from the FreeBSD msun implementations via musl libc (e_acos.c):
// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved. Developed at SunPro/SunSoft, a Sun Microsystems,
// Inc. business. Permission to use, copy, modify, and distribute this software is freely granted, provided that this
// notice is preserved.
//
//! Manifold's deterministic math (include/manifold/math.h: acos, sin, cos, asin, atan, atan2) and common.h's
//! `sind` / `cosd` / `radians` / `degrees`. Same operations in the same order, so they are bit-identical on every
//! platform (`remquo` is libm's port of musl's, which the wasm build links).
#![allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]
// the fdlibm constants as written there; `x - x` is how it makes a NaN
#![allow(clippy::excessive_precision, clippy::approx_constant, clippy::eq_op)]

/// math.h `acos`.
pub(crate) fn acos(x: f64) -> f64 {
    const PIO2_HI: f64 = 1.570_796_326_794_896_558_00e+00;
    const PIO2_LO: f64 = 6.123_233_995_736_766_035_87e-17;
    const PS0: f64 = 1.666_666_666_666_666_574_15e-01;
    const PS1: f64 = -3.255_658_186_224_009_154_05e-01;
    const PS2: f64 = 2.012_125_321_348_629_258_81e-01;
    const PS3: f64 = -4.005_553_450_067_941_140_27e-02;
    const PS4: f64 = 7.915_349_942_898_145_321_76e-04;
    const PS5: f64 = 3.479_331_075_960_211_675_70e-05;
    const QS1: f64 = -2.403_394_911_734_414_218_78e+00;
    const QS2: f64 = 2.020_945_760_233_505_694_71e+00;
    const QS3: f64 = -6.882_839_716_054_532_930_30e-01;
    const QS4: f64 = 7.703_815_055_590_193_527_91e-02;
    // 0x1p-120f, a float literal promoted to double
    const TINY: f64 = 7.523_163_845_262_640_050_999_913_838_222_372_338_039_8e-37;
    let r = |z: f64| -> f64 {
        let p = z * (PS0 + z * (PS1 + z * (PS2 + z * (PS3 + z * (PS4 + z * PS5)))));
        let q = 1.0 + z * (QS1 + z * (QS2 + z * (QS3 + z * QS4)));
        p / q
    };
    let xx = x.to_bits();
    let hx = (xx >> 32) as u32;
    let ix = hx & 0x7fff_ffff;
    if ix >= 0x3ff0_0000 {
        let lx = xx as u32;
        if ((ix - 0x3ff0_0000) | lx) == 0 {
            if (hx >> 31) != 0 {
                return 2.0 * PIO2_HI + TINY;
            }
            return 0.0;
        }
        return 0.0 / (x - x);
    }
    if ix < 0x3fe0_0000 {
        if ix <= 0x3c60_0000 {
            return PIO2_HI + TINY;
        }
        return PIO2_HI - (x - (PIO2_LO - x * r(x * x)));
    }
    if (hx >> 31) != 0 {
        let z = (1.0 + x) * 0.5;
        let s = z.sqrt();
        let w = r(z) * s - PIO2_LO;
        return 2.0 * (PIO2_HI - (s + w));
    }
    let z = (1.0 - x) * 0.5;
    let s = z.sqrt();
    let df = f64::from_bits(s.to_bits() & 0xffff_ffff_0000_0000);
    let c = (z - df * df) / (s + df);
    let w = r(z) * s + c;
    2.0 * (df + w)
}

/// math.h `SinKernel`.
fn sin_kernel(x: f64, y: f64, iy: i32) -> f64 {
    const S1: f64 = -1.666_666_666_666_663_243_48e-01;
    const S2: f64 = 8.333_333_333_322_489_461_24e-03;
    const S3: f64 = -1.984_126_982_985_794_931_34e-04;
    const S4: f64 = 2.755_731_370_707_006_767_89e-06;
    const S5: f64 = -2.505_076_025_340_686_341_95e-08;
    const S6: f64 = 1.589_690_995_211_550_102_21e-10;
    let z = x * x;
    let w = z * z;
    let r = S2 + z * (S3 + z * S4) + z * w * (S5 + z * S6);
    let v = z * x;
    if iy == 0 {
        return x + v * (S1 + z * r);
    }
    x - ((z * (0.5 * y - v * r) - y) - v * S1)
}

/// math.h `CosKernel`.
fn cos_kernel(x: f64, y: f64) -> f64 {
    const C1: f64 = 4.166_666_666_666_660_190_37e-02;
    const C2: f64 = -1.388_888_888_887_410_957_49e-03;
    const C3: f64 = 2.480_158_728_947_672_941_78e-05;
    const C4: f64 = -2.755_731_435_139_066_330_35e-07;
    const C5: f64 = 2.087_572_321_298_174_827_90e-09;
    const C6: f64 = -1.135_964_755_778_819_482_65e-11;
    let z = x * x;
    let w = z * z;
    let r = z * (C1 + z * (C2 + z * C3)) + w * w * (C4 + z * (C5 + z * C6));
    let hz = 0.5 * z;
    let w1 = 1.0 - hz;
    w1 + (((1.0 - w1) - hz) + (z * r - x * y))
}

/// math.h `RemPio2`: x reduced by multiples of pi/2, as (n, y0, y1).
fn rem_pio2(x: f64) -> (i32, f64, f64) {
    const HALF_PI: f64 = 1.570_796_326_794_896_619_231_321_691_639_751_44;
    const TOINT: f64 = 1.5 / f64::EPSILON;
    const PIO4: f64 = f64::from_bits(0x3fe9_21fb_5444_2d18); // 0x1.921fb54442d18p-1
    const INVPIO2: f64 = 6.366_197_723_675_813_824_33e-01;
    const PIO2_1: f64 = 1.570_796_326_734_125_614_17e+00;
    const PIO2_1T: f64 = 6.077_100_506_506_192_249_32e-11;
    const PIO2_2: f64 = 6.077_100_506_303_965_976_60e-11;
    const PIO2_2T: f64 = 2.022_266_248_795_950_631_54e-21;
    const PIO2_3: f64 = 2.022_266_248_711_166_455_80e-21;
    const PIO2_3T: f64 = 8.478_427_660_368_899_569_97e-32;
    let ux = x.to_bits();
    let sign = (ux >> 63) != 0;
    let ix = ((ux >> 32) & 0x7fff_ffff) as u32;
    let small = |k: f64| -> (i32, f64, f64) {
        if !sign {
            let z = x - k * PIO2_1;
            let y0 = z - k * PIO2_1T;
            (k as i32, y0, (z - y0) - k * PIO2_1T)
        } else {
            let z = x + k * PIO2_1;
            let y0 = z + k * PIO2_1T;
            (-(k as i32), y0, (z - y0) + k * PIO2_1T)
        }
    };
    let mut medium = ix < 0x4139_21fb;
    if ix <= 0x400f_6a7a {
        if (ix & 0xfffff) == 0x921fb {
            medium = true;
        } else if ix <= 0x4002_d97c {
            return small(1.0);
        } else {
            return small(2.0);
        }
    } else if ix <= 0x401c_463b {
        if ix <= 0x4015_fdbc {
            if ix == 0x4012_d97c {
                medium = true;
            } else {
                return small(3.0);
            }
        } else if ix == 0x4019_21fb {
            medium = true;
        } else {
            return small(4.0);
        }
    }
    if medium {
        let mut fn_ = x * INVPIO2 + TOINT - TOINT;
        let mut n = fn_ as i32;
        let mut r = x - fn_ * PIO2_1;
        let mut w = fn_ * PIO2_1T;
        if r - w < -PIO4 {
            n -= 1;
            fn_ -= 1.0;
            r = x - fn_ * PIO2_1;
            w = fn_ * PIO2_1T;
        } else if r - w > PIO4 {
            n += 1;
            fn_ += 1.0;
            r = x - fn_ * PIO2_1;
            w = fn_ * PIO2_1T;
        }
        let mut y0 = r - w;
        let ey = ((y0.to_bits() >> 52) & 0x7ff) as i32;
        let ex = (ix >> 20) as i32;
        if ex - ey > 16 {
            let t = r;
            w = fn_ * PIO2_2;
            r = t - w;
            w = fn_ * PIO2_2T - ((t - r) - w);
            y0 = r - w;
            let ey2 = ((y0.to_bits() >> 52) & 0x7ff) as i32;
            if ex - ey2 > 49 {
                let t2 = r;
                w = fn_ * PIO2_3;
                r = t2 - w;
                w = fn_ * PIO2_3T - ((t2 - r) - w);
                y0 = r - w;
            }
        }
        return (n, y0, (r - y0) - w);
    }
    if ix >= 0x7ff0_0000 {
        let y = x - x;
        return (0, y, y);
    }
    let (y0, q) = libm::remquo(x, HALF_PI);
    (q, y0, 0.0)
}

/// math.h `sin`.
pub(crate) fn sin(x: f64) -> f64 {
    let ix = ((x.to_bits() >> 32) & 0x7fff_ffff) as u32;
    if ix <= 0x3fe9_21fb {
        if ix < 0x3e50_0000 {
            return x;
        }
        return sin_kernel(x, 0.0, 0);
    }
    if ix >= 0x7ff0_0000 {
        return x - x;
    }
    let (n, y0, y1) = rem_pio2(x);
    match n & 3 {
        0 => sin_kernel(y0, y1, 1),
        1 => cos_kernel(y0, y1),
        2 => -sin_kernel(y0, y1, 1),
        _ => -cos_kernel(y0, y1),
    }
}

/// math.h `cos`.
pub(crate) fn cos(x: f64) -> f64 {
    let ix = ((x.to_bits() >> 32) & 0x7fff_ffff) as u32;
    if ix <= 0x3fe9_21fb {
        if ix < 0x3e46_a09e {
            return 1.0;
        }
        return cos_kernel(x, 0.0);
    }
    if ix >= 0x7ff0_0000 {
        return x - x;
    }
    let (n, y0, y1) = rem_pio2(x);
    match n & 3 {
        0 => cos_kernel(y0, y1),
        1 => -sin_kernel(y0, y1, 1),
        2 => -cos_kernel(y0, y1),
        _ => sin_kernel(y0, y1, 1),
    }
}

/// math.h `asin` (through Manifold's `acos`).
pub(crate) fn asin(x: f64) -> f64 {
    const HALF_PI: f64 = 1.570_796_326_794_896_619_231_321_691_639_751_44;
    if !x.is_finite() || !(-1.0..=1.0).contains(&x) {
        return f64::NAN;
    }
    if x == 1.0 {
        return HALF_PI;
    }
    if x == -1.0 {
        return -HALF_PI;
    }
    HALF_PI - acos(x)
}

/// math.h `atan`.
pub(crate) fn atan(x: f64) -> f64 {
    const ATANHI: [f64; 4] = [
        4.636_476_090_008_060_935_15e-01,
        7.853_981_633_974_482_789_99e-01,
        9.827_937_232_473_290_540_82e-01,
        1.570_796_326_794_896_558_00e+00,
    ];
    const ATANLO: [f64; 4] = [
        2.269_877_745_296_168_709_24e-17,
        3.061_616_997_868_383_017_93e-17,
        1.390_331_103_123_099_845_16e-17,
        6.123_233_995_736_766_035_87e-17,
    ];
    const AT: [f64; 11] = [
        3.333_333_333_333_293_180_27e-01,
        -1.999_999_999_987_648_324_76e-01,
        1.428_571_427_250_346_637_11e-01,
        -1.111_111_040_546_235_578_80e-01,
        9.090_887_133_436_506_561_96e-02,
        -7.691_876_205_044_829_994_95e-02,
        6.661_073_137_387_531_206_69e-02,
        -5.833_570_133_790_573_486_45e-02,
        4.976_877_994_615_932_360_17e-02,
        -3.653_157_274_421_691_552_70e-02,
        1.628_582_011_536_578_236_23e-02,
    ];
    const TINY: f64 = 7.523_163_845_262_640_050_999_913_838_222_372_338_039_8e-37;
    let mut x = x;
    let mut ix = (x.to_bits() >> 32) as u32;
    let sign = ix >> 31;
    ix &= 0x7fff_ffff;
    let id: i32;
    if ix >= 0x4410_0000 {
        if x.is_nan() {
            return x;
        }
        let z = ATANHI[3] + TINY;
        return if sign != 0 { -z } else { z };
    }
    if ix < 0x3fdc_0000 {
        if ix < 0x3e40_0000 {
            return x;
        }
        id = -1;
    } else {
        x = x.abs();
        if ix < 0x3ff3_0000 {
            if ix < 0x3fe6_0000 {
                id = 0;
                x = (2.0 * x - 1.0) / (2.0 + x);
            } else {
                id = 1;
                x = (x - 1.0) / (x + 1.0);
            }
        } else if ix < 0x4003_8000 {
            id = 2;
            x = (x - 1.5) / (1.0 + 1.5 * x);
        } else {
            id = 3;
            x = -1.0 / x;
        }
    }
    let z = x * x;
    let w = z * z;
    let s1 = z * (AT[0] + w * (AT[2] + w * (AT[4] + w * (AT[6] + w * (AT[8] + w * AT[10])))));
    let s2 = w * (AT[1] + w * (AT[3] + w * (AT[5] + w * (AT[7] + w * AT[9]))));
    let Some(&hi) = ATANHI.get(id as usize) else { return x - x * (s1 + s2) };
    let lo = ATANLO.get(id as usize).copied().unwrap_or(0.0);
    let zz = hi - (x * (s1 + s2) - lo - x);
    if sign != 0 { -zz } else { zz }
}

/// math.h `atan2`.
pub(crate) fn atan2(y: f64, x: f64) -> f64 {
    const PI: f64 = 3.141_592_653_589_793_116_0e+00;
    const PI_LO: f64 = 1.224_646_799_147_353_177_2e-16;
    if x.is_nan() || y.is_nan() {
        return x + y;
    }
    let mut ix = (x.to_bits() >> 32) as u32;
    let mut iy = (y.to_bits() >> 32) as u32;
    let lx = x.to_bits() as u32;
    let ly = y.to_bits() as u32;
    if ((ix.wrapping_sub(0x3ff0_0000)) | lx) == 0 {
        return atan(y);
    }
    let m = ((iy >> 31) & 1) | ((ix >> 30) & 2);
    ix &= 0x7fff_ffff;
    iy &= 0x7fff_ffff;
    if (iy | ly) == 0 {
        return match m {
            0 | 1 => y,
            2 => PI,
            _ => -PI,
        };
    }
    if (ix | lx) == 0 {
        return if m & 1 != 0 { -PI / 2.0 } else { PI / 2.0 };
    }
    if ix == 0x7ff0_0000 {
        if iy == 0x7ff0_0000 {
            return match m {
                0 => PI / 4.0,
                1 => -PI / 4.0,
                2 => 3.0 * PI / 4.0,
                _ => -3.0 * PI / 4.0,
            };
        }
        return match m {
            0 => 0.0,
            1 => -0.0,
            2 => PI,
            _ => -PI,
        };
    }
    if ix.wrapping_add(64 << 20) < iy || iy == 0x7ff0_0000 {
        return if m & 1 != 0 { -PI / 2.0 } else { PI / 2.0 };
    }
    let z = if (m & 2) != 0 && iy.wrapping_add(64 << 20) < ix { 0.0 } else { atan((y / x).abs()) };
    match m {
        0 => z,
        1 => -z,
        2 => PI - (z - PI_LO),
        _ => (z - PI_LO) - PI,
    }
}

/// common.h `kPi`.
const K_PI: f64 = 3.141_592_653_589_793_238_462_643_383_279_502_88;

/// common.h `radians`: `a * kPi / 180`.
#[inline]
pub(crate) fn radians(a: f64) -> f64 {
    a * K_PI / 180.0
}

/// common.h `degrees`: `a * 180 / kPi`.
#[inline]
pub(crate) fn degrees(a: f64) -> f64 {
    a * 180.0 / K_PI
}

/// common.h `sind`: sine in degrees, exact at multiples of 90 (`remquo` as musl computes it, which the wasm uses).
pub(crate) fn sind(x: f64) -> f64 {
    if !x.is_finite() {
        return f64::NAN;
    }
    if x < 0.0 {
        return -sind(-x);
    }
    let (r, quo) = libm::remquo(x.abs(), 90.0);
    let xr = radians(r);
    match quo % 4 {
        0 => sin(xr),
        1 => cos(xr),
        2 => -sin(xr),
        3 => -cos(xr),
        _ => 0.0,
    }
}

/// common.h `cosd`.
#[inline]
pub(crate) fn cosd(x: f64) -> f64 {
    sind(x + 90.0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn tiny_is_two_to_minus_120() {
        assert_eq!(super::acos(1.0), 0.0);
        assert_eq!(f64::from(2f32.powi(-120)), 7.523_163_845_262_640_050_999_913_838_222_372_338_039_8e-37);
        let a = super::acos(0.5);
        assert!((a - core::f64::consts::FRAC_PI_3).abs() < 1e-15);
    }

    #[test]
    fn trig_matches_musl() {
        // math.h is musl's sin / cos / atan / atan2 restated: bit-equal to libm's ports of the same code
        let mut x = -1000.0f64;
        while x < 1000.0 {
            assert_eq!(super::sin(x).to_bits(), libm::sin(x).to_bits(), "sin {x}");
            assert_eq!(super::cos(x).to_bits(), libm::cos(x).to_bits(), "cos {x}");
            assert_eq!(super::atan(x).to_bits(), libm::atan(x).to_bits(), "atan {x}");
            assert_eq!(super::atan2(x, 0.37).to_bits(), libm::atan2(x, 0.37).to_bits(), "atan2 {x}");
            x += 0.913_7;
        }
        // up to 2^20·pi/2; past it math.h reduces with remquo (musl uses __rem_pio2_large)
        for x in [1.5e6, -1.2e6, 5.0e-9, 1e-300] {
            assert_eq!(super::sin(x).to_bits(), libm::sin(x).to_bits(), "sin {x}");
            assert_eq!(super::cos(x).to_bits(), libm::cos(x).to_bits(), "cos {x}");
        }
    }

    #[test]
    fn degrees_exact_at_quarter_turns() {
        assert_eq!(super::sind(90.0), 1.0);
        assert_eq!(super::cosd(90.0), 0.0);
        assert_eq!(super::sind(180.0), 0.0);
        assert_eq!(super::cosd(-180.0), -1.0);
        assert_eq!(super::sind(270.0), -1.0);
        assert!(super::sind(f64::NAN).is_nan());
    }
}
