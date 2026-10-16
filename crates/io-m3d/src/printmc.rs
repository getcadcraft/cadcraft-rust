//! print-mc.js — what ModelCraft hands print3d (0.5 SG.4): `printTriangles(model, {path, sel})`. What prints is what
//! is on screen (hidden faces, hidden placements and placements on hidden tags left out); with placements or faces
//! selected in the open group `path`, only they print. Metres → millimetres, Z up; colours as the view shows them.

use std::collections::HashSet;

use cc_m3d::def::{Def, face_tris};
use cc_m3d::vec::{M4, apply, det3, mmul};
use cc_m3d::{Model, Sel};

use crate::print3d::Tri;
use crate::util::paint_hex;

const MM: f64 = 1000.0;

/// `model.walkShown(cb, def, m, [], 0, color)`: every placement on screen under def, with the nearest placement
/// colour (the kernel's walkShown starts with no colour; print-mc hands one in).
#[allow(clippy::too_many_arguments)]
fn walk_shown(
    model: &Model,
    d: &Def,
    m: &M4,
    depth: u32,
    color: Option<&str>,
    off: &HashSet<String>,
    cb: &mut dyn FnMut(&Def, &M4, Option<&str>),
    budget: &mut u64,
) {
    if depth > 32 || *budget == 0 {
        return;
    }
    *budget = budget.saturating_sub(1);
    cb(d, m, color);
    for inst in d.insts.values() {
        if inst.hidden || inst.tag.as_deref().is_some_and(|t| !t.is_empty() && off.contains(t)) {
            continue;
        }
        let Some(sub) = model.def(&inst.def) else { continue };
        let c = inst.color.as_deref().filter(|c| !c.is_empty()).or(color);
        walk_shown(model, sub, &mmul(m, &inst.m), depth.saturating_add(1), c, off, cb, budget);
    }
}

fn add(tris: &mut Vec<Tri>, def: &Def, m: &M4, color: Option<&str>, only: Option<&HashSet<u64>>) {
    let mirror = det3(m) < 0.0;
    for (fid, f) in def.faces.iter() {
        if f.hidden || only.is_some_and(|o| !o.contains(&fid.to_bits())) {
            continue;
        }
        for t in face_tris(def, f) {
            let pt = |v: f64| {
                let p = apply(m, def.v(v));
                [p[0] * MM, p[1] * MM, p[2] * MM]
            };
            let mut p = [pt(t[0]), pt(t[1]), pt(t[2])];
            if mirror {
                p = [p[0], p[2], p[1]];
            }
            let c = f.color.as_deref().filter(|c| !c.is_empty()).or(color.filter(|c| !c.is_empty()));
            tris.push(Tri { p, color: c.map(paint_hex) });
        }
    }
}

/// `printTriangles(model, {path, sel})` → (triangles in mm, 'selection' | 'all').
pub fn print_triangles(model: &Model, path: &[f64], sel: &[Sel]) -> (Vec<Tri>, &'static str) {
    let Some(ctx) = model.context(path).or_else(|| model.context(&[])) else { return (Vec::new(), "all") };
    let Some(cdef) = model.def(&ctx.def) else { return (Vec::new(), "all") };
    let mut tris = Vec::new();
    let off = model.hidden_tags();
    let mut budget = cc_m3d::model::WALK_BUDGET;
    let insts: Vec<f64> = sel.iter().filter(|s| s.is("i") && cdef.insts.has(s.id)).map(|s| s.id).collect();
    let faces: HashSet<u64> = sel.iter().filter(|s| s.is("f") && cdef.faces.has(s.id)).map(|s| s.id.to_bits()).collect();
    let base = model.path_color(path);
    if !insts.is_empty() || !faces.is_empty() {
        for iid in insts {
            let Some(inst) = cdef.insts.get(iid) else { continue };
            let Some(d) = model.def(&inst.def) else { continue };
            if inst.hidden || inst.tag.as_deref().is_some_and(|t| !t.is_empty() && off.contains(t)) {
                continue;
            }
            let c = inst.color.clone().filter(|c| !c.is_empty()).or_else(|| base.clone());
            walk_shown(
                model,
                d,
                &mmul(&ctx.m, &inst.m),
                0,
                c.as_deref(),
                &off,
                &mut |dd, wm, cc| add(&mut tris, dd, wm, cc, None),
                &mut budget,
            );
        }
        if !faces.is_empty() {
            add(&mut tris, cdef, &ctx.m, base.as_deref(), Some(&faces));
        }
        return (tris, "selection");
    }
    let m0 = model.context(&[]).map_or(cc_m3d::vec::IDENT, |c| c.m);
    if let Some(root) = model.root() {
        walk_shown(model, root, &m0, 0, None, &off, &mut |dd, wm, cc| add(&mut tris, dd, wm, cc, None), &mut budget);
    }
    (tris, "all")
}
