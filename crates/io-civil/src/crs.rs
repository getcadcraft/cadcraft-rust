//! crs.js — coordinate reference systems for CivilCraft: the EPSG catalog (crs-db.json), search and suggestions, .prj
//! matching, the project's `Frame` (lat/long, other systems, local / ground steps, scale factors), grid shift files.
//! The maths is cc-proj (proj4js 2.22.0 ported one for one).
//!
//! Systems travel as the JS shapes (`state.crs`, catalog entries, definition strings, null = WGS 84) in
//! [`serde_json::Value`]s; the catalog and the .prj WKT table are this app's own data files, included in the binary and
//! read on first use.

use cc_civil::i18n::t;
use cc_civil::js::{G, J, Obj, arr, hypot, max, min, num_to_string, numf};
use cc_civil::tr;
use cc_proj::transform::{Converter, JsNum};
use cc_proj::{Proj, Pt};
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

type R<T> = Result<T, String>;

pub const WGS84: &str = "+proj=longlat +datum=WGS84 +no_defs";
static DB: &str = include_str!("../../../assets/data/civilcraft/crs-db.json");
static WKT_DB: &str = include_str!("../../../assets/data/civilcraft/crs-wkt.json");

/// `METRES_PER[unit]`.
pub fn metres_per(u: &str) -> Option<f64> {
    match u {
        "m" => Some(1.0),
        "ft" => Some(0.3048),
        "ftUS" => Some(1200.0 / 3937.0),
        _ => None,
    }
}

/// A catalog entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub code: f64,
    pub name: String,
    pub proj4: String,
    pub unit: String,
    pub area: Option<[f64; 4]>,
    pub shift_accuracy: Option<f64>,
    pub geographic: bool,
    /// place in the catalog
    pub idx: usize,
    /// `rank(e)` and suggest's `rs(e)`, worked out once
    rk: f64,
    rks: f64,
}
impl J for Entry {
    fn j(&self) -> Value {
        Obj::new()
            .set("code", self.code)
            .set("name", &self.name)
            .set("proj4", &self.proj4)
            .set("unit", &self.unit)
            .set("area", self.area.map(|a| a.to_vec()))
            .set("shiftAccuracy", self.shift_accuracy)
            .set("geographic", self.geographic)
            .done()
    }
}
impl Entry {
    /// The entry as a JS object value (what the JS passes around).
    pub fn value(&self) -> Value {
        self.j()
    }
}

fn catalog_all() -> &'static Vec<Entry> {
    static C: OnceLock<Vec<Entry>> = OnceLock::new();
    C.get_or_init(|| {
        let v: Value = serde_json::from_str(DB).unwrap_or(Value::Null);
        let fields: Vec<String> = match v.get("fields").and_then(Value::as_array) {
            Some(f) => f.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect(),
            None => ["code", "name", "proj4", "unit", "area", "shiftAccuracy"].iter().map(|s| s.to_string()).collect(),
        };
        let fi = |k: &str| fields.iter().position(|f| f == k);
        arr(v.get("crs"))
            .iter()
            .enumerate()
            .map(|(idx, r)| {
                let g = |k: &str| fi(k).and_then(|i| r.get(i)).filter(|x| !x.is_null());
                let unit = g("unit").and_then(Value::as_str).unwrap_or("").to_string();
                let area = g("area").and_then(Value::as_array).and_then(|a| {
                    if a.len() == 4 { Some([numf(a.first()), numf(a.get(1)), numf(a.get(2)), numf(a.get(3))]) } else { None }
                });
                let name = g("name").and_then(Value::as_str).unwrap_or("").to_string();
                Entry {
                    code: numf(g("code")),
                    proj4: g("proj4").and_then(Value::as_str).unwrap_or("").to_string(),
                    geographic: unit == "deg",
                    unit,
                    area,
                    shift_accuracy: g("shiftAccuracy").and_then(Value::as_f64),
                    idx,
                    rk: rank(&name),
                    rks: rank_s(&name),
                    name,
                }
            })
            .collect()
    })
}
fn by_code_map() -> &'static HashMap<u64, usize> {
    static M: OnceLock<HashMap<u64, usize>> = OnceLock::new();
    M.get_or_init(|| {
        let mut m = HashMap::new();
        for (i, e) in catalog_all().iter().enumerate() {
            // a later entry with the same code wins (new Map(entries))
            m.insert(e.code.to_bits(), i);
        }
        m
    })
}

static LOADED: AtomicBool = AtomicBool::new(false);
static WKT_LOADED: AtomicBool = AtomicBool::new(false);

/// `setCatalog(json)` (the app's own crs-db.json).
pub fn set_catalog() -> &'static Vec<Entry> {
    LOADED.store(true, Ordering::Relaxed);
    catalog_all()
}
pub fn catalog_loaded() -> bool {
    LOADED.load(Ordering::Relaxed)
}
/// `setWktCatalog(json)`.
pub fn set_wkt_catalog() {
    WKT_LOADED.store(true, Ordering::Relaxed);
}
fn catalog() -> R<&'static Vec<Entry>> {
    if !catalog_loaded() {
        return Err(t("crs.notloaded", &[]));
    }
    Ok(catalog_all())
}

/// `byCode(code)` (Number(code); unknown → None).
pub fn by_code(code: f64) -> R<Option<&'static Entry>> {
    if !catalog_loaded() {
        return Err(t("crs.notloaded", &[]));
    }
    let key = if code == 0.0 { 0.0f64.to_bits() } else { code.to_bits() };
    Ok(by_code_map().get(&key).and_then(|&i| catalog_all().get(i)))
}
/// `byCode(v)` for a JS value (Number(v)).
pub fn by_code_val(v: Option<&Value>) -> R<Option<&'static Entry>> {
    by_code(cc_civil::js::number(v))
}

fn in_area(a: Option<&[f64; 4]>, lon: f64, lat: f64) -> bool {
    match a {
        None => false,
        Some(a) => lat >= a[1] && lat <= a[3] && (if a[0] <= a[2] { lon >= a[0] && lon <= a[2] } else { lon >= a[0] || lon <= a[2] }),
    }
}
fn area_size(a: Option<&[f64; 4]>) -> f64 {
    match a {
        Some(a) => {
            let w = (a[2] - a[0] + 360.0) % 360.0;
            let w = if w == 0.0 || w.is_nan() { 360.0 } else { w };
            w * (a[3] - a[1])
        }
        None => 1e9,
    }
}
pub fn in_area_v(a: Option<&[f64; 4]>, lon: f64, lat: f64) -> bool {
    in_area(a, lon, lat)
}
pub fn area_size_v(a: Option<&[f64; 4]>) -> f64 {
    area_size(a)
}

fn re(p: &str) -> Regex {
    // the patterns are constants: a bad one is a programming error caught by the tests; fall back to a never-match
    Regex::new(p).unwrap_or_else(|_| Regex::new("$^").unwrap_or_else(|_| never()))
}
#[allow(clippy::expect_used)]
fn never() -> Regex {
    Regex::new("a^").expect("a constant pattern")
}

/// rank(e) of crs.js.
pub fn rank(name: &str) -> f64 {
    static P: OnceLock<[Regex; 4]> = OnceLock::new();
    let p = P.get_or_init(|| {
        [re(r"^NAD83\(2011\)"), re(r"^NAD83 /"), re(r"^WGS 84"), re(r"^(ETRS89|GDA2020|NZGD2000|NAD83\(CSRS\)|RGF93|OSGB36)")]
    });
    if p[0].is_match(name) {
        0.0
    } else if p[1].is_match(name) {
        1.0
    } else if p[2].is_match(name) {
        2.0
    } else if p[3].is_match(name) {
        3.0
    } else {
        5.0
    }
}

fn sort_js<T>(v: &mut [T], mut cmp: impl FnMut(&T, &T) -> f64) {
    cc_civil::js::sort_by(v, |a, b| cmp(a, b));
}

/// suggest's `rs(e)`.
fn rank_s(name: &str) -> f64 {
    static P: OnceLock<[Regex; 2]> = OnceLock::new();
    let p = P.get_or_init(|| [re(r"^WGS 84 / UTM zone"), re(r"^WGS 84\b")]);
    if p[0].is_match(name) {
        2.0
    } else if p[1].is_match(name) {
        6.0
    } else {
        rank(name)
    }
}

// ---- localeCompare with the collation elements worked out once per name. A private copy of cc-civil's js.rs
// `collation_elements` / `collate_with` (ICU root order for the text the catalog holds), so the whole catalog sorts
// in milliseconds instead of re-deriving both names on every comparison.

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Prim {
    Sym(u16),
    Num(usize, String),
    Letter(u8),
    Other(u32),
}
const SYMBOLS: &str = "\t\n\u{b}\u{c}\r \u{a0}_-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";
fn latin1_base(c: char) -> Option<(&'static str, bool)> {
    let (b, up) = match c {
        'À'..='Å' => ("a", true),
        'à'..='å' => ("a", false),
        'Æ' => ("ae", true),
        'æ' => ("ae", false),
        'Ç' => ("c", true),
        'ç' => ("c", false),
        'È'..='Ë' => ("e", true),
        'è'..='ë' => ("e", false),
        'Ì'..='Ï' => ("i", true),
        'ì'..='ï' => ("i", false),
        'Ñ' => ("n", true),
        'ñ' => ("n", false),
        'Ò'..='Ö' | 'Ø' => ("o", true),
        'ò'..='ö' | 'ø' => ("o", false),
        'Ù'..='Ü' => ("u", true),
        'ù'..='ü' => ("u", false),
        'Ý' => ("y", true),
        'ý' | 'ÿ' => ("y", false),
        'ß' => ("ss", false),
        _ => return None,
    };
    Some((b, up))
}
type Coll = Vec<(Prim, u32, u8)>;
fn collation_elements(s: &str) -> Coll {
    let mut out = Vec::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            out.push((Prim::Num(1, c.to_string()), 0, 0));
            continue;
        }
        let cp = u32::from(c);
        if let Some(k) = SYMBOLS.chars().position(|x| x == c) {
            out.push((Prim::Sym(u16::try_from(k).unwrap_or(u16::MAX)), 0, 0));
            continue;
        }
        if cp < 0x20 || (0x7F..=0x9F).contains(&cp) {
            continue;
        }
        if c.is_ascii_alphabetic() {
            let l = c.to_ascii_lowercase();
            out.push((Prim::Letter(u8::try_from(u32::from(l)).unwrap_or(0)), 0, u8::from(c.is_ascii_uppercase())));
            continue;
        }
        if let Some((base, up)) = latin1_base(c) {
            let sharp = c == 'ß';
            for (n, b) in base.chars().enumerate() {
                let sec = if base.len() == 1 { cp } else { 0 };
                let ter = u8::from(up) | if sharp && n == 0 { 2 } else { 0 };
                out.push((Prim::Letter(u8::try_from(u32::from(b)).unwrap_or(0)), sec, ter));
            }
            continue;
        }
        out.push((Prim::Other(cp), 0, 0));
    }
    out
}
fn collate(a: &str, ea: &Coll, b: &str, eb: &Coll) -> std::cmp::Ordering {
    let p = ea.iter().map(|e| &e.0).cmp(eb.iter().map(|e| &e.0));
    if p.is_ne() {
        return p;
    }
    let s = ea.iter().map(|e| e.1).filter(|&x| x != 0).cmp(eb.iter().map(|e| e.1).filter(|&x| x != 0));
    if s.is_ne() {
        return s;
    }
    let t = ea.iter().map(|e| e.2).cmp(eb.iter().map(|e| e.2));
    if t.is_ne() {
        return t;
    }
    a.cmp(b)
}
/// Each entry's place in the catalog sorted by `rank || localeCompare` (ties keep catalog order, as a stable sort does).
fn name_order() -> &'static Vec<u32> {
    static O: OnceLock<Vec<u32>> = OnceLock::new();
    O.get_or_init(|| {
        let cat = catalog_all();
        let keys: Vec<Coll> = cat.iter().map(|e| collation_elements(&e.name)).collect();
        let mut ix: Vec<usize> = (0..cat.len()).collect();
        ix.sort_by(|&a, &b| {
            let (ea, eb) = (cat.get(a), cat.get(b));
            let (Some(ea), Some(eb)) = (ea, eb) else { return std::cmp::Ordering::Equal };
            ea.rk
                .partial_cmp(&eb.rk)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| match (keys.get(a), keys.get(b)) {
                    (Some(ka), Some(kb)) => collate(&ea.name, ka, &eb.name, kb),
                    _ => std::cmp::Ordering::Equal,
                })
                .then(a.cmp(&b))
        });
        let mut order = vec![0u32; cat.len()];
        for (p, &i) in ix.iter().enumerate() {
            if let Some(o) = order.get_mut(i) {
                *o = u32::try_from(p).unwrap_or(u32::MAX);
            }
        }
        order
    })
}

/// `search(text, { near, limit })`.
pub fn search(text: &str, near: Option<[f64; 2]>, limit: f64) -> R<Vec<&'static Entry>> {
    let cat = catalog()?;
    let q0 = cc_civil::js::trim(text).to_lowercase();
    static EP: OnceLock<Regex> = OnceLock::new();
    let q = EP.get_or_init(|| re(r"^epsg:\s*")).replace(&q0, "").into_owned();
    let mut hits: Vec<&Entry>;
    let numeric = !q.is_empty() && q.bytes().all(|b| b.is_ascii_digit());
    if numeric {
        let qn = cc_civil::js::str_to_number(&q);
        hits = cat.iter().filter(|e| num_to_string(e.code).starts_with(&q)).collect();
        sort_js(&mut hits, |a, b| {
            if a.code == qn {
                -1.0
            } else if b.code == qn {
                1.0
            } else {
                a.code - b.code
            }
        });
    } else {
        static SP: OnceLock<Regex> = OnceLock::new();
        let words: Vec<&str> = SP.get_or_init(|| re(r"[\s/,()]+")).split(&q).filter(|w| !w.is_empty()).collect();
        hits = cat
            .iter()
            .filter(|e| {
                let n = e.name.to_lowercase();
                words.iter().all(|w| n.contains(w))
            })
            .collect();
        // `rank(a) - rank(b) || a.name.localeCompare(b.name)`: a total order, so a stable sort of any subset follows
        // the order of the whole catalog sorted once
        let order = name_order();
        let pos = |e: &Entry| order.get(e.idx).copied().unwrap_or(u32::MAX);
        sort_js(&mut hits, |a, b| f64::from(pos(a)) - f64::from(pos(b)));
    }
    if let Some([lon, lat]) = near {
        sort_js(&mut hits, |a, b| {
            f64::from(u8::from(in_area(b.area.as_ref(), lon, lat))) - f64::from(u8::from(in_area(a.area.as_ref(), lon, lat)))
        });
    }
    let lim = cc_civil::js::usize_of(limit.clamp(0.0, 1e9)).unwrap_or(0);
    hits.truncate(lim);
    Ok(hits)
}

/// `suggest([lon, lat], { unit, limit })`.
pub fn suggest(lon: f64, lat: f64, unit: Option<&str>, limit: f64) -> R<Vec<&'static Entry>> {
    let cat = catalog()?;
    let fam: Option<Vec<&str>> = match unit {
        Some("m") => Some(vec!["m"]),
        Some("ft") => Some(vec!["ft", "ftUS"]),
        _ => None,
    };
    let rs = |e: &Entry| e.rks;
    let mut out: Vec<&Entry> = cat
        .iter()
        .filter(|e| !e.geographic && in_area(e.area.as_ref(), lon, lat) && fam.as_ref().is_none_or(|f| f.contains(&e.unit.as_str())))
        .collect();
    sort_js(&mut out, |a, b| {
        let d = rs(a) - rs(b);
        if d != 0.0 && !d.is_nan() { d } else { area_size(a.area.as_ref()) - area_size(b.area.as_ref()) }
    });
    out.truncate(cc_civil::js::usize_of(limit.clamp(0.0, 1e9)).unwrap_or(0));
    Ok(out)
}

/// `crsFromEntry(e)` → state.crs.
pub fn crs_from_entry(e: &Entry) -> Value {
    json!({ "code": e.code.j(), "name": e.name, "proj4": e.proj4, "unit": if e.unit == "deg" { "deg".to_string() } else { e.unit.clone() }, "local": null })
}

// ------------------------------------------------------------------ grid shift files (0.4 V11)

/// A loaded grid's facts.
#[derive(Clone, Debug, PartialEq)]
pub struct GridInfo {
    pub name: String,
    pub kind: String,
    pub extent: [f64; 4],
    pub subgrids: usize,
    pub size: usize,
}
impl J for GridInfo {
    fn j(&self) -> Value {
        json!({ "name": self.name, "kind": self.kind, "extent": self.extent.to_vec().j(), "subgrids": self.subgrids, "size": self.size })
    }
}
fn grids() -> &'static Mutex<Vec<(String, GridInfo)>> {
    static G: OnceLock<Mutex<Vec<(String, GridInfo)>>> = OnceLock::new();
    G.get_or_init(|| Mutex::new(Vec::new()))
}
fn grids_has(name: &str) -> bool {
    grids().lock().unwrap_or_else(PoisonError::into_inner).iter().any(|(n, _)| n == name)
}
pub fn grid_info(name: &str) -> Option<GridInfo> {
    grids().lock().unwrap_or_else(PoisonError::into_inner).iter().find(|(n, _)| n == name).map(|(_, g)| g.clone())
}
pub fn grids_loaded() -> Vec<String> {
    grids().lock().unwrap_or_else(PoisonError::into_inner).iter().map(|(n, _)| n.clone()).collect()
}

/// GRID_SUGGEST (labels in the page's words).
pub fn grid_suggest() -> Vec<Value> {
    vec![
        json!({ "name": "us_noaa_nadcon5_nad27_nad83_1986_conus.tif", "datum": "NAD27", "area": [-125, 24, -66, 50], "accuracy": 0.1, "size": 139914, "label": t("grid.s.conus", &[]) }),
        json!({ "name": "us_noaa_alaska.tif", "datum": "NAD27", "area": [-194, 46, -128, 77], "accuracy": 0.5, "size": 524798, "label": t("grid.s.alaska", &[]) }),
        json!({ "name": "ca_nrc_ntv2_0.tif", "datum": "NAD27", "area": [-142, 40, -47, 84], "accuracy": 0.1, "size": 7784410, "label": t("grid.s.canada", &[]) }),
    ]
}
pub const GRID_CDN: &str = "https://cdn.proj.org/";

/// `suggestGrids(crs)`.
pub fn suggest_grids(crs: &Value) -> R<Vec<Value>> {
    if !cc_civil::js::truthy(Some(crs)) {
        return Ok(Vec::new());
    }
    static N: OnceLock<Regex> = OnceLock::new();
    let name = crs.get("name").filter(|v| cc_civil::js::truthy(Some(v))).map_or_else(String::new, cc_civil::js::to_js_string);
    if !N.get_or_init(|| re(r"(?i)NAD27")).is_match(&name) {
        return Ok(Vec::new());
    }
    let e = by_code_val(crs.get("code"))?.and_then(|e| e.area);
    let all = grid_suggest();
    let Some(e) = e else { return Ok(all) };
    let cx = (e[0] + e[2]) / 2.0;
    let cy = (e[1] + e[3]) / 2.0;
    Ok(all
        .into_iter()
        .filter(|g| {
            let a = arr(g.get("area"));
            let ga = |i: usize| numf(a.get(i));
            cx >= ga(0) && cx <= ga(2) && cy >= ga(1) && cy <= ga(3)
        })
        .collect())
}

fn safe_name(n: &str) -> String {
    let s: String = n.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' { c } else { '_' }).collect();
    cc_civil::js::slice16(&s, 80)
}

/// `loadGrid(name, bytes)`.
pub fn load_grid(name: &str, bytes: &[u8]) -> R<GridInfo> {
    let nm = safe_name(name);
    if nm.is_empty() {
        return Err(t("grid.err.name", &[]));
    }
    let head: String = bytes.iter().take(8).map(|&c| char::from(c)).collect();
    let (g, kind) = if head == "NUM_OREC" {
        (cc_proj::nadgrid::read_ntv2(&nm, bytes)?, "ntv2")
    } else if head.starts_with("II*\u{0}") || head.starts_with("MM\u{0}*") {
        let tiff = cc_proj::tiff::Tiff::from_bytes(bytes)?;
        tiff.image_count_checked()?;
        (cc_proj::nadgrid::read_geotiff_grid(&nm, &tiff)?, "geotiff")
    } else {
        return Err(t("grid.err.kind", &[]));
    };
    if g.subgrids.is_empty() {
        return Err(t("grid.err.empty", &[]));
    }
    let d = 180.0 / std::f64::consts::PI;
    let (mut w, mut s, mut e, mut n) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for sg in &g.subgrids {
        let east = -sg.ll[0] * d;
        let west = -(sg.ll[0] + (sg.lim[0] - 1.0) * sg.del[0]) * d;
        let south = sg.ll[1] * d;
        let north = (sg.ll[1] + (sg.lim[1] - 1.0) * sg.del[1]) * d;
        w = min(w, west);
        e = max(e, east);
        s = min(s, south);
        n = max(n, north);
    }
    let info = GridInfo { name: nm.clone(), kind: kind.into(), extent: [w, s, e, n], subgrids: g.subgrids.len(), size: bytes.len() };
    {
        let mut gl = grids().lock().unwrap_or_else(PoisonError::into_inner);
        match gl.iter_mut().find(|(k, _)| *k == nm) {
            Some(slot) => slot.1 = info.clone(),
            None => gl.push((nm, info.clone())),
        }
    }
    cache().lock().unwrap_or_else(PoisonError::into_inner).clear();
    proj_cache().lock().unwrap_or_else(PoisonError::into_inner).clear();
    Ok(info)
}

/// `defFor(c)`: the definition actually used (None = undefined; Err = a non-text proj4).
pub fn def_for(c: &Value) -> Option<String> {
    let proj4 = c.get("proj4");
    // `GRIDS.has(c.grid)`: the Map's keys are strings
    let grid = c.get("grid").and_then(Value::as_str).filter(|g| !g.is_empty());
    let p4s = proj4.and_then(Value::as_str);
    match (grid, p4s) {
        (Some(g), Some(p)) if grids_has(g) && p.starts_with('+') => {
            static A: OnceLock<[Regex; 2]> = OnceLock::new();
            let r = A.get_or_init(|| [re(r"(?i)\+datum=NAD27\b"), re(r"\s*\+(?:towgs84|nadgrids)=\S+")]);
            let s1 = r[0].replace(p, "+ellps=clrk66").into_owned();
            let s2 = r[1].replace_all(&s1, "").into_owned();
            Some(format!("{s2} +nadgrids=@{g}"))
        }
        _ => proj4.map(|v| match v {
            Value::String(s) => s.clone(),
            other => cc_civil::js::to_js_string(other),
        }),
    }
}

/// `gridStatus(c, lon, lat)`.
pub fn grid_status(c: &Value, lon: Option<f64>, lat: Option<f64>) -> Value {
    let Some(g) = c.get("grid").filter(|g| cc_civil::js::truthy(Some(g))) else { return Value::Null };
    let name = cc_civil::js::to_js_string(g);
    match grid_info(&name) {
        None => json!({ "name": name, "loaded": false, "inside": null }),
        Some(gi) => {
            let inside = match (lon, lat) {
                (Some(lo), Some(la)) => json!(lo >= gi.extent[0] && lo <= gi.extent[2] && la >= gi.extent[1] && la <= gi.extent[3]),
                _ => Value::Null,
            };
            json!({ "name": name, "loaded": true, "inside": inside, "info": gi.j() })
        }
    }
}

type Cache = Mutex<Vec<(String, Arc<Converter>)>>;
fn cache() -> &'static Cache {
    static C: OnceLock<Cache> = OnceLock::new();
    C.get_or_init(|| Mutex::new(Vec::new()))
}

/// `converter(a, b)` (cached, 64 at most as the JS keeps).
pub fn converter(a: Option<&str>, b: Option<&str>) -> R<Arc<Converter>> {
    let key = format!("{}\u{0}{}", a.unwrap_or("undefined"), b.unwrap_or("undefined"));
    if let Some(c) = cache().lock().unwrap_or_else(PoisonError::into_inner).iter().find(|(k, _)| *k == key).map(|(_, c)| c.clone()) {
        return Ok(c);
    }
    let from = proj_of(a)?;
    let to = proj_of(b)?;
    let c = Arc::new(Converter::from_projs(from, to));
    let mut g = cache().lock().unwrap_or_else(PoisonError::into_inner);
    if g.len() > 64 {
        g.clear();
    }
    g.push((key, c.clone()));
    Ok(c)
}
fn proj_of(d: Option<&str>) -> R<Arc<Proj>> {
    match d {
        None => Err("Cannot use 'in' operator to search for 'projName' in undefined".into()),
        Some(s) => parsed(s),
    }
}

type ProjCache = Mutex<HashMap<String, R<Arc<Proj>>>>;
fn proj_cache() -> &'static ProjCache {
    static C: OnceLock<ProjCache> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}
/// `proj4.Proj(def)`, parsed once per definition (a projection is immutable once made; the grids a definition names
/// bind when it is parsed, so the cache is cleared when a grid is loaded, as the JS clears its converters).
pub fn parsed(def: &str) -> R<Arc<Proj>> {
    if let Some(p) = proj_cache().lock().unwrap_or_else(PoisonError::into_inner).get(def) {
        return p.clone();
    }
    let p = Proj::new(def).map(Arc::new);
    let mut c = proj_cache().lock().unwrap_or_else(PoisonError::into_inner);
    if c.len() > 20000 {
        c.clear();
    }
    c.insert(def.to_string(), p.clone());
    p
}

/// `defOf(c)`.
pub fn def_of(c: &Value) -> Option<String> {
    if !cc_civil::js::truthy(Some(c)) {
        return Some(WGS84.into());
    }
    match c {
        Value::String(s) => Some(s.clone()),
        other => def_for(other),
    }
}

/// An array result as numbers (undefined → NaN in arithmetic).
pub fn nn(v: [JsNum; 2]) -> [f64; 2] {
    [v[0].unwrap_or(f64::NAN), v[1].unwrap_or(f64::NAN)]
}

/// `transformer(from, to)(x, y)`.
pub fn transform(from: &Value, to: &Value, x: f64, y: f64) -> R<[JsNum; 2]> {
    let t = converter(def_of(from).as_deref(), def_of(to).as_deref())?;
    t.forward(x, y)
}

/// The `local` step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Local {
    pub scale: f64,
    pub rotation: f64,
    pub from: [f64; 2],
    pub to: [f64; 2],
}
pub fn read_local(v: Option<&Value>) -> Option<Local> {
    let l = v.filter(|l| l.is_object())?;
    let p = |k: &str| {
        let a = arr(l.get(k));
        [numf(a.first()), numf(a.get(1))]
    };
    Some(Local { scale: numf(l.get("scale")), rotation: numf(l.get("rotation")), from: p("from"), to: p("to") })
}
impl J for Local {
    fn j(&self) -> Value {
        json!({ "scale": self.scale.j(), "rotation": self.rotation.j(), "from": self.from.to_vec().j(), "to": self.to.to_vec().j() })
    }
}

/// `frame(crs)`.
#[derive(Clone, Debug)]
pub struct Frame {
    pub crs: Value,
    local: Option<Local>,
    geo: Arc<Converter>,
}

/// The `factors` result.
#[derive(Clone, Copy, Debug)]
pub struct Factors {
    pub k: f64,
    pub k_meridian: f64,
    pub k_parallel: f64,
    pub convergence: f64,
    pub elevation: f64,
    pub combined: f64,
    pub lon: f64,
    pub lat: f64,
}
impl J for Factors {
    fn j(&self) -> Value {
        json!({ "k": self.k.j(), "kMeridian": self.k_meridian.j(), "kParallel": self.k_parallel.j(), "convergence": self.convergence.j(),
            "elevation": self.elevation.j(), "combined": self.combined.j(), "lon": self.lon.j(), "lat": self.lat.j() })
    }
}

/// `frame(crs)` (None = null for a falsy crs).
pub fn frame(crs: &Value) -> R<Option<Frame>> {
    if !cc_civil::js::truthy(Some(crs)) {
        return Ok(None);
    }
    let local = read_local(crs.get("local"));
    let geo = converter(def_for(crs).as_deref(), Some(WGS84))?;
    Ok(Some(Frame { crs: crs.clone(), local, geo }))
}

fn deg_trig(rot: f64) -> (f64, f64) {
    use cc_civil::js::JsMath;
    let r = rot * std::f64::consts::PI / 180.0;
    (r.jcos(), r.jsin())
}

impl Frame {
    pub fn to_grid(&self, x: f64, y: f64) -> [f64; 2] {
        match &self.local {
            None => [x, y],
            Some(l) => {
                let (c, s) = deg_trig(l.rotation);
                let k = l.scale;
                let dx = x - l.from[0];
                let dy = y - l.from[1];
                [l.to[0] + k * (c * dx - s * dy), l.to[1] + k * (s * dx + c * dy)]
            }
        }
    }
    pub fn from_grid(&self, gx: f64, gy: f64) -> [f64; 2] {
        match &self.local {
            None => [gx, gy],
            Some(l) => {
                let (c, s) = deg_trig(l.rotation);
                let k = l.scale;
                let dx = (gx - l.to[0]) / k;
                let dy = (gy - l.to[1]) / k;
                [l.from[0] + c * dx + s * dy, l.from[1] - s * dx + c * dy]
            }
        }
    }
    pub fn to_lon_lat(&self, x: f64, y: f64) -> R<[JsNum; 2]> {
        let g = self.to_grid(x, y);
        self.geo.forward(g[0], g[1])
    }
    pub fn from_lon_lat(&self, lon: f64, lat: f64) -> R<[f64; 2]> {
        let g = nn(self.geo.inverse(lon, lat)?);
        Ok(self.from_grid(g[0], g[1]))
    }
    /// The converter `f.to(other)` builds when it is made (a definition proj4 cannot read throws there).
    pub fn check_to(&self, other: &Value) -> R<()> {
        if cc_civil::js::truthy(Some(other)) {
            converter(def_for(&self.crs).as_deref(), def_of(other).as_deref())?;
        }
        Ok(())
    }
    /// The converter `f.from(other)` builds when it is made.
    pub fn check_from(&self, other: &Value) -> R<()> {
        if cc_civil::js::truthy(Some(other)) {
            converter(def_of(other).as_deref(), def_for(&self.crs).as_deref())?;
        }
        Ok(())
    }
    /// `f.to(other)(x, y)`.
    pub fn to(&self, other: &Value, x: f64, y: f64) -> R<[JsNum; 2]> {
        if !cc_civil::js::truthy(Some(other)) {
            return self.to_lon_lat(x, y);
        }
        let t = converter(def_for(&self.crs).as_deref(), def_of(other).as_deref())?;
        let g = self.to_grid(x, y);
        t.forward(g[0], g[1])
    }
    /// `f.from(other)(x, y)`.
    pub fn from(&self, other: &Value, x: f64, y: f64) -> R<[f64; 2]> {
        if !cc_civil::js::truthy(Some(other)) {
            return self.from_lon_lat(x, y);
        }
        let t = converter(def_of(other).as_deref(), def_for(&self.crs).as_deref())?;
        let g = nn(t.forward(x, y)?);
        Ok(self.from_grid(g[0], g[1]))
    }
    /// `f.factors(x, y, z)`.
    pub fn factors(&self, x: f64, y: f64, z: Option<&Value>) -> R<Factors> {
        use cc_civil::js::JsMath;
        let p4 = self.crs.get("proj4").map(cc_civil::js::to_js_string).unwrap_or_else(|| "undefined".into());
        let p = parsed(&p4)?;
        let to_m = p.to_meter.filter(|v| *v != 0.0 && !v.is_nan()).unwrap_or(1.0);
        let g = self.to_grid(x, y);
        let ll = match p.inverse(Pt::xy(g[0] * to_m, g[1] * to_m))? {
            cc_proj::Out::P(q) => q,
            cc_proj::Out::Null => return Err("Cannot read properties of null (reading 'x')".into()),
            cc_proj::Out::Undef => return Err("Cannot read properties of undefined (reading 'x')".into()),
            cc_proj::Out::Num(_) => Pt::xy(f64::NAN, f64::NAN),
        };
        let lam = ll.x;
        let phi = ll.y;
        let d = 1e-5;
        let fwd = |a: f64, b: f64| -> R<Pt> {
            match p.forward(Pt::xy(a, b))? {
                cc_proj::Out::P(q) => Ok(q),
                cc_proj::Out::Null => Err("Cannot read properties of null (reading 'x')".into()),
                cc_proj::Out::Undef => Err("Cannot read properties of undefined (reading 'x')".into()),
                cc_proj::Out::Num(_) => Ok(Pt::xy(f64::NAN, f64::NAN)),
            }
        };
        let pn = fwd(lam, phi + d)?;
        let ps = fwd(lam, phi - d)?;
        let pe = fwd(lam + d, phi)?;
        let pw = fwd(lam - d, phi)?;
        let e2 = if p.es != 0.0 && !p.es.is_nan() { p.es } else { 0.0 };
        let w = (1.0 - e2 * cc_proj::math::pow(phi.jsin(), 2.0)).sqrt();
        let n = p.a / w;
        let m = p.a * (1.0 - e2) / cc_proj::math::pow(w, 3.0);
        let k_n = hypot(pn.x - ps.x, pn.y - ps.y) / (2.0 * m * d);
        let k_e = hypot(pe.x - pw.x, pe.y - pw.y) / (2.0 * n * phi.jcos() * d);
        let k = (k_n + k_e) / 2.0;
        let convergence = -(pn.x - ps.x).jatan2(pn.y - ps.y) * 180.0 / std::f64::consts::PI;
        let r = (m * n).sqrt();
        let unit = self.crs.get("unit").map(cc_civil::js::to_js_string).unwrap_or_default();
        let zz = cc_civil::js::number_or0(z);
        let h = zz * metres_per(&unit).unwrap_or(1.0);
        let elevation = r / (r + h);
        let ll2 = nn(self.geo.forward(g[0], g[1])?);
        Ok(Factors { k, k_meridian: k_n, k_parallel: k_e, convergence, elevation, combined: k * elevation, lon: ll2[0], lat: ll2[1] })
    }
}

/// `localFromPairs([[pLocal, pGrid], [pLocal, pGrid]])`.
pub fn local_from_pairs(pairs: &Value) -> R<Local> {
    use cc_civil::js::JsMath;
    let ps = pairs.as_array().filter(|a| a.len() == 2).ok_or_else(|| t("crs.pairs.two", &[]))?;
    let pt = |v: Option<&Value>| {
        let a = arr(v);
        [numf(a.first()), numf(a.get(1))]
    };
    let p0 = arr(ps.first());
    let p1 = arr(ps.get(1));
    let (a, aa, b, bb) = (pt(p0.first()), pt(p0.get(1)), pt(p1.first()), pt(p1.get(1)));
    let dl = [b[0] - a[0], b[1] - a[1]];
    let dg = [bb[0] - aa[0], bb[1] - aa[1]];
    let ll = hypot(dl[0], dl[1]);
    let lg = hypot(dg[0], dg[1]);
    if !(ll > 1e-6) || !(lg > 1e-6) {
        return Err(t("crs.pairs.same", &[]));
    }
    let rotation = (dg[1].jatan2(dg[0]) - dl[1].jatan2(dl[0])) * 180.0 / std::f64::consts::PI;
    Ok(Local { scale: lg / ll, rotation: ((rotation + 540.0) % 360.0) - 180.0, from: [a[0], a[1]], to: [aa[0], aa[1]] })
}
/// `groundLocal(combined, base)`.
pub fn ground_local(combined: f64, base: [f64; 2]) -> Local {
    Local { scale: combined, rotation: 0.0, from: base, to: base }
}

/// `crsProblem(crs, projectUnits)` → None = no problem.
pub fn crs_problem(crs: &Value, project_units: &str) -> Option<String> {
    if !cc_civil::js::truthy(Some(crs)) {
        return None;
    }
    let p4 = crs.get("proj4");
    if !cc_civil::js::truthy(p4) {
        return Some(t("crs.p.nodef", &[]));
    }
    let unit = crs.get("unit").map(cc_civil::js::to_js_string).unwrap_or_else(|| "undefined".into());
    if unit == "deg" {
        return Some(t("crs.p.deg", &[]));
    }
    if metres_per(&unit).is_none() {
        return Some(t("crs.p.unit", &[]));
    }
    let fam = if unit == "m" { "m" } else { "ft" };
    if fam != project_units {
        return Some(if project_units == "m" { t("crs.p.units.m", &[]) } else { t("crs.p.units.ft", &[]) });
    }
    if let Some(l) = crs.get("local").filter(|l| cc_civil::js::truthy(Some(l))) {
        let lo = read_local(Some(l));
        let ok = lo.is_some_and(|l| {
            l.scale > 0.0
                && l.scale.is_finite()
                && l.rotation.is_finite()
                && [l.from[0], l.from[1], l.to[0], l.to[1]].iter().all(|v| v.is_finite())
        });
        // `[...l.from, ...l.to]` needs arrays: a local without them throws in the JS (not reached through readCrs)
        if !ok {
            return Some(t("crs.p.local", &[]));
        }
    }
    let def = p4.map(cc_civil::js::to_js_string).unwrap_or_default();
    if parsed(WGS84).is_err() || parsed(&def).is_err() {
        return Some(t("crs.p.use", &[]));
    }
    None
}

/// `prjFor(crs)`.
pub fn prj_for(crs: &Value) -> Option<String> {
    if !cc_civil::js::truthy(Some(crs)) {
        return None;
    }
    let code = crs.get("code");
    if code.is_none_or(Value::is_null) {
        let p = crs.get("proj4").map(cc_civil::js::to_js_string).unwrap_or_else(|| "undefined".into());
        static P: OnceLock<Regex> = OnceLock::new();
        return if P.get_or_init(|| re(r"(?i)^(PROJCS|GEOGCS)\[")).is_match(&p) { Some(p) } else { None };
    }
    static W: OnceLock<Map<String, Value>> = OnceLock::new();
    let w = W.get_or_init(|| match serde_json::from_str::<Value>(WKT_DB) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    });
    let key = code.map(cc_civil::js::to_js_string).unwrap_or_default();
    w.get(&key).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string)
}

// ------------------------------------------------------------------ .prj files

/// `crsFromPrj(wkt)` → { crs, matched, notes }.
pub struct PrjResult {
    pub crs: Value,
    pub matched: Option<&'static Entry>,
    pub notes: Vec<String>,
}

fn unit_of_proj4(u: &str) -> Option<&'static str> {
    match u {
        "m" => Some("m"),
        "ft" => Some("ft"),
        "us-ft" => Some("ftUS"),
        _ => None,
    }
}

fn unit_from_def(def: &Proj, text: &str) -> Option<&'static str> {
    if let Some(u) = def.units.as_str().filter(|s| !s.is_empty()).and_then(unit_of_proj4) {
        return Some(u);
    }
    static P: OnceLock<[Regex; 2]> = OnceLock::new();
    let p = P.get_or_init(|| {
        [re(r#"(?i)UNIT\["([^"]+)",\s*([\d.eE+-]+)\]\s*(,\s*AXIS|,\s*AUTHORITY|\]\s*$)"#), re(r#"(?i)UNIT\["([^"]+)",\s*([\d.eE+-]+)"#)]
    });
    let m: Option<String> = match p[0].captures(text) {
        Some(c) => c.get(2).map(|g| g.as_str().to_string()),
        None => {
            let last = p[1].find_iter(text).last().map(|m| m.as_str().to_string());
            last.and_then(|s| p[1].captures(&s).and_then(|c| c.get(2).map(|g| g.as_str().to_string())))
        }
    };
    let k = match m {
        Some(s) => cc_civil::js::str_to_number(&s),
        None => def.to_meter.filter(|v| *v != 0.0 && !v.is_nan()).unwrap_or(1.0),
    };
    if (k - 1.0).abs() < 1e-12 {
        return Some("m");
    }
    if (k - 0.3048).abs() < 1e-10 {
        return Some("ft");
    }
    if (k - 1200.0 / 3937.0).abs() < 1e-10 {
        return Some("ftUS");
    }
    None
}

fn probe_points(def: &Proj, geographic: bool) -> [[f64; 2]; 3] {
    let deg = 180.0 / std::f64::consts::PI;
    let zone_t = def.zone.filter(|z| *z != 0.0 && !z.is_nan());
    let lon0 = match zone_t {
        Some(z) => z * 6.0 - 183.0,
        None => def.long0.filter(|v| *v != 0.0 && !v.is_nan()).unwrap_or(0.0) * deg,
    };
    let mut lat = match (def.lat1, def.lat2) {
        (Some(a), Some(b)) => (a + b) / 2.0 * deg,
        _ => def.lat0.filter(|v| *v != 0.0 && !v.is_nan()).unwrap_or(0.0) * deg,
    };
    if zone_t.is_some() {
        lat = if def.utm_south { -45.0 } else { 45.0 };
    }
    if geographic {
        lat = 0.0;
    }
    lat = max(-80.0, min(80.0, lat));
    [[lon0, lat], [lon0 + 0.5, lat + 0.5], [lon0 - 0.5, lat + 0.25]]
}

fn norm_name(x: &str) -> String {
    static P: OnceLock<[Regex; 3]> = OnceLock::new();
    let p = P.get_or_init(|| [re(r"NORTH_?AMERICAN_?(DATUM_?)?19?83"), re(r"19(\d\d)"), re(r"[^A-Z0-9]")]);
    let u = x.to_uppercase();
    let a = p[0].replace_all(&u, "NAD83").into_owned();
    let b = p[1].replace_all(&a, "$1").into_owned();
    p[2].replace_all(&b, "").into_owned()
}

fn prj_head_name(text: &str) -> Option<String> {
    static P: OnceLock<Regex> = OnceLock::new();
    P.get_or_init(|| re(r#"^[A-Za-z0-9_]+\["([^"]{1,200})""#)).captures(text).and_then(|c| c.get(1)).map(|m| m.as_str().to_string())
}

fn name_likeness(name: &str, text: &str) -> f64 {
    static P: OnceLock<[Regex; 2]> = OnceLock::new();
    let p = P.get_or_init(|| [re(r"NORTH_?AMERICAN_?(DATUM_?)?19?83"), re(r"[^A-Z0-9]+")]);
    let words = |s: &str| -> Vec<String> {
        let u = s.to_uppercase();
        let a = p[0].replace_all(&u, "NAD83").into_owned();
        let mut out: Vec<String> = Vec::new();
        for w in p[1].split(&a) {
            if !w.is_empty() && !out.iter().any(|x| x == w) {
                out.push(w.to_string());
            }
        }
        out
    };
    let a = words(name);
    let b = words(&prj_head_name(text).unwrap_or_default());
    let both = a.iter().filter(|w| b.contains(w)).count() as f64;
    let den = a.len() as f64 + b.len() as f64 - both;
    both / (if den != 0.0 { den } else { 1.0 })
}

fn prj_rank(e: &Entry, text: &str) -> f64 {
    static P: OnceLock<[Regex; 6]> = OnceLock::new();
    let p = P.get_or_init(|| {
        [
            re(r"\(.*$"),
            re(r"^[^/(]+\(([^)]+)\)"),
            re(r"2011|HARN|NSRS|CSRS|CORS|MA11|PA11"),
            re(r#"(?i)DATUM\["([^"]{0,200})""#),
            re(r"(?i)^D_"),
            re(r"(?i)_?DATUM_?"),
        ]
    });
    let tt = norm_name(text);
    let first = e.name.split(" / ").next().unwrap_or("");
    let datum = norm_name(&p[0].replace(first, ""));
    let real = p[1].captures(&e.name).and_then(|c| c.get(1)).map(|m| m.as_str().to_string());
    let named = match &real {
        Some(r) => tt.contains(&norm_name(r)),
        None => !p[2].is_match(&tt),
    };
    let dn_raw = p[3].captures(text).and_then(|c| c.get(1)).map_or_else(String::new, |m| m.as_str().to_string());
    let dn = norm_name(&p[5].replace_all(&p[4].replace(&dn_raw, ""), ""));
    let datum_hit = if !dn.is_empty() { dn.contains(&datum) || (dn.len() >= 3 && datum.contains(&dn)) } else { tt.contains(&datum) };
    (if datum_hit { 0.0 } else { 20.0 })
        + (if named { 0.0 } else { 10.0 })
        + (1.0 - name_likeness(&e.name, text)) * 5.0
        + rank(&e.name) / 10.0
}

/// `crsFromPrj(wkt)`.
pub fn crs_from_prj(wkt: &str) -> R<PrjResult> {
    let mut notes = Vec::new();
    let text = cc_civil::js::trim(wkt).to_string();
    static P: OnceLock<[Regex; 5]> = OnceLock::new();
    let p = P.get_or_init(|| {
        [
            re(r"(?i)^(PROJCS|GEOGCS|PROJCRS|GEOGCRS|GEODCRS)\["),
            re(r#"(?i)AUTHORITY\["EPSG",\s*"?(\d+)"?\]\s*\]\s*$"#),
            re(r#"(?i)ID\["EPSG",\s*(\d+)\]\s*\]\s*$"#),
            re(r"(?i)^PROJCS\["),
            re(r"(?i)Mercator_Auxiliary_Sphere|Pseudo[_ -]?Mercator|Popular[_ ]Visuali[sz]ation"),
        ]
    });
    if text.is_empty() || text.encode_utf16().count() > 20000 || !p[0].is_match(&text) {
        return Err(t("crs.prj.not", &[]));
    }
    let auth = p[1].captures(&text).or_else(|| p[2].captures(&text)).and_then(|c| c.get(1)).map(|m| m.as_str().to_string());
    if let Some(a) = auth
        && catalog_loaded()
        && let Some(e) = by_code(cc_civil::js::str_to_number(&a))?
    {
        return Ok(PrjResult { crs: crs_from_entry(e), matched: Some(e), notes });
    }
    if catalog_loaded()
        && let Some(e) = by_code(3857.0)?
        && p[3].is_match(&text)
        && p[4].is_match(&text)
    {
        return Ok(PrjResult { crs: crs_from_entry(e), matched: Some(e), notes });
    }
    let def = parsed(&text).map_err(|_| t("crs.prj.bad", &[]))?;
    let name = prj_head_name(&text).unwrap_or_else(|| t("crs.prj.name", &[]));
    static G: OnceLock<Regex> = OnceLock::new();
    let geographic = G.get_or_init(|| re(r"(?i)^GEOG")).is_match(&text);
    let unit = if geographic { Some("deg") } else { unit_from_def(&def, &text) };
    let Some(unit) = unit else { return Err(t("crs.prj.unit", &[])) };
    let crs = json!({ "code": null, "name": name, "proj4": text, "unit": unit, "local": null });
    let mut matched: Option<&'static Entry> = None;
    if catalog_loaded() {
        let pts = probe_points(&def, geographic);
        let same_ell = |pp: &Proj| (pp.a - def.a).abs() < 1e-3 && (or0(pp.es) - or0(def.es)).abs() < 1e-9;
        let fwd = |pp: &Proj, q: [f64; 2]| -> R<[f64; 2]> {
            let r = match pp.forward(Pt::xy(q[0] * std::f64::consts::PI / 180.0, q[1] * std::f64::consts::PI / 180.0))? {
                cc_proj::Out::P(r) => r,
                cc_proj::Out::Num(_) => Pt::xy(f64::NAN, f64::NAN),
                _ => return Err("null".into()),
            };
            let k = if geographic { 1.0 } else { pp.to_meter.filter(|v| *v != 0.0 && !v.is_nan()).unwrap_or(1.0) };
            Ok([r.x / k, r.y / k])
        };
        let want: Option<Vec<[f64; 2]>> = pts.iter().map(|q| fwd(&def, *q).ok()).collect();
        if let Some(want) = want.filter(|w| w.iter().all(|x| x[0].is_finite() && x[1].is_finite())) {
            let tol = if geographic { 1e-9 } else { 1e-3 };
            for e in catalog()? {
                if e.unit != unit || e.geographic != geographic {
                    continue;
                }
                let Ok(pp) = parsed(&e.proj4) else { continue };
                if !same_ell(&pp) {
                    continue;
                }
                let all = pts.iter().enumerate().all(|(i, q)| match fwd(&pp, *q) {
                    Ok(g) => hypot(g[0] - want.g(i)[0], g[1] - want.g(i)[1]) < tol,
                    Err(_) => false,
                });
                if all && matched.is_none_or(|m| prj_rank(e, &text) < prj_rank(m, &text)) {
                    matched = Some(e);
                }
            }
        }
    }
    if let Some(m) = matched {
        return Ok(PrjResult { crs: crs_from_entry(m), matched: Some(m), notes });
    }
    notes.push(t("crs.prj.nomatch", &[]));
    Ok(PrjResult { crs, matched: None, notes })
}

fn or0(v: f64) -> f64 {
    if v != 0.0 && !v.is_nan() { v } else { 0.0 }
}

/// The ESRI/OGC name of a datum in words, for messages built elsewhere.
pub fn fmt_m(m: f64) -> String {
    if m < 1.0 {
        format!("{} cm", num_to_string(cc_civil::js::round(m * 100.0)))
    } else if m < 10.0 {
        format!("{} m", num_to_string(cc_civil::js::round_fixed(m, 1)))
    } else {
        format!("{} m", num_to_string(cc_civil::js::round(m)))
    }
}

/// `t(key, {n})` helper for messages with a code.
pub fn unknown_code_note(code: f64) -> String {
    tr!("terrain.crs.unknown", code = code)
}
