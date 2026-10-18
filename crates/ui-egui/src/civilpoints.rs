//! CivilCraft's survey point editing (M11.10): selecting several points (Shift+click, Shift+drag a box, Ctrl+A), Copy /
//! Cut / Paste / Paste in place / Duplicate and Delete, as civilcraft.js does them with clip.js (the engine's `clip`):
//! a copy is kept in the app and put on the system clipboard as point rows; a paste carries the points under the pointer
//! until a click places them (Enter = at their own coordinates, Esc cancels), renumbers clashing numbers and is one
//! undo step; text copied elsewhere (a spreadsheet) pastes too when it holds point rows.

use std::collections::BTreeSet;

use cc_civil::clip::{Clip, centre, choose_paste, make_clip, move_by, read_clip, renumber, renumber_text};
use cc_civil::i18n::t;
use cc_civil::points::Pt;
use cc_civil::tr;
use cc_civil::units::grouped;
use serde_json::Value;

use crate::civiltools::Tool;
use crate::civilview::{CivilView, Sel};

/// The point order CivilCraft copies and reads (settings.order's default).
pub const ORDER: &str = "PNEZD";

/// Points being carried by the pointer (civilcraft.js `pasteJob`).
#[derive(Clone, Debug)]
pub struct PasteJob {
    pub pts: Vec<Pt>,
    pub anchor: (f64, f64),
    /// "Paste", "Paste in place" or "Duplicate" (civilcraft.js's internal labels)
    pub label: &'static str,
    pub from: String,
    pub note: String,
}

/// The survey point selection, the copy and a paste on the way.
#[derive(Clone, Debug, Default)]
pub struct PointsState {
    /// selected survey points by number (civilcraft.js `picked`; one picked point also shows as `Sel::Point`)
    pub picked: BTreeSet<String>,
    pub clip: Option<Clip>,
    pub paste: Option<PasteJob>,
    /// text for the system clipboard, handed to egui on the next frame
    pub copy_out: Option<String>,
    /// a Shift+drag box being drawn (world start)
    pub boxing: Option<(f64, f64)>,
}

fn count(n: usize) -> String {
    grouped(n as f64, 0)
}

impl CivilView {
    /// The selected survey points' numbers (the picked set, or the one point selected).
    pub fn picked_numbers(&self) -> BTreeSet<String> {
        let mut s = self.points.picked.clone();
        if let Some(Sel::Point(n)) = &self.sel {
            s.insert(n.clone());
        }
        s
    }

    /// civilcraft.js `pickedPoints()`: in the project's order.
    pub fn picked_points(&self) -> Vec<Pt> {
        let s = self.picked_numbers();
        self.project.points.iter().filter(|p| s.contains(&p.n)).cloned().collect()
    }

    /// One picked point is the selected point (its Properties and grip); more are the picked set.
    fn set_picked(&mut self, s: BTreeSet<String>) {
        self.profile.sel_pvi = None;
        if s.len() == 1 {
            self.sel = s.into_iter().next().map(Sel::Point);
            self.points.picked.clear();
        } else {
            self.points.picked = s;
            self.sel = None;
        }
    }

    /// Shift+click (civilcraft.js `click`, Select with Shift): the nearest survey point within 8 px joins or leaves.
    pub fn pick_toggle(&mut self, x: f64, y: f64) {
        let s = self.css(x, y);
        let Some(p) = self.nearest_px(s, 8.0, false).and_then(|i| self.project.points.get(i)).map(|p| p.n.clone()) else { return };
        let mut set = self.picked_numbers();
        if !set.remove(&p) {
            set.insert(p);
        }
        let n = set.len();
        self.set_picked(set);
        self.message = tr!("cv.pts.selected", n = n, count = count(n));
    }

    /// civilcraft.js `boxSelect(a, b)` in world units: the survey points inside join the selection.
    pub fn box_select(&mut self, a: (f64, f64), b: (f64, f64)) {
        let (x0, x1, y0, y1) = (a.0.min(b.0), a.0.max(b.0), a.1.min(b.1), a.1.max(b.1));
        let mut set = self.picked_numbers();
        let mut added = 0usize;
        for p in &self.project.points {
            if p.x >= x0 && p.x <= x1 && p.y >= y0 && p.y <= y1 && set.insert(p.n.clone()) {
                added += 1;
            }
        }
        let n = set.len();
        self.set_picked(set);
        self.message = if added > 0 {
            tr!("cv.pts.box", added = tr!("cv.pts.n", n = added, count = count(added)), total = tr!("cv.pts.n", n = n, count = count(n)))
        } else {
            t("cv.pts.box.none", &[])
        };
    }

    /// Ctrl+A (civilcraft.js `selectAll`).
    pub fn select_all(&mut self) {
        if self.project.points.is_empty() {
            self.message = t("cv.pts.none", &[]);
            return;
        }
        if self.tools.tool != Tool::Select || self.points.paste.is_some() {
            self.set_tool(Tool::Select);
        }
        let set: BTreeSet<String> = self.project.points.iter().map(|p| p.n.clone()).collect();
        let n = set.len();
        self.set_picked(set);
        self.message = tr!("cv.pts.selected", n = n, count = count(n));
    }

    /// civilcraft.js `selectNone`.
    pub fn select_none(&mut self) {
        if self.points.paste.is_some() {
            self.set_tool(Tool::Select);
        }
        self.set_picked(BTreeSet::new());
    }

    /// civilcraft.js `copyPoints(cut)`: the copy (and its rows for the system clipboard); a cut removes them too.
    pub fn copy_points(&mut self, cut: bool, at_ms: f64) {
        let pts = self.picked_points();
        if pts.is_empty() {
            self.message = t(
                match (self.project.points.is_empty(), cut) {
                    (false, true) => "cv.cut.need",
                    (false, false) => "cv.copy.need",
                    (true, true) => "cv.cut.none",
                    (true, false) => "cv.copy.none",
                },
                &[],
            );
            return;
        }
        let c = make_clip(&pts, self.project.units(), ORDER, "", at_ms);
        self.points.clip = read_clip(&c);
        self.points.copy_out = c.get("csv").and_then(Value::as_str).map(str::to_string);
        let n = pts.len();
        if cut {
            self.remove_points(&tr!("cv.undo.pts.cut", n = n, count = count(n)));
        }
        let order = ORDER.chars().map(String::from).collect::<Vec<_>>().join(",");
        let mut msg = vec![tr!(if cut { "cv.pts.cut.done" } else { "cv.pts.copied" }, n = n, count = count(n), order = order)];
        if cut {
            msg.push(self.rebuilt());
        }
        msg.push(t("cv.pts.paste.how", &[]));
        self.message = msg.join(" ");
    }

    /// civilcraft.js `rebuilt()`: what happened to the surface (a sentence).
    pub fn rebuilt(&self) -> String {
        let tin = self.project.state.get("surface").and_then(|s| s.get("tin")).is_some_and(|t| cc_civil::js::truthy(Some(t)));
        t(if tin { "cv.surface.tin.kept" } else { "cv.surface.rebuilt" }, &[])
    }

    /// civilcraft.js `removePoints(label)`: the selected points go (one undo step).
    pub fn remove_points(&mut self, label: &str) {
        let gone = self.picked_numbers();
        self.project.edit_points(label, &mut |pts, _| Some(pts.into_iter().filter(|p| !gone.contains(&p.n)).collect()));
        self.set_picked(BTreeSet::new());
    }

    /// civilcraft.js `paste(inPlace, systemText)`.
    pub fn paste(&mut self, in_place: bool, system_text: Option<&str>) {
        let pick = choose_paste(self.points.clip.as_ref(), system_text, ORDER);
        let kind = pick.get("kind").and_then(Value::as_str).unwrap_or("none");
        if kind == "none" {
            self.message = pick.get("why").and_then(Value::as_str).unwrap_or("").to_string();
            return;
        }
        let pts: Vec<Pt> = cc_civil::js::arr(pick.get("points")).iter().map(cc_civil::points::read_pt).collect();
        let from = t(if kind == "text" { "cv.paste.from.text" } else { "cv.paste.from.clip" }, &[]);
        let units = self.project.units().to_string();
        let pu = pick.get("units").and_then(Value::as_str).unwrap_or(&units).to_string();
        let note = if kind == "clip" && pu != units {
            format!(" {}", t(if pu == "m" { "cv.paste.units.m" } else { "cv.paste.units.ft" }, &[]))
        } else {
            String::new()
        };
        if in_place {
            self.commit_paste(&pts, 0.0, 0.0, "Paste in place", &note);
        } else {
            self.start_placing(pts, "Paste", from, note);
        }
    }

    /// civilcraft.js `duplicatePoints()`.
    pub fn duplicate_points(&mut self) {
        let pts = self.picked_points();
        if pts.is_empty() {
            self.message = t(if self.project.points.is_empty() { "cv.dup.none" } else { "cv.dup.need" }, &[]);
            return;
        }
        self.start_placing(pts, "Duplicate", t("cv.paste.from.dup", &[]), String::new());
    }

    /// civilcraft.js `startPlacing`: the points follow the pointer.
    fn start_placing(&mut self, pts: Vec<Pt>, label: &'static str, from: String, note: String) {
        self.set_tool(Tool::Select);
        let c = centre(&pts);
        let n = pts.len();
        self.message = format!("{}{note}", tr!("cv.paste.follow", n = n, count = count(n), from = from.as_str()));
        self.points.paste = Some(PasteJob { pts, anchor: (c.x, c.y), label, from, note });
    }

    /// A click while carrying points: they go there (civilcraft.js `click` → `commitPaste`).
    pub fn paste_at(&mut self, x: f64, y: f64) {
        let Some(j) = self.points.paste.take() else { return };
        self.commit_paste(&j.pts, x - j.anchor.0, y - j.anchor.1, j.label, &j.note);
    }

    /// Enter while carrying points: at their own coordinates.
    pub fn paste_in_place_now(&mut self) {
        let Some(j) = self.points.paste.take() else { return };
        let label = if j.label == "Paste" { "Paste in place" } else { j.label };
        self.commit_paste(&j.pts, 0.0, 0.0, label, &j.note);
    }

    /// civilcraft.js `commitPaste(pts, dx, dy, label, from, note)`: renumbered, one undo step, the new points selected.
    fn commit_paste(&mut self, pts: &[Pt], dx: f64, dy: f64, label: &str, note: &str) {
        let moved = if dx != 0.0 || dy != 0.0 { move_by(pts, dx, dy) } else { pts.to_vec() };
        let existing: Vec<String> = self.project.points.iter().map(|p| p.n.clone()).collect();
        let (out, changed, kept) = renumber(&moved, &existing, "clash", None);
        let (undo, done) = match label {
            "Duplicate" => ("cv.undo.duplicate", "cv.dup.done"),
            "Paste in place" => ("cv.undo.paste.place", "cv.paste.done.place"),
            _ => ("cv.undo.paste", "cv.paste.done"),
        };
        let n = pts.len();
        let add = out.clone();
        self.project.edit_points(&tr!(undo, n = n, count = count(n)), &mut |mut all, _| {
            all.extend(add.iter().cloned());
            Some(all)
        });
        self.points.paste = None;
        self.set_tool(Tool::Select);
        self.set_picked(out.iter().map(|p| p.n.clone()).collect());
        self.message = format!("{} {}{note}", tr!(done, n = n, count = count(n), renumber = renumber_text(&changed, kept)), self.rebuilt());
    }

    /// Esc while carrying points.
    pub fn paste_cancel(&mut self) -> bool {
        if self.points.paste.take().is_some() {
            self.set_tool(Tool::Select);
            self.message = t("cv.paste.cancelled", &[]);
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_paste_renumbers_clashes_and_is_one_step() {
        let mut cv = CivilView::sample();
        cv.view.width = 1000.0;
        cv.view.height = 800.0;
        cv.view.cx = 6_150_200.0;
        cv.view.cy = 2_080_150.0;
        cv.view.zoom = 1.5;
        let a: Vec<(f64, f64)> = cv.project.points.iter().take(2).map(|p| (p.x, p.y)).collect();
        cv.pick_toggle(a[0].0, a[0].1);
        cv.pick_toggle(a[1].0, a[1].1);
        assert_eq!(cv.picked_numbers().len(), 2);
        cv.copy_points(false, 1.0);
        assert!(cv.points.copy_out.as_deref().is_some_and(|s| s.contains(',')));
        cv.paste(false, None);
        assert!(cv.points.paste.is_some());
        cv.paste_at(6_150_300.0, 2_080_200.0);
        assert_eq!(cv.project.points.len(), 278);
        assert_eq!(cv.project.undo_stack.len(), 1);
        assert_eq!(cv.picked_numbers().len(), 2);
        cv.duplicate_points();
        assert!(cv.paste_cancel());
        cv.select_all();
        cv.copy_points(true, 2.0);
        assert!(cv.project.points.is_empty());
        cv.paste(true, None);
        assert_eq!(cv.project.points.len(), 278);
        cv.select_none();
        cv.paste(false, Some("P1,100,200,10,X\nP2,110,210,11,Y\n"));
        let pts = cv.points.paste.as_ref().map(|j| j.pts.len());
        assert_eq!(pts, Some(2));
        cv.paste_in_place_now();
        assert!(cv.project.points.iter().any(|p| p.n == "P1"));
    }

    #[test]
    fn a_box_selects_what_is_inside() {
        let mut cv = CivilView::sample();
        cv.box_select((6_150_000.0, 2_080_000.0), (6_150_100.0, 2_080_100.0));
        let n = cv.picked_numbers().len();
        assert!(n > 1);
        cv.box_select((0.0, 0.0), (1.0, 1.0));
        assert_eq!(cv.picked_numbers().len(), n);
        assert_eq!(cv.message, t("cv.pts.box.none", &[]));
    }
}
