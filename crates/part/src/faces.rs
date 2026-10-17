//! faces.js — planar faces of a triangle mesh: edge-connected triangles in one plane grouped, each group's boundary
//! traced into closed loops (outer + holes), loops in the group's own plane frame. Used to highlight a picked face,
//! export a face profile as DXF and write merged planar faces to STEP. Map iteration follows JS insertion order.

use crate::js;
use crate::vec::{self as v, V3};
use std::collections::HashMap;

/// A triangle mesh: positions (xyz, f32 values as f64) and indices (3 per triangle); `tri_face` from the kernel.
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub positions: Vec<f64>,
    pub indices: Vec<u32>,
    pub tri_face: Option<Vec<i32>>,
}

impl Mesh {
    pub fn tri_count(&self) -> usize {
        self.indices.len() / 3
    }
    fn idx(&self, i: usize) -> usize {
        self.indices.get(i).map_or(usize::MAX, |&x| x as usize)
    }
    pub fn point(&self, vi: usize) -> V3 {
        let g =
            |k: usize| vi.checked_mul(3).and_then(|b| b.checked_add(k)).and_then(|j| self.positions.get(j)).copied().unwrap_or(f64::NAN);
        [g(0), g(1), g(2)]
    }
    pub fn corner(&self, t: usize, k: usize) -> usize {
        self.idx(t.saturating_mul(3).saturating_add(k))
    }
}

/// `triNormal(pos, idx, t)` → (n, a, area2).
pub fn tri_normal(m: &Mesh, t: usize) -> (V3, V3, f64) {
    let a = m.point(m.corner(t, 0));
    let b = m.point(m.corner(t, 1));
    let c = m.point(m.corner(t, 2));
    let n = v::cross(v::sub(b, a), v::sub(c, a));
    let l = v::len(n);
    (if l > 0.0 { v::mul(n, 1.0 / l) } else { [0.0; 3] }, a, l)
}

/// A planar group.
#[derive(Clone, Debug)]
pub struct Group {
    pub tris: Vec<usize>,
    pub n: V3,
    pub d: f64,
    pub face: Option<i32>,
}

/// `planarGroups(mesh, angTol)` → (faceOf, groups).
pub fn planar_groups(m: &Mesh, ang_tol: f64) -> (Vec<usize>, Vec<Group>) {
    if m.tri_face.is_some() {
        return face_groups(m);
    }
    planar_groups_mesh(m, ang_tol)
}

/// `faceGroups(mesh)`: one group per kernel face.
pub fn face_groups(m: &Mesh) -> (Vec<usize>, Vec<Group>) {
    let nt = m.tri_count();
    let mut face_of = vec![0usize; nt];
    let mut by_face: HashMap<i32, usize> = HashMap::new();
    let mut groups: Vec<Group> = Vec::new();
    let tf = m.tri_face.as_deref().unwrap_or(&[]);
    for (t, slot) in face_of.iter_mut().enumerate() {
        let f = tf.get(t).copied().unwrap_or(0);
        let g = match by_face.get(&f) {
            Some(&g) => g,
            None => {
                let (n, a, _) = tri_normal(m, t);
                let g = groups.len();
                by_face.insert(f, g);
                groups.push(Group { tris: Vec::new(), n, d: v::dot(n, a), face: Some(f) });
                g
            }
        };
        if let Some(gr) = groups.get_mut(g) {
            gr.tris.push(t);
        }
        *slot = g;
    }
    (face_of, groups)
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    let mut guard = 0usize;
    while let Some(&p) = parent.get(i) {
        if p == i || guard > parent.len() {
            break;
        }
        let pp = parent.get(p).copied().unwrap_or(p);
        if let Some(x) = parent.get_mut(i) {
            *x = pp;
        }
        i = pp;
        guard = guard.saturating_add(1);
    }
    i
}

/// `planarGroupsMesh(mesh, angTol)`.
pub fn planar_groups_mesh(m: &Mesh, ang_tol: f64) -> (Vec<usize>, Vec<Group>) {
    let nt = m.tri_count();
    let mut size: f64 = 1.0;
    for &p in &m.positions {
        size = js::max(size, p.abs());
    }
    let d_tol = 1e-5 * size;
    let info: Vec<(V3, f64)> = (0..nt)
        .map(|t| {
            let (n, a, _) = tri_normal(m, t);
            (n, v::dot(n, a))
        })
        .collect();
    let mut parent: Vec<usize> = (0..nt).collect();
    let mut edges: HashMap<u64, usize> = HashMap::new();
    for t in 0..nt {
        for k in 0..3usize {
            let a = m.corner(t, k) as u64;
            let b = m.corner(t, k.saturating_add(1) % 3) as u64;
            let key = if a < b { (a << 32) | b } else { (b << 32) | a };
            match edges.get(&key) {
                None => {
                    edges.insert(key, t);
                }
                Some(&o) => {
                    let (an, ad) = info.get(t).copied().unwrap_or(([f64::NAN; 3], f64::NAN));
                    let (bn, bd) = info.get(o).copied().unwrap_or(([f64::NAN; 3], f64::NAN));
                    if v::dot(an, bn) > 1.0 - ang_tol && (ad - bd).abs() < d_tol {
                        let rt = find(&mut parent, t);
                        let ro = find(&mut parent, o);
                        if let Some(x) = parent.get_mut(rt) {
                            *x = ro;
                        }
                    }
                }
            }
        }
    }
    let mut face_of = vec![0usize; nt];
    let mut groups: Vec<Group> = Vec::new();
    let mut map: HashMap<usize, usize> = HashMap::new();
    for t in 0..nt {
        let r = find(&mut parent, t);
        let g = match map.get(&r) {
            Some(&g) => g,
            None => {
                let g = groups.len();
                map.insert(r, g);
                let (n, d) = info.get(t).copied().unwrap_or(([f64::NAN; 3], f64::NAN));
                groups.push(Group { tris: Vec::new(), n, d, face: None });
                g
            }
        };
        if let Some(gr) = groups.get_mut(g) {
            gr.tris.push(t);
        }
        if let Some(x) = face_of.get_mut(t) {
            *x = g;
        }
    }
    (face_of, groups)
}

/// `boundaryLoops(mesh, tris)`: vertex index lists of a triangle set's boundary.
pub fn boundary_loops(m: &Mesh, tris: &[usize]) -> Vec<Vec<usize>> {
    // dir: insertion-ordered map "a>b" → [a, b] with deletions (tombstones keep the order of what is left)
    let mut order: Vec<Option<(usize, usize)>> = Vec::new();
    let mut at: HashMap<(usize, usize), usize> = HashMap::new();
    for &t in tris {
        for k in 0..3usize {
            let a = m.corner(t, k);
            let b = m.corner(t, k.saturating_add(1) % 3);
            if let Some(i) = at.remove(&(b, a)) {
                if let Some(slot) = order.get_mut(i) {
                    *slot = None;
                }
            } else if let std::collections::hash_map::Entry::Vacant(e) = at.entry((a, b)) {
                e.insert(order.len());
                order.push(Some((a, b)));
            } else if let Some(&i) = at.get(&(a, b))
                && let Some(slot) = order.get_mut(i)
            {
                // Map.set on an existing key keeps its place
                *slot = Some((a, b));
            }
        }
    }
    let mut next: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut next_at: HashMap<usize, usize> = HashMap::new();
    for &(a, b) in order.iter().flatten() {
        let i = *next_at.entry(a).or_insert_with(|| {
            next.push((a, Vec::new()));
            next.len().saturating_sub(1)
        });
        if let Some(e) = next.get_mut(i) {
            e.1.push(b);
        }
    }
    let mut loops = Vec::new();
    for si in 0..next.len() {
        let start = next.get(si).map_or(0, |e| e.0);
        while let Some(cur0) = next.get_mut(si).and_then(|e| e.1.pop()) {
            let mut lp = vec![start];
            let mut cur = cur0;
            let mut guard = 0usize;
            while cur != start && guard < 1_000_000 {
                guard = guard.saturating_add(1);
                lp.push(cur);
                let Some(&ni) = next_at.get(&cur) else { break };
                let Some(nx) = next.get_mut(ni).and_then(|e| e.1.pop()) else { break };
                cur = nx;
            }
            if lp.len() >= 3 {
                loops.push(lp);
            }
        }
    }
    loops
}

/// `faceLoops2D(mesh, group)` → (frame, loops (outer first), area).
pub fn face_loops_2d(m: &Mesh, g: &Group) -> (v::Frame, Vec<Vec<[f64; 2]>>, f64) {
    let frame = v::frame_for(g.n, v::mul(g.n, g.d));
    let loops: Vec<Vec<[f64; 2]>> =
        boundary_loops(m, &g.tris).iter().map(|l| l.iter().map(|&vi| v::to_plane(&frame, m.point(vi))).collect()).collect();
    let area = |l: &[[f64; 2]]| -> f64 {
        let n = l.len();
        let mut s = 0.0;
        for i in 0..n {
            let j = if i == 0 { n.saturating_sub(1) } else { i.saturating_sub(1) };
            let (pj, pi) = (l.get(j).copied().unwrap_or([f64::NAN; 2]), l.get(i).copied().unwrap_or([f64::NAN; 2]));
            s += pj[0] * pi[1] - pi[0] * pj[1];
        }
        s / 2.0
    };
    let mut sorted = loops.clone();
    cc_m3d::sort::timsort(&mut sorted, |a, b| area(b).abs() - area(a).abs());
    let total = sorted.iter().fold(0.0, |s, l| s + area(l));
    (frame, sorted.iter().map(|l| drop_collinear(l, 1e-6)).collect(), total)
}

/// `dropCollinear(loop, sinTol)`.
pub fn drop_collinear(lp: &[[f64; 2]], sin_tol: f64) -> Vec<[f64; 2]> {
    let n = lp.len();
    if n <= 3 {
        return lp.to_vec();
    }
    let mut out = Vec::new();
    for i in 0..n {
        let g = |k: usize| lp.get(k.checked_rem(n).unwrap_or(0)).copied().unwrap_or([f64::NAN; 2]);
        let (a, b, c) = (g(i.saturating_add(n).saturating_sub(1)), g(i), g(i.saturating_add(1)));
        let u = [b[0] - a[0], b[1] - a[1]];
        let w = [c[0] - b[0], c[1] - b[1]];
        let l = js::hypot2(u[0], u[1]) * js::hypot2(w[0], w[1]);
        let straight = l > 0.0 && (u[0] * w[1] - u[1] * w[0]).abs() / l < sin_tol && u[0] * w[0] + u[1] * w[1] > 0.0;
        if !straight {
            out.push(b);
        }
    }
    if out.len() >= 3 { out } else { lp.to_vec() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_has_six_faces_each_one_loop() {
        let p = [
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [10.0, 5.0, 0.0],
            [0.0, 5.0, 0.0],
            [0.0, 0.0, 2.0],
            [10.0, 0.0, 2.0],
            [10.0, 5.0, 2.0],
            [0.0, 5.0, 2.0],
        ];
        let indices = vec![0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7, 6, 3, 0, 4, 3, 4, 7];
        let m = Mesh { positions: p.iter().flatten().copied().collect(), indices, tri_face: None };
        let (_, groups) = planar_groups_mesh(&m, 1e-5);
        assert_eq!(groups.len(), 6);
        for g in &groups {
            let (_, loops, area) = face_loops_2d(&m, g);
            assert_eq!(loops.len(), 1);
            assert!(area.abs() > 0.0);
        }
        // hostile: indices past the positions, a lone index
        let bad = Mesh { positions: vec![0.0; 3], indices: vec![0, 7, 9, 4], tri_face: Some(vec![]) };
        let _ = planar_groups(&bad, 1e-5);
        let _ = boundary_loops(&bad, &[0, 5]);
    }
}
