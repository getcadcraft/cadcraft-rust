//! lasso.js / lasso-core.js: lasso selection on CadCraft's geometry. Press and drag on an empty spot draws a freehand
//! lasso; its mode follows the first sideways move (right = window: wholly inside; left = crossing: inside or
//! touching); Space while dragging cycles window → crossing → fence (what the open path crosses).

use cc_cad::entity::{Entity, Geo};
use cc_cad_geom::geom::{Blocks, Bounds, EPS, Prim, dist, intersect, prim_point, prims};
use cc_cad_geom::pick::{empty_bounds, entity_bounds, fence_hits, grow_pt, point_in_poly, valid_bounds};
use cc_geom::P2;

/// The lasso's modes, in the order Space cycles them.
pub const MODES: [&str; 3] = ["window", "crossing", "fence"];

/// `nextMode(mode)`.
pub fn next_mode(m: &str) -> &'static str {
    match m {
        "window" => "crossing",
        "crossing" => "fence",
        _ => "window",
    }
}

/// `lassoMode(start, cur, minPx)` on screen points: None until the pointer has moved `min_px` sideways.
pub fn lasso_mode(start_x: f64, cur_x: f64, min_px: f64) -> Option<&'static str> {
    let dx = cur_x - start_x;
    if dx.abs() < min_px {
        return None;
    }
    Some(if dx > 0.0 { "window" } else { "crossing" })
}

/// `thin(pts, minStep)`: the pointer's path without crowded points (its last point kept).
pub fn thin(pts: &[P2], min_step: f64) -> Vec<P2> {
    let mut out: Vec<P2> = Vec::new();
    for p in pts {
        if out.last().is_none_or(|l| dist(*l, *p) >= min_step) {
            out.push(*p);
        }
    }
    if pts.len() > 1
        && let (Some(l), Some(e)) = (out.last().copied(), pts.last().copied())
        && dist(l, e) > 1e-12
    {
        out.push(e);
    }
    out
}

fn edges_of(pts: &[P2]) -> Vec<Prim> {
    let n = pts.len();
    (0..n)
        .filter_map(|i| {
            let a = *pts.get(i)?;
            let b = *pts.get(i.saturating_add(1).checked_rem(n)?)?;
            (dist(a, b) > EPS).then(|| Prim::l(a.x, a.y, b.x, b.y))
        })
        .collect()
}

/// `lassoHits(entities, pts, mode, blocks)` → the ids the lasso selects, in the entities' order.
pub fn lasso_hits(entities: &[Entity], pts: &[P2], mode: &str, blocks: Option<&Blocks>) -> Vec<u64> {
    if mode == "fence" {
        return fence_hits(entities, pts, blocks).into_iter().flatten().collect();
    }
    if pts.len() < 3 {
        return Vec::new();
    }
    let mut lb = empty_bounds();
    for p in pts {
        grow_pt(&mut lb, p.x, p.y);
    }
    let edges = edges_of(pts);
    let cuts_any = |ps: &[Prim]| ps.iter().any(|p| edges.iter().any(|ed| !intersect(p, ed, false, false).is_empty()));
    let mut out = Vec::new();
    for e in entities {
        let Some(id) = e.id else { continue };
        if e.unbounded() {
            // a construction line is never inside a lasso; crossing, it is caught where it cuts or (a ray) starts inside
            let starts_inside = match &e.geo {
                Geo::Ray(r) => point_in_poly(P2::new(r.x, r.y), pts),
                _ => false,
            };
            if mode == "crossing" && (cuts_any(&prims(e, None)) || starts_inside) {
                out.push(id);
            }
            continue;
        }
        let b: Bounds = entity_bounds(e, blocks);
        if !valid_bounds(&b) {
            continue;
        }
        if b.max_x < lb.min_x || b.min_x > lb.max_x || b.max_y < lb.min_y || b.min_y > lb.max_y {
            continue;
        }
        let box_inside = b.min_x >= lb.min_x && b.max_x <= lb.max_x && b.min_y >= lb.min_y && b.max_y <= lb.max_y;
        if mode == "window" && !box_inside {
            continue;
        }
        let point = match &e.geo {
            Geo::Point(p) => Some(P2::new(p.x, p.y)),
            _ => None,
        };
        let ps = if point.is_some() { Vec::new() } else { prims(e, blocks) };
        let cuts = cuts_any(&ps);
        if mode == "window" {
            if cuts {
                continue;
            }
            let probe: Vec<P2> = match point {
                Some(q) => vec![q],
                None if !ps.is_empty() => ps.iter().map(|q| prim_point(q, 0.0)).collect(),
                None => vec![P2::new((b.min_x + b.max_x) / 2.0, (b.min_y + b.max_y) / 2.0)],
            };
            if probe.iter().all(|q| point_in_poly(*q, pts)) {
                out.push(id);
            }
            continue;
        }
        if cuts {
            out.push(id);
            continue;
        }
        if let Some(q) = point {
            if point_in_poly(q, pts) {
                out.push(id);
            }
            continue;
        }
        if ps.iter().any(|q| point_in_poly(prim_point(q, 0.0), pts)) {
            out.push(id);
            continue;
        }
        // a lasso drawn inside a text's or a hatch's box touches it (as a crossing window does)
        if matches!(e.geo, Geo::Text(_) | Geo::Hatch(_))
            && pts.iter().any(|q| q.x >= b.min_x && q.x <= b.max_x && q.y >= b.min_y && q.y <= b.max_y)
        {
            out.push(id);
        }
    }
    out
}
