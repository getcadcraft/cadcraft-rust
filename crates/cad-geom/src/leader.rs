//! leader.js: multileaders — ONE entity that draws itself like a dimension:
//! `{ type: 'mleader', pts: [{x, y}, …], dir: 1 | -1, s, h, ww?, ls?, st: { asz, dogleg, gap } }`.

use cc_cad::entity::{Color, Entity, Geo, Hatch, Obj, Poly, Text, Vertex};
use cc_cad::js;
use cc_geom::P2;

use crate::geom::p;

/// `{ ...MLEADER_DEFAULTS, ...(e.st || {}) }`
pub fn leader_style(st: Option<&Obj>) -> Obj {
    let mut o = Obj::new();
    o.insert("asz".into(), 2.5.into());
    o.insert("dogleg".into(), 2.5.into());
    o.insert("gap".into(), 1.into());
    if let Some(s) = st {
        for (k, v) in s {
            o.insert(k.clone(), v.clone());
        }
    }
    o
}
fn n(st: &Obj, k: &str) -> f64 {
    js::to_number_opt(st.get(k))
}

/// Where the landing starts and ends and where the text starts.
pub fn landing(e: &Entity) -> Option<(P2, P2, P2)> {
    let Geo::Mleader(l) = &e.geo else { return None };
    let st = leader_style(l.st.as_ref());
    let last = *l.pts.last()?;
    let dir = if l.dir == Some(-1.0) { -1.0 } else { 1.0 };
    let end = p(last.x + dir * n(&st, "dogleg"), last.y);
    Some((last, end, p(end.x + dir * n(&st, "gap"), end.y)))
}

/// The plain entities a multileader draws: the leader (an open polyline with its landing), the arrowhead (a solid
/// hatch) and the text — on layer 0, colour by block.
pub fn leader_geometry(e: &Entity) -> Vec<Entity> {
    let Geo::Mleader(l) = &e.geo else { return Vec::new() };
    if l.pts.len() < 2 {
        return Vec::new();
    }
    let st = leader_style(l.st.as_ref());
    let dir = if l.dir == Some(-1.0) { -1.0 } else { 1.0 };
    let Some((_, end, txt)) = landing(e) else { return Vec::new() };
    let base = |g: Geo| Entity::with(g, Some("0".into()), Some(Color::Num(0.0)));
    let mut pts: Vec<Vertex> = l.pts.iter().map(|q| Vertex::new(q.x, q.y, 0.0)).collect();
    if n(&st, "dogleg") > 0.0 {
        pts.push(Vertex::new(end.x, end.y, 0.0));
    }
    let mut out = vec![base(Geo::Poly(Poly { pts, closed: Some(false) }))];
    let (Some(&tip), Some(&nxt)) = (l.pts.first(), l.pts.get(1)) else { return out };
    let (dx, dy) = (tip.x - nxt.x, tip.y - nxt.y);
    let len = js::hypot(dx, dy);
    let asz = n(&st, "asz");
    if len > 1e-12 && asz > 0.0 {
        let (ux, uy) = (dx / len, dy / len);
        let (a, w) = (asz, asz / 6.0);
        let (bx, by) = (tip.x - ux * a, tip.y - uy * a);
        out.push(base(Geo::Hatch(Hatch {
            loops: vec![vec![tip.x, tip.y, bx - uy * w, by + ux * w, bx + uy * w, by - ux * w]],
            solid: Some(true),
        })));
    }
    let s = l.s.clone().unwrap_or_default();
    if !js::trim(&s).is_empty() {
        let h = match l.h {
            Some(h) if h > 0.0 => h,
            _ => asz,
        };
        out.push(base(Geo::Text(Text {
            x: txt.x,
            y: txt.y + h / 2.0,
            h,
            s,
            rot: Some(0.0),
            ha: Some(if dir == 1.0 { "left" } else { "right" }.into()),
            va: Some("top".into()),
            wf: Some(1.0),
            ww: l.ww.filter(|w| *w > 0.0),
            ls: l.ls.filter(|v| *v != 0.0 && !v.is_nan() && *v != 1.0),
        })));
    }
    out
}
