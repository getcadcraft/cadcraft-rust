//! model3d.js and shapes3d.js on ModelCraft's kernel (cc-m3d): the drawing's 3D model (`doc.model3d`), meters, Z up.
//!
//! The drawing keeps the model as its JSON snapshot (`Doc::m3d`, undoable through `Op::M3d`); before anything made one
//! it is what the file brought ([`from_file`]: CadCraft's own JSON from block CADCRAFT_3D, else the drawing's 3D
//! faces as loose faces in one group). [`for_file3d`] is what a file writes for it: every face as a 3DFACE (convex
//! triangles and quads as themselves, others as triangles with their inner edges invisible) and the exact JSON in
//! CADCRAFT xdata strings. [`outline2d`] and the `make_*` functions are the solids EXTRUDE, BOX, CYLINDER, WEDGE,
//! REVOLVE and SWEEP make.
//!
//! Hostile input: a file's model JSON goes through the kernel's own `load` (it checks everything); loose faces are
//! capped at 200,000 and every coordinate is checked (finite, under 1e7 m).

use cc_cad::Doc;
use cc_cad::entity::{Entity, Geo};
use cc_cad::js;
use cc_cad_geom::geom::{Prim, prims};
use cc_cad_geom::jsmath::JsMath;
use cc_m3d::model::Model;
use cc_m3d::vec::{M4, P3, apply, ident, translation};
use serde_json::{Value, json};

fn sgn(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        x
    }
}
/// model3d.js `convexQuad(P)`
pub fn convex_quad(p: &[P3]) -> bool {
    let g = |i: usize| p.get(i & 3).copied().unwrap_or([f64::NAN; 3]);
    let mut n = [0.0; 3];
    for i in 0..4 {
        let (a, b) = (g(i), g(i.wrapping_add(1)));
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let mut sign = 0.0;
    for i in 0..4 {
        let (a, b, c) = (g(i), g(i.wrapping_add(1)), g(i.wrapping_add(2)));
        let u = sub3(b, a);
        let v = sub3(c, b);
        let x = cross3(u, v);
        let s = sgn(x[0] * n[0] + x[1] * n[1] + x[2] * n[2]);
        if s == 0.0 || s.is_nan() {
            return false;
        }
        if sign != 0.0 && s != sign {
            return false;
        }
        sign = s;
    }
    true
}

/// model3d.js UNIT_M / mPerUnit
pub fn m_per_unit(units: &str) -> f64 {
    match units {
        "" | "millimeters" => 0.001,
        "inches" => 0.0254,
        "feet" => 0.3048,
        "miles" => 1609.344,
        "centimeters" => 0.01,
        "meters" => 1.0,
        "kilometers" => 1000.0,
        "microinches" => 2.54e-8,
        "mils" => 2.54e-5,
        "yards" => 0.9144,
        _ => 0.001,
    }
}
pub fn display_unit(units: &str) -> &'static str {
    match units {
        "inches" | "mils" | "microinches" => "in",
        "feet" | "yards" | "miles" => "ft-in",
        "centimeters" => "cm",
        "meters" | "kilometers" => "m",
        _ => "mm",
    }
}

/// One 3DFACE: four corners and the invisible-edges flags.
pub struct Face {
    pub pts: [P3; 4],
    pub invis: u32,
}
/// `forFile3d(doc)`: { layer, faces, points (CADCRAFT xdata strings per POINT) }.
pub struct File3d {
    pub layer: &'static str,
    pub faces: Vec<Face>,
    pub points: Vec<Vec<String>>,
}

const TAU: f64 = std::f64::consts::PI * 2.0;
/// ModelCraft's siteEdges and pulled circles alike: 7.5° steps, soft side edges.
const PER_TURN: usize = 48;
const MAX_LOOSE: usize = 200_000;

fn units_of(doc: &Doc) -> String {
    doc.meta.get("units").filter(|v| js::truthy(Some(v))).map(js::to_js_string).unwrap_or_default()
}

/// `m.k`: meters per drawing unit.
pub fn k_of(doc: &Doc) -> f64 {
    m_per_unit(&units_of(doc))
}

/// `Model3D.empty`: nothing at the root and no guides.
pub fn is_empty(m: &Model) -> bool {
    m.root().is_none_or(|r| r.faces.is_empty() && r.edges.is_empty() && r.insts.is_empty()) && m.guides.is_empty()
}

/// `Model3D.fromFile(in3d, units)`: the model a file brought (its notes for the report come with the read, as the
/// readers make them).
pub fn from_file(doc: &Doc, faces_name: &str) -> Model {
    let units = units_of(doc);
    let mut m = Model::new();
    m.units = display_unit(&units).to_string();
    let k = m_per_unit(&units);
    let mut restored = false;
    if let Some(text) = doc.model3d_text.as_deref().filter(|t| !t.is_empty()) {
        // the kernel's own load checks everything (the file is untrusted); one that fails leaves the loose faces
        match serde_json::from_str::<Value>(text) {
            Ok(j) if m.load(&j, false).is_ok() => restored = true,
            _ => m = Model::new(),
        }
    }
    m.units = display_unit(&units).to_string();
    let loose: Vec<&Value> = doc
        .faces3d
        .iter()
        .filter(|f| !restored || f.get("layer").map(js::to_js_string).unwrap_or_default().to_uppercase() != crate::model3d::M3D_LAYER)
        .collect();
    if !loose.is_empty() {
        let mut polys: Vec<Value> = Vec::new();
        for f in loose.iter().take(MAX_LOOSE) {
            let mut pts: Vec<P3> = Vec::new();
            for p in f.get("pts").and_then(Value::as_array).map_or(&[][..], Vec::as_slice) {
                let a = p.as_array();
                let ok =
                    a.is_some_and(|a| a.len() >= 3 && a.iter().all(|x| x.as_f64().is_some_and(|x| x.is_finite() && (x * k).abs() < 1e7)));
                let Some(a) = a.filter(|_| ok) else {
                    pts.clear();
                    break;
                };
                let n = |i: usize| a.get(i).and_then(Value::as_f64).unwrap_or(f64::NAN);
                let q = [n(0) * k, n(1) * k, n(2) * k];
                if pts.last().is_none_or(|l| l.iter().zip(q.iter()).any(|(x, y)| (x - y).abs() > 1e-9)) {
                    pts.push(q);
                }
            }
            if pts.len() > 1
                && let (Some(f0), Some(l)) = (pts.first(), pts.last())
                && f0.iter().zip(l.iter()).all(|(x, y)| (x - y).abs() <= 1e-9)
            {
                pts.pop();
            }
            if pts.len() >= 3 {
                polys.push(Value::Array(pts.iter().map(|p| json!([p[0], p[1], p[2]])).collect()));
            }
        }
        if !polys.is_empty() {
            let id = cc_m3d::mesh::mesh_def(&mut m, &polys, Some(faces_name), "group", &[]);
            if let Some(root) = m.root_mut() {
                cc_m3d::model::place(root, &id, &ident());
            }
        }
    }
    m
}

/// The drawing's model: its snapshot, else what the file brought.
pub fn model_of(doc: &Doc, faces_name: &str) -> Model {
    if let Some(s) = &doc.m3d {
        let mut m = Model::new();
        if m.restore(&cc_m3d::model::SnapRef::Str(std::rc::Rc::from(&**s))).is_ok() {
            return m;
        }
    }
    from_file(doc, faces_name)
}

/// `forFile3d(doc)`: what a file needs for the 3D model, or None when there is none.
pub fn for_file3d(m: &mut Model, units: &str, view: Option<&Value>) -> Option<File3d> {
    if is_empty(m) {
        return None;
    }
    let k = m_per_unit(units);
    let mut faces: Vec<Face> = Vec::new();
    let mut cb = |d: &cc_m3d::def::Def, wm: &M4, _: &[f64], _: Option<&str>| -> bool {
        let w = |v: f64| {
            let p = apply(wm, d.verts.get(v).copied().unwrap_or([f64::NAN; 3]));
            [p[0] / k, p[1] / k, p[2] / k]
        };
        for f in d.faces.values() {
            let Some(l0) = f.loops.first() else { continue };
            let quad =
                l0.len() == 4 && convex_quad(&l0.iter().map(|v| d.verts.get(*v).copied().unwrap_or([f64::NAN; 3])).collect::<Vec<_>>());
            if f.loops.len() == 1 && (l0.len() == 3 || quad) {
                let p: Vec<P3> = l0.iter().map(|v| w(*v)).collect();
                let g = |i: usize| p.get(i).copied().unwrap_or([f64::NAN; 3]);
                let fourth = if p.len() == 3 { g(2) } else { g(3) };
                faces.push(Face { pts: [g(0), g(1), g(2), fourth], invis: 0 });
                continue;
            }
            let mut along: std::collections::HashSet<(u64, u64)> = std::collections::HashSet::new();
            let key = |x: f64| x.to_bits();
            for l in &f.loops {
                let n = l.len();
                for i in 0..n {
                    if let (Some(a), Some(b)) = (l.get(i), l.get(nxt(i, n))) {
                        along.insert((key(*a), key(*b)));
                        along.insert((key(*b), key(*a)));
                    }
                }
            }
            for [a, b, c] in cc_m3d::def::face_tris(d, f) {
                let vis = |p: f64, q: f64| along.contains(&(key(p), key(q)));
                let invis = (if vis(a, b) { 0 } else { 1 }) | (if vis(b, c) { 0 } else { 2 }) | (if vis(c, a) { 0 } else { 8 });
                let cc = w(c);
                faces.push(Face { pts: [w(a), w(b), cc, cc], invis });
            }
        }
        true
    };
    // CadCraft 0.5's copy of the kernel: every face through earcut (no triangle / quad shortcut)
    cc_m3d::def::with_plain_face_tris(|| m.walk("root", &ident(), &mut cb));
    let raw = m.to_json_string(false);
    let mut text = String::with_capacity(raw.len());
    for u in raw.encode_utf16() {
        if (0x21..=0x7e).contains(&u) {
            text.push(char::from(u8::try_from(u).unwrap_or(b'?')));
        } else {
            text.push_str(&format!("\\u{u:04x}"));
        }
    }
    let mut points = Vec::new();
    let n = text.len();
    let mut i = 0usize;
    while i < n {
        let piece = text.get(i..i.saturating_add(230).min(n)).unwrap_or("");
        points.push(vec![format!("CC3D:{}:{piece}", points.len().saturating_add(1))]);
        i = i.saturating_add(230);
    }
    // the 3D view the file brought (0.4 C14; no 3D tab here keeps a newer one)
    if let Some(v) = view.and_then(|v| crate::model3d::clean_view(Some(&crate::jsv::from_json(v)))) {
        points.push(vec![format!("CCCAM:{}", crate::props::stringify(&v))]);
    }
    Some(File3d { layer: crate::model3d::M3D_LAYER, faces, points })
}

/// The file form of a drawing's model (`forFile3d(doc)`), from its snapshot or what the file brought.
pub fn doc_for_file3d(doc: &Doc, faces_name: &str) -> Option<File3d> {
    let mut m = model_of(doc, faces_name);
    for_file3d(&mut m, &units_of(doc), doc.view3d.as_ref())
}

// ---------------------------------------------------------------------------------------------------------------
// 2D outlines and solids

/// Why a 2D object is no outline (the message keys model3d.js prints).
pub enum NotClosed {
    Key(&'static str),
    What(&'static str),
}

/// `outline2d(e)`: a closed 2D object's boundary as a ring of [x, y] (drawing units).
pub fn outline2d(e: &Entity) -> Result<Vec<[f64; 2]>, NotClosed> {
    let mut ring: Vec<[f64; 2]> = Vec::new();
    let push = |ring: &mut Vec<[f64; 2]>, x: f64, y: f64| {
        if ring.last().is_none_or(|l| js::hypot(l[0] - x, l[1] - y) > 1e-9) {
            ring.push([x, y]);
        }
    };
    match &e.geo {
        Geo::Circle(c) => {
            for i in 0..PER_TURN {
                let a = (i as f64 / PER_TURN as f64) * TAU;
                push(&mut ring, c.cx + c.r * a.jcos(), c.cy + c.r * a.jsin());
            }
        }
        Geo::Ellipse(el) => {
            let (t0, t1) = (el.t0, el.t1);
            if !((t1 - t0).abs() >= TAU - 1e-6 || (t0 == 0.0 && t1 == 0.0)) {
                return Err(NotClosed::Key("model3d.elliptical_arc_not_closed"));
            }
            let mut full = e.clone();
            if let Geo::Ellipse(f) = &mut full.geo {
                f.t0 = 0.0;
                f.t1 = TAU;
            }
            for p in cc_cad_geom::geom::ellipse_points(&full, 96.0) {
                push(&mut ring, p.x, p.y);
            }
        }
        Geo::Poly(pl) => {
            let pts = &pl.pts;
            let closed = pl.is_closed()
                || (pts.len() > 2
                    && match (pts.first(), pts.last()) {
                        (Some(a), Some(b)) => js::hypot(a.x - b.x, a.y - b.y) < 1e-9,
                        _ => false,
                    });
            if !closed {
                return Err(NotClosed::Key("model3d.polyline_open_close_c"));
            }
            let mut cl = e.clone();
            if let Geo::Poly(p) = &mut cl.geo {
                p.closed = Some(true);
            }
            for p in prims(&cl, None) {
                if p.is_l() {
                    push(&mut ring, p.x1, p.y1);
                    push(&mut ring, p.x2, p.y2);
                    continue;
                }
                arc_points(&mut ring, &p, &push);
            }
        }
        Geo::Hatch(h) if h.loops.len() == 1 => {
            if let Some(l) = h.loops.first() {
                for c in l.as_chunks::<2>().0 {
                    push(&mut ring, c[0], c[1]);
                }
            }
        }
        _ => {
            let what = match e.typ() {
                "line" => "model3d.a_line",
                "arc" => "model3d.an_arc",
                "text" => "model3d.a_text",
                "point" => "model3d.a_point",
                "insert" => "model3d.block_reference",
                "dim" => "model3d.a_dimension",
                "viewport" => "model3d.a_viewport",
                _ => "model3d.this_object",
            };
            return Err(NotClosed::What(what));
        }
    }
    if ring.len() > 1
        && let (Some(a), Some(b)) = (ring.first(), ring.last())
        && js::hypot(a[0] - b[0], a[1] - b[1]) < 1e-9
    {
        ring.pop();
    }
    if ring.len() < 3 {
        return Err(NotClosed::Key("model3d.shape_has_no_area"));
    }
    let mut a = 0.0;
    for i in 0..ring.len() {
        let (p, q) = (ring.get(i).copied().unwrap_or([0.0; 2]), ring.get(nxt(i, ring.len())).copied().unwrap_or([0.0; 2]));
        a += p[0] * q[1] - q[0] * p[1];
    }
    if a.abs() < 1e-12 {
        return Err(NotClosed::Key("model3d.shape_has_no_area"));
    }
    Ok(ring)
}

/// An arc segment of a polyline, followed in the polyline's direction (`outline2d`).
fn arc_points(ring: &mut Vec<[f64; 2]>, p: &Prim, push: &Push<'_>) {
    let sweep = p.a1 - p.a0;
    let nf = js::max(2.0, (sweep.abs() / (TAU / PER_TURN as f64)).ceil());
    let n = if nf.is_finite() { nf.min(1.0e6) } else { 2.0 };
    let s = [p.cx + p.r * p.a0.jcos(), p.cy + p.r * p.a0.jsin()];
    let fwd = ring.last().is_none_or(|l| js::hypot(l[0] - s[0], l[1] - s[1]) < 1e-6 * js::max(1.0, p.r));
    let mut i = 0.0;
    while i <= n {
        let a = if fwd { p.a0 + (sweep * i) / n } else { p.a1 - (sweep * i) / n };
        push(ring, p.cx + p.r * a.jcos(), p.cy + p.r * a.jsin());
        i += 1.0;
    }
}

/// `t(key, fills)`: the message of a failed shape (shapes3d.js throws it).
pub type Tr<'a> = dyn Fn(&str, &[(&str, String)]) -> String + 'a;
type Push<'a> = dyn Fn(&mut Vec<[f64; 2]>, f64, f64) + 'a;

/// The next / previous index round a ring of n.
fn nxt(i: usize, n: usize) -> usize {
    i.saturating_add(1).checked_rem(n).unwrap_or(0)
}
fn prv(i: usize, n: usize) -> usize {
    if i == 0 { n.saturating_sub(1) } else { i.saturating_sub(1) }
}

/// A plain message (`throw new Error(t(key))`).
pub type Fail = String;

fn min_xy(ring: &[[f64; 2]]) -> (f64, f64) {
    let (mut ox, mut oy) = (f64::INFINITY, f64::INFINITY);
    for [x, y] in ring {
        ox = js::min(ox, *x);
        oy = js::min(oy, *y);
    }
    (ox, oy)
}

/// `makePrism(model, ring, z0, h, name)` (meters) → instance id.
pub fn make_prism(m: &mut Model, ring: &[[f64; 2]], z0: f64, h: f64, name: &str, t: &dyn Fn(&str) -> String) -> Result<f64, Fail> {
    if !(h.abs() > 1e-6) {
        return Err(t("model3d.height_must_not_zero"));
    }
    let (ox, oy) = min_xy(ring);
    let nm = m.unique_name(name);
    let id = m.new_def("group", Some(&nm));
    let pts: Vec<P3> = ring.iter().map(|[x, y]| [x - ox, y - oy, 0.0]).collect();
    let fids: Vec<f64> = match m.defs.get_mut(&id) {
        Some(d) => {
            let _ = cc_m3d::ops::add_polygon(d, &pts, Some([0.0, 0.0, 1.0]));
            d.faces.keys().collect()
        }
        None => Vec::new(),
    };
    let Some(&fid) = fids.first().filter(|_| fids.len() == 1) else {
        m.defs.delete(&id);
        return Err(t(if fids.is_empty() { "model3d.outline_has_no_area" } else { "model3d.outline_crosses_itself_draw" }));
    };
    let ok = m.defs.get_mut(&id).is_some_and(|d| cc_m3d::ops::push_pull(d, fid, h).ok);
    if !ok {
        m.defs.delete(&id);
        return Err(t("model3d.outline_could_not_extruded"));
    }
    Ok(place_root(m, &id, &translation([ox, oy, z0])))
}

fn place_root(m: &mut Model, id: &str, mm: &M4) -> f64 {
    match m.root_mut() {
        Some(r) => cc_m3d::model::place(r, id, mm),
        None => f64::NAN,
    }
}

/// `makeBox(model, a, b, z0, h)`
pub fn make_box(m: &mut Model, a: [f64; 2], b: [f64; 2], z0: f64, h: f64, t: &dyn Fn(&str) -> String) -> Result<f64, Fail> {
    let (x0, x1, y0, y1) = (js::min(a[0], b[0]), js::max(a[0], b[0]), js::min(a[1], b[1]), js::max(a[1], b[1]));
    if !(x1 - x0 > 1e-6 && y1 - y0 > 1e-6) {
        return Err(t("model3d.base_has_no_area"));
    }
    make_prism(m, &[[x0, y0], [x1, y0], [x1, y1], [x0, y1]], z0, h, "Box", t)
}

/// `makeCylinder(model, c, r, z0, h)` (48 sides)
pub fn make_cylinder(m: &mut Model, c: [f64; 2], r: f64, z0: f64, h: f64, t: &dyn Fn(&str) -> String) -> Result<f64, Fail> {
    if !(r > 1e-6) {
        return Err(t("model3d.radius_must_more_than"));
    }
    let ring: Vec<[f64; 2]> = cc_m3d::ops::ngon_pts([c[0], c[1], 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], r, PER_TURN as f64)
        .iter()
        .map(|p| [p[0], p[1]])
        .collect();
    make_prism(m, &ring, z0, h, "Cylinder", t)
}

/// `makeWedge(model, a, b, z0, h)`: full height at a's X sloping to nothing at b's X.
pub fn make_wedge(m: &mut Model, a: [f64; 2], b: [f64; 2], z0: f64, h: f64, t: &dyn Fn(&str) -> String) -> Result<f64, Fail> {
    if !((b[0] - a[0]).abs() > 1e-6 && (b[1] - a[1]).abs() > 1e-6) {
        return Err(t("model3d.base_has_no_area"));
    }
    if !(h.abs() > 1e-6) {
        return Err(t("model3d.height_must_not_zero"));
    }
    let (ox, oy) = (js::min(a[0], b[0]), js::min(a[1], b[1]));
    let nm = m.unique_name("Wedge");
    let id = m.new_def("group", Some(&nm));
    let (ax, bx, ay) = (a[0] - ox, b[0] - ox, a[1] - oy);
    let ok = match m.defs.get_mut(&id) {
        Some(d) => {
            let _ = cc_m3d::ops::add_polygon(d, &[[ax, ay, 0.0], [bx, ay, 0.0], [ax, ay, h]], Some([0.0, -1.0, 0.0]));
            let first = d.faces.keys().next();
            match first {
                Some(fid) => {
                    let n = d.faces.get(fid).map_or([0.0; 3], |f| cc_m3d::def::face_normal(d, f));
                    let r = cc_m3d::ops::push_pull(d, fid, (b[1] - a[1]) * n[1]);
                    r.ok && cc_m3d::solids::solid_info(d).closed
                }
                None => false,
            }
        }
        None => false,
    };
    if !ok {
        m.defs.delete(&id);
        return Err(t("model3d.wedge_could_not_made"));
    }
    Ok(place_root(m, &id, &translation([ox, oy, z0])))
}

/// `solidsOf(model)` volumes of the root's instances (m³; None when not closed), in placement order.
pub fn solid_volumes(m: &Model) -> Vec<(f64, Option<f64>)> {
    let mut out = Vec::new();
    let Some(root) = m.root() else { return out };
    for (iid, inst) in root.insts.iter() {
        let Some(d) = m.def(&inst.def) else { continue };
        // CadCraft 0.5's copy of the kernel: every face through earcut (the volume's sum follows its triangles)
        let info = cc_m3d::def::with_plain_face_tris(|| cc_m3d::solids::solid_info(d));
        let mm = &inst.m;
        let g = |i: usize| mm.get(i).copied().unwrap_or(f64::NAN);
        let det = (g(0) * (g(5) * g(10) - g(9) * g(6)) - g(4) * (g(1) * g(10) - g(9) * g(2)) + g(8) * (g(1) * g(6) - g(5) * g(2))).abs();
        out.push((iid, if info.closed { Some(info.volume.unwrap_or(f64::NAN).abs() * det) } else { None }));
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------
// shapes3d.js

fn area2(r: &[[f64; 2]]) -> f64 {
    let mut a = 0.0;
    for i in 0..r.len() {
        let p = r.get(i).copied().unwrap_or([0.0; 2]);
        let q = r.get(nxt(i, r.len())).copied().unwrap_or([0.0; 2]);
        a += p[0] * q[1] - q[0] * p[1];
    }
    a / 2.0
}
fn ccw(r: &[[f64; 2]]) -> Vec<[f64; 2]> {
    if area2(r) < 0.0 { r.iter().rev().copied().collect() } else { r.to_vec() }
}
fn norm2(v: [f64; 2]) -> [f64; 2] {
    let h = js::hypot(v[0], v[1]);
    let l = if h == 0.0 || h.is_nan() { 1.0 } else { h };
    [v[0] / l, v[1] / l]
}
fn sub3(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross3(a: P3, b: P3) -> P3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot3(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn at2(r: &[[f64; 2]], i: usize) -> [f64; 2] {
    r.get(i).copied().unwrap_or([f64::NAN; 2])
}

/// `insetRing(ring, d)`: the ring moved inward by d (corners mitered), or None when an edge would turn round.
pub fn inset_ring(ring: &[[f64; 2]], d: f64) -> Option<Vec<[f64; 2]>> {
    let r = ccw(ring);
    let n = r.len();
    if n < 3 {
        return None;
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let (p, q, s) = (at2(&r, prv(i, n)), at2(&r, i), at2(&r, nxt(i, n)));
        let e1 = norm2([q[0] - p[0], q[1] - p[1]]);
        let e2 = norm2([s[0] - q[0], s[1] - q[1]]);
        let (n1, n2) = ([-e1[1], e1[0]], [-e2[1], e2[0]]);
        let bis = norm2([n1[0] + n2[0], n1[1] + n2[1]]);
        let c = bis[0] * n1[0] + bis[1] * n1[1];
        if !(c > 1e-6) {
            return None;
        }
        out.push([q[0] + bis[0] * d / c, q[1] + bis[1] * d / c]);
    }
    for i in 0..n {
        let (a, b, aa, bb) = (at2(&r, i), at2(&r, nxt(i, n)), at2(&out, i), at2(&out, nxt(i, n)));
        if (b[0] - a[0]) * (bb[0] - aa[0]) + (b[1] - a[1]) * (bb[1] - aa[1]) <= 0.0 {
            return None;
        }
    }
    (area2(&out) > 1e-12).then_some(out)
}

/// shapes3d.js `place`: polygons → a closed solid at `origin`, its faces turned outward.
fn place_polys(m: &mut Model, polys: Vec<Vec<P3>>, origin: P3, name: &str, what: &str, t: &Tr<'_>) -> Result<f64, Fail> {
    let mut v = 0.0;
    for p in &polys {
        let mut i = 1usize;
        while i.saturating_add(1) < p.len() {
            let (a, b, c) = (
                p.first().copied().unwrap_or([0.0; 3]),
                p.get(i).copied().unwrap_or([0.0; 3]),
                p.get(i.saturating_add(1)).copied().unwrap_or([0.0; 3]),
            );
            v += dot3(a, cross3(b, c)) / 6.0;
            i = i.saturating_add(1);
        }
    }
    if v.abs() < 1e-18 {
        return Err(t("cmd.shape3d.flat", &[("what", what.to_string())]));
    }
    let faces: Vec<Value> = polys
        .iter()
        .map(|p| {
            let it: Vec<&P3> = if v < 0.0 { p.iter().rev().collect() } else { p.iter().collect() };
            Value::Array(it.iter().map(|q| json!([q[0], q[1], q[2]])).collect())
        })
        .collect();
    let nm = m.unique_name(name);
    let id = cc_m3d::mesh::mesh_def(m, &faces, Some(&nm), "group", &[]);
    let closed = m.def(&id).is_some_and(|d| cc_m3d::solids::solid_info(d).closed);
    if !closed {
        m.defs.delete(&id);
        return Err(t("cmd.shape3d.open", &[("what", what.to_string())]));
    }
    Ok(place_root(m, &id, &translation(origin)))
}

/// `makeTapered(model, ring, z0, h, angle)` → instance id.
pub fn make_tapered(m: &mut Model, ring: &[[f64; 2]], z0: f64, h: f64, angle: f64, t: &Tr<'_>) -> Result<f64, Fail> {
    if !(h.abs() > 1e-6) {
        return Err(t("cmd.shape3d.height", &[]));
    }
    if !(angle.abs() < std::f64::consts::FRAC_PI_2 - 1e-3) {
        return Err(t("cmd.shape3d.taper.range", &[]));
    }
    let r = ccw(ring);
    let Some(top) = inset_ring(&r, h.abs() * angle.jtan()) else { return Err(t("cmd.shape3d.taper.steep", &[])) };
    let (ox, oy) = min_xy(&r);
    let bb: Vec<P3> = r.iter().map(|[x, y]| [x - ox, y - oy, 0.0]).collect();
    let tt: Vec<P3> = top.iter().map(|[x, y]| [x - ox, y - oy, h]).collect();
    let mut polys: Vec<Vec<P3>> = vec![bb.iter().rev().copied().collect(), tt.clone()];
    let n = bb.len();
    for i in 0..n {
        let j = nxt(i, n);
        let g = |v: &Vec<P3>, k: usize| v.get(k).copied().unwrap_or([f64::NAN; 3]);
        polys.push(vec![g(&bb, i), g(&bb, j), g(&tt, j), g(&tt, i)]);
    }
    place_polys(m, polys, [ox, oy, z0], "Extrusion", &t("cmd.shape3d.what.taper", &[]), t)
}

/// `makeRevolved(model, ring, a, b, angle)` → instance id.
pub fn make_revolved(m: &mut Model, ring: &[[f64; 2]], a: [f64; 2], b: [f64; 2], angle: f64, t: &Tr<'_>) -> Result<f64, Fail> {
    let ax = [b[0] - a[0], b[1] - a[1], 0.0];
    let l = js::hypot(ax[0], ax[1]);
    if !(l > 1e-9) {
        return Err(t("cmd.shape3d.axis", &[]));
    }
    let u = [ax[0] / l, ax[1] / l, 0.0];
    let r = ccw(ring);
    let side: Vec<f64> = r.iter().map(|[x, y]| u[0] * (y - a[1]) - u[1] * (x - a[0])).collect();
    let lim = 1e-9 * js::max(1.0, l);
    if side.iter().any(|s| *s > lim) && side.iter().any(|s| *s < -lim) {
        return Err(t("cmd.shape3d.cross", &[]));
    }
    let full = angle.abs() >= TAU - 1e-9;
    let nf = js::max(2.0, (PER_TURN as f64 * angle.abs() / TAU).ceil());
    if !nf.is_finite() || nf > 1.0e5 {
        return Err(t("cmd.shape3d.axis", &[]));
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = nf as usize;
    let rot = |p: [f64; 2], th: f64| -> P3 {
        let v = [p[0] - a[0], p[1] - a[1], 0.0];
        let (c, s) = (th.jcos(), th.jsin());
        let k = cross3(u, v);
        let d = dot3(u, v);
        [v[0] * c + k[0] * s + u[0] * d * (1.0 - c), v[1] * c + k[1] * s + u[1] * d * (1.0 - c), v[2] * c + k[2] * s + u[2] * d * (1.0 - c)]
    };
    let last = if full { n.saturating_sub(1) } else { n };
    let rings: Vec<Vec<P3>> = (0..=last).map(|k| r.iter().map(|p| rot(*p, angle * k as f64 / n as f64)).collect()).collect();
    let mut polys: Vec<Vec<P3>> = Vec::new();
    let g = |v: &Vec<P3>, k: usize| v.get(k).copied().unwrap_or([f64::NAN; 3]);
    for k in 0..n {
        let (r0, r1) = (rings.get(k), rings.get(nxt(k, rings.len())));
        let (Some(r0), Some(r1)) = (r0, r1) else { continue };
        for i in 0..r.len() {
            let j = nxt(i, r.len());
            polys.push(vec![g(r0, i), g(r0, j), g(r1, j), g(r1, i)]);
        }
    }
    if !full {
        if let Some(f) = rings.first() {
            polys.push(f.iter().rev().copied().collect());
        }
        if let Some(l) = rings.last() {
            polys.push(l.clone());
        }
    }
    place_polys(m, polys, [a[0], a[1], 0.0], "Revolution", &t("cmd.shape3d.what.revolve", &[]), t)
}

/// `makeSwept(model, ring, path, closed)` → instance id.
pub fn make_swept(m: &mut Model, ring: &[[f64; 2]], path: &[[f64; 2]], closed: bool, t: &Tr<'_>) -> Result<f64, Fail> {
    let mut pp: Vec<[f64; 2]> = Vec::new();
    for p in path {
        if pp.last().is_none_or(|l| js::hypot(p[0] - l[0], p[1] - l[1]) > 1e-9) {
            pp.push(*p);
        }
    }
    if closed
        && pp.len() > 2
        && let (Some(a), Some(b)) = (pp.first(), pp.last())
        && js::hypot(a[0] - b[0], a[1] - b[1]) <= 1e-9
    {
        pp.pop();
    }
    if pp.len() < 2 {
        return Err(t("cmd.shape3d.path", &[]));
    }
    let r = ccw(ring);
    let (mut cx, mut cy, mut aa) = (0.0, 0.0, 0.0);
    for i in 0..r.len() {
        let (p, q) = (at2(&r, i), at2(&r, nxt(i, r.len())));
        let w = p[0] * q[1] - q[0] * p[1];
        aa += w;
        cx += (p[0] + q[0]) * w;
        cy += (p[1] + q[1]) * w;
    }
    cx /= 3.0 * aa;
    cy /= 3.0 * aa;
    let prof: Vec<[f64; 2]> = r.iter().map(|[x, y]| [x - cx, y - cy]).collect();
    let n = pp.len();
    let dir = |i: usize, j: usize| {
        let (a, b) = (at2(&pp, i), at2(&pp, j));
        norm2([b[0] - a[0], b[1] - a[1]])
    };
    let mut frames: Vec<([f64; 2], f64, f64)> = Vec::with_capacity(n);
    for i in 0..n {
        let has_prev = closed || i > 0;
        let has_next = closed || i.saturating_add(1) < n;
        let tin = has_prev.then(|| dir(prv(i, n), i));
        let tout = has_next.then(|| dir(i, nxt(i, n)));
        let n1 = tin.map(|t| [-t[1], t[0]]);
        let n2 = tout.map(|t| [-t[1], t[0]]);
        let nn = match (n1, n2) {
            (Some(a), Some(b)) => norm2([a[0] + b[0], a[1] + b[1]]),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => [f64::NAN; 2],
        };
        let c = match (n1, n2) {
            (Some(a), Some(_)) => nn[0] * a[0] + nn[1] * a[1],
            _ => 1.0,
        };
        if !(c > 0.05) {
            return Err(t("cmd.shape3d.sharp", &[]));
        }
        frames.push((at2(&pp, i), nn[0] / c, nn[1] / c));
    }
    let (ox, oy) = (at2(&pp, 0)[0], at2(&pp, 0)[1]);
    let rings: Vec<Vec<P3>> =
        frames.iter().map(|(o, nx, ny)| prof.iter().map(|[x, y]| [o[0] - ox + nx * x, o[1] - oy + ny * x, *y]).collect()).collect();
    let mut polys: Vec<Vec<P3>> = Vec::new();
    let segs = if closed { n } else { n.saturating_sub(1) };
    let g = |v: &Vec<P3>, k: usize| v.get(k).copied().unwrap_or([f64::NAN; 3]);
    for k in 0..segs {
        let (Some(r0), Some(r1)) = (rings.get(k), rings.get(nxt(k, n))) else { continue };
        for i in 0..prof.len() {
            let j = nxt(i, prof.len());
            polys.push(vec![g(r0, i), g(r0, j), g(r1, j), g(r1, i)]);
        }
    }
    if !closed {
        if let Some(f) = rings.first() {
            polys.push(f.iter().rev().copied().collect());
        }
        if let Some(l) = rings.get(n.saturating_sub(1)) {
            polys.push(l.clone());
        }
    }
    place_polys(m, polys, [ox, oy, 0.0], "Sweep", &t("cmd.shape3d.what.sweep", &[]), t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_survives_its_snapshot() {
        let mut m = Model::new();
        let t = |k: &str| k.to_string();
        let iid = make_box(&mut m, [0.0, 0.0], [0.01, 0.005], 0.0, 0.003, &t);
        assert!(iid.is_ok());
        let s = m.snapshot();
        let mut doc = Doc::new();
        doc.m3d = Some(std::sync::Arc::from(s.as_str()));
        let mut back = model_of(&doc, "Faces");
        assert!(!is_empty(&back));
        assert_eq!(back.snapshot(), s);
    }
}
