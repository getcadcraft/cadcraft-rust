//! solver.js — damped least-norm Newton (`dx = Jᵀ(JJᵀ + μI)⁻¹(−r)`, halving line search, μ grown when a step does not
//! help) over the residual equations of sketch.js's `equations()`, plus the rank analysis (Gram–Schmidt over the rows in
//! the order the relations were made: rank, redundant relations, degrees of freedom, determined entities).
//!
//! The equations are compiled once per solve: every point / radius / spline-angle reference is resolved to a variable
//! index or the stored value (what the JS closures read). Iterations are capped (100, 60 while dragging; 12 μ tries ×
//! 20 halvings per iteration). Spec gap: a file whose relation refs are stored out of signature order makes the JS
//! closures read `undefined` fields (some throw a TypeError); here they read NaN, so the solve fails and the geometry
//! stays as it was.

use crate::js;
use crate::jv;
use crate::omap::OMap;
use crate::sketch::{self as sk_, Ent, ORIGIN};
use crate::spline::{self as sp, P2};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
enum Pr {
    V(usize, usize),
    C(f64, f64),
}
#[derive(Clone, Debug)]
enum Rr {
    V(usize),
    C(f64),
    Arc(Pr, Pr),
}
#[derive(Clone, Debug)]
enum Ar {
    V(usize),
    C(Option<f64>),
}
#[derive(Clone, Debug)]
struct Lr(Pr, Pr);
#[derive(Clone, Debug)]
struct Spl {
    control: bool,
    fit: bool,
    closed: bool,
    pts: Vec<Pr>,
    a0: Ar,
    a1: Ar,
    /// the stored end angles (`splineCurve` reads `e.a0` / `e.a1`, not the variables)
    a0s: Option<f64>,
    a1s: Option<f64>,
}

#[derive(Clone, Debug)]
enum Res {
    ArcR(Pr, Pr, Pr),
    Dx(Pr, Pr),
    Dy(Pr, Pr),
    PL(Pr, Lr),
    PR(Pr, Pr, Rr),
    LineK(Lr, usize),
    PPk(Pr, Pr, usize),
    Par(Lr, Lr),
    Perp(Lr, Lr),
    TanLS(Lr, Spl, char),
    TanSS(Spl, char, Spl, char),
    TanRS(Spl, char, Pr, Pr, Rr),
    TanLRj(Lr, Pr, Rr, Pr),
    TanRRj(Pr, Pr, Pr, Rr, Rr),
    TanLR(f64, Pr, Lr, Rr),
    TanRR(Pr, Pr, Rr, Rr, bool, f64),
    EqLL(Lr, Lr),
    EqRR(Rr, Rr),
    MidX(Pr, Lr),
    MidY(Pr, Lr),
    FixX(Pr, f64),
    FixY(Pr, f64),
    FixR(Rr, f64),
    DistPP(Pr, Pr, f64),
    DistL(Lr, f64),
    DistPL(f64, Pr, Lr, f64),
    DistLL(f64, Lr, Lr, f64),
    DistK(f64, Pr, Pr, usize, f64),
    Sym1(Pr, Pr, Lr),
    Sym2(Pr, Pr, Lr),
    OffRR(f64, Rr, Rr, f64),
    Radius(Rr, f64),
    Diameter(Rr, f64),
    Angle(Lr, Lr, f64, f64),
}

/// The most equations a solve takes on: Newton builds m × m matrices (three of them), so 2000 equations stay near
/// 100 MB. Spec gap: a bigger sketch (only hand-made files have one; the JS would allocate gigabytes) is not solved —
/// the geometry stays as it was and the result says so (`ok: false`).
pub const MAX_EQUATIONS: usize = 2000;
/// The largest equations × variables the rank analysis takes on (its basis holds a vector of all variables per rank).
pub const MAX_ANALYSIS: usize = 10_000_000;

/// One residual: its constraint id, the variables it depends on, the residual.
#[derive(Clone, Debug)]
pub struct Eq {
    pub cid: String,
    pub deps: Vec<usize>,
    r: Res,
}

#[inline]
fn xv(x: &[f64], i: usize) -> f64 {
    x.get(i).copied().unwrap_or(f64::NAN)
}
fn p(x: &[f64], q: &Pr) -> P2 {
    match q {
        Pr::V(i, j) => [xv(x, *i), xv(x, *j)],
        Pr::C(a, b) => [*a, *b],
    }
}
fn rad(x: &[f64], r: &Rr) -> f64 {
    match r {
        Rr::V(i) => xv(x, *i),
        Rr::C(v) => *v,
        Rr::Arc(c, s) => {
            let (c, s) = (p(x, c), p(x, s));
            js::hypot2(s[0] - c[0], s[1] - c[1])
        }
    }
}
fn dir(x: &[f64], l: &Lr) -> P2 {
    let (a, b) = (p(x, &l.0), p(x, &l.1));
    [b[0] - a[0], b[1] - a[1]]
}
fn sd(x: &[f64], q: P2, l: &Lr) -> f64 {
    sk_::signed_dist(q, [p(x, &l.0), p(x, &l.1)])
}
fn or_tiny(v: f64) -> f64 {
    if v == 0.0 || v.is_nan() { 1e-300 } else { v }
}
fn hyp(u: P2) -> f64 {
    js::hypot2(u[0], u[1])
}
fn cross_n(u: P2, v: P2) -> f64 {
    (u[0] * v[1] - u[1] * v[0]) / or_tiny(hyp(u) * hyp(v))
}

fn tangent(x: &[f64], s: &Spl, end: char) -> P2 {
    let key = if end == 's' { &s.a0 } else { &s.a1 };
    let ang = match key {
        Ar::V(i) => Some(xv(x, *i)),
        Ar::C(v) => *v,
    };
    if s.fit
        && let Some(a) = ang.filter(|a| a.is_finite())
    {
        return [js::cos(a), js::sin(a)];
    }
    let q: Vec<P2> = s.pts.iter().map(|r| p(x, r)).collect();
    let n = q.len();
    let g = |i: usize| q.get(i).copied().unwrap_or([f64::NAN; 2]);
    if s.control {
        let d = if end == 's' {
            [g(1)[0] - g(0)[0], g(1)[1] - g(0)[1]]
        } else {
            [g(n.saturating_sub(1))[0] - g(n.saturating_sub(2))[0], g(n.saturating_sub(1))[1] - g(n.saturating_sub(2))[1]]
        };
        let l = or_tiny(hyp(d));
        return [d[0] / l, d[1] / l];
    }
    let a0 = s.a0s.filter(|a| a.is_finite());
    let a1 = s.a1s.filter(|a| a.is_finite());
    let Ok(c) = sp::interp_fit(&q, s.closed, a0, a1) else { return [f64::NAN; 2] };
    let dom = sp::domain(&c);
    let d = sp::derivs_at(&c, if end == 's' { dom[0] } else { dom[1] }, 1);
    let tt = d.get(1).copied().unwrap_or([f64::NAN; 2]);
    let l = or_tiny(hyp(tt));
    [tt[0] / l, tt[1] / l]
}

impl Eq {
    /// The residual at x.
    pub fn f(&self, x: &[f64]) -> f64 {
        match &self.r {
            Res::ArcR(c, s, e) => {
                let (c, s, q) = (p(x, c), p(x, s), p(x, e));
                js::hypot2(q[0] - c[0], q[1] - c[1]) - js::hypot2(s[0] - c[0], s[1] - c[1])
            }
            Res::Dx(a, b) => p(x, a)[0] - p(x, b)[0],
            Res::Dy(a, b) => p(x, a)[1] - p(x, b)[1],
            Res::PL(q, l) => sd(x, p(x, q), l),
            Res::PR(q, c, r) => {
                let (q, ce) = (p(x, q), p(x, c));
                js::hypot2(q[0] - ce[0], q[1] - ce[1]) - rad(x, r)
            }
            Res::LineK(l, k) => {
                let (a, b) = (p(x, &l.0), p(x, &l.1));
                b.get(*k).copied().unwrap_or(f64::NAN) - a.get(*k).copied().unwrap_or(f64::NAN)
            }
            Res::PPk(a, b, k) => p(x, b).get(*k).copied().unwrap_or(f64::NAN) - p(x, a).get(*k).copied().unwrap_or(f64::NAN),
            Res::Par(l1, l2) => cross_n(dir(x, l1), dir(x, l2)),
            Res::Perp(l1, l2) => {
                let (u, v) = (dir(x, l1), dir(x, l2));
                (u[0] * v[0] + u[1] * v[1]) / or_tiny(hyp(u) * hyp(v))
            }
            Res::TanLS(l, s, end) => cross_n(dir(x, l), tangent(x, s, *end)),
            Res::TanSS(a, ea, b, eb) => cross_n(tangent(x, a, *ea), tangent(x, b, *eb)),
            Res::TanRS(b, eb, ce, q, ra) => {
                let tt = tangent(x, b, *eb);
                let (ce, q) = (p(x, ce), p(x, q));
                (tt[0] * (q[0] - ce[0]) + tt[1] * (q[1] - ce[1])) / or_tiny(rad(x, ra))
            }
            Res::TanLRj(l, ce, r, j) => {
                let u = dir(x, l);
                let (ce, q) = (p(x, ce), p(x, j));
                (u[0] * (q[0] - ce[0]) + u[1] * (q[1] - ce[1])) / or_tiny(hyp(u) * rad(x, r))
            }
            Res::TanRRj(j, ca, cb, ra, rb) => {
                let (q, a, b) = (p(x, j), p(x, ca), p(x, cb));
                ((q[0] - a[0]) * (q[1] - b[1]) - (q[1] - a[1]) * (q[0] - b[0])) / or_tiny(rad(x, ra) * rad(x, rb))
            }
            Res::TanLR(sign, ce, l, r) => sign * sd(x, p(x, ce), l) - rad(x, r),
            Res::TanRR(ca, cb, ra, rb, int, sign) => {
                let (ca, cb) = (p(x, ca), p(x, cb));
                let d = js::hypot2(cb[0] - ca[0], cb[1] - ca[1]);
                if *int { d - sign * (rad(x, ra) - rad(x, rb)) } else { d - (rad(x, ra) + rad(x, rb)) }
            }
            Res::EqLL(a, b) => hyp(dir(x, a)) - hyp(dir(x, b)),
            Res::EqRR(a, b) => rad(x, a) - rad(x, b),
            Res::MidX(q, l) => {
                let (a, b) = (p(x, &l.0), p(x, &l.1));
                p(x, q)[0] - (a[0] + b[0]) / 2.0
            }
            Res::MidY(q, l) => {
                let (a, b) = (p(x, &l.0), p(x, &l.1));
                p(x, q)[1] - (a[1] + b[1]) / 2.0
            }
            Res::FixX(q, v) => p(x, q)[0] - v,
            Res::FixY(q, v) => p(x, q)[1] - v,
            Res::FixR(r, v) => rad(x, r) - v,
            Res::DistPP(a, b, v) => {
                let (a, b) = (p(x, a), p(x, b));
                js::hypot2(b[0] - a[0], b[1] - a[1]) - v
            }
            Res::DistL(l, v) => hyp(dir(x, l)) - v,
            Res::DistPL(sign, q, l, v) => sign * sd(x, p(x, q), l) - v,
            Res::DistLL(sign, l0, l1, v) => sign * sd(x, p(x, &l1.0), l0) - v,
            Res::DistK(sign, a, b, k, v) => {
                let (a, b) = (p(x, a), p(x, b));
                sign * (b.get(*k).copied().unwrap_or(f64::NAN) - a.get(*k).copied().unwrap_or(f64::NAN)) - v
            }
            Res::Sym1(a, b, l) => {
                let (a, b) = (p(x, a), p(x, b));
                sd(x, [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0], l)
            }
            Res::Sym2(a, b, l) => {
                let (a, b) = (p(x, a), p(x, b));
                let u = dir(x, l);
                ((b[0] - a[0]) * u[0] + (b[1] - a[1]) * u[1]) / or_tiny(hyp(u))
            }
            Res::OffRR(sign, ra, rb, v) => sign * (rad(x, rb) - rad(x, ra)) - v,
            Res::Radius(r, v) => rad(x, r) - v,
            Res::Diameter(r, v) => 2.0 * rad(x, r) - v,
            Res::Angle(l1, l2, sign, v) => {
                let (u, w) = (dir(x, l1), dir(x, l2));
                let pi = std::f64::consts::PI;
                let mut d = js::atan2(u[0] * w[1] - u[1] * w[0], u[0] * w[0] + u[1] * w[1]) - sign * v * pi / 180.0;
                let mut guard = 0u32;
                while d > pi && guard < 10_000 {
                    d -= 2.0 * pi;
                    guard = guard.saturating_add(1);
                }
                while d <= -pi && guard < 20_000 {
                    d += 2.0 * pi;
                    guard = guard.saturating_add(1);
                }
                d
            }
        }
    }
}

/// `vars`: point id → [ix, iy], circle id → ir, '<spline id>:a0' / ':a1' → i.
#[derive(Clone, Debug, Default)]
pub struct Vars {
    pts: HashMap<String, (usize, usize)>,
    one: HashMap<String, usize>,
}

fn build_vars(sk: &Value, pinned: &HashSet<String>) -> (Vars, Vec<f64>) {
    let mut v = Vars::default();
    let mut x = Vec::new();
    for e in sk_::entities(sk) {
        let en = Ent::E(e);
        if en.ty() == "point" && !pinned.contains(en.id()) {
            v.pts.insert(en.id().to_string(), (x.len(), x.len().saturating_add(1)));
            x.push(en.num("x"));
            x.push(en.num("y"));
        }
    }
    for e in sk_::entities(sk) {
        let en = Ent::E(e);
        if en.ty() == "circle" && !pinned.contains(en.id()) {
            v.one.insert(en.id().to_string(), x.len());
            x.push(en.num("r"));
        }
        if en.ty() == "spline" {
            for k in ["a0", "a1"] {
                if let Some(a) = jv::finite(en.val(k)) {
                    v.one.insert(format!("{}:{k}", en.id()), x.len());
                    x.push(a);
                }
            }
        }
    }
    (v, x)
}

struct Build<'a> {
    ents: HashMap<&'a str, &'a Value>,
    vars: &'a Vars,
}

impl<'a> Build<'a> {
    fn ent(&self, id: &str) -> Option<Ent<'a>> {
        self.ents.get(id).map(|e| Ent::E(e))
    }
    fn pr(&self, id: &str) -> Pr {
        if id == ORIGIN {
            return Pr::C(0.0, 0.0);
        }
        if let Some(&(i, j)) = self.vars.pts.get(id) {
            return Pr::V(i, j);
        }
        match self.ents.get(id) {
            Some(e) => Pr::C(jv::n(e, "x"), jv::n(e, "y")),
            None => Pr::C(f64::NAN, f64::NAN),
        }
    }
    fn deps_p(&self, id: &str) -> Vec<usize> {
        self.vars.pts.get(id).map_or_else(Vec::new, |&(i, j)| vec![i, j])
    }
    fn deps_e(&self, id: &str) -> Vec<usize> {
        let Some(e) = self.ents.get(id).filter(|_| id != ORIGIN) else { return self.deps_p(id) };
        let en = Ent::E(e);
        if en.ty() == "point" {
            return self.deps_p(id);
        }
        let mut d: Vec<usize> = sk_::points_of(&en).iter().flat_map(|q| self.deps_p(q)).collect();
        if en.ty() == "circle"
            && let Some(&i) = self.vars.one.get(id)
        {
            d.push(i);
        }
        if en.ty() == "spline" {
            for k in [":a0", ":a1"] {
                if let Some(&i) = self.vars.one.get(&format!("{id}{k}")) {
                    d.push(i);
                }
            }
        }
        d
    }
    fn rr(&self, id: &str) -> Rr {
        let Some(e) = self.ent(id) else { return Rr::C(f64::NAN) };
        if e.ty() == "circle" {
            return match self.vars.one.get(id) {
                Some(&i) => Rr::V(i),
                None => Rr::C(e.num("r")),
            };
        }
        Rr::Arc(self.pr(e.str("c")), self.pr(e.str("s")))
    }
    fn lr(&self, id: &str) -> Lr {
        match self.ent(id) {
            Some(l) => Lr(self.pr(l.str("p1")), self.pr(l.str("p2"))),
            None => Lr(Pr::C(f64::NAN, f64::NAN), Pr::C(f64::NAN, f64::NAN)),
        }
    }
    fn spl(&self, id: &str) -> Spl {
        let e = self.ent(id);
        let pts: Vec<Pr> = e.map(|e| sk_::points_of(&e)).unwrap_or_default().iter().map(|q| self.pr(q)).collect();
        let ang = |k: &str| match self.vars.one.get(&format!("{id}:{k}")) {
            Some(&i) => Ar::V(i),
            None => Ar::C(e.and_then(|e| jv::as_num(e.val(k)))),
        };
        Spl {
            control: e.is_some_and(|e| e.str("kind") == "control"),
            fit: e.is_some_and(|e| e.str("kind") == "fit"),
            closed: e.is_some_and(|e| jv::truthy(e.val("closed"))),
            pts,
            a0: ang("a0"),
            a1: ang("a1"),
            a0s: e.and_then(|e| jv::as_num(e.val("a0"))),
            a1s: e.and_then(|e| jv::as_num(e.val("a1"))),
        }
    }
}

/// `equations(sk, x, vars)`.
fn equations(sk: &Value, vars: &Vars) -> Vec<Eq> {
    let mut ents: HashMap<&str, &Value> = HashMap::new();
    for e in sk_::entities(sk) {
        ents.entry(jv::s(e, "id")).or_insert(e);
    }
    // `new Map(entities.map(e => [e.id, e]))` keeps the last entity of a repeated id
    for e in sk_::entities(sk) {
        ents.insert(jv::s(e, "id"), e);
    }
    let b = Build { ents, vars };
    let mut out: Vec<Eq> = Vec::new();
    let mut push = |cid: &str, deps: &[usize], r: Res| out.push(Eq { cid: cid.to_string(), deps: deps.to_vec(), r });
    for e in sk_::entities(sk) {
        let en = Ent::E(e);
        if en.ty() != "arc" {
            continue;
        }
        let deps = b.deps_e(en.id());
        push(&format!("arc:{}", en.id()), &deps, Res::ArcR(b.pr(en.str("c")), b.pr(en.str("s")), b.pr(en.str("e"))));
    }
    for c in sk_::constraints(sk) {
        let ty = jv::s(c, "type");
        let refs: Vec<String> = jv::arr(c.get("refs")).iter().map(|v| js::str_of(Some(v))).collect();
        let Some((sig, _)) = sk_::match_sig(sk, ty, &refs).ok().flatten() else { continue };
        let r = |i: usize| refs.get(i).map_or("", String::as_str);
        let mut deps: Vec<usize> = Vec::new();
        for id in &refs {
            for d in b.deps_e(id) {
                if !deps.contains(&d) {
                    deps.push(d);
                }
            }
        }
        let cid = jv::s(c, "id").to_string();
        let sign = match jv::as_num(c.get("sign")) {
            Some(s) if s != 0.0 && !s.is_nan() => s,
            _ => 1.0,
        };
        let value = jv::n(c, "value");
        let ent_c = |id: &str| b.ent(id).map(|e| b.pr(e.str("c"))).unwrap_or(Pr::C(f64::NAN, f64::NAN));
        match ty {
            "coincident" => {
                if sig == "PP" {
                    push(&cid, &deps, Res::Dx(b.pr(r(0)), b.pr(r(1))));
                    push(&cid, &deps, Res::Dy(b.pr(r(0)), b.pr(r(1))));
                } else if sig == "PL" {
                    push(&cid, &deps, Res::PL(b.pr(r(0)), b.lr(r(1))));
                } else {
                    push(&cid, &deps, Res::PR(b.pr(r(0)), ent_c(r(1)), b.rr(r(1))));
                }
            }
            "horizontal" | "vertical" => {
                let k = usize::from(ty == "horizontal");
                if sig == "L" {
                    push(&cid, &deps, Res::LineK(b.lr(r(0)), k));
                } else {
                    push(&cid, &deps, Res::PPk(b.pr(r(0)), b.pr(r(1)), k));
                }
            }
            "parallel" => push(&cid, &deps, Res::Par(b.lr(r(0)), b.lr(r(1)))),
            "perpendicular" => push(&cid, &deps, Res::Perp(b.lr(r(0)), b.lr(r(1)))),
            "tangent" => {
                let (ea, eb) = (b.ent(r(0)), b.ent(r(1)));
                if sig.contains('S') {
                    let (Some(ea), Some(eb)) = (ea, eb) else { continue };
                    let Some(meet) = sk_::end_meet(sk, &ea, &eb) else { continue };
                    let end_of = |e: &Ent, pid: &str| {
                        let pts = sk_::points_of(e);
                        if pts.first().map(String::as_str) == Some(pid) { 's' } else { 'e' }
                    };
                    if sig == "LS" {
                        push(&cid, &deps, Res::TanLS(b.lr(r(0)), b.spl(r(1)), end_of(&eb, &meet.1)));
                    } else if sig == "SS" {
                        push(&cid, &deps, Res::TanSS(b.spl(r(0)), end_of(&ea, &meet.0), b.spl(r(1)), end_of(&eb, &meet.1)));
                    } else {
                        push(&cid, &deps, Res::TanRS(b.spl(r(1)), end_of(&eb, &meet.1), ent_c(r(0)), b.pr(&meet.1), b.rr(r(0))));
                    }
                    continue;
                }
                let j = match (ea, eb) {
                    (Some(a), Some(bb)) => sk_::shared_end(sk, &a, &bb),
                    _ => None,
                };
                if let Some(j) = &j
                    && sig == "LR"
                {
                    push(&cid, &deps, Res::TanLRj(b.lr(r(0)), ent_c(r(1)), b.rr(r(1)), b.pr(j)));
                    continue;
                }
                if let Some(j) = &j {
                    push(&cid, &deps, Res::TanRRj(b.pr(j), ent_c(r(0)), ent_c(r(1)), b.rr(r(0)), b.rr(r(1))));
                    continue;
                }
                if sig == "LR" {
                    push(&cid, &deps, Res::TanLR(sign, ent_c(r(1)), b.lr(r(0)), b.rr(r(1))));
                } else {
                    let int = jv::is_str(c.get("kind"), "int");
                    push(&cid, &deps, Res::TanRR(ent_c(r(0)), ent_c(r(1)), b.rr(r(0)), b.rr(r(1)), int, sign));
                }
            }
            "equal" => {
                if sig == "LL" {
                    push(&cid, &deps, Res::EqLL(b.lr(r(0)), b.lr(r(1))));
                } else {
                    push(&cid, &deps, Res::EqRR(b.rr(r(0)), b.rr(r(1))));
                }
            }
            "concentric" => {
                let ca = if sig == "PR" { b.pr(r(0)) } else { ent_c(r(0)) };
                let cb = ent_c(r(1));
                push(&cid, &deps, Res::Dx(ca.clone(), cb.clone()));
                push(&cid, &deps, Res::Dy(ca, cb));
            }
            "midpoint" => {
                push(&cid, &deps, Res::MidX(b.pr(r(0)), b.lr(r(1))));
                push(&cid, &deps, Res::MidY(b.pr(r(0)), b.lr(r(1))));
            }
            "fix" => {
                let e = b.ent(r(0)).or_else(|| sk_::by_id(sk, r(0)));
                let Some(e) = e else { continue };
                let v: Vec<f64> = if jv::truthy(c.get("value")) {
                    jv::arr(c.get("value")).iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect()
                } else {
                    Vec::new()
                };
                let vv = |i: usize| v.get(i).copied().unwrap_or(f64::NAN);
                let pts = if e.ty() == "point" { vec![e.id().to_string()] } else { sk_::points_of(&e) };
                for (i, pid) in pts.iter().enumerate() {
                    push(&cid, &deps, Res::FixX(b.pr(pid), vv(i.saturating_mul(2))));
                    push(&cid, &deps, Res::FixY(b.pr(pid), vv(i.saturating_mul(2).saturating_add(1))));
                }
                if e.ty() == "circle" {
                    push(&cid, &deps, Res::FixR(b.rr(e.id()), vv(2)));
                }
            }
            "distance" => match sig.as_str() {
                "PP" => push(&cid, &deps, Res::DistPP(b.pr(r(0)), b.pr(r(1)), value)),
                "L" => push(&cid, &deps, Res::DistL(b.lr(r(0)), value)),
                "PL" => push(&cid, &deps, Res::DistPL(sign, b.pr(r(0)), b.lr(r(1)), value)),
                _ => push(&cid, &deps, Res::DistLL(sign, b.lr(r(0)), b.lr(r(1)), value)),
            },
            "hdistance" | "vdistance" => {
                let k = usize::from(ty != "hdistance");
                let (a, bb) = if sig == "L" {
                    let l = b.lr(r(0));
                    (l.0, l.1)
                } else {
                    (b.pr(r(0)), b.pr(r(1)))
                };
                push(&cid, &deps, Res::DistK(sign, a, bb, k, value));
            }
            "symmetric" => {
                push(&cid, &deps, Res::Sym1(b.pr(r(0)), b.pr(r(1)), b.lr(r(2))));
                push(&cid, &deps, Res::Sym2(b.pr(r(0)), b.pr(r(1)), b.lr(r(2))));
            }
            "offset" | "offsetlink" => {
                let master = if ty == "offset" {
                    Some(c)
                } else {
                    let of = c.get("of");
                    sk_::constraints(sk).iter().find(|k| jv::strict_eq(k.get("id"), of) && jv::s(k, "type") == "offset")
                };
                let Some(master) = master else { continue };
                let mv = jv::n(master, "value");
                if sig == "LL" {
                    push(&cid, &deps, Res::Par(b.lr(r(0)), b.lr(r(1))));
                    push(&cid, &deps, Res::DistLL(sign, b.lr(r(0)), b.lr(r(1)), mv));
                } else {
                    let (ca, cb) = (ent_c(r(0)), ent_c(r(1)));
                    push(&cid, &deps, Res::Dx(ca.clone(), cb.clone()));
                    push(&cid, &deps, Res::Dy(ca, cb));
                    push(&cid, &deps, Res::OffRR(sign, b.rr(r(0)), b.rr(r(1)), mv));
                }
            }
            "radius" => push(&cid, &deps, Res::Radius(b.rr(r(0)), value)),
            "diameter" => push(&cid, &deps, Res::Diameter(b.rr(r(0)), value)),
            "angle" => push(&cid, &deps, Res::Angle(b.lr(r(0)), b.lr(r(1)), sign, value)),
            _ => {}
        }
    }
    out
}

fn residuals(eqs: &[Eq], x: &[f64]) -> Vec<f64> {
    eqs.iter().map(|q| q.f(x)).collect()
}
fn norm2(r: &[f64]) -> f64 {
    r.iter().fold(0.0, |s, v| s + v * v)
}

/// A sparse Jacobian row: (variable, ∂f/∂x) in insertion order.
type Row = Vec<(usize, f64)>;

fn jacobian(eqs: &[Eq], x: &mut [f64]) -> Vec<Row> {
    eqs.iter()
        .map(|q| {
            let mut row: Row = Vec::new();
            for &k in &q.deps {
                if row.iter().any(|(i, _)| *i == k) {
                    continue;
                }
                let x0 = xv(x, k);
                let h = 1e-6 * js::max(1.0, x0.abs());
                if let Some(s) = x.get_mut(k) {
                    *s = x0 + h;
                }
                let fp = q.f(x);
                if let Some(s) = x.get_mut(k) {
                    *s = x0 - h;
                }
                let fm = q.f(x);
                if let Some(s) = x.get_mut(k) {
                    *s = x0;
                }
                let d = (fp - fm) / (2.0 * h);
                if d != 0.0 {
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
        if let Some((_, w)) = b.iter().find(|(i, _)| i == k) {
            s += v * w;
        }
    }
    s
}

fn chol_solve(aa: &[Vec<f64>], b: &[f64]) -> Option<Vec<f64>> {
    let n = b.len();
    let mut l: Vec<Vec<f64>> = aa.to_vec();
    let g = |m: &Vec<Vec<f64>>, i: usize, j: usize| m.get(i).and_then(|r| r.get(j)).copied().unwrap_or(f64::NAN);
    for j in 0..n {
        let mut s = g(&l, j, j);
        for k in 0..j {
            s -= g(&l, j, k) * g(&l, j, k);
        }
        if !(s > 0.0) {
            return None;
        }
        let d = s.sqrt();
        if let Some(x) = l.get_mut(j).and_then(|r| r.get_mut(j)) {
            *x = d;
        }
        for i in j.saturating_add(1)..n {
            let mut tt = g(&l, i, j);
            for k in 0..j {
                tt -= g(&l, i, k) * g(&l, j, k);
            }
            if let Some(x) = l.get_mut(i).and_then(|r| r.get_mut(j)) {
                *x = tt / d;
            }
        }
    }
    let mut y = vec![0.0; n];
    for i in 0..n {
        let mut s = b.get(i).copied().unwrap_or(f64::NAN);
        for k in 0..i {
            s -= g(&l, i, k) * y.get(k).copied().unwrap_or(f64::NAN);
        }
        if let Some(v) = y.get_mut(i) {
            *v = s / g(&l, i, i);
        }
    }
    let mut z = vec![0.0; n];
    for i in (0..n).rev() {
        let mut s = y.get(i).copied().unwrap_or(f64::NAN);
        for k in i.saturating_add(1)..n {
            s -= g(&l, k, i) * z.get(k).copied().unwrap_or(f64::NAN);
        }
        if let Some(v) = z.get_mut(i) {
            *v = s / g(&l, i, i);
        }
    }
    Some(z)
}

fn newton(eqs: &[Eq], x: &mut Vec<f64>, tol: f64, max_iter: usize) -> (bool, usize) {
    if eqs.len() > MAX_EQUATIONS {
        return (false, 0);
    }
    let mut r = residuals(eqs, x);
    let mut f = norm2(&r);
    let mut mu: f64 = 1e-12;
    let mut it = 0usize;
    let all_ok = |r: &[f64]| r.iter().all(|v| v.abs() <= tol);
    while it < max_iter {
        if all_ok(&r) {
            return (true, it);
        }
        let jm = jacobian(eqs, x);
        let m = jm.len();
        let mut aa: Vec<Vec<f64>> = vec![vec![0.0; m]; m];
        let mut scale: f64 = 0.0;
        for i in 0..m {
            for j in 0..=i {
                let (Some(ri), Some(rj)) = (jm.get(i), jm.get(j)) else { continue };
                let v = sparse_dot(ri, rj);
                if let Some(c) = aa.get_mut(i).and_then(|row| row.get_mut(j)) {
                    *c = v;
                }
                if j < i
                    && let Some(c) = aa.get_mut(j).and_then(|row| row.get_mut(i))
                {
                    *c = v;
                }
            }
            scale = js::max(scale, aa.get(i).and_then(|row| row.get(i)).copied().unwrap_or(f64::NAN));
        }
        let mut improved = false;
        let mut tries: u32 = 0;
        while tries < 12 && !improved {
            tries = tries.saturating_add(1);
            let bb: Vec<Vec<f64>> = aa
                .iter()
                .enumerate()
                .map(|(i, row)| {
                    row.iter().enumerate().map(|(j, v)| if i == j { v + mu * js::max(scale, 1e-12) + 1e-300 } else { *v }).collect()
                })
                .collect();
            let neg: Vec<f64> = r.iter().map(|v| -v).collect();
            let Some(y) = chol_solve(&bb, &neg) else {
                mu *= 100.0;
                continue;
            };
            let mut dx = vec![0.0; x.len()];
            for (i, row) in jm.iter().enumerate() {
                let yi = y.get(i).copied().unwrap_or(f64::NAN);
                for (k, v) in row {
                    if let Some(d) = dx.get_mut(*k) {
                        *d += v * yi;
                    }
                }
            }
            let x0 = x.clone();
            let mut alpha = 1.0;
            for _ls in 0..20 {
                for (k, xk) in x.iter_mut().enumerate() {
                    *xk = x0.get(k).copied().unwrap_or(f64::NAN) + alpha * dx.get(k).copied().unwrap_or(f64::NAN);
                }
                let r2 = residuals(eqs, x);
                let f2 = norm2(&r2);
                if f2 < f {
                    r = r2;
                    f = f2;
                    improved = true;
                    break;
                }
                alpha /= 2.0;
            }
            if improved {
                mu = js::max(1e-12, mu / 10.0);
            } else {
                x.clone_from(&x0);
                mu *= 100.0;
                if mu > 1e6 {
                    break;
                }
            }
        }
        if !improved {
            return (all_ok(&r), it);
        }
        it = it.saturating_add(1);
    }
    (all_ok(&r), it)
}

/// `analyze(sk, tolScale)`'s result.
#[derive(Clone, Debug, Default)]
pub struct Analysis {
    pub dof: usize,
    pub rank: usize,
    pub vars: usize,
    pub redundant: Vec<String>,
    pub conflicts: Vec<String>,
    pub determined: Vec<String>,
}

/// `analyze(sk, tolScale)`.
pub fn analyze(sk: &Value, tol_scale: Option<f64>) -> Analysis {
    let (vars, mut x) = build_vars(sk, &HashSet::new());
    let eqs = equations(sk, &vars);
    let tol = 1e-8 * tol_scale.unwrap_or_else(|| js::max(1.0, sk_::extent(sk)));
    let n = x.len();
    if eqs.len().saturating_mul(n.max(1)) > MAX_ANALYSIS {
        return Analysis { dof: n, rank: 0, vars: n, redundant: Vec::new(), conflicts: Vec::new(), determined: vec![ORIGIN.to_string()] };
    }
    let jm = jacobian(&eqs, &mut x);
    let mut qs: Vec<Vec<f64>> = Vec::new();
    let mut redundant: Vec<String> = Vec::new();
    let mut conflicts: Vec<String> = Vec::new();
    let add = |l: &mut Vec<String>, s: &str| {
        if !l.iter().any(|x| x == s) {
            l.push(s.to_string());
        }
    };
    for (i, q) in eqs.iter().enumerate() {
        let r = q.f(&x);
        if r.abs() > tol * 100.0 {
            add(&mut conflicts, &q.cid);
        }
        let mut v = vec![0.0; n];
        let mut vn = 0.0;
        for (k, d) in jm.get(i).map_or(&[][..], Vec::as_slice) {
            if let Some(s) = v.get_mut(*k) {
                *s = *d;
            }
            vn += d * d;
        }
        vn = f64::sqrt(vn);
        if vn < 1e-12 {
            add(&mut redundant, &q.cid);
            continue;
        }
        for s in &mut v {
            *s /= vn;
        }
        for _pass in 0..2 {
            for b in &qs {
                let mut d = 0.0;
                for (bk, vk) in b.iter().zip(v.iter()) {
                    d += bk * vk;
                }
                if d != 0.0 {
                    for (vk, bk) in v.iter_mut().zip(b.iter()) {
                        *vk -= d * bk;
                    }
                }
            }
        }
        let mut wn = 0.0;
        for vk in &v {
            wn += vk * vk;
        }
        wn = f64::sqrt(wn);
        if wn < 1e-7 {
            add(&mut redundant, &q.cid);
            continue;
        }
        for s in &mut v {
            *s /= wn;
        }
        qs.push(v);
    }
    let mut fixed = vec![0.0; n];
    for b in &qs {
        for (f, bk) in fixed.iter_mut().zip(b.iter()) {
            *f += bk * bk;
        }
    }
    let det = |k: usize| fixed.get(k).is_some_and(|f| *f > 1.0 - 1e-6);
    let mut determined = vec![ORIGIN.to_string()];
    let pt_det = |id: &str| id == ORIGIN || vars.pts.get(id).is_some_and(|&(i, j)| det(i) && det(j));
    for e in sk_::entities(sk) {
        let en = Ent::E(e);
        let mut ok = sk_::points_of(&en).iter().all(|q| pt_det(q));
        if en.ty() == "circle" {
            ok = ok && vars.one.get(en.id()).is_some_and(|&i| det(i));
        }
        if en.ty() == "spline" {
            for k in [":a0", ":a1"] {
                if let Some(&i) = vars.one.get(&format!("{}{k}", en.id())) {
                    ok = ok && det(i);
                }
            }
        }
        if ok && !determined.iter().any(|d| d == en.id()) {
            determined.push(en.id().to_string());
        }
    }
    let strip = |l: Vec<String>| l.into_iter().filter(|c| !c.starts_with("arc:")).collect::<Vec<_>>();
    Analysis {
        dof: n.saturating_sub(qs.len()),
        rank: qs.len(),
        vars: n,
        redundant: strip(redundant),
        conflicts: strip(conflicts),
        determined,
    }
}

/// `solveSketch(sk, opts)`'s result.
#[derive(Clone, Debug, Default)]
pub struct Solved {
    pub ok: bool,
    pub dragged: bool,
    pub status: &'static str,
    pub dof: usize,
    pub conflicts: Vec<String>,
    pub redundant: Vec<String>,
    pub determined: Vec<String>,
    pub iterations: usize,
}
impl Solved {
    pub fn to_json(&self) -> Value {
        jv::obj(vec![
            ("ok", self.ok.into()),
            ("dragged", self.dragged.into()),
            ("status", self.status.into()),
            ("dof", jv::unum(self.dof)),
            ("conflicts", jv::strs(&self.conflicts)),
            ("redundant", jv::strs(&self.redundant)),
            ("determined", jv::strs(&self.determined)),
            ("iterations", jv::unum(self.iterations)),
        ])
    }
}

fn write_back(sk: &mut Value, vars: &Vars, x: &[f64]) {
    let Some(ents) = sk.get_mut("entities").and_then(Value::as_array_mut) else { return };
    for e in ents.iter_mut() {
        let id = jv::s(e, "id").to_string();
        match jv::s(e, "type") {
            "point" => {
                if let Some(&(i, j)) = vars.pts.get(&id) {
                    jv::set(e, "x", jv::num(xv(x, i)));
                    jv::set(e, "y", jv::num(xv(x, j)));
                }
            }
            "circle" => {
                if let Some(&i) = vars.one.get(&id) {
                    jv::set(e, "r", jv::num(xv(x, i)));
                }
            }
            _ => {}
        }
    }
    for e in ents.iter_mut() {
        if jv::s(e, "type") != "spline" {
            continue;
        }
        let id = jv::s(e, "id").to_string();
        for k in ["a0", "a1"] {
            if let Some(&i) = vars.one.get(&format!("{id}:{k}")) {
                let a = xv(x, i);
                jv::set(e, k, jv::num(js::atan2(js::sin(a), js::cos(a))));
            }
        }
    }
}

/// `solveSketch(sk, { pin, pinRadius })`.
pub fn solve_sketch(sk: &mut Value, pin: Option<&OMap<P2>>, pin_radius: Option<&OMap<f64>>) -> Solved {
    #[derive(Clone)]
    enum Snap {
        P(Option<Value>, Option<Value>),
        C(Option<Value>),
        S(Option<Value>, Option<Value>),
        N,
    }
    let snap: Vec<Snap> = sk_::entities(sk)
        .iter()
        .map(|e| match jv::s(e, "type") {
            "point" => Snap::P(e.get("x").cloned(), e.get("y").cloned()),
            "circle" => Snap::C(e.get("r").cloned()),
            "spline" => Snap::S(e.get("a0").cloned(), e.get("a1").cloned()),
            _ => Snap::N,
        })
        .collect();
    let restore = |sk: &mut Value| {
        let Some(ents) = sk.get_mut("entities").and_then(Value::as_array_mut) else { return };
        for (e, s) in ents.iter_mut().zip(snap.iter()) {
            match s {
                Snap::P(x, y) => {
                    jv::set(e, "x", x.clone().unwrap_or(Value::Null));
                    jv::set(e, "y", y.clone().unwrap_or(Value::Null));
                }
                Snap::C(r) => jv::set(e, "r", r.clone().unwrap_or(Value::Null)),
                Snap::S(a0, a1) => {
                    if let Some(v) = a0 {
                        jv::set(e, "a0", v.clone());
                    }
                    if let Some(v) = a1 {
                        jv::set(e, "a1", v.clone());
                    }
                }
                Snap::N => {}
            }
        }
    };
    let tol_scale = js::max(1.0, sk_::extent(sk));
    let tol = 1e-9 * tol_scale;
    let mut res = (false, 0usize);
    let mut pinned_ok = false;
    let any_pin = pin.is_some_and(|p| !p.is_empty()) || pin_radius.is_some_and(|p| !p.is_empty());
    if any_pin {
        let mut pinned: HashSet<String> = HashSet::new();
        if let Some(p) = pin {
            pinned.extend(p.keys().map(str::to_string));
        }
        if let Some(p) = pin_radius {
            pinned.extend(p.keys().map(str::to_string));
        }
        if let Some(ents) = sk.get_mut("entities").and_then(Value::as_array_mut) {
            for e in ents.iter_mut() {
                let id = jv::s(e, "id").to_string();
                if let Some(q) = pin.and_then(|p| p.get(&id)) {
                    jv::set(e, "x", jv::num(q[0]));
                    jv::set(e, "y", jv::num(q[1]));
                }
                if let Some(r) = pin_radius.and_then(|p| p.get(&id)) {
                    jv::set(e, "r", jv::num(js::max(1e-9, *r)));
                }
            }
        }
        let (vars, mut x) = build_vars(sk, &pinned);
        let eqs = equations(sk, &vars);
        res = newton(&eqs, &mut x, tol, 60);
        if res.0 {
            write_back(sk, &vars, &x);
            pinned_ok = true;
        } else {
            restore(sk);
        }
    }
    if !pinned_ok {
        let (vars, mut x) = build_vars(sk, &HashSet::new());
        let eqs = equations(sk, &vars);
        res = newton(&eqs, &mut x, tol, 100);
        if res.0 {
            write_back(sk, &vars, &x);
        } else {
            restore(sk);
        }
    }
    let a = analyze(sk, Some(tol_scale));
    let geometry = !sk_::entities(sk).is_empty();
    let status = if !geometry {
        "empty"
    } else if !a.conflicts.is_empty() || !a.redundant.is_empty() {
        "over"
    } else if a.dof == 0 {
        "full"
    } else {
        "under"
    };
    Solved {
        ok: res.0,
        dragged: pinned_ok,
        status,
        dof: a.dof,
        conflicts: a.conflicts,
        redundant: a.redundant,
        determined: a.determined,
        iterations: res.1,
    }
}

/// The compiled residuals at the sketch's current geometry (for the parity harness: `equations` values).
pub fn residuals_now(sk: &Value) -> Vec<(String, f64)> {
    let (vars, x) = build_vars(sk, &HashSet::new());
    equations(sk, &vars).iter().map(|q| (q.cid.clone(), q.f(&x))).collect()
}
