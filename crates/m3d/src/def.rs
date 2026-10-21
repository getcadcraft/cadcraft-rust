//! Definitions (`Def`): a model is a set of them. The root definition is the model itself; a group is a definition
//! with one instance, a component a definition with any number of instances. Each holds
//!
//! - `verts`  id → `[x, y, z]` (metres, Z up)
//! - `edges`  id → {a, b, soft?, curve?, hidden?}; `curve` ('arc:12', 'circle:40', 'polygon:7', 'curve:3') tags the
//!   segments of one arc, circle, polygon or smooth curve
//! - `faces`  id → {loops, color?, hidden?}: loops[0] the outer boundary (counter-clockwise seen from the front),
//!   loops[1..] holes (clockwise); every loop segment is also an edge
//! - `insts`  id → {def, m, name?, color?, hidden?, tag?, rule?}: placements of other definitions (m column-major)
//! - `splines` curve id → {pts, closed, segs}: the points a smooth curve was drawn through (it can be edited)
//!
//! Ids are JS numbers handed out by `next++` (so f64 here, exactly as the JS counts). The edge-key cache `_ek` is
//! kept the JS way: built on first use, then kept up by eAdd / eRemove only.

use crate::dense::VAdj;
use crate::earcut::earcut;
use crate::omap::{FxMap, IdMap, OMap, kb};
use crate::vec::{EPS, Frame, P3, Plane, cross, dot, frame, len, near, norm, sub, to2};

pub type P2 = [f64; 2];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Edge {
    pub a: f64,
    pub b: f64,
    pub soft: bool,
    pub curve: Option<String>,
    pub hidden: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub loops: Vec<Vec<f64>>,
    pub color: Option<String>,
    pub hidden: bool,
}

/// How a part follows when its component stretches (0.4 M26): kept as the file gives it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rule {
    pub x: Option<String>,
    pub y: Option<String>,
    pub z: Option<String>,
    pub sx: Option<f64>,
    pub sy: Option<f64>,
    pub sz: Option<f64>,
    /// The keys in the order the page set them (a JS object keeps its keys in insertion order, and the file writes
    /// them so); None: x y z sx sy sz, as a file is read.
    pub order: Option<Vec<&'static str>>,
}

impl Rule {
    const KEYS: [&'static str; 6] = ["x", "y", "z", "sx", "sy", "sz"];
    fn has(&self, k: &str) -> bool {
        match k {
            "x" => self.x.is_some(),
            "y" => self.y.is_some(),
            "z" => self.z.is_some(),
            "sx" => self.sx.is_some(),
            "sy" => self.sy.is_some(),
            "sz" => self.sz.is_some(),
            _ => false,
        }
    }
    /// The keys present, in the order they are written.
    pub fn keys(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> =
            self.order.as_ref().map(|o| o.iter().copied().filter(|k| self.has(k)).collect()).unwrap_or_default();
        for k in Self::KEYS {
            if self.has(k) && !out.contains(&k) {
                out.push(k);
            }
        }
        out
    }
    /// `rule[k] = v` (a new key goes last) or `delete rule[k]` (both None): a word for x / y / z, a number for
    /// sx / sy / sz.
    pub fn set(&mut self, k: &str, word: Option<String>, num: Option<f64>) {
        let mut order = self.keys();
        let Some(&key) = Self::KEYS.iter().find(|q| **q == k) else { return };
        let present = word.is_some() || num.is_some();
        match key {
            "x" => self.x = word,
            "y" => self.y = word,
            "z" => self.z = word,
            "sx" => self.sx = num,
            "sy" => self.sy = num,
            _ => self.sz = num,
        }
        if present {
            if !order.contains(&key) {
                order.push(key);
            }
        } else {
            order.retain(|q| *q != key);
        }
        self.order = Some(order);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Inst {
    pub def: String,
    pub m: crate::vec::M4,
    pub name: Option<String>,
    pub color: Option<String>,
    pub hidden: bool,
    pub tag: Option<String>,
    pub rule: Option<Rule>,
}
impl Inst {
    pub fn new(def: String, m: crate::vec::M4) -> Self {
        Self { def, m, name: None, color: None, hidden: false, tag: None, rule: None }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spline {
    pub pts: Vec<P3>,
    pub closed: bool,
    pub segs: f64,
}

/// A stretched copy's original and size (0.4 M26).
#[derive(Clone, Debug, PartialEq)]
pub struct Dyn {
    pub src: String,
    pub size: Vec<f64>,
}

pub type EdgeKey = (u64, u64);

/// `key(a, b)`: the same for (a, b) and (b, a).
#[inline]
pub fn key(a: f64, b: f64) -> EdgeKey {
    if a < b { (kb(a), kb(b)) } else { (kb(b), kb(a)) }
}

#[derive(Clone, Debug)]
pub struct Def {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub verts: IdMap<P3>,
    pub edges: IdMap<Edge>,
    pub faces: IdMap<Face>,
    pub insts: IdMap<Inst>,
    pub splines: OMap<String, Spline>,
    pub next: f64,
    pub ek: Option<FxMap<EdgeKey, f64>>,
    pub starter: Option<String>,
    pub dyn_: Option<Dyn>,
    /// `defStrs`: this definition's JSON (a snapshot entry) and the model version it was made at (Model.defString).
    pub cache: Option<(f64, crate::part::PartRef)>,
}

impl Def {
    pub fn new(id: &str, kind: &str, name: &str) -> Self {
        Self {
            id: id.to_string(),
            kind: kind.to_string(),
            name: name.to_string(),
            verts: IdMap::new(),
            edges: IdMap::new(),
            faces: IdMap::new(),
            insts: IdMap::new(),
            splines: OMap::new(),
            next: 1.0,
            ek: None,
            starter: None,
            dyn_: None,
            cache: None,
        }
    }
    /// A vertex's position (NaNs for a missing id: a model the kernel keeps never refers to one).
    #[inline]
    pub fn v(&self, id: f64) -> P3 {
        self.verts.get(id).copied().unwrap_or([f64::NAN; 3])
    }
}

/// What defToJSON reads of a definition: implemented by [`Def`] and by a snapshot's compact copy ([`DefJson`]).
pub trait DefView {
    fn id(&self) -> &str;
    fn kind(&self) -> &str;
    fn name(&self) -> &str;
    fn next(&self) -> f64;
    fn starter(&self) -> Option<&String>;
    fn dyn_info(&self) -> Option<&Dyn>;
    fn verts(&self) -> impl Iterator<Item = (f64, &P3)>;
    fn edges(&self) -> impl Iterator<Item = (f64, &Edge)>;
    fn faces(&self) -> impl Iterator<Item = (f64, &Face)>;
    fn insts(&self) -> impl Iterator<Item = (f64, &Inst)>;
    fn splines(&self) -> impl Iterator<Item = (&String, &Spline)>;
    fn has_splines(&self) -> bool;
}
impl DefView for Def {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind(&self) -> &str {
        &self.kind
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn next(&self) -> f64 {
        self.next
    }
    fn starter(&self) -> Option<&String> {
        self.starter.as_ref()
    }
    fn dyn_info(&self) -> Option<&Dyn> {
        self.dyn_.as_ref()
    }
    fn verts(&self) -> impl Iterator<Item = (f64, &P3)> {
        self.verts.iter()
    }
    fn edges(&self) -> impl Iterator<Item = (f64, &Edge)> {
        self.edges.iter()
    }
    fn faces(&self) -> impl Iterator<Item = (f64, &Face)> {
        self.faces.iter()
    }
    fn insts(&self) -> impl Iterator<Item = (f64, &Inst)> {
        self.insts.iter()
    }
    fn splines(&self) -> impl Iterator<Item = (&String, &Spline)> {
        self.splines.iter()
    }
    fn has_splines(&self) -> bool {
        !self.splines.is_empty()
    }
}

/// A definition's JSON content as plain lists (an undo snapshot's entry: compact, quick to make and compare).
#[derive(Clone, Debug)]
pub struct DefJson {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub next: f64,
    pub starter: Option<String>,
    pub dyn_: Option<Dyn>,
    pub verts: Vec<(f64, P3)>,
    pub edges: Vec<(f64, Edge)>,
    pub faces: Vec<(f64, Face)>,
    pub insts: Vec<(f64, Inst)>,
    pub splines: Vec<(String, Spline)>,
}
impl DefJson {
    pub fn of(d: &Def) -> Self {
        Self {
            id: d.id.clone(),
            kind: d.kind.clone(),
            name: d.name.clone(),
            next: d.next,
            starter: d.starter.clone(),
            dyn_: d.dyn_.clone(),
            verts: d.verts.iter().map(|(k, p)| (k, *p)).collect(),
            edges: d.edges.entries(),
            faces: d.faces.entries(),
            insts: d.insts.entries(),
            splines: d.splines.iter().map(|(k, s)| (k.clone(), s.clone())).collect(),
        }
    }
}
impl DefView for DefJson {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind(&self) -> &str {
        &self.kind
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn next(&self) -> f64 {
        self.next
    }
    fn starter(&self) -> Option<&String> {
        self.starter.as_ref()
    }
    fn dyn_info(&self) -> Option<&Dyn> {
        self.dyn_.as_ref()
    }
    fn verts(&self) -> impl Iterator<Item = (f64, &P3)> {
        self.verts.iter().map(|(k, p)| (*k, p))
    }
    fn edges(&self) -> impl Iterator<Item = (f64, &Edge)> {
        self.edges.iter().map(|(k, e)| (*k, e))
    }
    fn faces(&self) -> impl Iterator<Item = (f64, &Face)> {
        self.faces.iter().map(|(k, f)| (*k, f))
    }
    fn insts(&self) -> impl Iterator<Item = (f64, &Inst)> {
        self.insts.iter().map(|(k, i)| (*k, i))
    }
    fn splines(&self) -> impl Iterator<Item = (&String, &Spline)> {
        self.splines.iter().map(|(k, s)| (k, s))
    }
    fn has_splines(&self) -> bool {
        !self.splines.is_empty()
    }
}

/// `nid(def)`.
#[inline]
pub fn nid(def: &mut Def) -> f64 {
    let id = def.next;
    def.next += 1.0;
    id
}
pub fn v_add(def: &mut Def, p: P3) -> f64 {
    let id = nid(def);
    def.verts.set(id, p);
    id
}
pub fn find_vert(def: &Def, p: P3, tol: f64) -> Option<f64> {
    def.verts.iter().find(|(_, q)| near(p, **q, tol)).map(|(id, _)| id)
}
/// `ek(def)`: the edge-key cache, built from the edges on first use.
pub fn ek(def: &mut Def) -> &mut FxMap<EdgeKey, f64> {
    if def.ek.is_none() {
        let mut m = FxMap::default();
        for (id, e) in def.edges.iter() {
            m.insert(key(e.a, e.b), id);
        }
        def.ek = Some(m);
    }
    def.ek.get_or_insert_with(FxMap::default)
}
pub fn edge_between(def: &mut Def, a: f64, b: f64) -> Option<f64> {
    ek(def).get(&key(a, b)).copied()
}
/// `eAdd(def, a, b, soft, curve, hidden)` → the edge id (an existing one between a and b), None when a = b.
pub fn e_add(def: &mut Def, a: f64, b: f64, soft: bool, curve: Option<String>, hidden: bool) -> Option<f64> {
    if a == b {
        return None;
    }
    let k = key(a, b);
    if let Some(&have) = ek(def).get(&k) {
        return Some(have);
    }
    let id = nid(def);
    let curve = curve.filter(|c| !c.is_empty());
    def.edges.set(id, Edge { a, b, soft, curve, hidden });
    ek(def).insert(k, id);
    Some(id)
}
#[inline]
pub fn e_add0(def: &mut Def, a: f64, b: f64) -> Option<f64> {
    e_add(def, a, b, false, None, false)
}
pub fn e_remove(def: &mut Def, id: f64) {
    let Some(e) = def.edges.delete(id) else { return };
    if let Some(m) = def.ek.as_mut() {
        m.remove(&key(e.a, e.b));
    }
}
/// `fAdd(def, loops, color, id, hidden)`.
pub fn f_add(def: &mut Def, loops: Vec<Vec<f64>>, color: Option<String>, id: Option<f64>, hidden: bool) -> f64 {
    let fid = match id {
        Some(i) => i,
        None => nid(def),
    };
    for l in &loops {
        let n = l.len();
        for (i, &a) in l.iter().enumerate() {
            let b = l.get((i + 1) % n).copied().unwrap_or(a);
            e_add0(def, a, b);
        }
    }
    let color = color.filter(|c| !c.is_empty());
    def.faces.set(fid, Face { loops, color, hidden });
    fid
}

/// `fPts(def, f)`.
pub fn f_pts(def: &Def, f: &Face) -> Vec<Vec<P3>> {
    f.loops.iter().map(|l| l.iter().map(|&v| def.v(v)).collect()).collect()
}
/// The Newell vector (unnormalised) of a loop of vertex ids, the JS's order of operations.
fn newell_ids(def: &Def, l: &[f64]) -> P3 {
    let (mut x, mut y, mut z) = (0.0, 0.0, 0.0);
    let Some(&l0) = l.first() else { return [x, y, z] };
    // each corner looked up once: (p, q) = (l[i], l[(i + 1) % n]) in the JS's order
    let first = def.v(l0);
    let mut p = first;
    for (i, _) in l.iter().enumerate() {
        let q = match l.get(i + 1) {
            Some(&b) => def.v(b),
            None => first,
        };
        x += (p[1] - q[1]) * (p[2] + q[2]);
        y += (p[2] - q[2]) * (p[0] + q[0]);
        z += (p[0] - q[0]) * (p[1] + q[1]);
        p = q;
    }
    [x, y, z]
}
/// `newell(loop.map(v → def.verts.get(v)))`.
pub fn newell_of(def: &Def, l: &[f64]) -> P3 {
    norm(newell_ids(def, l))
}
/// `polyArea3(loop.map(v → def.verts.get(v)))`.
pub fn poly_area_of(def: &Def, l: &[f64]) -> f64 {
    let [x, y, z] = newell_ids(def, l);
    crate::js::hypot3(x, y, z) / 2.0
}
pub fn face_normal(def: &Def, f: &Face) -> P3 {
    newell_of(def, f.loops.first().map_or(&[][..], |l| l.as_slice()))
}
pub fn face_plane(def: &Def, f: &Face) -> Plane {
    let n = face_normal(def, f);
    let p0 = f.loops.first().and_then(|l| l.first()).map_or([f64::NAN; 3], |&v| def.v(v));
    Plane { n, d: dot(n, p0) }
}
pub fn face_area(def: &Def, f: &Face) -> f64 {
    let mut a = poly_area_of(def, f.loops.first().map_or(&[][..], |l| l.as_slice()));
    for l in f.loops.iter().skip(1) {
        a -= poly_area_of(def, l);
    }
    a
}
/// `loopSegs(f)`.
pub fn loop_segs(f: &Face) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for l in &f.loops {
        let n = l.len();
        for (i, &a) in l.iter().enumerate() {
            out.push((a, l.get((i + 1) % n).copied().unwrap_or(a)));
        }
    }
    out
}
/// The loop segments of a face without a Vec (same order as loopSegs).
pub fn for_segs(f: &Face, mut cb: impl FnMut(f64, f64)) {
    for l in &f.loops {
        let n = l.len();
        for (i, &a) in l.iter().enumerate() {
            cb(a, l.get((i + 1) % n).copied().unwrap_or(a));
        }
    }
}
pub type EdgeFaces = crate::dense::Rows<EdgeKey>;
/// `edgeFaces(def)`: edge key → the ids of the faces using it (in face order).
pub fn edge_faces(def: &Def) -> EdgeFaces {
    let mut pairs: Vec<(EdgeKey, f64)> = Vec::with_capacity(def.edges.len() * 2);
    for (fid, f) in def.faces.iter() {
        for_segs(f, |a, b| pairs.push((key(a, b), fid)));
    }
    crate::dense::Rows::build(&pairs, false)
}
#[inline]
pub fn ef_get(ef: &EdgeFaces, a: f64, b: f64) -> &[f64] {
    ef.get(&key(a, b))
}
/// `vertEdges(def)`: vertex → edge ids.
pub fn vert_edges(def: &Def) -> VAdj<'_> {
    let mut pairs = Vec::with_capacity(def.edges.len() * 2);
    for (id, e) in def.edges.iter() {
        pairs.push((e.a, id));
        pairs.push((e.b, id));
    }
    VAdj::build(&def.verts, &pairs, false)
}
#[inline]
pub fn ve_get<'a>(ve: &'a VAdj<'_>, v: f64) -> &'a [f64] {
    ve.get(v)
}
/// `vertFaces(def)`: vertex → face ids (a Set in insertion order: the faces of one vertex are added face by face, so
/// a repeat is always the last one).
pub fn vert_faces(def: &Def) -> VAdj<'_> {
    let mut pairs = Vec::new();
    for (fid, f) in def.faces.iter() {
        for l in &f.loops {
            for &v in l {
                pairs.push((v, fid));
            }
        }
    }
    VAdj::build(&def.verts, &pairs, true)
}

/// Triangles of a face as vertex-id triples (counter-clockwise seen from the front).
pub fn face_tris(def: &Def, f: &Face) -> Vec<[f64; 3]> {
    let n = face_normal(def, f);
    if !crate::js::truthy(len(n)) {
        return Vec::new();
    }
    // the common faces of big (imported) models: a triangle is itself, a quad is split along a diagonal that keeps
    // both halves facing front
    if let [l] = f.loops.as_slice()
        && (l.len() == 3 || l.len() == 4)
    {
        if let [a, b, c] = l.as_slice() {
            return vec![[*a, *b, *c]];
        }
        if let [l0, l1, l2, l3] = *l.as_slice() {
            let p = [def.v(l0), def.v(l1), def.v(l2), def.v(l3)];
            let up = |a: usize, b: usize, c: usize| {
                let g = |i: usize| p.get(i).copied().unwrap_or([f64::NAN; 3]);
                let (pa, pb, pc) = (g(a), g(b), g(c));
                dot(cross(sub(pb, pa), sub(pc, pa)), n) > 0.0
            };
            if up(0, 1, 2) && up(0, 2, 3) {
                return vec![[l0, l1, l2], [l0, l2, l3]];
            }
            if up(1, 2, 3) && up(1, 3, 0) {
                return vec![[l1, l2, l3], [l1, l3, l0]];
            }
        }
    }
    let fr = frame(n);
    let mut flat = Vec::new();
    let mut ids = Vec::new();
    let mut holes = Vec::new();
    for (i, l) in f.loops.iter().enumerate() {
        if i > 0 {
            holes.push(ids.len());
        }
        for &v in l {
            let p = to2(&fr, def.v(v));
            flat.push(p[0]);
            flat.push(p[1]);
            ids.push(v);
        }
    }
    let t = earcut(&flat, &holes);
    let id = |i: usize| ids.get(i).copied().unwrap_or(f64::NAN);
    let mut out = Vec::with_capacity(t.len() / 3);
    for c in t.chunks(3) {
        if let [a, b, cc] = *c {
            let tri = [id(a), id(b), id(cc)];
            let (pa, pb, pc) = (def.v(tri[0]), def.v(tri[1]), def.v(tri[2]));
            // earcut keeps the input winding of the outer ring only when it was clockwise in its frame: put every
            // triangle on the face's front
            out.push(if dot(cross(sub(pb, pa), sub(pc, pa)), n) < 0.0 { [tri[0], tri[2], tri[1]] } else { tri });
        }
    }
    out
}

// ---- 2D helpers ----
pub fn area2(poly: &[P2]) -> f64 {
    let mut s = 0.0;
    let n = poly.len();
    for (i, p) in poly.iter().enumerate() {
        let q = poly.get((i + 1) % n).copied().unwrap_or(*p);
        s += p[0] * q[1] - q[0] * p[1];
    }
    s / 2.0
}
pub fn in_poly(pt: P2, poly: &[P2]) -> bool {
    let mut inside = false;
    let n = poly.len();
    if n == 0 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (Some(a), Some(b)) = (poly.get(i), poly.get(j)) else { break };
        if (a[1] > pt[1]) != (b[1] > pt[1]) && pt[0] < (b[0] - a[0]) * (pt[1] - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
        j = i;
    }
    inside
}
pub fn in_region(pt: P2, loops: &[Vec<P2>]) -> bool {
    loops.first().is_some_and(|o| in_poly(pt, o)) && !loops.iter().skip(1).any(|h| in_poly(pt, h))
}
/// A point well inside a region (outer + holes): the centroid of its largest triangle.
pub fn sample_point(loops: &[&[P2]]) -> Option<P2> {
    let mut flat = Vec::new();
    let mut holes = Vec::new();
    let mut pts: Vec<P2> = Vec::new();
    for (i, l) in loops.iter().enumerate() {
        if i > 0 {
            holes.push(pts.len());
        }
        for p in l.iter() {
            flat.push(p[0]);
            flat.push(p[1]);
            pts.push(*p);
        }
    }
    let t = earcut(&flat, &holes);
    let mut best = None;
    let mut ba = -1.0;
    let at = |i: usize| pts.get(i).copied().unwrap_or([f64::NAN; 2]);
    for c in t.chunks(3) {
        if let [i, j, k] = *c {
            let (a, b, cc) = (at(i), at(j), at(k));
            let ar = ((b[0] - a[0]) * (cc[1] - a[1]) - (cc[0] - a[0]) * (b[1] - a[1])).abs();
            if ar > ba {
                ba = ar;
                best = Some([(a[0] + b[0] + cc[0]) / 3.0, (a[1] + b[1] + cc[1]) / 3.0]);
            }
        }
    }
    if ba > 1e-14 { best } else { None }
}

/// A face's loops in a frame's 2D coordinates.
pub fn loops2(def: &Def, f: &Face, fr: &Frame) -> Vec<Vec<P2>> {
    f.loops.iter().map(|l| l.iter().map(|&v| to2(fr, def.v(v))).collect()).collect()
}

/// Default tolerance wrapper: `findVert(def, p)`.
#[inline]
pub fn find_vert_eps(def: &Def, p: P3) -> Option<f64> {
    find_vert(def, p, EPS)
}
