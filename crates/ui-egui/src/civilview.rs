//! CivilCraft in the app (M11): a project opened from the files 0.5 opens — its own `.civilcraft.json`, LandXML,
//! survey point files (CSV / text, PNEZD and friends) — shown on a plan (civilplan.rs) or as the 3D site scene
//! (civil3d.rs), with the left and right panels (civilpanel.rs). The engine (cc-civil / cc-io-civil) does every
//! computation; this module keeps the view, the selection and per-version caches, and maps the `civil.*` UI commands.
//!
//! Large coordinates: the view is f64 (cc-render `View2D`), picking and hover run in f64 world units, the GPU gets
//! vertices relative to the plan's centre; the 3D scene is relative to the engine's site origin.

use std::collections::HashMap;

use cc_civil::i18n::t;
use cc_civil::js::{arr, numf};
use cc_civil::project::Project;
use cc_civil::tr;
use cc_civil::units::fixed;
use cc_render::View2D;
use egui::{Color32, Pos2, Rect, RichText, Sense};
use serde_json::{Value, json};

use crate::civil3d::{self, Site3};
use crate::civilplan::{self, Palette, Plan, PlanKey};
use crate::theme::{Preset, Tokens};
use crate::{ImportedFile, Params, UiApp};

/// CivilCraft 0.5's sample (`civilcraft/dev/sample-site.csv`: 276 survey points at state-plane coordinates).
pub static SAMPLE_CSV: &str = include_str!("../../../assets/samples/civilcraft-sample-site.csv");

/// Largest text file opened (civilcraft.js: 200 MB).
pub const MAX_TEXT_BYTES: usize = 200 * 1024 * 1024;

/// The `civil.*` UI commands (id, label, shortcut, params doc).
pub const COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("civil.sample", "Open the sample site", "", "{} → CivilCraft 0.5's sample-site.csv (276 survey points at state-plane coordinates)"),
    (
        "civil.demo",
        "Open the demo site",
        "",
        "{} → the sample site plus an alignment with a profile and corridor, a pad, a pipe network, a parcel and a feature line made in code",
    ),
    (
        "civil.synthetic",
        "Synthetic survey",
        "",
        "{factor: number of points, 100…2000000} → survey points on a smooth hill at x ≈ 500 000, y ≈ 4 000 000 (timings in the status line)",
    ),
    ("civil.view", "Plan or 3D", "", "{name?: \"plan\"|\"3d\"} → shows the plan or the 3D site scene (no name: switches)"),
    (
        "civil.display",
        "Display setting",
        "",
        "{name: \"key\" | \"key=value\"} → switches contours, triangles, points, pointLabels, labels, breaklines, outline, pipes, slopes, bands; sets interval, major, bandCount, surface (eg|fg)",
    ),
    (
        "civil.select",
        "Select",
        "",
        "{name: \"kind:id\" | \"\"} → alignment:a1, pad:p2, structure:s3, pipe:q4, feature:f5, parcel:r6, point:<number>; empty clears",
    ),
    ("civil.pick", "Click on the plan", "", "{name: \"x,y\"} → world coordinates: selects what a click there selects"),
    ("civil.hover", "Point at the plan", "", "{name: \"x,y\"} → world coordinates: the status line reads E/N, ground, graded, station"),
    (
        "civil.goto",
        "Centre the plan",
        "",
        "{name: \"x,y\" | \"x,y,zoom\"} → centres the plan on a world point (zoom: screen pixels per unit)",
    ),
    ("civil.group", "Point group use", "", "{name: \"GROUP=points|breakline|off\"} → how a point group builds the surface (one undo step)"),
    ("civil.undo", "Undo", "Ctrl+Z", "{}"),
    ("civil.redo", "Redo", "Ctrl+Y", "{}"),
    ("civil.camera", "3D view direction", "", "{name: iso|top|front|back|left|right}"),
    ("civil.exag", "Vertical exaggeration", "", "{factor: 1|2|3|5|10} → the 3D scene's vertical exaggeration"),
    (
        "civil.timings",
        "Timings",
        "",
        "{} → the status line lists the last open / parse / TIN / plan build (by stage) / 3D build / panel facts times in ms",
    ),
    ("civil.save", "Save the project", "Ctrl+S", "{} → the .civilcraft.json project file (0.5's format)"),
    (
        "civil.export",
        "Export",
        "",
        "{name: \"project\"|\"landxml\"|\"dxf\"|\"dwg[:AC1015|AC1018|AC1021|AC1024|AC1027|AC1032]\"|\"pdf[:paper=…,scale=…,ink=mono|color,area=view|all]\"|\"glb\" + optional \"@<time ms>\" (a fixed clock; DWG GUIDs then a fixed sequence)} → the file 0.5 writes, through the save service",
    ),
];

/// What is selected (civilcraft.js `sel` / `picked`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sel {
    Alignment(String),
    Pad(String),
    Structure(String),
    Pipe(String),
    Feature(String),
    Parcel(String),
    Point(String),
}

impl Sel {
    pub fn parse(s: &str) -> Option<Sel> {
        let (k, id) = s.split_once(':')?;
        let id = id.trim().to_string();
        if id.is_empty() {
            return None;
        }
        Some(match k.trim() {
            "alignment" => Sel::Alignment(id),
            "pad" => Sel::Pad(id),
            "structure" => Sel::Structure(id),
            "pipe" => Sel::Pipe(id),
            "feature" => Sel::Feature(id),
            "parcel" => Sel::Parcel(id),
            "point" => Sel::Point(id),
            _ => return None,
        })
    }
    pub fn key(&self) -> String {
        match self {
            Sel::Alignment(i) => format!("alignment:{i}"),
            Sel::Pad(i) => format!("pad:{i}"),
            Sel::Structure(i) => format!("structure:{i}"),
            Sel::Pipe(i) => format!("pipe:{i}"),
            Sel::Feature(i) => format!("feature:{i}"),
            Sel::Parcel(i) => format!("parcel:{i}"),
            Sel::Point(i) => format!("point:{i}"),
        }
    }
}

/// Facts the panels show, computed once per project version (volumes and the network check are not memoised by the
/// engine: the panels would otherwise recompute them every frame).
#[derive(Default)]
pub struct Facts {
    pub version: u64,
    pub volumes: Option<Value>,
    pub network: Option<cc_civil::pipes::Analysis>,
    pub breaklines: usize,
    pub groups: Vec<(String, usize, String)>,
    pub graded_problems: Vec<String>,
    pub ms: f64,
}

/// One open CivilCraft project and its view.
pub struct CivilView {
    pub project: Project,
    pub name: String,
    /// the plan view (physical pixels)
    pub view: View2D,
    pending_fit: bool,
    plan: Option<Plan>,
    fixed: Option<civilplan::Fixed>,
    pub sel: Option<Sel>,
    pub hover_pt: Option<usize>,
    /// the 3D scene instead of the plan
    pub show3d: bool,
    pub site3: Option<Site3>,
    pub exag: f64,
    s3_seq: u64,
    /// world point under the pointer (E, N)
    pub cursor: Option<(f64, f64)>,
    /// the last thing said (civilcraft.js `say`)
    pub message: String,
    /// notes of the last open (civilcraft.js `lastReport`)
    pub report: Vec<String>,
    pub facts: Facts,
    /// the last plan frame: labels drawn, build ms
    pub last_labels: usize,
    /// scene build timings (ms) by step, for the status line and the benchmark
    pub timings: HashMap<&'static str, f64>,
    /// the Export group's choices (DWG format, plot paper / scale / area / ink)
    pub export: crate::civilexport::ExportOpts,
    dark: bool,
}

/// Is this a file CivilCraft opens? (civilcraft.js `openText`: JSON = project, XML = LandXML, else points.)
pub fn is_civil_file(lower: &str, bytes: &[u8]) -> bool {
    if lower.ends_with(".civilcraft.json") || lower.ends_with(".landxml") {
        return true;
    }
    let head: String = String::from_utf8_lossy(bytes.get(..bytes.len().min(400)).unwrap_or(&[])).trim_start().chars().take(200).collect();
    if lower.ends_with(".json") {
        return head.starts_with('{')
            && String::from_utf8_lossy(bytes.get(..bytes.len().min(4096)).unwrap_or(&[])).contains("civilcraft-project");
    }
    if lower.ends_with(".xml") {
        return head.contains("LandXML") || String::from_utf8_lossy(bytes.get(..bytes.len().min(4096)).unwrap_or(&[])).contains("<LandXML");
    }
    [".csv", ".txt", ".pnezd", ".penzd", ".pnez", ".penz", ".nez", ".xyz", ".asc"].iter().any(|e| lower.ends_with(e))
}

fn ms(t0: web_time::Instant) -> f64 {
    t0.elapsed().as_secs_f64() * 1000.0
}

fn stem(name: &str) -> String {
    let n = name.strip_suffix(".civilcraft.json").unwrap_or(name);
    n.rsplit_once('.').map_or(n, |(a, _)| a).chars().take(120).collect()
}

impl CivilView {
    pub fn new(project: Project, name: &str) -> CivilView {
        CivilView {
            project,
            name: name.to_string(),
            view: View2D::default(),
            pending_fit: true,
            plan: None,
            fixed: None,
            sel: None,
            hover_pt: None,
            show3d: false,
            site3: None,
            exag: 1.0,
            s3_seq: 0,
            cursor: None,
            message: String::new(),
            report: Vec::new(),
            facts: Facts { version: u64::MAX, ..Facts::default() },
            last_labels: 0,
            timings: HashMap::new(),
            export: crate::civilexport::ExportOpts::default(),
            dark: true,
        }
    }

    /// Open a file's text the way civilcraft.js `openText` does.
    pub fn open(name: &str, bytes: &[u8]) -> Result<CivilView, String> {
        if bytes.len() > MAX_TEXT_BYTES {
            return Err(t("cv.file.toobig", &[]));
        }
        let text = String::from_utf8_lossy(bytes);
        let lower = name.to_ascii_lowercase();
        let head: String = text.chars().take(400).collect::<String>().trim_start().to_string();
        if lower.ends_with(".json") || head.starts_with('{') {
            return Self::open_project(&text, name);
        }
        if lower.ends_with(".xml") || lower.ends_with(".landxml") || head.starts_with('<') {
            return Self::open_landxml(&text, name);
        }
        Ok(Self::import_points(&text, name))
    }

    /// civilcraft.js `openProject`: `Project.fromJSON` (untrusted: fields validated, bad items dropped with notes).
    pub fn open_project(text: &str, name: &str) -> Result<CivilView, String> {
        let t0 = web_time::Instant::now();
        let v: Value = serde_json::from_str(text).map_err(|e| tr!("cv.open.failed", file = name, why = e.to_string()))?;
        let (p, notes) = Project::from_json(&v).map_err(|e| tr!("cv.open.failed", file = name, why = e))?;
        let mut cv = CivilView::new(p, name);
        cv.project.saved = i64::try_from(cv.project.version).unwrap_or(0);
        cv.report = notes;
        cv.timings.insert("open", ms(t0));
        cv.message = tr!("cv.opened", file = name);
        Ok(cv)
    }

    /// civilcraft.js `openLandXml`: a new project from the first surface (as a TIN), every alignment (its first
    /// profile) and the first pipe network; the rest named in the notes.
    pub fn open_landxml(text: &str, name: &str) -> Result<CivilView, String> {
        let t0 = web_time::Instant::now();
        let d = cc_io_civil::landxml::read_land_xml(text).map_err(|e| tr!("cv.open.failed", file = name, why = e))?;
        let n_pts = arr(d.get("points")).len();
        let surfaces = arr(d.get("surfaces"));
        let als = arr(d.get("alignments"));
        let nets = arr(d.get("networks"));
        let mut notes: Vec<String> = arr(d.get("notes")).iter().filter_map(|n| n.as_str().map(str::to_string)).collect();
        if n_pts == 0 && surfaces.is_empty() && als.is_empty() && nets.is_empty() {
            let mut m = tr!("cv.lx.empty", file = name);
            if !notes.is_empty() {
                m.push(' ');
                m.push_str(&notes.join(" "));
            }
            return Err(m);
        }
        let units = if d.get("units").and_then(Value::as_str) == Some("m") { "m" } else { "ft" };
        let pname =
            d.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()).map_or_else(|| stem(name), |s| s.chars().take(120).collect());
        let points: Vec<Value> = arr(d.get("points"))
            .iter()
            .map(|p| {
                let q = cc_civil::points::read_pt(p);
                json!([q.n, q.x, q.y, q.z, q.d])
            })
            .collect();
        let mut surface = json!({ "groups": {}, "breaklines": [], "boundary": null, "maxEdge": 0, "tin": null });
        if let (Some(s0), Some(sm)) = (surfaces.first(), surface.as_object_mut()) {
            let sname: String = s0.get("name").and_then(Value::as_str).unwrap_or("").chars().take(120).collect();
            sm.insert("name".into(), Value::String(sname.clone()));
            let bls: Vec<Value> = arr(s0.get("breaklines"))
                .iter()
                .map(|b| json!({ "name": b.get("name").and_then(Value::as_str).unwrap_or("").chars().take(120).collect::<String>(), "pts": b.get("pts").cloned().unwrap_or(json!([])) }))
                .collect();
            sm.insert("breaklines".into(), Value::Array(bls));
            sm.insert("boundary".into(), s0.get("boundary").cloned().unwrap_or(Value::Null));
            if !arr(s0.get("faces")).is_empty() {
                sm.insert(
                    "tin".into(),
                    json!({ "pts": s0.get("pts").cloned().unwrap_or(json!([])), "faces": s0.get("faces").cloned().unwrap_or(json!([])) }),
                );
            }
            if surfaces.len() > 1 {
                notes.push(tr!("cv.lx.surfaces.more", name = sname.as_str(), n = surfaces.len() - 1));
            }
        }
        let mut seq = 1usize;
        let mut next = |pre: &str| {
            let s = format!("{pre}{seq}");
            seq += 1;
            s
        };
        let mut alignments = Vec::new();
        for a in als {
            let profs = arr(a.get("profiles"));
            let aname: String = a.get("name").and_then(Value::as_str).unwrap_or("").chars().take(120).collect();
            alignments.push(json!({ "id": next("a"), "name": aname, "start": a.get("start").cloned().unwrap_or(json!(0)), "pis": a.get("pis").cloned().unwrap_or(json!([])),
                "pvis": profs.first().and_then(|p| p.get("pvis")).cloned().unwrap_or(json!([])) }));
            if profs.len() > 1 {
                notes.push(tr!("cv.lx.profiles.more", name = aname.as_str()));
            }
        }
        let mut network = Value::Null;
        if let Some(n0) = nets.first() {
            let mut map: Vec<(String, String)> = Vec::new();
            let structures: Vec<Value> = arr(n0.get("structures"))
                .iter()
                .map(|x| {
                    let id = next("s");
                    map.push((cc_civil::js::string_of(x.get("id")), id.clone()));
                    let dia = numf(x.get("dia"));
                    let mut o = x.clone();
                    if let Some(m) = o.as_object_mut() {
                        m.insert("id".into(), Value::String(id));
                        m.insert(
                            "dia".into(),
                            json!(if dia > 0.0 {
                                dia
                            } else if units == "m" {
                                1200.0
                            } else {
                                48.0
                            }),
                        );
                    }
                    o
                })
                .collect();
            let find =
                |old: &Value| map.iter().find(|(o, _)| *o == cc_civil::js::string_of(Some(old))).map(|(_, n)| Value::String(n.clone()));
            let pipes: Vec<Value> = arr(n0.get("pipes"))
                .iter()
                .map(|q| {
                    let dia = numf(q.get("dia"));
                    let mut o = q.clone();
                    if let Some(m) = o.as_object_mut() {
                        m.insert("id".into(), Value::String(next("q")));
                        m.insert("from".into(), q.get("from").and_then(find).unwrap_or(Value::Null));
                        m.insert("to".into(), q.get("to").and_then(find).unwrap_or(Value::Null));
                        m.insert(
                            "dia".into(),
                            json!(if dia > 0.0 {
                                dia
                            } else if units == "m" {
                                300.0
                            } else {
                                12.0
                            }),
                        );
                    }
                    o
                })
                .collect();
            let mut net = cc_civil::js::J::j(&cc_civil::pipes::default_network(units));
            if let Some(m) = net.as_object_mut() {
                m.insert("name".into(), Value::String(n0.get("name").and_then(Value::as_str).unwrap_or("").chars().take(60).collect()));
                m.insert("type".into(), n0.get("type").cloned().unwrap_or(json!("storm")));
                m.insert("structures".into(), Value::Array(structures));
                m.insert("pipes".into(), Value::Array(pipes));
            }
            network = net;
            notes.push(t("cv.lx.net.rules", &[]));
            if nets.len() > 1 {
                notes.push(tr!("cv.lx.nets.more", n = nets.len() - 1));
            }
        }
        let file = json!({ "format": "civilcraft-project", "version": 1, "name": pname, "units": units, "pointsFile": if n_pts > 0 { name } else { "" },
            "points": points, "surface": surface, "alignments": alignments, "pads": [], "network": network, "seq": seq, "features": [], "parcels": [] });
        let (p, more) = Project::from_json(&file).map_err(|e| tr!("cv.open.failed", file = name, why = e))?;
        notes.extend(more);
        let mut cv = CivilView::new(p, name);
        cv.project.saved = i64::try_from(cv.project.version).unwrap_or(0);
        cv.report = notes;
        cv.timings.insert("open", ms(t0));
        cv.message = tr!(
            "cv.lx.opened",
            file = name,
            points = tr!("cv.pts.n", n = n_pts, count = cc_civil::units::grouped(n_pts as f64, 0)),
            surfaces = tr!("cv.surfaces.n", n = surfaces.len()),
            alignments = tr!("cv.als.n", n = als.len())
        );
        Ok(cv)
    }

    /// civilcraft.js `importPoints` with the format detected (the dialog's "Detect"), into a new project.
    pub fn import_points(text: &str, name: &str) -> CivilView {
        let t0 = web_time::Instant::now();
        let res = cc_civil::points::parse_points(text, "auto", "PNEZD");
        let parse_ms = ms(t0);
        let mut p = Project::new();
        if let Some(m) = p.state.as_object_mut() {
            m.insert("name".into(), Value::String(stem(name)));
        }
        let n = res.points.len();
        p.set_points(res.points, name);
        let mut cv = CivilView::new(p, name);
        cv.timings.insert("parse", parse_ms);
        let t1 = web_time::Instant::now();
        let tris = cv.project.eg().surface.as_ref().map(|s| s.n_t);
        cv.timings.insert("tin", ms(t1));
        let count = cc_civil::units::grouped(n as f64, 0);
        cv.message = match tris {
            Some(tr) => {
                tr!("cv.imp.imported.sf", n = n, count = count.as_str(), file = name, tris = cc_civil::units::grouped(tr as f64, 0))
            }
            None => tr!("cv.imp.imported", n = n, count = count.as_str(), file = name),
        };
        if res.bad_count > 0 {
            cv.report.push(tr!("cv.imp.bad", n = res.bad_count, count = cc_civil::units::grouped(res.bad_count as f64, 0)));
            for b in res.bad.iter().take(25) {
                let text: String = b.text.chars().take(60).collect();
                cv.report.push(tr!("cv.imp.bad.line", line = b.line, why = b.why.as_str(), text = text));
            }
        }
        cv
    }

    /// The sample site (civilcraft.js "Open the sample site").
    pub fn sample() -> CivilView {
        CivilView::import_points(SAMPLE_CSV, "sample-site.csv")
    }

    /// The sample site with design items made in code, so every plan layer and panel has something to show.
    pub fn demo() -> Result<CivilView, String> {
        let s = CivilView::sample();
        let mut file = s.project.to_json();
        let (e0, n0) = (6_149_985.0, 2_079_988.0);
        let at = |dx: f64, dy: f64| json!({ "x": e0 + dx, "y": n0 + dy });
        if let Some(m) = file.as_object_mut() {
            m.insert("name".into(), json!("Demo site"));
            m.insert(
                "alignments".into(),
                json!([{ "id": "a1", "name": "Main Street", "start": 0,
                "pis": [{ "x": e0 + 10.0, "y": n0 + 60.0, "r": 0 }, { "x": e0 + 200.0, "y": n0 + 140.0, "r": 150 }, { "x": e0 + 420.0, "y": n0 + 120.0, "r": 0 }],
                "pvis": [{ "sta": 0, "z": 251.0, "L": 0 }, { "sta": 220, "z": 255.5, "L": 120 }, { "sta": 430, "z": 252.0, "L": 0 }],
                "corridor": { "on": true },
                "sampling": { "step": 50, "left": 60, "right": 60, "extra": [] } }]),
            );
            m.insert(
                "pads".into(),
                json!([{ "id": "p2", "name": "Building pad", "z": 256.0, "cut": 2, "fill": 3,
                "pts": [at(250.0, 200.0), at(330.0, 200.0), at(330.0, 260.0), at(250.0, 260.0)] }]),
            );
            m.insert(
                "parcels".into(),
                json!([{ "id": "r3", "name": "Lot 1", "pts": [at(40.0, 170.0), at(200.0, 170.0), at(200.0, 300.0), at(40.0, 300.0)] }]),
            );
            m.insert(
                "features".into(),
                json!([{ "id": "f4", "name": "Swale", "closed": false, "use": "off", "daylight": false, "cut": 2, "fill": 2,
                "pts": [{ "x": e0 + 60.0, "y": n0 + 20.0, "z": 249.0 }, { "x": e0 + 220.0, "y": n0 + 30.0, "z": 247.5 }, { "x": e0 + 400.0, "y": n0 + 40.0, "z": 246.0 }] }]),
            );
            let mut net = cc_civil::js::J::j(&cc_civil::pipes::default_network("ft"));
            if let Some(nm) = net.as_object_mut() {
                nm.insert("name".into(), json!("Storm"));
                nm.insert(
                    "structures".into(),
                    json!([
                        { "id": "s5", "name": "S1", "x": e0 + 120.0, "y": n0 + 85.0, "rim": null, "sumpDepth": 0, "dia": 48 },
                        { "id": "s6", "name": "S2", "x": e0 + 260.0, "y": n0 + 110.0, "rim": null, "sumpDepth": 0, "dia": 48 },
                        { "id": "s7", "name": "S3", "x": e0 + 390.0, "y": n0 + 95.0, "rim": null, "sumpDepth": 0, "dia": 48 }
                    ]),
                );
                nm.insert(
                    "pipes".into(),
                    json!([
                        { "id": "q8", "name": "P1", "from": "s5", "to": "s6", "dia": 15, "invUp": 245.0, "invDn": 244.0 },
                        { "id": "q9", "name": "P2", "from": "s6", "to": "s7", "dia": 18, "invUp": 243.8, "invDn": 242.9 }
                    ]),
                );
            }
            m.insert("network".into(), net);
            m.insert("seq".into(), json!(10));
        }
        let (p, notes) = Project::from_json(&file)?;
        let mut cv = CivilView::new(p, "demo-site.civilcraft.json");
        cv.report = notes;
        cv.message = tr!("cv.opened", file = "demo-site.civilcraft.json");
        Ok(cv)
    }

    /// `n` survey points on a smooth hill at survey magnitudes (x ≈ 500 000, y ≈ 4 000 000), deterministic.
    pub fn synthetic(n: usize) -> CivilView {
        let n = n.clamp(100, 2_000_000);
        let side = (n as f64).sqrt().ceil().max(2.0);
        let span = 3000.0;
        let step = span / side;
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut text = String::with_capacity(n * 48);
        text.push_str("Point,Northing,Easting,Elevation,Description\n");
        for i in 0..n {
            let (gx, gy) = ((i as f64 % side), (i as f64 / side).floor());
            let x = 500_000.0 + gx * step + (rnd() - 0.5) * step * 0.6;
            let y = 4_000_000.0 + gy * step + (rnd() - 0.5) * step * 0.6;
            let (u, v) = ((x - 501_500.0) / 900.0, (y - 4_001_500.0) / 700.0);
            let z = 120.0 + 25.0 * (-(u * u + v * v)).exp() + 4.0 * (x / 230.0).sin() * (y / 310.0).cos();
            text.push_str(&format!("{},{y:.3},{x:.3},{z:.3},GRD\n", i + 1));
        }
        CivilView::import_points(&text, "synthetic.csv")
    }

    pub fn fit(&mut self) {
        self.pending_fit = true;
        if let Some(s) = self.site3.as_mut() {
            s.view.fit_pending = true;
        }
    }

    fn fit_now(&mut self) {
        match civilplan::extents(&mut self.project) {
            Some((x0, y0, x1, y1)) => {
                // view.js fit: 88 % of the view
                self.view.fit(x0, y0, x1, y1, 0.06);
            }
            None => {
                self.view.cx = 0.0;
                self.view.cy = 0.0;
                self.view.zoom = 1.0;
            }
        }
        self.pending_fit = false;
    }

    /// The plan for the current zoom (rebuilt when the project, the bucket or the theme changed).
    fn plan_for(&mut self, ppp: f64, pal: &Palette) -> &Plan {
        let mm = civilplan::mm_bucket(self.view.zoom / ppp);
        let key = PlanKey {
            version: self.project.version,
            points_version: self.project.points_version,
            mm_bits: mm.to_bits(),
            dark: pal.dark,
            palette_ink: pal.ink.to_array(),
        };
        if self.plan.as_ref().is_none_or(|p| p.key != key) {
            let plan = civilplan::build(&mut self.project, mm, pal, key, &mut self.fixed);
            self.timings.insert("plan", plan.build_ms);
            self.plan = Some(plan);
        }
        match self.plan.as_ref() {
            Some(p) => p,
            None => self.empty_plan(),
        }
    }
    #[allow(clippy::unused_self)]
    fn empty_plan(&self) -> &'static Plan {
        static EMPTY: std::sync::OnceLock<Plan> = std::sync::OnceLock::new();
        EMPTY.get_or_init(|| {
            let mut p = Project::new();
            let pal = Palette::of(&Tokens::preset(Preset::Dark), true);
            civilplan::build(
                &mut p,
                1.0,
                &pal,
                PlanKey { version: 0, points_version: 0, mm_bits: 0, dark: true, palette_ink: [0; 4] },
                &mut None,
            )
        })
    }

    /// The plan's segment count and last build time (for the benchmark).
    pub fn plan_stats(&self) -> (u64, f64) {
        self.plan.as_ref().map_or((0, 0.0), |p| (p.segments, p.build_ms))
    }

    /// Every recorded timing in one line (ms).
    pub fn timings_text(&self) -> String {
        let mut keys: Vec<(&&str, &f64)> = self.timings.iter().collect();
        keys.sort_by(|a, b| a.0.cmp(b.0));
        let mut out: Vec<String> = keys.iter().map(|(k, v)| format!("{k} {v:.0}")).collect();
        if let Some(p) = &self.plan {
            let st: Vec<String> = p.stages.iter().map(|(k, v)| format!("{k} {v:.0}")).collect();
            out.push(format!("plan stages [{}] · {} segments · {} labels", st.join(", "), p.segments, p.labels.len()));
        }
        out.push(format!("panel facts {:.0}", self.facts.ms));
        format!("timings (ms): {}", out.join(" · "))
    }

    /// Recompute the panels' facts if the project changed.
    pub fn refresh_facts(&mut self) {
        if self.facts.version == self.project.version {
            return;
        }
        let t0 = web_time::Instant::now();
        let p = &mut self.project;
        let g = p.graded();
        let mut probs = Vec::new();
        for (name, list) in &g.report_items {
            for x in list {
                let n = if name.is_empty() { t("cv.pad", &[]) } else { name.clone() };
                probs.push(tr!("cv.ew.problem", name = n, problem = x.as_str()));
            }
        }
        self.facts = Facts {
            version: p.version,
            volumes: p.volumes(f64::NAN),
            network: p.network_check(),
            breaklines: p.eg_breaklines().len(),
            groups: p.groups(),
            graded_problems: probs,
            ms: 0.0,
        };
        self.facts.ms = ms(t0);
    }

    // ------------------------------------------------------------------------------------------- picking

    /// civilcraft.js `pickAt`: structures (10 px), then the nearest pipe, alignment, feature line or pad within 7 px,
    /// then a survey point within 6 px, then the parcel under the pointer. `zoom` = screen pixels per unit.
    pub fn pick_at(&mut self, x: f64, y: f64, zoom: f64) -> Option<Sel> {
        let zoom = if zoom > 0.0 && zoom.is_finite() { zoom } else { 1.0 };
        let tol = 7.0 / zoom;
        let st = &self.project.state;
        let net = st.get("network").filter(|n| n.is_object());
        let structs: Vec<(String, f64, f64)> = net
            .map(|n| {
                arr(n.get("structures"))
                    .iter()
                    .map(|s| (cc_civil::js::string_of(s.get("id")), numf(s.get("x")), numf(s.get("y"))))
                    .collect()
            })
            .unwrap_or_default();
        let mut best: Option<(f64, Sel)> = None;
        for (id, sx, sy) in &structs {
            let d = (sx - x).hypot(sy - y) * zoom;
            if d < 10.0 && best.as_ref().is_none_or(|b| d < b.0) {
                best = Some((d, Sel::Structure(id.clone())));
            }
        }
        if let Some((_, s)) = best {
            return Some(s);
        }
        let mut bd = tol;
        let mut hit: Option<Sel> = None;
        if let Some(n) = net {
            for pp in arr(n.get("pipes")) {
                let f = structs.iter().find(|s| Some(s.0.as_str()) == pp.get("from").and_then(Value::as_str));
                let t2 = structs.iter().find(|s| Some(s.0.as_str()) == pp.get("to").and_then(Value::as_str));
                if let (Some(a), Some(b)) = (f, t2) {
                    let d = poly_dist(&[(a.1, a.2), (b.1, b.2)], x, y);
                    if d < bd {
                        bd = d;
                        hit = Some(Sel::Pipe(cc_civil::js::string_of(pp.get("id"))));
                    }
                }
            }
        }
        let als: Vec<Value> = arr(st.get("alignments")).to_vec();
        for a in &als {
            let al = self.project.alignment(a);
            let line: Vec<(f64, f64)> = if al.ok() {
                al.polyline(f64::INFINITY, std::f64::consts::PI / 90.0).iter().map(|p| (p.0, p.1)).collect()
            } else {
                arr(a.get("pis")).iter().map(|p| (numf(p.get("x")), numf(p.get("y")))).collect()
            };
            let d = poly_dist(&line, x, y);
            if d < bd {
                bd = d;
                hit = Some(Sel::Alignment(cc_civil::js::string_of(a.get("id"))));
            }
        }
        let st = &self.project.state;
        for f in arr(st.get("features")) {
            let mut pts: Vec<(f64, f64)> = arr(f.get("pts")).iter().map(|p| (numf(p.get("x")), numf(p.get("y")))).collect();
            if f.get("closed") == Some(&Value::Bool(true))
                && let Some(p0) = pts.first().copied()
            {
                pts.push(p0);
            }
            let d = poly_dist(&pts, x, y);
            if d < bd {
                bd = 0.0;
                hit = Some(Sel::Feature(cc_civil::js::string_of(f.get("id"))));
            }
        }
        for pd in arr(st.get("pads")) {
            let pts: Vec<(f64, f64)> = arr(pd.get("pts")).iter().map(|p| (numf(p.get("x")), numf(p.get("y")))).collect();
            let inside = point_in(&pts, x, y);
            let mut ring = pts.clone();
            if let Some(p0) = pts.first().copied() {
                ring.push(p0);
            }
            let d = if inside { 0.0 } else { poly_dist(&ring, x, y) };
            let is_feature = matches!(hit, Some(Sel::Feature(_)));
            if (d < bd || (d == 0.0 && hit.is_none())) && !is_feature {
                bd = d;
                hit = Some(Sel::Pad(cc_civil::js::string_of(pd.get("id"))));
            }
        }
        if hit.is_some() {
            return hit;
        }
        if let Some(i) = self.nearest_point(x, y, 6.0 / zoom) {
            return self.project.points.get(i).map(|p| Sel::Point(p.n.clone()));
        }
        for pc in arr(self.project.state.get("parcels")) {
            let pts: Vec<(f64, f64)> = arr(pc.get("pts")).iter().map(|p| (numf(p.get("x")), numf(p.get("y")))).collect();
            if point_in(&pts, x, y) {
                return Some(Sel::Parcel(cc_civil::js::string_of(pc.get("id"))));
            }
        }
        None
    }

    /// The survey point nearest (x, y) within `r` world units.
    pub fn nearest_point(&self, x: f64, y: f64, r: f64) -> Option<usize> {
        let mut best = None;
        let mut bd = r;
        for (i, p) in self.project.points.iter().enumerate() {
            if (p.x - x).abs() > bd || (p.y - y).abs() > bd {
                continue;
            }
            let d = (p.x - x).hypot(p.y - y);
            if d < bd {
                bd = d;
                best = Some(i);
            }
        }
        best
    }

    /// civilcraft.js `updateStatus`: E/N, ground and graded elevation, station/offset on the nearest alignment.
    pub fn status_at(&mut self, x: f64, y: f64) -> String {
        let mut parts = vec![tr!("cv.status.en", e = fixed(x, 2), n = fixed(y, 2))];
        let z = self.project.eg().surface.as_ref().and_then(|s| s.z_at(x, y));
        let zf = self.project.graded().surface.as_ref().and_then(|s| s.z_at(x, y));
        let mut e = match z {
            Some(z) => tr!("cv.read.ground", z = fixed(z, 2)),
            None => t("cv.status.off", &[]),
        };
        if let Some(f) = zf
            && z.is_none_or(|z| (f - z).abs() > 1e-6)
        {
            e.push_str(" · ");
            e.push_str(&tr!("cv.status.graded", z = fixed(f, 2)));
        }
        parts.push(e);
        let units = self.project.units().to_string();
        let als: Vec<Value> = arr(self.project.state.get("alignments")).to_vec();
        let sel_al = match &self.sel {
            Some(Sel::Alignment(id)) => als.iter().find(|a| a.get("id").and_then(Value::as_str) == Some(id.as_str())).cloned(),
            _ => None,
        };
        let mut pick: Option<(Value, cc_civil::alignment::StaOff)> = None;
        let cands = match sel_al {
            Some(a) => vec![a],
            None => als,
        };
        for a in cands {
            let al = self.project.alignment(&a);
            if !al.ok() {
                continue;
            }
            if let Some(so) = al.station_offset(x, y)
                && pick.as_ref().is_none_or(|(_, b)| so.dist < b.dist)
            {
                pick = Some((a, so));
            }
        }
        if let Some((a, so)) = pick {
            let side = t(if so.off >= 0.0 { "cv.side.r" } else { "cv.side.l" }, &[]);
            parts.push(tr!(
                if so.outside { "cv.status.staoff.out" } else { "cv.status.staoff" },
                name = cc_civil::js::string_or_empty(a.get("name")),
                sta = cc_civil::units::format_station(so.sta, &units, None),
                off = fixed(so.off.abs(), 2),
                side = side
            ));
        }
        parts.join("   ")
    }

    // ------------------------------------------------------------------------------------------- drawing

    /// The plan or the 3D scene in `rect`, with its input.
    pub fn show(&mut self, ui: &mut egui::Ui, rect: Rect, t: &Tokens, dark: bool) {
        self.dark = dark;
        if self.show3d {
            self.show_3d(ui, rect, t, dark);
        } else {
            self.show_plan(ui, rect, t, dark);
        }
    }

    fn show_plan(&mut self, ui: &mut egui::Ui, rect: Rect, t: &Tokens, dark: bool) {
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let ppp = f64::from(ui.ctx().pixels_per_point());
        self.view.width = (f64::from(rect.width()) * ppp).max(1.0);
        self.view.height = (f64::from(rect.height()) * ppp).max(1.0);
        if self.pending_fit {
            self.fit_now();
        }
        // input: drag pans (any button), wheel / pinch zooms about the pointer, click picks, Esc clears
        if resp.dragged() {
            let d = resp.drag_delta();
            self.view.pan(f64::from(d.x) * ppp, f64::from(d.y) * ppp);
        }
        let to_px = |p: Pos2| (f64::from(p.x - rect.min.x) * ppp, f64::from(p.y - rect.min.y) * ppp);
        if let Some(p) = resp.hover_pos() {
            let (sx, sy) = to_px(p);
            let (scroll, zoom) = ui.ctx().input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            if scroll != 0.0 {
                self.view.zoom_at(sx, sy, (f64::from(scroll) / 200.0).exp());
            }
            if zoom != 1.0 {
                self.view.zoom_at(sx, sy, f64::from(zoom));
            }
            let (wx, wy) = self.view.to_world(sx, sy);
            self.cursor = Some((wx, wy));
            self.hover_pt = self.nearest_point(wx, wy, 6.0 * ppp / self.view.zoom);
        } else {
            self.hover_pt = None;
        }
        if resp.clicked_by(egui::PointerButton::Primary)
            && let Some(p) = resp.interact_pointer_pos()
        {
            let (sx, sy) = to_px(p);
            let (wx, wy) = self.view.to_world(sx, sy);
            let zoom = self.view.zoom / ppp;
            self.sel = self.pick_at(wx, wy, zoom);
        }
        self.keys(ui);
        let pal = Palette::of(t, dark);
        let view = self.view;
        let plan = self.plan_for(ppp, &pal);
        let n = civilplan::paint(ui, rect, plan, &view, &pal);
        self.last_labels = n;
        self.overlay(ui, rect, &pal, ppp);
    }

    fn keys(&mut self, ui: &egui::Ui) {
        if ui.ctx().memory(|m| m.focused().is_some()) {
            return;
        }
        let (esc, undo, redo) = ui.ctx().input(|i| {
            (
                i.key_pressed(egui::Key::Escape),
                i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::Z),
                i.modifiers.command && (i.key_pressed(egui::Key::Y) || (i.modifiers.shift && i.key_pressed(egui::Key::Z))),
            )
        });
        if esc {
            self.sel = None;
        }
        if undo {
            self.undo();
        }
        if redo {
            self.redo();
        }
    }

    pub fn undo(&mut self) {
        match self.project.undo() {
            Some(l) => self.message = tr!("cv.undo.done", what = l),
            None => self.message = t("cv.undo.none", &[]),
        }
    }
    pub fn redo(&mut self) {
        match self.project.redo() {
            Some(l) => self.message = tr!("cv.redo.done", what = l),
            None => self.message = t("cv.redo.none", &[]),
        }
    }

    /// The selection on top of the plan (civilcraft.js `drawOverlay`): crisp vectors in screen space.
    fn overlay(&mut self, ui: &egui::Ui, rect: Rect, pal: &Palette, ppp: f64) {
        let painter = ui.painter_at(rect);
        let view = self.view;
        let s = |x: f64, y: f64| {
            let (sx, sy) = view.to_screen(x, y);
            Pos2::new(rect.min.x + (sx / ppp) as f32, rect.min.y + (sy / ppp) as f32)
        };
        let act = pal.action;
        let halo = act.gamma_multiply(0.45);
        let grip = |p: Pos2| {
            let r = Rect::from_center_size(p, egui::vec2(9.0, 9.0));
            painter.rect_filled(r, 0.0, pal.bg);
            painter.rect_stroke(r, 0.0, egui::Stroke::new(1.5, act), egui::StrokeKind::Middle);
        };
        let xy = |v: &Value| (numf(v.get("x")), numf(v.get("y")));
        let find = |list: &str, id: &str| {
            arr(self.project.state.get(list)).iter().find(|o| o.get("id").and_then(Value::as_str) == Some(id)).cloned()
        };
        match self.sel.clone() {
            Some(Sel::Alignment(id)) => {
                if let Some(a) = find("alignments", &id) {
                    let al = self.project.alignment(&a);
                    let line: Vec<Pos2> = if al.ok() {
                        al.polyline(f64::INFINITY, std::f64::consts::PI / 180.0).iter().map(|p| s(p.0, p.1)).collect()
                    } else {
                        arr(a.get("pis")).iter().map(|p| s(numf(p.get("x")), numf(p.get("y")))).collect()
                    };
                    painter.add(egui::Shape::line(line, egui::Stroke::new(5.0, halo)));
                    let pis: Vec<Pos2> = arr(a.get("pis")).iter().map(|p| s(numf(p.get("x")), numf(p.get("y")))).collect();
                    painter.add(egui::Shape::dashed_line(&pis, egui::Stroke::new(1.0, act), 5.0, 4.0));
                    for p in pis {
                        grip(p);
                    }
                }
            }
            Some(Sel::Pad(id)) => {
                if let Some(d) = find("pads", &id) {
                    let pts: Vec<Pos2> = arr(d.get("pts"))
                        .iter()
                        .map(|p| {
                            let (x, y) = xy(p);
                            s(x, y)
                        })
                        .collect();
                    painter.add(egui::Shape::closed_line(pts.clone(), egui::Stroke::new(3.0, halo)));
                    for p in pts {
                        grip(p);
                    }
                }
            }
            Some(Sel::Feature(id)) | Some(Sel::Parcel(id)) => {
                let list = if matches!(self.sel, Some(Sel::Feature(_))) { "features" } else { "parcels" };
                if let Some(f) = find(list, &id) {
                    let pts: Vec<Pos2> = arr(f.get("pts"))
                        .iter()
                        .map(|p| {
                            let (x, y) = xy(p);
                            s(x, y)
                        })
                        .collect();
                    let closed = list == "parcels" || f.get("closed") == Some(&Value::Bool(true));
                    let stroke = egui::Stroke::new(4.0, halo);
                    painter.add(if closed {
                        egui::Shape::closed_line(pts.clone(), stroke)
                    } else {
                        egui::Shape::line(pts.clone(), stroke)
                    });
                    for p in pts {
                        grip(p);
                    }
                }
            }
            Some(Sel::Structure(id)) => {
                let st = self.project.state.get("network").map(|n| arr(n.get("structures")).to_vec()).unwrap_or_default();
                if let Some(x) = st.iter().find(|x| x.get("id").and_then(Value::as_str) == Some(id.as_str())) {
                    let (a, b) = xy(x);
                    let q = s(a, b);
                    painter.circle_stroke(q, 11.0, egui::Stroke::new(2.5, act));
                    grip(q);
                }
            }
            Some(Sel::Pipe(id)) => {
                if let Some(n) = self.project.state.get("network") {
                    let st = arr(n.get("structures"));
                    if let Some(pp) = arr(n.get("pipes")).iter().find(|x| x.get("id").and_then(Value::as_str) == Some(id.as_str())) {
                        let f = |k: &str| st.iter().find(|x| x.get("id") == pp.get(k)).map(xy);
                        if let (Some(a), Some(b)) = (f("from"), f("to")) {
                            painter.line_segment([s(a.0, a.1), s(b.0, b.1)], egui::Stroke::new(6.0, halo));
                        }
                    }
                }
            }
            Some(Sel::Point(n)) => {
                if let Some(p) = self.project.points.iter().find(|p| p.n == n) {
                    painter.circle_stroke(s(p.x, p.y), 5.0, egui::Stroke::new(2.0, act));
                }
            }
            None => {}
        }
        if let Some(p) = self.hover_pt.and_then(|i| self.project.points.get(i)) {
            painter.circle_stroke(s(p.x, p.y), 7.0, egui::Stroke::new(1.5, act));
        }
    }

    fn show_3d(&mut self, ui: &mut egui::Ui, rect: Rect, t: &Tokens, dark: bool) {
        let stale = self.site3.as_ref().is_none_or(|s| s.version != self.project.version || (s.exag - self.exag).abs() > 1e-9);
        if stale {
            let keep = self.site3.as_ref().map(|s| (s.view.cam, s.version));
            self.s3_seq = self.s3_seq.wrapping_add(1);
            let mut s = civil3d::build(&mut self.project, dark, self.exag, (1 << 41) + self.s3_seq);
            self.timings.insert("site3d", s.build_ms);
            let c = t.canvas_bg;
            s.view.style.background = [f32::from(c.r()) / 255.0, f32::from(c.g()) / 255.0, f32::from(c.b()) / 255.0, 1.0];
            s.view.style.edges = false;
            if let Some((cam, _)) = keep {
                s.view.cam = cam;
                s.view.fit_pending = false;
            }
            self.site3 = Some(s);
        }
        let Some(s) = self.site3.as_mut() else { return };
        let resp = crate::view3d::show(ui, rect, &s.scene, &mut s.view);
        let halo = if dark { Color32::from_rgb(0x1b, 0x1c, 0x1e) } else { Color32::WHITE };
        civil3d::paint_labels(ui, rect, s, t.text, halo);
        if resp.clicked_by(egui::PointerButton::Primary)
            && let Some(p) = resp.interact_pointer_pos()
        {
            let ppp = f64::from(ui.ctx().pixels_per_point());
            let (sx, sy) = (f64::from(p.x - rect.min.x) * ppp, f64::from(p.y - rect.min.y) * ppp);
            match crate::view3d::pick(&s.scene, &s.view.cam, sx, sy) {
                Some(h) => {
                    let z = s.origin[2] + (h.point[2] - s.origin[2]) / s.exag.max(1e-9);
                    self.message = format!(
                        "{} · {}",
                        tr!("cv.status.en", e = fixed(h.point[0], 2), n = fixed(h.point[1], 2)),
                        tr!("cv.read.ground", z = fixed(z, 2))
                    );
                }
                None => self.message.clear(),
            }
        }
        if let Some(p) = resp.hover_pos() {
            let _ = p;
        }
    }

    /// The status line (civilcraft.js status: the pointer's reading, else what was last said).
    pub fn status(&mut self) -> String {
        if !self.show3d
            && let Some((x, y)) = self.cursor
        {
            let s = self.status_at(x, y);
            if self.message.is_empty() {
                return s;
            }
            return format!("{s}   ·   {}", self.message);
        }
        self.message.clone()
    }

    /// Set a display switch or value (civilcraft.js `setDisplay`: not an undo step).
    pub fn display(&mut self, spec: &str) -> Result<(), String> {
        const SWITCHES: [&str; 11] =
            ["contours", "triangles", "points", "pointLabels", "labels", "breaklines", "outline", "pipes", "slopes", "bands", "alignments"];
        match spec.split_once('=') {
            None => {
                if !SWITCHES.contains(&spec) {
                    return Err(format!("civil.display: unknown switch {spec}"));
                }
                let on = self
                    .project
                    .state
                    .get("display")
                    .and_then(|d| d.get(spec))
                    .map_or(spec == "alignments", |v| cc_civil::js::truthy(Some(v)));
                self.project.set_display(spec, Value::Bool(!on));
            }
            Some((k, v)) => match k {
                "interval" | "major" | "bandCount" => {
                    let x: f64 = v.trim().parse().map_err(|_| format!("civil.display: {k} must be a number"))?;
                    let ok = match k {
                        "interval" => x > 0.0 && x <= 10_000.0,
                        "major" => (1.0..=100.0).contains(&x),
                        _ => (2.0..=64.0).contains(&x),
                    };
                    if !ok || !x.is_finite() {
                        return Err(format!("civil.display: {k} = {v} is out of range"));
                    }
                    self.project.set_display(k, json!(x));
                }
                "surface" if v == "eg" || v == "fg" => self.project.set_display("surface", json!(v)),
                _ if SWITCHES.contains(&k) && (v == "true" || v == "false") => self.project.set_display(k, Value::Bool(v == "true")),
                _ => return Err(format!("civil.display: unknown setting {spec}")),
            },
        }
        Ok(())
    }

    /// Set a point group's use (civilcraft.js group select: one undo step).
    pub fn group_use(&mut self, group: &str, use_: &str) -> Result<(), String> {
        if !matches!(use_, "points" | "breakline" | "off") {
            return Err(format!("civil.group: use must be points, breakline or off (not {use_})"));
        }
        if !self.project.groups().iter().any(|g| g.0 == group) {
            return Err(format!("civil.group: no group {group}"));
        }
        let label = tr!("cv.undo.group", group = group, r#use = use_);
        let (g, u) = (group.to_string(), use_.to_string());
        self.project.change(&label, &mut |s: &mut Value| {
            if let Some(m) = s.get_mut("surface").and_then(|x| x.get_mut("groups")).and_then(Value::as_object_mut) {
                m.insert(g.clone(), Value::String(u.clone()));
            }
        });
        Ok(())
    }

    /// Centre the plan on a world point (and zoom: screen pixels per unit).
    pub fn goto(&mut self, x: f64, y: f64, zoom_css: Option<f64>, ppp: f64) {
        self.pending_fit = false;
        self.view.cx = x;
        self.view.cy = y;
        if let Some(z) = zoom_css.filter(|z| z.is_finite() && *z > 0.0) {
            self.view.zoom = (z * ppp).clamp(cc_render::ZOOM_MIN, cc_render::ZOOM_MAX);
        }
    }

    /// The project file (civilcraft.js Save: `JSON.stringify(project.toJSON())`, `saved` = the time given).
    pub fn file_bytes(&self, now_ms: f64) -> (String, Vec<u8>) {
        crate::civilexport::project_file(&self.project, now_ms)
    }
}

fn poly_dist(pts: &[(f64, f64)], x: f64, y: f64) -> f64 {
    let mut d = f64::INFINITY;
    for w in pts.windows(2) {
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let l = dx * dx + dy * dy;
        let u = if l > 0.0 { (((x - a.0) * dx + (y - a.1) * dy) / l).clamp(0.0, 1.0) } else { 0.0 };
        d = d.min((x - a.0 - dx * u).hypot(y - a.1 - dy * u));
    }
    d
}

fn point_in(pts: &[(f64, f64)], x: f64, y: f64) -> bool {
    let mut inside = false;
    let n = pts.len();
    for i in 0..n {
        let (Some(a), Some(b)) = (pts.get(i), pts.get((i + n - 1) % n)) else { continue };
        if (a.1 > y) != (b.1 > y) && x < (b.0 - a.0) * (y - a.1) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
    }
    inside
}

fn xy_param(s: &str) -> Option<Vec<f64>> {
    let v: Vec<f64> = s.split(',').map(|x| x.trim().parse::<f64>()).collect::<Result<_, _>>().ok()?;
    (v.len() >= 2 && v.iter().all(|x| x.is_finite())).then_some(v)
}

// ------------------------------------------------------------------------------------------------ app hooks

/// Show a CivilCraft project in the app (any other document closes).
pub fn install(app: &mut UiApp, cv: CivilView) {
    app.doc = None;
    app.editor = None;
    app.source = None;
    app.model3 = None;
    app.mc = None;
    app.scene3 = None;
    app.pdf = None;
    app.error = None;
    app.message = cv.message.clone();
    app.civil = Some(cv);
}

/// Open a CivilCraft file (project, LandXML, points) dropped, picked or given to the CLI.
pub fn open_file(app: &mut UiApp, f: &ImportedFile) {
    match CivilView::open(&f.name, &f.bytes) {
        Ok(cv) => install(app, cv),
        Err(e) => app.error = Some(e),
    }
}

/// The `civil.*` commands.
pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    let name = params.name.clone().unwrap_or_default();
    match id {
        "civil.sample" => {
            install(app, CivilView::sample());
            return Ok(());
        }
        "civil.demo" => {
            install(app, CivilView::demo()?);
            return Ok(());
        }
        "civil.synthetic" => {
            let n = params
                .factor
                .or_else(|| name.parse().ok())
                .filter(|f| f.is_finite() && *f >= 1.0)
                .ok_or("civil.synthetic: factor = number of points")?;
            let t0 = web_time::Instant::now();
            let mut cv = CivilView::synthetic(n.min(2e6) as usize);
            let t1 = web_time::Instant::now();
            let c = cv.project.contours();
            let contour_ms = ms(t1);
            cv.timings.insert("contours", contour_ms);
            cv.message = format!(
                "{} · parse {:.0} ms · TIN {:.0} ms · contours {:.0} ms ({} lines) · total {:.0} ms",
                cv.message,
                cv.timings.get("parse").copied().unwrap_or(0.0),
                cv.timings.get("tin").copied().unwrap_or(0.0),
                contour_ms,
                c.len(),
                ms(t0)
            );
            install(app, cv);
            return Ok(());
        }
        _ => {}
    }
    let dark = app.preset == Preset::Dark;
    let cv = app.civil.as_mut().ok_or_else(|| format!("{id}: open a CivilCraft project first (civil.sample)"))?;
    match id {
        "civil.view" => {
            cv.show3d = match name.as_str() {
                "plan" => false,
                "3d" => true,
                "" => !cv.show3d,
                o => return Err(format!("civil.view: plan or 3d, not {o}")),
            };
        }
        "civil.display" => cv.display(&name)?,
        "civil.select" => {
            cv.sel = if name.is_empty() {
                None
            } else {
                Some(Sel::parse(&name).ok_or_else(|| format!("civil.select: kind:id expected, not {name}"))?)
            };
        }
        "civil.pick" | "civil.hover" => {
            let v = xy_param(&name).ok_or_else(|| format!("{id}: name = \"x,y\" (world)"))?;
            let (x, y) = (v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0));
            if id == "civil.pick" {
                let zoom = cv.view.zoom.max(1e-9);
                cv.sel = cv.pick_at(x, y, zoom);
                cv.message = cv.sel.as_ref().map_or_else(String::new, Sel::key);
            } else {
                cv.cursor = Some((x, y));
                cv.hover_pt = cv.nearest_point(x, y, 6.0 / cv.view.zoom.max(1e-9));
            }
        }
        "civil.goto" => {
            let v = xy_param(&name).ok_or("civil.goto: name = \"x,y\" or \"x,y,zoom\"")?;
            cv.goto(v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0), v.get(2).copied(), 1.0);
        }
        "civil.group" => {
            let (g, u) = name.split_once('=').ok_or("civil.group: name = \"GROUP=use\"")?;
            cv.group_use(g.trim(), u.trim())?;
        }
        "civil.timings" => cv.message = cv.timings_text(),
        "civil.undo" => cv.undo(),
        "civil.redo" => cv.redo(),
        "civil.camera" => {
            let dir = match name.as_str() {
                "top" => [0.0, -1e-4, 1.0],
                "front" => [0.0, -1.0, 0.25],
                "back" => [0.0, 1.0, 0.25],
                "left" => [-1.0, 0.0, 0.25],
                "right" => [1.0, 0.0, 0.25],
                "iso" | "" => [0.6, -1.0, 0.75],
                o => return Err(format!("civil.camera: unknown view {o}")),
            };
            cv.show3d = true;
            if cv.site3.is_none() {
                cv.site3 = Some(civil3d::build(&mut cv.project, dark, cv.exag, (1 << 41) + 999));
            }
            if let Some(s) = cv.site3.as_mut() {
                s.view.cam.fit(s.scene.min, s.scene.max, Some(dir));
                s.view.fit_pending = false;
            }
        }
        "civil.exag" => {
            let f = params.factor.filter(|f| site_exag_ok(*f)).ok_or("civil.exag: factor = 1, 2, 3, 5 or 10")?;
            cv.exag = f;
        }
        "civil.save" => {
            let (fname, bytes) = cv.file_bytes(now_ms());
            let save = app.services.save.as_mut().ok_or("civil.save: saving is not available here")?;
            if let Some(to) = save(&fname, bytes)?
                && let Some(cv) = app.civil.as_mut()
            {
                cv.project.saved = i64::try_from(cv.project.version).unwrap_or(0);
                cv.message = tr!("cv.saved.dl", name = to);
            }
        }
        "civil.export" => {
            // civilcraft.js exportLandXml / exportDxf / exportDwg / plot, site-view3d.js exportGLB, saveProject
            match crate::civilexport::run(cv, &name, now_ms(), dark)? {
                crate::civilexport::Outcome::Nothing(m) => cv.message = m,
                crate::civilexport::Outcome::File(fname, bytes, msg) => {
                    let project = name.trim_start().starts_with("project");
                    let save = app.services.save.as_mut().ok_or("civil.export: saving is not available here")?;
                    if save(&fname, bytes)?.is_some()
                        && let Some(cv) = app.civil.as_mut()
                    {
                        if project {
                            cv.project.saved = i64::try_from(cv.project.version).unwrap_or(0);
                        }
                        cv.message = msg;
                    }
                }
            }
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

/// The wall clock in ms (what date-stamped exports use unless a test fixes it).
fn now_ms() -> f64 {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
}

fn site_exag_ok(f: f64) -> bool {
    cc_civil::site3d::EXAGGERATIONS.contains(&f)
}

/// The CivilCraft frame: tool bar, left and right panels, the plan or the 3D scene (lib.rs calls this when a project
/// is open).
pub fn ui(app: &mut UiApp, ui: &mut egui::Ui) {
    let t = app.tokens;
    let dark = app.preset == Preset::Dark;
    let mut cmd: Option<(&'static str, Params)> = None;
    if let Some(cv) = app.civil.as_mut() {
        egui::Panel::top("civil_bar")
            .exact_size(30.0)
            .frame(egui::Frame::new().fill(t.panel_darker).inner_margin(egui::Margin::symmetric(8, 3)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let site = cv.project.state.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                    ui.label(RichText::new(site).strong().color(t.text_strong));
                    ui.separator();
                    if ui.selectable_label(!cv.show3d, t0("plot.title")).on_hover_text(t0("ui.plan.canvas")).clicked() {
                        cmd = Some(("civil.view", Params { name: Some("plan".into()), ..Default::default() }));
                    }
                    if ui.selectable_label(cv.show3d, t0("ui.tb.view3d")).on_hover_text(t0("ui.tb.view3d.title")).clicked() {
                        cmd = Some(("civil.view", Params { name: Some("3d".into()), ..Default::default() }));
                    }
                    ui.separator();
                    if ui.button(t0("ui.tb.fit")).on_hover_text(t0("ui.tb.fit.title")).clicked() {
                        cv.fit();
                    }
                    if ui
                        .add_enabled(!cv.project.undo_stack.is_empty(), egui::Button::new(t0("ui.tb.undo")))
                        .on_hover_text(t0("ui.tb.undo.title"))
                        .clicked()
                    {
                        cmd = Some(("civil.undo", Params::default()));
                    }
                    if ui
                        .add_enabled(!cv.project.redo_stack.is_empty(), egui::Button::new(t0("ui.tb.redo")))
                        .on_hover_text(t0("ui.tb.redo.title"))
                        .clicked()
                    {
                        cmd = Some(("civil.redo", Params::default()));
                    }
                    if ui.button(t0("ui.tb.save")).on_hover_text(t0("ui.tb.save.title")).clicked() {
                        cmd = Some(("civil.save", Params::default()));
                    }
                    if cv.show3d {
                        ui.separator();
                        for v in ["iso", "top", "front", "right"] {
                            if ui.small_button(v).clicked() {
                                cmd = Some(("civil.camera", Params { name: Some(v.into()), ..Default::default() }));
                            }
                        }
                        let mut ex = cv.exag;
                        egui::ComboBox::new("civil_exag", "").selected_text(tr!("v3d.exag.opt", x = ex)).width(104.0).show_ui(ui, |ui| {
                            for e in cc_civil::site3d::EXAGGERATIONS {
                                ui.selectable_value(&mut ex, e, tr!("v3d.exag.opt", x = e));
                            }
                        });
                        if ex != cv.exag {
                            cmd = Some(("civil.exag", Params { factor: Some(ex), ..Default::default() }));
                        }
                    }
                });
            });
        let mut acts = Vec::new();
        cv.refresh_facts();
        let frame = egui::Frame::new().fill(t.panel).inner_margin(8);
        egui::Panel::left("civil_left").default_size(270.0).size_range(200.0..=420.0).resizable(true).frame(frame).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| crate::civilpanel::left(cv, ui, &t, &mut acts));
        });
        egui::Panel::right("civil_right").default_size(300.0).size_range(220.0..=460.0).resizable(true).frame(frame).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| crate::civilpanel::right(cv, ui, &t, &mut acts));
        });
        for a in acts {
            let r = match a {
                crate::civilpanel::Act::Select(s) => {
                    cv.sel = s;
                    Ok(())
                }
                crate::civilpanel::Act::Display(s) => cv.display(&s),
                crate::civilpanel::Act::Group(g, u) => cv.group_use(&g, &u),
                crate::civilpanel::Act::Export(k) => {
                    cmd = Some(("civil.export", Params { name: Some(k), ..Default::default() }));
                    Ok(())
                }
            };
            if let Err(e) = r {
                cv.message = e;
            }
        }
        egui::CentralPanel::no_frame().show(ui, |ui| {
            let rect = ui.max_rect();
            cv.show(ui, rect, &t, dark);
        });
        app.cursor = if cv.show3d { None } else { cv.cursor };
        app.message = cv.status();
    }
    if let Some((id, p)) = cmd
        && let Err(e) = run(app, id, &p)
    {
        app.error = Some(e);
    }
}

/// `t(key)` without fills.
pub fn t0(key: &str) -> String {
    t(key, &[])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_unique_and_apart_from_the_viewer_s() {
        let mut ids: Vec<&str> = COMMANDS.iter().map(|c| c.0).chain(crate::UI_COMMANDS.iter().map(|c| c.0)).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }

    #[test]
    fn files_are_told_apart() {
        assert!(is_civil_file("site.civilcraft.json", b"{\"format\":\"civilcraft-project\"}"));
        assert!(!is_civil_file("other.json", b"{\"a\":1}"));
        assert!(is_civil_file("s.xml", b"<?xml version=\"1.0\"?><LandXML>"));
        assert!(is_civil_file("pts.csv", b"1,2,3"));
        assert!(!is_civil_file("plan.dwg", b"AC1032"));
    }

    #[test]
    fn the_sample_opens_with_a_surface_and_survey_coordinates() {
        let mut cv = CivilView::sample();
        assert_eq!(cv.project.points.len(), 276);
        let tris = cv.project.eg().surface.as_ref().map_or(0, |s| s.n_t);
        assert!(tris > 400, "{tris} triangles");
        let (x0, y0, x1, y1) = civilplan::extents(&mut cv.project).unwrap();
        assert!(x0 > 6_149_900.0 && x1 < 6_150_500.0 && y0 > 2_079_900.0 && y1 < 2_080_400.0);
    }

    #[test]
    fn the_demo_keeps_every_item_and_grades() {
        let mut cv = CivilView::demo().unwrap();
        assert!(cv.report.is_empty(), "notes: {:?}", cv.report);
        for k in ["alignments", "pads", "parcels", "features"] {
            assert_eq!(arr(cv.project.state.get(k)).len(), 1, "{k}");
        }
        assert!(cv.project.graded().surface.is_some(), "the corridor and the pad grade");
        cv.refresh_facts();
        let v = cv.facts.volumes.as_ref().unwrap();
        assert!(numf(v.get("cut")) + numf(v.get("fill")) > 0.0, "{v}");
        assert_eq!(cv.facts.network.as_ref().map(|n| n.pipes.len()), Some(2));
    }

    #[test]
    fn picking_at_survey_coordinates_tells_points_a_millimetre_apart() {
        let text = "1,4000000.000,500000.000,10\n2,4000000.000,500000.001,10\n3,4000010,500010,11\n";
        let mut cv = CivilView::import_points(text, "near.csv");
        // 10 000 screen pixels per unit: the points are 10 px apart
        let hit = cv.pick_at(500_000.001_2, 4_000_000.0, 10_000.0);
        assert_eq!(hit, Some(Sel::Point("2".into())));
        let hit = cv.pick_at(499_999.999_8, 4_000_000.0, 10_000.0);
        assert_eq!(hit, Some(Sel::Point("1".into())));
    }

    #[test]
    fn the_demo_picks_its_items() {
        let mut cv = CivilView::demo().unwrap();
        let (e0, n0) = (6_149_985.0, 2_079_988.0);
        assert_eq!(cv.pick_at(e0 + 260.0, n0 + 110.0, 2.0), Some(Sel::Structure("s6".into())));
        assert_eq!(cv.pick_at(e0 + 290.0, n0 + 230.0, 2.0), Some(Sel::Pad("p2".into())));
        assert!(matches!(cv.pick_at(e0 + 100.0, n0 + 240.0, 2.0), Some(Sel::Parcel(_)) | Some(Sel::Point(_))));
        let s = cv.status_at(e0 + 200.0, n0 + 120.0);
        assert!(s.contains("Main Street"), "{s}");
    }

    #[test]
    fn display_and_group_commands_validate() {
        let mut cv = CivilView::sample();
        assert!(cv.display("nope").is_err());
        assert!(cv.display("interval=-1").is_err());
        cv.display("triangles").unwrap();
        assert_eq!(cv.project.state["display"]["triangles"], Value::Bool(true));
        cv.display("interval=2").unwrap();
        assert!(cv.group_use("RIDGE", "sideways").is_err());
        cv.group_use("RIDGE", "breakline").unwrap();
        assert_eq!(cv.project.undo_stack.len(), 1);
        cv.undo();
        assert_eq!(cv.project.state["surface"]["groups"]["RIDGE"], Value::String("points".into()));
    }

    #[test]
    fn the_project_file_opens_again() {
        let cv = CivilView::demo().unwrap();
        let (name, bytes) = cv.file_bytes(0.0);
        assert!(name.ends_with(".civilcraft.json"));
        let again = CivilView::open(&name, &bytes).unwrap();
        assert_eq!(again.project.points.len(), 276);
        assert_eq!(arr(again.project.state.get("alignments")).len(), 1);
    }

    #[test]
    fn landxml_written_by_the_engine_opens() {
        let text = "<?xml version=\"1.0\"?><LandXML xmlns=\"http://www.landxml.org/schema/LandXML-1.2\" version=\"1.2\"><Units><Imperial linearUnit=\"USSurveyFoot\" areaUnit=\"squareFoot\" volumeUnit=\"cubicYard\" angularUnit=\"decimal degrees\"/></Units><CgPoints><CgPoint name=\"1\" desc=\"GRD\">2080000 6150000 250</CgPoint><CgPoint name=\"2\" desc=\"GRD\">2080100 6150000 251</CgPoint><CgPoint name=\"3\" desc=\"GRD\">2080000 6150100 252</CgPoint></CgPoints></LandXML>";
        let mut cv = CivilView::open("t.xml", text.as_bytes()).unwrap();
        assert_eq!(cv.project.points.len(), 3);
        assert!(cv.project.eg().surface.is_some());
        assert!(CivilView::open("e.xml", b"<LandXML></LandXML>").is_err());
    }

    #[test]
    fn hostile_input_is_refused_not_a_crash() {
        assert!(CivilView::open("x.civilcraft.json", b"{not json").is_err());
        assert!(CivilView::open("x.civilcraft.json", b"{\"format\":\"other\"}").is_err());
        let cv = CivilView::open("junk.csv", &[0xff, 0xfe, 0x00, b',', b'\n']).unwrap();
        assert!(cv.project.points.is_empty());
    }
}
