//! The drawing canvas: GPU lines and fills through an `egui_wgpu` paint callback, then texts and frames as
//! egui shapes on top. Model space draws the model; a sheet draws its paper, each viewport (the model,
//! clipped, scaled and twisted) and then its own entities.

use std::collections::HashMap;
use std::sync::Arc;

use cc_drawing::{Color, Drawing, SpaceKind, Viewport};
use cc_render::{Renderer, SpaceGeometry, SpaceMesh, View2D, ViewUniform};
use egui::{Color32, Pos2, Rect};

use crate::theme::Tokens;

/// GPU objects kept in egui_wgpu's callback resources.
pub struct GpuState {
    pub renderer: Renderer,
    /// (load id, space index) → mesh
    pub meshes: HashMap<(u64, usize), SpaceMesh>,
}

/// Install the renderer once the wgpu render state exists (desktop and web hosts call this at start-up).
pub fn install_gpu(rs: &egui_wgpu::RenderState) {
    let renderer = Renderer::new(&rs.device, rs.target_format, 1);
    rs.renderer.write().callback_resources.insert(GpuState { renderer, meshes: HashMap::new() });
    crate::view3d::install(rs);
}

#[derive(Clone)]
struct DrawItem {
    space: usize,
    slot: u32,
    uniform: ViewUniform,
    /// physical pixels in the framebuffer: x, y, w, h
    scissor: Option<[u32; 4]>,
}

struct CanvasCallback {
    id: u64,
    geometry: Arc<Vec<SpaceGeometry>>,
    draws: Vec<DrawItem>,
    /// per space, per batch
    visible: Arc<Vec<Vec<bool>>>,
}

impl egui_wgpu::CallbackTrait for CanvasCallback {
    fn prepare(
        &self,
        device: &egui_wgpu::wgpu::Device,
        queue: &egui_wgpu::wgpu::Queue,
        _sd: &egui_wgpu::ScreenDescriptor,
        _enc: &mut egui_wgpu::wgpu::CommandEncoder,
        res: &mut egui_wgpu::CallbackResources,
    ) -> Vec<egui_wgpu::wgpu::CommandBuffer> {
        let Some(gpu) = res.get_mut::<GpuState>() else { return Vec::new() };
        // keep only this drawing's meshes
        gpu.meshes.retain(|(id, _), _| *id == self.id);
        for d in &self.draws {
            if !gpu.meshes.contains_key(&(self.id, d.space))
                && let Some(g) = self.geometry.get(d.space)
            {
                gpu.meshes.insert((self.id, d.space), SpaceMesh::upload(device, g));
            }
            gpu.renderer.set_view(queue, d.slot, &d.uniform);
        }
        Vec::new()
    }

    fn paint(&self, info: egui::PaintCallbackInfo, pass: &mut egui_wgpu::wgpu::RenderPass<'static>, res: &egui_wgpu::CallbackResources) {
        let Some(gpu) = res.get::<GpuState>() else { return };
        let clip = info.clip_rect_in_pixels();
        let (cx, cy, cw, ch) =
            (clip.left_px.max(0) as u32, clip.top_px.max(0) as u32, clip.width_px.max(0) as u32, clip.height_px.max(0) as u32);
        for d in &self.draws {
            let Some(mesh) = gpu.meshes.get(&(self.id, d.space)) else { continue };
            match d.scissor {
                Some([x, y, w, h]) => {
                    // intersect with the canvas clip
                    let x0 = x.max(cx);
                    let y0 = y.max(cy);
                    let x1 = (x.saturating_add(w)).min(cx.saturating_add(cw));
                    let y1 = (y.saturating_add(h)).min(cy.saturating_add(ch));
                    if x1 <= x0 || y1 <= y0 {
                        continue;
                    }
                    pass.set_scissor_rect(x0, y0, x1 - x0, y1 - y0);
                }
                None => pass.set_scissor_rect(cx, cy, cw, ch),
            }
            let vis = self.visible.get(d.space);
            gpu.renderer.draw(pass, mesh, d.slot, &|b| vis.and_then(|v| v.get(b)).copied().unwrap_or(true));
        }
        pass.set_scissor_rect(cx, cy, cw, ch);
    }
}

/// Everything the canvas needs to draw one frame.
pub struct Frame<'a> {
    pub drawing: &'a Drawing,
    pub geometry: &'a Arc<Vec<SpaceGeometry>>,
    pub id: u64,
    pub space: usize,
    pub view: View2D,
    pub hidden: &'a std::collections::HashSet<String>,
    pub tokens: &'a Tokens,
    pub line_px: f32,
    /// the picture files loaded as textures, by the name a drawing gives them (`SpaceImage::file`)
    pub pictures: &'a HashMap<String, egui::TextureId>,
    /// a point cloud under the drawing (cloudview.rs): its pictures, where they go and their clips
    pub underlay: &'a [(egui::TextureId, Rect, Rect)],
}

fn rgba(c: Color32) -> [f32; 4] {
    let f = |u: u8| f32::from(u) / 255.0;
    [f(c.r()), f(c.g()), f(c.b()), 1.0]
}
fn colour32(c: Color, ink: Color32) -> Color32 {
    match c {
        Color::Ink => ink,
        Color::Rgb(n) => Color32::from_rgb(((n >> 16) & 0xFF) as u8, ((n >> 8) & 0xFF) as u8, (n & 0xFF) as u8),
    }
}

/// Model view for a sheet viewport (JS vpMatrix: paper = k·R(twist)·(model − v) + c).
pub fn viewport_view(sheet: &View2D, vp: &Viewport) -> View2D {
    let k = if vp.view_height != 0.0 { vp.h / vp.view_height } else { 1.0 };
    let (sx, sy) = sheet.to_screen(vp.cx, vp.cy);
    let mut v =
        View2D { cx: vp.vx, cy: vp.vy, zoom: sheet.zoom * k, twist: sheet.twist + vp.twist, width: sheet.width, height: sheet.height };
    v.pan(sx - sheet.width / 2.0, sy - sheet.height / 2.0);
    v
}

/// Stats of the frame (for the status bar).
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    pub draw_items: usize,
    pub texts: usize,
}

/// Paint one frame of the canvas into `rect` (points). Returns stats.
pub fn paint(ui: &egui::Ui, rect: Rect, f: &Frame) -> FrameStats {
    let ppp = ui.ctx().pixels_per_point();
    let painter = ui.painter_at(rect);
    let t = f.tokens;
    let Some(space) = f.drawing.spaces.get(f.space) else { return FrameStats::default() };
    let paper = space.kind == SpaceKind::Paper;
    painter.rect_filled(rect, 0.0, if paper { t.desk } else { t.canvas_bg });
    let ink = if paper { t.sheet_ink } else { t.ink };
    // views are in physical pixels
    let view = f.view;
    let to_pt = |v: &View2D, x: f64, y: f64| {
        let (sx, sy) = v.to_screen(x, y);
        Pos2::new(rect.min.x + (sx as f32) / ppp, rect.min.y + (sy as f32) / ppp)
    };
    if paper && let Some(p) = space.paper {
        let a = to_pt(&view, p.min_x, p.max_y);
        let b = to_pt(&view, p.max_x, p.min_y);
        painter.rect_filled(Rect::from_two_pos(a, b), 0.0, t.sheet);
    }
    // visibility per batch
    let visible: Vec<Vec<bool>> =
        f.drawing.spaces.iter().map(|s| s.batches.iter().map(|b| !f.hidden.contains(&b.layer)).collect()).collect();
    let mut draws = Vec::new();
    let mut slot = 0u32;
    let model = f.drawing.spaces.iter().position(|s| s.kind == SpaceKind::Model);
    let mut vp_views = Vec::new();
    if paper && let Some(mi) = model {
        let morigin = f.geometry.get(mi).map(|g| g.origin).unwrap_or((0.0, 0.0));
        for vp in &space.viewports {
            if slot as u64 >= cc_render::SLOTS - 1 {
                break;
            }
            let mv = viewport_view(&view, vp);
            // scissor = the viewport rectangle on screen (axis-aligned box of the sheet rectangle)
            let corners = [
                view.to_screen(vp.cx - vp.w / 2.0, vp.cy - vp.h / 2.0),
                view.to_screen(vp.cx + vp.w / 2.0, vp.cy + vp.h / 2.0),
                view.to_screen(vp.cx - vp.w / 2.0, vp.cy + vp.h / 2.0),
                view.to_screen(vp.cx + vp.w / 2.0, vp.cy - vp.h / 2.0),
            ];
            let minx = corners.iter().map(|c| c.0).fold(f64::INFINITY, f64::min);
            let maxx = corners.iter().map(|c| c.0).fold(f64::NEG_INFINITY, f64::max);
            let miny = corners.iter().map(|c| c.1).fold(f64::INFINITY, f64::min);
            let maxy = corners.iter().map(|c| c.1).fold(f64::NEG_INFINITY, f64::max);
            let ox = f64::from(rect.min.x * ppp);
            let oy = f64::from(rect.min.y * ppp);
            let sc =
                [(ox + minx).max(0.0) as u32, (oy + miny).max(0.0) as u32, (maxx - minx).max(0.0) as u32, (maxy - miny).max(0.0) as u32];
            draws.push(DrawItem { space: mi, slot, uniform: ViewUniform::new(&mv, morigin, rgba(ink), f.line_px), scissor: Some(sc) });
            vp_views.push((mv, minx, miny, maxx, maxy));
            slot += 1;
        }
    }
    // a point cloud under the drawing (view.js underlay: after the background, before the linework)
    crate::cloudview::paint(&painter, f.underlay);
    // pictures under the linework (pictures.js draws them after the fills): the model's inside each viewport, then the
    // space's own
    if paper && let Some(mi) = model {
        for (mv, x0, y0, x1, y1) in &vp_views {
            let clip = Rect::from_min_max(
                Pos2::new(rect.min.x + (*x0 as f32) / ppp, rect.min.y + (*y0 as f32) / ppp),
                Pos2::new(rect.min.x + (*x1 as f32) / ppp, rect.min.y + (*y1 as f32) / ppp),
            );
            let vpp = painter.with_clip_rect(clip.intersect(rect));
            for im in f.drawing.spaces.get(mi).map(|s| s.images.as_slice()).unwrap_or(&[]) {
                if f.hidden.contains(&im.layer) {
                    continue;
                }
                if let Some(tex) = f.pictures.get(&im.file)
                    && let Some(mesh) = crate::picmesh::picture_mesh(im, *tex, &|x, y| to_pt(mv, x, y))
                {
                    vpp.add(egui::Shape::mesh(mesh));
                }
            }
        }
    }
    for im in &space.images {
        if f.hidden.contains(&im.layer) {
            continue;
        }
        if let Some(tex) = f.pictures.get(&im.file)
            && let Some(mesh) = crate::picmesh::picture_mesh(im, *tex, &|x, y| to_pt(&view, x, y))
        {
            painter.add(egui::Shape::mesh(mesh));
        }
    }
    let origin = f.geometry.get(f.space).map(|g| g.origin).unwrap_or((0.0, 0.0));
    draws.push(DrawItem { space: f.space, slot, uniform: ViewUniform::new(&view, origin, rgba(ink), f.line_px), scissor: None });
    let n_draws = draws.len();
    let cb = CanvasCallback { id: f.id, geometry: Arc::clone(f.geometry), draws, visible: Arc::new(visible) };
    painter.add(egui_wgpu::Callback::new_paint_callback(rect, cb));
    // raster images whose file is not loaded: their frame (as the JS viewer shows a missing one); a drawing being
    // edited has the frame as an object of its own
    for im in &space.images {
        if f.hidden.contains(&im.layer) || im.clip.is_some() || f.pictures.contains_key(&im.file) {
            continue;
        }
        let m = im.m;
        let corner = |u: f64, v: f64| to_pt(&view, m[0] * u + m[2] * v + m[4], m[1] * u + m[3] * v + m[5]);
        let pts = vec![corner(0.0, 0.0), corner(im.w, 0.0), corner(im.w, im.h), corner(0.0, im.h), corner(0.0, 0.0)];
        painter.add(egui::Shape::line(pts, egui::Stroke::new(1.0, ink)));
    }
    // viewport frames
    for (_, x0, y0, x1, y1) in &vp_views {
        let r = Rect::from_min_max(
            Pos2::new(rect.min.x + (*x0 as f32) / ppp, rect.min.y + (*y0 as f32) / ppp),
            Pos2::new(rect.min.x + (*x1 as f32) / ppp, rect.min.y + (*y1 as f32) / ppp),
        );
        painter.rect_stroke(r, 0.0, egui::Stroke::new(1.0, t.viewport_frame), egui::StrokeKind::Inside);
    }
    // texts: the space's own, and the model's inside each viewport
    let mut texts = 0usize;
    let budget = 30_000usize;
    let mut draw_texts = |sp: &cc_drawing::Space, v: &View2D, clip: Rect| {
        let p2 = painter.with_clip_rect(clip);
        for b in &sp.batches {
            if f.hidden.contains(&b.layer) {
                continue;
            }
            let col = colour32(b.color, ink);
            for tx in &b.texts {
                if texts >= budget {
                    return;
                }
                let px_h = tx.h * v.zoom / f64::from(ppp);
                if !(3.0..=2000.0).contains(&px_h) {
                    continue;
                }
                let p = to_pt(v, tx.x, tx.y);
                if !clip.expand(px_h as f32 * 20.0).contains(p) {
                    continue;
                }
                let font = egui::FontId::proportional(px_h as f32 * 1.25);
                let galley = p2.layout_no_wrap(tx.s.clone(), font, col);
                let w = galley.size().x;
                let gh = galley.size().y;
                // anchor: ha 0 left 1 centre 2 right; va 0 baseline 1 bottom 2 middle 3 top
                let ax = match tx.ha {
                    1 => -w / 2.0,
                    2 => -w,
                    _ => 0.0,
                };
                let ay = match tx.va {
                    3 => 0.0,
                    2 => -gh / 2.0,
                    _ => -gh * 0.8,
                };
                let angle = -(tx.rot + v.twist) as f32;
                let rot = egui::emath::Rot2::from_angle(angle);
                let pos = p + rot * egui::vec2(ax, ay);
                p2.add(egui::epaint::TextShape::new(pos, galley, col).with_angle(angle));
                texts += 1;
            }
        }
    };
    if let Some(mi) = model
        && paper
    {
        for (vi, (mv, x0, y0, x1, y1)) in vp_views.iter().enumerate() {
            let _ = vi;
            let clip = Rect::from_min_max(
                Pos2::new(rect.min.x + (*x0 as f32) / ppp, rect.min.y + (*y0 as f32) / ppp),
                Pos2::new(rect.min.x + (*x1 as f32) / ppp, rect.min.y + (*y1 as f32) / ppp),
            )
            .intersect(rect);
            if let Some(ms) = f.drawing.spaces.get(mi) {
                draw_texts(ms, mv, clip);
            }
        }
    }
    draw_texts(space, &view, rect);
    FrameStats { draw_items: n_draws, texts }
}
