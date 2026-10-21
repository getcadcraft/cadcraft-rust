//! Rendered stills, High quality (0.5 render/render-pt.js): the scene baked to world triangles (cut by the section on
//! the CPU, as `bakeItems`), a box tree built over them (binned surface-area splits, iterative), and a path tracer in a
//! compute shader adding one sample a pixel per call — light bouncing between surfaces, glass that lets the light and
//! the view through (thin glass: it does not bend), the sun and the lamps sampled with shadow rays, the gradient dome
//! as the light of what escapes. The sum per pixel is divided, tone-mapped (ACES filmic) and encoded once, so a stop
//! keeps a picture. Storage buffers and a compute shader: WebGPU core.

use wgpu::util::DeviceExt;

use crate::mesh3d::{Cam3, IDENT, Instance3, MeshData, TEX_LAYERS, TEX_SIZE, invert, mmul, translation};
use crate::still::{StillJob, basis};

/// Triangles past this are left out (the GPU's storage buffers have limits; 160 bytes a triangle).
pub const MAX_TRIS: usize = 700_000;

/// The baked scene: per triangle three corners, three normals with u, the corners' v, albedo + rough, metal glass
/// layer — relative to `origin`.
#[derive(Clone, Debug, Default)]
pub struct PtScene {
    pub origin: [f64; 3],
    pub pos: Vec<[f32; 4]>,
    pub attr: Vec<[f32; 4]>,
    pub nodes: Vec<[f32; 4]>,
    pub dropped: usize,
}

#[derive(Clone, Copy)]
struct V {
    p: [f64; 3],
    n: [f64; 3],
    uv: [f64; 2],
}

fn lerp(a: &V, b: &V, f: f64) -> V {
    let l3 = |x: [f64; 3], y: [f64; 3]| [x[0] + (y[0] - x[0]) * f, x[1] + (y[1] - x[1]) * f, x[2] + (y[2] - x[2]) * f];
    V { p: l3(a.p, b.p), n: l3(a.n, b.n), uv: [a.uv[0] + (b.uv[0] - a.uv[0]) * f, a.uv[1] + (b.uv[1] - a.uv[1]) * f] }
}

fn srgb1(v: u8) -> f32 {
    let x = f32::from(v) / 255.0;
    if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
}

impl PtScene {
    /// Every placed mesh baked to world triangles (relative to `origin`), cut by `clip` (keep n · p ≤ d), plus the
    /// ground disc (`ground`: centre, radius, linear colour), then the box tree.
    pub fn build(
        meshes: &[MeshData],
        looks: &[Vec<[u8; 4]>],
        instances: &[Instance3],
        origin: [f64; 3],
        clip: Option<([f64; 3], f64)>,
        ground: Option<([f64; 3], f64, [f32; 3])>,
    ) -> PtScene {
        let mut tris: Vec<([V; 3], [f32; 4], [f32; 4])> = Vec::new();
        let mut dropped = 0usize;
        for inst in instances.iter().filter(|i| !i.hidden) {
            let (Some(m), Some(lk)) = (meshes.get(inst.mesh), looks.get(inst.mesh)) else { continue };
            let full = mmul(&inst.m, &translation(m.origin));
            let nm = crate::mesh3d::normal_matrix_f64(&inst.m);
            let at = |i: u32| -> Option<(V, [f32; 4], [f32; 4])> {
                let i = i as usize;
                let p = m.positions.get(i)?;
                let n = m.normals.get(i).copied().unwrap_or([0.0, 0.0, 1.0]);
                let c = m.colors.get(i).copied().unwrap_or([200, 200, 200, 255]);
                let t = m.tex.get(i).copied().unwrap_or([0.0, 0.0, -1.0]);
                let l = lk.get(i).copied().unwrap_or([199, 0, 0, 0]);
                let pp = [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])];
                let w = [
                    full[0] * pp[0] + full[4] * pp[1] + full[8] * pp[2] + full[12] - origin[0],
                    full[1] * pp[0] + full[5] * pp[1] + full[9] * pp[2] + full[13] - origin[1],
                    full[2] * pp[0] + full[6] * pp[1] + full[10] * pp[2] + full[14] - origin[2],
                ];
                let nn = [f64::from(n[0]), f64::from(n[1]), f64::from(n[2])];
                let wn = [
                    nm[0] * nn[0] + nm[3] * nn[1] + nm[6] * nn[2],
                    nm[1] * nn[0] + nm[4] * nn[1] + nm[7] * nn[2],
                    nm[2] * nn[0] + nm[5] * nn[1] + nm[8] * nn[2],
                ];
                let col = [srgb1(c[0]), srgb1(c[1]), srgb1(c[2]), f32::from(l[0]) / 255.0];
                let mat = [f32::from(l[1]) / 255.0, f32::from(l[2]) / 255.0, t[2], 0.0];
                Some((V { p: w, n: wn, uv: [f64::from(t[0]), f64::from(t[1])] }, col, mat))
            };
            for t in m.indices.as_chunks::<3>().0 {
                let (Some(a), Some(b), Some(c)) = (at(t[0]), at(t[1]), at(t[2])) else { continue };
                let mut poly = vec![a.0, b.0, c.0];
                if let Some((n, d)) = clip {
                    // keep n · (p + origin) ≤ d
                    let w = d - (n[0] * origin[0] + n[1] * origin[1] + n[2] * origin[2]);
                    let side = |q: &V| n[0] * q.p[0] + n[1] * q.p[1] + n[2] * q.p[2] - w;
                    let ds: Vec<f64> = poly.iter().map(side).collect();
                    if ds.iter().any(|x| *x > 0.0) {
                        let mut next = Vec::new();
                        for i in 0..poly.len() {
                            let j = (i + 1) % poly.len();
                            let (Some(pa), Some(pb), Some(da), Some(db)) = (poly.get(i), poly.get(j), ds.get(i), ds.get(j)) else {
                                continue;
                            };
                            if *da <= 0.0 {
                                next.push(*pa);
                            }
                            if (*da <= 0.0) != (*db <= 0.0) {
                                next.push(lerp(pa, pb, da / (da - db)));
                            }
                        }
                        poly = next;
                    }
                }
                for i in 1..poly.len().saturating_sub(1) {
                    let (Some(p0), Some(p1), Some(p2)) = (poly.first(), poly.get(i), poly.get(i + 1)) else { continue };
                    if tris.len() >= MAX_TRIS {
                        dropped += 1;
                        continue;
                    }
                    tris.push(([*p0, *p1, *p2], a.1, a.2));
                }
            }
        }
        if let Some((c, r, col)) = ground {
            let seg = 96;
            let up = V { p: c, n: [0.0, 0.0, 1.0], uv: [0.0, 0.0] };
            for k in 0..seg {
                let a0 = k as f64 / seg as f64 * std::f64::consts::TAU;
                let a1 = (k + 1) as f64 / seg as f64 * std::f64::consts::TAU;
                let q = |a: f64| V { p: [c[0] + r * a.cos(), c[1] + r * a.sin(), c[2]], n: [0.0, 0.0, 1.0], uv: [0.0, 0.0] };
                tris.push(([up, q(a0), q(a1)], [col[0], col[1], col[2], 1.0], [0.0, 0.0, -1.0, 0.0]));
            }
        }
        let mut s = PtScene { origin, dropped, ..PtScene::default() };
        let order = bvh(&tris, &mut s.nodes);
        s.pos.reserve(order.len() * 3);
        s.attr.reserve(order.len() * 6);
        for i in order {
            let Some((v, col, mat)) = tris.get(i) else { continue };
            for q in v {
                s.pos.push([q.p[0] as f32, q.p[1] as f32, q.p[2] as f32, 0.0]);
            }
            for q in v {
                s.attr.push([q.n[0] as f32, q.n[1] as f32, q.n[2] as f32, q.uv[0] as f32]);
            }
            s.attr.push([v[0].uv[1] as f32, v[1].uv[1] as f32, v[2].uv[1] as f32, 0.0]);
            s.attr.push(*col);
            s.attr.push(*mat);
        }
        if s.nodes.is_empty() {
            // an empty tree: one leaf that holds nothing
            s.nodes.push([f32::MAX, f32::MAX, f32::MAX, 0.0]);
            s.nodes.push([f32::MIN, f32::MIN, f32::MIN, f32::from_bits(0)]);
        }
        s
    }
}

/// Widen the box (lo, hi) by the box (a, b).
fn grow(lo: &mut [f64; 3], hi: &mut [f64; 3], a: [f64; 3], b: [f64; 3]) {
    for (((l, h), x), y) in lo.iter_mut().zip(hi.iter_mut()).zip(a).zip(b) {
        *l = l.min(x);
        *h = h.max(y);
    }
}

/// A box tree over the triangles' boxes: binned SAH (12 bins), at most 4 triangles a leaf, built with an explicit
/// stack. Nodes as pairs of vec4: (min, first child | first triangle as bits), (max, triangle count as bits; 0 = inner).
/// Returns the triangles' order (leaves hold runs of it).
fn bvh(tris: &[([V; 3], [f32; 4], [f32; 4])], nodes: &mut Vec<[f32; 4]>) -> Vec<usize> {
    let n = tris.len();
    let mut idx: Vec<usize> = (0..n).collect();
    if n == 0 {
        return idx;
    }
    let bbox = |v: &[V; 3]| {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for q in v {
            grow(&mut lo, &mut hi, q.p, q.p);
        }
        (lo, hi)
    };
    let boxes: Vec<([f64; 3], [f64; 3])> = tris.iter().map(|t| bbox(&t.0)).collect();
    let cent: Vec<[f64; 3]> = boxes.iter().map(|(lo, hi)| [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0]).collect();
    // node slots: (start, end) of idx; children allocated in pairs
    nodes.clear();
    nodes.resize(2, [0.0; 4]);
    let mut stack: Vec<(usize, usize, usize)> = vec![(0, 0, n)];
    let area = |lo: [f64; 3], hi: [f64; 3]| {
        let d = [(hi[0] - lo[0]).max(0.0), (hi[1] - lo[1]).max(0.0), (hi[2] - lo[2]).max(0.0)];
        d[0] * d[1] + d[1] * d[2] + d[2] * d[0]
    };
    while let Some((node, s, e)) = stack.pop() {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        let mut clo = [f64::INFINITY; 3];
        let mut chi = [f64::NEG_INFINITY; 3];
        for &i in idx.get(s..e).unwrap_or(&[]) {
            let (Some((bl, bh)), Some(c)) = (boxes.get(i), cent.get(i)) else { continue };
            grow(&mut lo, &mut hi, *bl, *bh);
            grow(&mut clo, &mut chi, *c, *c);
        }
        let set = |nodes: &mut Vec<[f32; 4]>, a: u32, b: u32| {
            if let Some(x) = nodes.get_mut(node * 2) {
                *x = [lo[0] as f32, lo[1] as f32, lo[2] as f32, f32::from_bits(a)];
            }
            if let Some(x) = nodes.get_mut(node * 2 + 1) {
                *x = [hi[0] as f32, hi[1] as f32, hi[2] as f32, f32::from_bits(b)];
            }
        };
        let count = e - s;
        // the best split by binned SAH on the widest centroid axis
        let ext = [chi[0] - clo[0], chi[1] - clo[1], chi[2] - clo[2]];
        let ax = if ext[0] >= ext[1] && ext[0] >= ext[2] {
            0
        } else if ext[1] >= ext[2] {
            1
        } else {
            2
        };
        let width = ext.get(ax).copied().unwrap_or(0.0);
        if count <= 4 || width.is_nan() || width <= 1e-12 || nodes.len() > 8 * n + 64 {
            set(nodes, s as u32, count as u32);
            continue;
        }
        const BINS: usize = 12;
        let mut bins = [(0usize, [f64::INFINITY; 3], [f64::NEG_INFINITY; 3]); BINS];
        let c0 = clo.get(ax).copied().unwrap_or(0.0);
        let bin_of = |c: f64| (((c - c0) / width * BINS as f64) as usize).min(BINS - 1);
        for &i in idx.get(s..e).unwrap_or(&[]) {
            let (Some((bl, bh)), Some(c)) = (boxes.get(i), cent.get(i)) else { continue };
            let Some(b) = bins.get_mut(bin_of(c.get(ax).copied().unwrap_or(0.0))) else { continue };
            b.0 += 1;
            grow(&mut b.1, &mut b.2, *bl, *bh);
        }
        let mut best = (f64::INFINITY, BINS / 2);
        for split in 1..BINS {
            let (mut nl, mut nr) = (0usize, 0usize);
            let (mut l0, mut l1, mut r0, mut r1) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3], [f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
            for (k, b) in bins.iter().enumerate() {
                if b.0 == 0 {
                    continue;
                }
                let (cnt, lo_, hi_) = if k < split { (&mut nl, &mut l0, &mut l1) } else { (&mut nr, &mut r0, &mut r1) };
                *cnt += b.0;
                grow(lo_, hi_, b.1, b.2);
            }
            if nl == 0 || nr == 0 {
                continue;
            }
            let cost = nl as f64 * area(l0, l1) + nr as f64 * area(r0, r1);
            if cost < best.0 {
                best = (cost, split);
            }
        }
        // partition idx[s..e] by bin < split
        let Some(part) = idx.get_mut(s..e) else { continue };
        let split = best.1;
        let mut mid = 0usize;
        for k in 0..part.len() {
            let i = part.get(k).copied().unwrap_or(0);
            let c = cent.get(i).and_then(|c| c.get(ax)).copied().unwrap_or(0.0);
            if bin_of(c) < split {
                part.swap(k, mid);
                mid += 1;
            }
        }
        if mid == 0 || mid == part.len() {
            mid = part.len() / 2;
        }
        let left = nodes.len() / 2;
        nodes.resize(nodes.len() + 4, [0.0; 4]);
        set(nodes, left as u32, 0);
        stack.push((left, s, s + mid));
        stack.push((left + 1, s + mid, e));
    }
    idx
}

/// The GPU side of a High quality render.
pub struct PtRenderer {
    pub w: u32,
    pub h: u32,
    pipe: wgpu::ComputePipeline,
    out_pipe: wgpu::RenderPipeline,
    bind: wgpu::BindGroup,
    out_bind: wgpu::BindGroup,
    uni: wgpu::Buffer,
    post: wgpu::Buffer,
    textures: wgpu::Texture,
    out_tex: wgpu::Texture,
    out_view: wgpu::TextureView,
    origin: [f64; 3],
    pub done: u32,
    pub triangles: usize,
}

impl std::fmt::Debug for PtRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PtRenderer {}×{} ({} samples)", self.w, self.h, self.done)
    }
}

const PT_UNIFORM: u64 = 64 + 16 * 9 + 64 * 80;

impl PtRenderer {
    pub fn new(device: &wgpu::Device, w: u32, h: u32, scene: &PtScene) -> PtRenderer {
        let (w, h) = (w.clamp(1, 4096), h.clamp(1, 4096));
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cc pt"),
            source: wgpu::ShaderSource::Wgsl(include_str!("pt.wgsl").into()),
        });
        let post_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cc pt post"),
            source: wgpu::ShaderSource::Wgsl(include_str!("pt_post.wgsl").into()),
        });
        let bytes = |v: &[[f32; 4]]| -> Vec<u8> { v.iter().flat_map(|q| q.iter().flat_map(|x| x.to_le_bytes())).collect() };
        let sbuf = |label: &str, data: Vec<u8>| {
            let data = if data.is_empty() { vec![0u8; 16] } else { data };
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: &data,
                usage: wgpu::BufferUsages::STORAGE,
            })
        };
        let nodes = sbuf("pt nodes", bytes(&scene.nodes));
        let pos = sbuf("pt triangles", bytes(&scene.pos));
        let attr = sbuf("pt attributes", bytes(&scene.attr));
        let acc = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pt sum"),
            size: u64::from(w) * u64::from(h) * 16,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let uni = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pt frame"),
            size: PT_UNIFORM,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let post = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pt post"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let textures = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("pt material textures"),
            size: wgpu::Extent3d { width: TEX_SIZE, height: TEX_SIZE, depth_or_array_layers: TEX_LAYERS },
            mip_level_count: 9,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let tex_view = textures
            .create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let samp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("pt textures"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let st = |binding: u32, read_only: bool, vis: wgpu::ShaderStages| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: vis,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let cs = wgpu::ShaderStages::COMPUTE;
        let ub = |binding: u32, vis: wgpu::ShaderStages| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: vis,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pt"),
            entries: &[
                ub(0, cs),
                st(1, true, cs),
                st(2, true, cs),
                st(3, true, cs),
                st(4, false, cs),
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: cs,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: cs,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pt"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uni.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: nodes.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: pos.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: attr.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: acc.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&tex_view) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::Sampler(&samp) },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pt"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipe = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("pt trace"),
            layout: Some(&layout),
            module: &module,
            entry_point: Some("trace_px"),
            compilation_options: Default::default(),
            cache: None,
        });
        let fs = wgpu::ShaderStages::FRAGMENT;
        let obgl = device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("pt post"), entries: &[ub(0, fs), st(1, true, fs)] });
        let out_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pt post"),
            layout: &obgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: post.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: acc.as_entire_binding() },
            ],
        });
        let olayout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pt post"),
            bind_group_layouts: &[Some(&obgl)],
            immediate_size: 0,
        });
        let out_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pt picture"),
            layout: Some(&olayout),
            vertex: wgpu::VertexState {
                module: &post_module,
                entry_point: Some("vs_full"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &post_module,
                entry_point: Some("fs_out"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: crate::still::STILL_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let out_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("pt picture"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: crate::still::STILL_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let out_view = out_tex.create_view(&Default::default());
        PtRenderer {
            w,
            h,
            pipe,
            out_pipe,
            bind,
            out_bind,
            uni,
            post,
            textures,
            out_tex,
            out_view,
            origin: scene.origin,
            done: 0,
            triangles: scene.pos.len() / 3,
        }
    }

    pub fn set_texture(&self, queue: &wgpu::Queue, layer: u32, rgba: &[u8]) {
        crate::mesh3d::write_texture_layer(queue, &self.textures, layer, rgba);
    }

    /// Add one sample to every pixel.
    pub fn sample(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, job: &StillJob) {
        let origin = self.origin;
        let up = basis(job.up).0;
        let (c, r) = match job.bounds {
            Some((lo, hi)) => {
                let d = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
                (
                    [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0],
                    ((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() / 2.0).max(0.5),
                )
            }
            None => ([0.0; 3], 5.0),
        };
        let _ = c;
        let mut cam: Cam3 = job.cam;
        cam.width = f64::from(self.w);
        cam.height = f64::from(self.h);
        cam.far = cam.far.max(r * 200.0);
        let t = translation([origin[0] - cam.eye[0], origin[1] - cam.eye[1], origin[2] - cam.eye[2]]);
        let inv = invert(&mmul(&cam.proj(), &mmul(&cam.view_rot(), &t))).unwrap_or(IDENT);
        let sky = job.sky;
        let sun_on = job.sun.is_some_and(|s| s[0] * up[0] + s[1] * up[1] + s[2] * up[2] > 0.02) && !sky.night && sky.sun > 0.0;
        let mut fb: Vec<f32> = inv.iter().map(|v| *v as f32).collect();
        fb.extend([self.w as f32, self.h as f32, self.done as f32, job.lamps.len().min(64) as f32]);
        fb.extend([up[0] as f32, up[1] as f32, up[2] as f32, 0.0]);
        let s = job.sun.unwrap_or(up);
        fb.extend([s[0] as f32, s[1] as f32, s[2] as f32, if sun_on { 1.0 } else { 0.0 }]);
        fb.extend([sky.tint[0] * sky.sun as f32, sky.tint[1] * sky.sun as f32, sky.tint[2] * sky.sun as f32, 0.0]);
        for c in [sky.zenith, sky.horizon, sky.ground] {
            fb.extend([c[0], c[1], c[2], 1.0]);
        }
        fb.extend([if sky.plain { 1.2 } else { 1.0 }, 1.0, 6.0, 10.0]);
        fb.extend([0.0, 0.0, 0.0, 0.0]);
        for l in job.lamps.iter().take(64) {
            let rel = [l.p[0] - origin[0], l.p[1] - origin[1], l.p[2] - origin[2]];
            let kind = match l.kind.as_str() {
                "spot" => 1.0,
                "area" => 2.0,
                _ => 0.0,
            };
            let half = l.angle.to_radians() / 2.0;
            fb.extend([rel[0] as f32, rel[1] as f32, rel[2] as f32, kind]);
            fb.extend([l.dir[0] as f32, l.dir[1] as f32, l.dir[2] as f32, half.cos() as f32]);
            // an area light: ShapedAreaLight's intensity over its area, its power spread over its panel
            let k = l.candela as f32;
            fb.extend([l.color[0] * k, l.color[1] * k, l.color[2] * k, (half * (1.0 - 0.35)).cos() as f32]);
            let (_, e1, e2) = basis(l.dir);
            let (hw, hh) = (l.size[0] / 2.0, l.size[1] / 2.0);
            let bulb = if l.kind == "area" { 0.0 } else { l.size[0] / 2.0 };
            fb.extend([(e1[0] * hw) as f32, (e1[1] * hw) as f32, (e1[2] * hw) as f32, bulb as f32]);
            fb.extend([(e2[0] * hh) as f32, (e2[1] * hh) as f32, (e2[2] * hh) as f32, 0.0]);
        }
        fb.resize(PT_UNIFORM as usize / 4, 0.0);
        queue.write_buffer(&self.uni, 0, &fb.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>());
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("pt sample") });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("pt sample"), timestamp_writes: None });
            pass.set_pipeline(&self.pipe);
            pass.set_bind_group(0, &self.bind, &[]);
            pass.dispatch_workgroups(self.w.div_ceil(8), self.h.div_ceil(8), 1);
        }
        queue.submit([enc.finish()]);
        self.done += 1;
    }

    /// The picture of the samples so far.
    pub fn picture(&self, device: &wgpu::Device, queue: &wgpu::Queue, exposure: f64) -> Result<Vec<u8>, String> {
        let k = [self.w as f32, 2f64.powf(exposure) as f32, 0.0, 0.0];
        queue.write_buffer(&self.post, 0, &k.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>());
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("pt picture") });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pt picture"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.out_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.out_pipe);
            pass.set_bind_group(0, &self.out_bind, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit([enc.finish()]);
        crate::mesh3d::read_texture(device, queue, &self.out_tex, self.w, self.h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tree_holds_every_triangle_once() {
        let mut tris = Vec::new();
        for i in 0..1000 {
            let x = (i % 37) as f64;
            let y = (i / 37) as f64;
            let v = |dx: f64, dy: f64| V { p: [x + dx, y + dy, (i % 5) as f64], n: [0.0, 0.0, 1.0], uv: [0.0, 0.0] };
            tris.push(([v(0.0, 0.0), v(1.0, 0.0), v(0.0, 1.0)], [1.0; 4], [0.0; 4]));
        }
        let mut nodes = Vec::new();
        let order = bvh(&tris, &mut nodes);
        let mut seen = order.clone();
        seen.sort_unstable();
        assert_eq!(seen, (0..1000).collect::<Vec<_>>());
        // every leaf's run lies inside the order, the leaves cover it exactly
        let mut covered = 0u32;
        for pair in nodes.chunks(2) {
            let count = pair[1][3].to_bits();
            if count > 0 {
                assert!(count <= 4 || pair[0][3].to_bits() + count <= 1000);
                covered += count;
            }
        }
        assert_eq!(covered, 1000);
    }
}

#[cfg(test)]
mod gpu_tests {
    use super::*;
    use crate::still::StillMesh;

    #[test]
    fn a_cube_path_traced_headless() {
        let inst = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Some(adapter) = pollster::block_on(inst.request_adapter(&wgpu::RequestAdapterOptions::default())).ok() else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        let Some((device, queue)) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok() else { return };
        let _ = StillMesh::upload;
        let m = crate::still::gpu_tests::cube();
        let looks = vec![vec![[199u8, 0, 0, 0]; m.positions.len()]];
        let j = crate::still::gpu_tests::job(64, 48);
        let scene = PtScene::build(
            std::slice::from_ref(&m),
            &looks,
            &[Instance3 { mesh: 0, m: IDENT, tint: None, hidden: false }],
            [0.0, 0.0, 1.0],
            None,
            Some(([0.0, 0.0, -1.0], 60.0, [0.6, 0.58, 0.55])),
        );
        assert_eq!(scene.pos.len() / 3, 12 + 96);
        let mut r = PtRenderer::new(&device, 64, 48, &scene);
        for _ in 0..64 {
            r.sample(&device, &queue, &j);
        }
        let px = r.picture(&device, &queue, 0.0).unwrap();
        if let Ok(dir) = std::env::var("CC_RENDER_DUMP") {
            let mut ppm = b"P6\n64 48\n255\n".to_vec();
            ppm.extend(px.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]));
            std::fs::write(std::path::Path::new(&dir).join("pt.ppm"), ppm).unwrap();
        }
        let at = |x: usize, y: usize| [px[(y * 64 + x) * 4], px[(y * 64 + x) * 4 + 1], px[(y * 64 + x) * 4 + 2]];
        let sky = at(32, 1);
        assert!(sky[2] > sky[0], "sky {sky:?}");
        let mid = at(32, 26);
        assert!(mid != sky && mid[0] > 40, "cube {mid:?}");
    }
}
