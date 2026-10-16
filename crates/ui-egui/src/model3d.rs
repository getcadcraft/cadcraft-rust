//! A ModelCraft model (cc-m3d) as a 3D view scene, built the way ModelCraft's view3d.js builds its three.js scene: one
//! geometry per definition and placement colour (a painted group fills its faces that have none), faces in their paint
//! or the front colour, smooth normals across soft edges, hidden faces and soft or hidden edges left out, every
//! placement through `walk_shown` (hidden placements and hidden tags skipped).

use std::collections::HashMap;

use cc_m3d::def::{Def, face_normal, face_tris};
use cc_m3d::model::Model;
use cc_render::mesh3d::{IDENT, Instance3, MeshData};

use crate::view3d::Scene3D;

/// The front colour of an unpainted face (view3d.js: paper '#f2efe7', model '#c9c3b5').
pub const FRONT_PAPER: [u8; 4] = [0xf2, 0xef, 0xe7, 255];
pub const FRONT_MODEL: [u8; 4] = [0xc9, 0xc3, 0xb5, 255];
/// The back colour (view3d.js: paper '#a9bacb', model '#5d7186').
pub const BACK_PAPER: [f32; 3] = [0xa9 as f32 / 255.0, 0xba as f32 / 255.0, 0xcb as f32 / 255.0];
pub const BACK_MODEL: [f32; 3] = [0x5d as f32 / 255.0, 0x71 as f32 / 255.0, 0x86 as f32 / 255.0];

fn hex_rgba(s: &str) -> Option<[u8; 4]> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8, 255])
}

/// A paint (`#rrggbb`, `mat:<id>`, `img:<id>|…`) → its colour, as `paintHex` without a texture library.
fn paint_rgba(c: &str) -> Option<[u8; 4]> {
    let hex = cc_m3d::paint::paint_hex(c, &|_| None, &|_| None);
    hex_rgba(&hex)
}

fn grow(lo: &mut [f64; 3], hi: &mut [f64; 3], p: [f64; 3]) {
    for ((l, h), v) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
        *l = l.min(v);
        *h = h.max(v);
    }
}

fn rel(p: [f64; 3], o: [f64; 3]) -> [f32; 3] {
    [(p[0] - o[0]) as f32, (p[1] - o[1]) as f32, (p[2] - o[2]) as f32]
}

/// One definition's faces and edges as a mesh; faces without paint take `dflt` (a placement's paint) or `front`.
pub fn def_mesh(def: &Def, dflt: Option<&str>, front: [u8; 4]) -> MeshData {
    let fill = dflt.and_then(paint_rgba).unwrap_or(front);
    let smooth = cc_m3d::edges::smooth_normals(def);
    // the definition's own origin: the middle of its vertices' box (positions are f32 relative to it)
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for (_, p) in def.verts.iter() {
        grow(&mut lo, &mut hi, *p);
    }
    let origin = if lo[0].is_finite() { [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0] } else { [0.0; 3] };
    let mut m = MeshData { origin, ..MeshData::default() };
    for (fid, f) in def.faces.iter() {
        if f.hidden {
            continue;
        }
        let n = face_normal(def, f);
        let c = f.color.as_deref().filter(|c| !c.is_empty()).and_then(paint_rgba).unwrap_or(fill);
        // a face's corners are shared by its triangles (same normal and colour): each is stored once
        let mut at: HashMap<u64, u32> = HashMap::new();
        for t in face_tris(def, f) {
            for v in t {
                let j = match at.get(&v.to_bits()) {
                    Some(j) => *j,
                    None => {
                        let j = m.positions.len() as u32;
                        at.insert(v.to_bits(), j);
                        let mut key = cc_m3d::js::num_str(fid);
                        key.push('|');
                        cc_m3d::js::push_num(&mut key, v);
                        let q = smooth.get(&key).copied().unwrap_or(n);
                        m.positions.push(rel(def.v(v), origin));
                        m.normals.push([q[0] as f32, q[1] as f32, q[2] as f32]);
                        m.colors.push(c);
                        j
                    }
                };
                m.indices.push(j);
            }
        }
    }
    for (_, e) in def.edges.iter() {
        if e.soft || e.hidden {
            continue;
        }
        m.edges.push(rel(def.v(e.a), origin));
        m.edges.push(rel(def.v(e.b), origin));
    }
    m
}

/// The whole model as a scene: one mesh per (definition, placement colour), one instance per placement shown.
pub fn scene_of(model: &Model, id: u64, dark: bool) -> Scene3D {
    let front = if dark { FRONT_MODEL } else { FRONT_PAPER };
    let mut meshes: Vec<MeshData> = Vec::new();
    let mut index: HashMap<(String, String), usize> = HashMap::new();
    let mut instances: Vec<Instance3> = Vec::new();
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    model.walk_shown("root", &IDENT, &mut |def: &Def, m: &[f64; 16], _path: &[f64], color: Option<&str>| {
        if def.faces.is_empty() && def.edges.is_empty() {
            return true;
        }
        let key = (def.id.clone(), color.unwrap_or("").to_string());
        let mi = *index.entry(key).or_insert_with(|| {
            meshes.push(def_mesh(def, color, front));
            meshes.len() - 1
        });
        for (_, p) in def.verts.iter() {
            let w = [
                m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
                m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
                m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
            ];
            grow(&mut lo, &mut hi, w);
        }
        instances.push(Instance3 { mesh: mi, m: *m, tint: None, hidden: false });
        true
    });
    if !lo[0].is_finite() {
        // an empty model: a 10 m square around the origin to look at
        lo = [-5.0, -5.0, 0.0];
        hi = [5.0, 5.0, 3.0];
    }
    Scene3D::new(id, meshes, instances, lo, hi)
}

/// A sample model made in code (no file): a small building with a door and two windows pushed in, a porch slab and a
/// row of posts placed as one component — what `view.model.sample` and the web build's `?model` open.
pub fn sample_model() -> Model {
    use cc_m3d::ops::{add_polygon, push_pull, rect_pts};
    let mut model = Model::new();
    let rect = |d: &mut Def, o: [f64; 3], u: [f64; 3], v: [f64; 3], w: f64, h: f64| {
        let _ = add_polygon(d, &rect_pts(o, u, v, w, h), None);
    };
    let face_at = |d: &Def, p: [f64; 3], n: [f64; 3]| -> Option<f64> {
        // the face whose plane holds p and whose normal is n
        d.faces.iter().find_map(|(id, f)| {
            let fnrm = face_normal(d, f);
            let q = d.v(*f.loops.first()?.first()?);
            let on = ((p[0] - q[0]) * fnrm[0] + (p[1] - q[1]) * fnrm[1] + (p[2] - q[2]) * fnrm[2]).abs() < 1e-6;
            let same = (fnrm[0] - n[0]).abs() + (fnrm[1] - n[1]).abs() + (fnrm[2] - n[2]).abs() < 1e-6;
            (on && same && cc_m3d::def::face_area(d, f) < 5.0).then_some(id)
        })
    };
    let _ = model.change("Sample", |m: &mut Model| {
        let d = m.root_mut().ok_or(cc_m3d::error::Error::Type)?;
        // the building: 8 × 6 m, 3 m high
        rect(d, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 8.0, 6.0);
        let first = d.faces.keys().next();
        if let Some(f) = first {
            let _ = push_pull(d, f, 3.0);
        }
        // a door and two windows on the front (y = 0), each pushed 0.25 m in
        for (x, z, w, h) in [(3.5, 0.15, 1.0, 2.1), (1.0, 1.0, 1.4, 1.2), (5.6, 1.0, 1.4, 1.2)] {
            rect(d, [x, 0.0, z], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], w, h);
            if let Some(f) = face_at(d, [x + w / 2.0, 0.0, z + h / 2.0], [0.0, -1.0, 0.0]) {
                let _ = push_pull(d, f, -0.25);
            }
        }
        // a porch slab in front
        rect(d, [2.0, -2.5, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 4.0, 2.5);
        Ok(())
    });
    // a post as a component placed four times along the porch
    let post = model.new_def("component", Some("Post"));
    let _ = model.change("Posts", |m: &mut Model| {
        if let Some(d) = m.defs.get_mut(&post) {
            rect(d, [-0.08, -0.08, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0.16, 0.16);
            let first = d.faces.keys().next();
            if let Some(f) = first {
                let _ = push_pull(d, f, 2.6);
            }
        }
        if let Some(root) = m.root_mut() {
            for x in [2.2, 3.4, 4.6, 5.8] {
                cc_m3d::model::place(root, &post, &cc_m3d::vec::translation([x, -2.3, 0.0]));
            }
        }
        Ok(())
    });
    model
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pushed_box_becomes_one_mesh_with_its_edges() {
        let mut model = Model::new();
        let r = model.change("box", |m: &mut Model| {
            let d = m.root_mut().ok_or(cc_m3d::error::Error::Type)?;
            cc_m3d::ops::add_polygon(d, &[[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 1.0, 0.0], [0.0, 1.0, 0.0]], None)?;
            let first = d.faces.keys().next();
            if let Some(fid) = first {
                let _ = cc_m3d::ops::push_pull(d, fid, 1.5);
            }
            Ok(())
        });
        assert!(r.is_ok(), "{r:?}");
        let s = scene_of(&model, 1, false);
        assert_eq!(s.instances.len(), 1);
        let m = &s.meshes[0];
        assert_eq!(m.indices.len() / 3, 12, "six faces, two triangles each");
        assert_eq!(m.edges.len() / 2, 12, "twelve edges");
        assert!((s.max[2] - 1.5).abs() < 1e-9 && s.min[0].abs() < 1e-9);
        assert!(m.colors.iter().all(|c| *c == FRONT_PAPER));
    }

    #[test]
    fn the_sample_model_has_a_building_and_four_posts() {
        let model = sample_model();
        let s = scene_of(&model, 1, false);
        assert_eq!(s.instances.len(), 5, "the root and four posts");
        assert_eq!(s.meshes.len(), 2, "one mesh per definition: the posts share theirs");
        assert!(s.meshes[0].indices.len() / 3 > 12, "openings pushed in");
    }
}
