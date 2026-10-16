//! 3D on the GPU (ModelCraft's viewport, M6): one mesh per shared definition, instances with f64 transforms, drawn
//! into an offscreen target (colour + reversed-Z depth, MSAA) on demand — the UI shows the result as a texture, and
//! headless tests and screenshots read it back.
//!
//! **Large coordinates:** a mesh's positions are f32 relative to its own origin; each instance's transform is
//! composed in f64 on the CPU relative to the scene's origin (`T(-origin) · M · T(mesh origin)`, once per scene) and
//! only then cast to f32, and the frame's matrix carries `P · R · T(origin − eye)`, also made in f64: a model at
//! x = 500 000 m, y = 4 000 000 m draws as steadily as one at the origin, and orbiting rewrites one uniform, not the
//! instances. (f32 relative to the scene origin keeps 1 mm up to about 8 km from it.)
//!
//! **Depth:** reversed Z (`Depth32Float`, near = 1, far = 0, compare Greater): precision where it matters.

use wgpu::util::DeviceExt;

/// A camera as ModelCraft's camera.js holds it (Z up), plus the drawing area in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cam3 {
    pub eye: [f64; 3],
    pub target: [f64; 3],
    /// vertical field of view, degrees
    pub fov: f64,
    pub persp: bool,
    pub width: f64,
    pub height: f64,
    pub near: f64,
    pub far: f64,
}

impl Default for Cam3 {
    fn default() -> Self {
        Cam3 {
            eye: [9.0, -12.0, 8.0],
            target: [0.0, 0.0, 1.0],
            fov: 35.0,
            persp: true,
            width: 800.0,
            height: 600.0,
            near: 0.05,
            far: 5000.0,
        }
    }
}

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn len3(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
fn norm3(a: [f64; 3]) -> [f64; 3] {
    let l = len3(a);
    if l > 0.0 { [a[0] / l, a[1] / l, a[2] / l] } else { a }
}

/// Column-major 4×4 product a · b.
pub fn mmul(a: &[f64; 16], b: &[f64; 16]) -> [f64; 16] {
    let at = |m: &[f64; 16], i: usize| m.get(i).copied().unwrap_or(0.0);
    std::array::from_fn(|i| {
        let (c, r) = (i / 4, i % 4);
        (0..4).fold(0.0, |s, k| s + at(a, k * 4 + r) * at(b, c * 4 + k))
    })
}

/// A translation, column-major.
pub fn translation(t: [f64; 3]) -> [f64; 16] {
    [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, t[0], t[1], t[2], 1.0]
}

/// The identity, column-major.
pub const IDENT: [f64; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

impl Cam3 {
    pub fn aspect(&self) -> f64 {
        self.width.max(1.0) / self.height.max(1.0)
    }
    pub fn distance(&self) -> f64 {
        len3(sub3(self.eye, self.target))
    }
    /// camera.js `axes()`: x right, y up, z towards the eye.
    pub fn axes(&self) -> ([f64; 3], [f64; 3], [f64; 3]) {
        let z = norm3(sub3(self.eye, self.target));
        let mut x = cross3([0.0, 0.0, 1.0], z);
        if len3(x) < 1e-9 {
            x = [1.0, 0.0, 0.0];
        }
        let x = norm3(x);
        (x, cross3(z, x), z)
    }
    /// The view rotation (the eye at the origin), column-major.
    pub fn view_rot(&self) -> [f64; 16] {
        let (x, y, z) = self.axes();
        [x[0], y[0], z[0], 0.0, x[1], y[1], z[1], 0.0, x[2], y[2], z[2], 0.0, 0.0, 0.0, 0.0, 1.0]
    }
    /// Reversed-Z projection for WebGPU clip space (depth 1 at near, 0 at far), column-major. The parallel view shows
    /// what the perspective view shows at the target and keeps a near plane behind the eye (camera.js does the same).
    pub fn proj(&self) -> [f64; 16] {
        let (n, f) = (self.near.max(1e-9), self.far.max(self.near + 1e-6));
        let th = (self.fov.to_radians() / 2.0).tan();
        if self.persp {
            let t = 1.0 / th;
            let a = n / (f - n);
            let b = n * f / (f - n);
            [t / self.aspect(), 0.0, 0.0, 0.0, 0.0, t, 0.0, 0.0, 0.0, 0.0, a, -1.0, 0.0, 0.0, b, 0.0]
        } else {
            let hh = self.distance() * th;
            let hw = hh * self.aspect();
            let nn = -f;
            // z_ndc = (z_view + f) / (f - nn)
            let a = 1.0 / (f - nn);
            let b = f / (f - nn);
            [1.0 / hw, 0.0, 0.0, 0.0, 0.0, 1.0 / hh, 0.0, 0.0, 0.0, 0.0, a, 0.0, 0.0, 0.0, b, 1.0]
        }
    }
    /// Metres per pixel at a point's depth (camera.js `pixelSize`).
    pub fn pixel_size(&self, p: [f64; 3]) -> f64 {
        let (_, _, z) = self.axes();
        let depth = if self.persp {
            let d = sub3(p, self.eye);
            (-(d[0] * z[0] + d[1] * z[1] + d[2] * z[2])).max(1e-6)
        } else {
            self.distance()
        };
        (2.0 * depth * (self.fov.to_radians() / 2.0).tan()) / self.height.max(1.0)
    }
    /// camera.js `orbit(dx, dy)` (pixels): about the target, Z stays up.
    pub fn orbit(&mut self, dx: f64, dy: f64) {
        let off = sub3(self.eye, self.target);
        let r = len3(off);
        if r <= 0.0 {
            return;
        }
        let yaw = off[1].atan2(off[0]) - dx * 0.008;
        let pitch = ((off[2] / r).clamp(-1.0, 1.0).acos() - dy * 0.008).clamp(0.001, std::f64::consts::PI - 0.001);
        self.eye =
            [self.target[0] + r * pitch.sin() * yaw.cos(), self.target[1] + r * pitch.sin() * yaw.sin(), self.target[2] + r * pitch.cos()];
    }
    /// camera.js `pan(dx, dy)` (pixels).
    pub fn pan(&mut self, dx: f64, dy: f64) {
        let (x, y, _) = self.axes();
        let s = self.pixel_size(self.target);
        let mv = [x[0] * -dx * s + y[0] * dy * s, x[1] * -dx * s + y[1] * dy * s, x[2] * -dx * s + y[2] * dy * s];
        self.eye = [self.eye[0] + mv[0], self.eye[1] + mv[1], self.eye[2] + mv[2]];
        self.target = [self.target[0] + mv[0], self.target[1] + mv[1], self.target[2] + mv[2]];
    }
    /// camera.js `zoomAt(factor, x, y)`: factor < 1 zooms in, keeping the point under (sx, sy) on the target's plane.
    pub fn zoom_at(&mut self, factor: f64, sx: f64, sy: f64) {
        let (o, d) = self.ray(sx, sy);
        let (_, _, z) = self.axes();
        let dn = d[0] * z[0] + d[1] * z[1] + d[2] * z[2];
        let p = if dn.abs() > 1e-12 {
            let t = ((self.target[0] - o[0]) * z[0] + (self.target[1] - o[1]) * z[1] + (self.target[2] - o[2]) * z[2]) / dn;
            if t >= 0.0 { [o[0] + d[0] * t, o[1] + d[1] * t, o[2] + d[2] * t] } else { self.target }
        } else {
            self.target
        };
        let nd = self.distance() * factor;
        if !(1e-3..=1e6).contains(&nd) {
            return;
        }
        let scale = |q: [f64; 3]| [p[0] + (q[0] - p[0]) * factor, p[1] + (q[1] - p[1]) * factor, p[2] + (q[2] - p[2]) * factor];
        if self.persp {
            self.eye = scale(self.eye);
            self.target = scale(self.target);
        } else {
            let tgt = scale(self.target);
            let along = (tgt[0] - self.target[0]) * z[0] + (tgt[1] - self.target[1]) * z[1] + (tgt[2] - self.target[2]) * z[2];
            let t2 = [tgt[0] - z[0] * along, tgt[1] - z[1] * along, tgt[2] - z[2] * along];
            self.target = t2;
            self.eye = [t2[0] + z[0] * nd, t2[1] + z[1] * nd, t2[2] + z[2] * nd];
        }
    }
    /// camera.js `ray(x, y)`: the pick ray through a pixel (origin, unit direction).
    pub fn ray(&self, sx: f64, sy: f64) -> ([f64; 3], [f64; 3]) {
        let (x, y, z) = self.axes();
        let nx = (2.0 * sx) / self.width.max(1.0) - 1.0;
        let ny = 1.0 - (2.0 * sy) / self.height.max(1.0);
        let th = (self.fov.to_radians() / 2.0).tan();
        if self.persp {
            let (a, b) = (nx * th * self.aspect(), ny * th);
            let d = norm3([x[0] * a + y[0] * b - z[0], x[1] * a + y[1] * b - z[1], x[2] * a + y[2] * b - z[2]]);
            return (self.eye, d);
        }
        let hh = self.distance() * th;
        let hw = hh * self.aspect();
        let k = self.far * 0.5;
        let o = [
            self.eye[0] + x[0] * nx * hw + y[0] * ny * hh + z[0] * k,
            self.eye[1] + x[1] * nx * hw + y[1] * ny * hh + z[1] * k,
            self.eye[2] + x[2] * nx * hw + y[2] * ny * hh + z[2] * k,
        ];
        (o, [-z[0], -z[1], -z[2]])
    }
    /// camera.js `fit(bbox, dir)`: look at a box from the current direction (or `dir`), near/far from its size.
    pub fn fit(&mut self, min: [f64; 3], max: [f64; 3], dir: Option<[f64; 3]>) {
        let d = norm3(dir.unwrap_or_else(|| sub3(self.eye, self.target)));
        let c = [(min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0, (min[2] + max[2]) / 2.0];
        let r = (len3(sub3(max, min)) / 2.0).max(0.01);
        let fov_min = self.fov.min(self.fov * self.aspect()).to_radians() / 2.0;
        let dd = (r / fov_min.sin()) * 1.08;
        self.target = c;
        self.eye = [c[0] + d[0] * dd, c[1] + d[1] * dd, c[2] + d[2] * dd];
        // camera.js clip(r)
        let dist = self.distance();
        self.far = dist + r.max(1.0) * 4.0 + 100.0;
        self.near = (self.far / 1e5).max((dist * 0.01).min(0.05));
    }
    /// The frame matrix for instances made relative to `origin`: `P · R · T(origin − eye)`, in f64, then f32.
    pub fn frame_matrix(&self, origin: [f64; 3]) -> [f32; 16] {
        let t = translation([origin[0] - self.eye[0], origin[1] - self.eye[1], origin[2] - self.eye[2]]);
        mmul(&self.proj(), &mmul(&self.view_rot(), &t)).map(|v| v as f32)
    }
}

/// A mesh as the CPU builds it: positions relative to `origin`, one normal and colour per vertex, triangles, edges.
#[derive(Clone, Debug, Default)]
pub struct MeshData {
    pub origin: [f64; 3],
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[u8; 4]>,
    pub indices: Vec<u32>,
    /// edge segments: pairs of points relative to `origin`
    pub edges: Vec<[f32; 3]>,
}

impl MeshData {
    /// From world triangles (f64, 9 per triangle) with one colour per triangle and edge segments (6 per segment):
    /// the origin is the box centre, normals are per face.
    pub fn from_triangles(tris: &[f64], colors: &[[u8; 4]], edges: &[f64]) -> MeshData {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in tris.as_chunks::<3>().0.iter().chain(edges.as_chunks::<3>().0) {
            for ((l, h), v) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
                *l = l.min(*v);
                *h = h.max(*v);
            }
        }
        let origin = if lo[0].is_finite() { [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0] } else { [0.0; 3] };
        let rel = |p: &[f64; 3]| [(p[0] - origin[0]) as f32, (p[1] - origin[1]) as f32, (p[2] - origin[2]) as f32];
        let mut m = MeshData { origin, ..MeshData::default() };
        for (ti, t) in tris.as_chunks::<9>().0.iter().enumerate() {
            let (a, b, c) = ([t[0], t[1], t[2]], [t[3], t[4], t[5]], [t[6], t[7], t[8]]);
            let n = norm3(cross3(sub3(b, a), sub3(c, a)));
            let col = colors.get(ti).copied().unwrap_or([200, 200, 200, 255]);
            for p in [a, b, c] {
                m.indices.push(m.positions.len() as u32);
                m.positions.push(rel(&p));
                m.normals.push(n.map(|v| v as f32));
                m.colors.push(col);
            }
        }
        m.edges = edges.as_chunks::<3>().0.iter().map(rel).collect();
        m
    }
}

const VSTRIDE: u64 = 28; // pos 12 + normal 12 + colour 4
const ISTRIDE: u64 = 128; // m 64 + normal matrix 48 + tint 16
const FRAME: u64 = 128;

/// A mesh on the GPU.
#[derive(Debug)]
pub struct GpuMesh {
    pub origin: [f64; 3],
    verts: Option<wgpu::Buffer>,
    idx: Option<wgpu::Buffer>,
    edges: Option<wgpu::Buffer>,
    pub triangles: u32,
    pub segments: u32,
}

impl GpuMesh {
    pub fn upload(device: &wgpu::Device, m: &MeshData) -> GpuMesh {
        let mut vb = Vec::with_capacity(m.positions.len() * VSTRIDE as usize);
        for (i, p) in m.positions.iter().enumerate() {
            let n = m.normals.get(i).copied().unwrap_or([0.0, 0.0, 1.0]);
            let c = m.colors.get(i).copied().unwrap_or([200, 200, 200, 255]);
            for v in p.iter().chain(n.iter()) {
                vb.extend_from_slice(&v.to_le_bytes());
            }
            vb.extend_from_slice(&c);
        }
        let ib: Vec<u8> = m.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let eb: Vec<u8> = m.edges.iter().flat_map(|p| p.iter().flat_map(|v| v.to_le_bytes())).collect();
        let mk = |label: &str, bytes: &[u8], usage: wgpu::BufferUsages| {
            (!bytes.is_empty())
                .then(|| device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: bytes, usage }))
        };
        GpuMesh {
            origin: m.origin,
            verts: mk("cc3 verts", &vb, wgpu::BufferUsages::VERTEX),
            idx: mk("cc3 idx", &ib, wgpu::BufferUsages::INDEX),
            edges: mk("cc3 edges", &eb, wgpu::BufferUsages::VERTEX),
            triangles: u32::try_from(m.indices.len() / 3).unwrap_or(u32::MAX),
            segments: u32::try_from(m.edges.len() / 2).unwrap_or(u32::MAX),
        }
    }
}

/// One placement of a mesh: a column-major f64 transform and an optional colour painted on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Instance3 {
    pub mesh: usize,
    pub m: [f64; 16],
    pub tint: Option<[u8; 4]>,
}

/// How faces and edges are drawn (ModelCraft's styles: shaded, hidden line, wireframe, x-ray).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style3 {
    pub faces: bool,
    pub edges: bool,
    /// faces drawn see-through (x-ray)
    pub xray: bool,
    /// faces drawn in the background colour (hidden line)
    pub hidden_line: bool,
    pub background: [f32; 4],
    pub edge: [f32; 4],
    pub back: [f32; 3],
}

impl Default for Style3 {
    fn default() -> Self {
        Style3 {
            faces: true,
            edges: true,
            xray: false,
            hidden_line: false,
            background: [0.93, 0.94, 0.95, 1.0],
            edge: [0.08, 0.09, 0.1, 1.0],
            back: [0.62, 0.66, 0.74],
        }
    }
}

/// The offscreen target: MSAA colour + resolve + reversed-Z depth.
#[derive(Debug)]
pub struct Target3 {
    pub width: u32,
    pub height: u32,
    msaa: wgpu::TextureView,
    pub color: wgpu::Texture,
    pub view: wgpu::TextureView,
    depth: wgpu::TextureView,
}

/// The colour format of [`Target3`].
pub const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const SAMPLES: u32 = 4;

impl Target3 {
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Target3 {
        let (width, height) = (width.clamp(1, 8192), height.clamp(1, 8192));
        let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let tex = |label: &str, format: wgpu::TextureFormat, samples: u32, usage: wgpu::TextureUsages| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let msaa = tex("cc3 msaa", TARGET_FORMAT, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let color = tex(
            "cc3 color",
            TARGET_FORMAT,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        );
        let depth = tex("cc3 depth", wgpu::TextureFormat::Depth32Float, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let view = color.create_view(&wgpu::TextureViewDescriptor::default());
        Target3 {
            width,
            height,
            msaa: msaa.create_view(&wgpu::TextureViewDescriptor::default()),
            view,
            color,
            depth: depth.create_view(&wgpu::TextureViewDescriptor::default()),
        }
    }
}

/// The 3D pipelines and the per-frame uniform.
pub struct Renderer3 {
    face: wgpu::RenderPipeline,
    face_xray: wgpu::RenderPipeline,
    edge: wgpu::RenderPipeline,
    frame: wgpu::Buffer,
    bind: wgpu::BindGroup,
}

impl std::fmt::Debug for Renderer3 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Renderer3")
    }
}

/// Placements on the GPU: every instance's transform relative to the scene's origin, grouped by mesh (made once per
/// scene; orbiting does not touch it).
#[derive(Debug)]
pub struct InstanceSet {
    pub origin: [f64; 3],
    buf: Option<wgpu::Buffer>,
    ranges: Vec<(usize, std::ops::Range<u32>)>,
    pub count: u32,
}

/// `T(-origin) · m · T(mesh origin)` in f64, then f32.
pub fn relative(m: &[f64; 16], origin: [f64; 3], mesh_origin: [f64; 3]) -> [f32; 16] {
    mmul(&translation([-origin[0], -origin[1], -origin[2]]), &mmul(m, &translation(mesh_origin))).map(|v| v as f32)
}

impl InstanceSet {
    /// Upload `instances` of `meshes`, relative to `origin` (the scene's box centre is a good one).
    pub fn new(device: &wgpu::Device, meshes: &[GpuMesh], instances: &[Instance3], origin: [f64; 3]) -> InstanceSet {
        let mut order: Vec<&Instance3> = instances.iter().filter(|i| i.mesh < meshes.len()).collect();
        order.sort_by_key(|i| i.mesh);
        let mut ib: Vec<u8> = Vec::with_capacity(order.len() * ISTRIDE as usize);
        let mut ranges: Vec<(usize, std::ops::Range<u32>)> = Vec::new();
        for (k, inst) in order.iter().enumerate() {
            let Some(mesh) = meshes.get(inst.mesh) else { continue };
            let m = relative(&inst.m, origin, mesh.origin);
            let nm = normal_matrix(&inst.m);
            for v in m.iter().chain(nm.iter()) {
                ib.extend_from_slice(&v.to_le_bytes());
            }
            let tint = inst.tint.map_or([0.0; 4], |c| c.map(|x| f32::from(x) / 255.0));
            for v in tint {
                ib.extend_from_slice(&v.to_le_bytes());
            }
            let k = k as u32;
            match ranges.last_mut() {
                Some((mi, r)) if *mi == inst.mesh => r.end = k + 1,
                _ => ranges.push((inst.mesh, k..k + 1)),
            }
        }
        let buf = (!ib.is_empty()).then(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("cc3 instances"),
                contents: &ib,
                usage: wgpu::BufferUsages::VERTEX,
            })
        });
        InstanceSet { origin, buf, ranges, count: u32::try_from(order.len()).unwrap_or(u32::MAX) }
    }
}

/// What to draw: the meshes and their placements.
#[derive(Clone, Copy, Debug)]
pub struct Scene3<'a> {
    pub meshes: &'a [GpuMesh],
    pub instances: &'a InstanceSet,
}

/// What one frame drew.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats3 {
    pub instances: u32,
    pub triangles: u64,
    pub segments: u64,
    pub draw_calls: u32,
}

impl Renderer3 {
    pub fn new(device: &wgpu::Device) -> Renderer3 {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cc 3d shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader3d.wgsl").into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cc3 frame"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(FRAME),
                },
                count: None,
            }],
        });
        let frame = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cc3 frame"),
            size: FRAME,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cc3 frame"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: frame.as_entire_binding() }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cc3"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let inst_attrs = wgpu::vertex_attr_array![
            3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4,
            7 => Float32x4, 8 => Float32x4, 9 => Float32x4, 10 => Float32x4
        ];
        let vert_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4];
        let edge_attrs = wgpu::vertex_attr_array![0 => Float32x3];
        let ms = wgpu::MultisampleState { count: SAMPLES, mask: !0, alpha_to_coverage_enabled: false };
        let depth = |write: bool, cmp: wgpu::CompareFunction| {
            Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(write),
                depth_compare: Some(cmp),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            })
        };
        let inst = wgpu::VertexBufferLayout { array_stride: ISTRIDE, step_mode: wgpu::VertexStepMode::Instance, attributes: &inst_attrs };
        let face_pipe = |label: &str, blend: Option<wgpu::BlendState>, write: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_face"),
                    compilation_options: Default::default(),
                    buffers: &[
                        Some(wgpu::VertexBufferLayout {
                            array_stride: VSTRIDE,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &vert_attrs,
                        }),
                        Some(inst.clone()),
                    ],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_face"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: TARGET_FORMAT, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
                depth_stencil: depth(write, wgpu::CompareFunction::Greater),
                multisample: ms,
                multiview_mask: None,
                cache: None,
            })
        };
        let face = face_pipe("cc3 faces", None, true);
        let face_xray = face_pipe("cc3 faces x-ray", Some(wgpu::BlendState::ALPHA_BLENDING), false);
        let edge = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cc3 edges"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_edge"),
                compilation_options: Default::default(),
                buffers: &[
                    Some(wgpu::VertexBufferLayout { array_stride: 12, step_mode: wgpu::VertexStepMode::Vertex, attributes: &edge_attrs }),
                    Some(inst.clone()),
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_edge"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: TARGET_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::LineList, ..Default::default() },
            depth_stencil: depth(true, wgpu::CompareFunction::GreaterEqual),
            multisample: ms,
            multiview_mask: None,
            cache: None,
        });
        Renderer3 { face, face_xray, edge, frame, bind }
    }

    /// Draw `instances` of `meshes` seen by `cam` into `target`; returns what was drawn. The camera's size should be
    /// the target's.
    pub fn render(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &Target3,
        scene: Scene3<'_>,
        cam: &Cam3,
        style: &Style3,
    ) -> Stats3 {
        let meshes = scene.meshes;
        let set = scene.instances;
        let pv = cam.frame_matrix(set.origin);
        let shade = if style.hidden_line { [1.0, 0.0, 1.0, 0.0] } else { [0.45, 0.6, if style.xray { 0.45 } else { 1.0 }, 0.0] };
        // edges win over their faces by a depth nudge that is a fixed fraction of the depth range near the eye
        let nudge = if cam.persp { 2e-6 } else { 1e-5 };
        let mut fb: Vec<f32> = pv.to_vec();
        fb.extend_from_slice(&[0.35, -0.45, 0.82, 0.0]);
        fb.extend_from_slice(&[style.back[0], style.back[1], style.back[2], 0.25]);
        fb.extend_from_slice(&style.edge);
        fb.extend_from_slice(&[shade[0], shade[1], shade[2], nudge]);
        let bytes: Vec<u8> = fb.iter().flat_map(|v| v.to_le_bytes()).collect();
        queue.write_buffer(&self.frame, 0, &bytes);
        let (inst_buf, ranges) = (&set.buf, &set.ranges);

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("cc3") });
        let mut stats = Stats3 { instances: set.count, ..Stats3::default() };
        {
            let bg = style.background;
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cc3"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.msaa,
                    depth_slice: None,
                    resolve_target: Some(&target.view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(bg[0]),
                            g: f64::from(bg[1]),
                            b: f64::from(bg[2]),
                            a: f64::from(bg[3]),
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(inst_buf) = &inst_buf {
                pass.set_bind_group(0, &self.bind, &[]);
                pass.set_vertex_buffer(1, inst_buf.slice(..));
                if style.faces {
                    pass.set_pipeline(if style.xray { &self.face_xray } else { &self.face });
                    for (mi, r) in ranges {
                        let Some(mesh) = meshes.get(*mi) else { continue };
                        if let (Some(v), Some(i)) = (&mesh.verts, &mesh.idx) {
                            pass.set_vertex_buffer(0, v.slice(..));
                            pass.set_index_buffer(i.slice(..), wgpu::IndexFormat::Uint32);
                            pass.draw_indexed(0..mesh.triangles * 3, 0, r.clone());
                            stats.draw_calls += 1;
                            stats.triangles += u64::from(mesh.triangles) * u64::from(r.end - r.start);
                        }
                    }
                }
                if style.edges {
                    pass.set_pipeline(&self.edge);
                    for (mi, r) in ranges {
                        let Some(mesh) = meshes.get(*mi) else { continue };
                        if let Some(e) = &mesh.edges {
                            pass.set_vertex_buffer(0, e.slice(..));
                            pass.draw(0..mesh.segments * 2, r.clone());
                            stats.draw_calls += 1;
                            stats.segments += u64::from(mesh.segments) * u64::from(r.end - r.start);
                        }
                    }
                }
            }
        }
        queue.submit([enc.finish()]);
        stats
    }
}

/// Draws a [`Target3`] onto another pass (the UI's): a full-viewport triangle sampling the target; the caller sets
/// the viewport to where the 3D view goes.
pub struct Blit {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl std::fmt::Debug for Blit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Blit")
    }
}

impl Blit {
    /// For a pass of `format` with `msaa` samples.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, msaa: u32) -> Blit {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cc3 blit"),
            source: wgpu::ShaderSource::Wgsl(include_str!("blit.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cc3 blit"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cc3 blit"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cc3 blit"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState { count: msaa.max(1), mask: !0, alpha_to_coverage_enabled: false },
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("cc3 blit"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Blit { pipeline, layout, sampler }
    }

    /// The bind group for a target (make it again when the target is re-made).
    pub fn bind(&self, device: &wgpu::Device, target: &Target3) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cc3 blit"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&target.view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        })
    }

    /// Draw the bound target over the pass's current viewport.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, bind: &wgpu::BindGroup) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// The inverse transpose of a transform's 3×3 part (normals), as three padded columns.
fn normal_matrix(m: &[f64; 16]) -> [f32; 12] {
    let a = [m[0], m[1], m[2], m[4], m[5], m[6], m[8], m[9], m[10]];
    // cofactor matrix = det · inverse transpose
    let c = [
        a[4] * a[8] - a[5] * a[7],
        a[5] * a[6] - a[3] * a[8],
        a[3] * a[7] - a[4] * a[6],
        a[2] * a[7] - a[1] * a[8],
        a[0] * a[8] - a[2] * a[6],
        a[1] * a[6] - a[0] * a[7],
        a[1] * a[5] - a[2] * a[4],
        a[2] * a[3] - a[0] * a[5],
        a[0] * a[4] - a[1] * a[3],
    ];
    let det = a[0] * c[0] + a[1] * c[1] + a[2] * c[2];
    let s = if det < 0.0 { -1.0 } else { 1.0 };
    // columns of the cofactor matrix (a mirrored instance keeps outward normals)
    [
        (s * c[0]) as f32,
        (s * c[1]) as f32,
        (s * c[2]) as f32,
        0.0,
        (s * c[3]) as f32,
        (s * c[4]) as f32,
        (s * c[5]) as f32,
        0.0,
        (s * c[6]) as f32,
        (s * c[7]) as f32,
        (s * c[8]) as f32,
        0.0,
    ]
}

/// Read a target's colour back as tightly packed RGBA rows (headless tests, screenshots, benchmarks).
pub fn read_back(device: &wgpu::Device, queue: &wgpu::Queue, target: &Target3) -> Result<Vec<u8>, String> {
    let (w, h) = (target.width, target.height);
    let row = w * 4;
    let padded = row.div_ceil(256) * 256;
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("cc3 read back"),
        size: u64::from(padded) * u64::from(h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("cc3 read back") });
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &target.color, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(h) },
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    queue.submit([enc.finish()]);
    let slice = buf.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
    let data = slice.get_mapped_range().map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity((row * h) as usize);
    for y in 0..h {
        let start = (y * padded) as usize;
        out.extend_from_slice(data.get(start..start + row as usize).ok_or("short read back")?);
    }
    drop(data);
    buf.unmap();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit cube's 12 triangles (CCW from outside) and 12 edges, at `o`.
    fn cube(o: [f64; 3]) -> (Vec<f64>, Vec<f64>) {
        let v = |i: usize| [o[0] + (i & 1) as f64, o[1] + ((i >> 1) & 1) as f64, o[2] + ((i >> 2) & 1) as f64];
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
        (t, e)
    }

    #[test]
    fn projection_is_reversed_z() {
        let c = Cam3 { eye: [0.0, -10.0, 0.0], target: [0.0, 0.0, 0.0], near: 1.0, far: 100.0, ..Cam3::default() };
        let pv = mmul(&c.proj(), &c.view_rot());
        let depth = |y: f64| {
            let p = [0.0, y + 10.0, 0.0]; // camera-relative: the eye at the origin
            let z = pv[2] * p[0] + pv[6] * p[1] + pv[10] * p[2] + pv[14];
            let w = pv[3] * p[0] + pv[7] * p[1] + pv[11] * p[2] + pv[15];
            z / w
        };
        assert!((depth(-9.0) - 1.0).abs() < 1e-9, "near = 1");
        assert!(depth(90.0).abs() < 1e-9, "far = 0");
        assert!(depth(0.0) > depth(10.0), "nearer is larger");
    }

    #[test]
    fn survey_coordinates_stay_steady() {
        // the same cube near the origin and at x = 500 000, y = 4 000 000 gives the same camera-relative transform
        let far = [500_000.0, 4_000_000.0, 120.0];
        let c0 = Cam3 { eye: [3.0, -4.0, 2.0], target: [0.5, 0.5, 0.5], ..Cam3::default() };
        let c1 = Cam3 { eye: [far[0] + 3.0, far[1] - 4.0, far[2] + 2.0], target: [far[0] + 0.5, far[1] + 0.5, far[2] + 0.5], ..c0 };
        let m0 = relative(&IDENT, [0.5, 0.5, 0.5], [0.0; 3]);
        let m1 = relative(&IDENT, [far[0] + 0.5, far[1] + 0.5, far[2] + 0.5], far);
        let f0 = c0.frame_matrix([0.5, 0.5, 0.5]);
        let f1 = c1.frame_matrix([far[0] + 0.5, far[1] + 0.5, far[2] + 0.5]);
        for (a, b) in m0.iter().zip(&m1).chain(f0.iter().zip(&f1)) {
            assert!((a - b).abs() < 1e-5, "{m0:?} vs {m1:?}, {f0:?} vs {f1:?}");
        }
    }

    fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let inst = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(inst.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
    }

    #[test]
    fn draws_a_cube_headless() {
        let Some((device, queue)) = device() else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        let (t, e) = cube([500_000.0, 4_000_000.0, 0.0]);
        let colors = vec![[200u8, 60, 40, 255]; t.len() / 9];
        let mesh = GpuMesh::upload(&device, &MeshData::from_triangles(&t, &colors, &e));
        let target = Target3::new(&device, 64, 64);
        let c = [500_000.5, 4_000_000.5, 0.5];
        let cam = Cam3 {
            eye: [c[0] + 3.0, c[1] - 4.0, c[2] + 2.5],
            target: c,
            width: 64.0,
            height: 64.0,
            near: 0.1,
            far: 100.0,
            ..Cam3::default()
        };
        let r = Renderer3::new(&device);
        let meshes = [mesh];
        let set = InstanceSet::new(&device, &meshes, &[Instance3 { mesh: 0, m: IDENT, tint: None }], c);
        let st = r.render(&device, &queue, &target, Scene3 { meshes: &meshes, instances: &set }, &cam, &Style3::default());
        assert_eq!(st.triangles, 12);
        assert_eq!(st.segments, 12);
        let px = read_back(&device, &queue, &target).unwrap();
        if let Ok(dir) = std::env::var("CC_RENDER_DUMP") {
            // a PPM to look at (CC_RENDER_DUMP=<dir>)
            let mut ppm = b"P6\n64 64\n255\n".to_vec();
            ppm.extend(px.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]));
            std::fs::write(std::path::Path::new(&dir).join("cube.ppm"), ppm).unwrap();
        }
        let at = |x: usize, y: usize| &px[(y * 64 + x) * 4..(y * 64 + x) * 4 + 3];
        let centre = at(32, 32);
        assert!(centre[0] > centre[2] + 20, "the cube's red faces in the middle: {centre:?}");
        let corner = at(1, 1);
        assert!(corner[0] > 200 && corner[2] > 200, "background in the corner: {corner:?}");
    }
}
