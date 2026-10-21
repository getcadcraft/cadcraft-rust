//! gsolve.js — the 2D constraint solver for CadCraft drawings: the variables are the fields of every entity a constraint
//! touches (line x1 y1 x2 y2 · circle cx cy r · arc cx cy r a0 a1 · point x y · polyline vertex i x y); a pinned field
//! (`<id>|<field>`) is held. Damped Newton with the least-norm step dx = Jᵀ(JJᵀ + μI)⁻¹(−r), a halving line search
//! and Levenberg–Marquardt growth of μ; Jacobian rows by central differences over each equation's own variables;
//! `analyze` is Gram–Schmidt over the rows in constraint order (rank, redundant constraints, degrees of freedom,
//! entities fully determined). Every sum and product runs in the JS's order, so the numbers are the JS's.
//!
//! Hostile input: iterations are capped (100 Newton steps × 12 μ tries × 20 halvings), `wrap` of a huge or non-finite
//! angle does not loop (the JS would), and every lookup is checked (a missing field reads NaN, as `undefined` does).

use std::collections::HashMap;

use cc_cad::entity::{Entity, Geo};
use cc_cad::js;
use cc_cad_geom::jsmath::JsMath;

use crate::con::{Con, Ref};

const TAU: f64 = std::f64::consts::PI * 2.0;
const PI: f64 = std::f64::consts::PI;

/// The entity kinds constraints work on (`SOLVABLE`).
pub fn solvable(e: &Entity) -> bool {
    matches!(e.geo, Geo::Line(_) | Geo::Circle(_) | Geo::Arc(_) | Geo::Point(_) | Geo::Poly(_))
}

/// `fieldsOf(e)`
pub fn fields_of(e: &Entity) -> Vec<String> {
    match &e.geo {
        Geo::Line(_) => ["x1", "y1", "x2", "y2"].iter().map(|s| s.to_string()).collect(),
        Geo::Circle(_) => ["cx", "cy", "r"].iter().map(|s| s.to_string()).collect(),
        Geo::Arc(_) => ["cx", "cy", "r", "a0", "a1"].iter().map(|s| s.to_string()).collect(),
        Geo::Point(_) => ["x", "y"].iter().map(|s| s.to_string()).collect(),
        Geo::Poly(p) => {
            let mut out = Vec::with_capacity(p.pts.len().saturating_mul(2));
            for i in 0..p.pts.len() {
                out.push(format!("{i}x"));
                out.push(format!("{i}y"));
            }
            out
        }
        _ => Vec::new(),
    }
}

/// The values of `fields_of(e)`, in order.
pub fn field_values(e: &Entity) -> Vec<f64> {
    match &e.geo {
        Geo::Line(l) => vec![l.x1, l.y1, l.x2, l.y2],
        Geo::Circle(c) => vec![c.cx, c.cy, c.r],
        Geo::Arc(a) => vec![a.cx, a.cy, a.r, a.a0, a.a1],
        Geo::Point(p) => vec![p.x, p.y],
        Geo::Poly(p) => p.pts.iter().flat_map(|v| [v.x, v.y]).collect(),
        _ => Vec::new(),
    }
}

/// `getField(e, f)`: NaN when the entity has no such field (`undefined` in the JS).
pub fn get_field(e: &Entity, f: &str) -> f64 {
    match &e.geo {
        Geo::Poly(p) => {
            let i = parse_int_prefix(f);
            let Some(i) = i else { return f64::NAN };
            p.pts.get(i).map_or(f64::NAN, |v| if f.ends_with('x') { v.x } else { v.y })
        }
        Geo::Line(l) => match f {
            "x1" => l.x1,
            "y1" => l.y1,
            "x2" => l.x2,
            "y2" => l.y2,
            _ => f64::NAN,
        },
        Geo::Circle(c) => match f {
            "cx" => c.cx,
            "cy" => c.cy,
            "r" => c.r,
            _ => f64::NAN,
        },
        Geo::Arc(a) => match f {
            "cx" => a.cx,
            "cy" => a.cy,
            "r" => a.r,
            "a0" => a.a0,
            "a1" => a.a1,
            _ => f64::NAN,
        },
        Geo::Point(p) => match f {
            "x" => p.x,
            "y" => p.y,
            _ => f64::NAN,
        },
        _ => f64::NAN,
    }
}

/// `parseInt(s, 10)`: leading white space, a sign, then digits (NaN when there are none).
pub fn parse_int(s: &str) -> f64 {
    let t = js::trim(s);
    let (neg, rest) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let d: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if d.is_empty() {
        return f64::NAN;
    }
    let v = js::str_to_number(&d);
    if neg { -v } else { v }
}
/// `parseInt(f, 10)` of a polyline field ("12x" → 12) as an index.
fn parse_int_prefix(f: &str) -> Option<usize> {
    let d: String = f.chars().take_while(char::is_ascii_digit).collect();
    d.parse().ok()
}

fn norm(a: f64) -> f64 {
    let mut a = a % TAU;
    if a < 0.0 {
        a += TAU;
    }
    a
}

/// `wrap(d)` into (−π, π] by whole turns, as the JS loops do; a value the loops could not bring back (non-finite, or
/// so big a turn does not change it) is reduced with a remainder instead of looping forever.
pub fn wrap(d: f64) -> f64 {
    if !d.is_finite() {
        return if d.is_nan() { d } else { f64::NAN };
    }
    let mut d = d;
    if d.abs() > 1.0e6 {
        d %= TAU;
    }
    while d > PI {
        d -= TAU;
    }
    while d <= -PI {
        d += TAU;
    }
    d
}

/// `withFields(e, vals)`: a copy with new field values; radii stay positive, arc angles in [0, 2π).
pub fn with_fields(e: &Entity, vals: &[(String, f64)]) -> Entity {
    let mut out = e.clone();
    let get = |f: &str| vals.iter().find(|(k, _)| k == f).map(|(_, v)| *v);
    match &mut out.geo {
        Geo::Poly(p) => {
            for (i, v) in p.pts.iter_mut().enumerate() {
                if let Some(x) = get(&format!("{i}x")) {
                    v.x = x;
                }
                if let Some(y) = get(&format!("{i}y")) {
                    v.y = y;
                }
            }
        }
        Geo::Line(l) => {
            for (f, v) in vals {
                match f.as_str() {
                    "x1" => l.x1 = *v,
                    "y1" => l.y1 = *v,
                    "x2" => l.x2 = *v,
                    "y2" => l.y2 = *v,
                    _ => {}
                }
            }
        }
        Geo::Circle(c) => {
            for (f, v) in vals {
                match f.as_str() {
                    "cx" => c.cx = *v,
                    "cy" => c.cy = *v,
                    "r" => c.r = *v,
                    _ => {}
                }
            }
            if c.r < 0.0 {
                c.r = -c.r;
            }
        }
        Geo::Arc(a) => {
            for (f, v) in vals {
                match f.as_str() {
                    "cx" => a.cx = *v,
                    "cy" => a.cy = *v,
                    "r" => a.r = *v,
                    "a0" => a.a0 = *v,
                    "a1" => a.a1 = *v,
                    _ => {}
                }
            }
            if a.r < 0.0 {
                a.r = -a.r;
                a.a0 += PI;
                a.a1 += PI;
            }
            a.a0 = norm(a.a0);
            a.a1 = norm(a.a1);
        }
        Geo::Point(p) => {
            for (f, v) in vals {
                match f.as_str() {
                    "x" => p.x = *v,
                    "y" => p.y = *v,
                    _ => {}
                }
            }
        }
        _ => {}
    }
    out
}

/// `segCount(e)`: segments of a polyline (closed ones wrap round).
pub fn seg_count(e: &Entity) -> usize {
    match &e.geo {
        Geo::Poly(p) => {
            if p.pts.len() < 2 {
                0
            } else if p.is_closed() {
                p.pts.len()
            } else {
                p.pts.len().saturating_sub(1)
            }
        }
        _ => 0,
    }
}

/// A reference key parsed: `p1 p2 mid L c R s e p * v<i> s<i>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RK {
    P1,
    P2,
    Mid,
    L,
    C,
    R,
    S,
    E,
    P,
    All,
    V(usize),
    Seg(usize),
    Other,
}

impl RK {
    pub fn parse(k: &str) -> RK {
        match k {
            "p1" => RK::P1,
            "p2" => RK::P2,
            "mid" => RK::Mid,
            "L" => RK::L,
            "c" => RK::C,
            "R" => RK::R,
            "s" => RK::S,
            "e" => RK::E,
            "p" => RK::P,
            "*" => RK::All,
            _ => {
                let (h, rest) = k.split_at(k.chars().next().map_or(0, char::len_utf8));
                if (h == "v" || h == "s") && !rest.is_empty() && rest.len() <= 6 && rest.bytes().all(|b| b.is_ascii_digit()) {
                    let i: usize = rest.parse().unwrap_or(usize::MAX);
                    if h == "v" { RK::V(i) } else { RK::Seg(i) }
                } else {
                    RK::Other
                }
            }
        }
    }
    /// `+k.slice(1)` for a polyline key
    fn index(self) -> Option<usize> {
        match self {
            RK::V(i) | RK::Seg(i) => Some(i),
            _ => None,
        }
    }
}

/// `refKind(e, k)` → 'P' | 'L' | 'R' | 'E' | None.
pub fn ref_kind(e: &Entity, k: &str) -> Option<char> {
    ref_kind_rk(e, RK::parse(k))
}
pub fn ref_kind_rk(e: &Entity, k: RK) -> Option<char> {
    if k == RK::All {
        return if solvable(e) { Some('E') } else { None };
    }
    match &e.geo {
        Geo::Line(_) => match k {
            RK::L => Some('L'),
            RK::P1 | RK::P2 | RK::Mid => Some('P'),
            _ => None,
        },
        Geo::Circle(_) => match k {
            RK::R => Some('R'),
            RK::C => Some('P'),
            _ => None,
        },
        Geo::Arc(_) => match k {
            RK::R => Some('R'),
            RK::C | RK::S | RK::E | RK::Mid => Some('P'),
            _ => None,
        },
        Geo::Point(_) => match k {
            RK::P => Some('P'),
            _ => None,
        },
        Geo::Poly(p) => match k {
            RK::V(i) => (i < p.pts.len()).then_some('P'),
            RK::Seg(i) => {
                if i >= seg_count(e) {
                    return None;
                }
                let b = p.pts.get(i).and_then(|v| v.b).filter(|b| !b.is_nan()).unwrap_or(0.0);
                Some(if b.abs() < 1e-12 { 'L' } else { 'R' })
            }
            _ => None,
        },
        _ => None,
    }
}

// ---------------------------------------------------------------- geometry over a value function

/// What kind of entity a slot holds (the shape `geo` reads).
#[derive(Clone, Debug)]
enum Shape {
    Line,
    Circle,
    Arc,
    Point,
    Poly { n: usize, bulges: Vec<f64> },
    Other,
}

fn shape_of(e: &Entity) -> Shape {
    match &e.geo {
        Geo::Line(_) => Shape::Line,
        Geo::Circle(_) => Shape::Circle,
        Geo::Arc(_) => Shape::Arc,
        Geo::Point(_) => Shape::Point,
        Geo::Poly(p) => Shape::Poly { n: p.pts.len(), bulges: p.pts.iter().map(|v| v.b.filter(|b| !b.is_nan()).unwrap_or(0.0)).collect() },
        _ => Shape::Other,
    }
}

/// One entity of the system: its shape, stored values, and each field's variable (None = held at its value).
#[derive(Clone, Debug)]
pub struct Slot {
    pub id: u64,
    shape: Shape,
    vals: Vec<f64>,
    var: Vec<Option<usize>>,
}

type P = [f64; 2];

struct Geo2<'a> {
    s: &'a Slot,
    x: &'a [f64],
}

impl Geo2<'_> {
    /// V(field i)
    fn v(&self, i: usize) -> f64 {
        match self.s.var.get(i).copied().flatten() {
            Some(k) => self.x.get(k).copied().unwrap_or(f64::NAN),
            None => self.s.vals.get(i).copied().unwrap_or(f64::NAN),
        }
    }
    fn pt(&self, i: usize) -> P {
        let b = i.saturating_mul(2);
        [self.v(b), self.v(b.saturating_add(1))]
    }
    fn n(&self) -> usize {
        if let Shape::Poly { n, .. } = &self.s.shape { *n } else { 0 }
    }
    fn bulge_circle(&self, i: usize) -> (P, f64) {
        let n = self.n().max(1);
        let p = self.pt(i);
        let q = self.pt(i.saturating_add(1) % n);
        let b = if let Shape::Poly { bulges, .. } = &self.s.shape { bulges.get(i).copied().unwrap_or(0.0) } else { 0.0 };
        let th = 4.0 * b.jatan();
        let (cx, cy) = (q[0] - p[0], q[1] - p[1]);
        let c = js::hypot(cx, cy);
        let c = if c == 0.0 || c.is_nan() { 1e-300 } else { c };
        let r = (c / (2.0 * (th / 2.0).jsin())).abs();
        let h = r * (th / 2.0).jcos() * if b > 0.0 { 1.0 } else { -1.0 };
        ([(p[0] + q[0]) / 2.0 - (cy / c) * h, (p[1] + q[1]) / 2.0 + (cx / c) * h], r)
    }
    fn point(&self, k: RK) -> P {
        match &self.s.shape {
            Shape::Line => {
                let a = [self.v(0), self.v(1)];
                let b = [self.v(2), self.v(3)];
                match k {
                    RK::P1 => a,
                    RK::P2 => b,
                    _ => [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0],
                }
            }
            Shape::Circle => [self.v(0), self.v(1)],
            Shape::Arc => {
                let (cx, cy, r, a0, a1) = (self.v(0), self.v(1), self.v(2), self.v(3), self.v(4));
                if k == RK::C {
                    return [cx, cy];
                }
                let a = match k {
                    RK::S => a0,
                    RK::E => a1,
                    _ => a0 + norm(a1 - a0) / 2.0,
                };
                [cx + r * a.jcos(), cy + r * a.jsin()]
            }
            Shape::Point => [self.v(0), self.v(1)],
            Shape::Poly { .. } => match k.index() {
                Some(i) => self.pt(i),
                None => [f64::NAN, f64::NAN],
            },
            Shape::Other => [0.0, 0.0],
        }
    }
    fn line(&self, k: RK) -> [P; 2] {
        if matches!(self.s.shape, Shape::Line) {
            return [[self.v(0), self.v(1)], [self.v(2), self.v(3)]];
        }
        let n = self.n();
        match k.index() {
            Some(i) if n > 0 => [self.pt(i), self.pt(i.saturating_add(1) % n)],
            _ => [[f64::NAN, f64::NAN], [f64::NAN, f64::NAN]],
        }
    }
    fn circle(&self, k: RK) -> (P, f64) {
        match &self.s.shape {
            Shape::Poly { .. } => match k.index() {
                Some(i) => self.bulge_circle(i),
                None => ([f64::NAN, f64::NAN], f64::NAN),
            },
            Shape::Circle | Shape::Arc => ([self.v(0), self.v(1)], self.v(2)),
            _ => ([f64::NAN, f64::NAN], f64::NAN),
        }
    }
    /// `fieldValue(g, e, f)`: one raw field through the geometry reader (Fix on a whole entity, an arc's sweep).
    fn field_value(&self, f: usize, stored: &[f64]) -> f64 {
        match &self.s.shape {
            Shape::Line => {
                let l = self.line(RK::L);
                [l[0][0], l[0][1], l[1][0], l[1][1]].get(f).copied().unwrap_or(f64::NAN)
            }
            Shape::Circle => {
                let (c, r) = self.circle(RK::R);
                [c[0], c[1], r].get(f).copied().unwrap_or(f64::NAN)
            }
            Shape::Arc => {
                let (c, r) = self.circle(RK::R);
                match f {
                    0 => c[0],
                    1 => c[1],
                    2 => r,
                    _ => {
                        let p = self.point(if f == 3 { RK::S } else { RK::E });
                        let rf = stored.get(f).copied().unwrap_or(f64::NAN);
                        rf + wrap((p[1] - c[1]).jatan2(p[0] - c[0]) - rf)
                    }
                }
            }
            Shape::Point => {
                let p = self.point(RK::P);
                if f == 0 { p[0] } else { p[1] }
            }
            Shape::Poly { .. } => {
                let p = self.point(RK::V(f / 2));
                if f.is_multiple_of(2) { p[0] } else { p[1] }
            }
            Shape::Other => 0.0,
        }
    }
}

/// `p[k]` (k is 0 or 1)
fn ix(p: P, k: usize) -> f64 {
    p.get(k).copied().unwrap_or(f64::NAN)
}
fn cross(u: P, v: P) -> f64 {
    u[0] * v[1] - u[1] * v[0]
}
fn dot(u: P, v: P) -> f64 {
    u[0] * v[0] + u[1] * v[1]
}
fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1]]
}
fn len(u: P) -> f64 {
    js::hypot(u[0], u[1])
}
fn or_tiny(v: f64) -> f64 {
    if v == 0.0 || v.is_nan() { 1e-300 } else { v }
}
/// Signed distance of p from the infinite line a→b (left positive).
fn sd(p: P, l: [P; 2]) -> f64 {
    let u = sub(l[1], l[0]);
    cross(u, sub(p, l[0])) / or_tiny(len(u))
}

// ---------------------------------------------------------------- the system

/// One equation: what it measures, the slots and keys of its references, and its variables.
#[derive(Clone, Debug)]
pub struct Eq {
    pub cid: String,
    f: F,
    refs: Vec<(usize, RK)>,
    pub deps: Vec<usize>,
}

#[derive(Clone, Debug)]
enum F {
    CoincPx,
    CoincPy,
    CoincPL,
    CoincPR,
    ConcX,
    ConcY,
    Collin(usize),
    FixPx(f64),
    FixPy(f64),
    FixField(usize, f64),
    Parallel,
    Perp,
    HvL(usize),
    HvPP(usize),
    TanL(f64),
    TanRR {
        int: bool,
        s: f64,
    },
    EqualL,
    EqualR,
    /// (which pair: 0 = the points / the first ends / the centres; 1 = the second ends), the mid / the dot equation
    SymMid(SymPair),
    SymDot(SymPair),
    SymR,
    HvDist {
        k: usize,
        s: f64,
        v: f64,
    },
    DistPP(f64),
    DistPL {
        s: f64,
        v: f64,
    },
    DistLL {
        s: f64,
        v: f64,
    },
    Radius(f64),
    Diam(f64),
    AngleArc(f64),
    AngleLL {
        s1: f64,
        s2: f64,
        s: f64,
        v: f64,
    },
}

#[derive(Clone, Copy, Debug)]
enum SymPair {
    Pts,
    Ends { sw: bool, end: usize },
    Centres,
}

/// `buildSystem(constraints, get, pinned)`
pub struct System {
    /// '<id>|<field>' → variable index, in the order they were made
    pub vars: Vec<(u64, usize)>,
    pub x: Vec<f64>,
    pub eqs: Vec<Eq>,
    pub slots: Vec<Slot>,
    /// the entities read (id → its entity), in first-use order
    pub ents: Vec<Entity>,
}

impl System {
    fn slot_of(&self, id: u64) -> Option<usize> {
        self.slots.iter().position(|s| s.id == id)
    }
    pub fn var_of(&self, id: u64, f: usize) -> Option<usize> {
        self.slot_of(id).and_then(|s| self.slots.get(s)).and_then(|s| s.var.get(f).copied().flatten())
    }
}

/// Pinned fields: (entity id, field name).
pub type Pins = std::collections::HashSet<(u64, String)>;

pub fn build_system(cs: &[Con], get: &dyn Fn(u64) -> Option<Entity>, pinned: Option<&Pins>) -> System {
    let mut slots: Vec<Slot> = Vec::new();
    let mut ents: Vec<Entity> = Vec::new();
    let mut x: Vec<f64> = Vec::new();
    let mut vars: Vec<(u64, usize)> = Vec::new();
    let mut index: HashMap<u64, usize> = HashMap::new();
    for c in cs {
        for r in c.refs.iter() {
            if index.contains_key(&r.e) {
                continue;
            }
            let Some(e) = get(r.e) else { continue };
            let names = fields_of(&e);
            let vals = field_values(&e);
            let mut var = Vec::with_capacity(names.len());
            for (i, f) in names.iter().enumerate() {
                if pinned.is_some_and(|p| p.contains(&(r.e, f.clone()))) {
                    var.push(None);
                    continue;
                }
                var.push(Some(x.len()));
                vars.push((r.e, i));
                x.push(vals.get(i).copied().unwrap_or(f64::NAN));
            }
            index.insert(r.e, slots.len());
            slots.push(Slot { id: r.e, shape: shape_of(&e), vals, var });
            ents.push(e);
        }
    }
    let mut eqs = Vec::new();
    for c in cs {
        equations_for(c, &slots, &ents, &index, &mut eqs);
    }
    System { vars, x, eqs, slots, ents }
}

fn equations_for(c: &Con, slots: &[Slot], ents: &[Entity], index: &HashMap<u64, usize>, out: &mut Vec<Eq>) {
    let mut refs: Vec<(usize, RK)> = Vec::with_capacity(c.refs.len());
    let mut kinds: Vec<char> = Vec::with_capacity(c.refs.len());
    for r in c.refs.iter() {
        let Some(&s) = index.get(&r.e) else { return };
        let Some(e) = ents.get(s) else { return };
        let rk = RK::parse(&r.k);
        let Some(k) = ref_kind_rk(e, rk) else { return };
        refs.push((s, rk));
        kinds.push(k);
    }
    let mut deps: Vec<usize> = Vec::new();
    for (s, _) in &refs {
        if let Some(sl) = slots.get(*s) {
            for k in sl.var.iter().flatten() {
                if !deps.contains(k) {
                    deps.push(*k);
                }
            }
        }
    }
    let k_of = |i: usize| kinds.get(i).copied();
    let mut push = |f: F| out.push(Eq { cid: c.id.clone(), f, refs: refs.clone(), deps: deps.clone() });
    let s = c.sign_or_1();
    match c.typ.as_str() {
        "coincident" => match k_of(1) {
            Some('P') => {
                push(F::CoincPx);
                push(F::CoincPy);
            }
            Some('L') => push(F::CoincPL),
            _ => push(F::CoincPR),
        },
        "concentric" => {
            push(F::ConcX);
            push(F::ConcY);
        }
        "collinear" => {
            push(F::Collin(0));
            push(F::Collin(1));
        }
        "fix" => {
            if k_of(0) == Some('P') {
                let v = c.value_pair().unwrap_or([0.0, 0.0]);
                push(F::FixPx(v[0]));
                push(F::FixPy(v[1]));
            } else if let Some((s0, _)) = refs.first()
                && let Some(e) = ents.get(*s0)
            {
                for (i, f) in fields_of(e).iter().enumerate() {
                    if let Some(v) = c.fixed_field(f) {
                        push(F::FixField(i, v));
                    }
                }
            }
        }
        "parallel" => push(F::Parallel),
        "perpendicular" => push(F::Perp),
        "horizontal" | "vertical" => {
            let k = if c.typ == "horizontal" { 1 } else { 0 };
            if k_of(0) == Some('L') { push(F::HvL(k)) } else { push(F::HvPP(k)) }
        }
        "tangent" => {
            if k_of(0) == Some('L') {
                push(F::TanL(s))
            } else {
                push(F::TanRR { int: c.kind_int(), s })
            }
        }
        "equal" => {
            if k_of(0) == Some('L') {
                push(F::EqualL)
            } else {
                push(F::EqualR)
            }
        }
        "symmetric" => {
            match k_of(0) {
                Some('P') => {
                    push(F::SymMid(SymPair::Pts));
                    push(F::SymDot(SymPair::Pts));
                }
                Some('L') => {
                    let sw = c.pair() == Some(1);
                    for end in 0..2 {
                        push(F::SymMid(SymPair::Ends { sw, end }));
                        push(F::SymDot(SymPair::Ends { sw, end }));
                    }
                }
                _ => {
                    push(F::SymMid(SymPair::Centres));
                    push(F::SymDot(SymPair::Centres));
                }
            }
            if k_of(0) == Some('R') {
                push(F::SymR);
            }
        }
        "hdist" | "vdist" => push(F::HvDist { k: if c.typ == "hdist" { 0 } else { 1 }, s, v: c.value_num() }),
        "dist" => {
            if k_of(1) == Some('P') {
                push(F::DistPP(c.value_num()))
            } else if k_of(0) == Some('P') {
                push(F::DistPL { s, v: c.value_num() })
            } else {
                push(F::DistLL { s, v: c.value_num() })
            }
        }
        "radius" => push(F::Radius(c.value_num())),
        "diameter" => push(F::Diam(c.value_num())),
        "angle" => {
            if k_of(0) == Some('R') {
                push(F::AngleArc(c.value_num()))
            } else {
                let [s1, s2] = c.dirs().unwrap_or([1.0, 1.0]);
                push(F::AngleLL { s1, s2, s, v: c.value_num() })
            }
        }
        _ => {}
    }
}

impl System {
    /// q.f() at the current `x`.
    pub fn eval(&self, q: &Eq, x: &[f64]) -> f64 {
        let g = |i: usize| q.refs.get(i).and_then(|(s, k)| self.slots.get(*s).map(|sl| (Geo2 { s: sl, x }, *k)));
        let pt = |i: usize| g(i).map_or([f64::NAN, f64::NAN], |(g, k)| g.point(k));
        let ln = |i: usize| g(i).map_or([[f64::NAN; 2]; 2], |(g, k)| g.line(k));
        let ci = |i: usize| g(i).map_or(([f64::NAN; 2], f64::NAN), |(g, k)| g.circle(k));
        let dir = |i: usize| {
            let l = ln(i);
            sub(l[1], l[0])
        };
        match &q.f {
            F::CoincPx => pt(0)[0] - pt(1)[0],
            F::CoincPy => pt(0)[1] - pt(1)[1],
            F::CoincPL => sd(pt(0), ln(1)),
            F::CoincPR => {
                let (c, r) = ci(1);
                len(sub(pt(0), c)) - r
            }
            F::ConcX => ci(0).0[0] - ci(1).0[0],
            F::ConcY => ci(0).0[1] - ci(1).0[1],
            F::Collin(e) => sd(ln(1).get(*e).copied().unwrap_or([f64::NAN; 2]), ln(0)),
            F::FixPx(v) => pt(0)[0] - v,
            F::FixPy(v) => pt(0)[1] - v,
            F::FixField(f, v) => match g(0) {
                Some((g, _)) => g.field_value(*f, &g.s.vals) - v,
                None => f64::NAN,
            },
            F::Parallel => {
                let (u, v) = (dir(0), dir(1));
                cross(u, v) / or_tiny(len(u) * len(v))
            }
            F::Perp => {
                let (u, v) = (dir(0), dir(1));
                dot(u, v) / or_tiny(len(u) * len(v))
            }
            F::HvL(k) => {
                let l = ln(0);
                ix(l[1], *k) - ix(l[0], *k)
            }
            F::HvPP(k) => ix(pt(1), *k) - ix(pt(0), *k),
            F::TanL(s) => {
                let (c, r) = ci(1);
                s * sd(c, ln(0)) - r
            }
            F::TanRR { int, s } => {
                let (a, b) = (ci(0), ci(1));
                let d = len(sub(b.0, a.0));
                if *int { d - s * (a.1 - b.1) } else { d - (a.1 + b.1) }
            }
            F::EqualL => len(dir(0)) - len(dir(1)),
            F::EqualR => ci(0).1 - ci(1).1,
            F::SymMid(p) | F::SymDot(p) => {
                let (a, b) = match p {
                    SymPair::Pts => (pt(0), pt(1)),
                    SymPair::Ends { sw, end } => {
                        let (l0, l1) = (ln(0), ln(1));
                        let j = if *sw { 1usize.saturating_sub(*end) } else { *end };
                        (l0.get(*end).copied().unwrap_or([f64::NAN; 2]), l1.get(j).copied().unwrap_or([f64::NAN; 2]))
                    }
                    SymPair::Centres => (ci(0).0, ci(1).0),
                };
                if matches!(q.f, F::SymMid(_)) {
                    sd([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0], ln(2))
                } else {
                    let u = dir(2);
                    dot(sub(b, a), u) / or_tiny(len(u))
                }
            }
            F::SymR => ci(0).1 - ci(1).1,
            F::HvDist { k, s, v } => s * (ix(pt(1), *k) - ix(pt(0), *k)) - v,
            F::DistPP(v) => len(sub(pt(1), pt(0))) - v,
            F::DistPL { s, v } => s * sd(pt(0), ln(1)) - v,
            F::DistLL { s, v } => s * sd(ln(1)[0], ln(0)) - v,
            F::Radius(v) => ci(0).1 - v,
            F::Diam(v) => 2.0 * ci(0).1 - v,
            F::AngleArc(v) => match g(0) {
                Some((g, _)) => wrap(g.field_value(4, &g.s.vals) - g.field_value(3, &g.s.vals) - v * PI / 180.0),
                None => f64::NAN,
            },
            F::AngleLL { s1, s2, s, v } => {
                let u0 = dir(0);
                let v0 = dir(1);
                let u = [u0[0] * s1, u0[1] * s1];
                let w = [v0[0] * s2, v0[1] * s2];
                wrap(cross(u, w).jatan2(dot(u, w)) - s * v * PI / 180.0)
            }
        }
    }
    fn residuals(&self, x: &[f64]) -> Vec<f64> {
        self.eqs.iter().map(|q| self.eval(q, x)).collect()
    }
}

// ---------------------------------------------------------------- numerics (as partcraft/dev/solver.js)

fn norm2(r: &[f64]) -> f64 {
    r.iter().fold(0.0, |s, v| s + v * v)
}

/// A sparse Jacobian row: (variable, derivative) in the order the variables were tried.
type Row = Vec<(usize, f64)>;

fn jacobian(sys: &System, x: &mut [f64]) -> Vec<Row> {
    sys.eqs
        .iter()
        .map(|q| {
            let mut row: Row = Vec::new();
            for &k in &q.deps {
                if row.iter().any(|(j, _)| *j == k) {
                    continue;
                }
                let Some(x0) = x.get(k).copied() else { continue };
                let h = 1e-6 * js::max(1.0, x0.abs());
                if let Some(v) = x.get_mut(k) {
                    *v = x0 + h;
                }
                let fp = sys.eval(q, x);
                if let Some(v) = x.get_mut(k) {
                    *v = x0 - h;
                }
                let fm = sys.eval(q, x);
                if let Some(v) = x.get_mut(k) {
                    *v = x0;
                }
                let d = (fp - fm) / (2.0 * h);
                if d != 0.0 && d.is_finite() {
                    row.push((k, d));
                }
            }
            row
        })
        .collect()
}

fn sparse_dot(a: &Row, b: &Row) -> f64 {
    let (a, b) = if a.len() > b.len() { (b, a) } else { (a, b) };
    let mut s = 0.0;
    for (k, v) in a {
        if let Some((_, w)) = b.iter().find(|(j, _)| j == k) {
            s += v * w;
        }
    }
    s
}

fn at(m: &[Vec<f64>], i: usize, j: usize) -> f64 {
    m.get(i).and_then(|r| r.get(j)).copied().unwrap_or(f64::NAN)
}
fn set(m: &mut [Vec<f64>], i: usize, j: usize, v: f64) {
    if let Some(c) = m.get_mut(i).and_then(|r| r.get_mut(j)) {
        *c = v;
    }
}

fn chol_solve(a: &[Vec<f64>], b: &[f64]) -> Option<Vec<f64>> {
    let n = b.len();
    let mut l: Vec<Vec<f64>> = a.to_vec();
    for j in 0..n {
        let mut s = at(&l, j, j);
        for k in 0..j {
            s -= at(&l, j, k) * at(&l, j, k);
        }
        if !(s > 0.0) {
            return None;
        }
        let d = s.sqrt();
        set(&mut l, j, j, d);
        for i in j.saturating_add(1)..n {
            let mut t = at(&l, i, j);
            for k in 0..j {
                t -= at(&l, i, k) * at(&l, j, k);
            }
            set(&mut l, i, j, t / d);
        }
    }
    let mut y = vec![0.0; n];
    for i in 0..n {
        let mut s = b.get(i).copied().unwrap_or(f64::NAN);
        for k in 0..i {
            s -= at(&l, i, k) * y.get(k).copied().unwrap_or(f64::NAN);
        }
        if let Some(v) = y.get_mut(i) {
            *v = s / at(&l, i, i);
        }
    }
    let mut z = vec![0.0; n];
    for i in (0..n).rev() {
        let mut s = y.get(i).copied().unwrap_or(f64::NAN);
        for k in i.saturating_add(1)..n {
            s -= at(&l, k, i) * z.get(k).copied().unwrap_or(f64::NAN);
        }
        if let Some(v) = z.get_mut(i) {
            *v = s / at(&l, i, i);
        }
    }
    Some(z)
}

/// The most rows a system may have before it is refused (each Newton step builds an m × m matrix).
pub const MAX_ROWS: usize = 4000;

fn newton(sys: &System, x: &mut Vec<f64>, tol: f64, max_iter: usize) -> (bool, usize) {
    let mut r = sys.residuals(x);
    let mut f = norm2(&r);
    let mut mu = 1e-12f64;
    let met = |r: &[f64]| r.iter().all(|v| v.abs() <= tol);
    let mut it = 0usize;
    if sys.eqs.len() > MAX_ROWS {
        return (false, 0);
    }
    while it < max_iter {
        if met(&r) {
            return (true, it);
        }
        if x.is_empty() {
            break;
        }
        let j = jacobian(sys, x);
        let m = j.len();
        let mut a: Vec<Vec<f64>> = vec![vec![0.0; m]; m];
        let mut scale = 0.0f64;
        for i in 0..m {
            for jj in 0..=i {
                let v = match (j.get(i), j.get(jj)) {
                    (Some(p), Some(q)) => sparse_dot(p, q),
                    _ => f64::NAN,
                };
                set(&mut a, i, jj, v);
                if jj < i {
                    set(&mut a, jj, i, v);
                }
            }
            scale = js::max(scale, at(&a, i, i));
        }
        let mut improved = false;
        let mut tries = 0;
        while tries < 12 && !improved {
            tries += 1;
            let bm: Vec<Vec<f64>> = a
                .iter()
                .enumerate()
                .map(|(i, row)| {
                    row.iter().enumerate().map(|(k, v)| if i == k { v + mu * js::max(scale, 1e-12) + 1e-300 } else { *v }).collect()
                })
                .collect();
            let rhs: Vec<f64> = r.iter().map(|v| -v).collect();
            let Some(y) = chol_solve(&bm, &rhs) else {
                mu *= 100.0;
                continue;
            };
            let mut dx = vec![0.0; x.len()];
            for (i, row) in j.iter().enumerate() {
                let yi = y.get(i).copied().unwrap_or(f64::NAN);
                for (k, v) in row {
                    if let Some(d) = dx.get_mut(*k) {
                        *d += v * yi;
                    }
                }
            }
            let x0 = x.clone();
            let mut alpha = 1.0;
            for _ in 0..20 {
                for (k, v) in x.iter_mut().enumerate() {
                    *v = x0.get(k).copied().unwrap_or(f64::NAN) + alpha * dx.get(k).copied().unwrap_or(f64::NAN);
                }
                let r2 = sys.residuals(x);
                let f2 = norm2(&r2);
                if f2 < f {
                    r = r2;
                    f = f2;
                    improved = true;
                    break;
                }
                alpha /= 2.0;
            }
            if !improved {
                x.clone_from(&x0);
                mu *= 100.0;
                if mu > 1e6 {
                    break;
                }
            } else {
                mu = js::max(1e-12, mu / 10.0);
            }
        }
        if !improved {
            return (met(&r), it);
        }
        it += 1;
    }
    (met(&r), it)
}

/// `extentOf(ents)`: the size of the geometry (for tolerances).
fn extent_of(ents: &[Entity]) -> f64 {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut grow = |x: f64, y: f64| {
        if x.is_finite() && y.is_finite() {
            min_x = js::min(min_x, x);
            max_x = js::max(max_x, x);
            min_y = js::min(min_y, y);
            max_y = js::max(max_y, y);
        }
    };
    for e in ents {
        match &e.geo {
            Geo::Line(l) => {
                grow(l.x1, l.y1);
                grow(l.x2, l.y2);
            }
            Geo::Circle(c) => {
                grow(c.cx - c.r, c.cy - c.r);
                grow(c.cx + c.r, c.cy + c.r);
            }
            Geo::Arc(c) => {
                grow(c.cx - c.r, c.cy - c.r);
                grow(c.cx + c.r, c.cy + c.r);
            }
            Geo::Point(p) => grow(p.x, p.y),
            Geo::Poly(p) => {
                for v in &p.pts {
                    grow(v.x, v.y);
                }
            }
            _ => {}
        }
    }
    if min_x.is_finite() {
        [1.0, max_x - min_x, max_y - min_y, min_x.abs(), max_x.abs(), min_y.abs(), max_y.abs()].into_iter().fold(f64::NEG_INFINITY, js::max)
    } else {
        1.0
    }
}
pub fn tolerance(ents: &[Entity]) -> f64 {
    1e-9 * extent_of(ents)
}

/// What `solveSystem` returns.
pub struct Solved {
    pub ok: bool,
    /// changed entities (id → new entity), in first-use order
    pub updates: Vec<(u64, Entity)>,
    pub iterations: usize,
    pub residual: f64,
    pub worst: Vec<String>,
}

/// `solveSystem(constraints, get, { pinned, maxIter })`: nothing is mutated; changed entities come back.
pub fn solve_system(cs: &[Con], get: &dyn Fn(u64) -> Option<Entity>, pinned: Option<&Pins>, max_iter: usize) -> Solved {
    let mut sys = build_system(cs, get, pinned);
    let tol = tolerance(&sys.ents);
    let mut x = std::mem::take(&mut sys.x);
    let (ok, it) = newton(&sys, &mut x, tol, max_iter);
    let mut updates = Vec::new();
    if ok {
        for (si, e) in sys.ents.iter().enumerate() {
            let Some(slot) = sys.slots.get(si) else { continue };
            let names = fields_of(e);
            let mut vals: Vec<(String, f64)> = Vec::new();
            let mut moved = false;
            for (i, f) in names.iter().enumerate() {
                let Some(k) = slot.var.get(i).copied().flatten() else { continue };
                let v = x.get(k).copied().unwrap_or(f64::NAN);
                let was = slot.vals.get(i).copied().unwrap_or(f64::NAN);
                vals.push((f.clone(), v));
                if (v - was).abs() > 1e-12 * js::max(1.0, was.abs()) {
                    moved = true;
                }
            }
            if moved {
                updates.push((slot.id, with_fields(e, &vals)));
            }
        }
    }
    let resid = sys.residuals(&x);
    let worst = worst_of(&sys.eqs, &resid, tol);
    Solved { ok, updates, iterations: it, residual: norm2(&resid).sqrt(), worst }
}

fn worst_of(eqs: &[Eq], r: &[f64], tol: f64) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (i, v) in r.iter().enumerate() {
        if !(v.abs() <= tol * 100.0)
            && let Some(q) = eqs.get(i)
            && !out.contains(&q.cid)
        {
            out.push(q.cid.clone());
        }
    }
    out
}

/// `residualOk(constraints, get)`: every equation met at the current geometry?
pub fn residual_ok(cs: &[Con], get: &dyn Fn(u64) -> Option<Entity>) -> bool {
    let sys = build_system(cs, get, None);
    let tol = tolerance(&sys.ents) * 1000.0;
    sys.eqs.iter().all(|q| sys.eval(q, &sys.x).abs() <= tol)
}

/// What `analyze` returns.
#[derive(Clone, Debug, Default)]
pub struct Analysis {
    pub dof: usize,
    pub rank: usize,
    pub vars: usize,
    pub redundant: Vec<String>,
    pub conflicts: Vec<String>,
    pub determined: Vec<u64>,
}

pub fn analyze(cs: &[Con], get: &dyn Fn(u64) -> Option<Entity>) -> Analysis {
    let mut sys = build_system(cs, get, None);
    let tol = tolerance(&sys.ents);
    let mut x = std::mem::take(&mut sys.x);
    let j = jacobian(&sys, &mut x);
    let n = x.len();
    let mut q: Vec<Vec<f64>> = Vec::new();
    let mut redundant: Vec<String> = Vec::new();
    let mut conflicts: Vec<String> = Vec::new();
    let add = |l: &mut Vec<String>, c: &str| {
        if !l.iter().any(|x| x == c) {
            l.push(c.to_string());
        }
    };
    // a system too big for the dense Gram–Schmidt: analysed as having no rank (the counts say so)
    let dense_ok = n.saturating_mul(sys.eqs.len()) <= 4_000_000;
    for (i, eq) in sys.eqs.iter().enumerate() {
        if sys.eval(eq, &x).abs() > tol * 1000.0 {
            add(&mut conflicts, &eq.cid);
        }
        if !dense_ok {
            continue;
        }
        let mut v = vec![0.0f64; n];
        let mut vn = 0.0;
        for (k, d) in j.get(i).map_or(&[][..], Vec::as_slice) {
            if let Some(c) = v.get_mut(*k) {
                *c = *d;
            }
            vn += d * d;
        }
        vn = vn.sqrt();
        if vn < 1e-12 {
            add(&mut redundant, &eq.cid);
            continue;
        }
        for c in v.iter_mut() {
            *c /= vn;
        }
        for _ in 0..2 {
            for b in &q {
                let mut d = 0.0;
                for k in 0..n {
                    d += b.get(k).copied().unwrap_or(0.0) * v.get(k).copied().unwrap_or(0.0);
                }
                if d != 0.0 {
                    for k in 0..n {
                        if let Some(c) = v.get_mut(k) {
                            *c -= d * b.get(k).copied().unwrap_or(0.0);
                        }
                    }
                }
            }
        }
        let mut wn = 0.0;
        for c in &v {
            wn += c * c;
        }
        wn = wn.sqrt();
        if wn < 1e-7 {
            add(&mut redundant, &eq.cid);
            continue;
        }
        for c in v.iter_mut() {
            *c /= wn;
        }
        q.push(v);
    }
    let mut fixed = vec![0.0f64; n];
    for b in &q {
        for k in 0..n {
            if let Some(c) = fixed.get_mut(k) {
                *c += b.get(k).copied().unwrap_or(0.0) * b.get(k).copied().unwrap_or(0.0);
            }
        }
    }
    let mut determined = Vec::new();
    for (si, slot) in sys.slots.iter().enumerate() {
        let _ = si;
        if slot.var.iter().all(|k| match k {
            None => true,
            Some(k) => fixed.get(*k).copied().unwrap_or(0.0) > 1.0 - 1e-6,
        }) {
            determined.push(slot.id);
        }
    }
    Analysis { dof: n.saturating_sub(q.len()), rank: q.len(), vars: n, redundant, conflicts, determined }
}

/// `components(constraints)`: constraints in groups that share entities (union–find over the entity ids; groups in
/// the order their root is first met, as the JS Map keeps them).
pub fn components(cs: &[Con]) -> Vec<Vec<Con>> {
    components_idx(cs).into_iter().map(|g| g.into_iter().filter_map(|i| cs.get(i).cloned()).collect()).collect()
}

/// [`components`] as indexes into `cs` (nothing copied).
pub fn components_idx(cs: &[Con]) -> Vec<Vec<usize>> {
    type IdMap<V> = HashMap<u64, V, cc_cad::omap::IdBuild>;
    let mut parent: IdMap<u64> = IdMap::default();
    fn find(parent: &mut IdMap<u64>, a: u64) -> u64 {
        let mut a = a;
        let mut guard = 0usize;
        while let Some(&p) = parent.get(&a) {
            if p == a || guard > 1_000_000 {
                break;
            }
            let gp = parent.get(&p).copied().unwrap_or(p);
            parent.insert(a, gp);
            a = gp;
            guard += 1;
        }
        a
    }
    for c in cs {
        for r in c.refs.iter() {
            parent.entry(r.e).or_insert(r.e);
        }
    }
    for c in cs {
        let Some(first) = c.refs.first() else { continue };
        for r in c.refs.iter().skip(1) {
            let (ra, rb) = (find(&mut parent, first.e), find(&mut parent, r.e));
            if ra != rb {
                parent.insert(ra, rb);
            }
        }
    }
    let mut keys: IdMap<usize> = IdMap::default();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (i, c) in cs.iter().enumerate() {
        let Some(first) = c.refs.first() else { continue };
        let k = find(&mut parent, first.e);
        match keys.get(&k) {
            Some(&g) => {
                if let Some(v) = groups.get_mut(g) {
                    v.push(i);
                }
            }
            None => {
                keys.insert(k, groups.len());
                groups.push(vec![i]);
            }
        }
    }
    groups
}

/// `refPoint(e, k)` (current geometry).
pub fn ref_point(e: &Entity, k: &str) -> Option<[f64; 2]> {
    let rk = RK::parse(k);
    let kind = ref_kind_rk(e, rk)?;
    let slot = Slot { id: 0, shape: shape_of(e), vals: field_values(e), var: vec![None; field_values(e).len()] };
    let g = Geo2 { s: &slot, x: &[] };
    match kind {
        'P' => Some(g.point(rk)),
        'L' => {
            let l = g.line(rk);
            Some([(l[0][0] + l[1][0]) / 2.0, (l[0][1] + l[1][1]) / 2.0])
        }
        'R' => Some(g.circle(rk).0),
        _ => None,
    }
}
/// `refLine(e, k)` → [x1, y1, x2, y2]
pub fn ref_line(e: &Entity, k: &str) -> [f64; 4] {
    let slot = Slot { id: 0, shape: shape_of(e), vals: field_values(e), var: vec![None; field_values(e).len()] };
    let l = Geo2 { s: &slot, x: &[] }.line(RK::parse(k));
    [l[0][0], l[0][1], l[1][0], l[1][1]]
}
/// `refCircle(e, k)` → [cx, cy, r]
pub fn ref_circle(e: &Entity, k: &str) -> [f64; 3] {
    let slot = Slot { id: 0, shape: shape_of(e), vals: field_values(e), var: vec![None; field_values(e).len()] };
    let (c, r) = Geo2 { s: &slot, x: &[] }.circle(RK::parse(k));
    [c[0], c[1], r]
}

#[allow(dead_code)]
fn _unused(_: Ref) {}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_cad::entity::{Geo, Line};

    fn line(id: u64, x1: f64, y1: f64, x2: f64, y2: f64) -> Entity {
        let mut e = Entity::new(Geo::Line(Line { x1, y1, x2, y2 }));
        e.id = Some(id);
        e
    }

    #[test]
    fn horizontal_and_perpendicular_solve() {
        let ents = [line(1, 0.0, 0.0, 10.0, 1.0), line(2, 10.0, 1.0, 11.0, 10.0)];
        let get = |id: u64| ents.iter().find(|e| e.id == Some(id)).cloned();
        let h = Con::new("k1", "horizontal", vec![Ref { e: 1, k: "L".into() }]);
        let p = Con::new("k2", "perpendicular", vec![Ref { e: 1, k: "L".into() }, Ref { e: 2, k: "L".into() }]);
        let r = solve_system(&[h.clone(), p.clone()], &get, None, 100);
        assert!(r.ok);
        let now = |id: u64| r.updates.iter().find(|(i, _)| *i == id).map(|(_, e)| e.clone()).or_else(|| get(id));
        let (Some(a), Some(b)) = (now(1), now(2)) else { panic!("no lines") };
        let (Geo::Line(a), Geo::Line(b)) = (&a.geo, &b.geo) else { panic!("not lines") };
        assert!((a.y2 - a.y1).abs() < 1e-9 && (b.x2 - b.x1).abs() < 1e-6);
        let an = analyze(&[h, p], &get);
        assert_eq!((an.vars, an.rank), (8, 2));
    }

    #[test]
    fn huge_angles_do_not_loop() {
        assert!(wrap(1e300).is_finite());
        assert!(wrap(f64::INFINITY).is_nan());
        assert!((wrap(3.0 * std::f64::consts::PI) - std::f64::consts::PI).abs() < 1e-12);
    }
}
