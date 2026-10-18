//! PartCraft in the app (M13): a part opened from the files 0.5 PartCraft opens — its own `.partcraft.json`
//! (`P.toJSON`) and the `.pcscript` text form — or the built-in sample, rebuilt by cc-part's mesh kernel
//! (`kernel::regenerate`, the same code at parity with kernel.js) and shown in the 3D view, one GPU mesh per named
//! face so a hovered or picked face is tinted and named as partcraft.js `faceWords` names it. The feature history,
//! the parameters, undo, the rollback bar, Save and the exports are partcraft.js's (`commit`, `rollTo`, `savePart`,
//! `exportMesh`, `exportIfc`); the panels are in partpanel.rs.
//!
//! Units are millimetres, Y up (the part's own frame); the 3D view is Z up, so points go through `z_up` exactly as
//! the STL / 3MF exports turn them (x, −z, y).

use cc_part::{configs as cfg, export as X, i18n, jv, kernel, params, params_part as PP, part as P, pcscript};
use cc_render::mesh3d::{Instance3, MeshData};
use serde_json::Value;

use crate::view3d::{Scene3D, View3};
use crate::{ImportedFile, Params, UiApp};

/// The built-in sample: a bracket with parameters, a fillet, chamfers, bolt holes in a pattern and a slot (our own
/// part, written for the Rust test version in PartCraft's script form; `.pcscript` 1).
pub static SAMPLE_SCRIPT: &str = include_str!("../../../assets/samples/partcraft-bracket.pcscript");

/// Largest part file opened (partcraft.js `openFile`: 50 MB).
pub const MAX_FILE_BYTES: usize = 50 * 1024 * 1024;

/// More named faces than this and the solid is one mesh (no per-face tint; hover still names the face).
const MAX_FACE_MESHES: usize = 3000;

/// The `part.*` UI commands (id, label, shortcut, params doc).
pub const COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("part.sample", "Open the sample part", "", "{} → the built-in PartCraft sample (a bracket with parameters)"),
    ("part.save", "Save part", "", "{} → <name>.partcraft.json through the platform's save (partcraft.js savePart)"),
    (
        "part.export",
        "Export the part",
        "",
        "{name: \"stl\"|\"3mf\"|\"step\"|\"step214\"|\"ifc\" [\"@<ms>\" = the clock for STEP / IFC]} → partcraft.js exportMesh / exportIfc",
    ),
    (
        "part.param",
        "Set a parameter",
        "",
        "{name: \"width=150\" (a parameter, new or changed) | \"Body.depth=12\" (a number; solved backward when an equation drives it) | \"Body.depth==thick\" (link)} → one undo step, rebuilt",
    ),
    ("part.undo", "Undo", "", "{}"),
    ("part.redo", "Redo", "", "{}"),
    ("part.rollto", "Roll back to here", "", "{name: feature id or name | \"end\"} → the rollback bar above that feature (one undo step)"),
    ("part.suppress", "Suppress / unsuppress", "", "{name: feature id or name} → one undo step"),
    ("part.select", "Select a feature", "", "{name: feature id or name | \"\"} → the feature shown in the history panel"),
    ("part.face", "Pick a face", "", "{name: face name (\"f2.end\") | \"\"} → the face selected and described"),
];

/// A word of PartCraft's own catalog (`lang/en.json`; the key when missing).
pub fn tr(key: &str) -> String {
    i18n::t0(key)
}

/// `t(key, vars)` with string fills.
pub fn trv(key: &str, vars: &[(&str, String)]) -> String {
    let v: Vec<(&str, i18n::F)> = vars.iter().map(|(k, s)| (*k, i18n::F::S(s.clone()))).collect();
    i18n::t(key, &v)
}

fn js_f(i: usize) -> f64 {
    cc_part::js::f(i)
}

fn ms_since(t0: web_time::Instant) -> f64 {
    t0.elapsed().as_secs_f64() * 1000.0
}

/// `[x, −z, y]`: the part's Y-up frame to the view's (and the exports') Z up.
fn z_up(p: [f64; 3]) -> [f64; 3] {
    [p[0], -p[2], p[1]]
}

/// The view's Z up back to the part's frame.
fn y_up(p: [f64; 3]) -> [f64; 3] {
    [p[0], p[2], -p[1]]
}

/// One open part.
pub struct PartView {
    /// the document (part.js shape, insertion-ordered JSON)
    pub part: Value,
    /// the file it came from (shown in the status line)
    pub file: String,
    pub regen: kernel::Regen,
    /// how long the last rebuild took (ms)
    pub regen_ms: f64,
    undo: Vec<(String, Value)>,
    redo: Vec<(String, Value)>,
    pub dirty: bool,
    pub scene: Scene3D,
    pub view: View3,
    scene_seq: u64,
    /// the face each scene instance draws (None = several faces in one mesh, or the edges)
    inst_face: Vec<Option<usize>>,
    pub hover: Option<usize>,
    pub sel_face: Option<usize>,
    pub sel_feature: Option<String>,
    pub message: String,
    /// the right panel's tab: "props" or "params"
    pub tab: &'static str,
    /// text typed into number and expression fields, by target key / "var:<name>" (kept while the field has focus)
    pub edits: std::collections::HashMap<String, String>,
    /// the new parameter's name and value fields
    pub new_param: (String, String),
    /// tint colours: hover, selected (from the theme)
    pub tints: ([u8; 4], [u8; 4]),
    pub fit_pending: bool,
    /// what the hover was worked out for: the pointer (physical px in the view) and the camera
    pub hover_key: Option<(Option<(f64, f64)>, cc_render::mesh3d::Cam3)>,
    /// partcraft.js `mode`: model, sketch (a sketch is open) or pick (a fillet's edges / a shell's faces are picked)
    pub mode: Mode,
    /// the feature a sketch or a pick is open for
    pub editing: Option<String>,
    /// a base plane selected in the history (front / top / right)
    pub sel_plane: Option<String>,
    /// geometry picked in the 3D view (a face or an edge)
    pub pick: Option<Pick>,
    /// edges or faces added with Shift (for Fillet / Chamfer / Shell)
    pub multi: Vec<Pick>,
    /// Sketch pressed with nothing picked: the next plane or flat face starts it
    pub pending_sketch: bool,
    /// the open sketch's last solve
    pub last_solve: Option<cc_part::solver::Solved>,
    /// the sketch editor (sketchview.js)
    pub sk: crate::partsketch::SketchEd,
    /// the status line's message (partcraft.js `status`); `message` is the build chip
    pub note: String,
    /// the pointer in sketch coordinates
    pub coords: Option<[f64; 2]>,
    /// a question waiting for an answer (Delete with dependents): the feature and the words
    pub confirm: Option<(String, String, String)>,
    /// the sketch panel's numeric entry: kind and the two fields
    pub num_entry: (String, String, String),
    /// the feature panel's name field while it is being typed
    pub name_edit: Option<(String, String)>,
    /// time spent in `part.*` commands and in rebuilds since the part opened: (ms, count) each
    pub cmd_ms: (f64, usize),
    pub rebuild_total: (f64, usize),
    /// the rebuild being worked on a thread (desktop): its generation and where the result arrives
    #[cfg(not(target_arch = "wasm32"))]
    bg: Option<(u64, std::sync::mpsc::Receiver<(kernel::Regen, f64)>)>,
    /// rebuild generations: bumped by every rebuild asked for; a thread's result is used only if it is still the newest
    generation: u64,
    /// a rebuild was asked for while one was on the thread: send the newest part when it returns
    bg_again: bool,
    /// big rebuilds go to a thread (set from `BACKGROUND_REBUILDS` when the part is put in the window)
    pub background: bool,
}

/// Rebuilds of parts whose last rebuild took longer than this (ms) go to a thread on the desktop, so the window keeps
/// drawing (partcraft.js does the same with the exact kernel's worker: the newest request wins).
pub const BACKGROUND_MS: f64 = 60.0;

/// Set by the desktop app: big parts rebuild on a thread. Headless runs (CLI, harnesses, tests) rebuild in place.
pub static BACKGROUND_REBUILDS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// partcraft.js `mode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Model,
    Sketch,
    Pick,
}

/// A pick in the 3D view (partcraft.js `pick` / `multi` items).
#[derive(Clone, Debug, PartialEq)]
pub struct Pick {
    /// true = a face, false = an edge
    pub face: bool,
    /// the face's or edge's index in the rebuild's records
    pub index: usize,
    /// a flat face (a sketch can sit on it)
    pub flat: bool,
    /// the reference a feature stores (a PlaneRef for a flat face, an EdgeRef for an edge)
    pub rf: Option<Value>,
    /// an edge's ends
    pub a: [f64; 3],
    pub b: [f64; 3],
}

impl std::fmt::Debug for PartView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PartView").field("file", &self.file).field("dirty", &self.dirty).finish_non_exhaustive()
    }
}

impl PartView {
    /// A part read into memory (any problem is the reader's error, as partcraft.js `openText` shows it).
    pub fn new(part: Value, file: &str) -> PartView {
        let mut pv = PartView {
            part,
            file: file.to_string(),
            regen: kernel::regenerate(&Value::Null, None),
            regen_ms: 0.0,
            undo: Vec::new(),
            redo: Vec::new(),
            dirty: false,
            scene: Scene3D::new(0, Vec::new(), Vec::new(), [f64::NAN; 3], [f64::NAN; 3]),
            view: View3::default(),
            scene_seq: 0,
            inst_face: Vec::new(),
            hover: None,
            sel_face: None,
            sel_feature: None,
            message: String::new(),
            tab: "props",
            edits: std::collections::HashMap::new(),
            new_param: (String::new(), String::new()),
            tints: ([120, 170, 230, 255], [70, 130, 240, 255]),
            fit_pending: true,
            hover_key: None,
            mode: Mode::Model,
            editing: None,
            sel_plane: None,
            pick: None,
            multi: Vec::new(),
            pending_sketch: false,
            last_solve: None,
            sk: crate::partsketch::SketchEd::default(),
            note: String::new(),
            coords: None,
            confirm: None,
            num_entry: ("line".into(), String::new(), String::new()),
            name_edit: None,
            cmd_ms: (0.0, 0),
            rebuild_total: (0.0, 0),
            #[cfg(not(target_arch = "wasm32"))]
            bg: None,
            generation: 0,
            bg_again: false,
            background: false,
        };
        pv.rebuild();
        pv.view.cam.fit(pv.scene.min, pv.scene.max, Some(ISO_DIR));
        pv.fit_pending = true;
        let n = P::features(&pv.part).len();
        let errs = pv.failing();
        pv.note = if errs > 0 {
            trv("app.file.opened_failing", &[("name", file.to_string()), ("n", n.to_string()), ("errs", errs.to_string())])
        } else {
            trv("app.file.opened", &[("name", file.to_string()), ("n", n.to_string())])
        };
        pv
    }

    /// partcraft.js `newPart()`: an empty part named Part1.
    pub fn new_part() -> PartView {
        let mut part = P::new_part("Part1");
        // part.js newPart() writes version 2 (fileVersion() decides what a save says)
        jv::set(&mut part, "version", 2.into());
        let mut pv = PartView::new(part, "Part1");
        pv.view.cam.fit([-50.0, -50.0, -50.0], [50.0, 50.0, 50.0], Some(ISO_DIR));
        pv.note = tr("app.file.new_part");
        pv
    }

    /// The built-in sample part.
    pub fn sample() -> Result<PartView, String> {
        let r = pcscript::script_to_part(SAMPLE_SCRIPT, None).map_err(|e| script_error("sample", &e))?;
        Ok(PartView::new(r.part, "partcraft-bracket.pcscript"))
    }

    /// A part file: `.partcraft.json` (part.js readPart) or a `.pcscript` (script.js scriptToPart).
    pub fn open(name: &str, bytes: &[u8]) -> Result<PartView, String> {
        if bytes.len() > MAX_FILE_BYTES {
            return Err(tr("app.file.too_big"));
        }
        let text = String::from_utf8_lossy(bytes);
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".pcscript") || pcscript::looks_like_script(&text) {
            let r = pcscript::script_to_part(&text, None).map_err(|e| script_error(name, &e))?;
            return Ok(PartView::new(r.part, name));
        }
        let head: String = text.chars().take(400).collect();
        if head.contains("\"partcraft-assembly\"") {
            return Err(format!("{name}: assemblies do not open in the Rust test version yet"));
        }
        let raw: Value =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", trv("app.file.cannot_open", &[("name", name.to_string())])))?;
        let part = P::read_part(&raw).map_err(|e| format!("{}: {e}", trv("app.file.cannot_open", &[("name", name.to_string())])))?;
        Ok(PartView::new(part, name))
    }

    /// Features whose rebuild failed.
    pub fn failing(&self) -> usize {
        self.regen.results.values().filter(|r| jv::is_str(r.get("status"), "error")).count()
    }

    /// partcraft.js `rebuild` + `applyRegen`: the kernel rebuilds the part (up to the open sketch, or to the feature
    /// whose references are being picked); the solved sketches go back into it (the JS solves them in place), the view
    /// gets the new solid. While a sketch is open its solve runs before and after the rebuild, as the JS's does.
    pub fn rebuild(&mut self) {
        if self.mode == Mode::Sketch && self.editing_index().is_some() {
            self.last_solve = self.solve_open();
        }
        let up_to = match (self.mode, self.editing_index()) {
            (Mode::Sketch, Some(i)) => Some(js_f(i) + 1.0),
            (Mode::Pick, Some(i)) => Some(js_f(i)),
            _ => None,
        };
        self.generation = self.generation.wrapping_add(1);
        #[cfg(not(target_arch = "wasm32"))]
        if self.mode == Mode::Model && self.regen_ms > BACKGROUND_MS && self.background {
            if self.bg.is_some() {
                self.bg_again = true;
            } else {
                self.spawn_rebuild();
            }
            self.message = tr("app.build.rebuilding");
            return;
        }
        let t0 = web_time::Instant::now();
        let r = kernel::regenerate(&self.part, up_to);
        self.regen_ms = ms_since(t0);
        self.rebuild_total = (self.rebuild_total.0 + self.regen_ms, self.rebuild_total.1 + 1);
        self.apply_regen(r);
    }

    /// Send the part as it is now to a rebuild thread.
    #[cfg(not(target_arch = "wasm32"))]
    fn spawn_rebuild(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        let part = self.part.clone();
        let spawned = std::thread::Builder::new().name("part-rebuild".into()).spawn(move || {
            let t0 = web_time::Instant::now();
            let r = kernel::regenerate(&part, None);
            let _ = tx.send((r, ms_since(t0)));
        });
        match spawned {
            Ok(_) => self.bg = Some((self.generation, rx)),
            Err(_) => {
                // no thread: rebuild here
                let t0 = web_time::Instant::now();
                let r = kernel::regenerate(&self.part, None);
                self.regen_ms = ms_since(t0);
                self.apply_regen(r);
            }
        }
    }

    /// Is a rebuild running on a thread?
    pub fn rebuilding(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.bg.is_some()
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }

    /// Every frame: a finished thread rebuild is applied when the part has not changed since it was sent; otherwise
    /// the newest part goes to the thread (partcraft.js `requestExact`). True while one is still running.
    pub fn poll_rebuild(&mut self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some((generation, rx)) = self.bg.as_ref() else { return false };
            let generation = *generation;
            match rx.try_recv() {
                Err(std::sync::mpsc::TryRecvError::Empty) => true,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.bg = None;
                    self.bg_again = false;
                    self.message = tr("app.build.kernel_error");
                    false
                }
                Ok((r, ms)) => {
                    self.bg = None;
                    self.rebuild_total = (self.rebuild_total.0 + ms, self.rebuild_total.1 + 1);
                    if generation == self.generation && !self.bg_again {
                        self.regen_ms = ms;
                        self.apply_regen(r);
                        return false;
                    }
                    self.bg_again = false;
                    self.spawn_rebuild();
                    true
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        false
    }

    /// partcraft.js `applyRegen(r)`.
    pub fn apply_regen(&mut self, r: kernel::Regen) {
        if let Some(feats) = self.part.get_mut("features").and_then(Value::as_array_mut) {
            for f in feats.iter_mut() {
                let id = jv::s(f, "id").to_string();
                if let Some(sk) = r.solved.get(&id) {
                    jv::set(f, "sketch", sk.clone());
                }
            }
        }
        self.regen = r;
        let nfaces = self.faces().len();
        if self.hover.is_some_and(|h| h >= nfaces) {
            self.hover = None;
        }
        self.sel_face = None;
        if self.pick.as_ref().is_some_and(|p| p.face) {
            self.pick = None;
        }
        if let Some(p) = self.pick.clone().filter(|p| !p.face) {
            self.pick = p.rf.as_ref().and_then(|r| self.find_edge(r)).and_then(|i| self.edge_pick(i));
        }
        let multi = std::mem::take(&mut self.multi);
        self.multi = multi
            .into_iter()
            .filter(|m| !m.face)
            .filter_map(|m| m.rf.as_ref().and_then(|r| self.find_edge(r)).and_then(|i| self.edge_pick(i)))
            .collect();
        self.build_scene();
        self.message = self.build_words();
        if self.mode == Mode::Sketch && self.editing.is_some() {
            self.last_solve = self.solve_open();
        }
    }

    fn editing_index(&self) -> Option<usize> {
        self.editing.as_deref().and_then(|id| P::index_of(&self.part, id))
    }

    /// `solveSketch(editingFeature().sketch)`: the open sketch solved in place.
    fn solve_open(&mut self) -> Option<cc_part::solver::Solved> {
        let i = self.editing_index()?;
        let sk = self.part.get_mut("features")?.as_array_mut()?.get_mut(i)?.get_mut("sketch")?;
        Some(cc_part::solver::solve_sketch(sk, None, None))
    }

    /// The build chip's words (partcraft.js `applyRegen`).
    pub fn build_words(&self) -> String {
        let errs = self.failing();
        if errs > 0 {
            i18n::t("app.build.failing", &[("n", i18n::F::from(errs))])
        } else if self.mode == Mode::Sketch {
            String::new()
        } else {
            i18n::t("app.build.rebuilt", &[("ms", i18n::F::N(self.regen_ms.round()))])
        }
    }

    /// The named faces of the solid (meshfaces.js records), when the rebuild named them.
    pub fn faces(&self) -> &[Value] {
        jv::arr(self.regen.faces.as_ref())
    }

    fn build_scene(&mut self) {
        self.scene_seq = self.scene_seq.wrapping_add(1);
        let mut meshes: Vec<MeshData> = Vec::new();
        let mut inst_face: Vec<Option<usize>> = Vec::new();
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        if let Some(m) = self.regen.raw_mesh.as_ref() {
            let nt = m.tri_count();
            let tri_face = m.tri_face.as_deref();
            let nfaces = self.faces().len();
            let per_face = nfaces > 0 && nfaces <= MAX_FACE_MESHES && tri_face.is_some_and(|tf| tf.len() == nt && !tf.contains(&-1));
            let groups = if per_face { nfaces } else { 1 };
            let mut tris: Vec<Vec<f64>> = vec![Vec::new(); groups];
            for t in 0..nt {
                let g = if per_face {
                    tri_face.and_then(|tf| tf.get(t)).and_then(|&f| usize::try_from(f).ok()).filter(|&f| f < groups).unwrap_or(0)
                } else {
                    0
                };
                let Some(out) = tris.get_mut(g) else { continue };
                for k in 0..3 {
                    let vi = m.indices.get(t * 3 + k).copied().unwrap_or(0) as usize;
                    let p = [
                        m.positions.get(vi * 3).copied().unwrap_or(0.0),
                        m.positions.get(vi * 3 + 1).copied().unwrap_or(0.0),
                        m.positions.get(vi * 3 + 2).copied().unwrap_or(0.0),
                    ];
                    let w = z_up(p);
                    for (i, v) in w.iter().enumerate() {
                        if let (Some(l), Some(h)) = (lo.get_mut(i), hi.get_mut(i)) {
                            *l = l.min(*v);
                            *h = h.max(*v);
                        }
                    }
                    out.extend_from_slice(&w);
                }
            }
            for (g, t) in tris.iter().enumerate() {
                let n = t.len() / 9;
                meshes.push(MeshData::from_triangles(t, &vec![BODY; n], &[]));
                inst_face.push(per_face.then_some(g));
            }
            // the solid's edges (meshfaces.js edgeSegs), one mesh with no faces
            let segs: Vec<f64> =
                jv::arr(self.regen.mesh.as_ref().and_then(|x| x.get("edgeSegs"))).iter().map(|x| x.as_f64().unwrap_or(0.0)).collect();
            if !segs.is_empty() {
                let mut e = Vec::with_capacity(segs.len());
                for [x, y, z] in segs.as_chunks::<3>().0 {
                    e.extend_from_slice(&z_up([*x, *y, *z]));
                }
                meshes.push(MeshData::from_triangles(&[], &[], &e));
                inst_face.push(None);
            }
        }
        let instances: Vec<Instance3> = (0..meshes.len()).map(|i| Instance3 { mesh: i, m: IDENTITY, tint: None, hidden: false }).collect();
        let (min, max) = if lo[0].is_finite() { (lo, hi) } else { ([-50.0, -50.0, -50.0], [50.0, 50.0, 50.0]) };
        self.scene = Scene3D::new(self.scene_seq, meshes, instances, min, max);
        self.inst_face = inst_face;
        self.retint();
    }

    /// Paint the hovered and selected faces.
    pub fn retint(&mut self) {
        let (h, s) = (self.hover, self.sel_face);
        let (th, ts) = self.tints;
        let faces = self.inst_face.clone();
        // Shift-picked faces and the faces of the shell being picked for are marked too
        let mut marked: Vec<usize> = self.multi.iter().filter(|m| m.face).map(|m| m.index).collect();
        if self.mode == Mode::Pick
            && let Some(f) = self.editing.as_deref().and_then(|id| P::feature_by_id(&self.part, id))
        {
            marked.extend(jv::arr(f.get("faces")).iter().filter_map(|r| self.find_face(r)));
        }
        self.scene.set_tints(&|i| {
            let f = faces.get(i).copied().flatten()?;
            if Some(f) == s || marked.contains(&f) {
                Some(ts)
            } else if Some(f) == h {
                Some(th)
            } else {
                None
            }
        });
    }

    /// The face under pixel (sx, sy) of the view (physical pixels): the nearest triangle the camera's ray meets, in
    /// the part's own frame, through the mesh's triangle → face table.
    pub fn face_at(&self, sx: f64, sy: f64) -> Option<usize> {
        let m = self.regen.raw_mesh.as_ref()?;
        let tf = m.tri_face.as_deref()?;
        let (o, d) = self.view.cam.ray(sx, sy);
        let (o, d) = (y_up(o), y_up(d));
        let pt = |vi: u32| -> [f64; 3] {
            let i = vi as usize * 3;
            [
                m.positions.get(i).copied().unwrap_or(0.0),
                m.positions.get(i + 1).copied().unwrap_or(0.0),
                m.positions.get(i + 2).copied().unwrap_or(0.0),
            ]
        };
        let mut best: Option<(f64, usize)> = None;
        for (t, tri) in m.indices.as_chunks::<3>().0.iter().enumerate() {
            let [a, b, c] = tri.map(pt);
            if let Some(hit) = ray_tri(o, d, a, b, c)
                && best.is_none_or(|(bt, _)| hit < bt)
            {
                best = Some((hit, t));
            }
        }
        let (_, t) = best?;
        tf.get(t).and_then(|&f| usize::try_from(f).ok())
    }

    /// A point of the part on the view (physical pixels), if it is in front of the camera.
    pub fn screen_of(&self, p: [f64; 3]) -> Option<(f64, f64)> {
        let cam = &self.view.cam;
        let w = z_up(p);
        let (x, y, z) = cam.axes();
        let d = [w[0] - cam.eye[0], w[1] - cam.eye[1], w[2] - cam.eye[2]];
        let (vx, vy, vz) =
            (d[0] * x[0] + d[1] * x[1] + d[2] * x[2], d[0] * y[0] + d[1] * y[1] + d[2] * y[2], d[0] * z[0] + d[1] * z[1] + d[2] * z[2]);
        let th = (cam.fov.to_radians() / 2.0).tan();
        let (nx, ny) = if cam.persp {
            if vz >= -1e-9 {
                return None;
            }
            (vx / (-vz * th * cam.aspect()), vy / (-vz * th))
        } else {
            let hh = cam.distance() * th;
            (vx / (hh * cam.aspect()), vy / hh)
        };
        Some(((nx + 1.0) / 2.0 * cam.width, (1.0 - ny) / 2.0 * cam.height))
    }

    /// The named edge nearest pixel (sx, sy) within `tol` pixels (the solid's edge segments, meshfaces.js `segEdge`).
    pub fn edge_at(&self, sx: f64, sy: f64, tol: f64) -> Option<usize> {
        let mesh = self.regen.mesh.as_ref()?;
        let segs: Vec<f64> = jv::arr(mesh.get("edgeSegs")).iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect();
        let owner = jv::arr(mesh.get("segEdge"));
        let mut best: Option<(f64, usize)> = None;
        for (k, s) in segs.as_chunks::<6>().0.iter().enumerate() {
            let (Some(a), Some(b)) = (self.screen_of([s[0], s[1], s[2]]), self.screen_of([s[3], s[4], s[5]])) else { continue };
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let l2 = dx * dx + dy * dy;
            let t = if l2 > 0.0 { (((sx - a.0) * dx + (sy - a.1) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
            let d = (sx - a.0 - t * dx).hypot(sy - a.1 - t * dy);
            if d <= tol
                && best.is_none_or(|(bd, _)| d < bd)
                && let Some(e) = owner.get(k).and_then(Value::as_u64).and_then(|e| usize::try_from(e).ok())
            {
                best = Some((d, e));
            }
        }
        best.map(|(_, e)| e)
    }

    /// `[name, record]` of face `i`.
    pub fn face(&self, i: usize) -> Option<&Value> {
        self.faces().get(i)
    }

    /// partcraft.js `faceWords(name)`: "Extrude1 end face", "Hole1 hole wall (copy 3 of CirPattern1)".
    pub fn face_words(&self, name: &str) -> String {
        face_words(&self.part, name)
    }

    /// One undo step (partcraft.js `commit`): the part before goes on the undo stack, `f` changes it, the part is
    /// rebuilt. An `Err` from `f` leaves the part as it was.
    pub fn commit(&mut self, label: &str, f: impl FnOnce(&mut Value) -> Result<(), String>) -> Result<(), String> {
        let before = self.part.clone();
        if let Err(e) = f(&mut self.part) {
            self.part = before;
            return Err(e);
        }
        self.undo.push((label.to_string(), before));
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
        self.rebuild();
        Ok(())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// partcraft.js `undoRedo`.
    pub fn undo_redo(&mut self, undo: bool) {
        let (from, to) = if undo { (&mut self.undo, &mut self.redo) } else { (&mut self.redo, &mut self.undo) };
        let Some((label, snap)) = from.pop() else {
            self.note = tr(if undo { "app.undo.nothing_undo" } else { "app.undo.nothing_redo" });
            return;
        };
        let now = std::mem::replace(&mut self.part, snap);
        to.push((label.clone(), now));
        self.dirty = true;
        self.edits.clear();
        if let Some(id) = self.editing.clone()
            && P::feature_by_id(&self.part, &id).is_none()
        {
            if self.mode == Mode::Sketch {
                self.sk.active = false;
                self.sk.ask = None;
            }
            self.mode = Mode::Model;
            self.editing = None;
        }
        if let Some(id) = self.sel_feature.clone()
            && P::feature_by_id(&self.part, &id).is_none()
        {
            self.sel_feature = None;
        }
        self.sk.sel.clear();
        self.rebuild();
        self.note = trv(if undo { "app.undo.undid" } else { "app.undo.redid" }, &[("what", label.to_lowercase())]);
    }

    // ------------------------------------------------------------------ the sketch editor's host (sketchview.js `host`)

    /// The open sketch (an empty one when none is open).
    pub fn sketch(&self) -> &Value {
        static EMPTY: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
        self.editing
            .as_deref()
            .and_then(|id| P::feature_by_id(&self.part, id))
            .and_then(|f| f.get("sketch"))
            .filter(|s| s.is_object())
            .unwrap_or_else(|| EMPTY.get_or_init(cc_part::sketch::new_sketch))
    }

    fn sketch_mut(&mut self) -> Option<&mut Value> {
        let i = self.editing_index()?;
        self.part.get_mut("features")?.as_array_mut()?.get_mut(i)?.get_mut("sketch")
    }

    /// Change the open sketch without an undo step (a dimension label being dragged).
    pub fn with_sketch(&mut self, f: impl FnOnce(&mut Value)) {
        if let Some(sk) = self.sketch_mut() {
            f(sk);
        }
    }

    /// partcraft.js `status(msg)`.
    pub fn set_note(&mut self, msg: &str) {
        self.note = msg.to_string();
    }

    /// The relations the last solve says conflict or repeat.
    pub fn bad_ids(&self) -> Vec<String> {
        let Some(s) = &self.last_solve else { return Vec::new() };
        let mut v = s.conflicts.clone();
        for r in &s.redundant {
            if !v.contains(r) {
                v.push(r.clone());
            }
        }
        v
    }

    fn push_undo(&mut self, label: &str, before: Value) {
        self.undo.push((label.to_string(), before));
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// partcraft.js `sketchCommit(label, fn, { squash })`: `f` edits the open sketch as one undo step (or joins the last
    /// one with `squash`), the sketch is solved, conflicts are reported, the part is rebuilt. An `Err` leaves it as it was.
    pub fn sketch_commit<T>(&mut self, label: &str, squash: bool, f: impl FnOnce(&mut Value) -> Result<T, String>) -> Result<T, String> {
        let before = self.part.clone();
        let sk = self.sketch_mut().ok_or("no sketch is open")?;
        let r = match f(sk) {
            Ok(r) => r,
            Err(e) => {
                self.part = before;
                return Err(e);
            }
        };
        if !squash {
            self.push_undo(label, before);
        }
        self.dirty = true;
        self.last_solve = self.solve_open();
        if let Some(s) = self.last_solve.clone() {
            let sk = self.sketch().clone();
            let describe = |id: &String| {
                cc_part::sketch::constraints(&sk)
                    .iter()
                    .find(|c| jv::is_str(c.get("id"), id))
                    .map_or_else(|| id.clone(), crate::partsketch::describe3)
            };
            if !s.conflicts.is_empty() {
                let mut all: Vec<String> = Vec::new();
                for id in s.conflicts.iter().chain(s.redundant.iter()) {
                    if !all.contains(id) {
                        all.push(id.clone());
                    }
                }
                let list: Vec<String> = all.iter().map(describe).collect();
                self.note = trv("app.sketch.over_conflict", &[("list", list.join(", "))]);
            } else if !s.redundant.is_empty() {
                let list: Vec<String> = s.redundant.iter().map(describe).collect();
                self.note =
                    i18n::t("app.sketch.over_redundant", &[("n", i18n::F::from(s.redundant.len())), ("list", i18n::F::S(list.join(", ")))]);
            }
        }
        self.rebuild();
        Ok(r)
    }

    /// `dragStart(label)`: the part before the drag is the undo step.
    pub fn drag_start(&mut self, label: &str) {
        let before = self.part.clone();
        self.push_undo(label, before);
        self.dirty = true;
    }

    /// `dragSolve(pins, pinRadius)`: the open sketch solved with points (or a circle's radius) pinned under the pointer.
    pub fn drag_solve(&mut self, pins: Option<&cc_part::omap::OMap<[f64; 2]>>, radius: Option<&cc_part::omap::OMap<f64>>) {
        if let Some(sk) = self.sketch_mut() {
            let s = cc_part::solver::solve_sketch(sk, pins, radius);
            self.last_solve = Some(s);
        }
    }

    /// `dragEnd()`.
    pub fn drag_end(&mut self) {
        self.rebuild();
    }

    /// The editor pasted a clip (partcraft.js `pasted`).
    pub fn pasted(&mut self, label: &str, clip: &Value, made: &Value) {
        let rel = jv::arr(made.get("constraints")).len();
        let dropped = jv::n(clip, "dropped");
        let key = if label == tr("app.undo.duplicate") { "app.clip.sketch_duplicated" } else { "app.clip.sketch_pasted" };
        self.note = trv(
            key,
            &[
                ("what", cc_part::clipboard::describe_clip(clip)),
                ("relations", if rel > 0 { trv("app.clip.sketch_with_relations", &[("n", rel.to_string())]) } else { String::new() }),
                (
                    "dropped",
                    if dropped > 0.0 { trv("app.clip.sketch_dropped", &[("n", cc_part::js::num_str(dropped))]) } else { String::new() },
                ),
            ],
        );
    }

    /// params-panel.js `dimTyped(sketchId, cid, raw)`: "=width / 2" in a dimension's box links it (true = handled).
    pub fn dim_typed(&mut self, cid: &str, raw: &str) -> bool {
        let Some(sid) = self.editing.clone() else { return false };
        let key = format!("{sid}.{cid}");
        let s = raw.trim();
        let linked = PP::params_of(&self.part).link(&key).is_some();
        let lit = cc_part::expr::literal(s);
        if !s.starts_with('=') && lit.is_some() && !linked {
            return false;
        }
        if !s.starts_with('=') && lit.is_none() && !s.chars().any(|c| c.is_ascii_alphabetic() || c == '_') {
            return false;
        }
        match self.set_param(&format!("{key}={s}")) {
            Ok(w) => {
                if !w.is_empty() {
                    self.note = w;
                }
            }
            Err(e) => self.note = e,
        }
        true
    }

    /// The open sketch's frame (partcraft.js `currentFrame`).
    pub fn current_frame(&self) -> Option<cc_part::vec::Frame> {
        let id = self.editing.as_deref()?;
        if let Some(f) = self.regen.frames.get(id) {
            return Some(*f);
        }
        let f = P::feature_by_id(&self.part, id)?;
        plane_frame(f.get("plane"))
    }

    // ------------------------------------------------------------------ sketch mode (partcraft.js startSketch … exitSketch)

    /// The Sketch button (partcraft.js `startSketch`): edit the selected sketch, or start one on the selected plane or
    /// the picked flat face; with nothing chosen the next plane or face pick starts it.
    pub fn start_sketch(&mut self) {
        if self.mode == Mode::Sketch {
            return;
        }
        if self.mode == Mode::Pick {
            self.end_pick();
        }
        if let Some(id) = self.sel_feature.clone()
            && let Some(f) = P::feature_by_id(&self.part, &id)
        {
            let ty = jv::s(f, "type").to_string();
            if ty == "sketch" {
                self.edit_sketch(&id, false);
                return;
            }
            if ty == "plane" && self.regen.ref_geom.get(&id).is_some() {
                self.start_sketch_on(jv::obj(vec![("kind", "ref".into()), ("feature", id.as_str().into())]));
                return;
            }
        }
        if let Some(p) = self.pick.clone().filter(|p| p.face) {
            if !p.flat {
                self.note = tr("app.sketch.face_curved");
                return;
            }
            if let Some(r) = p.rf {
                self.start_sketch_on(r);
            }
            return;
        }
        if let Some(name) = self.sel_plane.clone() {
            self.start_sketch_on(jv::obj(vec![("kind", "base".into()), ("name", name.as_str().into())]));
            return;
        }
        self.pending_sketch = true;
        self.note = tr("app.sketch.pick_plane");
    }

    /// partcraft.js `startSketchOn(plane)`: a new sketch on a plane reference, opened for drawing.
    pub fn start_sketch_on(&mut self, plane: Value) {
        self.pending_sketch = false;
        let mut id = String::new();
        let r = self.commit(&tr("app.undo.new_sketch"), |p| {
            let f = P::add_feature(p, "sketch", &jv::obj(vec![("plane", plane)]))?;
            id = jv::s(&f, "id").to_string();
            Ok(())
        });
        match r {
            Ok(()) => self.edit_sketch(&id, true),
            Err(e) => self.note = e,
        }
    }

    /// partcraft.js `editSketch(id, fresh)`.
    pub fn edit_sketch(&mut self, id: &str, fresh: bool) {
        let Some(f) = P::feature_by_id(&self.part, id) else { return };
        if jv::s(f, "type") != "sketch" {
            return;
        }
        let res = self.regen.results.get(id);
        if res.is_some_and(|r| jv::is_str(r.get("status"), "skipped"))
            || (self.regen.frames.get(id).is_none() && plane_frame(f.get("plane")).is_none())
        {
            let reason =
                res.and_then(|r| r.get("message")).and_then(Value::as_str).map_or_else(|| tr("app.sketch.plane_gone"), str::to_string);
            self.note = trv("app.sketch.cannot_open", &[("reason", reason)]);
            return;
        }
        let fname = jv::s(f, "name").to_string();
        self.mode = Mode::Sketch;
        self.editing = Some(id.to_string());
        self.sel_feature = Some(id.to_string());
        self.sel_plane = None;
        self.pick = None;
        self.hover = None;
        self.rebuild();
        let Some(frame) = self.current_frame() else {
            self.exit_sketch();
            self.note = tr("app.sketch.no_plane");
            return;
        };
        self.sk.active = true;
        self.sk.sel.clear();
        let (w, h) = self.sk.size;
        let (size, c) = match self.regen.stats.as_ref() {
            Some(st) => {
                let lo = jv::arr(st.get("bbox").and_then(|b| b.get("min")));
                let hi = jv::arr(st.get("bbox").and_then(|b| b.get("max")));
                let g = |a: &[Value], i: usize| a.get(i).and_then(Value::as_f64).unwrap_or(f64::NAN);
                let size = cc_part::js::max_of((0..3).map(|i| g(hi, i) - g(lo, i)));
                let mid = [(g(lo, 0) + g(hi, 0)) / 2.0, (g(lo, 1) + g(hi, 1)) / 2.0, (g(lo, 2) + g(hi, 2)) / 2.0];
                (size, cc_part::vec::to_plane(&frame, mid))
            }
            None => (100.0, [0.0, 0.0]),
        };
        let span = size * 1.6;
        let span = if span == 0.0 || span.is_nan() { 160.0 } else { span };
        self.sk.view = (c[0], c[1], cc_part::js::max(0.01, cc_part::js::min(w, h) / span));
        if !fresh && cc_part::sketch::bounds(self.sketch()).is_some() {
            let sk = self.sketch().clone();
            self.sk.fit(&sk);
        }
        let mut ed = std::mem::take(&mut self.sk);
        ed.set_tool(self, if fresh { "line" } else { "select" });
        self.sk = ed;
        self.note = if fresh { tr("app.sketch.draw_profile") } else { trv("app.sketch.editing", &[("name", fname)]) };
    }

    /// partcraft.js `exitSketch()`.
    pub fn exit_sketch(&mut self) {
        if self.mode != Mode::Sketch {
            return;
        }
        self.sk.ask = None;
        self.sk.active = false;
        self.sk.step = None;
        self.sk.size_pin = None;
        let id = self.editing.take();
        self.mode = Mode::Model;
        self.last_solve = None;
        self.coords = None;
        self.sel_feature = id.filter(|id| P::feature_by_id(&self.part, id).is_some());
        self.rebuild();
        self.standard("iso");
        self.note = String::new();
    }

    /// The sketch's name and plane as the sketch bar says them.
    pub fn sketch_title(&self) -> String {
        let Some(f) = self.editing.as_deref().and_then(|id| P::feature_by_id(&self.part, id)) else { return String::new() };
        trv("app.sketch.name_on", &[("name", jv::s(f, "name").to_string()), ("plane", self.plane_label(f.get("plane")))])
    }

    /// partcraft.js `planeLabel(p)`.
    pub fn plane_label(&self, p: Option<&Value>) -> String {
        let Some(p) = p.filter(|p| p.is_object()) else { return String::new() };
        match jv::s(p, "kind") {
            "base" => plane_name(jv::s(p, "name")),
            "face" => {
                let n = P::feature_by_id(&self.part, jv::s(p, "feature"))
                    .map_or_else(|| tr("app.plane.deleted_feature"), |f| jv::s(f, "name").to_string());
                trv("app.plane.face_of", &[("name", n)])
            }
            "ref" => P::feature_by_id(&self.part, jv::s(p, "feature"))
                .map_or_else(|| tr("app.plane.deleted_plane"), |f| jv::s(f, "name").to_string()),
            _ => tr("app.plane.fixed"),
        }
    }

    /// A base plane clicked in the history (partcraft.js tree `onclick`).
    pub fn select_plane(&mut self, name: &str) {
        if self.mode == Mode::Sketch {
            return;
        }
        self.sel_plane = Some(name.to_string());
        self.sel_feature = None;
        self.pick = None;
        self.sel_face = None;
        self.retint();
        if self.pending_sketch {
            self.start_sketch_on(jv::obj(vec![("kind", "base".into()), ("name", name.into())]));
        }
    }

    /// Is a face a flat face (partcraft.js `isFlatFace` with named faces).
    fn face_flat(&self, i: usize) -> bool {
        self.face(i).is_some_and(|f| jv::s(f, "type") == "plane")
    }

    /// partcraft.js `onPick` for a face (model mode): the face is picked; a flat face carries the plane a sketch would
    /// sit on; with Sketch waiting, it starts there.
    pub fn pick_face(&mut self, i: usize) {
        let flat = self.face_flat(i);
        let rf = if flat {
            self.face(i).map(|f| {
                let name = f.get("name").and_then(Value::as_str).filter(|n| !n.is_empty());
                match name {
                    Some(n) => ref_for_name(n),
                    None => {
                        let fix = |k: &str, d: usize| -> Value {
                            Value::Array(
                                jv::arr(f.get(k))
                                    .iter()
                                    .map(|v| jv::num(cc_part::js::fixed_num(v.as_f64().unwrap_or(f64::NAN), d)))
                                    .collect(),
                            )
                        };
                        jv::obj(vec![("kind", "fixed".into()), ("o", fix("o", 9)), ("n", fix("n", 12))])
                    }
                }
            })
        } else {
            None
        };
        self.multi.clear();
        self.pick = Some(Pick { face: true, index: i, flat, rf: rf.clone(), a: [0.0; 3], b: [0.0; 3] });
        self.sel_face = Some(i);
        if self.ref_feature().is_none() {
            self.sel_feature = None;
            self.sel_plane = None;
        }
        self.tab = "props";
        self.retint();
        if self.pending_sketch {
            match rf {
                Some(r) => self.start_sketch_on(r),
                None => self.note = tr("app.sketch.face_curved"),
            }
        }
    }

    /// A click on nothing in the 3D view.
    pub fn pick_none(&mut self) {
        self.multi.clear();
        self.pick = None;
        self.sel_face = None;
        if self.ref_feature().is_none() {
            self.sel_feature = None;
            self.sel_plane = None;
        }
        self.retint();
        if self.pending_sketch {
            self.note = tr("app.sketch.pick_plane");
        }
    }

    /// The selected feature when it takes references picked in the view (patterns, mirror).
    pub fn ref_feature(&self) -> Option<&Value> {
        let f = P::feature_by_id(&self.part, self.sel_feature.as_deref()?)?;
        matches!(jv::s(f, "type"), "lpattern" | "cpattern" | "mirror").then_some(f)
    }

    /// The edges of the rebuild (meshfaces.js records).
    pub fn edges(&self) -> &[Value] {
        jv::arr(self.regen.edges.as_ref())
    }

    /// brep.js `findEdge(edges, ref)`.
    pub fn find_edge(&self, r: &Value) -> Option<usize> {
        let mut faces: Vec<String> = jv::arr(r.get("faces")).iter().map(|x| cc_part::js::str_of(Some(x))).collect();
        faces.sort_by(|a, b| cc_part::js::cmp16(a, b));
        let key = faces.join("|");
        let at = r.get("at").filter(|a| jv::truthy(Some(a))).map(p3);
        let mut best = None;
        let mut bd = f64::INFINITY;
        for (i, e) in self.edges().iter().enumerate() {
            if jv::s(e, "key") != key {
                continue;
            }
            let d = at.map_or(0.0, |a| cc_part::vec::len(cc_part::vec::sub(p3(e.get("at").unwrap_or(&Value::Null)), a)));
            if d < bd {
                bd = d;
                best = Some(i);
            }
        }
        best
    }

    /// brep.js `findFace(faces, ref)`.
    pub fn find_face(&self, r: &Value) -> Option<usize> {
        let name = r.get("name");
        let at = r.get("at").filter(|a| jv::truthy(Some(a))).map(p3);
        let mut best = None;
        let mut bd = f64::INFINITY;
        for (i, f) in self.faces().iter().enumerate() {
            if !jv::strict_eq(f.get("name"), name) {
                continue;
            }
            let d = at.map_or(0.0, |a| cc_part::vec::len(cc_part::vec::sub(p3(f.get("at").unwrap_or(&Value::Null)), a)));
            if d < bd {
                bd = d;
                best = Some(i);
            }
        }
        best
    }

    /// partcraft.js `edgePick(i)`.
    pub fn edge_pick(&self, i: usize) -> Option<Pick> {
        let e = self.edges().get(i)?;
        Some(Pick { face: false, index: i, flat: false, rf: Some(edge_ref(e)), a: p3(e.get("a")?), b: p3(e.get("b")?) })
    }

    /// partcraft.js `facePickAt(fi)`.
    pub fn face_pick_at(&self, i: usize) -> Option<Pick> {
        let f = self.face(i)?;
        Some(Pick { face: true, index: i, flat: self.face_flat(i), rf: Some(face_ref(f)), a: [0.0; 3], b: [0.0; 3] })
    }

    /// partcraft.js `selectFeature(id)` (None = nothing selected).
    pub fn select_feature(&mut self, id: Option<String>) {
        if self.mode == Mode::Sketch {
            if id != self.editing {
                self.exit_sketch();
            } else {
                return;
            }
        }
        if self.mode == Mode::Pick {
            if id != self.editing {
                self.end_pick();
            } else {
                return;
            }
        }
        self.sel_feature = id;
        self.sel_plane = None;
    }

    /// partcraft.js `endPick()`.
    pub fn end_pick(&mut self) {
        if self.mode != Mode::Pick {
            return;
        }
        let id = self.editing.take();
        self.mode = Mode::Model;
        self.sel_feature = id.filter(|id| P::feature_by_id(&self.part, id).is_some());
        self.hover = None;
        self.rebuild();
        self.note = String::new();
    }

    /// A feature by id or name.
    pub fn feature_id(&self, key: &str) -> Option<String> {
        P::features(&self.part)
            .iter()
            .find(|f| jv::is_str(f.get("id"), key))
            .or_else(|| P::features(&self.part).iter().find(|f| jv::is_str(f.get("name"), key)))
            .map(|f| jv::s(f, "id").to_string())
    }

    /// partcraft.js `rollTo(n)`: the rollback bar above feature `n` (n ≥ the count = roll to the end).
    pub fn roll_to(&mut self, n: usize) -> Result<(), String> {
        let len = P::features(&self.part).len();
        self.commit(&tr("app.undo.roll_back"), |p| {
            jv::set(p, "rollback", if n >= len { Value::Null } else { jv::unum(n) });
            Ok(())
        })
    }

    /// The history's Suppress / Unsuppress (partcraft.js `setFeature`).
    pub fn toggle_suppress(&mut self, id: &str) -> Result<(), String> {
        let f = P::feature_by_id(&self.part, id).ok_or_else(|| format!("part.suppress: no feature {id}"))?;
        let label = tr(if jv::truthy(f.get("suppressed")) { "app.history.unsuppress" } else { "app.history.suppress" });
        let id = id.to_string();
        self.commit(&label, |p| {
            let f = p
                .get_mut("features")
                .and_then(Value::as_array_mut)
                .and_then(|a| a.iter_mut().find(|f| jv::is_str(f.get("id"), &id)))
                .ok_or("part.suppress: the feature is gone")?;
            if jv::truthy(f.get("suppressed")) {
                jv::del(f, "suppressed");
            } else {
                jv::set(f, "suppressed", true.into());
            }
            Ok(())
        })?;
        if let Some(r) = self.regen.results.get(&id).filter(|r| jv::is_str(r.get("status"), "error"))
            && let Some(f) = P::feature_by_id(&self.part, &id)
        {
            self.message = trv("app.feature.error", &[("name", jv::s(f, "name").to_string()), ("error", jv::s(r, "message").to_string())]);
        }
        Ok(())
    }

    /// `part.param`: "name=value" — a parameter (new or changed: params-panel.js `setVarFrom` / `addVar`), a number of
    /// the part (`typed`: set directly, or solved backward when an equation drives it) or, with "==" / "=expr", a link.
    /// Each is one undo step (params-panel.js `edit`); returns the status line.
    pub fn set_param(&mut self, spec: &str) -> Result<String, String> {
        let (k, v) = spec.split_once('=').ok_or_else(|| format!("part.param: \"{spec}\" is not name=value"))?;
        let (k, v) = (k.trim(), v.trim());
        if k.is_empty() || v.is_empty() {
            return Err(format!("part.param: \"{spec}\" is not name=value"));
        }
        let p0 = PP::params_of(&self.part);
        let key = if p0.var(k).is_some() { None } else { PP::resolve_target(&self.part, k) };
        let Some(key) = key else {
            // a parameter, new or changed
            let expr = v.trim_start_matches('=').trim().to_string();
            let existed = p0.var(k).is_some();
            let label = tr(if existed { "ui.params.undo.change_param" } else { "ui.params.undo.add" });
            let name = k.to_string();
            self.param_edit(&label, |p, _| {
                let r = params::set_var(p, &name, &expr, None, None);
                if r.ok { Ok(()) } else { Err(r.error.unwrap_or_default()) }
            })?;
            return Ok(if existed { String::new() } else { trv("ui.params.added", &[("name", k.to_string()), ("expr", expr)]) });
        };
        let targets = PP::targets(&self.part);
        let tg = targets.get(&key).cloned().ok_or_else(|| format!("part.param: no number {key}"))?;
        let linked = p0.link(&key).is_some();
        let lit = if v.starts_with('=') { None } else { cc_part::expr::literal(v) };
        match lit {
            None => {
                let expr = v.trim_start_matches('=').trim().to_string();
                if expr.is_empty() {
                    return Err(tr("ui.params.need_expr"));
                }
                let label = tr(if linked { "ui.params.undo.change_equation" } else { "ui.params.undo.link" });
                let k2 = key.clone();
                self.param_edit(&label, |p, part| {
                    let r = params::set_link(p, &PP::PartAdapter::default(), part, &k2, &expr);
                    if r.ok { Ok(()) } else { Err(r.error.unwrap_or_default()) }
                })
                .map_err(|e| trv("ui.params.target_error", &[("label", tg.label.clone()), ("error", e)]))?;
                Ok(trv("ui.params.linked", &[("label", tg.label.clone()), ("expr", expr)]))
            }
            Some(n) if linked => {
                let label = trv("ui.params.undo.change", &[("name", tg.label.clone())]);
                let mut dry = p0.clone();
                let r = params::drive_back(&mut dry, &PP::PartAdapter::default(), &self.part, &key, n, None);
                if let params::Drive::Fail { error, choices } = &r {
                    return Err(match choices {
                        Some(c) if c.len() > 1 => format!("{error} ({})", c.join(", ")),
                        _ => error.clone(),
                    });
                }
                let k2 = key.clone();
                self.param_edit(&label, |p, part| match params::drive_back(p, &PP::PartAdapter::default(), part, &k2, n, None) {
                    params::Drive::Fail { error, .. } => Err(error),
                    _ => Ok(()),
                })?;
                Ok(match r {
                    params::Drive::Var { var, from, to } => trv(
                        "ui.params.solve.done",
                        &[("name", var), ("from", cc_part::expr::format_number(from)), ("to", cc_part::expr::format_number(to))],
                    ),
                    params::Drive::Link { to, .. } | params::Drive::Set { to, .. } => trv("ui.params.solve.gives", &[("value", to)]),
                    _ => String::new(),
                })
            }
            Some(n) => {
                if let Some(vp) = params::value_problem(&tg, n) {
                    return Err(vp);
                }
                let label = trv("ui.params.undo.change", &[("name", tg.label.clone())]);
                self.commit(&label, |p| {
                    use params::Adapter;
                    PP::PartAdapter::default().set(p, &tg, n);
                    Ok(())
                })?;
                Ok(String::new())
            }
        }
    }

    /// params-panel.js `edit(label, op)`: `op` on a copy of the parameters; when it works, one undo step that puts
    /// them in the part, tidies them and applies the equations.
    fn param_edit(&mut self, label: &str, op: impl FnOnce(&mut params::Params, &Value) -> Result<(), String>) -> Result<(), String> {
        self.param_edit_cfg(label, |p, part| op(p, part).map(|()| None)).map(|_| ())
    }

    /// params-panel.js `edit(label, op)` with configurations: `op` may hand back a new configuration list
    /// (`Some(None)` = none left); either way the list follows renamed and deleted parameters (configs.js `followParams`).
    /// Returns whether the parameters' numbers all applied.
    fn param_edit_cfg(
        &mut self,
        label: &str,
        op: impl FnOnce(&mut params::Params, &Value) -> Result<Option<Option<cfg::Configs>>, String>,
    ) -> Result<bool, String> {
        let mut trial = PP::params_of(&self.part);
        let given = op(&mut trial, &self.part)?;
        let before = self.part.get("params").filter(|v| v.is_object()).map(|_| PP::params_of(&self.part));
        let current = self.part.get("configs").filter(|c| c.is_object()).map(cfg::Configs::from_json);
        let list = match given {
            Some(c) => c,
            None => current,
        };
        let followed = cfg::follow_params(list.as_ref(), before.as_ref(), Some(&trial));
        let mut applied: Option<params::Applied> = None;
        self.commit(label, |p| {
            match followed.as_ref().filter(|c| !c.rows.is_empty()) {
                Some(c) => jv::set(p, "configs", c.to_json()),
                None => jv::del(p, "configs"),
            }
            PP::put_params(p, &trial);
            PP::tidy_params(p);
            applied = Some(PP::apply_part_params(p, None));
            Ok(())
        })?;
        if let Some(a) = applied.filter(|a| !a.errors.is_empty()) {
            let tg = PP::targets(&self.part);
            let words: Vec<String> = a
                .errors
                .iter()
                .map(|e| {
                    let label = tg.get(&e.at).map_or_else(|| e.at.clone(), |t| t.label.clone());
                    trv("ui.params.target_error", &[("label", label), ("error", e.message.clone())])
                })
                .collect();
            self.note = words.join(" ");
            return Ok(false);
        }
        Ok(true)
    }

    /// The part's configurations (design-table rows).
    pub fn configs(&self) -> Option<cfg::Configs> {
        self.part.get("configs").filter(|c| c.is_object()).map(cfg::Configs::from_json)
    }

    fn values_text(row: &cfg::Row) -> String {
        row.values.iter().map(|(k, v)| format!("{k} {}", cc_part::expr::format_number(v.number()))).collect::<Vec<_>>().join(", ")
    }

    /// params-panel.js `saveCurrent(name, label)`.
    fn config_save(&mut self, name: &str, label: Option<String>) -> Result<(), String> {
        let clean = cfg::clean_name(Some(&Value::from(name)));
        if clean.is_empty() {
            self.note = tr("configs.need_name");
            return Ok(());
        }
        let p = PP::params_of(&self.part);
        if self.part.get("params").is_none_or(|v| !v.is_object()) || !p.vars.iter().any(params::is_driving) {
            self.note = tr("configs.need_params");
            return Ok(());
        }
        let row = cfg::row_from_current(Some(&p), Some(&Value::from(clean.as_str())));
        let label = label.unwrap_or_else(|| trv("configs.undo.save", &[("name", clean.clone())]));
        let cur = self.configs();
        let r = self.param_edit_cfg(&label, |_, _| {
            let mut c = cfg::save_row(cur.as_ref(), &row)?;
            c.active = Some(row.name.clone());
            Ok(Some(Some(c)))
        });
        match r {
            Ok(_) => self.note = trv("configs.saved", &[("name", clean), ("values", Self::values_text(&row))]),
            Err(e) => self.note = e,
        }
        Ok(())
    }

    /// `part.config`: "save:<name>", "update:<name>", "apply:<name>", "rename:<from>=<to>", "delete:<name>" (the
    /// Parameters tab's Configurations; params-panel.js), each one undo step.
    pub fn config_cmd(&mut self, spec: &str) -> Result<(), String> {
        let (kind, arg) = spec.split_once(':').unwrap_or((spec, ""));
        let cur = self.configs();
        let row_of = |n: &str| cur.as_ref().and_then(|c| c.rows.iter().find(|r| r.name == n).cloned());
        match kind {
            "save" => self.config_save(arg, None)?,
            "update" => {
                let row = row_of(arg).ok_or_else(|| format!("part.config: no configuration {arg}"))?;
                self.config_save(&row.name, Some(trv("configs.undo.update", &[("name", row.name.clone())])))?;
            }
            "apply" => {
                let row = row_of(arg).ok_or_else(|| format!("part.config: no configuration {arg}"))?;
                let label = trv("configs.undo.apply", &[("name", row.name.clone())]);
                let r = self.param_edit_cfg(&label, |p, _| {
                    cfg::apply_row(p, &row)?;
                    let mut c = cur.clone().ok_or("no configurations")?;
                    c.active = Some(row.name.clone());
                    Ok(Some(Some(c)))
                });
                match r {
                    Ok(true) => self.note = trv("configs.applied", &[("name", row.name.clone()), ("values", Self::values_text(&row))]),
                    Ok(false) => {}
                    Err(e) => self.note = e,
                }
            }
            "rename" => {
                let (from, to) = arg.split_once('=').ok_or_else(|| format!("part.config: \"{arg}\" is not from=to"))?;
                let row = row_of(from).ok_or_else(|| format!("part.config: no configuration {from}"))?;
                if cfg::clean_name(Some(&Value::from(to))) == row.name {
                    return Ok(());
                }
                let label = trv("configs.undo.rename", &[("name", row.name.clone())]);
                let to = Value::from(to);
                let r = self.param_edit_cfg(&label, |_, _| {
                    let c = cur.as_ref().ok_or("no configurations")?;
                    Ok(Some(Some(cfg::rename_row(c, &row.name, Some(&to))?)))
                });
                if let Err(e) = r {
                    self.note = e;
                }
            }
            "delete" => {
                let row = row_of(arg).ok_or_else(|| format!("part.config: no configuration {arg}"))?;
                let label = trv("configs.undo.delete", &[("name", row.name.clone())]);
                let r = self.param_edit_cfg(&label, |_, _| {
                    let c = cur.as_ref().ok_or("no configurations")?;
                    Ok(Some(cfg::delete_row(c, &row.name)))
                });
                match r {
                    Ok(_) => self.note = trv("configs.deleted", &[("name", row.name)]),
                    Err(e) => self.note = e,
                }
            }
            other => return Err(format!("part.config: save, update, apply, rename or delete, not {other}")),
        }
        Ok(())
    }

    /// params-panel.js `exportTable()`: the design table as CSV (file name, bytes, status), or the reason there is none.
    pub fn configs_csv(&self) -> Result<(String, Vec<u8>, String), String> {
        let p = PP::params_of(&self.part);
        if p.vars.is_empty() {
            return Err(tr("configs.nothing"));
        }
        let have = self.configs();
        let c = have.clone().unwrap_or_else(|| cfg::Configs {
            rows: vec![cfg::row_from_current(Some(&p), Some(&Value::from(tr("configs.current"))))],
            active: None,
        });
        let words = match &have {
            Some(h) => trv("configs.exported", &[("n", h.rows.len().to_string())]),
            None => tr("configs.exported_current"),
        };
        Ok((format!("{} design table.csv", self.file_base()), cfg::configs_csv(Some(&c)).into_bytes(), words))
    }

    /// partcraft.js `fileBase()`.
    pub fn file_base(&self) -> String {
        file_base(&self.part)
    }

    /// Save part: `<name>.partcraft.json` = `P.toJSON(part)`.
    pub fn save_bytes(&self) -> (String, Vec<u8>) {
        (format!("{}.partcraft.json", self.file_base()), P::to_json(&self.part).into_bytes())
    }

    /// partcraft.js `exportMesh(kind)` / `exportIfc()`: (file name, bytes, status line). `now_ms` is the clock STEP
    /// and IFC write into their headers.
    pub fn export(&self, kind: &str, now_ms: f64) -> Result<(String, Vec<u8>, String), String> {
        let Some(m) = self.regen.raw_mesh.as_ref() else { return Err(tr("app.export.no_solid")) };
        let name = self.file_base();
        let iso: String = crate::civilexport::iso_time(now_ms).chars().take(19).collect();
        match kind {
            "stl" => {
                let file = format!("{name}.stl");
                let n = m.tri_count().to_string();
                Ok((file.clone(), X::stl_binary(m, &name), trv("app.export.stl", &[("file", file), ("n", n)])))
            }
            "3mf" => {
                let file = format!("{name}.3mf");
                Ok((file.clone(), X::three_mf(m, &name), trv("app.export.3mf", &[("file", file)])))
            }
            "step" | "step214" => {
                let (text, counts) = X::step_faceted(m, &name, &iso, kind == "step214");
                let file = format!("{name}.step");
                let n = cc_part::js::string_of(counts.get("faces"));
                Ok((file.clone(), text.into_bytes(), trv("app.export.step_faceted", &[("file", file), ("n", n)])))
            }
            "ifc" => {
                let file = format!("{name}.ifc");
                let (text, counts) =
                    cc_part::ifc_export::part_ifc(&self.part, &m.positions, &m.indices, self.regen.stats.as_ref(), &iso, Some(&file));
                let words = trv(
                    "app.export.ifc",
                    &[
                        ("file", file.clone()),
                        ("psets", cc_part::js::string_of(counts.get("psets"))),
                        ("n", cc_part::js::string_of(counts.get("properties"))),
                    ],
                );
                Ok((file, text.into_bytes(), words))
            }
            other => Err(format!("part.export: unknown format {other} (stl, 3mf, step, step214, ifc)")),
        }
    }

    /// The status line: the hovered face, the message (partcraft.js `status`), the build chip.
    pub fn status(&self) -> String {
        let hover = self.hover.and_then(|i| self.face(i)).map(|f| self.face_words(jv::s(f, "name"))).filter(|s| s != "?");
        let parts: Vec<&str> =
            [hover.as_deref().unwrap_or(""), self.note.as_str(), self.message.as_str()].into_iter().filter(|s| !s.is_empty()).collect();
        parts.join(" · ")
    }

    pub fn fit(&mut self) {
        self.fit_pending = true;
    }

    /// A standard view (iso, front, back, top, bottom, right, left; the part's own directions).
    pub fn standard(&mut self, name: &str) {
        let dir = match name {
            "front" => [0.0, -1.0, 0.0],
            "back" => [0.0, 1.0, 0.0],
            "top" => [0.0, -1e-4, 1.0],
            "bottom" => [0.0, -1e-4, -1.0],
            "right" => [1.0, 0.0, 0.0],
            "left" => [-1.0, 0.0, 0.0],
            _ => ISO_DIR,
        };
        self.view.cam.fit(self.scene.min, self.scene.max, Some(dir));
        self.fit_pending = false;
    }
}

const ISO_DIR: [f64; 3] = [1.0, -1.15, 0.85];
const IDENTITY: [f64; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
/// The solid's colour (a light machined grey, the same in both themes, as PartCraft's).
const BODY: [u8; 4] = [196, 202, 210, 255];

fn script_error(name: &str, e: &pcscript::ScriptError) -> String {
    let lines: Vec<String> = e
        .errors
        .iter()
        .map(|p| match p.line {
            Some(l) => trv("ui.params.script.line", &[("line", l.to_string()), ("message", p.message.clone())]),
            None => p.message.clone(),
        })
        .collect();
    trv("ui.params.script.cannot", &[("file", name.to_string()), ("error", lines.join(" "))])
}

/// `[x, y, z]` of a JSON array.
pub fn p3(v: &Value) -> [f64; 3] {
    let a = jv::arr(Some(v));
    let g = |i: usize| a.get(i).and_then(Value::as_f64).unwrap_or(f64::NAN);
    [g(0), g(1), g(2)]
}

/// brep.js `edgeRef(e)`: the two face names and the edge's point to 6 places.
pub fn edge_ref(e: &Value) -> Value {
    let at =
        Value::Array(jv::arr(e.get("at")).iter().map(|v| jv::num(cc_part::js::fixed_num(v.as_f64().unwrap_or(f64::NAN), 6))).collect());
    jv::obj(vec![("faces", e.get("faces").cloned().unwrap_or(Value::Array(Vec::new()))), ("at", at)])
}

/// brep.js `faceRef(f)`.
pub fn face_ref(f: &Value) -> Value {
    let at =
        Value::Array(jv::arr(f.get("at")).iter().map(|v| jv::num(cc_part::js::fixed_num(v.as_f64().unwrap_or(f64::NAN), 6))).collect());
    jv::obj(vec![("name", f.get("name").cloned().unwrap_or(Value::Null)), ("at", at)])
}

/// brep.js `refForName(name)`: the PlaneRef of a named flat face.
pub fn ref_for_name(name: &str) -> Value {
    // /^(f\d+)\.(start|end|side)(?::([A-Za-z0-9_:-]+))?$/
    if let Some((fid, rest)) = name.split_once('.')
        && fid.len() > 1
        && fid.starts_with('f')
        && fid.chars().skip(1).all(|c| c.is_ascii_digit())
    {
        let (face, ent) = match rest.split_once(':') {
            Some((a, b)) => (a, Some(b)),
            None => (rest, None),
        };
        let ent_ok = ent.is_none_or(|e| !e.is_empty() && e.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ':' | '-')));
        if matches!(face, "start" | "end" | "side") && ent_ok && (face != "side" || ent.is_some()) {
            let mut v = jv::obj(vec![("kind", "face".into()), ("feature", fid.into()), ("face", face.into())]);
            if let Some(e) = ent {
                jv::set(&mut v, "entity", e.into());
            }
            return v;
        }
    }
    let first = name_features(name).into_iter().next();
    jv::obj(vec![
        ("kind", "face".into()),
        ("feature", first.map_or(Value::Null, Value::from)),
        ("face", "named".into()),
        ("name", name.into()),
    ])
}

/// brep.js `nameFeatures(name)`: the features a face name mentions.
pub fn name_features(name: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let digits = |s: &str| -> String { s.chars().take_while(char::is_ascii_digit).collect() };
    if let Some(rest) = name.strip_prefix('f') {
        let d = digits(rest);
        if !d.is_empty() && rest.get(d.len()..).is_some_and(|r| r.starts_with('.')) {
            out.push(format!("f{d}"));
        }
    }
    let chars: Vec<char> = name.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if (*c == '@' || *c == '~') && chars.get(i + 1) == Some(&'f') {
            let d: String = chars.iter().skip(i + 2).take_while(|c| c.is_ascii_digit()).collect();
            if !d.is_empty() {
                let id = format!("f{d}");
                if !out.contains(&id) {
                    out.push(id);
                }
            }
        }
    }
    out
}

/// vec.js `PLANE_NAMES[name]`.
pub fn plane_name(name: &str) -> String {
    match name {
        "front" => tr("model.plane.front"),
        "top" => tr("model.plane.top"),
        "right" => tr("model.plane.right"),
        other => other.to_string(),
    }
}

/// A plane reference's frame without a rebuild (base planes; kernel.js `planeFrame` for the rest needs the build).
pub fn plane_frame(p: Option<&Value>) -> Option<cc_part::vec::Frame> {
    let p = p?;
    if jv::s(p, "kind") == "base" {
        return cc_part::vec::base_plane(jv::s(p, "name"));
    }
    None
}

/// partcraft.js `fileBase()`: the part's name with anything but `A–Z a–z 0–9 space _ . -` as `_`, trimmed, ≤ 80.
pub fn file_base(part: &Value) -> String {
    let name = part.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("part");
    let mut out = String::new();
    let mut run = false;
    for c in name.chars() {
        let ok = c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '.' | '-');
        if ok {
            out.push(c);
            run = false;
        } else if !run {
            // /[^…]+/g: a run of other characters becomes one "_"
            out.push('_');
            run = true;
        }
        // a character outside the BMP is two UTF-16 units in the JS, still one run
    }
    let t: String = out.trim_matches(|c: char| c == ' ').chars().take(80).collect();
    if t.is_empty() { "part".into() } else { t }
}

/// partcraft.js `faceWords(name)`.
pub fn face_words(part: &Value, name: &str) -> String {
    // /^(f\d+)\.([a-z]+)(?::([^@~]+))?/
    let Some((fid, rest)) = name.split_once('.') else { return "?".into() };
    if !(fid.len() > 1 && fid.starts_with('f') && fid.chars().skip(1).all(|c| c.is_ascii_digit())) {
        return "?".into();
    }
    let kind: String = rest.chars().take_while(|c| c.is_ascii_lowercase()).collect();
    if kind.is_empty() {
        return "?".into();
    }
    let after = rest.get(kind.len()..).unwrap_or("");
    let sub: Option<String> =
        after.strip_prefix(':').map(|s| s.chars().take_while(|c| *c != '@' && *c != '~').collect()).filter(|s: &String| !s.is_empty());
    const KINDS: [&str; 17] = [
        "start", "end", "side", "wall", "floor", "cbore", "cfloor", "top", "blend", "corner", "bevel", "face", "draft", "csink", "far",
        "thread", "tend",
    ];
    let g = P::feature_by_id(part, fid);
    let kind_words = if KINDS.contains(&kind.as_str()) { tr(&format!("app.face.kind.{kind}")) } else { kind.clone() };
    let feat = g.map_or_else(|| tr("app.plane.deleted_feature"), |g| jv::s(g, "name").to_string());
    let mut out = trv("app.face.words", &[("feature", feat), ("kind", kind_words)]);
    if kind == "face"
        && let Some(s) = sub
    {
        let nums: Vec<String> = s.split('_').map(|x| cc_part::js::num_str(cc_cad::js::str_to_number(x) + 1.0)).collect();
        out.push(' ');
        out.push_str(&nums.join("."));
    }
    // /([@~])(f\d+)(?:#(\d+|m))?/g
    let chars: Vec<char> = name.chars().collect();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        i += 1;
        if c != '@' && c != '~' {
            continue;
        }
        if chars.get(i) != Some(&'f') {
            continue;
        }
        let mut j = i + 1;
        while chars.get(j).is_some_and(char::is_ascii_digit) {
            j += 1;
        }
        if j == i + 1 {
            continue;
        }
        let hid: String = chars.get(i..j).map(|s| s.iter().collect()).unwrap_or_default();
        let mut copy: Option<String> = None;
        if chars.get(j) == Some(&'#') {
            if chars.get(j + 1) == Some(&'m') {
                copy = Some("m".into());
                j += 2;
            } else {
                let mut k = j + 1;
                while chars.get(k).is_some_and(char::is_ascii_digit) {
                    k += 1;
                }
                if k > j + 1 {
                    copy = chars.get(j + 1..k).map(|s| s.iter().collect());
                    j = k;
                }
            }
        }
        i = j;
        let h = P::feature_by_id(part, &hid).map(|h| jv::s(h, "name").to_string());
        out.push_str(&if c == '~' {
            trv("app.face.inside_of", &[("name", h.unwrap_or_else(|| tr("app.face.a_shell")))])
        } else if copy.as_deref() == Some("m") {
            trv("app.face.mirrored_by", &[("name", h.unwrap_or_else(|| "?".into()))])
        } else {
            trv("app.face.copy_of", &[("n", copy.unwrap_or_else(|| "undefined".into())), ("name", h.unwrap_or_else(|| "?".into()))])
        });
    }
    out
}

/// Möller–Trumbore: the ray parameter where o + t·d meets triangle abc (both sides), if it does in front.
fn ray_tri(o: [f64; 3], d: [f64; 3], a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Option<f64> {
    let sub = |p: [f64; 3], q: [f64; 3]| [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
    let cross = |p: [f64; 3], q: [f64; 3]| [p[1] * q[2] - p[2] * q[1], p[2] * q[0] - p[0] * q[2], p[0] * q[1] - p[1] * q[0]];
    let dot = |p: [f64; 3], q: [f64; 3]| p[0] * q[0] + p[1] * q[1] + p[2] * q[2];
    let (e1, e2) = (sub(b, a), sub(c, a));
    let p = cross(d, e2);
    let det = dot(e1, p);
    if det.abs() < 1e-14 {
        return None;
    }
    let inv = 1.0 / det;
    let s = sub(o, a);
    let u = dot(s, p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = cross(s, e1);
    let v = dot(d, q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = dot(e2, q) * inv;
    (t > 0.0 && t.is_finite()).then_some(t)
}

/// Put a part view in the window (any other document closes).
pub fn install(app: &mut UiApp, mut pv: PartView) {
    app.doc = None;
    app.editor = None;
    app.source = None;
    app.model3 = None;
    app.mc = None;
    app.scene3 = None;
    app.pdf = None;
    app.civil = None;
    app.detail = None;
    app.error = None;
    let t = app.tokens;
    pv.tints = ([t.accent.r(), t.accent.g(), t.accent.b(), 255], [t.row_selected.r(), t.row_selected.g(), t.row_selected.b(), 255]);
    pv.background = BACKGROUND_REBUILDS.load(std::sync::atomic::Ordering::Relaxed);
    app.message = pv.message.clone();
    app.part = Some(pv);
}

/// Is this a file PartCraft opens? (`.partcraft.json`, a JSON naming `"format": "partcraft…"`, a `.pcscript`.)
pub fn is_part_file(lower: &str, bytes: &[u8]) -> bool {
    if lower.ends_with(".partcraft.json") || lower.ends_with(".pcscript") || lower.ends_with(".partcraft-asm.json") {
        return true;
    }
    if !lower.ends_with(".json") {
        return false;
    }
    let head = String::from_utf8_lossy(bytes.get(..bytes.len().min(400)).unwrap_or(&[])).to_string();
    let compact: String = head.chars().filter(|c| !c.is_whitespace()).collect();
    compact.starts_with('{') && (compact.contains("\"format\":\"partcraft\"") || compact.contains("\"format\":\"partcraft-assembly\""))
}

/// Open a part file dropped, picked or given to the CLI.
pub fn open_file(app: &mut UiApp, f: &ImportedFile) {
    match PartView::open(&f.name, &f.bytes) {
        Ok(pv) => install(app, pv),
        Err(e) => app.error = Some(e),
    }
}

fn now_ms() -> f64 {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
}

/// Write a file through `Services::save`; the status line says where it went.
fn save_out(app: &mut UiApp, name: &str, bytes: Vec<u8>, words: String) -> Result<bool, String> {
    let save = app.services.save.as_mut().ok_or("Saving is not available here")?;
    match save(name, bytes)? {
        Some(_) => {
            if let Some(pv) = app.part.as_mut() {
                pv.note = words;
            }
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Run `f` with the sketch editor taken out of the part view (the editor's host is the view).
fn with_ed<R>(pv: &mut PartView, f: impl FnOnce(&mut crate::partsketch::SketchEd, &mut PartView) -> R) -> R {
    let mut ed = std::mem::take(&mut pv.sk);
    let r = f(&mut ed, pv);
    pv.sk = ed;
    r
}

/// "x,y[,shift|ctrl]" → (x, y, modifiers).
fn xy_mods(s: &str) -> Result<(f64, f64, crate::partsketch::Mods), String> {
    let parts: Vec<&str> = s.split(',').map(str::trim).collect();
    let num = |i: usize| parts.get(i).and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite());
    let (Some(x), Some(y)) = (num(0), num(1)) else { return Err(format!("\"{s}\" is not x,y (stage points)")) };
    let mut m = crate::partsketch::Mods::default();
    for p in parts.iter().skip(2) {
        match *p {
            "shift" => m.shift = true,
            "ctrl" => m.ctrl = true,
            _ => {}
        }
    }
    Ok((x, y, m))
}

/// partcraft.js `numericEntry()`'s add: "line:0,0;10,0" (point, line, centerline, rect, circle).
fn numeric_entry(pv: &mut PartView, spec: &str) -> Result<(), String> {
    let (kind, rest) = spec.split_once(':').ok_or_else(|| format!("part.sk.num: \"{spec}\" is not kind:values"))?;
    let fields: Vec<&str> = rest.split(';').collect();
    // /^\s*(-?[\d.]+(?:e-?\d+)?)\s*[, ]\s*(-?[\d.]+(?:e-?\d+)?)\s*$/i
    let num_tok = |t: &str| -> Option<f64> {
        let b = t.as_bytes();
        let mut i = usize::from(b.first() == Some(&b'-'));
        let s0 = i;
        while b.get(i).is_some_and(|c| c.is_ascii_digit() || *c == b'.') {
            i += 1;
        }
        if i == s0 {
            return None;
        }
        if matches!(b.get(i), Some(b'e' | b'E')) {
            i += 1;
            if b.get(i) == Some(&b'-') {
                i += 1;
            }
            let s1 = i;
            while b.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
            if i == s1 {
                return None;
            }
        }
        (i == b.len()).then(|| cc_part::js::str_num(t))
    };
    let pair = |s: &str| -> Option<[f64; 2]> {
        let t = cc_part::js::trim(s);
        let cut = t.find([',', ' '])?;
        let (a, b) = (t.get(..cut)?, t.get(cut + 1..)?);
        let (a, b) = (cc_part::js::trim(a), cc_part::js::trim(b));
        let (x, y) = (num_tok(a)?, num_tok(b)?);
        (x.is_finite() && y.is_finite()).then_some([x, y])
    };
    let get = |i: usize| fields.get(i).copied().unwrap_or("");
    let bad = || tr("app.num.bad_input");
    let (undo, added) = match kind {
        "point" | "line" | "centerline" | "rect" | "circle" => (format!("app.num.undo_{kind}"), format!("app.num.added_{kind}")),
        other => return Err(format!("part.sk.num: point, line, centerline, rect or circle, not {other}")),
    };
    let r = match kind {
        "point" => {
            let Some(a) = pair(get(0)) else {
                pv.note = bad();
                return Ok(());
            };
            pv.sketch_commit(&tr(&undo), false, |sk| {
                cc_part::sketch::add_point(sk, a[0], a[1], false);
                Ok(())
            })
        }
        "line" | "centerline" | "rect" => {
            let (Some(a), Some(b)) = (pair(get(0)), pair(get(1))) else {
                pv.note = bad();
                return Ok(());
            };
            let k = kind.to_string();
            pv.sketch_commit(&tr(&undo), false, |sk| {
                if k == "rect" {
                    cc_part::sketch::add_rect(sk, a, b, false)?;
                } else {
                    cc_part::sketch::add_line(sk, &cc_part::sketch::At::Xy(a), &cc_part::sketch::At::Xy(b), k == "centerline")?;
                }
                Ok(())
            })
        }
        _ => {
            let c = pair(get(0));
            let r = cc_part::js::str_num(cc_part::js::trim(get(1)));
            let (Some(c), true) = (c, r > 0.0) else {
                pv.note = bad();
                return Ok(());
            };
            pv.sketch_commit(&tr(&undo), false, |sk| {
                cc_part::sketch::add_circle(sk, &cc_part::sketch::At::Xy(c), r, false)?;
                Ok(())
            })
        }
    };
    pv.note = match r {
        Ok(()) => tr(&added),
        Err(e) => e,
    };
    Ok(())
}

/// The `part.sk.*` commands: the sketch editor's tools and gestures (stage points), its panel's buttons.
fn sketch_cmd(pv: &mut PartView, id: &str, name: &str) -> Result<(), String> {
    if pv.mode != Mode::Sketch {
        return Err(format!("{id}: open a sketch first"));
    }
    match id {
        "part.sk.tool" => {
            if !crate::partsketch::TOOLS.iter().any(|(t, _)| *t == name) {
                return Err(format!("part.sk.tool: no tool {name}"));
            }
            let tool = if pv.sk.tool == name { "select".to_string() } else { name.to_string() };
            with_ed(pv, |ed, pv| ed.set_tool(pv, &tool));
        }
        "part.sk.view" => {
            if !name.is_empty() {
                let v: Vec<f64> = name.split(',').filter_map(|x| x.trim().parse::<f64>().ok()).filter(|x| x.is_finite()).collect();
                let (Some(&cx), Some(&cy), Some(&s)) = (v.first(), v.get(1), v.get(2)) else {
                    return Err(format!("part.sk.view: \"{name}\" is not cx,cy,scale[,w,h]"));
                };
                if !(1e-4..=1e5).contains(&s) {
                    return Err("part.sk.view: the scale is 0.0001 to 100000".into());
                }
                pv.sk.view = (cx, cy, s);
                if let (Some(&w), Some(&h)) = (v.get(3), v.get(4)) {
                    if !(10.0..=20000.0).contains(&w) || !(10.0..=20000.0).contains(&h) {
                        return Err("part.sk.view: the stage is 10 to 20000 points".into());
                    }
                    pv.sk.size_pin = Some((w, h));
                    pv.sk.size = (w, h);
                }
            }
            let (cx, cy, s) = pv.sk.view;
            let (w, h) = pv.sk.size;
            pv.note = format!("sketch view {cx},{cy},{s} on {w}x{h}");
        }
        "part.sk.click" => {
            let (x, y, m) = xy_mods(name)?;
            with_ed(pv, |ed, pv| {
                ed.move_to(pv, x, y);
                ed.down(pv, x, y, 0, m);
                ed.up(pv, x, y, 0, m);
            });
        }
        "part.sk.move" => {
            let (x, y, _) = xy_mods(name)?;
            with_ed(pv, |ed, pv| ed.move_to(pv, x, y));
        }
        "part.sk.right" => {
            let (x, y, m) = xy_mods(name)?;
            with_ed(pv, |ed, pv| {
                ed.move_to(pv, x, y);
                ed.down(pv, x, y, 2, m);
                ed.up(pv, x, y, 2, m);
            });
        }
        "part.sk.dbl" => {
            let (x, y, m) = xy_mods(name)?;
            with_ed(pv, |ed, pv| {
                ed.move_to(pv, x, y);
                ed.down(pv, x, y, 0, m);
                ed.up(pv, x, y, 0, m);
                ed.dbl(pv, x, y);
            });
        }
        "part.sk.drag" => {
            // "x1,y1:x2,y2[:steps][:shift]": down, `steps` moves (default 8), up
            let parts: Vec<&str> = name.split(':').collect();
            let (x1, y1, m) = xy_mods(parts.first().copied().unwrap_or(""))?;
            let (x2, y2, _) = xy_mods(parts.get(1).copied().unwrap_or(""))?;
            let steps = parts.get(2).and_then(|s| s.trim().parse::<usize>().ok()).unwrap_or(8).clamp(1, 1000);
            with_ed(pv, |ed, pv| {
                ed.move_to(pv, x1, y1);
                ed.down(pv, x1, y1, 0, m);
                for i in 1..=steps {
                    let k = cc_part::js::f(i) / cc_part::js::f(steps);
                    ed.move_to(pv, x1 + (x2 - x1) * k, y1 + (y2 - y1) * k);
                }
                ed.up(pv, x2, y2, 0, m);
            });
        }
        "part.sk.key" => {
            let (k, shift) = match name.strip_suffix("+shift") {
                Some(k) => (k, true),
                None => (name, false),
            };
            sketch_key(pv, k, shift);
        }
        "part.sk.value" | "part.sk.enter" => {
            if pv.sk.ask.is_none() {
                return Ok(());
            }
            if id == "part.sk.value"
                && let Some(a) = pv.sk.ask.as_mut()
            {
                a.text = name.to_string();
            }
            with_ed(pv, |ed, pv| ed.input_done(pv, true));
        }
        "part.sk.rel" => {
            // the panel's relation / dimension buttons (sketchview.js addRelation on the selected geometry)
            let sk = pv.sketch();
            let ents: Vec<String> = pv.sk.sel.iter().filter(|i| cc_part::sketch::by_id(sk, i).is_some()).cloned().collect();
            let dim = cc_part::sketch::DIMENSIONS.contains(&name);
            let ty = name.to_string();
            with_ed(pv, |ed, pv| {
                ed.add_relation(pv, &ty, &ents);
                if dim {
                    let last = cc_part::sketch::constraints(pv.sketch())
                        .last()
                        .filter(|c| jv::s(c, "type") == ty)
                        .map(|c| jv::s(c, "id").to_string());
                    if let Some(cid) = last {
                        ed.edit_dimension(pv, &cid, false);
                    }
                }
            });
        }
        "part.sk.select" => {
            pv.sk.sel = name.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect();
        }
        "part.sk.all" => {
            with_ed(pv, |ed, pv| ed.set_tool(pv, "select"));
            let ids: Vec<String> = cc_part::sketch::entities(pv.sketch()).iter().map(|e| jv::s(e, "id").to_string()).collect();
            pv.note = trv("app.sketch.selected_n", &[("n", ids.len().to_string())]);
            pv.sk.sel = ids;
        }
        "part.sk.none" => {
            with_ed(pv, |ed, pv| ed.set_tool(pv, "select"));
            pv.sk.sel.clear();
        }
        "part.sk.construction" => with_ed(pv, |ed, pv| ed.toggle_construction(pv)),
        "part.sk.unrel" => {
            let cid = name.to_string();
            let r = pv.sketch_commit(&tr("app.undo.delete_relation"), false, |sk| {
                cc_part::sketch::remove_constraints(sk, &[cid]);
                Ok(())
            });
            if let Err(e) = r {
                pv.note = e;
            }
        }
        "part.sk.edit" => with_ed(pv, |ed, pv| ed.edit_dimension(pv, name, false)),
        "part.sk.num" => numeric_entry(pv, name)?,
        "part.sk.fit" => {
            let sk = pv.sketch().clone();
            pv.sk.fit(&sk);
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

/// A key in sketch mode (partcraft.js document keydown → `editor.key(e)`, then Esc exits, F fits).
pub fn sketch_key(pv: &mut PartView, k: &str, shift: bool) {
    let used = with_ed(pv, |ed, pv| ed.key(pv, k, shift));
    if used {
        return;
    }
    if k == "Escape" {
        pv.exit_sketch();
    } else if k.eq_ignore_ascii_case("f") && !shift {
        let sk = pv.sketch().clone();
        pv.sk.fit(&sk);
    }
}

/// The `part.*` commands.
pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    let t0 = web_time::Instant::now();
    let r = run_cmd(app, id, params);
    if let Some(pv) = app.part.as_mut()
        && id != "part.times"
    {
        pv.cmd_ms = (pv.cmd_ms.0 + ms_since(t0), pv.cmd_ms.1 + 1);
    }
    r
}

fn run_cmd(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    let name = params.name.clone().unwrap_or_default();
    if id == "part.sample" {
        install(app, PartView::sample()?);
        return Ok(());
    }
    if id == "part.new" {
        install(app, PartView::new_part());
        return Ok(());
    }
    let pv = app.part.as_mut().ok_or_else(|| format!("{id}: open a part first"))?;
    match id {
        "part.save" => {
            let (file, bytes) = pv.save_bytes();
            let words = trv("app.file.saved", &[("file", file.clone())]);
            if save_out(app, &file, bytes, words)?
                && let Some(pv) = app.part.as_mut()
            {
                pv.dirty = false;
            }
        }
        "part.export" => {
            let (kind, at) = match name.split_once('@') {
                Some((k, ms)) => (k.to_string(), ms.trim().parse::<f64>().ok().filter(|x| x.is_finite())),
                None => (name.clone(), None),
            };
            let (file, bytes, words) = pv.export(&kind, at.unwrap_or_else(now_ms))?;
            save_out(app, &file, bytes, words)?;
        }
        "part.param" => {
            let words = pv.set_param(&name)?;
            if !words.is_empty() {
                pv.note = words;
            }
        }
        "part.undo" => pv.undo_redo(true),
        "part.redo" => pv.undo_redo(false),
        "part.rollto" => {
            let n = if name == "end" {
                P::features(&pv.part).len()
            } else {
                let fid = pv.feature_id(&name).ok_or_else(|| format!("part.rollto: no feature {name}"))?;
                P::index_of(&pv.part, &fid).unwrap_or(0)
            };
            pv.roll_to(n)?;
        }
        "part.suppress" => {
            let fid = pv.feature_id(&name).ok_or_else(|| format!("part.suppress: no feature {name}"))?;
            pv.toggle_suppress(&fid)?;
        }
        "part.select" => {
            let fid =
                if name.is_empty() { None } else { Some(pv.feature_id(&name).ok_or_else(|| format!("part.select: no feature {name}"))?) };
            pv.select_feature(fid);
        }
        "part.face" | "part.edge" => {
            let (shift, key) = match name.strip_prefix('+') {
                Some(k) => (true, k.to_string()),
                None => (false, name.clone()),
            };
            let face = id == "part.face";
            if key.is_empty() {
                pv.pick_none();
            } else {
                let i = if face {
                    pv.faces().iter().position(|f| jv::is_str(f.get("name"), &key)).ok_or_else(|| format!("part.face: no face {key}"))?
                } else if let Some(n) = key.strip_prefix('#') {
                    n.parse::<usize>().ok().filter(|&i| i < pv.edges().len()).ok_or_else(|| format!("part.edge: no edge {key}"))?
                } else {
                    pv.edges().iter().position(|e| jv::is_str(e.get("key"), &key)).ok_or_else(|| format!("part.edge: no edge {key}"))?
                };
                pv.pick_geometry(face, i, shift);
            }
            pv.tab = "props";
        }
        "part.plane" => {
            if !matches!(name.as_str(), "front" | "top" | "right") {
                return Err(format!("part.plane: front, top or right, not {name}"));
            }
            pv.select_plane(&name);
        }
        "part.sketch" => {
            if name.is_empty() {
                pv.start_sketch();
            } else {
                let fid = pv.feature_id(&name).ok_or_else(|| format!("part.sketch: no feature {name}"))?;
                pv.edit_sketch(&fid, false);
            }
        }
        "part.sketch.exit" => pv.exit_sketch(),
        "part.feature" => pv.feature_cmd(&name)?,
        "part.set" => pv.set_field(&name)?,
        "part.dim" => pv.set_dimension(&name)?,
        "part.delete" => {
            if pv.mode == Mode::Sketch {
                with_ed(pv, |ed, pv| ed.delete_selection(pv));
            } else {
                pv.delete_selected(name == "yes");
            }
        }
        "part.move" => match name.as_str() {
            "up" => pv.move_selected(-1.0),
            "down" => pv.move_selected(1.0),
            other => return Err(format!("part.move: up or down, not {other}")),
        },
        "part.pick" => {
            let fid = if name.is_empty() { pv.sel_feature.clone() } else { pv.feature_id(&name) };
            let fid = fid.ok_or("part.pick: select a fillet, chamfer or shell first")?;
            pv.start_pick(&fid);
        }
        "part.pick.done" => pv.end_pick(),
        "part.config" => {
            if name == "export" {
                match pv.configs_csv() {
                    Ok((file, bytes, words)) => {
                        save_out(app, &file, bytes, words)?;
                    }
                    Err(e) => pv.note = e,
                }
            } else {
                pv.config_cmd(&name)?;
            }
        }
        "part.times" => {
            pv.note =
                format!("{} commands {:.1} ms; {} rebuilds {:.1} ms", pv.cmd_ms.1, pv.cmd_ms.0, pv.rebuild_total.1, pv.rebuild_total.0);
        }
        id if id.starts_with("part.sk.") => sketch_cmd(pv, id, &name)?,
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn the_sample_builds_with_named_faces() {
        let pv = PartView::sample().unwrap();
        assert_eq!(pv.failing(), 0, "{:?}", pv.regen.results.iter().map(|(k, v)| (k.to_string(), v.clone())).collect::<Vec<_>>());
        let st = pv.regen.stats.clone().unwrap();
        assert_eq!(jv::n(&st, "genus"), 7.0, "six bolt holes and a slot");
        assert!(pv.faces().len() > 15);
        assert!(pv.scene.instances.len() > pv.faces().len(), "one mesh per face plus the edges");
        assert_eq!(pv.file_base(), "Bracket");
        assert!(pv.note.starts_with("Opened partcraft-bracket.pcscript: 9 features"), "{}", pv.note);
        assert!(pv.build_words().starts_with("Rebuilt in"));
    }

    #[test]
    fn face_words_as_partcraft_says_them() {
        let pv = PartView::sample().unwrap();
        assert_eq!(pv.face_words("f2.end"), "Body end face");
        assert_eq!(pv.face_words("f6.wall:h1@f7#2"), "Bolt hole wall (copy 2 of Bolts)");
        assert_eq!(pv.face_words("nope"), "?");
        assert_eq!(file_base(&serde_json::json!({"name": "Bad/name: ünï"})), "Bad_name_ _n_");
    }

    #[test]
    fn parameters_undo_redo_rollback_and_suppress() {
        let mut pv = PartView::sample().unwrap();
        let vol0 = jv::n(pv.regen.stats.as_ref().unwrap(), "volume");
        pv.set_param("length=150").unwrap();
        let vol1 = jv::n(pv.regen.stats.as_ref().unwrap(), "volume");
        assert!(vol1 > vol0, "longer base: {vol0} → {vol1}");
        assert!(pv.can_undo());
        pv.undo_redo(true);
        assert_eq!(jv::n(pv.regen.stats.as_ref().unwrap(), "volume"), vol0);
        pv.undo_redo(false);
        assert_eq!(jv::n(pv.regen.stats.as_ref().unwrap(), "volume"), vol1);
        // a linked number solves its parameter backward
        let words = pv.set_param("f2.depth=70").unwrap();
        assert!(words.contains("width"), "{words}");
        assert_eq!(PP::params_of(&pv.part).var("width").unwrap().expr, "70");
        // a plain number, a new parameter, a link
        pv.set_param("Inner.r=6").unwrap();
        pv.set_param("fr=5").unwrap();
        pv.set_param("Inner.r==fr").unwrap();
        assert_eq!(PP::params_of(&pv.part).link("f3.r").unwrap().expr, "fr");
        assert!(pv.set_param("thick=-1").is_ok(), "a parameter takes any value; the features say what fails");
        pv.undo_redo(true);
        assert!(pv.set_param("nonsense").is_err());
        // roll back to the fillet: only Profile + Body built
        pv.roll_to(2).unwrap();
        assert_eq!(jv::n(pv.regen.stats.as_ref().unwrap(), "genus"), 0.0);
        assert!(pv.regen.results.get("f3").is_some_and(|r| jv::is_str(r.get("status"), "rolled")));
        pv.roll_to(99).unwrap();
        assert!(pv.part.get("rollback").is_some_and(Value::is_null));
        pv.toggle_suppress("f9").unwrap();
        assert_eq!(jv::n(pv.regen.stats.as_ref().unwrap(), "genus"), 6.0, "the slot is off");
        assert!(pv.dirty);
    }

    #[test]
    fn opening_files_and_hostile_bytes() {
        let pv = PartView::sample().unwrap();
        let (name, bytes) = pv.save_bytes();
        assert_eq!(name, "Bracket.partcraft.json");
        assert!(is_part_file("bracket.partcraft.json", &bytes));
        assert!(is_part_file("x.json", &bytes));
        assert!(!is_part_file("x.json", b"{\"format\":\"civilcraft-project\"}"));
        let back = PartView::open(&name, &bytes).unwrap();
        assert_eq!(P::to_json(&back.part), P::to_json(&pv.part), "saved and reopened: the same part");
        for junk in [&b""[..], b"{", b"[]", b"{\"format\":\"partcraft\",\"version\":99}", b"\xff\xfe\x00", b"pcscript 1\nnonsense here\n"] {
            assert!(PartView::open("x.partcraft.json", junk).is_err());
        }
        assert!(PartView::open("a.partcraft-asm.json", b"{\"format\":\"partcraft-assembly\"}").is_err());
    }

    #[test]
    fn commands_save_and_export_through_services() {
        type Saved = Arc<Mutex<Vec<(String, Vec<u8>)>>>;
        let saved: Saved = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&saved);
        let mut app = UiApp::new(crate::Services {
            save: Some(Box::new(move |n, b| {
                sink.lock().unwrap().push((n.to_string(), b));
                Ok(Some(n.to_string()))
            })),
            ..Default::default()
        });
        assert!(app.run("part.save", &Params::default()).is_err(), "nothing open");
        app.run("part.sample", &Params::default()).unwrap();
        app.run("part.param", &Params { name: Some("thick=12".into()), ..Default::default() }).unwrap();
        for k in ["stl", "3mf", "step@0", "step214@0", "ifc@0"] {
            app.run("part.export", &Params { name: Some(k.into()), ..Default::default() }).unwrap();
        }
        assert!(app.run("part.export", &Params { name: Some("obj".into()), ..Default::default() }).is_err());
        app.run("part.save", &Params::default()).unwrap();
        let s = saved.lock().unwrap();
        let names: Vec<&str> = s.iter().map(|x| x.0.as_str()).collect();
        assert_eq!(names, ["Bracket.stl", "Bracket.3mf", "Bracket.step", "Bracket.step", "Bracket.ifc", "Bracket.partcraft.json"]);
        assert!(s[2].1.starts_with(b"ISO-10303-21;"));
        assert!(String::from_utf8_lossy(&s[2].1).contains("1970-01-01T00:00:00"));
        assert!(!app.part.as_ref().unwrap().dirty);
        app.run("part.rollto", &Params { name: Some("Inner".into()), ..Default::default() }).unwrap();
        app.run("part.undo", &Params::default()).unwrap();
        app.run("part.suppress", &Params { name: Some("Slot".into()), ..Default::default() }).unwrap();
        app.run("part.face", &Params { name: Some("f2.end".into()), ..Default::default() }).unwrap();
        assert!(app.run("part.face", &Params { name: Some("f99.end".into()), ..Default::default() }).is_err());
    }

    fn run(app: &mut UiApp, id: &str, name: &str) {
        app.run(id, &Params { name: Some(name.into()), ..Default::default() }).unwrap_or_else(|e| panic!("{id}={name}: {e}"));
    }

    #[test]
    fn sketch_a_rectangle_and_extrude_it() {
        let mut app = UiApp::default();
        run(&mut app, "part.new", "");
        run(&mut app, "part.sketch", "");
        assert!(app.part.as_ref().unwrap().pending_sketch);
        run(&mut app, "part.plane", "front");
        let pv = app.part.as_ref().unwrap();
        assert_eq!(pv.mode, Mode::Sketch);
        assert_eq!(pv.sk.tool, "line");
        run(&mut app, "part.sk.view", "0,0,4");
        run(&mut app, "part.sk.tool", "rect");
        // stage 800 × 600, scale 4: (400, 300) is the origin
        run(&mut app, "part.sk.click", "300,350");
        run(&mut app, "part.sk.move", "520,200");
        run(&mut app, "part.sk.click", "520,200");
        let pv = app.part.as_ref().unwrap();
        let sk = pv.sketch();
        assert_eq!(cc_part::sketch::entities(sk).iter().filter(|e| jv::s(e, "type") == "line").count(), 4);
        assert_eq!(pv.last_solve.as_ref().unwrap().dof, 4);
        // a dimension on the bottom line: click it, click where the label goes, type the value
        run(&mut app, "part.sk.tool", "dimension");
        run(&mut app, "part.sk.click", "400,350");
        run(&mut app, "part.sk.click", "400,380");
        assert!(app.part.as_ref().unwrap().sk.ask.is_some());
        run(&mut app, "part.sk.value", "60");
        let pv = app.part.as_ref().unwrap();
        assert_eq!(pv.last_solve.as_ref().unwrap().dof, 3, "{:?}", pv.note);
        run(&mut app, "part.sketch.exit", "");
        run(&mut app, "part.feature", "extrude");
        let pv = app.part.as_ref().unwrap();
        assert_eq!(pv.failing(), 0, "{}", pv.note);
        let vol = jv::n(pv.regen.stats.as_ref().unwrap(), "volume");
        assert!((vol - 60.0 * 37.5 * 10.0).abs() < 1e-6, "volume {vol}");
        assert!(pv.note.starts_with("Extrude1 added"), "{}", pv.note);
        // depth, rename, move, delete with its sketch kept, undo all the way back
        run(&mut app, "part.set", "Extrude1:depth=25");
        run(&mut app, "part.set", "Extrude1:name=Base");
        assert_eq!(P::features(&app.part.as_ref().unwrap().part)[1]["name"], "Base");
        run(&mut app, "part.select", "Sketch1");
        run(&mut app, "part.delete", "");
        assert!(app.part.as_ref().unwrap().confirm.is_some(), "the extrude uses the sketch");
        run(&mut app, "part.delete", "yes");
        assert!(P::features(&app.part.as_ref().unwrap().part).is_empty());
        for _ in 0..12 {
            run(&mut app, "part.undo", "");
        }
        assert!(P::features(&app.part.as_ref().unwrap().part).is_empty());
        assert!(app.part.as_ref().unwrap().note.contains("nothing") || app.part.as_ref().unwrap().note.contains("Nothing"));
    }

    #[test]
    fn fillet_picks_edges_and_shell_faces_on_the_mesh() {
        let mut app = UiApp::default();
        run(&mut app, "part.sample", "");
        let pv = app.part.as_ref().unwrap();
        let e = pv.edges().iter().position(|e| jv::s(e, "type") == "line").unwrap();
        run(&mut app, "part.edge", &format!("#{e}"));
        run(&mut app, "part.feature", "chamfer");
        let pv = app.part.as_ref().unwrap();
        assert_eq!(pv.mode, Mode::Model, "{}", pv.note);
        let f = P::features(&pv.part).last().unwrap().clone();
        assert_eq!(jv::arr(f.get("edges")).len(), 1);
        // a fillet with nothing picked opens picking
        run(&mut app, "part.feature", "fillet");
        assert_eq!(app.part.as_ref().unwrap().mode, Mode::Pick);
        let pv = app.part.as_ref().unwrap();
        let e = pv.edges().iter().position(|e| jv::s(e, "type") == "line").unwrap();
        run(&mut app, "part.edge", &format!("#{e}"));
        let pv = app.part.as_ref().unwrap();
        assert!(pv.note.contains('1'), "{}", pv.note);
        run(&mut app, "part.pick.done", "");
        assert_eq!(app.part.as_ref().unwrap().mode, Mode::Model);
    }

    #[test]
    fn big_parts_rebuild_on_a_thread_and_the_newest_part_wins() {
        let mut pv = PartView::sample().unwrap();
        let vol0 = jv::n(pv.regen.stats.as_ref().unwrap(), "volume");
        pv.background = true;
        pv.regen_ms = BACKGROUND_MS + 1.0;
        pv.set_param("length=150").unwrap();
        assert!(pv.rebuilding(), "sent to the thread");
        assert_eq!(pv.message, tr("app.build.rebuilding"));
        // a second edit while the first is on the thread: the first result is dropped, the newest part is built
        pv.set_param("length=160").unwrap();
        let t0 = std::time::Instant::now();
        while pv.poll_rebuild() && t0.elapsed().as_secs() < 60 {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!pv.rebuilding());
        let vol = jv::n(pv.regen.stats.as_ref().unwrap(), "volume");
        let mut sync = PartView::new(pv.part.clone(), "x");
        sync.rebuild();
        assert_eq!(vol, jv::n(sync.regen.stats.as_ref().unwrap(), "volume"), "the thread built the newest part");
        assert!(vol > vol0);
    }

    #[test]
    fn a_ray_finds_the_face_in_front() {
        let mut pv = PartView::sample().unwrap();
        pv.view.cam.width = 800.0;
        pv.view.cam.height = 600.0;
        pv.standard("top");
        let f = pv.face_at(400.0, 300.0).expect("the middle of the top view hits the part");
        let name = jv::s(pv.face(f).unwrap(), "name").to_string();
        assert!(name.starts_with("f2.") || name.starts_with("f3") || name.starts_with("f4"), "{name}");
    }
}
