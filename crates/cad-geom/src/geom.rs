//! geom.js, part 1: primitives, transforms, text layout, block references, intersections.
//!
//! Every entity breaks down into primitives ([`prims`]): `L` segments and `A` arcs (counter-clockwise from a0 to
//! a1, a1 > a0, sweep ≤ 2π). Snaps, picking, intersections, trim and extend all work on primitives. Angles are
//! radians; world Y points up.

use crate::jsmath::JsMath;
use cc_cad::Block;
use cc_cad::entity::{Arc, Color, Entity, Geo, Insert, Line, Obj, Text, Vertex, num};
use cc_cad::js;
use cc_cad::omap::OMap;
use cc_geom::P2;
use serde_json::Value;

use crate::font::advance;

pub const TAU: f64 = std::f64::consts::PI * 2.0;
pub const PI: f64 = std::f64::consts::PI;
pub const EPS: f64 = 1e-9;
/// The stand-in half length of construction lines (DR.6).
pub const FAR: f64 = 1e9;
const ELLIPSE_STEPS: f64 = 72.0;
/// × text height, baseline to baseline (the MTEXT default).
pub const LINE_SPACING: f64 = 5.0 / 3.0;
/// Font size for a text height: CAD text height is the capital height (Helvetica's is 0.718 em).
pub const FONT_EM: f64 = 1.0 / 0.718;
/// Most segments one arc or ellipse is cut into (guards absurd sweeps; the JS has no cap).
const MAX_STEPS: f64 = 1e6;

/// Blocks by name (doc.blocks).
pub type Blocks = OMap<String, Block>;

/// An affine transform [a, b, c, d, e, f]: x' = a·x + c·y + e, y' = b·x + d·y + f.
pub type M = [f64; 6];

pub fn p(x: f64, y: f64) -> P2 {
    P2::new(x, y)
}
pub fn dist(a: P2, b: P2) -> f64 {
    js::hypot(b.x - a.x, b.y - a.y)
}
pub fn angle_of(a: P2, b: P2) -> f64 {
    (b.y - a.y).jatan2(b.x - a.x)
}
pub fn polar(q: P2, ang: f64, d: f64) -> P2 {
    p(q.x + ang.jcos() * d, q.y + ang.jsin() * d)
}
pub fn norm_angle(a: f64) -> f64 {
    let a = a % TAU;
    if a < 0.0 { a + TAU } else { a }
}
pub fn deg(r: f64) -> f64 {
    r * 180.0 / PI
}
pub fn rad(d: f64) -> f64 {
    d * PI / 180.0
}

/// Is angle `a` on the CCW sweep a0 → a1 (with a small tolerance)?
pub fn on_sweep(a: f64, a0: f64, a1: f64, tol: f64) -> bool {
    let sweep = a1 - a0;
    if sweep >= TAU - tol {
        return true;
    }
    let d = norm_angle(a - a0);
    d <= sweep + tol || d >= TAU - tol
}
pub fn on_sweep_d(a: f64, a0: f64, a1: f64) -> bool {
    on_sweep(a, a0, a1, 1e-9)
}

/// A float step count `Math.max(lo, Math.ceil(x))` as a loop bound (NaN → no steps; capped).
pub fn steps(lo: f64, x: f64) -> usize {
    let n = js::max(lo, x.ceil());
    if !n.is_finite() || n < 0.0 {
        return 0;
    }
    let n = n.min(MAX_STEPS);
    // finite, 0..=1e6, integral
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let u = n as usize;
    u
}
/// usize → f64 (exact below 2^53).
pub fn f(n: usize) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let x = n as f64;
    x
}

// ---------------------------------------------------------------------------------------------------------------
// Primitives

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum K {
    L,
    A,
}

/// A primitive: `{k:'L', x1, y1, x2, y2}` or `{k:'A', cx, cy, r, a0, a1}` plus the flags the JS sets on some:
/// full (a whole circle), rev (a bulge arc travelled clockwise), seg (polyline segment), box (picking only, not
/// snaps), inf (a construction line's stand-in).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prim {
    pub k: K,
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub cx: f64,
    pub cy: f64,
    pub r: f64,
    pub a0: f64,
    pub a1: f64,
    pub full: bool,
    pub rev: Option<bool>,
    pub seg: Option<usize>,
    pub boxed: bool,
    pub inf: bool,
}
impl Prim {
    pub fn l(x1: f64, y1: f64, x2: f64, y2: f64) -> Prim {
        Prim {
            k: K::L,
            x1,
            y1,
            x2,
            y2,
            cx: 0.0,
            cy: 0.0,
            r: 0.0,
            a0: 0.0,
            a1: 0.0,
            full: false,
            rev: None,
            seg: None,
            boxed: false,
            inf: false,
        }
    }
    pub fn a(cx: f64, cy: f64, r: f64, a0: f64, a1: f64) -> Prim {
        Prim { k: K::A, cx, cy, r, a0, a1, ..Prim::l(0.0, 0.0, 0.0, 0.0) }
    }
    /// A whole circle `{k:'A', …, a0: 0, a1: 2π, full: true}`.
    pub fn circle(cx: f64, cy: f64, r: f64) -> Prim {
        Prim { full: true, ..Prim::a(cx, cy, r, 0.0, TAU) }
    }
    pub fn is_l(&self) -> bool {
        self.k == K::L
    }
    pub fn rev(&self) -> bool {
        self.rev.unwrap_or(false)
    }
    pub fn to_value(&self) -> Value {
        let mut m = Obj::new();
        match self.k {
            K::L => {
                m.insert("k".into(), "L".into());
                for (k, v) in [("x1", self.x1), ("y1", self.y1), ("x2", self.x2), ("y2", self.y2)] {
                    m.insert(k.into(), num(v));
                }
            }
            K::A => {
                m.insert("k".into(), "A".into());
                for (k, v) in [("cx", self.cx), ("cy", self.cy), ("r", self.r), ("a0", self.a0), ("a1", self.a1)] {
                    m.insert(k.into(), num(v));
                }
            }
        }
        if self.full {
            m.insert("full".into(), true.into());
        }
        if let Some(r) = self.rev {
            m.insert("rev".into(), r.into());
        }
        if let Some(s) = self.seg {
            m.insert("seg".into(), Value::from(s));
        }
        if self.boxed {
            m.insert("box".into(), true.into());
        }
        if self.inf {
            m.insert("inf".into(), true.into());
        }
        Value::Object(m)
    }
    /// From the JSON shape (for the parity harness and scripts).
    pub fn from_value(v: &Value) -> Option<Prim> {
        let g = |k: &str| v.get(k).and_then(Value::as_f64);
        let mut out = match v.get("k")?.as_str()? {
            "L" => Prim::l(g("x1")?, g("y1")?, g("x2")?, g("y2")?),
            "A" => Prim::a(g("cx")?, g("cy")?, g("r")?, g("a0")?, g("a1")?),
            _ => return None,
        };
        out.full = v.get("full").and_then(Value::as_bool).unwrap_or(false);
        out.rev = v.get("rev").and_then(Value::as_bool);
        out.seg = v.get("seg").and_then(Value::as_u64).and_then(|s| usize::try_from(s).ok());
        out.boxed = v.get("box").and_then(Value::as_bool).unwrap_or(false);
        out.inf = v.get("inf").and_then(Value::as_bool).unwrap_or(false);
        Some(out)
    }
}

/// The arc between p and q for bulge b (tan of a quarter of the included angle; + = CCW).
pub fn bulge_arc(pp: P2, q: P2, b: f64) -> Prim {
    let theta = 4.0 * b.jatan();
    let c = dist(pp, q);
    let r = (c / (2.0 * (theta / 2.0).jsin())).abs();
    let mid = p((pp.x + q.x) / 2.0, (pp.y + q.y) / 2.0);
    let h = r * (theta / 2.0).jcos();
    let ux = (q.x - pp.x) / c;
    let uy = (q.y - pp.y) / c;
    let s = if b > 0.0 { 1.0 } else { -1.0 };
    let cx = mid.x - uy * h * s;
    let cy = mid.y + ux * h * s;
    let mut a0 = (pp.y - cy).jatan2(pp.x - cx);
    let mut a1 = (q.y - cy).jatan2(q.x - cx);
    if b < 0.0 {
        std::mem::swap(&mut a0, &mut a1);
    }
    a0 = norm_angle(a0);
    a1 = norm_angle(a1);
    if a1 <= a0 {
        a1 += TAU;
    }
    Prim { rev: Some(b < 0.0), ..Prim::a(cx, cy, r, a0, a1) }
}

/// Bulge for an arc from its sweep (CCW positive).
pub fn sweep_to_bulge(sweep: f64) -> f64 {
    (sweep / 4.0).jtan()
}

// ---------------------------------------------------------------------------------------------------------------
// Affine transforms

pub const IDENTITY: M = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
pub fn m_translate(x: f64, y: f64) -> M {
    [1.0, 0.0, 0.0, 1.0, x, y]
}
pub fn m_scale(sx: f64, sy: f64) -> M {
    [sx, 0.0, 0.0, sy, 0.0, 0.0]
}
pub fn m_rotate(r: f64) -> M {
    let (c, s) = (r.jcos(), r.jsin());
    [c, s, -s, c, 0.0, 0.0]
}
pub fn m_mul(m: &M, n: &M) -> M {
    let [m0, m1, m2, m3, m4, m5] = *m;
    let [n0, n1, n2, n3, n4, n5] = *n;
    [m0 * n0 + m2 * n1, m1 * n0 + m3 * n1, m0 * n2 + m2 * n3, m1 * n2 + m3 * n3, m0 * n4 + m2 * n5 + m4, m1 * n4 + m3 * n5 + m5]
}
pub fn m_apply(m: &M, q: P2) -> P2 {
    p(m[0] * q.x + m[2] * q.y + m[4], m[1] * q.x + m[3] * q.y + m[5])
}
pub fn m_about(q: P2, m: &M) -> M {
    m_mul(&m_translate(q.x, q.y), &m_mul(m, &m_translate(-q.x, -q.y)))
}
/// Mirror across the line through a and b.
pub fn m_mirror(a: P2, b: P2) -> M {
    let ang = angle_of(a, b);
    let (c, s) = ((2.0 * ang).jcos(), (2.0 * ang).jsin());
    m_about(a, &[c, s, s, -c, 0.0, 0.0])
}
pub fn m_det(m: &M) -> f64 {
    m[0] * m[3] - m[1] * m[2]
}
/// Uniform scale, rotation and flip of a similar transform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sim {
    pub s: f64,
    pub rot: f64,
    pub flip: bool,
}
/// Uniform scale factor and rotation of m, or None when it shears/stretches (arcs then become ellipses).
pub fn m_similar(m: &M) -> Option<Sim> {
    let sx = js::hypot(m[0], m[1]);
    let sy = js::hypot(m[2], m[3]);
    let dot = m[0] * m[2] + m[1] * m[3];
    if (sx - sy).abs() > 1e-9 * js::max(sx, sy) || dot.abs() > 1e-9 * sx * sy {
        return None;
    }
    Some(Sim { s: sx, rot: m[1].jatan2(m[0]), flip: m_det(m) < 0.0 })
}
pub fn m_inverse(m: &M) -> Option<M> {
    let d = m_det(m);
    if d == 0.0 || d.is_nan() {
        return None;
    }
    let [a, b, c, e, f, g] = *m;
    Some([e / d, -b / d, -c / d, a / d, (c * g - e * f) / d, (b * f - a * g) / d])
}

// ---------------------------------------------------------------------------------------------------------------
// Construction lines

/// A unit direction (dx, dy), or None for a zero one.
pub fn unit_dir(dx: f64, dy: f64) -> Option<(f64, f64)> {
    let l = js::hypot(dx, dy);
    if l > EPS { Some((dx / l, dy / l)) } else { None }
}

/// Bounds {minX, minY, maxX, maxY}.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}
impl Bounds {
    pub fn to_value(&self) -> Value {
        serde_json::json!({ "minX": num(self.min_x), "minY": num(self.min_y), "maxX": num(self.max_x), "maxY": num(self.max_y) })
    }
    pub fn from_value(v: &Value) -> Bounds {
        let g = |k: &str| v.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
        Bounds { min_x: g("minX"), min_y: g("minY"), max_x: g("maxX"), max_y: g("maxY") }
    }
}

/// The part of the line q + t·d (t ≥ 0 when `ray`) inside rect → [t0, t1], or None (Liang–Barsky).
pub fn clip_infinite(q: P2, d: P2, rect: &Bounds, ray: bool) -> Option<(f64, f64)> {
    let mut t0 = if ray { 0.0 } else { f64::NEG_INFINITY };
    let mut t1 = f64::INFINITY;
    for (qq, lo, hi, v) in [(d.x, rect.min_x, rect.max_x, q.x), (d.y, rect.min_y, rect.max_y, q.y)] {
        if qq.abs() < 1e-15 {
            if v < lo || v > hi {
                return None;
            }
            continue;
        }
        let mut a = (lo - v) / qq;
        let mut b = (hi - v) / qq;
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        t0 = js::max(t0, a);
        t1 = js::min(t1, b);
        if t0 > t1 {
            return None;
        }
    }
    if t0.is_finite() && t1.is_finite() { Some((t0, t1)) } else { None }
}

// ---------------------------------------------------------------------------------------------------------------
// Entity → primitives

/// Primitives of an entity in world coordinates. `blocks` for inserts (and dimensions with imported geometry).
pub fn prims(e: &Entity, blocks: Option<&Blocks>) -> Vec<Prim> {
    prims_d(e, blocks, 0)
}

pub fn prims_d(e: &Entity, blocks: Option<&Blocks>, depth: u32) -> Vec<Prim> {
    prims_cap(e, blocks, depth, MAX_PRIMS)
}

/// Primitives per top-level entity at most. Spec gap vs 0.5: the JS has no cap, and blocks inserted inside blocks
/// (16 levels deep) multiply into billions of pieces — a hostile file then runs the JS out of memory; here the
/// outline stops at this many (no real drawing comes near it).
pub const MAX_PRIMS: usize = 2_000_000;

/// [`prims_d`] with at most `cap` primitives (nested inserts stop filling once the budget is spent).
fn prims_cap(e: &Entity, blocks: Option<&Blocks>, depth: u32, cap: usize) -> Vec<Prim> {
    if cap == 0 {
        return Vec::new();
    }
    let mut v = prims_one(e, blocks, depth, cap);
    v.truncate(cap);
    v
}

fn prims_one(e: &Entity, blocks: Option<&Blocks>, depth: u32, cap: usize) -> Vec<Prim> {
    match &e.geo {
        Geo::Line(l) => vec![Prim::l(l.x1, l.y1, l.x2, l.y2)],
        Geo::Circle(c) => vec![Prim::circle(c.cx, c.cy, c.r)],
        Geo::Arc(a) => {
            let (a0, a1) = arc_range(a.a0, a.a1);
            vec![Prim::a(a.cx, a.cy, a.r, a0, a1)]
        }
        Geo::Poly(pl) => poly_prims(&pl.pts, pl.is_closed()),
        Geo::Ellipse(_) => ellipse_prims(e),
        Geo::Point(_) | Geo::Other => Vec::new(),
        Geo::Text(t) => {
            let c = text_corners(t);
            box4(&c, true)
        }
        Geo::Hatch(h) => h.loops.iter().flat_map(|l| ring_prims(l)).collect(),
        Geo::Insert(ins) => {
            let Some(blk) = blocks.and_then(|b| b.get(&ins.block)) else { return Vec::new() };
            if depth > 16 {
                return Vec::new();
            }
            let m = insert_matrix(ins, blk);
            let mut out = Vec::new();
            'ents: for c in &blk.entities {
                for q in prims_cap(c, blocks, depth + 1, cap.saturating_sub(out.len())) {
                    for r in tx_prim(&q, &m) {
                        if out.len() >= cap {
                            break 'ents;
                        }
                        out.push(r);
                    }
                }
            }
            'atts: for t in attr_texts(ins, Some(blk), false) {
                for q in prims(&t, None) {
                    for r in tx_prim(&q, &m) {
                        if out.len() >= cap {
                            break 'atts;
                        }
                        out.push(Prim { boxed: true, ..r });
                    }
                }
            }
            out
        }
        Geo::Dim(d) => {
            let parts = dim_parts(e, d.block.as_deref(), blocks);
            if depth > 16 {
                return Vec::new();
            }
            let mut out = Vec::new();
            for c in parts.iter() {
                let boxed = matches!(c.geo, Geo::Hatch(_) | Geo::Text(_));
                for q in prims_cap(c, blocks, depth + 1, cap.saturating_sub(out.len())) {
                    out.push(if boxed { Prim { boxed: true, ..q } } else { q });
                }
            }
            out
        }
        Geo::Viewport(v) => {
            let c = viewport_corners(v.x, v.y, v.w, v.h);
            box4(&c, false)
        }
        Geo::Xline(i) | Geo::Ray(i) => {
            let s = if matches!(e.geo, Geo::Ray(_)) { 0.0 } else { -FAR };
            vec![Prim { inf: true, ..Prim::l(i.x + i.dx * s, i.y + i.dy * s, i.x + i.dx * FAR, i.y + i.dy * FAR) }]
        }
        Geo::Mleader(_) => {
            let mut out = Vec::new();
            for c in crate::leader::leader_geometry(e) {
                let boxed = matches!(c.geo, Geo::Hatch(_) | Geo::Text(_));
                for q in prims_cap(&c, blocks, depth + 1, cap.saturating_sub(out.len())) {
                    out.push(if boxed { Prim { boxed: true, ..q } } else { q });
                }
            }
            out
        }
    }
}

/// Four corners → four segments (closing back to the first).
fn box4(c: &[P2; 4], boxed: bool) -> Vec<Prim> {
    let [a, b, cc, d] = *c;
    [(a, b), (b, cc), (cc, d), (d, a)].iter().map(|(u, v)| Prim { boxed, ..Prim::l(u.x, u.y, v.x, v.y) }).collect()
}

/// What a dimension draws: its imported block while it has one, else its own geometry.
pub fn dim_parts(e: &Entity, block: Option<&str>, blocks: Option<&Blocks>) -> Vec<Entity> {
    match block.filter(|b| !b.is_empty()).and_then(|b| blocks.and_then(|m| m.get(&b.to_string()))) {
        Some(blk) => blk.entities.clone(),
        None => crate::dims::dim_geometry(e),
    }
}

/// Model → paper for a viewport: its view centre (cx, cy) lands on its centre (x, y), at scale s, turned by `tw`.
pub fn viewport_matrix(x: f64, y: f64, cx: f64, cy: f64, s: f64, tw: Option<f64>) -> M {
    let tw = js::or(tw, 0.0);
    if tw == 0.0 {
        return [s, 0.0, 0.0, s, x - cx * s, y - cy * s];
    }
    let c = tw.jcos() * s;
    let n = tw.jsin() * s;
    [c, n, -n, c, x - (c * cx - n * cy), y - (n * cx + c * cy)]
}

/// Corners of a viewport (paper units), counter-clockwise from the lower left.
pub fn viewport_corners(x: f64, y: f64, w: f64, h: f64) -> [P2; 4] {
    let hw = w / 2.0;
    let hh = h / 2.0;
    [p(x - hw, y - hh), p(x + hw, y - hh), p(x + hw, y + hh), p(x - hw, y + hh)]
}

pub fn arc_range(a0: f64, a1: f64) -> (f64, f64) {
    let a0 = norm_angle(a0);
    let mut a1 = norm_angle(a1);
    if a1 <= a0 + EPS {
        a1 += TAU;
    }
    (a0, a1)
}

pub fn poly_prims(v: &[Vertex], closed: bool) -> Vec<Prim> {
    let n = v.len();
    let mut out = Vec::new();
    if n == 0 {
        return out;
    }
    let segs = if closed { n } else { n - 1 };
    for i in 0..segs {
        let (Some(a), Some(b)) = (v.get(i), v.get((i + 1) % n)) else { continue };
        let (pp, q) = (a.p(), b.p());
        if a.bulge().abs() > 1e-12 && dist(pp, q) > EPS {
            out.push(Prim { seg: Some(i), ..bulge_arc(pp, q, a.bulge()) });
        } else {
            out.push(Prim { seg: Some(i), ..Prim::l(pp.x, pp.y, q.x, q.y) });
        }
    }
    out
}

/// A polyline entity's primitives (`polyPrims(e)`); others: none.
pub fn poly_prims_of(e: &Entity) -> Vec<Prim> {
    match &e.geo {
        Geo::Poly(pl) => poly_prims(&pl.pts, pl.is_closed()),
        _ => Vec::new(),
    }
}

fn ring_prims(flat: &[f64]) -> Vec<Prim> {
    let mut out = Vec::new();
    let mut i = 0;
    while let (Some(&x1), Some(&y1), Some(&x2), Some(&y2)) = (flat.get(i), flat.get(i + 1), flat.get(i + 2), flat.get(i + 3)) {
        out.push(Prim::l(x1, y1, x2, y2));
        i += 2;
    }
    out
}

pub fn ellipse_points(e: &Entity, steps_n: f64) -> Vec<P2> {
    let Geo::Ellipse(el) = &e.geo else { return Vec::new() };
    let ma = js::hypot(el.mx, el.my);
    let mi = ma * el.ratio;
    let rot = el.my.jatan2(el.mx);
    let (t0, t1) = if el.t1 - el.t0 >= TAU - EPS { (0.0, TAU) } else { arc_range(el.t0, el.t1) };
    let n = steps(8.0, steps_n * (t1 - t0) / TAU);
    let (c, s) = (rot.jcos(), rot.jsin());
    let mut pts = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let t = t0 + (t1 - t0) * f(i) / f(n);
        let x = ma * t.jcos();
        let y = mi * t.jsin();
        pts.push(p(el.cx + x * c - y * s, el.cy + x * s + y * c));
    }
    pts
}
fn ellipse_prims(e: &Entity) -> Vec<Prim> {
    let q = ellipse_points(e, ELLIPSE_STEPS);
    q.iter().zip(q.iter().skip(1)).map(|(a, b)| Prim::l(a.x, a.y, b.x, b.y)).collect()
}

// ---------------------------------------------------------------------------------------------------------------
// Text

/// Width of a line of a text entity's text, in drawing units (Helvetica metrics).
pub fn text_width(s: &str, t: &Text) -> f64 {
    advance(s) / 1000.0 * t.h * FONT_EM * js::or(t.wf, 1.0)
}

/// A text's layout in its own frame (insertion point at 0,0, x along the text).
#[derive(Clone, Debug, PartialEq)]
pub struct TextLayout {
    pub lines: Vec<String>,
    pub widths: Vec<f64>,
    /// {x, y, w, h}
    pub bx: [f64; 4],
    pub baselines: Vec<f64>,
}
impl TextLayout {
    pub fn to_value(&self) -> Value {
        serde_json::json!({
            "lines": self.lines,
            "widths": self.widths.iter().map(|v| num(*v)).collect::<Vec<_>>(),
            "box": { "x": num(self.bx[0]), "y": num(self.bx[1]), "w": num(self.bx[2]), "h": num(self.bx[3]) },
            "baselines": self.baselines.iter().map(|v| num(*v)).collect::<Vec<_>>(),
        })
    }
}

/// Lines (wrapped to `ww` when the text has a wrap width), each line's width and baseline, and the box. va:
/// 'baseline' (first line's baseline at 0), 'bottom', 'middle' or 'top'; ha: 'left', 'center' or 'right'.
pub fn text_layout(t: &Text) -> TextLayout {
    let mut lines: Vec<String> = t.s.split('\n').map(str::to_string).collect();
    if t.ww.is_some_and(|w| w > 0.0) {
        lines = lines.iter().flat_map(|l| wrap_line(l, t)).collect();
    }
    let widths: Vec<f64> = lines.iter().map(|l| text_width(l, t)).collect();
    let w = js::max(js::max_of(widths.iter().copied()), t.h * 0.4);
    let ls = LINE_SPACING * js::or(t.ls, 1.0);
    let h = t.h * (1.0 + (f(lines.len()) - 1.0) * ls);
    let x = match t.ha.as_deref() {
        Some("center") => -w / 2.0,
        Some("right") => -w,
        _ => 0.0,
    };
    let top = match t.va.as_deref() {
        Some("top") => 0.0,
        Some("middle") => h / 2.0,
        Some("bottom") => h,
        _ => t.h,
    };
    let baselines = (0..lines.len()).map(|k| top - t.h - f(k) * t.h * ls).collect();
    TextLayout { lines, widths, bx: [x, top - h, w, h], baselines }
}

/// `line.split(/(?<= )/)`: pieces that each end just after a space.
fn split_after_spaces(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, c) in line.char_indices() {
        if c == ' ' {
            let end = i + 1;
            if end < line.len() {
                out.push(line.get(start..end).unwrap_or(""));
                start = end;
            }
        }
    }
    out.push(line.get(start..).unwrap_or(""));
    out
}

/// Break one line into lines no wider than the wrap width (a word longer than that keeps its own line).
fn wrap_line(line: &str, t: &Text) -> Vec<String> {
    let ww = t.ww.unwrap_or(f64::NAN);
    if text_width(line, t) <= ww {
        return vec![line.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in split_after_spaces(line) {
        let joined = format!("{cur}{word}");
        if !cur.is_empty() && text_width(js::trim_end(&joined), t) > ww {
            out.push(js::trim_end(&cur).to_string());
            cur = js::trim_start(word).to_string();
        } else {
            cur.push_str(word);
        }
    }
    if !cur.is_empty() || out.is_empty() {
        out.push(js::trim_end(&cur).to_string());
    }
    out
}

/// Corners of a text's box in world coordinates.
pub fn text_corners(t: &Text) -> [P2; 4] {
    let b = text_layout(t).bx;
    let m = m_mul(&m_translate(t.x, t.y), &m_rotate(js::or(t.rot, 0.0)));
    [
        m_apply(&m, p(b[0], b[1])),
        m_apply(&m, p(b[0] + b[2], b[1])),
        m_apply(&m, p(b[0] + b[2], b[1] + b[3])),
        m_apply(&m, p(b[0], b[1] + b[3])),
    ]
}

// ---------------------------------------------------------------------------------------------------------------
// Block references

pub fn insert_matrix(e: &Insert, blk: &Block) -> M {
    let b = blk.base;
    m_mul(
        &m_translate(e.x, e.y),
        &m_mul(&m_rotate(js::or(e.rot, 0.0)), &m_mul(&m_scale(e.sx.unwrap_or(1.0), e.sy.unwrap_or(1.0)), &m_translate(-b.x, -b.y))),
    )
}

/// An attribute tag as CAD programs keep it: upper case, no spaces.
pub fn norm_tag(t: &str) -> String {
    let up = js::trim(t).to_uppercase();
    let mut out = String::new();
    let mut in_ws = false;
    for c in up.chars() {
        if js::is_ws(c) {
            if !in_ws {
                out.push('_');
            }
            in_ws = true;
        } else {
            out.push(c);
            in_ws = false;
        }
    }
    js::slice16(&out, 255)
}

/// `Object.keys(o)` order: array-index keys ascending first, then the rest in insertion order.
pub fn js_keys(o: &Obj) -> Vec<&String> {
    let index = |k: &str| -> Option<u32> {
        if k == "0" {
            return Some(0);
        }
        if k.starts_with('0') || k.is_empty() || !k.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        k.parse::<u32>().ok().filter(|n| *n != u32::MAX)
    };
    let mut idx: Vec<(u32, &String)> = o.keys().filter_map(|k| index(k).map(|n| (n, k))).collect();
    idx.sort_by_key(|(n, _)| *n);
    let mut out: Vec<&String> = idx.into_iter().map(|(_, k)| k).collect();
    out.extend(o.keys().filter(|k| index(k).is_none()));
    out
}

fn num_or(v: Option<&Value>, d: f64) -> f64 {
    match v {
        Some(Value::Number(n)) => js::or(n.as_f64(), d),
        Some(x) if js::truthy(Some(x)) => js::to_number(x),
        _ => d,
    }
}
fn str_or(v: Option<&Value>, d: &str) -> String {
    match v {
        Some(x) if js::truthy(Some(x)) => js::to_js_string(x),
        _ => d.into(),
    }
}

/// Attributes of a block reference, as text entities in the BLOCK's coordinates (put them through insertMatrix
/// like the block's own entities). Each carries `tag`. Invisible attributes and empty values are left out unless
/// `all`.
pub fn attr_texts(e: &Insert, blk: Option<&Block>, all: bool) -> Vec<Entity> {
    let mut out = Vec::new();
    let attdefs: &[Value] = blk.and_then(|b| b.attdefs.as_deref()).unwrap_or(&[]);
    if e.attrs.is_none() && attdefs.is_empty() {
        return out;
    }
    let empty = Obj::new();
    let vals = e.attrs.as_ref().unwrap_or(&empty);
    let loc = |tag: &str| e.attloc.as_ref().and_then(|a| a.get(tag)).filter(|v| js::truthy(Some(v)));
    let mut make = |tag: &Value, value: Option<&Value>, d: Option<&Value>| {
        let s = match value {
            None | Some(Value::Null) => String::new(),
            Some(v) => js::to_js_string(v),
        };
        let Some(d) = d.filter(|d| js::truthy(Some(d))) else { return };
        let invisible = js::truthy(d.get("invisible"));
        if !all && (invisible || js::trim(&s).is_empty()) {
            return;
        }
        let h = match d.get("h") {
            Some(v) if js::to_number(v) > 0.0 => js::to_number(v),
            _ => 2.5,
        };
        let layer = match d.get("layer") {
            None | Some(Value::Null) => "0".to_string(),
            Some(v) => js::to_js_string(v),
        };
        let color = d.get("color").and_then(Color::from_value).unwrap_or(Color::Null);
        let x = js::or(Some(js::to_number_opt(d.get("x"))), 0.0);
        let y = js::or(Some(js::to_number_opt(d.get("y"))), 0.0);
        let t = Text {
            x,
            y,
            h,
            s,
            rot: Some(num_or(d.get("rot"), 0.0)),
            ha: Some(str_or(d.get("ha"), "left")),
            va: Some(str_or(d.get("va"), "baseline")),
            wf: Some(num_or(d.get("wf"), 1.0)),
            ww: None,
            ls: None,
        };
        let mut ent = Entity::with(Geo::Text(t), Some(layer), Some(color));
        ent.extra.insert("tag".into(), tag.clone());
        if invisible {
            ent.extra.insert("invisible".into(), true.into());
        }
        out.push(ent);
    };
    let mut seen: Vec<String> = Vec::new();
    for d in attdefs {
        let tag_v = d.get("tag").cloned().unwrap_or(Value::Null);
        let tag = js::to_js_string(&tag_v);
        seen.push(tag.clone());
        if js::truthy(d.get("constant")) {
            make(&tag_v, d.get("def"), Some(d));
        } else if vals.contains_key(&tag) {
            make(&tag_v, vals.get(&tag), Some(loc(&tag).unwrap_or(d)));
        }
    }
    for tag in js_keys(vals) {
        if !seen.contains(tag)
            && let Some(l) = loc(tag)
        {
            make(&Value::String(tag.clone()), vals.get(tag), Some(l));
        }
    }
    out
}

/// A primitive through a transform (arcs become polylines when the transform is not similar).
pub fn tx_prim(q: &Prim, m: &M) -> Vec<Prim> {
    if q.is_l() {
        let a = m_apply(m, p(q.x1, q.y1));
        let b = m_apply(m, p(q.x2, q.y2));
        return vec![Prim::l(a.x, a.y, b.x, b.y)];
    }
    if let Some(sim) = m_similar(m) {
        let c = m_apply(m, p(q.cx, q.cy));
        if q.full {
            return vec![Prim::circle(c.x, c.y, q.r * sim.s)];
        }
        // a mirror reverses the direction of travel: the image of the old end is the new CCW start
        let s = m_apply(m, prim_point(q, if sim.flip { 1.0 } else { 0.0 }));
        let a0 = norm_angle((s.y - c.y).jatan2(s.x - c.x));
        return vec![Prim::a(c.x, c.y, q.r * sim.s, a0, a0 + (q.a1 - q.a0))];
    }
    let n = steps(8.0, 48.0 * (q.a1 - q.a0) / TAU);
    let mut out = Vec::with_capacity(n);
    let mut prev = m_apply(m, prim_point(q, 0.0));
    for i in 1..=n {
        let r = m_apply(m, prim_point(q, f(i) / f(n)));
        out.push(Prim::l(prev.x, prev.y, r.x, r.y));
        prev = r;
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------
// Primitive helpers

pub fn prim_point(q: &Prim, t: f64) -> P2 {
    if q.is_l() {
        return p(q.x1 + (q.x2 - q.x1) * t, q.y1 + (q.y2 - q.y1) * t);
    }
    let a = q.a0 + (q.a1 - q.a0) * t;
    p(q.cx + q.r * a.jcos(), q.cy + q.r * a.jsin())
}

/// Parameter of the point on q closest to x (clamped to [0, 1]).
pub fn prim_param(q: &Prim, x: P2, clamp: bool) -> f64 {
    if q.is_l() {
        let dx = q.x2 - q.x1;
        let dy = q.y2 - q.y1;
        let l = dx * dx + dy * dy;
        let t = if l < EPS * EPS { 0.0 } else { ((x.x - q.x1) * dx + (x.y - q.y1) * dy) / l };
        return if clamp { js::max(0.0, js::min(1.0, t)) } else { t };
    }
    let a = (x.y - q.cy).jatan2(x.x - q.cx);
    let sweep = q.a1 - q.a0;
    let d = norm_angle(a - q.a0);
    if d > sweep + EPS {
        if !clamp {
            return d / sweep;
        }
        let after = d - sweep;
        let before = TAU - d;
        return if after < before { 1.0 } else { 0.0 };
    }
    d / sweep
}

pub fn dist_to_prim(q: &Prim, x: P2) -> f64 {
    dist(x, prim_point(q, prim_param(q, x, true)))
}

pub fn prim_length(q: &Prim) -> f64 {
    if q.is_l() { js::hypot(q.x2 - q.x1, q.y2 - q.y1) } else { q.r * (q.a1 - q.a0) }
}

/// Intersections of two primitives (on both). `ext1` / `ext2`: treat as unbounded (line / full circle).
pub fn intersect(a: &Prim, b: &Prim, ext1: bool, ext2: bool) -> Vec<P2> {
    let pts = match (a.k, b.k) {
        (K::L, K::L) => ll_inter(a, b),
        (K::L, K::A) => lc_inter(a, b),
        (K::A, K::L) => lc_inter(b, a),
        (K::A, K::A) => cc_inter(a, b),
    };
    let tol = 1e-7;
    pts.into_iter().filter(|x| (ext1 || on_prim(a, *x, tol)) && (ext2 || on_prim(b, *x, tol))).collect()
}

fn on_prim(q: &Prim, x: P2, tol: f64) -> bool {
    if q.is_l() {
        let t = prim_param(q, x, false);
        let l = prim_length(q);
        return t >= -tol / js::max(l, EPS) - 1e-9 && t <= 1.0 + tol / js::max(l, EPS) + 1e-9;
    }
    on_sweep((x.y - q.cy).jatan2(x.x - q.cx), q.a0, q.a1, 1e-7)
}

fn ll_inter(a: &Prim, b: &Prim) -> Vec<P2> {
    let (d1x, d1y, d2x, d2y) = (a.x2 - a.x1, a.y2 - a.y1, b.x2 - b.x1, b.y2 - b.y1);
    let den = d1x * d2y - d1y * d2x;
    if den.abs() < 1e-12 * js::hypot(d1x, d1y) * js::hypot(d2x, d2y) {
        return Vec::new();
    }
    let t = ((b.x1 - a.x1) * d2y - (b.y1 - a.y1) * d2x) / den;
    vec![p(a.x1 + d1x * t, a.y1 + d1y * t)]
}

fn lc_inter(l: &Prim, c: &Prim) -> Vec<P2> {
    let (dx, dy, fx, fy) = (l.x2 - l.x1, l.y2 - l.y1, l.x1 - c.cx, l.y1 - c.cy);
    let a = dx * dx + dy * dy;
    let b = 2.0 * (fx * dx + fy * dy);
    let cc = fx * fx + fy * fy - c.r * c.r;
    if a < EPS * EPS {
        return Vec::new();
    }
    let mut disc = b * b - 4.0 * a * cc;
    if disc < 0.0 {
        if disc > -1e-9 * b * b {
            disc = 0.0;
        } else {
            return Vec::new();
        }
    }
    let s = disc.sqrt();
    let ts = if disc == 0.0 { vec![-b / (2.0 * a)] } else { vec![(-b - s) / (2.0 * a), (-b + s) / (2.0 * a)] };
    ts.into_iter().map(|t| p(l.x1 + dx * t, l.y1 + dy * t)).collect()
}

fn cc_inter(a: &Prim, b: &Prim) -> Vec<P2> {
    let d = js::hypot(b.cx - a.cx, b.cy - a.cy);
    if d < EPS || d > a.r + b.r + 1e-9 || d < (a.r - b.r).abs() - 1e-9 {
        return Vec::new();
    }
    let x = (d * d + a.r * a.r - b.r * b.r) / (2.0 * d);
    let h = js::max(0.0, a.r * a.r - x * x).sqrt();
    let ux = (b.cx - a.cx) / d;
    let uy = (b.cy - a.cy) / d;
    let px = a.cx + ux * x;
    let py = a.cy + uy * x;
    if h < 1e-12 {
        return vec![p(px, py)];
    }
    vec![p(px - uy * h, py + ux * h), p(px + uy * h, py - ux * h)]
}

// ---------------------------------------------------------------------------------------------------------------
// Small entity builders used across the kernel

pub fn line_ent(x1: f64, y1: f64, x2: f64, y2: f64) -> Geo {
    Geo::Line(Line { x1, y1, x2, y2 })
}
pub fn arc_ent(cx: f64, cy: f64, r: f64, a0: f64, a1: f64) -> Geo {
    Geo::Arc(Arc { cx, cy, r, a0, a1 })
}
/// A copy of `e` with new geometry (`{ ...e, … }`).
pub fn with_geo(e: &Entity, geo: Geo) -> Entity {
    Entity { geo, ..e.clone() }
}
