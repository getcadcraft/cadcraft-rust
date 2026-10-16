// Copyright 2026 The Manifold Authors; Rust port 2026 CadCraft.
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file except in compliance with
// the License. You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
//
// Adapted (by the Manifold Authors, math.h) from the FreeBSD msun implementations via musl libc (e_acos.c):
// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved. Developed at SunPro/SunSoft, a Sun Microsystems,
// Inc. business. Permission to use, copy, modify, and distribute this software is freely granted, provided that this
// notice is preserved.
//
//! Manifold's deterministic `math::acos` (include/manifold/math.h), the only transcendental function on the boolean
//! path (vertex pseudo-normals). Same operations in the same order, so it is bit-identical on every platform.
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

#[cfg(test)]
mod tests {
    #[test]
    fn tiny_is_two_to_minus_120() {
        assert_eq!(super::acos(1.0), 0.0);
        assert_eq!(f64::from(2f32.powi(-120)), 7.523_163_845_262_640_050_999_913_838_222_372_338_039_8e-37);
        let a = super::acos(0.5);
        assert!((a - core::f64::consts::FRAC_PI_3).abs() < 1e-15);
    }
}
