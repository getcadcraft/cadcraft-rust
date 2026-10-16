//! dims.js: a dimension is one entity that keeps its defining points and draws itself from them:
//! `{ type: 'dim', kind, p1, p2, c, loc, rot, text, st, block? }` (linear, aligned, angular, radius, diameter).
//! `text` is '' for the measured value or an override where '<>' stands for it; `st` the style (DIM_DEFAULTS +
//! dimstyle.js extras, sizes × st.scale).

use crate::jsmath::JsMath;
use cc_cad::entity::{Arc, Color, Dim, Entity, Geo, Hatch, Obj, Text};
use cc_cad::js;
use cc_geom::P2;
use serde_json::Value;

use crate::dimstyle::{St, dim_value, replace_value, style_dim_parts};
use crate::font::advance;
use crate::geom::*;

pub use cc_cad::doc::{DIM_DEFAULTS, dim_defaults};
/// Dimension kinds; their names for people are the catalog keys `dimkind.<kind>`.
pub const DIM_KINDS: [&str; 5] = ["linear", "aligned", "angular", "radius", "diameter"];
pub const DIM_POINT_KEYS: [&str; 4] = ["p1", "p2", "c", "loc"];

fn dim_of(e: &Entity) -> Option<&Dim> {
    if let Geo::Dim(d) = &e.geo { Some(d) } else { None }
}
/// `{ ...DIM_DEFAULTS, ...(e.st || {}) }`
fn style(d: &Dim) -> St {
    let mut o = dim_defaults();
    if let Some(s) = &d.st {
        for (k, v) in s {
            o.insert(k.clone(), v.clone());
        }
    }
    St(o)
}
fn nan2() -> P2 {
    p(f64::NAN, f64::NAN)
}

/// The measured value (drawing units, or radians for angular).
pub fn dim_measure(e: &Entity) -> f64 {
    let Some(d) = dim_of(e) else { return 0.0 };
    let (p1, p2, c) = (d.p1.unwrap_or_else(nan2), d.p2.unwrap_or_else(nan2), d.c.unwrap_or_else(nan2));
    match d.kind() {
        "linear" => {
            let r = js::or(d.rot, 0.0);
            let u = p(r.jcos(), r.jsin());
            ((p2.x - p1.x) * u.x + (p2.y - p1.y) * u.y).abs()
        }
        "aligned" => dist(p1, p2),
        "radius" => dist(c, p1),
        "diameter" => 2.0 * dist(c, p1),
        "angular" => angular_sweep(d).2,
        _ => 0.0,
    }
}

/// The value as it is written on the drawing (dimstyle.js `dimValue(...).main`).
pub fn format_measure(e: &Entity, v: Option<f64>) -> String {
    dim_value(e, v.unwrap_or_else(|| dim_measure(e))).0
}

/// What the dimension's text says.
pub fn dim_text(e: &Entity) -> String {
    let t = dim_of(e).and_then(|d| d.text.clone()).unwrap_or_default();
    let v = format_measure(e, None);
    if t.is_empty() || t == "<>" { v } else { replace_value(&t, &v) }
}

/// The CCW sector of an angular dimension: (start, end, sweep).
pub fn angular_sweep(d: &Dim) -> (f64, f64, f64) {
    let (c, p1, p2, loc) = (d.c.unwrap_or_else(nan2), d.p1.unwrap_or_else(nan2), d.p2.unwrap_or_else(nan2), d.loc.unwrap_or_else(nan2));
    let a1 = angle_of(c, p1);
    let a2 = angle_of(c, p2);
    let al = angle_of(c, loc);
    let (s, end) = if on_sweep_d(norm_angle(al), norm_angle(a1), norm_angle(a1) + norm_angle(a2 - a1)) { (a1, a2) } else { (a2, a1) };
    let mut sweep = norm_angle(end - s);
    if sweep < 1e-12 {
        sweep = TAU;
    }
    (norm_angle(s), norm_angle(s) + sweep, sweep)
}

/// A readable text angle: never upside down.
pub fn readable(a: f64) -> f64 {
    let mut a = norm_angle(a);
    if a > PI / 2.0 + 1e-9 && a <= 1.5 * PI + 1e-9 {
        a -= PI;
    }
    norm_angle(a)
}

fn part(geo: Geo, role: Option<&str>) -> Entity {
    let mut e = Entity::with(geo, Some("0".into()), Some(Color::Num(0.0)));
    if let Some(r) = role {
        e.extra.insert("role".into(), Value::String(r.into()));
    }
    e
}

/// The entities a dimension draws as (lines, arcs, arrowheads as solid hatches and one text), in world coordinates,
/// on layer 0 with colour by block, each marked with its `role`, then styled (dimstyle.js `styleDimParts`).
pub fn dim_geometry(e: &Entity) -> Vec<Entity> {
    let Some(d) = dim_of(e) else { return Vec::new() };
    let st = style(d);
    let sc = st.or("scale", 1.0);
    let (txt, asz, exe, exo, gap) = (st.n("txt") * sc, st.n("asz") * sc, st.n("exe") * sc, st.n("exo") * sc, st.n("gap") * sc);
    let mut out: Vec<Entity> = Vec::new();
    let line = |out: &mut Vec<Entity>, a: P2, b: P2, role: Option<&str>| {
        if dist(a, b) > 1e-12 {
            out.push(part(line_ent(a.x, a.y, b.x, b.y), role));
        }
    };
    let arrow = |out: &mut Vec<Entity>, tip: P2, ang: f64, role: &str| {
        let base = polar(tip, ang + PI, asz);
        let w = asz / 6.0;
        let l = polar(base, ang + PI / 2.0, w);
        let r = polar(base, ang - PI / 2.0, w);
        let mut h =
            part(Geo::Hatch(Hatch { loops: vec![vec![tip.x, tip.y, l.x, l.y, r.x, r.y, tip.x, tip.y]], solid: Some(true) }), Some(role));
        h.extra.insert("tip".into(), cc_cad::pt(tip));
        h.extra.insert("ang".into(), cc_cad::num(norm_angle(ang)));
        out.push(h);
    };
    let measure = dim_measure(e);
    let text = |out: &mut Vec<Entity>, q: P2, rot: f64, ha: &str, va: &str, s: &str| {
        let t = Text {
            x: q.x,
            y: q.y,
            h: txt,
            s: s.to_string(),
            rot: Some(norm_angle(rot)),
            ha: Some(ha.into()),
            va: Some(va.into()),
            wf: Some(1.0),
            ww: None,
            ls: None,
        };
        let mut te = part(Geo::Text(t), Some("text"));
        te.extra.insert("measure".into(), cc_cad::num(measure));
        out.push(te);
    };
    let s = dim_text(e);
    let tw = advance(&s) / 1000.0 * txt * FONT_EM;
    let (p1o, p2o, co, loco) = (d.p1, d.p2, d.c, d.loc);
    match d.kind() {
        "linear" | "aligned" => {
            let (Some(p1), Some(p2), Some(loc)) = (p1o, p2o, loco) else { return Vec::new() };
            let r = if d.kind() == "aligned" { angle_of(p1, p2) } else { js::or(d.rot, 0.0) };
            let u = p(r.jcos(), r.jsin());
            let d1 = (p1.x - loc.x) * u.x + (p1.y - loc.y) * u.y;
            let d2 = (p2.x - loc.x) * u.x + (p2.y - loc.y) * u.y;
            let q1 = p(loc.x + u.x * d1, loc.y + u.y * d1);
            let q2 = p(loc.x + u.x * d2, loc.y + u.y * d2);
            for (pp, q, role) in [(p1, q1, "ext1"), (p2, q2, "ext2")] {
                let l = dist(pp, q);
                if l < 1e-12 {
                    continue;
                }
                let a = angle_of(pp, q);
                if exo < l + exe {
                    line(&mut out, polar(pp, a, js::min(exo, l)), polar(q, a, exe), Some(role));
                }
            }
            let len = dist(q1, q2);
            let inside = len >= 2.0 * asz + 1e-12;
            let a12 = if len > 1e-12 { angle_of(q1, q2) } else { r };
            let mid = p((q1.x + q2.x) / 2.0, (q1.y + q2.y) / 2.0);
            let ta = readable(a12);
            let up = ta + PI / 2.0;
            let off = js::min(len / 2.0, 2.0 * asz + tw / 2.0 + gap);
            let tm = if st.is("just", 1.0) && len > 1e-12 {
                polar(q1, a12, off)
            } else if st.is("just", 2.0) && len > 1e-12 {
                polar(q2, a12 + PI, off)
            } else {
                mid
            };
            let fits = st.is("tad", 0.0) && len > tw + 2.0 * gap + if inside { 2.0 * asz } else { 0.0 };
            if inside {
                if fits {
                    line(&mut out, q1, polar(tm, a12 + PI, tw / 2.0 + gap), Some("dim1"));
                    line(&mut out, polar(tm, a12, tw / 2.0 + gap), q2, Some("dim2"));
                } else {
                    line(&mut out, q1, q2, Some("dim"));
                }
                arrow(&mut out, q1, a12 + PI, "arrow1");
                arrow(&mut out, q2, a12, "arrow2");
            } else {
                line(&mut out, polar(q1, a12 + PI, 2.0 * asz), polar(q2, a12, 2.0 * asz), Some("dim"));
                arrow(&mut out, q1, a12, "arrow1");
                arrow(&mut out, q2, a12 + PI, "arrow2");
            }
            if fits {
                text(&mut out, tm, ta, "center", "middle", &s);
            } else if st.is("tad", 4.0) {
                text(&mut out, polar(tm, up, -gap), ta, "center", "top", &s);
            } else {
                text(&mut out, polar(tm, up, gap), ta, "center", "bottom", &s);
            }
        }
        "angular" => {
            let (Some(c), Some(loc), Some(_), Some(_)) = (co, loco, p1o, p2o) else { return Vec::new() };
            let (a0, a1, sweep) = angular_sweep(d);
            let rr = dist(c, loc);
            if rr < 1e-12 {
                return style_dim_parts(e, out);
            }
            out.push(part(Geo::Arc(Arc { cx: c.x, cy: c.y, r: rr, a0: norm_angle(a0), a1: norm_angle(a1) }), Some("dim")));
            for (ang, pp, role) in [(a0, nearest_on_ray(d, a0), "ext1"), (a1, nearest_on_ray(d, a1), "ext2")] {
                let dp = dist(c, pp);
                if rr > dp + 1e-9 {
                    line(&mut out, polar(c, ang, dp + exo), polar(c, ang, rr + exe), Some(role));
                } else if rr < dp - 1e-9 && dp - exo > rr - exe {
                    line(&mut out, polar(c, ang, dp - exo), polar(c, ang, js::max(0.0, rr - exe)), Some(role));
                }
            }
            let start = polar(c, a0, rr);
            let end = polar(c, a1, rr);
            if rr * sweep >= 2.0 * asz {
                arrow(&mut out, start, a0 - PI / 2.0, "arrow1");
                arrow(&mut out, end, a1 + PI / 2.0, "arrow2");
            } else {
                arrow(&mut out, start, a0 + PI / 2.0, "arrow1");
                arrow(&mut out, end, a1 - PI / 2.0, "arrow2");
            }
            let m = a0 + sweep / 2.0;
            let ta = readable(m - PI / 2.0);
            let outward = (ta + PI / 2.0 - m).jcos() > 0.0;
            text(&mut out, polar(c, m, rr + gap), ta, "center", if outward { "bottom" } else { "top" }, &s);
        }
        "radius" | "diameter" => {
            let (Some(c), Some(p1), Some(loc)) = (co, p1o, loco) else { return Vec::new() };
            let r = dist(c, p1);
            if r < 1e-12 {
                return style_dim_parts(e, out);
            }
            let dl = dist(c, loc);
            let u = if dl > 1e-12 { angle_of(c, loc) } else { angle_of(c, p1) };
            let pp = polar(c, u, r);
            let cm = asz / 2.0;
            line(&mut out, polar(c, 0.0, -cm), polar(c, 0.0, cm), None);
            line(&mut out, polar(c, PI / 2.0, -cm), polar(c, PI / 2.0, cm), None);
            if dl > r + 1e-9 {
                line(&mut out, pp, loc, Some("dim"));
                arrow(&mut out, pp, u + PI, "arrow1");
                let sx = if u.jcos() >= 0.0 { 1.0 } else { -1.0 };
                let land = p(loc.x + sx * asz, loc.y);
                line(&mut out, loc, land, Some("dim"));
                text(&mut out, p(land.x + sx * gap, land.y), 0.0, if sx > 0.0 { "left" } else { "right" }, "middle", &s);
            } else if d.kind() == "radius" {
                line(&mut out, c, pp, Some("dim"));
                arrow(&mut out, pp, u, "arrow1");
                let ta = readable(u);
                text(&mut out, polar(polar(c, u, r / 2.0), ta + PI / 2.0, gap), ta, "center", "bottom", &s);
            } else {
                let q = polar(c, u + PI, r);
                line(&mut out, q, pp, Some("dim"));
                arrow(&mut out, pp, u, "arrow1");
                arrow(&mut out, q, u + PI, "arrow2");
                let ta = readable(u);
                text(&mut out, polar(c, ta + PI / 2.0, gap), ta, "center", "bottom", &s);
            }
        }
        _ => {}
    }
    style_dim_parts(e, out)
}

/// The defining point that lies on the ray from the vertex at angle a (p1 or p2).
fn nearest_on_ray(d: &Dim, a: f64) -> P2 {
    let c = d.c.unwrap_or_else(nan2);
    let f = |q: P2| (norm_angle(angle_of(c, q) - a + PI) - PI).abs();
    let (p1, p2) = (d.p1.unwrap_or_else(nan2), d.p2.unwrap_or_else(nan2));
    if f(p1) <= f(p2) { p1 } else { p2 }
}

/// The text point of a dimension (DXF group 11): the middle of its text.
pub fn dim_text_point(e: &Entity) -> Option<P2> {
    for q in dim_geometry(e) {
        if let Geo::Text(t) = &q.geo {
            return Some(p(t.x, t.y));
        }
    }
    dim_of(e).and_then(|d| d.loc)
}

/// Angular dimension between two lines: the vertex, and a point on each leg so that the sector holding `loc` lies
/// between them → (c, p1, p2), or None.
pub fn angular_from_lines(l1: [f64; 4], l2: [f64; 4], loc: P2) -> Option<(P2, P2, P2)> {
    let [x11, y11, x12, y12] = l1;
    let [x21, y21, x22, y22] = l2;
    let (d1x, d1y, d2x, d2y) = (x12 - x11, y12 - y11, x22 - x21, y22 - y21);
    let den = d1x * d2y - d1y * d2x;
    if den.abs() < 1e-12 * js::hypot(d1x, d1y) * js::hypot(d2x, d2y) {
        return None;
    }
    let t = ((x21 - x11) * d2y - (y21 - y11) * d2x) / den;
    let c = p(x11 + d1x * t, y11 + d1y * t);
    let al = angle_of(c, loc);
    let legs: Vec<Vec<(f64, P2)>> = [l1, l2]
        .iter()
        .map(|l| {
            let a = (l[3] - l[1]).jatan2(l[2] - l[0]);
            [a, a + PI]
                .iter()
                .map(|&ang| {
                    let mut best: Option<(f64, P2)> = None;
                    for q in [p(l[0], l[1]), p(l[2], l[3])] {
                        let along = (q.x - c.x) * ang.jcos() + (q.y - c.y) * ang.jsin();
                        if along > 1e-9 && best.is_none_or(|b| along > b.0) {
                            best = Some((along, q));
                        }
                    }
                    (norm_angle(ang), best.map_or_else(|| polar(c, ang, js::or(Some(js::hypot(d1x, d1y) * 1e-3), 1.0)), |b| b.1))
                })
                .collect()
        })
        .collect();
    let (l0, l1v) = (legs.first()?, legs.get(1)?);
    for r1 in l0 {
        for r2 in l1v {
            for (a, b) in [(r1, r2), (r2, r1)] {
                let sw = norm_angle(b.0 - a.0);
                if sw > 1e-9 && sw < PI - 1e-9 && on_sweep_d(norm_angle(al), a.0, a.0 + sw) {
                    return Some((c, a.1, b.1));
                }
            }
        }
    }
    None
}

/// Read a style object (for the parity harness).
pub fn style_of(st: Option<&Obj>) -> Obj {
    crate::dimstyle::full(st).0
}
