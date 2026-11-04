//! CadCraft in a Studio Live session (0.5 cadcraft/dev/live.js, LV.8): every finished step's records become wire ops —
//! plain values, so they pack as MessagePack:
//!
//! - `{o:'add', id, sp, e, m}` an entity created; `{o:'rep', id, sp, e, m}` put back whole (or a changed type)
//! - `{o:'set', id, sp, ly, f, v | x:1, s?}` one property (or a point pair `x1+y1` with v [x, y]); `s:1` = constrained
//!   geometry: structural; `{o:'text', id, sp, ly, f, v}` text content (structural without a version)
//! - `{o:'del', id, sp, ly}`; `{o:'blk', name, v, m}` a block; `{o:'lay', name, v, m}` / `{o:'layp', name, f, v}` /
//!   `{o:'layord', v}` layers; `{o:'lo', lid, v, m}` / `{o:'lop', lid, f, v}` / `{o:'loord', v}` sheets; `{o:'meta', k, v}`;
//!   `{o:'views'|'par'|'m3d', v}`; `{o:'sty', t, name, v}`; `{o:'g', g}` the command it came from (one Ctrl+Z undoes it)
//!
//! Personal and never sent: the current layer, colour, text height and dimension settings, the split screen, the UCS,
//! the block editor's own space. Ids: client c draws ids c × 1e9 + 1 …; ids from the opened file stay. Sheets get a live
//! id: 'L0', 'L1' … in the file's order, new ones the client's prefix + a counter. A command's steps go out together when
//! it ends (one step: one undo of the session undoes the whole command, as live.js `liveUndo` does by group).
//!
//! Not here (0.5 live.js `attachUi`): other people's cursors and selections on the drawing, holds of layers and areas
//! (LIVEHOLD), the host's padlocks (LIVELOCK / LIVEUNLOCK), the same-file join (`hello.hash`).

use cc_cad::doc::{Block, Doc, Layer, LayerState, Layout, Op, StyleTable, View};
use cc_cad::entity::Entity;
use cc_cad::{MODEL, SpaceId};
use cc_live::doc::{Adapter, Cond, TouchReq};
use cc_live::msgpack::Value as Mp;
use serde_json::{Map, Value as J, json};

use crate::livestate::{ops_json, same, to_json, to_mp};

pub const SCHEMA: u64 = 1;
const ID_BLOCK: f64 = 1e9;
const PAIRS: [[&str; 2]; 5] = [["x", "y"], ["x1", "y1"], ["x2", "y2"], ["cx", "cy"], ["mx", "my"]];

/// One field of a JSON object (nothing when it is no object).
fn setk(o: &mut J, k: &str, v: J) {
    if let Some(m) = o.as_object_mut() {
        m.insert(k.to_string(), v);
    }
}
/// A command's records folded per key: (id, space, before, after) — `after` Some once seen (inner None: gone).
type EntFold = Vec<(u64, SpaceId, Option<Entity>, Option<Option<Entity>>)>;
type StyleFold = Vec<(StyleTable, String, Option<J>, Option<Option<J>>)>;

fn pair_of(f: &str) -> Option<[&'static str; 2]> {
    PAIRS.iter().copied().find(|p| p.contains(&f))
}
fn is_text(e: &J, f: &str) -> bool {
    match e.get("type").and_then(J::as_str) {
        Some("text" | "mtext") => f == "s",
        Some("mleader") => f == "text" || f == "s",
        Some("dim") => f == "text",
        _ => false,
    }
}
fn op_s<'a>(op: &'a J, k: &str) -> &'a str {
    op.get(k).and_then(J::as_str).unwrap_or("")
}
fn truthy(v: Option<&J>) -> bool {
    v.is_some_and(|v| !matches!(v, J::Null | J::Bool(false)) && v != &J::from(0) && v != &J::from(""))
}
fn layer_of(e: Option<&J>) -> J {
    let l = e.and_then(|e| e.get("layer")).filter(|l| truthy(Some(l))).cloned();
    l.unwrap_or_else(|| J::from("0"))
}
fn id_u64(v: Option<&J>) -> Option<u64> {
    v.and_then(J::as_f64).filter(|x| *x >= 0.0 && x.fract() == 0.0 && *x < 9.0e15).map(|x| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let u = x as u64;
        u
    })
}

/// The window's sheet ids (live ids of the layouts' spaces).
#[derive(Default, Debug, Clone)]
pub struct Lids {
    pub by_space: Vec<(SpaceId, String)>,
    pub counter: u64,
}

impl Lids {
    pub fn of(&mut self, space: SpaceId, prefix: &str) -> String {
        if space == MODEL {
            return String::new();
        }
        if let Some((_, l)) = self.by_space.iter().find(|(s, _)| *s == space) {
            return l.clone();
        }
        self.counter = self.counter.saturating_add(1);
        let l = if prefix.is_empty() { format!("n{}", self.counter) } else { format!("{prefix}:s{}", self.counter) };
        self.by_space.push((space, l.clone()));
        l
    }
    pub fn space(&self, lid: &str, doc: &Doc) -> Option<SpaceId> {
        if lid.is_empty() {
            return Some(MODEL);
        }
        self.by_space.iter().find(|(s, l)| l == lid && doc.layouts.iter().any(|x| x.space == *s)).map(|(s, _)| *s)
    }
    /// Live ids for the sheets as they are now (at start, after a snapshot).
    pub fn name_sheets(&mut self, doc: &Doc) {
        for (i, l) in doc.layouts.iter().enumerate() {
            if !self.by_space.iter().any(|(s, _)| *s == l.space) {
                self.by_space.push((l.space, format!("L{i}")));
            }
        }
    }
}

fn find_ent(doc: &Doc, id: u64) -> Option<(SpaceId, Entity)> {
    std::iter::once(MODEL)
        .chain(doc.layouts.iter().map(|l| l.space))
        .find_map(|s| doc.space_entities(s).find(|e| e.id == Some(id)).map(|e| (s, e.clone())))
}
fn known(doc: &Doc, s: SpaceId) -> bool {
    s == MODEL || doc.layouts.iter().any(|l| l.space == s)
}

#[derive(Default)]
struct Fold {
    ents: EntFold,
    blocks: Vec<(String, Option<Block>, Option<Option<Block>>)>,
    metas: Vec<(String, Option<J>, Option<Option<J>>)>,
    styles: StyleFold,
    lay: Option<(LayerState, LayerState)>,
    los: Option<(Vec<Layout>, Vec<Layout>)>,
    views: Option<(Vec<View>, Vec<View>)>,
    par: Option<(J, J)>,
    m3d: Option<(Option<String>, Option<String>)>,
}

fn par_json(p: &(std::sync::Arc<Vec<cc_cad::constraint::Con>>, J)) -> J {
    json!({"constraints": J::Array(p.0.iter().map(cc_cad::constraint::Con::to_value).collect()), "params": p.1.clone()})
}

/// live.js `stepOps(step, reverse)`: a command's records folded per key (first before, last after) as wire ops.
pub fn step_ops(doc: &Doc, lids: &mut Lids, prefix: &str, g: &str, steps: &[Vec<Op>], reverse: bool) -> Vec<J> {
    let mut f = Fold::default();
    for op in steps.iter().flatten() {
        match op {
            Op::Ent { space, id, before, after } => {
                // a done step need not keep the object that is in the drawing: read it there
                let after = after.as_deref().cloned().or_else(|| find_ent(doc, *id).filter(|(s, _)| s == space).map(|(_, e)| e));
                match f.ents.iter_mut().find(|x| x.0 == *id) {
                    Some(x) => x.3 = Some(after),
                    None => f.ents.push((*id, *space, before.as_deref().cloned(), Some(after))),
                }
            }
            Op::Block { name, before, after } => match f.blocks.iter_mut().find(|x| x.0 == *name) {
                Some(x) => x.2 = Some(after.clone()),
                None => f.blocks.push((name.clone(), before.clone(), Some(after.clone()))),
            },
            Op::LayerState { before, after } => {
                f.lay = Some((f.lay.take().map_or_else(|| before.clone(), |x| x.0), after.clone()));
            }
            Op::Layouts { before, after } => {
                f.los = Some((f.los.take().map_or_else(|| before.clone(), |x| x.0), after.clone()));
            }
            Op::Meta { before, after } => {
                for ((k, b), (_, a)) in before.iter().zip(after) {
                    match f.metas.iter_mut().find(|x| x.0 == *k) {
                        Some(x) => x.2 = Some(a.clone()),
                        None => f.metas.push((k.clone(), b.clone(), Some(a.clone()))),
                    }
                }
            }
            Op::Style { table, name, before, after } => match f.styles.iter_mut().find(|x| x.0 == *table && x.1 == *name) {
                Some(x) => x.3 = Some(after.clone()),
                None => f.styles.push((*table, name.clone(), before.clone(), Some(after.clone()))),
            },
            Op::Views { before, after } => f.views = Some((f.views.take().map_or_else(|| before.clone(), |x| x.0), after.clone())),
            Op::Parametric { before, after } => {
                f.par = Some((f.par.take().map_or_else(|| par_json(before), |x| x.0), par_json(after)));
            }
            Op::M3d { before, after } => {
                let b = before.as_deref().map(str::to_string);
                f.m3d = Some((f.m3d.take().map_or(b, |x| x.0), after.as_deref().map(str::to_string)));
            }
        }
    }
    let flip = |b: Option<Entity>, a: Option<Entity>| if reverse { (a, b) } else { (b, a) };
    let (mut head, mut mid, mut tail): (Vec<J>, Vec<J>, Vec<J>) = (vec![json!({"o": "g", "g": g})], vec![], vec![]);
    let mut whole: Vec<SpaceId> = vec![];
    if let Some((b, a)) = &f.los {
        let (b, a) = if reverse { (a, b) } else { (b, a) };
        for (at, l) in a.iter().enumerate() {
            let lid = lids.of(l.space, prefix);
            match b.iter().find(|x| x.space == l.space) {
                None => {
                    let ents: Vec<J> = doc.space_entities(l.space).map(Entity::to_value).collect();
                    head.push(json!({"o": "lo", "lid": lid, "m": "c", "v": {"name": l.name, "paper": J::Object(l.paper.clone()), "ents": ents, "at": at}}));
                    whole.push(l.space);
                }
                Some(was) => {
                    if was.name != l.name {
                        mid.push(json!({"o": "lop", "lid": lid, "f": "name", "v": l.name}));
                    }
                    if was.paper != l.paper {
                        mid.push(json!({"o": "lop", "lid": lid, "f": "paper", "v": J::Object(l.paper.clone())}));
                    }
                }
            }
        }
        for l in b {
            if !a.iter().any(|x| x.space == l.space) {
                let lid = lids.of(l.space, prefix);
                tail.push(json!({"o": "lo", "lid": lid, "m": "d", "v": J::Null}));
            }
        }
        let order: Vec<String> = a.iter().map(|l| lids.of(l.space, prefix)).collect();
        let was: Vec<String> = b.iter().filter(|l| a.iter().any(|x| x.space == l.space)).map(|l| lids.of(l.space, prefix)).collect();
        let o1: Vec<&String> = order.iter().filter(|l| was.contains(l)).collect();
        let o2: Vec<&String> = was.iter().filter(|l| order.contains(l)).collect();
        if o1 != o2 {
            tail.push(json!({"o": "loord", "v": order}));
        }
    }
    if let Some((b, a)) = &f.lay {
        let (b, a) = if reverse { (a, b) } else { (b, a) };
        for l in &a.list {
            match b.list.iter().find(|x| x.name == l.name) {
                None => head.push(json!({"o": "lay", "name": l.name, "m": "c", "v": l.to_value()})),
                Some(was) => {
                    let (wv, lv) = (was.to_value(), l.to_value());
                    let (Some(wo), Some(lo)) = (wv.as_object(), lv.as_object()) else { continue };
                    let mut ks: Vec<&String> = wo.keys().collect();
                    ks.extend(lo.keys().filter(|k| !wo.contains_key(*k)));
                    for k in ks {
                        if k != "name" && !same(wo.get(k), lo.get(k)) {
                            mid.push(match lo.get(k) {
                                None => json!({"o": "layp", "name": l.name, "f": k, "x": 1}),
                                Some(v) => json!({"o": "layp", "name": l.name, "f": k, "v": v}),
                            });
                        }
                    }
                }
            }
        }
        for l in &b.list {
            if !a.list.iter().any(|x| x.name == l.name) {
                tail.push(json!({"o": "lay", "name": l.name, "m": "d", "v": J::Null}));
            }
        }
        let expect: Vec<&String> = b
            .list
            .iter()
            .map(|l| &l.name)
            .filter(|n| a.list.iter().any(|x| &x.name == *n))
            .chain(a.list.iter().map(|l| &l.name).filter(|n| !b.list.iter().any(|x| &x.name == *n)))
            .collect();
        let now: Vec<&String> = a.list.iter().map(|l| &l.name).collect();
        if expect != now {
            tail.push(json!({"o": "layord", "v": now}));
        }
    }
    for (name, b, a) in &f.blocks {
        let a = a.clone().flatten();
        let (b, a) = if reverse { (a, b.clone()) } else { (b.clone(), a) };
        match (b, a) {
            (None, None) => {}
            (_, None) => tail.push(json!({"o": "blk", "name": name, "m": "d", "v": J::Null})),
            (was, Some(x)) => head.push(json!({"o": "blk", "name": name, "m": if was.is_some() { "s" } else { "c" }, "v": x.to_value()})),
        }
    }
    let constrained: Vec<u64> = {
        let p = f.par.as_ref().map(|(b, a)| if reverse { b.clone() } else { a.clone() });
        let cons = match &p {
            Some(p) => p.get("constraints").and_then(J::as_array).cloned().unwrap_or_default(),
            None => doc.constraints.iter().map(cc_cad::constraint::Con::to_value).collect(),
        };
        cons.iter()
            .flat_map(|c| c.get("refs").and_then(J::as_array).cloned().unwrap_or_default())
            .filter_map(|r| id_u64(r.get("e")))
            .collect()
    };
    for (id, space, b, a) in &f.ents {
        let (b, a) = flip(b.clone(), a.clone().flatten());
        if !known(doc, *space) && !whole.contains(space) {
            continue;
        }
        if whole.contains(space) {
            continue;
        }
        let sp = lids.of(*space, prefix);
        match (b, a) {
            (None, None) => {}
            (None, Some(a)) => mid.push(json!({"o": "add", "id": id, "sp": sp, "m": "c", "e": a.to_value()})),
            (Some(b), None) => tail.insert(0, json!({"o": "del", "id": id, "sp": sp, "ly": layer_of(Some(&b.to_value()))})),
            (Some(b), Some(a)) => {
                let (bv, av) = (b.to_value(), a.to_value());
                if bv.get("type") != av.get("type") {
                    mid.push(json!({"o": "rep", "id": id, "sp": sp, "m": "s", "e": av}));
                    continue;
                }
                let strict = constrained.contains(id) || f.par.is_some();
                let (Some(bo), Some(ao)) = (bv.as_object(), av.as_object()) else { continue };
                let mut ks: Vec<&String> = bo.keys().collect();
                ks.extend(ao.keys().filter(|k| !bo.contains_key(*k)));
                let mut done: Vec<&str> = vec![];
                for k in ks {
                    if k == "id" || done.contains(&k.as_str()) || same(bo.get(k), ao.get(k)) {
                        continue;
                    }
                    if let Some(pair) = pair_of(k) {
                        done.extend(pair);
                        let v: Vec<J> = pair.iter().map(|p| ao.get(*p).cloned().unwrap_or(J::Null)).collect();
                        let mut o = json!({"o": "set", "id": id, "sp": sp, "ly": layer_of(Some(&av)), "f": pair.join("+"), "v": v});
                        if strict {
                            setk(&mut o, "s", J::from(1));
                        }
                        mid.push(o);
                        continue;
                    }
                    let text = is_text(&av, k) && !strict;
                    let ly = if k == "layer" { layer_of(Some(&bv)) } else { layer_of(Some(&av)) };
                    let mut o = json!({"o": if text { "text" } else { "set" }, "id": id, "sp": sp, "ly": ly, "f": k});
                    match ao.get(k) {
                        None => setk(&mut o, "x", J::from(1)),
                        Some(v) => setk(&mut o, "v", v.clone()),
                    }
                    if strict && !text {
                        setk(&mut o, "s", J::from(1));
                    }
                    mid.push(o);
                }
            }
        }
    }
    for (k, b, a) in &f.metas {
        let a = a.clone().flatten();
        let (b, a) = if reverse { (a, b.clone()) } else { (b.clone(), a) };
        if same(b.as_ref(), a.as_ref()) {
            continue;
        }
        mid.push(match a {
            None => json!({"o": "meta", "k": k, "x": 1}),
            Some(v) => json!({"o": "meta", "k": k, "v": v}),
        });
    }
    for (t, name, b, a) in &f.styles {
        let a = a.clone().flatten();
        let v = if reverse { b.clone() } else { a };
        let t = match t {
            StyleTable::DimStyles => "dimStyles",
            StyleTable::TextStyles => "textStyles",
        };
        mid.push(json!({"o": "sty", "t": t, "name": name, "v": v.unwrap_or(J::Null)}));
    }
    if let Some((b, a)) = &f.views {
        let v = if reverse { b } else { a };
        mid.push(json!({"o": "views", "v": v.iter().map(View::to_value).collect::<Vec<_>>()}));
    }
    if let Some((b, a)) = &f.par {
        mid.push(json!({"o": "par", "v": if reverse { b.clone() } else { a.clone() }}));
    }
    if let Some((b, a)) = &f.m3d {
        let v = if reverse { b.clone() } else { a.clone() };
        mid.push(json!({"o": "m3d", "v": v.map_or(J::Null, J::String)}));
    }
    let mut out = head;
    out.extend(mid);
    out.extend(tail);
    if out.iter().all(|o| op_s(o, "o") == "g") { vec![] } else { out }
}

/// Run `f` as one change of the drawing that leaves no step of its own (others' edits are not this window's to undo).
fn quietly(doc: &mut Doc, f: impl FnOnce(&mut Doc) -> Result<(), cc_cad::doc::DocError>) {
    let n = doc.undo_stack.len();
    let redo = std::mem::take(&mut doc.redo_stack);
    let solving = doc.solving;
    doc.solving = true;
    let _ = doc.change("live", f);
    doc.solving = solving;
    if doc.undo_stack.len() > n {
        doc.undo_stack.truncate(n);
    }
    doc.redo_stack = redo;
    doc.version = doc.version.saturating_add(1);
}

fn layer_state_with(doc: &Doc, f: impl FnOnce(&mut Vec<Layer>)) -> LayerState {
    let mut st = doc.layer_state();
    f(&mut st.list);
    st
}

/// live.js `applyOp(op)`.
fn apply_one(d: &mut Doc, lids: &mut Lids, op: &J) -> Result<(), cc_cad::doc::DocError> {
    let id = id_u64(op.get("id"));
    match op_s(op, "o") {
        "add" | "rep" => {
            let (Some(id), Some(e)) = (id, op.get("e").filter(|e| e.is_object())) else { return Ok(()) };
            let Some(space) = lids.space(op_s(op, "sp"), d) else { return Ok(()) };
            if let Some((s, _)) = find_ent(d, id)
                && s != space
            {
                d.remove_in(id, s)?;
            }
            let ly = layer_of(Some(e));
            d.ensure_layer(ly.as_str().unwrap_or("0"));
            let mut ent = Entity::from_value(e);
            ent.id = Some(id);
            d.reinsert_in(id, ent, space)?;
        }
        "set" | "text" => {
            let Some(id) = id else { return Ok(()) };
            let Some((space, cur)) = find_ent(d, id) else { return Ok(()) };
            let mut e = cur.to_value();
            let Some(o) = e.as_object_mut() else { return Ok(()) };
            let f = op_s(op, "f");
            if f.contains('+') {
                for (i, k) in f.split('+').enumerate() {
                    match op.get("v").and_then(|v| v.get(i)).filter(|v| !v.is_null()) {
                        Some(v) => {
                            o.insert(k.to_string(), v.clone());
                        }
                        None => {
                            o.shift_remove(k);
                        }
                    }
                }
            } else if truthy(op.get("x")) {
                o.shift_remove(f);
            } else {
                o.insert(f.to_string(), op.get("v").cloned().unwrap_or(J::Null));
            }
            if f == "layer" {
                let ly = layer_of(Some(&e));
                d.ensure_layer(ly.as_str().unwrap_or("0"));
            }
            d.replace_in(id, Entity::from_value(&e), space)?;
        }
        "del" => {
            if let Some(id) = id
                && let Some((s, _)) = find_ent(d, id)
            {
                d.remove_in(id, s)?;
            }
        }
        "blk" => {
            let name = op_s(op, "name");
            match op.get("v").filter(|v| v.is_object()) {
                Some(v) => d.define_block_owned(Block::from_value(v))?,
                None => d.remove_block(name)?,
            }
        }
        "lay" => {
            let name = op_s(op, "name").to_string();
            let st = match op.get("v").filter(|v| v.is_object()) {
                Some(v) => {
                    let l = Layer::from_state_value(v);
                    layer_state_with(d, |list| match list.iter_mut().find(|x| x.name == name) {
                        Some(x) => *x = l,
                        None => list.push(l),
                    })
                }
                None if name != "0" => layer_state_with(d, |list| list.retain(|x| x.name != name)),
                None => return Ok(()),
            };
            d.set_layer_state(&st)?;
        }
        "layp" => {
            let name = op_s(op, "name").to_string();
            let Some(l) = d.layers.get(&name).cloned() else { return Ok(()) };
            let mut v = l.to_value();
            if let Some(o) = v.as_object_mut() {
                if truthy(op.get("x")) {
                    o.shift_remove(op_s(op, "f"));
                } else {
                    o.insert(op_s(op, "f").to_string(), op.get("v").cloned().unwrap_or(J::Null));
                }
            }
            let nl = Layer::from_state_value(&v);
            let st = layer_state_with(d, |list| {
                if let Some(x) = list.iter_mut().find(|x| x.name == name) {
                    *x = nl;
                }
            });
            d.set_layer_state(&st)?;
        }
        "layord" => {
            let want: Vec<String> =
                op.get("v").and_then(J::as_array).map(|a| a.iter().filter_map(J::as_str).map(str::to_string).collect()).unwrap_or_default();
            let st = layer_state_with(d, |list| {
                let mut next: Vec<Layer> = want.iter().filter_map(|n| list.iter().find(|x| &x.name == n).cloned()).collect();
                for l in list.iter() {
                    if !next.iter().any(|x| x.name == l.name) {
                        next.push(l.clone());
                    }
                }
                *list = next;
            });
            d.set_layer_state(&st)?;
        }
        "lo" => {
            let lid = op_s(op, "lid").to_string();
            let at = lids.space(&lid, d).and_then(|s| d.layouts.iter().position(|l| l.space == s));
            match op.get("v").filter(|v| v.is_object()) {
                Some(v) => {
                    if at.is_some() {
                        return Ok(());
                    }
                    let ents: Vec<Entity> = v
                        .get("ents")
                        .and_then(J::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(|e| {
                                    let mut x = Entity::from_value(e);
                                    x.id = id_u64(e.get("id"));
                                    x.id.map(|_| x)
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    for e in &ents {
                        let ly = layer_of(Some(&e.to_value()));
                        d.ensure_layer(ly.as_str().unwrap_or("0"));
                    }
                    let space = d.loose_space(ents);
                    lids.by_space.retain(|(_, l)| *l != lid);
                    lids.by_space.push((space, lid));
                    let mut list = d.layouts.clone();
                    let at = v.get("at").and_then(J::as_u64).and_then(|a| usize::try_from(a).ok()).unwrap_or(list.len()).min(list.len());
                    let paper = v.get("paper").and_then(J::as_object).cloned().unwrap_or_default();
                    list.insert(at, Layout { name: op_s(v, "name").to_string(), paper, space });
                    d.set_layouts(list)?;
                }
                None => {
                    if let Some(i) = at {
                        let mut list = d.layouts.clone();
                        list.remove(i);
                        d.set_layouts(list)?;
                    }
                }
            }
        }
        "lop" => {
            if let Some(s) = lids.space(op_s(op, "lid"), d) {
                let mut list = d.layouts.clone();
                if let Some(l) = list.iter_mut().find(|l| l.space == s) {
                    match op_s(op, "f") {
                        "name" => l.name = op_s(op, "v").to_string(),
                        "paper" => l.paper = op.get("v").and_then(J::as_object).cloned().unwrap_or_default(),
                        _ => return Ok(()),
                    }
                }
                d.set_layouts(list)?;
            }
        }
        "loord" => {
            let want: Vec<SpaceId> = op
                .get("v")
                .and_then(J::as_array)
                .map(|a| a.iter().filter_map(|l| l.as_str().and_then(|l| lids.space(l, d))).collect())
                .unwrap_or_default();
            let mut next: Vec<Layout> = want.iter().filter_map(|s| d.layouts.iter().find(|l| l.space == *s).cloned()).collect();
            for l in &d.layouts {
                if !next.iter().any(|x| x.space == l.space) {
                    next.push(l.clone());
                }
            }
            d.set_layouts(next)?;
        }
        "meta" => {
            let k = op_s(op, "k").to_string();
            if truthy(op.get("x")) {
                d.meta.shift_remove(&k);
            } else {
                let mut patch = Map::new();
                patch.insert(k, op.get("v").cloned().unwrap_or(J::Null));
                d.set_meta(&patch)?;
            }
        }
        "views" => {
            let list: Vec<View> = op.get("v").and_then(J::as_array).map(|a| a.iter().map(View::from_value).collect()).unwrap_or_default();
            d.set_views(list)?;
        }
        "sty" => {
            let Ok(t) = Doc::style_table(op_s(op, "t")) else { return Ok(()) };
            d.set_style(t, op_s(op, "name"), op.get("v").filter(|v| v.is_object()))?;
        }
        "par" => {
            let v = op.get("v").cloned().unwrap_or(J::Null);
            let cons: Vec<cc_cad::constraint::Con> = v
                .get("constraints")
                .and_then(J::as_array)
                .map(|a| a.iter().filter_map(cc_cad::constraint::Con::from_value).collect())
                .unwrap_or_default();
            let params = v.get("params").filter(|p| p.is_object()).cloned().unwrap_or_else(cc_cad::doc::empty_params);
            d.set_parametric(cons, params)?;
        }
        "m3d" => {
            if let Some(s) = op.get("v").and_then(J::as_str) {
                let before = d.m3d.clone();
                d.record_m3d("live", before, std::sync::Arc::from(s))?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// live.js `invertOp(op)`, from the drawing as it is now.
fn invert_one(d: &Doc, lids: &mut Lids, prefix: &str, op: &J) -> Option<J> {
    let id = id_u64(op.get("id"));
    Some(match op_s(op, "o") {
        "add" | "rep" => match id.and_then(|i| find_ent(d, i)) {
            Some((s, e)) => json!({"o": "rep", "id": id, "sp": lids.of(s, prefix), "m": "s", "e": e.to_value()}),
            None => json!({"o": "del", "id": id, "sp": op_s(op, "sp"), "ly": layer_of(op.get("e"))}),
        },
        "set" | "text" => {
            let (s, e) = find_ent(d, id?)?;
            let ev = e.to_value();
            let f = op_s(op, "f");
            let mut r = json!({"o": op_s(op, "o"), "id": id, "sp": lids.of(s, prefix), "ly": layer_of(Some(&ev)), "f": f});
            if truthy(op.get("s")) {
                setk(&mut r, "s", J::from(1));
            }
            if f.contains('+') {
                setk(&mut r, "v", J::Array(f.split('+').map(|k| ev.get(k).cloned().unwrap_or(J::Null)).collect()));
            } else {
                match ev.get(f) {
                    None => setk(&mut r, "x", J::from(1)),
                    Some(v) => setk(&mut r, "v", v.clone()),
                }
            }
            r
        }
        "del" => {
            let (s, e) = find_ent(d, id?)?;
            json!({"o": "rep", "id": id, "sp": lids.of(s, prefix), "m": "c", "e": e.to_value()})
        }
        "blk" => {
            let cur = d.blocks.get(op_s(op, "name")).map(Block::to_value);
            let m = match (&cur, op.get("v").is_some_and(|v| !v.is_null())) {
                (None, _) => "d",
                (Some(_), true) => "s",
                (Some(_), false) => "c",
            };
            json!({"o": "blk", "name": op_s(op, "name"), "m": m, "v": cur.unwrap_or(J::Null)})
        }
        "lay" => {
            let cur = d.layers.get(op_s(op, "name")).map(Layer::to_value);
            json!({"o": "lay", "name": op_s(op, "name"), "m": if cur.is_some() { "c" } else { "d" }, "v": cur.unwrap_or(J::Null)})
        }
        "layp" => {
            let l = d.layers.get(op_s(op, "name"))?.to_value();
            match l.get(op_s(op, "f")) {
                None => json!({"o": "layp", "name": op_s(op, "name"), "f": op_s(op, "f"), "x": 1}),
                Some(v) => json!({"o": "layp", "name": op_s(op, "name"), "f": op_s(op, "f"), "v": v}),
            }
        }
        "lo" => {
            let lid = op_s(op, "lid");
            match lids.space(lid, d).and_then(|s| d.layouts.iter().position(|l| l.space == s).map(|i| (s, i))) {
                None => json!({"o": "lo", "lid": lid, "m": "d", "v": J::Null}),
                Some((s, at)) => {
                    let l = d.layouts.get(at)?;
                    let ents: Vec<J> = d.space_entities(s).map(Entity::to_value).collect();
                    json!({"o": "lo", "lid": lid, "m": "c", "v": {"name": l.name, "paper": J::Object(l.paper.clone()), "ents": ents, "at": at}})
                }
            }
        }
        "lop" => {
            let s = lids.space(op_s(op, "lid"), d)?;
            let l = d.layouts.iter().find(|l| l.space == s)?;
            let v = if op_s(op, "f") == "name" { J::from(l.name.clone()) } else { J::Object(l.paper.clone()) };
            json!({"o": "lop", "lid": op_s(op, "lid"), "f": op_s(op, "f"), "v": v})
        }
        "loord" => json!({"o": "loord", "v": d.layouts.iter().map(|l| lids.of(l.space, prefix)).collect::<Vec<_>>()}),
        "layord" => json!({"o": "layord", "v": d.layers.keys().cloned().collect::<Vec<_>>()}),
        "meta" => match d.meta.get(op_s(op, "k")) {
            None => json!({"o": "meta", "k": op_s(op, "k"), "x": 1}),
            Some(v) => json!({"o": "meta", "k": op_s(op, "k"), "v": v}),
        },
        "views" => json!({"o": "views", "v": d.views.iter().map(View::to_value).collect::<Vec<_>>()}),
        "sty" => {
            let t = op_s(op, "t");
            let name = op_s(op, "name");
            let cur = match t {
                "dimStyles" => d.dim_styles.get(name).map(|o| J::Object(o.clone())),
                "textStyles" => d.text_styles.get(name).map(|o| J::Object(o.clone())),
                _ => None,
            };
            json!({"o": "sty", "t": t, "name": name, "v": cur.unwrap_or(J::Null)})
        }
        "par" => json!({"o": "par", "v": par_json(&(d.constraints.clone(), d.params.clone()))}),
        "m3d" => json!({"o": "m3d", "v": d.m3d.as_deref().map_or_else(|| op.get("v").cloned().unwrap_or(J::Null), J::from)}),
        "g" => op.clone(),
        _ => return None,
    })
}

/// Adds one `touched` entry (key, mode, field, condition, guarded) unless it is there already.
type AddTouch<'a> = dyn FnMut(String, char, Option<String>, Cond, bool, &mut Vec<TouchReq>) + 'a;

/// live.js `touched(ops)`.
pub fn touched(ops: &[J]) -> Vec<TouchReq> {
    let mut out: Vec<TouchReq> = vec![];
    let mut seen = std::collections::HashSet::new();
    let mut add = |k: String, m: char, f: Option<String>, r: Cond, g: bool, out: &mut Vec<TouchReq>| {
        if seen.insert(format!("{k}\u{0}{m}\u{0}{}", f.clone().unwrap_or_default())) {
            out.push(TouchReq { k, m, f, r, g });
        }
    };
    let txt = crate::livestate::id_text;
    for op in ops {
        let id = txt(op.get("id").unwrap_or(&J::Null));
        let sheet = |out: &mut Vec<TouchReq>, add: &mut AddTouch<'_>| {
            let sp = op_s(op, "sp");
            if !sp.is_empty() {
                add(format!("layout:{sp}"), 'r', None, Cond::Auto, false, out);
            }
        };
        let lay = |ly: Option<&J>, out: &mut Vec<TouchReq>, add: &mut AddTouch<'_>| {
            let l = ly.filter(|l| truthy(Some(l))).map(txt).unwrap_or_else(|| "0".into());
            add(format!("layer:{l}"), 'r', None, Cond::Auto, true, out);
        };
        let mch = |d: char| op.get("m").and_then(J::as_str).and_then(|m| m.chars().next()).unwrap_or(d);
        match op_s(op, "o") {
            "add" | "rep" => {
                add(format!("e:{id}"), if op_s(op, "o") == "add" { 'c' } else { mch('s') }, None, Cond::Auto, false, &mut out);
                lay(op.get("e").and_then(|e| e.get("layer")), &mut out, &mut add);
                sheet(&mut out, &mut add);
                if op.get("e").and_then(|e| e.get("type")).and_then(J::as_str) == Some("insert")
                    && let Some(b) = op.get("e").and_then(|e| e.get("block")).filter(|b| truthy(Some(b)))
                {
                    add(format!("block:{}", txt(b)), 'r', None, Cond::Auto, false, &mut out);
                }
            }
            "set" => {
                if truthy(op.get("s")) {
                    add(format!("e:{id}"), 's', None, Cond::Auto, false, &mut out);
                } else {
                    add(format!("e:{id}"), 'p', Some(op_s(op, "f").to_string()), Cond::Auto, false, &mut out);
                }
                lay(op.get("ly"), &mut out, &mut add);
                sheet(&mut out, &mut add);
                if op_s(op, "f") == "layer" && !truthy(op.get("x")) {
                    lay(op.get("v"), &mut out, &mut add);
                }
                if op_s(op, "f") == "block"
                    && let Some(b) = op.get("v").and_then(J::as_str)
                {
                    add(format!("block:{b}"), 'r', None, Cond::Auto, false, &mut out);
                }
            }
            "text" => {
                add(format!("e:{id}"), 's', None, Cond::None, false, &mut out);
                lay(op.get("ly"), &mut out, &mut add);
                sheet(&mut out, &mut add);
            }
            "del" => {
                add(format!("e:{id}"), 'd', None, Cond::Auto, false, &mut out);
                lay(op.get("ly"), &mut out, &mut add);
                sheet(&mut out, &mut add);
            }
            "blk" => add(
                format!("block:{}", op_s(op, "name")),
                mch(if op.get("v").is_some_and(|v| !v.is_null()) { 's' } else { 'd' }),
                None,
                Cond::Auto,
                false,
                &mut out,
            ),
            "lay" => add(
                format!("layer:{}", op_s(op, "name")),
                mch(if op.get("v").is_some_and(|v| !v.is_null()) { 'c' } else { 'd' }),
                None,
                Cond::Auto,
                false,
                &mut out,
            ),
            "layp" => add(format!("layer:{}", op_s(op, "name")), 'p', Some(op_s(op, "f").to_string()), Cond::Auto, false, &mut out),
            "lo" => add(
                format!("layout:{}", op_s(op, "lid")),
                mch(if op.get("v").is_some_and(|v| !v.is_null()) { 'c' } else { 'd' }),
                None,
                Cond::Auto,
                false,
                &mut out,
            ),
            "lop" => add(format!("layout:{}", op_s(op, "lid")), 'p', Some(op_s(op, "f").to_string()), Cond::Auto, false, &mut out),
            "loord" => add("meta:layouts".into(), 's', None, Cond::Auto, false, &mut out),
            "layord" => add("meta:layers".into(), 'p', Some("v".into()), Cond::Auto, false, &mut out),
            "meta" => add(format!("meta:{}", op_s(op, "k")), 'p', Some("v".into()), Cond::Auto, false, &mut out),
            "views" => add("meta:views".into(), 'p', Some("v".into()), Cond::Auto, false, &mut out),
            "units" => add("meta:units".into(), 'p', Some("v".into()), Cond::Auto, false, &mut out),
            "sty" => add(format!("style:{}/{}", op_s(op, "t"), op_s(op, "name")), 'p', Some("v".into()), Cond::Auto, false, &mut out),
            "par" => add("meta:parametric".into(), 's', None, Cond::Auto, false, &mut out),
            "m3d" => add("meta:m3d".into(), 's', None, Cond::Auto, false, &mut out),
            _ => {}
        }
    }
    out
}

fn hatch_ok(loops: Option<&J>) -> bool {
    let Some(J::Array(ls)) = loops else { return false };
    !ls.is_empty()
        && ls.iter().all(|l| match l {
            J::Array(a) => a.len() >= 6 && a.len() % 2 == 0 && a.iter().all(|x| x.as_f64().is_some_and(f64::is_finite)),
            J::Object(_) => true,
            _ => false,
        })
}

/// live.js `validate(ops)`: the host's checks before a commit.
pub fn validate(d: &Doc, ops: &[J]) -> Option<String> {
    let mut blocks_after: Vec<String> = d.blocks.keys().cloned().collect();
    let mut gone: Vec<u64> = vec![];
    let mut added: Vec<(u64, J)> = vec![];
    let mut layers_gone: Vec<String> = vec![];
    for op in ops {
        let id = id_u64(op.get("id"));
        match op_s(op, "o") {
            "blk" => {
                let n = op_s(op, "name").to_string();
                if op.get("v").is_some_and(|v| !v.is_null()) {
                    if !blocks_after.contains(&n) {
                        blocks_after.push(n);
                    }
                } else {
                    blocks_after.retain(|x| *x != n);
                }
            }
            "del" => gone.extend(id),
            "add" | "rep" => {
                if let (Some(i), Some(e)) = (id, op.get("e")) {
                    added.retain(|(x, _)| *x != i);
                    added.push((i, e.clone()));
                    gone.retain(|x| *x != i);
                }
            }
            "lay" if op.get("v").is_none_or(J::is_null) => {
                if op_s(op, "name") == "0" {
                    return Some("layer-zero".into());
                }
                layers_gone.push(op_s(op, "name").to_string());
            }
            "lay" => layers_gone.retain(|x| x != op_s(op, "name")),
            "set" if op_s(op, "f") == "layer" => {
                if let Some(i) = id {
                    let cur =
                        added.iter().find(|(x, _)| *x == i).map(|(_, e)| e.clone()).or_else(|| find_ent(d, i).map(|(_, e)| e.to_value()));
                    if let Some(mut c) = cur {
                        setk(&mut c, "layer", if truthy(op.get("x")) { J::from("0") } else { op.get("v").cloned().unwrap_or(J::Null) });
                        added.retain(|(x, _)| *x != i);
                        added.push((i, c));
                    }
                }
            }
            _ => {}
        }
    }
    for op in ops {
        let o = op_s(op, "o");
        if (o == "add" || o == "rep")
            && let Some(e) = op.get("e")
        {
            if op_s(e, "type") == "insert" && truthy(e.get("block")) && !blocks_after.iter().any(|b| b == op_s(e, "block")) {
                return Some("no-block".into());
            }
            if op_s(e, "type") == "hatch" && !hatch_ok(e.get("loops")) {
                return Some("hatch".into());
            }
        }
        if o == "set"
            && op_s(op, "f") == "block"
            && !truthy(op.get("x"))
            && let Some(b) = op.get("v").and_then(J::as_str)
            && !blocks_after.iter().any(|x| x == b)
            && id_u64(op.get("id")).and_then(|i| find_ent(d, i)).is_some_and(|(_, e)| op_s(&e.to_value(), "type") == "insert")
        {
            return Some("no-block".into());
        }
        if o == "set" && op_s(op, "f") == "loops" && !truthy(op.get("x")) && !hatch_ok(op.get("v")) {
            return Some("hatch".into());
        }
        if o == "blk"
            && let Some(ents) = op.get("v").and_then(|v| v.get("entities")).and_then(J::as_array)
            && ents
                .iter()
                .any(|e| op_s(e, "type") == "insert" && truthy(e.get("block")) && !blocks_after.iter().any(|b| b == op_s(e, "block")))
        {
            return Some("no-block".into());
        }
    }
    let removed: Vec<String> = ops
        .iter()
        .filter(|op| op_s(op, "o") == "blk" && op.get("v").is_none_or(J::is_null))
        .map(|op| op_s(op, "name").to_string())
        .filter(|n| !blocks_after.contains(n))
        .collect();
    if !removed.is_empty() || !layers_gone.is_empty() {
        let check = |e: &J, id: u64| -> Option<&'static str> {
            if gone.contains(&id) {
                return None;
            }
            if truthy(e.get("block")) && removed.iter().any(|b| b == op_s(e, "block")) {
                return Some("block-in-use");
            }
            let ly = layer_of(Some(e));
            if layers_gone.iter().any(|l| Some(l.as_str()) == ly.as_str()) {
                return Some("layer-in-use");
            }
            None
        };
        for e in d.every_entity() {
            let Some(i) = e.id else { continue };
            let v = added.iter().find(|(x, _)| *x == i).map_or_else(|| e.to_value(), |(_, e)| e.clone());
            if let Some(r) = check(&v, i) {
                return Some(r.into());
            }
        }
        for (i, e) in &added {
            if find_ent(d, *i).is_none()
                && let Some(r) = check(e, *i)
            {
                return Some(r.into());
            }
        }
        for b in d.blocks.values() {
            if blocks_after.contains(&b.name)
                && b.entities
                    .iter()
                    .any(|e| truthy(e.to_value().get("block")) && removed.iter().any(|r| *r == op_s(&e.to_value(), "block")))
            {
                return Some("block-in-use".into());
            }
        }
    }
    let par = ops.iter().rev().find(|op| op_s(op, "o") == "par");
    if let Some(cons) = par.and_then(|p| p.get("v")).and_then(|v| v.get("constraints")).and_then(J::as_array) {
        for c in cons {
            for r in c.get("refs").and_then(J::as_array).map_or(&[][..], Vec::as_slice) {
                let Some(e) = id_u64(r.get("e")) else { continue };
                if gone.contains(&e) || (find_ent(d, e).is_none() && !added.iter().any(|(x, _)| *x == e)) {
                    return Some("constraint".into());
                }
            }
        }
    } else if !gone.is_empty() && !d.constraints.is_empty() {
        for c in d.constraints.iter() {
            for r in c.to_value().get("refs").and_then(J::as_array).map_or(&[][..], Vec::as_slice) {
                if id_u64(r.get("e")).is_some_and(|e| gone.contains(&e)) {
                    return Some("constraint".into());
                }
            }
        }
    }
    None
}

/// live.js `state()`: the whole drawing, for a joiner (and, without what moves, what the convergence checks compare).
pub fn state(d: &Doc, lids: &mut Lids, prefix: &str) -> J {
    lids.name_sheets(d);
    json!({
        "v": SCHEMA,
        "model": d.space_entities(MODEL).map(Entity::to_value).collect::<Vec<_>>(),
        "layouts": d.layouts.iter().map(|l| json!({"lid": lids.of(l.space, prefix), "name": l.name, "paper": J::Object(l.paper.clone()), "ents": d.space_entities(l.space).map(Entity::to_value).collect::<Vec<_>>()})).collect::<Vec<_>>(),
        "layers": d.layers.values().map(Layer::to_value).collect::<Vec<_>>(),
        "blocks": d.blocks.values().map(Block::to_value).collect::<Vec<_>>(),
        "meta": J::Object(d.meta.clone()),
        "views": d.views.iter().map(View::to_value).collect::<Vec<_>>(),
        "linetypes": d.linetypes.values().map(|l| json!({"name": l.name, "description": l.description, "pattern": l.pattern})).collect::<Vec<_>>(),
        "textStyles": d.text_styles.iter().map(|(n, v)| json!([n, J::Object(v.clone())])).collect::<Vec<_>>(),
        "dimStyles": d.dim_styles.iter().map(|(n, v)| json!([n, J::Object(v.clone())])).collect::<Vec<_>>(),
        "constraints": J::Array(d.constraints.iter().map(cc_cad::constraint::Con::to_value).collect()),
        "params": d.params.clone(),
        "ucsNamed": d.ucs.as_ref().and_then(|u| u.get("named")).cloned().unwrap_or_else(|| J::Array(vec![])),
        "m3d": d.m3d.as_deref().unwrap_or(""),
    })
}

/// live.js `loadState(s)`: the drawing replaced by a host's (every object under its own id).
pub fn load_state(d: &mut Doc, lids: &mut Lids, s: &J) -> Result<(), String> {
    if s.get("v").and_then(J::as_u64) != Some(SCHEMA) {
        return Err("live snapshot: unknown format".into());
    }
    let ents = |a: Option<&J>| -> Vec<Entity> {
        a.and_then(J::as_array)
            .map(|l| {
                l.iter()
                    .filter_map(|e| {
                        let mut x = Entity::from_value(e);
                        x.id = id_u64(e.get("id"));
                        x.id.map(|_| x)
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut nd = Doc::new();
    nd.current = d.current.clone();
    lids.by_space.clear();
    let layouts: Vec<Layout> = s
        .get("layouts")
        .and_then(J::as_array)
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .map(|l| {
            let space = nd.loose_space(ents(l.get("ents")));
            lids.by_space.push((space, op_s(l, "lid").to_string()));
            Layout { name: op_s(l, "name").to_string(), paper: l.get("paper").and_then(J::as_object).cloned().unwrap_or_default(), space }
        })
        .collect();
    let model = ents(s.get("model"));
    let _ = nd.change("live", |x| {
        for e in model {
            if let Some(i) = e.id {
                x.reinsert_in(i, e, MODEL)?;
            }
        }
        let list: Vec<Layer> =
            s.get("layers").and_then(J::as_array).map(|a| a.iter().map(Layer::from_state_value).collect()).unwrap_or_default();
        let cur = x.current.layer.clone();
        x.set_layer_state(&LayerState { list, current: cur })?;
        for b in s.get("blocks").and_then(J::as_array).map_or(&[][..], Vec::as_slice) {
            x.define_block_owned(Block::from_value(b))?;
        }
        x.set_layouts(layouts)?;
        x.set_views(s.get("views").and_then(J::as_array).map(|a| a.iter().map(View::from_value).collect()).unwrap_or_default())?;
        for (t, k) in [(StyleTable::TextStyles, "textStyles"), (StyleTable::DimStyles, "dimStyles")] {
            for p in s.get(k).and_then(J::as_array).map_or(&[][..], Vec::as_slice) {
                if let (Some(n), Some(v)) = (p.get(0).and_then(J::as_str), p.get(1)) {
                    x.set_style(t, n, Some(v))?;
                }
            }
        }
        let cons: Vec<cc_cad::constraint::Con> = s
            .get("constraints")
            .and_then(J::as_array)
            .map(|a| a.iter().filter_map(cc_cad::constraint::Con::from_value).collect())
            .unwrap_or_default();
        let params = s.get("params").filter(|p| p.is_object()).cloned().unwrap_or_else(cc_cad::doc::empty_params);
        x.set_parametric(cons, params)?;
        Ok(())
    });
    if let Some(m) = s.get("meta").and_then(J::as_object) {
        nd.meta = m.clone();
    }
    for l in s.get("linetypes").and_then(J::as_array).map_or(&[][..], Vec::as_slice) {
        let name = op_s(l, "name").to_string();
        let pattern = l.get("pattern").and_then(J::as_array).map(|a| a.iter().filter_map(J::as_f64).collect()).unwrap_or_default();
        nd.linetypes.insert(name.clone(), cc_cad::doc::Linetype { name, description: op_s(l, "description").to_string(), pattern });
    }
    if let Some(m) = s.get("m3d").and_then(J::as_str).filter(|m| !m.is_empty()) {
        nd.m3d = Some(std::sync::Arc::from(m));
    }
    if !nd.layers.contains_key(&d.current.layer) {
        nd.current.layer = "0".into();
    }
    nd.undo_stack.clear();
    nd.redo_stack.clear();
    nd.version = d.version.saturating_add(1);
    nd.saved_version = nd.version;
    *d = nd;
    Ok(())
}

/// live.js `claimIds()`: new objects take ids in this client's range (after any of ours already in the drawing).
pub fn claim_ids(d: &mut Doc, c: u64) {
    #[allow(clippy::cast_precision_loss)]
    let lo = c as f64 * ID_BLOCK;
    let hi = lo + ID_BLOCK;
    let mut next = lo + 1.0;
    for e in d.every_entity() {
        #[allow(clippy::cast_precision_loss)]
        let id = e.id.unwrap_or(0) as f64;
        if id >= lo && id < hi && id >= next {
            next = id + 1.0;
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let cur = d.next_id as f64;
    let v = next.max(if cur >= lo && cur < hi { cur } else { 0.0 });
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = v as u64;
    d.next_id = n;
}

/// The adapter over the open drawing.
pub struct CadAdapter<'a> {
    pub doc: &'a mut Doc,
    pub lids: &'a mut Lids,
    pub prefix: String,
    pub c: u64,
}

impl Adapter for CadAdapter<'_> {
    fn apply(&mut self, ops: &Mp, _origin: &str) -> Result<(), String> {
        let ops = ops_json(ops);
        let lids = &mut *self.lids;
        quietly(self.doc, |d| {
            for op in &ops {
                apply_one(d, lids, op)?;
            }
            Ok(())
        });
        Ok(())
    }
    fn invert(&mut self, ops: &Mp) -> Result<Mp, String> {
        let mut out: Vec<J> = ops_json(ops).iter().rev().filter_map(|op| invert_one(self.doc, self.lids, &self.prefix, op)).collect();
        if let Some(gi) = out.iter().position(|o| op_s(o, "o") == "g")
            && gi > 0
        {
            let g = out.remove(gi);
            out.insert(0, g);
        }
        Ok(Mp::Arr(out.iter().map(to_mp).collect()))
    }
    fn touched(&self, ops: &Mp) -> Vec<TouchReq> {
        touched(&ops_json(ops))
    }
    fn snapshot(&mut self) -> Vec<u8> {
        cc_live::msgpack::pack(&to_mp(&state(self.doc, self.lids, &self.prefix))).unwrap_or_default()
    }
    fn load(&mut self, bytes: &[u8], _q: u64) -> Result<(), String> {
        let x = cc_live::msgpack::unpack(bytes).map_err(|e| e.0.to_string())?;
        load_state(self.doc, self.lids, &to_json(&x))?;
        claim_ids(self.doc, self.c);
        Ok(())
    }
    fn validate(&mut self, ops: &Mp) -> Option<String> {
        validate(self.doc, &ops_json(ops))
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The window's side (liveapps.rs)

/// CadCraft's [`crate::live::LiveView`] over the drawing open for editing (a joiner with nothing open gets a new one).
pub struct CadLive<'a>(pub &'a mut crate::UiApp);

impl CadLive<'_> {
    fn ensure(&mut self) {
        if self.0.editor.is_none() {
            let _ = self.0.run("file.new", &crate::Params::default());
        }
    }
    fn prefix(&self) -> String {
        self.0.live.as_ref().map(|s| s.doc.prefix.clone()).unwrap_or_default()
    }
    /// Undo / Redo asked for at the command line while live (the drawing's UNDO / REDO): the person's own steps.
    pub fn reverse_asked(&mut self) {
        let asks: Vec<bool> = match self.0.editor.as_ref() {
            Some(ed) => std::mem::take(&mut ed.eng.doc_mut().live_asks),
            None => return,
        };
        #[cfg(not(target_arch = "wasm32"))]
        for undo in asks {
            if let Some(l) = crate::live::reverse(self, undo) {
                let msg = match l {
                    Some(l) if !l.is_empty() => format!("{} {l}", if undo { "Undid" } else { "Redid" }),
                    _ => self
                        .0
                        .editor
                        .as_ref()
                        .map(|ed| ed.eng.t(if undo { "cmd.undo.nothing_undo" } else { "cmd.redo.nothing_redo" }))
                        .unwrap_or_default(),
                };
                if let Some(ed) = self.0.editor.as_ref() {
                    ed.eng.print(&msg);
                }
                self.0.message = msg;
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = asks;
    }
}

impl crate::live::LiveView for CadLive<'_> {
    const APP: &'static str = "cadcraft";
    const PREFIX: &'static str = "cad";
    fn live_slot(&mut self) -> &mut Option<crate::live::LiveSession> {
        &mut self.0.live
    }
    fn live_window_open(&mut self) -> &mut bool {
        &mut self.0.live_window
    }
    fn with_adapter(&mut self, c: u64, f: &mut dyn FnMut(&mut dyn Adapter)) {
        self.ensure();
        let prefix = self.prefix();
        let Some(ed) = self.0.editor.as_ref() else { return };
        let mut doc = ed.eng.doc_mut();
        let mut a = CadAdapter { doc: &mut doc, lids: &mut self.0.live_cad_lids, prefix, c };
        f(&mut a);
    }
    fn claim_ids(&mut self, c: u64) {
        self.ensure();
        if let Some(ed) = self.0.editor.as_ref() {
            claim_ids(&mut ed.eng.doc_mut(), c);
        }
    }
    fn release_ids(&mut self) {
        if let Some(ed) = self.0.editor.as_ref() {
            let mut d = ed.eng.doc_mut();
            d.live = false;
            d.undo_stack.clear();
            d.redo_stack.clear();
        }
        self.0.live_cad_lids = Lids::default();
    }
    fn live_bind(&mut self) {
        self.ensure();
        if let Some(ed) = self.0.editor.as_ref() {
            let mut d = ed.eng.doc_mut();
            d.undo_stack.clear();
            d.redo_stack.clear();
            d.live = true;
            self.0.live_cad_lids.name_sheets(&d);
        }
    }
    fn model_version(&self) -> u64 {
        self.0.editor.as_ref().map_or(0, |ed| ed.eng.doc().version)
    }
    fn after_remote(&mut self) {
        if let Some(ed) = self.0.editor.as_ref() {
            ed.eng.doc_mut().live = true;
        }
        self.0.sync_editor();
    }
    fn tr(&self, key: &str, vars: &[(&str, String)]) -> String {
        self.0.editor.as_ref().map_or_else(|| key.to_string(), |ed| ed.eng.tv(key, vars))
    }
    fn say(&mut self, msg: String) {
        if let Some(ed) = self.0.editor.as_ref() {
            ed.eng.print(&msg);
        }
        self.0.message = msg;
    }
    fn live_state(&mut self) -> Option<J> {
        let prefix = self.prefix();
        let ed = self.0.editor.as_ref()?;
        let d = ed.eng.doc();
        Some(state(&d, &mut self.0.live_cad_lids, &prefix))
    }
    fn local_steps(&mut self, g: &mut dyn FnMut() -> String) -> Vec<(String, Mp, Mp)> {
        let prefix = self.prefix();
        let Some(ed) = self.0.editor.as_ref() else { return vec![] };
        // a command's steps go out when it ends (inside it, its own undo and squash work on them)
        if ed.eng.busy() {
            return vec![];
        }
        let mut d = ed.eng.doc_mut();
        if d.undo_stack.is_empty() {
            return vec![];
        }
        let steps = std::mem::take(&mut d.undo_stack);
        let label = steps.last().map(|s| s.label.clone()).unwrap_or_default();
        let ops: Vec<Vec<Op>> = steps.into_iter().map(|s| s.ops).collect();
        let mark = g();
        let fwd = step_ops(&d, &mut self.0.live_cad_lids, &prefix, &mark, &ops, false);
        if fwd.is_empty() {
            return vec![];
        }
        let inv = step_ops(&d, &mut self.0.live_cad_lids, &prefix, &mark, &ops, true);
        let pack = |v: &[J]| Mp::Arr(v.iter().map(to_mp).collect());
        vec![(label, pack(&fwd), pack(&inv))]
    }
}
