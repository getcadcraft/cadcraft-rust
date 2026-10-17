//! terrain.js — what elevation grids and LiDAR share on their way into a project: units, GeoKeys → a system, the
//! file → project transform, raster windows and nodes, grid points and grid TINs.
//!
//! A grid read ([`Grid`]) is the JS `{ c0, r0, step, nx, ny, z }`: node (i, j) is cell (c0 + i·step, r0 + j·step) and
//! `z` holds the file's values (NaN = nodata).

use crate::crs;
use cc_civil::i18n::t;
use cc_civil::js::{J, max, min, to_js_string, usize_of};
use cc_civil::tr;
use serde_json::{Map, Value, json};

type R<T> = Result<T, String>;

/// `LIMITS`.
pub mod limits {
    pub const FILE_BYTES: f64 = 200.0 * 1024.0 * 1024.0;
    pub const POINTS: f64 = 500000.0;
    pub const TIN_NODES: f64 = 500000.0;
    pub const READ_CELLS: f64 = 400e6;
    pub const DIM: f64 = 1e6;
}

/// `UNIT_OF_EPSG[v]` (a property lookup: the key is `String(v)`).
pub fn unit_of_epsg(v: Option<&Value>) -> Option<&'static str> {
    match v.map(to_js_string).as_deref() {
        Some("9001") => Some("m"),
        Some("9002") => Some("ft"),
        Some("9003") => Some("ftUS"),
        _ => None,
    }
}
/// `EPSG_OF_UNIT[u]`.
pub fn epsg_of_unit(u: &str) -> Option<f64> {
    match u {
        "m" => Some(9001.0),
        "ft" => Some(9002.0),
        "ftUS" => Some(9003.0),
        _ => None,
    }
}
/// `UNIT_WORDS[u]`.
pub fn unit_words(u: &str) -> Option<String> {
    match u {
        "m" => Some(t("terrain.unit.m", &[])),
        "ft" => Some(t("terrain.unit.ft", &[])),
        "ftUS" => Some(t("terrain.unit.ftus", &[])),
        _ => None,
    }
}
/// `UNIT_SHORT[u]`.
pub fn unit_short(u: &str) -> Option<&'static str> {
    match u {
        "m" => Some("m"),
        "ft" => Some("ft"),
        "ftUS" => Some("ft (US)"),
        _ => None,
    }
}

/// `unitFromName(s)`.
pub fn unit_from_name(s: &str) -> Option<&'static str> {
    let lower = cc_civil::js::trim(s).to_lowercase();
    // `/[_\s]+/g` → ' '
    let mut t0 = String::with_capacity(lower.len());
    let mut in_run = false;
    for c in lower.chars() {
        if c == '_' || cc_civil::js::is_ws(c) {
            if !in_run {
                t0.push(' ');
            }
            in_run = true;
        } else {
            t0.push(c);
            in_run = false;
        }
    }
    if t0.is_empty() {
        return None;
    }
    match t0.as_str() {
        "m" | "metre" | "meter" | "metres" | "meters" | "9001" => Some("m"),
        "us survey foot" | "us survey feet" | "survey foot" | "us-ft" | "ftus" | "us ft" | "foot us" | "foot_us" | "9003" | "us foot" => {
            Some("ftUS")
        }
        "ft" | "foot" | "feet" | "international foot" | "intl foot" | "9002" => Some("ft"),
        _ => None,
    }
}

/// `crsFromGeoKeys(keys)`.
#[derive(Clone, Debug, PartialEq)]
pub struct GeoCrs {
    pub crs: Value,
    pub code: Option<f64>,
    pub note: Option<String>,
    pub z_unit: Option<&'static str>,
    pub xy_unit: Option<&'static str>,
}
impl J for GeoCrs {
    fn j(&self) -> Value {
        json!({ "crs": self.crs, "code": self.code.j(), "note": self.note, "zUnit": self.z_unit, "xyUnit": self.xy_unit })
    }
}

/// `crsFromGeoKeys(keys)`: `keys` by geotiff.js names or numeric ids.
pub fn crs_from_geo_keys(keys: Option<&Map<String, Value>>) -> GeoCrs {
    let k = |name: &str, id: u32| keys.and_then(|m| m.get(name).filter(|v| !v.is_null()).or_else(|| m.get(&id.to_string())));
    let model = k("GTModelTypeGeoKey", 1024);
    let proj = k("ProjectedCSTypeGeoKey", 3072);
    let geog = k("GeographicTypeGeoKey", 2048);
    let lin = k("ProjLinearUnitsGeoKey", 3076);
    let vert = k("VerticalUnitsGeoKey", 4099);
    let mut out = GeoCrs { crs: Value::Null, code: None, note: None, z_unit: unit_of_epsg(vert), xy_unit: unit_of_epsg(lin) };
    if keys.is_none() {
        out.note = Some(t("terrain.crs.none", &[]));
        return out;
    }
    let model_n = model.and_then(Value::as_f64);
    let code: Option<&Value> = if model_n == Some(2.0) {
        geog
    } else {
        match proj.filter(|v| !v.is_null()) {
            Some(p) => Some(p),
            None => {
                if model_n != Some(1.0) {
                    geog
                } else {
                    None
                }
            }
        }
    };
    let code_n = code.and_then(Value::as_f64).filter(|c| cc_civil::js::is_integer(*c));
    let Some(c) = code_n.filter(|c| *c > 0.0 && *c != 32767.0) else {
        out.note = Some(if code_n == Some(32767.0) { t("terrain.crs.own", &[]) } else { t("terrain.crs.none", &[]) });
        return out;
    };
    out.code = Some(c);
    if !crs::catalog_loaded() {
        out.note = Some(t("terrain.crs.noload", &[]));
        return out;
    }
    match crs::by_code(c).ok().flatten() {
        None => out.note = Some(tr!("terrain.crs.unknown", code = c)),
        Some(e) => out.crs = crs::crs_from_entry(e),
    }
    out
}

/// `projectZUnit(project)` from the project's state (`units`, `crs`).
pub fn project_z_unit(state: &Value) -> &'static str {
    if state.get("units").and_then(Value::as_str) == Some("m") {
        return "m";
    }
    if state.get("crs").and_then(|c| c.get("unit")).and_then(Value::as_str) == Some("ft") { "ft" } else { "ftUS" }
}
/// `zFactor(from, to)`.
pub fn z_factor(from: &str, to: &str) -> f64 {
    crs::metres_per(from).unwrap_or(f64::NAN) / crs::metres_per(to).unwrap_or(f64::NAN)
}

fn same_crs(a: &Value, b: &Value) -> bool {
    if !cc_civil::js::truthy(Some(a)) || !cc_civil::js::truthy(Some(b)) || cc_civil::js::truthy(b.get("local")) {
        return false;
    }
    let ac = a.get("code").filter(|c| !c.is_null());
    (ac.is_some() && ac == b.get("code") && strict_eq(ac, b.get("code"))) || strict_eq(a.get("proj4"), b.get("proj4"))
}
/// `===` on two JSON values (undefined === undefined; objects by identity → never equal here).
fn strict_eq(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(Value::Number(x)), Some(Value::Number(y))) => x.as_f64() == y.as_f64(),
        (Some(Value::String(x)), Some(Value::String(y))) => x == y,
        (Some(Value::Bool(x)), Some(Value::Bool(y))) => x == y,
        (Some(Value::Null), Some(Value::Null)) => true,
        _ => false,
    }
}

/// A point transform `(x, y) → [x, y]` (a thrown error comes back as Err).
pub type XyFn = Box<dyn Fn(f64, f64) -> R<[f64; 2]>>;

/// `toProject(src, pc)` → (fn, identity).
pub fn to_project(src: &Value, pc: &Value) -> R<(XyFn, bool)> {
    if !cc_civil::js::truthy(Some(src)) || !cc_civil::js::truthy(Some(pc)) || same_crs(src, pc) {
        return Ok((Box::new(|x, y| Ok([x, y])), true));
    }
    let f = crs::frame(pc)?.ok_or("frame")?;
    let s = src.clone();
    // `f.from(src)` builds its converter at once (a definition proj4 cannot read throws here)
    f.check_from(&s)?;
    Ok((Box::new(move |x, y| f.from(&s, x, y)), false))
}
/// `toSource(src, pc)`.
pub fn to_source(src: &Value, pc: &Value) -> R<XyFn> {
    if !cc_civil::js::truthy(Some(src)) || !cc_civil::js::truthy(Some(pc)) || same_crs(src, pc) {
        return Ok(Box::new(|x, y| Ok([x, y])));
    }
    let f = crs::frame(pc)?.ok_or("frame")?;
    let s = src.clone();
    f.check_to(&s)?;
    Ok(Box::new(move |x, y| f.to(&s, x, y).map(crs::nn)))
}
/// `lonLatOf(src)`.
pub fn lon_lat_of(src: &Value) -> R<Option<XyFn>> {
    if !cc_civil::js::truthy(Some(src)) {
        return Ok(None);
    }
    if src.get("unit").and_then(Value::as_str) == Some("deg") {
        return Ok(Some(Box::new(|x, y| Ok([x, y]))));
    }
    let def = crs::def_of(src);
    let t = crs::converter(def.as_deref(), Some(crs::WGS84))?;
    Ok(Some(Box::new(move |x, y| t.forward(x, y).map(crs::nn))))
}

/// `{ minX, minY, maxX, maxY }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxXY {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}
impl J for BoxXY {
    fn j(&self) -> Value {
        json!({ "minX": self.min_x.j(), "minY": self.min_y.j(), "maxX": self.max_x.j(), "maxY": self.max_y.j() })
    }
}
pub fn read_box(v: Option<&Value>) -> Option<BoxXY> {
    let b = v.filter(|b| b.is_object())?;
    let n = |k: &str| cc_civil::js::numf(b.get(k));
    Some(BoxXY { min_x: n("minX"), min_y: n("minY"), max_x: n("maxX"), max_y: n("maxY") })
}

/// `boxThrough(box, fn, n)`.
pub fn box_through(b: &BoxXY, f: &dyn Fn(f64, f64) -> R<[f64; 2]>, n: f64) -> R<Option<BoxXY>> {
    let mut o = BoxXY { min_x: f64::INFINITY, min_y: f64::INFINITY, max_x: f64::NEG_INFINITY, max_y: f64::NEG_INFINITY };
    let mut add = |x: f64, y: f64| -> R<()> {
        let [u, v] = f(x, y)?;
        if !u.is_finite() || !v.is_finite() {
            return Ok(());
        }
        o.min_x = min(o.min_x, u);
        o.max_x = max(o.max_x, u);
        o.min_y = min(o.min_y, v);
        o.max_y = max(o.max_y, v);
        Ok(())
    };
    let mut i = 0.0;
    while i <= n {
        let tt = i / n;
        let x = b.min_x + (b.max_x - b.min_x) * tt;
        let y = b.min_y + (b.max_y - b.min_y) * tt;
        add(x, b.min_y)?;
        add(x, b.max_y)?;
        add(b.min_x, y)?;
        add(b.max_x, y)?;
        i += 1.0;
    }
    Ok(if o.min_x.is_finite() { Some(o) } else { None })
}

/// `nodeXY(src, col, row)`.
pub fn node_xy(gt: &[f64; 6], col: f64, row: f64) -> [f64; 2] {
    let c = col + 0.5;
    let r = row + 0.5;
    [gt[0] + c * gt[1] + r * gt[2], gt[3] + c * gt[4] + r * gt[5]]
}

/// A window of cells, half-open.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Win {
    pub c0: f64,
    pub r0: f64,
    pub c1: f64,
    pub r1: f64,
}
impl J for Win {
    fn j(&self) -> Value {
        json!({ "c0": self.c0.j(), "r0": self.r0.j(), "c1": self.c1.j(), "r1": self.r1.j() })
    }
}

/// `gridWindow(src, box)`.
pub fn grid_window(gt: &[f64; 6], width: f64, height: f64, b: Option<&BoxXY>) -> Option<Win> {
    let g = gt;
    let det = g[1] * g[5] - g[2] * g[4];
    let b = b?;
    if det == 0.0 || det.is_nan() {
        return None;
    }
    let inv = |x: f64, y: f64| {
        let dx = x - g[0];
        let dy = y - g[3];
        [(g[5] * dx - g[2] * dy) / det - 0.5, (-g[4] * dx + g[1] * dy) / det - 0.5]
    };
    let (mut c0, mut r0, mut c1, mut r1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for (x, y) in [(b.min_x, b.min_y), (b.max_x, b.min_y), (b.max_x, b.max_y), (b.min_x, b.max_y)] {
        let [c, r] = inv(x, y);
        c0 = min(c0, c);
        c1 = max(c1, c);
        r0 = min(r0, r);
        r1 = max(r1, r);
    }
    let w = Win {
        c0: max(0.0, (c0 - 1e-9).ceil()),
        r0: max(0.0, (r0 - 1e-9).ceil()),
        c1: min(width, (c1 + 1e-9).floor() + 1.0),
        r1: min(height, (r1 + 1e-9).floor() + 1.0),
    };
    if w.c1 > w.c0 && w.r1 > w.r0 { Some(w) } else { None }
}

/// `nextNumber(points)`.
pub fn next_number(points: &[Value]) -> f64 {
    let mut hi = 0.0;
    for p in points {
        let n = cc_civil::js::number(p.get("n"));
        if cc_civil::js::is_integer(n) && n > hi && n < 1e12 {
            hi = n;
        }
    }
    hi + 1.0
}

/// A grid read.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    pub c0: f64,
    pub r0: f64,
    pub step: f64,
    pub nx: usize,
    pub ny: usize,
    pub z: Vec<f64>,
}
impl J for Grid {
    fn j(&self) -> Value {
        json!({ "c0": self.c0.j(), "r0": self.r0.j(), "step": self.step.j(), "nx": self.nx, "ny": self.ny, "z": self.z.j() })
    }
}
fn nf(n: usize) -> f64 {
    n as f64
}

/// One survey point of `gridPoints`.
#[derive(Clone, Debug, PartialEq)]
pub struct GPt {
    pub n: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub d: String,
}
impl J for GPt {
    fn j(&self) -> Value {
        json!({ "n": self.n, "x": self.x.j(), "y": self.y.j(), "z": self.z.j(), "d": self.d })
    }
}

/// `gridPoints(grid, src, { fn, zf, desc, start })`.
pub fn grid_points(grid: &Grid, gt: &[f64; 6], f: &dyn Fn(f64, f64) -> R<[f64; 2]>, zf: f64, desc: &str, start: f64) -> R<Vec<GPt>> {
    let mut out = Vec::new();
    let mut n = start;
    for j in 0..grid.ny {
        for i in 0..grid.nx {
            let z = grid.z.get(j.saturating_mul(grid.nx).saturating_add(i)).copied().unwrap_or(f64::NAN);
            if z.is_nan() {
                continue;
            }
            let [sx, sy] = node_xy(gt, grid.c0 + nf(i) * grid.step, grid.r0 + nf(j) * grid.step);
            let [x, y] = f(sx, sy)?;
            if !x.is_finite() || !y.is_finite() {
                continue;
            }
            out.push(GPt { n: cc_civil::js::num_to_string(n), x, y, z: z * zf, d: desc.to_string() });
            n += 1.0;
        }
    }
    Ok(out)
}

/// `gridTin(grid, src, { fn, zf })` → (pts, faces, cells, skipped).
pub struct GridTin {
    pub pts: Vec<[f64; 3]>,
    pub faces: Vec<[usize; 3]>,
    pub cells: usize,
    pub skipped: usize,
}
impl J for GridTin {
    fn j(&self) -> Value {
        let pts: Vec<Value> = self.pts.iter().map(|p| json!({ "x": p[0].j(), "y": p[1].j(), "z": p[2].j() })).collect();
        json!({ "pts": pts, "faces": self.faces.iter().map(|f| f.to_vec()).collect::<Vec<_>>(), "cells": self.cells, "skipped": self.skipped })
    }
}
pub fn grid_tin(grid: &Grid, gt: &[f64; 6], f: &dyn Fn(f64, f64) -> R<[f64; 2]>, zf: f64) -> R<GridTin> {
    let (nx, ny) = (grid.nx, grid.ny);
    let total = nx.checked_mul(ny).ok_or("Invalid typed array length")?;
    let mut idx: Vec<Option<usize>> = vec![None; total];
    let mut pts = Vec::new();
    for j in 0..ny {
        for i in 0..nx {
            let k = j.saturating_mul(nx).saturating_add(i);
            let v = grid.z.get(k).copied().unwrap_or(f64::NAN);
            if v.is_nan() {
                continue;
            }
            let [sx, sy] = node_xy(gt, grid.c0 + nf(i) * grid.step, grid.r0 + nf(j) * grid.step);
            let [x, y] = f(sx, sy)?;
            if !x.is_finite() || !y.is_finite() {
                continue;
            }
            if let Some(s) = idx.get_mut(k) {
                *s = Some(pts.len());
            }
            pts.push([x, y, v * zf]);
        }
    }
    let mut faces = Vec::new();
    let (mut cells, mut skipped) = (0usize, 0usize);
    let at = |i: usize, j: usize| idx.get(j.saturating_mul(nx).saturating_add(i)).copied().flatten();
    for j in 0..ny.saturating_sub(1) {
        for i in 0..nx.saturating_sub(1) {
            let (i1, j1) = (i.saturating_add(1), j.saturating_add(1));
            let (a, b, c, d) = (at(i, j1), at(i1, j1), at(i1, j), at(i, j));
            cells = cells.saturating_add(1);
            match (a, b, c) {
                (Some(a), Some(b), Some(c)) => faces.push([a, b, c]),
                _ => skipped = skipped.saturating_add(1),
            }
            match (a, c, d) {
                (Some(a), Some(c), Some(d)) => faces.push([a, c, d]),
                _ => skipped = skipped.saturating_add(1),
            }
        }
    }
    Ok(GridTin { pts, faces, cells, skipped })
}

/// `niceStep(x)`.
pub fn nice_step(x: f64) -> f64 {
    if !(x > 0.0) || !x.is_finite() {
        return 1.0;
    }
    let p = cc_civil::js::pow(10.0, cc_civil::js::log10(x).floor());
    let m = x / p;
    (if m <= 1.0 + 1e-9 {
        1.0
    } else if m <= 2.0 + 1e-9 {
        2.0
    } else if m <= 5.0 + 1e-9 {
        5.0
    } else {
        10.0
    }) * p
}

/// A count from a JS number (0 when it is not a whole non-negative number).
pub fn count_of(x: f64) -> usize {
    usize_of(x).unwrap_or(0)
}
