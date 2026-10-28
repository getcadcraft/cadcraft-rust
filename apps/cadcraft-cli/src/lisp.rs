//! Dev: `cadcraft-cli lisp <programs.json> [--bench N]` — CadCraft's LISP with no drawing (cc-lisp), for the parity
//! harness (`forks/tools/lispdiff.mjs`, which runs the frozen JS interpreter on the same programs).
//!
//! Input: `[{ text }]`. Output: one record per program `{ out, ok, value, error, cancel }`: what it printed, and the
//! value of its last form as prin1 prints it (or the error that reached the top). `--bench N`: the best of N runs of
//! all programs, in ms, after one warm-up.

use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;

use serde_json::{Value, json};

fn run_one(text: &str) -> Value {
    let out = Rc::new(RefCell::new(String::new()));
    let o2 = Rc::clone(&out);
    let mut l = cc_lisp::new_lisp(None, Rc::new(move |s: &str| o2.borrow_mut().push_str(s)));
    let units = cc_lisp::value::lstr(text);
    let r = cc_lisp::block_on(l.eval_text(&units));
    let printed = out.borrow().clone();
    match r {
        Some(o) => json!({
            "out": printed,
            "ok": o.ok,
            "value": if o.ok { Value::String(l.show(&o.value, true)) } else { Value::Null },
            "error": o.error,
            "cancel": o.cancel,
        }),
        None => json!({ "out": printed, "ok": false, "value": Value::Null, "error": "(waits)", "cancel": false }),
    }
}

pub fn lisp(rest: &[&str]) -> ExitCode {
    let Some(file) = rest.first() else {
        eprintln!("usage: cadcraft-cli lisp <programs.json> [--bench N]");
        return ExitCode::from(2);
    };
    let progs: Value =
        match std::fs::read(file).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string())) {
            Ok(v) => v,
            Err(e) => {
                println!(r#"{{"$error":{{"code":"io","message":{}}}}}"#, Value::String(e));
                return ExitCode::from(1);
            }
        };
    let texts: Vec<String> =
        progs.as_array().into_iter().flatten().map(|p| p.get("text").and_then(Value::as_str).unwrap_or("").to_string()).collect();
    let bench = rest.iter().position(|a| *a == "--bench").and_then(|i| rest.get(i + 1)).and_then(|n| n.parse::<u32>().ok());
    if let Some(n) = bench {
        for t in &texts {
            run_one(t);
        }
        let mut best = f64::INFINITY;
        for _ in 0..n.max(1) {
            let t0 = std::time::Instant::now();
            for t in &texts {
                run_one(t);
            }
            best = best.min(t0.elapsed().as_secs_f64() * 1000.0);
        }
        println!("{}", json!({ "ms": best }));
        return ExitCode::SUCCESS;
    }
    let out: Vec<Value> = texts.iter().map(|t| run_one(t)).collect();
    println!("{}", Value::Array(out));
    ExitCode::SUCCESS
}
