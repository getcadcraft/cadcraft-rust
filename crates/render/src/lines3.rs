//! Lines in the 3D view's scene (ModelCraft's view3d.js buildExtras: the drawing axes and guide lines are three.js
//! lines in the scene, so the model in front of them hides them): one-pixel line segments in their own colours,
//! depth-tested against the solids. Drawn into a [`Target3`] after [`crate::mesh3d::Renderer3::render`], on top of
//! what it drew (same depth buffer).
//!
//! Large coordinates: the segments are f32 offsets from their own origin; the camera is combined with it in f64 on the
//! CPU, so survey coordinates keep their millimetres on the GPU.

use wgpu::util::DeviceExt;

use crate::mesh3d::{Cam3, TARGET_FORMAT, Target3, mmul, translation};

const SHADER: &str = r#"
struct Frame {
    // offsets from the lines' origin -> clip
    mvp: mat4x4<f32>,
};
@group(0) @binding(0) var<uniform> fr: Frame;

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) col: vec4<f32>,
};

fn lin(c: f32) -> f32 {
    // the target encodes sRGB: the colours go in linear so they come out as they are (three.js LineBasicMaterial)
    if c <= 0.04045 { return c / 12.92; }
    return pow((c + 0.055) / 1.055, 2.4);
}

@vertex
fn vs(@location(0) p: vec3<f32>, @location(1) c: vec4<f32>) -> Out {
    var o: Out;
    o.pos = fr.mvp * vec4<f32>(p, 1.0);
    // reversed z: a hair nearer, so a line lying on a face (an axis on the ground) is not hidden by it
    o.pos.z = o.pos.z * 1.0002;
    o.col = vec4<f32>(lin(c.r), lin(c.g), lin(c.b), c.a);
    return o;
}

@fragment
fn fs(i: Out) -> @location(0) vec4<f32> {
    return i.col;
}
"#;

/// Line segments to draw: pairs of points (world, f64) and a colour per segment (RGBA).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lines3 {
    pub segs: Vec<([f64; 3], [f64; 3], [u8; 4])>,
}

/// Segments on the GPU: 16 bytes a point (x y z as f32 offsets from `origin`, then r g b a).
#[derive(Debug)]
pub struct GpuLines {
    buf: Option<wgpu::Buffer>,
    origin: [f64; 3],
    count: u32,
}

impl GpuLines {
    pub fn upload(device: &wgpu::Device, lines: &Lines3) -> GpuLines {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for (a, b, _) in &lines.segs {
            for p in [a, b] {
                for ((l, h), v) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
                    *l = l.min(*v);
                    *h = h.max(*v);
                }
            }
        }
        let origin = if lo[0].is_finite() { [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0] } else { [0.0; 3] };
        let mut bytes: Vec<u8> = Vec::with_capacity(lines.segs.len().saturating_mul(32));
        for (a, b, c) in &lines.segs {
            for p in [a, b] {
                for (v, o) in p.iter().zip(origin) {
                    bytes.extend_from_slice(&((v - o) as f32).to_le_bytes());
                }
                bytes.extend_from_slice(c);
            }
        }
        let buf = (!bytes.is_empty()).then(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("cc3 lines"),
                contents: &bytes,
                usage: wgpu::BufferUsages::VERTEX,
            })
        });
        GpuLines { buf, origin, count: u32::try_from(lines.segs.len().saturating_mul(2)).unwrap_or(u32::MAX) }
    }
}

/// The line pipeline.
pub struct LinePass {
    pipe: wgpu::RenderPipeline,
    /// the same without the depth test: lines over everything (PartCraft's move / turn gizmo, as three.js's
    /// TransformControls draws with depthTest off)
    pipe_top: wgpu::RenderPipeline,
    frame: wgpu::Buffer,
    bind: wgpu::BindGroup,
}

impl std::fmt::Debug for LinePass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LinePass")
    }
}

const FRAME: u64 = 64;

impl LinePass {
    pub fn new(device: &wgpu::Device) -> LinePass {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cc3 lines"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cc3 lines"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(FRAME),
                },
                count: None,
            }],
        });
        let frame = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cc3 lines"),
            size: FRAME,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cc3 lines"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: frame.as_entire_binding() }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cc3 lines"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Unorm8x4];
        let make = |compare: wgpu::CompareFunction| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("cc3 lines"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: 16,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &attrs,
                    })],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: TARGET_FORMAT,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::LineList, cull_mode: None, ..Default::default() },
                // reversed z: nearer is greater (no depth bias: lines cannot take one; the shader moves them a hair nearer)
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(compare),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState { count: crate::mesh3d::SAMPLES, mask: !0, alpha_to_coverage_enabled: false },
                multiview_mask: None,
                cache: None,
            })
        };
        let pipe = make(wgpu::CompareFunction::GreaterEqual);
        let pipe_top = make(wgpu::CompareFunction::Always);
        LinePass { pipe, pipe_top, frame, bind }
    }

    /// Draw `lines` into `target` over what is there (seen by `cam`).
    pub fn render(&self, device: &wgpu::Device, queue: &wgpu::Queue, target: &Target3, cam: &Cam3, lines: &GpuLines) {
        self.draw(&self.pipe, device, queue, target, cam, lines);
    }

    /// Draw `lines` over everything in `target`, the solids in front of them too (no depth test).
    pub fn render_on_top(&self, device: &wgpu::Device, queue: &wgpu::Queue, target: &Target3, cam: &Cam3, lines: &GpuLines) {
        self.draw(&self.pipe_top, device, queue, target, cam, lines);
    }

    fn draw(
        &self,
        pipe: &wgpu::RenderPipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &Target3,
        cam: &Cam3,
        lines: &GpuLines,
    ) {
        let Some(buf) = &lines.buf else { return };
        // proj · view rotation · T(origin − eye), in f64: the big translations cancel before anything is f32
        let o = lines.origin;
        let shift = translation([o[0] - cam.eye[0], o[1] - cam.eye[1], o[2] - cam.eye[2]]);
        let mvp = mmul(&cam.proj(), &mmul(&cam.view_rot(), &shift));
        let bytes: Vec<u8> = mvp.iter().flat_map(|v| (*v as f32).to_le_bytes()).collect();
        queue.write_buffer(&self.frame, 0, &bytes);
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("cc3 lines") });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cc3 lines"),
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
            pass.set_pipeline(pipe);
            pass.set_bind_group(0, &self.bind, &[]);
            pass.set_vertex_buffer(0, buf.slice(..));
            pass.draw(0..lines.count, 0..1);
        }
        queue.submit([enc.finish()]);
    }
}
