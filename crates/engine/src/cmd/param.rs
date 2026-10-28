//! pcommands.js — the parametric commands, AutoCAD's names and prompts: GEOMCONSTRAINT (GCON) and GCHORIZONTAL …
//! GCFIX; DIMCONSTRAINT (DCON) and DCLINEAR … DCCONVERT; DELCONSTRAINT, AUTOCONSTRAIN, PARAMETERS (the window's panel,
//! else the list), -PARAMETERS (New / Edit / Rename / Delete / ?), CONSTRAINTBAR, DCDISPLAY. A click takes the
//! constraint point nearest it (end, middle, centre, vertex) where a point is asked for, and the line / circle /
//! polyline segment under it where an object is; the first object stays and the second moves, as in AutoCAD.
//!
//! Not here: SWBOUT (swsketch.js: a SolidWorks macro).

use cc_cad::entity::Geo;
use cc_cad::js;
use cc_cad_geom::jsmath::JsMath;
use cc_gcons::con::Ref;
use cc_gcons::gcons::{
    GEOMETRIC, MakeOpts, cons, constraint_points, curve_ref, dim_entity_for, is_dim, make_constraint, nearest_point_ref, status_text,
    type_label,
};
use cc_gcons::gsolve::{ref_kind, ref_line, ref_point, seg_count, solvable};
use cc_gcons::paramsdoc::{self, AddOpts, Res};
use cc_geom::P2;

use crate::engine::{Ctx, Opts, R, Reply, Sel, doc_err};
use crate::input::{Def, Opt};

/// What a command remembers about constraints (`E.lastGc`, `E.lastDc`, `E.autoc`, `host.constraintView`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParamMemo {
    pub last_gc: Option<String>,
    pub last_dc: Option<String>,
    pub autoc: Option<(f64, f64)>,
    pub view: Option<ConstraintView>,
}
/// `host.constraintView`: constraint bars for all / none (with exceptions) and dimensional constraints shown.
#[derive(Clone, Debug, PartialEq)]
pub struct ConstraintView {
    pub bars_all: bool,
    pub hidden: Vec<u64>,
    pub dims: bool,
}
impl Default for ConstraintView {
    fn default() -> Self {
        ConstraintView { bars_all: true, hidden: Vec::new(), dims: true }
    }
}

/// commands-api.js `option(key, label)`: the registry's options are not translated (no `en`), as pcommands.js gets them.
fn ro(key: &str, label: &str) -> Opt {
    Opt { key: key.into(), label: label.into(), en: None, hidden: false }
}

fn model_only(c: &Ctx) -> bool {
    if c.doc().in_model() {
        return true;
    }
    c.print(&c.t("cmd.constraints_work_model_tab"));
    false
}
/// `say(c, r, done)`
fn say(c: &Ctx, r: &Res, done: Option<String>) -> bool {
    if r.ok {
        if let Some(d) = done {
            c.print(&d);
        }
        if let Some(m) = &r.message {
            c.print(m);
        }
    } else {
        c.print(&r.error);
    }
    r.ok
}

/// What a pick test takes from an object at a point → the reference key.
type Test = fn(&cc_cad::entity::Entity, P2) -> Option<String>;

fn as_point(e: &cc_cad::entity::Entity, p: P2) -> Option<String> {
    nearest_point_ref(e, p).map(|(k, _)| k)
}
fn as_kind(e: &cc_cad::entity::Entity, p: P2, kinds: &[char]) -> Option<String> {
    let k = curve_ref(e, p)?;
    ref_kind(e, &k).filter(|x| kinds.contains(x)).map(|_| k)
}
fn as_line(e: &cc_cad::entity::Entity, p: P2) -> Option<String> {
    as_kind(e, p, &['L'])
}
fn as_round(e: &cc_cad::entity::Entity, p: P2) -> Option<String> {
    as_kind(e, p, &['R'])
}
fn as_curve(e: &cc_cad::entity::Entity, p: P2) -> Option<String> {
    as_kind(e, p, &['L', 'R'])
}
fn as_whole(e: &cc_cad::entity::Entity, _p: P2) -> Option<String> {
    solvable(e).then(|| "*".to_string())
}
fn as_angular(e: &cc_cad::entity::Entity, p: P2) -> Option<String> {
    if matches!(e.geo, Geo::Arc(_)) { Some("R".into()) } else { as_line(e, p) }
}

/// A pick: an option, or the reference taken from an object.
enum Picked {
    None,
    Opt(String),
    Ref { r: Ref, id: u64 },
}

fn option_key(options: &[Opt], label: &str) -> String {
    options.iter().find(|o| o.label == label || o.key == label).map_or_else(|| label.to_string(), |o| o.key.clone())
}

/// `pick(c, prompt, test, why, opts)`
async fn pick(c: &Ctx, prompt: String, test: Test, why: &str, options: Vec<Opt>, default: Option<&str>) -> R<Picked> {
    loop {
        let opts = Opts { options: options.clone(), default: default.map(|d| Def::Str(d.into())), ..Opts::default() };
        let p = c.entity(prompt.clone(), opts).await?;
        let (id, at) = match p {
            Reply::Str(s) => return Ok(Picked::Opt(option_key(&options, &s))),
            Reply::Opt(k) => return Ok(Picked::Opt(k)),
            Reply::Pick { id, at, .. } => (id, at),
            _ => return Ok(Picked::None),
        };
        let e = c.doc().get(id).cloned();
        let r = e.as_ref().filter(|e| solvable(e)).and_then(|e| test(e, at));
        if let (Some(k), Some(e)) = (r, e) {
            let eid = e.id.unwrap_or(id);
            return Ok(Picked::Ref { r: Ref { e: eid, k }, id: eid });
        }
        c.print(why);
    }
}
async fn first_point(c: &Ctx, prompt: String, options: Vec<Opt>, default: Option<&str>) -> R<Picked> {
    let why = c.t("cmd.select_line_polyline_circle");
    pick(c, prompt, as_point, &why, options, default).await
}

fn tr(c: &Ctx) -> cc_cad::i18n::Catalog {
    c.sh().lang.cat()
}

/// `add(c, type, refs, extra)`
fn add(c: &Ctx, typ: &str, refs: &[Ref], o: &AddOpts) -> R<bool> {
    let t = tr(c);
    let r = paramsdoc::add_constraint(&mut c.sh().doc.borrow_mut(), &t, typ, refs, o).map_err(doc_err)?;
    c.flush_notes();
    let done = GEOMETRIC.iter().any(|g| g.0 == typ).then(|| c.tv("cmd.gc.added", &[("what", type_label(&t, typ))]));
    Ok(say(c, &r, done))
}

async fn horizontal_like(c: &Ctx, typ: &str) -> R {
    let w_line = c.t("cmd.select_line_straight_polyline");
    let a = pick(c, c.t("cmd.select_object"), as_line, &w_line, vec![ro("2P", "2Points")], Some("2Points")).await?;
    match a {
        Picked::None => Ok(()),
        Picked::Opt(k) if k == "2P" => {
            let Picked::Ref { r: p, .. } = first_point(c, c.t("cmd.select_first_point"), Vec::new(), None).await? else { return Ok(()) };
            let Picked::Ref { r: q, .. } = first_point(c, c.t("cmd.select_second_point"), Vec::new(), None).await? else { return Ok(()) };
            add(c, typ, &[p, q], &AddOpts::default())?;
            Ok(())
        }
        Picked::Opt(_) => {
            // pick() gives back any option typed: JS adds the constraint on `a.ref` (undefined) → the command errs
            c.print("Error: Cannot read properties of undefined (reading 'e')");
            Ok(())
        }
        Picked::Ref { r, .. } => {
            add(c, typ, &[r], &AddOpts::default())?;
            Ok(())
        }
    }
}

/// `twoCurves(c, type, test1, test2, why1, why2)`
async fn two_curves(c: &Ctx, t1: Test, t2: Test, why1: &str, why2: &str) -> R<Option<(Ref, Ref)>> {
    let Picked::Ref { r: a, id: aid } = pick(c, c.t("cmd.fillet.first"), t1, why1, Vec::new(), None).await? else { return Ok(None) };
    c.set_selection(&[aid]);
    let b = pick(c, c.t("cmd.select_second_object"), t2, why2, Vec::new(), None).await?;
    c.set_selection(&[]);
    let Picked::Ref { r: b, .. } = b else { return Ok(None) };
    Ok(Some((a, b)))
}

fn kind_of(c: &Ctx, r: &Ref) -> Option<char> {
    c.doc().get(r.e).and_then(|e| ref_kind(e, &r.k))
}

async fn gc_run(c: &Ctx, name: &str) -> R {
    let w_line = c.t("cmd.select_line_straight_polyline");
    let w_round = c.t("cmd.select_circle_arc_polyline");
    let w_curve = c.t("cmd.select_line_circle_arc");
    match name {
        "GCHORIZONTAL" => horizontal_like(c, "horizontal").await,
        "GCVERTICAL" => horizontal_like(c, "vertical").await,
        "GCPERPENDICULAR" | "GCPARALLEL" | "GCCOLLINEAR" => {
            let typ = match name {
                "GCPERPENDICULAR" => "perpendicular",
                "GCPARALLEL" => "parallel",
                _ => "collinear",
            };
            if let Some((a, b)) = two_curves(c, as_line, as_line, &w_line, &w_line).await? {
                add(c, typ, &[a, b], &AddOpts::default())?;
            }
            Ok(())
        }
        "GCCONCENTRIC" => {
            if let Some((a, b)) = two_curves(c, as_round, as_round, &w_round, &w_round).await? {
                add(c, "concentric", &[a, b], &AddOpts::default())?;
            }
            Ok(())
        }
        "GCEQUAL" => {
            let Some((a, b)) = two_curves(c, as_curve, as_curve, &w_curve, &w_curve).await? else { return Ok(()) };
            if kind_of(c, &a) != kind_of(c, &b) {
                c.print(&c.t("cmd.equal_takes_two_lines"));
                return Ok(());
            }
            add(c, "equal", &[a, b], &AddOpts::default())?;
            Ok(())
        }
        "GCTANGENT" => {
            let Some((a, b)) = two_curves(c, as_curve, as_curve, &w_curve, &w_curve).await? else { return Ok(()) };
            let (ka, kb) = (kind_of(c, &a), kind_of(c, &b));
            if ka == Some('L') && kb == Some('L') {
                c.print(&c.t("cmd.tangent_needs_circle_arc"));
                return Ok(());
            }
            let pin = a.e;
            let refs = if ka == Some('R') && kb == Some('L') { [b, a] } else { [a, b] };
            add(c, "tangent", &refs, &AddOpts { pin_entity: Some(pin), ..AddOpts::default() })?;
            Ok(())
        }
        "GCCOINCIDENT" => {
            let a =
                first_point(c, c.t("cmd.select_first_point"), vec![ro("O", "Object"), ro("A", "Autoconstrain")], Some("Object")).await?;
            match a {
                Picked::None => Ok(()),
                Picked::Opt(k) if k == "A" => autoconstrain(c, true).await,
                Picked::Opt(k) if k == "O" => {
                    let Picked::Ref { r: o, id: oid } = pick(c, c.t("cmd.select_object"), as_curve, &w_curve, Vec::new(), None).await?
                    else {
                        return Ok(());
                    };
                    c.set_selection(&[oid]);
                    let p = first_point(c, c.t("cmd.select_point"), Vec::new(), None).await?;
                    c.set_selection(&[]);
                    let Picked::Ref { r: p, .. } = p else { return Ok(()) };
                    let pin = o.e;
                    add(c, "coincident", &[p, o], &AddOpts { pin_entity: Some(pin), ..AddOpts::default() })?;
                    Ok(())
                }
                Picked::Opt(_) => {
                    c.print("Error: Cannot read properties of undefined (reading 'e')");
                    Ok(())
                }
                Picked::Ref { r: a, id: aid } => {
                    c.set_selection(&[aid]);
                    let b = first_point(c, c.t("cmd.select_second_point"), vec![ro("O", "Object")], Some("Object")).await?;
                    c.set_selection(&[]);
                    match b {
                        Picked::None => Ok(()),
                        Picked::Opt(k) if k == "O" => {
                            let Picked::Ref { r: o, .. } = pick(c, c.t("cmd.select_object"), as_curve, &w_curve, Vec::new(), None).await?
                            else {
                                return Ok(());
                            };
                            let pin = a.e;
                            add(c, "coincident", &[a, o], &AddOpts { pin_entity: Some(pin), ..AddOpts::default() })?;
                            Ok(())
                        }
                        Picked::Opt(_) => {
                            c.print("Error: Cannot read properties of undefined (reading 'e')");
                            Ok(())
                        }
                        Picked::Ref { r: b, .. } => {
                            let pin = a.e;
                            add(c, "coincident", &[b, a], &AddOpts { pin_entity: Some(pin), ..AddOpts::default() })?;
                            Ok(())
                        }
                    }
                }
            }
        }
        "GCFIX" => {
            let a = first_point(c, c.t("cmd.select_point"), vec![ro("O", "Object")], Some("Object")).await?;
            match a {
                Picked::None => Ok(()),
                Picked::Opt(k) if k == "O" => {
                    let w = c.t("cmd.select_line_polyline_circle");
                    let Picked::Ref { r: o, .. } = pick(c, c.t("cmd.area.object"), as_whole, &w, Vec::new(), None).await? else {
                        return Ok(());
                    };
                    add(c, "fix", &[o], &AddOpts::default())?;
                    Ok(())
                }
                Picked::Opt(_) => {
                    c.print("Error: Cannot read properties of undefined (reading 'e')");
                    Ok(())
                }
                Picked::Ref { r, .. } => {
                    add(c, "fix", &[r], &AddOpts::default())?;
                    Ok(())
                }
            }
        }
        "GCSYMMETRIC" => {
            let w2 = c.t("cmd.select_line_circle_arc_2");
            let a = pick(c, c.t("cmd.fillet.first"), as_curve, &w2, vec![ro("2P", "2Points")], Some("2Points")).await?;
            let refs = match a {
                Picked::None => return Ok(()),
                Picked::Opt(k) if k == "2P" => {
                    let Picked::Ref { r: p, .. } = first_point(c, c.t("cmd.select_first_point"), Vec::new(), None).await? else {
                        return Ok(());
                    };
                    let Picked::Ref { r: q, .. } = first_point(c, c.t("cmd.select_second_point"), Vec::new(), None).await? else {
                        return Ok(());
                    };
                    vec![p, q]
                }
                Picked::Opt(_) => {
                    // `a.id` undefined: setSelection(new Set([undefined])), then the second pick …
                    c.set_selection(&[]);
                    let b = pick(c, c.t("cmd.select_second_object"), as_curve, &w2, Vec::new(), None).await?;
                    if !matches!(b, Picked::Ref { .. }) {
                        return Ok(());
                    }
                    let m = pick(c, c.t("cmd.select_symmetry_line"), as_line, &w_line, Vec::new(), None).await?;
                    c.set_selection(&[]);
                    if !matches!(m, Picked::Ref { .. }) {
                        return Ok(());
                    }
                    c.print("Error: Cannot read properties of undefined (reading 'e')");
                    return Ok(());
                }
                Picked::Ref { r: a, id: aid } => {
                    c.set_selection(&[aid]);
                    let Picked::Ref { r: b, .. } = pick(c, c.t("cmd.select_second_object"), as_curve, &w2, Vec::new(), None).await? else {
                        return Ok(());
                    };
                    vec![a, b]
                }
            };
            let m = pick(c, c.t("cmd.select_symmetry_line"), as_line, &w_line, Vec::new(), None).await?;
            c.set_selection(&[]);
            let Picked::Ref { r: m, .. } = m else { return Ok(()) };
            let mut all = refs;
            all.push(m);
            add(c, "symmetric", &all, &AddOpts::default())?;
            Ok(())
        }
        _ => Ok(()),
    }
}

pub async fn gc(c: Ctx, name: &'static str) -> R {
    if model_only(&c) {
        gc_run(&c, name).await?;
    }
    Ok(())
}

const GC_RUN: &[(&str, &str, &str)] = &[
    ("H", "Horizontal", "GCHORIZONTAL"),
    ("V", "Vertical", "GCVERTICAL"),
    ("P", "Perpendicular", "GCPERPENDICULAR"),
    ("PA", "PArallel", "GCPARALLEL"),
    ("T", "Tangent", "GCTANGENT"),
    ("C", "Coincident", "GCCOINCIDENT"),
    ("CON", "CONcentric", "GCCONCENTRIC"),
    ("COL", "COLlinear", "GCCOLLINEAR"),
    ("S", "Symmetric", "GCSYMMETRIC"),
    ("E", "Equal", "GCEQUAL"),
    ("F", "Fix", "GCFIX"),
];

pub async fn geomconstraint(c: Ctx) -> R {
    if !model_only(&c) {
        return Ok(());
    }
    let options: Vec<Opt> = GC_RUN.iter().map(|(k, l, _)| ro(k, l)).collect();
    let def = c.st().memo.param.last_gc.clone().unwrap_or_else(|| "Horizontal".into());
    let k = c
        .keyword(
            c.t("cmd.geomconstraint.enter_constraint_type"),
            Opts { options: options.clone(), default: Some(Def::Str(def)), ..Opts::default() },
        )
        .await?;
    let key = match k {
        Reply::Opt(o) => o,
        Reply::Str(s) => {
            let k = option_key(&options, &s);
            if k.is_empty() { "H".into() } else { k }
        }
        _ => "H".into(),
    };
    let Some((_, label, run)) = GC_RUN.iter().find(|(k, _, _)| *k == key) else { return Ok(()) };
    c.st().memo.param.last_gc = Some((*label).to_string());
    gc_run(&c, run).await
}

// ---- dimensional

fn linear_rot(p1: [f64; 2], p2: [f64; 2], cur: P2) -> &'static str {
    let (min_x, max_x, min_y, max_y) = (js::min(p1[0], p2[0]), js::max(p1[0], p2[0]), js::min(p1[1], p2[1]), js::max(p1[1], p2[1]));
    let out_x = if cur.x < min_x {
        min_x - cur.x
    } else if cur.x > max_x {
        cur.x - max_x
    } else {
        0.0
    };
    let out_y = if cur.y < min_y {
        min_y - cur.y
    } else if cur.y > max_y {
        cur.y - max_y
    } else {
        0.0
    };
    if out_y >= out_x { "hdist" } else { "vdist" }
}

/// `twoPoints(c)`
async fn two_points(c: &Ctx) -> R<Option<[Ref; 2]>> {
    let a = first_point(c, c.t("cmd.geomconstraint.specify_first_constraint_point"), vec![ro("O", "Object")], Some("Object")).await?;
    match a {
        Picked::None => Ok(None),
        Picked::Opt(k) if k == "O" => {
            let w = c.t("cmd.select_line_straight_polyline");
            let Picked::Ref { r: o, id } = pick(c, c.t("cmd.area.object"), as_line, &w, Vec::new(), None).await? else { return Ok(None) };
            let Some(e) = c.doc().get(id).cloned() else { return Ok(None) };
            if matches!(e.geo, Geo::Line(_)) {
                return Ok(Some([Ref { e: id, k: "p1".into() }, Ref { e: id, k: "p2".into() }]));
            }
            let n = if let Geo::Poly(p) = &e.geo { p.pts.len().max(1) } else { 1 };
            let i: usize = o.k.get(1..).and_then(|s| s.parse().ok()).unwrap_or(0);
            Ok(Some([Ref { e: id, k: format!("v{i}") }, Ref { e: id, k: format!("v{}", i.saturating_add(1).checked_rem(n).unwrap_or(0)) }]))
        }
        Picked::Opt(_) => {
            // `a.id` is undefined: the JS picks the second point next and then refers to `a.ref` (undefined)
            c.set_selection(&[]);
            let b = first_point(c, c.t("cmd.geomconstraint.specify_second_constraint_point"), Vec::new(), None).await?;
            c.set_selection(&[]);
            if !matches!(b, Picked::Ref { .. }) {
                return Ok(None);
            }
            c.print("Error: Cannot read properties of undefined (reading 'e')");
            Ok(None)
        }
        Picked::Ref { r: a, id } => {
            c.set_selection(&[id]);
            let b = first_point(c, c.t("cmd.geomconstraint.specify_second_constraint_point"), Vec::new(), None).await?;
            c.set_selection(&[]);
            let Picked::Ref { r: b, .. } = b else { return Ok(None) };
            Ok(Some([a, b]))
        }
    }
}

/// How the dimension type is chosen: fixed, or by the cursor (DCLINEAR).
#[derive(Clone, Copy)]
enum TypeFor {
    Fixed(&'static str),
    Linear,
}

/// `placeAndAdd(c, typeFor, refs)`
async fn place_and_add(c: &Ctx, tf: TypeFor, refs: Vec<Ref>) -> R {
    let make = {
        let refs = refs.clone();
        move |doc: &cc_cad::Doc,
              t: &cc_cad::i18n::Catalog,
              cur: P2|
              -> Result<(cc_gcons::con::Con, &'static str, Option<cc_cad::entity::Entity>), String> {
            let typ = match tf {
                TypeFor::Fixed(t) => t,
                TypeFor::Linear => {
                    let p = |i: usize| refs.get(i).and_then(|r| doc.get(r.e).and_then(|e| ref_point(e, &r.k))).unwrap_or([f64::NAN; 2]);
                    linear_rot(p(0), p(1), cur)
                }
            };
            let m = make_constraint(doc, t, typ, &refs, &MakeOpts { loc: Some(cur), id: Some("k0".into()), ..MakeOpts::default() })?;
            let d = dim_entity_for(doc, &m, None);
            Ok((m, typ, d))
        }
    };
    let t = tr(c);
    let probe_at = c.sh().host.borrow().cursor().or_else(|| {
        let d = c.doc();
        refs.first().and_then(|r| d.get(r.e).and_then(|e| ref_point(e, &r.k))).map(|p| P2::new(p[0], p[1]))
    });
    let probe = make(&c.doc(), &t, probe_at.unwrap_or(P2::new(f64::NAN, f64::NAN)));
    if let Err(e) = probe {
        c.print(&e);
        return Ok(());
    }
    {
        let make = make.clone();
        let t2 = t.clone();
        c.preview(Some(std::rc::Rc::new(move |sh: &crate::engine::Shared, cur: P2| {
            let d = sh.doc.borrow();
            match make(&d, &t2, cur) {
                Ok((_, _, Some(e))) => vec![e],
                _ => Vec::new(),
            }
        })));
    }
    let loc = c.point(c.t("cmd.explode.specify_dimension_line_location"), Opts::default()).await?;
    c.preview(None);
    let Some(loc) = loc.point() else { return Ok(()) };
    let m = make(&c.doc(), &t, loc);
    let (mc, typ, _) = match m {
        Ok(x) => x,
        Err(e) => {
            c.print(&e);
            return Ok(());
        }
    };
    let shown = js::num_to_string(js::round(mc.value_num() * 1e6) / 1e6);
    let answer = c.string(format!("Dimension text <{}={shown}>", mc.name().unwrap_or_default()), Opts::default()).await?;
    let typed = js::trim(&crate::cmd::block::s_reply(&answer)).to_string();
    let (name, ex) = if typed.is_empty() {
        (None, None)
    } else {
        let (n, e) = paramsdoc::parse_dim_input(&typed);
        (n, Some(e))
    };
    let name = name.filter(|n| Some(n.as_str()) != mc.name());
    let o = AddOpts { loc: Some(loc), expr: ex.filter(|e| !e.is_empty()), name, ..AddOpts::default() };
    let r = paramsdoc::add_constraint(&mut c.sh().doc.borrow_mut(), &t, typ, &refs, &o).map_err(doc_err)?;
    c.flush_notes();
    if say(c, &r, None)
        && let Some(k) = &r.c
    {
        let v = js::num_to_string(js::round(k.value_num() * 1e6) / 1e6) + if k.typ == "angle" { "°" } else { "" };
        c.print(&c.tv("cmd.dc.added", &[("name", k.name().unwrap_or_default().to_string()), ("value", v)]));
    }
    Ok(())
}

async fn dc_run(c: &Ctx, name: &str) -> R {
    let w_line = c.t("cmd.select_line_straight_polyline");
    let w_round = c.t("cmd.select_circle_arc_polyline");
    match name {
        "DCLINEAR" => {
            if let Some(r) = two_points(c).await? {
                place_and_add(c, TypeFor::Linear, r.to_vec()).await?;
            }
            Ok(())
        }
        "DCHORIZONTAL" | "DCVERTICAL" => {
            if let Some(r) = two_points(c).await? {
                place_and_add(c, TypeFor::Fixed(if name == "DCHORIZONTAL" { "hdist" } else { "vdist" }), r.to_vec()).await?;
            }
            Ok(())
        }
        "DCALIGNED" => {
            let a = first_point(
                c,
                c.t("cmd.geomconstraint.specify_first_constraint_point"),
                vec![ro("O", "Object"), ro("P", &c.t("cmd.geomconstraint.point_line")), ro("2L", "2Lines")],
                Some("Object"),
            )
            .await?;
            match a {
                Picked::None => Ok(()),
                Picked::Opt(k) if k == "P" => {
                    let Picked::Ref { r: p, .. } =
                        first_point(c, c.t("cmd.geomconstraint.specify_constraint_point"), Vec::new(), None).await?
                    else {
                        return Ok(());
                    };
                    let Picked::Ref { r: l, .. } =
                        pick(c, c.t("cmd.geomconstraint.select_line"), as_line, &w_line, Vec::new(), None).await?
                    else {
                        return Ok(());
                    };
                    place_and_add(c, TypeFor::Fixed("dist"), vec![p, l]).await
                }
                Picked::Opt(k) if k == "2L" => {
                    let Picked::Ref { r: l1, .. } = pick(c, c.t("cmd.chamfer.first"), as_line, &w_line, Vec::new(), None).await? else {
                        return Ok(());
                    };
                    let Picked::Ref { r: l2, .. } =
                        pick(c, c.t("cmd.geomconstraint.select_second_line_make"), as_line, &w_line, Vec::new(), None).await?
                    else {
                        return Ok(());
                    };
                    let t = tr(c);
                    let r = paramsdoc::add_constraint(
                        &mut c.sh().doc.borrow_mut(),
                        &t,
                        "parallel",
                        &[l1.clone(), l2.clone()],
                        &AddOpts::default(),
                    )
                    .map_err(doc_err)?;
                    c.flush_notes();
                    if !say(c, &r, None) {
                        return Ok(());
                    }
                    place_and_add(c, TypeFor::Fixed("dist"), vec![l1, l2]).await
                }
                Picked::Opt(k) if k == "O" => {
                    let Picked::Ref { r: o, id } = pick(c, c.t("cmd.area.object"), as_line, &w_line, Vec::new(), None).await? else {
                        return Ok(());
                    };
                    let Some(e) = c.doc().get(id).cloned() else { return Ok(()) };
                    let refs = if matches!(e.geo, Geo::Line(_)) {
                        vec![Ref { e: id, k: "p1".into() }, Ref { e: id, k: "p2".into() }]
                    } else {
                        let n = if let Geo::Poly(p) = &e.geo { p.pts.len().max(1) } else { 1 };
                        let i: usize = o.k.get(1..).and_then(|s| s.parse().ok()).unwrap_or(0);
                        vec![
                            Ref { e: id, k: format!("v{i}") },
                            Ref { e: id, k: format!("v{}", i.saturating_add(1).checked_rem(n).unwrap_or(0)) },
                        ]
                    };
                    place_and_add(c, TypeFor::Fixed("dist"), refs).await
                }
                Picked::Opt(_) => {
                    let b = first_point(c, c.t("cmd.geomconstraint.specify_second_constraint_point"), Vec::new(), None).await?;
                    if !matches!(b, Picked::Ref { .. }) {
                        return Ok(());
                    }
                    c.print("Error: Cannot read properties of undefined (reading 'e')");
                    Ok(())
                }
                Picked::Ref { r: a, .. } => {
                    let Picked::Ref { r: b, .. } =
                        first_point(c, c.t("cmd.geomconstraint.specify_second_constraint_point"), Vec::new(), None).await?
                    else {
                        return Ok(());
                    };
                    place_and_add(c, TypeFor::Fixed("dist"), vec![a, b]).await
                }
            }
        }
        "DCRADIUS" | "DCDIAMETER" => {
            let o = pick(c, c.t("cmd.mea.arc"), as_round, &w_round, Vec::new(), None).await?;
            if let Picked::Ref { r, .. } = o {
                place_and_add(c, TypeFor::Fixed(if name == "DCRADIUS" { "radius" } else { "diameter" }), vec![r]).await?;
            }
            Ok(())
        }
        "DCANGULAR" => {
            let w = c.t("cmd.geomconstraint.select_line_arc");
            let Picked::Ref { r: a, id } =
                pick(c, c.t("cmd.geomconstraint.select_first_line_arc"), as_angular, &w, Vec::new(), None).await?
            else {
                return Ok(());
            };
            if c.doc().get(id).is_some_and(|e| matches!(e.geo, Geo::Arc(_))) {
                return place_and_add(c, TypeFor::Fixed("angle"), vec![a]).await;
            }
            c.set_selection(&[id]);
            let b = pick(c, c.t("cmd.mea.line2"), as_line, &w_line, Vec::new(), None).await?;
            c.set_selection(&[]);
            let Picked::Ref { r: b, .. } = b else { return Ok(()) };
            place_and_add(c, TypeFor::Fixed("angle"), vec![a, b]).await
        }
        "DCCONVERT" => {
            let ids = c.select(Some(c.t("cmd.geomconstraint.select_dimensions_convert")), false).await?;
            let (mut n, mut skipped) = (0usize, 0usize);
            let t = tr(c);
            for id in ids.0.iter().copied() {
                let d = c.doc().get(id).cloned();
                let Some(d) = d.filter(|d| matches!(d.geo, Geo::Dim(_))) else {
                    skipped = skipped.saturating_add(1);
                    continue;
                };
                let conv = convert_dim(&c.doc(), &d);
                let Some((typ, refs)) = conv else {
                    skipped = skipped.saturating_add(1);
                    continue;
                };
                let loc = if let Geo::Dim(dd) = &d.geo { dd.loc } else { None };
                let r = paramsdoc::add_constraint(&mut c.sh().doc.borrow_mut(), &t, typ, &refs, &AddOpts { loc, ..AddOpts::default() })
                    .map_err(doc_err)?;
                c.flush_notes();
                if !r.ok {
                    c.print(&c.tv("cmd.dcconvert.dim_error", &[("id", id.to_string()), ("why", r.error)]));
                    skipped = skipped.saturating_add(1);
                    continue;
                }
                c.change("DCCONVERT", |doc| doc.remove(id))?;
                n = n.saturating_add(1);
            }
            c.set_selection(&[]);
            let mut s = c.tv("cmd.dcconvert.converted", &[("n", n.to_string())]);
            if skipped > 0 {
                s.push(' ');
                s.push_str(&c.tv("cmd.dcconvert.skipped", &[("n", skipped.to_string())]));
            }
            c.print(&s);
            Ok(())
        }
        _ => Ok(()),
    }
}

pub async fn dc(c: Ctx, name: &'static str) -> R {
    if model_only(&c) {
        dc_run(&c, name).await?;
    }
    Ok(())
}

const DC_RUN: &[(&str, &str, &str)] = &[
    ("LI", "LInear", "DCLINEAR"),
    ("H", "Horizontal", "DCHORIZONTAL"),
    ("V", "Vertical", "DCVERTICAL"),
    ("AL", "ALigned", "DCALIGNED"),
    ("AN", "ANgular", "DCANGULAR"),
    ("R", "Radial", "DCRADIUS"),
    ("D", "Diameter", "DCDIAMETER"),
    ("C", "Convert", "DCCONVERT"),
];

pub async fn dimconstraint(c: Ctx) -> R {
    if !model_only(&c) {
        return Ok(());
    }
    let options: Vec<Opt> = DC_RUN.iter().map(|(k, l, _)| ro(k, l)).collect();
    let def = c.st().memo.param.last_dc.clone().unwrap_or_else(|| "ALigned".into());
    let p = c
        .entity(
            c.t("cmd.dimconstraint.select_associative_dimension_convert"),
            Opts { options: options.clone(), default: Some(Def::Str(def)), ..Opts::default() },
        )
        .await?;
    let key = match &p {
        Reply::Opt(k) => Some(k.clone()),
        Reply::Str(s) => Some(option_key(&options, s)),
        _ => None,
    };
    if let Some(key) = key {
        let Some((_, label, run)) = DC_RUN.iter().find(|(k, _, _)| *k == key) else { return Ok(()) };
        c.st().memo.param.last_dc = Some((*label).to_string());
        return dc_run(&c, run).await;
    }
    let Reply::Pick { id, .. } = p else { return Ok(()) };
    let d = c.doc().get(id).cloned();
    let Some(d) = d.filter(|d| matches!(d.geo, Geo::Dim(_))) else {
        c.print(&c.t("cmd.dimconstraint.not_dimension_choose_option"));
        return Ok(());
    };
    let Some((typ, refs)) = convert_dim(&c.doc(), &d) else {
        c.print(&c.t("cmd.dimconstraint.dimension_not_lines_circles"));
        return Ok(());
    };
    let loc = if let Geo::Dim(dd) = &d.geo { dd.loc } else { None };
    let t = tr(&c);
    let r =
        paramsdoc::add_constraint(&mut c.sh().doc.borrow_mut(), &t, typ, &refs, &AddOpts { loc, ..AddOpts::default() }).map_err(doc_err)?;
    c.flush_notes();
    if !say(&c, &r, None) {
        return Ok(());
    }
    c.change("DCCONVERT", |doc| doc.remove(id))?;
    c.print(&c.tv("cmd.dcconvert.to", &[("name", r.c.as_ref().and_then(|k| k.name().map(str::to_string)).unwrap_or_default())]));
    Ok(())
}

/// `convertDim(doc, d)`: a plain dimension → the constraint it stands for.
pub fn convert_dim(doc: &cc_cad::Doc, d: &cc_cad::entity::Entity) -> Option<(&'static str, Vec<Ref>)> {
    let Geo::Dim(dm) = &d.geo else { return None };
    let ents: Vec<cc_cad::entity::Entity> = doc.model().filter(|e| solvable(e)).cloned().collect();
    let mut size = 1.0f64;
    for e in &ents {
        for (_, x, y) in constraint_points(e) {
            size = js::max(size, js::max(x.abs(), y.abs()));
        }
    }
    let tol = 1e-7 * size;
    let point_at = |p: P2| -> Option<Ref> {
        for e in &ents {
            for (k, x, y) in constraint_points(e) {
                if js::hypot(x - p.x, y - p.y) <= tol {
                    return Some(Ref { e: e.id?, k });
                }
            }
        }
        None
    };
    let nan = P2::new(f64::NAN, f64::NAN);
    match dm.kind() {
        "linear" | "aligned" => {
            let a = point_at(dm.p1.unwrap_or(nan))?;
            let b = point_at(dm.p2.unwrap_or(nan))?;
            if dm.kind() == "aligned" {
                return Some(("dist", vec![a, b]));
            }
            let pi = std::f64::consts::PI;
            let rot = (js::or(dm.rot, 0.0) % pi + pi) % pi;
            if rot.abs() < 1e-9 || (rot - pi).abs() < 1e-9 {
                return Some(("hdist", vec![a, b]));
            }
            if (rot - pi / 2.0).abs() < 1e-9 {
                return Some(("vdist", vec![a, b]));
            }
            Some(("dist", vec![a, b]))
        }
        "radius" | "diameter" => {
            let (p1, cc) = (dm.p1.unwrap_or(nan), dm.c.unwrap_or(nan));
            let r = js::hypot(p1.x - cc.x, p1.y - cc.y);
            let e = ents.iter().find(|x| match &x.geo {
                Geo::Circle(k) => js::hypot(k.cx - cc.x, k.cy - cc.y) <= tol && (k.r - r).abs() <= tol,
                Geo::Arc(k) => js::hypot(k.cx - cc.x, k.cy - cc.y) <= tol && (k.r - r).abs() <= tol,
                _ => false,
            })?;
            Some((if dm.kind() == "radius" { "radius" } else { "diameter" }, vec![Ref { e: e.id?, k: "R".into() }]))
        }
        "angular" => {
            let on_line = |l: &cc_cad::entity::Entity, p: P2| -> bool {
                let Geo::Line(l) = &l.geo else { return false };
                let (ux, uy) = (l.x2 - l.x1, l.y2 - l.y1);
                let h = js::hypot(ux, uy);
                let len = if h == 0.0 || h.is_nan() { 1e-300 } else { h };
                (ux * (p.y - l.y1) - uy * (p.x - l.x1)).abs() / len <= tol * 10.0
            };
            let (cc, p1, p2) = (dm.c.unwrap_or(nan), dm.p1.unwrap_or(nan), dm.p2.unwrap_or(nan));
            let through = |a: P2, b: P2| ents.iter().find(|e| matches!(e.geo, Geo::Line(_)) && on_line(e, a) && on_line(e, b));
            let (l1, l2) = (through(cc, p1), through(cc, p2));
            if let (Some(l1), Some(l2)) = (l1, l2)
                && l1.id != l2.id
            {
                return Some(("angle", vec![Ref { e: l1.id?, k: "L".into() }, Ref { e: l2.id?, k: "L".into() }]));
            }
            let arc = ents.iter().find(|e| matches!(&e.geo, Geo::Arc(k) if js::hypot(k.cx - cc.x, k.cy - cc.y) <= tol))?;
            Some(("angle", vec![Ref { e: arc.id?, k: "R".into() }]))
        }
        _ => None,
    }
}

// ---- managing

pub async fn delconstraint(c: Ctx) -> R {
    if !model_only(&c) {
        return Ok(());
    }
    let ids = c.select(Some(c.t("cmd.sim.select")), false).await?;
    if ids.is_empty() {
        return Ok(());
    }
    let gone: Vec<String> = cons(&c.doc()).iter().filter(|k| k.refs.iter().any(|r| ids.has(r.e))).map(|k| k.id.clone()).collect();
    let n = paramsdoc::remove_constraints(&mut c.sh().doc.borrow_mut(), &gone, "DELCONSTRAINT").map_err(doc_err)?;
    c.flush_notes();
    c.set_selection(&[]);
    c.print(&if n > 0 {
        c.tv("cmd.delconstraint.removed", &[("n", n.to_string())])
    } else {
        c.t("cmd.delconstraint.those_objects_have_no")
    });
    Ok(())
}

/// `autoconstrain(c, { only: 'coincident' })` (only = true) or every kind.
async fn autoconstrain(c: &Ctx, only_coincident: bool) -> R {
    let (mut dist, mut ang) = c.st().memo.param.autoc.unwrap_or((0.05, 1.0));
    c.st().memo.param.autoc = Some((dist, ang));
    let set = loop {
        let own = if only_coincident { Vec::new() } else { vec![ro("S", "Settings")] };
        match c.select_with(Some(c.t("cmd.sim.select")), false, own).await? {
            Sel::Opt(k) if k == "S" => {
                let d = c
                    .distance(
                        c.t("cmd.delconstraint.specify_distance_tolerance"),
                        Opts { default: Some(Def::Num(dist)), ..Opts::default() },
                    )
                    .await?;
                if let Reply::Num(d) = d
                    && d >= 0.0
                {
                    dist = d;
                }
                let a = c
                    .number(
                        c.t("cmd.delconstraint.specify_angle_tolerance_degrees"),
                        Opts { default: Some(Def::Num(ang)), ..Opts::default() },
                    )
                    .await?;
                if let Reply::Num(a) = a
                    && (0.0..45.0).contains(&a)
                {
                    ang = a;
                }
                c.st().memo.param.autoc = Some((dist, ang));
                c.print(&c.tv("cmd.autoconstrain.tolerances", &[("dist", js::num_to_string(dist)), ("angle", js::num_to_string(ang))]));
            }
            Sel::Opt(_) => break crate::engine::IdSet::default(),
            Sel::Set(s) => break s,
        }
    };
    let ents: Vec<cc_cad::entity::Entity> = set.0.iter().copied().filter_map(|id| c.doc().get(id).cloned()).filter(solvable).collect();
    if ents.is_empty() {
        c.print(&c.t("cmd.delconstraint.nothing_there_takes_constraints"));
        return Ok(());
    }
    let has = |c: &Ctx, typ: &str, refs: &[Ref]| {
        cons(&c.doc())
            .iter()
            .any(|k| k.typ == typ && k.refs.len() == refs.len() && refs.iter().all(|r| k.refs.iter().any(|q| q.e == r.e && q.k == r.k)))
    };
    let t = tr(c);
    let mut n = 0usize;
    // coincident: ends within the distance tolerance
    let mut ends: Vec<(u64, String, f64, f64)> = Vec::new();
    for e in &ents {
        let ks: Vec<String> = match &e.geo {
            Geo::Line(_) => vec!["p1".into(), "p2".into()],
            Geo::Arc(_) => vec!["s".into(), "e".into()],
            Geo::Point(_) => vec!["p".into()],
            Geo::Poly(p) if !p.is_closed() => vec!["v0".into(), format!("v{}", p.pts.len().saturating_sub(1))],
            _ => Vec::new(),
        };
        for k in ks {
            let p = ref_point(e, &k).unwrap_or([f64::NAN; 2]);
            ends.push((e.id.unwrap_or(0), k, p[0], p[1]));
        }
    }
    let mut joined: Vec<usize> = Vec::new();
    for i in 0..ends.len() {
        for j in i.saturating_add(1)..ends.len() {
            let (Some(a), Some(b)) = (ends.get(i).cloned(), ends.get(j).cloned()) else { continue };
            if a.0 == b.0 || joined.contains(&j) || js::hypot(a.2 - b.2, a.3 - b.3) > dist {
                continue;
            }
            let refs = [Ref { e: b.0, k: b.1.clone() }, Ref { e: a.0, k: a.1.clone() }];
            if has(c, "coincident", &refs) {
                joined.push(j);
                continue;
            }
            let r = paramsdoc::add_constraint(
                &mut c.sh().doc.borrow_mut(),
                &t,
                "coincident",
                &refs,
                &AddOpts { pin_first: false, ..AddOpts::default() },
            )
            .map_err(doc_err)?;
            c.flush_notes();
            if r.ok {
                n = n.saturating_add(1);
                joined.push(j);
            }
        }
    }
    if !only_coincident {
        let tol = ang * std::f64::consts::PI / 180.0;
        for e in &ents {
            let id = e.id.unwrap_or(0);
            let segs: Vec<String> = match &e.geo {
                Geo::Line(_) => vec!["L".into()],
                Geo::Poly(_) => (0..seg_count(e)).map(|i| format!("s{i}")).filter(|k| ref_kind(e, k) == Some('L')).collect(),
                _ => Vec::new(),
            };
            for k in segs {
                let l = ref_line(e, &k);
                let a = (l[3] - l[1]).jatan2(l[2] - l[0]);
                let h = a.jsin().abs() <= tol.jsin();
                let v = a.jcos().abs() <= tol.jsin();
                let typ = if h {
                    "horizontal"
                } else if v {
                    "vertical"
                } else {
                    continue;
                };
                let r1 = [Ref { e: id, k: k.clone() }];
                if has(c, typ, &r1) || has(c, if typ == "horizontal" { "vertical" } else { "horizontal" }, &r1) {
                    continue;
                }
                let r = paramsdoc::add_constraint(
                    &mut c.sh().doc.borrow_mut(),
                    &t,
                    typ,
                    &r1,
                    &AddOpts { pin_first: false, ..AddOpts::default() },
                )
                .map_err(doc_err)?;
                c.flush_notes();
                if r.ok {
                    n = n.saturating_add(1);
                }
            }
        }
    }
    c.set_selection(&[]);
    if n > 0 {
        let st = status_text(&c.doc(), &t);
        c.print(&format!("{} {st}", c.tv("cmd.autoconstrain.applied", &[("n", n.to_string())])));
    } else {
        c.print(&c.tv("cmd.autoconstrain.none", &[("dist", js::num_to_string(dist)), ("angle", js::num_to_string(ang))]));
    }
    Ok(())
}

pub async fn autoconstrain_cmd(c: Ctx) -> R {
    if model_only(&c) {
        autoconstrain(&c, false).await?;
    }
    Ok(())
}

pub async fn parameters(c: Ctx) -> R {
    if c.sh().host.borrow_mut().parameters() {
        return Ok(());
    }
    list_params(&c);
    Ok(())
}

fn list_params(c: &Ctx) {
    let t = tr(c);
    let (rows, status) = paramsdoc::describe(&c.doc(), &t);
    if rows.is_empty() {
        c.print(&c.t("cmd.parameters.no_parameters_yet_dimensional"));
    }
    for r in &rows {
        let mut s = format!("  {} = {}", r.name, r.expr);
        if !r.driving {
            s.push_str("  → ");
            s.push_str(&r.value.map_or_else(|| "?".to_string(), |v| js::num_to_string(js::round(v * 1e6) / 1e6)));
        }
        let kind =
            if r.dim { c.tv("cmd.parameters.kind.dim", &[("type", r.type_label.to_lowercase())]) } else { c.t("cmd.parameters.kind.user") };
        s.push_str(&format!("   ({kind})"));
        if let Some(e) = &r.error {
            s.push_str("  ");
            s.push_str(&c.tv("cmd.parameters.error", &[("why", e.clone())]));
        }
        c.print(&s);
    }
    c.print(&status);
}

pub async fn parameters_cl(c: Ctx) -> R {
    let options = vec![ro("N", "New"), ro("E", "Edit"), ro("R", "Rename"), ro("D", "Delete"), ro("?", "?")];
    let k =
        c.keyword(c.t("cmd.mea.option"), Opts { options: options.clone(), default: Some(Def::Str("?".into())), ..Opts::default() }).await?;
    let key = match k {
        Reply::Opt(o) => o,
        Reply::Str(s) => s,
        _ => String::new(),
    };
    let s_of = |r: &Reply| js::trim(&crate::cmd::block::s_reply(r)).to_string();
    let t = tr(&c);
    match key.as_str() {
        "N" => {
            let name = s_of(&c.string(c.t("cmd.parameters.enter_name_new_parameter"), Opts::default()).await?);
            if name.is_empty() {
                return Ok(());
            }
            let mut ex = s_of(&c.string("Enter expression <0>".into(), Opts::default()).await?);
            if ex.is_empty() {
                ex = "0".into();
            }
            let r = paramsdoc::set_parameter(&mut c.sh().doc.borrow_mut(), &t, &name, &ex, None).map_err(doc_err)?;
            c.flush_notes();
            say(&c, &r, Some(format!("{name} = {ex}.")));
        }
        "E" => {
            let name = s_of(&c.string(c.t("cmd.parameters.enter_name_parameter_edit"), Opts::default()).await?);
            if name.is_empty() {
                return Ok(());
            }
            let con = cons(&c.doc()).iter().find(|x| is_dim(x) && x.name() == Some(name.as_str())).cloned();
            let cur = cc_gcons::gcons::params_of(&c.doc()).var(&name).cloned();
            let Some(cur) = cur else {
                c.print(&c.tv("cmd.parameters.none_called", &[("name", name)]));
                return Ok(());
            };
            let ex = s_of(&c.string(format!("Enter expression <{}>", cur.expr), Opts::default()).await?);
            if ex.is_empty() {
                return Ok(());
            }
            let r = match con {
                Some(k) => paramsdoc::set_dim_input(&mut c.sh().doc.borrow_mut(), &t, &k.id, &ex),
                None => paramsdoc::set_parameter(&mut c.sh().doc.borrow_mut(), &t, &name, &ex, None),
            }
            .map_err(doc_err)?;
            c.flush_notes();
            say(&c, &r, Some(format!("{name} = {ex}.")));
        }
        "R" => {
            let from = s_of(&c.string(c.t("cmd.parameters.enter_name_parameter_rename"), Opts::default()).await?);
            if from.is_empty() {
                return Ok(());
            }
            let to = s_of(&c.string(c.t("cmd.parameters.enter_new_name"), Opts::default()).await?);
            if to.is_empty() {
                return Ok(());
            }
            let cur = cc_gcons::gcons::params_of(&c.doc()).var(&from).cloned();
            let Some(cur) = cur else {
                c.print(&c.tv("cmd.parameters.none_called", &[("name", from)]));
                return Ok(());
            };
            let r = paramsdoc::set_parameter(&mut c.sh().doc.borrow_mut(), &t, &to, &cur.expr, Some(&from)).map_err(doc_err)?;
            c.flush_notes();
            say(&c, &r, Some(c.tv("params.is_now", &[("name", from), ("value", to)])));
        }
        "D" => {
            let name = s_of(&c.string(c.t("cmd.parameters.enter_name_parameter_delete"), Opts::default()).await?);
            if name.is_empty() {
                return Ok(());
            }
            let mut r = paramsdoc::remove_parameter(&mut c.sh().doc.borrow_mut(), &t, &name, false).map_err(doc_err)?;
            if !r.ok && r.uses {
                let yn = c
                    .keyword(
                        format!("{} {}", r.error, c.t("cmd.parameters.inline_ask")),
                        Opts { options: vec![ro("Y", "Yes"), ro("N", "No")], default: Some(Def::Str("No".into())), ..Opts::default() },
                    )
                    .await?;
                let yes = matches!(&yn, Reply::Opt(k) if k == "Y") || matches!(&yn, Reply::Str(s) if s == "Yes");
                if !yes {
                    return Ok(());
                }
                r = paramsdoc::remove_parameter(&mut c.sh().doc.borrow_mut(), &t, &name, true).map_err(doc_err)?;
            }
            c.flush_notes();
            say(&c, &r, Some(format!("{name} deleted.")));
        }
        _ => list_params(&c),
    }
    Ok(())
}

pub async fn constraintbar(c: Ctx) -> R {
    let mut v = c.st().memo.param.view.clone().unwrap_or_default();
    c.st().memo.param.view = Some(v.clone());
    let set = c.select_with(Some(c.t("cmd.sim.select")), false, vec![ro("S", "Showall"), ro("H", "Hideall")]).await?;
    match set {
        Sel::Opt(k) => {
            v.bars_all = k == "S";
            v.hidden.clear();
            c.print(&c.t(if k == "S" { "cmd.constraintbar.constraint_bars_shown" } else { "cmd.constraintbar.constraint_bars_hidden" }));
        }
        Sel::Set(s) if !s.is_empty() => {
            let k = c
                .keyword(
                    c.t("cmd.mea.option"),
                    Opts { options: vec![ro("S", "Show"), ro("H", "Hide")], default: Some(Def::Str("Show".into())), ..Opts::default() },
                )
                .await?;
            let show = !matches!(&k, Reply::Opt(o) if o == "H") && !matches!(&k, Reply::Str(o) if o == "Hide");
            for id in s.0.iter().copied() {
                if show {
                    v.hidden.retain(|x| *x != id);
                } else if !v.hidden.contains(&id) {
                    v.hidden.push(id);
                }
            }
            if show && !v.bars_all {
                v.bars_all = true;
                let all: Vec<u64> = c.doc().model().filter_map(|e| e.id).collect();
                for e in all {
                    if !s.has(e) && !v.hidden.contains(&e) {
                        v.hidden.push(e);
                    }
                }
            }
            c.print(
                &c.tv(if show { "cmd.constraintbar.shown_for" } else { "cmd.constraintbar.hidden_for" }, &[("n", s.len().to_string())]),
            );
        }
        Sel::Set(_) => {}
    }
    c.st().memo.param.view = Some(v);
    c.set_selection(&[]);
    Ok(())
}

pub async fn dcdisplay(c: Ctx) -> R {
    let mut v = c.st().memo.param.view.clone().unwrap_or_default();
    c.st().memo.param.view = Some(v.clone());
    let def = if v.dims { "Hide" } else { "Show" };
    let k = c
        .keyword(
            c.t("cmd.mea.option"),
            Opts { options: vec![ro("S", "Show"), ro("H", "Hide")], default: Some(Def::Str(def.into())), ..Opts::default() },
        )
        .await?;
    v.dims = matches!(&k, Reply::Opt(o) if o == "S") || matches!(&k, Reply::Str(o) if o == "Show");
    c.print(&c.t(if v.dims { "cmd.dcdisplay.shown" } else { "cmd.dcdisplay.hidden" }));
    c.st().memo.param.view = Some(v);
    Ok(())
}

/// SWBOUT (SWMACRO): the model space as a SolidWorks macro (swsketch.js `docToSwb`), handed to the host as a download
/// (`host.download`; a host that cannot save keeps nothing, as the JS keeps `engine.lastSwb`).
pub async fn swbout(c: Ctx) -> R {
    let base = c.doc().meta.get("name").filter(|v| js::truthy(Some(v))).map(js::to_js_string).unwrap_or_else(|| "Drawing1".into());
    let name = cc_gcons::swsketch::underscore_runs(&base);
    let date = cc_gcons::swsketch::iso_date(c.sh().host.borrow().now_ms().unwrap_or(0.0));
    let tr = cc_cad::i18n::catalog();
    let out = cc_gcons::swsketch::doc_to_swb(&tr, &c.doc(), &name, &date);
    let out = match out {
        Ok(o) => o,
        Err(e) => {
            c.print(&e.error);
            return Ok(());
        }
    };
    let file = format!("{name}.swb");
    if c.sh().host.borrow().can_save_blob() {
        c.sh().host.borrow_mut().save_blob(&file, &out.bytes, "text/plain");
    }
    c.print(&format!(
        "Wrote {file}: a macro that SolidWorks runs (Tools, Macro, Run) to build a part with one sketch on the Front Plane. {}",
        out.summary
    ));
    for n in &out.notes {
        c.print(&format!("  {n}"));
    }
    c.print(&c.t("cmd.swbout.not_yet_tested_solidworks"));
    Ok(())
}
