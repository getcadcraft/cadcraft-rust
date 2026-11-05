//! TakeoffCraft's 3D check view, as takeoffcraft.js has it: the takeoff of the sheet on screen standing on the ground
//! (walls from runs with a height, slabs and plates from areas, posts for counts; cc-takeoff `build_scene`, at parity
//! with scene3d.js), or the sheets that have a level stacked as floors (`build_stack`); the note of what it shows, a
//! click on an object selects its mark (Shift adds or takes out) with 0.5's line of its measures, the GLB export
//! (cc-io-takeoff `scene_to_glb`, glb.js) and the Levels window. Drawn by the shared 3D viewport with ModelCraft's
//! camera (m3d camera.js: orbit, pan, zoom at the pointer; the standard views look at the takeoff, Zoom extents shows
//! the whole sheet; perspective or parallel), the sheet's picture laid on the ground (view3d.js setGround: the page
//! rendered once at ≤ 2048 px, inverted as the 2D page is in a dark theme; the Sheet box hides it) and the view saved as
//! a PNG (viewimage.js: at its size on screen or twice it).

use std::sync::{Arc, Mutex};

use cc_m3d_tools::camera::{Box3, Camera};
use cc_render::mesh3d::{Cam3, Instance3, MeshData, Style3};
use cc_takeoff::i18n::{F, t, tf, tn};
use cc_takeoff::scene3d::{Level, Scene, View, build_scene, build_stack};
use egui::Rect;
use serde_json::Value;

use crate::pdfview::PdfView;
use crate::view3d::{Scene3D, Snapshot};

/// The 3D view: the takeoff scene, what the viewport draws (one mesh per object, in the scene's order), its camera,
/// stacked or not, the sheet it shows.
pub struct D3 {
    pub scene: Scene,
    pub view: Scene3D,
    pub cam: Camera,
    /// a standard view to take once the view's size is known (setScene's 'iso')
    pub fit: Option<&'static str>,
    /// the Sheet box: the ground (with the sheet's picture) shown
    pub ground_on: bool,
    /// the sheet's picture on the ground: its sheet, the dark theme's inversion, the tile layers
    pub ground: Option<Ground>,
    /// the theme the ground was made for (dark)
    pub dark: bool,
    /// a PNG being made: the picture asked of the GPU and the file's name
    pub png: Option<(Arc<Mutex<Snapshot>>, String)>,
    /// the view's size (points), as the camera last had it
    pub wh: (f64, f64),
    /// the Levels window: each open sheet (key, title, the elevation typed) and the check's message
    pub levels: Option<LevelsDlg>,
    pub stack: bool,
    pub sheet: String,
    /// each drawn mesh's mark id
    pub ids: Vec<String>,
    /// the takeoff's version and the selection the view was last made from
    pub ver: f64,
    pub sel_seen: Vec<String>,
}

/// The sheet's picture on the ground: its sheet, made for a dark theme, the texture tiles (layer, RGBA).
pub type Ground = (String, bool, Arc<Vec<(u32, Vec<u8>)>>);
/// The Levels window: each sheet (key, title, the elevation typed) and the check's message.
pub type LevelsDlg = (Vec<(String, String, String)>, String);

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
                if !same && let Some(d) = self.d3.as_mut() {
                    d.ground = None;
                }
                let ground = self.d3.as_ref().and_then(|d| d.ground.clone()).map(|g| g.2);
                let (ground_on, dark) = self.d3.as_ref().map_or((true, false), |d| (d.ground_on, d.dark));
                let (view, ids) = drawn(&scene, &self.takeoff, ground.as_deref(), ground_on, dark);
                let ver = self.takeoff.tk.version;
                let sel = self.takeoff.sel_ids();
                let stacked = scene.ground_z.is_some();
                if let Some(d) = self.d3.as_mut() {
                    let keep = same && d.scene.ground_z.is_some() == stacked;
                    d.scene = scene;
                    d.view = view;
                    d.ids = ids;
                    d.ver = ver;
                    d.sel_seen = sel;
                    if !same {
                        d.sheet = self.takeoff.cur_sheet.clone();
                    }
                    if !keep {
                        d.fit = Some("iso");
                    }
                }
                if !same {
                    self.load_ground();
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
                let (view, ids) = drawn(&scene, &self.takeoff, None, true, false);
                let (ver, sel_seen) = (self.takeoff.tk.version, self.takeoff.sel_ids());
                self.d3 = Some(D3 {
                    scene,
                    view,
                    cam: Camera::new(),
                    fit: Some("iso"),
                    ground_on: true,
                    ground: None,
                    dark: false,
                    png: None,
                    wh: (800.0, 600.0),
                    levels: None,
                    stack: false,
                    sheet: self.takeoff.cur_sheet.clone(),
                    ids,
                    ver,
                    sel_seen,
                });
                self.load_ground();
            }
            Err(e) => self.takeoff.message = e,
        }
    }

    /// loadGround: the sheet on screen rendered once (≤ 2048 px, its annotations drawn, white paper), cut into the
    /// view's texture tiles and laid on the ground; a sheet that cannot be drawn keeps a plain ground (d3.err.ground).
    fn load_ground(&mut self) {
        let Some(dark) = self.d3.as_ref().map(|d| d.dark) else { return };
        let key = self.takeoff.cur_sheet.clone();
        let Some(p) = self.cur.pages.get(self.page) else { return };
        let sc = (2048.0 / p.width.max(p.height).max(1.0)).min(2.0);
        match cc_io_takeoff::pdfread::render_rgba(&self.cur.doc, self.page, sc, true) {
            Ok((mut px, w, h)) => {
                if dark {
                    // view3d.js paintGround: invert(.9) hue-rotate(180deg), as 229.5 − 0.8 c
                    for c in px.as_chunks_mut::<4>().0 {
                        for v in c.iter_mut().take(3) {
                            *v = (229.5 - 0.8 * f64::from(*v)).round().clamp(0.0, 255.0) as u8;
                        }
                    }
                }
                let tiles = Arc::new(crate::civildrape::tiles(w as u32, h as u32, &px));
                if let Some(d) = self.d3.as_mut() {
                    d.ground = Some((key, dark, tiles));
                }
                self.rebuild3d();
            }
            Err(_) => self.takeoff.message = t("d3.err.ground"),
        }
    }

    /// The viewport's meshes made again from the scene (the ground's picture or the Sheet box changed).
    fn rebuild3d(&mut self) {
        let Some(d) = self.d3.as_ref() else { return };
        let ground = d.ground.as_ref().map(|g| g.2.clone());
        let (view, ids) = drawn(&d.scene, &self.takeoff, ground.as_deref(), d.ground_on, d.dark);
        if let Some(d) = self.d3.as_mut() {
            d.view = view;
            d.ids = ids;
        }
    }

    /// The box the standard views look at (focusBox: the takeoff and a quarter round it, else the sheet) and the whole
    /// sheet's (bbox, at least 1 high).
    fn boxes3d(&self) -> Option<(Box3, Box3)> {
        let d = self.d3.as_ref()?;
        let b = &d.scene.bbox;
        let bbox = Box3 { min: b.min, max: [b.max[0], b.max[1], b.max[2].max(1.0)] };
        let focus = match &d.scene.marks_box {
            Some(m) => {
                let pad = ((m.max[0] - m.min[0]).hypot(m.max[1] - m.min[1]) * 0.25).max(4.0);
                Box3 { min: [m.min[0] - pad, m.min[1] - pad, 0.0], max: [m.max[0] + pad, m.max[1] + pad, m.max[2].max(1.0)] }
            }
            None => bbox,
        };
        Some((focus, bbox))
    }

    /// standard(name) / extents(): the camera on the takeoff from a side, or on the whole sheet.
    fn view3d_cam(&mut self, name: &str) {
        let Some((focus, bbox)) = self.boxes3d() else { return };
        if let Some(d) = self.d3.as_mut() {
            if name == "extents" {
                d.cam.fit(Some(&bbox), None);
            } else {
                d.cam.standard(name, Some(&focus));
            }
        }
    }

    /// exportPng: the view as a PNG at its size on screen (`scale` 1) or twice it; the file comes in a later frame.
    pub fn export_png(&mut self, scale: f64) {
        let tname = s(self.takeoff.tk.state.get("name"));
        let Some(d) = self.d3.as_mut() else { return };
        let base = crate::takeofftools::safe_name(if tname.is_empty() { "Takeoff" } else { &tname });
        let base = if d.stack {
            format!("{base} 3D")
        } else {
            format!("{base} - {} 3D", crate::takeofftools::safe_name(&self.takeoff.sheet_label(&d.sheet)))
        };
        let file = format!("{}{}.png", base.trim(), if scale > 1.0 { format!("@{scale}x") } else { String::new() });
        let cap = Snapshot::new(scale);
        d.view.snapshot = Some(cap.clone());
        d.png = Some((cap, file));
    }

    /// A PNG the GPU has read back: encoded and handed to the host's save, with 0.5's line.
    fn take_png(&mut self) {
        let Some(d) = self.d3.as_mut() else { return };
        let Some((cap, file)) = d.png.clone() else { return };
        let out = cap.lock().ok().and_then(|mut c| c.out.take());
        let Some(out) = out else { return };
        d.png = None;
        d.view.snapshot = None;
        let png = out.and_then(|(w, h, mut px)| {
            // the target stores colours sRGB-encoded and the view shows them decoded: the file gets what the screen shows
            let lut: Vec<u8> = (0..=255u8)
                .map(|v| {
                    let c = f64::from(v) / 255.0;
                    let l = if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
                    (l * 255.0).round().clamp(0.0, 255.0) as u8
                })
                .collect();
            for c in px.as_chunks_mut::<4>().0 {
                for v in c.iter_mut().take(3) {
                    *v = lut.get(usize::from(*v)).copied().unwrap_or(*v);
                }
            }
            let img = image::RgbaImage::from_raw(w, h, px).ok_or("the picture's size")?;
            let mut buf = std::io::Cursor::new(Vec::new());
            img.write_to(&mut buf, image::ImageFormat::Png).map_err(|e| e.to_string())?;
            Ok(buf.into_inner())
        });
        match png {
            Ok(bytes) => {
                self.takeoff.message = tf("d3.png.done", &[("file", F::S(file.clone()))]);
                self.outbox.push((file, bytes));
            }
            Err(e) => self.takeoff.message = tf("d3.png.failed", &[("reason", F::S(e))]),
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
        let bytes = cc_io_takeoff::glb::scene_glb(&d.scene, &tname);
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
            "view" => {
                if cc_m3d_tools::camera::view_dir(rest).is_none() && rest != "extents" {
                    return Err(format!("takeoff.view3d: view:{rest} (iso, top, front, back, left, right, bottom, extents)"));
                }
                if let Some(d) = self.d3.as_mut() {
                    d.fit = None;
                }
                self.view3d_cam(rest);
                if let Some(d) = self.d3.as_ref() {
                    let c = &d.cam;
                    let f = |v: [f64; 3]| format!("{:.6},{:.6},{:.6}", v[0], v[1], v[2]);
                    self.note = format!("eye {} target {} size {}x{} persp {}", f(c.eye), f(c.target), c.w, c.h, c.persp);
                }
            }
            "persp" => {
                if let Some(d) = self.d3.as_mut() {
                    d.cam.persp = rest != "off";
                }
            }
            "ground" => {
                if let Some(d) = self.d3.as_mut() {
                    d.ground_on = rest != "off";
                }
                self.rebuild3d();
            }
            "png" => self.export_png(if rest == "2" { 2.0 } else { 1.0 }),
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
            _ => {
                return Err(format!("takeoff.view3d: unknown {op} (on, off, pick, glb, png[:2], stack, levels, view:NAME, persp, ground)"));
            }
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
        // the theme: the ground is inverted in a dark one, as the 2D page is
        let dark = tok.sheet.r() < 128;
        if self.d3.as_ref().is_some_and(|d| d.dark != dark) {
            if let Some(d) = self.d3.as_mut() {
                d.dark = dark;
            }
            self.load_ground();
        }
        self.take_png();
        let bar = Rect::from_min_size(rect.min, egui::vec2(rect.width(), 28.0));
        ui.painter().rect_filled(bar, 0.0, tok.app_bar);
        let area = Rect::from_min_max(egui::pos2(rect.min.x, bar.max.y), rect.max);
        let mut act: Option<String> = None;
        let (stack, persp, ground_on) = self.d3.as_ref().map_or((false, true, true), |d| (d.stack, d.cam.persp, d.ground_on));
        ui.scope_builder(egui::UiBuilder::new().max_rect(bar), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for v in ["iso", "top", "front", "back", "left", "right", "bottom"] {
                    if ui.small_button(t(&format!("d3.view.{v}"))).on_hover_text(t(&format!("d3.view.{v}.title"))).clicked() {
                        act = Some(format!("view:{v}"));
                    }
                }
                if ui.selectable_label(persp, t("d3.persp")).on_hover_text(t("d3.persp.title")).clicked() {
                    act = Some(if persp { "persp:off" } else { "persp:on" }.into());
                }
                if ui.small_button(t("d3.extents")).on_hover_text(t("d3.extents.title")).clicked() {
                    act = Some("view:extents".into());
                }
                let mut g = ground_on;
                if ui.checkbox(&mut g, t("d3.ground")).changed() {
                    act = Some(if g { "ground:on" } else { "ground:off" }.into());
                }
                if ui.small_button(t("d3.levels")).on_hover_text(t("d3.levels.title")).clicked() {
                    act = Some("levels-dialog".into());
                }
                if ui.selectable_label(stack, t("d3.stack")).on_hover_text(t("d3.stack.title")).clicked() {
                    act = Some(if stack { "stack:off" } else { "stack:on" }.into());
                }
                if ui.small_button(t("d3.glb")).on_hover_text(t("d3.glb.title")).clicked() {
                    act = Some("glb".into());
                }
                let png = ui.small_button(t("d3.png")).on_hover_text(t("d3.png.title"));
                if png.clicked() {
                    act = Some(if ui.input(|i| i.modifiers.shift) { "png:2" } else { "png" }.into());
                }
                if ui.small_button(t("d3.close")).on_hover_text(t("d3.close.title")).clicked() {
                    act = Some("off".into());
                }
            });
        });
        let note = self.note3d();
        let mut pick: Option<(Option<String>, bool)> = None;
        let ppp = f64::from(ui.ctx().pixels_per_point());
        let (w, h) = (f64::from(area.width()), f64::from(area.height()));
        let (focus, _) =
            self.boxes3d().unwrap_or((Box3 { min: [0.0; 3], max: [10.0, 10.0, 1.0] }, Box3 { min: [0.0; 3], max: [10.0, 10.0, 1.0] }));
        let bg = {
            let (a, b) = (tok.sheet, tok.ink);
            let m = |x: u8, y: u8| (f32::from(x) * 0.9 + f32::from(y) * 0.1) / 255.0;
            [m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()), 1.0]
        };
        if let Some(d) = self.d3.as_mut() {
            d.cam.set_size(w, h);
            d.wh = (w, h);
            if let Some(v) = d.fit.take() {
                d.cam.standard(v, Some(&focus));
            }
            let resp = ui.allocate_rect(area, egui::Sense::click_and_drag());
            input3d(ui, &resp, area, &mut d.cam);
            let b = &d.scene.bbox;
            let radius = ((b.max[0] - b.min[0]).hypot(b.max[1] - b.min[1]).hypot(b.max[2] - b.min[2]) / 2.0).max(1.0);
            d.cam.clip(radius);
            let c = &d.cam;
            let c3 = Cam3 {
                eye: c.eye,
                target: c.target,
                fov: c.fov,
                persp: c.persp,
                width: w * ppp,
                height: h * ppp,
                near: c.near,
                far: c.far,
            };
            let style = Style3 { background: bg, textures: true, ..Style3::default() };
            d.view.snapshot = d.png.as_ref().map(|p| p.0.clone());
            crate::view3d::paint(ui, area, &d.view, &c3, &style);
            if d.png.is_some() {
                ui.ctx().request_repaint();
            }
            if resp.clicked()
                && let Some(p) = resp.interact_pointer_pos()
            {
                let (sx, sy) = (f64::from(p.x - area.min.x) * ppp, f64::from(p.y - area.min.y) * ppp);
                let hit = crate::view3d::pick(&d.view, &c3, sx, sy);
                let id = hit.and_then(|h| d.ids.get(h.instance).cloned()).filter(|i| !i.is_empty());
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
                tok.text,
            );
        }
        if let Some((id, shift)) = pick {
            self.pick3d(id.as_deref(), shift);
        }
        if act.as_deref() == Some("levels-dialog") {
            self.open_levels();
        } else if let Some(a) = act {
            let _ = self.takeoff_3d(&a);
        }
        self.levels_dialog(ui.ctx());
    }

    /// levelsDialog: every open sheet with its floor elevation (blank = not stacked).
    fn open_levels(&mut self) {
        let mut rows = Vec::new();
        for f in self.files() {
            for i in 0..f.pages.len() {
                #[allow(clippy::cast_precision_loss)]
                let key = cc_takeoff::model::sheet_key(f.hash(), i as f64);
                let v = self.takeoff.tk.level_of(&key).map(|x| cc_takeoff::units::format_ft_in(x, 16.0)).unwrap_or_default();
                rows.push((key.clone(), self.takeoff.sheet_label(&key), v));
            }
        }
        if let Some(d) = self.d3.as_mut() {
            d.levels = Some((rows, String::new()));
        }
    }

    fn levels_dialog(&mut self, ctx: &egui::Context) {
        let Some((mut rows, mut error)) = self.d3.as_ref().and_then(|d| d.levels.clone()) else { return };
        let (mut ok, mut close) = (false, false);
        crate::shell::Dialog::new(t("d3.levels.dlg")).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(t("d3.levels.text"));
            egui::Grid::new("tk-levels").num_columns(2).show(ui, |ui| {
                for (_, title, v) in rows.iter_mut() {
                    let l = ui.label(title.as_str());
                    ui.add(crate::shell::field(v).hint_text("0'-0\"").desired_width(120.0)).labelled_by(l.id);
                    ui.end_row();
                }
            });
            if !error.is_empty() {
                ui.colored_label(ui.visuals().error_fg_color, &error);
            }
            crate::shell::foot(ui, |ui| {
                close = ui.add(crate::shell::Btn::new(t("btn.cancel"))).clicked();
                ok = ui.add(crate::shell::Btn::primary(t("d3.levels.ok"))).clicked();
            });
        });
        if ok {
            error.clear();
            let mut map = serde_json::Map::new();
            for (key, title, v) in &rows {
                match level_value(v) {
                    Ok(x) => {
                        map.insert(key.clone(), x);
                    }
                    Err(e) => {
                        error = format!("{title}: {e}");
                        map.clear();
                        break;
                    }
                }
            }
            if error.is_empty() {
                if let Err(e) = self.takeoff.tk.set_levels(&map) {
                    self.takeoff.message = e;
                }
                self.update3d();
                close = true;
            }
        }
        if let Some(d) = self.d3.as_mut() {
            d.levels = if close { None } else { Some((rows, error)) };
        }
    }
}

/// view3d.js's navigation: a left or middle drag orbits (Shift pans), a right drag pans, the wheel zooms at the pointer.
fn input3d(ui: &egui::Ui, resp: &egui::Response, rect: Rect, cam: &mut Camera) {
    let (mods, scroll, zoom, hover) = ui.input(|i| (i.modifiers, i.smooth_scroll_delta.y, i.zoom_delta(), i.pointer.hover_pos()));
    let d = resp.drag_delta();
    let (dx, dy) = (f64::from(d.x), f64::from(d.y));
    if dx != 0.0 || dy != 0.0 {
        if resp.dragged_by(egui::PointerButton::Secondary)
            || (mods.shift && (resp.dragged_by(egui::PointerButton::Primary) || resp.dragged_by(egui::PointerButton::Middle)))
        {
            cam.pan(dx, dy);
        } else if resp.dragged_by(egui::PointerButton::Primary) || resp.dragged_by(egui::PointerButton::Middle) {
            cam.orbit(dx, dy);
        }
    }
    if resp.hovered()
        && let Some(p) = hover
    {
        let (x, y) = (f64::from(p.x - rect.min.x), f64::from(p.y - rect.min.y));
        if scroll != 0.0 {
            cam.zoom_at((-f64::from(scroll) * 0.003).exp(), Some(x), Some(y), None);
        }
        if zoom != 1.0 && zoom > 0.0 {
            cam.zoom_at(1.0 / f64::from(zoom), Some(x), Some(y), None);
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
fn drawn(
    scene: &Scene,
    tk: &crate::takeofftools::TakeoffMode,
    ground: Option<&Vec<(u32, Vec<u8>)>>,
    ground_on: bool,
    dark: bool,
) -> (Scene3D, Vec<String>) {
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
    // the ground: the sheet (its picture in TILES × TILES texture layers, else plain paper) under everything
    let (gw, gh) = scene.ground;
    if gw > 0.0 && gh > 0.0 {
        // under everything (slabs go down from the sheet's level)
        let z = scene.ground_z.unwrap_or(0.0).min(if lo[2].is_finite() { lo[2] } else { 0.0 }) - 0.01;
        inst.push(Instance3 {
            mesh: meshes.len(),
            m: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            tint: None,
            hidden: !ground_on,
        });
        meshes.push(ground_mesh(gw, gh, z, ground.is_some(), dark));
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
    let mut sc = Scene3D::new(id, meshes, inst, lo, hi);
    if let Some(g) = ground {
        sc.textures = Arc::new(g.clone());
    }
    (sc, ids)
}

/// The ground plate: TILES × TILES quads, each on its texture layer (u, v 0–1 with half a texel kept off the edge), or
/// one plain quad in the paper's colour (white; the model space's dark grey).
fn ground_mesh(gw: f64, gh: f64, z: f64, textured: bool, dark: bool) -> MeshData {
    use crate::civildrape::{LAYER0, TILES};
    let n = if textured { TILES } else { 1 };
    let origin = [gw / 2.0, gh / 2.0, z];
    let col = if textured || !dark { [255, 255, 255, 255] } else { [0x23, 0x29, 0x2f, 255] };
    let inset = 0.5 / cc_render::mesh3d::TEX_SIZE as f32;
    let mut m = MeshData { origin, ..MeshData::default() };
    for r in 0..n {
        for c in 0..n {
            #[allow(clippy::cast_precision_loss)]
            let (x0, x1) = (gw * c as f64 / n as f64, gw * (c + 1) as f64 / n as f64);
            #[allow(clippy::cast_precision_loss)]
            let (y0, y1) = (gh * r as f64 / n as f64, gh * (r + 1) as f64 / n as f64);
            #[allow(clippy::cast_possible_truncation)]
            let layer = if textured { (LAYER0 + (r * TILES + c) as u32) as f32 } else { -1.0 };
            let base = m.positions.len() as u32;
            for (x, y, u, v) in
                [(x0, y0, inset, inset), (x1, y0, 1.0 - inset, inset), (x1, y1, 1.0 - inset, 1.0 - inset), (x0, y1, inset, 1.0 - inset)]
            {
                #[allow(clippy::cast_possible_truncation)]
                m.positions.push([(x - origin[0]) as f32, (y - origin[1]) as f32, 0.0]);
                m.normals.push([0.0, 0.0, 1.0]);
                m.colors.push(col);
                m.tex.push([u, v, layer]);
            }
            m.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    let rel = |x: f64, y: f64| -> [f32; 3] {
        #[allow(clippy::cast_possible_truncation)]
        [(x - origin[0]) as f32, (y - origin[1]) as f32, 0.0]
    };
    m.edges = vec![rel(0.0, 0.0), rel(gw, 0.0), rel(gw, 0.0), rel(gw, gh), rel(gw, gh), rel(0.0, gh), rel(0.0, gh), rel(0.0, 0.0)];
    m
}
