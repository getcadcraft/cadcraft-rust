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
        Ok(PdfView {
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
        match pdfread::render_rgba(&self.doc, self.page, scale) {
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
        if let Some(e) = &self.error {
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, e, egui::FontId::proportional(14.0), t.text);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_scale_steps_and_caps() {
        assert_eq!(render_scale(1.0, 612.0, 792.0), 1.0);
        assert_eq!(render_scale(1.1, 612.0, 792.0), 2f64.sqrt());
        // a huge zoom on an A0 sheet stops at the side cap
        let s = render_scale(40.0, 3370.0, 2384.0);
        assert!((3370.0 * s - MAX_SIDE).abs() < 1e-6, "{s}");
    }
}
