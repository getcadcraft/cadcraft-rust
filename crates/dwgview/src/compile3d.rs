//! dwg-compile3d.js: a drawing model's model space as 3D batches for the drawing's 3D view. Line for line:
//! `from_model` is the adapter (`fromDwgModel`: the reader's model → a plain DXF-semantic scene, defaults as the JS
//! writes them), `compile_scene` the compiler (`compileScene`). One batch per layer × colour (layer "0" and BYBLOCK
//! inherit from the insert); real Z for lines, polylines at their elevation, 3D polylines, 3D faces, polyface and
//! polygon meshes, circles and arcs in their plane, ellipses, splines, leaders, multilines, solids / traces, hatch
//! outlines, thickness (walls, cylinder walls capped as AutoCAD hides them), wide polylines (ribbons), text lines, and
//! inserts with full 3D transforms (OCS by the arbitrary-axis rule, scale, rotation, arrays), nested, linked drawings
//! included. Dimensions and tables draw their anonymous blocks. Multileaders, pictures, hatch patterns are counted
//! as "2D only"; ACIS solids, surfaces, subdivision meshes and the unknown as "not drawn", never guessed.
//!
//! JS semantics kept: `||` defaults, V8's `Math.sin` / `cos` / `atan2` / `hypot`, Map insertion order of batches and
//! counters, `localeCompare(…, {numeric: true})` for the layer list. What would hang or overflow the JS (cyclic
//! dimension blocks or linked drawings, huge MINSERT arrays) is bounded here.

use std::collections::HashMap;

use cc_cad::js;
use cc_cad_geom::jsmath::JsMath;
use cc_drawing::Color;
use cc_io_cad::jsv::{self, Ov};
use cc_io_cad::small::hypot3;
use cc_io_dwg::compile::{
    aci_color, arc_points, bulge_polyline, hatch_rings, js_hypot, layer_color, mtext_lines, plain_text, rgb_hex, wrap_sweep,
};
use cc_io_dwg::value::{Obj, V};

use crate::tr;

const MAX_PRIMS: u64 = 4_000_000;
const TAU: f64 = std::f64::consts::TAU;
const FLAT_ONLY: &[&str] = &["MULTILEADER", "IMAGE", "ARCALIGNEDTEXT", "TOLERANCE", "WIPEOUT", "OLE2FRAME", "OLEFRAME"];
const SILENT: &[&str] = &["VIEWPORT", "POINT", "XLINE", "RAY", "SEQEND"];
const ACIS: &[&str] = &["3DSOLID", "BODY", "REGION"];
/// Nesting the JS leaves unbounded (dimension blocks inside dimension blocks; linked drawings inside linked drawings).
const MAX_DEPTH: u32 = 200;
const MAX_XREF_DEPTH: u32 = 8;
/// MINSERT copies per reference (rows × columns) looked at; the JS would spin on more.
const MAX_ARRAY: f64 = 1_000_000.0;

type P = [f64; 3];
/// 3 × 4 affine matrix, row-major.
pub type M3 = [f64; 12];
pub const I3: M3 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];

/// `mul3(a, b)`.
pub fn mul3(a: &M3, b: &M3) -> M3 {
    let row = |r: [f64; 4]| -> [f64; 4] {
        [
            r[0] * b[0] + r[1] * b[4] + r[2] * b[8],
            r[0] * b[1] + r[1] * b[5] + r[2] * b[9],
            r[0] * b[2] + r[1] * b[6] + r[2] * b[10],
            r[0] * b[3] + r[1] * b[7] + r[2] * b[11] + r[3],
        ]
    };
    let [r0, r1, r2] = [row([a[0], a[1], a[2], a[3]]), row([a[4], a[5], a[6], a[7]]), row([a[8], a[9], a[10], a[11]])];
    [r0[0], r0[1], r0[2], r0[3], r1[0], r1[1], r1[2], r1[3], r2[0], r2[1], r2[2], r2[3]]
}
fn t3(x: f64, y: f64, z: f64) -> M3 {
    [1.0, 0.0, 0.0, x, 0.0, 1.0, 0.0, y, 0.0, 0.0, 1.0, z]
}
fn s3(x: f64, y: f64, z: f64) -> M3 {
    [x, 0.0, 0.0, 0.0, 0.0, y, 0.0, 0.0, 0.0, 0.0, z, 0.0]
}
fn rz(a: f64) -> M3 {
    let (c, s) = (a.jcos(), a.jsin());
    [c, -s, 0.0, 0.0, s, c, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0]
}
/// `apply3(m, x, y, z)`.
pub fn apply3(m: &M3, x: f64, y: f64, z: f64) -> P {
    [m[0] * x + m[1] * y + m[2] * z + m[3], m[4] * x + m[5] * y + m[6] * z + m[7], m[8] * x + m[9] * y + m[10] * z + m[11]]
}
fn dir3(m: &M3, x: f64, y: f64, z: f64) -> P {
    [m[0] * x + m[1] * y + m[2] * z, m[4] * x + m[5] * y + m[6] * z, m[8] * x + m[9] * y + m[10] * z]
}

/// The extrusion direction, or Z when it is missing or zero (`normal(ext)`).
fn normal(ext: &Option<P>) -> P {
    let [x, y, z] = ext.unwrap_or([0.0, 0.0, 0.0]);
    let l = hypot3(x, y, z);
    if l < 1e-12 { [0.0, 0.0, 1.0] } else { [x / l, y / l, z / l] }
}
/// Object coordinate system → world: AutoCAD's arbitrary-axis algorithm (`ocsMatrix(ext)`).
fn ocs_matrix(ext: &Option<P>) -> M3 {
    let n = normal(ext);
    if n[0] == 0.0 && n[1] == 0.0 && n[2] == 1.0 {
        return I3;
    }
    let cross = |a: P, b: P| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let unit = |v: P| {
        let l = hypot3(v[0], v[1], v[2]);
        let l = if l == 0.0 || l.is_nan() { 1.0 } else { l };
        [v[0] / l, v[1] / l, v[2] / l]
    };
    let ax = unit(if n[0].abs() < 1.0 / 64.0 && n[1].abs() < 1.0 / 64.0 { cross([0.0, 1.0, 0.0], n) } else { cross([0.0, 0.0, 1.0], n) });
    let ay = unit(cross(n, ax));
    [ax[0], ay[0], n[0], 0.0, ax[1], ay[1], n[1], 0.0, ax[2], ay[2], n[2], 0.0]
}

// ---------------------------------------------------------------------------------------------------------------
// The scene (fromDwgModel)

/// `P(p)`: `{x: p?.x || 0, …}`.
fn pp(p: Ov) -> P {
    [jsv::or(jsv::get(p, "x"), 0.0), jsv::or(jsv::get(p, "y"), 0.0), jsv::or(jsv::get(p, "z"), 0.0)]
}
fn ext_of(e: &Obj) -> Option<P> {
    let x = e.get("extrusion");
    if jsv::truthy(x) { Some(pp(x)) } else { None }
}

#[derive(Clone, Debug)]
enum Col {
    Rgb(f64),
    Index(f64),
}

/// A polyline vertex of the scene.
#[derive(Clone, Debug)]
struct PV {
    /// the reader's x / y values (`v.x`, `v.y`, kept as they are: the corner keys are their JS strings)
    x: Option<V>,
    y: Option<V>,
    bulge: f64,
    flags: f64,
    sw: f64,
    ew: f64,
}

/// One line of text in its own coordinate system (`textLines`).
#[derive(Clone, Debug)]
struct TextLine {
    s: String,
    x: f64,
    y: f64,
    z: f64,
    ux: f64,
    uy: f64,
    vx: f64,
    vy: f64,
    ha: Option<f64>,
    va: f64,
}

#[derive(Clone, Debug)]
enum Kind {
    Line {
        start: P,
        end: P,
        thickness: f64,
        ext: Option<P>,
    },
    Circle {
        center: P,
        radius: f64,
        thickness: f64,
        ext: Option<P>,
    },
    Arc {
        center: P,
        radius: f64,
        sa: f64,
        ea: f64,
        thickness: f64,
        ext: Option<P>,
    },
    Poly2 {
        verts: Vec<PV>,
        closed: bool,
        elevation: f64,
        thickness: f64,
        ext: Option<P>,
    },
    Text {
        lines: Vec<TextLine>,
        ext: Option<P>,
    },
    Poly3 {
        verts: Vec<(P, f64)>,
        closed: bool,
    },
    PFace {
        verts: Vec<P>,
        faces: Vec<Option<Vec<f64>>>,
    },
    Mesh {
        m: f64,
        n: f64,
        verts: Vec<P>,
        closed_m: bool,
        closed_n: bool,
    },
    Face3 {
        pts: [P; 4],
        inv: f64,
    },
    Solid {
        pts: [P; 4],
        elevation: Option<f64>,
        ext: Option<P>,
    },
    Ellipse {
        center: P,
        major: P,
        ratio: f64,
        sp: f64,
        ep: f64,
        ext: Option<P>,
    },
    Spline {
        pts: Vec<P>,
    },
    Path {
        verts: Vec<P>,
    },
    Hatch {
        rings: Vec<Vec<f64>>,
        solid: bool,
        elevation: f64,
        ext: Option<P>,
    },
    Insert {
        block: String,
        insert: P,
        scale: P,
        rotation: f64,
        ext: Option<P>,
        cols: f64,
        rows: f64,
        col_sp: f64,
        row_sp: f64,
        attribs: Vec<Ent>,
    },
    Block {
        block: Option<String>,
        insert: P,
    },
    Other,
}

#[derive(Clone, Debug)]
struct Ent {
    ty: String,
    handle: Option<V>,
    layer: String,
    color: Col,
    invisible: bool,
    kind: Kind,
}

#[derive(Clone, Debug)]
struct LayerDef {
    name: String,
    color: Color,
    on: bool,
}

#[derive(Debug, Default)]
struct Blk {
    base: P,
    entities: Vec<Ent>,
    xref: Option<Box<Scene>>,
    xref_missing: bool,
}

#[derive(Debug, Default)]
struct Scene {
    units: &'static str,
    layers: Vec<LayerDef>,
    model: Vec<Ent>,
    blocks: HashMap<String, Blk>,
    base: P,
}

/// `textLines(e)` for TEXT / ATTRIB / ATTDEF / MTEXT (the scene's fields).
fn text_lines(e: &Obj, ty: &str) -> Vec<TextLine> {
    let g = |k: &str| e.get(k);
    let h = jsv::or(g("height"), 0.0);
    // !(h > 0): NaN too
    if h.is_nan() || h <= 0.0 {
        return Vec::new();
    }
    let ins = pp(g("insert"));
    let u16s = |v: Ov| -> Vec<u16> { jsv::str_or_empty(v).encode_utf16().collect() };
    let blank = |s: &str| js::trim(s).is_empty();
    if ty == "MTEXT" {
        let raw: Vec<String> = mtext_lines(&u16s(g("value"))).iter().map(|l| String::from_utf16_lossy(l)).collect();
        let n = raw.len();
        let lines: Vec<String> =
            raw.into_iter().enumerate().filter(|(i, l)| !blank(l) || i.saturating_add(1) < n).map(|(_, l)| l).collect();
        if lines.is_empty() {
            return Vec::new();
        }
        let xa = g("xAxis");
        let xax = if jsv::truthy(xa) { Some(pp(xa)) } else { None };
        let rot = match xax {
            Some(x) if x[0] != 0.0 || x[1] != 0.0 => x[1].jatan2(x[0]),
            _ => jsv::or(g("rotation"), 0.0),
        };
        let ap = jsv::or(g("attach"), 1.0) - 1.0;
        let ha = ap % 3.0;
        let row = (ap / 3.0).floor();
        let gap = h * 1.667 * jsv::or(g("lineSpacing"), 1.0);
        let total = gap * (lines.len() as f64 - 1.0);
        let top = if row == 0.0 {
            -h
        } else if row == 1.0 {
            total / 2.0 - h / 2.0
        } else {
            total
        };
        let (c, sn) = (rot.jcos(), rot.jsin());
        return lines
            .into_iter()
            .enumerate()
            .map(|(i, s)| {
                let dy = top - gap * i as f64;
                TextLine {
                    s,
                    x: ins[0] - dy * sn,
                    y: ins[1] + dy * c,
                    z: ins[2],
                    ux: c * h,
                    uy: sn * h,
                    vx: -sn * h,
                    vy: c * h,
                    ha: Some(ha),
                    va: 0.0,
                }
            })
            .filter(|l| !blank(&l.s))
            .collect();
    }
    let s = String::from_utf16_lossy(&plain_text(&u16s(g("value"))));
    if blank(&s) {
        return Vec::new();
    }
    let h_align = jsv::or(g("hAlign"), 0.0);
    let v_align = jsv::or(g("vAlign"), 0.0);
    let aligned = h_align != 0.0 || v_align != 0.0;
    let al = g("align");
    let align = if jsv::truthy(al) { Some(pp(al)) } else { None };
    let p = match align {
        Some(a) if aligned && (a[0] != 0.0 || a[1] != 0.0) => a,
        _ => ins,
    };
    let rot = jsv::or(g("rotation"), 0.0);
    let (c, sn) = (rot.jcos(), rot.jsin());
    let generation = jsv::or(g("generation"), 0.0);
    let wf = jsv::or(g("widthFactor"), 1.0) * if js::int32(generation) as i64 & 2 != 0 { -1.0 } else { 1.0 };
    let ha = [0.0, 1.0, 2.0, 0.0, 1.0, 0.0];
    let ha = if h_align >= 0.0 && h_align.fract() == 0.0 { ha.get(h_align as usize).copied() } else { None };
    vec![TextLine {
        s,
        x: p[0],
        y: p[1],
        z: if p[2] != 0.0 && !p[2].is_nan() { p[2] } else { ins[2] },
        ux: c * h * wf,
        uy: sn * h * wf,
        vx: -sn * h,
        vy: c * h,
        ha,
        va: if h_align == 4.0 { 2.0 } else { v_align },
    }]
}

/// `spline3(e)` on the scene's spline: its fit points, else de Boor from the control points (3D).
fn spline3(e: &Obj) -> Vec<P> {
    let pts = |k: &str| -> Vec<P> { jsv::arr(e.get(k)).iter().map(|q| pp(Some(q))).collect() };
    let fit = pts("fit");
    if fit.len() > 1 {
        return fit;
    }
    let cp = pts("control");
    if cp.len() < 2 {
        return Vec::new();
    }
    let k: Vec<f64> =
        if jsv::truthy(e.get("knots")) { jsv::arr(e.get("knots")).iter().map(|v| jsv::to_num(Some(v))).collect() } else { Vec::new() };
    let p = jsv::or(e.get("degree"), 3.0);
    if k.len() as f64 != cp.len() as f64 + p + 1.0 {
        return cp;
    }
    if p < 0.0 || p.fract() != 0.0 {
        return Vec::new();
    }
    let p = p as usize;
    let w: Option<Vec<f64>> = match e.get("weights") {
        Some(V::Arr(a)) if a.len() == cp.len() => Some(a.iter().map(|v| jsv::to_num(Some(v))).collect()),
        _ => None,
    };
    let kk = |i: usize| k.get(i).copied().unwrap_or(f64::NAN);
    let lo = kk(p);
    let hi = kk(k.len().saturating_sub(p).saturating_sub(1));
    let n = 16usize.max(cp.len().saturating_mul(8));
    let mut out = Vec::with_capacity(n.saturating_add(1));
    let last = cp.len().saturating_sub(1);
    for s in 0..=n {
        let u = if s == n { hi - 1e-9 * (hi - lo) } else { lo + (hi - lo) * (s as f64 / n as f64) };
        let mut span = p;
        while span < last && u >= kk(span.saturating_add(1)) {
            span = span.saturating_add(1);
        }
        let base = span.saturating_sub(p);
        let mut d: Vec<[f64; 4]> = Vec::with_capacity(p.saturating_add(1));
        for j in 0..=p {
            let q = cp.get(base.saturating_add(j)).copied().unwrap_or([f64::NAN; 3]);
            let wt = w.as_ref().map_or(1.0, |w| w.get(base.saturating_add(j)).copied().unwrap_or(f64::NAN));
            d.push([q[0] * wt, q[1] * wt, q[2] * wt, wt]);
        }
        for r in 1..=p {
            for j in (r..=p).rev() {
                let i = base.saturating_add(j);
                let den = kk(i.saturating_add(p).saturating_sub(r).saturating_add(1)) - kk(i);
                let a = if den == 0.0 { 0.0 } else { (u - kk(i)) / den };
                let prev = d.get(j.saturating_sub(1)).copied().unwrap_or([f64::NAN; 4]);
                if let Some(dj) = d.get_mut(j) {
                    *dj = [
                        (1.0 - a) * prev[0] + a * dj[0],
                        (1.0 - a) * prev[1] + a * dj[1],
                        (1.0 - a) * prev[2] + a * dj[2],
                        (1.0 - a) * prev[3] + a * dj[3],
                    ];
                }
            }
        }
        let dp = d.get(p).copied().unwrap_or([f64::NAN; 4]);
        out.push([dp[0] / dp[3], dp[1] / dp[3], dp[2] / dp[3]]);
    }
    out
}

fn color_of(e: &Obj) -> Col {
    let c = e.get("color");
    if jsv::truthy(c) && jsv::get(c, "rgb").is_some() {
        return Col::Rgb(jsv::to_num(jsv::get(c, "rgb")));
    }
    match jsv::get(c, "index") {
        None | Some(V::Null) => Col::Index(256.0),
        Some(V::Num(n)) => Col::Index(*n),
        Some(_) => Col::Index(f64::NAN),
    }
}

/// `ent(e)` of the adapter.
fn ent(e: &Obj) -> Ent {
    let g = |k: &str| e.get(k);
    let ty0 = jsv::str_nn(g("type"));
    let mut ty = ty0.clone();
    let layer = if jsv::truthy(g("layer")) { jsv::to_str(g("layer")) } else { "0".into() };
    let mut invisible = jsv::truthy(g("invisible"));
    let t = jsv::or(g("thickness"), 0.0);
    let ext = ext_of(e);
    let verts = |k: &str| jsv::arr(g(k));
    let kind = match ty0.as_str() {
        "LINE" => Kind::Line { start: pp(g("start")), end: pp(g("end")), thickness: t, ext },
        "CIRCLE" => Kind::Circle { center: pp(g("center")), radius: jsv::to_num(g("radius")), thickness: t, ext },
        "ARC" => Kind::Arc {
            center: pp(g("center")),
            radius: jsv::to_num(g("radius")),
            sa: jsv::to_num(g("startAngle")),
            ea: jsv::to_num(g("endAngle")),
            thickness: t,
            ext,
        },
        "LWPOLYLINE" | "POLYLINE_2D" => {
            let lw = ty0 == "LWPOLYLINE";
            let cw = jsv::or(g("constantWidth"), 0.0);
            let nn2 = |v: Ov, k: &str, d: f64| -> f64 {
                let x = jsv::get(v, k);
                if jsv::nullish(x) { d } else { jsv::to_num(x) }
            };
            let vs = verts("vertices")
                .iter()
                .map(|v| {
                    let v = Some(v);
                    let (sw, ew) = if lw {
                        (nn2(v, "startWidth", cw), nn2(v, "endWidth", cw))
                    } else {
                        (nn2(v, "startWidth", jsv::nn(g("startWidth"), 0.0)), nn2(v, "endWidth", jsv::nn(g("endWidth"), 0.0)))
                    };
                    PV {
                        x: jsv::get(v, "x").cloned(),
                        y: jsv::get(v, "y").cloned(),
                        bulge: jsv::or(jsv::get(v, "bulge"), 0.0),
                        flags: if lw { 0.0 } else { jsv::or(jsv::get(v, "flags"), 0.0) },
                        sw,
                        ew,
                    }
                })
                .collect();
            Kind::Poly2 { verts: vs, closed: jsv::truthy(g("closed")), elevation: jsv::or(g("elevation"), 0.0), thickness: t, ext }
        }
        "TEXT" | "ATTRIB" | "ATTDEF" => {
            invisible = invisible || js::int32(jsv::or(g("flags"), 0.0)) as i64 & 1 == 1;
            Kind::Text { lines: text_lines(e, &ty0), ext }
        }
        "MTEXT" => Kind::Text { lines: text_lines(e, "MTEXT"), ext },
        "POLYLINE_3D" => Kind::Poly3 {
            verts: verts("vertices")
                .iter()
                .map(|v| {
                    let v = Some(v);
                    (
                        [jsv::to_num(jsv::get(v, "x")), jsv::to_num(jsv::get(v, "y")), jsv::or(jsv::get(v, "z"), 0.0)],
                        jsv::or(jsv::get(v, "flags"), 0.0),
                    )
                })
                .collect(),
            closed: jsv::truthy(g("closed")),
        },
        "POLYLINE_PFACE" => Kind::PFace {
            verts: verts("vertices").iter().map(|v| pp(Some(v))).collect(),
            faces: verts("faces")
                .iter()
                .map(|f| match f {
                    V::Arr(a) => Some(
                        a.iter()
                            .map(|i| match i {
                                V::Num(n) => *n,
                                other => jsv::to_num(Some(other)),
                            })
                            .collect(),
                    ),
                    _ => None,
                })
                .collect(),
        },
        "POLYLINE_MESH" => {
            let fl = js::int32(jsv::to_num(g("flags"))) as i64;
            Kind::Mesh {
                m: jsv::to_num(g("m")),
                n: jsv::to_num(g("n")),
                verts: verts("vertices").iter().map(|v| pp(Some(v))).collect(),
                closed_m: fl & 1 != 0,
                closed_n: fl & 32 != 0,
            }
        }
        "3DFACE" | "SOLID" | "TRACE" => {
            let p = verts("points");
            let q = |i: usize| pp(p.get(i));
            let p3 = if jsv::truthy(p.get(3)) { q(3) } else { q(2) };
            let pts = [q(0), q(1), q(2), p3];
            if ty0 == "3DFACE" {
                Kind::Face3 { pts, inv: jsv::or(g("invisibleEdges"), 0.0) }
            } else {
                // e.elevation ?? p[0]?.z ?? 0
                let el = g("elevation");
                let elev = if !jsv::nullish(el) {
                    el
                } else {
                    let z = jsv::get(p.first(), "z");
                    if jsv::nullish(z) { None } else { z }
                };
                let elevation = match elev {
                    Some(V::Num(n)) => Some(*n),
                    None => Some(0.0),
                    Some(_) => None,
                };
                Kind::Solid { pts, elevation, ext }
            }
        }
        "ELLIPSE" => Kind::Ellipse {
            center: pp(g("center")),
            major: pp(g("majorAxis")),
            ratio: jsv::to_num(g("ratio")),
            sp: jsv::to_num(g("startParam")),
            ep: jsv::to_num(g("endParam")),
            ext,
        },
        "SPLINE" => Kind::Spline { pts: spline3(e) },
        "LEADER" => Kind::Path { verts: verts("vertices").iter().map(|v| pp(Some(v))).collect() },
        "MLINE" => {
            let mut vs: Vec<P> = verts("vertices").iter().map(|v| pp(jsv::get(Some(v), "point"))).collect();
            if jsv::truthy(g("closed"))
                && vs.len() > 2
                && let Some(f) = vs.first().copied()
            {
                vs.push(f);
            }
            Kind::Path { verts: vs }
        }
        "HATCH" => Kind::Hatch { rings: hatch_rings(e), solid: jsv::truthy(g("solid")), elevation: jsv::or(g("elevation"), 0.0), ext },
        "INSERT" | "MINSERT" => {
            ty = "INSERT".into();
            let sc = g("scale");
            Kind::Insert {
                block: jsv::str_nn(g("block")),
                insert: pp(g("insert")),
                scale: [jsv::or(jsv::get(sc, "x"), 1.0), jsv::or(jsv::get(sc, "y"), 1.0), jsv::or(jsv::get(sc, "z"), 1.0)],
                rotation: jsv::or(g("rotation"), 0.0),
                ext,
                cols: jsv::or(g("cols"), 1.0),
                rows: jsv::or(g("rows"), 1.0),
                col_sp: jsv::or(g("colSpacing"), 0.0),
                row_sp: jsv::or(g("rowSpacing"), 0.0),
                attribs: verts("attribs").iter().filter_map(V::as_obj).map(ent).collect(),
            }
        }
        "ACAD_TABLE" => {
            Kind::Block { block: if jsv::truthy(g("block")) { Some(jsv::to_str(g("block"))) } else { None }, insert: pp(g("insert")) }
        }
        "UNKNOWN" => {
            ty = if jsv::truthy(g("dxfName")) {
                jsv::to_str(g("dxfName"))
            } else if jsv::truthy(g("className")) {
                jsv::to_str(g("className"))
            } else {
                "UNKNOWN".into()
            };
            Kind::Other
        }
        other => {
            if other.starts_with("DIMENSION") || other == "ARC_DIMENSION" || other == "LARGE_RADIAL_DIMENSION" {
                ty = "DIMENSION".into();
                Kind::Block { block: if jsv::truthy(g("block")) { Some(jsv::to_str(g("block"))) } else { None }, insert: pp(g("insert")) }
            } else {
                Kind::Other
            }
        }
    };
    Ent { ty, handle: g("handle").cloned(), layer, color: color_of(e), invisible, kind }
}

fn scene(d: &Obj, xrefs: &[(String, Obj)], depth: u32) -> Scene {
    let layers: Vec<LayerDef> = jsv::arr(jsv::get(d.get("tables"), "layers"))
        .iter()
        .filter_map(V::as_obj)
        .map(|l| LayerDef { name: jsv::str_nn(l.get("name")), color: layer_color(l), on: !l.truthy("off") && !l.truthy("frozen") })
        .collect();
    let blocks_v = d.arr("blocks").map(Vec::as_slice).unwrap_or(&[]);
    let model = blocks_v.iter().filter_map(V::as_obj).find(|b| jsv::str_nn(b.get("name")).eq_ignore_ascii_case("*model_space"));
    let mut blocks: HashMap<String, Blk> = HashMap::new();
    for b in blocks_v.iter().filter_map(V::as_obj) {
        if model.is_some_and(|m| std::ptr::eq(m, b)) {
            continue;
        }
        let name = jsv::str_nn(b.get("name"));
        if name.len() >= 12 && name.get(..12).is_some_and(|p| p.eq_ignore_ascii_case("*paper_space")) {
            continue;
        }
        let mut blk = Blk { base: pp(b.get("base")), ..Blk::default() };
        if js::int32(jsv::to_num(b.get("flags"))) as i64 & 4 != 0 {
            let lower = name.to_lowercase();
            match xrefs.iter().find(|(k, _)| *k == lower) {
                Some((_, x)) if depth < MAX_XREF_DEPTH => blk.xref = Some(Box::new(scene(x, xrefs, depth.saturating_add(1)))),
                _ => blk.xref_missing = true,
            }
        } else {
            blk.entities = b
                .arr("entities")
                .map(Vec::as_slice)
                .unwrap_or(&[])
                .iter()
                .filter_map(V::as_obj)
                .filter(|e| e.str("type") != Some("ATTDEF"))
                .map(ent)
                .collect();
        }
        blocks.insert(name, blk);
    }
    let units = cc_io_dwg::compile::units_of(jsv::get(d.get("header"), "INSUNITS"));
    Scene {
        units,
        layers,
        model: model
            .map(|m| m.arr("entities").map(Vec::as_slice).unwrap_or(&[]).iter().filter_map(V::as_obj).map(ent).collect())
            .unwrap_or_default(),
        blocks,
        base: pp(jsv::get(d.get("header"), "INSBASE")),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The output

/// A line of text in 3D: at `o`, one height unit along `xd` (baseline) and `yd` (up).
#[derive(Clone, Debug, PartialEq)]
pub struct Text3 {
    pub owner: u32,
    pub s: String,
    pub o: P,
    pub xd: P,
    pub yd: P,
    /// 0 left, 1 centre, 2 right (None: the JS `undefined` of an out-of-range alignment)
    pub ha: Option<f64>,
    /// 0 baseline, 1 bottom, 2 middle, 3 top
    pub va: f64,
}

/// One layer × colour.
#[derive(Clone, Debug, PartialEq)]
pub struct Batch3 {
    pub layer: String,
    pub color: Color,
    /// segment pairs, x y z
    pub lines: Vec<f64>,
    /// x y z per corner
    pub tris: Vec<f64>,
    pub line_owner: Vec<u32>,
    pub tri_owner: Vec<u32>,
    pub texts: Vec<Text3>,
}

/// A model-space entity as the pick names it.
#[derive(Clone, Debug, PartialEq)]
pub struct Owner {
    pub ty: String,
    pub handle: Option<String>,
    pub layer: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layer3 {
    pub name: String,
    pub color: Color,
    pub on: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats3 {
    pub entities: u64,
    pub segments: u64,
    pub triangles: u64,
    /// type → count, in first-seen order (the JS object's key order)
    pub drawn: Vec<(String, u64)>,
    pub not_drawn: Vec<(String, u64)>,
    pub flat_only: Vec<(String, u64)>,
    pub polyline3d_fixed: u64,
    pub truncated: bool,
}

/// `compileModel3d(model, { xrefs })`.
#[derive(Clone, Debug, PartialEq)]
pub struct Model3 {
    pub units: String,
    pub batches: Vec<Batch3>,
    pub owners: Vec<Owner>,
    /// drawing units; empty when min > max
    pub min: P,
    pub max: P,
    pub layers: Vec<Layer3>,
    pub stats: Stats3,
}

fn count(o: &mut Vec<(String, u64)>, t: &str) {
    match o.iter_mut().find(|(k, _)| k == t) {
        Some((_, n)) => *n = n.saturating_add(1),
        None => o.push((t.to_string(), 1)),
    }
}

/// `'ink'` or `'#rrggbb'`.
pub fn color_name(c: Color) -> String {
    match c {
        Color::Ink => "ink".into(),
        Color::Rgb(n) => format!("#{:06x}", n & 0xff_ffff),
    }
}

struct Ctx<'s> {
    m: M3,
    layer: Option<String>,
    color: Option<Color>,
    layer_color: Option<Color>,
    depth: u32,
    owner: u32,
    blocks: &'s HashMap<String, Blk>,
    xp: String,
    xlayers: Option<HashMap<String, (Color, bool)>>,
}

struct Compiler {
    layers: Vec<LayerDef>,
    layer_index: HashMap<String, usize>,
    stats: Stats3,
    bmin: P,
    bmax: P,
    batches: Vec<Batch3>,
    batch_index: HashMap<(String, Color), usize>,
}

impl Compiler {
    fn full(&mut self) -> bool {
        if self.stats.segments.saturating_add(self.stats.triangles) > MAX_PRIMS {
            self.stats.truncated = true;
            return true;
        }
        false
    }
    fn grow(&mut self, p: &P) -> bool {
        if p.iter().any(|v| !v.is_finite()) {
            return false;
        }
        for ((v, lo), hi) in p.iter().zip(self.bmin.iter_mut()).zip(self.bmax.iter_mut()) {
            if *v < *lo {
                *lo = *v;
            }
            if *v > *hi {
                *hi = *v;
            }
        }
        true
    }
    fn layer_of(&mut self, name: &str, fb: Option<(Color, bool)>) -> (Color, bool) {
        if let Some(&i) = self.layer_index.get(name)
            && let Some(l) = self.layers.get(i)
        {
            return (l.color, l.on);
        }
        let (color, on) = fb.unwrap_or((Color::Ink, true));
        self.layers.push(LayerDef { name: name.to_string(), color, on });
        self.layer_index.insert(name.to_string(), self.layers.len().saturating_sub(1));
        (color, on)
    }
    fn batch(&mut self, layer: &str, color: Color) -> usize {
        if let Some(&i) = self.batch_index.get(&(layer.to_string(), color)) {
            return i;
        }
        self.batches.push(Batch3 {
            layer: layer.to_string(),
            color,
            lines: Vec::new(),
            tris: Vec::new(),
            line_owner: Vec::new(),
            tri_owner: Vec::new(),
            texts: Vec::new(),
        });
        let i = self.batches.len().saturating_sub(1);
        self.batch_index.insert((layer.to_string(), color), i);
        i
    }
    fn seg(&mut self, bi: usize, own: u32, a: &P, b: &P) {
        if self.grow(a) && self.grow(b) {
            if let Some(out) = self.batches.get_mut(bi) {
                out.lines.extend_from_slice(a);
                out.lines.extend_from_slice(b);
                out.line_owner.push(own);
            }
            self.stats.segments = self.stats.segments.saturating_add(1);
        }
    }
    fn path(&mut self, bi: usize, own: u32, pts: &[P]) {
        for w in pts.windows(2) {
            if let [a, b] = w {
                self.seg(bi, own, a, b);
            }
        }
    }
    fn tri(&mut self, bi: usize, own: u32, a: &P, b: &P, c: &P) {
        if self.grow(a) && self.grow(b) && self.grow(c) {
            if let Some(out) = self.batches.get_mut(bi) {
                out.tris.extend_from_slice(a);
                out.tris.extend_from_slice(b);
                out.tris.extend_from_slice(c);
                out.tri_owner.push(own);
            }
            self.stats.triangles = self.stats.triangles.saturating_add(1);
        }
    }
    fn quad(&mut self, bi: usize, own: u32, a: &P, b: &P, c: &P, d: &P) {
        self.tri(bi, own, a, b, c);
        self.tri(bi, own, a, c, d);
    }
    /// A path plus, with thickness, the same path moved along `up` and walls between them.
    fn extrude(&mut self, bi: usize, own: u32, pts: &[P], up: Option<P>, verticals: &[usize], cap: Option<P>) {
        self.path(bi, own, pts);
        let Some(up) = up else { return };
        let top: Vec<P> = pts.iter().map(|p| [p[0] + up[0], p[1] + up[1], p[2] + up[2]]).collect();
        self.path(bi, own, &top);
        for i in 0..pts.len().saturating_sub(1) {
            if let (Some(a), Some(b), Some(c), Some(d)) = (pts.get(i), pts.get(i + 1), top.get(i + 1), top.get(i)) {
                self.quad(bi, own, a, b, c, d);
            }
        }
        for &i in verticals {
            if let (Some(a), Some(b)) = (pts.get(i), top.get(i)) {
                self.seg(bi, own, a, b);
            }
        }
        if let Some(cap) = cap {
            let ct = [cap[0] + up[0], cap[1] + up[1], cap[2] + up[2]];
            for i in 0..pts.len().saturating_sub(1) {
                if let (Some(p0), Some(p1), Some(t0), Some(t1)) = (pts.get(i), pts.get(i + 1), top.get(i), top.get(i + 1)) {
                    self.tri(bi, own, &cap, p1, p0);
                    self.tri(bi, own, &ct, t0, t1);
                }
            }
        }
    }

    fn emit(&mut self, e: &Ent, ctx: &Ctx<'_>) {
        if self.full() || e.invisible {
            return;
        }
        let src = e.layer.as_str();
        let layer = match &ctx.layer {
            Some(l) if src == "0" && !l.is_empty() => l.clone(),
            _ => format!("{}{src}", ctx.xp),
        };
        let fb = ctx.xlayers.as_ref().and_then(|x| x.get(src).copied());
        let (lay_color, _) = self.layer_of(&layer, fb);
        let color = match e.color {
            Col::Rgb(n) => rgb_hex(n),
            Col::Index(0.0) => ctx.color.unwrap_or(Color::Ink),
            Col::Index(256.0) => match ctx.layer_color {
                Some(lc) if src == "0" => lc,
                _ => lay_color,
            },
            Col::Index(i) => aci_color(i),
        };
        let bi = self.batch(&layer, color);
        let m = ctx.m;
        let own = ctx.owner;
        let w = |p: &P| apply3(&m, p[0], p[1], p[2]);
        let thick = |t: f64, ext: &Option<P>| -> Option<P> {
            if t == 0.0 || t.is_nan() {
                return None;
            }
            let n = normal(ext);
            let v = dir3(&m, n[0] * t, n[1] * t, n[2] * t);
            if hypot3(v[0], v[1], v[2]) > 0.0 { Some(v) } else { None }
        };
        let ocs_pts =
            |flat: &[f64], z: f64, om: &M3| -> Vec<P> { flat.as_chunks::<2>().0.iter().map(|c| apply3(om, c[0], c[1], z)).collect() };
        let ty = e.ty.as_str();
        let mut drawn = true;
        match &e.kind {
            Kind::Line { start, end, thickness, ext } => {
                let up = thick(*thickness, ext);
                self.extrude(bi, own, &[w(start), w(end)], up, &[0, 1], None);
            }
            Kind::Poly2 { verts, closed, elevation, thickness, ext } => {
                let vs: Vec<&PV> = verts.iter().filter(|v| js::int32(v.flags) as i64 & 16 == 0).collect();
                if vs.len() < 2 {
                    return;
                }
                let om = mul3(&m, &ocs_matrix(ext));
                let objs: Vec<V> = vs.iter().map(|v| pv_obj(v.x.clone(), v.y.clone(), v.bulge)).collect();
                let refs: Vec<&V> = objs.iter().collect();
                let flat = bulge_polyline(&refs, *closed);
                let pts = ocs_pts(&flat, *elevation, &om);
                let up = thick(*thickness, ext);
                let mut at: Vec<usize> = Vec::new();
                if up.is_some() {
                    let key = |x: &Option<V>, y: &Option<V>| format!("{},{}", jsv::to_str(x.as_ref()), jsv::to_str(y.as_ref()));
                    let corners: std::collections::HashSet<String> = vs.iter().map(|v| key(&v.x, &v.y)).collect();
                    for (k, c) in flat.as_chunks::<2>().0.iter().enumerate() {
                        if corners.contains(&format!("{},{}", js::num_to_string(c[0]), js::num_to_string(c[1]))) {
                            at.push(k);
                        }
                    }
                }
                self.extrude(bi, own, &pts, up, &at, None);
                if vs.iter().any(|v| v.sw > 0.0 || v.ew > 0.0) {
                    let n = if *closed { vs.len() } else { vs.len().saturating_sub(1) };
                    for i in 0..n {
                        let (Some(a), Some(b)) = (vs.get(i), vs.get((i + 1) % vs.len())) else { continue };
                        let same = std::ptr::eq(*a, *b);
                        let pa = pv_obj(a.x.clone(), a.y.clone(), a.bulge);
                        let pb = pv_obj(b.x.clone(), b.y.clone(), 0.0);
                        let seg = bulge_polyline(&[&pa, &pb], false);
                        let p2: Vec<[f64; 2]> = seg.as_chunks::<2>().0.to_vec();
                        if p2.len() < 2 {
                            continue;
                        }
                        let mut total = 0.0;
                        let mut at2 = vec![0.0];
                        for w2 in p2.windows(2) {
                            if let [a0, a1] = w2 {
                                total += js_hypot(a1[0] - a0[0], a1[1] - a0[1]);
                            }
                            at2.push(total);
                        }
                        let a_sw = if a.sw == 0.0 || a.sw.is_nan() { 0.0 } else { a.sw };
                        let a_ew = if a.ew == 0.0 || a.ew.is_nan() { 0.0 } else { a.ew };
                        let side = |k: usize| -> [f64; 2] {
                            let frac = if total != 0.0 && !total.is_nan() { at2.get(k).copied().unwrap_or(f64::NAN) / total } else { 0.0 };
                            let wd = (a_sw + ((if same { 0.0 } else { a_ew }) - a_sw) * frac) / 2.0;
                            let p = p2.get(k.saturating_sub(1)).copied().unwrap_or([f64::NAN; 2]);
                            let q = p2.get((k + 1).min(p2.len() - 1)).copied().unwrap_or([f64::NAN; 2]);
                            let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
                            let l = js_hypot(dx, dy);
                            let l = if l == 0.0 || l.is_nan() { 1.0 } else { l };
                            [-dy / l * wd, dx / l * wd]
                        };
                        let z = *elevation;
                        for (k, w2) in p2.windows(2).enumerate() {
                            let [c0, c1] = [w2.first().copied().unwrap_or([f64::NAN; 2]), w2.get(1).copied().unwrap_or([f64::NAN; 2])];
                            let (s0, s1) = (side(k), side(k + 1));
                            let l0 = apply3(&om, c0[0] + s0[0], c0[1] + s0[1], z);
                            let r0 = apply3(&om, c0[0] - s0[0], c0[1] - s0[1], z);
                            let l1 = apply3(&om, c1[0] + s1[0], c1[1] + s1[1], z);
                            let r1 = apply3(&om, c1[0] - s1[0], c1[1] - s1[1], z);
                            self.quad(bi, own, &l0, &r0, &r1, &l1);
                        }
                    }
                }
            }
            Kind::Text { lines, ext } => {
                if lines.is_empty() {
                    return;
                }
                let om = mul3(&m, &ocs_matrix(ext));
                for ln in lines {
                    let o = apply3(&om, ln.x, ln.y, ln.z);
                    let xd = dir3(&om, ln.ux, ln.uy, 0.0);
                    let yd = dir3(&om, ln.vx, ln.vy, 0.0);
                    if !self.grow(&o) {
                        continue;
                    }
                    // o + xd * length * 0.6, multiplied left to right as the JS does
                    let len = ln.s.encode_utf16().count() as f64;
                    self.grow(&[o[0] + xd[0] * len * 0.6, o[1] + xd[1] * len * 0.6, o[2] + xd[2] * len * 0.6]);
                    if let Some(out) = self.batches.get_mut(bi) {
                        out.texts.push(Text3 { owner: own, s: ln.s.clone(), o, xd, yd, ha: ln.ha, va: ln.va });
                    }
                }
            }
            Kind::Poly3 { verts, closed } => {
                let spline_fit = verts.iter().any(|(_, f)| js::int32(*f) as i64 & 8 != 0);
                let mut pts: Vec<P> =
                    verts.iter().filter(|(_, f)| !spline_fit || js::int32(*f) as i64 & 16 == 0).map(|(p, _)| w(p)).collect();
                if *closed
                    && pts.len() > 2
                    && let Some(f) = pts.first().copied()
                {
                    pts.push(f);
                }
                self.path(bi, own, &pts);
            }
            Kind::Face3 { pts, inv } => {
                let c4: [P; 4] = [w(&pts[0]), w(&pts[1]), w(&pts[2]), w(&pts[3])];
                let same = |p: &P, q: &P| p[0] == q[0] && p[1] == q[1] && p[2] == q[2];
                self.tri(bi, own, &c4[0], &c4[1], &c4[2]);
                if !same(&c4[2], &c4[3]) {
                    self.tri(bi, own, &c4[0], &c4[2], &c4[3]);
                }
                let bits = js::int32(*inv) as i64;
                for (i, (a, b)) in [(c4[0], c4[1]), (c4[1], c4[2]), (c4[2], c4[3]), (c4[3], c4[0])].into_iter().enumerate() {
                    if bits & (1 << i) == 0 && !same(&a, &b) {
                        self.seg(bi, own, &a, &b);
                    }
                }
            }
            Kind::PFace { verts, faces } => {
                let vv: Vec<P> = verts.iter().map(w).collect();
                for f in faces {
                    let Some(f) = f else { continue };
                    let idx: Vec<f64> = f.iter().copied().filter(|i| *i != 0.0).collect();
                    let pts: Vec<Option<P>> = idx
                        .iter()
                        .map(|i| {
                            let k = i.abs() - 1.0;
                            if k >= 0.0 && k.fract() == 0.0 { vv.get(k as usize).copied() } else { None }
                        })
                        .collect();
                    if pts.len() < 3 || pts.iter().any(Option::is_none) {
                        continue;
                    }
                    let pts: Vec<P> = pts.into_iter().flatten().collect();
                    let Some(&p0) = pts.first() else { continue };
                    for w2 in pts.get(1..).unwrap_or(&[]).windows(2) {
                        if let [a, b] = w2 {
                            self.tri(bi, own, &p0, a, b);
                        }
                    }
                    for (k, i) in idx.iter().enumerate() {
                        if *i > 0.0
                            && let (Some(a), Some(b)) = (pts.get(k), pts.get((k + 1) % pts.len()))
                        {
                            self.seg(bi, own, a, b);
                        }
                    }
                }
            }
            Kind::Mesh { m: mm, n: nn, verts, closed_m, closed_n } => {
                let (mi_, ni_) = (js::int32(*mm), js::int32(*nn));
                let vv: Vec<P> = verts.iter().map(w).collect();
                if mi_ < 2.0 || ni_ < 2.0 || (vv.len() as f64) < mi_ * ni_ {
                    count(&mut self.stats.not_drawn, &tr("dwg.skip.incomplete", &[("type", "POLYGON MESH".into())]));
                    return;
                }
                let (mu, nu) = (mi_ as usize, ni_ as usize);
                let at = |i: usize, j: usize| vv.get((i % mu) * nu + (j % nu)).copied().unwrap_or([f64::NAN; 3]);
                let mi = if *closed_m { mu } else { mu - 1 };
                let nj = if *closed_n { nu } else { nu - 1 };
                for i in 0..mi {
                    for j in 0..nj {
                        self.quad(bi, own, &at(i, j), &at(i + 1, j), &at(i + 1, j + 1), &at(i, j + 1));
                    }
                }
                for i in 0..mu {
                    for j in 0..nj {
                        self.seg(bi, own, &at(i, j), &at(i, j + 1));
                    }
                }
                for j in 0..nu {
                    for i in 0..mi {
                        self.seg(bi, own, &at(i, j), &at(i + 1, j));
                    }
                }
            }
            Kind::Circle { center, radius, thickness, ext } | Kind::Arc { center, radius, thickness, ext, .. } => {
                let om = mul3(&m, &ocs_matrix(ext));
                let z = center[2];
                let circle = matches!(e.kind, Kind::Circle { .. });
                let flat = match &e.kind {
                    Kind::Arc { sa, ea, .. } => arc_points(center[0], center[1], *radius, *sa, *ea, true),
                    _ => arc_points(center[0], center[1], *radius, 0.0, TAU, true),
                };
                let pts = ocs_pts(&flat, z, &om);
                let up = thick(*thickness, ext);
                let n = pts.len();
                let verts: Vec<usize> = if circle { vec![0, n / 4, n / 2, 3 * n / 4] } else { vec![0, n.saturating_sub(1)] };
                let cap = if circle && up.is_some() { Some(apply3(&om, center[0], center[1], z)) } else { None };
                self.extrude(bi, own, &pts, up, &verts, cap);
            }
            Kind::Ellipse { center, major, ratio, sp, ep, ext } => {
                let n = normal(ext);
                let (mj, c0) = (major, center);
                let mn =
                    [(n[1] * mj[2] - n[2] * mj[1]) * ratio, (n[2] * mj[0] - n[0] * mj[2]) * ratio, (n[0] * mj[1] - n[1] * mj[0]) * ratio];
                let sweep = wrap_sweep(ep - sp);
                let k = (sweep / TAU * 72.0).ceil();
                let cnt = if k.is_nan() { f64::NAN } else { k.clamp(8.0, 128.0) };
                let mut pts = Vec::new();
                if cnt.is_finite() {
                    let mut i = 0.0;
                    while i <= cnt {
                        let t = sp + sweep * (i / cnt);
                        let (co, si) = (t.jcos(), t.jsin());
                        pts.push(apply3(
                            &m,
                            c0[0] + co * mj[0] + si * mn[0],
                            c0[1] + co * mj[1] + si * mn[1],
                            c0[2] + co * mj[2] + si * mn[2],
                        ));
                        i += 1.0;
                    }
                }
                self.path(bi, own, &pts);
            }
            Kind::Spline { pts } => {
                let p: Vec<P> = pts.iter().map(|q| apply3(&m, q[0], q[1], q[2])).collect();
                self.path(bi, own, &p);
            }
            Kind::Path { verts } => {
                let p: Vec<P> = verts.iter().map(w).collect();
                self.path(bi, own, &p);
            }
            Kind::Solid { pts, elevation, ext } => {
                let om = mul3(&m, &ocs_matrix(ext));
                let z = elevation.unwrap_or(0.0);
                let q: [P; 4] = [pts[0], pts[1], pts[3], pts[2]].map(|p| apply3(&om, p[0], p[1], z));
                self.tri(bi, own, &q[0], &q[1], &q[2]);
                self.tri(bi, own, &q[0], &q[2], &q[3]);
                self.path(bi, own, &[q[0], q[1], q[2], q[3], q[0]]);
            }
            Kind::Hatch { rings, solid, elevation, ext } => {
                let om = mul3(&m, &ocs_matrix(ext));
                for r in rings {
                    let mut pts = ocs_pts(r, *elevation, &om);
                    if pts.len() > 2 {
                        if let Some(f) = pts.first().copied() {
                            pts.push(f);
                        }
                        self.path(bi, own, &pts);
                    }
                }
                if !solid {
                    count(&mut self.stats.flat_only, "HATCH pattern");
                }
            }
            Kind::Insert { block, insert, scale, rotation, ext, cols, rows, col_sp, row_sp, attribs } => {
                let Some(blk) = ctx.blocks.get(block) else {
                    count(&mut self.stats.not_drawn, &tr("dwg.skip.missing_block", &[("type", "INSERT".into())]));
                    return;
                };
                if ctx.depth > 20 {
                    count(&mut self.stats.not_drawn, &tr("dwg.skip.nested", &[("type", "INSERT".into())]));
                    return;
                }
                if blk.xref_missing {
                    return;
                }
                let x = blk.xref.as_deref();
                let base = x.map_or(blk.base, |x| x.base);
                let content: &[Ent] = x.map_or(&blk.entities, |x| &x.model);
                let om = mul3(&m, &ocs_matrix(ext));
                let nr = if rows.is_nan() { 0.0 } else { rows.max(1.0) };
                let nc = if cols.is_nan() { 0.0 } else { cols.max(1.0) };
                let xl: Option<HashMap<String, (Color, bool)>> =
                    x.map(|x| x.layers.iter().map(|l| (l.name.clone(), (l.color, l.on))).collect());
                let mut copies = 0.0;
                let mut r = 0.0;
                'rows: while r < nr {
                    let mut k = 0.0;
                    while k < nc {
                        copies += 1.0;
                        if copies > MAX_ARRAY || self.full() {
                            break 'rows;
                        }
                        let local = mul3(
                            &mul3(&t3(insert[0], insert[1], insert[2]), &rz(*rotation)),
                            &mul3(
                                &t3(k * col_sp, r * row_sp, 0.0),
                                &mul3(&s3(scale[0], scale[1], scale[2]), &t3(-base[0], -base[1], -base[2])),
                            ),
                        );
                        let sub = Ctx {
                            m: mul3(&om, &local),
                            layer: Some(layer.clone()),
                            color: Some(color),
                            layer_color: Some(lay_color),
                            depth: ctx.depth.saturating_add(1),
                            owner: ctx.owner,
                            blocks: x.map_or(ctx.blocks, |x| &x.blocks),
                            xp: if x.is_some() { format!("{block}|") } else { ctx.xp.clone() },
                            xlayers: if x.is_some() { xl.clone() } else { ctx.xlayers.clone() },
                        };
                        for be in content {
                            self.emit(be, &sub);
                        }
                        k += 1.0;
                    }
                    r += 1.0;
                }
                for at in attribs {
                    self.emit(at, ctx);
                }
            }
            Kind::Block { block, insert } => {
                let blk = block.as_ref().and_then(|b| ctx.blocks.get(b));
                let Some(blk) = blk else {
                    count(&mut self.stats.not_drawn, ty);
                    return;
                };
                if ctx.depth > MAX_DEPTH {
                    return;
                }
                let sub = Ctx {
                    m: mul3(&m, &t3(insert[0], insert[1], insert[2])),
                    layer: Some(layer.clone()),
                    color: Some(color),
                    layer_color: Some(lay_color),
                    depth: ctx.depth.saturating_add(1),
                    owner: ctx.owner,
                    blocks: ctx.blocks,
                    xp: ctx.xp.clone(),
                    xlayers: ctx.xlayers.clone(),
                };
                for be in &blk.entities {
                    self.emit(be, &sub);
                }
            }
            Kind::Other => {
                drawn = false;
                if FLAT_ONLY.contains(&ty) {
                    count(&mut self.stats.flat_only, ty);
                } else if !SILENT.contains(&ty) {
                    let name = if ACIS.contains(&ty) {
                        format!("{ty} (ACIS)")
                    } else if ty == "MESH" {
                        tr("dwg.skip.subdivision", &[("type", ty.to_string())])
                    } else {
                        ty.to_string()
                    };
                    count(&mut self.stats.not_drawn, &name);
                }
            }
        }
        if drawn {
            count(&mut self.stats.drawn, ty);
        }
    }
}

/// A `{x, y, bulge}` object for bulgePolyline.
fn pv_obj(x: Option<V>, y: Option<V>, bulge: f64) -> V {
    let mut o = Obj::new();
    if let Some(x) = x {
        o.set("x", x);
    }
    if let Some(y) = y {
        o.set("y", y);
    }
    o.set("bulge", V::Num(bulge));
    V::Obj(o)
}

/// `compileModel3d(model, { xrefs })`: `xrefs` = linked drawings (name in lower case → model).
pub fn compile_model3d(model: &Obj, xrefs: &[(String, Obj)]) -> Model3 {
    let sc = scene(model, xrefs, 0);
    let mut c = Compiler {
        layers: sc.layers.clone(),
        layer_index: HashMap::new(),
        stats: Stats3::default(),
        bmin: [f64::INFINITY; 3],
        bmax: [f64::NEG_INFINITY; 3],
        batches: Vec::new(),
        batch_index: HashMap::new(),
    };
    // `new Map(sc.layers.map(...))`: a later layer of the same name replaces the earlier one in place
    let mut uniq: Vec<LayerDef> = Vec::new();
    for l in &sc.layers {
        match uniq.iter_mut().find(|u| u.name == l.name) {
            Some(u) => *u = l.clone(),
            None => uniq.push(l.clone()),
        }
    }
    c.layers = uniq;
    c.layer_index = c.layers.iter().enumerate().map(|(i, l)| (l.name.clone(), i)).collect();
    let mut top = Ctx {
        m: I3,
        layer: None,
        color: None,
        layer_color: None,
        depth: 0,
        owner: 0,
        blocks: &sc.blocks,
        xp: String::new(),
        xlayers: None,
    };
    for (i, e) in sc.model.iter().enumerate() {
        c.stats.entities = c.stats.entities.saturating_add(1);
        top.owner = u32::try_from(i).unwrap_or(u32::MAX);
        c.emit(e, &top);
    }
    let batches: Vec<Batch3> = c.batches.into_iter().filter(|b| !b.lines.is_empty() || !b.tris.is_empty() || !b.texts.is_empty()).collect();
    let mut layers: Vec<Layer3> = c.layers.into_iter().map(|l| Layer3 { name: l.name, color: l.color, on: l.on }).collect();
    layers.sort_by(|a, b| cc_io_dwg::compile::collate(&a.name, &b.name));
    let owners = sc
        .model
        .iter()
        .map(|e| Owner { ty: e.ty.clone(), handle: e.handle.as_ref().map(|h| jsv::to_str(Some(h))), layer: e.layer.clone() })
        .collect();
    Model3 { units: sc.units.to_string(), batches, owners, min: c.bmin, max: c.bmax, layers, stats: c.stats }
}

impl Model3 {
    /// Is there anything to show (min ≤ max)?
    pub fn has_bounds(&self) -> bool {
        self.min[0] <= self.max[0]
    }
}
