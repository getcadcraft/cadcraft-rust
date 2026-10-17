//! drawing.js `meshSection` and the section branch of `meshViews` (0.4 K.3): the solid cut by the plane square to a
//! view, the viewer's side trimmed away with the solid library (`trimByPlane`), the rest projected like the other views;
//! `cut` = the outline of the facets lying in the cutting plane, `trace` = where the plane shows edge-on in the
//! neighbouring view. Same calls in the same order as the JS (they number meshes in the same solid library).

use crate::drawing::{self, DMesh, Seg, View};
use crate::js;
use crate::jv;
use crate::vec::{self as v, V3};
use cc_manifold::{Manifold, Mesh, Status};
use serde_json::Value;
use std::collections::HashMap;

/// manifold-3d's `ManifoldError` message for a status (what `new Manifold(mesh)` throws).
pub fn manifold_error(st: Status) -> String {
    match st {
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
    .to_string()
}

/// `new M.Mesh({ numProp: 3, vertProperties: Float32Array.from(P), triVerts: Uint32Array.from(I) })`, merged, then
/// `new M.Manifold(gl)` (throws on a bad mesh).
pub fn solid_of(positions: &[f64], indices: &[u32]) -> Result<Manifold, String> {
    #[allow(clippy::cast_possible_truncation)]
    let vp: Vec<f32> = positions.iter().map(|&x| x as f32).collect();
    let mut gl = Mesh::new(3, vp, indices.to_vec());
    gl.merge();
    Manifold::from_mesh(&gl).map_err(manifold_error)
}

fn trace_view(of: &str) -> Option<&'static str> {
    match of {
        "front" => Some("top"),
        "top" => Some("front"),
        "right" => Some("front"),
        _ => None,
    }
}

/// `meshSection(M, mesh, views, sec)`: the section view, or None when nothing is left behind the plane.
pub fn mesh_section(mesh: &DMesh, views: &[(String, View)], of: &str, at: Option<f64>) -> Result<Option<View>, String> {
    let Some(view) = drawing::view_dir(of) else { return Err("Cannot read properties of undefined (reading 'dir')".into()) };
    let p = &mesh.positions;
    let mut mn = [f64::INFINITY; 3];
    let mut mx = [f64::NEG_INFINITY; 3];
    for c in p.chunks(3) {
        for (k, &x) in c.iter().enumerate() {
            if let (Some(l), Some(h)) = (mn.get_mut(k), mx.get_mut(k)) {
                *l = js::min(*l, x);
                *h = js::max(*h, x);
            }
        }
    }
    let centre = v::mul(v::add(mn, mx), 0.5);
    let size = js::max(1e-6, v::len(v::sub(mx, mn)));
    let at = match at {
        Some(a) if a.is_finite() => a,
        _ => v::dot(centre, view.dir),
    };
    let o = v::add(v::sub(centre, v::mul(view.dir, v::dot(centre, view.dir))), v::mul(view.dir, at));
    let whole = solid_of(p, &mesh.indices)?;
    let rest = whole.trim_by_plane(v::mul(view.dir, -1.0), -at);
    if rest.is_empty() {
        return Ok(None);
    }
    let g = rest.get_mesh();
    let np = usize::try_from(g.num_prop).unwrap_or(3).max(1);
    let nv = g.vert_properties.len().checked_div(np).unwrap_or(0);
    let mut pos = Vec::with_capacity(nv.saturating_mul(3));
    for i in 0..nv {
        for k in 0..3 {
            pos.push(f64::from(g.vert_properties.get(i.saturating_mul(np).saturating_add(k)).copied().unwrap_or(f32::NAN)));
        }
    }
    let cut_mesh = DMesh { positions: pos, indices: g.tri_verts.clone() };
    let (edges, normals) = drawing::mesh_edges(&cut_mesh);
    let vw = drawing::project_view(&cut_mesh, &edges, &normals, &view, false);
    let tol = 1e-6 * size;
    let ii = &cut_mesh.indices;
    let pt = |i: u32| -> V3 {
        let i = i as usize;
        let g = |k: usize| cut_mesh.positions.get(i.saturating_mul(3).saturating_add(k)).copied().unwrap_or(f64::NAN);
        [g(0), g(1), g(2)]
    };
    // undirected key → (count, the edge as its facet runs it), in first-seen order (JS Maps)
    let mut order: Vec<(u32, u32)> = Vec::new();
    let mut use_: HashMap<(u32, u32), (usize, (u32, u32))> = HashMap::new();
    for tri in ii.chunks(3) {
        let [ia, ib, ic] = [tri.first().copied().unwrap_or(0), tri.get(1).copied().unwrap_or(0), tri.get(2).copied().unwrap_or(0)];
        let (a, b, c) = (pt(ia), pt(ib), pt(ic));
        if [a, b, c].iter().any(|&q| (v::dot(q, view.dir) - at).abs() > tol) {
            continue;
        }
        let nrm = v::cross(v::sub(b, a), v::sub(c, a));
        if v::dot(nrm, view.dir) <= 0.0 {
            continue;
        }
        for (p0, q0) in [(ia, ib), (ib, ic), (ic, ia)] {
            let key = if p0 < q0 { (p0, q0) } else { (q0, p0) };
            match use_.get_mut(&key) {
                Some(e) => {
                    e.0 = e.0.saturating_add(1);
                    e.1 = (p0, q0);
                }
                None => {
                    order.push(key);
                    use_.insert(key, (1, (p0, q0)));
                }
            }
        }
    }
    let mut cut: Vec<Seg> = Vec::new();
    for key in &order {
        let Some(&(n, (p0, q0))) = use_.get(key) else { continue };
        if n != 1 {
            continue;
        }
        let (a, b) = (pt(p0), pt(q0));
        cut.push([v::dot(a, view.right), v::dot(a, view.up), v::dot(b, view.right), v::dot(b, view.up)]);
    }
    let (mut u0, mut v0, mut u1, mut v1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for s in vw.visible.iter().chain(cut.iter()) {
        u0 = js::min(u0, js::min(s[0], s[2]));
        u1 = js::max(u1, js::max(s[0], s[2]));
        v0 = js::min(v0, js::min(s[1], s[3]));
        v1 = js::max(v1, js::max(s[1], s[3]));
    }
    let Some(tvk) = trace_view(of) else { return Err("Cannot read properties of undefined (reading 'dir')".into()) };
    let Some(tv) = drawing::view_dir(tvk) else { return Err("Cannot read properties of undefined (reading 'dir')".into()) };
    let along = v::norm(v::cross(view.dir, tv.dir));
    let tbox = views.iter().find(|(k, _)| k == tvk).map(|(_, vv)| {
        jv::obj(vec![("u0", jv::num(vv.bx[0])), ("v0", jv::num(vv.bx[1])), ("u1", jv::num(vv.bx[2])), ("v1", jv::num(vv.bx[3]))])
    });
    let neg = v::mul(view.dir, -1.0);
    let trace = jv::obj(vec![
        ("view", tvk.into()),
        ("p", jv::nums(&[v::dot(o, tv.right), v::dot(o, tv.up)])),
        ("d", jv::nums(&[v::dot(along, tv.right), v::dot(along, tv.up)])),
        ("sight", jv::nums(&[v::dot(neg, tv.right), v::dot(neg, tv.up)])),
        ("box", tbox.unwrap_or(Value::Null)),
    ]);
    let bx = if u0 == f64::INFINITY { [0.0, 0.0, 1.0, 1.0] } else { [u0, v0, u1, v1] };
    Ok(Some(View {
        visible: vw.visible,
        hidden: Vec::new(),
        bx,
        cut: Some(cut),
        trace: Some(trace),
        of: Some(of.to_string()),
        at: Some(at),
    }))
}

/// `meshViews(mesh, { iso, M, section })`: the four views, and the section view when asked for.
pub fn mesh_views_section(mesh: &DMesh, iso: bool, section: Option<(&str, Option<f64>)>) -> Result<Vec<(String, View)>, String> {
    let mut views = drawing::mesh_views(mesh, iso);
    if let Some((of, at)) = section
        && drawing::view_dir(of).is_some()
        && let Some(s) = mesh_section(mesh, &views, of, at)?
    {
        views.push(("section".into(), s));
    }
    Ok(views)
}
