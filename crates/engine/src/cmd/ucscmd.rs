//! UCS, PLAN, TURNLEFT / TURNRIGHT / TURNWORLD (commands.js over ucs.js), LWDISPLAY and POLARANG (the host's
//! settings), COUNT (layertools.js over count.js).
//!
//! A UCS is not an edit (no undo step, no unsaved mark): the Doc keeps the file's `{ current, named }` and the engine
//! keeps UCS Previous for the session (doc.js `ucsPrev`). Setting one asks the host to turn the view so its X axis is
//! level (`host.turnView`).

use std::rc::Rc;

use cc_cad::Doc;
use cc_cad::entity::{Entity, Geo};
use cc_cad::js;
use cc_cad_geom::geom::{EPS, PI, deg, dist, norm_angle, prim_param, prim_point, prims};
use cc_cad_geom::jsmath::JsMath;
use cc_geom::P2;
use serde_json::{Value, json};

use crate::cmd::arrange::fmt_len;
use crate::cmd::draw::seg;
use crate::engine::{Ctx, Opts, R, Reply};
use crate::input::{Def, fmt};
use crate::ucs::{Ucs, active_ucs, clean_ucs, to_world};

/// `UCS_NAME_RE`: `^[^<>/\\":;?*|,=`]{1,255}$`
pub fn ucs_name_ok(n: &str) -> bool {
    let l: usize = n.chars().map(char::len_utf16).sum();
    (1..=255).contains(&l) && !n.chars().any(|c| "<>/\\\":;?*|,=`".contains(c))
}

/// `ucsData(v)`: a UCS record as data (the world included), or None.
fn ucs_data(v: &Value) -> Option<Ucs> {
    let f = |k: &str| js::to_number_opt(v.get(k));
    let (x, y, ang) = (f("x"), f("y"), f("ang"));
    ([x, y, ang].iter().all(|n| n.is_finite()) && x.abs() <= 1e15 && y.abs() <= 1e15).then(|| Ucs { x, y, ang: norm_angle(ang) })
}

/// The doc's current UCS (doc.js `doc.ucs`, cleaned).
pub fn current_ucs(doc: &Doc) -> Option<Ucs> {
    clean_ucs(doc.ucs.as_ref().and_then(|u| u.get("current")))
}

/// The saved UCSs (doc.js `ucsNamed`: the file's list, cleaned as load cleans it).
pub fn named_ucs(doc: &Doc) -> Vec<(String, Ucs)> {
    let list = doc.ucs.as_ref().and_then(|u| u.get("named")).and_then(Value::as_array).cloned().unwrap_or_default();
    list.iter()
        .filter_map(|u| {
            let name = u.get("name")?.as_str().filter(|n| ucs_name_ok(n))?;
            Some((name.to_string(), ucs_data(u)?))
        })
        .take(1000)
        .collect()
}

fn ucs_value(u: Option<Ucs>) -> Value {
    u.map_or(Value::Null, |u| json!({ "x": cc_cad::entity::num(u.x), "y": cc_cad::entity::num(u.y), "ang": cc_cad::entity::num(u.ang) }))
}

fn write_ucs(doc: &mut Doc, current: Option<Ucs>, named: &[(String, Ucs)]) {
    let named: Vec<Value> = named
        .iter()
        .map(|(n, u)| json!({ "name": n, "x": cc_cad::entity::num(u.x), "y": cc_cad::entity::num(u.y), "ang": cc_cad::entity::num(u.ang) }))
        .collect();
    doc.ucs = Some(json!({ "current": ucs_value(current), "named": named }));
}

/// `threePoint(o, px)`: the origin and a point on the positive X axis → a UCS (Some(None) = the world), or None.
fn three_point(o: P2, px: P2) -> Option<Option<Ucs>> {
    if dist(o, px) < EPS {
        return None;
    }
    let v = json!({ "x": o.x, "y": o.y, "ang": (px.y - o.y).jatan2(px.x - o.x) });
    Some(clean_ucs(Some(&v)))
}

fn mk(x: f64, y: f64, ang: f64) -> Option<Ucs> {
    clean_ucs(Some(&json!({ "x": x, "y": y, "ang": ang })))
}

/// `fromObject(e, pick, blocks)`: UCS Object → Some(the UCS; None = the world), or None when the object gives none.
pub fn from_object(e: &Entity, pick: Option<P2>, blocks: &cc_cad_geom::geom::Blocks) -> Option<Option<Ucs>> {
    match &e.geo {
        Geo::Line(l) => {
            let (a, b) = (P2::new(l.x1, l.y1), P2::new(l.x2, l.y2));
            if dist(a, b) < EPS {
                return None;
            }
            Some(if pick.is_some_and(|p| dist(p, b) < dist(p, a)) {
                mk(b.x, b.y, (a.y - b.y).jatan2(a.x - b.x))
            } else {
                mk(a.x, a.y, (b.y - a.y).jatan2(b.x - a.x))
            })
        }
        Geo::Circle(ci) => Some(mk(ci.cx, ci.cy, pick.map_or(0.0, |p| (p.y - ci.cy).jatan2(p.x - ci.cx)))),
        Geo::Arc(a) => Some(mk(a.cx, a.cy, a.a0)),
        Geo::Poly(_) => {
            let ps = prims(e, Some(blocks));
            let first = ps.first()?;
            let mut best = first;
            if let Some(p) = pick {
                let mut bd = f64::INFINITY;
                for q in &ps {
                    let d = dist(prim_point(q, prim_param(q, p, true)), p);
                    if d < bd {
                        bd = d;
                        best = q;
                    }
                }
            }
            let s = prim_point(best, if best.rev() { 1.0 } else { 0.0 });
            let t = prim_point(best, if best.rev() { 0.999 } else { 0.001 });
            Some(mk(s.x, s.y, (t.y - s.y).jatan2(t.x - s.x)))
        }
        Geo::Text(t) => Some(mk(t.x, t.y, js::or(t.rot, 0.0))),
        Geo::Insert(i) => Some(mk(i.x, i.y, js::or(i.rot, 0.0))),
        Geo::Point(p) => Some(mk(p.x, p.y, 0.0)),
        Geo::Xline(i) | Geo::Ray(i) => Some(mk(i.x, i.y, i.dy.jatan2(i.dx))),
        Geo::Dim(d) => d.p1.map(|p| mk(p.x, p.y, js::or(d.rot, 0.0))),
        _ => None,
    }
}

/// `setUcs(doc, u, { remember })`: make u current (the old one goes on the Previous stack, at most 10).
fn set_ucs(c: &Ctx, u: Option<Ucs>, remember: bool) -> Option<Ucs> {
    let next = u.and_then(|u| mk(u.x, u.y, u.ang));
    let (old, named) = {
        let d = c.doc();
        (current_ucs(&d), named_ucs(&d))
    };
    if remember {
        let mut st = c.st();
        st.memo.ucs_prev.push(old);
        if st.memo.ucs_prev.len() > 10 {
            st.memo.ucs_prev.remove(0);
        }
    }
    write_ucs(&mut c.sh().doc.borrow_mut(), next, &named);
    next
}

fn ucs_label(c: &Ctx, u: Option<Ucs>) -> String {
    match u {
        Some(u) => c.tv("cmd.ucs.is", &[("at", format!("{}, {}", fmt_len(c, u.x), fmt_len(c, u.y))), ("ang", fmt(deg(norm_angle(u.ang))))]),
        None => c.t("cmd.ucs.world"),
    }
}

/// `afterUcs`: the host turns the view to the UCS, and the line says where it is.
fn after_ucs(c: &Ctx) {
    let u = current_ucs(&c.doc());
    {
        let mut h = c.sh().host.borrow_mut();
        if h.has_turn_view() {
            h.turn_view(u.map_or(0.0, |u| u.ang));
        }
    }
    c.print(&ucs_label(c, u));
}

pub async fn ucs(c: Ctx) -> R {
    if !c.doc().in_model() {
        c.print(&c.t("cmd.ucs.sheet"));
        return Ok(());
    }
    let cur = current_ucs(&c.doc());
    let options = vec![
        c.opt("NA", "NAmed"),
        c.opt("OB", "OBject"),
        c.opt("P", "Previous"),
        c.opt("W", "World"),
        c.opt("Z", "Z"),
        c.opt("3", "3point"),
    ];
    let r = c.point(c.t("cmd.ucs.origin"), Opts { options, default: Some(Def::Str("World".into())), ..Opts::default() }).await?;
    let key = match &r {
        Reply::Str(s) if s == "World" => "W".to_string(),
        Reply::Opt(k) => k.clone(),
        _ => String::new(),
    };
    if key.is_empty() {
        let Reply::Point(o, _) = r else { return Ok(()) };
        c.preview(Some(Rc::new(move |_, p| vec![seg(o, p)])));
        let px = c.point(c.t("cmd.ucs.xaxis"), Opts { base: Some(o), none: true, ..Opts::default() }).await?;
        let u = match px.point().filter(|p| dist(o, *p) > 1e-12) {
            Some(p) => three_point(o, p).flatten(),
            None => mk(o.x, o.y, cur.map_or(0.0, |u| u.ang)),
        };
        set_ucs(&c, u, true);
        after_ucs(&c);
        return Ok(());
    }
    match key.as_str() {
        "W" => {
            set_ucs(&c, None, true);
            after_ucs(&c);
        }
        "P" => {
            let prev = c.st().memo.ucs_prev.pop();
            let Some(prev) = prev else {
                c.print(&c.t("cmd.ucs.no_previous"));
                return Ok(());
            };
            set_ucs(&c, prev, false);
            after_ucs(&c);
        }
        "Z" => {
            let a = c.angle(c.t("cmd.ucs.zangle"), Opts { default: Some(Def::Num(PI / 2.0)), ..Opts::default() }).await?;
            let a = if let Reply::Num(a) = a { a } else { PI / 2.0 };
            let (x, y, ang) = cur.map_or((0.0, 0.0, 0.0), |u| (u.x, u.y, u.ang));
            set_ucs(&c, Some(Ucs { x, y, ang: ang + a }), true);
            after_ucs(&c);
        }
        "3" => {
            let o0 = to_world(cur, P2::new(0.0, 0.0));
            let o = c.point(c.t("cmd.ucs.p_origin"), Opts { none: true, ..Opts::default() }).await?.point().unwrap_or(o0);
            c.preview(Some(Rc::new(move |_, p| vec![seg(o, p)])));
            let px = c.point(c.t("cmd.ucs.p_x"), Opts { base: Some(o), none: true, ..Opts::default() }).await?;
            let u = match px.point() {
                Some(p) => match three_point(o, p) {
                    Some(u) => u,
                    None => {
                        c.print(&c.t("cmd.ucs.same_point"));
                        return Ok(());
                    }
                },
                None => mk(o.x, o.y, cur.map_or(0.0, |u| u.ang)),
            };
            // in a flat drawing the Y side is the usual one: read, not used
            c.point(c.t("cmd.ucs.p_y"), Opts { base: Some(o), none: true, ..Opts::default() }).await?;
            set_ucs(&c, u, true);
            after_ucs(&c);
        }
        "OB" => loop {
            let pick = c.entity(c.t("cmd.ucs.object"), Opts::default()).await?;
            let Reply::Pick { id, at, .. } = pick else { continue };
            let u = {
                let d = c.doc();
                d.get(id).and_then(|e| from_object(e, Some(at), &d.blocks))
            };
            let Some(u) = u else {
                c.print(&c.t("cmd.ucs.object_no"));
                continue;
            };
            set_ucs(&c, u, true);
            after_ucs(&c);
            return Ok(());
        },
        "NA" => {
            let options = vec![c.opt("R", "Restore"), c.opt("S", "Save"), c.opt("D", "Delete"), c.opt("?", "?")];
            let k = c.keyword(c.t("cmd.ucs.named"), Opts { options, default: Some(Def::Str("Restore".into())), ..Opts::default() }).await?;
            let kk = match &k {
                Reply::Str(s) if s == "Restore" => "R".to_string(),
                Reply::Opt(o) => o.clone(),
                _ => String::new(),
            };
            let mut list = named_ucs(&c.doc());
            if kk == "?" {
                let msg = if list.is_empty() {
                    c.t("cmd.ucs.none_saved")
                } else {
                    list.iter().map(|(n, u)| format!("{n}: {}", ucs_label(&c, mk(u.x, u.y, u.ang)))).collect::<Vec<_>>().join("\n")
                };
                c.print(&msg);
                return Ok(());
            }
            let ask = match kk.as_str() {
                "S" => "cmd.ucs.save_as",
                "D" => "cmd.ucs.delete_which",
                _ => "cmd.ucs.restore_which",
            };
            let name = match c.string(c.t(ask), Opts::default()).await? {
                Reply::Str(s) => js::trim(&s).to_string(),
                _ => String::new(),
            };
            if name.is_empty() {
                return Ok(());
            }
            let i = list.iter().position(|(n, _)| n.to_uppercase() == js::trim(&name).to_uppercase());
            if kk == "S" {
                if !ucs_name_ok(&name) {
                    c.print(&c.t("cmd.ucs.bad_name"));
                    return Ok(());
                }
                let rec = cur.unwrap_or(Ucs { x: 0.0, y: 0.0, ang: 0.0 });
                let rec = Ucs { ang: norm_angle(rec.ang), ..rec };
                match i {
                    Some(i) => {
                        if let Some(slot) = list.get_mut(i) {
                            *slot = (name.clone(), rec);
                        }
                    }
                    None => list.push((name.clone(), rec)),
                }
                let cur_now = current_ucs(&c.doc());
                write_ucs(&mut c.sh().doc.borrow_mut(), cur_now, &list);
                c.print(&c.tv("cmd.ucs.saved", &[("name", name)]));
                return Ok(());
            }
            let Some(i) = i else {
                c.print(&c.tv("cmd.ucs.no_such", &[("name", name)]));
                return Ok(());
            };
            if kk == "D" {
                list.remove(i);
                let cur_now = current_ucs(&c.doc());
                write_ucs(&mut c.sh().doc.borrow_mut(), cur_now, &list);
                c.print(&c.tv("cmd.ucs.deleted", &[("name", name)]));
                return Ok(());
            }
            let u = list.get(i).map(|(_, u)| *u);
            set_ucs(&c, u, true);
            after_ucs(&c);
        }
        _ => {}
    }
    Ok(())
}

/// TURNLEFT (-1), TURNRIGHT (1), TURNWORLD (0): UCS Z ∓90 (or World), and the view follows.
pub async fn quarter_turn(c: Ctx, sign: f64) -> R {
    if !c.doc().in_model() {
        c.print(&c.t("cmd.ucs.sheet"));
        return Ok(());
    }
    let cur = current_ucs(&c.doc());
    let u = if sign == 0.0 {
        None
    } else {
        let (x, y, ang) = cur.map_or((0.0, 0.0, 0.0), |u| (u.x, u.y, u.ang));
        Some(Ucs { x, y, ang: ang + sign * PI / 2.0 })
    };
    set_ucs(&c, u, true);
    after_ucs(&c);
    Ok(())
}

pub async fn plan(c: Ctx) -> R {
    let options = vec![c.opt("C", "Current ucs"), c.opt("U", "Ucs"), c.opt("W", "World")];
    let k = c.keyword(c.t("cmd.plan.ask"), Opts { options, default: Some(Def::Str("Current ucs".into())), ..Opts::default() }).await?;
    let kk = match &k {
        Reply::Str(s) if s == "Current ucs" => "C".to_string(),
        Reply::Opt(o) => o.clone(),
        _ => String::new(),
    };
    let mut ang = 0.0;
    if kk == "C" {
        ang = active_ucs(&c.doc()).map_or(0.0, |u| u.ang);
        if ang.is_nan() {
            ang = 0.0;
        }
    }
    if kk == "U" {
        let name = match c.string(c.t("cmd.ucs.restore_which"), Opts::default()).await? {
            Reply::Str(s) => js::trim(&s).to_string(),
            _ => String::new(),
        };
        let rec = named_ucs(&c.doc()).into_iter().find(|(n, _)| n.to_uppercase() == name.to_uppercase());
        let Some((_, u)) = rec else {
            c.print(&c.tv("cmd.ucs.no_such", &[("name", name)]));
            return Ok(());
        };
        ang = u.ang;
    }
    if !c.sh().host.borrow().has_turn_view() {
        c.print(&c.t("cmd.plan.browser"));
        return Ok(());
    }
    c.sh().host.borrow_mut().turn_view(ang);
    let msg = c.tv(if ang != 0.0 { "cmd.plan.turned" } else { "cmd.plan.world" }, &[("ang", fmt(deg(norm_angle(ang))))]);
    c.print(&msg);
    Ok(())
}

pub async fn lwdisplay(c: Ctx) -> R {
    let cur = c.sh().host.borrow().lwt();
    let Some(cur) = cur else {
        c.print(&c.t("cmd.lwdisplay.browser"));
        return Ok(());
    };
    let options = vec![c.opt("ON", "ON"), c.opt("OFF", "OFF")];
    let def = if cur { "ON" } else { "OFF" };
    let r = c.keyword(c.t("cmd.lwdisplay.ask"), Opts { options, default: Some(Def::Str(def.into())), ..Opts::default() }).await?;
    let k = match r {
        Reply::Str(s) | Reply::Opt(s) => s,
        _ => String::new(),
    };
    if k == "ON" || k == "OFF" {
        c.sh().host.borrow_mut().set_toggle("lwt", k == "ON");
    }
    Ok(())
}

pub async fn polarang(c: Ctx) -> R {
    let cur = c.sh().host.borrow().polar_inc();
    let Some(cur) = cur else {
        c.print(&c.t("cmd.polarang.browser"));
        return Ok(());
    };
    let v = c.number(c.t("cmd.polarang.ask"), Opts { default: Some(Def::Num(cur)), ..Opts::default() }).await?;
    let v = if let Reply::Num(v) = v { v } else { f64::NAN };
    if !(v > 0.0 && v <= 180.0) {
        c.print(&c.t("cmd.polarang.bad"));
        return Ok(());
    }
    c.sh().host.borrow_mut().set_polar_inc(v);
    c.print(&c.tv("cmd.polarang.set", &[("v", fmt(v))]));
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------- COUNT

/// `countBlocks(doc, entities)` → [(block, ids)] most first, then by name (unnamed *blocks left out).
pub fn count_blocks<'a>(entities: impl Iterator<Item = &'a Entity>) -> Vec<(String, Vec<u64>)> {
    let mut by: Vec<(String, Vec<u64>)> = Vec::new();
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for e in entities {
        let Geo::Insert(i) = &e.geo else { continue };
        if i.block.is_empty() || i.block.starts_with('*') {
            continue;
        }
        let k = match index.get(&i.block) {
            Some(k) => *k,
            None => {
                by.push((i.block.clone(), Vec::new()));
                let k = by.len().saturating_sub(1);
                index.insert(i.block.clone(), k);
                k
            }
        };
        if let (Some(slot), Some(id)) = (by.get_mut(k), e.id) {
            slot.1.push(id);
        }
    }
    by.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| crate::cmd::qselect::locale_cmp(&a.0, &b.0)));
    by
}

/// count.js `cell(v)`: a leading ' for = + - @ (formula injection), quoted when it holds " , CR or LF.
fn push_cell(out: &mut String, v: &str) {
    let lead = v.starts_with(['=', '+', '-', '@']);
    if v.contains(['"', ',', '\r', '\n']) {
        out.push('"');
        if lead {
            out.push('\'');
        }
        for ch in v.chars() {
            if ch == '"' {
                out.push_str("\"\"");
            } else {
                out.push(ch);
            }
        }
        out.push('"');
    } else {
        if lead {
            out.push('\'');
        }
        out.push_str(v);
    }
}
fn csv(rows: &[Vec<String>]) -> String {
    let mut out = String::with_capacity(rows.len().saturating_mul(64));
    for (i, r) in rows.iter().enumerate() {
        if i > 0 {
            out.push_str("\r\n");
        }
        for (j, c) in r.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            push_cell(&mut out, c);
        }
    }
    out.push_str("\r\n");
    out
}
/// `+v.toFixed(6)` as text ('' for a non-number).
fn num6(v: f64) -> String {
    if !v.is_finite() {
        return String::new();
    }
    // the usual case without the exact decimal expansion: below 1e9 a value whose shortest form has at most six
    // decimals rounds to itself (its exact value is within half an ulp, far under 0.5e-6)
    if v.abs() < 1e9 {
        let short = js::num_to_string(v);
        if !short.contains('e') && short.split_once('.').is_none_or(|(_, f)| f.len() <= 6) {
            return short;
        }
    }
    // below 1e6 the product v·1e6 is off by far less than 1e-3 from the exact one: round it unless it sits near a tie
    if v.abs() < 1e6 {
        let p = v * 1e6;
        let f = p - p.floor();
        if (f - 0.5).abs() > 1e-3 {
            // toFixed rounds half away from zero on the exact value; away from ties that is round()
            return js::num_to_string(p.round() / 1e6);
        }
    }
    js::num_to_string(js::str_to_number(&js::to_fixed(v, 6)))
}

/// `countsCsv(doc)`: Block, In the model, On sheets, Total.
pub fn counts_csv(doc: &Doc, t: &dyn Fn(&str) -> String) -> String {
    let model = count_blocks(doc.model());
    let sheets = count_blocks(doc.layouts.iter().flat_map(|l| doc.space_entities(l.space)));
    let mut names: Vec<String> = Vec::new();
    for (n, _) in model.iter().chain(sheets.iter()) {
        if !names.contains(n) {
            names.push(n.clone());
        }
    }
    let get = |list: &[(String, Vec<u64>)], n: &str| list.iter().find(|(b, _)| b == n).map_or(0, |(_, ids)| ids.len());
    let mut rows: Vec<(String, usize, usize, usize)> = names
        .iter()
        .map(|n| {
            let (m, s) = (get(&model, n), get(&sheets, n));
            (n.clone(), m, s, m.saturating_add(s))
        })
        .collect();
    // String(a).localeCompare(String(b)): no numeric option here
    rows.sort_by(|a, b| b.3.cmp(&a.3).then_with(|| crate::cmd::qselect::locale_cmp_plain(&a.0, &b.0)));
    let mut out = vec![vec![t("count.col.block"), t("count.col.model"), t("count.col.sheets"), t("count.col.total")]];
    out.extend(rows.into_iter().map(|(n, m, s, tot)| vec![n, m.to_string(), s.to_string(), tot.to_string()]));
    csv(&out)
}

/// `attributesCsv(doc)`: one row per reference (model, then sheets) with every attribute tag as a column.
pub fn attributes_csv(doc: &Doc, t: &dyn Fn(&str) -> String) -> String {
    let mut refs: Vec<(&Entity, String)> = Vec::new();
    let named = |e: &Entity| matches!(&e.geo, Geo::Insert(i) if !i.block.is_empty() && !i.block.starts_with('*'));
    for e in doc.model() {
        if named(e) {
            refs.push((e, t("count.model")));
        }
    }
    for l in &doc.layouts {
        for e in doc.space_entities(l.space) {
            if named(e) {
                refs.push((e, l.name.clone()));
            }
        }
    }
    let mut tags: Vec<String> = Vec::new();
    let mut have: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for (e, _) in &refs {
        if let Geo::Insert(i) = &e.geo
            && let Some(a) = &i.attrs
        {
            for k in cc_cad_geom::geom::js_keys(a) {
                if have.insert(k.as_str()) {
                    tags.push(k.clone());
                }
            }
        }
    }
    let mut head = vec![t("count.col.block"), t("count.col.space"), t("count.col.layer"), "X".into(), "Y".into(), t("count.col.rotation")];
    head.extend(tags.iter().cloned());
    let mut rows = vec![head];
    for (e, space) in &refs {
        let Geo::Insert(i) = &e.geo else { continue };
        let mut r = vec![
            i.block.clone(),
            space.clone(),
            e.layer.clone().unwrap_or_default(),
            num6(i.x),
            num6(i.y),
            num6(js::or(i.rot, 0.0) * 180.0 / PI),
        ];
        for k in &tags {
            r.push(i.attrs.as_ref().and_then(|a| a.get(k.as_str())).filter(|v| !v.is_null()).map(js::to_js_string).unwrap_or_default());
        }
        rows.push(r);
    }
    csv(&rows)
}

pub async fn count(c: Ctx) -> R {
    let here = |c: &Ctx| count_blocks(c.doc().entities());
    let list = |c: &Ctx| {
        let rows = here(c);
        if rows.is_empty() {
            c.print(&c.t("count.none"));
            return;
        }
        let total: usize = rows.iter().map(|r| r.1.len()).sum();
        c.print(&c.tv("count.list", &[("n", total.to_string()), ("blocks", rows.len().to_string())]));
        for (b, ids) in rows.iter().take(40) {
            c.print(&format!("  {b}: {}", ids.len()));
        }
        if rows.len() > 40 {
            c.print(&format!("  {}", c.tv("count.more", &[("n", (rows.len().saturating_sub(40)).to_string())])));
        }
    };
    loop {
        let options = vec![c.opt("A", &c.t("opt.all")), c.opt("E", &c.t("opt.export"))];
        let p = c.entity(c.t("count.prompt"), Opts { none: true, options, ..Opts::default() }).await?;
        let id = match &p {
            Reply::Null => {
                list(&c);
                return Ok(());
            }
            Reply::Opt(o) if o == "A" => {
                list(&c);
                return Ok(());
            }
            Reply::Opt(o) if o == "E" => {
                let options = vec![c.opt("C", &c.t("opt.counts")), c.opt("A", &c.t("opt.attributes"))];
                let k = c.keyword(c.t("count.export"), Opts { options, default: Some(Def::Str("C".into())), ..Opts::default() }).await?;
                let kind = match k {
                    Reply::Opt(o) | Reply::Str(o) => o,
                    _ => return Ok(()),
                };
                if !c.sh().host.borrow().can_save_blob() {
                    c.print(&c.t("count.nodownload"));
                    return Ok(());
                }
                let base = c.doc().meta.get("name").filter(|v| js::truthy(Some(v))).map_or_else(|| "Drawing1".into(), js::to_js_string);
                let name = format!("{base}{}.csv", if kind == "A" { " attributes" } else { " block counts" });
                let text = {
                    let lang = c.sh().lang.clone();
                    let t = |k: &str| lang.s(k);
                    let d = c.doc();
                    format!("\u{feff}{}", if kind == "A" { attributes_csv(&d, &t) } else { counts_csv(&d, &t) })
                };
                c.sh().host.borrow_mut().save_blob(&name, text.as_bytes(), "text/csv");
                c.print(&c.tv("count.saved", &[("name", name)]));
                return Ok(());
            }
            Reply::Pick { id, .. } => *id,
            _ => continue,
        };
        let block = c.doc().get(id).and_then(|e| match &e.geo {
            Geo::Insert(i) if !i.block.is_empty() && !i.block.starts_with('*') => Some(i.block.clone()),
            _ => None,
        });
        let Some(block) = block else {
            c.print(&c.t("count.notblock"));
            continue;
        };
        let mine = here(&c).into_iter().find(|(b, _)| *b == block).map(|(_, ids)| ids).unwrap_or_default();
        let all = c.doc().every_entity().filter(|x| matches!(&x.geo, Geo::Insert(i) if i.block == block)).count();
        c.set_selection(&mine);
        c.print(&c.tv("count.one", &[("block", block), ("n", mine.len().to_string()), ("all", all.to_string())]));
        return Ok(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ucs_from_objects_and_names() {
        let blocks = cc_cad_geom::geom::Blocks::new();
        let line = Entity::from_value(&json!({ "type": "line", "x1": 0, "y1": 0, "x2": 10, "y2": 0 }));
        let u = from_object(&line, Some(P2::new(9.0, 0.0)), &blocks).expect("a line gives a UCS").expect("not the world");
        assert!((u.x - 10.0).abs() < 1e-12 && (u.ang - PI).abs() < 1e-12, "from the end nearer the pick, along the line");
        let hatch = Entity::from_value(&json!({ "type": "hatch", "loops": [[0, 0, 1, 0, 1, 1]] }));
        assert!(from_object(&hatch, None, &blocks).is_none());
        assert!(ucs_name_ok("Plan 2") && !ucs_name_ok("a,b") && !ucs_name_ok(""));
    }

    #[test]
    fn six_decimals_as_to_fixed() {
        for v in [0.0, -0.0, 1.0, 1.2345675, 89.99999999, -12.3456789, 999_999.123_456_7, 2.5e-7, -2.5e-7, 1e12 + 0.123_456_7, 0.1 + 0.2] {
            assert_eq!(num6(v), js::num_to_string(js::str_to_number(&js::to_fixed(v, 6))), "{v}");
        }
    }

    #[test]
    fn count_csv_escapes_and_sorts() {
        let mut d = Doc::new();
        d.load(&json!({
            "entities": [
                { "type": "insert", "block": "B", "x": 0, "y": 0, "attrs": { "N": "=1" } },
                { "type": "insert", "block": "B", "x": 1.23456789, "y": 0, "attrs": { "N": "a, \"b\"" } },
                { "type": "insert", "block": "A", "x": 0, "y": 0 },
                { "type": "insert", "block": "*U1", "x": 0, "y": 0 }
            ],
            "blocks": [{ "name": "A", "entities": [] }, { "name": "B", "entities": [] }, { "name": "*U1", "entities": [] }]
        }));
        let t = |k: &str| k.to_string();
        let counts = counts_csv(&d, &t);
        assert_eq!(counts, "count.col.block,count.col.model,count.col.sheets,count.col.total\r\nB,2,0,2\r\nA,1,0,1\r\n");
        let attrs = attributes_csv(&d, &t);
        assert!(attrs.contains("B,count.model,0,0,0,0,'=1\r\n") && attrs.contains("1.234568,0,0,\"a, \"\"b\"\"\""), "{attrs}");
    }
}
