//! CivilCraft's map layers under the plan (gis-map-ui.js, basemap.js; GIS › Map): a georeferenced picture (PNG or JPEG
//! with its world file, kept in the project), map tiles (XYZ templates), WMS, WMTS and ArcGIS map services. Pictures in
//! another system than the project's are drawn as affine pieces within a quarter pixel of the exact reprojection
//! (basemap.rs `plan_warp`). The PDF and the DWG export carry the visible layers as one picture (0.4 V8).
//!
//! Network: nothing is ever asked while Settings › CivilCraft › "Map layers from the internet" is Off. On, the host's
//! `Services::fetch` asks for the tiles / pictures / capabilities 0.5 asks for (no credentials, no referrer), at most 64
//! tiles a view; a server that refuses four times with no picture is not asked again until the layer is shown again.
//! A server named by a project file is never asked until the person allows it in the Map dialog (a document never
//! chooses what this app fetches). `civil.maps.*` drives every part headless.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cc_civil::i18n::t;
use cc_civil::js::{arr, numf, string_of};
use cc_civil::tr;
use cc_io_civil::basemap::{self as bm, Grid};
use egui::{Color32, Pos2, Rect, RichText};
use serde_json::{Map, Value, json};

use crate::civilview::{CivilView, t0};
use crate::theme::Tokens;

const MAX_TILES: f64 = 64.0;
const MAX_CACHE: usize = 200;
const MAX_ACTIVE: usize = 6;

/// A picture to draw: opacity, picture key, its warp cells, its pixel size.
type Piece = (f64, String, Vec<bm::Cell>, u32, u32);

/// A fetched or decoded picture.
pub struct Pic {
    pub w: u32,
    pub h: u32,
    pub px: Arc<Vec<u8>>,
    tex: Option<egui::TextureHandle>,
}

enum Slot {
    Queued,
    Loading,
    Ok(Pic),
    Err,
}

/// One-picture layers (WMS, ArcGIS export): the picture asked for once the view settles.
#[derive(Default)]
struct One {
    view_key: String,
    since: f64,
    want: Option<(String, [f64; 4])>,
    shown: Option<(String, [f64; 4])>,
}

/// What the Map dialog holds while it is open.
#[derive(Clone, Default)]
pub struct MapDlg {
    pub kind: String,
    pub url: String,
    pub name: String,
    pub att: String,
    pub status: String,
    /// a server's capabilities read: (kind, the parsed answer, the base URL)
    pub found: Option<(String, Value, String)>,
    pub layer: usize,
    pub set: String,
    /// the capabilities URL being read
    pub reading: Option<String>,
}

/// The runtime of the map layers (never saved).
#[derive(Default)]
pub struct Maps {
    slots: HashMap<String, (Slot, String, f64)>,
    images: HashMap<String, Option<Pic>>,
    notes: HashMap<String, (String, bool)>,
    fails: HashMap<String, (u32, u32, bool)>,
    one: HashMap<String, One>,
    /// layers whose servers this session may ask (added here, or allowed)
    pub trusted: HashSet<String>,
    /// URLs for the host to fetch (drained by civilview each frame)
    pub to_fetch: Vec<String>,
    active: usize,
    wanted: HashSet<String>,
    /// picture + world file (+ .prj) gathering
    pub files: Vec<(String, Vec<u8>)>,
    pub dlg: Option<MapDlg>,
    /// the attribution shown under the plan
    pub attribution: String,
    clock: f64,
}

/// A record for state.gis.maps (gis-map-ui.js `record`).
#[allow(clippy::too_many_arguments)]
fn record(
    id: &str,
    kind: &str,
    name: &str,
    url: &str,
    layer: &str,
    style: &str,
    format: &str,
    version: &str,
    crs: Option<&Value>,
    attribution: &str,
    image: &str,
    wh: (f64, f64),
    gt: Option<[f64; 6]>,
    grid: &str,
    set: &str,
    template: &str,
) -> Value {
    let nm = bm::clean(name, 120);
    let gt = gt.unwrap_or([0.0, 1.0, 0.0, 0.0, 0.0, -1.0]);
    let c = |k: &str| crs.and_then(|c| c.get(k)).cloned();
    json!({
        "id": id, "name": if nm.is_empty() { t("mapl.default_name", &[]) } else { nm }, "kind": kind, "url": url, "layer": layer,
        "style": style, "format": format, "version": version,
        "crsCode": c("code").unwrap_or(Value::Null), "crsName": c("name").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default(),
        "crsProj4": c("proj4").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default(),
        "crsUnit": c("unit").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_else(|| "m".into()),
        "opacity": 1, "visible": true, "attribution": bm::clean(attribution, 300), "image": image, "width": wh.0, "height": wh.1,
        "a0": gt[0], "a1": gt[1], "a2": gt[2], "a3": gt[3], "a4": gt[4], "a5": gt[5], "grid": grid, "set": set, "template": template,
        "subdomains": "",
    })
}

fn is_internet(m: &Value) -> bool {
    matches!(m.get("kind").and_then(Value::as_str), Some("xyz" | "wms" | "wmts" | "arcgis"))
}

fn b64_decode(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut n) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        acc = (acc << 6) | u32::from(v);
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push(((acc >> n) & 0xFF) as u8);
        }
    }
    out
}

fn b64_encode(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(b.len().div_ceil(3) * 4);
    for ch in b.chunks(3) {
        let v = (u32::from(ch.first().copied().unwrap_or(0)) << 16)
            | (u32::from(ch.get(1).copied().unwrap_or(0)) << 8)
            | u32::from(ch.get(2).copied().unwrap_or(0));
        for i in 0..4 {
            if i <= ch.len() {
                out.push(char::from(A.get(((v >> (18 - 6 * i)) & 63) as usize).copied().unwrap_or(b'A')));
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn decode_pic(bytes: &[u8]) -> Option<Pic> {
    let r = cc_io_cad::raster::decode(bytes, 4096).ok()?;
    Some(Pic { w: r.w, h: r.h, px: Arc::new(r.px), tex: None })
}

/// Project ↔ a layer's system (gis-map-ui.js `conv`): None when it needs a project system and there is none.
pub struct Conv {
    frame: Option<cc_io_civil::crs::Frame>,
    src: Option<Value>,
    same: bool,
}
impl Conv {
    pub fn to_src(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        let Some(f) = &self.frame else { return Some((x, y)) };
        let Some(src) = &self.src else { return Some((x, y)) };
        if self.same {
            let g = f.to_grid(x, y);
            return Some((g[0], g[1]));
        }
        f.to(src, x, y).ok().map(|v| {
            let [a, b] = cc_io_civil::crs::nn(v);
            (a, b)
        })
    }
    pub fn from_src(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        let Some(f) = &self.frame else { return Some((x, y)) };
        let Some(src) = &self.src else { return Some((x, y)) };
        if self.same {
            let g = f.from_grid(x, y);
            return Some((g[0], g[1]));
        }
        f.from(src, x, y).ok().map(|v| (v[0], v[1]))
    }
}

/// A layer's own system (gis-map-ui.js `layerCrs`): None = the project's own coordinates.
fn layer_crs(m: &Value) -> Option<Value> {
    let p4 = m.get("crsProj4").and_then(Value::as_str).unwrap_or("");
    if !p4.is_empty() {
        return Some(
            json!({ "code": m.get("crsCode").cloned().unwrap_or(Value::Null), "name": m.get("crsName").cloned().unwrap_or(json!("")),
            "proj4": p4, "unit": m.get("crsUnit").cloned().unwrap_or(json!("m")) }),
        );
    }
    if m.get("kind").and_then(Value::as_str) == Some("xyz") {
        return crate::civilgis::crs_of_code("3857").ok();
    }
    None
}

impl CivilView {
    fn maps_list(&self) -> Vec<Value> {
        arr(self.project.state.get("gis").and_then(|g| g.get("maps"))).to_vec()
    }

    /// The project's usable system (crsProblem-free) or None.
    fn pcrs(&self) -> Option<Value> {
        let c = self.project.state.get("crs").filter(|c| cc_civil::js::truthy(Some(c)))?;
        crate::civilgis::ensure_catalog();
        cc_io_civil::crs::crs_problem(c, self.project.units()).is_none().then(|| c.clone())
    }

    /// `conv(m)`.
    pub fn map_conv(&self, m: &Value) -> Option<Conv> {
        let Some(src) = layer_crs(m) else { return Some(Conv { frame: None, src: None, same: true }) };
        let pc = self.pcrs()?;
        let f = cc_io_civil::crs::frame(&pc).ok()??;
        let same = src.get("proj4") == pc.get("proj4");
        Some(Conv { frame: Some(f), src: Some(src), same })
    }

    fn internet_on(&self) -> bool {
        self.pt_settings.internet
    }

    fn map_note(&mut self, id: &str, text: &str) {
        let e = self.maps.notes.entry(id.to_string()).or_insert((String::new(), false));
        if e.0 == text {
            return;
        }
        e.0 = text.to_string();
        if !text.is_empty() && !e.1 {
            e.1 = true;
            let name = self.maps_list().iter().find(|m| m.get("id").and_then(Value::as_str) == Some(id)).map(|m| string_of(m.get("name")));
            self.message = match name {
                Some(n) => format!("{n}: {text}"),
                None => text.to_string(),
            };
        }
    }

    /// `want(url, layerId)`: a picture the plan needs; asked for when the internet is on and the server is trusted.
    fn want(&mut self, url: &str, id: &str) -> bool {
        self.maps.wanted.insert(url.to_string());
        if let Some(s) = self.maps.slots.get_mut(url) {
            s.2 = self.maps.clock;
            return matches!(s.0, Slot::Ok(_));
        }
        let blocked = self.maps.fails.get(id).is_some_and(|f| f.2);
        if !self.internet_on() || blocked || !self.maps.trusted.contains(id) {
            return false;
        }
        self.maps.slots.insert(url.to_string(), (Slot::Queued, id.to_string(), self.maps.clock));
        false
    }

    /// The host's answers: decoded, or a note on the layer (gis-map-ui.js `load`).
    pub fn maps_answer(&mut self, url: &str, result: Result<(String, Vec<u8>), String>) {
        if let Some(d) = self.maps.dlg.as_mut()
            && d.reading.as_deref() == Some(url)
        {
            d.reading = None;
            match result {
                Ok((_, bytes)) => {
                    let text = String::from_utf8_lossy(&bytes).into_owned();
                    self.maps_read_answer(&text);
                }
                Err(e) => d.status = net_words(&e, true),
            }
            return;
        }
        let Some((slot, id, _)) = self.maps.slots.get_mut(url) else { return };
        self.maps.active = self.maps.active.saturating_sub(1);
        let id = id.clone();
        let out = match result {
            Ok((ty, bytes)) if ty.to_ascii_lowercase().starts_with("image/") || ty.is_empty() => {
                decode_pic(&bytes).ok_or_else(|| t("mapl.err.decode", &[]))
            }
            Ok((ty, bytes)) => {
                let why = if bytes.len() < 20_000 && (ty.contains("xml") || ty.contains("text") || ty.contains("json")) {
                    let s = String::from_utf8_lossy(&bytes);
                    let mut plain = String::new();
                    let mut inside = false;
                    for ch in s.chars() {
                        match ch {
                            '<' => inside = true,
                            '>' => {
                                inside = false;
                                plain.push(' ');
                            }
                            c if !inside => plain.push(c),
                            _ => {}
                        }
                    }
                    bm::clean(&plain, 160)
                } else {
                    String::new()
                };
                let tt = ty.split(';').next().unwrap_or("").to_string();
                Err(if why.is_empty() { tr!("mapl.err.sent", type = tt) } else { tr!("mapl.err.sent_why", type = tt, why = why) })
            }
            Err(e) => Err(net_words(&e, false)),
        };
        match out {
            Ok(p) => {
                *slot = Slot::Ok(p);
                let f = self.maps.fails.entry(id).or_insert((0, 0, false));
                f.1 = f.1.saturating_add(1);
            }
            Err(msg) => {
                *slot = Slot::Err;
                let f = self.maps.fails.entry(id.clone()).or_insert((0, 0, false));
                f.0 = f.0.saturating_add(1);
                if f.1 == 0 && f.0 >= 4 && !msg.contains("404") {
                    f.2 = true;
                    self.map_note(&id, &format!("{msg} {}", t("mapl.err.stopped", &[])));
                } else {
                    self.map_note(&id, &msg);
                }
            }
        }
        if self.maps.slots.len() > MAX_CACHE {
            let wanted = self.maps.wanted.clone();
            let mut old: Vec<(String, f64)> = self
                .maps
                .slots
                .iter()
                .filter(|(u, s)| !matches!(s.0, Slot::Loading) && !wanted.contains(*u))
                .map(|(u, s)| (u.clone(), s.2))
                .collect();
            old.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            let n = self.maps.slots.len() - MAX_CACHE;
            for (u, _) in old.into_iter().take(n) {
                self.maps.slots.remove(&u);
            }
        }
    }

    /// After a paint: queued pictures nobody wants go; up to six ask the host at once (gis-map-ui.js pump).
    fn maps_pump(&mut self) {
        let wanted = std::mem::take(&mut self.maps.wanted);
        self.maps.slots.retain(|u, s| !matches!(s.0, Slot::Queued) || wanted.contains(u));
        let mut queued: Vec<String> = self.maps.slots.iter().filter(|(_, s)| matches!(s.0, Slot::Queued)).map(|(u, _)| u.clone()).collect();
        queued.sort();
        for u in queued {
            if self.maps.active >= MAX_ACTIVE || !self.internet_on() {
                break;
            }
            if let Some(s) = self.maps.slots.get_mut(&u) {
                s.0 = Slot::Loading;
                self.maps.active += 1;
                self.maps.to_fetch.push(u);
            }
        }
        self.maps.wanted = wanted;
    }

    /// The picture of an `image` layer (decoded once from the project's data URL).
    fn image_pic(&mut self, image: &str) -> bool {
        if !self.maps.images.contains_key(image) {
            let data = self.project.gis_images.iter().find(|(k, _)| k == image).map(|(_, v)| v.clone());
            let pic = data.and_then(|d| d.split_once(";base64,").map(|(_, b)| b64_decode(b))).and_then(|b| decode_pic(&b));
            self.maps.images.insert(image.to_string(), pic);
        }
        self.maps.images.get(image).is_some_and(Option::is_some)
    }

    /// Every visible layer's pictures as (layer opacity, picture key, pixel → project mapping) for the plan / drape.
    fn map_pieces(&mut self, vb: [f64; 4], view_w: f64, view_h: f64, now: f64) -> (Vec<Piece>, Vec<String>) {
        let mut out = Vec::new();
        let mut shown = Vec::new();
        let zoom = (view_w / (vb[2] - vb[0]).max(1e-12)).max(1e-12);
        let tol = 0.25 / zoom;
        let bucket = ((tol.log2() * 2.0).round()) / 2.0;
        for m in self.maps_list() {
            if m.get("visible") != Some(&Value::Bool(true)) {
                continue;
            }
            if is_internet(&m) && !self.internet_on() {
                continue;
            }
            let Some(cv) = self.map_conv(&m) else { continue };
            let id = string_of(m.get("id"));
            let op = numf(m.get("opacity")).clamp(0.0, 1.0);
            let depth = if cv.same { 0 } else { 7 };
            let kind = m.get("kind").and_then(Value::as_str).unwrap_or("").to_string();
            match kind.as_str() {
                "image" => {
                    let image = string_of(m.get("image"));
                    if !self.image_pic(&image) {
                        self.map_note(&id, &t("mapl.pic.missing", &[]));
                        continue;
                    }
                    let Some(Some(p)) = self.maps.images.get(&image) else { continue };
                    let (pw, ph) = (p.w, p.h);
                    let kx = numf(m.get("width")) / f64::from(pw);
                    let ky = numf(m.get("height")) / f64::from(ph);
                    let g = |k: &str| numf(m.get(k));
                    let gt = [g("a0"), g("a1") * kx, g("a2") * ky, g("a3"), g("a4") * kx, g("a5") * ky];
                    let cells = bm::plan_warp(
                        &|u, v| {
                            let (x, y) = bm::pixel_to_system(&gt, u, v);
                            cv.from_src(x, y)
                        },
                        f64::from(pw),
                        f64::from(ph),
                        2f64.powf(bucket),
                        depth,
                        0,
                    );
                    out.push((op, format!("img:{image}"), cells, pw, ph));
                }
                "xyz" | "wmts" | "arcgis" if kind != "arcgis" || m.get("grid").and_then(Value::as_str).is_some_and(|g| !g.is_empty()) => {
                    let grid: Grid = if kind == "xyz" {
                        bm::xyz_grid()
                    } else {
                        match bm::read_grid(m.get("grid").unwrap_or(&Value::Null)) {
                            Ok(g) => g,
                            Err(_) => continue,
                        }
                    };
                    let Some(bx) = bm::view_source_box(vb, &|x, y| cv.to_src(x, y), 8) else { continue };
                    let res = ((bx[2] - bx[0]) / view_w).max((bx[3] - bx[1]) / view_h);
                    let (matrix, tiles) = bm::grid_tiles_any(&grid, bx, res, MAX_TILES);
                    let Some(mx) = matrix else { continue };
                    for tl in tiles {
                        let (col, row) = (cc_civil::js::num_to_string(tl.col), cc_civil::js::num_to_string(tl.row));
                        let url = match kind.as_str() {
                            "xyz" => {
                                let z: f64 = mx.id.parse().unwrap_or(0.0);
                                Ok(bm::xyz_url(
                                    &string_of(m.get("url")),
                                    z,
                                    tl.col,
                                    tl.row,
                                    m.get("subdomains").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("abc"),
                                ))
                            }
                            "wmts" => bm::wmts_tile_url(
                                &string_of(m.get("template")),
                                &string_of(m.get("url")),
                                &string_of(m.get("layer")),
                                m.get("style").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("default"),
                                &string_of(m.get("format")),
                                &string_of(m.get("set")),
                                &mx.id,
                                &row,
                                &col,
                            )
                            .map_err(|e| e.0),
                            _ => bm::arcgis_tile_url(&string_of(m.get("url")), &mx.id, &row, &col).map_err(|e| e.0),
                        };
                        let Ok(url) = url else { continue };
                        if !self.want(&url, &id) {
                            continue;
                        }
                        let Some((Slot::Ok(p), _, _)) = self.maps.slots.get(&url) else { continue };
                        let (pw, ph) = (p.w, p.h);
                        let [x0, _, x1, y1] = tl.bounds;
                        let rx = (x1 - x0) / f64::from(pw);
                        let ry = (y1 - tl.bounds[1]) / f64::from(ph);
                        let cells = bm::plan_warp(
                            &|u, v| cv.from_src(x0 + u * rx, y1 - v * ry),
                            f64::from(pw),
                            f64::from(ph),
                            2f64.powf(bucket),
                            depth,
                            0,
                        );
                        out.push((op, format!("url:{url}"), cells, pw, ph));
                    }
                    if let Some(a) = m.get("attribution").and_then(Value::as_str).filter(|a| !a.is_empty()) {
                        shown.push(a.to_string());
                    }
                }
                _ => {
                    // one picture per view (WMS GetMap, ArcGIS export), asked for once the view has settled (350 ms)
                    let Some(bx) = bm::view_source_box(vb, &|x, y| cv.to_src(x, y), 8) else { continue };
                    let w = view_w.round().clamp(16.0, 2048.0);
                    let h = view_h.round().clamp(16.0, 2048.0);
                    let code = numf(m.get("crsCode"));
                    let url = if kind == "wms" {
                        bm::wms_get_map_url(
                            &string_of(m.get("url")),
                            &string_of(m.get("version")),
                            &string_of(m.get("layer")),
                            &string_of(m.get("style")),
                            m.get("format").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("image/png"),
                            code,
                            bx,
                            w,
                            h,
                            true,
                        )
                    } else {
                        let k = if m.get("layer").and_then(Value::as_str) == Some("ImageServer") { "ImageServer" } else { "MapServer" };
                        bm::arcgis_export_url(&string_of(m.get("url")), k, bx, code, w, h)
                    };
                    let Ok(url) = url else { continue };
                    let one = self.maps.one.entry(id.clone()).or_default();
                    if one.view_key != url {
                        one.view_key = url.clone();
                        one.since = now;
                    } else if now - one.since > 0.35 && one.want.as_ref().is_none_or(|w| w.0 != url) {
                        one.want = Some((url.clone(), bx));
                    }
                    let want = one.want.clone();
                    if let Some((wu, wb)) = want
                        && self.want(&wu, &id)
                    {
                        self.maps.one.entry(id.clone()).or_default().shown = Some((wu, wb));
                    }
                    let shown_pic = self.maps.one.get(&id).and_then(|o| o.shown.clone());
                    if let Some((su, sb)) = shown_pic {
                        self.maps.wanted.insert(su.clone());
                        if let Some((Slot::Ok(p), _, _)) = self.maps.slots.get(&su) {
                            let (pw, ph) = (p.w, p.h);
                            let rx = (sb[2] - sb[0]) / f64::from(pw);
                            let ry = (sb[3] - sb[1]) / f64::from(ph);
                            let cells = bm::plan_warp(
                                &|u, v| cv.from_src(sb[0] + u * rx, sb[3] - v * ry),
                                f64::from(pw),
                                f64::from(ph),
                                2f64.powf(bucket),
                                depth,
                                0,
                            );
                            out.push((op, format!("url:{su}"), cells, pw, ph));
                        }
                    }
                    if let Some(a) = m.get("attribution").and_then(Value::as_str).filter(|a| !a.is_empty()) {
                        shown.push(a.to_string());
                    }
                }
            }
        }
        (out, shown)
    }

    /// The map layers under the plan (gis-map-ui.js `paint`), in CSS pixels of `rect`.
    pub fn maps_paint(&mut self, painter: &egui::Painter, rect: Rect, ppp: f64) {
        if self.maps_list().is_empty() {
            self.maps.attribution.clear();
            return;
        }
        let now = painter.ctx().input(|i| i.time);
        self.maps.clock = now;
        let (x0, y0, x1, y1) = self.view_bounds();
        let (vw, vh) = (f64::from(rect.width()), f64::from(rect.height()));
        let (pieces, shown) = self.map_pieces([x0, y0, x1, y1], vw, vh, now);
        let view = self.view;
        let s = |x: f64, y: f64| {
            let (sx, sy) = view.to_screen(x, y);
            Pos2::new(rect.min.x + (sx / ppp) as f32, rect.min.y + (sy / ppp) as f32)
        };
        for (op, key, cells, pw, ph) in pieces {
            let tex = self.pic_texture(painter.ctx(), &key);
            let Some(tex) = tex else { continue };
            let col = Color32::from_white_alpha((op * 255.0).round() as u8);
            let mut mesh = egui::Mesh::with_texture(tex);
            for c in &cells {
                let base = mesh.vertices.len() as u32;
                for k in 0..4 {
                    let (Some(src), Some(dst)) = (c.src.get(k), c.dst.get(k)) else { continue };
                    mesh.vertices.push(egui::epaint::Vertex {
                        pos: s(dst[0], dst[1]),
                        uv: egui::pos2((src[0] / f64::from(pw)) as f32, (src[1] / f64::from(ph)) as f32),
                        color: col,
                    });
                }
                mesh.add_triangle(base, base + 1, base + 2);
                mesh.add_triangle(base, base + 2, base + 3);
            }
            painter.add(egui::Shape::mesh(mesh));
        }
        let mut uniq: Vec<String> = Vec::new();
        for a in shown {
            if !uniq.contains(&a) {
                uniq.push(a);
            }
        }
        self.maps.attribution = uniq.join("; ");
        self.maps_pump();
        if self.maps.active > 0
            || self.maps.slots.values().any(|s| matches!(s.0, Slot::Queued))
            || self.maps.one.values().any(|o| o.want.is_none())
        {
            painter.ctx().request_repaint_after(std::time::Duration::from_millis(120));
        }
    }

    fn pic_texture(&mut self, ctx: &egui::Context, key: &str) -> Option<egui::TextureId> {
        let pic = if let Some(img) = key.strip_prefix("img:") {
            self.maps.images.get_mut(img).and_then(Option::as_mut)
        } else {
            let url = key.strip_prefix("url:").unwrap_or(key);
            match self.maps.slots.get_mut(url) {
                Some((Slot::Ok(p), _, _)) => Some(p),
                _ => None,
            }
        }?;
        if pic.tex.is_none() {
            let img = egui::ColorImage::from_rgba_unmultiplied([pic.w as usize, pic.h as usize], &pic.px);
            pic.tex = Some(ctx.load_texture(format!("cv-map-{key}"), img, egui::TextureOptions::LINEAR));
        }
        pic.tex.as_ref().map(egui::TextureHandle::id)
    }

    // ------------------------------------------------------------------------------------------- the drape (exports)

    /// `drape.render(box, maxPx)`: the visible layers over `box` (project coordinates) as RGBA pixels on white, at most
    /// `max_px` on the longer side → (w, h, pixels, attribution, complete). None when no layer would draw.
    pub fn maps_drape(&mut self, b: [f64; 4], max_px: f64, now: f64) -> Option<(u32, u32, Vec<u8>, String, bool)> {
        let any = self.maps_list().iter().any(|m| {
            m.get("visible") == Some(&Value::Bool(true)) && !(is_internet(m) && !self.internet_on()) && self.map_conv(m).is_some()
        });
        if !any {
            return None;
        }
        let (bw, bh) = ((b[2] - b[0]).max(1e-9), (b[3] - b[1]).max(1e-9));
        let k = max_px / bw.max(bh);
        let w = ((bw * k).round()).max(16.0);
        let h = ((bh * k).round()).max(16.0);
        let (wu, hu) = (w as usize, h as usize);
        let mut px = vec![255u8; wu.saturating_mul(hu).saturating_mul(4)];
        let (pieces, shown) = self.map_pieces(b, w, h, now + 1.0);
        let mut complete = true;
        for (op, key, cells, pw, ph) in &pieces {
            let pic = if let Some(img) = key.strip_prefix("img:") {
                self.maps.images.get(img).and_then(Option::as_ref)
            } else {
                match self.maps.slots.get(key.strip_prefix("url:").unwrap_or(key)) {
                    Some((Slot::Ok(p), _, _)) => Some(p),
                    _ => None,
                }
            };
            let Some(pic) = pic else {
                complete = false;
                continue;
            };
            for c in cells {
                // the cell's affine (pixel → project) inverted: each target pixel inside its box samples the picture
                let m = c.m;
                let det = m[0] * m[3] - m[2] * m[1];
                if det.abs() < 1e-300 {
                    continue;
                }
                let xs = c.dst.iter().map(|p| p[0]);
                let ys = c.dst.iter().map(|p| p[1]);
                let (mnx, mxx) = xs.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, z), x| (a.min(x), z.max(x)));
                let (mny, mxy) = ys.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, z), y| (a.min(y), z.max(y)));
                let i0 = (((mnx - b[0]) * k).floor().max(0.0)) as usize;
                let i1 = (((mxx - b[0]) * k).ceil().min(w)) as usize;
                let j0 = (((b[3] - mxy) * k).floor().max(0.0)) as usize;
                let j1 = (((b[3] - mny) * k).ceil().min(h)) as usize;
                let (u0, v0, u1, v1) = (c.src[0][0], c.src[0][1], c.src[2][0], c.src[2][1]);
                for j in j0..j1 {
                    let y = b[3] - (j as f64 + 0.5) / k;
                    for i in i0..i1 {
                        let x = b[0] + (i as f64 + 0.5) / k;
                        let dx = x - m[4];
                        let dy = y - m[5];
                        let u = (m[3] * dx - m[2] * dy) / det;
                        let v = (-m[1] * dx + m[0] * dy) / det;
                        if u < u0 || u >= u1 || v < v0 || v >= v1 {
                            continue;
                        }
                        let (su, sv) = ((u.floor() as u32).min(pic.w - 1), (v.floor() as u32).min(pic.h - 1));
                        let si = ((sv as usize) * (pic.w as usize) + su as usize) * 4;
                        let ti = (j * wu + i) * 4;
                        let (Some(&[sr, sg, sb, sa]), Some(tp)) = (pic.px.get(si..si + 4), px.get_mut(ti..ti + 4)) else { continue };
                        let a = f64::from(sa) / 255.0 * op;
                        for (t, sv) in tp.iter_mut().zip([sr, sg, sb]) {
                            *t = (f64::from(*t) * (1.0 - a) + f64::from(sv) * a).round() as u8;
                        }
                    }
                }
                let _ = (pw, ph);
            }
        }
        if self.maps.slots.values().any(|s| matches!(s.0, Slot::Queued | Slot::Loading)) {
            complete = false;
        }
        self.maps_pump();
        let mut uniq: Vec<String> = Vec::new();
        for a in shown {
            if !uniq.contains(&a) {
                uniq.push(a);
            }
        }
        Some((w as u32, h as u32, px, uniq.join("; "), complete))
    }

    pub fn visible_maps(&self) -> bool {
        self.maps_list().iter().any(|m| m.get("visible") == Some(&Value::Bool(true)))
    }

    // ------------------------------------------------------------------------------------------- adding, editing

    fn new_map_id(&self) -> String {
        let used: Vec<String> = self.maps_list().iter().map(|m| string_of(m.get("id"))).collect();
        let mut i = 1;
        while used.contains(&format!("m{i}")) {
            i += 1;
        }
        format!("m{i}")
    }

    fn add_map(&mut self, rec: Value, label: &str) {
        let id = string_of(rec.get("id"));
        self.project.change(label, &mut |s: &mut Value| {
            let Some(m) = s.as_object_mut() else { return };
            let g = m.entry("gis").or_insert_with(|| json!({ "layers": [], "maps": [] }));
            if let Some(gm) = g.as_object_mut() {
                let ms = gm.entry("maps").or_insert_with(|| Value::Array(Vec::new()));
                if let Some(l) = ms.as_array_mut() {
                    l.push(rec.clone());
                }
            }
        });
        self.maps.trusted.insert(id);
    }

    /// A picture file or a world file (or a .prj) opened while CivilCraft is open (gis-map-ui.js addImageFiles).
    pub fn maps_file(&mut self, name: &str, bytes: Vec<u8>) {
        self.maps.files.retain(|(n, _)| n != name);
        self.maps.files.push((name.to_string(), bytes));
        let lower = |n: &str| n.to_ascii_lowercase();
        let img = self.maps.files.iter().find(|(n, _)| {
            let l = lower(n);
            l.ends_with(".png") || l.ends_with(".jpg") || l.ends_with(".jpeg")
        });
        let Some((iname, ibytes)) = img.cloned() else { return };
        let names = bm::world_file_for(&iname.to_ascii_lowercase());
        let wf = self.maps.files.iter().find(|(n, _)| names.contains(&lower(n))).or_else(|| {
            self.maps.files.iter().find(|(n, _)| {
                let l = lower(n);
                [".pgw", ".pngw", ".jgw", ".jpgw", ".jpegw", ".tfw", ".tifw", ".wld"].iter().any(|e| l.ends_with(e))
            })
        });
        let Some((_, wbytes)) = wf.cloned() else {
            // the world file may come next (a drop of both arrives one by one)
            self.message = tr!(
                "mapl.world.none",
                name = iname.as_str(),
                a = names.first().cloned().unwrap_or_default(),
                b = names.get(1).cloned().unwrap_or_default()
            );
            return;
        };
        let prj = self.maps.files.iter().find(|(n, _)| lower(n).ends_with(".prj")).map(|(_, b)| String::from_utf8_lossy(b).into_owned());
        self.maps.files.clear();
        if ibytes.len() > 25 * 1024 * 1024 {
            self.message = tr!("mapl.pic.big", name = iname.as_str(), mb = cc_civil::js::to_fixed(ibytes.len() as f64 / 1_048_576.0, 1));
            return;
        }
        let gt = match bm::parse_world_file(&String::from_utf8_lossy(&wbytes)) {
            Ok(g) => g,
            Err(e) => {
                self.message = e.0;
                return;
            }
        };
        let Some(pic) = decode_pic(&ibytes) else {
            self.message = tr!("mapl.pic.not_picture", name = iname.as_str());
            return;
        };
        let (w, h) = (pic.w, pic.h);
        // the picture's system: its .prj, else the project's (0.5 asks; here a .prj or the project's)
        crate::civilgis::ensure_catalog();
        let crs = match prj.as_deref().map(cc_io_civil::crs::crs_from_prj) {
            Some(Ok(r)) => Some(r.crs),
            Some(Err(e)) => {
                self.message = tr!("mapl.prj.bad", name = "prj", error = e);
                return;
            }
            None => self.pcrs().map(|mut c| {
                if let Some(o) = c.as_object_mut() {
                    o.insert("local".into(), Value::Null);
                }
                c
            }),
        };
        let mime = if ibytes.starts_with(&[0x89, b'P', b'N', b'G']) { "image/png" } else { "image/jpeg" };
        let data = format!("data:{mime};base64,{}", b64_encode(&ibytes));
        let used: Vec<String> = self
            .project
            .gis_images
            .iter()
            .map(|(k, _)| k.clone())
            .chain(self.maps_list().iter().map(|m| string_of(m.get("image"))))
            .collect();
        let mut n = 1;
        while used.contains(&format!("img{n}")) {
            n += 1;
        }
        let img_id = format!("img{n}");
        self.project.gis_images.push((img_id.clone(), data));
        let stem = iname.rsplit_once('.').map_or(iname.as_str(), |(a, _)| a).to_string();
        let id = self.new_map_id();
        let rec = record(
            &id,
            "image",
            &stem,
            "",
            "",
            "",
            "image/png",
            "1.3.0",
            crs.as_ref(),
            "",
            &img_id,
            (f64::from(w), f64::from(h)),
            Some(gt),
            "",
            "",
            "",
        );
        self.add_map(rec, &tr!("mapl.undo.picture", name = iname.as_str()));
        self.message = match &crs {
            Some(c) => tr!("mapl.added.pic_crs", name = iname.as_str(), w = f64::from(w), h = f64::from(h), crs = string_of(c.get("name"))),
            None => tr!("mapl.added.pic_proj", name = iname.as_str(), w = f64::from(w), h = f64::from(h)),
        };
        self.fit();
    }

    /// `civil.maps=…`: visible=<id>:on|off · opacity=<id>:<0..1> · move=<id>:-1|1 · remove=<id> · zoom=<id> · allow=<id>
    /// · xyz=<template>|<name>|<attribution> · open | close · kind=<kind> · url=<text> · read · add=<layer index>[:<set>]
    pub fn maps_cmd(&mut self, spec: &str) -> Result<(), String> {
        let (k, v) = spec.split_once('=').map_or((spec, ""), |(a, b)| (a, b));
        let (id, val) = v.split_once(':').map_or((v, ""), |(a, b)| (a, b));
        let edit = |cv: &mut CivilView, label: &str, f: &dyn Fn(&mut Vec<Value>)| {
            cv.project.change(label, &mut |s: &mut Value| {
                if let Some(l) = s.get_mut("gis").and_then(|g| g.get_mut("maps")).and_then(Value::as_array_mut) {
                    f(l);
                }
            });
        };
        let find = |cv: &CivilView| cv.maps_list().into_iter().find(|m| m.get("id").and_then(Value::as_str) == Some(id));
        match k {
            "open" => {
                self.maps.dlg = Some(MapDlg { kind: "image".into(), ..MapDlg::default() });
            }
            "close" => self.maps.dlg = None,
            "visible" => {
                find(self).ok_or_else(|| format!("civil.maps: no map layer {id}"))?;
                let on = val != "off";
                self.maps.fails.remove(id);
                if let Some(n) = self.maps.notes.get_mut(id) {
                    *n = (String::new(), false);
                }
                let idc = id.to_string();
                edit(self, &t(if on { "mapl.undo.show" } else { "mapl.undo.hide" }, &[]), &|l| {
                    if let Some(m) =
                        l.iter_mut().find(|m| m.get("id").and_then(Value::as_str) == Some(idc.as_str())).and_then(Value::as_object_mut)
                    {
                        m.insert("visible".into(), Value::Bool(on));
                    }
                });
            }
            "opacity" => {
                find(self).ok_or_else(|| format!("civil.maps: no map layer {id}"))?;
                let x = cc_civil::js::str_to_number(val);
                if !x.is_finite() {
                    return Err("civil.maps: opacity = 0 … 1".into());
                }
                let x = ((x.clamp(0.0, 1.0) * 100.0 / 5.0).round() * 5.0) / 100.0;
                let idc = id.to_string();
                edit(self, &t("mapl.undo.opacity", &[]), &|l| {
                    if let Some(m) =
                        l.iter_mut().find(|m| m.get("id").and_then(Value::as_str) == Some(idc.as_str())).and_then(Value::as_object_mut)
                    {
                        m.insert("opacity".into(), cc_civil::js::jnum(x));
                    }
                });
            }
            "move" => {
                let d: i64 = val.parse().map_err(|_| "civil.maps: move=<id>:-1|1")?;
                let idc = id.to_string();
                edit(self, &t("mapl.undo.reorder", &[]), &|l| {
                    if let Some(i) = l.iter().position(|m| m.get("id").and_then(Value::as_str) == Some(idc.as_str())) {
                        let j = i as i64 + d;
                        if j >= 0 && (j as usize) < l.len() {
                            l.swap(i, j as usize);
                        }
                    }
                });
            }
            "remove" => {
                let name = find(self).map(|m| string_of(m.get("name"))).unwrap_or_default();
                let idc = id.to_string();
                edit(self, &tr!("mapl.undo.remove", name = name.as_str()), &|l| {
                    l.retain(|m| m.get("id").and_then(Value::as_str) != Some(idc.as_str()));
                });
                self.maps.notes.remove(id);
                self.maps.one.remove(id);
            }
            "zoom" => {
                let m = find(self).ok_or_else(|| format!("civil.maps: no map layer {id}"))?;
                let Some(cv) = self.map_conv(&m) else { return Ok(()) };
                let g = |k: &str| numf(m.get(k));
                let gt = [g("a0"), g("a1"), g("a2"), g("a3"), g("a4"), g("a5")];
                let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
                for (u, vv) in [(0.0, 0.0), (g("width"), 0.0), (g("width"), g("height")), (0.0, g("height"))] {
                    let (sx, sy) = bm::pixel_to_system(&gt, u, vv);
                    if let Some((x, y)) = cv.from_src(sx, sy) {
                        x0 = x0.min(x);
                        y0 = y0.min(y);
                        x1 = x1.max(x);
                        y1 = y1.max(y);
                    }
                }
                self.fit_box(x0, y0, x1, y1);
            }
            "allow" => {
                find(self).ok_or_else(|| format!("civil.maps: no map layer {id}"))?;
                self.maps.trusted.insert(id.to_string());
                self.maps.fails.remove(id);
            }
            "xyz" => {
                let mut parts = v.splitn(3, '|');
                let tpl = parts.next().unwrap_or("");
                let name = parts.next().unwrap_or("").trim().to_string();
                let att = parts.next().unwrap_or("");
                self.maps_add_xyz(tpl, &name, att);
            }
            "kind" => {
                let d = self.maps.dlg.get_or_insert_with(MapDlg::default);
                if !bm::KINDS.contains(&v) {
                    return Err(format!("civil.maps: kind {} (not {v})", bm::KINDS.join(", ")));
                }
                d.kind = v.to_string();
                d.found = None;
                d.status.clear();
            }
            "url" => self.maps.dlg.get_or_insert_with(MapDlg::default).url = v.to_string(),
            "read" => self.maps_read(),
            "add" => self.maps_add_found(v),
            o => return Err(format!("civil.maps: unknown action {o}")),
        }
        Ok(())
    }

    /// The XYZ panel's Add (gis-map-ui.js addXyz).
    pub fn maps_add_xyz(&mut self, tpl: &str, name: &str, att: &str) {
        let r = bm::check_template(tpl);
        let msg = match r {
            Ok(tpl) => {
                let host = url_host(&tpl);
                let nm = if name.trim().is_empty() { host } else { name.trim().to_string() };
                let crs = crate::civilgis::crs_of_code("3857").ok();
                let id = self.new_map_id();
                let rec = record(&id, "xyz", &nm, &tpl, "", "", "image/png", "1.3.0", crs.as_ref(), att, "", (0.0, 0.0), None, "", "", "");
                self.add_map(rec, &tr!("mapl.undo.add", name = bm::clean(&nm, 120)));
                t(if self.internet_on() { "mapl.added" } else { "mapl.added_off" }, &[])
            }
            Err(e) => e.0,
        };
        // gis-map-ui.js: said in the dialog only
        self.maps.dlg.get_or_insert_with(MapDlg::default).status = msg;
    }

    /// "Get layers" / "Read the service": the capabilities asked of the server (the internet must be on).
    fn maps_read(&mut self) {
        let on = self.internet_on();
        let Some(d) = self.maps.dlg.as_mut() else { return };
        if !on {
            d.status = t("mapl.net_first", &[]);
            return;
        }
        let url = match d.kind.as_str() {
            "wms" => bm::capabilities_url(&d.url, "WMS"),
            "wmts" => bm::capabilities_url(&d.url, "WMTS"),
            "arcgis" => bm::arcgis_base(&d.url).map(|(b, _)| format!("{b}?f=json")),
            _ => return,
        };
        match url {
            Ok(u) => {
                d.status = t("mapl.asking", &[]);
                d.reading = Some(u.clone());
                self.maps.to_fetch.push(u);
            }
            Err(e) => d.status = e.0,
        }
    }

    fn maps_read_answer(&mut self, text: &str) {
        let Some(d) = self.maps.dlg.as_mut() else { return };
        let r = match d.kind.as_str() {
            "wms" => bm::parse_wms_capabilities(text).map(|c| {
                let base = c.get("getMap").and_then(Value::as_str).filter(|s| !s.is_empty()).map_or_else(|| d.url.clone(), str::to_string);
                ("wms".to_string(), c, base)
            }),
            "wmts" => bm::parse_wmts_capabilities(text).map(|c| ("wmts".to_string(), c, d.url.clone())),
            _ => bm::arcgis_base(&d.url).and_then(|(base, _)| {
                let j: Value = serde_json::from_str(text).map_err(|e| bm::MapError(e.to_string()))?;
                bm::parse_arcgis_service(&j, &base).map(|s| ("arcgis".to_string(), s, base))
            }),
        };
        match r {
            Ok(f) => {
                d.found = Some(f);
                d.layer = 0;
                d.status.clear();
            }
            Err(e) => d.status = e.0,
        }
    }

    /// "Add" after a server was read (gis-map-ui.js addFound): `layer[:set]`.
    fn maps_add_found(&mut self, v: &str) {
        let Some((kind, cap, base)) = self.maps.dlg.as_ref().and_then(|d| d.found.clone()) else { return };
        let (li, set) = v.split_once(':').map_or((v, ""), |(a, b)| (a, b));
        let li: usize = li.parse().unwrap_or(0);
        let pc = self.pcrs();
        let pcode = pc.as_ref().and_then(|c| c.get("code")).and_then(Value::as_f64);
        let with_local_null = |c: &Value| {
            let mut c = c.clone();
            if let Some(o) = c.as_object_mut() {
                o.insert("local".into(), Value::Null);
            }
            c
        };
        let status: Result<(), String> = (|| {
            match kind.as_str() {
                "wms" => {
                    let l = arr(cap.get("layers")).get(li).cloned().ok_or("no layer")?;
                    let codes: Vec<f64> = arr(l.get("codes")).iter().filter_map(Value::as_f64).collect();
                    let code = bm::wms_pick_code(&codes, pcode).ok_or_else(|| t("mapl.wms.no_system", &[]))?;
                    let crs = if Some(code) == pcode {
                        pc.as_ref().map(with_local_null)
                    } else {
                        crate::civilgis::crs_of_code(&cc_civil::js::num_to_string(code)).ok()
                    };
                    let fmts: Vec<String> = arr(cap.get("formats")).iter().map(|f| string_of(Some(f))).collect();
                    let fmt = fmts
                        .iter()
                        .find(|f| f.to_ascii_lowercase().contains("png"))
                        .or_else(|| fmts.iter().find(|f| f.to_ascii_lowercase().contains("jp")))
                        .cloned()
                        .unwrap_or_else(|| "image/png".into());
                    let url = bm::check_url(&base).map_err(|e| e.0)?.to_string();
                    let id = self.new_map_id();
                    let rec = record(
                        &id,
                        "wms",
                        &string_of(l.get("title")),
                        &url,
                        &string_of(l.get("name")),
                        "",
                        &fmt,
                        &string_of(cap.get("version")),
                        crs.as_ref(),
                        &string_of(l.get("attribution")),
                        "",
                        (0.0, 0.0),
                        None,
                        "",
                        "",
                        "",
                    );
                    self.add_map(rec, &tr!("mapl.undo.add", name = string_of(l.get("title"))));
                }
                "wmts" => {
                    let l = arr(cap.get("layers")).get(li).cloned().ok_or("no layer")?;
                    let sets = cap.get("sets").cloned().unwrap_or(Value::Null);
                    let sid = if set.is_empty() {
                        arr(l.get("sets"))
                            .iter()
                            .map(|s| string_of(Some(s)))
                            .find(|s| sets.get(s).and_then(|x| x.get("code")).and_then(Value::as_f64) == Some(3857.0))
                            .or_else(|| arr(l.get("sets")).first().map(|s| string_of(Some(s))))
                            .unwrap_or_default()
                    } else {
                        set.to_string()
                    };
                    let st = sets.get(&sid).cloned().ok_or_else(|| t("mapl.wmts.no_set", &[]))?;
                    let code = st.get("code").and_then(Value::as_f64).ok_or_else(|| t("mapl.wmts.no_set", &[]))?;
                    let crs = crate::civilgis::crs_of_code(&cc_civil::js::num_to_string(code)).map_err(|_| t("mapl.wmts.no_set", &[]))?;
                    let unit = string_of(crs.get("unit"));
                    let f = cc_io_civil::crs::frame(&crs).ok().flatten();
                    let to_set = |lon: f64, lat: f64| f.as_ref().and_then(|f| f.from_lon_lat(lon, lat).ok()).map(|v| (v[0], v[1]));
                    let bbox84 = l.get("bbox84").and_then(Value::as_array).and_then(|a| {
                        let v: Vec<f64> = a.iter().filter_map(Value::as_f64).collect();
                        match v.as_slice() {
                            [a, b, c, d] => Some([*a, *b, *c, *d]),
                            _ => None,
                        }
                    });
                    let grid = bm::wmts_grid(&st, &unit, Some(&to_set), bbox84);
                    let fmts: Vec<String> = arr(l.get("formats")).iter().map(|x| string_of(Some(x))).collect();
                    let fmt = fmts
                        .iter()
                        .find(|x| x.to_ascii_lowercase().contains("png"))
                        .or(fmts.first())
                        .cloned()
                        .unwrap_or_else(|| "image/png".into());
                    let tpls = arr(l.get("templates"));
                    let tpl = tpls
                        .iter()
                        .find(|x| x.get("format").and_then(Value::as_str) == Some(fmt.as_str()))
                        .or(tpls.first())
                        .map(|x| string_of(x.get("template")))
                        .unwrap_or_default();
                    let styles = arr(l.get("styles"));
                    let style = styles
                        .iter()
                        .find(|x| x.get("isDefault") == Some(&Value::Bool(true)))
                        .or(styles.first())
                        .map_or_else(|| "default".into(), |x| string_of(x.get("id")));
                    let kvp = string_of(cap.get("kvp"));
                    if tpl.is_empty() && kvp.is_empty() {
                        return Err(t("mapl.wmts.no_tile", &[]));
                    }
                    let url = bm::check_url(if kvp.is_empty() { &base } else { &kvp }).map_err(|e| e.0)?.to_string();
                    let id = self.new_map_id();
                    let rec = record(
                        &id,
                        "wmts",
                        &string_of(l.get("title")),
                        &url,
                        &string_of(l.get("id")),
                        &style,
                        &fmt,
                        "1.3.0",
                        Some(&crs),
                        &string_of(l.get("attribution")),
                        "",
                        (0.0, 0.0),
                        None,
                        &bm::grid_json(&grid),
                        &sid,
                        &tpl,
                    );
                    self.add_map(rec, &tr!("mapl.undo.add", name = string_of(l.get("title"))));
                }
                _ => {
                    let name = cap
                        .get("name")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .map_or_else(|| t("mapl.arc.default_name", &[]), str::to_string);
                    let id = self.new_map_id();
                    let rec = if cap.get("tiled") == Some(&Value::Bool(true)) {
                        let crs = crate::civilgis::crs_of_code("3857").ok();
                        let grid = cap.get("grid").and_then(|g| bm::read_grid(g).ok()).map(|g| bm::grid_json(&g)).unwrap_or_default();
                        record(
                            &id,
                            "arcgis",
                            &name,
                            &base,
                            &string_of(cap.get("kind")),
                            "",
                            "image/png",
                            "1.3.0",
                            crs.as_ref(),
                            &string_of(cap.get("attribution")),
                            "",
                            (0.0, 0.0),
                            None,
                            &grid,
                            "",
                            "",
                        )
                    } else {
                        let code = pcode.or_else(|| cap.get("code").and_then(Value::as_f64)).ok_or_else(|| t("mapl.arc.no_place", &[]))?;
                        let crs = if Some(code) == pcode {
                            pc.as_ref().map(with_local_null)
                        } else {
                            crate::civilgis::crs_of_code(&cc_civil::js::num_to_string(code)).ok()
                        };
                        record(
                            &id,
                            "arcgis",
                            &name,
                            &base,
                            &string_of(cap.get("kind")),
                            "",
                            "image/png",
                            "1.3.0",
                            crs.as_ref(),
                            &string_of(cap.get("attribution")),
                            "",
                            (0.0, 0.0),
                            None,
                            "",
                            "",
                            "",
                        )
                    };
                    self.add_map(rec, &tr!("mapl.undo.add", name = name));
                }
            }
            Ok(())
        })();
        let msg = match status {
            Ok(()) => t(if self.internet_on() { "mapl.added" } else { "mapl.added_off" }, &[]),
            Err(e) => e,
        };
        // gis-map-ui.js: said in the dialog only
        self.maps.dlg.get_or_insert_with(MapDlg::default).status = msg;
    }

    /// `civil.maps.info`: each layer's row (what the dialog shows) and the attribution under the plan.
    pub fn maps_info(&self) -> String {
        let mut out: Vec<String> = Vec::new();
        for m in self.maps_list() {
            let id = string_of(m.get("id"));
            let off = is_internet(&m) && !self.internet_on();
            let needs = self.map_conv(&m).is_none();
            let why = if off {
                t("mapl.why.off", &[])
            } else if needs {
                t("mapl.why.crs", &[])
            } else if is_internet(&m) && !self.maps.trusted.contains(&id) {
                "server named by the project file: allow it first".to_string()
            } else {
                self.maps.notes.get(&id).map(|n| n.0.clone()).unwrap_or_default()
            };
            out.push(format!(
                "{} {} {} {}% {}",
                id,
                string_of(m.get("name")),
                if m.get("visible") == Some(&Value::Bool(true)) { "on" } else { "off" },
                cc_civil::js::num_to_string((numf(m.get("opacity")) * 100.0).round()),
                why
            ));
        }
        if out.is_empty() {
            out.push(t("mapl.none", &[]));
        }
        out.join(" | ")
    }
}

/// The host's failure words in 0.5's: "http:<status> <text>" and "net:" (gis-map-ui.js load / netError).
fn net_words(e: &str, reading: bool) -> String {
    if let Some(st) = e.strip_prefix("http:") {
        if st.starts_with("404") {
            return t(if reading { "mapl.http.404" } else { "mapl.err.404" }, &[]);
        }
        return tr!("mapl.http.status", status = st.trim());
    }
    if e.starts_with("net:") {
        return t(if reading { "mapl.err.cors_read" } else { "mapl.err.cors_img" }, &[]);
    }
    e.to_string()
}

/// `new URL(tpl.replace(/\{-?\w+\}/g, '0')).hostname`.
fn url_host(tpl: &str) -> String {
    let mut z = String::new();
    let mut rest = tpl;
    while let Some(i) = rest.find('{') {
        z.push_str(rest.get(..i).unwrap_or(""));
        let tail = rest.get(i + 1..).unwrap_or("");
        let body: String = tail.chars().take_while(|c| *c != '}').collect();
        let inner = body.strip_prefix('-').unwrap_or(&body);
        if tail.len() > body.len() && !inner.is_empty() && inner.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            z.push('0');
            rest = tail.get(body.len() + 1..).unwrap_or("");
        } else {
            z.push('{');
            rest = tail;
        }
    }
    z.push_str(rest);
    url::Url::parse(&z).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_else(|| url_host_plain(&z))
}

fn url_host_plain(tpl: &str) -> String {
    let s = tpl.split("://").nth(1).unwrap_or(tpl);
    s.split(['/', '?', '#']).next().unwrap_or("").rsplit('@').next().unwrap_or("").split(':').next().unwrap_or("").to_string()
}

/// The Map dialog (gis-map-ui.js openDialog).
pub fn window(cv: &mut CivilView, ctx: &egui::Context, tk: &Tokens, acts: &mut Vec<(&'static str, String)>) {
    let Some(mut d) = cv.maps.dlg.clone() else { return };
    let list = cv.maps_list();
    let mut open = true;
    egui::Window::new(t0("mapl.title")).id(egui::Id::unique("cv_maps")).open(&mut open).collapsible(false).default_width(520.0).show(
        ctx,
        |ui| {
            if !cv.internet_on() {
                ui.label(RichText::new(format!("{} {}", t0("mapl.off.head"), t0("mapl.off.body"))).small().color(tk.text));
                if ui.button(t0("mapl.open_settings")).clicked() {
                    acts.push(("civil.settings.open", String::new()));
                }
            }
            if cv.pcrs().is_none() {
                ui.label(RichText::new(t0("mapl.no_crs")).small().color(Color32::from_rgb(0xd0, 0x4a, 0x5a)));
            }
            ui.label(RichText::new(t0("mapl.layers")).strong().color(tk.text_strong));
            if list.is_empty() {
                ui.label(RichText::new(t0("mapl.none")).small().color(tk.text_dim));
            }
            let n = list.len();
            for (i, m) in list.iter().enumerate() {
                let id = string_of(m.get("id"));
                ui.horizontal(|ui| {
                    let mut vis = m.get("visible") == Some(&Value::Bool(true));
                    if ui.checkbox(&mut vis, string_of(m.get("name"))).changed() {
                        acts.push(("civil.maps", format!("visible={id}:{}", if vis { "on" } else { "off" })));
                    }
                    let mut op = (numf(m.get("opacity")) * 100.0).round();
                    if ui.add(egui::Slider::new(&mut op, 0.0..=100.0).step_by(5.0).text(t0("mapl.opacity"))).drag_stopped() {
                        acts.push(("civil.maps", format!("opacity={id}:{}", op / 100.0)));
                    }
                    if ui.add_enabled(i > 0, egui::Button::new("↓")).on_hover_text(t0("mapl.down.title")).clicked() {
                        acts.push(("civil.maps", format!("move={id}:-1")));
                    }
                    if ui.add_enabled(i + 1 < n, egui::Button::new("↑")).on_hover_text(t0("mapl.up.title")).clicked() {
                        acts.push(("civil.maps", format!("move={id}:1")));
                    }
                    if m.get("kind").and_then(Value::as_str) == Some("image") && ui.button(t0("vec.zoomto")).clicked() {
                        acts.push(("civil.maps", format!("zoom={id}")));
                    }
                    if ui.button("✕").on_hover_text(t0("mapl.remove.title")).clicked() {
                        acts.push(("civil.maps", format!("remove={id}")));
                    }
                });
                if is_internet(m) && !cv.maps.trusted.contains(&id) {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("This layer's server came with the project file.").small().color(tk.text_dim));
                        if ui.small_button("Allow it").clicked() {
                            acts.push(("civil.maps", format!("allow={id}")));
                        }
                    });
                }
                if let Some(a) = m.get("attribution").and_then(Value::as_str).filter(|a| !a.is_empty()) {
                    ui.label(RichText::new(tr!("mapl.attribution", text = a)).small().color(tk.text_dim));
                }
            }
            ui.separator();
            ui.label(RichText::new(t0("mapl.add_layer")).strong().color(tk.text_strong));
            egui::ComboBox::new("cv_map_kind", t0("mapl.from")).selected_text(kind_label(&d.kind)).show_ui(ui, |ui| {
                for k in bm::KINDS {
                    if ui.selectable_label(d.kind == k, kind_label(k)).clicked() {
                        acts.push(("civil.maps", format!("kind={k}")));
                    }
                }
            });
            if d.kind == "image" {
                ui.label(RichText::new(t0("mapl.files.help")).small().color(tk.text_dim));
                if ui.button(t0("mapl.files")).clicked() {
                    acts.push(("civil.gis.pick", String::new()));
                }
            } else {
                ui.label(RichText::new(t0(&format!("mapl.help.{}", d.kind))).small().color(tk.text_dim));
                ui.horizontal(|ui| {
                    ui.label(t0("mapl.f.url"));
                    if ui.add(egui::TextEdit::singleline(&mut d.url).desired_width(360.0)).changed() {
                        cv.maps.dlg.get_or_insert_with(MapDlg::default).url = d.url.clone();
                    }
                });
                if d.kind == "xyz" {
                    ui.horizontal(|ui| {
                        ui.label(t0("mapl.f.name"));
                        if ui.text_edit_singleline(&mut d.name).changed() {
                            cv.maps.dlg.get_or_insert_with(MapDlg::default).name = d.name.clone();
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(t0("mapl.f.att"));
                        if ui.text_edit_singleline(&mut d.att).changed() {
                            cv.maps.dlg.get_or_insert_with(MapDlg::default).att = d.att.clone();
                        }
                    });
                    if ui.button(t0("mapl.add")).clicked() {
                        acts.push(("civil.maps", format!("xyz={}|{}|{}", d.url, d.name, d.att)));
                    }
                } else {
                    let label = if d.kind == "arcgis" { t0("mapl.read_service") } else { t0("mapl.get_layers") };
                    if ui.add_enabled(cv.internet_on(), egui::Button::new(label)).clicked() {
                        acts.push(("civil.maps", "read".into()));
                    }
                    if let Some((_, cap, _)) = &d.found {
                        let layers = arr(cap.get("layers"));
                        if !layers.is_empty() {
                            egui::ComboBox::new("cv_map_layer", t0("mapl.f.layer"))
                                .selected_text(layers.get(d.layer).map(|l| string_of(l.get("title"))).unwrap_or_default())
                                .show_ui(ui, |ui| {
                                    for (i, l) in layers.iter().enumerate().take(2000) {
                                        if ui.selectable_label(d.layer == i, string_of(l.get("title"))).clicked() {
                                            cv.maps.dlg.get_or_insert_with(MapDlg::default).layer = i;
                                        }
                                    }
                                });
                        }
                        if ui.button(t0("mapl.add")).clicked() {
                            acts.push(("civil.maps", format!("add={}", d.layer)));
                        }
                    }
                    if !cv.internet_on() {
                        ui.label(RichText::new(t0("mapl.read.needs_net")).small().color(tk.text_dim));
                    }
                }
            }
            if !d.status.is_empty() {
                ui.label(RichText::new(&d.status).small().color(tk.text));
            }
            ui.label(RichText::new(t0("mapl.screen_only")).small().color(tk.text_dim));
        },
    );
    if !open {
        cv.maps.dlg = None;
    }
}

fn kind_label(k: &str) -> String {
    match k {
        "image" => t0("mapl.kind.image"),
        "xyz" => t0("mapl.kind.xyz"),
        "wms" => t0("mapl.kind.wms_server"),
        "wmts" => t0("mapl.kind.wmts_server"),
        _ => t0("mapl.kind.arcgis"),
    }
}

/// The visible map layers over `b` as a picture file (civilcraft.js `mapPicture`): JPEG (quality 0.88, the PDF) or
/// PNG (the DWG) → (bytes, px w, px h, attribution, complete).
pub fn map_picture(cv: &mut CivilView, b: [f64; 4], max_px: f64, jpeg: bool, now: f64) -> Option<(Vec<u8>, f64, f64, String, bool)> {
    if !cv.visible_maps() {
        return None;
    }
    let t0 = web_time::Instant::now();
    let (w, h, px, att, complete) = cv.maps_drape(b, max_px, now)?;
    cv.timings.insert("map drape", t0.elapsed().as_secs_f64() * 1000.0);
    let img = image::RgbaImage::from_raw(w, h, px)?;
    let mut out = std::io::Cursor::new(Vec::new());
    if jpeg {
        let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 88);
        enc.encode_image(&rgb).ok()?;
    } else {
        img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    }
    Some((out.into_inner(), f64::from(w), f64::from(h), att, complete))
}

/// The record a picture keeps in the project needs the map list type for tests.
pub fn records_of(state: &Value) -> Vec<Map<String, Value>> {
    arr(state.get("gis").and_then(|g| g.get("maps"))).iter().filter_map(|m| m.as_object().cloned()).collect()
}

/// gis-map-ui.js `reg.extents`: the visible pictures' corners in project coordinates (they count for Fit and the
/// exports' extents).
pub fn image_extents(p: &cc_civil::project::Project) -> Option<(f64, f64, f64, f64)> {
    let maps = arr(p.state.get("gis").and_then(|g| g.get("maps")));
    let mut b: Option<(f64, f64, f64, f64)> = None;
    for m in maps {
        if m.get("kind").and_then(Value::as_str) != Some("image") || m.get("visible") != Some(&Value::Bool(true)) {
            continue;
        }
        let conv = match layer_crs(m) {
            None => Conv { frame: None, src: None, same: true },
            Some(src) => {
                let Some(pc) = p.state.get("crs").filter(|c| cc_civil::js::truthy(Some(c))) else { continue };
                crate::civilgis::ensure_catalog();
                if cc_io_civil::crs::crs_problem(pc, p.units()).is_some() {
                    continue;
                }
                let Ok(Some(f)) = cc_io_civil::crs::frame(pc) else { continue };
                let same = src.get("proj4") == pc.get("proj4");
                Conv { frame: Some(f), src: Some(src), same }
            }
        };
        let g = |k: &str| numf(m.get(k));
        let gt = [g("a0"), g("a1"), g("a2"), g("a3"), g("a4"), g("a5")];
        for (u, v) in [(0.0, 0.0), (g("width"), 0.0), (g("width"), g("height")), (0.0, g("height"))] {
            let (sx, sy) = bm::pixel_to_system(&gt, u, v);
            let Some((x, y)) = conv.from_src(sx, sy).filter(|(x, y)| x.is_finite() && y.is_finite()) else { continue };
            b = Some(match b {
                None => (x, y, x, y),
                Some((a, c, d, e)) => (a.min(x), c.min(y), d.max(x), e.max(y)),
            });
        }
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let px: Vec<u8> = (0..w * h).flat_map(|i| [(i % 251) as u8, 120, 60, 255]).collect();
        cc_io_markup::picture::encode_png(w, h, &px).unwrap()
    }

    #[test]
    fn a_picture_with_its_world_file_draws_into_the_drape_and_never_asks_a_server() {
        let mut cv = CivilView::sample();
        cv.maps_file("a.pgw", b"1\n0\n0\n-1\n6150000\n2080200\n".to_vec());
        assert!(cv.maps_list().is_empty(), "the world file waits for its picture");
        cv.maps_file("a.png", png(40, 30));
        let ms = cv.maps_list();
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0]["image"], "img1");
        assert!(
            image_extents(&cv.project).is_some_and(|b| (b.0 - 6_149_999.5).abs() < 1e-9 && (b.3 - 2_080_200.5).abs() < 1e-9),
            "world files name pixel centres"
        );
        let (w, h, px, _, complete) = cv.maps_drape([6_150_000.0, 2_080_170.0, 6_150_040.0, 2_080_200.0], 80.0, 0.0).unwrap();
        assert_eq!((w, h), (80, 60));
        assert!(complete);
        assert!(px.chunks(4).any(|p| p.first().copied() != Some(255)), "the picture is in the drape");
        cv.maps_cmd("opacity=m1:0.33").unwrap();
        assert_eq!(cv.maps_list()[0]["opacity"], json!(0.35));
        cv.maps_cmd("xyz=https://t.example.org/{z}/{x}/{y}.png||").unwrap();
        let ms = cv.maps_list();
        assert_eq!(ms.len(), 2);
        assert_eq!(ms[1]["name"], "t.example.org");
        // internet off: tiles are never asked for
        let _ = cv.maps_drape([6_150_000.0, 2_080_170.0, 6_150_040.0, 2_080_200.0], 80.0, 0.0);
        assert!(cv.maps.to_fetch.is_empty());
        cv.maps_cmd("remove=m2").unwrap();
        assert_eq!(cv.maps_list().len(), 1);
        assert!(cv.maps_cmd("bogus=1").is_err());
    }

    #[test]
    fn a_layer_from_a_file_is_not_fetched_until_allowed() {
        let mut cv = CivilView::sample();
        cv.pt_settings.internet = true;
        cv.crs_set("2232").unwrap();
        let rec = record(
            "m1",
            "xyz",
            "Streets",
            "https://t.example.org/{z}/{x}/{y}.png",
            "",
            "",
            "image/png",
            "1.3.0",
            crate::civilgis::crs_of_code("3857").ok().as_ref(),
            "",
            "",
            (0.0, 0.0),
            None,
            "",
            "",
            "",
        );
        cv.project.change("x", &mut |s: &mut Value| {
            s["gis"]["maps"] = json!([rec.clone()]);
        });
        let _ = cv.maps_drape([3_140_000.0, 1_700_000.0, 3_141_000.0, 1_701_000.0], 256.0, 0.0);
        assert!(cv.maps.to_fetch.is_empty(), "named by the file: not asked");
        cv.maps_cmd("allow=m1").unwrap();
        let _ = cv.maps_drape([3_140_000.0, 1_700_000.0, 3_141_000.0, 1_701_000.0], 256.0, 0.0);
        assert!(!cv.maps.to_fetch.is_empty() && cv.maps.to_fetch.iter().all(|u| u.starts_with("https://t.example.org/")));
        let u = cv.maps.to_fetch.first().cloned().unwrap();
        cv.maps_answer(&u, Err("http:404 Not Found".into()));
        assert!(cv.maps_info().contains("m1"));
    }
}
