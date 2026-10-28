//! flatview.js — flat 2D views of the 3D solids, placed on a sheet or in the model (0.4 C18). VIEWBASE (FLATSHOT):
//! view (front, top, right, left, back, bottom, iso), hidden lines shown or not, scale (on a sheet the largest
//! standard scale that fits), location → one block reference "VIEW-<View>-<n>": visible edges on VIEW-VISIBLE
//! (0.5 mm), hidden ones on VIEW-HIDDEN (linetype HIDDEN, 0.25 mm). One undo step; a snapshot (change the solids, run
//! it again).
//!
//! Which edges: every hard edge (and loose edges); a soft edge only where it is the outline seen from that side.
//! Hidden-line removal: each edge is sampled against the model's triangles seen from the view (a 96 × 96 grid finds the
//! triangles under a point); where it changes between in front and behind, the change is found by halving to a
//! millionth of the view. Pieces on one line merge; a hidden piece where a visible one is drawn goes.

use std::collections::HashMap;

use cc_cad::entity::Entity;
use cc_cad::js;
use cc_cad_geom::jsmath::JsMath;
use cc_m3d::def::{face_normal, face_tris, with_plain_face_tris};
use cc_m3d::model::Model;
use cc_m3d::vec::apply;
use serde_json::{Value, json};

use crate::engine::{Ctx, Opts, R, Reply};
use crate::input::Def as DefOpt;

type V3 = [f64; 3];

/// A view frame: u (to the right on paper), v (up on paper), w (toward the viewer).
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub u: V3,
    pub v: V3,
    pub w: V3,
}

/// flatview.js `VIEWS[name]` (front for an unknown name).
pub fn frame(view: &str) -> Frame {
    let s2 = std::f64::consts::FRAC_1_SQRT_2;
    let s3 = 1.0 / 3f64.sqrt();
    let s6 = 1.0 / 6f64.sqrt();
    match view {
        "top" => Frame { u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0], w: [0.0, 0.0, 1.0] },
        "bottom" => Frame { u: [1.0, 0.0, 0.0], v: [0.0, -1.0, 0.0], w: [0.0, 0.0, -1.0] },
        "back" => Frame { u: [-1.0, 0.0, 0.0], v: [0.0, 0.0, 1.0], w: [0.0, 1.0, 0.0] },
        "right" => Frame { u: [0.0, 1.0, 0.0], v: [0.0, 0.0, 1.0], w: [1.0, 0.0, 0.0] },
        "left" => Frame { u: [0.0, -1.0, 0.0], v: [0.0, 0.0, 1.0], w: [-1.0, 0.0, 0.0] },
        "iso" => Frame { u: [s2, s2, 0.0], v: [-s6, s6, 2.0 * s6], w: [s3, -s3, s3] },
        _ => Frame { u: [1.0, 0.0, 0.0], v: [0.0, 0.0, 1.0], w: [0.0, -1.0, 0.0] },
    }
}

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The model seen from a view: its edges split into visible and hidden runs (meters, the view's u, v).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Projection {
    pub visible: Vec<[f64; 4]>,
    pub hidden: Vec<[f64; 4]>,
    /// u0, v0, u1, v1; None when the model has no edges
    pub bbox: Option<[f64; 4]>,
}

/// `collect(model, V)`: every edge and triangle of the model in the view's frame (u, v, depth toward the viewer).
fn collect(model: &Model, fr: &Frame) -> (Vec<[V3; 3]>, Vec<[V3; 2]>) {
    let mut tris: Vec<[V3; 3]> = Vec::new();
    let mut edges: Vec<[V3; 2]> = Vec::new();
    let Some(root) = model.root().map(|r| r.id.clone()) else { return (tris, edges) };
    with_plain_face_tris(|| {
        model.walk(&root, &cc_m3d::vec::ident(), &mut |def, m, _path, _color| {
            if def.id == root && def.faces.is_empty() && def.edges.is_empty() {
                return true;
            }
            let at = |vid: f64| -> V3 {
                let p = apply(m, def.v(vid));
                [dot(p, fr.u), dot(p, fr.v), dot(p, fr.w)]
            };
            let mut facing: HashMap<u64, f64> = HashMap::new();
            let mut by_edge: HashMap<(u64, u64), Vec<f64>> = HashMap::new();
            let key = |a: f64, b: f64| if a < b { (a.to_bits(), b.to_bits()) } else { (b.to_bits(), a.to_bits()) };
            for (fid, f) in def.faces.iter() {
                let fid: f64 = fid;
                for tr in face_tris(def, f) {
                    tris.push([at(tr[0]), at(tr[1]), at(tr[2])]);
                }
                let n = face_normal(def, f);
                let o = apply(m, [0.0, 0.0, 0.0]);
                let e = apply(m, n);
                facing.insert(fid.to_bits(), dot([e[0] - o[0], e[1] - o[1], e[2] - o[2]], fr.w));
                for l in &f.loops {
                    let n = l.len();
                    for i in 0..n {
                        let (Some(&a), Some(&b)) = (l.get(i), l.get(i.saturating_add(1).checked_rem(n).unwrap_or(0))) else { continue };
                        by_edge.entry(key(a, b)).or_default().push(fid);
                    }
                }
            }
            for e in def.edges.values() {
                if e.soft {
                    let fs = by_edge.get(&key(e.a, e.b)).map_or(&[][..], Vec::as_slice);
                    if let [f0, f1] = fs {
                        let a = facing.get(&f0.to_bits()).copied().unwrap_or(f64::NAN);
                        let b = facing.get(&f1.to_bits()).copied().unwrap_or(f64::NAN);
                        if a * b > 0.0 || (a.abs() < 1e-9 && b.abs() < 1e-9) {
                            continue;
                        }
                    }
                }
                edges.push([at(e.a), at(e.b)]);
            }
            true
        });
    });
    (tris, edges)
}

const G: usize = 96;

// clamped to 0 … G − 1 before the cast
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn cell(x: f64, x0: f64, w: f64) -> usize {
    let c = ((x - x0) / w).floor();
    let g = (G - 1) as f64;
    let c = if c.is_nan() { 0.0 } else { c.clamp(0.0, g) };
    c as usize
}

struct Tri {
    a: V3,
    b: V3,
    c: V3,
    area: f64,
}

/// `projectModel(model, view, { hidden })`.
pub fn project_model(model: &Model, view: &str, hidden: bool) -> Projection {
    let fr = frame(view);
    let (tris, edges) = collect(model, &fr);
    let (mut u0, mut v0, mut u1, mut v1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for e in &edges {
        for p in e {
            u0 = js::min(u0, p[0]);
            u1 = js::max(u1, p[0]);
            v0 = js::min(v0, p[1]);
            v1 = js::max(v1, p[1]);
        }
    }
    if edges.is_empty() {
        return Projection::default();
    }
    for t in &tris {
        for p in t {
            u0 = js::min(u0, p[0]);
            u1 = js::max(u1, p[0]);
            v0 = js::min(v0, p[1]);
            v1 = js::max(v1, p[1]);
        }
    }
    let diag = js::or(Some(js::hypot(u1 - u0, v1 - v0)), 1.0);
    let tol = diag * 1e-6;
    let gf = G as f64;
    let cw = js::or(Some((u1 - u0) / gf), 1.0);
    let ch = js::or(Some((v1 - v0) / gf), 1.0);
    let mut grid: Vec<Vec<usize>> = vec![Vec::new(); G * G];
    let mut tl: Vec<Tri> = Vec::new();
    for [a, b, c] in tris {
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
        if area.abs() < diag * diag * 1e-12 {
            continue;
        }
        let i = tl.len();
        let cx0 = cell(js::min(a[0], js::min(b[0], c[0])), u0, cw);
        let cx1 = cell(js::max(a[0], js::max(b[0], c[0])), u0, cw);
        let cy0 = cell(js::min(a[1], js::min(b[1], c[1])), v0, ch);
        let cy1 = cell(js::max(a[1], js::max(b[1], c[1])), v0, ch);
        tl.push(Tri { a, b, c, area });
        for y in cy0..=cy1 {
            for x in cx0..=cx1 {
                if let Some(g) = grid.get_mut(y.saturating_mul(G).saturating_add(x)) {
                    g.push(i);
                }
            }
        }
    }
    let behind = |u: f64, v: f64, d: f64| -> bool {
        let Some(list) = grid.get(cell(v, v0, ch).saturating_mul(G).saturating_add(cell(u, u0, cw))) else { return false };
        for &i in list {
            let Some(t) = tl.get(i) else { continue };
            let (a, b, c) = (t.a, t.b, t.c);
            let l1 = ((b[0] - u) * (c[1] - v) - (c[0] - u) * (b[1] - v)) / t.area;
            let l2 = ((c[0] - u) * (a[1] - v) - (a[0] - u) * (c[1] - v)) / t.area;
            let l3 = 1.0 - l1 - l2;
            if l1 < 1e-9 || l2 < 1e-9 || l3 < 1e-9 {
                continue;
            }
            if l1 * a[2] + l2 * b[2] + l3 * c[2] > d + tol {
                return true;
            }
        }
        false
    };
    let mut vis: Vec<[f64; 4]> = Vec::new();
    let mut hid: Vec<[f64; 4]> = Vec::new();
    for [a, b] in &edges {
        let l = js::hypot(b[0] - a[0], b[1] - a[1]);
        if l < tol {
            continue;
        }
        let pt = |s: f64| -> V3 { [a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s, a[2] + (b[2] - a[2]) * s] };
        let hidf = |s: f64| {
            let p = pt(s);
            behind(p[0], p[1], p[2])
        };
        let n = js::max(1.0, js::min(400.0, f64::ceil(l / (diag / 300.0))));
        let mut runs: Vec<(f64, f64, bool)> = Vec::new();
        let mut s0 = 0.0;
        let mut prev = hidf(0.5 / n);
        let mut i = 1.0;
        while i < n {
            let cur = hidf((i + 0.5) / n);
            if cur != prev {
                let (mut lo, mut hi) = ((i - 0.5) / n, (i + 0.5) / n);
                let mut guard: u32 = 0;
                while (hi - lo) * l > tol && guard < 200 {
                    let mid = (lo + hi) / 2.0;
                    if hidf(mid) == prev {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                    guard = guard.saturating_add(1);
                }
                runs.push((s0, (lo + hi) / 2.0, prev));
                s0 = (lo + hi) / 2.0;
                prev = cur;
            }
            i += 1.0;
        }
        runs.push((s0, 1.0, prev));
        for (r0, r1, h) in runs {
            if (r1 - r0) * l < tol * 10.0 {
                continue;
            }
            let (p, q) = (pt(r0), pt(r1));
            if h { &mut hid } else { &mut vis }.push([p[0], p[1], q[0], q[1]]);
        }
    }
    let vm = merge(&vis, tol);
    let visible = flatten(&vm, None);
    let hidden = if hidden { flatten(&merge(&hid, tol), Some(&vm)) } else { Vec::new() };
    Projection { visible, hidden, bbox: Some([u0, v0, u1, v1]) }
}

struct Line {
    dx: f64,
    dy: f64,
    off: f64,
    spans: Vec<[f64; 2]>,
}

/// Lines in the order first met, found by their key.
#[derive(Default)]
struct Lines {
    list: Vec<((i64, i64), Line)>,
    at: HashMap<(i64, i64), usize>,
}

/// `merge(segs, tol)`: segments grouped by the line they lie on, their spans merged.
fn merge(segs: &[[f64; 4]], tol: f64) -> Lines {
    let mut lines = Lines::default();
    for [x1, y1, x2, y2] in segs.iter().copied() {
        let (mut dx, mut dy) = (x2 - x1, y2 - y1);
        let l = js::hypot(dx, dy);
        if l < tol {
            continue;
        }
        dx /= l;
        dy /= l;
        if dx < -1e-12 || (dx.abs() <= 1e-12 && dy < 0.0) {
            dx = -dx;
            dy = -dy;
        }
        let off = x1 * -dy + y1 * dx;
        // `Math.round(…) + ':' + Math.round(…)`: whole numbers (a float → i64 cast saturates; NaN → 0)
        #[allow(clippy::cast_possible_truncation)]
        let k = (js::round(dy.jatan2(dx) * 1e6) as i64, js::round(off / (tol * 20.0)) as i64);
        let idx = match lines.at.get(&k) {
            Some(i) => *i,
            None => {
                lines.list.push((k, Line { dx, dy, off, spans: Vec::new() }));
                let i = lines.list.len().saturating_sub(1);
                lines.at.insert(k, i);
                i
            }
        };
        if let Some((_, ln)) = lines.list.get_mut(idx) {
            // this segment's own direction (the line keeps the first one's)
            let a = x1 * dx + y1 * dy;
            let b = x2 * dx + y2 * dy;
            ln.spans.push([js::min(a, b), js::max(a, b)]);
        }
    }
    for (_, ln) in &mut lines.list {
        ln.spans.sort_by(|p, q| (p[0] - q[0]).partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut m: Vec<[f64; 2]> = Vec::new();
        for sp in &ln.spans {
            match m.last_mut() {
                Some(last) if sp[0] <= last[1] + tol => last[1] = js::max(last[1], sp[1]),
                _ => m.push(*sp),
            }
        }
        ln.spans = m;
    }
    lines
}

/// `flatten(lines, minus)`: merged lines → segments; `minus` (merged visible lines) takes away what it covers.
fn flatten(lines: &Lines, minus: Option<&Lines>) -> Vec<[f64; 4]> {
    let mut out = Vec::new();
    for (k, ln) in &lines.list {
        let mut spans = ln.spans.clone();
        if let Some(cut) = minus.and_then(|m| m.at.get(k).and_then(|i| m.list.get(*i))).map(|(_, l)| &l.spans) {
            spans = spans
                .into_iter()
                .flat_map(|[a, b]| {
                    let mut parts = vec![[a, b]];
                    for [c, d] in cut {
                        parts = parts
                            .into_iter()
                            .flat_map(|[p, q]| {
                                if *d <= p || *c >= q {
                                    vec![[p, q]]
                                } else {
                                    let mut v = Vec::new();
                                    if *c > p {
                                        v.push([p, *c]);
                                    }
                                    if *d < q {
                                        v.push([*d, q]);
                                    }
                                    v
                                }
                            })
                            .collect();
                    }
                    parts
                })
                .collect();
        }
        for [a, b] in spans {
            if b - a <= 0.0 {
                continue;
            }
            let (px, py) = (-ln.dy * ln.off, ln.dx * ln.off);
            out.push([px + ln.dx * a, py + ln.dy * a, px + ln.dx * b, py + ln.dy * b]);
        }
    }
    out
}

/// `parseScale('1:50')` → 0.02; '2:1', '1/20', '0.5' too; None when it is not a scale.
pub fn parse_scale(s: &str) -> Option<f64> {
    let t = js::trim(s);
    let ratio = || -> Option<f64> {
        let (a, b) = t.split_once([':', '/'])?;
        let num = |x: &str| -> Option<f64> {
            let x = x.trim_matches(|c: char| c.is_whitespace());
            let ok = !x.is_empty()
                && x.chars().all(|c| c.is_ascii_digit() || c == '.')
                && x.matches('.').count() <= 1
                && !x.starts_with('.')
                && !x.ends_with('.');
            ok.then(|| js::str_to_number(x))
        };
        Some(num(a)? / num(b)?)
    };
    let v = match ratio() {
        Some(v) => v,
        None => js::str_to_number(t),
    };
    (v.is_finite() && v > 0.0 && v < 1e6).then_some(v)
}

const STD: [f64; 17] = [
    10.0,
    5.0,
    2.0,
    1.0,
    1.0 / 2.0,
    1.0 / 5.0,
    1.0 / 10.0,
    1.0 / 20.0,
    1.0 / 25.0,
    1.0 / 50.0,
    1.0 / 100.0,
    1.0 / 200.0,
    1.0 / 500.0,
    1.0 / 1000.0,
    1.0 / 2000.0,
    1.0 / 5000.0,
    1.0 / 10000.0,
];

/// flatview.js `scaleLabel(s)`.
pub fn scale_label(s: f64) -> String {
    if s >= 1.0 {
        format!("{}:1", js::num_to_string(js::round(s * 1000.0) / 1000.0))
    } else {
        format!("1:{}", js::num_to_string(js::round(1000.0 / s) / 1000.0))
    }
}

/// VIEWBASE (FLATSHOT).
pub async fn viewbase(c: Ctx) -> R {
    // the model the 3D commands keep while the drawing holds its snapshot (else read from it once)
    let empty = crate::cmd::solid3d::with_model(&c, cc_io_cad::m3dmodel::is_empty);
    if empty {
        c.print(&c.t("viewbase.empty"));
        return Ok(());
    }
    let names = ["front", "top", "right", "left", "back", "bottom", "iso"];
    let key_of = |n: &str| match n {
        "bottom" => "BO",
        "back" => "BA",
        "front" => "F",
        "top" => "T",
        "right" => "R",
        "left" => "L",
        _ => "I",
    };
    let options = names.iter().map(|n| c.opt(key_of(n), &c.t(&format!("viewbase.view.{n}")))).collect();
    let k1 = c.keyword(c.t("viewbase.which"), Opts { options, default: Some(DefOpt::Str("F".into())), ..Opts::default() }).await?;
    let o1 = match k1 {
        Reply::Opt(o) | Reply::Str(o) => o,
        _ => String::new(),
    };
    if o1.is_empty() {
        return Ok(());
    }
    let view = match o1.to_uppercase().as_str() {
        "T" => "top",
        "R" => "right",
        "L" => "left",
        "BA" => "back",
        "BO" => "bottom",
        "I" => "iso",
        _ => "front",
    };
    let options = vec![c.opt("S", &c.t("opt.show")), c.opt("H", &c.t("opt.hide"))];
    let k2 = c.keyword(c.t("viewbase.hidden"), Opts { options, default: Some(DefOpt::Str("S".into())), ..Opts::default() }).await?;
    let o2 = match k2 {
        Reply::Opt(o) | Reply::Str(o) => o,
        _ => String::new(),
    };
    if o2.is_empty() {
        return Ok(());
    }
    let pr = crate::cmd::solid3d::with_model(&c, |m| project_model(m, view, o2 != "H"));
    let Some([bu0, bv0, bu1, bv1]) = pr.bbox else {
        c.print(&c.t("viewbase.empty"));
        return Ok(());
    };
    // drawing units per meter of the model, times the view's scale (sheets: paper units)
    let (paper, units) = {
        let d = c.doc();
        let paper = d.layout().map(|l| (l.unit().map(str::to_string), l.w(), l.h()));
        let units = d.meta.get("units").and_then(Value::as_str).unwrap_or("").to_string();
        (paper, units)
    };
    let k = cc_io_cad::m3dmodel::m_per_unit(&units);
    let paper_per_m = match &paper {
        Some((u, _, _)) => {
            if u.as_deref() == Some("in") {
                1.0 / 0.0254
            } else {
                1000.0
            }
        }
        None => 1.0 / k,
    };
    let (w_m, h_m) = (bu1 - bu0, bv1 - bv0);
    let mut scale = 1.0;
    if let Some((_, pw, ph)) = &paper {
        // `L.paper?.w || 420`
        let pw = pw.filter(|x| *x != 0.0 && !x.is_nan()).unwrap_or(420.0);
        let ph = ph.filter(|x| *x != 0.0 && !x.is_nan()).unwrap_or(297.0);
        scale = STD.iter().copied().find(|s| w_m * paper_per_m * s <= pw * 0.45 && h_m * paper_per_m * s <= ph * 0.45).unwrap_or(STD[16]);
    }
    loop {
        let v = c.string(c.tv("viewbase.scale", &[("scale", scale_label(scale))]), Opts::default()).await?;
        let v = if let Reply::Str(s) = v { s } else { String::new() };
        if js::trim(&v).is_empty() {
            break;
        }
        if let Some(s) = parse_scale(&v) {
            scale = s;
            break;
        }
        c.print(&c.t("viewbase.badscale"));
    }
    let f = paper_per_m * scale;
    let (cu, cv) = ((bu0 + bu1) / 2.0, (bv0 + bv1) / 2.0);
    let line = |layer: &str, s: &[f64; 4]| json!({ "type": "line", "layer": layer, "x1": (s[0] - cu) * f, "y1": (s[1] - cv) * f, "x2": (s[2] - cu) * f, "y2": (s[3] - cv) * f });
    let ents: Vec<Entity> = pr
        .visible
        .iter()
        .map(|s| line("VIEW-VISIBLE", s))
        .chain(pr.hidden.iter().map(|s| line("VIEW-HIDDEN", s)))
        .map(|v| Entity::from_value(&v))
        .collect();
    let (hw, hh) = (w_m * f / 2.0, h_m * f / 2.0);
    c.preview(Some(std::rc::Rc::new(move |_, cur| {
        [[-hw, -hh, hw, -hh], [hw, -hh, hw, hh], [hw, hh, -hw, hh], [-hw, hh, -hw, -hh]]
            .iter()
            .map(|[a, b, d, e]| {
                Entity::from_value(
                    &json!({ "type": "line", "layer": "0", "x1": cur.x + a, "y1": cur.y + b, "x2": cur.x + d, "y2": cur.y + e }),
                )
            })
            .collect()
    })));
    let at = c.must_point(c.t("viewbase.at"), Opts::default()).await?;
    c.preview(None);
    let label = c.t(&format!("viewbase.view.{view}"));
    let up: String = label.to_uppercase();
    // `.replace(/[^A-Z0-9]+/g, '')`
    let base = format!("VIEW-{}", up.chars().filter(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit()).collect::<String>());
    let name = {
        let d = c.doc();
        let mut n: u64 = 1;
        while d.blocks.contains_key(&format!("{base}-{n}")) && n < 1_000_000 {
            n = n.saturating_add(1);
        }
        format!("{base}-{n}")
    };
    let has_hidden = !pr.hidden.is_empty();
    let current = c.doc().current.layer.clone();
    {
        let block = cc_cad::doc::Block::new(&name, cc_geom::P2::new(0.0, 0.0), ents);
        c.change("VIEWBASE", |d| {
            if !d.layers.contains_key("VIEW-VISIBLE") {
                let p = json!({ "color": 7, "lineweight": 0.5 });
                d.set_layer("VIEW-VISIBLE", p.as_object().unwrap_or(&serde_json::Map::new()), false)?;
            }
            if has_hidden && !d.layers.contains_key("VIEW-HIDDEN") {
                let p = json!({ "color": 8, "linetype": "HIDDEN", "lineweight": 0.25 });
                d.set_layer("VIEW-HIDDEN", p.as_object().unwrap_or(&serde_json::Map::new()), false)?;
            }
            d.define_block(&block)?;
            d.add(Entity::from_value(
                &json!({ "type": "insert", "layer": current, "block": name, "x": at.x, "y": at.y, "sx": 1, "sy": 1, "rot": 0 }),
            ))?;
            Ok(())
        })?;
    }
    c.print(&c.tv(
        "viewbase.done",
        &[
            ("name", name),
            ("view", label),
            ("scale", scale_label(scale)),
            ("visible", pr.visible.len().to_string()),
            ("hidden", pr.hidden.len().to_string()),
        ],
    ));
    Ok(())
}
