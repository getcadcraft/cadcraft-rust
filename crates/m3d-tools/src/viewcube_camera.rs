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

/// viewcube.js `AXES6`.
const AXES6: [P3; 6] = [[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]];

/// viewcube.js `snapTo(v, AXES6)`: the axis direction nearest `v` (the first of equals).
fn snap6(v: P3) -> P3 {
    let mut best = AXES6[0];
    for a in AXES6 {
        if cc_m3d::vec::dot(a, v) > cc_m3d::vec::dot(best, v) {
            best = a;
        }
    }
    best
}

/// viewcube.js `keyOf` for an axis direction: the face it points at.
fn face_key(v: P3) -> &'static str {
    if v[2] > 0.5 {
        "top"
    } else if v[2] < -0.5 {
        "bottom"
    } else if v[1] < -0.5 {
        "front"
    } else if v[1] > 0.5 {
        "back"
    } else if v[0] < -0.5 {
        "left"
    } else {
        "right"
    }
}

/// viewcube.js `faceOn(view)`: looking straight at a face with the screen square to the axes → that face, else None
/// (the cube then shows its face-on step arrows).
pub fn face_on(dir: P3, up: Option<P3>) -> Option<&'static str> {
    let (_, u, d) = basis(dir, up);
    let (da, ua) = (snap6(d), snap6(u));
    if 1.0 - cc_m3d::vec::dot(da, d) > 1e-3 || 1.0 - cc_m3d::vec::dot(ua, u) > 1e-3 {
        return None;
    }
    Some(face_key(mul(da, -1.0)))
}

/// viewcube.js `stepView(view, which)`: 'up' | 'down' | 'left' | 'right' bring the neighbouring face on that side of
/// the screen to the front; 'cw' | 'ccw' turn the picture a quarter turn about the line of sight. → (dir, up), or
/// None for another word.
pub fn step_view(dir: P3, up: Option<P3>, which: &str) -> Option<(P3, P3)> {
    let (r, u, d) = basis(dir, up);
    let (r, u, d) = (snap6(r), snap6(u), snap6(d));
    Some(match which {
        "up" => (mul(u, -1.0), d),
        "down" => (u, mul(d, -1.0)),
        "left" => (r, u),
        "right" => (mul(r, -1.0), u),
        "cw" => (d, mul(r, -1.0)),
        "ccw" => (d, r),
        _ => return None,
    })
}

#[cfg(test)]
mod step_tests {
    use super::*;

    #[test]
    fn face_on_and_steps_as_viewcube_js() {
        // looking from the front (dir = +Y), Z up
        assert_eq!(face_on([0.0, 1.0, 0.0], Some([0.0, 0.0, 1.0])), Some("front"));
        assert_eq!(face_on([0.3, 1.0, -0.2], Some([0.0, 0.0, 1.0])), None);
        // straight down from the top, north up
        assert_eq!(face_on([0.0, 0.0, -1.0], Some([0.0, 1.0, 0.0])), Some("top"));
        // from the front: up brings the top (look down, the front's direction up), right brings the right side
        let f = ([0.0, 1.0, 0.0], Some([0.0, 0.0, 1.0]));
        assert_eq!(step_view(f.0, f.1, "up"), Some(([0.0, 0.0, -1.0], [0.0, 1.0, 0.0])));
        assert_eq!(step_view(f.0, f.1, "down"), Some(([0.0, 0.0, 1.0], [0.0, -1.0, 0.0])));
        assert_eq!(step_view(f.0, f.1, "right"), Some(([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0])));
        assert_eq!(step_view(f.0, f.1, "left"), Some(([1.0, 0.0, 0.0], [0.0, 0.0, 1.0])));
        assert_eq!(step_view(f.0, f.1, "cw"), Some(([0.0, 1.0, 0.0], [-1.0, 0.0, 0.0])));
        assert_eq!(step_view(f.0, f.1, "ccw"), Some(([0.0, 1.0, 0.0], [1.0, 0.0, 0.0])));
        assert_eq!(step_view(f.0, f.1, "sideways"), None);
        // the camera from the top turned a quarter (cw): straight down with east up
        let mut cam = crate::camera::Camera::default();
        let (d, u) = step_view([0.0, 0.0, -1.0], Some([0.0, 1.0, 0.0]), "cw").unwrap();
        apply_view(&mut cam, d, Some(u));
        let v = camera_view(&cam);
        assert_eq!(face_on(v.dir, Some(v.up)), Some("top"));
        assert!(cc_m3d::vec::dot(v.up, [-1.0, 0.0, 0.0]) > 0.999, "{:?}", v.up);
    }
}
