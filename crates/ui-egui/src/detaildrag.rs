//! DetailCraft: dragging selected parts to move them (0.5 D2; view3d.js `dragParts` and detailcraft.js's hooks). With
//! the Select tool, a press on a part that is already selected and a drag moves the whole selection (connections ride
//! along): across the level in 3D and Plan (Ctrl/Cmd held in 3D: up and down), within the elevation's plane in
//! Elevation. The held part's points snap onto grid intersections within 18 points (transform.js `snapOffset`), else
//! the offset goes in 10 mm steps; a dashed ghost follows and the status line says the offset. Letting go is one
//! "Move" step (`runXf`); Esc puts it back. A press without a drag is a click; a press on an unselected part still turns
//! or pans the view. Touch: a long press (0.55 s, the finger within 12 points) adds the part to the selection or takes
//! it out, as Shift+click does (view3d.js `onLongPress`).
//!
//! Headless: `detail.drag` = `<kind:id>:dx,dy,dz` drags the held part by that offset in the drag's plane (mm, before
//! snapping, as the pointer's ray would give it), `ctrl:<kind:id>:…` the vertical drag in 3D, a trailing `:esc` lets
//! Esc put it back; `detail.longpress` = `<kind:id>`.

use cc_detail::i18n::{F, t, t0};
use cc_detail::js::{num, num_to_string, round};
use serde_json::Value;

use crate::detail3d::Ref;
use crate::detailtools::How;
use crate::detailview::{DetailView, ViewMode};

/// view3d.js `TAP_PX` / `TAP_TOUCH` / `SNAP_PX` / `LONG_MS`.
pub const TAP_PX: f32 = 5.0;
pub const TAP_TOUCH: f32 = 12.0;
pub const SNAP_PX: f64 = 18.0;
pub const LONG_MS: f64 = 550.0;

/// A drag of selected parts under way (view3d.js `drag` + `partDrag`).
#[derive(Clone, Debug)]
pub struct PartDrag {
    /// what moves (the selection without connections)
    pub refs: Vec<Ref>,
    /// the part pressed on
    pub held: Ref,
    /// where the press met the part (mm) and the plane the pointer moves in
    pub grab: [f64; 3],
    pub n: [f64; 3],
    pub mask: [f64; 3],
    /// the held part's points that snap
    pub points: Vec<[f64; 3]>,
    /// the selection's outline (mm, absolute)
    pub ghost: Vec<[[f64; 3]; 2]>,
    /// the press, in view points
    pub start: egui::Pos2,
    pub moved: bool,
    pub offset: [f64; 3],
    pub node: Option<[f64; 3]>,
    pub touch: bool,
}

/// A press that may become a long press (view3d.js `long`).
#[derive(Clone, Debug)]
pub struct LongPress {
    pub at: egui::Pos2,
    pub since: f64,
    pub r: Option<Ref>,
}

/// detailcraft.js `mm(v)`: `String(Math.round(v))`.
fn mm(v: f64) -> String {
    let r = round(v);
    num_to_string(if r == 0.0 { 0.0 } else { r })
}

impl DetailView {
    /// detailcraft.js `dragParts.can(ref)`.
    pub fn drag_can(&self, r: &Ref) -> bool {
        self.tools.tool == "select" && !matches!(r, Ref::Conn(_)) && self.selected().contains(r)
    }

    /// The held part's points that snap (detailcraft.js `dragParts.start`): a member's ends, a footing's top centre,
    /// a stair's start.
    fn drag_points(&self, r: &Ref) -> Vec<[f64; 3]> {
        let find = |key: &str, id: &str| {
            cc_detail::js::arr(self.model.state.get(key)).iter().find(|x| x.get("id").and_then(Value::as_str) == Some(id)).cloned()
        };
        let p3 = |v: Option<&Value>| [0, 1, 2].map(|i| num(v.and_then(|a| a.get(i))));
        match r {
            Ref::Member(i) => find("members", i).map(|m| vec![p3(m.get("start")), p3(m.get("end"))]).unwrap_or_default(),
            Ref::Footing(i) => find("footings", i).map(|f| vec![[num(f.get("x")), num(f.get("y")), num(f.get("top"))]]).unwrap_or_default(),
            Ref::Stair(i) => find("stairs", i).map(|s| vec![[num(s.get("x")), num(s.get("y")), num(s.get("z"))]]).unwrap_or_default(),
            Ref::Conn(_) => Vec::new(),
        }
    }

    /// A press on `held` at model point `grab` (mm): starts a drag when the part may be dragged (view3d.js pointerdown).
    /// `vertical`: Ctrl/Cmd held in 3D; `look`: the camera's direction (a vertical drag's plane faces it).
    pub fn drag_start(&mut self, held: Ref, grab: [f64; 3], start: egui::Pos2, vertical: bool, touch: bool) -> bool {
        if !self.drag_can(&held) {
            return false;
        }
        let refs: Vec<Ref> = self.selected().into_iter().filter(|r| !matches!(r, Ref::Conn(_))).collect();
        if refs.is_empty() {
            return false;
        }
        let vertical = vertical && self.mode == ViewMode::D3;
        let mask = match self.mode {
            ViewMode::Elevation => [1.0, 0.0, 1.0],
            _ if vertical => [0.0, 0.0, 1.0],
            _ => [1.0, 1.0, 0.0],
        };
        let mut n = if self.mode == ViewMode::Elevation { [0.0, 1.0, 0.0] } else { [0.0, 0.0, 1.0] };
        if vertical {
            let c = &self.view.cam;
            let d = [c.target[0] - c.eye[0], c.target[1] - c.eye[1]];
            n = if d[0].hypot(d[1]) < 1e-6 { [0.0, 1.0, 0.0] } else { [d[0], d[1], 0.0] };
        }
        let rj: Vec<Value> = refs.iter().map(crate::detailtools::ref_json).collect();
        let ghost = cc_detail::transform::ghost_lines(&self.model.state, &rj, [0.0, 0.0, 0.0]).unwrap_or_default();
        let points = self.drag_points(&held);
        self.drag = Some(PartDrag { refs, held, grab, n, mask, points, ghost, start, moved: false, offset: [0.0; 3], node: None, touch });
        true
    }

    /// The pointer moved to `at` (view points): past the tap distance the drag shows its offset (view3d.js
    /// pointermove → `dragParts.move`).
    pub fn drag_to(&mut self, at: egui::Pos2) {
        let ppp = self.ppp;
        let Some(d) = self.drag.as_mut() else { return };
        if !d.moved && (at - d.start).length() > if d.touch { TAP_TOUCH } else { TAP_PX } {
            d.moved = true;
        }
        if !d.moved {
            return;
        }
        let (o, dir) = self.view.cam.ray(f64::from(at.x) * ppp, f64::from(at.y) * ppp);
        let (n, g) = (d.n, [d.grab[0] * 0.001, d.grab[1] * 0.001, d.grab[2] * 0.001]);
        let den = n[0] * dir[0] + n[1] * dir[1] + n[2] * dir[2];
        if den.abs() < 1e-12 {
            return;
        }
        let s = (n[0] * (g[0] - o[0]) + n[1] * (g[1] - o[1]) + n[2] * (g[2] - o[2])) / den;
        if !s.is_finite() {
            return;
        }
        let q = [(o[0] + dir[0] * s) / 0.001, (o[1] + dir[1] * s) / 0.001, (o[2] + dir[2] * s) / 0.001];
        let keep = |m: f64, v: f64| if m != 0.0 { v } else { 0.0 };
        let raw = [keep(d.mask[0], q[0] - d.grab[0]), keep(d.mask[1], q[1] - d.grab[1]), keep(d.mask[2], q[2] - d.grab[2])];
        self.drag_raw(raw);
    }

    /// The drag's offset from the pointer's (unsnapped) offset in its plane: snapped and said.
    pub fn drag_raw(&mut self, raw: [f64; 3]) {
        let ppp = self.ppp;
        let nodes: Vec<[f64; 3]> = self.active_nodes().into_iter().map(|n| n.p).collect();
        let cam = self.view.cam;
        let proj = move |p: [f64; 3]| match crate::detailview::project(&cam, [p[0] * 0.001, p[1] * 0.001, p[2] * 0.001]) {
            Some((x, y)) => cc_detail::transform::Screen { x: x / ppp, y: y / ppp, behind: false },
            None => cc_detail::transform::Screen { x: f64::NAN, y: f64::NAN, behind: true },
        };
        let Some(d) = self.drag.as_mut() else { return };
        let (off, node, _) = cc_detail::transform::snap_offset(raw, &d.points, &nodes, &proj, d.mask, SNAP_PX, 10.0);
        d.moved = true;
        d.offset = off;
        d.node = node;
        let fills = [
            ("x", F::S(mm(off[0]))),
            ("y", F::S(mm(off[1]))),
            ("z", F::S(mm(off[2]))),
            ("node", F::S(node.map(|p| cc_detail::model::node_name(&self.model.state, p)).unwrap_or_default())),
        ];
        self.message = t(if node.is_some() { "drag.snap" } else { "drag.free" }, &fills);
    }

    /// Letting go (view3d.js pointerup): a moved drag with an offset is one Move step, a moved one back where it started
    /// is cancelled, a press without a move is a click on the part.
    pub fn drag_end(&mut self, add: bool) {
        let Some(d) = self.drag.take() else { return };
        if !d.moved {
            self.pick(Some(d.held), add);
            return;
        }
        if d.offset.iter().any(|v| *v != 0.0) {
            if let Ok(xf) = cc_detail::transform::move_xf([
                Some(&Value::from(d.offset[0])),
                Some(&Value::from(d.offset[1])),
                Some(&Value::from(d.offset[2])),
            ]) {
                self.run_xf(&d.refs, &xf, false, How::Move(d.offset));
            }
        } else {
            self.message = t0("drag.cancel");
        }
    }

    /// Esc while dragging (view3d.js `cancelDrag`): nothing moved yet.
    pub fn drag_cancel(&mut self) -> bool {
        if self.drag.take().is_none() {
            return false;
        }
        self.message = t0("drag.cancel");
        true
    }

    /// detailcraft.js `view.onLongPress(ref)`: with the Select tool, Shift+click.
    pub fn long_press(&mut self, r: Option<Ref>) {
        self.drag = None;
        if self.tools.tool != "select" {
            return;
        }
        if let Some(r) = r {
            self.pick(Some(r), true);
        }
    }

    /// `detail.drag`: `[ctrl:]<kind:id>:dx,dy,dz[:esc]`.
    pub fn drag_cmd(&mut self, spec: &str) -> Result<(), String> {
        let (spec, vertical) = match spec.strip_prefix("ctrl:") {
            Some(s) => (s, true),
            None => (spec, false),
        };
        let (spec, esc) = match spec.strip_suffix(":esc") {
            Some(s) => (s, true),
            None => (spec, false),
        };
        let bad = || format!("detail.drag: [ctrl:]kind:id:dx,dy,dz[:esc], not {spec}");
        let (r, off) = spec.rsplit_once(':').ok_or_else(bad)?;
        let r = Ref::parse(r).ok_or_else(bad)?;
        let v: Vec<f64> = off.split(',').map(|x| cc_detail::js::str_to_number(x.trim())).collect();
        let raw = [0, 1, 2].map(|i| v.get(i).copied().unwrap_or(0.0));
        if v.len() != 3 || raw.iter().any(|x| !x.is_finite() || x.abs() > 1e7) {
            return Err(bad());
        }
        if !self.exists(&r) {
            return Err(format!("detail.drag: no {}", r.key()));
        }
        let grab = self.drag_points(&r).first().copied().unwrap_or([0.0; 3]);
        if !self.drag_start(r, grab, egui::Pos2::ZERO, vertical, false) {
            return Ok(());
        }
        let mask = self.drag.as_ref().map_or([0.0; 3], |d| d.mask);
        let keep = |m: f64, v: f64| if m != 0.0 { v } else { 0.0 };
        self.drag_raw([keep(mask[0], raw[0]), keep(mask[1], raw[1]), keep(mask[2], raw[2])]);
        if esc {
            self.drag_cancel();
        } else {
            self.drag_end(false);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_moves_the_selection_in_one_step_and_esc_puts_it_back() {
        let mut dv = DetailView::sample().expect("sample");
        let id = cc_detail::js::arr(dv.model.state.get("members"))
            .iter()
            .find(|m| m.get("type").and_then(Value::as_str) == Some("beam"))
            .and_then(|m| m.get("id").and_then(Value::as_str))
            .expect("a beam")
            .to_string();
        let before = dv.member(&id).cloned();
        // not selected: no drag
        dv.drag_cmd(&format!("member:{id}:1234,0,0")).expect("cmd");
        assert_eq!(dv.member(&id).cloned(), before);
        dv.select(Some(Ref::Member(id.clone())));
        dv.drag_cmd(&format!("member:{id}:1234,0,0:esc")).expect("cmd");
        assert_eq!(dv.member(&id).cloned(), before);
        assert_eq!(dv.message, t0("drag.cancel"));
        dv.drag_cmd(&format!("member:{id}:1234,0,0")).expect("cmd");
        let after = dv.member(&id).cloned().expect("still there");
        let x0 = num(before.as_ref().and_then(|m| m.get("start")).and_then(|s| s.get(0)));
        assert_eq!(num(after.get("start").and_then(|s| s.get(0))), x0 + 1230.0);
        dv.undo();
        let back = dv.member(&id).cloned().expect("still there");
        assert_eq!(num(back.get("start").and_then(|s| s.get(0))), x0);
        assert!(dv.drag_cmd("member:x:nan,0,0").is_err());
    }
}
