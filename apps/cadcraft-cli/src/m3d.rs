//! `cadcraft-cli m3d`: the 3D model kernel (cc-m3d, ModelCraft's model.js) driven by op scripts — the parity
//! harness's side of forks/tools/mdiff.mjs.
//!
//!   cadcraft-cli m3d <script.json>                 run {init?, steps: [{op, path?, args}]}: one record per step
//!                                                  (toJSON, ret | err, undo / redo labels, version …) as a JSON array
//!   cadcraft-cli m3d --cases <cases.json>          function-level cases [{fn, args}] → [{ret} | {err}]
//!   cadcraft-cli m3d --bench <script.json> [--runs N]   ms per run of the steps (no records), after one warm-up

use std::process::ExitCode;
use std::time::Instant;

fn read_json(path: &str) -> Result<serde_json::Value, ExitCode> {
    let bytes = std::fs::read(path).map_err(|e| {
        crate::print_error("io", &e.to_string());
        ExitCode::from(1)
    })?;
    serde_json::from_slice(&bytes).map_err(|e| {
        crate::print_error("json", &e.to_string());
        ExitCode::from(1)
    })
}

pub fn m3d(rest: &[&str]) -> ExitCode {
    let flag = |k: &str| rest.iter().position(|a| *a == k);
    let value_of = |k: &str| flag(k).and_then(|i| rest.get(i + 1)).copied();
    if let Some(file) = value_of("--cases") {
        let v = match read_json(file) {
            Ok(v) => v,
            Err(c) => return c,
        };
        println!("{}", cc_m3d::script::run_cases(&v));
        return ExitCode::SUCCESS;
    }
    if let Some(file) = value_of("--bench") {
        let v = match read_json(file) {
            Ok(v) => v,
            Err(c) => return c,
        };
        let runs: usize = value_of("--runs").and_then(|s| s.parse().ok()).unwrap_or(5).clamp(1, 1000);
        if let Err(e) = cc_m3d::script::run_script(&v, false) {
            crate::print_error("script", &e.to_string());
            return ExitCode::from(1);
        }
        let mut ms = Vec::with_capacity(runs);
        for _ in 0..runs {
            let t = Instant::now();
            let r = cc_m3d::script::run_script(&v, false);
            ms.push(t.elapsed().as_secs_f64() * 1000.0);
            drop(r);
        }
        let best = ms.iter().copied().fold(f64::INFINITY, f64::min);
        let mut sorted = ms.clone();
        sorted.sort_by(f64::total_cmp);
        let median = sorted.get(sorted.len() / 2).copied().unwrap_or(best);
        println!("{}", serde_json::json!({ "runs": ms, "best": best, "median": median }));
        return ExitCode::SUCCESS;
    }
    let Some(file) = rest.iter().find(|a| !a.starts_with("--")) else {
        eprintln!("usage: cadcraft-cli m3d <script.json> | --cases <cases.json> | --bench <script.json> [--runs N]");
        return ExitCode::from(2);
    };
    let v = match read_json(file) {
        Ok(v) => v,
        Err(c) => return c,
    };
    match cc_m3d::script::run_script(&v, true) {
        Ok(s) => {
            println!("{s}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            crate::print_error("script", &e.to_string());
            ExitCode::from(1)
        }
    }
}
