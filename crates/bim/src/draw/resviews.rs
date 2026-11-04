//! res-views.js (0.5 part 4, RC.5): elevations and sections from the 3D model with hidden lines removed. Every
//! element's triangles (`modelTriangles`) are projected along a horizontal direction (`projectView`); a depth buffer of
//! every triangle (f32, as the JS's Float32Array) decides which feature edges (boundaries and creases over 25°) are
//! seen; a section's plane cuts what it passes through into poché loops.
//!
//! View coordinates (mm): x along the view's right ([dir.y, −dir.x]), y = z.

use std::collections::HashMap;

use crate::draw::m;
use crate::model::Model;
use crate::util::M4;

const CREASE_DEG: f64 = 25.0;

/// res-views.js elevationDirs: the side an elevation shows → the way it looks.
pub const ELEVATION_DIRS: [(&str, [f64; 2]); 4] =
    [("north", [0.0, -1.0]), ("east", [-1.0, 0.0]), ("south", [0.0, 1.0]), ("west", [1.0, 0.0])];

/// An element's triangles (plan mm, z up): 9 numbers a triangle.
#[derive(Clone, Debug, PartialEq)]
pub struct Elem {
    pub key: f64,
    pub cls: String,
    pub tris: Vec<f64>,
}

/// `modelTriangles(model, M, { skip })`: every element's triangles through M (viewer → plan mm), in the batches' order.
pub fn model_triangles(model: &Model, mm: &M4, skip: &std::collections::HashSet<u64>) -> Vec<Elem> {
    let mut order: Vec<u64> = Vec::new();
    let mut map: HashMap<u64, Elem> = HashMap::new();
    let ap = |x: f64, y: f64, z: f64| -> [f64; 3] {
        [
            mm[0] * x + mm[4] * y + mm[8] * z + mm[12],
            mm[1] * x + mm[5] * y + mm[9] * z + mm[13],
            mm[2] * x + mm[6] * y + mm[10] * z + mm[14],
        ]
    };
    let kb = model.key_base;
    for b in &model.batches {
        if b.space {
            continue;
        }
        let p = &b.positions;
        let ntri = b.indices.len() / 3;
        for t in 0..ntri {
            let Some(&key) = b.tri_ids.get(t) else { continue };
            let k = u64::from(key);
            if skip.contains(&k) {
                continue;
            }
            let e = map.entry(k).or_insert_with(|| {
                order.push(k);
                let cls = model.class_of.get(&crate::model::u32_of(f64::from(key) - kb).into()).cloned().unwrap_or_default();
                Elem { key: f64::from(key), cls, tris: Vec::new() }
            });
            for c in 0..3 {
                let Some(&v) = b.indices.get(t.saturating_mul(3).saturating_add(c)) else { continue };
                let v = usize::try_from(v).unwrap_or(usize::MAX).saturating_mul(3);
                let g = |o: usize| p.get(v.saturating_add(o)).map_or(f64::NAN, |x| f64::from(*x));
                e.tris.extend(ap(g(0), g(1), g(2)));
            }
        }
    }
    order.iter().filter_map(|k| map.remove(k)).collect()
}

/// A section's plane: `cut.o` with how far behind it to look (0 = all).
#[derive(Clone, Copy, Debug)]
pub struct CutAt {
    pub o: [f64; 2],
    pub far: f64,
}

/// What a view draws: lines [x1, y1, x2, y2, key], cut loops, the frame, how many elements, whether the ground shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub lines: Vec<[f64; 5]>,
    pub cuts: Vec<ViewCut>,
    pub bounds: Option<[f64; 4]>,
    pub n: usize,
    pub ground: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ViewCut {
    pub key: f64,
    pub cls: String,
    pub loops: Vec<Vec<[f64; 2]>>,
}

fn is_ground(cls: &str) -> bool {
    cls.contains("Geographic")
}

/// A triangle in view coordinates: three (x, y, depth).
type Tri = [[f64; 3]; 3];
/// Its edges: (a, b) in order.
fn edges3(t: &Tri) -> [([f64; 3], [f64; 3]); 3] {
    [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])]
}

/// `clipTri(t, d0)`: a triangle cut to depth ≥ d0 → 0, 1 or 2 triangles.
fn clip_tri(t: Tri, d0: f64) -> Vec<Tri> {
    let n = t.iter().filter(|p| p[2] >= d0).count();
    if n == 3 {
        return vec![t];
    }
    if n == 0 {
        return Vec::new();
    }
    let mut out: Vec<[f64; 3]> = Vec::new();
    for (a, b) in edges3(&t) {
        if a[2] >= d0 {
            out.push(a);
        }
        if (a[2] >= d0) != (b[2] >= d0) {
            let f = (d0 - a[2]) / (b[2] - a[2]);
            out.push([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, d0]);
        }
    }
    let Some((&p, rest)) = out.split_first() else { return Vec::new() };
    rest.windows(2).filter_map(|w| if let [q, r] = w { Some([p, *q, *r]) } else { None }).collect()
}

/// `projectView(elements, { dir, cut, far, res, margin })`.
pub fn project_view(elements: &[Elem], dir: [f64; 2], cut: Option<CutAt>, res: f64, margin: [f64; 3]) -> View {
    let right = [dir[1], -dir[0]];
    let d0 = cut.map_or(f64::NEG_INFINITY, |c| c.o[0] * dir[0] + c.o[1] * dir[1]);
    let d1 = match cut {
        Some(c) if c.far > 0.0 => d0 + c.far,
        _ => f64::INFINITY,
    };
    let (mut x0, mut y0, mut x1, mut y1, mut dmin, mut dmax) =
        (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    // an elevation cuts the ground 600 in front of the building
    let mut ground_d0: Option<f64> = None;
    if cut.is_none() && elements.iter().any(|e| is_ground(&e.cls)) {
        let mut near = f64::INFINITY;
        for e in elements.iter().filter(|e| !is_ground(&e.cls)) {
            for q in e.tris.as_chunks::<3>().0 {
                near = m::min(near, q[0] * dir[0] + q[1] * dir[1]);
            }
        }
        if near.is_finite() {
            ground_d0 = Some(near - 600.0);
        }
    }
    struct V<'a> {
        key: f64,
        cls: &'a str,
        tri: Vec<Tri>,
    }
    let mut view: Vec<V<'_>> = Vec::new();
    let mut cuts: Vec<ViewCut> = Vec::new();
    for e in elements {
        let mut tri: Vec<Tri> = Vec::new();
        let mut segs: Vec<[[f64; 2]; 2]> = Vec::new();
        let is_g = ground_d0.is_some() && is_ground(&e.cls);
        let c0 = if is_g { ground_d0.unwrap_or(d0) } else { d0 };
        let cutting = cut.is_some() || is_g;
        for q in e.tris.as_chunks::<9>().0 {
            let vtx = |v: [f64; 3]| -> [f64; 3] { [v[0] * right[0] + v[1] * right[1], v[2], v[0] * dir[0] + v[1] * dir[1]] };
            let [a0, a1, a2, b0, b1, b2, c0_, c1, c2] = *q;
            let t: Tri = [vtx([a0, a1, a2]), vtx([b0, b1, b2]), vtx([c0_, c1, c2])];
            let dd = m::min(m::min(t[0][2], t[1][2]), t[2][2]);
            let big = m::max(m::max(t[0][2], t[1][2]), t[2][2]);
            if big < c0 || dd > d1 {
                continue;
            }
            if cutting && dd < c0 && big > c0 {
                let mut pts: Vec<[f64; 2]> = Vec::new();
                for (a, b) in edges3(&t) {
                    if (a[2] - c0) * (b[2] - c0) < 0.0 {
                        let f = (c0 - a[2]) / (b[2] - a[2]);
                        pts.push([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]);
                    }
                }
                if let [a, b] = pts[..] {
                    segs.push([a, b]);
                }
            }
            if cutting { tri.extend(clip_tri(t, c0)) } else { tri.push(t) }
        }
        if tri.is_empty() && segs.is_empty() {
            continue;
        }
        for v in tri.iter().flatten() {
            x0 = m::min(x0, v[0]);
            x1 = m::max(x1, v[0]);
            y0 = m::min(y0, v[1]);
            y1 = m::max(y1, v[1]);
            dmin = m::min(dmin, v[2]);
            dmax = m::max(dmax, v[2]);
        }
        if !segs.is_empty() {
            cuts.push(ViewCut { key: e.key, cls: e.cls.clone(), loops: chain(&segs) });
        }
        view.push(V { key: e.key, cls: &e.cls, tri });
    }
    if view.is_empty() {
        return View { lines: Vec::new(), cuts, bounds: None, n: 0, ground: false };
    }
    // framed on the building: the ground cut to a margin round it
    if view.iter().any(|v| is_ground(v.cls)) && view.iter().any(|v| !is_ground(v.cls)) {
        let (mut a, mut b, mut c, mut d) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for v in view.iter().filter(|v| !is_ground(v.cls)) {
            for q in v.tri.iter().flatten() {
                a = m::min(a, q[0]);
                c = m::max(c, q[0]);
                b = m::min(b, q[1]);
                d = m::max(d, q[1]);
            }
        }
        for cu in cuts.iter().filter(|cu| !is_ground(&cu.cls)) {
            for l in &cu.loops {
                for q in l {
                    a = m::min(a, q[0]);
                    c = m::max(c, q[0]);
                    b = m::min(b, q[1]);
                    d = m::max(d, q[1]);
                }
            }
        }
        if a.is_finite() {
            x0 = m::max(x0, a - margin[0]);
            x1 = m::min(x1, c + margin[0]);
            y0 = m::max(y0, b - margin[1]);
            y1 = m::min(y1, d + margin[2]);
        }
    }
    // the depth buffer
    let w = m::max(1.0, x1 - x0);
    let h = m::max(1.0, y1 - y0);
    let s = res / m::max(w, h);
    let nxf = (w * s).ceil() + 3.0;
    let nyf = (h * s).ceil() + 3.0;
    if !(nxf.is_finite() && nyf.is_finite() && nxf * nyf <= 64.0e6) {
        return View { lines: Vec::new(), cuts, bounds: None, n: view.len(), ground: false };
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (nx, ny) = (nxf as usize, nyf as usize);
    let mut zb = vec![f32::INFINITY; nx.saturating_mul(ny)];
    let px = |x: f64| (x - x0) * s + 1.0;
    let py = |y: f64| (y - y0) * s + 1.0;
    for v in &view {
        for t in &v.tri {
            let s3 = |q: [f64; 3]| [px(q[0]), py(q[1]), q[2]];
            raster(&mut zb, nx, ny, s3(t[0]), s3(t[1]), s3(t[2]));
        }
    }
    let tol = m::max(m::max(3.0, (dmax - dmin) * 0.002), 4.0 / s);
    let visible = |qx: f64, qy: f64, d: f64| -> bool {
        let (i0, j0) = (qx.floor(), qy.floor());
        let mut best = f64::INFINITY;
        for dj in -1..=1 {
            for di in -1..=1 {
                let (i, j) = (i0 + f64::from(di), j0 + f64::from(dj));
                if i < 0.0 || j < 0.0 || i >= nxf || j >= nyf {
                    continue;
                }
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let z = (j as usize)
                    .checked_mul(nx)
                    .and_then(|r| r.checked_add(i as usize))
                    .and_then(|k| zb.get(k))
                    .map_or(f64::INFINITY, |z| f64::from(*z));
                if z < best {
                    best = z;
                }
            }
        }
        d <= best + tol
    };
    let mut lines: Vec<[f64; 5]> = Vec::new();
    for v in &view {
        for (a, b) in feature_edges(&v.tri) {
            let l = m::hypot((b[0] - a[0]) * s, (b[1] - a[1]) * s);
            let n = m::max(1.0, (l / 0.75).ceil());
            if n > 1e6 {
                continue;
            }
            let mut run: Option<[f64; 2]> = None;
            let mut k = 0.0;
            while k <= n {
                let f = k / n;
                let (x, y, d) = (a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f);
                let vis = visible(px(x), py(y), d);
                if vis && run.is_none() {
                    run = Some([x, y]);
                }
                if (!vis || k == n)
                    && let Some(r0) = run
                {
                    let ex = if vis { [x, y] } else { [a[0] + (b[0] - a[0]) * ((k - 1.0) / n), a[1] + (b[1] - a[1]) * ((k - 1.0) / n)] };
                    if m::hypot(ex[0] - r0[0], ex[1] - r0[1]) > 1.0 {
                        lines.push([r0[0], r0[1], ex[0], ex[1], v.key]);
                    }
                    run = None;
                }
                k += 1.0;
            }
        }
    }
    let bx = [x0, y0, x1, y1];
    let kept: Vec<[f64; 5]> = merge_collinear(lines).into_iter().filter_map(|l| clip_seg(l, bx)).collect();
    for cu in &mut cuts {
        cu.loops = cu.loops.iter().map(|lp| clip_poly(lp, bx)).filter(|lp| lp.len() >= 3).collect();
    }
    cuts.retain(|c| !c.loops.is_empty());
    View { lines: kept, cuts, bounds: Some(bx), n: view.len(), ground: view.iter().any(|v| is_ground(v.cls)) }
}

/// `clipSeg(l, b)` (Liang–Barsky)
fn clip_seg(l: [f64; 5], b: [f64; 4]) -> Option<[f64; 5]> {
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let (dx, dy) = (l[2] - l[0], l[3] - l[1]);
    for (p, q) in [(-dx, l[0] - b[0]), (dx, b[2] - l[0]), (-dy, l[1] - b[1]), (dy, b[3] - l[1])] {
        if p.abs() < 1e-12 {
            if q < -1e-9 {
                return None;
            }
            continue;
        }
        let r = q / p;
        if p < 0.0 {
            if r > t1 {
                return None;
            }
            if r > t0 {
                t0 = r;
            }
        } else {
            if r < t0 {
                return None;
            }
            if r < t1 {
                t1 = r;
            }
        }
    }
    if t0 == 0.0 && t1 == 1.0 {
        return Some(l);
    }
    Some([l[0] + dx * t0, l[1] + dy * t0, l[0] + dx * t1, l[1] + dy * t1, l[4]])
}

/// `clipPoly(poly, b)` (Sutherland–Hodgman)
fn clip_poly(poly: &[[f64; 2]], b: [f64; 4]) -> Vec<[f64; 2]> {
    let mut out = poly.to_vec();
    for side in 0..4 {
        let inside = |p: &[f64; 2]| match side {
            0 => p[0] >= b[0],
            1 => p[0] <= b[2],
            2 => p[1] >= b[1],
            _ => p[1] <= b[3],
        };
        let cross = |p: [f64; 2], q: [f64; 2]| -> [f64; 2] {
            match side {
                0 => [b[0], p[1] + (q[1] - p[1]) * (b[0] - p[0]) / (q[0] - p[0])],
                1 => [b[2], p[1] + (q[1] - p[1]) * (b[2] - p[0]) / (q[0] - p[0])],
                2 => [p[0] + (q[0] - p[0]) * (b[1] - p[1]) / (q[1] - p[1]), b[1]],
                _ => [p[0] + (q[0] - p[0]) * (b[3] - p[1]) / (q[1] - p[1]), b[3]],
            }
        };
        let src = std::mem::take(&mut out);
        let n = src.len();
        for i in 0..n {
            let (p, q) = (m::at(&src, i), m::at(&src, m::nx(i, n)));
            let (pi, qi) = (inside(&p), inside(&q));
            if pi {
                out.push(p);
            }
            if pi != qi {
                out.push(cross(p, q));
            }
        }
        if out.is_empty() {
            break;
        }
    }
    out
}

/// `raster(Z, …)`: a triangle (pixel x, y, depth) into the buffer, the nearest depth kept.
fn raster(zb: &mut [f32], nx: usize, ny: usize, a: [f64; 3], b: [f64; 3], c: [f64; 3]) {
    let (nxf, nyf) = (nx as f64, ny as f64);
    let put = |zb: &mut [f32], i: f64, j: f64, d: f64| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let Some(k) = (j as usize).checked_mul(nx).and_then(|r| r.checked_add(i as usize)) else { return };
        if let Some(z) = zb.get_mut(k)
            && d < f64::from(*z)
        {
            #[allow(clippy::cast_possible_truncation)]
            {
                *z = d as f32;
            }
        }
    };
    let minx = m::max(0.0, m::min(m::min(a[0], b[0]), c[0]).floor());
    let maxx = m::min(nxf - 1.0, m::max(m::max(a[0], b[0]), c[0]).ceil());
    let miny = m::max(0.0, m::min(m::min(a[1], b[1]), c[1]).floor());
    let maxy = m::min(nyf - 1.0, m::max(m::max(a[1], b[1]), c[1]).ceil());
    let den = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
    if den.abs() < 1e-12 {
        // edge-on: its outline pixels still hide what is behind it
        for (p, q) in [(a, b), (b, c), (c, a)] {
            let l = m::hypot(q[0] - p[0], q[1] - p[1]).ceil() + 1.0;
            if !(l <= 1e6) {
                continue;
            }
            let mut k = 0.0;
            while k <= l {
                let f = k / l;
                let (x, y, d) = (p[0] + (q[0] - p[0]) * f, p[1] + (q[1] - p[1]) * f, p[2] + (q[2] - p[2]) * f);
                let (i, j) = (m::round(x), m::round(y));
                if i >= 0.0 && j >= 0.0 && i < nxf && j < nyf {
                    put(zb, i, j, d);
                }
                k += 1.0;
            }
        }
        return;
    }
    let mut j = miny;
    while j <= maxy {
        let mut i = minx;
        while i <= maxx {
            let (qx, qy) = (i + 0.5, j + 0.5);
            let w0 = ((b[1] - c[1]) * (qx - c[0]) + (c[0] - b[0]) * (qy - c[1])) / den;
            let w1 = ((c[1] - a[1]) * (qx - c[0]) + (a[0] - c[0]) * (qy - c[1])) / den;
            let w2 = 1.0 - w0 - w1;
            if !(w0 < -1e-6 || w1 < -1e-6 || w2 < -1e-6) {
                put(zb, i, j, w0 * a[2] + w1 * b[2] + w2 * c[2]);
            }
            i += 1.0;
        }
        j += 1.0;
    }
}

/// `String(Math.round(v * 2))` parts of an edge key.
fn kpart(v: f64) -> String {
    crate::js::num_str(m::round(v * 2.0))
}

/// An edge's ends and the normals of the triangles that share it.
type EdgeNs = ([f64; 3], [f64; 3], Vec<[f64; 3]>);

/// `featureEdges(tris)`: boundary and crease edges (vertices merged within 0.5 mm), in the order first met.
fn feature_edges(tris: &[Tri]) -> Vec<([f64; 3], [f64; 3])> {
    let crease = (CREASE_DEG * std::f64::consts::PI / 180.0).cos();
    let key = |p: [f64; 3]| format!("{},{},{}", kpart(p[0]), kpart(p[1]), kpart(p[2]));
    let mut order: Vec<String> = Vec::new();
    let mut edges: HashMap<String, EdgeNs> = HashMap::new();
    for t in tris {
        let (p0, p1, p2) = (t[0], t[1], t[2]);
        let (ux, uy, ud, vx, vy, vd) = (p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2], p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]);
        let (mut nx, mut ny, mut nz) = (uy * vd - ud * vy, ud * vx - ux * vd, ux * vy - uy * vx);
        let l = (nx * nx + ny * ny + nz * nz).sqrt();
        if l < 1e-9 {
            continue;
        }
        nx /= l;
        ny /= l;
        nz /= l;
        for (a, b) in edges3(t) {
            let (ka, kb) = (key(a), key(b));
            let id = if ka < kb { format!("{ka}|{kb}") } else { format!("{kb}|{ka}") };
            match edges.get_mut(&id) {
                Some(e) => e.2.push([nx, ny, nz]),
                None => {
                    order.push(id.clone());
                    edges.insert(id, (a, b, vec![[nx, ny, nz]]));
                }
            }
        }
    }
    let mut out = Vec::new();
    for id in &order {
        let Some((a, b, n)) = edges.get(id) else { continue };
        match n.as_slice() {
            [p, q] => {
                let c = (p[0] * q[0] + p[1] * q[1] + p[2] * q[2]).abs();
                if c < crease {
                    out.push((*a, *b));
                }
            }
            _ => out.push((*a, *b)),
        }
    }
    out
}

/// `chain(segs)`: 2D segments → closed loops where they join (ends within 0.5 mm).
fn chain(segs: &[[[f64; 2]; 2]]) -> Vec<Vec<[f64; 2]>> {
    let k = |p: [f64; 2]| format!("{},{}", kpart(p[0]), kpart(p[1]));
    let mut adj: HashMap<String, Vec<(usize, [f64; 2])>> = HashMap::new();
    for (i, [a, b]) in segs.iter().enumerate() {
        for (p, q) in [(*a, *b), (*b, *a)] {
            adj.entry(k(p)).or_default().push((i, q));
        }
    }
    let mut used = vec![false; segs.len()];
    let mut loops = Vec::new();
    let taken = |used: &[bool], i: usize| used.get(i).copied().unwrap_or(true);
    for (s, seg) in segs.iter().enumerate() {
        if taken(&used, s) {
            continue;
        }
        if let Some(u) = used.get_mut(s) {
            *u = true;
        }
        let [a, b] = *seg;
        let mut lp = vec![a, b];
        let mut cur = b;
        for _ in 0..segs.len() {
            let nx = adj.get(&k(cur)).and_then(|v| v.iter().find(|o| !taken(&used, o.0)).copied());
            let Some((i, to)) = nx else { break };
            if let Some(u) = used.get_mut(i) {
                *u = true;
            }
            cur = to;
            if k(cur) == k(a) {
                break;
            }
            lp.push(cur);
        }
        if lp.len() >= 3 {
            loops.push(lp);
        }
    }
    loops
}

/// `mergeCollinear(lines)`: touching collinear segments of one element joined.
fn merge_collinear(lines: Vec<[f64; 5]>) -> Vec<[f64; 5]> {
    let mut order: Vec<u64> = Vec::new();
    let mut by: HashMap<u64, Vec<[f64; 5]>> = HashMap::new();
    for l in lines {
        let k = l[4].to_bits();
        by.entry(k)
            .or_insert_with(|| {
                order.push(k);
                Vec::new()
            })
            .push(l);
    }
    let mut out = Vec::new();
    let mut budget: u64 = 50_000_000;
    for k in &order {
        let Some(mut list) = by.remove(k) else { continue };
        let mut changed = true;
        while changed && budget > 0 {
            changed = false;
            let n = list.len();
            let mut hit: Option<(usize, usize, [f64; 5])> = None;
            'outer: for i in 0..n {
                for j in i.saturating_add(1)..n {
                    budget = budget.saturating_sub(1);
                    let (Some(&a), Some(&b)) = (list.get(i), list.get(j)) else { continue };
                    for (p, q, r, t) in [
                        ([a[0], a[1]], [a[2], a[3]], [b[0], b[1]], [b[2], b[3]]),
                        ([a[0], a[1]], [a[2], a[3]], [b[2], b[3]], [b[0], b[1]]),
                        ([a[2], a[3]], [a[0], a[1]], [b[0], b[1]], [b[2], b[3]]),
                        ([a[2], a[3]], [a[0], a[1]], [b[2], b[3]], [b[0], b[1]]),
                    ] {
                        if m::hypot(q[0] - r[0], q[1] - r[1]) > 0.5 {
                            continue;
                        }
                        let (d1, d2) = ([q[0] - p[0], q[1] - p[1]], [t[0] - r[0], t[1] - r[1]]);
                        let (l1, l2) = (m::hypot(d1[0], d1[1]), m::hypot(d2[0], d2[1]));
                        if l1 == 0.0 || l2 == 0.0 || l1.is_nan() || l2.is_nan() || (d1[0] * d2[0] + d1[1] * d2[1]) / (l1 * l2) < 0.99999 {
                            continue;
                        }
                        hit = Some((i, j, [p[0], p[1], t[0], t[1], a[4]]));
                        break 'outer;
                    }
                }
            }
            if let Some((i, j, l)) = hit {
                if let Some(x) = list.get_mut(i) {
                    *x = l;
                }
                list.remove(j);
                changed = true;
            }
        }
        out.extend(list);
    }
    out
}
