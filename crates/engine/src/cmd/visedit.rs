//! Visibility states in the block editor (visedit.js, 0.5 DB.9) and the commands BVSTATE / BVHIDE / BVSHOW.
//!
//! Only a block that already has a visibility parameter (`block.dyn.vis`, from a DWG): its states can be added,
//! renamed, deleted and made current, and objects hidden or shown per state. In the editor every file object of the
//! block is known by its index `fi` (`dyn.all[fi]` = its parts); the editor's space holds the current state's objects,
//! the others wait in `store`. What is in the space is the truth for the current state, so UNDO inside the editor stays
//! right (state names and the list of states are not undone; Discard is).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use cc_cad::entity::{Color, Entity, Geo};
use cc_cad::{Block, Doc, DocError, MODEL, SpaceId, js};
use cc_cad_geom::geom::{dist_to_prim, prims};
use serde_json::{Value, json};

use crate::engine::{Ctx, Opts, R, Reply};
use crate::input::Def;

/// One visibility state: its name, the file objects it shows, the name it has in the file when renamed or new.
#[derive(Clone, Debug, PartialEq)]
pub struct VisState {
    pub name: String,
    pub show: Vec<usize>,
    pub src: Option<String>,
}

/// `be.vis`: the block editor's view of the visibility states.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VisEdit {
    pub def: String,
    pub states: Vec<VisState>,
    pub cur: String,
    /// editor object id → file object index
    pub fi_of: HashMap<u64, usize>,
    /// file objects out of the space (Map order: first stored first)
    pub store: Vec<(usize, Vec<Entity>)>,
    pub ad: HashSet<usize>,
    pub ghost: HashSet<usize>,
    pub n: usize,
    /// file name → name now (Map order)
    pub renamed: Vec<(String, String)>,
    /// deleted states (Set order)
    pub deleted: Vec<String>,
}

fn clone_ent(e: &Entity) -> Entity {
    let mut c = e.clone();
    c.id = None;
    c.extra.remove("id");
    c
}

fn arr_usize(v: Option<&Value>) -> Vec<usize> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.as_u64().and_then(|u| usize::try_from(u).ok())).collect())
        .unwrap_or_default()
}

fn parts_of(v: &Value) -> Vec<Entity> {
    v.as_array().map(|a| a.iter().map(Entity::from_value).collect()).unwrap_or_default()
}

/// `currentAll(b)`: dyn.all as it is now — the shown parts from the block's objects, the hidden ones from dyn.all.
fn current_all(b: &Block, d: &Value) -> Option<Vec<Vec<Entity>>> {
    let all = d.get("all")?.as_array()?;
    let shown = d.get("shown").and_then(Value::as_array).cloned().unwrap_or_default();
    if all.len() != shown.len() {
        return None;
    }
    let have = &b.entities;
    let mut k = 0usize;
    let mut out = Vec::with_capacity(all.len());
    for (i, parts) in all.iter().enumerate() {
        let n = parts.as_array().map_or(0, Vec::len);
        if !js::truthy(shown.get(i)) {
            out.push(parts_of(parts));
            continue;
        }
        let end = k.saturating_add(n).min(have.len());
        let now: Vec<Entity> = have.get(k..end).map(<[Entity]>::to_vec).unwrap_or_default();
        k = k.saturating_add(n);
        if now.len() != n {
            return None;
        }
        out.push(now);
    }
    (k == have.len()).then_some(out)
}

/// `openVis(doc, blk)` → the editor's view and the entities the editor space starts with (with ids).
pub fn open_vis(doc: &mut Doc, blk: &Block) -> Option<(VisEdit, Vec<Entity>)> {
    let d = blk.dyn_.as_ref()?;
    let vis = d.get("vis")?;
    let vstates = vis.get("states").and_then(Value::as_array).filter(|a| !a.is_empty())?;
    if !js::truthy(d.get("def")) || !d.get("all").is_some_and(Value::is_array) {
        return None;
    }
    let now = current_all(blk, d)?;
    let ad: HashSet<usize> = arr_usize(d.get("ad")).into_iter().collect();
    let shown = d.get("shown").and_then(Value::as_array).cloned().unwrap_or_default();
    let all = d.get("all").and_then(Value::as_array).cloned().unwrap_or_default();
    let states: Vec<VisState> = vstates
        .iter()
        .map(|s| {
            let name = s.get("name").map(js::to_js_string).unwrap_or_default();
            let src = s.get("src").map(js::to_js_string).filter(|x| *x != name);
            VisState { name, show: arr_usize(s.get("show")), src }
        })
        .collect();
    let same = |s: &VisState| (0..all.len()).all(|i| ad.contains(&i) || s.show.contains(&i) == js::truthy(shown.get(i)));
    let cur = states.iter().find(|s| same(s)).or(states.first())?.name.clone();
    let ghost: HashSet<usize> =
        all.iter().enumerate().filter(|(i, p)| !ad.contains(i) && p.as_array().is_none_or(Vec::is_empty)).map(|(i, _)| i).collect();
    let show: HashSet<usize> = states.iter().find(|s| s.name == cur).map(|s| s.show.iter().copied().collect()).unwrap_or_default();
    let mut v = VisEdit { def: blk.name.clone(), states, cur, ad, ghost, n: all.len(), ..VisEdit::default() };
    let mut ents = Vec::new();
    for (fi, parts) in now.iter().enumerate() {
        if v.ad.contains(&fi) || v.ghost.contains(&fi) {
            continue;
        }
        if show.contains(&fi) {
            for p in parts {
                let mut x = clone_ent(p);
                if x.color.is_none() {
                    x.color = Some(Color::Null);
                }
                let id = doc.next_id;
                doc.next_id = doc.next_id.saturating_add(1);
                x.id = Some(id);
                v.fi_of.insert(id, fi);
                ents.push(x);
            }
        } else {
            store_set(&mut v.store, fi, parts.iter().map(clone_ent).collect());
        }
    }
    Some((v, ents))
}

/// `Map.set` on the store (an existing key keeps its place).
fn store_set(store: &mut Vec<(usize, Vec<Entity>)>, fi: usize, parts: Vec<Entity>) {
    match store.iter_mut().find(|(k, _)| *k == fi) {
        Some(slot) => slot.1 = parts,
        None => store.push((fi, parts)),
    }
}

/// `inSpace(v, map)`: the space's objects by file index, in the space's order.
fn in_space(v: &VisEdit, doc: &Doc, space: SpaceId) -> Vec<(usize, Vec<Entity>)> {
    let mut by: Vec<(usize, Vec<Entity>)> = Vec::new();
    for e in doc.space_entities(space) {
        let Some(fi) = e.id.and_then(|id| v.fi_of.get(&id)).copied() else { continue };
        match by.iter_mut().find(|(k, _)| *k == fi) {
            Some(slot) => slot.1.push(e.clone()),
            None => by.push((fi, vec![e.clone()])),
        }
    }
    by
}

/// `syncCurrent(v, map)`: the current state's show = the file objects in the space now.
pub fn sync_current(v: &mut VisEdit, doc: &Doc, space: SpaceId) {
    let keys: Vec<usize> = in_space(v, doc, space).into_iter().map(|(k, _)| k).collect();
    let (ghost, ad) = (v.ghost.clone(), v.ad.clone());
    let cur = v.cur.clone();
    let Some(st) = v.states.iter_mut().find(|s| s.name == cur) else { return };
    let mut show: Vec<usize> = st.show.iter().copied().filter(|fi| ghost.contains(fi) || ad.contains(fi)).collect();
    for k in keys {
        if !show.contains(&k) {
            show.push(k);
        }
    }
    show.sort_unstable();
    st.show = show;
}

fn move_out(doc: &mut Doc, v: &mut VisEdit, space: SpaceId, fis: &[usize]) -> Result<usize, DocError> {
    let by = in_space(v, doc, space);
    let mut n = 0usize;
    for fi in fis {
        let Some((_, parts)) = by.iter().find(|(k, _)| k == fi) else { continue };
        store_set(&mut v.store, *fi, parts.iter().map(clone_ent).collect());
        for e in parts {
            if let Some(id) = e.id {
                doc.remove_in(id, space)?;
            }
        }
        n = n.saturating_add(1);
    }
    Ok(n)
}

fn move_in(doc: &mut Doc, v: &mut VisEdit, space: SpaceId, fis: &[usize]) -> Result<usize, DocError> {
    let by = in_space(v, doc, space);
    let mut n = 0usize;
    for fi in fis {
        if by.iter().any(|(k, _)| k == fi) {
            continue;
        }
        let Some((_, parts)) = v.store.iter().find(|(k, _)| k == fi).cloned() else { continue };
        for p in parts {
            let x = doc.add_in(p, space)?;
            if let Some(id) = x.id {
                v.fi_of.insert(id, *fi);
            }
        }
        n = n.saturating_add(1);
    }
    Ok(n)
}

/// `switchState(doc, be, name)` → an error key, or None.
pub fn switch_state(doc: &mut Doc, v: &mut VisEdit, space: SpaceId, name: &str) -> Result<Option<&'static str>, DocError> {
    let low = name.to_lowercase();
    let Some(to) = v.states.iter().find(|s| s.name.to_lowercase() == low).cloned() else { return Ok(Some("no-state")) };
    sync_current(v, doc, space);
    let want: HashSet<usize> = to.show.iter().copied().collect();
    let have: Vec<usize> = in_space(v, doc, space).into_iter().map(|(k, _)| k).collect();
    let out: Vec<usize> = have.iter().copied().filter(|fi| !want.contains(fi)).collect();
    let inn: Vec<usize> = {
        let mut seen = HashSet::new();
        to.show.iter().copied().filter(|fi| !have.contains(fi) && seen.insert(*fi)).collect()
    };
    doc.change("BVSTATE", |d| {
        move_out(d, v, space, &out)?;
        move_in(d, v, space, &inn)?;
        Ok(())
    })?;
    v.cur = to.name;
    Ok(None)
}

/// `NAME_OK`: 1–255 characters, none of < > / \ " : ; ? * | , = `.
fn name_ok(s: &str) -> bool {
    let n = s.encode_utf16().count();
    (1..=255).contains(&n) && !s.contains(['<', '>', '/', '\\', '"', ':', ';', '?', '*', '|', ',', '=', '`'])
}

pub fn new_state(v: &mut VisEdit, doc: &Doc, space: SpaceId, name: &str, how: &str) -> Option<&'static str> {
    let nm = js::trim(name).to_string();
    if !name_ok(&nm) {
        return Some("bad-name");
    }
    if v.states.iter().any(|s| s.name.to_lowercase() == nm.to_lowercase()) {
        return Some("taken");
    }
    sync_current(v, doc, space);
    let cur = v.states.iter().find(|s| s.name == v.cur).cloned()?;
    let show: Vec<usize> = match how {
        "show" => (0..v.n).collect(),
        "hide" => Vec::new(),
        _ => cur.show.clone(),
    };
    v.states.push(VisState { name: nm, show, src: Some(cur.src.clone().unwrap_or(cur.name)) });
    None
}

pub fn rename_state(v: &mut VisEdit, from: &str, to: &str) -> Option<&'static str> {
    let low = from.to_lowercase();
    let Some(i) = v.states.iter().position(|s| s.name.to_lowercase() == low) else { return Some("no-state") };
    let nm = js::trim(to).to_string();
    if !name_ok(&nm) {
        return Some("bad-name");
    }
    if v.states.iter().enumerate().any(|(j, s)| j != i && s.name.to_lowercase() == nm.to_lowercase()) {
        return Some("taken");
    }
    let old = v.states.get(i).map(|s| s.name.clone()).unwrap_or_default();
    let orig = v.renamed.iter().find(|(_, now)| *now == old).map_or_else(|| old.clone(), |(o, _)| o.clone());
    match v.renamed.iter_mut().find(|(o, _)| *o == orig) {
        Some(slot) => slot.1.clone_from(&nm),
        None => v.renamed.push((orig, nm.clone())),
    }
    if v.cur == old {
        v.cur.clone_from(&nm);
    }
    if let Some(st) = v.states.get_mut(i) {
        if st.src.is_none() {
            st.src = Some(st.name.clone());
        }
        st.name.clone_from(&nm);
        if st.src.as_deref() == Some(nm.as_str()) {
            st.src = None;
        }
    }
    None
}

pub fn delete_state(v: &mut VisEdit, name: &str) -> Option<&'static str> {
    let low = name.to_lowercase();
    let Some(i) = v.states.iter().position(|s| s.name.to_lowercase() == low) else { return Some("no-state") };
    if v.states.len() < 2 {
        return Some("last");
    }
    let nm = v.states.get(i).map(|s| s.name.clone()).unwrap_or_default();
    if nm == v.cur {
        return Some("current");
    }
    if !v.deleted.contains(&nm) {
        v.deleted.push(nm);
    }
    v.states.remove(i);
    None
}

/// What `closeVis` made: the definition with its states, or why it goes back plain.
pub enum Closed {
    Block { block: Box<Block>, renamed: Vec<(String, String)>, deleted: Vec<String>, current: String, first: String },
    Plain,
}

fn states_json(states: &[VisState]) -> Value {
    Value::Array(
        states
            .iter()
            .map(|s| {
                let mut show = s.show.clone();
                show.sort_unstable();
                let mut o = serde_json::Map::new();
                o.insert("name".into(), Value::String(s.name.clone()));
                o.insert("show".into(), json!(show));
                if let Some(src) = s.src.as_ref().filter(|x| **x != s.name) {
                    o.insert("src".into(), Value::String(src.clone()));
                }
                Value::Object(o)
            })
            .collect(),
    )
}

/// `closeVis(doc, be, entities)`: rebuild the definition from the editor (`entities`: its objects, attribute
/// definitions split off), or Plain when objects were added, erased or exploded.
pub fn close_vis(doc: &Doc, v: &mut VisEdit, space: SpaceId, base: cc_geom::P2, entities: &[Entity]) -> Closed {
    sync_current(v, doc, space);
    let Some(blk) = doc.blocks.get(&v.def) else { return Closed::Plain };
    let Some(d) = blk.dyn_.clone() else { return Closed::Plain };
    let mut by: Vec<(usize, Vec<Entity>)> = Vec::new();
    for e in entities {
        let Some(fi) = e.id.and_then(|id| v.fi_of.get(&id)).copied() else { return Closed::Plain };
        match by.iter_mut().find(|(k, _)| *k == fi) {
            Some(slot) => slot.1.push(e.clone()),
            None => by.push((fi, vec![e.clone()])),
        }
    }
    let dall = d.get("all").and_then(Value::as_array).cloned().unwrap_or_default();
    let dshown = d.get("shown").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut all: Vec<Value> = Vec::with_capacity(v.n);
    let mut all_e: Vec<Vec<Entity>> = Vec::with_capacity(v.n);
    for fi in 0..v.n {
        let orig = dall.get(fi).cloned().unwrap_or(Value::Array(Vec::new()));
        let olen = orig.as_array().map_or(0, Vec::len);
        if v.ad.contains(&fi) || olen == 0 {
            all_e.push(parts_of(&orig));
            all.push(orig);
            continue;
        }
        let parts = by
            .iter()
            .find(|(k, _)| *k == fi)
            .map(|(_, p)| p.clone())
            .or_else(|| v.store.iter().find(|(k, _)| *k == fi).map(|(_, p)| p.clone()));
        let Some(parts) = parts else { return Closed::Plain };
        let types: Vec<String> = orig
            .as_array()
            .map(|a| a.iter().map(|p| p.get("type").map(js::to_js_string).unwrap_or_default()).collect())
            .unwrap_or_default();
        if parts.len() != olen || parts.iter().zip(types.iter()).any(|(p, t)| p.typ() != t) {
            return Closed::Plain;
        }
        let cl: Vec<Entity> = parts.iter().map(clone_ent).collect();
        all.push(Value::Array(cl.iter().map(Entity::to_value).collect()));
        all_e.push(cl);
    }
    let Some(cur) = v.states.iter().find(|s| s.name == v.cur).cloned() else { return Closed::Plain };
    let show_set: HashSet<usize> = cur.show.iter().copied().collect();
    let shown: Vec<bool> =
        (0..all.len()).map(|fi| if v.ad.contains(&fi) { js::truthy(dshown.get(fi)) } else { show_set.contains(&fi) }).collect();
    let new_states = states_json(&v.states);
    let mut vis = d.get("vis").cloned().unwrap_or(Value::Null);
    let key = |states: &Value| -> String {
        let pairs: Vec<Value> =
            states.as_array().map(|a| a.iter().map(|s| json!([s.get("name"), s.get("show")])).collect()).unwrap_or_default();
        serde_json::to_string(&pairs).unwrap_or_default()
    };
    let changed = key(&new_states) != key(vis.get("states").unwrap_or(&Value::Null));
    if let Value::Object(o) = &mut vis {
        o.insert("states".into(), new_states);
    }
    let mut dd = d.as_object().cloned().unwrap_or_default();
    dd.insert("all".into(), Value::Array(all));
    dd.insert("shown".into(), json!(shown));
    dd.insert("vis".into(), vis);
    if changed || js::truthy(d.get("visEdited")) {
        dd.insert("visEdited".into(), Value::Bool(true));
    }
    let ents: Vec<Entity> = all_e.into_iter().zip(shown.iter()).filter(|(_, s)| **s).flat_map(|(p, _)| p).collect();
    let mut block = Block::new(&blk.name, base, ents);
    block.dyn_ = Some(Value::Object(dd));
    let first = v.states.first().map(|s| s.name.clone()).unwrap_or_default();
    Closed::Block { block: Box::new(block), renamed: v.renamed.clone(), deleted: v.deleted.clone(), current: cur.name, first }
}

// ---------------------------------------------------------------------------------------------- dyn-blocks.js

/// `dynEntities(b)`: the definition's objects per file index, the shown ones from its objects; None when they no
/// longer line up (objects or attribute definitions added or removed).
fn dyn_entities(b: &Block) -> Option<Vec<Vec<Entity>>> {
    let d = b.dyn_.as_ref()?;
    let all = d.get("all")?.as_array()?;
    let shown = d.get("shown")?.as_array()?;
    if all.len() != shown.len() || b.attdefs.as_ref().map_or(0, Vec::len) != arr_usize(d.get("ad")).len() {
        return None;
    }
    let have = &b.entities;
    let mut k = 0usize;
    let mut out = Vec::with_capacity(all.len());
    for (i, parts) in all.iter().enumerate() {
        let pv = parts.as_array().cloned().unwrap_or_default();
        if !js::truthy(shown.get(i)) {
            out.push(pv.iter().map(Entity::from_value).collect());
            continue;
        }
        let end = k.saturating_add(pv.len()).min(have.len());
        let now: Vec<Entity> = have.get(k..end).map(<[Entity]>::to_vec).unwrap_or_default();
        k = k.saturating_add(pv.len());
        let types_ok = now.len() == pv.len()
            && now.iter().zip(pv.iter()).all(|(x, p)| p.get("type").map(js::to_js_string).as_deref() == Some(x.typ()));
        if !types_ok {
            return None;
        }
        out.push(now);
    }
    (k == have.len()).then_some(out)
}

/// `plain(x)`: JSON without ids.
fn plain_parts(parts: &[Entity]) -> String {
    let v: Vec<Value> = parts.iter().map(|e| clone_ent(e).to_value()).collect();
    serde_json::to_string(&v).unwrap_or_default()
}
fn plain_values(a: &[Value]) -> String {
    let v: Vec<Value> = a
        .iter()
        .map(|x| {
            let mut o = x.clone();
            if let Value::Object(m) = &mut o {
                m.shift_remove("id");
            }
            o
        })
        .collect();
    serde_json::to_string(&v).unwrap_or_default()
}

/// `showsState(d, st)`: a dynamic block's shown flags are exactly a state's objects (attribute definitions aside).
fn shows_state(d: Option<&Value>, show: &[usize]) -> bool {
    let Some(shown) = d.and_then(|d| d.get("shown")).and_then(Value::as_array) else { return true };
    let ad: HashSet<usize> = arr_usize(d.and_then(|d| d.get("ad"))).into_iter().collect();
    shown.iter().enumerate().all(|(i, v)| ad.contains(&i) || js::truthy(Some(v)) == show.contains(&i))
}

/// `setVisibility` with its refusals: Ok(Err("not-dynamic" | "no-state" | "changed")) leaves the drawing as it was.
pub fn set_visibility_checked(doc: &mut Doc, id: u64, state: &str, space: SpaceId) -> Result<Result<(), &'static str>, DocError> {
    let Some(rf) = doc.space_map(space).and_then(|m| m.get(&id)).cloned() else { return Ok(Err("not-dynamic")) };
    let Geo::Insert(ins) = &rf.geo else { return Ok(Err("not-dynamic")) };
    let Some(b) = doc.blocks.get(&ins.block).cloned() else { return Ok(Err("not-dynamic")) };
    let Some((_, states, _)) = crate::props::visibility_of(doc, &rf.to_obj()) else { return Ok(Err("not-dynamic")) };
    if !states.iter().any(|s| s == state) {
        return Ok(Err("no-state"));
    }
    if dyn_entities(&b).is_none() {
        return Ok(Err("changed"));
    }
    set_visibility(doc, id, state, space).map(Ok)
}

/// dyn-blocks.js `setVisibility(doc, id, state, map)`: the reference moves to a copy of its block showing that
/// state's objects (an existing one when one matches, else a new "*B<n>"). One undo step.
fn set_visibility(doc: &mut Doc, id: u64, state: &str, space: SpaceId) -> Result<(), DocError> {
    let Some(rf) = doc.space_map(space).and_then(|m| m.get(&id)).cloned() else { return Ok(()) };
    let Geo::Insert(ins) = &rf.geo else { return Ok(()) };
    let Some(b) = doc.blocks.get(&ins.block).cloned() else { return Ok(()) };
    let Some(bd) = b.dyn_.clone() else { return Ok(()) };
    let def_name = if js::truthy(bd.get("def")) { b.name.clone() } else { bd.get("of").map(js::to_js_string).unwrap_or_default() };
    let Some(def) = doc.blocks.get(&def_name).cloned() else { return Ok(()) };
    let Some(vis) = def.dyn_.as_ref().and_then(|d| d.get("vis")).cloned() else { return Ok(()) };
    let states = vis.get("states").and_then(Value::as_array).cloned().unwrap_or_default();
    if states.is_empty() {
        return Ok(());
    }
    let Some(st) = states.iter().find(|s| s.get("name").map(js::to_js_string).as_deref() == Some(state)) else { return Ok(()) };
    let Some(g) = dyn_entities(&b) else { return Ok(()) };
    let mut shown = vec![false; g.len()];
    for i in arr_usize(st.get("show")) {
        if let Some(s) = shown.get_mut(i) {
            *s = true;
        }
    }
    let bshown = bd.get("shown").and_then(Value::as_array).cloned().unwrap_or_default();
    for i in arr_usize(bd.get("ad")) {
        if let Some(s) = shown.get_mut(i) {
            *s = js::truthy(bshown.get(i));
        }
    }
    let ad_plain = plain_values(b.attdefs.as_deref().unwrap_or(&[]));
    let gplain: Vec<String> = g.iter().map(|p| plain_parts(p)).collect();
    let mut target: Option<String> = None;
    for x in doc.blocks.values() {
        let Some(xd) = x.dyn_.as_ref() else { continue };
        if !(x.name == def.name || xd.get("of").map(js::to_js_string).as_deref() == Some(def.name.as_str())) {
            continue;
        }
        let Some(gx) = dyn_entities(x) else { continue };
        let xs: Vec<bool> =
            xd.get("shown").and_then(Value::as_array).map(|a| a.iter().map(|v| js::truthy(Some(v))).collect()).unwrap_or_default();
        // `X.dyn.shown.every((v, i) => v === shown[i])`: strict, so only real booleans match
        let strict = xd.get("shown").and_then(Value::as_array).is_some_and(|a| a.iter().all(Value::is_boolean));
        if strict
            && xs == shown
            && gx.len() == g.len()
            && gx.iter().zip(gplain.iter()).all(|(p, q)| plain_parts(p) == *q)
            && plain_values(x.attdefs.as_deref().unwrap_or(&[])) == ad_plain
        {
            target = Some(x.name.clone());
            break;
        }
    }
    let made = if target.is_none() {
        let used: HashSet<String> = doc.blocks.keys().map(|k| k.to_uppercase()).collect();
        let mut n: u64 = 1;
        while n < 10_000_000 && used.contains(&format!("*B{n}")) {
            n = n.saturating_add(1);
        }
        let all: Vec<Vec<Entity>> = g.iter().map(|p| p.iter().map(clone_ent).collect()).collect();
        let ents: Vec<Entity> = all.iter().zip(shown.iter()).filter(|(_, s)| **s).flat_map(|(p, _)| p.clone()).collect();
        let mut nb = Block::new(&format!("*B{n}"), b.base, ents);
        if b.attdefs.as_ref().is_some_and(|a| !a.is_empty()) {
            nb.attdefs = b.attdefs.clone();
        }
        let allv: Vec<Value> = all.iter().map(|p| Value::Array(p.iter().map(Entity::to_value).collect())).collect();
        nb.dyn_ = Some(json!({ "of": def.name, "made": true, "h": [], "shown": shown, "all": allv, "ad": arr_usize(bd.get("ad")) }));
        Some(nb)
    } else {
        None
    };
    let node = vis.get("node").cloned().unwrap_or(Value::Null);
    let st_name = st.get("name").map(js::to_js_string).unwrap_or_default();
    doc.change("VISIBILITY", |d| {
        let tname = match &made {
            Some(nb) => d.define_block(nb)?.name,
            None => target.clone().unwrap_or_default(),
        };
        let rdyn = rf.get("dyn").cloned().unwrap_or_else(|| json!({}));
        let mut values: Vec<Value> = rdyn
            .get("values")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|v| {
                if v.get("node") == Some(&node) {
                    let mut o = v.as_object().cloned().unwrap_or_default();
                    o.insert("state".into(), Value::String(st_name.clone()));
                    Value::Object(o)
                } else {
                    v
                }
            })
            .collect();
        if !values.iter().any(|v| v.get("node") == Some(&node)) {
            let mut o = serde_json::Map::new();
            o.insert("node".into(), node.clone());
            o.insert("kind".into(), json!("visibility"));
            o.insert("state".into(), Value::String(st_name.clone()));
            if let Some(p) = vis.get("point").filter(|p| js::truthy(Some(p))) {
                o.insert("point".into(), p.clone());
            }
            values.push(Value::Object(o));
        }
        let mut nd = rdyn.as_object().cloned().unwrap_or_default();
        nd.insert("state".into(), Value::String(st_name.clone()));
        nd.insert("values".into(), Value::Array(values));
        let mut e = rf.clone();
        if let Geo::Insert(i) = &mut e.geo {
            i.block = tname;
        }
        e.extra.insert("dyn".into(), Value::Object(nd));
        d.update_in(id, e, space)?;
        Ok(())
    })
}

/// `followStates(doc, def, vr)`: references take renamed states; a deleted one → the first state; a reference whose
/// block no longer shows its state's objects moves to a copy that does.
fn follow_states(
    doc: &mut Doc,
    def: &str,
    renamed: &[(String, String)],
    deleted: &[String],
    current: &str,
    first: &str,
) -> Result<(), DocError> {
    let spaces: Vec<SpaceId> = std::iter::once(MODEL).chain(doc.layouts.iter().map(|l| l.space)).collect();
    for sp in spaces {
        let ids: Vec<u64> = doc.space_entities(sp).filter_map(|e| e.id).collect();
        for id in ids {
            let Some(e) = doc.space_map(sp).and_then(|m| m.get(&id)).cloned() else { continue };
            let Geo::Insert(ins) = &e.geo else { continue };
            let Some(edyn) = e.get("dyn").filter(|v| js::truthy(Some(v))).cloned() else { continue };
            let ok = doc.blocks.get(&ins.block).is_some_and(|b| {
                b.dyn_.as_ref().is_some_and(|bd| b.name == def || bd.get("of").map(js::to_js_string).as_deref() == Some(def))
            });
            if !ok {
                continue;
            }
            let was: Option<String> = edyn.get("state").filter(|v| !v.is_null()).map(js::to_js_string);
            let was_t = was.as_deref().filter(|s| !s.is_empty());
            let gone = was_t.is_some_and(|w| deleted.iter().any(|x| x == w));
            let now: Option<String> = match was_t {
                Some(_) if gone => Some(first.to_string()),
                Some(w) => Some(renamed.iter().find(|(o, _)| o == w).map_or_else(|| w.to_string(), |(_, n)| n.clone())),
                None => was.clone(),
            };
            if now != was {
                let mut ne = e.clone();
                if gone && let Geo::Insert(i) = &mut ne.geo {
                    i.block = def.to_string();
                }
                let mut nd = edyn.as_object().cloned().unwrap_or_default();
                let nv = now.clone().map_or(Value::Null, Value::String);
                let values: Vec<Value> = edyn
                    .get("values")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|v| {
                        let vs = v.get("state").cloned().unwrap_or(Value::Null);
                        let wv = was.clone().map_or(Value::Null, Value::String);
                        if vs == wv {
                            let mut o = v.as_object().cloned().unwrap_or_default();
                            o.insert("state".into(), nv.clone());
                            Value::Object(o)
                        } else {
                            v
                        }
                    })
                    .collect();
                nd.insert("state".into(), nv);
                nd.insert("values".into(), Value::Array(values));
                ne.extra.insert("dyn".into(), Value::Object(nd));
                doc.change("BEDIT", |d| d.update_in(id, ne, sp).map(|_| ()))?;
            }
            let Some(cur) = doc.space_map(sp).and_then(|m| m.get(&id)).cloned() else { continue };
            let Geo::Insert(ci) = &cur.geo else { continue };
            let st: Option<String> = cur
                .get("dyn")
                .and_then(|d| d.get("state"))
                .filter(|v| !v.is_null())
                .map(js::to_js_string)
                .or_else(|| (ci.block == def).then(|| current.to_string()));
            let Some(st) = st else { continue };
            let want = doc
                .blocks
                .get(def)
                .and_then(|b| b.dyn_.as_ref())
                .and_then(|d| d.get("vis"))
                .and_then(|v| v.get("states"))
                .and_then(Value::as_array)
                .and_then(|a| a.iter().find(|s| s.get("name").map(js::to_js_string).as_deref() == Some(st.as_str())))
                .map(|s| arr_usize(s.get("show")));
            if let Some(show) = want
                && !shows_state(doc.blocks.get(&ci.block).and_then(|b| b.dyn_.as_ref()), &show)
            {
                set_visibility(doc, id, &st, sp)?;
            }
        }
    }
    Ok(())
}

/// The block editor's close with visibility states (closeBlockEdit's DB.9 branch): define the block with its states and
/// let references follow → false when the states no longer line up (saved plain: the note is said).
pub fn save_with_states(
    doc: &mut Doc,
    v: &mut VisEdit,
    space: SpaceId,
    base: cc_geom::P2,
    entities: &[Entity],
    attdefs: &[Value],
    name: &str,
) -> Result<bool, DocError> {
    match close_vis(doc, v, space, base, entities) {
        Closed::Block { mut block, renamed, deleted, current, first } => {
            if !attdefs.is_empty() {
                block.attdefs = Some(attdefs.to_vec());
            }
            doc.change("BEDIT", |d| d.define_block(&block).map(|_| ()))?;
            follow_states(doc, name, &renamed, &deleted, &current, &first)?;
            Ok(true)
        }
        Closed::Plain => Ok(false),
    }
}

// ---------------------------------------------------------------------------------------------- commands

/// `visEditor(c)`: the open block editor with visibility states, or why not (printed).
fn vis_editor(c: &Ctx) -> Option<(cc_cad::SpaceId, String)> {
    let be = c.st().memo.block_edit.clone();
    let Some(be) = be else {
        c.print(&c.t("cmd.bvstate.no_editor"));
        return None;
    };
    if be.vis.is_none() {
        c.print(&c.tv("cmd.bvstate.no_states", &[("name", be.name)]));
        return None;
    }
    Some((be.space, be.name))
}

/// Run `f` on the editor's visibility view with the document.
fn with_vis<T>(c: &Ctx, f: impl FnOnce(&mut VisEdit, &mut Doc) -> T) -> Option<T> {
    let sh = c.sh();
    let mut st = sh.st.borrow_mut();
    let v = st.memo.block_edit.as_mut()?.vis.as_mut()?;
    let mut d = sh.doc.borrow_mut();
    Some(f(v, &mut d))
}

fn cur_state(c: &Ctx) -> String {
    c.st().memo.block_edit.as_ref().and_then(|b| b.vis.as_ref()).map(|v| v.cur.clone()).unwrap_or_default()
}

fn state_err(c: &Ctx, e: &str, name: &str) {
    c.print(&c.tv(&format!("cmd.bvstate.err.{e}"), &[("name", name.to_string())]));
}

async fn ask(c: &Ctx, key: &str, def: Option<String>) -> R<String> {
    let r = c.string(c.t(key), Opts { default: def.map(Def::Str), ..Opts::default() }).await?;
    Ok(match r {
        Reply::Str(s) => js::trim(&s).to_string(),
        _ => String::new(),
    })
}

pub async fn bvstate(c: Ctx) -> R {
    let Some((space, _)) = vis_editor(&c) else { return Ok(()) };
    let list = with_vis(&c, |v, d| {
        sync_current(v, d, space);
        v.states.iter().map(|s| if s.name == v.cur { format!("[{}]", s.name) } else { s.name.clone() }).collect::<Vec<_>>().join(", ")
    })
    .unwrap_or_default();
    c.print(&c.tv("cmd.bvstate.list", &[("list", list)]));
    let options = vec![c.opt("S", "Set current"), c.opt("N", "New"), c.opt("R", "Rename"), c.opt("D", "Delete")];
    let k = c
        .keyword(
            c.tv("cmd.bvstate.ask", &[("state", cur_state(&c))]),
            Opts { options, default: Some(Def::Str("Set current".into())), ..Opts::default() },
        )
        .await?;
    let o = match k {
        Reply::Opt(s) => s,
        Reply::Str(s) if s == "Set current" => "S".into(),
        Reply::Str(s) => s,
        _ => String::new(),
    };
    match o.as_str() {
        "S" => {
            let name = ask(&c, "cmd.bvstate.which", None).await?;
            if name.is_empty() {
                return Ok(());
            }
            let e = with_vis(&c, |v, d| switch_state(d, v, space, &name)).transpose().map_err(crate::engine::doc_err)?.flatten();
            match e {
                Some(e) => state_err(&c, e, &name),
                None => c.print(&c.tv("cmd.bvstate.current", &[("state", cur_state(&c))])),
            }
        }
        "N" => {
            let name = ask(&c, "cmd.bvstate.new_name", None).await?;
            if name.is_empty() {
                return Ok(());
            }
            let options = vec![c.opt("H", "Hide all"), c.opt("S", "Show all"), c.opt("K", "Keep current")];
            let h = c
                .keyword(c.t("cmd.bvstate.new_how"), Opts { options, default: Some(Def::Str("Keep current".into())), ..Opts::default() })
                .await?;
            let how = match h {
                Reply::Opt(s) if s == "H" => "hide",
                Reply::Opt(s) if s == "S" => "show",
                _ => "keep",
            };
            let e = with_vis(&c, |v, d| new_state(v, d, space, &name, how)).flatten();
            if let Some(e) = e {
                state_err(&c, e, &name);
                return Ok(());
            }
            with_vis(&c, |v, d| switch_state(d, v, space, &name)).transpose().map_err(crate::engine::doc_err)?;
            c.print(&c.tv("cmd.bvstate.made", &[("state", name)]));
        }
        "R" => {
            let from = ask(&c, "cmd.bvstate.which", Some(cur_state(&c))).await?;
            let to = if from.is_empty() { String::new() } else { ask(&c, "cmd.bvstate.new_name", None).await? };
            if to.is_empty() {
                return Ok(());
            }
            match with_vis(&c, |v, _| rename_state(v, &from, &to)).flatten() {
                Some(e) => state_err(&c, e, &from),
                None => c.print(&c.tv("cmd.bvstate.renamed", &[("from", from), ("to", to)])),
            }
        }
        "D" => {
            let name = ask(&c, "cmd.bvstate.which", None).await?;
            if name.is_empty() {
                return Ok(());
            }
            match with_vis(&c, |v, _| delete_state(v, &name)).flatten() {
                Some(e) => state_err(&c, e, &name),
                None => c.print(&c.tv("cmd.bvstate.deleted", &[("state", name)])),
            }
        }
        _ => {}
    }
    Ok(())
}

pub async fn bvhide(c: Ctx) -> R {
    let Some((space, _)) = vis_editor(&c) else { return Ok(()) };
    let ids = c.select(Some(c.t("cmd.bvhide.select")), false).await?;
    if ids.is_empty() {
        return Ok(());
    }
    let n = with_vis(&c, |v, d| -> Result<usize, DocError> {
        let mut fis: Vec<usize> = Vec::new();
        for id in &ids.0 {
            if let Some(fi) = v.fi_of.get(id)
                && !fis.contains(fi)
            {
                fis.push(*fi);
            }
        }
        let mut n = 0;
        if !fis.is_empty() {
            n = d.change("BVHIDE", |d| move_out(d, v, space, &fis))?;
        }
        sync_current(v, d, space);
        Ok(n)
    })
    .transpose()
    .map_err(crate::engine::doc_err)?
    .unwrap_or(0);
    c.print(&if n > 0 { c.tv("cmd.bvhide.done", &[("n", n.to_string()), ("state", cur_state(&c))]) } else { c.t("cmd.bvhide.none") });
    Ok(())
}

pub async fn bvshow(c: Ctx) -> R {
    let Some((space, _)) = vis_editor(&c) else { return Ok(()) };
    let mut fis: Vec<usize> = Vec::new();
    loop {
        let hidden: Vec<(usize, Vec<Entity>)> = with_vis(&c, |v, d| {
            let shown: Vec<usize> = in_space(v, d, space).into_iter().map(|(k, _)| k).collect();
            v.store.iter().filter(|(fi, _)| !shown.contains(fi)).cloned().collect::<Vec<_>>()
        })
        .unwrap_or_default()
        .into_iter()
        .filter(|(fi, _)| !fis.contains(fi))
        .collect();
        if hidden.is_empty() {
            if fis.is_empty() {
                c.print(&c.tv("cmd.bvshow.none", &[("state", cur_state(&c))]));
            }
            break;
        }
        let ghosts: Vec<Entity> = hidden
            .iter()
            .flat_map(|(_, p)| p.iter().cloned())
            .map(|mut e| {
                e.extra.insert("ghost".into(), Value::Bool(true));
                e
            })
            .collect();
        c.preview(Some(Rc::new(move |_, _| ghosts.clone())));
        let p = c.point(c.t("cmd.bvshow.pick"), Opts { none: true, ..Opts::default() }).await?;
        let Some(p) = p.point() else { break };
        let tol = c.sh().host.borrow().pick_radius().unwrap_or(1.0);
        let mut best: Option<usize> = None;
        let mut bd = f64::INFINITY;
        {
            let d = c.doc();
            for (fi, parts) in &hidden {
                for part in parts {
                    for q in prims(part, Some(&d.blocks)) {
                        let dd = dist_to_prim(&q, p);
                        if dd < bd {
                            bd = dd;
                            best = Some(*fi);
                        }
                    }
                }
            }
        }
        match best {
            Some(fi) if bd <= tol => fis.push(fi),
            _ => c.print(&c.t("cmd.bvshow.miss")),
        }
    }
    if fis.is_empty() {
        return Ok(());
    }
    let n = with_vis(&c, |v, d| -> Result<usize, DocError> {
        let n = d.change("BVSHOW", |d| move_in(d, v, space, &fis))?;
        sync_current(v, d, space);
        Ok(n)
    })
    .transpose()
    .map_err(crate::engine::doc_err)?
    .unwrap_or(0);
    c.print(&c.tv("cmd.bvshow.done", &[("n", n.to_string()), ("state", cur_state(&c))]));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn door() -> Doc {
        let la = json!({ "type": "line", "layer": "0", "x1": 0, "y1": 0, "x2": 10, "y2": 0 });
        let ar = json!({ "type": "arc", "layer": "0", "cx": 0, "cy": 0, "r": 10, "a0": 0, "a1": 1.5 });
        let lb = json!({ "type": "line", "layer": "0", "x1": 10, "y1": 0, "x2": 10, "y2": 10 });
        let mut d = Doc::new();
        d.load(&json!({ "blocks": [{ "name": "Door", "base": { "x": 0, "y": 0 }, "entities": [la, ar],
            "dyn": { "def": "h1", "vis": { "node": 7, "states": [{ "name": "Open", "show": [0, 1] }, { "name": "Closed", "show": [0, 2] }] },
                "all": [[la], [ar], [lb]], "shown": [true, true, false], "ad": [] } }] }));
        d
    }

    #[test]
    fn states_switch_rename_and_close_back_into_the_definition() {
        let mut d = door();
        let blk = d.blocks.get("Door").cloned().unwrap();
        let (mut v, ents) = open_vis(&mut d, &blk).unwrap();
        assert_eq!((v.cur.as_str(), ents.len(), v.store.len()), ("Open", 2, 1));
        let sp = d.loose_space(ents);
        assert_eq!(switch_state(&mut d, &mut v, sp, "closed").unwrap(), None);
        assert_eq!(v.cur, "Closed");
        assert_eq!(d.space_entities(sp).count(), 2);
        assert_eq!(rename_state(&mut v, "open", "Ajar"), None);
        assert_eq!(delete_state(&mut v, "closed"), Some("current"));
        assert_eq!(new_state(&mut v, &d, sp, "a/b", "keep"), Some("bad-name"));
        let list: Vec<Entity> = d.space_entities(sp).cloned().collect();
        match close_vis(&d, &mut v, sp, cc_geom::P2::new(0.0, 0.0), &list) {
            Closed::Block { block, renamed, .. } => {
                assert_eq!(renamed, vec![("Open".to_string(), "Ajar".to_string())]);
                let dy = block.dyn_.unwrap();
                assert_eq!(dy["shown"], json!([true, false, true]));
                assert_eq!(dy["visEdited"], json!(true));
                assert_eq!(dy["vis"]["states"][0], json!({ "name": "Ajar", "show": [0, 1], "src": "Open" }));
            }
            Closed::Plain => panic!("expected the block with its states"),
        }
    }

    #[test]
    fn a_block_whose_objects_no_longer_line_up_opens_plain() {
        let mut d = door();
        let mut blk = d.blocks.get("Door").cloned().unwrap();
        blk.entities.pop();
        assert!(open_vis(&mut d, &blk).is_none());
    }
}
