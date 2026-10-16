//! TRIM and EXTEND (0.4 part 2, T.1–T.3). Quick mode (default; every object is a cutting edge or boundary) or
//! Standard (the edges you choose). Click a piece, drag a freehand path through several (one undo step), click two
//! empty points for a fence, or type F (fence) / C (crossing). Shift switches trim ↔ extend for that pick. What will go
//! (or be added) previews greyed and dashed (`ghost`). Quick mode erases an object nothing crosses.

use std::rc::Rc;

use cc_cad::entity::{Entity, Geo};
use cc_cad::{Doc, js};
use cc_cad_geom::edit::{Split, extend_entity, extension_part, path_crossings, piece_at, piece_entity, trim_result, trim_split};
use cc_cad_geom::geom::Bounds;
use cc_cad_geom::pick::{entity_bounds, in_window, valid_bounds};
use cc_geom::P2;
use serde_json::Value;

use crate::cmd::draw::{open_poly, rect, seg};
use crate::cmd::modify::undo_inside;
use crate::engine::{Ctx, Host, IdSet, Opts, R, Reply, Shared};
use crate::input::Def;

fn trimmable(e: &Entity) -> bool {
    matches!(e.geo, Geo::Line(_) | Geo::Arc(_) | Geo::Circle(_) | Geo::Poly(_) | Geo::Xline(_) | Geo::Ray(_))
}
fn edge_usable(d: &Doc, o: &Entity) -> bool {
    d.visible(o) && o.typ() != "text" && o.typ() != "point"
}
fn overlaps(ob: &Bounds, b: &Bounds, p: f64) -> bool {
    ob.max_x >= b.min_x - p && ob.min_x <= b.max_x + p && ob.max_y >= b.min_y - p && ob.min_y <= b.max_y + p
}

/// Entities whose bounds overlap e's (padded): candidate cutting edges and boundaries.
fn near(d: &Doc, e: &Entity) -> Vec<Entity> {
    if e.unbounded() {
        return d.entities().filter(|o| o.id != e.id && edge_usable(d, o)).cloned().collect();
    }
    let b = entity_bounds(e, Some(&d.blocks));
    if !valid_bounds(&b) {
        return Vec::new();
    }
    let p = js::max(b.max_x - b.min_x, b.max_y - b.min_y) * 0.01;
    d.entities().filter(|o| edge_usable(d, o) && (o.unbounded() || overlaps(&entity_bounds(o, Some(&d.blocks)), &b, p))).cloned().collect()
}

/// Cutting edges (or boundaries) for e: the chosen ones in Standard mode, else every visible object round it.
fn edges_for(d: &Doc, edges: Option<&IdSet>, e: &Entity, all: bool) -> Vec<Entity> {
    if let Some(ed) = edges {
        return ed.0.iter().filter_map(|id| d.get(*id)).filter(|o| o.id != e.id && edge_usable(d, o)).cloned().collect();
    }
    if all {
        return d.entities().filter(|o| o.id != e.id && edge_usable(d, o)).cloned().collect();
    }
    near(d, e)
}

/// How a trim or extend is aimed.
#[derive(Clone, Debug)]
enum How {
    Pick { id: u64, at: P2 },
    Path(Vec<P2>),
    Window(Bounds),
}

/// Objects a path or a crossing window reaches.
fn candidates(d: &Doc, host: &dyn Host, how: &How) -> Vec<Entity> {
    match how {
        How::Window(w) => host.pick_window(d, w, true).into_iter().filter_map(|id| d.get(id).cloned()).collect(),
        How::Path(pts) => {
            let mut b = Bounds { min_x: f64::INFINITY, min_y: f64::INFINITY, max_x: f64::NEG_INFINITY, max_y: f64::NEG_INFINITY };
            for p in pts {
                b.min_x = js::min(b.min_x, p.x);
                b.min_y = js::min(b.min_y, p.y);
                b.max_x = js::max(b.max_x, p.x);
                b.max_y = js::max(b.max_y, p.y);
            }
            d.entities()
                .filter(|e| {
                    if !d.editable(e) || !trimmable(e) {
                        return false;
                    }
                    if e.unbounded() {
                        return true;
                    }
                    let eb = entity_bounds(e, Some(&d.blocks));
                    valid_bounds(&eb) && overlaps(&eb, &b, 0.0)
                })
                .cloned()
                .collect()
        }
        How::Pick { .. } => Vec::new(),
    }
}

struct Item {
    e: Entity,
    split: Split,
    /// piece indexes, in the order they were added (a JS Set)
    gone: Vec<usize>,
}
struct Plan {
    refused: bool,
    list: Vec<Item>,
}

/// The session's TRIM/EXTEND settings.
#[derive(Clone, Copy)]
struct Mode {
    standard: bool,
    edge_extend: bool,
}
fn mode_of(sh: &Shared) -> Mode {
    let st = sh.st.borrow();
    Mode { standard: st.trim_standard, edge_extend: st.edge_mode == 1 }
}

fn push_unique(v: &mut Vec<usize>, i: usize) {
    if !v.contains(&i) {
        v.push(i);
    }
}

/// What a trim would remove, before anything changes.
fn trim_plan(d: &Doc, host: &dyn Host, m: Mode, edges: Option<&IdSet>, how: &How) -> Plan {
    let quick = !m.standard;
    let usable = |s: &Split| quick || s.pieces.len() > 1 || (s.closed && s.pieces.first().is_some_and(|p| !p.whole));
    if let How::Pick { id, at } = how {
        let Some(e) = d.get(*id).filter(|e| trimmable(e) && d.editable(e)) else { return Plan { refused: true, list: Vec::new() } };
        let split = trim_split(e, &edges_for(d, edges, e, m.edge_extend), Some(&d.blocks), m.edge_extend);
        let list = match split {
            Some(s) if usable(&s) => {
                let g = piece_at(&s, *at);
                vec![Item { e: e.clone(), split: s, gone: vec![g] }]
            }
            _ => Vec::new(),
        };
        return Plan { refused: false, list };
    }
    let mut list = Vec::new();
    for e in candidates(d, host, how) {
        if !trimmable(&e) {
            continue;
        }
        let hits = if let How::Path(pts) = how { Some(path_crossings(&e, pts, Some(&d.blocks))) } else { None };
        if hits.as_ref().is_some_and(Vec::is_empty) {
            continue;
        }
        let Some(split) = trim_split(&e, &edges_for(d, edges, &e, m.edge_extend), Some(&d.blocks), m.edge_extend) else { continue };
        if !usable(&split) {
            continue;
        }
        let mut gone = Vec::new();
        match (&hits, how) {
            (Some(h), _) => {
                for x in h {
                    push_unique(&mut gone, piece_at(&split, *x));
                }
            }
            (None, How::Window(w)) => {
                for i in 0..split.pieces.len() {
                    if piece_entity(&split, i).is_some_and(|pc| in_window(&pc, w, true, Some(&d.blocks))) {
                        push_unique(&mut gone, i);
                    }
                }
            }
            _ => {}
        }
        if !gone.is_empty() {
            list.push(Item { e, split, gone });
        }
    }
    Plan { refused: false, list }
}

/// Carry out a trim plan in one undo step → (objects changed, erased).
fn apply_trim(c: &Ctx, list: &[Item]) -> R<(usize, usize)> {
    if list.is_empty() {
        return Ok((0, 0));
    }
    c.change("TRIM", |d| {
        let (mut n, mut erased) = (0usize, 0usize);
        for it in list {
            let Some(eid) = it.e.id else { continue };
            if d.get(eid).is_none() {
                continue;
            }
            let Some(out) = trim_result(&it.split, &it.gone) else { continue };
            n = n.saturating_add(1);
            // the first piece keeps the object's id (so its constraints could stay with it)
            for (i, piece) in out.iter().enumerate() {
                let mut p = piece.clone();
                p.id = None;
                p.extra.remove("id");
                if i == 0 {
                    d.update(eid, p)?;
                } else {
                    d.add(p)?;
                }
            }
            if out.is_empty() {
                d.remove(eid)?;
                erased = erased.saturating_add(1);
            }
        }
        Ok((n, erased))
    })
}

/// What an extend would do: [(e, new e)].
fn extend_plan(d: &Doc, host: &dyn Host, m: Mode, edges: Option<&IdSet>, how: &How) -> Vec<(Entity, Entity)> {
    let one = |e: Option<&Entity>, at: P2| -> Option<(Entity, Entity)> {
        let e = e.filter(|e| d.editable(e) && matches!(e.geo, Geo::Line(_) | Geo::Arc(_) | Geo::Poly(_)))?;
        let nu = extend_entity(e, &edges_for(d, edges, e, true), at, Some(&d.blocks), m.edge_extend)?;
        Some((e.clone(), nu))
    };
    if let How::Pick { id, at } = how {
        return one(d.get(*id), *at).into_iter().collect();
    }
    let mut out = Vec::new();
    for e in candidates(d, host, how) {
        let at = match how {
            How::Path(pts) => match path_crossings(&e, pts, Some(&d.blocks)).first() {
                Some(x) => *x,
                None => continue,
            },
            How::Window(w) => P2::new((w.min_x + w.max_x) / 2.0, (w.min_y + w.max_y) / 2.0),
            How::Pick { .. } => continue,
        };
        if let Some(r) = one(Some(&e), at) {
            out.push(r);
        }
    }
    out
}

fn apply_extend(c: &Ctx, list: &[(Entity, Entity)]) -> R<usize> {
    if !list.is_empty() {
        c.change("EXTEND", |d| {
            for (e, nu) in list {
                if let Some(id) = e.id
                    && d.get(id).is_some()
                {
                    d.update(id, nu.clone())?;
                }
            }
            Ok(())
        })?;
    }
    Ok(list.len())
}

fn ghost(mut e: Entity) -> Entity {
    e.extra.insert("ghost".into(), Value::Bool(true));
    e
}

/// Preview entities: the pieces a trim would remove, or the parts an extend would add, greyed and dashed.
fn trim_ghosts(sh: &Shared, edges: Option<&IdSet>, cur: P2, extend: bool, path: Option<&[P2]>) -> Vec<Entity> {
    let d = sh.doc.borrow();
    let host = sh.host.borrow();
    let how = match path {
        Some(p) if p.len() > 1 => How::Path(p.to_vec()),
        _ => match host.pick_at(&d, cur) {
            Some(id) if id != 0 => How::Pick { id, at: cur },
            _ => return Vec::new(),
        },
    };
    let m = mode_of(sh);
    if extend {
        return extend_plan(&d, host.as_ref(), m, edges, &how).iter().filter_map(|(e, nu)| extension_part(e, nu).map(ghost)).collect();
    }
    trim_plan(&d, host.as_ref(), m, edges, &how)
        .list
        .iter()
        .flat_map(|it| it.gone.iter().filter_map(|i| piece_entity(&it.split, *i).map(ghost)))
        .collect()
}

fn settings_line(c: &Ctx) {
    let m = mode_of(c.sh());
    let mode = c.t(if m.standard { "opt.standard" } else { "opt.quick" });
    let edge = c.t(if m.edge_extend { "opt.extend" } else { "opt.noextend" });
    c.print(&c.tv("cmd.trim.settings", &[("mode", mode), ("edge", edge)]));
}

async fn choose_edges(c: &Ctx, is_trim: bool) -> R<Option<IdSet>> {
    let ids = c.select(Some(c.t(if is_trim { "cmd.trim.edges" } else { "cmd.extend.edges" })), false).await?;
    let edges = if ids.is_empty() { None } else { Some(ids) };
    c.set_selection(&[]);
    let msg = match &edges {
        Some(e) => c.tv("cmd.trim.edges.n", &[("n", e.len().to_string())]),
        None => c.t("cmd.trim.edges.all"),
    };
    c.print(&msg);
    Ok(edges)
}

/// Trim (or extend) as aimed, with the messages.
fn run(c: &Ctx, edges: Option<&IdSet>, how: &How, do_trim: bool) -> R {
    let m = mode_of(c.sh());
    let is_pick = matches!(how, How::Pick { .. });
    if do_trim {
        let plan = {
            let d = c.doc();
            let host = c.sh().host.borrow();
            trim_plan(&d, host.as_ref(), m, edges, how)
        };
        if plan.refused {
            c.print(&c.t("cmd.trim.cannot"));
            return Ok(());
        }
        if plan.list.is_empty() {
            c.print(&c.t(if is_pick { "cmd.trim.nothing" } else { "cmd.trim.nothing.path" }));
            return Ok(());
        }
        let (n, erased) = apply_trim(c, &plan.list)?;
        if !is_pick {
            c.print(&c.tv("cmd.trim.done", &[("n", n.to_string())]));
        }
        if erased > 0 && !m.standard && plan.list.iter().any(|it| it.split.pieces.len() == 1) {
            c.print(&c.tv("cmd.trim.erased", &[("n", erased.to_string())]));
        }
    } else {
        let list = {
            let d = c.doc();
            let host = c.sh().host.borrow();
            extend_plan(&d, host.as_ref(), m, edges, how)
        };
        if list.is_empty() {
            c.print(&c.t(if is_pick { "cmd.extend.nothing" } else { "cmd.extend.nothing.path" }));
            return Ok(());
        }
        let n = apply_extend(c, &list)?;
        if !is_pick {
            c.print(&c.tv("cmd.extend.done", &[("n", n.to_string())]));
        }
    }
    Ok(())
}

fn window_of(a: P2, b: P2) -> Bounds {
    Bounds { min_x: js::min(a.x, b.x), min_y: js::min(a.y, b.y), max_x: js::max(a.x, b.x), max_y: js::max(a.y, b.y) }
}

/// The answer to a Quick/Standard or Extend/No extend question → the key typed or the default.
fn key_of(k: &Reply) -> Option<String> {
    match k {
        Reply::Str(s) => Some(s.to_uppercase()),
        Reply::Opt(o) => Some(o.clone()),
        _ => None,
    }
}

pub async fn trim_or_extend(c: Ctx, is_trim: bool) -> R {
    let mut edges: Option<IdSet> = None;
    settings_line(&c);
    if mode_of(c.sh()).standard {
        edges = choose_edges(&c, is_trim).await?;
    } else {
        c.set_selection(&[]);
    }
    let options = if is_trim {
        vec![
            c.opt("T", &c.t("opt.cuttingedges")),
            c.opt("F", &c.t("sel.opt.fence")),
            c.opt("C", &c.t("sel.opt.crossing")),
            c.opt("O", &c.t("opt.mode")),
            c.opt("E", &c.t("opt.edge")),
            c.opt("R", &c.t("opt.erase")),
            c.opt("U", &c.t("opt.undo")),
        ]
    } else {
        vec![
            c.opt("B", &c.t("opt.boundaryedges")),
            c.opt("F", &c.t("sel.opt.fence")),
            c.opt("C", &c.t("sel.opt.crossing")),
            c.opt("O", &c.t("opt.mode")),
            c.opt("E", &c.t("opt.edge")),
            c.opt("U", &c.t("opt.undo")),
        ]
    };
    // true = this one trims
    let flips = move |shift: bool| is_trim != shift;
    loop {
        let ed = edges.clone();
        c.preview(Some(Rc::new(move |sh, cur| {
            let shift = sh.host.borrow().shift();
            let drag = sh.st.borrow().drag_path.clone();
            trim_ghosts(sh, ed.as_ref(), cur, !flips(shift), drag.as_deref())
        })));
        let prompt = c.t(if is_trim { "cmd.trim.pick" } else { "cmd.extend.pick" });
        let r = c.entity(prompt, Opts { none: true, path: true, empty: true, options: options.clone(), ..Opts::default() }).await?;
        match r {
            Reply::Null => return Ok(()),
            Reply::Opt(k) => match k.as_str() {
                "U" => undo_inside(&c),
                "T" | "B" => {
                    c.st().trim_standard = true;
                    c.preview(None);
                    edges = choose_edges(&c, is_trim).await?;
                }
                "O" => {
                    c.preview(None);
                    let std = mode_of(c.sh()).standard;
                    let opts = vec![c.opt("Q", &c.t("opt.quick")), c.opt("S", &c.t("opt.standard"))];
                    let k = c
                        .keyword(
                            c.t("cmd.trim.mode.ask"),
                            Opts { options: opts, default: Some(Def::Str(if std { "S" } else { "Q" }.into())), ..Opts::default() },
                        )
                        .await?;
                    match key_of(&k).as_deref() {
                        Some("S") => {
                            c.st().trim_standard = true;
                            edges = choose_edges(&c, is_trim).await?;
                        }
                        Some("Q") => {
                            c.st().trim_standard = false;
                            edges = None;
                        }
                        _ => {}
                    }
                    settings_line(&c);
                }
                "E" => {
                    c.preview(None);
                    let ext = mode_of(c.sh()).edge_extend;
                    let opts = vec![c.opt("E", &c.t("opt.extend")), c.opt("N", &c.t("opt.noextend"))];
                    let k = c
                        .keyword(
                            c.t("cmd.trim.edge.ask"),
                            Opts { options: opts, default: Some(Def::Str(if ext { "E" } else { "N" }.into())), ..Opts::default() },
                        )
                        .await?;
                    match key_of(&k).as_deref() {
                        Some("E") => c.st().edge_mode = 1,
                        Some("N") => c.st().edge_mode = 0,
                        _ => {}
                    }
                    settings_line(&c);
                }
                "R" => {
                    c.preview(None);
                    let sel = c.select(Some(c.t("cmd.trim.erase.select")), true).await?;
                    let ids: Vec<u64> = {
                        let d = c.doc();
                        sel.0.into_iter().filter(|id| d.get(*id).is_some_and(|e| d.editable(e))).collect()
                    };
                    if !ids.is_empty() {
                        c.change("ERASE", |d| {
                            for id in &ids {
                                d.remove(*id)?;
                            }
                            Ok(())
                        })?;
                    }
                    c.set_selection(&[]);
                }
                "F" => {
                    let mut pts: Vec<P2> = Vec::new();
                    loop {
                        let snap = pts.clone();
                        let ed = edges.clone();
                        c.preview(Some(Rc::new(move |sh, cur| {
                            if snap.is_empty() {
                                return Vec::new();
                            }
                            let mut v = snap.clone();
                            v.push(cur);
                            let mut out = vec![open_poly(&v)];
                            out.extend(trim_ghosts(sh, ed.as_ref(), cur, !is_trim, Some(&v)));
                            out
                        })));
                        let options = if pts.is_empty() { Vec::new() } else { vec![c.opt("U", &c.t("opt.undo"))] };
                        let prompt = if pts.is_empty() { c.t("sel.fence.first") } else { c.t("sel.fence.next") };
                        let p = c.point(prompt, Opts { base: pts.last().copied(), none: true, options, ..Opts::default() }).await?;
                        match p {
                            Reply::Opt(k) if k == "U" => {
                                pts.pop();
                            }
                            Reply::Point(q, _) => pts.push(q),
                            _ => break,
                        }
                    }
                    if pts.len() >= 2 {
                        run(&c, edges.as_ref(), &How::Path(pts), is_trim)?;
                    }
                }
                "C" => {
                    c.preview(None);
                    let a = c.must_point(c.t("sel.window.first"), Opts::default()).await?;
                    c.preview(Some(Rc::new(move |_, cur| vec![rect(a, cur)])));
                    let b = c.must_point(c.t("sel.window.second"), Opts { base: Some(a), ..Opts::default() }).await?;
                    run(&c, edges.as_ref(), &How::Window(window_of(a, b)), is_trim)?;
                }
                _ => {}
            },
            Reply::Path { pts, shift } => run(&c, edges.as_ref(), &How::Path(pts), flips(shift))?,
            Reply::Empty { at: a, .. } => {
                // two clicks on empty spots: a fence between them
                let ed = edges.clone();
                c.preview(Some(Rc::new(move |sh, cur| {
                    let shift = sh.host.borrow().shift();
                    let mut out = vec![seg(a, cur)];
                    out.extend(trim_ghosts(sh, ed.as_ref(), cur, !flips(shift), Some(&[a, cur])));
                    out
                })));
                let b = c.point(c.t("cmd.trim.fence.second"), Opts { base: Some(a), none: true, ..Opts::default() }).await?;
                if let Some(b) = b.point() {
                    let shift = c.sh().host.borrow().shift();
                    run(&c, edges.as_ref(), &How::Path(vec![a, b]), flips(shift))?;
                }
            }
            Reply::Pick { id, at, shift } => run(&c, edges.as_ref(), &How::Pick { id, at }, flips(shift))?,
            _ => return Ok(()),
        }
    }
}
