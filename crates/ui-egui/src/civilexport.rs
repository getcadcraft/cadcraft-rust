//! CivilCraft's exports in the app (M11.5): civilcraft.js `saveProject`, `exportLandXml`, `exportDxf`, `exportDwg`
//! and `plotDialog`, and site-view3d.js `exportGLB`, composed from the engine's writers exactly as 0.5 composes them,
//! so the files match the 0.5 app's byte for byte (the PDF: operator for operator).
//!
//! The DXF, DWG and PDF draw civilcraft.js `buildScene(mm)`: io-civil `plan_scene` over the whole project (every
//! survey point, the contours of the shown surface, …) plus the screen's items (sample lines, feature lines, parcels,
//! line-code figures, daylighted feature lines) — not the plan view's draw lists, which leave the heavy parts out.
//!
//! The clock comes from the caller (anything date-stamped: the project's `saved`, LandXML's date, the DWG's created
//! date, the PDF's title strip and info), so a test passes a fixed time. Map pictures in the DWG / PDF are left out:
//! the view has no GIS map layers yet.

use cc_civil::i18n::{F, t};
use cc_civil::js::{arr, numf};
use cc_civil::project::{Project, js_stringify};
use cc_civil::tr;
use cc_civil::units::grouped;
use cc_io_civil::landxml::{WAlignment, WNetwork, WStruct, WSurface};
use cc_io_civil::plot::{self, Ext, Scale};
use cc_io_civil::scene::{self, SceneAlignment, SceneCorridor, ScenePad, SceneState, Show};
use serde_json::{Map, Value, json};

use crate::civilview::CivilView;

/// DWG formats offered (civilcraft.js `DWG_YEAR`).
pub const DWG_YEAR: [(&str, &str); 6] =
    [("AC1015", "2000"), ("AC1018", "2004"), ("AC1021", "2007"), ("AC1024", "2010"), ("AC1027", "2013"), ("AC1032", "2018")];

/// The export choices the panel keeps (the DWG and plot dialogs' fields).
#[derive(Clone, Debug)]
pub struct ExportOpts {
    /// DWG version (AC1015 = 2000, the default)
    pub dwg: String,
    /// plot paper key; None = by units (a3 for metres, tabloid for feet)
    pub paper: Option<String>,
    /// plot scale label; None = the scale that fits
    pub scale: Option<String>,
    /// plot area: false = everything (extents), true = what is on screen
    pub area_view: bool,
    /// plot in black
    pub mono: bool,
    /// the site's extents for the panel's scale list, by project version
    ext_cache: Option<(u64, (f64, f64, f64, f64))>,
}

impl Default for ExportOpts {
    fn default() -> Self {
        ExportOpts { dwg: "AC1015".into(), paper: None, scale: None, area_view: false, mono: false, ext_cache: None }
    }
}

impl ExportOpts {
    /// The site's extents (a unit square when there is nothing), computed once per project version.
    pub fn extents(&mut self, p: &mut Project) -> Ext {
        let e = match self.ext_cache {
            Some((v, e)) if v == p.version => e,
            _ => {
                let e = extents_all(p).map_or((0.0, 0.0, 1.0, 1.0), |e| (e.min_x, e.min_y, e.max_x, e.max_y));
                self.ext_cache = Some((p.version, e));
                e
            }
        };
        Ext { min_x: e.0, min_y: e.1, max_x: e.2, max_y: e.3 }
    }
}

/// What an export made.
pub enum Outcome {
    /// a file to save (name, bytes) and what to say once it is saved
    File(String, Vec<u8>, String),
    /// nothing to export: what to say
    Nothing(String),
}

/// A parsed `civil.export` name: `kind[:options][@time ms]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub kind: String,
    pub opts: String,
    /// a fixed clock (tests): also makes the DWG's GUIDs a fixed sequence
    pub at: Option<f64>,
}

impl Spec {
    pub fn parse(s: &str) -> Result<Spec, String> {
        let s = s.trim();
        let (body, at) = match s.rsplit_once('@') {
            Some((b, t)) => {
                let ms: f64 = t.trim().parse().map_err(|_| format!("civil.export: @{t} is not a time in ms"))?;
                if !ms.is_finite() || ms.abs() > 8.64e15 {
                    return Err(format!("civil.export: @{t} is out of range"));
                }
                (b, Some(ms))
            }
            None => (s, None),
        };
        let (kind, opts) = body.split_once(':').map_or((body, ""), |(k, o)| (k, o));
        let kind = kind.trim().to_ascii_lowercase();
        if !matches!(kind.as_str(), "project" | "landxml" | "dxf" | "dwg" | "pdf" | "glb") {
            return Err(format!(
                "civil.export: name = project | landxml | dxf | dwg[:AC1018…] | pdf[:paper=…,scale=…,ink=mono,area=view] | glb, not {kind}"
            ));
        }
        Ok(Spec { kind, opts: opts.trim().to_string(), at })
    }
}

/// civilcraft.js `fileStem()`: the site's name made safe for a file name (runs of `\/:*?"<>|` and control characters
/// → `_`), trimmed, at most 80 UTF-16 units, else "site".
pub fn file_stem(name: &str) -> String {
    let src = if name.is_empty() { "site" } else { name };
    let s = safe(src, "_");
    let s = cut80(js_trim(&s));
    if s.is_empty() { "site".into() } else { s }
}

/// site-view3d.js `fileName(ext)`: runs of unsafe characters → a space, trimmed, 80 units, + `-3d.<ext>`.
pub fn file_name_3d(name: &str, ext: &str) -> String {
    let src = if name.is_empty() { "site" } else { name };
    format!("{}-3d.{ext}", cut80(js_trim(&safe(src, " "))))
}

fn safe(s: &str, with: &str) -> String {
    let bad = |c: char| matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || (c as u32) < 0x20;
    let mut out = String::with_capacity(s.len());
    let mut in_run = false;
    for c in s.chars() {
        if bad(c) {
            if !in_run {
                out.push_str(with);
            }
            in_run = true;
        } else {
            out.push(c);
            in_run = false;
        }
    }
    out
}

/// `String.prototype.trim` (whitespace and line terminators, the BOM included).
fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

/// `.slice(0, 80)` in UTF-16 units (a character that would straddle the cut is left out).
fn cut80(s: &str) -> String {
    let mut n = 0usize;
    let mut out = String::new();
    for c in s.chars() {
        n = n.saturating_add(c.len_utf16());
        if n > 80 {
            break;
        }
        out.push(c);
    }
    out
}

/// `new Date(ms).toISOString()`.
pub fn iso_time(ms: f64) -> String {
    let (y, mo, d, h, mi, s) = cc_io_civil::landxml::date_parts(ms);
    let milli = (ms - (ms / 1000.0).floor() * 1000.0).floor();
    let milli = if milli.is_finite() { milli.clamp(0.0, 999.0) as i64 } else { 0 };
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{milli:03}Z")
}

fn units_of(p: &Project) -> String {
    p.units().to_string()
}

fn site_name(p: &Project) -> String {
    p.state.get("name").and_then(Value::as_str).unwrap_or("").to_string()
}

fn notes_of(report: &Value) -> String {
    let n: Vec<String> = arr(report.get("notes")).iter().map(|x| cc_civil::js::string_of(Some(x))).collect();
    if n.is_empty() { String::new() } else { format!(" {}", n.join(" ")) }
}

/// A catalog fill from a JSON value (`String(v)`).
fn fill_of(v: Option<&Value>) -> F {
    match v {
        Some(Value::Number(_)) => F::N(numf(v)),
        Some(Value::String(s)) => F::S(s.clone()),
        Some(o) => F::S(cc_civil::js::string_of(Some(o))),
        None => F::S("undefined".into()),
    }
}

// ------------------------------------------------------------------------------------------------ project

/// civilcraft.js `saveProject`: `JSON.stringify(project.toJSON())` (`saved` = the time as an ISO string).
pub fn project_file(p: &Project, now_ms: f64) -> (String, Vec<u8>) {
    let j = p.to_json();
    let mut m = Map::new();
    if let Value::Object(src) = j {
        for (k, v) in src {
            let after_version = k == "version";
            m.insert(k, v);
            if after_version {
                m.insert("saved".into(), Value::String(iso_time(now_ms)));
            }
        }
    }
    let name = format!("{}.civilcraft.json", file_stem(&site_name(p)));
    (name, js_stringify(&Value::Object(m)).into_bytes())
}

// ------------------------------------------------------------------------------------------------ LandXML

/// civilcraft.js `exportLandXml`.
pub fn landxml(p: &mut Project, now_ms: f64, zone: cc_clock::Zone) -> Outcome {
    let eg = p.eg();
    let als: Vec<Value> = arr(p.state.get("alignments")).to_vec();
    if p.points.is_empty() && eg.surface.is_none() && als.is_empty() {
        return Outcome::Nothing(t("cv.export.none", &[]));
    }
    let surface_cfg = p.state.get("surface").cloned().unwrap_or(Value::Null);
    let eg_name =
        surface_cfg.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()).map_or_else(|| t("surface.eg", &[]), str::to_string);
    let graded = p.graded();
    let pads_n = arr(p.state.get("pads")).len();
    let breaklines = if eg.surface.is_some() { p.eg_breaklines() } else { Vec::new() };
    let mut surfaces: Vec<WSurface> = Vec::new();
    if let Some(s) = eg.surface.as_ref() {
        surfaces.push(WSurface {
            name: eg_name.clone(),
            desc: t("surface.eg", &[]),
            surface: Some(s),
            breaklines,
            boundary: surface_cfg.get("boundary").filter(|b| b.is_array()).map(|b| cc_civil::geo::read_p2s(Some(b))),
        });
    }
    if let Some(g) = graded.surface.as_deref() {
        surfaces.push(WSurface {
            name: t("cv.lx.proposed", &[]),
            desc: tr!("cv.lx.proposed.desc", n = pads_n),
            surface: Some(g),
            breaklines: Vec::new(),
            boundary: None,
        });
    }
    // the engine's pieces per alignment, held while the writer borrows them
    let built: Vec<_> = als
        .iter()
        .map(|a| {
            let al = p.alignment(a);
            let egp = if eg.surface.is_some() { Some(p.eg_profile(a)) } else { None };
            let pr = if arr(a.get("pvis")).len() >= 2 { Some(p.design_profile(a)) } else { None };
            (al, egp, pr)
        })
        .collect();
    let alignments: Vec<WAlignment> = als
        .iter()
        .zip(built.iter())
        .map(|(a, (al, egp, pr))| {
            let name = cc_civil::js::string_or_empty(a.get("name"));
            WAlignment {
                eg: egp.as_ref().map(|e| (eg_name.clone(), e.as_ref().clone())),
                profile: pr.as_ref().map(|pp| (tr!("cv.lx.design", name = name.as_str()), pp.as_ref())),
                name,
                alignment: Some(al.as_ref()),
            }
        })
        .collect();
    let network = p.state.get("network").filter(|n| cc_civil::js::truthy(Some(n))).cloned();
    let check = if network.is_some() { p.network_check() } else { None };
    let networks: Vec<WNetwork> = network
        .iter()
        .map(|n| {
            let net = cc_civil::pipes::read_network(n);
            let structures = arr(n.get("structures"))
                .iter()
                .map(|s| {
                    let id = cc_civil::js::string_or_empty(s.get("id"));
                    let info = check.as_ref().and_then(|c| c.structs.iter().find(|(k, _)| *k == id).map(|(_, i)| i));
                    WStruct {
                        name: cc_civil::js::string_or_empty(s.get("name")),
                        x: numf(s.get("x")),
                        y: numf(s.get("y")),
                        dia: numf(s.get("dia")),
                        rim_shown: info.and_then(|i| i.rim),
                        sump: info.and_then(|i| i.sump),
                        id,
                    }
                })
                .collect();
            WNetwork { name: net.name, ty: net.ty, structures, pipes: net.pipes }
        })
        .collect();
    let units = units_of(p);
    let name = p.state.get("name").and_then(Value::as_str).unwrap_or("CivilCraft project").to_string();
    let crs = p.state.get("crs").filter(|c| !c.is_null());
    let xml = cc_io_civil::landxml::write_land_xml(&name, &units, &p.points, &surfaces, &alignments, &networks, zone.local_ms(now_ms), crs);
    let skipped = built.iter().filter(|(al, _, _)| !al.ok()).count();
    let als_t = tr!("cv.als.n", n = als.len().saturating_sub(skipped));
    let n_pts = p.points.len();
    let mut list = vec![
        tr!("cv.pts.n", n = n_pts, count = grouped(n_pts as f64, 0)),
        tr!("cv.surfaces.n", n = surfaces.len()),
        if skipped > 0 { tr!("cv.lx.als.skipped", alignments = als_t, n = skipped) } else { als_t },
    ];
    if network.is_some() {
        list.push(t("cv.lx.network", &[]));
    }
    let msg = tr!("cv.lx.exported", list = list.join(", "));
    Outcome::File(format!("{}.xml", file_stem(&site_name(p))), xml.into_bytes(), msg)
}

// ------------------------------------------------------------------------------------------------ the plan scene

/// civilcraft.js `extentsAll()`: surface bounds, survey points, alignment PIs, pads, parcels, catchments, feature
/// lines (no map layers in the view yet).
pub fn extents_all(p: &mut Project) -> Option<Ext> {
    let mut b = Ext { min_x: f64::INFINITY, min_y: f64::INFINITY, max_x: f64::NEG_INFINITY, max_y: f64::NEG_INFINITY };
    let mut g = |x: f64, y: f64| {
        b.min_x = b.min_x.min(x);
        b.max_x = b.max_x.max(x);
        b.min_y = b.min_y.min(y);
        b.max_y = b.max_y.max(y);
    };
    if let Some(s) = p.eg().surface.as_ref() {
        let bb = s.bounds();
        g(bb.min_x, bb.min_y);
        g(bb.max_x, bb.max_y);
    }
    for q in &p.points {
        g(q.x, q.y);
    }
    for a in arr(p.state.get("alignments")) {
        for q in arr(a.get("pis")) {
            g(numf(q.get("x")), numf(q.get("y")));
        }
    }
    let mut lists: Vec<&Value> = Vec::new();
    lists.extend(arr(p.state.get("pads")));
    lists.extend(arr(p.state.get("parcels")));
    if let Some(n) = p.state.get("network") {
        lists.extend(arr(n.get("catchments")));
    }
    lists.extend(arr(p.state.get("features")));
    for d in lists {
        for q in arr(d.get("pts")) {
            g(numf(q.get("x")), numf(q.get("y")));
        }
    }
    // civilcraft.js extentsAll: the GIS parts' extents (gis-vector-ui.js: the visible layers)
    if let Some(gb) = cc_io_civil::gis_layers::gis_extents(&crate::civilgis::layers_of(&p.state), &p.gis_features)
        && gb.min_x.is_finite()
    {
        g(gb.min_x, gb.min_y);
        g(gb.max_x, gb.max_y);
    }
    b.min_x.is_finite().then_some(b)
}

fn shown_fg(p: &mut Project) -> bool {
    crate::civilplan::shown_is_fg(p)
}

/// civilcraft.js `buildScene(mm)`: the plan scene the DXF, DWG and PDF draw.
pub fn build_scene(p: &mut Project, mm: f64, labels: &str) -> Vec<Value> {
    let d = p.state.get("display").cloned().unwrap_or(Value::Null);
    let units = units_of(p);
    let fg = shown_fg(p);
    let eg = p.eg();
    let graded = p.graded();
    let surface = if fg { graded.surface.as_deref() } else { eg.surface.as_ref() };
    let contour_list = if surface.is_none() {
        Vec::new()
    } else if fg {
        p.contours_fg().as_ref().clone()
    } else {
        p.contours().as_ref().clone()
    };
    let mut breaklines: Vec<Vec<Value>> = Vec::new();
    if !fg {
        for b in p.eg_breaklines() {
            breaklines.push(b.pts.iter().map(cc_civil::js::J::j).collect());
        }
    }
    if let Some(bnd) = p.state.get("surface").and_then(|s| s.get("boundary")).and_then(Value::as_array) {
        let mut v = bnd.clone();
        v.push(bnd.first().cloned().unwrap_or(Value::Null));
        breaklines.push(v);
    }
    let als: Vec<Value> = arr(p.state.get("alignments")).to_vec();
    let al_built: Vec<_> = als.iter().map(|a| p.alignment(a)).collect();
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
            // gr.items.find((x) => x.id === p.id)
            let id = pd.get("id").cloned().unwrap_or(Value::Null);
            let g = graded.items.iter().find(|x| x.id == id);
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
                .map(|ls| ls.iter().map(|(k, v)| (k.clone(), v.iter().map(cc_civil::js::J::j).collect())).collect())
                .unwrap_or_default(),
            sections: arr(x.info.get("sections")).iter().map(|s| (arr(s.get("L")).to_vec(), arr(s.get("R")).to_vec())).collect(),
        })
        .collect();
    let network = p.state.get("network").filter(|n| n.is_object()).cloned().map(|n| (cc_civil::pipes::read_network(&n), p.network_check()));
    let st = SceneState {
        units: units.clone(),
        points: p.points.clone(),
        surface,
        contours: Some((cc_civil::js::number(d.get("interval")), cc_civil::js::number(d.get("major")), contour_list)),
        breaklines,
        alignments,
        pads,
        corridors,
        network,
    };
    let b = |k: &str| d.get(k).and_then(Value::as_bool);
    let show = Show {
        triangles: b("triangles"),
        contours: b("contours"),
        labels: b("labels"),
        breaklines: b("breaklines"),
        outline: b("outline"),
        points: b("points"),
        point_labels: b("pointLabels"),
        slopes: b("slopes"),
        alignments: b("alignments"),
        pads: b("pads"),
        pipes: b("pipes"),
        // Settings › CivilCraft › point labels (0.5's default: number, elevation, description)
        point_label_text: Some(labels.into()),
    };
    let mut items = scene::plan_scene(&st, mm, &show);
    drop(st);
    // 0.4 V13 sample lines
    if d.get("alignments") != Some(&Value::Bool(false)) {
        for a in &als {
            if let Some(set) = p.sections(a) {
                for s in &set.sections {
                    items.push(
                        json!({ "layer": "C-ROAD-SCTN", "kind": "poly", "pts": [cc_civil::js::J::j(&s.a), cc_civil::js::J::j(&s.b)] }),
                    );
                }
            }
        }
    }
    // 0.4 V14 feature lines (z kept: 3D polylines in DXF / DWG)
    for f in arr(p.state.get("features")) {
        let mut o = Map::new();
        o.insert("layer".into(), json!("C-GRAD-FEAT"));
        o.insert("kind".into(), json!("poly"));
        o.insert("pts".into(), f.get("pts").cloned().unwrap_or(Value::Null));
        if let Some(c) = f.get("closed") {
            o.insert("closed".into(), c.clone());
        }
        o.insert("z".into(), Value::Bool(true));
        items.push(Value::Object(o));
    }
    // 0.4 V21 parcels, their courses and areas
    for pc in arr(p.state.get("parcels")) {
        items.extend(cc_civil::parcels::parcel_labels(&cc_civil::parcels::read_parcel(pc), &units, 1.0 / mm));
    }
    // 0.4 V16 lines from line codes (groups that are not breaklines)
    if cc_civil::js::truthy(d.get("points")) && !p.points.is_empty() {
        let groups = p.state.get("surface").and_then(|s| s.get("groups")).cloned().unwrap_or(Value::Null);
        for f in p.figures(None) {
            let g = groups.get(&f.group).and_then(Value::as_str);
            if f.coded && g != Some("breakline") && g != Some("off") {
                let pts: Vec<Value> = f.pts.iter().map(|q| json!({ "x": q.x, "y": q.y, "z": q.z })).collect();
                items.push(json!({ "layer": "V-NODE-LINE", "kind": "poly", "pts": pts, "closed": f.closed, "z": true }));
            }
        }
    }
    // 0.4 V14: a daylighted feature line's slopes, as a pad's
    for it in graded.items.iter().filter(|x| x.kind == "feature") {
        let dl = arr(it.info.get("daylight"));
        if dl.is_empty() {
            continue;
        }
        items.push(json!({ "layer": "C-GRAD-DAYL", "kind": "poly", "pts": dl, "closed": true, "z": true }));
        for r in arr(it.info.get("rays")) {
            if numf(r.get("d")) > 1e-6 {
                items.push(json!({ "layer": "C-GRAD-SLOP", "kind": "poly",
                    "pts": [{ "x": r.get("x0").cloned().unwrap_or(Value::Null), "y": r.get("y0").cloned().unwrap_or(Value::Null) },
                            { "x": r.get("x").cloned().unwrap_or(Value::Null), "y": r.get("y").cloned().unwrap_or(Value::Null) }] }));
            }
        }
    }
    // gisReg.scene (gis-vector-ui.js): the GIS layers' items last (their layers go first in the scene's list)
    let gl = crate::civilgis::layers_of(&p.state);
    if !gl.is_empty() {
        let g = cc_io_civil::gis_layers::gis_scene(&gl, &p.gis_features, mm);
        items.extend(arr(g.get("items")).iter().cloned());
    }
    items
}

fn sheet_paper(units: &str) -> &'static str {
    if units == "m" { "a3" } else { "tabloid" }
}

fn layer_values() -> Vec<Value> {
    scene::LAYERS.iter().map(|(n, _, a, _)| json!({ "name": n, "color": a })).collect()
}

// ------------------------------------------------------------------------------------------------ DXF, DWG

/// civilcraft.js `exportDxf`: the plan scene at the scale that fits A3 (metres) / tabloid (feet), as R12 DXF.
pub fn dxf(p: &mut Project, labels: &str) -> Outcome {
    let Some(ext) = extents_all(p) else { return Outcome::Nothing(t("cv.export.none", &[])) };
    let units = units_of(p);
    let scale = plot::fit_scale(&units, &ext, &plot::paper_by(sheet_paper(&units)), true);
    let sc = build_scene(p, scale.mm_per_unit, labels);
    let ents = scene::to_dxf_entities(&sc, scale.mm_per_unit);
    let (text, report) = cc_io_civil::dxf::write_dxf(&units, &cc_io_civil::dxf::scene_layers(), &ents);
    let n = sc.len();
    let msg = format!(
        "{}{} {}",
        tr!("cv.dxf.done", n = n, count = grouped(n as f64, 0), scale = scale.label.as_str()),
        notes_of(&report),
        t("cv.bands.no", &[])
    );
    Outcome::File(format!("{}.dxf", file_stem(&site_name(p))), text.into_bytes(), msg)
}

/// civilcraft.js `exportDwg(version)`: the DXF's drawing as a DWG of that year (`seq_guids`: the GUIDs a fixed
/// sequence, for tests; else random as 0.5's).
pub fn dwg(p: &mut Project, version: &str, now_ms: f64, seq_guids: bool, labels: &str) -> Result<Outcome, String> {
    let Some(year) = DWG_YEAR.iter().find(|(v, _)| *v == version).map(|(_, y)| *y) else {
        return Err(format!("civil.export: DWG version {version} (AC1015, AC1018, AC1021, AC1024, AC1027 or AC1032)"));
    };
    let Some(ext) = extents_all(p) else { return Ok(Outcome::Nothing(t("cv.export.none", &[]))) };
    let units = units_of(p);
    let scale = plot::fit_scale(&units, &ext, &plot::paper_by(sheet_paper(&units)), true);
    let sc = build_scene(p, scale.mm_per_unit, labels);
    let ents = scene::to_dxf_entities(&sc, scale.mm_per_unit);
    drop(sc);
    let (bytes, report) = cc_io_civil::dwg_write::write_dwg(Some(&units), &layer_values(), &ents, now_ms, version, &[], seq_guids)?;
    let c = report.get("counts");
    let parts: Vec<String> = [
        ("lwpolyline", "cv.dwg.n.poly"),
        ("polyline3d", "cv.dwg.n.poly3d"),
        ("arc", "cv.dwg.n.arc"),
        ("text", "cv.dwg.n.label"),
        ("point", "cv.pts.n"),
    ]
    .iter()
    .filter_map(|(k, key)| {
        let n = c.and_then(|c| c.get(*k)).map_or(0.0, |v| numf(Some(v)));
        (n != 0.0 && n.is_finite()).then(|| tr!(key, n = n, count = grouped(n, 0)))
    })
    .collect();
    let msg = format!(
        "{}{} {}",
        tr!("cv.dwg.done", year = year, list = parts.join(", "), n = arr(report.get("layers")).len(), scale = scale.label.as_str()),
        notes_of(&report),
        t("cv.bands.no", &[])
    );
    Ok(Outcome::File(format!("{}.dwg", file_stem(&site_name(p))), bytes, msg))
}

// ------------------------------------------------------------------------------------------------ plot

/// The plot's choices resolved (civilcraft.js `plotDialog` fields).
pub struct PlotChoice {
    pub paper: String,
    pub area: Ext,
    pub scale: Scale,
    pub fits: String,
    pub mono: bool,
}

/// Read `paper=…,scale=…,ink=mono|color,area=view|all` over the panel's choices.
pub fn plot_choice(cv: &mut CivilView, opts: &str) -> Result<Option<PlotChoice>, String> {
    let mut o = cv.export.clone();
    for kv in opts.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (k, v) = kv.split_once('=').ok_or_else(|| format!("civil.export: pdf option {kv} (key=value)"))?;
        let v = v.trim();
        match k.trim() {
            "paper" => o.paper = Some(v.to_string()),
            "scale" => o.scale = Some(v.to_string()),
            "ink" => o.mono = v == "mono",
            "area" => o.area_view = v == "view",
            other => return Err(format!("civil.export: unknown pdf option {other} (paper, scale, ink, area)")),
        }
    }
    let Some(ext) = extents_all(&mut cv.project) else { return Ok(None) };
    let units = units_of(&cv.project);
    let paper = o.paper.unwrap_or_else(|| sheet_paper(&units).to_string());
    if !plot::PAPERS.iter().any(|p| p.key == paper) {
        let keys: Vec<&str> = plot::PAPERS.iter().map(|p| p.key).collect();
        return Err(format!("civil.export: paper {paper} ({})", keys.join(", ")));
    }
    let area = if o.area_view { view_bounds(&cv.view) } else { ext };
    let fit = plot::fit_scale(&units, &area, &plot::paper_by(&paper), true);
    let scale = match o.scale.as_deref() {
        None | Some("") => fit.clone(),
        Some(l) => plot::scales_for(&units)
            .into_iter()
            .find(|s| s.label == l)
            .ok_or_else(|| format!("civil.export: scale {l} is not one of {}'s", if units == "m" { "metres" } else { "feet" }))?,
    };
    Ok(Some(PlotChoice { paper, area, scale, fits: fit.label, mono: o.mono }))
}

/// view.js `viewBounds()`: the world rectangle on screen.
pub fn view_bounds(v: &cc_render::View2D) -> Ext {
    let corners = [v.to_world(0.0, 0.0), v.to_world(v.width, 0.0), v.to_world(0.0, v.height), v.to_world(v.width, v.height)];
    let mut e = Ext { min_x: f64::INFINITY, min_y: f64::INFINITY, max_x: f64::NEG_INFINITY, max_y: f64::NEG_INFINITY };
    for (x, y) in corners {
        e.min_x = e.min_x.min(x);
        e.max_x = e.max_x.max(x);
        e.min_y = e.min_y.min(y);
        e.max_y = e.max_y.max(y);
    }
    e
}

/// civilcraft.js `plotDialog` "Download PDF": the plan scene on one sheet.
pub fn pdf(p: &mut Project, c: &PlotChoice, now_ms: f64, labels: &str) -> Outcome {
    let units = units_of(p);
    let sc = build_scene(p, c.scale.mm_per_unit, labels);
    let d = p.state.get("display").cloned().unwrap_or(Value::Null);
    let mut notes = Vec::new();
    let fg = shown_fg(p);
    let has_surface = if fg { true } else { p.eg().surface.is_some() };
    if cc_civil::js::truthy(d.get("contours")) && has_surface {
        notes.push(cc_civil::i18n::t(
            if fg { "cv.plot.contours.fg" } else { "cv.plot.contours.eg" },
            &[("i", fill_of(d.get("interval"))), ("u", F::S(cc_civil::units::unit_of(&units).short.to_string()))],
        ));
    }
    let title = site_name(p);
    let sheet = plot::plan_sheet(&sc, &c.area, &c.paper, true, &c.scale, Some(&title), &units, &notes, &[], &[], now_ms);
    drop(sc);
    let (bytes, report) = plot::to_pdf(&sheet, c.mono, now_ms);
    let stem = file_stem(&site_name(p));
    let missing = report.get("missing").map_or(0.0, |v| numf(Some(v)));
    let mut msg = tr!("cv.plot.done", file = format!("{stem}.pdf"), scale = c.scale.label.as_str());
    if missing > 0.0 {
        msg.push(' ');
        msg.push_str(&tr!("pp.ui.missing", n = missing));
    }
    Outcome::File(format!("{stem}.pdf"), bytes, msg)
}

// ------------------------------------------------------------------------------------------------ GLB

/// site-view3d.js `exportGLB()`: the 3D site scene (as the view builds it) as GLB.
pub fn glb(p: &mut Project, exag: f64, dark: bool) -> Outcome {
    let input = cc_civil::site3d::site_input(p);
    let opts = cc_civil::site3d::Opts {
        exag: Some(json!(exag)),
        color: Some("bands".into()),
        band_count: None,
        dark,
        colors: Vec::new(),
        show: cc_civil::site3d::Show::default(),
        drape: None,
    };
    let sc = cc_civil::site3d::build_site3d(&input, &opts);
    drop(input);
    let name = site_name(p);
    let bytes = cc_io_civil::site3d_export::export_glb(&sc, &name);
    let file = file_name_3d(&name, "glb");
    let msg = tr!("v3d.exported", file = file.as_str(), e = sc.origin.x, n = sc.origin.y, z = sc.origin.z);
    Outcome::File(file, bytes, msg)
}

// ------------------------------------------------------------------------------------------------ the command

/// `civil.export` on the open project → what to save (or what to say). `now_ms` is the clock when the name has no
/// `@time`.
pub fn run(cv: &mut CivilView, name: &str, now_ms: f64, dark: bool, zone: cc_clock::Zone) -> Result<Outcome, String> {
    let spec = Spec::parse(name)?;
    let now = spec.at.unwrap_or(now_ms);
    let t0 = web_time::Instant::now();
    let out = match spec.kind.as_str() {
        "project" => {
            let (n, b) = project_file(&cv.project, now);
            let msg =
                if cfg!(target_arch = "wasm32") { tr!("cv.saved.dl", name = n.as_str()) } else { tr!("save.written", name = n.as_str()) };
            Outcome::File(n, b, msg)
        }
        "landxml" => landxml(&mut cv.project, now, zone),
        "dxf" => dxf(&mut cv.project, &cv.pt_settings.labels),
        "dwg" => {
            let v = if spec.opts.is_empty() { cv.export.dwg.clone() } else { spec.opts.clone() };
            // a year is fine too (2004 → AC1018)
            let v = DWG_YEAR.iter().find(|(_, y)| *y == v).map_or(v.to_ascii_uppercase(), |(k, _)| (*k).to_string());
            dwg(&mut cv.project, &v, now, spec.at.is_some(), &cv.pt_settings.labels)?
        }
        "pdf" => match plot_choice(cv, &spec.opts)? {
            Some(c) => pdf(&mut cv.project, &c, now, &cv.pt_settings.labels),
            None => Outcome::Nothing(t("cv.plot.none", &[])),
        },
        "glb" => glb(&mut cv.project, cv.exag, dark),
        _ => return Err(format!("civil.export: unknown kind {}", spec.kind)),
    };
    cv.timings.insert("export", t0.elapsed().as_secs_f64() * 1000.0);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems_follow_file_stem() {
        assert_eq!(file_stem(""), "site");
        assert_eq!(file_stem("  a/b::c  "), "a_b_c");
        assert_eq!(file_stem("///"), "_");
        assert_eq!(file_stem(" \t "), "_");
        assert_eq!(file_stem("   "), "site");
        assert_eq!(file_stem(&"x".repeat(100)).len(), 80);
        assert_eq!(file_name_3d("Main: St", "glb"), "Main  St-3d.glb");
        assert_eq!(cut80(&"😀".repeat(50)).chars().count(), 40);
    }

    #[test]
    fn iso_times() {
        assert_eq!(iso_time(0.0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_time(1_760_700_000_123.0), "2025-10-17T11:20:00.123Z");
    }

    #[test]
    fn specs_parse_and_refuse() {
        assert_eq!(Spec::parse("dwg:AC1018@5").unwrap(), Spec { kind: "dwg".into(), opts: "AC1018".into(), at: Some(5.0) });
        assert_eq!(Spec::parse("pdf:scale=1:500").unwrap().opts, "scale=1:500");
        assert!(Spec::parse("svg").is_err());
        assert!(Spec::parse("dxf@soon").is_err());
        assert!(Spec::parse("dxf@1e300").is_err());
    }

    #[test]
    fn the_demo_exports_everything() {
        let mut cv = CivilView::demo().unwrap();
        for k in ["project", "landxml", "dxf", "dwg", "dwg:2018", "pdf", "pdf:paper=a1,ink=mono,area=view", "glb"] {
            match run(&mut cv, &format!("{k}@1760700000000"), 0.0, true, cc_clock::Zone::Utc).unwrap() {
                Outcome::File(name, bytes, msg) => {
                    assert!(!bytes.is_empty() && !msg.is_empty(), "{k}");
                    assert!(name.starts_with("Demo site"), "{k}: {name}");
                }
                Outcome::Nothing(m) => panic!("{k}: {m}"),
            }
        }
        assert!(run(&mut cv, "dwg:AC9999", 0.0, true, cc_clock::Zone::Utc).is_err());
        assert!(run(&mut cv, "pdf:paper=b5", 0.0, true, cc_clock::Zone::Utc).is_err());
        assert!(run(&mut cv, "pdf:scale=1:3", 0.0, true, cc_clock::Zone::Utc).is_err());
    }

    #[test]
    fn exports_are_stable_with_a_fixed_clock() {
        let mut cv = CivilView::demo().unwrap();
        for k in ["project", "landxml", "dxf", "dwg", "pdf"] {
            let a = run(&mut cv, &format!("{k}@1760700000000"), 0.0, true, cc_clock::Zone::Utc).unwrap();
            let b = run(&mut cv, &format!("{k}@1760700000000"), 9.0, true, cc_clock::Zone::Utc).unwrap();
            let (Outcome::File(_, x, _), Outcome::File(_, y, _)) = (a, b) else { panic!("{k}") };
            assert_eq!(x, y, "{k}");
        }
        let Outcome::File(_, b, _) = run(&mut cv, "project@1760700000000", 0.0, true, cc_clock::Zone::Utc).unwrap() else { panic!() };
        let s = String::from_utf8(b).unwrap();
        assert!(s.starts_with("{\"format\":\"civilcraft-project\",\"version\":1,\"saved\":\"2025-10-17T11:20:00.000Z\","), "{}", &s[..120]);
        let Outcome::File(_, b, _) = run(&mut cv, "pdf@1760700000000", 0.0, true, cc_clock::Zone::Utc).unwrap() else { panic!() };
        assert!(b.windows(17).any(|w| w == b"D:20251017112000Z"));
    }

    #[test]
    fn an_empty_project_has_nothing_to_export() {
        let mut cv = CivilView::new(Project::new(), "empty.civilcraft.json");
        for k in ["landxml", "dxf", "dwg", "pdf"] {
            assert!(matches!(run(&mut cv, k, 0.0, true, cc_clock::Zone::Utc).unwrap(), Outcome::Nothing(_)), "{k}");
        }
    }
}
