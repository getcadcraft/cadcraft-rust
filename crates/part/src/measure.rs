//! measure.js — measuring in 3D on a built part (or placed instances): snapping candidates, the facts of one pick,
//! the distance and angle between two picks (exact for points, else over the tessellation), mass properties over the
//! closed mesh (divergence theorem), principal moments (cyclic Jacobi) and the panel's rows. Builds and picks come in
//! as the JS objects (JSON); positions and edge segments are f32 values, as the kernel's typed arrays hold them.

use crate::bom;
use crate::i18n::{F, t as tr};
use crate::js;
use crate::jv;
use crate::vec::{self as v, M4, V3};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

const IDENT: M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

pub fn is_point_kind(k: &str) -> bool {
    matches!(k, "vertex" | "midpoint" | "center" | "point")
}
fn p3(x: Option<&Value>) -> V3 {
    let a = jv::arr(x);
    let g = |i: usize| jv::as_num(a.get(i)).unwrap_or(f64::NAN);
    [g(0), g(1), g(2)]
}
fn m4(x: Option<&Value>) -> M4 {
    match x.filter(|m| jv::truthy(Some(m))) {
        None => IDENT,
        Some(m) => {
            let a = jv::arr(Some(m));
            let mut out = [f64::NAN; 16];
            for (i, o) in out.iter_mut().enumerate() {
                *o = jv::as_num(a.get(i)).unwrap_or(f64::NAN);
            }
            out
        }
    }
}
fn nums(x: Option<&Value>) -> Vec<f64> {
    jv::arr(x).iter().map(|y| jv::as_num(Some(y)).unwrap_or(f64::NAN)).collect()
}

/// The parts of a build measuring reads.
pub struct BuildM {
    pub exact: bool,
    pub named: bool,
    pub edges: Vec<Value>,
    pub faces: Vec<Value>,
    pub positions: Vec<f64>,
    pub indices: Vec<u32>,
    pub tri_face: Option<Vec<f64>>,
    pub edge_segs: Option<Vec<f64>>,
    pub seg_edge: Option<Vec<f64>>,
    pub has_mesh: bool,
    pub stats: Option<Value>,
}
impl BuildM {
    pub fn from_json(b: &Value) -> BuildM {
        let m = b.get("mesh").filter(|m| jv::truthy(Some(m)));
        let arr_opt = |k: &str| m.and_then(|m| m.get(k)).filter(|x| x.is_array()).map(|x| nums(Some(x)));
        BuildM {
            exact: jv::truthy(b.get("exact")),
            named: jv::truthy(b.get("named")),
            edges: jv::arr(b.get("edges")).to_vec(),
            faces: jv::arr(b.get("faces")).to_vec(),
            positions: m.map(|m| nums(m.get("positions"))).unwrap_or_default(),
            indices: m
                .map(|m| {
                    jv::arr(m.get("indices")).iter().map(|x| x.as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(u32::MAX)).collect()
                })
                .unwrap_or_default(),
            tri_face: arr_opt("triFace"),
            edge_segs: arr_opt("edgeSegs"),
            seg_edge: arr_opt("segEdge"),
            has_mesh: m.is_some(),
            stats: b.get("stats").filter(|s| jv::truthy(Some(s))).cloned(),
        }
    }
    fn pt(&self, i: usize) -> V3 {
        let g = |k: usize| i.checked_mul(3).and_then(|b| b.checked_add(k)).and_then(|j| self.positions.get(j)).copied().unwrap_or(f64::NAN);
        [g(0), g(1), g(2)]
    }
    fn idx(&self, i: usize) -> usize {
        self.indices.get(i).map_or(usize::MAX, |&x| x as usize)
    }
    fn tri_pts(&self, t: usize) -> [V3; 3] {
        let b = t.saturating_mul(3);
        [self.pt(self.idx(b)), self.pt(self.idx(b.saturating_add(1))), self.pt(self.idx(b.saturating_add(2)))]
    }
    fn seg(&self, k: usize) -> (V3, V3) {
        let s = self.edge_segs.as_deref().unwrap_or(&[]);
        let g = |i: usize| s.get(k.saturating_mul(6).saturating_add(i)).copied().unwrap_or(f64::NAN);
        ([g(0), g(1), g(2)], [g(3), g(4), g(5)])
    }
}

fn g1(a: &[f64; 3], i: usize) -> f64 {
    a.get(i).copied().unwrap_or(f64::NAN)
}
fn g3(m: &[[f64; 3]; 3], i: usize, j: usize) -> f64 {
    m.get(i).and_then(|r| r.get(j)).copied().unwrap_or(f64::NAN)
}
fn s3(m: &mut [[f64; 3]; 3], i: usize, j: usize, x: f64) {
    if let Some(c) = m.get_mut(i).and_then(|r| r.get_mut(j)) {
        *c = x;
    }
}

/// `circleOf(e, build, index)` → (c, r, n).
pub fn circle_of(e: &Value, build: Option<&BuildM>, index: f64) -> Option<(V3, f64, Option<V3>)> {
    let (ea, eb, eat) = (p3(e.get("a")), p3(e.get("b")), p3(e.get("at")));
    let full = v::len(v::sub(ea, eb)) < 1e-9 * js::max(1.0, v::len(ea));
    let c = if full {
        v::mul(v::add(ea, eat), 0.5)
    } else {
        let (a, b, cc) = (ea, eat, eb);
        let ab = v::sub(b, a);
        let ac = v::sub(cc, a);
        let n = v::cross(ab, ac);
        let nn = v::dot(n, n);
        if nn < 1e-24 {
            return None;
        }
        v::add(a, v::mul(v::add(v::mul(v::cross(n, ab), v::dot(ac, ac)), v::mul(v::cross(ac, n), v::dot(ab, ab))), 1.0 / (2.0 * nn)))
    };
    let r = if full { v::len(v::sub(ea, eat)) / 2.0 } else { v::len(v::sub(ea, c)) };
    let mut n = if full { None } else { Some(v::norm(v::cross(v::sub(ea, c), v::sub(eb, c)))) };
    if full || n.is_some_and(|x| v::len(x) < 0.5) {
        n = None;
        if let Some(b) = build
            && b.has_mesh
            && let (Some(_), Some(se)) = (&b.edge_segs, &b.seg_edge)
        {
            let mut best = 0.0;
            for (k, s) in se.iter().enumerate() {
                if *s != index {
                    continue;
                }
                let (q, _) = b.seg(k);
                let x = v::cross(v::sub(ea, c), v::sub(q, c));
                if v::len(x) > best {
                    best = v::len(x);
                    n = Some(v::norm(x));
                }
            }
        }
    }
    Some((c, r, n))
}

/// A snap candidate.
#[derive(Clone, Debug)]
pub struct Cand {
    pub kind: &'static str,
    pub p: V3,
    pub edge: f64,
}

/// `snapCandidates(build)`.
pub fn snap_candidates(b: &BuildM) -> Vec<Cand> {
    let mut out: Vec<Cand> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut add = |out: &mut Vec<Cand>, kind: &'static str, p: V3, edge: f64| {
        let k = format!("{kind}{}", p.iter().map(|x| js::to_fixed(*x, 6)).collect::<Vec<_>>().join(","));
        if seen.contains(&k) {
            return;
        }
        seen.insert(k);
        out.push(Cand { kind, p, edge });
    };
    if !b.edges.is_empty() {
        for (i, e) in b.edges.iter().enumerate() {
            let fi = js::f(i);
            let (ea, eb, eat) = (p3(e.get("a")), p3(e.get("b")), p3(e.get("at")));
            if v::len(v::sub(ea, eb)) > 1e-9 {
                add(&mut out, "vertex", ea, fi);
                add(&mut out, "vertex", eb, fi);
            }
            match jv::s(e, "type") {
                "line" => add(&mut out, "midpoint", v::mul(v::add(ea, eb), 0.5), fi),
                "circle" => {
                    if let Some((c, _, _)) = circle_of(e, Some(b), fi) {
                        add(&mut out, "center", c, fi);
                    }
                    if v::len(v::sub(ea, eb)) > 1e-9 {
                        add(&mut out, "midpoint", eat, fi);
                    }
                }
                _ => add(&mut out, "midpoint", eat, fi),
            }
        }
        return out;
    }
    if !b.has_mesh {
        return out;
    }
    let nt = b.indices.len() / 3;
    let tn: Vec<V3> = (0..nt)
        .map(|t| {
            let [a, bb, c] = b.tri_pts(t);
            v::norm(v::cross(v::sub(bb, a), v::sub(c, a)))
        })
        .collect();
    let mut order: Vec<(usize, usize)> = Vec::new();
    let mut edge_t: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for t in 0..nt {
        for k in 0..3usize {
            let a = b.idx(t.saturating_mul(3).saturating_add(k));
            let c = b.idx(t.saturating_mul(3).saturating_add(k.saturating_add(1) % 3));
            let key = if a < c { (a, c) } else { (c, a) };
            edge_t
                .entry(key)
                .or_insert_with(|| {
                    order.push(key);
                    Vec::new()
                })
                .push(t);
        }
    }
    let cos20 = js::cos(20.0 * std::f64::consts::PI / 180.0);
    let mut deg_order: Vec<usize> = Vec::new();
    let mut deg: HashMap<usize, usize> = HashMap::new();
    for key in &order {
        let ts = edge_t.get(key).map_or(&[][..], Vec::as_slice);
        if let [t0, t1] = ts
            && v::dot(tn.get(*t0).copied().unwrap_or([f64::NAN; 3]), tn.get(*t1).copied().unwrap_or([f64::NAN; 3])) > cos20
        {
            continue;
        }
        for vx in [key.0, key.1] {
            let e = deg.entry(vx).or_insert_with(|| {
                deg_order.push(vx);
                0
            });
            *e = e.saturating_add(1);
        }
    }
    for vx in deg_order {
        if deg.get(&vx).copied().unwrap_or(0) >= 3 && out.len() < 20000 {
            add(&mut out, "vertex", b.pt(vx), -1.0);
        }
    }
    out
}

/// A screen projection: `project(p)` → [x, y] or None.
pub type Project<'a> = &'a dyn Fn(V3) -> Option<[f64; 2]>;

/// `snapAt(cands, project, x, y, tolPx)`.
pub fn snap_at<'a>(cands: &'a [Cand], project: Project, x: f64, y: f64, tol_px: f64) -> Option<&'a Cand> {
    let mut best: Option<&Cand> = None;
    let mut bd = tol_px;
    for c in cands {
        let Some(s) = project(c.p) else { continue };
        let rank = match c.kind {
            "vertex" => 0.0,
            "center" => 1.0,
            _ => 2.0,
        };
        let d = js::hypot2(s[0] - x, s[1] - y) - (2.0 - rank) * 0.01;
        if d < bd {
            bd = d;
            best = Some(c);
        }
    }
    best
}

fn seg_point2(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l = dx * dx + dy * dy;
    let tt = if l != 0.0 && !l.is_nan() { js::max(0.0, js::min(1.0, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l)) } else { 0.0 };
    js::hypot2(a[0] + dx * tt - p[0], a[1] + dy * tt - p[1])
}

/// `nearestEdgeAt(build, project, x, y, tolPx)` → (index, d).
pub fn nearest_edge_at(b: &BuildM, project: Project, x: f64, y: f64, tol_px: f64) -> (f64, f64) {
    let (Some(_), Some(se)) = (&b.edge_segs, &b.seg_edge) else { return (-1.0, f64::INFINITY) };
    if !b.has_mesh {
        return (-1.0, f64::INFINITY);
    }
    let mut best = -1.0;
    let mut bd = tol_px;
    for (k, s) in se.iter().enumerate() {
        if *s < 0.0 {
            continue;
        }
        let (pa, pb) = b.seg(k);
        let (Some(a), Some(bb)) = (project(pa), project(pb)) else { continue };
        let d = seg_point2(a, bb, [x, y]);
        if d < bd {
            bd = d;
            best = *s;
        }
    }
    (best, if best < 0.0 { f64::INFINITY } else { bd })
}

fn w(m: &M4, p: V3) -> V3 {
    v::apply_mat(m, p)
}
fn wd(m: &M4, d: V3) -> V3 {
    v::norm(v::apply_dir(m, d))
}

fn face_tris(b: &BuildM, pick: &Value) -> Vec<usize> {
    let mut out = Vec::new();
    if !b.has_mesh {
        return out;
    }
    let index = jv::as_num(pick.get("index"));
    if let (Some(ix), Some(tf)) = (index.filter(|x| *x >= 0.0), &b.tri_face) {
        for (t, f) in tf.iter().enumerate() {
            if *f == ix {
                out.push(t);
            }
        }
    } else if let Some(ts) = pick.get("tris").filter(|x| jv::truthy(Some(x))) {
        out.extend(jv::arr(Some(ts)).iter().filter_map(|x| x.as_u64().and_then(|n| usize::try_from(n).ok())));
    }
    out
}

/// `info(build, pick)`.
pub fn info(b: &BuildM, pick: &Value) -> Result<Value, String> {
    let m = m4(pick.get("matrix"));
    let kind = js::str_of(pick.get("kind"));
    let mut out = jv::obj(vec![("kind", pick.get("kind").cloned().unwrap_or(Value::Null)), ("exact", b.exact.into())]);
    if is_point_kind(&kind) {
        jv::set(&mut out, "p", jv::pt3(w(&m, p3(pick.get("local")))));
        return Ok(out);
    }
    if kind == "body" {
        let st = b.stats.as_ref();
        jv::set(&mut out, "volume", st.and_then(|s| s.get("volume")).filter(|x| !x.is_null()).cloned().unwrap_or(Value::Null));
        jv::set(&mut out, "area", st.and_then(|s| s.get("area")).filter(|x| !x.is_null()).cloned().unwrap_or(Value::Null));
        return Ok(out);
    }
    if kind == "edge" {
        let ix = jv::as_num(pick.get("index")).unwrap_or(f64::NAN);
        let e = js::usize_of(js::i64_of(ix));
        let e = if ix >= 0.0 && js::is_integer(ix) { b.edges.get(e) } else { None };
        let Some(e) = e else { return Err(tr("model.measure.edge_gone", &[])) };
        let (ea, eb, eat) = (p3(e.get("a")), p3(e.get("b")), p3(e.get("at")));
        jv::set(&mut out, "type", e.get("type").cloned().unwrap_or(Value::Null));
        jv::set(&mut out, "length", e.get("len").cloned().unwrap_or(Value::Null));
        jv::set(&mut out, "a", jv::pt3(w(&m, ea)));
        jv::set(&mut out, "b", jv::pt3(w(&m, eb)));
        jv::set(&mut out, "p", jv::pt3(w(&m, eat)));
        jv::set(&mut out, "name", e.get("key").cloned().unwrap_or(Value::Null));
        if jv::s(e, "type") == "line" {
            jv::set(&mut out, "dir", jv::pt3(wd(&m, v::sub(eb, ea))));
        }
        if jv::s(e, "type") == "circle"
            && let Some((c, r, n)) = circle_of(e, Some(b), ix)
        {
            jv::set(&mut out, "centre", jv::pt3(w(&m, c)));
            jv::set(&mut out, "r", jv::num(r));
            if let Some(n) = n {
                jv::set(&mut out, "axis", jv::pt3(wd(&m, n)));
            }
            jv::set(&mut out, "full", (v::len(v::sub(ea, eb)) < 1e-9 * js::max(1.0, v::len(ea))).into());
        }
        return Ok(out);
    }
    if kind == "face" {
        let ix = jv::as_num(pick.get("index"));
        let face = ix.filter(|x| *x >= 0.0 && js::is_integer(*x)).and_then(|x| b.faces.get(js::usize_of(js::i64_of(x))));
        if (b.exact || b.named)
            && let Some(f) = face.filter(|f| jv::truthy(Some(f)))
        {
            jv::set(&mut out, "type", f.get("type").cloned().unwrap_or(Value::Null));
            jv::set(&mut out, "area", f.get("area").cloned().unwrap_or(Value::Null));
            jv::set(&mut out, "p", jv::pt3(w(&m, p3(f.get("at")))));
            jv::set(&mut out, "name", f.get("name").cloned().unwrap_or(Value::Null));
            if jv::s(f, "type") == "plane" {
                jv::set(&mut out, "n", jv::pt3(wd(&m, p3(f.get("n")))));
                jv::set(&mut out, "o", jv::pt3(w(&m, p3(f.get("o")))));
            }
            if jv::s(f, "type") == "cylinder" {
                jv::set(&mut out, "r", f.get("r").cloned().unwrap_or(Value::Null));
                let ax = f.get("axis");
                jv::set(
                    &mut out,
                    "axisLine",
                    jv::obj(vec![
                        ("o", jv::pt3(w(&m, p3(ax.and_then(|a| a.get("o")))))),
                        ("d", jv::pt3(wd(&m, p3(ax.and_then(|a| a.get("d")))))),
                    ]),
                );
            }
            return Ok(out);
        }
        let tris = face_tris(b, pick);
        let mut area = 0.0;
        let mut nsum = [0.0; 3];
        for tt in tris {
            let [a, bb, c] = b.tri_pts(tt);
            let x = v::cross(v::sub(bb, a), v::sub(c, a));
            area += v::len(x) / 2.0;
            nsum = v::add(nsum, x);
        }
        let local = p3(pick.get("local"));
        jv::set(&mut out, "type", "facets".into());
        jv::set(&mut out, "area", jv::num(area));
        jv::set(&mut out, "exact", false.into());
        jv::set(&mut out, "p", jv::pt3(w(&m, local)));
        if v::len(nsum) > 0.999 * 2.0 * area {
            jv::set(&mut out, "type", "plane".into());
            jv::set(&mut out, "n", jv::pt3(wd(&m, nsum)));
            jv::set(&mut out, "o", jv::pt3(w(&m, local)));
        }
        return Ok(out);
    }
    Err("Unknown kind of pick.".into())
}

/// `exactSub(pick)`.
pub fn exact_sub(pick: &Value) -> Value {
    let kind = js::str_of(pick.get("kind"));
    if is_point_kind(&kind) {
        return jv::obj(vec![("kind", "point".into()), ("p", pick.get("local").cloned().unwrap_or(Value::Null))]);
    }
    if kind == "body" {
        return jv::obj(vec![("kind", "body".into())]);
    }
    let mut o = jv::obj(vec![("kind", pick.get("kind").cloned().unwrap_or(Value::Null))]);
    if let Some(ix) = pick.get("index") {
        jv::set(&mut o, "index", ix.clone());
    }
    if let Some(at) = pick.get("at").filter(|a| jv::truthy(Some(a))) {
        jv::set(&mut o, "at", at.clone());
    }
    o
}

/// Tessellated pick geometry.
#[derive(Clone, Debug, Default)]
pub struct Geo {
    pub pts: Vec<V3>,
    pub segs: Vec<[V3; 2]>,
    pub tris: Vec<[V3; 3]>,
}

/// `geometryOf(build, pick)`.
pub fn geometry_of(b: &BuildM, pick: &Value) -> Geo {
    let m = m4(pick.get("matrix"));
    let kind = js::str_of(pick.get("kind"));
    if is_point_kind(&kind) {
        return Geo { pts: vec![w(&m, p3(pick.get("local")))], ..Geo::default() };
    }
    if kind == "edge" {
        let ix = jv::as_num(pick.get("index")).unwrap_or(f64::NAN);
        let mut segs = Vec::new();
        for (k, s) in b.seg_edge.as_deref().unwrap_or(&[]).iter().enumerate() {
            if *s == ix {
                let (pa, pb) = b.seg(k);
                segs.push([w(&m, pa), w(&m, pb)]);
            }
        }
        let pts = segs.iter().flat_map(|s| s.iter().copied()).collect();
        return Geo { pts, segs, tris: Vec::new() };
    }
    let ids: Vec<usize> = if kind == "body" { (0..b.indices.len() / 3).collect() } else { face_tris(b, pick) };
    let tris: Vec<[V3; 3]> = ids.iter().map(|&t| b.tri_pts(t).map(|p| w(&m, p))).collect();
    let mut segs = Vec::new();
    let mut pts = Vec::new();
    for [a, bb, c] in &tris {
        segs.push([*a, *bb]);
        segs.push([*bb, *c]);
        segs.push([*c, *a]);
        pts.push(*a);
        pts.push(*bb);
        pts.push(*c);
    }
    Geo { pts, segs, tris }
}

/// `pointTri(p, [a, b, c])` (Ericson 5.1.5).
pub fn point_tri(p: V3, tri: &[V3; 3]) -> V3 {
    let [a, b, c] = *tri;
    let ab = v::sub(b, a);
    let ac = v::sub(c, a);
    let ap = v::sub(p, a);
    let (d1, d2) = (v::dot(ab, ap), v::dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = v::sub(p, b);
    let (d3, d4) = (v::dot(ab, bp), v::dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return v::add(a, v::mul(ab, d1 / (d1 - d3)));
    }
    let cp = v::sub(p, c);
    let (d5, d6) = (v::dot(ab, cp), v::dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return v::add(a, v::mul(ac, d2 / (d2 - d6)));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
        return v::add(b, v::mul(v::sub(c, b), (d4 - d3) / ((d4 - d3) + (d5 - d6))));
    }
    let den = 1.0 / (va + vb + vc);
    v::add(a, v::add(v::mul(ab, vb * den), v::mul(ac, vc * den)))
}

/// `segSeg([p1, q1], [p2, q2])` (Ericson 5.1.9).
pub fn seg_seg(s1: &[V3; 2], s2: &[V3; 2]) -> [V3; 2] {
    let [p1, q1] = *s1;
    let [p2, q2] = *s2;
    let d1 = v::sub(q1, p1);
    let d2 = v::sub(q2, p2);
    let r = v::sub(p1, p2);
    let (a, e, f) = (v::dot(d1, d1), v::dot(d2, d2), v::dot(d2, r));
    let clamp = |x: f64| js::max(0.0, js::min(1.0, x));
    if a <= 1e-18 && e <= 1e-18 {
        return [p1, p2];
    }
    let (s, tt);
    if a <= 1e-18 {
        s = 0.0;
        tt = clamp(f / e);
    } else {
        let c = v::dot(d1, r);
        if e <= 1e-18 {
            tt = 0.0;
            s = clamp(-c / a);
        } else {
            let b = v::dot(d1, d2);
            let den = a * e - b * b;
            let s0 = if den > 1e-18 { clamp((b * f - c * e) / den) } else { 0.0 };
            let t0 = (b * s0 + f) / e;
            if t0 < 0.0 {
                tt = 0.0;
                s = clamp(-c / a);
            } else if t0 > 1.0 {
                tt = 1.0;
                s = clamp((b - c) / a);
            } else {
                tt = t0;
                s = s0;
            }
        }
    }
    [v::add(p1, v::mul(d1, s)), v::add(p2, v::mul(d2, tt))]
}

/// `meshDistance(A, B)` → (distance, p1, p2).
pub fn mesh_distance(a: &Geo, b: &Geo) -> (f64, Option<V3>, Option<V3>) {
    let mut best = (f64::INFINITY, None::<V3>, None::<V3>);
    let mut take = |p: V3, q: V3| {
        let d = v::len(v::sub(p, q));
        if d < best.0 {
            best = (d, Some(p), Some(q));
        }
    };
    for p in &a.pts {
        for q in &b.pts {
            take(*p, *q);
        }
    }
    for p in &a.pts {
        for tri in &b.tris {
            take(*p, point_tri(*p, tri));
        }
    }
    for q in &b.pts {
        for tri in &a.tris {
            take(point_tri(*q, tri), *q);
        }
    }
    for p in &a.pts {
        for s in &b.segs {
            take(*p, seg_seg(&[*p, *p], s)[1]);
        }
    }
    for q in &b.pts {
        for s in &a.segs {
            take(seg_seg(s, &[*q, *q])[0], *q);
        }
    }
    if js::f(a.segs.len()) * js::f(b.segs.len()) <= 4e6 {
        for s in &a.segs {
            for u in &b.segs {
                let [p, q] = seg_seg(s, u);
                take(p, q);
            }
        }
    }
    best
}

fn deg(r: f64) -> f64 {
    r * 180.0 / std::f64::consts::PI
}
fn acos_c(x: f64) -> f64 {
    js::acos(js::max(-1.0, js::min(1.0, x)))
}
fn dir_of(inf: &Value) -> Option<(&'static str, V3)> {
    if jv::truthy(inf.get("n")) {
        return Some(("plane", p3(inf.get("n"))));
    }
    if jv::truthy(inf.get("dir")) {
        return Some(("line", p3(inf.get("dir"))));
    }
    if let Some(al) = inf.get("axisLine").filter(|x| jv::truthy(Some(x))) {
        return Some(("line", p3(al.get("d"))));
    }
    if jv::truthy(inf.get("axis")) && jv::truthy(inf.get("centre")) {
        return Some(("line", p3(inf.get("axis"))));
    }
    None
}
fn centre_line(inf: &Value) -> Option<(V3, V3)> {
    if let Some(al) = inf.get("axisLine").filter(|x| jv::truthy(Some(x))) {
        return Some((p3(al.get("o")), p3(al.get("d"))));
    }
    if jv::truthy(inf.get("centre")) && jv::truthy(inf.get("axis")) {
        return Some((p3(inf.get("centre")), p3(inf.get("axis"))));
    }
    None
}

/// `pair(A, B, ia, ib, exactDistance)`; geometry = the picks' tessellated geometry when they carry it.
pub fn pair(a_kind: &str, b_kind: &str, ia: &Value, ib: &Value, exact_distance: Option<&Value>, geo: Option<(&Geo, &Geo)>) -> Value {
    let mut out = jv::obj(vec![]);
    let mut p12: Option<(V3, V3)> = None;
    if is_point_kind(a_kind) && is_point_kind(b_kind) {
        let (pa, pb) = (p3(ia.get("p")), p3(ib.get("p")));
        jv::set(&mut out, "distance", jv::num(v::len(v::sub(pb, pa))));
        jv::set(&mut out, "p1", jv::pt3(pa));
        jv::set(&mut out, "p2", jv::pt3(pb));
        jv::set(&mut out, "method", "exact".into());
        p12 = Some((pa, pb));
    } else if let Some(ed) = exact_distance.filter(|x| jv::truthy(Some(x))) {
        jv::set(&mut out, "distance", ed.get("distance").cloned().unwrap_or(Value::Null));
        jv::set(&mut out, "p1", ed.get("p1").cloned().unwrap_or(Value::Null));
        jv::set(&mut out, "p2", ed.get("p2").cloned().unwrap_or(Value::Null));
        jv::set(&mut out, "method", "exact".into());
        if let Some(i) = ed.get("inside") {
            jv::set(&mut out, "inside", i.clone());
        }
        if jv::truthy(ed.get("p1")) {
            p12 = Some((p3(ed.get("p1")), p3(ed.get("p2"))));
        }
    } else if let Some((ga, gb)) = geo {
        let (d, p1, p2) = mesh_distance(ga, gb);
        jv::set(&mut out, "distance", jv::num(d));
        jv::set(&mut out, "p1", p1.map_or(Value::Null, jv::pt3));
        jv::set(&mut out, "p2", p2.map_or(Value::Null, jv::pt3));
        jv::set(&mut out, "method", "facets".into());
        if let (Some(a), Some(b)) = (p1, p2) {
            p12 = Some((a, b));
        }
    }
    if let Some((a, b)) = p12 {
        jv::set(&mut out, "delta", jv::pt3(v::sub(b, a)));
    }
    let (da, db) = (dir_of(ia), dir_of(ib));
    if let (Some((ka, va)), Some((kb, vb))) = (da, db) {
        let c = v::dot(va, vb);
        let ang = if ka == "plane" && kb == "plane" {
            deg(acos_c(c))
        } else if ka == "line" && kb == "line" {
            deg(acos_c(c.abs()))
        } else {
            90.0 - deg(acos_c(c.abs()))
        };
        jv::set(&mut out, "angle", jv::num(ang));
        if ka == "plane" && kb == "plane" && (v::dot(va, vb).abs() - 1.0).abs() < 1e-9 {
            jv::set(&mut out, "planeGap", jv::num(v::dot(v::sub(p3(ib.get("o")), p3(ia.get("o"))), va).abs()));
        }
    }
    if jv::truthy(ia.get("centre")) && jv::truthy(ib.get("centre")) {
        jv::set(&mut out, "centre", jv::num(v::len(v::sub(p3(ib.get("centre")), p3(ia.get("centre"))))));
    }
    if let (Some((oa, la)), Some((ob, lb))) = (centre_line(ia), centre_line(ib))
        && (v::dot(la, lb).abs() - 1.0).abs() < 1e-9
    {
        let wv = v::sub(ob, oa);
        jv::set(&mut out, "axisGap", jv::num(v::len(v::sub(wv, v::mul(la, v::dot(wv, la))))));
    }
    out
}

/// Mass properties.
#[derive(Clone, Debug)]
pub struct Mass {
    pub mass: Option<f64>,
    pub volume: f64,
    pub area: f64,
    pub centre: V3,
    pub tensor: [[f64; 3]; 3],
    pub exact: bool,
}
impl Mass {
    pub fn to_json(&self) -> Value {
        let mut o = jv::obj(vec![]);
        if let Some(m) = self.mass {
            jv::set(&mut o, "mass", jv::num(m));
        }
        jv::set(&mut o, "volume", jv::num(self.volume));
        jv::set(&mut o, "area", jv::num(self.area));
        jv::set(&mut o, "centre", jv::pt3(self.centre));
        jv::set(&mut o, "tensor", Value::Array(self.tensor.iter().map(|r| jv::nums(r)).collect()));
        jv::set(&mut o, "exact", self.exact.into());
        o
    }
    pub fn from_json(x: &Value) -> Mass {
        let tt = jv::arr(x.get("tensor"));
        let row = |i: usize| p3(tt.get(i));
        Mass {
            mass: jv::as_num(x.get("mass")),
            volume: jv::as_num(x.get("volume")).unwrap_or(f64::NAN),
            area: jv::as_num(x.get("area")).unwrap_or(f64::NAN),
            centre: p3(x.get("centre")),
            tensor: [row(0), row(1), row(2)],
            exact: jv::truthy(x.get("exact")),
        }
    }
}

/// `meshMass(mesh)`.
pub fn mesh_mass(positions: &[f64], indices: &[u32]) -> Mass {
    let pt = |i: Option<&u32>| -> V3 {
        let i = i.map_or(usize::MAX, |&x| x as usize);
        let g = |k: usize| i.checked_mul(3).and_then(|b| b.checked_add(k)).and_then(|j| positions.get(j)).copied().unwrap_or(f64::NAN);
        [g(0), g(1), g(2)]
    };
    let (mut vol, mut area) = (0.0, 0.0);
    let mut s1 = [0.0f64; 3];
    let mut c = [0.0f64; 6];
    for tri in indices.chunks(3) {
        let (a, b, cc) = (pt(tri.first()), pt(tri.get(1)), pt(tri.get(2)));
        let vv = v::dot(a, v::cross(b, cc)) / 6.0;
        area += v::len(v::cross(v::sub(b, a), v::sub(cc, a))) / 2.0;
        vol += vv;
        let s = v::add(v::add(a, b), cc);
        for (s1k, sk) in s1.iter_mut().zip(s) {
            *s1k += vv * sk / 4.0;
        }
        let q = |i: usize, j: usize| {
            vv / 20.0 * (g1(&a, i) * g1(&a, j) + g1(&b, i) * g1(&b, j) + g1(&cc, i) * g1(&cc, j) + g1(&s, i) * g1(&s, j))
        };
        c[0] += q(0, 0);
        c[1] += q(1, 1);
        c[2] += q(2, 2);
        c[3] += q(0, 1);
        c[4] += q(1, 2);
        c[5] += q(0, 2);
    }
    let g = v::mul(s1, 1.0 / vol);
    let (cxx, cyy, czz) = (c[0] - vol * g[0] * g[0], c[1] - vol * g[1] * g[1], c[2] - vol * g[2] * g[2]);
    let (cxy, cyz, cxz) = (c[3] - vol * g[0] * g[1], c[4] - vol * g[1] * g[2], c[5] - vol * g[0] * g[2]);
    let tensor = [[cyy + czz, -cxy, -cxz], [-cxy, cxx + czz, -cyz], [-cxz, -cyz, cxx + cyy]];
    Mass { mass: None, volume: vol, area, centre: g, tensor, exact: false }
}

/// `placeMass(props, matrix, density)`.
pub fn place_mass(p: &Mass, matrix: Option<&M4>, density: f64) -> Mass {
    let m = matrix.copied().unwrap_or(IDENT);
    let r = [[m[0], m[4], m[8]], [m[1], m[5], m[9]], [m[2], m[6], m[10]]];
    let k = density / 1000.0;
    let tt = p.tensor;
    let mut rt = [[0.0f64; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            s3(&mut rt, i, j, g3(&r, i, 0) * g3(&tt, 0, j) + g3(&r, i, 1) * g3(&tt, 1, j) + g3(&r, i, 2) * g3(&tt, 2, j));
        }
    }
    let mut tensor = [[0.0f64; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            s3(&mut tensor, i, j, k * (g3(&rt, i, 0) * g3(&r, j, 0) + g3(&rt, i, 1) * g3(&r, j, 1) + g3(&rt, i, 2) * g3(&r, j, 2)));
        }
    }
    Mass {
        mass: Some(bom::mass_of(p.volume, density)),
        volume: p.volume,
        area: p.area,
        centre: v::apply_mat(&m, p.centre),
        tensor,
        exact: p.exact,
    }
}

/// `combineMass(list)`.
pub fn combine_mass(list: &[Mass]) -> Mass {
    let mass = list.iter().fold(0.0, |s, x| s + x.mass.unwrap_or(f64::NAN));
    let div = if mass == 0.0 || mass.is_nan() { 1.0 } else { mass };
    let mut centre = [0.0; 3];
    for (k, c) in centre.iter_mut().enumerate() {
        *c = list.iter().fold(0.0, |s, x| s + x.mass.unwrap_or(f64::NAN) * g1(&x.centre, k)) / div;
    }
    let mut tensor = [[0.0f64; 3]; 3];
    for x in list {
        let d = v::sub(x.centre, centre);
        let dd = v::dot(d, d);
        for i in 0..3 {
            for j in 0..3 {
                let add = g3(&x.tensor, i, j) + x.mass.unwrap_or(f64::NAN) * ((if i == j { dd } else { 0.0 }) - g1(&d, i) * g1(&d, j));
                let cur = g3(&tensor, i, j);
                s3(&mut tensor, i, j, cur + add);
            }
        }
    }
    Mass {
        mass: Some(mass),
        volume: list.iter().fold(0.0, |s, x| s + x.volume),
        area: list.iter().fold(0.0, |s, x| s + x.area),
        centre,
        tensor,
        exact: list.iter().all(|x| x.exact),
    }
}

/// `principal(T)` → (moments ascending, axes).
pub fn principal(t: &[[f64; 3]; 3]) -> ([f64; 3], [[f64; 3]; 3]) {
    let mut a = *t;
    let mut vv = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..50 {
        let off = a[0][1].abs() + a[0][2].abs() + a[1][2].abs();
        if off < 1e-15 * (a[0][0].abs() + a[1][1].abs() + a[2][2].abs() + 1e-300) {
            break;
        }
        for (p, q) in [(0usize, 1usize), (0, 2), (1, 2)] {
            if g3(&a, p, q).abs() < 1e-300 {
                continue;
            }
            let th = (g3(&a, q, q) - g3(&a, p, p)) / (2.0 * g3(&a, p, q));
            let sg = js::sign(if th == 0.0 || th.is_nan() { 1.0 } else { th });
            let tt = sg / (th.abs() + js::sqrt(th * th + 1.0));
            let c = 1.0 / js::sqrt(tt * tt + 1.0);
            let s = tt * c;
            for k in 0..3 {
                let (akp, akq) = (g3(&a, k, p), g3(&a, k, q));
                s3(&mut a, k, p, c * akp - s * akq);
                s3(&mut a, k, q, s * akp + c * akq);
            }
            for k in 0..3 {
                let (apk, aqk) = (g3(&a, p, k), g3(&a, q, k));
                s3(&mut a, p, k, c * apk - s * aqk);
                s3(&mut a, q, k, s * apk + c * aqk);
            }
            for k in 0..3 {
                let (vkp, vkq) = (g3(&vv, k, p), g3(&vv, k, q));
                s3(&mut vv, k, p, c * vkp - s * vkq);
                s3(&mut vv, k, q, s * vkp + c * vkq);
            }
        }
    }
    let mut order = [0usize, 1, 2];
    cc_m3d::sort::timsort(&mut order, |i, j| g3(&a, *i, *i) - g3(&a, *j, *j));
    (order.map(|i| g3(&a, i, i)), order.map(|i| [g3(&vv, 0, i), g3(&vv, 1, i), g3(&vv, 2, i)]))
}

fn fdp(x: Option<f64>, dp: usize) -> String {
    match x {
        Some(v) if v.is_finite() => js::num_str(js::fixed_num(v, dp)),
        _ => String::new(),
    }
}
fn f4(x: f64) -> String {
    fdp(Some(x), 4)
}
fn p3s(p: V3) -> String {
    format!("{}, {}, {}", f4(p[0]), f4(p[1]), f4(p[2]))
}

/// `kindLabel(k)`.
pub fn kind_label(k: &str) -> String {
    match k {
        "vertex" | "midpoint" | "center" | "point" | "edge" | "face" | "body" => tr(&format!("model.measure.kind.{k}"), &[]),
        _ => k.to_string(),
    }
}

fn row(k: impl Into<String>, v: impl Into<String>) -> (String, String) {
    (k.into(), v.into())
}

/// `infoRows(inf)`.
pub fn info_rows(inf: &Value) -> Vec<(String, String)> {
    let mut r = Vec::new();
    let kind = js::str_of(inf.get("kind"));
    let num = |k: &str| jv::as_num(inf.get(k));
    if jv::truthy(inf.get("p")) && is_point_kind(&kind) {
        r.push(row("X, Y, Z", p3s(p3(inf.get("p")))));
    }
    if kind == "edge" {
        let ty = jv::s(inf, "type");
        let key = if ty == "line" {
            "model.measure.edge.straight"
        } else if ty == "circle" {
            if jv::truthy(inf.get("full")) { "model.measure.edge.circle" } else { "model.measure.edge.arc" }
        } else {
            "model.measure.edge.curve"
        };
        r.push(row(tr("model.measure.row.edge", &[]), tr(key, &[])));
        r.push(row(tr("model.measure.row.length", &[]), format!("{} mm", fdp(num("length"), 4))));
        if inf.get("r").is_some() {
            let rr = num("r");
            r.push(row(tr("model.measure.row.radius", &[]), format!("{} mm", fdp(rr, 4))));
            r.push(row(tr("model.measure.row.diameter", &[]), format!("{} mm", fdp(rr.map(|x| 2.0 * x), 4))));
            r.push(row(tr("model.measure.row.centre", &[]), p3s(p3(inf.get("centre")))));
        }
    }
    if kind == "face" {
        let ty = js::str_of(inf.get("type"));
        let ft = match ty.as_str() {
            "plane" | "cylinder" | "cone" | "sphere" | "torus" | "spline" | "facets" | "other" => {
                tr(&format!("model.measure.face.{ty}"), &[])
            }
            _ => ty.clone(),
        };
        r.push(row(tr("model.measure.row.face", &[]), ft));
        let area = if jv::truthy(inf.get("exact")) {
            format!("{} mm²", fdp(num("area"), 4))
        } else {
            tr("model.measure.area_facets", &[("v", F::S(fdp(num("area"), 4)))])
        };
        r.push(row(tr("model.measure.row.area", &[]), area));
        if jv::truthy(inf.get("n")) {
            r.push(row(tr("model.measure.row.normal", &[]), p3s(p3(inf.get("n")))));
        }
        if inf.get("r").is_some() {
            let rr = num("r");
            r.push(row(tr("model.measure.row.radius", &[]), format!("{} mm", fdp(rr, 4))));
            r.push(row(tr("model.measure.row.diameter", &[]), format!("{} mm", fdp(rr.map(|x| 2.0 * x), 4))));
        }
    }
    if kind == "body" {
        r.push(row(tr("model.measure.row.volume", &[]), format!("{} mm³", fdp(num("volume"), 3))));
        r.push(row(tr("model.measure.row.area", &[]), format!("{} mm²", fdp(num("area"), 3))));
    }
    r
}

/// `pairRows(res)`.
pub fn pair_rows(res: &Value) -> Vec<(String, String)> {
    let mut r = Vec::new();
    let num = |k: &str| jv::as_num(res.get(k));
    if res.get("distance").is_some() {
        let v = if jv::is_str(res.get("method"), "facets") {
            tr("model.measure.distance_facets", &[("v", F::S(fdp(num("distance"), 4)))])
        } else {
            format!("{} mm", fdp(num("distance"), 4))
        };
        r.push(row(tr("model.measure.row.min_distance", &[]), v));
    }
    if let Some(d) = res.get("delta").filter(|x| jv::truthy(Some(x))) {
        let d = p3(Some(d));
        r.push(row("ΔX", format!("{} mm", f4(d[0]))));
        r.push(row("ΔY", format!("{} mm", f4(d[1]))));
        r.push(row("ΔZ", format!("{} mm", f4(d[2]))));
    }
    if jv::truthy(res.get("p1")) {
        r.push(row(
            tr("model.measure.row.closest", &[]),
            tr("model.measure.closest_points", &[("a", F::S(p3s(p3(res.get("p1"))))), ("b", F::S(p3s(p3(res.get("p2")))))]),
        ));
    }
    if jv::truthy(res.get("inside")) {
        r.push(row(tr("model.measure.row.note", &[]), tr("model.measure.inside", &[])));
    }
    if res.get("angle").is_some() {
        r.push(row(tr("model.measure.row.angle", &[]), format!("{}°", fdp(num("angle"), 6))));
    }
    if res.get("planeGap").is_some() {
        r.push(row(tr("model.measure.row.plane_gap", &[]), format!("{} mm", fdp(num("planeGap"), 4))));
    }
    if res.get("centre").is_some() {
        r.push(row(tr("model.measure.row.centre_to_centre", &[]), format!("{} mm", fdp(num("centre"), 4))));
    }
    if res.get("axisGap").is_some() {
        r.push(row(tr("model.measure.row.axis_to_axis", &[]), format!("{} mm", fdp(num("axisGap"), 4))));
    }
    r
}

/// `massRows(m)`.
pub fn mass_rows(m: &Mass) -> Vec<(String, String)> {
    let (moments, _) = principal(&m.tensor);
    let mut r = Vec::new();
    if let Some(ms) = m.mass {
        r.push(row(
            tr("model.measure.row.mass", &[]),
            if ms >= 1000.0 { format!("{} kg", fdp(Some(ms / 1000.0), 4)) } else { format!("{} g", fdp(Some(ms), 3)) },
        ));
    }
    r.push(row(tr("model.measure.row.volume", &[]), format!("{} mm³", fdp(Some(m.volume), 3))));
    r.push(row(tr("model.measure.row.area", &[]), format!("{} mm²", fdp(Some(m.area), 3))));
    r.push(row(tr("model.measure.row.centre_of_mass", &[]), p3s(m.centre)));
    let unit = if m.mass.is_some() { " kg·mm²" } else { " mm⁵" };
    let s = |x: f64| if m.mass.is_some() { fdp(Some(x / 1000.0), 3) } else { fdp(Some(x), 3) };
    let tt = m.tensor;
    r.push(row("Ixx, Iyy, Izz", format!("{}, {}, {}{unit}", s(tt[0][0]), s(tt[1][1]), s(tt[2][2]))));
    r.push(row("Ixy, Iyz, Ixz", format!("{}, {}, {}{unit}", s(tt[0][1]), s(tt[1][2]), s(tt[0][2]))));
    r.push(row(tr("model.measure.row.principal", &[]), format!("{}{unit}", moments.iter().map(|x| s(*x)).collect::<Vec<_>>().join(", "))));
    r.push(row(
        tr("model.measure.row.method", &[]),
        tr(if m.exact { "model.measure.method.exact" } else { "model.measure.method.mesh" }, &[]),
    ));
    r
}

/// `asText(rows)`.
pub fn as_text(rows: &[(String, String)]) -> String {
    rows.iter().map(|(k, v)| format!("{k}\t{v}")).collect::<Vec<_>>().join("\n")
}
