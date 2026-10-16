//! What can be snapped to and picked, in world space (infer.js `sceneOf`, `Placed`, `Scene`). Each definition's faces
//! and edges are kept in its own space with trees over its triangles and edges ([`Local`]); each placement is a
//! definition and its matrix ([`Placed`]), whose world faces and edges are made as they are asked for. Hidden things
//! and what the active section cut takes away are left out, as in the JS.
//!
//! The JS keeps a definition's [`Local`] in a WeakMap keyed by the definition object, valid while
//! `model.unsafeVersion` has not moved past it. A Rust model has no object identity, so [`LocalCache`] keys on the
//! definition id plus a fingerprint of the definition's geometry: the same answer (a [`Local`] is a pure function of
//! the definition's vertices, edges and faces), found the same way.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use cc_geom::bvh::{BoxTree, boxes_of};
use cc_m3d::def::{Def, face_normal, face_tris};
use cc_m3d::model::{Guide, Model};
use cc_m3d::vec::{IDENT, M4, P3, Plane, add, apply, apply_dir, dot, invert, mul, norm, sub};

use crate::camera::{Camera, Ray};
use crate::js::{hypot3, jmax, jmin};

/// A face in a definition's own space.
#[derive(Clone, Debug)]
pub struct LFace {
    pub id: f64,
    pub tris: Vec<[P3; 3]>,
    pub n: P3,
    /// the first point of its outer loop (for its plane)
    pub p: P3,
}
/// An edge in a definition's own space.
#[derive(Clone, Debug)]
pub struct LEdge {
    pub id: f64,
    pub a: P3,
    pub b: P3,
    pub soft: bool,
}

/// A definition's faces and edges in its own space, with trees over its triangles and edges.
#[derive(Debug)]
pub struct Local {
    pub faces: Vec<LFace>,
    pub edges: Vec<LEdge>,
    /// triangles packed 9 numbers each
    pub tri: Vec<f64>,
    /// the face (index into `faces`) of each triangle
    pub tri_face: Vec<u32>,
    pub ftree: BoxTree,
    pub etree: BoxTree,
    /// the world boxes of whole placements (collide.js: per matrix)
    pub boxes: RefCell<HashMap<String, Option<crate::camera::Box3>>>,
}

/// `localOf(def)`.
pub fn local_of(def: &Def) -> Local {
    let mut faces = Vec::new();
    let mut edges = Vec::new();
    let mut nt = 0usize;
    for (id, f) in def.faces.iter() {
        if f.hidden {
            continue;
        }
        let tris: Vec<[P3; 3]> = face_tris(def, f).into_iter().map(|t| [def.v(t[0]), def.v(t[1]), def.v(t[2])]).collect();
        nt = nt.saturating_add(tris.len());
        let p = f.loops.first().and_then(|l| l.first()).map_or([f64::NAN; 3], |&v| def.v(v));
        faces.push(LFace { id, tris, n: face_normal(def, f), p });
    }
    for (id, e) in def.edges.iter() {
        if !e.hidden {
            edges.push(LEdge { id, a: def.v(e.a), b: def.v(e.b), soft: e.soft });
        }
    }
    let mut tri = Vec::with_capacity(nt.saturating_mul(9));
    let mut tri_face = Vec::with_capacity(nt);
    for (fi, f) in faces.iter().enumerate() {
        for t in &f.tris {
            for p in t {
                tri.extend_from_slice(p);
            }
            tri_face.push(u32::try_from(fi).unwrap_or(u32::MAX));
        }
    }
    let mut tb = Vec::with_capacity(nt.saturating_mul(6));
    for t in tri.as_chunks::<9>().0 {
        let at = |k: usize| t.get(k).copied().unwrap_or(f64::NAN);
        // Math.min(x, y, z) / Math.max(x, y, z) per axis
        let lo: [f64; 3] = std::array::from_fn(|a| jmin(jmin(at(a), at(3 + a)), at(6 + a)));
        let hi: [f64; 3] = std::array::from_fn(|a| jmax(jmax(at(a), at(3 + a)), at(6 + a)));
        tb.extend_from_slice(&lo);
        tb.extend_from_slice(&hi);
    }
    let ftree = BoxTree::new(tb);
    let lists: Vec<[P3; 2]> = edges.iter().map(|e| [e.a, e.b]).collect();
    let etree = BoxTree::new(boxes_of(lists.iter().map(|l| &l[..]), 0.0));
    Local { faces, edges, tri, tri_face, ftree, etree, boxes: RefCell::new(HashMap::new()) }
}

/// A fingerprint of what a [`Local`] is made from.
fn fingerprint(def: &Def) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for (id, p) in def.verts.iter() {
        id.to_bits().hash(&mut h);
        for x in p {
            x.to_bits().hash(&mut h);
        }
    }
    0xffu8.hash(&mut h);
    for (id, e) in def.edges.iter() {
        (id.to_bits(), e.a.to_bits(), e.b.to_bits(), e.soft, e.hidden).hash(&mut h);
    }
    0xfeu8.hash(&mut h);
    for (id, f) in def.faces.iter() {
        id.to_bits().hash(&mut h);
        f.hidden.hash(&mut h);
        for l in &f.loops {
            l.len().hash(&mut h);
            for v in l {
                v.to_bits().hash(&mut h);
            }
        }
    }
    h.finish()
}

/// Definitions' [`Local`]s kept between scenes (see the module header).
#[derive(Debug, Default)]
pub struct LocalCache {
    map: HashMap<String, (u64, Rc<Local>)>,
}
impl LocalCache {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn get(&mut self, def: &Def) -> Rc<Local> {
        let fp = fingerprint(def);
        if let Some((f, l)) = self.map.get(&def.id)
            && *f == fp
        {
            return l.clone();
        }
        let l = Rc::new(local_of(def));
        if self.map.len() > 4096 {
            self.map.clear();
        }
        self.map.insert(def.id.clone(), (fp, l.clone()));
        l
    }
    pub fn clear(&mut self) {
        self.map.clear();
    }
}

/// The active section cut: what lies on its far side cannot be snapped to or picked.
#[derive(Clone, Copy, Debug)]
pub struct Cut {
    pub p: P3,
    pub n: P3,
}
impl Cut {
    /// `keep(p)`.
    #[inline]
    pub fn keep(&self, q: P3) -> bool {
        dot(sub(q, self.p), self.n) <= 1e-9
    }
}
/// `sc.keep && !sc.keep(p)`: true when p is cut away.
#[inline]
pub fn cut_away(keep: Option<&Cut>, p: P3) -> bool {
    keep.is_some_and(|k| !k.keep(p))
}

/// A world face: {path, def, id, tris, n, plane}.
#[derive(Clone, Debug)]
pub struct SFace {
    pub path: Rc<[f64]>,
    pub def: Rc<str>,
    pub id: f64,
    pub tris: Vec<[P3; 3]>,
    pub n: P3,
    pub plane: Plane,
}
/// A world edge: {path, def, id, a, b, soft}.
#[derive(Clone, Debug)]
pub struct SEdge {
    pub path: Rc<[f64]>,
    pub def: Rc<str>,
    pub id: f64,
    pub a: P3,
    pub b: P3,
    pub soft: bool,
}

/// What `skip(o, kind)` / `moving(o, kind)` look at: a path and an id.
#[derive(Clone, Copy, Debug)]
pub struct ObjRef<'a> {
    pub path: &'a [f64],
    pub id: f64,
}

/// One placement of a definition.
#[derive(Debug)]
pub struct Placed {
    pub def: Rc<str>,
    pub m: M4,
    pub path: Rc<[f64]>,
    pub l: Rc<Local>,
    pub keep: Option<Cut>,
    /// how much the matrix can stretch a length
    pub s: f64,
    inv: OnceCell<M4>,
    /// world faces / edges made so far, by index (None = not made yet; Some(None) = cut away)
    fo: RefCell<Vec<Option<Option<Rc<SFace>>>>>,
    eo: RefCell<Vec<Option<Option<Rc<SEdge>>>>>,
}

impl Placed {
    pub fn new(def: &str, m: M4, path: &[f64], l: Rc<Local>, keep: Option<Cut>) -> Placed {
        let s = jmax(
            jmax(m[0] * m[0] + m[1] * m[1] + m[2] * m[2], m[4] * m[4] + m[5] * m[5] + m[6] * m[6]),
            m[8] * m[8] + m[9] * m[9] + m[10] * m[10],
        )
        .sqrt();
        Placed {
            def: def.into(),
            m,
            path: path.into(),
            l,
            keep,
            s,
            inv: OnceCell::new(),
            fo: RefCell::new(Vec::new()),
            eo: RefCell::new(Vec::new()),
        }
    }
    pub fn inv(&self) -> &M4 {
        self.inv.get_or_init(|| invert(&self.m))
    }
    /// World face i (None when the section cut hides it).
    pub fn face(&self, i: usize) -> Option<Rc<SFace>> {
        if let Some(Some(f)) = self.fo.borrow().get(i) {
            return f.clone();
        }
        let lf = self.l.faces.get(i)?;
        let m = &self.m;
        let tris: Vec<[P3; 3]> = lf.tris.iter().map(|t| [apply(m, t[0]), apply(m, t[1]), apply(m, t[2])]).collect();
        let n = norm(apply_dir(m, lf.n));
        let f = match &self.keep {
            Some(k) if !tris.iter().any(|t| t.iter().any(|p| k.keep(*p))) => None,
            _ => Some(Rc::new(SFace {
                path: self.path.clone(),
                def: self.def.clone(),
                id: lf.id,
                tris,
                n,
                plane: Plane { n, d: dot(n, apply(m, lf.p)) },
            })),
        };
        let mut fo = self.fo.borrow_mut();
        if fo.len() <= i {
            fo.resize(self.l.faces.len().max(i.saturating_add(1)), None);
        }
        if let Some(slot) = fo.get_mut(i) {
            *slot = Some(f.clone());
        }
        f
    }
    /// World edge i (None when the section cut hides it).
    pub fn edge(&self, i: usize) -> Option<Rc<SEdge>> {
        if let Some(Some(e)) = self.eo.borrow().get(i) {
            return e.clone();
        }
        let le = self.l.edges.get(i)?;
        let a = apply(&self.m, le.a);
        let b = apply(&self.m, le.b);
        let e = match &self.keep {
            Some(k) if !k.keep(a) && !k.keep(b) => None,
            _ => Some(Rc::new(SEdge { path: self.path.clone(), def: self.def.clone(), id: le.id, a, b, soft: le.soft })),
        };
        let mut eo = self.eo.borrow_mut();
        if eo.len() <= i {
            eo.resize(self.l.edges.len().max(i.saturating_add(1)), None);
        }
        if let Some(slot) = eo.get_mut(i) {
            *slot = Some(e.clone());
        }
        e
    }
}

/// A face under a ray: {t, f, p}.
#[derive(Clone, Debug)]
pub struct Hit {
    pub t: f64,
    pub f: Rc<SFace>,
    pub p: P3,
}

/// The scene.
#[derive(Debug)]
pub struct Scene {
    pub insts: Vec<Placed>,
    pub guides: Vec<(f64, Guide)>,
    pub version: f64,
    pub keep: Option<Cut>,
    faces: OnceCell<Vec<Rc<SFace>>>,
    edges: OnceCell<Vec<Rc<SEdge>>>,
}

impl Scene {
    /// `sceneOf(model)` (each definition's [`Local`] from `cache`).
    pub fn of(model: &Model, cache: &mut LocalCache) -> Scene {
        let keep = model.active_cut().map(|(_, p, n)| Cut { p, n });
        let mut insts = Vec::new();
        let mut per: HashMap<String, Rc<Local>> = HashMap::new();
        model.walk_shown("root", &IDENT, &mut |def, m, path, _| {
            let l = match per.get(&def.id) {
                Some(l) => l.clone(),
                None => {
                    let l = cache.get(def);
                    per.insert(def.id.clone(), l.clone());
                    l
                }
            };
            if !l.faces.is_empty() || !l.edges.is_empty() {
                insts.push(Placed::new(&def.id, *m, path, l, keep));
            }
            true
        });
        Scene {
            insts,
            guides: model.guides.iter().map(|(id, g)| (id, g.clone())).collect(),
            version: model.version(),
            keep,
            faces: OnceCell::new(),
            edges: OnceCell::new(),
        }
    }
    /// Every face (made when first asked).
    pub fn faces(&self) -> &[Rc<SFace>] {
        self.faces.get_or_init(|| {
            let mut out = Vec::new();
            for p in &self.insts {
                for i in 0..p.l.faces.len() {
                    if let Some(f) = p.face(i) {
                        out.push(f);
                    }
                }
            }
            out
        })
    }
    /// Every edge (made when first asked).
    pub fn edges(&self) -> &[Rc<SEdge>] {
        self.edges.get_or_init(|| {
            let mut out = Vec::new();
            for p in &self.insts {
                for i in 0..p.l.edges.len() {
                    if let Some(e) = p.edge(i) {
                        out.push(e);
                    }
                }
            }
            out
        })
    }
    /// One face: the first placement with this path decides.
    pub fn face(&self, path: &[f64], id: f64) -> Option<Rc<SFace>> {
        for p in &self.insts {
            if p.path.len() != path.len() || !p.path.iter().zip(path).all(|(a, b)| a == b) {
                continue;
            }
            let i = p.l.faces.iter().position(|f| f.id == id)?;
            return p.face(i);
        }
        None
    }
    /// The nearest face a ray meets (filter(f) → false skips a face).
    pub fn hit(&self, ray: &Ray, filter: Option<&dyn Fn(&SFace) -> bool>) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        let lim = Cell::new(f64::INFINITY);
        let (o, d) = (ray.o, ray.d);
        for p in &self.insts {
            let inv = p.inv();
            let o_l = apply(inv, o);
            let d_l = apply_dir(inv, d);
            let tri = &p.l.tri;
            p.l.ftree.ray(o_l, d_l, &lim, |i| {
                let Some(t) = ray_tri_a(o_l, d_l, tri, i.saturating_mul(9)) else { return false };
                if t >= lim.get() {
                    return false;
                }
                let fi = p.l.tri_face.get(i).copied().map_or(usize::MAX, |x| usize::try_from(x).unwrap_or(usize::MAX));
                let Some(f) = p.face(fi) else { return false };
                if filter.is_some_and(|fl| !fl(&f)) {
                    return false;
                }
                let q = add(o, mul(d, t));
                if cut_away(self.keep.as_ref(), q) {
                    return false;
                }
                best = Some(Hit { t, f, p: q });
                lim.set(t);
                false
            });
        }
        best
    }
    /// Edges that may come within px pixels of the ray on screen (a superset; the caller measures).
    pub fn near(&self, cam: &Camera, ray: &Ray, px: f64) -> Vec<Rc<SEdge>> {
        let mut out = Vec::new();
        let (o, d) = (ray.o, ray.d);
        let k = if cam.persp { (2.0 * libm::tan((cam.fov * std::f64::consts::PI) / 360.0)) / cam.h * px } else { 0.0 };
        let flat = if cam.persp { 0.0 } else { cam.pixel_size0() * px };
        for p in &self.insts {
            if p.l.edges.is_empty() {
                continue;
            }
            let (m, s) = (&p.m, p.s);
            let test = |lo: &[f64; 3], hi: &[f64; 3]| {
                let cl = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
                let r = hypot3(hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]) / 2.0 * s;
                let w = apply(m, cl);
                let c = [w[0] - o[0], w[1] - o[1], w[2] - o[2]];
                let along = c[0] * d[0] + c[1] * d[1] + c[2] * d[2];
                if cam.persp && along < -r {
                    return false;
                }
                let perp = jmax(0.0, c[0] * c[0] + c[1] * c[1] + c[2] * c[2] - along * along).sqrt();
                let reach = if cam.persp { k * (hypot3(c[0], c[1], c[2]) + r) } else { flat };
                perp <= r + reach * 1.01 + 1e-9
            };
            p.l.etree.walk(test, |i| {
                if let Some(e) = p.edge(i) {
                    out.push(e);
                }
                false
            });
        }
        out
    }
}

/// rayTri on a triangle packed at q (9 numbers).
pub fn ray_tri_a(o: P3, d: P3, t: &[f64], q: usize) -> Option<f64> {
    let g = |k: usize| t.get(q.saturating_add(k)).copied().unwrap_or(f64::NAN);
    let (ax, ay, az) = (g(0), g(1), g(2));
    let (e1x, e1y, e1z, e2x, e2y, e2z) = (g(3) - ax, g(4) - ay, g(5) - az, g(6) - ax, g(7) - ay, g(8) - az);
    let px = d[1] * e2z - d[2] * e2y;
    let py = d[2] * e2x - d[0] * e2z;
    let pz = d[0] * e2y - d[1] * e2x;
    let det = e1x * px + e1y * py + e1z * pz;
    if det.abs() < 1e-14 {
        return None;
    }
    let inv = 1.0 / det;
    let (sx, sy, sz) = (o[0] - ax, o[1] - ay, o[2] - az);
    let u = (sx * px + sy * py + sz * pz) * inv;
    if u < -1e-9 || u > 1.0 + 1e-9 {
        return None;
    }
    let qx = sy * e1z - sz * e1y;
    let qy = sz * e1x - sx * e1z;
    let qz = sx * e1y - sy * e1x;
    let v = (d[0] * qx + d[1] * qy + d[2] * qz) * inv;
    if v < -1e-9 || u + v > 1.0 + 1e-9 {
        return None;
    }
    let tt = (e2x * qx + e2y * qy + e2z * qz) * inv;
    if tt > 0.0 { Some(tt) } else { None }
}
