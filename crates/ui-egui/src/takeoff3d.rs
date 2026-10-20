//! TakeoffCraft's 3D check view, as takeoffcraft.js has it: the takeoff of the sheet on screen standing on the ground
//! (walls from runs with a height, slabs and plates from areas, posts for counts; cc-takeoff `build_scene`, at parity
//! with scene3d.js), or the sheets that have a level stacked as floors (`build_stack`); the note of what it shows, a
//! click on an object selects its mark (Shift adds or takes out) with 0.5's line of its measures, the GLB export
//! (cc-io-takeoff `scene_to_glb`, glb.js) and the Levels window. Drawn by the shared 3D viewport (orbit, pan, zoom);
//! the sheet's picture is not laid on the ground (a plain ground).

use cc_render::mesh3d::{Instance3, MeshData};
use cc_takeoff::i18n::{F, t, tf, tn};
use cc_takeoff::scene3d::{Level, Scene, View, build_scene, build_stack};
use egui::{Rect, RichText};
use serde_json::Value;

use crate::pdfview::PdfView;
use crate::view3d::{Scene3D, View3};

/// The 3D view: the takeoff scene, what the viewport draws (one mesh per object, in the scene's order), its camera,
/// stacked or not, the sheet it shows.
pub struct D3 {
    pub scene: Scene,
    pub view: Scene3D,
    pub cam: View3,
    pub stack: bool,
    pub sheet: String,
    /// each drawn mesh's mark id
    pub ids: Vec<String>,
    /// the takeoff's version and the selection the view was last made from
    pub ver: f64,
    pub sel_seen: Vec<String>,
}

impl std::fmt::Debug for D3 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("D3").field("sheet", &self.sheet).field("objects", &self.scene.objects.len()).finish()
    }
}

fn s(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(x)) => x.clone(),
        _ => String::new(),
    }
}

fn rgb(hex: &str) -> [u8; 4] {
    crate::pdfview::hex_color(hex, 1.0).map_or([150, 150, 150, 255], |c| [c.r(), c.g(), c.b(), 255])
}

impl PdfView {
    fn sheet_view(&self, key: &str) -> Option<View> {
        let (hash, idx) = key.split_once(':')?;
        let f = self.files().into_iter().find(|f| f.hash() == hash)?;
        let p = f.pages.get(idx.parse::<usize>().ok()?)?;
        Some(View { transform: Some(p.transform), width: p.width, height: p.height, step: None })
    }

    /// The scene of the sheet on screen, or the stack (the sheet on screen first, then every other sheet with a level).
    fn takeoff_scene(&self, stack: bool) -> Result<Scene, String> {
        let key = self.takeoff.cur_sheet.clone();
        if !stack {
            let v = self.sheet_view(&key).unwrap_or(View { transform: None, width: 0.0, height: 0.0, step: None });
            return build_scene(&self.takeoff.tk.state, &key, &v);
        }
        build_stack(&self.takeoff.tk.state, &self.stack_levels(), None)
    }

    /// stackLevels: the sheet on screen first, then every other open sheet that has a level, at its elevation.
    fn stack_levels(&self) -> Vec<Level> {
        let cur = self.takeoff.cur_sheet.clone();
        let mut keys = vec![cur.clone()];
        for f in self.files() {
            for i in 0..f.pages.len() {
                let k = cc_takeoff::model::sheet_key(f.hash(), i as f64);
                if k != cur && self.takeoff.tk.level_of(&k).is_some() {
                    keys.push(k);
                }
            }
        }
        keys.into_iter()
            .filter_map(|k| {
                let v = self.sheet_view(&k)?;
                let elev = self.takeoff.tk.level_of(&k).unwrap_or(0.0);
                Some(Level { key: k, elev_ft: elev, view: v })
            })
            .collect()
    }

    /// update3d: rebuild from the model (after an edit, a new sheet, a selection made elsewhere). A scene that cannot be
    /// built closes the view and says why.
    pub fn update3d(&mut self) {
        let Some(d) = self.d3.as_ref() else { return };
        let stack = d.stack;
        let same = d.sheet == self.takeoff.cur_sheet;
        match self.takeoff_scene(stack) {
            Ok(scene) => {
                let (view, ids) = drawn(&scene, &self.takeoff);
                let ver = self.takeoff.tk.version;
                let sel = self.takeoff.sel_ids();
                if let Some(d) = self.d3.as_mut() {
                    d.scene = scene;
                    d.view = view;
                    d.ids = ids;
                    d.ver = ver;
                    d.sel_seen = sel;
                    if !same {
                        d.sheet = self.takeoff.cur_sheet.clone();
                        d.cam.fit_pending = true;
                    }
                }
            }
            Err(e) => {
                self.d3 = None;
                self.takeoff.message = e;
            }
        }
    }

    /// open3d / close3d.
    pub fn toggle3d(&mut self, on: bool) {
        self.sync_takeoff();
        if !on {
            self.d3 = None;
            return;
        }
        if self.d3.is_some() {
            return;
        }
        if self.takeoff.tk.scale_of(&self.takeoff.cur_sheet).is_none() {
            self.takeoff.message = t("d3.err.noscale");
            return;
        }
        self.takeoff.end_paste(true);
        self.takeoff.cancel_draft();
        self.takeoff.ac = None;
        if self.takeoff.tool != "select" {
            self.takeoff.set_tool("select");
        }
        match self.takeoff_scene(false) {
            Ok(scene) => {
                let (view, ids) = drawn(&scene, &self.takeoff);
                let (ver, sel_seen) = (self.takeoff.tk.version, self.takeoff.sel_ids());
                self.d3 = Some(D3 {
                    scene,
                    view,
                    cam: View3::default(),
                    stack: false,
                    sheet: self.takeoff.cur_sheet.clone(),
                    ids,
                    ver,
                    sel_seen,
                });
            }
            Err(e) => self.takeoff.message = e,
        }
    }

    /// renderNote3d: what the view shows, then the scene's notes.
    pub fn note3d(&self) -> Vec<String> {
        let Some(d) = self.d3.as_ref() else { return Vec::new() };
        let tt = &d.scene.totals;
        let parts: Vec<String> =
            [("walls", tt.walls), ("slabs", tt.slabs), ("plates", tt.plates), ("runs", tt.runs), ("markers", tt.markers)]
                .iter()
                .filter(|(_, n)| *n > 0.0)
                .map(|(k, n)| tn(&format!("d3.part.{k}"), *n))
                .collect();
        let head = match &d.scene.levels {
            Some(lv) if d.stack => {
                let list = if parts.is_empty() { "—".to_string() } else { parts.join(", ") };
                #[allow(clippy::cast_precision_loss)]
                let fills = [("list", F::S(list)), ("n", F::N(lv.len() as f64))];
                tf("d3.note.stacked", &fills)
            }
            _ if !parts.is_empty() => tf("d3.note", &[("list", F::S(parts.join(", ")))]),
            _ => t("d3.note.none"),
        };
        std::iter::once(head).chain(d.scene.notes.iter().cloned()).collect()
    }

    /// pick3d: a click on an object selects its mark (Shift adds or takes out), with its measures in the status line.
    pub fn pick3d(&mut self, id: Option<&str>, shift: bool) {
        let tk = &mut self.takeoff;
        let m = id.and_then(|i| tk.tk.mark(i)).cloned();
        if shift && (tk.sel.is_some() || !tk.sel_more.is_empty()) {
            let Some(m) = m else { return };
            let mid = s(m.get("id"));
            if tk.is_sel(&mid) {
                if tk.sel.as_deref() == Some(mid.as_str()) {
                    let rest = tk.sel_more.clone();
                    tk.set_selection(&rest);
                } else {
                    tk.sel_more.retain(|x| *x != mid);
                }
            } else {
                tk.sel_more.push(mid);
            }
            #[allow(clippy::cast_precision_loss)]
            let n = tk.sel_ids().len() as f64;
            tk.message = tf("sel.several", &[("n", F::N(n)), ("mod", F::S("Ctrl".into()))]);
            return;
        }
        let Some(m) = m else {
            tk.set_sel(None);
            tk.message = t("sel.none");
            return;
        };
        let mid = s(m.get("id"));
        tk.set_sel(Some(mid.clone()));
        tk.item = Some(s(m.get("item")));
        let name = tk.tk.item(&s(m.get("item"))).map(|i| s(i.get("name"))).unwrap_or_default();
        let o = self.d3.as_ref().and_then(|d| d.scene.objects.iter().find(|x| x.id.as_str() == Some(mid.as_str())));
        let ms = o.map(|o| &o.measure);
        let num = |k: &str| ms.and_then(|m| m.get(k)).and_then(Value::as_f64);
        let ft_in = |v: f64| cc_takeoff::units::format_ft_in(v, 16.0);
        let kind = o.map_or("", |o| o.kind);
        let sf = match ms {
            Some(_) => {
                cc_takeoff::units::fixed(if kind == "wall" { num("wallSqft").unwrap_or(0.0) } else { num("netSqft").unwrap_or(0.0) }, 2)
            }
            None => String::new(),
        };
        let fills = [
            ("item", F::S(name)),
            ("mod", F::S("Ctrl".into())),
            ("len", F::S(if ms.is_some() { ft_in(num("lengthFt").unwrap_or(f64::NAN)) } else { String::new() })),
            ("h", F::S(num("heightFt").filter(|h| *h != 0.0).map(ft_in).unwrap_or_default())),
            ("sf", F::S(sf)),
            ("depth", F::S(num("depthFt").filter(|h| *h != 0.0).map(ft_in).unwrap_or_default())),
            ("cy", F::S(num("volumeCuft").filter(|h| *h != 0.0).map(|v| cc_takeoff::units::fixed(v / 27.0, 2)).unwrap_or_default())),
        ];
        let key = if matches!(kind, "wall" | "slab" | "plate" | "run") {
            format!("d3.pick.{kind}")
        } else {
            format!("d3.pick.{}", s(m.get("kind")))
        };
        self.takeoff.message = tf(&key, &fills);
        self.refresh3d_tints();
    }

    /// The selected marks drawn in the selection colour.
    fn refresh3d_tints(&mut self) {
        let sel: Vec<String> = self.takeoff.sel_ids();
        if let Some(d) = self.d3.as_mut() {
            d.sel_seen.clone_from(&sel);
            let ids = d.ids.clone();
            d.view.set_tints(&|i| ids.get(i).filter(|id| sel.contains(id)).map(|_| [30, 72, 200, 255]));
        }
    }

    /// exportGlb: what the view shows as a GLB (glTF binary), metres, Y up.
    pub fn export_glb(&mut self) -> Option<(String, Vec<u8>)> {
        let d = self.d3.as_ref()?;
        if !d.scene.objects.iter().any(|o| o.kind != "frame" && (!o.tris.is_empty() || o.kind == "run")) {
            self.takeoff.message = t("d3.glb.empty");
            return None;
        }
        let tname = s(self.takeoff.tk.state.get("name"));
        let base = crate::takeofftools::safe_name(if tname.is_empty() { "Takeoff" } else { &tname });
        let file = if d.stack {
            format!("{base}.glb")
        } else {
            format!("{base} - {}.glb", crate::takeofftools::safe_name(&self.takeoff.sheet_label(&d.sheet)))
        };
        let bytes = cc_io_takeoff::glb::scene_to_glb(&d.scene.to_json(), &tname);
        let n = d.scene.objects.iter().filter(|o| o.kind != "frame").count();
        #[allow(clippy::cast_precision_loss)]
        let fills = [("file", F::S(file.clone())), ("n", F::N(n as f64))];
        self.takeoff.message = tf("d3.glb.done", &fills);
        Some((file, bytes))
    }

    /// toggleStack.
    pub fn toggle_stack(&mut self, on: bool) {
        let n = self.stack_levels().len();
        if let Some(d) = self.d3.as_mut() {
            d.stack = on;
        } else {
            return;
        }
        self.update3d();
        #[allow(clippy::cast_precision_loss)]
        let fills = [("n", F::N(n as f64))];
        self.takeoff.message = if !on {
            t("d3.stack.off")
        } else if n > 1 {
            tf("d3.stack.on", &fills)
        } else {
            t("d3.stack.none")
        };
    }

    /// `takeoff.view3d`: on | off (no name: toggles) | pick:<mark id>[,shift] | glb | stack:on|off |
    /// levels:<file>:<page>=<length>;… (Levels: a blank length clears a sheet's level).
    pub fn takeoff_3d(&mut self, spec: &str) -> Result<(), String> {
        self.sync_takeoff();
        let (op, rest) = spec.split_once(':').unwrap_or((spec, ""));
        match op {
            "on" => self.toggle3d(true),
            "off" => self.toggle3d(false),
            "" => {
                let on = self.d3.is_none();
                self.toggle3d(on);
            }
            "pick" => {
                let shift = rest.ends_with(",shift");
                let id = rest.trim_end_matches(",shift");
                self.pick3d((!id.is_empty()).then_some(id), shift);
            }
            "glb" => {
                if let Some(f) = self.export_glb() {
                    self.outbox.push(f);
                }
            }
            "stack" => self.toggle_stack(rest != "off"),
            "levels" => {
                let mut map = serde_json::Map::new();
                for part in rest.split(';').filter(|p| !p.is_empty()) {
                    let (sheet, v) = part.split_once('=').unwrap_or((part, ""));
                    let (fid, page) = sheet.split_once(':').ok_or("takeoff.view3d: levels:<file>:<page>=<length>;…")?;
                    let f = self.files().into_iter().find(|f| f.id == fid).ok_or_else(|| format!("takeoff.view3d: no file {fid}"))?;
                    let key = cc_takeoff::model::sheet_key(f.hash(), page.parse::<f64>().map_err(|_| "takeoff.view3d: page number")?);
                    map.insert(key, level_value(v).map_err(|e| format!("{sheet}: {e}"))?);
                }
                self.takeoff.tk.set_levels(&map).map_err(|e| e.clone())?;
                self.update3d();
            }
            _ => return Err(format!("takeoff.view3d: unknown {op} (on, off, pick, glb, stack, levels)")),
        }
        Ok(())
    }

    /// The 3D view in place of the sheet, with its bar and note.
    pub(crate) fn show3d(&mut self, ui: &mut egui::Ui, rect: Rect, tok: &crate::theme::Tokens) {
        // the model changed (an edit, an undo) or the sheet did: build again; the selection changed: paint it
        let (ver, sheet, sel) = (self.takeoff.tk.version, self.takeoff.cur_sheet.clone(), self.takeoff.sel_ids());
        if self.d3.as_ref().is_some_and(|d| d.ver != ver || d.sheet != sheet) {
            self.update3d();
        } else if self.d3.as_ref().is_some_and(|d| d.sel_seen != sel) {
            self.refresh3d_tints();
        }
        let bar = Rect::from_min_size(rect.min, egui::vec2(rect.width(), 26.0));
        let area = Rect::from_min_max(egui::pos2(rect.min.x, bar.max.y), rect.max);
        let mut act: Option<&str> = None;
        let stack = self.d3.as_ref().is_some_and(|d| d.stack);
        ui.scope_builder(egui::UiBuilder::new().max_rect(bar), |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(t("d3.bar")).strong().color(tok.text_strong));
                if ui.small_button(t("d3.extents")).on_hover_text(t("d3.extents.title")).clicked() {
                    act = Some("extents");
                }
                if ui.selectable_label(stack, t("d3.stack")).on_hover_text(t("d3.stack.title")).clicked() {
                    act = Some(if stack { "stack:off" } else { "stack:on" });
                }
                if ui.small_button(t("d3.glb")).on_hover_text(t("d3.glb.title")).clicked() {
                    act = Some("glb");
                }
                if ui.small_button(t("d3.close")).on_hover_text(t("d3.close.title")).clicked() {
                    act = Some("off");
                }
            });
        });
        let note = self.note3d();
        let mut pick: Option<(Option<String>, bool)> = None;
        if let Some(d) = self.d3.as_mut() {
            let r = crate::view3d::show(ui, area, &d.view, &mut d.cam);
            if r.clicked()
                && let Some(p) = r.interact_pointer_pos()
            {
                let ppp = f64::from(ui.ctx().pixels_per_point());
                let (sx, sy) = (f64::from(p.x - area.min.x) * ppp, f64::from(p.y - area.min.y) * ppp);
                let hit = crate::view3d::pick(&d.view, &d.cam.cam, sx, sy);
                let id = hit.and_then(|h| d.ids.get(h.instance).cloned());
                pick = Some((id, ui.input(|i| i.modifiers.shift)));
            }
        }
        let painter = ui.painter_at(area);
        for (i, line) in note.iter().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            painter.text(
                area.left_bottom() + egui::vec2(8.0, -8.0 - 16.0 * (note.len() - 1 - i) as f32),
                egui::Align2::LEFT_BOTTOM,
                line,
                egui::FontId::proportional(12.0),
                egui::Color32::from_rgb(0x17, 0x20, 0x2b),
            );
        }
        if let Some((id, shift)) = pick {
            self.pick3d(id.as_deref(), shift);
        }
        match act {
            Some("extents") => {
                if let Some(d) = self.d3.as_mut() {
                    d.cam.fit_pending = true;
                }
            }
            Some(a) => {
                let _ = self.takeoff_3d(a);
            }
            None => {}
        }
    }
}

/// A Levels entry: blank = not stacked; "-" for below zero; 0 in any unit; else a length.
fn level_value(txt: &str) -> Result<Value, String> {
    let v = txt.trim();
    if v.is_empty() {
        return Ok(Value::Null);
    }
    let neg = v.starts_with('-') || v.starts_with('−');
    let body = v.trim_start_matches(['-', '−']);
    let zero = {
        let b = body.trim_end_matches(['\'', '"']).trim_end_matches("ft").trim_end_matches('m').trim();
        let (a, c) = b.split_once('.').unwrap_or((b, ""));
        !a.is_empty() && a.chars().all(|x| x == '0') && c.chars().all(|x| x == '0')
    };
    let ft = if zero { 0.0 } else { cc_takeoff::units::parse_length(body, "ft")? };
    Ok(serde_json::json!(if neg { -ft } else { ft }))
}

/// The box `lo`..`hi` takes in point `p`.
fn grow(lo: &mut [f64; 3], hi: &mut [f64; 3], p: &[f64; 3]) {
    for ((l, h), v) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
        *l = l.min(*v);
        *h = h.max(*v);
    }
}

/// The viewport's meshes: one per object with triangles, in the object's colour; the mark id of each.
fn drawn(scene: &Scene, tk: &crate::takeofftools::TakeoffMode) -> (Scene3D, Vec<String>) {
    let mut meshes = Vec::new();
    let mut inst = Vec::new();
    let mut ids = Vec::new();
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for o in &scene.objects {
        if o.tris.is_empty() && o.lines.is_empty() {
            continue;
        }
        let c = rgb(o.color.as_str().unwrap_or("#888888"));
        let n = o.tris.len() / 9;
        let mesh = MeshData::from_triangles(&o.tris, &vec![c; n], &o.lines);
        for p in o.tris.as_chunks::<3>().0.iter().chain(o.lines.as_chunks::<3>().0.iter()) {
            grow(&mut lo, &mut hi, p);
        }
        let sel = o.id.as_str().is_some_and(|id| tk.is_sel(id));
        inst.push(Instance3 {
            mesh: meshes.len(),
            m: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            tint: sel.then_some([30, 72, 200, 255]),
            hidden: false,
        });
        meshes.push(mesh);
        ids.push(o.id.as_str().unwrap_or("").to_string());
    }
    // the ground: the sheet's outline (or the stack's box) as a thin plate under everything
    let (gw, gh) = scene.ground;
    if gw > 0.0 && gh > 0.0 {
        // under everything (slabs go down from the sheet's level)
        let z = scene.ground_z.unwrap_or(0.0).min(if lo[2].is_finite() { lo[2] } else { 0.0 }) - 0.01;
        let tri = [0.0, 0.0, z, gw, 0.0, z, gw, gh, z, 0.0, 0.0, z, gw, gh, z, 0.0, gh, z];
        let edges = [0.0, 0.0, z, gw, 0.0, z, gw, 0.0, z, gw, gh, z, gw, gh, z, 0.0, gh, z, 0.0, gh, z, 0.0, 0.0, z];
        let mesh = MeshData::from_triangles(&tri, &[[245, 245, 240, 255], [245, 245, 240, 255]], &edges);
        inst.push(Instance3 {
            mesh: meshes.len(),
            m: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            tint: None,
            hidden: false,
        });
        meshes.push(mesh);
        ids.push(String::new());
        grow(&mut lo, &mut hi, &[0.0, 0.0, z]);
        grow(&mut lo, &mut hi, &[gw, gh, z]);
    }
    if !lo[0].is_finite() {
        lo = [0.0; 3];
        hi = [1.0; 3];
    }
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let id = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    (Scene3D::new(id, meshes, inst, lo, hi), ids)
}
