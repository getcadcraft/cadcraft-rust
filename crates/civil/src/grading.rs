//! grading.js — pads daylighted to a surface, the graded (proposed) surface, and cut/fill volumes by the grid method.

use crate::geo::{P2, P3, PZ, p2};
use crate::i18n::t;
use crate::js::{J, JsMath, Obj, hypot, log10, max, min, pow, sign};
use crate::tin::{Breakline, Built, Surface, ZPt, ccw, point_in_poly, poly_area};
use crate::tr;
use serde_json::Value;
use std::f64::consts::PI;

/// One slope line's result (`castSlope`).
#[derive(Clone, Copy, Debug)]
pub struct Ray {
    pub x0: f64,
    pub y0: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub d: f64,
    /// 'cut' | 'fill' | 'flat' | 'off'
    pub kind: &'static str,
    /// the edge it leaves (rays along an edge)
    pub edge: Option<usize>,
    /// its start elevation (set by daylightPad)
    pub z0: Option<f64>,
}
impl J for Ray {
    fn j(&self) -> Value {
        Obj::new()
            .set("x0", self.x0)
            .set("y0", self.y0)
            .set("x", self.x)
            .set("y", self.y)
            .set("z", self.z)
            .set("d", self.d)
            .set("kind", self.kind)
            .opt("z0", self.z0)
            .opt("edge", self.edge)
            .done()
    }
}

/// `castSlope(surface, p0, ux, uy, z, cut, fill, { rate, step, far })`.
#[allow(clippy::too_many_arguments)]
pub fn cast_slope(surface: &Surface, p0: P2, ux: f64, uy: f64, z: f64, cut: f64, fill: f64, rate: f64, step: f64, far: f64) -> Ray {
    let mk = |x: f64, y: f64, zz: f64, d: f64, kind: &'static str| Ray { x0: p0.x, y0: p0.y, x, y, z: zz, d, kind, edge: None, z0: None };
    let Some(z0) = surface.elevation_at(p0.x, p0.y, 1e-6) else { return mk(p0.x, p0.y, z, 0.0, "off") };
    if (z0 - z).abs() < 1e-9 {
        return mk(p0.x, p0.y, z, 0.0, "flat");
    }
    let kind = if z0 > z { "cut" } else { "fill" };
    let g = if kind == "cut" { rate / cut } else { -rate / fill };
    let f = |d: f64| surface.elevation_at(p0.x + ux * d, p0.y + uy * d, 1e-6).map(|e| z + g * d - e);
    // f(0) has the sign of (design − ground); march until it changes, then bisect
    let s0 = f(0.0).map_or(f64::NAN, sign);
    let mut a = 0.0;
    let mut b = step;
    let mut fb = f(b);
    let mut guard = 0u64;
    while let Some(v) = fb {
        if !(sign(v) == s0 && b < far) || guard > 100_000_000 {
            break;
        }
        a = b;
        b += step;
        fb = f(b);
        guard = guard.saturating_add(1);
    }
    if fb.is_none() || b >= far {
        return mk(p0.x + ux * a, p0.y + uy * a, z + g * a, a, "off");
    }
    let mut it = 0;
    while it < 60 && b - a > 1e-7 {
        let m = (a + b) / 2.0;
        let fm = f(m);
        if fm.is_some_and(|v| sign(v) == s0) {
            a = m;
        } else {
            b = m;
        }
        it += 1;
    }
    let d = (a + b) / 2.0;
    mk(p0.x + ux * d, p0.y + uy * d, z + g * d, d, kind)
}

/// A pad `{ pts, z, cut, fill, name }` (corners may carry z, 0.4 V14); spacing / fanStep / maxDist are its
/// `options` (NaN = not given).
#[derive(Clone, Debug)]
pub struct Pad {
    pub pts: Vec<PZ>,
    pub z: f64,
    pub cut: f64,
    pub fill: f64,
    pub name: String,
    pub spacing: f64,
    pub fan_step: f64,
    pub max_dist: f64,
}
impl Default for Pad {
    fn default() -> Self {
        Pad {
            pts: Vec::new(),
            z: f64::NAN,
            cut: f64::NAN,
            fill: f64::NAN,
            name: String::new(),
            spacing: f64::NAN,
            fan_step: f64::NAN,
            max_dist: f64::NAN,
        }
    }
}

/// `daylightPad(…)` → { rays, daylight, problems, poly }.
#[derive(Clone, Debug, Default)]
pub struct Daylight {
    pub rays: Vec<Ray>,
    pub daylight: Vec<P3>,
    pub problems: Vec<String>,
    pub poly: Vec<PZ>,
}
impl J for Daylight {
    fn j(&self) -> Value {
        crate::jo! { "rays" => self.rays, "daylight" => self.daylight, "problems" => self.problems, "poly" => self.poly }
    }
}

/// `daylightPad(surface, pad, { spacing, fanStep, maxDist })` (NaN options = not given).
pub fn daylight_pad(surface: &Surface, pad: &Pad) -> Daylight {
    let mut problems = Vec::new();
    let filtered: Vec<PZ> = pad
        .pts
        .iter()
        .enumerate()
        .filter(|(i, p)| *i == 0 || pad.pts.get(i.wrapping_sub(1)).is_some_and(|a| hypot(p.x - a.x, p.y - a.y) > 1e-9))
        .map(|(_, p)| *p)
        .collect();
    let mut poly = ccw(&filtered);
    if poly.len() > 2 {
        let (f0, fl) = (poly.first().copied().unwrap_or_default(), poly.last().copied().unwrap_or_default());
        if hypot(f0.x - fl.x, f0.y - fl.y) < 1e-9 {
            poly.pop();
        }
    }
    if poly.len() >= 4 && self_crosses(&poly) {
        return Daylight { problems: vec![t("grade.err.cross", &[])], poly, ..Daylight::default() };
    }
    if poly.len() < 3 || poly_area(&poly).abs() < 1e-9 {
        return Daylight { problems: vec![t("grade.err.three", &[])], poly, ..Daylight::default() };
    }
    let (cut, fill, z) = (pad.cut, pad.fill, pad.z);
    let zv = |p: &PZ| p.zor(z);
    if !(cut > 0.0) || !(fill > 0.0) {
        return Daylight { problems: vec![t("grade.err.slopes", &[])], poly, ..Daylight::default() };
    }
    let mut size = 0.0f64;
    for p in &poly {
        for q in &poly {
            size = max(size, hypot(p.x - q.x, p.y - q.y));
        }
    }
    let sp = if pad.spacing > 0.0 { pad.spacing } else { max(size / 20.0, 0.5) };
    let sb = surface.bounds();
    let far =
        if pad.max_dist != 0.0 && !pad.max_dist.is_nan() { pad.max_dist } else { 2.0 * hypot(sb.max_x - sb.min_x, sb.max_y - sb.min_y) };
    let fan_step = if pad.fan_step.is_nan() { PI / 18.0 } else { pad.fan_step };
    let n = poly.len();
    let mut rays: Vec<Ray> = Vec::new();
    struct Concave {
        i: usize,
        v: PZ,
        bx: f64,
        by: f64,
    }
    let mut concave: Vec<Concave> = Vec::new();
    let pg = |i: usize| poly.get(i).copied().unwrap_or_default();
    let edge_n = |i: usize| {
        let a = pg(i);
        let b = pg((i + 1) % n);
        let l = hypot(b.x - a.x, b.y - a.y);
        ((b.y - a.y) / l, -(b.x - a.x) / l, l)
    };
    let cast = |p0: P2, ux: f64, uy: f64, rate: f64, z0: f64| {
        let mut r = cast_slope(surface, p0, ux, uy, z0, cut, fill, rate, max(sp / 4.0, 0.05), far);
        r.z0 = Some(z0);
        r
    };
    for i in 0..n {
        let v = pg(i);
        let n_in = edge_n((i + n - 1) % n);
        let n_out = edge_n(i);
        let turn = (n_in.0 * n_out.1 - n_in.1 * n_out.0).jatan2(n_in.0 * n_out.0 + n_in.1 * n_out.1); // > 0 at a convex corner
        if turn > 1e-9 {
            let a0 = n_in.1.jatan2(n_in.0);
            let k = max(1.0, (turn / fan_step).ceil());
            let mut j = 0.0;
            // Spec gap: a fan step of 0 makes the JS loop forever; capped here
            while j <= k && j <= 1e6 {
                let a = a0 + (turn * j) / k;
                rays.push(cast(v.xy(), a.jcos(), a.jsin(), 1.0, zv(&v)));
                j += 1.0;
            }
        } else if turn < -1e-9 {
            let bx = n_in.0 + n_out.0;
            let by = n_in.1 + n_out.1;
            let l = hypot(bx, by);
            rays.push(cast(v.xy(), bx / l, by / l, (turn.abs() / 2.0).jcos(), zv(&v)));
            concave.push(Concave { i, v, bx: bx / l, by: by / l });
        } else {
            rays.push(cast(v.xy(), n_out.0, n_out.1, 1.0, zv(&v)));
        }
        // along the edge
        let b = pg((i + 1) % n);
        let m = max(1.0, (n_out.2 / sp).ceil());
        let mut j = 1.0;
        while j < m {
            let tt = j / m;
            let mut r = cast(p2(v.x + (b.x - v.x) * tt, v.y + (b.y - v.y) * tt), n_out.0, n_out.1, 1.0, zv(&v) + (zv(&b) - zv(&v)) * tt);
            r.edge = Some(i);
            rays.push(r);
            j += 1.0;
        }
    }
    // at a concave corner the two slopes meet along the bisector: edge rays that would reach past it are dropped
    for c in &concave {
        let side = |x: f64, y: f64| c.bx * (y - c.v.y) - c.by * (x - c.v.x);
        let prev_edge = (c.i + n - 1) % n;
        let mut k = rays.len();
        while k > 0 {
            k -= 1;
            let Some(r) = rays.get(k) else { continue };
            if r.edge != Some(c.i) && r.edge != Some(prev_edge) {
                continue;
            }
            let s0 = side(r.x0, r.y0);
            let s1 = side(r.x, r.y);
            if (s0 > 1e-9 && s1 < -1e-9) || (s0 < -1e-9 && s1 > 1e-9) {
                rays.remove(k);
            }
        }
    }
    let bad = rays.iter().filter(|r| r.kind == "off").count();
    if bad > 0 {
        problems.push(tr!("grade.off", n = bad));
    }
    let daylight: Vec<P3> = rays.iter().map(|r| P3 { x: r.x, y: r.y, z: r.z }).collect();
    if bad == 0 && self_crosses(&daylight) {
        problems.push(t("grade.daycross", &[]));
    }
    Daylight { rays, daylight, problems, poly }
}

/// What a graded item puts into the surface (`padPatch` / `corridorPatch` / feature-line patches).
#[derive(Clone, Debug, Default)]
pub struct Patch {
    pub problems: Vec<String>,
    pub region: Option<Vec<P3>>,
    pub points: Vec<P3>,
    pub breaklines: Vec<Breakline>,
    /// the item's own facts (daylightPad result, corridor info …) as JSON
    pub info: Value,
}

/// A graded item: a pad, or anything that patches the surface.
pub enum Item<'a> {
    Pad(Pad),
    Patch(Box<dyn Fn(&Surface) -> Patch + 'a>),
}

/// The build function (buildSurface with the project's boundary rules).
pub type BuildFn<'a> = &'a dyn Fn(&[ZPt], &[Breakline]) -> Built;

/// `gradedSurface(base, basePoints, baseBreaklines, items, build)`.
pub struct Graded {
    pub surface: Option<Surface>,
    pub notes: Vec<String>,
    pub errors: Vec<String>,
    /// per item: (name, problems)
    pub items_report: Vec<(String, Vec<String>)>,
    pub regions: Vec<Option<Vec<P3>>>,
    pub infos: Vec<Value>,
}

/// `gradedSurface(base, basePoints, baseBreaklines, items, build)`; `names` are the items' names (report).
pub fn graded_surface(
    base: &Surface,
    base_points: &[ZPt],
    base_breaklines: &[Breakline],
    items: &[(String, Item)],
    build: BuildFn,
) -> Graded {
    let mut out =
        Graded { surface: None, notes: Vec::new(), errors: Vec::new(), items_report: Vec::new(), regions: Vec::new(), infos: Vec::new() };
    let mut points: Vec<ZPt> = base_points.to_vec();
    let mut breaklines: Vec<Breakline> = base_breaklines.to_vec();
    let mut surf: Option<Surface> = None;
    for (name, item) in items {
        let cur: &Surface = surf.as_ref().unwrap_or(base);
        let pt = match item {
            Item::Patch(f) => f(cur),
            Item::Pad(p) => pad_patch(cur, p),
        };
        out.items_report.push((name.clone(), pt.problems.clone()));
        out.infos.push(pt.info.clone());
        let region = match pt.region {
            Some(r) if pt.problems.is_empty() && r.len() >= 3 => r,
            _ => {
                out.regions.push(None);
                continue;
            }
        };
        // inside the region or on its edge (a ground point on the edge would pin the new surface to the old ground)
        let inside = |x: f64, y: f64| point_in_poly(x, y, &region) || near_poly(x, y, &region, 1e-6);
        points.retain(|p| !inside(p.x, p.y));
        breaklines.retain(|b| b.pts.iter().all(|p| !inside(p.x, p.y)) && !crosses_poly(&b.pts, &region));
        out.regions.push(Some(region));
        points.extend(pt.points.iter().map(|p| ZPt { x: p.x, y: p.y, z: Some(p.z) }));
        breaklines.extend(pt.breaklines);
        let built = build(&points, &breaklines);
        match built.surface {
            None => {
                out.errors.extend(built.report.errors);
                out.surface = None;
                return out;
            }
            Some(s) => {
                out.notes.extend(built.report.notes);
                surf = Some(s);
            }
        }
    }
    out.surface = surf;
    out
}

fn pad_patch(surf: &Surface, pad: &Pad) -> Patch {
    let dl = daylight_pad(surf, pad);
    if !dl.problems.is_empty() || dl.rays.is_empty() {
        let problems = if dl.problems.is_empty() { vec![t("grade.none", &[])] } else { dl.problems.clone() };
        return Patch { problems, region: None, points: Vec::new(), breaklines: Vec::new(), info: dl.j() };
    }
    let region = dl.daylight.clone();
    let pad_z = pad.z;
    let zv = |p: &PZ| p.zor(pad_z);
    let pname = if pad.name.is_empty() { t("scene.pad.default", &[]) } else { pad.name.clone() };
    let mut bl = Vec::new();
    let mut edge: Vec<P3> = dl.poly.iter().map(|p| P3 { x: p.x, y: p.y, z: zv(p) }).collect();
    if let Some(f) = edge.first().copied() {
        edge.push(f);
    }
    bl.push(Breakline { name: tr!("grade.bl.edge", name = pname.as_str()), pts: edge, from_group: false });
    let mut day = region.clone();
    if let Some(f) = day.first().copied() {
        day.push(f);
    }
    bl.push(Breakline { name: tr!("grade.bl.daylight", name = pname.as_str()), pts: day, from_group: false });
    for r in &dl.rays {
        if r.d > 1e-6 {
            bl.push(Breakline {
                name: "slope".into(),
                pts: vec![P3 { x: r.x0, y: r.y0, z: r.z0.unwrap_or(pad_z) }, P3 { x: r.x, y: r.y, z: r.z }],
                from_group: false,
            });
        }
    }
    Patch { problems: Vec::new(), region: Some(region), points: Vec::new(), breaklines: bl, info: dl.j() }
}

/// `gridVolume(base, top, …)` → { cut, fill, net, cells, cell, area }.
#[derive(Clone, Copy, Debug, Default)]
pub struct Volume {
    pub cut: f64,
    pub fill: f64,
    pub net: f64,
    pub cells: f64,
    pub cell: f64,
    pub area: f64,
}
impl J for Volume {
    fn j(&self) -> Value {
        crate::jo! { "cut" => self.cut, "fill" => self.fill, "net" => self.net, "cells" => self.cells, "cell" => self.cell, "area" => self.area }
    }
}

/// `gridVolume(base, top, { cell, region, bounds, maxCells })`.
pub fn grid_volume(
    base: &Surface,
    top: &Surface,
    cell: f64,
    regions: Option<&[Vec<P3>]>,
    bounds: Option<crate::tin::Bounds>,
    max_cells: f64,
) -> Volume {
    let ba = base.bounds();
    let bb = top.bounds();
    let mut b = bounds.unwrap_or(crate::tin::Bounds {
        min_x: max(ba.min_x, bb.min_x),
        min_y: max(ba.min_y, bb.min_y),
        max_x: min(ba.max_x, bb.max_x),
        max_y: min(ba.max_y, bb.max_y),
    });
    if let Some(regions) = regions.filter(|r| !r.is_empty()) {
        let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for poly in regions {
            for p in poly {
                x0 = min(x0, p.x);
                y0 = min(y0, p.y);
                x1 = max(x1, p.x);
                y1 = max(y1, p.y);
            }
        }
        b = crate::tin::Bounds { min_x: max(b.min_x, x0), min_y: max(b.min_y, y0), max_x: min(b.max_x, x1), max_y: min(b.max_y, y1) };
    }
    let w = b.max_x - b.min_x;
    let h = b.max_y - b.min_y;
    if !(w > 0.0 && h > 0.0) {
        return Volume { cell: if cell != 0.0 && !cell.is_nan() { cell } else { 0.0 }, ..Volume::default() };
    }
    let mut c = if cell > 0.0 { cell } else { nice_cell(((w * h) / 250_000.0).sqrt()) };
    while (w / c) * (h / c) > max_cells {
        c *= 2.0;
    }
    // cells on a grid anchored at whole multiples of c, so results do not depend on the bounds
    let i0 = (b.min_x / c).floor();
    let i1 = (b.max_x / c).ceil();
    let j0 = (b.min_y / c).floor();
    let j1 = (b.max_y / c).ceil();
    let (mut cut_v, mut fill_v, mut cells) = (0.0f64, 0.0f64, 0.0f64);
    let a = c * c;
    let mut j = j0;
    while j < j1 {
        let y = (j + 0.5) * c;
        let mut i = i0;
        while i < i1 {
            let x = (i + 0.5) * c;
            i += 1.0;
            if let Some(regions) = regions
                && !regions.iter().any(|poly| point_in_poly(x, y, poly))
            {
                continue;
            }
            let (Some(z0), Some(z1)) = (base.z_at(x, y), top.z_at(x, y)) else { continue };
            cells += 1.0;
            let dz = z1 - z0;
            if dz > 0.0 {
                fill_v += dz * a;
            } else {
                cut_v -= dz * a;
            }
        }
        j += 1.0;
    }
    Volume { cut: cut_v, fill: fill_v, net: fill_v - cut_v, cells, cell: c, area: cells * a }
}

fn nice_cell(v: f64) -> f64 {
    if !(v > 0.0) {
        return 1.0;
    }
    let p = pow(10.0, log10(v).floor());
    let m = v / p;
    (if m < 1.5 {
        1.0
    } else if m < 3.5 {
        2.0
    } else if m < 7.5 {
        5.0
    } else {
        10.0
    }) * p
}

/// True when a closed polygon's edges cross (not at shared corners).
pub fn self_crosses<T: crate::geo::XY>(pts: &[T]) -> bool {
    let n = pts.len();
    let g = |i: usize| pts.get(i % n.max(1)).map_or((f64::NAN, f64::NAN), |p| (p.x(), p.y()));
    for i in 0..n {
        let a = g(i);
        let b = g(i + 1);
        for j in (i + 2)..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            let c = g(j);
            let d = g(j + 1);
            if proper_cross(a, b, c, d) {
                return true;
            }
        }
    }
    false
}
/// Segments cross properly (strictly, not at ends).
pub fn proper_cross(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> bool {
    let o = |p: (f64, f64), q: (f64, f64), r: (f64, f64)| (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0);
    let d1 = o(a, b, c);
    let d2 = o(a, b, d);
    let d3 = o(c, d, a);
    let d4 = o(c, d, b);
    ((d1 > 1e-12 && d2 < -1e-12) || (d1 < -1e-12 && d2 > 1e-12)) && ((d3 > 1e-12 && d4 < -1e-12) || (d3 < -1e-12 && d4 > 1e-12))
}
fn near_poly<T: crate::geo::XY>(px: f64, py: f64, poly: &[T], tol: f64) -> bool {
    let n = poly.len();
    for i in 0..n {
        let (Some(a), Some(b)) = (poly.get(i), poly.get((i + 1) % n)) else { continue };
        let dx = b.x() - a.x();
        let dy = b.y() - a.y();
        let l = dx * dx + dy * dy;
        let t = if l != 0.0 { max(0.0, min(1.0, ((px - a.x()) * dx + (py - a.y()) * dy) / l)) } else { 0.0 };
        if hypot(px - a.x() - dx * t, py - a.y() - dy * t) <= tol {
            return true;
        }
    }
    false
}
fn crosses_poly<T: crate::geo::XY, U: crate::geo::XY>(line: &[T], poly: &[U]) -> bool {
    let n = poly.len();
    for w in line.windows(2) {
        let [p, q] = w else { continue };
        for j in 0..n {
            let (Some(a), Some(b)) = (poly.get(j), poly.get((j + 1) % n)) else { continue };
            if proper_cross((p.x(), p.y()), (q.x(), q.y()), (a.x(), a.y()), (b.x(), b.y())) {
                return true;
            }
        }
    }
    false
}
