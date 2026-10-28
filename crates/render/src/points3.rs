//! Point clouds in the 3D view (pointcloud-three.js `CloudLayer` in CadCraft's 3D tab, 0.5 SG.1): every point a round
//! sprite of a fixed size in pixels, coloured by the cloud's own colours, depth-tested against the solids, optionally
//! cut to a slice (only the points within `half` of a plane). Drawn into a [`Target3`] after
//! [`crate::mesh3d::Renderer3::render`], on top of what it drew (same depth buffer).
//!
//! Large coordinates: the points are f32 offsets from the cloud's origin; the placement (file units → the view's
//! metres) and the camera are combined in f64 on the CPU, so survey coordinates keep their millimetres on the GPU.

use wgpu::util::DeviceExt;

use crate::mesh3d::{Cam3, TARGET_FORMAT, Target3, mmul, translation};

const SHADER: &str = r#"
struct Frame {
    // cloud offsets (file units) -> clip
    mvp: mat4x4<f32>,
    // the world z of a point = dot(zrow.xyz, p) + zrow.w (metres; the slice is along z)
    zrow: vec4<f32>,
    // x: point size (px), y, z: 2 / viewport size, w: 1 = a slice
    size: vec4<f32>,
    // x: the slice's height, y: half its thickness (metres)
    slice: vec4<f32>,
};
@group(0) @binding(0) var<uniform> fr: Frame;

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) col: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) d: f32,
};

fn lin(c: f32) -> f32 {
    // the target encodes sRGB: the cloud's bytes go in linear so they come out as they are (three.js ShaderMaterial)
    if c <= 0.04045 { return c / 12.92; }
    return pow((c + 0.055) / 1.055, 2.4);
}

@vertex
fn vs(@builtin(vertex_index) vi: u32, @location(0) p: vec3<f32>, @location(1) c: vec4<f32>) -> Out {
    var corners = array<vec2<f32>, 6>(vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0));
    let k = corners[vi];
    var o: Out;
    let clip = fr.mvp * vec4<f32>(p, 1.0);
    o.pos = vec4<f32>(clip.xy + k * fr.size.x * 0.5 * fr.size.yz * clip.w, clip.z, clip.w);
    o.col = vec3<f32>(lin(c.r), lin(c.g), lin(c.b));
    o.uv = k * 0.5;
    o.d = dot(fr.zrow.xyz, p) + fr.zrow.w - fr.slice.x;
    return o;
}

@fragment
fn fs(i: Out) -> @location(0) vec4<f32> {
    if fr.size.w > 0.5 && abs(i.d) > fr.slice.y { discard; }
    if dot(i.uv, i.uv) > 0.25 { discard; }
    return vec4<f32>(i.col, 1.0);
}
"#;

/// A cloud on the GPU: 16 bytes a point (x y z as f32 offsets, then r g b 255).
#[derive(Debug)]
pub struct GpuCloud {
    buf: Option<wgpu::Buffer>,
    pub count: u32,
}

impl GpuCloud {
    /// `pos`: 3 f32 per point; `rgb`: 3 bytes per point (missing ones grey).
    pub fn upload(device: &wgpu::Device, pos: &[f32], rgb: &[u8]) -> GpuCloud {
        let n = pos.len() / 3;
        let mut bytes: Vec<u8> = Vec::with_capacity(n.saturating_mul(16));
        for (k, p) in pos.as_chunks::<3>().0.iter().enumerate() {
            for v in p {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
            let c = rgb.get(k * 3..k * 3 + 3).unwrap_or(&[128, 128, 128]);
            bytes.extend_from_slice(c);
            bytes.push(255);
        }
        let buf = (!bytes.is_empty()).then(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("cc3 cloud"),
                contents: &bytes,
                usage: wgpu::BufferUsages::VERTEX,
            })
        });
        GpuCloud { buf, count: u32::try_from(n).unwrap_or(u32::MAX) }
    }
}

/// How a cloud is drawn this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CloudDraw {
    /// file offsets → the view's world (metres, z up), column-major (cloud-cad.js `matrix3d`)
    pub model: [f64; 16],
    /// point size in device pixels
    pub size: f64,
    /// the slice: its height (world z) and half its thickness
    pub slice: Option<(f64, f64)>,
}

/// The point pipeline.
pub struct Points3 {
    pipe: wgpu::RenderPipeline,
    frame: wgpu::Buffer,
    bind: wgpu::BindGroup,
}

impl std::fmt::Debug for Points3 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Points3")
    }
}

const FRAME: u64 = 64 + 16 * 3;

impl Points3 {
    pub fn new(device: &wgpu::Device) -> Points3 {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cc3 points"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cc3 points"),
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
            label: Some("cc3 points"),
            size: FRAME,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cc3 points"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: frame.as_entire_binding() }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cc3 points"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Unorm8x4];
        let pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cc3 points"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 16,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &attrs,
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: TARGET_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Greater),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState { count: crate::mesh3d::SAMPLES, mask: !0, alpha_to_coverage_enabled: false },
            multiview_mask: None,
            cache: None,
        });
        Points3 { pipe, frame, bind }
    }

    /// Draw `cloud` into `target` over what is there (seen by `cam`, sized as the target).
    pub fn render(&self, device: &wgpu::Device, queue: &wgpu::Queue, target: &Target3, cam: &Cam3, cloud: &GpuCloud, d: &CloudDraw) {
        let Some(buf) = &cloud.buf else { return };
        // proj · view rotation · T(−eye) · model, in f64: the big translations cancel before anything is f32
        let eye = translation([-cam.eye[0], -cam.eye[1], -cam.eye[2]]);
        let mvp = mmul(&cam.proj(), &mmul(&cam.view_rot(), &mmul(&eye, &d.model)));
        let mut fb: Vec<f32> = mvp.iter().map(|v| *v as f32).collect();
        // world z = row 2 of the model: m[2], m[6], m[10], m[14] (column-major)
        let m = &d.model;
        fb.extend_from_slice(&[m[2] as f32, m[6] as f32, m[10] as f32, m[14] as f32]);
        let (w, h) = (f64::from(target.width.max(1)), f64::from(target.height.max(1)));
        fb.extend_from_slice(&[d.size as f32, (2.0 / w) as f32, (2.0 / h) as f32, if d.slice.is_some() { 1.0 } else { 0.0 }]);
        let (at, half) = d.slice.unwrap_or((0.0, 0.0));
        fb.extend_from_slice(&[at as f32, half as f32, 0.0, 0.0]);
        let bytes: Vec<u8> = fb.iter().flat_map(|v| v.to_le_bytes()).collect();
        queue.write_buffer(&self.frame, 0, &bytes);
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("cc3 points") });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cc3 points"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.msaa,
                    depth_slice: None,
                    resolve_target: Some(&target.view),
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipe);
            pass.set_bind_group(0, &self.bind, &[]);
            pass.set_vertex_buffer(0, buf.slice(..));
            pass.draw(0..6, 0..cloud.count);
        }
        queue.submit([enc.finish()]);
    }
}
