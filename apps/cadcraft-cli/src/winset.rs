//! Dev: `cadcraft-cli winset <script.json>` — compare.js (COMPARESETTINGS) and shortcuts.js (SHORTCUTS) in cc-engine
//! on a script, for `forks/tools/winsetdiff.mjs` (which runs the frozen JS on the same script).
//!
//! Script: `{ strings, docs: [doc.load input, doc.load input], steps: [step] }`; a step is one of
//! `{ op: "items", doc, cat }` · `{ op: "compare", cat, a, b, right }` (left = docs[0], right = docs[right]) ·
//! `{ op: "copy", doc, cat, item, key, value }` · `{ op: "valueText", cat, key, v }` · `{ op: "normKey", s }` ·
//! `{ op: "keyProblem", k, taken: [[key, owner]] }` · `{ op: "clean", v, known: [names], taken }`. Output per step:
//! `{ r, docs: [snapshot, snapshot] }`.

use std::process::ExitCode;

use cc_cad::Doc;
use cc_engine::compare::{compare_items, copy_value, items_of, value_text};
use cc_engine::shortcuts::{clean_shortcuts, key_problem, norm_key};
use serde_json::{Value, json};

fn snap(d: &Doc) -> Value {
    json!({
        "layers": d.layers.values().map(cc_cad::doc::Layer::to_value).collect::<Vec<_>>(),
        "current": d.layer_state().current,
        "dimStyles": d.dim_styles.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>(),
        "textStyles": d.text_styles.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>(),
        "layouts": d.layouts.iter().map(|l| json!({ "name": l.name, "paper": l.paper })).collect::<Vec<_>>(),
        "meta": d.meta,
        "dim": d.current.dim,
        "undo": d.undo_stack.iter().map(|s| s.label.clone()).collect::<Vec<_>>(),
    })
}

fn pairs(v: Option<&Value>) -> Vec<(String, String)> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|p| Some((p.get(0)?.as_str()?.to_string(), p.get(1).and_then(Value::as_str).unwrap_or("").to_string())))
                .collect()
        })
        .unwrap_or_default()
}

fn run(script: &Value) -> Vec<Value> {
    let mut docs: Vec<Doc> = (0..2)
        .map(|i| {
            let mut d = Doc::new();
            if let Some(v) = script.get("docs").and_then(|a| a.get(i)).filter(|v| v.is_object()) {
                d.load(v);
            }
            d
        })
        .collect();
    let empty = Vec::new();
    let mut out = Vec::new();
    for st in script.get("steps").and_then(Value::as_array).unwrap_or(&empty) {
        let s = |k: &str| st.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let di = usize::try_from(st.get("doc").and_then(Value::as_u64).unwrap_or(0)).unwrap_or(0).min(1);
        let r = match s("op").as_str() {
            "items" => json!(docs.get(di).map(|d| items_of(d, &s("cat"))).unwrap_or_default()),
            "compare" => {
                let ri = usize::try_from(st.get("right").and_then(Value::as_u64).unwrap_or(0)).unwrap_or(0).min(1);
                let a = st.get("a").and_then(Value::as_str);
                let b = st.get("b").and_then(Value::as_str);
                match (docs.first(), docs.get(ri)) {
                    (Some(da), Some(db)) => json!(
                        compare_items(&s("cat"), da, a, db, b)
                            .into_iter()
                            .map(|r| json!({ "key": r.key, "a": r.a, "b": r.b, "same": r.same }))
                            .collect::<Vec<_>>()
                    ),
                    _ => Value::Null,
                }
            }
            "copy" => {
                let v = st.get("value").cloned().unwrap_or(Value::Null);
                json!(docs.get_mut(di).is_some_and(|d| copy_value(d, &s("cat"), &s("item"), &s("key"), &v)))
            }
            "valueText" => json!(value_text(&s("cat"), &s("key"), st.get("v"))),
            "normKey" => json!(norm_key(&s("s"))),
            "keyProblem" => json!(key_problem(&s("k"), &pairs(st.get("taken")))),
            "clean" => {
                let known: Vec<String> = st
                    .get("known")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_uppercase)).collect())
                    .unwrap_or_default();
                let (keys, errors) = clean_shortcuts(
                    st.get("v").unwrap_or(&Value::Null),
                    &|c: &str| known.contains(&c.to_uppercase()),
                    &pairs(st.get("taken")),
                );
                json!({
                    "keys": keys.iter().map(|(k, t)| json!([k, t])).collect::<Vec<_>>(),
                    "errors": errors.iter().map(|e| json!({ "key": e.key, "fills": e.fills.iter().map(|(k, v)| (k.clone(), json!(v))).collect::<serde_json::Map<_, _>>() })).collect::<Vec<_>>(),
                })
            }
            other => json!({ "$error": format!("no op {other}") }),
        };
        out.push(json!({ "r": r, "docs": docs.iter().map(snap).collect::<Vec<_>>() }));
    }
    out
}

pub fn winset(rest: &[&str]) -> ExitCode {
    let Some(file) = rest.first() else {
        eprintln!("usage: cadcraft-cli winset <script.json>");
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
