//! Linked drawings and clipped references: xref.js (`attachXref`, `detachXref`, `bindXref`, `xrefs`), layertools.js's
//! XATTACH / XREF (? list, Attach, Reload, Bind with WL.17's "clipped part only", Detach) and xclip.js's XCLIP.
//!
//! A linked drawing is a block whose definition carries `xref`: the root `{ path, loaded }` holds the other drawing's
//! model space; its own blocks come along as parts `{ of: root }`, their names and layers with the link's name in front
//! ("Plan|Walls"). The other drawing comes from the host (`host.pickDrawing`): the window's picker, the CLI's script.

use cc_cad::doc::LayerState;
use cc_cad::entity::{Entity, Geo, Obj, num};
use cc_cad::{Block, Doc, DocError, Layer, MODEL, SpaceId, js};
use cc_cad_geom::geom::{insert_matrix, m_apply};
use cc_cad_geom::xclip::{clip_entities, clip_poly};
use cc_geom::P2;
use serde_json::{Value, json};

use crate::cmd::qselect::locale_cmp_plain;
use crate::engine::{Ctx, Opts, R, Reply};
use crate::input::Def;
use crate::lang::Lang;

/// What the window's picker prompt says while it is open (plain English: 0.5's browser picker has no prompt).
const PICK_DRAWING: &str = "Pick the drawing to link";

/// `isXrefRoot(b)`
pub fn is_xref_root(b: Option<&Block>) -> bool {
    b.and_then(|b| b.xref.as_ref()).is_some_and(|x| x.is_object() && x.get("of").is_none())
}

fn part_of(b: &Block, name: &str) -> bool {
    b.xref.as_ref().and_then(|x| x.get("of")).is_some_and(|of| match of {
        Value::String(s) => s == name,
        v => js::to_js_string(v) == name,
    })
}

/// One row of `xrefs(doc)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub name: String,
    pub path: String,
    pub loaded: bool,
    pub refs: usize,
    pub objects: usize,
}

/// `xrefs(doc)`: the links, by name.
pub fn xrefs(doc: &Doc) -> Vec<Link> {
    let mut out: Vec<Link> = doc
        .blocks
        .values()
        .filter(|b| is_xref_root(Some(b)))
        .map(|b| {
            let refs = doc.every_entity().filter(|e| matches!(&e.geo, Geo::Insert(i) if i.block == b.name)).count();
            let x = b.xref.as_ref();
            let path = x.and_then(|x| x.get("path")).filter(|p| js::truthy(Some(p))).map(js::to_js_string).unwrap_or_default();
            let loaded = x.and_then(|x| x.get("loaded")) != Some(&Value::Bool(false)) && !b.entities.is_empty();
            Link { name: b.name.clone(), path, loaded, refs, objects: b.entities.len() }
        })
        .collect();
    out.sort_by(|a, b| locale_cmp_plain(&a.name, &b.name));
    out
}

/// blockcopy.js `MM_PER` / `mmPerUnit`.
pub fn mm_per_unit(units: Option<&Value>) -> Option<f64> {
    Some(match units?.as_str()? {
        "millimeters" => 1.0,
        "centimeters" => 10.0,
        "meters" => 1000.0,
        "kilometers" => 1e6,
        "inches" => 25.4,
        "feet" => 304.8,
        "yards" => 914.4,
        "miles" => 1_609_344.0,
        "mils" => 0.0254,
        "microinches" => 0.000_025_4,
        _ => return None,
    })
}

/// blockcopy.js `blockNameFromFile(file)`: no folder, no extension, no characters block names refuse.
pub fn block_name_from_file(file: &str) -> String {
    let f = if file.is_empty() { "Drawing" } else { file };
    let base = f.rsplit(['\\', '/']).next().unwrap_or("");
    let base = match base.rfind('.') {
        Some(i) if i.saturating_add(1) < base.len() => base.get(..i).unwrap_or(base),
        _ => base,
    };
    let cleaned: String = base.chars().map(|c| if "<>/\\\":;?*|=`".contains(c) { '_' } else { c }).collect();
    let n = js::slice16(js::trim(cleaned.trim_start_matches('*')), 255);
    if n.is_empty() { "Drawing".into() } else { n }
}

/// The other drawing, as the host's read gives it (`src` of `attachXref`): doc.load's input with its objects typed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct XSource {
    pub meta: Value,
    pub layers: Vec<Value>,
    pub blocks: Vec<XBlock>,
    pub entities: Vec<Entity>,
}

/// One block of an [`XSource`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct XBlock {
    /// `String(b.name)`
    pub name: String,
    pub base: Value,
    pub entities: Vec<Entity>,
    pub attdefs: Option<Vec<Value>>,
}

impl XSource {
    /// doc.load's input → typed objects (anything not an array or object reads as empty).
    pub fn from_value(v: &Value) -> XSource {
        let arr = |k: &str| v.get(k).and_then(Value::as_array);
        let ents = |a: Option<&Vec<Value>>| a.map(|a| a.iter().map(Entity::from_value).collect()).unwrap_or_default();
        XSource {
            meta: v.get("meta").cloned().unwrap_or(Value::Null),
            layers: arr("layers").cloned().unwrap_or_default(),
            blocks: arr("blocks")
                .map(|a| {
                    a.iter()
                        .map(|b| XBlock {
                            name: b
                                .get("name")
                                .map(|n| n.as_str().map_or_else(|| js::to_js_string(n), str::to_string))
                                .unwrap_or_else(|| "undefined".into()),
                            base: b.get("base").cloned().unwrap_or(Value::Null),
                            entities: ents(b.get("entities").and_then(Value::as_array)),
                            attdefs: b.get("attdefs").and_then(Value::as_array).cloned(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            entities: ents(arr("entities")),
        }
    }
}

/// What `attachXref` returns.
#[derive(Clone, Debug, PartialEq)]
pub struct Attached {
    pub name: String,
    pub scale: f64,
    pub note: String,
    pub layers: usize,
    pub blocks: usize,
}

fn name_of(v: Option<&Value>) -> Option<String> {
    v.filter(|n| js::truthy(Some(n))).map(|n| n.as_str().map_or_else(|| js::to_js_string(n), str::to_string))
}

/// `attachXref(doc, src, fileName, { name })`: bring the other drawing in as the link `name` (None: a free name from the
/// file). Inside a change.
pub fn attach_xref(doc: &mut Doc, src: XSource, file_name: &str, name: Option<&str>, lang: &Lang) -> Result<Attached, DocError> {
    let mut retained: Vec<Layer> = Vec::new();
    let root = match name {
        Some(n) => {
            let pre = format!("{n}|");
            retained = doc.layer_state().list.into_iter().filter(|l| l.name.starts_with(&pre)).collect();
            detach_xref(doc, n, true, true)?;
            n.to_string()
        }
        None => {
            let base = block_name_from_file(file_name);
            let mut root = base.clone();
            let mut k: u32 = 2;
            while k < 100_000 && (doc.blocks.contains_key(&root) || doc.layers.keys().any(|l| l.starts_with(&format!("{root}|")))) {
                root = format!("{base} ({k})");
                k = k.saturating_add(1);
            }
            root
        }
    };
    let p = format!("{root}|");
    let lay_s = |n: &str| -> String {
        if n.is_empty() || n == "0" {
            return "0".into();
        }
        let mut s = String::with_capacity(p.len().saturating_add(n.len()));
        s.push_str(&p);
        s.push_str(n);
        s
    };
    let lay = |n: Option<&Value>| -> String { name_of(n).map_or_else(|| "0".into(), |s| lay_s(&s)) };
    // the last block of a name wins, as a Map keeps it
    let bmap: std::collections::HashMap<String, String> = src.blocks.iter().map(|b| (b.name.clone(), format!("{p}{}", b.name))).collect();
    // each object in place: its layer under the link's name (the old text's buffer grows in front), no id
    let map_e = |c: &mut Entity| {
        match &mut c.layer {
            Some(l) if l.is_empty() || l.as_str() == "0" => "0".clone_into(l),
            Some(l) => l.insert_str(0, &p),
            None => c.layer = Some(lay(c.extra.get("layer"))),
        }
        c.extra.remove("layer");
        c.id = None;
        c.extra.remove("id");
        match &mut c.geo {
            Geo::Insert(i) => {
                if !i.block.is_empty()
                    && let Some(to) = bmap.get(i.block.as_str())
                {
                    i.block.clone_from(to);
                }
            }
            Geo::Dim(d) => {
                if let Some(b) = d.block.as_mut().filter(|b| !b.is_empty())
                    && let Some(to) = bmap.get(b.as_str())
                {
                    b.clone_from(to);
                }
            }
            _ => {
                if let Some(to) =
                    c.extra.get("block").filter(|b| js::truthy(Some(b))).map(js::to_js_string).and_then(|b| bmap.get(b.as_str()).cloned())
                {
                    c.extra.insert("block".into(), Value::String(to));
                }
            }
        }
    };
    // layers: the other drawing's, under the link's name (its layer 0 is this drawing's 0)
    let mut s: LayerState = doc.layer_state();
    let mut layers = 0usize;
    for l in &src.layers {
        let Some(n) = name_of(l.get("name")) else { continue };
        let full = format!("{p}{n}");
        if n == "0" || s.list.iter().any(|x| x.name == full) {
            continue;
        }
        let mut own = Obj::new();
        own.insert("name".into(), Value::String(full.clone()));
        own.insert("color".into(), l.get("color").filter(|c| !c.is_null()).cloned().unwrap_or(json!(7)));
        own.insert("on".into(), Value::Bool(l.get("on") != Some(&Value::Bool(false))));
        own.insert("frozen".into(), Value::Bool(js::truthy(l.get("frozen"))));
        own.insert("locked".into(), Value::Bool(js::truthy(l.get("locked"))));
        own.insert("plot".into(), Value::Bool(l.get("plot") != Some(&Value::Bool(false))));
        own.insert("linetype".into(), l.get("linetype").filter(|v| js::truthy(Some(v))).cloned().unwrap_or_else(|| json!("Continuous")));
        let fin = |v: Option<&Value>| v.and_then(Value::as_f64).filter(|x| x.is_finite());
        own.insert("lineweight".into(), num(fin(l.get("lineweight")).unwrap_or(-3.0)));
        if let Some(tr) = fin(l.get("tr")) {
            own.insert("tr".into(), num(tr));
        }
        if let Some(kept) = retained.iter().find(|x| x.name == full)
            && let Value::Object(k) = kept.to_value()
        {
            for (key, v) in k {
                own.insert(key, v);
            }
            own.insert("name".into(), Value::String(full.clone()));
        }
        s.list.push(Layer::from_state_value(&Value::Object(own)));
        layers = layers.saturating_add(1);
    }
    doc.set_layer_state(&s)?;
    let mut blocks = 0usize;
    let XSource { meta, layers: _, blocks: src_blocks, entities: src_entities } = src;
    for b in src_blocks {
        let mut blk = Block::from_value(&json!({ "name": format!("{p}{}", b.name), "base": b.base }));
        let mut ents = b.entities;
        ents.iter_mut().for_each(&map_e);
        blk.entities = ents.into();
        blk.attdefs = b.attdefs.as_ref().filter(|a| !a.is_empty()).map(|a| {
            a.iter()
                .map(|d| {
                    let mut o = d.as_object().cloned().unwrap_or_default();
                    o.insert("layer".into(), Value::String(lay(d.get("layer"))));
                    Value::Object(o)
                })
                .collect()
        });
        blk.xref = Some(json!({ "of": root }));
        doc.define_block_owned(blk)?;
        blocks = blocks.saturating_add(1);
    }
    let insbase = meta.get("insbase").cloned().unwrap_or(Value::Null);
    let mut rb = Block::from_value(&json!({ "name": root, "base": insbase }));
    let mut ents = src_entities;
    ents.iter_mut().for_each(&map_e);
    rb.entities = ents.into();
    rb.xref = Some(json!({ "path": file_name, "loaded": true }));
    doc.define_block_owned(rb)?;
    let su = meta.get("units");
    let du = doc.meta.get("units").cloned();
    let (a, b) = (mm_per_unit(su), mm_per_unit(du.as_ref()));
    let scale = match (a, b) {
        (Some(a), Some(b)) if a != 0.0 && b != 0.0 => a / b,
        _ => 1.0,
    };
    let note = match (a, b) {
        (Some(a), Some(b)) if a != 0.0 && b != 0.0 && a != b => {
            let sc = js::str_to_number(&js::to_exponential(scale, 5));
            lang.t(
                "xref.units",
                &[
                    ("a", su.map(js::to_js_string).unwrap_or_default()),
                    ("b", du.as_ref().map(js::to_js_string).unwrap_or_default()),
                    ("scale", js::num_to_string(sc)),
                ],
            )
        }
        _ => String::new(),
    };
    Ok(Attached { name: root, scale, note, layers, blocks })
}

/// The top-level references to a block: (space, id).
fn refs_to(doc: &Doc, name: &str) -> Vec<(SpaceId, u64)> {
    let mut out = Vec::new();
    for sp in std::iter::once(MODEL).chain(doc.layouts.iter().map(|l| l.space)) {
        for e in doc.space_entities(sp) {
            if let (Geo::Insert(i), Some(id)) = (&e.geo, e.id)
                && i.block == name
            {
                out.push((sp, id));
            }
        }
    }
    out
}

/// `detachXref(doc, name, { keepRefs, keepRoot })`: take a link out. Inside a change. → false when it is no link.
pub fn detach_xref(doc: &mut Doc, name: &str, keep_refs: bool, keep_root: bool) -> Result<bool, DocError> {
    let Some(root) = doc.blocks.get(&name.to_string()).filter(|b| is_xref_root(Some(b))).cloned() else { return Ok(false) };
    let parts: Vec<String> = doc.blocks.values().filter(|b| part_of(b, name)).map(|b| b.name.clone()).collect();
    if !keep_refs {
        for (sp, id) in refs_to(doc, name) {
            doc.remove_in(id, sp)?;
        }
    }
    for n in &parts {
        doc.remove_block(n)?;
    }
    if keep_root {
        let mut b = root.clone();
        b.entities = cc_cad::Ents::new();
        let path = root.xref.as_ref().and_then(|x| x.get("path")).cloned();
        let mut x = Obj::new();
        if let Some(p) = path {
            x.insert("path".into(), p);
        }
        x.insert("loaded".into(), Value::Bool(false));
        b.xref = Some(Value::Object(x));
        doc.define_block(&b)?;
    } else {
        doc.remove_block(name)?;
    }
    // its layers, when nothing else uses them
    let p = format!("{name}|");
    let mut used: std::collections::HashSet<Option<String>> = std::collections::HashSet::new();
    for e in doc.every_entity() {
        used.insert(e.layer.clone());
    }
    for b in doc.blocks.values() {
        for e in &b.entities {
            used.insert(e.layer.clone());
        }
    }
    let mut s = doc.layer_state();
    s.list.retain(|l| !l.name.starts_with(&p) || used.contains(&Some(l.name.clone())));
    if !s.list.iter().any(|l| l.name == s.current) {
        s.current = "0".into();
    }
    doc.set_layer_state(&s)?;
    Ok(true)
}

/// `bindXref(doc, name)`: a link becomes ordinary blocks and layers ("Plan$0$Walls"). Inside a change.
pub fn bind_xref(doc: &mut Doc, name: &str) -> Result<bool, DocError> {
    let Some(root) = doc.blocks.get(&name.to_string()).filter(|b| is_xref_root(Some(b))).cloned() else { return Ok(false) };
    let p = format!("{name}|");
    let q = format!("{name}$0$");
    let bn = |n: &str| -> String { n.strip_prefix(&p).map_or_else(|| n.to_string(), |rest| format!("{q}{rest}")) };
    let fix = |e: &Entity| -> Entity {
        let mut c = e.clone();
        c.layer = e.layer.as_deref().map(bn);
        match &mut c.geo {
            Geo::Insert(i) if !i.block.is_empty() => i.block = bn(&i.block),
            Geo::Dim(d) => {
                if let Some(b) = d.block.as_mut().filter(|b| !b.is_empty()) {
                    *b = bn(b);
                }
            }
            _ => {
                if let Some(b) = c.extra.get("block").and_then(Value::as_str).filter(|b| !b.is_empty()).map(bn) {
                    c.extra.insert("block".into(), Value::String(b));
                }
            }
        }
        c
    };
    let parts: Vec<Block> = doc.blocks.values().filter(|b| part_of(b, name)).cloned().collect();
    for b in &parts {
        doc.remove_block(&b.name)?;
        let mut plain = b.clone();
        plain.xref = None;
        plain.name = bn(&b.name);
        plain.entities = b.entities.iter().map(&fix).collect();
        if let Some(a) = &b.attdefs {
            plain.attdefs = Some(
                a.iter()
                    .map(|d| {
                        let mut o = d.as_object().cloned().unwrap_or_default();
                        if let Some(l) = d.get("layer").and_then(Value::as_str) {
                            o.insert("layer".into(), Value::String(bn(l)));
                        }
                        Value::Object(o)
                    })
                    .collect(),
            );
        }
        doc.define_block(&plain)?;
    }
    let mut plain = root.clone();
    plain.xref = None;
    plain.entities = root.entities.iter().map(&fix).collect();
    doc.define_block(&plain)?;
    let mut s = doc.layer_state();
    for l in &mut s.list {
        if l.name.starts_with(&p) {
            l.name = bn(&l.name);
        }
    }
    s.current = bn(&s.current);
    doc.set_layer_state(&s)?;
    Ok(true)
}

// ---------------------------------------------------------------------------------------------------- commands

fn lang(c: &Ctx) -> Lang {
    c.sh().lang.clone()
}

/// XATTACH (layertools.js `attach`): pick a DWG / DXF, link it, place one reference.
async fn attach(c: &Ctx) -> R {
    let Some(got) = c.pick_drawing(PICK_DRAWING.into()).await? else {
        c.print(&c.t("xref.nobrowser"));
        return Ok(());
    };
    let Some(got) = got else { return Ok(()) };
    let mark = c.doc().mark();
    let lg = lang(c);
    let file_name = got.file_name.clone();
    let r = c.change("XATTACH", |d| attach_xref(d, got.drawing, &got.file_name, None, &lg))?;
    if !r.note.is_empty() {
        c.print(&r.note);
    }
    let at = c
        .point(c.tv("xref.at", &[("name", r.name.clone())]), Opts { default: Some(Def::Point(P2::new(0.0, 0.0))), ..Opts::default() })
        .await?;
    let Some(at) = at.point() else {
        c.sh().doc.borrow_mut().undo();
        c.print(&c.t("xref.cancel"));
        return Ok(());
    };
    let layer = c.doc().current.layer.clone();
    let e = Entity::from_value(&json!({ "type": "insert", "layer": layer, "color": null, "block": r.name, "x": num(at.x), "y": num(at.y),
        "sx": num(r.scale), "sy": num(r.scale), "rot": 0 }));
    c.change("XATTACH", |d| d.add(e))?;
    c.sh().doc.borrow_mut().squash(mark, "XATTACH");
    let n = c.doc().blocks.get(&r.name).map_or(0, |b| b.entities.len());
    c.print(&c.tv("xref.attached", &[("name", r.name), ("file", file_name), ("n", n.to_string()), ("layers", r.layers.to_string())]));
    Ok(())
}

pub async fn xattach(c: Ctx) -> R {
    attach(&c).await
}

/// `whichLink(c, verb)`: a link by name, or by picking one of its references.
async fn which_link(c: &Ctx, verb: &str) -> R<Option<String>> {
    let links = xrefs(&c.doc());
    if links.is_empty() {
        c.print(&c.t("xref.none"));
        return Ok(None);
    }
    if let [one] = links.as_slice() {
        return Ok(Some(one.name.clone()));
    }
    loop {
        let opts = Opts { none: true, options: vec![c.opt("N", &c.t("opt.name"))], ..Opts::default() };
        let p = c.entity(c.tv("xref.pick", &[("verb", verb.to_string())]), opts).await?;
        match p {
            Reply::Null => return Ok(None),
            Reply::Opt(k) if k == "N" => {
                let s = match c.string(c.t("xref.name"), Opts::default()).await? {
                    Reply::Str(s) => js::trim(&s).to_string(),
                    _ => String::new(),
                };
                if let Some(hit) = links.iter().find(|l| l.name.to_uppercase() == s.to_uppercase()) {
                    return Ok(Some(hit.name.clone()));
                }
                if !s.is_empty() {
                    c.print(&c.tv("xref.unknown", &[("name", s)]));
                }
            }
            Reply::Pick { id, .. } => {
                let blk = {
                    let d = c.doc();
                    d.get(id)
                        .and_then(|e| if let Geo::Insert(i) = &e.geo { Some(i.block.clone()) } else { None })
                        .filter(|b| is_xref_root(d.blocks.get(b)))
                };
                if let Some(b) = blk {
                    return Ok(Some(b));
                }
                c.print(&c.t("xref.notlink"));
            }
            _ => c.print(&c.t("xref.notlink")),
        }
    }
}

fn key_of(r: &Reply) -> Option<String> {
    match r {
        Reply::Opt(k) | Reply::Str(k) => Some(k.clone()),
        _ => None,
    }
}

pub async fn xref(c: Ctx) -> R {
    let options = vec![
        c.opt("?", "?"),
        c.opt("A", &c.t("opt.attach")),
        c.opt("R", &c.t("opt.reload")),
        c.opt("B", &c.t("opt.bind")),
        c.opt("D", &c.t("opt.detach")),
    ];
    let k = c.keyword(c.t("xref.prompt"), Opts { options, default: Some(Def::Str("?".into())), ..Opts::default() }).await?;
    let Some(o) = key_of(&k).filter(|s| !s.is_empty()) else { return Ok(()) };
    if o == "?" {
        let links = xrefs(&c.doc());
        if links.is_empty() {
            c.print(&c.t("xref.none"));
            return Ok(());
        }
        for l in links {
            let key = if l.loaded { "xref.row.loaded" } else { "xref.row.missing" };
            c.print(&format!(
                "  {}",
                c.tv(key, &[("name", l.name), ("path", l.path), ("refs", l.refs.to_string()), ("n", l.objects.to_string())])
            ));
        }
        return Ok(());
    }
    if o == "A" {
        return attach(&c).await;
    }
    let verb = match o.as_str() {
        "R" => c.t("opt.reload"),
        "B" => c.t("opt.bind"),
        _ => c.t("opt.detach"),
    }
    .to_lowercase();
    let Some(name) = which_link(&c, &verb).await? else { return Ok(()) };
    let lg = lang(&c);
    if o == "R" {
        let path = {
            let d = c.doc();
            d.blocks
                .get(&name)
                .and_then(|b| b.xref.as_ref())
                .and_then(|x| x.get("path"))
                .filter(|p| js::truthy(Some(p)))
                .map(js::to_js_string)
                .unwrap_or_else(|| name.clone())
        };
        if !c.sh().host.borrow().can_pick_files() {
            c.print(&c.t("xref.nobrowser"));
            return Ok(());
        }
        c.print(&c.tv("xref.reload.which", &[("name", name.clone()), ("path", path)]));
        let Some(Some(got)) = c.pick_drawing(PICK_DRAWING.into()).await? else { return Ok(()) };
        let file_name = got.file_name.clone();
        let r = c.change("XREF", |d| attach_xref(d, got.drawing, &got.file_name, Some(&name), &lg))?;
        let n = c.doc().blocks.get(&name).map_or(0, |b| b.entities.len());
        c.print(&c.tv("xref.reloaded", &[("name", name.clone()), ("file", file_name), ("n", n.to_string())]));
        if !r.note.is_empty() {
            c.print(&r.note);
        }
    } else if o == "B" {
        // WL.17: references clipped with XCLIP may keep only their clipped part
        let clipped: Vec<(SpaceId, Entity, Vec<P2>)> = {
            let d = c.doc();
            let mut v = Vec::new();
            for sp in std::iter::once(MODEL).chain(d.layouts.iter().map(|l| l.space)) {
                for e in d.space_entities(sp) {
                    if let Geo::Insert(i) = &e.geo
                        && i.block == name
                        && let Some(cp) = clip_poly(e)
                    {
                        v.push((sp, e.clone(), cp));
                    }
                }
            }
            v
        };
        let mut only = false;
        if !clipped.is_empty() {
            let options = vec![c.opt("A", &c.t("opt.all")), c.opt("C", &c.t("opt.clipped"))];
            let k = c
                .keyword(
                    c.tv("xref.bind.which", &[("n", clipped.len().to_string())]),
                    Opts { options, default: Some(Def::Str("A".into())), ..Opts::default() },
                )
                .await?;
            let Some(v) = key_of(&k).filter(|s| !s.is_empty()) else { return Ok(()) };
            only = v == "C";
        }
        let n_clipped = clipped.len();
        let cut = c.change("XREF", |d| {
            bind_xref(d, &name)?;
            let mut cut = 0usize;
            if !only {
                return Ok(cut);
            }
            let Some(root) = d.blocks.get(&name).cloned() else { return Ok(cut) };
            for (sp, e, cp) in &clipped {
                let r = clip_entities(&d.blocks, &root.entities, cp);
                let mut n: u32 = 1;
                while n < 1_000_000 && d.blocks.contains_key(&format!("{name}$clip{n}")) {
                    n = n.saturating_add(1);
                }
                let bname = format!("{name}$clip{n}");
                let mut nb = Block::new(&bname, root.base, r.entities);
                nb.attdefs = root.attdefs.clone();
                d.define_block(&nb)?;
                let mut ne = e.clone();
                ne.extra.remove("clip");
                if let Geo::Insert(i) = &mut ne.geo {
                    i.block = bname;
                }
                if let Some(id) = e.id {
                    d.update_in(id, ne, *sp)?;
                }
                cut = cut.saturating_add(r.cut);
            }
            Ok(cut)
        })?;
        let key = if only { "xref.bound.clipped" } else { "xref.bound" };
        c.print(&c.tv(key, &[("name", name), ("n", n_clipped.to_string()), ("cut", cut.to_string())]));
    } else {
        c.change("XREF", |d| detach_xref(d, &name, false, false))?;
        c.print(&c.tv("xref.detached", &[("name", name)]));
    }
    Ok(())
}

/// XCLIP (xclip.js `installXclip`): New boundary (Rectangular / Polygonal) or Delete, on block references.
pub async fn xclip(c: Ctx) -> R {
    let set = c.select(Some(c.t("xclip.select")), false).await?;
    let ids: Vec<u64> = {
        let d = c.doc();
        set.0
            .iter()
            .copied()
            .filter(|id| d.get(*id).is_some_and(|e| matches!(&e.geo, Geo::Insert(i) if d.blocks.contains_key(&i.block))))
            .collect()
    };
    if ids.is_empty() {
        c.print(&c.t("xclip.none"));
        return Ok(());
    }
    let options = vec![c.opt("N", &c.t("opt.new")), c.opt("D", &c.t("opt.delete"))];
    let k = c.keyword(c.t("xclip.prompt"), Opts { options, default: Some(Def::Str("N".into())), ..Opts::default() }).await?;
    let Some(o) = key_of(&k).filter(|s| !s.is_empty()) else { return Ok(()) };
    let mut world: Option<Vec<P2>> = None;
    if o == "N" {
        let options = vec![c.opt("R", &c.t("opt.rectangular")), c.opt("P", &c.t("opt.polygonal"))];
        let s = c.keyword(c.t("xclip.shape"), Opts { options, default: Some(Def::Str("R".into())), ..Opts::default() }).await?;
        let Some(sh) = key_of(&s).filter(|s| !s.is_empty()) else { return Ok(()) };
        world = Some(if sh == "R" {
            let Some(a) = c.point(c.t("xclip.corner1"), Opts::default()).await?.point() else { return Ok(()) };
            let Some(b) = c.point(c.t("xclip.corner2"), Opts { base: Some(a), ..Opts::default() }).await?.point() else { return Ok(()) };
            vec![a, P2::new(b.x, a.y), b, P2::new(a.x, b.y)]
        } else {
            let pts = polygon(&c, "xclip.vertex", "xclip.vertex_or_enter").await?;
            let Some(pts) = pts else { return Ok(()) };
            pts
        });
    }
    let n = ids.len();
    c.change("XCLIP", |d| {
        for id in &ids {
            let Some(e) = d.get(*id).cloned() else { continue };
            let Geo::Insert(ins) = &e.geo else { continue };
            let Some(blk) = d.blocks.get(&ins.block) else { continue };
            let m = insert_matrix(ins, blk);
            let det = m[0] * m[3] - m[1] * m[2];
            let mut rest = e.clone();
            rest.extra.remove("clip");
            match &world {
                Some(w) if det.abs() >= 1e-300 => {
                    let inv = [
                        m[3] / det,
                        -m[1] / det,
                        -m[2] / det,
                        m[0] / det,
                        (m[2] * m[5] - m[3] * m[4]) / det,
                        (m[1] * m[4] - m[0] * m[5]) / det,
                    ];
                    let pts: Vec<Value> = w
                        .iter()
                        .map(|q| {
                            let r = m_apply(&inv, *q);
                            json!({ "x": num(r.x), "y": num(r.y) })
                        })
                        .collect();
                    rest.extra.insert("clip".into(), json!({ "pts": pts }));
                }
                _ => {}
            }
            d.update(*id, rest)?;
        }
        Ok(())
    })?;
    c.print(&c.tv(if world.is_some() { "xclip.done" } else { "xclip.deleted" }, &[("n", n.to_string())]));
    Ok(())
}

/// The polygonal boundary loop (XCLIP and IMAGECLIP): points until Enter after three; None when cut short.
pub(crate) async fn polygon(c: &Ctx, first: &str, more: &str) -> R<Option<Vec<P2>>> {
    let mut pts: Vec<P2> = Vec::new();
    loop {
        let opts = Opts { base: pts.last().copied(), none: pts.len() >= 3, flat_base: true, ..Opts::default() };
        let q = c.point(c.t(if pts.len() < 3 { first } else { more }), opts).await?;
        match q {
            Reply::Null => break,
            Reply::Point(p, _) => pts.push(p),
            _ => return Ok(None),
        }
    }
    Ok(if pts.len() < 3 { None } else { Some(pts) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src() -> Value {
        json!({ "meta": { "units": "meters", "insbase": { "x": 1, "y": 2 } },
            "layers": [{ "name": "0" }, { "name": "Walls", "color": 3 }],
            "blocks": [{ "name": "Door", "entities": [{ "type": "line", "layer": "Walls", "x1": 0, "y1": 0, "x2": 1, "y2": 0 }] }],
            "entities": [{ "id": 4, "type": "insert", "layer": "Walls", "block": "Door", "x": 0, "y": 0 }, { "type": "circle", "cx": 0, "cy": 0, "r": 1 }] })
    }

    #[test]
    fn attach_names_layers_and_parts_then_binds_and_detaches() {
        let lang = Lang::new(&json!({ "xref.units": "{a} {b} {scale}" }));
        let mut d = Doc::new();
        d.meta.insert("units".into(), json!("millimeters"));
        let r = d.change("XATTACH", |d| attach_xref(d, XSource::from_value(&src()), "C:\\x\\Plan.dwg", None, &lang)).unwrap();
        assert_eq!((r.name.as_str(), r.layers, r.blocks, r.scale), ("Plan", 1, 1, 1000.0));
        assert_eq!(r.note, "meters millimeters 1000");
        assert!(d.layers.contains_key("Plan|Walls"));
        let root = d.blocks.get("Plan").unwrap();
        assert_eq!(root.base, P2::new(1.0, 2.0));
        assert!(matches!(&root.entities[0].geo, Geo::Insert(i) if i.block == "Plan|Door"));
        assert_eq!(root.entities[1].layer.as_deref(), Some("0"));
        assert_eq!(xrefs(&d)[0].objects, 2);
        // a second link of the same file gets a name of its own
        let r2 = d.change("XATTACH", |d| attach_xref(d, XSource::from_value(&src()), "Plan.dxf", None, &lang)).unwrap();
        assert_eq!(r2.name, "Plan (2)");
        d.change("XREF", |d| bind_xref(d, "Plan")).unwrap();
        assert!(d.blocks.contains_key("Plan$0$Door") && d.layers.contains_key("Plan$0$Walls") && !is_xref_root(d.blocks.get("Plan")));
        d.change("XREF", |d| detach_xref(d, "Plan (2)", false, false)).unwrap();
        assert!(!d.blocks.contains_key("Plan (2)") && !d.layers.contains_key("Plan (2)|Walls"));
        d.undo();
        assert!(d.blocks.contains_key("Plan (2)|Door"));
    }

    #[test]
    fn file_names_become_block_names() {
        assert_eq!(block_name_from_file("C:\\a\\b/My*Plan:1.dwg"), "My_Plan_1");
        assert_eq!(block_name_from_file("***.dwg"), "___");
        assert_eq!(block_name_from_file(".dwg"), "Drawing");
        assert_eq!(block_name_from_file("x."), "x.");
        assert_eq!(block_name_from_file(""), "Drawing");
        assert_eq!(block_name_from_file("noext"), "noext");
    }

    #[test]
    fn hostile_sources_attach_without_panicking() {
        let lang = Lang::new(&json!({}));
        let mut d = Doc::new();
        for s in [
            json!(null),
            json!({ "layers": 5, "blocks": [7, null, { "name": 3 }], "entities": ["x", { "block": 9 }] }),
            json!({ "meta": { "units": 3 } }),
        ] {
            let _ = d.change("XATTACH", |d| attach_xref(d, XSource::from_value(&s), "x.dwg", None, &lang));
        }
        assert!(xrefs(&d).len() >= 3);
    }
}
