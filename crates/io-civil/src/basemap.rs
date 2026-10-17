//! basemap.js — map layers under the plan, the DOM-free half: web Mercator tile grids and tile planning, WMS / WMTS
//! capabilities and ArcGIS service descriptions (untrusted: sizes and counts capped, text only), request URLs (WHATWG
//! URLs through the `url` crate, query strings re-serialised as `URLSearchParams` does), world files, warping a picture
//! into the plan in affine pieces, and the project file's map layer records.

use crate::gjs::{push_str_json, stringify};
use crate::xml::{El, parse_xml};
use cc_civil::js::{G, J, JsMath, hypot, int32, max, min, num_to_string, round, slice16, str_to_number, to_fixed};
use cc_civil::tr;
use serde_json::{Map, Value, json};

pub const R_MERC: f64 = 6_378_137.0;
pub const HALF_MERC: f64 = std::f64::consts::PI * R_MERC;
const MAX_TEXT: usize = 8 * 1024 * 1024;
const MAX_LAYERS: usize = 5000;
const MAX_MATRICES: usize = 40;

pub fn is_merc(code: f64) -> bool {
    [3857.0, 900913.0, 102100.0, 102113.0, 3785.0, 41001.0].contains(&code)
}

/// An error with the JS's message.
#[derive(Clone, Debug)]
pub struct MapError(pub String);
type Res<T> = Result<T, MapError>;
fn e<T>(s: String) -> Res<T> {
    Err(MapError(s))
}

// ------------------------------------------------------------------ web Mercator
pub fn lon_lat_to_merc(lon: f64, lat: f64) -> (f64, f64) {
    let l = max(-85.0511287798066, min(85.0511287798066, lat));
    (
        R_MERC * lon * std::f64::consts::PI / 180.0,
        R_MERC * cc_proj::math::log((std::f64::consts::PI / 4.0 + l * std::f64::consts::PI / 360.0).jtan()),
    )
}
pub fn merc_to_lon_lat(x: f64, y: f64) -> (f64, f64) {
    (
        x / R_MERC * 180.0 / std::f64::consts::PI,
        (2.0 * libm::exp(y / R_MERC).jatan() - std::f64::consts::PI / 2.0) * 180.0 / std::f64::consts::PI,
    )
}

/// A tile matrix `{ id, res, tw, th, mw, mh, origin? }`.
#[derive(Clone, Debug, PartialEq)]
pub struct Matrix {
    pub id: String,
    pub res: f64,
    pub tw: f64,
    pub th: f64,
    pub mw: f64,
    pub mh: f64,
    pub origin: Option<(f64, f64)>,
}
impl J for Matrix {
    fn j(&self) -> Value {
        let mut o = cc_civil::js::Obj::new()
            .set("id", &self.id)
            .set("res", self.res)
            .set("tw", self.tw)
            .set("th", self.th)
            .set("mw", self.mw)
            .set("mh", self.mh);
        if let Some((x, y)) = self.origin {
            o = o.set("origin", vec![x, y]);
        }
        o.done()
    }
}
/// A tile grid `{ code, origin, matrices }`.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    pub code: Option<f64>,
    pub origin: (f64, f64),
    pub matrices: Vec<Matrix>,
}
impl J for Grid {
    fn j(&self) -> Value {
        cc_civil::jo! { "code" => self.code, "origin" => vec![self.origin.0, self.origin.1], "matrices" => self.matrices }
    }
}
/// `XYZ_GRID`.
pub fn xyz_grid() -> Grid {
    Grid {
        code: Some(3857.0),
        origin: (-HALF_MERC, HALF_MERC),
        matrices: (0..23)
            .map(|z| {
                let n = 2f64.jpow(f64::from(z));
                Matrix { id: z.to_string(), res: (2.0 * HALF_MERC) / 256.0 / n, tw: 256.0, th: 256.0, mw: n, mh: n, origin: None }
            })
            .collect(),
    }
}
/// `tileAt(lon, lat, z)`.
pub fn tile_at(lon: f64, lat: f64, z: usize) -> (f64, f64) {
    let (x, y) = lon_lat_to_merc(lon, lat);
    let g = xyz_grid();
    let res = g.matrices.get(z).map_or(f64::NAN, |m| m.res);
    (((x + HALF_MERC) / (res * 256.0)).floor(), ((HALF_MERC - y) / (res * 256.0)).floor())
}

fn sorted_by_res(grid: &Grid) -> Vec<Matrix> {
    let mut ms = grid.matrices.clone();
    cc_civil::js::sort_by(&mut ms, |a, b| b.res - a.res);
    ms
}
/// `pickMatrix(grid, res)`.
pub fn pick_matrix(grid: &Grid, res: f64) -> Option<Matrix> {
    let ms = sorted_by_res(grid);
    let mut pick = ms.first().cloned();
    for m in ms {
        let stop = m.res <= res * 1.5;
        pick = Some(m);
        if stop {
            break;
        }
    }
    pick
}
/// A tile `{ col, row, bounds }`.
#[derive(Clone, Debug)]
pub struct Tile {
    pub col: f64,
    pub row: f64,
    pub bounds: [f64; 4],
}
impl J for Tile {
    fn j(&self) -> Value {
        cc_civil::jo! { "col" => self.col, "row" => self.row, "bounds" => self.bounds }
    }
}
/// `gridTiles(grid, bbox, res, { cap })` → (matrix, tiles).
pub fn grid_tiles(grid: &Grid, bbox: [f64; 4], res: f64, cap: f64) -> (Option<Matrix>, Vec<Tile>) {
    let ms = sorted_by_res(grid);
    let pm = pick_matrix(grid, res);
    let mut i: i64 = pm.as_ref().and_then(|p| ms.iter().position(|m| m == p)).map_or(-1, |x| i64::try_from(x).unwrap_or(-1));
    while i >= 0 {
        let Some(m) = ms.get(usize::try_from(i).unwrap_or(0)) else { break };
        let tw = m.res * m.tw;
        let th = m.res * m.th;
        let mut c0 = ((bbox[0] - grid.origin.0) / tw).floor();
        let mut c1 = ((bbox[2] - grid.origin.0) / tw).floor();
        let mut r0 = ((grid.origin.1 - bbox[3]) / th).floor();
        let mut r1 = ((grid.origin.1 - bbox[1]) / th).floor();
        c0 = max(0.0, c0);
        r0 = max(0.0, r0);
        if m.mw.is_finite() {
            c1 = min(m.mw - 1.0, c1);
        }
        if m.mh.is_finite() {
            r1 = min(m.mh - 1.0, r1);
        }
        if !(c1 >= c0 && r1 >= r0) {
            return (Some(m.clone()), Vec::new());
        }
        if (c1 - c0 + 1.0) * (r1 - r0 + 1.0) > cap && i > 0 {
            i = i.saturating_sub(1);
            continue;
        }
        let mut tiles = Vec::new();
        let mut r = r0;
        'rows: while r <= r1 {
            let mut c = c0;
            while c <= c1 {
                if tiles.len() as f64 >= cap {
                    // the JS breaks the column loop only; the next row checks again
                    break;
                }
                tiles.push(Tile {
                    col: c,
                    row: r,
                    bounds: [
                        grid.origin.0 + c * tw,
                        grid.origin.1 - (r + 1.0) * th,
                        grid.origin.0 + (c + 1.0) * tw,
                        grid.origin.1 - r * th,
                    ],
                });
                c += 1.0;
            }
            r += 1.0;
            if tiles.len() > 10_000_000 {
                break 'rows;
            }
        }
        let cx = (bbox[0] + bbox[2]) / 2.0;
        let cy = (bbox[1] + bbox[3]) / 2.0;
        let d = |t: &Tile| hypot((t.bounds[0] + t.bounds[2]) / 2.0 - cx, (t.bounds[1] + t.bounds[3]) / 2.0 - cy);
        cc_civil::js::sort_by(&mut tiles, |a, b| d(a) - d(b));
        return (Some(m.clone()), tiles);
    }
    (ms.first().cloned(), Vec::new())
}

// ------------------------------------------------------------------ CRS names in capabilities
/// `codeOfCrs(text)` → (code, lonFirst, crs84).
pub fn code_of_crs(text: &str) -> Option<(f64, bool, bool)> {
    let s = cc_civil::js::trim(text);
    let l = s.to_ascii_lowercase();
    let digits_dots = |t: &str| t.bytes().all(|c| c.is_ascii_digit() || c == b'.');
    // ^(urn:ogc:def:crs:OGC:[\d.]*:CRS84|CRS:?84|http://www.opengis.net/def/crs/OGC/[\d.]+/CRS84)$
    let crs84 = l == "crs:84"
        || l == "crs84"
        || l.strip_prefix("urn:ogc:def:crs:ogc:").and_then(|r| r.strip_suffix(":crs84")).is_some_and(digits_dots)
        || l.strip_prefix("http://www.opengis.net/def/crs/ogc/")
            .and_then(|r| r.strip_suffix("/crs84"))
            .is_some_and(|m| !m.is_empty() && digits_dots(m));
    if crs84 {
        return Some((4326.0, true, true));
    }
    let code_digits = |t: &str| (3..=6).contains(&t.len()) && t.bytes().all(|c| c.is_ascii_digit());
    let mut digits: Option<&str> = None;
    if let Some(r) = l.strip_prefix("epsg:")
        && code_digits(r)
    {
        digits = Some(r);
    } else if let Some(r) = l.strip_prefix("urn:ogc:def:crs:epsg:") {
        if let Some(i) = r.find(':')
            && digits_dots(r.get(..i).unwrap_or(""))
            && code_digits(r.get(i.saturating_add(1)..).unwrap_or(""))
        {
            digits = r.get(i.saturating_add(1)..);
        }
    } else if let Some(r) =
        l.strip_prefix("https://www.opengis.net/def/crs/epsg/").or_else(|| l.strip_prefix("http://www.opengis.net/def/crs/epsg/"))
        && let Some(i) = r.find('/')
        && i > 0
        && digits_dots(r.get(..i).unwrap_or(""))
        && code_digits(r.get(i.saturating_add(1)..).unwrap_or(""))
    {
        digits = r.get(i.saturating_add(1)..);
    }
    if digits.is_none() && (4..=6).contains(&l.len()) && l.bytes().all(|c| c.is_ascii_digit()) {
        digits = Some(&l);
    }
    let d = digits?;
    let mut code = str_to_number(d);
    if is_merc(code) {
        code = 3857.0;
    }
    let lon_first = l.starts_with("epsg:") || l.as_bytes().first().is_some_and(u8::is_ascii_digit);
    Some((code, lon_first, false))
}

// ------------------------------------------------------------------ XML helpers
fn kids<'a>(e: Option<&'a El>, n: &str) -> Vec<&'a El> {
    e.map_or_else(Vec::new, |e| e.children.iter().filter(|c| c.name == n).collect())
}
fn kid<'a>(e: Option<&'a El>, n: &str) -> Option<&'a El> {
    e.and_then(|e| e.children.iter().find(|c| c.name == n))
}
fn txt(e: Option<&El>, max: usize) -> String {
    clean(e.map_or("", |e| e.text.as_str()), max)
}
fn attr(e: Option<&El>, n: &str) -> String {
    let Some(e) = e else { return String::new() };
    if let Some(v) = e.attr(n) {
        return v.to_string();
    }
    if let Some(v) = e.attr(&format!("xlink:{n}")) {
        return v.to_string();
    }
    let suffix = format!(":{n}");
    e.attrs.iter().find(|(k, _)| k.ends_with(&suffix)).map_or(String::new(), |(_, v)| v.clone())
}
/// `clean(v, max)`: control characters out, white space runs → one space, trimmed, capped.
pub fn clean(v: &str, max: usize) -> String {
    let soft = crate::gjs::soft_controls(v);
    let mut out = String::with_capacity(soft.len());
    let mut ws = false;
    for c in soft.chars() {
        if cc_civil::js::is_ws(c) {
            if !ws {
                out.push(' ');
            }
            ws = true;
        } else {
            out.push(c);
            ws = false;
        }
    }
    slice16(cc_civil::js::trim(&out), max)
}
fn num(v: &str) -> Option<f64> {
    let n = str_to_number(cc_civil::js::trim(v));
    if n.is_finite() { Some(n) } else { None }
}
fn find_text(e: &El, depth: usize) -> String {
    let mut s = e.text.clone();
    if depth > 300 {
        return s;
    }
    let parts: Vec<String> = e.children.iter().map(|c| find_text(c, depth.saturating_add(1))).collect();
    s.push_str(&parts.join(" "));
    s
}

fn read_xml(s: &str, wms: bool) -> Res<El> {
    let k = |x: &str| format!("basemap.{}.{x}", if wms { "wms" } else { "wmts" });
    if cc_civil::js::trim(s).is_empty() {
        return e(tr!("basemap.xml.nothing"));
    }
    if s.encode_utf16().count() > MAX_TEXT {
        return e(tr!(&k("big"), mb = round(s.encode_utf16().count() as f64 / 1_048_576.0)));
    }
    let t0 = cc_civil::js::trim_start(s);
    if t0.starts_with('{') || t0.starts_with('[') {
        return e(tr!(&k("json")));
    }
    let lt = t0.to_ascii_lowercase();
    if lt.starts_with("<!doctype html") || lt.starts_with("<html") {
        return e(tr!(&k("html")));
    }
    let root = parse_xml(s, 64).map_err(|x| MapError(tr!(&k("unreadable"), reason = clean(&x.0, 120))))?;
    if root.name == "ServiceExceptionReport" || root.name == "ExceptionReport" {
        return e(tr!("basemap.server_error", error = clean(&find_text(&root, 0), 200)));
    }
    Ok(root)
}

// ------------------------------------------------------------------ WMS

/// WMS 1.3.0 (and WMTS) put latitude first for geographic EPSG codes but not for CRS:84.
pub fn is_lat_first(code: Option<f64>, crs84: bool) -> bool {
    let Some(c) = code else { return false };
    if crs84 {
        return false;
    }
    [4326.0, 4269.0, 4258.0, 4283.0, 4167.0, 4617.0, 4759.0, 6318.0, 7844.0, 4674.0, 4612.0, 4171.0].contains(&c)
        || ((4000.0..5000.0).contains(&c) && !is_merc(c))
}

#[derive(Clone)]
struct Inh {
    codes: Vec<f64>,
    bboxes: Vec<(f64, [f64; 4])>,
    bbox84: Option<[f64; 4]>,
    styles: Vec<(String, String)>,
    attribution: String,
}

fn bboxes_json(b: &[(f64, [f64; 4])]) -> Value {
    let mut m = Map::new();
    for (k, v) in b {
        m.insert(num_to_string(*k), v.j());
    }
    Value::Object(m)
}

/// `parseWmsCapabilities(text)`.
pub fn parse_wms_capabilities(text: &str) -> Res<Value> {
    let root = read_xml(text, true)?;
    if root.name != "WMS_Capabilities" && root.name != "WMT_MS_Capabilities" {
        return e(tr!("basemap.wms.not", root = clean(&root.name, 60)));
    }
    let v_attr = clean(root.attr("version").unwrap_or(""), 20);
    let version = if !v_attr.is_empty() {
        v_attr
    } else if root.name == "WMS_Capabilities" {
        "1.3.0".into()
    } else {
        "1.1.1".into()
    };
    let v13 = !(version.starts_with("1.0") || version.starts_with("1.1"));
    let cap = kid(Some(&root), "Capability");
    let gm = kid(kid(cap, "Request"), "GetMap");
    let formats: Vec<String> = kids(gm, "Format").into_iter().map(|f| txt(Some(f), 80)).filter(|f| !f.is_empty()).take(50).collect();
    let get = kid(kid(kid(kid(gm, "DCPType"), "HTTP"), "Get"), "OnlineResource");
    let get_map = attr(get, "href");
    let mut layers: Vec<Value> = Vec::new();
    fn walk(el: &El, inh: &Inh, depth: usize, v13: bool, layers: &mut Vec<Value>) {
        if depth > 20 || layers.len() >= MAX_LAYERS {
            return;
        }
        let mut codes = inh.codes.clone();
        for c in kids(Some(el), "CRS").into_iter().chain(kids(Some(el), "SRS")) {
            for part in cc_civil::points::split_ws(&txt(Some(c), 2000)) {
                if let Some((k, _, _)) = code_of_crs(&part)
                    && !codes.contains(&k)
                {
                    codes.push(k);
                }
            }
        }
        let mut bboxes = inh.bboxes.clone();
        for b in kids(Some(el), "BoundingBox") {
            let mut cs = attr(Some(b), "CRS");
            if cs.is_empty() {
                cs = attr(Some(b), "SRS");
            }
            let k = code_of_crs(&cs).map(|x| (x.0, x.2));
            let v: Vec<Option<f64>> = ["minx", "miny", "maxx", "maxy"].iter().map(|n| num(&attr(Some(b), n))).collect();
            if let Some((code, crs84)) = k
                && v.iter().all(Option::is_some)
            {
                let v: Vec<f64> = v.into_iter().flatten().collect();
                let val = if v13 && is_lat_first(Some(code), crs84) {
                    [v.g(1), v.g(0), v.g(3), v.g(2)]
                } else {
                    [v.g(0), v.g(1), v.g(2), v.g(3)]
                };
                match bboxes.iter_mut().find(|(c, _)| *c == code) {
                    Some(slot) => slot.1 = val,
                    None => bboxes.push((code, val)),
                }
            }
        }
        let mut bbox84 = inh.bbox84;
        if let Some(ex) = kid(Some(el), "EX_GeographicBoundingBox") {
            let v: Vec<Option<f64>> = ["westBoundLongitude", "southBoundLatitude", "eastBoundLongitude", "northBoundLatitude"]
                .iter()
                .map(|n| num(&txt(kid(Some(ex), n), 500)))
                .collect();
            if v.iter().all(Option::is_some) {
                let v: Vec<f64> = v.into_iter().flatten().collect();
                bbox84 = Some([v.g(0), v.g(1), v.g(2), v.g(3)]);
            }
        }
        if let Some(ll) = kid(Some(el), "LatLonBoundingBox") {
            let v: Vec<Option<f64>> = ["minx", "miny", "maxx", "maxy"].iter().map(|n| num(&attr(Some(ll), n))).collect();
            if v.iter().all(Option::is_some) {
                let v: Vec<f64> = v.into_iter().flatten().collect();
                bbox84 = Some([v.g(0), v.g(1), v.g(2), v.g(3)]);
            }
        }
        let mut styles = inh.styles.clone();
        for s in kids(Some(el), "Style") {
            let n = txt(kid(Some(s), "Name"), 200);
            if !n.is_empty() && !styles.iter().any(|x| x.0 == n) {
                let t = txt(kid(Some(s), "Title"), 200);
                styles.push((n.clone(), if t.is_empty() { n } else { t }));
            }
        }
        let attribution = match kid(Some(el), "Attribution") {
            Some(at) => {
                let t = txt(kid(Some(at), "Title"), 300);
                if t.is_empty() { inh.attribution.clone() } else { t }
            }
            None => inh.attribution.clone(),
        };
        let name = txt(kid(Some(el), "Name"), 300);
        if !name.is_empty() {
            let title = txt(kid(Some(el), "Title"), 300);
            layers.push(json!({
                "name": name, "title": if title.is_empty() { name.clone() } else { title },
                "codes": codes.iter().take(500).map(|c| c.j()).collect::<Vec<_>>(),
                "bbox84": bbox84.map(|b| b.j()), "bboxes": bboxes_json(&bboxes),
                "styles": styles.iter().take(50).map(|(n, t)| json!({ "name": n, "title": t })).collect::<Vec<_>>(),
                "attribution": attribution, "queryable": el.attr("queryable") == Some("1"),
            }));
        }
        let next = Inh { codes, bboxes, bbox84, styles, attribution };
        for c in kids(Some(el), "Layer") {
            walk(c, &next, depth.saturating_add(1), v13, layers);
        }
    }
    let base = Inh { codes: Vec::new(), bboxes: Vec::new(), bbox84: None, styles: Vec::new(), attribution: String::new() };
    for l in kids(cap, "Layer") {
        walk(l, &base, 0, v13, &mut layers);
    }
    if layers.is_empty() {
        return e(tr!("basemap.wms.no_layers"));
    }
    Ok(json!({
        "version": if v13 { "1.3.0" } else { "1.1.1" },
        "title": txt(kid(kid(Some(&root), "Service"), "Title"), 300),
        "getMap": if get_map.is_empty() { String::new() } else { clean(&get_map, 2000) },
        "formats": formats, "layers": layers,
    }))
}

/// `wmsPickCode(codes, projectCode)`.
pub fn wms_pick_code(codes: &[f64], project: Option<f64>) -> Option<f64> {
    if let Some(p) = project
        && p != 0.0
        && codes.contains(&p)
    {
        return Some(p);
    }
    if codes.contains(&3857.0) {
        return Some(3857.0);
    }
    if codes.contains(&4326.0) {
        return Some(4326.0);
    }
    None
}

fn fmt_num(v: f64) -> String {
    if v.abs() >= 1e4 { to_fixed(v, 3) } else { num_to_string(str_to_number(&cc_m3d::js::to_precision(v, 12))) }
}

// ------------------------------------------------------------------ URLs

/// `checkUrl(text)`: http(s) only, ≤ 2000 characters, no credentials.
pub fn check_url(text: &str) -> Res<url::Url> {
    let s = cc_civil::js::trim(text);
    if s.encode_utf16().count() > 2000 {
        return e(tr!("basemap.url.long"));
    }
    let u = url::Url::parse(s).map_err(|_| MapError(tr!("basemap.url.not")))?;
    if u.scheme() != "https" && u.scheme() != "http" {
        return e(tr!("basemap.url.scheme"));
    }
    if !u.username().is_empty() || u.password().is_some() {
        return e(tr!("basemap.url.creds"));
    }
    Ok(u)
}

/// `URLSearchParams` over a URL's query: the list, edited, then written back as the JS does.
struct Params(Vec<(String, String)>);
impl Params {
    fn of(u: &url::Url) -> Params {
        Params(
            u.query().map_or_else(Vec::new, |q| {
                url::form_urlencoded::parse(q.as_bytes()).map(|(a, b)| (a.into_owned(), b.into_owned())).collect()
            }),
        )
    }
    fn keys(&self) -> Vec<String> {
        self.0.iter().map(|(k, _)| k.clone()).collect()
    }
    fn delete(&mut self, k: &str) {
        self.0.retain(|(a, _)| a != k);
    }
    fn set(&mut self, k: &str, v: &str) {
        match self.0.iter().position(|(a, _)| a == k) {
            Some(i) => {
                if let Some(slot) = self.0.get_mut(i) {
                    slot.1 = v.to_string();
                }
                let mut seen = false;
                self.0.retain(|(a, _)| {
                    if a == k {
                        if seen {
                            return false;
                        }
                        seen = true;
                    }
                    true
                });
            }
            None => self.0.push((k.to_string(), v.to_string())),
        }
    }
    fn write(&self, u: &mut url::Url) {
        let s = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(self.0.iter()).finish();
        if s.is_empty() {
            u.set_query(None);
        } else {
            u.set_query(Some(&s));
        }
    }
}

/// `wmsGetMapUrl({...})`.
#[allow(clippy::too_many_arguments)]
pub fn wms_get_map_url(
    url: &str,
    version: &str,
    layer: &str,
    style: &str,
    format: &str,
    code: f64,
    bbox: [f64; 4],
    width: f64,
    height: f64,
    transparent: bool,
) -> Res<String> {
    let mut u = check_url(url)?;
    let v13 = version != "1.1.1";
    let swap = v13 && is_lat_first(Some(code), false);
    let b = if swap { [bbox[1], bbox[0], bbox[3], bbox[2]] } else { bbox };
    let q: Vec<(&str, String)> = vec![
        ("SERVICE", "WMS".into()),
        ("VERSION", if v13 { "1.3.0" } else { "1.1.1" }.into()),
        ("REQUEST", "GetMap".into()),
        ("LAYERS", layer.into()),
        ("STYLES", style.into()),
        ("FORMAT", format.into()),
        ("TRANSPARENT", if transparent { "TRUE" } else { "FALSE" }.into()),
        (if v13 { "CRS" } else { "SRS" }, format!("EPSG:{}", num_to_string(code))),
        ("BBOX", b.iter().map(|v| fmt_num(*v)).collect::<Vec<_>>().join(",")),
        ("WIDTH", num_to_string(round(width))),
        ("HEIGHT", num_to_string(round(height))),
    ];
    let mut p = Params::of(&u);
    for (k, _) in &q {
        for have in p.keys() {
            if have.to_uppercase() == *k {
                p.delete(&have);
            }
        }
    }
    for (k, v) in &q {
        p.set(k, v);
    }
    p.write(&mut u);
    Ok(u.as_str().to_string())
}

/// `encodeURIComponent(s)`.
pub fn encode_uri_component(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

// ------------------------------------------------------------------ WMTS

/// `parseWmtsCapabilities(text)`.
pub fn parse_wmts_capabilities(text: &str) -> Res<Value> {
    let root = read_xml(text, false)?;
    if root.name != "Capabilities" {
        return e(tr!("basemap.wmts.not", root = clean(&root.name, 60)));
    }
    let Some(contents) = kid(Some(&root), "Contents") else { return e(tr!("basemap.wmts.no_contents")) };
    let mut sets = Map::new();
    for s in kids(Some(contents), "TileMatrixSet").into_iter().take(200) {
        let id = txt(kid(Some(s), "Identifier"), 200);
        if id.is_empty() {
            continue;
        }
        let k = code_of_crs(&txt(kid(Some(s), "SupportedCRS"), 200));
        let mut matrices = Vec::new();
        for m in kids(Some(s), "TileMatrix").into_iter().take(MAX_MATRICES) {
            let scale = num(&txt(kid(Some(m), "ScaleDenominator"), 500));
            let tl: Vec<f64> =
                cc_civil::points::split_ws(&txt(kid(Some(m), "TopLeftCorner"), 100)).iter().map(|x| str_to_number(x)).collect();
            let g = |n: &str| num(&txt(kid(Some(m), n), 500));
            let (tw, th, mw, mh) = (g("TileWidth"), g("TileHeight"), g("MatrixWidth"), g("MatrixHeight"));
            let pos = |v: Option<f64>| v.is_some_and(|x| x > 0.0);
            let le = |v: Option<f64>| v.is_some_and(|x| x <= 4096.0);
            if !pos(scale)
                || tl.len() != 2
                || !tl.iter().all(|x| x.is_finite())
                || !(pos(tw) && le(tw))
                || !(pos(th) && le(th))
                || !pos(mw)
                || !pos(mh)
            {
                continue;
            }
            matrices.push(json!({
                "id": txt(kid(Some(m), "Identifier"), 100), "scale": scale.j(), "tl": tl.j(),
                "tw": tw.j(), "th": th.j(), "mw": mw.j(), "mh": mh.j(),
            }));
        }
        if !matrices.is_empty() {
            let (code, lon_first, crs84) = match k {
                Some((c, l, c84)) => (Some(c), l, c84),
                None => (None, false, false),
            };
            sets.insert(
                id.clone(),
                json!({ "id": id, "code": code.j(), "lonFirst": lon_first || crs84, "crs84": crs84, "latFirst": is_lat_first(code, crs84), "matrices": matrices }),
            );
        }
    }
    let mut layers = Vec::new();
    for l in kids(Some(contents), "Layer").into_iter().take(MAX_LAYERS) {
        let id = txt(kid(Some(l), "Identifier"), 300);
        if id.is_empty() {
            continue;
        }
        let mut bbox84 = Value::Null;
        if let Some(bb) = kid(Some(l), "WGS84BoundingBox") {
            let lo: Vec<f64> =
                cc_civil::points::split_ws(&txt(kid(Some(bb), "LowerCorner"), 100)).iter().map(|x| str_to_number(x)).collect();
            let hi: Vec<f64> =
                cc_civil::points::split_ws(&txt(kid(Some(bb), "UpperCorner"), 100)).iter().map(|x| str_to_number(x)).collect();
            let all: Vec<f64> = lo.iter().chain(hi.iter()).copied().collect();
            if all.len() == 4 && all.iter().all(|x| x.is_finite()) {
                bbox84 = vec![lo.g(0), lo.g(1), hi.g(0), hi.g(1)].j();
            }
        }
        let title = txt(kid(Some(l), "Title"), 300);
        layers.push(json!({
            "id": id, "title": if title.is_empty() { id.clone() } else { title },
            "formats": kids(Some(l), "Format").into_iter().map(|f| txt(Some(f), 80)).filter(|f| !f.is_empty()).take(20).collect::<Vec<_>>(),
            "styles": kids(Some(l), "Style").into_iter().map(|s| (txt(kid(Some(s), "Identifier"), 200), s.attr("isDefault") == Some("true"))).filter(|(i, _)| !i.is_empty()).take(50).map(|(i, d)| json!({ "id": i, "isDefault": d })).collect::<Vec<_>>(),
            "sets": kids(Some(l), "TileMatrixSetLink").into_iter().map(|s| txt(kid(Some(s), "TileMatrixSet"), 200)).filter(|s| sets.contains_key(s)).take(50).collect::<Vec<_>>(),
            "templates": kids(Some(l), "ResourceURL").into_iter()
                .filter(|r| { let t = r.attr("resourceType").unwrap_or(""); (if t.is_empty() { "tile" } else { t }) == "tile" })
                .map(|r| (clean(r.attr("format").unwrap_or(""), 80), clean(r.attr("template").unwrap_or(""), 2000)))
                .filter(|(_, t)| { let l = t.to_ascii_lowercase(); l.starts_with("http://") || l.starts_with("https://") })
                .take(20).map(|(f, t)| json!({ "format": f, "template": t })).collect::<Vec<_>>(),
            "bbox84": bbox84, "attribution": "",
        }));
    }
    if layers.is_empty() {
        return e(tr!("basemap.wmts.no_layers"));
    }
    let mut kvp = String::new();
    'ops: for op in kids(kid(Some(&root), "OperationsMetadata"), "Operation") {
        if op.attr("name") != Some("GetTile") {
            continue;
        }
        for g in kids(kid(kid(Some(op), "DCP"), "HTTP"), "Get") {
            let enc = find_text(g, 0).to_uppercase();
            if !enc.contains("RESTFUL") || enc.contains("KVP") {
                kvp = clean(&attr(Some(g), "href"), 2000);
                if !kvp.is_empty() {
                    break 'ops;
                }
            }
        }
    }
    let prov = txt(kid(kid(Some(&root), "ServiceProvider"), "ProviderName"), 200);
    for l in &mut layers {
        if let Some(o) = l.as_object_mut() {
            o.insert("attribution".into(), Value::String(prov.clone()));
        }
    }
    Ok(
        json!({ "title": txt(kid(kid(Some(&root), "ServiceIdentification"), "Title"), 300), "layers": layers, "sets": Value::Object(sets), "kvp": kvp }),
    )
}

/// `metresPerUnit(unit)`.
pub fn metres_per_unit(unit: &str) -> f64 {
    match unit {
        "deg" => 2.0 * std::f64::consts::PI * 6_378_137.0 / 360.0,
        "ft" => 0.3048,
        "ftUS" => 1200.0 / 3937.0,
        _ => 1.0,
    }
}

/// `wmtsGrid(set, unit, { toSet, bbox84 })` from a set as parseWmtsCapabilities gives it.
#[allow(clippy::type_complexity)]
pub fn wmts_grid(set: &Value, unit: &str, to_set: Option<&dyn Fn(f64, f64) -> Option<(f64, f64)>>, bbox84: Option<[f64; 4]>) -> Grid {
    let k = metres_per_unit(unit);
    let mut swap = set.get("latFirst") == Some(&Value::Bool(true));
    let mats = cc_civil::js::arr(set.get("matrices"));
    let f = |m: &Value, key: &str| cc_civil::js::numf(m.get(key));
    let tl = |m: &Value| {
        let t = cc_civil::js::arr(m.get("tl"));
        (cc_civil::js::numf(t.first()), cc_civil::js::numf(t.get(1)))
    };
    if set.get("crs84") != Some(&Value::Bool(true))
        && !swap
        && let (Some(to_set), Some(bb)) = (to_set, bbox84)
        && let Some((mx, my)) = to_set((bb[0] + bb[2]) / 2.0, (bb[1] + bb[3]) / 2.0)
        && let Some(m) = mats.first()
    {
        let w = f(m, "scale") * 0.00028 / k * f(m, "tw") * f(m, "mw");
        let h = f(m, "scale") * 0.00028 / k * f(m, "th") * f(m, "mh");
        let holds = |(x, y): (f64, f64)| mx >= x && mx <= x + w && my <= y && my >= y - h;
        let t = tl(m);
        if !holds(t) && holds((t.1, t.0)) {
            swap = true;
        }
    }
    let matrices: Vec<Matrix> = mats
        .iter()
        .map(|m| {
            let t = tl(m);
            Matrix {
                id: cc_civil::js::string_or_empty(m.get("id")),
                res: f(m, "scale") * 0.00028 / k,
                tw: f(m, "tw"),
                th: f(m, "th"),
                mw: f(m, "mw"),
                mh: f(m, "mh"),
                origin: Some(if swap { (t.1, t.0) } else { (t.0, t.1) }),
            }
        })
        .collect();
    let origin = matrices.first().and_then(|m| m.origin).unwrap_or((f64::NAN, f64::NAN));
    let code = set.get("code").and_then(Value::as_f64);
    Grid { code, origin, matrices }
}

/// `gridTilesAny(grid, bbox, res, opt)`.
pub fn grid_tiles_any(grid: &Grid, bbox: [f64; 4], res: f64, cap: f64) -> (Option<Matrix>, Vec<Tile>) {
    let Some(m) = pick_matrix(grid, res) else { return grid_tiles(grid, bbox, res, cap) };
    match m.origin {
        None => return grid_tiles(grid, bbox, res, cap),
        Some(o) if o.0 == grid.origin.0 && o.1 == grid.origin.1 => return grid_tiles(grid, bbox, res, cap),
        _ => {}
    }
    let ms = sorted_by_res(grid);
    let start = ms.iter().position(|x| *x == m);
    let mut i: i64 = start.map_or(-1, |x| i64::try_from(x).unwrap_or(-1));
    while i >= 0 {
        let Some(mi) = ms.get(usize::try_from(i).unwrap_or(0)) else { break };
        let one = Grid { code: grid.code, origin: mi.origin.unwrap_or(grid.origin), matrices: vec![mi.clone()] };
        let r = grid_tiles(&one, bbox, mi.res, cap);
        let tw = mi.res * mi.tw;
        let th = mi.res * mi.th;
        let need = (((bbox[2] - one.origin.0) / tw).floor() - ((bbox[0] - one.origin.0) / tw).floor() + 1.0)
            * (((one.origin.1 - bbox[1]) / th).floor() - ((one.origin.1 - bbox[3]) / th).floor() + 1.0);
        if need <= cap || i == 0 {
            return r;
        }
        i = i.saturating_sub(1);
    }
    (Some(m), Vec::new())
}

/// `wmtsTileUrl({...})`.
#[allow(clippy::too_many_arguments)]
pub fn wmts_tile_url(
    template: &str,
    kvp: &str,
    layer: &str,
    style: &str,
    format: &str,
    set: &str,
    matrix: &str,
    row: &str,
    col: &str,
) -> Res<String> {
    if !template.is_empty() {
        let mut out = String::new();
        let mut rest = template;
        while let Some(i) = rest.find('{') {
            out.push_str(rest.get(..i).unwrap_or(""));
            let tail = rest.get(i..).unwrap_or("");
            let name: String = tail.chars().skip(1).take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            let close = tail.get(name.len().saturating_add(1)..).is_some_and(|t| t.starts_with('}'));
            if !name.is_empty() && close {
                let v = match name.to_lowercase().as_str() {
                    "tilematrixset" => Some(set),
                    "tilematrix" => Some(matrix),
                    "tilerow" => Some(row),
                    "tilecol" => Some(col),
                    "style" => Some(style),
                    "layer" => Some(layer),
                    _ => None,
                };
                match v {
                    Some(v) => out.push_str(&encode_uri_component(v)),
                    None => out.push_str(tail.get(..name.len().saturating_add(2)).unwrap_or("")),
                }
                rest = tail.get(name.len().saturating_add(2)..).unwrap_or("");
            } else {
                out.push('{');
                rest = tail.get(1..).unwrap_or("");
            }
        }
        out.push_str(rest);
        return Ok(check_url(&out)?.as_str().to_string());
    }
    let mut u = check_url(kvp)?;
    let mut p = Params::of(&u);
    for (k, v) in [
        ("SERVICE", "WMTS"),
        ("REQUEST", "GetTile"),
        ("VERSION", "1.0.0"),
        ("LAYER", layer),
        ("STYLE", style),
        ("FORMAT", format),
        ("TILEMATRIXSET", set),
        ("TILEMATRIX", matrix),
        ("TILEROW", row),
        ("TILECOL", col),
    ] {
        p.set(k, v);
    }
    p.write(&mut u);
    Ok(u.as_str().to_string())
}

// ------------------------------------------------------------------ ArcGIS REST

/// `arcgisBase(text)` → (base, kind).
pub fn arcgis_base(text: &str) -> Res<(String, String)> {
    let mut u = check_url(text)?;
    let path = u.path().to_string();
    let lower = path.to_ascii_lowercase();
    let mut found: Option<(usize, &str)> = None;
    let mut i = 0usize;
    while i < lower.len() {
        let rest = lower.get(i..).unwrap_or("");
        for (pat, kind) in [("/mapserver", "MapServer"), ("/imageserver", "ImageServer")] {
            if rest.starts_with(pat) {
                let end = i.saturating_add(pat.len());
                if end == lower.len() || lower.as_bytes().get(end) == Some(&b'/') {
                    found = Some((end, kind));
                }
            }
        }
        if found.is_some() {
            break;
        }
        i = i.saturating_add(lower.get(i..).and_then(|r| r.chars().next()).map_or(1, char::len_utf8));
    }
    let Some((end, kind)) = found else { return e(tr!("basemap.arc.url")) };
    let p1 = path.get(..end).unwrap_or("").to_string();
    u.set_path(&p1);
    u.set_query(None);
    u.set_fragment(None);
    let href = u.as_str();
    Ok((href.strip_suffix('/').unwrap_or(href).to_string(), kind.to_string()))
}

/// `parseArcgisService(json, url)`.
pub fn parse_arcgis_service(j: &Value, url: &str) -> Res<Value> {
    let parsed;
    let j = match j {
        Value::String(s) => {
            if s.encode_utf16().count() > MAX_TEXT {
                return e(tr!("basemap.arc.big"));
            }
            parsed = serde_json::from_str::<Value>(s).map_err(|_| MapError(tr!("basemap.arc.not_json")))?;
            &parsed
        }
        other => other,
    };
    let Some(o) = j.as_object() else { return e(tr!("basemap.arc.not_desc")) };
    if let Some(er) = o.get("error").filter(|v| cc_civil::js::truthy(Some(v))) {
        let code = er.get("code").and_then(Value::as_f64);
        let msg = match er.get("message").filter(|v| cc_civil::js::truthy(Some(v))) {
            Some(m) => cc_civil::js::to_js_string(m),
            None => cc_civil::js::string_or_empty(er.get("code")),
        };
        let key = if code == Some(499.0) || code == Some(498.0) { "basemap.arc.error_signin" } else { "basemap.arc.error" };
        return e(tr!(key, error = clean(&msg, 200)));
    }
    let (_, kind) = arcgis_base(url)?;
    let first_obj = |vals: [Option<&Value>; 3]| vals.into_iter().flatten().find(|v| cc_civil::js::truthy(Some(v))).cloned();
    let sr = first_obj([
        o.get("spatialReference"),
        o.get("extent").and_then(|e| e.get("spatialReference")),
        o.get("fullExtent").and_then(|e| e.get("spatialReference")),
    ])
    .unwrap_or(Value::Object(Map::new()));
    let code_of = |s: Option<&Value>| -> Option<f64> {
        let s = s?;
        let lw = s.get("latestWkid");
        let v = if cc_civil::js::truthy(lw) { lw } else { s.get("wkid") };
        let c = cc_civil::js::number(v);
        if cc_civil::js::is_integer(c) && c > 0.0 { Some(if is_merc(c) { 3857.0 } else { c }) } else { None }
    };
    let code = code_of(Some(&sr));
    let mut tiled = false;
    let mut grid = Value::Null;
    if let Some(ti) = o.get("tileInfo").filter(|t| t.is_object() || t.is_array())
        && (o.get("singleFusedMapCache") == Some(&Value::Bool(true)) || kind == "ImageServer")
        && ti.get("lods").is_some_and(Value::is_array)
    {
        let tc = code_of(ti.get("spatialReference")).or(code);
        let n = |v: Option<&Value>| -> Option<f64> {
            let x = str_to_number(cc_civil::js::trim(&match v {
                None | Some(Value::Null) => String::new(),
                Some(x) => cc_civil::js::to_js_string(x),
            }));
            if x.is_finite() { Some(x) } else { None }
        };
        let rows = n(ti.get("rows"));
        let cols = n(ti.get("cols"));
        let ox = n(ti.get("origin").and_then(|o| o.get("x")));
        let oy = n(ti.get("origin").and_then(|o| o.get("y")));
        let lods: Vec<(String, f64)> = cc_civil::js::arr(ti.get("lods"))
            .iter()
            .take(MAX_MATRICES)
            .map(|l| {
                // Math.trunc(null) = 0: a level that does not read as a number is level 0
                let lv = n(l.get("level")).map_or(0.0, f64::trunc);
                (num_to_string(lv), n(l.get("resolution")))
            })
            .filter(|(id, res)| res.is_some_and(|r| r > 0.0) && !id.is_empty() && id.bytes().all(|c| c.is_ascii_digit()))
            .map(|(id, r)| (id, r.unwrap_or(f64::NAN)))
            .collect();
        let ok = |v: Option<f64>| v.is_some_and(|x| x > 0.0 && x <= 4096.0);
        if tc == Some(3857.0) && ok(rows) && ok(cols) && ox.is_some() && oy.is_some() && !lods.is_empty() {
            tiled = true;
            let (cols, rows) = (cols.unwrap_or(0.0), rows.unwrap_or(0.0));
            grid = Grid {
                code: Some(3857.0),
                origin: (ox.unwrap_or(0.0), oy.unwrap_or(0.0)),
                matrices: lods
                    .into_iter()
                    .map(|(id, res)| Matrix { id, res, tw: cols, th: rows, mw: f64::INFINITY, mh: f64::INFINITY, origin: None })
                    .collect(),
            }
            .j();
        }
    }
    let ext = first_obj([o.get("fullExtent"), o.get("extent"), o.get("initialExtent")]);
    let fin = |v: Option<&Value>| v.and_then(Value::as_f64).filter(|x| x.is_finite());
    let extent = match &ext {
        Some(x) if ["xmin", "ymin", "xmax", "ymax"].iter().all(|k| fin(x.get(*k)).is_some()) => json!({
            "bbox": [fin(x.get("xmin")).j(), fin(x.get("ymin")).j(), fin(x.get("xmax")).j(), fin(x.get("ymax")).j()],
            "code": code_of(x.get("spatialReference")).or(code).j(),
        }),
        _ => Value::Null,
    };
    let s = |v: Option<&Value>| match v.filter(|v| cc_civil::js::truthy(Some(v))) {
        Some(v) => cc_civil::js::to_js_string(v),
        None => String::new(),
    };
    let name = {
        let a = s(o.get("mapName"));
        let b = s(o.get("name"));
        let c = s(o.get("documentInfo").and_then(|d| d.get("Title")));
        if !a.is_empty() {
            a
        } else if !b.is_empty() {
            b
        } else {
            c
        }
    };
    Ok(json!({
        "kind": kind, "name": clean(&name, 200), "code": code.j(), "tiled": tiled, "grid": grid, "extent": extent,
        "attribution": clean(&s(o.get("copyrightText")), 300), "formats": clean(&s(o.get("supportedImageFormatTypes")), 300),
    }))
}

/// `arcgisExportUrl({...})`.
pub fn arcgis_export_url(base: &str, kind: &str, bbox: [f64; 4], code: f64, width: f64, height: f64) -> Res<String> {
    let b = base.strip_suffix('/').unwrap_or(base);
    let mut u = check_url(&format!("{b}{}", if kind == "ImageServer" { "/exportImage" } else { "/export" }))?;
    let mut p = Params::of(&u);
    let size = format!("{},{}", num_to_string(round(width)), num_to_string(round(height)));
    let bb = bbox.iter().map(|v| fmt_num(*v)).collect::<Vec<_>>().join(",");
    let cs = num_to_string(code);
    for (k, v) in [
        ("bbox", bb.as_str()),
        ("bboxSR", cs.as_str()),
        ("imageSR", cs.as_str()),
        ("size", size.as_str()),
        ("format", if kind == "ImageServer" { "png" } else { "png32" }),
        ("transparent", "true"),
        ("f", "image"),
    ] {
        p.set(k, v);
    }
    p.write(&mut u);
    Ok(u.as_str().to_string())
}
/// `arcgisTileUrl(base, level, row, col)`.
pub fn arcgis_tile_url(base: &str, level: &str, row: &str, col: &str) -> Res<String> {
    let b = base.strip_suffix('/').unwrap_or(base);
    Ok(check_url(&format!("{b}/tile/{level}/{row}/{col}"))?.as_str().to_string())
}

// ------------------------------------------------------------------ XYZ templates

/// `checkTemplate(text)`.
pub fn check_template(text: &str) -> Res<String> {
    let s = cc_civil::js::trim(text).to_string();
    let l = s.to_ascii_lowercase();
    if !(l.starts_with("http://") || l.starts_with("https://")) {
        return e(tr!("basemap.tile.https"));
    }
    if s.encode_utf16().count() > 2000 {
        return e(tr!("basemap.url.long"));
    }
    if !s.contains("{z}") || !s.contains("{x}") || !(s.contains("{y}") || s.contains("{-y}")) {
        return e(tr!("basemap.tile.xyz"));
    }
    // /\{(?!z\}|x\}|y\}|-y\}|s\})[^}]*\}/
    let mut from = 0usize;
    while let Some(i) = s.get(from..).and_then(|r| r.find('{')).map(|k| k.saturating_add(from)) {
        let tail = s.get(i..).unwrap_or("");
        let okp = ["{z}", "{x}", "{y}", "{-y}", "{s}"].iter().any(|p| tail.starts_with(p));
        if !okp && let Some(j) = tail.find('}') {
            return e(tr!("basemap.tile.placeholder", name = tail.get(..j.saturating_add(1)).unwrap_or("")));
        }
        from = i.saturating_add(1);
    }
    check_url(&replace_placeholders(&s))?;
    Ok(s)
}
/// `.replace(/\{-?\w+\}/g, '0')`.
fn replace_placeholders(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('{') {
        out.push_str(rest.get(..i).unwrap_or(""));
        let tail = rest.get(i..).unwrap_or("");
        let body = tail.get(1..).unwrap_or("");
        let neg = body.starts_with('-');
        let w = body.get(usize::from(neg)..).unwrap_or("");
        let n = w.bytes().take_while(|c| c.is_ascii_alphanumeric() || *c == b'_').count();
        let len = usize::from(neg).saturating_add(n);
        if n > 0 && body.as_bytes().get(len) == Some(&b'}') {
            out.push('0');
            rest = tail.get(len.saturating_add(2)..).unwrap_or("");
        } else {
            out.push('{');
            rest = body;
        }
    }
    out.push_str(rest);
    out
}
/// `xyzUrl(template, z, x, y, subdomains)`.
pub fn xyz_url(template: &str, z: f64, x: f64, y: f64, subdomains: &str) -> String {
    let subs0: String = (if subdomains.is_empty() { "abc" } else { subdomains }).chars().filter(char::is_ascii_alphanumeric).collect();
    let subs = if subs0.is_empty() { "abc".to_string() } else { subs0 };
    let k = (x + y) % subs.chars().count() as f64;
    let s = cc_civil::js::usize_of(k)
        .filter(|_| k >= 0.0 && cc_civil::js::is_integer(k))
        .and_then(|i| subs.chars().nth(i))
        .map_or_else(|| "undefined".to_string(), |c| c.to_string());
    template
        .replacen("{z}", &num_to_string(z), 1)
        .replacen("{x}", &num_to_string(x), 1)
        .replacen("{-y}", &num_to_string(2f64.jpow(z) - 1.0 - y), 1)
        .replacen("{y}", &num_to_string(y), 1)
        .replacen("{s}", &s, 1)
}

/// `capabilitiesUrl(text, service)`.
pub fn capabilities_url(text: &str, service: &str) -> Res<String> {
    let mut u = check_url(text)?;
    if service == "WMTS" && u.path().to_ascii_lowercase().ends_with("/1.0.0/wmtscapabilities.xml") {
        return Ok(u.as_str().to_string());
    }
    let mut p = Params::of(&u);
    for k in ["SERVICE", "REQUEST", "VERSION"] {
        if let Some(h) = p.keys().into_iter().find(|x| x.to_uppercase() == k) {
            p.delete(&h);
        }
    }
    p.set("SERVICE", service);
    p.set("REQUEST", "GetCapabilities");
    if service == "WMS" {
        p.set("VERSION", "1.3.0");
    }
    p.write(&mut u);
    Ok(u.as_str().to_string())
}

// ------------------------------------------------------------------ world files

/// `parseWorldFile(text)` → GDAL geotransform.
pub fn parse_world_file(s: &str) -> Res<[f64; 6]> {
    if s.encode_utf16().count() > 10000 {
        return e(tr!("basemap.world.long"));
    }
    let v: Vec<f64> = cc_civil::points::split_ws(s).iter().map(|x| str_to_number(x)).collect();
    if v.len() != 6 || !v.iter().all(|x| x.is_finite()) {
        return e(tr!("basemap.world.six"));
    }
    let (a, d, b, ee, c, f) = (v.g(0), v.g(1), v.g(2), v.g(3), v.g(4), v.g(5));
    if (a * ee - b * d).abs() < 1e-30 {
        return e(tr!("basemap.world.zero"));
    }
    Ok([c - a / 2.0 - b / 2.0, a, b, f - d / 2.0 - ee / 2.0, d, ee])
}
/// `worldFileFor(name)`.
pub fn world_file_for(name: &str) -> Vec<String> {
    let Some(dot) = name.rfind('.') else { return Vec::new() };
    let stem = name.get(..dot).unwrap_or("");
    let ext = name.get(dot.saturating_add(1)..).unwrap_or("");
    if ext.is_empty() {
        return Vec::new();
    }
    let e = ext.to_lowercase();
    let u: Vec<u16> = e.encode_utf16().collect();
    let short =
        if u.len() >= 3 { String::from_utf16_lossy(&[u.g(0), u.g(u.len().saturating_sub(1)), u16::from(b'w')]) } else { format!("{e}w") };
    [format!("{stem}.{short}"), format!("{stem}.{e}w"), format!("{stem}.wld"), format!("{name}.wld"), format!("{name}.aux.wld")]
        .iter()
        .map(|s| s.to_lowercase())
        .collect()
}
/// `pixelToSystem(gt, u, v)`.
pub fn pixel_to_system(gt: &[f64; 6], u: f64, v: f64) -> (f64, f64) {
    (gt[0] + u * gt[1] + v * gt[2], gt[3] + u * gt[4] + v * gt[5])
}

// ------------------------------------------------------------------ warping

/// `affineFrom3(src, dst)` → [a, b, c, d, e, f].
pub fn affine_from3(s: [[f64; 2]; 3], d: [[f64; 2]; 3]) -> Option<[f64; 6]> {
    let ([u0, v0], [u1, v1], [u2, v2]) = (s[0], s[1], s[2]);
    let ([x0, y0], [x1, y1], [x2, y2]) = (d[0], d[1], d[2]);
    let det = (u1 - u0) * (v2 - v0) - (u2 - u0) * (v1 - v0);
    if det.abs() < 1e-18 {
        return None;
    }
    let a = ((x1 - x0) * (v2 - v0) - (x2 - x0) * (v1 - v0)) / det;
    let c = ((x2 - x0) * (u1 - u0) - (x1 - x0) * (u2 - u0)) / det;
    let b = ((y1 - y0) * (v2 - v0) - (y2 - y0) * (v1 - v0)) / det;
    let dd = ((y2 - y0) * (u1 - u0) - (y1 - y0) * (u2 - u0)) / det;
    Some([a, b, c, dd, x0 - a * u0 - c * v0, y0 - b * u0 - dd * v0])
}
pub fn apply_affine(m: &[f64; 6], u: f64, v: f64) -> [f64; 2] {
    [m[0] * u + m[2] * v + m[4], m[1] * u + m[3] * v + m[5]]
}

/// A warp cell.
#[derive(Clone, Debug)]
pub struct Cell {
    pub src: [[f64; 2]; 4],
    pub dst: [[f64; 2]; 4],
    pub m: [f64; 6],
    pub err: f64,
}
impl J for Cell {
    fn j(&self) -> Value {
        cc_civil::jo! { "src" => self.src.map(|p| p.to_vec()).to_vec(), "dst" => self.dst.map(|p| p.to_vec()).to_vec(), "m" => self.m.to_vec(), "err" => self.err }
    }
}

/// `planWarp(pixToScreen, w, h, { tol, maxDepth, minCells })`.
pub fn plan_warp(pix: &dyn Fn(f64, f64) -> Option<(f64, f64)>, w: f64, h: f64, tol: f64, max_depth: u32, min_cells: u32) -> Vec<Cell> {
    struct St<'a> {
        pix: &'a dyn Fn(f64, f64) -> Option<(f64, f64)>,
        cache: std::collections::HashMap<(u64, u64), Option<[f64; 2]>>,
        out: Vec<Cell>,
        tol: f64,
        max_depth: u32,
        min_cells: u32,
    }
    fn key(x: f64) -> u64 {
        if x == 0.0 { 0 } else { x.to_bits() }
    }
    impl St<'_> {
        fn p(&mut self, u: f64, v: f64) -> Option<[f64; 2]> {
            let k = (key(u), key(v));
            if let Some(r) = self.cache.get(&k) {
                return *r;
            }
            let r = (self.pix)(u, v).filter(|(x, y)| x.is_finite() && y.is_finite()).map(|(x, y)| [x, y]);
            self.cache.insert(k, r);
            r
        }
        fn cell(&mut self, u0: f64, v0: f64, u1: f64, v1: f64, depth: u32) {
            if self.out.len() > 4_000_000 {
                return;
            }
            let c = [self.p(u0, v0), self.p(u1, v0), self.p(u1, v1), self.p(u0, v1)];
            if c.iter().any(Option::is_none) {
                if depth < self.max_depth {
                    self.split(u0, v0, u1, v1, depth);
                }
                return;
            }
            let c = c.map(|p| p.unwrap_or([f64::NAN; 2]));
            let m = affine_from3([[u0, v0], [u1, v0], [u0, v1]], [c[0], c[1], c[3]]);
            let mut err = f64::INFINITY;
            if let Some(m) = &m {
                let um = (u0 + u1) / 2.0;
                let vm = (v0 + v1) / 2.0;
                err = 0.0;
                for (u, v) in [(u1, v1), (um, vm), (um, v0), (u1, vm), (um, v1), (u0, vm)] {
                    let Some(ev) = self.p(u, v) else {
                        err = f64::INFINITY;
                        break;
                    };
                    let a = apply_affine(m, u, v);
                    err = max(err, hypot(a[0] - ev[0], a[1] - ev[1]));
                }
            }
            if (err > self.tol || depth < self.min_cells) && depth < self.max_depth && u1 - u0 > 1.0 && v1 - v0 > 1.0 {
                self.split(u0, v0, u1, v1, depth);
                return;
            }
            let Some(m) = m else { return };
            let m2 = affine_from3([[u1, v1], [u0, v1], [u1, v0]], [c[2], c[3], c[1]]);
            let avg = match m2 {
                Some(m2) => {
                    let mut a = [0.0; 6];
                    for (i, x) in a.iter_mut().enumerate() {
                        *x = (m.g(i) + m2.g(i)) / 2.0;
                    }
                    a
                }
                None => m,
            };
            self.out.push(Cell { src: [[u0, v0], [u1, v0], [u1, v1], [u0, v1]], dst: c, m: avg, err });
        }
        fn split(&mut self, u0: f64, v0: f64, u1: f64, v1: f64, depth: u32) {
            let um = (u0 + u1) / 2.0;
            let vm = (v0 + v1) / 2.0;
            let d = depth.saturating_add(1);
            self.cell(u0, v0, um, vm, d);
            self.cell(um, v0, u1, vm, d);
            self.cell(u0, vm, um, v1, d);
            self.cell(um, vm, u1, v1, d);
        }
    }
    // the JS recursion is bounded by maxDepth; cap it so a hostile depth cannot exhaust the stack
    let mut st = St { pix, cache: std::collections::HashMap::new(), out: Vec::new(), tol, max_depth: max_depth.min(24), min_cells };
    st.cell(0.0, 0.0, w, h, 0);
    st.out
}

/// `viewSourceBox(vb, toSource, n)`.
pub fn view_source_box(vb: [f64; 4], to_source: &dyn Fn(f64, f64) -> Option<(f64, f64)>, n: u32) -> Option<[f64; 4]> {
    let (mut xs, mut ys) = (Vec::new(), Vec::new());
    let nf = f64::from(n);
    for i in 0..=n {
        let t = f64::from(i) / nf;
        let (min_x, min_y, max_x, max_y) = (vb[0], vb[1], vb[2], vb[3]);
        for (x, y) in [
            (min_x + (max_x - min_x) * t, min_y),
            (min_x + (max_x - min_x) * t, max_y),
            (min_x, min_y + (max_y - min_y) * t),
            (max_x, min_y + (max_y - min_y) * t),
        ] {
            if let Some((px, py)) = to_source(x, y)
                && px.is_finite()
                && py.is_finite()
            {
                xs.push(px);
                ys.push(py);
            }
        }
    }
    if xs.is_empty() {
        return None;
    }
    Some([
        cc_civil::js::min_of(xs.iter().copied()),
        cc_civil::js::min_of(ys.iter().copied()),
        cc_civil::js::max_of(xs.iter().copied()),
        cc_civil::js::max_of(ys.iter().copied()),
    ])
}

// ------------------------------------------------------------------ the project file's map layers (untrusted)

pub const KINDS: [&str; 5] = ["image", "xyz", "wms", "wmts", "arcgis"];

/// ToNumber for comparisons on untrusted JSON (strings coerce as in JS).
fn jsn(v: Option<&Value>) -> f64 {
    cc_civil::js::number(v)
}
fn finite_num(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|x| x.is_finite())
}

/// `readGrid(text)` → the grid (Infinity for open-ended matrices).
pub fn read_grid(g: &Value) -> Res<Grid> {
    let bad = || MapError("grid".into());
    let parsed;
    let g = match g {
        Value::String(s) => {
            parsed = if s.encode_utf16().count() > 200_000 { Value::Null } else { serde_json::from_str::<Value>(s).map_err(|_| bad())? };
            &parsed
        }
        o => o,
    };
    let o = g.as_object().ok_or_else(bad)?;
    let mats = o.get("matrices").and_then(Value::as_array).ok_or_else(bad)?;
    let origin = o.get("origin").and_then(Value::as_array).filter(|a| a.len() == 2).ok_or_else(bad)?;
    let (ox, oy) = (finite_num(origin.first()).ok_or_else(bad)?, finite_num(origin.get(1)).ok_or_else(bad)?);
    let mut out = Vec::new();
    for m in mats.iter().take(MAX_MATRICES) {
        let Some(mo) = m.as_object() else { continue };
        let Some(id) = mo.get("id").and_then(Value::as_str) else { continue };
        let res = jsn(mo.get("res"));
        let (tw, th) = (jsn(mo.get("tw")), jsn(mo.get("th")));
        let ok = id.encode_utf16().count() <= 100
            && res > 0.0
            && finite_num(mo.get("res")).is_some()
            && tw > 0.0
            && tw <= 4096.0
            && th > 0.0
            && th <= 4096.0;
        if !ok {
            continue;
        }
        let pos_or_inf = |v: Option<&Value>| finite_num(v).filter(|x| *x > 0.0).unwrap_or(f64::INFINITY);
        let origin_m = mo
            .get("origin")
            .and_then(Value::as_array)
            .filter(|a| a.len() == 2)
            .and_then(|a| Some((finite_num(a.first())?, finite_num(a.get(1))?)));
        out.push(Matrix {
            id: clean(id, 100),
            res,
            tw: int32(tw),
            th: int32(th),
            mw: pos_or_inf(mo.get("mw")),
            mh: pos_or_inf(mo.get("mh")),
            origin: origin_m,
        });
    }
    if out.is_empty() {
        return Err(bad());
    }
    let code = o.get("code").and_then(Value::as_f64).filter(|c| cc_civil::js::is_integer(*c));
    Ok(Grid { code, origin: (ox, oy), matrices: out })
}

/// `gridJson(g)` (open-ended sizes left out).
pub fn grid_json(g: &Grid) -> String {
    let mut s = String::from("{\"code\":");
    match g.code {
        Some(c) => s.push_str(&num_to_string(c)),
        None => s.push_str("null"),
    }
    s.push_str(&format!(",\"origin\":[{},{}],\"matrices\":[", jn(g.origin.0), jn(g.origin.1)));
    for (i, m) in g.matrices.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str("{\"id\":");
        push_str_json(&mut s, &m.id);
        s.push_str(&format!(",\"res\":{},\"tw\":{},\"th\":{}", jn(m.res), jn(m.tw), jn(m.th)));
        if m.mw.is_finite() {
            s.push_str(&format!(",\"mw\":{}", jn(m.mw)));
        }
        if m.mh.is_finite() {
            s.push_str(&format!(",\"mh\":{}", jn(m.mh)));
        }
        if let Some((x, y)) = m.origin {
            s.push_str(&format!(",\"origin\":[{},{}]", jn(x), jn(y)));
        }
        s.push('}');
    }
    s.push_str("]}");
    s
}
/// A number as JSON.stringify writes it (non-finite → null).
fn jn(x: f64) -> String {
    if x.is_finite() { num_to_string(x) } else { "null".into() }
}
/// `JSON.stringify(readGrid(g))` (Infinity → null, as JSON.stringify does).
fn grid_stringified(g: &Grid) -> String {
    let mut s = String::from("{\"code\":");
    s.push_str(&g.code.map_or("null".into(), num_to_string));
    s.push_str(&format!(",\"origin\":[{},{}],\"matrices\":[", jn(g.origin.0), jn(g.origin.1)));
    for (i, m) in g.matrices.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str("{\"id\":");
        push_str_json(&mut s, &m.id);
        s.push_str(&format!(",\"res\":{},\"tw\":{},\"th\":{},\"mw\":{},\"mh\":{}", jn(m.res), jn(m.tw), jn(m.th), jn(m.mw), jn(m.mh)));
        if let Some((x, y)) = m.origin {
            s.push_str(&format!(",\"origin\":[{},{}]", jn(x), jn(y)));
        }
        s.push('}');
    }
    s.push_str("]}");
    s
}

fn check_template2(t: &str) -> Res<String> {
    let mut out = String::new();
    let mut rest = t;
    while let Some(i) = rest.find('{') {
        out.push_str(rest.get(..i).unwrap_or(""));
        let tail = rest.get(i..).unwrap_or("");
        let n = tail.get(1..).map_or(0, |b| b.bytes().take_while(|c| c.is_ascii_alphanumeric() || *c == b'_').count());
        if n > 0 && tail.as_bytes().get(n.saturating_add(1)) == Some(&b'}') {
            out.push('0');
            rest = tail.get(n.saturating_add(2)..).unwrap_or("");
        } else {
            out.push('{');
            rest = tail.get(1..).unwrap_or("");
        }
    }
    out.push_str(rest);
    check_url(&out)?;
    if t.encode_utf16().count() > 2000 {
        return e("long".into());
    }
    Ok(t.to_string())
}

fn read_map(m: &Value, images: Option<&[String]>) -> Res<Value> {
    let bad = |w: &str| MapError(w.to_string());
    let o = m.as_object().ok_or_else(|| bad("kind"))?;
    let kind = o.get("kind").and_then(Value::as_str).filter(|k| KINDS.contains(k)).ok_or_else(|| bad("kind"))?;
    let s = |k: &str, n: usize| match o.get(k) {
        Some(Value::String(v)) => clean(v, n),
        _ => String::new(),
    };
    let f = |k: &str| finite_num(o.get(k));
    let id_ok = o.get("id").and_then(Value::as_str).is_some_and(|i| {
        i.len() >= 2 && i.len() <= 10 && i.starts_with('m') && i.get(1..).is_some_and(|d| d.bytes().all(|c| c.is_ascii_digit()))
    });
    // Spec gap: the JS makes up a random id ('m' + a random number) for a record without a valid one; here it is
    // 'm0' (made unique by readMaps like any repeat).
    let id = if id_ok { o.get("id").and_then(Value::as_str).unwrap_or("m0").to_string() } else { "m0".to_string() };
    let name = {
        let n = s("name", 120);
        if n.is_empty() { tr!("mapl.default_name") } else { n }
    };
    let crs_code = o.get("crsCode").and_then(Value::as_f64).filter(|c| cc_civil::js::is_integer(*c) && *c > 0.0 && *c < 1e6);
    let crs_proj4 = match o.get("crsProj4") {
        Some(Value::String(p)) if p.encode_utf16().count() <= 20000 => {
            cc_civil::js::trim(&cc_civil::js::strip_controls(p, " ")).to_string()
        }
        _ => String::new(),
    };
    let crs_unit = o.get("crsUnit").and_then(Value::as_str).filter(|u| ["m", "ft", "ftUS", "deg"].contains(u)).unwrap_or("m");
    let mut r: Vec<(String, Value)> = vec![
        ("id".into(), Value::String(id)),
        ("name".into(), Value::String(name)),
        ("kind".into(), Value::String(kind.into())),
        ("url".into(), Value::String(String::new())),
        ("layer".into(), Value::String(s("layer", 300))),
        ("style".into(), Value::String(s("style", 200))),
        (
            "format".into(),
            Value::String({
                let x = s("format", 80);
                if x.is_empty() { "image/png".into() } else { x }
            }),
        ),
        ("version".into(), Value::String(if o.get("version").and_then(Value::as_str) == Some("1.1.1") { "1.1.1" } else { "1.3.0" }.into())),
        ("crsCode".into(), crs_code.j()),
        ("crsName".into(), Value::String(s("crsName", 200))),
        ("crsProj4".into(), Value::String(crs_proj4.clone())),
        ("crsUnit".into(), Value::String(crs_unit.into())),
        ("opacity".into(), f("opacity").map_or(Value::from(1), |v| max(0.0, min(1.0, v)).j())),
        ("visible".into(), Value::Bool(o.get("visible") != Some(&Value::Bool(false)))),
        ("attribution".into(), Value::String(s("attribution", 300))),
        ("image".into(), Value::String(String::new())),
        ("width".into(), Value::from(0)),
        ("height".into(), Value::from(0)),
        ("a0".into(), Value::from(0)),
        ("a1".into(), Value::from(1)),
        ("a2".into(), Value::from(0)),
        ("a3".into(), Value::from(0)),
        ("a4".into(), Value::from(0)),
        ("a5".into(), Value::from(-1)),
        ("grid".into(), Value::String(String::new())),
        ("set".into(), Value::String(s("set", 200))),
        ("template".into(), Value::String(String::new())),
        ("subdomains".into(), Value::String(s("subdomains", 20).chars().filter(char::is_ascii_alphanumeric).collect())),
    ];
    let set = |r: &mut Vec<(String, Value)>, k: &str, v: Value| {
        if let Some(slot) = r.iter_mut().find(|(a, _)| a == k) {
            slot.1 = v;
        }
    };
    if kind == "image" {
        let img = o.get("image").and_then(Value::as_str).unwrap_or("");
        let img_ok = img.len() >= 4
            && img.len() <= 12
            && img.starts_with("img")
            && img.get(3..).is_some_and(|d| !d.is_empty() && d.bytes().all(|c| c.is_ascii_digit()));
        if !img_ok || images.is_some_and(|l| !l.iter().any(|x| x == img)) {
            return Err(bad("image"));
        }
        set(&mut r, "image", Value::String(img.into()));
        let dim =
            |k: &str| o.get(k).and_then(Value::as_f64).filter(|v| cc_civil::js::is_integer(*v) && *v > 0.0 && *v <= 65536.0).unwrap_or(0.0);
        let (w, h) = (dim("width"), dim("height"));
        set(&mut r, "width", w.j());
        set(&mut r, "height", h.j());
        let a: Vec<Option<f64>> = ["a0", "a1", "a2", "a3", "a4", "a5"].iter().map(|k| f(k)).collect();
        if w == 0.0 || h == 0.0 || a.iter().any(Option::is_none) {
            return Err(bad("affine"));
        }
        let a: Vec<f64> = a.into_iter().flatten().collect();
        if (a.g(1) * a.g(5) - a.g(2) * a.g(4)).abs() < 1e-30 {
            return Err(bad("affine"));
        }
        for (i, k) in ["a0", "a1", "a2", "a3", "a4", "a5"].iter().enumerate() {
            set(&mut r, k, a.g(i).j());
        }
    } else {
        let url = cc_civil::js::string_or_empty(o.get("url"));
        let u = check_url(&url)?;
        set(&mut r, "url", Value::String(u.as_str().into()));
        if kind == "xyz" {
            set(&mut r, "url", Value::String(check_template(&url)?));
        }
        if kind == "wmts" && cc_civil::js::truthy(o.get("template")) {
            set(
                &mut r,
                "template",
                Value::String(check_template2(&cc_civil::js::to_js_string(o.get("template").unwrap_or(&Value::Null)))?),
            );
        }
        if kind == "wmts" || kind == "arcgis" {
            let g = if cc_civil::js::truthy(o.get("grid")) {
                grid_stringified(&read_grid(o.get("grid").unwrap_or(&Value::Null))?)
            } else {
                String::new()
            };
            set(&mut r, "grid", Value::String(g));
        }
        let grid_empty = r.iter().find(|(k, _)| k == "grid").is_none_or(|(_, v)| v.as_str() == Some(""));
        if kind == "wmts" && (grid_empty || s("set", 200).is_empty()) {
            return Err(bad("grid"));
        }
        if crs_proj4.is_empty() && kind != "xyz" {
            return Err(bad("crs"));
        }
    }
    Ok(Value::Object(r.into_iter().collect()))
}

/// `readMaps(list, notes, images)`.
pub fn read_maps(list: &Value, notes: &mut Vec<String>, images: Option<&[String]>) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut bad = 0usize;
    for m in cc_civil::js::arr(Some(list)).iter().take(100) {
        match read_map(m, images) {
            Ok(mut r) => {
                let id = r.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                if seen.contains(&id) {
                    let nid = format!("m{}", out.len().saturating_add(1000));
                    if let Some(o) = r.as_object_mut() {
                        o.insert("id".into(), Value::String(nid));
                    }
                }
                seen.push(r.get("id").and_then(Value::as_str).unwrap_or("").to_string());
                out.push(r);
            }
            Err(_) => bad = bad.saturating_add(1),
        }
    }
    if bad > 0 {
        notes.push(tr!("basemap.maps.bad", n = bad));
    }
    out
}

/// `checkImageData(v)`.
pub fn check_image_data(v: &Value) -> bool {
    let Some(s) = v.as_str() else { return false };
    let max_len = (25.0 * 1024.0 * 1024.0 * 4.0 / 3.0f64).ceil() + 40.0;
    if s.len() as f64 > max_len {
        return false;
    }
    let body = s.strip_prefix("data:image/png;base64,").or_else(|| s.strip_prefix("data:image/jpeg;base64,"));
    let Some(b) = body else { return false };
    let core = b.trim_end_matches('=');
    !core.is_empty() && core.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'+' || c == b'/')
}

/// The `stringify` helper kept for the ops.
pub fn to_text(v: &Value) -> String {
    stringify(v)
}
