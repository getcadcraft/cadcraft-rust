//! meshfaces.js — named faces and edges for the mesh kernel. Every tool of a feature carries its surfaces (planes,
//! cylinders, cones, spheres, tori, threads, named the way brep.js names them); after the booleans each triangle is
//! matched to a surface of the tool that made it, connected triangles on one surface become one face record, and
//! mesh edges between two faces are chained into edge records (line / circle / curve) with brep.js's shapes.
//! Map iteration follows JS insertion order; edge segments are stored as f32 (a Float32Array in the JS).

use crate::faces::Mesh;
use crate::js;
use crate::jv;
use crate::vec::{self as v, M4, V3};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

fn p3(x: Option<&Value>) -> V3 {
    let a = jv::arr(x);
    let g = |i: usize| jv::as_num(a.get(i)).unwrap_or(f64::NAN);
    [g(0), g(1), g(2)]
}
fn num(x: Option<&Value>) -> f64 {
    jv::as_num(x).unwrap_or(f64::NAN)
}

/// An axis `{ o, d }`.
#[derive(Clone, Copy, Debug)]
pub struct Axis {
    pub o: V3,
    pub d: V3,
}
#[derive(Clone, Copy, Debug)]
struct Within {
    o: V3,
    d: V3,
    r: f64,
}
#[derive(Clone, Copy, Debug)]
struct Span {
    a: V3,
    d: V3,
    len: f64,
}

/// A tool surface (the JS object is kept for moveSurfs' copies).
#[derive(Clone, Debug)]
pub struct Surf {
    pub name: Option<String>,
    pub ty: String,
    n: V3,
    o: V3,
    axis: Axis,
    r: f64,
    k: f64,
    c: V3,
    big_r: f64,
    r0: f64,
    r1: f64,
    t0: f64,
    t1: f64,
    within: Option<Within>,
    span: Option<Span>,
}

impl Surf {
    pub fn from_json(s: &Value) -> Surf {
        let name = match s.get("name") {
            Some(Value::String(x)) if !x.is_empty() => Some(x.clone()),
            Some(x) if jv::truthy(Some(x)) => Some(js::string_of(Some(x))),
            _ => None,
        };
        let ax = s.get("axis");
        Surf {
            name,
            ty: js::str_of(s.get("type")),
            n: p3(s.get("n")),
            o: p3(s.get("o")),
            axis: Axis { o: p3(ax.and_then(|a| a.get("o"))), d: p3(ax.and_then(|a| a.get("d"))) },
            r: num(s.get("r")),
            k: num(s.get("k")),
            c: p3(s.get("c")),
            big_r: num(s.get("R")),
            r0: num(s.get("r0")),
            r1: num(s.get("r1")),
            t0: num(s.get("t0")),
            t1: num(s.get("t1")),
            within: s.get("within").filter(|w| jv::truthy(Some(w))).map(|w| Within {
                o: p3(w.get("o")),
                d: p3(w.get("d")),
                r: num(w.get("r")),
            }),
            span: s.get("span").filter(|w| jv::truthy(Some(w))).map(|w| Span {
                a: p3(w.get("a")),
                d: p3(w.get("d")),
                len: num(w.get("len")),
            }),
        }
    }
}

/// M4 applied to a direction (the linear part only).
fn dir_of(m: &M4, d: V3) -> V3 {
    v::apply_dir(m, d)
}

/// `moveSurfs(surfs, M, suffix)` on the JSON objects.
pub fn move_surfs(surfs: Option<&Value>, m: &M4, suffix: &str) -> Value {
    let Some(list) = surfs.and_then(Value::as_array) else { return Value::Null };
    Value::Array(
        list.iter()
            .map(|s0| {
                let mut s = s0.clone();
                let name = match s0.get("name") {
                    Some(n) if jv::truthy(Some(n)) => Value::String(format!("{}{suffix}", js::string_of(Some(n)))),
                    Some(n) => n.clone(),
                    None => Value::Null,
                };
                let has_name = s0.get("name").is_some();
                if let Some(w) = s0.get("within").filter(|w| jv::truthy(Some(w))) {
                    let mut w2 = w.clone();
                    jv::set(&mut w2, "o", jv::pt3(v::apply_mat(m, p3(w.get("o")))));
                    jv::set(&mut w2, "d", jv::pt3(v::norm(dir_of(m, p3(w.get("d"))))));
                    jv::set(&mut s, "within", w2);
                }
                if let Some(w) = s0.get("span").filter(|w| jv::truthy(Some(w))) {
                    let mut w2 = w.clone();
                    jv::set(&mut w2, "a", jv::pt3(v::apply_mat(m, p3(w.get("a")))));
                    jv::set(&mut w2, "d", jv::pt3(v::norm(dir_of(m, p3(w.get("d"))))));
                    jv::set(&mut s, "span", w2);
                }
                let set_name = |s: &mut Value| {
                    if has_name || !name.is_null() {
                        jv::set(s, "name", name.clone());
                    } else {
                        // `{ ...s, name }` adds the key even when it was missing (undefined → dropped in JSON)
                    }
                };
                match jv::s(s0, "type") {
                    "plane" => {
                        set_name(&mut s);
                        jv::set(&mut s, "n", jv::pt3(v::norm(dir_of(m, p3(s0.get("n"))))));
                        jv::set(&mut s, "o", jv::pt3(v::apply_mat(m, p3(s0.get("o")))));
                    }
                    "cylinder" | "cone" | "torus" | "thread" => {
                        set_name(&mut s);
                        let ax = s0.get("axis");
                        jv::set(
                            &mut s,
                            "axis",
                            jv::obj(vec![
                                ("o", jv::pt3(v::apply_mat(m, p3(ax.and_then(|a| a.get("o")))))),
                                ("d", jv::pt3(v::norm(dir_of(m, p3(ax.and_then(|a| a.get("d"))))))),
                            ]),
                        );
                    }
                    "sphere" => {
                        set_name(&mut s);
                        jv::set(&mut s, "c", jv::pt3(v::apply_mat(m, p3(s0.get("c")))));
                    }
                    _ => set_name(&mut s),
                }
                s
            })
            .collect(),
    )
}

fn sub(a: V3, b: V3) -> V3 {
    v::sub(a, b)
}
fn hyp(a: V3) -> f64 {
    js::hypot3(a[0], a[1], a[2])
}
/// `radial(p, ax)` → [distance from the axis, height along it].
fn radial(p: V3, o: V3, d: V3) -> [f64; 2] {
    let w = sub(p, o);
    let h = v::dot(w, d);
    [hyp(sub(w, v::mul(d, h))), h]
}
fn centre(pts: &[V3; 3]) -> V3 {
    v::mul(pts.iter().fold([0.0; 3], |q, p| v::add(q, *p)), 1.0 / 3.0)
}

/// `onSurface(s, P, nt, tol, sag)`.
fn on_surface(s: &Surf, pts: &[V3; 3], nt: Option<V3>, tol: f64, sag: f64) -> bool {
    if let Some(w) = &s.within
        && radial(centre(pts), w.o, w.d)[0] > w.r + tol
    {
        return false;
    }
    if let Some(sp) = &s.span {
        let u = v::dot(sub(centre(pts), sp.a), sp.d);
        if u < -tol || u > sp.len + tol {
            return false;
        }
    }
    match s.ty.as_str() {
        "plane" => nt.is_none_or(|n| v::dot(n, s.n).abs() > 0.9) && pts.iter().all(|p| v::dot(sub(*p, s.o), s.n).abs() <= tol),
        "cylinder" => {
            if nt.is_some_and(|n| v::dot(n, s.axis.d).abs() > 0.5) {
                return false;
            }
            pts.iter().all(|p| {
                let d = radial(*p, s.axis.o, s.axis.d)[0] - s.r;
                d <= tol + 1e-6 * s.r && d >= -(tol + sag * s.r)
            })
        }
        "cone" => {
            if nt.is_some_and(|n| v::dot(n, s.axis.d).abs() > 0.999) {
                return false;
            }
            pts.iter().all(|p| {
                let [rho, h] = radial(*p, s.axis.o, s.axis.d);
                let d = rho - h * s.k;
                h >= -tol && d <= tol * (1.0 + s.k) && d >= -(tol * (1.0 + s.k) + sag * h * s.k)
            })
        }
        "sphere" => pts.iter().all(|p| {
            let d = hyp(sub(*p, s.c)) - s.r;
            d <= tol && d >= -(tol + sag * s.r)
        }),
        "torus" => pts.iter().all(|p| {
            let [rho, h] = radial(*p, s.axis.o, s.axis.d);
            let d = js::hypot2(rho - s.big_r, h) - s.r;
            d <= tol && d >= -(tol + sag * (s.big_r + s.r))
        }),
        "thread" => {
            if nt.is_some_and(|n| v::dot(n, s.axis.d).abs() > 0.99) {
                return false;
            }
            pts.iter().all(|p| {
                let [rho, h] = radial(*p, s.axis.o, s.axis.d);
                rho >= s.r0 - tol - sag * s.r0 && rho <= s.r1 + tol && h >= s.t0 - tol && h <= s.t1 + tol
            })
        }
        _ => false,
    }
}

/// A face record.
#[derive(Clone, Debug)]
pub struct FaceRec {
    pub name: Option<String>,
    pub ty: String,
    pub area: f64,
    pub at: V3,
    pub n: Option<V3>,
    pub o: Option<V3>,
    pub axis: Option<Axis>,
    pub r: Option<f64>,
    pub surf: Option<Value>,
}
impl FaceRec {
    pub fn to_json(&self) -> Value {
        let mut o = jv::obj(vec![
            ("name", self.name.as_deref().map_or(Value::Null, Into::into)),
            ("type", self.ty.as_str().into()),
            ("area", jv::num(self.area)),
            ("at", jv::pt3(self.at)),
        ]);
        if let Some(n) = self.n {
            jv::set(&mut o, "n", jv::pt3(n));
        }
        if let Some(p) = self.o {
            jv::set(&mut o, "o", jv::pt3(p));
        }
        if let Some(a) = self.axis {
            jv::set(&mut o, "axis", jv::obj(vec![("o", jv::pt3(a.o)), ("d", jv::pt3(a.d))]));
        }
        if let Some(r) = self.r {
            jv::set(&mut o, "r", jv::num(r));
        }
        if let Some(s) = &self.surf {
            jv::set(&mut o, "surf", s.clone());
        }
        o
    }
}

/// An edge record.
#[derive(Clone, Debug)]
pub struct EdgeRec {
    pub faces: [String; 2],
    pub at: V3,
    pub a: V3,
    pub b: V3,
    pub len: f64,
    pub ty: &'static str,
    pub circ: Option<(V3, V3, f64)>,
}
impl EdgeRec {
    pub fn to_json(&self) -> Value {
        let mut o = jv::obj(vec![
            ("faces", jv::strs(&self.faces)),
            ("key", self.faces.join("|").into()),
            ("at", jv::pt3(self.at)),
            ("a", jv::pt3(self.a)),
            ("b", jv::pt3(self.b)),
            ("len", jv::num(self.len)),
            ("type", self.ty.into()),
        ]);
        if let Some((c, n, r)) = self.circ {
            jv::set(&mut o, "circ", jv::obj(vec![("c", jv::pt3(c)), ("n", jv::pt3(n)), ("r", jv::num(r))]));
        }
        o
    }
}

/// What `namedFaces` gives.
#[derive(Clone, Debug, Default)]
pub struct Named {
    pub tri_face: Vec<i32>,
    pub faces: Vec<FaceRec>,
    pub edges: Vec<EdgeRec>,
    pub edge_segs: Vec<f64>,
    pub seg_edge: Vec<usize>,
}
impl Named {
    pub fn to_json(&self) -> Value {
        jv::obj(vec![
            ("triFace", Value::Array(self.tri_face.iter().map(|&x| Value::from(x)).collect())),
            ("faces", Value::Array(self.faces.iter().map(FaceRec::to_json).collect())),
            ("edges", Value::Array(self.edges.iter().map(EdgeRec::to_json).collect())),
            ("edgeSegs", jv::nums(&self.edge_segs)),
            ("segEdge", Value::Array(self.seg_edge.iter().map(|&x| jv::unum(x)).collect())),
        ])
    }
}

/// `Math.fround`: a Float32Array store.
#[allow(clippy::cast_possible_truncation)]
fn fround(x: f64) -> f64 {
    f64::from(x as f32)
}

fn ekey(u: usize, w: usize) -> (usize, usize) {
    if u < w { (u, w) } else { (w, u) }
}

/// An insertion-ordered list of triangles per mesh edge (the JS Map keyed u·2³² + v).
struct EdgeTris {
    keys: Vec<(usize, usize)>,
    tris: Vec<Vec<usize>>,
    at: HashMap<(usize, usize), usize>,
}
impl EdgeTris {
    fn build(m: &Mesh) -> EdgeTris {
        let mut e = EdgeTris { keys: Vec::new(), tris: Vec::new(), at: HashMap::new() };
        for t in 0..m.tri_count() {
            for k in 0..3usize {
                let key = ekey(m.corner(t, k), m.corner(t, k.saturating_add(1) % 3));
                match e.at.get(&key) {
                    Some(&i) => {
                        if let Some(l) = e.tris.get_mut(i) {
                            l.push(t);
                        }
                    }
                    None => {
                        e.at.insert(key, e.keys.len());
                        e.keys.push(key);
                        e.tris.push(vec![t]);
                    }
                }
            }
        }
        e
    }
    fn of(&self, u: usize, w: usize) -> &[usize] {
        self.at.get(&ekey(u, w)).and_then(|&i| self.tris.get(i)).map_or(&[], Vec::as_slice)
    }
}

fn find(parent: &mut [usize], mut x: usize) -> usize {
    let mut guard = 0usize;
    while let Some(&p) = parent.get(x) {
        if p == x || guard > parent.len() {
            break;
        }
        let pp = parent.get(p).copied().unwrap_or(p);
        if let Some(s) = parent.get_mut(x) {
            *s = pp;
        }
        x = pp;
        guard = guard.saturating_add(1);
    }
    x
}

/// `namedFaces(mesh, triTool, toolSurfs, { segments })`.
pub fn named_faces(m: &Mesh, tri_tool: &[i64], tool_surfs: &[Vec<Surf>], segments: f64) -> Named {
    let sag = 1.5 * (1.0 - js::cos(std::f64::consts::PI / js::max(8.0, segments)));
    let nt = m.tri_count();
    let pt = |i: usize| m.point(i);
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for c in m.positions.chunks(3) {
        for (k, &x) in c.iter().enumerate() {
            if let (Some(l), Some(h)) = (lo.get_mut(k), hi.get_mut(k)) {
                *l = js::min(*l, x);
                *h = js::max(*h, x);
            }
        }
    }
    let size = js::max(1e-9, js::hypot3(hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]));
    let tol = 2e-6 * size + 1e-7;
    // 1. each triangle → (tool, surface index)
    let mut surf_of: Vec<i64> = vec![-1; nt];
    let mut surf_key: Vec<(usize, usize)> = Vec::new();
    let mut key_index: HashMap<(usize, usize), usize> = HashMap::new();
    let mut tri_area = vec![0.0f64; nt];
    let mut tri_cen = vec![[0.0f64; 3]; nt];
    let mut tri_n = vec![[0.0f64; 3]; nt];
    let tool_at = |t: usize| tri_tool.get(t).copied().unwrap_or(-1);
    for t in 0..nt {
        let a = pt(m.corner(t, 0));
        let b = pt(m.corner(t, 1));
        let c = pt(m.corner(t, 2));
        let cr = v::cross(sub(b, a), sub(c, a));
        let ln = hyp(cr);
        if let Some(x) = tri_area.get_mut(t) {
            *x = ln / 2.0;
        }
        let cen = [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0, (a[2] + b[2] + c[2]) / 3.0];
        let nn = if ln > 0.0 { [cr[0] / ln, cr[1] / ln, cr[2] / ln] } else { [0.0; 3] };
        if let Some(x) = tri_cen.get_mut(t) {
            *x = cen;
        }
        if let Some(x) = tri_n.get_mut(t) {
            *x = nn;
        }
        let ti = tool_at(t);
        let Some(surfs) = usize::try_from(ti).ok().and_then(|i| tool_surfs.get(i)) else { continue };
        let longest = js::max(js::max(hyp(sub(b, a)), hyp(sub(c, b))), hyp(sub(a, c)));
        let n = if ln > 1e-6 * longest * longest { Some(nn) } else { None };
        let tri = [a, b, c];
        let mut hit: Option<usize> = None;
        for want in ["plane", "cylinder", "cone", "sphere", "torus", "thread"] {
            hit = surfs.iter().position(|s| s.ty == want && on_surface(s, &tri, n, tol, sag));
            if hit.is_some() {
                break;
            }
        }
        let Some(h) = hit else { continue };
        let tu = usize::try_from(ti).unwrap_or(0);
        let key = (tu, h);
        let g = *key_index.entry(key).or_insert_with(|| {
            surf_key.push(key);
            surf_key.len().saturating_sub(1)
        });
        if let Some(x) = surf_of.get_mut(t) {
            *x = i64::try_from(g).unwrap_or(-1);
        }
    }
    // 1b. what is left joins an edge-neighbour's surface of the same tool
    let et = EdgeTris::build(m);
    let sk_tool = |g: i64, surf_key: &[(usize, usize)]| usize::try_from(g).ok().and_then(|i| surf_key.get(i)).map(|x| x.0);
    for _pass in 0..4 {
        let mut changed = 0usize;
        for t in 0..nt {
            if surf_of.get(t).copied().unwrap_or(-1) >= 0 || tool_at(t) < 0 {
                continue;
            }
            let mut found: Option<usize> = None;
            'k: for k in 0..3usize {
                let u = m.corner(t, k);
                let w = m.corner(t, k.saturating_add(1) % 3);
                for &q in et.of(u, w) {
                    if q == t {
                        continue;
                    }
                    let sq = surf_of.get(q).copied().unwrap_or(-1);
                    if sq >= 0 && sk_tool(sq, &surf_key).and_then(|x| i64::try_from(x).ok()) == Some(tool_at(t)) {
                        found = Some(q);
                        break 'k;
                    }
                }
            }
            if let Some(q) = found {
                let sq = surf_of.get(q).copied().unwrap_or(-1);
                if let Some(x) = surf_of.get_mut(t) {
                    *x = sq;
                }
                changed = changed.saturating_add(1);
            }
        }
        if changed == 0 {
            break;
        }
    }
    // 2. connected triangles on one surface → one face
    let mut parent: Vec<usize> = (0..nt).collect();
    for tris in &et.tris {
        if let [p, q] = tris.as_slice() {
            let sp = surf_of.get(*p).copied().unwrap_or(-1);
            if sp >= 0 && sp == surf_of.get(*q).copied().unwrap_or(-2) {
                let rp = find(&mut parent, *p);
                let rq = find(&mut parent, *q);
                if rp != rq
                    && let Some(x) = parent.get_mut(rp)
                {
                    *x = rq;
                }
            }
        }
    }
    let mut tri_face: Vec<i32> = vec![-1; nt];
    struct Acc {
        s: (usize, usize),
        area: f64,
        cx: f64,
        cy: f64,
        cz: f64,
        nx: f64,
        ny: f64,
        nz: f64,
    }
    let mut faces: Vec<Acc> = Vec::new();
    let mut face_of_root: HashMap<usize, usize> = HashMap::new();
    for t in 0..nt {
        let so = surf_of.get(t).copied().unwrap_or(-1);
        let Ok(su) = usize::try_from(so) else { continue };
        let r = find(&mut parent, t);
        let fi = match face_of_root.get(&r) {
            Some(&f) => f,
            None => {
                let f = faces.len();
                face_of_root.insert(r, f);
                let s = surf_key.get(su).copied().unwrap_or((0, 0));
                faces.push(Acc { s, area: 0.0, cx: 0.0, cy: 0.0, cz: 0.0, nx: 0.0, ny: 0.0, nz: 0.0 });
                f
            }
        };
        if let Some(x) = tri_face.get_mut(t) {
            *x = i32::try_from(fi).unwrap_or(-1);
        }
        let aa = tri_area.get(t).copied().unwrap_or(0.0);
        let cen = tri_cen.get(t).copied().unwrap_or([0.0; 3]);
        let nn = tri_n.get(t).copied().unwrap_or([0.0; 3]);
        if let Some(f) = faces.get_mut(fi) {
            f.area += aa;
            f.cx += cen[0] * aa;
            f.cy += cen[1] * aa;
            f.cz += cen[2] * aa;
            f.nx += nn[0] * aa;
            f.ny += nn[1] * aa;
            f.nz += nn[2] * aa;
        }
    }
    let face_recs: Vec<FaceRec> = faces
        .iter()
        .map(|f| {
            let s = tool_surfs.get(f.s.0).and_then(|l| l.get(f.s.1));
            let at = if f.area > 0.0 { [f.cx / f.area, f.cy / f.area, f.cz / f.area] } else { [0.0; 3] };
            let Some(s) = s else {
                return FaceRec { name: None, ty: String::new(), area: f.area, at, n: None, o: None, axis: None, r: None, surf: None };
            };
            let mut rec =
                FaceRec { name: s.name.clone(), ty: s.ty.clone(), area: f.area, at, n: None, o: None, axis: None, r: None, surf: None };
            let ax = Some(s.axis);
            let axo = || jv::pt3(s.axis.o);
            let axd = || jv::pt3(s.axis.d);
            match s.ty.as_str() {
                "plane" => {
                    let sign = if v::dot([f.nx, f.ny, f.nz], s.n) < 0.0 { -1.0 } else { 1.0 };
                    rec.n = Some(v::mul(s.n, sign));
                    rec.o = Some(s.o);
                }
                "cylinder" => {
                    rec.axis = ax;
                    rec.r = Some(s.r);
                }
                "cone" => {
                    rec.axis = ax;
                    rec.surf =
                        Some(jv::obj(vec![("type", "cone".into()), ("o", axo()), ("d", axd()), ("r0", 0.into()), ("k", jv::num(s.k))]));
                }
                "sphere" => rec.surf = Some(jv::obj(vec![("type", "sphere".into()), ("c", jv::pt3(s.c)), ("r", jv::num(s.r))])),
                "thread" => {
                    rec.axis = ax;
                    rec.r = Some(s.r1);
                }
                "torus" => {
                    rec.axis = ax;
                    rec.surf = Some(jv::obj(vec![
                        ("type", "torus".into()),
                        ("c", axo()),
                        ("d", axd()),
                        ("R", jv::num(s.big_r)),
                        ("r", jv::num(s.r)),
                    ]));
                }
                _ => {}
            }
            rec
        })
        .collect();
    // 3. edges: mesh edges between two different faces, chained per face pair
    let mut pair_keys: Vec<(usize, usize)> = Vec::new();
    let mut pair_list: Vec<Vec<(usize, usize)>> = Vec::new();
    let mut pair_at: HashMap<(usize, usize), usize> = HashMap::new();
    for (key, tris) in et.keys.iter().zip(et.tris.iter()) {
        let [t0, t1] = tris.as_slice() else { continue };
        let fa = tri_face.get(*t0).copied().unwrap_or(-1);
        let fb = tri_face.get(*t1).copied().unwrap_or(-1);
        if fa < 0 || fb < 0 || fa == fb {
            continue;
        }
        let (fa, fb) = (usize::try_from(fa).unwrap_or(0), usize::try_from(fb).unwrap_or(0));
        let pk = if fa < fb { (fa, fb) } else { (fb, fa) };
        let i = *pair_at.entry(pk).or_insert_with(|| {
            pair_keys.push(pk);
            pair_list.push(Vec::new());
            pair_list.len().saturating_sub(1)
        });
        if let Some(l) = pair_list.get_mut(i) {
            l.push(*key);
        }
    }
    let mut edges: Vec<EdgeRec> = Vec::new();
    let mut segs: Vec<f64> = Vec::new();
    let mut seg_edge: Vec<usize> = Vec::new();
    for (pk, list) in pair_keys.iter().zip(pair_list.iter()) {
        let (fa, fb) = *pk;
        let mut adj: HashMap<usize, Vec<(usize, usize)>> = HashMap::new();
        for (i, &(u, w)) in list.iter().enumerate() {
            adj.entry(u).or_default().push((w, i));
            adj.entry(w).or_default().push((u, i));
        }
        let mut used = vec![false; list.len()];
        for s0 in 0..list.len() {
            if used.get(s0).copied().unwrap_or(true) {
                continue;
            }
            let first = list.get(s0).map_or(0, |x| x.0);
            let mut stack = vec![first];
            let mut seen_order = vec![first];
            let mut seen: HashSet<usize> = HashSet::from([first]);
            while let Some(x) = stack.pop() {
                for &(y, _) in adj.get(&x).map_or(&[][..], Vec::as_slice) {
                    if seen.insert(y) {
                        seen_order.push(y);
                        stack.push(y);
                    }
                }
            }
            let mut start = first;
            for &x in &seen_order {
                if adj.get(&x).map_or(0, Vec::len) == 1 {
                    start = x;
                    break;
                }
            }
            let mut chain = vec![start];
            let mut cur = start;
            loop {
                let step = adj.get(&cur).and_then(|l| l.iter().find(|(_, i)| !used.get(*i).copied().unwrap_or(true)).copied());
                let Some((y, i)) = step else { break };
                if let Some(u) = used.get_mut(i) {
                    *u = true;
                }
                cur = y;
                chain.push(cur);
            }
            if chain.len() < 2 {
                continue;
            }
            let pts: Vec<V3> = chain.iter().map(|&i| pt(i)).collect();
            let mut len = 0.0;
            let mut cum = vec![0.0];
            for w in pts.windows(2) {
                if let [p0, p1] = w {
                    len += hyp(sub(*p1, *p0));
                    cum.push(len);
                }
            }
            let mut mi = 1usize;
            while mi < cum.len().saturating_sub(1) && cum.get(mi).copied().unwrap_or(f64::NAN) < len / 2.0 {
                mi = mi.saturating_add(1);
            }
            let cm = cum.get(mi).copied().unwrap_or(f64::NAN);
            let cm1 = cum.get(mi.saturating_sub(1)).copied().unwrap_or(f64::NAN);
            let f0 = (cm - len / 2.0) / js::max(1e-30, cm - cm1);
            let pm1 = pts.get(mi.saturating_sub(1)).copied().unwrap_or([f64::NAN; 3]);
            let pmi = pts.get(mi).copied().unwrap_or([f64::NAN; 3]);
            let at = v::add(v::mul(pm1, f0), v::mul(pmi, 1.0 - f0));
            let fra = face_recs.get(fa);
            let frb = face_recs.get(fb);
            let mut names = [
                fra.and_then(|f| f.name.clone()).unwrap_or_else(|| "?".into()),
                frb.and_then(|f| f.name.clone()).unwrap_or_else(|| "?".into()),
            ];
            if js::cmp16(&names[0], &names[1]) == std::cmp::Ordering::Greater {
                names.swap(0, 1);
            }
            let p_first = pts.first().copied().unwrap_or([f64::NAN; 3]);
            let p_last = pts.last().copied().unwrap_or([f64::NAN; 3]);
            let mut rec = EdgeRec { faces: names, at, a: p_first, b: p_last, len, ty: "curve", circ: None };
            let dir = v::sub(p_last, p_first);
            let dl = hyp(dir);
            if dl > tol && pts.iter().all(|p| hyp(v::cross(sub(*p, p_first), v::mul(dir, 1.0 / dl))) <= tol * 10.0) {
                rec.ty = "line";
            } else {
                let round = |f: Option<&FaceRec>| f.is_some_and(|x| matches!(x.ty.as_str(), "cylinder" | "cone" | "torus"));
                let provider = if round(fra) {
                    fra
                } else if round(frb) {
                    frb
                } else {
                    None
                };
                let ax = provider.and_then(|p| p.axis);
                let rh: Option<Vec<[f64; 2]>> = ax.map(|a| pts.iter().map(|p| radial(*p, a.o, a.d)).collect());
                let rmax = rh.as_ref().map_or(0.0, |l| l.iter().fold(f64::NEG_INFINITY, |m, x| js::max(m, x[0])));
                let circle_ok = match (&rh, ax) {
                    (Some(l), Some(_)) => {
                        let h0 = l.first().map_or(f64::NAN, |x| x[1]);
                        l.iter().all(|&[r, h]| r <= rmax + tol && r >= rmax - tol * 10.0 - sag * rmax && (h - h0).abs() <= tol * 10.0)
                            && rmax > tol
                    }
                    _ => false,
                };
                if circle_ok && let (Some(a), Some(l)) = (ax, &rh) {
                    let h0 = l.first().map_or(f64::NAN, |x| x[1]);
                    let cyl = provider.filter(|p| p.ty == "cylinder");
                    let r = match cyl.and_then(|c| c.r) {
                        Some(cr) if (cr - rmax).abs() <= tol * 10.0 + sag * cr => cr,
                        _ => rmax,
                    };
                    rec.ty = "circle";
                    rec.circ = Some((v::add(a.o, v::mul(a.d, h0)), a.d, r));
                } else if chain.first() == chain.last() && pts.len() > 4 {
                    let ring = pts.get(..pts.len().saturating_sub(1)).unwrap_or(&[]);
                    let rl = js::f(ring.len());
                    let c = v::mul(ring.iter().fold([0.0; 3], |q, p| v::add(q, *p)), 1.0 / rl);
                    let r = ring.iter().fold(0.0, |q, p| q + hyp(sub(*p, c))) / rl;
                    let k = ring.len() / 4;
                    let r0 = ring.first().copied().unwrap_or([f64::NAN; 3]);
                    let rk = ring.get(k).copied().unwrap_or([f64::NAN; 3]);
                    let n = v::norm(v::cross(sub(r0, c), sub(rk, c)));
                    if r > tol
                        && ring.iter().all(|p| (hyp(sub(*p, c)) - r).abs() <= tol * 10.0 && v::dot(sub(*p, c), n).abs() <= tol * 10.0)
                    {
                        rec.ty = "circle";
                        rec.circ = Some((c, n, r));
                    }
                }
            }
            if let Some((c, n, r)) = rec.circ.filter(|_| rec.ty == "circle") {
                let on_c = |p: V3| {
                    let w0 = sub(p, c);
                    let w = sub(w0, v::mul(n, v::dot(w0, n)));
                    let l = hyp(w);
                    if l > 0.0 { v::add(c, v::mul(w, r / l)) } else { p }
                };
                let closed = chain.first() == chain.last();
                rec.a = on_c(rec.a);
                rec.b = if closed { rec.a } else { on_c(rec.b) };
                rec.at = if closed { v::sub(v::mul(c, 2.0), rec.a) } else { on_c(rec.at) };
            }
            let ei = edges.len();
            edges.push(rec);
            for w in pts.windows(2) {
                if let [p0, p1] = w {
                    for x in p0.iter().chain(p1.iter()) {
                        segs.push(fround(*x));
                    }
                    seg_edge.push(ei);
                }
            }
        }
    }
    Named { tri_face, faces: face_recs, edges, edge_segs: segs, seg_edge }
}
