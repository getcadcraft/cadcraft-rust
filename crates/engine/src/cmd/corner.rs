//! Corners (commands.js): FILLET and CHAMFER (geom.js filletPair / chamferPair / polyCorners) and EXPLODE.

use std::rc::Rc;

use cc_cad::entity::{Entity, Geo};
use cc_cad_geom::corners::{
    ChamferSpec, Corner, CornerSpec, Current, chamfer_pair, corner_target, explode_entity, fillet_pair, poly_corners,
};
use cc_cad_geom::geom::{deg, norm_angle};
use cc_geom::P2;
use serde_json::Value;

use crate::cmd::modify::undo_inside;
use crate::engine::{Ctx, Opts, R, Reply, Shared};
use crate::input::{Def, Opt, fmt};

/// CHAMFER's settings (`E.chamfer`).
#[derive(Clone, Debug, PartialEq)]
pub struct ChamferMemo {
    pub method: String,
    pub d1: f64,
    pub d2: f64,
    pub len: f64,
    pub ang: f64,
}
impl ChamferMemo {
    fn spec(&self) -> ChamferSpec {
        ChamferSpec { method: Some(self.method.clone()), d1: Some(self.d1), d2: Some(self.d2), len: Some(self.len), ang: Some(self.ang) }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Fillet,
    Chamfer,
}

fn corner_error(kind: Kind, code: &str) -> &'static str {
    let k = match (kind, code) {
        (Kind::Fillet, "type") => Some("cmd.fillet.err.type"),
        (Kind::Fillet, "large") => Some("cmd.fillet.err.large"),
        (Kind::Fillet, "none") => Some("cmd.fillet.err.none"),
        (Kind::Fillet, "parallel") => Some("cmd.chamfer.err.parallel"),
        (Kind::Chamfer, "type") => Some("cmd.chamfer.err.type"),
        (Kind::Chamfer, "large") => Some("cmd.chamfer.err.large"),
        (Kind::Chamfer, "none" | "parallel") => Some("cmd.chamfer.err.parallel"),
        (Kind::Chamfer, "angle") => Some("cmd.chamfer.err.angle"),
        _ => None,
    };
    k.unwrap_or(match code {
        "same" => "cmd.corner.err.same",
        "collinear" => "cmd.corner.err.collinear",
        "middle" => "cmd.corner.err.middle",
        _ => "cmd.corner.err.none",
    })
}

fn trim_on(sh: &Shared) -> bool {
    sh.st.borrow().memo.corner_trim != Some(false)
}
fn trim_label(c: &Ctx) -> String {
    c.t(if trim_on(c.sh()) { "cmd.corner.mode.trim" } else { "cmd.corner.mode.notrim" })
}
fn current_of(sh: &Shared) -> Current {
    let d = sh.doc.borrow();
    Current { layer: Some(d.current.layer.clone()), color: Some(d.current.color.clone()) }
}

/// A corner pick: the object (when it can take a corner: `target`), where it was picked and Shift.
struct Pick {
    e: Entity,
    at: P2,
    target: bool,
    shift: bool,
}
enum Got {
    Null,
    Opt(String),
    Pick(Box<Pick>),
}

/// `cornerPick`: locked objects ask again.
async fn corner_pick(c: &Ctx, prompt: &str, options: Vec<Opt>, none: bool) -> R<Got> {
    loop {
        let a = c.entity(prompt.into(), Opts { options: options.clone(), none, ..Opts::default() }).await?;
        let (id, at, shift) = match a {
            Reply::Opt(k) => return Ok(Got::Opt(k)),
            Reply::Pick { id, at, shift } => (id, at, shift),
            _ => return Ok(Got::Null),
        };
        let Some(e) = c.doc().get(id).cloned() else { return Ok(Got::Null) };
        if !c.doc().editable(&e) {
            c.print(&c.t("cmd.corner.err.locked"));
            continue;
        }
        let target = corner_target(&e, at).is_some();
        return Ok(Got::Pick(Box::new(Pick { e, at, target, shift })));
    }
}

/// `applyCorner` → true when the corner was made (or there was nothing to do).
fn apply_corner(c: &Ctx, kind: Kind, r: &Corner, label: &str) -> R<bool> {
    if let Some(code) = r.error {
        c.print(&c.t(corner_error(kind, code)));
        return Ok(false);
    }
    if r.edits.is_empty() && r.add.is_empty() {
        c.print(&c.t("cmd.corner.nothing"));
        return Ok(true);
    }
    c.change(label, |d| {
        for e in &r.edits {
            d.update(e.id.unwrap_or(0), e.clone())?;
        }
        for e in &r.add {
            d.add(e.clone())?;
        }
        Ok(())
    })?;
    Ok(true)
}

type MakeCorner = Rc<dyn Fn(&Shared, &Entity, P2, Option<&Entity>, P2) -> Corner>;

/// `cornerPreview`: the corner with the object under the cursor as the second pick.
fn corner_preview(c: &Ctx, first: &Pick, make: MakeCorner) {
    let (fe, fat) = (first.e.clone(), first.at);
    c.preview(Some(Rc::new(move |sh, cur| {
        let e = {
            let d = sh.doc.borrow();
            let id = sh.host.borrow().pick_at(&d, cur);
            id.and_then(|id| d.get(id).cloned()).filter(|e| d.editable(e))
        };
        let Some(e) = e else { return Vec::new() };
        let r = make(sh, &fe, fat, Some(&e), cur);
        if r.error.is_none() { r.edits.into_iter().chain(r.add).collect() } else { Vec::new() }
    })));
}

fn fillet_make() -> MakeCorner {
    Rc::new(|sh, a, aat, b, bat| {
        let r = sh.st.borrow().memo.fillet_r.unwrap_or(0.0);
        let (ta, tb) = (corner_target(a, aat), b.and_then(|b| corner_target(b, bat)));
        fillet_pair(ta.as_ref(), tb.as_ref(), r, trim_on(sh), Some(&current_of(sh)))
    })
}
fn chamfer_make() -> MakeCorner {
    Rc::new(|sh, a, aat, b, bat| {
        let s = chamfer_memo_of(sh);
        let (ta, tb) = (corner_target(a, aat), b.and_then(|b| corner_target(b, bat)));
        chamfer_pair(ta.as_ref(), tb.as_ref(), &s.spec(), trim_on(sh), Some(&current_of(sh)))
    })
}

async fn ask_trim_mode(c: &Ctx) -> R {
    let options = vec![c.opt("T", &c.t("opt.trim")), c.opt("N", &c.t("opt.notrim"))];
    let def = if trim_on(c.sh()) { "T" } else { "N" };
    let k = c.keyword(c.t("cmd.corner.trim"), Opts { options, default: Some(Def::Str(def.into())), ..Opts::default() }).await?;
    let v = match &k {
        Reply::Opt(s) | Reply::Str(s) => s.as_str() != "N",
        _ => true,
    };
    c.st().memo.corner_trim = Some(v);
    Ok(())
}

async fn ask_radius(c: &Ctx) -> R {
    let cur = c.st().memo.fillet_r.unwrap_or(0.0);
    let r = c.distance(c.t("cmd.fillet.radius"), Opts { default: Some(Def::Num(cur)), ..Opts::default() }).await?;
    if let Reply::Num(v) = r {
        c.st().memo.fillet_r = Some(v);
    }
    Ok(())
}

/// The P option: every corner of one polyline.
async fn poly_corner_cmd(c: &Ctx, kind: Kind) -> R {
    loop {
        let options = match kind {
            Kind::Fillet => vec![c.opt("R", &c.t("opt.radius"))],
            Kind::Chamfer => chamfer_set_opts(c),
        };
        let p = corner_pick(c, &c.t("cmd.corner.poly"), options, false).await?;
        let e = match p {
            Got::Opt(k) => {
                match kind {
                    Kind::Fillet => ask_radius(c).await?,
                    Kind::Chamfer => ask_chamfer_opt(c, &k).await?,
                }
                continue;
            }
            Got::Pick(p) => p.e.clone(),
            Got::Null => return Ok(()),
        };
        if !matches!(e.geo, Geo::Poly(_)) {
            c.print(&c.t("cmd.corner.notpoly"));
            continue;
        }
        let spec = match kind {
            Kind::Fillet => CornerSpec::Fillet(c.st().memo.fillet_r.unwrap_or(0.0)),
            Kind::Chamfer => CornerSpec::Chamfer(chamfer_memo_of(c.sh()).spec()),
        };
        let r = poly_corners(&e, &spec);
        if r.done > 0 || r.removed > 0 {
            let id = e.id.unwrap_or(0);
            let ne = r.e.clone();
            c.change(if kind == Kind::Fillet { "FILLET" } else { "CHAMFER" }, |d| d.update(id, ne))?;
        }
        if r.done > 0 {
            let key = if kind == Kind::Fillet { "cmd.fillet.poly.done" } else { "cmd.chamfer.poly.done" };
            c.print(&c.tv(key, &[("n", r.done.to_string())]));
        } else {
            c.print(&c.t("cmd.corner.poly.none"));
        }
        if r.short > 0 {
            c.print(&c.tv("cmd.corner.poly.short", &[("n", r.short.to_string())]));
        }
        if r.arcs > 0 {
            c.print(&c.tv("cmd.corner.poly.arcs", &[("n", r.arcs.to_string())]));
        }
        return Ok(());
    }
}

pub async fn fillet(c: Ctx) -> R {
    if c.st().memo.fillet_r.is_none() {
        c.st().memo.fillet_r = Some(0.0);
    }
    c.set_selection(&[]);
    let mut multiple = false;
    let r0 = c.st().memo.fillet_r.unwrap_or(0.0);
    c.print(&c.tv("cmd.fillet.settings", &[("mode", trim_label(&c)), ("r", fmt(r0))]));
    loop {
        let options = vec![
            c.opt("U", &c.t("opt.undo")),
            c.opt("P", &c.t("opt.polyline")),
            c.opt("R", &c.t("opt.radius")),
            c.opt("T", &c.t("opt.trim")),
            c.opt("M", &c.t("opt.multiple")),
        ];
        let a = match corner_pick(&c, &c.t("cmd.fillet.first"), options, true).await? {
            Got::Null => return Ok(()),
            Got::Opt(k) => {
                match k.as_str() {
                    "U" => undo_inside(&c),
                    "R" => ask_radius(&c).await?,
                    "T" => ask_trim_mode(&c).await?,
                    "M" => multiple = true,
                    "P" => {
                        poly_corner_cmd(&c, Kind::Fillet).await?;
                        if multiple {
                            continue;
                        }
                        return Ok(());
                    }
                    _ => {}
                }
                continue;
            }
            Got::Pick(p) => p,
        };
        if !a.target {
            c.print(&c.t("cmd.fillet.err.type"));
            continue;
        }
        c.set_selection(&[a.e.id.unwrap_or(0)]);
        corner_preview(&c, &a, fillet_make());
        let b = loop {
            match corner_pick(&c, &c.t("cmd.fillet.second"), vec![c.opt("R", &c.t("opt.radius"))], false).await? {
                Got::Opt(k) if k == "R" => ask_radius(&c).await?,
                Got::Pick(p) => break p,
                _ => return Ok(()),
            }
        };
        c.preview(None);
        c.set_selection(&[]);
        if !b.target {
            c.print(&c.t("cmd.fillet.err.type"));
            continue;
        }
        let rad = if b.shift { 0.0 } else { c.st().memo.fillet_r.unwrap_or(0.0) };
        let r = {
            let (ta, tb) = (corner_target(&a.e, a.at), corner_target(&b.e, b.at));
            fillet_pair(ta.as_ref(), tb.as_ref(), rad, trim_on(c.sh()), Some(&current_of(c.sh())))
        };
        if r.error.is_none() && r.semicircle {
            c.print(&c.t("cmd.fillet.parallel"));
        }
        if !apply_corner(&c, Kind::Fillet, &r, "FILLET")? {
            continue;
        }
        if !multiple {
            return Ok(());
        }
    }
}

fn chamfer_memo_of(sh: &Shared) -> ChamferMemo {
    sh.st.borrow().memo.chamfer.clone().unwrap_or(ChamferMemo { method: "D".into(), d1: 0.0, d2: 0.0, len: 0.0, ang: 0.0 })
}
fn set_chamfer(c: &Ctx, m: ChamferMemo) {
    c.st().memo.chamfer = Some(m);
}
fn chamfer_set_opts(c: &Ctx) -> Vec<Opt> {
    vec![c.opt("D", &c.t("opt.distance")), c.opt("A", &c.t("opt.angle")), c.opt("E", &c.t("opt.method"))]
}

async fn ask_chamfer_opt(c: &Ctx, o: &str) -> R {
    match o {
        "D" => {
            let s = chamfer_memo_of(c.sh());
            let d1 = c.distance(c.t("cmd.chamfer.d1"), Opts { default: Some(Def::Num(s.d1)), ..Opts::default() }).await?;
            if let Reply::Num(v) = d1 {
                let mut s = chamfer_memo_of(c.sh());
                s.d1 = v;
                s.d2 = v;
                set_chamfer(c, s);
            }
            let s = chamfer_memo_of(c.sh());
            let d2 = c.distance(c.t("cmd.chamfer.d2"), Opts { default: Some(Def::Num(s.d2)), ..Opts::default() }).await?;
            let mut s = chamfer_memo_of(c.sh());
            if let Reply::Num(v) = d2 {
                s.d2 = v;
            }
            s.method = "D".into();
            set_chamfer(c, s);
        }
        "A" => {
            let s = chamfer_memo_of(c.sh());
            let len = c.distance(c.t("cmd.chamfer.len"), Opts { default: Some(Def::Num(s.len)), ..Opts::default() }).await?;
            if let Reply::Num(v) = len {
                let mut s = chamfer_memo_of(c.sh());
                s.len = v;
                set_chamfer(c, s);
            }
            let s = chamfer_memo_of(c.sh());
            let ang = c.angle(c.t("cmd.chamfer.ang"), Opts { default: Some(Def::Num(s.ang)), ..Opts::default() }).await?;
            let mut s = chamfer_memo_of(c.sh());
            if let Reply::Num(v) = ang {
                s.ang = norm_angle(v);
            }
            s.method = "A".into();
            set_chamfer(c, s);
        }
        "E" => {
            let s = chamfer_memo_of(c.sh());
            let options = vec![c.opt("D", &c.t("opt.distance")), c.opt("A", &c.t("opt.angle"))];
            let k = c
                .keyword(c.t("cmd.chamfer.method"), Opts { options, default: Some(Def::Str(s.method.clone())), ..Opts::default() })
                .await?;
            let mut s = chamfer_memo_of(c.sh());
            s.method = match &k {
                Reply::Opt(x) | Reply::Str(x) if x == "A" => "A".into(),
                _ => "D".into(),
            };
            set_chamfer(c, s);
        }
        _ => {}
    }
    Ok(())
}

pub async fn chamfer(c: Ctx) -> R {
    let s = chamfer_memo_of(c.sh());
    set_chamfer(&c, s.clone());
    c.set_selection(&[]);
    let mut multiple = false;
    let line = if s.method == "A" {
        c.tv("cmd.chamfer.settings.angle", &[("mode", trim_label(&c)), ("len", fmt(s.len)), ("ang", fmt(deg(s.ang)))])
    } else {
        c.tv("cmd.chamfer.settings", &[("mode", trim_label(&c)), ("d1", fmt(s.d1)), ("d2", fmt(s.d2))])
    };
    c.print(&line);
    loop {
        let mut options = vec![c.opt("U", &c.t("opt.undo")), c.opt("P", &c.t("opt.polyline"))];
        options.extend(chamfer_set_opts(&c));
        options.extend([c.opt("T", &c.t("opt.trim")), c.opt("M", &c.t("opt.multiple"))]);
        let a = match corner_pick(&c, &c.t("cmd.chamfer.first"), options, true).await? {
            Got::Null => return Ok(()),
            Got::Opt(k) => {
                match k.as_str() {
                    "U" => undo_inside(&c),
                    "T" => ask_trim_mode(&c).await?,
                    "M" => multiple = true,
                    "P" => {
                        poly_corner_cmd(&c, Kind::Chamfer).await?;
                        if multiple {
                            continue;
                        }
                        return Ok(());
                    }
                    o => ask_chamfer_opt(&c, o).await?,
                }
                continue;
            }
            Got::Pick(p) => p,
        };
        let is_l = corner_target(&a.e, a.at).is_some_and(|t| t.prim.is_l());
        if !a.target || !is_l {
            c.print(&c.t("cmd.chamfer.err.type"));
            continue;
        }
        c.set_selection(&[a.e.id.unwrap_or(0)]);
        corner_preview(&c, &a, chamfer_make());
        let b = loop {
            match corner_pick(&c, &c.t("cmd.chamfer.second"), chamfer_set_opts(&c), false).await? {
                Got::Opt(k) => ask_chamfer_opt(&c, &k).await?,
                Got::Pick(p) => break p,
                Got::Null => return Ok(()),
            }
        };
        c.preview(None);
        c.set_selection(&[]);
        if !b.target {
            c.print(&c.t("cmd.chamfer.err.type"));
            continue;
        }
        let spec = if b.shift {
            ChamferSpec { method: Some("D".into()), d1: Some(0.0), d2: Some(0.0), len: None, ang: None }
        } else {
            chamfer_memo_of(c.sh()).spec()
        };
        let r = {
            let (ta, tb) = (corner_target(&a.e, a.at), corner_target(&b.e, b.at));
            chamfer_pair(ta.as_ref(), tb.as_ref(), &spec, trim_on(c.sh()), Some(&current_of(c.sh())))
        };
        if !apply_corner(&c, Kind::Chamfer, &r, "CHAMFER")? {
            continue;
        }
        if !multiple {
            return Ok(());
        }
    }
}

/// `linkOf` for other commands (BEDIT).
pub(crate) fn link_of_pub(c: &Ctx, name: &str) -> Option<String> {
    link_of(c, name)
}

/// `linkOf`: the linked drawing a block belongs to (its root's name), or None for an ordinary block.
fn link_of(c: &Ctx, name: &str) -> Option<String> {
    let d = c.doc();
    let b = d.blocks.get(&name.to_string())?;
    let x = b.xref.as_ref().filter(|x| cc_cad::js::truthy(Some(x)))?;
    match x.get("of") {
        None => Some(b.name.clone()),
        Some(of) => Some(match of {
            Value::String(s) => s.clone(),
            v => cc_cad::js::to_js_string(v),
        }),
    }
}

pub async fn explode(c: Ctx) -> R {
    let ids = c.select(None, false).await?;
    let mut n = 0usize;
    let mut linked: Vec<String> = Vec::new();
    let plan: Vec<(u64, Option<String>, Option<Vec<Entity>>)> = ids
        .0
        .iter()
        .map(|id| {
            let e = c.doc().get(*id).cloned();
            let link = match &e {
                Some(Entity { geo: Geo::Insert(i), .. }) => link_of(&c, &i.block),
                _ => None,
            };
            let parts = if link.is_some() { None } else { e.as_ref().and_then(|e| explode_entity(e, Some(&c.doc().blocks))) };
            (*id, link, parts)
        })
        .collect();
    c.change("EXPLODE", |d| {
        for (id, link, parts) in &plan {
            if let Some(l) = link {
                if !linked.contains(l) {
                    linked.push(l.clone());
                }
                continue;
            }
            let Some(parts) = parts else { continue };
            d.remove(*id)?;
            for p in parts {
                d.add(p.clone())?;
            }
            n = n.saturating_add(1);
        }
        Ok(())
    })?;
    c.set_selection(&[]);
    for name in &linked {
        c.print(&c.tv("cmd.xref.readonly", &[("name", name.clone())]));
    }
    if n > 0 || linked.is_empty() {
        c.print(&if n > 0 { format!("{n} exploded") } else { c.t("cmd.explode.only_polylines_block_references") });
    }
    Ok(())
}
