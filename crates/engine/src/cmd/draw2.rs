//! Draw, part 2 (commands.js): XLINE RAY MTEXT REVCLOUD MLEADER TEXTEDIT.

use std::cell::RefCell;
use std::rc::Rc;

use cc_cad::entity::{Color, Entity, Geo, Inf, Mleader, Obj, Poly, Text, Vertex, num};
use cc_cad::js;
use cc_cad_geom::edit::{path_of, revcloud};
use cc_cad_geom::geom::{dist, dist_to_prim, norm_angle, prims, unit_dir};
use cc_cad_geom::jsmath::JsMath;
use cc_geom::P2;

use crate::cmd::arrange::fmt_len;
use crate::cmd::draw::{ent, rect, seg};
use crate::engine::{Ctx, OffsetMemo, Opts, R, Reply};
use crate::input::Def;
use crate::ucs::{active_ucs, ang_of};

fn none() -> Opts {
    Opts { none: true, ..Opts::default() }
}

// ---- XLINE / RAY

type Dir = (f64, f64);

fn inf_geo(typ: &str, p: P2, u: Dir) -> Geo {
    let i = Inf { x: p.x, y: p.y, dx: u.0, dy: u.1 };
    if typ == "ray" { Geo::Ray(i) } else { Geo::Xline(i) }
}

/// `straightDir(e, at)`: the direction of a straight object (a polyline's straight segment nearest `at`).
fn straight_dir(e: &Entity, at: Option<P2>) -> Option<Dir> {
    match &e.geo {
        Geo::Line(l) => unit_dir(l.x2 - l.x1, l.y2 - l.y1),
        Geo::Xline(i) | Geo::Ray(i) => Some((i.dx, i.dy)),
        Geo::Poly(_) => {
            let mut best = None;
            let mut bd = f64::INFINITY;
            for q in prims(e, None) {
                if !q.is_l() {
                    continue;
                }
                let d = dist_to_prim(&q, at.unwrap_or(P2::new(q.x1, q.y1)));
                if d < bd {
                    bd = d;
                    best = Some(q);
                }
            }
            best.and_then(|q| unit_dir(q.x2 - q.x1, q.y2 - q.y1))
        }
        _ => None,
    }
}

async fn pick_straight(c: &Ctx, prompt: &str) -> R<Option<Dir>> {
    loop {
        let r = c.entity(prompt.into(), Opts::default()).await?;
        let Reply::Pick { id, at, .. } = r else { return Ok(None) };
        let u = c.doc().get(id).and_then(|e| straight_dir(e, Some(at)));
        if u.is_some() {
            return Ok(u);
        }
        c.print(&c.t("cmd.xline.not_straight"));
    }
}

type MakeInf = Rc<dyn Fn(P2) -> Option<(P2, Dir)>>;

/// `throughPoints`: through points until Enter, each one more construction line.
async fn through_points(c: &Ctx, typ: &'static str, make: MakeInf) -> R {
    loop {
        {
            let m = Rc::clone(&make);
            c.preview(Some(Rc::new(move |_, cur| m(cur).map(|(p, u)| Entity::new(inf_geo(typ, p, u))).into_iter().collect())));
        }
        let p = c.point(c.t("cmd.xline.through"), none()).await?;
        let Reply::Point(p, _) = p else { return Ok(()) };
        match make(p) {
            Some((q, u)) => {
                c.add(ent(c, inf_geo(typ, q, u)))?;
            }
            None => c.print(&c.t("cmd.xline.same_point")),
        }
    }
}

fn from_point(a: P2) -> MakeInf {
    Rc::new(move |p| unit_dir(p.x - a.x, p.y - a.y).map(|u| (a, u)))
}
fn fixed(u: Dir) -> MakeInf {
    Rc::new(move |p| Some((p, u)))
}

pub async fn xline(c: Ctx) -> R {
    let options = vec![c.opt("H", "Hor"), c.opt("V", "Ver"), c.opt("A", "Ang"), c.opt("B", "Bisect"), c.opt("O", "Offset")];
    let first = c.point(c.t("cmd.xline.first"), Opts { options, ..Opts::default() }).await?;
    let k = match first {
        Reply::Point(p, _) => return through_points(&c, "xline", from_point(p)).await,
        Reply::Opt(k) => k,
        _ => return Ok(()),
    };
    match k.as_str() {
        "H" => through_points(&c, "xline", fixed((1.0, 0.0))).await,
        "V" => through_points(&c, "xline", fixed((0.0, 1.0))).await,
        "A" => {
            let u = ang_of(active_ucs(&c.doc()));
            let a = c
                .angle(
                    c.t("cmd.xline.angle"),
                    Opts { options: vec![c.opt("R", "Reference")], default: Some(Def::Num(u)), ucs: true, ..Opts::default() },
                )
                .await?;
            let mut ang = if let Reply::Num(a) = a { a } else { 0.0 };
            if a.option() == Some("R") {
                let Some(u) = pick_straight(&c, &c.t("cmd.xline.ref_line")).await? else { return Ok(()) };
                let r = c.angle(c.t("cmd.xline.angle"), Opts { default: Some(Def::Num(0.0)), ..Opts::default() }).await?;
                ang = u.1.jatan2(u.0) + if let Reply::Num(r) = r { r } else { 0.0 };
            }
            through_points(&c, "xline", fixed((ang.jcos(), ang.jsin()))).await
        }
        "B" => {
            let v = c.must_point(c.t("cmd.xline.vertex"), Opts::default()).await?;
            let s = c.must_point(c.t("cmd.xline.start"), Opts { base: Some(v), ..Opts::default() }).await?;
            let Some(u0) = unit_dir(s.x - v.x, s.y - v.y) else {
                c.print(&c.t("cmd.xline.same_point"));
                return Ok(());
            };
            let half = move |p: P2| -> Option<(P2, Dir)> {
                let u1 = unit_dir(p.x - v.x, p.y - v.y)?;
                let b = unit_dir(u0.0 + u1.0, u0.1 + u1.1).unwrap_or((-u0.1, u0.0));
                Some((v, b))
            };
            loop {
                c.preview(Some(Rc::new(move |_, cur| half(cur).map(|(p, u)| Entity::new(inf_geo("xline", p, u))).into_iter().collect())));
                let p = c.point(c.t("cmd.xline.end"), Opts { base: Some(v), none: true, ..Opts::default() }).await?;
                let Reply::Point(p, _) = p else { return Ok(()) };
                match half(p) {
                    Some((q, u)) => {
                        c.add(ent(&c, inf_geo("xline", q, u)))?;
                    }
                    None => c.print(&c.t("cmd.xline.same_point")),
                }
            }
        }
        "O" => {
            let d0 = c.st().memo.xline_offset.clone();
            let default = match d0 {
                Some(OffsetMemo::Dist(d)) => Def::Num(d),
                _ => Def::Str("Through".into()),
            };
            let d = c
                .distance(c.t("cmd.xline.offset"), Opts { options: vec![c.opt("T", "Through")], default: Some(default), ..Opts::default() })
                .await?;
            let through = matches!(&d, Reply::Str(s) if s == "Through") || d.option() == Some("T");
            let dv = if let Reply::Num(n) = d { n } else { f64::NAN };
            if !through {
                c.st().memo.xline_offset = Some(OffsetMemo::Dist(dv));
            }
            loop {
                let pick = c.entity(c.t("cmd.xline.offset_line"), none()).await?;
                let Reply::Pick { id, at, .. } = pick else { return Ok(()) };
                let Some(e) = c.doc().get(id).cloned() else { continue };
                let Some(u) = straight_dir(&e, Some(at)) else {
                    c.print(&c.t("cmd.xline.not_straight"));
                    continue;
                };
                let on = match &e.geo {
                    Geo::Line(l) => P2::new(l.x1, l.y1),
                    Geo::Poly(_) => at,
                    Geo::Xline(i) | Geo::Ray(i) => P2::new(i.x, i.y),
                    _ => at,
                };
                let foot = move |q: P2| {
                    let k = (q.x - on.x) * u.0 + (q.y - on.y) * u.1;
                    P2::new(on.x + u.0 * k, on.y + u.1 * k)
                };
                if through {
                    c.preview(Some(Rc::new(move |_, cur| vec![Entity::new(inf_geo("xline", cur, u))])));
                    let p = c.must_point(c.t("cmd.xline.through"), Opts::default()).await?;
                    c.add(ent(&c, inf_geo("xline", p, u)))?;
                } else {
                    let f = foot(at);
                    c.preview(Some(Rc::new(move |_, cur| {
                        let g = foot(cur);
                        unit_dir(cur.x - g.x, cur.y - g.y)
                            .map(|n| Entity::new(inf_geo("xline", P2::new(f.x + n.0 * dv, f.y + n.1 * dv), u)))
                            .into_iter()
                            .collect()
                    })));
                    let side = c.must_point(c.t("cmd.xline.side"), Opts::default()).await?;
                    let g = foot(side);
                    let Some(n) = unit_dir(side.x - g.x, side.y - g.y) else {
                        c.print(&c.t("cmd.xline.side_on_line"));
                        continue;
                    };
                    c.add(ent(&c, inf_geo("xline", P2::new(f.x + n.0 * dv, f.y + n.1 * dv), u)))?;
                }
            }
        }
        _ => Ok(()),
    }
}

pub async fn ray(c: Ctx) -> R {
    let a = c.must_point(c.t("cmd.ray.start"), Opts::default()).await?;
    through_points(&c, "ray", from_point(a)).await
}

// ---- MTEXT

const MTEXT_JUST: [(&str, &str, &str); 9] = [
    ("TL", "left", "top"),
    ("TC", "center", "top"),
    ("TR", "right", "top"),
    ("ML", "left", "middle"),
    ("MC", "center", "middle"),
    ("MR", "right", "middle"),
    ("BL", "left", "bottom"),
    ("BC", "center", "bottom"),
    ("BR", "right", "bottom"),
];

/// `textWords`: lines typed at the command line (an empty line ends) → the text, or None.
async fn text_words(c: &Ctx) -> R<Option<String>> {
    let mut lines: Vec<String> = Vec::new();
    loop {
        let key = if lines.is_empty() { "cmd.mtext.enter" } else { "cmd.mtext.more" };
        let r = c.string(c.t(key), Opts::default()).await?;
        match r {
            Reply::Str(s) if !s.is_empty() => lines.push(s),
            _ => break,
        }
    }
    Ok(if lines.is_empty() { None } else { Some(lines.join("\n")) })
}

struct Frame {
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
}

pub async fn mtext(c: Ctx) -> R {
    let h0 = c.doc().current.text_height;
    let ls0 = c.st().memo.mtext_ls.unwrap_or(1.0);
    let rot0 = norm_angle(ang_of(active_ucs(&c.doc())));
    let just0 = c.st().memo.mtext_just.clone().unwrap_or_else(|| "TL".into());
    let (mut h, mut ls, mut rot, mut just, mut width): (f64, f64, f64, String, Option<f64>) = (h0, ls0, rot0, just0, None);
    let a = c.must_point(c.t("cmd.mtext.first"), Opts::default()).await?;
    let axes = |rot: f64| (P2::new(rot.jcos(), rot.jsin()), P2::new(-rot.jsin(), rot.jcos()));
    let frame = move |rot: f64, b: P2| {
        let (u, n) = axes(rot);
        let d = P2::new(b.x - a.x, b.y - a.y);
        let lx = d.x * u.x + d.y * u.y;
        let ly = d.x * n.x + d.y * n.y;
        Frame { left: js::min(0.0, lx), right: js::max(0.0, lx), top: js::max(0.0, ly), bottom: js::min(0.0, ly) }
    };
    let box_poly = move |rot: f64, f: &Frame| {
        let (u, n) = axes(rot);
        let pts = [(f.left, f.top), (f.right, f.top), (f.right, f.bottom), (f.left, f.bottom)]
            .iter()
            .map(|&(x, y)| Vertex::new(a.x + u.x * x + n.x * y, a.y + u.y * x + n.y * y, 0.0))
            .collect();
        Entity::new(Geo::Poly(Poly { pts, closed: Some(true) }))
    };
    let mut b: Option<P2> = None;
    loop {
        c.preview(Some(Rc::new(move |_, cur| vec![box_poly(rot, &frame(rot, cur))])));
        let options = vec![
            c.opt("H", &c.t("opt.height")),
            c.opt("J", &c.t("opt.justify")),
            c.opt("L", &c.t("opt.linespacing")),
            c.opt("R", &c.t("opt.rotation")),
            c.opt("W", &c.t("opt.width")),
        ];
        let p = c.point(c.t("cmd.mtext.corner"), Opts { base: Some(a), options, ..Opts::default() }).await?;
        match p {
            Reply::Opt(k) => match k.as_str() {
                "H" => {
                    let v =
                        c.distance(c.t("cmd.mtext.height"), Opts { base: Some(a), default: Some(Def::Num(h)), ..Opts::default() }).await?;
                    if let Reply::Num(v) = v
                        && v > 0.0
                    {
                        h = v;
                    }
                }
                "J" => {
                    let options = MTEXT_JUST.iter().map(|(k, _, _)| c.opt(k, k)).collect();
                    let k = c
                        .keyword(c.t("cmd.mtext.just"), Opts { options, default: Some(Def::Str(just.clone())), ..Opts::default() })
                        .await?;
                    if let Reply::Opt(key) | Reply::Str(key) = &k
                        && MTEXT_JUST.iter().any(|(j, _, _)| j == key)
                    {
                        just = key.clone();
                        c.st().memo.mtext_just = Some(key.clone());
                    }
                }
                "L" => {
                    let f = c.number(c.t("cmd.mtext.ls"), Opts { default: Some(Def::Num(ls)), ..Opts::default() }).await?;
                    match f {
                        Reply::Num(f) if (0.25..=4.0).contains(&f) => {
                            ls = f;
                            c.st().memo.mtext_ls = Some(f);
                        }
                        _ => c.print(&c.t("cmd.mtext.ls.bad")),
                    }
                }
                "R" => {
                    let r = c
                        .angle(c.t("cmd.mtext.rot"), Opts { base: Some(a), default: Some(Def::Num(rot)), ucs: true, ..Opts::default() })
                        .await?;
                    rot = norm_angle(match r {
                        Reply::Num(v) if v != 0.0 && !v.is_nan() => v,
                        _ => 0.0,
                    });
                }
                "W" => {
                    let w =
                        c.distance(c.t("cmd.mtext.width"), Opts { base: Some(a), default: Some(Def::Num(0.0)), ..Opts::default() }).await?;
                    width = Some(match w {
                        Reply::Num(v) if v != 0.0 && !v.is_nan() => v,
                        _ => 0.0,
                    });
                    break;
                }
                _ => {}
            },
            Reply::Point(q, _) => {
                b = Some(q);
                break;
            }
            _ => return Ok(()),
        }
    }
    c.preview(None);
    let (_, ha, va) = MTEXT_JUST.iter().find(|(j, _, _)| *j == just).copied().unwrap_or(("TL", "left", "top"));
    let mut at = a;
    let mut ww = width.unwrap_or(0.0);
    if let Some(b) = b {
        let f = frame(rot, b);
        ww = f.right - f.left;
        let lx = match ha {
            "left" => f.left,
            "right" => f.right,
            _ => (f.left + f.right) / 2.0,
        };
        let ly = match va {
            "top" => f.top,
            "bottom" => f.bottom,
            _ => (f.top + f.bottom) / 2.0,
        };
        let (u, n) = axes(rot);
        at = P2::new(a.x + u.x * lx + n.x * ly, a.y + u.y * lx + n.y * ly);
    }
    let Some(s) = text_words(&c).await? else {
        c.print(&c.t("cmd.mtext.empty"));
        return Ok(());
    };
    if js::trim(&s).is_empty() {
        c.print(&c.t("cmd.mtext.empty"));
        return Ok(());
    }
    let t = Text {
        x: at.x,
        y: at.y,
        h,
        s,
        rot: Some(rot),
        ha: Some(ha.into()),
        va: Some(va.into()),
        wf: Some(1.0),
        ww: (ww > 1e-9).then_some(ww),
        ls: (ls != 1.0 && ls != 0.0 && !ls.is_nan()).then_some(ls),
    };
    c.sh().doc.borrow_mut().current.text_height = h;
    c.add(ent(&c, Geo::Text(t)))?;
    Ok(())
}

// ---- REVCLOUD

/// REVCLOUD's settings (`E.revcloud`): arc length and type R / P / F.
#[derive(Clone, Debug, PartialEq)]
pub struct CloudMemo {
    pub arc: f64,
    pub typ: String,
}

fn cloud_memo(c: &Ctx) -> CloudMemo {
    if let Some(m) = c.st().memo.revcloud.clone() {
        return m;
    }
    let dim = c.doc().current.dim.clone();
    let g = |k: &str| dim.get(k).map_or(0.0, js::to_number);
    let txt = g("txt");
    let scale = g("scale");
    let txt = if txt == 0.0 || txt.is_nan() { 2.5 } else { txt };
    let scale = if scale == 0.0 || scale.is_nan() { 1.0 } else { scale };
    let m = CloudMemo { arc: txt * scale * 4.0, typ: "R".into() };
    c.st().memo.revcloud = Some(m.clone());
    m
}

fn type_key(t: &str) -> &'static str {
    match t {
        "P" => "opt.polygonal",
        "F" => "opt.freehand",
        _ => "opt.rectangular",
    }
}

fn rect_pts(a: P2, b: P2) -> Vec<P2> {
    vec![P2::new(a.x, a.y), P2::new(b.x, a.y), P2::new(b.x, b.y), P2::new(a.x, b.y)]
}

fn cloud(c: &Ctx, pts: &[P2], closed: bool, arc: f64) -> Entity {
    let e = revcloud(pts, closed, arc, false);
    ent(c, e.geo)
}

pub async fn revcloud_cmd(c: Ctx) -> R {
    let s = cloud_memo(&c);
    c.print(&c.tv("cmd.rc.now", &[("arc", fmt_len(&c, s.arc)), ("type", c.t(type_key(&s.typ)))]));
    loop {
        let options = vec![
            c.opt("A", &c.t("opt.arclength")),
            c.opt("O", &c.t("opt.object")),
            c.opt("R", &c.t("opt.rectangular")),
            c.opt("P", &c.t("opt.polygonal")),
            c.opt("F", &c.t("opt.freehand")),
        ];
        let p = c.point(c.t("cmd.rc.first"), Opts { options, ..Opts::default() }).await?;
        let p = match p {
            Reply::Opt(k) => {
                match k.as_str() {
                    "A" => {
                        let s = cloud_memo(&c);
                        let v = c.distance(c.t("cmd.rc.arc"), Opts { default: Some(Def::Num(s.arc)), ..Opts::default() }).await?;
                        if let Reply::Num(v) = v
                            && v > 0.0
                        {
                            c.st().memo.revcloud = Some(CloudMemo { arc: v, ..s });
                        }
                    }
                    "R" | "P" | "F" => {
                        let s = CloudMemo { typ: k.clone(), ..cloud_memo(&c) };
                        c.st().memo.revcloud = Some(s.clone());
                        c.print(&c.tv("cmd.rc.now", &[("arc", fmt_len(&c, s.arc)), ("type", c.t(type_key(&s.typ)))]));
                    }
                    "O" => loop {
                        let r = c.entity(c.t("cmd.rc.object"), Opts::default()).await?;
                        let Reply::Pick { id, .. } = r else { return Ok(()) };
                        let Some(e) = c.doc().get(id).cloned() else { continue };
                        let path = path_of(&e);
                        let Some((pts, closed)) = path.filter(|_| c.doc().editable(&e)) else {
                            c.print(&c.t("cmd.rc.notobject"));
                            continue;
                        };
                        c.set_selection(&[id]);
                        let options = vec![c.opt("Y", &c.t("opt.yes")), c.opt("N", &c.t("opt.no"))];
                        let k = c
                            .keyword(c.t("cmd.rc.reverse"), Opts { options, default: Some(Def::Str("N".into())), ..Opts::default() })
                            .await?;
                        c.set_selection(&[]);
                        let rev = matches!(&k, Reply::Opt(x) | Reply::Str(x) if x == "Y");
                        let s = cloud_memo(&c);
                        let mut made = revcloud(&pts, closed, s.arc, rev);
                        made.layer = e.layer.clone();
                        made.color = Some(Color::or_null(&e.color));
                        c.change("REVCLOUD", |d| {
                            d.remove(id)?;
                            d.add(made)
                        })?;
                        c.print(&c.t("cmd.rc.done"));
                        return Ok(());
                    },
                    _ => {}
                }
                continue;
            }
            Reply::Point(p, _) => p,
            _ => return Ok(()),
        };
        let s = cloud_memo(&c);
        let arc = s.arc;
        match s.typ.as_str() {
            "R" => {
                let cc = c.clone();
                c.preview(Some(Rc::new(move |_, cur| {
                    if (cur.x - p.x).abs() > 1e-9 && (cur.y - p.y).abs() > 1e-9 {
                        vec![cloud(&cc, &rect_pts(p, cur), true, arc)]
                    } else {
                        vec![rect(p, cur)]
                    }
                })));
                let q = c.must_point(c.t("cmd.rc.corner"), Opts { base: Some(p), ..Opts::default() }).await?;
                c.preview(None);
                if (q.x - p.x).abs() < 1e-9 || (q.y - p.y).abs() < 1e-9 {
                    c.print(&c.t("cmd.rc.flat"));
                    return Ok(());
                }
                c.add(cloud(&c, &rect_pts(p, q), true, arc))?;
            }
            "P" => {
                let mut pts = vec![p];
                loop {
                    {
                        let (snap, cc) = (pts.clone(), c.clone());
                        c.preview(Some(Rc::new(move |_, cur| {
                            if snap.len() >= 2 {
                                let mut v = snap.clone();
                                v.push(cur);
                                vec![cloud(&cc, &v, true, arc)]
                            } else {
                                snap.first().map(|f| seg(*f, cur)).into_iter().collect()
                            }
                        })));
                    }
                    let options = if pts.len() > 1 { vec![c.opt("U", &c.t("opt.undo"))] } else { Vec::new() };
                    let q = c.point(c.t("cmd.rc.next"), Opts { base: pts.last().copied(), none: true, options, ..Opts::default() }).await?;
                    match q {
                        Reply::Opt(k) if k == "U" => {
                            pts.pop();
                        }
                        Reply::Point(q, _) => {
                            if pts.last().is_none_or(|l| dist(q, *l) > 1e-12) {
                                pts.push(q);
                            }
                        }
                        _ => break,
                    }
                }
                c.preview(None);
                if pts.len() < 3 {
                    c.print(&c.t("cmd.area.few"));
                    return Ok(());
                }
                c.add(cloud(&c, &pts, true, arc))?;
            }
            _ => {
                // freehand: the path follows the pointer; a point is taken every arc length
                let pts = Rc::new(RefCell::new(vec![p]));
                {
                    let (pp, cc) = (Rc::clone(&pts), c.clone());
                    c.preview(Some(Rc::new(move |_, cur| {
                        let mut v = pp.borrow_mut();
                        if v.last().is_some_and(|l| dist(cur, *l) >= arc) {
                            v.push(cur);
                        }
                        if v.len() >= 2 {
                            let mut w = v.clone();
                            w.push(cur);
                            vec![cloud(&cc, &w, false, arc)]
                        } else {
                            Vec::new()
                        }
                    })));
                }
                let q = c.point(c.t("cmd.rc.freehand"), none()).await?;
                c.preview(None);
                if let Reply::Point(q, _) = q {
                    pts.borrow_mut().push(q);
                }
                let v = pts.borrow().clone();
                if v.len() < 3 {
                    c.print(&c.t("cmd.rc.short"));
                    return Ok(());
                }
                c.add(cloud(&c, &v, true, arc))?;
            }
        }
        c.print(&c.t("cmd.rc.done"));
        return Ok(());
    }
}

// ---- MLEADER

pub async fn mleader(c: Ctx) -> R {
    let dim = c.doc().current.dim.clone();
    let g = |k: &str| dim.get(k).map_or(f64::NAN, js::to_number);
    let k = {
        let s = g("scale");
        if s == 0.0 || s.is_nan() { 1.0 } else { s }
    };
    let (m_asz, m_dog) = {
        let st = c.st();
        (st.memo.mleader_asz, st.memo.mleader_dogleg)
    };
    let mut asz = m_asz.unwrap_or(g("asz") * k);
    let mut dogleg = m_dog.unwrap_or(g("asz") * k);
    let gap = match dim.get("gap") {
        None | Some(serde_json::Value::Null) => 0.625,
        Some(v) => js::to_number(v),
    } * k;
    let tip = loop {
        let p = c
            .point(
                c.t("cmd.mld.tip"),
                Opts { options: vec![c.opt("A", &c.t("opt.arrowsize")), c.opt("L", &c.t("opt.landing"))], ..Opts::default() },
            )
            .await?;
        match p {
            Reply::Opt(o) if o == "A" => {
                let v = c.distance(c.t("cmd.mld.asz"), Opts { default: Some(Def::Num(asz)), ..Opts::default() }).await?;
                if let Reply::Num(v) = v
                    && v >= 0.0
                {
                    asz = v;
                    c.st().memo.mleader_asz = Some(v);
                }
            }
            Reply::Opt(o) if o == "L" => {
                let v = c.distance(c.t("cmd.mld.dogleg"), Opts { default: Some(Def::Num(dogleg)), ..Opts::default() }).await?;
                if let Reply::Num(v) = v
                    && v >= 0.0
                {
                    dogleg = v;
                    c.st().memo.mleader_dogleg = Some(v);
                }
            }
            Reply::Point(p, _) => break p,
            _ => return Ok(()),
        }
    };
    let h = g("txt") * k;
    let mut st = Obj::new();
    st.insert("asz".into(), num(asz));
    st.insert("dogleg".into(), num(dogleg));
    st.insert("gap".into(), num(gap));
    let make = move |c: &Ctx, to: P2| {
        let l = Mleader {
            pts: vec![tip, to],
            dir: Some(if to.x >= tip.x { 1.0 } else { -1.0 }),
            s: Some(String::new()),
            h: Some(h),
            ww: None,
            ls: None,
            st: Some(st.clone()),
        };
        ent(c, Geo::Mleader(l))
    };
    {
        let (m, cc) = (make.clone(), c.clone());
        c.preview(Some(Rc::new(move |_, cur| if dist(tip, cur) > 1e-12 { vec![m(&cc, cur)] } else { Vec::new() })));
    }
    let land = c.must_point(c.t("cmd.mld.landing"), Opts { base: Some(tip), ..Opts::default() }).await?;
    c.preview(None);
    if dist(tip, land) < 1e-12 {
        c.print(&c.t("cmd.mld.same"));
        return Ok(());
    }
    let mut e = make(&c, land);
    let r = text_words(&c).await?;
    if let (Some(s), Geo::Mleader(l)) = (r, &mut e.geo) {
        l.s = Some(s);
    }
    let empty = matches!(&e.geo, Geo::Mleader(l) if js::trim(l.s.as_deref().unwrap_or("")).is_empty());
    c.add(e)?;
    if empty {
        c.print(&c.t("cmd.mld.notext"));
    }
    Ok(())
}

// ---- TEXTEDIT

fn is_texty(e: &Entity) -> bool {
    matches!(e.geo, Geo::Text(_) | Geo::Mleader(_))
}

pub async fn textedit(c: Ctx) -> R {
    let mut pre: std::collections::VecDeque<Entity> = {
        let d = c.doc();
        let sel = c.sh().host.borrow().selection();
        sel.iter().filter_map(|id| d.get(*id).cloned()).filter(|e| is_texty(e) && d.editable(e)).collect()
    };
    let once = !pre.is_empty();
    c.set_selection(&[]);
    loop {
        if once && pre.is_empty() {
            return Ok(());
        }
        let e = match pre.pop_front() {
            Some(e) => e,
            None => {
                let pick = c.entity(c.t("cmd.textedit.pick"), none()).await?;
                let Reply::Pick { id, .. } = pick else { return Ok(()) };
                let Some(e) = c.doc().get(id).cloned() else { continue };
                if !is_texty(&e) {
                    c.print(&c.t("cmd.textedit.only"));
                    continue;
                }
                e
            }
        };
        let id = e.id.unwrap_or(0);
        c.set_selection(&[id]);
        let cur_s = match &e.geo {
            Geo::Text(t) => Some(t.s.clone()),
            Geo::Mleader(l) => l.s.clone(),
            _ => None,
        };
        let v = c.string(c.t("cmd.textedit.enter"), Opts { default: cur_s.map(Def::Str), ..Opts::default() }).await?;
        let r = match v {
            Reply::Str(s) if !s.is_empty() => Some(s),
            _ => None,
        };
        c.set_selection(&[]);
        if let Some(s) = r.filter(|s| !js::trim(s).is_empty()) {
            let Some(mut next) = c.doc().get(id).cloned() else { continue };
            match &mut next.geo {
                Geo::Text(t) => {
                    t.s = s;
                    if !t.ww.is_some_and(|w| w > 1e-9) {
                        t.ww = None;
                    }
                    if t.ls.is_none_or(|l| l == 0.0 || l == 1.0 || l.is_nan()) {
                        t.ls = None;
                    }
                }
                Geo::Mleader(l) => {
                    l.s = Some(s);
                    if !l.ww.is_some_and(|w| w > 1e-9) {
                        l.ww = None;
                    }
                    if l.ls.is_none_or(|x| x == 0.0 || x == 1.0 || x.is_nan()) {
                        l.ls = None;
                    }
                }
                _ => {}
            }
            c.change("TEXTEDIT", |d| d.update(id, next))?;
        }
    }
}
