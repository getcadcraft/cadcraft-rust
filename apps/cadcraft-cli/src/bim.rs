//! `cadcraft-cli bim <job.json> [--bench N]`: BimCraft's engine modules (cc-bim) on a parity job (forks/tools/bimdiff.mjs):
//! `{ models: [{ file, keyBase }], cases: [{ op, … }] }` → `{ models: [load error | null], results: [...] }` on stdout.
//! `--bench N`: best-of-N wall time of the open + load and of the cases (after one warm-up), as JSON on stderr.

use std::process::ExitCode;
use std::time::Instant;

use cc_bim::job::{open_models, run_job};
use cc_io_ifc::geo::bool_ops::ManifoldOps;
use serde_json::Value as J;

pub fn bim(rest: &[&str]) -> ExitCode {
    let Some(path) = rest.first() else {
        eprintln!("usage: cadcraft-cli bim <job.json> [--bench N]");
        return ExitCode::from(2);
    };
    let bench: usize = rest.iter().position(|a| *a == "--bench").and_then(|i| rest.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(0);
    let job: J = match std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()) {
        Some(j) => j,
        None => {
            eprintln!("cadcraft-cli bim: cannot read the job {path}");
            return ExitCode::from(1);
        }
    };
    let specs: Vec<J> = job.get("models").and_then(J::as_array).cloned().unwrap_or_default();
    let mut bytes = Vec::with_capacity(specs.len());
    for s in &specs {
        let f = s.get("file").and_then(J::as_str).unwrap_or("");
        match std::fs::read(f) {
            Ok(b) => bytes.push(b),
            Err(e) => {
                eprintln!("cadcraft-cli bim: {f}: {e}");
                return ExitCode::from(1);
            }
        }
    }
    let ops = ManifoldOps;
    if bench > 0 {
        let mut best_load = f64::INFINITY;
        let mut best_cases = f64::INFINITY;
        for i in 0..=bench {
            let t = Instant::now();
            let opened = open_models(&bytes, &specs, &ops);
            let load = t.elapsed().as_secs_f64() * 1000.0;
            let t = Instant::now();
            let out = run_job(&job, &opened);
            let cases = t.elapsed().as_secs_f64() * 1000.0;
            std::hint::black_box(out);
            if i > 0 {
                best_load = best_load.min(load);
                best_cases = best_cases.min(cases);
            }
        }
        eprintln!("{{\"load_ms\":{best_load:.3},\"cases_ms\":{best_cases:.3}}}");
    }
    let opened = open_models(&bytes, &specs, &ops);
    println!("{}", run_job(&job, &opened));
    ExitCode::SUCCESS
}
