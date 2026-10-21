//! Rendered stills (0.5 render/render-core.js, "Fast"): the scene drawn many times into a linear HDR target, each pass
//! jittering the camera under a pixel (anti-aliasing), the sun inside its small disc (soft shadow edges), one sky light
//! over the hemisphere (soft contact shadows: daylight's ambient occlusion) and every lamp inside its bulb or panel;
//! the passes are summed, divided by how many were done and tone-mapped (ACES filmic, exposure in stops) to sRGB once,
//! so a stopped render keeps a finished picture of what it did. Physically based faces (rough, metal, glass from the
//! colours' looks), the gradient dome as background and image light, a matte ground disc that takes the shadows and
//! fades into the horizon. The halton / hemisphere / sun-disc / lamp-point sequences are looks.js's, so the same model
//! gives the same picture.
//!
//! WebGPU-safe: Rgba16Float targets (blendable without extra features), depth textures with comparison samplers,
//! `textureSampleCompareLevel`, a uniform array of 64 lamps.

use wgpu::util::DeviceExt;

use crate::mesh3d::{Cam3, IDENT, Instance3, InstanceSet, MeshData, TEX_LAYERS, TEX_SIZE, mmul, translation};

/// A light of the picture's scene (the dome's lights are in [`StillSky`]); world coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct StillLamp {
    /// "point", "spot" or "area"
    pub kind: String,
    pub p: [f64; 3],
    pub dir: [f64; 3],
    /// linear colour
    pub color: [f32; 3],
    /// three.js candela (looks.js lampCandela)
    pub candela: f64,
    /// spot: full beam, degrees
    pub angle: f64,
    pub size: [f64; 2],
}

/// The sky of a picture (looks.js SKIES / NIGHT), colours linear.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StillSky {
    pub zenith: [f32; 3],
    pub horizon: [f32; 3],
    pub ground: [f32; 3],
    pub sun: f64,
    pub sky: f64,
    pub env: f64,
    pub tint: [f32; 3],
    pub sky_tint: [f32; 3],
    pub plain: bool,
    pub night: bool,
}

/// What to render: camera (sized to the picture), sky, sun, ground and the rest of renderStill's inputs.
#[derive(Clone, Debug)]
pub struct StillJob {
    pub cam: Cam3,
    pub w: u32,
    pub h: u32,
    pub passes: u32,
    pub sky: StillSky,
    /// towards the sun (world), None when it is down
    pub sun: Option<[f64; 3]>,
    pub up: [f64; 3],
    pub bounds: Option<([f64; 3], [f64; 3])>,
    pub ground_at: f64,
    pub ground: bool,
    pub edges: bool,
    /// stops
    pub exposure: f64,
    /// a section cut: keep n · p ≤ d
    pub clip: Option<([f64; 3], f64)>,
    pub lamps: Vec<StillLamp>,
}

/// looks.js `halton(i, b)`.
pub fn halton(mut i: u64, b: u64) -> f64 {
    let (mut f, mut r) = (1.0f64, 0.0f64);
    let bf = b as f64;
    while i > 0 && b > 1 {
        f /= bf;
        r += f * (i % b) as f64;
        i /= b;
    }
    r
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
/// Σ v · k over the pairs.
fn lin3(terms: &[([f64; 3], f64)]) -> [f64; 3] {
    terms.iter().fold([0.0; 3], |a, (v, k)| [a[0] + v[0] * k, a[1] + v[1] * k, a[2] + v[2] * k])
}
fn vnorm(v: [f64; 3]) -> [f64; 3] {
    let l = dot(v, v).sqrt();
    let l = if l > 0.0 && l.is_finite() { l } else { 1.0 };
    [v[0] / l, v[1] / l, v[2] / l]
}

/// looks.js `basis(up)`: up and two unit vectors square to it.
pub fn basis(up: [f64; 3]) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let u = vnorm(up);
    let ax = if u[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
    let e1 = vnorm(cross(u, ax));
    let e2 = cross(u, e1);
    (u, e1, e2)
}

/// looks.js `hemisphere(i, up)`: pass i's sky light, cosine-weighted, kept ~10° above the horizon.
pub fn hemisphere(i: u64, up: [f64; 3]) -> [f64; 3] {
    let (u, e1, e2) = basis(up);
    let r = halton(i + 1, 11).min(0.97).sqrt();
    let a = 2.0 * std::f64::consts::PI * halton(i + 1, 13);
    let z = (1.0 - r * r).sqrt();
    let (x, y) = (r * a.cos(), r * a.sin());
    vnorm(lin3(&[(e1, x), (e2, y), (u, z)]))
}

/// looks.js `sunDisc(dir, i, degrees)`: a direction inside the sun's small cone for pass i.
pub fn sun_disc(dir: [f64; 3], i: u64, degrees: f64) -> [f64; 3] {
    let (d, e1, e2) = basis(dir);
    let r = halton(i + 1, 5).sqrt() * degrees.to_radians().tan();
    let a = 2.0 * std::f64::consts::PI * halton(i + 1, 7);
    let (x, y) = (r * a.cos(), r * a.sin());
    vnorm(lin3(&[(d, 1.0), (e1, x), (e2, y)]))
}

/// looks.js `lampAt(L, i)`: where pass i puts a lamp (a point of its bulb, or of its panel).
pub fn lamp_at(l: &StillLamp, i: u64) -> [f64; 3] {
    let (a, b) = (halton(i + 1, 17), halton(i + 1, 19));
    if l.kind == "area" {
        let (_, e1, e2) = basis(l.dir);
        let (w, h) = (l.size[0], l.size[1]);
        return lin3(&[(l.p, 1.0), (e1, (a - 0.5) * w), (e2, (b - 0.5) * h)]);
    }
    let r = l.size[0] / 2.0;
    let z = 2.0 * a - 1.0;
    let ph = 2.0 * std::f64::consts::PI * b;
    let s = (1.0 - z * z).max(0.0).sqrt();
    let c = halton(i + 1, 23).powf(1.0 / 3.0);
    [l.p[0] + r * c * s * ph.cos(), l.p[1] + r * c * s * ph.sin(), l.p[2] + r * c * z]
}

/// A mesh of the picture: the view's mesh data plus each vertex's look (rough, metal, glass as 0…255).
#[derive(Debug)]
pub struct StillMesh {
    pub origin: [f64; 3],
    verts: Option<wgpu::Buffer>,
    idx: Option<wgpu::Buffer>,
    edges: Option<wgpu::Buffer>,
    triangles: u32,
    segments: u32,
}

const SVSTRIDE: u64 = 44; // pos 12 + normal 12 + colour 4 + texture u, v, layer 12 + look 4
const SHADOW: u32 = 2048;
const UNIFORM: u64 = 16 * 4 * 4 + 16 * 16 + 64 * 48;

impl StillMesh {
    pub fn upload(device: &wgpu::Device, m: &MeshData, looks: &[[u8; 4]]) -> StillMesh {
        let mut vb = Vec::with_capacity(m.positions.len() * SVSTRIDE as usize);
        for (i, p) in m.positions.iter().enumerate() {
            let n = m.normals.get(i).copied().unwrap_or([0.0, 0.0, 1.0]);
            for v in p.iter().chain(n.iter()) {
                vb.extend_from_slice(&v.to_le_bytes());
            }
            vb.extend_from_slice(&m.colors.get(i).copied().unwrap_or([200, 200, 200, 255]));
            for v in m.tex.get(i).copied().unwrap_or([0.0, 0.0, -1.0]) {
                vb.extend_from_slice(&v.to_le_bytes());
            }
            vb.extend_from_slice(&looks.get(i).copied().unwrap_or([199, 0, 0, 0]));
        }
        let ib: Vec<u8> = m.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let eb: Vec<u8> = m.edges.iter().flat_map(|p| p.iter().flat_map(|v| v.to_le_bytes())).collect();
        let mk = |label: &str, bytes: &[u8], usage: wgpu::BufferUsages| {
            (!bytes.is_empty())
                .then(|| device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: bytes, usage }))
        };
        StillMesh {
            origin: m.origin,
            verts: mk("still verts", &vb, wgpu::BufferUsages::VERTEX),
            idx: mk("still idx", &ib, wgpu::BufferUsages::INDEX),
            edges: mk("still edges", &eb, wgpu::BufferUsages::VERTEX),
            triangles: u32::try_from(m.indices.len() / 3).unwrap_or(u32::MAX),
            segments: u32::try_from(m.edges.len() / 2).unwrap_or(u32::MAX),
        }
    }
}

/// The GPU side of one render: pipelines, targets, shadow maps; passes are added one call at a time.
pub struct StillRenderer {
    pub w: u32,
    pub h: u32,
    face: wgpu::RenderPipeline,
    glass: wgpu::RenderPipeline,
    edge: wgpu::RenderPipeline,
    shadow: wgpu::RenderPipeline,
    sky: wgpu::RenderPipeline,
    ground: wgpu::RenderPipeline,
    add: wgpu::RenderPipeline,
    out: wgpu::RenderPipeline,
    uni: wgpu::Buffer,
    shd: [wgpu::Buffer; 2],
    bind: [wgpu::BindGroup; 2],
    maps_bind: wgpu::BindGroup,
    map_views: [wgpu::TextureView; 2],
    textures: wgpu::Texture,
    pass_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    acc_view: wgpu::TextureView,
    out_tex: wgpu::Texture,
    out_view: wgpu::TextureView,
    add_bind: wgpu::BindGroup,
    out_bind: wgpu::BindGroup,
    post: wgpu::Buffer,
    meshes: Vec<StillMesh>,
    instances: Option<InstanceSet>,
    /// passes summed so far
    pub done: u32,
}

impl std::fmt::Debug for StillRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "StillRenderer {}×{} ({} passes)", self.w, self.h, self.done)
    }
}

const HDR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// The picture's format (sRGB-encoded by the shader, read back as bytes).
pub const STILL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

impl StillRenderer {
    /// For a picture of `w × h` (each side 1…4096) of `meshes` (with their looks) placed by `instances`, relative to
    /// `origin` (the scene's box centre).
    pub fn new(device: &wgpu::Device, w: u32, h: u32, meshes: Vec<StillMesh>, instances: &[Instance3], origin: [f64; 3]) -> StillRenderer {
        let (w, h) = (w.clamp(1, 4096), h.clamp(1, 4096));
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cc still"),
            source: wgpu::ShaderSource::Wgsl(include_str!("still.wgsl").into()),
        });
        let post_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cc still post"),
            source: wgpu::ShaderSource::Wgsl(include_str!("still_post.wgsl").into()),
        });
        let ubuf = |label: &str, size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let uni = ubuf("still frame", UNIFORM);
        let shd = [ubuf("still shadow sun", 64), ubuf("still shadow sky", 64)];
        let post = ubuf("still post", 16);
        let vis = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let ub_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: vis,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let bgl0 = device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("still 0"), entries: &[ub_entry(0), ub_entry(1)] });
        let bind = [0, 1].map(|k| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("still 0"),
                layout: &bgl0,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: uni.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: shd.get(k).unwrap_or(&shd[0]).as_entire_binding() },
                ],
            })
        });
        let depth_tex = |label: &str, tw: u32, th: u32, extra: wgpu::TextureUsages| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: tw, height: th, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | extra,
                view_formats: &[],
            })
        };
        let map_views = [0, 1]
            .map(|_| depth_tex("still shadow map", SHADOW, SHADOW, wgpu::TextureUsages::TEXTURE_BINDING).create_view(&Default::default()));
        let textures = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("still material textures"),
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
        let fs_tex = |binding: u32, sample_type: wgpu::TextureSampleType, dim: wgpu::TextureViewDimension| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type, view_dimension: dim, multisampled: false },
            count: None,
        };
        let samp_entry = |binding: u32, ty: wgpu::SamplerBindingType| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(ty),
            count: None,
        };
        let bgl1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("still 1"),
            entries: &[
                fs_tex(0, wgpu::TextureSampleType::Depth, wgpu::TextureViewDimension::D2),
                fs_tex(1, wgpu::TextureSampleType::Depth, wgpu::TextureViewDimension::D2),
                samp_entry(2, wgpu::SamplerBindingType::Comparison),
                fs_tex(3, wgpu::TextureSampleType::Float { filterable: true }, wgpu::TextureViewDimension::D2Array),
                samp_entry(4, wgpu::SamplerBindingType::Filtering),
            ],
        });
        let cmp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("still shadow"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let tsamp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("still textures"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        let [mv0, mv1] = &map_views;
        let maps_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("still 1"),
            layout: &bgl1,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(mv0) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(mv1) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&cmp) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&tex_view) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&tsamp) },
            ],
        });
        let main_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("still"),
            bind_group_layouts: &[Some(&bgl0), Some(&bgl1)],
            immediate_size: 0,
        });
        let shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("still shadow"),
            bind_group_layouts: &[Some(&bgl0)],
            immediate_size: 0,
        });
        let inst_attrs = wgpu::vertex_attr_array![
            3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4,
            7 => Float32x4, 8 => Float32x4, 9 => Float32x4, 10 => Float32x4
        ];
        let vert_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4, 11 => Float32x3, 12 => Unorm8x4];
        let edge_attrs = wgpu::vertex_attr_array![0 => Float32x3];
        let inst = wgpu::VertexBufferLayout { array_stride: 128, step_mode: wgpu::VertexStepMode::Instance, attributes: &inst_attrs };
        let verts = wgpu::VertexBufferLayout { array_stride: SVSTRIDE, step_mode: wgpu::VertexStepMode::Vertex, attributes: &vert_attrs };
        let edges = wgpu::VertexBufferLayout { array_stride: 12, step_mode: wgpu::VertexStepMode::Vertex, attributes: &edge_attrs };
        let ds = |write: bool, cmp: wgpu::CompareFunction| {
            Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(write),
                depth_compare: Some(cmp),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            })
        };
        let target = |format: wgpu::TextureFormat, blend: Option<wgpu::BlendState>| {
            [Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })]
        };
        let hdr_t = target(HDR, None);
        let hdr_blend = target(HDR, Some(wgpu::BlendState::ALPHA_BLENDING));
        let add_t = target(
            HDR,
            Some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent::REPLACE,
            }),
        );
        let out_t = target(STILL_FORMAT, None);
        let pipe = |label: &str,
                    layout: &wgpu::PipelineLayout,
                    module: &wgpu::ShaderModule,
                    vs: &str,
                    fs: &str,
                    buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
                    targets: &[Option<wgpu::ColorTargetState>],
                    topology: wgpu::PrimitiveTopology,
                    cull: Option<wgpu::Face>,
                    depth: Option<wgpu::DepthStencilState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState { module, entry_point: Some(vs), compilation_options: Default::default(), buffers },
                fragment: Some(wgpu::FragmentState { module, entry_point: Some(fs), compilation_options: Default::default(), targets }),
                primitive: wgpu::PrimitiveState { topology, cull_mode: cull, ..Default::default() },
                depth_stencil: depth,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let tri = wgpu::PrimitiveTopology::TriangleList;
        let face_bufs = [Some(verts.clone()), Some(inst.clone())];
        let face = pipe(
            "still faces",
            &main_layout,
            &shader,
            "vs_face",
            "fs_face",
            &face_bufs,
            &hdr_t,
            tri,
            None,
            ds(true, wgpu::CompareFunction::Greater),
        );
        let glass = pipe(
            "still glass",
            &main_layout,
            &shader,
            "vs_face",
            "fs_glass",
            &face_bufs,
            &hdr_blend,
            tri,
            None,
            ds(false, wgpu::CompareFunction::Greater),
        );
        let edge = pipe(
            "still edges",
            &main_layout,
            &shader,
            "vs_edge",
            "fs_edge",
            &[Some(edges), Some(inst.clone())],
            &hdr_blend,
            wgpu::PrimitiveTopology::LineList,
            None,
            ds(false, wgpu::CompareFunction::GreaterEqual),
        );
        let shadow = pipe(
            "still shadow",
            &shadow_layout,
            &shader,
            "vs_shadow",
            "fs_shadow",
            &face_bufs,
            &[],
            tri,
            Some(wgpu::Face::Front),
            ds(true, wgpu::CompareFunction::Less),
        );
        let sky =
            pipe("still sky", &main_layout, &shader, "vs_full", "fs_sky", &[], &hdr_t, tri, None, ds(false, wgpu::CompareFunction::Always));
        let ground = pipe(
            "still ground",
            &main_layout,
            &shader,
            "vs_ground",
            "fs_ground",
            &[],
            &hdr_t,
            tri,
            Some(wgpu::Face::Back),
            ds(true, wgpu::CompareFunction::Greater),
        );
        // the post passes read one texture and the post uniform
        let post_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("still post"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                ub_entry(1),
            ],
        });
        let post_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("still post"),
            bind_group_layouts: &[Some(&post_bgl)],
            immediate_size: 0,
        });
        let add = pipe("still add", &post_layout, &post_shader, "vs_full", "fs_add", &[], &add_t, tri, None, None);
        let out = pipe("still out", &post_layout, &post_shader, "vs_full", "fs_out", &[], &out_t, tri, None, None);
        let color_tex = |label: &str, format: wgpu::TextureFormat, extra: wgpu::TextureUsages| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | extra,
                view_formats: &[],
            })
        };
        let pass_view = color_tex("still pass", HDR, wgpu::TextureUsages::empty()).create_view(&Default::default());
        let acc_view = color_tex("still sum", HDR, wgpu::TextureUsages::empty()).create_view(&Default::default());
        let out_tex = color_tex("still picture", STILL_FORMAT, wgpu::TextureUsages::COPY_SRC);
        let out_view = out_tex.create_view(&Default::default());
        let depth_view = depth_tex("still depth", w, h, wgpu::TextureUsages::empty()).create_view(&Default::default());
        let post_bind = |view: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("still post"),
                layout: &post_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                    wgpu::BindGroupEntry { binding: 1, resource: post.as_entire_binding() },
                ],
            })
        };
        let add_bind = post_bind(&pass_view);
        let out_bind = post_bind(&acc_view);
        let origins: Vec<[f64; 3]> = meshes.iter().map(|m| m.origin).collect();
        let set = InstanceSet::with_origins(device, &origins, instances, origin);
        StillRenderer {
            w,
            h,
            face,
            glass,
            edge,
            shadow,
            sky,
            ground,
            add,
            out,
            uni,
            shd,
            bind,
            maps_bind,
            map_views,
            textures,
            pass_view,
            depth_view,
            acc_view,
            out_tex,
            out_view,
            add_bind,
            out_bind,
            post,
            meshes,
            instances: Some(set),
            done: 0,
        }
    }

    /// A material texture into its layer (see `Renderer3::set_texture`).
    pub fn set_texture(&self, queue: &wgpu::Queue, layer: u32, rgba: &[u8]) {
        crate::mesh3d::write_texture_layer(queue, &self.textures, layer, rgba);
    }

    /// Add pass `i` (0-based) of `job` to the sum.
    pub fn pass(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, job: &StillJob) {
        let i = u64::from(self.done);
        let Some(set) = &self.instances else { return };
        let origin = set.origin;
        let up = basis(job.up).0;
        let (c, r) = match job.bounds {
            Some((lo, hi)) => (
                [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0],
                (dot(sub(hi, lo), sub(hi, lo)).sqrt() / 2.0).max(0.5),
            ),
            None => ([0.0; 3], 5.0),
        };
        let rel = |p: [f64; 3]| sub(p, origin);
        // the camera: under a pixel, its far plane out to the horizon
        let mut cam = job.cam;
        cam.width = f64::from(self.w);
        cam.height = f64::from(self.h);
        cam.far = cam.far.max(r * 200.0);
        let (jx, jy) = ((halton(i + 1, 2) - 0.5) * 2.0 / f64::from(self.w), (halton(i + 1, 3) - 0.5) * 2.0 / f64::from(self.h));
        let mut p = cam.proj();
        for k in 0..4 {
            let w = p.get(k * 4 + 3).copied().unwrap_or(0.0);
            if let Some(x) = p.get_mut(k * 4) {
                *x += jx * w;
            }
            if let Some(y) = p.get_mut(k * 4 + 1) {
                *y += jy * w;
            }
        }
        let t = translation([origin[0] - cam.eye[0], origin[1] - cam.eye[1], origin[2] - cam.eye[2]]);
        let pv = mmul(&p, &mmul(&cam.view_rot(), &t));
        let inv_pr = crate::mesh3d::invert(&mmul(&cam.proj(), &cam.view_rot())).unwrap_or(IDENT);
        // the lights
        let sky = job.sky;
        let sun_on = job.sun.is_some_and(|s| dot(s, up) > 0.02) && sky.sun > 0.0;
        let sun_d = job.sun.map(|s| sun_disc(s, i, 1.5));
        let sky_d = hemisphere(i, up);
        let (_, e1, e2) = basis(up);
        let light_cam = |d: [f64; 3]| -> [f64; 16] {
            // place(): at c + d · 4R looking at c, ±1.6R, near 0.05R, far 8R; its up never along the light
            let d = vnorm(d);
            let eye = rel([c[0] + d[0] * r * 4.0, c[1] + d[1] * r * 4.0, c[2] + d[2] * r * 4.0]);
            let fwd = [-d[0], -d[1], -d[2]];
            let up0 = if dot(d, e1).abs() < dot(d, e2).abs() { e1 } else { e2 };
            let right = vnorm(cross(fwd, up0));
            let upv = cross(right, fwd);
            let (near, far, half) = (r * 0.05, r * 8.0, r * 1.6);
            let (sx, sz) = (1.0 / half, 1.0 / (far - near));
            [
                right[0] * sx,
                upv[0] * sx,
                fwd[0] * sz,
                0.0,
                right[1] * sx,
                upv[1] * sx,
                fwd[1] * sz,
                0.0,
                right[2] * sx,
                upv[2] * sx,
                fwd[2] * sz,
                0.0,
                -dot(right, eye) * sx,
                -dot(upv, eye) * sx,
                (-dot(fwd, eye) - near) * sz,
                1.0,
            ]
        };
        let lpv_sun = sun_d.filter(|_| sun_on).map_or(IDENT, light_cam);
        let lpv_sky = light_cam(sky_d);
        let sky_int = sky.sky * if sun_on || sky.night { 1.0 } else { 1.35 };
        let mut fb: Vec<f32> = Vec::with_capacity(UNIFORM as usize / 4);
        let m32 = |fb: &mut Vec<f32>, m: &[f64; 16]| fb.extend(m.iter().map(|v| *v as f32));
        let v4 = |fb: &mut Vec<f32>, a: [f64; 3], w: f64| fb.extend([a[0] as f32, a[1] as f32, a[2] as f32, w as f32]);
        let c4 = |fb: &mut Vec<f32>, a: [f32; 3], k: f64, w: f32| fb.extend([a[0] * k as f32, a[1] * k as f32, a[2] * k as f32, w]);
        m32(&mut fb, &pv);
        m32(&mut fb, &inv_pr);
        m32(&mut fb, &lpv_sun);
        m32(&mut fb, &lpv_sky);
        v4(&mut fb, rel(cam.eye), 1.0);
        v4(&mut fb, up, 0.0);
        v4(&mut fb, sun_d.unwrap_or(up), 0.0);
        c4(&mut fb, sky.tint, sky.sun, if sun_on { 1.0 } else { 0.0 });
        v4(&mut fb, sky_d, 0.0);
        c4(&mut fb, sky.sky_tint, sky_int, if sky_int > 0.0 { 1.0 } else { 0.0 });
        c4(&mut fb, sky.zenith, 1.0, 1.0);
        c4(&mut fb, sky.horizon, 1.0, 1.0);
        c4(&mut fb, sky.ground, 1.0, 1.0);
        fb.extend([sky.env as f32, (r * 10.0) as f32, (r * 120.0) as f32, if sky.plain { 0.0 } else { 1.0 }]);
        // the ground disc: SKY_R · 0.98 at the ground's height, a hair under it
        let gz = job.ground_at - r * 1e-4;
        v4(&mut fb, rel([c[0], c[1], gz]), r * 150.0 * 0.98);
        let gc = if sky.plain { srgb([0xf4, 0xf4, 0xf2]) } else { srgb([0xcb, 0xc6, 0xbb]) };
        fb.extend([gc[0], gc[1], gc[2], if job.ground { 1.0 } else { 0.0 }]);
        fb.extend([-0.0002, (r * 0.002) as f32, 1.0 / SHADOW as f32, if job.edges { 1.0 } else { 0.0 }]);
        let ec = srgb([0x2a, 0x2f, 0x36]);
        fb.extend([ec[0], ec[1], ec[2], 0.55]);
        match job.clip {
            Some((n, d)) => {
                let w = d - dot(n, origin);
                fb.extend([n[0] as f32, n[1] as f32, n[2] as f32, w as f32]);
            }
            None => fb.extend([0.0; 4]),
        }
        let lamps: Vec<&StillLamp> = job.lamps.iter().take(64).collect();
        fb.extend([lamps.len() as f32, 0.0, 0.0, 1.0]);
        for l in &lamps {
            let q = lamp_at(l, i);
            let off = if l.kind == "area" { 0.01 } else { 0.0 };
            let pos = [q[0] + l.dir[0] * off, q[1] + l.dir[1] * off, q[2] + l.dir[2] * off];
            let kind = match l.kind.as_str() {
                "spot" => 1.0,
                "area" => 2.0,
                _ => 0.0,
            };
            let (half, pen) = if l.kind == "spot" { (l.angle.to_radians() / 2.0, 0.35) } else { (85f64.to_radians(), 1.0) };
            v4(&mut fb, rel(pos), kind);
            v4(&mut fb, vnorm(l.dir), half.cos());
            c4(&mut fb, l.color, l.candela, (half * (1.0 - pen)).cos() as f32);
        }
        fb.resize(UNIFORM as usize / 4, 0.0);
        let bytes: Vec<u8> = fb.iter().flat_map(|v| v.to_le_bytes()).collect();
        queue.write_buffer(&self.uni, 0, &bytes);
        let [s0, s1] = &self.shd;
        queue.write_buffer(s0, 0, &lpv_sun.map(|v| v as f32).iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>());
        queue.write_buffer(s1, 0, &lpv_sky.map(|v| v as f32).iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>());

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("still pass") });
        let inst_buf = set.buf.as_ref();
        let draw_faces = |pass: &mut wgpu::RenderPass<'_>| {
            for (mi, rg) in &set.ranges {
                let Some(mesh) = self.meshes.get(*mi) else { continue };
                if let (Some(v), Some(ix)) = (&mesh.verts, &mesh.idx) {
                    pass.set_vertex_buffer(0, v.slice(..));
                    pass.set_index_buffer(ix.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.triangles * 3, 0, rg.clone());
                }
            }
        };
        for (k, on) in [sun_on, true].into_iter().enumerate() {
            let (Some(view), Some(bg)) = (self.map_views.get(k), self.bind.get(k)) else { continue };
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("still shadow"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if on && let Some(ib) = inst_buf {
                pass.set_pipeline(&self.shadow);
                pass.set_bind_group(0, bg, &[]);
                pass.set_vertex_buffer(1, ib.slice(..));
                draw_faces(&mut pass);
            }
        }
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("still main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.pass_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(b0) = self.bind.first() {
                pass.set_bind_group(0, b0, &[]);
            }
            pass.set_bind_group(1, &self.maps_bind, &[]);
            pass.set_pipeline(&self.sky);
            pass.draw(0..3, 0..1);
            if job.ground {
                pass.set_pipeline(&self.ground);
                pass.draw(0..6, 0..1);
            }
            if let Some(ib) = inst_buf {
                pass.set_vertex_buffer(1, ib.slice(..));
                pass.set_pipeline(&self.face);
                draw_faces(&mut pass);
                pass.set_pipeline(&self.glass);
                draw_faces(&mut pass);
                if job.edges {
                    pass.set_pipeline(&self.edge);
                    for (mi, rg) in &set.ranges {
                        let Some(mesh) = self.meshes.get(*mi) else { continue };
                        if let Some(e) = &mesh.edges {
                            pass.set_vertex_buffer(0, e.slice(..));
                            pass.draw(0..mesh.segments * 2, rg.clone());
                        }
                    }
                }
            }
        }
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("still add"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.acc_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if self.done == 0 { wgpu::LoadOp::Clear(wgpu::Color::BLACK) } else { wgpu::LoadOp::Load },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.add);
            pass.set_bind_group(0, &self.add_bind, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit([enc.finish()]);
        self.done += 1;
    }

    /// The picture of the passes so far (tone-mapped, sRGB), tightly packed RGBA rows.
    pub fn picture(&self, device: &wgpu::Device, queue: &wgpu::Queue, exposure: f64) -> Result<Vec<u8>, String> {
        let k = [1.0 / self.done.max(1) as f32, 2f64.powf(exposure) as f32, 0.0, 0.0];
        queue.write_buffer(&self.post, 0, &k.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>());
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("still picture") });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("still picture"),
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
            pass.set_pipeline(&self.out);
            pass.set_bind_group(0, &self.out_bind, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit([enc.finish()]);
        crate::mesh3d::read_texture(device, queue, &self.out_tex, self.w, self.h)
    }
}

/// '#rrggbb' bytes → linear (three.js Color from a hex, colour managed).
pub fn srgb(c: [u8; 3]) -> [f32; 3] {
    c.map(|v| {
        let x = f32::from(v) / 255.0;
        if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halton_and_the_sky_samples() {
        assert_eq!(halton(1, 2), 0.5);
        assert_eq!(halton(3, 2), 0.75);
        assert!((halton(5, 3) - (2.0 / 3.0 + 1.0 / 9.0)).abs() < 1e-12);
        for i in 0..64 {
            let d = hemisphere(i, [0.0, 0.0, 1.0]);
            assert!(d[2] > 0.17, "kept above the horizon: {d:?}");
            let s = sun_disc([0.0, 0.0, 1.0], i, 1.5);
            assert!(s[2] > 1.5f64.to_radians().cos() - 1e-9);
        }
    }
}

#[cfg(test)]
pub(crate) mod gpu_tests {
    use super::*;

    fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let inst = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(inst.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
    }

    /// A 2 m cube on the ground, white, as MeshData.
    pub(crate) fn cube() -> MeshData {
        let v = |i: usize| [if i & 1 == 1 { 1.0 } else { -1.0 }, if i & 2 == 2 { 1.0 } else { -1.0 }, if i & 4 == 4 { 2.0 } else { 0.0 }];
        let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
        let mut t = Vec::new();
        for q in quads {
            for tri in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
                for i in tri {
                    t.extend_from_slice(&v(i));
                }
            }
        }
        MeshData::from_triangles(&t, &[[230, 227, 220, 255]; 12], &[])
    }

    pub(crate) fn job(w: u32, h: u32) -> StillJob {
        let cam = Cam3 {
            eye: [6.0, -8.0, 2.0],
            target: [0.0, 0.0, 1.5],
            width: f64::from(w),
            height: f64::from(h),
            near: 0.1,
            far: 100.0,
            ..Cam3::default()
        };
        let c = |x: [u8; 3]| srgb(x);
        StillJob {
            cam,
            w,
            h,
            passes: 8,
            sky: StillSky {
                zenith: c([0x2f, 0x6c, 0xb3]),
                horizon: c([0xc4, 0xd6, 0xe8]),
                ground: c([0x8f, 0x8a, 0x80]),
                sun: 3.2,
                sky: 1.6,
                env: 0.45,
                tint: c([0xff, 0xf4, 0xe5]),
                sky_tint: c([0xdf, 0xea, 0xff]),
                plain: false,
                night: false,
            },
            sun: Some(vnorm([0.5, -0.3, 0.8])),
            up: [0.0, 0.0, 1.0],
            bounds: Some(([-1.0, -1.0, 0.0], [1.0, 1.0, 2.0])),
            ground_at: 0.0,
            ground: true,
            edges: false,
            exposure: 0.0,
            clip: None,
            lamps: vec![StillLamp {
                kind: "point".into(),
                p: [2.0, -2.0, 1.0],
                dir: [0.0, 0.0, -1.0],
                color: [1.0, 0.9, 0.8],
                candela: 1.27,
                angle: 60.0,
                size: [0.06, 0.06],
            }],
        }
    }

    #[test]
    fn a_cube_in_the_sun_renders_headless() {
        let Some((device, queue)) = device() else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        let m = cube();
        let looks = vec![[199u8, 0, 0, 0]; m.positions.len()];
        let mesh = StillMesh::upload(&device, &m, &looks);
        let j = job(96, 64);
        let mut r =
            StillRenderer::new(&device, 96, 64, vec![mesh], &[Instance3 { mesh: 0, m: IDENT, tint: None, hidden: false }], [0.0, 0.0, 1.0]);
        for _ in 0..j.passes {
            r.pass(&device, &queue, &j);
        }
        let px = r.picture(&device, &queue, 0.0).unwrap();
        assert_eq!(px.len(), 96 * 64 * 4);
        if let Ok(dir) = std::env::var("CC_RENDER_DUMP") {
            let mut ppm = b"P6\n96 64\n255\n".to_vec();
            ppm.extend(px.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]));
            std::fs::write(std::path::Path::new(&dir).join("still.ppm"), ppm).unwrap();
        }
        let at = |x: usize, y: usize| [px[(y * 96 + x) * 4], px[(y * 96 + x) * 4 + 1], px[(y * 96 + x) * 4 + 2]];
        // the sky at the top is blue-ish, the cube in the middle is lit and not the sky
        let sky = at(48, 2);
        assert!(sky[2] > sky[0], "sky {sky:?}");
        let mid = at(48, 34);
        assert!(mid != sky && mid[0] > 60, "cube {mid:?}");
    }
}
