//! DetailCraft's corner indicator in the 3D view (Settings › DetailCraft › Corner indicator; detailcraft.js `syncCorner`
//! with viewcube.js through viewcube-three.js): the view cube at the top right (click a face to look from it, the step
//! arrows face-on, a compass letter, Home = fit, drag it to orbit), the X/Y/Z axis icon at the bottom left, or nothing.
//! Plan and elevation are fixed square views: neither shows there. The cube and the icon are the 3D tab's
//! (cad3dtab.rs `cube_draw`, `axis_icon`, shared with ModelCraft), turned onto the view's camera here.

use cc_m3d_tools::camera::Axes;
use cc_render::mesh3d::Cam3;

use crate::detailview::{DetailView, ViewMode};
use crate::theme::Tokens;

type P3 = [f64; 3];

fn axes(cam: &Cam3) -> Axes {
    let (x, y, z) = cam.axes();
    Axes { x, y, z }
}

/// viewcube-three.js `applyThreeView`: the camera looks along `dir` (screen-up `up` where it can), same target and
/// distance; straight down or up is tilted 0.0002 rad towards screen-up, as OrbitControls keeps it.
pub fn look(cam: &mut Cam3, dir: P3, up: Option<P3>) {
    let (_, u, d) = cc_m3d_tools::viewcube_camera::basis(dir, up);
    let off0 = [cam.eye[0] - cam.target[0], cam.eye[1] - cam.target[1], cam.eye[2] - cam.target[2]];
    let dist = (off0[0] * off0[0] + off0[1] * off0[1] + off0[2] * off0[2]).sqrt();
    let dist = if dist.is_finite() && dist > 1e-6 { dist } else { 10.0 };
    let mut off = [-d[0], -d[1], -d[2]];
    if d[2].abs() > 0.9999 {
        let k = 2e-4 * d[2].signum();
        let o = [off[0] + u[0] * k, off[1] + u[1] * k, off[2] + u[2] * k];
        let l = (o[0] * o[0] + o[1] * o[1] + o[2] * o[2]).sqrt().max(1e-12);
        off = [o[0] / l, o[1] / l, o[2] / l];
    }
    cam.eye = [cam.target[0] + off[0] * dist, cam.target[1] + off[1] * dist, cam.target[2] + off[2] * dist];
}

impl DetailView {
    /// The corner indicator shown now: the setting in 3D, nothing in plan or elevation.
    pub fn corner(&self) -> &str {
        if self.mode == ViewMode::D3 { self.settings.corner.as_str() } else { "none" }
    }

    /// Does the view cube take a press at `p` (screen points)?
    pub fn on_cube(&self, rect: egui::Rect, p: egui::Pos2) -> bool {
        self.corner() == "cube" && crate::cad3dtab::in_cube(rect, p)
    }

    /// One of the cube's actions on the view's camera (`detail.cube`): a face name, `step:<up|down|left|right|cw|ccw>`,
    /// `letter:<n|e|s|w>` or `home`.
    pub fn cube_act(&mut self, act: &str) -> Result<(), String> {
        if self.mode != ViewMode::D3 {
            return Err("detail.cube: the view cube is in the 3D view only".into());
        }
        let ax = axes(&self.view.cam);
        let view_dir = [-ax.z[0], -ax.z[1], -ax.z[2]];
        if act == "home" {
            // view3d.js fit(): the whole model from the standard corner
            self.camera("iso")?;
        } else if let Some(which) = act.strip_prefix("step:") {
            cc_m3d_tools::viewcube_camera::face_on(view_dir, Some(ax.y)).ok_or("detail.cube: the view does not look straight at a side")?;
            let (dir, up) = cc_m3d_tools::viewcube_camera::step_view(view_dir, Some(ax.y), which)
                .ok_or_else(|| format!("detail.cube: no step {which} (up, down, left, right, cw, ccw)"))?;
            look(&mut self.view.cam, dir, Some(up));
        } else if let Some(k) = act.strip_prefix("letter:") {
            let side = match k {
                "n" => [0.0, 1.0],
                "e" => [1.0, 0.0],
                "s" => [0.0, -1.0],
                "w" => [-1.0, 0.0],
                _ => return Err(format!("detail.cube: no letter {k} (n, e, s, w)")),
            };
            // look from that side, keeping the tilt (viewcube.js viewFromLetter)
            let z = ax.z[2].clamp(-0.9999, 0.9999);
            let h = (1.0 - z * z).sqrt();
            look(&mut self.view.cam, [-side[0] * h, -side[1] * h, -z], None);
        } else {
            let d = cc_m3d_tools::camera::view_dir(act).ok_or_else(|| format!("detail.cube: no view {act}"))?;
            look(&mut self.view.cam, [-d[0], -d[1], -d[2]], None);
        }
        Ok(())
    }

    /// Draw the corner indicator over the view and act on the cube.
    pub fn corner_ui(&mut self, ui: &mut egui::Ui, rect: egui::Rect, t: &Tokens) {
        match self.corner() {
            "cube" => {
                let tr = |k: &str, d: &str| {
                    let w = cc_detail::i18n::t0(k);
                    if w == k { d.to_string() } else { w }
                };
                let (act, on_cube_drag, delta) = crate::cad3dtab::cube_draw(ui, rect, t, axes(&self.view.cam), &tr, "dc_wcs");
                if on_cube_drag {
                    let ppp = self.ppp;
                    self.view.cam.orbit(f64::from(delta.x) * ppp, f64::from(delta.y) * ppp);
                    ui.ctx().request_repaint();
                }
                if let Some(a) = act {
                    let _ = self.cube_act(&a);
                    ui.ctx().request_repaint();
                }
            }
            "axes" => crate::cad3dtab::axis_icon(ui, rect, t, axes(&self.view.cam)),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cube_turns_the_camera_and_steps_face_on() {
        let mut dv = DetailView::sample().expect("sample");
        dv.cube_act("top").expect("top");
        let (_, _, z) = dv.view.cam.axes();
        assert!(z[2] > 0.999, "looking down");
        dv.cube_act("step:down").expect("step");
        let (_, _, z) = dv.view.cam.axes();
        assert!(z[2].abs() < 1e-3, "a side view after stepping down from the top");
        dv.cube_act("letter:n").expect("letter");
        let (_, _, z) = dv.view.cam.axes();
        assert!(z[1] > 0.99, "looking from the north");
        assert!(dv.cube_act("letter:q").is_err());
        assert!(dv.cube_act("nowhere").is_err());
        dv.set_view("plan").expect("plan");
        assert_eq!(dv.corner(), "none");
        assert!(dv.cube_act("top").is_err());
    }
}
