//! winsel.js — window and crossing selection in the 3D view, plan and elevation (0.4 D1): `selectionItems` (each
//! part's box as 12 edges, footings with their bars' reach), `clipSeg` (the view's cut planes), `windowSelect`
//! (through lasso-core), `rectPath`.

use crate::build::Built;
use crate::geom::V3;
use crate::js::{cos, max, min, num, num_or0, sin, string_of, truthy};
use crate::lasso_core::{Box2, Pt, lasso_select};
use crate::transform::Screen;
use serde_json::{Value, json};
use std::f64::consts::PI;

const EDGES: [[usize; 2]; 12] = [[0, 1], [1, 2], [2, 3], [3, 0], [4, 5], [5, 6], [6, 7], [7, 4], [0, 4], [1, 5], [2, 6], [3, 7]];

/// A selectable item: its ref and edges.
#[derive(Clone, Debug)]
pub struct Item {
    pub r#ref: Value,
    pub segs: Vec<[V3; 2]>,
}

fn corner(o: V3, x: V3, u: V3, v: V3, a: f64, b: f64, c: f64) -> V3 {
    [0, 1, 2].map(|i| crate::js::g3(o, i) + crate::js::g3(x, i) * a + crate::js::g3(u, i) * b + crate::js::g3(v, i) * c)
}
/// `boxSegs(o, x, u, v, xr, ur, vr)`.
pub fn box_segs(o: V3, x: V3, u: V3, v: V3, xr: [f64; 2], ur: [f64; 2], vr: [f64; 2]) -> Vec<[V3; 2]> {
    let [x0, x1] = xr;
    let [u0, u1] = ur;
    let [v0, v1] = vr;
    let k: Vec<V3> = [[x0, u0, v0], [x0, u1, v0], [x0, u1, v1], [x0, u0, v1], [x1, u0, v0], [x1, u1, v0], [x1, u1, v1], [x1, u0, v1]]
        .iter()
        .map(|&[a, b, c]| corner(o, x, u, v, a, b, c))
        .collect();
    EDGES.iter().filter_map(|&[i, j]| Some([*k.get(i)?, *k.get(j)?])).collect()
}

/// `selectionItems(state, built, hiddenPhases)`.
pub fn selection_items(state: &Value, built: &Built, hidden: Option<&[f64]>) -> Vec<Item> {
    let mut items: Vec<Item> = Vec::new();
    let mut by: Vec<String> = Vec::new();
    let mut push = |items: &mut Vec<Item>, r: Value, segs: Vec<[V3; 2]>| {
        let key = format!("{}:{}", string_of(r.get("kind")), string_of(r.get("id")));
        match by.iter().position(|k| *k == key) {
            Some(i) => {
                if let Some(it) = items.get_mut(i) {
                    it.segs.extend(segs);
                }
            }
            None => {
                by.push(key);
                items.push(Item { r#ref: r, segs });
            }
        }
    };
    let shown = |ph: f64| hidden.is_none_or(|h| !h.contains(&crate::js::or(ph, 1.0)));
    for p in &built.parts {
        if !shown(p.phase) {
            continue;
        }
        let f = &p.frame;
        let (w, h) = (p.profile.width / 2.0, p.profile.depth / 2.0);
        let segs = box_segs(f.o, f.x, f.u, f.v, [0.0, p.length], [-w, w], [p.voff - h, p.voff + h]);
        let r = match (&p.member, &p.stair) {
            (Some(m), _) => json!({ "kind": "member", "id": m }),
            (None, Some(s)) => json!({ "kind": "stair", "id": s }),
            _ => json!({ "kind": "conn", "id": p.conn }),
        };
        push(&mut items, r, segs);
    }
    for fv in crate::js::arr(state.get("footings")) {
        if !shown(crate::js::f(fv.get("phase"))) {
            continue;
        }
        let a = num_or0(fv.get("rotation")) * PI / 180.0;
        let x = [cos(a), sin(a), 0.0];
        let u = [-sin(a), cos(a), 0.0];
        let g = |k: &str| num(fv.get(k));
        let st = fv.get("starters");
        let top = if truthy(st.and_then(|s| s.get("on"))) { g("top") + num_or0(st.and_then(|s| s.get("projection"))) } else { g("top") };
        let segs = box_segs(
            [g("x"), g("y"), 0.0],
            x,
            u,
            [0.0, 0.0, 1.0],
            [-g("L") / 2.0, g("L") / 2.0],
            [-g("B") / 2.0, g("B") / 2.0],
            [g("top") - g("H"), top],
        );
        push(&mut items, json!({ "kind": "footing", "id": fv.get("id") }), segs);
    }
    items
}

/// A cut plane: n·p + d ≥ 0 is kept.
#[derive(Clone, Copy, Debug)]
pub struct Plane {
    pub n: V3,
    pub d: f64,
}
fn side(pl: &Plane, p: V3) -> f64 {
    pl.n[0] * p[0] + pl.n[1] * p[1] + pl.n[2] * p[2] + pl.d
}
/// `clipSeg(a, b, planes)`.
pub fn clip_seg(a: V3, b: V3, planes: &[Plane]) -> Option<[V3; 2]> {
    let (mut t0, mut t1) = (0.0, 1.0);
    for pl in planes {
        let (da, db) = (side(pl, a), side(pl, b));
        if da < 0.0 && db < 0.0 {
            return None;
        }
        if da >= 0.0 && db >= 0.0 {
            continue;
        }
        let t = da / (da - db);
        if da < 0.0 {
            t0 = max(t0, t);
        } else {
            t1 = min(t1, t);
        }
        if t0 > t1 {
            return None;
        }
    }
    let at = |t: f64| [0, 1, 2].map(|i| crate::js::g3(a, i) + (crate::js::g3(b, i) - crate::js::g3(a, i)) * t);
    Some([at(t0), at(t1)])
}

/// `windowSelect(items, project, path, mode, planes)` → the refs picked.
pub fn window_select(items: &[Item], project: &dyn Fn(V3) -> Screen, path: &[Pt], mode: &str, planes: &[Plane]) -> Vec<Value> {
    struct Flat {
        r: Value,
        segs: Vec<[f64; 4]>,
        bx: Box2,
    }
    let mut flat = Vec::new();
    for it in items {
        let mut segs = Vec::new();
        for [a, b] in &it.segs {
            let Some(c) = clip_seg(*a, *b, planes) else { continue };
            let (p, q) = (project(c[0]), project(c[1]));
            if p.behind || q.behind {
                continue;
            }
            segs.push([p.x, p.y, q.x, q.y]);
        }
        if segs.is_empty() {
            continue;
        }
        let (mut mnx, mut mny, mut mxx, mut mxy) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for [x1, y1, x2, y2] in &segs {
            mnx = min(min(mnx, *x1), *x2);
            mny = min(min(mny, *y1), *y2);
            mxx = max(max(mxx, *x1), *x2);
            mxy = max(max(mxy, *y1), *y2);
        }
        flat.push(Flat { r: it.r#ref.clone(), segs, bx: Box2 { min_x: mnx, min_y: mny, max_x: mxx, max_y: mxy } });
    }
    let picked = lasso_select(&flat, path, mode, &|f: &Flat| Some(f.bx), &|f: &Flat| f.segs.clone(), &|_| true);
    picked.into_iter().filter_map(|i| flat.get(i).map(|f| f.r.clone())).collect()
}

/// `rectPath(a, b)`.
pub fn rect_path(a: Pt, b: Pt) -> Vec<Pt> {
    vec![Pt { x: a.x, y: a.y }, Pt { x: b.x, y: a.y }, Pt { x: b.x, y: b.y }, Pt { x: a.x, y: b.y }]
}
