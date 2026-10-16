//! A JSON script of document operations, run step by step with a snapshot after each: the doc-ops parity harness
//! (`forks/tools/gdiff.mjs --doc`) runs the same script on the JS `Doc` and compares every step.
//!
//! A step is one of
//! `{op:'load', input}` · `{op:'change', label, edits:[…]}` · `{op:'undo'}` · `{op:'redo'}` ·
//! `{op:'setSpace', name|null}` · `{op:'mark', key}` · `{op:'squash', key, label}` ·
//! `{op:'setLayer', name, patch, current}` · `{op:'markSaved'}`;
//! an edit (inside a change) is one of
//! `{add: entity, space?}` · `{update: id, e: entity, space?}` · `{patch: id, with: {…}, space?}` (update with
//! `{...doc.get(id), ...with}`) · `{remove: id, space?}` · `{defineBlock: {name, base, entities, attdefs?}}` ·
//! `{removeBlock: name}` · `{setMeta: {…}}` · `{setStyle: [table, name, value|null]}` · `{setViews: [...]}` ·
//! `{addLayout: {name, paper}}` · `{removeLayout: name}` · `{renameLayout: [old, new]}` · `{reverseLayouts: true}` ·
//! `{setLayerState: {list, current}}` · `{setLayer: [name, patch, current]}` · `{fail: message}` (throws: the change
//! rolls back). `space`: 'model', a layout name, or absent = the open space.

use serde_json::{Value, json};

use crate::doc::{Block, Doc, DocError, LayerState, Layout, MODEL, SpaceId, View};
use crate::entity::{Entity, Obj};

fn space_of(d: &Doc, v: Option<&Value>) -> SpaceId {
    match v.and_then(Value::as_str) {
        Some("model") => MODEL,
        Some(n) => d.layouts.iter().find(|l| l.name == n).map_or(d.space, |l| l.space),
        None => d.space,
    }
}

fn edit(d: &mut Doc, e: &Value) -> Result<Value, DocError> {
    let id_of = |v: Option<&Value>| v.and_then(Value::as_u64).unwrap_or(0);
    if let Some(x) = e.get("add") {
        let s = space_of(d, e.get("space"));
        return Ok(d.add_in(Entity::from_value(x), s)?.to_value());
    }
    if let Some(id) = e.get("update") {
        let s = space_of(d, e.get("space"));
        let ent = Entity::from_value(e.get("e").unwrap_or(&Value::Null));
        return Ok(d.update_in(id_of(Some(id)), ent, s)?.map_or(Value::Null, |x| x.to_value()));
    }
    if let Some(id) = e.get("patch") {
        let s = space_of(d, e.get("space"));
        let id = id_of(Some(id));
        let Some(cur) = d.space_map(s).and_then(|m| m.get(&id)).cloned() else { return Ok(Value::Null) };
        let mut o = cur.to_obj();
        if let Some(w) = e.get("with").and_then(Value::as_object) {
            for (k, v) in w {
                o.insert(k.clone(), v.clone());
            }
        }
        return Ok(d.update_in(id, Entity::from_value(&Value::Object(o)), s)?.map_or(Value::Null, |x| x.to_value()));
    }
    if let Some(id) = e.get("remove") {
        let s = space_of(d, e.get("space"));
        d.remove_in(id_of(Some(id)), s)?;
        return Ok(Value::Null);
    }
    if let Some(b) = e.get("defineBlock") {
        return Ok(d.define_block(&Block::from_value(b))?.to_value());
    }
    if let Some(n) = e.get("removeBlock").and_then(Value::as_str) {
        d.remove_block(n)?;
        return Ok(Value::Null);
    }
    if let Some(p) = e.get("setMeta").and_then(Value::as_object) {
        d.set_meta(p)?;
        return Ok(Value::Null);
    }
    if let Some(Value::Array(a)) = e.get("setStyle") {
        let table = Doc::style_table(a.first().and_then(Value::as_str).unwrap_or(""))?;
        let name = a.get(1).and_then(Value::as_str).unwrap_or("");
        let v = a.get(2).filter(|v| !v.is_null());
        d.set_style(table, name, v)?;
        return Ok(Value::Null);
    }
    if let Some(Value::Array(a)) = e.get("setViews") {
        d.set_views(a.iter().map(View::from_value).collect())?;
        return Ok(Value::Null);
    }
    if let Some(l) = e.get("addLayout") {
        let s = d.new_space();
        let mut list = d.layouts.clone();
        list.push(Layout {
            name: l.get("name").and_then(Value::as_str).unwrap_or("").into(),
            paper: l.get("paper").and_then(Value::as_object).cloned().unwrap_or_default(),
            space: s,
        });
        d.set_layouts(list)?;
        return Ok(Value::Null);
    }
    if let Some(n) = e.get("removeLayout").and_then(Value::as_str) {
        let list = d.layouts.iter().filter(|l| l.name != n).cloned().collect();
        d.set_layouts(list)?;
        return Ok(Value::Null);
    }
    if let Some(Value::Array(a)) = e.get("renameLayout") {
        let (old, new) = (a.first().and_then(Value::as_str).unwrap_or(""), a.get(1).and_then(Value::as_str).unwrap_or(""));
        let list = d.layouts.iter().map(|l| if l.name == old { Layout { name: new.into(), ..l.clone() } } else { l.clone() }).collect();
        d.set_layouts(list)?;
        return Ok(Value::Null);
    }
    if e.get("reverseLayouts").is_some() {
        let mut list = d.layouts.clone();
        list.reverse();
        d.set_layouts(list)?;
        return Ok(Value::Null);
    }
    if let Some(st) = e.get("setLayerState") {
        let list = st
            .get("list")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(crate::doc::Layer::from_state_value).collect())
            .unwrap_or_default();
        let current = st.get("current").and_then(Value::as_str).unwrap_or("").to_string();
        d.set_layer_state(&LayerState { list, current })?;
        return Ok(Value::Null);
    }
    if let Some(Value::Array(a)) = e.get("setLayer") {
        set_layer(d, a)?;
        return Ok(Value::Null);
    }
    if let Some(m) = e.get("fail") {
        return Err(DocError::Failed(m.as_str().unwrap_or("fail").into()));
    }
    Ok(Value::Null)
}

fn set_layer(d: &mut Doc, a: &[Value]) -> Result<(), DocError> {
    let name = a.first().and_then(Value::as_str).unwrap_or("");
    let patch: Obj = a.get(1).and_then(Value::as_object).cloned().unwrap_or_default();
    let current = a.get(2).and_then(Value::as_bool).unwrap_or(false);
    d.set_layer(name, &patch, current)
}

/// Run one step → `{ result, error?, events, snap, uses }` (`uses`: blockUses of every block name).
pub fn run_step(d: &mut Doc, step: &Value, marks: &mut Vec<(String, usize)>) -> Value {
    let op = step.get("op").and_then(Value::as_str).unwrap_or("");
    let mut error = Value::Null;
    let result = match op {
        "load" => {
            d.load(step.get("input").unwrap_or(&Value::Null));
            Value::Null
        }
        "change" => {
            let label = step.get("label").and_then(Value::as_str).unwrap_or("").to_string();
            let edits = step.get("edits").and_then(Value::as_array).cloned().unwrap_or_default();
            match d.change(&label, |d| {
                let mut out = Vec::new();
                for e in &edits {
                    out.push(edit(d, e)?);
                }
                Ok(Value::Array(out))
            }) {
                Ok(v) => v,
                Err(e) => {
                    error = Value::String(e.to_string());
                    Value::Null
                }
            }
        }
        "undo" => d.undo().map_or(Value::Null, Value::String),
        "redo" => d.redo().map_or(Value::Null, Value::String),
        "setSpace" => {
            d.set_space(step.get("name").and_then(Value::as_str));
            Value::Null
        }
        "mark" => {
            marks.push((step.get("key").and_then(Value::as_str).unwrap_or("").into(), d.mark()));
            Value::Null
        }
        "squash" => {
            let key = step.get("key").and_then(Value::as_str).unwrap_or("");
            let m = marks.iter().rev().find(|(k, _)| k == key).map_or(0, |(_, m)| *m);
            d.squash(m, step.get("label").and_then(Value::as_str).unwrap_or(""));
            Value::Null
        }
        "setLayer" => {
            let a = [
                step.get("name").cloned().unwrap_or(Value::Null),
                step.get("patch").cloned().unwrap_or(Value::Null),
                step.get("current").cloned().unwrap_or(Value::Null),
            ];
            if let Err(e) = set_layer(d, &a) {
                error = Value::String(e.to_string());
            }
            Value::Null
        }
        "markSaved" => {
            d.mark_saved();
            Value::Null
        }
        _ => Value::Null,
    };
    let events: Vec<&str> = d.drain_events().into_iter().map(|e| e.name()).collect();
    let uses: Obj = d.blocks.keys().map(|n| (n.clone(), Value::from(d.block_uses(n)))).collect();
    json!({ "result": result, "error": error, "events": events, "snap": d.snapshot(), "uses": uses })
}

/// Run a whole script (an array of steps) on a new Doc.
pub fn run(steps: &[Value]) -> Vec<Value> {
    let mut d = Doc::new();
    let mut marks = Vec::new();
    steps.iter().map(|s| run_step(&mut d, s, &mut marks)).collect()
}
