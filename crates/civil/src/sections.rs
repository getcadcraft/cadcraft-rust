//! sections.js — sample lines and cross sections along an alignment (0.4 V13): exact TIN sections, cut/fill areas,
//! corridor marks, graded regions, average-end-area volumes, the sections CSV.

use crate::alignment::Alignment;
use crate::geo::{P2, P3, p2};
use crate::i18n::t;
use crate::js::{J, JsMath, Obj, hypot, max, min, round_fixed, to_fixed};
use crate::tin::Surface;
use serde_json::Value;

pub const MAX_SECTIONS: usize = 2000;

/// `{ step, left, right, extra }`.
#[derive(Clone, Debug, PartialEq)]
pub struct Sampling {
    pub step: f64,
    pub left: f64,
    pub right: f64,
    pub extra: Vec<f64>,
}
impl J for Sampling {
    fn j(&self) -> Value {
        crate::jo! { "step" => self.step, "left" => self.left, "right" => self.right, "extra" => self.extra }
    }
}
/// `SAMPLE_DEFAULTS(units)` (no extra).
pub fn sample_defaults(units: &str) -> Sampling {
    if units == "m" {
        Sampling { step: 20.0, left: 30.0, right: 30.0, extra: Vec::new() }
    } else {
        Sampling { step: 50.0, left: 100.0, right: 100.0, extra: Vec::new() }
    }
}
/// `cleanSampling(o, units)` (untrusted).
pub fn clean_sampling(o: Option<&Value>, units: &str) -> Option<Sampling> {
    let o = o.filter(|o| o.is_object() || o.is_array())?;
    let d = sample_defaults(units);
    let pos = |v: Option<&Value>, dv: f64, mx: f64| crate::js::fin(v).filter(|&x| x > 0.0 && x <= mx).unwrap_or(dv);
    let mut extra: Vec<f64> =
        crate::js::arr(o.get("extra")).iter().filter_map(|v| crate::js::fin(Some(v)).filter(|x| x.abs() < 1e8)).take(500).collect();
    let mut seen = std::collections::HashSet::new();
    extra.retain(|x| seen.insert(if *x == 0.0 { 0 } else { x.to_bits() }));
    crate::js::sort_by(&mut extra, |a, b| a - b);
    Some(Sampling {
        step: pos(o.get("step"), d.step, 1e5),
        left: pos(o.get("left"), d.left, 1e4),
        right: pos(o.get("right"), d.right, 1e4),
        extra,
    })
}
/// A sampling as given (trusted).
pub fn read_sampling(o: &Value) -> Sampling {
    let n = |k: &str| crate::js::numf(o.get(k));
    Sampling {
        step: n("step"),
        left: n("left"),
        right: n("right"),
        extra: crate::js::arr(o.get("extra")).iter().map(|v| crate::js::numf(Some(v))).collect(),
    }
}

fn set_add(v: &mut Vec<f64>, seen: &mut std::collections::HashSet<u64>, x: f64) {
    let k = if x == 0.0 {
        0
    } else if x.is_nan() {
        1
    } else {
        x.to_bits()
    };
    if seen.insert(k) {
        v.push(x);
    }
}

/// `sampleStations(al, { step, extra, from, to })`.
pub fn sample_stations(al: &Alignment, step: f64, extra: &[f64], from: f64, to: f64) -> Vec<f64> {
    if !al.ok() {
        return Vec::new();
    }
    let lo = max(al.start, if from.is_finite() { from } else { al.start });
    let hi = min(al.end, if to.is_finite() { to } else { al.end });
    if !(hi >= lo) {
        return Vec::new();
    }
    let mut st = Vec::new();
    let mut seen = std::collections::HashSet::new();
    set_add(&mut st, &mut seen, lo);
    set_add(&mut st, &mut seen, hi);
    if step > 0.0 {
        let mut k = (lo / step - 1e-9).ceil();
        while k * step < hi - 1e-9 && st.len() < MAX_SECTIONS {
            set_add(&mut st, &mut seen, round_fixed(k * step, 6));
            k += 1.0;
        }
    }
    for (s, _) in al.key_stations() {
        if s > lo && s < hi {
            set_add(&mut st, &mut seen, s);
        }
    }
    for &s in extra {
        if s >= lo && s <= hi {
            set_add(&mut st, &mut seen, s);
        }
    }
    crate::js::sort_by(&mut st, |a, b| a - b);
    let mut out: Vec<f64> = Vec::new();
    for (i, &s) in st.iter().enumerate() {
        if i == 0 || st.get(i - 1).is_some_and(|&p| s - p > 1e-6) {
            out.push(s);
        }
    }
    out.truncate(MAX_SECTIONS);
    out
}

/// `sampleLine(al, sta, left, right)`.
#[derive(Clone, Copy, Debug)]
pub struct Line {
    pub sta: f64,
    pub c: P2,
    pub a: P2,
    pub b: P2,
    pub dir: f64,
    pub left: f64,
    pub right: f64,
}
impl J for Line {
    fn j(&self) -> Value {
        crate::jo! { "sta" => self.sta, "c" => self.c, "a" => self.a, "b" => self.b, "dir" => self.dir, "left" => self.left, "right" => self.right }
    }
}
pub fn sample_line(al: &Alignment, sta: f64, left: f64, right: f64) -> Option<Line> {
    let p = al.point_at(sta)?;
    let rx = p.dir.jsin();
    let ry = -p.dir.jcos();
    Some(Line {
        sta,
        c: p2(p.x, p.y),
        a: p2(p.x - rx * left, p.y - ry * left),
        b: p2(p.x + rx * right, p.y + ry * right),
        dir: p.dir,
        left,
        right,
    })
}

/// A section point `{ o, z }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OZ {
    pub o: f64,
    pub z: f64,
}
impl J for OZ {
    fn j(&self) -> Value {
        crate::jo! { "o" => self.o, "z" => self.z }
    }
}
/// A section point whose z may be missing (clipRuns keeps a null z when zAt finds none).
pub type Run = Vec<OZ>;

/// `surfaceSection(surface, a, b, offA, offB)` → runs.
pub fn surface_section(surface: Option<&Surface>, a: P2, b: P2, off_a: f64, off_b: f64) -> Vec<Run> {
    let Some(surface) = surface else { return Vec::new() };
    let (ox, oy) = (surface.ox, surface.oy);
    let ax = a.x - ox;
    let ay = a.y - oy;
    let bx = b.x - ox;
    let by = b.y - oy;
    let dx = bx - ax;
    let dy = by - ay;
    let l = hypot(dx, dy);
    if !(l > 0.0) {
        return Vec::new();
    }
    let mut ts: Vec<f64> = vec![0.0, 1.0];
    let g = surface.grid();
    let clamp = |v: f64, n: usize| -> f64 { max(0.0, min(n as f64 - 1.0, v.floor())) };
    let x0 = min(ax, bx);
    let x1 = max(ax, bx);
    let y0 = min(ay, by);
    let y1 = max(ay, by);
    if !(x1 < g.b.min_x || x0 > g.b.max_x || y1 < g.b.min_y || y0 > g.b.max_y) {
        let i0 = clamp((x0 - g.b.min_x) / g.cw, g.nx);
        let i1 = clamp((x1 - g.b.min_x) / g.cw, g.nx);
        let j0 = clamp((y0 - g.b.min_y) / g.ch, g.ny);
        let j1 = clamp((y1 - g.b.min_y) / g.ch, g.ny);
        let mut seen = std::collections::HashSet::new();
        let mut j = j0;
        while j <= j1 {
            let mut i = i0;
            while i <= i1 {
                let cx0 = g.b.min_x + i * g.cw;
                let cy0 = g.b.min_y + j * g.ch;
                let (ii, jj) = (i, j);
                i += 1.0;
                if !seg_hits_box(ax, ay, bx, by, cx0, cy0, cx0 + g.cw, cy0 + g.ch) {
                    continue;
                }
                let (Some(iu), Some(ju)) = (crate::js::usize_of(ii), crate::js::usize_of(jj)) else { continue };
                let k = ju.saturating_mul(g.nx).saturating_add(iu);
                let s0 = g.start.get(k).copied().unwrap_or(0);
                let s1 = g.start.get(k.saturating_add(1)).copied().unwrap_or(0);
                for s in s0..s1 {
                    let Some(&tri) = g.items.get(s as usize) else { continue };
                    if !seen.insert(tri) {
                        continue;
                    }
                    let tb = (tri as usize).saturating_mul(3);
                    for e in 0..3usize {
                        let p = surface.tris.g(tb.saturating_add(e)) as usize;
                        let q = surface.tris.g(tb.saturating_add((e + 1) % 3)) as usize;
                        let (px, py, qx, qy) = (surface.x.g(p), surface.y.g(p), surface.x.g(q), surface.y.g(q));
                        if let Some(tt) = seg_cross(ax, ay, dx, dy, px, py, qx - px, qy - py) {
                            ts.push(tt);
                        }
                    }
                }
            }
            j += 1.0;
        }
    }
    crate::js::sort_by(&mut ts, |u, v| u - v);
    let mut runs: Vec<Run> = Vec::new();
    let mut in_run = false;
    let mut last = -1.0;
    for tt in ts {
        if tt - last < 1e-12 {
            continue;
        }
        last = tt;
        let z = surface.elevation_at(a.x + (b.x - a.x) * tt, a.y + (b.y - a.y) * tt, 1e-7);
        let o = off_a + (off_b - off_a) * tt;
        let Some(z) = z else {
            in_run = false;
            continue;
        };
        if !in_run {
            runs.push(Vec::new());
            in_run = true;
        }
        if let Some(r) = runs.last_mut() {
            r.push(OZ { o, z });
        }
    }
    runs.retain(|r| r.len() >= 2);
    runs
}
/// segment p + t·r vs q + u·s → t in [0,1] or None.
#[allow(clippy::too_many_arguments)]
fn seg_cross(px: f64, py: f64, rx: f64, ry: f64, qx: f64, qy: f64, sx: f64, sy: f64) -> Option<f64> {
    let den = rx * sy - ry * sx;
    if !(den.abs() >= 1e-15) {
        return None;
    }
    let t0 = ((qx - px) * sy - (qy - py) * sx) / den;
    let u = ((qx - px) * ry - (qy - py) * rx) / den;
    if (-1e-12..=1.0 + 1e-12).contains(&t0) && (-1e-12..=1.0 + 1e-12).contains(&u) { Some(max(0.0, min(1.0, t0))) } else { None }
}
/// Liang–Barsky.
#[allow(clippy::too_many_arguments)]
fn seg_hits_box(ax: f64, ay: f64, bx: f64, by: f64, x0: f64, y0: f64, x1: f64, y1: f64) -> bool {
    let mut t0 = 0.0;
    let mut t1 = 1.0;
    let dx = bx - ax;
    let dy = by - ay;
    for (p, q) in [(-dx, ax - x0), (dx, x1 - ax), (-dy, ay - y0), (dy, y1 - ay)] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
            continue;
        }
        let r = q / p;
        if p < 0.0 {
            if r > t1 {
                return false;
            }
            if r > t0 {
                t0 = r;
            }
        } else {
            if r < t0 {
                return false;
            }
            if r < t1 {
                t1 = r;
            }
        }
    }
    true
}

/// `zAt(runs, o)`.
pub fn z_at(runs: &[Run], o: f64) -> Option<f64> {
    for r in runs {
        let (Some(first), Some(lastp)) = (r.first(), r.last()) else { continue };
        if o < first.o - 1e-9 || o > lastp.o + 1e-9 {
            continue;
        }
        for w in r.windows(2) {
            let [p, q] = w else { continue };
            if o <= q.o + 1e-9 {
                let d = q.o - p.o;
                return Some(if d > 0.0 { p.z + ((q.z - p.z) * (o - p.o)) / d } else { q.z });
            }
        }
    }
    None
}

/// A cut or fill piece.
#[derive(Clone, Copy, Debug)]
pub struct Part {
    pub o0: f64,
    pub o1: f64,
    pub cut: bool,
}
impl J for Part {
    fn j(&self) -> Value {
        crate::jo! { "o0" => self.o0, "o1" => self.o1, "kind" => if self.cut { "cut" } else { "fill" } }
    }
}

/// `cutFill(ground, design)` → (cut, fill, parts).
pub fn cut_fill(ground: &[Run], design: &[Run]) -> (f64, f64, Vec<Part>) {
    let mut os: Vec<f64> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for r in ground.iter().chain(design.iter()) {
        for p in r {
            set_add(&mut os, &mut seen, p.o);
        }
    }
    crate::js::sort_by(&mut os, |a, b| a - b);
    let (mut cut, mut fill) = (0.0f64, 0.0f64);
    let mut parts: Vec<Part> = Vec::new();
    let mut push = |o0: f64, o1: f64, is_cut: bool, area: f64, parts: &mut Vec<Part>| {
        if !(area > 0.0) || o1 - o0 <= 0.0 {
            return;
        }
        match parts.last_mut() {
            Some(l) if l.cut == is_cut && (l.o1 - o0).abs() < 1e-9 => l.o1 = o1,
            _ => parts.push(Part { o0, o1, cut: is_cut }),
        }
        if is_cut {
            cut += area;
        } else {
            fill += area;
        }
    };
    for w in os.windows(2) {
        let [o0, o1] = [w.first().copied().unwrap_or(f64::NAN), w.get(1).copied().unwrap_or(f64::NAN)];
        let om = (o0 + o1) / 2.0;
        if z_at(ground, om).is_none() || z_at(design, om).is_none() {
            continue;
        }
        let (Some(g0), Some(g1), Some(d0), Some(d1)) = (z_at(ground, o0), z_at(ground, o1), z_at(design, o0), z_at(design, o1)) else {
            continue;
        };
        let e0 = d0 - g0;
        let e1 = d1 - g1;
        let wd = o1 - o0;
        if e0 >= 0.0 && e1 >= 0.0 {
            push(o0, o1, false, ((e0 + e1) / 2.0) * wd, &mut parts);
        } else if e0 <= 0.0 && e1 <= 0.0 {
            push(o0, o1, true, ((-e0 - e1) / 2.0) * wd, &mut parts);
        } else {
            // the lines cross inside the interval
            let om2 = o0 + (wd * e0) / (e0 - e1);
            let a0 = (e0.abs() / 2.0) * (om2 - o0);
            let a1 = (e1.abs() / 2.0) * (o1 - om2);
            push(o0, om2, !(e0 > 0.0), a0, &mut parts);
            push(om2, o1, !(e1 > 0.0), a1, &mut parts);
        }
    }
    (cut, fill, parts)
}

/// A corridor mark `{ code, o, z }`.
#[derive(Clone, Debug)]
pub struct Mark {
    pub code: String,
    pub o: f64,
    pub z: Option<f64>,
}
impl J for Mark {
    fn j(&self) -> Value {
        crate::jo! { "code" => self.code, "o" => self.o, "z" => self.z }
    }
}
/// `featureMarks(lines, line)`: lines as (code, points with optional z).
pub fn feature_marks(lines: &[(String, Vec<P3>)], line: &Line) -> Vec<Mark> {
    let mut out = Vec::new();
    let (a, b) = (line.a, line.b);
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    for (code, pts) in lines {
        for w in pts.windows(2) {
            let [p, q] = w else { continue };
            let Some(tt) = seg_cross(a.x, a.y, dx, dy, p.x, p.y, q.x - p.x, q.y - p.y) else { continue };
            let u = seg_cross(p.x, p.y, q.x - p.x, q.y - p.y, a.x, a.y, dx, dy);
            let z = match u {
                Some(u) if p.z.is_finite() && q.z.is_finite() => Some(p.z + (q.z - p.z) * u),
                _ => None,
            };
            out.push(Mark { code: code.clone(), o: -line.left + (line.left + line.right) * tt, z });
            break;
        }
    }
    crate::js::sort_by(&mut out, |m, n| m.o - n.o);
    out
}

/// `regionSpans(line, regions)`.
pub fn region_spans(line: &Line, regions: &[Vec<P3>]) -> Vec<[f64; 2]> {
    let (a, b) = (line.a, line.b);
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let mut ts = Vec::new();
    let mut seen = std::collections::HashSet::new();
    set_add(&mut ts, &mut seen, 0.0);
    set_add(&mut ts, &mut seen, 1.0);
    for r in regions {
        let n = r.len();
        for i in 0..n {
            let (Some(p), Some(q)) = (r.get(i), r.get((i + 1) % n)) else { continue };
            if let Some(tt) = seg_cross(a.x, a.y, dx, dy, p.x, p.y, q.x - p.x, q.y - p.y) {
                set_add(&mut ts, &mut seen, tt);
            }
        }
    }
    crate::js::sort_by(&mut ts, |m, n| m - n);
    // inside, or on the outline (a section along a corridor's end edge is the corridor's end section)
    let on_edge = |x: f64, y: f64, r: &[P3]| {
        let n = r.len();
        (0..n).any(|i| {
            let (Some(p), Some(q)) = (r.get(i), r.get((i + 1) % n)) else { return false };
            let ex = q.x - p.x;
            let ey = q.y - p.y;
            let l = ex * ex + ey * ey;
            let u = if l != 0.0 { max(0.0, min(1.0, ((x - p.x) * ex + (y - p.y) * ey) / l)) } else { 0.0 };
            hypot(x - p.x - u * ex, y - p.y - u * ey) <= 1e-6 * max(1.0, l.sqrt())
        })
    };
    let inside = |x: f64, y: f64| regions.iter().any(|r| crate::tin::point_in_poly(x, y, r) || on_edge(x, y, r));
    let mut out: Vec<[f64; 2]> = Vec::new();
    for w in ts.windows(2) {
        let (t0, t1) = (w.first().copied().unwrap_or(f64::NAN), w.get(1).copied().unwrap_or(f64::NAN));
        let tm = (t0 + t1) / 2.0;
        if t1 - t0 < 1e-12 || !inside(a.x + dx * tm, a.y + dy * tm) {
            continue;
        }
        let o0 = -line.left + (line.left + line.right) * t0;
        let o1 = -line.left + (line.left + line.right) * t1;
        match out.last_mut() {
            Some(l) if (l[1] - o0).abs() < 1e-9 => l[1] = o1,
            _ => out.push([o0, o1]),
        }
    }
    out
}

/// `clipRuns(runs, spans)` (z of the cut ends may be missing: NaN stands for the JS's null there).
pub fn clip_runs(runs: &[Run], spans: &[[f64; 2]]) -> Vec<Run> {
    let mut out = Vec::new();
    for &[o0, o1] in spans {
        for r in runs {
            let (Some(first), Some(lastp)) = (r.first(), r.last()) else { continue };
            if lastp.o <= o0 || first.o >= o1 {
                continue;
            }
            let rr = std::slice::from_ref(r);
            let a = max(o0, first.o);
            let bb = min(o1, lastp.o);
            let mut pts = vec![OZ { o: a, z: z_at(rr, a).unwrap_or(f64::NAN) }];
            for q in r {
                if q.o > o0 + 1e-12 && q.o < o1 - 1e-12 {
                    pts.push(*q);
                }
            }
            pts.push(OZ { o: bb, z: z_at(rr, bb).unwrap_or(f64::NAN) });
            if pts.len() >= 2 && pts.last().map_or(f64::NAN, |p| p.o) - a > 1e-12 {
                out.push(pts);
            }
        }
    }
    out
}

/// One section.
#[derive(Clone, Debug)]
pub struct Sec {
    pub sta: f64,
    pub a: P2,
    pub b: P2,
    pub c: P2,
    pub left: f64,
    pub right: f64,
    pub ground: Vec<Run>,
    pub design: Vec<Run>,
    pub cut: f64,
    pub fill: f64,
    pub parts: Vec<Part>,
    pub marks: Vec<Mark>,
    /// the design is the ground itself (the JS's `design === ground`)
    pub same: bool,
}
fn runs_j(r: &[Run]) -> Value {
    Value::Array(
        r.iter()
            .map(|run| {
                Value::Array(
                    run.iter().map(|p| crate::jo! { "o" => p.o, "z" => if p.z.is_nan() { Value::Null } else { p.z.j() } }).collect(),
                )
            })
            .collect(),
    )
}
impl J for Sec {
    fn j(&self) -> Value {
        Obj::new()
            .set("sta", self.sta)
            .set("a", self.a)
            .set("b", self.b)
            .set("c", self.c)
            .set("left", self.left)
            .set("right", self.right)
            .set("ground", runs_j(&self.ground))
            .set("design", runs_j(&self.design))
            .set("cut", self.cut)
            .set("fill", self.fill)
            .set("parts", &self.parts)
            .set("marks", &self.marks)
            .done()
    }
}
/// A volume row.
#[derive(Clone, Copy, Debug)]
pub struct Row {
    pub sta: f64,
    pub cut: f64,
    pub fill: f64,
    pub cut_vol: f64,
    pub fill_vol: f64,
    pub cum_cut: f64,
    pub cum_fill: f64,
}
impl J for Row {
    fn j(&self) -> Value {
        crate::jo! { "sta" => self.sta, "cut" => self.cut, "fill" => self.fill, "cutVol" => self.cut_vol, "fillVol" => self.fill_vol, "cumCut" => self.cum_cut, "cumFill" => self.cum_fill }
    }
}
/// `buildSections(…)` → { sections, rows, totals }.
#[derive(Clone, Debug, Default)]
pub struct SectionSet {
    pub sections: Vec<Sec>,
    pub rows: Vec<Row>,
    pub total_cut: f64,
    pub total_fill: f64,
}
impl J for SectionSet {
    fn j(&self) -> Value {
        crate::jo! { "sections" => self.sections, "rows" => self.rows, "totals" => crate::jo! { "cut" => self.total_cut, "fill" => self.total_fill } }
    }
}

/// `buildSections({ alignment, ground, design, regions, corridor, sampling, from, to })`; `design` None = the ground.
#[allow(clippy::too_many_arguments)]
pub fn build_sections(
    alignment: &Alignment,
    ground: Option<&Surface>,
    design: Option<&Surface>,
    design_is_ground: bool,
    regions: Option<&[Vec<P3>]>,
    corridor: Option<&[(String, Vec<P3>)]>,
    sampling: Option<&Sampling>,
    from: f64,
    to: f64,
) -> SectionSet {
    let dflt = sample_defaults("ft");
    let smp = sampling.unwrap_or(&dflt);
    let stations = sample_stations(alignment, smp.step, &smp.extra, from, to);
    let mut sections = Vec::new();
    for sta in stations {
        let Some(line) = sample_line(alignment, sta, smp.left, smp.right) else { continue };
        let g = surface_section(ground, line.a, line.b, -smp.left, smp.right);
        let separate = design.is_some() && !design_is_ground;
        let mut d = if separate { surface_section(design, line.a, line.b, -smp.left, smp.right) } else { g.clone() };
        if separate && let Some(regions) = regions {
            d = clip_runs(&d, &region_spans(&line, regions));
        }
        let (cut, fill, parts) = cut_fill(&g, &d);
        let marks = corridor.map_or_else(Vec::new, |c| feature_marks(c, &line));
        sections.push(Sec {
            sta,
            a: line.a,
            b: line.b,
            c: line.c,
            left: smp.left,
            right: smp.right,
            ground: g,
            design: d,
            cut,
            fill,
            parts,
            marks,
            same: !separate,
        });
    }
    let mut rows = Vec::new();
    let (mut cum_cut, mut cum_fill) = (0.0f64, 0.0f64);
    for (i, s) in sections.iter().enumerate() {
        let (mut cut_vol, mut fill_vol) = (0.0, 0.0);
        if i > 0
            && let Some(p) = sections.get(i - 1)
        {
            let l = s.sta - p.sta;
            cut_vol = ((p.cut + s.cut) / 2.0) * l;
            fill_vol = ((p.fill + s.fill) / 2.0) * l;
        }
        cum_cut += cut_vol;
        cum_fill += fill_vol;
        rows.push(Row { sta: s.sta, cut: s.cut, fill: s.fill, cut_vol, fill_vol, cum_cut, cum_fill });
    }
    SectionSet { sections, rows, total_cut: cum_cut, total_fill: cum_fill }
}

fn csv_cell(s: &str) -> String {
    if s.contains(['"', ',', '\r', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}
/// `sectionsCsv(set, { units, station })`.
pub fn sections_csv(set: &SectionSet, units: &str, station: &dyn Fn(f64) -> String) -> String {
    let ft = units != "m";
    let vol_per = if ft { 27.0 } else { 1.0 };
    let keys: [&str; 7] = if ft {
        [
            "sec.csv.station",
            "sec.csv.cut.area.ft",
            "sec.csv.fill.area.ft",
            "sec.csv.cut.vol.ft",
            "sec.csv.fill.vol.ft",
            "sec.csv.cum.cut.ft",
            "sec.csv.cum.fill.ft",
        ]
    } else {
        [
            "sec.csv.station",
            "sec.csv.cut.area.m",
            "sec.csv.fill.area.m",
            "sec.csv.cut.vol.m",
            "sec.csv.fill.vol.m",
            "sec.csv.cum.cut.m",
            "sec.csv.cum.fill.m",
        ]
    };
    let mut lines = vec![keys.iter().map(|k| csv_cell(&t(k, &[]))).collect::<Vec<_>>().join(",")];
    for r in &set.rows {
        let cells = [
            station(r.sta),
            to_fixed(r.cut, 2),
            to_fixed(r.fill, 2),
            to_fixed(r.cut_vol / vol_per, 1),
            to_fixed(r.fill_vol / vol_per, 1),
            to_fixed(r.cum_cut / vol_per, 1),
            to_fixed(r.cum_fill / vol_per, 1),
        ];
        lines.push(cells.iter().map(|c| csv_cell(c)).collect::<Vec<_>>().join(","));
    }
    format!("{}\r\n", lines.join("\r\n"))
}
