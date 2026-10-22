//! boolean3d.js: union, subtract and intersect solids on the 3D tab (0.4 C12) through cc-manifold (the same results as
//! the manifold-3d wasm the JS loads). Solids are welded and moved near their common middle before Manifold sees them
//! (its vertices are 32-bit floats), and back after on the 1 µm grid of the drawing; the result is one group whose
//! coplanar triangles read as one face (meshDef's soft edges).
//!
//! Handle lifetimes follow the JS: the inputs and the result live until the model has changed (its `finally` deletes
//! them), and the two batch unions a subtraction makes are never deleted there — they stay until the end too (the lazy
//! CSG tree only folds a node no other handle holds, so this decides the result).

use cc_m3d::Model;
use cc_m3d::def::face_tris;
use cc_m3d::meshin::{PolyIn, mesh_def_in};
use cc_m3d::model::place;
use cc_m3d::vec::{M4, P3, apply, mmul, translation};
use cc_manifold::{Manifold, Mesh, Status};

/// The catalog's words with fills (the engine's `tv`).
pub type Tv<'a> = &'a dyn Fn(&str, &[(&str, String)]) -> String;

/// Most triangles one solid may bring (the JS runs out of memory long before).
const MAX_TRIS: usize = 8_000_000;
/// Most definitions visited for one solid (definitions placed in each other multiply).
const MAX_VISITS: u64 = 4_000_000;

/// `solidTriangles(model, iid)`: every triangle of a root instance (its definition and the instances inside it), in
/// meters. CadCraft 0.5's copy of the kernel triangulates every face with earcut.
pub fn solid_triangles(model: &Model, iid: f64) -> Vec<[P3; 3]> {
    let mut out = Vec::new();
    let Some(inst) = model.root().and_then(|r| r.insts.get(iid)) else { return out };
    // the JS recursion, depth first in order: a stack of pending (def, matrix, depth), children pushed in reverse
    let mut stack: Vec<(String, M4, u32)> = vec![(inst.def.clone(), inst.m, 0)];
    let mut visits = 0u64;
    cc_m3d::def::with_plain_face_tris(|| {
        while let Some((did, m, depth)) = stack.pop() {
            let Some(d) = model.def(&did) else { continue };
            if depth > 32 {
                continue;
            }
            visits = visits.saturating_add(1);
            if visits > MAX_VISITS || out.len() > MAX_TRIS {
                break;
            }
            for f in d.faces.values() {
                for t in face_tris(d, f) {
                    out.push([apply(&m, d.v(t[0])), apply(&m, d.v(t[1])), apply(&m, d.v(t[2]))]);
                }
            }
            let subs: Vec<(String, M4)> = d.insts.values().map(|s| (s.def.clone(), mmul(&m, &s.m))).collect();
            for (sd, sm) in subs.into_iter().rev() {
                stack.push((sd, sm, depth.saturating_add(1)));
            }
        }
    });
    out
}

/// `toManifold(M, tris, o, label)`: triangles (meters) → a Manifold near `o`; refused when they do not close a volume.
fn to_manifold(tris: &[[P3; 3]], o: P3, label: &str, tv: Tv<'_>) -> Result<Manifold, String> {
    let mut key: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut verts: Vec<f64> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let mut vid = |p: P3, verts: &mut Vec<f64>| -> u32 {
        let q = [p[0] - o[0], p[1] - o[1], p[2] - o[2]];
        let mut k = String::new();
        for (i, x) in q.iter().enumerate() {
            if i > 0 {
                k.push(',');
            }
            cc_m3d::js::push_num(&mut k, cc_m3d::js::round(x * 1e7));
        }
        if let Some(&i) = key.get(&k) {
            return i;
        }
        let i = u32::try_from(verts.len() / 3).unwrap_or(u32::MAX);
        key.insert(k, i);
        verts.extend_from_slice(&q);
        i
    };
    for t in tris {
        let a = vid(t[0], &mut verts);
        let b = vid(t[1], &mut verts);
        let c = vid(t[2], &mut verts);
        if a != b && b != c && a != c {
            idx.extend_from_slice(&[a, b, c]);
        }
    }
    let mut mesh = Mesh::new(3, verts.iter().map(|&x| cc_io_m3d::stl::f32_of(x)).collect(), idx);
    mesh.merge();
    let man = Manifold::from_mesh_unchecked(&mesh);
    // manifold-3d's `new Manifold(mesh)` throws a ManifoldError on a bad status (its message is what the JS prints)
    if man.status() != Status::NoError {
        return Err(manifold_error(man.status()).into());
    }
    if man.is_empty() {
        return Err(tv("cmd.bool.open", &[("name", label.to_string())]));
    }
    Ok(man)
}

/// manifold-3d's `ManifoldError` messages.
fn manifold_error(s: Status) -> &'static str {
    match s {
        Status::NonFiniteVertex => "Non-finite vertex",
        Status::NotManifold => "Not manifold",
        Status::VertexOutOfBounds => "Vertex index out of bounds",
        Status::PropertiesWrongLength => "Properties have wrong length",
        Status::MissingPositionProperties => "Less than three properties",
        Status::MergeVectorsDifferentLengths => "Merge vectors have different lengths",
        Status::MergeIndexOutOfBounds => "Merge index out of bounds",
        Status::TransformWrongLength => "Transform vector has wrong length",
        Status::RunIndexWrongLength => "Run index vector has wrong length",
        Status::FaceIDWrongLength => "Face ID vector has wrong length",
        Status::InvalidConstruction => "Manifold constructed with invalid parameters",
        Status::ResultTooLarge => "Result exceeds maximum size",
        Status::InvalidTangents => "Invalid halfedge tangents",
        _ => "Unknown error",
    }
}

/// `name.replace(/\s*\(\d+\)$/, '')`
fn strip_count(s: &str) -> String {
    let Some(body) = s.strip_suffix(')') else { return s.to_string() };
    let digits = body.chars().rev().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return s.to_string();
    }
    let cut = body.len().saturating_sub(digits);
    match body.get(..cut).and_then(|r| r.strip_suffix('(')) {
        Some(rest) => rest.trim_end_matches(cc_cad::js::is_ws).to_string(),
        None => s.to_string(),
    }
}

/// `booleanSolids(M, model, op, a, b)`: op "union" | "subtract" | "intersect"; a, b = root instance ids (b: the ones
/// taken away, for subtract; for the others a holds every solid and b is empty). One new solid in place of all of
/// them, in one change of the model (one undo step) → its instance id, or the plain message why not.
pub fn boolean_solids(model: &mut Model, op: &str, a: &[f64], b: &[f64], tv: Tv<'_>) -> Result<f64, String> {
    let all: Vec<f64> = a.iter().chain(b.iter()).copied().collect();
    let refuse = if op == "subtract" { a.is_empty() || b.is_empty() } else { all.len() < 2 };
    if refuse {
        return Err(tv(if op == "subtract" { "cmd.bool.need.subtract" } else { "cmd.bool.need.two" }, &[]));
    }
    // a Map: an instance named twice is one entry (the first place, the last value)
    let mut tris: Vec<(u64, Vec<[P3; 3]>)> = Vec::new();
    for &iid in &all {
        let v = solid_triangles(model, iid);
        match tris.iter_mut().find(|(k, _)| *k == iid.to_bits()) {
            Some(slot) => slot.1 = v,
            None => tris.push((iid.to_bits(), v)),
        }
    }
    // the origin: the middle of everything, so the 32-bit vertices keep their precision
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for (_, ts) in &tris {
        for t in ts {
            for p in t {
                for k in 0..3usize {
                    if let (Some(l), Some(h), Some(&v)) = (lo.get_mut(k), hi.get_mut(k), p.get(k)) {
                        *l = cc_m3d::js::min(*l, v);
                        *h = cc_m3d::js::max(*h, v);
                    }
                }
            }
        }
    }
    let o = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
    let names: Vec<(f64, String)> = model
        .root()
        .map(|r| {
            r.insts
                .iter()
                .map(|(iid, inst)| {
                    (iid, model.def(&inst.def).map(|d| d.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| "Solid".into()))
                })
                .collect()
        })
        .unwrap_or_default();
    let name_of = |iid: f64| names.iter().find(|(i, _)| *i == iid).map(|(_, n)| n.clone());
    let tris_of = |iid: f64| tris.iter().find(|(k, _)| *k == iid.to_bits()).map_or(&[][..], |(_, v)| v.as_slice());
    let label_of = |iid: f64| name_of(iid).unwrap_or_else(|| tv("cmd.bool.asolid", &[]));
    let mut a_list: Vec<Manifold> = Vec::new();
    for &iid in a {
        a_list.push(to_manifold(tris_of(iid), o, &label_of(iid), tv)?);
    }
    let mut b_list: Vec<Manifold> = Vec::new();
    for &iid in b {
        b_list.push(to_manifold(tris_of(iid), o, &label_of(iid), tv)?);
    }
    let mut temps: Vec<Manifold> = Vec::new();
    let r = match op {
        "union" => Manifold::union(&a_list),
        "intersect" => Manifold::intersection(&a_list),
        _ => {
            let ua = Manifold::union(&a_list);
            let ub = Manifold::union(&b_list);
            let r = ua.subtract(&ub);
            temps.push(ua);
            temps.push(ub);
            r
        }
    };
    if r.is_empty() {
        return Err(tv(if op == "intersect" { "cmd.bool.empty.intersect" } else { "cmd.bool.empty" }, &[]));
    }
    let g = r.get_mesh();
    // back from 32-bit floats: each corner on the 1 µm grid of the drawing (exact sizes stay exact), relative to o again
    let np = usize::try_from(g.num_prop).unwrap_or(3);
    let r6 = |v: f32, k: usize| {
        let ok = o.get(k).copied().unwrap_or(f64::NAN);
        cc_m3d::js::round((ok + f64::from(v)) * 1e6) / 1e6 - ok
    };
    let vp = |i: u32, k: usize| -> f32 {
        let i = usize::try_from(i).unwrap_or(usize::MAX);
        g.vert_properties.get(i.saturating_mul(np).saturating_add(k)).copied().unwrap_or(f32::NAN)
    };
    let pt = |i: u32| -> P3 { [r6(vp(i, 0), 0), r6(vp(i, 1), 1), r6(vp(i, 2), 2)] };
    let polys: Vec<PolyIn> = g.tri_verts.as_chunks::<3>().0.iter().map(|&[x, y, z]| PolyIn::outline(vec![pt(x), pt(y), pt(z)])).collect();
    let label = match op {
        "union" => "UNION",
        "subtract" => "SUBTRACT",
        _ => "INTERSECT",
    };
    let base = a.first().and_then(|i| name_of(*i)).unwrap_or_else(|| "Solid".into());
    let all_c = all.clone();
    let iid = model
        .change(label, |m| {
            let uname = m.unique_name(&strip_count(&base));
            let nd = mesh_def_in(m, &polys, Some(&uname), "group", &[]);
            let Some(root) = m.root_mut() else { return Err(cc_m3d::Error::Type) };
            for id in &all_c {
                root.insts.delete(*id);
            }
            Ok(place(root, &nd, &translation(o)))
        })
        .map_err(|_| tv("cmd.bool.empty", &[]))?;
    drop(r);
    drop(a_list);
    drop(b_list);
    drop(temps);
    Ok(iid)
}

#[cfg(test)]
mod tests {
    use super::strip_count;
    #[test]
    fn names() {
        assert_eq!(strip_count("Box (2)"), "Box");
        assert_eq!(strip_count("Box(12)"), "Box");
        assert_eq!(strip_count("Box"), "Box");
        assert_eq!(strip_count("Box ()"), "Box ()");
    }
}
