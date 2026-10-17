//! proj4js `transform.js`, `datum_transform.js`, `datumUtils.js`, `adjust_axis.js`, `checkSanity.js` and `core.js`:
//! moving a point from one system to another.

use crate::common::*;
use crate::math::{abs, atan, atan2, cos, sin, sqrt};
use crate::nadgrid::Subgrid;
use crate::proj::{Datum, Out, Proj, Pt, wgs84};
use crate::val::Val;
use std::sync::Arc;

type R<T> = Result<T, String>;

pub fn compare_datums(s: &Datum, d: &Datum) -> bool {
    if s.datum_type != d.datum_type {
        return false;
    }
    if s.a != d.a || abs(s.es - d.es) > 0.000000000050 {
        return false;
    }
    let g = |v: &Vec<f64>, i: usize| v.get(i).copied().unwrap_or(f64::NAN);
    if s.datum_type == PJD_3PARAM {
        return (0..3).all(|i| g(&s.datum_params, i) == g(&d.datum_params, i));
    }
    if s.datum_type == PJD_7PARAM {
        return (0..7).all(|i| g(&s.datum_params, i) == g(&d.datum_params, i));
    }
    true
}

pub fn geodetic_to_geocentric(p: &Pt, es: f64, a: f64) -> Pt {
    let mut lon = p.x;
    let mut lat = p.y;
    let height = match p.z {
        Some(z) if z != 0.0 && !z.is_nan() => z,
        _ => 0.0,
    };
    if lat < -HALF_PI && lat > -1.001 * HALF_PI {
        lat = -HALF_PI;
    } else if lat > HALF_PI && lat < 1.001 * HALF_PI {
        lat = HALF_PI;
    } else if lat < -HALF_PI {
        return Pt { x: f64::NEG_INFINITY, y: f64::NEG_INFINITY, z: p.z, m: None };
    } else if lat > HALF_PI {
        return Pt { x: f64::INFINITY, y: f64::INFINITY, z: p.z, m: None };
    }
    if lon > PI {
        lon -= 2.0 * PI;
    }
    let sin_lat = sin(lat);
    let cos_lat = cos(lat);
    let sin2 = sin_lat * sin_lat;
    let rn = a / sqrt(1.0e0 - es * sin2);
    Pt {
        x: (rn + height) * cos_lat * cos(lon),
        y: (rn + height) * cos_lat * sin(lon),
        z: Some(((rn * (1.0 - es)) + height) * sin_lat),
        m: None,
    }
}

pub fn geocentric_to_geodetic(p: &Pt, es: f64, a: f64, b: f64) -> Pt {
    let genau = 1e-12;
    let genau2 = genau * genau;
    let maxiter = 30;
    let x = p.x;
    let y = p.y;
    let z = match p.z {
        Some(z) if z != 0.0 && !z.is_nan() => z,
        _ => 0.0,
    };
    let pp = sqrt(x * x + y * y);
    let rr = sqrt(x * x + y * y + z * z);
    let lon;
    if pp / a < genau {
        lon = 0.0;
        if rr / a < genau {
            let _ = b;
            return Pt { x: p.x, y: p.y, z: p.z, m: None };
        }
    } else {
        lon = atan2(y, x);
    }
    let ct = z / rr;
    let st = pp / rr;
    let mut rx = 1.0 / sqrt(1.0 - es * (2.0 - es) * st * st);
    let mut cphi0 = st * (1.0 - es) * rx;
    let mut sphi0 = ct * rx;
    let mut iter = 0;
    let (mut height, mut cphi, mut sphi);
    loop {
        iter += 1;
        let rn = a / sqrt(1.0 - es * sphi0 * sphi0);
        height = pp * cphi0 + z * sphi0 - rn * (1.0 - es * sphi0 * sphi0);
        let rk = es * rn / (rn + height);
        rx = 1.0 / sqrt(1.0 - rk * (2.0 - rk) * st * st);
        cphi = st * (1.0 - rk) * rx;
        sphi = ct * rx;
        let sdphi = sphi * cphi0 - cphi * sphi0;
        cphi0 = cphi;
        sphi0 = sphi;
        if !(sdphi * sdphi > genau2 && iter < maxiter) {
            break;
        }
    }
    let lat = atan(sphi / abs(cphi));
    Pt { x: lon, y: lat, z: Some(height), m: None }
}

pub fn geocentric_to_wgs84(p: &Pt, ty: i32, dp: &[f64]) -> Option<Pt> {
    let g = |i: usize| dp.get(i).copied().unwrap_or(f64::NAN);
    let z = p.z.unwrap_or(f64::NAN);
    if ty == PJD_3PARAM {
        return Some(Pt { x: p.x + g(0), y: p.y + g(1), z: Some(z + g(2)), m: None });
    }
    if ty == PJD_7PARAM {
        let (dx, dy, dz, rx, ry, rz, m) = (g(0), g(1), g(2), g(3), g(4), g(5), g(6));
        return Some(Pt {
            x: m * (p.x - rz * p.y + ry * z) + dx,
            y: m * (rz * p.x + p.y - rx * z) + dy,
            z: Some(m * (-ry * p.x + rx * p.y + z) + dz),
            m: None,
        });
    }
    None
}

pub fn geocentric_from_wgs84(p: &Pt, ty: i32, dp: &[f64]) -> Option<Pt> {
    let g = |i: usize| dp.get(i).copied().unwrap_or(f64::NAN);
    let z = p.z.unwrap_or(f64::NAN);
    if ty == PJD_3PARAM {
        return Some(Pt { x: p.x - g(0), y: p.y - g(1), z: Some(z - g(2)), m: None });
    }
    if ty == PJD_7PARAM {
        let (dx, dy, dz, rx, ry, rz, m) = (g(0), g(1), g(2), g(3), g(4), g(5), g(6));
        let xt = (p.x - dx) / m;
        let yt = (p.y - dy) / m;
        let zt = (z - dz) / m;
        return Some(Pt { x: xt + rz * yt - ry * zt, y: -rz * xt + yt + rx * zt, z: Some(ry * xt - rx * yt + zt), m: None });
    }
    None
}

fn check_params(t: i32) -> bool {
    t == PJD_3PARAM || t == PJD_7PARAM
}

/// A point that went through `undefined` (geocentricToWgs84 for a datum type it does not handle): the JS then reads
/// `.x` of undefined.
const UNDEF_READ: &str = "Cannot read properties of undefined (reading 'x')";

/// `datum_transform(source, dest, point)` → None = undefined.
pub fn datum_transform(source: &Datum, dest: &Datum, mut point: Pt) -> R<Option<Pt>> {
    if compare_datums(source, dest) {
        return Ok(Some(point));
    }
    if source.datum_type == PJD_NODATUM || dest.datum_type == PJD_NODATUM {
        return Ok(Some(point));
    }
    let mut source_a = source.a;
    let mut source_es = source.es;
    if source.datum_type == PJD_GRIDSHIFT {
        if apply_grid_shift(source, false, &mut point)? != 0 {
            return Ok(None);
        }
        source_a = SRS_WGS84_SEMIMAJOR;
        source_es = SRS_WGS84_ESQUARED;
    }
    let mut dest_a = dest.a;
    let mut dest_b = dest.b;
    let mut dest_es = dest.es;
    if dest.datum_type == PJD_GRIDSHIFT {
        dest_a = SRS_WGS84_SEMIMAJOR;
        dest_b = SRS_WGS84_SEMIMINOR;
        dest_es = SRS_WGS84_ESQUARED;
    }
    if source_es == dest_es && source_a == dest_a && !check_params(source.datum_type) && !check_params(dest.datum_type) {
        return Ok(Some(point));
    }
    point = geodetic_to_geocentric(&point, source_es, source_a);
    if check_params(source.datum_type) {
        point = geocentric_to_wgs84(&point, source.datum_type, &source.datum_params).ok_or(UNDEF_READ)?;
    }
    if check_params(dest.datum_type) {
        point = geocentric_from_wgs84(&point, dest.datum_type, &dest.datum_params).ok_or(UNDEF_READ)?;
    }
    point = geocentric_to_geodetic(&point, dest_es, dest_a, dest_b);
    if dest.datum_type == PJD_GRIDSHIFT && apply_grid_shift(dest, true, &mut point)? != 0 {
        return Ok(None);
    }
    Ok(Some(point))
}

/// `applyGridShift(source, inverse, point)` → 0 or −1.
pub fn apply_grid_shift(source: &Datum, inverse: bool, point: &mut Pt) -> R<i32> {
    let Some(grids) = source.grids.as_ref().filter(|g| !g.is_empty()) else { return Ok(-1) };
    let input = (-point.x, point.y);
    let mut output = (f64::NAN, f64::NAN);
    'outer: for g in grids {
        let Some(grid) = g else { return Err("Cannot read properties of null (reading 'name')".into()) };
        if grid.is_null {
            output = input;
            break;
        }
        let Some(ng) = &grid.grid else {
            if grid.mandatory {
                return Ok(-1);
            }
            continue;
        };
        for sg in &ng.subgrids {
            let epsilon = (abs(sg.del[1]) + abs(sg.del[0])) / 10000.0;
            let min_x = sg.ll[0] - epsilon;
            let min_y = sg.ll[1] - epsilon;
            let max_x = sg.ll[0] + (sg.lim[0] - 1.0) * sg.del[0] + epsilon;
            let max_y = sg.ll[1] + (sg.lim[1] - 1.0) * sg.del[1] + epsilon;
            if min_y > input.1 || min_x > input.0 || max_y < input.1 || max_x < input.0 {
                continue;
            }
            output = apply_subgrid_shift(input, inverse, sg)?;
            if !output.0.is_nan() {
                break 'outer;
            }
        }
    }
    if output.0.is_nan() {
        return Ok(-1);
    }
    point.x = -output.0;
    point.y = output.1;
    Ok(0)
}

fn apply_subgrid_shift(pin: (f64, f64), inverse: bool, ct: &Subgrid) -> R<(f64, f64)> {
    let nan = (f64::NAN, f64::NAN);
    if pin.0.is_nan() {
        return Ok(nan);
    }
    let mut tb = (pin.0 - ct.ll[0], pin.1 - ct.ll[1]);
    tb.0 = adjust_lon(tb.0 - PI, false) + PI;
    let mut t = nad_interpolate(tb, ct)?;
    if inverse {
        if t.0.is_nan() {
            return Ok(nan);
        }
        t.0 = tb.0 - t.0;
        t.1 = tb.1 - t.1;
        let mut i: i32 = 9;
        let tol = 1e-12;
        loop {
            let del = nad_interpolate(t, ct)?;
            if del.0.is_nan() {
                break;
            }
            let dif = (tb.0 - (del.0 + t.0), tb.1 - (del.1 + t.1));
            t.0 += dif.0;
            t.1 += dif.1;
            let cont = i != 0 && abs(dif.0) > tol && abs(dif.1) > tol;
            i -= 1;
            if !cont {
                break;
            }
        }
        if i < 0 {
            return Ok(nan);
        }
        return Ok((adjust_lon(t.0 + ct.ll[0], false), t.1 + ct.ll[1]));
    }
    if !t.0.is_nan() {
        return Ok((pin.0 + t.0, pin.1 + t.1));
    }
    Ok(nan)
}

fn nad_interpolate(pin: (f64, f64), ct: &Subgrid) -> R<(f64, f64)> {
    let t = (pin.0 / ct.del[0], pin.1 / ct.del[1]);
    let indx = (t.0.floor(), t.1.floor());
    let frct = (t.0 - 1.0 * indx.0, t.1 - 1.0 * indx.1);
    let nan = (f64::NAN, f64::NAN);
    if indx.0 < 0.0 || indx.0 >= ct.lim[0] {
        return Ok(nan);
    }
    if indx.1 < 0.0 || indx.1 >= ct.lim[1] {
        return Ok(nan);
    }
    let at = |i: f64| -> R<[f64; 2]> {
        crate::tiff::idx(i)
            .and_then(|k| ct.cvs.get(k))
            .copied()
            .ok_or_else(|| "Cannot read properties of undefined (reading '0')".to_string())
    };
    let mut inx = indx.1 * ct.lim[0] + indx.0;
    let f00 = at(inx)?;
    inx += 1.0;
    let f10 = at(inx)?;
    inx += ct.lim[0];
    let f11 = at(inx)?;
    inx -= 1.0;
    let f01 = at(inx)?;
    let m11 = frct.0 * frct.1;
    let m10 = frct.0 * (1.0 - frct.1);
    let m00 = (1.0 - frct.0) * (1.0 - frct.1);
    let m01 = (1.0 - frct.0) * frct.1;
    Ok((m00 * f00[0] + m10 * f10[0] + m01 * f01[0] + m11 * f11[0], m00 * f00[1] + m10 * f10[1] + m01 * f01[1] + m11 * f11[1]))
}

fn axis_chars(p: &Proj) -> Vec<char> {
    p.axis.to_js_string().chars().collect()
}

fn adjust_axis_to_enu(crs: &Proj, point: &Pt) -> Option<Pt> {
    let mut out = Pt { x: f64::NAN, y: f64::NAN, z: None, m: None };
    let (mut hx, mut hy) = (false, false);
    for (i, c) in axis_chars(crs).into_iter().enumerate() {
        if i == 2 && point.z.is_none() {
            continue;
        }
        let v = match i {
            0 => Some(point.x),
            1 => Some(point.y),
            2 => point.z,
            _ => None,
        };
        let vv = v.unwrap_or(f64::NAN);
        match c {
            'e' => {
                out.x = vv;
                hx = true;
            }
            'w' => {
                out.x = -vv;
                hx = true;
            }
            'n' => {
                out.y = vv;
                hy = true;
            }
            's' => {
                out.y = -vv;
                hy = true;
            }
            'u' => out.z = Some(vv),
            'd' => out.z = Some(-vv),
            _ => return None,
        }
    }
    let _ = (hx, hy);
    Some(out)
}

fn adjust_axis_from_enu(crs: &Proj, point: &Pt) -> Option<Pt> {
    let mut vals: [Option<f64>; 3] = [None, None, None];
    for (i, c) in axis_chars(crs).into_iter().enumerate() {
        if i == 2 && point.z.is_none() {
            continue;
        }
        let z = point.z.unwrap_or(f64::NAN);
        let v = match c {
            'e' => point.x,
            'w' => -point.x,
            'n' => point.y,
            's' => -point.y,
            'u' => z,
            'd' => -z,
            _ => return None,
        };
        if let Some(slot) = vals.get_mut(i) {
            *slot = Some(v);
        }
    }
    Some(Pt { x: vals[0].unwrap_or(f64::NAN), y: vals[1].unwrap_or(f64::NAN), z: vals[2], m: None })
}

fn check_not_wgs(s: &Proj, d: &Proj) -> bool {
    let shifted = |t: i32| t == PJD_3PARAM || t == PJD_7PARAM || t == PJD_GRIDSHIFT;
    let w = Val::str("WGS84");
    (shifted(s.datum.datum_type) && d.datum_code != w) || (shifted(d.datum.datum_type) && s.datum_code != w)
}

/// What transformInternal returns: a point, `undefined`, or a bare number (a forward's error code).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TOut {
    P(Pt),
    Undef,
    Null,
    Num(f64),
}

/// `transformInternal(source, dest, point, enforceAxis)`.
pub fn transform_internal(source: &Proj, dest: &Proj, mut point: Pt, enforce_axis: bool) -> R<TOut> {
    let has_z = point.z.is_some();
    if !point.x.is_finite() || !point.y.is_finite() {
        return Err("coordinates must be finite numbers".into());
    }
    let w;
    let mut source = source;
    if check_not_wgs(source, dest) {
        w = wgs84();
        match transform_internal(source, &w, point, enforce_axis)? {
            TOut::P(p) => point = p,
            TOut::Undef | TOut::Null => return Err("Cannot read properties of undefined (reading 'x')".into()),
            TOut::Num(_) => return Err("Cannot read properties of undefined (reading 'x')".into()),
        }
        source = &w;
    }
    if enforce_axis && source.axis != Val::str("enu") {
        match adjust_axis_to_enu(source, &point) {
            Some(p) => point = p,
            None => return Err("Cannot read properties of null (reading 'x')".into()),
        }
    }
    let zor = |z: Option<f64>| match z {
        Some(v) if v != 0.0 && !v.is_nan() => Some(v),
        _ => Some(0.0),
    };
    if source.is_longlat() {
        point = Pt { x: point.x * D2R, y: point.y * D2R, z: zor(point.z), m: None };
    } else {
        if let Some(tm) = source.to_meter_t() {
            point = Pt { x: point.x * tm, y: point.y * tm, z: zor(point.z), m: None };
        }
        match source.inverse(point)? {
            Out::P(p) => point = p,
            Out::Null | Out::Undef => return Ok(TOut::Undef),
            Out::Num(nv) => {
                if nv == 0.0 || nv.is_nan() {
                    return Ok(TOut::Undef);
                }
                return Err(format!("Cannot create property 'x' on number '{}'", cc_cad::js::num_to_string(nv)));
            }
        }
    }
    if let Some(fg) = source.from_greenwich_t() {
        point.x += fg;
    }
    match datum_transform(&source.datum, &dest.datum, point)? {
        Some(p) => point = p,
        None => return Ok(TOut::Undef),
    }
    if let Some(fg) = dest.from_greenwich_t() {
        point = Pt { x: point.x - fg, y: point.y, z: zor(point.z), m: None };
    }
    if dest.is_longlat() {
        if let Some(lw) = dest.long_wrap {
            point.x = lw + adjust_lon(point.x - lw, false);
        }
        point = Pt { x: point.x * R2D, y: point.y * R2D, z: zor(point.z), m: None };
    } else {
        match dest.forward(point)? {
            Out::P(p) => point = p,
            Out::Null => {
                if dest.to_meter_t().is_some() {
                    return Err("Cannot read properties of null (reading 'x')".into());
                }
                return Ok(TOut::Null);
            }
            Out::Undef => {
                if dest.to_meter_t().is_some() {
                    return Err("Cannot read properties of undefined (reading 'x')".into());
                }
                return Ok(TOut::Undef);
            }
            Out::Num(nv) => {
                if dest.to_meter_t().is_some() {
                    point = Pt { x: f64::NAN, y: f64::NAN, z: Some(0.0), m: None };
                } else {
                    return Ok(TOut::Num(nv));
                }
            }
        }
        if let Some(tm) = dest.to_meter_t() {
            point = Pt { x: point.x / tm, y: point.y / tm, z: zor(point.z), m: None };
        }
    }
    if enforce_axis && dest.axis != Val::str("enu") {
        return Ok(match adjust_axis_from_enu(dest, &point) {
            Some(p) => TOut::P(p),
            None => TOut::Null,
        });
    }
    if !has_z && dest.proj_name != Val::str("geocent") {
        point.z = None;
    }
    Ok(TOut::P(point))
}

/// `proj4(from, to)`: a converter.
#[derive(Clone, Debug)]
pub struct Converter {
    pub from: Arc<Proj>,
    pub to: Arc<Proj>,
}

/// One coordinate of an output array: a number or JS `undefined`.
pub type JsNum = Option<f64>;

impl Converter {
    pub fn new(from: &str, to: &str) -> R<Converter> {
        Ok(Converter { from: Arc::new(Proj::new(from)?), to: Arc::new(Proj::new(to)?) })
    }
    pub fn from_projs(from: Arc<Proj>, to: Arc<Proj>) -> Converter {
        Converter { from, to }
    }
    /// `converter.forward([x, y])`.
    pub fn forward(&self, x: f64, y: f64) -> R<[JsNum; 2]> {
        transformer(&self.from, &self.to, x, y)
    }
    /// `converter.inverse([x, y])`.
    pub fn inverse(&self, x: f64, y: f64) -> R<[JsNum; 2]> {
        transformer(&self.to, &self.from, x, y)
    }
}

/// `transformer(from, to, [x, y])` → [x, y] (undefined when a forward returned a bare number).
pub fn transformer(from: &Proj, to: &Proj, x: f64, y: f64) -> R<[JsNum; 2]> {
    let out = transform_internal(from, to, Pt::xy(x, y), false)?;
    Ok(match out {
        TOut::P(p) => [Some(p.x), Some(p.y)],
        TOut::Undef | TOut::Null => [Some(f64::NAN), Some(f64::NAN)],
        TOut::Num(_) => [None, None],
    })
}
