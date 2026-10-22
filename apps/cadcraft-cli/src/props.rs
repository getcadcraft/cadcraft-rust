//! Dev: `cadcraft-cli props <script.json>` — the Properties panel's model (cc-engine `props`: propdefs.js +
//! propexpr.js) on a script, for `forks/tools/propsdiff.mjs` (which runs the frozen JS on the same script).
//!
//! Script: `{ strings, doc, steps: [{ sel: [ids], group?, vertex?, set?: { key, value } | { key, raw } }] }`. Each step
//! selects objects, picks a group (default: the first), and optionally sets a field: `value` as the panel's control
//! hands it over, or `raw` text read as a number field reads it (propexpr.js readValue). Output per step: the groups,
//! the fields of the group (key, section, kind, label, value — "$varies" when the objects differ — and choices), what
//! the change did, and the model and undo labels afterwards.

use std::process::ExitCode;

use cc_cad::Doc;
use cc_engine::Lang;
use cc_engine::props::{PCtx, PVal, ReadCtx, apply_value, defs_for, group_label, groups_of, read_value};
use serde_json::{Value, json};

fn run(script: &Value) -> Vec<Value> {
    let lang = Lang::new(script.get("strings").unwrap_or(&Value::Null));
    let mut doc = Doc::new();
    if let Some(d) = script.get("doc").filter(|d| d.is_object()) {
        doc.load(d);
    }
    let empty = Vec::new();
    let steps = script.get("steps").and_then(Value::as_array).unwrap_or(&empty);
    let mut out = Vec::new();
    for st in steps {
        // M4.31: the open space first (-1 = the model, else a sheet's index)
        if let Some(sp) = st.get("space").and_then(Value::as_i64) {
            let id = usize::try_from(sp).ok().and_then(|i| doc.layouts.get(i)).map_or(cc_cad::MODEL, |l| l.space);
            doc.set_space_id(id);
        }
        let ids: Vec<u64> =
            st.get("sel").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
        let vertex = st.get("vertex").and_then(Value::as_i64).unwrap_or(0);
        let ents: Vec<cc_cad::Entity> = ids.iter().filter_map(|id| doc.get(*id).cloned()).collect();
        let groups = groups_of(&ents, &lang);
        let gkey = st
            .get("group")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| groups.first().map(|g| g.key.clone()))
            .unwrap_or_default();
        let gids: Vec<u64> = groups.iter().find(|g| g.key == gkey).map(|g| g.ids.clone()).unwrap_or_default();
        let gents: Vec<cc_cad::Entity> = gids.iter().filter_map(|id| doc.get(*id).cloned()).collect();
        let mut defs_v = Vec::new();
        // the value to set (read while the group's fields are known), applied after
        let mut to_set: Option<(String, Result<Value, String>)> = None;
        {
            let c = PCtx { doc: &doc, ents: &gents, vertex, lang: &lang };
            let defs = defs_for(&gkey, &c);
            for d in &defs {
                let v = match cc_engine::props::value_of(d, &c) {
                    PVal::V(v) => v,
                    PVal::Varies => Value::String("$varies".into()),
                };
                let choices = d.choices.as_ref().map(|f| f(&c).into_iter().map(|(v, l)| json!([v, l])).collect::<Vec<_>>());
                defs_v.push(json!({ "key": d.key, "sec": d.sec, "kind": d.kind, "label": d.label, "value": v, "choices": choices, "ro": d.set.is_none() }));
            }
            if let Some(set) = st.get("set") {
                let key = set.get("key").and_then(Value::as_str).unwrap_or("").to_string();
                let def = defs.iter().find(|d| d.key == key);
                let value = match (set.get("raw"), def) {
                    (Some(raw), Some(d)) => {
                        let on_sheet = doc.layout().is_some();
                        let rc = ReadCtx {
                            lunits: doc.meta.get("lunits").and_then(Value::as_f64),
                            units: doc.meta.get("units").and_then(Value::as_str).unwrap_or("").to_string(),
                            paper: if on_sheet && !d.model { doc.layout().and_then(|l| l.unit()).map(str::to_string) } else { None },
                            gt0: d.gt0,
                            min: d.min,
                            max: d.max,
                        };
                        let kind = if d.kind == "area" { "num" } else { d.kind };
                        read_value(raw.as_str().unwrap_or(""), kind, &rc, &lang).map(cc_cad::num)
                    }
                    _ => Ok(set.get("value").cloned().unwrap_or(Value::Null)),
                };
                to_set = Some((key, value));
            }
        }
        let applied = match to_set {
            None => Value::Null,
            Some((_, Err(e))) => json!({ "read": e }),
            Some((key, Ok(v))) => {
                let r = apply_value(&mut doc, &key, &gkey, &gids, &v, vertex, &lang);
                json!({ "ok": r.ok, "changed": r.changed, "locked": r.locked, "error": r.error })
            }
        };
        let extra = st.get("extra").map_or(Value::Null, |x| extra_op(&mut doc, x, &gkey, &gids, vertex, &lang));
        let attrs = match ids.as_slice() {
            [one] => doc.get(*one).cloned().map_or(Value::Null, |e| {
                let (rows, lib) = cc_engine::propextra::attribute_rows(&doc, &e);
                json!({
                    "rows": rows.iter().map(|r| json!({ "tag": r.tag, "value": r.value, "invisible": r.invisible, "constant": r.constant, "locked": r.locked })).collect::<Vec<_>>(),
                    "lib": lib.map(|(a, b)| json!([a, b])),
                })
            }),
            _ => Value::Null,
        };
        let labels: Vec<Value> = groups.iter().map(|g| json!({ "key": g.key, "n": g.ids.len(), "label": group_label(g, &lang) })).collect();
        let model: Vec<Value> = doc.model().map(cc_cad::Entity::to_value).collect();
        let undo: Vec<String> = doc.undo_stack.iter().map(|s| s.label.clone()).collect();
        let mut rec = json!({ "groups": labels, "group": gkey, "defs": defs_v, "applied": applied, "model": model, "undo": undo });
        if script.get("more").and_then(Value::as_bool) == Some(true) {
            let layouts: Vec<Value> = doc
                .layouts
                .iter()
                .map(|l| {
                    let ents: Vec<Value> =
                        doc.space_map(l.space).map(|m| m.values().map(cc_cad::Entity::to_value).collect()).unwrap_or_default();
                    json!({ "name": l.name, "paper": l.paper, "entities": ents })
                })
                .collect();
            let blocks: Vec<Value> = doc.blocks.values().map(|b| json!({ "name": b.name, "n": b.entities.len(), "dyn": b.dyn_ })).collect();
            let layers: Vec<Value> = doc
                .layers
                .values()
                .map(|l| json!({ "name": l.name, "on": l.on, "frozen": l.frozen, "linetype": l.linetype, "lineweight": l.lineweight }))
                .collect();
            if let Value::Object(m) = &mut rec {
                m.insert("extra".into(), extra);
                m.insert("attrs".into(), attrs);
                m.insert("layouts".into(), Value::Array(layouts));
                m.insert("blocks".into(), Value::Array(blocks));
                m.insert("layers".into(), Value::Array(layers));
                m.insert("meta".into(), Value::Object(doc.meta.clone()));
                m.insert("current".into(), doc.current.to_value());
                m.insert(
                    "space".into(),
                    json!(doc.layouts.iter().position(|l| l.space == doc.space).map_or(-1, |i| i64::try_from(i).unwrap_or(-1))),
                );
            }
        }
        out.push(rec);
    }
    out
}

/// M4.31: the panel's rows that are not object fields (cc-engine `propextra`), as props.js does them.
fn extra_op(doc: &mut Doc, x: &Value, gkey: &str, gids: &[u64], vertex: i64, lang: &Lang) -> Value {
    let s = |k: &str| x.get(k).map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string)).unwrap_or_default();
    let id = x.get("id").and_then(Value::as_u64).unwrap_or(0);
    let r: Result<Value, String> = match s("op").as_str() {
        "attr" => cc_engine::propextra::set_attribute(doc, id, &s("tag"), &s("value")).map(|m| json!(m)),
        "attdef" => cc_engine::propextra::set_attdef(doc, id, &s("key"), &s("value")).map(|b| json!(b)),
        "vpscale" => cc_engine::propextra::set_viewport_scale(doc, id, &s("choice")).map(|b| json!(b)),
        "rename" => cc_engine::propextra::rename_sheet(doc, &s("name"), lang).map(|b| json!(b)),
        "paper" => {
            let portrait = x.get("portrait").and_then(Value::as_bool).unwrap_or(false);
            cc_engine::propextra::set_sheet_paper(doc, &s("key"), portrait, lang).map(|m| json!(m))
        }
        "delete" => cc_engine::propextra::delete_sheet(doc, lang).map(|m| json!(m)),
        "current" => cc_engine::propextra::set_current(doc, &s("key"), x.get("value").unwrap_or(&Value::Null), lang).map(|m| json!(m)),
        "pick" => {
            let p = (x.get("x").and_then(Value::as_f64).unwrap_or(0.0), x.get("y").and_then(Value::as_f64).unwrap_or(0.0));
            Ok(json!(cc_engine::propextra::pick_point(doc, gkey, gids, &s("pt"), p, vertex, lang)))
        }
        other => Err(format!("no op {other}")),
    };
    match r {
        Ok(v) => json!({ "ok": v }),
        Err(e) => json!({ "err": e }),
    }
}

pub fn props(rest: &[&str]) -> ExitCode {
    let Some(file) = rest.first() else {
        eprintln!("usage: cadcraft-cli props <script.json>");
        return ExitCode::from(2);
    };
    let script: Value =
        match std::fs::read(file).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string())) {
            Ok(v) => v,
            Err(e) => {
                println!("{}", json!({ "$error": { "code": "io", "message": e } }));
                return ExitCode::from(1);
            }
        };
    println!("{}", Value::Array(run(&script)));
    ExitCode::SUCCESS
}
