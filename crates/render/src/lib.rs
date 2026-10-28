//! CadCraft L3: the 2D drawing on the GPU (wgpu; no UI toolkit).
//!
//! * [`View2D`]: the view transform (f64), shared by drawing, picking and overlays.
//! * [`SpaceMesh`]: one space of a [`cc_drawing::Drawing`] as GPU buffers — line segments as instanced
//!   anti-aliased quads, fills tessellated (even-odd) — positions f32 *relative to the space's origin* so survey
//!   coordinates (x = 500 000 m) stay steady: the view centre is subtracted in f64 on the CPU.
//! * [`Renderer`]: the two pipelines and a uniform ring with dynamic offsets (one slot per view drawn in a frame:
//!   the model, or a sheet and each of its viewports).
//!
//! Speed: geometry is uploaded once per drawing; a frame is one uniform write per view and one draw call per
//! visible batch (batches are per layer × colour × line weight, so layer toggles skip draw calls).

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

pub mod mesh3d;
pub mod points3;
pub mod pt;
pub mod still;

use std::ops::Range;

use cc_drawing::{Color, Space};
use lyon_tessellation::{BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, VertexBuffers, math::point, path::Path};
use wgpu::util::DeviceExt;

/// The view: centre (world), zoom (physical pixels per world unit), twist (radians), viewport (physical pixels).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View2D {
    pub cx: f64,
    pub cy: f64,
    pub zoom: f64,
    pub twist: f64,
    pub width: f64,
    pub height: f64,
}

impl Default for View2D {
    fn default() -> Self {
        View2D { cx: 0.0, cy: 0.0, zoom: 1.0, twist: 0.0, width: 1.0, height: 1.0 }
    }
}

/// Zoom stays inside a range where the transforms stay finite.
pub const ZOOM_MIN: f64 = 1e-9;
pub const ZOOM_MAX: f64 = 1e9;

impl View2D {
    /// World → screen pixels (origin top left, y down).
    pub fn to_screen(&self, x: f64, y: f64) -> (f64, f64) {
        let (s, c) = self.twist.sin_cos();
        let dx = x - self.cx;
        let dy = y - self.cy;
        let rx = dx * c - dy * s;
        let ry = dx * s + dy * c;
        (self.width / 2.0 + rx * self.zoom, self.height / 2.0 - ry * self.zoom)
    }
    /// Screen pixels → world.
    pub fn to_world(&self, sx: f64, sy: f64) -> (f64, f64) {
        let rx = (sx - self.width / 2.0) / self.zoom;
        let ry = (self.height / 2.0 - sy) / self.zoom;
        let (s, c) = self.twist.sin_cos();
        (self.cx + rx * c + ry * s, self.cy - rx * s + ry * c)
    }
    /// Zoom by `factor` keeping the world point under screen point (sx, sy) fixed.
    pub fn zoom_at(&mut self, sx: f64, sy: f64, factor: f64) {
        let (wx, wy) = self.to_world(sx, sy);
        let z = (self.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
        if !z.is_finite() {
            return;
        }
        self.zoom = z;
        let (nx, ny) = self.to_world(sx, sy);
        self.cx += wx - nx;
        self.cy += wy - ny;
    }
    /// Pan by a screen delta (pixels).
    pub fn pan(&mut self, dx: f64, dy: f64) {
        let (x0, y0) = self.to_world(0.0, 0.0);
        let (x1, y1) = self.to_world(dx, dy);
        self.cx -= x1 - x0;
        self.cy -= y1 - y0;
    }
    /// Fit a world rectangle with a margin (fraction of the viewport).
    pub fn fit(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64, margin: f64) {
        let w = (max_x - min_x).abs().max(1e-12);
        let h = (max_y - min_y).abs().max(1e-12);
        let avail_w = self.width * (1.0 - 2.0 * margin);
        let avail_h = self.height * (1.0 - 2.0 * margin);
        let z = (avail_w / w).min(avail_h / h);
        if z.is_finite() && z > 0.0 {
            self.zoom = z.clamp(ZOOM_MIN, ZOOM_MAX);
        }
        self.cx = (min_x + max_x) / 2.0;
        self.cy = (min_y + max_y) / 2.0;
        self.twist = 0.0;
    }
}

/// What the shader needs for one view (64 bytes, written into one uniform slot).
#[derive(Clone, Copy, Debug)]
pub struct ViewUniform {
    /// view centre relative to the mesh origin
    pub centre: [f32; 2],
    pub scale: [f32; 2],
    pub rot: [f32; 2],
    pub viewport: [f32; 2],
    pub ink: [f32; 4],
    pub line_px: f32,
    pub fill_alpha: f32,
    /// print view (dwg-view.js `setPrint`): 1 = every line and fill in `ink`, lines as wide as their weight; 2 = the
    /// drawing's colours, lines as wide as their weight on screen (CadCraft's LWDISPLAY, lwdisplay.js)
    pub print: f32,
    /// physical pixels per millimetre of paper (sheets in print view; 0 in model space: screen weights only)
    pub px_per_mm: f32,
}

impl ViewUniform {
    /// For `view` drawing a mesh whose coordinates are relative to `origin`.
    pub fn new(view: &View2D, origin: (f64, f64), ink: [f32; 4], line_px: f32) -> Self {
        let (s, c) = view.twist.sin_cos();
        ViewUniform {
            centre: [(view.cx - origin.0) as f32, (view.cy - origin.1) as f32],
            scale: [(2.0 * view.zoom / view.width) as f32, (2.0 * view.zoom / view.height) as f32],
            rot: [c as f32, s as f32],
            viewport: [view.width as f32, view.height as f32],
            ink,
            line_px,
            fill_alpha: 0.18,
            print: 0.0,
            px_per_mm: 0.0,
        }
    }
    fn bytes(&self) -> [u8; 64] {
        let mut out = [0u8; 64];
        let vals: [f32; 16] = [
            self.centre[0],
            self.centre[1],
            self.scale[0],
            self.scale[1],
            self.rot[0],
            self.rot[1],
            self.viewport[0],
            self.viewport[1],
            self.ink[0],
            self.ink[1],
            self.ink[2],
            self.ink[3],
            self.line_px,
            self.fill_alpha,
            self.print,
            self.px_per_mm,
        ];
        for (chunk, v) in out.as_chunks_mut::<4>().0.iter_mut().zip(vals) {
            chunk.copy_from_slice(&v.to_le_bytes());
        }
        out
    }
}

/// One batch's ranges in the space's buffers.
#[derive(Clone, Debug)]
pub struct BatchRange {
    /// index into `space.batches`
    pub batch: usize,
    pub lines: Range<u32>,
    pub fills: Range<u32>,
}

/// One space on the GPU.
pub struct SpaceMesh {
    pub origin: (f64, f64),
    pub batches: Vec<BatchRange>,
    lines: Option<wgpu::Buffer>,
    fills: Option<wgpu::Buffer>,
    pub segments: u64,
    pub triangles: u64,
}

impl std::fmt::Debug for SpaceMesh {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpaceMesh")
            .field("origin", &self.origin)
            .field("segments", &self.segments)
            .field("triangles", &self.triangles)
            .finish()
    }
}

const LINE_STRIDE: u64 = 20;
const FILL_STRIDE: u64 = 16;

/// A batch's colour and line weight as the vertex's four bytes: RGB, then the alpha byte = the weight in 0.05 mm
/// steps (0–127; a weight not given = 0.25 mm), + 128 for a true colour (below 128: "ink"). The shader decodes it;
/// bytes written with alpha 255 read as a true colour.
fn rgba(c: Color, lw: f64) -> [u8; 4] {
    let code = if lw.is_finite() && lw >= 0.0 { (lw / 0.05).round().clamp(0.0, 127.0) as u8 } else { 5 };
    match c {
        Color::Ink => [0, 0, 0, code],
        Color::Rgb(n) => [((n >> 16) & 0xFF) as u8, ((n >> 8) & 0xFF) as u8, (n & 0xFF) as u8, 128 + code],
    }
}

/// Geometry of one space ready to upload (no GPU needed: also used by tests and benchmarks).
#[derive(Debug, Default)]
pub struct SpaceGeometry {
    pub origin: (f64, f64),
    pub batches: Vec<BatchRange>,
    pub line_bytes: Vec<u8>,
    pub fill_bytes: Vec<u8>,
}

impl SpaceGeometry {
    pub fn segments(&self) -> u64 {
        self.line_bytes.len() as u64 / LINE_STRIDE
    }
    pub fn fill_vertices(&self) -> u64 {
        self.fill_bytes.len() as u64 / FILL_STRIDE
    }
}

/// Finite points of a flat [x0, y0, …] list, relative to (ox, oy), as f32.
fn rel_points(pts: &[f64], ox: f64, oy: f64) -> Vec<(f32, f32)> {
    pts.as_chunks::<2>()
        .0
        .iter()
        .filter(|p| p[0].is_finite() && p[1].is_finite())
        .map(|&[x, y]| ((x - ox) as f32, (y - oy) as f32))
        .collect()
}

/// Flatten a space into GPU vertex data.
pub fn build_geometry(space: &Space) -> SpaceGeometry {
    let b = &space.bounds;
    let origin = if b.is_empty() { (0.0, 0.0) } else { ((b.min_x + b.max_x) / 2.0, (b.min_y + b.max_y) / 2.0) };
    let (ox, oy) = origin;
    let mut g = SpaceGeometry { origin, ..Default::default() };
    let mut tess = FillTessellator::new();
    let fill_opts = FillOptions::default().with_fill_rule(FillRule::EvenOdd).with_tolerance(0.01);
    // the segments' bytes in one allocation (a drawing with dense linetypes has millions of two-point dashes)
    let points: usize = space.batches.iter().flat_map(|b| &b.lines).map(Vec::len).sum();
    g.line_bytes.reserve(points.saturating_div(2).saturating_mul(LINE_STRIDE as usize));
    for (bi, batch) in space.batches.iter().enumerate() {
        let col = rgba(batch.color, batch.lw);
        let l0 = g.segments();
        for pl in &batch.lines {
            // as rel_points: the finite points, each joined to the one before (a PEN_UP pair lifts the pen)
            let mut prev: Option<[f32; 2]> = None;
            for p in pl.as_chunks::<2>().0 {
                if !(p[0].is_finite() && p[1].is_finite()) {
                    if cc_drawing::is_pen_up(p[0], p[1]) {
                        prev = None;
                    }
                    continue;
                }
                let b2 = [(p[0] - ox) as f32, (p[1] - oy) as f32];
                if let Some(a) = prev {
                    let mut seg = [0u8; LINE_STRIDE as usize];
                    for (chunk, v) in seg.as_chunks_mut::<4>().0.iter_mut().zip([a[0], a[1], b2[0], b2[1]]) {
                        chunk.copy_from_slice(&v.to_le_bytes());
                    }
                    if let Some(c) = seg.get_mut(16..20) {
                        c.copy_from_slice(&col);
                    }
                    g.line_bytes.extend_from_slice(&seg);
                }
                prev = Some(b2);
            }
        }
        let l1 = g.segments();
        let f0 = g.fill_vertices();
        for fill in &batch.fills {
            let mut pb = Path::builder();
            let mut any = false;
            for ring in &fill.rings {
                let pts: Vec<(f32, f32)> = rel_points(ring, ox, oy);
                if pts.len() < 3 {
                    continue;
                }
                let mut it = pts.iter();
                if let Some(&(x, y)) = it.next() {
                    pb.begin(point(x, y));
                    for &(x, y) in it {
                        pb.line_to(point(x, y));
                    }
                    pb.end(true);
                    any = true;
                }
            }
            if !any {
                continue;
            }
            let path = pb.build();
            let mut buf: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
            let ok = tess
                .tessellate_path(&path, &fill_opts, &mut BuffersBuilder::new(&mut buf, |v: FillVertex| v.position().to_array()))
                .is_ok();
            if !ok {
                continue;
            }
            let pattern: f32 = if fill.pattern { 1.0 } else { 0.0 };
            for &i in &buf.indices {
                if let Some(p) = buf.vertices.get(i as usize) {
                    g.fill_bytes.extend_from_slice(&p[0].to_le_bytes());
                    g.fill_bytes.extend_from_slice(&p[1].to_le_bytes());
                    g.fill_bytes.extend_from_slice(&col);
                    g.fill_bytes.extend_from_slice(&pattern.to_le_bytes());
                }
            }
        }
        let f1 = g.fill_vertices();
        if l1 > l0 || f1 > f0 {
            g.batches.push(BatchRange { batch: bi, lines: l0 as u32..l1 as u32, fills: f0 as u32..f1 as u32 });
        }
    }
    g
}

impl SpaceMesh {
    pub fn upload(device: &wgpu::Device, g: &SpaceGeometry) -> SpaceMesh {
        let mk = |label: &str, bytes: &[u8]| {
            if bytes.is_empty() {
                None
            } else {
                Some(device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: bytes,
                    usage: wgpu::BufferUsages::VERTEX,
                }))
            }
        };
        SpaceMesh {
            origin: g.origin,
            batches: g.batches.clone(),
            lines: mk("cc lines", &g.line_bytes),
            fills: mk("cc fills", &g.fill_bytes),
            segments: g.segments(),
            triangles: g.fill_vertices() / 3,
        }
    }
}

/// Uniform slots per frame (the model, or a sheet plus up to this many viewports − 1).
pub const SLOTS: u64 = 256;
const SLOT: u64 = 256;

pub struct Renderer {
    line_pipeline: wgpu::RenderPipeline,
    fill_pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind: wgpu::BindGroup,
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Renderer")
    }
}

impl Renderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, msaa: u32) -> Renderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cc 2d shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cc view"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(64),
                },
                count: None,
            }],
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cc view uniforms"),
            size: SLOTS * SLOT,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cc view"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &uniforms,
                    offset: 0,
                    size: wgpu::BufferSize::new(64),
                }),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cc 2d"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let blend = Some(wgpu::BlendState::ALPHA_BLENDING);
        let target = [Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })];
        let line_attrs = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Unorm8x4];
        let fill_attrs = wgpu::vertex_attr_array![0 => Float32x2, 1 => Unorm8x4, 2 => Float32];
        let ms = wgpu::MultisampleState { count: msaa.max(1), mask: !0, alpha_to_coverage_enabled: false };
        let line_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cc lines"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_line"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: LINE_STRIDE,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &line_attrs,
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_line"),
                compilation_options: Default::default(),
                targets: &target,
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
            depth_stencil: None,
            multisample: ms,
            multiview_mask: None,
            cache: None,
        });
        let fill_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cc fills"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_fill"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: FILL_STRIDE,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &fill_attrs,
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_fill"),
                compilation_options: Default::default(),
                targets: &target,
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
            depth_stencil: None,
            multisample: ms,
            multiview_mask: None,
            cache: None,
        });
        Renderer { line_pipeline, fill_pipeline, uniforms, bind }
    }

    /// Write the view for `slot` (0..SLOTS) before the pass.
    pub fn set_view(&self, queue: &wgpu::Queue, slot: u32, u: &ViewUniform) {
        let slot = u64::from(slot).min(SLOTS - 1);
        queue.write_buffer(&self.uniforms, slot * SLOT, &u.bytes());
    }

    /// Draw `mesh` with the view in `slot`; `visible(batch index)` skips hidden layers.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, mesh: &SpaceMesh, slot: u32, visible: &dyn Fn(usize) -> bool) {
        let off = (u64::from(slot).min(SLOTS - 1) * SLOT) as u32;
        if let Some(fills) = &mesh.fills {
            pass.set_pipeline(&self.fill_pipeline);
            pass.set_bind_group(0, &self.bind, &[off]);
            pass.set_vertex_buffer(0, fills.slice(..));
            for b in &mesh.batches {
                if !b.fills.is_empty() && visible(b.batch) {
                    pass.draw(b.fills.clone(), 0..1);
                }
            }
        }
        if let Some(lines) = &mesh.lines {
            pass.set_pipeline(&self.line_pipeline);
            pass.set_bind_group(0, &self.bind, &[off]);
            pass.set_vertex_buffer(0, lines.slice(..));
            for b in &mesh.batches {
                if !b.lines.is_empty() && visible(b.batch) {
                    pass.draw(0..6, b.lines.clone());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn view_round_trip_at_survey_coordinates() {
        let mut v = View2D { cx: 500_000.0, cy: 4_000_000.0, zoom: 100.0, twist: 0.3, width: 800.0, height: 600.0 };
        let (sx, sy) = v.to_screen(500_000.123, 4_000_000.456);
        let (x, y) = v.to_world(sx, sy);
        assert!((x - 500_000.123).abs() < 1e-6 && (y - 4_000_000.456).abs() < 1e-6);
        v.zoom_at(100.0, 100.0, 2.0);
        let (x2, y2) = v.to_world(100.0, 100.0);
        let (x1, y1) = (v.cx, v.cy);
        assert!(x1.is_finite() && y1.is_finite() && x2.is_finite() && y2.is_finite());
    }
    #[test]
    fn geometry_from_space() {
        use cc_drawing::*;
        let space = Space {
            key: "model".into(),
            name: "Model".into(),
            kind: SpaceKind::Model,
            batches: vec![Batch {
                layer: "0".into(),
                color: Color::Ink,
                lw: 0.25,
                lines: vec![vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0]],
                fills: vec![Fill { rings: vec![vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0]], pattern: false }],
                texts: vec![],
            }],
            bounds: Bounds { min_x: 0.0, min_y: 0.0, max_x: 10.0, max_y: 10.0 },
            paper: None,
            paper_unit: None,
            viewports: vec![],
            images: vec![],
        };
        let g = build_geometry(&space);
        assert_eq!(g.segments(), 2);
        assert_eq!(g.fill_vertices(), 3);
        assert_eq!(g.origin, (5.0, 5.0));
    }
}

#[cfg(test)]
mod line_bytes_tests {
    use super::*;
    use cc_drawing::{Batch, Bounds, SpaceKind};

    fn batch(lines: Vec<Vec<f64>>, color: Color, lw: f64) -> Batch {
        Batch { layer: "0".into(), color, lw, lines, fills: Vec::new(), texts: Vec::new() }
    }

    /// The segments' bytes as they were made before M4.52 (a point list per polyline, then its pairs).
    fn reference(space: &Space, origin: (f64, f64)) -> Vec<u8> {
        let mut out = Vec::new();
        for batch in &space.batches {
            let col = rgba(batch.color, batch.lw);
            for pl in &batch.lines {
                let pts = rel_points(pl, origin.0, origin.1);
                for w in pts.windows(2) {
                    if let [a, b] = *w {
                        for v in [a.0, a.1, b.0, b.1] {
                            out.extend_from_slice(&v.to_le_bytes());
                        }
                        out.extend_from_slice(&col);
                    }
                }
            }
        }
        out
    }

    #[test]
    fn segments_as_before() {
        let lines = vec![
            vec![0.0, 0.0, 10.0, 5.0, 20.0, -3.5],
            vec![1.0, 1.0],
            vec![],
            vec![f64::NAN, 2.0, 3.0, 4.0, f64::INFINITY, 0.0, 5.0, 6.0, 7.0],
            vec![1e9, -1e9, 1e9 + 0.25, -1e9 + 0.5],
        ];
        let mut bounds = Bounds::default();
        bounds.grow(&[-2.0, -1.0, 30.0, 9.0]);
        let space = Space {
            key: "model".into(),
            name: "Model".into(),
            kind: SpaceKind::Model,
            batches: vec![batch(lines.clone(), Color::Ink, -3.0), batch(lines, Color::Rgb(0x12_34_56), 0.35)],
            bounds,
            paper: None,
            paper_unit: None,
            viewports: Vec::new(),
            images: Vec::new(),
        };
        let g = build_geometry(&space);
        assert_eq!(g.line_bytes, reference(&space, g.origin));
        assert_eq!(g.segments(), 2 * (2 + 1 + 1));
        // a PEN_UP pair lifts the pen: the dashes of one list draw as the separate polylines they were
        let [px, py] = cc_drawing::PEN_UP;
        let one = vec![0.0, 0.0, 1.0, 0.0, px, py, 2.0, 0.0, 3.0, 0.0, 3.0, 1.0, px, py, 5.0, 5.0, 6.0, 6.0];
        let split = vec![vec![0.0, 0.0, 1.0, 0.0], vec![2.0, 0.0, 3.0, 0.0, 3.0, 1.0], vec![5.0, 5.0, 6.0, 6.0]];
        let mut a = space.clone();
        a.batches = vec![batch(vec![one], Color::Ink, -3.0)];
        let mut b = space.clone();
        b.batches = vec![batch(split, Color::Ink, -3.0)];
        let (ga, gb) = (build_geometry(&a), build_geometry(&b));
        assert_eq!(ga.line_bytes, gb.line_bytes);
        assert_eq!(ga.segments(), 4);
    }
}
