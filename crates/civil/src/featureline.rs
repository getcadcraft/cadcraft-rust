//! featureline.js — feature lines with grades (0.4 V14), feature lines in profile views (0.5 WL.20) and the grade
//! and elevation overlay (0.5 WL.21).

use crate::alignment::{Alignment, Element};
use crate::geo::{P3, PZ};
use crate::grading::{Pad, Patch};
use crate::js::{J, JsMath, Obj, hypot, max, max_of, min, min_of, round, slice16, strip_controls, to_fixed, trim};
use crate::tin::Breakline;
use crate::tr;
use serde_json::Value;
use std::f64::consts::PI;

pub const USES: [&str; 3] = ["eg", "design", "off"];
pub const MAX_VERTICES: usize = 10000;

/// A feature line.
#[derive(Clone, Debug, PartialEq)]
pub struct Feature {
    pub id: Option<String>,
    pub name: String,
    pub pts: Vec<P3>,
    pub closed: bool,
    pub use_: String,
    pub daylight: bool,
    pub cut: f64,
    pub fill: f64,
}
impl J for Feature {
    fn j(&self) -> Value {
        crate::jo! {
            "id" => self.id, "name" => self.name, "pts" => self.pts, "closed" => self.closed, "use" => self.use_,
            "daylight" => self.daylight, "cut" => self.cut, "fill" => self.fill,
        }
    }
}
/// A feature as given (trusted).
pub fn read_feature(o: &Value) -> Feature {
    Feature {
        id: o.get("id").and_then(Value::as_str).map(str::to_string),
        name: crate::js::string_or_empty(o.get("name")),
        pts: crate::geo::read_p3s(o.get("pts")),
        closed: o.get("closed") == Some(&Value::Bool(true)),
        use_: crate::js::string_or_empty(o.get("use")),
        daylight: o.get("daylight") == Some(&Value::Bool(true)),
        cut: crate::js::numf(o.get("cut")),
        fill: crate::js::numf(o.get("fill")),
    }
}

/// `FEATURE_DEFAULTS(units)` → (use, daylight, cut, fill, offset).
pub fn feature_defaults(units: &str) -> (&'static str, bool, f64, f64, f64) {
    ("eg", false, 2.0, 3.0, if units == "m" { 1.0 } else { 3.0 })
}

fn fin(v: Option<&Value>) -> Option<f64> {
    crate::js::fin(v)
}

/// `cleanFeature(o, units)` (untrusted).
pub fn clean_feature(o: &Value, units: &str) -> Option<Feature> {
    if !o.is_object() {
        return None;
    }
    let pts_v = o.get("pts")?.as_array()?;
    let pts: Vec<P3> = pts_v
        .iter()
        .take(MAX_VERTICES)
        .filter_map(|p| {
            let x = fin(p.get("x"))?;
            let y = fin(p.get("y"))?;
            if !(x.abs() < 1e9 && y.abs() < 1e9) {
                return None;
            }
            Some(P3 { x, y, z: fin(p.get("z")).filter(|z| z.abs() < 1e6).unwrap_or(0.0) })
        })
        .collect();
    if pts.len() < 2 {
        return None;
    }
    let d = feature_defaults(units);
    let closed = o.get("closed") == Some(&Value::Bool(true)) && pts.len() >= 3;
    let id_s = crate::js::string_of(o.get("id"));
    let id_ok = id_s.len() >= 2
        && id_s.len() <= 10
        && id_s.starts_with('f')
        && id_s.get(1..).is_some_and(|d| d.bytes().all(|b| b.is_ascii_digit()));
    let name = slice16(trim(&strip_controls(&crate::js::string_or_empty(o.get("name")), " ")), 120);
    Some(Feature {
        id: id_ok.then_some(id_s),
        name: if name.is_empty() { tr!("toolbar.feature") } else { name },
        pts,
        closed,
        use_: o.get("use").and_then(Value::as_str).filter(|u| USES.contains(u)).unwrap_or(d.0).to_string(),
        daylight: o.get("daylight") == Some(&Value::Bool(true)),
        cut: fin(o.get("cut")).filter(|&c| c > 0.0 && c < 1000.0).unwrap_or(d.2),
        fill: fin(o.get("fill")).filter(|&c| c > 0.0 && c < 1000.0).unwrap_or(d.3),
    })
}

fn seg_len(a: &P3, b: &P3) -> f64 {
    hypot(b.x - a.x, b.y - a.y)
}
fn pt(f: &Feature, i: usize) -> P3 {
    f.pts.get(i).copied().unwrap_or(P3 { x: f64::NAN, y: f64::NAN, z: f64::NAN })
}

/// `stations(f)`.
pub fn stations(f: &Feature) -> Vec<f64> {
    let mut out = vec![0.0];
    let n = f.pts.len();
    let m = if f.closed { n } else { n.saturating_sub(1) };
    for i in 0..m {
        let prev = out.get(i).copied().unwrap_or(f64::NAN);
        out.push(prev + seg_len(&pt(f, i), &pt(f, (i + 1) % n)));
    }
    out
}
/// A segment `{ i, j, len, dz, grade }`.
#[derive(Clone, Copy, Debug)]
pub struct Seg {
    pub i: usize,
    pub j: usize,
    pub len: f64,
    pub dz: f64,
    pub grade: Option<f64>,
}
impl J for Seg {
    fn j(&self) -> Value {
        crate::jo! { "i" => self.i, "j" => self.j, "len" => self.len, "dz" => self.dz, "grade" => self.grade }
    }
}
/// `segments(f)`.
pub fn segments(f: &Feature) -> Vec<Seg> {
    let n = f.pts.len();
    let m = if f.closed { n } else { n.saturating_sub(1) };
    (0..m)
        .map(|i| {
            let j = (i + 1) % n;
            let len = seg_len(&pt(f, i), &pt(f, j));
            let dz = pt(f, j).z - pt(f, i).z;
            Seg { i, j, len, dz, grade: if len > 1e-12 { Some(dz / len) } else { None } }
        })
        .collect()
}

fn with_pts(f: &Feature, pts: Vec<P3>) -> Feature {
    Feature { pts, ..f.clone() }
}
/// `setElevation(f, i, z)`.
pub fn set_elevation(f: &Feature, i: usize, z: f64) -> Feature {
    if !z.is_finite() || i >= f.pts.len() {
        return f.clone();
    }
    with_pts(f, f.pts.iter().enumerate().map(|(k, p)| if k == i { P3 { z, ..*p } } else { *p }).collect())
}
/// `setGrade(f, i, grade)`.
pub fn set_grade(f: &Feature, i: usize, grade: f64) -> Feature {
    let Some(s) = segments(f).get(i).copied() else { return f.clone() };
    if !grade.is_finite() || !(s.len > 1e-12) {
        return f.clone();
    }
    set_elevation(f, s.j, pt(f, i).z + grade * s.len)
}
/// `raise(f, dz)`.
pub fn raise(f: &Feature, dz: f64) -> Feature {
    if dz.is_finite() { with_pts(f, f.pts.iter().map(|p| P3 { z: p.z + dz, ..*p }).collect()) } else { f.clone() }
}
/// `evenGrade(f, i, j)`.
pub fn even_grade(f: &Feature, i: f64, j: f64) -> Feature {
    let n = f.pts.len() as f64;
    if !(i >= 0.0 && j < n && j > i + 1.0) {
        return f.clone();
    }
    let (Some(iu), Some(ju)) = (crate::js::usize_of(i), crate::js::usize_of(j)) else { return f.clone() };
    if i.fract() != 0.0 || j.fract() != 0.0 {
        return f.clone();
    }
    let st = stations(f);
    let s = |k: usize| st.get(k).copied().unwrap_or(f64::NAN);
    let l = s(ju) - s(iu);
    if !(l > 1e-12) {
        return f.clone();
    }
    let (z0, z1) = (pt(f, iu).z, pt(f, ju).z);
    with_pts(
        f,
        f.pts
            .iter()
            .enumerate()
            .map(|(k, p)| if k > iu && k < ju { P3 { z: z0 + ((z1 - z0) * (s(k) - s(iu))) / l, ..*p } } else { *p })
            .collect(),
    )
}
/// `fromSurface(f, surface, which)` → (f, missing).
pub fn from_surface(f: &Feature, surface: Option<&crate::tin::Surface>, which: Option<&[usize]>) -> (Feature, Vec<usize>) {
    let mut missing = Vec::new();
    let pts = f
        .pts
        .iter()
        .enumerate()
        .map(|(k, p)| {
            if which.is_some_and(|w| !w.contains(&k)) {
                return *p;
            }
            match surface.and_then(|s| s.elevation_at(p.x, p.y, 1e-6)) {
                None => {
                    missing.push(k);
                    *p
                }
                Some(z) => P3 { z, ..*p },
            }
        })
        .collect();
    (with_pts(f, pts), missing)
}

/// `offsetFeature(f, dist, { dz, slope })` → (f | None, problem).
pub fn offset_feature(f: &Feature, dist: f64, dz: Option<f64>, slope: Option<f64>) -> (Option<Feature>, Option<String>) {
    if !dist.is_finite() || dist == 0.0 {
        return (None, Some(tr!("fl.offset.zero")));
    }
    let raise_by = match (dz.filter(|v| v.is_finite()), slope.filter(|v| v.is_finite())) {
        (Some(d), _) => d,
        (None, Some(s)) => dist.abs() * s,
        _ => 0.0,
    };
    let p = &f.pts;
    let n = p.len();
    let norm = |a: &P3, b: &P3| {
        let l = seg_len(a, b);
        if l > 1e-12 { Some(((b.y - a.y) / l, -(b.x - a.x) / l)) } else { None }
    };
    let m = if f.closed { n } else { n.saturating_sub(1) };
    let seg_n: Vec<Option<(f64, f64)>> = (0..m).map(|i| norm(&pt(f, i), &pt(f, (i + 1) % n))).collect();
    if seg_n.iter().any(Option::is_none) {
        return (None, Some(tr!("fl.offset.repeat")));
    }
    let sn = |i: i64| -> Option<(f64, f64)> { usize::try_from(i).ok().and_then(|i| seg_n.get(i)).copied().flatten() };
    let mut out = Vec::with_capacity(n);
    let mi = i64::try_from(m).unwrap_or(0);
    for i in 0..n {
        let ii = i64::try_from(i).unwrap_or(0);
        let (n_in, n_out) = if f.closed { (sn((ii - 1 + mi).rem_euclid(mi.max(1))), sn(ii % mi.max(1))) } else { (sn(ii - 1), sn(ii)) };
        let (ox, oy) = match (n_in, n_out) {
            (None, Some(no)) => (no.0 * dist, no.1 * dist),
            (Some(ni), None) => (ni.0 * dist, ni.1 * dist),
            (Some(ni), Some(no)) => {
                let bx = ni.0 + no.0;
                let by = ni.1 + no.1;
                let c = (bx * no.0 + by * no.1) / hypot(bx, by);
                if hypot(bx, by) < 1e-9 || c < 0.1 {
                    return (None, Some(tr!("fl.offset.sharp"))); // a hairpin: the mitre would run away
                }
                let l = hypot(bx, by);
                ((bx / l) * (dist / c), (by / l) * (dist / c))
            }
            // Spec gap: a one-point line has no segments; the JS reads nOut.x of undefined (TypeError)
            (None, None) => return (None, Some(tr!("fl.offset.repeat"))),
        };
        let q = pt(f, i);
        out.push(P3 { x: q.x + ox, y: q.y + oy, z: q.z + raise_by });
    }
    // the offset must keep every segment's direction
    for i in 0..m {
        let (a, b) = (pt(f, i), pt(f, (i + 1) % n));
        let (c, d) = (out.get(i).copied().unwrap_or_default(), out.get((i + 1) % n).copied().unwrap_or_default());
        if (b.x - a.x) * (d.x - c.x) + (b.y - a.y) * (d.y - c.y) <= 0.0 {
            return (None, Some(tr!("fl.offset.fold")));
        }
    }
    (Some(Feature { id: None, pts: out, ..f.clone() }), None)
}

/// `featureBreaklines(features, use)` → [{ name, pts, feature }].
pub fn feature_breaklines(features: &[Feature], use_: &str) -> Vec<(Breakline, Option<String>)> {
    features
        .iter()
        .filter(|f| f.use_ == use_ && f.pts.len() >= 2)
        .map(|f| {
            let mut pts = f.pts.clone();
            if f.closed
                && let Some(p0) = f.pts.first()
            {
                pts.push(*p0);
            }
            (Breakline { name: f.name.clone(), pts, from_group: false }, f.id.clone())
        })
        .collect()
}

/// A closed design feature line as a grading item.
pub enum FeatureItem {
    /// with daylight: a sloped pad (z = the vertices' mean)
    Pad(Pad),
    /// without: the outline itself is the region and its edge the breakline
    Outline(Vec<P3>),
}
/// `featureItem(f)` → (id, name, item) | None.
pub fn feature_item(f: &Feature) -> Option<FeatureItem> {
    if f.use_ != "design" || !f.closed || f.pts.len() < 3 {
        return None;
    }
    let pts = f.pts.clone();
    let z = pts.iter().fold(0.0, |s, p| s + p.z) / pts.len() as f64;
    if f.daylight {
        return Some(FeatureItem::Pad(Pad {
            pts: pts.iter().map(|p| PZ { x: p.x, y: p.y, z: Some(p.z) }).collect(),
            z,
            cut: f.cut,
            fill: f.fill,
            name: f.name.clone(),
            ..Pad::default()
        }));
    }
    Some(FeatureItem::Outline(pts))
}
/// The patch of an outline item.
pub fn outline_patch(name: &str, pts: &[P3]) -> Patch {
    let mut bl = pts.to_vec();
    if let Some(p0) = pts.first() {
        bl.push(*p0);
    }
    Patch {
        problems: Vec::new(),
        region: Some(pts.to_vec()),
        points: Vec::new(),
        breaklines: vec![Breakline { name: name.to_string(), pts: bl, from_group: false }],
        info: crate::jo! { "poly" => pts.to_vec() },
    }
}

/// `gradeLabel(g)`.
pub fn grade_label(g: f64) -> String {
    if g.is_finite() { format!("{}{}%", if g >= 0.0 { "+" } else { "" }, to_fixed(g * 100.0, 2)) } else { "—".into() }
}

const TAU: f64 = PI * 2.0;
fn cross_element(e: &Element, a: &P3, b: &P3) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    match e {
        Element::Line { p0, p1, sta, len, .. } => {
            let ex = p1.x - p0.x;
            let ey = p1.y - p0.y;
            let den = dx * ey - dy * ex;
            if !(den.abs() >= 1e-15) {
                return out;
            }
            let wx = p0.x - a.x;
            let wy = p0.y - a.y;
            let t = (wx * ey - wy * ex) / den;
            let u = (wx * dy - wy * dx) / den;
            if (-1e-12..=1.0 + 1e-12).contains(&t) && (-1e-12..=1.0 + 1e-12).contains(&u) {
                out.push((min(1.0, max(0.0, t)), sta + min(1.0, max(0.0, u)) * len));
            }
            out
        }
        Element::Arc(e) => {
            let fx = a.x - e.c.x;
            let fy = a.y - e.c.y;
            let aa = dx * dx + dy * dy;
            let bb = 2.0 * (fx * dx + fy * dy);
            let cc = fx * fx + fy * fy - e.r * e.r;
            let disc = bb * bb - 4.0 * aa * cc;
            if aa < 1e-24 || disc < 0.0 {
                return out;
            }
            let sq = disc.sqrt();
            let ts: Vec<f64> = if disc == 0.0 { vec![-bb / (2.0 * aa)] } else { vec![(-bb - sq) / (2.0 * aa), (-bb + sq) / (2.0 * aa)] };
            for t in ts {
                if t < -1e-12 || t > 1.0 + 1e-12 {
                    continue;
                }
                let x = a.x + t * dx;
                let y = a.y + t * dy;
                let mut sw = (((y - e.c.y).jatan2(x - e.c.x) - e.a0) * if e.ccw { 1.0 } else { -1.0 }) % TAU;
                if sw < 0.0 {
                    sw += TAU;
                }
                if sw > e.delta + 1e-12 {
                    continue;
                }
                out.push((min(1.0, max(0.0, t)), e.sta + min(sw, e.delta) * e.r));
            }
            out
        }
    }
}

/// A crossing `{ sta, z, i, grade }`.
#[derive(Clone, Copy, Debug)]
pub struct Crossing {
    pub sta: f64,
    pub z: f64,
    pub i: usize,
    pub grade: Option<f64>,
}
impl J for Crossing {
    fn j(&self) -> Value {
        crate::jo! { "sta" => self.sta, "z" => self.z, "i" => self.i, "grade" => self.grade }
    }
}
/// `alignmentCrossings(al, f)`.
pub fn alignment_crossings(al: &Alignment, f: &Feature) -> Vec<Crossing> {
    if al.elements.is_empty() || f.pts.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for sg in segments(f) {
        if !(sg.len > 1e-12) {
            continue;
        }
        let (a, b) = (pt(f, sg.i), pt(f, sg.j));
        for e in &al.elements {
            for (t, sta) in cross_element(e, &a, &b) {
                out.push(Crossing { sta, z: a.z + t * sg.dz, i: sg.i, grade: sg.grade });
            }
        }
    }
    crate::js::sort_by(&mut out, |p, q| p.sta - q.sta);
    let mut kept: Vec<Crossing> = Vec::new();
    for (k, c) in out.iter().enumerate() {
        let keep = k == 0 || out.get(k - 1).is_some_and(|p| (c.sta - p.sta).abs() > 1e-6 || (c.z - p.z).abs() > 1e-6);
        if keep {
            kept.push(*c);
        }
    }
    kept
}

/// A profile point of a run `{ sta, z, off }`.
#[derive(Clone, Copy, Debug)]
pub struct RunPt {
    pub sta: f64,
    pub z: f64,
    pub off: f64,
}
impl J for RunPt {
    fn j(&self) -> Value {
        crate::jo! { "sta" => self.sta, "z" => self.z, "off" => self.off }
    }
}
/// `projectedRuns(al, f, within)`.
pub fn projected_runs(al: &Alignment, f: &Feature, within: f64) -> Vec<Vec<RunPt>> {
    if !(within > 0.0) || al.elements.is_empty() || f.pts.is_empty() {
        return Vec::new();
    }
    let n = f.pts.len();
    let mut idx: Vec<usize> = (0..n).collect();
    if f.closed {
        idx.push(0);
    }
    let cross = alignment_crossings(al, f);
    let mut runs: Vec<Vec<RunPt>> = Vec::new();
    let mut run: Vec<RunPt> = Vec::new();
    let flush = |run: &mut Vec<RunPt>, runs: &mut Vec<Vec<RunPt>>| {
        if run.len() >= 2 && max_of(run.iter().map(|q| q.sta)) - min_of(run.iter().map(|q| q.sta)) > 1e-6 {
            runs.push(std::mem::take(run));
        } else {
            run.clear();
        }
    };
    for k in 0..idx.len() {
        let ik = idx.get(k).copied().unwrap_or(0);
        let p = pt(f, ik);
        let so = al.station_offset(p.x, p.y);
        let inside = so.is_some_and(|so| !so.outside && so.off.abs() <= within + 1e-9);
        let Some(so) = so.filter(|_| inside) else {
            flush(&mut run, &mut runs);
            continue;
        };
        if k > 0 && !run.is_empty() {
            let prev = idx.get(k - 1).copied().unwrap_or(usize::MAX);
            for c in &cross {
                if c.i == prev {
                    run.push(RunPt { sta: c.sta, z: c.z, off: 0.0 });
                }
            }
        }
        run.push(RunPt { sta: so.sta, z: p.z, off: so.off });
    }
    flush(&mut run, &mut runs);
    runs.into_iter()
        .map(|mut r| {
            if r.last().map_or(f64::NAN, |q| q.sta) < r.first().map_or(f64::NAN, |q| q.sta) {
                r.reverse();
            }
            r
        })
        .collect()
}
/// `featureProfile(al, features, { within })` → [{ id, name, use, crossings, runs }].
pub fn feature_profile(al: &Alignment, features: &[Feature], within: f64) -> Value {
    let mut out = Vec::new();
    for f in features {
        let crossings = alignment_crossings(al, f);
        let runs = projected_runs(al, f, within);
        if !crossings.is_empty() || !runs.is_empty() {
            out.push(crate::jo! { "id" => f.id, "name" => f.name, "use" => f.use_, "crossings" => crossings, "runs" => runs });
        }
    }
    Value::Array(out)
}

/// `gradeOverlay(features, toScreen, { minSeg, minGap, skip })`.
pub fn grade_overlay(
    features: &[Feature],
    to_screen: &dyn Fn(f64, f64) -> (f64, f64),
    min_seg: f64,
    min_gap: f64,
    skip: Option<&str>,
) -> Vec<Value> {
    let mut out = Vec::new();
    let mut placed: Vec<(f64, f64)> = Vec::new();
    for f in features {
        if f.pts.is_empty() || (f.id.as_deref() == skip && (skip.is_some() || f.id.is_none())) {
            continue;
        }
        let s: Vec<(f64, f64)> = f.pts.iter().map(|p| to_screen(p.x, p.y)).collect();
        let sp = |i: usize| s.get(i).copied().unwrap_or((f64::NAN, f64::NAN));
        for sg in segments(f) {
            let (a, b) = (sp(sg.i), sp(sg.j));
            let len = hypot(b.0 - a.0, b.1 - a.1);
            let Some(grade) = sg.grade else { continue };
            if len < min_seg {
                continue;
            }
            let mut ang = (b.1 - a.1).jatan2(b.0 - a.0);
            let along = ang;
            if ang.abs() > PI / 2.0 {
                ang += if ang > 0.0 { -PI } else { PI };
            }
            let level = grade.abs() < 5e-5;
            let down = if level {
                None
            } else if grade < 0.0 {
                Some(along)
            } else {
                Some(along + PI)
            };
            out.push(
                Obj::new()
                    .set("kind", "grade")
                    .set("x", (a.0 + b.0) / 2.0)
                    .set("y", (a.1 + b.1) / 2.0)
                    .set("ang", ang)
                    .set("text", format!("{}%", to_fixed(grade.abs() * 100.0, 2)))
                    .set("down", down)
                    .done(),
            );
        }
        for (i, p) in f.pts.iter().enumerate() {
            let q = sp(i);
            if placed.iter().any(|o| hypot(o.0 - q.0, o.1 - q.1) < min_gap) {
                continue;
            }
            placed.push(q);
            out.push(crate::jo! { "kind" => "elev", "x" => q.0, "y" => q.1, "text" => to_fixed(round(p.z * 100.0) / 100.0, 2) });
        }
    }
    out
}
