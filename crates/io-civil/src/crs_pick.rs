//! crs-pick.js — the DOM-free half of the coordinate system dialog: systems a project's coordinates fit, words for a
//! system (unit, area of use, datum shift), latitude/longitude text both ways, control-point misclosure, ground factors
//! and reprojecting a whole project.

use crate::crs::{self, Local};
use cc_civil::i18n::t;
use cc_civil::js::{arr, hypot, num_to_string, numf, pad_start, round, to_fixed, to_js_string, truthy};
use cc_civil::tr;
use regex::Regex;
use serde_json::{Map, Value, json};
use std::sync::OnceLock;

type R<T> = Result<T, String>;

/// `familyOf(units)`.
pub fn family_of(units: &str) -> &'static [&'static str] {
    if units == "m" { &["m"] } else { &["ft", "ftUS"] }
}

/// One system that fits.
#[derive(Clone, Debug)]
pub struct Fit {
    pub entry: &'static crs::Entry,
    pub lon: f64,
    pub lat: f64,
}

fn rank_of(e: &crs::Entry) -> f64 {
    crs::rank(&e.name)
}

/// `systemsThatFit({x, y}, units, {limit})` → (list, total).
pub fn systems_that_fit(x: Option<f64>, y: Option<f64>, units: &str, limit: f64) -> R<(Vec<Fit>, usize)> {
    let (Some(x), Some(y)) = (x.filter(|v| v.is_finite()), y.filter(|v| v.is_finite())) else { return Ok((Vec::new(), 0)) };
    let fam = family_of(units);
    let all: Vec<&'static crs::Entry> =
        crs::search("", None, 1e6)?.into_iter().filter(|e| !e.geographic && fam.contains(&e.unit.as_str())).collect();
    let mut out: Vec<Fit> = Vec::new();
    for e in all {
        let Ok(t) = crs::converter(Some(&e.proj4), Some(crs::WGS84)) else { continue };
        let Ok(r) = t.forward(x, y) else { continue };
        let [lon, lat] = crs::nn(r);
        if !lon.is_finite() || !lat.is_finite() || !crs::in_area_v(e.area.as_ref(), lon, lat) {
            continue;
        }
        let Ok(b) = crs::converter(Some(crs::WGS84), Some(&e.proj4)) else { continue };
        let Ok(back) = b.forward(lon, lat) else { continue };
        let back = crs::nn(back);
        if !(hypot(back[0] - x, back[1] - y) < 1e-3) {
            continue;
        }
        out.push(Fit { entry: e, lon, lat });
    }
    cc_civil::js::sort_by(&mut out, |a, b| {
        let d = rank_of(a.entry) - rank_of(b.entry);
        if d != 0.0 && !d.is_nan() {
            return d;
        }
        let d = crs::area_size_v(a.entry.area.as_ref()) - crs::area_size_v(b.entry.area.as_ref());
        if d != 0.0 && !d.is_nan() {
            return d;
        }
        a.entry.code - b.entry.code
    });
    let total = out.len();
    out.truncate(cc_civil::js::usize_of(limit.clamp(0.0, 1e9)).unwrap_or(0));
    Ok((out, total))
}

/// `unitWords(u)`.
pub fn unit_words(u: Option<&Value>) -> String {
    let key = match u.map(to_js_string).as_deref() {
        Some("m") => Some("crs.unit.m"),
        Some("ft") => Some("crs.unit.ft"),
        Some("ftUS") => Some("crs.unit.ftus"),
        Some("deg") => Some("crs.unit.deg"),
        _ => None,
    };
    match key {
        Some(k) => t(k, &[]),
        None if truthy(u) => u.map(to_js_string).unwrap_or_default(),
        None => String::new(),
    }
}

fn deg(v: f64, pos: &str, neg: &str) -> String {
    format!("{}° {}", to_fixed(v.abs(), 2), if v >= 0.0 { pos } else { neg })
}

/// `areaWords(a)`.
pub fn area_words(a: Option<&Value>) -> String {
    let Some(l) = a.and_then(Value::as_array).filter(|l| l.len() == 4) else { return t("crs.area.none", &[]) };
    let v: [f64; 4] = [numf(l.first()), numf(l.get(1)), numf(l.get(2)), numf(l.get(3))];
    if v[1] <= -89.0 && v[3] >= 89.0 && crs::area_size_v(Some(&v)) >= 359.0 * 178.0 {
        return t("crs.area.world", &[]);
    }
    tr!("crs.area.box", south = deg(v[1], "N", "S"), north = deg(v[3], "N", "S"), west = deg(v[0], "E", "W"), east = deg(v[2], "E", "W"))
}

fn by_code_safe(c: Option<&Value>) -> Option<Value> {
    crs::by_code_val(c).ok().flatten().map(crs::Entry::value)
}

/// `shiftWords(entryOrCrs)`.
pub fn shift_words(x: &Value) -> String {
    let e: Option<Value> = if !truthy(Some(x)) {
        None
    } else if x.get("shiftAccuracy").is_some() {
        Some(x.clone())
    } else if x.get("code").is_some_and(|c| !c.is_null()) {
        by_code_safe(x.get("code"))
    } else {
        None
    };
    let name = x.get("name").filter(|n| truthy(Some(n))).map(to_js_string).unwrap_or_default();
    static P: OnceLock<[Regex; 3]> = OnceLock::new();
    let p = P.get_or_init(|| [re(r"^WGS 84\b"), re(r"^NAD83\b"), re(r"\+towgs84=0,0,0(\s|$)")]);
    if p[0].is_match(&name) {
        return t("crs.shift.wgs84", &[]);
    }
    if let Some(g) = x.get("grid").filter(|g| truthy(Some(g))) {
        let gname = to_js_string(g);
        let sg = crs::grid_suggest().into_iter().find(|q| q.get("name").and_then(Value::as_str) == Some(gname.as_str()));
        return if crs::grid_info(&gname).is_some() {
            let acc = match sg {
                Some(s) => crs::fmt_m(numf(s.get("accuracy"))),
                None => t("grid.shift.acc.own", &[]),
            };
            tr!("grid.shift.on", name = gname.as_str(), acc = acc)
        } else {
            tr!("grid.shift.missing", name = gname.as_str())
        };
    }
    if p[1].is_match(&name) {
        return t("crs.shift.nad83", &[]);
    }
    let Some(e) = e.filter(|e| truthy(Some(e))) else { return t("crs.shift.prj", &[]) };
    let acc = e.get("shiftAccuracy");
    let Some(a) = acc.filter(|a| !a.is_null()) else { return t("crs.shift.unknown", &[]) };
    let a = cc_civil::js::to_number(a);
    let p4 = e.get("proj4").filter(|v| truthy(Some(v))).map(to_js_string).unwrap_or_default();
    if a == 0.0 || (p[2].is_match(&p4) && a <= 1.0) {
        let v = if a != 0.0 && !a.is_nan() { a } else { 1.0 };
        return tr!("crs.shift.same", acc = crs::fmt_m(v));
    }
    tr!("crs.shift.known", acc = crs::fmt_m(a))
}

fn re(p: &str) -> Regex {
    Regex::new(p).unwrap_or_else(|_| never())
}
#[allow(clippy::expect_used)]
fn never() -> Regex {
    Regex::new("a^").expect("a constant pattern")
}

fn join_js(v: Option<&Value>) -> String {
    match v {
        Some(Value::Array(a)) => a.iter().map(|x| if x.is_null() { String::new() } else { to_js_string(x) }).collect::<Vec<_>>().join(", "),
        Some(other) => to_js_string(other),
        None => "undefined".into(),
    }
}

/// `describe(crs)`.
pub fn describe(c: &Value) -> Value {
    if !truthy(Some(c)) {
        return Value::Null;
    }
    let code = c.get("code").filter(|v| !v.is_null());
    let e = code.and_then(|v| by_code_safe(Some(v)));
    let mut local = t("crs.local.grid", &[]);
    if let Some(l) = c.get("local").filter(|l| truthy(Some(l))) {
        let from = arr(l.get("from"));
        let to = arr(l.get("to"));
        let same = |a: Option<&Value>, b: Option<&Value>| match (a, b) {
            (Some(Value::Number(x)), Some(Value::Number(y))) => x.as_f64() == y.as_f64(),
            (None, None) => true,
            (Some(x), Some(y)) => x == y && !x.is_number(),
            _ => false,
        };
        let rot0 = l.get("rotation").and_then(Value::as_f64) == Some(0.0);
        let ground = rot0 && same(from.first(), to.first()) && same(from.get(1), to.get(1));
        let scale = to_fixed(numf(l.get("scale")), 8);
        let fill = |v: Option<&Value>| v.map_or_else(|| "undefined".to_string(), to_js_string);
        local = if ground {
            tr!("crs.local.ground", scale = scale, e = fill(from.first()), n = fill(from.get(1)))
        } else {
            tr!(
                "crs.local.local",
                scale = scale,
                rotation = to_fixed(numf(l.get("rotation")), 6),
                from = join_js(l.get("from")),
                to = join_js(l.get("to"))
            )
        };
    }
    let mut o = Map::new();
    if let Some(n) = c.get("name") {
        o.insert("name".into(), n.clone());
    }
    o.insert("code".into(), code.cloned().unwrap_or(Value::Null));
    o.insert("unit".into(), Value::String(unit_words(c.get("unit"))));
    o.insert("shift".into(), Value::String(shift_words(c)));
    o.insert(
        "area".into(),
        Value::String(match &e {
            Some(e) => area_words(e.get("area")),
            None => t("crs.area.prj", &[]),
        }),
    );
    o.insert("local".into(), Value::String(local));
    Value::Object(o)
}

/// `latLonText(lon, lat, mode)`.
pub fn lat_lon_text(lon: f64, lat: f64, mode: &str) -> String {
    if !lon.is_finite() || !lat.is_finite() {
        return String::new();
    }
    if mode == "dd" {
        return format!(
            "{}° {}  {}° {}",
            to_fixed(lat.abs(), 6),
            if lat >= 0.0 { "N" } else { "S" },
            to_fixed(lon.abs(), 6),
            if lon >= 0.0 { "E" } else { "W" }
        );
    }
    format!("{}  {}", dms_of(lat, "N", "S"), dms_of(lon, "E", "W"))
}
fn dms_of(v: f64, pos: &str, neg: &str) -> String {
    let mut s = round(v.abs() * 360000.0) / 100.0;
    let d = (s / 3600.0).floor();
    s -= d * 3600.0;
    let m = (s / 60.0).floor();
    s -= m * 60.0;
    format!(
        "{}°{}′{}″ {}",
        num_to_string(d),
        pad_start(&num_to_string(m), 2, '0'),
        pad_start(&to_fixed(s, 2), 5, '0'),
        if v >= 0.0 { pos } else { neg }
    )
}

/// JS `\s` (inside a class, and as one).
const WS_IN: &str = r"\t\n\x0B\x0C\r \x{a0}\x{1680}\x{2000}-\x{200a}\x{2028}\x{2029}\x{202f}\x{205f}\x{3000}\x{feff}";
const WS: &str = r"[\t\n\x0B\x0C\r \x{a0}\x{1680}\x{2000}-\x{200a}\x{2028}\x{2029}\x{202f}\x{205f}\x{3000}\x{feff}]";

struct Angle {
    v: f64,
    axis: Option<&'static str>,
}

/// `parseLatLon(text)` → [lon, lat].
pub fn parse_lat_lon(text: &str) -> Option<[f64; 2]> {
    static P: OnceLock<[Regex; 3]> = OnceLock::new();
    let p = P.get_or_init(|| [re("[°º˚′'’″\"]"), re(&format!("{WS}+")), re(&format!("{WS}*[,;]{WS}*"))]);
    let s0 = cc_civil::js::slice16(cc_civil::js::trim(text), 200);
    let s1 = p[0].replace_all(&s0, " ").into_owned();
    let s = p[1].replace_all(&s1, " ").into_owned();
    if s.is_empty() {
        return None;
    }
    let halves: Vec<String> =
        if s.contains(',') || s.contains(';') { p[2].split(&s).map(str::to_string).collect() } else { split_hemis(&s)?.to_vec() };
    if halves.len() != 2 {
        return None;
    }
    let a = one_angle(halves.first()?)?;
    let b = one_angle(halves.get(1)?)?;
    let (lat, lon) = if a.axis == Some("lon") || b.axis == Some("lat") { (b, a) } else { (a, b) };
    if lat.axis == Some("lon") || lon.axis == Some("lat") {
        return None;
    }
    if !(lat.v.abs() <= 90.0) || !(lon.v.abs() <= 180.0) {
        return None;
    }
    Some([lon.v, lat.v])
}
fn split_hemis(s: &str) -> Option<[String; 2]> {
    static P: OnceLock<[Regex; 2]> = OnceLock::new();
    let p = P.get_or_init(|| {
        [
            re(&format!(r"^([NSns]?{WS}*[0-9.{w}]+[NSns]?){WS}+([EWew]?{WS}*[0-9.{w}]+[EWew]?)$", w = WS_IN)),
            re(&format!(r"^([EWew]?{WS}*[0-9.{w}]+[EWew]?){WS}+([NSns]?{WS}*[0-9.{w}]+[NSns]?)$", w = WS_IN)),
        ]
    });
    let c = p[0].captures(s).or_else(|| p[1].captures(s))?;
    Some([c.get(1)?.as_str().to_string(), c.get(2)?.as_str().to_string()])
}
fn one_angle(t0: &str) -> Option<Angle> {
    static P: OnceLock<[Regex; 3]> = OnceLock::new();
    let p = P.get_or_init(|| {
        [
            re(&format!(r"^([NSEWnsew]){WS}*|{WS}*([NSEWnsew])$")),
            re(&format!(r"^[NSEWnsew]{WS}*|{WS}*[NSEWnsew]$")),
            re(r"^[0-9]+(\.[0-9]+)?$"),
        ]
    });
    let mut s = cc_civil::js::trim(t0).to_string();
    let mut sign = 1.0;
    let mut axis = None;
    if let Some(h) = p[0].captures(&s) {
        let c = h.get(1).or_else(|| h.get(2)).map(|m| m.as_str().to_uppercase()).unwrap_or_default();
        axis = Some(if c == "N" || c == "S" { "lat" } else { "lon" });
        if c == "S" || c == "W" {
            sign = -1.0;
        }
        s = p[1].replace(&s, "").into_owned();
    }
    if let Some(r) = s.strip_prefix('-') {
        sign *= -1.0;
        s = r.to_string();
    } else if let Some(r) = s.strip_prefix('+') {
        s = r.to_string();
    }
    static SP: OnceLock<Regex> = OnceLock::new();
    let trimmed = cc_civil::js::trim(&s).to_string();
    let n: Vec<&str> = SP.get_or_init(|| re(&format!("{WS}+"))).split(&trimmed).collect();
    if n.is_empty() || n.len() > 3 || !n.iter().all(|x| p[2].is_match(x)) {
        return None;
    }
    let nums: Vec<f64> = n.iter().map(|x| cc_civil::js::str_to_number(x)).collect();
    let d = nums.first().copied().unwrap_or(f64::NAN);
    let m = nums.get(1).copied().unwrap_or(0.0);
    let sec = nums.get(2).copied().unwrap_or(0.0);
    if n.len() > 1 && (!cc_civil::js::is_integer(d) || m >= 60.0 || sec >= 60.0) {
        return None;
    }
    Some(Angle { v: sign * (d + m / 60.0 + sec / 3600.0), axis })
}

/// `gridOf(crs, lon, lat)`.
pub fn grid_of(c: &Value, lon: f64, lat: f64) -> R<[cc_proj::JsNum; 2]> {
    crs::transform(&Value::Null, c, lon, lat)
}

/// `localToGrid(l, x, y)`.
pub fn local_to_grid(l: &Local, x: f64, y: f64) -> [f64; 2] {
    use cc_civil::js::JsMath;
    let r = l.rotation * std::f64::consts::PI / 180.0;
    let (c, s) = (r.jcos(), r.jsin());
    let dx = x - l.from[0];
    let dy = y - l.from[1];
    [l.to[0] + l.scale * (c * dx - s * dy), l.to[1] + l.scale * (s * dx + c * dy)]
}

/// `misclosure(local, pLocal, pGrid)`.
pub fn misclosure(l: &Local, p_local: [f64; 2], p_grid: [f64; 2]) -> Value {
    let g = local_to_grid(l, p_local[0], p_local[1]);
    let dx = p_grid[0] - g[0];
    let dy = p_grid[1] - g[1];
    let dist = hypot(dx, dy);
    let span = hypot(p_local[0] - l.from[0], p_local[1] - l.from[1]);
    use cc_civil::js::J;
    json!({ "dx": dx.j(), "dy": dy.j(), "dist": dist.j(), "ppm": if span > 0.0 { (dist / span * 1e6).j() } else { Value::Null } })
}

/// `groundFactor(crs, x, y, z)`.
pub fn ground_factor(c: &Value, x: f64, y: f64, z: Option<&Value>) -> R<Value> {
    use cc_civil::js::J;
    let f = crs::frame(c)?.ok_or_else(|| "Cannot read properties of null (reading 'factors')".to_string())?;
    let r = f.factors(x, y, z)?;
    Ok(
        json!({ "k": r.k.j(), "elevation": r.elevation.j(), "combined": r.combined.j(), "convergence": r.convergence.j(), "lon": r.lon.j(), "lat": r.lat.j() }),
    )
}

/// `reprojectProject(state, points, map)` → { points, moved, refused } (the state is changed in place).
pub fn reproject_project(state: &mut Value, points: &[Value], map: &mut dyn FnMut(f64, f64) -> R<[f64; 2]>) -> R<Value> {
    if state.get("surface").and_then(|s| s.get("tin")).is_some_and(|v| truthy(Some(v))) {
        return Ok(json!({ "points": points, "moved": 0, "refused": t("crs.reproject.tin", &[]) }));
    }
    let mut moved: f64 = 0.0;
    let num = |v: Option<&Value>| cc_cad::js::to_number_opt(v);
    let mut pts = Vec::with_capacity(points.len());
    for p in points {
        let [x, y] = map(num(p.get("x")), num(p.get("y")))?;
        if !x.is_finite() || !y.is_finite() {
            let n = p.get("n").map_or_else(|| "undefined".to_string(), to_js_string);
            return Err(tr!("crs.reproject.point", n = n));
        }
        moved += 1.0;
        let mut o = match p {
            Value::Object(m) => m.clone(),
            _ => Map::new(),
        };
        o.insert("x".into(), cc_civil::js::jnum(x));
        o.insert("y".into(), cc_civil::js::jnum(y));
        pts.push(Value::Object(o));
    }
    let mut xy = |q: &mut Value, moved: &mut f64| -> R<()> {
        let [x, y] = map(num(q.get("x")), num(q.get("y")))?;
        if !x.is_finite() || !y.is_finite() {
            return Err(t("crs.reproject.coord", &[]));
        }
        if let Value::Object(m) = q {
            m.insert("x".into(), cc_civil::js::jnum(x));
            m.insert("y".into(), cc_civil::js::jnum(y));
        }
        *moved += 1.0;
        Ok(())
    };
    let each = |list: Option<&mut Value>, key: Option<&str>, xy: &mut dyn FnMut(&mut Value, &mut f64) -> R<()>, moved: &mut f64| -> R<()> {
        if let Some(Value::Array(items)) = list {
            for it in items.iter_mut() {
                match key {
                    Some(k) => {
                        if let Some(Value::Array(q)) = it.get_mut(k) {
                            for v in q.iter_mut() {
                                xy(v, moved)?;
                            }
                        }
                    }
                    None => xy(it, moved)?,
                }
            }
        }
        Ok(())
    };
    if let Some(s) = state.get_mut("surface") {
        each(s.get_mut("breaklines"), Some("pts"), &mut xy, &mut moved)?;
        if s.get("boundary").is_some_and(|b| truthy(Some(b))) {
            each(s.get_mut("boundary"), None, &mut xy, &mut moved)?;
        }
    }
    each(state.get_mut("alignments"), Some("pis"), &mut xy, &mut moved)?;
    each(state.get_mut("pads"), Some("pts"), &mut xy, &mut moved)?;
    if let Some(n) = state.get_mut("network") {
        each(n.get_mut("structures"), None, &mut xy, &mut moved)?;
    }
    Ok(json!({ "points": pts, "moved": cc_civil::js::jnum(moved), "refused": null }))
}
