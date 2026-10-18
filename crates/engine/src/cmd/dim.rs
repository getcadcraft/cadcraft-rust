//! Dimensions (commands.js): DIMLINEAR DIMALIGNED DIMRADIUS DIMDIAMETER DIMANGULAR DIMCONTINUE DIMBASELINE, the
//! dimension variables typed as commands (DIMSCALE DIMTXT DIMASZ DIMDEC DIMGAP DIMEXE DIMEXO) and DIMLUNIT.

use std::rc::Rc;

use cc_cad::entity::{Dim, Entity, Geo, num};
use cc_cad::js;
use cc_cad_geom::dims::{angular_from_lines, format_measure};
use cc_cad_geom::geom::{PI, angle_of, dist, norm_angle, polar, prim_point, prims};
use cc_cad_geom::jsmath::JsMath;
use cc_cad_geom::units::lunit_code;
use cc_geom::P2;
use serde_json::Value;

use crate::cmd::draw::ent;
use crate::cmd::modify::undo_inside;
use crate::engine::{Ctx, Opts, R, Reply};
use crate::input::Def;
use crate::prompt::CmdError;
use crate::ucs::{active_ucs, ang_of, to_ucs};

/// `dimEnt(c, e)`: a dimension on the current layer with the current dimension style.
fn dim_ent(c: &Ctx, d: Dim) -> Entity {
    let st = c.doc().current.dim.clone();
    let d = Dim { text: Some(d.text.unwrap_or_default()), st: Some(d.st.unwrap_or(st)), ..d };
    ent(c, Geo::Dim(d))
}

type Make = Rc<dyn Fn(&Ctx, P2, Option<f64>, &str) -> Option<Entity>>;

/// `placeDim`: where the dimension line goes (previewing); T(ext), and for linear H / V / R(otated).
async fn place_dim(c: &Ctx, make: Make, linear: bool) -> R<Option<Entity>> {
    let mut text = String::new();
    let mut fixed: Option<f64> = None;
    loop {
        {
            let (m, t, cc) = (Rc::clone(&make), text.clone(), c.clone());
            c.preview(Some(Rc::new(move |_, cur| m(&cc, cur, fixed, &t).into_iter().collect())));
        }
        let mut options = vec![c.opt("T", "Text")];
        if linear {
            options.extend([c.opt("H", "Horizontal"), c.opt("V", "Vertical"), c.opt("R", "Rotated")]);
        }
        let p = c.point(c.t("cmd.explode.specify_dimension_line_location"), Opts { options, ..Opts::default() }).await?;
        let u = ang_of(active_ucs(&c.doc()));
        let at = match p {
            Reply::Opt(k) => {
                match k.as_str() {
                    "T" => {
                        let r = c.string("Enter dimension text (<> is the measured value, Enter keeps it)".into(), Opts::default()).await?;
                        let s = if let Reply::Str(s) = r { s } else { String::new() };
                        text = if js::trim(&s).is_empty() { String::new() } else { s };
                    }
                    "H" => fixed = Some(norm_angle(u)),
                    "V" => fixed = Some(norm_angle(u + PI / 2.0)),
                    "R" => {
                        let a = c
                            .angle(
                                c.t("cmd.explode.specify_angle_dimension_line"),
                                Opts { default: Some(Def::Num(u)), ucs: true, ..Opts::default() },
                            )
                            .await?;
                        fixed = Some(if let Reply::Num(a) = a { a } else { 0.0 });
                    }
                    _ => {}
                }
                continue;
            }
            Reply::Point(q, _) => q,
            _ => return Ok(None),
        };
        let Some(d) = make(c, at, fixed, &text) else {
            c.print(&c.t("cmd.explode.pick_point_inside_angle"));
            continue;
        };
        c.preview(None);
        let made = c.change("DIM", |doc| doc.add(d))?;
        c.st().memo.last_dim = made.id;
        c.print(&format!("Dimension text = {}", format_measure(&made, None)));
        return Ok(Some(made));
    }
}

/// `linearRot`: horizontal or vertical by where the dimension line goes, in the UCS.
fn linear_rot(p1: P2, p2: P2, cur: P2, u: Option<crate::ucs::Ucs>) -> f64 {
    let (a, b, q) = (to_ucs(u, p1), to_ucs(u, p2), to_ucs(u, cur));
    let base = ang_of(u);
    let (min_x, max_x, min_y, max_y) = (js::min(a.x, b.x), js::max(a.x, b.x), js::min(a.y, b.y), js::max(a.y, b.y));
    let out_x = if q.x < min_x {
        min_x - q.x
    } else if q.x > max_x {
        q.x - max_x
    } else {
        0.0
    };
    let out_y = if q.y < min_y {
        min_y - q.y
    } else if q.y > max_y {
        q.y - max_y
    } else {
        0.0
    };
    norm_angle(base + if out_y >= out_x { 0.0 } else { PI / 2.0 })
}

/// `twoOrigins`: two picked points, or (S / Enter) an object's ends.
async fn two_origins(c: &Ctx) -> R<(P2, P2)> {
    let a = c
        .point(
            c.t("cmd.explode.specify_first_extension_line"),
            Opts { none: true, options: vec![c.opt("S", &c.t("cmd.area.object"))], ..Opts::default() },
        )
        .await?;
    if let Reply::Point(a, _) = a {
        let b = c.must_point(c.t("cmd.explode.specify_second_extension_line"), Opts { base: Some(a), ..Opts::default() }).await?;
        return Ok((a, b));
    }
    loop {
        let r = c.entity(c.t("cmd.explode.select_object_dimension"), Opts::default()).await?;
        let Reply::Pick { id, at, .. } = r else { return Err(CmdError::Cancelled) };
        let Some(e) = c.doc().get(id).cloned() else { continue };
        match &e.geo {
            Geo::Line(l) => return Ok((P2::new(l.x1, l.y1), P2::new(l.x2, l.y2))),
            Geo::Arc(a) => {
                let cen = P2::new(a.cx, a.cy);
                return Ok((polar(cen, a.a0, a.r), polar(cen, a.a1, a.r)));
            }
            Geo::Circle(ci) => return Ok((P2::new(ci.cx - ci.r, ci.cy), P2::new(ci.cx + ci.r, ci.cy))),
            Geo::Poly(_) => {
                let ps = prims(&e, None);
                let mut best = ps.first();
                let mut bd = f64::INFINITY;
                for q in &ps {
                    let d = dist(at, prim_point(q, 0.5));
                    if d < bd {
                        bd = d;
                        best = Some(q);
                    }
                }
                let Some(best) = best else { return Err(CmdError::Msg("Cannot read properties of undefined (reading 'k')".into())) };
                return Ok((prim_point(best, 0.0), prim_point(best, 1.0)));
            }
            _ => c.print(&c.t("cmd.explode.pick_line_arc_circle")),
        }
    }
}

fn dim(kind: &str) -> Dim {
    Dim { kind: Some(kind.into()), ..Dim::default() }
}

pub async fn dimlinear(c: Ctx) -> R {
    let (p1, p2) = two_origins(&c).await?;
    let make: Make = Rc::new(move |c, cur, rot, text| {
        let r = rot.unwrap_or_else(|| linear_rot(p1, p2, cur, active_ucs(&c.doc())));
        Some(dim_ent(c, Dim { p1: Some(p1), p2: Some(p2), loc: Some(cur), rot: Some(r), text: Some(text.into()), ..dim("linear") }))
    });
    place_dim(&c, make, true).await?;
    Ok(())
}

pub async fn dimaligned(c: Ctx) -> R {
    let (p1, p2) = two_origins(&c).await?;
    if dist(p1, p2) < 1e-12 {
        c.print(&c.t("cmd.dimaligned.two_points_same"));
        return Ok(());
    }
    let make: Make = Rc::new(move |c, cur, _, text| {
        Some(dim_ent(c, Dim { p1: Some(p1), p2: Some(p2), loc: Some(cur), text: Some(text.into()), ..dim("aligned") }))
    });
    place_dim(&c, make, false).await?;
    Ok(())
}

pub async fn radial(c: Ctx, kind: &'static str) -> R {
    loop {
        let r = c.entity(c.t("cmd.mea.arc"), Opts::default()).await?;
        let Reply::Pick { id, at, .. } = r else { return Ok(()) };
        let Some(e) = c.doc().get(id).cloned() else { continue };
        let (cx, cy, rr) = match &e.geo {
            Geo::Circle(ci) => (ci.cx, ci.cy, ci.r),
            Geo::Arc(a) => (a.cx, a.cy, a.r),
            _ => {
                c.print(&c.t("cmd.mea.arc.only"));
                continue;
            }
        };
        let cen = P2::new(cx, cy);
        let p1 = polar(cen, angle_of(cen, at), rr);
        let make: Make = Rc::new(move |c, cur, _, text| {
            let q = if dist(cen, cur) > 1e-12 { polar(cen, angle_of(cen, cur), rr) } else { p1 };
            Some(dim_ent(c, Dim { c: Some(cen), p1: Some(q), loc: Some(cur), text: Some(text.into()), ..dim(kind) }))
        });
        place_dim(&c, make, false).await?;
        return Ok(());
    }
}

type Geo3 = Rc<dyn Fn(P2) -> Option<(P2, P2, P2, P2)>>;

pub async fn dimangular(c: Ctx) -> R {
    let pick = c.entity(c.t("cmd.dimangular.select_arc_circle_line"), Opts { none: true, ..Opts::default() }).await?;
    let g: Geo3 = match pick {
        Reply::Pick { id, at, .. } => {
            let Some(e) = c.doc().get(id).cloned() else { return Ok(()) };
            match &e.geo {
                Geo::Arc(a) => {
                    let cen = P2::new(a.cx, a.cy);
                    let (p1, p2) = (polar(cen, a.a0, a.r), polar(cen, a.a1, a.r));
                    let mid = polar(cen, a.a0 + norm_angle(a.a1 - a.a0) / 2.0, a.r);
                    Rc::new(move |cur| Some((cen, p1, p2, if dist(cen, cur) > 1e-12 { cur } else { mid })))
                }
                Geo::Circle(ci) => {
                    let cen = P2::new(ci.cx, ci.cy);
                    let p1 = polar(cen, angle_of(cen, at), ci.r);
                    let b = c.must_point(c.t("cmd.mea.end2"), Opts { base: Some(cen), ..Opts::default() }).await?;
                    Rc::new(move |cur| Some((cen, p1, b, cur)))
                }
                Geo::Line(l1) => {
                    let l1 = l1.clone();
                    c.set_selection(&[id]);
                    let l2 = loop {
                        let q = c.entity(c.t("cmd.mea.line2"), Opts::default()).await?;
                        let Reply::Pick { id: fid, .. } = q else { return Ok(()) };
                        let f = c.doc().get(fid).cloned();
                        if let Some(Entity { geo: Geo::Line(l2), .. }) = f
                            && fid != id
                        {
                            break l2;
                        }
                        c.print(&c.t("cmd.dimangular.pick_second_different_line"));
                    };
                    c.set_selection(&[]);
                    let cross = (l1.x2 - l1.x1) * (l2.y2 - l2.y1) - (l1.y2 - l1.y1) * (l2.x2 - l2.x1);
                    if cross.abs() < 1e-12 * js::hypot(l1.x2 - l1.x1, l1.y2 - l1.y1) * js::hypot(l2.x2 - l2.x1, l2.y2 - l2.y1) {
                        c.print(&c.t("cmd.chamfer.err.parallel"));
                        return Ok(());
                    }
                    let (a, b) = ([l1.x1, l1.y1, l1.x2, l1.y2], [l2.x1, l2.y1, l2.x2, l2.y2]);
                    Rc::new(move |cur| angular_from_lines(a, b, cur).map(|(cc, p1, p2)| (cc, p1, p2, cur)))
                }
                _ => {
                    c.print(&c.t("cmd.dimangular.pick_arc_circle_line"));
                    return Ok(());
                }
            }
        }
        Reply::Null => {
            let v = c.must_point(c.t("cmd.mea.vertex"), Opts::default()).await?;
            let a = c.must_point(c.t("cmd.mea.end1"), Opts { base: Some(v), ..Opts::default() }).await?;
            let b = c.must_point(c.t("cmd.mea.end2"), Opts { base: Some(v), ..Opts::default() }).await?;
            Rc::new(move |cur| Some((v, a, b, cur)))
        }
        _ => return Ok(()),
    };
    let make: Make = Rc::new(move |c, cur, _, text| {
        let (cc, p1, p2, loc) = g(cur)?;
        (dist(cc, loc) > 1e-12)
            .then(|| dim_ent(c, Dim { c: Some(cc), p1: Some(p1), p2: Some(p2), loc: Some(loc), text: Some(text.into()), ..dim("angular") }))
    });
    place_dim(&c, make, false).await?;
    Ok(())
}

fn is_chainable(e: &Entity) -> bool {
    matches!(&e.geo, Geo::Dim(d) if d.kind() == "linear" || d.kind() == "aligned")
}

/// `chainFrom`: the last dimension made, else pick a linear or aligned one.
async fn chain_from(c: &Ctx) -> R<Entity> {
    let last = c.st().memo.last_dim;
    if let Some(d) = last.and_then(|id| c.doc().get(id).cloned()).filter(is_chainable) {
        return Ok(d);
    }
    loop {
        let r = c.entity(c.t("cmd.dimangular.select_linear_aligned_dimension"), Opts::default()).await?;
        let Reply::Pick { id, .. } = r else { return Err(CmdError::Cancelled) };
        if let Some(d) = c.doc().get(id).cloned().filter(is_chainable) {
            return Ok(d);
        }
        c.print(&c.t("cmd.dimangular.pick_linear_aligned_dimension"));
    }
}

fn dim_of(e: &Entity) -> Dim {
    if let Geo::Dim(d) = &e.geo { d.clone() } else { Dim::default() }
}
fn nan2() -> P2 {
    P2::new(f64::NAN, f64::NAN)
}

pub async fn chain(c: Ctx, baseline: bool) -> R {
    let mut d = chain_from(&c).await?;
    let first = d.clone();
    let fd = dim_of(&first);
    let mut n = 0usize;
    loop {
        let dd = dim_of(&d);
        let from = if baseline { fd.p1.unwrap_or_else(nan2) } else { dd.p2.unwrap_or_else(nan2) };
        let st = dd.st.clone().unwrap_or_else(|| c.doc().current.dim.clone());
        let rot = if dd.kind() == "aligned" {
            angle_of(dd.p1.unwrap_or_else(nan2), dd.p2.unwrap_or_else(nan2))
        } else {
            let r = js::or(dd.rot, 0.0);
            if r.is_nan() { 0.0 } else { r }
        };
        let nrm = P2::new(-rot.jsin(), rot.jcos());
        let (floc, fp1) = (fd.loc.unwrap_or_else(nan2), fd.p1.unwrap_or_else(nan2));
        let sg = js::sign((floc.x - fp1.x) * nrm.x + (floc.y - fp1.y) * nrm.y);
        let side = if sg == 0.0 || sg.is_nan() { 1.0 } else { sg };
        let dli = match st.get("dli") {
            Some(Value::Null) | None => 3.75,
            Some(v) => js::to_number(v),
        };
        let scale = st.get("scale").map_or(0.0, js::to_number);
        let scale = if scale == 0.0 || scale.is_nan() { 1.0 } else { scale };
        let step = dli * scale * side;
        let dloc = dd.loc.unwrap_or_else(nan2);
        let loc = if baseline { P2::new(dloc.x + nrm.x * step, dloc.y + nrm.y * step) } else { dloc };
        let st2 = st.clone();
        let make = move |c: &Ctx, cur: P2| {
            dim_ent(
                c,
                Dim { p1: Some(from), p2: Some(cur), loc: Some(loc), rot: Some(rot), st: Some(st2.clone()), text: None, ..dim("linear") },
            )
        };
        {
            let (m, cc) = (make.clone(), c.clone());
            c.preview(Some(Rc::new(move |_, cur| vec![m(&cc, cur)])));
        }
        let p = c
            .point(
                c.t("cmd.explode.specify_second_extension_line"),
                Opts { none: true, base: Some(from), options: vec![c.opt("U", "Undo")], ..Opts::default() },
            )
            .await?;
        match p {
            Reply::Null => return Ok(()),
            Reply::Opt(k) if k == "U" => {
                if n > 0 {
                    undo_inside(&c);
                    n = n.saturating_sub(1);
                    let last = c.st().memo.last_dim;
                    d = last.and_then(|id| c.doc().get(id).cloned()).unwrap_or_else(|| first.clone());
                } else {
                    c.print(&c.t("cmd.extend.nothing_undo_command"));
                }
                continue;
            }
            Reply::Point(q, _) => {
                let e = make(&c, q);
                let made = c.change("DIM", |doc| doc.add(e))?;
                c.st().memo.last_dim = made.id;
                c.print(&format!("Dimension text = {}", format_measure(&made, None)));
                d = made;
                n = n.saturating_add(1);
            }
            _ => return Ok(()),
        }
    }
}

/// The dimension variables typed as commands: (command, style key).
pub const DIMVARS: [(&str, &str); 7] = [
    ("DIMSCALE", "scale"),
    ("DIMTXT", "txt"),
    ("DIMASZ", "asz"),
    ("DIMDEC", "dec"),
    ("DIMGAP", "gap"),
    ("DIMEXE", "exe"),
    ("DIMEXO", "exo"),
];

fn js_str(v: Option<&Value>) -> Option<String> {
    match v {
        None | Some(Value::Null) => None,
        Some(v) => Some(js::to_js_string(v)),
    }
}

pub async fn dimvar(c: Ctx, name: &'static str) -> R {
    let Some((_, key)) = DIMVARS.iter().find(|(n, _)| *n == name) else { return Ok(()) };
    let cur = c.doc().current.dim.get(*key).cloned();
    let default = match &cur {
        Some(Value::Number(n)) => n.as_f64().map(Def::Num),
        _ => None,
    };
    let v = c.number(c.tv("cmd.dimvar.enter", &[("name", name.into())]), Opts { default, ..Opts::default() }).await?;
    let stays = |c: &Ctx| {
        let mut vars = vec![("name", name.to_string())];
        if let Some(s) = js_str(cur.as_ref()) {
            vars.push(("value", s));
        }
        c.print(&c.tv("cmd.dimvar.stays", &vars));
    };
    let Reply::Num(v) = v else {
        stays(&c);
        return Ok(());
    };
    if !(v >= 0.0) || (*key == "scale" && v == 0.0) {
        stays(&c);
        return Ok(());
    }
    let nv = if *key == "dec" { js::min(8.0, js::round(v)) } else { v };
    c.sh().doc.borrow_mut().current.dim.insert((*key).into(), num(nv));
    c.print(&c.tv("cmd.dimvar.set", &[("name", name.into()), ("value", js::num_to_string(nv))]));
    Ok(())
}

const LUNIT_NAMES: [&str; 5] =
    ["opt.lunit.scientific", "opt.lunit.decimal", "opt.lunit.engineering", "opt.lunit.architectural", "opt.lunit.fractional"];

pub async fn dimlunit(c: Ctx) -> R {
    let cur = c.doc().current.dim.get("lunit").map_or(0.0, js::to_number);
    let cur_v = if cur == 0.0 || cur.is_nan() { None } else { c.doc().current.dim.get("lunit").cloned() };
    let default = match &cur_v {
        Some(v) => Def::Num(js::to_number(v)),
        None => Def::Num(2.0),
    };
    let v = c.number(c.t("cmd.dimlunit.ask"), Opts { default: Some(default), ..Opts::default() }).await?;
    let known = |v: f64| [1.0, 2.0, 3.0, 4.0, 5.0].contains(&v);
    let ok = matches!(v, Reply::Num(x) if known(x) || x == 6.0);
    let Reply::Num(v) = v else { return Ok(()) };
    if !ok {
        let s = cur_v.map_or_else(|| "2".to_string(), |x| js::to_js_string(&x));
        c.print(&c.tv("cmd.dimlunit.stays", &[("v", s)]));
        return Ok(());
    }
    let code = lunit_code(v);
    c.sh().doc.borrow_mut().current.dim.insert("lunit".into(), num(f64::from(code)));
    let name_key = usize::try_from(code.saturating_sub(1)).ok().and_then(|i| LUNIT_NAMES.get(i)).copied().unwrap_or("opt.lunit.decimal");
    c.print(&c.tv("cmd.dimlunit.set", &[("v", code.to_string()), ("format", c.t(name_key))]));
    Ok(())
}
