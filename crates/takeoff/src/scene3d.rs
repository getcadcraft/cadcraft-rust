//! `scene3d.js` — the 3D check view's geometry, built from one sheet of the takeoff (feet, Z up): walls (strips or
//! mitred solids), flat runs, slabs and plates (earcut, cut-outs), count posts. The numbers each object reports are
//! the exact ones of `measureMark`, so they agree with the quantities.
// Integer arithmetic here is vertex and point indices within the lists built here; the readers cap list sizes: it cannot overflow, so the crate-wide
// `arithmetic_side_effects` deny is relaxed for this module only.
#![allow(clippy::arithmetic_side_effects)]

use std::collections::HashSet;

use serde_json::{Value, json};

use crate::geom::{P, flatten_step, spans};
use crate::i18n::{F, t, tf};
use crate::jsmath::{cos, sin};
use crate::jv::{self, Obj, n};
use crate::model::{self, Backouts, Measure, measure_mark};
use cc_m3d::js::{hypot2, max, min};
use std::f64::consts::PI;

pub const THIN_FT: f64 = 2.0 / 12.0;
pub const STEP: f64 = PI / 72.0;

type V2 = [f64; 2];

/// The page's viewport: transform (user → display), display width and height.
#[derive(Debug, Clone, Default)]
pub struct View {
    pub transform: Option<[f64; 6]>,
    pub width: f64,
    pub height: f64,
    pub step: Option<f64>,
}

/// The mapping from PDF user space to the ground, in feet.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub k: f64,
    pub w: f64,
    pub h: f64,
    t: [f64; 6],
    height: f64,
}
impl Frame {
    pub fn at(&self, p: &P) -> V2 {
        let tt = &self.t;
        let dx = tt[0] * p.x + tt[2] * p.y + tt[4];
        let dy = tt[1] * p.x + tt[3] * p.y + tt[5];
        [dx * self.k, (self.height - dy) * self.k]
    }
}
pub fn sheet_frame(ft_per_unit: f64, v: &View) -> Frame {
    let tt = v.transform.unwrap_or([1.0, 0.0, 0.0, -1.0, 0.0, v.height]);
    let s0 = (tt[0] * tt[3] - tt[1] * tt[2]).abs().sqrt();
    let s = if s0 == 0.0 || s0.is_nan() { 1.0 } else { s0 };
    let k = ft_per_unit / s;
    Frame { k, w: v.width * k, h: v.height * k, t: tt, height: v.height }
}

fn ring_area(r: &[V2]) -> f64 {
    let nn = r.len();
    let mut a = 0.0;
    for i in 0..nn {
        let (Some(p), Some(q)) = (r.get(i), r.get((i + 1) % nn)) else { continue };
        a += p[0] * q[1] - q[0] * p[1];
    }
    a / 2.0
}
fn clean(r: Vec<V2>, closed: bool) -> Vec<V2> {
    let mut out: Vec<V2> = Vec::with_capacity(r.len());
    for p in r {
        let keep = match out.last() {
            None => true,
            Some(q) => hypot2(p[0] - q[0], p[1] - q[1]) > 1e-9,
        };
        if keep {
            out.push(p);
        }
    }
    if closed
        && out.len() > 1
        && let (Some(a), Some(b)) = (out.first(), out.last())
        && hypot2(a[0] - b[0], a[1] - b[1]) <= 1e-9
    {
        out.pop();
    }
    out
}
fn quad(t: &mut Vec<f64>, a: &V2, b: &V2, z0: f64, z1: f64) {
    t.extend_from_slice(&[a[0], a[1], z0, b[0], b[1], z0, b[0], b[1], z1]);
    t.extend_from_slice(&[a[0], a[1], z0, b[0], b[1], z1, a[0], a[1], z1]);
}
fn seg(l: &mut Vec<f64>, a: &V2, za: f64, b: &V2, zb: f64) {
    l.extend_from_slice(&[a[0], a[1], za, b[0], b[1], zb]);
}

/// A mesh: triangles (9 numbers each), lines (6 each), and whether earcut filled it.
#[derive(Debug, Clone, Default)]
pub struct Mesh {
    pub tris: Vec<f64>,
    pub lines: Vec<f64>,
    pub triangulated: bool,
}

fn wall_mesh(run: &[V2], h: f64, corners: &[V2], z0: f64) -> Mesh {
    let mut m = Mesh::default();
    for w in run.windows(2) {
        if let [a, b] = w {
            quad(&mut m.tris, a, b, z0, z0 + h);
            seg(&mut m.lines, a, z0, b, z0);
            seg(&mut m.lines, a, z0 + h, b, z0 + h);
        }
    }
    for c in corners {
        seg(&mut m.lines, c, z0, c, z0 + h);
    }
    m
}

/// A wall with a thickness: the run offset half the thickness to each side (mitred), closed into one outline.
pub fn wall_outline(run: &[V2], thick: f64) -> Vec<V2> {
    let half = thick / 2.0;
    let nn = run.len();
    let dir = |a: &V2, b: &V2| -> V2 {
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let l0 = hypot2(dx, dy);
        let l = if l0 == 0.0 || l0.is_nan() { 1.0 } else { l0 };
        [dx / l, dy / l]
    };
    let g = |i: usize| run.get(i).copied().unwrap_or([f64::NAN, f64::NAN]);
    let mut left = Vec::with_capacity(nn);
    let mut right = Vec::with_capacity(nn);
    for i in 0..nn {
        let d0 = if i > 0 { dir(&g(i - 1), &g(i)) } else { dir(&g(0), &g(1)) };
        let d1 = if i + 1 < nn { dir(&g(i), &g(i + 1)) } else { d0 };
        let (mut mx, mut my) = (-(d0[1] + d1[1]), d0[0] + d1[0]);
        let ml = hypot2(mx, my);
        if ml < 1e-9 {
            mx = -d0[1];
            my = d0[0];
        } else {
            mx /= ml;
            my /= ml;
        }
        let c = mx * -d0[1] + my * d0[0];
        let k = min(half / max(c, 1e-6), half * 4.0);
        let p = g(i);
        left.push([p[0] + mx * k, p[1] + my * k]);
        right.push([p[0] - mx * k, p[1] - my * k]);
    }
    left.reverse();
    let mut ring = right;
    ring.extend(left);
    if ring_area(&ring) < 0.0 {
        ring.reverse();
    }
    ring
}
fn solid_wall(run: &[V2], thick: f64, z0: f64, z1: f64) -> Mesh {
    let ring = clean(wall_outline(run, thick), true);
    prism_mesh(&ring, &[], z0, z1)
}

/// A prism: outline (CCW) minus holes (CW), from z0 to z1, closed and facing out.
fn prism_mesh(outer: &[V2], holes: &[Vec<V2>], z0: f64, z1: f64) -> Mesh {
    let mut m = Mesh::default();
    let mut flat = Vec::new();
    let mut hole_idx = Vec::new();
    let mut all: Vec<V2> = Vec::new();
    for p in outer {
        flat.extend_from_slice(p);
        all.push(*p);
    }
    for h in holes {
        hole_idx.push(all.len());
        for p in h {
            flat.extend_from_slice(p);
            all.push(*p);
        }
    }
    let idx = cc_m3d::earcut::earcut(&flat, &hole_idx);
    for tri in idx.chunks(3) {
        let [ia, ib, ic] = tri else { continue };
        let (Some(a), Some(mut b), Some(mut c)) = (all.get(*ia).copied(), all.get(*ib).copied(), all.get(*ic).copied()) else { continue };
        if (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) < 0.0 {
            std::mem::swap(&mut b, &mut c);
        }
        m.tris.extend_from_slice(&[a[0], a[1], z1, b[0], b[1], z1, c[0], c[1], z1]);
        m.tris.extend_from_slice(&[a[0], a[1], z0, c[0], c[1], z0, b[0], b[1], z0]);
    }
    for r in std::iter::once(outer).chain(holes.iter().map(Vec::as_slice)) {
        for i in 0..r.len() {
            let (Some(a), Some(b)) = (r.get(i), r.get((i + 1) % r.len())) else { continue };
            quad(&mut m.tris, a, b, z0, z1);
            seg(&mut m.lines, a, z0, b, z0);
            seg(&mut m.lines, a, z1, b, z1);
        }
    }
    m.triangulated = !idx.is_empty();
    m
}

fn marker_mesh(p: &V2, r: f64, h: f64) -> Mesh {
    let ring: Vec<V2> = (0..8)
        .map(|i| {
            let tt = (f64::from(i) + 0.5) * PI / 4.0;
            [p[0] + r * cos(tt), p[1] + r * sin(tt)]
        })
        .collect();
    let mut m = prism_mesh(&ring, &[], 0.0, h);
    seg(&mut m.lines, p, 0.0, p, h);
    m
}

/// One object of the scene.
#[derive(Debug, Clone)]
pub struct Obj3 {
    pub id: Value,
    pub item: Value,
    pub name: Value,
    pub kind: &'static str,
    pub color: Value,
    pub tris: Vec<f64>,
    pub lines: Vec<f64>,
    pub anchor: Option<[f64; 3]>,
    pub measure: Value,
    pub level: Option<String>,
}
impl Obj3 {
    pub fn to_json(&self) -> Value {
        let mut o = Obj::new();
        o.insert("id".into(), self.id.clone());
        o.insert("item".into(), self.item.clone());
        o.insert("name".into(), self.name.clone());
        if self.kind == "frame" {
            o.insert("kind".into(), Value::String(self.kind.into()));
            o.insert("color".into(), self.color.clone());
        } else {
            o.insert("color".into(), self.color.clone());
            o.insert("kind".into(), Value::String(self.kind.into()));
        }
        o.insert("tris".into(), Value::Array(self.tris.iter().map(|v| n(*v)).collect()));
        o.insert("lines".into(), Value::Array(self.lines.iter().map(|v| n(*v)).collect()));
        o.insert("anchor".into(), self.anchor.map_or(Value::Null, |a| json!([n(a[0]), n(a[1]), n(a[2])])));
        o.insert("measure".into(), self.measure.clone());
        if let Some(l) = &self.level {
            o.insert("level".into(), Value::String(l.clone()));
        }
        Value::Object(o)
    }
}

/// `totals` of a scene.
#[derive(Debug, Clone, Default)]
pub struct Totals {
    pub length_ft: f64,
    pub wall_sqft: f64,
    pub net_sqft: f64,
    pub volume_cuft: f64,
    pub count: f64,
    pub walls: f64,
    pub runs: f64,
    pub slabs: f64,
    pub plates: f64,
    pub markers: f64,
}
impl Totals {
    fn add(&mut self, o: &Totals) {
        self.length_ft += o.length_ft;
        self.wall_sqft += o.wall_sqft;
        self.net_sqft += o.net_sqft;
        self.volume_cuft += o.volume_cuft;
        self.count += o.count;
        self.walls += o.walls;
        self.runs += o.runs;
        self.slabs += o.slabs;
        self.plates += o.plates;
        self.markers += o.markers;
    }
    pub fn to_json(&self) -> Value {
        json!({
            "lengthFt": n(self.length_ft), "wallSqft": n(self.wall_sqft), "netSqft": n(self.net_sqft), "volumeCuft": n(self.volume_cuft),
            "count": n(self.count), "walls": n(self.walls), "runs": n(self.runs), "slabs": n(self.slabs), "plates": n(self.plates), "markers": n(self.markers),
        })
    }
}
/// Per item on the sheet.
#[derive(Debug, Clone, Default)]
pub struct ByItem {
    pub length_ft: f64,
    pub wall_sqft: f64,
    pub net_sqft: f64,
    pub volume_cuft: f64,
    pub count: f64,
    pub marks: f64,
}
impl ByItem {
    fn to_json(&self) -> Value {
        json!({ "lengthFt": n(self.length_ft), "wallSqft": n(self.wall_sqft), "netSqft": n(self.net_sqft), "volumeCuft": n(self.volume_cuft), "count": n(self.count), "marks": n(self.marks) })
    }
}
/// A box.
#[derive(Debug, Clone, Copy)]
pub struct Bx {
    pub min: [f64; 3],
    pub max: [f64; 3],
}
fn bx_json(b: &Bx) -> Value {
    json!({ "min": [n(b.min[0]), n(b.min[1]), n(b.min[2])], "max": [n(b.max[0]), n(b.max[1]), n(b.max[2])] })
}

/// A built scene.
#[derive(Debug, Clone)]
pub struct Scene {
    pub sheet: String,
    pub ft_per_unit: f64,
    pub ground: (f64, f64),
    pub objects: Vec<Obj3>,
    pub totals: Totals,
    pub by_item: Vec<(String, ByItem)>,
    pub bbox: Bx,
    pub marks_box: Option<Bx>,
    pub notes: Vec<String>,
    /// stacked scenes: the ground's elevation and the levels used
    pub ground_z: Option<f64>,
    pub levels: Option<Vec<(String, f64, f64, f64)>>,
}
impl Scene {
    pub fn to_json(&self) -> Value {
        let mut o = Obj::new();
        o.insert("ok".into(), Value::Bool(true));
        o.insert("sheet".into(), Value::String(self.sheet.clone()));
        o.insert("ftPerUnit".into(), n(self.ft_per_unit));
        o.insert("ground".into(), json!({ "w": n(self.ground.0), "h": n(self.ground.1) }));
        if let Some(z) = self.ground_z {
            o.insert("groundZ".into(), n(z));
        }
        o.insert("objects".into(), Value::Array(self.objects.iter().map(Obj3::to_json).collect()));
        o.insert("totals".into(), self.totals.to_json());
        let mut bi = Obj::new();
        for (k, v) in &self.by_item {
            bi.insert(k.clone(), v.to_json());
        }
        o.insert("byItem".into(), Value::Object(bi));
        o.insert("bbox".into(), bx_json(&self.bbox));
        o.insert("marksBox".into(), self.marks_box.as_ref().map_or(Value::Null, bx_json));
        o.insert("notes".into(), Value::Array(self.notes.iter().map(|x| Value::String(x.clone())).collect()));
        if let Some(l) = &self.levels {
            o.insert(
                "levels".into(),
                Value::Array(l.iter().map(|(k, z, w, h)| json!({ "key": k, "elevFt": n(*z), "w": n(*w), "h": n(*h) })).collect()),
            );
            o.insert("stacked".into(), Value::Bool(true));
        }
        Value::Object(o)
    }
}

fn grow(bbox: &mut Bx, marks_box: &mut Option<Bx>, tris: &[f64]) {
    let mb = marks_box.get_or_insert(Bx { min: [f64::INFINITY; 3], max: [f64::NEG_INFINITY; 3] });
    for c in tris.chunks(3) {
        for (a, v) in c.iter().enumerate() {
            let v = *v;
            if let (Some(lo), Some(hi), Some(mlo), Some(mhi)) =
                (bbox.min.get_mut(a), bbox.max.get_mut(a), mb.min.get_mut(a), mb.max.get_mut(a))
            {
                if v < *lo {
                    *lo = v;
                }
                if v > *hi {
                    *hi = v;
                }
                if v < *mlo {
                    *mlo = v;
                }
                if v > *mhi {
                    *mhi = v;
                }
            }
        }
    }
}

/// Build the sheet's 3D check geometry. Err = the reason (no sheet / no scale).
pub fn build_scene(state: &Value, sheet: &str, opts: &View) -> Result<Scene, String> {
    if sheet.is_empty() {
        return Err(t("d3.err.nosheet"));
    }
    let sc = state.get("sheets").and_then(|s| s.get(sheet)).and_then(|s| s.get("scale")).filter(|s| jv::truthy(Some(s)));
    let fpu = sc.map_or(f64::NAN, |s| jv::to_number(s.get("ftPerUnit")));
    if !(fpu > 0.0) {
        return Err(t("d3.err.noscale"));
    }
    let step = opts.step.filter(|s| *s != 0.0 && !s.is_nan()).unwrap_or(STEP);
    let fr = sheet_frame(fpu, opts);
    let items = jv::arr(state.get("items")).map_or(&[][..], Vec::as_slice);
    let item_of = |id: &Value| items.iter().rev().find(|i| jv::strict_eq(i.get("id"), Some(id)));
    let d0 = hypot2(fr.w, fr.h);
    let diag = if d0 == 0.0 || d0.is_nan() { 100.0 } else { d0 };
    let post_h = min(150.0, max(2.5, diag * 0.04));
    let post_r = post_h * 0.18;
    let mut objects: Vec<Obj3> = Vec::new();
    let mut totals = Totals::default();
    let mut by_item: Vec<(String, ByItem)> = Vec::new();
    let mut notes = Vec::new();
    let mut bbox = Bx { min: [0.0, 0.0, 0.0], max: [fr.w, fr.h, 0.0] };
    let mut marks_box: Option<Bx> = None;
    let mut untriangulated = 0usize;
    let bo = Backouts::new(state);
    for m in jv::arr(state.get("marks")).map_or(&[][..], Vec::as_slice) {
        if jv::str(m.get("sheet")) != Some(sheet) {
            continue;
        }
        let Some(item) = m.get("item").and_then(item_of) else { continue };
        if jv::arr(m.get("pts")).is_none_or(Vec::is_empty) {
            continue;
        }
        let kind = jv::to_string(m.get("kind"));
        let synthetic;
        let it: &Value = if jv::str(item.get("type")) == Some(kind.as_str()) {
            item
        } else {
            synthetic = json!({ "id": item.get("id"), "name": item.get("name"), "color": item.get("color"), "type": kind, "heightFt": 0 });
            &synthetic
        };
        let Some(r) = measure_mark(m, fpu, Some(it)) else { continue };
        let it_id = jv::to_string(it.get("id"));
        let ti = match by_item.iter().position(|(k, _)| *k == it_id) {
            Some(i) => i,
            None => {
                by_item.push((it_id.clone(), ByItem::default()));
                by_item.len() - 1
            }
        };
        let Some((_, tb)) = by_item.get_mut(ti) else { continue };
        tb.marks += 1.0;
        let base = |kind: &'static str, tris: Vec<f64>, lines: Vec<f64>, anchor: Option<[f64; 3]>, measure: Value| Obj3 {
            id: m.get("id").cloned().unwrap_or(Value::Null),
            item: it.get("id").cloned().unwrap_or(Value::Null),
            name: it.get("name").cloned().unwrap_or(Value::Null),
            kind,
            color: it.get("color").cloned().unwrap_or(Value::Null),
            tris,
            lines,
            anchor,
            measure,
            level: None,
        };
        let pts = model::pts_of(m.get("pts"));
        let gnum = |k: &str| jv::num(it.get(k)).unwrap_or(f64::NAN);
        let o = match (kind.as_str(), &r) {
            ("linear", Measure::Linear { length_ft, .. }) => {
                let run = clean(flatten_step(&pts, false, step).iter().map(|p| fr.at(p)).collect(), false);
                let h = if gnum("heightFt") > 0.0 { gnum("heightFt") } else { 0.0 };
                totals.length_ft += length_ft;
                tb.length_ft += length_ft;
                tb.count += 1.0;
                if h > 0.0 && run.len() > 1 {
                    let z0 = if gnum("baseFt") > 0.0 { gnum("baseFt") } else { 0.0 };
                    let thick = if gnum("thickFt") > 0.0 { gnum("thickFt") } else { 0.0 };
                    let head = if gnum("headFt") > 0.0 && gnum("headFt") < h { gnum("headFt") } else { 0.0 };
                    let mut tris = Vec::new();
                    let mut lines = Vec::new();
                    let mut anchor: Option<[f64; 3]> = None;
                    let ded: HashSet<i64> = jv::arr(m.get("deducts"))
                        .map_or(&[][..], Vec::as_slice)
                        .iter()
                        .filter_map(|v| jv::num(Some(v)))
                        .map(|v| if v.trunc() == v && v.abs() < 1e15 { js_int(v) } else { i64::MIN })
                        .collect();
                    let is_ded = |i: usize| ded.contains(&i64::try_from(i).unwrap_or(i64::MIN));
                    for sp in spans(&pts, &is_ded) {
                        let part = clean(flatten_step(&sp.pts, false, step).iter().map(|p| fr.at(p)).collect(), false);
                        if part.len() < 2 {
                            continue;
                        }
                        let mut add = |mesh: Mesh| {
                            tris.extend(mesh.tris);
                            lines.extend(mesh.lines);
                        };
                        if sp.deduct {
                            let mut l = Vec::new();
                            for w in part.windows(2) {
                                if let [a, b] = w {
                                    seg(&mut l, a, z0, b, z0);
                                }
                            }
                            add(Mesh { tris: Vec::new(), lines: l, triangulated: true });
                            if head != 0.0 {
                                add(if thick != 0.0 {
                                    solid_wall(&part, thick, z0 + head, z0 + h)
                                } else {
                                    wall_mesh(&part, h - head, &[], z0 + head)
                                });
                            }
                            continue;
                        }
                        let corners: Vec<V2> = sp.pts.iter().map(|p| fr.at(p)).collect();
                        add(if thick != 0.0 { solid_wall(&part, thick, z0, z0 + h) } else { wall_mesh(&part, h, &corners, z0) });
                        if anchor.is_none()
                            && let (Some(a), Some(b)) = (part.first(), part.get(1))
                        {
                            anchor = Some([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, z0 + h / 2.0]);
                        }
                    }
                    let wall_sqft = length_ft * h;
                    totals.wall_sqft += wall_sqft;
                    tb.wall_sqft += wall_sqft;
                    totals.walls += 1.0;
                    base(
                        "wall",
                        tris,
                        lines,
                        anchor,
                        json!({ "lengthFt": n(*length_ft), "heightFt": n(h), "wallSqft": n(wall_sqft), "thickFt": n(thick), "baseFt": n(z0), "headFt": n(head) }),
                    )
                } else {
                    let mut lines = Vec::new();
                    for w in run.windows(2) {
                        if let [a, b] = w {
                            seg(&mut lines, a, 0.0, b, 0.0);
                        }
                    }
                    totals.runs += 1.0;
                    let a = run.first().copied().unwrap_or([f64::NAN, f64::NAN]);
                    let b = run.get(1).copied().unwrap_or(a);
                    base(
                        "run",
                        Vec::new(),
                        lines,
                        Some([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, 0.0]),
                        json!({ "lengthFt": n(*length_ft), "heightFt": 0, "wallSqft": 0 }),
                    )
                }
            }
            ("area", Measure::Area { area_sqft, factor, gross_sqft, cut_sqft, .. }) => {
                let mut outer = clean(flatten_step(&pts, true, step).iter().map(|p| fr.at(p)).collect(), true);
                if ring_area(&outer) < 0.0 {
                    outer.reverse();
                }
                let holes: Vec<Vec<V2>> = model::holes_of(m.get("holes"))
                    .iter()
                    .map(|h| {
                        let mut ring = clean(flatten_step(h, true, step).iter().map(|p| fr.at(p)).collect(), true);
                        if ring_area(&ring) > 0.0 {
                            ring.reverse();
                        }
                        ring
                    })
                    .filter(|h| h.len() > 2)
                    .collect();
                if outer.len() < 3 {
                    continue;
                }
                let hf = gnum("heightFt");
                let thin = !(hf > 0.0);
                let d = if thin { THIN_FT } else { hf };
                let zb = if gnum("baseFt") > 0.0 { gnum("baseFt") } else { 0.0 };
                let mesh = prism_mesh(&outer, &holes, zb, zb + d);
                if !mesh.triangulated {
                    untriangulated += 1;
                }
                let net = max(0.0, area_sqft - bo.cut(m, fpu) * factor);
                let volume = if thin { None } else { Some(net * d) };
                totals.net_sqft += net;
                tb.net_sqft += net;
                tb.count += 1.0;
                match volume {
                    None => totals.plates += 1.0,
                    Some(v) => {
                        totals.slabs += 1.0;
                        totals.volume_cuft += v;
                        tb.volume_cuft += v;
                    }
                }
                let tri = &mesh.tris;
                let top = zb + d;
                let first = outer.first().copied().unwrap_or([f64::NAN, f64::NAN]);
                let mut anchor = [first[0], first[1], top];
                let mut best = -1.0f64;
                let tv = |i: usize| tri.get(i).copied().unwrap_or(f64::NAN);
                let mut i = 0usize;
                while i < tri.len() && tv(i + 2) == top && tv(i + 5) == top && tv(i + 8) == top {
                    let ar = ((tv(i + 3) - tv(i)) * (tv(i + 7) - tv(i + 1)) - (tv(i + 4) - tv(i + 1)) * (tv(i + 6) - tv(i))).abs();
                    if ar > best {
                        best = ar;
                        anchor = [(tv(i) + tv(i + 3) + tv(i + 6)) / 3.0, (tv(i + 1) + tv(i + 4) + tv(i + 7)) / 3.0, top];
                    }
                    i += 18;
                }
                let measure = json!({
                    "netSqft": n(net), "grossSqft": n(*gross_sqft), "cutSqft": n(*cut_sqft), "backoutSqft": n(area_sqft - net),
                    "depthFt": n(if thin { 0.0 } else { d }), "drawnFt": n(d), "volumeCuft": volume.map_or(Value::Null, n), "thin": thin,
                });
                base(if thin { "plate" } else { "slab" }, mesh.tris.clone(), mesh.lines.clone(), Some(anchor), measure)
            }
            ("count", Measure::Count) => {
                let Some(p0) = pts.first() else { continue };
                let p = fr.at(p0);
                let mesh = marker_mesh(&p, post_r, post_h);
                totals.count += 1.0;
                totals.markers += 1.0;
                tb.count += 1.0;
                base("marker", mesh.tris, mesh.lines, Some([p[0], p[1], post_h]), json!({ "count": 1 }))
            }
            _ => continue,
        };
        if o.tris.is_empty() {
            grow(&mut bbox, &mut marks_box, &o.lines);
        } else {
            grow(&mut bbox, &mut marks_box, &o.tris);
        }
        objects.push(o);
    }
    if totals.runs > 0.0 {
        notes.push(tf("d3.note.runs", &[("n", F::N(totals.runs))]));
    }
    if totals.plates > 0.0 {
        notes.push(tf("d3.note.plates", &[("n", F::N(totals.plates))]));
    }
    if untriangulated > 0 {
        notes.push(tf("d3.note.unfilled", &[("n", F::from(untriangulated))]));
    }
    Ok(Scene {
        sheet: sheet.to_string(),
        ft_per_unit: fpu,
        ground: (fr.w, fr.h),
        objects,
        totals,
        by_item,
        bbox,
        marks_box,
        notes,
        ground_z: None,
        levels: None,
    })
}

/// A level of a stack: sheet key, elevation, viewport.
#[derive(Debug, Clone)]
pub struct Level {
    pub key: String,
    pub elev_ft: f64,
    pub view: View,
}

/// Several sheets stacked as floors.
pub fn build_stack(state: &Value, levels: &[Level], step: Option<f64>) -> Result<Scene, String> {
    let mut objects: Vec<Obj3> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut used: Vec<(String, f64, f64, f64)> = Vec::new();
    let mut totals = Totals::default();
    let mut by_item: Vec<(String, ByItem)> = Vec::new();
    let mut bbox: Option<Bx> = None;
    let mut marks_box: Option<Bx> = None;
    let mut first: Option<(f64, f64, (f64, f64))> = None;
    let mut skipped = 0usize;
    let growb = |b: Option<Bx>, mn: [f64; 3], mx: [f64; 3]| -> Bx {
        match b {
            Some(b) => Bx {
                min: [min(b.min[0], mn[0]), min(b.min[1], mn[1]), min(b.min[2], mn[2])],
                max: [max(b.max[0], mx[0]), max(b.max[1], mx[1]), max(b.max[2], mx[2])],
            },
            None => Bx { min: mn, max: mx },
        }
    };
    for lv in levels {
        let mut view = lv.view.clone();
        view.step = step;
        let r = match build_scene(state, &lv.key, &view) {
            Ok(r) => r,
            Err(e) => {
                if first.is_none() {
                    return Err(e);
                }
                skipped += 1;
                continue;
            }
        };
        let z = if lv.elev_ft == 0.0 || lv.elev_ft.is_nan() { 0.0 } else { lv.elev_ft };
        let lift = |a: &[f64]| -> Vec<f64> { a.iter().enumerate().map(|(i, v)| if i % 3 == 2 { v + z } else { *v }).collect() };
        for o in &r.objects {
            let mut no = o.clone();
            no.tris = lift(&o.tris);
            no.lines = lift(&o.lines);
            no.anchor = Some(o.anchor.map_or([f64::NAN, f64::NAN, f64::NAN], |a| [a[0], a[1], a[2] + z]));
            no.level = Some(lv.key.clone());
            objects.push(no);
        }
        if first.is_some() {
            let (w, h) = r.ground;
            objects.push(Obj3 {
                id: Value::Null,
                item: Value::Null,
                name: Value::String(String::new()),
                kind: "frame",
                color: Value::String("#888888".into()),
                tris: Vec::new(),
                lines: vec![0.0, 0.0, z, w, 0.0, z, w, 0.0, z, w, h, z, w, h, z, 0.0, h, z, 0.0, h, z, 0.0, 0.0, z],
                anchor: Some([0.0, 0.0, z]),
                measure: Value::Object(Obj::new()),
                level: Some(lv.key.clone()),
            });
        }
        if first.is_none() {
            first = Some((z, r.ft_per_unit, r.ground));
        }
        totals.add(&r.totals);
        for (id, v) in &r.by_item {
            match by_item.iter_mut().find(|(k, _)| k == id) {
                None => by_item.push((id.clone(), v.clone())),
                Some((_, b)) => {
                    b.length_ft += v.length_ft;
                    b.wall_sqft += v.wall_sqft;
                    b.net_sqft += v.net_sqft;
                    b.volume_cuft += v.volume_cuft;
                    b.count += v.count;
                    b.marks += v.marks;
                }
            }
        }
        bbox = Some(growb(bbox, [r.bbox.min[0], r.bbox.min[1], r.bbox.min[2] + z], [r.bbox.max[0], r.bbox.max[1], r.bbox.max[2] + z]));
        if let Some(mb) = r.marks_box {
            marks_box = Some(growb(marks_box, [mb.min[0], mb.min[1], mb.min[2] + z], [mb.max[0], mb.max[1], mb.max[2] + z]));
        }
        for nt in &r.notes {
            if !notes.contains(nt) {
                notes.push(nt.clone());
            }
        }
        used.push((lv.key.clone(), z, r.ground.0, r.ground.1));
    }
    let Some((gz, fpu, ground)) = first else { return Err(t("d3.err.nosheet")) };
    if skipped > 0 {
        notes.push(tf("d3.note.skipped", &[("n", F::from(skipped))]));
    }
    Ok(Scene {
        sheet: levels.first().map(|l| l.key.clone()).unwrap_or_default(),
        ft_per_unit: fpu,
        ground,
        objects,
        totals,
        by_item,
        bbox: bbox.unwrap_or(Bx { min: [0.0; 3], max: [0.0; 3] }),
        marks_box,
        notes,
        ground_z: Some(gz),
        levels: Some(used),
    })
}

/// An integral f64 under 1e15 as an i64 (exact: the caller checked it).
#[allow(clippy::cast_possible_truncation)]
fn js_int(v: f64) -> i64 {
    v as i64
}

/// Signed volume of a closed triangle mesh — cubic feet.
pub fn mesh_volume(tris: &[f64]) -> f64 {
    let mut v = 0.0;
    for [ax, ay, az, bx, by, bz, cx, cy, cz] in tris.as_chunks::<9>().0 {
        {
            v += ax * (by * cz - bz * cy) - ay * (bx * cz - bz * cx) + az * (bx * cy - by * cx);
        }
    }
    v / 6.0
}
/// Total area of a triangle list — square feet.
pub fn mesh_area(tris: &[f64]) -> f64 {
    let mut s = 0.0;
    for [ax, ay, az, bx, by, bz, cx, cy, cz] in tris.as_chunks::<9>().0 {
        {
            let (ux, uy, uz) = (bx - ax, by - ay, bz - az);
            let (vx, vy, vz) = (cx - ax, cy - ay, cz - az);
            s += cc_m3d::js::hypot3(uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx) / 2.0;
        }
    }
    s
}
