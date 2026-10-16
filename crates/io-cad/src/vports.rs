//! dwg-vports.js + the panes.js pieces it uses: the split screen (`doc.vports`) ↔ the VPORT table's `*ACTIVE` records.

use cc_cad::js;
use cc_io_dwg::value::{Obj, V};

use crate::jsv::{self, Ov};

const LAYOUTS: [&str; 6] = ["single", "2v", "2h", "3r", "3l", "4"];
const SCREEN_ASPECT: f64 = 1.6;

fn pane_count(layout: &str) -> usize {
    match layout {
        "2v" | "2h" => 2,
        "3r" | "3l" => 3,
        "4" => 4,
        _ => 1,
    }
}
/// `clampSplit(f)`
pub fn clamp_split(f: Ov) -> f64 {
    match jsv::fin(f) {
        Some(x) => js::max(0.1, js::min(0.9, x)),
        None => 0.5,
    }
}

/// Corners (x0, y0, x1, y1) of each pane, y up.
fn corners_of(layout: &str, x: f64, y: f64) -> Vec<[f64; 4]> {
    let yu = 1.0 - y;
    match layout {
        "2v" => vec![[0.0, 0.0, x, 1.0], [x, 0.0, 1.0, 1.0]],
        "2h" => vec![[0.0, yu, 1.0, 1.0], [0.0, 0.0, 1.0, yu]],
        "3r" => vec![[0.0, yu, x, 1.0], [0.0, 0.0, x, yu], [x, 0.0, 1.0, 1.0]],
        "3l" => vec![[0.0, 0.0, x, 1.0], [x, yu, 1.0, 1.0], [x, 0.0, 1.0, yu]],
        "4" => vec![[0.0, yu, x, 1.0], [x, yu, 1.0, 1.0], [0.0, 0.0, x, yu], [x, 0.0, 1.0, yu]],
        _ => vec![[0.0, 0.0, 1.0, 1.0]],
    }
}

fn corner(r: &V) -> Option<[f64; 4]> {
    let f = |k: &str| jsv::fin(jsv::get(Some(r), k));
    Some([f("x0")?, f("y0")?, f("x1")?, f("y1")?])
}

struct Fit {
    layout: &'static str,
    fx: f64,
    fy: f64,
    order: Vec<usize>,
}

/// `layoutFromCorners(list)`
fn layout_from_corners(list: &[&V]) -> Option<Fit> {
    let rs: Vec<[f64; 4]> = list.iter().filter_map(|r| corner(r)).filter(|c| c[2] > c[0] && c[3] > c[1]).collect();
    if rs.len() < 2 || rs.len() > 4 {
        return None;
    }
    let uniq = |vals: Vec<f64>| -> Vec<f64> {
        let mut out: Vec<f64> = Vec::new();
        for v in vals {
            let r = js::round(v * 1e4) / 1e4;
            if !out.contains(&r) {
                out.push(r);
            }
        }
        out.into_iter().filter(|v| *v > 0.001 && *v < 0.999).collect()
    };
    let xs = uniq(rs.iter().flat_map(|c| [c[0], c[2]]).collect());
    let ys = uniq(rs.iter().flat_map(|c| [c[1], c[3]]).collect());
    let x = xs.first().copied().unwrap_or(0.5);
    let y = ys.first().map_or(0.5, |v| 1.0 - v);
    for layout in LAYOUTS {
        if pane_count(layout) != rs.len() {
            continue;
        }
        let want = corners_of(layout, x, y);
        let order: Vec<Option<usize>> = want
            .iter()
            .map(|w| {
                rs.iter().position(|r| {
                    (r[0] - w[0]).abs() < 0.01 && (r[1] - w[1]).abs() < 0.01 && (r[2] - w[2]).abs() < 0.01 && (r[3] - w[3]).abs() < 0.01
                })
            })
            .collect();
        if order.iter().all(Option::is_some) {
            let order: Vec<usize> = order.into_iter().flatten().collect();
            let mut u = order.clone();
            u.sort_unstable();
            u.dedup();
            if u.len() == order.len() {
                return Some(Fit { layout, fx: clamp_split(Some(&V::Num(x))), fy: clamp_split(Some(&V::Num(y))), order });
            }
        }
    }
    None
}

/// `cleanPanes(v)`: a pane setup from a file (untrusted) → the panes.js shape, or None.
pub fn clean_panes(v: Ov) -> Option<Obj> {
    let o = jsv::obj(v).or(if let Some(V::Arr(_)) = v { None } else { None })?;
    let layout = jsv::s(o.get("layout")).filter(|l| LAYOUTS.contains(l) && *l != "single")?;
    let n = pane_count(layout);
    let mut list: Vec<V> = jsv::arr(o.get("list"))
        .iter()
        .take(n)
        .map(|p| {
            let g = |k: &str| jsv::get(Some(p), k);
            let (cx, cy, h) = (jsv::fin(g("cx")), jsv::fin(g("cy")), jsv::fin(g("h")));
            let ok = p.truthy()
                && matches!((cx, cy, h), (Some(cx), Some(cy), Some(h)) if h > 0.0 && cx.abs() < 1e15 && cy.abs() < 1e15 && h < 1e15);
            let tw = jsv::fin(g("tw")).filter(|t| t.abs() <= 100.0).unwrap_or(0.0);
            if !ok {
                return V::Null;
            }
            let mut q = Obj::new();
            q.set("cx", cx.unwrap_or(0.0));
            q.set("cy", cy.unwrap_or(0.0));
            q.set("h", h.unwrap_or(0.0));
            if tw != 0.0 {
                q.set("tw", tw);
            }
            if let Some(s) = jsv::s(g("sheet")).filter(|s| !s.is_empty()) {
                q.set("sheet", crate::text::slice16(s, 0, 255));
            }
            V::Obj(q)
        })
        .collect();
    while list.len() < n {
        list.push(V::Null);
    }
    let active = match jsv::fin(o.get("active")) {
        Some(a) if a.fract() == 0.0 && a >= 0.0 && a < jsv::us2f(n) => a,
        _ => 0.0,
    };
    let mut out = Obj::new();
    out.set("layout", layout);
    out.set("fx", clamp_split(o.get("fx")));
    out.set("fy", clamp_split(o.get("fy")));
    out.set("active", active);
    out.set("list", V::Arr(list));
    Some(out)
}

/// `vportsFrom(records)`: records read from a file (any order, the first one the active pane) → panes.js shape or None.
pub fn vports_from(records: &[V]) -> Option<Obj> {
    let rs: Vec<&V> = records
        .iter()
        .filter(|r| r.truthy() && ["x0", "y0", "x1", "y1", "cx", "cy", "h"].iter().all(|k| jsv::fin(jsv::get(Some(r), k)).is_some()))
        .collect();
    if rs.len() < 2 {
        return None;
    }
    let fit = layout_from_corners(&rs)?;
    let list: Vec<V> = fit
        .order
        .iter()
        .filter_map(|k| rs.get(*k))
        .map(|r| {
            let g = |k: &str| jsv::get(Some(r), k);
            let mut q = Obj::new();
            q.set("cx", g("cx").cloned().unwrap_or(V::Null));
            q.set("cy", g("cy").cloned().unwrap_or(V::Null));
            q.set("h", g("h").cloned().unwrap_or(V::Null));
            if let Some(tw) = jsv::fin(g("tw")).filter(|t| t.abs() > 1e-12 && t.abs() <= 100.0) {
                q.set("tw", tw);
            }
            if let Some(s) = jsv::s(g("sheet")).filter(|s| !s.is_empty()) {
                q.set("sheet", s);
            }
            V::Obj(q)
        })
        .collect();
    let active = fit.order.iter().position(|k| *k == 0).map_or(0.0, jsv::us2f);
    let mut v = Obj::new();
    v.set("layout", fit.layout);
    v.set("fx", fit.fx);
    v.set("fy", fit.fy);
    v.set("active", active);
    v.set("list", V::Arr(list));
    clean_panes(Some(&V::Obj(v)))
}

/// One VPORT record to write.
#[derive(Clone, Debug)]
pub struct Rec {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
    /// None = null (the writer fills in its own extents)
    pub cx: Option<f64>,
    pub cy: Option<f64>,
    pub h: Option<f64>,
    pub aspect: f64,
    pub tw: f64,
    pub sheet: Option<String>,
}

fn num0(x: Ov) -> f64 {
    jsv::fin(x).unwrap_or(0.0)
}

/// `vportRecords(vports, fallback, twist)`: the records to write, the active pane first.
pub fn vport_records(vports: Ov, fallback: Option<(f64, f64, f64)>, twist: f64) -> Vec<Rec> {
    let v = clean_panes(vports);
    let fb: (Option<f64>, Option<f64>, Option<f64>) = match fallback {
        Some((cx, cy, h)) if h.is_finite() && h > 0.0 => {
            (Some(if cx.is_finite() { cx } else { 0.0 }), Some(if cy.is_finite() { cy } else { 0.0 }), Some(h))
        }
        _ => (None, None, None),
    };
    let Some(v) = v else {
        return vec![Rec {
            x0: 0.0,
            y0: 0.0,
            x1: 1.0,
            y1: 1.0,
            cx: fb.0,
            cy: fb.1,
            h: fb.2,
            aspect: SCREEN_ASPECT,
            tw: if twist.is_finite() { twist } else { 0.0 },
            sheet: None,
        }];
    };
    let layout = jsv::s(v.get("layout")).unwrap_or("single");
    let corners = corners_of(layout, clamp_split(v.get("fx")), clamp_split(v.get("fy")));
    let list = jsv::arr(v.get("list"));
    let recs: Vec<Rec> = corners
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let p = list.get(i).filter(|p| p.truthy());
            let sheet = p.and_then(|p| jsv::s(jsv::get(Some(p), "sheet"))).filter(|s| !s.is_empty()).map(str::to_string);
            let model = p.filter(|_| sheet.is_none());
            let (cx, cy, h) = match model {
                Some(p) => (
                    Some(jsv::to_num(jsv::get(Some(p), "cx"))),
                    Some(jsv::to_num(jsv::get(Some(p), "cy"))),
                    Some(jsv::to_num(jsv::get(Some(p), "h"))),
                ),
                None => fb,
            };
            Rec {
                x0: c[0],
                y0: c[1],
                x1: c[2],
                y1: c[3],
                cx,
                cy,
                h,
                aspect: ((c[2] - c[0]) / (c[3] - c[1])) * SCREEN_ASPECT,
                tw: model.map_or(0.0, |p| num0(jsv::get(Some(p), "tw"))),
                sheet,
            }
        })
        .collect();
    let a = jsv::f2us(jsv::to_num(v.get("active")));
    let mut out = Vec::with_capacity(recs.len());
    if let Some(r) = recs.get(a) {
        out.push(r.clone());
    }
    out.extend(recs.iter().enumerate().filter(|(i, _)| *i != a).map(|(_, r)| r.clone()));
    out
}
