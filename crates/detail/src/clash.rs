//! clash.js — clash check between steel parts and footings (0.4 D15): oriented boxes (separating-axis test), then
//! points across each part's true section (copes cut away) tested inside the other; parts a connection means to meet,
//! one connection's plates, one stair's parts, bolts, rebar and a shear lug in its footing are left out.

use crate::build::Built;
use crate::geom::{Frame, V3, add, cross, dot, mul, sub, to_local, to_world};
use crate::js::{Obj, cos, hypot, hypot3, max, max_of, min, min_of, num, num_or0, round, sin, string_of};
use crate::sections::Loop;
use crate::state::{Conn, members_of_v};
use serde_json::{Value, json};
use std::f64::consts::PI;

/// An oriented box.
#[derive(Clone, Copy, Debug)]
pub struct Obb {
    pub c: V3,
    pub ax: [V3; 3],
    pub h: [f64; 3],
}

struct Solid {
    r#ref: Value,
    id: String,
    concrete: bool,
    conn: Option<String>,
    stair: Option<String>,
    frame: Frame,
    l: f64,
    loops: Vec<Loop>,
    voff: f64,
    cuts: Vec<(bool, bool, f64, f64)>, // (end is start, side is top, length, depth)
    depth: f64,
    lug: bool,
    bx: Obb,
}

fn solids(state: &Value, built: &Built, phases: Option<&[f64]>) -> Vec<Solid> {
    let keep = |ph: f64| phases.is_none_or(|p| p.contains(&crate::js::or(ph, 1.0)));
    let mut out = Vec::new();
    for p in &built.parts {
        if !keep(p.phase) {
            continue;
        }
        let (w, d) = (p.profile.width, p.profile.depth);
        let c = add(p.frame.o, add(mul(p.frame.x, p.length / 2.0), mul(p.frame.v, p.voff)));
        let r = match (&p.member, &p.stair) {
            (Some(m), _) => json!({ "kind": "member", "id": m }),
            (None, Some(s)) => json!({ "kind": "stair", "id": s }),
            _ => json!({ "kind": "conn", "id": p.conn }),
        };
        out.push(Solid {
            r#ref: r,
            id: p.id.clone(),
            concrete: false,
            conn: p.conn.clone(),
            stair: p.stair.clone(),
            frame: p.frame,
            l: p.length,
            loops: p.profile.loops.clone(),
            voff: p.voff,
            cuts: p.cuts.iter().filter(|k| k.kind == "cope").map(|k| (k.end == "start", k.side == "top", k.length, k.depth)).collect(),
            depth: d,
            lug: p.conn.as_ref().is_some_and(|c| p.id == format!("{c}.lug")),
            bx: Obb { c, ax: [p.frame.x, p.frame.u, p.frame.v], h: [p.length / 2.0, w / 2.0, d / 2.0] },
        });
    }
    for f in crate::js::arr(state.get("footings")) {
        if !keep(crate::js::f(f.get("phase"))) {
            continue;
        }
        let a = num_or0(f.get("rotation")) * PI / 180.0;
        let x = [cos(a), sin(a), 0.0];
        let u = [-sin(a), cos(a), 0.0];
        let v = [0.0, 0.0, 1.0];
        let g = |k: &str| num(f.get(k));
        let o = add([g("x"), g("y"), g("top") - g("H") / 2.0], mul(x, -g("L") / 2.0));
        let lp = vec![
            [-g("B") / 2.0, -g("H") / 2.0],
            [g("B") / 2.0, -g("H") / 2.0],
            [g("B") / 2.0, g("H") / 2.0],
            [-g("B") / 2.0, g("H") / 2.0],
        ];
        out.push(Solid {
            r#ref: json!({ "kind": "footing", "id": f.get("id") }),
            id: format!("footing:{}", string_of(f.get("id"))),
            concrete: true,
            conn: None,
            stair: None,
            frame: Frame { o, x, u, v, len: f64::NAN },
            l: g("L"),
            loops: vec![lp],
            voff: 0.0,
            cuts: vec![],
            depth: g("H"),
            lug: false,
            bx: Obb { c: [g("x"), g("y"), g("top") - g("H") / 2.0], ax: [x, u, v], h: [g("L") / 2.0, g("B") / 2.0, g("H") / 2.0] },
        });
    }
    out
}

/// `obbOverlap(A, B, tol)`.
pub fn obb_overlap(a: &Obb, b: &Obb, tol: f64) -> bool {
    let t = sub(b.c, a.c);
    let mut axes: Vec<V3> = a.ax.iter().chain(&b.ax).copied().collect();
    for x in &a.ax {
        for y in &b.ax {
            let c = cross(*x, *y);
            if hypot3(c[0], c[1], c[2]) > 1e-9 {
                axes.push(c);
            }
        }
    }
    for l in axes {
        let n = hypot3(l[0], l[1], l[2]);
        let l = mul(l, 1.0 / n);
        let ra = a.h.iter().zip(&a.ax).fold(0.0, |s, (h, ax)| s + h * dot(*ax, l).abs());
        let rb = b.h.iter().zip(&b.ax).fold(0.0, |s, (h, ax)| s + h * dot(*ax, l).abs());
        if dot(t, l).abs() > ra + rb - tol {
            return false;
        }
    }
    true
}

fn in_poly(p: [f64; 2], lp: &[[f64; 2]]) -> bool {
    let mut inside = false;
    let n = lp.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        if let (Some(a), Some(b)) = (lp.get(i), lp.get(j))
            && (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < ((b[0] - a[0]) * (p[1] - a[1])) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}
fn edge_dist(p: [f64; 2], lp: &[[f64; 2]]) -> f64 {
    let mut best = f64::INFINITY;
    let n = lp.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        if let (Some(a), Some(b)) = (lp.get(j), lp.get(i)) {
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let l2 = dx * dx + dy * dy;
            let t = if l2 != 0.0 && !l2.is_nan() { max(0.0, min(1.0, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2)) } else { 0.0 };
            best = min(best, hypot(p[0] - a[0] - t * dx, p[1] - a[1] - t * dy));
        }
        j = i;
    }
    best
}
fn loops_at(s: &Solid, x: f64) -> Vec<Loop> {
    let mut loops = s.loops.clone();
    for &(at_start, top, length, depth) in &s.cuts {
        let in_cope = if at_start { x < length } else { x > s.l - length };
        if !in_cope {
            continue;
        }
        loops = loops
            .iter()
            .enumerate()
            .map(|(k, l)| {
                if k == 0 {
                    if top {
                        crate::geom::clip_loop_v(l, s.depth / 2.0 - depth, true)
                    } else {
                        crate::geom::clip_loop_v(l, -s.depth / 2.0 + depth, false)
                    }
                } else {
                    l.clone()
                }
            })
            .filter(|l| !l.is_empty())
            .collect();
    }
    loops
}
fn depth_in(s: &Solid, w: V3) -> f64 {
    let l = to_local(&s.frame, w, s.voff);
    if l.x <= 0.0 || l.x >= s.l {
        return 0.0;
    }
    let loops = loops_at(s, l.x);
    let Some(first) = loops.first() else { return 0.0 };
    let p = [l.u, l.v];
    if !in_poly(p, first) || loops.iter().skip(1).any(|h| in_poly(p, h)) {
        return 0.0;
    }
    min(min(l.x, s.l - l.x), min_of(loops.iter().map(|lp| edge_dist(p, lp))))
}
fn ray_exit(p: [f64; 2], d: [f64; 2], loops: &[Loop]) -> f64 {
    let mut best = f64::INFINITY;
    for lp in loops {
        let n = lp.len();
        let mut j = n.wrapping_sub(1);
        for i in 0..n {
            if let (Some(a), Some(b)) = (lp.get(j), lp.get(i)) {
                let e = [b[0] - a[0], b[1] - a[1]];
                let den = d[0] * e[1] - d[1] * e[0];
                if den.abs() >= 1e-12 {
                    let w = [a[0] - p[0], a[1] - p[1]];
                    let t = (w[0] * e[1] - w[1] * e[0]) / den;
                    let u = (w[0] * d[1] - w[1] * d[0]) / den;
                    if t > 1e-6 && (-1e-9..=1.0 + 1e-9).contains(&u) {
                        best = min(best, t);
                    }
                }
            }
            j = i;
        }
    }
    best
}
fn samples(a: &Solid, t0: f64, t1: f64, tol: f64) -> Vec<V3> {
    let mut pts = Vec::new();
    let mut sec: Vec<[f64; 2]> = Vec::new();
    let empty: Loop = Vec::new();
    let first = a.loops.first().unwrap_or(&empty);
    let inside = |q: [f64; 2]| in_poly(q, first) && !a.loops.iter().skip(1).any(|h| in_poly(q, h));
    let step = tol + 1.0;
    for lp in &a.loops {
        let n = lp.len();
        for i in 0..n {
            let (Some(&pa), Some(&pb)) = (lp.get(i), lp.get((i + 1) % n)) else { continue };
            let m = [(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0];
            sec.push(pa);
            sec.push(m);
            let l = hypot(pb[0] - pa[0], pb[1] - pa[1]);
            if l < 1e-9 {
                continue;
            }
            let nn = [-(pb[1] - pa[1]) / l, (pb[0] - pa[0]) / l];
            for sg in [1.0, -1.0] {
                let q = [m[0] + sg * nn[0] * step, m[1] + sg * nn[1] * step];
                if !inside(q) {
                    continue;
                }
                sec.push(q);
                let d = [sg * nn[0], sg * nn[1]];
                let tx = ray_exit(m, d, &a.loops);
                if tx > 2.0 * step {
                    sec.push([m[0] + d[0] * tx / 2.0, m[1] + d[1] * tx / 2.0]);
                }
            }
        }
    }
    let (mut x0, mut x1, mut y0, mut y1) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for &[u, v] in first {
        x0 = min(x0, u);
        x1 = max(x1, u);
        y0 = min(y0, v);
        y1 = max(y1, v);
    }
    let nn = 7.0;
    for i in 0..=7 {
        for j in 0..=7 {
            let q = [x0 + (x1 - x0) * f64::from(i) / nn, y0 + (y1 - y0) * f64::from(j) / nn];
            if inside(q) {
                sec.push(q);
            }
        }
    }
    let n = max(2.0, min(40.0, ((t1 - t0) / 10.0).ceil()));
    let mut k = 0.0;
    while k <= n {
        let x = t0 + (t1 - t0) * k / n;
        for &[u, v] in &sec {
            pts.push(to_world(&a.frame, x, u, v, a.voff));
        }
        k += 1.0;
    }
    pts
}
fn reach(a: &Solid, b: &Solid) -> Option<[f64; 2]> {
    let mut corners = Vec::with_capacity(8);
    for sx in [-1.0, 1.0] {
        for su in [-1.0, 1.0] {
            for sv in [-1.0, 1.0] {
                corners.push(add(
                    b.bx.c,
                    add(mul(b.bx.ax[0], sx * b.bx.h[0]), add(mul(b.bx.ax[1], su * b.bx.h[1]), mul(b.bx.ax[2], sv * b.bx.h[2]))),
                ));
            }
        }
    }
    let xs: Vec<f64> = corners.iter().map(|c| dot(sub(*c, a.frame.o), a.frame.x)).collect();
    let pad = hypot(a.bx.h[1], a.bx.h[2]);
    let t0 = max(0.0, min_of(xs.iter().copied()) - pad);
    let t1 = min(a.l, max_of(xs.iter().copied()) + pad);
    if t1 > t0 { Some([t0, t1]) } else { None }
}

fn intended(conns: &[Value], built: &Built) -> std::collections::HashSet<String> {
    let mut ok = std::collections::HashSet::new();
    let mut pair = |a: &str, b: &str| {
        ok.insert(format!("{a}|{b}"));
        ok.insert(format!("{b}|{a}"));
    };
    for c in conns {
        let ids: Vec<String> = members_of_v(c).iter().map(|v| v.map_or_else(|| "undefined".to_string(), crate::js::to_js_string)).collect();
        if ids.len() == 2
            && let (Some(a), Some(b)) = (ids.first(), ids.get(1))
        {
            pair(a, b);
        }
        let cid = Conn::from_value(c).id;
        for p in built.parts.iter().filter(|x| x.conn.as_deref() == Some(cid.as_str())) {
            for id in &ids {
                pair(&p.id, id);
            }
        }
    }
    ok
}

/// One clash.
#[derive(Clone, Debug)]
pub struct Clash {
    pub a: Value,
    pub b: Value,
    pub a_part: String,
    pub b_part: String,
    pub depth: f64,
    pub at: V3,
    pub concrete: bool,
}
/// `clashCheck(state, built, { tol, phases })` → (clashes, pairs, checked).
pub fn clash_check(state: &Value, built: &Built, tol: f64, phases: Option<&[f64]>) -> (Vec<Clash>, usize, usize) {
    let s = solids(state, built, phases);
    let skip = intended(crate::js::arr(state.get("connections")), built);
    let mut clashes = Vec::new();
    let mut pairs = 0usize;
    for (i, a) in s.iter().enumerate() {
        for b in s.iter().skip(i.saturating_add(1)) {
            if a.concrete && b.concrete {
                continue;
            }
            if skip.contains(&format!("{}|{}", a.id, b.id)) {
                continue;
            }
            if a.conn.is_some() && a.conn == b.conn {
                continue;
            }
            if a.stair.is_some() && a.stair == b.stair {
                continue;
            }
            if (a.concrete && b.lug) || (b.concrete && a.lug) {
                continue;
            }
            if !obb_overlap(&a.bx, &b.bx, tol) {
                continue;
            }
            pairs = pairs.saturating_add(1);
            let mut best: Option<(f64, V3)> = None;
            for (p, q) in [(a, b), (b, a)] {
                let Some(r) = reach(p, q) else { continue };
                for w in samples(p, r[0], r[1], tol) {
                    let d = depth_in(q, w);
                    if d > tol && best.is_none_or(|bb| d > bb.0) {
                        best = Some((d, w));
                    }
                }
            }
            if let Some((d, at)) = best {
                clashes.push(Clash {
                    a: a.r#ref.clone(),
                    b: b.r#ref.clone(),
                    a_part: a.id.clone(),
                    b_part: b.id.clone(),
                    depth: round(d * 10.0) / 10.0,
                    at: at.map(|v| round(v * 10.0) / 10.0),
                    concrete: a.concrete || b.concrete,
                });
            }
        }
    }
    clashes.sort_by(|x, y| crate::js::ord(y.depth - x.depth));
    (clashes, pairs, s.len())
}

/// The JSON of a clash check (`ms`, the time it took, is the caller's).
pub fn clash_json(r: &(Vec<Clash>, usize, usize)) -> Value {
    let cl: Vec<Value> =
        r.0.iter()
            .map(|c| {
                Obj::new()
                    .set("a", c.a.clone())
                    .set("b", c.b.clone())
                    .set("aPart", c.a_part.as_str())
                    .set("bPart", c.b_part.as_str())
                    .set("depth", c.depth)
                    .set("at", crate::js::p3v(c.at))
                    .set("kind", if c.concrete { "concrete" } else { "steel" })
                    .done()
            })
            .collect();
    json!({ "clashes": cl, "pairs": r.1, "checked": r.2 })
}
