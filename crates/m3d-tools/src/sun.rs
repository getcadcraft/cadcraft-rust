//! Where the sun is, for shadows (sun.js, one for one): NOAA's general solar position equations (fractional year,
//! equation of time and declination from Spencer's Fourier series, true solar time, zenith and azimuth). The model's
//! north is +Y (green), east +X (red).

use cc_m3d::jsmath::{fd_cos, fd_sin};

use crate::js::{jmax, jmin};

const RAD: f64 = std::f64::consts::PI / 180.0;

fn leap(year: f64) -> bool {
    (year % 4.0 == 0.0 && year % 100.0 != 0.0) || year % 400.0 == 0.0
}

/// `dayOfYear(month, day, year)` (month 1–12; another month reads `cum[month − 1]` as undefined → NaN).
pub fn day_of_year(month: f64, day: f64, year: f64) -> f64 {
    const CUM: [f64; 12] = [0.0, 31.0, 59.0, 90.0, 120.0, 151.0, 181.0, 212.0, 243.0, 273.0, 304.0, 334.0];
    let c = if month.fract() == 0.0 && (1.0..=12.0).contains(&month) {
        let i = month as usize - 1;
        CUM.get(i).copied().unwrap_or(f64::NAN)
    } else {
        f64::NAN
    };
    c + day + if leap(year) && month > 2.0 { 1.0 } else { 0.0 }
}

/// The sun at a date, hour, place and time zone.
#[derive(Clone, Debug, PartialEq)]
pub struct Sun {
    pub altitude: f64,
    /// degrees from north, clockwise
    pub azimuth: f64,
    pub declination: f64,
    pub eqtime: f64,
    /// unit vector toward the sun in model space
    pub dir: [f64; 3],
    pub up: bool,
}

/// The inputs (`sunPosition`'s defaults: 21 June, noon, 40° N 105° W, UTC−7, 2026).
#[derive(Clone, Copy, Debug)]
pub struct When {
    pub month: f64,
    pub day: f64,
    pub hour: f64,
    pub lat: f64,
    pub lon: f64,
    pub tz: f64,
    pub year: f64,
}
impl Default for When {
    fn default() -> Self {
        When { month: 6.0, day: 21.0, hour: 12.0, lat: 40.0, lon: -105.0, tz: -7.0, year: 2026.0 }
    }
}

#[allow(clippy::many_single_char_names)]
pub fn sun_position(w: &When) -> Sun {
    let When { month, day, hour, lat, lon, tz, year } = *w;
    let n = day_of_year(month, day, year);
    let g = (2.0 * std::f64::consts::PI / if leap(year) { 366.0 } else { 365.0 }) * (n - 1.0 + (hour - 12.0) / 24.0);
    let eqtime =
        229.18 * (0.000075 + 0.001868 * fd_cos(g) - 0.032077 * fd_sin(g) - 0.014615 * fd_cos(2.0 * g) - 0.040849 * fd_sin(2.0 * g));
    let decl = 0.006918 - 0.399912 * fd_cos(g) + 0.070257 * fd_sin(g) - 0.006758 * fd_cos(2.0 * g) + 0.000907 * fd_sin(2.0 * g)
        - 0.002697 * fd_cos(3.0 * g)
        + 0.00148 * fd_sin(3.0 * g);
    let offset = eqtime + 4.0 * lon - 60.0 * tz;
    let tst = hour * 60.0 + offset;
    let ha = (tst / 4.0 - 180.0) * RAD;
    let phi = lat * RAD;
    let cos_z = fd_sin(phi) * fd_sin(decl) + fd_cos(phi) * fd_cos(decl) * fd_cos(ha);
    let zen = libm::acos(jmax(-1.0, jmin(1.0, cos_z)));
    let altitude = 90.0 - zen / RAD;
    let az = libm::atan2(fd_sin(ha), fd_cos(ha) * fd_sin(phi) - libm::tan(decl) * fd_cos(phi)) / RAD + 180.0;
    let azimuth = ((az % 360.0) + 360.0) % 360.0;
    let alt = altitude * RAD;
    let a = azimuth * RAD;
    Sun {
        altitude,
        azimuth,
        declination: decl / RAD,
        eqtime,
        dir: [fd_sin(a) * fd_cos(alt), fd_cos(a) * fd_cos(alt), fd_sin(alt)],
        up: altitude > 0.0,
    }
}
