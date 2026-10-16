//! The view cube's link to the camera (viewcube-camera.js `cameraView` / `applyView`, with viewcube.js's `basis`):
//! the camera keeps Z up and cannot roll, so a straight-down or straight-up view is tilted by 0.0002 rad toward the
//! wanted screen-up. The cube's drawing, animation and DOM stay in the UI.

use cc_m3d::vec::{P3, cross, len};

use crate::camera::Camera;
use crate::js::{hypot3, sign};

fn add(a: P3, b: P3) -> P3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: P3, s: f64) -> P3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
/// viewcube-camera.js's own `norm` (zero below 1e-12).
fn norm12(a: P3) -> P3 {
    let l = hypot3(a[0], a[1], a[2]);
    if l > 1e-12 { mul(a, 1.0 / l) } else { [0.0, 0.0, 0.0] }
}

/// A view for the cube: direction looked along, screen-up, perspective.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CubeView {
    pub dir: P3,
    pub up: P3,
    pub persp: bool,
}

/// `cameraView(cam)`.
pub fn camera_view(cam: &Camera) -> CubeView {
    let ax = cam.axes();
    CubeView { dir: mul(ax.z, -1.0), up: ax.y, persp: cam.persp }
}

/// viewcube.js `basis(view)`: right, up, direction.
pub fn basis(dir: P3, up: Option<P3>) -> (P3, P3, P3) {
    let d = norm12(dir);
    let mut r = cross(d, up.unwrap_or([0.0, 0.0, 1.0]));
    if len(r) < 1e-9 {
        r = cross(d, if d[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] });
    }
    let r = norm12(r);
    (r, cross(r, d), d)
}

/// `applyView(cam, view)`: point the camera along `dir` (screen-up `up` where it can), keeping target and distance.
pub fn apply_view(cam: &mut Camera, dir: P3, up: Option<P3>) {
    let (_, u, d) = basis(dir, up);
    let dist = crate::js::or(cam.distance(), 10.0);
    let mut off = mul(d, -1.0);
    if d[2].abs() > 0.9999 {
        off = norm12(add(mul(d, -1.0), mul(u, 2e-4 * sign(d[2]))));
    }
    cam.eye = add(cam.target, mul(off, dist));
}
