//! raster.js — elevation grids: GeoTIFF (through cc-proj's port of geotiff.js 3.0.5) and ESRI ASCII grids (.asc +
//! .prj). Files are hostile: sizes, counts and numbers are checked before use, and every read is bounded by the
//! window it was asked for.

use crate::crs;
use crate::terrain::{self, Grid, limits};
use cc_civil::i18n::{F, t};
use cc_civil::js::{J, max, min, num_to_string, str_to_number, to_fixed};
use cc_civil::tr;
use cc_proj::tiff::{Image, KeyVal, ReadOptions, TagVal, Tiff};
use regex::Regex;
use serde_json::{Map, Value, json};
use std::sync::OnceLock;

type R<T> = Result<T, String>;

/// One input file: its name and bytes (a browser `File`).
#[derive(Clone, Debug)]
pub struct InFile {
    pub name: String,
    pub bytes: Vec<u8>,
}
impl InFile {
    fn size(&self) -> f64 {
        self.bytes.len() as f64
    }
}

/// `String(e.message).slice(0, n)`.
fn cut(s: &str, n: usize) -> String {
    cc_civil::js::slice16(s, n)
}

fn open_tiff(bytes: &[u8]) -> R<Tiff> {
    let tiff = Tiff::from_bytes(bytes).map_err(|e| tr!("raster.not_tiff", reason = cut(&e, 120)))?;
    tiff.image(0).map_err(|e| tr!("raster.not_tiff", reason = cut(&e, 120)))?;
    Ok(tiff)
}

/// `tagNum(fd, name, def)`.
fn tag_num(img: &Image, name: &str, def: Option<f64>) -> Option<f64> {
    if !img.has(name) {
        return def;
    }
    match img.value(name) {
        Err(_) | Ok(None) => def,
        Ok(Some(TagVal::Num(n))) => Some(n),
        Ok(Some(TagVal::Arr(a))) => {
            if a.is_empty() {
                def
            } else {
                a.get(0)
            }
        }
        // `v.length ? v[0]`: the first character (never a number here)
        Ok(Some(TagVal::Str(s))) => {
            if s.is_empty() {
                def
            } else {
                Some(f64::NAN)
            }
        }
    }
}
/// `tagArr(fd, name)`: `Array.from(value || [])`.
fn tag_arr(img: &Image, name: &str) -> Option<Vec<f64>> {
    if !img.has(name) {
        return None;
    }
    match img.value(name) {
        Err(_) => None,
        Ok(None) => Some(Vec::new()),
        Ok(Some(TagVal::Num(_))) => Some(Vec::new()),
        Ok(Some(TagVal::Arr(a))) => Some(a.v.clone()),
        Ok(Some(TagVal::Str(s))) => Some(vec![f64::NAN; s.encode_utf16().count()]),
    }
}
/// `tagStr(fd, name)`.
fn tag_str(img: &Image, name: &str) -> Option<String> {
    if !img.has(name) {
        return None;
    }
    match img.value(name) {
        Ok(Some(TagVal::Str(s))) => Some(s.trim_end_matches('\u{0}').to_string()),
        _ => None,
    }
}
fn max_of(v: &[f64]) -> f64 {
    cc_civil::js::max_of(v.iter().copied())
}

/// `probeTiff(file)`.
pub fn probe_tiff(bytes: &[u8]) -> R<Value> {
    let tiff = open_tiff(bytes)?;
    let img = tiff.image(0)?;
    let bands = img.samples_per_pixel();
    let bits = max_of(&tag_arr(&img, "BitsPerSample").unwrap_or_else(|| vec![8.0]));
    let fmt = max_of(&tag_arr(&img, "SampleFormat").unwrap_or_else(|| vec![1.0]));
    let photometric = tag_num(&img, "PhotometricInterpretation", Some(1.0));
    let float = fmt == 3.0;
    let ph = photometric.unwrap_or(f64::NAN);
    let picture = bands >= 2.0 || ph == 2.0 || ph == 3.0 || ph == 6.0 || (bands == 1.0 && bits <= 8.0 && !float);
    Ok(json!({ "bands": bands.j(), "bits": bits.j(), "float": float, "photometric": photometric.j(), "image": picture }))
}

/// GDAL_METADATA items for band 0 and the dataset.
#[derive(Clone, Debug, Default)]
struct Meta {
    scale: Option<f64>,
    offset: Option<f64>,
    unit: Option<String>,
    items: Vec<(String, String)>,
}
fn gdal_meta(xml: Option<&str>) -> Meta {
    let mut out = Meta::default();
    let Some(xml) = xml.filter(|x| !x.is_empty() && x.encode_utf16().count() <= 1_000_000) else { return out };
    static P: OnceLock<Regex> = OnceLock::new();
    let re = P.get_or_init(|| rx(r"<Item\b([^>]*)>([^<]*)</Item>"));
    for m in re.captures_iter(xml) {
        let attrs = m.get(1).map_or("", |g| g.as_str());
        let val = cc_civil::js::trim(m.get(2).map_or("", |g| g.as_str())).to_string();
        let at = |k: &str| -> Option<String> {
            // `new RegExp(k + '="([^"]*)"')`: the first match anywhere in the attributes
            let pat = format!("{k}=\"");
            let i = attrs.find(&pat)?;
            let rest = attrs.get(i.saturating_add(pat.len())..)?;
            let e = rest.find('"')?;
            rest.get(..e).map(str::to_string)
        };
        let sample = at("sample");
        let role = at("role").unwrap_or_default().to_lowercase();
        let name = at("name").unwrap_or_default();
        if sample.as_deref().is_some_and(|s| s != "0") {
            continue;
        }
        if role == "scale" {
            out.scale = Some(str_to_number(&val));
        } else if role == "offset" {
            out.offset = Some(str_to_number(&val));
        } else if role == "unittype" {
            out.unit = Some(val);
        } else if !name.is_empty() && sample.is_none() {
            match out.items.iter_mut().find(|(k, _)| *k == name) {
                Some(slot) => slot.1 = val,
                None => out.items.push((name, val)),
            }
        }
    }
    out
}

fn rx(p: &str) -> Regex {
    Regex::new(p).unwrap_or_else(|_| never())
}
#[allow(clippy::expect_used)]
fn never() -> Regex {
    Regex::new("a^").expect("a constant pattern")
}

/// An open raster (the JS `src` without its File / image objects).
#[derive(Clone, Debug)]
pub struct Src {
    pub kind: &'static str,
    pub name: String,
    pub size: f64,
    pub width: f64,
    pub height: f64,
    pub bands: f64,
    pub dtype: String,
    pub gt: [f64; 6],
    pub pixel_is_point: bool,
    pub cell: Option<f64>,
    pub nodata: Option<f64>,
    pub scale: f64,
    pub offset: f64,
    pub crs: Value,
    pub code: Option<f64>,
    pub crs_note: Option<String>,
    pub z_unit: Option<&'static str>,
    pub z_unit_from: Option<String>,
    pub notes: Vec<String>,
    pub stats: Option<Stats>,
    pub tile_height: Option<f64>,
    pub data_start: Option<f64>,
    tiff: Option<Tiff>,
    text: Option<Vec<u8>>,
}

/// `{ min, max, sampled }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub sampled: bool,
}
impl J for Stats {
    fn j(&self) -> Value {
        json!({ "min": self.min.j(), "max": self.max.j(), "sampled": self.sampled })
    }
}

impl J for Src {
    fn j(&self) -> Value {
        let mut o = Map::new();
        o.insert("kind".into(), self.kind.into());
        o.insert("name".into(), self.name.clone().into());
        o.insert("size".into(), self.size.j());
        o.insert("width".into(), self.width.j());
        o.insert("height".into(), self.height.j());
        o.insert("bands".into(), self.bands.j());
        o.insert("dtype".into(), self.dtype.clone().into());
        o.insert("gt".into(), self.gt.to_vec().j());
        o.insert("pixelIsPoint".into(), self.pixel_is_point.into());
        o.insert("cell".into(), self.cell.j());
        o.insert("nodata".into(), self.nodata.j());
        o.insert("scale".into(), self.scale.j());
        o.insert("offset".into(), self.offset.j());
        o.insert("crs".into(), self.crs.clone());
        o.insert("code".into(), self.code.j());
        o.insert("crsNote".into(), self.crs_note.j());
        o.insert("zUnit".into(), self.z_unit.map(str::to_string).j());
        o.insert("zUnitFrom".into(), self.z_unit_from.j());
        o.insert("notes".into(), self.notes.j());
        o.insert("stats".into(), self.stats.j());
        if let Some(th) = self.tile_height {
            o.insert("tileHeight".into(), th.j());
        }
        if let Some(d) = self.data_start {
            o.insert("dataStart".into(), d.j());
        }
        Value::Object(o)
    }
}

fn sample_type(fmt: Option<f64>, bits: Option<f64>) -> Option<&'static str> {
    match (fmt, bits) {
        (Some(f), Some(b)) => match (f, b) {
            (1.0, 8.0) => Some("uint8"),
            (1.0, 16.0) => Some("uint16"),
            (1.0, 32.0) => Some("uint32"),
            (2.0, 8.0) => Some("int8"),
            (2.0, 16.0) => Some("int16"),
            (2.0, 32.0) => Some("int32"),
            (3.0, 32.0) => Some("float32"),
            (3.0, 64.0) => Some("float64"),
            _ => None,
        },
        _ => None,
    }
}
fn opt_text(v: Option<f64>) -> String {
    v.map_or_else(|| "undefined".to_string(), num_to_string)
}

/// The GeoKeys as an object (names → values; undefined values kept as null).
fn keys_obj(keys: &[(String, KeyVal)]) -> Map<String, Value> {
    keys.iter()
        .map(|(k, v)| {
            let j = match v {
                KeyVal::Undef => Value::Null,
                KeyVal::Num(n) => n.j(),
                KeyVal::Str(s) => Value::String(s.clone()),
                KeyVal::Arr(a) => a.j(),
            };
            (k.clone(), j)
        })
        .collect()
}

fn open_geotiff(name: &str, bytes: &[u8]) -> R<Src> {
    let tiff = open_tiff(bytes)?;
    let img = tiff.image(0)?;
    let width = img.width();
    let height = img.height();
    if !(width > 0.0 && height > 0.0) || width > limits::DIM || height > limits::DIM {
        return Err(tr!("raster.bad_size", w = width, h = height));
    }
    let bands = img.samples_per_pixel();
    let bits = tag_arr(&img, "BitsPerSample").unwrap_or_else(|| vec![8.0]).first().copied();
    let fmt = tag_arr(&img, "SampleFormat").unwrap_or_else(|| vec![1.0]).first().copied();
    let Some(dtype) = sample_type(fmt, bits) else { return Err(tr!("raster.bad_cells", bits = opt_text(bits), fmt = opt_text(fmt))) };
    let mut notes = Vec::new();
    if bands > 1.0 {
        notes.push(tr!("raster.bands_n", n = bands));
    }
    let keys = img.geo_keys().ok().flatten().map(|k| keys_obj(&k));
    let raster_type = keys.as_ref().and_then(|k| k.get("GTRasterTypeGeoKey")).filter(|v| !v.is_null()).cloned().unwrap_or_else(|| json!(1));
    let pixel_is_point = raster_type.as_f64() == Some(2.0);
    let tie = tag_arr(&img, "ModelTiepoint");
    let scl = tag_arr(&img, "ModelPixelScale");
    let mtx = tag_arr(&img, "ModelTransformation");
    let g = |a: &[f64], i: usize| a.get(i).copied().unwrap_or(f64::NAN);
    let mut gt: [f64; 6] = match (&mtx, &tie, &scl) {
        (Some(m), _, _) if m.len() >= 16 && m.iter().all(|v| v.is_finite()) => [g(m, 3), g(m, 0), g(m, 1), g(m, 7), g(m, 4), g(m, 5)],
        (_, Some(tp), Some(sc))
            if tp.len() >= 6
                && sc.len() >= 2
                && tp.iter().take(6).chain(sc.iter().take(2)).all(|v| v.is_finite())
                && g(sc, 0) > 0.0
                && g(sc, 1) > 0.0 =>
        {
            if tp.len() > 6 {
                notes.push(t("raster.tie_points", &[]));
            }
            [g(tp, 3) - g(tp, 0) * g(sc, 0), g(sc, 0), 0.0, g(tp, 4) + g(tp, 1) * g(sc, 1), 0.0, -g(sc, 1)]
        }
        _ => return Err(t("raster.no_georef", &[])),
    };
    if !((gt[1] * gt[5] - gt[2] * gt[4]).abs() > 0.0) {
        return Err(t("raster.zero_cell", &[]));
    }
    if pixel_is_point {
        gt = [gt[0] - 0.5 * (gt[1] + gt[2]), gt[1], gt[2], gt[3] - 0.5 * (gt[4] + gt[5]), gt[4], gt[5]];
    }
    let gk = terrain::crs_from_geo_keys(keys.as_ref());
    let meta = gdal_meta(tag_str(&img, "GDAL_METADATA").as_deref());
    let nd_str = tag_str(&img, "GDAL_NODATA");
    let mut nodata = nd_str.as_ref().filter(|s| !cc_civil::js::trim(s).is_empty()).map(|s| str_to_number(cc_civil::js::trim(s)));
    if let (Some(n), Some(s)) = (nodata, nd_str.as_ref())
        && n.is_nan()
        && !s.to_lowercase().contains("nan")
    {
        nodata = None;
    }
    let scale = meta.scale.filter(|s| s.is_finite() && *s != 0.0).unwrap_or(1.0);
    let offset = meta.offset.filter(|s| s.is_finite()).unwrap_or(0.0);
    let mut z_unit = meta.unit.as_deref().and_then(terrain::unit_from_name);
    let mut z_unit_from =
        if z_unit.is_some() { Some(tr!("raster.zfrom.band", unit = meta.unit.clone().unwrap_or_default())) } else { None };
    if z_unit.is_none() && gk.z_unit.is_some() {
        z_unit = gk.z_unit;
        z_unit_from = Some(t("raster.zfrom.key", &[]));
    }
    if let Some(u) = meta.unit.as_ref().filter(|u| !u.is_empty())
        && z_unit.is_none()
    {
        notes.push(tr!("raster.unknown_unit", unit = cut(u, 40)));
    }
    let item = |k: &str| meta.items.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    let num_item = |k: &str| item(k).map_or(f64::NAN, |v| str_to_number(&v));
    let (smin, smax) = (num_item("STATISTICS_MINIMUM"), num_item("STATISTICS_MAXIMUM"));
    let stats = if smin.is_finite() && smax.is_finite() {
        Some(Stats { min: Some(smin * scale + offset), max: Some(smax * scale + offset), sampled: false })
    } else {
        None
    };
    let cell = if gt[2] == 0.0 && gt[4] == 0.0 && (gt[1] + gt[5]).abs() < 1e-9 * gt[1].abs() { Some(gt[1]) } else { None };
    let tile_height = img.tile_height();
    Ok(Src {
        kind: "geotiff",
        name: name.to_string(),
        size: bytes.len() as f64,
        width,
        height,
        bands,
        dtype: dtype.into(),
        gt,
        pixel_is_point,
        cell,
        nodata,
        scale,
        offset,
        crs: gk.crs,
        code: gk.code,
        crs_note: gk.note,
        z_unit,
        z_unit_from,
        notes,
        stats,
        tile_height: Some(tile_height),
        data_start: None,
        tiff: Some(tiff),
        text: None,
    })
}

// ---------------------------------------------------------------- ESRI ASCII grid

struct AscHeader {
    ncols: f64,
    nrows: f64,
    dx: f64,
    dy: f64,
    xll: f64,
    yll: f64,
    nodata: Option<f64>,
    data_start: f64,
}

/// `^\s*([A-Za-z_]+)\s+(\S+)\s*$` on one line.
fn header_line(line: &str) -> Option<(String, String)> {
    let s = line.trim_start_matches(cc_civil::js::is_ws);
    let key_len = s.find(|c: char| !(c.is_ascii_alphabetic() || c == '_')).unwrap_or(s.len());
    if key_len == 0 {
        return None;
    }
    let (key, rest) = s.split_at(key_len);
    let after = rest.trim_start_matches(cc_civil::js::is_ws);
    if after.len() == rest.len() {
        return None;
    }
    let val_len = after.find(cc_civil::js::is_ws).unwrap_or(after.len());
    if val_len == 0 {
        return None;
    }
    let (val, tail) = after.split_at(val_len);
    if !tail.chars().all(cc_civil::js::is_ws) {
        return None;
    }
    Some((key.to_string(), val.to_string()))
}

const ASC_KEYS: [&str; 10] = ["ncols", "nrows", "xllcorner", "yllcorner", "xllcenter", "yllcenter", "cellsize", "dx", "dy", "nodata_value"];

fn asc_header(bytes: &[u8]) -> R<AscHeader> {
    let head = String::from_utf8_lossy(bytes.get(..bytes.len().min(4096)).unwrap_or(&[])).into_owned();
    let crlf = head.contains("\r\n");
    let mut h: Vec<(String, f64)> = Vec::new();
    let mut nbytes = 0.0;
    let mut n = 0u32;
    for raw in head.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let Some((k, v)) = header_line(line) else { break };
        let kl = k.to_lowercase();
        if !ASC_KEYS.contains(&kl.as_str()) {
            break;
        }
        let val = str_to_number(&v);
        match h.iter_mut().find(|(a, _)| *a == kl) {
            Some(slot) => slot.1 = val,
            None => h.push((kl, val)),
        }
        nbytes += line.len() as f64 + if crlf { 2.0 } else { 1.0 };
        n = n.saturating_add(1);
        if n > 10 {
            break;
        }
    }
    let get = |k: &str| h.iter().find(|(a, _)| a == k).map(|(_, v)| *v);
    let (ncols, nrows) = (get("ncols"), get("nrows"));
    let ok = |v: Option<f64>| v.filter(|x| cc_civil::js::is_integer(*x) && *x > 0.0 && *x <= limits::DIM);
    let (Some(ncols), Some(nrows)) = (ok(ncols), ok(nrows)) else { return Err(t("raster.asc.not_asc", &[])) };
    let dx = get("cellsize").or_else(|| get("dx")).unwrap_or(f64::NAN);
    let dy = get("cellsize").or_else(|| get("dy")).or_else(|| get("dx")).unwrap_or(f64::NAN);
    if !(dx > 0.0) || !(dy > 0.0) || !dx.is_finite() || !dy.is_finite() {
        return Err(t("raster.asc.no_cell", &[]));
    }
    let center = get("xllcenter").is_some();
    let xll = if center { get("xllcenter").unwrap_or(f64::NAN) - dx / 2.0 } else { get("xllcorner").unwrap_or(f64::NAN) };
    let yll = if center { get("yllcenter").unwrap_or(0.0) - dy / 2.0 } else { get("yllcorner").unwrap_or(f64::NAN) };
    if !xll.is_finite() || !yll.is_finite() {
        return Err(t("raster.asc.no_corner", &[]));
    }
    Ok(AscHeader { ncols, nrows, dx, dy, xll, yll, nodata: get("nodata_value").filter(|v| v.is_finite()), data_start: nbytes })
}

fn ends_with_ci(name: &str, ext: &str) -> bool {
    name.len() >= ext.len() && name.get(name.len().saturating_sub(ext.len())..).is_some_and(|e| e.eq_ignore_ascii_case(ext))
}
fn strip_ext_ci(name: &str, ext: &str) -> String {
    if ends_with_ci(name, ext) { name.get(..name.len().saturating_sub(ext.len())).unwrap_or("").to_string() } else { name.to_string() }
}

fn open_asc(files: &[InFile]) -> R<Src> {
    let file = files.iter().find(|f| ends_with_ci(&f.name, ".asc")).ok_or("Cannot read properties of undefined (reading 'slice')")?;
    let h = asc_header(&file.bytes)?;
    let mut crs = Value::Null;
    let mut code = None;
    let mut crs_note = Some(t("raster.asc.no_prj", &[]));
    let base = strip_ext_ci(&file.name, ".asc").to_lowercase();
    let prj = files
        .iter()
        .find(|f| ends_with_ci(&f.name, ".prj") && strip_ext_ci(&f.name, ".prj").to_lowercase() == base)
        .or_else(|| files.iter().find(|f| ends_with_ci(&f.name, ".prj")));
    if let Some(p) = prj {
        if p.size() > 20000.0 {
            crs_note = Some(t("raster.asc.prj_big", &[]));
        } else {
            match crs::crs_from_prj(&String::from_utf8_lossy(&p.bytes)) {
                Ok(r) => {
                    crs = r.crs;
                    code = r.matched.map(|e| e.code);
                    let joined = r.notes.join(" ");
                    crs_note = if joined.is_empty() { None } else { Some(joined) };
                }
                Err(e) => crs_note = Some(e),
            }
        }
    }
    Ok(Src {
        kind: "asc",
        name: file.name.clone(),
        size: file.size(),
        width: h.ncols,
        height: h.nrows,
        bands: 1.0,
        dtype: "text".into(),
        gt: [h.xll, h.dx, 0.0, h.yll + h.nrows * h.dy, 0.0, -h.dy],
        pixel_is_point: false,
        cell: if h.dx == h.dy { Some(h.dx) } else { None },
        nodata: h.nodata,
        scale: 1.0,
        offset: 0.0,
        crs,
        code,
        crs_note,
        z_unit: None,
        z_unit_from: None,
        notes: Vec::new(),
        stats: None,
        tile_height: None,
        data_start: Some(h.data_start),
        tiff: None,
        text: Some(file.bytes.clone()),
    })
}

/// `openRaster(files)`.
pub fn open_raster(files: &[InFile]) -> R<Src> {
    let is_grid = |n: &str| ends_with_ci(n, ".tif") || ends_with_ci(n, ".tiff") || ends_with_ci(n, ".asc");
    let main = files.iter().find(|f| is_grid(&f.name)).ok_or_else(|| t("raster.no_grid", &[]))?;
    if main.size() > limits::FILE_BYTES {
        return Err(too_big(&main.name, main.size()));
    }
    if ends_with_ci(&main.name, ".asc") {
        return open_asc(files);
    }
    open_geotiff(&main.name, &main.bytes)
}

pub fn too_big(name: &str, size: f64) -> String {
    tr!("raster.too_big", name = name, mb = to_fixed(size / 1048576.0, 0), max = num_to_string(limits::FILE_BYTES / 1048576.0))
}

// ---------------------------------------------------------------- reading

/// `fmtBig(n)`.
pub fn fmt_big(n: f64) -> String {
    if n >= 1e9 {
        tr!("raster.big.billion", n = to_fixed(n / 1e9, 1))
    } else if n >= 1e6 {
        tr!("raster.big.million", n = to_fixed(n / 1e6, 1))
    } else {
        locale_int(cc_civil::js::round(n))
    }
}
/// `Math.round(n).toLocaleString('en-US')` for a whole number.
pub fn locale_int(n: f64) -> String {
    if !n.is_finite() {
        return if n.is_nan() {
            "NaN".into()
        } else if n > 0.0 {
            "∞".into()
        } else {
            "-∞".into()
        };
    }
    let s = num_to_string(n.abs());
    if s.contains('e') {
        return s;
    }
    let mut out = String::new();
    let digits: Vec<char> = s.chars().collect();
    for (i, c) in digits.iter().enumerate() {
        if i > 0 && (digits.len().saturating_sub(i)) % 3 == 0 {
            out.push(',');
        }
        out.push(*c);
    }
    if n < 0.0 { format!("-{out}") } else { out }
}

fn clean_value(src: &Src, v: f64) -> f64 {
    if let Some(nd) = src.nodata
        && (v == nd || (nd.is_nan() && v.is_nan()))
    {
        return f64::NAN;
    }
    if !v.is_finite() {
        return f64::NAN;
    }
    // `Math.fround(src.nodata)`
    #[allow(clippy::cast_possible_truncation)]
    let fround = |x: f64| f64::from(x as f32);
    if let Some(nd) = src.nodata
        && src.dtype == "float32"
        && fround(nd) == v
    {
        return f64::NAN;
    }
    v * src.scale + src.offset
}

/// `readWindow(src, { c0, r0, c1, r1, step })` (None = the default).
pub fn read_window(src: &Src, c0: Option<f64>, r0: Option<f64>, c1: Option<f64>, r1: Option<f64>, step: Option<f64>) -> R<Grid> {
    let c0 = max(0.0, c0.unwrap_or(0.0).floor());
    let r0 = max(0.0, r0.unwrap_or(0.0).floor());
    let c1 = min(src.width, c1.unwrap_or(src.width).ceil());
    let r1 = min(src.height, r1.unwrap_or(src.height).ceil());
    let step = max(1.0, step.unwrap_or(1.0).floor());
    if !(c1 > c0 && r1 > r0) {
        return Err(t("raster.no_overlap", &[]));
    }
    let cells = (c1 - c0) * (r1 - r0);
    if cells > limits::READ_CELLS {
        return Err(tr!("raster.area_big", cells = fmt_big(cells), max = fmt_big(limits::READ_CELLS)));
    }
    let nx = ((c1 - c0) / step).ceil();
    let ny = ((r1 - r0) / step).ceil();
    let (nxu, nyu) = (terrain::count_of(nx), terrain::count_of(ny));
    let mut grid = Grid { c0, r0, step, nx: nxu, ny: nyu, z: vec![0.0; nxu.saturating_mul(nyu)] };
    if src.kind == "asc" {
        read_asc(src, &mut grid, c1, r1)?;
    } else {
        read_tiff(src, &mut grid, c1, r1).map_err(|e| tr!("raster.read_failed", reason = cut(&e, 80)))?;
    }
    Ok(grid)
}

fn put(src: &Src, grid: &mut Grid, row: f64, data: &[f64], row_in_data: f64, width: f64) {
    let j = (row - grid.r0) / grid.step;
    let base = row_in_data * width;
    let nx = grid.nx;
    for i in 0..nx {
        let di = base + (i as f64) * grid.step;
        let v = cc_civil::js::usize_of(di).and_then(|k| data.get(k)).copied().unwrap_or(f64::NAN);
        let zi = j * (nx as f64) + (i as f64);
        if let Some(slot) = cc_civil::js::usize_of(zi).and_then(|k| grid.z.get_mut(k)) {
            *slot = clean_value(src, v);
        }
    }
}

fn read_tiff(src: &Src, grid: &mut Grid, c1: f64, r1: f64) -> R<()> {
    let tiff = src.tiff.as_ref().ok_or("no image")?;
    let img = tiff.image(0)?;
    let (c0, r0, step, ny) = (grid.c0, grid.r0, grid.step, grid.ny);
    let width = c1 - c0;
    let band_rows = max(1.0, min(r1 - r0, (4e6 / width).floor()));
    let th = src.tile_height.filter(|v| *v != 0.0 && !v.is_nan()).unwrap_or(1.0);
    let read = |w: [f64; 4]| -> R<Vec<f64>> {
        let o = ReadOptions { window: Some(w), samples: vec![0], ..ReadOptions::default() };
        let mut r = img.read_rasters(&o)?;
        Ok(if r.bands.is_empty() { Vec::new() } else { r.bands.swap_remove(0) })
    };
    if step >= max(8.0, th) {
        for j in 0..ny {
            let row = r0 + (j as f64) * step;
            let data = read([c0, row, c1, row + 1.0])?;
            put(src, grid, row, &data, 0.0, width);
        }
    } else {
        let mut rs = r0;
        while rs < r1 {
            let re = min(r1, rs + band_rows);
            let data = read([c0, rs, c1, re])?;
            let mut row = rs;
            while row < re {
                if (row - r0) % step == 0.0 {
                    put(src, grid, row, &data, row - rs, width);
                }
                row += 1.0;
            }
            rs += band_rows;
        }
    }
    Ok(())
}

fn read_asc(src: &Src, grid: &mut Grid, c1: f64, r1: f64) -> R<()> {
    let bytes = src.text.as_deref().unwrap_or(&[]);
    let start = src.data_start.and_then(cc_civil::js::usize_of).unwrap_or(0).min(bytes.len());
    let text = String::from_utf8_lossy(bytes.get(start..).unwrap_or(&[]));
    let w = src.width;
    let need = r1 * w;
    let (c0, r0, step, nx) = (grid.c0, grid.r0, grid.step, grid.nx as f64);
    let mut k = 0.0f64;
    let mut take = |tok: &str, k: &mut f64| {
        let row = (*k / w).floor();
        let col = *k - row * w;
        if row >= r0 && row < r1 && (row - r0) % step == 0.0 && col >= c0 && col < c1 && (col - c0) % step == 0.0 {
            let v = str_to_number(tok);
            let zi = ((row - r0) / step) * nx + (col - c0) / step;
            if let Some(slot) = cc_civil::js::usize_of(zi).and_then(|i| grid.z.get_mut(i)) {
                *slot = if v.is_nan() { f64::NAN } else { clean_value(src, v) };
            }
        }
        *k += 1.0;
    };
    let mut s = 0usize;
    let tb = text.as_bytes();
    let mut i = 0usize;
    while i < tb.len() && k < need {
        if tb.get(i).is_some_and(|&c| c <= 32) {
            if i > s {
                take(text.get(s..i).unwrap_or(""), &mut k);
            }
            s = i.saturating_add(1);
        }
        i = i.saturating_add(1);
    }
    if k < need {
        let carry = cc_civil::js::trim(text.get(s..).unwrap_or("")).to_string();
        if !carry.is_empty() {
            take(&carry, &mut k);
        }
    }
    if k < need {
        return Err(tr!("raster.asc.short", read = fmt_big(k), total = fmt_big(w * src.height)));
    }
    Ok(())
}

/// `sampleStats(src)` (and keeps it on the source, as the JS does).
pub fn sample_stats(src: &mut Src) -> R<Option<Stats>> {
    if let Some(s) = src.stats {
        return Ok(Some(s));
    }
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    let sampled = if src.kind == "geotiff" {
        let w = min(256.0, src.width);
        let h = min(256.0, src.height);
        let tiff = src.tiff.as_ref().ok_or("no image")?;
        let img = tiff.image(0)?;
        let o =
            ReadOptions { samples: vec![0], width: Some(w), height: Some(h), resample: Some("nearest".into()), ..ReadOptions::default() };
        let r = img.read_rasters(&o)?;
        for &v in r.bands.first().map_or(&[][..], |b| &b[..]) {
            let c = clean_value(src, v);
            if c < lo {
                lo = c;
            }
            if c > hi {
                hi = c;
            }
        }
        w < src.width || h < src.height
    } else {
        if src.size > 20e6 {
            return Ok(None);
        }
        let g = read_window(src, None, None, None, None, None)?;
        for &v in &g.z {
            if v < lo {
                lo = v;
            }
            if v > hi {
                hi = v;
            }
        }
        false
    };
    let st = if lo.is_finite() { Stats { min: Some(lo), max: Some(hi), sampled } } else { Stats { min: None, max: None, sampled } };
    src.stats = Some(st);
    Ok(Some(st))
}

/// `Uint8ClampedArray` store: round half to even, clamp to 0..255, NaN → 0.
fn clamp_u8(v: f64) -> u8 {
    if !(v > 0.0) {
        return 0;
    }
    if v >= 255.0 {
        return 255;
    }
    let f = v.floor();
    let d = v - f;
    let r = if d > 0.5 || (d == 0.5 && f % 2.0 != 0.0) { f + 1.0 } else { f };
    // r is in 0..=255 here
    u8::try_from(cc_civil::js::usize_of(r).unwrap_or(0)).unwrap_or(255)
}

/// `readGeoImage(file)`.
pub struct GeoImage {
    pub width: f64,
    pub height: f64,
    pub rgba: Vec<u8>,
    pub affine: [f64; 6],
    pub bbox: [f64; 4],
    pub crs: Value,
    pub code: Option<f64>,
    pub crs_note: Option<String>,
    pub notes: Vec<String>,
}

const MAX_IMAGE_PX: f64 = 16.0 * 1024.0 * 1024.0;

pub fn read_geo_image(name: &str, bytes: &[u8]) -> R<GeoImage> {
    if bytes.len() as f64 > limits::FILE_BYTES {
        return Err(too_big(name, bytes.len() as f64));
    }
    let src = open_geotiff(name, bytes)?;
    let bands_note = t("raster.bands_n", &[("n", F::N(src.bands))]);
    let mut notes: Vec<String> = src.notes.iter().filter(|n| **n != bands_note).cloned().collect();
    let (mut w, mut h) = (src.width, src.height);
    if w * h > MAX_IMAGE_PX {
        let k = (MAX_IMAGE_PX / (w * h)).sqrt();
        w = max(1.0, (w * k).floor());
        h = max(1.0, (h * k).floor());
        notes.push(tr!("raster.image_reduced", w = src.width, h = src.height, rw = w, rh = h));
    }
    let tiff = src.tiff.as_ref().ok_or("no image")?;
    let img = tiff.image(0)?;
    let o = ReadOptions {
        width: Some(w),
        height: Some(h),
        enable_alpha: true,
        interleave: true,
        resample: Some("bilinear".into()),
        ..ReadOptions::default()
    };
    let rgb = img.read_rgb(&o)?;
    let data: &[f64] = rgb.bands.first().map_or(&[][..], |b| &b[..]);
    let n = w * h;
    let nu = terrain::count_of(n);
    let ch = data.len() as f64 / n;
    let bits = max_of(&tag_arr(&img, "BitsPerSample").unwrap_or_else(|| vec![8.0]));
    let k8 = if bits > 8.0 { 255.0 / (cc_civil::js::pow(2.0, bits) - 1.0) } else { 1.0 };
    let mut rgba = vec![0u8; nu.saturating_mul(4)];
    let at = |i: f64| cc_civil::js::usize_of(i).and_then(|k| data.get(k)).copied().unwrap_or(f64::NAN);
    for i in 0..nu {
        let fi = i as f64;
        let px = [at(ch * fi) * k8, at(ch * fi + 1.0) * k8, at(ch * fi + 2.0) * k8, if ch >= 4.0 { at(ch * fi + 3.0) * k8 } else { 255.0 }];
        for (c, v) in px.iter().enumerate() {
            if let Some(slot) = rgba.get_mut(i.saturating_mul(4).saturating_add(c)) {
                *slot = clamp_u8(*v);
            }
        }
    }
    let sx = src.width / w;
    let sy = src.height / h;
    let g = src.gt;
    let affine = [g[0], g[1] * sx, g[2] * sy, g[3], g[4] * sx, g[5] * sy];
    let (mut xs, mut ys) = (Vec::new(), Vec::new());
    for (c, r) in [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)] {
        xs.push(affine[0] + c * affine[1] + r * affine[2]);
        ys.push(affine[3] + c * affine[4] + r * affine[5]);
    }
    let mn = |v: &[f64]| cc_civil::js::min_of(v.iter().copied());
    let mx = |v: &[f64]| cc_civil::js::max_of(v.iter().copied());
    Ok(GeoImage {
        width: w,
        height: h,
        rgba,
        affine,
        bbox: [mn(&xs), mn(&ys), mx(&xs), mx(&ys)],
        crs: src.crs.clone(),
        code: src.code,
        crs_note: src.crs_note.clone(),
        notes,
    })
}
