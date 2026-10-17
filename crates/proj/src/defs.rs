//! proj4js `projString.js`, `defs.js` (+ `global.js`), `match.js` and `parseCode.js`: a definition (proj string, WKT
//! or a known name) → the plain object `Projection` is built from.

use crate::tables::{DATUMS, PRIME_MERIDIANS};
use crate::val::{Props, Val, parse_float, parse_int};

pub const D2R: f64 = 0.017_453_292_519_943_295_77;

/// constants/units.js
pub static UNITS: &[(&str, f64)] = &[
    ("mm", 0.001),
    ("cm", 0.01),
    ("ft", 0.3048),
    ("us-ft", 1200.0 / 3937.0),
    ("fath", 1.8288),
    ("kmi", 1852.0),
    ("us-ch", 20.1168402336805),
    ("us-mi", 1609.34721869444),
    ("km", 1000.0),
    ("ind-ft", 0.30479841),
    ("ind-yd", 0.91439523),
    ("mi", 1609.344),
    ("yd", 0.9144),
    ("ch", 20.1168),
    ("link", 0.201168),
    ("dm", 0.1),
    ("in", 0.0254),
    ("ind-ch", 20.11669506),
    ("us-in", 0.025400050800101),
    ("us-yd", 0.914401828803658),
];

/// `/[\s_\-\/\(\)]/g` removed, lower case — match.js's key normalisation.
pub fn norm_key(k: &str) -> String {
    k.to_lowercase().chars().filter(|&c| !(cc_cad::js::is_ws(c) || matches!(c, '_' | '-' | '/' | '(' | ')'))).collect()
}

/// match.js over a list of keys: the exact key when its value is truthy, else the first key that normalises the same.
/// `truthy(i)` says whether entry i's value is truthy (for the exact-key test). Err = the TypeError the JS throws when
/// the key is not text.
pub fn match_index(keys: &[&str], truthy: &dyn Fn(usize) -> bool, key: &Val) -> Result<Option<usize>, String> {
    let exact = key.to_js_string();
    if let Some(i) = keys.iter().position(|k| *k == exact)
        && truthy(i)
    {
        return Ok(Some(i));
    }
    let Val::Str(ks) = key else { return Err(type_error_call(key, "toLowerCase")) };
    let lkey = norm_key(ks);
    Ok(keys.iter().position(|k| norm_key(k) == lkey))
}

/// The JS TypeError text for calling a method on a value that has none.
pub fn type_error_call(v: &Val, method: &str) -> String {
    match v {
        Val::Undef => format!("Cannot read properties of undefined (reading '{method}')"),
        Val::Null => format!("Cannot read properties of null (reading '{method}')"),
        _ => format!("{}.{method} is not a function", js_repr(v)),
    }
}
fn js_repr(v: &Val) -> String {
    match v {
        Val::Str(s) => format!("\"{s}\""),
        Val::Arr(_) => "[object Array]".into(),
        other => other.to_js_string(),
    }
}

/// match.js on a plain object.
pub fn match_obj(obj: &Props, key: &str) -> Val {
    let v = obj.at(key);
    if v.truthy() {
        return v;
    }
    let lkey = norm_key(key);
    for k in obj.keys() {
        if norm_key(&k) == lkey {
            return obj.at(&k);
        }
    }
    Val::Undef
}

/// The datum table entry match.js finds for a datum code.
pub fn match_datum(code: &Val) -> Result<Option<&'static crate::tables::Dat>, String> {
    let keys: Vec<&str> = DATUMS.iter().map(|d| d.key).collect();
    Ok(match_index(&keys, &|_| true, code)?.and_then(|i| DATUMS.get(i)))
}
/// The ellipsoid table entry for a name.
pub fn match_ellipsoid(name: &Val) -> Result<Option<&'static crate::tables::Ell>, String> {
    let keys: Vec<&str> = crate::tables::ELLIPSOIDS.iter().map(|d| d.key).collect();
    Ok(match_index(&keys, &|_| true, name)?.and_then(|i| crate::tables::ELLIPSOIDS.get(i)))
}

/// `"x".split(sep)` as JS does it.
pub fn js_split(s: &str, sep: char) -> Vec<String> {
    s.split(sep).map(str::to_string).collect()
}

/// projString.js: `+proj=… +lat_0=…` → the definition object.
pub fn proj_str(def_data: &str) -> Result<Props, String> {
    let mut param_obj = Props::new();
    for part in def_data.split('+') {
        let a = cc_cad::js::trim(part);
        if a.is_empty() {
            continue;
        }
        let split: Vec<&str> = a.split('=').collect();
        let key = split.first().copied().unwrap_or("").to_lowercase();
        let val = match split.get(1) {
            Some(v) => Val::str(v),
            None => Val::Bool(true),
        };
        param_obj.set(&key, val);
    }
    let mut s = Props::new();
    let num = |v: &Val| v.num();
    for name in param_obj.keys() {
        let v = param_obj.at(&name);
        match name.as_str() {
            "proj" => {
                let pn = match &v {
                    Val::Str(t) if t == "lonlat" || t == "latlon" || t == "latlong" => Val::str("longlat"),
                    other => other.clone(),
                };
                s.set("projName", pn);
            }
            "datum" => s.set("datumCode", v),
            "rf" => s.set("rf", Val::Num(parse_float(&v))),
            "lat_0" => s.set("lat0", Val::Num(num(&v) * D2R)),
            "lat_1" => s.set("lat1", Val::Num(num(&v) * D2R)),
            "lat_2" => s.set("lat2", Val::Num(num(&v) * D2R)),
            "lat_ts" => s.set("lat_ts", Val::Num(num(&v) * D2R)),
            "lon_0" => s.set("long0", Val::Num(num(&v) * D2R)),
            "lon_wrap" => s.set("long_wrap", Val::Num(parse_float(&v) * D2R)),
            "lon_1" => s.set("long1", Val::Num(num(&v) * D2R)),
            "lon_2" => s.set("long2", Val::Num(num(&v) * D2R)),
            "alpha" => s.set("alpha", Val::Num(parse_float(&v) * D2R)),
            "gamma" => s.set("rectified_grid_angle", Val::Num(parse_float(&v) * D2R)),
            "lonc" => s.set("longc", Val::Num(num(&v) * D2R)),
            "x_0" => s.set("x0", Val::Num(parse_float(&v))),
            "y_0" => s.set("y0", Val::Num(parse_float(&v))),
            "k_0" | "k" => s.set("k0", Val::Num(parse_float(&v))),
            "a" => s.set("a", Val::Num(parse_float(&v))),
            "b" => s.set("b", Val::Num(parse_float(&v))),
            "r" => {
                let r = parse_float(&v);
                s.set("a", Val::Num(r));
                s.set("b", Val::Num(r));
            }
            "r_a" => s.set("R_A", Val::Bool(true)),
            "zone" => s.set("zone", Val::Num(parse_int(&v))),
            "south" => s.set("utmSouth", Val::Bool(true)),
            "towgs84" => {
                let Val::Str(t) = &v else { return Err(type_error_call(&v, "split")) };
                s.set("datum_params", Val::Arr(t.split(',').map(|a| Val::Num(crate::val::parse_float_str(a))).collect()));
            }
            "to_meter" => s.set("to_meter", Val::Num(parse_float(&v))),
            "units" => {
                s.set("units", v.clone());
                let keys: Vec<&str> = UNITS.iter().map(|u| u.0).collect();
                if let Some(i) = match_index(&keys, &|_| true, &v)?
                    && let Some(u) = UNITS.get(i)
                {
                    s.set("to_meter", Val::Num(u.1));
                }
            }
            "from_greenwich" => s.set("from_greenwich", Val::Num(num(&v) * D2R)),
            "pm" => {
                let keys: Vec<&str> = PRIME_MERIDIANS.iter().map(|p| p.0).collect();
                // match() returns the value: Greenwich's 0 is falsy, so it falls through to parseFloat
                let truthy = |i: usize| PRIME_MERIDIANS.get(i).is_some_and(|p| p.1 != 0.0);
                let pm = match_index(&keys, &truthy, &v)?.and_then(|i| PRIME_MERIDIANS.get(i)).map(|p| p.1);
                let val = match pm {
                    Some(x) if x != 0.0 => x,
                    _ => parse_float(&v),
                };
                s.set("from_greenwich", Val::Num(val * D2R));
            }
            "nadgrids" => {
                if v == Val::str("@null") {
                    s.set("datumCode", Val::str("none"));
                } else {
                    s.set("nadgrids", v);
                }
            }
            "axis" => {
                if let Val::Str(t) = &v {
                    let legal = "ewnsud";
                    let c: Vec<char> = t.chars().collect();
                    if t.encode_utf16().count() == 3 && c.iter().all(|ch| legal.contains(*ch)) {
                        s.set("axis", v.clone());
                    }
                } else {
                    // v.length of `true` is undefined: not 3, nothing set
                }
            }
            "approx" => s.set("approx", Val::Bool(true)),
            "over" => s.set("over", Val::Bool(true)),
            _ => s.set(&name, v),
        }
    }
    if let Some(Val::Str(dc)) = s.get("datumCode").cloned()
        && dc != "WGS84"
    {
        s.set("datumCode", Val::Str(dc.to_lowercase()));
    }
    s.set("projStr", Val::str(def_data));
    Ok(s)
}

/// The names global.js defines (and the aliases it adds), with their definitions.
pub fn known_def(name: &str) -> Option<String> {
    let merc3857 = "+title=WGS 84 / Pseudo-Mercator +proj=merc +a=6378137 +b=6378137 +lat_ts=0.0 +lon_0=0.0 +x_0=0.0 +y_0=0 +k=1.0 +units=m +nadgrids=@null +no_defs";
    match name {
        "EPSG:4326" | "WGS84" => return Some("+title=WGS 84 (long/lat) +proj=longlat +ellps=WGS84 +datum=WGS84 +units=degrees".into()),
        "EPSG:4269" => {
            return Some(
                "+title=NAD83 (long/lat) +proj=longlat +a=6378137.0 +b=6356752.31414036 +ellps=GRS80 +datum=NAD83 +units=degrees".into(),
            );
        }
        "EPSG:3857" | "EPSG:3785" | "GOOGLE" | "EPSG:900913" | "EPSG:102113" => return Some(merc3857.into()),
        "EPSG:5041" => {
            return Some(
                "+title=WGS 84 / UPS North (E,N) +proj=stere +lat_0=90 +lon_0=0 +k=0.994 +x_0=2000000 +y_0=2000000 +datum=WGS84 +units=m"
                    .into(),
            );
        }
        "EPSG:5042" => {
            return Some(
                "+title=WGS 84 / UPS South (E,N) +proj=stere +lat_0=-90 +lon_0=0 +k=0.994 +x_0=2000000 +y_0=2000000 +datum=WGS84 +units=m"
                    .into(),
            );
        }
        _ => {}
    }
    let code = name.strip_prefix("EPSG:")?;
    let n: u32 = code.parse().ok()?;
    if code != n.to_string() {
        return None;
    }
    if (32601..=32660).contains(&n) {
        return Some(format!("+proj=utm +zone={} +datum=WGS84 +units=m", n - 32600));
    }
    if (32701..=32760).contains(&n) {
        return Some(format!("+proj=utm +zone={} +south +datum=WGS84 +units=m", n - 32700));
    }
    None
}

/// Properties every function object (and Object.prototype) has: `code in defs` is true for them, and the value is
/// not an object.
const FUNCTION_PROPS: &[&str] = &[
    "length",
    "name",
    "prototype",
    "arguments",
    "caller",
    "apply",
    "bind",
    "call",
    "toString",
    "constructor",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toLocaleString",
    "valueOf",
    "__proto__",
    "__defineGetter__",
    "__defineSetter__",
    "__lookupGetter__",
    "__lookupSetter__",
];

/// What parseCode.js gives for a definition string: Ok(None) = not an object (the caller's "Could not parse").
pub fn parse(code: &str) -> Result<Option<Props>, String> {
    let out: Option<Props> = if let Some(d) = known_def(code) {
        Some(proj_str(&d)?)
    } else if FUNCTION_PROPS.contains(&code) {
        None
    } else if !code.starts_with('+') && code.contains('[') {
        let mut o = crate::wkt::wkt(code)?;
        // a WKT EXTENSION["PROJ4", "…"] wins
        if let Val::Obj(obj) = &o {
            let ext = match_obj(obj, "extension");
            if let Val::Obj(e) = &ext {
                let ps = match_obj(e, "proj4");
                if ps.truthy() {
                    o = Val::Obj(proj_str(&ps.to_js_string())?);
                }
            }
        }
        match o {
            Val::Obj(p) => Some(p),
            _ => None,
        }
    } else if code.starts_with('+') {
        Some(proj_str(code)?)
    } else {
        None
    };
    let Some(out) = out else { return Ok(None) };
    if check_mercator(&out)? {
        return Ok(Some(proj_str(&known_def("EPSG:3857").unwrap_or_default())?));
    }
    Ok(Some(out))
}

fn check_mercator(item: &Props) -> Result<bool, String> {
    const CODES: [&str; 4] = ["3857", "900913", "3785", "102113"];
    let title = item.at("title");
    if title.truthy() {
        let Val::Str(t) = &title else { return Err(type_error_call(&title, "toLowerCase")) };
        let lower = t.to_lowercase();
        let rest: String = t.chars().skip(5).collect();
        return Ok(lower.starts_with("epsg:") && CODES.contains(&rest.as_str()));
    }
    let auth = match_obj(item, "authority");
    if !auth.truthy() {
        return Ok(false);
    }
    let Val::Obj(a) = &auth else { return Ok(false) };
    let code = match_obj(a, "epsg");
    Ok(match &code {
        Val::Str(c) => CODES.contains(&c.as_str()),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strings() {
        let p = proj_str("+proj=utm +zone=14 +datum=NAD83 +units=us-ft +no_defs").unwrap();
        assert_eq!(p.at("projName"), Val::str("utm"));
        assert_eq!(p.at("zone"), Val::Num(14.0));
        assert_eq!(p.at("datumCode"), Val::str("nad83"));
        assert_eq!(p.at("to_meter"), Val::Num(1200.0 / 3937.0));
        assert_eq!(p.at("no_defs"), Val::Bool(true));
        let g = proj_str("+proj=longlat +pm=greenwich").unwrap();
        assert!(g.at("from_greenwich").num().is_nan());
    }
}
