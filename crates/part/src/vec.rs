//! vec.js — 3D vectors, plane frames, column-major 4×4 matrices (what manifold's `transform` takes), the base planes
//! (Y up: front XY n +Z, top XZ n +Y with sketch y = −Z, right YZ n +X with sketch x = −Z), `frameFor`, rotations,
//! reflections, a small dense solver and the number formatter.

use crate::js;

pub type V3 = [f64; 3];
pub type M4 = [f64; 16];

pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn mul(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn len(a: V3) -> f64 {
    js::hypot3(a[0], a[1], a[2])
}
/// `norm(a)`: a zero (or NaN-long) vector divides by 1.
pub fn norm(a: V3) -> V3 {
    let l = len(a);
    let l = if l == 0.0 || l.is_nan() { 1.0 } else { l };
    [a[0] / l, a[1] / l, a[2] / l]
}

/// A plane frame `{ o, x, y, n }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub o: V3,
    pub x: V3,
    pub y: V3,
    pub n: V3,
}

impl Frame {
    pub fn to_json(&self) -> serde_json::Value {
        use crate::jv;
        jv::obj(vec![("o", jv::pt3(self.o)), ("x", jv::pt3(self.x)), ("y", jv::pt3(self.y)), ("n", jv::pt3(self.n))])
    }
    pub fn from_json(v: &serde_json::Value) -> Frame {
        let g = |k: &str| -> V3 {
            let a = crate::jv::arr(v.get(k));
            let n = |i: usize| crate::jv::as_num(a.get(i)).unwrap_or(f64::NAN);
            [n(0), n(1), n(2)]
        };
        Frame { o: g("o"), x: g("x"), y: g("y"), n: g("n") }
    }
}

/// `BASE_PLANES[name]`.
pub fn base_plane(name: &str) -> Option<Frame> {
    Some(match name {
        "front" => Frame { o: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], n: [0.0, 0.0, 1.0] },
        "top" => Frame { o: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 0.0, -1.0], n: [0.0, 1.0, 0.0] },
        "right" => Frame { o: [0.0; 3], x: [0.0, 0.0, -1.0], y: [0.0, 1.0, 0.0], n: [1.0, 0.0, 0.0] },
        _ => return None,
    })
}

/// `frameFor(n, p)`.
pub fn frame_for(n: V3, p: V3) -> Frame {
    let n = norm(n);
    let x = if n[1].abs() > 0.9 { norm(sub([1.0, 0.0, 0.0], mul(n, n[0]))) } else { norm(cross([0.0, 1.0, 0.0], n)) };
    let y = cross(n, x);
    let d = dot(p, n);
    Frame { o: mul(n, d), x, y, n }
}

/// `toWorld(f, u, v)`.
pub fn to_world(f: &Frame, u: f64, v: f64) -> V3 {
    [f.o[0] + f.x[0] * u + f.y[0] * v, f.o[1] + f.x[1] * u + f.y[1] * v, f.o[2] + f.x[2] * u + f.y[2] * v]
}
/// `toPlane(f, p)`.
pub fn to_plane(f: &Frame, p: V3) -> [f64; 2] {
    let d = sub(p, f.o);
    [dot(d, f.x), dot(d, f.y)]
}
/// `flipFrame(f)`.
pub fn flip_frame(f: &Frame) -> Frame {
    Frame { o: f.o, x: mul(f.x, -1.0), y: f.y, n: mul(f.n, -1.0) }
}
/// `frameMatrix(f)`.
pub fn frame_matrix(f: &Frame) -> M4 {
    basis_matrix(f.x, f.y, f.n, f.o)
}
/// `basisMatrix(a, b, c, o)`.
pub fn basis_matrix(a: V3, b: V3, c: V3, o: V3) -> M4 {
    [a[0], a[1], a[2], 0.0, b[0], b[1], b[2], 0.0, c[0], c[1], c[2], 0.0, o[0], o[1], o[2], 1.0]
}
/// `translation(t)`.
pub fn translation(t: V3) -> M4 {
    [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, t[0], t[1], t[2], 1.0]
}

/// `rotationAbout(p, d, deg)`.
pub fn rotation_about(p: V3, d: V3, deg: f64) -> M4 {
    let [x, y, z] = norm(d);
    let a = deg * std::f64::consts::PI / 180.0;
    let (c, s) = (js::cos(a), js::sin(a));
    let t = 1.0 - c;
    let r = [
        t * x * x + c,
        t * x * y + s * z,
        t * x * z - s * y,
        t * x * y - s * z,
        t * y * y + c,
        t * y * z + s * x,
        t * x * z + s * y,
        t * y * z - s * x,
        t * z * z + c,
    ];
    let rp = [r[0] * p[0] + r[3] * p[1] + r[6] * p[2], r[1] * p[0] + r[4] * p[1] + r[7] * p[2], r[2] * p[0] + r[5] * p[1] + r[8] * p[2]];
    [r[0], r[1], r[2], 0.0, r[3], r[4], r[5], 0.0, r[6], r[7], r[8], 0.0, p[0] - rp[0], p[1] - rp[1], p[2] - rp[2], 1.0]
}

/// `reflection(o, n)`.
pub fn reflection(o: V3, n: V3) -> M4 {
    let n = norm(n);
    let k = 2.0 * dot(o, n);
    let ni = |i: usize| n.get(i).copied().unwrap_or(f64::NAN);
    let m = |i: usize, j: usize| (if i == j { 1.0 } else { 0.0 }) - 2.0 * ni(i) * ni(j);
    [m(0, 0), m(1, 0), m(2, 0), 0.0, m(0, 1), m(1, 1), m(2, 1), 0.0, m(0, 2), m(1, 2), m(2, 2), 0.0, k * n[0], k * n[1], k * n[2], 1.0]
}

/// `applyMat(m, p)`.
pub fn apply_mat(m: &M4, p: V3) -> V3 {
    [
        m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
        m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
        m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
    ]
}
/// `applyDir(m, d)`.
pub fn apply_dir(m: &M4, d: V3) -> V3 {
    [m[0] * d[0] + m[4] * d[1] + m[8] * d[2], m[1] * d[0] + m[5] * d[1] + m[9] * d[2], m[2] * d[0] + m[6] * d[1] + m[10] * d[2]]
}
/// `matMul(a, b)`.
pub fn mat_mul(a: &M4, b: &M4) -> M4 {
    // column-major: r[c*4 + ri] = Σk a[k*4 + ri] · b[c*4 + k]
    let mut r = [0.0; 16];
    for (i, x) in r.iter_mut().enumerate() {
        let (c, ri) = (i / 4, i % 4);
        let mut s = 0.0;
        for k in 0..4usize {
            let ai = k.saturating_mul(4).saturating_add(ri);
            let bi = c.saturating_mul(4).saturating_add(k);
            s += a.get(ai).copied().unwrap_or(f64::NAN) * b.get(bi).copied().unwrap_or(f64::NAN);
        }
        *x = s;
    }
    r
}

/// `solveDense(A, b)`: None when singular.
pub fn solve_dense(aa: &[Vec<f64>], b: &[f64]) -> Option<Vec<f64>> {
    let n = b.len();
    let mut m: Vec<Vec<f64>> = aa
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let mut r = row.clone();
            r.push(b.get(i).copied().unwrap_or(f64::NAN));
            r
        })
        .collect();
    let g = |m: &Vec<Vec<f64>>, i: usize, j: usize| m.get(i).and_then(|r| r.get(j)).copied().unwrap_or(f64::NAN);
    for c in 0..n {
        let mut p = c;
        for r in c.saturating_add(1)..n {
            if g(&m, r, c).abs() > g(&m, p, c).abs() {
                p = r;
            }
        }
        if g(&m, p, c).abs() < 1e-300 {
            return None;
        }
        m.swap(c, p);
        let piv = g(&m, c, c);
        for r in c.saturating_add(1)..n {
            let f = g(&m, r, c) / piv;
            if f == 0.0 {
                continue;
            }
            for k in c..=n {
                let v = g(&m, r, k) - f * g(&m, c, k);
                if let Some(x) = m.get_mut(r).and_then(|row| row.get_mut(k)) {
                    *x = v;
                }
            }
        }
    }
    let mut x = vec![0.0; n];
    for r in (0..n).rev() {
        let mut s = g(&m, r, n);
        for k in r.saturating_add(1)..n {
            s -= g(&m, r, k) * x.get(k).copied().unwrap_or(f64::NAN);
        }
        if let Some(v) = x.get_mut(r) {
            *v = s / g(&m, r, r);
        }
    }
    Some(x)
}

/// `fmt(n, dp)`.
pub fn fmt(n: f64, dp: usize) -> String {
    if !n.is_finite() {
        return String::new();
    }
    let a = if n.abs() < 1e-9 { 0.0 } else { n };
    js::num_str(js::fixed_num(a, dp))
}
