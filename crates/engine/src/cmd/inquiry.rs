//! Inquiry (commands.js): DIST AREA MEASUREGEOM ID LIST; and the selection commands SELECTALL DESELECT SELECT
//! DUPLICATE; PURGE.

use std::rc::Rc;

use cc_cad::entity::{Entity, Geo, Poly, Vertex};
use cc_cad::js;
use cc_cad_geom::dims::format_measure;
use cc_cad_geom::edit::{entity_area, polygon_measure};
use cc_cad_geom::geom::{Prim, TAU, angle_of, deg, dist, intersect, m_translate, norm_angle};
use cc_cad_geom::jsmath::JsMath;
use cc_cad_geom::pick::{entity_bounds, valid_bounds};
use cc_cad_geom::tx::tx_entity;
use cc_cad_geom::units::{format_area, format_length, units_of};
use cc_geom::P2;
use serde_json::Value;

use crate::cmd::arrange::fmt_len;
use crate::cmd::draw::seg;
use crate::engine::{Ctx, Opts, R, Reply};
use crate::input::{Def, fmt};
use crate::ucs::{active_ucs, to_ucs};

fn fmt_area(c: &Ctx, a: f64) -> String {
    let (code, prec) = units_of(Some(&c.doc().meta));
    let lang = &c.sh().lang;
    let look = |k: &str| lang.has(k).then(|| lang.s(k));
    format_area(a, f64::from(code), Some(&Value::from(prec)), &look)
}

fn dist_line(c: &Ctx, a: P2, b: P2) -> String {
    c.tv(
        "cmd.mea.dist",
        &[
            ("d", fmt_len(c, dist(a, b))),
            ("ang", fmt(deg(norm_angle(angle_of(a, b))))),
            ("dx", fmt_len(c, b.x - a.x)),
            ("dy", fmt_len(c, b.y - a.y)),
        ],
    )
}

pub async fn dist_cmd(c: Ctx) -> R {
    let a = c.must_point(c.t("cmd.mea.p1"), Opts::default()).await?;
    c.preview(Some(Rc::new(move |_, cur| vec![seg(a, cur)])));
    let b = c.must_point(c.t("cmd.mea.p2"), Opts { base: Some(a), ..Opts::default() }).await?;
    c.print(&dist_line(&c, a, b));
    Ok(())
}

fn closed_poly(pts: &[P2]) -> Entity {
    Entity::new(Geo::Poly(Poly { pts: pts.iter().map(|p| Vertex::new(p.x, p.y, 0.0)).collect(), closed: Some(true) }))
}

/// `areaByPoints`: area and perimeter of points picked one by one → the area, or None.
async fn area_by_points(c: &Ctx, first: P2) -> R<Option<f64>> {
    let mut pts = vec![first];
    loop {
        {
            let snap = pts.clone();
            c.preview(Some(Rc::new(move |_, cur| {
                let mut v = snap.clone();
                v.push(cur);
                vec![closed_poly(&v)]
            })));
        }
        let options = if pts.len() > 1 { vec![c.opt("U", &c.t("opt.undo"))] } else { Vec::new() };
        // the first point is the answer itself (its z on the 3D tab); the rest are copies
        let p = c
            .point(
                c.t("cmd.area.next"),
                Opts { base: pts.last().copied(), none: true, options, flat_base: pts.len() > 1, ..Opts::default() },
            )
            .await?;
        match p {
            Reply::Opt(k) if k == "U" => {
                pts.pop();
            }
            Reply::Point(p, _) => {
                if pts.last().is_none_or(|l| dist(p, *l) > 1e-12) {
                    pts.push(p);
                }
            }
            _ => break,
        }
    }
    c.preview(None);
    if pts.len() < 3 {
        c.print(&c.t("cmd.area.few"));
        return Ok(None);
    }
    let (area, per) = polygon_measure(&pts);
    c.print(&c.tv("cmd.area.result", &[("area", fmt_area(c, area.abs())), ("perimeter", fmt_len(c, per))]));
    Ok(Some(area.abs()))
}

/// `areaOfObject`: one object's area, printed.
fn area_of_object(c: &Ctx, e: Option<&Entity>) -> Option<f64> {
    let Some((area, per, kind)) = e.and_then(entity_area) else {
        c.print(&c.t("cmd.area.noarea"));
        return None;
    };
    let key = match kind {
        "circle" => "cmd.area.result.circle",
        "open" => "cmd.area.result.open",
        _ => "cmd.area.result",
    };
    c.print(&c.tv(key, &[("area", fmt_area(c, area)), ("perimeter", fmt_len(c, per))]));
    Some(area)
}

pub async fn area(c: Ctx) -> R {
    let mut mode: Option<&'static str> = None;
    let mut total = 0.0;
    let tally = |c: &Ctx, a: Option<f64>, mode: Option<&str>, total: &mut f64| {
        let (Some(a), Some(m)) = (a, mode) else { return };
        *total += if m == "add" { a } else { -a };
        c.print(&c.tv("cmd.area.total", &[("area", fmt_area(c, *total))]));
    };
    let lead = |c: &Ctx, mode: Option<&str>| match mode {
        Some(m) => format!("({}) ", c.t(if m == "add" { "cmd.area.mode.add" } else { "cmd.area.mode.sub" })),
        None => String::new(),
    };
    loop {
        let mut options = vec![c.opt("O", &c.t("opt.object"))];
        if mode != Some("add") {
            options.push(c.opt("A", &c.t("opt.addarea")));
        }
        if mode != Some("sub") {
            options.push(c.opt("S", &c.t("opt.subarea")));
        }
        let prompt = format!("{}{}", lead(&c, mode), c.t("cmd.area.first"));
        let default = if mode.is_some() { None } else { Some(Def::Str("O".into())) };
        let p = c.point(prompt, Opts { options, none: true, default, ..Opts::default() }).await?;
        match p {
            Reply::Null => return Ok(()),
            Reply::Opt(k) if k == "A" || k == "S" => {
                mode = Some(if k == "A" { "add" } else { "sub" });
                continue;
            }
            Reply::Opt(k) if k == "O" => {}
            Reply::Str(s) if s == "O" => {}
            Reply::Point(p, _) => {
                let m = area_by_points(&c, p).await?;
                tally(&c, m, mode, &mut total);
                if mode.is_none() {
                    return Ok(());
                }
                continue;
            }
            _ => return Ok(()),
        }
        loop {
            let prompt = format!("{}{}", lead(&c, mode), c.t("cmd.area.object"));
            let pick = c.entity(prompt, Opts { none: true, ..Opts::default() }).await?;
            let Reply::Pick { id, .. } = pick else { break };
            c.set_selection(&[id]);
            let e = c.doc().get(id).cloned();
            let a = area_of_object(&c, e.as_ref());
            tally(&c, a, mode, &mut total);
            if mode.is_none() {
                return Ok(());
            }
        }
        c.set_selection(&[]);
        if mode.is_none() {
            return Ok(());
        }
    }
}

pub async fn measuregeom(c: Ctx) -> R {
    let mut first = true;
    loop {
        let mut options = vec![
            c.opt("D", &c.t("opt.distance")),
            c.opt("R", &c.t("opt.radius")),
            c.opt("A", &c.t("opt.angle")),
            c.opt("AR", &c.t("opt.area")),
        ];
        if !first {
            options.push(c.opt("X", &c.t("opt.exit")));
        }
        let def = if first { "D" } else { "X" };
        first = false;
        let k = c.keyword(c.t("cmd.mea.option"), Opts { options, default: Some(Def::Str(def.into())), ..Opts::default() }).await?;
        let o = match &k {
            Reply::Opt(s) | Reply::Str(s) => s.clone(),
            _ => String::new(),
        };
        match o.as_str() {
            "" | "X" => return Ok(()),
            "D" => {
                let a = c.must_point(c.t("cmd.mea.p1"), Opts::default()).await?;
                c.preview(Some(Rc::new(move |_, cur| vec![seg(a, cur)])));
                let b = c.must_point(c.t("cmd.mea.p2"), Opts { base: Some(a), ..Opts::default() }).await?;
                c.preview(None);
                c.print(&dist_line(&c, a, b));
            }
            "R" => loop {
                let r = c.entity(c.t("cmd.mea.arc"), Opts::default()).await?;
                let Reply::Pick { id, .. } = r else { return Ok(()) };
                let rr = match c.doc().get(id).map(|e| &e.geo) {
                    Some(Geo::Arc(a)) => Some(a.r),
                    Some(Geo::Circle(ci)) => Some(ci.r),
                    _ => None,
                };
                let Some(rr) = rr else {
                    c.print(&c.t("cmd.mea.arc.only"));
                    continue;
                };
                c.print(&c.tv("cmd.mea.radius", &[("r", fmt_len(&c, rr)), ("d", fmt_len(&c, 2.0 * rr))]));
                break;
            },
            "A" => {
                let pick = c.entity(c.t("cmd.mea.angle.pick"), Opts { none: true, ..Opts::default() }).await?;
                let ang = match pick {
                    Reply::Pick { id, .. } => {
                        let e = c.doc().get(id).cloned();
                        match e.as_ref().map(|e| &e.geo) {
                            Some(Geo::Arc(a)) => js::or(Some(norm_angle(a.a1 - a.a0)), TAU),
                            Some(Geo::Line(l1)) => {
                                let l1 = l1.clone();
                                let q = c.entity(c.t("cmd.mea.line2"), Opts::default()).await?;
                                let Reply::Pick { id: fid, .. } = q else { return Ok(()) };
                                let f = c.doc().get(fid).cloned();
                                let Some(Entity { geo: Geo::Line(l2), .. }) = f else {
                                    c.print(&c.t("cmd.mea.line2.only"));
                                    continue;
                                };
                                let x = intersect(&Prim::l(l1.x1, l1.y1, l1.x2, l1.y2), &Prim::l(l2.x1, l2.y1, l2.x2, l2.y2), true, true);
                                let Some(x) = x.first().copied() else {
                                    c.print(&c.t("cmd.chamfer.err.parallel"));
                                    continue;
                                };
                                let far = |a: P2, b: P2| if dist(x, a) > dist(x, b) { a } else { b };
                                let fe = far(P2::new(l1.x1, l1.y1), P2::new(l1.x2, l1.y2));
                                let ff = far(P2::new(l2.x1, l2.y1), P2::new(l2.x2, l2.y2));
                                let mut a = norm_angle(angle_of(x, ff) - angle_of(x, fe)).abs();
                                if a > std::f64::consts::PI {
                                    a = TAU - a;
                                }
                                a
                            }
                            _ => {
                                c.print(&c.t("cmd.mea.angle.only"));
                                continue;
                            }
                        }
                    }
                    _ => {
                        let v = c.must_point(c.t("cmd.mea.vertex"), Opts::default()).await?;
                        let a = c.must_point(c.t("cmd.mea.end1"), Opts { base: Some(v), ..Opts::default() }).await?;
                        let b = c.must_point(c.t("cmd.mea.end2"), Opts { base: Some(v), ..Opts::default() }).await?;
                        let mut g = norm_angle(angle_of(v, b) - angle_of(v, a)).abs();
                        if g > std::f64::consts::PI {
                            g = TAU - g;
                        }
                        g
                    }
                };
                c.print(&c.tv("cmd.mea.angle", &[("a", fmt(deg(ang)))]));
            }
            "AR" => {
                let p = c
                    .point(
                        c.t("cmd.area.first"),
                        Opts { options: vec![c.opt("O", &c.t("opt.object"))], default: Some(Def::Str("O".into())), ..Opts::default() },
                    )
                    .await?;
                match p {
                    Reply::Point(p, _) => {
                        area_by_points(&c, p).await?;
                    }
                    Reply::Opt(_) | Reply::Str(_) => {
                        let r = c.entity(c.t("cmd.area.object"), Opts::default()).await?;
                        let Reply::Pick { id, .. } = r else { return Ok(()) };
                        let e = c.doc().get(id).cloned();
                        area_of_object(&c, e.as_ref());
                    }
                    _ => return Ok(()),
                }
            }
            _ => {}
        }
    }
}

pub async fn id_cmd(c: Ctx) -> R {
    let p = c.must_point(c.t("cmd.id.specify_point"), Opts::default()).await?;
    let q = to_ucs(active_ucs(&c.doc()), p);
    c.print(&c.tv("cmd.id.result", &[("x", fmt_len(&c, q.x)), ("y", fmt_len(&c, q.y))]));
    Ok(())
}

/// doc.js `typeName(e)`.
pub fn type_name(c: &Ctx, e: &Entity) -> String {
    cc_cad::type_name(e, &|k| c.t(k))
}

/// One attribute row (attrs.js `attrRows`).
pub struct AttrRow {
    pub tag: String,
    pub value: String,
    pub invisible: bool,
    pub constant: bool,
}

fn attr_str(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(v) => js::to_js_string(v),
    }
}

/// attrs.js `attrRows(e, blk)`: every attribute of a reference, in definition order.
pub fn attr_rows(e: &Entity, blk: Option<&cc_cad::doc::Block>) -> Vec<AttrRow> {
    let Geo::Insert(ins) = &e.geo else { return Vec::new() };
    let empty = cc_cad::entity::Obj::new();
    let vals = ins.attrs.as_ref().unwrap_or(&empty);
    let loc_inv = |tag: &str| -> Option<bool> { ins.attloc.as_ref()?.get(tag)?.get("invisible").map(|v| js::truthy(Some(v))) };
    let mut rows = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for d in blk.and_then(|b| b.attdefs.as_ref()).map_or(&[][..], Vec::as_slice) {
        let tag = attr_str(d.get("tag"));
        seen.push(tag.clone());
        let constant = js::truthy(d.get("constant"));
        if !constant && !vals.contains_key(&tag) {
            continue;
        }
        let value = if constant { attr_str(d.get("def")) } else { attr_str(vals.get(&tag)) };
        let invisible = loc_inv(&tag).unwrap_or_else(|| js::truthy(d.get("invisible")));
        rows.push(AttrRow { tag, value, invisible, constant });
    }
    for (tag, v) in vals {
        if !seen.contains(tag) {
            rows.push(AttrRow { tag: tag.clone(), value: attr_str(Some(v)), invisible: loc_inv(tag).unwrap_or(false), constant: false });
        }
    }
    rows
}

/// commands.js `describe(e, units)`.
pub fn describe(c: &Ctx, e: &Entity) -> String {
    let (code, prec) = units_of(Some(&c.doc().meta));
    let pv = Value::from(prec);
    let l = |n: f64| format_length(n, f64::from(code), Some(&pv));
    let pp = |x: f64, y: f64| format!("{}, {}", l(x), l(y));
    match &e.geo {
        Geo::Line(ln) => c.tv(
            "cmd.desc.line",
            &[("from", pp(ln.x1, ln.y1)), ("to", pp(ln.x2, ln.y2)), ("length", l(js::hypot(ln.x2 - ln.x1, ln.y2 - ln.y1)))],
        ),
        Geo::Circle(ci) => c.tv("cmd.desc.circle", &[("center", pp(ci.cx, ci.cy)), ("r", l(ci.r))]),
        Geo::Arc(a) => c.tv("cmd.desc.arc", &[("center", pp(a.cx, a.cy)), ("r", l(a.r)), ("a0", fmt(deg(a.a0))), ("a1", fmt(deg(a.a1)))]),
        Geo::Poly(p) => c.tv(if p.is_closed() { "cmd.desc.poly.closed" } else { "cmd.desc.poly" }, &[("n", p.pts.len().to_string())]),
        Geo::Text(t) => c.tv("cmd.desc.text", &[("s", t.s.clone()), ("at", pp(t.x, t.y)), ("h", l(t.h))]),
        Geo::Insert(i) => {
            let na = i.attrs.as_ref().map_or(0, |a| a.len());
            if na > 0 {
                c.tv("cmd.desc.insert.attrs", &[("block", i.block.clone()), ("at", pp(i.x, i.y)), ("n", na.to_string())])
            } else {
                c.tv("cmd.desc.insert", &[("block", i.block.clone()), ("at", pp(i.x, i.y))])
            }
        }
        Geo::Point(p) => pp(p.x, p.y),
        Geo::Ellipse(el) => c.tv("cmd.desc.ellipse", &[("center", pp(el.cx, el.cy)), ("ratio", fmt(el.ratio))]),
        Geo::Hatch(h) => c.tv("cmd.desc.hatch", &[("n", h.loops.len().to_string())]),
        Geo::Dim(d) => {
            let kind = match d.kind.as_deref() {
                Some(k @ ("linear" | "aligned" | "angular" | "radius" | "diameter")) => c.t(&format!("dimkind.{k}")),
                _ => c.t("cmd.desc.dim.imported"),
            }
            .to_lowercase();
            if d.kind.as_deref().is_none_or(str::is_empty) {
                let mut vars = vec![("kind", kind)];
                if let Some(b) = &d.block {
                    vars.push(("block", b.clone()));
                }
                c.tv("cmd.desc.dim.block", &vars)
            } else if d.text.as_deref().is_some_and(|t| !t.is_empty()) {
                c.tv(
                    "cmd.desc.dim.text",
                    &[("kind", kind), ("value", format_measure(e, None)), ("text", d.text.clone().unwrap_or_default())],
                )
            } else {
                c.tv("cmd.desc.dim", &[("kind", kind), ("value", format_measure(e, None))])
            }
        }
        Geo::Viewport(v) => {
            c.tv("cmd.desc.viewport", &[("w", l(v.w)), ("h", l(v.h)), ("at", pp(v.x, v.y)), ("view", pp(v.cx, v.cy)), ("s", fmt(v.s))])
        }
        Geo::Xline(i) | Geo::Ray(i) => {
            c.tv(&format!("cmd.desc.{}", e.typ()), &[("at", pp(i.x, i.y)), ("angle", fmt(deg(norm_angle(i.dy.jatan2(i.dx)))))])
        }
        Geo::Mleader(m) => {
            let s = m.s.clone().unwrap_or_default();
            let one: String = {
                // oneLine: runs of CR / LF become one space
                let mut o = String::new();
                let mut in_run = false;
                for ch in s.chars() {
                    if ch == '\r' || ch == '\n' {
                        if !in_run {
                            o.push(' ');
                        }
                        in_run = true;
                    } else {
                        o.push(ch);
                        in_run = false;
                    }
                }
                o
            };
            let at = m.pts.first().map_or_else(|| pp(f64::NAN, f64::NAN), |p| pp(p.x, p.y));
            c.tv("cmd.mld.describe", &[("s", one), ("at", at), ("n", m.pts.len().to_string())])
        }
        Geo::Other => String::new(),
    }
}

pub async fn list(c: Ctx) -> R {
    let ids = c.select(None, false).await?;
    for id in &ids.0 {
        let Some(e) = c.doc().get(*id).cloned() else { continue };
        let line = c.tv("cmd.list.line", &[("type", type_name(&c, &e)), ("layer", e.layer_str().to_string()), ("what", describe(&c, &e))]);
        c.print(&line);
        if let Geo::Insert(i) = &e.geo {
            let rows = {
                let d = c.doc();
                attr_rows(&e, d.blocks.get(&i.block))
            };
            for a in rows {
                let v = if a.value.is_empty() { c.t("cmd.list.attr.empty") } else { a.value };
                let inv = if a.invisible { format!("  {}", c.t("cmd.list.attr.invisible")) } else { String::new() };
                let con = if a.constant { format!("  {}", c.t("cmd.list.attr.constant")) } else { String::new() };
                c.print(&format!("  {} = {v}{inv}{con}", a.tag));
            }
        }
    }
    Ok(())
}

// ---- selection

pub async fn selectall(c: Ctx) -> R {
    let ids: Vec<u64> = {
        let d = c.doc();
        d.entities().filter(|e| d.editable(e)).filter_map(|e| e.id).collect()
    };
    c.set_selection(&ids);
    c.print(&c.tv("cmd.selectall.done", &[("n", ids.len().to_string())]));
    Ok(())
}

pub async fn deselect(c: Ctx) -> R {
    c.set_selection(&[]);
    Ok(())
}

pub async fn select_cmd(c: Ctx) -> R {
    let ids = c.select(None, true).await?;
    c.set_selection(&ids.0);
    c.print(&c.tv("sel.select.done", &[("n", ids.len().to_string())]));
    Ok(())
}

pub async fn duplicate(c: Ctx) -> R {
    let ids = c.select(Some(c.t("cmd.duplicate.select_objects_duplicate")), false).await?;
    if ids.is_empty() {
        return Ok(());
    }
    let src: Rc<Vec<Entity>> = Rc::new({
        let d = c.doc();
        ids.0.iter().filter_map(|id| d.get(*id).cloned()).collect()
    });
    let cur = c.sh().host.borrow().cursor().filter(|p| p.x.is_finite() && p.y.is_finite());
    let start = match cur {
        Some(p) => p,
        None => {
            let d = c.doc();
            let mut b = cc_cad_geom::pick::empty_bounds();
            for e in src.iter() {
                let eb = entity_bounds(e, Some(&d.blocks));
                if valid_bounds(&eb) {
                    b = cc_cad_geom::geom::Bounds {
                        min_x: js::min(b.min_x, eb.min_x),
                        min_y: js::min(b.min_y, eb.min_y),
                        max_x: js::max(b.max_x, eb.max_x),
                        max_y: js::max(b.max_y, eb.max_y),
                    };
                }
            }
            if valid_bounds(&b) { P2::new((b.min_x + b.max_x) / 2.0, (b.min_y + b.max_y) / 2.0) } else { P2::new(0.0, 0.0) }
        }
    };
    c.st().last_point = Some(start);
    {
        let s = Rc::clone(&src);
        c.preview(Some(Rc::new(move |_, cur| s.iter().map(|e| tx_entity(e, &m_translate(cur.x - start.x, cur.y - start.y))).collect())));
    }
    let p = c.must_point(c.t("cmd.duplicate.specify_where_copy_goes"), Opts { base: Some(start), ..Opts::default() }).await?;
    c.preview(None);
    let m = m_translate(p.x - start.x, p.y - start.y);
    let made = c.change("DUPLICATE", |d| {
        let mut out = Vec::new();
        for e in src.iter() {
            let base = e.id.and_then(|id| d.get(id).cloned()).unwrap_or_else(|| e.clone());
            let mut t = tx_entity(&base, &m);
            t.id = None;
            if let Some(id) = d.add(t)?.id {
                out.push(id);
            }
        }
        Ok(out)
    })?;
    c.set_selection(&made);
    c.print(&c.tv("cmd.duplicate.done", &[("objects", c.tv("cmd.n.objects", &[("n", made.len().to_string())]))]));
    Ok(())
}

pub async fn purge(c: Ctx) -> R {
    let mut n = 0usize;
    c.change("PURGE", |d| {
        let mut again = true;
        while again {
            again = false;
            let names: Vec<String> = d.blocks.keys().cloned().collect();
            for name in names {
                if d.block_uses(&name) == 0 {
                    d.remove_block(&name)?;
                    n = n.saturating_add(1);
                    again = true;
                }
            }
        }
        Ok(())
    })?;
    c.print(&if n > 0 { c.tv("cmd.purge.removed", &[("n", n.to_string())]) } else { c.t("cmd.purge.none") });
    Ok(())
}
