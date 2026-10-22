//! The 3D view's marks as view3d.js draws them (buildExtras + drawOverlay): the open context's drawing axes (solid on
//! the positive side, dashed on the negative, red / green / blue), guide lines (dashed) and guide points (crosses), the
//! open group's box, dimensions (linear with architectural ticks, radius / diameter / angle with arrowheads, the text
//! in a box), section planes (an outline, a light fill and four arrows toward the side cut away; the active one in
//! ochre), a tool's dimension or section being placed, and the selection's dimensions, sections and guides. Presentation
//! view leaves only the dimensions. Layouts are cc-m3d-tools' infer.js port (dimLayout, sectionLayout); the drawing is
//! the page's canvas code. Unlike 0.5 (where axes and guide lines are lines in the 3D scene), they are drawn over the
//! model.

use cc_m3d::model::{Guide, Radial};
use cc_m3d::vec::{IDENT, P3, add, mul, sub};
use cc_m3d_tools::infer::{
    DimLayout, SectionLayout, angle_layout, dim_layout, dim_text, dim_text_box, lin_layout, radial_layout, section_layout,
};
use cc_m3d_tools::overlay::DimOv;
use egui::{Color32, Painter, Pos2, Rect, Stroke};

use crate::modelcraft::ModelView;

/// view3d.js's colours (`--ink`, `--ink-2`, the layers' red, green, blue, ochre), light and dark.
pub struct Ink {
    pub bg: Color32,
    pub ink: Color32,
    pub ink2: Color32,
    pub sel: Color32,
    pub red: Color32,
    pub green: Color32,
    pub blue: Color32,
    pub section: Color32,
}

impl Ink {
    pub fn new(dark: bool) -> Ink {
        let c = |v: u32| Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8);
        if dark {
            Ink {
                bg: c(0x1d232b),
                ink: c(0xe6ebf0),
                ink2: c(0xb4bfca),
                sel: c(0x6b8cff),
                red: c(0xe0485e),
                green: c(0x3fb26a),
                blue: c(0x6b8cff),
                section: c(0xd99a4a),
            }
        } else {
            Ink {
                bg: c(0xf3f6f8),
                ink: c(0x17202b),
                ink2: c(0x34404e),
                sel: c(0x1e48c8),
                red: c(0xb3122e),
                green: c(0x17703a),
                blue: c(0x1e48c8),
                section: c(0xa8641a),
            }
        }
    }
}

fn alpha(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a * 255.0) as u8)
}

fn apply(m: &[f64; 16], p: P3) -> P3 {
    [
        m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
        m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
        m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
    ]
}

/// One painter for the marks: projection with the near plane clipped (view3d.js seg) and the canvas helpers.
struct Pen<'a> {
    painter: &'a Painter,
    rect: Rect,
    cam: &'a cc_m3d_tools::Camera,
    vp: [f64; 16],
}

impl Pen<'_> {
    fn pt(&self, p: P3) -> Option<Pos2> {
        self.cam.project_vp(p, &self.vp).map(|q| Pos2::new(self.rect.min.x + q[0] as f32, self.rect.min.y + q[1] as f32))
    }
    fn xy(&self, q: &[f64]) -> Option<Pos2> {
        Some(Pos2::new(self.rect.min.x + *q.first()? as f32, self.rect.min.y + *q.get(1)? as f32))
    }
    /// A segment clipped to the near plane before projecting, so lines through the eye still draw.
    fn seg(&self, mut a: P3, mut b: P3) -> Option<(Pos2, Pos2)> {
        let c = self.cam;
        if c.persp {
            let z = c.axes().z;
            let e = c.eye;
            let near = c.near * 1.01;
            let d = |p: P3| -((p[0] - e[0]) * z[0] + (p[1] - e[1]) * z[1] + (p[2] - e[2]) * z[2]);
            let (da, db) = (d(a), d(b));
            if da < near && db < near {
                return None;
            }
            if da < near {
                a = add(a, mul(sub(b, a), (near - da) / (db - da)));
            } else if db < near {
                b = add(b, mul(sub(a, b), (near - db) / (da - db)));
            }
        }
        Some((self.pt(a)?, self.pt(b)?))
    }
    fn line(&self, a: P3, b: P3, color: Color32, width: f32, dash: Option<(f32, f32)>) {
        let Some((p, q)) = self.seg(a, b) else { return };
        let st = Stroke::new(width, color);
        match dash {
            Some((on, off)) => {
                self.painter.extend(egui::Shape::dashed_line(&[p, q], st, on, off));
            }
            None => {
                self.painter.line_segment([p, q], st);
            }
        }
    }
    fn cross(&self, p: Option<Pos2>, color: Color32) {
        let Some(s) = p else { return };
        let st = Stroke::new(1.5, color);
        self.painter.line_segment([s + egui::vec2(-5.0, -5.0), s + egui::vec2(5.0, 5.0)], st);
        self.painter.line_segment([s + egui::vec2(5.0, -5.0), s + egui::vec2(-5.0, 5.0)], st);
    }
    fn arrowhead(&self, tip: Pos2, from: Pos2, k: f32, h: f32, color: Color32) {
        let d = from - tip;
        let n = d.length();
        if n < 1.0 {
            return;
        }
        let u = d / n;
        let pts = vec![tip, tip + u * k + egui::vec2(-u.y, u.x) * h, tip + u * k - egui::vec2(-u.y, u.x) * h];
        self.painter.add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
    }
    fn text_box(&self, m: &[f64], text: &str, color: Color32, ink: &Ink) {
        let (x, y, w, h) = dim_text_box(m, text);
        let Some(tl) = self.xy(&[x, y]) else { return };
        let r = Rect::from_min_size(tl, egui::vec2(w as f32, h as f32));
        self.painter.rect_filled(r, 0.0, ink.bg);
        self.painter.rect_stroke(r.shrink(0.5), 0.0, Stroke::new(1.0, color), egui::StrokeKind::Inside);
        self.painter.text(r.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(12.0), color);
    }
    /// drawDim: extension lines, the dimension line with 45° ticks, or the round kinds; the text in a box.
    fn dim(&self, l: &DimLayout, units: &str, color: Color32, width: f32, ink: &Ink) {
        let fmt = |v: f64| cc_m3d::units::format_length(v, units);
        if matches!(l.kind.as_str(), "rad" | "dia" | "ang") {
            for (a, b) in &l.lines {
                self.line(*a, *b, color, (width - 0.5).max(1.0), None);
            }
            let arc: Vec<Pos2> = l.arc.iter().filter_map(|p| self.pt(*p)).collect();
            if arc.len() == l.arc.len() && arc.len() >= 2 {
                self.painter.add(egui::Shape::line(arc, Stroke::new(width, color)));
            }
            for (tip, from) in &l.arrows {
                if let (Some(a), Some(b)) = (self.pt(*tip), self.pt(*from)) {
                    self.arrowhead(a, b, 9.0, 3.2, color);
                }
            }
            if let Some(m) = &l.m {
                self.text_box(m, &dim_text(l, &fmt), color, ink);
            }
            return;
        }
        let st = Stroke::new(width, color);
        for (p, q) in [(l.pa, l.qa), (l.pb, l.qb)] {
            let (Some(p), Some(q)) = (p, q) else { continue };
            let Some((s0, s1)) = self.seg(p, q) else { continue };
            let d = s1 - s0;
            let n = d.length();
            if n < 1.0 {
                continue;
            }
            let u = d / n;
            let gap = (n / 3.0).min(4.0);
            self.painter.line_segment([s0 + u * gap, s1 + u * 5.0], st);
        }
        let (Some(_), Some(_), Some(qa), Some(qb)) = (l.a, l.b, l.qa, l.qb) else { return };
        let Some((s0, s1)) = self.seg(qa, qb) else { return };
        self.painter.line_segment([s0, s1], st);
        let d = s1 - s0;
        let n = d.length().max(1.0);
        let t = egui::vec2(d.x - d.y, d.y + d.x) / n * 4.5;
        for e in [s0, s1] {
            self.painter.line_segment([e - t, e + t], Stroke::new(width + 0.75, color));
        }
        if let Some(m) = &l.m {
            self.text_box(m, &fmt(l.l), color, ink);
        }
    }
    /// drawSection: the outline, a light fill, four arrows toward the side cut away.
    fn section(&self, l: &SectionLayout, color: Color32, width: f32, dashed: bool) {
        let pts: Vec<Pos2> = l.corners.iter().filter_map(|p| self.pt(*p)).collect();
        if pts.len() != 4 {
            return;
        }
        self.painter.add(egui::Shape::convex_polygon(pts.clone(), alpha(color, 0.06), Stroke::NONE));
        let st = Stroke::new(width, color);
        if dashed {
            let mut c = pts.clone();
            c.extend(pts.first().copied());
            self.painter.extend(egui::Shape::dashed_line(&c, st, 6.0, 4.0));
        } else {
            self.painter.add(egui::Shape::closed_line(pts, st));
        }
        for (a, b) in &l.arrows {
            let (Some(pa), Some(pb)) = (self.pt(*a), self.pt(*b)) else { continue };
            self.painter.line_segment([pa, pb], st);
            if (pa - pb).length() >= 4.0 {
                self.arrowhead(pb, pa, 8.0, 3.5, color);
            }
        }
    }
}

/// A dimension being placed as a model dimension, for dimLayout.
fn ov_layout(d: &DimOv, cam: &cc_m3d_tools::Camera, vp: &[f64; 16]) -> DimLayout {
    match d {
        DimOv::Lin { a, b, off } => lin_layout(*a, *b, *off, cam, vp),
        DimOv::Ang { o, a, b, off } => angle_layout(*o, *a, *b, *off, cam, vp),
        DimOv::Rad { kind, p, r, n, off } => {
            let g = Radial { c: *p, r: *r, n: *n, live: true, flat: false, arc: false };
            radial_layout(Some(&g), kind, *off, cam, vp)
        }
    }
}

impl ModelView {
    /// The model's marks under the tools' overlay (view3d.js buildExtras' axes and guide lines, drawOverlay's box,
    /// guide points, dimensions and section planes); `present`: dimensions only.
    pub fn marks(&self, painter: &Painter, rect: Rect, dark: bool) {
        let ink = Ink::new(dark);
        let model = &self.tools.host.model;
        let cam = &self.tools.host.cam;
        let pen = Pen { painter, rect, cam, vp: cam.view_proj() };
        let present = self.presenting;
        let path = &self.tools.host.path;
        let r = model.bbox("root", &IDENT).map_or(30.0, |b| cc_m3d::vec::dist(b.min, b.max).max(10.0));
        let big = r * 4.0;
        if !present {
            // the drawing axes of the open context
            let am = model.axes_frame(path);
            let o = apply(&am, [0.0; 3]);
            for (dir, c) in [([1.0, 0.0, 0.0], ink.red), ([0.0, 1.0, 0.0], ink.green), ([0.0, 0.0, 1.0], ink.blue)] {
                pen.line(o, apply(&am, mul(dir, big)), alpha(c, 0.8), 1.0, None);
                pen.line(o, apply(&am, mul(dir, -big)), alpha(c, 0.6), 1.0, Some((6.0, 6.0)));
            }
            // guide lines and points
            for (_, g) in model.guides.iter() {
                match g {
                    Guide::Line { p, d } => {
                        pen.line(add(*p, mul(*d, -big)), add(*p, mul(*d, big)), alpha(ink.ink2, 0.75), 1.0, Some((5.0, 4.0)))
                    }
                    Guide::Point { p } => pen.cross(pen.pt(*p), ink.ink2),
                }
            }
            // the open group's box
            if !path.is_empty()
                && let Some(ctx) = model.context(path)
                && let Some(bb) = model.bbox(&ctx.def, &ctx.m)
            {
                let c = |i: usize| -> P3 {
                    [
                        if i & 1 == 0 { bb.min[0] } else { bb.max[0] },
                        if i & 2 == 0 { bb.min[1] } else { bb.max[1] },
                        if i & 4 == 0 { bb.min[2] } else { bb.max[2] },
                    ]
                };
                for (a, b) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
                    pen.line(c(a), c(b), ink.ink2, 1.0, Some((4.0, 4.0)));
                }
            }
        }
        // dimensions (on top of everything: notes about the model), laid out on copies so the model is left as it is
        let units = model.units.clone();
        for (_, dm) in model.dims.iter() {
            let mut d = dm.clone();
            let l = dim_layout(model, &mut d, cam, &pen.vp);
            pen.dim(&l, &units, ink.ink, 1.25, &ink);
        }
        if present {
            return;
        }
        if model.section_view.planes {
            for (id, s) in model.sections.iter() {
                let active = model.active_section == Some(id);
                pen.section(
                    &section_layout(model, s.p, s.n),
                    if active { ink.section } else { ink.ink2 },
                    if active { 1.75 } else { 1.25 },
                    false,
                );
            }
        }
        // the selection's dimensions, sections and guides; a tool's dimension or section being placed
        for s in &self.tools.host.sel {
            match s.t.as_str() {
                "d" => {
                    if let Some(dm) = model.dims.get(s.id) {
                        let mut d = dm.clone();
                        let l = dim_layout(model, &mut d, cam, &pen.vp);
                        pen.dim(&l, &units, ink.sel, 2.0, &ink);
                    }
                }
                "s" => {
                    if let Some(sp) = model.sections.get(s.id) {
                        pen.section(&section_layout(model, sp.p, sp.n), ink.sel, 2.5, false);
                    }
                }
                "g" => match model.guides.get(s.id) {
                    Some(Guide::Point { p }) => pen.cross(pen.pt(*p), ink.sel),
                    Some(Guide::Line { p, d }) => pen.line(add(*p, mul(*d, -big)), add(*p, mul(*d, big)), ink.sel, 2.5, Some((8.0, 4.0))),
                    None => {}
                },
                _ => {}
            }
        }
        if let Some(o) = &self.tools.host.overlay {
            if let Some((p, n)) = o.section {
                pen.section(&section_layout(model, p, n), ink.sel, 1.5, true);
            }
            for d in o.dims.iter().flatten() {
                let l = ov_layout(d, cam, &pen.vp);
                pen.dim(&l, &units, ink.sel, 1.5, &ink);
            }
        }
    }
}
