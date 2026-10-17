//! analysis.js, section.js and surface.js's mesh helpers: draft analysis, zebra stripes and mean-curvature colours on
//! the part's mesh (colours stored as f32, a Float32Array in the JS), the live section view's plane arithmetic, and
//! the cotangent-Laplacian curvature and zebra test they use.

use crate::faces::Mesh;
use crate::i18n::t;
use crate::js;
use crate::vec::{self as v, V3};
use std::collections::HashMap;

#[allow(clippy::cast_possible_truncation)]
fn fround(x: f64) -> f64 {
    f64::from(x as f32)
}

/// `SECTION_BASES[k]`.
pub fn section_base(k: &str) -> Option<V3> {
    Some(match k {
        "front" => [0.0, 0.0, 1.0],
        "top" => [0.0, 1.0, 0.0],
        "right" => [1.0, 0.0, 0.0],
        _ => return None,
    })
}

/// `sectionRange(box, n)`.
pub fn section_range(min: V3, max: V3, n: V3) -> [f64; 2] {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for k in 0..8u32 {
        let p =
            [if k & 1 != 0 { max[0] } else { min[0] }, if k & 2 != 0 { max[1] } else { min[1] }, if k & 4 != 0 { max[2] } else { min[2] }];
        let d = v::dot(n, p);
        if d < lo {
            lo = d;
        }
        if d > hi {
            hi = d;
        }
    }
    [lo, hi]
}

fn xf(m: Option<&[f64; 16]>, p: V3, w: f64) -> V3 {
    match m {
        Some(m) => [
            m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + w * m[12],
            m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + w * m[13],
            m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + w * m[14],
        ],
        None => p,
    }
}

/// `faceSection(face, M)` for a plane face (type, n, o) → (n, c).
pub fn face_section(ty: &str, n: Option<V3>, o: Option<V3>, m: Option<&[f64; 16]>) -> Result<(V3, f64), String> {
    let (Some(n), Some(o)) = (n, o) else { return Err(t("section.need_flat", &[])) };
    if ty != "plane" {
        return Err(t("section.need_flat", &[]));
    }
    let nn = xf(m, n, 0.0);
    let l = js::hypot3(nn[0], nn[1], nn[2]);
    if !(l > 1e-12) {
        return Err(t("section.no_direction", &[]));
    }
    let nu = [nn[0] / l, nn[1] / l, nn[2] / l];
    let oo = xf(m, o, 1.0);
    Ok((nu, v::dot(nu, oo)))
}

fn unit(a: V3) -> V3 {
    let l = js::hypot3(a[0], a[1], a[2]);
    if l > 0.0 { [a[0] / l, a[1] / l, a[2] / l] } else { [0.0; 3] }
}

/// `draftAngle(n, pull)`.
pub fn draft_angle(n: V3, pull: V3) -> f64 {
    js::asin(js::max(-1.0, js::min(1.0, n[0] * pull[0] + n[1] * pull[1] + n[2] * pull[2]))) * 180.0 / std::f64::consts::PI
}

fn tri(m: &Mesh, t: usize) -> (V3, V3, V3) {
    (m.point(m.corner(t, 0)), m.point(m.corner(t, 1)), m.point(m.corner(t, 2)))
}
fn corner_normal(cn: Option<&[f64]>, t: usize, k: usize, tn: V3) -> V3 {
    match cn {
        Some(c) => {
            let b = t.saturating_mul(9).saturating_add(k.saturating_mul(3));
            let g = |i: usize| c.get(b.saturating_add(i)).copied().unwrap_or(f64::NAN);
            unit([g(0), g(1), g(2)])
        }
        None => tn,
    }
}

/// `draftAnalysis(mesh, pull, minDeg, colors)` → (colors, [positive, needs, negative, total]).
pub fn draft_analysis(m: &Mesh, corner_normals: Option<&[f64]>, pull: V3, min_deg: f64, colors: [[f64; 3]; 3]) -> (Vec<f64>, [f64; 4]) {
    let nt = m.tri_count();
    let cn = corner_normals.filter(|c| c.len() == nt.saturating_mul(9));
    let d = unit(pull);
    let mm = min_deg.abs();
    let mut out = vec![0.0; nt.saturating_mul(9)];
    let mut areas = [0.0f64; 4];
    for t in 0..nt {
        let (a, b, c) = tri(m, t);
        let u = v::sub(b, a);
        let w = v::sub(c, a);
        let cr = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
        let area = js::hypot3(cr[0], cr[1], cr[2]) / 2.0;
        let tn = unit(cr);
        areas[3] += area;
        for k in 0..3usize {
            let n = corner_normal(cn, t, k, tn);
            let ang = draft_angle(n, d);
            let cls = if ang >= mm - 1e-9 {
                0
            } else if ang <= -mm + 1e-9 {
                2
            } else {
                1
            };
            if let Some(x) = areas.get_mut(cls) {
                *x += area / 3.0;
            }
            let col = colors.get(cls).copied().unwrap_or([f64::NAN; 3]);
            for (i, cv) in col.iter().enumerate() {
                if let Some(x) = out.get_mut(t.saturating_mul(9).saturating_add(k.saturating_mul(3)).saturating_add(i)) {
                    *x = fround(*cv);
                }
            }
        }
    }
    (out, [areas[0], areas[1], areas[2], areas[3]])
}

/// surface.js `zebra(n, view, stripes)`.
pub fn zebra(n: V3, view: V3, stripes: f64) -> bool {
    let d = v::norm(view);
    let r = v::sub(d, v::mul(n, 2.0 * v::dot(d, n)));
    let a = js::atan2(r[0], r[2]);
    let x = ((a / std::f64::consts::PI + 1.0) * stripes / 2.0).floor();
    x % 2.0 != 0.0 && !x.is_nan()
}

/// `zebraColors(mesh, view, stripes, { dark, light })`.
pub fn zebra_colors(m: &Mesh, corner_normals: Option<&[f64]>, view: V3, stripes: f64, dark: [f64; 3], light: [f64; 3]) -> Vec<f64> {
    let nt = m.tri_count();
    let cn = corner_normals.filter(|c| c.len() == nt.saturating_mul(9));
    let mut out = vec![0.0; nt.saturating_mul(9)];
    for t in 0..nt {
        let (a, b, c) = tri(m, t);
        let u = v::sub(b, a);
        let w = v::sub(c, a);
        let tn = unit([u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]]);
        for k in 0..3usize {
            let n = corner_normal(cn, t, k, tn);
            let col = if zebra(n, view, stripes) { light } else { dark };
            for (i, cv) in col.iter().enumerate() {
                if let Some(x) = out.get_mut(t.saturating_mul(9).saturating_add(k.saturating_mul(3)).saturating_add(i)) {
                    *x = fround(*cv);
                }
            }
        }
    }
    out
}

/// surface.js `curvature(mesh)` → (gauss, mean, boundary).
pub fn curvature(m: &Mesh) -> (Vec<f64>, Vec<f64>, Vec<bool>) {
    let n = m.positions.len() / 3;
    let mut angle = vec![0.0f64; n];
    let mut area = vec![0.0f64; n];
    let mut lap = vec![[0.0f64; 3]; n];
    let mut nrm = vec![[0.0f64; 3]; n];
    let mut edge_count: HashMap<(usize, usize), u32> = HashMap::new();
    for t in 0..m.tri_count() {
        let ids = [m.corner(t, 0), m.corner(t, 1), m.corner(t, 2)];
        let q = [m.point(ids[0]), m.point(ids[1]), m.point(ids[2])];
        let cr = v::cross(v::sub(q[1], q[0]), v::sub(q[2], q[0]));
        let aa = v::len(cr) / 2.0;
        if !(aa > 0.0) {
            continue;
        }
        for k in 0..3usize {
            let (k1, k2) = (k.saturating_add(1) % 3, k.saturating_add(2) % 3);
            let id = |x: usize| ids.get(x).copied().unwrap_or(usize::MAX);
            let qq = |x: usize| q.get(x).copied().unwrap_or([f64::NAN; 3]);
            let (i, j) = (id(k), id(k1));
            let a = v::sub(qq(k1), qq(k));
            let b = v::sub(qq(k2), qq(k));
            if let Some(x) = angle.get_mut(i) {
                *x += js::atan2(v::len(v::cross(a, b)), v::dot(a, b));
            }
            if let Some(x) = area.get_mut(i) {
                *x += aa / 3.0;
            }
            if let Some(x) = nrm.get_mut(i) {
                for (xc, cc) in x.iter_mut().zip(cr) {
                    *xc += cc;
                }
            }
            let u = v::sub(qq(k), qq(k2));
            let w = v::sub(qq(k1), qq(k2));
            let cot = v::dot(u, w) / v::len(v::cross(u, w));
            let d = v::sub(qq(k), qq(k1));
            if let Some(x) = lap.get_mut(i) {
                for (xc, dc) in x.iter_mut().zip(d) {
                    *xc += cot * dc;
                }
            }
            if let Some(x) = lap.get_mut(j) {
                for (xc, dc) in x.iter_mut().zip(d) {
                    *xc -= cot * dc;
                }
            }
            let key = if i < j { (i, j) } else { (j, i) };
            let e = edge_count.entry(key).or_insert(0);
            *e = e.saturating_add(1);
        }
    }
    let mut boundary = vec![false; n];
    for ((a, b), c) in &edge_count {
        if *c == 1 {
            for x in [a, b] {
                if let Some(s) = boundary.get_mut(*x) {
                    *s = true;
                }
            }
        }
    }
    let mut gauss = vec![0.0f64; n];
    let mut mean = vec![0.0f64; n];
    for i in 0..n {
        let ar = area.get(i).copied().unwrap_or(0.0);
        if !(ar > 0.0) {
            continue;
        }
        let bd = boundary.get(i).copied().unwrap_or(false);
        let pi = std::f64::consts::PI;
        if let Some(g) = gauss.get_mut(i) {
            *g = ((if bd { pi } else { 2.0 * pi }) - angle.get(i).copied().unwrap_or(0.0)) / ar;
        }
        let l = lap.get(i).copied().unwrap_or([0.0; 3]);
        let nn = nrm.get(i).copied().unwrap_or([0.0; 3]);
        if let Some(mv) = mean.get_mut(i) {
            *mv = if bd { 0.0 } else { (if v::dot(l, nn) >= 0.0 { 1.0 } else { -1.0 }) * v::len(l) / (4.0 * ar) };
        }
    }
    (gauss, mean, boundary)
}

/// `curvatureColors(mesh, { concave, flat, convex })` → (colors, lim).
pub fn curvature_colors(m: &Mesh, concave: [f64; 3], flat: [f64; 3], convex: [f64; 3]) -> (Vec<f64>, f64) {
    let (_, mean, _) = curvature(m);
    let mut abs: Vec<f64> = mean.iter().map(|x| x.abs()).collect();
    cc_m3d::sort::timsort(&mut abs, |x, y| x - y);
    let at = js::usize_of(js::i64_of((js::f(abs.len()) * 0.95).floor()));
    let l0 = abs.get(at).copied().unwrap_or(0.0);
    let lim = if l0 == 0.0 || l0.is_nan() { 1.0 } else { l0 };
    let n = m.positions.len() / 3;
    let mut out = vec![0.0; n.saturating_mul(3)];
    for i in 0..n {
        let tt = js::max(-1.0, js::min(1.0, mean.get(i).copied().unwrap_or(0.0) / lim));
        let to = if tt < 0.0 { concave } else { convex };
        let w = tt.abs();
        for (k, (fk, tk)) in flat.iter().zip(to).enumerate() {
            if let Some(x) = out.get_mut(i.saturating_mul(3).saturating_add(k)) {
                *x = fround(fk + (tk - fk) * w);
            }
        }
    }
    (out, lim)
}
