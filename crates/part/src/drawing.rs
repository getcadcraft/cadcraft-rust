//! drawing.js — a drawing sheet from the part: front, top and right orthographic views in third-angle projection plus
//! an isometric view, hidden edges dashed, a section view (from the views data) with hatching, a detail view, overall
//! dimensions, GD&T notes, a hole table, the title block and the revision block; written as a vector PDF.
//! meshSection (the mesh kernel's section view) needs manifold trimByPlane and is not built here: a section comes in
//! with the views data (`views.section`). Spec gap: the PDF's content stream is compressed with miniz_oxide (the JS
//! uses the browser's CompressionStream) and carries the time it was written, so the PDF bytes differ; the content
//! stream before compression is the JS's byte for byte.

use crate::font;
use crate::gdt;
use crate::i18n::{F, t};
use crate::js;
use crate::jv;
use crate::part;
use crate::re;
use crate::vec::{self as v, V3};
use serde_json::Value;
use std::collections::HashMap;

pub type Seg = [f64; 4];

/// A view direction.
#[derive(Clone, Copy, Debug)]
pub struct ViewDir {
    pub dir: V3,
    pub right: V3,
    pub up: V3,
}
/// `VIEWS[k]`.
pub fn view_dir(k: &str) -> Option<ViewDir> {
    Some(match k {
        "front" => ViewDir { dir: [0.0, 0.0, 1.0], right: [1.0, 0.0, 0.0], up: [0.0, 1.0, 0.0] },
        "top" => ViewDir { dir: [0.0, 1.0, 0.0], right: [1.0, 0.0, 0.0], up: [0.0, 0.0, -1.0] },
        "right" => ViewDir { dir: [1.0, 0.0, 0.0], right: [0.0, 0.0, -1.0], up: [0.0, 1.0, 0.0] },
        "iso" => {
            let d = v::norm([1.0, 1.0, 1.0]);
            let r = v::norm(v::cross([0.0, 1.0, 0.0], d));
            ViewDir { dir: d, right: r, up: v::cross(d, r) }
        }
        _ => return None,
    })
}
fn view_label(k: &str) -> String {
    t(&format!("model.drawing.view.{k}"), &[])
}
pub const SCALES: [f64; 13] =
    [10.0, 5.0, 2.0, 1.0, 1.0 / 2.0, 1.0 / 5.0, 1.0 / 10.0, 1.0 / 20.0, 1.0 / 50.0, 1.0 / 100.0, 1.0 / 200.0, 1.0 / 500.0, 1.0 / 1000.0];
pub const PAPERS: [(&str, f64, f64); 10] = [
    ("a4", 297.0, 210.0),
    ("a3", 420.0, 297.0),
    ("a2", 594.0, 420.0),
    ("a1", 841.0, 594.0),
    ("a0", 1189.0, 841.0),
    ("ansi-a", 279.4, 215.9),
    ("ansi-b", 431.8, 279.4),
    ("ansi-c", 558.8, 431.8),
    ("ansi-d", 863.6, 558.8),
    ("ansi-e", 1117.6, 863.6),
];
fn paper(k: &str) -> Option<(f64, f64)> {
    PAPERS.iter().find(|(n, _, _)| *n == k).map(|(_, w, h)| (*w, *h))
}

/// `sheetLabel(k)`.
pub fn sheet_label(k: &str) -> String {
    let (w, h) = paper(k).unwrap_or((420.0, 297.0));
    if k.starts_with("ansi") {
        format!(
            "ANSI {} ({} × {} in)",
            js::upper(k.get(5..).unwrap_or("")),
            js::num_str(js::fixed_num(w / 25.4, 1)),
            js::num_str(js::fixed_num(h / 25.4, 1))
        )
    } else {
        format!("{} ({} × {} mm)", js::upper(k), js::num_str(w), js::num_str(h))
    }
}

/// `nextRevision(revs)`.
pub fn next_revision(revs: &[Value]) -> String {
    let last = revs.last().map(|r| if jv::truthy(r.get("rev")) { js::string_of(r.get("rev")) } else { String::new() }).unwrap_or_default();
    if last.is_empty() {
        return "A".into();
    }
    if last.bytes().all(|b| b.is_ascii_digit()) {
        return js::num_str(js::str_num(&last) + 1.0);
    }
    if last.bytes().all(|b| b.is_ascii_uppercase()) {
        let mut a: Vec<u8> = last.bytes().collect();
        for i in (0..a.len()).rev() {
            if let Some(c) = a.get_mut(i) {
                if *c != b'Z' {
                    *c = c.saturating_add(1);
                    return String::from_utf8_lossy(&a).into_owned();
                }
                *c = b'A';
            }
        }
        return format!("A{}", String::from_utf8_lossy(&a));
    }
    format!("{last}1")
}

fn sharp() -> f64 {
    js::cos(20.0 * std::f64::consts::PI / 180.0)
}
const MARGIN0: f64 = 10.0;

/// `scaleLabel(s)`.
pub fn scale_label(s: f64) -> String {
    if s >= 1.0 { format!("{}:1", js::num_str(s)) } else { format!("1:{}", js::num_str(js::round(1.0 / s))) }
}

/// A mesh for drawing: positions (f32 values as f64; section meshes keep f64) and indices.
#[derive(Clone, Debug, Default)]
pub struct DMesh {
    pub positions: Vec<f64>,
    pub indices: Vec<u32>,
}
impl DMesh {
    fn tri_count(&self) -> usize {
        self.indices.len() / 3
    }
    fn idx(&self, i: usize) -> usize {
        self.indices.get(i).map_or(usize::MAX, |&x| x as usize)
    }
    fn p(&self, vi: usize) -> V3 {
        let g =
            |k: usize| vi.checked_mul(3).and_then(|b| b.checked_add(k)).and_then(|j| self.positions.get(j)).copied().unwrap_or(f64::NAN);
        [g(0), g(1), g(2)]
    }
}

/// One mesh edge.
#[derive(Clone, Copy, Debug)]
pub struct Edge {
    pub a: usize,
    pub b: usize,
    pub t1: usize,
    pub t2: Option<usize>,
    pub sharp: bool,
}

/// `meshEdges(mesh)` → (edges, triangle normals).
pub fn mesh_edges(m: &DMesh) -> (Vec<Edge>, Vec<V3>) {
    let nt = m.tri_count();
    let mut normals = Vec::with_capacity(nt);
    for t in 0..nt {
        let p = |k: usize| m.p(m.idx(t.saturating_mul(3).saturating_add(k)));
        normals.push(v::norm(v::cross(v::sub(p(1), p(0)), v::sub(p(2), p(0)))));
    }
    let mut order: Vec<Edge> = Vec::new();
    let mut at: HashMap<(usize, usize), usize> = HashMap::new();
    for t in 0..nt {
        for k in 0..3usize {
            let a = m.idx(t.saturating_mul(3).saturating_add(k));
            let b = m.idx(t.saturating_mul(3).saturating_add(k.saturating_add(1) % 3));
            let key = if a < b { (a, b) } else { (b, a) };
            match at.get(&key) {
                Some(&i) => {
                    if let Some(e) = order.get_mut(i) {
                        e.t2 = Some(t);
                    }
                }
                None => {
                    at.insert(key, order.len());
                    order.push(Edge { a: a.min(b), b: a.max(b), t1: t, t2: None, sharp: false });
                }
            }
        }
    }
    let sh = sharp();
    for e in &mut order {
        e.sharp = match e.t2 {
            None => true,
            Some(t2) => v::dot(normals.get(e.t1).copied().unwrap_or([f64::NAN; 3]), normals.get(t2).copied().unwrap_or([f64::NAN; 3])) < sh,
        };
    }
    (order, normals)
}

/// A projected view.
#[derive(Clone, Debug, Default)]
pub struct View {
    pub visible: Vec<Seg>,
    pub hidden: Vec<Seg>,
    pub bx: [f64; 4],
    pub cut: Option<Vec<Seg>>,
    pub trace: Option<Value>,
    pub of: Option<String>,
    pub at: Option<f64>,
}
impl View {
    pub fn to_json(&self) -> Value {
        let segs = |s: &[Seg]| Value::Array(s.iter().map(|g| jv::nums(g)).collect());
        let mut o = jv::obj(vec![
            ("visible", segs(&self.visible)),
            ("hidden", segs(&self.hidden)),
            (
                "box",
                jv::obj(vec![
                    ("u0", jv::num(self.bx[0])),
                    ("v0", jv::num(self.bx[1])),
                    ("u1", jv::num(self.bx[2])),
                    ("v1", jv::num(self.bx[3])),
                ]),
            ),
        ]);
        if let Some(c) = &self.cut {
            jv::set(&mut o, "cut", segs(c));
        }
        if let Some(of) = &self.of {
            jv::set(&mut o, "of", of.as_str().into());
        }
        if let Some(a) = self.at {
            jv::set(&mut o, "at", jv::num(a));
        }
        if let Some(tr) = &self.trace {
            jv::set(&mut o, "trace", tr.clone());
        }
        o
    }
    pub fn from_json(x: &Value) -> View {
        let segs = |k: &str| -> Vec<Seg> {
            jv::arr(x.get(k))
                .iter()
                .map(|s| {
                    let a = jv::arr(Some(s));
                    let g = |i: usize| jv::as_num(a.get(i)).unwrap_or(f64::NAN);
                    [g(0), g(1), g(2), g(3)]
                })
                .collect()
        };
        let b = x.get("box");
        let g = |k: &str| jv::as_num(b.and_then(|b| b.get(k))).unwrap_or(f64::NAN);
        View {
            visible: segs("visible"),
            hidden: segs("hidden"),
            bx: [g("u0"), g("v0"), g("u1"), g("v1")],
            cut: x.get("cut").filter(|c| c.is_array()).map(|_| segs("cut")),
            trace: x.get("trace").filter(|c| jv::truthy(Some(c))).cloned(),
            of: x.get("of").and_then(Value::as_str).map(str::to_string),
            at: jv::as_num(x.get("at")),
        }
    }
    fn bw(&self) -> f64 {
        self.bx[2] - self.bx[0]
    }
    fn bh(&self) -> f64 {
        self.bx[3] - self.bx[1]
    }
}

fn mini(a: f64, b: f64) -> f64 {
    js::min(a, b)
}
fn maxi(a: f64, b: f64) -> f64 {
    js::max(a, b)
}

/// `projectView(mesh, edgesInfo, view, { hidden })`.
pub fn project_view(m: &DMesh, edges: &[Edge], normals: &[V3], view: &ViewDir, with_hidden: bool) -> View {
    let nt = m.tri_count();
    let nv = m.positions.len() / 3;
    let mut q: Vec<V3> = Vec::with_capacity(nv);
    let (mut u0, mut v0, mut u1, mut v1, mut wmin, mut wmax) =
        (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for i in 0..nv {
        let p = m.p(i);
        let qq = [v::dot(p, view.right), v::dot(p, view.up), v::dot(p, view.dir)];
        u0 = mini(u0, qq[0]);
        u1 = maxi(u1, qq[0]);
        v0 = mini(v0, qq[1]);
        v1 = maxi(v1, qq[1]);
        wmin = mini(wmin, qq[2]);
        wmax = maxi(wmax, qq[2]);
        q.push(qq);
    }
    let size = maxi(maxi(maxi(u1 - u0, v1 - v0), wmax - wmin), 1e-9);
    const G: usize = 96;
    let gf = js::f(G);
    let cw = (u1 - u0) / gf;
    let cell_w = if cw == 0.0 || cw.is_nan() { 1.0 } else { cw };
    let ch = (v1 - v0) / gf;
    let cell_h = if ch == 0.0 || ch.is_nan() { 1.0 } else { ch };
    let mut grid: Vec<Vec<usize>> = vec![Vec::new(); G.saturating_mul(G)];
    let qv = |i: usize| q.get(i).copied().unwrap_or([f64::NAN; 3]);
    let tri =
        |t: usize| (m.idx(t.saturating_mul(3)), m.idx(t.saturating_mul(3).saturating_add(1)), m.idx(t.saturating_mul(3).saturating_add(2)));
    let nd = |t: usize| {
        let n = normals.get(t).copied().unwrap_or([f64::NAN; 3]);
        n[0] * view.dir[0] + n[1] * view.dir[1] + n[2] * view.dir[2]
    };
    let cell = |x: f64| -> usize { js::usize_of(js::i64_of(x)) };
    for t in 0..nt {
        if nd(t) <= 1e-9 || nd(t).is_nan() {
            continue;
        }
        let (a, b, c) = tri(t);
        let (qa, qb, qc) = (qv(a), qv(b), qv(c));
        let tu0 = mini(mini(qa[0], qb[0]), qc[0]);
        let tu1 = maxi(maxi(qa[0], qb[0]), qc[0]);
        let tv0 = mini(mini(qa[1], qb[1]), qc[1]);
        let tv1 = maxi(maxi(qa[1], qb[1]), qc[1]);
        let gi0 = maxi(0.0, ((tu0 - u0) / cell_w).floor());
        let gi1 = mini(gf - 1.0, ((tu1 - u0) / cell_w).floor());
        let gj0 = maxi(0.0, ((tv0 - v0) / cell_h).floor());
        let gj1 = mini(gf - 1.0, ((tv1 - v0) / cell_h).floor());
        if !(gi0 <= gi1 && gj0 <= gj1) {
            continue;
        }
        for i in cell(gi0)..=cell(gi1) {
            for j in cell(gj0)..=cell(gj1) {
                if let Some(g) = grid.get_mut(j.saturating_mul(G).saturating_add(i)) {
                    g.push(t);
                }
            }
        }
    }
    let eps = 1e-6 * size;
    let occluded = |u: f64, vv: f64, w: f64| -> bool {
        let i = cell(mini(gf - 1.0, maxi(0.0, ((u - u0) / cell_w).floor())));
        let j = cell(mini(gf - 1.0, maxi(0.0, ((vv - v0) / cell_h).floor())));
        for &t in grid.get(j.saturating_mul(G).saturating_add(i)).map_or(&[][..], Vec::as_slice) {
            let (a, b, c) = tri(t);
            let (qa, qb, qc) = (qv(a), qv(b), qv(c));
            let (x1, y1, x2, y2, x3, y3) = (qa[0], qa[1], qb[0], qb[1], qc[0], qc[1]);
            let den = (y2 - y3) * (x1 - x3) + (x3 - x2) * (y1 - y3);
            if den.abs() < 1e-12 * size * size {
                continue;
            }
            let l1 = ((y2 - y3) * (u - x3) + (x3 - x2) * (vv - y3)) / den;
            let l2 = ((y3 - y1) * (u - x3) + (x1 - x3) * (vv - y3)) / den;
            let l3 = 1.0 - l1 - l2;
            let mm = 1e-7;
            if l1 < mm || l2 < mm || l3 < mm {
                continue;
            }
            let depth = l1 * qa[2] + l2 * qb[2] + l3 * qc[2];
            if depth > w + eps * 10.0 {
                return true;
            }
        }
        false
    };
    let (mut visible, mut hid): (Vec<Seg>, Vec<Seg>) = (Vec::new(), Vec::new());
    let step = size / 150.0;
    for e in edges {
        let mut draw = e.sharp;
        if !draw && let Some(t2) = e.t2 {
            draw = (nd(e.t1) > 1e-9) != (nd(t2) > 1e-9);
        }
        if !draw {
            continue;
        }
        let a = qv(e.a);
        let b = qv(e.b);
        let len2 = js::hypot2(b[0] - a[0], b[1] - a[1]);
        if len2 < 1e-9 * size {
            continue;
        }
        let n = js::max(1.0, (js::hypot3(b[0] - a[0], b[1] - a[1], b[2] - a[2]) / step).ceil());
        let nn = js::usize_of(js::i64_of(n)).clamp(1, 10_000_000);
        let at = |k: usize| [a[0] + (b[0] - a[0]) * js::f(k) / n, a[1] + (b[1] - a[1]) * js::f(k) / n];
        let mut run_start = 0usize;
        let mut run_vis: Option<bool> = None;
        for k in 0..=nn {
            let mut vis: Option<bool> = None;
            if k < nn {
                let tm = (js::f(k) + 0.5) / n;
                vis = Some(!occluded(a[0] + (b[0] - a[0]) * tm, a[1] + (b[1] - a[1]) * tm, a[2] + (b[2] - a[2]) * tm));
            }
            if k == nn || (run_vis.is_some() && vis != run_vis) {
                let p = at(run_start);
                let qq = at(k);
                if run_vis == Some(true) {
                    visible.push([p[0], p[1], qq[0], qq[1]]);
                } else {
                    hid.push([p[0], p[1], qq[0], qq[1]]);
                }
                run_start = k;
            }
            run_vis = vis;
        }
    }
    let vis = merge_segments(&visible, size, &[]);
    let hidden = if with_hidden { merge_segments(&hid, size, &vis) } else { Vec::new() };
    View { visible: vis, hidden, bx: [u0, v0, u1, v1], ..View::default() }
}

/// `mergeSegments(segs, size, minus)`.
pub fn merge_segments(segs: &[Seg], size: f64, minus: &[Seg]) -> Vec<Seg> {
    let tol = 1e-6 * size;
    struct Line {
        dx: f64,
        dy: f64,
        off: f64,
        iv: Vec<[f64; 2]>,
    }
    let line_of = |s: &Seg| -> Option<(f64, f64, f64, String)> {
        let (mut dx, mut dy) = (s[2] - s[0], s[3] - s[1]);
        let l = js::hypot2(dx, dy);
        if l < tol || l.is_nan() {
            return None;
        }
        dx /= l;
        dy /= l;
        if dx < -1e-12 || (dx.abs() <= 1e-12 && dy < 0.0) {
            dx = -dx;
            dy = -dy;
        }
        let off = dx * s[1] - dy * s[0];
        let key = format!("{}:{}", js::num_str(js::round(js::atan2(dy, dx) * 1e6)), js::num_str(js::round(off / (tol * 10.0))));
        Some((dx, dy, off, key))
    };
    let group = |list: &[Seg], keys: &mut Vec<String>, into: &mut HashMap<String, Line>| {
        for s in list {
            let Some((dx, dy, off, key)) = line_of(s) else { continue };
            let g = into.entry(key.clone()).or_insert_with(|| {
                keys.push(key.clone());
                Line { dx, dy, off, iv: Vec::new() }
            });
            // the segment's own direction (JS: L, not the group's)
            let t0 = s[0] * dx + s[1] * dy;
            let t1 = s[2] * dx + s[3] * dy;
            g.iv.push([mini(t0, t1), maxi(t0, t1)]);
        }
    };
    let mut keys: Vec<String> = Vec::new();
    let mut lines: HashMap<String, Line> = HashMap::new();
    group(segs, &mut keys, &mut lines);
    let mut ckeys: Vec<String> = Vec::new();
    let mut cut: HashMap<String, Line> = HashMap::new();
    group(minus, &mut ckeys, &mut cut);
    let union = |iv: &mut Vec<[f64; 2]>| -> Vec<[f64; 2]> {
        cc_m3d::sort::timsort(iv, |a, b| a[0] - b[0]);
        let mut out: Vec<[f64; 2]> = Vec::new();
        for x in iv.iter() {
            match out.last_mut() {
                Some(last) if x[0] <= last[1] + tol => last[1] = maxi(last[1], x[1]),
                _ => out.push(*x),
            }
        }
        out
    };
    let mut result = Vec::new();
    for key in &keys {
        let Some(g) = lines.get_mut(key) else { continue };
        let mut iv = union(&mut g.iv);
        if let Some(c) = cut.get_mut(key) {
            let cu = union(&mut c.iv);
            let mut left = Vec::new();
            for [a, b] in iv {
                let mut pieces: Vec<[f64; 2]> = vec![[a, b]];
                for [x, y] in &cu {
                    pieces =
                        pieces
                            .into_iter()
                            .flat_map(|[p, qq]| {
                                if *y <= p + tol || *x >= qq - tol { vec![[p, qq]] } else { vec![[p, mini(qq, *x)], [maxi(p, *y), qq]] }
                            })
                            .filter(|[u, w]| w - u > tol)
                            .collect();
                }
                left.extend(pieces);
            }
            iv = left;
        }
        let (px, py) = (-g.dy * g.off, g.dx * g.off);
        for [a, b] in iv {
            result.push([px + g.dx * a, py + g.dy * a, px + g.dx * b, py + g.dy * b]);
        }
    }
    result
}

fn fmt_len(x: f64) -> String {
    js::num_str(js::fixed_num(x, 2))
}

/// `meshViews(mesh, { iso })` (no section: that needs manifold).
pub fn mesh_views(m: &DMesh, iso: bool) -> Vec<(String, View)> {
    let (edges, normals) = mesh_edges(m);
    let mut out = Vec::new();
    for k in ["front", "top", "right"] {
        if let Some(vd) = view_dir(k) {
            out.push((k.to_string(), project_view(m, &edges, &normals, &vd, true)));
        }
    }
    if iso && let Some(vd) = view_dir("iso") {
        out.push(("iso".to_string(), project_view(m, &edges, &normals, &vd, false)));
    }
    out
}

/// `hatch(segs, gap, angle)`.
pub fn hatch(segs: &[Seg], gap: f64, angle: f64) -> Vec<Seg> {
    let a = angle * std::f64::consts::PI / 180.0;
    let (dx, dy) = (js::cos(a), js::sin(a));
    let (nx, ny) = (-dy, dx);
    let (mut c0, mut c1) = (f64::INFINITY, f64::NEG_INFINITY);
    for s in segs {
        for (x, y) in [(s[0], s[1]), (s[2], s[3])] {
            let c = x * nx + y * ny;
            c0 = mini(c0, c);
            c1 = maxi(c1, c);
        }
    }
    let mut out = Vec::new();
    if !(c1 > c0) {
        return out;
    }
    let mut c = (c0 / gap).ceil() * gap;
    let mut guard = 0usize;
    while c < c1 && guard < 1_000_000 {
        guard = guard.saturating_add(1);
        let mut ts: Vec<f64> = Vec::new();
        for s in segs {
            let p = s[0] * nx + s[1] * ny - c;
            let qq = s[2] * nx + s[3] * ny - c;
            if (p > 0.0) == (qq > 0.0) || p == qq {
                continue;
            }
            let k = p / (p - qq);
            let x = s[0] + (s[2] - s[0]) * k;
            let y = s[1] + (s[3] - s[1]) * k;
            ts.push(x * dx + y * dy);
        }
        cc_m3d::sort::timsort(&mut ts, |u, w| u - w);
        let mut i = 0usize;
        while i.saturating_add(1) < ts.len() {
            let (t0, t1) = (ts.get(i).copied().unwrap_or(f64::NAN), ts.get(i.saturating_add(1)).copied().unwrap_or(f64::NAN));
            i = i.saturating_add(2);
            if t1 - t0 < 1e-9 {
                continue;
            }
            out.push([nx * c + dx * t0, ny * c + dy * t0, nx * c + dx * t1, ny * c + dy * t1]);
        }
        c += gap;
    }
    out
}

/// `clipToCircle(segs, c, r)`.
pub fn clip_to_circle(segs: &[Seg], c: [f64; 2], r: f64) -> Vec<Seg> {
    let mut out = Vec::new();
    for s in segs {
        let (ax, ay, bx, by) = (s[0] - c[0], s[1] - c[1], s[2] - c[0], s[3] - c[1]);
        let (dx, dy) = (bx - ax, by - ay);
        let aa = dx * dx + dy * dy;
        let bb = 2.0 * (ax * dx + ay * dy);
        let cc = ax * ax + ay * ay - r * r;
        if aa < 1e-24 {
            continue;
        }
        let disc = bb * bb - 4.0 * aa * cc;
        if disc <= 0.0 {
            continue;
        }
        let sq = js::sqrt(disc);
        let t0 = maxi(0.0, (-bb - sq) / (2.0 * aa));
        let t1 = mini(1.0, (-bb + sq) / (2.0 * aa));
        if t1 - t0 <= 1e-12 {
            continue;
        }
        out.push([s[0] + (s[2] - s[0]) * t0, s[1] + (s[3] - s[1]) * t0, s[0] + (s[2] - s[0]) * t1, s[1] + (s[3] - s[1]) * t1]);
    }
    out
}

fn circle_segs(cx: f64, cy: f64, r: f64) -> Vec<Seg> {
    let n = 72.0;
    (0..72)
        .map(|i| {
            let a = 2.0 * std::f64::consts::PI * js::f(i) / n;
            let b = 2.0 * std::f64::consts::PI * (js::f(i) + 1.0) / n;
            [cx + r * js::cos(a), cy + r * js::sin(a), cx + r * js::cos(b), cy + r * js::sin(b)]
        })
        .collect()
}

/// A hole for the hole table.
#[derive(Clone, Debug)]
pub struct Hole {
    pub o: V3,
    pub d: V3,
    pub dia: f64,
    pub depth: Option<f64>,
    pub callout: String,
    pub feature: String,
}
impl Hole {
    pub fn to_json(&self) -> Value {
        jv::obj(vec![
            ("o", jv::pt3(self.o)),
            ("d", jv::pt3(self.d)),
            ("dia", jv::num(self.dia)),
            ("depth", self.depth.map_or(Value::Null, jv::num)),
            ("callout", self.callout.as_str().into()),
            ("feature", self.feature.as_str().into()),
        ])
    }
    pub fn from_json(x: &Value) -> Hole {
        let p3 = |k: &str| {
            let a = jv::arr(x.get(k));
            let g = |i: usize| jv::as_num(a.get(i)).unwrap_or(f64::NAN);
            [g(0), g(1), g(2)]
        };
        Hole {
            o: p3("o"),
            d: p3("d"),
            dia: jv::as_num(x.get("dia")).unwrap_or(f64::NAN),
            depth: jv::as_num(x.get("depth")),
            callout: js::str_of(x.get("callout")),
            feature: js::str_of(x.get("feature")),
        }
    }
}

/// `partHoles(part, regen)`: faces = the kernel's named face records (JSON), threads = feature → callout.
pub fn part_holes(pt: &Value, named: bool, faces: Option<&[Value]>, threads: &dyn Fn(&str) -> Option<String>) -> Option<Vec<Hole>> {
    if !named {
        return None;
    }
    let faces = faces?;
    let hole_f: Vec<&Value> = part::features(pt).iter().filter(|f| jv::s(f, "type") == "hole").collect();
    let hole_of = |id: &str| hole_f.iter().rev().find(|f| jv::is_str(f.get("id"), id)).copied();
    struct G {
        fid: String,
        d: V3,
        o: V3,
        r: f64,
    }
    let mut groups: Vec<(String, G)> = Vec::new();
    for fr in faces {
        if !jv::truthy(Some(fr)) || jv::s(fr, "type") != "cylinder" || !jv::truthy(fr.get("axis")) || !jv::truthy(fr.get("name")) {
            continue;
        }
        let name = js::string_of(fr.get("name"));
        let Some(fid) = re::name_features(&name).into_iter().find(|id| hole_of(id).is_some()) else { continue };
        let ax = fr.get("axis").unwrap_or(&Value::Null);
        let p3 = |x: Option<&Value>| {
            let a = jv::arr(x);
            let g = |i: usize| jv::as_num(a.get(i)).unwrap_or(f64::NAN);
            [g(0), g(1), g(2)]
        };
        let mut d = v::norm(p3(ax.get("d")));
        if d[0] < -1e-9 || (d[0].abs() <= 1e-9 && (d[1] < -1e-9 || (d[1].abs() <= 1e-9 && d[2] < 0.0))) {
            d = v::mul(d, -1.0);
        }
        let ao = p3(ax.get("o"));
        let o = v::sub(ao, v::mul(d, v::dot(ao, d)));
        let key = format!(
            "{fid}|{}",
            [d[0], d[1], d[2], o[0], o[1], o[2]].iter().map(|x| js::num_str(js::round(x * 1e4))).collect::<Vec<_>>().join(",")
        );
        let r = jv::as_num(fr.get("r")).unwrap_or(f64::NAN);
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, g)) => g.r = js::min(g.r, r),
            None => groups.push((key, G { fid, d, o, r })),
        }
    }
    let f2 = |x: f64| js::num_str(js::fixed_num(x, 2));
    Some(
        groups
            .into_iter()
            .map(|(_, g)| {
                let f = hole_of(&g.fid).cloned().unwrap_or(Value::Null);
                let dia = 2.0 * g.r;
                let n = |k: &str| jv::as_num(f.get(k)).unwrap_or(f64::NAN);
                let through = jv::truthy(f.get("through"));
                let mut callout = if through {
                    t("drawing.holes.thru", &[("d", F::S(f2(dia)))])
                } else {
                    t("drawing.holes.deep", &[("d", F::S(f2(dia))), ("depth", F::S(f2(n("depth"))))])
                };
                match jv::s(&f, "kind") {
                    "cbore" => {
                        callout.push_str(&t("drawing.holes.cbore", &[("d", F::S(f2(n("cboreD")))), ("depth", F::S(f2(n("cboreDepth"))))]))
                    }
                    "csink" => callout.push_str(&t(
                        "drawing.holes.csink",
                        &[("d", F::S(f2(n("csinkD")))), ("angle", F::S(js::string_of(f.get("csinkAngle"))))],
                    )),
                    "tapped" => {
                        if let Some(c) = threads(&g.fid).filter(|c| !c.is_empty()) {
                            callout = c;
                        }
                    }
                    _ => {}
                }
                Hole {
                    o: g.o,
                    d: g.d,
                    dia,
                    depth: if through { None } else { Some(n("depth")) },
                    callout,
                    feature: js::string_of(f.get("name")),
                }
            })
            .collect(),
    )
}

/// A hole table row.
#[derive(Clone, Debug)]
pub struct HoleRow {
    pub u: f64,
    pub v: f64,
    pub x: f64,
    pub y: f64,
    pub dia: f64,
    pub size: String,
    pub depth: Option<f64>,
    pub feature: String,
    pub tag: String,
}

/// `holeTableRows(holes, view, box)`.
pub fn hole_table_rows(holes: &[Hole], view: &str, bx: [f64; 4]) -> (Vec<HoleRow>, usize) {
    let Some(v0) = view_dir(view) else { return (Vec::new(), holes.len()) };
    let mut rows: Vec<HoleRow> = Vec::new();
    let mut skipped = 0usize;
    for h in holes {
        if v::dot(v::norm(h.d), v0.dir).abs() < 1.0 - 1e-9 {
            skipped = skipped.saturating_add(1);
            continue;
        }
        let u = v::dot(h.o, v0.right);
        let vv = v::dot(h.o, v0.up);
        let size = if h.callout.is_empty() { format!("Ø{}", fmt_len(h.dia)) } else { h.callout.clone() };
        rows.push(HoleRow {
            u,
            v: vv,
            x: u - bx[0],
            y: vv - bx[1],
            dia: h.dia,
            size,
            depth: h.depth,
            feature: h.feature.clone(),
            tag: String::new(),
        });
    }
    let key = |r: &HoleRow| format!("{}|{}", js::to_fixed(r.dia, 6), r.size);
    let mut sizes: Vec<String> = Vec::new();
    for r in &rows {
        let k = key(r);
        if !sizes.contains(&k) {
            sizes.push(k);
        }
    }
    let pf = |s: &str| js::str_num(s.split('|').next().unwrap_or(""));
    cc_m3d::sort::timsort(&mut sizes, |a, b| {
        let d = pf(a) - pf(b);
        if d != 0.0 && !d.is_nan() {
            return d;
        }
        // a.localeCompare(b): code-unit order (ASCII digits, '|', the callout)
        match js::cmp16(a, b) {
            std::cmp::Ordering::Less => -1.0,
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => 1.0,
        }
    });
    let idx = |r: &HoleRow| sizes.iter().position(|s| *s == key(r)).map_or(-1.0, js::f);
    cc_m3d::sort::timsort(&mut rows, |a, b| {
        let d = idx(a) - idx(b);
        if d != 0.0 {
            return d;
        }
        let e = b.y - a.y;
        if e != 0.0 && !e.is_nan() {
            return e;
        }
        a.x - b.x
    });
    let mut count: HashMap<usize, usize> = HashMap::new();
    for r in &mut rows {
        let k = sizes.iter().position(|s| *s == key(r)).unwrap_or(0);
        let n = count.get(&k).copied().unwrap_or(0).saturating_add(1);
        count.insert(k, n);
        r.tag = format!(
            "{}{n}",
            if k < 26 {
                char::from(u8::try_from(65usize.saturating_add(k)).unwrap_or(b'A')).to_string()
            } else {
                format!("Z{}", k.saturating_sub(25))
            }
        );
    }
    (rows, skipped)
}

/// `textWidth(s, size)`.
fn text_width(s: &str, size: f64) -> f64 {
    s.chars().fold(0.0, |a, ch| a + font::char_advance(ch)) * size / 1000.0
}
/// `fitText(s, w, size)`.
fn fit_text(s: &str, w: f64, size: f64) -> String {
    if text_width(s, size) <= w {
        return s.to_string();
    }
    let (mut lo, mut hi) = (0usize, js::len16(s));
    while lo < hi {
        let m = (lo.saturating_add(hi).saturating_add(1)) >> 1;
        if text_width(&format!("{}…", js::slice16(s, m)), size) <= w {
            lo = m;
        } else {
            hi = m.saturating_sub(1);
        }
    }
    format!("{}…", js::slice16(s, lo))
}

fn arrow(fills: &mut Vec<Value>, p: [f64; 2], d: [f64; 2]) {
    let (l, w) = (3.0, 0.9);
    let (x, y) = (p[0], p[1]);
    let (dx, dy) = (d[0], d[1]);
    fills.push(jv::obj(vec![(
        "pts",
        Value::Array(vec![
            jv::pt2([x, y]),
            jv::pt2([x - dx * l - dy * w, y - dy * l + dx * w]),
            jv::pt2([x - dx * l + dy * w, y - dy * l - dx * w]),
        ]),
    )]));
}

/// A text item.
#[derive(Clone, Debug)]
struct Tx {
    x: f64,
    y: f64,
    size: f64,
    text: String,
    anchor: &'static str,
    bold: Option<bool>,
    rot: Option<f64>,
}
impl Tx {
    fn new(x: f64, y: f64, size: f64, text: impl Into<String>, anchor: &'static str) -> Tx {
        Tx { x, y, size, text: text.into(), anchor, bold: None, rot: None }
    }
    fn bold(mut self, b: bool) -> Tx {
        self.bold = Some(b);
        self
    }
    fn to_json(&self) -> Value {
        let mut o = jv::obj(vec![
            ("x", jv::num(self.x)),
            ("y", jv::num(self.y)),
            ("size", jv::num(self.size)),
            ("text", self.text.as_str().into()),
            ("anchor", self.anchor.into()),
        ]);
        if let Some(r) = self.rot {
            jv::set(&mut o, "rot", jv::num(r));
        }
        if let Some(b) = self.bold {
            jv::set(&mut o, "bold", b.into());
        }
        o
    }
}

#[derive(Clone, Copy, Debug)]
struct Place {
    x: f64,
    y: f64,
    s: f64,
    bx: [f64; 4],
}

/// Options of `composeSheet`.
pub struct SheetOpts<'a> {
    pub name: String,
    /// `d.toISOString().slice(0, 10)` of the date
    pub date_ymd: String,
    pub paper: Option<String>,
    pub drawing: Value,
    pub annotate: bool,
    pub material: String,
    pub holes: Option<&'a [Hole]>,
}

/// `composeSheet(data, opts)`; views = (key, view) in data order; threads = data.threads.
pub fn compose_sheet(views: &[(String, View)], exact: bool, threads: &[Value], o: &SheetOpts) -> Result<Value, String> {
    let drawing = &o.drawing;
    let vget = |k: &str| views.iter().find(|(n, _)| n == k).map(|(_, v)| v);
    let sheet_key = match drawing.get("sheet").and_then(Value::as_str).filter(|k| paper(k).is_some()) {
        Some(k) => k.to_string(),
        None => match o.paper.as_deref().filter(|k| paper(k).is_some()) {
            Some(k) => k.to_string(),
            None => "a3".to_string(),
        },
    };
    let (w, h) = paper(&sheet_key).unwrap_or((420.0, 297.0));
    let all_revs = jv::arr(drawing.get("revisions"));
    let revs: Vec<Value> = all_revs.get(all_revs.len().saturating_sub(part::MAX_REVISIONS)..).unwrap_or(&[]).to_vec();
    let rev_h = if revs.is_empty() { 0.0 } else { 7.0 + 5.0 * js::f(revs.len()) };
    let rev_w = mini(150.0, w - 2.0 * MARGIN0 - 40.0);
    let empty = View::default();
    let front = vget("front").ok_or("Cannot read properties of undefined (reading 'box')")?;
    let topv = vget("top").ok_or("Cannot read properties of undefined (reading 'box')")?;
    let rightv = vget("right").ok_or("Cannot read properties of undefined (reading 'box')")?;
    let (margin, gap, tb_h, tb_w) = (MARGIN0, 18.0, 40.0, 180.0);
    let area_w = w - 2.0 * margin - 2.0 * gap;
    let area_h = h - 2.0 * margin - tb_h - 2.0 * gap - rev_h;
    let need_w = front.bw() + rightv.bw();
    let need_h = front.bh() + topv.bh();
    let scale = SCALES.iter().copied().find(|s| need_w * s <= area_w * 0.82 && need_h * s <= area_h * 0.82).unwrap_or(SCALES[12]);
    // strokes: visible, hidden, thin, chain, border
    let mut st_vis: Vec<Seg> = Vec::new();
    let mut st_hid: Vec<Seg> = Vec::new();
    let mut st_thin: Vec<Seg> = Vec::new();
    let mut st_chain: Vec<Seg> = Vec::new();
    let mut st_border: Vec<Seg> = Vec::new();
    let mut extra: Vec<Value> = Vec::new();
    let mut fills: Vec<Value> = Vec::new();
    let mut texts: Vec<Tx> = Vec::new();
    let mut omitted: Vec<String> = Vec::new();
    let x0 = margin + gap;
    let y0 = margin + tb_h + gap + 6.0;
    let mut place: Vec<(String, Place)> = Vec::new();
    let pget = |pl: &[(String, Place)], k: &str| pl.iter().find(|(n, _)| n == k).map(|(_, p)| *p);
    let set_place = |pl: &mut Vec<(String, Place)>, k: &str, p: Place| match pl.iter_mut().find(|(n, _)| n == k) {
        Some(e) => e.1 = p,
        None => pl.push((k.to_string(), p)),
    };
    set_place(&mut place, "front", Place { x: x0 + 12.0, y: y0 + 8.0, s: scale, bx: front.bx });
    let pf = pget(&place, "front").unwrap_or(Place { x: 0.0, y: 0.0, s: scale, bx: front.bx });
    set_place(&mut place, "top", Place { x: pf.x, y: pf.y + front.bh() * scale + gap, s: scale, bx: topv.bx });
    set_place(&mut place, "right", Place { x: pf.x + front.bw() * scale + gap, y: pf.y, s: scale, bx: rightv.bx });
    let pt_ = pget(&place, "top").unwrap_or(pf);
    let pr = pget(&place, "right").unwrap_or(pf);
    struct Slot {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    }
    let mut slots: Vec<Slot> = Vec::new();
    {
        let left = maxi(pr.x, pt_.x + topv.bw() * scale + gap);
        let bottom = pr.y + rightv.bh() * scale + gap + 8.0;
        slots.push(Slot { x: left, y: bottom, w: w - margin - gap - left, h: h - margin - gap - rev_h - bottom });
        let l2 = pr.x + rightv.bw() * scale + gap;
        slots.push(Slot {
            x: l2,
            y: margin + tb_h + gap + 8.0,
            w: w - margin - gap - l2,
            h: bottom - gap - 8.0 - (margin + tb_h + gap + 8.0),
        });
    }
    struct Want {
        key: &'static str,
        w: f64,
        h: f64,
        max: f64,
        fixed: Option<f64>,
    }
    let mut wanted: Vec<Want> = Vec::new();
    let sec = if jv::truthy(drawing.get("section")) { vget("section") } else { None };
    let det = drawing.get("detail").filter(|d| {
        jv::truthy(Some(d)) && {
            let of = js::str_of(d.get("of"));
            vget(&of).is_some() || (of == "section" && sec.is_some())
        }
    });
    let det_r = det.map_or(f64::NAN, |d| jv::as_num(d.get("r")).unwrap_or(f64::NAN));
    let det_at = det.map_or([f64::NAN; 2], |d| {
        let a = jv::arr(d.get("at"));
        [jv::as_num(a.first()).unwrap_or(f64::NAN), jv::as_num(a.get(1)).unwrap_or(f64::NAN)]
    });
    if let Some(sv) = sec {
        wanted.push(Want { key: "section", w: sv.bw(), h: sv.bh(), max: scale, fixed: None });
    }
    if let Some(d) = det {
        let ds = scale * jv::as_num(d.get("scale")).unwrap_or(f64::NAN);
        wanted.push(Want { key: "detail", w: 2.0 * det_r, h: 2.0 * det_r, max: ds, fixed: Some(ds) });
    }
    let ht_view = drawing.get("holeTable").filter(|x| jv::truthy(Some(x))).map(|x| js::str_of(x.get("view")));
    let ht = match (&ht_view, o.holes) {
        (Some(vk), Some(holes)) if view_dir(vk).is_some() && vk != "iso" => vget(vk).map(|vw| hole_table_rows(holes, vk, vw.bx)),
        _ => None,
    };
    let mut htc = [12.0, 22.0, 22.0, 0.0];
    let htr = 5.0;
    if let Some((rows, _)) = &ht {
        let mut m = text_width(&t("drawing.holes.size", &[]), 2.5) + 4.0;
        for r in rows {
            m = maxi(m, text_width(&r.size, 2.5) + 4.0);
        }
        htc[3] = mini(90.0, maxi(30.0, m));
        wanted.push(Want { key: "holes", w: htc.iter().sum(), h: htr * (js::f(rows.len()) + 1.0) + 7.0, max: 1.0, fixed: Some(1.0) });
    }
    if let Some(iv) = vget("iso") {
        wanted.push(Want { key: "iso", w: iv.bw(), h: iv.bh(), max: scale, fixed: None });
    }
    let mut used: Vec<f64> = slots.iter().map(|_| 0.0).collect();
    let nwanted = wanted.len();
    for it in &wanted {
        let mut ok = false;
        for (i, sl) in slots.iter().enumerate() {
            if ok {
                break;
            }
            let u = used.get(i).copied().unwrap_or(0.0);
            let (fx, fw, fh) = (sl.x + u, sl.w - u, sl.h);
            if fw < 20.0 || fh < 20.0 {
                continue;
            }
            let s = match it.fixed {
                Some(fx2) => (it.w * fx2 <= fw && it.h * fx2 + 10.0 <= fh).then_some(fx2),
                None => SCALES.iter().copied().find(|q| *q <= it.max && it.w * q <= fw && it.h * q + 10.0 <= fh),
            };
            let Some(s) = s else { continue };
            let (wv, hv) = (it.w * s, it.h * s);
            let px = fx + if nwanted > 1 && sl.w > 2.0 * wv { 0.0 } else { (fw - wv) / 2.0 };
            let py = sl.y + (fh - hv) / 2.0;
            match it.key {
                "detail" => set_place(
                    &mut place,
                    "detail",
                    Place { x: px, y: py, s, bx: [det_at[0] - det_r, det_at[1] - det_r, det_at[0] + det_r, det_at[1] + det_r] },
                ),
                "holes" => set_place(&mut place, "holes", Place { x: px, y: py, s: 1.0, bx: [0.0, 0.0, it.w, it.h] }),
                k => {
                    let bx = vget(k).map_or([f64::NAN; 4], |vv| vv.bx);
                    set_place(&mut place, k, Place { x: px, y: py, s, bx });
                }
            }
            if let Some(x) = used.get_mut(i) {
                *x += wv + gap;
            }
            ok = true;
        }
        if !ok {
            omitted.push(it.key.to_string());
        }
    }
    let to_sheet = |pl: &[(String, Place)], k: &str, u: f64, vv: f64| -> [f64; 2] {
        let p = pget(pl, k).unwrap_or(Place { x: f64::NAN, y: f64::NAN, s: f64::NAN, bx: [f64::NAN; 4] });
        [p.x + (u - p.bx[0]) * p.s, p.y + (vv - p.bx[1]) * p.s]
    };
    let put = |pl: &[(String, Place)], dst: &mut Vec<Seg>, k: &str, segs: &[Seg]| {
        for g in segs {
            let a = to_sheet(pl, k, g[0], g[1]);
            let b = to_sheet(pl, k, g[2], g[3]);
            dst.push([a[0], a[1], b[0], b[1]]);
        }
    };
    for k in ["front", "top", "right", "iso"] {
        let Some(p) = pget(&place, k) else { continue };
        let vw = vget(k).unwrap_or(&empty);
        put(&place, &mut st_vis, k, &vw.visible);
        if k != "iso" {
            put(&place, &mut st_hid, k, &vw.hidden);
        }
        let label = if p.s != scale {
            t("model.drawing.view_scaled", &[("view", F::S(view_label(k))), ("scale", F::S(scale_label(p.s)))])
        } else {
            view_label(k)
        };
        texts.push(Tx::new(p.x + vw.bw() * p.s / 2.0, p.y - if k == "front" { 24.0 } else { 9.0 }, 3.5, label, "middle"));
    }
    if let (Some(sv), Some(p)) = (sec, pget(&place, "section")) {
        let cut = sv.cut.clone().unwrap_or_default();
        put(&place, &mut st_vis, "section", &sv.visible);
        put(&place, &mut st_vis, "section", &cut);
        let hs = hatch(&cut, 2.5 / p.s, 45.0);
        put(&place, &mut st_thin, "section", &hs);
        let label = if p.s != scale {
            t("model.drawing.view_scaled", &[("view", F::S(t("model.drawing.section", &[]))), ("scale", F::S(scale_label(p.s)))])
        } else {
            t("model.drawing.section", &[])
        };
        texts.push(Tx::new(p.x + sv.bw() * p.s / 2.0, p.y - 9.0, 3.5, label, "middle"));
        if let Some(tr) = &sv.trace {
            let tview = js::str_of(tr.get("view"));
            if let Some(tp) = pget(&place, &tview) {
                let p2 = |k: &str| {
                    let a = jv::arr(tr.get(k));
                    [jv::as_num(a.first()).unwrap_or(f64::NAN), jv::as_num(a.get(1)).unwrap_or(f64::NAN)]
                };
                let tb = tr.get("box");
                let gb = |k: &str| jv::as_num(tb.and_then(|b| b.get(k))).unwrap_or(f64::NAN);
                let (bu0, bv0, bu1, bv1) = (gb("u0"), gb("v0"), gb("u1"), gb("v1"));
                let (d, trp, sight) = (p2("d"), p2("p"), p2("sight"));
                let l0 = js::hypot2(d[0], d[1]);
                let len = if l0 == 0.0 || l0.is_nan() { 1.0 } else { l0 };
                let u = [d[0] / len, d[1] / len];
                let ts: Vec<f64> = [(bu0, bv0), (bu1, bv0), (bu0, bv1), (bu1, bv1)]
                    .iter()
                    .map(|(x, y)| (x - trp[0]) * u[0] + (y - trp[1]) * u[1])
                    .collect();
                let ext = 6.0 / tp.s;
                let tmin = js::min_of(ts.iter().copied());
                let tmax = js::max_of(ts.iter().copied());
                let pa = [trp[0] + u[0] * (tmin - ext), trp[1] + u[1] * (tmin - ext)];
                let pb = [trp[0] + u[0] * (tmax + ext), trp[1] + u[1] * (tmax + ext)];
                let a = to_sheet(&place, &tview, pa[0], pa[1]);
                let bb = to_sheet(&place, &tview, pb[0], pb[1]);
                st_chain.push([a[0], a[1], bb[0], bb[1]]);
                let sl0 = js::hypot2(sight[0], sight[1]);
                let sl = if sl0 == 0.0 || sl0.is_nan() { 1.0 } else { sl0 };
                let sg = [sight[0] / sl, sight[1] / sl];
                let ll0 = js::hypot2(bb[0] - a[0], bb[1] - a[1]);
                let ll = if ll0 == 0.0 || ll0.is_nan() { 1.0 } else { ll0 };
                let uu = [(bb[0] - a[0]) / ll, (bb[1] - a[1]) / ll];
                for (e, dir_in) in [(a, 1.0), (bb, -1.0)] {
                    let thick = [e[0] + uu[0] * 5.0 * dir_in, e[1] + uu[1] * 5.0 * dir_in];
                    extra.push(jv::obj(vec![
                        ("width", jv::num(0.7)),
                        ("dash", Value::Null),
                        ("segs", Value::Array(vec![jv::nums(&[e[0], e[1], thick[0], thick[1]])])),
                    ]));
                    let tail = [e[0] - sg[0] * 7.0, e[1] - sg[1] * 7.0];
                    st_thin.push([tail[0], tail[1], e[0] - sg[0] * 0.1, e[1] - sg[1] * 0.1]);
                    arrow(&mut fills, e, sg);
                    texts.push(Tx::new(tail[0] - sg[0] * 3.0, tail[1] - sg[1] * 3.0 - 1.2, 3.5, "A", "middle").bold(true));
                }
            }
        }
    }
    if let (Some(d), Some(pd)) = (det, pget(&place, "detail")) {
        let of = js::str_of(d.get("of"));
        let parent = if of == "section" { pget(&place, "section").map(|_| "section".to_string()) } else { Some(of.clone()) };
        let src: Option<&View> = if of == "section" { sec } else { vget(&of) };
        match (parent.as_deref().and_then(|p| pget(&place, p).map(|pp| (p.to_string(), pp))), src) {
            (Some((parent, pp)), Some(src)) => {
                let c = to_sheet(&place, &parent, det_at[0], det_at[1]);
                st_thin.extend(circle_segs(c[0], c[1], det_r * pp.s));
                texts.push(Tx::new(c[0] + det_r * pp.s * 0.75 + 1.5, c[1] + det_r * pp.s * 0.75 + 1.5, 3.5, "B", "start").bold(true));
                let mut vis_src = src.visible.clone();
                if of == "section" {
                    vis_src.extend(src.cut.clone().unwrap_or_default());
                }
                put(&place, &mut st_vis, "detail", &clip_to_circle(&vis_src, det_at, det_r));
                put(&place, &mut st_hid, "detail", &clip_to_circle(&src.hidden, det_at, det_r));
                if of == "section" {
                    put(
                        &place,
                        &mut st_thin,
                        "detail",
                        &clip_to_circle(&hatch(&src.cut.clone().unwrap_or_default(), 2.5 / pd.s, 45.0), det_at, det_r),
                    );
                }
                let dc = to_sheet(&place, "detail", det_at[0], det_at[1]);
                st_thin.extend(circle_segs(dc[0], dc[1], det_r * pd.s));
                texts.push(Tx::new(
                    dc[0],
                    dc[1] - det_r * pd.s - 6.0,
                    3.5,
                    t("model.drawing.view_scaled", &[("view", F::S(t("model.drawing.detail", &[]))), ("scale", F::S(scale_label(pd.s)))]),
                    "middle",
                ));
            }
            _ => omitted.push("detail".into()),
        }
    }
    // overall dimensions
    let dim_h = |st: &mut Vec<Seg>, fills: &mut Vec<Value>, texts: &mut Vec<Tx>, xa: f64, xb: f64, y: f64, yref: f64, value: String| {
        st.push([xa, yref, xa, y - 2.0]);
        st.push([xb, yref, xb, y - 2.0]);
        st.push([xa, y, xb, y]);
        arrow(fills, [xa, y], [-1.0, 0.0]);
        arrow(fills, [xb, y], [1.0, 0.0]);
        texts.push(Tx::new((xa + xb) / 2.0, y + 1.2, 3.5, value, "middle"));
    };
    let dim_v = |st: &mut Vec<Seg>, fills: &mut Vec<Value>, texts: &mut Vec<Tx>, ya: f64, yb: f64, x: f64, xref: f64, value: String| {
        st.push([xref, ya, x - 2.0, ya]);
        st.push([xref, yb, x - 2.0, yb]);
        st.push([x, ya, x, yb]);
        arrow(fills, [x, ya], [0.0, -1.0]);
        arrow(fills, [x, yb], [0.0, 1.0]);
        let mut tx = Tx::new(x - 1.2, (ya + yb) / 2.0, 3.5, value, "middle");
        tx.rot = Some(90.0);
        texts.push(tx);
    };
    let pf = pget(&place, "front").unwrap_or(pf);
    let (fx0, fy0) = (pf.x, pf.y);
    let fx1 = fx0 + front.bw() * scale;
    let fy1 = fy0 + front.bh() * scale;
    dim_h(&mut st_thin, &mut fills, &mut texts, fx0, fx1, fy0 - 14.0, fy0 - 1.0, fmt_len(front.bw()));
    dim_v(&mut st_thin, &mut fills, &mut texts, fy0, fy1, fx0 - 8.0, fx0 - 1.0, fmt_len(front.bh()));
    let ptp = pget(&place, "top").unwrap_or(pt_);
    let (tx0, ty0) = (ptp.x, ptp.y);
    let ty1 = ty0 + topv.bh() * scale;
    dim_v(&mut st_thin, &mut fills, &mut texts, ty0, ty1, tx0 - 8.0, tx0 - 1.0, fmt_len(topv.bh()));
    // annotations
    let mut notes: Vec<Value> = Vec::new();
    let mut extra_texts: Vec<Value> = Vec::new();
    let mut text_order: Vec<(bool, usize)> = Vec::new(); // (from gdt, index)
    for tx_i in 0..texts.len() {
        text_order.push((false, tx_i));
    }
    for n in jv::arr(drawing.get("notes")) {
        let nv = js::str_of(n.get("view"));
        let k = if nv == "detail" { pget(&place, "detail").map(|_| "detail".to_string()) } else { pget(&place, &nv).map(|_| nv.clone()) };
        let Some(k) = k.filter(|_| o.annotate) else {
            notes.push(jv::obj(vec![("id", n.get("id").cloned().unwrap_or(Value::Null)), ("missing", true.into())]));
            continue;
        };
        let a = jv::arr(n.get("at"));
        let anchor = to_sheet(&place, &k, jv::as_num(a.first()).unwrap_or(f64::NAN), jv::as_num(a.get(1)).unwrap_or(f64::NAN));
        let g = gdt::annotation_geometry(n, anchor)?.to_json();
        for stv in jv::arr(g.get("strokes")) {
            let dash = stv.get("dash").filter(|d| jv::truthy(Some(d))).cloned().unwrap_or(Value::Null);
            extra.push(jv::obj(vec![
                ("width", stv.get("width").cloned().unwrap_or(Value::Null)),
                ("dash", dash),
                ("segs", stv.get("segs").cloned().unwrap_or(Value::Null)),
            ]));
        }
        fills.extend(jv::arr(g.get("fills")).iter().cloned());
        for tx in jv::arr(g.get("texts")) {
            text_order.push((true, extra_texts.len()));
            extra_texts.push(tx.clone());
        }
        notes.push(jv::obj(vec![
            ("id", n.get("id").cloned().unwrap_or(Value::Null)),
            ("box", g.get("box").cloned().unwrap_or(Value::Null)),
            ("anchor", jv::pt2(anchor)),
        ]));
    }
    let mut more: Vec<Tx> = Vec::new();
    // hole table
    let mut hole_table = Value::Null;
    if let (Some((rows, skipped)), Some(ph), Some(vk)) =
        (&ht, pget(&place, "holes"), ht_view.as_deref().filter(|k| pget(&place, k).is_some()))
    {
        let pk = pget(&place, vk).unwrap_or(ph);
        let b = vget(vk).map_or([f64::NAN; 4], |x| x.bx);
        for r in rows {
            let c = to_sheet(&place, vk, r.u, r.v);
            let off = r.dia / 2.0 * pk.s;
            more.push(Tx::new(c[0] + off * 0.75 + 1.0, c[1] + off * 0.75 + 1.0, 2.5, r.tag.clone(), "start").bold(true));
        }
        let og = to_sheet(&place, vk, b[0], b[1]);
        st_thin.push([og[0] - 3.0, og[1], og[0] + 3.0, og[1]]);
        st_thin.push([og[0], og[1] - 3.0, og[0], og[1] + 3.0]);
        more.push(Tx::new(og[0] - 1.5, og[1] - 4.5, 2.5, "X0 Y0", "end"));
        let tw: f64 = htc.iter().sum();
        let th = htr * (js::f(rows.len()) + 1.0);
        let top = ph.y + th;
        more.push(
            Tx::new(ph.x, top + 2.5, 3.0, t("drawing.holes.title", &[("view", F::S(t(&format!("drawing.view.{vk}"), &[])))]), "start")
                .bold(true),
        );
        st_thin.push([ph.x, top, ph.x + tw, top]);
        st_thin.push([ph.x, ph.y, ph.x + tw, ph.y]);
        st_thin.push([ph.x, ph.y, ph.x, top]);
        st_thin.push([ph.x + tw, ph.y, ph.x + tw, top]);
        let mut xx = ph.x;
        let head = [t("drawing.holes.tag", &[]), "X".to_string(), "Y".to_string(), t("drawing.holes.size", &[])];
        for (i, wc) in htc.iter().enumerate() {
            if i > 0 {
                st_thin.push([xx, ph.y, xx, top]);
            }
            more.push(Tx::new(xx + 1.2, top - htr + 1.4, 2.5, head.get(i).cloned().unwrap_or_default(), "start").bold(true));
            xx += wc;
        }
        for (i, r) in rows.iter().enumerate() {
            let y = top - htr * (js::f(i) + 2.0);
            st_thin.push([ph.x, y + htr, ph.x + tw, y + htr]);
            let cells = [r.tag.clone(), fmt_len(r.x), fmt_len(r.y), r.size.clone()];
            let mut x2 = ph.x;
            for (c, vtxt) in cells.iter().enumerate() {
                let wc = htc.get(c).copied().unwrap_or(0.0);
                more.push(Tx::new(x2 + 1.2, y + 1.4, 2.5, fit_text(vtxt, wc - 2.4, 2.5), "start"));
                x2 += wc;
            }
        }
        hole_table = jv::obj(vec![
            ("view", vk.into()),
            (
                "rows",
                Value::Array(
                    rows.iter()
                        .map(|r| {
                            jv::obj(vec![
                                ("tag", r.tag.as_str().into()),
                                ("x", jv::num(r.x)),
                                ("y", jv::num(r.y)),
                                ("size", r.size.as_str().into()),
                                ("dia", jv::num(r.dia)),
                                ("depth", r.depth.map_or(Value::Null, jv::num)),
                            ])
                        })
                        .collect(),
                ),
            ),
            ("skipped", jv::unum(*skipped)),
        ]);
    } else if drawing.get("holeTable").is_some_and(|x| jv::truthy(Some(x))) && o.holes.is_none() {
        omitted.push("holes".into());
    }
    // border, title block, revision block
    let rect = |st: &mut Vec<Seg>, x: f64, y: f64, ww: f64, hh: f64| {
        st.push([x, y, x + ww, y]);
        st.push([x + ww, y, x + ww, y + hh]);
        st.push([x + ww, y + hh, x, y + hh]);
        st.push([x, y + hh, x, y]);
    };
    rect(&mut st_border, margin, margin, w - 2.0 * margin, h - 2.0 * margin);
    let (bx, by) = (w - margin - tb_w, margin);
    rect(&mut st_border, bx, by, tb_w, tb_h);
    let tt = drawing.get("title").filter(|x| jv::truthy(Some(x))).cloned().unwrap_or(Value::Object(jv::Obj::new()));
    let tts = |k: &str| if jv::truthy(tt.get(k)) { js::string_of(tt.get(k)) } else { String::new() };
    let rev = if jv::truthy(tt.get("rev")) {
        js::string_of(tt.get("rev"))
    } else if let Some(last) = revs.last().filter(|r| jv::truthy(r.get("rev"))) {
        js::string_of(last.get("rev"))
    } else {
        String::new()
    };
    let title = if jv::truthy(tt.get("title")) { js::string_of(tt.get("title")) } else { o.name.clone() };
    let size_txt =
        if sheet_key.starts_with("ansi") { format!("ANSI {}", js::upper(sheet_key.get(5..).unwrap_or(""))) } else { js::upper(&sheet_key) };
    let date_txt = if jv::truthy(tt.get("date")) { js::string_of(tt.get("date")) } else { o.date_ymd.clone() };
    type Cell = (f64, Option<String>, String, f64, bool);
    let rows_tb: Vec<(f64, Vec<Cell>)> = vec![
        (
            14.0,
            vec![
                (120.0, Some(t("drawing.tb.title", &[])), title, 5.0, true),
                (60.0, Some(t("drawing.tb.company", &[])), tts("company"), 3.0, false),
            ],
        ),
        (
            9.0,
            vec![
                (60.0, Some(t("drawing.tb.number", &[])), tts("number"), 3.5, true),
                (20.0, Some(t("drawing.tb.rev", &[])), rev, 3.5, true),
                (100.0, Some(t("drawing.tb.material", &[])), o.material.clone(), 3.0, false),
            ],
        ),
        (
            9.0,
            vec![
                (45.0, Some(t("drawing.tb.drawn", &[])), tts("drawnBy"), 3.0, false),
                (45.0, Some(t("drawing.tb.checked", &[])), tts("checkedBy"), 3.0, false),
                (45.0, Some(t("drawing.tb.approved", &[])), tts("approvedBy"), 3.0, false),
                (45.0, Some(t("drawing.tb.date", &[])), date_txt, 3.0, false),
            ],
        ),
        (
            8.0,
            vec![
                (25.0, Some(t("drawing.tb.scale", &[])), scale_label(scale), 3.5, true),
                (30.0, Some(t("drawing.tb.size", &[])), size_txt, 3.5, false),
                (125.0, None, String::new(), 0.0, false),
            ],
        ),
    ];
    let mut ry = by + tb_h;
    let mut cells = jv::Obj::new();
    for (ri, (rh, cs)) in rows_tb.iter().enumerate() {
        ry -= rh;
        if ri > 0 {
            st_thin.push([bx, ry + rh, bx + tb_w, ry + rh]);
        }
        let mut cx = bx;
        for (ci, (cw, label, value, size, bold)) in cs.iter().enumerate() {
            if ci > 0 {
                st_thin.push([cx, ry, cx, ry + rh]);
            }
            if let Some(label) = label {
                more.push(Tx::new(cx + 1.5, ry + rh - 2.6, 2.0, label.clone(), "start"));
                if !value.is_empty() {
                    more.push(
                        Tx::new(cx + 1.5, ry + 1.6, *size, fit_text(value, (cw - 3.0) * if *bold { 0.9 } else { 1.0 }, *size), "start")
                            .bold(*bold),
                    );
                }
                cells
                    .insert(label.clone(), jv::obj(vec![("x", jv::num(cx)), ("y", jv::num(ry)), ("w", jv::num(*cw)), ("h", jv::num(*rh))]));
            }
            cx += cw;
        }
    }
    let nx = bx + 55.0 + 1.5;
    let units = format!(
        "{}{}",
        t("drawing.tb.units", &[]),
        if exact { format!(" {}", t("drawing.tb.exact", &[])) } else { format!(" {}", t("drawing.tb.facets", &[])) }
    );
    more.push(Tx::new(nx, by + 4.6, 2.0, fit_text(&units, 122.0, 2.0), "start"));
    let made = format!("{}{}", t("drawing.tb.made", &[]), if exact { format!(" {}", t("drawing.tb.occt", &[])) } else { String::new() });
    more.push(Tx::new(nx, by + 1.4, 2.0, fit_text(&made, 122.0, 2.0), "start"));
    let mut general: Vec<String> = Vec::new();
    if jv::truthy(tt.get("tolerance")) {
        general.push(t("drawing.tb.tolerance", &[("v", F::S(js::string_of(tt.get("tolerance"))))]));
    }
    if jv::truthy(tt.get("finish")) {
        general.push(t("drawing.tb.finish", &[("v", F::S(js::string_of(tt.get("finish"))))]));
    }
    let threads_y = by + tb_h + 4.0 + if general.is_empty() { 0.0 } else { 5.0 };
    if !general.is_empty() {
        more.push(Tx::new(bx, by + tb_h + 4.0, 3.0, fit_text(&general.join("   "), tb_w, 3.0), "start"));
    }
    for (i, th) in threads.iter().enumerate() {
        let cnt = jv::as_num(th.get("count")).unwrap_or(0.0);
        let pre = if cnt > 1.0 { format!("{}× ", js::num_str(cnt)) } else { String::new() };
        more.push(Tx::new(bx, threads_y + js::f(i) * 5.0, 3.5, format!("{pre}{}", js::string_of(th.get("callout"))), "start"));
    }
    if !revs.is_empty() {
        let rx = w - margin - rev_w;
        let top = h - margin;
        let cw = [12.0, rev_w - 12.0 - 22.0 - 18.0, 22.0, 18.0];
        rect(&mut st_border, rx, top - rev_h, rev_w, rev_h);
        let head = [t("drawing.rev.rev", &[]), t("drawing.rev.desc", &[]), t("drawing.rev.date", &[]), t("drawing.rev.by", &[])];
        st_thin.push([rx, top - 7.0, rx + rev_w, top - 7.0]);
        let mut x = rx;
        for (i, wc) in cw.iter().enumerate() {
            if i > 0 {
                st_thin.push([x, top - rev_h, x, top]);
            }
            more.push(Tx::new(x + 1.5, top - 5.0, 2.5, head.get(i).cloned().unwrap_or_default(), "start").bold(true));
            x += wc;
        }
        for (i, r) in revs.iter().enumerate() {
            let y = top - 7.0 - 5.0 * (js::f(i) + 1.0);
            if i > 0 {
                st_thin.push([rx, y + 5.0, rx + rev_w, y + 5.0]);
            }
            let mut xx = rx;
            for (k, key) in ["rev", "desc", "date", "by"].iter().enumerate() {
                let wc = cw.get(k).copied().unwrap_or(0.0);
                if jv::truthy(r.get(key)) {
                    more.push(Tx::new(xx + 1.5, y + 1.4, 2.5, fit_text(&js::string_of(r.get(key)), wc - 3.0, 2.5), "start"));
                }
                xx += wc;
            }
        }
    }
    // views out
    let mut views_out = jv::Obj::new();
    for (k, p) in &place {
        if k == "holes" {
            continue;
        }
        let vw = if k == "detail" { None } else { vget(k) };
        views_out.insert(
            k.clone(),
            jv::obj(vec![
                ("x", jv::num(p.x)),
                ("y", jv::num(p.y)),
                ("w", jv::num((p.bx[2] - p.bx[0]) * p.s)),
                ("h", jv::num((p.bx[3] - p.bx[1]) * p.s)),
                ("s", jv::num(p.s)),
                ("visible", jv::unum(vw.map_or(0, |x| x.visible.len()))),
                ("hidden", jv::unum(vw.map_or(0, |x| x.hidden.len()))),
            ]),
        );
    }
    let mut place_out = jv::Obj::new();
    for (k, p) in &place {
        place_out.insert(
            k.clone(),
            jv::obj(vec![
                ("x", jv::num(p.x)),
                ("y", jv::num(p.y)),
                ("s", jv::num(p.s)),
                (
                    "box",
                    jv::obj(vec![("u0", jv::num(p.bx[0])), ("v0", jv::num(p.bx[1])), ("u1", jv::num(p.bx[2])), ("v1", jv::num(p.bx[3]))]),
                ),
            ]),
        );
    }
    let segs = |s: &[Seg]| Value::Array(s.iter().map(|g| jv::nums(g)).collect());
    let stroke = |wd: f64, dash: Option<&[f64]>, s: &[Seg]| {
        jv::obj(vec![("width", jv::num(wd)), ("dash", dash.map_or(Value::Null, jv::nums)), ("segs", segs(s))])
    };
    let mut strokes = vec![
        stroke(0.5, None, &st_vis),
        stroke(0.25, Some(&[2.0, 1.0]), &st_hid),
        stroke(0.18, None, &st_thin),
        stroke(0.18, Some(&[8.0, 1.5, 1.5, 1.5]), &st_chain),
        stroke(0.7, None, &st_border),
    ];
    strokes.extend(extra);
    let mut texts_out: Vec<Value> = Vec::new();
    for (from_gdt, i) in &text_order {
        if *from_gdt {
            texts_out.push(extra_texts.get(*i).cloned().unwrap_or(Value::Null));
        } else if let Some(tx) = texts.get(*i) {
            texts_out.push(tx.to_json());
        }
    }
    texts_out.extend(more.iter().map(Tx::to_json));
    let iso_scale = pget(&place, "iso").map_or(Value::Null, |p| jv::num(p.s));
    Ok(jv::obj(vec![
        ("w", jv::num(w)),
        ("h", jv::num(h)),
        ("paper", sheet_key.as_str().into()),
        ("name", o.name.as_str().into()),
        ("scale", jv::num(scale)),
        ("titleCells", Value::Object(cells)),
        ("revisions", jv::unum(revs.len())),
        ("isoScale", iso_scale),
        ("strokes", Value::Array(strokes)),
        ("fills", Value::Array(fills)),
        ("texts", Value::Array(texts_out)),
        ("views", Value::Object(views_out)),
        ("place", Value::Object(place_out)),
        ("notes", Value::Array(notes)),
        ("omitted", jv::strs(&omitted)),
        ("exact", exact.into()),
        ("holeTable", hole_table),
    ]))
}

fn pnum(x: f64) -> String {
    if !x.is_finite() {
        return "0".into();
    }
    let s = js::to_fixed(if x.abs() < 5e-5 { 0.0 } else { x }, 4);
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

/// `winAnsi(s)` → (escaped literal, width, missing).
pub fn win_ansi(s: &str) -> (String, f64, usize) {
    let mut out = String::new();
    let mut width = 0.0;
    let mut missing = 0usize;
    for ch0 in s.chars() {
        let ch = font::subst(ch0);
        let mut code = u32::from(ch);
        if let Some(c) = font::cp1252(ch) {
            code = c;
        } else if code > 0xff || (0x7f..0xa0).contains(&code) || code < 0x20 {
            code = 0x3f;
            missing = missing.saturating_add(1);
            width += 556.0;
        }
        if code == 0x28 || code == 0x29 || code == 0x5c {
            out.push('\\');
            out.push(char::from_u32(code).unwrap_or('?'));
        } else if (0x20..0x7f).contains(&code) {
            out.push(char::from_u32(code).unwrap_or('?'));
        } else {
            out.push_str(&format!("\\{code:03o}"));
        }
        if code != 0x3f || ch == '?' {
            width += font::char_advance(ch);
        }
    }
    (out, width, missing)
}

/// The PDF content stream of a sheet (before compression) and the count of characters with no glyph.
pub fn pdf_content(sheet: &Value) -> (String, usize) {
    let mut ops: Vec<String> = Vec::new();
    let k = js::to_fixed(72.0 / 25.4, 9);
    ops.push(format!("{k} 0 0 {k} 0 0 cm"));
    ops.push("0 0 0 RG 0 0 0 rg 1 J 1 j".into());
    let nn = |x: Option<&Value>| jv::as_num(x).unwrap_or(f64::NAN);
    for s in jv::arr(sheet.get("strokes")) {
        let segs = jv::arr(s.get("segs"));
        if segs.is_empty() {
            continue;
        }
        let dash = match s.get("dash").filter(|d| jv::truthy(Some(d))) {
            Some(d) => format!("[{}] 0 d", jv::arr(Some(d)).iter().map(|x| pnum(nn(Some(x)))).collect::<Vec<_>>().join(" ")),
            None => "[] 0 d".into(),
        };
        ops.push(format!("{} w {dash}", pnum(nn(s.get("width")))));
        for g in segs {
            let a = jv::arr(Some(g));
            ops.push(format!("{} {} m {} {} l", pnum(nn(a.first())), pnum(nn(a.get(1))), pnum(nn(a.get(2))), pnum(nn(a.get(3)))));
        }
        ops.push("S".into());
    }
    for f in jv::arr(sheet.get("fills")) {
        let parts: Vec<String> = jv::arr(f.get("pts"))
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let a = jv::arr(Some(p));
                format!("{} {}{}", pnum(nn(a.first())), pnum(nn(a.get(1))), if i > 0 { " l" } else { " m" })
            })
            .collect();
        ops.push(format!("{} h f", parts.join(" ")));
    }
    let mut missing = 0usize;
    for tx in jv::arr(sheet.get("texts")) {
        let (str_, width, miss) = win_ansi(&js::string_of(tx.get("text")));
        missing = missing.saturating_add(miss);
        let size = nn(tx.get("size"));
        let w = width / 1000.0 * size;
        let dx = match jv::s(tx, "anchor") {
            "middle" => -w / 2.0,
            "end" => -w,
            _ => 0.0,
        };
        let r = jv::as_num(tx.get("rot")).filter(|x| *x != 0.0 && !x.is_nan()).unwrap_or(0.0) * std::f64::consts::PI / 180.0;
        let (c, sn) = (js::cos(r), js::sin(r));
        let x = nn(tx.get("x")) + dx * c;
        let y = nn(tx.get("y")) + dx * sn;
        let font = if jv::truthy(tx.get("bold")) { "F2" } else { "F1" };
        let m = [size * c, size * sn, -size * sn, size * c, x, y].iter().map(|q| pnum(*q)).collect::<Vec<_>>().join(" ");
        ops.push(format!("BT /{font} 1 Tf {m} Tm ({str_}) Tj ET"));
    }
    (format!("{}\n", ops.join("\n")), missing)
}

/// `toPdf(sheet)` → (bytes, missing); `date` = the PDF date string ("D:YYYYMMDDHHMMSSZ").
pub fn to_pdf(sheet: &Value, date: &str) -> (Vec<u8>, usize) {
    let (content, missing) = pdf_content(sheet);
    let packed = miniz_oxide::deflate::compress_to_vec_zlib(content.as_bytes(), 6);
    let w = jv::as_num(sheet.get("w")).unwrap_or(f64::NAN);
    let h = jv::as_num(sheet.get("h")).unwrap_or(f64::NAN);
    (assemble(w, h, &packed, true, &js::string_of(sheet.get("name")), date), missing)
}

fn assemble(w_mm: f64, h_mm: f64, content: &[u8], flate: bool, title: &str, date: &str) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut offsets: Vec<usize> = vec![0; 8];
    let pt = |mm: f64| pnum(mm * 72.0 / 25.4);
    out.extend_from_slice(b"%PDF-1.4\n");
    out.extend_from_slice(&[0x25, 0xe2, 0xe3, 0xcf, 0xd3, 0x0a]);
    let mut obj = |out: &mut Vec<u8>, n: usize, body: &[&[u8]]| {
        if let Some(o) = offsets.get_mut(n) {
            *o = out.len();
        }
        out.extend_from_slice(format!("{n} 0 obj\n").as_bytes());
        for b in body {
            out.extend_from_slice(b);
        }
        out.extend_from_slice(b"\nendobj\n");
    };
    obj(&mut out, 1, &[b"<< /Type /Catalog /Pages 2 0 R >>"]);
    obj(&mut out, 2, &[b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"]);
    let page = format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 5 0 R /F2 7 0 R >> >> /Contents 4 0 R >>",
        pt(w_mm),
        pt(h_mm)
    );
    obj(&mut out, 3, &[page.as_bytes()]);
    let head = format!("<< /Length {}{} >>\nstream\n", content.len(), if flate { " /Filter /FlateDecode" } else { "" });
    obj(&mut out, 4, &[head.as_bytes(), content, b"\nendstream"]);
    obj(&mut out, 5, &[b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"]);
    let info = format!("<< /Title ({}) /Producer (PartCraft) /Creator (PartCraft) /CreationDate ({date}) >>", win_ansi(title).0);
    obj(&mut out, 6, &[info.as_bytes()]);
    obj(&mut out, 7, &[b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>"]);
    let xref = out.len();
    let mut tail = String::from("xref\n0 8\n0000000000 65535 f \n");
    for o in offsets.iter().skip(1) {
        tail.push_str(&format!("{o:010} 00000 n \n"));
    }
    tail.push_str(&format!("trailer\n<< /Size 8 /Root 1 0 R /Info 6 0 R >>\nstartxref\n{xref}\n%%EOF\n"));
    out.extend_from_slice(tail.as_bytes());
    out
}
