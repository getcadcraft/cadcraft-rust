//! An IFC model (cc-ifc) in the 3D view, the way BimCraft's viewer shows it: each stored mesh once per colour, placed
//! by every piece that uses it (instancing), feature edges where faces meet at more than 30° or a triangle edge has no
//! neighbour; IfcSpace and openings are left out by cc-ifc's product list.

use std::collections::HashMap;
use std::sync::Arc;

use cc_io_ifc::view::{IfcView, ViewMesh};
use cc_render::mesh3d::{Instance3, MeshData};

use crate::view3d::Scene3D;

/// A welded edge: its first raw corner pair and the normals of the triangles on it.
type EdgeFaces = ([usize; 2], Vec<[f64; 3]>);

fn rgba(c: [f64; 4]) -> [u8; 4] {
    let b = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [b(c[0]), b(c[1]), b(c[2]), 255]
}

/// Feature edges of a mesh: welded by position (1 µm), an edge with one triangle or two meeting at more than 30°.
fn feature_edges(m: &ViewMesh) -> Vec<[usize; 2]> {
    let pos = m.pos.as_chunks::<3>().0;
    let q = |i: usize| -> Option<[i64; 3]> {
        let p = pos.get(i)?;
        Some([(p[0] * 1e6).round() as i64, (p[1] * 1e6).round() as i64, (p[2] * 1e6).round() as i64])
    };
    let mut weld: HashMap<[i64; 3], usize> = HashMap::new();
    let mut id = |i: usize| -> Option<usize> {
        let k = q(i)?;
        let n = weld.len();
        Some(*weld.entry(k).or_insert(n))
    };
    let tri_normal = |a: usize, b: usize, c: usize| -> Option<[f64; 3]> {
        let p = |i: usize| pos.get(i).copied();
        let (pa, pb, pc) = (p(a)?, p(b)?, p(c)?);
        let u = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
        let v = [pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2]];
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        (l > 1e-18).then(|| [n[0] / l, n[1] / l, n[2] / l])
    };
    // welded edge → (first raw corner pair, normals of the triangles on it)
    let mut edges: HashMap<(usize, usize), EdgeFaces> = HashMap::new();
    for t in m.idx.as_chunks::<3>().0 {
        let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
        let Some(n) = tri_normal(a, b, c) else { continue };
        for (x, y) in [(a, b), (b, c), (c, a)] {
            let (Some(wx), Some(wy)) = (id(x), id(y)) else { continue };
            if wx == wy {
                continue;
            }
            let key = (wx.min(wy), wx.max(wy));
            edges.entry(key).or_insert_with(|| ([x, y], Vec::new())).1.push(n);
        }
    }
    let cos30 = 30f64.to_radians().cos();
    let mut out: Vec<[usize; 2]> = edges
        .into_values()
        .filter(|(_, ns)| match ns.as_slice() {
            [a, b] => a[0] * b[0] + a[1] * b[1] + a[2] * b[2] < cos30,
            _ => true,
        })
        .map(|(e, _)| e)
        .collect();
    out.sort_unstable();
    out
}

fn mesh_data(m: &ViewMesh, color: [u8; 4], edges: &[[usize; 2]]) -> MeshData {
    let n = m.pos.len() / 3;
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let pos = m.pos.as_chunks::<3>().0;
    let nrm = m.nrm.as_chunks::<3>().0;
    for p in pos {
        for ((l, h), v) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
            *l = l.min(*v);
            *h = h.max(*v);
        }
    }
    let o = if lo[0].is_finite() { [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0] } else { [0.0; 3] };
    let rel =
        |i: usize| -> [f32; 3] { pos.get(i).map_or([0.0; 3], |p| [(p[0] - o[0]) as f32, (p[1] - o[1]) as f32, (p[2] - o[2]) as f32]) };
    let mut d = MeshData { origin: o, ..MeshData::default() };
    for i in 0..n {
        d.positions.push(rel(i));
        let nr = nrm.get(i).map_or([0.0, 0.0, 1.0], |v| [v[0] as f32, v[1] as f32, v[2] as f32]);
        d.normals.push(nr);
        d.colors.push(color);
    }
    d.indices = m.idx.iter().copied().filter(|&i| (i as usize) < n).collect();
    d.indices.truncate(d.indices.len() / 3 * 3);
    for e in edges {
        d.edges.push(rel(e[0]));
        d.edges.push(rel(e[1]));
    }
    d
}

/// The scene of an IFC view; `max_edge_tris`: above this many triangles in total no edges are drawn (BimCraft: 1.5 M).
pub fn scene_of(v: &IfcView, id: u64, max_edge_tris: usize) -> Scene3D {
    let total: usize = v.pieces.iter().filter_map(|p| v.meshes.get(p.mesh)).map(|m| m.idx.len() / 3).sum();
    let with_edges = total <= max_edge_tris;
    let mut edge_cache: HashMap<usize, Vec<[usize; 2]>> = HashMap::new();
    let mut index: HashMap<(usize, [u8; 4]), usize> = HashMap::new();
    let mut meshes: Vec<MeshData> = Vec::new();
    let mut instances: Vec<Instance3> = Vec::new();
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in &v.pieces {
        let Some(m) = v.meshes.get(p.mesh) else { continue };
        let c = rgba(p.color);
        let mi = *index.entry((p.mesh, c)).or_insert_with(|| {
            let edges = if with_edges { edge_cache.entry(p.mesh).or_insert_with(|| feature_edges(m)).clone() } else { Vec::new() };
            meshes.push(mesh_data(m, c, &edges));
            meshes.len() - 1
        });
        let t = &p.matrix;
        for q in m.pos.as_chunks::<3>().0 {
            let w = [
                t[0] * q[0] + t[4] * q[1] + t[8] * q[2] + t[12],
                t[1] * q[0] + t[5] * q[1] + t[9] * q[2] + t[13],
                t[2] * q[0] + t[6] * q[1] + t[10] * q[2] + t[14],
            ];
            for ((l, h), x) in lo.iter_mut().zip(hi.iter_mut()).zip(w) {
                *l = l.min(x);
                *h = h.max(x);
            }
        }
        instances.push(Instance3 { mesh: mi, m: p.matrix, tint: None });
    }
    if !lo[0].is_finite() {
        lo = [-5.0, -5.0, 0.0];
        hi = [5.0, 5.0, 3.0];
    }
    Scene3D { id, meshes: Arc::new(meshes), instances: Arc::new(instances), min: lo, max: hi }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_has_twelve_feature_edges() {
        // two triangles per side, 24 corners (not welded, as cc-ifc stores flat-shaded boxes)
        let mut pos = Vec::new();
        let mut idx = Vec::new();
        let quads: [[[f64; 3]; 4]; 6] = [
            [[0., 0., 0.], [0., 1., 0.], [1., 1., 0.], [1., 0., 0.]],
            [[0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]],
            [[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]],
            [[0., 1., 0.], [0., 1., 1.], [1., 1., 1.], [1., 1., 0.]],
            [[0., 0., 0.], [0., 0., 1.], [0., 1., 1.], [0., 1., 0.]],
            [[1., 0., 0.], [1., 1., 0.], [1., 1., 1.], [1., 0., 1.]],
        ];
        for q in quads {
            let b = (pos.len() / 3) as u32;
            for p in q {
                pos.extend_from_slice(&p);
            }
            idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
        }
        let m = ViewMesh { nrm: vec![0.0; pos.len()], pos, idx };
        assert_eq!(feature_edges(&m).len(), 12, "the diagonals of the sides are not edges");
    }
}
