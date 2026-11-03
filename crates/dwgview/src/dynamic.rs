//! dwg-dynamic.js: dynamic blocks in a drawing model, read only (0.5 DB.11). A dynamic block's definition has
//! `block.dynamic = { params: [{ kind, label, states: [{ name, entities: [handles] }] }] }`; a reference's own copy
//! (`*B…`) has `dynamicOf` = the definition's record handle and holds every entity of the definition in the same order,
//! the hidden ones flagged `invisible`; an INSERT of one may carry `dyn = { def, state? }`.

use std::collections::{HashMap, HashSet};

use cc_io_cad::jsv;
use cc_io_dwg::value::{Obj, V};

/// A visibility parameter: its label and each state's entity handles.
#[derive(Clone, Debug)]
pub struct Visibility {
    pub label: Option<String>,
    pub states: Vec<(String, HashSet<String>)>,
}

/// A dynamic definition.
#[derive(Clone, Debug)]
pub struct DynDef<'a> {
    pub name: String,
    pub block: &'a Obj,
    pub vis: Option<Visibility>,
}

/// What a reference is as a dynamic block.
#[derive(Clone, Debug, PartialEq)]
pub struct DynInfo {
    pub name: String,
    pub label: Option<String>,
    /// the state shown now (None: it cannot be told)
    pub state: Option<String>,
    pub states: Vec<String>,
    pub others: Vec<String>,
}

/// One line of the Drawing panel's list.
#[derive(Clone, Debug, PartialEq)]
pub struct DynSummary {
    pub name: String,
    pub label: Option<String>,
    pub count: usize,
    /// (state, references showing it), most first; "" = not known
    pub shown: Vec<(String, usize)>,
    pub states: Vec<String>,
}

fn blocks(model: &Obj) -> &[V] {
    model.arr("blocks").map(Vec::as_slice).unwrap_or(&[])
}

/// `visibilityOf(block)`: the first parameter with states.
pub fn visibility_of(block: &Obj) -> Option<Visibility> {
    let params = jsv::arr(jsv::get(block.get("dynamic"), "params"));
    let p = params
        .iter()
        .find(|x| !jsv::arr(jsv::get(Some(x), "states")).is_empty() && matches!(jsv::get(Some(x), "states"), Some(V::Arr(_))))?;
    let label = match jsv::get(Some(p), "label") {
        Some(V::Str(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    };
    let states = jsv::arr(jsv::get(Some(p), "states"))
        .iter()
        .map(|s| {
            let name = jsv::str_nn(jsv::get(Some(s), "name"));
            let hs: HashSet<String> = jsv::arr(jsv::get(Some(s), "entities")).iter().map(|h| jsv::to_str(Some(h))).collect();
            (name, hs)
        })
        .collect();
    Some(Visibility { label, states })
}

/// `dynamicDefs(model)`: definition record handle → definition; with the blocks by name (the first of a name, as
/// `blocks.find` gives it) so a reference's block is found without a scan.
#[derive(Debug, Default)]
pub struct Defs<'a> {
    pub defs: HashMap<String, DynDef<'a>>,
    by_name: HashMap<&'a str, &'a Obj>,
}

pub fn dynamic_defs(model: &Obj) -> Defs<'_> {
    let mut out = Defs::default();
    for b in blocks(model).iter().filter_map(V::as_obj) {
        if let Some(n) = b.str("name") {
            out.by_name.entry(n).or_insert(b);
        }
        if b.truthy("dynamic") && b.truthy("record") {
            let tn = b.get("trueName");
            let name = if jsv::truthy(tn) && !jsv::to_str(tn).starts_with('*') { jsv::to_str(tn) } else { jsv::to_str(b.get("name")) };
            out.defs.insert(jsv::to_str(b.get("record")), DynDef { name, block: b, vis: visibility_of(b) });
        }
    }
    out
}

/// `stateFor(vis, shown)`: the state whose entities are exactly these handles.
fn state_for(vis: &Visibility, shown: &HashSet<String>) -> Option<String> {
    vis.states.iter().find(|(_, hs)| hs.len() == shown.len() && shown.iter().all(|h| hs.contains(h))).map(|(n, _)| n.clone())
}

/// `dynamicOf(model, insert, defs)`.
pub fn dynamic_of(model: &Obj, insert: &Obj, defs: &Defs<'_>) -> Option<DynInfo> {
    let ty = insert.str("type").unwrap_or("");
    if ty != "INSERT" && ty != "MINSERT" {
        return None;
    }
    if defs.defs.is_empty() {
        return None;
    }
    let blk = match insert.get("block") {
        Some(V::Str(n)) => defs.by_name.get(n.as_str()).copied(),
        Some(w) => blocks(model).iter().filter_map(V::as_obj).find(|b| b.get("name") == Some(w)),
        None => None,
    };
    let dy = insert.get("dyn");
    let rec = if jsv::truthy(jsv::get(dy, "def")) {
        jsv::get(dy, "def")
    } else if blk.is_some_and(|b| b.truthy("dynamicOf")) {
        blk.and_then(|b| b.get("dynamicOf"))
    } else if blk.is_some_and(|b| b.truthy("dynamic")) {
        blk.and_then(|b| b.get("record"))
    } else {
        None
    };
    let rec = rec?;
    let def = defs.defs.get(&jsv::to_str(Some(rec)))?;
    let mut state = match jsv::get(dy, "state") {
        Some(V::Str(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    };
    if state.is_none()
        && let (Some(vis), Some(blk)) = (&def.vis, blk)
    {
        let d_ents = def.block.arr("entities").map(Vec::as_slice).unwrap_or(&[]);
        let shown: HashSet<String> = if std::ptr::eq(blk, def.block) {
            d_ents
                .iter()
                .filter(|e| !jsv::truthy(jsv::get(Some(e), "invisible")))
                .map(|e| jsv::get(Some(e), "handle"))
                .filter(|h| jsv::truthy(*h))
                .map(jsv::to_str)
                .collect()
        } else {
            blk.arr("entities")
                .map(Vec::as_slice)
                .unwrap_or(&[])
                .iter()
                .enumerate()
                .filter_map(|(i, e)| {
                    if jsv::truthy(Some(e)) && !jsv::truthy(jsv::get(Some(e), "invisible")) {
                        let h = jsv::get(d_ents.get(i), "handle");
                        if jsv::truthy(h) { Some(jsv::to_str(h)) } else { None }
                    } else {
                        None
                    }
                })
                .collect()
        };
        state = state_for(vis, &shown);
    }
    let states: Vec<String> = def.vis.as_ref().map(|v| v.states.iter().map(|(n, _)| n.clone()).collect()).unwrap_or_default();
    let others = states.iter().filter(|s| Some(*s) != state.as_ref()).cloned().collect();
    Some(DynInfo { name: def.name.clone(), label: def.vis.as_ref().and_then(|v| v.label.clone()), state, states, others })
}

/// `dynamicSummary(model, defs)`: model-space references of dynamic blocks, by block.
pub fn dynamic_summary(model: &Obj, defs: &Defs<'_>) -> Vec<DynSummary> {
    let ms = blocks(model).iter().filter_map(V::as_obj).find(|b| jsv::str_nn(b.get("name")).eq_ignore_ascii_case("*model_space"));
    let mut by: Vec<DynSummary> = Vec::new();
    for e in ms.and_then(|m| m.arr("entities")).map(Vec::as_slice).unwrap_or(&[]) {
        let Some(e) = e.as_obj() else { continue };
        let Some(d) = dynamic_of(model, e, defs) else { continue };
        let i = match by.iter().position(|s| s.name == d.name) {
            Some(i) => i,
            None => {
                by.push(DynSummary { name: d.name.clone(), label: d.label.clone(), count: 0, shown: Vec::new(), states: d.states.clone() });
                by.len().saturating_sub(1)
            }
        };
        let Some(s) = by.get_mut(i) else { continue };
        s.count = s.count.saturating_add(1);
        let k = d.state.unwrap_or_default();
        match s.shown.iter_mut().find(|(x, _)| *x == k) {
            Some((_, n)) => *n = n.saturating_add(1),
            None => s.shown.push((k, 1)),
        }
    }
    for s in &mut by {
        s.shown.sort_by(|p, q| q.1.cmp(&p.1).then_with(|| cc_io_dwg::compile::collate_plain(&p.0, &q.0)));
    }
    by.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| cc_io_dwg::compile::collate_plain(&a.name, &b.name)));
    by
}
