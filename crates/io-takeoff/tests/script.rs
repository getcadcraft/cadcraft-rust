//! The op-script runner behind `cadcraft-cli takeoff <script.json>` (the format forks/tools/todiff.mjs drives).

use cc_io_takeoff::script;
use serde_json::{Value, json};

#[test]
fn runs_model_ops_and_function_calls() {
    let h = "a".repeat(64);
    let sk = format!("{h}:0");
    let out = script::run(&json!({ "steps": [
        { "op": "addFile", "f": { "hash": h, "name": "a.pdf", "size": 1, "pages": 1 } },
        { "op": "setScale", "sheet": sk, "scale": { "method": "preset", "preset": "a1/4", "ftPerUnit": 1.0 / 18.0, "label": "1/4\"", "cal": null } },
        { "op": "addItem", "fields": { "name": "Wall", "type": "linear", "mat": 100000 } },
        { "op": "addMark", "m": { "item": "i1", "sheet": sk, "pts": [{ "x": 0, "y": 0 }, { "x": 180, "y": 0 }] } },
        { "op": "addMark", "m": { "item": "nope", "sheet": sk, "pts": [{ "x": 0, "y": 0 }] } },
        { "op": "undo" },
        { "fn": "numStr", "args": [[0.1, 1e21, -0.0, 5e-7]] },
        { "fn": "noSuchFunction", "args": [] },
        { "op": "noSuchOp" },
    ] }));
    let r = out["records"].as_array().unwrap();
    assert_eq!(r.len(), 9);
    assert_eq!(r[2]["ret"], "i1");
    assert_eq!(r[3]["est"]["total"], 10_000.0, "10 LF at $10.00 (rate4) = 10,000 cents");
    assert!(r[4].get("err").is_some() || r[4]["ret"].is_null());
    assert_eq!(r[5]["project"]["marks"].as_array().map(Vec::len), Some(0));
    assert_eq!(r[6]["ret"], json!(["0.1", "1e+21", "0", "5e-7"]));
    assert!(r[7].get("err").is_some());
    assert!(r[8].get("err").is_some());
    // hostile scripts never panic
    for s in [
        Value::Null,
        json!({ "steps": "x" }),
        json!({ "steps": [null, 1, "x", { "op": 5 }, { "fn": "numStr" }, { "op": "addMark", "m": { "pts": "x" } }] }),
    ] {
        let _ = script::run(&s);
    }
}
