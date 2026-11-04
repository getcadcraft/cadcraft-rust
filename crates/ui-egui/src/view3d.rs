//! The 3D viewport (ModelCraft, M6): the scene drawn by `cc-render::mesh3d` into its own offscreen target only when
//! something changed (scene, camera, style or size), then laid onto the UI's pass. Orbit with the middle button (or
//! the right button), pan with Shift + middle, zoom with the wheel about the pointer: on desktop and web alike.

use std::sync::Arc;

use cc_render::mesh3d::{Blit, Cam3, GpuMesh, Instance3, InstanceSet, MeshData, Renderer3, Scene3, Stats3, Style3, Target3};
use egui::{Rect, Sense};

/// A scene to show: meshes (one per shared definition) and their placements. `id` changes when the meshes do;
/// `version` when only the placements (or their tints: selection) do.
#[derive(Clone, Debug)]
pub struct Scene3D {
    pub id: u64,
    pub version: u64,
    pub meshes: Arc<Vec<MeshData>>,
    pub instances: Arc<Vec<Instance3>>,
    /// the box of everything (world), for Zoom Extents
    pub min: [f64; 3],
    pub max: [f64; 3],
    /// the picking tree, made on the first pick
    pub picker: Arc<std::sync::OnceLock<Picker>>,
    /// a point cloud drawn with the scene (CadCraft's 3D tab: the cloud under the drawing)
    pub cloud: Option<CloudLayer>,
    /// texture layers this scene brings itself (CivilCraft's map drape): (layer, TEX_SIZE² RGBA), uploaded with it
    pub textures: Arc<Vec<(u32, Vec<u8>)>>,
    /// pictures as texture layers: (layer, key of the picture, TEX_SIZE² RGBA)
    pub images: Vec<(u32, u64, Arc<Vec<u8>>)>,
    /// lines in the scene, depth-tested with it (ModelCraft's drawing axes and guide lines)
    pub lines: Option<Arc<cc_render::lines3::Lines3>>,
    /// a picture of the view asked for (TakeoffCraft's PNG): drawn again offscreen and read back into it
    pub snapshot: Option<Arc<std::sync::Mutex<Snapshot>>>,
}

/// A picture read back: width, height, RGBA rows.
pub type Picture = (u32, u32, Vec<u8>);

/// A picture of the view (viewimage.js `viewPng`): the view drawn again offscreen at `scale` times its size in physical
/// pixels (at most 8192 a side) and read back without waiting on the GPU — the pixels land in a later frame, on the web
/// too — as (width, height, RGBA rows) in `out`.
#[derive(Debug, Default)]
pub struct Snapshot {
    pub scale: f64,
    pub out: Option<Result<Picture, String>>,
    started: bool,
}

impl Snapshot {
    pub fn new(scale: f64) -> Arc<std::sync::Mutex<Snapshot>> {
        Arc::new(std::sync::Mutex::new(Snapshot { scale, out: None, started: false }))
    }
}

/// A read back under way: the buffer, the picture's size, its padded row, whether the map finished, where it goes.
struct Pending {
    buf: egui_wgpu::wgpu::Buffer,
    w: u32,
    h: u32,
    padded: u32,
    done: Arc<std::sync::Mutex<Option<Result<(), String>>>>,
    slot: Arc<std::sync::Mutex<Snapshot>>,
}

/// A point cloud in a 3D scene: its points and colours (shared, not copied) and how they are drawn.
#[derive(Clone, Debug)]
pub struct CloudLayer {
    pub cloud: Arc<cc_io_civil::pointcloud::Cloud>,
    pub colors: Arc<Vec<u8>>,
    pub draw: cc_render::points3::CloudDraw,
}
impl CloudLayer {
    /// What the GPU copy was made from (the same arrays: no upload).
    fn key(&self) -> (usize, usize) {
        (Arc::as_ptr(&self.cloud) as usize, Arc::as_ptr(&self.colors) as usize)
    }
}

impl Scene3D {
    pub fn new(id: u64, meshes: Vec<MeshData>, instances: Vec<Instance3>, min: [f64; 3], max: [f64; 3]) -> Scene3D {
        Scene3D {
            id,
            version: 0,
            meshes: Arc::new(meshes),
            instances: Arc::new(instances),
            min,
            max,
            picker: Arc::default(),
            cloud: None,
            textures: Arc::default(),
            images: Vec::new(),
            lines: None,
            snapshot: None,
        }
    }
    /// Hide some instances (a storey, a class): not drawn, not picked.
    pub fn set_hidden(&mut self, hidden: &dyn Fn(usize) -> bool) {
        let list: Vec<Instance3> = self.instances.iter().enumerate().map(|(i, x)| Instance3 { hidden: hidden(i), ..*x }).collect();
        self.instances = Arc::new(list);
        self.version = self.version.wrapping_add(1);
    }
    /// Paint some instances (selection) and leave the rest as they are: the GPU instance data is made again, the
    /// meshes stay.
    pub fn set_tints(&mut self, tint: &dyn Fn(usize) -> Option<[u8; 4]>) {
        let list: Vec<Instance3> = self.instances.iter().enumerate().map(|(i, x)| Instance3 { tint: tint(i), ..*x }).collect();
        self.instances = Arc::new(list);
        self.version = self.version.wrapping_add(1);
    }
}

/// Finds what is under a pixel: a box tree over the instances' world boxes, then each candidate's triangles in its own
/// frame (exact, nearest hit).
#[derive(Debug)]
pub struct Picker {
    tree: cc_geom::bvh::BoxTree,
}

/// A hit: which instance and where (world).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub instance: usize,
    pub t: f64,
    pub point: [f64; 3],
}

fn world_box(mesh: &MeshData, m: &[f64; 16]) -> [f64; 6] {
    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    for p in &mesh.positions {
        for ((l, h), v) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
            *l = l.min(*v);
            *h = h.max(*v);
        }
    }
    if !lo[0].is_finite() {
        return [f64::NAN; 6];
    }
    let mut out = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for k in 0..8 {
        let c = [
            mesh.origin[0] + f64::from(if k & 1 == 0 { lo[0] } else { hi[0] }),
            mesh.origin[1] + f64::from(if k & 2 == 0 { lo[1] } else { hi[1] }),
            mesh.origin[2] + f64::from(if k & 4 == 0 { lo[2] } else { hi[2] }),
        ];
        let w = [
            m[0] * c[0] + m[4] * c[1] + m[8] * c[2] + m[12],
            m[1] * c[0] + m[5] * c[1] + m[9] * c[2] + m[13],
            m[2] * c[0] + m[6] * c[1] + m[10] * c[2] + m[14],
        ];
        let (lo_o, hi_o) = out.split_at_mut(3);
        for ((l, h), v) in lo_o.iter_mut().zip(hi_o.iter_mut()).zip(w) {
            *l = l.min(v);
            *h = h.max(v);
        }
    }
    out
}

impl Picker {
    pub fn new(scene: &Scene3D) -> Picker {
        let mut boxes = Vec::with_capacity(scene.instances.len() * 6);
        for inst in scene.instances.iter() {
            match scene.meshes.get(inst.mesh) {
                Some(m) => boxes.extend_from_slice(&world_box(m, &inst.m)),
                None => boxes.extend_from_slice(&[f64::NAN; 6]),
            }
        }
        Picker { tree: cc_geom::bvh::BoxTree::new(boxes) }
    }
}

/// The nearest instance under pixel (sx, sy) of the camera's drawing area.
pub fn pick(scene: &Scene3D, cam: &Cam3, sx: f64, sy: f64) -> Option<Hit> {
    pick_where(scene, cam, sx, sy, &|_| true)
}

/// [`pick`] keeping only hits at world points (metres) that pass `keep` (what a section cut leaves drawn).
pub fn pick_where(scene: &Scene3D, cam: &Cam3, sx: f64, sy: f64, keep: &dyn Fn([f64; 3]) -> bool) -> Option<Hit> {
    let picker = scene.picker.get_or_init(|| Picker::new(scene));
    let (o, d) = cam.ray(sx, sy);
    let tmax = std::cell::Cell::new(f64::INFINITY);
    let mut best: Option<Hit> = None;
    picker.tree.ray(o, d, &tmax, |i| {
        let Some(inst) = scene.instances.get(i).filter(|x| !x.hidden) else { return false };
        let Some(mesh) = scene.meshes.get(inst.mesh) else { return false };
        let full = cc_render::mesh3d::mmul(&inst.m, &cc_render::mesh3d::translation(mesh.origin));
        let Some(inv) = cc_render::mesh3d::invert(&full) else { return false };
        let lo = [
            inv[0] * o[0] + inv[4] * o[1] + inv[8] * o[2] + inv[12],
            inv[1] * o[0] + inv[5] * o[1] + inv[9] * o[2] + inv[13],
            inv[2] * o[0] + inv[6] * o[1] + inv[10] * o[2] + inv[14],
        ];
        let ld = [
            inv[0] * d[0] + inv[4] * d[1] + inv[8] * d[2],
            inv[1] * d[0] + inv[5] * d[1] + inv[9] * d[2],
            inv[2] * d[0] + inv[6] * d[1] + inv[10] * d[2],
        ];
        let at = |t: f64| [o[0] + d[0] * t, o[1] + d[1] * t, o[2] + d[2] * t];
        if let Some(t) = cc_render::mesh3d::ray_mesh_where(mesh, lo, ld, &|t| keep(at(t)))
            && t < tmax.get()
        {
            tmax.set(t);
            best = Some(Hit { instance: i, t, point: [o[0] + d[0] * t, o[1] + d[1] * t, o[2] + d[2] * t] });
        }
        false
    });
    best
}

/// GPU objects of the 3D view, kept in egui_wgpu's callback resources.
pub struct Gpu3 {
    renderer: Renderer3,
    blit: Blit,
    target: Option<(Target3, egui_wgpu::wgpu::BindGroup)>,
    scene: u64,
    meshes: Vec<GpuMesh>,
    instances: Option<InstanceSet>,
    /// the scene version the instance data was made for
    inst_version: u64,
    /// material texture layers uploaded so far
    tex_loaded: u64,
    /// the picture in each picture layer (layer → key)
    img_keys: std::collections::HashMap<u32, u64>,
    /// a frame being read back
    reading: Option<Reading>,
    /// what the target shows now: (scene id, version, camera, style)
    shown: Option<(u64, u64, Cam3, Style3)>,
    pub last: Stats3,
    /// point clouds: the pipeline, the cloud on the GPU (and what it was made from), what the target shows of it
    points: cc_render::points3::Points3,
    cloud: Option<((usize, usize), cc_render::points3::GpuCloud)>,
    shown_cloud: Option<((usize, usize), cc_render::points3::CloudDraw)>,
    /// lines in the scene: the pipeline, the lines on the GPU (keyed by what they were made from), what the target shows
    line_pass: cc_render::lines3::LinePass,
    lines: Option<(Arc<cc_render::lines3::Lines3>, cc_render::lines3::GpuLines)>,
    shown_lines: Option<Arc<cc_render::lines3::Lines3>>,
    /// pictures being read back (Snapshot)
    pending: Vec<Pending>,
}

/// Install the 3D renderer next to the 2D one (hosts call [`crate::canvas::install_gpu`], which calls this).
pub fn install(rs: &egui_wgpu::RenderState) {
    let g = Gpu3 {
        renderer: Renderer3::new(&rs.device),
        blit: Blit::new(&rs.device, rs.target_format, 1),
        target: None,
        scene: u64::MAX,
        meshes: Vec::new(),
        instances: None,
        inst_version: u64::MAX,
        tex_loaded: 0,
        img_keys: std::collections::HashMap::new(),
        reading: None,
        shown: None,
        last: Stats3::default(),
        points: cc_render::points3::Points3::new(&rs.device),
        cloud: None,
        shown_cloud: None,
        line_pass: cc_render::lines3::LinePass::new(&rs.device),
        lines: None,
        shown_lines: None,
        pending: Vec::new(),
    };
    rs.renderer.write().callback_resources.insert(g);
}

struct Callback {
    scene: Scene3D,
    cam: Cam3,
    style: Style3,
    /// target size in physical pixels
    px: (u32, u32),
    /// a frame wanted back (scene recording)
    capture: Option<Arc<std::sync::Mutex<Capture>>>,
}

/// Frames of the 3D view read back (ModelCraft's scene recording): set `want` and a later frame's picture (the view as
/// rendered, RGBA rows top to bottom) lands in `frame`; `failed` when the GPU cannot give it back.
#[derive(Debug, Default)]
pub struct Capture {
    pub want: bool,
    pub frame: Option<(u32, u32, Vec<u8>)>,
    pub failed: bool,
}

/// A read back under way: the buffer, the size, the padded row, whether it is mapped (or failed).
struct Reading {
    buf: egui_wgpu::wgpu::Buffer,
    w: u32,
    h: u32,
    padded: u32,
    done: Arc<std::sync::atomic::AtomicU8>,
}

impl egui_wgpu::CallbackTrait for Callback {
    fn prepare(
        &self,
        device: &egui_wgpu::wgpu::Device,
        queue: &egui_wgpu::wgpu::Queue,
        _sd: &egui_wgpu::ScreenDescriptor,
        _enc: &mut egui_wgpu::wgpu::CommandEncoder,
        res: &mut egui_wgpu::CallbackResources,
    ) -> Vec<egui_wgpu::wgpu::CommandBuffer> {
        let Some(g) = res.get_mut::<Gpu3>() else { return Vec::new() };
        if g.scene != self.scene.id {
            g.meshes = self.scene.meshes.iter().map(|m| GpuMesh::upload(device, m)).collect();
            // the material textures this scene shows, drawn by code once each (textures.js at aspect 1: a square layer)
            let own: u64 = self.scene.textures.iter().filter(|(l, _)| *l < 64).fold(0, |a, (l, _)| a | (1u64 << l));
            for m in self.scene.meshes.iter() {
                for t in &m.tex {
                    let layer = t[2];
                    if layer < 0.0 || layer >= 64.0 || own & (1u64 << (layer as u32)) != 0 {
                        continue;
                    }
                    let bit = 1u64 << (layer as u32);
                    if g.tex_loaded & bit != 0 {
                        continue;
                    }
                    g.tex_loaded |= bit;
                    if let Some(id) = cc_m3d_tools::matlib::TEXTURED.get(layer as usize)
                        && let Some(spec) = cc_m3d_tools::matlib::tex_spec(id)
                    {
                        let t = cc_m3d_tools::matlib::make_texture_aspect(&spec, cc_render::mesh3d::TEX_SIZE as usize, 1.0);
                        g.renderer.set_texture(queue, layer as u32, &t.albedo);
                    }
                }
            }
            // the scene's own layers (a map drape): uploaded now; a library material or a picture there is drawn again when needed
            for (layer, rgba) in self.scene.textures.iter() {
                g.renderer.set_texture(queue, *layer, rgba);
                g.img_keys.remove(layer);
                if *layer < 64 {
                    g.tex_loaded &= !(1u64 << layer);
                }
            }
            // imported pictures, into their layers when the picture there changed
            for (layer, key, px) in &self.scene.images {
                if g.img_keys.get(layer) != Some(key) {
                    g.renderer.set_texture(queue, *layer, px);
                    g.img_keys.insert(*layer, *key);
                    if *layer < 64 {
                        g.tex_loaded &= !(1u64 << layer);
                    }
                }
            }
            g.scene = self.scene.id;
            g.inst_version = u64::MAX;
            g.shown = None;
        }
        if g.inst_version != self.scene.version {
            let (lo, hi) = (self.scene.min, self.scene.max);
            let origin = if lo[0].is_finite() { [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0] } else { [0.0; 3] };
            g.instances = Some(InstanceSet::new(device, &g.meshes, &self.scene.instances, origin));
            g.inst_version = self.scene.version;
        }
        if g.target.as_ref().is_none_or(|(t, _)| (t.width, t.height) != self.px) {
            let t = Target3::new(device, self.px.0, self.px.1);
            let b = g.blit.bind(device, &t);
            g.target = Some((t, b));
            g.shown = None;
        }
        let key = (self.scene.id, self.scene.version, self.cam, self.style);
        // a point cloud: uploaded once per cloud (and colours), drawn after the scene into the same target
        let ckey = self.scene.cloud.as_ref().map(|c| (c.key(), c.draw));
        if let Some(c) = &self.scene.cloud
            && g.cloud.as_ref().is_none_or(|(k, _)| *k != c.key())
        {
            g.cloud = Some((c.key(), cc_render::points3::GpuCloud::upload(device, &c.cloud.pos, &c.colors)));
        }
        if g.shown_cloud != ckey {
            g.shown = None;
        }
        // lines in the scene: uploaded once per set (the Arc kept, so a new set is never mistaken for an old one)
        let lkey = self.scene.lines.clone();
        if let Some(l) = &self.scene.lines
            && g.lines.as_ref().is_none_or(|(k, _)| !Arc::ptr_eq(k, l))
        {
            g.lines = Some((l.clone(), cc_render::lines3::GpuLines::upload(device, l)));
        }
        let same = match (&g.shown_lines, &lkey) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        };
        if !same {
            g.shown = None;
        }
        if g.shown != Some(key)
            && let (Some((t, _)), Some(set)) = (&g.target, &g.instances)
        {
            g.last = g.renderer.render(
                device,
                queue,
                t,
                Scene3 { meshes: &g.meshes, instances: set, bounds: Some((self.scene.min, self.scene.max)) },
                &self.cam,
                &self.style,
            );
            if let (Some(c), Some((_, gc))) = (&self.scene.cloud, &g.cloud) {
                g.points.render(device, queue, t, &self.cam, gc, &c.draw);
            }
            if let (Some(_), Some((_, gl))) = (&self.scene.lines, &g.lines) {
                g.line_pass.render(device, queue, t, &self.cam, gl);
            }
            g.shown = Some(key);
            g.shown_cloud = ckey;
            g.shown_lines = lkey;
        }
        if let Some(cap) = &self.capture {
            capture(device, queue, g, cap);
        }
        // a picture asked for: drawn again offscreen at its size, copied to a buffer that is mapped without waiting
        if let Some(slot) = &self.scene.snapshot {
            let start = slot.lock().map(|mut c| !std::mem::replace(&mut c.started, true)).unwrap_or(false);
            if start && let Some(set) = &g.instances {
                let k = slot.lock().map(|c| c.scale).unwrap_or(1.0);
                let k = if k.is_finite() && k > 0.0 { k } else { 1.0 };
                let (pw, ph) = (f64::from(self.px.0) * k, f64::from(self.px.1) * k);
                let fit = (8192.0 / pw.max(ph).max(1.0)).min(1.0);
                let (w, h) = (((pw * fit).round() as u32).max(1), ((ph * fit).round() as u32).max(1));
                let t = Target3::new(device, w, h);
                let cam = Cam3 { width: f64::from(w), height: f64::from(h), ..self.cam };
                g.renderer.render(
                    device,
                    queue,
                    &t,
                    Scene3 { meshes: &g.meshes, instances: set, bounds: Some((self.scene.min, self.scene.max)) },
                    &cam,
                    &self.style,
                );
                let padded = (w * 4).div_ceil(256) * 256;
                let buf = device.create_buffer(&egui_wgpu::wgpu::BufferDescriptor {
                    label: Some("cc3 picture"),
                    size: u64::from(padded) * u64::from(h),
                    usage: egui_wgpu::wgpu::BufferUsages::COPY_DST | egui_wgpu::wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
                let mut enc = device.create_command_encoder(&egui_wgpu::wgpu::CommandEncoderDescriptor { label: Some("cc3 picture") });
                enc.copy_texture_to_buffer(
                    egui_wgpu::wgpu::TexelCopyTextureInfo {
                        texture: &t.color,
                        mip_level: 0,
                        origin: egui_wgpu::wgpu::Origin3d::ZERO,
                        aspect: egui_wgpu::wgpu::TextureAspect::All,
                    },
                    egui_wgpu::wgpu::TexelCopyBufferInfo {
                        buffer: &buf,
                        layout: egui_wgpu::wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(h) },
                    },
                    egui_wgpu::wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                );
                queue.submit([enc.finish()]);
                let done: Arc<std::sync::Mutex<Option<Result<(), String>>>> = Arc::default();
                let d2 = done.clone();
                buf.slice(..).map_async(egui_wgpu::wgpu::MapMode::Read, move |r| {
                    if let Ok(mut d) = d2.lock() {
                        *d = Some(r.map_err(|e| e.to_string()));
                    }
                });
                g.pending.push(Pending { buf, w, h, padded, done, slot: slot.clone() });
            }
        }
        if !g.pending.is_empty() {
            let _ = device.poll(egui_wgpu::wgpu::PollType::Poll);
            g.pending.retain(|p| {
                let Some(r) = p.done.lock().ok().and_then(|mut d| d.take()) else { return true };
                let out = r.and_then(|()| {
                    let data = p.buf.slice(..).get_mapped_range().map_err(|e| e.to_string())?;
                    let row = (p.w * 4) as usize;
                    let mut px = Vec::with_capacity(row * p.h as usize);
                    for y in 0..p.h as usize {
                        let a = y * p.padded as usize;
                        px.extend_from_slice(data.get(a..a + row).ok_or("short read back")?);
                    }
                    Ok((p.w, p.h, px))
                });
                p.buf.unmap();
                if let Ok(mut c) = p.slot.lock() {
                    c.out = Some(out);
                }
                false
            });
        }
        Vec::new()
    }

    fn paint(&self, info: egui::PaintCallbackInfo, pass: &mut egui_wgpu::wgpu::RenderPass<'static>, res: &egui_wgpu::CallbackResources) {
        let Some(g) = res.get::<Gpu3>() else { return };
        let Some((_, bind)) = &g.target else { return };
        let vp = info.viewport_in_pixels();
        pass.set_viewport(vp.left_px as f32, vp.top_px as f32, vp.width_px as f32, vp.height_px as f32, 0.0, 1.0);
        g.blit.draw(pass, bind);
    }
}

/// The read back of a recorded frame: a finished one goes to `cap.frame`; a wanted one starts (one at a time, never
/// blocking: the buffer is mapped asynchronously, as WebGPU needs).
fn capture(device: &egui_wgpu::wgpu::Device, queue: &egui_wgpu::wgpu::Queue, g: &mut Gpu3, cap: &Arc<std::sync::Mutex<Capture>>) {
    use egui_wgpu::wgpu;
    use std::sync::atomic::Ordering;
    let mut c = cap.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = device.poll(wgpu::PollType::Poll);
    if let Some(r) = &g.reading {
        match r.done.load(Ordering::Acquire) {
            1 => {
                let slice = r.buf.slice(..);
                let rows = slice.get_mapped_range().map(|data| {
                    let row = (r.w * 4) as usize;
                    let mut out = Vec::with_capacity(row * r.h as usize);
                    for y in 0..r.h as usize {
                        let start = y * r.padded as usize;
                        out.extend_from_slice(data.get(start..start + row).unwrap_or(&[]));
                    }
                    out
                });
                r.buf.unmap();
                match rows {
                    Ok(px) if px.len() == (r.w * r.h * 4) as usize => c.frame = Some((r.w, r.h, px)),
                    _ => c.failed = true,
                }
                g.reading = None;
            }
            2 => {
                c.failed = true;
                g.reading = None;
            }
            _ => return,
        }
    }
    if !c.want || c.frame.is_some() {
        return;
    }
    let Some((t, _)) = &g.target else { return };
    let (w, h) = (t.width, t.height);
    let padded = (w * 4).div_ceil(256) * 256;
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("cc3 capture"),
        size: u64::from(padded) * u64::from(h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("cc3 capture") });
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &t.color, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(h) },
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    queue.submit([enc.finish()]);
    let done = Arc::new(std::sync::atomic::AtomicU8::new(0));
    let d2 = done.clone();
    buf.slice(..).map_async(wgpu::MapMode::Read, move |r| d2.store(if r.is_ok() { 1 } else { 2 }, Ordering::Release));
    g.reading = Some(Reading { buf, w, h, padded, done });
    c.want = false;
}

/// The camera and style of one 3D view (UI state).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View3 {
    pub cam: Cam3,
    pub style: Style3,
    /// fit the camera to the scene on the next frame
    pub fit_pending: bool,
    /// the middle and right buttons pan instead of orbiting (a plan or an elevation)
    pub lock_orbit: bool,
}

impl Default for View3 {
    fn default() -> Self {
        View3 { cam: Cam3::default(), style: Style3::default(), fit_pending: true, lock_orbit: false }
    }
}

/// Show `scene` in `rect`, handling orbit / pan / zoom. Returns the response (for the tools on top).
pub fn show(ui: &mut egui::Ui, rect: Rect, scene: &Scene3D, view: &mut View3) -> egui::Response {
    crate::viewshot::note_view_rect(ui.ctx(), rect);
    let ppp = f64::from(ui.ctx().pixels_per_point());
    let resp = ui.allocate_rect(rect, Sense::click_and_drag());
    view.cam.width = f64::from(rect.width()) * ppp;
    view.cam.height = f64::from(rect.height()) * ppp;
    if view.fit_pending && scene.min[0].is_finite() {
        view.cam.fit(scene.min, scene.max, None);
        view.fit_pending = false;
    }
    let (mods, scroll, pointer) = ui.input(|i| (i.modifiers, i.smooth_scroll_delta.y, i.pointer.hover_pos()));
    let orbiting = resp.dragged_by(egui::PointerButton::Middle) || resp.dragged_by(egui::PointerButton::Secondary);
    if orbiting {
        let d = resp.drag_delta();
        let (dx, dy) = (f64::from(d.x) * ppp, f64::from(d.y) * ppp);
        if mods.shift || view.lock_orbit {
            view.cam.pan(dx, dy);
        } else {
            view.cam.orbit(dx, dy);
        }
    }
    if resp.hovered() && scroll != 0.0 {
        let p = pointer.unwrap_or(rect.center()) - rect.min;
        let factor = (-f64::from(scroll) * 0.002).exp();
        view.cam.zoom_at(factor, f64::from(p.x) * ppp, f64::from(p.y) * ppp);
    }
    paint(ui, rect, scene, &view.cam, &view.style);
    resp
}

/// Draw `scene` into `rect` with a camera sized in physical pixels (no input handling: the caller moves the camera).
pub fn paint(ui: &egui::Ui, rect: Rect, scene: &Scene3D, cam: &Cam3, style: &Style3) {
    paint_capture(ui, rect, scene, cam, style, None);
}

/// [`paint`], reading a frame back when `capture` wants one.
pub fn paint_capture(
    ui: &egui::Ui,
    rect: Rect,
    scene: &Scene3D,
    cam: &Cam3,
    style: &Style3,
    capture: Option<Arc<std::sync::Mutex<Capture>>>,
) {
    let px = ((cam.width.round() as u32).max(1), (cam.height.round() as u32).max(1));
    let cb = Callback { scene: scene.clone(), cam: *cam, style: *style, px, capture };
    ui.painter().add(egui_wgpu::Callback::new_paint_callback(rect, cb));
}

/// A test scene: one box mesh (12 triangles, 12 edges) placed `n × n` times on a grid at `origin`, plus one plate
/// under them — a stand-in until models load (screenshots and the frame benchmark at survey coordinates).
pub fn demo_scene(n: usize, origin: [f64; 3]) -> Scene3D {
    let boxm = |w: f64, d: f64, h: f64, col: [u8; 4]| {
        let v = |i: usize| [if i & 1 == 1 { w } else { 0.0 }, if i & 2 == 2 { d } else { 0.0 }, if i & 4 == 4 { h } else { 0.0 }];
        let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
        let mut t = Vec::new();
        for q in quads {
            for tri in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
                for i in tri {
                    t.extend_from_slice(&v(i));
                }
            }
        }
        let mut e = Vec::new();
        for (a, b) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
            e.extend_from_slice(&v(a));
            e.extend_from_slice(&v(b));
        }
        MeshData::from_triangles(&t, &[col; 12], &e)
    };
    let n = n.clamp(1, 2000);
    let span = n as f64 * 2.0;
    let meshes = vec![boxm(1.2, 1.2, 1.0, [214, 196, 160, 255]), boxm(span + 2.0, span + 2.0, 0.2, [150, 160, 150, 255])];
    let mut inst = Vec::with_capacity(n * n + 1);
    inst.push(Instance3 {
        mesh: 1,
        m: cc_render::mesh3d::translation([origin[0] - 1.0, origin[1] - 1.0, origin[2] - 0.2]),
        tint: None,
        hidden: false,
    });
    for i in 0..n {
        for j in 0..n {
            let h = 1.0 + ((i * 7 + j * 3) % 5) as f64 * 0.5;
            let mut m = cc_render::mesh3d::translation([origin[0] + i as f64 * 2.0, origin[1] + j as f64 * 2.0, origin[2]]);
            m[10] = h;
            inst.push(Instance3 { mesh: 0, m, tint: None, hidden: false });
        }
    }
    Scene3D::new(
        1,
        meshes,
        inst,
        [origin[0] - 1.0, origin[1] - 1.0, origin[2] - 0.2],
        [origin[0] + span, origin[1] + span, origin[2] + 3.0],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orbit_keeps_the_distance_and_z_up() {
        let mut c = Cam3::default();
        let d0 = c.distance();
        c.orbit(120.0, -40.0);
        assert!((c.distance() - d0).abs() < 1e-9);
        let (x, _, _) = c.axes();
        assert!(x[2].abs() < 1e-12, "the right axis stays horizontal");
        c.zoom_at(0.5, 400.0, 300.0);
        assert!(c.distance() < d0);
    }
}
