//! kernel.js — rebuilds a part's solid from its feature history on the mesh kernel (cc-manifold, the Rust Manifold
//! 3.5.4 the 0.5 build runs as wasm). Every feature makes tools (a solid + add / cut / intersect) applied to the body in
//! order; patterns and mirrors copy earlier features' tools. Each tool gets its own mesh ID (`asOriginal`), so every
//! triangle of the result knows which feature made it, and the named faces of meshfaces.js follow.
//!
//! The JS calls are kept one for one and in the same order, because the solid library numbers its meshes in call order
//! and orders the result's triangles by those numbers: every solid made during a rebuild is kept alive until the end
//! (the JS arena `ctx.T`), so the lazy boolean tree collapses exactly as the wasm's does. A failing feature records its
//! message and is skipped; nothing panics out of [`regenerate`]. Exact-only features answer `kernel.feature.not_built`
//! as the JS does with the exact kernel off.
//!
//! Spec gap: JS runtime errors (a TypeError on a malformed part) become `model.kernel.build_failed` with V8's message;
//! the cases a read part can reach are reproduced, others are reported with the nearest wording.

use crate::i18n::{F, t};
use crate::js;
use crate::jv;
use crate::kernel_edge;
use crate::meshfaces::{self as mf, Surf};
use crate::omap::OMap;
use crate::part;
use crate::readers;
use crate::refgeom::{self, EdgeRes, FaceRes, PlaneRes, Ref, Resolve};
use crate::sketch::{self, Ent};
use crate::solver;
use crate::spline::P2;
use crate::threads;
use crate::vec::{self as v, Frame, M4, V3};
use cc_manifold::{CrossSection, FillRule, Manifold, Status};
use serde_json::Value;
use std::collections::HashMap;
use std::rc::Rc;

/// A JavaScript handle on a solid (`new Manifold` / a method's result): cloning the handle never makes a second
/// Manifold, as in the JS, where one handle stored in several places is still one C++ object (the boolean tree's
/// collapse rule counts those).
pub type H = Rc<Manifold>;

/// Exact-only features the mesh kernel builds anyway, approximately (0.4 K.4).
pub const MESH_BUILT: [&str; 4] = ["fillet", "chamfer", "shell", "draft"];

/// Largest number of tools one rebuild may make (patterns of patterns): past it the feature fails.
pub const MAX_TOOLS: usize = 20_000;

/// A feature's tool: a solid and what it does to the body, with its named surfaces.
#[derive(Clone)]
pub struct Tool {
    pub m: H,
    pub op: String,
    pub surfs: Option<Value>,
}

/// A face a sketch can sit on.
#[derive(Clone, Debug)]
pub struct Cand {
    pub face: String,
    pub entity: Option<String>,
    pub frame: Frame,
}
impl Cand {
    pub fn to_json(&self) -> Value {
        let mut r = jv::obj(vec![("face", self.face.clone().into())]);
        if let Some(e) = &self.entity {
            jv::set(&mut r, "entity", e.clone().into());
        }
        jv::obj(vec![("ref", r), ("frame", self.frame.to_json())])
    }
}

/// What a feature build returns (`{ tools, warning, count, solve }`).
#[derive(Default)]
pub struct Out {
    pub tools: Option<Vec<Tool>>,
    pub warning: Option<String>,
    pub count: Option<usize>,
    pub solve: Option<Value>,
}

/// The rebuild's working state (`ctx`).
pub struct Ctx<'a> {
    pub part: &'a Value,
    pub body: Option<H>,
    live: Vec<H>,
    /// The sketches as the solver left them (the JS mutates each sketch in place).
    pub solved: OMap<Value>,
    /// The last call to meshfaces.js `namedFaces` (its mesh, triangle → tool list, tool surface lists, segments):
    /// what pcdiff's stage-3 generator replays.
    pub last_named: std::cell::RefCell<Option<NamedCall>>,
    pub results: OMap<Value>,
    pub frames: OMap<Frame>,
    pub candidates: OMap<Vec<Cand>>,
    pub axes: OMap<(V3, V3)>,
    pub hole_axes: OMap<Vec<(V3, V3)>>,
    pub tools: OMap<Vec<Tool>>,
    pub ref_geom: OMap<Ref>,
    pub threads: OMap<Value>,
    pub tool_surfs: HashMap<i32, Value>,
    pub edge_surfs: Vec<Value>,
    pub solve_info: Option<Value>,
    tool_count: usize,
}

/// `fail(m)`: a feature problem with its message.
pub type FR<T> = Result<T, String>;

pub fn tk(k: &str) -> String {
    t(k, &[])
}

impl<'a> Ctx<'a> {
    pub fn new(part: &'a Value) -> Ctx<'a> {
        Ctx {
            part,
            body: None,
            live: Vec::new(),
            results: OMap::new(),
            frames: OMap::new(),
            candidates: OMap::new(),
            axes: OMap::new(),
            hole_axes: OMap::new(),
            tools: OMap::new(),
            ref_geom: OMap::new(),
            threads: OMap::new(),
            tool_surfs: HashMap::new(),
            edge_surfs: Vec::new(),
            solve_info: None,
            tool_count: 0,
            solved: OMap::new(),
            last_named: std::cell::RefCell::new(None),
        }
    }

    /// `ctx.T(m)`: kept alive until the rebuild ends.
    pub fn t(&mut self, m: Manifold) -> H {
        let h = Rc::new(m);
        self.live.push(Rc::clone(&h));
        h
    }

    /// A sketch feature's sketch, solved when the rebuild has solved it.
    pub fn sketch_value(&self, f: &Value) -> Value {
        let id = js::str_of(f.get("id"));
        self.solved.get(&id).cloned().unwrap_or_else(|| f.get("sketch").cloned().unwrap_or(Value::Null))
    }

    /// `ctx.part.resolution` (undefined → NaN).
    pub fn resolution(&self) -> f64 {
        jv::as_num(self.part.get("resolution")).unwrap_or(f64::NAN)
    }

    /// `ctx.part.resolution || 64`.
    pub fn resolution_or64(&self) -> f64 {
        let r = self.resolution();
        if r == 0.0 || r.is_nan() { 64.0 } else { r }
    }

    pub fn features(&self) -> &'a [Value] {
        part::features(self.part)
    }

    pub fn feature(&self, id: &str) -> Option<&'a Value> {
        self.features().iter().find(|g| jv::is_str(g.get("id"), id))
    }

    fn count_tools(&mut self, n: usize) -> FR<()> {
        self.tool_count = self.tool_count.saturating_add(n);
        if self.tool_count > MAX_TOOLS {
            return Err(t("model.kernel.build_failed", &[("error", F::S("too many tools".into()))]));
        }
        Ok(())
    }
}

/// Point `vi` of a flat xyz array (NaN past the end).
pub fn pt_at(p: &[f64], vi: usize) -> V3 {
    let b = vi.saturating_mul(3);
    let g = |k: usize| p.get(b.saturating_add(k)).copied().unwrap_or(f64::NAN);
    [g(0), g(1), g(2)]
}

/// Vertex `k` of triangle `ti` (0 past the end).
pub fn tri_v(ii: &[u32], ti: usize, k: usize) -> u32 {
    ii.get(ti.saturating_mul(3).saturating_add(k)).copied().unwrap_or(0)
}

/// manifold-3d's `transform(mat4)` (JS column-major 4×4).
pub fn xf(m: &Manifold, mat: &M4) -> Manifold {
    m.transform_mat4(mat)
}

fn name_of(f: &Value) -> String {
    js::str_of(f.get("name"))
}

fn p3(x: Option<&Value>) -> V3 {
    let a = jv::arr(x);
    let g = |i: usize| jv::as_num(a.get(i)).unwrap_or(f64::NAN);
    [g(0), g(1), g(2)]
}

/// World frame of a plane reference, or a feature problem.
pub fn resolve_plane(r: &Value, ctx: &Ctx) -> FR<Frame> {
    let kind = jv::s(r, "kind");
    if kind == "base" {
        return v::base_plane(jv::s(r, "name"))
            .ok_or_else(|| t("model.kernel.build_failed", &[("error", F::S("Cannot read properties of undefined (reading 'o')".into()))]));
    }
    if kind == "fixed" {
        return Ok(v::frame_for(p3(r.get("n")), p3(r.get("o"))));
    }
    if kind == "ref" {
        return match ctx.ref_geom.get(jv::s(r, "feature")) {
            Some(Ref::Plane { frame, .. }) => Ok(*frame),
            _ => Err(tk("model.kernel.ref_plane_not_built")),
        };
    }
    let fid = js::str_of(r.get("feature"));
    let res = ctx.results.get(&fid);
    let owner = ctx.feature(&fid);
    if !res.is_some_and(|x| jv::is_str(x.get("status"), "ok")) {
        return Err(match owner {
            Some(o) => t("model.kernel.face_gone_not_built", &[("name", F::S(name_of(o)))]),
            None => tk("model.kernel.face_gone_deleted"),
        });
    }
    let face = js::str_of(r.get("face"));
    if face == "named" {
        return Err(tk("kernel.mesh.named_face"));
    }
    let entity = r.get("entity");
    let c = ctx.candidates.get(&fid).and_then(|l| {
        l.iter().find(|k| {
            k.face == face && (face != "side" || jv::strict_eq(k.entity.as_ref().map(|e| Value::String(e.clone())).as_ref(), entity))
        })
    });
    match c {
        Some(c) => Ok(c.frame),
        None => Err(t("model.kernel.face_gone_from", &[("name", F::S(owner.map(name_of).unwrap_or_default()))])),
    }
}

/// A direction reference → `{ o, d }`.
pub fn resolve_dir(r: &Value, ctx: &Ctx) -> FR<(V3, V3)> {
    let kind = jv::s(r, "kind");
    if kind == "world" {
        let d = match jv::s(r, "axis") {
            "x" => [1.0, 0.0, 0.0],
            "y" => [0.0, 1.0, 0.0],
            "z" => [0.0, 0.0, 1.0],
            _ => [f64::NAN; 3],
        };
        return Ok(([0.0; 3], d));
    }
    if kind == "fixed" {
        return Ok((p3(r.get("o")), v::norm(p3(r.get("d")))));
    }
    if kind == "axis" {
        return ctx.axes.get(jv::s(r, "feature")).copied().ok_or_else(|| tk("model.kernel.axis_revolve_not_built"));
    }
    if kind == "ref" {
        return match ctx.ref_geom.get(jv::s(r, "feature")) {
            Some(Ref::Axis { o, d }) => Ok((*o, *d)),
            Some(Ref::Plane { frame, .. }) => Ok((frame.o, frame.n)),
            _ => Err(tk("model.kernel.ref_not_built")),
        };
    }
    let sid = js::str_of(r.get("sketch"));
    let frame = ctx.frames.get(&sid).copied();
    let sf = ctx.feature(&sid);
    let (Some(frame), Some(sf)) = (frame, sf) else { return Err(tk("model.kernel.sketch_line_not_built")) };
    let skv = ctx.sketch_value(sf);
    let sk = &skv;
    let l = sketch::by_id(sk, jv::s(r, "entity"));
    let Some(l) = l.filter(|l| l.ty() == "line") else {
        return Err(t("model.kernel.line_gone_from", &[("name", F::S(name_of(sf)))]));
    };
    let pa = sketch::pxy(sk, l.str("p1"));
    let pb = sketch::pxy(sk, l.str("p2"));
    let a = v::to_world(&frame, pa[0], pa[1]);
    let b = v::to_world(&frame, pb[0], pb[1]);
    if v::len(v::sub(b, a)) < 1e-12 {
        return Err(tk("model.kernel.line_no_length"));
    }
    Ok((a, v::norm(v::sub(b, a))))
}

/// Extent of the body along a plane normal, relative to the plane: `[min, max]` or None.
pub fn body_range(ctx: &Ctx, frame: &Frame) -> Option<(f64, f64)> {
    let body = ctx.body.as_ref()?;
    let (mn, mx) = body.bounding_box();
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for x in [mn[0], mx[0]] {
        for y in [mn[1], mx[1]] {
            for z in [mn[2], mx[2]] {
                let d = v::dot(v::sub([x, y, z], frame.o), frame.n);
                lo = js::min(lo, d);
                hi = js::max(hi, d);
            }
        }
    }
    Some((lo, hi))
}

/// `sketchOf(ctx, id)`.
pub fn sketch_of<'b>(ctx: &Ctx<'b>, id: &str) -> FR<(&'b Value, Value, Frame)> {
    let f = ctx.feature(id);
    let frame = ctx.frames.get(id).copied();
    match (f, frame) {
        (Some(f), Some(frame)) if jv::is_str(f.get("type"), "sketch") => Ok((f, ctx.sketch_value(f), frame)),
        _ => Err(tk("model.kernel.sketch_not_built")),
    }
}

fn polys(loops: &[Vec<P2>]) -> Vec<Vec<[f64; 2]>> {
    loops.to_vec()
}

/// `crossSection(ctx, loops)`: even-odd, refused when it has no area.
pub fn cross_section(loops: &[Vec<P2>]) -> FR<CrossSection> {
    let cs = CrossSection::new(&polys(loops), FillRule::EvenOdd);
    if cs.is_empty() || cs.area() < 1e-12 {
        return Err(tk("model.kernel.profile_no_area"));
    }
    Ok(cs)
}

/// `cs.extrude(h, nDiv, twist, scaleTop)` as the binding runs it (`Module._Extrude(cs._ToPolygons(), …)`).
pub fn cs_extrude(cs: &CrossSection, h: f64, n_div: i32, twist: f64, scale_top: [f64; 2]) -> Manifold {
    Manifold::extrude(&cs.to_polygons(), h, n_div, twist, scale_top)
}

/// `cs.revolve(circularSegments, degrees)`; `undefined` segments → 0 (the default).
pub fn cs_revolve(cs: &CrossSection, segments: f64, degrees: f64) -> Manifold {
    Manifold::revolve(&cs.to_polygons(), js_int(segments), degrees)
}

/// A JS number passed to an embind `int` (truncated toward zero, NaN → 0).
pub fn js_int(x: f64) -> i32 {
    if x.is_nan() {
        return 0;
    }
    #[allow(clippy::cast_possible_truncation)]
    let i = x.trunc().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32;
    i
}

fn max_abs_coord(loops: &[Vec<P2>]) -> f64 {
    js::max_of(std::iter::once(1.0).chain(loops.iter().flatten().map(|p| js::max(p[0].abs(), p[1].abs()))))
}

/// `sideCandidates(sk, loops, frame, curves, op, extra)`.
fn side_candidates(
    sk: &Value,
    loops: &[Vec<P2>],
    curves: &[(String, usize)],
    op: &str,
    extra: &dyn Fn(P2, P2, f64) -> Option<(V3, V3)>,
) -> Vec<Cand> {
    let mut out = Vec::new();
    let eps = 1e-4 * max_abs_coord(loops);
    for (id, _) in curves {
        let Some(l) = sketch::by_id(sk, id) else { continue };
        if l.ty() != "line" {
            continue;
        }
        let a = sketch::pxy(sk, l.str("p1"));
        let b = sketch::pxy(sk, l.str("p2"));
        let Some((n, p)) = extra(a, b, eps) else { continue };
        let n = if op == "cut" { v::mul(n, -1.0) } else { n };
        out.push(Cand { face: "side".into(), entity: Some(id.clone()), frame: v::frame_for(n, p) });
    }
    out
}

/// brep-design.js `regionsAt(loops, pts, fail)` → `[{ outer, holes }]`.
fn regions_at(loops: &[Vec<P2>], pts: &[Value]) -> FR<Vec<(usize, Vec<usize>)>> {
    let contains = |i: usize, p: P2| loops.get(i).is_some_and(|l| sketch::inside_loops(std::slice::from_ref(l), p));
    let first = |i: usize| loops.get(i).and_then(|l| l.first()).copied().unwrap_or([f64::NAN, f64::NAN]);
    let depth: Vec<usize> = (0..loops.len())
        .map(|i| {
            (0..loops.len())
                .filter(|&j| {
                    j != i
                        && loops.get(j).is_some_and(|l| sketch::inside_loops(std::slice::from_ref(l), first(i)))
                        && !contains(i, first(j))
                })
                .count()
        })
        .collect();
    let mut out: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for pv in pts {
        let p = [jv::as_num(pv.get(0)).unwrap_or(f64::NAN), jv::as_num(pv.get(1)).unwrap_or(f64::NAN)];
        let inside: Vec<usize> = (0..loops.len()).filter(|&i| contains(i, p)).collect();
        let Some(&first_in) = inside.first() else {
            return Err(t("model.brep_design.region_gone", &[("x", F::N(js::fixed_num(p[0], 3))), ("y", F::N(js::fixed_num(p[1], 3)))]));
        };
        let dep = |i: usize| depth.get(i).copied().unwrap_or(0);
        let outer = inside.iter().skip(1).fold(first_in, |a, &b| if dep(b) > dep(a) { b } else { a });
        if !seen.insert(outer) {
            continue;
        }
        let holes: Vec<usize> = (0..loops.len())
            .filter(|&j| {
                j != outer
                    && dep(j) == dep(outer).saturating_add(1)
                    && loops.get(outer).is_some_and(|l| sketch::inside_loops(std::slice::from_ref(l), first(j)))
            })
            .collect();
        out.push((outer, holes));
    }
    Ok(out)
}

/// The JS `Map` of a feature's tool surfaces, as JSON values.
pub fn surf_plane(name: &str, n: V3, o: V3) -> Value {
    jv::obj(vec![("name", name.into()), ("type", "plane".into()), ("n", jv::pt3(v::norm(n))), ("o", jv::pt3(o))])
}
pub fn surf_cylinder(name: &str, o: V3, d: V3, r: f64) -> Value {
    jv::obj(vec![
        ("name", name.into()),
        ("type", "cylinder".into()),
        ("axis", jv::obj(vec![("o", jv::pt3(o)), ("d", jv::pt3(v::norm(d)))])),
        ("r", jv::num(r)),
    ])
}
pub fn surf_cone(name: &str, apex: V3, d: V3, k: f64) -> Value {
    jv::obj(vec![
        ("name", name.into()),
        ("type", "cone".into()),
        ("axis", jv::obj(vec![("o", jv::pt3(apex)), ("d", jv::pt3(v::norm(d)))])),
        ("k", jv::num(k)),
    ])
}
pub fn surf_sphere(name: &str, c: V3, r: f64) -> Value {
    jv::obj(vec![("name", name.into()), ("type", "sphere".into()), ("c", jv::pt3(c)), ("r", jv::num(r))])
}
pub fn surf_torus(name: &str, c: V3, d: V3, big_r: f64, r: f64) -> Value {
    jv::obj(vec![
        ("name", name.into()),
        ("type", "torus".into()),
        ("axis", jv::obj(vec![("o", jv::pt3(c)), ("d", jv::pt3(v::norm(d)))])),
        ("R", jv::num(big_r)),
        ("r", jv::num(r)),
    ])
}
pub fn surf_other(name: &str) -> Value {
    jv::obj(vec![("name", name.into()), ("type", "other".into())])
}
pub fn surf_thread(name: &str, o: V3, d: V3, r0: f64, r1: f64, t0: f64, t1: f64) -> Value {
    jv::obj(vec![
        ("name", name.into()),
        ("type", "thread".into()),
        ("axis", jv::obj(vec![("o", jv::pt3(o)), ("d", jv::pt3(v::norm(d)))])),
        ("r0", jv::num(r0)),
        ("r1", jv::num(r1)),
        ("t0", jv::num(t0)),
        ("t1", jv::num(t1)),
    ])
}
pub fn surf_within(s: Value, o: V3, d: V3, r: f64) -> Value {
    let mut s = s;
    jv::set(&mut s, "within", jv::obj(vec![("o", jv::pt3(o)), ("d", jv::pt3(v::norm(d))), ("r", jv::num(r))]));
    s
}
pub fn surf_span(s: Value, a: V3, b: V3) -> Value {
    let d = v::sub(b, a);
    let len = v::len(d);
    let mut s = s;
    jv::set(&mut s, "span", jv::obj(vec![("a", jv::pt3(a)), ("d", jv::pt3(v::mul(d, 1.0 / len))), ("len", jv::num(len))]));
    s
}

/// One `namedFaces(mesh, triTool, toolSurfs, { segments })` call's arguments.
#[derive(Clone, Debug)]
pub struct NamedCall {
    pub positions: Vec<f64>,
    pub indices: Vec<u32>,
    pub tri_tool: Vec<i64>,
    pub lists: Vec<Value>,
    pub segments: f64,
}

/// A body's mesh as meshfaces.js and the edge tools read it, with the named faces and edges (K.2b).
pub struct BodyFaces {
    pub mesh: crate::faces::Mesh,
    pub named: mf::Named,
}

/// `bodyFaces(ctx)`.
pub fn body_faces(ctx: &Ctx) -> Option<BodyFaces> {
    let body = ctx.body.as_ref()?;
    let gm = body.get_mesh();
    let (mesh, tri_tool, lists, lists_json) = mesh_and_tools(&gm, &ctx.tool_surfs);
    let named = mf::named_faces(&mesh, &tri_tool, &lists, ctx.resolution_or64());
    *ctx.last_named.borrow_mut() = Some(NamedCall {
        positions: mesh.positions.clone(),
        indices: mesh.indices.clone(),
        tri_tool,
        lists: lists_json,
        segments: ctx.resolution_or64(),
    });
    Some(BodyFaces { mesh, named })
}

/// The mesh (positions as f32 values), each triangle's tool list index and the lists, from a result mesh.
fn mesh_and_tools(gm: &cc_manifold::Mesh, tool_surfs: &HashMap<i32, Value>) -> (crate::faces::Mesh, Vec<i64>, Vec<Vec<Surf>>, Vec<Value>) {
    let np = usize::try_from(gm.num_prop).unwrap_or(3).max(1);
    let nv = gm.vert_properties.len().checked_div(np).unwrap_or(0);
    let mut positions = Vec::with_capacity(nv.saturating_mul(3));
    for i in 0..nv {
        for k in 0..3 {
            positions.push(f64::from(gm.vert_properties.get(i.saturating_mul(np).saturating_add(k)).copied().unwrap_or(f32::NAN)));
        }
    }
    let indices = gm.tri_verts.clone();
    let ntri = indices.len() / 3;
    let mut lists: Vec<Vec<Surf>> = Vec::new();
    let mut lists_json: Vec<Value> = Vec::new();
    let mut list_of: HashMap<i32, i64> = HashMap::new();
    let mut tri_tool = vec![-1i64; ntri];
    let runs = &gm.run_index;
    for r in 0..runs.len().saturating_sub(1) {
        let id = gm.run_original_id.get(r).map_or(-1, |&x| x.cast_signed());
        let Some(sf) = tool_surfs.get(&id) else { continue };
        let k = *list_of.entry(id).or_insert_with(|| {
            lists.push(jv::arr(Some(sf)).iter().map(Surf::from_json).collect());
            lists_json.push(sf.clone());
            i64::try_from(lists.len().saturating_sub(1)).unwrap_or(-1)
        });
        let (a, b) = (runs.get(r).copied().unwrap_or(0) / 3, runs.get(r.saturating_add(1)).copied().unwrap_or(0) / 3);
        for ti in a..b {
            if let Some(x) = tri_tool.get_mut(ti as usize) {
                *x = k;
            }
        }
    }
    (crate::faces::Mesh { positions, indices, tri_face: None }, tri_tool, lists, lists_json)
}

// ---------------------------------------------------------------- features

fn n(f: &Value, k: &str) -> f64 {
    jv::n(f, k)
}

/// `FEATURES.sketch`.
fn feat_sketch(f: &Value, ctx: &mut Ctx) -> FR<Out> {
    let frame = resolve_plane(f.get("plane").unwrap_or(&Value::Null), ctx)?;
    let id = js::str_of(f.get("id"));
    ctx.frames.set(&id, frame);
    let mut sk = f.get("sketch").cloned().unwrap_or(Value::Null);
    let s = solver::solve_sketch(&mut sk, None, None);
    ctx.solved.set(&id, sk);
    let solve = jv::obj(vec![
        ("status", s.status.into()),
        ("dof", jv::unum(s.dof)),
        ("conflicts", jv::strs(&s.conflicts)),
        ("redundant", jv::strs(&s.redundant)),
    ]);
    ctx.solve_info = Some(solve.clone());
    if !s.conflicts.is_empty() {
        return Err(t("model.kernel.over_defined.conflicts", &[("n", F::from(s.conflicts.len()))]));
    }
    let warning = (!s.redundant.is_empty()).then(|| t("model.kernel.over_defined.redundant", &[("n", F::from(s.redundant.len()))]));
    Ok(Out { solve: Some(solve), warning, ..Out::default() })
}

/// `FEATURES.extrude`.
fn feat_extrude(f: &Value, ctx: &mut Ctx) -> FR<Out> {
    let (_, skv, frame) = sketch_of(ctx, jv::s(f, "sketch"))?;
    let sk = &skv;
    let pr = sketch::profile(sk, ctx.resolution()).map_err(|e| e.error)?;
    if pr.loops.is_empty() {
        return Err(tk("model.kernel.no_profile_extrude"));
    }
    let end = js::str_of(f.get("end"));
    let taper = jv::truthy(f.get("taper"));
    if ["upto", "next", "vertex", "offset"].contains(&end.as_str()) || taper {
        return Err(tk(if taper { "kernel.mesh.taper" } else { "kernel.mesh.extrude_to" }));
    }
    let loops: Vec<Vec<P2>> = if jv::arr(f.get("regions")).is_empty() {
        pr.loops.clone()
    } else {
        let regs = regions_at(&pr.loops, jv::arr(f.get("regions")))?;
        regs.iter().flat_map(|(o, h)| std::iter::once(*o).chain(h.iter().copied())).filter_map(|i| pr.loops.get(i).cloned()).collect()
    };
    let cs = cross_section(&loops)?;
    let op = js::str_of(f.get("op"));
    let sign = (if op == "cut" { -1.0 } else { 1.0 }) * (if jv::truthy(f.get("reverse")) { -1.0 } else { 1.0 });
    let mut h = n(f, "depth");
    let z0;
    if jv::truthy(f.get("two")) && end == "blind" {
        h = n(f, "depth") + n(f, "depth2");
        z0 = if sign > 0.0 { -n(f, "depth2") } else { -n(f, "depth") };
    } else if end == "blind" {
        z0 = if sign > 0.0 { 0.0 } else { -h };
    } else if end == "mid" {
        z0 = -h / 2.0;
    } else {
        if op != "cut" {
            return Err(tk("model.kernel.through_all_cuts"));
        }
        let Some(rg) = body_range(ctx, &frame) else { return Err(tk("model.kernel.nothing_to_cut")) };
        let margin = 1.0 + 0.01 * (rg.1 - rg.0);
        if sign > 0.0 {
            if rg.1 <= 1e-9 {
                return Err(tk("model.kernel.nothing_to_cut_side"));
            }
            z0 = 0.0;
            h = rg.1 + margin;
        } else {
            if rg.0 >= -1e-9 {
                return Err(tk("model.kernel.nothing_to_cut_side"));
            }
            h = -rg.0 + margin;
            z0 = -h;
        }
    }
    if !(h > 0.0) {
        return Err(tk("model.kernel.depth_zero"));
    }
    let e = cs_extrude(&cs, h, 0, 0.0, [1.0, 1.0]);
    let e = ctx.t(e);
    let tr = ctx.t(e.translate([0.0, 0.0, z0]));
    let tool = ctx.t(xf(&tr, &v::frame_matrix(&frame)));
    let at = |z: f64| v::add(frame.o, v::mul(frame.n, z));
    let flip = if op == "cut" { -1.0 } else { 1.0 };
    let mut cand = vec![
        Cand { face: "start".into(), entity: None, frame: v::frame_for(v::mul(frame.n, -flip), at(z0)) },
        Cand { face: "end".into(), entity: None, frame: v::frame_for(v::mul(frame.n, flip), at(z0 + h)) },
    ];
    let loops0 = pr.loops.clone();
    cand.extend(side_candidates(sk, &pr.loops, &pr.curves, &op, &|a, b, eps| {
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let l = js::hypot2(dx, dy);
        let mut nn = [dy / l, -dx / l];
        let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        if sketch::inside_loops(&loops0, [m[0] + nn[0] * eps, m[1] + nn[1] * eps]) {
            nn = [-nn[0], -nn[1]];
        }
        Some((v::add(v::mul(frame.x, nn[0]), v::mul(frame.y, nn[1])), v::to_world(&frame, a[0], a[1])))
    }));
    let id = js::str_of(f.get("id"));
    ctx.candidates.set(&id, cand);
    let mut surfs = vec![surf_plane(&format!("{id}.start"), frame.n, at(z0)), surf_plane(&format!("{id}.end"), frame.n, at(z0 + h))];
    for (cid, _) in &pr.curves {
        let Some(e) = sketch::by_id(sk, cid) else { continue };
        let nm = format!("{id}.side:{cid}");
        match e.ty() {
            "line" => {
                let pa = sketch::pxy(sk, e.str("p1"));
                let pb = sketch::pxy(sk, e.str("p2"));
                let a = v::to_world(&frame, pa[0], pa[1]);
                let b = v::to_world(&frame, pb[0], pb[1]);
                surfs.push(surf_span(surf_plane(&nm, v::cross(v::sub(b, a), frame.n), a), a, b));
            }
            "circle" | "arc" => {
                let pc = sketch::pxy(sk, e.str("c"));
                let c = v::to_world(&frame, pc[0], pc[1]);
                let r = if e.ty() == "circle" {
                    e.num("r")
                } else {
                    let ps = sketch::pxy(sk, e.str("s"));
                    v::len(v::sub(v::to_world(&frame, ps[0], ps[1]), c))
                };
                surfs.push(surf_cylinder(&nm, c, frame.n, r));
            }
            _ => surfs.push(surf_other(&nm)),
        }
    }
    Ok(Out { tools: Some(vec![Tool { m: tool, op, surfs: Some(Value::Array(surfs)) }]), ..Out::default() })
}

/// `FEATURES.revolve`.
fn feat_revolve(f: &Value, ctx: &mut Ctx) -> FR<Out> {
    let (_, skv, frame) = sketch_of(ctx, jv::s(f, "sketch"))?;
    let sk = &skv;
    let axis_v = f.get("axis");
    let (a2, b2, axis_id, axis_construction): (P2, P2, Option<String>, bool) = if axis_v.is_some_and(|a| a.is_object()) {
        let ax = resolve_dir(axis_v.unwrap_or(&Value::Null), ctx)?;
        if v::dot(ax.1, frame.n).abs() > 1e-9
            || v::dot(v::sub(ax.0, frame.o), frame.n).abs() > 1e-7 * js::max(1.0, v::len(v::sub(ax.0, frame.o)))
        {
            return Err(tk("model.kernel.axis_in_plane"));
        }
        (v::to_plane(&frame, ax.0), v::to_plane(&frame, v::add(ax.0, ax.1)), None, true)
    } else {
        let axis = if jv::truthy(axis_v) {
            sketch::by_id(sk, &js::str_of(axis_v))
        } else {
            sketch::entities(sk).iter().find(|e| jv::is_str(e.get("type"), "line") && jv::truthy(e.get("construction"))).map(Ent::E)
        };
        let Some(axis) = axis.filter(|a| a.ty() == "line") else {
            return Err(tk(if jv::truthy(axis_v) { "model.kernel.axis_gone" } else { "model.kernel.draw_centerline" }));
        };
        (sketch::pxy(sk, axis.str("p1")), sketch::pxy(sk, axis.str("p2")), Some(axis.id().to_string()), axis.construction())
    };
    let ul = js::hypot2(b2[0] - a2[0], b2[1] - a2[1]);
    if ul < 1e-12 {
        return Err(tk("model.kernel.axis_no_length"));
    }
    let u = [(b2[0] - a2[0]) / ul, (b2[1] - a2[1]) / ul];
    let sk_no_axis: Value = if axis_construction {
        sk.clone()
    } else {
        let mut c = sk.clone();
        if let Some(ents) = c.get_mut("entities").and_then(Value::as_array_mut) {
            for e in ents.iter_mut() {
                if axis_id.as_deref().is_some_and(|id| jv::is_str(e.get("id"), id)) {
                    jv::set(e, "construction", true.into());
                }
            }
        }
        c
    };
    let res = ctx.resolution();
    let mut pr = sketch::profile(&sk_no_axis, res);
    if pr.is_err() && !axis_construction {
        pr = sketch::profile(sk, res);
    }
    let pr = pr.map_err(|e| e.error)?;
    if pr.loops.is_empty() {
        return Err(tk("model.kernel.no_profile_revolve"));
    }
    let scale = js::max_of(std::iter::once(1.0).chain(pr.loops.iter().flatten().map(|p| js::hypot2(p[0] - a2[0], p[1] - a2[1]))));
    let (mut pos, mut neg) = (0usize, 0usize);
    for p in pr.loops.iter().flatten() {
        let s = -u[1] * (p[0] - a2[0]) + u[0] * (p[1] - a2[1]);
        if s > 1e-7 * scale {
            pos = pos.saturating_add(1);
        } else if s < -1e-7 * scale {
            neg = neg.saturating_add(1);
        }
    }
    if pos > 0 && neg > 0 {
        return Err(tk("model.kernel.profile_crosses_axis"));
    }
    let side = if neg > 0 { -1.0 } else { 1.0 };
    let n2 = [-u[1] * side, u[0] * side];
    let loops: Vec<Vec<P2>> = pr
        .loops
        .iter()
        .map(|lp| {
            lp.iter()
                .map(|p| {
                    let d = [p[0] - a2[0], p[1] - a2[1]];
                    [js::max(0.0, d[0] * n2[0] + d[1] * n2[1]), d[0] * u[0] + d[1] * u[1]]
                })
                .collect()
        })
        .collect();
    let cs = cross_section(&loops)?;
    let u3 = v::add(v::mul(frame.x, u[0]), v::mul(frame.y, u[1]));
    let n3 = v::add(v::mul(frame.x, n2[0]), v::mul(frame.y, n2[1]));
    let w3 = v::cross(u3, n3);
    let a = v::to_world(&frame, a2[0], a2[1]);
    let angle = n(f, "angle");
    let r0 = cs_revolve(&cs, res, angle);
    let r0 = ctx.t(r0);
    let tool = ctx.t(xf(&r0, &v::basis_matrix(n3, w3, u3, a)));
    let id = js::str_of(f.get("id"));
    ctx.axes.set(&id, (a, u3));
    let op = js::str_of(f.get("op"));
    let loops0 = pr.loops.clone();
    let cand = side_candidates(sk, &pr.loops, &pr.curves, &op, &|pa, pb, eps| {
        let d = [pb[0] - pa[0], pb[1] - pa[1]];
        let l = js::hypot2(d[0], d[1]);
        if (d[0] * u[0] + d[1] * u[1]).abs() > 1e-9 * l {
            return None;
        }
        let m = [(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0];
        let up = sketch::inside_loops(&loops0, [m[0] + u[0] * eps, m[1] + u[1] * eps]);
        let tt = (pa[0] - a2[0]) * u[0] + (pa[1] - a2[1]) * u[1];
        Some((v::mul(u3, if up { -1.0 } else { 1.0 }), v::add(a, v::mul(u3, tt))))
    });
    ctx.candidates.set(&id, cand);
    let mut surfs = Vec::new();
    let rz = |q: P2| -> [f64; 2] {
        let d = [q[0] - a2[0], q[1] - a2[1]];
        [d[0] * n2[0] + d[1] * n2[1], d[0] * u[0] + d[1] * u[1]]
    };
    for (cid, _) in &pr.curves {
        let Some(e) = sketch::by_id(sk, cid) else { continue };
        if axis_id.as_deref().is_some_and(|x| !x.is_empty() && e.id() == x) {
            continue;
        }
        let nm = format!("{id}.side:{cid}");
        if e.ty() == "arc" || e.ty() == "circle" {
            let [rc, zc] = rz(sketch::pxy(sk, e.str("c")));
            let ra = if e.ty() == "circle" {
                e.num("r")
            } else {
                let ps = sketch::pxy(sk, e.str("s"));
                let pc = sketch::pxy(sk, e.str("c"));
                js::hypot2(ps[0] - pc[0], ps[1] - pc[1])
            };
            let c3 = v::add(a, v::mul(u3, zc));
            surfs.push(if rc.abs() <= 1e-9 * scale { surf_sphere(&nm, c3, ra) } else { surf_torus(&nm, c3, u3, rc, ra) });
            continue;
        }
        if e.ty() != "line" {
            surfs.push(surf_other(&nm));
            continue;
        }
        let [r1, z1] = rz(sketch::pxy(sk, e.str("p1")));
        let [r2, z2] = rz(sketch::pxy(sk, e.str("p2")));
        let eps = 1e-9 * scale;
        if (z2 - z1).abs() <= eps {
            surfs.push(surf_plane(&nm, u3, v::add(a, v::mul(u3, z1))));
        } else if (r2 - r1).abs() <= eps {
            if r1 > eps {
                surfs.push(surf_cylinder(&nm, a, u3, r1));
            }
        } else {
            let slope = (r2 - r1) / (z2 - z1);
            let za = z1 - r1 / slope;
            surfs.push(surf_cone(&nm, v::add(a, v::mul(u3, za)), if slope > 0.0 { u3 } else { v::mul(u3, -1.0) }, slope.abs()));
        }
    }
    if (angle - 360.0).abs() > 1e-9 {
        surfs.push(surf_plane(&format!("{id}.start"), w3, a));
        surfs.push(surf_plane(&format!("{id}.end"), v::apply_mat(&v::rotation_about([0.0; 3], u3, angle), w3), a));
    }
    Ok(Out { tools: Some(vec![Tool { m: tool, op, surfs: Some(Value::Array(surfs)) }]), ..Out::default() })
}

/// `FEATURES.hole`.
fn feat_hole(f: &Value, ctx: &mut Ctx) -> FR<Out> {
    let (_, skv, frame) = sketch_of(ctx, jv::s(f, "sketch"))?;
    let sk = &skv;
    let pts: Vec<&Value> = sketch::loose_points(sk).into_iter().filter(|p| !jv::truthy(p.get("construction"))).collect();
    if pts.is_empty() {
        return Err(tk("model.kernel.hole_points"));
    }
    if !(n(f, "d") > 0.0) {
        return Err(tk("model.kernel.hole_diameter_zero"));
    }
    let kind = js::str_of(f.get("kind"));
    let size = js::str_of(f.get("size"));
    let thread = if kind == "tapped" { readers::iso_coarse(&size) } else { None };
    if kind == "tapped" && thread.is_none() {
        return Err(tk("model.kernel.thread_size"));
    }
    let sign = if jv::truthy(f.get("reverse")) { 1.0 } else { -1.0 };
    let mut depth = n(f, "depth");
    let through = jv::truthy(f.get("through"));
    if through {
        let Some(rg) = body_range(ctx, &frame) else { return Err(tk("model.kernel.nothing_to_drill")) };
        let margin = 1.0 + 0.01 * (rg.1 - rg.0);
        depth = if sign < 0.0 { -rg.0 + margin } else { rg.1 + margin };
        if depth <= margin {
            return Err(tk("model.kernel.nothing_to_drill_side"));
        }
    }
    if !(depth > 0.0) {
        return Err(tk("model.kernel.hole_depth_zero"));
    }
    if kind == "cbore" {
        if !(n(f, "cboreD") > n(f, "d")) {
            return Err(tk("model.kernel.cbore_wider"));
        }
        if !(n(f, "cboreDepth") > 0.0) || (!through && n(f, "cboreDepth") >= depth) {
            return Err(tk("model.kernel.cbore_shallower"));
        }
    }
    let res = ctx.resolution();
    let disk = |r: f64, x: f64, y: f64| CrossSection::new(&[sketch::tess_circle([x, y], r, res)], FillRule::Positive);
    let mut parts: Vec<H> = Vec::new();
    let hd = thread.map_or(n(f, "d"), |tt| tt[2]);
    if kind == "csink" && !(n(f, "csinkD") > n(f, "d")) {
        return Err(tk("model.kernel.csink_wider"));
    }
    for p in &pts {
        let (px, py) = (jv::n(p, "x"), jv::n(p, "y"));
        let z0 = if sign < 0.0 { -depth } else { 0.0 };
        let e = cs_extrude(&disk(hd / 2.0, px, py), depth, 0, 0.0, [1.0, 1.0]);
        let e = ctx.t(e);
        let m = ctx.t(e.translate([0.0, 0.0, z0]));
        parts.push(m);
        if kind == "csink" {
            let big_r = n(f, "csinkD") / 2.0;
            let r = n(f, "d") / 2.0;
            let hc = (big_r - r) / js::tan(n(f, "csinkAngle") * std::f64::consts::PI / 360.0);
            let cone0 = cs_extrude(&disk(big_r, 0.0, 0.0), hc, 0, 0.0, [r / big_r, r / big_r]);
            let cone = ctx.t(cone0);
            let c2 = if sign < 0.0 { ctx.t(cone.scale([1.0, 1.0, -1.0])) } else { cone };
            let m = ctx.t(c2.translate([px, py, 0.0]));
            parts.push(m);
        }
        if kind == "cbore" {
            let cz = if sign < 0.0 { -n(f, "cboreDepth") } else { 0.0 };
            let e = cs_extrude(&disk(n(f, "cboreD") / 2.0, px, py), n(f, "cboreDepth"), 0, 0.0, [1.0, 1.0]);
            let e = ctx.t(e);
            let m = ctx.t(e.translate([0.0, 0.0, cz]));
            parts.push(m);
        }
    }
    let thread_depth = n(f, "threadDepth");
    let td_or = |d: f64| if thread_depth == 0.0 || thread_depth.is_nan() { d } else { thread_depth };
    let th_len = if thread.is_some() { js::min(depth, td_or(depth)) } else { 0.0 };
    let modelled = jv::truthy(f.get("modelled"));
    let clearance = jv::as_num(f.get("clearance")).filter(|_| !jv::nullish(f.get("clearance"))).unwrap_or(threads::DEFAULT_CLEARANCE);
    if thread.is_some() && modelled {
        let Some(_g) = threads::thread_dims("iso", &size) else { return Err(tk("model.kernel.thread_size")) };
        let cut = kernel_edge::thread_tool(
            ctx,
            &kernel_edge::ThreadOpts {
                form: "iso".into(),
                size: size.clone(),
                external: false,
                len: th_len,
                clearance,
                hand: "right".into(),
                segments: js::max(48.0, res),
                outer: 0.0,
                ext0: 0.0,
                ext1: 0.0,
                face: Some((hd / 2.0, res)),
            },
        )?;
        let Some(cut) = cut else {
            return Err(t("model.kernel.build_failed", &[("error", F::S("Cannot read properties of null (reading 'rotate')".into()))]));
        };
        let down = if sign < 0.0 { ctx.t(cut.rotate(180.0, 0.0, 0.0)) } else { cut };
        for p in &pts {
            let m = ctx.t(down.translate([jv::n(p, "x"), jv::n(p, "y"), 0.0]));
            parts.push(m);
        }
    }
    ctx.count_tools(parts.len())?;
    let local = match parts.as_slice() {
        [one] => Rc::clone(one),
        _ => {
            let list: Vec<Manifold> = parts.iter().map(|h| (**h).clone()).collect();
            ctx.t(Manifold::union(&list))
        }
    };
    let tool = ctx.t(xf(&local, &v::frame_matrix(&frame)));
    let id = js::str_of(f.get("id"));
    ctx.hole_axes.set(&id, pts.iter().map(|p| (v::to_world(&frame, jv::n(p, "x"), jv::n(p, "y")), v::mul(frame.n, -sign))).collect());
    if let Some(tt) = thread {
        let list: Vec<Value> = pts
            .iter()
            .map(|p| {
                let td =
                    if thread_depth == 0.0 || thread_depth.is_nan() { if through { depth - 1.0 } else { depth } } else { thread_depth };
                jv::obj(vec![
                    ("o", jv::pt3(v::to_world(&frame, jv::n(p, "x"), jv::n(p, "y")))),
                    ("d", jv::pt3(v::mul(frame.n, sign))),
                    ("r", jv::num(tt[0] / 2.0)),
                    ("depth", jv::num(td)),
                    ("r0", jv::num(js::min(tt[2], tt[0] - 1.082532 * tt[1]) / 2.0)),
                    ("r1", jv::num(tt[0] / 2.0)),
                ])
            })
            .collect();
        ctx.threads.set(
            &id,
            jv::obj(vec![("callout", readers::hole_callout(f, None).into()), ("modelled", modelled.into()), ("list", Value::Array(list))]),
        );
    }
    let din = v::mul(frame.n, sign);
    let mut surfs = Vec::new();
    for p in &pts {
        let p0 = v::to_world(&frame, jv::n(p, "x"), jv::n(p, "y"));
        let pid = js::str_of(p.get("id"));
        let nm = |k: &str| format!("{id}.{k}:{pid}");
        let rmax = js::max_of([
            hd / 2.0,
            if kind == "cbore" { n(f, "cboreD") / 2.0 } else { 0.0 },
            if kind == "csink" { n(f, "csinkD") / 2.0 } else { 0.0 },
        ]);
        let own = |s: Value| surf_within(s, p0, din, rmax);
        surfs.push(own(surf_cylinder(&nm("wall"), p0, din, hd / 2.0)));
        if !through {
            surfs.push(own(surf_plane(&nm("floor"), din, v::add(p0, v::mul(din, depth)))));
        }
        if kind == "cbore" {
            surfs.push(own(surf_cylinder(&nm("cbore"), p0, din, n(f, "cboreD") / 2.0)));
            surfs.push(own(surf_plane(&nm("cfloor"), din, v::add(p0, v::mul(din, n(f, "cboreDepth"))))));
        }
        if kind == "csink" {
            let big_r = n(f, "csinkD") / 2.0;
            let r = n(f, "d") / 2.0;
            let hc = (big_r - r) / js::tan(n(f, "csinkAngle") * std::f64::consts::PI / 360.0);
            surfs.push(own(surf_cone(
                &nm("csink"),
                v::add(p0, v::mul(din, big_r * hc / (big_r - r))),
                v::mul(din, -1.0),
                (big_r - r) / hc,
            )));
        }
        if thread.is_some() && modelled {
            if let Some(g) = threads::thread_dims("iso", &size) {
                surfs.push(surf_thread(&nm("thread"), p0, din, g.d1 / 2.0 + clearance, g.d / 2.0 + clearance, 0.0, th_len));
            }
            if th_len < depth - 1e-9 {
                surfs.push(own(surf_plane(&nm("tend"), din, v::add(p0, v::mul(din, th_len)))));
            }
        }
    }
    Ok(Out {
        tools: Some(vec![Tool { m: tool, op: "cut".into(), surfs: Some(Value::Array(surfs)) }]),
        count: Some(pts.len()),
        ..Out::default()
    })
}

/// `seedTools(f, ctx)`.
fn seed_tools(f: &Value, ctx: &Ctx) -> FR<Vec<Tool>> {
    let seeds = jv::arr(f.get("seeds"));
    if seeds.is_empty() {
        return Err(tk("model.kernel.choose_seeds"));
    }
    let mut out = Vec::new();
    for s in seeds {
        let id = js::str_of(Some(s));
        let Some(tl) = ctx.tools.get(&id) else {
            return Err(match ctx.feature(&id) {
                Some(g) => t("model.kernel.seed_no_solid", &[("name", F::S(name_of(g)))]),
                None => tk("model.kernel.seed_no_solid_gone"),
            });
        };
        out.extend(tl.iter().cloned());
    }
    if out.is_empty() {
        return Err(tk("model.kernel.seeds_no_solid"));
    }
    Ok(out)
}

fn moved(ctx: &mut Ctx, seeds: &[Tool], m4: &M4, suffix: &str) -> Vec<Tool> {
    let mut tools = Vec::new();
    for sd in seeds {
        let m = ctx.t(xf(&sd.m, m4));
        tools.push(Tool { m, op: sd.op.clone(), surfs: Some(mf::move_surfs(sd.surfs.as_ref(), m4, suffix)) });
    }
    tools
}

/// `FEATURES.lpattern`.
fn feat_lpattern(f: &Value, ctx: &mut Ctx) -> FR<Out> {
    if !jv::arr(f.get("bodies")).is_empty() {
        return Err(tk("kernel.mesh.pattern_bodies"));
    }
    let seeds = seed_tools(f, ctx)?;
    let (_, d) = resolve_dir(f.get("dir").unwrap_or(&Value::Null), ctx)?;
    let dir = v::mul(d, if jv::truthy(f.get("reverse")) { -1.0 } else { 1.0 });
    let count = js::count(n(f, "count"), 1000);
    ctx.count_tools(count.saturating_mul(seeds.len()))?;
    let id = js::str_of(f.get("id"));
    let mut tools = Vec::new();
    for i in 1..count {
        let m4 = v::translation(v::mul(dir, n(f, "spacing") * js::f(i)));
        tools.extend(moved(ctx, &seeds, &m4, &format!("@{id}#{i}")));
    }
    Ok(Out { tools: Some(tools), ..Out::default() })
}

/// `FEATURES.cpattern`.
fn feat_cpattern(f: &Value, ctx: &mut Ctx) -> FR<Out> {
    if !jv::arr(f.get("bodies")).is_empty() {
        return Err(tk("kernel.mesh.pattern_bodies"));
    }
    let seeds = seed_tools(f, ctx)?;
    let (o, d) = resolve_dir(f.get("axis").unwrap_or(&Value::Null), ctx)?;
    let angle = n(f, "angle");
    let cnt = n(f, "count");
    let full = (angle - 360.0).abs() < 1e-9;
    let step = (if full {
        360.0 / cnt
    } else if cnt > 1.0 {
        angle / (cnt - 1.0)
    } else {
        0.0
    }) * (if jv::truthy(f.get("reverse")) { -1.0 } else { 1.0 });
    let count = js::count(cnt, 1000);
    ctx.count_tools(count.saturating_mul(seeds.len()))?;
    let id = js::str_of(f.get("id"));
    let mut tools = Vec::new();
    for i in 1..count {
        let m4 = v::rotation_about(o, d, step * js::f(i));
        tools.extend(moved(ctx, &seeds, &m4, &format!("@{id}#{i}")));
    }
    Ok(Out { tools: Some(tools), ..Out::default() })
}

/// `FEATURES.mirror`.
fn feat_mirror(f: &Value, ctx: &mut Ctx) -> FR<Out> {
    let frame = resolve_plane(f.get("plane").unwrap_or(&Value::Null), ctx)?;
    let m4 = v::reflection(frame.o, frame.n);
    if jv::is_str(f.get("seeds"), "body") {
        let Some(body) = ctx.body.clone() else { return Err(tk("model.kernel.nothing_to_mirror")) };
        let m = ctx.t(xf(&body, &m4));
        return Ok(Out { tools: Some(vec![Tool { m, op: "add".into(), surfs: None }]), ..Out::default() });
    }
    let seeds = seed_tools(f, ctx)?;
    ctx.count_tools(seeds.len())?;
    let id = js::str_of(f.get("id"));
    let tools = moved(ctx, &seeds, &m4, &format!("@{id}#m"));
    Ok(Out { tools: Some(tools), ..Out::default() })
}

/// The reference-geometry resolvers of the mesh kernel (faces and edges need OpenCascade).
struct MeshResolve<'c, 'a> {
    ctx: &'c Ctx<'a>,
}
impl Resolve for MeshResolve<'_, '_> {
    fn plane(&self, r: &Value) -> Result<PlaneRes, String> {
        let frame = resolve_plane(r, self.ctx)?;
        let center = if jv::is_str(r.get("kind"), "ref") {
            match self.ctx.ref_geom.get(jv::s(r, "feature")) {
                Some(Ref::Plane { center, .. }) => Some(*center),
                _ => None,
            }
        } else {
            None
        };
        Ok(PlaneRes { frame, center })
    }
    fn dir(&self, r: &Value) -> Result<(V3, V3), String> {
        resolve_dir(r, self.ctx)
    }
    fn point(&self, r: &Value) -> Result<V3, String> {
        let kind = jv::s(r, "kind");
        if kind == "fixed" {
            return Ok(p3(r.get("p")));
        }
        if kind == "ref" {
            return match self.ctx.ref_geom.get(jv::s(r, "feature")) {
                Some(Ref::Point { p }) => Ok(*p),
                _ => Err(tk("model.kernel.point_feature_not_built")),
            };
        }
        if kind == "sketch" {
            let sid = js::str_of(r.get("sketch"));
            let fr = self.ctx.frames.get(&sid).copied();
            let sf = self.ctx.feature(&sid);
            let (Some(fr), Some(sf)) = (fr, sf) else { return Err(tk("model.kernel.point_sketch_not_built")) };
            let ent = js::str_of(r.get("entity"));
            let q = if ent == "O" { Some([0.0, 0.0]) } else { sketch::point_xy(&self.ctx.sketch_value(sf), &ent) };
            let Some(q) = q else { return Err(t("model.kernel.point_gone_from", &[("name", F::S(name_of(sf)))])) };
            return Ok(v::to_world(&fr, q[0], q[1]));
        }
        Err(tk("kernel.mesh.ref_face"))
    }
    fn face(&self, _r: &Value) -> Result<FaceRes, String> {
        Err(tk("kernel.mesh.ref_face"))
    }
    fn edge(&self, _r: &Value) -> Result<EdgeRes, String> {
        Err(tk("kernel.mesh.ref_face"))
    }
}

/// `FEATURES.plane / axis / point`.
fn feat_ref(f: &Value, ctx: &mut Ctx) -> FR<Out> {
    let g = refgeom::ref_feature(f, &MeshResolve { ctx })?;
    let id = js::str_of(f.get("id"));
    if let Ref::Axis { o, d } = &g {
        ctx.axes.set(&id, (*o, *d));
    }
    ctx.ref_geom.set(&id, g);
    Ok(Out::default())
}

fn needs_key(status: Option<&str>) -> &'static str {
    match status {
        Some("error") => "model.kernel.needs.failing",
        Some("suppressed") => "model.kernel.needs.suppressed",
        Some("rolled") => "model.kernel.needs.rolled",
        _ => "model.kernel.needs.not_built",
    }
}

/// The plain-data result of a rebuild (no live solids).
pub struct Regen {
    pub results: OMap<Value>,
    pub mesh: Option<Value>,
    pub stats: Option<Value>,
    pub frames: OMap<Frame>,
    pub candidates: OMap<Vec<Cand>>,
    pub axes: OMap<(V3, V3)>,
    pub hole_axes: OMap<Vec<(V3, V3)>>,
    pub ref_geom: OMap<Ref>,
    pub faces: Option<Value>,
    pub edges: Option<Value>,
    pub threads: OMap<Value>,
    /// The sketches as the solver left them (`regenerate` mutates each sketch to its solved state).
    pub solved: OMap<Value>,
    /// The result mesh (positions as f32 values) for callers that keep working on it.
    pub raw_mesh: Option<crate::faces::Mesh>,
    /// The last `namedFaces` call of the rebuild.
    pub last_named: Option<NamedCall>,
}

fn pt_pairs(m: &OMap<(V3, V3)>) -> Value {
    let mut o = jv::Obj::new();
    for (k, (a, d)) in m.iter() {
        o.insert(k.to_string(), jv::obj(vec![("o", jv::pt3(*a)), ("d", jv::pt3(*d))]));
    }
    Value::Object(o)
}

impl Regen {
    /// The JS result object (Maps as objects in insertion order, typed arrays as arrays), as the parity kit sees it.
    pub fn to_json(&self) -> Value {
        let map = |items: Vec<(String, Value)>| Value::Object(items.into_iter().collect());
        jv::obj(vec![
            ("results", map(self.results.iter().map(|(k, x)| (k.to_string(), x.clone())).collect())),
            ("mesh", self.mesh.clone().unwrap_or(Value::Null)),
            ("stats", self.stats.clone().unwrap_or(Value::Null)),
            ("frames", map(self.frames.iter().map(|(k, x)| (k.to_string(), x.to_json())).collect())),
            (
                "candidates",
                map(self.candidates.iter().map(|(k, l)| (k.to_string(), Value::Array(l.iter().map(Cand::to_json).collect()))).collect()),
            ),
            ("axes", pt_pairs(&self.axes)),
            (
                "holeAxes",
                map(self
                    .hole_axes
                    .iter()
                    .map(|(k, l)| {
                        (
                            k.to_string(),
                            Value::Array(l.iter().map(|(a, d)| jv::obj(vec![("o", jv::pt3(*a)), ("d", jv::pt3(*d))])).collect()),
                        )
                    })
                    .collect()),
            ),
            ("refGeom", map(self.ref_geom.iter().map(|(k, g)| (k.to_string(), g.to_json())).collect())),
            ("faces", self.faces.clone().unwrap_or(Value::Null)),
            ("edges", self.edges.clone().unwrap_or(Value::Null)),
            ("named", self.faces.is_some().into()),
            ("threads", map(self.threads.iter().map(|(k, x)| (k.to_string(), x.clone())).collect())),
        ])
    }
}

/// `regenerate(part, M, { upTo })`. Never panics; every feature problem is a result status.
pub fn regenerate(part: &Value, up_to: Option<f64>) -> Regen {
    let mut ctx = Ctx::new(part);
    let mut origin: HashMap<i32, String> = HashMap::new();
    let feats = ctx.features();
    let built = match up_to {
        None => part::built_count(part),
        Some(u) => js::min(u, js::f(feats.len())),
    };
    for (i, f) in feats.iter().enumerate() {
        let fid = js::str_of(f.get("id"));
        let mut r = jv::obj(vec![("status", "ok".into())]);
        ctx.results.set(&fid, r.clone());
        if js::f(i) >= built {
            jv::set(&mut r, "status", "rolled".into());
            ctx.results.set(&fid, r);
            continue;
        }
        if jv::truthy(f.get("suppressed")) {
            jv::set(&mut r, "status", "suppressed".into());
            ctx.results.set(&fid, r);
            continue;
        }
        let blocked = part::depends_on(f).into_iter().find(|d| !ctx.results.get(d).is_some_and(|x| jv::is_str(x.get("status"), "ok")));
        if let Some(d) = blocked {
            jv::set(&mut r, "status", "skipped".into());
            let msg = match ctx.feature(&d) {
                Some(g) => {
                    let st = ctx.results.get(&d).and_then(|x| x.get("status")).and_then(Value::as_str);
                    t(needs_key(st), &[("name", F::S(name_of(g)))])
                }
                None => tk("model.kernel.needs.gone"),
            };
            jv::set(&mut r, "message", msg.into());
            ctx.results.set(&fid, r);
            continue;
        }
        ctx.solve_info = None;
        let ty = js::str_of(f.get("type"));
        if part::exact_only(&ty) && !MESH_BUILT.contains(&ty.as_str()) {
            jv::set(&mut r, "status", "skipped".into());
            jv::set(&mut r, "message", t("kernel.feature.not_built", &[("label", F::S(part::feature_label(&ty)))]).into());
            ctx.results.set(&fid, r);
            continue;
        }
        let res = build_feature(&ty, f, &mut ctx, &mut origin, &fid, &mut r);
        if let Err(msg) = res {
            jv::set(&mut r, "status", "error".into());
            jv::set(&mut r, "message", msg.into());
            if ty == "sketch"
                && let Some(s) = &ctx.solve_info
            {
                jv::set(&mut r, "solve", s.clone());
            }
        }
        ctx.results.set(&fid, r);
    }
    finish(ctx, origin)
}

#[allow(clippy::too_many_arguments)]
fn build_feature(ty: &str, f: &Value, ctx: &mut Ctx, origin: &mut HashMap<i32, String>, fid: &str, r: &mut Value) -> FR<()> {
    let out = match ty {
        "sketch" => feat_sketch(f, ctx)?,
        "extrude" => feat_extrude(f, ctx)?,
        "revolve" => feat_revolve(f, ctx)?,
        "hole" => feat_hole(f, ctx)?,
        "thread" => kernel_edge::thread_feature(f, ctx)?,
        "lpattern" => feat_lpattern(f, ctx)?,
        "cpattern" => feat_cpattern(f, ctx)?,
        "mirror" => feat_mirror(f, ctx)?,
        "plane" | "axis" | "point" => feat_ref(f, ctx)?,
        "fillet" | "chamfer" => {
            ctx.edge_surfs = Vec::new();
            let mut out = kernel_edge::edge_tools(f, ctx, ty)?;
            if let Some(tools) = out.tools.as_mut() {
                for tl in tools.iter_mut() {
                    tl.surfs = Some(Value::Array(ctx.edge_surfs.clone()));
                }
            }
            out
        }
        "shell" => kernel_edge::shell_tools(f, ctx)?,
        "draft" => kernel_edge::draft_tools(f, ctx)?,
        _ => return Err(t("model.kernel.build_failed", &[("error", F::S("FEATURES[f.type] is not a function".into()))])),
    };
    if let Some(s) = out.solve {
        jv::set(r, "solve", s);
    }
    if let Some(w) = out.warning {
        jv::set(r, "warning", w.into());
    }
    if let Some(c) = out.count.filter(|&c| c > 0) {
        jv::set(r, "count", jv::unum(c));
    }
    let Some(tools) = out.tools else { return Ok(()) };
    let mut placed = Vec::with_capacity(tools.len());
    for tl in tools {
        let m = ctx.t(tl.m.as_original());
        let oid = m.original_id();
        origin.insert(oid, fid.to_string());
        if let Some(s) = &tl.surfs {
            ctx.tool_surfs.insert(oid, s.clone());
        }
        placed.push(Tool { m, op: tl.op, surfs: tl.surfs });
    }
    ctx.tools.set(fid, placed.clone());
    let mut body = ctx.body.clone();
    for tl in &placed {
        if tl.op == "new" {
            return Err(tk("kernel.mesh.new_body"));
        }
        if tl.op == "add" {
            body = Some(match body {
                Some(b) => ctx.t(b.add(&tl.m)),
                None => Rc::clone(&tl.m),
            });
        } else if tl.op == "intersect" {
            let Some(b) = body else { return Err(tk("model.kernel.nothing_to_intersect")) };
            body = Some(ctx.t(b.intersect(&tl.m)));
        } else {
            let Some(b) = body else { return Err(tk("model.kernel.nothing_to_cut")) };
            body = Some(ctx.t(b.subtract(&tl.m)));
        }
    }
    let Some(body) = body else { return Ok(()) };
    let st = body.status();
    if st != Status::NoError {
        return Err(t("model.kernel.kernel_failed", &[("status", F::S(st.as_str().into()))]));
    }
    if ctx.body.is_some() && body.is_empty() {
        jv::set(r, "warning", tk("model.kernel.nothing_left").into());
    }
    ctx.body = if body.is_empty() { None } else { Some(body) };
    Ok(())
}

fn f32s(xs: &[f64]) -> Value {
    Value::Array(xs.iter().map(|&x| jv::num(x)).collect())
}

fn finish(ctx: Ctx, origin: HashMap<i32, String>) -> Regen {
    let mut mesh = None;
    let mut stats = None;
    let mut faces = None;
    let mut edges = None;
    let mut raw_mesh = None;
    if let Some(b) = ctx.body.as_ref() {
        let gm = b.get_mesh();
        let (m, tri_tool, lists, lists_json) = mesh_and_tools(&gm, &ctx.tool_surfs);
        let ntri = m.indices.len() / 3;
        let mut feat_ids: Vec<String> = Vec::new();
        let mut tri_feature = vec![-1i64; ntri];
        let runs = &gm.run_index;
        for r in 0..runs.len().saturating_sub(1) {
            let id = gm.run_original_id.get(r).map_or(-1, |&x| x.cast_signed());
            let mut k: i64 = -1;
            if let Some(fid) = origin.get(&id).filter(|s| !s.is_empty()) {
                k = match feat_ids.iter().position(|x| x == fid) {
                    Some(p) => i64::try_from(p).unwrap_or(-1),
                    None => {
                        feat_ids.push(fid.clone());
                        i64::try_from(feat_ids.len().saturating_sub(1)).unwrap_or(-1)
                    }
                };
            }
            let (a, z) = (runs.get(r).copied().unwrap_or(0) / 3, runs.get(r.saturating_add(1)).copied().unwrap_or(0) / 3);
            for ti in a..z {
                if let Some(x) = tri_feature.get_mut(ti as usize) {
                    *x = k;
                }
            }
        }
        let (mn, mx) = b.bounding_box();
        let bodies = b.decompose_count();
        let mut st = jv::obj(vec![
            ("volume", jv::num(b.volume())),
            ("area", jv::num(b.surface_area())),
            ("genus", jv::num(b.genus() as f64)),
            ("tris", jv::unum(ntri)),
            ("bodies", jv::unum(bodies)),
            ("bbox", jv::obj(vec![("min", jv::pt3(mn)), ("max", jv::pt3(mx))])),
        ]);
        let named = mf::named_faces(&m, &tri_tool, &lists, ctx.resolution_or64());
        *ctx.last_named.borrow_mut() = Some(NamedCall {
            positions: m.positions.clone(),
            indices: m.indices.clone(),
            tri_tool: tri_tool.clone(),
            lists: lists_json,
            segments: ctx.resolution_or64(),
        });
        let mut mo = jv::obj(vec![
            ("positions", f32s(&m.positions)),
            ("indices", Value::Array(m.indices.iter().map(|&x| Value::from(x)).collect())),
            ("triFeature", Value::Array(tri_feature.iter().map(|&x| Value::from(x)).collect())),
            ("featIds", jv::strs(&feat_ids)),
            ("triFace", Value::Array(named.tri_face.iter().map(|&x| Value::from(x)).collect())),
        ]);
        if !named.tri_face.contains(&-1) {
            jv::set(&mut mo, "edgeSegs", jv::nums(&named.edge_segs));
            jv::set(&mut mo, "segEdge", Value::Array(named.seg_edge.iter().map(|&x| jv::unum(x)).collect()));
        }
        jv::set(&mut st, "faces", jv::unum(named.faces.len()));
        jv::set(&mut st, "edges", jv::unum(named.edges.len()));
        let nj = named.to_json();
        faces = nj.get("faces").cloned();
        edges = nj.get("edges").cloned();
        mesh = Some(mo);
        stats = Some(st);
        raw_mesh = Some(crate::faces::Mesh { tri_face: Some(named.tri_face.clone()), ..m });
    }
    let mut threads = ctx.threads;
    let ids: Vec<String> = threads.keys().map(str::to_string).collect();
    for id in ids {
        if !ctx.results.get(&id).is_some_and(|x| jv::is_str(x.get("status"), "ok")) {
            threads.delete(&id);
        }
    }
    Regen {
        results: ctx.results,
        mesh,
        stats,
        frames: ctx.frames,
        candidates: ctx.candidates,
        axes: ctx.axes,
        hole_axes: ctx.hole_axes,
        ref_geom: ctx.ref_geom,
        faces,
        edges,
        threads,
        solved: ctx.solved,
        raw_mesh,
        last_named: ctx.last_named.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_part(w: f64, h: f64, d: f64) -> Value {
        serde_json::json!({
            "format": "partcraft", "version": 2, "name": "Block", "units": "mm", "resolution": 32, "seq": 3, "rollback": null,
            "features": [
                { "id": "f1", "type": "sketch", "name": "Sketch1", "plane": { "kind": "base", "name": "front" }, "sketch": {
                    "seq": 9,
                    "entities": [
                        { "id": "p1", "type": "point", "x": 0, "y": 0 }, { "id": "p2", "type": "point", "x": w, "y": 0 },
                        { "id": "p3", "type": "point", "x": w, "y": h }, { "id": "p4", "type": "point", "x": 0, "y": h },
                        { "id": "l1", "type": "line", "p1": "p1", "p2": "p2" }, { "id": "l2", "type": "line", "p1": "p2", "p2": "p3" },
                        { "id": "l3", "type": "line", "p1": "p3", "p2": "p4" }, { "id": "l4", "type": "line", "p1": "p4", "p2": "p1" }
                    ],
                    "constraints": [] } },
                { "id": "f2", "type": "extrude", "name": "Extrude1", "sketch": "f1", "op": "add", "end": "blind", "depth": d }
            ]
        })
    }

    #[test]
    fn a_block_rebuilds_with_named_faces() {
        let r = regenerate(&block_part(40.0, 20.0, 10.0), None);
        assert!(r.results.iter().all(|(_, x)| jv::is_str(x.get("status"), "ok")));
        let st = r.stats.as_ref().map(|s| (jv::n(s, "volume"), jv::n(s, "tris"), jv::n(s, "faces"))).unwrap_or_default();
        assert!((st.0 - 8000.0).abs() < 1e-9, "volume {}", st.0);
        assert_eq!(st.1, 12.0);
        assert_eq!(st.2, 6.0);
        assert!(r.candidates.get("f2").is_some_and(|c| c.len() == 6));
    }

    #[test]
    fn exact_only_features_say_not_built() {
        let mut p = block_part(10.0, 10.0, 10.0);
        if let Some(fs) = p.get_mut("features").and_then(Value::as_array_mut) {
            fs.push(serde_json::json!({ "id": "f3", "type": "loft", "name": "Loft1", "sections": [] }));
        }
        let r = regenerate(&p, None);
        let f3 = r.results.get("f3").cloned().unwrap_or(Value::Null);
        assert_eq!(jv::s(&f3, "status"), "skipped");
        assert!(!jv::s(&f3, "message").is_empty());
    }

    #[test]
    fn hostile_features_never_panic() {
        for junk in [
            serde_json::json!({ "id": "f3", "type": "extrude", "sketch": "f9", "depth": -1 }),
            serde_json::json!({ "id": "f3", "type": "lpattern", "seeds": ["f2"], "count": 1e9, "dir": { "kind": "world", "axis": "x" }, "spacing": 1 }),
            serde_json::json!({ "id": "f3", "type": "fillet", "edges": [{ "faces": [1, 2] }], "r": 1 }),
            serde_json::json!({ "id": "f3", "type": "shell", "faces": [{ "name": null }], "t": 1e300 }),
            serde_json::json!({ "id": "f3", "type": "thread", "faces": "x", "form": 5 }),
            serde_json::json!({ "id": "f3", "type": "hole", "sketch": "f1", "d": 1e308, "depth": 1e308 }),
            serde_json::json!({ "id": "f3", "type": "mirror", "plane": { "kind": "fixed", "o": [0, 0, 0], "n": [0, 0, 0] }, "seeds": "body" }),
            serde_json::json!({ "id": "f3", "type": "constructor" }),
        ] {
            let mut p = block_part(10.0, 10.0, 10.0);
            if let Some(fs) = p.get_mut("features").and_then(Value::as_array_mut) {
                fs.push(junk);
            }
            let r = regenerate(&p, None);
            assert!(r.results.get("f3").is_some());
            let _ = r.to_json();
        }
    }
}
