//! ModelCraft's Render a picture… dialog (0.5 render/render-ui.js with render-mc.js's adapter): the settings (size,
//! quality, sky, the sun's own date and time, brightness, ground, edges, a look per colour on screen), then Render ⇄
//! Stop — the picture fills in as the passes add up (cc-render `still`, the GPU accumulating renderer of
//! render-core.js) — and Save PNG ("<file> render.png"). Nothing here touches the model. The model's lamps (the Lights
//! panel) light it, most of all at night.
//!
//! Actions (`model.edit=<action>`): render (opens the dialog), render-set:<settings JSON> (cleanSettings), render-go,
//! render-stop, render-save, render-close, render-status (scripting: the dialog's status into the status bar).

use std::sync::{Arc, Mutex, PoisonError};

use cc_m3d_tools::looks::{self, Settings};
use cc_render::mesh3d::{Cam3, IDENT, Instance3, MeshData, TEX_SIZE};
use cc_render::pt::{PtRenderer, PtScene};
use cc_render::still::{StillJob, StillLamp, StillMesh, StillRenderer, StillSky, srgb};
use egui::RichText;

use crate::modelcraft::{ModelView, mt};

/// Unpainted faces in a picture (render-mc.js DEFAULT_FACE): a warm light grey whatever the theme.
pub const DEFAULT_FACE: &str = "#e6e3dc";

/// The dialog's state (render-ui.js openRenderDialog).
#[derive(Default)]
pub struct RenderDlg {
    pub status: String,
    pub job: Option<Arc<Mutex<Job>>>,
    /// the job's picture last turned into `tex` (its sequence number)
    seen: u64,
    pub tex: Option<egui::TextureHandle>,
    /// the last finished picture (w, h, RGBA) and its PNG
    pub last: Option<(u32, u32, Vec<u8>)>,
    pub png: Option<Vec<u8>>,
    /// the job's start (ms on the view's clock) and whether its sun was up
    t0: f64,
    sun_up: bool,
    reported: bool,
}

impl std::fmt::Debug for RenderDlg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RenderDlg {:?}", self.status)
    }
}

/// What the GPU side needs, handed over once.
pub struct JobInput {
    pub still: StillJob,
    pub meshes: Vec<MeshData>,
    pub looks: Vec<Vec<[u8; 4]>>,
    pub instances: Vec<Instance3>,
    pub origin: [f64; 3],
}

/// A render under way, shared by the dialog and the GPU callback.
#[derive(Default)]
pub struct Job {
    pub id: u64,
    /// High quality (path traced) rather than Fast
    pub hq: bool,
    /// stopped at the time limit
    pub budget: bool,
    pub input: Option<JobInput>,
    pub passes: u32,
    pub done: u32,
    pub stop: bool,
    pub finished: bool,
    pub exposure: f64,
    pub picture: Option<(u32, u32, Vec<u8>)>,
    pub seq: u64,
    pub err: Option<String>,
}

static JOB_IDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// The GPU side of the render under way, in egui_wgpu's callback resources.
#[derive(Default)]
struct StillSlot {
    id: u64,
    r: Option<(Gpu, StillJob)>,
}

enum Gpu {
    Fast(Box<StillRenderer>),
    Hq(Box<PtRenderer>),
}

impl Gpu {
    fn done(&self) -> u32 {
        match self {
            Gpu::Fast(r) => r.done,
            Gpu::Hq(r) => r.done,
        }
    }
    fn size(&self) -> (u32, u32) {
        match self {
            Gpu::Fast(r) => (r.w, r.h),
            Gpu::Hq(r) => (r.w, r.h),
        }
    }
}

struct StillCallback {
    job: Arc<Mutex<Job>>,
}

impl egui_wgpu::CallbackTrait for StillCallback {
    fn prepare(
        &self,
        device: &egui_wgpu::wgpu::Device,
        queue: &egui_wgpu::wgpu::Queue,
        _sd: &egui_wgpu::ScreenDescriptor,
        _enc: &mut egui_wgpu::wgpu::CommandEncoder,
        res: &mut egui_wgpu::CallbackResources,
    ) -> Vec<egui_wgpu::wgpu::CommandBuffer> {
        let mut j = self.job.lock().unwrap_or_else(PoisonError::into_inner);
        if j.finished {
            return Vec::new();
        }
        if res.get::<StillSlot>().is_none() {
            res.insert(StillSlot::default());
        }
        let Some(slot) = res.get_mut::<StillSlot>() else { return Vec::new() };
        if slot.id != j.id {
            slot.r = None;
            if let Some(inp) = j.input.take() {
                let r = if j.hq {
                    // bakeItems + the box tree on the CPU (render.hq.preparing), then the path tracer
                    let st = &inp.still;
                    let (c, rr) = match st.bounds {
                        Some((lo, hi)) => {
                            let d = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
                            (
                                [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0],
                                ((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() / 2.0).max(0.5),
                            )
                        }
                        None => ([0.0; 3], 5.0),
                    };
                    let gcol = if st.sky.plain { srgb([0xf4, 0xf4, 0xf2]) } else { srgb([0xcb, 0xc6, 0xbb]) };
                    let ground = st.ground.then_some((
                        [c[0] - inp.origin[0], c[1] - inp.origin[1], st.ground_at - rr * 1e-4 - inp.origin[2]],
                        rr * 60.0,
                        gcol,
                    ));
                    let scene = PtScene::build(&inp.meshes, &inp.looks, &inp.instances, inp.origin, st.clip, ground);
                    Gpu::Hq(Box::new(PtRenderer::new(device, st.w, st.h, &scene)))
                } else {
                    let meshes: Vec<StillMesh> = inp
                        .meshes
                        .iter()
                        .enumerate()
                        .map(|(k, m)| StillMesh::upload(device, m, inp.looks.get(k).map_or(&[][..], |v| v.as_slice())))
                        .collect();
                    Gpu::Fast(Box::new(StillRenderer::new(device, inp.still.w, inp.still.h, meshes, &inp.instances, inp.origin)))
                };
                // the material textures the scene uses
                let mut loaded = 0u64;
                for m in &inp.meshes {
                    for t in &m.tex {
                        let layer = t[2];
                        if !(0.0..64.0).contains(&layer) || loaded & (1u64 << (layer as u32)) != 0 {
                            continue;
                        }
                        loaded |= 1u64 << (layer as u32);
                        if let Some(spec) =
                            cc_m3d_tools::matlib::TEXTURED.get(layer as usize).and_then(|id| cc_m3d_tools::matlib::tex_spec(id))
                        {
                            let t = cc_m3d_tools::matlib::make_texture_aspect(&spec, TEX_SIZE as usize, 1.0);
                            match &r {
                                Gpu::Fast(x) => x.set_texture(queue, layer as u32, &t.albedo),
                                Gpu::Hq(x) => x.set_texture(queue, layer as u32, &t.albedo),
                            }
                        }
                    }
                }
                slot.r = Some((r, inp.still));
            }
            slot.id = j.id;
        }
        let Some((r, still)) = slot.r.as_mut() else {
            j.finished = true;
            return Vec::new();
        };
        // previewEvery = 4: four passes (or samples) a frame, then the picture so far
        for _ in 0..4 {
            if j.stop || r.done() >= j.passes {
                break;
            }
            match r {
                Gpu::Fast(x) => x.pass(device, queue, still),
                Gpu::Hq(x) => x.sample(device, queue, still),
            }
        }
        j.done = r.done();
        let end = j.stop || r.done() >= j.passes;
        if r.done() > 0 && (r.done() % 4 == 0 || end) {
            let pic = match r {
                Gpu::Fast(x) => x.picture(device, queue, j.exposure),
                Gpu::Hq(x) => x.picture(device, queue, j.exposure),
            };
            match pic {
                Ok(px) => {
                    let (w, h) = r.size();
                    j.picture = Some((w, h, px));
                    j.seq += 1;
                }
                Err(e) => j.err = Some(e),
            }
        }
        if end {
            j.finished = true;
            slot.r = None;
        }
        Vec::new()
    }

    fn paint(&self, _info: egui::PaintCallbackInfo, _pass: &mut egui_wgpu::wgpu::RenderPass<'static>, _res: &egui_wgpu::CallbackResources) {
    }
}

fn rgb3(hex: &str) -> [f32; 3] {
    let (r, g, b) = crate::mcmats::rgb(hex).unwrap_or((0xe6, 0xe3, 0xdc));
    srgb([r, g, b])
}

fn still_sky(s: &looks::Sky, night: bool) -> StillSky {
    StillSky {
        zenith: rgb3(s.zenith),
        horizon: rgb3(s.horizon),
        ground: rgb3(s.ground),
        sun: s.sun,
        sky: s.sky,
        env: s.env,
        tint: rgb3(s.tint),
        sky_tint: rgb3(s.sky_tint),
        plain: s.plain,
        night,
    }
}

const MONTHS: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// render-ui.js hourText: "15:00".
pub fn hour_text(h: f64) -> String {
    let m = (cc_m3d::js::round((h % 1.0) * 60.0) as i64).rem_euclid(60);
    format!("{:02}:{m:02}", h.floor() as i64)
}

impl ModelView {
    /// render-mc.js keyOf(f, paint): a face's look key.
    fn render_key(paint: Option<&str>) -> String {
        let p = paint.unwrap_or("");
        if cc_m3d::paint::is_material_paint(p) {
            return if p.strip_prefix("mat:").and_then(crate::mcmats::material).is_some() { p.to_string() } else { "default".into() };
        }
        if cc_m3d::paint::is_image_paint(p) {
            return format!("img:{}", cc_m3d::paint::image_id(p));
        }
        looks::color_key(p).unwrap_or_else(|| "default".into())
    }

    /// mcColors(model): the colours on screen, most used first (library materials and pictures bring their own look).
    pub fn render_colors(&self) -> Vec<(String, usize)> {
        let mut n: Vec<(String, usize)> = Vec::new();
        self.tools.host.model.walk_shown("root", &IDENT, &mut |def: &cc_m3d::def::Def, _m: &[f64; 16], _p: &[f64], paint: Option<&str>| {
            for (_, f) in def.faces.iter() {
                if f.hidden {
                    continue;
                }
                let k = Self::render_key(f.color.as_deref().filter(|c| !c.is_empty()).or(paint));
                if k.starts_with("mat:") || k.starts_with("img:") {
                    continue;
                }
                match n.iter_mut().find(|(kk, _)| *kk == k) {
                    Some(x) => x.1 += 1,
                    None => n.push((k, 1)),
                }
            }
            true
        });
        n.sort_by_key(|x| std::cmp::Reverse(x.1));
        n
    }

    /// The sun for this picture (mcSun): the style's place and date, with the picture's own date and time if set.
    fn render_sun(&self, s: &Settings) -> Option<[f64; 3]> {
        let [mut month, mut day, mut hour, lat, lon, tz] = self.sun;
        if let Some((m, d, h)) = s.sun {
            (month, day, hour) = (m, d, h);
        }
        let p = cc_m3d_tools::sun::sun_position(&cc_m3d_tools::sun::When { month, day, hour, lat, lon, tz, year: 2026.0 });
        p.up.then_some(p.dir)
    }

    /// mcScene + renderStill's set-up: the shown model as meshes with looks, the camera at the picture's size.
    fn render_input(&self, s: &Settings) -> Option<JobInput> {
        let model = &self.tools.host.model;
        let mut meshes: Vec<MeshData> = Vec::new();
        let mut mlooks: Vec<Vec<[u8; 4]>> = Vec::new();
        let mut index: std::collections::HashMap<(String, String), usize> = std::collections::HashMap::new();
        let mut instances: Vec<Instance3> = Vec::new();
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        let front = crate::model3d::hex_rgba(DEFAULT_FACE).unwrap_or([230, 227, 220, 255]);
        let look_of = |key: &str| -> [u8; 4] {
            let q = |x: f64| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
            if let Some(id) = key.strip_prefix("mat:")
                && let Some(m) = crate::mcmats::material(id)
            {
                // libMaterial: textured ones roughness 1 (from their map), the rest their own
                let rough = if cc_m3d_tools::matlib::tex_spec(id).is_some() { 1.0 } else { m.rough };
                return [q(rough), q(m.metal), q(m.glass), 0];
            }
            if key.starts_with("img:") {
                return [q(0.7), 0, 0, 0];
            }
            let (_, rough, metal, glass, _) = looks::look_for(key, s, DEFAULT_FACE, None);
            [q(rough), q(metal), q(glass), 0]
        };
        model.walk_shown("root", &IDENT, &mut |def: &cc_m3d::def::Def, m: &[f64; 16], _p: &[f64], color: Option<&str>| {
            if def.faces.is_empty() {
                return true;
            }
            let key = (def.id.clone(), color.unwrap_or("").to_string());
            let mi = *index.entry(key).or_insert_with(|| {
                let mut per: Vec<[u8; 4]> = Vec::new();
                let mesh = crate::model3d::def_mesh_with(def, color, front, &mut |paint| per.push(look_of(&Self::render_key(paint))));
                meshes.push(mesh);
                mlooks.push(per);
                meshes.len() - 1
            });
            for (_, p) in def.verts.iter() {
                let w = [
                    m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
                    m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
                    m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
                ];
                for ((l, h), x) in lo.iter_mut().zip(hi.iter_mut()).zip(w) {
                    *l = l.min(x);
                    *h = h.max(x);
                }
            }
            instances.push(Instance3 { mesh: mi, m: *m, tint: None, hidden: false });
            true
        });
        if instances.is_empty() || !lo[0].is_finite() {
            return None;
        }
        let (vw, vh) = (self.tools.host.cam.w, self.tools.host.cam.h);
        let (w, h) = looks::size_for(s, vw, vh);
        let c = &self.tools.host.cam;
        let cam = Cam3 {
            eye: c.eye,
            target: c.target,
            fov: c.fov,
            persp: c.persp,
            width: f64::from(w),
            height: f64::from(h),
            near: c.near,
            far: c.far,
        };
        let sky0 = looks::sky(&s.sky);
        let sun = self.render_sun(s);
        let night = !sky0.plain && sun.is_none();
        let sky = if night { looks::NIGHT } else { sky0 };
        let mut lamps: Vec<&cc_m3d::model::Light> = model.lights.iter().map(|(_, l)| l).filter(|l| l.on && l.lumens > 0.0).collect();
        lamps.sort_by(|a, b| b.lumens.total_cmp(&a.lumens));
        let lamps: Vec<StillLamp> = lamps
            .into_iter()
            .take(64)
            .map(|l| StillLamp {
                kind: l.kind.clone(),
                p: l.p,
                dir: l.dir,
                color: rgb3(&l.color),
                candela: looks::lamp_candela(&l.kind, l.lumens),
                angle: l.angle,
                size: l.size,
            })
            .collect();
        let clip = model.active_cut().map(|(_, p, n)| (n, n[0] * p[0] + n[1] * p[1] + n[2] * p[2]));
        let still = StillJob {
            cam,
            w,
            h,
            passes: looks::quality(&s.quality),
            sky: still_sky(&sky, night),
            sun,
            up: [0.0, 0.0, 1.0],
            bounds: Some((lo, hi)),
            ground_at: lo[2].min(0.0),
            ground: s.ground,
            edges: s.edges,
            exposure: s.exposure,
            clip,
            lamps,
        };
        let origin = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
        Some(JobInput { still, meshes, looks: mlooks, instances, origin })
    }

    /// The render actions; None when `name` is not one of them.
    pub fn render_act(&mut self, name: &str, arg: &str) -> Option<Result<(), String>> {
        let r = match name {
            "render" => {
                if self.render_dlg.is_none() {
                    self.render_dlg = Some(RenderDlg { status: String::new(), ..RenderDlg::default() });
                }
                Ok(())
            }
            "render-set" => match serde_json::from_str::<serde_json::Value>(arg) {
                Ok(v) => {
                    self.render_settings = looks::clean_settings(&v);
                    Ok(())
                }
                Err(e) => Err(format!("model.edit: render-set: {e}")),
            },
            "render-go" => {
                self.render_go();
                Ok(())
            }
            "render-stop" => {
                if let Some(j) = self.render_dlg.as_ref().and_then(|d| d.job.clone()) {
                    j.lock().unwrap_or_else(PoisonError::into_inner).stop = true;
                }
                Ok(())
            }
            "render-save" => {
                let file = format!("{} render.png", self.base_name());
                if let Some(d) = self.render_dlg.as_mut()
                    && let Some(png) = d.png.clone()
                {
                    self.pending_save = Some((file.clone(), png));
                    d.status = mt("render.saved", &[("file", &file)]);
                }
                Ok(())
            }
            "render-status" => {
                // scripting only: the dialog's status line into the status bar (harnesses, timings)
                let st = self.render_dlg.as_ref().map(|d| d.status.clone()).unwrap_or_default();
                self.print(st);
                Ok(())
            }
            "render-close" => {
                if let Some(j) = self.render_dlg.as_ref().and_then(|d| d.job.clone()) {
                    j.lock().unwrap_or_else(PoisonError::into_inner).stop = true;
                }
                self.render_dlg = None;
                Ok(())
            }
            _ => return self.lights_act(name, arg),
        };
        Some(r)
    }

    /// go(): Render ⇄ Stop.
    fn render_go(&mut self) {
        let running = self
            .render_dlg
            .as_ref()
            .and_then(|d| d.job.as_ref())
            .is_some_and(|j| !j.lock().unwrap_or_else(PoisonError::into_inner).finished);
        if running {
            let _ = self.render_act("render-stop", "");
            return;
        }
        if self.render_dlg.is_none() {
            self.render_dlg = Some(RenderDlg::default());
        }
        let s = self.render_settings.clone();
        let input = self.render_input(&s);
        let now = self.now();
        let Some(d) = self.render_dlg.as_mut() else { return };
        let Some(input) = input else {
            d.status = mt("render.empty", &[]);
            return;
        };
        let n = input.still.passes;
        let hq = s.mode == "hq";
        d.sun_up = input.still.sun.is_some();
        d.status = if hq { mt("render.hq.preparing", &[]) } else { mt("render.progress", &[("done", "0"), ("n", &n.to_string())]) };
        d.t0 = now;
        d.reported = false;
        d.job = Some(Arc::new(Mutex::new(Job {
            id: JOB_IDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            input: Some(input),
            passes: n,
            hq,
            exposure: s.exposure,
            ..Job::default()
        })));
    }

    /// The dialog, each frame: drive the GPU job, follow its progress, draw the form.
    pub fn render_dialog(&mut self, ctx: &egui::Context, act: &mut Option<String>) {
        let now = self.now();
        let colors = if self.render_dlg.is_some() { self.render_colors() } else { Vec::new() };
        let (vw, vh) = (self.tools.host.cam.w, self.tools.host.cam.h);
        let msun = (self.sun[0], self.sun[1], self.sun[2]);
        let mut s = self.render_settings.clone();
        let Some(d) = self.render_dlg.as_mut() else { return };
        // progress
        let mut running = false;
        if let Some(j) = d.job.clone() {
            let mut j = j.lock().unwrap_or_else(PoisonError::into_inner);
            running = !j.finished;
            // the time limit (High quality)
            if running && j.hq && s.minutes > 0.0 && now - d.t0 > s.minutes * 60_000.0 && !j.stop {
                j.stop = true;
                j.budget = true;
            }
            if j.seq != d.seen
                && let Some((w, h, px)) = &j.picture
            {
                d.seen = j.seq;
                let img = egui::ColorImage::from_rgba_unmultiplied([*w as usize, *h as usize], px);
                d.tex = Some(ctx.load_texture("mc-render", img, egui::TextureOptions::LINEAR));
                d.last = Some((*w, *h, px.clone()));
            }
            let n = j.passes;
            if running {
                if !(j.hq && j.done == 0) {
                    d.status = mt(
                        if j.hq { "render.hq.progress" } else { "render.progress" },
                        &[("done", &j.done.to_string()), ("n", &n.to_string())],
                    );
                }
            } else if !d.reported {
                d.reported = true;
                let secs = format!("{:.1}", ((now - d.t0) / 1000.0).max(0.0));
                d.status = if let Some(e) = &j.err {
                    mt("render.failed", &[("error", e)])
                } else if j.budget {
                    mt("render.budget", &[("passes", &j.done.to_string()), ("s", &secs)])
                } else if j.stop && j.done < n {
                    if j.done > 0 {
                        mt(if j.hq { "render.hq.stopped" } else { "render.stopped" }, &[("passes", &j.done.to_string()), ("s", &secs)])
                    } else {
                        mt("render.stopped.none", &[])
                    }
                } else {
                    let (w, h) = d.last.as_ref().map_or((0, 0), |l| (l.0, l.1));
                    mt(
                        if j.hq { "render.hq.done" } else { "render.done" },
                        &[("w", &w.to_string()), ("h", &h.to_string()), ("passes", &j.done.to_string()), ("s", &secs)],
                    )
                };
                if !d.sun_up {
                    d.status.push(' ');
                    d.status.push_str(&mt("render.sun.down", &[]));
                }
                d.png = d.last.as_ref().and_then(|(w, h, px)| cc_io_markup::picture::encode_png(*w, *h, px));
            }
        }
        let mut open = true;
        egui::Window::new(mt("render.title", &[]))
            .id(egui::Id::unique("mc-render-dialog"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                if let Some(j) = d.job.clone().filter(|_| running) {
                    // the GPU job runs in this paint callback (a pixel of the dialog), four passes a frame
                    let r = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(1.0, 1.0));
                    ui.painter().add(egui_wgpu::Callback::new_paint_callback(r, StillCallback { job: j }));
                    ctx.request_repaint();
                }
                ui.horizontal_top(|ui| {
                    // the picture
                    ui.vertical(|ui| {
                        let box_size = egui::vec2(560.0, 380.0);
                        match &d.tex {
                            Some(t) => {
                                let sz = t.size_vec2();
                                let k = (box_size.x / sz.x).min(box_size.y / sz.y).min(1.0);
                                ui.image((t.id(), sz * k));
                            }
                            None => {
                                ui.allocate_ui(box_size, |ui| {
                                    ui.centered_and_justified(|ui| ui.label(mt("render.placeholder", &[])));
                                });
                            }
                        }
                    });
                    // the form
                    ui.vertical(|ui| {
                        ui.set_width(270.0);
                        ui.add_enabled_ui(!running, |ui| {
                            egui::Grid::new("mc-render-form").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                                ui.label(mt("render.mode", &[]));
                                egui::ComboBox::new("mc-render-mode", "")
                                    .selected_text(mt(&format!("render.mode.{}", s.mode), &[]))
                                    .show_ui(ui, |ui| {
                                        for k in looks::MODES {
                                            ui.selectable_value(&mut s.mode, k.to_string(), mt(&format!("render.mode.{k}"), &[]));
                                        }
                                    });
                                ui.end_row();
                                if s.mode == "hq" {
                                    ui.label(mt("render.limit", &[]));
                                    let ml = |m: f64| // 0.5 asks for a key its catalog lacks (render.minutes): plain words here
                                    if m > 0.0 { format!("{} min", cc_m3d::js::num_str(m)) } else { mt("render.minutes.none", &[]) };
                                    egui::ComboBox::new("mc-render-minutes", "").selected_text(ml(s.minutes)).show_ui(ui, |ui| {
                                        for m in looks::MINUTES {
                                            ui.selectable_value(&mut s.minutes, m, ml(m));
                                        }
                                    });
                                    ui.end_row();
                                }
                                ui.label(mt("render.size", &[]));
                                let lab = |k: &str| {
                                    let z = looks::size_for(&Settings { size: k.into(), ..Settings::default() }, vw, vh);
                                    mt(&format!("render.size.{k}"), &[("w", &z.0.to_string()), ("h", &z.1.to_string())])
                                };
                                egui::ComboBox::new("mc-render-size", "").selected_text(lab(&s.size)).show_ui(ui, |ui| {
                                    for k in looks::SIZES {
                                        ui.selectable_value(&mut s.size, k.to_string(), lab(k));
                                    }
                                });
                                ui.end_row();
                                ui.label(mt("render.quality", &[]));
                                let ql = |k: &str| mt(&format!("render.quality.{k}"), &[("n", &looks::quality(k).to_string())]);
                                egui::ComboBox::new("mc-render-quality", "").selected_text(ql(&s.quality)).show_ui(ui, |ui| {
                                    for (k, _) in looks::QUALITY {
                                        ui.selectable_value(&mut s.quality, k.to_string(), ql(k));
                                    }
                                });
                                ui.end_row();
                                ui.label(mt("render.sky", &[]));
                                egui::ComboBox::new("mc-render-sky", "").selected_text(mt(&format!("render.sky.{}", s.sky), &[])).show_ui(
                                    ui,
                                    |ui| {
                                        for k in looks::SKIES.map(|x| x.key) {
                                            ui.selectable_value(&mut s.sky, k.to_string(), mt(&format!("render.sky.{k}"), &[]));
                                        }
                                    },
                                );
                                ui.end_row();
                            });
                            ui.label(
                                RichText::new(mt(if s.mode == "hq" { "render.mode.hq.note" } else { "render.mode.fast.note" }, &[]))
                                    .small(),
                            );
                            // the sun
                            ui.label(RichText::new(mt("render.sun", &[])).strong());
                            let month = MONTHS.get((msun.0 as usize).saturating_sub(1)).copied().unwrap_or("June");
                            ui.label(
                                RichText::new(mt(
                                    "render.sun.model",
                                    &[
                                        ("date", &format!("{month} {}", cc_m3d::js::num_str(msun.1))),
                                        ("time", &hour_text(msun.2)),
                                        ("lat", "40"),
                                    ],
                                ))
                                .small(),
                            );
                            let mut own = s.sun.is_some();
                            if ui.checkbox(&mut own, mt("render.sun.own", &[])).changed() {
                                s.sun = own.then_some((msun.0, msun.1, msun.2));
                            }
                            if let Some((mut m, mut dd, mut h)) = s.sun {
                                ui.horizontal(|ui| {
                                    egui::ComboBox::new("mc-render-month", "")
                                        .selected_text(MONTHS.get((m as usize).saturating_sub(1)).copied().unwrap_or(""))
                                        .show_ui(ui, |ui| {
                                            for (k, name) in MONTHS.iter().enumerate() {
                                                ui.selectable_value(&mut m, (k + 1) as f64, *name);
                                            }
                                        });
                                    ui.add(egui::DragValue::new(&mut dd).range(1.0..=31.0).speed(0.2).max_decimals(0));
                                });
                                let label = hour_text(h);
                                ui.add(egui::Slider::new(&mut h, 0.0..=23.75).step_by(0.25).show_value(false).text(label));
                                s.sun = Some((m, dd, h));
                            }
                            ui.horizontal(|ui| {
                                let ev = s.exposure;
                                ui.label(format!(
                                    "{} {}{}",
                                    mt("render.exposure", &[]),
                                    if ev > 0.0 { "+" } else { "" },
                                    cc_m3d::js::num_str(ev)
                                ));
                            });
                            ui.add(egui::Slider::new(&mut s.exposure, -3.0..=3.0).step_by(0.5).show_value(false));
                            ui.checkbox(&mut s.ground, mt("render.ground", &[]));
                            if s.mode != "hq" {
                                ui.checkbox(&mut s.edges, mt("render.edges", &[]));
                            }
                            if !colors.is_empty() {
                                ui.label(RichText::new(mt("render.looks", &[])).strong());
                                ui.label(RichText::new(mt("render.looks.hint", &[])).small());
                                egui::ScrollArea::vertical().max_height(160.0).id_salt("mc-render-looks").show(ui, |ui| {
                                    for (key, faces) in colors.iter().take(200) {
                                        ui.horizontal(|ui| {
                                            let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                                            let col = crate::mcmats::rgb(if key == "default" { DEFAULT_FACE } else { key })
                                                .map_or(egui::Color32::GRAY, |(r, g, b)| egui::Color32::from_rgb(r, g, b));
                                            ui.painter().rect_filled(r, 1.0, col);
                                            let name = if key == "default" { mt("render.default", &[]) } else { key.clone() };
                                            ui.label(&name).on_hover_text(faces.to_string());
                                            let cur = s.look_of(key).unwrap_or("matte").to_string();
                                            let mut pick = cur.clone();
                                            egui::ComboBox::new(("mc-render-look", key.as_str()), "")
                                                .selected_text(mt(&format!("render.look.{cur}"), &[]))
                                                .show_ui(ui, |ui| {
                                                    for (k, ..) in looks::LOOKS {
                                                        ui.selectable_value(&mut pick, k.to_string(), mt(&format!("render.look.{k}"), &[]));
                                                    }
                                                });
                                            if pick != cur {
                                                s.looks.retain(|(k, _)| k != key);
                                                if pick != "matte" {
                                                    s.looks.push((key.clone(), pick));
                                                }
                                            }
                                        });
                                    }
                                });
                            }
                        });
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&d.status).small());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(mt("render.close", &[])).clicked() {
                            *act = Some("render-close".into());
                        }
                        if ui.add_enabled(!running && d.png.is_some(), egui::Button::new(mt("render.save", &[]))).clicked() {
                            *act = Some("render-save".into());
                        }
                        if ui.button(mt(if running { "render.stop" } else { "render.go" }, &[])).clicked() {
                            *act = Some("render-go".into());
                        }
                    });
                });
            });
        if !open {
            *act = Some("render-close".into());
        }
        self.render_settings = s;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dialog_cleans_its_settings_and_starts_a_job() {
        let mut v = ModelView::new(cc_m3d::model::Model::new(), "t", false).unwrap();
        v.act("render").unwrap();
        v.act("render-go").unwrap();
        assert_eq!(v.render_dlg.as_ref().unwrap().status, mt("render.empty", &[]), "an empty model has nothing to render");
        v.act(r##"render-set:{"mode":"hq","quality":"draft","exposure":9,"looks":{"#ABC":"glass"}}"##).unwrap();
        assert_eq!(v.render_settings.mode, "hq");
        assert_eq!(v.render_settings.exposure, 3.0);
        assert_eq!(v.render_settings.looks, vec![("#aabbcc".to_string(), "glass".to_string())]);
        assert!(v.act("render-set:{").is_err());
        let mut m = v.tools.host.model.clone();
        let _ = m.change("box", |m: &mut cc_m3d::model::Model| {
            let d = m.root_mut().ok_or(cc_m3d::error::Error::Type)?;
            cc_m3d::ops::add_polygon(d, &[[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 1.0, 0.0], [0.0, 1.0, 0.0]], None)?;
            Ok(())
        });
        v.tools.host.model = m;
        v.act("render-go").unwrap();
        let d = v.render_dlg.as_ref().unwrap();
        assert_eq!(d.status, mt("render.hq.preparing", &[]));
        let j = d.job.as_ref().unwrap().lock().unwrap();
        assert_eq!(j.passes, 16);
        assert!(j.hq && j.input.is_some());
        drop(j);
        v.act("render-close").unwrap();
        assert!(v.render_dlg.is_none());
    }
}
