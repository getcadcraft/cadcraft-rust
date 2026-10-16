//! solids.js — Union, Subtract and Intersect on closed groups and components (0.4 M9), through cc-manifold (the same
//! results as the manifold-3d wasm the JS loads). Solids are welded and moved near their common middle before Manifold
//! sees them (its vertices are 32-bit floats), and back after on a 1 µm grid; coplanar triangles of the result are
//! merged into one face per flat side.
//!
//! Handle lifetimes follow the JS: every Manifold the JS `.delete()`s is dropped at that point, and the two batch
//! unions a subtraction makes (never deleted in the JS) live until the operation ends — the lazy CSG tree only folds a
//! node no other handle holds, so this decides the result.

use cc_m3d::Model;
use cc_m3d::def::face_tris;
use cc_m3d::edges::merge_coplanar;
use cc_m3d::model::place;
use cc_m3d::omap::FxMap;
use cc_m3d::solids::solid_info;
use cc_m3d::vec::{M4, P3, apply, mmul, translation};
use cc_manifold::{Manifold, Mesh, Status};

use crate::Res;
use crate::i18n::{err, err_with, t};
use crate::stl::f32_of;

/// Most triangles a solid may bring (the JS runs out of memory long before).
const MAX_TRIS: usize = 8_000_000;
/// Most definitions visited while checking one placement (definitions placed in each other multiply).
const MAX_VISITS: u64 = 4_000_000;

/// `placementTris(model, def, iid)`: every triangle of placement iid of `def`, in def's coordinates.
fn placement_tris(model: &Model, def: &str, iid: f64) -> Res<Vec<[P3; 3]>> {
    let mut out = Vec::new();
    let Some(inst) = model.def(def).and_then(|d| d.insts.get(iid)) else { return Ok(out) };
    let mut stack: Vec<(String, M4, u32)> = vec![(inst.def.clone(), inst.m, 0)];
    let mut visits = 0u64;
    // the JS recursion, depth first in order: a stack of pending (def, matrix, depth), children pushed in reverse
    while let Some((did, m, depth)) = stack.pop() {
        let Some(d) = model.def(&did) else { continue };
        if depth > 32 {
            continue;
        }
        visits = visits.saturating_add(1);
        if visits > MAX_VISITS {
            return Err(err("m3d.io.this_model_is_too_big_to"));
        }
        for f in d.faces.values() {
            for tri in face_tris(d, f) {
                out.push([apply(&m, d.v(tri[0])), apply(&m, d.v(tri[1])), apply(&m, d.v(tri[2]))]);
            }
            if out.len() > MAX_TRIS {
                return Err(err("m3d.io.this_model_is_too_big_to"));
            }
        }
        let subs: Vec<(String, M4)> = d.insts.values().map(|s| (s.def.clone(), mmul(&m, &s.m))).collect();
        for (sd, sm) in subs.into_iter().rev() {
            stack.push((sd, sm, depth.saturating_add(1)));
        }
    }
    Ok(out)
}

/// `isSolid(model, def, iid)`: is that placement a closed solid (its geometry and everything in it, each definition
/// closed on its own, as SketchUp asks)?
pub fn is_solid(model: &Model, def: &str, iid: f64) -> bool {
    let Some(inst) = model.def(def).and_then(|d| d.insts.get(iid)) else { return false };
    let mut ok = true;
    let mut any = false;
    let mut stack: Vec<(String, u32)> = vec![(inst.def.clone(), 0)];
    let mut visits = 0u64;
    while let Some((did, depth)) = stack.pop() {
        if !ok {
            break;
        }
        let Some(d) = model.def(&did) else { continue };
        if depth > 32 {
            continue;
        }
        visits = visits.saturating_add(1);
        if visits > MAX_VISITS {
            return false;
        }
        if !d.faces.is_empty() {
            any = true;
            if !solid_info(d).closed {
                ok = false;
            }
        }
        let subs: Vec<String> = d.insts.values().map(|s| s.def.clone()).collect();
        for sd in subs.into_iter().rev() {
            stack.push((sd, depth.saturating_add(1)));
        }
    }
    ok && any
}

/// `toManifold(M, tris, o, name)`: triangles → a Manifold near the origin `o`; refused when they do not close a volume.
fn to_manifold(tris: &[[P3; 3]], o: P3, name: &str) -> Res<Manifold> {
    let mut key: FxMap<String, u32> = FxMap::default();
    let mut verts: Vec<f64> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let mut kbuf = String::new();
    let mut vid = |p: P3, verts: &mut Vec<f64>| -> u32 {
        let q = [p[0] - o[0], p[1] - o[1], p[2] - o[2]];
        kbuf.clear();
        for (i, x) in q.iter().enumerate() {
            if i > 0 {
                kbuf.push(',');
            }
            cc_m3d::js::push_num(&mut kbuf, cc_m3d::js::round(x * 1e7));
        }
        if let Some(&i) = key.get(&kbuf) {
            return i;
        }
        let i = u32::try_from(verts.len() / 3).unwrap_or(u32::MAX);
        key.insert(kbuf.clone(), i);
        verts.extend_from_slice(&q);
        i
    };
    for tri in tris {
        let a = vid(tri[0], &mut verts);
        let b = vid(tri[1], &mut verts);
        let c = vid(tri[2], &mut verts);
        if a != b && b != c && a != c {
            idx.extend_from_slice(&[a, b, c]);
        }
    }
    let mut mesh = Mesh::new(3, verts.iter().map(|&x| f32_of(x)).collect(), idx);
    mesh.merge();
    let man = Manifold::from_mesh_unchecked(&mesh);
    if man.status() != Status::NoError || man.is_empty() {
        drop(man);
        return Err(err_with("app.solid.open", &[("name", name)]));
    }
    Ok(man)
}

/// What a solid operation returns: the new placement, and its solid's faces, volume, closed.
#[derive(Clone, Debug, PartialEq)]
pub struct SolidResult {
    pub iid: f64,
    pub faces: f64,
    pub volume: f64,
    pub closed: bool,
}

/// `name.replace(/\s*#\d+$/, '')`.
fn strip_number(s: &str) -> String {
    let digits = s.chars().rev().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return s.to_string();
    }
    let cut = s.len().saturating_sub(digits);
    match s.get(..cut).and_then(|r| r.strip_suffix('#')) {
        Some(rest) => rest.trim_end_matches(crate::num::is_space).to_string(),
        None => s.to_string(),
    }
}

/// `solidOp(M, model, path, op, keep, cut)`: op 'union' | 'subtract' | 'intersect' on placements of the open context
/// `path`. One new group in their place, in one change of the model (one undo step); it keeps the first one's name,
/// colour and tag.
#[allow(clippy::too_many_lines)]
pub fn solid_op(model: &mut Model, path: &[f64], op: &str, keep: &[f64], cut: &[f64]) -> Res<SolidResult> {
    let Some(ctx) = model.context(path) else { return Err(err("app.solid.gone")) };
    let def = ctx.def.clone();
    let all: Vec<f64> = keep.iter().chain(cut.iter()).copied().collect();
    let refuse = if op == "subtract" { keep.is_empty() || cut.is_empty() } else { all.len() < 2 };
    if refuse {
        return Err(err(if op == "subtract" { "app.solid.need.subtract" } else { "app.solid.need.two" }));
    }
    let name_of = |model: &Model, iid: f64| -> String {
        model
            .def(&def)
            .and_then(|d| d.insts.get(iid))
            .and_then(|i| model.def(&i.def))
            .map(|d| d.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| t("app.solid.a_solid", &[]))
    };
    for &iid in &all {
        if !is_solid(model, &def, iid) {
            return Err(err_with("app.solid.open", &[("name", &name_of(model, iid))]));
        }
    }
    // a Map: a placement named twice is one entry (the first place, the last value)
    let mut tris: Vec<(u64, Vec<[P3; 3]>)> = Vec::new();
    for &iid in &all {
        let v = placement_tris(model, &def, iid)?;
        match tris.iter_mut().find(|(k, _)| *k == iid.to_bits()) {
            Some(slot) => slot.1 = v,
            None => tris.push((iid.to_bits(), v)),
        }
    }
    // the origin: the middle of everything, so the 32-bit vertices keep their precision
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for (_, ts) in &tris {
        for tri in ts {
            for p in tri {
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
    let tris_of = |iid: f64| -> &[[P3; 3]] { tris.iter().find(|(k, _)| *k == iid.to_bits()).map_or(&[][..], |(_, v)| v.as_slice()) };
    // one handle per JS Manifold object: the inputs (in `made`, deleted in the JS's finally), the result, and the
    // temporaries a subtraction makes (never deleted in the JS): all live until the operation ends
    let mut a_list: Vec<Manifold> = Vec::new();
    for &iid in keep {
        a_list.push(to_manifold(tris_of(iid), o, &name_of(model, iid))?);
    }
    let mut b_list: Vec<Manifold> = Vec::new();
    for &iid in cut {
        b_list.push(to_manifold(tris_of(iid), o, &name_of(model, iid))?);
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
        return Err(err(if op == "intersect" { "app.solid.empty.intersect" } else { "app.solid.empty" }));
    }
    let g = r.get_mesh();
    // back from 32-bit floats: each corner on a 1 µm grid of the model (exact sizes stay exact), relative to o again
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
    let polys: Vec<cc_m3d::meshin::PolyIn> =
        g.tri_verts.as_chunks::<3>().0.iter().map(|&[a, b, d]| cc_m3d::meshin::PolyIn::outline(vec![pt(a), pt(b), pt(d)])).collect();
    let first = model.def(&def).and_then(|d| d.insts.get(keep.first().copied().unwrap_or(f64::NAN))).cloned();
    let label = t(
        match op {
            "union" => "app.solid.union",
            "subtract" => "app.solid.subtract",
            _ => "app.solid.intersect",
        },
        &[],
    );
    let base = strip_number(&name_of(model, keep.first().copied().unwrap_or(f64::NAN)));
    let path_v = path.to_vec();
    let all_c = all.clone();
    let iid = model.change(&label, |m| {
        let Some(c) = m.context(&path_v) else { return Err(cc_m3d::Error::Type) };
        let uname = m.unique_name(&base);
        let nd = cc_m3d::meshin::mesh_def_in(m, &polys, Some(&uname), "group", &[]);
        // one face per flat side, as drawn faces are (manifold hands back triangles)
        if let Some(d) = m.def_mut(&nd) {
            let soft: Vec<f64> = d.edges.iter().filter(|(_, e)| e.soft).map(|(id, _)| id).collect();
            merge_coplanar(d, &soft);
        }
        let iid = match m.def_mut(&c.def) {
            Some(d) => {
                for id in &all_c {
                    d.insts.delete(*id);
                }
                let iid = place(d, &nd, &translation(o));
                if let (Some(inst), Some(f)) = (d.insts.get_mut(iid), first.as_ref()) {
                    if let Some(col) = f.color.clone().filter(|s| !s.is_empty()) {
                        inst.color = Some(col);
                    }
                    if let Some(tg) = f.tag.clone().filter(|s| !s.is_empty()) {
                        inst.tag = Some(tg);
                    }
                }
                iid
            }
            None => return Err(cc_m3d::Error::Type),
        };
        m.purge();
        Ok(iid)
    })?;
    drop(r);
    drop(a_list);
    drop(b_list);
    drop(temps);
    let info = model
        .context(path)
        .and_then(|c| model.def(&c.def).and_then(|d| d.insts.get(iid)).map(|i| i.def.clone()))
        .and_then(|id| model.def(&id).map(solid_info));
    let Some(info) = info else { return Err(cc_m3d::Error::Type) };
    Ok(SolidResult { iid, faces: info.faces, volume: info.volume.unwrap_or(0.0).abs(), closed: info.closed })
}

#[cfg(test)]
mod tests {
    use super::strip_number;
    #[test]
    fn names() {
        assert_eq!(strip_number("Box #12"), "Box");
        assert_eq!(strip_number("Group#3"), "Group");
        assert_eq!(strip_number("A #1 #2"), "A #1");
        assert_eq!(strip_number("Plain"), "Plain");
        assert_eq!(strip_number("No12"), "No12");
    }
}
