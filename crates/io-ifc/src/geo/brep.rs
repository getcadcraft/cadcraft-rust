//! geo-brep.js — boundary and tessellated representations as meshes (IFC4 ADD2 TC1 §8.8, §8.19 Topology resource):
//! IfcFacetedBrep (+ WithVoids), IfcClosedShell / IfcOpenShell / IfcConnectedFaceSet of IfcFace with IfcPolyLoop or
//! IfcEdgeLoop bounds, IfcShellBasedSurfaceModel, IfcFaceBasedSurfaceModel, IfcPolygonalFaceSet, IfcTriangulatedFaceSet
//! (+ Normals, PnIndex; IfcTriangulatedIrregularNetwork), IfcBoundingBox. Faces on curved surfaces go to nurbs.rs.

use super::ctx::{GR, GeoCtx, arr, bool_, idx, list_or_empty, numd, refs, type_error_read};
use super::math::{V3, len, newell, sub, translation};
use super::mesh::Mesh;
use super::nurbs::{edge_loop_points, surface_face};
use super::profile::Shape;
use super::solid::extrude_shapes;
use crate::jsmath::hypot3;
use crate::model::Id;
use crate::value::Value;

pub fn brep(g: &mut GeoCtx<'_>, id: Option<Id>) -> GR<Mesh> {
    let mut m = Mesh::new();
    let outer = g.ra(id, "Outer")?;
    shell_faces(g, outer, &mut m)?;
    for v in refs(g.a(id, "Voids")?) {
        shell_faces(g, Some(v), &mut m)?;
    }
    Ok(m)
}

pub fn surface_model(g: &mut GeoCtx<'_>, id: Option<Id>) -> GR<Mesh> {
    let mut m = Mesh::new();
    let list = if g.t(id) == Some("IFCSHELLBASEDSURFACEMODEL") { g.a(id, "SbsmBoundary")? } else { g.a(id, "FbsmFaces")? };
    for s in refs(list) {
        shell_faces(g, Some(s), &mut m)?;
    }
    Ok(m)
}

pub fn shell_faces(g: &mut GeoCtx<'_>, shell: Option<Id>, m: &mut Mesh) -> GR<()> {
    if shell.is_none() {
        return Ok(());
    }
    for f in refs(g.a(shell, "CfsFaces")?) {
        face(g, Some(f), m)?;
    }
    Ok(())
}

fn face(g: &mut GeoCtx<'_>, f: Option<Id>, m: &mut Mesh) -> GR<()> {
    if surface_face(g, f, m)? {
        return Ok(());
    }
    let bounds = refs(g.a(f, "Bounds")?);
    if bounds.is_empty() {
        return Ok(());
    }
    let mut outer: Option<Vec<V3>> = None;
    let mut holes: Vec<Vec<V3>> = Vec::new();
    for b in bounds {
        let lp = g.ra(Some(b), "Bound")?;
        let mut pts = loop_points(g, lp)?;
        if pts.len() < 3 {
            continue;
        }
        if bool_(g.a(Some(b), "Orientation")?, Some(true)) == Some(false) {
            pts.reverse();
        }
        if g.t(Some(b)) == Some("IFCFACEOUTERBOUND") && outer.is_none() {
            outer = Some(pts);
        } else {
            holes.push(pts);
        }
    }
    let mut outer = match outer {
        Some(o) => o,
        None => {
            // no bound marked outer: the largest is the outer one
            if holes.is_empty() {
                return Ok(());
            }
            let mut best = 0usize;
            let mut area = -1.0;
            for (k, h) in holes.iter().enumerate() {
                let a = ring_area(h);
                if a > area {
                    area = a;
                    best = k;
                }
            }
            holes.remove(best)
        }
    };
    let ft = g.t(f);
    if (ft == Some("IFCFACESURFACE") || ft == Some("IFCADVANCEDFACE")) && bool_(g.a(f, "SameSense")?, Some(true)) == Some(false) {
        outer.reverse();
        for h in &mut holes {
            h.reverse();
        }
    }
    // inner bounds run the other way round from the outer one
    let mut rings = vec![outer];
    rings.extend(holes);
    m.polygon(&rings);
    Ok(())
}

fn ring_area(r: &[V3]) -> f64 {
    let n = newell(r);
    hypot3(n[0], n[1], n[2])
}

fn loop_points(g: &mut GeoCtx<'_>, lp: Option<Id>) -> GR<Vec<V3>> {
    let t = g.t(lp);
    match t.unwrap_or("") {
        "IFCPOLYLOOP" => {
            let mut v = Vec::new();
            for p in refs(g.a(lp, "Polygon")?) {
                v.push(g.point(Some(p))?);
            }
            Ok(dedupe(&v))
        }
        "IFCEDGELOOP" => edge_loop_points(g, lp),
        "IFCVERTEXLOOP" | "" => Ok(Vec::new()),
        other => {
            let p = g.pretty(other);
            g.note(format!("{p} face bounds are not drawn yet"));
            Ok(Vec::new())
        }
    }
}

fn dedupe(pts: &[V3]) -> Vec<V3> {
    let mut out: Vec<V3> = Vec::new();
    for p in pts {
        if out.last().is_none_or(|l| len(&sub(l, p)) > 1e-12) {
            out.push(*p);
        }
    }
    if out.len() > 1
        && let (Some(a), Some(b)) = (out.first(), out.last())
        && len(&sub(a, b)) < 1e-12
    {
        out.pop();
    }
    out
}

/// IfcPolygonalFaceSet / IfcTriangulatedFaceSet.
pub fn face_set(g: &mut GeoCtx<'_>, id: Option<Id>) -> GR<Mesh> {
    let mut m = Mesh::new();
    let c = g.ra(id, "Coordinates")?;
    let Some(coords) = arr(g.a(c, "CoordList")?) else { return Ok(m) };
    let mut pts: Vec<V3> = Vec::with_capacity(coords.len());
    for p in coords {
        pts.push([numd(idx(Some(p), 0)?, 0.0), numd(idx(Some(p), 1)?, 0.0), numd(idx(Some(p), 2)?, 0.0)]);
    }
    let map: Option<Vec<f64>> = match arr(g.a(id, "PnIndex")?) {
        Some(pn) if !pn.is_empty() => Some(pn.iter().map(|v| numd(Some(v), 1.0) - 1.0).collect()),
        _ => None,
    };
    // P[map ? map[k − 1] : k − 1] for a JS-number k
    let pt = |k: f64| -> Option<V3> {
        let i = match &map {
            Some(mp) => *super::curve::at_f(mp, k - 1.0)?,
            None => k - 1.0,
        };
        super::curve::at_f(&pts, i).copied()
    };
    let t = g.t(id);
    if t == Some("IFCTRIANGULATEDFACESET") || t == Some("IFCTRIANGULATEDIRREGULARNETWORK") {
        let ci: &[Value] = match g.a(id, "CoordIndex")? {
            Some(Value::List(l)) => l,
            _ => &[],
        };
        let normals = arr(g.a(id, "Normals")?);
        let ni_raw = g.a(id, "NormalIndex")?;
        let ni = arr(ni_raw);
        for (ti, tri) in ci.iter().enumerate() {
            let a = pt(numd(idx(Some(tri), 0)?, 0.0));
            let b = pt(numd(idx(Some(tri), 1)?, 0.0));
            let c = pt(numd(idx(Some(tri), 2)?, 0.0));
            let (Some(a), Some(b), Some(c)) = (a, b, c) else { continue };
            if let Some(normals) = normals
                && !normals.is_empty()
                && ni.is_none_or(|n| !n.is_empty())
            {
                // per-vertex normals (NormalIndex in IFC4X3, else the same indexes as the points)
                let nit = ni.and_then(|n| n.get(ti)).filter(|v| v.truthy());
                let ix: Vec<f64> = match nit {
                    Some(v) => list_or_empty(Some(v))?.iter().map(|v| numd(Some(v), 1.0)).collect(),
                    None => list_or_empty(Some(tri))?.iter().map(|v| numd(Some(v), 1.0)).collect(),
                };
                let mut nn: Vec<Option<V3>> = Vec::with_capacity(ix.len());
                for k in ix {
                    let n = super::curve::at_f(normals, k - 1.0).filter(|v| v.truthy());
                    nn.push(match n {
                        Some(n) => Some([numd(idx(Some(n), 0)?, 0.0), numd(idx(Some(n), 1)?, 0.0), numd(idx(Some(n), 2)?, 1.0)]),
                        None => None,
                    });
                }
                if nn.iter().all(Option::is_some) {
                    let nv: Vec<V3> = nn.into_iter().flatten().collect();
                    m.add_indexed(&[a, b, c], &[0, 1, 2], Some(&nv));
                    continue;
                }
            }
            m.tri(&a, &b, &c);
        }
        return Ok(m);
    }
    for f in refs(g.a(id, "Faces")?) {
        let outer: Vec<V3> = list_or_empty(g.a(Some(f), "CoordIndex")?)?.iter().filter_map(|k| pt(numd(Some(k), 0.0))).collect();
        let mut holes: Vec<Vec<V3>> = Vec::new();
        for h in list_or_empty(g.a(Some(f), "InnerCoordIndices")?)? {
            let l = match h {
                Value::List(l) => l,
                Value::Null => return Err(type_error_read("null", "map")),
                _ => return Err(super::ctx::not_a_function("map")),
            };
            holes.push(l.iter().filter_map(|k| pt(numd(Some(k), 0.0))).collect());
        }
        let mut rings = vec![dedupe(&outer)];
        rings.extend(holes.iter().map(|h| dedupe(h)));
        m.polygon(&rings);
    }
    Ok(m)
}

/// IfcBoundingBox: a box from its corner.
pub fn bounding_box(g: &mut GeoCtx<'_>, id: Option<Id>) -> GR<Mesh> {
    let c = g.point(g.ra(id, "Corner")?)?;
    let (x, y, z) = (g.na(id, "XDim", 0.0)?, g.na(id, "YDim", 0.0)?, g.na(id, "ZDim", 0.0)?);
    let mut m = extrude_shapes(
        &[Shape { outer: vec![[0.0, 0.0], [x, 0.0], [x, y], [0.0, y]], holes: vec![], open: false }],
        &[0.0, 0.0, 1.0],
        z,
        None,
    );
    m.transform(&translation(c[0], c[1], c[2]));
    Ok(m)
}
