//! drawings.js — general-arrangement plans and elevations, the anchor bolt plan and assembly drawings made from the
//! built model, as display lists (one landscape sheet each, mm, y up). Nothing is stored: the set is made again from
//! the model each time; title blocks carry the model ref (CRC-32 of the model text), sheets a content hash for the
//! issue log, prints of the parts they draw for revision clouds. Hidden lines (0.4 D12), weld symbols (0.4 D8).
//!
//! The JS keeps the sheet being drawn on in module globals (SHEET, AREA, HIDDEN) that `drawingSet` sets for one set;
//! here they travel in a [`Ctx`].

use crate::build::{Built, Hole, Part};
use crate::display::{Item, Pen, To};
use crate::geom::{V3, add, dot, mul, to_local, to_world};
use crate::i18n::{F, t, t0};
use crate::js::{Obj, atan2, cos, hypot, jnum, max, max_of, min, min_of, num_to_string, round, sin, to_fixed};
use crate::marks::Marks;
use crate::paper::{DEFAULT_PAPER, Paper, paper_of};
use crate::pdf::text_width;
use crate::state::Conn;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::f64::consts::PI;

pub const SCALES: [f64; 15] = [1.0, 2.0, 5.0, 10.0, 20.0, 25.0, 50.0, 100.0, 200.0, 250.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0];
/// `pickScale(size, room)`.
pub fn pick_scale(size: f64, room: f64) -> f64 {
    SCALES.iter().copied().find(|n| size / n <= room).unwrap_or(10000.0)
}
fn ratio(n: f64) -> String {
    format!("1:{}", num_to_string(n))
}
fn fmt(v: f64) -> String {
    num_to_string(round(v))
}
const PLAN_BAND: f64 = 1000.0;
const ELEV_BAND: f64 = 1000.0;

/// The drawing area above the title block band.
#[derive(Clone, Copy, Debug)]
pub struct Area {
    pub x0: f64,
    pub x1: f64,
    pub y0: f64,
    pub y1: f64,
}
/// The sheet being drawn on.
#[derive(Clone, Copy, Debug)]
pub struct Ctx {
    pub sheet: Paper,
    pub area: Area,
    pub hidden: bool,
}
impl Ctx {
    pub fn new(paper: &str, hidden: bool) -> Ctx {
        let p = paper_of(paper);
        Ctx { sheet: p, area: Area { x0: 15.0, x1: p.w - 15.0, y0: 58.0, y1: p.h - 27.0 }, hidden }
    }
}

fn st_outline() -> Pen {
    Pen::new(0.35)
}
fn st_thick() -> Pen {
    Pen::new(0.5)
}
fn st_thin() -> Pen {
    Pen::new(0.18)
}
fn st_hidden() -> Pen {
    Pen::dashed(0.18, &[1.5, 1.0])
}
fn st_center() -> Pen {
    Pen::dashed(0.13, &[4.0, 1.0, 0.6, 1.0])
}
fn st_grid() -> Pen {
    Pen::dashed(0.18, &[8.0, 1.5, 1.0, 1.5])
}
fn st_footing() -> Pen {
    Pen::dashed(0.18, &[2.0, 1.2])
}
fn st_level() -> Pen {
    Pen::dashed(0.18, &[5.0, 1.5])
}

/// A paper box.
#[derive(Clone, Copy, Debug)]
pub struct BoxP {
    pub x0: f64,
    pub x1: f64,
    pub y0: f64,
    pub y1: f64,
}
fn overlap(a: &BoxP, b: &BoxP) -> bool {
    a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1
}
/// `textBox(s, x, y, h, o)`.
fn text_box(s: &str, x: f64, y: f64, h: f64, o: To) -> BoxP {
    let w = text_width(s, h, o.bold);
    let a = if o.rot == 0.0 || o.rot.is_nan() { 0.0 } else { o.rot };
    let (c, sn) = (cos(a), sin(a));
    let dx = match o.align {
        "center" => -w / 2.0,
        "right" => -w,
        _ => 0.0,
    };
    let pts: Vec<[f64; 2]> = [[dx, -0.25 * h], [dx + w, -0.25 * h], [dx + w, h], [dx, h]]
        .iter()
        .map(|&[u, v]| [x + u * c - v * sn, y + u * sn + v * c])
        .collect();
    BoxP {
        x0: min_of(pts.iter().map(|p| p[0])) - 0.4,
        x1: max_of(pts.iter().map(|p| p[0])) + 0.4,
        y0: min_of(pts.iter().map(|p| p[1])) - 0.4,
        y1: max_of(pts.iter().map(|p| p[1])) + 0.4,
    }
}

/// A region (where a part or footing is drawn), for revision clouds.
#[derive(Clone, Debug)]
pub struct Region {
    pub key: String,
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}
/// A drawing dimension.
#[derive(Clone, Debug)]
pub struct DDim {
    pub dir: &'static str,
    pub value: f64,
    pub view: &'static str,
}
/// A weld symbol placed.
#[derive(Clone, Debug)]
pub struct WeldSym {
    pub x: f64,
    pub y: f64,
    pub size: f64,
    pub all_around: bool,
}

/// `sheetBuilder()`.
#[derive(Default)]
pub struct S {
    pub items: Vec<Item>,
    pub dims: Vec<DDim>,
    pub labels: Vec<String>,
    pub regions: Vec<Region>,
    pub taken: Vec<BoxP>,
    pub welds: Vec<WeldSym>,
}
impl S {
    fn region(&mut self, key: String, pts: &[[f64; 2]]) {
        if pts.is_empty() {
            return;
        }
        self.regions.push(Region {
            key,
            x0: min_of(pts.iter().map(|q| q[0])),
            y0: min_of(pts.iter().map(|q| q[1])),
            x1: max_of(pts.iter().map(|q| q[0])),
            y1: max_of(pts.iter().map(|q| q[1])),
        });
    }
    fn p(&mut self, pts: Vec<[f64; 2]>, pen: &Pen, closed: bool) {
        if pts.len() >= 2 {
            self.items.push(Item::Path { pts, w: pen.w, dash: pen.dash.clone(), closed, fill: false });
        }
    }
    fn pf(&mut self, pts: Vec<[f64; 2]>, pen: &Pen, closed: bool, fill: bool) {
        if pts.len() >= 2 {
            self.items.push(Item::Path { pts, w: pen.w, dash: pen.dash.clone(), closed, fill });
        }
    }
    fn t(&mut self, s: &str, x: f64, y: f64, h: f64, o: To) {
        self.items.push(Item::Text {
            s: s.to_string(),
            x,
            y,
            h,
            align: if o.align.is_empty() { "left" } else { o.align },
            rot: o.rot,
            bold: o.bold,
        });
    }
    fn c(&mut self, c: [f64; 2], r: f64, pen: &Pen) {
        self.items.push(Item::Circle { c, r, w: pen.w, dash: pen.dash.clone(), fill: Some(false) });
    }
    fn rect(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, pen: &Pen) {
        self.p(vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]], pen, true);
    }
    #[allow(clippy::too_many_arguments)]
    fn dim_h(&mut self, x1: f64, x2: f64, y: f64, from: f64, value: f64, view: &'static str, out: f64, mut avoid: Option<&mut Vec<BoxP>>) {
        let sg = if y < from { -1.0 } else { 1.0 };
        let thin = st_thin();
        self.p(vec![[x1, from], [x1, y + sg * 1.5]], &thin, false);
        self.p(vec![[x2, from], [x2, y + sg * 1.5]], &thin, false);
        self.p(vec![[x1, y], [x2, y]], &thin, false);
        for x in [x1, x2] {
            self.p(vec![[x - 0.9, y - 0.9], [x + 0.9, y + 0.9]], &Pen::new(0.35), false);
        }
        let label = fmt(value);
        let tw = text_width(&label, 2.2, false);
        let fits = (x2 - x1).abs() >= tw + 1.0;
        let tx = if fits || out == 0.0 {
            (x1 + x2) / 2.0
        } else if out < 0.0 {
            min(x1, x2) - tw / 2.0 - 1.2
        } else {
            max(x1, x2) + tw / 2.0 + 1.2
        };
        let mut ty = y + if fits || out != 0.0 { 0.8 } else { -3.2 };
        if let Some(av) = avoid.as_mut() {
            let mut i = 0;
            while i < 3 && av.iter().any(|b| overlap(b, &text_box(&label, tx, ty, 2.2, To::center()))) {
                ty += sg * 2.8;
                i += 1;
            }
            av.push(text_box(&label, tx, ty, 2.2, To::center()));
        }
        self.t(&label, tx, ty, 2.2, To::center());
        self.dims.push(DDim { dir: "h", value: round(value), view });
    }
    #[allow(clippy::too_many_arguments)]
    fn dim_v(&mut self, y1: f64, y2: f64, x: f64, from: f64, value: f64, view: &'static str, out: f64, mut avoid: Option<&mut Vec<BoxP>>) {
        let sg = if x < from { -1.0 } else { 1.0 };
        let thin = st_thin();
        self.p(vec![[from, y1], [x + sg * 1.5, y1]], &thin, false);
        self.p(vec![[from, y2], [x + sg * 1.5, y2]], &thin, false);
        self.p(vec![[x, y1], [x, y2]], &thin, false);
        for y in [y1, y2] {
            self.p(vec![[x - 0.9, y - 0.9], [x + 0.9, y + 0.9]], &Pen::new(0.35), false);
        }
        let label = fmt(value);
        let tw = text_width(&label, 2.2, false);
        let fits = (y2 - y1).abs() >= tw + 1.0;
        let ty = if fits || out == 0.0 {
            (y1 + y2) / 2.0
        } else if out < 0.0 {
            min(y1, y2) - tw / 2.0 - 1.2
        } else {
            max(y1, y2) + tw / 2.0 + 1.2
        };
        let mut tx = x - 0.8;
        let o = To::center().rot(PI / 2.0);
        if let Some(av) = avoid.as_mut() {
            let mut i = 0;
            while i < 3 && av.iter().any(|b| overlap(b, &text_box(&label, tx, ty, 2.2, o))) {
                tx += sg * 2.8;
                i += 1;
            }
            av.push(text_box(&label, tx, ty, 2.2, o));
        }
        self.t(&label, tx, ty, 2.2, o);
        self.dims.push(DDim { dir: "v", value: round(value), view });
    }
    fn chain_h(&mut self, vals: &[f64], xf: &dyn Fn(f64) -> f64, y: f64, from: f64, step: f64, view: &'static str) {
        if vals.len() < 2 {
            return;
        }
        let (first, last) = (vals.first().copied().unwrap_or(f64::NAN), vals.last().copied().unwrap_or(f64::NAN));
        let mid = (first + last) / 2.0;
        let mut avoid = Vec::new();
        if vals.len() > 2 {
            for w in vals.windows(2) {
                let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else { continue };
                self.dim_h(xf(a), xf(b), y, from, b - a, view, if (a + b) / 2.0 < mid { -1.0 } else { 1.0 }, Some(&mut avoid));
            }
        }
        self.dim_h(xf(first), xf(last), if vals.len() > 2 { y + step } else { y }, from, last - first, view, 0.0, None);
    }
    fn chain_v(&mut self, vals: &[f64], yf: &dyn Fn(f64) -> f64, x: f64, from: f64, step: f64, view: &'static str) {
        if vals.len() < 2 {
            return;
        }
        let (first, last) = (vals.first().copied().unwrap_or(f64::NAN), vals.last().copied().unwrap_or(f64::NAN));
        let mid = (first + last) / 2.0;
        let mut avoid = Vec::new();
        if vals.len() > 2 {
            for w in vals.windows(2) {
                let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else { continue };
                self.dim_v(yf(a), yf(b), x, from, b - a, view, if (a + b) / 2.0 < mid { -1.0 } else { 1.0 }, Some(&mut avoid));
            }
        }
        self.dim_v(yf(first), yf(last), if vals.len() > 2 { x + step } else { x }, from, last - first, view, 0.0, None);
    }
    fn label(&mut self, s: &str, x: f64, y: f64, h: f64, o: To) {
        self.t(s, x, y, h, o);
        self.labels.push(s.to_string());
        self.taken.push(text_box(s, x, y, h, o));
    }
    fn label_free(&mut self, s: &str, cands: &[(f64, f64, To)], h: f64) {
        let free = cands.iter().find(|(x, y, o)| {
            let b = text_box(s, *x, *y, h, *o);
            !self.taken.iter().any(|t| overlap(t, &b))
        });
        if let Some(&(x, y, o)) = free.or_else(|| cands.first()) {
            self.label(s, x, y, h, o);
        }
    }
    fn weld_symbol(&mut self, tip: [f64; 2], size: f64, all_around: bool, dir: f64) {
        let (kx, ky) = (tip[0] + dir * 5.0, tip[1] + 6.0);
        self.p(vec![tip, [kx, ky]], &Pen::new(0.18), false);
        let a = atan2(ky - tip[1], kx - tip[0]);
        let (ah, aw) = (1.6, 0.55);
        self.pf(
            vec![
                tip,
                [tip[0] + ah * cos(a) - aw * sin(a), tip[1] + ah * sin(a) + aw * cos(a)],
                [tip[0] + ah * cos(a) + aw * sin(a), tip[1] + ah * sin(a) - aw * cos(a)],
            ],
            &Pen::new(0.18),
            true,
            true,
        );
        let rx = kx + dir * 14.0;
        self.p(vec![[kx, ky], [rx, ky]], &Pen::new(0.18), false);
        let t0 = min(kx, rx) + 6.0;
        self.p(vec![[t0, ky], [t0, ky - 2.6], [t0 + 2.6, ky]], &Pen::new(0.18), true);
        self.t(&num_to_string(size), t0 - 0.8, ky - 2.4, 2.0, To::right());
        self.taken.push(BoxP { x0: min(kx, rx) - 0.5, x1: max(kx, rx) + 0.5, y0: ky - 3.2, y1: ky + 0.5 });
        if all_around {
            self.c([kx, ky], 1.2, &Pen::new(0.18));
        }
        self.welds.push(WeldSym { x: tip[0], y: tip[1], size, all_around });
    }
}

/// `uniq(vals, tol)` (finite values only).
pub fn uniq(vals: impl IntoIterator<Item = f64>, tol: f64) -> Vec<f64> {
    let mut s: Vec<f64> = vals.into_iter().filter(|v| v.is_finite()).collect();
    s.sort_by(|a, b| crate::js::ord(a - b));
    let mut o: Vec<f64> = Vec::new();
    for v in s {
        if o.last().is_none_or(|l| v - l > tol) {
            o.push(v);
        }
    }
    o
}

/// `hull(pts)` (Andrew's monotone chain, counter-clockwise).
pub fn hull(pts: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut p = pts.to_vec();
    p.sort_by(|a, b| {
        let c = a[0] - b[0];
        crate::js::ord(if c == 0.0 || c.is_nan() { a[1] - b[1] } else { c })
    });
    if p.len() < 3 {
        return p;
    }
    let cr = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut lo: Vec<[f64; 2]> = Vec::new();
    let mut up: Vec<[f64; 2]> = Vec::new();
    for &q in &p {
        while let (Some(&o), Some(&a)) = (lo.len().checked_sub(2).and_then(|i| lo.get(i)), lo.last())
            && cr(o, a, q) <= 1e-9
        {
            lo.pop();
        }
        lo.push(q);
    }
    for &q in p.iter().rev() {
        while let (Some(&o), Some(&a)) = (up.len().checked_sub(2).and_then(|i| up.get(i)), up.last())
            && cr(o, a, q) <= 1e-9
        {
            up.pop();
        }
        up.push(q);
    }
    lo.pop();
    up.pop();
    lo.extend(up);
    lo
}

/// A run of a polyline, behind an occluder or not.
#[derive(Clone, Debug)]
pub struct Run {
    pub pts: Vec<[f64; 2]>,
    pub hidden: bool,
}
fn seg_x(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> Option<f64> {
    let r = [b[0] - a[0], b[1] - a[1]];
    let q = [d[0] - c[0], d[1] - c[1]];
    let den = r[0] * q[1] - r[1] * q[0];
    if den.abs() < 1e-12 {
        return None;
    }
    let tt = ((c[0] - a[0]) * q[1] - (c[1] - a[1]) * q[0]) / den;
    let u = ((c[0] - a[0]) * r[1] - (c[1] - a[1]) * r[0]) / den;
    if (-1e-9..=1.0 + 1e-9).contains(&u) { Some(tt) } else { None }
}
fn in_poly(p: [f64; 2], poly: &[[f64; 2]]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        if let (Some(a), Some(b)) = (poly.get(i), poly.get(j))
            && (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < ((b[0] - a[0]) * (p[1] - a[1])) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}
fn edge_dist(p: [f64; 2], poly: &[[f64; 2]]) -> f64 {
    let mut best = f64::INFINITY;
    let n = poly.len();
    for i in 0..n {
        let (Some(&a), Some(&b)) = (poly.get(i), poly.get((i + 1) % n)) else { continue };
        let ab = [b[0] - a[0], b[1] - a[1]];
        let l2 = ab[0] * ab[0] + ab[1] * ab[1];
        let k = if l2 != 0.0 && !l2.is_nan() { max(0.0, min(1.0, ((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / l2)) } else { 0.0 };
        best = min(best, hypot(p[0] - a[0] - ab[0] * k, p[1] - a[1] - ab[1] * k));
    }
    best
}
/// `runsBehind(pts, closed, occluders)`.
pub fn runs_behind(pts: &[[f64; 2]], closed: bool, occluders: &[Vec<[f64; 2]>]) -> Vec<Run> {
    let n = pts.len();
    let mut segs = Vec::new();
    let m = if closed { n } else { n.saturating_sub(1) };
    for i in 0..m {
        if let (Some(&a), Some(&b)) = (pts.get(i), pts.get((i + 1) % n)) {
            segs.push((a, b));
        }
    }
    let mut out: Vec<Run> = Vec::new();
    for (a, b) in segs {
        let mut ts = vec![0.0, 1.0];
        for o in occluders {
            let k = o.len();
            for i in 0..k {
                if let (Some(&c), Some(&d)) = (o.get(i), o.get((i + 1) % k))
                    && let Some(tt) = seg_x(a, b, c, d)
                    && tt > 1e-9
                    && tt < 1.0 - 1e-9
                {
                    ts.push(tt);
                }
            }
        }
        ts.sort_by(|x, y| crate::js::ord(x - y));
        for w in ts.windows(2) {
            let (Some(&t0), Some(&t1)) = (w.first(), w.get(1)) else { continue };
            if t1 - t0 < 1e-9 {
                continue;
            }
            let pp = |tt: f64| [a[0] + (b[0] - a[0]) * tt, a[1] + (b[1] - a[1]) * tt];
            let mp = pp((t0 + t1) / 2.0);
            let hidden = occluders.iter().any(|o| in_poly(mp, o) && edge_dist(mp, o) > 0.05);
            let (pa, pb) = (pp(t0), pp(t1));
            match out.last_mut() {
                Some(last) if last.hidden == hidden && last.pts.last().is_some_and(|q| hypot(q[0] - pa[0], q[1] - pa[1]) < 1e-9) => {
                    last.pts.push(pb)
                }
                _ => out.push(Run { pts: vec![pa, pb], hidden }),
            }
        }
    }
    out
}
fn draw_hidden(ctx: &Ctx, s: &mut S, pts: Vec<[f64; 2]>, style: &Pen, closed: bool, occluders: &[Vec<[f64; 2]>], hidden: Option<&Pen>) {
    if occluders.is_empty() || !ctx.hidden {
        s.p(pts, style, closed);
        return;
    }
    let runs = runs_behind(&pts, closed, occluders);
    if runs.iter().all(|r| !r.hidden) {
        s.p(pts, style, closed);
        return;
    }
    for r in runs {
        if !r.hidden {
            s.p(r.pts, style, false);
        } else if let Some(h) = hidden {
            s.p(r.pts, h, false);
        }
    }
}

fn box_corners(part: &Part, t0: f64, t1: f64) -> Vec<V3> {
    let (w, d) = (part.profile.width / 2.0, part.profile.depth / 2.0);
    let mut out = Vec::with_capacity(8);
    for tt in [t0, t1] {
        for [u, v] in [[-w, -d], [w, -d], [w, d], [-w, d]] {
            out.push(to_world(&part.frame, tt, u, v, part.voff));
        }
    }
    out
}

/// A hole's center, axis and the thickness it goes through.
#[derive(Clone, Copy, Debug)]
pub struct HoleG {
    pub c: V3,
    pub axis: V3,
    pub t: f64,
    pub d: f64,
}
fn ft(p: &crate::profiles::Profile, face: &str) -> f64 {
    crate::profiles::face_thickness(p, face).unwrap_or(f64::NAN)
}
/// `holeGeom(part, h)`.
pub fn hole_geom(part: &Part, h: &Hole) -> HoleG {
    let p = &part.profile;
    let f = &part.frame;
    let (mut u, mut v) = (0.0, 0.0);
    let axis;
    if h.face == "web" {
        v = h.y;
        if p.kind == "C" {
            u = -p.bf() / 2.0 + p.tw() / 2.0;
        }
        if p.kind == "L" {
            u = -p.b() / 2.0 + p.t() / 2.0;
        }
        axis = f.u;
    } else if h.face == "plate" {
        u = h.y;
        axis = f.v;
    } else {
        u = h.y;
        let tt = ft(p, &h.face);
        v = (if h.face == "top" { 1.0 } else { -1.0 }) * (p.depth / 2.0 - tt / 2.0);
        axis = f.v;
    }
    HoleG { c: to_world(f, h.x, u, v, part.voff), axis, t: ft(p, if h.face == "plate" { "web" } else { &h.face }), d: h.d }
}

fn frame_of_sheet(ctx: &Ctx, s: &mut S) {
    s.rect(10.0, 10.0, ctx.sheet.w - 10.0, ctx.sheet.h - 10.0, &st_thick());
}

struct Tb {
    title: String,
    subtitle: String,
    number: String,
    scale: String,
    model: String,
    r#ref: String,
    date: String,
    sheet: String,
}
fn title_block(ctx: &Ctx, s: &mut S, tb: &Tb) {
    let (x0, y0, x1, y1, mid) = (ctx.sheet.w - 170.0, 10.0, ctx.sheet.w - 10.0, 52.0, ctx.sheet.w - 90.0);
    s.rect(x0, y0, x1, y1, &st_thick());
    s.t(&tb.title, x0 + 2.0, y1 - 6.0, 3.6, To::left().bold());
    if !tb.subtitle.is_empty() {
        s.t(&tb.subtitle, x0 + 2.0, y1 - 10.5, 2.2, To::left());
    }
    let rows = [
        [t0("sheet.tb.drawing"), tb.number.clone(), t0("sheet.tb.scale"), tb.scale.clone()],
        [t0("sheet.tb.model"), tb.model.clone(), t0("sheet.tb.model-ref"), tb.r#ref.clone()],
        [t0("sheet.tb.date"), tb.date.clone(), t0("sheet.tb.sheet"), tb.sheet.clone()],
    ];
    s.p(vec![[x0, y1 - 12.5], [x1, y1 - 12.5]], &st_thin(), false);
    for (i, r) in rows.iter().enumerate() {
        let y = y1 - 12.5 - (i as f64 + 1.0) * 8.5;
        s.p(vec![[x0, y], [x1, y]], &st_thin(), false);
        s.t(&r[0], x0 + 2.0, y + 5.6, 1.8, To::left());
        s.t(&r[1], x0 + 2.0, y + 1.6, 2.6, if i == 0 { To::left().bold() } else { To::left() });
        s.t(&r[2], mid + 2.0, y + 5.6, 1.8, To::left());
        s.t(&r[3], mid + 2.0, y + 1.6, 2.6, To::left());
    }
    s.p(vec![[mid, y0 + 2.5], [mid, y1 - 12.5]], &st_thin(), false);
    s.t(&t0("sheet.tb.footer"), x0 + 2.0, y0 + 1.0, 1.7, To::left());
}

// ---------------------------------------------------------------------------------------------
// general arrangement

#[derive(Clone, Copy)]
struct View {
    elevation: bool,
    r: V3,
    u: V3,
    n: V3,
    a: V3,
    lo: f64,
    hi: f64,
    scale_hint: f64,
}

#[derive(Clone)]
struct Prim {
    is_loop: bool,
    pts: Vec<[f64; 2]>,
    style: Pen,
    part: Option<usize>,
    owner: Option<usize>,
    footing: Option<usize>,
    tag: String,
    along: Option<[[f64; 2]; 2]>,
    depth: Option<[f64; 2]>,
}

fn clip_axis(part: &Part, a: V3, lo: f64, hi: f64) -> Option<[f64; 2]> {
    let s0 = dot(part.frame.o, a);
    let k = dot(part.frame.x, a);
    let l = part.length;
    if k.abs() < 1e-9 {
        return if s0 >= lo - 1e-6 && s0 <= hi + 1e-6 { Some([0.0, l]) } else { None };
    }
    let (mut aa, mut bb) = ((lo - s0) / k, (hi - s0) / k);
    if aa > bb {
        std::mem::swap(&mut aa, &mut bb);
    }
    let t0 = max(0.0, aa);
    let t1 = min(l, bb);
    if t1 - t0 > 1e-6 || (t1 >= t0 && l < 1e-6) { Some([t0, t1]) } else { None }
}

/// Footing marks by footing id (`marks.footingMark`).
pub type FootingMarks = [(String, String)];
fn fmark<'a>(fm: Option<&'a FootingMarks>, id: &str) -> Option<&'a str> {
    fm.and_then(|m| m.iter().rev().find(|(k, _)| k == id)).map(|(_, v)| v.as_str())
}

fn ga_content(built: &Built, marks: &Marks, fm: Option<&FootingMarks>, view: &View) -> (Vec<Prim>, Option<[f64; 4]>) {
    let mut prims = Vec::new();
    let pv = |p: V3| [dot(p, view.r), dot(p, view.u)];
    let depth_of = |pts: &[V3]| {
        let d: Vec<f64> = pts.iter().map(|p| dot(*p, view.n)).collect();
        [min_of(d.iter().copied()), max_of(d.iter().copied())]
    };
    for (pi, part) in built.parts.iter().enumerate() {
        let x = part.frame.x;
        let par = dot(x, view.n).abs();
        let is_main = marks.asm_of(&part.id) == Some(part.id.as_str());
        let tag = if is_main { marks.asm_mark(&part.id).unwrap_or("").to_string() } else { String::new() };
        if !view.elevation && par > 0.02 && par < 0.98 {
            let top = if part.start[2] >= part.end[2] { part.start } else { part.end };
            if top[2] < view.lo || top[2] > view.hi {
                continue;
            }
            let a = pv(to_world(&part.frame, 0.0, 0.0, 0.0, part.voff));
            let b = pv(to_world(&part.frame, part.length, 0.0, 0.0, part.voff));
            prims.push(Prim {
                is_loop: false,
                pts: vec![a, b],
                style: Pen::dashed(0.35, &[3.0, 1.5]),
                part: Some(pi),
                owner: None,
                footing: None,
                tag,
                along: Some([a, b]),
                depth: Some(depth_of(&box_corners(part, 0.0, part.length))),
            });
            continue;
        }
        let Some(r) = clip_axis(part, view.a, view.lo, view.hi) else { continue };
        let depth = depth_of(&box_corners(part, r[0], r[1]));
        if view.elevation && par < 0.98 && r[1] - r[0] < part.length - 1e-6 {
            continue;
        }
        if par >= 0.98 {
            let tm = (r[0] + r[1]) / 2.0;
            for lp in &part.profile.loops {
                prims.push(Prim {
                    is_loop: true,
                    pts: lp.iter().map(|&[u, v]| pv(to_world(&part.frame, tm, u, v, part.voff))).collect(),
                    style: st_outline(),
                    part: Some(pi),
                    owner: None,
                    footing: None,
                    tag: tag.clone(),
                    along: None,
                    depth: Some(depth),
                });
            }
            continue;
        }
        let corners: Vec<[f64; 2]> = box_corners(part, r[0], r[1]).into_iter().map(pv).collect();
        let pts = hull(&corners);
        let a = pv(to_world(&part.frame, r[0], 0.0, 0.0, part.voff));
        let b = pv(to_world(&part.frame, r[1], 0.0, 0.0, part.voff));
        let across = hull_width(&pts, a, b);
        if across / view.scale_hint < 0.8 {
            prims.push(Prim {
                is_loop: false,
                pts: vec![a, b],
                style: st_thick(),
                part: Some(pi),
                owner: None,
                footing: None,
                tag: tag.clone(),
                along: Some([a, b]),
                depth: Some(depth),
            });
        } else {
            prims.push(Prim {
                is_loop: true,
                pts,
                style: st_outline(),
                part: Some(pi),
                owner: None,
                footing: None,
                tag: tag.clone(),
                along: Some([a, b]),
                depth: Some(depth),
            });
        }
        if (part.profile.kind == "I" || part.profile.kind == "C") && dot(part.frame.v, view.n).abs() < 0.02 {
            for sg in [1.0, -1.0] {
                let v = sg * (part.profile.depth / 2.0 - part.profile.tf());
                prims.push(Prim {
                    is_loop: false,
                    pts: vec![pv(to_world(&part.frame, r[0], 0.0, v, part.voff)), pv(to_world(&part.frame, r[1], 0.0, v, part.voff))],
                    style: st_thin(),
                    part: None,
                    owner: Some(pi),
                    footing: None,
                    tag: String::new(),
                    along: None,
                    depth: Some(depth),
                });
            }
        }
    }
    for (fi, bf) in built.footings.iter().enumerate() {
        let f = &bf.f;
        let rot = crate::js::or(f.rotation, 0.0) * PI / 180.0;
        let (c, s) = (cos(rot), sin(rot));
        let zs = [f.top - f.h, f.top];
        if !view.elevation {
            if f.top < view.lo || f.top - f.h > view.hi {
                continue;
            }
        } else if (dot([f.x, f.y, 0.0], view.a) - (view.lo + view.hi) / 2.0).abs() > ELEV_BAND + max(f.l, f.b) / 2.0 {
            continue;
        }
        let mut corners = Vec::new();
        for z in zs {
            for [a, b] in [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]] {
                corners.push(pv([f.x + c * a * f.l / 2.0 - s * b * f.b / 2.0, f.y + s * a * f.l / 2.0 + c * b * f.b / 2.0, z]));
            }
        }
        let tag = if view.elevation { String::new() } else { fmark(fm, &f.id).filter(|m| !m.is_empty()).unwrap_or("").to_string() };
        prims.push(Prim {
            is_loop: true,
            pts: hull(&corners),
            style: st_footing(),
            part: None,
            owner: None,
            footing: Some(fi),
            tag,
            along: None,
            depth: None,
        });
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for pr in &prims {
        for &[x, y] in &pr.pts {
            x0 = min(x0, x);
            x1 = max(x1, x);
            y0 = min(y0, y);
            y1 = max(y1, y);
        }
    }
    (prims, if x0.is_finite() { Some([x0, y0, x1, y1]) } else { None })
}

fn hull_width(pts: &[[f64; 2]], a: [f64; 2], b: [f64; 2]) -> f64 {
    let l = hypot(b[0] - a[0], b[1] - a[1]);
    if l < 1e-9 {
        return f64::INFINITY;
    }
    let (nx, ny) = (-(b[1] - a[1]) / l, (b[0] - a[0]) / l);
    let d: Vec<f64> = pts.iter().map(|&[x, y]| (x - a[0]) * nx + (y - a[1]) * ny).collect();
    max_of(d.iter().copied()) - min_of(d.iter().copied())
}

fn bb4(q: &[[f64; 2]]) -> [f64; 4] {
    [min_of(q.iter().map(|p| p[0])), min_of(q.iter().map(|p| p[1])), max_of(q.iter().map(|p| p[0])), max_of(q.iter().map(|p| p[1]))]
}

fn draw_prims(ctx: &Ctx, s: &mut S, built: &Built, prims: &[Prim], m: &dyn Fn([f64; 2]) -> [f64; 2], sc: f64) {
    let mapped: Vec<Vec<[f64; 2]>> = prims.iter().map(|pr| pr.pts.iter().map(|p| m(*p)).collect()).collect();
    struct Occ {
        id: usize,
        near: f64,
        poly: Vec<[f64; 2]>,
        bx: [f64; 4],
    }
    let occ: Vec<Occ> = prims
        .iter()
        .enumerate()
        .filter_map(|(i, pr)| match (pr.is_loop, pr.part, pr.depth) {
            (true, Some(id), Some(d)) => mapped.get(i).map(|q| Occ { id, near: d[0], poly: q.clone(), bx: bb4(q) }),
            _ => None,
        })
        .collect();
    for (i, pr) in prims.iter().enumerate() {
        let Some(q) = mapped.get(i) else { continue };
        let own = pr.part.or(pr.owner);
        let mut front: Vec<Vec<[f64; 2]>> = Vec::new();
        if let (Some(d), None) = (pr.depth, pr.footing) {
            let b = bb4(q);
            front = occ
                .iter()
                .filter(|o| {
                    Some(o.id) != own && o.near >= d[1] + 1.0 && o.bx[0] < b[2] && o.bx[2] > b[0] && o.bx[1] < b[3] && o.bx[3] > b[1]
                })
                .map(|o| o.poly.clone())
                .collect();
        }
        draw_hidden(ctx, s, q.clone(), &pr.style, pr.is_loop, &front, None);
        if let Some(pi) = pr.part {
            if let Some(p) = built.parts.get(pi) {
                s.region(format!("part:{}", p.id), q);
            }
        } else if let Some(fi) = pr.footing
            && let Some(f) = built.footings.get(fi)
        {
            s.region(format!("footing:{}", f.f.id), q);
        }
    }
    let mut done: Vec<String> = Vec::new();
    for pr in prims {
        if pr.tag.is_empty() {
            continue;
        }
        let part = pr.part.and_then(|i| built.parts.get(i));
        let key = match (part, pr.footing.and_then(|i| built.footings.get(i))) {
            (Some(p), _) => p.id.clone(),
            (None, Some(f)) => f.f.id.clone(),
            _ => continue,
        };
        if done.contains(&key) {
            continue;
        }
        done.push(key);
        let text = match part {
            Some(p) => format!("{} {}", pr.tag, p.profile_name),
            None => pr.tag.clone(),
        };
        if let Some([pa, pb]) = pr.along {
            let (a, b) = (m(pa), m(pb));
            let mut ang = atan2(b[1] - a[1], b[0] - a[0]);
            if ang > PI / 2.0 + 1e-6 {
                ang -= PI;
            } else if ang <= -PI / 2.0 + 1e-6 {
                ang += PI;
            }
            let half = part.map_or(0.0, |p| max(p.profile.width, p.profile.depth) / 2.0 / sc);
            let (nx, ny, tx, ty) = (-sin(ang), cos(ang), cos(ang), sin(ang));
            let off = min(half, 6.0) + 1.2;
            let len = hypot(b[0] - a[0], b[1] - a[1]);
            let mut cands = Vec::new();
            for along in [0.0, 0.25, -0.25, 0.4, -0.4] {
                for side in [off, -(off + 2.2), off + 3.0, -(off + 5.2)] {
                    cands.push((
                        (a[0] + b[0]) / 2.0 + tx * along * len + nx * side,
                        (a[1] + b[1]) / 2.0 + ty * along * len + ny * side,
                        To::center().rot(ang),
                    ));
                }
            }
            s.label_free(&text, &cands, 2.2);
        } else {
            let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
            for p in pr.pts.iter().map(|p| m(*p)) {
                x0 = min(x0, p[0]);
                x1 = max(x1, p[0]);
                y0 = min(y0, p[1]);
                y1 = max(y1, p[1]);
            }
            s.label_free(
                &text,
                &[
                    (x1 + 1.0, y1 + 0.8, To::left()),
                    (x0 - 1.0, y1 + 0.8, To::right()),
                    (x1 + 1.0, y0 - 3.0, To::left()),
                    (x0 - 1.0, y0 - 3.0, To::right()),
                    (x1 + 1.0, y1 + 4.0, To::left()),
                    (x0 - 1.0, y0 - 6.0, To::right()),
                ],
                2.2,
            );
        }
    }
}

fn grid_bubble(s: &mut S, c: [f64; 2], label: &str) {
    s.c(c, 4.0, &Pen::new(0.3));
    s.taken.push(BoxP { x0: c[0] - 4.4, x1: c[0] + 4.4, y0: c[1] - 4.4, y1: c[1] + 4.4 });
    s.t(label, c[0], c[1] - 1.25, 3.2, To::center());
}

/// `fit(bbox, m)` → (scale, origin x, origin y).
fn fit(area: &Area, bb: [f64; 4], l: f64, r: f64, b: f64, tm: f64) -> (f64, f64, f64) {
    let (w, h) = (bb[2] - bb[0], bb[3] - bb[1]);
    let room_w = area.x1 - area.x0 - l - r;
    let room_h = area.y1 - area.y0 - b - tm;
    let s = SCALES.iter().copied().find(|n| w / n <= room_w && h / n <= room_h).unwrap_or(10000.0);
    let ox = area.x0 + l + (room_w - w / s) / 2.0 - bb[0] / s;
    let oy = area.y0 + b + (room_h - h / s) / 2.0 - bb[1] / s;
    (s, ox, oy)
}
fn grow(b: [f64; 4], x0: f64, y0: f64, x1: f64, y1: f64) -> [f64; 4] {
    [min(b[0], x0), min(b[1], y0), max(b[2], x1), max(b[3], y1)]
}

/// A sheet before its title block.
pub struct Raw {
    pub id: String,
    pub kind: &'static str,
    pub title: String,
    pub scale: String,
    pub s: S,
    pub asm: Option<Asm>,
    pub rods: Option<Value>,
}

fn grids(state: &Value, k: &str) -> Vec<(String, f64)> {
    crate::js::arr(state.get("grids").and_then(|g| g.get(k)))
        .iter()
        .map(|g| (crate::js::string_of(g.get("label")), crate::js::num(g.get("at"))))
        .collect()
}
fn levels_sorted(state: &Value) -> Vec<(String, f64)> {
    let mut lv: Vec<(String, f64)> =
        crate::js::arr(state.get("levels")).iter().map(|l| (crate::js::string_of(l.get("name")), crate::js::num(l.get("z")))).collect();
    lv.sort_by(|a, b| crate::js::ord(a.1 - b.1));
    lv
}

fn plan_sheet(ctx: &Ctx, state: &Value, built: &Built, marks: &Marks, fm: Option<&FootingMarks>, level: &(String, f64)) -> Option<Raw> {
    let lv = levels_sorted(state);
    let k = lv.iter().position(|l| l.0 == level.0);
    let below = match k {
        Some(k) if k > 0 => min(PLAN_BAND, (level.1 - lv.get(k - 1).map_or(f64::NAN, |l| l.1)) / 2.0),
        _ => PLAN_BAND,
    };
    let above = match k {
        Some(k) if k + 1 < lv.len() => min(PLAN_BAND, (lv.get(k + 1).map_or(f64::NAN, |l| l.1) - level.1) / 2.0),
        None if !lv.is_empty() => min(PLAN_BAND, (lv.first().map_or(f64::NAN, |l| l.1) - level.1) / 2.0),
        _ => PLAN_BAND,
    };
    let view = View {
        elevation: false,
        r: [1.0, 0.0, 0.0],
        u: [0.0, 1.0, 0.0],
        n: [0.0, 0.0, 1.0],
        a: [0.0, 0.0, 1.0],
        lo: level.1 - below,
        hi: level.1 + above,
        scale_hint: 1e-9,
    };
    let (prims, bbox) = ga_content(built, marks, fm, &view);
    if prims.is_empty() {
        return None;
    }
    let (gx, gy) = (grids(state, "x"), grids(state, "y"));
    let mut bb = bbox.unwrap_or([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY]);
    if !gx.is_empty() {
        bb = grow(bb, min_of(gx.iter().map(|g| g.1)), bb[1], max_of(gx.iter().map(|g| g.1)), bb[3]);
    }
    if !gy.is_empty() {
        bb = grow(bb, bb[0], min_of(gy.iter().map(|g| g.1)), bb[2], max_of(gy.iter().map(|g| g.1)));
    }
    let (sc, ox, oy) = fit(
        &ctx.area,
        bb,
        if gy.is_empty() { 6.0 } else { 34.0 },
        if gy.is_empty() { 6.0 } else { 18.0 },
        if gx.is_empty() { 6.0 } else { 18.0 },
        if gx.is_empty() { 6.0 } else { 34.0 },
    );
    let m = move |[x, y]: [f64; 2]| [ox + x / sc, oy + y / sc];
    let prims = ga_content(built, marks, fm, &View { scale_hint: sc, ..view }).0;
    let mut s = S::default();
    frame_of_sheet(ctx, &mut s);
    let [px0, py0] = m([bb[0], bb[1]]);
    let [px1, py1] = m([bb[2], bb[3]]);
    for (label, at) in &gx {
        let [x, _] = m([*at, 0.0]);
        s.p(vec![[x, py0 - 6.0], [x, py1 + 6.0]], &st_grid(), false);
        grid_bubble(&mut s, [x, py1 + 10.0], label);
        grid_bubble(&mut s, [x, py0 - 10.0], label);
    }
    for (label, at) in &gy {
        let [_, y] = m([0.0, *at]);
        s.p(vec![[px0 - 6.0, y], [px1 + 6.0, y]], &st_grid(), false);
        grid_bubble(&mut s, [px0 - 10.0, y], label);
        grid_bubble(&mut s, [px1 + 10.0, y], label);
    }
    if gx.len() > 1 {
        let v: Vec<f64> = gx.iter().map(|g| g.1).collect();
        s.chain_h(&v, &|x| m([x, 0.0])[0], py1 + 19.0, py1 + 15.0, 7.0, "grids");
    }
    if gy.len() > 1 {
        let v: Vec<f64> = gy.iter().map(|g| g.1).collect();
        s.chain_v(&v, &|y| m([0.0, y])[1], px0 - 19.0, px0 - 15.0, -7.0, "grids");
    }
    draw_prims(ctx, &mut s, built, &prims, &m, sc);
    let title = t("sheet.plan.title", &[("level", F::from(level.0.as_str()))]);
    s.t(
        &format!("{title} ({}{})  {}", if level.1 >= 0.0 { "+" } else { "" }, fmt(level.1), ratio(sc)),
        ctx.area.x0 + 2.0,
        ctx.area.y1 + 8.0,
        3.5,
        To::left().bold(),
    );
    s.t(
        &t("sheet.plan.sub", &[("below", F::S(fmt(below))), ("above", F::S(fmt(above)))]),
        ctx.area.x0 + 2.0,
        ctx.area.y1 + 3.5,
        2.0,
        To::left(),
    );
    Some(Raw { id: format!("plan:{}", level.0), kind: "plan", title, scale: ratio(sc), s, asm: None, rods: None })
}

fn elevation_sheet(
    ctx: &Ctx,
    state: &Value,
    built: &Built,
    marks: &Marks,
    fm: Option<&FootingMarks>,
    grid: &(String, f64),
    axis: &str,
) -> Option<Raw> {
    let is_y = axis == "y";
    let (r, u, n) =
        if is_y { ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]) } else { ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]) };
    let a = if is_y { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
    let view = View { elevation: true, r, u, n, a, lo: grid.1 - ELEV_BAND, hi: grid.1 + ELEV_BAND, scale_hint: 1e-9 };
    let (prims, bbox) = ga_content(built, marks, fm, &view);
    if !prims.iter().any(|p| p.part.is_some()) {
        return None;
    }
    let cross = if is_y { grids(state, "x") } else { grids(state, "y") };
    let levels = levels_sorted(state);
    let mut bb = bbox.unwrap_or([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY]);
    if !cross.is_empty() {
        bb = grow(bb, min_of(cross.iter().map(|g| g.1)), bb[1], max_of(cross.iter().map(|g| g.1)), bb[3]);
    }
    bb = grow(bb, bb[0], min_of(levels.iter().map(|l| l.1)), bb[2], max_of(levels.iter().map(|l| l.1)));
    let (sc, ox, oy) = fit(
        &ctx.area,
        bb,
        if levels.len() > 1 { 28.0 } else { 8.0 },
        34.0,
        if cross.len() > 1 { 24.0 } else { 8.0 },
        if cross.is_empty() { 6.0 } else { 18.0 },
    );
    let m = move |[x, y]: [f64; 2]| [ox + x / sc, oy + y / sc];
    let prims = ga_content(built, marks, fm, &View { scale_hint: sc, ..view }).0;
    let mut s = S::default();
    frame_of_sheet(ctx, &mut s);
    let [px0, py0] = m([bb[0], bb[1]]);
    let [px1, py1] = m([bb[2], bb[3]]);
    for (label, at) in &cross {
        let [x, _] = m([*at, 0.0]);
        s.p(vec![[x, py0 - 3.0], [x, py1 + 6.0]], &st_grid(), false);
        grid_bubble(&mut s, [x, py1 + 10.0], label);
    }
    for (name, z) in &levels {
        let [_, y] = m([0.0, *z]);
        s.p(vec![[px0 - 4.0, y], [px1 + 8.0, y]], &st_level(), false);
        s.p(vec![[px1 + 8.0, y], [px1 + 10.5, y + 2.5], [px1 + 5.5, y + 2.5]], &Pen::new(0.25), true);
        s.label(name, px1 + 12.0, y + 0.8, 2.5, To::left().bold());
        s.t(&format!("{}{}", if *z >= 0.0 { "+" } else { "" }, fmt(*z)), px1 + 12.0, y - 2.8, 2.2, To::left());
    }
    if cross.len() > 1 {
        let v: Vec<f64> = cross.iter().map(|g| g.1).collect();
        s.chain_h(&v, &|x| m([x, 0.0])[0], py0 - 12.0, py0 - 4.0, -7.0, "grids");
    }
    if levels.len() > 1 {
        let v: Vec<f64> = levels.iter().map(|l| l.1).collect();
        s.chain_v(&v, &|y| m([0.0, y])[1], px0 - 14.0, px0 - 5.0, -7.0, "levels");
    }
    draw_prims(ctx, &mut s, built, &prims, &m, sc);
    let from = if is_y { t0("sheet.elev.from-y") } else { t0("sheet.elev.from-x") };
    let title = t("sheet.elev.title", &[("grid", F::from(grid.0.as_str()))]);
    s.t(&format!("{title}  {}", ratio(sc)), ctx.area.x0 + 2.0, ctx.area.y1 + 8.0, 3.5, To::left().bold());
    s.t(
        &t("sheet.elev.sub", &[("grid", F::from(grid.0.as_str())), ("from", F::S(from)), ("band", F::S(fmt(ELEV_BAND)))]),
        ctx.area.x0 + 2.0,
        ctx.area.y1 + 3.5,
        2.0,
        To::left(),
    );
    Some(Raw { id: format!("elevation:{axis}:{}", grid.0), kind: "elevation", title, scale: ratio(sc), s, asm: None, rods: None })
}

// ---------------------------------------------------------------------------------------------
// assembly drawings

/// A part's box in the main part's frame.
#[derive(Clone, Debug)]
pub struct AsmBox {
    pub part: usize,
    pub pts: Vec<V3>,
    pub mark: Option<String>,
    pub center: V3,
}
/// A hole in the main part's frame.
#[derive(Clone, Debug)]
pub struct AsmHole {
    pub part: usize,
    pub c: V3,
    pub a: V3,
    pub t: f64,
    pub d: f64,
}
/// `assemblyGeometry(built, marks, mainId)` (parts, bolts and welds by index into `built`).
#[derive(Clone, Debug)]
pub struct Asm {
    pub main: usize,
    pub subs: Vec<usize>,
    pub boxes: Vec<AsmBox>,
    pub holes: Vec<AsmHole>,
    pub bolts: Vec<usize>,
    pub welds: Vec<usize>,
    pub ext: [[f64; 2]; 3],
}

pub fn assembly_geometry(built: &Built, marks: &Marks, main_id: &str) -> Option<Asm> {
    let mi = *built.by_id.get(main_id)?;
    let main = built.parts.get(mi)?;
    let subs: Vec<usize> =
        built.parts.iter().enumerate().filter(|(_, p)| p.id != main_id && marks.asm_of(&p.id) == Some(main_id)).map(|(i, _)| i).collect();
    let f = &main.frame;
    let loc = |w: V3| {
        let l = to_local(f, w, main.voff);
        [l.x, l.u, l.v]
    };
    let dir = |a: V3| [dot(a, f.x), dot(a, f.u), dot(a, f.v)];
    let boxes: Vec<AsmBox> = subs
        .iter()
        .filter_map(|&i| {
            let p = built.parts.get(i)?;
            Some(AsmBox {
                part: i,
                pts: box_corners(p, 0.0, p.length).into_iter().map(loc).collect(),
                mark: marks.part_mark(&p.id).map(str::to_string),
                center: loc(to_world(&p.frame, p.length / 2.0, 0.0, 0.0, p.voff)),
            })
        })
        .collect();
    let mut holes = Vec::new();
    for &i in std::iter::once(&mi).chain(subs.iter()) {
        let Some(p) = built.parts.get(i) else { continue };
        for h in &p.holes {
            let g = hole_geom(p, h);
            holes.push(AsmHole { part: i, c: loc(g.c), a: dir(g.axis), t: g.t, d: g.d });
        }
    }
    let conns: Vec<&str> =
        subs.iter().filter_map(|&i| built.parts.get(i)).filter_map(|p| p.conn.as_deref()).filter(|c| !c.is_empty()).collect();
    let bolts: Vec<usize> =
        built.bolts.iter().enumerate().filter(|(_, b)| b.conn.as_deref().is_some_and(|c| conns.contains(&c))).map(|(i, _)| i).collect();
    let mut ids: Vec<&str> = vec![main_id];
    ids.extend(subs.iter().filter_map(|&i| built.parts.get(i)).map(|p| p.id.as_str()));
    let welds: Vec<usize> =
        built.welds.iter().enumerate().filter(|(_, w)| w.between.iter().all(|x| ids.contains(&x.as_str()))).map(|(i, _)| i).collect();
    let pd = &main.profile;
    let mut all: Vec<V3> = vec![[0.0, -pd.width / 2.0, -pd.depth / 2.0], [main.length, pd.width / 2.0, pd.depth / 2.0]];
    for b in &boxes {
        all.extend(b.pts.iter().copied());
    }
    let ext = [0, 1, 2].map(|k| {
        [
            min_of(all.iter().map(|p| p.get(k).copied().unwrap_or(f64::NAN))),
            max_of(all.iter().map(|p| p.get(k).copied().unwrap_or(f64::NAN))),
        ]
    });
    Some(Asm { main: mi, subs, boxes, holes, bolts, welds, ext })
}

fn at(v: V3, i: usize) -> f64 {
    v.get(i).copied().unwrap_or(f64::NAN)
}
#[allow(clippy::too_many_arguments)]
fn draw_holes(s: &mut S, holes: &[AsmHole], i: usize, j: usize, k: usize, fi: &dyn Fn(f64) -> f64, fj: &dyn Fn(f64) -> f64, r: f64) {
    for h in holes {
        let a = h.a;
        let c = [fi(at(h.c, i)), fj(at(h.c, j))];
        if at(a, k).abs() > 0.99 {
            let rr = max(0.5, h.d / 2.0 * r);
            s.c(c, rr, &Pen::new(0.3));
            s.p(vec![[c[0] - rr - 1.0, c[1]], [c[0] + rr + 1.0, c[1]]], &st_center(), false);
            s.p(vec![[c[0], c[1] - rr - 1.0], [c[0], c[1] + rr + 1.0]], &st_center(), false);
        } else if at(a, k).abs() < 0.01 {
            let (ai, aj) = (at(a, i), at(a, j));
            let (half, rad) = (h.t / 2.0, h.d / 2.0);
            let pp = |sv: f64, n: f64| [fi(at(h.c, i) + ai * sv - aj * n), fj(at(h.c, j) + aj * sv + ai * n)];
            for n in [-rad, rad] {
                s.p(vec![pp(-half, n), pp(half, n)], &st_hidden(), false);
            }
            s.p(vec![pp(-half - 2.0 / r * 0.5, 0.0), pp(half + 2.0 / r * 0.5, 0.0)], &st_center(), false);
        }
    }
}

fn assembly_sheet(ctx: &Ctx, built: &Built, marks: &Marks, row: &crate::marks::AsmRow) -> Option<Raw> {
    let g = assembly_geometry(built, marks, row.ids.first()?)?;
    let main = built.parts.get(g.main)?;
    let p = &main.profile;
    let l = main.length;
    let mut s = S::default();
    frame_of_sheet(ctx, &mut s);
    let [ex, eu, ev] = g.ext;
    let (k, w_, hs) = (ctx.sheet.k, ctx.sheet.w, ctx.sheet.h);
    let room_x = w_ - 28.0 - 62.0 * k - 68.0;
    let room_v = 48.0 * k;
    let room_u = 32.0 * k;
    let nx = pick_scale(ex[1] - ex[0], room_x);
    let (mut nv, mut nu) = (nx, nx);
    if (ev[1] - ev[0]) / nx < 20.0 {
        nv = pick_scale(ev[1] - ev[0], room_v);
    }
    if (eu[1] - eu[0]) / nx < 15.0 {
        nu = pick_scale(eu[1] - eu[0], room_u);
    }
    let x0 = 28.0;
    let xx = move |x: f64| x0 + (x - ex[0]) / nx;
    let yb = if k > 1.0 { hs - 50.0 - (ev[1] - ev[0]) / nv } else { hs - 105.0 * k };
    let yf = move |v: f64| yb + (v - ev[0]) / nv;
    let yt = yb - 24.0;
    let ytf = move |u: f64| yt - (eu[1] - u) / nu;
    let band = min(104.0, yt - room_u - 8.0);
    let (main_mark, asm_mark) = (row.main.as_str(), row.mark.as_str());

    let cope = |end: &str, side: &str| main.cuts.iter().find(|c| c.kind == "cope" && c.end == end && c.side == side);
    let h2 = p.depth / 2.0;
    let mut out: Vec<[f64; 2]> = Vec::new();
    let (bs, be, te, ts) = (cope("start", "bottom"), cope("end", "bottom"), cope("end", "top"), cope("start", "top"));
    match bs {
        Some(c) => out.extend([[0.0, -h2 + c.depth], [c.length, -h2 + c.depth], [c.length, -h2]]),
        None => out.push([0.0, -h2]),
    }
    match be {
        Some(c) => out.extend([[l - c.length, -h2], [l - c.length, -h2 + c.depth], [l, -h2 + c.depth]]),
        None => out.push([l, -h2]),
    }
    match te {
        Some(c) => out.extend([[l, h2 - c.depth], [l - c.length, h2 - c.depth], [l - c.length, h2]]),
        None => out.push([l, h2]),
    }
    match ts {
        Some(c) => out.extend([[c.length, h2], [c.length, h2 - c.depth], [0.0, h2 - c.depth]]),
        None => out.push([0.0, h2]),
    }
    let out_p: Vec<[f64; 2]> = out.iter().map(|&[x, v]| [xx(x), yf(v)]).collect();
    s.p(out_p.clone(), &st_thick(), true);
    s.region(format!("part:{}", main.id), &out_p);
    let inner = if p.kind == "I" || p.kind == "C" {
        p.tf()
    } else if p.kind == "RHS" || p.kind == "CHS" {
        p.t()
    } else {
        0.0
    };
    if inner != 0.0 && !inner.is_nan() {
        for side in ["top", "bottom"] {
            let v = if side == "top" { h2 - inner } else { -h2 + inner };
            let (a, b) = (cope("start", side), cope("end", side));
            let xa = match a {
                Some(a) if a.depth > inner => a.length,
                _ => 0.0,
            };
            let xb = match b {
                Some(b) if b.depth > inner => l - b.length,
                _ => l,
            };
            if xb > xa {
                s.p(
                    vec![[xx(xa), yf(v)], [xx(xb), yf(v)]],
                    &if p.kind == "I" || p.kind == "C" { st_outline() } else { st_hidden() },
                    false,
                );
            }
        }
    }
    s.p(vec![[xx(ex[0]) - 3.0, yf(0.0)], [xx(ex[1]) + 3.0, yf(0.0)]], &st_center(), false);
    let proj_box = |b: &AsmBox, i1: usize, i2: usize, fi: &dyn Fn(f64) -> f64, fj: &dyn Fn(f64) -> f64| {
        hull(&b.pts.iter().map(|q| [fi(at(*q, i1)), fj(at(*q, i2))]).collect::<Vec<_>>())
    };
    let k0 = p.kind.as_str();
    let web_far = match k0 {
        "I" => Some(p.tw() / 2.0),
        "C" => Some(-p.width / 2.0 + p.tw()),
        "RHS" | "CHS" | "L" => Some(-p.width / 2.0 + p.t()),
        _ => None,
    };
    let flange_under = match k0 {
        "I" | "C" => Some(p.depth / 2.0 - p.tf()),
        "RHS" | "CHS" => Some(p.depth / 2.0 - p.t()),
        _ => None,
    };
    let front_occ: Vec<Vec<[f64; 2]>> = if web_far.is_none() { vec![] } else { vec![out_p.clone()] };
    let behind_web = |b: &AsmBox| web_far.is_some_and(|wf| min_of(b.pts.iter().map(|q| q[1])) >= wf - 0.01);
    for b in &g.boxes {
        let pts = proj_box(b, 0, 2, &xx, &yf);
        draw_hidden(ctx, &mut s, pts.clone(), &st_outline(), true, if behind_web(b) { &front_occ } else { &[] }, Some(&st_hidden()));
        if let Some(bp) = built.parts.get(b.part) {
            s.region(format!("part:{}", bp.id), &pts);
        }
        let top = max_of(pts.iter().map(|q| q[1]));
        let cx = pts.iter().fold(0.0, |a, q| a + q[0]) / pts.len() as f64;
        s.label(b.mark.as_deref().unwrap_or("undefined"), cx, top + 2.0, 2.2, To::center());
    }
    s.label(main_mark, xx((ex[0] + ex[1]) / 2.0), yf(ev[1]) + 2.5, 2.6, To::center().bold());
    draw_holes(&mut s, &g.holes, 0, 2, 1, &xx, &yf, 1.0 / nv);
    let x_mid_paper = (xx(ex[0]) + xx(ex[1])) / 2.0;
    for &wi in &g.welds {
        let Some(w) = built.welds.get(wi) else { continue };
        let Some(b) = g.boxes.iter().find(|bx| built.parts.get(bx.part).is_some_and(|bp| w.between.contains(&bp.id))) else { continue };
        let xs: Vec<f64> = b.pts.iter().map(|q| q[0]).collect();
        let vs: Vec<f64> = b.pts.iter().map(|q| q[2]).collect();
        let (bx0, bx1, bv1, bv0) =
            (min_of(xs.iter().copied()), max_of(xs.iter().copied()), max_of(vs.iter().copied()), min_of(vs.iter().copied()));
        let tip = if bx0 >= -0.5 && bx1 <= l + 0.5 {
            let above = bv0 >= h2 - 0.5;
            [
                xx((bx0 + bx1) / 2.0),
                yf(if above {
                    h2
                } else if bv1 <= -h2 + 0.5 {
                    -h2
                } else {
                    min(bv1, h2)
                }),
            ]
        } else {
            let xc = if bx1 <= l / 2.0 { max(0.0, bx1) } else { min(l, bx0) };
            [xx(xc), yf(min(bv1, h2))]
        };
        s.weld_symbol(tip, w.size, w.all_round, if tip[0] <= x_mid_paper { 1.0 } else { -1.0 });
    }

    let mut xs_all = vec![ex[0], ex[1]];
    for b in &g.boxes {
        let v: Vec<f64> = b.pts.iter().map(|q| q[0]).collect();
        xs_all.push(min_of(v.iter().copied()));
        xs_all.push(max_of(v.iter().copied()));
    }
    xs_all.extend(g.holes.iter().filter(|h| h.a[0].abs() < 0.5).map(|h| h.c[0]));
    let xs = uniq(xs_all, 0.5);
    s.chain_h(&xs, &xx, yf(ev[0]) - 8.0, yf(ev[0]) - 1.0, -7.0, "front");
    let mut vs_all = vec![ev[0], ev[1]];
    vs_all.extend(g.holes.iter().filter(|h| h.a[1].abs() > 0.99).map(|h| h.c[2]));
    for b in &g.boxes {
        let v: Vec<f64> = b.pts.iter().map(|q| q[2]).collect();
        vs_all.push(min_of(v.iter().copied()));
        vs_all.push(max_of(v.iter().copied()));
    }
    let vs_front = uniq(vs_all, 0.5);
    s.chain_v(&vs_front, &yf, xx(ex[1]) + 8.0, xx(ex[1]) + 1.0, 7.0, "front");
    let f_title = if nx == nv { ratio(nx) } else { t("sheet.view.two-scales", &[("length", F::S(ratio(nx))), ("depth", F::S(ratio(nv)))]) };
    s.t(&format!("{}  {f_title}", t0("sheet.view.front")), x0, yf(ev[1]) + 4.0 + 5.0 * min(k, 1.0), 2.8, To::left().bold());

    s.rect(xx(0.0), ytf(-p.width / 2.0), xx(l), ytf(p.width / 2.0), &st_thick());
    if p.kind == "I" {
        for sg in [-1.0, 1.0] {
            s.p(vec![[xx(0.0), ytf(sg * p.tw() / 2.0)], [xx(l), ytf(sg * p.tw() / 2.0)]], &st_hidden(), false);
        }
    }
    s.region(format!("part:{}", main.id), &[[xx(0.0), ytf(-p.width / 2.0)], [xx(l), ytf(p.width / 2.0)]]);
    let top_occ: Vec<Vec<[f64; 2]>> = if flange_under.is_none() {
        vec![]
    } else {
        vec![vec![[xx(0.0), ytf(-p.width / 2.0)], [xx(l), ytf(-p.width / 2.0)], [xx(l), ytf(p.width / 2.0)], [xx(0.0), ytf(p.width / 2.0)]]]
    };
    let under_flange = |b: &AsmBox| flange_under.is_some_and(|fu| max_of(b.pts.iter().map(|q| q[2])) <= fu + 0.01);
    for b in &g.boxes {
        let q = proj_box(b, 0, 1, &xx, &ytf);
        draw_hidden(ctx, &mut s, q.clone(), &st_outline(), true, if under_flange(b) { &top_occ } else { &[] }, Some(&st_hidden()));
        if let Some(bp) = built.parts.get(b.part) {
            s.region(format!("part:{}", bp.id), &q);
        }
    }
    draw_holes(&mut s, &g.holes, 0, 1, 2, &xx, &ytf, 1.0 / nu);
    let mut us = vec![eu[0], eu[1]];
    us.extend(g.holes.iter().filter(|h| h.a[2].abs() > 0.99 && h.part == g.main).map(|h| h.c[1]));
    let us_top = uniq(us, 0.5);
    s.chain_v(&us_top, &ytf, xx(ex[1]) + 8.0, xx(ex[1]) + 1.0, 7.0, "top");
    let tt = if nu == nx {
        t0("sheet.view.top")
    } else {
        format!("{}  {}", t0("sheet.view.top"), t("sheet.view.width", &[("scale", F::S(ratio(nu)))]))
    };
    s.t(&tt, x0, ytf(eu[1]) + 2.0, 2.4, To::left().bold());

    let half = l / 2.0;
    let at = |end: &str| -> Vec<&AsmBox> {
        g.boxes.iter().filter(|b| if end == "start" { b.center[0] < half } else { b.center[0] >= half }).collect()
    };
    let mut ends: Vec<&str> = Vec::new();
    if !at("start").is_empty() {
        ends.push("start");
    }
    if !at("end").is_empty() {
        ends.push("end");
    }
    if ends.is_empty() {
        ends.push("section");
    }
    let top1 = hs - 52.0 - 28.0 * k;
    let slots: Vec<f64> = if k > 1.0 {
        if ends.len() == 1 { vec![top1] } else { vec![top1, top1 - 56.0 * k - 24.0] }
    } else if ends.len() == 1 {
        vec![yb + 13.0 * k]
    } else {
        vec![yb + 36.0 * k, max(yb - 46.0 * k, band + 28.0 * k + 12.0)]
    };
    for (ki, end) in ends.iter().enumerate() {
        let bx: Vec<&AsmBox> = if *end == "section" { vec![] } else { at(end) };
        let hs_: Vec<&AsmHole> = g.holes.iter().filter(|h| h.part != g.main && bx.iter().any(|b| b.part == h.part)).collect();
        let mir = if *end == "start" { -1.0 } else { 1.0 };
        let mut pts: Vec<[f64; 2]> = p.loops.first().map_or(vec![], |lp| lp.iter().map(|&[u, v]| [mir * u, v]).collect());
        for b in &bx {
            pts.extend(b.pts.iter().map(|q| [mir * q[1], q[2]]));
        }
        let ux0 = min_of(pts.iter().map(|q| q[0]));
        let ux1 = max_of(pts.iter().map(|q| q[0]));
        let vy0 = min_of(pts.iter().map(|q| q[1]));
        let vy1 = max_of(pts.iter().map(|q| q[1]));
        let ne = pick_scale(max((ux1 - ux0) / (62.0 * ctx.sheet.k), (vy1 - vy0) / (56.0 * ctx.sheet.k)), 1.0);
        let cx = w_ - 58.0 * ctx.sheet.k;
        let cy = slots.get(ki).copied().unwrap_or(f64::NAN);
        let uf = move |u: f64| cx + (u - (ux0 + ux1) / 2.0) / ne;
        let vf = move |v: f64| cy + (v - (vy0 + vy1) / 2.0) / ne;
        for lp in &p.loops {
            s.p(lp.iter().map(|&[u, v]| [uf(mir * u), vf(v)]).collect(), &st_outline(), true);
        }
        for b in &bx {
            let q = hull(&b.pts.iter().map(|r| [uf(mir * r[1]), vf(r[2])]).collect::<Vec<_>>());
            s.p(q, &st_thick(), true);
        }
        let mapped: Vec<AsmHole> = hs_
            .iter()
            .map(|h| AsmHole { part: h.part, c: [h.c[0], mir * h.c[1], h.c[2]], a: [h.a[0], mir * h.a[1], h.a[2]], t: h.t, d: h.d })
            .collect();
        draw_holes(&mut s, &mapped, 1, 2, 0, &uf, &vf, 1.0 / ne);
        let seen: Vec<&&AsmHole> = hs_.iter().filter(|h| h.a[0].abs() > 0.99).collect();
        let mut hv = vec![ux0, ux1];
        hv.extend(seen.iter().map(|h| mir * h.c[1]));
        s.chain_h(&uniq(hv, 0.5), &uf, vf(vy0) - 7.0, vf(vy0) - 1.0, -6.5, "end");
        let mut vv = vec![vy0, vy1];
        vv.extend(seen.iter().map(|h| h.c[2]));
        s.chain_v(&uniq(vv, 0.5), &vf, uf(ux0) - 7.0, uf(ux0) - 1.0, -6.5, "end");
        let name = match *end {
            "section" => t0("sheet.view.section"),
            "start" => t0("sheet.view.end-start"),
            _ => t("sheet.view.end-end", &[("x", F::S(fmt(l)))]),
        };
        s.t(&format!("{name}  {}", ratio(ne)), cx, vf(vy1) + 4.0, 2.3, To::center().bold());
    }

    // part list, loose bolts, welds
    let mut groups: Vec<(Option<String>, usize, usize)> = Vec::new();
    for &qi in std::iter::once(&g.main).chain(g.subs.iter()) {
        let Some(q) = built.parts.get(qi) else { continue };
        let m = marks.part_mark(&q.id).map(str::to_string);
        match groups.iter_mut().find(|x| x.0 == m) {
            Some(gr) => gr.1 = gr.1.saturating_add(1),
            None => groups.push((m, 1, qi)),
        }
    }
    let mut rows_t: Vec<Vec<String>> = vec![
        ["part", "qty", "profile", "length", "material", "kg-each", "kg"].iter().map(|c| t0(&format!("sheet.parts.col.{c}"))).collect(),
    ];
    let mut total = 0.0;
    for (m, n, qi) in &groups {
        let Some(q) = built.parts.get(*qi) else { continue };
        let nf = *n as f64;
        total += nf * q.weight;
        rows_t.push(vec![
            m.clone().unwrap_or_else(|| "undefined".into()),
            n.to_string(),
            q.profile_name.clone(),
            fmt(q.length),
            q.material.clone(),
            to_fixed(q.weight, 1),
            to_fixed(nf * q.weight, 1),
        ]);
    }
    rows_t.push(vec![
        t("sheet.asm.title", &[("mark", F::from(asm_mark))]),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        to_fixed(total, 1),
    ]);
    let right = [1usize, 3, 5, 6];
    let nrows = rows_t.len();
    let mut widths: Vec<f64> = (0..7)
        .map(|ci| {
            max_of(
                rows_t.iter().enumerate().map(|(ri, r)| text_width(r.get(ci).map_or("", String::as_str), 2.2, ri == 0 || ri == nrows - 1)),
            )
        })
        .collect();
    if let Some(w0) = widths.get_mut(0) {
        *w0 = max(*w0, 8.0);
    }
    let mut col_x = Vec::new();
    let mut cxx = 22.0;
    for (ci, wd) in widths.iter().enumerate() {
        col_x.push(if right.contains(&ci) { cxx + wd } else { cxx });
        cxx += wd + 3.0;
    }
    let list_right = cxx - 3.0;
    let mut y = band;
    s.t(&t("sheet.parts.title", &[("count", F::N(row.count as f64))]), 22.0, y + 2.0, 2.6, To::left().bold());
    y -= 3.0;
    for (ri, r) in rows_t.iter().enumerate() {
        y -= 4.6;
        for (ci, c) in r.iter().enumerate() {
            let xc = col_x.get(ci).copied().unwrap_or(f64::NAN);
            let mut o = if right.contains(&ci) { To::right() } else { To::left() };
            o.bold = ri == 0 || ri == nrows - 1;
            s.t(c, xc, y, 2.2, o);
        }
        if ri == 0 || ri + 2 == nrows {
            s.p(vec![[20.0, y - 1.3], [list_right, y - 1.3]], &st_thin(), false);
        }
    }
    let mut bl: Vec<(String, usize)> = Vec::new();
    for &bi in &g.bolts {
        let Some(b) = built.bolts.get(bi) else { continue };
        let key = format!("{}|{}|{}|{}", b.kind, num_to_string(b.d), num_to_string(b.length), b.grade);
        match bl.iter_mut().find(|x| x.0 == key) {
            Some(e) => e.1 = e.1.saturating_add(1),
            None => bl.push((key, 1)),
        }
    }
    if !bl.is_empty() {
        y -= 7.0;
        s.t(&t0("sheet.loose.title"), 22.0, y, 2.4, To::left().bold());
        for (key, n) in &bl {
            y -= 4.4;
            let parts: Vec<&str> = key.split('|').collect();
            let g4 = |i: usize| F::from(parts.get(i).copied().unwrap_or(""));
            let kk = if parts.first() == Some(&"ANCHORBOLT") { "sheet.loose.rod" } else { "sheet.loose.bolt" };
            s.t(&t(kk, &[("n", F::N(*n as f64)), ("d", g4(1)), ("length", g4(2)), ("grade", g4(3))]), 22.0, y, 2.2, To::left());
        }
    }
    let mut ny = band;
    let notes_x = w_ - 165.0;
    s.t(&t0("sheet.notes"), notes_x, ny + 2.0, 2.6, To::left().bold());
    let mut notes: Vec<String> = Vec::new();
    let to = format!(" {} ", t0("sheet.weld.to"));
    let mut wcount: Vec<(String, usize)> = Vec::new();
    for &wi in &g.welds {
        let Some(w) = built.welds.get(wi) else { continue };
        let between = w.between.iter().map(|id| marks.part_mark(id).unwrap_or("")).collect::<Vec<_>>().join(&to);
        let ws = t("sheet.weld", &[("size", F::N(w.size)), ("where", F::S(w.r#where.to_lowercase())), ("between", F::S(between))]);
        match wcount.iter_mut().find(|x| x.0 == ws) {
            Some(e) => e.1 = e.1.saturating_add(1),
            None => wcount.push((ws, 1)),
        }
    }
    for (weld, n) in wcount {
        notes.push(t("sheet.weld.note", &[("weld", F::S(weld)), ("n", F::N(n as f64))]));
    }
    let mut per_part: Vec<(usize, String, usize)> = Vec::new();
    for h in &g.holes {
        let ds = num_to_string(h.d);
        match per_part.iter_mut().find(|x| x.0 == h.part && x.1 == ds) {
            Some(e) => e.2 = e.2.saturating_add(1),
            None => per_part.push((h.part, ds, 1)),
        }
    }
    let mut seen_notes: Vec<String> = Vec::new();
    for (pi, d, n) in per_part {
        let Some(pp) = built.parts.get(pi) else { continue };
        let key = if pi == g.main { "sheet.holes" } else { "sheet.holes.each" };
        let mut vars = vec![("n", F::N(n as f64)), ("d", F::S(d))];
        if let Some(m) = marks.part_mark(&pp.id) {
            vars.push(("mark", F::from(m)));
        }
        let note = t(key, &vars);
        if !seen_notes.contains(&note) {
            seen_notes.push(note);
        }
    }
    notes.extend(seen_notes);
    notes.push(t0("sheet.dims-from"));
    for n in &notes {
        ny -= 4.4;
        s.t(n, notes_x, ny, 2.1, To::left());
    }
    s.t(&t("sheet.asm.title", &[("mark", F::from(asm_mark))]), 22.0, hs - 18.0, 4.2, To::left().bold());
    let mut vars = Vec::new();
    if let Some(r) = &main.role {
        vars.push(("role", F::from(r.as_str())));
    }
    vars.extend([
        ("profile", F::from(main.profile_name.as_str())),
        ("length", F::S(fmt(l))),
        ("n", F::N(g.subs.len() as f64)),
        ("count", F::N(row.count as f64)),
    ]);
    s.t(&t("sheet.asm.sub", &vars), 22.0, hs - 24.0, 2.6, To::left());
    let scale = if nx != nv { format!("{} / {}", ratio(nx), ratio(nv)) } else { ratio(nx) };
    Some(Raw {
        id: format!("assembly:{asm_mark}"),
        kind: "assembly",
        title: t("sheet.asm.title", &[("mark", F::from(asm_mark))]),
        scale,
        s,
        asm: Some(g),
        rods: None,
    })
}

// ---------------------------------------------------------------------------------------------
// anchor bolt plan

struct Grp<'a> {
    c: Conn,
    plate: &'a Part,
    col: &'a Part,
    rods: Vec<&'a crate::connections::Bolt>,
    top: f64,
}

fn anchor_sheet(ctx: &Ctx, state: &Value, built: &Built, marks: &Marks) -> Option<Raw> {
    let mut groups: Vec<Grp> = Vec::new();
    for cv in crate::js::arr(state.get("connections")) {
        if cv.get("type").and_then(Value::as_str) != Some("baseplate") {
            continue;
        }
        let c = Conn::from_value(cv);
        let (Some(plate), Some(col)) = (built.get(&format!("{}.plate", c.id)), c.column.as_deref().and_then(|id| built.get(id))) else {
            continue;
        };
        let rods: Vec<&crate::connections::Bolt> =
            built.bolts.iter().filter(|b| b.conn.as_deref() == Some(c.id.as_str()) && b.kind == "ANCHORBOLT").collect();
        if rods.is_empty() {
            continue;
        }
        let top = max_of(rods.iter().map(|r| r.head[2])) - c.p("projection");
        groups.push(Grp { c, plate, col, rods, top });
    }
    if groups.is_empty() {
        return None;
    }
    let pstr = |c: &Conn, k: &str| c.params.get(k).map_or_else(String::new, crate::js::to_js_string);
    let rod_key = |g: &Grp| {
        [
            pstr(&g.c, "rodD"),
            num_to_string(g.rods.first().map_or(f64::NAN, |r| r.length)),
            pstr(&g.c, "projection"),
            pstr(&g.c, "embed"),
            pstr(&g.c, "grade"),
        ]
        .join("|")
    };
    let pat_key = |g: &Grp| {
        [
            pstr(&g.c, "sx"),
            pstr(&g.c, "sy"),
            g.plate.profile_name.clone(),
            num_to_string(g.plate.length),
            g.rods.len().to_string(),
            pstr(&g.c, "hole"),
        ]
        .join("|")
    };
    let mut rod_mark: Vec<(String, String)> = Vec::new();
    let mut pat_mark: Vec<(String, String)> = Vec::new();
    for g in &groups {
        let rk = rod_key(g);
        if !rod_mark.iter().any(|x| x.0 == rk) {
            let m = format!("R{}", rod_mark.len() + 1);
            rod_mark.push((rk, m));
        }
        let pk = pat_key(g);
        if !pat_mark.iter().any(|x| x.0 == pk) {
            let m = format!("BP{}", pat_mark.len() + 1);
            pat_mark.push((pk, m));
        }
    }
    let lookup = |list: &Vec<(String, String)>, k: &str| list.iter().find(|x| x.0 == k).map(|x| x.1.clone()).unwrap_or_default();
    let (gx, gy) = (grids(state, "x"), grids(state, "y"));
    let near = |list: &Vec<(String, f64)>, v: f64| -> Option<(String, f64)> {
        let mut b: Option<&(String, f64)> = None;
        for g in list {
            if b.is_none_or(|bb| (g.1 - v).abs() < (bb.1 - v).abs()) {
                b = Some(g);
            }
        }
        b.cloned()
    };
    let where_ = |g: &Grp| -> String {
        let (x, y) = (near(&gx, g.col.start[0]), near(&gy, g.col.start[1]));
        match (x, y) {
            (Some(x), Some(y)) if (x.1 - g.col.start[0]).abs() < 0.5 && (y.1 - g.col.start[1]).abs() < 0.5 => format!("{}/{}", y.0, x.0),
            _ => format!("({}, {})", fmt(g.col.start[0]), fmt(g.col.start[1])),
        }
    };
    let mut s = S::default();
    frame_of_sheet(ctx, &mut s);
    let mut rod_rows: Vec<Vec<String>> =
        vec![["mark", "qty", "dia", "length", "proj", "embed", "grade"].iter().map(|c| t0(&format!("dwg.anchor.col.{c}"))).collect()];
    for (k, m) in &rod_mark {
        let parts: Vec<&str> = k.split('|').collect();
        let q = groups.iter().filter(|g| rod_key(g) == *k).fold(0usize, |a, g| a.saturating_add(g.rods.len()));
        let gp = |i: usize| parts.get(i).copied().unwrap_or("").to_string();
        rod_rows.push(vec![m.clone(), q.to_string(), gp(0), gp(1), gp(2), gp(3), gp(4)]);
    }
    let mut bp_rows: Vec<Vec<String>> =
        vec![["grid", "column", "pattern", "plate", "rods", "top"].iter().map(|c| t0(&format!("dwg.anchor.col.{c}"))).collect()];
    let mut order: Vec<&Grp> = groups.iter().collect();
    order.sort_by(|a, b| crate::js::ord(crate::js::locale_compare_num(&where_(a), &where_(b))));
    for g in order {
        let asm = marks.asm_of(&g.col.id).and_then(|m| marks.asm_mark(m)).unwrap_or("").to_string();
        bp_rows.push(vec![
            where_(g),
            asm,
            lookup(&pat_mark, &pat_key(g)),
            format!("{} x {}", g.plate.profile_name, fmt(g.plate.length)),
            format!("{} {}", g.rods.len(), lookup(&rod_mark, &rod_key(g))),
            format!("{}{}", if g.top >= 0.0 { "+" } else { "" }, fmt(g.top)),
        ]);
    }
    let col_w = |rows: &Vec<Vec<String>>| -> Vec<f64> {
        let n = rows.first().map_or(0, Vec::len);
        (0..n).map(|c| max_of(rows.iter().map(|r| text_width(r.get(c).map_or("undefined", String::as_str), 2.2, true))) + 3.0).collect()
    };
    let (w_r, w_b) = (col_w(&rod_rows), col_w(&bp_rows));
    let sum = |v: &Vec<f64>| v.iter().fold(0.0, |a, b| a + b);
    let table_w = max(sum(&w_r), sum(&w_b));
    let right = ctx.area.x1;
    let tx0 = right - max(table_w, 70.0);
    let draw_table = |s: &mut S, rows: &Vec<Vec<String>>, widths: &Vec<f64>, title: &str, mut y: f64| -> f64 {
        s.t(title, tx0, y, 2.8, To::left().bold());
        y -= 5.0;
        for (ri, r) in rows.iter().enumerate() {
            let mut x = tx0;
            for (ci, c) in r.iter().enumerate() {
                s.t(c, x, y, 2.2, if ri == 0 { To::left().bold() } else { To::left() });
                x += widths.get(ci).copied().unwrap_or(f64::NAN);
            }
            if ri == 0 {
                s.p(vec![[tx0, y - 1.3], [tx0 + sum(widths), y - 1.3]], &st_thin(), false);
            }
            y -= 4.4;
        }
        y - 4.0
    };
    let mut ty = ctx.area.y1 - 2.0;
    let tables_h = (rod_rows.len() + bp_rows.len()) as f64 * 4.4 + 2.0 * 9.0 + 10.0;
    let det_w = max(table_w, 70.0);
    let det_h = min(70.0, (ctx.area.y1 - ctx.area.y0 - 6.0 - tables_h) / pat_mark.len() as f64 - 10.0);
    let plan_pts = |g: &Grp| -> Vec<[f64; 2]> { box_corners(g.plate, 0.0, g.plate.length).into_iter().map(|w| [w[0], w[1]]).collect() };
    for (k, pm) in &pat_mark {
        let Some(g) = groups.iter().find(|x| pat_key(x) == *k) else { continue };
        if det_h < 30.0 {
            break;
        }
        let cx = tx0 + det_w / 2.0;
        let cy = ty - 6.0 - det_h / 2.0;
        let pl = hull(&plan_pts(g));
        let cc = [g.col.start[0], g.col.start[1]];
        let ext = max_of(pl.iter().map(|q| max((q[0] - cc[0]).abs(), (q[1] - cc[1]).abs()))) * 2.0 + 60.0;
        let n = pick_scale(ext, min(det_w, det_h) - 14.0);
        let dm = |[x, y]: [f64; 2]| [cx + (x - cc[0]) / n, cy + (y - cc[1]) / n];
        s.p(pl.iter().map(|q| dm(*q)).collect(), &st_outline(), true);
        for lp in &g.col.profile.loops {
            s.p(
                lp.iter()
                    .map(|&[u, v]| {
                        let w = to_world(&g.col.frame, 0.0, u, v, g.col.voff);
                        dm([w[0], w[1]])
                    })
                    .collect(),
                &st_thin(),
                true,
            );
        }
        let half = ext / 2.0 / n;
        s.p(vec![[cx - half, cy], [cx + half, cy]], &st_center(), false);
        s.p(vec![[cx, cy - half], [cx, cy + half]], &st_center(), false);
        let xs = uniq(g.rods.iter().map(|r| r.head[0]), 0.5);
        let ys = uniq(g.rods.iter().map(|r| r.head[1]), 0.5);
        for r in &g.rods {
            let q = dm([r.head[0], r.head[1]]);
            s.c(q, max(0.5, g.c.p("rodD") / 2.0 / n), &Pen::new(0.3));
        }
        let px: Vec<f64> = pl.iter().map(|q| q[0]).collect();
        let py: Vec<f64> = pl.iter().map(|q| q[1]).collect();
        let (pxmin, pxmax, pymin, pymax) =
            (min_of(px.iter().copied()), max_of(px.iter().copied()), min_of(py.iter().copied()), max_of(py.iter().copied()));
        let mut hv = vec![pxmin];
        hv.extend(&xs);
        hv.push(pxmax);
        s.chain_h(&uniq(hv, 0.5), &|v| dm([v, 0.0])[0], cy - half - 3.0, dm([0.0, pymin])[1] - 1.0, -5.0, "anchor");
        let mut vv = vec![pymin];
        vv.extend(&ys);
        vv.push(pymax);
        s.chain_v(&uniq(vv, 0.5), &|v| dm([0.0, v])[1], cx - half - 3.0, dm([pxmin, 0.0])[0] - 1.0, -5.0, "anchor");
        s.t(&format!("{pm}  {}", ratio(n)), tx0, ty, 2.6, To::left().bold());
        s.t(
            &t(
                "dwg.anchor.typical",
                &[("n", F::N(g.rods.len() as f64)), ("mark", F::S(lookup(&rod_mark, &rod_key(g)))), ("hole", F::S(fmt(g.c.p("hole"))))],
            ),
            tx0 + 22.0,
            ty,
            2.1,
            To::left(),
        );
        ty -= det_h + 10.0;
    }
    ty = draw_table(&mut s, &rod_rows, &w_r, &t0("dwg.anchor.rods"), ty);
    ty = draw_table(&mut s, &bp_rows, &w_b, &t0("dwg.anchor.plates"), ty);
    s.t(&t0("dwg.anchor.note1"), tx0, ty, 1.9, To::left());
    s.t(&t0("dwg.anchor.note2"), tx0, ty - 3.2, 1.9, To::left());

    let pts: Vec<[f64; 2]> = groups.iter().flat_map(&plan_pts).collect();
    let mut bb = [
        min_of(pts.iter().map(|q| q[0])),
        min_of(pts.iter().map(|q| q[1])),
        max_of(pts.iter().map(|q| q[0])),
        max_of(pts.iter().map(|q| q[1])),
    ];
    if !gx.is_empty() {
        bb = grow(bb, min_of(gx.iter().map(|g| g.1)), bb[1], max_of(gx.iter().map(|g| g.1)), bb[3]);
    }
    if !gy.is_empty() {
        bb = grow(bb, bb[0], min_of(gy.iter().map(|g| g.1)), bb[2], max_of(gy.iter().map(|g| g.1)));
    }
    let area2 = Area { x1: tx0 - 8.0, ..ctx.area };
    let (sc, ox, oy) = fit(
        &area2,
        bb,
        if gy.is_empty() { 6.0 } else { 34.0 },
        if gy.is_empty() { 6.0 } else { 18.0 },
        if gx.is_empty() { 6.0 } else { 18.0 },
        if gx.is_empty() { 6.0 } else { 34.0 },
    );
    let m = move |[x, y]: [f64; 2]| [ox + x / sc, oy + y / sc];
    let [px0, py0] = m([bb[0], bb[1]]);
    let [px1, py1] = m([bb[2], bb[3]]);
    for (label, at) in &gx {
        let [x, _] = m([*at, 0.0]);
        s.p(vec![[x, py0 - 6.0], [x, py1 + 6.0]], &st_grid(), false);
        grid_bubble(&mut s, [x, py1 + 10.0], label);
        grid_bubble(&mut s, [x, py0 - 10.0], label);
    }
    for (label, at) in &gy {
        let [_, y] = m([0.0, *at]);
        s.p(vec![[px0 - 6.0, y], [px1 + 6.0, y]], &st_grid(), false);
        grid_bubble(&mut s, [px0 - 10.0, y], label);
        grid_bubble(&mut s, [px1 + 10.0, y], label);
    }
    if gx.len() > 1 {
        let v: Vec<f64> = gx.iter().map(|g| g.1).collect();
        s.chain_h(&v, &|x| m([x, 0.0])[0], py1 + 19.0, py1 + 15.0, 7.0, "grids");
    }
    if gy.len() > 1 {
        let v: Vec<f64> = gy.iter().map(|g| g.1).collect();
        s.chain_v(&v, &|y| m([0.0, y])[1], px0 - 19.0, px0 - 15.0, -7.0, "grids");
    }
    for g in &groups {
        let pl = hull(&plan_pts(g));
        let q: Vec<[f64; 2]> = pl.iter().map(|p| m(*p)).collect();
        s.p(q.clone(), &st_outline(), true);
        s.region(format!("part:{}", g.plate.id), &q);
        for lp in &g.col.profile.loops {
            s.p(
                lp.iter()
                    .map(|&[u, v]| {
                        let w = to_world(&g.col.frame, 0.0, u, v, g.col.voff);
                        m([w[0], w[1]])
                    })
                    .collect(),
                &st_thin(),
                true,
            );
        }
        for r in &g.rods {
            s.c(m([r.head[0], r.head[1]]), max(0.4, g.c.p("rodD") / 2.0 / sc), &Pen::new(0.25));
        }
        let nr = g.rods.len() as f64;
        let cx = g.rods.iter().fold(0.0, |a, r| a + r.head[0]) / nr;
        let cy = g.rods.iter().fold(0.0, |a, r| a + r.head[1]) / nr;
        let off = if hypot(cx - g.col.start[0], cy - g.col.start[1]) > 0.5 {
            format!(" {}", t("dwg.anchor.off", &[("x", F::S(fmt(cx - g.col.start[0]))), ("y", F::S(fmt(cy - g.col.start[1])))]))
        } else {
            String::new()
        };
        let (x1, y1, x0, y0) = (
            max_of(q.iter().map(|v| v[0])),
            max_of(q.iter().map(|v| v[1])),
            min_of(q.iter().map(|v| v[0])),
            min_of(q.iter().map(|v| v[1])),
        );
        let text = format!("{}{off}", lookup(&pat_mark, &pat_key(g)));
        s.label_free(
            &text,
            &[
                (x1 + 1.0, y1 + 0.8, To::left()),
                (x0 - 1.0, y1 + 0.8, To::right()),
                (x1 + 1.0, y0 - 3.0, To::left()),
                (x0 - 1.0, y0 - 3.0, To::right()),
            ],
            2.2,
        );
    }
    let lowest = min_of(groups.iter().map(|g| g.top));
    s.t(&format!("{}  {}", t0("dwg.anchor.title"), ratio(sc)), ctx.area.x0 + 2.0, ctx.area.y1 + 8.0, 3.5, To::left().bold());
    s.t(
        &t("dwg.anchor.sub", &[("z", F::S(format!("{}{}", if lowest >= 0.0 { "+" } else { "" }, fmt(lowest))))]),
        ctx.area.x0 + 2.0,
        ctx.area.y1 + 3.5,
        2.0,
        To::left(),
    );
    let count = groups.iter().fold(0usize, |a, g| a.saturating_add(g.rods.len()));
    let rods =
        json!({ "marks": rod_mark.iter().map(|x| x.1.clone()).collect::<Vec<_>>(), "count": count, "rows": rod_rows, "plates": bp_rows });
    Some(Raw { id: "anchor".into(), kind: "anchor", title: t0("dwg.anchor.title"), scale: ratio(sc), s, asm: None, rods: Some(rods) })
}

// ---------------------------------------------------------------------------------------------

/// A hole's thickness is `undefined` in the JS when the profile has no such face thickness (a pipe): drop the key.
pub fn drop_nan_t(mut v: Value, t: f64) -> Value {
    if t.is_nan()
        && let Some(m) = v.as_object_mut()
    {
        m.shift_remove("t");
    }
    v
}
/// A number as JSON.stringify writes it inside a hashed value (NaN and infinities → null).
fn hnum(x: f64) -> Value {
    if x.is_finite() { jnum(x) } else { Value::Null }
}
fn crc_hex(v: &Value) -> String {
    format!("{:08X}", crate::zip::crc32(crate::js::stringify(v).as_bytes()))
}
/// `modelRef(state)`.
pub fn model_ref(state: &Value) -> String {
    crc_hex(state)
}

fn content_hash(items: &[Item]) -> String {
    let r = |v: f64| hnum(round(v * 100.0) / 100.0);
    let flat: Vec<Value> = items
        .iter()
        .map(|it| match it {
            Item::Text { s, x, y, h, align, rot, .. } => json!(["t", s, r(*x), r(*y), r(*h), r(crate::js::or(*rot, 0.0)), align]),
            Item::Circle { c, r: rr, .. } => json!(["c", r(c[0]), r(c[1]), r(*rr)]),
            Item::Path { pts, closed, dash, .. } => json!([
                "p",
                pts.iter().map(|p| json!([r(p[0]), r(p[1])])).collect::<Vec<_>>(),
                closed,
                dash.as_ref().map(|d| d.iter().map(|x| jnum(*x)).collect::<Vec<_>>())
            ]),
        })
        .collect();
    crc_hex(&Value::Array(flat))
}

/// `partPrints(built, marks)` → (detail, ga) maps by key, in insertion order.
pub fn part_prints(
    state: &Value,
    built: &Built,
    marks: &Marks,
    fm: Option<&FootingMarks>,
) -> (Vec<(String, String)>, Vec<(String, String)>) {
    let r = |v: f64| hnum(round(v * 10.0) / 10.0);
    let mut detail: Vec<(String, String)> = Vec::new();
    let mut ga: Vec<(String, String)> = Vec::new();
    let set = |m: &mut Vec<(String, String)>, k: String, v: String| match m.iter_mut().find(|x| x.0 == k) {
        Some(e) => e.1 = v,
        None => m.push((k, v)),
    };
    for p in &built.parts {
        let body = vec![
            Value::from(p.profile_name.clone()),
            Value::from(p.material.clone()),
            json!(p.start.map(r)),
            json!(p.end.map(r)),
            r(crate::js::or(p.rotation, 0.0)),
            r(crate::js::or(p.voff, 0.0)),
            r(p.length),
        ];
        let mut d = body.clone();
        d.push(Value::Array(p.holes.iter().map(|h| json!([h.face, r(h.x), r(h.y), r(h.d)])).collect()));
        d.push(Value::Array(p.cuts.iter().map(|c| json!([c.kind, c.end, c.side, r(c.length), r(c.depth)])).collect()));
        set(&mut detail, format!("part:{}", p.id), crc_hex(&Value::Array(d)));
        let mut g = body;
        let tag = if marks.asm_of(&p.id) == Some(p.id.as_str()) { marks.asm_mark(&p.id).unwrap_or("").to_string() } else { String::new() };
        g.push(Value::from(tag));
        set(&mut ga, format!("part:{}", p.id), crc_hex(&Value::Array(g)));
    }
    let sf = crate::js::arr(state.get("footings"));
    for (i, bf) in built.footings.iter().enumerate() {
        let mut f = sf.get(i).and_then(Value::as_object).cloned().unwrap_or_else(|| bf.f.fields());
        f.shift_remove("id");
        let id = bf.f.id.clone();
        let v = crc_hex(&json!([Value::Object(f), fmark(fm, &id).filter(|m| !m.is_empty()).unwrap_or("")]));
        set(&mut detail, format!("footing:{id}"), v.clone());
        set(&mut ga, format!("footing:{id}"), v);
    }
    (detail, ga)
}

fn cloud_path(b: &BoxP) -> Vec<[f64; 2]> {
    let mut pts = Vec::new();
    let mut side = |ax: f64, ay: f64, bx: f64, by: f64| {
        let l = hypot(bx - ax, by - ay);
        let n = max(1.0, round(l / 4.0));
        let (nx, ny) = ((by - ay) / l, -(bx - ax) / l);
        let mut i = 0.0;
        while i < n && i < 100_000.0 {
            for j in 0..6 {
                let jf = f64::from(j);
                let f = (i + jf / 6.0) / n;
                let bulge = sin(PI * jf / 6.0) * min(1.3, l / n / 3.0);
                pts.push([ax + (bx - ax) * f + nx * bulge, ay + (by - ay) * f + ny * bulge]);
            }
            i += 1.0;
        }
    };
    side(b.x0, b.y0, b.x1, b.y0);
    side(b.x1, b.y0, b.x1, b.y1);
    side(b.x1, b.y1, b.x0, b.y1);
    side(b.x0, b.y1, b.x0, b.y0);
    pts
}

/// A cloud placed.
#[derive(Clone, Debug)]
pub struct Cloud {
    pub key: String,
    pub b: BoxP,
}
fn revision_clouds(ctx: &Ctx, s: &mut S, prints: &[(String, String)], base: &(Map<String, Value>, String)) -> Vec<Cloud> {
    let mut by: Vec<(String, Vec<Region>)> = Vec::new();
    for g in &s.regions {
        let bp = base.0.get(&g.key);
        let pr = prints.iter().find(|x| x.0 == g.key).map(|x| x.1.as_str());
        if bp.is_some() && bp.and_then(Value::as_str) == pr && pr.is_some() {
            continue;
        }
        match by.iter_mut().find(|x| x.0 == g.key) {
            Some(e) => e.1.push(g.clone()),
            None => by.push((g.key.clone(), vec![g.clone()])),
        }
    }
    let mut clouds: Vec<Cloud> = Vec::new();
    for (key, list) in by {
        for g in list {
            let pad = 2.5;
            let b = BoxP {
                x0: max(ctx.area.x0, g.x0 - pad),
                y0: max(11.0, g.y0 - pad),
                x1: min(ctx.sheet.w - 11.0, g.x1 + pad),
                y1: min(ctx.sheet.h - 11.0, g.y1 + pad),
            };
            if clouds
                .iter()
                .any(|c| c.key == key && c.b.x0 <= b.x0 + 0.1 && c.b.y0 <= b.y0 + 0.1 && c.b.x1 >= b.x1 - 0.1 && c.b.y1 >= b.y1 - 0.1)
            {
                continue;
            }
            s.p(cloud_path(&b), &Pen::new(0.3), true);
            let (tx, ty) = (b.x1, b.y1 + 0.6);
            s.p(vec![[tx - 2.6, ty], [tx + 2.6, ty], [tx, ty + 4.5]], &Pen::new(0.25), true);
            s.t(&base.1, tx, ty + 0.9, 2.0, To::center());
            clouds.push(Cloud { key: key.clone(), b });
        }
    }
    clouds
}

fn revision_table(ctx: &Ctx, s: &mut S, log: &[&Value]) {
    if log.is_empty() {
        return;
    }
    let x1 = ctx.sheet.w - 172.0;
    let x0 = x1 - 108.0;
    let h = 5.2;
    let mut low = 52.0;
    for it in &s.items {
        let ys: Vec<f64> = match it {
            Item::Text { s: st, x, y, h, align, rot, bold } => {
                let b = text_box(st, *x, *y, *h, To { align, rot: *rot, bold: *bold });
                if b.x1 > x0 && b.x0 < x1 { vec![b.y0] } else { vec![] }
            }
            Item::Circle { c, r, .. } => {
                if c[0] + r > x0 && c[0] - r < x1 {
                    vec![c[1] - r]
                } else {
                    vec![]
                }
            }
            Item::Path { pts, .. } => pts.iter().filter(|p| p[0] > x0 && p[0] < x1).map(|p| p[1]).collect(),
        };
        for y in ys {
            if y > 11.0 && y < low + 1.5 {
                low = min(low, y - 1.5);
            }
        }
    }
    let fitn = ((low - 10.0) / h).floor() - 1.0;
    if fitn < 1.0 || fitn.is_nan() {
        return;
    }
    let take = if fitn >= 6.0 {
        6
    } else if fitn >= 5.0 {
        5
    } else if fitn >= 4.0 {
        4
    } else if fitn >= 3.0 {
        3
    } else if fitn >= 2.0 {
        2
    } else {
        1
    };
    let start = log.len().saturating_sub(take);
    let rows: Vec<&&Value> = log.get(start..).unwrap_or(&[]).iter().rev().collect();
    let y1 = min(52.0, low);
    let y0 = y1 - h * (rows.len() as f64 + 1.0);
    s.rect(x0, y0, x1, y1, &st_thin());
    s.p(vec![[x0 + 12.0, y0], [x0 + 12.0, y1]], &st_thin(), false);
    s.p(vec![[x0 + 33.0, y0], [x0 + 33.0, y1]], &st_thin(), false);
    for (st, x) in [(t0("dwg.rev.rev"), x0 + 2.0), (t0("dwg.rev.date"), x0 + 14.0), (t0("dwg.rev.desc"), x0 + 35.0)] {
        s.t(&st, x, y1 - h + 1.6, 2.0, To::left().bold());
    }
    s.p(vec![[x0, y1 - h], [x1, y1 - h]], &st_thin(), false);
    for (i, r) in rows.iter().enumerate() {
        let y = y1 - h * (i as f64 + 2.0) + 1.6;
        s.t(&crate::js::string_of(r.get("rev")), x0 + 2.0, y, 2.2, if i == 0 { To::left().bold() } else { To::left() });
        s.t(&crate::js::string_of(r.get("date")), x0 + 14.0, y, 2.0, To::left());
        let note = match r.get("note") {
            None | Some(Value::Null) => String::new(),
            Some(v) if !crate::js::truthy(Some(v)) => String::new(),
            Some(v) => crate::js::to_js_string(v),
        };
        s.t(&crate::js::slice16(&note, 40), x0 + 35.0, y, 2.0, To::left());
    }
}

/// A finished sheet.
pub struct Sheet {
    pub id: String,
    pub kind: &'static str,
    pub number: String,
    pub title: String,
    pub scale: String,
    pub w: f64,
    pub h: f64,
    pub paper: &'static str,
    pub items: Vec<Item>,
    pub dims: Vec<DDim>,
    pub labels: Vec<String>,
    pub asm: Option<Asm>,
    pub rods: Option<Value>,
    pub welds: Vec<WeldSym>,
    pub hash: String,
    pub rev: String,
    pub prints: Map<String, Value>,
    pub clouds: Vec<Cloud>,
}
/// `drawingSet(...)`.
pub struct Set {
    pub sheets: Vec<Sheet>,
    pub r#ref: String,
    pub ga_count: usize,
    pub paper: &'static str,
}

/// `drawingSet(state, built, marks, { date, paper, hiddenLines })` (footing marks: `marks.footingMark`).
pub fn drawing_set(
    state: &Value,
    built: &Built,
    marks: &Marks,
    fm: Option<&FootingMarks>,
    date: &str,
    paper: &str,
    hidden_lines: bool,
) -> Set {
    let ctx = Ctx::new(if paper.is_empty() { DEFAULT_PAPER } else { paper }, hidden_lines);
    let r#ref = model_ref(state);
    let mut raw: Vec<Raw> = Vec::new();
    for lv in levels_sorted(state) {
        if let Some(s) = plan_sheet(&ctx, state, built, marks, fm, &lv) {
            raw.push(s);
        }
    }
    for g in grids(state, "y") {
        if let Some(s) = elevation_sheet(&ctx, state, built, marks, fm, &g, "y") {
            raw.push(s);
        }
    }
    for g in grids(state, "x") {
        if let Some(s) = elevation_sheet(&ctx, state, built, marks, fm, &g, "x") {
            raw.push(s);
        }
    }
    if let Some(a) = anchor_sheet(&ctx, state, built, marks) {
        raw.push(a);
    }
    let n_ga = raw.len();
    for a in &marks.assemblies {
        if let Some(s) = assembly_sheet(&ctx, built, marks, a) {
            raw.push(s);
        }
    }
    let (detail, ga) = part_prints(state, built, marks, fm);
    let issues = crate::js::arr(state.get("issues"));
    let total = raw.len();
    let mut sheets = Vec::with_capacity(total);
    for (k, mut r) in raw.into_iter().enumerate() {
        let number =
            if r.kind == "assembly" { format!("A-{}", r.id.get("assembly:".len()..).unwrap_or("")) } else { format!("GA-{:02}", k + 1) };
        let hash = content_hash(&r.s.items);
        let log = crate::issues::sheet_log(issues, &r.id);
        let rev = log.last().map_or_else(String::new, |l| crate::js::string_of(l.get("rev")));
        let all = if r.kind == "plan" || r.kind == "elevation" { &ga } else { &detail };
        let mut prints = Map::new();
        for g in &r.s.regions {
            if let Some((_, v)) = all.iter().find(|x| x.0 == g.key) {
                prints.insert(g.key.clone(), Value::from(v.clone()));
            }
        }
        let clouds = match crate::issues::cloud_base(&log, &hash) {
            Some(base) => revision_clouds(&ctx, &mut r.s, all, &base),
            None => vec![],
        };
        let number_shown = if rev.is_empty() {
            number.clone()
        } else {
            t("dwg.rev.number", &[("number", F::S(number.clone())), ("rev", F::S(rev.clone()))])
        };
        let tb = Tb {
            title: r.title.clone(),
            subtitle: if r.kind == "assembly" { t0("sheet.sub.assembly") } else { t0("sheet.sub.ga") },
            number: number_shown,
            scale: r.scale.clone(),
            model: crate::js::string_of(state.get("name")),
            r#ref: r#ref.clone(),
            date: date.to_string(),
            sheet: format!(
                "{}  {}",
                t("sheet.tb.sheet-of", &[("n", F::N((k + 1) as f64)), ("total", F::N(total as f64))]),
                crate::paper::key_label(ctx.sheet.key)
            ),
        };
        title_block(&ctx, &mut r.s, &tb);
        revision_table(&ctx, &mut r.s, &log);
        sheets.push(Sheet {
            id: r.id,
            kind: r.kind,
            number,
            title: r.title,
            scale: r.scale,
            w: ctx.sheet.w,
            h: ctx.sheet.h,
            paper: ctx.sheet.key,
            items: r.s.items,
            dims: r.s.dims,
            labels: r.s.labels,
            asm: r.asm,
            rods: r.rods,
            welds: r.s.welds,
            hash,
            rev,
            prints,
            clouds,
        });
    }
    Set { sheets, r#ref, ga_count: n_ga, paper: ctx.sheet.key }
}

/// The JSON of an assembly's geometry (parts by id; the harness compares it whole).
pub fn asm_json(built: &Built, a: &Asm) -> Value {
    let pid = |i: usize| built.parts.get(i).map_or(Value::Null, |p| Value::from(p.id.clone()));
    let p3 = |p: &V3| crate::js::p3v(*p);
    Obj::new()
        .set("main", pid(a.main))
        .set("subs", Value::Array(a.subs.iter().map(|i| pid(*i)).collect()))
        .set(
            "boxes",
            Value::Array(
                a.boxes
                    .iter()
                    .map(|b| {
                        Obj::new()
                            .set("part", pid(b.part))
                            .set("pts", Value::Array(b.pts.iter().map(p3).collect()))
                            .set("mark", b.mark.clone())
                            .set("center", p3(&b.center))
                            .done()
                    })
                    .collect(),
            ),
        )
        .set(
            "holes",
            Value::Array(
                a.holes
                    .iter()
                    .map(|h| {
                        drop_nan_t(
                            Obj::new().set("part", pid(h.part)).set("c", p3(&h.c)).set("a", p3(&h.a)).set("t", h.t).set("d", h.d).done(),
                            h.t,
                        )
                    })
                    .collect(),
            ),
        )
        .set("bolts", Value::Array(a.bolts.iter().filter_map(|i| built.bolts.get(*i)).map(crate::build::bolt_json).collect()))
        .set("welds", Value::Array(a.welds.iter().filter_map(|i| built.welds.get(*i)).map(crate::build::weld_json).collect()))
        .set("ext", Value::Array(a.ext.iter().map(|e| json!([jnum(e[0]), jnum(e[1])])).collect()))
        .done()
}

/// The JSON of a sheet.
pub fn sheet_json(built: &Built, s: &Sheet) -> Value {
    Obj::new()
        .set("id", s.id.as_str())
        .set("kind", s.kind)
        .set("number", s.number.as_str())
        .set("title", s.title.as_str())
        .set("scale", s.scale.as_str())
        .set("w", s.w)
        .set("h", s.h)
        .set("paper", s.paper)
        .set("items", Value::Array(s.items.iter().map(Item::j).collect()))
        .set(
            "dims",
            Value::Array(s.dims.iter().map(|d| Obj::new().set("dir", d.dir).set("value", d.value).set("view", d.view).done()).collect()),
        )
        .set("labels", s.labels.clone())
        .opt("asm", s.asm.as_ref().map(|a| asm_json(built, a)))
        .opt("rods", s.rods.clone())
        .set(
            "welds",
            Value::Array(
                s.welds
                    .iter()
                    .map(|w| Obj::new().set("x", w.x).set("y", w.y).set("size", w.size).set("allAround", w.all_around).done())
                    .collect(),
            ),
        )
        .set("hash", s.hash.as_str())
        .set("rev", s.rev.as_str())
        .set("prints", Value::Object(s.prints.clone()))
        .set(
            "clouds",
            Value::Array(
                s.clouds
                    .iter()
                    .map(|c| {
                        Obj::new().set("key", c.key.as_str()).set("x0", c.b.x0).set("y0", c.b.y0).set("x1", c.b.x1).set("y1", c.b.y1).done()
                    })
                    .collect(),
            ),
        )
        .done()
}

/// The JSON of a set.
pub fn set_json(built: &Built, s: &Set) -> Value {
    Obj::new()
        .set("sheets", Value::Array(s.sheets.iter().map(|x| sheet_json(built, x)).collect()))
        .set("ref", s.r#ref.as_str())
        .set("gaCount", s.ga_count)
        .set("paper", s.paper)
        .done()
}

/// `holeGeom` of every hole of a part (tests).
pub fn hole_geoms(part: &Part) -> Vec<HoleG> {
    part.holes.iter().map(|h| hole_geom(part, h)).collect()
}

/// A footing marks list from a map (helper for callers holding a HashMap).
pub fn footing_marks_of(m: &HashMap<String, String>) -> Vec<(String, String)> {
    m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// `add`/`mul` re-exported for the kiss module (kept for API symmetry).
pub fn shift(a: V3, b: V3, s: f64) -> V3 {
    add(a, mul(b, s))
}
