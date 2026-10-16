//! The ModelCraft camera (camera.js, one for one): Z up, column-major OpenGL clip space like three.js, picking rays
//! through the same matrices that draw. Screen pixels from the top-left. `Math.sin` / `Math.cos` are V8's (fdlibm,
//! `cc_m3d::jsmath`); tan / atan2 / acos are `libm`'s (FreeBSD msun, as V8).

use cc_m3d::jsmath::{fd_cos, fd_sin};
use cc_m3d::vec::{M4, P3, Plane, add, cross, dist, dot, len, mmul, mul, norm, ray_plane, sub};

use crate::js::{jmax, jmin};

/// `VIEWS`: the standard views' directions (from the target toward the eye).
pub fn view_dir(name: &str) -> Option<P3> {
    Some(match name {
        "iso" => [1.0, -1.15, 0.85],
        "top" => [0.0, -1e-4, 1.0],
        "bottom" => [0.0, -1e-4, -1.0],
        "front" => [0.0, -1.0, 0.0],
        "back" => [0.0, 1.0, 0.0],
        "left" => [-1.0, 0.0, 0.0],
        "right" => [1.0, 0.0, 0.0],
        _ => return None,
    })
}

/// The camera's own axes: x right, y up, z back (from the target toward the eye).
#[derive(Clone, Copy, Debug)]
pub struct Axes {
    pub x: P3,
    pub y: P3,
    pub z: P3,
}

/// A pick ray (`d` unit).
#[derive(Clone, Copy, Debug)]
pub struct Ray {
    pub o: P3,
    pub d: P3,
}

/// An axis-aligned box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Box3 {
    pub min: P3,
    pub max: P3,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Camera {
    pub eye: P3,
    pub target: P3,
    /// vertical field of view, degrees
    pub fov: f64,
    pub persp: bool,
    pub w: f64,
    pub h: f64,
    pub near: f64,
    pub far: f64,
    pub scene_radius: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Self::new()
    }
}

/// `Math.tan((fov * Math.PI) / 360)`.
fn half_tan(fov: f64) -> f64 {
    libm::tan((fov * std::f64::consts::PI) / 360.0)
}

impl Camera {
    pub fn new() -> Camera {
        Camera {
            eye: [9.0, -12.0, 8.0],
            target: [0.0, 0.0, 1.0],
            fov: 35.0,
            persp: true,
            w: 800.0,
            h: 600.0,
            near: 0.05,
            far: 5000.0,
            scene_radius: 20.0,
        }
    }
    pub fn set_size(&mut self, w: f64, h: f64) {
        self.w = jmax(1.0, w);
        self.h = jmax(1.0, h);
    }
    pub fn aspect(&self) -> f64 {
        self.w / self.h
    }
    pub fn distance(&self) -> f64 {
        dist(self.eye, self.target)
    }
    /// Height of the parallel view (m): what the perspective view shows at the target distance.
    pub fn height(&self) -> f64 {
        2.0 * self.distance() * half_tan(self.fov)
    }
    pub fn axes(&self) -> Axes {
        let z = norm(sub(self.eye, self.target));
        let mut x = cross([0.0, 0.0, 1.0], z);
        if len(x) < 1e-9 {
            x = [1.0, 0.0, 0.0];
        }
        let x = norm(x);
        Axes { x, y: cross(z, x), z }
    }
    /// Near/far planes from the model's size so nothing clips and depth stays precise.
    pub fn clip(&mut self, radius: f64) {
        self.scene_radius = jmax(radius, 1.0);
        let d = self.distance();
        self.far = d + self.scene_radius * 4.0 + 100.0;
        self.near = jmax(self.far / 1e5, jmin(d * 0.01, 0.05));
    }
    /// `clip()` with the default radius (the current scene radius).
    pub fn clip_default(&mut self) {
        let r = self.scene_radius;
        self.clip(r);
    }
    pub fn view(&self) -> M4 {
        let Axes { x, y, z } = self.axes();
        let e = self.eye;
        [x[0], y[0], z[0], 0.0, x[1], y[1], z[1], 0.0, x[2], y[2], z[2], 0.0, -dot(x, e), -dot(y, e), -dot(z, e), 1.0]
    }
    pub fn proj(&self) -> M4 {
        let (n, f) = (self.near, self.far);
        if self.persp {
            let t = 1.0 / half_tan(self.fov);
            return [
                t / self.aspect(),
                0.0,
                0.0,
                0.0,
                0.0,
                t,
                0.0,
                0.0,
                0.0,
                0.0,
                (f + n) / (n - f),
                -1.0,
                0.0,
                0.0,
                (2.0 * f * n) / (n - f),
                0.0,
            ];
        }
        let hh = self.height() / 2.0;
        let hw = hh * self.aspect();
        let nn = -f;
        [1.0 / hw, 0.0, 0.0, 0.0, 0.0, 1.0 / hh, 0.0, 0.0, 0.0, 0.0, -2.0 / (f - nn), 0.0, 0.0, 0.0, -(f + nn) / (f - nn), 1.0]
    }
    pub fn view_proj(&self) -> M4 {
        mmul(&self.proj(), &self.view())
    }
    /// Screen pixels and depth, or None behind the eye.
    pub fn project_vp(&self, p: P3, vp: &M4) -> Option<[f64; 3]> {
        let x = vp[0] * p[0] + vp[4] * p[1] + vp[8] * p[2] + vp[12];
        let y = vp[1] * p[0] + vp[5] * p[1] + vp[9] * p[2] + vp[13];
        let z = vp[2] * p[0] + vp[6] * p[1] + vp[10] * p[2] + vp[14];
        let w = vp[3] * p[0] + vp[7] * p[1] + vp[11] * p[2] + vp[15];
        // NaN passes (the JS compares the same way and returns NaNs)
        if w <= 1e-9 {
            return None;
        }
        Some([(x / w + 1.0) * self.w / 2.0, (1.0 - y / w) * self.h / 2.0, z / w])
    }
    pub fn project(&self, p: P3) -> Option<[f64; 3]> {
        let vp = self.view_proj();
        self.project_vp(p, &vp)
    }
    pub fn ray(&self, sx: f64, sy: f64) -> Ray {
        let Axes { x, y, z } = self.axes();
        let nx = (2.0 * sx) / self.w - 1.0;
        let ny = 1.0 - (2.0 * sy) / self.h;
        let th = half_tan(self.fov);
        if self.persp {
            return Ray { o: self.eye, d: norm(add(add(mul(x, nx * th * self.aspect()), mul(y, ny * th)), mul(z, -1.0))) };
        }
        let hh = self.height() / 2.0;
        let hw = hh * self.aspect();
        let o = add(add(add(self.eye, mul(x, nx * hw)), mul(y, ny * hh)), mul(z, self.far * 0.5));
        Ray { o, d: mul(z, -1.0) }
    }
    /// Metres per pixel at a point's depth.
    pub fn pixel_size(&self, p: P3) -> f64 {
        let depth = if self.persp { jmax(dot(sub(p, self.eye), mul(self.axes().z, -1.0)), 1e-6) } else { self.distance() };
        (2.0 * depth * half_tan(self.fov)) / self.h
    }
    /// `pixelSize()` at the target.
    pub fn pixel_size0(&self) -> f64 {
        self.pixel_size(self.target)
    }
    pub fn orbit(&mut self, dx: f64, dy: f64) {
        let off = sub(self.eye, self.target);
        let r = len(off);
        let yaw = libm::atan2(off[1], off[0]) - dx * 0.008;
        let mut pitch = libm::acos(jmax(-1.0, jmin(1.0, off[2] / r))) - dy * 0.008;
        pitch = jmax(0.001, jmin(std::f64::consts::PI - 0.001, pitch));
        self.eye = add(self.target, [r * fd_sin(pitch) * fd_cos(yaw), r * fd_sin(pitch) * fd_sin(yaw), r * fd_cos(pitch)]);
    }
    pub fn pan(&mut self, dx: f64, dy: f64) {
        let Axes { x, y, .. } = self.axes();
        let s = self.pixel_size(self.target);
        let mv = add(mul(x, -dx * s), mul(y, dy * s));
        self.eye = add(self.eye, mv);
        self.target = add(self.target, mv);
    }
    /// factor < 1 zooms in, keeping `pivot` (or the point under the cursor on the target plane) still. `sx`/`sy`
    /// None = the middle of the view (the JS's default parameters, also for an explicit `undefined`).
    pub fn zoom_at(&mut self, factor: f64, sx: Option<f64>, sy: Option<f64>, pivot: Option<P3>) {
        let sx = sx.unwrap_or(self.w / 2.0);
        let sy = sy.unwrap_or(self.h / 2.0);
        let pp = match pivot {
            Some(p) => p,
            None => {
                let r = self.ray(sx, sy);
                let z = self.axes().z;
                match ray_plane(r.o, r.d, &Plane { n: z, d: dot(z, self.target) }) {
                    Some((_, p)) => p,
                    None => self.target,
                }
            }
        };
        let d0 = self.distance();
        let nd = d0 * factor;
        if nd < 1e-3 || nd > 1e6 {
            return;
        }
        if self.persp {
            self.eye = add(pp, mul(sub(self.eye, pp), factor));
            self.target = add(pp, mul(sub(self.target, pp), factor));
        } else {
            let z = self.axes().z;
            let tgt = add(pp, mul(sub(self.target, pp), factor));
            let along = dot(sub(tgt, self.target), z);
            let t2 = sub(tgt, mul(z, along));
            self.target = t2;
            self.eye = add(t2, mul(z, nd));
        }
    }
    /// Look at a bounding box from the current direction (or `dir`).
    pub fn fit(&mut self, bb: Option<&Box3>, dir: Option<P3>) {
        let d = norm(dir.unwrap_or_else(|| sub(self.eye, self.target)));
        let c = bb.map_or([0.0, 0.0, 0.0], |b| mul(add(b.min, b.max), 0.5));
        let r = bb.map_or(5.0, |b| jmax(dist(b.min, b.max) / 2.0, 0.01));
        let fov_min = jmin(self.fov, self.fov * self.aspect()) * std::f64::consts::PI / 360.0;
        let dd = (r / fd_sin(fov_min)) * 1.08;
        self.target = c;
        self.eye = add(c, mul(d, dd));
        self.clip(r);
    }
    pub fn standard(&mut self, name: &str, bb: Option<&Box3>) {
        let dir = view_dir(name).or_else(|| view_dir("iso"));
        self.fit(bb, dir);
    }
    /// `toJSON()`.
    pub fn to_json(&self) -> serde_json::Value {
        use crate::outj::{num, p3};
        serde_json::json!({ "eye": p3(&self.eye), "target": p3(&self.target), "fov": num(self.fov), "persp": self.persp, "height": num(self.height()) })
    }
    /// `set(j)`: eye and target (fov when truthy; persp unless false).
    pub fn set(&mut self, eye: P3, target: P3, fov: Option<f64>, persp: Option<bool>) {
        self.eye = eye;
        self.target = target;
        if let Some(f) = fov.filter(|f| *f != 0.0 && !f.is_nan()) {
            self.fov = f;
        }
        self.persp = persp != Some(false);
    }
}

/// A camera halfway between two saved cameras (scene transitions): (eye, target, fov, persp).
pub fn lerp_cam(a: &Camera, b: &Camera, t: f64) -> (P3, P3, f64, bool) {
    let s = t * t * (3.0 - 2.0 * t);
    let l = |p: P3, q: P3| add(p, mul(sub(q, p), s));
    (l(a.eye, b.eye), l(a.target, b.target), a.fov + (b.fov - a.fov) * s, if s < 0.5 { a.persp } else { b.persp })
}
