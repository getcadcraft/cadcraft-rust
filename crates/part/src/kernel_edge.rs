//! kernel.js, the features that work on the body as it is now (0.4 K.4 / K.5, 0.5 WL.29): fillets and chamfers on
//! straight edges between flat faces, shell (Minkowski erosion / dilation), draft on flat faces, threads on round faces,
//! and threads.js's `threadTool` / `boltSolid` (the helical solids). Same calls in the same order as the JS.

use crate::i18n::{F, t};
use crate::js;
use crate::jv;
use crate::kernel::{self as k, Ctx, FR, H, Out, Tool, tk};
use crate::meshfaces::FaceRec;
use crate::threads;
use crate::vec::{self as v, V3};
use cc_manifold::{CrossSection, FillRule, Manifold};
use serde_json::Value;
use std::cell::RefCell;
use std::rc::Rc;

/// brep.js `findEdge(edges, ref)`: the edge with the same two face names nearest the reference's point; -1 = gone.
fn find_edge(edges: &[crate::meshfaces::EdgeRec], r: &Value) -> Option<usize> {
    let mut faces: Vec<String> = jv::arr(r.get("faces")).iter().map(|x| js::str_of(Some(x))).collect();
    faces.sort_by(|a, b| js::cmp16(a, b));
    let key = faces.join("|");
    let at = r.get("at").filter(|a| jv::truthy(Some(a)));
    let mut best = None;
    let mut bd = f64::INFINITY;
    for (i, e) in edges.iter().enumerate() {
        if e.faces.join("|") != key {
            continue;
        }
        let d = match at {
            Some(a) => v::len(v::sub(e.at, p3(Some(a)))),
            None => 0.0,
        };
        if d < bd {
            bd = d;
            best = Some(i);
        }
    }
    best
}

/// brep.js `findFace(faces, ref)`.
fn find_face(faces: &[FaceRec], r: &Value) -> Option<usize> {
    let name = r.get("name");
    let at = r.get("at").filter(|a| jv::truthy(Some(a)));
    let mut best = None;
    let mut bd = f64::INFINITY;
    for (i, f) in faces.iter().enumerate() {
        let same = match (&f.name, name) {
            (Some(n), Some(Value::String(s))) => n == s,
            (None, None) => true,
            (None, Some(Value::Null)) => true,
            _ => false,
        };
        if !same {
            continue;
        }
        let d = match at {
            Some(a) => v::len(v::sub(f.at, p3(Some(a)))),
            None => 0.0,
        };
        if d < bd {
            bd = d;
            best = Some(i);
        }
    }
    best
}

fn p3(x: Option<&Value>) -> V3 {
    let a = jv::arr(x);
    let g = |i: usize| jv::as_num(a.get(i)).unwrap_or(f64::NAN);
    [g(0), g(1), g(2)]
}

fn n(f: &Value, key: &str) -> f64 {
    jv::n(f, key)
}

fn union_of(ctx: &mut Ctx, list: &[H]) -> H {
    match list {
        [one] => Rc::clone(one),
        _ => {
            let l: Vec<Manifold> = list.iter().map(|h| (**h).clone()).collect();
            ctx.t(Manifold::union(&l))
        }
    }
}

/// `edgeTools(f, ctx, kind)`: fillets and chamfers on straight edges between two flat faces.
pub fn edge_tools(f: &Value, ctx: &mut Ctx, kind: &str) -> FR<Out> {
    let fillet = kind == "fillet";
    if ctx.body.is_none() {
        return Err(tk(if fillet { "model.brep.no_body_round" } else { "model.brep.no_body_chamfer" }));
    }
    let size = if fillet { n(f, "r") } else { n(f, "d") };
    if !(size > 0.0) {
        return Err(tk(if fillet { "model.brep.radius_positive" } else { "model.brep.distance_positive" }));
    }
    let edges_v = jv::arr(f.get("edges"));
    if edges_v.is_empty() {
        return Err(tk("model.brep.pick_edges"));
    }
    if fillet && f.get("r2").is_some_and(Value::is_number) {
        return Err(tk("kernel.mesh.fillet_variable"));
    }
    let mode = js::str_of(f.get("mode"));
    if !fillet && mode == "two" && !(n(f, "d2") > 0.0) {
        return Err(tk("model.brep.d2_positive"));
    }
    if !fillet && mode == "angle" && !(n(f, "angle") > 0.0 && n(f, "angle") < 90.0) {
        return Err(tk("model.brep.chamfer_angle"));
    }
    let Some(b) = k::body_faces(ctx) else { return Err(tk("model.brep.no_body_round")) };
    let pp = &b.mesh.positions;
    let ii = &b.mesh.indices;
    let pt = |i: u32| -> V3 { k::pt_at(pp, i as usize) };
    let seg = js::max(8.0, ctx.resolution_or64());
    let label = crate::part::feature_label(kind);
    let straight = || t("kernel.mesh.fillet_straight", &[("what", F::S(label.clone()))]);
    let fid = js::str_of(f.get("id"));
    let mut cut: Vec<H> = Vec::new();
    let mut fill: Vec<H> = Vec::new();
    let mut done = std::collections::HashSet::new();
    let mut ends: Vec<V3> = Vec::new();
    for (kk, r) in edges_v.iter().enumerate() {
        let Some(ei) = find_edge(&b.named.edges, r) else {
            let fs = jv::arr(r.get("faces"));
            return Err(t(
                if fillet { "model.brep.round_edge_gone" } else { "model.brep.chamfer_edge_gone" },
                &[("a", F::S(js::string_of(fs.first()))), ("b", F::S(js::string_of(fs.get(1))))],
            ));
        };
        if !done.insert(ei) {
            continue;
        }
        let Some(e) = b.named.edges.get(ei) else { continue };
        let fa = b.named.faces.iter().position(|x| x.name.as_deref() == e.faces.first().map(String::as_str));
        let fb = b.named.faces.iter().position(|x| x.name.as_deref() == e.faces.get(1).map(String::as_str));
        let (Some(fa), Some(fb)) = (fa, fb) else { return Err(straight()) };
        let (Some(face_a), Some(face_b)) = (b.named.faces.get(fa), b.named.faces.get(fb)) else { return Err(straight()) };
        if e.ty != "line" || face_a.ty != "plane" || face_b.ty != "plane" {
            return Err(straight());
        }
        let a = e.a;
        let d = v::norm(v::sub(e.b, e.a));
        let l = v::len(v::sub(e.b, e.a));
        let o = a;
        ends.push(e.a);
        ends.push(e.b);
        let into = |fi: usize, nrm: V3| -> FR<V3> {
            let mut u = v::norm(v::cross(d, nrm));
            let tol = 1e-5 * js::max(1.0, l);
            let fi32 = i32::try_from(fi).unwrap_or(-1);
            for ti in 0..ii.len() / 3 {
                if b.named.tri_face.get(ti).copied() != Some(fi32) {
                    continue;
                }
                let vs = [pt(k::tri_v(ii, ti, 0)), pt(k::tri_v(ii, ti, 1)), pt(k::tri_v(ii, ti, 2))];
                let off: Vec<f64> = vs
                    .iter()
                    .map(|&vv| {
                        let w = v::sub(vv, a);
                        v::len(v::sub(w, v::mul(d, v::dot(w, d))))
                    })
                    .collect();
                if off.iter().filter(|&&x| x <= tol).count() != 2 {
                    continue;
                }
                let far = off.iter().position(|&x| x > tol).and_then(|p| vs.get(p)).copied().unwrap_or([f64::NAN; 3]);
                if v::dot(v::sub(far, a), u) < 0.0 {
                    u = v::mul(u, -1.0);
                }
                return Ok(u);
            }
            Err(straight())
        };
        let na = face_a.n.unwrap_or([f64::NAN; 3]);
        let nb = face_b.n.unwrap_or([f64::NAN; 3]);
        let ua = into(fa, na)?;
        let ub = into(fb, nb)?;
        let convex = v::dot(ua, nb) < 0.0;
        let s = if convex { -1.0 } else { 1.0 };
        let (ta, tb);
        let mut arc: Vec<V3> = Vec::new();
        if fillet {
            let k1 = 1.0 - v::dot(na, nb);
            let sa = (s * size * k1) / v::dot(ua, nb);
            let sb = (s * size * k1) / v::dot(ub, na);
            if !(sa > 0.0 && sb > 0.0) || sa > l * 1e3 {
                return Err(t("model.brep.round_no_fit", &[("size", F::N(size))]));
            }
            ta = v::add(o, v::mul(ua, sa));
            tb = v::add(o, v::mul(ub, sb));
            let c = v::add(v::add(o, v::mul(ua, sa)), v::mul(na, s * size));
            let v0 = v::sub(ta, c);
            let v1 = v::sub(tb, c);
            let ax = v::norm(v::cross(v0, v1));
            let ang = js::acos(js::max(-1.0, js::min(1.0, v::dot(v0, v1) / (size * size))));
            let nseg = js::max(2.0, (ang / (2.0 * std::f64::consts::PI) * seg).ceil());
            let nn = js::count(nseg, 1_000_000);
            for i in 1..nn {
                arc.push(v::add(
                    c,
                    v::apply_mat(&v::rotation_about([0.0; 3], ax, (ang * js::f(i) / nseg) * 180.0 / std::f64::consts::PI), v0),
                ));
            }
            ctx.edge_surfs.push(k::surf_cylinder(&format!("{fid}.blend:{kk}"), c, d, size));
        } else {
            let flip = jv::truthy(f.get("flip"));
            let (first_a, ua2, ub2) = if flip { (false, ub, ua) } else { (true, ua, ub) };
            let theta = js::acos(js::max(-1.0, js::min(1.0, v::dot(ua2, ub2))));
            let da = size;
            let mut db = size;
            if mode == "two" {
                db = n(f, "d2");
            } else if mode == "angle" {
                let al = n(f, "angle") * std::f64::consts::PI / 180.0;
                db = size * js::sin(al) / js::sin(std::f64::consts::PI - al - theta);
            }
            if !(db > 0.0) {
                return Err(t("model.brep.chamfer_no_fit", &[("size", F::N(size))]));
            }
            let pa = v::add(o, v::mul(ua2, da));
            let pb = v::add(o, v::mul(ub2, db));
            if first_a {
                ta = pa;
                tb = pb;
            } else {
                ta = pb;
                tb = pa;
            }
            ctx.edge_surfs.push(k::surf_plane(&format!("{fid}.bevel:{kk}"), v::cross(v::sub(tb, ta), d), ta));
        }
        let eps = 0.01 * size;
        let out = v::mul(v::add(na, nb), -s * eps);
        let mut poly = vec![v::add(o, out), v::sub(ta, v::mul(na, s * eps)), ta];
        poly.extend(arc.iter().copied());
        poly.push(tb);
        poly.push(v::sub(tb, v::mul(nb, s * eps)));
        let x = v::norm(v::sub(ta, o));
        let y = v::cross(d, x);
        let mut lp: Vec<[f64; 2]> = poly.iter().map(|&p| [v::dot(v::sub(p, o), x), v::dot(v::sub(p, o), y)]).collect();
        let mut ar = 0.0;
        for i in 0..lp.len() {
            let q = lp.get(i).copied().unwrap_or([0.0; 2]);
            let r2 = lp.get(i.saturating_add(1).checked_rem(lp.len()).unwrap_or(0)).copied().unwrap_or([0.0; 2]);
            ar += q[0] * r2[1] - r2[0] * q[1];
        }
        if ar < 0.0 {
            lp.reverse();
        }
        let cs = CrossSection::new(&[lp], FillRule::Positive);
        let e = k::cs_extrude(&cs, l, 0, 0.0, [1.0, 1.0]);
        let e = ctx.t(e);
        let prism = ctx.t(k::xf(&e, &v::basis_matrix(x, y, d, o)));
        if convex {
            cut.push(prism);
        } else {
            fill.push(prism);
        }
    }
    let mut tools = Vec::new();
    if !cut.is_empty() {
        let m = union_of(ctx, &cut);
        tools.push(Tool { m, op: "cut".into(), surfs: None });
    }
    if !fill.is_empty() {
        let m = union_of(ctx, &fill);
        tools.push(Tool { m, op: "add".into(), surfs: None });
    }
    let tol_e = 1e-6 * js::max_of(std::iter::once(1.0).chain(ends.iter().map(|&p| v::len(p))));
    let meet = ends
        .iter()
        .enumerate()
        .any(|(i, &p)| ends.iter().enumerate().any(|(j, &q)| j > i && (j >> 1) != (i >> 1) && v::len(v::sub(p, q)) <= tol_e));
    Ok(Out { tools: Some(tools), warning: meet.then(|| t("kernel.mesh.fillet_approx", &[("what", F::S(label))])), ..Out::default() })
}

thread_local! {
    /// kernel.js `offsetCache`: eroded / dilated bodies by feature, across rebuilds (insertion order, at most 8).
    static OFFSET_CACHE: RefCell<Vec<(String, H)>> = const { RefCell::new(Vec::new()) };
}
const OFFSET_CACHE_MAX: usize = 8;

/// Empties the offset cache (a fresh JS module).
pub fn clear_offset_cache() {
    OFFSET_CACHE.with(|c| c.borrow_mut().clear());
}

/// `shellTools(f, ctx)`.
pub fn shell_tools(f: &Value, ctx: &mut Ctx) -> FR<Out> {
    let Some(body) = ctx.body.clone() else { return Err(tk("model.brep.no_body_shell")) };
    let th = n(f, "t");
    if !(th > 0.0) {
        return Err(tk("model.brep.wall_positive"));
    }
    let faces_v = jv::arr(f.get("faces"));
    if faces_v.is_empty() {
        return Err(tk("model.brep.shell_pick_faces"));
    }
    let Some(b) = k::body_faces(ctx) else { return Err(tk("model.brep.no_body_shell")) };
    let mut open: Vec<FaceRec> = Vec::new();
    for r in faces_v {
        let Some(i) = find_face(&b.named.faces, r) else {
            return Err(t("model.brep.shell_face_gone", &[("face", F::S(js::string_of(r.get("name"))))]));
        };
        let Some(fc) = b.named.faces.get(i) else { continue };
        if fc.ty != "plane" {
            return Err(tk("kernel.mesh.shell_flat"));
        }
        open.push(fc.clone());
    }
    let outward = jv::truthy(f.get("outward"));
    let key =
        format!("{}|{}|{}|{}|{}", js::str_of(f.get("id")), js::num_str(th), outward, body.num_tri(), js::to_precision(body.volume(), 12));
    let cached = OFFSET_CACHE.with(|c| c.borrow().iter().find(|(kk, _)| *kk == key).map(|(_, h)| Rc::clone(h)));
    let off = match cached {
        Some(h) => h,
        None => {
            let segs = js::max(8.0, js::min(32.0, ctx.resolution_or64() / 2.0));
            let ball = Manifold::sphere(th, k::js_int(segs));
            let off = Rc::new(if outward { body.minkowski_sum(&ball) } else { body.minkowski_difference(&ball) });
            drop(ball);
            OFFSET_CACHE.with(|c| {
                let mut c = c.borrow_mut();
                c.push((key, Rc::clone(&off)));
                if c.len() > OFFSET_CACHE_MAX {
                    c.remove(0);
                }
            });
            off
        }
    };
    if off.is_empty() {
        return Err(tk("model.brep.shell_thickness"));
    }
    let mut hollow: H = if outward { Rc::clone(&body) } else { Rc::clone(&off) };
    for fc in &open {
        let nrm = fc.n.unwrap_or([f64::NAN; 3]);
        let len = th * 1.5 + 1e-3;
        let c0 = ctx.t(Manifold::cube([1e-7 * th, 1e-7 * th, len], true));
        let x = v::norm(v::cross(nrm, if nrm[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] }));
        let bar = ctx.t(k::xf(&c0, &v::basis_matrix(x, v::cross(nrm, x), nrm, v::mul(nrm, len / 2.0))));
        let ms = ctx.t(hollow.minkowski_sum(&bar));
        hollow = ctx.t(hollow.add(&ms));
    }
    let s = if outward { 1.0 } else { -1.0 };
    let fid = js::str_of(f.get("id"));
    let mut moved: Vec<Value> = Vec::new();
    let mut kept: Vec<Value> = Vec::new();
    for fc in &b.named.faces {
        let Some(name) = fc.name.as_deref().filter(|x| !x.is_empty()) else { continue };
        let nm = format!("{name}~{fid}");
        let nan3 = [f64::NAN; 3];
        let (fn_, fo) = (fc.n.unwrap_or(nan3), fc.o.unwrap_or(nan3));
        let (ao, ad) = fc.axis.map_or((nan3, nan3), |a| (a.o, a.d));
        let fr = fc.r.unwrap_or(f64::NAN);
        if fc.ty == "plane" {
            moved.push(k::surf_plane(&nm, fn_, v::add(fo, v::mul(fn_, s * th))));
            kept.push(k::surf_plane(name, fn_, fo));
        } else if fc.ty == "cylinder" {
            moved.push(k::surf_cylinder(&nm, ao, ad, fr + th));
            moved.push(k::surf_cylinder(&nm, ao, ad, js::max(1e-9, fr - th)));
            kept.push(k::surf_cylinder(name, ao, ad, fr));
        }
    }
    if outward {
        return Ok(Out {
            tools: Some(vec![
                Tool { m: off, op: "add".into(), surfs: Some(Value::Array(moved)) },
                Tool { m: hollow, op: "cut".into(), surfs: Some(Value::Array(kept)) },
            ]),
            ..Out::default()
        });
    }
    Ok(Out { tools: Some(vec![Tool { m: hollow, op: "cut".into(), surfs: Some(Value::Array(moved)) }]), ..Out::default() })
}

/// `draftTools(f, ctx)`.
pub fn draft_tools(f: &Value, ctx: &mut Ctx) -> FR<Out> {
    let Some(body) = ctx.body.clone() else { return Err(tk("model.brep_design.no_body_draft")) };
    let faces_v = jv::arr(f.get("faces"));
    if faces_v.is_empty() {
        return Err(tk("model.brep_design.draft_pick_faces"));
    }
    if !(n(f, "angle").abs() > 0.0) {
        return Err(tk("model.brep_design.draft_angle_zero"));
    }
    let neutral = k::resolve_plane(f.get("neutral").unwrap_or(&Value::Null), ctx)?;
    let pull = if jv::truthy(f.get("pull")) { v::norm(k::resolve_dir(f.get("pull").unwrap_or(&Value::Null), ctx)?.1) } else { neutral.n };
    let Some(b) = k::body_faces(ctx) else { return Err(tk("model.brep_design.no_body_draft")) };
    let pp = &b.mesh.positions;
    let ii = &b.mesh.indices;
    let (mn, mx) = body.bounding_box();
    let size = js::max(1.0, v::len(v::sub(mx, mn)));
    let a = n(f, "angle") * std::f64::consts::PI / 180.0;
    let mut cuts: Vec<H> = Vec::new();
    let mut adds: Vec<H> = Vec::new();
    let mut surfs: Vec<Value> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let fid = js::str_of(f.get("id"));
    for (i, r) in faces_v.iter().enumerate() {
        let Some(fi) = find_face(&b.named.faces, r) else {
            return Err(t("model.brep_design.draft_face_gone", &[("face", F::S(js::string_of(r.get("name"))))]));
        };
        if !seen.insert(fi) {
            continue;
        }
        let Some(fc) = b.named.faces.get(fi) else { continue };
        if fc.ty != "plane" {
            return Err(tk("kernel.mesh.draft_flat"));
        }
        let nrm = fc.n.unwrap_or([f64::NAN; 3]);
        let fco = fc.o.unwrap_or([f64::NAN; 3]);
        let c0 = v::dot(fco, nrm);
        let np = v::sub(nrm, v::mul(pull, v::dot(nrm, pull)));
        let face_name = js::string_of(r.get("name"));
        if v::len(np) < 1e-9 {
            return Err(t("model.brep_design.draft_neutral", &[("face", F::S(face_name))]));
        }
        let n2 = v::norm(v::add(v::mul(v::norm(np), js::cos(a)), v::mul(pull, js::sin(a))));
        let m = neutral.n;
        let d1 = c0;
        let d2 = v::dot(neutral.o, m);
        let nm = v::dot(nrm, m);
        let den = 1.0 - nm * nm;
        if den.abs() < 1e-12 {
            return Err(t("model.brep_design.draft_neutral", &[("face", F::S(face_name))]));
        }
        let q = v::add(v::mul(nrm, (d1 - d2 * nm) / den), v::mul(m, (d2 - d1 * nm) / den));
        let c1 = v::dot(q, n2);
        let x = v::norm(if nrm[0].abs() < 0.9 { v::cross(nrm, [1.0, 0.0, 0.0]) } else { v::cross(nrm, [0.0, 1.0, 0.0]) });
        let y = v::cross(nrm, x);
        // the face's outline: edges only one of its triangles uses, in first-seen order (a JS Map)
        let mut cnt: Vec<((u32, u32), usize, (u32, u32))> = Vec::new();
        let mut index: std::collections::HashMap<(u32, u32), usize> = std::collections::HashMap::new();
        let fi32 = i32::try_from(fi).unwrap_or(-1);
        for ti in 0..ii.len() / 3 {
            if b.named.tri_face.get(ti).copied() != Some(fi32) {
                continue;
            }
            for kk in 0..3 {
                let u = k::tri_v(ii, ti, kk);
                let w = k::tri_v(ii, ti, if kk == 2 { 0 } else { kk.saturating_add(1) });
                let key = if u < w { (u, w) } else { (w, u) };
                match index.get(&key) {
                    Some(&p) => {
                        if let Some(e) = cnt.get_mut(p) {
                            e.1 = e.1.saturating_add(1);
                            e.2 = (u, w);
                        }
                    }
                    None => {
                        index.insert(key, cnt.len());
                        cnt.push((key, 1, (u, w)));
                    }
                }
            }
        }
        // next: a JS Map set in that order (a later set keeps the first insertion position)
        let mut next_keys: Vec<u32> = Vec::new();
        let mut next: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
        for (_, c, (u, w)) in &cnt {
            if *c == 1 {
                if !next.contains_key(u) {
                    next_keys.push(*u);
                }
                next.insert(*u, *w);
            }
        }
        let mut loops: Vec<Vec<[f64; 2]>> = Vec::new();
        let mut used = std::collections::HashSet::new();
        for &st in &next_keys {
            if used.contains(&st) {
                continue;
            }
            let mut lp = Vec::new();
            let mut cur = Some(st);
            let mut g = 0usize;
            while let Some(vv) = cur {
                if used.contains(&vv) || g > next.len() {
                    break;
                }
                used.insert(vv);
                let vi = vv as usize;
                let w = v::sub(k::pt_at(pp, vi), fco);
                lp.push([v::dot(w, x), v::dot(w, y)]);
                cur = next.get(&vv).copied();
                g = g.saturating_add(1);
            }
            if lp.len() >= 3 {
                loops.push(lp);
            }
        }
        if loops.is_empty() {
            return Err(tk("model.brep_design.draft_failed"));
        }
        let ll = size * 2.0;
        let cs = CrossSection::new(&loops, FillRule::EvenOdd);
        let e = k::cs_extrude(&cs, 2.0 * ll, 0, 0.0, [1.0, 1.0]);
        let e = ctx.t(e);
        let prism = ctx.t(k::xf(&e, &v::basis_matrix(x, y, nrm, v::sub(fco, v::mul(nrm, ll)))));
        let c_a = ctx.t(prism.trim_by_plane(v::mul(nrm, -1.0), -c0));
        let cut = ctx.t(c_a.trim_by_plane(n2, c1));
        let a_a = ctx.t(prism.trim_by_plane(nrm, c0));
        let add = ctx.t(a_a.trim_by_plane(v::mul(n2, -1.0), -c1));
        if !cut.is_empty() {
            cuts.push(cut);
        }
        if !add.is_empty() {
            adds.push(add);
        }
        surfs.push(k::surf_plane(&format!("{fid}.draft:{i}"), n2, q));
    }
    let mut tools = Vec::new();
    if !cuts.is_empty() {
        let m = union_of(ctx, &cuts);
        tools.push(Tool { m, op: "cut".into(), surfs: Some(Value::Array(surfs.clone())) });
    }
    if !adds.is_empty() {
        let m = union_of(ctx, &adds);
        tools.push(Tool { m, op: "add".into(), surfs: Some(Value::Array(surfs)) });
    }
    Ok(Out { tools: Some(tools), ..Out::default() })
}

/// threads.js `threadTool` options.
pub struct ThreadOpts {
    pub form: String,
    pub size: String,
    pub external: bool,
    pub len: f64,
    pub clearance: f64,
    pub hand: String,
    pub segments: f64,
    pub outer: f64,
    pub ext0: f64,
    pub ext1: f64,
    /// `face { r, segments }`.
    pub face: Option<(f64, f64)>,
}

/// Largest number of extrusion slices one helical solid may have.
pub const MAX_THREAD_SLICES: f64 = 200_000.0;

/// threads.js `boltSolid(M, T, corners, h, { hand, segments })`.
fn bolt_solid(ctx: &mut Ctx, corners: &[[f64; 2]], h: f64, hand: &str, segments: f64) -> FR<H> {
    let p = corners.last().map_or(f64::NAN, |c| c[0]);
    let poly = threads::bolt_section(corners, "right", segments);
    let cs = CrossSection::new(&[poly], FillRule::Positive);
    let turns = h / p;
    let slices = js::max(1.0, (turns * js::max(24.0, segments)).ceil());
    if !(slices <= MAX_THREAD_SLICES) {
        return Err(t("model.kernel.build_failed", &[("error", F::S("thread too long".into()))]));
    }
    let r = k::cs_extrude(&cs, h, k::js_int(slices - 1.0), 360.0 * turns, [1.0, 1.0]);
    let right = ctx.t(r);
    Ok(if hand == "left" { ctx.t(right.mirror([0.0, 1.0, 0.0])) } else { right })
}

/// threads.js `threadTool(M, T, opts)` → the cutting tool in its own frame, or None (unknown size).
pub fn thread_tool(ctx: &mut Ctx, o: &ThreadOpts) -> FR<Option<H>> {
    let Some(mut corners) = threads::profile_corners(&o.form, &o.size, if o.external { -o.clearance } else { o.clearance })? else {
        return Ok(None);
    };
    if let Some((fr, fseg)) = o.face {
        let lo = fr * js::cos(std::f64::consts::PI / js::max(8.0, fseg));
        let eps = 1e-4 * fr;
        let kk = if o.external { 0 } else { 2 };
        let vv = corners.get(kk).map_or(f64::NAN, |c| c[1]);
        if vv > lo - eps && vv < fr + eps {
            let to = if o.external { lo - eps } else { fr + eps };
            for c in corners.iter_mut() {
                if (c[1] - vv).abs() < 1e-12 {
                    c[1] = to;
                }
            }
        }
    }
    let h = o.len + o.ext0 + o.ext1;
    let p = corners.last().map_or(f64::NAN, |c| c[0]);
    let pre = (o.ext0 / p).ceil() * p;
    let bs = bolt_solid(ctx, &corners, o.len + pre + o.ext1, &o.hand, o.segments)?;
    let bolt = ctx.t(bs.translate([0.0, 0.0, -pre]));
    if !o.external {
        return Ok(Some(if pre > o.ext0 + 1e-9 { ctx.t(bolt.trim_by_plane([0.0, 0.0, 1.0], -o.ext0)) } else { bolt }));
    }
    let circ = CrossSection::circle(o.outer, k::js_int(js::max(24.0, o.segments)));
    let e = k::cs_extrude(&circ, h, 0, 0.0, [1.0, 1.0]);
    let e = ctx.t(e);
    let tube = ctx.t(e.translate([0.0, 0.0, -o.ext0]));
    let cut = ctx.t(tube.subtract(&bolt));
    Ok(Some(cut))
}

/// `fmtMm(v)`: `String(+Number(v).toFixed(2))`.
fn fmt_mm(x: f64) -> String {
    js::fmt_fixed(x, 2)
}

/// `threadFeature(f, ctx)`: threads on picked round faces.
pub fn thread_feature(f: &Value, ctx: &mut Ctx) -> FR<Out> {
    let Some(body) = ctx.body.clone() else { return Err(tk("thread.err.no_body")) };
    let faces_v = jv::arr(f.get("faces"));
    if faces_v.is_empty() {
        return Err(tk("thread.err.pick"));
    }
    let Some(b) = k::body_faces(ctx) else { return Err(tk("thread.err.no_body")) };
    let pp = b.mesh.positions.clone();
    let ii = b.mesh.indices.clone();
    let res = js::max(48.0, ctx.resolution_or64());
    let mut tools = Vec::new();
    let mut list: Vec<Value> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let form = js::str_of(f.get("form"));
    let fid = js::str_of(f.get("id"));
    let modelled = jv::truthy(f.get("modelled"));
    for (i, r) in faces_v.iter().enumerate() {
        let Some(fi) = find_face(&b.named.faces, r) else {
            return Err(t("thread.err.face_gone", &[("face", F::S(js::string_of(r.get("name"))))]));
        };
        let Some(fc) = b.named.faces.get(fi) else { continue };
        if fc.ty != "cylinder" {
            return Err(tk("thread.err.round"));
        }
        let nan3 = [f64::NAN; 3];
        let (o, d0) = fc.axis.map_or((nan3, nan3), |a| (a.o, a.d));
        let d = v::norm(d0);
        let rr = fc.r.unwrap_or(f64::NAN);
        let mut t0 = f64::INFINITY;
        let mut t1 = f64::NEG_INFINITY;
        let mut out = 0.0;
        let fi32 = i32::try_from(fi).unwrap_or(-1);
        let pt = |vi: u32| -> V3 { k::pt_at(&pp, vi as usize) };
        for ti in 0..ii.len() / 3 {
            if b.named.tri_face.get(ti).copied() != Some(fi32) {
                continue;
            }
            let q = [pt(k::tri_v(&ii, ti, 0)), pt(k::tri_v(&ii, ti, 1)), pt(k::tri_v(&ii, ti, 2))];
            for p in q {
                let hh = v::dot(v::sub(p, o), d);
                t0 = js::min(t0, hh);
                t1 = js::max(t1, hh);
            }
            let c = v::mul(v::add(v::add(q[0], q[1]), q[2]), 1.0 / 3.0);
            let nn = v::cross(v::sub(q[1], q[0]), v::sub(q[2], q[0]));
            let vv = v::sub(c, o);
            let rad = v::sub(vv, v::mul(d, v::dot(vv, d)));
            out += v::dot(nn, rad) / js::max(1e-30, v::len(rad));
        }
        if !(t1 > t0) {
            return Err(tk("thread.err.round"));
        }
        let external = out > 0.0;
        let size_v = js::str_of(f.get("size"));
        let size: String =
            if size_v == "auto" { threads::size_for(&form, 2.0 * rr, external).unwrap_or("").to_string() } else { size_v.clone() };
        if size.is_empty() {
            return Err(t("thread.err.no_size", &[("d", F::S(fmt_mm(2.0 * rr)))]));
        }
        let Some(g) = threads::thread_dims(&form, &size) else {
            return Err(t("model.kernel.build_failed", &[("error", F::S("Cannot read properties of null (reading 'd')".into()))]));
        };
        if size_v != "auto" {
            let want = if external { g.d } else { g.d1 };
            if (2.0 * rr - want).abs() > 0.12 * want {
                warnings.push(t(
                    if external { "thread.warn.shaft" } else { "thread.warn.bore" },
                    &[("d", F::S(fmt_mm(2.0 * rr))), ("size", F::S(size.clone())), ("want", F::S(fmt_mm(want)))],
                ));
            }
        }
        let dl = js::min(0.5 * g.p, 0.25 * rr);
        let s = js::min(dl, 0.25 * rr);
        let open0 = !inside(ctx, &body, v::add(o, v::mul(d, t0 - dl)), s);
        let open1 = !inside(ctx, &body, v::add(o, v::mul(d, t1 + dl)), s);
        let mut from_hi = open1 && !open0;
        if jv::truthy(f.get("flip")) {
            from_hi = !from_hi;
        }
        let full = t1 - t0;
        let mut len = if jv::nullish(f.get("length")) { full } else { n(f, "length") };
        if len > full + 1e-6 {
            warnings.push(t("thread.warn.long", &[("length", F::S(fmt_mm(len))), ("full", F::S(fmt_mm(full)))]));
            len = full;
        }
        let dz = if from_hi { v::mul(d, -1.0) } else { d };
        let origin = v::add(o, v::mul(d, if from_hi { t1 } else { t0 }));
        let start_open = if from_hi { open1 } else { open0 };
        let end_open = if from_hi { open0 } else { open1 };
        let ext0 = if start_open { g.p } else { 0.0 };
        let ext1 = if end_open && len >= full - 1e-6 { g.p } else { 0.0 };
        let c = if modelled { if jv::nullish(f.get("clearance")) { threads::DEFAULT_CLEARANCE } else { n(f, "clearance") } } else { 0.0 };
        let through = !external && open0 && open1 && len >= full - 1e-6;
        let callout = threads::thread_callout(&form, &size, external, len, through, modelled)?;
        list.push(jv::obj(vec![
            ("o", jv::pt3(origin)),
            ("d", jv::pt3(dz)),
            ("r", jv::num(if external { g.d1 / 2.0 } else { g.d / 2.0 })),
            ("r0", jv::num(js::min(g.d1 / 2.0, rr))),
            ("r1", jv::num(js::max(g.d / 2.0, if external { rr } else { 0.0 }))),
            ("depth", jv::num(len)),
            ("callout", callout.into()),
        ]));
        if !modelled {
            continue;
        }
        let hand = jv::as_str(f.get("hand")).unwrap_or("right").to_string();
        let local = thread_tool(
            ctx,
            &ThreadOpts {
                form: form.clone(),
                size: size.clone(),
                external,
                len,
                clearance: c,
                hand,
                segments: res,
                outer: (js::max(rr, g.d / 2.0) + 1e-4 * rr) / js::cos(std::f64::consts::PI / res),
                ext0,
                ext1,
                face: Some((rr, ctx.resolution_or64())),
            },
        )?;
        let Some(local) = local else {
            return Err(t("model.kernel.build_failed", &[("error", F::S("Cannot read properties of null (reading 'transform')".into()))]));
        };
        let x = v::norm(if dz[0].abs() < 0.9 { v::cross(dz, [1.0, 0.0, 0.0]) } else { v::cross(dz, [0.0, 1.0, 0.0]) });
        let y = v::cross(dz, x);
        let m = ctx.t(k::xf(&local, &v::basis_matrix(x, y, dz, origin)));
        let shift = if external { -c } else { c };
        let nm = |kk: &str| format!("{fid}.{kk}:{i}");
        let mut surfs = vec![k::surf_thread(&nm("thread"), origin, dz, g.d1 / 2.0 + shift, g.d / 2.0 + shift, -ext0, len + ext1)];
        if ext1 == 0.0 {
            surfs.push(k::surf_within(
                k::surf_plane(&nm("tend"), dz, v::add(origin, v::mul(dz, len))),
                origin,
                dz,
                js::max(rr, g.d / 2.0) * 1.01 + 1e-3,
            ));
        }
        tools.push(Tool { m, op: "cut".into(), surfs: Some(Value::Array(surfs)) });
    }
    let mut callouts: Vec<(String, usize)> = Vec::new();
    for th in &list {
        let co = js::str_of(th.get("callout"));
        match callouts.iter_mut().find(|x| x.0 == co) {
            Some(x) => x.1 = x.1.saturating_add(1),
            None => callouts.push((co, 1)),
        }
    }
    let first = callouts.first().map(|x| x.0.clone()).unwrap_or_default();
    ctx.threads.set(
        &fid,
        jv::obj(vec![
            ("callout", first.into()),
            (
                "callouts",
                Value::Array(callouts.iter().map(|(c, n)| jv::obj(vec![("callout", c.clone().into()), ("count", jv::unum(*n))])).collect()),
            ),
            ("modelled", modelled.into()),
            ("list", Value::Array(list)),
        ]),
    );
    Ok(Out { tools: Some(tools), warning: (!warnings.is_empty()).then(|| warnings.join(" ")), ..Out::default() })
}

/// threadFeature's `inside(q, s)`: does a small cube about q keep some of the body's volume?
fn inside(ctx: &mut Ctx, body: &H, q: V3, s: f64) -> bool {
    let c0 = ctx.t(Manifold::cube([s, s, s], true));
    let c = ctx.t(c0.translate(q));
    let i = ctx.t(body.intersect(&c));
    i.volume() > 0.05 * s * s * s
}
