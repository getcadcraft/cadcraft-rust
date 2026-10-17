//! drive.js — driving along an alignment in the 3D view, and where the sun is (0.4 V10; NOAA / Meeus).

use crate::alignment::Alignment;
use crate::js::{J, JsMath, max, min, pow};
use crate::profile::Profile;
use serde_json::Value;
use std::f64::consts::PI;

/// `EYE_HEIGHT[units]`.
pub fn eye_height(units: &str) -> f64 {
    if units == "m" { 1.07 } else { 3.5 }
}
/// `DRIVE_SPEEDS[units]` → (label, perSec).
pub fn drive_speeds(units: &str) -> Vec<(String, f64)> {
    if units == "m" {
        [15.0, 40.0, 70.0, 100.0].iter().map(|k| (format!("{} km/h", crate::js::num_to_string(*k)), (k * 1000.0) / 3600.0)).collect()
    } else {
        [10.0, 25.0, 45.0, 65.0].iter().map(|m| (format!("{} mph", crate::js::num_to_string(*m)), (m * 5280.0) / 3600.0)).collect()
    }
}

/// `elevationAlong(al, profile, ground)` → a stateful (sta) → z | None (it keeps the last good z).
pub fn elevation_along<'a>(
    al: &'a Alignment,
    profile: Option<&'a Profile>,
    ground: Option<&'a dyn Fn(f64, f64) -> Option<f64>>,
) -> impl FnMut(f64) -> Option<f64> + 'a {
    let use_prof = profile.is_some_and(Profile::ok);
    let mut last: Option<f64> = None;
    move |sta: f64| {
        let mut z = if use_prof { profile.and_then(|p| p.elevation_at(sta)) } else { None };
        if !z.is_some_and(f64::is_finite) {
            z = match (al.point_at(sta), ground) {
                (Some(p), Some(g)) => g(p.x, p.y),
                _ => None,
            };
        }
        if let Some(v) = z.filter(|v| v.is_finite()) {
            last = Some(v);
        }
        last
    }
}

/// `driveFrame(…)` → { eye, target, z, sta }.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub eye: [f64; 3],
    pub target: [f64; 3],
    pub z: f64,
    pub sta: f64,
}
impl J for Frame {
    fn j(&self) -> Value {
        crate::jo! { "eye" => self.eye, "target" => self.target, "z" => self.z, "sta" => self.sta }
    }
}
/// `driveFrame(al, elevAt, sta, { eye, ahead, origin, exag })`.
pub fn drive_frame(
    al: &Alignment,
    elev_at: &mut dyn FnMut(f64) -> Option<f64>,
    sta: f64,
    eye: f64,
    ahead: f64,
    origin: [f64; 3],
    exag: f64,
) -> Option<Frame> {
    let s = max(al.start, min(al.end, sta));
    let p = al.point_at(s)?;
    let z = elev_at(s).unwrap_or(origin[2]);
    let l = |x: f64, y: f64, zz: f64| [x - origin[0], y - origin[1], (zz - origin[2]) * exag];
    let s2 = min(al.end, s + ahead);
    let (qx, qy, z2);
    if s2 - s > ahead * 0.25 {
        let q = al.point_at(s2)?;
        qx = q.x;
        qy = q.y;
        z2 = elev_at(s2).unwrap_or(z);
    } else {
        qx = p.x + ahead * p.dir.jcos();
        qy = p.y + ahead * p.dir.jsin();
        z2 = z;
    }
    Some(Frame { eye: l(p.x, p.y, z + eye), target: l(qx, qy, z2 + eye), z, sta: s })
}

const R: f64 = PI / 180.0;

/// `sunPosition(ms, lat, lon)` → (azimuth, altitude, declination, eot).
pub fn sun_position(ms: f64, lat: f64, lon: f64) -> [f64; 4] {
    let jd = ms / 86_400_000.0 + 2_440_587.5;
    let t = (jd - 2_451_545.0) / 36525.0;
    let l0 = (((280.46646 + t * (36000.76983 + t * 0.0003032)) % 360.0) + 360.0) % 360.0;
    let m = 357.52911 + t * (35999.05029 - 0.0001537 * t);
    let e = 0.016708634 - t * (0.000042037 + 0.0000001267 * t);
    let c = (m * R).jsin() * (1.914602 - t * (0.004817 + 0.000014 * t))
        + (2.0 * m * R).jsin() * (0.019993 - 0.000101 * t)
        + (3.0 * m * R).jsin() * 0.000289;
    let omega = 125.04 - 1934.136 * t;
    let lambda = l0 + c - 0.00569 - 0.00478 * (omega * R).jsin();
    let eps0 = 23.0 + (26.0 + (21.448 - t * (46.815 + t * (0.00059 - t * 0.001813))) / 60.0) / 60.0;
    let eps = eps0 + 0.00256 * (omega * R).jcos();
    let decl = libm::asin((eps * R).jsin() * (lambda * R).jsin()) / R;
    let y = pow(((eps / 2.0) * R).jtan(), 2.0);
    let eot = (4.0 / R)
        * (y * (2.0 * l0 * R).jsin() - 2.0 * e * (m * R).jsin() + 4.0 * e * y * (m * R).jsin() * (2.0 * l0 * R).jcos()
            - 0.5 * y * y * (4.0 * l0 * R).jsin()
            - 1.25 * e * e * (2.0 * m * R).jsin());
    // the time of day (UTC) in minutes, from the ms (as Date's getUTC* parts)
    let day_ms = ms.trunc().rem_euclid(86_400_000.0);
    let hours = (day_ms / 3_600_000.0).floor();
    let mins = ((day_ms - hours * 3_600_000.0) / 60_000.0).floor();
    let secs = ((day_ms - hours * 3_600_000.0 - mins * 60_000.0) / 1000.0).floor();
    let msp = day_ms - hours * 3_600_000.0 - mins * 60_000.0 - secs * 1000.0;
    let minutes = hours * 60.0 + mins + secs / 60.0 + msp / 60000.0;
    let mut tst = (minutes + eot + 4.0 * lon) % 1440.0;
    if tst < 0.0 {
        tst += 1440.0;
    }
    let ha = if tst / 4.0 < 0.0 { tst / 4.0 + 180.0 } else { tst / 4.0 - 180.0 };
    let cos_z = (lat * R).jsin() * (decl * R).jsin() + (lat * R).jcos() * (decl * R).jcos() * (ha * R).jcos();
    let zen = max(-1.0, min(1.0, cos_z)).jacos() / R;
    let elev = 90.0 - zen;
    let sz = (zen * R).jsin();
    let az = if sz.abs() < 1e-9 {
        if lat > decl { 180.0 } else { 0.0 }
    } else {
        let cc = ((lat * R).jsin() * (zen * R).jcos() - (decl * R).jsin()) / ((lat * R).jcos() * sz);
        let a = max(-1.0, min(1.0, cc)).jacos() / R;
        if ha > 0.0 { (a + 180.0) % 360.0 } else { (540.0 - a) % 360.0 }
    };
    [az, elev + refraction(elev), decl, eot]
}
/// NOAA's approximation of atmospheric refraction (degrees).
fn refraction(h: f64) -> f64 {
    if h > 85.0 {
        return 0.0;
    }
    let te = (h * R).jtan();
    let r = if h > 5.0 {
        58.1 / te - 0.07 / pow(te, 3.0) + 0.000086 / pow(te, 5.0)
    } else if h > -0.575 {
        1735.0 + h * (-518.2 + h * (103.4 + h * (-12.79 + h * 0.711)))
    } else {
        -20.774 / te
    };
    r / 3600.0
}
/// `sunVector({ azimuth, altitude })` → [east, north, up].
pub fn sun_vector(azimuth: f64, altitude: f64) -> [f64; 3] {
    let a = azimuth * R;
    let h = altitude * R;
    [h.jcos() * a.jsin(), h.jcos() * a.jcos(), h.jsin()]
}
