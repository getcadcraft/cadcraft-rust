//! shapes/parts.js — mechanical standard parts from public standard tables (ISO 4014/4017/4032/7089/2338/15,
//! DIN 6885-1, ASME B18.2.1/B18.2.2, ISO 54/53 spur gears): the families, their sizes, default options and the part
//! record (dimensions in mm, designation, mass of steel, plan box). Only the numbers are taken from each standard.

use crate::js;
use crate::jv;
use serde_json::Value;

pub const STEEL_DENSITY: f64 = 7850.0;
const IN: f64 = 25.4;

/// `PART_STANDARDS`: id → (title, url).
pub const PART_STANDARDS: [(&str, &str, &str); 11] = [
    ("ISO 4014", "ISO 4014:2022 Fasteners — Hexagon head bolts — Product grades A and B", "https://www.iso.org/standard/72579.html"),
    ("ISO 4017", "ISO 4017:2022 Fasteners — Hexagon head screws — Product grades A and B", "https://www.iso.org/standard/72585.html"),
    ("ISO 4032", "ISO 4032:2023 Fasteners — Hexagon regular nuts (style 1)", "https://www.iso.org/standard/75016.html"),
    ("ISO 7089", "ISO 7089:2000 Plain washers — Normal series — Product grade A", "https://www.iso.org/standard/13666.html"),
    (
        "ISO 2338",
        "ISO 2338:1997 Parallel pins, of unhardened steel and austenitic stainless steel",
        "https://www.iso.org/standard/20001.html",
    ),
    (
        "ISO 15",
        "ISO 15:2017 Rolling bearings — Radial bearings — Boundary dimensions, general plan",
        "https://www.iso.org/standard/69977.html",
    ),
    (
        "DIN 6885-1",
        "DIN 6885-1 Drive type fastenings without taper action — Parallel keys, keyways, deep pattern",
        "https://www.din.de/en/getting-involved/standards-committees/nam/publications/wdc-beuth:din21:346315269",
    ),
    (
        "ASME B18.2.1",
        "ASME B18.2.1 Square, hex, heavy hex and askew head bolts and hex, heavy hex, hex flange, lobed head and lag screws (inch series)",
        "https://www.asme.org/codes-standards/find-codes-standards/b18-2-1-square-hex-heavy-hex-askew-head-bolts-hex-heavy-hex-hex-flange-lobed-head-lag-screws",
    ),
    (
        "ASME B18.2.2",
        "ASME B18.2.2-2022 Nuts for general applications: machine screw nuts, and hex, square, hex flange and coupling nuts (inch series)",
        "https://www.asme.org/codes-standards/find-codes-standards/b18-2-2-nuts-general-applications-machine-screw-nuts-hex-square-hex-flange-coupling-nuts",
    ),
    (
        "ISO 54",
        "ISO 54:1996 Cylindrical gears for general engineering and for heavy engineering — Modules",
        "https://www.iso.org/standard/22644.html",
    ),
    (
        "ISO 53",
        "ISO 53:1998 Cylindrical gears for general and heavy engineering — Standard basic rack tooth profile",
        "https://www.iso.org/standard/22643.html",
    ),
];
pub fn standard(id: &str) -> Option<(&'static str, &'static str)> {
    PART_STANDARDS.iter().find(|(k, _, _)| *k == id).map(|(_, t, u)| (*t, *u))
}

// [size, d, P, s, k, m, d1, d2, h]
const METRIC: [(&str, f64, f64, f64, f64, f64, f64, f64, f64); 10] = [
    ("M5", 5.0, 0.8, 8.0, 3.5, 4.7, 5.3, 10.0, 1.0),
    ("M6", 6.0, 1.0, 10.0, 4.0, 5.2, 6.4, 12.0, 1.6),
    ("M8", 8.0, 1.25, 13.0, 5.3, 6.8, 8.4, 16.0, 1.6),
    ("M10", 10.0, 1.5, 16.0, 6.4, 8.4, 10.5, 20.0, 2.0),
    ("M12", 12.0, 1.75, 18.0, 7.5, 10.8, 13.0, 24.0, 2.5),
    ("M16", 16.0, 2.0, 24.0, 10.0, 14.8, 17.0, 30.0, 3.0),
    ("M20", 20.0, 2.5, 30.0, 12.5, 18.0, 21.0, 37.0, 3.0),
    ("M24", 24.0, 3.0, 36.0, 15.0, 21.5, 25.0, 44.0, 4.0),
    ("M30", 30.0, 3.5, 46.0, 18.7, 25.6, 31.0, 56.0, 4.0),
    ("M36", 36.0, 4.0, 55.0, 22.5, 31.0, 37.0, 66.0, 5.0),
];
// [size, D, tpi, F, H bolt, H nut]
const INCH: [(&str, f64, f64, f64, f64, f64); 9] = [
    ("1/4", 1.0 / 4.0, 20.0, 7.0 / 16.0, 11.0 / 64.0, 7.0 / 32.0),
    ("5/16", 5.0 / 16.0, 18.0, 1.0 / 2.0, 7.0 / 32.0, 17.0 / 64.0),
    ("3/8", 3.0 / 8.0, 16.0, 9.0 / 16.0, 1.0 / 4.0, 21.0 / 64.0),
    ("7/16", 7.0 / 16.0, 14.0, 5.0 / 8.0, 19.0 / 64.0, 3.0 / 8.0),
    ("1/2", 1.0 / 2.0, 13.0, 3.0 / 4.0, 11.0 / 32.0, 7.0 / 16.0),
    ("5/8", 5.0 / 8.0, 11.0, 15.0 / 16.0, 27.0 / 64.0, 35.0 / 64.0),
    ("3/4", 3.0 / 4.0, 10.0, 9.0 / 8.0, 1.0 / 2.0, 41.0 / 64.0),
    ("7/8", 7.0 / 8.0, 9.0, 21.0 / 16.0, 37.0 / 64.0, 3.0 / 4.0),
    ("1", 1.0, 8.0, 3.0 / 2.0, 43.0 / 64.0, 55.0 / 64.0),
];
const PINS: [(f64, f64); 9] =
    [(3.0, 0.5), (4.0, 0.63), (5.0, 0.8), (6.0, 1.2), (8.0, 1.6), (10.0, 2.0), (12.0, 2.5), (16.0, 3.0), (20.0, 3.5)];
const BEARINGS: [(&str, f64, f64, f64); 33] = [
    ("6000", 10.0, 26.0, 8.0),
    ("6001", 12.0, 28.0, 8.0),
    ("6002", 15.0, 32.0, 9.0),
    ("6003", 17.0, 35.0, 10.0),
    ("6004", 20.0, 42.0, 12.0),
    ("6005", 25.0, 47.0, 12.0),
    ("6006", 30.0, 55.0, 13.0),
    ("6007", 35.0, 62.0, 14.0),
    ("6008", 40.0, 68.0, 15.0),
    ("6009", 45.0, 75.0, 16.0),
    ("6010", 50.0, 80.0, 16.0),
    ("6200", 10.0, 30.0, 9.0),
    ("6201", 12.0, 32.0, 10.0),
    ("6202", 15.0, 35.0, 11.0),
    ("6203", 17.0, 40.0, 12.0),
    ("6204", 20.0, 47.0, 14.0),
    ("6205", 25.0, 52.0, 15.0),
    ("6206", 30.0, 62.0, 16.0),
    ("6207", 35.0, 72.0, 17.0),
    ("6208", 40.0, 80.0, 18.0),
    ("6209", 45.0, 85.0, 19.0),
    ("6210", 50.0, 90.0, 20.0),
    ("6300", 10.0, 35.0, 11.0),
    ("6301", 12.0, 37.0, 12.0),
    ("6302", 15.0, 42.0, 13.0),
    ("6303", 17.0, 47.0, 14.0),
    ("6304", 20.0, 52.0, 15.0),
    ("6305", 25.0, 62.0, 17.0),
    ("6306", 30.0, 72.0, 19.0),
    ("6307", 35.0, 80.0, 21.0),
    ("6308", 40.0, 90.0, 23.0),
    ("6309", 45.0, 100.0, 25.0),
    ("6310", 50.0, 110.0, 27.0),
];
// [b, h, shaft from, shaft to, t1, t2]
const KEYS: [(f64, f64, f64, f64, f64, f64); 16] = [
    (2.0, 2.0, 6.0, 8.0, 1.2, 1.0),
    (3.0, 3.0, 8.0, 10.0, 1.8, 1.4),
    (4.0, 4.0, 10.0, 12.0, 2.5, 1.8),
    (5.0, 5.0, 12.0, 17.0, 3.0, 2.3),
    (6.0, 6.0, 17.0, 22.0, 3.5, 2.8),
    (8.0, 7.0, 22.0, 30.0, 4.0, 3.3),
    (10.0, 8.0, 30.0, 38.0, 5.0, 3.3),
    (12.0, 8.0, 38.0, 44.0, 5.0, 3.3),
    (14.0, 9.0, 44.0, 50.0, 5.5, 3.8),
    (16.0, 10.0, 50.0, 58.0, 6.0, 4.3),
    (18.0, 11.0, 58.0, 65.0, 7.0, 4.4),
    (20.0, 12.0, 65.0, 75.0, 7.5, 4.9),
    (22.0, 14.0, 75.0, 85.0, 9.0, 5.4),
    (25.0, 14.0, 85.0, 95.0, 9.0, 5.4),
    (28.0, 16.0, 95.0, 110.0, 10.0, 6.4),
    (32.0, 18.0, 110.0, 130.0, 11.0, 7.4),
];
const MODULES: [f64; 13] = [0.5, 0.8, 1.0, 1.25, 1.5, 2.0, 2.5, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0];
const R_LENGTHS: [f64; 42] = [
    6.0, 8.0, 10.0, 12.0, 14.0, 16.0, 18.0, 20.0, 22.0, 25.0, 28.0, 30.0, 32.0, 35.0, 36.0, 40.0, 45.0, 50.0, 55.0, 56.0, 60.0, 63.0, 65.0,
    70.0, 80.0, 90.0, 100.0, 110.0, 120.0, 125.0, 130.0, 140.0, 150.0, 160.0, 180.0, 200.0, 220.0, 240.0, 250.0, 260.0, 280.0, 300.0,
];
const KEY_LENGTHS: [f64; 34] = [
    6.0, 8.0, 10.0, 12.0, 14.0, 16.0, 18.0, 20.0, 22.0, 25.0, 28.0, 32.0, 36.0, 40.0, 45.0, 50.0, 56.0, 63.0, 70.0, 80.0, 90.0, 100.0,
    110.0, 125.0, 140.0, 160.0, 180.0, 200.0, 220.0, 250.0, 280.0, 320.0, 360.0, 400.0,
];
const INCH_LENGTHS: [f64; 18] = [0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 3.5, 4.0, 4.5, 5.0, 6.0, 7.0, 8.0, 10.0, 12.0];

fn near(list: &[f64], v: f64) -> f64 {
    let mut a = list.first().copied().unwrap_or(f64::NAN);
    for &b in list {
        if (b - v).abs() < (a - v).abs() {
            a = b;
        }
    }
    a
}

/// One option of a family.
pub struct Opt {
    pub key: &'static str,
    pub unit: &'static str,
    pub min: f64,
    pub max: f64,
    pub integer: bool,
}
/// A family.
pub struct Family {
    pub id: &'static str,
    pub ty: &'static str,
    pub name: &'static str,
    pub standard: &'static str,
    pub units: &'static str,
    pub material: &'static str,
    pub options: &'static [Opt],
}
const fn opt(key: &'static str, unit: &'static str, min: f64, max: f64) -> Opt {
    Opt { key, unit, min, max, integer: false }
}
pub const PART_FAMILIES: [Family; 10] = [
    Family {
        id: "iso4017",
        ty: "hex-screw",
        name: "Hex head screw",
        standard: "ISO 4017",
        units: "mm",
        material: "Steel, property class 8.8",
        options: &[opt("length", "mm", 4.0, 500.0)],
    },
    Family {
        id: "iso4014",
        ty: "hex-bolt",
        name: "Hex bolt",
        standard: "ISO 4014",
        units: "mm",
        material: "Steel, property class 8.8",
        options: &[opt("length", "mm", 20.0, 500.0)],
    },
    Family {
        id: "iso4032",
        ty: "hex-nut",
        name: "Hex nut",
        standard: "ISO 4032",
        units: "mm",
        material: "Steel, property class 8",
        options: &[],
    },
    Family {
        id: "iso7089",
        ty: "washer",
        name: "Plain washer",
        standard: "ISO 7089",
        units: "mm",
        material: "Steel, 200 HV",
        options: &[],
    },
    Family {
        id: "asme-hex-bolt",
        ty: "hex-bolt",
        name: "Hex bolt",
        standard: "ASME B18.2.1",
        units: "in",
        material: "Steel",
        options: &[opt("length", "in", 0.25, 24.0)],
    },
    Family { id: "asme-hex-nut", ty: "hex-nut", name: "Hex nut", standard: "ASME B18.2.2", units: "in", material: "Steel", options: &[] },
    Family {
        id: "iso2338",
        ty: "pin",
        name: "Parallel pin",
        standard: "ISO 2338",
        units: "mm",
        material: "Steel, unhardened",
        options: &[opt("length", "mm", 4.0, 300.0)],
    },
    Family {
        id: "iso15",
        ty: "bearing",
        name: "Deep groove ball bearing",
        standard: "ISO 15",
        units: "mm",
        material: "Bearing steel",
        options: &[],
    },
    Family {
        id: "din6885",
        ty: "key",
        name: "Parallel key",
        standard: "DIN 6885-1",
        units: "mm",
        material: "Steel, key steel",
        options: &[opt("length", "mm", 4.0, 800.0)],
    },
    Family {
        id: "spur-gear",
        ty: "gear",
        name: "Spur gear",
        standard: "ISO 54",
        units: "mm",
        material: "Steel",
        options: &[
            Opt { key: "teeth", unit: "", min: 6.0, max: 400.0, integer: true },
            opt("width", "mm", 1.0, 1000.0),
            opt("bore", "mm", 0.0, 5000.0),
        ],
    },
];
pub fn find_family(id: &str) -> Option<&'static Family> {
    PART_FAMILIES.iter().find(|f| f.id == id)
}

fn n(x: f64) -> Value {
    jv::num(x)
}
fn g(z: &Value, k: &str) -> f64 {
    jv::as_num(z.get(k)).unwrap_or(f64::NAN)
}

/// `partSizes(id)`.
pub fn part_sizes(id: &str) -> Vec<Value> {
    sizes_ref(id).to_vec()
}
/// The size table of a family, built once (the tables are constants).
fn sizes_ref(id: &str) -> &'static [Value] {
    static CACHE: std::sync::OnceLock<Vec<(&'static str, Vec<Value>)>> = std::sync::OnceLock::new();
    let all = CACHE.get_or_init(|| PART_FAMILIES.iter().map(|f| (f.id, build_sizes(f.id))).collect());
    all.iter().find(|(k, _)| *k == id).map_or(&[], |(_, v)| v.as_slice())
}
fn build_sizes(id: &str) -> Vec<Value> {
    match id {
        "iso4014" | "iso4017" => METRIC
            .iter()
            .map(|m| jv::obj(vec![("size", m.0.into()), ("label", m.0.into()), ("d", n(m.1)), ("P", n(m.2)), ("s", n(m.3)), ("k", n(m.4))]))
            .collect(),
        "iso4032" => METRIC
            .iter()
            .map(|m| jv::obj(vec![("size", m.0.into()), ("label", m.0.into()), ("d", n(m.1)), ("P", n(m.2)), ("s", n(m.3)), ("m", n(m.5))]))
            .collect(),
        "iso7089" => METRIC
            .iter()
            .map(|m| {
                let s = js::num_str(m.1);
                jv::obj(vec![
                    ("size", s.as_str().into()),
                    ("label", s.as_str().into()),
                    ("thread", m.0.into()),
                    ("d1", n(m.6)),
                    ("d2", n(m.7)),
                    ("h", n(m.8)),
                ])
            })
            .collect(),
        "asme-hex-bolt" => INCH
            .iter()
            .map(|r| {
                jv::obj(vec![
                    ("size", r.0.into()),
                    ("label", format!("{}-{} UNC", r.0, js::num_str(r.2)).into()),
                    ("d", n(r.1 * IN)),
                    ("tpi", n(r.2)),
                    ("P", n(IN / r.2)),
                    ("s", n(r.3 * IN)),
                    ("k", n(r.4 * IN)),
                ])
            })
            .collect(),
        "asme-hex-nut" => INCH
            .iter()
            .map(|r| {
                jv::obj(vec![
                    ("size", r.0.into()),
                    ("label", format!("{}-{} UNC", r.0, js::num_str(r.2)).into()),
                    ("d", n(r.1 * IN)),
                    ("tpi", n(r.2)),
                    ("P", n(IN / r.2)),
                    ("s", n(r.3 * IN)),
                    ("m", n(r.5 * IN)),
                ])
            })
            .collect(),
        "iso2338" => PINS
            .iter()
            .map(|(d, c)| {
                let s = js::num_str(*d);
                jv::obj(vec![("size", s.as_str().into()), ("label", s.as_str().into()), ("d", n(*d)), ("c", n(*c))])
            })
            .collect(),
        "iso15" => BEARINGS
            .iter()
            .map(|b| jv::obj(vec![("size", b.0.into()), ("label", b.0.into()), ("d", n(b.1)), ("D", n(b.2)), ("B", n(b.3))]))
            .collect(),
        "din6885" => KEYS
            .iter()
            .map(|k| {
                jv::obj(vec![
                    ("size", format!("{}x{}", js::num_str(k.0), js::num_str(k.1)).into()),
                    ("label", format!("{} × {}", js::num_str(k.0), js::num_str(k.1)).into()),
                    ("b", n(k.0)),
                    ("h", n(k.1)),
                    ("shaft", jv::nums(&[k.2, k.3])),
                    ("t1", n(k.4)),
                    ("t2", n(k.5)),
                ])
            })
            .collect(),
        "spur-gear" => MODULES
            .iter()
            .map(|m| {
                jv::obj(vec![
                    ("size", format!("m{}", js::num_str(*m)).into()),
                    ("label", format!("m {}", js::num_str(*m)).into()),
                    ("m", n(*m)),
                ])
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn size_row(id: &str, size: &str) -> Option<Value> {
    sizes_ref(id).iter().find(|z| jv::is_str(z.get("size"), size)).cloned()
}

/// `partOptions(id, size)`.
pub fn part_options(id: &str, size: &str) -> Value {
    let Some(z) = size_row(id, size) else { return jv::obj(vec![]) };
    match id {
        "iso4017" => jv::obj(vec![("length", n(near(&R_LENGTHS, 4.0 * g(&z, "d"))))]),
        "iso4014" => jv::obj(vec![("length", n(near(&R_LENGTHS, js::max(6.0 * g(&z, "d"), 2.0 * g(&z, "d") + 6.0 + 10.0))))]),
        "asme-hex-bolt" => jv::obj(vec![("length", n(near(&INCH_LENGTHS, 4.0 * g(&z, "d") / IN)))]),
        "iso2338" => jv::obj(vec![("length", n(near(&R_LENGTHS, 5.0 * g(&z, "d"))))]),
        "din6885" => jv::obj(vec![("length", n(near(&KEY_LENGTHS, 4.0 * g(&z, "b"))))]),
        "spur-gear" => {
            let teeth = 20.0;
            let m = g(&z, "m");
            jv::obj(vec![("teeth", n(teeth)), ("width", n(10.0 * m)), ("bore", n(js::max(0.0, js::round(m * (teeth - 2.5) / 3.0))))])
        }
        _ => jv::obj(vec![]),
    }
}

fn fmt(v: f64) -> String {
    js::num_str(js::fixed_num(v, 3))
}
fn inch_text(v: f64) -> String {
    let nn = js::round(v * 64.0);
    let w = (nn / 64.0).floor();
    let mut num = nn - w * 64.0;
    let mut den = 64.0;
    let mut guard = 0u32;
    while num != 0.0 && num % 2.0 == 0.0 && guard < 64 {
        num /= 2.0;
        den /= 2.0;
        guard = guard.saturating_add(1);
    }
    if num != 0.0 {
        format!("{}{}/{}", if w != 0.0 { format!("{}-", js::num_str(w)) } else { String::new() }, js::num_str(num), js::num_str(den))
    } else {
        js::num_str(w)
    }
}
fn hex_area(s: f64) -> f64 {
    (js::sqrt(3.0) / 2.0) * s * s
}
fn kg(mm3: f64) -> f64 {
    js::fixed_num(mm3 * 1e-9 * STEEL_DENSITY, 4)
}
/// `Number(v)` for an option value.
fn js_number(v: &Value) -> f64 {
    match v {
        Value::Number(x) => x.as_f64().unwrap_or(f64::NAN),
        Value::String(s) => js::str_num(s),
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::Null => 0.0,
        Value::Array(a) => match a.as_slice() {
            [] => 0.0,
            [one] => js_number(&Value::String(js::string_of(Some(one)))),
            _ => f64::NAN,
        },
        Value::Object(_) => f64::NAN,
    }
}

fn bad(key: &str) -> Value {
    jv::obj(vec![("ok", false.into()), ("error", format!("bad {key}").into()), ("key", key.into())])
}

/// `makePart(id, size, opts)` → { ok, part | error }.
pub fn make_part(id: &str, size: &Value, opts: &Value) -> Value {
    let Some(fam) = find_family(id) else { return jv::obj(vec![("ok", false.into()), ("error", "unknown family".into())]) };
    let size_s = js::string_of(Some(size));
    let Some(z) = size_row(id, &size_s) else { return jv::obj(vec![("ok", false.into()), ("error", "unknown size".into())]) };
    let zs = js::str_of(z.get("size"));
    let mut o = part_options(id, &zs);
    for spec in fam.options {
        let x = opts.get(spec.key);
        match x {
            None | Some(Value::Null) => continue,
            Some(Value::String(s)) if s.is_empty() => continue,
            _ => {}
        }
        let val = js_number(x.unwrap_or(&Value::Null));
        if !val.is_finite() || val < spec.min || val > spec.max || (spec.integer && !js::is_integer(val)) {
            return bad(spec.key);
        }
        jv::set(&mut o, spec.key, n(val));
    }
    let (title, url) = standard(fam.standard).unwrap_or(("", ""));
    let og = |k: &str| g(&o, k);
    let (d, s, k) = (g(&z, "d"), g(&z, "s"), g(&z, "k"));
    let e = s * 2.0 / js::sqrt(3.0);
    let pi = std::f64::consts::PI;
    let (dims, size_text, designation, mass, bx): (Value, String, String, Option<f64>, (f64, f64, f64));
    match id {
        "iso4017" | "iso4014" => {
            let l = og("length");
            if !(l > k) {
                return bad("length");
            }
            let b = if id == "iso4014" {
                if l <= 125.0 {
                    2.0 * d + 6.0
                } else if l <= 200.0 {
                    2.0 * d + 12.0
                } else {
                    2.0 * d + 25.0
                }
            } else {
                l
            };
            if id == "iso4014" && !(b < l) {
                return bad("length");
            }
            dims = jv::obj(vec![("d", n(d)), ("P", n(g(&z, "P"))), ("s", n(s)), ("e", n(e)), ("k", n(k)), ("l", n(l)), ("b", n(b))]);
            size_text = format!("{zs} × {}", fmt(l));
            designation = format!("{} {size_text}", fam.standard);
            mass = Some(kg(hex_area(s) * k + pi / 4.0 * d * d * l));
            bx = (e, k + l, s);
        }
        "asme-hex-bolt" => {
            let lin = og("length");
            let l = lin * IN;
            let b = js::min(l, (if lin <= 6.0 { 2.0 * d / IN + 0.25 } else { 2.0 * d / IN + 0.5 }) * IN);
            dims = jv::obj(vec![
                ("d", n(d)),
                ("P", n(g(&z, "P"))),
                ("tpi", n(g(&z, "tpi"))),
                ("s", n(s)),
                ("e", n(e)),
                ("k", n(k)),
                ("l", n(l)),
                ("b", n(b)),
            ]);
            size_text = format!("{} × {} in", js::str_of(z.get("label")), inch_text(lin));
            designation = format!("{size_text}, {}", fam.standard);
            mass = Some(kg(hex_area(s) * k + pi / 4.0 * d * d * l));
            bx = (e, k + l, s);
        }
        "iso4032" | "asme-hex-nut" => {
            let m = g(&z, "m");
            let mut dv = jv::obj(vec![("d", n(d)), ("P", n(g(&z, "P"))), ("s", n(s)), ("e", n(e)), ("m", n(m))]);
            if jv::truthy(z.get("tpi")) {
                jv::set(&mut dv, "tpi", n(g(&z, "tpi")));
            }
            dims = dv;
            size_text = if id == "iso4032" { zs.clone() } else { js::str_of(z.get("label")) };
            designation = if id == "iso4032" {
                format!("{} {zs}", fam.standard)
            } else {
                format!("{}, {}", js::str_of(z.get("label")), fam.standard)
            };
            mass = Some(kg((hex_area(s) - pi / 4.0 * d * d) * m));
            bx = (e, e, m);
        }
        "iso7089" => {
            let (d1, d2, h) = (g(&z, "d1"), g(&z, "d2"), g(&z, "h"));
            dims = jv::obj(vec![("d1", n(d1)), ("d2", n(d2)), ("h", n(h))]);
            size_text = zs.clone();
            designation = format!("{} {zs}", fam.standard);
            mass = Some(kg(pi / 4.0 * (d2 * d2 - d1 * d1) * h));
            bx = (d2, d2, h);
        }
        "iso2338" => {
            let l = og("length");
            let c = g(&z, "c");
            if !(l > 2.0 * c) {
                return bad("length");
            }
            dims = jv::obj(vec![("d", n(d)), ("c", n(c)), ("l", n(l))]);
            size_text = format!("{zs} × {}", fmt(l));
            designation = format!("{} {size_text}", fam.standard);
            mass = Some(kg(pi / 4.0 * d * d * l));
            bx = (d, l, d);
        }
        "iso15" => {
            let (dd, bb) = (g(&z, "D"), g(&z, "B"));
            dims = jv::obj(vec![("d", n(d)), ("D", n(dd)), ("B", n(bb))]);
            size_text = format!("{zs} ({} × {} × {})", js::num_str(d), js::num_str(dd), js::num_str(bb));
            designation = size_text.clone();
            mass = None;
            bx = (dd, dd, bb);
        }
        "din6885" => {
            let l = og("length");
            let (b, h) = (g(&z, "b"), g(&z, "h"));
            if !(l > b) {
                return bad("length");
            }
            dims = jv::obj(vec![
                ("b", n(b)),
                ("h", n(h)),
                ("l", n(l)),
                ("t1", n(g(&z, "t1"))),
                ("t2", n(g(&z, "t2"))),
                ("shaft", z.get("shaft").cloned().unwrap_or(Value::Null)),
            ]);
            size_text = format!("A {} × {} × {}", js::num_str(b), js::num_str(h), fmt(l));
            designation = format!("{} {size_text}", fam.standard);
            mass = Some(kg((b * (l - b) + pi / 4.0 * b * b) * h));
            bx = (l, b, h);
        }
        _ => {
            let m = g(&z, "m");
            let teeth = og("teeth");
            let (dg, da, df) = (m * teeth, m * (teeth + 2.0), m * (teeth - 2.5));
            if !(og("bore") < df) {
                return bad("bore");
            }
            dims = jv::obj(vec![
                ("m", n(m)),
                ("z", n(teeth)),
                ("d", n(dg)),
                ("da", n(da)),
                ("df", n(df)),
                ("db", n(dg * js::cos(20.0 * pi / 180.0))),
                ("width", n(og("width"))),
                ("bore", n(og("bore"))),
                ("alpha", n(20.0)),
            ]);
            size_text = format!("m {}, z {}, {} wide", fmt(m), js::num_str(teeth), fmt(og("width")));
            designation = format!("{size_text}, ISO 53 / ISO 54");
            mass = None;
            bx = (da, da, og("width"));
        }
    }
    let part = jv::obj(vec![
        ("family", id.into()),
        ("type", fam.ty.into()),
        ("name", fam.name.into()),
        ("standard", fam.standard.into()),
        ("standardTitle", title.into()),
        ("url", url.into()),
        ("size", zs.as_str().into()),
        ("units", fam.units.into()),
        ("material", fam.material.into()),
        ("options", o),
        ("dims", dims),
        ("designation", designation.into()),
        ("sizeText", size_text.into()),
        ("massKg", mass.map_or(Value::Null, n)),
        ("box", jv::obj(vec![("w", n(bx.0)), ("d", n(bx.1)), ("h", n(bx.2))])),
    ]);
    jv::obj(vec![("ok", true.into()), ("part", part)])
}

/// `PART_FAMILIES` as JSON.
pub fn families_json() -> Value {
    Value::Array(
        PART_FAMILIES
            .iter()
            .map(|f| {
                jv::obj(vec![
                    ("id", f.id.into()),
                    ("type", f.ty.into()),
                    ("name", f.name.into()),
                    ("standard", f.standard.into()),
                    ("units", f.units.into()),
                    ("material", f.material.into()),
                    (
                        "options",
                        Value::Array(
                            f.options
                                .iter()
                                .map(|o| {
                                    let mut x =
                                        jv::obj(vec![("key", o.key.into()), ("unit", o.unit.into()), ("min", n(o.min)), ("max", n(o.max))]);
                                    if o.integer {
                                        jv::set(&mut x, "integer", true.into());
                                    }
                                    x
                                })
                                .collect(),
                        ),
                    ),
                ])
            })
            .collect(),
    )
}
