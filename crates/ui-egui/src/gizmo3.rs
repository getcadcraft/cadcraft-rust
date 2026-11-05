//! The move / turn gizmo of PartCraft 0.5's assemblies (asm-ui.js `startGizmo`): three.js's TransformControls
//! (three 0.186.1, MIT) on a stand-in object at the middle of the parts, in translate or rotate mode, world space,
//! size 0.9. This is that control's picking and drag maths on the CPU, in f64, with the operations in three's order
//! (Matrix4 / Quaternion / Vector3 / Ray / Raycaster / Mesh.raycast), so a drag with the same camera lands on the same
//! numbers as the 0.5 page; and its handles as line segments for the 3D view's line pass.
//!
//! Coordinates are the part's (three's world in PartCraft: Y up); the caller converts to the view's (Z up). A render
//! of the page is [`Tc::update`]: the handles and the drag plane follow the camera and the object as they stood at
//! the last frame, as three's do (TransformControls reads what the last `updateMatrixWorld` left).

use cc_part::js::{acos, atan2, cos, sin, tan};

pub type V3 = [f64; 3];
/// x, y, z, w
pub type Q4 = [f64; 4];
/// column-major, as three's `elements`
pub type M4 = [f64; 16];

const ID_Q: Q4 = [0.0, 0.0, 0.0, 1.0];
const ONE: V3 = [1.0, 1.0, 1.0];
const PI: f64 = std::f64::consts::PI;

// ---------------------------------------------------------------------------------------------- three's maths

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
/// `crossVectors(a, b)`
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn length_sq(a: V3) -> f64 {
    a[0] * a[0] + a[1] * a[1] + a[2] * a[2]
}
fn length(a: V3) -> f64 {
    length_sq(a).sqrt()
}
/// `normalize()`: `divideScalar(this.length() || 1)` (a NaN length is 1 too)
fn normalize(a: V3) -> V3 {
    let l = length(a);
    let l = if l == 0.0 || l.is_nan() { 1.0 } else { l };
    scale(a, 1.0 / l)
}
fn distance(a: V3, b: V3) -> f64 {
    let (dx, dy, dz) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    (dx * dx + dy * dy + dz * dz).sqrt()
}
fn angle_to(a: V3, b: V3) -> f64 {
    let den = (length_sq(a) * length_sq(b)).sqrt();
    if den == 0.0 {
        return PI / 2.0;
    }
    acos((dot(a, b) / den).clamp(-1.0, 1.0))
}
/// `applyMatrix4` (with the divide by w)
fn apply_m4(v: V3, e: &M4) -> V3 {
    let [x, y, z] = v;
    let w = 1.0 / (e[3] * x + e[7] * y + e[11] * z + e[15]);
    [
        (e[0] * x + e[4] * y + e[8] * z + e[12]) * w,
        (e[1] * x + e[5] * y + e[9] * z + e[13]) * w,
        (e[2] * x + e[6] * y + e[10] * z + e[14]) * w,
    ]
}
fn transform_dir(v: V3, e: &M4) -> V3 {
    let [x, y, z] = v;
    normalize([e[0] * x + e[4] * y + e[8] * z, e[1] * x + e[5] * y + e[9] * z, e[2] * x + e[6] * y + e[10] * z])
}
fn apply_q(v: V3, q: Q4) -> V3 {
    let [vx, vy, vz] = v;
    let [qx, qy, qz, qw] = q;
    let tx = 2.0 * (qy * vz - qz * vy);
    let ty = 2.0 * (qz * vx - qx * vz);
    let tz = 2.0 * (qx * vy - qy * vx);
    [vx + qw * tx + qy * tz - qz * ty, vy + qw * ty + qz * tx - qx * tz, vz + qw * tz + qx * ty - qy * tx]
}

fn q_axis_angle(axis: V3, angle: f64) -> Q4 {
    let half = angle / 2.0;
    let s = sin(half);
    [axis[0] * s, axis[1] * s, axis[2] * s, cos(half)]
}
fn q_from_rot(te: &M4) -> Q4 {
    let (m11, m12, m13) = (te[0], te[4], te[8]);
    let (m21, m22, m23) = (te[1], te[5], te[9]);
    let (m31, m32, m33) = (te[2], te[6], te[10]);
    let trace = m11 + m22 + m33;
    if trace > 0.0 {
        let s = 0.5 / (trace + 1.0).sqrt();
        [(m32 - m23) * s, (m13 - m31) * s, (m21 - m12) * s, 0.25 / s]
    } else if m11 > m22 && m11 > m33 {
        let s = 2.0 * (1.0 + m11 - m22 - m33).sqrt();
        [0.25 * s, (m12 + m21) / s, (m13 + m31) / s, (m32 - m23) / s]
    } else if m22 > m33 {
        let s = 2.0 * (1.0 + m22 - m11 - m33).sqrt();
        [(m12 + m21) / s, 0.25 * s, (m23 + m32) / s, (m13 - m31) / s]
    } else {
        let s = 2.0 * (1.0 + m33 - m11 - m22).sqrt();
        [(m13 + m31) / s, (m23 + m32) / s, 0.25 * s, (m21 - m12) / s]
    }
}
/// `setFromEuler` with the order XYZ
fn q_euler(x: f64, y: f64, z: f64) -> Q4 {
    let (c1, c2, c3) = (cos(x / 2.0), cos(y / 2.0), cos(z / 2.0));
    let (s1, s2, s3) = (sin(x / 2.0), sin(y / 2.0), sin(z / 2.0));
    [s1 * c2 * c3 + c1 * s2 * s3, c1 * s2 * c3 - s1 * c2 * s3, c1 * c2 * s3 + s1 * s2 * c3, c1 * c2 * c3 - s1 * s2 * s3]
}
/// `multiplyQuaternions(a, b)`
fn q_mul(a: Q4, b: Q4) -> Q4 {
    let [qax, qay, qaz, qaw] = a;
    let [qbx, qby, qbz, qbw] = b;
    [
        qax * qbw + qaw * qbx + qay * qbz - qaz * qby,
        qay * qbw + qaw * qby + qaz * qbx - qax * qbz,
        qaz * qbw + qaw * qbz + qax * qby - qay * qbx,
        qaw * qbw - qax * qbx - qay * qby - qaz * qbz,
    ]
}
fn q_normalize(q: Q4) -> Q4 {
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if l == 0.0 {
        return ID_Q;
    }
    let l = 1.0 / l;
    [q[0] * l, q[1] * l, q[2] * l, q[3] * l]
}
/// `invert()` (the conjugate: `*= -1`)
fn q_conj(q: Q4) -> Q4 {
    // `*= -1` is the negation (−0 for 0)
    [-q[0], -q[1], -q[2], q[3]]
}

fn compose(p: V3, q: Q4, s: V3) -> M4 {
    let [x, y, z, w] = q;
    let (x2, y2, z2) = (x + x, y + y, z + z);
    let (xx, xy, xz) = (x * x2, x * y2, x * z2);
    let (yy, yz, zz) = (y * y2, y * z2, z * z2);
    let (wx, wy, wz) = (w * x2, w * y2, w * z2);
    let [sx, sy, sz] = s;
    [
        (1.0 - (yy + zz)) * sx,
        (xy + wz) * sx,
        (xz - wy) * sx,
        0.0,
        (xy - wz) * sy,
        (1.0 - (xx + zz)) * sy,
        (yz + wx) * sy,
        0.0,
        (xz + wy) * sz,
        (yz - wx) * sz,
        (1.0 - (xx + yy)) * sz,
        0.0,
        p[0],
        p[1],
        p[2],
        1.0,
    ]
}
fn det_affine(te: &M4) -> f64 {
    let (n11, n12, n13) = (te[0], te[4], te[8]);
    let (n21, n22, n23) = (te[1], te[5], te[9]);
    let (n31, n32, n33) = (te[2], te[6], te[10]);
    n11 * (n22 * n33 - n23 * n32) - n12 * (n21 * n33 - n23 * n31) + n13 * (n21 * n32 - n22 * n31)
}
fn decompose(te: &M4) -> (V3, Q4, V3) {
    let p = [te[12], te[13], te[14]];
    let det = det_affine(te);
    if det == 0.0 {
        return (p, ID_Q, ONE);
    }
    let mut sx = length([te[0], te[1], te[2]]);
    let sy = length([te[4], te[5], te[6]]);
    let sz = length([te[8], te[9], te[10]]);
    if det < 0.0 {
        sx = -sx;
    }
    let mut m = *te;
    let (ix, iy, iz) = (1.0 / sx, 1.0 / sy, 1.0 / sz);
    m[0] *= ix;
    m[1] *= ix;
    m[2] *= ix;
    m[4] *= iy;
    m[5] *= iy;
    m[6] *= iy;
    m[8] *= iz;
    m[9] *= iz;
    m[10] *= iz;
    (p, q_from_rot(&m), [sx, sy, sz])
}
/// `Matrix4.lookAt(eye, target, up)`: the rotation part (the rest as the identity's; three leaves it untouched and
/// nothing here reads it)
fn look_at(eye: V3, target: V3, up: V3) -> M4 {
    let mut z = sub(eye, target);
    if length_sq(z) == 0.0 {
        z[2] = 1.0;
    }
    z = normalize(z);
    let mut x = cross(up, z);
    if length_sq(x) == 0.0 {
        if up[2].abs() == 1.0 {
            z[0] += 0.0001;
        } else {
            z[2] += 0.0001;
        }
        z = normalize(z);
        x = cross(up, z);
    }
    let x = normalize(x);
    let y = cross(z, x);
    [x[0], x[1], x[2], 0.0, y[0], y[1], y[2], 0.0, z[0], z[1], z[2], 0.0, 0.0, 0.0, 0.0, 1.0]
}
fn invert(te: &M4) -> M4 {
    let (n11, n21, n31, n41) = (te[0], te[1], te[2], te[3]);
    let (n12, n22, n32, n42) = (te[4], te[5], te[6], te[7]);
    let (n13, n23, n33, n43) = (te[8], te[9], te[10], te[11]);
    let (n14, n24, n34, n44) = (te[12], te[13], te[14], te[15]);
    let t1 = n11 * n22 - n21 * n12;
    let t2 = n11 * n32 - n31 * n12;
    let t3 = n11 * n42 - n41 * n12;
    let t4 = n21 * n32 - n31 * n22;
    let t5 = n21 * n42 - n41 * n22;
    let t6 = n31 * n42 - n41 * n32;
    let t7 = n13 * n24 - n23 * n14;
    let t8 = n13 * n34 - n33 * n14;
    let t9 = n13 * n44 - n43 * n14;
    let t10 = n23 * n34 - n33 * n24;
    let t11 = n23 * n44 - n43 * n24;
    let t12 = n33 * n44 - n43 * n34;
    let det = t1 * t12 - t2 * t11 + t3 * t10 + t4 * t9 - t5 * t8 + t6 * t7;
    if det == 0.0 {
        return [0.0; 16];
    }
    let d = 1.0 / det;
    [
        (n22 * t12 - n32 * t11 + n42 * t10) * d,
        (n31 * t11 - n21 * t12 - n41 * t10) * d,
        (n24 * t6 - n34 * t5 + n44 * t4) * d,
        (n33 * t5 - n23 * t6 - n43 * t4) * d,
        (n32 * t9 - n12 * t12 - n42 * t8) * d,
        (n11 * t12 - n31 * t9 + n41 * t8) * d,
        (n34 * t3 - n14 * t6 - n44 * t2) * d,
        (n13 * t6 - n33 * t3 + n43 * t2) * d,
        (n12 * t11 - n22 * t9 + n42 * t7) * d,
        (n21 * t9 - n11 * t11 - n41 * t7) * d,
        (n14 * t5 - n24 * t3 + n44 * t1) * d,
        (n23 * t3 - n13 * t5 - n43 * t1) * d,
        (n22 * t8 - n12 * t10 - n32 * t7) * d,
        (n11 * t10 - n21 * t8 + n31 * t7) * d,
        (n24 * t2 - n14 * t4 - n34 * t1) * d,
        (n13 * t4 - n23 * t2 + n33 * t1) * d,
    ]
}
/// `multiplyMatrices(a, b)`
pub fn mul(a: &M4, b: &M4) -> M4 {
    [
        a[0] * b[0] + a[4] * b[1] + a[8] * b[2] + a[12] * b[3],
        a[1] * b[0] + a[5] * b[1] + a[9] * b[2] + a[13] * b[3],
        a[2] * b[0] + a[6] * b[1] + a[10] * b[2] + a[14] * b[3],
        a[3] * b[0] + a[7] * b[1] + a[11] * b[2] + a[15] * b[3],
        a[0] * b[4] + a[4] * b[5] + a[8] * b[6] + a[12] * b[7],
        a[1] * b[4] + a[5] * b[5] + a[9] * b[6] + a[13] * b[7],
        a[2] * b[4] + a[6] * b[5] + a[10] * b[6] + a[14] * b[7],
        a[3] * b[4] + a[7] * b[5] + a[11] * b[6] + a[15] * b[7],
        a[0] * b[8] + a[4] * b[9] + a[8] * b[10] + a[12] * b[11],
        a[1] * b[8] + a[5] * b[9] + a[9] * b[10] + a[13] * b[11],
        a[2] * b[8] + a[6] * b[9] + a[10] * b[10] + a[14] * b[11],
        a[3] * b[8] + a[7] * b[9] + a[11] * b[10] + a[15] * b[11],
        a[0] * b[12] + a[4] * b[13] + a[8] * b[14] + a[12] * b[15],
        a[1] * b[12] + a[5] * b[13] + a[9] * b[14] + a[13] * b[15],
        a[2] * b[12] + a[6] * b[13] + a[10] * b[14] + a[14] * b[15],
        a[3] * b[12] + a[7] * b[13] + a[11] * b[14] + a[15] * b[15],
    ]
}
/// The scene's (and every plain group's) world matrix: `compose(0, identity, 1)`.
fn group() -> M4 {
    compose([0.0; 3], ID_Q, ONE)
}

// ---------------------------------------------------------------------------------------------- the camera and rays

/// A three.js camera as the page holds it (no parent but the scene, zoom 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub persp: bool,
    pub fov: f64,
    /// orthographic: the frustum's top and bottom
    pub top: f64,
    pub bottom: f64,
    pub world: M4,
    pub proj: M4,
    pub proj_inv: M4,
}

impl Camera {
    /// `position.copy(eye); up.copy(up); lookAt(target)`, then the scene's `updateMatrixWorld`; `PerspectiveCamera(fov,
    /// aspect, near, far)`.
    pub fn perspective(eye: V3, target: V3, up: V3, fov: f64, aspect: f64, near: f64, far: f64) -> Camera {
        let world = Self::placed(eye, target, up);
        let top = near * tan(PI / 180.0 * 0.5 * fov) / 1.0;
        let height = 2.0 * top;
        let width = aspect * height;
        let left = -0.5 * width;
        let (l, r, t, b) = (left, left + width, top, top - height);
        let x = 2.0 * near / (r - l);
        let y = 2.0 * near / (t - b);
        let a = (r + l) / (r - l);
        let bb = (t + b) / (t - b);
        let c = -(far + near) / (far - near);
        let d = (-2.0 * far * near) / (far - near);
        let proj = [x, 0.0, 0.0, 0.0, 0.0, y, 0.0, 0.0, a, bb, c, -1.0, 0.0, 0.0, d, 0.0];
        Camera { persp: true, fov, top: 0.0, bottom: 0.0, world, proj, proj_inv: invert(&proj) }
    }

    /// `OrthographicCamera(left, right, top, bottom, near, far)` placed as [`Camera::perspective`].
    pub fn orthographic(eye: V3, target: V3, up: V3, lrtb: [f64; 4], near: f64, far: f64) -> Camera {
        let world = Self::placed(eye, target, up);
        let [l0, r0, t0, b0] = lrtb;
        let (dx, dy) = ((r0 - l0) / (2.0 * 1.0), (t0 - b0) / (2.0 * 1.0));
        let (cx, cy) = ((r0 + l0) / 2.0, (t0 + b0) / 2.0);
        let (l, r, t, b) = (cx - dx, cx + dx, cy + dy, cy - dy);
        let x = 2.0 / (r - l);
        let y = 2.0 / (t - b);
        let a = -(r + l) / (r - l);
        let bb = -(t + b) / (t - b);
        let c = -2.0 / (far - near);
        let d = -(far + near) / (far - near);
        let proj = [x, 0.0, 0.0, 0.0, 0.0, y, 0.0, 0.0, 0.0, 0.0, c, 0.0, a, bb, d, 1.0];
        Camera { persp: false, fov: 0.0, top: t0, bottom: b0, world, proj, proj_inv: invert(&proj) }
    }

    fn placed(eye: V3, target: V3, up: V3) -> M4 {
        let q = q_from_rot(&look_at(eye, target, up));
        mul(&group(), &compose(eye, q, ONE))
    }

    fn position(&self) -> V3 {
        [self.world[12], self.world[13], self.world[14]]
    }

    /// Where a point is seen in a `w` × `h` view (pixels; the scripted gestures start there).
    pub fn pixel(&self, p: V3, w: f64, h: f64) -> (f64, f64) {
        let v = apply_m4(apply_m4(p, &invert(&self.world)), &self.proj);
        ((v[0] + 1.0) / 2.0 * w, (1.0 - v[1]) / 2.0 * h)
    }

    /// `Raycaster.setFromCamera` for a pointer in normalised device coordinates.
    fn ray(&self, x: f64, y: f64) -> Ray {
        if self.persp {
            let o = self.position();
            let p = apply_m4(apply_m4([x, y, 0.5], &self.proj_inv), &self.world);
            Ray { o, d: normalize(sub(p, o)) }
        } else {
            let o = apply_m4(apply_m4([x, y, self.proj[14]], &self.proj_inv), &self.world);
            Ray { o, d: transform_dir([0.0, 0.0, -1.0], &self.world) }
        }
    }
}

/// The pointer as TransformControls' `getPointer` gives it: a pixel of a `w` × `h` view → (−1…1, −1…1), y up.
pub fn ndc(px: f64, py: f64, w: f64, h: f64) -> (f64, f64) {
    (px / w * 2.0 - 1.0, -py / h * 2.0 + 1.0)
}

#[derive(Clone, Copy, Debug)]
struct Ray {
    o: V3,
    d: V3,
}

impl Ray {
    fn at(&self, t: f64) -> V3 {
        [self.o[0] + self.d[0] * t, self.o[1] + self.d[1] * t, self.o[2] + self.d[2] * t]
    }
    fn apply(&self, m: &M4) -> Ray {
        Ray { o: apply_m4(self.o, m), d: transform_dir(self.d, m) }
    }
    /// `Ray.intersectTriangle(a, b, c, backfaceCulling)`
    fn triangle(&self, a: V3, b: V3, c: V3, cull: bool) -> Option<V3> {
        let (o, d) = (self.o, self.d);
        let (dx, dy, dz) = (d[0], d[1], d[2]);
        let (aox, aoy, aoz) = (a[0] - o[0], a[1] - o[1], a[2] - o[2]);
        let (box_, boy, boz) = (b[0] - o[0], b[1] - o[1], b[2] - o[2]);
        let (cox, coy, coz) = (c[0] - o[0], c[1] - o[1], c[2] - o[2]);
        let (adx, ady, adz) = (dx.abs(), dy.abs(), dz.abs());
        let (dkx, dky, dkz, akx, aky, akz, bkx, bky, bkz, ckx, cky, ckz);
        if adx >= ady && adx >= adz {
            (dkz, akz, bkz, ckz) = (dx, aox, box_, cox);
            if dx >= 0.0 {
                (dkx, dky, akx, aky, bkx, bky, ckx, cky) = (dy, dz, aoy, aoz, boy, boz, coy, coz);
            } else {
                (dkx, dky, akx, aky, bkx, bky, ckx, cky) = (dz, dy, aoz, aoy, boz, boy, coz, coy);
            }
        } else if ady >= adz {
            (dkz, akz, bkz, ckz) = (dy, aoy, boy, coy);
            if dy >= 0.0 {
                (dkx, dky, akx, aky, bkx, bky, ckx, cky) = (dz, dx, aoz, aox, boz, box_, coz, cox);
            } else {
                (dkx, dky, akx, aky, bkx, bky, ckx, cky) = (dx, dz, aox, aoz, box_, boz, cox, coz);
            }
        } else {
            (dkz, akz, bkz, ckz) = (dz, aoz, boz, coz);
            if dz >= 0.0 {
                (dkx, dky, akx, aky, bkx, bky, ckx, cky) = (dx, dy, aox, aoy, box_, boy, cox, coy);
            } else {
                (dkx, dky, akx, aky, bkx, bky, ckx, cky) = (dy, dx, aoy, aox, boy, box_, coy, cox);
            }
        }
        if dkz == 0.0 {
            return None;
        }
        let (sx, sy, sz) = (dkx / dkz, dky / dkz, 1.0 / dkz);
        let (ax, ay) = (akx - sx * akz, aky - sy * akz);
        let (bx, by) = (bkx - sx * bkz, bky - sy * bkz);
        let (cx, cy) = (ckx - sx * ckz, cky - sy * ckz);
        let u = cx * by - cy * bx;
        let v = ax * cy - ay * cx;
        let w = bx * ay - by * ax;
        if cull {
            if u < 0.0 || v < 0.0 || w < 0.0 {
                return None;
            }
        } else if (u < 0.0 || v < 0.0 || w < 0.0) && (u > 0.0 || v > 0.0 || w > 0.0) {
            return None;
        }
        let det = u + v + w;
        if det == 0.0 {
            return None;
        }
        let ts = sz * (u * akz + v * bkz + w * ckz);
        if if det > 0.0 { ts < 0.0 } else { ts > 0.0 } {
            return None;
        }
        Some(self.at(ts / det))
    }
}

/// A mesh's triangles (three's position attribute, f32) and how it is raycast.
#[derive(Clone, Debug, Default)]
struct Geo {
    pos: Vec<[f32; 3]>,
    /// None: every three positions a triangle
    index: Option<Vec<u32>>,
}

impl Geo {
    fn tri(&self, k: usize) -> Option<[V3; 3]> {
        let at = |i: usize| self.pos.get(i).map(|p| [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]);
        let (a, b, c) = match &self.index {
            Some(ix) => (*ix.get(k * 3)? as usize, *ix.get(k * 3 + 1)? as usize, *ix.get(k * 3 + 2)? as usize),
            None => (k * 3, k * 3 + 1, k * 3 + 2),
        };
        Some([at(a)?, at(b)?, at(c)?])
    }
    fn tris(&self) -> usize {
        self.index.as_ref().map_or(self.pos.len() / 3, |ix| ix.len() / 3)
    }
    /// `Mesh.raycast` → (distance, world point) of each triangle hit, in the triangles' order. `cull`: FrontSide.
    fn raycast(&self, world: &M4, ray: &Ray, cull: bool, out: &mut Vec<(f64, V3)>) {
        let local = ray.apply(&invert(world));
        for k in 0..self.tris() {
            let Some([a, b, c]) = self.tri(k) else { continue };
            if let Some(p) = local.triangle(a, b, c, cull) {
                let w = apply_m4(p, world);
                out.push((distance(ray.o, w), w));
            }
        }
    }
    /// `geometry.applyMatrix4(m)` on the f32 positions.
    fn applied(mut self, m: &M4) -> Geo {
        for p in &mut self.pos {
            let q = apply_m4([f64::from(p[0]), f64::from(p[1]), f64::from(p[2])], m);
            *p = [q[0] as f32, q[1] as f32, q[2] as f32];
        }
        self
    }
}

fn f32s(v: &[f64]) -> Vec<[f32; 3]> {
    v.as_chunks::<3>().0.iter().map(|c| c.map(|x| x as f32)).collect()
}

/// `CylinderGeometry(radiusTop, radiusBottom, height, radialSegments)` (one height segment, closed).
fn cylinder(rt: f64, rb: f64, h: f64, radial: u32) -> Geo {
    let mut v: Vec<f64> = Vec::new();
    let mut ix: Vec<u32> = Vec::new();
    let half = h / 2.0;
    let len = PI * 2.0;
    let mut rows: Vec<Vec<u32>> = Vec::new();
    let mut index = 0u32;
    for y in 0..=1u32 {
        let vv = f64::from(y);
        let radius = vv * (rb - rt) + rt;
        let mut row = Vec::new();
        for x in 0..=radial {
            let u = f64::from(x) / f64::from(radial);
            let th = u * len + 0.0;
            v.extend_from_slice(&[radius * sin(th), -vv * h + half, radius * cos(th)]);
            row.push(index);
            index += 1;
        }
        rows.push(row);
    }
    let get = |y: usize, x: usize| rows.get(y).and_then(|r| r.get(x)).copied().unwrap_or(0);
    for x in 0..radial as usize {
        let (a, b, c, d) = (get(0, x), get(1, x), get(1, x + 1), get(0, x + 1));
        if rt > 0.0 {
            ix.extend_from_slice(&[a, b, d]);
        }
        if rb > 0.0 {
            ix.extend_from_slice(&[b, c, d]);
        }
    }
    for top in [true, false] {
        let r = if top { rt } else { rb };
        if r <= 0.0 {
            continue;
        }
        let sign = if top { 1.0 } else { -1.0 };
        let c0 = index;
        for _ in 1..=radial {
            v.extend_from_slice(&[0.0, half * sign, 0.0]);
            index += 1;
        }
        let i0 = index;
        for x in 0..=radial {
            let u = f64::from(x) / f64::from(radial);
            let th = u * len + 0.0;
            v.extend_from_slice(&[r * sin(th), half * sign, r * cos(th)]);
            index += 1;
        }
        for x in 0..radial {
            let (c, i) = (c0 + x, i0 + x);
            if top {
                ix.extend_from_slice(&[i, i + 1, c]);
            } else {
                ix.extend_from_slice(&[i + 1, i, c]);
            }
        }
    }
    Geo { pos: f32s(&v), index: Some(ix) }
}

/// `BoxGeometry(width, height, depth)` (one segment a side).
fn cuboid(w: f64, h: f64, d: f64) -> Geo {
    let mut v: Vec<f64> = Vec::new();
    let mut ix: Vec<u32> = Vec::new();
    let mut n = 0u32;
    // (u, v, w axes, udir, vdir, width, height, depth)
    type Side = (usize, usize, usize, f64, f64, f64, f64, f64);
    let planes: [Side; 6] = [
        (2, 1, 0, -1.0, -1.0, d, h, w),
        (2, 1, 0, 1.0, -1.0, d, h, -w),
        (0, 2, 1, 1.0, 1.0, w, d, h),
        (0, 2, 1, 1.0, -1.0, w, d, -h),
        (0, 1, 2, 1.0, -1.0, w, h, d),
        (0, 1, 2, -1.0, -1.0, w, h, -d),
    ];
    for (u, vv, ww, udir, vdir, width, height, depth) in planes {
        for iy in 0..2 {
            let y = f64::from(iy) * height - height / 2.0;
            for ixx in 0..2 {
                let x = f64::from(ixx) * width - width / 2.0;
                let mut p = [0.0; 3];
                if let Some(e) = p.get_mut(u) {
                    *e = x * udir;
                }
                if let Some(e) = p.get_mut(vv) {
                    *e = y * vdir;
                }
                if let Some(e) = p.get_mut(ww) {
                    *e = depth / 2.0;
                }
                v.extend_from_slice(&p);
            }
        }
        let (a, b, c, dd) = (n, n + 2, n + 3, n + 1);
        ix.extend_from_slice(&[a, b, dd, b, c, dd]);
        n += 4;
    }
    Geo { pos: f32s(&v), index: Some(ix) }
}

/// `OctahedronGeometry(radius, 0)`.
fn octahedron(r: f64) -> Geo {
    let vs: [V3; 6] = [[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]];
    let faces: [[usize; 3]; 8] = [[0, 2, 4], [0, 4, 3], [0, 3, 5], [0, 5, 2], [1, 2, 5], [1, 5, 3], [1, 3, 4], [1, 4, 2]];
    let mut v = Vec::new();
    for [a, b, c] in faces {
        let (a, b, c) =
            (vs.get(a).copied().unwrap_or_default(), vs.get(b).copied().unwrap_or_default(), vs.get(c).copied().unwrap_or_default());
        // subdivideFace with detail 0 pushes b, c, a
        for p in [b, c, a] {
            v.extend_from_slice(&scale(normalize(p), r));
        }
    }
    Geo { pos: f32s(&v), index: None }
}

/// `SphereGeometry(radius, widthSegments, heightSegments)`.
fn sphere(r: f64, ws: u32, hs: u32) -> Geo {
    let mut v = Vec::new();
    let mut grid: Vec<Vec<u32>> = Vec::new();
    let mut index = 0u32;
    for iy in 0..=hs {
        let vv = f64::from(iy) / f64::from(hs);
        let th = 0.0 + vv * PI;
        let y = r * cos(th);
        let ring = (r * r - y * y).sqrt();
        let mut row = Vec::new();
        for ix in 0..=ws {
            let u = f64::from(ix) / f64::from(ws);
            let phi = 0.0 + u * (PI * 2.0);
            v.extend_from_slice(&[-ring * cos(phi), y, ring * sin(phi)]);
            row.push(index);
            index += 1;
        }
        grid.push(row);
    }
    let g = |y: usize, x: usize| grid.get(y).and_then(|r| r.get(x)).copied().unwrap_or(0);
    let mut ix = Vec::new();
    for iy in 0..hs as usize {
        for x in 0..ws as usize {
            let (a, b, c, d) = (g(iy, x + 1), g(iy, x), g(iy + 1, x), g(iy + 1, x + 1));
            if iy != 0 {
                ix.extend_from_slice(&[a, b, d]);
            }
            if iy != hs as usize - 1 {
                ix.extend_from_slice(&[b, c, d]);
            }
        }
    }
    Geo { pos: f32s(&v), index: Some(ix) }
}

/// `TorusGeometry(radius, tube, radialSegments, tubularSegments)` (a whole ring).
fn torus(r: f64, tube: f64, radial: u32, tubular: u32) -> Geo {
    let mut v = Vec::new();
    let arc = PI * 2.0;
    for j in 0..=radial {
        let vv = 0.0 + (f64::from(j) / f64::from(radial)) * (PI * 2.0);
        for i in 0..=tubular {
            let u = f64::from(i) / f64::from(tubular) * arc;
            v.extend_from_slice(&[(r + tube * cos(vv)) * cos(u), (r + tube * cos(vv)) * sin(u), tube * sin(vv)]);
        }
    }
    let mut ix = Vec::new();
    for j in 1..=radial {
        for i in 1..=tubular {
            let a = (tubular + 1) * j + i - 1;
            let b = (tubular + 1) * (j - 1) + i - 1;
            let c = (tubular + 1) * (j - 1) + i;
            let d = (tubular + 1) * j + i;
            ix.extend_from_slice(&[a, b, d, b, c, d]);
        }
    }
    Geo { pos: f32s(&v), index: Some(ix) }
}

/// A gizmo definition's entry as `setupGizmo` bakes it: the geometry moved by (position, rotation).
fn baked(g: Geo, pos: V3, rot: V3) -> Geo {
    let m = compose(pos, q_euler(rot[0], rot[1], rot[2]), ONE);
    g.applied(&m)
}

// ---------------------------------------------------------------------------------------------- the control

/// translate or rotate (asm-ui.js never scales).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Translate,
    Rotate,
}

/// One picker handle: its name and baked geometry.
#[derive(Clone, Debug)]
struct Picker {
    name: &'static str,
    geo: Geo,
}

/// The pickers of a mode in the order `setupGizmo` adds them (a name's entries last first).
fn pickers(mode: Mode) -> Vec<Picker> {
    let h = PI / 2.0;
    let cyl = || cylinder(0.2, 0.0, 0.6, 4);
    let defs: Vec<(&'static str, Vec<Geo>)> = match mode {
        Mode::Translate => vec![
            ("X", vec![baked(cyl(), [0.3, 0.0, 0.0], [0.0, 0.0, -h]), baked(cyl(), [-0.3, 0.0, 0.0], [0.0, 0.0, h])]),
            ("Y", vec![baked(cyl(), [0.0, 0.3, 0.0], [0.0; 3]), baked(cyl(), [0.0, -0.3, 0.0], [0.0, 0.0, PI])]),
            ("Z", vec![baked(cyl(), [0.0, 0.0, 0.3], [h, 0.0, 0.0]), baked(cyl(), [0.0, 0.0, -0.3], [-h, 0.0, 0.0])]),
            ("XYZ", vec![baked(octahedron(0.2), [0.0; 3], [0.0; 3])]),
            ("XY", vec![baked(cuboid(0.2, 0.2, 0.01), [0.15, 0.15, 0.0], [0.0; 3])]),
            ("YZ", vec![baked(cuboid(0.2, 0.2, 0.01), [0.0, 0.15, 0.15], [0.0, h, 0.0])]),
            ("XZ", vec![baked(cuboid(0.2, 0.2, 0.01), [0.15, 0.0, 0.15], [-h, 0.0, 0.0])]),
        ],
        Mode::Rotate => vec![
            ("XYZE", vec![baked(sphere(0.25, 10, 8), [0.0; 3], [0.0; 3])]),
            ("X", vec![baked(torus(0.5, 0.1, 4, 24), [0.0; 3], [0.0, -h, -h])]),
            ("Y", vec![baked(torus(0.5, 0.1, 4, 24), [0.0; 3], [h, 0.0, 0.0])]),
            ("Z", vec![baked(torus(0.5, 0.1, 4, 24), [0.0; 3], [0.0, 0.0, -h])]),
            ("E", vec![baked(torus(0.75, 0.1, 2, 24), [0.0; 3], [0.0; 3])]),
        ],
    };
    let mut out = Vec::new();
    for (name, geos) in defs {
        for geo in geos.into_iter().rev() {
            out.push(Picker { name, geo });
        }
    }
    out
}

/// PlaneGeometry(100000, 100000, 2, 2): the plane the pointer is followed on.
fn drag_plane() -> Geo {
    let mut v = Vec::new();
    for iy in 0..3 {
        let y = f64::from(iy) * 50000.0 - 50000.0;
        for ix in 0..3 {
            let x = f64::from(ix) * 50000.0 - 50000.0;
            v.extend_from_slice(&[x, -y, 0.0]);
        }
    }
    let mut ix = Vec::new();
    for iy in 0..2u32 {
        for x in 0..2u32 {
            let (a, b, c, d) = (x + 3 * iy, x + 3 * (iy + 1), (x + 1) + 3 * (iy + 1), (x + 1) + 3 * iy);
            ix.extend_from_slice(&[a, b, d, b, c, d]);
        }
    }
    Geo { pos: f32s(&v), index: Some(ix) }
}

const UX: V3 = [1.0, 0.0, 0.0];
const UY: V3 = [0.0, 1.0, 0.0];
const UZ: V3 = [0.0, 0.0, 1.0];

fn unit(axis: &str) -> V3 {
    match axis {
        "X" => UX,
        "Y" => UY,
        _ => UZ,
    }
}

/// TransformControls attached to a stand-in object (no parent but the scene), space "world", size 0.9.
#[derive(Clone, Debug)]
pub struct Tc {
    pub mode: Mode,
    pub size: f64,
    pub axis: Option<&'static str>,
    pub dragging: bool,
    /// the object
    pub position: V3,
    pub quaternion: Q4,
    cam: Option<Camera>,
    // what the last update left
    world_position: V3,
    eye: V3,
    camera_position: V3,
    camera_quaternion: Q4,
    parent_q_inv: Q4,
    root: M4,
    handles: Vec<(usize, M4, bool)>,
    plane: M4,
    // the drag
    world_position_start: V3,
    position_start: V3,
    quaternion_start: Q4,
    point_start: V3,
    point_end: V3,
    rotation_axis: V3,
    rotation_angle: f64,
    picker_list: Vec<Picker>,
    plane_geo: Geo,
}

impl Tc {
    pub fn new(mode: Mode, size: f64) -> Tc {
        Tc {
            mode,
            size,
            axis: None,
            dragging: false,
            position: [0.0; 3],
            quaternion: ID_Q,
            cam: None,
            world_position: [0.0; 3],
            eye: [0.0; 3],
            camera_position: [0.0; 3],
            camera_quaternion: ID_Q,
            parent_q_inv: ID_Q,
            root: group(),
            handles: Vec::new(),
            plane: group(),
            world_position_start: [0.0; 3],
            position_start: [0.0; 3],
            quaternion_start: ID_Q,
            point_start: [0.0; 3],
            point_end: [0.0; 3],
            rotation_axis: [0.0; 3],
            rotation_angle: 0.0,
            picker_list: pickers(mode),
            plane_geo: drag_plane(),
        }
    }

    /// The object's world matrix (`updateMatrixWorld(true)` under the scene).
    pub fn matrix_world(&self) -> M4 {
        mul(&group(), &compose(self.position, self.quaternion, ONE))
    }

    /// Where the handles are drawn (the object, or where the drag began).
    pub fn centre(&self) -> V3 {
        self.world_position
    }

    /// A point `u` handle lengths from the middle (along the part's axes), as the last update placed the handles.
    pub fn handle_point(&self, u: V3) -> V3 {
        let s = self.cam.map_or(1.0, |c| self.handle_scale(&c));
        add(self.world_position, scale(u, s))
    }

    /// The handle scale (`factor * size / 4`).
    fn handle_scale(&self, cam: &Camera) -> f64 {
        let factor = if cam.persp {
            distance(self.world_position, self.camera_position) * (1.9 * tan(PI * cam.fov / 360.0) / 1.0).min(7.0)
        } else {
            (cam.top - cam.bottom) / 1.0
        };
        factor * self.size / 4.0
    }

    /// A render: `TransformControlsRoot.updateMatrixWorld` (the object, the camera, the eye), then the gizmo's
    /// pickers and the plane.
    pub fn update(&mut self, cam: &Camera) {
        self.cam = Some(*cam);
        let (wp, _, _) = decompose(&self.matrix_world());
        self.world_position = wp;
        let (_, pq, _) = decompose(&group());
        self.parent_q_inv = q_conj(pq);
        let (cp, cq, _) = decompose(&cam.world);
        self.camera_position = cp;
        self.camera_quaternion = cq;
        self.eye = if cam.persp { normalize(sub(cp, wp)) } else { normalize([cam.world[8], cam.world[9], cam.world[10]]) };
        // the root cancels its parent's transform (the overlay group: none)
        let (rp, rq, rs) = decompose(&invert(&group()));
        self.root = mul(&group(), &compose(rp, rq, rs));
        let gw = mul(&mul(&self.root, &group()), &group());
        let s = self.handle_scale(cam);
        let eye = self.eye;
        let align = apply_q(eye, q_conj(ID_Q));
        let mut handles = Vec::with_capacity(self.picker_list.len());
        for (k, p) in self.picker_list.iter().enumerate() {
            let (q, visible) = handle_quat(self.mode, p.name, eye, align);
            let sc = if visible { s } else { 1e-10 };
            handles.push((k, mul(&gw, &compose(self.world_position, q, [sc, sc, sc])), visible));
        }
        self.handles = handles;
        // TransformControlsPlane
        let (v1, v2, v3) = (apply_q(UX, ID_Q), apply_q(UY, ID_Q), apply_q(UZ, ID_Q));
        let mut align = v2;
        let mut dir = [0.0; 3];
        if self.mode == Mode::Translate {
            match self.axis {
                Some("X") => {
                    align = cross(eye, v1);
                    dir = cross(v1, align);
                }
                Some("Y") => {
                    align = cross(eye, v2);
                    dir = cross(v2, align);
                }
                Some("Z") => {
                    align = cross(eye, v3);
                    dir = cross(v3, align);
                }
                Some("XY") => dir = v3,
                Some("YZ") => dir = v1,
                Some("XZ") => {
                    align = v3;
                    dir = v2;
                }
                _ => {}
            }
        }
        let pq = if length(dir) == 0.0 { self.camera_quaternion } else { q_from_rot(&look_at([0.0; 3], dir, align)) };
        self.plane = mul(&self.root, &compose(self.world_position, pq, ONE));
    }

    fn cam_ray(&self, x: f64, y: f64) -> Option<Ray> {
        self.cam.map(|c| c.ray(x, y))
    }

    /// `pointerHover`: the handle under the pointer (visible pickers only), or none.
    pub fn hover(&mut self, x: f64, y: f64) {
        if self.dragging {
            return;
        }
        let Some(ray) = self.cam_ray(x, y) else { return };
        let mut hits: Vec<(f64, usize, bool)> = Vec::new();
        for (k, m, vis) in &self.handles {
            let Some(p) = self.picker_list.get(*k) else { continue };
            let mut out = Vec::new();
            p.geo.raycast(m, &ray, true, &mut out);
            hits.extend(out.into_iter().map(|(d, _)| (d, *k, *vis)));
        }
        // Array.sort is stable: equal distances keep the handles' order
        hits.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        self.axis = hits.iter().find(|h| h.2).and_then(|h| self.picker_list.get(h.1)).map(|p| p.name);
    }

    fn plane_hit(&self, ray: &Ray) -> Option<V3> {
        let mut out = Vec::new();
        self.plane_geo.raycast(&self.plane, ray, false, &mut out);
        out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        out.first().map(|h| h.1)
    }

    /// `onPointerDown`: hover, then `pointerDown` (the left button) → whether a drag began.
    pub fn down(&mut self, x: f64, y: f64) -> bool {
        self.hover(x, y);
        if self.dragging || self.axis.is_none() {
            return false;
        }
        if let Some(ray) = self.cam_ray(x, y)
            && let Some(p) = self.plane_hit(&ray)
        {
            self.position_start = self.position;
            self.quaternion_start = self.quaternion;
            let (wps, _, _) = decompose(&self.matrix_world());
            self.world_position_start = wps;
            self.point_start = sub(p, wps);
        }
        self.dragging = true;
        true
    }

    /// `pointerMove` while dragging → whether the object changed.
    pub fn mv(&mut self, x: f64, y: f64) -> bool {
        let Some(axis) = self.axis else { return false };
        if !self.dragging {
            return false;
        }
        let Some(ray) = self.cam_ray(x, y) else { return false };
        let Some(p) = self.plane_hit(&ray) else { return false };
        self.point_end = sub(p, self.world_position_start);
        let pqi = self.parent_q_inv;
        match self.mode {
            Mode::Translate => {
                let mut off = sub(self.point_end, self.point_start);
                if !axis.contains('X') {
                    off[0] = 0.0;
                }
                if !axis.contains('Y') {
                    off[1] = 0.0;
                }
                if !axis.contains('Z') {
                    off[2] = 0.0;
                }
                let off = apply_q(off, pqi);
                let off = [off[0] / 1.0, off[1] / 1.0, off[2] / 1.0];
                let p = add(off, self.position_start);
                // three clamps to minX…maxX (±Infinity): no change
                self.position = p;
            }
            Mode::Rotate => {
                let off = sub(self.point_end, self.point_start);
                let speed = 20.0 / distance(self.world_position, self.camera_position);
                let eye = self.eye;
                let mut in_plane = false;
                if axis == "XYZE" {
                    self.rotation_axis = normalize(cross(off, eye));
                    self.rotation_angle = dot(off, cross(self.rotation_axis, eye)) * speed;
                } else if matches!(axis, "X" | "Y" | "Z") {
                    self.rotation_axis = unit(axis);
                    let t = cross(unit(axis), eye);
                    if length(t) == 0.0 {
                        in_plane = true;
                    } else {
                        self.rotation_angle = dot(off, normalize(t)) * speed;
                    }
                }
                if axis == "E" || in_plane {
                    self.rotation_axis = eye;
                    self.rotation_angle = angle_to(self.point_end, self.point_start);
                    let (s, e) = (normalize(self.point_start), normalize(self.point_end));
                    self.rotation_angle *= if dot(cross(e, s), eye) < 0.0 { 1.0 } else { -1.0 };
                }
                self.rotation_axis = apply_q(self.rotation_axis, pqi);
                let q = q_axis_angle(self.rotation_axis, self.rotation_angle);
                self.quaternion = q_normalize(q_mul(q, self.quaternion_start));
            }
        }
        true
    }

    /// `pointerUp` → whether it ends a drag (three's `mouseUp`).
    pub fn up(&mut self) -> bool {
        let done = self.dragging && self.axis.is_some();
        self.dragging = false;
        self.axis = None;
        done
    }

    /// The handles to draw (part coordinates): segments with three's colours, the axis being used yellow.
    pub fn lines(&self) -> Vec<(V3, V3, [u8; 4])> {
        let Some(cam) = self.cam else { return Vec::new() };
        let mut out = Vec::new();
        let s = self.handle_scale(&cam);
        let eye = self.eye;
        let align = apply_q(eye, q_conj(ID_Q));
        let at = if self.dragging { self.world_position_start } else { self.world_position };
        let active = |name: &str| self.axis.is_some_and(|a| a == name || a.chars().any(|c| name.len() == 1 && name.starts_with(c)));
        let col = |name: &str, base: [u8; 4]| if active(name) { [255, 255, 0, 255] } else { base };
        let put = |out: &mut Vec<(V3, V3, [u8; 4])>, q: Q4, pts: &[V3], closed: bool, c: [u8; 4]| {
            let m = compose(self.world_position, q, [s, s, s]);
            let w: Vec<V3> = pts.iter().map(|p| apply_m4(*p, &m)).collect();
            for k in 1..w.len() {
                if let (Some(a), Some(b)) = (w.get(k - 1), w.get(k)) {
                    out.push((*a, *b, c));
                }
            }
            if closed && let (Some(a), Some(b)) = (w.last(), w.first()) {
                out.push((*a, *b, c));
            }
        };
        let red = [255, 0, 0, 255];
        let green = [0, 255, 0, 255];
        let blue = [0, 0, 255, 255];
        match self.mode {
            Mode::Translate => {
                for (name, u, base) in [("X", UX, red), ("Y", UY, green), ("Z", UZ, blue)] {
                    if !handle_quat(self.mode, name, eye, align).1 {
                        continue;
                    }
                    let c = col(name, base);
                    let (a, b) = perp(u);
                    put(&mut out, ID_Q, &[[0.0; 3], scale(u, 0.5)], false, c);
                    for sg in [1.0, -1.0] {
                        // the arrow: a cone 0.1 long, 0.04 wide, from ±0.5
                        let base0 = scale(u, 0.5 * sg);
                        let tip = scale(u, 0.6 * sg);
                        let ring: Vec<V3> = (0..8)
                            .map(|k| {
                                let t = f64::from(k) * PI / 4.0;
                                add(base0, add(scale(a, 0.04 * cos(t)), scale(b, 0.04 * sin(t))))
                            })
                            .collect();
                        put(&mut out, ID_Q, &ring, true, c);
                        for r in ring.iter().step_by(2) {
                            put(&mut out, ID_Q, &[*r, tip], false, c);
                        }
                    }
                }
                for (name, a, b, base) in
                    [("XY", UX, UY, [0, 0, 255, 128]), ("YZ", UY, UZ, [255, 0, 0, 128]), ("XZ", UX, UZ, [0, 255, 0, 128])]
                {
                    if !handle_quat(self.mode, name, eye, align).1 {
                        continue;
                    }
                    let c = col(name, base);
                    let o = add(scale(a, 0.15), scale(b, 0.15));
                    let h = 0.075;
                    let sq = [
                        add(o, add(scale(a, -h), scale(b, -h))),
                        add(o, add(scale(a, h), scale(b, -h))),
                        add(o, add(scale(a, h), scale(b, h))),
                        add(o, add(scale(a, -h), scale(b, h))),
                    ];
                    put(&mut out, ID_Q, &sq, true, c);
                }
                let oc = [0.1, 0.0, 0.0];
                let oct = [[0.0, 0.1, 0.0], oc, [0.0, -0.1, 0.0], [-0.1, 0.0, 0.0]];
                put(&mut out, ID_Q, &oct, true, col("XYZ", [255, 255, 255, 64]));
                put(
                    &mut out,
                    ID_Q,
                    &[[0.0, 0.0, 0.1], [0.0, 0.1, 0.0], [0.0, 0.0, -0.1], [0.0, -0.1, 0.0]],
                    true,
                    col("XYZ", [255, 255, 255, 64]),
                );
            }
            Mode::Rotate => {
                for (name, base, r, arc) in [
                    ("X", red, 0.5, 0.5),
                    ("Y", green, 0.5, 0.5),
                    ("Z", blue, 0.5, 0.5),
                    ("XYZE", [120, 120, 120, 255], 0.5, 1.0),
                    ("E", [255, 255, 0, 64], 0.75, 1.0),
                ] {
                    let (q, _) = handle_quat(self.mode, name, eye, align);
                    // CircleGeometry: a ring in the YZ plane turned per handle as gizmoRotate does
                    let setup = match name {
                        "Y" => q_euler(0.0, 0.0, -PI / 2.0),
                        "Z" | "XYZE" | "E" => q_euler(0.0, PI / 2.0, 0.0),
                        _ => ID_Q,
                    };
                    let n = if arc < 1.0 { 32 } else { 64 };
                    let pts: Vec<V3> = (0..=n)
                        .map(|k| {
                            let u = f64::from(k) / f64::from(n) * arc * PI * 2.0;
                            apply_q([0.0, r * cos(u), r * sin(u)], setup)
                        })
                        .collect();
                    let c = if self.dragging && name == "XYZE" { base } else { col(name, base) };
                    put(&mut out, q, &pts, false, c);
                }
            }
        }
        // the helpers: the axis lines the drag follows, and the drag from where it began
        let helper = [255, 255, 255, 128];
        let long = 1e3 * s;
        if let Some(axis) = self.axis {
            match self.mode {
                Mode::Translate => {
                    for (l, u) in [('X', UX), ('Y', UY), ('Z', UZ)] {
                        if axis.contains(l) {
                            out.push((add(at, scale(u, -long)), add(at, scale(u, long)), helper));
                        }
                    }
                }
                Mode::Rotate => {
                    if matches!(axis, "X" | "Y" | "Z") && dot(unit(axis), eye).abs() <= 0.9 {
                        out.push((
                            add(self.world_position, scale(unit(axis), -long)),
                            add(self.world_position, scale(unit(axis), long)),
                            helper,
                        ));
                    } else if axis == "XYZE" && self.dragging {
                        let u = self.rotation_axis;
                        out.push((add(self.world_position, scale(u, -long)), add(self.world_position, scale(u, long)), helper));
                    }
                }
            }
        }
        if self.dragging && self.mode == Mode::Translate {
            out.push((self.world_position_start, self.world_position, helper));
        }
        out
    }
}

/// Two unit vectors square to an axis.
fn perp(u: V3) -> (V3, V3) {
    if u[0] != 0.0 {
        (UY, UZ)
    } else if u[1] != 0.0 {
        (UZ, UX)
    } else {
        (UX, UY)
    }
}

/// A handle's quaternion and whether it shows (TransformControlsGizmo.updateMatrixWorld, world space).
fn handle_quat(mode: Mode, name: &str, eye: V3, align: V3) -> (Q4, bool) {
    match mode {
        Mode::Translate => {
            let vis = match name {
                "X" => dot(UX, eye).abs() <= 0.99,
                "Y" => dot(UY, eye).abs() <= 0.99,
                "Z" => dot(UZ, eye).abs() <= 0.99,
                "XY" => dot(UZ, eye).abs() >= 0.2,
                "YZ" => dot(UX, eye).abs() >= 0.2,
                "XZ" => dot(UY, eye).abs() >= 0.2,
                _ => true,
            };
            (ID_Q, vis)
        }
        Mode::Rotate => {
            let mut q = ID_Q;
            if name.contains('E') {
                q = q_from_rot(&look_at(eye, [0.0; 3], UY));
            }
            match name {
                "X" => q = q_mul(ID_Q, q_axis_angle(UX, atan2(-align[1], align[2]))),
                "Y" => q = q_mul(ID_Q, q_axis_angle(UY, atan2(align[0], align[2]))),
                "Z" => q = q_mul(ID_Q, q_axis_angle(UZ, atan2(align[1], align[0]))),
                _ => {}
            }
            (q, true)
        }
    }
}

/// asm-ui.js `deltaOf(g)`: the object's world matrix after the drag times the start's inverse, near-zeros made 0.
pub fn delta_of(tc: &Tc, start: &M4) -> M4 {
    mul(&tc.matrix_world(), &invert(start)).map(|v| if v.abs() < 1e-12 { 0.0 } else { v })
}

/// The stand-in placed at `c` with no turn (asm-ui.js `placeProxy`) → its start matrix.
pub fn place(tc: &mut Tc, c: V3) -> M4 {
    tc.position = c;
    tc.quaternion = ID_Q;
    tc.matrix_world()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam() -> Camera {
        Camera::perspective([300.0, 240.0, 300.0], [0.0; 3], UY, 35.0, 800.0 / 600.0, 1.0, 5000.0)
    }

    /// A pixel where a point (part coordinates) is seen.
    fn px_of(c: &Camera, p: V3) -> (f64, f64) {
        let v = apply_m4(apply_m4(p, &invert(&c.world)), &c.proj);
        ((v[0] + 1.0) / 2.0 * 800.0, (1.0 - v[1]) / 2.0 * 600.0)
    }

    #[test]
    fn translate_along_an_axis() {
        let c = cam();
        let mut tc = Tc::new(Mode::Translate, 0.9);
        let start = place(&mut tc, [0.0; 3]);
        tc.update(&c);
        let s = tc.handle_scale(&c);
        // the X arrow's middle
        let (x0, y0) = px_of(&c, [0.3 * s, 0.0, 0.0]);
        let (x1, y1) = px_of(&c, [0.3 * s + 40.0, 0.0, 0.0]);
        let (a, b) = ndc(x0, y0, 800.0, 600.0);
        tc.hover(a, b);
        assert_eq!(tc.axis, Some("X"));
        tc.update(&c);
        assert!(tc.down(a, b));
        tc.update(&c);
        let (a, b) = ndc(x1, y1, 800.0, 600.0);
        assert!(tc.mv(a, b));
        assert!(tc.up());
        let d = delta_of(&tc, &start);
        assert!((d[12] - 40.0).abs() < 1e-6 && d[13] == 0.0 && d[14] == 0.0, "{d:?}");
        assert!(!tc.lines().is_empty());
    }

    #[test]
    fn turn_about_an_axis() {
        let c = cam();
        let mut tc = Tc::new(Mode::Rotate, 0.9);
        let start = place(&mut tc, [10.0, 0.0, 0.0]);
        tc.update(&c);
        let s = tc.handle_scale(&c);
        // a point on the Y ring (the XZ plane), on the half facing the camera
        let r = 0.5 * s;
        let p0 = [10.0 + r * (PI / 4.0).cos(), 0.0, r * (PI / 4.0).sin()];
        let (x0, y0) = px_of(&c, p0);
        let (a, b) = ndc(x0, y0, 800.0, 600.0);
        tc.hover(a, b);
        assert_eq!(tc.axis, Some("Y"));
        tc.update(&c);
        assert!(tc.down(a, b));
        tc.update(&c);
        let (a, b) = ndc(x0 + 30.0, y0, 800.0, 600.0);
        assert!(tc.mv(a, b));
        assert!(tc.up());
        let d = delta_of(&tc, &start);
        // a turn about Y through the centre: Y stays, the centre stays
        assert_eq!(d[5], 1.0);
        let c2 = apply_m4([10.0, 0.0, 0.0], &d);
        assert!(distance(c2, [10.0, 0.0, 0.0]) < 1e-9, "{c2:?}");
        assert!(d[0] < 1.0);
    }

    #[test]
    fn nothing_under_the_pointer() {
        let c = cam();
        let mut tc = Tc::new(Mode::Translate, 0.9);
        place(&mut tc, [0.0; 3]);
        tc.update(&c);
        tc.hover(0.95, 0.95);
        assert_eq!(tc.axis, None);
        assert!(!tc.down(0.95, 0.95));
        assert!(!tc.mv(0.9, 0.9));
        assert!(!tc.up());
    }
}
