//! Blocks (commands.js): BLOCK ATTDEF INSERT BEDIT BCLOSE. The block editor opens the block's objects (and its
//! attribute definitions as texts) in a space of their own; BCLOSE saves them back as one undo step or throws the
//! edits away. Not ported: visibility states of dynamic blocks in the editor (BVSTATE / BVHIDE / BVSHOW), INSERT's
//! File and Library options (they need the browser's file picker and library panel; the command line says so).

use std::cmp::Ordering;
use std::rc::Rc;

use cc_cad::doc::Block;
use cc_cad::entity::{Color, Entity, Geo, Insert, Obj, Text};
use cc_cad::js;
use cc_cad_geom::geom::{norm_angle, norm_tag};
use cc_geom::P2;
use serde_json::{Map, Value};

use crate::cmd::draw::ent;
use crate::engine::{Ctx, Opts, R, Reply};
use crate::input::Def;
use crate::ucs::{active_ucs, ang_of};

/// The block editor while it is open (doc.js `blockEdit`).
#[derive(Clone, Debug)]
pub struct BlockEdit {
    pub name: String,
    pub space: cc_cad::SpaceId,
    pub base: P2,
    pub mark: usize,
    /// the layout it was opened from (None = the model)
    pub from: Option<String>,
}

const BAD_CHARS: &[char] = &['<', '>', '/', '\\', '"', ':', ';', '?', '*', '|', '=', '`'];

fn valid_block_name(n: &str) -> bool {
    !n.is_empty() && !n.starts_with('*') && !n.contains(BAD_CHARS) && n.encode_utf16().count() <= 255
}

/// `a.localeCompare(b, undefined, { numeric: true })`, near enough: case-insensitive with digit runs as numbers,
/// lower case first on a tie.
fn natural_cmp(a: &str, b: &str) -> Ordering {
    fn runs(s: &str) -> Vec<(bool, String)> {
        let mut out: Vec<(bool, String)> = Vec::new();
        for ch in s.chars() {
            let d = ch.is_ascii_digit();
            match out.last_mut() {
                Some((k, r)) if *k == d => r.push(ch),
                _ => out.push((d, ch.to_string())),
            }
        }
        out
    }
    let (ra, rb) = (runs(&a.to_lowercase()), runs(&b.to_lowercase()));
    for (x, y) in ra.iter().zip(&rb) {
        let o = if x.0 && y.0 {
            let (tx, ty) = (x.1.trim_start_matches('0'), y.1.trim_start_matches('0'));
            tx.len().cmp(&ty.len()).then_with(|| tx.cmp(ty))
        } else {
            x.1.cmp(&y.1)
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    ra.len().cmp(&rb.len()).then_with(|| b.cmp(a))
}

/// `namedBlocks(doc)`: the block names people use (not `*` ones), sorted.
pub fn named_blocks(c: &Ctx) -> Vec<String> {
    let mut v: Vec<String> = c.doc().blocks.keys().filter(|n| !n.starts_with('*')).cloned().collect();
    v.sort_by(|a, b| natural_cmp(a, b));
    v
}

/// `nests(doc, inner, name)`: does block `inner` contain a reference to `name` (16 levels at most)?
fn nests(c: &Ctx, inner: &str, name: &str, depth: u32) -> bool {
    let b = c.doc().blocks.get(&inner.to_string()).cloned();
    let Some(b) = b else { return false };
    if depth > 16 {
        return false;
    }
    b.entities.iter().any(|e| matches!(&e.geo, Geo::Insert(i) if i.block == name || nests(c, &i.block, name, depth.saturating_add(1))))
}

fn s_of(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(v) => js::to_js_string(v),
    }
}

/// attrs.js `oneLine`: runs of CR / LF become one space.
pub fn one_line(s: &str) -> String {
    let mut o = String::new();
    let mut run = false;
    for ch in s.chars() {
        if ch == '\r' || ch == '\n' {
            if !run {
                o.push(' ');
            }
            run = true;
        } else {
            o.push(ch);
            run = false;
        }
    }
    o
}

fn is_attdef_text(e: &Entity) -> bool {
    matches!(e.geo, Geo::Text(_)) && e.get("attdef").and_then(|a| a.get("tag")).is_some_and(Value::is_string)
}

/// `splitAttdefs(list)`: attribute-definition texts → block attdefs; the rest stays as the block's objects.
fn split_attdefs(list: &[Entity]) -> (Vec<Entity>, Vec<Value>) {
    let mut ents = Vec::new();
    let mut defs = Vec::new();
    for e in list {
        let (true, Geo::Text(t)) = (is_attdef_text(e), &e.geo) else {
            ents.push(e.clone());
            continue;
        };
        let a = e.get("attdef").cloned().unwrap_or(Value::Null);
        let or_s = |k: &str| match a.get(k) {
            None | Some(Value::Null) => Value::String(String::new()),
            Some(v) => v.clone(),
        };
        let mut d = Map::new();
        d.insert("tag".into(), Value::String(norm_tag(&s_of(a.get("tag")))));
        d.insert("prompt".into(), or_s("prompt"));
        d.insert("def".into(), or_s("def"));
        d.insert("x".into(), cc_cad::num(t.x));
        d.insert("y".into(), cc_cad::num(t.y));
        d.insert("h".into(), cc_cad::num(t.h));
        d.insert("rot".into(), cc_cad::num(js::or(t.rot, 0.0)));
        d.insert("ha".into(), Value::String(t.ha.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "left".into())));
        d.insert("va".into(), Value::String(t.va.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "baseline".into())));
        d.insert("wf".into(), cc_cad::num(js::or(t.wf, 1.0)));
        for k in ["invisible", "constant", "preset"] {
            d.insert(k.into(), Value::Bool(js::truthy(a.get(k))));
        }
        if let Some(l) = &e.layer {
            d.insert("layer".into(), Value::String(l.clone()));
        }
        d.insert("color".into(), Color::or_null(&e.color).to_value());
        defs.push(Value::Object(d));
    }
    (ents, defs)
}

/// `attdefText(d)`: a block attdef as an attribute-definition text, for the block editor.
fn attdef_text(d: &Value) -> Entity {
    let n = |k: &str| d.get(k).and_then(Value::as_f64);
    let tag = s_of(d.get("tag"));
    let t = Text {
        x: n("x").unwrap_or(f64::NAN),
        y: n("y").unwrap_or(f64::NAN),
        h: n("h").filter(|h| *h > 0.0).unwrap_or(2.5),
        s: tag.clone(),
        rot: Some(js::or(n("rot"), 0.0)),
        ha: Some(d.get("ha").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("left").into()),
        va: Some(d.get("va").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("baseline").into()),
        wf: Some(js::or(n("wf"), 1.0)),
        ww: None,
        ls: None,
    };
    let layer = match d.get("layer") {
        None | Some(Value::Null) => "0".to_string(),
        Some(v) => s_of(Some(v)),
    };
    let color = d.get("color").and_then(Color::from_value).unwrap_or(Color::Null);
    let mut e = Entity::with(Geo::Text(t), Some(layer), Some(color));
    let or_s = |k: &str| match d.get(k) {
        None | Some(Value::Null) => Value::String(String::new()),
        Some(v) => v.clone(),
    };
    let mut a = Map::new();
    a.insert("tag".into(), Value::String(tag));
    a.insert("prompt".into(), or_s("prompt"));
    a.insert("def".into(), or_s("def"));
    for k in ["invisible", "constant", "preset"] {
        a.insert(k.into(), Value::Bool(js::truthy(d.get(k))));
    }
    e.extra.insert("attdef".into(), Value::Object(a));
    e
}

/// attrs.js `defaultAttrs(blk)`: each variable definition's default.
pub fn default_attrs(blk: Option<&Block>) -> Obj {
    let mut out = Obj::new();
    for d in blk.and_then(|b| b.attdefs.as_ref()).map_or(&[][..], Vec::as_slice) {
        if js::truthy(d.get("constant")) {
            continue;
        }
        let def = d.get("def").filter(|v| js::truthy(Some(v))).map_or(String::new(), |v| s_of(Some(v)));
        out.insert(s_of(d.get("tag")), Value::String(def));
    }
    out
}

fn insert_geo(block: &str, at: P2, sc: f64, rot: f64, attrs: Option<Obj>) -> Geo {
    Geo::Insert(Insert { block: block.into(), x: at.x, y: at.y, sx: Some(sc), sy: Some(sc), rot: Some(rot), attrs, attloc: None })
}

pub async fn block(c: Ctx) -> R {
    let name = loop {
        let r = c.string(c.t("cmd.block.enter_block_name"), Opts::default()).await?;
        let name = js::trim(&s_reply(&r)).to_string();
        if name.is_empty() {
            return Ok(());
        }
        if !valid_block_name(&name) {
            c.print(&c.t("cmd.block.badname"));
            continue;
        }
        if c.doc().blocks.get(&name).is_some() {
            let options = vec![c.opt("Y", &c.t("opt.yes")), c.opt("N", &c.t("opt.no"))];
            let k = c
                .keyword(
                    c.tv("cmd.block.redefine", &[("name", name.clone())]),
                    Opts { options, default: Some(Def::Str("N".into())), ..Opts::default() },
                )
                .await?;
            if !matches!(&k, Reply::Opt(x) | Reply::Str(x) if x == "Y") {
                continue;
            }
        }
        break name;
    };
    let base = c.must_point(c.t("cmd.block.specify_insertion_base_point"), Opts::default()).await?;
    let ids = c.select(None, false).await?;
    if ids.is_empty() {
        c.print(&c.t("cmd.block.noobjects"));
        return Ok(());
    }
    let src: Vec<Entity> = {
        let d = c.doc();
        ids.0.iter().filter_map(|id| d.get(*id).cloned()).collect()
    };
    if src.iter().any(|e| matches!(e.geo, Geo::Viewport(_))) {
        c.print(&c.t("cmd.block.viewport_cannot_go_block"));
        return Ok(());
    }
    if src.iter().any(|e| matches!(&e.geo, Geo::Insert(i) if i.block == name || nests(&c, &i.block, &name, 0))) {
        c.print(&c.t("cmd.block.block_cannot_contain_itself"));
        return Ok(());
    }
    let (plain, attdefs) = split_attdefs(&src);
    let nat = attdefs.len();
    let mut ins = ent(&c, insert_geo(&name, base, 1.0, 0.0, None));
    c.change("BLOCK", |d| {
        let mut b = Block::new(&name, base, plain);
        b.attdefs = Some(attdefs);
        let blk = d.define_block(&b)?;
        for e in &src {
            d.remove(e.id.unwrap_or(0))?;
        }
        if nat > 0
            && let Geo::Insert(i) = &mut ins.geo
        {
            i.attrs = Some(default_attrs(Some(&blk)));
        }
        d.add(ins)
    })?;
    if nat > 0 {
        c.print(&c.tv("cmd.attdef.inblock", &[("n", nat.to_string())]));
    }
    c.st().memo.last_block = Some(name.clone());
    c.print(&c.tv("cmd.block.made", &[("name", name), ("n", src.len().to_string())]));
    Ok(())
}

fn s_reply(r: &Reply) -> String {
    if let Reply::Str(s) = r { s.clone() } else { String::new() }
}

/// ATTDEF's modes (`E.attModes`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AttModes {
    pub invisible: bool,
    pub constant: bool,
    pub preset: bool,
}

pub async fn attdef(c: Ctx) -> R {
    let yn = |c: &Ctx, b: bool| c.t(if b { "opt.yes" } else { "opt.no" });
    loop {
        let m = c.st().memo.att_modes;
        c.print(&c.tv("cmd.attdef.modes", &[("i", yn(&c, m.invisible)), ("c", yn(&c, m.constant)), ("p", yn(&c, m.preset))]));
        let options = vec![c.opt("I", &c.t("opt.invisible")), c.opt("C", &c.t("opt.constant")), c.opt("P", &c.t("opt.preset"))];
        let k = c.keyword(c.t("cmd.attdef.mode"), Opts { options, none: true, ..Opts::default() }).await?;
        let Reply::Opt(o) = k else { break };
        let mut st = c.st();
        let m = &mut st.memo.att_modes;
        match o.as_str() {
            "I" => m.invisible = !m.invisible,
            "C" => m.constant = !m.constant,
            "P" => m.preset = !m.preset,
            _ => {}
        }
    }
    let tag = loop {
        let r = c.string(c.t("cmd.attdef.tag"), Opts::default()).await?;
        let tag = norm_tag(&s_reply(&r));
        if tag.is_empty() {
            return Ok(());
        }
        if tag.contains(BAD_CHARS) {
            c.print(&c.t("cmd.attdef.badtag"));
            continue;
        }
        break tag;
    };
    let m = c.st().memo.att_modes;
    let prompt = if m.constant {
        String::new()
    } else {
        js::trim(&s_reply(&c.string(c.t("cmd.attdef.prompt"), Opts::default()).await?)).to_string()
    };
    let dkey = if m.constant { "cmd.attdef.value" } else { "cmd.attdef.default" };
    let def = one_line(js::trim(&s_reply(&c.string(c.t(dkey), Opts::default()).await?)));
    let at = c.must_point(c.t("cmd.attdef.at"), Opts::default()).await?;
    let th = c.doc().current.text_height;
    let h = c.distance(c.t("cmd.attdef.height"), Opts { base: Some(at), default: Some(Def::Num(th)), ..Opts::default() }).await?;
    let Reply::Num(h) = h else { return Ok(()) };
    if !(h > 0.0) {
        return Ok(());
    }
    let u = ang_of(active_ucs(&c.doc()));
    let rot = c.angle(c.t("cmd.attdef.rot"), Opts { base: Some(at), default: Some(Def::Num(u)), ucs: true, ..Opts::default() }).await?;
    let rot = match rot {
        Reply::Num(r) if r != 0.0 && !r.is_nan() => r,
        _ => 0.0,
    };
    let t = Text {
        x: at.x,
        y: at.y,
        h,
        s: tag.clone(),
        rot: Some(norm_angle(rot)),
        ha: Some("left".into()),
        va: Some("baseline".into()),
        wf: Some(1.0),
        ww: None,
        ls: None,
    };
    let mut e = ent(&c, Geo::Text(t));
    let mut a = Map::new();
    a.insert("tag".into(), Value::String(tag.clone()));
    a.insert("prompt".into(), Value::String(prompt));
    a.insert("def".into(), Value::String(def));
    a.insert("invisible".into(), Value::Bool(m.invisible));
    a.insert("constant".into(), Value::Bool(m.constant));
    a.insert("preset".into(), Value::Bool(m.preset));
    e.extra.insert("attdef".into(), Value::Object(a));
    c.add(e)?;
    c.print(&c.tv("cmd.attdef.made", &[("tag", tag)]));
    Ok(())
}

fn trim_end_colons(s: &str) -> &str {
    s.trim_end_matches(|ch: char| ch == ':' || ch.is_whitespace() || ch == '\u{feff}')
}

/// `placeBlock`: preview, S / R options, then each attribute value the block asks for → the new insert.
pub async fn place_block(c: &Ctx, name: &str) -> R<Option<Entity>> {
    let blk = c.doc().blocks.get(&name.to_string()).cloned();
    let (mut sc, mut rot) = {
        let st = c.st();
        (st.memo.last_insert_scale.unwrap_or(1.0), st.memo.last_insert_rot.unwrap_or(0.0))
    };
    let mut values = default_attrs(blk.as_ref());
    let nm = name.to_string();
    loop {
        {
            let (nm, v, layer) = (nm.clone(), values.clone(), c.doc().current.layer.clone());
            c.preview(Some(Rc::new(move |_, cur| {
                let mut e = Entity::new(insert_geo(&nm, cur, sc, rot, Some(v.clone())));
                e.layer = Some(layer.clone());
                vec![e]
            })));
        }
        let p = c
            .point(
                c.t("cmd.pasteclip.specify_insertion_point"),
                Opts { options: vec![c.opt("S", "Scale"), c.opt("R", "Rotate")], ..Opts::default() },
            )
            .await?;
        let at = match p {
            Reply::Opt(k) if k == "S" => {
                let v = c.number(c.t("cmd.scale.specify_scale_factor"), Opts { default: Some(Def::Num(sc)), ..Opts::default() }).await?;
                if let Reply::Num(v) = v
                    && v != 0.0
                    && !v.is_nan()
                {
                    sc = v;
                }
                c.st().memo.last_insert_scale = Some(sc);
                continue;
            }
            Reply::Opt(k) if k == "R" => {
                let a = c.angle(c.t("cmd.mtext.rot"), Opts { default: Some(Def::Num(rot)), ucs: true, ..Opts::default() }).await?;
                rot = norm_angle(if let Reply::Num(a) = a { a } else { 0.0 });
                c.st().memo.last_insert_rot = Some(rot);
                continue;
            }
            Reply::Point(p, _) => p,
            _ => return Ok(None),
        };
        c.preview(None);
        let defs = blk.as_ref().and_then(|b| b.attdefs.clone()).unwrap_or_default();
        for d in &defs {
            if js::truthy(d.get("constant")) || js::truthy(d.get("preset")) {
                continue;
            }
            let tag = s_of(d.get("tag"));
            let pr = s_of(d.get("prompt"));
            let label = if pr.is_empty() { tag.clone() } else { pr };
            let cur = values.get(&tag).map(|v| s_of(Some(v))).filter(|s| !s.is_empty());
            let v = c
                .string(format!("Enter {} ({tag})", trim_end_colons(&label)), Opts { default: cur.map(Def::Str), ..Opts::default() })
                .await?;
            let v = s_reply(&v);
            let t = js::trim(&v);
            let nv = if t.is_empty() { values.get(&tag).map(|x| s_of(Some(x))).unwrap_or_default() } else { t.to_string() };
            values.insert(tag, Value::String(one_line(&nv)));
        }
        let has_defs = !defs.is_empty();
        let attrs = if has_defs || !values.is_empty() { Some(values) } else { None };
        let e = ent(c, insert_geo(name, at, sc, rot, attrs));
        return Ok(Some(c.add(e)?));
    }
}

pub async fn insert(c: Ctx) -> R {
    let names = named_blocks(&c);
    if names.is_empty() {
        c.print(&c.t("cmd.insert.drawing_has_no_blocks"));
    }
    let name = loop {
        let last = c.st().memo.last_block.clone();
        let def = last.clone().filter(|l| c.doc().blocks.get(l).is_some());
        let r = c.string(c.t("cmd.insert.enter_block_name_file"), Opts { default: def.map(Def::Str), ..Opts::default() }).await?;
        let typed = js::trim(&s_reply(&r)).to_string();
        let name = if typed.is_empty() { last.unwrap_or_default() } else { typed };
        if !name.is_empty() && c.doc().blocks.get(&name).is_some() {
            break name;
        }
        if name.is_empty() {
            return Ok(());
        }
        if let Some(near) = names.iter().find(|n| n.to_lowercase() == name.to_lowercase()) {
            break near.clone();
        }
        let low = name.to_lowercase();
        if low == "f" || low == "file" {
            c.print(&c.t("cmd.library.opening_another_drawing_needs"));
            return Ok(());
        }
        if low == "l" || low == "lib" || low == "library" {
            c.print(&c.t("cmd.library.library_needs_browser"));
            return Ok(());
        }
        let mut list = names.iter().take(30).cloned().collect::<Vec<_>>().join(", ");
        if list.is_empty() {
            list = c.t("cmd.none");
        }
        if names.len() > 30 {
            list.push_str(" …");
        }
        c.print(&c.tv("cmd.insert.noblock", &[("name", name), ("list", list)]));
    };
    c.st().memo.last_block = Some(name.clone());
    place_block(&c, &name).await?;
    Ok(())
}

pub async fn bedit(c: Ctx) -> R {
    let open = c.st().memo.block_edit.clone();
    if let Some(be) = open {
        if c.doc().space != be.space {
            c.sh().doc.borrow_mut().set_space_id(be.space);
        }
        c.print(&c.tv("cmd.bedit.already", &[("name", be.name)]));
        return Ok(());
    }
    let sel_block: Option<String> = {
        let d = c.doc();
        let sel = c.sh().host.borrow().selection();
        sel.iter().filter_map(|id| d.get(*id)).find_map(|e| if let Geo::Insert(i) = &e.geo { Some(i.block.clone()) } else { None })
    };
    let names = named_blocks(&c);
    if names.is_empty() {
        c.print(&c.t("cmd.bedit.none"));
        return Ok(());
    }
    let name = loop {
        let def = sel_block.clone().or_else(|| c.st().memo.last_block.clone());
        let r = c.string(c.t("cmd.bedit.name"), Opts { default: def.map(Def::Str), ..Opts::default() }).await?;
        let v = js::trim(&s_reply(&r)).to_string();
        if v.is_empty() {
            return Ok(());
        }
        if let Some(n) = names.iter().find(|n| **n == v).or_else(|| names.iter().find(|n| n.to_lowercase() == v.to_lowercase())) {
            break n.clone();
        }
        c.print(&c.tv("cmd.bedit.unknown", &[("name", v), ("list", names.iter().take(30).cloned().collect::<Vec<_>>().join(", "))]));
    };
    if let Some(link) = crate::cmd::corner::link_of_pub(&c, &name) {
        c.print(&c.tv("cmd.xref.readonly", &[("name", link)]));
        return Ok(());
    }
    let Some(blk) = c.doc().blocks.get(&name).cloned() else { return Ok(()) };
    let mut ents: Vec<Entity> = Vec::new();
    {
        let mut d = c.sh().doc.borrow_mut();
        let fresh = |mut e: Entity, d: &mut cc_cad::Doc| {
            if e.color.is_none() {
                e.color = Some(Color::Null);
            }
            e.id = Some(d.next_id);
            e.extra.remove("id");
            d.next_id = d.next_id.saturating_add(1);
            e
        };
        for e in &blk.entities {
            ents.push(if e.id.is_some_and(|i| i != 0) { e.clone() } else { fresh(e.clone(), &mut d) });
        }
        for a in blk.attdefs.as_deref().unwrap_or(&[]) {
            ents.push(fresh(attdef_text(a), &mut d));
        }
    }
    let from = c.doc().layout().map(|l| l.name.clone());
    c.set_selection(&[]);
    let mark = c.doc().mark();
    let space = {
        let mut d = c.sh().doc.borrow_mut();
        let s = d.loose_space(ents);
        d.set_space_id(s);
        s
    };
    c.st().memo.block_edit = Some(BlockEdit { name: name.clone(), space, base: blk.base, mark, from });
    c.sh().host.borrow_mut().zoom_extents();
    c.print(&c.tv("cmd.bedit.open", &[("name", name)]));
    Ok(())
}

/// `closeBlockEdit(doc, save)`: save the block (one undo step with everything done in it) or throw the edits away.
pub fn close_block_edit(c: &Ctx, save: bool) -> Option<BlockEdit> {
    let be = c.st().memo.block_edit.take()?;
    let list: Vec<Entity> = c.doc().space_entities(be.space).cloned().collect();
    let (entities, attdefs) = split_attdefs(&list);
    let mut d = c.sh().doc.borrow_mut();
    d.set_space(be.from.as_deref());
    if save {
        let mut b = Block::new(&be.name, be.base, entities);
        b.attdefs = Some(attdefs);
        let _ = d.change("BEDIT", |d| d.define_block(&b));
        d.squash(be.mark, "BEDIT");
    } else {
        while d.undo_stack.len() > be.mark {
            d.undo();
        }
        d.redo_stack.clear();
    }
    d.drop_space(be.space);
    Some(be)
}

pub async fn bclose(c: Ctx) -> R {
    let Some(be) = c.st().memo.block_edit.clone() else {
        c.print(&c.t("cmd.bclose.none"));
        return Ok(());
    };
    let options = vec![c.opt("S", &c.t("opt.save")), c.opt("D", &c.t("opt.discard"))];
    let k = c
        .keyword(
            c.tv("cmd.bclose.ask", &[("name", be.name.clone())]),
            Opts { options, default: Some(Def::Str("S".into())), ..Opts::default() },
        )
        .await?;
    let save = !matches!(&k, Reply::Opt(x) | Reply::Str(x) if x == "D");
    close_block_edit(&c, save);
    let n = c.doc().block_uses(&be.name);
    c.print(&c.tv(if save { "cmd.bclose.saved" } else { "cmd.bclose.discarded" }, &[("name", be.name), ("n", n.to_string())]));
    Ok(())
}
