//! TakeoffCraft's snapping to the drawing's own linework, 45° steps and typed lengths (M9.26), ported from
//! takeoffcraft.js: pointFor (a pick lands on the PDF's linework when Snap is on and a line is within 10 screen pixels —
//! cc-takeoff snap `SnapIndex` over cc-io-takeoff `page_linework`, read once per sheet, four sheets kept — else 45° from
//! the last point with Shift, or without it when Settings › Snap to 45° is on), the snap marker (square = end, triangle
//! = midpoint, X = crossing, hourglass = on the line, with its name), F3 / the Snap toggle (toggleSnap), and the typed
//! entry of a run, outline, cut-out or circle (openTypeIn / applyTyped: cc-takeoff typein `parse_entry`).

use std::collections::HashMap;
use std::sync::Arc;

use cc_takeoff::geom::{self as G, P};
use cc_takeoff::i18n::{F, t, tf};
use cc_takeoff::snap::{Hit, Kinds, MAX_SEGS, SnapIndex};
use egui::{Pos2, Stroke};

use crate::pdfview::PdfView;
use crate::theme::Tokens;

/// takeoffcraft.js `SNAP`: how near (screen px) a line of the drawing a point snaps to it.
const SNAP_PX: f64 = 10.0;
/// Sheets whose linework is kept (`snaps.size > 4` drops the oldest).
const KEEP: usize = 4;
/// The tools whose picks snap (SNAP_TOOLS; any drag of a corner snaps too).
const SNAP_TOOLS: [&str; 7] = ["linear", "area", "cutout", "circle", "cutcircle", "calibrate", "check"];
/// The tools a typed entry places points for (TYPE_TOOLS).
const TYPE_TOOLS: [&str; 5] = ["linear", "area", "cutout", "circle", "cutcircle"];

/// Snapping and typing: the settings, the hit under the pointer (for the marker), the typed box, the sheets' linework.
#[derive(Debug, Default)]
pub struct SnapState {
    /// Snap off (0.5's default is on)
    pub off: bool,
    /// Settings › Snap to 45° without holding Shift
    pub s45: bool,
    /// the snap the pointer is on now
    pub hit: Option<Hit>,
    /// the typed entry box while it is open, and its text
    pub typein: Option<String>,
    /// each sheet's snap index (None: no linework), oldest first
    idx: HashMap<String, Option<Arc<SnapIndex>>>,
    order: Vec<String>,
}

impl PdfView {
    /// loadSnap: the open sheet's linework, read once (a sheet cut at the segment cap says so).
    fn tk_load_snap(&mut self) -> Option<Arc<SnapIndex>> {
        let key = self.takeoff.cur_sheet.clone();
        if let Some(x) = self.takeoff.snap.idx.get(&key) {
            return x.clone();
        }
        let lw = cc_io_takeoff::pdfread::page_linework(&self.cur.doc, self.page, MAX_SEGS).ok();
        let truncated = lw.as_ref().is_some_and(|l| l.truncated);
        let ix = lw.filter(|l| l.n > 0).map(|l| Arc::new(SnapIndex::new(&l)));
        let s = &mut self.takeoff.snap;
        s.idx.insert(key.clone(), ix.clone());
        s.order.push(key);
        while s.order.len() > KEEP {
            let k = s.order.remove(0);
            s.idx.remove(&k);
        }
        if truncated && !s.off {
            self.takeoff.message = tf("snap.truncated", &[("n", F::S(group3(MAX_SEGS)))]);
        }
        ix
    }

    /// snapHit: the snap within reach of `raw` (Snap on, a measuring tool or a corner being dragged).
    fn tk_snap_hit(&mut self, raw: [f64; 2]) -> Option<Hit> {
        let kind = self.takeoff.tool_kind().unwrap_or_default();
        if self.takeoff.snap.off || !(SNAP_TOOLS.contains(&kind.as_str()) || self.takeoff.drag.is_some()) {
            return None;
        }
        let ix = self.tk_load_snap()?;
        ix.snap(raw[0], raw[1], SNAP_PX / self.zoom, Kinds::default())
    }

    /// pointFor(raw): where a pick lands — on the drawing's linework when it snaps, else 45° from the last point with
    /// Shift (or without it when Snap to 45° is on).
    pub fn tk_point_for(&mut self, raw: [f64; 2], shift: bool) -> [f64; 2] {
        self.takeoff.snap.hit = None;
        if self.takeoff.paste.is_some() {
            return raw;
        }
        if let Some(h) = self.tk_snap_hit(raw) {
            self.takeoff.snap.hit = Some(h);
            return [h.x, h.y];
        }
        let kind = self.takeoff.tool_kind().unwrap_or_default();
        if kind == "circle" || kind == "cutcircle" || self.takeoff.drag.as_ref().is_some_and(|d| d.arc) {
            return raw;
        }
        // arc mode: the point on the curve goes anywhere; the arc's end snaps 45° from where the arc starts
        if self.takeoff.arc && !self.takeoff.pts.is_empty() && self.takeoff.mid.is_none() {
            return raw;
        }
        let last = self.takeoff.pts.last().copied().or_else(|| self.takeoff.drag.as_ref().and_then(|d| d.anchor));
        let snap = if self.takeoff.snap.s45 { !shift } else { shift };
        match last {
            Some(l) if snap => {
                let q = G::snap45(&l, &P::new(raw[0], raw[1]));
                [q.x, q.y]
            }
            _ => raw,
        }
    }

    /// toggleSnap: Snap on or off (F3, the toggle); a sheet with no lines says so.
    pub fn tk_toggle_snap(&mut self, on: bool) {
        self.takeoff.snap.off = !on;
        self.takeoff.message = t(if on { "snap.on" } else { "snap.off" });
        if on && self.tk_load_snap().is_none() {
            self.takeoff.message = t("snap.none");
        }
    }

    /// canType: a run, outline, cut-out or circle is being drawn.
    pub fn tk_can_type(&self) -> bool {
        let kind = self.takeoff.tool_kind().unwrap_or_default();
        TYPE_TOOLS.contains(&kind.as_str()) && !self.takeoff.pts.is_empty() && self.takeoff.paste.is_none()
    }

    /// openTypeIn: the box opens (with what was typed into it first); its help is the status line.
    pub fn tk_open_typein(&mut self, first: &str) {
        let kind = self.takeoff.tool_kind().unwrap_or_default();
        self.takeoff.snap.typein = Some(first.to_string());
        self.takeoff.message = t(if kind == "circle" || kind == "cutcircle" { "typein.hint.circle" } else { "typein.hint" });
    }

    /// Enter in the box: an empty box closes and finishes the draft; else applyTyped — the point read from the text is
    /// placed as a click would (an error leaves the box open with 0.5's line).
    pub fn tk_enter_typein(&mut self) {
        let Some(text) = self.takeoff.snap.typein.clone() else { return };
        let sheet = self.takeoff.cur_sheet.clone();
        if cc_m3d::js::trim(&text).is_empty() {
            self.takeoff.snap.typein = None;
            self.takeoff.finish(&sheet);
            return;
        }
        let Some(first) = self.takeoff.pts.first().copied() else {
            self.takeoff.snap.typein = None;
            return;
        };
        let last = self.takeoff.pts.last().copied().unwrap_or(first);
        let ftu = self.takeoff.ft_per_unit(&sheet);
        let kind = self.takeoff.tool_kind().unwrap_or_default();
        let cursor = self.takeoff.cursor.map(|c| P::new(c[0], c[1]));
        let res: Result<P, &'static str> = if kind == "circle" || kind == "cutcircle" {
            let c = first;
            let to = cursor.filter(|q| G::dist(q, &c) > 0.0).unwrap_or(P::new(c.x + 1.0, c.y));
            let n = G::dist(&to, &c);
            match (cc_takeoff::units::parse_length(&text, "ft"), ftu) {
                (Err(_), _) => Err("typein.err.read"),
                (Ok(_), None) => Err("typein.err.noscale"),
                (Ok(r), Some(k)) => Ok(P::new(c.x + (to.x - c.x) / n * r / k, c.y + (to.y - c.y) / n * r / k)),
            }
        } else {
            let tr = self.cur.pages.get(self.page).map_or([1.0, 0.0, 0.0, -1.0, 0.0, 0.0], |p| p.transform);
            let z = self.zoom;
            let ctx = cc_takeoff::typein::Ctx {
                first: Some(first),
                last: Some(last),
                cursor,
                ft_per_unit: ftu.unwrap_or(f64::NAN),
                linear: [tr[0] * z, tr[1] * z, tr[2] * z, tr[3] * z],
            };
            cc_takeoff::typein::parse_entry(&text, &ctx).map(|(p, _)| p)
        };
        match res {
            Err(e) => self.takeoff.message = t(e),
            Ok(p) => {
                self.takeoff.snap.typein = Some(String::new());
                // a typed point is never the second press of a double-click
                self.takeoff.last_click = None;
                self.takeoff.click(&sheet, [p.x, p.y]);
                if self.takeoff.pts.is_empty() {
                    self.takeoff.snap.typein = None;
                }
            }
        }
    }

    /// `takeoff.type=TEXT`: the box opened, the text typed, Enter.
    pub fn tk_type(&mut self, text: &str) -> Result<(), String> {
        if !self.tk_can_type() {
            return Err("takeoff.type: no run, outline, cut-out or circle is being drawn".into());
        }
        self.tk_open_typein("");
        self.takeoff.snap.typein = Some(text.to_string());
        self.tk_enter_typein();
        Ok(())
    }

    /// drawSnap: the marker of the snap under the pointer, with its name.
    pub(crate) fn tk_draw_snap(&self, painter: &egui::Painter, at: &dyn Fn(f64, f64) -> Pos2, tok: &Tokens) {
        let (Some(h), Some(_)) = (self.takeoff.snap.hit, self.takeoff.cursor) else { return };
        let s = at(h.x, h.y);
        let r = 6.0;
        let st = Stroke::new(2.0, tok.accent);
        let p = |dx: f32, dy: f32| s + egui::vec2(dx, dy);
        match h.kind {
            "end" => {
                painter.rect_stroke(egui::Rect::from_center_size(s, egui::vec2(2.0 * r, 2.0 * r)), 0.0, st, egui::StrokeKind::Middle);
            }
            "mid" => {
                painter.add(egui::Shape::closed_line(vec![p(0.0, -r), p(r, r * 0.8), p(-r, r * 0.8)], st));
            }
            "int" => {
                painter.line_segment([p(-r, -r), p(r, r)], st);
                painter.line_segment([p(r, -r), p(-r, r)], st);
            }
            _ => {
                painter.add(egui::Shape::closed_line(vec![p(-r, -r), p(r, -r), p(-r, r), p(r, r)], st));
            }
        }
        let txt = t(&format!("snap.{}", h.kind));
        let font = egui::FontId::proportional(11.0);
        let g = painter.layout_no_wrap(txt, font, tok.accent);
        let rect = egui::Rect::from_min_size(p(r + 4.0, r + 2.0), egui::vec2(g.size().x + 8.0, 16.0));
        painter.rect_filled(rect, 0.0, tok.panel);
        painter.rect_stroke(rect, 0.0, Stroke::new(1.0, tok.accent), egui::StrokeKind::Inside);
        painter.galley(rect.min + egui::vec2(4.0, 2.0), g, tok.accent);
    }

    /// The status row's Snap toggle, the 45° hint and setting, and the typed entry box.
    pub fn tk_snap_row(&mut self, ui: &mut egui::Ui, tok: &Tokens) {
        ui.horizontal_wrapped(|ui| {
            if let Some(mut text) = self.takeoff.snap.typein.clone() {
                let l = ui.label(t("typein.label"));
                let r = ui.add(crate::shell::field(&mut text).hint_text(t("typein.placeholder")).desired_width(150.0)).labelled_by(l.id);
                r.request_focus();
                let (enter, esc) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
                self.takeoff.snap.typein = Some(text);
                if esc {
                    self.takeoff.snap.typein = None;
                    self.takeoff.message.clear();
                } else if enter {
                    self.tk_enter_typein();
                }
            }
            let on = !self.takeoff.snap.off;
            if crate::shell::bar_toggle(ui, tok, &t("snap.toggle"), on).on_hover_text(t("snap.toggle.title")).clicked() {
                self.tk_toggle_snap(!on);
            }
            let mut s45 = self.takeoff.snap.s45;
            if ui.checkbox(&mut s45, t("set.snap45")).on_hover_text(t("set.snap45.hint")).changed() {
                self.takeoff.snap.s45 = s45;
            }
            ui.label(egui::RichText::new(t(if s45 { "snap45.on" } else { "snap45.hint" })).small().color(tok.text_dim));
        });
    }
}

/// `n.toLocaleString()` in English: 400000 → "400,000".
fn group3(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The keys of the sheet view in TakeoffCraft: F3 (Snap), and a digit, @ or # during a run (the typed entry box).
pub(crate) fn keys(p: &mut PdfView, ui: &egui::Ui) {
    if p.takeoff.snap.typein.is_some() {
        return;
    }
    let (f3, typed) = ui.input(|i| {
        let typed = i.events.iter().find_map(|e| match e {
            egui::Event::Text(s) if s.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '@' || c == '#') => Some(s.clone()),
            _ => None,
        });
        (i.key_pressed(egui::Key::F3), typed)
    });
    if f3 {
        let on = p.takeoff.snap.off;
        p.tk_toggle_snap(on);
    }
    if let Some(s) = typed
        && p.tk_can_type()
    {
        p.tk_open_typein(&s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_are_grouped_as_english_writes_them() {
        assert_eq!(group3(400_000), "400,000");
        assert_eq!(group3(12), "12");
        assert_eq!(group3(1234), "1,234");
    }
}
