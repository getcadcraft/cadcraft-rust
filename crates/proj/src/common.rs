//! proj4js `lib/common/*.js` and `lib/constants/values.js`.

use crate::math::{abs, asin, atan, cos, exp, log, pow, sin, sqrt, tan};

pub const PJD_3PARAM: i32 = 1;
pub const PJD_7PARAM: i32 = 2;
pub const PJD_GRIDSHIFT: i32 = 3;
pub const PJD_WGS84: i32 = 4;
pub const PJD_NODATUM: i32 = 5;
pub const SRS_WGS84_SEMIMAJOR: f64 = 6378137.0;
pub const SRS_WGS84_SEMIMINOR: f64 = 6356752.314;
pub const SRS_WGS84_ESQUARED: f64 = 0.0066943799901413165;
pub const SEC_TO_RAD: f64 = 4.84813681109535993589914102357e-6;
pub const HALF_PI: f64 = std::f64::consts::PI / 2.0;
pub const SIXTH: f64 = 0.1666666666666666667;
pub const RA4: f64 = 0.04722222222222222222;
pub const RA6: f64 = 0.02215608465608465608;
pub const EPSLN: f64 = 1.0e-10;
pub const D2R: f64 = 0.01745329251994329577;
pub const R2D: f64 = 57.29577951308232088;
pub const FORTPI: f64 = std::f64::consts::PI / 4.0;
pub const TWO_PI: f64 = std::f64::consts::PI * 2.0;
pub const SPI: f64 = 3.14159265359;
pub const PI: f64 = std::f64::consts::PI;

#[inline]
pub fn sign(x: f64) -> f64 {
    if x < 0.0 { -1.0 } else { 1.0 }
}
/// `adjust_lon(x, skipAdjust)`.
#[inline]
pub fn adjust_lon(x: f64, skip: bool) -> f64 {
    if skip {
        return x;
    }
    if abs(x) <= SPI { x } else { x - sign(x) * TWO_PI }
}
#[inline]
pub fn adjust_lat(x: f64) -> f64 {
    if abs(x) < HALF_PI { x } else { x - sign(x) * PI }
}
/// `adjust_zone(zone, lon)` (None = undefined).
pub fn adjust_zone(zone: Option<f64>, lon: f64) -> Option<f64> {
    match zone {
        None => {
            let z = ((adjust_lon(lon, false) + PI) * 30.0 / PI).floor() + 1.0;
            if z < 0.0 {
                Some(0.0)
            } else if z > 60.0 {
                Some(60.0)
            } else {
                Some(z)
            }
        }
        z => z,
    }
}
pub fn asinz(x: f64) -> f64 {
    let x = if abs(x) > 1.0 { if x > 1.0 { 1.0 } else { -1.0 } } else { x };
    asin(x)
}
pub fn authlat(beta: f64, apa: &[f64; 3]) -> f64 {
    let t = beta + beta;
    beta + apa[0] * sin(t) + apa[1] * sin(t + t) + apa[2] * sin(t + t + t)
}
pub fn authset(es: f64) -> [f64; 3] {
    const P00: f64 = 0.33333333333333333333;
    const P01: f64 = 0.17222222222222222222;
    const P02: f64 = 0.10257936507936507936;
    const P10: f64 = 0.06388888888888888888;
    const P11: f64 = 0.06640211640211640211;
    const P20: f64 = 0.01641501294219154443;
    let mut apa = [0.0; 3];
    apa[0] = es * P00;
    let mut t = es * es;
    apa[0] += t * P01;
    apa[1] = t * P10;
    t *= es;
    apa[0] += t * P02;
    apa[1] += t * P11;
    apa[2] = t * P20;
    apa
}
pub fn clens(pp: &[f64], arg_r: f64) -> f64 {
    let r = 2.0 * cos(arg_r);
    let mut i = pp.len();
    let Some(last) = pp.last() else { return f64::NAN };
    i -= 1;
    let mut hr1 = *last;
    let mut hr2 = 0.0;
    let mut hr = f64::NAN;
    while i > 0 {
        i -= 1;
        hr = -hr2 + r * hr1 + pp.get(i).copied().unwrap_or(f64::NAN);
        hr2 = hr1;
        hr1 = hr;
    }
    sin(arg_r) * hr
}
#[inline]
pub fn sinh(x: f64) -> f64 {
    let r = exp(x);
    (r - 1.0 / r) / 2.0
}
#[inline]
pub fn cosh(x: f64) -> f64 {
    let r = exp(x);
    (r + 1.0 / r) / 2.0
}
pub fn clens_cmplx(pp: &[f64], arg_r: f64, arg_i: f64) -> [f64; 2] {
    let sin_arg_r = sin(arg_r);
    let cos_arg_r = cos(arg_r);
    let sinh_arg_i = sinh(arg_i);
    let cosh_arg_i = cosh(arg_i);
    let mut r = 2.0 * cos_arg_r * cosh_arg_i;
    let mut i = -2.0 * sin_arg_r * sinh_arg_i;
    let mut j = pp.len();
    let Some(last) = pp.last() else { return [f64::NAN, f64::NAN] };
    j -= 1;
    let mut hr = *last;
    let mut hi1 = 0.0;
    let mut hr1 = 0.0;
    let mut hi = 0.0;
    let (mut hr2, mut hi2);
    while j > 0 {
        j -= 1;
        hr2 = hr1;
        hi2 = hi1;
        hr1 = hr;
        hi1 = hi;
        hr = -hr2 + r * hr1 - i * hi1 + pp.get(j).copied().unwrap_or(f64::NAN);
        hi = -hi2 + i * hr1 + r * hi1;
    }
    r = sin_arg_r * cosh_arg_i;
    i = cos_arg_r * sinh_arg_i;
    [r * hr - i * hi, r * hi + i * hr]
}
pub fn e0fn(x: f64) -> f64 {
    1.0 - 0.25 * x * (1.0 + x / 16.0 * (3.0 + 1.25 * x))
}
pub fn e1fn(x: f64) -> f64 {
    0.375 * x * (1.0 + 0.25 * x * (1.0 + 0.46875 * x))
}
pub fn e2fn(x: f64) -> f64 {
    0.05859375 * x * x * (1.0 + 0.75 * x)
}
pub fn e3fn(x: f64) -> f64 {
    x * x * x * (35.0 / 3072.0)
}
pub fn g_n(a: f64, e: f64, sinphi: f64) -> f64 {
    let temp = e * sinphi;
    a / sqrt(1.0 - temp * temp)
}
pub fn gatg(pp: &[f64], b: f64) -> f64 {
    let cos_2b = 2.0 * cos(2.0 * b);
    let mut i = pp.len();
    let Some(last) = pp.last() else { return f64::NAN };
    i -= 1;
    let mut h1 = *last;
    let mut h2 = 0.0;
    let mut h = f64::NAN;
    while i > 0 {
        i -= 1;
        h = -h2 + cos_2b * h1 + pp.get(i).copied().unwrap_or(f64::NAN);
        h2 = h1;
        h1 = h;
    }
    b + h * sin(2.0 * b)
}
/// common/hypot.js (not Math.hypot).
pub fn hypot(x: f64, y: f64) -> f64 {
    let x = abs(x);
    let y = abs(y);
    let a = crate::math::max(x, y);
    let b = crate::math::min(x, y) / (if a != 0.0 && !a.is_nan() { a } else { 1.0 });
    a * sqrt(1.0 + pow(b, 2.0))
}
pub fn imlfn(ml: f64, e0: f64, e1: f64, e2: f64, e3: f64) -> f64 {
    let mut phi = ml / e0;
    for _ in 0..15 {
        let dphi = (ml - (e0 * phi - e1 * sin(2.0 * phi) + e2 * sin(4.0 * phi) - e3 * sin(6.0 * phi)))
            / (e0 - 2.0 * e1 * cos(2.0 * phi) + 4.0 * e2 * cos(4.0 * phi) - 6.0 * e3 * cos(6.0 * phi));
        phi += dphi;
        if abs(dphi) <= 0.0000000001 {
            return phi;
        }
    }
    f64::NAN
}
pub fn iqsfnz(eccent: f64, q: f64) -> f64 {
    let temp = 1.0 - (1.0 - eccent * eccent) / (2.0 * eccent) * log((1.0 - eccent) / (1.0 + eccent));
    if abs(abs(q) - temp) < 1.0E-6 {
        return if q < 0.0 { -HALF_PI } else { HALF_PI };
    }
    let mut phi = asin(0.5 * q);
    for _ in 0..30 {
        let sin_phi = sin(phi);
        let cos_phi = cos(phi);
        let con = eccent * sin_phi;
        let dphi = pow(1.0 - con * con, 2.0) / (2.0 * cos_phi)
            * (q / (1.0 - eccent * eccent) - sin_phi / (1.0 - con * con) + 0.5 / eccent * log((1.0 - con) / (1.0 + con)));
        phi += dphi;
        if abs(dphi) <= 0.0000000001 {
            return phi;
        }
    }
    f64::NAN
}
pub fn log1py(x: f64) -> f64 {
    let y = 1.0 + x;
    let z = y - 1.0;
    if z == 0.0 { x } else { x * log(y) / z }
}
pub fn asinhy(x: f64) -> f64 {
    let mut y = abs(x);
    y = log1py(y * (1.0 + y / (hypot(1.0, y) + 1.0)));
    if x < 0.0 { -y } else { y }
}
pub fn mlfn(e0: f64, e1: f64, e2: f64, e3: f64, phi: f64) -> f64 {
    e0 * phi - e1 * sin(2.0 * phi) + e2 * sin(4.0 * phi) - e3 * sin(6.0 * phi)
}
pub fn msfnz(eccent: f64, sinphi: f64, cosphi: f64) -> f64 {
    let con = eccent * sinphi;
    cosphi / sqrt(1.0 - con * con)
}
/// `phi2z` (−9999 = no convergence).
pub fn phi2z(eccent: f64, ts: f64) -> f64 {
    let eccnth = 0.5 * eccent;
    let mut phi = HALF_PI - 2.0 * atan(ts);
    for _ in 0..=15 {
        let con = eccent * sin(phi);
        let dphi = HALF_PI - 2.0 * atan(ts * pow((1.0 - con) / (1.0 + con), eccnth)) - phi;
        phi += dphi;
        if abs(dphi) <= 0.0000000001 {
            return phi;
        }
    }
    -9999.0
}
pub fn pj_enfn(es: f64) -> [f64; 5] {
    const C00: f64 = 1.0;
    const C02: f64 = 0.25;
    const C04: f64 = 0.046875;
    const C06: f64 = 0.01953125;
    const C08: f64 = 0.01068115234375;
    const C22: f64 = 0.75;
    const C44: f64 = 0.46875;
    const C46: f64 = 0.01302083333333333333;
    const C48: f64 = 0.00712076822916666666;
    const C66: f64 = 0.36458333333333333333;
    const C68: f64 = 0.00569661458333333333;
    const C88: f64 = 0.3076171875;
    let mut en = [0.0; 5];
    en[0] = C00 - es * (C02 + es * (C04 + es * (C06 + es * C08)));
    en[1] = es * (C22 - es * (C04 + es * (C06 + es * C08)));
    let mut t = es * es;
    en[2] = t * (C44 - es * (C46 + es * C48));
    t *= es;
    en[3] = t * (C66 - es * C68);
    en[4] = t * es * C88;
    en
}
pub fn pj_mlfn(phi: f64, sphi: f64, cphi: f64, en: &[f64; 5]) -> f64 {
    let cphi = cphi * sphi;
    let sphi = sphi * sphi;
    en[0] * phi - cphi * (en[1] + sphi * (en[2] + sphi * (en[3] + sphi * en[4])))
}
pub fn pj_inv_mlfn(arg: f64, es: f64, en: &[f64; 5]) -> f64 {
    let k = 1.0 / (1.0 - es);
    let mut phi = arg;
    for _ in 0..20 {
        let s = sin(phi);
        let mut t = 1.0 - es * s * s;
        t = (pj_mlfn(phi, s, cos(phi), en) - arg) * (t * sqrt(t)) * k;
        phi -= t;
        if abs(t) < EPSLN {
            return phi;
        }
    }
    phi
}
pub fn qsfnz(eccent: f64, sinphi: f64) -> f64 {
    if eccent > 1.0e-7 {
        let con = eccent * sinphi;
        (1.0 - eccent * eccent) * (sinphi / (1.0 - con * con) - (0.5 / eccent) * log((1.0 - con) / (1.0 + con)))
    } else {
        2.0 * sinphi
    }
}
pub fn srat(esinp: f64, ex: f64) -> f64 {
    pow((1.0 - esinp) / (1.0 + esinp), ex)
}
pub fn tsfnz(eccent: f64, phi: f64, sinphi: f64) -> f64 {
    let mut con = eccent * sinphi;
    let com = 0.5 * eccent;
    con = pow((1.0 - con) / (1.0 + con), com);
    tan(0.5 * (HALF_PI - phi)) / con
}

/// vincenty.js inverse (as written, including its sinSigma).
pub fn vincenty_inverse(lat1: f64, lon1: f64, lat2: f64, lon2: f64, a: f64, f: f64) -> (f64, f64) {
    let l = lon2 - lon1;
    let u1 = atan((1.0 - f) * tan(lat1));
    let u2 = atan((1.0 - f) * tan(lat2));
    let (sin_u1, cos_u1) = (sin(u1), cos(u1));
    let (sin_u2, cos_u2) = (sin(u2), cos(u2));
    let mut lambda = l;
    let mut lambda_p;
    let mut iter_limit = 100i32;
    let (mut sin_sigma, mut cos_sigma, mut sigma, mut cos2_alpha, mut cos2_sigma_m);
    let mut sin_lambda;
    let mut cos_lambda;
    loop {
        sin_lambda = sin(lambda);
        cos_lambda = cos(lambda);
        sin_sigma = sqrt(
            (cos_u2 * sin_lambda) * (cos_u2 * sin_lambda)
                + (cos_u1 * sin_u2 - sin_u1 * cos_u2 * cos_lambda) * (cos_u1 * sin_u2 - sin_u1 * cos_u2 * cos_lambda),
        );
        if sin_sigma == 0.0 {
            return (0.0, 0.0);
        }
        cos_sigma = sin_u1 * sin_u2 + cos_u1 * cos_u2 * cos_lambda;
        sigma = crate::math::atan2(sin_sigma, cos_sigma);
        let sin_alpha = cos_u1 * cos_u2 * sin_lambda / sin_sigma;
        cos2_alpha = 1.0 - sin_alpha * sin_alpha;
        cos2_sigma_m = if cos2_alpha != 0.0 { cos_sigma - 2.0 * sin_u1 * sin_u2 / cos2_alpha } else { 0.0 };
        let c = f / 16.0 * cos2_alpha * (4.0 + f * (4.0 - 3.0 * cos2_alpha));
        lambda_p = lambda;
        lambda = l
            + (1.0 - c)
                * f
                * sin_alpha
                * (sigma + c * sin_sigma * (cos2_sigma_m + c * cos_sigma * (-1.0 + 2.0 * cos2_sigma_m * cos2_sigma_m)));
        if !(abs(lambda - lambda_p) > 1e-12 && {
            iter_limit -= 1;
            iter_limit > 0
        }) {
            break;
        }
    }
    if iter_limit == 0 {
        return (f64::NAN, f64::NAN);
    }
    let b = a * (1.0 - f);
    let u_sq = cos2_alpha * (a * a - b * b) / (b * b);
    let aa = 1.0 + u_sq / 16384.0 * (4096.0 + u_sq * (-768.0 + u_sq * (320.0 - 175.0 * u_sq)));
    let bb = u_sq / 1024.0 * (256.0 + u_sq * (-128.0 + u_sq * (74.0 - 47.0 * u_sq)));
    let delta_sigma = bb
        * sin_sigma
        * (cos2_sigma_m
            + bb / 4.0
                * (cos_sigma * (-1.0 + 2.0 * cos2_sigma_m * cos2_sigma_m)
                    - bb / 6.0 * cos2_sigma_m * (-3.0 + 4.0 * sin_sigma * sin_sigma) * (-3.0 + 4.0 * cos2_sigma_m * cos2_sigma_m)));
    let s = (a * (1.0 - f)) * aa * (sigma - delta_sigma);
    let azi1 = crate::math::atan2(cos_u2 * sin_lambda, cos_u1 * sin_u2 - sin_u1 * cos_u2 * cos_lambda);
    (azi1, s)
}

/// vincenty.js direct → (lat2, lon2).
pub fn vincenty_direct(lat1: f64, lon1: f64, azi1: f64, s12: f64, a: f64, f: f64) -> (f64, f64) {
    let u1 = atan((1.0 - f) * tan(lat1));
    let (sin_u1, cos_u1) = (sin(u1), cos(u1));
    let (sin_alpha1, cos_alpha1) = (sin(azi1), cos(azi1));
    let sigma1 = crate::math::atan2(sin_u1, cos_u1 * cos_alpha1);
    let sin_alpha = cos_u1 * sin_alpha1;
    let cos2_alpha = 1.0 - sin_alpha * sin_alpha;
    let b = a * (1.0 - f);
    let u_sq = cos2_alpha * (a * a - b * b) / (b * b);
    let aa = 1.0 + u_sq / 16384.0 * (4096.0 + u_sq * (-768.0 + u_sq * (320.0 - 175.0 * u_sq)));
    let bb = u_sq / 1024.0 * (256.0 + u_sq * (-128.0 + u_sq * (74.0 - 47.0 * u_sq)));
    let mut sigma = s12 / ((a * (1.0 - f)) * aa);
    let mut iter_limit = 100i32;
    let (mut cos2_sigma_m, mut sin_sigma, mut cos_sigma);
    loop {
        cos2_sigma_m = cos(2.0 * sigma1 + sigma);
        sin_sigma = sin(sigma);
        cos_sigma = cos(sigma);
        let delta_sigma = bb
            * sin_sigma
            * (cos2_sigma_m
                + bb / 4.0
                    * (cos_sigma * (-1.0 + 2.0 * cos2_sigma_m * cos2_sigma_m)
                        - bb / 6.0 * cos2_sigma_m * (-3.0 + 4.0 * sin_sigma * sin_sigma) * (-3.0 + 4.0 * cos2_sigma_m * cos2_sigma_m)));
        let sigma_p = sigma;
        sigma = s12 / ((a * (1.0 - f)) * aa) + delta_sigma;
        if !(abs(sigma - sigma_p) > 1e-12 && {
            iter_limit -= 1;
            iter_limit > 0
        }) {
            break;
        }
    }
    if iter_limit == 0 {
        return (f64::NAN, f64::NAN);
    }
    let tmp = sin_u1 * sin_sigma - cos_u1 * cos_sigma * cos_alpha1;
    let lat2 =
        crate::math::atan2(sin_u1 * cos_sigma + cos_u1 * sin_sigma * cos_alpha1, (1.0 - f) * sqrt(sin_alpha * sin_alpha + tmp * tmp));
    let lambda = crate::math::atan2(sin_sigma * sin_alpha1, cos_u1 * cos_sigma - sin_u1 * sin_sigma * cos_alpha1);
    let c = f / 16.0 * cos2_alpha * (4.0 + f * (4.0 - 3.0 * cos2_alpha));
    let l = lambda
        - (1.0 - c) * f * sin_alpha * (sigma + c * sin_sigma * (cos2_sigma_m + c * cos_sigma * (-1.0 + 2.0 * cos2_sigma_m * cos2_sigma_m)));
    (lat2, lon1 + l)
}

/// fL.js
pub fn f_l(x: f64, l: f64) -> f64 {
    2.0 * atan(x * exp(l)) - HALF_PI
}
/// latiso.js
pub fn latiso(eccent: f64, phi: f64, sinphi: f64) -> f64 {
    if abs(phi) > HALF_PI {
        return f64::NAN;
    }
    if phi == HALF_PI {
        return f64::INFINITY;
    }
    if phi == -HALF_PI {
        return f64::NEG_INFINITY;
    }
    let con = eccent * sinphi;
    log(tan((HALF_PI + phi) / 2.0)) + eccent * log((1.0 - con) / (1.0 + con)) / 2.0
}
/// invlatiso.js (iteration bounded; the JS loops until it converges)
pub fn invlatiso(eccent: f64, ts: f64) -> f64 {
    let mut phi = f_l(1.0, ts);
    let mut guard = 0u32;
    loop {
        let iphi = phi;
        let con = eccent * sin(iphi);
        phi = f_l(exp(eccent * log((1.0 + con) / (1.0 - con)) / 2.0), ts);
        guard = guard.saturating_add(1);
        if !(abs(phi - iphi) > 1.0e-12) || guard > 10_000 {
            break;
        }
    }
    phi
}
