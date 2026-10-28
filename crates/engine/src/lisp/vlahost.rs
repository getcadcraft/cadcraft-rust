//! The drawing as objects for vla- / vlax- (vla-host.js). Handles are `{ kind, key, tag }`:
//!
//!   App · Doc · Documents · Blocks · Layers · Layouts · Linetypes · TextStyles · DimStyles · Views   (collections)
//!   Block {key: name}   model space is "*Model_Space", the sheets "*Paper_Space", "*Paper_Space0", …
//!   Layer {key: name} · Layout {key: "Model" | sheet name} · Linetype / TextStyle / DimStyle / View {key: name}
//!   Ent {key: id}  an object of the model or a sheet;  Ent {key: "m:<block>:<n>"}  an object inside a block definition
//!   Att {key: id, tag}  an attribute of a block reference
//!
//! Objects inside block definitions get a serial number that follows them when the block is redefined here (deleting
//! one while vlax-for walks the block must not shift the others): 0.5 keeps them in a WeakMap of the objects; here they
//! sit beside the block's list of objects (a list shared copy-on-write, so a redefinition elsewhere starts afresh and
//! an undo that brings an old list back brings its numbers back). Every change goes through `doc.change`.

use cc_cad::entity::{Entity, Geo, Obj};
use cc_cad::{Block, Doc, Ents, MODEL, SpaceId, js};
use cc_cad_geom::geom::{m_about, m_mirror, m_rotate, m_scale, m_translate};
use cc_cad_geom::jsmath::JsMath;
use cc_cad_geom::pick::{entity_bounds, valid_bounds};
use cc_geom::P2;
use cc_lisp::host::{HR, HostErr, Pv, VlaOp};
use cc_lisp::value::{EId, VKey, VlaH};
use serde_json::{Value, json};

use super::dxflist;
use super::host::CadHost;
use crate::engine::Shared;

/// The serial numbers of block members: per block, per version of its list.
#[derive(Default)]
pub struct VlaState {
    lists: Vec<(String, Ents, Vec<Option<u64>>)>,
    next: u64,
}

impl VlaState {
    fn entry(&mut self, name: &str, ents: &Ents) -> usize {
        if let Some(k) = self.lists.iter().position(|(n, e, _)| n == name && Ents::ptr_eq(e, ents)) {
            return k;
        }
        self.lists.push((name.to_string(), ents.clone(), vec![None; ents.len()]));
        self.lists.len().saturating_sub(1)
    }
    /// The serial number of a block's member (made on first use).
    fn serial_of(&mut self, name: &str, ents: &Ents, idx: usize) -> u64 {
        let k = self.entry(name, ents);
        let next = &mut self.next;
        let Some((_, _, v)) = self.lists.get_mut(k) else { return 0 };
        if v.len() <= idx {
            v.resize(idx.saturating_add(1), None);
        }
        match v.get(idx).copied().flatten() {
            Some(s) => s,
            None => {
                *next = next.saturating_add(1);
                if let Some(slot) = v.get_mut(idx) {
                    *slot = Some(*next);
                }
                *next
            }
        }
    }
    /// A member by serial number → its index in the block's current list.
    fn index_of(&self, name: &str, ents: &Ents, serial: u64) -> Option<usize> {
        let (_, _, v) = self.lists.iter().find(|(n, e, _)| n == name && Ents::ptr_eq(e, ents))?;
        v.iter().position(|s| *s == Some(serial))
    }
    fn serials(&self, name: &str, ents: &Ents) -> Vec<Option<u64>> {
        self.lists
            .iter()
            .find(|(n, e, _)| n == name && Ents::ptr_eq(e, ents))
            .map(|x| x.2.clone())
            .unwrap_or_else(|| vec![None; ents.len()])
    }
    fn set_serials(&mut self, name: &str, ents: &Ents, serials: Vec<Option<u64>>) {
        let k = self.entry(name, ents);
        if let Some(slot) = self.lists.get_mut(k) {
            slot.2 = serials;
        }
    }
}

fn err(m: &str) -> HostErr {
    HostErr::Msg(format!("Automation Error. {m}"))
}
fn notfound() -> HostErr {
    err("Key not found")
}
fn b(v: bool) -> Pv {
    Pv::Bool(v)
}
fn i(n: f64) -> Pv {
    Pv::Int(cc_lisp::value::to_i32(n))
}
fn n0(x: f64) -> f64 {
    if x.is_nan() { 0.0 } else { x }
}
fn p3(x: f64, y: f64) -> Pv {
    Pv::Arr(vec![Pv::Num(n0(x)), Pv::Num(n0(y)), Pv::Num(0.0)], Some(5))
}
fn h(kind: &str, key: Option<VKey>) -> Pv {
    Pv::Vla(VlaH { kind: kind.into(), key, tag: None })
}
fn hk(kind: &str, key: &str) -> Pv {
    h(kind, Some(VKey::Str(key.to_string())))
}
fn pt_in(v: &Pv) -> Option<P2> {
    let Pv::Arr(items, _) = v else { return None };
    let a: Vec<f64> = items.iter().map(|x| x.num().unwrap_or(f64::NAN)).collect();
    if a.len() >= 2 && a.iter().all(|x| x.is_finite()) { Some(P2::new(a.first().copied()?, a.get(1).copied()?)) } else { None }
}
fn num_in(v: &Pv) -> f64 {
    match v {
        Pv::Int(n) => f64::from(*n),
        Pv::Num(n) => *n,
        _ => f64::NAN,
    }
}
fn bool_in(v: &Pv) -> bool {
    match v {
        Pv::Bool(x) => *x,
        Pv::Int(n) => *n != 0,
        Pv::Num(n) => *n != 0.0,
        Pv::Nil => false,
        Pv::Str(s) => !s.is_empty(),
        _ => true,
    }
}
fn s_of(v: Option<&Pv>) -> String {
    match v {
        None | Some(Pv::Nil) => String::new(),
        Some(x) => dxflist::js_str(x),
    }
}
fn key_eid(k: &Option<VKey>) -> Option<EId> {
    match k {
        Some(VKey::Num(n)) if *n >= 0.0 && n.fract() == 0.0 => {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let u = *n as u64;
            Some(EId::N(u))
        }
        Some(VKey::Str(s)) => Some(EId::S(s.as_str().into())),
        _ => None,
    }
}
fn ent_h(id: u64) -> Pv {
    #[allow(clippy::cast_precision_loss)]
    let k = id as f64;
    h("Ent", Some(VKey::Num(k)))
}

/// The block-table name of a space: model, then each sheet.
fn space_block_name(doc: &Doc, s: SpaceId) -> String {
    if s == MODEL {
        return "*Model_Space".into();
    }
    match doc.layouts.iter().position(|l| l.space == s) {
        Some(0) => "*Paper_Space".into(),
        Some(k) => format!("*Paper_Space{}", k.saturating_sub(1)),
        None => "*Paper_Space-1".into(),
    }
}
fn space_of(doc: &Doc, name: &str) -> Option<SpaceId> {
    let n = name.to_uppercase();
    if n == "*MODEL_SPACE" {
        return Some(MODEL);
    }
    let rest = n.strip_prefix("*PAPER_SPACE")?;
    if !rest.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let k = if rest.is_empty() { 0 } else { rest.parse::<usize>().ok()?.checked_add(1)? };
    doc.layouts.get(k).map(|l| l.space)
}
fn all_block_names(doc: &Doc) -> Vec<String> {
    let mut v = vec!["*Model_Space".to_string()];
    for (k, _) in doc.layouts.iter().enumerate() {
        v.push(if k == 0 { "*Paper_Space".into() } else { format!("*Paper_Space{}", k.saturating_sub(1)) });
    }
    v.extend(doc.blocks.keys().cloned());
    v
}
fn find_block(doc: &Doc, name: &str) -> Option<String> {
    let u = name.to_uppercase();
    all_block_names(doc).into_iter().find(|n| n.to_uppercase() == u)
}
fn layout_names(doc: &Doc) -> Vec<String> {
    let mut v = vec!["Model".to_string()];
    v.extend(doc.layouts.iter().map(|l| l.name.clone()));
    v
}

/// An Ent handle's object: a model / sheet object, or a block member (short-lived: the size is fine).
#[allow(clippy::large_enum_variant)]
enum EntRef {
    Space { e: Entity, space: SpaceId },
    Member { block: Block, idx: usize, e: Entity },
}
impl EntRef {
    fn e(&self) -> &Entity {
        match self {
            EntRef::Space { e, .. } | EntRef::Member { e, .. } => e,
        }
    }
}

struct Ctx<'a> {
    host: &'a CadHost,
    sh: &'a Shared,
}

impl Ctx<'_> {
    fn member(&self, key: &str) -> Option<EntRef> {
        let rest = key.strip_prefix("m:")?;
        let k = rest.rfind(':')?;
        let name = rest.get(..k)?;
        let serial: u64 = rest.get(k.saturating_add(1)..)?.parse().ok()?;
        let b = self.sh.doc.borrow().blocks.get(&name.to_string()).cloned()?;
        let idx = self.host.vla.borrow().index_of(name, &b.entities, serial)?;
        let e = b.entities.get(idx)?.clone();
        Some(EntRef::Member { block: b, idx, e })
    }
    fn ent(&self, hh: &VlaH) -> Option<EntRef> {
        if let Some(VKey::Str(s)) = &hh.key
            && s.starts_with("m:")
        {
            return self.member(s);
        }
        let id = key_eid(&hh.key)?;
        let f = CadHost::find(&self.sh.doc.borrow(), &id)?;
        if f.block.is_some() {
            return None;
        }
        let (space, _) = f.space?;
        Some(EntRef::Space { e: f.e, space })
    }
    fn member_h(&self, name: &str, ents: &Ents, idx: usize) -> Pv {
        let s = self.host.vla.borrow_mut().serial_of(name, ents, idx);
        hk("Ent", &format!("m:{name}:{s}"))
    }
    /// Redefine a block with new members; serial numbers follow their objects.
    fn redefine(&self, b: &Block, entities: Vec<Entity>, serials: Vec<Option<u64>>, label: &str) -> HR<Block> {
        let mut nb = b.clone();
        nb.entities = entities.into();
        let r = self.sh.doc.borrow_mut().change(label, |d| d.define_block(&nb));
        self.sh.flush_notes();
        let nb = r.map_err(|e| HostErr::Msg(e.to_string()))?;
        self.host.vla.borrow_mut().set_serials(&nb.name, &nb.entities, serials);
        Ok(nb)
    }
    fn layer_locked(&self, e: &Entity) -> bool {
        self.sh.doc.borrow().layers.get(e.layer_str()).is_some_and(|l| l.locked)
    }
    /// Replace an object (model, sheet or block member) with a changed copy.
    fn replace(&self, f: &EntRef, next: Entity, label: &str) -> HR<()> {
        match f {
            EntRef::Member { block, idx, .. } => {
                let mut ents = block.entities.to_vec();
                if let Some(slot) = ents.get_mut(*idx) {
                    *slot = next;
                }
                let serials = self.host.vla.borrow().serials(&block.name, &block.entities);
                self.redefine(block, ents, serials, label)?;
                Ok(())
            }
            EntRef::Space { e, space } => {
                self.sh.doc.borrow_mut().ensure_layer(next.layer.as_deref().unwrap_or("0"));
                let id = e.id.unwrap_or(0);
                let r = self.sh.doc.borrow_mut().change(label, |d| d.replace_in(id, next, *space));
                self.sh.flush_notes();
                r.map(|_| ()).map_err(|e| HostErr::Msg(e.to_string()))
            }
        }
    }
    /// Add to a space or a block: `{ layer: current, color: current, ...e }`.
    fn add_to(&self, target: &Target, e: Obj, label: &str) -> HR<Pv> {
        let (layer, color) = {
            let d = self.sh.doc.borrow();
            (d.current.layer.clone(), d.current.color.to_value())
        };
        let mut full = Obj::new();
        full.insert("layer".into(), Value::String(layer));
        full.insert("color".into(), color);
        for (k, v) in e {
            full.insert(k, v);
        }
        let ent = Entity::from_value(&Value::Object(full));
        match target {
            Target::Block(b) => {
                let mut ents = b.entities.to_vec();
                ents.push(ent);
                let mut serials = self.host.vla.borrow().serials(&b.name, &b.entities);
                serials.push(None);
                let nb = self.redefine(b, ents, serials, label)?;
                let last = nb.entities.len().saturating_sub(1);
                Ok(self.member_h(&nb.name, &nb.entities, last))
            }
            Target::Space(s) => {
                let r = self.sh.doc.borrow_mut().change(label, |d| d.add_in_id(ent, *s));
                self.sh.flush_notes();
                let id = r.map_err(|e| HostErr::Msg(e.to_string()))?;
                Ok(ent_h(id))
            }
        }
    }
    fn block_items(&self, name: &str) -> Option<Vec<Pv>> {
        let d = self.sh.doc.borrow();
        if let Some(s) = space_of(&d, name) {
            return Some(d.space_entities(s).filter_map(|e| e.id).map(ent_h).collect());
        }
        let b = d
            .blocks
            .get(&name.to_string())
            .cloned()
            .or_else(|| d.blocks.values().find(|x| x.name.to_uppercase() == name.to_uppercase()).cloned())?;
        drop(d);
        Some((0..b.entities.len()).map(|k| self.member_h(&b.name, &b.entities, k)).collect())
    }
    fn coll_items(&self, kind: &str) -> Option<Vec<VlaH>> {
        let d = self.sh.doc.borrow();
        let mk = |k: &str, n: &str| VlaH { kind: k.into(), key: Some(VKey::Str(n.to_string())), tag: None };
        Some(match kind {
            "Blocks" => all_block_names(&d).iter().map(|n| mk("Block", n)).collect(),
            "Layers" => d.layers.keys().map(|n| mk("Layer", n)).collect(),
            "Layouts" => layout_names(&d).iter().map(|n| mk("Layout", n)).collect(),
            "Linetypes" => {
                let mut v: Vec<String> = vec!["ByBlock".into(), "ByLayer".into(), "Continuous".into()];
                v.extend(d.linetypes.keys().cloned());
                v.iter().map(|n| mk("Linetype", n)).collect()
            }
            "TextStyles" => vec![mk("TextStyle", "Standard")],
            "DimStyles" => vec![mk("DimStyle", "Standard")],
            "Views" => d.views.iter().map(|v| mk("View", &v.name)).collect(),
            "Documents" => vec![VlaH { kind: "Doc".into(), key: None, tag: None }],
            _ => return None,
        })
    }
    fn item_of(&self, kind: &str, which: &Pv) -> Option<HR<VlaH>> {
        let items = self.coll_items(kind)?;
        let n = num_in(which);
        if n.is_finite() && !matches!(which, Pv::Str(_)) {
            let hit = (n >= 0.0 && n.fract() == 0.0).then(|| {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let k = n as usize;
                items.get(k).cloned()
            });
            return Some(hit.flatten().ok_or_else(notfound));
        }
        let s = dxflist::js_str(which).to_uppercase();
        Some(
            items
                .into_iter()
                .find(|x| x.key.as_ref().map_or_else(|| "UNDEFINED".to_string(), |k| k.text().to_uppercase()) == s)
                .ok_or_else(notfound),
        )
    }

    // ---- get ----

    fn get(&self, hh: &VlaH, p: &str) -> HR<Pv> {
        let key = hh.key.as_ref().map(VKey::text).unwrap_or_default();
        match hh.kind.as_str() {
            "App" => match p {
                "ACTIVEDOCUMENT" => return Ok(h("Doc", None)),
                "DOCUMENTS" => return Ok(h("Documents", None)),
                "NAME" | "CAPTION" => return Ok(Pv::Str("CadCraft".into())),
                "VERSION" => return Ok(Pv::Str("24.0".into())),
                "APPLICATION" => return Ok(h("App", None)),
                _ => {}
            },
            "Doc" => {
                let coll = match p {
                    "BLOCKS" => Some("Blocks"),
                    "LAYERS" => Some("Layers"),
                    "LAYOUTS" => Some("Layouts"),
                    "LINETYPES" => Some("Linetypes"),
                    "TEXTSTYLES" => Some("TextStyles"),
                    "DIMSTYLES" => Some("DimStyles"),
                    "VIEWS" => Some("Views"),
                    _ => None,
                };
                if let Some(cl) = coll {
                    return Ok(h(cl, None));
                }
                let d = self.sh.doc.borrow();
                let name = d.meta.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("Drawing1").to_string();
                match p {
                    "NAME" | "FULLNAME" => return Ok(Pv::Str(format!("{name}.dwg"))),
                    "PATH" => return Ok(Pv::Str(String::new())),
                    "MODELSPACE" => return Ok(hk("Block", "*Model_Space")),
                    "PAPERSPACE" => {
                        let n = if d.layout().is_some() { space_block_name(&d, d.space) } else { "*Paper_Space".into() };
                        return Ok(hk("Block", &n));
                    }
                    "ACTIVELAYER" => return Ok(hk("Layer", &d.current.layer)),
                    "ACTIVELAYOUT" => return Ok(hk("Layout", &d.layout().map_or_else(|| "Model".to_string(), |l| l.name.clone()))),
                    "ACTIVESPACE" => return Ok(Pv::Int(if d.layout().is_some() { 0 } else { 1 })),
                    "SAVED" => return Ok(b(!d.dirty())),
                    "OBJECTNAME" => return Ok(Pv::Str("AcDbDatabase".into())),
                    "APPLICATION" => return Ok(h("App", None)),
                    "DATABASE" => return Ok(h("Doc", None)),
                    _ => {}
                }
            }
            "Documents" | "Blocks" | "Layers" | "Layouts" | "Linetypes" | "TextStyles" | "DimStyles" | "Views" => {
                if p == "COUNT" {
                    return Ok(i(self.coll_items(&hh.kind).map_or(0, |v| v.len()) as f64));
                }
                if p == "APPLICATION" {
                    return Ok(h("App", None));
                }
            }
            "Block" => {
                let d = self.sh.doc.borrow();
                let Some(name) = find_block(&d, &key) else { return Err(err("Object was erased")) };
                let sp = space_of(&d, &name);
                let bl = if sp.is_some() { None } else { d.blocks.get(&name).cloned() };
                let xref = bl.as_ref().and_then(|b| b.xref.clone()).filter(Value::is_object);
                match p {
                    "NAME" => return Ok(Pv::Str(name)),
                    "COUNT" => {
                        return Ok(i(
                            sp.map_or_else(|| bl.as_ref().map_or(0, |b| b.entities.len()), |s| d.space_entities(s).count()) as f64
                        ));
                    }
                    "ISLAYOUT" => return Ok(b(sp.is_some())),
                    "ISXREF" => return Ok(b(xref.as_ref().is_some_and(|x| !js::truthy(x.get("of"))))),
                    "ISDYNAMICBLOCK" => return Ok(b(false)),
                    "ISANONYMOUS" => return Ok(b(name.starts_with('*') && sp.is_none())),
                    "ORIGIN" => return Ok(bl.as_ref().map_or(p3(0.0, 0.0), |b| p3(b.base.x, b.base.y))),
                    "OBJECTNAME" => return Ok(Pv::Str("AcDbBlockTableRecord".into())),
                    "COMMENTS" => return Ok(Pv::Str(String::new())),
                    "PATH" => {
                        return Ok(Pv::Str(
                            xref.as_ref()
                                .and_then(|x| x.get("path"))
                                .filter(|v| js::truthy(Some(v)))
                                .map(js::to_js_string)
                                .unwrap_or_default(),
                        ));
                    }
                    "LAYOUT" => {
                        let Some(s) = sp else { return Err(err("Not applicable")) };
                        let k = if name == "*Model_Space" {
                            Some("Model".to_string())
                        } else {
                            d.layouts.iter().find(|l| l.space == s).map(|l| l.name.clone())
                        };
                        return Ok(h("Layout", k.map(VKey::Str)));
                    }
                    "HANDLE" => return Ok(Pv::Str(name)),
                    _ => {}
                }
            }
            "Layer" => {
                let d = self.sh.doc.borrow();
                let Some(l) = d.layers.get(&key) else { return Err(err("Object was erased")) };
                let lv = l.to_value();
                match p {
                    "NAME" => return Ok(Pv::Str(l.name.clone())),
                    "COLOR" => return Ok(i(lv.get("color").and_then(Value::as_f64).unwrap_or(7.0))),
                    "LOCK" => return Ok(b(l.locked)),
                    "FREEZE" => return Ok(b(l.frozen)),
                    "LAYERON" => return Ok(b(l.on)),
                    "PLOTTABLE" => return Ok(b(l.plot)),
                    "LINETYPE" => return Ok(Pv::Str(l.linetype.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "Continuous".into()))),
                    "LINEWEIGHT" => {
                        return Ok(i(l.lineweight.filter(|w| w.is_finite() && *w >= 0.0).map_or(-3.0, |w| js::round(w * 100.0))));
                    }
                    "DESCRIPTION" => return Ok(Pv::Str(String::new())),
                    "USED" => return Ok(b(d.every_entity().any(|e| e.layer_str() == l.name))),
                    "OBJECTNAME" => return Ok(Pv::Str("AcDbLayerTableRecord".into())),
                    "HANDLE" => return Ok(Pv::Str(format!("L:{}", l.name))),
                    _ => {}
                }
            }
            "Layout" => {
                let d = self.sh.doc.borrow();
                let is_model = key == "Model";
                let l = if is_model { None } else { d.layouts.iter().position(|x| x.name == key) };
                if !is_model && l.is_none() {
                    return Err(err("Object was erased"));
                }
                match p {
                    "NAME" => return Ok(Pv::Str(if is_model { "Model".into() } else { key.clone() })),
                    "BLOCK" => {
                        let n = match l.and_then(|k| d.layouts.get(k)) {
                            Some(lay) => space_block_name(&d, lay.space),
                            None => "*Model_Space".into(),
                        };
                        return Ok(hk("Block", &n));
                    }
                    "TABORDER" => return Ok(i(l.map_or(0.0, |k| f64::from(u32::try_from(k.saturating_add(1)).unwrap_or(u32::MAX))))),
                    "MODELTYPE" => return Ok(b(is_model)),
                    "OBJECTNAME" => return Ok(Pv::Str("AcDbLayout".into())),
                    _ => {}
                }
            }
            "Linetype" | "TextStyle" | "DimStyle" | "View" => {
                if p == "NAME" {
                    return Ok(Pv::Str(key));
                }
                if p == "OBJECTNAME" {
                    return Ok(Pv::Str(
                        match hh.kind.as_str() {
                            "Linetype" => "AcDbLinetypeTableRecord",
                            "TextStyle" => "AcDbTextStyleTableRecord",
                            "DimStyle" => "AcDbDimStyleTableRecord",
                            _ => "AcDbViewTableRecord",
                        }
                        .into(),
                    ));
                }
            }
            "Att" => {
                let id = key_eid(&hh.key);
                let f = id.and_then(|id| CadHost::find(&self.sh.doc.borrow(), &id));
                let Some(f) = f.filter(|f| matches!(f.e.geo, Geo::Insert(_))) else { return Err(err("Object was erased")) };
                let Geo::Insert(ins) = &f.e.geo else { return Err(err("Object was erased")) };
                let tag = hh.tag.clone().unwrap_or_default();
                match p {
                    "TAGSTRING" => return Ok(Pv::Str(tag)),
                    "TEXTSTRING" => {
                        return Ok(Pv::Str(
                            ins.attrs.as_ref().and_then(|a| a.get(&tag)).filter(|v| !v.is_null()).map(js::to_js_string).unwrap_or_default(),
                        ));
                    }
                    "OBJECTNAME" => return Ok(Pv::Str("AcDbAttribute".into())),
                    "LAYER" => return Ok(f.e.layer.clone().map_or(Pv::Nil, Pv::Str)),
                    "INVISIBLE" => {
                        let d = self.sh.doc.borrow();
                        let inv = d
                            .blocks
                            .get(&ins.block)
                            .and_then(|b| b.attdefs.as_ref())
                            .and_then(|a| a.iter().find(|x| x.get("tag").map(js::to_js_string).as_deref() == Some(tag.as_str())))
                            .is_some_and(|x| js::truthy(x.get("invisible")));
                        return Ok(b(inv));
                    }
                    _ => {}
                }
            }
            "Ent" => {
                let Some(f) = self.ent(hh) else { return Err(err("Object was erased")) };
                let e = f.e();
                let o = e.to_obj();
                let g = |k: &str| js::to_number_opt(o.get(k));
                match p {
                    "OBJECTNAME" | "ENTITYNAME" => return Ok(Pv::Str(dxflist::object_name(&o))),
                    "HANDLE" => {
                        return Ok(Pv::Str(match &hh.key {
                            Some(VKey::Num(_)) => format!("{:X}", key_eid(&hh.key).and_then(|x| x.num()).unwrap_or(0)),
                            Some(VKey::Str(s)) => s.clone(),
                            None => String::new(),
                        }));
                    }
                    "LAYER" => return Ok(Pv::Str(e.layer.clone().unwrap_or_else(|| "0".into()))),
                    "COLOR" => {
                        return Ok(i(match o.get("color") {
                            Some(Value::Number(n)) => n.as_f64().unwrap_or(256.0),
                            _ => 256.0,
                        }));
                    }
                    "LINETYPE" => {
                        return Ok(Pv::Str(
                            o.get("lt").filter(|v| js::truthy(Some(v))).map(js::to_js_string).unwrap_or_else(|| "ByLayer".into()),
                        ));
                    }
                    "LINEWEIGHT" => {
                        return Ok(i(match js::finite(o.get("lw")) {
                            Some(w) if w < 0.0 => w,
                            Some(w) => js::round(w * 100.0),
                            None => -1.0,
                        }));
                    }
                    "LINETYPESCALE" => return Ok(Pv::Num(js::or(o.get("lts").and_then(Value::as_f64), 1.0))),
                    "VISIBLE" => return Ok(b(true)),
                    "OWNERID" => return Ok(Pv::Int(0)),
                    _ => {}
                }
                let ty = dxflist::dxf_type(&o);
                let tau = std::f64::consts::PI * 2.0;
                match ty.as_str() {
                    "LINE" => match p {
                        "STARTPOINT" => return Ok(p3(g("x1"), g("y1"))),
                        "ENDPOINT" => return Ok(p3(g("x2"), g("y2"))),
                        "LENGTH" => return Ok(Pv::Num(js::hypot(g("x2") - g("x1"), g("y2") - g("y1")))),
                        "ANGLE" => {
                            let a = (g("y2") - g("y1")).jatan2(g("x2") - g("x1"));
                            return Ok(Pv::Num(if a < 0.0 { a + tau } else { a }));
                        }
                        "DELTA" => return Ok(p3(g("x2") - g("x1"), g("y2") - g("y1"))),
                        _ => {}
                    },
                    "CIRCLE" | "ARC" => {
                        let r = g("r");
                        match p {
                            "CENTER" => return Ok(p3(g("cx"), g("cy"))),
                            "RADIUS" => return Ok(Pv::Num(r)),
                            _ => {}
                        }
                        if ty == "CIRCLE" {
                            match p {
                                "DIAMETER" => return Ok(Pv::Num(2.0 * r)),
                                "AREA" => return Ok(Pv::Num(std::f64::consts::PI * r * r)),
                                "CIRCUMFERENCE" => return Ok(Pv::Num(2.0 * std::f64::consts::PI * r)),
                                _ => {}
                            }
                        } else {
                            let (a0, a1) = (g("a0"), g("a1"));
                            let sweep = (((a1 - a0) % tau) + tau) % tau;
                            match p {
                                "STARTANGLE" => return Ok(Pv::Num(a0)),
                                "ENDANGLE" => return Ok(Pv::Num(a1)),
                                "TOTALANGLE" => return Ok(Pv::Num(sweep)),
                                "ARCLENGTH" => return Ok(Pv::Num(r * sweep)),
                                "STARTPOINT" => return Ok(p3(g("cx") + r * a0.jcos(), g("cy") + r * a0.jsin())),
                                "ENDPOINT" => return Ok(p3(g("cx") + r * a1.jcos(), g("cy") + r * a1.jsin())),
                                "AREA" => return Ok(Pv::Num(cc_cad_geom::edit::entity_area(e).map_or(0.0, |a| a.0))),
                                _ => {}
                            }
                        }
                    }
                    "LWPOLYLINE" => match p {
                        "COORDINATES" => {
                            let pts = o.get("pts").and_then(Value::as_array).cloned().unwrap_or_default();
                            let mut v = Vec::new();
                            for q in &pts {
                                v.push(Pv::Num(js::to_number_opt(q.get("x"))));
                                v.push(Pv::Num(js::to_number_opt(q.get("y"))));
                            }
                            return Ok(Pv::Arr(v, Some(5)));
                        }
                        "CLOSED" => return Ok(b(js::truthy(o.get("closed")))),
                        "CONSTANTWIDTH" => return Ok(Pv::Num(js::or(o.get("w").and_then(Value::as_f64), 0.0))),
                        "ELEVATION" => return Ok(Pv::Num(0.0)),
                        "AREA" => return Ok(Pv::Num(cc_cad_geom::edit::entity_area(e).map_or(0.0, |a| a.0))),
                        "LENGTH" => return Ok(Pv::Num(cc_cad_geom::edit::curve_length(e).unwrap_or(0.0))),
                        _ => {}
                    },
                    "ELLIPSE" => match p {
                        "CENTER" => return Ok(p3(g("cx"), g("cy"))),
                        "MAJORAXIS" => return Ok(p3(g("mx"), g("my"))),
                        "RADIUSRATIO" => return Ok(Pv::Num(g("ratio"))),
                        "STARTANGLE" => return Ok(Pv::Num(o.get("t0").and_then(Value::as_f64).unwrap_or(0.0))),
                        "ENDANGLE" => return Ok(Pv::Num(o.get("t1").and_then(Value::as_f64).unwrap_or(tau))),
                        "MAJORRADIUS" => return Ok(Pv::Num(js::hypot(g("mx"), g("my")))),
                        "MINORRADIUS" => return Ok(Pv::Num(js::hypot(g("mx"), g("my")) * g("ratio"))),
                        _ => {}
                    },
                    "TEXT" | "MTEXT" => match p {
                        "TEXTSTRING" => {
                            let s = o.get("s").filter(|v| !v.is_null()).map(js::to_js_string).unwrap_or_default();
                            return Ok(Pv::Str(if ty == "MTEXT" { s.replace('\n', "\\P") } else { s }));
                        }
                        "INSERTIONPOINT" | "TEXTALIGNMENTPOINT" => return Ok(p3(g("x"), g("y"))),
                        "HEIGHT" => return Ok(Pv::Num(g("h"))),
                        "ROTATION" => return Ok(Pv::Num(js::or(o.get("rot").and_then(Value::as_f64), 0.0))),
                        "SCALEFACTOR" => return Ok(Pv::Num(js::or(o.get("wf").and_then(Value::as_f64), 1.0))),
                        "STYLENAME" => return Ok(Pv::Str("Standard".into())),
                        "WIDTH" if ty == "MTEXT" => return Ok(Pv::Num(js::or(o.get("ww").and_then(Value::as_f64), 0.0))),
                        _ => {}
                    },
                    "INSERT" => match p {
                        "NAME" | "EFFECTIVENAME" => return Ok(Pv::Str(o.get("block").map(js::to_js_string).unwrap_or_default())),
                        "INSERTIONPOINT" => return Ok(p3(g("x"), g("y"))),
                        "ROTATION" => return Ok(Pv::Num(js::or(o.get("rot").and_then(Value::as_f64), 0.0))),
                        "XSCALEFACTOR" | "XEFFECTIVESCALEFACTOR" | "ZSCALEFACTOR" | "ZEFFECTIVESCALEFACTOR" => {
                            return Ok(Pv::Num(o.get("sx").filter(|v| !v.is_null()).map_or(1.0, js::to_number)));
                        }
                        "YSCALEFACTOR" | "YEFFECTIVESCALEFACTOR" => {
                            return Ok(Pv::Num(o.get("sy").filter(|v| !v.is_null()).map_or(1.0, js::to_number)));
                        }
                        "HASATTRIBUTES" => {
                            let has_vals = o.get("attrs").and_then(Value::as_object).is_some_and(|a| !a.is_empty());
                            let name = o.get("block").map(js::to_js_string).unwrap_or_default();
                            let has_defs =
                                self.sh.doc.borrow().blocks.get(&name).and_then(|b| b.attdefs.as_ref()).is_some_and(|a| !a.is_empty());
                            return Ok(b(has_vals || has_defs));
                        }
                        "ISDYNAMICBLOCK" => return Ok(b(false)),
                        _ => {}
                    },
                    "POINT" => {
                        if p == "COORDINATES" {
                            return Ok(p3(g("x"), g("y")));
                        }
                    }
                    "DIMENSION" => match p {
                        "MEASUREMENT" => return Ok(Pv::Num(cc_cad_geom::dims::dim_measure(e))),
                        "TEXTOVERRIDE" => {
                            return Ok(Pv::Str(o.get("text").filter(|v| js::truthy(Some(v))).map(js::to_js_string).unwrap_or_default()));
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
            _ => {}
        }
        Err(err(&format!("Unknown property: {p}")))
    }

    // ---- put ----

    fn put(&self, hh: &VlaH, p: &str, v: &Pv) -> HR<Pv> {
        let key = hh.key.as_ref().map(VKey::text).unwrap_or_default();
        match hh.kind.as_str() {
            "Doc" => {
                if p == "ACTIVELAYER" {
                    let k = match v {
                        Pv::Vla(x) => x.key.as_ref().map(VKey::text),
                        _ => None,
                    };
                    let Some(k) = k.filter(|k| self.sh.doc.borrow().layers.contains_key(k.as_str())) else {
                        return Err(err("Invalid input"));
                    };
                    let r = self.sh.doc.borrow_mut().set_layer(&k, &Obj::new(), true);
                    self.sh.flush_notes();
                    r.map_err(|e| HostErr::Msg(e.to_string()))?;
                    return Ok(Pv::T);
                }
            }
            "Layer" => {
                let l = self.sh.doc.borrow().layers.get(&key).cloned();
                let Some(l) = l else { return Err(err("Object was erased")) };
                let n = num_in(v);
                let patch: Option<(&str, Value)> = match p {
                    "NAME" => Some(("name", Value::String(dxflist::js_str(v)))),
                    "COLOR" => Some(("color", cc_cad::entity::num(n))),
                    "LOCK" => Some(("locked", Value::Bool(bool_in(v)))),
                    "FREEZE" => Some(("frozen", Value::Bool(bool_in(v)))),
                    "LAYERON" => Some(("on", Value::Bool(bool_in(v)))),
                    "PLOTTABLE" => Some(("plot", Value::Bool(bool_in(v)))),
                    "LINETYPE" => Some(("linetype", Value::String(dxflist::js_str(v)))),
                    "LINEWEIGHT" => Some(("lineweight", cc_cad::entity::num(if n < 0.0 { n } else { n / 100.0 }))),
                    _ => None,
                };
                if let Some((k, val)) = patch {
                    if p == "FREEZE" && bool_in(v) && self.sh.doc.borrow().current.layer == l.name {
                        return Err(err("Invalid layer"));
                    }
                    if p == "NAME" && (l.name == "0" || self.sh.doc.borrow().layers.contains_key(dxflist::js_str(v).as_str())) {
                        return Err(err("Duplicate record name"));
                    }
                    let mut o = Obj::new();
                    o.insert(k.into(), val);
                    let r = self.sh.doc.borrow_mut().set_layer(&l.name, &o, false);
                    self.sh.flush_notes();
                    r.map_err(|e| HostErr::Msg(e.to_string()))?;
                    return Ok(Pv::T);
                }
            }
            "Att" => {
                let id = key_eid(&hh.key);
                let f = id.and_then(|id| CadHost::find(&self.sh.doc.borrow(), &id));
                let Some(f) = f.filter(|f| matches!(f.e.geo, Geo::Insert(_))) else { return Err(err("Object was erased")) };
                if p == "TEXTSTRING" {
                    let mut ne = f.e.clone();
                    if let Geo::Insert(g) = &mut ne.geo {
                        let mut a = g.attrs.clone().unwrap_or_default();
                        a.insert(hh.tag.clone().unwrap_or_default(), Value::String(dxflist::js_str(v)));
                        g.attrs = Some(a);
                    }
                    let (space, _) = f.space.unwrap_or((MODEL, String::new()));
                    self.replace(&EntRef::Space { e: f.e, space }, ne, "VLA")?;
                    return Ok(Pv::T);
                }
            }
            "Ent" => {
                let Some(f) = self.ent(hh) else { return Err(err("Object was erased")) };
                if self.layer_locked(f.e()) {
                    return Err(err("On locked layer"));
                }
                let mut o = f.e().to_obj();
                let pt = pt_in(v);
                let n = num_in(v);
                let ty = o.get("type").and_then(Value::as_str).unwrap_or("").to_string();
                let set = |o: &mut Obj, k: &str, x: f64| {
                    o.insert(k.into(), cc_cad::entity::num(x));
                };
                match p {
                    "LAYER" => {
                        let Pv::Str(s) = v else { return Err(err("Invalid input")) };
                        if s.is_empty() {
                            return Err(err("Invalid input"));
                        }
                        o.insert("layer".into(), Value::String(s.clone()));
                    }
                    "COLOR" => {
                        o.insert("color".into(), if n == 256.0 { Value::Null } else { cc_cad::entity::num(n) });
                    }
                    "LINETYPE" => {
                        let s = dxflist::js_str(v);
                        if s.eq_ignore_ascii_case("bylayer") {
                            o.remove("lt");
                        } else {
                            o.insert("lt".into(), Value::String(s));
                        }
                    }
                    "LINEWEIGHT" => {
                        if n == -1.0 {
                            o.remove("lw");
                        } else {
                            set(&mut o, "lw", if n < 0.0 { n } else { n / 100.0 });
                        }
                    }
                    "LINETYPESCALE" => set(&mut o, "lts", n),
                    "STARTPOINT" | "ENDPOINT" => {
                        let Some(q) = pt.filter(|_| ty == "line") else { return Err(err("Invalid input")) };
                        let (kx, ky) = if p == "STARTPOINT" { ("x1", "y1") } else { ("x2", "y2") };
                        set(&mut o, kx, q.x);
                        set(&mut o, ky, q.y);
                    }
                    "CENTER" => {
                        let Some(q) = pt.filter(|_| js::finite(o.get("cx")).is_some()) else { return Err(err("Invalid input")) };
                        set(&mut o, "cx", q.x);
                        set(&mut o, "cy", q.y);
                    }
                    "RADIUS" => {
                        if !(n > 0.0) || js::finite(o.get("r")).is_none() {
                            return Err(err("Invalid input"));
                        }
                        set(&mut o, "r", n);
                    }
                    "DIAMETER" => {
                        if !(n > 0.0) || ty != "circle" {
                            return Err(err("Invalid input"));
                        }
                        set(&mut o, "r", n / 2.0);
                    }
                    "STARTANGLE" | "ENDANGLE" => {
                        let k = match (ty.as_str(), p) {
                            ("arc", "STARTANGLE") => "a0",
                            ("arc", _) => "a1",
                            ("ellipse", "STARTANGLE") => "t0",
                            ("ellipse", _) => "t1",
                            _ => return Err(err("Invalid input")),
                        };
                        set(&mut o, k, n);
                    }
                    "TEXTSTRING" => {
                        if ty != "text" {
                            return Err(err("Invalid input"));
                        }
                        o.insert("s".into(), Value::String(dxflist::js_str(v).replace("\\P", "\n")));
                    }
                    "INSERTIONPOINT" | "COORDINATES" => {
                        if p == "COORDINATES" && ty == "poly" {
                            let Pv::Arr(items, _) = v else { return Err(err("Invalid input")) };
                            let a: Vec<f64> = items.iter().map(num_in).collect();
                            if a.len() < 4 || !a.len().is_multiple_of(2) || !a.iter().all(|x| x.is_finite()) {
                                return Err(err("Invalid input"));
                            }
                            let old_pts = o.get("pts").and_then(Value::as_array).cloned().unwrap_or_default();
                            let pts: Vec<Value> = a
                                .chunks(2)
                                .enumerate()
                                .map(|(k, c)| {
                                    let bulge = old_pts.get(k).and_then(|q| q.get("b")).and_then(Value::as_f64).filter(|b| *b != 0.0 && !b.is_nan()).unwrap_or(0.0);
                                    json!({ "x": cc_cad::entity::num(c.first().copied().unwrap_or(0.0)), "y": cc_cad::entity::num(c.get(1).copied().unwrap_or(0.0)), "b": cc_cad::entity::num(bulge) })
                                })
                                .collect();
                            o.insert("pts".into(), Value::Array(pts));
                        } else {
                            let Some(q) = pt.filter(|_| js::finite(o.get("x")).is_some()) else { return Err(err("Invalid input")) };
                            set(&mut o, "x", q.x);
                            set(&mut o, "y", q.y);
                        }
                    }
                    "HEIGHT" => {
                        if !(n > 0.0) || ty != "text" {
                            return Err(err("Invalid input"));
                        }
                        set(&mut o, "h", n);
                    }
                    "ROTATION" => {
                        if !n.is_finite() || !(o.contains_key("rot") || ty == "text" || ty == "insert") {
                            return Err(err("Invalid input"));
                        }
                        set(&mut o, "rot", n);
                    }
                    "XSCALEFACTOR" | "YSCALEFACTOR" => {
                        if ty != "insert" || n == 0.0 || n.is_nan() {
                            return Err(err("Invalid input"));
                        }
                        set(&mut o, if p == "XSCALEFACTOR" { "sx" } else { "sy" }, n);
                    }
                    "NAME" => {
                        let s = dxflist::js_str(v);
                        if ty != "insert" || !self.sh.doc.borrow().blocks.contains_key(s.as_str()) {
                            return Err(err("Invalid input"));
                        }
                        o.insert("block".into(), Value::String(s));
                    }
                    "CLOSED" => {
                        if ty != "poly" {
                            return Err(err("Invalid input"));
                        }
                        o.insert("closed".into(), Value::Bool(bool_in(v)));
                    }
                    "CONSTANTWIDTH" => {
                        if ty != "poly" {
                            return Err(err("Invalid input"));
                        }
                        if n > 0.0 {
                            set(&mut o, "w", n);
                        } else {
                            o.remove("w");
                        }
                    }
                    "TEXTOVERRIDE" => {
                        if ty != "dim" {
                            return Err(err("Invalid input"));
                        }
                        o.insert("text".into(), Value::String(dxflist::js_str(v)));
                        o.remove("block");
                    }
                    _ => return Err(err(&format!("Unknown property: {p}"))),
                }
                let ne = Entity::from_value(&Value::Object(o));
                self.replace(&f, ne, "VLA")?;
                return Ok(Pv::T);
            }
            "Block" if p == "NAME" => {
                let name = find_block(&self.sh.doc.borrow(), &key);
                let bl = name.as_ref().and_then(|n| self.sh.doc.borrow().blocks.get(n).cloned());
                let (Some(name), Some(bl)) = (name, bl) else { return Err(err("Invalid input")) };
                let to = dxflist::js_str(v);
                if to.is_empty() || find_block(&self.sh.doc.borrow(), &to).is_some() {
                    return Err(err("Duplicate record name"));
                }
                let r = self.sh.doc.borrow_mut().change("RENAME", |d| {
                    let mut nb = bl.clone();
                    nb.name = to.clone();
                    d.define_block(&nb)?;
                    d.remove_block(&name)?;
                    let mut spaces = vec![MODEL];
                    spaces.extend(d.layouts.iter().map(|l| l.space));
                    for s in spaces {
                        let list: Vec<Entity> =
                            d.space_entities(s).filter(|e| cc_cad::doc::block_ref(e) == Some(name.as_str())).cloned().collect();
                        for e in list {
                            let mut o = e.to_obj();
                            o.insert("block".into(), Value::String(to.clone()));
                            if let Some(id) = e.id {
                                d.replace_in(id, Entity::from_value(&Value::Object(o)), s)?;
                            }
                        }
                    }
                    let others: Vec<Block> = d
                        .blocks
                        .values()
                        .filter(|b| b.entities.iter().any(|e| cc_cad::doc::block_ref(e) == Some(name.as_str())))
                        .cloned()
                        .collect();
                    for ob in others {
                        let mut nb = ob.clone();
                        nb.entities = ob
                            .entities
                            .iter()
                            .map(|e| {
                                if cc_cad::doc::block_ref(e) == Some(name.as_str()) {
                                    let mut o = e.to_obj();
                                    o.insert("block".into(), Value::String(to.clone()));
                                    Entity::from_value(&Value::Object(o))
                                } else {
                                    e.clone()
                                }
                            })
                            .collect();
                        d.define_block(&nb)?;
                    }
                    Ok(())
                });
                self.sh.flush_notes();
                r.map_err(|e| HostErr::Msg(e.to_string()))?;
                return Ok(Pv::T);
            }
            _ => {}
        }
        Err(err(&format!("Property is read only: {p}")))
    }

    // ---- call ----

    fn call(&self, hh: &VlaH, m: &str, a: &[Pv]) -> HR<Pv> {
        let key = hh.key.as_ref().map(VKey::text).unwrap_or_default();
        if matches!(m, "UPDATE" | "REGEN" | "STARTUNDOMARK" | "ENDUNDOMARK" | "HIGHLIGHT" | "EVALUATE") {
            if m == "REGEN" {
                self.sh.host.borrow_mut().preview(Vec::new());
            }
            return Ok(Pv::Nil);
        }
        if hh.kind == "App" && (m == "ZOOMEXTENTS" || m == "ZOOMALL") {
            self.sh.host.borrow_mut().zoom_extents();
            return Ok(Pv::Nil);
        }
        let arg = |k: usize| a.get(k).cloned().unwrap_or(Pv::Nil);
        if m == "ITEM" {
            if hh.kind == "Block" {
                let name = find_block(&self.sh.doc.borrow(), &key).unwrap_or_default();
                let items = self.block_items(&name);
                let n = num_in(&arg(0));
                let hit = items.and_then(|v| {
                    (n >= 0.0 && n.fract() == 0.0)
                        .then(|| {
                            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                            let k = n as usize;
                            v.get(k).cloned()
                        })
                        .flatten()
                });
                return hit.ok_or_else(notfound);
            }
            return match self.item_of(&hh.kind, &arg(0)) {
                None => Err(err("Not applicable")),
                Some(Err(e)) => Err(e),
                Some(Ok(x)) => Ok(Pv::Vla(x)),
            };
        }
        if hh.kind == "Layers" && m == "ADD" {
            let n = s_of(a.first());
            if n.is_empty() {
                return Err(err("Invalid input"));
            }
            if !self.sh.doc.borrow().layers.contains_key(n.as_str()) {
                let r = self.sh.doc.borrow_mut().set_layer(&n, &Obj::new(), false);
                self.sh.flush_notes();
                r.map_err(|e| HostErr::Msg(e.to_string()))?;
            }
            return Ok(hk("Layer", &n));
        }
        if hh.kind == "Blocks" && m == "ADD" {
            let p = pt_in(&arg(0));
            let n = s_of(a.get(1));
            let Some(p) = p.filter(|_| !n.is_empty()) else { return Err(err("Invalid input")) };
            if let Some(f) = find_block(&self.sh.doc.borrow(), &n) {
                return Ok(hk("Block", &f));
            }
            let nb = Block::new(&n, p, Vec::new());
            let r = self.sh.doc.borrow_mut().change("BLOCK", |d| d.define_block(&nb).map(|_| ()));
            self.sh.flush_notes();
            r.map_err(|e| HostErr::Msg(e.to_string()))?;
            return Ok(hk("Block", &n));
        }
        if hh.kind == "Doc" && m == "HANDLETOOBJECT" {
            let s = dxflist::js_str(&arg(0));
            let id = i64::from_str_radix(&s.chars().take_while(char::is_ascii_hexdigit).collect::<String>(), 16).ok();
            if let Some(id) = id.filter(|x| *x >= 0)
                && CadHost::find(&self.sh.doc.borrow(), &EId::N(id.unsigned_abs())).is_some()
            {
                return Ok(ent_h(id.unsigned_abs()));
            }
            return Err(err("Invalid handle"));
        }
        if hh.kind == "Doc" && m == "PURGEALL" {
            let r = self.sh.doc.borrow_mut().change("PURGE", |d| {
                let names: Vec<String> =
                    d.blocks.values().filter(|b| d.block_uses(&b.name) == 0 && !b.name.starts_with('*')).map(|b| b.name.clone()).collect();
                for n in names {
                    d.remove_block(&n)?;
                }
                Ok(())
            });
            self.sh.flush_notes();
            r.map_err(|e| HostErr::Msg(e.to_string()))?;
            return Ok(Pv::Nil);
        }
        if hh.kind == "Block" {
            let Some(name) = find_block(&self.sh.doc.borrow(), &key) else { return Err(err("Object was erased")) };
            let sp = space_of(&self.sh.doc.borrow(), &name);
            let bl = if sp.is_some() { None } else { self.sh.doc.borrow().blocks.get(&name).cloned() };
            let target = match (sp, &bl) {
                (Some(s), _) => Target::Space(s),
                (None, Some(b)) => Target::Block(b.clone()),
                (None, None) => Target::Space(self.sh.doc.borrow().space),
            };
            let p = |k: usize| pt_in(&arg(k));
            let n = |k: usize| num_in(&arg(k));
            let add = |o: Value| -> HR<Pv> { self.add_to(&target, o.as_object().cloned().unwrap_or_default(), "VLA") };
            match m {
                "DELETE" => {
                    if sp.is_some() {
                        return Err(err("Invalid input"));
                    }
                    if self.sh.doc.borrow().block_uses(&name) > 0 {
                        return Err(err("Object is referenced"));
                    }
                    let r = self.sh.doc.borrow_mut().change("DELETE", |d| d.remove_block(&name));
                    self.sh.flush_notes();
                    r.map_err(|e| HostErr::Msg(e.to_string()))?;
                    return Ok(Pv::Nil);
                }
                "ADDLINE" => {
                    let (Some(a0), Some(a1)) = (p(0), p(1)) else { return Err(err("Invalid input")) };
                    return add(json!({ "type": "line", "x1": a0.x, "y1": a0.y, "x2": a1.x, "y2": a1.y }));
                }
                "ADDCIRCLE" => {
                    let Some(c) = p(0).filter(|_| n(1) > 0.0) else { return Err(err("Invalid input")) };
                    return add(json!({ "type": "circle", "cx": c.x, "cy": c.y, "r": n(1) }));
                }
                "ADDARC" => {
                    let Some(c) = p(0).filter(|_| n(1) > 0.0) else { return Err(err("Invalid input")) };
                    return add(
                        json!({ "type": "arc", "cx": c.x, "cy": c.y, "r": n(1), "a0": cc_cad::entity::num(n(2)), "a1": cc_cad::entity::num(n(3)) }),
                    );
                }
                "ADDPOINT" => {
                    let Some(c) = p(0) else { return Err(err("Invalid input")) };
                    return add(json!({ "type": "point", "x": c.x, "y": c.y }));
                }
                "ADDTEXT" => {
                    let Some(c) = p(1).filter(|_| n(2) > 0.0) else { return Err(err("Invalid input")) };
                    return add(
                        json!({ "type": "text", "s": dxflist::js_str(&arg(0)), "x": c.x, "y": c.y, "h": n(2), "rot": 0, "ha": "left", "va": "baseline" }),
                    );
                }
                "ADDMTEXT" => {
                    let Some(c) = p(0) else { return Err(err("Invalid input")) };
                    let th = self.sh.doc.borrow().current.text_height;
                    let mut o = json!({ "type": "text", "s": s_of(a.get(2)).replace("\\P", "\n"), "x": c.x, "y": c.y, "h": js::or(Some(th), 2.5), "rot": 0, "ha": "left", "va": "top" });
                    if n(1) > 0.0
                        && let Some(m) = o.as_object_mut()
                    {
                        m.insert("ww".into(), cc_cad::entity::num(n(1)));
                    }
                    return add(o);
                }
                "ADDLIGHTWEIGHTPOLYLINE" => {
                    let cs: Vec<f64> = match a.first() {
                        Some(Pv::Arr(items, _)) => items.iter().map(num_in).collect(),
                        _ => Vec::new(),
                    };
                    if cs.len() < 4 || !cs.len().is_multiple_of(2) || !cs.iter().all(|x| x.is_finite()) {
                        return Err(err("Invalid input"));
                    }
                    let pts: Vec<Value> = cs
                        .chunks(2)
                        .map(|c| json!({ "x": c.first().copied().unwrap_or(0.0), "y": c.get(1).copied().unwrap_or(0.0), "b": 0 }))
                        .collect();
                    return add(json!({ "type": "poly", "pts": pts, "closed": false }));
                }
                "INSERTBLOCK" => {
                    let bn = s_of(a.get(1));
                    let Some(c) = p(0).filter(|_| self.sh.doc.borrow().blocks.contains_key(bn.as_str())) else {
                        return Err(err("Invalid input"));
                    };
                    let f = |x: f64, d: f64| if x.is_finite() { x } else { d };
                    return add(
                        json!({ "type": "insert", "block": bn, "x": c.x, "y": c.y, "sx": f(n(2), 1.0), "sy": f(n(3), 1.0), "rot": f(n(5), 0.0) }),
                    );
                }
                _ => {}
            }
        }
        if hh.kind == "Layer" && m == "DELETE" {
            let l = self.sh.doc.borrow().layers.get(&key).cloned();
            let Some(l) = l else { return Err(err("Object was erased")) };
            let used = {
                let d = self.sh.doc.borrow();
                l.name == "0"
                    || l.name == d.current.layer
                    || d.every_entity().any(|e| e.layer_str() == l.name)
                    || d.blocks.values().any(|b| b.entities.iter().any(|e| e.layer_str() == l.name))
            };
            if used {
                return Err(err("Object is referenced"));
            }
            let mut state = self.sh.doc.borrow().layer_state();
            state.list.retain(|x| x.name != l.name);
            let r = self.sh.doc.borrow_mut().change("DELETE", |d| d.set_layer_state(&state));
            self.sh.flush_notes();
            r.map_err(|e| HostErr::Msg(e.to_string()))?;
            return Ok(Pv::Nil);
        }
        if hh.kind == "Layout" && m == "DELETE" {
            if key == "Model" {
                return Err(err("Invalid input"));
            }
            let list = self.sh.doc.borrow().layouts.clone();
            if !list.iter().any(|x| x.name == key) {
                return Err(err("Object was erased"));
            }
            let k = list.iter().position(|x| x.name == key);
            let mut nl = list.clone();
            if let Some(k) = k {
                nl.remove(k);
            }
            let r = self.sh.doc.borrow_mut().change("DELETE", |d| d.set_layouts(nl));
            self.sh.flush_notes();
            r.map_err(|e| HostErr::Msg(e.to_string()))?;
            return Ok(Pv::Nil);
        }
        if hh.kind == "Ent" {
            let Some(f) = self.ent(hh) else { return Err(err("Object was erased")) };
            let e = f.e().clone();
            let tx = |mx: &[f64; 6], keep: bool, label: &str| -> HR<Pv> {
                let moved = cc_cad_geom::tx::tx_entity(&e, mx);
                if keep {
                    let mut o = moved.to_obj();
                    o.remove("id");
                    return match &f {
                        EntRef::Member { block, .. } => self.add_to(&Target::Block(block.clone()), o, label),
                        EntRef::Space { space, .. } => self.add_to(&Target::Space(*space), o, label),
                    };
                }
                if self.layer_locked(&e) {
                    return Err(err("On locked layer"));
                }
                let mut next = moved;
                next.id = e.id;
                self.replace(&f, next, label)?;
                Ok(Pv::Nil)
            };
            let arg = |k: usize| a.get(k).cloned().unwrap_or(Pv::Nil);
            match m {
                "DELETE" => {
                    if self.layer_locked(&e) {
                        return Err(err("On locked layer"));
                    }
                    match &f {
                        EntRef::Member { block, idx, .. } => {
                            let mut ents = block.entities.to_vec();
                            let mut serials = self.host.vla.borrow().serials(&block.name, &block.entities);
                            if *idx < ents.len() {
                                ents.remove(*idx);
                            }
                            if *idx < serials.len() {
                                serials.remove(*idx);
                            }
                            self.redefine(block, ents, serials, "DELETE")?;
                        }
                        EntRef::Space { space, .. } => {
                            let id = e.id.unwrap_or(0);
                            let r = self.sh.doc.borrow_mut().change("DELETE", |d| d.remove_in(id, *space));
                            self.sh.flush_notes();
                            r.map_err(|e| HostErr::Msg(e.to_string()))?;
                        }
                    }
                    return Ok(Pv::Nil);
                }
                "COPY" => return tx(&[1.0, 0.0, 0.0, 1.0, 0.0, 0.0], true, "COPY"),
                "MOVE" => {
                    let (Some(p0), Some(p1)) = (pt_in(&arg(0)), pt_in(&arg(1))) else { return Err(err("Invalid input")) };
                    return tx(&m_translate(p1.x - p0.x, p1.y - p0.y), false, "MOVE");
                }
                "ROTATE" => {
                    let Some(p0) = pt_in(&arg(0)) else { return Err(err("Invalid input")) };
                    return tx(&m_about(p0, &m_rotate(num_in(&arg(1)))), false, "ROTATE");
                }
                "SCALEENTITY" => {
                    let s = num_in(&arg(1));
                    let Some(p0) = pt_in(&arg(0)).filter(|_| s > 0.0) else { return Err(err("Invalid input")) };
                    return tx(&m_about(p0, &m_scale(s, s)), false, "SCALE");
                }
                "MIRROR" => {
                    let (Some(p0), Some(p1)) = (pt_in(&arg(0)), pt_in(&arg(1))) else { return Err(err("Invalid input")) };
                    return tx(&m_mirror(p0, p1), true, "MIRROR");
                }
                "GETBOUNDINGBOX" => {
                    let bb = entity_bounds(&e, Some(&self.sh.doc.borrow().blocks));
                    if !valid_bounds(&bb) {
                        return Err(err("Invalid extents"));
                    }
                    return Ok(Pv::Bbox([bb.min_x, bb.min_y, 0.0], [bb.max_x, bb.max_y, 0.0]));
                }
                "GETATTRIBUTES" | "GETCONSTANTATTRIBUTES" => {
                    let Geo::Insert(ins) = &e.geo else { return Ok(Pv::Arr(Vec::new(), Some(9))) };
                    if matches!(f, EntRef::Member { .. }) {
                        return Ok(Pv::Arr(Vec::new(), Some(9)));
                    }
                    let defs: Vec<Value> = self.sh.doc.borrow().blocks.get(&ins.block).and_then(|b| b.attdefs.clone()).unwrap_or_default();
                    let tags: Vec<String> = if m == "GETCONSTANTATTRIBUTES" {
                        defs.iter().filter(|x| js::truthy(x.get("constant"))).filter_map(|x| x.get("tag").map(js::to_js_string)).collect()
                    } else {
                        let mut t: Vec<String> = defs
                            .iter()
                            .filter(|x| !js::truthy(x.get("constant")))
                            .filter_map(|x| x.get("tag").map(js::to_js_string))
                            .collect();
                        if let Some(vals) = &ins.attrs {
                            for k in cc_cad_geom::geom::js_keys(vals) {
                                t.push(k.clone());
                            }
                        }
                        let mut seen: Vec<String> = Vec::new();
                        for x in t {
                            if !seen.contains(&x) {
                                seen.push(x);
                            }
                        }
                        seen
                    };
                    let id = e.id.unwrap_or(0);
                    #[allow(clippy::cast_precision_loss)]
                    let kf = id as f64;
                    return Ok(Pv::Arr(
                        tags.into_iter()
                            .map(|tag| Pv::Vla(VlaH { kind: "Att".into(), key: Some(VKey::Num(kf)), tag: Some(tag) }))
                            .collect(),
                        Some(9),
                    ));
                }
                "EXPLODE" => {
                    let EntRef::Space { space, .. } = &f else { return Err(err("Invalid input")) };
                    if !matches!(e.geo, Geo::Insert(_)) {
                        return Err(err("Invalid input"));
                    }
                    let parts = cc_cad_geom::corners::explode_entity(&e, Some(&self.sh.doc.borrow().blocks)).unwrap_or_default();
                    let mut made = Vec::new();
                    for x in parts {
                        let mut o = x.to_obj();
                        o.remove("id");
                        made.push(self.add_to(&Target::Space(*space), o, "EXPLODE")?);
                    }
                    return Ok(Pv::Arr(made, Some(9)));
                }
                _ => {}
            }
        }
        if hh.kind == "Att" && m == "DELETE" {
            return Err(err("Invalid input"));
        }
        Err(err(&format!("Unknown method: {m}")))
    }
}

/// Where add_to adds (short-lived: the size is fine).
#[allow(clippy::large_enum_variant)]
enum Target {
    Space(SpaceId),
    Block(Block),
}

fn methods(kind: &str) -> &'static [&'static str] {
    match kind {
        "App" => &["ZOOMEXTENTS", "ZOOMALL", "UPDATE"],
        "Doc" => &["REGEN", "STARTUNDOMARK", "ENDUNDOMARK", "HANDLETOOBJECT", "PURGEALL"],
        "Blocks" | "Layers" => &["ITEM", "ADD"],
        "Layouts" | "Linetypes" | "TextStyles" | "DimStyles" | "Views" | "Documents" => &["ITEM"],
        "Block" => &[
            "ITEM",
            "DELETE",
            "ADDLINE",
            "ADDCIRCLE",
            "ADDARC",
            "ADDPOINT",
            "ADDTEXT",
            "ADDMTEXT",
            "ADDLIGHTWEIGHTPOLYLINE",
            "INSERTBLOCK",
        ],
        "Layer" | "Layout" => &["DELETE"],
        "Ent" => &["DELETE", "COPY", "MOVE", "ROTATE", "SCALEENTITY", "MIRROR", "GETBOUNDINGBOX", "HIGHLIGHT", "UPDATE"],
        "Att" => &["UPDATE"],
        _ => &[],
    }
}

/// vla.js → vla-host.js `vlaCall(op, h, args)`.
pub fn vla_call(host: &CadHost, sh: &Shared, op: VlaOp) -> HR<Pv> {
    let c = Ctx { host, sh };
    match op {
        VlaOp::Get(hh, p) => c.get(&hh, &p.to_uppercase()),
        VlaOp::Put(hh, p, v) => c.put(&hh, &p.to_uppercase(), &v),
        VlaOp::Call(hh, m, args) => c.call(&hh, &m.to_uppercase(), &args),
        VlaOp::Has(hh, p, put) => {
            let up = p.to_uppercase();
            if c.get(&hh, &up).is_err() {
                return Ok(Pv::Bool(false));
            }
            if !put {
                return Ok(Pv::Bool(true));
            }
            Ok(Pv::Bool(
                !["OBJECTNAME", "HANDLE", "COUNT", "EFFECTIVENAME", "AREA", "LENGTH", "ISLAYOUT", "ISXREF", "HASATTRIBUTES"]
                    .contains(&up.as_str()),
            ))
        }
        VlaOp::Can(hh, m) => {
            let up = m.to_uppercase();
            let ok = methods(&hh.kind).contains(&up.as_str())
                || (hh.kind == "Ent"
                    && ["GETATTRIBUTES", "EXPLODE"].contains(&up.as_str())
                    && c.ent(&hh).is_some_and(|f| matches!(f.e().geo, Geo::Insert(_))));
            Ok(Pv::Bool(ok))
        }
        VlaOp::Items(hh) => {
            if hh.kind == "Block" {
                let key = hh.key.as_ref().map(VKey::text).unwrap_or_default();
                let name = find_block(&sh.doc.borrow(), &key).unwrap_or(key);
                return c.block_items(&name).map(Pv::List).ok_or_else(|| err("Object was erased"));
            }
            c.coll_items(&hh.kind).map(|v| Pv::List(v.into_iter().map(Pv::Vla).collect())).ok_or_else(|| err("Not a collection"))
        }
        VlaOp::FromEname(id) => match &id {
            EId::S(s) if s.starts_with("b:") => {
                let f = CadHost::find(&sh.doc.borrow(), &id);
                match f.and_then(|f| f.block.map(|b| (b, f.e))) {
                    Some(((name, idx), _)) => {
                        let ents = sh.doc.borrow().blocks.get(&name).map(|b| b.entities.clone());
                        ents.map(|e| c.member_h(&name, &e, idx)).ok_or_else(|| err("Invalid input"))
                    }
                    None => Err(err("Invalid input")),
                }
            }
            EId::S(s) if s.starts_with("a:") => {
                let rest = s.get(2..).unwrap_or("");
                let k = rest.find(':');
                let (num, tag) = match k {
                    Some(k) => (rest.get(..k).unwrap_or(""), rest.get(k.saturating_add(1)..).unwrap_or("")),
                    None => (rest, ""),
                };
                let ok = k.is_some() && !num.is_empty() && num.chars().all(|ch| ch.is_ascii_digit());
                let n: f64 = num.parse().unwrap_or(f64::NAN);
                if ok && super::host::CadHost::find(&sh.doc.borrow(), &EId::N(num.parse().unwrap_or(0))).is_some() {
                    return Ok(Pv::Vla(VlaH { kind: "Att".into(), key: Some(VKey::Num(n)), tag: Some(tag.to_string()) }));
                }
                Err(err("Invalid input"))
            }
            EId::S(s) if s.starts_with("T:") => {
                let rest = s.get(2..).unwrap_or("");
                let Some(k) = rest.find(':') else { return Err(err("Invalid input")) };
                let (table, name) = (rest.get(..k).unwrap_or(""), rest.get(k.saturating_add(1)..).unwrap_or(""));
                match table {
                    "LAYER" => Ok(hk("Layer", name)),
                    "BLOCK" => Ok(hk("Block", name)),
                    _ => Err(err("Invalid input")),
                }
            }
            _ => {
                if CadHost::find(&sh.doc.borrow(), &id).is_some()
                    && let Some(n) = id.num()
                {
                    return Ok(ent_h(n));
                }
                Err(err("Invalid input"))
            }
        },
        VlaOp::ToEname(hh) => {
            if hh.kind != "Ent" {
                return Ok(Pv::Nil);
            }
            match &hh.key {
                Some(VKey::Num(_)) => {
                    let id = key_eid(&hh.key);
                    Ok(id.filter(|x| CadHost::find(&sh.doc.borrow(), x).is_some()).map_or(Pv::Nil, Pv::E))
                }
                Some(VKey::Str(s)) => match c.member(s) {
                    Some(EntRef::Member { block, idx, .. }) => Ok(Pv::E(EId::S(format!("b:{}:{idx}", block.name).into()))),
                    _ => Ok(Pv::Nil),
                },
                None => Ok(Pv::Nil),
            }
        }
        VlaOp::Erased(hh) => Ok(Pv::Bool(hh.kind == "Ent" && c.ent(&hh).is_none())),
    }
}
