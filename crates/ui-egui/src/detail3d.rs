//! A DetailCraft model in the 3D view, built the way DetailCraft's view3d.js builds its three.js scene from
//! `build(state)`: each part its profile extruded along its own frame (copes cut by splitting the length, holes as
//! dark cylinders), bolts and anchor rods, footings drawn as see-through boxes (their edges) with their bars as tubes,
//! grid lines on every level. The scene is in metres (the model is mm, as view3d.js scales its root by 0.001); parts
//! with the same profile, length, cuts and holes share one mesh (one GPU mesh per shared shape, placed by each part's
//! frame). Grid bubbles and level names are drawn by the view as text over the 3D picture ([`Label`]).

use std::collections::HashMap;

use cc_detail::build::{Built, Part};
use cc_detail::geom::clip_loop_v;
use cc_detail::marks::Marks;
use cc_render::mesh3d::{IDENT, Instance3, MeshData};
use serde_json::Value;

use crate::view3d::Scene3D;

/// mm → m
const MM: f64 = 0.001;

/// view3d.js's colours (its light-theme layer palette): scene content, not chrome.
pub mod palette {
    pub const COLUMN: [u8; 4] = [0x5b, 0x3c, 0xc4, 255];
    pub const BEAM: [u8; 4] = [0x00, 0x70, 0x7e, 255];
    pub const BRACE: [u8; 4] = [0x17, 0x70, 0x3a, 255];
    pub const PLATE: [u8; 4] = [0xa8, 0x4a, 0x00, 255];
    pub const BOLT: [u8; 4] = [0x8a, 0x62, 0x00, 255];
    pub const BAR: [u8; 4] = [0xb3, 0x12, 0x2e, 255];
    pub const HOLE: [u8; 4] = [0x17, 0x20, 0x2b, 255];
    pub const PICK: [u8; 4] = [0x1e, 0x48, 0xc8, 255];
}

/// What a click selects (view3d.js `ref`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Ref {
    Member(String),
    Conn(String),
    Stair(String),
    Footing(String),
}

impl Ref {
    /// "member:m3" …
    pub fn key(&self) -> String {
        match self {
            Ref::Member(i) => format!("member:{i}"),
            Ref::Conn(i) => format!("conn:{i}"),
            Ref::Stair(i) => format!("stair:{i}"),
            Ref::Footing(i) => format!("footing:{i}"),
        }
    }
    pub fn parse(s: &str) -> Option<Ref> {
        let (k, id) = s.split_once(':')?;
        let id = id.trim();
        if id.is_empty() {
            return None;
        }
        Some(match k.trim() {
            "member" => Ref::Member(id.into()),
            "conn" => Ref::Conn(id.into()),
            "stair" => Ref::Stair(id.into()),
            "footing" => Ref::Footing(id.into()),
            _ => return None,
        })
    }
    pub fn id(&self) -> &str {
        match self {
            Ref::Member(i) | Ref::Conn(i) | Ref::Stair(i) | Ref::Footing(i) => i,
        }
    }
    /// The ref a part belongs to (view3d.js: member, else stair, else connection).
    pub fn of_part(p: &Part) -> Option<Ref> {
        if let Some(m) = &p.member {
            return Some(Ref::Member(m.clone()));
        }
        if let Some(s) = &p.stair {
            return Some(Ref::Stair(s.clone()));
        }
        p.conn.as_ref().map(|c| Ref::Conn(c.clone()))
    }
}

/// A text drawn over the 3D view at a world point (metres): grid bubbles and level names.
#[derive(Clone, Debug)]
pub struct Label {
    pub text: String,
    pub at: [f64; 3],
    pub bubble: bool,
    /// 'x' a numbered grid line's bubble, 'y' a lettered one's, 'l' a level's name
    pub axis: char,
    /// a level's z (mm), so a plan shows only its own
    pub z: f64,
}

/// A footing's box for picking (the faces are not drawn, so the triangle picker cannot find it): centre (m), half
/// sizes (m), rotation about z (radians).
#[derive(Clone, Debug)]
pub struct FootBox {
    pub r: Ref,
    pub c: [f64; 3],
    pub half: [f64; 3],
    pub rot: f64,
}

/// The built scene and what each instance stands for.
pub struct DetailScene {
    pub scene: Scene3D,
    /// per instance: the ref it selects (None: grids, bolts)
    pub refs: Vec<Option<Ref>>,
    /// per instance: its colour when not selected
    pub base: Vec<[u8; 4]>,
    pub footings: Vec<FootBox>,
    pub labels: Vec<Label>,
    /// triangles in the scene (shared meshes counted once per placement)
    pub triangles: usize,
    /// per instance: its box (metres; grids and bolts: none, never hidden by a plan or elevation)
    pub boxes: Vec<Option<([f64; 3], [f64; 3])>>,
}

fn colour_of(kind: &str) -> [u8; 4] {
    match kind {
        "column" => palette::COLUMN,
        "beam" | "stringer" => palette::BEAM,
        "brace" => palette::BRACE,
        _ => palette::PLATE,
    }
}

fn signed_area(l: &[[f64; 2]]) -> f64 {
    let n = l.len();
    let mut a = 0.0;
    for i in 0..n {
        let (Some(p), Some(q)) = (l.get(i), l.get((i + 1) % n)) else { continue };
        a += p[0] * q[1] - q[0] * p[1];
    }
    a / 2.0
}

/// A mesh being made (metres, in the part's own frame).
#[derive(Default)]
struct Mk {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    col: Vec<[u8; 4]>,
    idx: Vec<u32>,
    edges: Vec<[f32; 3]>,
}

fn f3(p: [f64; 3]) -> [f32; 3] {
    [p[0] as f32, p[1] as f32, p[2] as f32]
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: [f64; 3]) -> [f64; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if l > 1e-300 { [a[0] / l, a[1] / l, a[2] / l] } else { [0.0, 0.0, 1.0] }
}

impl Mk {
    /// A flat triangle (corners in metres), wound so its normal is the right-hand one.
    fn tri(&mut self, a: [f64; 3], b: [f64; 3], c: [f64; 3], col: [u8; 4]) {
        let n = f3(norm(cross(sub(b, a), sub(c, a))));
        for p in [a, b, c] {
            let Ok(i) = u32::try_from(self.pos.len()) else { return };
            self.idx.push(i);
            self.pos.push(f3(p));
            self.nrm.push(n);
            self.col.push(col);
        }
    }
    fn quad(&mut self, a: [f64; 3], b: [f64; 3], c: [f64; 3], d: [f64; 3], col: [u8; 4]) {
        self.tri(a, b, c, col);
        self.tri(a, c, d, col);
    }
    fn edge(&mut self, a: [f64; 3], b: [f64; 3]) {
        self.edges.push(f3(a));
        self.edges.push(f3(b));
    }
    fn done(self) -> MeshData {
        MeshData { origin: [0.0; 3], positions: self.pos, normals: self.nrm, colors: self.col, indices: self.idx, edges: self.edges }
    }
    fn triangles(&self) -> usize {
        self.idx.len() / 3
    }
}

/// One slice of a profile (outer loop + holes, mm) extruded from z = a to z = b (mm), in metres; edges where the
/// outline turns by more than 35° (view3d.js `EdgesGeometry(geo, 35)`) and the outlines of both ends.
fn extrude(m: &mut Mk, loops: &[Vec<[f64; 2]>], a: f64, b: f64, col: [u8; 4]) {
    let Some(outer) = loops.first() else { return };
    if outer.len() < 3 {
        return;
    }
    // outer counter-clockwise, holes clockwise (seen from +z)
    let mut ls: Vec<Vec<[f64; 2]>> = Vec::with_capacity(loops.len());
    for (k, l) in loops.iter().enumerate() {
        if l.len() < 3 {
            continue;
        }
        let ccw = signed_area(l) > 0.0;
        let want_ccw = k == 0;
        let mut l = l.clone();
        if ccw != want_ccw {
            l.reverse();
        }
        ls.push(l);
    }
    let p3 = |q: [f64; 2], z: f64| [q[0] * MM, q[1] * MM, z * MM];
    // caps through earcut (outer first, then the holes)
    let mut flat: Vec<f64> = Vec::new();
    let mut holes: Vec<usize> = Vec::new();
    let mut all: Vec<[f64; 2]> = Vec::new();
    for (k, l) in ls.iter().enumerate() {
        if k > 0 {
            holes.push(all.len());
        }
        for q in l {
            flat.push(q[0]);
            flat.push(q[1]);
            all.push(*q);
        }
    }
    let tris = cc_m3d::earcut::earcut(&flat, &holes);
    for &[i0, i1, i2] in tris.as_chunks::<3>().0 {
        let (Some(&pa), Some(&pb), Some(&pc)) = (all.get(i0), all.get(i1), all.get(i2)) else { continue };
        let ccw = (pb[0] - pa[0]) * (pc[1] - pa[1]) - (pc[0] - pa[0]) * (pb[1] - pa[1]) > 0.0;
        let (q1, q2) = if ccw { (pb, pc) } else { (pc, pb) };
        // the far end faces +z (counter-clockwise), the near end −z
        m.tri(p3(pa, b), p3(q1, b), p3(q2, b), col);
        m.tri(p3(pa, a), p3(q2, a), p3(q1, a), col);
    }
    let cos35 = 35f64.to_radians().cos();
    for l in &ls {
        let n = l.len();
        for i in 0..n {
            let (Some(&p), Some(&q)) = (l.get(i), l.get((i + 1) % n)) else { continue };
            m.quad(p3(p, a), p3(q, a), p3(q, b), p3(p, b), col);
            m.edge(p3(p, a), p3(q, a));
            m.edge(p3(p, b), p3(q, b));
            // a long edge at q when the outline turns there
            if let Some(&r) = l.get((i + 2) % n) {
                let (d1, d2) = ([q[0] - p[0], q[1] - p[1]], [r[0] - q[0], r[1] - q[1]]);
                let (l1, l2) = ((d1[0] * d1[0] + d1[1] * d1[1]).sqrt(), (d2[0] * d2[0] + d2[1] * d2[1]).sqrt());
                if l1 > 0.0 && l2 > 0.0 && (d1[0] * d2[0] + d1[1] * d2[1]) / (l1 * l2) < cos35 {
                    m.edge(p3(q, a), p3(q, b));
                }
            }
        }
    }
}

/// A closed cylinder of `sides` sides: centre `c`, unit axis `ax`, length `len`, diameter `d` (all metres).
#[allow(clippy::too_many_arguments)]
fn cylinder(m: &mut Mk, c: [f64; 3], ax: [f64; 3], len: f64, d: f64, sides: usize, col: [u8; 4], from_end: bool) {
    let ax = norm(ax);
    let helper = if ax[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
    let u = norm(cross(ax, helper));
    let v = cross(ax, u);
    let r = d / 2.0;
    let (z0, z1) = if from_end { (0.0, len) } else { (-len / 2.0, len / 2.0) };
    let at = |k: usize, z: f64| {
        let t = std::f64::consts::TAU * k as f64 / sides as f64;
        let (s, co) = t.sin_cos();
        [
            c[0] + (u[0] * co + v[0] * s) * r + ax[0] * z,
            c[1] + (u[1] * co + v[1] * s) * r + ax[1] * z,
            c[2] + (u[2] * co + v[2] * s) * r + ax[2] * z,
        ]
    };
    let e0 = [c[0] + ax[0] * z0, c[1] + ax[1] * z0, c[2] + ax[2] * z0];
    let e1 = [c[0] + ax[0] * z1, c[1] + ax[1] * z1, c[2] + ax[2] * z1];
    for k in 0..sides {
        let k1 = (k + 1) % sides;
        m.quad(at(k, z0), at(k1, z0), at(k1, z1), at(k, z1), col);
        m.tri(e1, at(k, z1), at(k1, z1), col);
        m.tri(e0, at(k1, z0), at(k, z0), col);
    }
}

/// view3d.js `partGeometry(p)` (+ its holes): the part's solid in its own frame (u, v, x), metres.
fn part_mesh(p: &Part) -> Mk {
    let pr = &p.profile;
    let l = p.length;
    let col = colour_of(&p.kind);
    let copes: Vec<_> = p.cuts.iter().filter(|c| c.kind == "cope").collect();
    let mut xs: Vec<f64> = vec![0.0, l];
    for c in &copes {
        xs.push(if c.end == "start" { c.length.min(l) } else { (l - c.length).max(0.0) });
    }
    xs.retain(|x| x.is_finite());
    xs.sort_by(f64::total_cmp);
    xs.dedup();
    let mut m = Mk::default();
    for w in xs.windows(2) {
        let &[a, b] = w else { continue };
        if b - a < 0.01 {
            continue;
        }
        let mid = (a + b) / 2.0;
        let mut loops: Vec<Vec<[f64; 2]>> = pr.loops.clone();
        for c in &copes {
            let in_cope = if c.end == "start" { mid < c.length } else { mid > l - c.length };
            if !in_cope {
                continue;
            }
            let lim = if c.side == "top" { pr.depth / 2.0 - c.depth } else { -pr.depth / 2.0 + c.depth };
            loops = loops.first().map(|l0| clip_loop_v(l0, lim, c.side == "top")).filter(|l| !l.is_empty()).into_iter().collect();
        }
        if loops.is_empty() {
            continue;
        }
        extrude(&mut m, &loops, a, b, col);
    }
    let g = |v: Option<f64>| v.unwrap_or(0.0);
    for h in &p.holes {
        let kind = pr.kind.as_str();
        let (c, axis, len) = if h.face == "web" {
            let cu = match kind {
                "C" => -g(pr.bf) / 2.0 + g(pr.tw) / 2.0,
                "L" => -g(pr.b) / 2.0 + g(pr.t) / 2.0,
                _ => 0.0,
            };
            let t = match kind {
                "RHS" => g(pr.b),
                "L" => g(pr.t),
                _ => g(pr.tw),
            };
            ([cu, h.y, h.x], [1.0, 0.0, 0.0], t + 2.0)
        } else if h.face == "plate" {
            ([h.y, 0.0, h.x], [0.0, 1.0, 0.0], g(pr.t) + 2.0)
        } else {
            let s = if h.face == "top" { 1.0 } else { -1.0 };
            let t = if kind == "RHS" || kind == "L" { g(pr.t) } else { g(pr.tf) };
            ([h.y, s * (pr.depth / 2.0 - t / 2.0), h.x], [0.0, 1.0, 0.0], t + 2.0)
        };
        if h.d.is_finite() && h.d > 0.0 && len.is_finite() {
            cylinder(&mut m, [c[0] * MM, c[1] * MM, c[2] * MM], axis, len * MM, h.d * MM, 14, palette::HOLE, false);
        }
    }
    m
}

/// The shape key of a part: parts with equal keys share one mesh.
fn shape_key(p: &Part) -> String {
    use std::fmt::Write;
    let mut k = String::with_capacity(96);
    let _ = write!(k, "{}|{}|{}|{}", p.profile_name, p.kind, p.length.to_bits(), p.profile.loops.len());
    for c in &p.cuts {
        let _ = write!(k, "|c{}{}{}{}{}", c.kind, c.end, c.side, c.length.to_bits(), c.depth.to_bits());
    }
    for h in &p.holes {
        let _ = write!(k, "|h{}{}{}{}", h.face, h.x.to_bits(), h.y.to_bits(), h.d.to_bits());
    }
    k
}

/// The part's frame as a column-major matrix in metres: columns u, v, x and the origin moved by `voff` along v.
fn part_matrix(p: &Part) -> [f64; 16] {
    let f = &p.frame;
    let o = [f.o[0] + f.v[0] * p.voff, f.o[1] + f.v[1] * p.voff, f.o[2] + f.v[2] * p.voff];
    [f.u[0], f.u[1], f.u[2], 0.0, f.v[0], f.v[1], f.v[2], 0.0, f.x[0], f.x[1], f.x[2], 0.0, o[0] * MM, o[1] * MM, o[2] * MM, 1.0]
}

fn grow(lo: &mut [f64; 3], hi: &mut [f64; 3], p: [f64; 3]) {
    for ((l, h), v) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
        if v.is_finite() {
            *l = l.min(v);
            *h = h.max(v);
        }
    }
}

fn mul_pt(m: &[f64; 16], p: [f32; 3]) -> [f64; 3] {
    let (x, y, z) = (f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
    [m[0] * x + m[4] * y + m[8] * z + m[12], m[1] * x + m[5] * y + m[9] * z + m[13], m[2] * x + m[6] * y + m[10] * z + m[14]]
}

fn num(v: Option<&Value>) -> f64 {
    v.and_then(Value::as_f64).unwrap_or(f64::NAN)
}

/// Build the scene of a model: `hidden` phases are left out (view3d.js `hiddenPhases`), bolts only when `bolts`.
pub fn build(state: &Value, built: &Built, _marks: &Marks, id: u64, hidden: &[f64], bolts: bool) -> DetailScene {
    let shown = |ph: f64| {
        let ph = if ph == 0.0 || ph.is_nan() { 1.0 } else { ph };
        !hidden.contains(&ph)
    };
    let mut meshes: Vec<MeshData> = Vec::new();
    let mut instances: Vec<Instance3> = Vec::new();
    let mut refs: Vec<Option<Ref>> = Vec::new();
    let mut base: Vec<[u8; 4]> = Vec::new();
    let mut labels: Vec<Label> = Vec::new();
    let mut footings: Vec<FootBox> = Vec::new();
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let mut triangles = 0usize;
    let mut boxes: Vec<Option<([f64; 3], [f64; 3])>> = Vec::new();
    let push = |meshes: &mut Vec<MeshData>, m: MeshData| {
        meshes.push(m);
        meshes.len() - 1
    };

    // grids on every level, labels at the first
    let gx: Vec<(String, f64)> = cc_detail::js::arr(state.get("grids").and_then(|g| g.get("x")))
        .iter()
        .map(|g| (cc_detail::js::string_of(g.get("label")), num(g.get("at"))))
        .collect();
    let gy: Vec<(String, f64)> = cc_detail::js::arr(state.get("grids").and_then(|g| g.get("y")))
        .iter()
        .map(|g| (cc_detail::js::string_of(g.get("label")), num(g.get("at"))))
        .collect();
    let levels: Vec<(String, f64)> =
        cc_detail::js::arr(state.get("levels")).iter().map(|l| (cc_detail::js::string_of(l.get("name")), num(l.get("z")))).collect();
    if let (Some(fx), Some(lx), Some(fy), Some(ly), Some(l0)) = (gx.first(), gx.last(), gy.first(), gy.last(), levels.first()) {
        let pad = 1500.0;
        let (x0, x1, y0, y1) = (fx.1 - pad, lx.1 + pad, fy.1 - pad, ly.1 + pad);
        let mut m = Mk::default();
        for (_, z) in &levels {
            for (_, at) in &gx {
                m.edge([at * MM, y0 * MM, z * MM], [at * MM, y1 * MM, z * MM]);
            }
            for (_, at) in &gy {
                m.edge([x0 * MM, at * MM, z * MM], [x1 * MM, at * MM, z * MM]);
            }
        }
        for (name, z) in &levels {
            labels.push(Label {
                text: format!("{name} ({})", z.round()),
                at: [(x1 + 300.0) * MM, y0 * MM, z * MM],
                bubble: false,
                axis: 'l',
                z: *z,
            });
        }
        for (l, at) in &gx {
            labels.push(Label { text: l.clone(), at: [at * MM, (y0 - 700.0) * MM, l0.1 * MM], bubble: true, axis: 'x', z: l0.1 });
        }
        for (l, at) in &gy {
            labels.push(Label { text: l.clone(), at: [(x0 - 700.0) * MM, at * MM, l0.1 * MM], bubble: true, axis: 'y', z: l0.1 });
        }
        let zt = levels.last().map_or(l0.1, |l| l.1);
        grow(&mut lo, &mut hi, [x0 * MM, y0 * MM, l0.1 * MM]);
        grow(&mut lo, &mut hi, [x1 * MM, y1 * MM, zt * MM]);
        let mi = push(&mut meshes, m.done());
        instances.push(Instance3 { mesh: mi, m: IDENT, tint: None, hidden: false });
        refs.push(None);
        base.push([0; 4]);
        boxes.push(None);
    }

    // parts: one mesh per shape
    let mut by_shape: HashMap<String, (usize, usize)> = HashMap::new();
    for p in &built.parts {
        if !shown(p.phase) {
            continue;
        }
        let key = shape_key(p);
        let (mi, tris) = match by_shape.get(&key) {
            Some(x) => *x,
            None => {
                let mk = part_mesh(p);
                let tris = mk.triangles();
                let mi = push(&mut meshes, mk.done());
                by_shape.insert(key, (mi, tris));
                (mi, tris)
            }
        };
        triangles = triangles.saturating_add(tris);
        let mat = part_matrix(p);
        let (mut blo, mut bhi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        if let Some(md) = meshes.get(mi) {
            // the box of the part: its mesh's corners through the frame (8 corners of the local box)
            let mut l = [f32::INFINITY; 3];
            let mut h = [f32::NEG_INFINITY; 3];
            for q in &md.positions {
                for ((lk, hk), v) in l.iter_mut().zip(h.iter_mut()).zip(q) {
                    *lk = lk.min(*v);
                    *hk = hk.max(*v);
                }
            }
            if l[0].is_finite() {
                for c in 0..8 {
                    let q =
                        [if c & 1 == 0 { l[0] } else { h[0] }, if c & 2 == 0 { l[1] } else { h[1] }, if c & 4 == 0 { l[2] } else { h[2] }];
                    grow(&mut lo, &mut hi, mul_pt(&mat, q));
                    grow(&mut blo, &mut bhi, mul_pt(&mat, q));
                }
            }
        }
        instances.push(Instance3 { mesh: mi, m: mat, tint: None, hidden: false });
        refs.push(Ref::of_part(p));
        base.push(colour_of(&p.kind));
        boxes.push(blo[0].is_finite().then_some((blo, bhi)));
    }

    // bolts and anchor rods: one mesh (they are not picked: view3d.js gives them no ref)
    if bolts {
        let mut m = Mk::default();
        for b in built.bolts.iter().filter(|b| shown(b.phase)) {
            if !(b.d > 0.0 && b.length > 0.0) {
                continue;
            }
            let head = [b.head[0] * MM, b.head[1] * MM, b.head[2] * MM];
            cylinder(&mut m, head, b.axis, b.length * MM, b.d * MM, 12, palette::BOLT, true);
        }
        if !m.idx.is_empty() {
            triangles = triangles.saturating_add(m.triangles());
            let mi = push(&mut meshes, m.done());
            instances.push(Instance3 { mesh: mi, m: IDENT, tint: None, hidden: false });
            refs.push(None);
            base.push(palette::BOLT);
            boxes.push(None);
        }
    }

    // footings: the box's edges (see-through) and the bars as tubes, picked as the footing
    for ft in &built.footings {
        let f = &ft.f;
        if !shown(f.phase) {
            continue;
        }
        let r = Ref::Footing(f.id.clone());
        let rot = f.rotation.to_radians();
        let (s, c) = rot.sin_cos();
        let cz = f.top - f.h / 2.0;
        let corner = |i: usize| {
            let (dx, dy, dz) = (
                if i & 1 == 0 { -f.l / 2.0 } else { f.l / 2.0 },
                if i & 2 == 0 { -f.b / 2.0 } else { f.b / 2.0 },
                if i & 4 == 0 { -f.h / 2.0 } else { f.h / 2.0 },
            );
            [(f.x + dx * c - dy * s) * MM, (f.y + dx * s + dy * c) * MM, (cz + dz) * MM]
        };
        let mut m = Mk::default();
        for (a, b) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
            m.edge(corner(a), corner(b));
        }
        let (mut blo, mut bhi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for i in 0..8 {
            grow(&mut lo, &mut hi, corner(i));
            grow(&mut blo, &mut bhi, corner(i));
        }
        for g in &ft.bars {
            let db = cc_detail::rebar::bar(&g.bar).map_or(12.0, |b| b.db);
            for set in &g.sets {
                let pts: Vec<[f64; 3]> =
                    cc_detail::rebar::bar_polyline(g, set, 4).iter().map(|p| [p[0] * MM, p[1] * MM, p[2] * MM]).collect();
                for w in pts.windows(2) {
                    let &[p0, p1] = w else { continue };
                    let d = sub(p1, p0);
                    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                    if len > 1e-9 {
                        cylinder(&mut m, p0, d, len, db * MM, 6, palette::BAR, true);
                    }
                }
            }
        }
        triangles = triangles.saturating_add(m.triangles());
        let mi = push(&mut meshes, m.done());
        instances.push(Instance3 { mesh: mi, m: IDENT, tint: None, hidden: false });
        refs.push(Some(r.clone()));
        base.push(palette::BAR);
        boxes.push(Some((blo, bhi)));
        footings.push(FootBox { r, c: [f.x * MM, f.y * MM, cz * MM], half: [f.l / 2.0 * MM, f.b / 2.0 * MM, f.h / 2.0 * MM], rot });
    }

    if !lo[0].is_finite() {
        lo = [-5.0, -5.0, 0.0];
        hi = [5.0, 5.0, 3.0];
    }
    DetailScene { scene: Scene3D::new(id, meshes, instances, lo, hi), refs, base, footings, labels, triangles, boxes }
}

/// The nearest footing box a ray (metres) passes through: (ref, distance).
pub fn pick_footing(list: &[FootBox], o: [f64; 3], d: [f64; 3]) -> Option<(Ref, f64)> {
    let mut best: Option<(Ref, f64)> = None;
    for f in list {
        // into the box's frame
        let (s, c) = f.rot.sin_cos();
        let rel = sub(o, f.c);
        let lo = [rel[0] * c + rel[1] * s, -rel[0] * s + rel[1] * c, rel[2]];
        let ld = [d[0] * c + d[1] * s, -d[0] * s + d[1] * c, d[2]];
        let (mut t0, mut t1) = (0.0f64, f64::INFINITY);
        let mut hit = true;
        for ((oo, dd), h) in lo.iter().copied().zip(ld.iter().copied()).zip(f.half.iter().copied()) {
            if dd.abs() < 1e-15 {
                if oo.abs() > h {
                    hit = false;
                    break;
                }
                continue;
            }
            let (a, b) = ((-h - oo) / dd, (h - oo) / dd);
            t0 = t0.max(a.min(b));
            t1 = t1.min(a.max(b));
        }
        if hit && t0 <= t1 && best.as_ref().is_none_or(|(_, bt)| t0 < *bt) {
            best = Some((f.r.clone(), t0));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> (Value, Built, Marks) {
        let m = cc_detail::sample::sample_model("Sample frame").unwrap();
        let b = cc_detail::build::build(&m.state).unwrap();
        let k = cc_detail::marks::mark_parts(&b, 1.0, true);
        (m.state, b, k)
    }

    #[test]
    fn the_sample_becomes_a_scene_with_every_part() {
        let (s, b, k) = sample();
        let d = build(&s, &b, &k, 1, &[], true);
        let parts = d.refs.iter().filter(|r| matches!(r, Some(Ref::Member(_)) | Some(Ref::Conn(_)))).count();
        assert_eq!(parts, b.parts.len());
        assert!(d.scene.meshes.len() < d.scene.instances.len(), "equal parts share a mesh");
        assert_eq!(d.footings.len(), 7);
        // the frame is 12 m × 7 m × 4 m: the box (metres) holds it with the grid margin
        assert!(d.scene.min[0] < 0.0 && d.scene.max[0] > 12.0 && d.scene.max[2] >= 4.0 && d.scene.max[2] < 5.0);
        assert!(d.triangles > 1000);
        assert_eq!(d.labels.iter().filter(|l| l.bubble).count(), 5);
    }

    #[test]
    fn an_extrusion_is_closed_and_faces_out() {
        let mut m = Mk::default();
        let sq = vec![
            vec![[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]],
            vec![[25.0, 25.0], [25.0, 75.0], [75.0, 75.0], [75.0, 25.0]],
        ];
        extrude(&mut m, &sq, 0.0, 1000.0, palette::PLATE);
        // caps: 8 triangles each; sides: 8 quads
        assert_eq!(m.triangles(), 8 * 2 + 8 * 2);
        // every side normal points away from the section's middle (0.05, 0.05) for the outer loop
        let mid = [0.05f32, 0.05];
        let mut out = 0;
        for (p, n) in m.pos.iter().zip(&m.nrm) {
            if n[2].abs() < 0.5 {
                let r = [p[0] - mid[0], p[1] - mid[1]];
                let outer = r[0].abs() > 0.049 || r[1].abs() > 0.049;
                let dot = r[0] * n[0] + r[1] * n[1];
                if outer {
                    assert!(dot > 0.0);
                } else {
                    assert!(dot < 0.0, "a hole's walls face into the hole");
                }
                out += 1;
            }
        }
        assert!(out > 0);
    }

    #[test]
    fn a_ray_finds_a_footing() {
        let f = FootBox { r: Ref::Footing("f1".into()), c: [1.0, 1.0, -0.3], half: [1.0, 1.0, 0.3], rot: 0.3 };
        let hit = pick_footing(&[f], [1.0, 1.0, 10.0], [0.0, 0.0, -1.0]);
        assert_eq!(hit.map(|h| (h.0, (h.1 * 1000.0).round())), Some((Ref::Footing("f1".into()), 10000.0)));
        let f = FootBox { r: Ref::Footing("f1".into()), c: [1.0, 1.0, -0.3], half: [1.0, 1.0, 0.3], rot: 0.0 };
        assert!(pick_footing(&[f], [5.0, 5.0, 10.0], [0.0, 0.0, -1.0]).is_none());
    }

    #[test]
    fn refs_read_back() {
        for r in [Ref::Member("m1".into()), Ref::Conn("j2".into()), Ref::Stair("s1".into()), Ref::Footing("f3".into())] {
            assert_eq!(Ref::parse(&r.key()), Some(r));
        }
        assert_eq!(Ref::parse("member:"), None);
        assert_eq!(Ref::parse("x:y"), None);
    }
}
