//! A PDF plan set in the app (the sheet view MarkupCraft and TakeoffCraft are built on): pages listed by their labels,
//! the page rendered by hayro (cc-io-takeoff `render_rgba`, the renderer auto count reads) into one texture, pan and
//! zoom about the pointer. The texture is re-rendered when the zoom moves to another half-octave step, capped at
//! `MAX_SIDE` pixels a side; between renders the last one is drawn scaled.

use cc_io_takeoff::pdfread::{self, Doc, PageInfo};
use egui::{Color32, Pos2, Rect, Sense, Vec2};

use crate::theme::Tokens;

/// The largest side of a page texture (GPU limits allow more; memory and render time say this).
const MAX_SIDE: f64 = 8192.0;
/// Zoom limits (screen pixels per PDF point).
const ZOOM_MIN: f64 = 0.02;
const ZOOM_MAX: f64 = 64.0;

/// One open PDF.
pub struct PdfView {
    pub name: String,
    doc: Doc,
    pub pages: Vec<PageInfo>,
    pub page: usize,
    /// screen pixels (points of the UI) per PDF point
    zoom: f64,
    /// the page's top-left corner, from the view's top-left
    pan: Vec2,
    fitted: bool,
    /// the texture, the page it shows and the scale it was rendered at
    tex: Option<(egui::TextureHandle, usize, f64)>,
    /// how long the last render took (ms)
    pub render_ms: f64,
    pub error: Option<String>,
    /// the markups read back from the file (MarkupCraft's openPdf), when it has any
    pub markups: Vec<cc_markup::model::Markup>,
    /// each page's key ("f1:<index>") markups name their sheet by
    page_keys: Vec<String>,
    /// pages whose annotations MarkupCraft draws itself (annotMode "disable"): rendered without them while markups show
    annots_drawn: Vec<bool>,
    /// draw the markups over the sheet
    pub show_markups: bool,
}

impl std::fmt::Debug for PdfView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PdfView").field("name", &self.name).field("pages", &self.pages.len()).field("page", &self.page).finish()
    }
}

/// The render scale for a zoom: half-octave steps (so small zooms do not re-render), capped by `MAX_SIDE`.
pub fn render_scale(zoom_px: f64, page_w: f64, page_h: f64) -> f64 {
    let step = (zoom_px.max(1e-3).log2() * 2.0).ceil() / 2.0;
    let s = 2f64.powf(step);
    let cap = MAX_SIDE / page_w.max(page_h).max(1.0);
    s.min(cap).max(0.05)
}

impl PdfView {
    /// Open the bytes as a PDF (no scripts, no forms: the page tree and content only).
    pub fn open(name: &str, bytes: &[u8]) -> Result<PdfView, String> {
        let doc = pdfread::open(bytes)?;
        let pages = pdfread::pages(&doc);
        if pages.is_empty() {
            return Err("the PDF has no pages".into());
        }
        // MarkupCraft's reading of the same file: its markups (a file it cannot read still shows as a plan set)
        let mut env = cc_markup::model::Env::default();
        let opts = cc_io_markup::read::OpenOpts { file_id: "f1".into(), name: name.to_string(), read_text: false };
        let (markups, page_keys, annots_drawn) = match cc_io_markup::read::open_pdf(&mut env, bytes, &opts) {
            Ok(r) => {
                (r.markups, r.pages.iter().map(|p| p.key.clone()).collect(), r.pages.iter().map(|p| p.annot_mode == "disable").collect())
            }
            Err(_) => (Vec::new(), Vec::new(), Vec::new()),
        };
        Ok(PdfView {
            markups,
            page_keys,
            annots_drawn,
            show_markups: true,
            name: name.to_string(),
            doc,
            pages,
            page: 0,
            zoom: 1.0,
            pan: Vec2::ZERO,
            fitted: false,
            tex: None,
            render_ms: 0.0,
            error: None,
        })
    }

    fn size(&self) -> (f64, f64) {
        self.pages.get(self.page).map_or((612.0, 792.0), |p| (p.width, p.height))
    }

    /// Go to a page (0-based) and fit it.
    pub fn go(&mut self, i: usize) {
        if i < self.pages.len() && i != self.page {
            self.page = i;
            self.fitted = false;
        }
    }

    /// Fit the page in the view on the next frame.
    pub fn fit(&mut self) {
        self.fitted = false;
    }

    fn fit_to(&mut self, view: Vec2) {
        let (w, h) = self.size();
        let z = (f64::from(view.x) / w).min(f64::from(view.y) / h) * 0.95;
        self.zoom = z.clamp(ZOOM_MIN, ZOOM_MAX);
        let (sw, sh) = (w * self.zoom, h * self.zoom);
        self.pan = Vec2::new(((f64::from(view.x) - sw) / 2.0) as f32, ((f64::from(view.y) - sh) / 2.0) as f32);
        self.fitted = true;
    }

    /// Render the page at `scale` into the texture.
    fn render(&mut self, ctx: &egui::Context, scale: f64) {
        let t0 = web_time::Instant::now();
        let own = self.show_markups && self.annots_drawn.get(self.page).copied().unwrap_or(false);
        match pdfread::render_rgba(&self.doc, self.page, scale, !own) {
            Ok((rgba, w, h)) => {
                let img = egui::ColorImage::from_rgba_premultiplied([w, h], &rgba);
                let opts = egui::TextureOptions::LINEAR;
                match &mut self.tex {
                    Some((t, _, _)) => t.set(img, opts),
                    None => self.tex = Some((ctx.load_texture("pdf-page", img, opts), self.page, scale)),
                }
                if let Some(t) = &mut self.tex {
                    t.1 = self.page;
                    t.2 = scale;
                }
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
        self.render_ms = t0.elapsed().as_secs_f64() * 1000.0;
    }

    /// The sheet list (page labels).
    pub fn sheets(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let mut go: Option<usize> = None;
        if !self.markups.is_empty() {
            let label = format!("Markups ({} on this sheet, {} in all)", self.page_markups(), self.markups.len());
            if ui.checkbox(&mut self.show_markups, egui::RichText::new(label).color(t.text)).changed() {
                // the page is drawn with or without its annotations' own appearances
                self.tex = None;
            }
            ui.separator();
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (i, p) in self.pages.iter().enumerate() {
                let label = if p.label.is_empty() { format!("Page {}", i + 1) } else { p.label.clone() };
                if ui.selectable_label(i == self.page, egui::RichText::new(label).color(t.text)).clicked() {
                    go = Some(i);
                }
            }
        });
        if let Some(i) = go {
            self.go(i);
        }
    }

    /// The page in `rect`: drag pans (any button), the wheel zooms about the pointer, Page Up / Down turn pages.
    pub fn show(&mut self, ui: &mut egui::Ui, rect: Rect, t: &Tokens) {
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        ui.painter().rect_filled(rect, 0.0, t.canvas_bg);
        if !self.fitted {
            self.fit_to(rect.size());
        }
        if resp.dragged() {
            self.pan += resp.drag_delta();
        }
        if let Some(p) = resp.hover_pos() {
            let (scroll, zoom_delta) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let f = f64::from(zoom_delta) * (f64::from(scroll) / 400.0).exp();
            if (f - 1.0).abs() > 1e-6 {
                let nz = (self.zoom * f).clamp(ZOOM_MIN, ZOOM_MAX);
                let k = (nz / self.zoom) as f32;
                let at = p - rect.min;
                self.pan = at - (at - self.pan) * k;
                self.zoom = nz;
            }
        }
        let (prev, next) = ui.input(|i| (i.key_pressed(egui::Key::PageUp), i.key_pressed(egui::Key::PageDown)));
        if prev && self.page > 0 {
            self.go(self.page - 1);
        }
        if next {
            self.go(self.page + 1);
        }
        let (w, h) = self.size();
        let ppp = f64::from(ui.ctx().pixels_per_point());
        let want = render_scale(self.zoom * ppp, w, h);
        let stale = self.tex.as_ref().is_none_or(|(_, pg, s)| *pg != self.page || (*s - want).abs() > 1e-9);
        if stale {
            self.render(ui.ctx(), want);
        }
        let page_rect = Rect::from_min_size(rect.min + self.pan, Vec2::new((w * self.zoom) as f32, (h * self.zoom) as f32));
        let painter = ui.painter_at(rect);
        painter.rect_filled(page_rect.expand(1.0), 0.0, Color32::from_gray(90));
        if let Some((tex, _, _)) = &self.tex {
            painter.image(tex.id(), page_rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        if self.show_markups {
            self.draw_markups(&painter, page_rect.min);
        }
        if let Some(e) = &self.error {
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, e, egui::FontId::proportional(14.0), t.text);
        }
    }

    /// The markups of this page, drawn from MarkupCraft's display lists (shapes.js: the same list the saved PDF's
    /// appearance streams are made from), PDF user space → the screen through the page's viewport transform.
    fn draw_markups(&self, painter: &egui::Painter, origin: Pos2) {
        use cc_markup::shapes::{Item, ShapeEnv, flatten, place_text, shapes};
        let (Some(key), Some(info)) = (self.page_keys.get(self.page), self.pages.get(self.page)) else { return };
        let tr = info.transform;
        let z = self.zoom;
        let at = |x: f64, y: f64| {
            let (vx, vy) = pdfread::to_viewport(&tr, x, y);
            origin + Vec2::new((vx * z) as f32, (vy * z) as f32)
        };
        let env = ShapeEnv { cal: None, badge: 0.0, legend: None };
        for m in self.markups.iter().filter(|m| m.sheet.as_deref() == Some(key.as_str())) {
            for item in shapes(m, &env) {
                match item {
                    Item::Path(p) => {
                        for sub in flatten(&p.d) {
                            let pts: Vec<Pos2> = sub.pts.iter().map(|q| at(q[0], q[1])).collect();
                            if pts.len() < 2 {
                                continue;
                            }
                            if sub.closed
                                && pts.len() >= 3
                                && let Some(f) = p.fill.as_deref().and_then(|c| hex_color(c, p.falpha))
                            {
                                painter.add(egui::Shape::convex_polygon(pts.clone(), f, egui::Stroke::NONE));
                            }
                            if let Some(c) = p.stroke.as_deref().and_then(|c| hex_color(c, p.alpha)) {
                                let mut line = pts;
                                if sub.closed
                                    && let Some(first) = line.first().copied()
                                {
                                    line.push(first);
                                }
                                let w = ((p.w * z) as f32).max(1.0);
                                painter.add(egui::Shape::line(line, egui::Stroke::new(w, c)));
                            }
                        }
                    }
                    Item::Text(t) => {
                        let Some(c) = hex_color(&t.color, t.alpha) else { continue };
                        let (lines, _angle) = place_text(&t);
                        let size = ((t.size * z) as f32).max(4.0);
                        for l in lines {
                            painter.text(at(l.x, l.y), egui::Align2::LEFT_BOTTOM, &l.s, egui::FontId::proportional(size), c);
                        }
                    }
                    Item::Image(im) => {
                        let (a, b) = (at(im.bbox[0], im.bbox[1]), at(im.bbox[2], im.bbox[3]));
                        painter.rect_stroke(
                            Rect::from_two_pos(a, b),
                            0.0,
                            egui::Stroke::new(1.0, Color32::from_gray(140)),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
            }
        }
    }

    /// How many markups the open page has.
    pub fn page_markups(&self) -> usize {
        let key = self.page_keys.get(self.page);
        self.markups.iter().filter(|m| m.sheet.as_ref() == key).count()
    }

    /// What the status bar says.
    pub fn status(&self) -> String {
        let p = self.pages.get(self.page);
        let label = p.map(|p| p.label.clone()).filter(|l| !l.is_empty()).unwrap_or_else(|| format!("{}", self.page + 1));
        format!(
            "{}: sheet {label} ({} of {}) · {:.0} × {:.0} pt · zoom {:.0}% · render {:.0} ms · drag pans, wheel zooms, Page Up / Down",
            self.name,
            self.page + 1,
            self.pages.len(),
            p.map_or(0.0, |p| p.width),
            p.map_or(0.0, |p| p.height),
            self.zoom * 100.0,
            self.render_ms
        )
    }
}

/// "#rrggbb" (or "#rgb") with an alpha → a colour; None for anything else (a markup's colours are data from the file).
pub fn hex_color(s: &str, alpha: f64) -> Option<Color32> {
    let h = s.strip_prefix('#')?;
    let full: String = match h.len() {
        3 => h.chars().flat_map(|c| [c, c]).collect(),
        6 => h.to_string(),
        _ => return None,
    };
    let v = u32::from_str_radix(&full, 16).ok()?;
    let a = if alpha.is_finite() { alpha.clamp(0.0, 1.0) } else { 1.0 };
    Some(Color32::from_rgba_unmultiplied((v >> 16) as u8, (v >> 8) as u8, v as u8, (a * 255.0).round() as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_colours() {
        assert_eq!(hex_color("#e5322d", 1.0), Some(Color32::from_rgb(0xe5, 0x32, 0x2d)));
        assert_eq!(hex_color("#fff", 1.0), Some(Color32::WHITE));
        assert_eq!(hex_color("red", 1.0), None);
        assert_eq!(hex_color("#12345", 1.0), None);
    }

    #[test]
    fn render_scale_steps_and_caps() {
        assert_eq!(render_scale(1.0, 612.0, 792.0), 1.0);
        assert_eq!(render_scale(1.1, 612.0, 792.0), 2f64.sqrt());
        // a huge zoom on an A0 sheet stops at the side cap
        let s = render_scale(40.0, 3370.0, 2384.0);
        assert!((3370.0 * s - MAX_SIDE).abs() < 1e-6, "{s}");
    }
}
