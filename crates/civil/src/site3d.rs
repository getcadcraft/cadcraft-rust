//! site3d.js — the CivilCraft 3D site scene as plain arrays (no GPU types): the 3D view draws them, the GLB / OBJ
//! exporters (cc-io-civil `site3d_export`) write them, the tests check them.
//!
//! Large coordinates: every position is LOCAL — project coordinate minus `scene.origin` (x, y: the existing ground's
//! own local origin, so state-plane eastings and northings in the millions keep their precision in f32; z: the lowest
//! elevation, rounded down) — and z is multiplied by the vertical exaggeration. `X = x + origin.x`,
//! `Y = y + origin.y`, `Z = z / exag + origin.z`. Positions are computed in f64 and stored as f32 (the JS
//! `Float32Array`), so the arrays are the same numbers the JS builds.
//!
//! Picking and label decluttering need a camera; they take one through [`PickCamera`] (ModelCraft's camera,
//! cc-m3d-tools, implements it above this layer).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde_json::{Map, Value};

use crate::alignment::Alignment;
use crate::geo::{P2, P3};
use crate::gis_state::{Feature as GisFeature, Geom};
use crate::js::{G, J, JsMath, arr, hypot3, numf};
use crate::pipes::{Analysis, Network};
use crate::points::Pt;
use crate::profile::Profile;
use crate::project::{GradedResult, Project};
use crate::tin::{Built, Contour, Surface};
use crate::tr;
use crate::units::{fixed, format_station};

pub const MAX_LABELS: usize = 6000;
pub const MAX_GIS_SEGMENTS: i64 = 300_000;
pub const EXAGGERATIONS: [f64; 5] = [1.0, 2.0, 3.0, 5.0, 10.0];

/// `LABEL_PRI[kind]`.
pub fn label_pri(kind: &str) -> Option<f64> {
    Some(match kind {
        "alignment" => 0.0,
        "structure" => 1.0,
        "pad" => 2.0,
        "feature" => 3.0,
        "station" => 4.0,
        "contour" => 5.0,
        "point" => 6.0,
        _ => return None,
    })
}

/// `DEFAULT_COLORS`, in order.
pub const DEFAULT_COLORS: [(&str, &str); 22] = [
    ("surface", "#c9c1aa"),
    ("graded", "#b9c4cf"),
    ("tin", "#7a8796"),
    ("major", "#17202b"),
    ("minor", "#8c96a2"),
    ("breakline", "#a84a00"),
    ("edge", "#34404e"),
    ("alignment", "#b3122e"),
    ("corridor", "#8a6200"),
    ("pad", "#1e48c8"),
    ("daylight", "#9a1f86"),
    ("pipe", "#1e48c8"),
    ("structure", "#1e48c8"),
    ("problem", "#b3122e"),
    ("point", "#17703a"),
    ("gis:blue", "#1e48c8"),
    ("gis:orange", "#a84a00"),
    ("gis:magenta", "#9a1f86"),
    ("gis:cyan", "#00707e"),
    ("gis:ochre", "#8a6200"),
    ("gis:green", "#17703a"),
    ("gis:red", "#b3122e"),
];

/// `SHOW_DEFAULTS`.
#[derive(Clone, Copy, Debug)]
pub struct Show {
    pub eg: bool,
    pub fg: bool,
    pub triangles: bool,
    pub contours: bool,
    pub breaklines: bool,
    pub boundary: bool,
    pub alignments: bool,
    pub corridors: bool,
    pub pads: bool,
    pub pipes: bool,
    pub points: bool,
    pub gis: bool,
    pub labels: bool,
}
impl Default for Show {
    fn default() -> Self {
        Show {
            eg: true,
            fg: true,
            triangles: false,
            contours: true,
            breaklines: true,
            boundary: true,
            alignments: true,
            corridors: true,
            pads: true,
            pipes: true,
            points: false,
            gis: true,
            labels: true,
        }
    }
}
impl Show {
    /// `{ ...SHOW_DEFAULTS, ...show }`, each flag read as JS truthiness.
    pub fn read(v: Option<&Value>) -> Show {
        let mut s = Show::default();
        let Some(Value::Object(m)) = v else { return s };
        for (k, x) in m {
            let b = crate::js::truthy(Some(x));
            match k.as_str() {
                "eg" => s.eg = b,
                "fg" => s.fg = b,
                "triangles" => s.triangles = b,
                "contours" => s.contours = b,
                "breaklines" => s.breaklines = b,
                "boundary" => s.boundary = b,
                "alignments" => s.alignments = b,
                "corridors" => s.corridors = b,
                "pads" => s.pads = b,
                "pipes" => s.pipes = b,
                "points" => s.points = b,
                "gis" => s.gis = b,
                "labels" => s.labels = b,
                _ => {}
            }
        }
        s
    }
}

// ---------------------------------------------------------------------------- input

/// A surface held by the project's caches (EG lives in a `Built`, the graded surface in an `Arc`).
#[derive(Clone)]
pub enum SurfRef {
    Built(Arc<Built>),
    Graded(Arc<GradedResult>),
}
impl SurfRef {
    pub fn get(&self) -> Option<&Surface> {
        match self {
            SurfRef::Built(b) => b.surface.as_ref(),
            SurfRef::Graded(g) => g.surface.as_deref(),
        }
    }
}

/// What a line or mesh piece belongs to (for picking): `{kind, id, name}`; `None` = undefined in the JS.
#[derive(Clone, Debug, PartialEq)]
pub struct Owner {
    pub kind: &'static str,
    pub id: Option<Value>,
    pub name: Option<Value>,
}
impl J for Owner {
    fn j(&self) -> Value {
        let mut m = Map::new();
        m.insert("kind".into(), Value::String(self.kind.into()));
        if let Some(v) = &self.id {
            m.insert("id".into(), v.clone());
        }
        if let Some(v) = &self.name {
            m.insert("name".into(), v.clone());
        }
        Value::Object(m)
    }
}

pub struct InBreakline {
    pub pts: Vec<P3>,
    pub owner: Owner,
}
pub struct InFeature {
    pub id: Option<Value>,
    pub name: Option<Value>,
    pub pts: Vec<P3>,
}
pub struct InAlignment {
    pub id: Option<Value>,
    pub name: Option<Value>,
    pub alignment: Option<Arc<Alignment>>,
    pub profile: Option<Arc<Profile>>,
}
pub struct InPad {
    pub id: Option<Value>,
    pub name: Option<Value>,
    pub pts: Vec<P2>,
    pub z: f64,
    pub daylight: Vec<P3>,
}
pub struct InGis {
    pub id: String,
    pub name: String,
    pub color: String,
    pub features: Vec<GisFeature>,
}

/// `siteInput(project)` (or one assembled by hand: every field may be empty).
pub struct SiteInput {
    pub units: String,
    pub eg: Option<SurfRef>,
    pub fg: Option<SurfRef>,
    pub contours_eg: Arc<Vec<Contour>>,
    pub contours_fg: Arc<Vec<Contour>>,
    pub interval: f64,
    pub major: f64,
    pub band_count: f64,
    pub breaklines: Vec<InBreakline>,
    pub features: Vec<InFeature>,
    pub alignments: Vec<InAlignment>,
    /// corridor feature lines, per corridor: (name, points)
    pub corridors: Vec<Vec<(String, Vec<P3>)>>,
    pub pads: Vec<InPad>,
    pub network: Option<Network>,
    pub check: Option<Analysis>,
    pub points: Vec<Pt>,
    pub gis: Vec<InGis>,
}
impl SiteInput {
    pub fn empty(units: &str) -> SiteInput {
        SiteInput {
            units: units.to_string(),
            eg: None,
            fg: None,
            contours_eg: Arc::new(Vec::new()),
            contours_fg: Arc::new(Vec::new()),
            interval: 1.0,
            major: 5.0,
            band_count: 8.0,
            breaklines: Vec::new(),
            features: Vec::new(),
            alignments: Vec::new(),
            corridors: Vec::new(),
            pads: Vec::new(),
            network: None,
            check: None,
            points: Vec::new(),
            gis: Vec::new(),
        }
    }
    pub fn eg(&self) -> Option<&Surface> {
        self.eg.as_ref().and_then(SurfRef::get)
    }
    pub fn fg(&self) -> Option<&Surface> {
        self.fg.as_ref().and_then(SurfRef::get)
    }
}

/// `a === b` for JSON values: objects and arrays are never the same object here.
fn strict_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Null, Value::Null) => true,
        _ => false,
    }
}
fn gf(a: &[f32], i: usize) -> f64 {
    a.get(i).map_or(f64::NAN, |v| f64::from(*v))
}
fn gu(a: &[u32], i: usize) -> u32 {
    a.get(i).copied().unwrap_or(0)
}
fn opt_val(v: Option<&Value>) -> Option<Value> {
    v.cloned()
}
fn or1(v: Option<&Value>, d: f64) -> f64 {
    let n = crate::js::number(v);
    if n != 0.0 && !n.is_nan() { n } else { d }
}

/// `siteInput(project)`.
pub fn site_input(project: &mut Project) -> SiteInput {
    let units = crate::js::to_js_string(project.state.get("units").unwrap_or(&Value::Null));
    let eg_b = project.eg();
    let g = project.graded();
    let has_eg = eg_b.surface.is_some();
    let has_fg = g.surface.is_some();
    let d = project.state.get("display").cloned().unwrap_or(Value::Null);
    let contours_eg = if has_eg { project.contours() } else { Arc::new(Vec::new()) };
    let contours_fg = if has_fg { project.contours_fg() } else { Arc::new(Vec::new()) };
    // breaklines: point groups' (owner group:<name>) and drawn ones (owner drawn:<index>); feature lines separately
    let mut breaklines = Vec::new();
    let all = project.eg_breaklines();
    let n_feat = crate::featureline::feature_breaklines(&project.features(), "eg").len();
    let drawn = arr(project.state.get("surface").and_then(|s| s.get("breaklines"))).to_vec();
    let n_keep = all.len().saturating_sub(n_feat);
    let n_group = n_keep.saturating_sub(drawn.len());
    for (i, b) in all.into_iter().take(n_keep).enumerate() {
        let owner = if i < n_group {
            Owner { kind: "breakline", id: Some(Value::String(format!("group:{}", b.name))), name: Some(Value::String(b.name.clone())) }
        } else {
            let k = i.saturating_sub(n_group);
            Owner {
                kind: "breakline",
                id: Some(Value::String(format!("drawn:{k}"))),
                name: drawn.get(k).and_then(|v| v.get("name")).cloned(),
            }
        };
        breaklines.push(InBreakline { pts: b.pts, owner });
    }
    let features = arr(project.state.get("features"))
        .iter()
        .map(|f| {
            let rf = crate::featureline::read_feature(f);
            let mut pts = rf.pts.clone();
            if rf.closed
                && let Some(p0) = pts.first().copied()
            {
                pts.push(p0);
            }
            InFeature { id: opt_val(f.get("id")), name: opt_val(f.get("name")), pts }
        })
        .collect();
    let als = arr(project.state.get("alignments")).to_vec();
    let alignments = als
        .iter()
        .map(|a| InAlignment {
            id: opt_val(a.get("id")),
            name: opt_val(a.get("name")),
            alignment: Some(project.alignment(a)),
            profile: Some(project.design_profile(a)),
        })
        .collect();
    let corridors = g.items.iter().filter(|x| x.kind == "corridor").filter_map(|x| x.lines.clone()).collect();
    let pads = arr(project.state.get("pads"))
        .iter()
        .map(|p| {
            let id = p.get("id");
            let daylight = g
                .items
                .iter()
                .find(|x| id.is_some_and(|id| strict_eq(&x.id, id)))
                .and_then(|x| x.info.get("daylight"))
                .map(|v| arr(Some(v)).iter().map(crate::geo::read_p3).collect())
                .unwrap_or_default();
            InPad {
                id: opt_val(id),
                name: opt_val(p.get("name")),
                pts: crate::geo::read_p2s(p.get("pts")),
                z: crate::js::number(p.get("z")),
                daylight,
            }
        })
        .collect();
    let network = project.state.get("network").filter(|v| crate::js::truthy(Some(v))).map(crate::pipes::read_network);
    let check = if network.is_some() { project.network_check() } else { None };
    let mut gis = Vec::new();
    for l in arr(project.state.get("gis").and_then(|g| g.get("layers"))) {
        if !crate::js::truthy(l.get("visible")) {
            continue;
        }
        let id = crate::js::string_of(l.get("id"));
        let features = project.gis_features.iter().find(|(k, _)| *k == id).map(|(_, f)| f.clone()).unwrap_or_default();
        gis.push(InGis { id, name: crate::js::string_of(l.get("name")), color: crate::js::string_of(l.get("color")), features });
    }
    SiteInput {
        units,
        eg: if has_eg { Some(SurfRef::Built(eg_b)) } else { None },
        fg: if has_fg { Some(SurfRef::Graded(g.clone())) } else { None },
        contours_eg,
        contours_fg,
        interval: or1(d.get("interval"), 1.0),
        major: or1(d.get("major"), 5.0),
        band_count: or1(d.get("bandCount"), 8.0),
        breaklines,
        features,
        alignments,
        corridors,
        pads,
        network,
        check,
        points: project.points.clone(),
        gis,
    }
}

// ---------------------------------------------------------------------------- colours

/// `bandLevels(stats, n)`.
pub fn band_levels(min_z: f64, max_z: f64, n: f64) -> Vec<f64> {
    let mut levels = Vec::new();
    let mut i = 1.0;
    while i < n {
        levels.push(min_z + ((max_z - min_z) * i) / n);
        i += 1.0;
    }
    levels
}
/// `bandColor(t, dark)`.
pub fn band_color(t: f64, dark: bool) -> String {
    let h = 190.0 - 160.0 * t;
    let s = 45.0;
    let l = if dark { 30.0 + 10.0 * t } else { 82.0 - 22.0 * t };
    hsl_hex(h, s, l)
}
fn hsl_hex(h: f64, s: f64, l: f64) -> String {
    let s = s / 100.0;
    let l = l / 100.0;
    let k = |n: f64| (n + h / 30.0) % 12.0;
    let a = s * l.min(1.0 - l);
    let f = |n: f64| crate::js::round(255.0 * (l - a * crate::js::max(-1.0, crate::js::min(k(n) - 3.0, crate::js::min(9.0 - k(n), 1.0)))));
    // the values are whole numbers in 0–255 (`v.toString(16).padStart(2, '0')`)
    let hx = |v: f64| format!("{:02x}", crate::js::usize_of(v).unwrap_or(0));
    format!("#{}{}{}", hx(f(0.0)), hx(f(8.0)), hx(f(4.0)))
}
/// `rgb(hex)` → [r, g, b] in 0–1.
pub fn rgb(hex: &str) -> [f64; 3] {
    let t = crate::js::trim(hex);
    let h = t.strip_prefix('#').unwrap_or(t);
    if h.len() == 6 && h.bytes().all(|c| c.is_ascii_hexdigit()) {
        let v = u32::from_str_radix(h, 16).unwrap_or(0);
        return [f64::from((v >> 16) & 255) / 255.0, f64::from((v >> 8) & 255) / 255.0, f64::from(v & 255) / 255.0];
    }
    // /rgba?\(\s*([\d.]+)[ ,]+([\d.]+)[ ,]+([\d.]+)/i
    if let Some(m) = crate::js::exec(crate::re!(r"rgba?\(\s*([\d.]+)[ ,]+([\d.]+)[ ,]+([\d.]+)", i), hex) {
        let n = |i: usize| crate::js::str_to_number(m.get(i).and_then(|x| x.as_deref()).unwrap_or("")) / 255.0;
        return [n(1), n(2), n(3)];
    }
    [0.5, 0.5, 0.5]
}

// ---------------------------------------------------------------------------- scene

/// A mesh: `positions`, `normals`, `colors` (f32 xyz / rgb), `indices`, and for owned meshes `owners` + `tri_owner`.
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub key: String,
    pub name: String,
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub colors: Vec<f32>,
    pub indices: Vec<u32>,
    pub uvs: Option<Vec<f32>>,
    pub owners: Option<Vec<Owner>>,
    pub tri_owner: Option<Vec<u32>>,
}
/// A line set (segment pairs).
#[derive(Clone, Debug, Default)]
pub struct Line {
    pub key: String,
    pub name: String,
    pub color: String,
    pub positions: Vec<f32>,
    pub owners: Option<Vec<Owner>>,
    pub seg_owner: Option<Vec<u32>>,
}
#[derive(Clone, Debug, Default)]
pub struct Points {
    pub positions: Vec<f32>,
    pub ids: Vec<String>,
    pub color: String,
}
#[derive(Clone, Debug)]
pub struct Label {
    pub text: String,
    pub at: [f64; 3],
    pub kind: &'static str,
    pub pri: Option<f64>,
    pub color: String,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Counts {
    pub triangles: f64,
    pub segments: f64,
    pub points: f64,
    pub pipes: f64,
    pub structures: f64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Origin {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}
#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub units: String,
    pub origin: Origin,
    pub exag: f64,
    pub meshes: Vec<Mesh>,
    pub lines: Vec<Line>,
    pub points: Option<Points>,
    pub notes: Vec<String>,
    pub labels: Vec<Label>,
    pub counts: Counts,
    pub bbox: Option<([f64; 3], [f64; 3])>,
}

fn f32s(v: &[f64]) -> Vec<f32> {
    v.iter().map(|&x| to_f32(x)).collect()
}
/// `Math.fround` / a Float32Array store (round to nearest, ties to even).
#[allow(clippy::cast_possible_truncation)]
pub fn to_f32(x: f64) -> f32 {
    x as f32
}
fn f32v(v: &[f32]) -> Value {
    Value::Array(v.iter().map(|&x| crate::js::jnum(f64::from(x))).collect())
}
fn u32v(v: &[u32]) -> Value {
    Value::Array(v.iter().map(|&x| Value::from(x)).collect())
}
impl J for Mesh {
    fn j(&self) -> Value {
        let mut m = Map::new();
        m.insert("key".into(), Value::String(self.key.clone()));
        m.insert("name".into(), Value::String(self.name.clone()));
        m.insert("positions".into(), f32v(&self.positions));
        m.insert("normals".into(), f32v(&self.normals));
        m.insert("colors".into(), f32v(&self.colors));
        m.insert("indices".into(), u32v(&self.indices));
        if let Some(u) = &self.uvs {
            m.insert("uvs".into(), f32v(u));
        }
        if let Some(o) = &self.owners {
            m.insert("owners".into(), o.j());
        }
        if let Some(t) = &self.tri_owner {
            m.insert("triOwner".into(), u32v(t));
        }
        Value::Object(m)
    }
}
impl J for Line {
    fn j(&self) -> Value {
        let mut m = Map::new();
        m.insert("key".into(), Value::String(self.key.clone()));
        m.insert("name".into(), Value::String(self.name.clone()));
        m.insert("color".into(), Value::String(self.color.clone()));
        m.insert("positions".into(), f32v(&self.positions));
        if let Some(o) = &self.owners {
            m.insert("owners".into(), o.j());
        }
        if let Some(t) = &self.seg_owner {
            m.insert("segOwner".into(), u32v(t));
        }
        Value::Object(m)
    }
}
impl J for Scene {
    fn j(&self) -> Value {
        let labels: Vec<Value> = self
            .labels
            .iter()
            .map(|l| {
                let mut m = Map::new();
                m.insert("text".into(), Value::String(l.text.clone()));
                m.insert("at".into(), Value::Array(l.at.iter().map(|&x| crate::js::jnum(x)).collect()));
                m.insert("kind".into(), Value::String(l.kind.into()));
                if let Some(p) = l.pri {
                    m.insert("pri".into(), crate::js::jnum(p));
                }
                m.insert("color".into(), Value::String(l.color.clone()));
                Value::Object(m)
            })
            .collect();
        let c = &self.counts;
        crate::jo! {
            "units" => self.units,
            "origin" => crate::jo! { "x" => self.origin.x, "y" => self.origin.y, "z" => self.origin.z },
            "exag" => self.exag,
            "meshes" => self.meshes,
            "lines" => self.lines,
            "points" => self.points.as_ref().map(|p| crate::jo! { "positions" => f32v(&p.positions), "ids" => p.ids, "color" => p.color }),
            "notes" => self.notes,
            "labels" => Value::Array(labels),
            "counts" => crate::jo! { "triangles" => c.triangles, "segments" => c.segments, "points" => c.points, "pipes" => c.pipes, "structures" => c.structures },
            "bbox" => self.bbox.map(|(a, b)| crate::jo! { "min" => a.to_vec(), "max" => b.to_vec() }),
        }
    }
}

/// `buildSite3D` options.
#[derive(Clone, Debug, Default)]
pub struct Opts {
    pub exag: Option<Value>,
    /// 'bands' | 'single' | 'map'
    pub color: Option<String>,
    pub band_count: Option<Value>,
    pub dark: bool,
    pub colors: Vec<(String, String)>,
    pub show: Show,
    pub drape: Option<[f64; 4]>,
}
impl Opts {
    pub fn read(v: Option<&Value>) -> Opts {
        let g = |k: &str| v.and_then(|o| o.get(k));
        Opts {
            exag: g("exag").cloned(),
            color: g("color").map(crate::js::to_js_string),
            band_count: g("bandCount").filter(|x| !x.is_null()).cloned(),
            dark: crate::js::truthy(g("dark")),
            colors: g("colors")
                .and_then(Value::as_object)
                .map(|m| m.iter().map(|(k, x)| (k.clone(), crate::js::to_js_string(x))).collect())
                .unwrap_or_default(),
            show: Show::read(g("show")),
            drape: g("drape")
                .filter(|d| crate::js::truthy(Some(d)))
                .map(|d| [numf(d.get("minX")), numf(d.get("minY")), numf(d.get("maxX")), numf(d.get("maxY"))]),
        }
    }
}

struct Colors(Vec<(String, String)>);
impl Colors {
    fn get(&self, k: &str) -> Option<&str> {
        self.0.iter().rev().find(|(n, _)| n == k).map(|(_, c)| c.as_str())
    }
    fn c(&self, k: &str) -> String {
        self.get(k).unwrap_or("undefined").to_string()
    }
}

/// `owned()`: segments plus who owns each.
struct Owned {
    seg: Vec<f64>,
    owners: Vec<Owner>,
    so: Vec<u32>,
}
impl Owned {
    fn new() -> Owned {
        Owned { seg: Vec::new(), owners: Vec::new(), so: Vec::new() }
    }
    fn add(&mut self, owner: Owner, f: impl FnOnce(&mut Vec<f64>)) {
        let n0 = self.seg.len();
        f(&mut self.seg);
        self.owners.push(owner);
        let k = u32::try_from(self.owners.len().saturating_sub(1)).unwrap_or(u32::MAX);
        let mut i = n0;
        while i < self.seg.len() {
            self.so.push(k);
            i = i.saturating_add(6);
        }
    }
    fn done(self, key: &str, name: String, color: String) -> Line {
        Line { key: key.into(), name, color, positions: f32s(&self.seg), owners: Some(self.owners), seg_owner: Some(self.so) }
    }
}

fn poly_segs(out: &mut Vec<f64>, pts: &[[f64; 3]], closed: bool) {
    for w in pts.windows(2) {
        if let [a, b] = w {
            out.extend_from_slice(a);
            out.extend_from_slice(b);
        }
    }
    if closed
        && pts.len() > 2
        && let (Some(l), Some(f)) = (pts.last(), pts.first())
    {
        out.extend_from_slice(l);
        out.extend_from_slice(f);
    }
}

fn js_text(v: Option<&Value>) -> String {
    v.map_or_else(|| "undefined".to_string(), crate::js::to_js_string)
}

/// `buildSite3D(input, opts)`.
pub fn build_site3d(input: &SiteInput, opts: &Opts) -> Scene {
    let show = opts.show;
    let ex = crate::js::number(opts.exag.as_ref());
    let exag = crate::js::min(10.0, crate::js::max(1.0, if ex != 0.0 && !ex.is_nan() { ex } else { 1.0 }));
    let mut cols: Vec<(String, String)> = DEFAULT_COLORS.iter().map(|(k, c)| ((*k).to_string(), (*c).to_string())).collect();
    cols.extend(opts.colors.iter().cloned());
    let cc = Colors(cols);
    let units = if input.units == "m" { "m" } else { "ft" };
    let mut notes: Vec<String> = Vec::new();
    let origin = site_origin(input);
    let ll = |x: f64, y: f64, z: f64| [x - origin.x, y - origin.y, (z - origin.z) * exag];
    let mut scene = Scene { units: units.into(), origin, exag, ..Scene::default() };
    let mut labels: Vec<Label> = Vec::new();
    let mut label = |kind: &'static str, text: String, x: f64, y: f64, z: Option<f64>, color: String| {
        if let Some(z) = z
            && show.labels
            && labels.len() < MAX_LABELS
            && z.is_finite()
        {
            labels.push(Label { text, at: ll(x, y, z), kind, pri: label_pri(kind), color });
        }
    };
    let map = opts.color.as_deref() == Some("map") && opts.drape.is_some();

    // surfaces
    let mut surfs: Vec<(&str, String, &Surface, String, &[Contour])> = Vec::new();
    if show.eg
        && let Some(s) = input.eg()
    {
        surfs.push(("eg", tr!("surface.eg"), s, cc.c("surface"), input.contours_eg.as_slice()));
    }
    if show.fg
        && let Some(s) = input.fg()
    {
        surfs.push(("fg", tr!("surface.fg"), s, cc.c("graded"), input.contours_fg.as_slice()));
    }
    for (key, name, s, color, contours) in surfs {
        let st = s.stats();
        let bc = match &opts.band_count {
            Some(v) => crate::js::number(Some(v)),
            None => input.band_count,
        };
        let n = crate::js::max(2.0, crate::js::min(64.0, if bc != 0.0 && !bc.is_nan() { bc } else { 8.0 }));
        let bands = if opts.color.as_deref() != Some("single") && !map {
            let mut colors = Vec::new();
            let mut i = 0.0;
            while i < n {
                colors.push(rgb(&band_color(i / crate::js::max(1.0, n - 1.0), opts.dark)));
                i += 1.0;
            }
            Some((band_levels(st.min_z, st.max_z, n), colors))
        } else {
            None
        };
        let mut m = surface_mesh(s, origin, exag, bands.as_ref(), if map { [1.0, 1.0, 1.0] } else { rgb(&color) });
        if let (true, Some(d)) = (map, opts.drape) {
            m.uvs = Some(drape_uvs(&m.positions, origin, d));
        }
        m.key = key.into();
        m.name = name.clone();
        scene.counts.triangles += m.indices.len() as f64 / 3.0;
        scene.meshes.push(m);
        if show.triangles {
            scene.lines.push(Line {
                key: format!("{key}-tin"),
                name: format!("{name} triangles"),
                color: cc.c("tin"),
                positions: surface_edges(s, origin, exag, false),
                ..Line::default()
            });
        }
        if show.boundary {
            scene.lines.push(Line {
                key: format!("{key}-edge"),
                name: format!("{name} edge"),
                color: cc.c("edge"),
                positions: surface_edges(s, origin, exag, true),
                ..Line::default()
            });
        }
        if show.contours {
            let iv = if input.interval != 0.0 && !input.interval.is_nan() { input.interval } else { 1.0 };
            let mj = if input.major != 0.0 && !input.major.is_nan() { input.major } else { 5.0 };
            let step = iv * mj;
            let is_maj = |z: f64| step > 0.0 && (z / step - crate::js::round(z / step)).abs() < 1e-6;
            let mut maj = Vec::new();
            let mut min = Vec::new();
            for c in contours {
                let pts: Vec<[f64; 3]> = c.pts.iter().map(|p| ll(p.x, p.y, c.z)).collect();
                poly_segs(if is_maj(c.z) { &mut maj } else { &mut min }, &pts, c.closed);
                if is_maj(c.z)
                    && c.pts.len() >= 4
                    && let Some(m) = c.pts.get(c.pts.len() / 2)
                {
                    label("contour", fixed(c.z, if step < 1.0 { 1 } else { 0 }).replace(',', ""), m.x, m.y, Some(c.z), cc.c("major"));
                }
            }
            if !min.is_empty() {
                scene.lines.push(Line {
                    key: format!("{key}-minor"),
                    name: tr!("s3.contours.minor", surface = name.as_str()),
                    color: cc.c("minor"),
                    positions: f32s(&min),
                    ..Line::default()
                });
            }
            if !maj.is_empty() {
                scene.lines.push(Line {
                    key: format!("{key}-major"),
                    name: tr!("s3.contours.major", surface = name.as_str()),
                    color: cc.c("major"),
                    positions: f32s(&maj),
                    ..Line::default()
                });
            }
        }
    }

    let fin3 = |pts: &[P3]| -> Vec<[f64; 3]> { pts.iter().filter(|p| p.z.is_finite()).map(|p| ll(p.x, p.y, p.z)).collect() };
    // breaklines
    if show.breaklines && !input.breaklines.is_empty() {
        let mut o = Owned::new();
        for b in &input.breaklines {
            o.add(b.owner.clone(), |seg| poly_segs(seg, &fin3(&b.pts), false));
        }
        if !o.seg.is_empty() {
            scene.lines.push(o.done("breaklines", tr!("scene.layer.brkl"), cc.c("breakline")));
        }
    }
    // feature lines
    if show.breaklines && !input.features.is_empty() {
        let mut o = Owned::new();
        for f in &input.features {
            o.add(Owner { kind: "feature", id: f.id.clone(), name: f.name.clone() }, |seg| poly_segs(seg, &fin3(&f.pts), false));
            let mi = f.pts.len().saturating_sub(1) / 2;
            if let Some(m) = f.pts.get(mi) {
                label("feature", js_text(f.name.as_ref()), m.x, m.y, Some(m.z), cc.c("gis:green"));
            }
        }
        if !o.seg.is_empty() {
            scene.lines.push(o.done("features", tr!("scene.layer.feat"), cc.c("gis:green")));
        }
    }
    // alignments on the design profile (else on the existing ground), with station ticks
    if show.alignments {
        let mut o = Owned::new();
        let mut tick: Vec<f64> = Vec::new();
        let eg = input.eg();
        for a in &input.alignments {
            let Some(al) = a.alignment.as_deref().filter(|al| al.ok()) else { continue };
            let prof = a.profile.as_deref().filter(|p| p.ok());
            let z_at = |sta: f64, x: f64, y: f64| -> Option<f64> {
                let z = prof.and_then(|p| p.elevation_at(sta));
                z.or_else(|| eg.and_then(|s| s.elevation_at(x, y, 1e-6)))
            };
            o.add(Owner { kind: "alignment", id: a.id.clone(), name: a.name.clone() }, |seg| {
                let step = crate::js::max(al.length / 400.0, 0.5);
                let mut stas: Vec<f64> = al.polyline(step, std::f64::consts::PI / 360.0).iter().map(|p| p.2).collect();
                let mut s = al.start;
                while s < al.end {
                    stas.push(s);
                    s += step;
                }
                stas.push(al.end);
                if let Some(p) = prof {
                    for c in &p.curves {
                        for k in 0..=16 {
                            let s = c.bvc + (c.l * f64::from(k)) / 16.0;
                            if s >= al.start && s <= al.end {
                                stas.push(s);
                            }
                        }
                    }
                }
                // a Set, then sorted: equal stations once
                let mut seen: HashSet<u64> = HashSet::new();
                let mut uniq: Vec<f64> = stas.into_iter().filter(|s| seen.insert(if *s == 0.0 { 0 } else { s.to_bits() })).collect();
                crate::js::sort_by(&mut uniq, |p, q| p - q);
                let mut run: Vec<[f64; 3]> = Vec::new();
                for sta in uniq {
                    let p = al.point_at(sta);
                    let z = p.as_ref().and_then(|p| z_at(sta, p.x, p.y));
                    match (p, z) {
                        (Some(p), Some(z)) => run.push(ll(p.x, p.y, z)),
                        _ => {
                            poly_segs(seg, &run, false);
                            run.clear();
                        }
                    }
                }
                poly_segs(seg, &run, false);
            });
            if let Some(p0) = al.point_at(al.start) {
                label("alignment", js_text(a.name.as_ref()), p0.x, p0.y, z_at(al.start, p0.x, p0.y), cc.c("alignment"));
            }
            let minor = if units == "m" { 20.0 } else { 50.0 };
            let major = 100.0;
            for (sta, t) in al.ticks(minor) {
                let Some(z) = z_at(sta, t.x, t.y) else { continue };
                let is_maj = (sta / major - crate::js::round(sta / major)).abs() < 1e-9;
                let k = (if units == "m" { 1.5 } else { 5.0 }) * (if is_maj { 1.6 } else { 1.0 });
                let nx = t.dir.jsin();
                let ny = -t.dir.jcos();
                tick.extend_from_slice(&ll(t.x - nx * k, t.y - ny * k, z));
                tick.extend_from_slice(&ll(t.x + nx * k, t.y + ny * k, z));
                if is_maj {
                    label("station", format_station(sta, units, Some(0)), t.x, t.y, Some(z), cc.c("alignment"));
                }
            }
        }
        if !o.seg.is_empty() {
            scene.lines.push(o.done("alignments", tr!("s3.alignments"), cc.c("alignment")));
        }
        if !tick.is_empty() {
            scene.lines.push(Line {
                key: "stations".into(),
                name: tr!("s3.stations"),
                color: cc.c("alignment"),
                positions: f32s(&tick),
                ..Line::default()
            });
        }
    }
    // corridor feature lines
    if show.corridors {
        let mut seg = Vec::new();
        for c in &input.corridors {
            for (name, pts) in c {
                if name == "CL" {
                    continue;
                }
                poly_segs(&mut seg, &fin3(pts), false);
            }
        }
        if !seg.is_empty() {
            scene.lines.push(Line {
                key: "corridors".into(),
                name: tr!("s3.corridors"),
                color: cc.c("corridor"),
                positions: f32s(&seg),
                ..Line::default()
            });
        }
    }
    // pads and their daylight lines
    if show.pads {
        let mut pad = Owned::new();
        let mut day = Owned::new();
        for p in &input.pads {
            if !p.z.is_finite() || p.pts.len() < 3 {
                continue;
            }
            let ow = Owner { kind: "pad", id: p.id.clone(), name: p.name.clone() };
            let pts: Vec<[f64; 3]> = p.pts.iter().map(|q| ll(q.x, q.y, p.z)).collect();
            pad.add(ow.clone(), |seg| poly_segs(seg, &pts, true));
            let n = p.pts.len() as f64;
            let cx = p.pts.iter().fold(0.0, |a, q| a + q.x) / n;
            let cy = p.pts.iter().fold(0.0, |a, q| a + q.y) / n;
            label("pad", js_text(p.name.as_ref()), cx, cy, Some(p.z), cc.c("pad"));
            day.add(ow, |seg| poly_segs(seg, &fin3(&p.daylight), true));
        }
        if !pad.seg.is_empty() {
            scene.lines.push(pad.done("pads", tr!("scene.layer.pads"), cc.c("pad")));
        }
        if !day.seg.is_empty() {
            scene.lines.push(day.done("daylight", tr!("scene.layer.dayl"), cc.c("daylight")));
        }
    }
    // pipe network
    if show.pipes
        && let Some(net) = &input.network
    {
        let chk = input.check.as_ref();
        let per = crate::pipes::per_unit(units);
        let bad: HashSet<&str> =
            chk.map(|c| c.problems.iter().filter_map(|p| p.id.as_deref()).filter(|s| !s.is_empty()).collect()).unwrap_or_default();
        let mut by_id: HashMap<&str, &crate::pipes::Structure> = HashMap::new();
        for s in &net.structures {
            by_id.insert(s.id.as_str(), s);
        }
        let mut pipe_b = MeshB::default();
        let mut str_b = MeshB::default();
        let mut no_inv = 0usize;
        let mut no_rim = 0usize;
        for p in &net.pipes {
            let (Some(a), Some(b)) = (by_id.get(p.from.as_str()), by_id.get(p.to.as_str())) else { continue };
            let (Some(iu), Some(id)) = (p.inv_up.filter(|v| v.is_finite()), p.inv_dn.filter(|v| v.is_finite())) else {
                no_inv = no_inv.saturating_add(1);
                continue;
            };
            let dia = if p.dia.is_nan() { 0.0 } else { p.dia };
            let dd = dia / per;
            if !(dd > 0.0) {
                continue;
            }
            let r = dd / 2.0;
            pipe_b.owners.push(Owner { kind: "pipe", id: Some(Value::String(p.id.clone())), name: Some(Value::String(p.name.clone())) });
            let col = rgb(&cc.c(if bad.contains(p.id.as_str()) { "problem" } else { "pipe" }));
            cylinder(&mut pipe_b, [a.x, a.y, iu + r], [b.x, b.y, id + r], r, 16, false, &ll, exag, col);
            scene.counts.pipes += 1.0;
        }
        for s in &net.structures {
            let info = chk.and_then(|c| c.structs.iter().find(|(k, _)| *k == s.id)).map(|(_, v)| v);
            let rim = info.and_then(|i| i.rim).or(s.rim.filter(|r| r.is_finite()));
            let Some(rim) = rim.filter(|r| r.is_finite()) else {
                no_rim = no_rim.saturating_add(1);
                continue;
            };
            let mut bottom = info.and_then(|i| i.sump).unwrap_or(f64::NAN);
            if !bottom.is_finite() {
                bottom = rim - if units == "m" { 1.5 } else { 5.0 };
            }
            let dia = if s.dia.is_nan() { 0.0 } else { s.dia };
            let r = dia / per / 2.0;
            if !(r > 0.0) || bottom >= rim {
                continue;
            }
            str_b.owners.push(Owner {
                kind: "structure",
                id: Some(Value::String(s.id.clone())),
                name: Some(Value::String(s.name.clone())),
            });
            label("structure", s.name.clone(), s.x, s.y, Some(rim), cc.c("structure"));
            let col = rgb(&cc.c(if bad.contains(s.id.as_str()) { "problem" } else { "structure" }));
            cylinder(&mut str_b, [s.x, s.y, bottom], [s.x, s.y, rim], r, 24, true, &ll, exag, col);
            scene.counts.structures += 1.0;
        }
        if no_inv > 0 {
            notes.push(tr!("s3.noinv", n = no_inv));
        }
        if no_rim > 0 {
            notes.push(tr!("s3.norim", n = no_rim));
        }
        if !pipe_b.owners.is_empty() {
            scene.meshes.push(pipe_b.done("pipes", tr!("scene.layer.pipe")));
        }
        if !str_b.owners.is_empty() {
            scene.meshes.push(str_b.done("structures", tr!("scene.layer.strc")));
        }
    }
    // GIS layers: at their own z when every vertex has one, else draped on the existing ground; points as crosses
    if show.gis && !input.gis.is_empty() {
        let mut budget = MAX_GIS_SEGMENTS;
        let mut cut = false;
        let mut flat_off = 0usize;
        let eg = input.eg();
        let cross = eg.map_or(1.0, |s| {
            let b = s.bounds();
            crate::js::max(b.max_x - b.min_x, b.max_y - b.min_y) / 300.0
        });
        for l in &input.gis {
            let mut acc = GisAcc { budget, cut, flat_off, seg: Vec::new() };
            for f in &l.features {
                let Some(g) = &f.g else { continue };
                match g {
                    Geom::Ln(c) => {
                        for q in c {
                            acc.part(q, eg, &ll);
                        }
                    }
                    Geom::Pg(c) => {
                        for poly in c {
                            for r in poly {
                                acc.part(r, eg, &ll);
                            }
                        }
                    }
                    Geom::Pt(c) => {
                        let (x, y) = (c.g(0), c.g(1));
                        let z = if c.len() > 2 && c.g(2).is_finite() { Some(c.g(2)) } else { eg.and_then(|s| s.elevation_at(x, y, 1e-6)) };
                        let Some(z) = z else {
                            acc.flat_off = acc.flat_off.saturating_add(1);
                            continue;
                        };
                        acc.add([x - cross, y, z], [x + cross, y, z], &ll);
                        acc.add([x, y - cross, z], [x, y + cross, z], &ll);
                    }
                }
                if acc.cut {
                    break;
                }
            }
            budget = acc.budget;
            cut = acc.cut;
            flat_off = acc.flat_off;
            let seg = acc.seg;
            if !seg.is_empty() {
                let color = cc.get(&format!("gis:{}", l.color)).map_or_else(|| cc.c("gis:blue"), str::to_string);
                scene.lines.push(Line {
                    key: format!("gis-{}", l.id),
                    name: l.name.clone(),
                    color,
                    positions: f32s(&seg),
                    ..Line::default()
                });
            }
            if cut {
                break;
            }
        }
        if cut {
            notes.push(tr!("v3d.gis.cut", n = "300,000"));
        }
        if flat_off > 0 {
            notes.push(tr!("v3d.gis.off", n = flat_off));
        }
    }
    // survey points
    if show.points && !input.points.is_empty() {
        let mut pos = Vec::new();
        let mut ids = Vec::new();
        for p in &input.points {
            let Some(z) = p.z.filter(|z| z.is_finite()) else { continue };
            pos.extend_from_slice(&ll(p.x, p.y, z));
            ids.push(p.n.clone());
            label("point", tr!("s3.label.point", n = p.n.as_str(), z = fixed(z, 2).replace(',', "")), p.x, p.y, Some(z), cc.c("point"));
        }
        scene.counts.points = ids.len() as f64;
        if !ids.is_empty() {
            scene.points = Some(Points { positions: f32s(&pos), ids, color: cc.c("point") });
        }
    }
    for l in &scene.lines {
        scene.counts.segments += l.positions.len() as f64 / 6.0;
    }
    if show.labels && labels.len() >= MAX_LABELS {
        notes.push(tr!("s3.labels.cut", n = "6,000"));
    }
    scene.labels = labels;
    scene.notes = notes;
    scene.bbox = bbox_of(&scene);
    scene
}

/// `siteOrigin(input)`.
pub fn site_origin(input: &SiteInput) -> Origin {
    let (eg, fg) = (input.eg(), input.fg());
    if let Some(s) = eg.or(fg) {
        let st = s.stats();
        let other = match (eg, fg) {
            (Some(_), Some(f)) => f.stats().min_z,
            _ => f64::INFINITY,
        };
        return Origin { x: s.ox, y: s.oy, z: crate::js::min(st.min_z, other).floor() };
    }
    let first: Option<(f64, f64)> = input
        .points
        .iter()
        .find(|p| p.x.is_finite())
        .map(|p| (p.x, p.y))
        .or_else(|| input.network.as_ref().and_then(|n| n.structures.first()).map(|s| (s.x, s.y)))
        .or_else(|| input.alignments.first().and_then(|a| a.alignment.as_ref()).and_then(|a| a.pis.first()).map(|p| (p.x, p.y)));
    let mut z = f64::INFINITY;
    for p in &input.points {
        if let Some(pz) = p.z
            && pz.is_finite()
            && pz < z
        {
            z = pz;
        }
    }
    Origin { x: first.map_or(0.0, |f| f.0.floor()), y: first.map_or(0.0, |f| f.1.floor()), z: if z.is_finite() { z.floor() } else { 0.0 } }
}

fn bbox_of(scene: &Scene) -> Option<([f64; 3], [f64; 3])> {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    let mut add = |a: &[f32]| {
        for c in a.as_chunks::<3>().0 {
            for (k, v) in c.iter().enumerate() {
                let v = f64::from(*v);
                if let Some(m) = min.get_mut(k)
                    && v < *m
                {
                    *m = v;
                }
                if let Some(m) = max.get_mut(k)
                    && v > *m
                {
                    *m = v;
                }
            }
        }
    };
    for m in &scene.meshes {
        add(&m.positions);
    }
    for l in &scene.lines {
        add(&l.positions);
    }
    if let Some(p) = &scene.points {
        add(&p.positions);
    }
    if min[0] == f64::INFINITY { None } else { Some((min, max)) }
}

/// The GIS line budget and output of one layer (`add` / `part` in buildSite3D).
struct GisAcc {
    budget: i64,
    cut: bool,
    flat_off: usize,
    seg: Vec<f64>,
}
impl GisAcc {
    fn add(&mut self, a: [f64; 3], b: [f64; 3], ll: &dyn Fn(f64, f64, f64) -> [f64; 3]) {
        if self.budget <= 0 {
            self.cut = true;
            return;
        }
        self.budget = self.budget.saturating_sub(1);
        self.seg.extend_from_slice(&ll(a[0], a[1], a[2]));
        self.seg.extend_from_slice(&ll(b[0], b[1], b[2]));
    }
    fn part(&mut self, pts: &[Vec<f64>], eg: Option<&Surface>, ll: &dyn Fn(f64, f64, f64) -> [f64; 3]) {
        if pts.len() < 2 || self.cut {
            return;
        }
        let p3 = |q: &Vec<f64>| [q.g(0), q.g(1), q.g(2)];
        if pts.iter().all(|q| q.len() > 2 && q.g(2).is_finite()) {
            for w in pts.windows(2) {
                if let [a, b] = w {
                    self.add(p3(a), p3(b), ll);
                }
            }
            return;
        }
        let Some(eg) = eg else {
            self.flat_off = self.flat_off.saturating_add(1);
            return;
        };
        let mut i = 1;
        while i < pts.len() && !self.cut {
            let (Some(qa), Some(qb)) = (pts.get(i - 1), pts.get(i)) else { break };
            let a = P2 { x: qa.g(0), y: qa.g(1) };
            let b = P2 { x: qb.g(0), y: qb.g(1) };
            let len = crate::js::hypot(b.x - a.x, b.y - a.y);
            for run in crate::sections::surface_section(Some(eg), a, b, 0.0, len) {
                for w in run.windows(2) {
                    if let [p0, p1] = w {
                        let f0 = if len != 0.0 { p0.o / len } else { 0.0 };
                        let f1 = if len != 0.0 { p1.o / len } else { 0.0 };
                        self.add(
                            [a.x + (b.x - a.x) * f0, a.y + (b.y - a.y) * f0, p0.z],
                            [a.x + (b.x - a.x) * f1, a.y + (b.y - a.y) * f1, p1.z],
                            ll,
                        );
                    }
                }
            }
            i += 1;
        }
    }
}

// ---------------------------------------------------------------------------- surfaces

/// `drapeUVs(positions, O, box)`: u east, v north over the box (project coordinates).
pub fn drape_uvs(positions: &[f32], o: Origin, b: [f64; 4]) -> Vec<f32> {
    let w = crate::js::max(b[2] - b[0], 1e-9);
    let h = crate::js::max(b[3] - b[1], 1e-9);
    let mut out = Vec::with_capacity(positions.len() / 3 * 2);
    for c in positions.as_chunks::<3>().0 {
        out.push(to_f32((gf(c, 0) + o.x - b[0]) / w));
        out.push(to_f32((gf(c, 1) + o.y - b[1]) / h));
    }
    out
}

type Bands = (Vec<f64>, Vec<[f64; 3]>);

/// `surfaceMesh(s, O, exag, bands, single)`: smooth vertex normals (in exaggerated space); with bands each triangle is
/// cut at the band levels into pieces of one colour.
pub fn surface_mesh(s: &Surface, o: Origin, exag: f64, bands: Option<&Bands>, single: [f64; 3]) -> Mesh {
    let (xs, ys, zs, tris) = (&s.x, &s.y, &s.z, &s.tris);
    let n_v = xs.len();
    let dx = s.ox - o.x;
    let dy = s.oy - o.y;
    let mut nn = vec![0.0f64; n_v.saturating_mul(3)];
    let idx = |t: usize| tris.g(t) as usize;
    let mut t = 0;
    while t + 2 < tris.len() {
        let (a, b, c) = (idx(t), idx(t + 1), idx(t + 2));
        let ux = xs.g(b) - xs.g(a);
        let uy = ys.g(b) - ys.g(a);
        let uz = (zs.g(b) - zs.g(a)) * exag;
        let vx = xs.g(c) - xs.g(a);
        let vy = ys.g(c) - ys.g(a);
        let vz = (zs.g(c) - zs.g(a)) * exag;
        let mut nx = uy * vz - uz * vy;
        let mut ny = uz * vx - ux * vz;
        let mut nz = ux * vy - uy * vx;
        if nz < 0.0 {
            nx = -nx;
            ny = -ny;
            nz = -nz;
        }
        for v in [a, b, c] {
            let k = v.saturating_mul(3);
            if let Some(q) = nn.get_mut(k) {
                *q += nx;
            }
            if let Some(q) = nn.get_mut(k + 1) {
                *q += ny;
            }
            if let Some(q) = nn.get_mut(k + 2) {
                *q += nz;
            }
        }
        t += 3;
    }
    for v in 0..n_v {
        let k = v * 3;
        let l0 = hypot3(nn.g(k), nn.g(k + 1), nn.g(k + 2));
        let l = if l0 != 0.0 && !l0.is_nan() { l0 } else { 1.0 };
        for j in 0..3 {
            if let Some(q) = nn.get_mut(k + j) {
                *q /= l;
            }
        }
        if l == 1.0 && nn.g(k + 2) == 0.0 {
            nn.s(k + 2, 1.0);
        }
    }
    let mut pos: Vec<f64> = Vec::new();
    let mut nor: Vec<f64> = Vec::new();
    let mut col: Vec<f64> = Vec::new();
    let mut ix: Vec<u32> = Vec::new();
    let up_ccw = |pos: &[f64], i0: u32, i1: u32, i2: u32| {
        let (i0, i1, i2) = (i0 as usize * 3, i1 as usize * 3, i2 as usize * 3);
        let ux = pos.g(i1) - pos.g(i0);
        let uy = pos.g(i1 + 1) - pos.g(i0 + 1);
        let vx = pos.g(i2) - pos.g(i0);
        let vy = pos.g(i2 + 1) - pos.g(i0 + 1);
        ux * vy - uy * vx >= 0.0
    };
    let tri = |pos: &[f64], ix: &mut Vec<u32>, i0: u32, i1: u32, i2: u32| {
        if up_ccw(pos, i0, i1, i2) {
            ix.extend_from_slice(&[i0, i1, i2]);
        } else {
            ix.extend_from_slice(&[i0, i2, i1]);
        }
    };
    let push = |pos: &mut Vec<f64>, nor: &mut Vec<f64>, col: &mut Vec<f64>, q: [f64; 6], c: [f64; 3]| -> u32 {
        pos.extend_from_slice(&[q[0] + dx, q[1] + dy, (q[2] - o.z) * exag]);
        nor.extend_from_slice(&[q[3], q[4], q[5]]);
        col.extend_from_slice(&c);
        u32::try_from(pos.len() / 3 - 1).unwrap_or(u32::MAX)
    };
    let vert = |v: usize| [xs.g(v), ys.g(v), zs.g(v), nn.g(v * 3), nn.g(v * 3 + 1), nn.g(v * 3 + 2)];
    match bands {
        None => {
            let mut map = vec![-1i64; n_v];
            let mg = |map: &[i64], v: usize| map.get(v).copied().unwrap_or(-1);
            let mut t = 0;
            while t + 2 < tris.len() {
                let mut ids = [0u32; 3];
                for (k, id) in ids.iter_mut().enumerate() {
                    let v = idx(t + k);
                    if mg(&map, v) < 0 {
                        let i = push(&mut pos, &mut nor, &mut col, vert(v), single);
                        if let Some(m) = map.get_mut(v) {
                            *m = i64::from(i);
                        }
                    }
                    *id = u32::try_from(mg(&map, v)).unwrap_or(0);
                }
                tri(&pos, &mut ix, ids[0], ids[1], ids[2]);
                t += 3;
            }
        }
        Some((levels, colors)) => {
            let band = |z: f64| {
                let mut b = 0usize;
                while b < levels.len() && z > levels.g(b) {
                    b += 1;
                }
                b
            };
            let colr = |b: usize| colors.get(b).copied().unwrap_or([f64::NAN; 3]);
            let mut shared: HashMap<usize, u32> = HashMap::new();
            let mut t = 0;
            while t + 2 < tris.len() {
                let vs = [idx(t), idx(t + 1), idx(t + 2)];
                let bs = vs.map(|v| band(zs.g(v)));
                let lo = bs.iter().copied().min().unwrap_or(0);
                let hi = bs.iter().copied().max().unwrap_or(0);
                if lo == hi {
                    let mut ids = [0u32; 3];
                    for (id, &v) in ids.iter_mut().zip(vs.iter()) {
                        let key = v.saturating_mul(128).saturating_add(lo);
                        let i = match shared.get(&key) {
                            Some(i) => *i,
                            None => {
                                let i = push(&mut pos, &mut nor, &mut col, vert(v), colr(lo));
                                shared.insert(key, i);
                                i
                            }
                        };
                        *id = i;
                    }
                    tri(&pos, &mut ix, ids[0], ids[1], ids[2]);
                    t += 3;
                    continue;
                }
                let poly0: Vec<[f64; 6]> = vs.iter().map(|&v| vert(v)).collect();
                for b in lo..=hi {
                    let mut poly = poly0.clone();
                    if b > 0 {
                        poly = clip_z(&poly, levels.g(b - 1), 1.0);
                    }
                    if b < levels.len() {
                        poly = clip_z(&poly, levels.g(b), -1.0);
                    }
                    if poly.len() < 3 {
                        continue;
                    }
                    let ids: Vec<u32> = poly
                        .iter()
                        .map(|q| {
                            let l0 = hypot3(q[3], q[4], q[5]);
                            let l = if l0 != 0.0 && !l0.is_nan() { l0 } else { 1.0 };
                            push(&mut pos, &mut nor, &mut col, [q[0], q[1], q[2], q[3] / l, q[4] / l, q[5] / l], colr(b))
                        })
                        .collect();
                    for k in 1..ids.len().saturating_sub(1) {
                        tri(&pos, &mut ix, gu(&ids, 0), gu(&ids, k), gu(&ids, k + 1));
                    }
                }
                t += 3;
            }
        }
    }
    Mesh { positions: f32s(&pos), normals: f32s(&nor), colors: f32s(&col), indices: ix, ..Mesh::default() }
}

/// `clipZ(poly, level, side)`: the part with z ≥ level (side 1) or z ≤ level (side −1), attributes interpolated.
fn clip_z(poly: &[[f64; 6]], level: f64, side: f64) -> Vec<[f64; 6]> {
    let mut out = Vec::new();
    let n = poly.len();
    for i in 0..n {
        let (Some(a), Some(b)) = (poly.get(i), poly.get((i + 1) % n)) else { continue };
        let da = (a[2] - level) * side;
        let db = (b[2] - level) * side;
        if da >= 0.0 {
            out.push(*a);
        }
        if (da > 0.0 && db < 0.0) || (da < 0.0 && db > 0.0) {
            let t = da / (da - db);
            let mut p = [0.0; 6];
            for ((v, x), y) in p.iter_mut().zip(a.iter()).zip(b.iter()) {
                *v = x + (y - x) * t;
            }
            p[2] = level;
            out.push(p);
        }
    }
    out
}

/// `surfaceEdges(s, O, exag, outerOnly)`: every triangle edge, or the outer edge (edges with one triangle).
fn surface_edges(s: &Surface, o: Origin, exag: f64, outer_only: bool) -> Vec<f32> {
    let (xs, ys, zs, tris) = (&s.x, &s.y, &s.z, &s.tris);
    let dx = s.ox - o.x;
    let dy = s.oy - o.y;
    let mut out: Vec<f64> = Vec::new();
    let mut seg = |a: usize, b: usize| {
        out.extend_from_slice(&[xs.g(a) + dx, ys.g(a) + dy, (zs.g(a) - o.z) * exag, xs.g(b) + dx, ys.g(b) + dy, (zs.g(b) - o.z) * exag]);
    };
    if !outer_only {
        let e = s.edges();
        for p in e.as_chunks::<2>().0 {
            seg(gu(p, 0) as usize, gu(p, 1) as usize);
        }
    } else {
        let key = |a: u32, b: u32| (u64::from(a) << 32) | u64::from(b);
        let mut dir: HashSet<u64> = HashSet::new();
        let mut t = 0;
        while t + 2 < tris.len() {
            for k in 0..3 {
                dir.insert(key(tris.g(t + k), tris.g(t + (k + 1) % 3)));
            }
            t += 3;
        }
        let mut t = 0;
        while t + 2 < tris.len() {
            for k in 0..3 {
                let a = tris.g(t + k);
                let b = tris.g(t + (k + 1) % 3);
                if !dir.contains(&key(b, a)) {
                    seg(a as usize, b as usize);
                }
            }
            t += 3;
        }
    }
    f32s(&out)
}

// ---------------------------------------------------------------------------- pipes and structures

#[derive(Default)]
struct MeshB {
    pos: Vec<f64>,
    nor: Vec<f64>,
    col: Vec<f64>,
    idx: Vec<u32>,
    tri_owner: Vec<u32>,
    owners: Vec<Owner>,
}
impl MeshB {
    fn tri(&mut self, a: u32, b: u32, c: u32) {
        self.idx.extend_from_slice(&[a, b, c]);
        self.tri_owner.push(u32::try_from(self.owners.len().saturating_sub(1)).unwrap_or(0));
    }
    fn vert(&mut self, p: [f64; 3], n: [f64; 3], c: [f64; 3]) -> u32 {
        self.pos.extend_from_slice(&p);
        self.nor.extend_from_slice(&n);
        self.col.extend_from_slice(&c);
        u32::try_from(self.pos.len() / 3 - 1).unwrap_or(u32::MAX)
    }
    fn done(self, key: &str, name: String) -> Mesh {
        Mesh {
            key: key.into(),
            name,
            positions: f32s(&self.pos),
            normals: f32s(&self.nor),
            colors: f32s(&self.col),
            indices: self.idx,
            uvs: None,
            owners: Some(self.owners),
            tri_owner: Some(self.tri_owner),
        }
    }
}

/// `cylinder(B, a, b, r, sides, caps, L, exag, color)`: radius r around a → b (true scale), mapped by L.
#[allow(clippy::too_many_arguments)]
fn cylinder(
    bld: &mut MeshB,
    a: [f64; 3],
    b: [f64; 3],
    r: f64,
    sides: usize,
    caps: bool,
    ll: &dyn Fn(f64, f64, f64) -> [f64; 3],
    exag: f64,
    color: [f64; 3],
) {
    let ax = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let len = hypot3(ax[0], ax[1], ax[2]);
    if len <= 0.0 {
        return;
    }
    let w = ax.map(|v| v / len);
    let u0 = if w[2].abs() < 0.9 { [-w[1], w[0], 0.0] } else { [1.0, 0.0, 0.0] };
    let ul = hypot3(u0[0], u0[1], u0[2]);
    let u = u0.map(|v| v / ul);
    let v = [w[1] * u[2] - w[2] * u[1], w[2] * u[0] - w[0] * u[2], w[0] * u[1] - w[1] * u[0]];
    let nrm = |n: [f64; 3]| {
        let m = [n[0], n[1], n[2] / exag];
        let l0 = hypot3(m[0], m[1], m[2]);
        let l = if l0 != 0.0 && !l0.is_nan() { l0 } else { 1.0 };
        m.map(|x| x / l)
    };
    let mut ring: Vec<[u32; 2]> = Vec::with_capacity(sides);
    let sf = sides as f64;
    for k in 0..sides {
        let t = (2.0 * std::f64::consts::PI * k as f64) / sf;
        let (c, s) = (t.jcos(), t.jsin());
        let n = [u[0] * c + v[0] * s, u[1] * c + v[1] * s, u[2] * c + v[2] * s];
        let p0 = ll(a[0] + n[0] * r, a[1] + n[1] * r, a[2] + n[2] * r);
        let p1 = ll(b[0] + n[0] * r, b[1] + n[1] * r, b[2] + n[2] * r);
        let nn = nrm(n);
        let i0 = bld.vert(p0, nn, color);
        let i1 = bld.vert(p1, nn, color);
        ring.push([i0, i1]);
    }
    for k in 0..sides {
        let (Some(&[a0, a1]), Some(&[b0, b1])) = (ring.get(k), ring.get((k + 1) % sides)) else { continue };
        bld.tri(a0, b0, b1);
        bld.tri(a0, b1, a1);
    }
    if caps {
        for (end, sgn, j) in [(a, -1.0, 0usize), (b, 1.0, 1usize)] {
            let n = nrm(w.map(|x| x * sgn));
            let c = bld.vert(ll(end[0], end[1], end[2]), n, color);
            let rim: Vec<u32> = ring
                .iter()
                .map(|q| {
                    let i = q.get(j).copied().unwrap_or(0) as usize * 3;
                    let p = [bld.pos.g(i), bld.pos.g(i + 1), bld.pos.g(i + 2)];
                    bld.vert(p, n, color)
                })
                .collect();
            for k in 0..sides {
                let (r0, r1) = (gu(&rim, k), gu(&rim, (k + 1) % sides));
                if sgn > 0.0 {
                    bld.tri(c, r0, r1);
                } else {
                    bld.tri(c, r1, r0);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------- labels and picking

/// The camera picking and decluttering look through (camera.js): `ray`, `viewProj`, `project(p, vp)`.
pub trait PickCamera {
    fn ray(&self, sx: f64, sy: f64) -> ([f64; 3], [f64; 3]);
    fn view_proj(&self) -> [f64; 16];
    fn project_vp(&self, p: [f64; 3], vp: &[f64; 16]) -> Option<[f64; 3]>;
}

/// `declutterLabels(labels, project, {w, h, hPx, charPx, gap})` → 1 = draw.
pub fn declutter_labels(
    labels: &[Label],
    project: &dyn Fn([f64; 3]) -> Option<[f64; 3]>,
    w: f64,
    h: f64,
    h_px: f64,
    char_px: f64,
    gap: f64,
) -> Vec<u8> {
    struct Cand {
        i: usize,
        x0: f64,
        x1: f64,
        y0: f64,
        y1: f64,
        d: f64,
        pri: f64,
    }
    let mut vis = vec![0u8; labels.len()];
    let mut cand: Vec<Cand> = Vec::new();
    for (i, l) in labels.iter().enumerate() {
        let Some(s) = project(l.at) else { continue };
        if s[0] < -50.0 || s[0] > w + 50.0 || s[1] < -20.0 || s[1] > h + 20.0 {
            continue;
        }
        let lw = crate::js::len16(&l.text) as f64 * char_px + 8.0;
        cand.push(Cand {
            i,
            x0: s[0] - lw / 2.0 - gap,
            x1: s[0] + lw / 2.0 + gap,
            y0: s[1] - 6.0 - h_px - gap,
            y1: s[1] - 6.0 + gap,
            d: s[2],
            pri: l.pri.unwrap_or(9.0),
        });
    }
    crate::js::sort_by(&mut cand, |a, b| {
        let p = a.pri - b.pri;
        if p != 0.0 && !p.is_nan() { p } else { a.d - b.d }
    });
    let cell = 64.0;
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    let fl = |x: f64| crate::js::i64_of((x / cell).floor()).unwrap_or(0);
    for (ci, c) in cand.iter().enumerate() {
        let (cx0, cx1, cy0, cy1) = (fl(c.x0), fl(c.x1), fl(c.y0), fl(c.y1));
        let mut hit = false;
        let mut cx = cx0;
        while cx <= cx1 && !hit {
            let mut cy = cy0;
            while cy <= cy1 && !hit {
                for &oi in grid.get(&(cx, cy)).map_or(&[][..], Vec::as_slice) {
                    if let Some(o) = cand.get(oi)
                        && o.x0 < c.x1
                        && c.x0 < o.x1
                        && o.y0 < c.y1
                        && c.y0 < o.y1
                    {
                        hit = true;
                        break;
                    }
                }
                cy = cy.saturating_add(1);
            }
            cx = cx.saturating_add(1);
        }
        if hit {
            continue;
        }
        if let Some(v) = vis.get_mut(c.i) {
            *v = 1;
        }
        for cx in cx0..=cx1 {
            for cy in cy0..=cy1 {
                grid.entry((cx, cy)).or_default().push(ci);
            }
        }
    }
    vis
}

/// What a pick found.
#[derive(Clone, Debug)]
pub struct Pick {
    pub kind: String,
    pub id: Option<Value>,
    pub name: Option<Value>,
    pub at: [f64; 3],
}
impl J for Pick {
    fn j(&self) -> Value {
        let mut m = Map::new();
        m.insert("kind".into(), Value::String(self.kind.clone()));
        if let Some(v) = &self.id {
            m.insert("id".into(), v.clone());
        }
        if let Some(v) = &self.name {
            m.insert("name".into(), v.clone());
        }
        m.insert("at".into(), crate::jo! { "x" => self.at[0], "y" => self.at[1], "z" => self.at[2] });
        Value::Object(m)
    }
}

fn tri_at(p: &[f32], i: &[u32], t: usize) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let v = |k: usize| {
        let j = i.get(t + k).copied().unwrap_or(0) as usize * 3;
        [gf(p, j), gf(p, j + 1), gf(p, j + 2)]
    };
    (v(0), v(1), v(2))
}

fn ray_tri(o: [f64; 3], d: [f64; 3], a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Option<f64> {
    cc_m3d::vec::ray_tri(o, d, a, b, c)
}

/// `surfaceHit(scene, o, d)`: the nearest hit on the surfaces (meshes without owners).
fn surface_hit(scene: &Scene, o: [f64; 3], d: [f64; 3]) -> Option<f64> {
    let mut t: Option<f64> = None;
    for m in &scene.meshes {
        if m.owners.is_some() {
            continue;
        }
        let mut k = 0;
        while k + 2 < m.indices.len() {
            let (a, b, c) = tri_at(&m.positions, &m.indices, k);
            if let Some(h) = ray_tri(o, d, a, b, c)
                && t.is_none_or(|t| h < t)
            {
                t = Some(h);
            }
            k += 3;
        }
    }
    t
}

/// `pickSite(scene, cam, sx, sy, {tol})`.
pub fn pick_site(scene: &Scene, cam: &dyn PickCamera, sx: f64, sy: f64, tol: f64) -> Option<Pick> {
    let (o, d) = cam.ray(sx, sy);
    let to_project = |p: [f64; 3]| [p[0] + scene.origin.x, p[1] + scene.origin.y, p[2] / scene.exag + scene.origin.z];
    struct Best<'a> {
        t: f64,
        m: &'a Mesh,
        tri: usize,
    }
    let mut best: Option<Best> = None;
    // owned meshes first (a stable sort, as the JS)
    let mut meshes: Vec<&Mesh> = scene.meshes.iter().collect();
    meshes.sort_by_key(|m| if m.owners.is_some() { 0 } else { 1 });
    for m in meshes {
        if best.as_ref().is_some_and(|b| b.m.owners.is_some()) && m.owners.is_none() {
            break;
        }
        let mut t = 0;
        while t + 2 < m.indices.len() {
            let (a, b, c) = tri_at(&m.positions, &m.indices, t);
            if let Some(hit) = ray_tri(o, d, a, b, c)
                && best.as_ref().is_none_or(|b| hit < b.t)
            {
                best = Some(Best { t: hit, m, tri: t / 3 });
            }
            t += 3;
        }
    }
    let dot = |p: [f64; 3]| (p[0] - o[0]) * d[0] + (p[1] - o[1]) * d[1] + (p[2] - o[2]) * d[2];
    if let Some(pts) = &scene.points {
        let vp = cam.view_proj();
        let mut pb: Option<(f64, usize, [f64; 3])> = None;
        for (i, c) in pts.positions.as_chunks::<3>().0.iter().enumerate() {
            let p = [gf(c, 0), gf(c, 1), gf(c, 2)];
            let Some(s) = cam.project_vp(p, &vp) else { continue };
            let dd = crate::js::hypot(s[0] - sx, s[1] - sy);
            if dd <= tol && pb.is_none_or(|q| dd < q.0) {
                pb = Some((dd, i, p));
            }
        }
        if let Some((_, i, p)) = pb {
            let along = dot(p);
            let solid = best.as_ref().filter(|b| b.m.owners.is_some());
            if solid.is_none_or(|b| along <= b.t * 1.001) {
                let id = pts.ids.get(i).cloned().unwrap_or_default();
                return Some(Pick {
                    kind: "point".into(),
                    name: Some(Value::String(tr!("s3.point", n = id.as_str()))),
                    id: Some(Value::String(id)),
                    at: to_project(p),
                });
            }
        }
    }
    // an owned line within tol px, unless the ground is in front of it
    {
        let vp = cam.view_proj();
        let ground_t = if scene.meshes.iter().any(|m| m.owners.is_none()) { surface_hit(scene, o, d) } else { None };
        let mut lb: Option<(f64, &Line, usize, [f64; 3])> = None;
        for l in &scene.lines {
            if l.owners.is_none() {
                continue;
            }
            for (k, c) in l.positions.as_chunks::<6>().0.iter().enumerate() {
                let a = [gf(c, 0), gf(c, 1), gf(c, 2)];
                let b = [gf(c, 3), gf(c, 4), gf(c, 5)];
                let (Some(sa), Some(sb)) = (cam.project_vp(a, &vp), cam.project_vp(b, &vp)) else { continue };
                let ex = sb[0] - sa[0];
                let ey = sb[1] - sa[1];
                let l2 = ex * ex + ey * ey;
                let u = if l2 != 0.0 && !l2.is_nan() {
                    crate::js::max(0.0, crate::js::min(1.0, ((sx - sa[0]) * ex + (sy - sa[1]) * ey) / l2))
                } else {
                    0.0
                };
                let dd = crate::js::hypot(sx - sa[0] - ex * u, sy - sa[1] - ey * u);
                if dd > tol || lb.as_ref().is_some_and(|q| dd >= q.0) {
                    continue;
                }
                let p = [a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u, a[2] + (b[2] - a[2]) * u];
                let along = dot(p);
                if let Some(g) = ground_t
                    && along > g * 1.01 + 1e-6
                {
                    continue;
                }
                lb = Some((dd, l, k, p));
            }
        }
        let solid_under = best.as_ref().is_some_and(|b| b.m.owners.is_some());
        if let Some((dd, l, k, p)) = lb
            && (!solid_under || dd <= 3.0)
        {
            let ow = l.seg_owner.as_ref().and_then(|s| s.get(k)).and_then(|&i| l.owners.as_ref().and_then(|o| o.get(i as usize)));
            return Some(Pick {
                kind: ow.map_or_else(String::new, |o| o.kind.to_string()),
                id: ow.and_then(|o| o.id.clone()),
                name: ow.and_then(|o| o.name.clone()),
                at: to_project(p),
            });
        }
    }
    let best = best?;
    let at = to_project([o[0] + d[0] * best.t, o[1] + d[1] * best.t, o[2] + d[2] * best.t]);
    if let Some(owners) = &best.m.owners {
        let ow = best.m.tri_owner.as_ref().and_then(|t| t.get(best.tri)).and_then(|&i| owners.get(i as usize));
        return Some(Pick {
            kind: ow.map_or_else(String::new, |o| o.kind.to_string()),
            id: ow.and_then(|o| o.id.clone()),
            name: ow.and_then(|o| o.name.clone()),
            at,
        });
    }
    Some(Pick { kind: "surface".into(), id: Some(Value::String(best.m.key.clone())), name: Some(Value::String(best.m.name.clone())), at })
}
