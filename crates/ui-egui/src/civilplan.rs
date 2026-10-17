//! CivilCraft's plan view (view.js): the plan scene (io-civil `plan_scene`, the same linework and labels DXF, DWG and
//! PDF draw) plus the screen's own items (feature lines, parcels, sample lines, line-code figures, daylighted feature
//! lines), asked for at the zoom rounded to a power of two (paper mm per unit) so labels keep their spacing without a
//! rebuild on every wheel step. Linework goes to the GPU as one cc-render space whose vertices are relative to the
//! site's centre (f32 there is exact to well under a millimetre at state-plane magnitudes); the view itself is f64.
//! Survey points are crosses baked at the zoom bucket's size, so a million points cost one buffer, not a million
//! egui shapes. Labels are egui text in screen space, culled to the view, at a fixed size on screen.

use std::sync::Arc;

use cc_civil::geo::P2;
use cc_civil::js::{J, arr, numf};
use cc_civil::project::Project;
use cc_drawing::{Batch, Bounds, Color, Fill, Space, SpaceKind};
use cc_io_civil::scene::{self, SceneAlignment, SceneCorridor, ScenePad, SceneState, Show};
use cc_render::{SpaceGeometry, SpaceMesh, View2D, ViewUniform};
use egui::{Color32, Pos2, Rect};
use serde_json::Value;

use crate::canvas::GpuState;
use crate::theme::Tokens;

/// CSS pixels per paper millimetre (view.js `PX_PER_MM`).
pub const PX_PER_MM: f64 = 96.0 / 25.4;

/// view.js `WIDTHS`: line widths in screen pixels per layer (1 when not listed).
fn width_of(layer: &str) -> f32 {
    match layer {
        "C-TOPO-MAJR" => 1.5,
        "C-TOPO-MINR" => 0.75,
        "C-TOPO-TINN" => 0.6,
        "C-TOPO-BRKL" => 1.4,
        "C-TOPO-BNDY" => 1.2,
        "C-ROAD-CNTR" => 2.2,
        "C-ROAD-STAN" => 1.2,
        "C-ROAD-CORR" => 1.4,
        "C-GRAD-PADS" => 2.0,
        "C-GRAD-DAYL" => 1.6,
        "C-GRAD-SLOP" => 0.6,
        "C-STRM-PIPE" => 2.4,
        "C-STRM-STRC" => 1.6,
        "C-STRM-PROB" => 2.8,
        "V-NODE" => 1.2,
        _ => 1.0,
    }
}

/// The plan's colours: the canvas and ink from the theme tokens, and CivilCraft's layer hues (its own values: the
/// light set is 0.5's, the dark set the same hues lifted for a dark canvas). view.js `theme()`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub bg: Color32,
    pub ink: Color32,
    pub ink2: Color32,
    pub line: Color32,
    pub action: Color32,
    pub red: Color32,
    pub orange: Color32,
    pub green: Color32,
    pub blue: Color32,
    pub magenta: Color32,
    pub cyan: Color32,
    pub ochre: Color32,
    pub major: Color32,
    pub minor: Color32,
    pub tin: Color32,
    pub dark: bool,
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (f32::from(x) * (1.0 - t) + f32::from(y) * t).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

impl Palette {
    pub fn of(t: &Tokens, dark: bool) -> Palette {
        let hex = |n: u32| Color32::from_rgb(((n >> 16) & 0xFF) as u8, ((n >> 8) & 0xFF) as u8, (n & 0xFF) as u8);
        let (bg, ink) = (t.canvas_bg, t.ink);
        let (ink2, line, red, orange, green, blue, magenta, cyan, ochre) = if dark {
            (
                hex(0xc3cad3),
                hex(0x8d98a6),
                hex(0xff6f83),
                hex(0xf0a35a),
                hex(0x5fcf8f),
                hex(0x82a6ff),
                hex(0xe58ad6),
                hex(0x52d0dc),
                hex(0xdcbb52),
            )
        } else {
            (
                hex(0x34404e),
                hex(0x7a8796),
                hex(0xb3122e),
                hex(0xa84a00),
                hex(0x17703a),
                hex(0x1e48c8),
                hex(0x9a1f86),
                hex(0x00707e),
                hex(0x8a6200),
            )
        };
        Palette {
            bg,
            ink,
            ink2,
            line,
            action: t.accent,
            red,
            orange,
            green,
            blue,
            magenta,
            cyan,
            ochre,
            major: ink,
            minor: mix(ink, bg, if dark { 0.55 } else { 0.5 }),
            tin: mix(line, bg, 0.35),
            dark,
        }
    }
    /// view.js `colorOf(token)`.
    pub fn token(&self, token: &str) -> Color32 {
        match token {
            "ink2" => self.ink2,
            "line" => self.line,
            "red" => self.red,
            "orange" => self.orange,
            "green" => self.green,
            "blue" => self.blue,
            "magenta" => self.magenta,
            "cyan" => self.cyan,
            "ochre" => self.ochre,
            "major" => self.major,
            "minor" => self.minor,
            "tin" => self.tin,
            _ => self.ink,
        }
    }
    /// The colour of a plan layer (scene.js `LAYERS` token).
    pub fn layer(&self, layer: &str) -> Color32 {
        let tok = scene::LAYERS.iter().find(|l| l.0 == layer).map_or(
            match layer {
                "C-GRAD-FEAT" | "V-NODE-LINE" => "green",
                "C-ROAD-SCTN" => "ink2",
                _ => "ink",
            },
            |l| l.1,
        );
        self.token(tok)
    }
}

/// A label in world coordinates, drawn at a fixed size on screen (scene.js `text` items).
#[derive(Clone, Debug)]
pub struct Label {
    pub x: f64,
    pub y: f64,
    pub rot: f64,
    pub mm: f64,
    /// 0 left, 1 centre, 2 right
    pub align: u8,
    pub middle: bool,
    pub mask: bool,
    pub color: Color32,
    pub text: String,
}

/// The plan built for one zoom bucket.
pub struct Plan {
    /// paper mm per unit this was built for
    pub mm: f64,
    pub key: PlanKey,
    pub space: Space,
    pub geometry: Arc<Vec<SpaceGeometry>>,
    /// GPU meshes are keyed by it
    pub id: u64,
    /// line width (screen pixels) of each batch
    pub widths: Vec<f32>,
    pub labels: Vec<Label>,
    pub build_ms: f64,
    /// time per build stage (ms)
    pub stages: Vec<(&'static str, f64)>,
    pub segments: u64,
}

/// What a plan depends on: the project's edits and display settings, the zoom bucket and the theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlanKey {
    pub version: u64,
    pub points_version: u64,
    pub mm_bits: u64,
    pub dark: bool,
    pub palette_ink: [u8; 4],
}

fn next_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    // far from the drawing loader's ids (they count from 1), so a civil plan never shares a GPU mesh key
    static N: AtomicU64 = AtomicU64::new(1 << 40);
    N.fetch_add(1, Ordering::Relaxed)
}

/// view.js `mmBucket()`: paper mm per unit for a zoom in screen (CSS) pixels per unit, rounded to a power of two.
pub fn mm_bucket(zoom_css: f64) -> f64 {
    let mm = zoom_css / PX_PER_MM;
    if !(mm.is_finite() && mm > 0.0) {
        return 1.0;
    }
    2f64.powf(mm.log2().round()).clamp(1e-9, 1e9)
}

fn bool_of(d: Option<&Value>, k: &str, def: bool) -> bool {
    d.and_then(|d| d.get(k)).map_or(def, |v| cc_civil::js::truthy(Some(v)))
}

/// Accumulates batches per (layer, colour) in first-seen order.
struct Builder {
    batches: Vec<Batch>,
    widths: Vec<f32>,
    labels: Vec<Label>,
    lo: (f64, f64),
    hi: (f64, f64),
}

impl Builder {
    fn new() -> Self {
        Builder {
            batches: Vec::new(),
            widths: Vec::new(),
            labels: Vec::new(),
            lo: (f64::INFINITY, f64::INFINITY),
            hi: (f64::NEG_INFINITY, f64::NEG_INFINITY),
        }
    }
    /// The batch for (layer, colour), made on first use; its index.
    fn batch(&mut self, layer: &str, color: Color32, width: f32) -> usize {
        let rgb = (u32::from(color.r()) << 16) | (u32::from(color.g()) << 8) | u32::from(color.b());
        if let Some(i) = self.batches.iter().position(|b| b.layer == layer && b.color == Color::Rgb(rgb)) {
            return i;
        }
        self.batches.push(Batch {
            layer: layer.to_string(),
            color: Color::Rgb(rgb),
            lw: f64::from(width),
            lines: Vec::new(),
            fills: Vec::new(),
            texts: Vec::new(),
        });
        self.widths.push(width);
        self.batches.len() - 1
    }
    fn lines_of(&mut self, layer: &str, color: Color32, width: f32) -> Option<&mut Vec<Vec<f64>>> {
        let i = self.batch(layer, color, width);
        self.batches.get_mut(i).map(|b| &mut b.lines)
    }
    fn grow(&mut self, x: f64, y: f64) {
        if x.is_finite() && y.is_finite() {
            self.lo = (self.lo.0.min(x), self.lo.1.min(y));
            self.hi = (self.hi.0.max(x), self.hi.1.max(y));
        }
    }
    fn line(&mut self, layer: &str, color: Color32, width: f32, flat: Vec<f64>) {
        if flat.len() < 4 {
            return;
        }
        for p in flat.as_chunks::<2>().0 {
            self.grow(p[0], p[1]);
        }
        if let Some(l) = self.lines_of(layer, color, width) {
            l.push(flat);
        }
    }
}

fn pts_flat(pts: &[Value], closed: bool) -> Vec<f64> {
    let mut out = Vec::with_capacity(pts.len() * 2 + 2);
    for p in pts {
        out.push(numf(p.get("x")));
        out.push(numf(p.get("y")));
    }
    if closed && let (Some(&x), Some(&y)) = (out.first(), out.get(1)) {
        out.push(x);
        out.push(y);
    }
    out
}

/// An arc as a polyline (every 2° at most), angles counter-clockwise from a0 to a1.
fn arc_flat(cx: f64, cy: f64, r: f64, a0: f64, a1: f64) -> Vec<f64> {
    let mut sweep = a1 - a0;
    while sweep < 0.0 {
        sweep += std::f64::consts::TAU;
    }
    if !(sweep.is_finite() && r.is_finite()) {
        return Vec::new();
    }
    let n = ((sweep / 2f64.to_radians()).ceil() as usize).clamp(2, 720);
    (0..=n)
        .flat_map(|i| {
            let a = a0 + sweep * i as f64 / n as f64;
            [cx + r * a.cos(), cy + r * a.sin()]
        })
        .collect()
}

/// Dashes along a polyline (lengths in world units).
fn dashes(flat: &[f64], on: f64, off: f64) -> Vec<Vec<f64>> {
    let mut out = Vec::new();
    let pts = flat.as_chunks::<2>().0;
    let period = on + off;
    if period.is_nan() || period <= 0.0 || !period.is_finite() {
        return out;
    }
    let mut at = 0.0f64;
    let mut cur: Vec<f64> = Vec::new();
    for w in pts.windows(2) {
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        if len.is_nan() || len <= 0.0 {
            continue;
        }
        let mut s = 0.0;
        let mut guard = 0u32;
        while s < len && guard < 100_000 {
            guard += 1;
            let phase = at % period;
            let drawing = phase < on;
            let left = if drawing { on - phase } else { period - phase };
            let step = left.min(len - s);
            let p = |u: f64| [a[0] + (b[0] - a[0]) * u / len, a[1] + (b[1] - a[1]) * u / len];
            if drawing {
                if cur.is_empty() {
                    cur.extend_from_slice(&p(s));
                }
                cur.extend_from_slice(&p(s + step));
            } else if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            s += step;
            at += step;
        }
    }
    if cur.len() >= 4 {
        out.push(cur);
    }
    out
}

/// Which surface the plan shows (civilcraft.js `shown()`): the graded one when asked for and there is one.
pub fn shown_is_fg(p: &mut Project) -> bool {
    let want = p.state.get("display").and_then(|d| d.get("surface")).and_then(Value::as_str) == Some("fg");
    want && p.graded().surface.is_some()
}

/// civilcraft.js `buildScene(mm)` + view.js `buildPaths`: everything the plan draws at `mm` paper mm per unit.
pub fn build(p: &mut Project, mm: f64, pal: &Palette, key: PlanKey, fixed: &mut Option<Fixed>) -> Plan {
    let t0 = web_time::Instant::now();
    let d = p.state.get("display").cloned();
    let d = d.as_ref();
    let units = p.units().to_string();
    let mut stages: Vec<(&'static str, f64)> = Vec::new();
    let fg = shown_is_fg(p);
    let eg = p.eg();
    let graded = p.graded();
    let surface = if fg { graded.surface.as_deref() } else { eg.surface.as_ref() };
    let contours = if surface.is_none() {
        Arc::new(Vec::new())
    } else if fg {
        p.contours_fg()
    } else {
        p.contours()
    };
    let interval = numf(d.and_then(|d| d.get("interval")));
    let major = d.and_then(|d| d.get("major")).map_or(f64::NAN, |m| numf(Some(m)));
    // breaklines (EG only) and the boundary
    let mut breaklines: Vec<Vec<Value>> = Vec::new();
    if !fg {
        for b in p.eg_breaklines() {
            breaklines.push(b.pts.iter().map(J::j).collect());
        }
    }
    if let Some(bnd) = p.state.get("surface").and_then(|s| s.get("boundary")).and_then(Value::as_array).filter(|b| !b.is_empty()) {
        let mut v = bnd.clone();
        if let Some(f) = bnd.first() {
            v.push(f.clone());
        }
        breaklines.push(v);
    }
    let als: Vec<Value> = arr(p.state.get("alignments")).to_vec();
    let al_built: Vec<Arc<cc_civil::alignment::Alignment>> = als.iter().map(|a| p.alignment(a)).collect();
    let alignments: Vec<SceneAlignment> = als
        .iter()
        .zip(al_built.iter())
        .map(|(a, al)| SceneAlignment {
            name: cc_civil::js::string_or_empty(a.get("name")),
            alignment: Some(al.as_ref()),
            pis: arr(a.get("pis")).to_vec(),
        })
        .collect();
    let pads: Vec<ScenePad> = arr(p.state.get("pads"))
        .iter()
        .map(|pd| {
            let g = graded.items.iter().find(|x| x.kind == "pad" && x.id == pd.get("id").cloned().unwrap_or(Value::Null));
            let daylight = g.map(|g| arr(g.info.get("daylight")).to_vec()).unwrap_or_default();
            let rays = g
                .map(|g| {
                    arr(g.info.get("rays"))
                        .iter()
                        .map(|r| (numf(r.get("x0")), numf(r.get("y0")), numf(r.get("x")), numf(r.get("y")), numf(r.get("d"))))
                        .collect()
                })
                .unwrap_or_default();
            ScenePad {
                name: cc_civil::js::string_or_empty(pd.get("name")),
                pts: arr(pd.get("pts")).to_vec(),
                z: cc_civil::js::number(pd.get("z")),
                daylight,
                rays,
            }
        })
        .collect();
    let corridors: Vec<SceneCorridor> = graded
        .items
        .iter()
        .filter(|x| x.kind == "corridor")
        .map(|x| SceneCorridor {
            lines: x
                .lines
                .as_ref()
                .map(|ls| ls.iter().map(|(k, v)| (k.clone(), v.iter().map(J::j).collect())).collect())
                .unwrap_or_default(),
            sections: arr(x.info.get("sections")).iter().map(|s| (arr(s.get("L")).to_vec(), arr(s.get("R")).to_vec())).collect(),
        })
        .collect();
    let network = p.state.get("network").filter(|n| n.is_object()).cloned().map(|n| (cc_civil::pipes::read_network(&n), p.network_check()));
    let st = SceneState {
        units: units.clone(),
        // survey points are drawn below (crosses baked per bucket, labels in screen space)
        points: Vec::new(),
        surface,
        // contours and the outline are drawn below from the engine's own lists (millions of vertices as JSON otherwise)
        contours: None,
        breaklines,
        alignments,
        pads,
        corridors,
        network,
    };
    let show_points = bool_of(d, "points", true);
    let point_labels = bool_of(d, "pointLabels", false);
    let triangles = bool_of(d, "triangles", false);
    let show = Show {
        // TIN edges are written straight into the batches below (millions of JSON objects otherwise)
        triangles: Some(false),
        contours: Some(false),
        labels: Some(bool_of(d, "labels", true)),
        breaklines: Some(bool_of(d, "breaklines", true)),
        outline: Some(false),
        points: Some(false),
        point_labels: Some(false),
        slopes: Some(bool_of(d, "slopes", false)),
        alignments: d.and_then(|d| d.get("alignments")).map(|v| cc_civil::js::truthy(Some(v))),
        pads: None,
        pipes: Some(bool_of(d, "pipes", true)),
        point_label_text: None,
    };
    stages.push(("engine", ms_since(t0)));
    let t_scene = web_time::Instant::now();
    let mut items = scene::plan_scene(&st, mm, &show);
    drop(st);
    stages.push(("planScene", ms_since(t_scene)));
    let t_extra = web_time::Instant::now();
    // 0.4 V13 sample lines, V14 feature lines, V21 parcels, V16 line-code figures, V14 daylighted feature lines
    if d.and_then(|d| d.get("alignments")) != Some(&Value::Bool(false)) {
        for a in &als {
            if let Some(set) = p.sections(a) {
                for s in &set.sections {
                    items.push(serde_json::json!({ "layer": "C-ROAD-SCTN", "kind": "poly", "pts": [s.a.j(), s.b.j()] }));
                }
            }
        }
    }
    for f in arr(p.state.get("features")) {
        items.push(serde_json::json!({ "layer": "C-GRAD-FEAT", "kind": "poly", "pts": f.get("pts").cloned().unwrap_or(Value::Null),
            "closed": f.get("closed").cloned().unwrap_or(Value::Bool(false)) }));
    }
    for pc in arr(p.state.get("parcels")) {
        items.extend(cc_civil::parcels::parcel_labels(&cc_civil::parcels::read_parcel(pc), &units, 1.0 / mm));
    }
    for it in graded.items.iter().filter(|x| x.kind == "feature") {
        let dl = arr(it.info.get("daylight"));
        if dl.is_empty() {
            continue;
        }
        items.push(serde_json::json!({ "layer": "C-GRAD-DAYL", "kind": "poly", "pts": dl, "closed": true }));
        for r in arr(it.info.get("rays")) {
            if numf(r.get("d")) > 1e-6 {
                items.push(serde_json::json!({ "layer": "C-GRAD-SLOP", "kind": "poly",
                    "pts": [{ "x": numf(r.get("x0")), "y": numf(r.get("y0")) }, { "x": numf(r.get("x")), "y": numf(r.get("y")) }] }));
            }
        }
    }

    stages.push(("screen items", ms_since(t_extra)));
    // what does not change with the zoom: kept between buckets
    let t_fixed = web_time::Instant::now();
    let fkey = format!(
        "{}|{}|{:p}|{:p}|{}",
        fg,
        p.points_version,
        Arc::as_ptr(&eg),
        Arc::as_ptr(&graded),
        cc_civil::project::js_stringify(
            &p.state
                .get("surface")
                .map(|s| serde_json::json!([s.get("descKeys"), s.get("lineCodes"), s.get("groups")]))
                .unwrap_or(Value::Null)
        )
    );
    if fixed.as_ref().is_none_or(|f| f.key != fkey) {
        *fixed = Some(Fixed::make(p, surface, fkey));
    }
    stages.push(("fixed", ms_since(t_fixed)));
    if let Some(fx) = fixed.as_ref() {
        stages.push(("outline", fx.ms.0));
        stages.push(("figures", fx.ms.1));
    }
    let mut b = Builder::new();
    // elevation bands under everything (civilcraft.js bands(): band_levels over the shown surface, band colours)
    if bool_of(d, "bands", false)
        && let Some(s) = surface
    {
        let stt = s.stats();
        let n = numf(d.and_then(|d| d.get("bandCount")));
        let n = if n >= 2.0 { n.min(64.0) } else { 8.0 };
        let levels = cc_civil::site3d::band_levels(stt.min_z, stt.max_z, n);
        let count = levels.len().saturating_sub(1).max(1) as f64;
        for piece in s.bands(&levels) {
            let tt = piece.band as f64 / (count - 1.0).max(1.0);
            let rgb = cc_civil::site3d::rgb(&cc_civil::site3d::band_color(tt, pal.dark));
            let c = Color32::from_rgb((rgb[0] * 255.0) as u8, (rgb[1] * 255.0) as u8, (rgb[2] * 255.0) as u8);
            let i = b.batch("bands", c, 0.0);
            if let Some(bt) = b.batches.get_mut(i) {
                bt.fills.push(Fill { rings: vec![piece.ring], pattern: false });
            }
        }
    }
    // TIN edges
    if triangles && let Some(s) = surface {
        let e = s.edges();
        let mut flat_all: Vec<Vec<f64>> = Vec::with_capacity(e.len() / 2);
        for pair in e.as_chunks::<2>().0 {
            let (a, c) = (pair[0] as usize, pair[1] as usize);
            let (Some(&ax), Some(&ay), Some(&cx), Some(&cy)) = (s.x.get(a), s.y.get(a), s.x.get(c), s.y.get(c)) else { continue };
            flat_all.push(vec![ax + s.ox, ay + s.oy, cx + s.ox, cy + s.oy]);
        }
        let col = pal.layer("C-TOPO-TINN");
        for f in flat_all {
            b.line("C-TOPO-TINN", col, width_of("C-TOPO-TINN"), f);
        }
    }
    let unit_px = mm * PX_PER_MM; // CSS px per unit at this bucket
    if let Some(fx) = fixed.as_ref() {
        if bool_of(d, "outline", true) {
            let col = pal.layer("C-TOPO-BNDY");
            for r in &fx.outline {
                b.line("C-TOPO-BNDY", col, width_of("C-TOPO-BNDY"), r.clone());
            }
        }
        if bool_of(d, "contours", true) && surface.is_some() {
            contour_lines(&mut b, &contours, interval, major, mm, bool_of(d, "labels", true), pal);
        }
        if show_points && !p.points.is_empty() {
            let col = pal.layer("V-NODE-LINE");
            for f in &fx.figures {
                b.line("V-NODE-LINE", col, width_of("V-NODE-LINE"), f.clone());
            }
        }
    }
    for it in &items {
        let layer = it.get("layer").and_then(Value::as_str).unwrap_or("0");
        let col = pal.layer(layer);
        match it.get("kind").and_then(Value::as_str) {
            Some("poly") => {
                let closed = it.get("closed") == Some(&Value::Bool(true));
                let flat = pts_flat(arr(it.get("pts")), closed);
                if it.get("dashed") == Some(&Value::Bool(true)) {
                    for dsh in dashes(&flat, 6.0 / unit_px, 4.0 / unit_px) {
                        b.line(layer, col, 1.4, dsh);
                    }
                } else {
                    b.line(layer, col, width_of(layer), flat);
                }
            }
            Some("arc") => {
                let c = it.get("c");
                let flat = arc_flat(
                    numf(c.and_then(|c| c.get("x"))),
                    numf(c.and_then(|c| c.get("y"))),
                    numf(it.get("r")),
                    numf(it.get("a0")),
                    numf(it.get("a1")),
                );
                b.line(layer, col, width_of(layer), flat);
            }
            Some("text") => {
                let align = match it.get("align").and_then(Value::as_str) {
                    Some("center") => 1,
                    Some("right") => 2,
                    _ => 0,
                };
                b.labels.push(Label {
                    x: numf(it.get("x")),
                    y: numf(it.get("y")),
                    rot: numf(it.get("rot")),
                    mm: numf(it.get("mm")),
                    align,
                    middle: it.get("middle") == Some(&Value::Bool(true)),
                    mask: it.get("mask") == Some(&Value::Bool(true)),
                    color: col,
                    text: it.get("text").and_then(Value::as_str).unwrap_or("").to_string(),
                });
            }
            _ => {}
        }
    }
    drop(items);
    // survey points: view.js crosses (2.5 px, 1.5 px for big sets zoomed out), baked at this bucket
    if show_points && !p.points.is_empty() {
        // a batch with no lines: its crosses are written straight into the GPU bytes after build_geometry (a
        // million points would otherwise be two million small Vecs)
        let _ = b.lines_of("V-NODE", pal.green, width_of("V-NODE"));
        for q in &p.points {
            b.grow(q.x, q.y);
        }
        if point_labels {
            // scene.js point labels (the setting's "n z d" text)
            let upm = 1.0 / mm;
            for q in &p.points {
                let mut s = q.n.clone();
                if let Some(z) = q.z {
                    s.push_str("  ");
                    s.push_str(&cc_civil::units::fixed(z, 2));
                }
                if !q.d.is_empty() {
                    s.push_str("  ");
                    s.push_str(&q.d);
                }
                b.labels.push(Label {
                    x: q.x + 1.2 * upm,
                    y: q.y + 0.4 * upm,
                    rot: 0.0,
                    mm: 1.5,
                    align: 0,
                    middle: false,
                    mask: false,
                    color: pal.layer("V-NODE-TEXT"),
                    text: s,
                });
            }
        }
    }
    let bounds = if b.lo.0.is_finite() {
        Bounds { min_x: b.lo.0, min_y: b.lo.1, max_x: b.hi.0, max_y: b.hi.1 }
    } else {
        Bounds { min_x: 0.0, min_y: 0.0, max_x: 0.0, max_y: 0.0 }
    };
    let space = Space {
        key: "plan".into(),
        name: "Plan".into(),
        kind: SpaceKind::Model,
        batches: b.batches,
        bounds,
        paper: None,
        paper_unit: None,
        viewports: Vec::new(),
        images: Vec::new(),
    };
    stages.push(("draw lists", ms_since(t_scene)));
    let t_geo = web_time::Instant::now();
    let mut geometry = cc_render::build_geometry(&space);
    if show_points && !p.points.is_empty() {
        let k_px = if p.points.len() > 5000 && unit_px < 0.5 { 1.5 } else { 2.5 };
        point_crosses(&mut geometry, &space, &p.points, k_px / unit_px, pal.green);
    }
    let segments = geometry.segments();
    stages.push(("geometry", ms_since(t_geo)));
    Plan {
        mm,
        key,
        space,
        geometry: Arc::new(vec![geometry]),
        id: next_id(),
        widths: b.widths,
        labels: b.labels,
        build_ms: t0.elapsed().as_secs_f64() * 1000.0,
        segments,
        stages,
    }
}

/// The parts of a plan that do not change with the zoom (the TIN outline, line-code figures), kept between zoom
/// buckets: on a million-point survey they cost seconds.
pub struct Fixed {
    key: String,
    outline: Vec<Vec<f64>>,
    figures: Vec<Vec<f64>>,
    /// (outline ms, figures ms)
    pub ms: (f64, f64),
}

impl Fixed {
    fn make(p: &mut Project, surface: Option<&cc_civil::tin::Surface>, key: String) -> Fixed {
        let t0 = web_time::Instant::now();
        let outline = surface
            .map(|s| {
                s.outline()
                    .iter()
                    .map(|r| {
                        let mut f: Vec<f64> = r.iter().flat_map(|q| [q.x, q.y]).collect();
                        if let (Some(&x), Some(&y)) = (f.first(), f.get(1)) {
                            f.push(x);
                            f.push(y);
                        }
                        f
                    })
                    .collect()
            })
            .unwrap_or_default();
        let outline_ms = ms_since(t0);
        let t1 = web_time::Instant::now();
        // 0.4 V16: lines that line codes make in groups that are not breaklines (codes-ui.js sceneItems)
        let mut figures = Vec::new();
        if !p.points.is_empty() {
            let groups = p.state.get("surface").and_then(|s| s.get("groups")).cloned().unwrap_or(Value::Null);
            // only codes that carry a line-code control make drawn figures: the rest (a million plain ground shots)
            // are left out before the engine sorts them; the figures that come back are the same
            let lc = match p.state.get("surface").and_then(|s| s.get("lineCodes")).filter(|v| cc_civil::js::truthy(Some(v))) {
                Some(v) => cc_civil::desckeys::read_line_codes(v),
                None => cc_civil::desckeys::clean_line_codes(None),
            };
            let mut coded: std::collections::HashSet<String> = std::collections::HashSet::new();
            for q in p.points.iter().filter(|q| q.z.is_some()) {
                let (code, ctrl, _) = cc_civil::desckeys::parse_desc(&q.d, &lc);
                if ctrl.is_some() {
                    coded.insert(code);
                }
            }
            let subset: Vec<cc_civil::points::Pt> = if coded.is_empty() {
                Vec::new()
            } else {
                p.points.iter().filter(|q| q.z.is_some() && coded.contains(&cc_civil::desckeys::parse_desc(&q.d, &lc).0)).cloned().collect()
            };
            let keys = p.state_keys();
            for f in cc_civil::desckeys::figures(&subset, &keys, &lc, None) {
                let g = groups.get(&f.group).and_then(Value::as_str).unwrap_or("points");
                if f.coded && g != "breakline" && g != "off" {
                    let mut flat: Vec<f64> = f.pts.iter().flat_map(|q| [q.x, q.y]).collect();
                    if f.closed
                        && let (Some(&x), Some(&y)) = (flat.first(), flat.get(1))
                    {
                        flat.push(x);
                        flat.push(y);
                    }
                    figures.push(flat);
                }
            }
        }
        Fixed { key, outline, figures, ms: (outline_ms, ms_since(t1)) }
    }
}

/// scene.js contours: minor / major layers, major labels every 60 paper mm (not on lines under 25 mm), masked.
fn contour_lines(b: &mut Builder, list: &[cc_civil::tin::Contour], interval: f64, major_n: f64, mm: f64, labels: bool, pal: &Palette) {
    let major = interval * if major_n != 0.0 && !major_n.is_nan() { major_n } else { 5.0 };
    let is_major = |z: f64| (z / major - (z / major).round()).abs() < 1e-6;
    // scene.js decimalsOf(interval)
    let dec = {
        let s = cc_civil::js::num_to_string(interval);
        s.find('.').map_or(0, |i| (s.len() - i - 1).min(3))
    };
    let upm = 1.0 / mm;
    let (cmaj, cmin, ctext) = (pal.layer("C-TOPO-MAJR"), pal.layer("C-TOPO-MINR"), pal.layer("C-TOPO-TEXT"));
    for c in list {
        let maj = is_major(c.z);
        let mut flat: Vec<f64> = c.pts.iter().flat_map(|q| [q.x, q.y]).collect();
        if c.closed
            && let (Some(&x), Some(&y)) = (flat.first(), flat.get(1))
        {
            flat.push(x);
            flat.push(y);
        }
        let layer = if maj { "C-TOPO-MAJR" } else { "C-TOPO-MINR" };
        b.line(layer, if maj { cmaj } else { cmin }, width_of(layer), flat);
        if maj && labels {
            let pts: Vec<(f64, f64)> = c.pts.iter().map(|q| (q.x, q.y)).collect();
            for (x, y, rot) in scene::labels_along(&pts, 60.0 * upm, 25.0 * upm) {
                b.labels.push(Label {
                    x,
                    y,
                    rot,
                    mm: 2.0,
                    align: 1,
                    middle: true,
                    mask: true,
                    color: ctext,
                    text: cc_civil::units::fixed(c.z, dec),
                });
            }
        }
    }
}

fn ms_since(t: web_time::Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// Survey point crosses (half size `k` world units) appended to the "V-NODE" batch's range in the GPU bytes, in the
/// line layout cc-render reads (x0 y0 x1 y1 relative to the origin as f32, then RGBA).
fn point_crosses(g: &mut SpaceGeometry, space: &Space, pts: &[cc_civil::points::Pt], k: f64, col: Color32) {
    let Some(bi) = space.batches.iter().position(|b| b.layer == "V-NODE") else { return };
    let (ox, oy) = g.origin;
    let rgba = [col.r(), col.g(), col.b(), 255];
    let l0 = g.segments();
    g.line_bytes.reserve(pts.len().saturating_mul(40));
    for q in pts {
        if !(q.x.is_finite() && q.y.is_finite()) {
            continue;
        }
        let (x, y) = (q.x - ox, q.y - oy);
        for v in [[x - k, y - k, x + k, y + k], [x - k, y + k, x + k, y - k]] {
            for c in v {
                g.line_bytes.extend_from_slice(&(c as f32).to_le_bytes());
            }
            g.line_bytes.extend_from_slice(&rgba);
        }
    }
    let l1 = g.segments();
    if l1 > l0 {
        let f = g.fill_vertices() as u32;
        g.batches.push(cc_render::BatchRange { batch: bi, lines: l0 as u32..l1 as u32, fills: f..f });
    }
}

/// The site's extents (civilcraft.js `extentsAll()`): surface, points, alignment PIs, pads, parcels, feature lines,
/// catchments.
pub fn extents(p: &mut Project) -> Option<(f64, f64, f64, f64)> {
    let mut lo = (f64::INFINITY, f64::INFINITY);
    let mut hi = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut g = |x: f64, y: f64| {
        if x.is_finite() && y.is_finite() {
            lo = (lo.0.min(x), lo.1.min(y));
            hi = (hi.0.max(x), hi.1.max(y));
        }
    };
    if let Some(s) = p.eg().surface.as_ref() {
        let b = s.bounds();
        g(b.min_x, b.min_y);
        g(b.max_x, b.max_y);
    }
    for q in &p.points {
        g(q.x, q.y);
    }
    let pts_of = |v: &Value| -> Vec<P2> { arr(v.get("pts")).iter().map(cc_civil::geo::read_p2).collect() };
    for a in arr(p.state.get("alignments")) {
        for q in arr(a.get("pis")) {
            g(numf(q.get("x")), numf(q.get("y")));
        }
    }
    for k in ["pads", "parcels", "features"] {
        for o in arr(p.state.get(k)) {
            for q in pts_of(o) {
                g(q.x, q.y);
            }
        }
    }
    if let Some(n) = p.state.get("network") {
        for o in arr(n.get("catchments")) {
            for q in pts_of(o) {
                g(q.x, q.y);
            }
        }
        for s in arr(n.get("structures")) {
            g(numf(s.get("x")), numf(s.get("y")));
        }
    }
    lo.0.is_finite().then_some((lo.0, lo.1, hi.0, hi.1))
}

// ------------------------------------------------------------------------------------------------ drawing

struct PlanCallback {
    id: u64,
    geometry: Arc<Vec<SpaceGeometry>>,
    /// per batch: its slot
    slots: Vec<u32>,
    uniforms: Vec<ViewUniform>,
}

impl egui_wgpu::CallbackTrait for PlanCallback {
    fn prepare(
        &self,
        device: &egui_wgpu::wgpu::Device,
        queue: &egui_wgpu::wgpu::Queue,
        _sd: &egui_wgpu::ScreenDescriptor,
        _enc: &mut egui_wgpu::wgpu::CommandEncoder,
        res: &mut egui_wgpu::CallbackResources,
    ) -> Vec<egui_wgpu::wgpu::CommandBuffer> {
        let Some(gpu) = res.get_mut::<GpuState>() else { return Vec::new() };
        // keep only this plan's mesh (a new bucket or edit = a new id)
        gpu.meshes.retain(|(id, _), _| *id == self.id);
        if !gpu.meshes.contains_key(&(self.id, 0))
            && let Some(g) = self.geometry.first()
        {
            gpu.meshes.insert((self.id, 0), SpaceMesh::upload(device, g));
        }
        for (slot, u) in self.uniforms.iter().enumerate() {
            gpu.renderer.set_view(queue, slot as u32, u);
        }
        Vec::new()
    }

    fn paint(&self, info: egui::PaintCallbackInfo, pass: &mut egui_wgpu::wgpu::RenderPass<'static>, res: &egui_wgpu::CallbackResources) {
        let Some(gpu) = res.get::<GpuState>() else { return };
        let Some(mesh) = gpu.meshes.get(&(self.id, 0)) else { return };
        let clip = info.clip_rect_in_pixels();
        pass.set_scissor_rect(
            clip.left_px.max(0) as u32,
            clip.top_px.max(0) as u32,
            clip.width_px.max(0) as u32,
            clip.height_px.max(0) as u32,
        );
        // layer by layer in the scene's order, each with its own width (slot)
        for (bi, slot) in self.slots.iter().enumerate() {
            gpu.renderer.draw(pass, mesh, *slot, &|b| b == bi);
        }
    }
}

fn rgba(c: Color32) -> [f32; 4] {
    let f = |u: u8| f32::from(u) / 255.0;
    [f(c.r()), f(c.g()), f(c.b()), 1.0]
}

/// Draw the plan into `rect` with `view` (physical pixels): GPU linework, then the labels.
pub fn paint(ui: &egui::Ui, rect: Rect, plan: &Plan, view: &View2D, pal: &Palette) -> usize {
    let ppp = ui.ctx().pixels_per_point();
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, pal.bg);
    let origin = plan.geometry.first().map_or((0.0, 0.0), |g| g.origin);
    // one slot per distinct width (the renderer has 256)
    let mut widths: Vec<f32> = Vec::new();
    let slots: Vec<u32> = plan
        .widths
        .iter()
        .map(|w| {
            let i = widths.iter().position(|x| (x - w).abs() < 1e-3).unwrap_or_else(|| {
                widths.push(*w);
                widths.len() - 1
            });
            (i as u32).min(cc_render::SLOTS as u32 - 1)
        })
        .collect();
    let uniforms: Vec<ViewUniform> = widths
        .iter()
        .map(|w| {
            let mut u = ViewUniform::new(view, origin, rgba(pal.ink), (w * ppp).max(0.6));
            u.fill_alpha = if pal.dark { 0.55 } else { 0.6 };
            u
        })
        .collect();
    let cb = PlanCallback { id: plan.id, geometry: Arc::clone(&plan.geometry), slots, uniforms };
    painter.add(egui_wgpu::Callback::new_paint_callback(rect, cb));
    // labels: screen space, culled to the view (+40 px), at their paper size (view.js `fs = mm·PX_PER_MM/0.72`)
    let mut n = 0usize;
    let budget = 30_000usize;
    let grow = rect.expand(40.0);
    for t in &plan.labels {
        if n >= budget {
            break;
        }
        let (sx, sy) = view.to_screen(t.x, t.y);
        let p = Pos2::new(rect.min.x + (sx as f32) / ppp, rect.min.y + (sy as f32) / ppp);
        if !grow.contains(p) || t.text.is_empty() {
            continue;
        }
        let fs = (t.mm * PX_PER_MM / 0.72) as f32;
        if !(fs.is_finite() && fs > 1.0) {
            continue;
        }
        let galley = painter.layout_no_wrap(t.text.clone(), egui::FontId::proportional(fs), t.color);
        let w = galley.size().x;
        let gh = galley.size().y;
        let ax = match t.align {
            1 => -w / 2.0,
            2 => -w,
            _ => 0.0,
        };
        // the canvas draws at the alphabetic baseline (+0.36 fs when vertically centred); egui places the top
        let ay = if t.middle { fs * 0.36 } else { 0.0 } - gh * 0.8;
        let angle = -(t.rot as f32);
        let rot = egui::emath::Rot2::from_angle(angle);
        if t.mask {
            let corners = [
                egui::vec2(ax - 2.0, ay + gh * 0.05),
                egui::vec2(ax + w + 2.0, ay + gh * 0.05),
                egui::vec2(ax + w + 2.0, ay + gh),
                egui::vec2(ax - 2.0, ay + gh),
            ];
            let pts: Vec<Pos2> = corners.iter().map(|c| p + rot * *c).collect();
            painter.add(egui::Shape::convex_polygon(pts, pal.bg, egui::Stroke::NONE));
        }
        let pos = p + rot * egui::vec2(ax, ay);
        painter.add(egui::epaint::TextShape::new(pos, galley, t.color).with_angle(angle));
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_are_powers_of_two() {
        assert_eq!(mm_bucket(PX_PER_MM), 1.0);
        assert_eq!(mm_bucket(PX_PER_MM * 3.0), 4.0);
        assert_eq!(mm_bucket(PX_PER_MM / 3.0), 0.25);
        assert_eq!(mm_bucket(f64::NAN), 1.0);
    }

    #[test]
    fn dashes_cover_on_parts_only() {
        let d = dashes(&[0.0, 0.0, 10.0, 0.0], 3.0, 2.0);
        assert_eq!(d.len(), 2, "{d:?}");
        assert_eq!(d.first().map(|x| x.as_slice()), Some([0.0, 0.0, 3.0, 0.0].as_slice()));
    }

    #[test]
    fn prefiltered_figures_match_the_engine_s() {
        let text = "1,100,100,10,EP B\n2,100,110,10,EP\n3,100,120,11,EP E\n4,105,100,10,GRD\n5,106,108,10,GRD\n7,90,95,9,TC B\n8,90,125,9,TC\n6,107,118,10,GRD\n";
        let mut p = Project::new();
        p.set_points(cc_civil::points::parse_points(text, "PNEZD", "PNEZD").points, "t.csv");
        let want: Vec<Vec<f64>> =
            p.figures(None).into_iter().filter(|f| f.coded).map(|f| f.pts.iter().flat_map(|q| [q.x, q.y]).collect()).collect();
        assert_eq!(want.len(), 2, "{want:?}");
        let fx = Fixed::make(&mut p, None, String::new());
        assert_eq!(fx.figures, want);
    }

    #[test]
    fn arcs_are_counter_clockwise() {
        let a = arc_flat(0.0, 0.0, 1.0, 0.0, std::f64::consts::FRAC_PI_2);
        let (x, y) = (a[a.len() - 2], a[a.len() - 1]);
        assert!(x.abs() < 1e-12 && (y - 1.0).abs() < 1e-12);
    }
}
