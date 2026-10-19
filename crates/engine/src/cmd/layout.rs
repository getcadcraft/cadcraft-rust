//! Sheets and viewports (commands.js, sheets.js): LAYOUT MVIEW VPORTS MSPACE PSPACE VPLAYER VIEWLABEL, and NAMEDVIEW.
//!
//! Papers, units and scales are cc-io-cad's `plot` port of sheets.js; `sheet_contents` (the new sheet's border, title
//! block and viewport) and `aim_viewport` are here. The browser keeps split panes and the viewport being worked in
//! (MSPACE) in the page: the host says whether it has them, as the JS host does.

use std::rc::Rc;

use cc_cad::Doc;
use cc_cad::doc::{Layout, View};
use cc_cad::entity::{Entity, Geo, Line, Obj, Poly, Text, Vertex, Viewport};
use cc_cad::js;
use cc_cad_geom::geom::Bounds;
use cc_cad_geom::pick::{empty_bounds, entity_bounds, grow_pt, valid_bounds};
use cc_geom::P2;
use cc_io_cad::plot::{PAPERS, make_paper, paper_by_key, paper_label, s_of, scale_label, scale_list, unit_mm};
use serde_json::{Value, json};

use crate::cmd::draw::{ent, rect};
use crate::engine::{Ctx, Opts, R, Reply};
use crate::input::{Def, hidden};

/// `bestScale(doc, paperUnit, sFit)` → s: the largest standard scale whose s is at most sFit.
pub fn best_scale(doc: &Doc, unit: &str, s_fit: f64) -> f64 {
    let mut best: Option<f64> = None;
    for x in scale_list(doc) {
        let s = s_of(doc, unit, x.n);
        if s <= s_fit * (1.0 + 1e-9) && best.is_none_or(|b| s > b) {
            best = Some(s);
        }
    }
    best.unwrap_or(s_fit)
}

/// `modelExtents(doc)`: bounds of the model's visible objects.
pub fn model_extents(doc: &Doc) -> Bounds {
    let mut b = empty_bounds();
    for e in doc.model() {
        if !doc.visible(e) {
            continue;
        }
        let eb = entity_bounds(e, Some(&doc.blocks));
        if valid_bounds(&eb) {
            grow_pt(&mut b, eb.min_x, eb.min_y);
            grow_pt(&mut b, eb.max_x, eb.max_y);
        }
    }
    b
}

/// `aimViewport(doc, paperUnit, w, h, win)` → (cx, cy, s): centre and the largest standard scale that fits.
pub fn aim_viewport(doc: &Doc, unit: &str, w: f64, h: f64, win: &Bounds) -> (f64, f64, f64) {
    if !valid_bounds(win) {
        return (0.0, 0.0, s_of(doc, unit, 1.0));
    }
    let bw = js::max(win.max_x - win.min_x, 1e-9);
    let bh = js::max(win.max_y - win.min_y, 1e-9);
    let s_fit = js::min(w / bw, h / bh);
    ((win.min_x + win.max_x) / 2.0, (win.min_y + win.max_y) / 2.0, best_scale(doc, unit, s_fit))
}

/// `defaultPaper(doc)`: Tabloid for imperial drawings, else ISO A3.
pub fn default_paper(doc: &Doc) -> &'static cc_io_cad::plot::PaperDef {
    let key = if cc_io_cad::plot::is_imperial(doc) { "ansi-b" } else { "iso-a3" };
    paper_by_key(key).unwrap_or(&PAPERS[1])
}

/// A layout's paper object { key, unit, w, h, x0, y0 }.
pub fn paper_obj(key: &str, portrait: bool) -> Obj {
    let p = make_paper(key, portrait);
    let mut o = Obj::new();
    o.insert("key".into(), Value::String(p.key));
    o.insert("unit".into(), Value::String(p.unit));
    o.insert("w".into(), cc_cad::entity::num(p.w));
    o.insert("h".into(), cc_cad::entity::num(p.h));
    o.insert("x0".into(), cc_cad::entity::num(p.x0));
    o.insert("y0".into(), cc_cad::entity::num(p.y0));
    o
}

fn pnum(p: &Obj, k: &str) -> f64 {
    p.get(k).map_or(f64::NAN, js::to_number)
}
fn punit(p: &Obj) -> String {
    p.get("unit").map(js::to_js_string).unwrap_or_default()
}

/// The ISO date (YYYY-MM-DD, UTC) of a time in ms since 1970 (`new Date(ms).toISOString().slice(0, 10)`).
pub fn iso_date(ms: f64) -> String {
    let days = (ms / 86_400_000.0).floor();
    // civil from days (Howard Hinnant's algorithm), in f64 to stay free of integer casts
    let z = days + 719_468.0;
    let era = (z / 146_097.0).floor();
    let doe = z - era * 146_097.0;
    let yoe = ((doe - (doe / 1460.0).floor() + (doe / 36524.0).floor() - (doe / 146_096.0).floor()) / 365.0).floor();
    let y = yoe + era * 400.0;
    let doy = doe - (365.0 * yoe + (yoe / 4.0).floor() - (yoe / 100.0).floor());
    let mp = ((5.0 * doy + 2.0) / 153.0).floor();
    let d = doy - ((153.0 * mp + 2.0) / 5.0).floor() + 1.0;
    let m = if mp < 10.0 { mp + 3.0 } else { mp - 9.0 };
    let y = if m <= 2.0 { y + 1.0 } else { y };
    format!("{y:04.0}-{m:02.0}-{d:02.0}")
}

/// `sheetContents(doc, paper, { title, sheetNo, date })`: a border, a title block in the lower right (layer "Title
/// block") and one viewport showing the whole model at a standard scale → the entities (in paper units).
pub fn sheet_contents(doc: &Doc, paper: &Obj, title: &str, sheet_no: usize, date: &str, title_layer: &str) -> Vec<Entity> {
    let unit = punit(paper);
    let k = 1.0 / unit_mm(&unit).unwrap_or(f64::NAN);
    let (pw, ph, px0, py0) = (pnum(paper, "w"), pnum(paper, "h"), pnum(paper, "x0"), pnum(paper, "y0"));
    let w = pw / k;
    let h = ph / k;
    let m = 10.0;
    let mut out: Vec<Entity> = Vec::new();
    let at = |x: f64, y: f64| P2::new(px0 + x * k, py0 + y * k);
    let lay = Some(title_layer.to_string());
    let with = |geo: Geo| Entity { layer: lay.clone(), color: Some(cc_cad::entity::Color::Null), ..Entity::new(geo) };
    let line = |out: &mut Vec<Entity>, x1: f64, y1: f64, x2: f64, y2: f64| {
        let (a, b) = (at(x1, y1), at(x2, y2));
        out.push(with(Geo::Line(Line { x1: a.x, y1: a.y, x2: b.x, y2: b.y })));
    };
    let rect = |out: &mut Vec<Entity>, x1: f64, y1: f64, x2: f64, y2: f64| {
        let pts = [at(x1, y1), at(x2, y1), at(x2, y2), at(x1, y2)].iter().map(|p| Vertex::new(p.x, p.y, 0.0)).collect();
        out.push(with(Geo::Poly(Poly { pts, closed: Some(true) })));
    };
    let text = |out: &mut Vec<Entity>, x: f64, y: f64, th: f64, s: &str| {
        let p = at(x, y);
        out.push(with(Geo::Text(Text {
            x: p.x,
            y: p.y,
            h: th * k,
            s: s.to_string(),
            rot: Some(0.0),
            ha: Some("left".into()),
            va: Some("baseline".into()),
            wf: Some(1.0),
            ww: None,
            ls: None,
        })));
    };
    rect(&mut out, m, m, w - m, h - m);
    let tw = js::min(180.0, w - 2.0 * m);
    let th = 32.0;
    let x0 = w - m - tw;
    let y0 = m;
    rect(&mut out, x0, y0, w - m, y0 + th);
    line(&mut out, x0, y0 + 14.0, w - m, y0 + 14.0);
    let cut = tw * 2.0 / 3.0;
    line(&mut out, x0 + cut, y0 + 14.0, x0 + cut, y0 + th);
    let cells = [0.0, tw / 3.0, tw / 2.0, tw * 3.0 / 4.0, tw];
    for c in cells.iter().skip(1).take(3) {
        line(&mut out, x0 + c, y0, x0 + c, y0 + 14.0);
    }
    // viewport: the space above the title block, inside the border
    let (vx0, vx1, vy0, vy1) = (m + 5.0, w - m - 5.0, m + th + 5.0, h - m - 5.0);
    let (vw, vh) = ((vx1 - vx0) * k, (vy1 - vy0) * k);
    let (cx, cy, s) = aim_viewport(doc, &unit, vw, vh, &model_extents(doc));
    let vc = at((vx0 + vx1) / 2.0, (vy0 + vy1) / 2.0);
    out.push(Entity {
        layer: Some("Viewports".into()),
        color: Some(cc_cad::entity::Color::Null),
        ..Entity::new(Geo::Viewport(Viewport { x: vc.x, y: vc.y, w: vw, h: vh, cx, cy, s, tw: None }))
    });
    text(&mut out, x0 + 2.0, y0 + th - 4.0, 1.8, "TITLE");
    text(&mut out, x0 + 2.0, y0 + 18.0, 5.0, title);
    text(&mut out, x0 + cut + 2.0, y0 + th - 4.0, 1.8, "DRAWN BY");
    let project = doc.meta.get("name").filter(|v| js::truthy(Some(v))).map(js::to_js_string).unwrap_or_default();
    let fields = [("PROJECT", project), ("SCALE", scale_label(doc, &unit, s)), ("DATE", date.to_string()), ("SHEET", sheet_no.to_string())];
    for (i, (label, value)) in fields.iter().enumerate() {
        let cx = cells.get(i).copied().unwrap_or(0.0);
        text(&mut out, x0 + cx + 2.0, y0 + 10.0, 1.8, label);
        text(&mut out, x0 + cx + 2.0, y0 + 3.5, 2.5, value);
    }
    // an empty value would be invisible and unpickable: drop it (the cell stays for writing in)
    out.retain(|e| !matches!(&e.geo, Geo::Text(t) if t.s.is_empty()));
    out
}

pub async fn layout(c: Ctx) -> R {
    let now = c.sh().host.borrow().now_ms();
    let lang = c.sh().lang.clone();
    let (name, label) = {
        let mut doc = c.sh().doc.borrow_mut();
        let mut n = doc.layouts.len().saturating_add(1);
        while doc.layouts.iter().any(|l| l.name == format!("Layout{n}")) {
            n = n.saturating_add(1);
        }
        let name = format!("Layout{n}");
        let p = default_paper(&doc);
        let paper = paper_obj(p.key, false);
        let title = doc.meta.get("name").filter(|v| js::truthy(Some(v))).map_or_else(|| "Drawing1".into(), js::to_js_string);
        let tl = lang.s("sheets.title_block");
        let ents = sheet_contents(&doc, &paper, &title, doc.layouts.len().saturating_add(1), &iso_date(now.unwrap_or(0.0)), &tl);
        for l in [json!({ "name": tl, "color": 7 }), json!({ "name": "Viewports", "color": 8, "plot": false })] {
            let ln = l.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            if !doc.layers.contains_key(ln.as_str()) {
                doc.add_layer(&l);
            }
        }
        let space = doc.new_space();
        let lay = Layout { name: name.clone(), paper, space };
        doc.change("LAYOUT", |d| {
            let mut list = d.layouts.clone();
            list.push(lay);
            d.set_layouts(list)?;
            for e in ents {
                d.add_in(e, space)?;
            }
            Ok(())
        })
        .map_err(crate::engine::doc_err)?;
        doc.set_space_id(space);
        (name, paper_label(p, &|k| lang.s(k)))
    };
    c.print(&c.tv("cmd.layout.made", &[("name", name), ("paper", label)]));
    Ok(())
}

pub async fn mview(c: Ctx) -> R {
    let unit = match c.doc().layout() {
        Some(l) => punit(&l.paper),
        None => {
            c.print(&c.t("cmd.mview.viewports_go_layout_open"));
            return Ok(());
        }
    };
    let a = c.must_point(c.t("cmd.mview.specify_corner_viewport"), Opts::default()).await?;
    c.preview(Some(Rc::new(move |_, cur| vec![rect(a, cur)])));
    let b = c.must_point(c.t("cmd.mtext.corner"), Opts { base: Some(a), ..Opts::default() }).await?;
    let (w, h) = ((b.x - a.x).abs(), (b.y - a.y).abs());
    if w < 1e-9 || h < 1e-9 {
        c.print(&c.t("cmd.mview.viewport_needs_width_height"));
        return Ok(());
    }
    let s = {
        let mut doc = c.sh().doc.borrow_mut();
        let (cx, cy, s) = aim_viewport(&doc, &unit, w, h, &model_extents(&doc));
        if !doc.layers.contains_key("Viewports") {
            doc.add_layer(&json!({ "name": "Viewports", "color": 8, "plot": false }));
        }
        let frz: Vec<Value> =
            doc.layers.values().filter(|l| l.new_vp_frozen == Some(true)).map(|l| Value::String(l.name.clone())).collect();
        let mut e = Entity {
            layer: Some("Viewports".into()),
            color: Some(cc_cad::entity::Color::Null),
            ..Entity::new(Geo::Viewport(Viewport { x: (a.x + b.x) / 2.0, y: (a.y + b.y) / 2.0, w, h, cx, cy, s, tw: None }))
        };
        if !frz.is_empty() {
            e.extra.insert("frz".into(), Value::Array(frz));
        }
        doc.change("MVIEW", |d| d.add(e)).map_err(crate::engine::doc_err)?;
        s
    };
    let label = scale_label(&c.doc(), &unit, s);
    c.print(&c.tv("cmd.mview.made", &[("scale", label)]));
    Ok(())
}

/// VPORTS: on a sheet it is MVIEW; in the model it splits the drawing area into panes (the host's).
pub async fn vports(c: Ctx) -> R {
    if c.doc().layout().is_some() {
        return mview(c).await;
    }
    if !c.sh().host.borrow().has_panes() {
        c.print(&c.t("cmd.vports.browser"));
        return Ok(());
    }
    let options = vec![
        c.opt("SI", &c.t("opt.single")),
        c.opt("2", "2"),
        c.opt("3", "3"),
        c.opt("4", "4"),
        hidden("2V", "2V"),
        hidden("2H", "2H"),
        hidden("3R", "3R"),
        hidden("3L", "3L"),
    ];
    let k = c.keyword(c.t("cmd.vports.ask"), Opts { options, default: Some(Def::Str("3".into())), ..Opts::default() }).await?;
    let key_of = |r: &Reply| match r {
        Reply::Opt(o) | Reply::Str(o) => o.to_uppercase(),
        _ => String::new(),
    };
    let mut key = key_of(&k);
    if key == "2" {
        let options = vec![c.opt("H", &c.t("opt.horizontal")), c.opt("V", &c.t("opt.vertical"))];
        let r = c.keyword(c.t("cmd.vports.two"), Opts { options, default: Some(Def::Str("V".into())), ..Opts::default() }).await?;
        let s = key_of(&r);
        key = format!("2{}", if s.is_empty() { "V".to_string() } else { s.chars().take(1).collect() });
    } else if key == "3" {
        let options = vec![c.opt("L", &c.t("opt.left")), c.opt("R", &c.t("opt.right"))];
        let r = c.keyword(c.t("cmd.vports.three"), Opts { options, default: Some(Def::Str("R".into())), ..Opts::default() }).await?;
        let s = key_of(&r);
        key = format!("3{}", if s.is_empty() { "R".to_string() } else { s.chars().take(1).collect() });
    }
    let layout = match key.as_str() {
        "SI" => "single",
        "2V" => "2v",
        "2H" => "2h",
        "3R" => "3r",
        "3L" => "3l",
        "4" => "4",
        _ => {
            c.print(&c.t("cmd.vports.which"));
            return Ok(());
        }
    };
    c.sh().host.borrow_mut().set_panes(layout);
    let n = match layout {
        "2v" | "2h" => 2,
        "3r" | "3l" => 3,
        _ => 4,
    };
    let msg = if layout == "single" { c.t("cmd.vports.single") } else { c.tv("cmd.vports.done", &[("n", n.to_string())]) };
    c.print(&msg);
    Ok(())
}

fn is_on_viewport(e: &Entity) -> bool {
    matches!(e.geo, Geo::Viewport(_)) && e.extra.get("on") != Some(&Value::Bool(false))
}

pub async fn mspace(c: Ctx) -> R {
    if !c.sh().host.borrow().has_viewports() {
        c.print(&c.t("cmd.mspace.browser"));
        return Ok(());
    }
    if c.sh().host.borrow().viewport_entered().is_some() {
        c.print(&c.t("cmd.mspace.already"));
        return Ok(());
    }
    if c.doc().layout().is_none() {
        c.print(&c.t("cmd.mspace.sheet_first"));
        return Ok(());
    }
    let (vps, sel): (Vec<u64>, Vec<u64>) = {
        let d = c.doc();
        (d.entities().filter(|e| is_on_viewport(e)).filter_map(|e| e.id).collect(), c.sh().host.borrow().selection())
    };
    if vps.is_empty() {
        c.print(&c.t("cmd.mspace.none"));
        return Ok(());
    }
    let mut vp = {
        let d = c.doc();
        sel.iter().copied().find(|id| d.get(*id).is_some_and(is_on_viewport)).or(if vps.len() == 1 { vps.first().copied() } else { None })
    };
    while vp.is_none() {
        let p = c.must_point(c.t("cmd.mspace.pick"), Opts::default()).await?;
        vp = {
            let d = c.doc();
            c.sh().host.borrow().viewport_at(&d, p).filter(|id| d.get(*id).is_some_and(is_on_viewport))
        };
        if vp.is_none() {
            c.print(&c.t("cmd.mspace.not_inside"));
        }
    }
    c.set_selection(&[]);
    if let Some(id) = vp {
        let space = c.doc().space;
        c.sh().host.borrow_mut().enter_viewport(space, id);
    }
    Ok(())
}

pub async fn pspace(c: Ctx) -> R {
    if c.sh().host.borrow().viewport_entered().is_none() {
        c.print(&c.t("cmd.pspace.not_inside"));
        return Ok(());
    }
    c.sh().host.borrow_mut().leave_viewport();
    c.print(&c.t("cmd.pspace.done"));
    Ok(())
}

/// `wildRe(pat)`: * any run, ? one character, case-insensitive, whole name.
fn wild_name(pat: &str, name: &str) -> bool {
    crate::cmd::qselect::wild_match(js::trim(pat), name)
}

/// `layerNames(c, text)`: the layers the comma-separated patterns name, pattern by pattern, without repeats.
fn layer_names(doc: &Doc, text: &str) -> Vec<String> {
    let names: Vec<&String> = doc.layers.keys().collect();
    let mut out: Vec<String> = Vec::new();
    for p in text.split(',').map(js::trim).filter(|p| !p.is_empty()) {
        for n in &names {
            if wild_name(p, n) && !out.contains(n) {
                out.push((*n).clone());
            }
        }
    }
    out
}

fn frz_of(e: &Entity) -> Vec<String> {
    e.extra.get("frz").and_then(Value::as_array).map(|a| a.iter().map(js::to_js_string).collect()).unwrap_or_default()
}

pub async fn vplayer(c: Ctx) -> R {
    let ev = c.sh().host.borrow().viewport_entered();
    let space = match ev {
        Some((s, _)) => Some(s),
        None => {
            let d = c.doc();
            d.layout().map(|_| d.space)
        }
    };
    let Some(space) = space else {
        c.print(&c.t("cmd.vplayer.sheet_first"));
        return Ok(());
    };
    let all_vps = |c: &Ctx| -> Vec<u64> {
        let d = c.doc();
        d.space_entities(space).filter(|e| matches!(e.geo, Geo::Viewport(_))).filter_map(|e| e.id).collect()
    };
    let in_map = |c: &Ctx, id: u64| -> Option<Entity> { c.doc().space_map(space).and_then(|m| m.get(&id)).cloned() };
    let current = |c: &Ctx| -> Option<u64> {
        if let Some((_, id)) = ev {
            return Some(id);
        }
        let sel = c.sh().host.borrow().selection();
        sel.into_iter().find(|id| in_map(c, *id).is_some_and(|e| matches!(e.geo, Geo::Viewport(_))))
    };
    loop {
        let options = vec![c.opt("?", "?"), c.opt("F", "Freeze"), c.opt("T", "Thaw"), c.opt("R", "Reset"), c.opt("V", "Vpvisdflt")];
        let k = c.keyword(c.t("cmd.vplayer.ask"), Opts { options, none: true, ..Opts::default() }).await?;
        let Reply::Opt(key) = k else { return Ok(()) };
        if key == "?" {
            let Some(vp) = current(&c).or_else(|| all_vps(&c).first().copied()).and_then(|id| in_map(&c, id)) else {
                c.print(&c.t("cmd.vplayer.none"));
                continue;
            };
            let frz = frz_of(&vp);
            let msg = if frz.is_empty() { c.t("cmd.vplayer.list_none") } else { c.tv("cmd.vplayer.list", &[("list", frz.join(", "))]) };
            c.print(&msg);
            continue;
        }
        let ask = match key.as_str() {
            "F" => "cmd.vplayer.names_freeze",
            "T" => "cmd.vplayer.names_thaw",
            "R" => "cmd.vplayer.names_reset",
            _ => "cmd.vplayer.names_dflt",
        };
        let raw = match c.string(c.t(ask), Opts::default()).await? {
            Reply::Str(s) => s,
            _ => String::new(),
        };
        let names = layer_names(&c.doc(), &raw);
        if names.is_empty() {
            c.print(&c.tv("cmd.vplayer.no_layer", &[("text", js::slice16(&raw, 60))]));
            continue;
        }
        if key == "V" {
            let options = vec![c.opt("F", "Frozen"), c.opt("T", "Thawed")];
            let f =
                c.keyword(c.t("cmd.vplayer.dflt"), Opts { options, default: Some(Def::Str("Thawed".into())), ..Opts::default() }).await?;
            let frozen = f.option() == Some("F");
            c.change("VPLAYER", |d| {
                let mut st = d.layer_state();
                for l in &mut st.list {
                    if names.contains(&l.name) {
                        l.new_vp_frozen = if frozen { Some(true) } else { None };
                    }
                }
                d.set_layer_state(&st)
            })?;
            c.print(&c.tv(if frozen { "cmd.vplayer.dflt_frozen" } else { "cmd.vplayer.dflt_thawed" }, &[("n", names.len().to_string())]));
            continue;
        }
        let options = vec![c.opt("A", "All"), c.opt("S", "Select"), c.opt("C", "Current")];
        let w = c.keyword(c.t("cmd.vplayer.where"), Opts { options, default: Some(Def::Str("Current".into())), ..Opts::default() }).await?;
        let wk = match &w {
            Reply::Str(s) if s == "Current" => "C".to_string(),
            Reply::Opt(o) => o.clone(),
            _ => String::new(),
        };
        let mut vps: Vec<u64> = Vec::new();
        if wk == "A" {
            vps = all_vps(&c);
        } else if wk == "S" {
            loop {
                let pick = c.entity(c.t("cmd.vplayer.pick"), Opts { none: true, ..Opts::default() }).await?;
                let Reply::Pick { id, .. } = pick else { break };
                if in_map(&c, id).is_some_and(|e| matches!(e.geo, Geo::Viewport(_))) {
                    if !vps.contains(&id) {
                        vps.push(id);
                    }
                } else {
                    c.print(&c.t("cmd.vpview.pick_viewport_border"));
                }
            }
        } else if let Some(v) = current(&c) {
            vps = vec![v];
        }
        if vps.is_empty() {
            c.print(&c.t("cmd.vplayer.none"));
            continue;
        }
        c.change("VPLAYER", |d| {
            for id in &vps {
                let Some(vp) = d.space_map(space).and_then(|m| m.get(id)).cloned() else { continue };
                let upper: Vec<String> = names.iter().map(|x| x.to_uppercase()).collect();
                let mut frz: Vec<String> = frz_of(&vp).into_iter().filter(|n| !upper.contains(&n.to_uppercase())).collect();
                if key == "F" {
                    frz.extend(names.iter().cloned());
                }
                if key == "R" {
                    frz.extend(names.iter().filter(|n| d.layers.get(n.as_str()).is_some_and(|l| l.new_vp_frozen == Some(true))).cloned());
                }
                let mut n = vp.clone();
                if frz.is_empty() {
                    n.extra.remove("frz");
                } else {
                    n.extra.insert("frz".into(), Value::Array(frz.into_iter().map(Value::String).collect()));
                }
                d.replace_in(*id, n, space)?;
            }
            Ok(())
        })?;
        let what = match key.as_str() {
            "F" => "freeze",
            "T" => "thaw",
            _ => "reset",
        };
        c.print(&c.tv(&format!("cmd.vplayer.done.{what}"), &[("n", names.len().to_string()), ("v", vps.len().to_string())]));
    }
}

/// `viewLabelText(vp)`: the viewport's view name, or "Unnamed view".
pub fn view_label_text(c: &Ctx, vp: &Entity) -> String {
    match vp.extra.get("vname").filter(|v| js::truthy(Some(v))) {
        Some(v) => js::to_js_string(v),
        None => c.t("field.vpview.none"),
    }
}

pub async fn viewlabel(c: Ctx) -> R {
    if c.doc().layout().is_none() {
        c.print(&c.t("cmd.viewlabel.sheet"));
        return Ok(());
    }
    let mut vp: Option<Entity> = {
        let d = c.doc();
        let sel = c.sh().host.borrow().selection();
        sel.iter().filter_map(|id| d.get(*id)).find(|e| matches!(e.geo, Geo::Viewport(_))).cloned()
    };
    while vp.is_none() {
        let pick = c.entity(c.t("cmd.vpview.select_viewport"), Opts::default()).await?;
        let Reply::Pick { id, .. } = pick else { continue };
        let e = c.doc().get(id).cloned();
        match e {
            Some(e) if matches!(e.geo, Geo::Viewport(_)) => vp = Some(e),
            _ => c.print(&c.t("cmd.vpview.pick_viewport_border")),
        }
    }
    let Some(vp) = vp else { return Ok(()) };
    c.set_selection(&[]);
    let h = {
        let th = c.doc().current.text_height;
        if th != 0.0 && !th.is_nan() { th } else { 2.5 }
    };
    let s = view_label_text(&c, &vp);
    let text_at = move |p: P2, s: &str| Text {
        x: p.x,
        y: p.y,
        h,
        s: s.to_string(),
        rot: Some(0.0),
        ha: Some("left".into()),
        va: Some("baseline".into()),
        wf: Some(1.0),
        ww: None,
        ls: None,
    };
    {
        let s = s.clone();
        c.preview(Some(Rc::new(move |_, cur| vec![Entity::new(Geo::Text(text_at(cur, &s)))])));
    }
    let at = c.point(c.t("cmd.viewlabel.where"), Opts { none: true, ..Opts::default() }).await?;
    let Geo::Viewport(v) = &vp.geo else { return Ok(()) };
    let p = at.point().unwrap_or_else(|| P2::new(v.x - v.w / 2.0, v.y - v.h / 2.0 - h * 1.6));
    let mut e = ent(&c, Geo::Text(text_at(p, &s)));
    let vid = vp.id.unwrap_or(0);
    e.extra.insert("field".into(), json!({ "k": "vpview", "vp": vid }));
    c.add(e)?;
    if !vp.extra.get("vname").is_some_and(|v| js::truthy(Some(v))) {
        c.print(&c.t("cmd.viewlabel.unnamed"));
    }
    Ok(())
}

pub async fn namedview(c: Ctx) -> R {
    let here: Option<String> = c.doc().layout().map(|l| l.name.clone());
    let mine = |c: &Ctx| -> Vec<View> { c.doc().views.iter().filter(|v| v.layout == here).cloned().collect() };
    let find = |c: &Ctx, name: &str| -> Option<View> { mine(c).into_iter().find(|v| v.name.to_lowercase() == name.to_lowercase()) };
    let options = vec![
        c.opt("?", &c.t("opt.list")),
        c.opt("R", &c.t("opt.restore")),
        c.opt("S", &c.t("opt.save")),
        c.opt("W", &c.t("opt.window")),
        c.opt("D", &c.t("opt.delete")),
    ];
    let k = c.keyword(c.t("cmd.nv.option"), Opts { options, default: Some(Def::Str("R".into())), ..Opts::default() }).await?;
    let o = match k {
        Reply::Opt(o) | Reply::Str(o) => o,
        _ => String::new(),
    };
    let fl = |c: &Ctx, n: f64| crate::cmd::arrange::fmt_len(c, n);
    if o == "?" {
        let list = mine(&c);
        let msg = if list.is_empty() {
            c.t("cmd.nv.none")
        } else {
            list.iter().map(|v| format!("{}  ({} × {})", v.name, fl(&c, v.w), fl(&c, v.h))).collect::<Vec<_>>().join("\n")
        };
        c.print(&msg);
        return Ok(());
    }
    let ask = match o.as_str() {
        "D" => "cmd.nv.delname",
        "R" => "cmd.nv.restname",
        _ => "cmd.nv.name",
    };
    let name = match c.string(c.t(ask), Opts::default()).await? {
        Reply::Str(s) => js::trim(&s).to_string(),
        _ => String::new(),
    };
    if name.is_empty() {
        return Ok(());
    }
    let list_names = |c: &Ctx| {
        let l = mine(c).iter().map(|x| x.name.clone()).collect::<Vec<_>>().join(", ");
        if l.is_empty() { c.t("cmd.nv.none") } else { l }
    };
    if o == "R" {
        let Some(v) = find(&c, &name) else {
            c.print(&c.tv("cmd.nv.unknown", &[("name", name.clone()), ("list", list_names(&c))]));
            return Ok(());
        };
        c.sh().host.borrow_mut().zoom_window(
            P2::new(v.cx - v.w / 2.0, v.cy - v.h / 2.0),
            P2::new(v.cx + v.w / 2.0, v.cy + v.h / 2.0),
            true,
        );
        c.print(&c.tv("cmd.nv.restored", &[("name", v.name)]));
        return Ok(());
    }
    if o == "D" {
        let Some(v) = find(&c, &name) else {
            c.print(&c.tv("cmd.nv.unknown", &[("name", name.clone()), ("list", list_names(&c))]));
            return Ok(());
        };
        c.change("NAMEDVIEW", |d| {
            let mut list = d.views.clone();
            if let Some(i) = list.iter().position(|x| *x == v) {
                list.remove(i);
            }
            d.set_views(list)
        })?;
        c.print(&c.tv("cmd.nv.deleted", &[("name", v.name)]));
        return Ok(());
    }
    if name.chars().map(char::len_utf16).sum::<usize>() > 255 || name.chars().any(|ch| "<>/\\\":;?*|=`".contains(ch)) {
        c.print(&c.t("cmd.nv.badname"));
        return Ok(());
    }
    let (cx, cy, w, h) = if o == "W" {
        let a = c.must_point(c.t("sel.window.first"), Opts::default()).await?;
        c.preview(Some(Rc::new(move |_, cur| vec![rect(a, cur)])));
        let b = c.must_point(c.t("sel.window.second"), Opts { base: Some(a), ..Opts::default() }).await?;
        c.preview(None);
        ((a.x + b.x) / 2.0, (a.y + b.y) / 2.0, (b.x - a.x).abs(), (b.y - a.y).abs())
    } else {
        let host = c.sh().host.borrow();
        let Some((w, h)) = host.view_rect() else {
            drop(host);
            c.print(&c.t("cmd.polarang.browser"));
            return Ok(());
        };
        let ctr = host.view_center().unwrap_or(P2::new(0.0, 0.0));
        (ctr.x, ctr.y, w, h)
    };
    if !(w > 0.0 && h > 0.0) {
        c.print(&c.t("cmd.nv.empty"));
        return Ok(());
    }
    let old = find(&c, &name);
    let v = View { name: old.as_ref().map_or_else(|| name.clone(), |x| x.name.clone()), cx, cy, w, h, layout: here.clone() };
    let vname = v.name.clone();
    c.change("NAMEDVIEW", |d| {
        let mut list = d.views.clone();
        if let Some(o) = &old
            && let Some(i) = list.iter().position(|x| x == o)
        {
            list.remove(i);
        }
        list.push(v);
        d.set_views(list)
    })?;
    c.print(&c.tv(if old.is_some() { "cmd.nv.replaced" } else { "cmd.nv.saved" }, &[("name", vname)]));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_io_cad::plot::n_of;
    #[test]
    fn iso_dates() {
        assert_eq!(iso_date(0.0), "1970-01-01");
        assert_eq!(iso_date(1_760_918_400_000.0), "2025-10-20");
        assert_eq!(iso_date(951_782_400_000.0), "2000-02-29");
    }
    #[test]
    fn a_new_sheet_has_a_viewport_onto_the_model() {
        let mut d = Doc::new();
        d.load(&json!({ "entities": [{ "type": "line", "x1": 0, "y1": 0, "x2": 1000, "y2": 500 }] }));
        let paper = paper_obj("iso-a3", false);
        let ents = sheet_contents(&d, &paper, "T", 1, "2026-10-19", "Title block");
        let vp = ents.iter().find_map(|e| if let Geo::Viewport(v) = &e.geo { Some(v.clone()) } else { None }).expect("a viewport");
        assert!((vp.cx - 500.0).abs() < 1e-9 && vp.s > 0.0);
        assert_eq!(n_of(&d, "mm", vp.s), 5.0, "1:5 fits 1000 × 500 on A3");
    }
}
