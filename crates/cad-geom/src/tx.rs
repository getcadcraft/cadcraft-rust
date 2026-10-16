//! geom.js, part 3: entities through transforms (MOVE, COPY, ROTATE, SCALE, MIRROR), grips.

use crate::jsmath::JsMath;
use cc_cad::entity::{Entity, Geo, Obj, Vertex, num};
use cc_cad::js;
use cc_geom::P2;
use serde_json::Value;

use crate::geom::*;
use crate::leader::leader_style;

/// A new entity: e put through the affine m (similar transforms only, except inserts and text).
pub fn tx_entity(e: &Entity, m: &M) -> Entity {
    let pp = |x: f64, y: f64| m_apply(m, p(x, y));
    let det = m_det(m);
    let sim = m_similar(m).unwrap_or(Sim { s: det.abs().sqrt(), rot: m[1].jatan2(m[0]), flip: det < 0.0 });
    let mut o = e.clone();
    match &mut o.geo {
        Geo::Line(l) => {
            let a = pp(l.x1, l.y1);
            let b = pp(l.x2, l.y2);
            (l.x1, l.y1, l.x2, l.y2) = (a.x, a.y, b.x, b.y);
        }
        Geo::Point(pt) => {
            let a = pp(pt.x, pt.y);
            (pt.x, pt.y) = (a.x, a.y);
        }
        Geo::Circle(c) => {
            let a = pp(c.cx, c.cy);
            (c.cx, c.cy, c.r) = (a.x, a.y, c.r * sim.s);
        }
        Geo::Arc(a) => {
            let c = pp(a.cx, a.cy);
            let s = pp(a.cx + a.r * a.a0.jcos(), a.cy + a.r * a.a0.jsin());
            let t = pp(a.cx + a.r * a.a1.jcos(), a.cy + a.r * a.a1.jsin());
            let mut a0 = (s.y - c.y).jatan2(s.x - c.x);
            let mut a1 = (t.y - c.y).jatan2(t.x - c.x);
            if sim.flip {
                std::mem::swap(&mut a0, &mut a1);
            }
            (a.cx, a.cy, a.r, a.a0, a.a1) = (c.x, c.y, a.r * sim.s, norm_angle(a0), norm_angle(a1));
        }
        Geo::Poly(pl) => {
            pl.pts = pl
                .pts
                .iter()
                .map(|v| {
                    let q = pp(v.x, v.y);
                    Vertex::new(q.x, q.y, if sim.flip { -v.bulge() } else { v.bulge() })
                })
                .collect();
        }
        Geo::Ellipse(el) => {
            let c = pp(el.cx, el.cy);
            let a = pp(el.cx + el.mx, el.cy + el.my);
            let (mut t0, mut t1) = (el.t0, el.t1);
            if sim.flip && t1 - t0 < TAU - EPS {
                (t0, t1) = (TAU - t1, TAU - t0);
            }
            (el.cx, el.cy, el.mx, el.my, el.t0, el.t1) = (c.x, c.y, a.x - c.x, a.y - c.y, t0, t1);
        }
        Geo::Text(t) => {
            let a = pp(t.x, t.y);
            let mut rot = js::or(t.rot, 0.0) + sim.rot;
            if sim.flip {
                // mirrored text is placed mirrored but stays readable (MIRRTEXT 0)
                rot = m[1].jatan2(m[0]) - js::or(t.rot, 0.0);
                if rot.jcos() < -1e-9 {
                    rot += PI;
                }
            }
            t.x = a.x;
            t.y = a.y;
            t.h *= sim.s;
            t.rot = Some(norm_angle(rot));
            if let Some(ww) = t.ww.filter(|w| *w != 0.0 && !w.is_nan()) {
                t.ww = Some(ww * sim.s);
            }
        }
        Geo::Insert(i) => {
            let a = pp(i.x, i.y);
            let r0 = js::or(i.rot, 0.0);
            let rot = if sim.flip { sim.rot + PI - r0 } else { r0 + sim.rot };
            i.x = a.x;
            i.y = a.y;
            i.rot = Some(norm_angle(rot));
            i.sx = Some(i.sx.unwrap_or(1.0) * sim.s * if sim.flip { -1.0 } else { 1.0 });
            i.sy = Some(i.sy.unwrap_or(1.0) * sim.s);
        }
        Geo::Hatch(h) => {
            h.loops = h
                .loops
                .iter()
                .map(|l| {
                    let mut out = Vec::with_capacity(l.len());
                    for c in l.chunks_exact(2) {
                        if let [x, y] = c {
                            let q = pp(*x, *y);
                            out.push(q.x);
                            out.push(q.y);
                        }
                    }
                    out
                })
                .collect();
        }
        Geo::Dim(d) => {
            d.block = None;
            for k in ["p1", "p2", "c", "loc"] {
                if let Some(q) = d.point(k) {
                    d.set_point(k, pp(q.x, q.y));
                }
            }
            if d.kind.as_deref() == Some("linear") {
                let r = js::or(d.rot, 0.0);
                let u = p(m[0] * r.jcos() + m[2] * r.jsin(), m[1] * r.jcos() + m[3] * r.jsin());
                d.rot = Some(norm_angle(u.y.jatan2(u.x)));
            }
        }
        Geo::Viewport(v) => {
            let c = pp(v.x, v.y);
            (v.x, v.y, v.w, v.h, v.s) = (c.x, c.y, v.w * sim.s, v.h * sim.s, v.s * sim.s);
        }
        Geo::Xline(i) | Geo::Ray(i) => {
            let a = pp(i.x, i.y);
            let (dx, dy) = unit_dir(m[0] * i.dx + m[2] * i.dy, m[1] * i.dx + m[3] * i.dy).unwrap_or((i.dx, i.dy));
            (i.x, i.y, i.dx, i.dy) = (a.x, a.y, dx, dy);
        }
        Geo::Mleader(l) => {
            let Some(last) = l.pts.last().copied() else { return o };
            let pts: Vec<P2> = l.pts.iter().map(|q| pp(q.x, q.y)).collect();
            let far = pp(last.x + if l.dir == Some(-1.0) { -1.0 } else { 1.0 }, last.y);
            let q = pts.last().copied().unwrap_or(far);
            let st = leader_style(l.st.as_ref());
            let k = sim.s;
            let g = |n: &str| js::to_number_opt(st.get(n));
            let mut st2: Obj = st.clone();
            st2.insert("asz".into(), num(g("asz") * k));
            st2.insert("dogleg".into(), num(g("dogleg") * k));
            st2.insert("gap".into(), num(g("gap") * k));
            l.h = Some(js::or(l.h, g("asz")) * k);
            l.pts = pts;
            l.dir = Some(if far.x >= q.x { 1.0 } else { -1.0 });
            if let Some(ww) = l.ww.filter(|w| *w != 0.0 && !w.is_nan()) {
                l.ww = Some(ww * k);
            }
            l.st = Some(st2);
        }
        Geo::Other => {}
    }
    o
}

/// One grip: { x, y, key }.
#[derive(Clone, Debug, PartialEq)]
pub struct Grip {
    pub x: f64,
    pub y: f64,
    pub key: String,
}
impl Grip {
    fn new(q: P2, key: &str) -> Self {
        Grip { x: q.x, y: q.y, key: key.into() }
    }
    pub fn to_value(&self) -> Value {
        serde_json::json!({ "x": num(self.x), "y": num(self.y), "key": self.key })
    }
}

/// Grip points of an entity (add-on grip hooks are not ported).
pub fn grips(e: &Entity) -> Vec<Grip> {
    match &e.geo {
        Geo::Line(l) => vec![
            Grip::new(p(l.x1, l.y1), "p1"),
            Grip::new(p((l.x1 + l.x2) / 2.0, (l.y1 + l.y2) / 2.0), "mid"),
            Grip::new(p(l.x2, l.y2), "p2"),
        ],
        Geo::Circle(c) => {
            let mut out = vec![Grip::new(p(c.cx, c.cy), "c")];
            for k in 0..4 {
                let a = f(k) * PI / 2.0;
                out.push(Grip::new(p(c.cx + c.r * a.jcos(), c.cy + c.r * a.jsin()), "r"));
            }
            out
        }
        Geo::Arc(a) => {
            let c = p(a.cx, a.cy);
            vec![Grip::new(c, "c"), Grip::new(polar(c, a.a0, a.r), "a0"), Grip::new(polar(c, a.a1, a.r), "a1")]
        }
        Geo::Poly(pl) => pl.pts.iter().enumerate().map(|(i, v)| Grip::new(v.p(), &format!("v{i}"))).collect(),
        Geo::Ellipse(el) => vec![Grip::new(p(el.cx, el.cy), "c")],
        Geo::Text(t) => vec![Grip::new(p(t.x, t.y), "p")],
        Geo::Point(t) => vec![Grip::new(p(t.x, t.y), "p")],
        Geo::Insert(t) => vec![Grip::new(p(t.x, t.y), "p")],
        Geo::Xline(t) | Geo::Ray(t) => vec![Grip::new(p(t.x, t.y), "p")],
        Geo::Dim(d) => {
            let rd = matches!(d.kind(), "radius" | "diameter");
            ["p1", "p2", "c", "loc"].iter().filter_map(|k| d.point(k).filter(|_| !(*k == "p2" && rd)).map(|q| Grip::new(q, k))).collect()
        }
        Geo::Viewport(v) => {
            let mut out = vec![Grip::new(p(v.x, v.y), "c")];
            for (i, q) in viewport_corners(v.x, v.y, v.w, v.h).iter().enumerate() {
                out.push(Grip::new(*q, &format!("k{i}")));
            }
            out
        }
        Geo::Mleader(l) => l.pts.iter().enumerate().map(|(i, q)| Grip::new(*q, &format!("v{i}"))).collect(),
        Geo::Hatch(_) | Geo::Other => Vec::new(),
    }
}

/// `+key.slice(1)` as an index.
fn key_index(key: &str) -> Option<usize> {
    let rest: String = key.chars().skip(1).collect();
    let n = js::str_to_number(&rest);
    if n.is_finite() && n >= 0.0 && n.fract() == 0.0 && n < 1e15 { usize::try_from(format!("{n}").parse::<u64>().ok()?).ok() } else { None }
}

/// e with grip `key` moved to `to`.
pub fn grip_move(e: &Entity, key: &str, to: P2) -> Entity {
    let mut o = e.clone();
    match &mut o.geo {
        Geo::Line(l) => match key {
            "p1" => (l.x1, l.y1) = (to.x, to.y),
            "p2" => (l.x2, l.y2) = (to.x, to.y),
            _ => {
                let dx = to.x - (l.x1 + l.x2) / 2.0;
                let dy = to.y - (l.y1 + l.y2) / 2.0;
                (l.x1, l.y1, l.x2, l.y2) = (l.x1 + dx, l.y1 + dy, l.x2 + dx, l.y2 + dy);
            }
        },
        Geo::Circle(c) => {
            if key == "c" {
                (c.cx, c.cy) = (to.x, to.y);
            } else {
                c.r = js::max(dist(p(c.cx, c.cy), to), EPS);
            }
        }
        Geo::Arc(a) => {
            if key == "c" {
                (a.cx, a.cy) = (to.x, to.y);
            } else {
                let v = norm_angle((to.y - a.cy).jatan2(to.x - a.cx));
                match key {
                    "a0" => a.a0 = v,
                    "a1" => a.a1 = v,
                    "r" => a.r = v,
                    "cx" => a.cx = v,
                    "cy" => a.cy = v,
                    _ => {
                        o.extra.insert(key.into(), num(v));
                    }
                }
            }
        }
        Geo::Poly(pl) => {
            if let Some(i) = key_index(key)
                && let Some(v) = pl.pts.get_mut(i)
            {
                (v.x, v.y) = (to.x, to.y);
            }
        }
        Geo::Ellipse(el) => (el.cx, el.cy) = (to.x, to.y),
        Geo::Text(t) => (t.x, t.y) = (to.x, to.y),
        Geo::Point(t) => (t.x, t.y) = (to.x, to.y),
        Geo::Insert(t) => (t.x, t.y) = (to.x, to.y),
        Geo::Xline(t) | Geo::Ray(t) => (t.x, t.y) = (to.x, to.y),
        Geo::Dim(d) => {
            d.block = None;
            if key == "c" && matches!(d.kind(), "radius" | "diameter") {
                let Some(c) = d.c else { return o };
                let (dx, dy) = (to.x - c.x, to.y - c.y);
                let mv = |q: P2| p(q.x + dx, q.y + dy);
                d.c = Some(mv(c));
                d.p1 = d.p1.map(mv);
                d.loc = d.loc.map(mv);
            } else if matches!(key, "p1" | "p2" | "c" | "loc") {
                d.set_point(key, to);
            } else {
                o.extra.insert(key.into(), cc_cad::pt(to));
            }
        }
        Geo::Mleader(l) => {
            if let Some(i) = key_index(key)
                && let Some(v) = l.pts.get_mut(i)
            {
                *v = to;
            }
        }
        Geo::Viewport(v) => {
            if key == "c" {
                (v.x, v.y) = (to.x, to.y);
            } else {
                let Some(k) = key_index(key) else { return o };
                let c = viewport_corners(v.x, v.y, v.w, v.h);
                let Some(oc) = c.get((k + 2) % 4).copied() else { return o };
                let w = js::max((to.x - oc.x).abs(), EPS);
                let h = js::max((to.y - oc.y).abs(), EPS);
                let x = (to.x + oc.x) / 2.0;
                let y = (to.y + oc.y) / 2.0;
                let tw = js::or(v.tw, 0.0);
                let tc = (-tw).jcos();
                let tn = (-tw).jsin();
                let dx = (x - v.x) / v.s;
                let dy = (y - v.y) / v.s;
                (v.x, v.y, v.w, v.h, v.cx, v.cy) = (x, y, w, h, v.cx + tc * dx - tn * dy, v.cy + tn * dx + tc * dy);
            }
        }
        Geo::Hatch(_) | Geo::Other => {}
    }
    o
}
