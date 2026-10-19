//! Draw: LINE PLINE CIRCLE (2P 3P TTR TTT) ARC RECTANG POLYGON ELLIPSE POINT TEXT.

use std::rc::Rc;

use cc_cad::entity::{Arc, Circle, Ellipse, Entity, Geo, Line, Point, Poly, Text, Vertex};
use cc_cad::js;
use cc_cad_geom::geom::{PI, TAU, angle_of, bulge_arc, dist, norm_angle, polar};
use cc_cad_geom::jsmath::JsMath;
use cc_cad_geom::pick::{TanOther, resolve_tangent};
use cc_geom::P2;

use crate::engine::{Ctx, Opts, R, Reply};
use crate::input::{Def, hidden};
use crate::tancircle::{Curve, circle_ttr, circle_ttt, curve_at};
use crate::ucs::{active_ucs, ang_of};

/// `ent(c, e)`: a new object on the current layer, in the current colour, linetype and lineweight.
pub fn ent(c: &Ctx, geo: Geo) -> Entity {
    let d = c.doc();
    let (lt, lw) = d.current_style();
    Entity {
        id: None,
        layer: Some(d.current.layer.clone()),
        color: Some(d.current.color.clone()),
        lt: lt.map(Some),
        lw: lw.map(Some),
        geo,
        extra: Default::default(),
    }
}

pub fn line_g(a: P2, b: P2) -> Geo {
    Geo::Line(Line { x1: a.x, y1: a.y, x2: b.x, y2: b.y })
}
pub fn circle_g(cx: f64, cy: f64, r: f64) -> Geo {
    Geo::Circle(Circle { cx, cy, r })
}
/// A bare preview line.
pub fn seg(a: P2, b: P2) -> Entity {
    Entity::new(line_g(a, b))
}
/// `rect(a, b)`: a closed four-corner polyline.
pub fn rect(a: P2, b: P2) -> Entity {
    Entity::new(rect_g(a, b))
}
fn rect_g(a: P2, b: P2) -> Geo {
    let v = |x: f64, y: f64| Vertex::new(x, y, 0.0);
    Geo::Poly(Poly { pts: vec![v(a.x, a.y), v(b.x, a.y), v(b.x, b.y), v(a.x, b.y)], closed: Some(true) })
}
/// An open polyline through points (fence previews).
pub fn open_poly(pts: &[P2]) -> Entity {
    Entity::new(Geo::Poly(Poly { pts: pts.iter().map(|p| Vertex::new(p.x, p.y, 0.0)).collect(), closed: Some(false) }))
}

/// `circle3(a, b, c)`: the circle through three points, or None when they are in line.
pub fn circle3(a: P2, b: P2, c: P2) -> Option<(f64, f64, f64)> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-12 {
        return None;
    }
    let a2 = a.x * a.x + a.y * a.y;
    let b2 = b.x * b.x + b.y * b.y;
    let c2 = c.x * c.x + c.y * c.y;
    let cx = (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d;
    let cy = (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d;
    Some((cx, cy, js::hypot(a.x - cx, a.y - cy)))
}

/// `arc3(s, m, e)`: the arc from s through m to e.
pub fn arc3(s: P2, m: P2, e: P2) -> Option<Arc> {
    let (cx, cy, r) = circle3(s, m, e)?;
    let c = P2::new(cx, cy);
    let ccw = (m.x - s.x) * (e.y - s.y) - (m.y - s.y) * (e.x - s.x) > 0.0;
    let (mut a0, mut a1) = (angle_of(c, s), angle_of(c, e));
    if !ccw {
        std::mem::swap(&mut a0, &mut a1);
    }
    Some(Arc { cx, cy, r, a0: norm_angle(a0), a1: norm_angle(a1) })
}

fn none() -> Opts {
    Opts { none: true, ..Opts::default() }
}
fn based(b: P2) -> Opts {
    Opts { base: Some(b), ..Opts::default() }
}

// ---- LINE

pub async fn line(c: Ctx) -> R {
    let (first, ftan) = match c.point(c.t("cmd.mea.p1"), none()).await? {
        Reply::Point(p, t) => (p, t),
        _ => match c.st().last_point {
            Some(p) => (p, None),
            None => return Ok(()),
        },
    };
    let mut start = first;
    let mut prev = first;
    let mut deferred = ftan;
    let mut made: Vec<(u64, P2)> = Vec::new();
    loop {
        let (dfr, pv) = (deferred, prev);
        c.preview(Some(Rc::new(move |_, cur| {
            let a = match dfr {
                Some(t) => resolve_tangent(&t.c, t.near, TanOther::Point(cur)).and_then(|v| v.first().copied()),
                None => Some(pv),
            };
            a.map(|a| vec![seg(a, cur)]).unwrap_or_default()
        })));
        let mut options = vec![c.opt("U", "Undo")];
        if made.len() >= 2 {
            options.push(c.opt("C", "Close"));
        }
        let base = if deferred.is_some() { None } else { Some(prev) };
        let r = c.point(c.t("cmd.line.specify_next_point"), Opts { base, options, none: true, ..Opts::default() }).await?;
        let (mut p, ptan, key) = match r {
            Reply::Null => return Ok(()),
            Reply::Point(q, t) => (q, t, None),
            Reply::Opt(k) => (P2::new(f64::NAN, f64::NAN), None, Some(k)),
            _ => return Ok(()),
        };
        if let (Some(dt), None) = (deferred, &key) {
            // the tangent's touch point is known now: from this point, or the line touching both circles
            let other = match ptan {
                Some(t2) => TanOther::Tan(t2.c, t2.near),
                None => TanOther::Point(p),
            };
            let Some(v) = resolve_tangent(&dt.c, dt.near, other) else {
                c.print(&c.t("cmd.line.no_tangent"));
                continue;
            };
            match (ptan.is_some(), v.first(), v.get(1)) {
                (true, Some(a), Some(b)) => {
                    prev = *a;
                    p = *b;
                }
                (_, Some(a), _) => prev = *a,
                _ => {}
            }
            start = prev;
            deferred = None;
        }
        match key.as_deref() {
            Some("U") => {
                let Some((id, _)) = made.pop() else {
                    c.print(&c.t("cmd.line.all_segments_already_undone"));
                    continue;
                };
                c.change("LINE", |d| d.remove(id))?;
                prev = made.last().map_or(start, |m| m.1);
                c.st().last_point = Some(prev);
                continue;
            }
            Some("C") => {
                c.add(ent(&c, line_g(prev, start)))?;
                c.st().last_point = Some(start);
                return Ok(());
            }
            Some(_) => continue,
            None => {}
        }
        if dist(prev, p) < 1e-12 {
            continue;
        }
        let e = c.add(ent(&c, line_g(prev, p)))?;
        made.push((e.id.unwrap_or(0), p));
        prev = p;
    }
}

// ---- PLINE

/// The direction at the end of the last segment.
fn pl_tangent(pts: &[Vertex]) -> Option<f64> {
    let n = pts.len();
    if n < 2 {
        return None;
    }
    let (p, q) = (pts.get(n.saturating_sub(2))?, pts.get(n.saturating_sub(1))?);
    let b = p.bulge();
    if b == 0.0 || b.is_nan() {
        return Some(angle_of(p.p(), q.p()));
    }
    let a = bulge_arc(p.p(), q.p(), b);
    let ang = (q.y - a.cy).jatan2(q.x - a.cx);
    Some(ang + if b > 0.0 { PI / 2.0 } else { -PI / 2.0 })
}
fn arc_bulge(pts: &[Vertex], p: P2, q: P2) -> f64 {
    let t = pl_tangent(pts).unwrap_or(0.0);
    let mut phi = norm_angle(angle_of(p, q) - t);
    if phi > PI {
        phi -= TAU;
    }
    (phi / 2.0).jtan()
}

pub async fn pline(c: Ctx) -> R {
    let start = c.must_point(c.t("cmd.pline.specify_start_point"), Opts::default()).await?;
    let mut pts = vec![Vertex::new(start.x, start.y, 0.0)];
    let mut arc_mode = false;
    let mut closed = false;
    loop {
        let snap = pts.clone();
        let am = arc_mode;
        c.preview(Some(Rc::new(move |_, cur| {
            let Some(last) = snap.last() else { return Vec::new() };
            let b = if am { arc_bulge(&snap, last.p(), cur) } else { 0.0 };
            let mut v: Vec<Vertex> = snap.iter().take(snap.len().saturating_sub(1)).cloned().collect();
            let mut l = last.clone();
            l.b = Some(b);
            v.push(l);
            v.push(Vertex::new(cur.x, cur.y, 0.0));
            vec![Entity::new(Geo::Poly(Poly { pts: v, closed: Some(false) }))]
        })));
        let many = pts.len() > 2;
        let options = if !arc_mode {
            let mut o = vec![c.opt("A", "Arc")];
            if many {
                o.push(c.opt("C", "Close"));
            }
            o.push(c.opt("U", "Undo"));
            o
        } else {
            let mut o = vec![c.opt("L", "Line")];
            if many {
                o.push(c.opt("CL", "CLose"));
            }
            o.push(c.opt("U", "Undo"));
            o
        };
        let last = pts.last().map(Vertex::p).unwrap_or(start);
        let prompt = if arc_mode { c.t("cmd.pline.specify_endpoint_arc") } else { c.t("cmd.line.specify_next_point") };
        let r = c.point(prompt, Opts { base: Some(last), options, none: true, ..Opts::default() }).await?;
        match r {
            Reply::Opt(k) => match k.as_str() {
                "A" => arc_mode = true,
                "L" => arc_mode = false,
                "U" => {
                    if pts.len() > 1 {
                        pts.pop();
                        if let Some(l) = pts.last_mut() {
                            l.b = Some(0.0);
                        }
                        c.st().last_point = pts.last().map(Vertex::p);
                    }
                }
                "C" | "CL" => {
                    if arc_mode {
                        let first = pts.first().map(Vertex::p).unwrap_or(start);
                        let b = arc_bulge(&pts, last, first);
                        if let Some(l) = pts.last_mut() {
                            l.b = Some(b);
                        }
                    }
                    closed = true;
                    break;
                }
                _ => {}
            },
            Reply::Point(p, _) => {
                if dist(last, p) < 1e-12 {
                    continue;
                }
                let b = if arc_mode { arc_bulge(&pts, last, p) } else { 0.0 };
                if let Some(l) = pts.last_mut() {
                    l.b = Some(b);
                }
                pts.push(Vertex::new(p.x, p.y, 0.0));
            }
            _ => break,
        }
    }
    if pts.len() >= 2 {
        c.add(ent(&c, Geo::Poly(Poly { pts, closed: Some(closed) })))?;
    }
    Ok(())
}

// ---- CIRCLE

/// Pick one line, arc, circle or polyline segment (also inside a block) for a tangent circle.
async fn pick_tangent(c: &Ctx, prompt: &str) -> R<(Curve, P2)> {
    loop {
        if let Reply::Pick { id, at, .. } = c.entity(c.t(prompt), Opts::default()).await? {
            let o = {
                let d = c.doc();
                curve_at(d.get(id), at, Some(&d.blocks))
            };
            if let Some(o) = o {
                return Ok((o, at));
            }
        }
        c.print(&c.t("cmd.circle.tan.not_curve"));
    }
}

pub async fn circle_ttr_cmd(c: Ctx) -> R {
    circle_ttr_(&c).await
}
async fn circle_ttr_(c: &Ctx) -> R {
    let (ao, aat) = pick_tangent(c, "cmd.circle.tan.first").await?;
    let (bo, bat) = pick_tangent(c, "cmd.circle.tan.second").await?;
    c.preview(Some(Rc::new(move |_, cur| {
        circle_ttr(&ao, aat, &bo, bat, js::max(dist(aat, cur), 1e-9))
            .map(|k| vec![Entity::new(circle_g(k.cx, k.cy, k.r))])
            .unwrap_or_default()
    })));
    let default = c.st().last_radius.map(Def::Num);
    let r = c.distance(c.t("cmd.circle.specify_radius_circle"), Opts { base: Some(aat), default, ..Opts::default() }).await?;
    let Reply::Num(r) = r else { return Ok(()) };
    if !(r > 0.0) {
        return Ok(());
    }
    let Some(k) = circle_ttr(&ao, aat, &bo, bat, r) else {
        c.print(&c.t("cmd.circle.no_circle"));
        return Ok(());
    };
    c.st().last_radius = Some(r);
    c.add(ent(c, circle_g(k.cx, k.cy, k.r)))?;
    Ok(())
}
async fn circle_ttt_(c: &Ctx) -> R {
    let (ao, aat) = pick_tangent(c, "cmd.circle.tan.first").await?;
    let (bo, bat) = pick_tangent(c, "cmd.circle.tan.second").await?;
    let (co, cat) = pick_tangent(c, "cmd.circle.tan.third").await?;
    let Some(k) = circle_ttt(&ao, aat, &bo, bat, &co, cat) else {
        c.print(&c.t("cmd.circle.no_circle"));
        return Ok(());
    };
    c.add(ent(c, circle_g(k.cx, k.cy, k.r)))?;
    Ok(())
}

pub async fn circle(c: Ctx) -> R {
    let options = vec![c.opt("3P", "3P"), c.opt("2P", "2P"), c.opt("T", "Ttr (tan tan radius)"), hidden("TTT", "TTT")];
    let r0 = c.point(c.t("cmd.circle.specify_center_point_circle"), Opts { options, ..Opts::default() }).await?;
    match r0.option() {
        Some("T") => return circle_ttr_(&c).await,
        Some("TTT") => return circle_ttt_(&c).await,
        Some("2P") => {
            let a = c.must_point(c.t("cmd.circle.specify_first_end_point"), Opts::default()).await?;
            c.preview(Some(Rc::new(move |_, cur| {
                vec![Entity::new(circle_g((a.x + cur.x) / 2.0, (a.y + cur.y) / 2.0, dist(a, cur) / 2.0))]
            })));
            let b = c.must_point(c.t("cmd.circle.specify_second_end_point"), based(a)).await?;
            c.add(ent(&c, circle_g((a.x + b.x) / 2.0, (a.y + b.y) / 2.0, dist(a, b) / 2.0)))?;
            return Ok(());
        }
        Some("3P") => {
            let a = c.must_point(c.t("cmd.circle.specify_first_point_circle"), Opts::default()).await?;
            let b = c.must_point(c.t("cmd.circle.specify_second_point_circle"), based(a)).await?;
            c.preview(Some(Rc::new(move |_, cur| {
                circle3(a, b, cur).map(|(x, y, r)| vec![Entity::new(circle_g(x, y, r))]).unwrap_or_default()
            })));
            let d = c.must_point(c.t("cmd.circle.specify_third_point_circle"), based(b)).await?;
            let Some((x, y, r)) = circle3(a, b, d) else {
                c.print(&c.t("cmd.circle.three_points_line"));
                return Ok(());
            };
            c.add(ent(&c, circle_g(x, y, r)))?;
            return Ok(());
        }
        _ => {}
    }
    let Some(center) = r0.point() else { return Ok(()) };
    let mut diameter = false;
    loop {
        let dm = diameter;
        c.preview(Some(Rc::new(move |_, cur| {
            vec![Entity::new(circle_g(center.x, center.y, js::max(dist(center, cur) / if dm { 2.0 } else { 1.0 }, 1e-9)))]
        })));
        let prompt = if diameter { c.t("cmd.circle.specify_diameter_circle") } else { c.t("cmd.circle.specify_radius_circle") };
        let options = if diameter { Vec::new() } else { vec![c.opt("D", "Diameter")] };
        let default = c.st().last_radius.map(Def::Num);
        let r = c.distance(prompt, Opts { base: Some(center), options, default, ..Opts::default() }).await?;
        if r.option() == Some("D") {
            diameter = true;
            continue;
        }
        let Reply::Num(r) = r else { return Ok(()) };
        if r == 0.0 || r.is_nan() {
            return Ok(());
        }
        let radius = if diameter { r / 2.0 } else { r };
        c.st().last_radius = Some(radius);
        c.add(ent(&c, circle_g(center.x, center.y, radius)))?;
        return Ok(());
    }
}

// ---- ARC

pub async fn arc(c: Ctx) -> R {
    let p0 = c.point(c.t("cmd.arc.specify_start_point_arc"), Opts { options: vec![c.opt("C", "Center")], ..Opts::default() }).await?;
    if p0.option() == Some("C") {
        let center = c.must_point(c.t("cmd.arc.specify_center_point_arc"), Opts::default()).await?;
        let s = c.must_point(c.t("cmd.arc.specify_start_point_arc"), based(center)).await?;
        let (r, a0) = (dist(center, s), angle_of(center, s));
        c.preview(Some(Rc::new(move |_, cur| {
            vec![Entity::new(Geo::Arc(Arc { cx: center.x, cy: center.y, r, a0, a1: angle_of(center, cur) }))]
        })));
        let e = c.must_point(c.t("cmd.arc.specify_end_point_arc"), based(center)).await?;
        c.add(ent(&c, Geo::Arc(Arc { cx: center.x, cy: center.y, r, a0: norm_angle(a0), a1: norm_angle(angle_of(center, e)) })))?;
        return Ok(());
    }
    let Some(p0) = p0.point() else { return Ok(()) };
    let p1 = c.must_point(c.t("cmd.arc.specify_second_point_arc"), based(p0)).await?;
    c.preview(Some(Rc::new(move |_, cur| match arc3(p0, p1, cur) {
        Some(a) => vec![Entity::new(Geo::Arc(a))],
        None => vec![seg(p0, p1)],
    })));
    let p2 = c.must_point(c.t("cmd.arc.specify_end_point_arc"), based(p1)).await?;
    let Some(a) = arc3(p0, p1, p2) else {
        c.print(&c.t("cmd.circle.three_points_line"));
        return Ok(());
    };
    c.add(ent(&c, Geo::Arc(a)))?;
    Ok(())
}

// ---- RECTANG

pub async fn rectang(c: Ctx) -> R {
    let a = c.must_point(c.t("cmd.area.first"), Opts::default()).await?;
    c.preview(Some(Rc::new(move |_, cur| vec![rect(a, cur)])));
    let b = c.must_point(c.t("cmd.rectang.specify_other_corner_point"), based(a)).await?;
    if (b.x - a.x).abs() < 1e-12 || (b.y - a.y).abs() < 1e-12 {
        c.print(&c.t("cmd.rectang.rectangle_needs_width_height"));
        return Ok(());
    }
    c.add(ent(&c, rect_g(a, b)))?;
    Ok(())
}

// ---- POLYGON

/// Most sides POLYGON draws (input-derived count, capped as the JS caps it).
pub const MAX_SIDES: f64 = 1024.0;

fn polygon_g(center: P2, cur: P2, sides: usize, circumscribed: bool) -> Geo {
    let n = cc_cad_geom::geom::f(sides);
    let r = dist(center, cur);
    let a = angle_of(center, cur);
    let rr = if circumscribed { r / (PI / n).jcos() } else { r };
    let a0 = if circumscribed { a + PI / n } else { a };
    let pts = (0..sides)
        .map(|i| {
            let q = polar(center, a0 + cc_cad_geom::geom::f(i) * TAU / n, rr);
            Vertex::new(q.x, q.y, 0.0)
        })
        .collect();
    Geo::Poly(Poly { pts, closed: Some(true) })
}

pub async fn polygon(c: Ctx) -> R {
    let default = Def::Num(c.st().last_sides.filter(|n| *n != 0.0 && !n.is_nan()).unwrap_or(4.0));
    let n = c.number(c.t("cmd.polygon.enter_number_sides"), Opts { default: Some(default), ..Opts::default() }).await?;
    let sides = match n {
        Reply::Num(n) => js::round(n),
        _ => f64::NAN,
    };
    if !(3.0..=MAX_SIDES).contains(&sides) {
        c.print(&c.t("cmd.polygon.requires_whole_number_3"));
        return Ok(());
    }
    c.st().last_sides = Some(sides);
    // 3..=1024 and integral
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let count = sides as usize;
    let center = c.must_point(c.t("cmd.polygon.specify_center_polygon"), Opts::default()).await?;
    let options = vec![c.opt("I", &c.t("cmd.polygon.inscribed_circle")), c.opt("C", &c.t("cmd.polygon.circumscribed_about_circle"))];
    let mode0 = c.st().last_poly_mode.clone().filter(|m| !m.is_empty()).unwrap_or_else(|| "I".into());
    let k = c.keyword(c.t("cmd.mea.option"), Opts { options, default: Some(Def::Str(mode0)), ..Opts::default() }).await?;
    let mode = match k {
        Reply::Opt(k) if !k.is_empty() => k,
        Reply::Str(s) => s,
        _ => "I".into(),
    };
    c.st().last_poly_mode = Some(mode.clone());
    let circ = mode == "C";
    c.preview(Some(Rc::new(move |_, cur| vec![Entity::new(polygon_g(center, cur, count, circ))])));
    let p = c.must_point(c.t("cmd.circle.specify_radius_circle"), based(center)).await?;
    c.add(ent(&c, polygon_g(center, p, count, circ)))?;
    Ok(())
}

// ---- ELLIPSE

pub async fn ellipse(c: Ctx) -> R {
    let a =
        c.point(c.t("cmd.ellipse.specify_axis_endpoint_ellipse"), Opts { options: vec![c.opt("C", "Center")], ..Opts::default() }).await?;
    let (center, major) = if a.option() == Some("C") {
        let center = c.must_point(c.t("cmd.ellipse.specify_center_ellipse"), Opts::default()).await?;
        let e = c.must_point(c.t("cmd.ellipse.specify_endpoint_axis"), based(center)).await?;
        (center, P2::new(e.x - center.x, e.y - center.y))
    } else {
        let Some(a) = a.point() else { return Ok(()) };
        let b = c.must_point(c.t("cmd.ellipse.specify_other_endpoint_axis"), based(a)).await?;
        let center = P2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
        (center, P2::new(b.x - center.x, b.y - center.y))
    };
    let l = js::hypot(major.x, major.y);
    if l < 1e-12 {
        return Ok(());
    }
    let make = move |half: f64| {
        Geo::Ellipse(Ellipse {
            cx: center.x,
            cy: center.y,
            mx: major.x,
            my: major.y,
            ratio: js::min(js::max(half / l, 1e-6), 1.0),
            t0: 0.0,
            t1: TAU,
        })
    };
    c.preview(Some(Rc::new(move |_, cur| {
        vec![Entity::new(make((((cur.x - center.x) * -major.y + (cur.y - center.y) * major.x) / l).abs()))]
    })));
    let d = c.distance(c.t("cmd.ellipse.specify_distance_other_axis"), based(center)).await?;
    let Reply::Num(d) = d else { return Ok(()) };
    if d == 0.0 || d.is_nan() {
        return Ok(());
    }
    if d > l {
        // the other axis is the longer one: swap so the ratio stays ≤ 1
        let u = P2::new(-major.y / l * d, major.x / l * d);
        c.add(ent(&c, Geo::Ellipse(Ellipse { cx: center.x, cy: center.y, mx: u.x, my: u.y, ratio: l / d, t0: 0.0, t1: TAU })))?;
    } else {
        c.add(ent(&c, make(d)))?;
    }
    Ok(())
}

// ---- POINT

pub async fn point(c: Ctx) -> R {
    loop {
        let Reply::Point(p, _) = c.point(c.t("cmd.point.specify_point"), none()).await? else { return Ok(()) };
        c.add(ent(&c, Geo::Point(Point { x: p.x, y: p.y })))?;
    }
}

// ---- TEXT

pub async fn text(c: Ctx) -> R {
    let p = c.point(c.t("cmd.attdef.at"), Opts { options: vec![c.opt("J", "Justify")], ..Opts::default() }).await?;
    let (mut ha, mut va) = ("left", "baseline");
    let at = if p.option() == Some("J") {
        let options =
            vec![c.opt("L", "Left"), c.opt("C", "Center"), c.opt("R", "Right"), c.opt("M", "Middle"), c.opt("TL", "TL"), c.opt("MC", "MC")];
        let j = c.keyword(c.t("cmd.mea.option"), Opts { options, default: Some(Def::Str("L".into())), ..Opts::default() }).await?;
        let k = match &j {
            Reply::Opt(k) if !k.is_empty() => k.as_str(),
            _ => "L",
        };
        ha = match k {
            "C" | "M" | "MC" => "center",
            "R" => "right",
            _ => "left",
        };
        va = match k {
            "M" | "MC" => "middle",
            "TL" => "top",
            _ => "baseline",
        };
        c.must_point(c.t("cmd.text.specify_insertion_point_text"), Opts::default()).await?
    } else {
        match p.point() {
            Some(q) => q,
            None => return Ok(()),
        }
    };
    let th = c.doc().current.text_height;
    let h = c.distance(c.t("cmd.mtext.height"), Opts { base: Some(at), default: Some(Def::Num(th)), ..Opts::default() }).await?;
    let Reply::Num(h) = h else { return Ok(()) };
    if h == 0.0 || h.is_nan() {
        return Ok(());
    }
    c.sh().doc.borrow_mut().current.text_height = h;
    let u = ang_of(active_ucs(&c.doc()));
    let rot = c.angle(c.t("cmd.attdef.rot"), Opts { base: Some(at), default: Some(Def::Num(u)), ucs: true, ..Opts::default() }).await?;
    let rot = match rot {
        Reply::Num(r) if r != 0.0 && !r.is_nan() => r,
        _ => 0.0,
    };
    let mut y = 0.0;
    loop {
        let line = match c.string(c.t("cmd.text.enter_text"), Opts::default()).await? {
            Reply::Str(s) if !s.is_empty() => s,
            _ => return Ok(()),
        };
        let pos = polar(at, rot - PI / 2.0, y);
        let t = Text {
            x: pos.x,
            y: pos.y,
            h,
            s: line,
            rot: Some(norm_angle(rot)),
            ha: Some(ha.into()),
            va: Some(va.into()),
            wf: Some(1.0),
            ww: None,
            ls: None,
        };
        c.add(ent(&c, Geo::Text(t)))?;
        y += h * 1.667; // DTEXT line spacing
    }
}
