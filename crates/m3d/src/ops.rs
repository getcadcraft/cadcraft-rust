//! Push/pull, erasing, moving and copying, shapes, curves and smooth curves, hide and isolate, flip and follow me
//! (model.js §§ Push/pull … Follow me). Functions that throw in the JS return `Err`; the messages are the JS's.

use crate::def::*;
use crate::edges::*;
use crate::error::{Error, Res, msg};
use crate::js;
use crate::jsmath::JsMath;
use crate::omap::{FxMap, IdMap, IdSet, kb};
use crate::vec::*;

/// A selection item: {t: 'f' | 'e' | 'i' (| 'g' …), id}.
#[derive(Clone, Debug, PartialEq)]
pub struct Sel {
    pub t: String,
    pub id: f64,
}
impl Sel {
    pub fn new(t: &str, id: f64) -> Self {
        Self { t: t.to_string(), id }
    }
    #[inline]
    pub fn is(&self, t: &str) -> bool {
        self.t == t
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Push/pull

/// Is the face part of a closed surface (every boundary edge shared with another face)?
fn backed(def: &Def, fid: f64, ef: &EdgeFaces) -> bool {
    let Some(f) = def.faces.get(fid) else { return false };
    let mut all = true;
    for_segs(f, |a, b| {
        if ef_get(ef, a, b).len() < 2 {
            all = false;
        }
    });
    all
}

/// How far a face can be pushed in before it meets the first face behind it (Infinity if none).
pub fn depth_behind(def: &Def, fid: f64) -> f64 {
    let Some(f) = def.faces.get(fid) else { return f64::INFINITY };
    let n = face_normal(def, f);
    let fr = frame(n);
    let l2 = loops2(def, f, &fr);
    let refs: Vec<&[P2]> = l2.iter().map(|l| l.as_slice()).collect();
    let Some(pt2) = sample_point(&refs) else { return f64::INFINITY };
    let p0 = f.loops.first().and_then(|l| l.first()).map_or([f64::NAN; 3], |&v| def.v(v));
    let o = add(add(mul(fr.u, pt2[0]), mul(fr.v, pt2[1])), mul(n, dot(n, p0)));
    let d = mul(n, -1.0);
    let mut best = f64::INFINITY;
    for (gid, g) in def.faces.iter() {
        if gid == fid {
            continue;
        }
        for [a, b, c] in face_tris(def, g) {
            if let Some(t) = ray_tri(o, d, def.v(a), def.v(b), def.v(c))
                && t > EPS
                && t < best
            {
                best = t;
            }
        }
    }
    best
}

/// What pushPull did.
#[derive(Clone, Debug, PartialEq)]
pub struct PushResult {
    pub ok: bool,
    /// 'move' | 'extrude'
    pub mode: &'static str,
    pub distance: f64,
    pub face: Option<f64>,
}
impl PushResult {
    fn no() -> Self {
        Self { ok: false, mode: "", distance: 0.0, face: None }
    }
}

/// Push or pull face `fid` by `distance` along its normal (positive = out of its front).
pub fn push_pull(def: &mut Def, fid: f64, distance: f64) -> PushResult {
    let mut distance = distance;
    let Some(f) = def.faces.get(fid).cloned() else { return PushResult::no() };
    if !(distance.abs() > EPS) {
        return PushResult::no();
    }
    let n = face_normal(def, &f);
    if !js::truthy(len(n)) {
        return PushResult::no();
    }
    let ef = edge_faces(def);
    let vids = IdSet::from_iter_ids(f.loops.iter().flatten().copied());
    let segs = loop_segs(&f);
    let contains = |def: &Def, g: f64| def.faces.get(g).is_some_and(|g| dot(face_normal(def, g), n).abs() < 1e-6);

    // Move mode: every boundary edge has a neighbour face that runs along n, and so does every other face touching
    // the face's corners.
    let mut mv = segs.iter().all(|&(a, b)| {
        let others: Vec<f64> = ef_get(&ef, a, b).iter().copied().filter(|&x| x != fid).collect();
        !others.is_empty() && others.iter().all(|&o| contains(def, o))
    });
    if mv {
        let vf = vert_faces(def);
        for v in vids.iter() {
            for &g in ve_get(&vf, v) {
                if g != fid && !contains(def, g) {
                    mv = false;
                }
            }
        }
    }
    if mv {
        // the face can come back no further than its shortest side
        let mut limit = f64::INFINITY;
        let ve = vert_edges(def);
        for v in vids.iter() {
            for &eid in ve_get(&ve, v) {
                let Some(e) = def.edges.get(eid) else { continue };
                let w = if e.a == v { e.b } else { e.a };
                if vids.has(w) {
                    continue;
                }
                let along = dot(sub(def.v(w), def.v(v)), n);
                if along < -EPS {
                    limit = js::min(limit, -along);
                }
            }
        }
        if distance < 0.0 && -distance >= limit - EPS {
            distance = -limit;
        }
        for v in vids.iter() {
            let p = add(def.v(v), mul(n, distance));
            def.verts.set(v, p);
        }
        let first = f.loops.first().and_then(|l| l.first()).copied().unwrap_or(f64::NAN);
        let anchor = add(def.v(first), [0.0, 0.0, 0.0]);
        cleanup(def);
        reface(def, &plane_from(n, anchor), &IdSet::new(), None);
        cleanup(def);
        return PushResult { ok: true, mode: "move", distance, face: def.faces.has(fid).then_some(fid) };
    }

    let solid = backed(def, fid, &ef);
    if solid && distance < 0.0 {
        let t = depth_behind(def, fid);
        if -distance > t - EPS {
            distance = -t;
        }
    }
    let mut map: FxMap<u64, f64> = FxMap::default();
    for v in vids.iter() {
        let p = add(def.v(v), mul(n, distance));
        let nv = v_add(def, p);
        map.insert(kb(v), nv);
    }
    let mp = |v: f64| map.get(&kb(v)).copied().unwrap_or(f64::NAN);
    let orig: Vec<Vec<f64>> = f.loops.clone();
    let top: Vec<Vec<f64>> = orig.iter().map(|l| l.iter().map(|&v| mp(v)).collect()).collect();
    let lone = !solid;
    let flip = lone && distance < 0.0;
    if lone && distance > 0.0 {
        if let Some(fm) = def.faces.get_mut(fid) {
            fm.loops = orig.iter().map(|l| l.iter().rev().copied().collect()).collect();
        }
    } else if !lone {
        def.faces.delete(fid);
    }
    let top_loops = if flip { top.iter().map(|l| l.iter().rev().copied().collect()).collect() } else { top };
    let top_id = f_add(def, top_loops, f.color.clone(), None, false);
    let mut side_edges: Vec<Option<f64>> = Vec::new();
    for l in &orig {
        let m = l.len();
        for (i, &a) in l.iter().enumerate() {
            let b = l.get((i + 1) % m).copied().unwrap_or(a);
            let (a2, b2) = (mp(a), mp(b));
            let mut quad = vec![a, b, b2, a2];
            if flip {
                quad.reverse();
            }
            f_add(def, vec![quad], f.color.clone(), None, false);
            let s1 = edge_between(def, a, a2);
            let s2 = edge_between(def, a, b);
            side_edges.push(s1);
            side_edges.push(s2);
        }
    }
    cleanup(def);
    let top_first = def.faces.get(top_id).and_then(|tf| tf.loops.first()).and_then(|l| l.first()).copied();
    if let Some(v) = top_first {
        let pl = plane_from(n, def.v(v));
        reface(def, &pl, &IdSet::new(), None);
    }
    cleanup(def);
    let se: Vec<f64> = side_edges.into_iter().flatten().collect();
    merge_coplanar(def, &se);
    // the sides of a pulled circle read as one smooth surface: soften edges between side faces < 25° apart
    let ef2 = edge_faces(def);
    let c25 = (25.0 * std::f64::consts::PI / 180.0).jcos();
    for l in &orig {
        for &a in l {
            let Some(eid) = edge_between(def, a, mp(a)) else { continue };
            let Some(e) = def.edges.get(eid) else { continue };
            let fs = ef_get(&ef2, e.a, e.b);
            if let [f0, f1] = fs
                && let (Some(fa), Some(fb)) = (def.faces.get(*f0), def.faces.get(*f1))
                && dot(face_normal(def, fa), face_normal(def, fb)) > c25
                && let Some(e) = def.edges.get_mut(eid)
            {
                e.soft = true;
            }
        }
    }
    PushResult { ok: true, mode: "extrude", distance, face: def.faces.has(top_id).then_some(top_id) }
}

// ---------------------------------------------------------------------------------------------------------------
// Erasing, moving, copying

/// Erase edges: an edge between two coplanar faces merges them; otherwise its faces go too.
pub fn erase_edges(def: &mut Def, eids: &[f64]) {
    let mut ends = Vec::new();
    for &eid in eids {
        let Some(e) = def.edges.get(eid).cloned() else { continue };
        ends.push(e.a);
        ends.push(e.b);
        let ef = edge_faces(def);
        let fs: Vec<f64> = ef_get(&ef, e.a, e.b).to_vec();
        if let [f0, f1] = fs.as_slice()
            && let (Some(fa), Some(fb)) = (def.faces.get(*f0), def.faces.get(*f1))
        {
            let p1 = face_plane(def, fa);
            let p2 = face_plane(def, fb);
            if dot(p1.n, p2.n) > 1.0 - 1e-9 && (p1.d - p2.d).abs() <= EPS {
                e_remove(def, eid);
                let pl = plane_from(p1.n, def.v(e.a));
                reface(def, &pl, &IdSet::new(), None);
                continue;
            }
        }
        for fid in fs {
            def.faces.delete(fid);
        }
        e_remove(def, eid);
    }
    remove_loose_verts(def);
    heal_collinear(def, &ends);
}

/// Every vertex used by a selection's edges and faces.
pub fn sel_verts(def: &Def, sel: &[Sel]) -> IdSet {
    let mut vs = IdSet::new();
    for s in sel {
        if s.is("e") {
            if let Some(e) = def.edges.get(s.id) {
                vs.add(e.a);
                vs.add(e.b);
            }
        } else if s.is("f")
            && let Some(f) = def.faces.get(s.id)
        {
            for l in &f.loops {
                for &v in l {
                    vs.add(v);
                }
            }
        }
    }
    vs
}

/// Apply matrix m to a selection (vertices of its edges and faces, and its instances).
pub fn transform_sel(def: &mut Def, sel: &[Sel], m: &M4) {
    let vs = sel_verts(def, sel);
    transform_verts(def, &vs, m);
    for s in sel {
        if s.is("i")
            && let Some(inst) = def.insts.get_mut(s.id)
        {
            inst.m = mmul(m, &inst.m);
        }
    }
}

/// Move vertices by matrix m; faces bent out of plane fold into triangles.
pub fn transform_verts(def: &mut Def, vs: &IdSet, m: &M4) {
    if vs.is_empty() {
        return;
    }
    let vf = vert_faces(def);
    let mut touched = IdSet::new();
    for v in vs.iter() {
        for &f in ve_get(&vf, v) {
            touched.add(f);
        }
    }
    let mut before: FxMap<u64, Vec<[f64; 3]>> = FxMap::default();
    for fid in touched.iter() {
        if let Some(f) = def.faces.get(fid) {
            before.insert(kb(fid), face_tris(def, f));
        }
    }
    for v in vs.iter() {
        let p = apply(m, def.v(v));
        def.verts.set(v, p);
    }
    // a mirror (Flip, a negative scale) turns faces inside out: those it moved whole are reversed so they face out again
    if det3(m) < 0.0 {
        for f in def.faces.values_mut() {
            if f.loops.iter().all(|l| l.iter().all(|&v| vs.has(v))) {
                for l in f.loops.iter_mut() {
                    l.reverse();
                }
            }
        }
    }
    // a curve moved whole keeps its points; stretched in part, it is no longer editable
    let cids: Vec<String> = def.splines.keys().cloned().collect();
    for cid in cids {
        let own: Vec<(f64, f64)> = def.edges.values().filter(|e| e.curve.as_deref() == Some(cid.as_str())).map(|e| (e.a, e.b)).collect();
        let moved = own.iter().filter(|(a, b)| vs.has(*a) && vs.has(*b)).count();
        if !own.is_empty() && moved == own.len() {
            if let Some(sp) = def.splines.get_mut(&cid) {
                sp.pts = sp.pts.iter().map(|p| apply(m, *p)).collect();
            }
        } else if moved > 0 || own.iter().any(|(a, b)| vs.has(*a) || vs.has(*b)) {
            def.splines.delete(&cid);
        }
    }
    for fid in touched.iter() {
        let Some(f) = def.faces.get(fid) else { continue };
        if planar(def, f) {
            continue;
        }
        let (color, hidden) = (f.color.clone(), f.hidden);
        def.faces.delete(fid);
        for tri in before.get(&kb(fid)).cloned().unwrap_or_default() {
            f_add(def, vec![tri.to_vec()], color.clone(), None, hidden);
        }
    }
    cleanup(def);
}

fn planar(def: &Def, f: &Face) -> bool {
    let pl = face_plane(def, f);
    if !js::truthy(len(pl.n)) {
        return false;
    }
    f.loops.iter().all(|l| l.iter().all(|&v| (dot(pl.n, def.v(v)) - pl.d).abs() <= EPS * 10.0))
}

/// Stretch (resize after the fact): move every vertex of `def` at or beyond `cut` along unit direction `d` by
/// `delta` — only the vertices in `only`, when given — and every instance standing wholly beyond the cut on that
/// geometry. Faces between the moved and the still vertices stretch. Fails (changing nothing) when a face would bend
/// out of its plane or turn inside out. `box_of(defId)` gives a definition's own box. → the number of vertices moved.
pub fn stretch(def: &mut Def, d: P3, cut: f64, delta: f64, only: Option<&IdSet>, box_of: &dyn Fn(&str) -> Option<BBox>) -> Res<f64> {
    let tol = EPS * 10.0;
    let mut vs = IdSet::new();
    for (id, p) in def.verts.iter() {
        if only.is_none_or(|o| o.has(id)) && dot(*p, d) >= cut - tol {
            vs.add(id);
        }
    }
    if vs.is_empty() && def.insts.is_empty() {
        return Err(msg("m3d.model.there_is_nothing_on_that_side"));
    }
    let mut at: IdMap<P3> = IdMap::new();
    for v in vs.iter() {
        at.set(v, add(def.v(v), mul(d, delta)));
    }
    let pos = |def: &Def, v: f64| at.get(v).copied().unwrap_or_else(|| def.v(v));
    for f in def.faces.values() {
        let all: Vec<f64> = f.loops.iter().flatten().copied().collect();
        let n = all.iter().filter(|&&v| vs.has(v)).count();
        if n == 0 || n == all.len() {
            continue;
        }
        let n0 = face_normal(def, f);
        let l0 = f.loops.first().map_or(&[][..], |l| l.as_slice());
        let p1: Vec<P3> = l0.iter().map(|&v| pos(def, v)).collect();
        let n1 = newell(&p1);
        if len(n1) <= EPS2 || dot(norm(n1), n0) <= 0.0 {
            return Err(msg("m3d.model.a_face_would_shrink_to_nothing"));
        }
        let k = norm(n1);
        let c = dot(k, pos(def, l0.first().copied().unwrap_or(f64::NAN)));
        if !all.iter().all(|&v| (dot(k, pos(def, v)) - c).abs() <= tol) {
            return Err(msg("m3d.model.a_face_would_bend_out_of"));
        }
    }
    let mut bb: Option<BBox> = None;
    if let Some(o) = only {
        for v in o.iter() {
            bb = Some(bbox_add(bb, def.v(v)));
        }
    }
    for (v, p) in at.iter() {
        def.verts.set(v, *p);
    }
    for inst in def.insts.values_mut() {
        let Some(local) = box_of(&inst.def) else { continue };
        // the instance's corners in def's coordinates: it moves when it stands wholly beyond the cut, on this piece
        let mut corners = Vec::with_capacity(8);
        for i in 0..8u32 {
            let x = if i & 1 != 0 { local.max[0] } else { local.min[0] };
            let y = if i & 2 != 0 { local.max[1] } else { local.min[1] };
            let z = if i & 4 != 0 { local.max[2] } else { local.min[2] };
            corners.push(apply(&inst.m, [x, y, z]));
        }
        if corners.iter().any(|p| dot(*p, d) < cut - tol) {
            continue;
        }
        let mut ib: Option<BBox> = None;
        for p in &corners {
            ib = Some(bbox_add(ib, *p));
        }
        if let (Some(b), Some(ib)) = (bb, ib)
            && (0..3).any(|i| at3(&ib.min, i) > at3(&b.max, i) + tol || at3(&ib.max, i) < at3(&b.min, i) - tol)
        {
            continue;
        }
        inst.m = mmul(&translation(mul(d, delta)), &inst.m);
    }
    cleanup(def);
    Ok(vs.len() as f64)
}

/// `curveRemapper(def)`: gives copied edges fresh curve ids in def.
#[derive(Default)]
pub struct CurveRemap(Vec<(String, String)>);
impl CurveRemap {
    pub fn map(&mut self, def: &mut Def, c: Option<&str>) -> Option<String> {
        let c = c.filter(|c| !c.is_empty())?;
        if let Some((_, v)) = self.0.iter().find(|(k, _)| k == c) {
            return Some(v.clone());
        }
        let kind = c.split(':').next().unwrap_or("");
        let id = nid(def);
        let v = format!("{kind}:{}", js::num_str(id));
        self.0.push((c.to_string(), v.clone()));
        Some(v)
    }
}

/// Copy a selection in place; → the copy's selection.
pub fn copy_sel(def: &mut Def, sel: &[Sel]) -> Vec<Sel> {
    let mut vmap: FxMap<u64, f64> = FxMap::default();
    fn cv(def: &mut Def, vmap: &mut FxMap<u64, f64>, v: f64) -> f64 {
        if let Some(&x) = vmap.get(&kb(v)) {
            return x;
        }
        let p = def.v(v);
        let id = v_add(def, p);
        vmap.insert(kb(v), id);
        id
    }
    let mut out = Vec::new();
    let mut rc = CurveRemap::default();
    let faces: Vec<Face> = sel.iter().filter(|s| s.is("f")).filter_map(|s| def.faces.get(s.id).cloned()).collect();
    for f in faces {
        let loops: Vec<Vec<f64>> = f.loops.iter().map(|l| l.iter().map(|&v| cv(def, &mut vmap, v)).collect()).collect();
        let id = f_add(def, loops, f.color.clone(), None, f.hidden);
        out.push(Sel::new("f", id));
    }
    let mut spl: Vec<(Option<String>, Spline)> = Vec::new();
    for s in sel {
        if !s.is("e") {
            continue;
        }
        let Some(e) = def.edges.get(s.id).cloned() else { continue };
        let nc = rc.map(def, e.curve.as_deref());
        let a = cv(def, &mut vmap, e.a);
        let b = cv(def, &mut vmap, e.b);
        let id = e_add(def, a, b, e.soft, nc.clone(), e.hidden);
        out.push(Sel { t: "e".into(), id: id.unwrap_or(f64::NAN) });
        if let Some(c) = e.curve.as_deref()
            && let Some(sp) = def.splines.get(c)
        {
            let sp = sp.clone();
            match spl.iter_mut().find(|(k, _)| *k == nc) {
                Some(slot) => slot.1 = sp,
                None => spl.push((nc, sp)),
            }
        }
    }
    for (nc, sp) in spl {
        if let Some(nc) = nc {
            def.splines.set(nc, sp);
        }
    }
    for s in sel {
        if !s.is("i") {
            continue;
        }
        let Some(inst) = def.insts.get(s.id).cloned() else { continue };
        let id = nid(def);
        def.insts.set(id, inst);
        out.push(Sel::new("i", id));
    }
    out
}

pub fn erase_sel(def: &mut Def, sel: &[Sel]) {
    for s in sel {
        if s.is("f") {
            def.faces.delete(s.id);
        }
    }
    for s in sel {
        if s.is("i") {
            def.insts.delete(s.id);
        }
    }
    let eids: Vec<f64> = sel.iter().filter(|s| s.is("e")).map(|s| s.id).collect();
    erase_edges(def, &eids);
    remove_loose_verts(def);
}

/// Reverse faces (front ↔ back).
pub fn reverse_faces(def: &mut Def, fids: &[f64]) {
    for &fid in fids {
        if let Some(f) = def.faces.get_mut(fid) {
            for l in f.loops.iter_mut() {
                l.reverse();
            }
        }
    }
}

/// All edges and faces connected to a start set through shared vertices.
pub fn connected(def: &Def, sel: &[Sel]) -> Vec<Sel> {
    let vs = sel_verts(def, sel);
    let ve = vert_edges(def);
    let vf = vert_faces(def);
    let mut stack = vs.to_vec();
    let mut seen = vs.clone();
    let mut eids = IdSet::new();
    let mut fids = IdSet::new();
    while let Some(v) = stack.pop() {
        for &eid in ve_get(&ve, v) {
            eids.add(eid);
            let Some(e) = def.edges.get(eid) else { continue };
            for w in [e.a, e.b] {
                if !seen.has(w) {
                    seen.add(w);
                    stack.push(w);
                }
            }
        }
        for &fid in ve_get(&vf, v) {
            fids.add(fid);
        }
    }
    fids.iter().map(|id| Sel::new("f", id)).chain(eids.iter().map(|id| Sel::new("e", id))).collect()
}

/// Every vertex joined to vertex v by edges (one piece of loose geometry).
pub fn connected_verts(def: &Def, v: f64) -> IdSet {
    let ve = vert_edges(def);
    let mut seen = IdSet::new();
    seen.add(v);
    let mut stack = vec![v];
    while let Some(x) = stack.pop() {
        for &eid in ve_get(&ve, x) {
            let Some(e) = def.edges.get(eid) else { continue };
            for w in [e.a, e.b] {
                if !seen.has(w) {
                    seen.add(w);
                    stack.push(w);
                }
            }
        }
    }
    seen
}

/// A face and its bounding edges (double-click).
pub fn face_with_edges(def: &mut Def, fid: f64) -> Vec<Sel> {
    let Some(f) = def.faces.get(fid).cloned() else { return Vec::new() };
    let mut out = vec![Sel::new("f", fid)];
    for (a, b) in loop_segs(&f) {
        if let Some(e) = edge_between(def, a, b) {
            out.push(Sel::new("e", e));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------
// Shapes

/// A closed polygon on a plane: its edges (re-facing closes it into a face). → edge ids.
pub fn add_polygon(def: &mut Def, pts: &[P3], hint: Option<P3>) -> Res<Vec<f64>> {
    let mut ids = Vec::new();
    let n = pts.len();
    for (i, p) in pts.iter().enumerate() {
        let q = pts.get((i + 1) % n).copied().unwrap_or(*p);
        ids.extend(add_edge(def, *p, q, EdgeOpts { no_faces: true, hint: None }));
    }
    let mut planes = candidate_planes(def, &ids);
    // `pts[0]` of an empty list: the JS throws a TypeError in planeFrom
    let Some(&p0) = pts.first() else { return Err(Error::Type) };
    let pl = plane_from(newell(pts), p0);
    if !planes.iter().any(|q| same_plane(q, &pl)) {
        planes.insert(0, pl);
    }
    let ne = IdSet::from_iter_ids(ids.iter().copied());
    for p in &planes {
        reface(def, p, &ne, hint);
    }
    Ok(ids)
}

/// Corners of a rectangle from corner p along unit axes u, v with sizes w, h.
pub fn rect_pts(p: P3, u: P3, v: P3, w: f64, h: f64) -> Vec<P3> {
    vec![p, add(p, mul(u, w)), add(add(p, mul(u, w)), mul(v, h)), add(p, mul(v, h))]
}
/// A regular polygon (or a circle with `sides` segments) centred at c, radius r, normal n, first corner along u.
pub fn ngon_pts(c: P3, n: P3, u: P3, r: f64, sides: f64) -> Vec<P3> {
    let v = cross(norm(n), u);
    let mut out = Vec::new();
    let mut i = 0.0;
    while i < sides && out.len() < 1_000_000 {
        let a = (i / sides) * std::f64::consts::PI * 2.0;
        out.push(add(c, add(mul(u, r * a.jcos()), mul(v, r * a.jsin()))));
        i += 1.0;
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------
// Curves (0.4 M3): arcs, circles and polygons are plain edges carrying one `curve` id

/// `CURVE_ID.test(s)`: /^(arc|circle|polygon|curve):\d{1,9}$/
pub fn is_curve_id(s: &str) -> bool {
    let Some((k, n)) = s.split_once(':') else { return false };
    matches!(k, "arc" | "circle" | "polygon" | "curve") && (1..=9).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_digit())
}

/// Points of an arc: centre c, unit normal n, first direction u, radius r, sweep (radians), segs pieces.
pub fn arc_pts(c: P3, n: P3, u: P3, r: f64, sweep: f64, segs: f64) -> Vec<P3> {
    let v = cross(norm(n), u);
    let mut out = Vec::new();
    let mut i = 0.0;
    while i <= segs && out.len() < 1_000_000 {
        let a = (sweep * i) / segs;
        out.push(add(c, add(mul(u, r * a.jcos()), mul(v, r * a.jsin()))));
        i += 1.0;
    }
    out
}

/// Tag as one curve every edge lying along the polyline pts (closed: back to the first point). → the curve id.
pub fn tag_curve(def: &mut Def, pts: &[P3], kind: &str, closed: bool) -> String {
    let id = format!("{kind}:{}", js::num_str(nid(def)));
    let n = pts.len();
    let mut segs = Vec::new();
    let mut i = 0usize;
    while i + 1 < n + usize::from(closed) {
        let a = pts.get(i).copied().unwrap_or([f64::NAN; 3]);
        let b = pts.get((i + 1) % n.max(1)).copied().unwrap_or([f64::NAN; 3]);
        segs.push((a, b));
        i += 1;
    }
    let on = |p: P3, a: P3, b: P3| {
        let u = sub(b, a);
        let l2 = dot(u, u);
        let t = if js::truthy(l2) { dot(sub(p, a), u) / l2 } else { 0.0 };
        (-1e-9..=1.0 + 1e-9).contains(&t) && dist(add(a, mul(u, t)), p) <= EPS
    };
    let verts = &def.verts;
    for e in def.edges.values_mut() {
        let p = verts.get(e.a).copied().unwrap_or([f64::NAN; 3]);
        let q = verts.get(e.b).copied().unwrap_or([f64::NAN; 3]);
        if segs.iter().any(|&(a, b)| on(p, a, b) && on(q, a, b)) {
            e.curve = Some(id.clone());
        }
    }
    id
}

/// Edge ids of a curve.
pub fn curve_edges(def: &Def, cid: &str) -> Vec<f64> {
    def.edges.iter().filter(|(_, e)| e.curve.as_deref() == Some(cid)).map(|(id, _)| id).collect()
}

/// What a curve is (curveInfo).
#[derive(Clone, Debug, PartialEq)]
pub struct CurveInfo {
    pub id: String,
    pub kind: String,
    pub edges: Vec<f64>,
    pub verts: Vec<f64>,
    pub closed: bool,
    pub length: f64,
    pub segs: f64,
    pub round: bool,
    pub center: Option<P3>,
    pub normal: Option<P3>,
    pub radius: f64,
    pub flat: bool,
    pub sweep: f64,
}

/// Adjacency of a set of edges: vertex → neighbours, in the JS's insertion order.
fn adjacency(def: &Def, eids: &[f64]) -> IdMap<Vec<f64>> {
    let mut adj: IdMap<Vec<f64>> = IdMap::new();
    for &id in eids {
        let Some(e) = def.edges.get(id) else { continue };
        for (x, y) in [(e.a, e.b), (e.b, e.a)] {
            match adj.get_mut(x) {
                Some(l) => l.push(y),
                None => adj.set(x, vec![y]),
            }
        }
    }
    adj
}

pub fn curve_info(def: &Def, cid: &str) -> Option<CurveInfo> {
    let eids = curve_edges(def, cid);
    if eids.is_empty() {
        return None;
    }
    let adj = adjacency(def, &eids);
    let mut length = 0.0;
    for &id in &eids {
        if let Some(e) = def.edges.get(id) {
            length += dist(def.v(e.a), def.v(e.b));
        }
    }
    let ends: Vec<f64> = adj.iter().filter(|(_, l)| l.len() == 1).map(|(v, _)| v).collect();
    let closed = ends.is_empty() && adj.values().all(|l| l.len() == 2);
    let mut chain: Vec<f64> = Vec::new();
    let mut cur = ends.first().copied().or_else(|| adj.first_key());
    let mut prev: Option<f64> = None;
    while let Some(c) = cur {
        if chain.len() > adj.len() {
            break;
        }
        chain.push(c);
        let nx = adj.get(c).and_then(|l| l.iter().copied().find(|&x| Some(x) != prev && !chain.contains(&x)));
        prev = Some(c);
        cur = nx;
    }
    let p: Vec<P3> = chain.iter().map(|&v| def.v(v)).collect();
    let mut out = CurveInfo {
        id: cid.to_string(),
        kind: cid.split(':').next().unwrap_or("").to_string(),
        edges: eids.clone(),
        verts: chain.clone(),
        closed,
        length,
        segs: eids.len() as f64,
        round: false,
        center: None,
        normal: None,
        radius: 0.0,
        flat: false,
        sweep: 0.0,
    };
    if p.len() >= 3 && chain.len() == adj.len() {
        let pl = p.len();
        let at = |i: usize| p.get(i).copied().unwrap_or([f64::NAN; 3]);
        let a = at(0);
        let b = at(pl / 3);
        let c = at((2 * pl) / 3);
        let ab = sub(b, a);
        let ac = sub(c, a);
        let n = cross(ab, ac);
        let nn = dot(n, n);
        if nn > 1e-24 {
            let cc = add(a, mul(add(mul(cross(n, ab), dot(ac, ac)), mul(cross(ac, n), dot(ab, ab))), 1.0 / (2.0 * nn)));
            let r = dist(cc, a);
            let nrm = norm(n);
            let tol = js::max(EPS, r * 1e-7);
            if p.iter().all(|q| (dist(*q, cc) - r).abs() <= tol && dot(sub(*q, cc), nrm).abs() <= tol) {
                out.round = true;
                out.center = Some(cc);
                out.normal = Some(nrm);
                out.radius = r;
                let ef = edge_faces(def);
                out.flat = eids.iter().all(|&id| {
                    let Some(e) = def.edges.get(id) else { return true };
                    ef_get(&ef, e.a, e.b)
                        .iter()
                        .all(|&fid| def.faces.get(fid).is_some_and(|f| dot(face_normal(def, f), nrm).abs() > 1.0 - 1e-9))
                });
                let mut sw = 0.0;
                let mut i = 0usize;
                while i + 1 < pl + usize::from(closed) {
                    let u1 = norm(sub(at(i), cc));
                    let u2 = norm(sub(at((i + 1) % pl), cc));
                    sw += dot(cross(u1, u2), nrm).jatan2(dot(u1, u2));
                    i += 1;
                }
                out.sweep = sw.abs();
            }
        }
    }
    Some(out)
}

/// Give a round, flat curve a new radius about its centre (its points move; faces in its plane follow).
pub fn set_curve_radius(def: &mut Def, cid: &str, r: f64) -> Res<Option<CurveInfo>> {
    let ci = curve_info(def, cid);
    let Some(ci) = ci.filter(|c| c.round) else { return Err(msg("m3d.model.only_arcs_circles_and_polygons_have")) };
    if !ci.flat {
        return Err(msg("m3d.model.this_curve_bounds_a_face_that"));
    }
    if !(r > EPS) {
        return Err(msg("m3d.model.type_a_radius_above_zero"));
    }
    let k = r / ci.radius;
    let c = ci.center.unwrap_or([f64::NAN; 3]);
    for &v in &ci.verts {
        let p = add(c, mul(sub(def.v(v), c), k));
        def.verts.set(v, p);
    }
    Ok(curve_info(def, cid))
}

// ---------------------------------------------------------------------------------------------------------------
// Smooth curves (0.4 M27)

pub const SPLINE_SEGS: f64 = 8.0;

/// Points along a centripetal Catmull–Rom spline through P, segs per span; closed wraps round.
pub fn smooth_pts(pp: &[P3], closed: bool, segs: f64) -> Vec<P3> {
    let n = pp.len();
    if n < 3 {
        return pp.to_vec();
    }
    let g = |i: usize| pp.get(i).copied().unwrap_or([f64::NAN; 3]);
    let at = |i: isize| -> P3 {
        let ni = isize::try_from(n).unwrap_or(isize::MAX);
        if closed {
            g(usize::try_from((i + ni).rem_euclid(ni)).unwrap_or(0))
        } else if i < 0 {
            sub(mul(g(0), 2.0), g(1))
        } else if i >= ni {
            sub(mul(g(n - 1), 2.0), g(n - 2))
        } else {
            g(usize::try_from(i).unwrap_or(0))
        }
    };
    let lerp = |a: P3, b: P3, ta: f64, tb: f64, t: f64| add(mul(a, (tb - t) / (tb - ta)), mul(b, (t - ta) / (tb - ta)));
    let tj = |a: P3, b: P3| js::max(1e-9, dist(a, b).sqrt());
    let mut out = Vec::new();
    let spans = if closed { n } else { n - 1 };
    for i in 0..spans {
        let ii = isize::try_from(i).unwrap_or(0);
        let (p0, p1, p2, p3) = (at(ii - 1), at(ii), at(ii + 1), at(ii + 2));
        let t1 = tj(p0, p1);
        let t2 = t1 + tj(p1, p2);
        let t3 = t2 + tj(p2, p3);
        let mut k = 0.0;
        while k < segs && out.len() < 1_000_000 {
            let t = t1 + ((t2 - t1) * k) / segs;
            let a1 = lerp(p0, p1, 0.0, t1, t);
            let a2 = lerp(p1, p2, t1, t2, t);
            let a3 = lerp(p2, p3, t2, t3, t);
            let b1 = lerp(a1, a2, 0.0, t2, t);
            let b2 = lerp(a2, a3, t1, t3, t);
            out.push(lerp(b1, b2, t1, t2, t));
            k += 1.0;
        }
    }
    if !closed {
        out.push(g(n - 1));
    }
    out
}

/// Draw a smooth curve through pts (def coordinates) as edges. → its curve id.
pub fn add_spline(def: &mut Def, pts: &[P3], closed: bool, segs: f64, hint: Option<P3>) -> Res<String> {
    if pts.len() < 2 {
        return Err(msg("m3d.model.a_curve_needs_two_points_or"));
    }
    let s = smooth_pts(pts, closed, segs);
    let n = s.len();
    let mut i = 0usize;
    while i + 1 < n + usize::from(closed) {
        let a = s.get(i).copied().unwrap_or([f64::NAN; 3]);
        let b = s.get((i + 1) % n).copied().unwrap_or([f64::NAN; 3]);
        add_edge(def, a, b, EdgeOpts { no_faces: false, hint });
        i += 1;
    }
    let id = tag_curve(def, &s, "curve", closed);
    def.splines.set(id.clone(), Spline { pts: pts.to_vec(), closed, segs });
    Ok(id)
}

/// Redraw curve cid through new points (it must not bound a face).
pub fn edit_spline(def: &mut Def, cid: &str, pts: &[P3], closed: Option<bool>, segs: Option<f64>) -> Res<String> {
    let Some(sp) = def.splines.get(cid).cloned() else { return Err(msg("m3d.model.this_curve_can_no_longer_be")) };
    let ef = edge_faces(def);
    let own = curve_edges(def, cid);
    if own.iter().any(|&id| def.edges.get(id).is_some_and(|e| !ef_get(&ef, e.a, e.b).is_empty())) {
        return Err(msg("m3d.model.this_curve_bounds_a_face_erase"));
    }
    for &id in &own {
        e_remove(def, id);
    }
    remove_loose_verts(def);
    let c = closed.unwrap_or(sp.closed);
    let n = segs.unwrap_or(sp.segs);
    let s = smooth_pts(pts, c, n);
    let len_s = s.len();
    let mut i = 0usize;
    while i + 1 < len_s + usize::from(c) {
        let a = s.get(i).copied().unwrap_or([f64::NAN; 3]);
        let b = s.get((i + 1) % len_s.max(1)).copied().unwrap_or([f64::NAN; 3]);
        add_edge(def, a, b, EdgeOpts { no_faces: true, hint: None });
        i += 1;
    }
    let nid2 = tag_curve(def, &s, "curve", c);
    for e in def.edges.values_mut() {
        if e.curve.as_deref() == Some(nid2.as_str()) {
            e.curve = Some(cid.to_string());
        }
    }
    def.splines.set(cid.to_string(), Spline { pts: pts.to_vec(), closed: c, segs: n });
    Ok(cid.to_string())
}

// ---------------------------------------------------------------------------------------------------------------
// Hide and isolate (0.4 M17)

/// Hide (on = true) or show the selected edges, faces and placements of def.
pub fn hide_sel(def: &mut Def, sel: &[Sel], on: bool) {
    for s in sel {
        if s.is("e") {
            if let Some(o) = def.edges.get_mut(s.id) {
                o.hidden = on;
            }
        } else if s.is("f") {
            if let Some(o) = def.faces.get_mut(s.id) {
                o.hidden = on;
            }
        } else if s.is("i")
            && let Some(o) = def.insts.get_mut(s.id)
        {
            o.hidden = on;
        }
    }
}
/// Hide everything in def that is not selected (Isolate). → how many things it hid.
pub fn isolate_sel(def: &mut Def, sel: &[Sel]) -> f64 {
    let tid = |t: &str, id: f64| {
        let mut s = t.to_string();
        js::push_num(&mut s, id);
        s
    };
    let mut keep: std::collections::HashSet<String> = sel.iter().map(|s| tid(&s.t, s.id)).collect();
    // a selected face keeps its edges, so it still reads as a shape
    for s in sel {
        if !s.is("f") {
            continue;
        }
        let Some(f) = def.faces.get(s.id).cloned() else { continue };
        for l in &f.loops {
            let n = l.len();
            for (i, &a) in l.iter().enumerate() {
                let b = l.get((i + 1) % n).copied().unwrap_or(a);
                if let Some(id) = edge_between(def, a, b) {
                    keep.insert(tid("e", id));
                }
            }
        }
    }
    let mut n = 0.0;
    for (id, o) in def.edges.iter_mut() {
        if !keep.contains(&tid("e", id)) && !o.hidden {
            o.hidden = true;
            n += 1.0;
        }
    }
    for (id, o) in def.faces.iter_mut() {
        if !keep.contains(&tid("f", id)) && !o.hidden {
            o.hidden = true;
            n += 1.0;
        }
    }
    for (id, o) in def.insts.iter_mut() {
        if !keep.contains(&tid("i", id)) && !o.hidden {
            o.hidden = true;
            n += 1.0;
        }
    }
    n
}

// ---------------------------------------------------------------------------------------------------------------
// Flip (0.4 M15)

/// The mirror through the plane at point c with unit normal n.
pub fn mirror_matrix(c: P3, n: P3) -> M4 {
    let [x, y, z] = norm(n);
    let l = [
        1.0 - 2.0 * x * x,
        -2.0 * x * y,
        -2.0 * x * z,
        0.0,
        -2.0 * x * y,
        1.0 - 2.0 * y * y,
        -2.0 * y * z,
        0.0,
        -2.0 * x * z,
        -2.0 * y * z,
        1.0 - 2.0 * z * z,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ];
    mmul(&translation(c), &mmul(&l, &translation(mul(c, -1.0))))
}
/// Flip a selection through the plane at c with normal n; copy: flip a copy. → the flipped selection.
pub fn flip_sel(def: &mut Def, sel: &[Sel], c: P3, n: P3, copy: bool) -> Vec<Sel> {
    let target = if copy { copy_sel(def, sel) } else { sel.to_vec() };
    transform_sel(def, &target, &mirror_matrix(c, n));
    target
}

// ---------------------------------------------------------------------------------------------------------------
// Follow me (0.4 M10)

/// The vertices of a chain of edges in order (verts, closed), or an error when they branch or do not connect.
pub fn edge_chain(def: &Def, eids: &[f64]) -> Res<(Vec<f64>, bool)> {
    let adj = adjacency(def, eids);
    if adj.is_empty() {
        return Err(msg("m3d.model.choose_a_path_one_or_more"));
    }
    if adj.values().any(|l| l.len() > 2) {
        return Err(msg("m3d.model.the_path_branches_select_one_chain"));
    }
    let ends: Vec<f64> = adj.iter().filter(|(_, l)| l.len() == 1).map(|(v, _)| v).collect();
    let mut verts: Vec<f64> = Vec::new();
    let mut cur = ends.first().copied().or_else(|| adj.first_key());
    let mut prev: Option<f64> = None;
    while let Some(c) = cur {
        if verts.contains(&c) {
            break;
        }
        verts.push(c);
        let nx = adj.get(c).and_then(|l| l.iter().copied().find(|&x| Some(x) != prev && !verts.contains(&x)));
        prev = Some(c);
        cur = nx;
    }
    if verts.len() != adj.len() {
        return Err(msg("m3d.model.the_path_is_in_more_than"));
    }
    Ok((verts, ends.is_empty()))
}

/// The longest unbranched chain of edges through edge eid (stops where three or more edges meet).
pub fn chain_through(def: &Def, eid: f64) -> Vec<f64> {
    let ve = vert_edges(def);
    let Some(e0) = def.edges.get(eid) else { return Vec::new() };
    let mut out = IdSet::new();
    out.add(eid);
    for start in [e0.a, e0.b] {
        let mut v = start;
        let mut from = eid;
        for _ in 0..100_000 {
            let es: Vec<f64> = ve_get(&ve, v).iter().copied().filter(|&x| x != from).collect();
            let [e1] = es.as_slice() else { break };
            if out.has(*e1) {
                break;
            }
            out.add(*e1);
            let Some(e) = def.edges.get(*e1) else { break };
            v = if e.a == v { e.b } else { e.a };
            from = *e1;
        }
    }
    out.to_vec()
}

/// What followMe made.
#[derive(Clone, Debug, PartialEq)]
pub struct FollowResult {
    pub faces: Vec<f64>,
    pub closed: bool,
}

/// Sweep face fid along the edges path_eids (same definition): the profile moves along the path, its rings meeting
/// at the mitre plane of each path corner. Open path: caps at both ends. Closed path: a ring (the profile is used up).
pub fn follow_me(def: &mut Def, fid: f64, path_eids: &[f64]) -> Res<FollowResult> {
    let Some(f) = def.faces.get(fid).cloned() else { return Err(msg("m3d.model.choose_the_face_to_sweep")) };
    let (verts, closed) = edge_chain(def, path_eids)?;
    let mut pp: Vec<P3> = verts.iter().map(|&v| def.v(v)).collect();
    let np = face_normal(def, &f);
    let l0 = f.loops.first().cloned().unwrap_or_default();
    let p0 = l0.first().map_or([f64::NAN; 3], |&v| def.v(v));
    let cen = {
        let a: Vec<P3> = l0.iter().map(|&v| def.v(v)).collect();
        let s = a.iter().fold([0.0, 0.0, 0.0], |acc, q| add(acc, *q));
        mul(s, 1.0 / a.len() as f64)
    };
    if !closed {
        if let (Some(first), Some(last)) = (pp.first(), pp.last())
            && dist(*last, cen) < dist(*first, cen)
        {
            pp.reverse();
        }
    } else {
        // start the ring at the path corner nearest the profile
        let mut k = 0usize;
        for (i, q) in pp.iter().enumerate() {
            if dist(*q, cen) < dist(pp.get(k).copied().unwrap_or([f64::NAN; 3]), cen) {
                k = i;
            }
        }
        let kk = k.min(pp.len());
        pp.rotate_left(kk);
    }
    let pl = pp.len();
    let nseg = if closed { pl } else { pl.saturating_sub(1) };
    if nseg < 1 {
        return Err(msg("m3d.model.the_path_needs_at_least_one"));
    }
    let at = |i: usize| pp.get(i).copied().unwrap_or([f64::NAN; 3]);
    let mut dirs = Vec::new();
    for i in 0..nseg {
        let d = norm(sub(at((i + 1) % pl), at(i)));
        if !js::truthy(len(d)) {
            return Err(msg("m3d.model.the_path_has_an_edge_of"));
        }
        dirs.push(d);
    }
    let dir = |i: usize| dirs.get(i).copied().unwrap_or([f64::NAN; 3]);
    if dot(dir(0), np).abs() < 1e-3 {
        return Err(msg("m3d.model.the_face_lies_along_the_path"));
    }
    // rings: [loop][point]; ring 0 = the profile itself
    let mut ring: Vec<Vec<P3>> = f.loops.iter().map(|l| l.iter().map(|&v| def.v(v)).collect()).collect();
    let mut rings: Vec<Vec<Vec<f64>>> = vec![f.loops.clone()];
    for k in 1..=nseg {
        let d = dir(k - 1);
        let (n, o);
        if closed && k == nseg {
            n = np;
            o = p0;
        } else if !closed && k == nseg {
            n = d;
            o = at(k);
        } else {
            let dn = dir(k % nseg);
            if dot(d, dn) < -0.999 {
                return Err(msg("m3d.model.the_path_turns_straight_back_on"));
            }
            n = norm(add(d, dn));
            o = at(k);
        }
        let nd = dot(n, d);
        if nd.abs() < 1e-9 {
            return Err(msg("m3d.model.the_path_turns_too_sharply_here"));
        }
        ring = ring.iter().map(|l| l.iter().map(|q| add(*q, mul(d, dot(n, sub(o, *q)) / nd))).collect()).collect();
        if closed && k == nseg {
            let r0 = rings.first().cloned().unwrap_or_default();
            rings.push(r0);
        } else {
            let ids: Vec<Vec<f64>> = ring.iter().map(|l| l.iter().map(|q| v_add(def, *q)).collect()).collect();
            rings.push(ids);
        }
    }
    let mut added = Vec::new();
    for k in 0..nseg {
        let (Some(ra), Some(rb)) = (rings.get(k).cloned(), rings.get(k + 1).cloned()) else { continue };
        for (l, la) in ra.iter().enumerate() {
            let m = la.len();
            let lb = rb.get(l).cloned().unwrap_or_default();
            let g = |v: &[f64], i: usize| v.get(i).copied().unwrap_or(f64::NAN);
            for j in 0..m {
                let q = [g(la, j), g(la, (j + 1) % m), g(&lb, (j + 1) % m), g(&lb, j)];
                let mut uniq: Vec<f64> = Vec::new();
                for x in q {
                    if !uniq.iter().any(|&y| kb(y) == kb(x)) {
                        uniq.push(x);
                    }
                }
                if uniq.len() < 3 {
                    continue;
                }
                let lp = if uniq.len() == 4 { q.to_vec() } else { uniq };
                added.push(f_add(def, vec![lp], f.color.clone(), None, false));
            }
        }
    }
    if closed {
        def.faces.delete(fid);
    } else {
        let last = rings.get(nseg).cloned().unwrap_or_default();
        added.push(f_add(def, last, f.color.clone(), None, false));
    }
    // outward: the body's volume must come out positive
    let body: Vec<f64> = if closed { added.clone() } else { std::iter::once(fid).chain(added.iter().copied()).collect() };
    let mut vol = 0.0;
    for &id in &body {
        let Some(bf) = def.faces.get(id) else { continue };
        for [a, b, c] in face_tris(def, bf) {
            vol += dot(def.v(a), cross(def.v(b), def.v(c))) / 6.0;
        }
    }
    if vol < 0.0 {
        reverse_faces(def, &body);
    }
    // smooth: an edge between two new faces less than 25° apart is soft
    let ef = edge_faces(def);
    let mine = IdSet::from_iter_ids(body.iter().copied());
    let c25 = (25.0 * std::f64::consts::PI / 180.0).jcos();
    let mut soft = Vec::new();
    for (eid, e) in def.edges.iter() {
        let fs = ef_get(&ef, e.a, e.b);
        if let [f0, f1] = fs
            && mine.has(*f0)
            && mine.has(*f1)
            && let (Some(fa), Some(fb)) = (def.faces.get(*f0), def.faces.get(*f1))
            && dot(face_normal(def, fa), face_normal(def, fb)) > c25
        {
            soft.push(eid);
        }
    }
    for eid in soft {
        if let Some(e) = def.edges.get_mut(eid) {
            e.soft = true;
        }
    }
    Ok(FollowResult { faces: added, closed })
}
