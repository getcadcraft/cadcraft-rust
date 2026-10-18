//! PartCraft in the app (M13): a part opened from the files 0.5 PartCraft opens — its own `.partcraft.json`
//! (`P.toJSON`) and the `.pcscript` text form — or the built-in sample, rebuilt by cc-part's mesh kernel
//! (`kernel::regenerate`, the same code at parity with kernel.js) and shown in the 3D view, one GPU mesh per named
//! face so a hovered or picked face is tinted and named as partcraft.js `faceWords` names it. The feature history,
//! the parameters, undo, the rollback bar, Save and the exports are partcraft.js's (`commit`, `rollTo`, `savePart`,
//! `exportMesh`, `exportIfc`); the panels are in partpanel.rs.
//!
//! Units are millimetres, Y up (the part's own frame); the 3D view is Z up, so points go through `z_up` exactly as
//! the STL / 3MF exports turn them (x, −z, y).

use cc_part::{export as X, i18n, jv, kernel, params, params_part as PP, part as P, pcscript};
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
        };
        pv.rebuild();
        pv.view.cam.fit(pv.scene.min, pv.scene.max, Some(ISO_DIR));
        pv.fit_pending = true;
        let n = P::features(&pv.part).len();
        let errs = pv.failing();
        pv.message = if errs > 0 {
            trv("app.file.opened_failing", &[("name", file.to_string()), ("n", n.to_string()), ("errs", errs.to_string())])
        } else {
            trv("app.file.opened", &[("name", file.to_string()), ("n", n.to_string())])
        };
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

    /// partcraft.js `rebuild` + `applyRegen`: the kernel rebuilds the part; the solved sketches go back into it (the
    /// JS solves them in place), the view gets the new solid.
    pub fn rebuild(&mut self) {
        let t0 = web_time::Instant::now();
        let r = kernel::regenerate(&self.part, None);
        self.regen_ms = ms_since(t0);
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
        self.build_scene();
        self.message = self.build_words();
    }

    /// The build chip's words (partcraft.js `applyRegen`).
    pub fn build_words(&self) -> String {
        let errs = self.failing();
        if errs > 0 {
            i18n::t("app.build.failing", &[("n", i18n::F::from(errs))])
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
        self.scene.set_tints(&|i| {
            let f = faces.get(i).copied().flatten()?;
            if Some(f) == s {
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
            self.message = tr(if undo { "app.undo.nothing_undo" } else { "app.undo.nothing_redo" });
            return;
        };
        let now = std::mem::replace(&mut self.part, snap);
        to.push((label.clone(), now));
        self.dirty = true;
        self.edits.clear();
        self.rebuild();
        let build = self.message.clone();
        self.message = format!("{} {build}", trv(if undo { "app.undo.undid" } else { "app.undo.redid" }, &[("what", label)]));
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
        let mut trial = PP::params_of(&self.part);
        op(&mut trial, &self.part)?;
        let mut applied: Option<params::Applied> = None;
        self.commit(label, |p| {
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
            self.message = words.join(" ");
        }
        Ok(())
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

    /// The status line.
    pub fn status(&self) -> String {
        let hover = self.hover.and_then(|i| self.face(i)).map(|f| self.face_words(jv::s(f, "name"))).filter(|s| s != "?");
        match hover {
            Some(h) => format!("{h} · {}", self.message),
            None => self.message.clone(),
        }
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
                pv.message = words;
            }
            Ok(true)
        }
        None => Ok(false),
    }
}

/// The `part.*` commands.
pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    let name = params.name.clone().unwrap_or_default();
    if id == "part.sample" {
        install(app, PartView::sample()?);
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
                pv.message = format!("{words} {}", pv.message);
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
            pv.sel_feature =
                if name.is_empty() { None } else { Some(pv.feature_id(&name).ok_or_else(|| format!("part.select: no feature {name}"))?) };
        }
        "part.face" => {
            pv.sel_face = if name.is_empty() {
                None
            } else {
                Some(pv.faces().iter().position(|f| jv::is_str(f.get("name"), &name)).ok_or_else(|| format!("part.face: no face {name}"))?)
            };
            pv.tab = "props";
            pv.retint();
        }
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
        assert!(pv.message.starts_with("Opened partcraft-bracket.pcscript: 9 features"), "{}", pv.message);
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
