//! tin.js — TIN surfaces: constrained Delaunay triangulation (Delaunator, then breaklines forced in by Sloan's edge
//! flips and Delaunay restored round them), elevations, contours, bands, slopes, outline, faces.
//!
//! Inputs are in project units (eastings x, northings y). Coordinates move to a local origin (the floored minimum)
//! before triangulating, so state-plane and UTM values (x = 500 000, y = 4 000 000 m; millions of feet) keep their
//! precision — the large-coordinate rule of AGENTS.md § 12 holds here by construction, exactly as in the JS.

// Index arithmetic works on positions inside the mesh arrays (already allocated, so it cannot overflow).
#![allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use crate::delaunator::Delaunator;
use crate::geo::{P2, P3, PZ, XY, p2};
use crate::i18n::t;
use crate::js::{G, J, JA, JsMath, Obj, TA, fa, hypot, hypot3, ia, max, min, round};
use crate::predicates::{incircle, orient2d};
use crate::tr;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::sync::OnceLock;

pub const SNAP: f64 = 1e-4;
#[inline]
fn next(e: usize) -> usize {
    if e % 3 == 2 { e - 2 } else { e + 1 }
}
#[inline]
fn prev(e: usize) -> usize {
    if e.is_multiple_of(3) { e + 2 } else { e - 1 }
}

/// A breakline as tin.js reads it (only `pts` matters here).
#[derive(Clone, Debug, Default)]
pub struct Breakline {
    pub name: String,
    pub pts: Vec<P3>,
    /// `fromGroup: true` (point-group breaklines made by the project)
    pub from_group: bool,
}
impl J for Breakline {
    fn j(&self) -> Value {
        let o = Obj::new().set("name", &self.name).set("pts", &self.pts);
        if self.from_group { o.set("fromGroup", true).done() } else { o.done() }
    }
}

/// A point fed to the surface: plan place and an elevation that may be missing (`null`).
#[derive(Clone, Copy, Debug)]
pub struct ZPt {
    pub x: f64,
    pub y: f64,
    pub z: Option<f64>,
}

/// buildSurface's report.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub notes: Vec<String>,
    pub errors: Vec<String>,
    /// false for surfaceFromFaces' `{notes, errors}`
    pub full: bool,
    pub skipped_no_z: usize,
    pub merged: usize,
    pub crossings: usize,
    pub z_conflicts: usize,
    pub constraint_failures: usize,
    pub breakline_segments: Option<usize>,
}
impl Report {
    fn new() -> Report {
        Report { full: true, ..Report::default() }
    }
    fn short() -> Report {
        Report::default()
    }
}
impl J for Report {
    fn j(&self) -> Value {
        let mut o = Obj::new().set("notes", &self.notes).set("errors", &self.errors);
        if self.full {
            o = o
                .set("skippedNoZ", self.skipped_no_z)
                .set("merged", self.merged)
                .set("crossings", self.crossings)
                .set("zConflicts", self.z_conflicts)
                .set("constraintFailures", self.constraint_failures)
                .opt("breaklineSegments", self.breakline_segments);
        }
        o.done()
    }
}

/// `{ surface, report }`.
#[derive(Debug)]
pub struct Built {
    pub surface: Option<Surface>,
    pub report: Report,
}

// ---------------------------------------------------------------------------
// Building

/// `buildSurface({ points, breaklines, boundary, maxEdge })`.
pub fn build_surface(points: &[ZPt], breaklines: &[Breakline], boundary: Option<&[P2]>, max_edge: f64) -> Built {
    let mut report = Report::new();
    let mut clipped: Option<Vec<P3>> = None;
    if let Some(boundary) = boundary.filter(|b| b.len() >= 3) {
        // pass 1: the surface without the boundary gives the boundary corners their elevations
        let first = triangulate(points, breaklines, None, 0.0, Report::new());
        let Some(fs) = first.surface else {
            report.errors.extend(first.report.errors);
            return Built { surface: None, report };
        };
        let hull = fs.hull();
        let c = clip_convex(&ccw(boundary), &hull);
        if c.len() < 3 {
            report.errors.push(t("tin.err.bndy", &[]));
            return Built { surface: None, report };
        }
        let differs = c.len() != boundary.len()
            || c.iter().enumerate().any(|(i, p)| boundary.get(i).is_some_and(|b| hypot(p.x - b.x, p.y - b.y) > 1e-9));
        if differs && boundary.iter().any(|p| !point_in_poly(p.x, p.y, &hull)) {
            report.notes.push(t("tin.bndy.cut", &[]));
        }
        let mut out = Vec::with_capacity(c.len());
        for p in &c {
            match fs.elevation_at(p.x, p.y, 1e-6) {
                Some(z) => out.push(P3 { x: p.x, y: p.y, z }),
                None => {
                    report.errors.push(t("tin.err.bndyz", &[]));
                    return Built { surface: None, report };
                }
            }
        }
        clipped = Some(out);
    }
    triangulate(points, breaklines, clipped.as_deref(), max_edge, report)
}

/// An insertion-key for a snapped plan position (tin.js keys on the text `round(x/SNAP),round(y/SNAP)`).
#[inline]
fn key_part(v: f64) -> u64 {
    let r = round(v / SNAP);
    if r.is_nan() {
        0x7ff8_0000_0000_0000
    } else if r == 0.0 {
        0
    } else {
        r.to_bits()
    }
}
#[inline]
fn k_of(x: f64, y: f64) -> (u64, u64) {
    (key_part(x), key_part(y))
}

fn triangulate(points: &[ZPt], breaklines: &[Breakline], boundary: Option<&[P3]>, max_edge: f64, mut report: Report) -> Built {
    // local origin
    let mut ox = f64::INFINITY;
    let mut oy = f64::INFINITY;
    for p in points {
        if p.z.is_some_and(f64::is_finite) {
            if p.x < ox {
                ox = p.x;
            }
            if p.y < oy {
                oy = p.y;
            }
        }
    }
    for b in breaklines {
        for p in &b.pts {
            if p.x < ox {
                ox = p.x;
            }
            if p.y < oy {
                oy = p.y;
            }
        }
    }
    if !ox.is_finite() {
        report.errors.push(t("tin.err.noz", &[]));
        return Built { surface: None, report };
    }
    ox = ox.floor();
    oy = oy.floor();

    let mut xs = fa(Vec::new());
    let mut ys = fa(Vec::new());
    let mut zs = fa(Vec::new());
    let mut key: HashMap<(u64, u64), usize> = HashMap::new();
    let mut add_v =
        |x: f64, y: f64, z: f64, hard: bool, report: &mut Report, xs: &mut TA<f64>, ys: &mut TA<f64>, zs: &mut TA<f64>| -> usize {
            let lx = x - ox;
            let ly = y - oy;
            let k = k_of(lx, ly);
            if let Some(&at) = key.get(&k) {
                let za = zs.get(at).copied().unwrap_or(f64::NAN);
                if (za - z).abs() > 1e-3 {
                    report.z_conflicts += 1;
                    if hard && let Some(s) = zs.get_mut(at) {
                        *s = z;
                    }
                } else if !hard {
                    report.merged += 1;
                }
                return at;
            }
            key.insert(k, xs.len());
            xs.push(lx);
            ys.push(ly);
            zs.push(z);
            xs.len() - 1
        };
    for p in points {
        match p.z {
            Some(z) if z.is_finite() => {
                add_v(p.x, p.y, z, false, &mut report, &mut xs, &mut ys, &mut zs);
            }
            _ => report.skipped_no_z += 1,
        }
    }
    let mut segs: Vec<(usize, usize, bool)> = Vec::new();
    for b in breaklines {
        for w in b.pts.windows(2) {
            if let [p, q] = w {
                let a = add_v(p.x, p.y, p.z, true, &mut report, &mut xs, &mut ys, &mut zs);
                let c = add_v(q.x, q.y, q.z, true, &mut report, &mut xs, &mut ys, &mut zs);
                if a != c {
                    segs.push((a, c, true));
                }
            }
        }
    }
    let n_break = segs.len();
    if let Some(boundary) = boundary {
        let before = report.merged;
        let ids: Vec<usize> = boundary.iter().map(|p| add_v(p.x, p.y, p.z, false, &mut report, &mut xs, &mut ys, &mut zs)).collect();
        report.merged = before; // a boundary corner on a point is not a duplicate point
        for i in 0..ids.len() {
            let a = ids.g(i);
            let c = ids.g((i + 1) % ids.len());
            if a != c {
                segs.push((a, c, false));
            }
        }
    }
    let mut key_map = key;
    let pieces = split_crossings(&mut xs, &mut ys, &mut zs, &segs, &mut key_map, &mut report);

    if xs.len() < 3 {
        report.errors.push(t("tin.err.three", &[]));
        return Built { surface: None, report };
    }
    let mut coords = fa(vec![0.0f64; xs.len() * 2]);
    for i in 0..xs.len() {
        coords.s(2 * i, xs.g(i));
        coords.s(2 * i + 1, -ys.g(i)); // mirrored: Delaunator's triangles come out counter-clockwise in y-up
    }
    let del = Delaunator::new(&coords);
    if del.triangles.is_empty() {
        report.errors.push(t("tin.err.line", &[]));
        return Built { surface: None, report };
    }

    let tri: Vec<i32> = del.triangles.iter().map(|&v| v as i32).collect();
    let mut mesh = Mesh::new(&xs, &ys, ia(tri), ia(del.halfedges));
    for &(a, b) in &pieces {
        if !mesh.constrain(a, b) {
            report.constraint_failures += 1;
        }
    }
    if !pieces.is_empty() {
        mesh.legalize_all();
    }
    if report.constraint_failures > 0 {
        report.notes.push(tr!("tin.constraint", n = report.constraint_failures));
    }

    let mut keep = TA::filled(mesh.tri.len() / 3, 1u8, 0);
    if let Some(boundary) = boundary {
        let poly: Vec<P2> = boundary.iter().map(|p| p2(p.x - ox, p.y - oy)).collect();
        for (t, k) in keep.iter_mut().enumerate() {
            let a = mesh.tri.g(3 * t) as usize;
            let b = mesh.tri.g(3 * t + 1) as usize;
            let c = mesh.tri.g(3 * t + 2) as usize;
            if !point_in_poly((xs.g(a) + xs.g(b) + xs.g(c)) / 3.0, (ys.g(a) + ys.g(b) + ys.g(c)) / 3.0, &poly) {
                *k = 0;
            }
        }
    }
    if max_edge > 0.0 {
        mesh.peel(&mut keep, max_edge);
    }

    let mut tris: Vec<u32> = Vec::new();
    for (t, &k) in keep.iter().enumerate() {
        if k != 0 {
            tris.push(mesh.tri.g(3 * t) as u32);
            tris.push(mesh.tri.g(3 * t + 1) as u32);
            tris.push(mesh.tri.g(3 * t + 2) as u32);
        }
    }
    if tris.is_empty() {
        report.errors.push(t("tin.err.inside", &[]));
        return Built { surface: None, report };
    }
    if report.merged > 0 {
        report.notes.push(tr!("tin.merged", n = report.merged));
    }
    if report.z_conflicts > 0 {
        report.notes.push(tr!("tin.zconflict", n = report.z_conflicts));
    }
    if report.skipped_no_z > 0 {
        report.notes.push(tr!("tin.noz", n = report.skipped_no_z));
    }
    report.breakline_segments = Some(n_break);
    let constrained = mesh.constrained_pairs();
    let mut surface = Surface::new(xs.into_vec(), ys.into_vec(), zs.into_vec(), tris, ox, oy);
    surface.constrained_edges = constrained;
    Built { surface: Some(surface), report }
}

/// `surfaceFromFaces({ pts, faces })`: a TIN given as vertices (null = missing) and faces (from LandXML).
pub fn surface_from_faces(pts: &[Option<P3>], faces: &[Vec<f64>]) -> Built {
    let mut report = Report::short();
    let mut ox = f64::INFINITY;
    let mut oy = f64::INFINITY;
    for p in pts.iter().flatten() {
        ox = min(ox, p.x);
        oy = min(oy, p.y);
    }
    if !ox.is_finite() {
        report.errors.push(t("tin.err.nopts", &[]));
        return Built { surface: None, report };
    }
    ox = ox.floor();
    oy = oy.floor();
    let n = pts.len();
    let mut xs = fa(vec![0.0f64; n]);
    let mut ys = fa(vec![0.0f64; n]);
    let mut zs = fa(vec![0.0f64; n]);
    for (i, p) in pts.iter().enumerate() {
        if let Some(p) = p {
            xs.s(i, p.x - ox);
            ys.s(i, p.y - oy);
            zs.s(i, p.z);
        }
    }
    let mut tris: Vec<u32> = Vec::new();
    let mut bad = 0usize;
    for f in faces {
        let ok_idx = |i: f64| crate::js::is_integer(i) && i >= 0.0 && (i as usize) < n && pts.get(i as usize).is_some_and(Option::is_some);
        let g = |k: usize| f.get(k).copied().unwrap_or(f64::NAN);
        if f.iter().any(|&i| !ok_idx(i)) || g(0) == g(1) || g(1) == g(2) || g(0) == g(2) {
            bad += 1;
            continue;
        }
        // f has at least the three entries checked above (a missing one is NaN, never equal and never valid)
        if f.len() < 3 {
            bad += 1;
            continue;
        }
        let a = g(0) as usize;
        let mut b = g(1) as usize;
        let mut c = g(2) as usize;
        let o = (xs.g(b) - xs.g(a)) * (ys.g(c) - ys.g(a)) - (ys.g(b) - ys.g(a)) * (xs.g(c) - xs.g(a));
        if o == 0.0 {
            bad += 1;
            continue;
        }
        if o < 0.0 {
            std::mem::swap(&mut b, &mut c); // keep every triangle counter-clockwise
        }
        tris.push(a as u32);
        tris.push(b as u32);
        tris.push(c as u32);
    }
    if bad > 0 {
        report.notes.push(tr!("tin.badfaces", n = bad));
    }
    if tris.is_empty() {
        report.errors.push(t("tin.err.nofaces", &[]));
        return Built { surface: None, report };
    }
    Built { surface: Some(Surface::new(xs.into_vec(), ys.into_vec(), zs.into_vec(), tris, ox, oy)), report }
}

// ---------------------------------------------------------------------------
// Breakline crossings: split every pair of crossing pieces at the crossing (new vertex)

fn split_crossings(
    xs: &mut TA<f64>,
    ys: &mut TA<f64>,
    zs: &mut TA<f64>,
    segs_in: &[(usize, usize, bool)],
    key: &mut HashMap<(u64, u64), usize>,
    report: &mut Report,
) -> Vec<(usize, usize)> {
    let segs = JA::new(segs_in.to_vec(), (0, 0, false));
    let mut cuts: JA<Vec<(f64, usize)>> = JA::new(vec![Vec::new(); segs.len()], Vec::new());
    let minx = |s: &(usize, usize, bool), xs: &TA<f64>| min(xs.g(s.0), xs.g(s.1));
    let mut order: Vec<usize> = (0..segs.len()).collect();
    {
        let xr: &TA<f64> = xs;
        crate::js::sort_by(&mut order, |&i, &j| minx(&segs.g(i), xr) - minx(&segs.g(j), xr));
    }
    let mut active: JA<usize> = JA::new(Vec::new(), 0);
    for &i in &order {
        let (a, b, hi) = segs.g(i);
        let min_x = min(xs.g(a), xs.g(b));
        let mut k = active.len();
        while k > 0 {
            k -= 1;
            let (c, d, _) = segs.g(active.g(k));
            if max(xs.g(c), xs.g(d)) < min_x - SNAP {
                active.remove(k);
            }
        }
        for &j in active.iter() {
            let (c, d, hj) = segs.g(j);
            if a == c || a == d || b == c || b == d {
                continue;
            }
            let Some((ht, hu)) = seg_hit(xs.g(a), ys.g(a), xs.g(b), ys.g(b), xs.g(c), ys.g(c), xs.g(d), ys.g(d)) else { continue };
            let x = xs.g(a) + (xs.g(b) - xs.g(a)) * ht;
            let y = ys.g(a) + (ys.g(b) - ys.g(a)) * ht;
            let zi = zs.g(a) + (zs.g(b) - zs.g(a)) * ht;
            let zj = zs.g(c) + (zs.g(d) - zs.g(c)) * hu;
            let k = k_of(x, y);
            // a breakline gives the crossing its elevation; a boundary edge does not
            // between two breaklines the one given first wins
            let z = if hi && hj {
                if i < j { zi } else { zj }
            } else if hi {
                zi
            } else if hj {
                zj
            } else {
                zi
            };
            let v = match key.get(&k) {
                Some(&v) => v,
                None => {
                    let v = xs.len();
                    key.insert(k, v);
                    xs.push(x);
                    ys.push(y);
                    zs.push(z);
                    v
                }
            };
            if hi && hj && (zi - zj).abs() > 0.01 {
                report.z_conflicts += 1;
            }
            report.crossings += 1;
            if let Some(c) = cuts.get_mut(i) {
                c.push((ht, v));
            }
            if let Some(c) = cuts.get_mut(j) {
                c.push((hu, v));
            }
        }
        active.push(i);
    }
    let mut out = Vec::new();
    for (i, &(a, b, _)) in segs.iter().enumerate() {
        let mut c: Vec<(f64, usize)> = cuts.get_mut(i).map(std::mem::take).unwrap_or_default();
        crate::js::sort_by(&mut c, |p, q| p.0 - q.0);
        let mut from = a;
        for &(_, v) in c.iter() {
            if v != from {
                out.push((from, v));
            }
            from = v;
        }
        if from != b {
            out.push((from, b));
        }
    }
    out
}

/// Proper crossing of two segments (not at their ends): (t along the first, u along the second).
#[allow(clippy::too_many_arguments)]
fn seg_hit(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64, dx: f64, dy: f64) -> Option<(f64, f64)> {
    let rx = bx - ax;
    let ry = by - ay;
    let sx = dx - cx;
    let sy = dy - cy;
    let den = rx * sy - ry * sx;
    if !(den.abs() >= 1e-12) {
        return None;
    }
    let t = ((cx - ax) * sy - (cy - ay) * sx) / den;
    let u = ((cx - ax) * ry - (cy - ay) * rx) / den;
    let eps = 1e-9;
    if t <= eps || t >= 1.0 - eps || u <= eps || u >= 1.0 - eps {
        return None;
    }
    Some((t, u))
}

// ---------------------------------------------------------------------------
// Half-edge mesh with constraints (triangles counter-clockwise; e → its twin in he[e], -1 on the hull)

struct Mesh<'a> {
    x: &'a [f64],
    y: &'a [f64],
    tri: TA<i32>,
    he: TA<i32>,
    con: TA<u8>,
    vmap: TA<i32>,
}

enum Walk {
    Edge(usize),
    EdgeRest(usize, (usize, usize)),
    Crossed(Vec<usize>, Option<(usize, usize)>),
}

impl<'a> Mesh<'a> {
    fn new(x: &'a [f64], y: &'a [f64], tri: TA<i32>, he: TA<i32>) -> Mesh<'a> {
        let mut vmap = TA::filled(x.len(), -1i32, -1);
        for (e, &v) in tri.iter().enumerate() {
            if let Some(s) = vmap.get_mut(v as usize) {
                *s = e as i32;
            }
        }
        let con = TA::filled(he.len(), 0u8, 0);
        Mesh { x, y, tri, he, con, vmap }
    }
    #[inline]
    fn t(&self, e: usize) -> usize {
        self.tri.get(e).copied().unwrap_or(0) as usize
    }
    #[inline]
    fn h(&self, e: usize) -> i32 {
        self.he.get(e).copied().unwrap_or(-1)
    }
    #[inline]
    fn px(&self, v: usize) -> f64 {
        self.x.get(v).copied().unwrap_or(f64::NAN)
    }
    #[inline]
    fn py(&self, v: usize) -> f64 {
        self.y.get(v).copied().unwrap_or(f64::NAN)
    }
    /// > 0: counter-clockwise (y up)
    fn orient(&self, a: usize, b: usize, c: usize) -> f64 {
        -orient2d(self.px(a), self.py(a), self.px(b), self.py(b), self.px(c), self.py(c))
    }
    fn mark(&mut self, e: usize) {
        if let Some(s) = self.con.get_mut(e) {
            *s = 1;
        }
        let o = self.h(e);
        if o >= 0
            && let Some(s) = self.con.get_mut(o as usize)
        {
            *s = 1;
        }
    }

    /// Every half-edge leaving vertex v.
    fn around(&self, v: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let start = self.vmap.get(v).copied().unwrap_or(-1);
        if start < 0 {
            return out;
        }
        let start = start as usize;
        let mut e = start;
        let mut open = false;
        let mut guard = 0usize;
        loop {
            out.push(e);
            let o = self.h(prev(e));
            if o < 0 {
                open = true;
                break;
            }
            e = o as usize;
            guard += 1;
            if e == start || guard > self.tri.len() {
                break;
            }
        }
        if open {
            // hit the hull: go the other way from the start
            e = start;
            let mut guard = 0usize;
            loop {
                let o = self.h(e);
                if o < 0 {
                    break;
                }
                e = next(o as usize);
                if e == start {
                    break;
                }
                out.push(e);
                guard += 1;
                if guard > self.tri.len() {
                    break;
                }
            }
        }
        out
    }

    /// Forces the edge a–b in. Returns false when it cannot (it would cross another breakline).
    fn constrain(&mut self, a0: usize, b0: usize) -> bool {
        let mut todo = vec![(a0, b0)];
        let mut guard = 0usize;
        while let Some((p, q)) = todo.pop() {
            guard += 1;
            if guard > 100_000 {
                return false;
            }
            if p == q {
                continue;
            }
            if self.vmap.get(p).copied().unwrap_or(-1) < 0 || self.vmap.get(q).copied().unwrap_or(-1) < 0 {
                return false;
            }
            match self.walk(p, q) {
                None => return false,
                Some(Walk::Edge(e)) => self.mark(e),
                Some(Walk::EdgeRest(e, rest)) => {
                    self.mark(e);
                    todo.push(rest);
                }
                Some(Walk::Crossed(crossed, rest)) => {
                    if !self.flip_out(p, q, &crossed) {
                        return false;
                    }
                    if let Some(r) = rest {
                        todo.push(r);
                    }
                }
            }
        }
        true
    }

    fn walk(&self, p: usize, q: usize) -> Option<Walk> {
        let mut first: i64 = -1;
        for e in self.around(p) {
            let b = self.t(next(e));
            let c = self.t(prev(e));
            if b == q {
                return Some(Walk::Edge(e));
            }
            if c == q {
                return Some(Walk::Edge(prev(e)));
            }
            let ob = self.side(p, q, b);
            let oc = self.side(p, q, c);
            if ob == 0 && self.ahead(p, q, b) {
                return Some(Walk::EdgeRest(e, (b, q)));
            }
            if oc == 0 && self.ahead(p, q, c) {
                return Some(Walk::EdgeRest(prev(e), (c, q)));
            }
            if ob < 0 && oc > 0 {
                first = next(e) as i64; // q is between b (right) and c (left)
                break;
            }
        }
        if first < 0 {
            return None;
        }
        let mut crossed = Vec::new();
        let mut e = first as usize;
        for _ in 0..1_000_000 {
            if self.con.get(e).copied().unwrap_or(0) != 0 {
                return None;
            }
            crossed.push(e);
            let o = self.h(e);
            if o < 0 {
                return None;
            }
            let o = o as usize;
            let v = self.t(prev(o));
            if v == q {
                return Some(Walk::Crossed(crossed, None));
            }
            let ov = self.side(p, q, v);
            if ov == 0 {
                return Some(Walk::Crossed(crossed, Some((v, q))));
            }
            // o runs from the left vertex to the right one; leave through the edge on v's other side
            e = if ov > 0 { next(o) } else { prev(o) };
        }
        None
    }
    /// Which side of p→q v is on (1 left, -1 right), 0 when it is within a hair of the line.
    fn side(&self, p: usize, q: usize, v: usize) -> i32 {
        let o = self.orient(p, q, v);
        if o == 0.0 {
            return 0;
        }
        let l = hypot(self.px(q) - self.px(p), self.py(q) - self.py(p));
        if o.abs() / l < 1e-7 * (1.0 + l) {
            return 0;
        }
        if o > 0.0 { 1 } else { -1 }
    }
    fn ahead(&self, p: usize, q: usize, v: usize) -> bool {
        let dx = self.px(q) - self.px(p);
        let dy = self.py(q) - self.py(p);
        let t = ((self.px(v) - self.px(p)) * dx + (self.py(v) - self.py(p)) * dy) / (dx * dx + dy * dy);
        t > 0.0 && t < 1.0
    }

    /// Sloan's flips: swap crossed diagonals until none crosses p–end, then restore Delaunay.
    fn flip_out(&mut self, p: usize, _q0: usize, crossed: &[usize]) -> bool {
        // the segment really ends at the last vertex reached (q0 or a vertex on the line, handled by walk's rest)
        let q = {
            let last = crossed.last().copied().unwrap_or(0);
            let o = self.h(last);
            self.t(prev(o.max(0) as usize))
        };
        let mut queue: VecDeque<usize> = crossed.iter().copied().collect();
        let mut made: Vec<usize> = Vec::new();
        let mut guard = 0usize;
        let cl = crossed.len() + 10;
        while let Some(e) = queue.pop_front() {
            guard += 1;
            if guard > 50 * cl * cl {
                return false;
            }
            if !self.convex(e) {
                queue.push_back(e);
                continue;
            }
            // a flip moves the quad's outer edges c→a and d→b to the ids e and its twin: follow them
            let o = self.h(e) as usize;
            let bot = prev(e);
            let top = prev(o);
            for x in queue.iter_mut() {
                if *x == bot {
                    *x = o;
                } else if *x == top {
                    *x = e;
                }
            }
            let ne = self.flip(e);
            let u = self.t(ne);
            let v = self.t(next(ne));
            if u != p && u != q && v != p && v != q && self.crosses(p, q, u, v) {
                queue.push_back(ne);
            } else {
                made.push(ne);
            }
        }
        // mark p–q
        let mut found = false;
        for e in self.around(p) {
            if self.t(next(e)) == q {
                self.mark(e);
                found = true;
                break;
            }
            if self.t(prev(e)) == q {
                self.mark(prev(e));
                found = true;
                break;
            }
        }
        if !found {
            return false;
        }
        // restore Delaunay (Lawson) starting from the new edges; edges are kept as vertex pairs because a flip re-uses
        // half-edge ids; the cavity's own boundary edges now face new triangles too, so start from every edge of them
        let mut stack: Vec<(usize, usize)> = Vec::new();
        for &e in &made {
            let o = self.h(e);
            for f in [e, next(e), prev(e)] {
                stack.push((self.t(f), self.t(next(f))));
            }
            if o >= 0 {
                let o = o as usize;
                for f in [next(o), prev(o)] {
                    stack.push((self.t(f), self.t(next(f))));
                }
            }
        }
        let mut n = 0usize;
        while !stack.is_empty() && {
            let ok = n < 200_000;
            n += 1;
            ok
        } {
            let Some((u, v)) = stack.pop() else { break };
            let e = self.find_edge(u, v);
            if e < 0 {
                continue;
            }
            let e = e as usize;
            if self.con.get(e).copied().unwrap_or(0) != 0 || self.h(e) < 0 || !self.illegal(e) {
                continue;
            }
            let ne = self.flip(e);
            let o = self.h(ne) as usize;
            // the four outer edges of the quad may now be illegal
            for f in [next(ne), prev(ne), next(o), prev(o)] {
                stack.push((self.t(f), self.t(next(f))));
            }
        }
        true
    }
    /// Lawson flips over the whole mesh until every edge that is not a constraint is locally Delaunay.
    fn legalize_all(&mut self) {
        let mut stack: Vec<(usize, usize)> = Vec::new();
        for e in 0..self.tri.len() {
            if self.h(e) > e as i32 {
                stack.push((self.t(e), self.t(next(e))));
            }
        }
        let limit = 50 * self.tri.len();
        let mut n = 0usize;
        while !stack.is_empty() && {
            let ok = n < limit;
            n += 1;
            ok
        } {
            let Some((u, v)) = stack.pop() else { break };
            let e = self.find_edge(u, v);
            if e < 0 {
                continue;
            }
            let e = e as usize;
            if self.con.get(e).copied().unwrap_or(0) != 0 || self.h(e) < 0 || !self.illegal(e) {
                continue;
            }
            let ne = self.flip(e);
            let o = self.h(ne) as usize;
            for f in [next(ne), prev(ne), next(o), prev(o)] {
                stack.push((self.t(f), self.t(next(f))));
            }
        }
    }
    fn find_edge(&self, u: usize, v: usize) -> i64 {
        for e in self.around(u) {
            if self.t(next(e)) == v {
                return e as i64;
            }
            if self.t(prev(e)) == v {
                return prev(e) as i64;
            }
        }
        -1
    }
    fn crosses(&self, p: usize, q: usize, u: usize, v: usize) -> bool {
        let a = self.orient(p, q, u);
        let b = self.orient(p, q, v);
        let c = self.orient(u, v, p);
        let d = self.orient(u, v, q);
        ((a > 0.0 && b < 0.0) || (a < 0.0 && b > 0.0)) && ((c > 0.0 && d < 0.0) || (c < 0.0 && d > 0.0))
    }
    /// The two triangles on e make a strictly convex quadrilateral (so e can be flipped).
    fn convex(&self, e: usize) -> bool {
        let o = self.h(e);
        if o < 0 {
            return false;
        }
        let a = self.t(e);
        let b = self.t(next(e));
        let c = self.t(prev(e));
        let d = self.t(prev(o as usize));
        let s1 = self.orient(c, d, a);
        let s2 = self.orient(c, d, b);
        (s1 > 0.0 && s2 < 0.0) || (s1 < 0.0 && s2 > 0.0)
    }
    fn illegal(&self, e: usize) -> bool {
        let o = self.h(e);
        if o < 0 {
            return false;
        }
        let a = self.t(e);
        let b = self.t(next(e));
        let c = self.t(prev(e));
        let d = self.t(prev(o as usize));
        incircle(self.px(a), self.py(a), self.px(b), self.py(b), self.px(c), self.py(c), self.px(d), self.py(d)) > 0.0 && self.convex(e)
    }
    /// Flips the diagonal e; returns the new diagonal's half-edge in e's old triangle.
    fn flip(&mut self, e: usize) -> usize {
        let o = self.h(e) as usize;
        let bot = prev(e);
        let lft = next(e);
        let top = prev(o);
        let rgt = next(o);
        let adj_bot = self.h(bot);
        let adj_top = self.h(top);
        let c_bot = self.con.g(bot);
        let c_top = self.con.g(top);
        // e: a→b, lft: b→c, bot: c→a | o: b→a, rgt: a→d, top: d→b   ⇒   (d,b,c) and (c,a,d)
        self.tri.s(e, self.tri.g(top));
        self.he.s(e, adj_top);
        if adj_top >= 0 {
            self.he.s(adj_top as usize, e as i32);
        }
        self.con.s(e, c_top);
        self.tri.s(o, self.tri.g(bot));
        self.he.s(o, adj_bot);
        if adj_bot >= 0 {
            self.he.s(adj_bot as usize, o as i32);
        }
        self.con.s(o, c_bot);
        self.he.s(bot, top as i32);
        self.he.s(top, bot as i32);
        self.con.s(bot, 0);
        self.con.s(top, 0);
        for k in [e, lft, o, rgt] {
            let v = self.tri.g(k) as usize;
            if let Some(s) = self.vmap.get_mut(v) {
                *s = k as i32;
            }
        }
        bot
    }

    /// Removes outside triangles whose outer edge is longer than maxEdge, working inward.
    fn peel(&self, keep: &mut TA<u8>, max_edge: f64) {
        let len = |e: usize| hypot(self.px(self.t(next(e))) - self.px(self.t(e)), self.py(self.t(next(e))) - self.py(self.t(e)));
        let outer = |e: usize, keep: &TA<u8>| {
            let o = self.h(e);
            o < 0 || keep.get(o as usize / 3).copied().unwrap_or(0) == 0
        };
        let mut stack: Vec<usize> = Vec::new();
        for e in 0..self.tri.len() {
            if keep.get(e / 3).copied().unwrap_or(0) != 0 && outer(e, keep) {
                stack.push(e);
            }
        }
        while let Some(e) = stack.pop() {
            let t = e / 3;
            if keep.get(t).copied().unwrap_or(0) == 0 || self.con.g(e) != 0 || !outer(e, keep) || len(e) <= max_edge {
                continue;
            }
            keep.s(t, 0);
            for f in [3 * t, 3 * t + 1, 3 * t + 2] {
                let o = self.h(f);
                if o >= 0 && keep.get(o as usize / 3).copied().unwrap_or(0) != 0 {
                    stack.push(o as usize);
                }
            }
        }
    }
    fn constrained_pairs(&self) -> Vec<[u32; 2]> {
        let mut out = Vec::new();
        for e in 0..self.tri.len() {
            let h = self.h(e);
            if self.con.g(e) != 0 && (h < 0 || (e as i32) < h) {
                out.push([self.t(e) as u32, self.t(next(e)) as u32]);
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Surface

/// `surface.stats()`.
#[derive(Clone, Debug)]
pub struct Stats {
    pub points: usize,
    pub triangles: usize,
    pub min_z: f64,
    pub max_z: f64,
    pub mean_z: f64,
    pub area2d: f64,
    pub area3d: f64,
    pub bounds: Bounds,
}
#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}
impl J for Bounds {
    fn j(&self) -> Value {
        crate::jo! { "minX" => self.min_x, "minY" => self.min_y, "maxX" => self.max_x, "maxY" => self.max_y }
    }
}
impl J for Stats {
    fn j(&self) -> Value {
        crate::jo! {
            "points" => self.points, "triangles" => self.triangles, "minZ" => self.min_z, "maxZ" => self.max_z,
            "meanZ" => self.mean_z, "area2d" => self.area2d, "area3d" => self.area3d, "bounds" => self.bounds,
        }
    }
}

/// The surface's triangle grid index (cells of triangles by bounding box).
#[derive(Debug)]
pub struct Grid {
    pub b: Bounds,
    pub nx: usize,
    pub ny: usize,
    pub cw: f64,
    pub ch: f64,
    pub start: Vec<u32>,
    pub items: Vec<u32>,
}

/// A contour line `{ z, pts, closed }`.
#[derive(Clone, Debug)]
pub struct Contour {
    pub z: f64,
    pub pts: Vec<P2>,
    pub closed: bool,
}
impl J for Contour {
    fn j(&self) -> Value {
        crate::jo! { "z" => self.z, "pts" => self.pts, "closed" => self.closed }
    }
}
/// `{ band, loop }`.
#[derive(Clone, Debug)]
pub struct BandPiece {
    pub band: i64,
    pub ring: Vec<f64>,
}
impl J for BandPiece {
    fn j(&self) -> Value {
        crate::jo! { "band" => self.band, "loop" => self.ring }
    }
}
/// `surface.slopeOf(t)`.
#[derive(Clone, Copy, Debug)]
pub struct Slope {
    pub slope: f64,
    pub dir: f64,
    pub gx: f64,
    pub gy: f64,
}
/// One of `surface.slopes()`.
#[derive(Clone, Copy, Debug)]
pub struct SlopeArrow {
    pub x: f64,
    pub y: f64,
    pub slope: f64,
    pub dir: f64,
    pub size: f64,
}
impl J for SlopeArrow {
    fn j(&self) -> Value {
        crate::jo! { "x" => self.x, "y" => self.y, "slope" => self.slope, "dir" => self.dir, "size" => self.size }
    }
}

/// The triangle under a point with its barycentric weights.
#[derive(Clone, Copy, Debug)]
pub struct Located {
    pub t: usize,
    pub w: [f64; 3],
}

/// A TIN: vertices in local coordinates (X, Y relative to ox, oy), triangles counter-clockwise.
#[derive(Debug)]
pub struct Surface {
    pub x: TA<f64>,
    pub y: TA<f64>,
    pub z: TA<f64>,
    pub tris: TA<u32>,
    pub ox: f64,
    pub oy: f64,
    pub n_t: usize,
    pub constrained_edges: Vec<[u32; 2]>,
    grid: OnceLock<Grid>,
    stats: OnceLock<Stats>,
}

impl Clone for Surface {
    fn clone(&self) -> Self {
        let mut s = Surface::new(self.x.to_vec(), self.y.to_vec(), self.z.to_vec(), self.tris.to_vec(), self.ox, self.oy);
        s.constrained_edges = self.constrained_edges.clone();
        s
    }
}

fn clamp_i(v: f64, n: usize) -> Option<usize> {
    let i = v.floor();
    if i.is_nan() {
        return None;
    }
    Some(if i < 0.0 {
        0
    } else if i >= n as f64 {
        n.saturating_sub(1)
    } else {
        i as usize
    })
}

impl Surface {
    pub fn new(x: Vec<f64>, y: Vec<f64>, z: Vec<f64>, tris: Vec<u32>, ox: f64, oy: f64) -> Surface {
        let n_t = tris.len() / 3;
        Surface {
            x: fa(x),
            y: fa(y),
            z: fa(z),
            tris: crate::js::ua(tris),
            ox,
            oy,
            n_t,
            constrained_edges: Vec::new(),
            grid: OnceLock::new(),
            stats: OnceLock::new(),
        }
    }
    #[inline]
    fn tv(&self, k: usize) -> usize {
        self.tris.get(k).copied().unwrap_or(0) as usize
    }
    #[inline]
    fn vx(&self, i: usize) -> f64 {
        self.x.get(i).copied().unwrap_or(f64::NAN)
    }
    #[inline]
    fn vy(&self, i: usize) -> f64 {
        self.y.get(i).copied().unwrap_or(f64::NAN)
    }
    #[inline]
    fn vz(&self, i: usize) -> f64 {
        self.z.get(i).copied().unwrap_or(f64::NAN)
    }

    /// Vertices used by triangles.
    pub fn used_vertices(&self) -> TA<u8> {
        let mut used = TA::filled(self.x.len(), 0u8, 0);
        for &i in self.tris.iter() {
            if let Some(s) = used.get_mut(i as usize) {
                *s = 1;
            }
        }
        used
    }

    pub fn stats(&self) -> &Stats {
        self.stats.get_or_init(|| {
            let used = self.used_vertices();
            let mut n = 0usize;
            let (mut min_z, mut max_z, mut sum) = (f64::INFINITY, f64::NEG_INFINITY, 0.0f64);
            let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
            for i in 0..self.x.len() {
                if used.g(i) == 0 {
                    continue;
                }
                let (x, y, z) = (self.x.g(i), self.y.g(i), self.z.g(i));
                n += 1;
                sum += z;
                if z < min_z {
                    min_z = z;
                }
                if z > max_z {
                    max_z = z;
                }
                if x < min_x {
                    min_x = x;
                }
                if x > max_x {
                    max_x = x;
                }
                if y < min_y {
                    min_y = y;
                }
                if y > max_y {
                    max_y = y;
                }
            }
            let (mut a2, mut a3) = (0.0f64, 0.0f64);
            for t in (0..self.tris.len().saturating_sub(2)).step_by(3) {
                let (a, b, c) = (self.tv(t), self.tv(t + 1), self.tv(t + 2));
                let ux = self.vx(b) - self.vx(a);
                let uy = self.vy(b) - self.vy(a);
                let uz = self.vz(b) - self.vz(a);
                let vx = self.vx(c) - self.vx(a);
                let vy = self.vy(c) - self.vy(a);
                let vz = self.vz(c) - self.vz(a);
                let cx = uy * vz - uz * vy;
                let cy = uz * vx - ux * vz;
                let cz = ux * vy - uy * vx;
                a2 += cz.abs() / 2.0;
                a3 += hypot3(cx, cy, cz) / 2.0;
            }
            Stats {
                points: n,
                triangles: self.n_t,
                min_z,
                max_z,
                mean_z: sum / n as f64,
                area2d: a2,
                area3d: a3,
                bounds: Bounds { min_x: min_x + self.ox, min_y: min_y + self.oy, max_x: max_x + self.ox, max_y: max_y + self.oy },
            }
        })
    }
    pub fn bounds(&self) -> Bounds {
        self.stats().bounds
    }

    /// The convex hull of the used vertices, counter-clockwise, in project coordinates.
    pub fn hull(&self) -> Vec<P2> {
        let used = self.used_vertices();
        let mut pts = Vec::new();
        for i in 0..self.x.len() {
            if used.g(i) != 0 {
                pts.push(p2(self.x.g(i) + self.ox, self.y.g(i) + self.oy));
            }
        }
        convex_hull(&pts)
    }

    pub fn grid(&self) -> &Grid {
        self.grid.get_or_init(|| {
            let s = self.stats();
            let b = Bounds {
                min_x: s.bounds.min_x - self.ox,
                min_y: s.bounds.min_y - self.oy,
                max_x: s.bounds.max_x - self.ox,
                max_y: s.bounds.max_y - self.oy,
            };
            let n = max(1.0, round((self.n_t as f64 / 2.0).sqrt()));
            let w = max(b.max_x - b.min_x, 1e-9);
            let h = max(b.max_y - b.min_y, 1e-9);
            let nx = max(1.0, round(n * (w / h).sqrt()));
            let ny = max(1.0, round(n * (h / w).sqrt()));
            // a NaN size (no finite vertex) leaves one cell
            let nx = if nx.is_finite() { nx as usize } else { 1 };
            let ny = if ny.is_finite() { ny as usize } else { 1 };
            let cw = w / nx as f64;
            let ch = h / ny as f64;
            let ncell = nx.saturating_mul(ny);
            let mut counts = TA::filled(ncell + 1, 0u32, 0);
            let cells = |t: usize, f: &mut dyn FnMut(usize)| {
                let (a, c, d) = (self.tv(3 * t), self.tv(3 * t + 1), self.tv(3 * t + 2));
                let x0 = min(min(self.vx(a), self.vx(c)), self.vx(d));
                let x1 = max(max(self.vx(a), self.vx(c)), self.vx(d));
                let y0 = min(min(self.vy(a), self.vy(c)), self.vy(d));
                let y1 = max(max(self.vy(a), self.vy(c)), self.vy(d));
                let (Some(i0), Some(i1), Some(j0), Some(j1)) = (
                    clamp_i((x0 - b.min_x) / cw, nx),
                    clamp_i((x1 - b.min_x) / cw, nx),
                    clamp_i((y0 - b.min_y) / ch, ny),
                    clamp_i((y1 - b.min_y) / ch, ny),
                ) else {
                    return;
                };
                for j in j0..=j1 {
                    for i in i0..=i1 {
                        f(j * nx + i);
                    }
                }
            };
            for t in 0..self.n_t {
                cells(t, &mut |k| {
                    if let Some(c) = counts.get_mut(k + 1) {
                        *c += 1;
                    }
                });
            }
            for k in 1..counts.len() {
                counts.s(k, counts.g(k) + counts.g(k - 1));
            }
            let total = counts.last().copied().unwrap_or(0) as usize;
            let mut items = vec![0u32; total];
            let mut fill: Vec<u32> = counts.get(..ncell).map(<[u32]>::to_vec).unwrap_or_default();
            for t in 0..self.n_t {
                cells(t, &mut |k| {
                    if let Some(f) = fill.get_mut(k) {
                        if let Some(s) = items.get_mut(*f as usize) {
                            *s = t as u32;
                        }
                        *f += 1;
                    }
                });
            }
            Grid { b, nx, ny, cw, ch, start: counts.into_vec(), items }
        })
    }

    /// The triangle under (local) x, y and its barycentric weights.
    pub fn locate(&self, lx: f64, ly: f64, tol: f64) -> Option<Located> {
        let g = self.grid();
        if lx < g.b.min_x - tol || lx > g.b.max_x + tol || ly < g.b.min_y - tol || ly > g.b.max_y + tol {
            return None;
        }
        let i = clamp_i((lx - g.b.min_x) / g.cw, g.nx)?;
        let j = clamp_i((ly - g.b.min_y) / g.ch, g.ny)?;
        let k = j * g.nx + i;
        let mut best: Option<Located> = None;
        let mut best_min = f64::NEG_INFINITY;
        let s0 = g.start.get(k).copied().unwrap_or(0) as usize;
        let s1 = g.start.get(k + 1).copied().unwrap_or(0) as usize;
        for s in s0..s1 {
            let t = g.items.get(s).copied().unwrap_or(0) as usize;
            let (a, b, c) = (self.tv(3 * t), self.tv(3 * t + 1), self.tv(3 * t + 2));
            let (xa, ya, xb, yb, xc, yc) = (self.vx(a), self.vy(a), self.vx(b), self.vy(b), self.vx(c), self.vy(c));
            let d = (yb - yc) * (xa - xc) + (xc - xb) * (ya - yc);
            if d == 0.0 {
                continue;
            }
            let w1 = ((yb - yc) * (lx - xc) + (xc - xb) * (ly - yc)) / d;
            let w2 = ((yc - ya) * (lx - xc) + (xa - xc) * (ly - yc)) / d;
            let w3 = 1.0 - w1 - w2;
            let m = min(min(w1, w2), w3);
            if m >= 0.0 {
                return Some(Located { t, w: [w1, w2, w3] });
            }
            if m > best_min {
                best_min = m;
                best = Some(Located { t, w: [w1, w2, w3] });
            }
        }
        if let Some(b) = best
            && tol > 0.0
        {
            // within tol of the triangle in plan (points on the outer edge)
            let t = b.t;
            let (a, bb, c) = (self.tv(3 * t), self.tv(3 * t + 1), self.tv(3 * t + 2));
            let scale = max(
                max(hypot(self.vx(bb) - self.vx(a), self.vy(bb) - self.vy(a)), hypot(self.vx(c) - self.vx(bb), self.vy(c) - self.vy(bb))),
                hypot(self.vx(a) - self.vx(c), self.vy(a) - self.vy(c)),
            );
            if -best_min * scale <= tol {
                let w = b.w.map(|v| max(0.0, v));
                let s = w[0] + w[1] + w[2];
                return Some(Located { t, w: w.map(|v| v / s) });
            }
        }
        None
    }

    /// `elevationAt(x, y, tol = 1e-9)`.
    pub fn elevation_at(&self, x: f64, y: f64, tol: f64) -> Option<f64> {
        let r = self.locate(x - self.ox, y - self.oy, tol)?;
        let t = r.t;
        Some(r.w[0] * self.vz(self.tv(3 * t)) + r.w[1] * self.vz(self.tv(3 * t + 1)) + r.w[2] * self.vz(self.tv(3 * t + 2)))
    }
    /// `elevationAt(x, y)` with the default tolerance.
    pub fn z_at(&self, x: f64, y: f64) -> Option<f64> {
        self.elevation_at(x, y, 1e-9)
    }

    /// Triangle t's plane: slope (rise/run), downhill direction (radians), gradient.
    pub fn slope_of(&self, t: usize) -> Slope {
        let (a, b, c) = (self.tv(3 * t), self.tv(3 * t + 1), self.tv(3 * t + 2));
        let ux = self.vx(b) - self.vx(a);
        let uy = self.vy(b) - self.vy(a);
        let uz = self.vz(b) - self.vz(a);
        let vx = self.vx(c) - self.vx(a);
        let vy = self.vy(c) - self.vy(a);
        let vz = self.vz(c) - self.vz(a);
        let nx = uy * vz - uz * vy;
        let ny = uz * vx - ux * vz;
        let nz = ux * vy - uy * vx;
        let gx = -nx / nz;
        let gy = -ny / nz;
        Slope { slope: hypot(gx, gy), dir: (-gy).jatan2(-gx), gx, gy }
    }

    pub fn slopes(&self) -> Vec<SlopeArrow> {
        let mut out = Vec::with_capacity(self.n_t);
        for t in 0..self.n_t {
            let (a, b, c) = (self.tv(3 * t), self.tv(3 * t + 1), self.tv(3 * t + 2));
            let s = self.slope_of(t);
            let size =
                ((self.vx(b) - self.vx(a)) * (self.vy(c) - self.vy(a)) - (self.vy(b) - self.vy(a)) * (self.vx(c) - self.vx(a))).abs() / 2.0;
            out.push(SlopeArrow {
                x: (self.vx(a) + self.vx(b) + self.vx(c)) / 3.0 + self.ox,
                y: (self.vy(a) + self.vy(b) + self.vy(c)) / 3.0 + self.oy,
                slope: s.slope,
                dir: s.dir,
                size: size.sqrt(),
            });
        }
        out
    }

    /// Contour polylines every `interval` (levels at base + k·interval), joined across triangles; None (the JS's
    /// null) when there would be more than `max_levels` levels.
    pub fn contours(&self, interval: f64, base: f64, max_levels: f64) -> Option<Vec<Contour>> {
        if !(interval > 0.0) {
            return Some(Vec::new());
        }
        let s = self.stats();
        let k0 = ((s.min_z - base) / interval - 1e-9).ceil();
        let k1 = ((s.max_z - base) / interval + 1e-9).floor();
        if k1 - k0 > max_levels {
            return None;
        }
        let mut out = Vec::new();
        let eps = interval * 1e-7;
        let tiny = 1e-6 * hypot(s.bounds.max_x - s.bounds.min_x, s.bounds.max_y - s.bounds.min_y);
        let mut k = k0;
        while k <= k1 {
            let l = base + k * interval;
            let zv = |i: usize| {
                let z = self.vz(i);
                if (z - l).abs() < eps { l + eps } else { z }
            };
            let mut segs: Vec<[Hit; 2]> = Vec::new();
            let cross = |i: usize, j: usize| -> Option<Hit> {
                let zi = zv(i);
                let zj = zv(j);
                if (zi > l) == (zj > l) {
                    return None;
                }
                let t = (l - zi) / (zj - zi);
                let key = if i < j { ((i as u64) << 32) | j as u64 } else { ((j as u64) << 32) | i as u64 };
                Some(Hit { k: key, x: self.vx(i) + (self.vx(j) - self.vx(i)) * t, y: self.vy(i) + (self.vy(j) - self.vy(i)) * t })
            };
            for t in (0..self.tris.len().saturating_sub(2)).step_by(3) {
                let (a, b, c) = (self.tv(t), self.tv(t + 1), self.tv(t + 2));
                let (za, zb, zc) = (zv(a), zv(b), zv(c));
                if (za > l && zb > l && zc > l) || (za < l && zb < l && zc < l) {
                    continue;
                }
                let hits: Vec<Hit> = [cross(a, b), cross(b, c), cross(c, a)].into_iter().flatten().collect();
                if let [h0, h1] = hits.as_slice() {
                    segs.push([*h0, *h1]);
                }
            }
            for line in chain(&segs) {
                if extent(&line.0) > tiny {
                    out.push(Contour { z: l, pts: line.0.iter().map(|p| p2(p.x + self.ox, p.y + self.oy)).collect(), closed: line.1 });
                }
            }
            k += 1.0;
        }
        Some(out)
    }

    /// Elevation band pieces: levels ascending; band i is l(i) ≤ z < l(i+1) (below l0 = -1).
    pub fn bands(&self, levels: &[f64]) -> Vec<BandPiece> {
        let mut out = Vec::new();
        for t in (0..self.tris.len().saturating_sub(2)).step_by(3) {
            let v: [[f64; 3]; 3] =
                [self.tv(t), self.tv(t + 1), self.tv(t + 2)].map(|i| [self.vx(i) + self.ox, self.vy(i) + self.oy, self.vz(i)]);
            let zmin = min(min(v[0][2], v[1][2]), v[2][2]);
            let zmax = max(max(v[0][2], v[1][2]), v[2][2]);
            let mut bi = band_index(levels, zmin);
            let bj = band_index(levels, zmax);
            if bi == bj {
                out.push(BandPiece { band: bi, ring: v.iter().flat_map(|p| [p[0], p[1]]).collect() });
                continue;
            }
            while bi <= bj {
                let mut poly = v.to_vec();
                if bi >= 0 {
                    poly = clip_z(&poly, levels.get(bi as usize).copied().unwrap_or(f64::NAN), 1.0);
                }
                if bi + 1 < levels.len() as i64 {
                    poly = clip_z(&poly, levels.get((bi + 1) as usize).copied().unwrap_or(f64::NAN), -1.0);
                }
                if poly.len() >= 3 {
                    out.push(BandPiece { band: bi, ring: poly.iter().flat_map(|p| [p[0], p[1]]).collect() });
                }
                bi += 1;
            }
        }
        out
    }

    /// `[...surface.triangles()]`: [ax,ay,az,bx,by,bz,cx,cy,cz] per triangle, project coordinates.
    pub fn triangles(&self) -> impl Iterator<Item = [f64; 9]> + '_ {
        (0..self.n_t).map(move |t| {
            let (a, b, c) = (self.tv(3 * t), self.tv(3 * t + 1), self.tv(3 * t + 2));
            [
                self.vx(a) + self.ox,
                self.vy(a) + self.oy,
                self.vz(a),
                self.vx(b) + self.ox,
                self.vy(b) + self.oy,
                self.vz(b),
                self.vx(c) + self.ox,
                self.vy(c) + self.oy,
                self.vz(c),
            ]
        })
    }

    /// Unique edges as vertex pairs (flat list a, b, a, b …).
    pub fn edges(&self) -> Vec<u32> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for t in (0..self.tris.len().saturating_sub(2)).step_by(3) {
            for k in 0..3 {
                let a = self.tris.g(t + k);
                let b = self.tris.g(t + (k + 1) % 3);
                let key = if a < b { (u64::from(a) << 32) | u64::from(b) } else { (u64::from(b) << 32) | u64::from(a) };
                if !seen.insert(key) {
                    continue;
                }
                out.push(a);
                out.push(b);
            }
        }
        out
    }

    /// The outer edge(s) of the surface as closed loops (project coordinates).
    pub fn outline(&self) -> Vec<Vec<P2>> {
        let mut count: Vec<(u32, u32)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for t in (0..self.tris.len().saturating_sub(2)).step_by(3) {
            for k in 0..3 {
                let a = self.tris.g(t + k);
                let b = self.tris.g(t + (k + 1) % 3);
                if seen.insert((a, b)) {
                    count.push((a, b));
                }
            }
        }
        // nextOf: insertion-ordered map a → b
        let mut order: Vec<u32> = Vec::new();
        let mut next_of: HashMap<u32, u32> = HashMap::new();
        for &(a, b) in &count {
            if !seen.contains(&(b, a)) {
                if !next_of.contains_key(&a) {
                    order.push(a);
                }
                next_of.insert(a, b);
            }
        }
        let mut loops = Vec::new();
        let mut cursor = 0usize;
        while !next_of.is_empty() {
            while cursor < order.len() && !next_of.contains_key(&order.g(cursor)) {
                cursor += 1;
            }
            let Some(&s) = order.get(cursor) else { break };
            let mut ring = Vec::new();
            let mut v = s;
            let mut g = 0usize;
            while g < 10_000_000 {
                let Some(n) = next_of.remove(&v) else { break };
                ring.push(p2(self.vx(v as usize) + self.ox, self.vy(v as usize) + self.oy));
                v = n;
                g += 1;
            }
            if ring.len() >= 3 {
                loops.push(ring);
            }
        }
        loops
    }

    /// Vertices in project coordinates and faces (for LandXML).
    pub fn to_faces(&self) -> (Vec<P3>, Vec<[u32; 3]>) {
        let used = self.used_vertices();
        let mut map = TA::filled(self.x.len(), -1i64, -1);
        let mut pts = Vec::new();
        for i in 0..self.x.len() {
            if used.g(i) != 0 {
                map.s(i, pts.len() as i64);
                pts.push(P3 { x: self.x.g(i) + self.ox, y: self.y.g(i) + self.oy, z: self.z.g(i) });
            }
        }
        let mut faces = Vec::with_capacity(self.n_t);
        for t in 0..self.n_t {
            let m = |k: usize| map.get(self.tv(k)).copied().unwrap_or(-1) as u32;
            faces.push([m(3 * t), m(3 * t + 1), m(3 * t + 2)]);
        }
        (pts, faces)
    }
}

#[derive(Clone, Copy, Debug)]
struct Hit {
    k: u64,
    x: f64,
    y: f64,
}
impl crate::js::Und for Hit {
    fn und() -> Self {
        Hit { k: u64::MAX, x: f64::NAN, y: f64::NAN }
    }
}

/// Size of a point list's box (a level that only touches a peak or pit leaves a dot-sized loop: dropped).
fn extent(pts: &[Hit]) -> f64 {
    let (mut a, mut b, mut c, mut d) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in pts {
        a = min(a, p.x);
        b = min(b, p.y);
        c = max(c, p.x);
        d = max(d, p.y);
    }
    max(c - a, d - b)
}
fn band_index(levels: &[f64], z: f64) -> i64 {
    let mut i: i64 = -1;
    while ((i + 1) as usize) < levels.len() && z >= levels.g((i + 1) as usize) {
        i += 1;
    }
    i
}
/// Keeps the part of a triangle polygon [x,y,z] with sign·(z − L) ≥ 0.
fn clip_z(poly: &[[f64; 3]], l: f64, sign: f64) -> Vec<[f64; 3]> {
    let mut out = Vec::new();
    let n = poly.len();
    for i in 0..n {
        let p = poly.g(i);
        let q = poly.g((i + 1) % n);
        let fp = sign * (p[2] - l);
        let fq = sign * (q[2] - l);
        if fp >= 0.0 {
            out.push(p);
        }
        if (fp >= 0.0) != (fq >= 0.0) {
            let t = fp / (fp - fq);
            out.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t, l]);
        }
    }
    out
}

/// Joins two-point segments that share edge keys into polylines: (points, closed).
fn chain(segs: &[[Hit; 2]]) -> Vec<(Vec<Hit>, bool)> {
    let mut keys: Vec<u64> = Vec::new();
    let mut at: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, s) in segs.iter().enumerate() {
        for h in s {
            match at.get_mut(&h.k) {
                Some(a) => a.push(i),
                None => {
                    keys.push(h.k);
                    at.insert(h.k, vec![i]);
                }
            }
        }
    }
    let mut used = vec![0u8; segs.len()];
    let mut out = Vec::new();
    let follow = |i: usize, from_key: u64, used: &mut Vec<u8>| -> Vec<Hit> {
        let mut pts = Vec::new();
        let mut cur = i;
        let mut k = from_key;
        loop {
            used.s(cur, 1);
            let s = &segs.g(cur);
            let next_h = if s[0].k == k { s[1] } else { s[0] };
            pts.push(next_h);
            let list = at.get(&next_h.k).map(Vec::as_slice).unwrap_or(&[]);
            let n = list.iter().copied().find(|&j| used.g(j) == 0);
            match n {
                None => return pts,
                Some(n) => {
                    cur = n;
                    k = next_h.k;
                }
            }
        }
    };
    // open lines start at keys used once
    for &k in &keys {
        let list = at.get(&k).map(Vec::as_slice).unwrap_or(&[]);
        if list.len() != 1 || used.g(list.g(0)) != 0 {
            continue;
        }
        let s = &segs.g(list.g(0));
        let start_h = if s[0].k == k { s[0] } else { s[1] };
        let r = follow(list.g(0), k, &mut used);
        let mut pts = vec![start_h];
        pts.extend(r);
        out.push((pts, false));
    }
    for i in 0..segs.len() {
        if used.g(i) != 0 {
            continue;
        }
        let s = &segs.g(i);
        let r = follow(i, s[0].k, &mut used);
        let mut pts = vec![s[0]];
        pts.extend(r);
        out.push((pts, true));
    }
    out
}

// ---------------------------------------------------------------------------
// Plane geometry helpers (exported for grading and the UI)

pub fn convex_hull<T: XY>(pts: &[T]) -> Vec<P2> {
    convex_hull_idx(pts).into_iter().filter_map(|i| pts.get(i)).map(|q| p2(q.x(), q.y())).collect()
}
/// `convexHull(pts)` as indices into `pts` (the JS returns the input objects themselves).
pub fn convex_hull_idx<T: XY>(pts: &[T]) -> Vec<usize> {
    let mut p: Vec<(P2, usize)> = pts.iter().enumerate().map(|(i, q)| (p2(q.x(), q.y()), i)).collect();
    crate::js::sort_by(&mut p, |a, b| {
        let d = a.0.x - b.0.x;
        if d != 0.0 && !d.is_nan() { d } else { a.0.y - b.0.y }
    });
    if p.len() < 3 {
        return p.into_iter().map(|q| q.1).collect();
    }
    let cr = |o: &P2, a: &P2, b: &P2| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let mut lo: Vec<(P2, usize)> = Vec::new();
    let mut hi: Vec<(P2, usize)> = Vec::new();
    let last2 = |v: &[(P2, usize)]| match v {
        [.., a, b] => Some((a.0, b.0)),
        _ => None,
    };
    for q in &p {
        while let Some((a, b)) = last2(&lo)
            && cr(&a, &b, &q.0) <= 0.0
        {
            lo.pop();
        }
        lo.push(*q);
    }
    for q in p.iter().rev() {
        while let Some((a, b)) = last2(&hi)
            && cr(&a, &b, &q.0) <= 0.0
        {
            hi.pop();
        }
        hi.push(*q);
    }
    lo.pop();
    hi.pop();
    lo.extend(hi);
    lo.into_iter().map(|q| q.1).collect()
}
pub fn poly_area<T: XY + Copy + crate::js::Und>(pts: &[T]) -> f64 {
    let mut a = 0.0;
    let n = pts.len();
    for i in 0..n {
        let p = &pts.g(i);
        let q = &pts.g((i + 1) % n);
        a += p.x() * q.y() - q.x() * p.y();
    }
    a / 2.0
}
pub fn ccw<T: XY + Copy + crate::js::Und>(pts: &[T]) -> Vec<T> {
    let mut v = pts.to_vec();
    if poly_area(pts) < 0.0 {
        v.reverse();
    }
    v
}
pub fn point_in_poly<T: XY + Copy + crate::js::Und>(x: f64, y: f64, pts: &[T]) -> bool {
    let mut inside = false;
    let n = pts.len();
    if n == 0 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let a = &pts.g(i);
        let b = &pts.g(j);
        if (a.y() > y) != (b.y() > y) && x < ((b.x() - a.x()) * (y - a.y())) / (b.y() - a.y()) + a.x() {
            inside = !inside;
        }
        j = i;
    }
    inside
}
/// Sutherland–Hodgman: subject polygon clipped by a convex counter-clockwise polygon.
pub fn clip_convex<T: XY>(subject: &[T], clip: &[P2]) -> Vec<P2> {
    let mut out: Vec<P2> = subject.iter().map(|p| p2(p.x(), p.y())).collect();
    for i in 0..clip.len() {
        if out.is_empty() {
            break;
        }
        let a = clip.g(i);
        let b = clip.g((i + 1) % clip.len());
        let inside = |p: &P2| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x) >= -1e-9;
        let inp = std::mem::take(&mut out);
        for k in 0..inp.len() {
            let p = inp.g(k);
            let q = inp.g((k + 1) % inp.len());
            let ip = inside(&p);
            let iq = inside(&q);
            if ip {
                out.push(p);
            }
            if ip != iq {
                let dx = q.x - p.x;
                let dy = q.y - p.y;
                let ex = b.x - a.x;
                let ey = b.y - a.y;
                let t = ((a.x - p.x) * ey - (a.y - p.y) * ex) / (dx * ey - dy * ex);
                out.push(p2(p.x + dx * t, p.y + dy * t));
            }
        }
    }
    // drop repeats
    let n = out.len();
    (0..n).filter(|&i| hypot(out.g(i).x - out.g((i + 1) % n).x, out.g(i).y - out.g((i + 1) % n).y) > 1e-9).map(|i| out.g(i)).collect()
}

/// The PZ form of a P2 list (corners without their own elevation).
pub fn pz_of(p: &[P2]) -> Vec<PZ> {
    p.iter().map(|q| PZ { x: q.x, y: q.y, z: None }).collect()
}
