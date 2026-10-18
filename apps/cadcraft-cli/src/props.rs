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
        let labels: Vec<Value> = groups.iter().map(|g| json!({ "key": g.key, "n": g.ids.len(), "label": group_label(g, &lang) })).collect();
        let model: Vec<Value> = doc.model().map(cc_cad::Entity::to_value).collect();
        let undo: Vec<String> = doc.undo_stack.iter().map(|s| s.label.clone()).collect();
        out.push(json!({ "groups": labels, "group": gkey, "defs": defs_v, "applied": applied, "model": model, "undo": undo }));
    }
    out
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
