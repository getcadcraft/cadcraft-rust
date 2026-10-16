//! cadcraft-cli: the headless CadCraft (Rust test version).
//!
//!   cadcraft-cli info <file.dwg>                 version, objects, counts, notes (human text; --json for JSON)
//!   cadcraft-cli convert --json <file.dwg> [-o out.json]   the drawing model as JSON (the JS readDwg shape)
//!   cadcraft-cli parity <engine> <file>          fork-parity contract: engine output as JSON (engines: dwg)
//!   cadcraft-cli bench <engine> <file> --runs N  fork-parity contract: in-process timings after one warm-up
//!
//! Exit codes: 0 ok, 1 refused file / error, 2 usage, 3 engine not ported yet.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::process::ExitCode;
use std::time::Instant;

use cc_io_dwg::{Obj, ReadOpts, V, read_dwg};

fn usage() -> ExitCode {
    eprintln!(
        "usage: cadcraft-cli info [--json] <file.dwg> | convert --json <file.dwg> [-o out.json] | parity <engine> <file> | bench <engine> <file> [--runs N]"
    );
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("");
    let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    match cmd {
        "info" => info(&rest),
        "convert" => convert(&rest),
        "parity" => parity(&rest),
        "bench" => bench(&rest),
        _ => usage(),
    }
}

fn read_file(path: &str) -> Result<Vec<u8>, ExitCode> {
    std::fs::read(path).map_err(|e| {
        println!(r#"{{"$error":{{"code":"io","message":{}}}}}"#, V::Str(e.to_string()).to_json());
        ExitCode::from(1)
    })
}

/// The model of a refused file → the contract's $error (the JS reader returns a model with report.error).
fn refused(model: &Obj) -> Option<String> {
    model.obj("report").and_then(|r| r.str("error")).map(str::to_string)
}

fn print_error(code: &str, message: &str) {
    println!(r#"{{"$error":{{"code":{},"message":{}}}}}"#, V::from(code).to_json(), V::from(message).to_json());
}

fn parity(rest: &[&str]) -> ExitCode {
    let (Some(engine), Some(file)) = (rest.first(), rest.get(1)) else { return usage() };
    if *engine != "dwg" {
        return ExitCode::from(3);
    }
    let bytes = match read_file(file) {
        Ok(b) => b,
        Err(c) => return c,
    };
    let mut model = read_dwg(&bytes, &ReadOpts::default());
    if let Some(rep) = model.obj_mut("report") {
        rep.remove("ms");
    }
    if let Some(e) = refused(&model) {
        print_error("refused", &e);
        return ExitCode::from(1);
    }
    println!("{}", V::Obj(model).to_json());
    ExitCode::SUCCESS
}

fn bench(rest: &[&str]) -> ExitCode {
    let (Some(engine), Some(file)) = (rest.first(), rest.get(1)) else { return usage() };
    if *engine != "dwg" {
        return ExitCode::from(3);
    }
    let runs: usize = rest.iter().position(|a| *a == "--runs").and_then(|i| rest.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(5);
    let mut open = Vec::new();
    let mut parse = Vec::new();
    let _ = read_dwg(&std::fs::read(file).unwrap_or_default(), &ReadOpts::default()); // warm-up
    for _ in 0..runs.max(1) {
        let t = Instant::now();
        let bytes = match read_file(file) {
            Ok(b) => b,
            Err(c) => return c,
        };
        open.push(t.elapsed().as_secs_f64() * 1000.0);
        let t = Instant::now();
        let m = read_dwg(&bytes, &ReadOpts::default());
        parse.push(t.elapsed().as_secs_f64() * 1000.0);
        drop(m);
    }
    let arr = |v: &[f64]| V::Arr(v.iter().map(|&x| V::Num((x * 1000.0).round() / 1000.0)).collect()).to_json();
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    println!(
        r#"{{"open_ms":{},"parse_ms":{},"first_frame_ms":null,"runtime":"rust {} native, rayon {} threads"}}"#,
        arr(&open),
        arr(&parse),
        env!("CARGO_PKG_VERSION"),
        threads
    );
    ExitCode::SUCCESS
}

fn convert(rest: &[&str]) -> ExitCode {
    if !rest.contains(&"--json") {
        return usage();
    }
    let out = rest.iter().position(|a| *a == "-o").and_then(|i| rest.get(i + 1)).copied();
    let Some(file) = rest.iter().copied().find(|a| !a.starts_with('-') && Some(*a) != out) else { return usage() };
    let bytes = match read_file(file) {
        Ok(b) => b,
        Err(c) => return c,
    };
    let model = read_dwg(&bytes, &ReadOpts::default());
    let refused = refused(&model);
    let json = V::Obj(model).to_json();
    match out {
        Some(p) => {
            if let Err(e) = std::fs::write(p, json) {
                eprintln!("cadcraft-cli: {p}: {e}");
                return ExitCode::from(1);
            }
        }
        None => println!("{json}"),
    }
    if refused.is_some() { ExitCode::from(1) } else { ExitCode::SUCCESS }
}

fn info(rest: &[&str]) -> ExitCode {
    let json = rest.contains(&"--json");
    let Some(file) = rest.iter().copied().find(|a| !a.starts_with('-')) else { return usage() };
    let bytes = match read_file(file) {
        Ok(b) => b,
        Err(c) => return c,
    };
    let t = Instant::now();
    let model = read_dwg(&bytes, &ReadOpts::default());
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let rep = model.obj("report").cloned().unwrap_or_default();
    let mut o = Obj::new();
    o.set("file", file);
    o.set_opt("version", model.get("version").cloned());
    o.set_opt("maintenance", model.get("maintenance").cloned());
    o.set_opt("codepage", model.get("codepage").cloned());
    o.set("bytes", bytes.len());
    o.set("ms", (ms * 10.0).round() / 10.0);
    o.set_opt("objects", rep.get("objects").cloned());
    let n = |k: &str| model.arr(k).map(Vec::len).unwrap_or(0);
    o.set("blocks", n("blocks"));
    o.set("layouts", n("layouts"));
    o.set("layers", model.obj("tables").and_then(|t| t.arr("layers")).map(Vec::len).unwrap_or(0));
    o.set_opt("counts", rep.get("counts").cloned());
    o.set_opt("unknown", rep.get("unknown").cloned());
    o.set_opt("notes", rep.get("notes").cloned());
    o.set_opt("error", rep.get("error").cloned());
    if json {
        println!("{}", V::Obj(o).to_json());
    } else {
        for (k, v) in o.iter() {
            match v {
                V::Str(s) => println!("{k}: {s}"),
                V::Obj(x) => {
                    let parts: Vec<String> = x.iter().map(|(a, b)| format!("{a} {}", b.to_json())).collect();
                    println!("{k}: {}", parts.join(", "));
                }
                V::Arr(a) => {
                    println!("{k}: {}", a.len());
                    for x in a {
                        println!("  {}", x.as_str().unwrap_or(&x.to_json()));
                    }
                }
                other => println!("{k}: {}", other.to_json()),
            }
        }
    }
    if rep.has("error") { ExitCode::from(1) } else { ExitCode::SUCCESS }
}
