//! Modify, part 2 (commands.js): STRETCH, ARRAY (ARRAYRECT ARRAYPOLAR), BREAK (BREAKATPOINT), LENGTHEN, JOIN,
//! MATCHPROP.

use std::rc::Rc;

use cc_cad::entity::{Color, Entity, Geo};
use cc_cad::js;
use cc_cad_geom::edit::{break_entity, curve_length, join_entities, lengthen_entity, stretch_entity};
use cc_cad_geom::geom::{
    Blocks, Bounds, PI, TAU, angle_of, deg, dist, m_about, m_apply, m_rotate, m_translate, norm_angle, polar, prim_point, prims, rad,
};
use cc_cad_geom::pick::{entity_bounds, valid_bounds};
use cc_cad_geom::tx::tx_entity;
use cc_cad_geom::units::{format_length, units_of};
use cc_geom::P2;
use serde_json::Value;

use crate::cmd::modify::{closest_t, undo_inside};
use crate::engine::{Ctx, Opts, R, Reply, Sel};
use crate::input::{Def, fmt};

/// `fmtLen(c, n)`: a length in the drawing's format.
pub fn fmt_len(c: &Ctx, n: f64) -> String {
    let d = c.doc();
    let (code, prec) = units_of(Some(&d.meta));
    format_length(n, f64::from(code), Some(&Value::from(prec)))
}

/// The bounds of several objects (invalid ones skipped).
fn union_bounds(src: &[Entity], blocks: Option<&Blocks>) -> Bounds {
    let mut acc = Bounds { min_x: f64::INFINITY, min_y: f64::INFINITY, max_x: f64::NEG_INFINITY, max_y: f64::NEG_INFINITY };
    for e in src {
        let b = entity_bounds(e, blocks);
        if valid_bounds(&b) {
            acc = Bounds {
                min_x: js::min(acc.min_x, b.min_x),
                min_y: js::min(acc.min_y, b.min_y),
                max_x: js::max(acc.max_x, b.max_x),
                max_y: js::max(acc.max_y, b.max_y),
            };
        }
    }
    acc
}

fn not_viewport(e: &Entity) -> bool {
    !matches!(e.geo, Geo::Viewport(_))
}

// ---- STRETCH

pub async fn stretch(c: Ctx) -> R {
    c.print(&c.t("cmd.stretch.hint"));
    let ids = c.select(Some(c.t("cmd.stretch.select")), false).await?;
    if ids.is_empty() {
        return Ok(());
    }
    let wins: Rc<Vec<Bounds>> = Rc::new(c.st().sel_windows.clone());
    if wins.is_empty() {
        c.print(&c.t("cmd.stretch.nowindow"));
    }
    let src: Rc<Vec<Entity>> = Rc::new({
        let d = c.doc();
        ids.0.iter().filter_map(|id| d.get(*id).cloned()).filter(not_viewport).collect()
    });
    let moved = {
        let wins = Rc::clone(&wins);
        move |e: &Entity, dx: f64, dy: f64| -> Option<Entity> {
            if wins.is_empty() {
                return Some(tx_entity(e, &m_translate(dx, dy)));
            }
            let inside = |q: P2| {
                wins.iter().any(|w| q.x >= w.min_x - 1e-9 && q.x <= w.max_x + 1e-9 && q.y >= w.min_y - 1e-9 && q.y <= w.max_y + 1e-9)
            };
            stretch_entity(e, &inside, dx, dy)
        }
    };
    let moved = Rc::new(moved);
    let b = c.point(c.t("cmd.stretch.base"), Opts { options: vec![c.opt("D", &c.t("opt.displacement"))], ..Opts::default() }).await?;
    let d = match b {
        Reply::Opt(_) => c.must_point(c.t("cmd.stretch.disp"), Opts::default()).await?,
        Reply::Point(b, _) => {
            let (s, m) = (Rc::clone(&src), Rc::clone(&moved));
            c.preview(Some(Rc::new(move |_, cur| s.iter().filter_map(|e| m(e, cur.x - b.x, cur.y - b.y)).collect())));
            let p = c.must_point(c.t("cmd.stretch.second"), Opts { base: Some(b), ..Opts::default() }).await?;
            P2::new(p.x - b.x, p.y - b.y)
        }
        _ => return Ok(()),
    };
    c.preview(None);
    let mut n = 0usize;
    c.change("STRETCH", |doc| {
        for e in src.iter() {
            let id = e.id.unwrap_or(0);
            let nx = doc.get(id).and_then(|cur| moved(cur, d.x, d.y));
            if let Some(nx) = nx {
                doc.update(id, nx)?;
                n = n.saturating_add(1);
            }
        }
        Ok(())
    })?;
    c.print(&c.tv("cmd.stretch.done", &[("n", n.to_string())]));
    Ok(())
}

// ---- ARRAY

#[derive(Clone, Debug)]
struct Arr {
    rect: bool,
    cols: f64,
    rows: f64,
    dx: f64,
    dy: f64,
    center: P2,
    items: f64,
    fill: f64,
    rotate: bool,
}

/// `arrayCopies`: the copies an array makes (not the originals), or None past `limit`.
fn array_copies(src: &[Entity], a: &Arr, limit: usize) -> Option<Vec<Entity>> {
    let mut out = Vec::new();
    if a.rect {
        let mut r = 0.0;
        while r < a.rows {
            let mut k = 0.0;
            while k < a.cols {
                if r != 0.0 || k != 0.0 {
                    let m = m_translate(k * a.dx, r * a.dy);
                    for e in src {
                        out.push(tx_entity(e, &m));
                        if out.len() > limit {
                            return None;
                        }
                    }
                }
                k += 1.0;
            }
            r += 1.0;
        }
    } else {
        let bounds = union_bounds(src, None);
        let full = (a.fill.abs() - TAU).abs() < 1e-9;
        let step = if full { a.fill / a.items } else { a.fill / js::max(1.0, a.items - 1.0) };
        let mid = if valid_bounds(&bounds) {
            P2::new((bounds.min_x + bounds.max_x) / 2.0, (bounds.min_y + bounds.max_y) / 2.0)
        } else {
            a.center
        };
        let mut i = 1.0;
        while i < a.items {
            let turn = m_about(a.center, &m_rotate(step * i));
            let m = if a.rotate {
                turn
            } else {
                let p = m_apply(&turn, mid);
                m_translate(p.x - mid.x, p.y - mid.y)
            };
            for e in src {
                out.push(tx_entity(e, &m));
                if out.len() > limit {
                    return None;
                }
            }
            i += 1.0;
        }
    }
    Some(out)
}

fn whole(q: f64) -> bool {
    q.fract() == 0.0 && (1.0..=10000.0).contains(&q)
}

pub async fn array_cmd(c: Ctx, kind: Option<&'static str>) -> R {
    let ids = c.select(None, false).await?;
    if ids.is_empty() {
        return Ok(());
    }
    let src: Rc<Vec<Entity>> = Rc::new({
        let d = c.doc();
        ids.0.iter().filter_map(|id| d.get(*id).cloned()).filter(not_viewport).collect()
    });
    let rect = match kind {
        Some(k) => k == "rect",
        None => {
            let options = vec![c.opt("R", &c.t("opt.rectangular")), c.opt("PO", &c.t("opt.polar"))];
            let k = c.keyword(c.t("cmd.array.type"), Opts { options, default: Some(Def::Str("R".into())), ..Opts::default() }).await?;
            !matches!(&k, Reply::Opt(s) | Reply::Str(s) if s == "PO")
        }
    };
    let mut a = if rect {
        let b = union_bounds(&src, Some(&c.doc().blocks));
        let (w, h) = if valid_bounds(&b) { (js::max(b.max_x - b.min_x, 1e-6), js::max(b.max_y - b.min_y, 1e-6)) } else { (1.0, 1.0) };
        Arr { rect: true, cols: 4.0, rows: 3.0, dx: w * 1.5, dy: h * 1.5, center: P2::new(0.0, 0.0), items: 0.0, fill: 0.0, rotate: false }
    } else {
        let center = c.must_point(c.t("cmd.array.center"), Opts::default()).await?;
        Arr { rect: false, cols: 0.0, rows: 0.0, dx: 0.0, dy: 0.0, center, items: 6.0, fill: TAU, rotate: true }
    };
    // the preview reads the settings as they are now (a count changed while the next one is asked shows at once)
    let live = Rc::new(std::cell::RefCell::new(a.clone()));
    loop {
        *live.borrow_mut() = a.clone();
        {
            let (s, aa) = (Rc::clone(&src), Rc::clone(&live));
            c.preview(Some(Rc::new(move |_, _| array_copies(&s, &aa.borrow(), 3000).unwrap_or_default())));
        }
        if a.rect {
            c.print(&c.tv(
                "cmd.array.rect.now",
                &[
                    ("cols", js::num_to_string(a.cols)),
                    ("rows", js::num_to_string(a.rows)),
                    ("dx", fmt_len(&c, a.dx)),
                    ("dy", fmt_len(&c, a.dy)),
                ],
            ));
        } else {
            let rot = c.t(if a.rotate { "opt.yes" } else { "opt.no" });
            c.print(&c.tv("cmd.array.polar.now", &[("items", js::num_to_string(a.items)), ("fill", fmt(deg(a.fill))), ("rotate", rot)]));
        }
        let options = if a.rect {
            vec![
                c.opt("COU", &c.t("opt.count")),
                c.opt("S", &c.t("opt.spacing")),
                c.opt("COL", &c.t("opt.columns")),
                c.opt("R", &c.t("opt.rows")),
                c.opt("X", &c.t("opt.exit")),
            ]
        } else {
            vec![
                c.opt("I", &c.t("opt.items")),
                c.opt("F", &c.t("opt.fill")),
                c.opt("ROT", &c.t("opt.rotateitems")),
                c.opt("X", &c.t("opt.exit")),
            ]
        };
        let k = c.keyword(c.t("cmd.array.option"), Opts { options, default: Some(Def::Str("X".into())), ..Opts::default() }).await?;
        let o = match &k {
            Reply::Opt(s) | Reply::Str(s) => s.clone(),
            _ => String::new(),
        };
        if o == "X" || o.is_empty() {
            break;
        }
        async fn ask(c: &Ctx, key: &str, def: f64, test: fn(f64) -> bool, msg: &str, length: bool) -> R<f64> {
            let v = c.number(c.t(key), Opts { default: Some(Def::Num(def)), length, ..Opts::default() }).await?;
            if let Reply::Num(v) = v
                && test(v)
            {
                return Ok(v);
            }
            c.print(&c.t(msg));
            Ok(def)
        }
        let finite: fn(f64) -> bool = f64::is_finite;
        if o == "COU" || o == "COL" {
            a.cols = ask(&c, "cmd.array.cols", a.cols, whole, "cmd.array.whole", false).await?;
            *live.borrow_mut() = a.clone();
        }
        if o == "COU" || o == "R" {
            a.rows = ask(&c, "cmd.array.rows", a.rows, whole, "cmd.array.whole", false).await?;
            *live.borrow_mut() = a.clone();
        }
        if o == "S" || o == "COL" {
            a.dx = ask(&c, "cmd.array.dx", a.dx, finite, "cmd.array.number", true).await?;
            *live.borrow_mut() = a.clone();
        }
        if o == "S" || o == "R" {
            a.dy = ask(&c, "cmd.array.dy", a.dy, finite, "cmd.array.number", true).await?;
            *live.borrow_mut() = a.clone();
        }
        if o == "I" {
            a.items = ask(&c, "cmd.array.items", a.items, whole, "cmd.array.whole", false).await?;
            *live.borrow_mut() = a.clone();
        }
        if o == "F" {
            let v = c.number(c.t("cmd.array.fill"), Opts { default: Some(Def::Num(deg(a.fill))), ..Opts::default() }).await?;
            match v {
                Reply::Num(v) if v.is_finite() && v != 0.0 && v.abs() <= 360.0 => {
                    a.fill = rad(v);
                    *live.borrow_mut() = a.clone();
                }
                _ => c.print(&c.t("cmd.array.fill.bad")),
            }
        }
        if o == "ROT" {
            let options = vec![c.opt("Y", &c.t("opt.yes")), c.opt("N", &c.t("opt.no"))];
            let def = if a.rotate { "Y" } else { "N" };
            let r = c.keyword(c.t("cmd.array.rotate"), Opts { options, default: Some(Def::Str(def.into())), ..Opts::default() }).await?;
            a.rotate = !matches!(&r, Reply::Opt(s) | Reply::Str(s) if s == "N");
            *live.borrow_mut() = a.clone();
        }
    }
    c.preview(None);
    let count = if a.rect { a.cols * a.rows } else { a.items };
    if count < 2.0 {
        c.print(&c.t("cmd.array.one"));
        return Ok(());
    }
    let Some(copies) = array_copies(&src, &a, 20000) else {
        c.print(&c.t("cmd.array.toomany"));
        return Ok(());
    };
    let n = copies.len();
    c.change("ARRAY", |d| {
        for mut e in copies {
            e.id = None;
            d.add(e)?;
        }
        Ok(())
    })?;
    c.print(&c.tv("cmd.array.done", &[("n", n.to_string())]));
    Ok(())
}

// ---- BREAK

/// `onEntity(e, q)`: the point on e nearest q.
fn on_entity(e: &Entity, q: P2) -> P2 {
    let mut best: Option<P2> = None;
    let mut bd = f64::INFINITY;
    for p in prims(e, None) {
        let x = prim_point(&p, closest_t(&p, q));
        let d = dist(x, q);
        if d < bd {
            bd = d;
            best = Some(x);
        }
    }
    best.unwrap_or(q)
}

pub async fn break_cmd(c: Ctx, at_point: bool) -> R {
    loop {
        let r = c.entity(c.t("cmd.break.select"), Opts::default()).await?;
        let Reply::Pick { id, at, .. } = r else { return Ok(()) };
        let Some(e) = c.doc().get(id).cloned() else { continue };
        if !c.doc().editable(&e) {
            c.print(&c.t("cmd.corner.err.locked"));
            continue;
        }
        if !matches!(e.typ(), "line" | "arc" | "circle" | "poly" | "xline" | "ray") {
            c.print(&c.t("cmd.break.type"));
            continue;
        }
        c.set_selection(&[id]);
        let mut p1 = on_entity(&e, at);
        let p2;
        if at_point {
            let p = c.must_point(c.t("cmd.break.at"), Opts::default()).await?;
            p1 = on_entity(&e, p);
            p2 = p1;
        } else {
            let q = c.point(c.t("cmd.break.second"), Opts { options: vec![c.opt("F", &c.t("opt.firstpoint"))], ..Opts::default() }).await?;
            match q {
                Reply::Opt(_) => {
                    let a = c.must_point(c.t("cmd.break.first"), Opts::default()).await?;
                    p1 = on_entity(&e, a);
                    let b = c.must_point(c.t("cmd.break.second"), Opts::default()).await?;
                    p2 = on_entity(&e, b);
                }
                Reply::Point(q, _) => p2 = on_entity(&e, q),
                _ => return Ok(()),
            }
        }
        match break_entity(&e, p1, p2) {
            Err(code) => {
                c.print(&c.t(if code == "circle" { "cmd.break.circle" } else { "cmd.break.type" }));
                return Ok(());
            }
            Ok(pieces) => {
                let n = pieces.len();
                c.change("BREAK", |d| {
                    d.remove(id)?;
                    for mut x in pieces {
                        x.id = None;
                        d.add(x)?;
                    }
                    Ok(())
                })?;
                c.print(&c.tv("cmd.break.done", &[("n", n.to_string())]));
                return Ok(());
            }
        }
    }
}

// ---- LENGTHEN

/// LENGTHEN's settings (`E.lengthen`).
#[derive(Clone, Debug, PartialEq)]
pub struct LengthenMemo {
    pub mode: String,
    pub delta: f64,
    pub d_angle: Option<f64>,
    pub percent: f64,
    pub total: f64,
    pub t_angle: Option<f64>,
}

fn len_memo(c: &Ctx) -> LengthenMemo {
    c.st()
        .memo
        .lengthen
        .get_or_insert_with(|| LengthenMemo { mode: "T".into(), delta: 0.0, d_angle: None, percent: 100.0, total: 1.0, t_angle: None })
        .clone()
}
fn set_len(c: &Ctx, m: LengthenMemo) {
    c.st().memo.lengthen = Some(m);
}

async fn set_mode(c: &Ctx, m: &str) -> R {
    let s = len_memo(c);
    match m {
        "DE" => {
            let v = c
                .number(
                    c.t("cmd.len.delta"),
                    Opts {
                        default: Some(Def::Num(s.delta)),
                        length: true,
                        options: vec![c.opt("A", &c.t("opt.angle"))],
                        ..Opts::default()
                    },
                )
                .await?;
            match v {
                Reply::Opt(_) => {
                    let a = c
                        .number(c.t("cmd.len.dangle"), Opts { default: Some(Def::Num(deg(js::or(s.d_angle, 0.0)))), ..Opts::default() })
                        .await?;
                    let mut s = len_memo(c);
                    s.d_angle = Some(rad(if let Reply::Num(a) = a { a } else { f64::NAN }));
                    set_len(c, s);
                }
                Reply::Num(v) => {
                    let mut s = len_memo(c);
                    s.delta = v;
                    s.d_angle = None;
                    set_len(c, s);
                }
                _ => {}
            }
        }
        "P" => {
            let v = c.number(c.t("cmd.len.percent"), Opts { default: Some(Def::Num(s.percent)), ..Opts::default() }).await?;
            match v {
                Reply::Num(v) if v > 0.0 => {
                    let mut s = len_memo(c);
                    s.percent = v;
                    set_len(c, s);
                }
                _ => c.print(&c.t("cmd.len.percent.bad")),
            }
        }
        "T" => {
            let v = c
                .number(
                    c.t("cmd.len.total"),
                    Opts {
                        default: Some(Def::Num(s.total)),
                        length: true,
                        options: vec![c.opt("A", &c.t("opt.angle"))],
                        ..Opts::default()
                    },
                )
                .await?;
            match v {
                Reply::Opt(_) => {
                    let d = deg(match s.t_angle {
                        Some(a) if a != 0.0 && !a.is_nan() => a,
                        _ => PI / 2.0,
                    });
                    let a = c.number(c.t("cmd.len.tangle"), Opts { default: Some(Def::Num(d)), ..Opts::default() }).await?;
                    if let Reply::Num(a) = a
                        && a > 0.0
                        && a < 360.0
                    {
                        let mut s = len_memo(c);
                        s.t_angle = Some(rad(a));
                        set_len(c, s);
                    }
                }
                Reply::Num(v) if v > 0.0 => {
                    let mut s = len_memo(c);
                    s.total = v;
                    s.t_angle = None;
                    set_len(c, s);
                }
                _ => {}
            }
        }
        _ => {}
    }
    let mut s = len_memo(c);
    s.mode = m.into();
    set_len(c, s);
    Ok(())
}

/// The change for one object in the current mode.
fn len_change(s: &LengthenMemo, e: &Entity, pick: P2, cur: Option<P2>) -> Result<Entity, &'static str> {
    let Some(l) = curve_length(e) else { return Err("type") };
    if matches!(&e.geo, Geo::Poly(p) if p.is_closed()) {
        return Err("type");
    }
    let sweep = |e: &Entity| if let Geo::Arc(a) = &e.geo { js::or(Some(norm_angle(a.a1 - a.a0)), TAU) } else { f64::NAN };
    let is_arc = matches!(e.geo, Geo::Arc(_));
    match s.mode.as_str() {
        "DE" => match s.d_angle {
            Some(da) if is_arc => lengthen_entity(e, pick, 0.0, Some(da)),
            _ => lengthen_entity(e, pick, s.delta, None),
        },
        "P" => lengthen_entity(e, pick, l * (s.percent / 100.0 - 1.0), None),
        "T" => match s.t_angle {
            Some(ta) if is_arc => lengthen_entity(e, pick, 0.0, Some(ta - sweep(e))),
            _ => lengthen_entity(e, pick, s.total - l, None),
        },
        _ => {
            let Some(cur) = cur else { return Err("type") };
            let (a, b) = match &e.geo {
                Geo::Arc(a) => {
                    let c0 = P2::new(a.cx, a.cy);
                    let at_start = dist(pick, polar(c0, a.a0, a.r)) < dist(pick, polar(c0, a.a1, a.r));
                    let ang = norm_angle(angle_of(c0, cur));
                    let mut ne = e.clone();
                    if let Geo::Arc(x) = &mut ne.geo {
                        if at_start {
                            x.a0 = ang;
                        } else {
                            x.a1 = ang;
                        }
                    }
                    return Ok(ne);
                }
                Geo::Line(ln) => {
                    let (p1, p2) = (P2::new(ln.x1, ln.y1), P2::new(ln.x2, ln.y2));
                    let at_start = dist(pick, p1) < dist(pick, p2);
                    if at_start { (p2, p1) } else { (p1, p2) }
                }
                Geo::Poly(pl) => {
                    let n = pl.pts.len();
                    let (Some(first), Some(last)) = (pl.pts.first(), pl.pts.last()) else { return Err("type") };
                    let at_start = dist(pick, first.p()) < dist(pick, last.p());
                    let end_b = if at_start { first.bulge() } else { pl.pts.get(n.wrapping_sub(2)).map_or(0.0, |v| v.bulge()) };
                    if end_b.abs() > 1e-12 {
                        return Err("type");
                    }
                    let (Some(sec), Some(pen)) = (pl.pts.get(1), pl.pts.get(n.wrapping_sub(2))) else { return Err("type") };
                    if at_start { (sec.p(), first.p()) } else { (pen.p(), last.p()) }
                }
                _ => return Err("type"),
            };
            let ls = dist(a, b);
            let along = ((cur.x - a.x) * (b.x - a.x) + (cur.y - a.y) * (b.y - a.y)) / if ls == 0.0 || ls.is_nan() { 1.0 } else { ls };
            lengthen_entity(e, pick, along - ls, None)
        }
    }
}

pub async fn lengthen(c: Ctx) -> R {
    let _ = len_memo(&c);
    let mode_opts = vec![
        c.opt("DE", &c.t("opt.delta")),
        c.opt("P", &c.t("opt.percent")),
        c.opt("T", &c.t("opt.total")),
        c.opt("DY", &c.t("opt.dynamic")),
    ];
    loop {
        let pick = c.entity(c.t("cmd.len.measure"), Opts { options: mode_opts.clone(), none: true, ..Opts::default() }).await?;
        let id = match pick {
            Reply::Opt(k) => {
                set_mode(&c, &k).await?;
                break;
            }
            Reply::Pick { id, .. } => id,
            _ => break,
        };
        let Some(e) = c.doc().get(id).cloned() else { continue };
        let Some(l) = curve_length(&e) else {
            c.print(&c.t("cmd.len.type"));
            continue;
        };
        if let Geo::Arc(a) = &e.geo {
            let sw = js::or(Some(norm_angle(a.a1 - a.a0)), TAU);
            c.print(&c.tv("cmd.len.current.arc", &[("l", fmt_len(&c, l)), ("a", fmt(deg(sw)))]));
        } else {
            c.print(&c.tv("cmd.len.current", &[("l", fmt_len(&c, l))]));
        }
    }
    loop {
        if len_memo(&c).mode == "DY" {
            c.preview(None);
        }
        let pick =
            c.entity(c.t("cmd.len.change"), Opts { none: true, options: vec![c.opt("U", &c.t("opt.undo"))], ..Opts::default() }).await?;
        let (id, at) = match pick {
            Reply::Opt(k) if k == "U" => {
                undo_inside(&c);
                continue;
            }
            Reply::Pick { id, at, .. } => (id, at),
            _ => return Ok(()),
        };
        let Some(e) = c.doc().get(id).cloned() else { continue };
        if !c.doc().editable(&e) {
            c.print(&c.t("cmd.corner.err.locked"));
            continue;
        }
        let s = len_memo(&c);
        let r = if s.mode == "DY" {
            {
                let (e2, s2) = (e.clone(), s.clone());
                c.preview(Some(Rc::new(move |_, cur| len_change(&s2, &e2, at, Some(cur)).ok().into_iter().collect())));
            }
            let to = c.must_point(c.t("cmd.len.dynamic"), Opts::default()).await?;
            c.preview(None);
            len_change(&s, &e, at, Some(to))
        } else {
            len_change(&s, &e, at, None)
        };
        match r {
            Err(code) => c.print(&c.t(&format!("cmd.len.err.{code}"))),
            Ok(ne) => {
                c.change("LENGTHEN", |d| d.update(id, ne))?;
            }
        }
    }
}

// ---- JOIN

pub async fn join(c: Ctx) -> R {
    let ids = c.select(Some(c.t("cmd.join.select")), false).await?;
    if ids.len() < 2 {
        if !ids.is_empty() {
            c.print(&c.t("cmd.join.two"));
        }
        return Ok(());
    }
    let ents: Vec<Entity> = {
        let d = c.doc();
        ids.0.iter().filter_map(|id| d.get(*id).cloned()).collect()
    };
    let (results, left) = join_entities(&ents, None);
    if results.is_empty() {
        c.print(&c.t("cmd.join.none"));
        return Ok(());
    }
    let n: usize = results.iter().map(|r| r.from.len()).sum();
    let m = results.len();
    c.change("JOIN", |d| {
        for r in &results {
            for id in r.from.iter().flatten() {
                d.remove(*id)?;
            }
            let mut x = r.to.clone();
            x.id = None;
            d.add(x)?;
        }
        Ok(())
    })?;
    c.print(&c.tv("cmd.join.done", &[("n", n.to_string()), ("m", m.to_string())]));
    if !left.is_empty() {
        c.print(&c.tv("cmd.join.left", &[("n", left.len().to_string())]));
    }
    Ok(())
}

// ---- MATCHPROP

/// Which properties MATCHPROP copies (`E.matchOn`): colour, layer, text, dimension, hatch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MatchOn {
    pub color: bool,
    pub layer: bool,
    pub text: bool,
    pub dim: bool,
    pub hatch: bool,
}
impl Default for MatchOn {
    fn default() -> Self {
        MatchOn { color: true, layer: true, text: true, dim: true, hatch: true }
    }
}
const MATCH_KEYS: [(&str, &str); 5] =
    [("C", "opt.color"), ("LA", "opt.layer"), ("T", "opt.text"), ("D", "opt.dimension"), ("H", "opt.hatch")];
fn on_get(m: &MatchOn, i: usize) -> bool {
    [m.color, m.layer, m.text, m.dim, m.hatch].get(i).copied().unwrap_or(false)
}
fn on_flip(m: &mut MatchOn, i: usize) {
    match i {
        0 => m.color = !m.color,
        1 => m.layer = !m.layer,
        2 => m.text = !m.text,
        3 => m.dim = !m.dim,
        _ => m.hatch = !m.hatch,
    }
}

/// `matchProps(src, dst, on)`: dst with the source's properties that `on` allows.
pub fn match_props(src: &Entity, dst: &Entity, on: &MatchOn) -> Entity {
    let mut out = dst.clone();
    if on.layer {
        out.layer = src.layer.clone();
    }
    if on.color {
        out.color = Some(Color::or_null(&src.color));
    }
    if on.text
        && let (Geo::Text(s), Geo::Text(d)) = (&src.geo, &mut out.geo)
    {
        d.h = s.h;
        d.wf = Some(match s.wf {
            Some(w) if w != 0.0 && !w.is_nan() => w,
            _ => 1.0,
        });
    }
    if on.dim
        && let (Geo::Dim(s), Geo::Dim(d)) = (&src.geo, &mut out.geo)
        && let Some(st) = &s.st
    {
        d.st = Some(st.clone());
        d.block = None;
    }
    if on.hatch
        && let (Geo::Hatch(s), Geo::Hatch(d)) = (&src.geo, &mut out.geo)
    {
        d.solid = Some(s.solid == Some(true));
        for k in ["pattern", "pscale", "pangle", "piso", "patlines"] {
            match src.extra.get(k) {
                Some(v) => {
                    out.extra.insert(k.into(), v.clone());
                }
                None => {
                    out.extra.shift_remove(k);
                }
            }
        }
    }
    out
}

pub async fn matchprop(c: Ctx) -> R {
    let on0 = *c.st().memo.match_on.get_or_insert_with(MatchOn::default);
    let _ = on0;
    let r = c.entity(c.t("cmd.ma.source"), Opts::default()).await?;
    let Reply::Pick { id: sid, .. } = r else { return Ok(()) };
    let Some(src) = c.doc().get(sid).cloned() else { return Ok(()) };
    c.set_selection(&[sid]);
    let say = |c: &Ctx| {
        let on = c.st().memo.match_on.unwrap_or_default();
        let list: Vec<String> = MATCH_KEYS.iter().enumerate().filter(|(i, _)| on_get(&on, *i)).map(|(_, (_, l))| c.t(l)).collect();
        let list = if list.is_empty() { c.t("cmd.ma.nothing") } else { list.join(" ") };
        c.print(&c.tv("cmd.ma.active", &[("list", list)]));
    };
    say(&c);
    loop {
        let sel = c.select_with(Some(c.t("cmd.ma.dest")), true, vec![c.opt("S", &c.t("opt.settings"))]).await?;
        let mut ids = match sel {
            Sel::Opt(k) if k == "S" => {
                loop {
                    let options: Vec<_> = MATCH_KEYS.iter().map(|(k, l)| c.opt(k, &c.t(l))).collect();
                    let k = c.keyword(c.t("cmd.ma.toggle"), Opts { options, none: true, ..Opts::default() }).await?;
                    let Reply::Opt(key) = k else { break };
                    let Some(i) = MATCH_KEYS.iter().position(|(k, _)| *k == key) else { break };
                    {
                        let mut st = c.st();
                        let m = st.memo.match_on.get_or_insert_with(MatchOn::default);
                        on_flip(m, i);
                    }
                    say(&c);
                }
                continue;
            }
            Sel::Opt(_) => crate::engine::IdSet::default(),
            Sel::Set(s) => s,
        };
        ids.delete(sid);
        if ids.is_empty() {
            return Ok(());
        }
        let on = c.st().memo.match_on.unwrap_or_default();
        let mut n = 0usize;
        c.change("MATCHPROP", |d| {
            for id in &ids.0 {
                let next = match d.get(*id) {
                    Some(dst) if d.editable(dst) => match_props(&src, dst, &on),
                    _ => continue,
                };
                d.update(*id, next)?;
                n = n.saturating_add(1);
            }
            Ok(())
        })?;
        c.print(&c.tv("cmd.ma.done", &[("n", n.to_string())]));
        return Ok(());
    }
}
