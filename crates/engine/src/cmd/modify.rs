//! Modify: ERASE MOVE COPY ROTATE SCALE MIRROR OFFSET.

use std::rc::Rc;

use cc_cad::entity::{Entity, Geo};
use cc_cad::js;
use cc_cad_geom::edit::offset_entity;
use cc_cad_geom::geom::{M, Prim, TAU, angle_of, dist, m_about, m_mirror, m_rotate, m_scale, m_translate, norm_angle, prim_point, prims};
use cc_cad_geom::jsmath::JsMath;
use cc_cad_geom::tx::{tx_entity_sim, tx_sim};
use cc_geom::P2;

use crate::engine::{Ctx, OffsetMemo, Opts, R, Reply};
use crate::input::Def;

pub async fn erase(c: Ctx) -> R {
    let ids = c.select(None, false).await?;
    if ids.is_empty() {
        return Ok(());
    }
    c.change("ERASE", |d| {
        for id in &ids.0 {
            d.remove(*id)?;
        }
        Ok(())
    })?;
    c.set_selection(&[]);
    c.print(&format!("{} erased", ids.len()));
    Ok(())
}

/// `apply(c, src, m, copy)`: one change moving (or copying) the objects through m.
fn apply(c: &Ctx, src: &[u64], m: &M, copy: bool) -> R {
    apply_then(c, src, m, copy, |_, _| Ok(()))
}

/// commands.js `apply(c, src, m, copy, after)`: `after(doc, pairs)` runs inside the same change with (source id, new id)
/// for every object (MIRROR's constraints).
fn apply_then(
    c: &Ctx,
    src: &[u64],
    m: &M,
    copy: bool,
    after: impl FnOnce(&mut cc_cad::Doc, &[(u64, u64)]) -> Result<(), cc_cad::DocError>,
) -> R {
    let sim = tx_sim(m);
    c.change(if copy { "COPY" } else { "MOVE" }, |d| {
        let mut pairs: Vec<(u64, u64)> = Vec::with_capacity(src.len());
        for &id in src {
            let Some(e) = d.get(id) else { continue };
            let mut t = tx_entity_sim(e, m, &sim);
            if copy {
                t.id = None;
                t.extra.remove("id");
                let n = d.add(t)?;
                pairs.push((id, n.id.unwrap_or(0)));
            } else {
                d.update(id, t)?;
                pairs.push((id, id));
            }
        }
        after(d, &pairs)
    })
}

/// Undo the last step the running command made (its own U option).
pub fn undo_inside(c: &Ctx) {
    let mark = c.st().cmd_mark;
    let n = c.doc().undo_stack.len();
    if n > mark {
        let mut d = c.sh().doc.borrow_mut();
        d.undo();
        d.redo_stack.clear();
    } else {
        c.print(&c.t("cmd.extend.nothing_undo_command"));
    }
}

/// The selected objects that exist, by id: the command reads them from the drawing (which it does not change until
/// it applies), as the JS keeps references to them, instead of copying each one.
pub(crate) fn sources(c: &Ctx, ids: &[u64]) -> Vec<u64> {
    let d = c.doc();
    ids.iter().copied().filter(|id| d.get(*id).is_some()).collect()
}

fn tx_all(sh: &crate::engine::Shared, src: &[u64], m: &M) -> Vec<Entity> {
    let Ok(d) = sh.doc.try_borrow() else { return Vec::new() };
    let sim = tx_sim(m);
    src.iter().filter_map(|id| d.get(*id)).map(|e| tx_entity_sim(e, m, &sim)).collect()
}

pub async fn move_like(c: Ctx, copy: bool) -> R {
    let ids = c.select(None, false).await?;
    if ids.is_empty() {
        return Ok(());
    }
    let src = Rc::new(sources(&c, &ids.0));
    let b = c.point(c.t("cmd.stretch.base"), Opts { options: vec![c.opt("D", "Displacement")], ..Opts::default() }).await?;
    if b.option() == Some("D") {
        let d = c.must_point(c.t("cmd.stretch.disp"), Opts::default()).await?;
        return apply(&c, &src, &m_translate(d.x, d.y), copy);
    }
    let Some(base) = b.point() else { return Ok(()) };
    loop {
        let s = Rc::clone(&src);
        c.preview(Some(Rc::new(move |sh, cur| tx_all(sh, &s, &m_translate(cur.x - base.x, cur.y - base.y)))));
        let options = if copy { vec![c.opt("U", "Undo")] } else { Vec::new() };
        let p = c.point(c.t("cmd.mea.p2"), Opts { base: Some(base), none: true, options, ..Opts::default() }).await?;
        match p {
            Reply::Null => return Ok(()),
            Reply::Opt(k) if k == "U" => {
                undo_inside(&c);
                continue;
            }
            Reply::Point(p, _) => {
                apply(&c, &src, &m_translate(p.x - base.x, p.y - base.y), copy)?;
                if !copy {
                    return Ok(());
                }
            }
            _ => return Ok(()),
        }
    }
}

pub async fn rotate(c: Ctx) -> R {
    let ids = c.select(None, false).await?;
    if ids.is_empty() {
        return Ok(());
    }
    let src = Rc::new(sources(&c, &ids.0));
    let base = c.must_point(c.t("cmd.stretch.base"), Opts::default()).await?;
    let s = Rc::clone(&src);
    c.preview(Some(Rc::new(move |sh, cur| tx_all(sh, &s, &m_about(base, &m_rotate(angle_of(base, cur)))))));
    let default = Def::Num(c.st().last_rotation.unwrap_or(0.0));
    let a = c.angle(c.t("cmd.mtext.rot"), Opts { base: Some(base), default: Some(default), ..Opts::default() }).await?;
    let Reply::Num(a) = a else { return Ok(()) };
    c.st().last_rotation = Some(a);
    apply(&c, &src, &m_about(base, &m_rotate(a)), false)
}

pub async fn scale(c: Ctx) -> R {
    let ids = c.select(None, false).await?;
    if ids.is_empty() {
        return Ok(());
    }
    let src = Rc::new(sources(&c, &ids.0));
    let base = c.must_point(c.t("cmd.stretch.base"), Opts::default()).await?;
    let s = Rc::clone(&src);
    c.preview(Some(Rc::new(move |sh, cur| {
        let f = js::max(dist(base, cur), 1e-9);
        tx_all(sh, &s, &m_about(base, &m_scale(f, f)))
    })));
    let r = c.distance(c.t("cmd.scale.specify_scale_factor"), Opts { base: Some(base), ..Opts::default() }).await?;
    let Reply::Num(f) = r else { return Ok(()) };
    if f == 0.0 || f.is_nan() {
        return Ok(());
    }
    apply(&c, &src, &m_about(base, &m_scale(f, f)), false)
}

pub async fn mirror(c: Ctx) -> R {
    let ids = c.select(None, false).await?;
    if ids.is_empty() {
        return Ok(());
    }
    let src = Rc::new(sources(&c, &ids.0));
    let a = c.must_point(c.t("cmd.mirror.specify_first_point_mirror"), Opts::default()).await?;
    let s = Rc::clone(&src);
    c.preview(Some(Rc::new(move |sh, cur| if dist(a, cur) > 1e-12 { tx_all(sh, &s, &m_mirror(a, cur)) } else { Vec::new() })));
    let b = c.must_point(c.t("cmd.mirror.specify_second_point_mirror"), Opts { base: Some(a), ..Opts::default() }).await?;
    if dist(a, b) < 1e-12 {
        return Ok(());
    }
    c.preview(None);
    let options = vec![c.opt("Y", "Yes"), c.opt("N", "No")];
    let k =
        c.keyword(c.t("cmd.mirror.erase_source_objects"), Opts { options, default: Some(Def::Str("N".into())), ..Opts::default() }).await?;
    let erase = match &k {
        Reply::Opt(k) | Reply::Str(k) => k == "Y",
        _ => false,
    };
    // constraints come along: onto the copies (signs and sides worked out again), or kept on the mirrored objects (ckeep.js)
    let tr = c.sh().lang.cat();
    let n = std::cell::Cell::new(0usize);
    apply_then(&c, &src, &m_mirror(a, b), !erase, |d, pairs| {
        let k = if erase {
            let ids: Vec<u64> = pairs.iter().map(|p| p.0).collect();
            cc_gcons::ckeep::resign_among(d, &tr, &ids)?
        } else {
            cc_gcons::ckeep::mirror_constraints(d, &tr, pairs)?
        };
        n.set(k);
        Ok(())
    })?;
    if n.get() > 0 {
        c.print(&c.tv("cmd.mirror.constraints", &[("n", n.get().to_string())]));
    }
    Ok(())
}

/// geom.primParam without the arc rev flag (commands.js `closestT`).
pub(crate) fn closest_t(p: &Prim, q: P2) -> f64 {
    if p.is_l() {
        let (dx, dy) = (p.x2 - p.x1, p.y2 - p.y1);
        let l = dx * dx + dy * dy;
        return if l != 0.0 { js::max(0.0, js::min(1.0, ((q.x - p.x1) * dx + (q.y - p.y1) * dy) / l)) } else { 0.0 };
    }
    let a = norm_angle((q.y - p.cy).jatan2(q.x - p.cx) - p.a0);
    let s = p.a1 - p.a0;
    if a <= s {
        a / s
    } else if a - s < TAU - a {
        1.0
    } else {
        0.0
    }
}

pub async fn offset(c: Ctx) -> R {
    let default = match c.st().last_offset.clone() {
        Some(OffsetMemo::Dist(d)) => Def::Num(d),
        _ => Def::Str("Through".into()),
    };
    let d = c
        .distance(
            c.t("cmd.offset.specify_offset_distance"),
            Opts { options: vec![c.opt("T", "Through")], default: Some(default), ..Opts::default() },
        )
        .await?;
    let through = matches!(&d, Reply::Str(s) if s == "Through") || d.option() == Some("T");
    let dist_v = match d {
        Reply::Num(n) => n,
        _ => f64::NAN,
    };
    if !through {
        if !(dist_v > 0.0) {
            return Ok(());
        }
        c.st().last_offset = Some(OffsetMemo::Dist(dist_v));
    } else {
        c.st().last_offset = Some(OffsetMemo::Through);
    }
    c.set_selection(&[]);
    loop {
        let Reply::Pick { id, .. } = c.entity(c.t("cmd.offset.select_object_offset"), Opts { none: true, ..Opts::default() }).await? else {
            return Ok(());
        };
        let Some(e) = c.doc().get(id).cloned() else { return Ok(()) };
        if !matches!(e.geo, Geo::Line(_) | Geo::Circle(_) | Geo::Arc(_) | Geo::Poly(_)) {
            c.print(&c.t("cmd.offset.cannot_offset_object"));
            continue;
        }
        c.set_selection(&[id]);
        let e = Rc::new(e);
        let dist_for = {
            let e = Rc::clone(&e);
            move |p: P2| -> f64 {
                if through { js::min_of(prims(&e, None).iter().map(|q| dist(p, prim_point(q, closest_t(q, p))))) } else { dist_v }
            }
        };
        let df = dist_for.clone();
        let pe = Rc::clone(&e);
        c.preview(Some(Rc::new(move |_, cur| offset_entity(&pe, df(cur), cur).map(|o| vec![o]).unwrap_or_default())));
        let prompt = if through { c.t("cmd.offset.specify_through_point") } else { c.t("cmd.offset.specify_point_side_offset") };
        let side = c.point(prompt, Opts { none: true, ..Opts::default() }).await?;
        c.preview(None);
        c.set_selection(&[]);
        let Some(side) = side.point() else { return Ok(()) };
        let Some(mut o) = offset_entity(&e, dist_for(side), side) else {
            c.print(&c.t("cmd.offset.cannot_offset_far"));
            continue;
        };
        o.id = None;
        o.extra.remove("id");
        c.change("OFFSET", |d| d.add(o))?;
    }
}
