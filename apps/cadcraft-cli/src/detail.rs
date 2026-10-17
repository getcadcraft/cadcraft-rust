//! `cadcraft-cli detail`: DetailCraft's engine (cc-detail, cc-io-detail) driven by call scripts — the parity
//! harness's side of forks/tools/dcdiff.mjs.
//!
//!   cadcraft-cli detail <script.json>                      run {calls: [{op, args, as?, self?}]} → [{ok} | {} | {err}]
//!   cadcraft-cli detail --bench <script.json> [--runs N]   ms per run of the whole script, after one warm-up

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

fn run(v: &serde_json::Value) -> serde_json::Value {
    let start = Instant::now();
    let clock = move || start.elapsed().as_secs_f64() * 1000.0;
    cc_detail::script::run_with_timer(v, &[cc_detail::script::handle, cc_detail::script_edit::handle], Some(&clock))
}

pub fn detail(rest: &[&str]) -> ExitCode {
    let flag = |k: &str| rest.iter().position(|a| *a == k);
    let value_of = |k: &str| flag(k).and_then(|i| rest.get(i + 1)).copied();
    if let Some(file) = value_of("--bench") {
        let v = match read_json(file) {
            Ok(v) => v,
            Err(c) => return c,
        };
        let runs: usize = value_of("--runs").and_then(|s| s.parse().ok()).unwrap_or(5).clamp(1, 1000);
        drop(run(&v));
        let mut ms = Vec::with_capacity(runs);
        for _ in 0..runs {
            let t = Instant::now();
            let r = run(&v);
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
        eprintln!("usage: cadcraft-cli detail <script.json> | --bench <script.json> [--runs N]");
        return ExitCode::from(2);
    };
    let v = match read_json(file) {
        Ok(v) => v,
        Err(c) => return c,
    };
    println!("{}", run(&v));
    ExitCode::SUCCESS
}
