//! Edges, vertices and re-facing: the "sticky" geometry of a push/pull modeller (model.js §§ Edges and vertices,
//! Clean-up). addEdge splits the edges and vertices it meets; when the new edge closes a loop on a plane, or crosses
//! a face from edge to edge, the plane is re-faced (`reface`): every region of the plane's edge arrangement inside an
//! old face stays a face (same id, same side), regions inside two faces facing opposite ways cancel (that is how a
//! push through a solid opens a hole), and a region newly closed by the new edge becomes a face. Loops inside faces
//! become holes.

use crate::def::*;
use crate::js;
use crate::jsmath::JsMath;
use crate::omap::{FxMap, IdMap, IdSet, OMap, kb};
use crate::vec::*;

/// Split edge `eid` at vertex `v` (which lies on it); face loops get the vertex too.
pub fn split_edge(def: &mut Def, eid: f64, v: f64) {
    let Some(e) = def.edges.get(eid).cloned() else { return };
    if e.a == v || e.b == v {
        return;
    }
    e_remove(def, eid);
    e_add(def, e.a, v, e.soft, e.curve.clone(), e.hidden);
    e_add(def, v, e.b, e.soft, e.curve.clone(), e.hidden);
    for f in def.faces.values_mut() {
        for l in f.loops.iter_mut() {
            let n = l.len();
            for i in 0..n {
                let p = l.get(i).copied().unwrap_or(f64::NAN);
                let q = l.get((i + 1) % n).copied().unwrap_or(f64::NAN);
                if (p == e.a && q == e.b) || (p == e.b && q == e.a) {
                    l.insert(i + 1, v);
                    break;
                }
            }
        }
    }
}

/// The vertex at p: an existing one, or a new one (splitting the edge it lands on).
pub fn vertex_at(def: &mut Def, p: P3) -> f64 {
    if let Some(have) = find_vert(def, p, EPS) {
        return have;
    }
    let id = v_add(def, p);
    let mut hit = None;
    for (eid, e) in def.edges.iter() {
        let a = def.v(e.a);
        let b = def.v(e.b);
        if !near_box(p, a, b, EPS) {
            continue;
        }
        let u = sub(b, a);
        let l2 = dot(u, u);
        let s = dot(sub(p, a), u) / l2;
        if s <= 0.0 || s >= 1.0 {
            continue;
        }
        if dist(add(a, mul(u, s)), p) <= EPS {
            hit = Some(eid);
            break;
        }
    }
    if let Some(eid) = hit {
        split_edge(def, eid, id);
    }
    id
}

/// What addEdge takes besides its two points.
#[derive(Clone, Copy, Debug, Default)]
pub struct EdgeOpts {
    /// `faces: false`: draw the edges only, no re-facing.
    pub no_faces: bool,
    /// The side a new face faces.
    pub hint: Option<P3>,
}

/// Draw an edge from p to q. Splits what it meets; re-faces the planes it lies on. → the ids of the edges along p–q.
pub fn add_edge(def: &mut Def, p: P3, q: P3, opts: EdgeOpts) -> Vec<f64> {
    if dist(p, q) <= EPS {
        return Vec::new();
    }
    let a = vertex_at(def, p);
    let b = vertex_at(def, q);
    if a == b {
        return Vec::new();
    }
    let pa = def.v(a);
    let pb = def.v(b);
    let u = sub(pb, pa);
    let l2 = dot(u, u);
    let mut stops: Vec<(f64, f64)> = vec![(0.0, a), (1.0, b)];
    for (vid, pp) in def.verts.iter() {
        if vid == a || vid == b || !near_box(*pp, pa, pb, EPS) {
            continue;
        }
        let s = dot(sub(*pp, pa), u) / l2;
        if s <= 0.0 || s >= 1.0 {
            continue;
        }
        if dist(add(pa, mul(u, s)), *pp) <= EPS {
            stops.push((s, vid));
        }
    }
    let snapshot: Vec<(f64, f64, f64)> = def.edges.iter().map(|(id, e)| (id, e.a, e.b)).collect();
    for (eid, ea, eb) in snapshot {
        if ea == a || ea == b || eb == a || eb == b {
            continue;
        }
        if stops.iter().any(|st| st.1 == ea || st.1 == eb) {
            continue;
        }
        let c = def.v(ea);
        let d = def.v(eb);
        let w = sub(d, c);
        // Spec gap (as the JS does it): segSeg lets a zero-length edge through with NaNs (0/0 passes its `<=` tests)
        // and the edge is split at a NaN point; the box test only skips what segSeg would refuse
        let ww = dot(w, w);
        if ww > 0.0 && ww.is_finite() && l2 > 0.0 && l2.is_finite() && !boxes_near(pa, pb, c, d, EPS) {
            continue;
        }
        let Some((s, _t, rp)) = seg_seg(pa, u, c, w) else { continue };
        let v = v_add(def, rp);
        split_edge(def, eid, v);
        stops.push((s, v));
    }
    js::sort_by(&mut stops, |x, y| x.0 - y.0);
    let mut ids = Vec::new();
    for w in stops.windows(2) {
        if let [x, y] = w {
            if x.1 == y.1 {
                continue;
            }
            if let Some(id) = e_add0(def, x.1, y.1) {
                ids.push(id);
            }
        }
    }
    if !opts.no_faces && !ids.is_empty() {
        let ne = IdSet::from_iter_ids(ids.iter().copied());
        for pl in candidate_planes(def, &ids) {
            reface(def, &pl, &ne, opts.hint);
        }
    }
    ids
}

/// Proper crossing of segments A + s·u and C + t·w (both strictly inside), coplanar within EPS → (s, t, point).
pub fn seg_seg(a: P3, u: P3, c: P3, w: P3) -> Option<(f64, f64, P3)> {
    let n = cross(u, w);
    let nn = dot(n, n);
    if nn < 1e-18 * dot(u, u) * dot(w, w) {
        return None;
    }
    let ac = sub(c, a);
    let s = dot(cross(ac, w), n) / nn;
    let t = dot(cross(ac, u), n) / nn;
    let tol_s = EPS / dot(u, u).sqrt();
    let tol_t = EPS / dot(w, w).sqrt();
    if s <= tol_s || s >= 1.0 - tol_s || t <= tol_t || t >= 1.0 - tol_t {
        return None;
    }
    let p1 = add(a, mul(u, s));
    let p2 = add(c, mul(w, t));
    if dist(p1, p2) > EPS {
        return None;
    }
    Some((s, t, p1))
}

/// Planes an edge could close a face on: planes of faces it lies on, and of each neighbour edge.
pub fn candidate_planes(def: &Def, eids: &[f64]) -> Vec<Plane> {
    let mut planes: Vec<Plane> = Vec::new();
    let put = |planes: &mut Vec<Plane>, pl: Plane| {
        if !planes.iter().any(|q| same_plane(q, &pl)) {
            planes.push(pl);
        }
    };
    let ve = vert_edges(def);
    // the faces' planes do not change while the candidates are gathered
    let mut fplanes: Option<Vec<Plane>> = None;
    for &eid in eids {
        let Some(e) = def.edges.get(eid) else { continue };
        let pa = def.v(e.a);
        let pb = def.v(e.b);
        let u = sub(pb, pa);
        let fp = fplanes.get_or_insert_with(|| def.faces.values().map(|f| face_plane(def, f)).collect());
        for pl in fp.iter() {
            let pl = *pl;
            if js::truthy(len(pl.n)) && on_plane(&pl, pa, EPS) && on_plane(&pl, pb, EPS) {
                put(&mut planes, plane_from(pl.n, pa));
            }
        }
        for end in [e.a, e.b] {
            for &id2 in ve_get(&ve, end) {
                if id2 == eid {
                    continue;
                }
                let Some(e2) = def.edges.get(id2) else { continue };
                let c = def.v(if e2.a == end { e2.b } else { e2.a });
                let n = cross(u, sub(c, pa));
                if len(n) > 1e-9 * len(u) * dist(c, pa) {
                    put(&mut planes, plane_from(n, pa));
                }
            }
        }
    }
    planes
}

struct Old {
    fid: f64,
    color: Option<String>,
    hidden: bool,
    s: f64,
    poly: Vec<Vec<P2>>,
}
struct Cycle {
    verts: Vec<f64>,
    edges: Vec<f64>,
    poly: Vec<P2>,
    area: f64,
}

/// Rebuild the faces of one plane from its edge arrangement (see the module header). `new_edges` may close new faces;
/// `hint` (a direction) picks the side a new face faces. → whether the plane had anything to re-face.
pub fn reface(def: &mut Def, plane: &Plane, new_edges: &IdSet, hint: Option<P3>) -> bool {
    let fr = frame(plane.n);
    // the vertices on the plane, and their 2D places (a lookup: its order does not matter)
    let mut on_p = crate::dense::VSet::new(&def.verts);
    let mut p2: crate::dense::DMap<P2> = crate::dense::DMap::new();
    for (s, id, p) in def.verts.iter_slots() {
        if on_plane(plane, *p, EPS) {
            on_p.add_slot(s);
            p2.insert(id, to2(&fr, *p));
        }
    }
    let mut old: Vec<Old> = Vec::new();
    for (fid, f) in def.faces.iter() {
        if !f.loops.iter().all(|l| l.iter().all(|&v| on_p.has(v))) {
            continue;
        }
        let n = face_normal(def, f);
        if !js::truthy(len(n)) || dot(n, plane.n).abs() < 1.0 - 1e-6 {
            continue;
        }
        old.push(Old { fid, color: f.color.clone(), hidden: f.hidden, s: js::sign(dot(n, plane.n)), poly: loops2(def, f, &fr) });
    }
    let mut e_list = Vec::new();
    for (eid, e) in def.edges.iter() {
        if on_p.has(e.a) && on_p.has(e.b) {
            e_list.push(eid);
        }
    }
    if old.is_empty() && !e_list.iter().any(|&id| new_edges.has(id)) {
        return false;
    }

    let mut live = IdSet::from_iter_ids(e_list.iter().copied());
    let mut cycles: Vec<Cycle> = Vec::new();
    for _guard in 0..200 {
        prune_dangling(def, &mut live);
        cycles = walk_cycles(def, &live, &p2);
        let mut bridges = IdSet::new();
        for c in &cycles {
            let mut seen = IdSet::new();
            for &e in &c.edges {
                if seen.has(e) {
                    bridges.add(e);
                }
                seen.add(e);
            }
        }
        if bridges.is_empty() {
            break;
        }
        for b in bridges.iter() {
            live.delete(b);
        }
    }
    let pos: Vec<&Cycle> = cycles.iter().filter(|c| c.area > EPS2).collect();
    let negs: Vec<&Cycle> = cycles.iter().filter(|c| c.area < -EPS2).collect();
    let mut holes_of: Vec<Vec<usize>> = vec![Vec::new(); pos.len()];
    let pos_sets: Vec<IdSet> = pos.iter().map(|c| IdSet::from_iter_ids(c.verts.iter().copied())).collect();
    for (hi, h) in negs.iter().enumerate() {
        let mut best: Option<usize> = None;
        for (ci, c) in pos.iter().enumerate() {
            let Some(vs) = pos_sets.get(ci) else { continue };
            let Some(probe) = h.verts.iter().copied().find(|&v| !vs.has(v)) else { continue };
            let pp = p2.get(probe).copied().unwrap_or([f64::NAN; 2]);
            if !in_poly(pp, &c.poly) {
                continue;
            }
            if best.is_none_or(|b| pos.get(b).is_some_and(|bc| c.area < bc.area)) {
                best = Some(ci);
            }
        }
        if let Some(b) = best
            && let Some(list) = holes_of.get_mut(b)
        {
            list.push(hi);
        }
    }
    struct Made {
        fid: Option<f64>,
        loops: Vec<Vec<f64>>,
        color: Option<String>,
        hidden: bool,
    }
    let mut made: Vec<Made> = Vec::new();
    let mut reused = IdSet::new();
    for (ci, c) in pos.iter().enumerate() {
        let holes: Vec<&Cycle> = holes_of.get(ci).map(|l| l.iter().filter_map(|&h| negs.get(h).copied()).collect()).unwrap_or_default();
        let mut region: Vec<&[P2]> = vec![&c.poly];
        region.extend(holes.iter().map(|h| h.poly.as_slice()));
        let Some(pt) = sample_point(&region) else { continue };
        let mut inherit: Option<usize> = None;
        let mut sgn = 0.0;
        let mut mixed = false;
        for (oi, o) in old.iter().enumerate() {
            if !in_region(pt, &o.poly) {
                continue;
            }
            if inherit.is_none() {
                inherit = Some(oi);
            }
            if js::truthy(sgn) && sgn != o.s {
                mixed = true;
            }
            sgn = o.s;
        }
        if mixed {
            continue;
        }
        if inherit.is_none() {
            if !c.edges.iter().any(|&e| new_edges.has(e)) {
                continue;
            }
            sgn = if hint.is_some_and(|h| dot(h, plane.n) < 0.0) { -1.0 } else { 1.0 };
        }
        let mut loops: Vec<Vec<f64>> = vec![c.verts.clone()];
        loops.extend(holes.iter().map(|h| h.verts.clone()));
        if sgn < 0.0 {
            for l in loops.iter_mut() {
                l.reverse();
            }
        }
        let inh = inherit.and_then(|i| old.get(i));
        let fid = inh.map(|o| o.fid).filter(|f| !reused.has(*f));
        if let Some(f) = fid {
            reused.add(f);
        }
        made.push(Made { fid, loops, color: inh.and_then(|o| o.color.clone()), hidden: inh.is_some_and(|o| o.hidden) });
    }
    for o in &old {
        def.faces.delete(o.fid);
    }
    for m in made {
        f_add(def, m.loops, m.color, m.fid, m.hidden);
    }
    true
}

fn prune_dangling(def: &Def, live: &mut IdSet) {
    let mut changed = true;
    while changed {
        changed = false;
        let mut deg: FxMap<u64, u32> = FxMap::default();
        for id in live.iter() {
            if let Some(e) = def.edges.get(id) {
                *deg.entry(kb(e.a)).or_insert(0) += 1;
                *deg.entry(kb(e.b)).or_insert(0) += 1;
            }
        }
        for id in live.to_vec() {
            let Some(e) = def.edges.get(id) else { continue };
            let da = deg.get(&kb(e.a)).copied().unwrap_or(0);
            let db = deg.get(&kb(e.b)).copied().unwrap_or(0);
            if da < 2 || db < 2 {
                live.delete(id);
                changed = true;
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Half {
    from: f64,
    to: f64,
    e: f64,
    ang: f64,
}

/// Faces of a planar graph: each cycle keeps its region on the left (CCW positive area).
fn walk_cycles(def: &Def, live: &IdSet, p2: &crate::dense::DMap<P2>) -> Vec<Cycle> {
    let at = |v: f64| p2.get(v).copied().unwrap_or([f64::NAN; 2]);
    let mut out: IdMap<Vec<Half>> = IdMap::new();
    for id in live.iter() {
        let Some(e) = def.edges.get(id) else { continue };
        for (u, v) in [(e.a, e.b), (e.b, e.a)] {
            let p = at(u);
            let q = at(v);
            let h = Half { from: u, to: v, e: id, ang: (q[1] - p[1]).jatan2(q[0] - p[0]) };
            match out.get_mut(u) {
                Some(l) => l.push(h),
                None => out.set(u, vec![h]),
            }
        }
    }
    for l in out.values_mut() {
        js::sort_by(l, |x, y| x.ang - y.ang);
    }
    let mut seen: crate::omap::FxMap<(u64, u64), ()> = FxMap::default();
    let hk = |h: &Half| (kb(h.from), kb(h.to));
    let mut cycles = Vec::new();
    for l in out.values() {
        for h0 in l {
            if seen.contains_key(&hk(h0)) {
                continue;
            }
            let mut verts = Vec::new();
            let mut edges = Vec::new();
            let mut h = *h0;
            let mut guard = 0u32;
            while !seen.contains_key(&hk(&h)) && guard < 100_000 {
                guard += 1;
                seen.insert(hk(&h), ());
                verts.push(h.from);
                edges.push(h.e);
                let Some(list) = out.get(h.to) else { break };
                let n = list.len();
                let i = list.iter().position(|x| x.to == h.from).map_or(-1, |i| i64::try_from(i).unwrap_or(-1));
                let ni = i64::try_from(n).unwrap_or(1).max(1);
                let k = usize::try_from((i - 1 + ni).rem_euclid(ni)).unwrap_or(0);
                let Some(nh) = list.get(k) else { break };
                h = *nh;
            }
            let poly: Vec<P2> = verts.iter().map(|&v| at(v)).collect();
            let area = area2(&poly);
            cycles.push(Cycle { verts, edges, poly, area });
        }
    }
    cycles
}

// ---------------------------------------------------------------------------------------------------------------
// Clean-up

/// Forget the points of curves that have no edges left (erased, or moved into a group).
pub fn prune_splines(def: &mut Def) {
    if def.splines.is_empty() {
        return;
    }
    let live: std::collections::HashSet<&str> = def.edges.values().filter_map(|e| e.curve.as_deref()).collect();
    let dead: Vec<String> = def.splines.keys().filter(|k| !live.contains(k.as_str())).cloned().collect();
    for k in dead {
        def.splines.delete(&k);
    }
}

/// Merge coincident vertices, drop zero-length edges and degenerate faces, delete loose vertices. → whether anything
/// changed.
pub fn cleanup(def: &mut Def) -> bool {
    prune_splines(def);
    let mut ids: Vec<(f64, P3)> = def.verts.iter().map(|(id, p)| (id, *p)).collect();
    js::sort_by(&mut ids, |a, b| a.1[0] - b.1[0]);
    let mut to: FxMap<u64, f64> = FxMap::default();
    let mut to_order: Vec<f64> = Vec::new();
    for i in 0..ids.len() {
        let Some(&(idi, p)) = ids.get(i) else { break };
        if to.contains_key(&kb(idi)) {
            continue;
        }
        for &(idj, q) in ids.iter().skip(i + 1) {
            if q[0] - p[0] > EPS {
                break;
            }
            if !to.contains_key(&kb(idj)) && near(p, q, EPS) {
                to.insert(kb(idj), idi);
                to_order.push(idj);
            }
        }
    }
    let mut changed = !to.is_empty();
    if !to.is_empty() {
        let m = |v: f64| to.get(&kb(v)).copied().unwrap_or(v);
        let edges = def.edges.entries();
        def.edges.clear();
        def.ek = Some(FxMap::default());
        for (id, e) in edges {
            let a = m(e.a);
            let b = m(e.b);
            if a == b || edge_between(def, a, b).is_some() {
                continue;
            }
            def.edges.set(id, Edge { a, b, ..e });
            ek(def).insert(key(a, b), id);
        }
        for f in def.faces.values_mut() {
            for l in f.loops.iter_mut() {
                for v in l.iter_mut() {
                    *v = m(*v);
                }
            }
        }
        for v in to_order {
            def.verts.delete(v);
        }
    }
    for fid in def.faces.key_vec() {
        // the loops are taken out while they are checked (and put back: the face keeps its place)
        let Some(loops0) = def.faces.get_mut(fid).map(|f| std::mem::take(&mut f.loops)) else { continue };
        enum St {
            Keep,
            Short(Vec<f64>),
            Drop,
        }
        let mut st: Vec<St> = Vec::with_capacity(loops0.len());
        for l in &loops0 {
            let n = l.len();
            let next = |k: usize| l.get((k + 1) % n).copied().unwrap_or(f64::NAN);
            // l.filter((v, k) => v !== l[(k + 1) % n]): unchanged unless a corner repeats the next one
            let dup = l.iter().enumerate().any(|(k, v)| *v == next(k));
            let ok = |c: &[f64]| c.len() >= 3 && poly_area_of(def, c) > EPS2;
            st.push(if !dup {
                if ok(l) { St::Keep } else { St::Drop }
            } else {
                let c: Vec<f64> = l.iter().enumerate().filter(|(k, v)| **v != next(*k)).map(|(_, v)| *v).collect();
                if ok(&c) { St::Short(c) } else { St::Drop }
            });
        }
        if loops0.is_empty() || matches!(st.first(), Some(St::Drop)) {
            def.faces.delete(fid);
            changed = true;
            continue;
        }
        let loops = if st.iter().all(|s| matches!(s, St::Keep)) {
            loops0
        } else {
            changed = true;
            loops0
                .into_iter()
                .zip(st)
                .filter_map(|(l, s)| match s {
                    St::Keep => Some(l),
                    St::Short(c) => Some(c),
                    St::Drop => None,
                })
                .collect()
        };
        for l in &loops {
            let n = l.len();
            for (i, &a) in l.iter().enumerate() {
                e_add0(def, a, l.get((i + 1) % n).copied().unwrap_or(a));
            }
        }
        if let Some(f) = def.faces.get_mut(fid) {
            f.loops = loops;
        }
    }
    remove_loose_verts(def);
    changed
}

pub fn remove_loose_verts(def: &mut Def) {
    let mut used = crate::dense::VSet::new(&def.verts);
    for e in def.edges.values() {
        used.add(e.a);
        used.add(e.b);
    }
    for f in def.faces.values() {
        for l in &f.loops {
            for &v in l {
                used.add(v);
            }
        }
    }
    let dead: Vec<f64> = def.verts.iter_slots().filter(|(s, _, _)| !used.has_slot(*s)).map(|(_, v, _)| v).collect();
    drop(used);
    for v in dead {
        def.verts.delete(v);
    }
    prune_splines(def);
}

/// The ids of the edges at vertex v, in edge order (`vertEdges(def).get(v) || []`).
pub fn edges_at(def: &Def, v: f64) -> Vec<f64> {
    let mut out = Vec::new();
    for (id, e) in def.edges.iter() {
        if e.a == v {
            out.push(id);
        }
        if e.b == v {
            out.push(id);
        }
    }
    out
}

/// A vertex between two collinear edges (and nothing else) disappears; the edges become one.
pub fn heal_collinear(def: &mut Def, vids: &[f64]) {
    for &v in vids {
        if !def.verts.has(v) {
            continue;
        }
        let ve = edges_at(def, v);
        let [ve0, ve1] = ve.as_slice() else { continue };
        let (Some(e1), Some(e2)) = (def.edges.get(*ve0).cloned(), def.edges.get(*ve1).cloned()) else { continue };
        let a = if e1.a == v { e1.b } else { e1.a };
        let b = if e2.a == v { e2.b } else { e2.a };
        if a == b {
            continue;
        }
        let pv = def.v(v);
        let u1 = norm(sub(def.v(a), pv));
        let u2 = norm(sub(def.v(b), pv));
        if dot(u1, u2) > -1.0 + 1e-9 {
            continue;
        }
        if edge_between(def, a, b).is_some() {
            continue;
        }
        let ef = edge_faces(def);
        let f1 = js::sorted_join(ef_get(&ef, v, a));
        let f2 = js::sorted_join(ef_get(&ef, v, b));
        if f1 != f2 {
            continue;
        }
        for f in def.faces.values_mut() {
            for l in f.loops.iter_mut() {
                if let Some(i) = l.iter().position(|&x| x == v) {
                    l.remove(i);
                }
            }
        }
        e_remove(def, *ve0);
        e_remove(def, *ve1);
        let curve = if e1.curve == e2.curve { e1.curve.clone() } else { None };
        e_add(def, a, b, e1.soft && e2.soft, curve, e1.hidden && e2.hidden);
        def.verts.delete(v);
    }
}

/// Soften by angle (0.4 M18): each edge of `eids` between exactly two faces becomes soft when the faces meet at `deg`
/// degrees or less (0 = flat), hard when more; other edges are left as they are. → how many changed.
pub fn soften_edges(def: &mut Def, eids: &[f64], deg: f64) -> f64 {
    let ef = edge_faces(def);
    let cos = ((js::max(0.0, js::min(180.0, deg)) * std::f64::consts::PI) / 180.0).jcos();
    let mut n = 0.0;
    for &eid in eids {
        let Some(e) = def.edges.get(eid) else { continue };
        let fs = ef_get(&ef, e.a, e.b);
        let [f0, f1] = fs else { continue };
        let (Some(fa), Some(fb)) = (def.faces.get(*f0), def.faces.get(*f1)) else { continue };
        let c = dot(face_normal(def, fa), face_normal(def, fb));
        let soft = deg > 0.0 && c >= cos - 1e-12;
        if let Some(e) = def.edges.get_mut(eid)
            && e.soft != soft
        {
            e.soft = soft;
            n += 1.0;
        }
    }
    n
}

/// Smooth shading (0.4 M18): faces joined by soft edges are one smooth surface. → `${fid}|${v}` → the unit normal at
/// that corner (definition coordinates): the area-weighted average over the faces of its surface that meet at v.
/// Faces with no soft edge are left out.
pub fn smooth_normals(def: &Def) -> OMap<String, P3> {
    let mut out: OMap<String, P3> = OMap::new();
    if !def.edges.values().any(|e| e.soft) {
        return out;
    }
    let ef = edge_faces(def);
    let mut up: FxMap<u64, f64> = FxMap::default();
    fn find(up: &mut FxMap<u64, f64>, mut x: f64) -> f64 {
        let mut guard = 0u32;
        loop {
            let ux = up.get(&kb(x)).copied().unwrap_or(f64::NAN);
            if ux == x || guard > 1_000_000 {
                return x;
            }
            guard += 1;
            let uux = up.get(&kb(ux)).copied().unwrap_or(f64::NAN);
            up.insert(kb(x), uux);
            x = uux;
        }
    }
    for fid in def.faces.keys() {
        up.insert(kb(fid), fid);
    }
    let mut joined = IdSet::new();
    for e in def.edges.values() {
        if !e.soft {
            continue;
        }
        let fs = ef_get(&ef, e.a, e.b);
        let Some(&f0) = fs.first() else { continue };
        for &fi in fs.iter().skip(1) {
            let ri = find(&mut up, fi);
            let r0 = find(&mut up, f0);
            up.insert(kb(ri), r0);
            joined.add(f0);
            joined.add(fi);
        }
    }
    let mut acc: FxMap<(u64, u64), P3> = FxMap::default();
    for fid in joined.iter() {
        let Some(f) = def.faces.get(fid) else { continue };
        let n = face_normal(def, f);
        let a = js::or(poly_area_of(def, f.loops.first().map_or(&[][..], |l| l.as_slice())), 1e-12);
        let r = find(&mut up, fid);
        for l in &f.loops {
            for &v in l {
                let k = (kb(r), kb(v));
                let cur = acc.get(&k).copied().unwrap_or([0.0; 3]);
                acc.insert(k, add(cur, mul(n, a)));
            }
        }
    }
    for fid in joined.iter() {
        let Some(f) = def.faces.get(fid) else { continue };
        let r = find(&mut up, fid);
        let fnn = face_normal(def, f);
        for l in &f.loops {
            for &v in l {
                let s = acc.get(&(kb(r), kb(v))).copied().unwrap_or([f64::NAN; 3]);
                let ll = len(s);
                let mut k = js::num_str(fid);
                k.push('|');
                js::push_num(&mut k, v);
                out.set(k, if ll > 1e-12 { mul(s, 1.0 / ll) } else { fnn });
            }
        }
    }
    out
}

/// Remove the edge between two coplanar faces facing the same way, merging them.
pub fn merge_coplanar(def: &mut Def, eids: &[f64]) {
    for &eid in eids {
        let Some(e) = def.edges.get(eid).cloned() else { continue };
        let ef = edge_faces(def);
        let fs = ef_get(&ef, e.a, e.b);
        let [f0, f1] = fs else { continue };
        let (Some(fa), Some(fb)) = (def.faces.get(*f0), def.faces.get(*f1)) else { continue };
        let p1 = face_plane(def, fa);
        let p2 = face_plane(def, fb);
        if dot(p1.n, p2.n) < 1.0 - 1e-9 || (p1.d - p2.d).abs() > EPS {
            continue;
        }
        e_remove(def, eid);
        let pl = plane_from(p1.n, def.v(e.a));
        reface(def, &pl, &IdSet::new(), None);
        remove_loose_verts(def);
        heal_collinear(def, &[e.a, e.b]);
    }
}
