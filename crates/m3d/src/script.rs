//! Op scripts: a list of {op, path?, args} calls on a Model (kernel functions on the definition open at `path`, and
//! Model methods), run as forks/tools/mdiff.mjs runs them on the frozen JS Model. After each step the runner records
//! `model.toJSON()`, the op's return value (as JSON), its error (the JS message; `$TypeError` for an engine error),
//! the undo / redo labels, the version, dirty and previewing. Function-level cases (`run_cases`) call the pure
//! functions. Everything here is hostile input: unknown ops and malformed args are errors, never panics.
//!
//! Edit ops run inside `model.change(op, …)` (one undo step each, as the app does); history ops (undo, redo,
//! amendLast, begin / preview / commit / abort, reload) and queries run as they are.

#![deny(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use serde_json::Value;

use crate::def::*;
use crate::edges::*;
use crate::error::{Error, Res};
use crate::file::{clean_light, clean_scene, clean_storeys, light_json, scene_json};
use crate::js;
use crate::json::{self, Out};
use crate::mesh::{edges_def, intersect_faces, mesh_def, world_triangles};
use crate::model::{Dim, Model, SceneVis, hidden_count, unhide_all};
use crate::offset::{ChainOffset, offset_chain_pts, offset_face, offset_face_pts, offset2};
use crate::omap::IdSet;
use crate::ops::*;
use crate::paint::{image_paint, image_paint_fit, image_uv, is_paint, paint_faces, paint_instance};
use crate::solids::{fix_solid, solid_info, solid_problems};
use crate::vec::*;

/// Most steps a script may have.
pub const MAX_STEPS: usize = 1_000_000;

// ---------------------------------------------------------------------------------------------------------------
// arguments

fn arg(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i)
}
fn num(v: Option<&Value>) -> Res<f64> {
    v.and_then(Value::as_f64).ok_or(Error::Type)
}
fn num_or_nan(v: Option<&Value>) -> f64 {
    v.and_then(Value::as_f64).unwrap_or(f64::NAN)
}
fn p3(v: Option<&Value>) -> Res<P3> {
    let a = v.and_then(Value::as_array).ok_or(Error::Type)?;
    Ok([num_or_nan(a.first()), num_or_nan(a.get(1)), num_or_nan(a.get(2))])
}
fn opt_p3(v: Option<&Value>) -> Res<Option<P3>> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(x) => p3(Some(x)).map(Some),
    }
}
fn pts(v: Option<&Value>) -> Res<Vec<P3>> {
    let a = v.and_then(Value::as_array).ok_or(Error::Type)?;
    if a.len() > 1_000_000 {
        return Err(Error::Type);
    }
    a.iter().map(|x| p3(Some(x))).collect()
}
fn ids(v: Option<&Value>) -> Res<Vec<f64>> {
    let a = v.and_then(Value::as_array).ok_or(Error::Type)?;
    Ok(a.iter().map(|x| num_or_nan(Some(x))).collect())
}
fn m16(v: Option<&Value>) -> Res<M4> {
    let a = v.and_then(Value::as_array).ok_or(Error::Type)?;
    let mut m = [0.0; 16];
    for (k, s) in m.iter_mut().enumerate() {
        *s = num_or_nan(a.get(k));
    }
    Ok(m)
}
fn sel(v: Option<&Value>) -> Res<Vec<Sel>> {
    let a = v.and_then(Value::as_array).ok_or(Error::Type)?;
    Ok(a.iter().map(|s| Sel { t: json::str(s.get("t")).unwrap_or("").to_string(), id: num_or_nan(s.get("id")) }).collect())
}
fn string(v: Option<&Value>) -> Res<String> {
    v.and_then(Value::as_str).map(str::to_string).ok_or(Error::Type)
}
fn opt_str(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str)
}
fn boolean(v: Option<&Value>) -> bool {
    json::truthy(v)
}
fn path_of(v: Option<&Value>) -> Vec<f64> {
    v.and_then(Value::as_array).map(|a| a.iter().map(|x| num_or_nan(Some(x))).collect()).unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------------------------
// results as JSON (the parity kit's canonical numbers: non-finite as "$NaN" / "$Inf" / "$-Inf")

fn o() -> Out {
    Out::new(true)
}
fn r_num(x: f64) -> String {
    let mut w = o();
    w.num(x);
    w.s
}
fn r_bool(b: bool) -> String {
    b.to_string()
}
fn r_str(s: &str) -> String {
    let mut w = o();
    w.str(s);
    w.s
}
fn r_opt_num(x: Option<f64>) -> String {
    x.map_or_else(|| "null".into(), r_num)
}
fn r_opt_str(x: Option<&str>) -> String {
    x.map_or_else(|| "null".into(), r_str)
}
fn r_ids(v: &[f64]) -> String {
    let mut w = o();
    w.nums(v);
    w.s
}
fn r_p3(p: &P3) -> String {
    let mut w = o();
    w.p3(p);
    w.s
}
fn r_pts(v: &[P3]) -> String {
    let mut w = o();
    w.ch('[');
    for (i, p) in v.iter().enumerate() {
        if i > 0 {
            w.ch(',');
        }
        w.p3(p);
    }
    w.ch(']');
    w.s
}
fn r_sel(v: &[Sel]) -> String {
    let mut w = o();
    w.ch('[');
    for (i, s) in v.iter().enumerate() {
        if i > 0 {
            w.ch(',');
        }
        w.raw("{\"t\":");
        w.str(&s.t);
        w.raw(",\"id\":");
        w.num(s.id);
        w.ch('}');
    }
    w.ch(']');
    w.s
}
fn r_push(r: &PushResult) -> String {
    if !r.ok {
        return "{\"ok\":false}".into();
    }
    format!("{{\"ok\":true,\"mode\":{},\"distance\":{},\"face\":{}}}", r_str(r.mode), r_num(r.distance), r_opt_num(r.face))
}
fn r_curve(c: &Option<CurveInfo>) -> String {
    let Some(c) = c else { return "null".into() };
    format!(
        "{{\"id\":{},\"kind\":{},\"edges\":{},\"verts\":{},\"closed\":{},\"length\":{},\"segs\":{},\"round\":{},\"center\":{},\"normal\":{},\"radius\":{},\"flat\":{},\"sweep\":{}}}",
        r_str(&c.id),
        r_str(&c.kind),
        r_ids(&c.edges),
        r_ids(&c.verts),
        c.closed,
        r_num(c.length),
        r_num(c.segs),
        c.round,
        c.center.as_ref().map_or_else(|| "null".into(), r_p3),
        c.normal.as_ref().map_or_else(|| "null".into(), r_p3),
        r_num(c.radius),
        c.flat,
        r_num(c.sweep)
    )
}
fn r_chain(c: &ChainOffset) -> String {
    format!("{{\"pts\":{},\"closed\":{},\"chain\":{}}}", r_pts(&c.pts), c.closed, r_ids(&c.chain))
}
fn r_vis(v: &SceneVis) -> String {
    let mut w = o();
    w.raw("{\"insts\":[");
    for (i, p) in v.insts.iter().enumerate() {
        if i > 0 {
            w.ch(',');
        }
        w.nums(p);
    }
    w.raw("],\"tags\":");
    w.strs(&v.tags);
    w.raw(",\"section\":");
    match v.section {
        Some(x) => w.num(x),
        None => w.raw("null"),
    }
    w.raw(",\"cuts\":");
    w.bool(v.cuts);
    w.ch('}');
    w.s
}
fn r_value(v: &Value) -> String {
    let mut w = o();
    w.value(v);
    w.s
}
fn r_bbox(b: Option<BBox>) -> String {
    match b {
        Some(b) => format!("{{\"min\":{},\"max\":{}}}", r_p3(&b.min), r_p3(&b.max)),
        None => "null".into(),
    }
}
fn r_tris(t: &[[f64; 3]]) -> String {
    let mut w = o();
    w.ch('[');
    for (i, x) in t.iter().enumerate() {
        if i > 0 {
            w.ch(',');
        }
        w.nums(x);
    }
    w.ch(']');
    w.s
}

/// The vis a step names: a register (`{"$vis": step}`) or a literal.
fn vis_of(v: Option<&Value>, regs: &Regs) -> Res<SceneVis> {
    let v = v.ok_or(Error::Type)?;
    if let Some(k) = json::num(v.get("$vis")) {
        return regs.vis.iter().find(|(s, _)| *s == k).map(|(_, x)| x.clone()).ok_or(Error::Type);
    }
    let insts = v
        .get("insts")
        .and_then(Value::as_array)
        .ok_or(Error::Type)?
        .iter()
        .map(|p| p.as_array().map(|a| a.iter().map(|x| num_or_nan(Some(x))).collect()).ok_or(Error::Type))
        .collect::<Res<Vec<Vec<f64>>>>()?;
    let tags = v.get("tags").and_then(Value::as_array).ok_or(Error::Type)?.iter().map(|x| string(Some(x))).collect::<Res<Vec<String>>>()?;
    let section = match v.get("section") {
        None | Some(Value::Null) => None,
        Some(x) => x.as_f64(),
    };
    Ok(SceneVis { insts, tags, section, cuts: json::not_false(v.get("cuts")) })
}

/// What a script keeps between steps: clipboards, undo steps and captured scene states, by the step that made them.
#[derive(Default)]
pub struct Regs {
    pub clips: Vec<(f64, Value)>,
    pub steps: Vec<(f64, Option<u64>)>,
    pub vis: Vec<(f64, SceneVis)>,
    /// The benchmark: the big queries' results are made but not written out (the JS side writes nothing either).
    pub quiet: bool,
}

/// The ops that change the model: run inside `model.change(op, …)`.
const EDIT: &[&str] = &[
    "addEdge",
    "addPolygon",
    "rect",
    "ngon",
    "arc",
    "line",
    "pushPull",
    "followMe",
    "offsetFace",
    "offsetChain",
    "eraseEdges",
    "eraseSel",
    "transformSel",
    "move",
    "rotate",
    "scale",
    "flip",
    "copyMove",
    "copySel",
    "reverseFaces",
    "softenEdges",
    "mergeCoplanar",
    "stretch",
    "hideSel",
    "isolateSel",
    "paintFaces",
    "paintInstance",
    "addSpline",
    "editSpline",
    "setCurveRadius",
    "tagCurve",
    "vertexAt",
    "splitEdge",
    "cleanup",
    "removeLooseVerts",
    "healCollinear",
    "fixSolid",
    "intersectFaces",
    "meshDef",
    "edgesDef",
    "makeGroup",
    "explode",
    "makeUnique",
    "purge",
    "place",
    "moveInto",
    "pasteClip",
    "addTag",
    "renameTag",
    "deleteTag",
    "setTagHidden",
    "setTag",
    "ensureTags",
    "addSection",
    "deleteSection",
    "setAxes",
    "resetAxes",
    "addDim",
    "addRadialDim",
    "addAngleDim",
    "setDimLength",
    "setEdgeLength",
    "setDefSize",
    "addGuideLine",
    "addGuidePoint",
    "addLight",
    "setLight",
    "deleteLight",
    "addScene",
    "setStoreys",
    "unhideAll",
];

/// One step's outcome: the return value as JSON (None = undefined) or the error text.
pub type Outcome = Result<Option<String>, String>;

fn err_text(e: &Error) -> String {
    e.to_string()
}

/// The definition open at the step's path.
fn ctx_def(m: &Model, path: &[f64]) -> Res<String> {
    m.context(path).map(|c| c.def).ok_or(Error::Type)
}
fn with_def<R>(m: &mut Model, path: &[f64], f: impl FnOnce(&mut Def) -> Res<R>) -> Res<R> {
    let id = ctx_def(m, path)?;
    let d = m.def_mut(&id).ok_or(Error::Type)?;
    f(d)
}

/// Run one step (`wrap`: edit ops inside model.change).
pub fn exec(m: &mut Model, regs: &mut Regs, step: &Value, index: f64, wrap: bool) -> Res<Option<String>> {
    let op = json::str(step.get("op")).ok_or(Error::Type)?.to_string();
    if wrap && EDIT.contains(&op.as_str()) {
        let s = step.clone();
        let label = op.clone();
        // the clipboard registers are read inside
        let clips = std::mem::take(&mut regs.clips);
        let r = m.change(&label, |mm| {
            let mut inner = Regs { clips, steps: Vec::new(), vis: Vec::new(), quiet: regs.quiet };
            let r = run_op(mm, &mut inner, &s, index);
            regs.clips = std::mem::take(&mut inner.clips);
            r
        });
        return r;
    }
    run_op(m, regs, step, index)
}

#[allow(clippy::too_many_lines)]
fn run_op(m: &mut Model, regs: &mut Regs, step: &Value, index: f64) -> Res<Option<String>> {
    let op = json::str(step.get("op")).ok_or(Error::Type)?;
    let path = path_of(step.get("path"));
    let empty = Vec::new();
    let a = step.get("args").and_then(Value::as_array).unwrap_or(&empty);
    let a = a.as_slice();
    let some = |s: String| Ok(Some(s));
    match op {
        // ---- definition edits ----
        "addEdge" => {
            let (p, q) = (p3(arg(a, 0))?, p3(arg(a, 1))?);
            let o = arg(a, 2);
            let opts = EdgeOpts {
                no_faces: matches!(o.and_then(|o| o.get("faces")), Some(Value::Bool(false))),
                hint: opt_p3(o.and_then(|o| o.get("hint")))?,
            };
            with_def(m, &path, |d| some(r_ids(&add_edge(d, p, q, opts))))
        }
        "addPolygon" => {
            let (pp, hint) = (pts(arg(a, 0))?, opt_p3(arg(a, 1))?);
            with_def(m, &path, |d| Ok(Some(r_ids(&add_polygon(d, &pp, hint)?))))
        }
        "rect" => {
            let pp = rect_pts(p3(arg(a, 0))?, p3(arg(a, 1))?, p3(arg(a, 2))?, num(arg(a, 3))?, num(arg(a, 4))?);
            let hint = opt_p3(arg(a, 5))?;
            with_def(m, &path, |d| Ok(Some(r_ids(&add_polygon(d, &pp, hint)?))))
        }
        "ngon" => {
            let n = p3(arg(a, 1))?;
            let pp = ngon_pts(p3(arg(a, 0))?, n, p3(arg(a, 2))?, num(arg(a, 3))?, num(arg(a, 4))?.min(100_000.0));
            let kind = opt_str(arg(a, 5)).map(str::to_string);
            with_def(m, &path, |d| {
                let ids = add_polygon(d, &pp, Some(n))?;
                let cid = kind.filter(|k| !k.is_empty()).map(|k| tag_curve(d, &pp, &k, true));
                Ok(Some(format!("{{\"ids\":{},\"cid\":{}}}", r_ids(&ids), r_opt_str(cid.as_deref()))))
            })
        }
        "arc" => {
            let n = p3(arg(a, 1))?;
            let pp = arc_pts(p3(arg(a, 0))?, n, p3(arg(a, 2))?, num(arg(a, 3))?, num(arg(a, 4))?, num(arg(a, 5))?.min(100_000.0));
            with_def(m, &path, |d| {
                let mut i = 0usize;
                while i.saturating_add(1) < pp.len() {
                    let (Some(&x), Some(&y)) = (pp.get(i), pp.get(i.saturating_add(1))) else { break };
                    add_edge(d, x, y, EdgeOpts { no_faces: false, hint: Some(n) });
                    i = i.saturating_add(1);
                }
                some(r_str(&tag_curve(d, &pp, "arc", false)))
            })
        }
        "line" => {
            let (pp, closed) = (pts(arg(a, 0))?, boolean(arg(a, 1)));
            with_def(m, &path, |d| {
                let mut out = Vec::new();
                let n = pp.len();
                let k = if closed { n } else { n.saturating_sub(1) };
                for i in 0..k {
                    let (Some(&x), Some(&y)) = (pp.get(i), pp.get(i.saturating_add(1).checked_rem(n).unwrap_or(0))) else { break };
                    out.extend(add_edge(d, x, y, EdgeOpts::default()));
                }
                some(r_ids(&out))
            })
        }
        "pushPull" => {
            let (f, dd) = (num(arg(a, 0))?, num(arg(a, 1))?);
            with_def(m, &path, |d| some(r_push(&push_pull(d, f, dd))))
        }
        "followMe" => {
            let (f, e) = (num(arg(a, 0))?, ids(arg(a, 1))?);
            with_def(m, &path, |d| {
                let r = follow_me(d, f, &e)?;
                Ok(Some(format!("{{\"faces\":{},\"closed\":{}}}", r_ids(&r.faces), r.closed)))
            })
        }
        "offsetFace" => {
            let (f, dd) = (num(arg(a, 0))?, num(arg(a, 1))?);
            with_def(m, &path, |d| Ok(Some(format!("{{\"face\":{}}}", r_opt_num(offset_face(d, f, dd)?)))))
        }
        "offsetChain" => {
            let (e, dd, n) = (ids(arg(a, 0))?, num(arg(a, 1))?, p3(arg(a, 2))?);
            with_def(m, &path, |d| {
                let r = offset_chain_pts(d, &e, dd, n)?;
                let k = r.pts.len();
                let segs = if r.closed { k } else { k.saturating_sub(1) };
                for i in 0..segs {
                    let (Some(&x), Some(&y)) = (r.pts.get(i), r.pts.get(i.saturating_add(1).checked_rem(k).unwrap_or(0))) else { break };
                    add_edge(d, x, y, EdgeOpts::default());
                }
                some(r_chain(&r))
            })
        }
        "eraseEdges" => {
            let e = ids(arg(a, 0))?;
            with_def(m, &path, |d| {
                erase_edges(d, &e);
                Ok(None)
            })
        }
        "eraseSel" => {
            let s = sel(arg(a, 0))?;
            with_def(m, &path, |d| {
                erase_sel(d, &s);
                Ok(None)
            })
        }
        "transformSel" | "move" | "rotate" | "scale" => {
            let s = sel(arg(a, 0))?;
            let mm = match op {
                "transformSel" => m16(arg(a, 1))?,
                "move" => translation(p3(arg(a, 1))?),
                "rotate" => rotation(p3(arg(a, 1))?, num(arg(a, 2))?, p3(arg(a, 3))?),
                _ => scaling(num(arg(a, 1))?, p3(arg(a, 2))?),
            };
            with_def(m, &path, |d| {
                transform_sel(d, &s, &mm);
                Ok(None)
            })
        }
        "flip" => {
            let (s, c, n, copy) = (sel(arg(a, 0))?, p3(arg(a, 1))?, p3(arg(a, 2))?, boolean(arg(a, 3)));
            with_def(m, &path, |d| some(r_sel(&flip_sel(d, &s, c, n, copy))))
        }
        "copyMove" => {
            let (s, v) = (sel(arg(a, 0))?, p3(arg(a, 1))?);
            with_def(m, &path, |d| {
                let c = copy_sel(d, &s);
                transform_sel(d, &c, &translation(v));
                some(r_sel(&c))
            })
        }
        "copySel" => {
            let s = sel(arg(a, 0))?;
            with_def(m, &path, |d| some(r_sel(&copy_sel(d, &s))))
        }
        "reverseFaces" => {
            let f = ids(arg(a, 0))?;
            with_def(m, &path, |d| {
                reverse_faces(d, &f);
                Ok(None)
            })
        }
        "softenEdges" => {
            let (e, deg) = (ids(arg(a, 0))?, num(arg(a, 1))?);
            with_def(m, &path, |d| some(r_num(soften_edges(d, &e, deg))))
        }
        "mergeCoplanar" => {
            let e = ids(arg(a, 0))?;
            with_def(m, &path, |d| {
                merge_coplanar(d, &e);
                Ok(None)
            })
        }
        "stretch" => {
            let (dir, cut, delta) = (p3(arg(a, 0))?, num(arg(a, 1))?, num(arg(a, 2))?);
            let only = match arg(a, 3) {
                None | Some(Value::Null) => None,
                Some(v) => Some(IdSet::from_iter_ids(ids(Some(v))?)),
            };
            let id = ctx_def(m, &path)?;
            let boxes: Vec<(String, Option<BBox>)> =
                m.def(&id).map(|d| d.insts.values().map(|i| (i.def.clone(), m.bbox(&i.def, &IDENT))).collect()).unwrap_or_default();
            let d = m.def_mut(&id).ok_or(Error::Type)?;
            let n = stretch(d, dir, cut, delta, only.as_ref(), &|k| boxes.iter().find(|(x, _)| x == k).and_then(|(_, b)| *b))?;
            some(r_num(n))
        }
        "hideSel" => {
            let (s, on) = (sel(arg(a, 0))?, boolean(arg(a, 1)));
            with_def(m, &path, |d| {
                hide_sel(d, &s, on);
                Ok(None)
            })
        }
        "isolateSel" => {
            let s = sel(arg(a, 0))?;
            with_def(m, &path, |d| some(r_num(isolate_sel(d, &s))))
        }
        "paintFaces" => {
            let (f, c) = (ids(arg(a, 0))?, opt_str(arg(a, 1)).map(str::to_string));
            with_def(m, &path, |d| {
                paint_faces(d, &f, c.as_deref())?;
                Ok(None)
            })
        }
        "paintInstance" => {
            let (i, c) = (num(arg(a, 0))?, opt_str(arg(a, 1)).map(str::to_string));
            with_def(m, &path, |d| {
                paint_instance(d, i, c.as_deref())?;
                Ok(None)
            })
        }
        "addSpline" => {
            let (pp, closed) = (pts(arg(a, 0))?, boolean(arg(a, 1)));
            let segs = json::num(arg(a, 2)).unwrap_or(SPLINE_SEGS).min(1000.0);
            with_def(m, &path, |d| Ok(Some(r_str(&add_spline(d, &pp, closed, segs, None)?))))
        }
        "editSpline" => {
            let (cid, pp) = (string(arg(a, 0))?, pts(arg(a, 1))?);
            let closed = arg(a, 2).and_then(Value::as_bool);
            let segs = json::num(arg(a, 3)).map(|s| s.min(1000.0));
            with_def(m, &path, |d| Ok(Some(r_str(&edit_spline(d, &cid, &pp, closed, segs)?))))
        }
        "setCurveRadius" => {
            let (cid, r) = (string(arg(a, 0))?, num(arg(a, 1))?);
            with_def(m, &path, |d| Ok(Some(r_curve(&set_curve_radius(d, &cid, r)?))))
        }
        "tagCurve" => {
            let (pp, kind, closed) = (pts(arg(a, 0))?, string(arg(a, 1))?, boolean(arg(a, 2)));
            with_def(m, &path, |d| some(r_str(&tag_curve(d, &pp, &kind, closed))))
        }
        "vertexAt" => {
            let p = p3(arg(a, 0))?;
            with_def(m, &path, |d| some(r_num(vertex_at(d, p))))
        }
        "splitEdge" => {
            let (e, v) = (num(arg(a, 0))?, num(arg(a, 1))?);
            with_def(m, &path, |d| {
                split_edge(d, e, v);
                Ok(None)
            })
        }
        "cleanup" => with_def(m, &path, |d| some(r_bool(cleanup(d)))),
        "removeLooseVerts" => with_def(m, &path, |d| {
            remove_loose_verts(d);
            Ok(None)
        }),
        "healCollinear" => {
            let v = ids(arg(a, 0))?;
            with_def(m, &path, |d| {
                heal_collinear(d, &v);
                Ok(None)
            })
        }
        "fixSolid" => with_def(m, &path, |d| {
            let f = fix_solid(d);
            some(format!(
                "{{\"stray\":{},\"inner\":{},\"flipped\":{},\"holes\":{}}}",
                r_num(f.stray),
                r_num(f.inner),
                r_num(f.flipped),
                r_num(f.holes)
            ))
        }),
        "intersectFaces" => {
            let s = sel(arg(a, 0))?;
            let others = opt_str(arg(a, 1)).unwrap_or("model").to_string();
            some(r_num(intersect_faces(m, &path, &s, &others)))
        }
        "meshDef" => {
            let polys = arg(a, 0).and_then(Value::as_array).cloned().unwrap_or_default();
            let name = opt_str(arg(a, 1)).map(str::to_string);
            let kind = opt_str(arg(a, 2)).unwrap_or("group").to_string();
            let segs = arg(a, 3).and_then(Value::as_array).cloned().unwrap_or_default();
            some(r_str(&mesh_def(m, &polys, name.as_deref(), &kind, &segs)))
        }
        "edgesDef" => {
            let segs = arg(a, 0).and_then(Value::as_array).cloned().unwrap_or_default();
            let name = opt_str(arg(a, 1)).map(str::to_string);
            some(r_str(&edges_def(m, &segs, name.as_deref())))
        }
        // ---- model edits ----
        "makeGroup" => {
            let s = sel(arg(a, 0))?;
            let kind = opt_str(arg(a, 1)).unwrap_or("group").to_string();
            let name = opt_str(arg(a, 2)).map(str::to_string);
            let d = ctx_def(m, &path)?;
            some(r_opt_num(m.make_group(&d, &s, &kind, name.as_deref())?))
        }
        "explode" => {
            let i = num(arg(a, 0))?;
            let d = ctx_def(m, &path)?;
            some(r_sel(&m.explode(&d, i)?))
        }
        "makeUnique" => {
            let i = num(arg(a, 0))?;
            let d = ctx_def(m, &path)?;
            some(r_opt_str(m.make_unique(&d, i)?.as_deref()))
        }
        "purge" => {
            m.purge();
            Ok(None)
        }
        "place" => {
            let (def, mat) = (string(arg(a, 0))?, m16(arg(a, 1))?);
            let d = ctx_def(m, &path)?;
            let dd = m.def_mut(&d).ok_or(Error::Type)?;
            some(r_num(crate::model::place(dd, &def, &mat)))
        }
        "moveInto" => {
            let (from, i, to) = (path_of(arg(a, 0)), num(arg(a, 1))?, path_of(arg(a, 2)));
            some(r_num(m.move_into(&from, i, &to)?))
        }
        "pasteClip" => {
            let k = arg(a, 0).and_then(|v| json::num(v.get("$clip")));
            let clip = match k {
                Some(k) => regs.clips.iter().find(|(s, _)| *s == k).map(|(_, c)| c.clone()).unwrap_or(Value::Null),
                None => arg(a, 0).cloned().unwrap_or(Value::Null),
            };
            let mat = arg(a, 1).cloned().unwrap_or(Value::Null);
            some(r_sel(&m.paste_clip(&path, &clip, &mat)?))
        }
        "addTag" => some(r_str(&m.add_tag(opt_str(arg(a, 0)).unwrap_or("")))),
        "renameTag" => {
            let (old, name) = (string(arg(a, 0))?, opt_str(arg(a, 1)).unwrap_or("").to_string());
            some(r_str(&m.rename_tag(&old, &name)?))
        }
        "deleteTag" => some(r_num(m.delete_tag(&string(arg(a, 0))?))),
        "setTagHidden" => {
            m.set_tag_hidden(&string(arg(a, 0))?, boolean(arg(a, 1)));
            Ok(None)
        }
        "setTag" => {
            let (i, name) = (ids(arg(a, 0))?, opt_str(arg(a, 1)).map(str::to_string));
            let d = ctx_def(m, &path)?;
            m.set_tag(&d, &i, name.as_deref())?;
            Ok(None)
        }
        "ensureTags" => {
            m.ensure_tags();
            Ok(None)
        }
        "addSection" => {
            let (p, n) = (p3(arg(a, 0))?, p3(arg(a, 1))?);
            some(r_num(m.add_section(p, n, opt_str(arg(a, 2)).unwrap_or(""))))
        }
        "deleteSection" => {
            m.delete_section(num(arg(a, 0))?);
            Ok(None)
        }
        "setAxes" => {
            m.set_axes(p3(arg(a, 0))?, p3(arg(a, 1))?, p3(arg(a, 2))?)?;
            Ok(None)
        }
        "resetAxes" => {
            m.reset_axes();
            Ok(None)
        }
        "addDim" => {
            let (p, q, off, prefer) = (p3(arg(a, 0))?, p3(arg(a, 1))?, p3(arg(a, 2))?, path_of(arg(a, 3)));
            some(r_num(m.add_dim(p, q, off, &prefer)))
        }
        "addRadialDim" => {
            let (cp, cid, off) = (path_of(arg(a, 0)), string(arg(a, 1))?, p3(arg(a, 2))?);
            let kind = opt_str(arg(a, 3)).unwrap_or("rad").to_string();
            some(r_num(m.add_radial_dim(&cp, &cid, off, &kind)?))
        }
        "addAngleDim" => {
            let (p0, p1, p2, off, prefer) = (p3(arg(a, 0))?, p3(arg(a, 1))?, p3(arg(a, 2))?, p3(arg(a, 3))?, path_of(arg(a, 4)));
            some(r_num(m.add_angle_dim(p0, p1, p2, off, &prefer)))
        }
        "setDimLength" => some(r_num(m.set_dim_length(num(arg(a, 0))?, num(arg(a, 1))?)?)),
        "setEdgeLength" => {
            let (e, l) = (num(arg(a, 0))?, num(arg(a, 1))?);
            some(r_num(m.set_edge_length(&path, e, l)?))
        }
        "setDefSize" => {
            let (def, axis, size) = (string(arg(a, 0))?, num(arg(a, 1))?, num(arg(a, 2))?);
            let ax = if axis == 1.0 {
                1
            } else if axis == 2.0 {
                2
            } else {
                0
            };
            m.set_def_size(&def, ax, size)?;
            Ok(None)
        }
        "addGuideLine" => some(r_num(m.add_guide_line(p3(arg(a, 0))?, p3(arg(a, 1))?))),
        "addGuidePoint" => some(r_num(m.add_guide_point(p3(arg(a, 0))?))),
        "addLight" => some(r_num(m.add_light(arg(a, 0).unwrap_or(&Value::Null))?)),
        "setLight" => some(r_bool(m.set_light(num(arg(a, 0))?, arg(a, 1).unwrap_or(&Value::Null))?)),
        "deleteLight" => some(r_bool(m.delete_light(num(arg(a, 0))?))),
        "addScene" => {
            if let Some(s) = clean_scene(arg(a, 0).unwrap_or(&Value::Null)) {
                m.scenes.push(s);
            }
            Ok(None)
        }
        "setStoreys" => {
            m.storeys = clean_storeys(arg(a, 0));
            Ok(None)
        }
        "unhideAll" => some(r_num(unhide_all(m))),
        // ---- history ----
        "undo" => some(r_opt_str(m.undo()?.as_deref())),
        "redo" => some(r_opt_str(m.redo()?.as_deref())),
        "lastStep" => {
            let s = m.last_step();
            let label = m.undo_stack.last().map(|s| s.label.clone());
            regs.steps.push((index, s));
            some(r_opt_str(label.as_deref()))
        }
        "amendLast" => {
            let k = arg(a, 0).and_then(|v| json::num(v.get("$step")));
            let s = k.and_then(|k| regs.steps.iter().find(|(x, _)| *x == k).and_then(|(_, s)| *s));
            let inner = arg(a, 1).cloned().unwrap_or(Value::Null);
            let mut sub = Regs { clips: std::mem::take(&mut regs.clips), ..Regs::default() };
            let r = m.amend_last(s, |mm| run_op(mm, &mut sub, &inner, index).map(|_| ()));
            regs.clips = sub.clips;
            some(r_bool(r?))
        }
        "begin" => {
            m.begin();
            Ok(None)
        }
        "preview" => {
            let inner = arg(a, 0).cloned().unwrap_or(Value::Null);
            let scope: Option<Vec<String>> =
                arg(a, 1).and_then(Value::as_array).map(|s| s.iter().map(|x| json::str(Some(x)).unwrap_or("").to_string()).collect());
            let mut sub = Regs { clips: std::mem::take(&mut regs.clips), ..Regs::default() };
            let r = m.preview(scope.as_deref(), |mm| run_op(mm, &mut sub, &inner, index));
            regs.clips = sub.clips;
            match r? {
                Some(Some(s)) => some(s),
                // the frame's fn gave undefined: preview returns it
                Some(None) => Ok(None),
                None => some("null".into()),
            }
        }
        "commit" => {
            m.commit(opt_str(arg(a, 0)).unwrap_or(""))?;
            Ok(None)
        }
        "abort" => {
            m.abort()?;
            Ok(None)
        }
        "reload" => {
            let s = m.to_json_string(false);
            let j: Value = serde_json::from_str(&s).map_err(|_| Error::Type)?;
            m.load(&j, false)?;
            Ok(None)
        }
        "reset" => {
            m.reset();
            Ok(None)
        }
        "restoreSnapshot" => {
            let s = m.snapshot();
            m.restore(&crate::model::SnapRef::Str(s.into()))?;
            Ok(None)
        }
        "applyVis" => {
            let v = vis_of(arg(a, 0), regs)?;
            m.apply_vis(&v);
            Ok(None)
        }
        // ---- queries ----
        "faceTris" => {
            let f = num(arg(a, 0))?;
            let d = m.def(&ctx_def(m, &path)?).ok_or(Error::Type)?;
            let face = d.faces.get(f).ok_or(Error::Type)?;
            some(r_tris(&face_tris(d, face)))
        }
        "faceArea" | "faceNormal" => {
            let f = num(arg(a, 0))?;
            let d = m.def(&ctx_def(m, &path)?).ok_or(Error::Type)?;
            let face = d.faces.get(f).ok_or(Error::Type)?;
            if op == "faceArea" { some(r_num(face_area(d, face))) } else { some(r_p3(&face_normal(d, face))) }
        }
        "solidInfo" => {
            let d = m.def(&ctx_def(m, &path)?).ok_or(Error::Type)?;
            let s = solid_info(d);
            some(format!(
                "{{\"closed\":{},\"volume\":{},\"area\":{},\"faces\":{},\"edges\":{},\"verts\":{}}}",
                s.closed,
                r_opt_num(s.volume),
                r_num(s.area),
                r_num(s.faces),
                r_num(s.edges),
                r_num(s.verts)
            ))
        }
        "solidProblems" => {
            let d = m.def(&ctx_def(m, &path)?).ok_or(Error::Type)?;
            let p = solid_problems(d);
            if regs.quiet {
                return some(r_num(p.flipped.len() as f64));
            }
            some(format!(
                "{{\"holes\":{},\"extra\":{},\"inner\":{},\"stray\":{},\"flipped\":{}}}",
                r_ids(&p.holes),
                r_ids(&p.extra),
                r_ids(&p.inner),
                r_ids(&p.stray),
                r_ids(&p.flipped)
            ))
        }
        "smoothNormals" => {
            let d = m.def(&ctx_def(m, &path)?).ok_or(Error::Type)?;
            let sn = smooth_normals(d);
            if regs.quiet {
                return some(r_num(sn.len() as f64));
            }
            let mut w = o();
            w.ch('{');
            for (i, (k, v)) in sn.iter().enumerate() {
                if i > 0 {
                    w.ch(',');
                }
                w.key(k);
                w.p3(v);
            }
            w.ch('}');
            some(w.s)
        }
        "curveInfo" => {
            let cid = string(arg(a, 0))?;
            let d = m.def(&ctx_def(m, &path)?).ok_or(Error::Type)?;
            some(r_curve(&curve_info(d, &cid)))
        }
        "curveEdges" => {
            let cid = string(arg(a, 0))?;
            let d = m.def(&ctx_def(m, &path)?).ok_or(Error::Type)?;
            some(r_ids(&curve_edges(d, &cid)))
        }
        "connected" => {
            let s = sel(arg(a, 0))?;
            let d = m.def(&ctx_def(m, &path)?).ok_or(Error::Type)?;
            some(r_sel(&connected(d, &s)))
        }
        "connectedVerts" => {
            let v = num(arg(a, 0))?;
            let d = m.def(&ctx_def(m, &path)?).ok_or(Error::Type)?;
            some(r_ids(&connected_verts(d, v).to_vec()))
        }
        "selVerts" => {
            let s = sel(arg(a, 0))?;
            let d = m.def(&ctx_def(m, &path)?).ok_or(Error::Type)?;
            some(r_ids(&sel_verts(d, &s).to_vec()))
        }
        "faceWithEdges" => {
            let f = num(arg(a, 0))?;
            with_def(m, &path, |d| some(r_sel(&face_with_edges(d, f))))
        }
        "chainThrough" => {
            let e = num(arg(a, 0))?;
            let d = m.def(&ctx_def(m, &path)?).ok_or(Error::Type)?;
            some(r_ids(&chain_through(d, e)))
        }
        "edgeChain" => {
            let e = ids(arg(a, 0))?;
            let d = m.def(&ctx_def(m, &path)?).ok_or(Error::Type)?;
            let (v, c) = edge_chain(d, &e)?;
            some(format!("{{\"verts\":{},\"closed\":{}}}", r_ids(&v), c))
        }
        "depthBehind" => {
            let f = num(arg(a, 0))?;
            let d = m.def(&ctx_def(m, &path)?).ok_or(Error::Type)?;
            if !d.faces.has(f) {
                return Err(Error::Type);
            }
            some(r_num(depth_behind(d, f)))
        }
        "offsetFacePts" => {
            let (f, dd) = (num(arg(a, 0))?, num(arg(a, 1))?);
            let d = m.def(&ctx_def(m, &path)?).ok_or(Error::Type)?;
            let r = offset_face_pts(d, f, dd)?;
            let mut w = o();
            w.ch('[');
            for (i, l) in r.iter().enumerate() {
                if i > 0 {
                    w.ch(',');
                }
                w.raw(&r_pts(l));
            }
            w.ch(']');
            some(w.s)
        }
        "offsetChainPts" => {
            let (e, dd, n) = (ids(arg(a, 0))?, num(arg(a, 1))?, p3(arg(a, 2))?);
            let d = m.def(&ctx_def(m, &path)?).ok_or(Error::Type)?;
            some(r_chain(&offset_chain_pts(d, &e, dd, n)?))
        }
        "worldTriangles" => {
            let tris = world_triangles(m, "root", &IDENT);
            if regs.quiet {
                return some(r_num(tris.len() as f64));
            }
            let mut w = o();
            w.ch('[');
            for (i, t) in tris.iter().enumerate() {
                if i > 0 {
                    w.ch(',');
                }
                w.raw("{\"p\":[");
                w.p3(&t.p[0]);
                w.ch(',');
                w.p3(&t.p[1]);
                w.ch(',');
                w.p3(&t.p[2]);
                w.raw("],\"n\":");
                w.p3(&t.n);
                if let Some(c) = &t.color {
                    w.raw(",\"color\":");
                    w.str(c);
                }
                w.ch('}');
            }
            w.ch(']');
            some(w.s)
        }
        "hiddenCount" => some(r_num(hidden_count(m))),
        "captureVis" => {
            let v = m.capture_vis();
            let s = r_vis(&v);
            regs.vis.push((index, v));
            some(s)
        }
        "copyClip" => {
            let s = sel(arg(a, 0))?;
            let c = m.copy_clip(&path, &s)?;
            match c {
                Some(c) => {
                    let s = r_value(&c);
                    regs.clips.push((index, c));
                    some(s)
                }
                None => some("null".into()),
            }
        }
        "tagCounts" => {
            let mut w = o();
            w.ch('{');
            for (i, (k, v)) in m.tag_counts().iter().enumerate() {
                if i > 0 {
                    w.ch(',');
                }
                w.key(k);
                w.num(*v);
            }
            w.ch('}');
            some(w.s)
        }
        "instanceCounts" => {
            let mut w = o();
            w.ch('{');
            for (i, (k, v)) in m.instance_counts().iter().enumerate() {
                if i > 0 {
                    w.ch(',');
                }
                w.key(k);
                w.num(*v);
            }
            w.ch('}');
            some(w.s)
        }
        "activeCut" => some(match m.active_cut() {
            Some((id, p, n)) => format!("{{\"id\":{},\"p\":{},\"n\":{}}}", r_num(id), r_p3(&p), r_p3(&n)),
            None => "null".into(),
        }),
        "axesFrame" => some(r_ids(&m.axes_frame(&path))),
        "bbox" => some(r_bbox(m.bbox("root", &IDENT))),
        "anchorAt" => {
            let (p, prefer) = (p3(arg(a, 0))?, path_of(arg(a, 1)));
            some(match m.anchor_at(p, &prefer) {
                Some((pp, v)) => format!("{{\"path\":{},\"v\":{}}}", r_ids(&pp), r_num(v)),
                None => "null".into(),
            })
        }
        "dimEnds" => {
            let id = num(arg(a, 0))?;
            let Some(mut dm) = m.dims.get(id).cloned() else { return Err(Error::Type) };
            // a radial dimension has no ends: the JS reads `undefined.path`
            if matches!(dm, Dim::Rad { .. }) {
                return Err(Error::Type);
            }
            let e = m.dim_ends(&mut dm);
            m.dims.set(id, dm);
            some(r_pts(&e))
        }
        "addImage" => {
            let o_ = arg(a, 0).unwrap_or(&Value::Null);
            let r = m.add_image(
                opt_str(o_.get("name")).unwrap_or(""),
                opt_str(o_.get("mime")).unwrap_or(""),
                opt_str(o_.get("data")).unwrap_or(""),
                opt_str(o_.get("avg")),
            )?;
            some(r_str(&r))
        }
        "setImages" => {
            m.set_images(arg(a, 0).unwrap_or(&Value::Null));
            Ok(None)
        }
        "imagesJSON" => {
            let mut w = o();
            w.ch('[');
            for (i, (id, im)) in m.images_json().iter().enumerate() {
                if i > 0 {
                    w.ch(',');
                }
                w.raw("{\"id\":");
                w.str(id);
                w.raw(",\"name\":");
                w.str(&im.name);
                w.raw(",\"mime\":");
                w.str(&im.mime);
                w.raw(",\"data\":");
                w.str(&im.data);
                w.raw(",\"avg\":");
                match &im.avg {
                    Some(x) => w.str(x),
                    None => w.raw("null"),
                }
                w.ch('}');
            }
            w.ch(']');
            some(w.s)
        }
        _ => Err(Error::Msg(format!("unknown op {op}"))),
    }
}

/// One record: the step's outcome and the model after it.
pub fn record(m: &mut Model, out: &mut Out, op: &str, r: &Outcome) {
    out.raw("{\"op\":");
    out.str(op);
    match r {
        Ok(Some(s)) => {
            out.raw(",\"ret\":");
            out.raw(s);
        }
        Ok(None) => {}
        Err(e) => {
            out.raw(",\"err\":");
            out.str(e);
        }
    }
    out.raw(",\"json\":");
    m.to_json_out(out, false);
    out.raw(",\"undo\":[");
    for (i, s) in m.undo_stack.iter().enumerate() {
        if i > 0 {
            out.ch(',');
        }
        out.str(&s.label);
    }
    out.raw("],\"redo\":[");
    for (i, s) in m.redo_stack.iter().enumerate() {
        if i > 0 {
            out.ch(',');
        }
        out.str(&s.label);
    }
    out.raw("],\"v\":");
    out.num(m.version());
    out.raw(",\"dirty\":");
    out.bool(m.dirty);
    out.raw(",\"pv\":");
    out.bool(m.previewing());
    out.ch('}');
}

/// Run a script ({init?, steps}) and write one record per step (a JSON array). `records`: false runs the steps only
/// (the benchmark).
pub fn run_script(script: &Value, records: bool) -> Res<String> {
    let mut m = Model::new();
    m.uid = "u0".into();
    if let Some(init) = script.get("init").filter(|v| !v.is_null()) {
        m.load(init, false)?;
        m.uid = "u0".into();
    }
    let empty = Vec::new();
    let steps = script.get("steps").and_then(Value::as_array).unwrap_or(&empty);
    let mut regs = Regs { quiet: !records, ..Regs::default() };
    let mut out = Out::new(true);
    out.ch('[');
    for (i, st) in steps.iter().take(MAX_STEPS).enumerate() {
        let index = i as f64;
        let r: Outcome = exec(&mut m, &mut regs, st, index, true).map_err(|e| err_text(&e));
        if matches!(json::str(st.get("op")), Some("reload" | "reset")) {
            m.uid = format!("u{}", js::num_str(index));
        }
        if records {
            if i > 0 {
                out.ch(',');
            }
            record(&mut m, &mut out, json::str(st.get("op")).unwrap_or(""), &r);
        }
    }
    out.ch(']');
    Ok(out.s)
}

/// Function-level cases: [{fn, args}] → [{ret} | {err}].
pub fn run_cases(cases: &Value) -> String {
    let empty = Vec::new();
    let list = cases.as_array().unwrap_or(&empty);
    let mut out = Out::new(true);
    out.ch('[');
    for (i, c) in list.iter().enumerate() {
        if i > 0 {
            out.ch(',');
        }
        let f = json::str(c.get("fn")).unwrap_or("");
        let a = c.get("args").and_then(Value::as_array).unwrap_or(&empty);
        match case(f, a) {
            Ok(Some(s)) => {
                out.raw("{\"ret\":");
                out.raw(&s);
                out.ch('}');
            }
            Ok(None) => out.raw("{}"),
            Err(e) => {
                out.raw("{\"err\":");
                out.str(&err_text(&e));
                out.ch('}');
            }
        }
    }
    out.ch(']');
    out.s
}

/// A byte from a JSON number (0 outside 0..255).
fn byte_of(x: f64) -> u8 {
    if (0.0..256.0).contains(&x) {
        // in range by the check above
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let b = x as u8;
        b
    } else {
        0
    }
}

fn p2s(v: Option<&Value>) -> Res<Vec<P2>> {
    let a = v.and_then(Value::as_array).ok_or(Error::Type)?;
    Ok(a.iter().map(|p| [num_or_nan(p.get(0)), num_or_nan(p.get(1))]).collect())
}

fn case(f: &str, a: &[Value]) -> Res<Option<String>> {
    let some = |s: String| Ok(Some(s));
    match f {
        "offset2" => {
            let r = offset2(&p2s(arg(a, 0))?, num(arg(a, 1))?, boolean(arg(a, 2)));
            some(match r {
                Some(q) => {
                    let mut w = o();
                    w.ch('[');
                    for (i, p) in q.iter().enumerate() {
                        if i > 0 {
                            w.ch(',');
                        }
                        w.nums(p);
                    }
                    w.ch(']');
                    w.s
                }
                None => "null".into(),
            })
        }
        "ngonPts" => {
            some(r_pts(&ngon_pts(p3(arg(a, 0))?, p3(arg(a, 1))?, p3(arg(a, 2))?, num(arg(a, 3))?, num(arg(a, 4))?.min(100_000.0))))
        }
        "rectPts" => some(r_pts(&rect_pts(p3(arg(a, 0))?, p3(arg(a, 1))?, p3(arg(a, 2))?, num(arg(a, 3))?, num(arg(a, 4))?))),
        "arcPts" => some(r_pts(&arc_pts(
            p3(arg(a, 0))?,
            p3(arg(a, 1))?,
            p3(arg(a, 2))?,
            num(arg(a, 3))?,
            num(arg(a, 4))?,
            num(arg(a, 5))?.min(100_000.0),
        ))),
        "smoothPts" => {
            let segs = json::num(arg(a, 2)).unwrap_or(SPLINE_SEGS).min(1000.0);
            some(r_pts(&smooth_pts(&pts(arg(a, 0))?, boolean(arg(a, 1)), segs)))
        }
        "mirrorMatrix" => some(r_ids(&mirror_matrix(p3(arg(a, 0))?, p3(arg(a, 1))?))),
        "rotation" => some(r_ids(&rotation(p3(arg(a, 0))?, num(arg(a, 1))?, p3(arg(a, 2))?))),
        "invert" => some(r_ids(&invert(&m16(arg(a, 0))?))),
        "newell" => some(r_p3(&newell(&pts(arg(a, 0))?))),
        "earcut" => {
            let flat = ids(arg(a, 0))?;
            // in range by the check: a whole number below 1e9
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let holes: Vec<usize> = ids(arg(a, 1))?.iter().map(|&h| if (0.0..1e9).contains(&h) { h.trunc() as usize } else { 0 }).collect();
            let t: Vec<f64> = crate::earcut::earcut(&flat, &holes).iter().map(|&i| i as f64).collect();
            some(r_ids(&t))
        }
        "isPaint" => some(r_bool(opt_str(arg(a, 0)).is_some_and(is_paint))),
        "paintHex" => some(r_str(&crate::paint::paint_hex(&string(arg(a, 0))?, &|_| None, &|_| None))),
        "toBase64" => {
            let bytes: Vec<u8> = ids(arg(a, 0))?.iter().map(|&x| byte_of(x)).collect();
            some(r_str(&crate::paint::to_base64(&bytes)))
        }
        // V8's sort on keys (null = NaN) with (a, b) => a - b: the order of the indices
        "sort" => {
            let keys = ids(arg(a, 0))?;
            let mut idx: Vec<(f64, f64)> = keys.iter().enumerate().map(|(i, &k)| (k, i as f64)).collect();
            js::sort_by(&mut idx, |x, y| x.0 - y.0);
            some(r_ids(&idx.iter().map(|x| x.1).collect::<Vec<f64>>()))
        }
        "imagePaint" => {
            let (id, u, v) = (string(arg(a, 0))?, ids(arg(a, 1))?, ids(arg(a, 2))?);
            some(r_str(&image_paint(&id, &u, &v, opt_str(arg(a, 3)))))
        }
        "imagePaintFit" => {
            let id = string(arg(a, 0))?;
            let pp = pts(arg(a, 1))?;
            let uvs = p2s(arg(a, 2))?;
            some(r_opt_str(image_paint_fit(&id, &pp, &uvs, p3(arg(a, 3))?, opt_str(arg(a, 4))).as_deref()))
        }
        "imageUV" => some(r_ids(&image_uv(&string(arg(a, 0))?, p3(arg(a, 1))?)?)),
        "cleanLight" => some(match clean_light(arg(a, 0).unwrap_or(&Value::Null)) {
            Some(l) => {
                let mut w = o();
                light_json(0.0, &l, &mut w);
                // the light alone: drop the id written first
                w.s.replacen("{\"id\":0,", "{", 1)
            }
            None => "null".into(),
        }),
        "cleanScene" => some(match clean_scene(arg(a, 0).unwrap_or(&Value::Null)) {
            Some(s) => {
                let mut w = o();
                scene_json(&s, &mut w);
                w.s
            }
            None => "null".into(),
        }),
        "cleanStoreys" => {
            let st = clean_storeys(arg(a, 0));
            let mut w = o();
            w.ch('[');
            for (i, s) in st.iter().enumerate() {
                if i > 0 {
                    w.ch(',');
                }
                w.raw("{\"name\":");
                w.str(&s.name);
                w.raw(",\"elevation\":");
                w.num(s.elevation);
                w.ch('}');
            }
            w.ch(']');
            some(w.s)
        }
        "formatLength" => some(r_str(&crate::units::format_length(num_or_nan(arg(a, 0)), opt_str(arg(a, 1)).unwrap_or("m")))),
        "formatArea" => some(r_str(&crate::units::format_area(num_or_nan(arg(a, 0)), opt_str(arg(a, 1)).unwrap_or("m")))),
        "formatVolume" => some(r_str(&crate::units::format_volume(num_or_nan(arg(a, 0)), opt_str(arg(a, 1)).unwrap_or("m")))),
        // a model-level case: run a script, its last record's ret
        "script" => {
            let s = run_script(arg(a, 0).unwrap_or(&Value::Null), true)?;
            some(s)
        }
        _ => Err(Error::Msg(format!("unknown fn {f}"))),
    }
}
