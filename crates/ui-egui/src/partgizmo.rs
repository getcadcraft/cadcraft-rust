//! PartCraft's move / turn gizmo in an assembly (asm-ui.js `startGizmo` / `startDrag` / the explode step's Drag
//! direction): three.js's TransformControls (gizmo3.rs) on a stand-in at the middle of the parts. Drag to move or Drag
//! to turn on a part copy that is not joined moves it by hand (one undo step per drag, the gizmo stays until Esc or
//! the button again); a new exploded step with the direction "Drag an arrow in the view" adds the step the drag made.
//!
//! The pointer drives it from the view (hover lights a handle, a press on one drags it, letting go applies it) and
//! from scripts: `part.asm=gizmo:x,y;x,y;…` is one gesture in view pixels (physical): the pointer moves to the first
//! point, presses, goes through the others and lets go there; the trace note says the camera it was seen with (for
//! the parity harness, which replays the gesture through three's TransformControls with that camera).

use cc_part::{js, jv, vec as V};
use serde_json::Value;

use crate::gizmo3::{self as G, Mode, Tc};
use crate::partview::{PartView, tr, trv};

/// asm-ui.js `st.gizmo`.
#[derive(Clone, Debug)]
pub struct Gizmo {
    pub tc: Tc,
    pub insts: Vec<String>,
    /// "move" (a part copy by hand) or "explode" (the new step's Drag direction)
    pub purpose: &'static str,
    /// the stand-in's matrix where the drag starts from (`g.start`)
    pub start: G::M4,
    /// the parts' shown placements when the drag began (`g.base`)
    pub base: Option<Vec<(String, V::M4)>>,
    /// what the drawn handles were made from
    lines_key: Option<(cc_render::mesh3d::Cam3, Option<&'static str>, bool, G::V3, G::Q4)>,
}

/// The view is Z up, the part Y up: view (x, y, z) = part (x, −z, y).
fn to_view(p: G::V3) -> [f64; 3] {
    [p[0], -p[2], p[1]]
}
fn to_part(p: [f64; 3]) -> G::V3 {
    [p[0], p[2], -p[1]]
}

const IDENT: V::M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

impl PartView {
    /// The view's camera as three.js would hold it in the part's frame (Y up).
    pub fn gizmo_camera(&self) -> G::Camera {
        let c = &self.view.cam;
        let (eye, target) = (to_part(c.eye), to_part(c.target));
        if c.persp {
            G::Camera::perspective(eye, target, [0.0, 1.0, 0.0], c.fov, c.width / c.height, c.near, c.far)
        } else {
            let hh = c.distance() * (c.fov.to_radians() / 2.0).tan();
            let hw = hh * c.aspect();
            G::Camera::orthographic(eye, target, [0.0, 1.0, 0.0], [-hw, hw, hh, -hh], -c.far, c.far)
        }
    }

    pub fn gizmo(&self) -> Option<&Gizmo> {
        self.asm.as_ref()?.tools.gizmo.as_ref()
    }
    fn gizmo_mut(&mut self) -> Option<&mut Gizmo> {
        self.asm.as_mut()?.tools.gizmo.as_mut()
    }

    /// `placeProxy()`: the stand-in at the middle of the parts as shown (not while a drag is under way).
    pub fn asm_place_proxy(&mut self) {
        let Some(g) = self.gizmo() else { return };
        if g.tc.dragging {
            return;
        }
        let insts = g.insts.clone();
        let shown = self.asm_display_world();
        let cs: Vec<[f64; 3]> = insts.iter().map(|id| self.asm_centre_of(id, &shown)).collect();
        let n = cs.len().max(1) as f64;
        let c = V::mul(cs.iter().fold([0.0; 3], |s, p| V::add(s, *p)), 1.0 / n);
        if let Some(g) = self.gizmo_mut() {
            g.start = G::place(&mut g.tc, c);
            g.lines_key = None;
        }
    }

    /// `startGizmo(insts, mode, purpose)`.
    fn asm_start_gizmo(&mut self, insts: Vec<String>, mode: Mode, purpose: &'static str) {
        self.asm_stop_gizmo();
        let Some(st) = self.asm.as_mut() else { return };
        st.tools.gizmo = Some(Gizmo { tc: Tc::new(mode, 0.9), insts, purpose, start: IDENT, base: None, lines_key: None });
        self.asm_place_proxy();
        self.asm_scene();
    }

    /// `stopGizmo()`.
    pub fn asm_stop_gizmo(&mut self) {
        if let Some(st) = self.asm.as_mut() {
            st.tools.gizmo = None;
        }
        self.scene.top_lines = None;
    }

    /// `startDrag(id, mode)`: Drag to move / Drag to turn (the same again stops).
    pub fn asm_start_drag(&mut self, iid: &str, mode: Mode) {
        let same = self.gizmo().is_some_and(|g| g.purpose == "move" && g.insts.first().is_some_and(|x| x == iid) && g.tc.mode == mode);
        if same {
            self.asm_stop_gizmo();
            self.asm_scene();
            return;
        }
        self.asm_start_gizmo(vec![iid.to_string()], mode, "move");
        self.note = tr(if mode == Mode::Rotate { "ui.asm.drag.turn_hint" } else { "ui.asm.drag.move_hint" });
    }

    /// The new step's Drag direction: the gizmo on the ticked parts (asm-ui.js `add` with `d.dir === 'drag'`).
    pub fn asm_start_step_drag(&mut self, insts: Vec<String>) {
        self.asm_start_gizmo(insts, Mode::Translate, "explode");
        self.note = tr("ui.asm.step.drag_hint");
    }

    /// Esc with the gizmo up (asm-ui.js `keydown`): whether it took the key.
    pub fn asm_gizmo_escape(&mut self) -> bool {
        if self.gizmo().is_none() {
            return false;
        }
        self.asm_stop_gizmo();
        self.asm_scene();
        self.note = tr("ui.asm.stopped");
        true
    }

    /// A render before a pointer event: the handles and the drag plane follow the camera and the stand-in.
    fn gizmo_render(&mut self) {
        let cam = self.gizmo_camera();
        if let Some(g) = self.gizmo_mut() {
            g.tc.update(&cam);
        }
    }

    fn ndc(&self, x: f64, y: f64) -> (f64, f64) {
        G::ndc(x, y, self.view.cam.width, self.view.cam.height)
    }

    /// The pointer over the view (no button): the handle under it lights.
    pub fn asm_gizmo_hover(&mut self, x: f64, y: f64) {
        self.gizmo_render();
        let (a, b) = self.ndc(x, y);
        if let Some(g) = self.gizmo_mut() {
            g.tc.hover(a, b);
        }
    }

    /// The left button down at a view pixel → whether a handle took it (the drag begins).
    pub fn asm_gizmo_down(&mut self, x: f64, y: f64) -> bool {
        self.gizmo_render();
        let (a, b) = self.ndc(x, y);
        let shown = self.asm_display_world();
        let Some(g) = self.gizmo_mut() else { return false };
        if !g.tc.down(a, b) {
            return false;
        }
        // dragging-changed: the parts as they are drawn now
        g.base = Some(shown.into_iter().filter(|(k, _)| g.insts.contains(k)).collect());
        true
    }

    /// The pointer moves while dragging: the parts follow (what is drawn only).
    pub fn asm_gizmo_move(&mut self, x: f64, y: f64) {
        self.gizmo_render();
        let (a, b) = self.ndc(x, y);
        let Some(g) = self.gizmo_mut() else { return };
        if g.tc.mv(a, b) {
            let d = G::delta_of(&g.tc, &g.start);
            let moved: Vec<(String, V::M4)> = g.base.iter().flatten().map(|(k, m)| (k.clone(), V::mat_mul(&d, m))).collect();
            self.asm_show_moved(&moved);
        }
    }

    /// The button up: three's `mouseUp` → the drag applied (`onDone(deltaOf(g))`).
    pub fn asm_gizmo_up(&mut self) {
        let Some(g) = self.gizmo_mut() else { return };
        if !g.tc.up() {
            return;
        }
        let d = G::delta_of(&g.tc, &g.start);
        g.base = None;
        let (purpose, insts, mode) = (g.purpose, g.insts.clone(), g.tc.mode);
        if purpose == "explode" {
            self.asm_step_dragged(&insts, &d);
        } else if let Some(iid) = insts.first() {
            self.asm_moved_by_hand(iid, mode, &d);
        }
        // the parts drawn where they are kept, the stand-in at their middle again (afterShow)
        self.asm_scene();
    }

    /// startDrag's onDone: the part copy moved by the drag, one undo step.
    fn asm_moved_by_hand(&mut self, iid: &str, mode: Mode, d: &V::M4) {
        let Some(st) = self.asm.as_ref() else { return };
        let Some(inst) = st.inst(iid) else { return };
        if d.iter().zip(IDENT).all(|(v, i)| (v - i).abs() < 1e-12) {
            return;
        }
        let a = jv::arr(inst.get("place"));
        let mut place = [0.0; 16];
        for (k, x) in place.iter_mut().enumerate() {
            *x = a.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
        }
        let np = V::mat_mul(d, &place);
        let label = tr(if mode == Mode::Rotate { "ui.asm.undo.turn_hand" } else { "ui.asm.undo.move_hand" });
        let id = iid.to_string();
        if let Some(Err(e)) = self.asm_commit(&label, |a| cc_part::asm::move_instance(a, &id, &jv::nums(&np))) {
            self.note = e;
        }
    }

    /// The explode step's drag: a step along the drag, as long as it (nothing when it did not move).
    fn asm_step_dragged(&mut self, insts: &[String], m: &V::M4) {
        let vec = [m[12], m[13], m[14]];
        let l = V::len(vec);
        self.asm_stop_gizmo();
        if l < 1e-6 {
            return;
        }
        self.asm_set_view(&tr("ui.asm.undo.step"), |a, vid| cc_part::explode::add_step(a, vid, insts, vec, l).map(|_| ()));
        if let Some(st) = self.asm.as_mut() {
            st.tools.t = 1.0;
        }
        self.note = trv("ui.asm.step.added", &[("dist", V::fmt(l, 3))]);
    }

    /// Some part copies drawn at other placements (the drag under way): the instances' matrices only.
    fn asm_show_moved(&mut self, moved: &[(String, V::M4)]) {
        let zm: V::M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        let zi: V::M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        let ids = self.asm_inst_ids.clone();
        let list: Vec<cc_render::mesh3d::Instance3> = self
            .scene
            .instances
            .iter()
            .enumerate()
            .map(|(i, x)| match ids.get(i).and_then(|id| moved.iter().find(|(k, _)| k == id)) {
                Some((_, w)) => cc_render::mesh3d::Instance3 { m: V::mat_mul(&zm, &V::mat_mul(w, &zi)), ..*x },
                None => *x,
            })
            .collect();
        self.scene.instances = std::sync::Arc::new(list);
        self.scene.version = self.scene.version.wrapping_add(1);
    }

    /// The handles drawn over the view (made again when the camera, the handle in use or the stand-in changed).
    pub fn asm_gizmo_draw(&mut self) {
        let cam3 = self.view.cam;
        let cam = self.gizmo_camera();
        let dark = self.dark;
        let have = self.scene.top_lines.is_some();
        let Some(g) = self.gizmo_mut() else { return };
        g.tc.update(&cam);
        let key = (cam3, g.tc.axis, g.tc.dragging, g.tc.position, g.tc.quaternion);
        if g.lines_key == Some(key) && have {
            return;
        }
        g.lines_key = Some(key);
        let segs: Vec<([f64; 3], [f64; 3], [u8; 4])> =
            g.tc.lines()
                .into_iter()
                .map(|(a, b, c)| {
                    // three's helpers are white at half strength: dark lines on a light view
                    let c = if !dark && c == [255, 255, 255, 128] { [40, 40, 40, 140] } else { c };
                    (to_view(a), to_view(b), c)
                })
                .collect();
        self.scene.top_lines = Some(std::sync::Arc::new(cc_render::lines3::Lines3 { segs }));
    }

    /// `part.asm=gizmo:<start>;dx,dy;dx,dy…`: one gesture in view pixels (physical). The start is a pixel `x,y`, or
    /// `at:u,v,w`: where the point u, v, w handle lengths from the gizmo's middle (the part's axes) is seen (`at:0.3,0,0`
    /// the X arrow); the others are offsets from the start. The pointer moves to the start, presses, goes through the
    /// others and lets go at the last. The trace note gives the camera, the pixels, the handle taken and the drag.
    pub fn asm_gizmo_gesture(&mut self, spec: &str) -> Result<(), String> {
        if self.gizmo().is_none() {
            return Err("part.asm: gizmo: no gizmo (Drag to move, Drag to turn or a Drag step first)".into());
        }
        let num = |t: &str| -> Result<f64, String> {
            let x = js::str_num(t);
            if x.is_finite() { Ok(x) } else { Err(format!("part.asm: gizmo: {t} is not a number")) }
        };
        let mut parts = spec.split(';').filter(|p| !p.trim().is_empty());
        let first = parts.next().ok_or("part.asm: gizmo: no points")?;
        self.gizmo_render();
        let (x0, y0) = if let Some(at) = first.strip_prefix("at:") {
            let v: Vec<f64> = at.split(',').map(num).collect::<Result<_, _>>()?;
            let [u, vv, w] = v.as_slice() else { return Err("part.asm: gizmo: at:u,v,w".into()) };
            let cam = self.gizmo_camera();
            let g = self.gizmo().ok_or("part.asm: gizmo: no gizmo")?;
            let p = g.tc.handle_point([*u, *vv, *w]);
            cam.pixel(p, self.view.cam.width, self.view.cam.height)
        } else {
            let (x, y) = first.split_once(',').ok_or_else(|| format!("part.asm: gizmo: x,y expected, not {first}"))?;
            (num(x)?, num(y)?)
        };
        let mut pts = vec![(x0, y0)];
        for p in parts.take(10_000) {
            let (x, y) = p.split_once(',').ok_or_else(|| format!("part.asm: gizmo: dx,dy expected, not {p}"))?;
            pts.push((x0 + num(x)?, y0 + num(y)?));
        }
        let c = &self.view.cam;
        let cam = serde_json::json!({ "eye": to_part(c.eye), "target": to_part(c.target), "fov": c.fov, "w": c.width, "h": c.height, "near": c.near, "far": c.far, "persp": c.persp });
        self.asm_gizmo_hover(x0, y0);
        let took = self.asm_gizmo_down(x0, y0);
        let axis = self.gizmo().and_then(|g| g.tc.axis).unwrap_or("");
        let mut d = Vec::new();
        if took {
            for &(x, y) in pts.iter().skip(1) {
                self.asm_gizmo_move(x, y);
            }
            if let Some(g) = self.gizmo() {
                d = G::delta_of(&g.tc, &g.start).to_vec();
            }
            self.asm_gizmo_up();
        }
        let pts: Vec<[f64; 2]> = pts.iter().map(|&(x, y)| [x, y]).collect();
        self.trace_note = serde_json::json!({ "cam": cam, "pts": pts, "axis": axis, "d": d }).to_string();
        Ok(())
    }
}

/// The view's pointer while the gizmo is up (partasmui.rs `stage_input` calls it first) → whether it took the
/// pointer this frame (a press on a handle, a drag under way).
pub fn stage_pointer(ui: &egui::Ui, resp: &egui::Response, rect: egui::Rect, pv: &mut PartView) -> bool {
    if pv.gizmo().is_none() {
        return false;
    }
    let ppp = f64::from(ui.ctx().pixels_per_point());
    let to_px = |p: egui::Pos2| (f64::from(p.x - rect.min.x) * ppp, f64::from(p.y - rect.min.y) * ppp);
    let (pressed, released, moved, origin, at) = ui.input(|i| {
        (
            i.pointer.primary_pressed(),
            i.pointer.primary_released(),
            i.pointer.delta() != egui::Vec2::ZERO,
            i.pointer.press_origin(),
            i.pointer.latest_pos(),
        )
    });
    let dragging = pv.gizmo().is_some_and(|g| g.tc.dragging);
    let mut took = dragging;
    if pressed && let Some(o) = origin.filter(|o| rect.contains(*o)) {
        let (x, y) = to_px(o);
        took = pv.asm_gizmo_down(x, y);
    } else if dragging && moved {
        if let Some(p) = at {
            let (x, y) = to_px(p);
            pv.asm_gizmo_move(x, y);
        }
    } else if !dragging
        && moved
        && resp.hovered()
        && let Some(p) = at
    {
        let (x, y) = to_px(p);
        pv.asm_gizmo_hover(x, y);
    }
    if released && pv.gizmo().is_some_and(|g| g.tc.dragging) {
        pv.asm_gizmo_up();
    }
    pv.asm_gizmo_draw();
    if took {
        ui.ctx().request_repaint();
    }
    took
}

#[cfg(test)]
mod tests {
    use crate::{Params, UiApp};

    fn run(app: &mut UiApp, id: &str, name: &str) -> Result<(), String> {
        app.run(id, &Params { name: Some(name.into()), ..Default::default() })
    }

    fn place_of(app: &UiApp, iid: &str) -> Vec<f64> {
        let st = app.part.as_ref().and_then(|p| p.asm.as_ref()).unwrap();
        cc_part::jv::arr(st.inst(iid).unwrap().get("place")).iter().map(|v| v.as_f64().unwrap()).collect()
    }

    #[test]
    fn drag_to_move_turn_and_a_dragged_step() {
        let mut app = UiApp::default();
        run(&mut app, "part.sample", "").unwrap();
        run(&mut app, "part.asm", "new").unwrap();
        run(&mut app, "part.asm", "insert-copy").unwrap();
        run(&mut app, "part.asm", "sel:inst:i3").unwrap();
        {
            let pv = app.part.as_mut().unwrap();
            pv.view.cam.width = 800.0;
            pv.view.cam.height = 600.0;
        }
        assert!(run(&mut app, "part.asm", "gizmo:at:0.3,0,0;40,0").is_err(), "no gizmo yet");
        run(&mut app, "part.asm.set", "drag=translate").unwrap();
        let before = place_of(&app, "i3");
        // hostile gestures: refused or harmless
        for bad in ["", "at:", "at:1,2", "at:x,0,0", "1e999,0", "NaN,1", "5", "at:0,0,0;a,b"] {
            assert!(run(&mut app, "part.asm", &format!("gizmo:{bad}")).is_err(), "{bad}");
        }
        assert!(run(&mut app, "part.asm.set", "drag=scale").is_err());
        run(&mut app, "part.asm", "gizmo:at:5,5,5;30,0").unwrap();
        assert_eq!(place_of(&app, "i3"), before, "nothing under the pointer: no change");
        run(&mut app, "part.asm", "gizmo:at:0.3,0,0;10,0;40,0").unwrap();
        let moved = place_of(&app, "i3");
        assert!(moved[12] != before[12] && moved[13] == before[13] && moved[14] == before[14], "{moved:?}");
        // a turn: the same button again stops, Esc stops
        run(&mut app, "part.asm.set", "drag=rotate").unwrap();
        run(&mut app, "part.asm", "gizmo:at:0.3536,0,0.3536;30,0").unwrap();
        let turned = place_of(&app, "i3");
        assert!(turned[0] != moved[0], "{turned:?}");
        run(&mut app, "part.asm.set", "drag=rotate").unwrap();
        assert!(app.part.as_ref().unwrap().gizmo().is_none());
        run(&mut app, "part.asm.set", "drag=translate").unwrap();
        run(&mut app, "part.asm", "key:Escape").unwrap();
        assert!(app.part.as_ref().unwrap().gizmo().is_none());
        assert_eq!(app.part.as_ref().unwrap().note, crate::partview::tr("ui.asm.stopped"));
        // undo takes the turn back
        run(&mut app, "part.undo", "").unwrap();
        assert_eq!(place_of(&app, "i3"), moved);
        // an exploded step dragged along an arrow
        run(&mut app, "part.asm", "explode-new").unwrap();
        run(&mut app, "part.asm", "step-dir:drag").unwrap();
        run(&mut app, "part.asm", "step-inst:i3=on").unwrap();
        run(&mut app, "part.asm", "step-add").unwrap();
        assert!(app.part.as_ref().unwrap().gizmo().is_some_and(|g| g.purpose == "explode"));
        run(&mut app, "part.asm", "gizmo:at:0,0.3,0;0,-40").unwrap();
        assert!(app.part.as_ref().unwrap().gizmo().is_none(), "a dragged step ends the gizmo");
        assert!(app.part.as_ref().unwrap().asm_steps_words().contains("1."));
    }
}
