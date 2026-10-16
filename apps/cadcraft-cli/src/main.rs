//! cadcraft-cli: the headless CadCraft (Rust test version).
//!
//!   cadcraft-cli info <file.dwg>                 version, objects, counts, notes (human text; --json for JSON)
//!   cadcraft-cli convert --json <file.dwg> [-o out.json]   the drawing model as JSON (the JS readDwg shape)
//!   cadcraft-cli parity <engine> <file>          fork-parity contract: engine output as JSON (engines: dwg)
//!   cadcraft-cli bench <engine> <file> --runs N  fork-parity contract: in-process timings after one warm-up
//!   cadcraft-cli compile <file.dwg|file.dxf> [--json] [--bench N]   dev: the 2D display list (BimCraft's compileDrawing);
//!                                                --bench N: best-of-N compile time after one warm-up
//!   cadcraft-cli parity dxf <file.dxf>           dev: readDxfModel's model as JSON
//!
//! Exit codes: 0 ok, 1 refused file / error, 2 usage, 3 engine not ported yet.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::process::ExitCode;
use std::time::Instant;

use cc_io_dwg::compile::{compile_drawing, to_json};
use cc_io_dwg::dxf_model::{decode_dxf_bytes, try_read_dxf_model};
use cc_io_dwg::{Obj, ReadOpts, V, read_dwg};

fn usage() -> ExitCode {
    eprintln!(
        "usage: cadcraft-cli info [--json] <file.dwg> | convert --json <file.dwg> [-o out.json] | parity <engine> <file> | bench <engine> <file> [--runs N] | compile <file.dwg|file.dxf> [--json] [--bench N]"
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
        "compile" => compile(&rest),
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
    if *engine == "dxf" {
        let bytes = match read_file(file) {
            Ok(b) => b,
            Err(c) => return c,
        };
        return match try_read_dxf_model(&decode_dxf_bytes(&bytes)) {
            Ok(model) => {
                println!("{}", V::Obj(model).to_json());
                ExitCode::SUCCESS
            }
            Err(e) => {
                print_error("refused", &e);
                ExitCode::from(1)
            }
        };
    }
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

/// A DWG or DXF file → its model (DXF by extension, decoded as BimCraft does: UTF-8, else windows-1252).
fn read_model(path: &str, bytes: &[u8]) -> Result<Obj, String> {
    if path.to_ascii_lowercase().ends_with(".dxf") {
        return try_read_dxf_model(&decode_dxf_bytes(bytes));
    }
    // A refused DWG still compiles (to nothing), as BimCraft's readDwg → compileDrawing does with report.error set.
    Ok(read_dwg(bytes, &ReadOpts::default()))
}

/// Linked drawings: "<xref name>.dwg" next to the drawing (case-insensitive), as bin/test-bimcraft.mjs looks for them.
fn find_xrefs(path: &str, model: &Obj) -> Vec<(String, Obj)> {
    let dir = std::path::Path::new(path).parent().map(std::path::Path::to_path_buf).unwrap_or_default();
    let dir = if dir.as_os_str().is_empty() { std::path::PathBuf::from(".") } else { dir };
    let names: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| rd.filter_map(Result::ok).filter_map(|e| e.file_name().into_string().ok()).collect())
        .unwrap_or_default();
    let mut out = Vec::new();
    for b in model.arr("blocks").map(Vec::as_slice).unwrap_or(&[]) {
        let Some(b) = b.as_obj() else { continue };
        let flags = b.num("flags").unwrap_or(0.0);
        if !flags.is_finite() || (flags as i64) & 4 == 0 {
            continue;
        }
        let Some(name) = b.str("name") else { continue };
        let want = format!("{name}.dwg").to_lowercase();
        if let Some(cand) = names.iter().find(|n| n.to_lowercase() == want)
            && let Ok(bytes) = std::fs::read(dir.join(cand))
        {
            out.push((name.to_lowercase(), read_dwg(&bytes, &ReadOpts::default())));
        }
    }
    out
}

fn compile(rest: &[&str]) -> ExitCode {
    let json = rest.contains(&"--json");
    let bench_n = rest.iter().position(|a| *a == "--bench").and_then(|i| rest.get(i + 1)).and_then(|s| s.parse::<usize>().ok());
    let Some(file) = rest.iter().copied().find(|a| !a.starts_with('-') && a.parse::<usize>().is_err()) else { return usage() };
    let bytes = match read_file(file) {
        Ok(b) => b,
        Err(c) => return c,
    };
    let model = match read_model(file, &bytes) {
        Ok(m) => m,
        Err(e) => {
            print_error("refused", &e);
            return ExitCode::from(1);
        }
    };
    let is_dxf = file.to_ascii_lowercase().ends_with(".dxf");
    // The version tag: the file's first six bytes as text (BimCraft: TextDecoder over data.subarray(0, 6)); none for DXF.
    let tag = if is_dxf { String::new() } else { String::from_utf8_lossy(bytes.get(..6).unwrap_or(&bytes)).into_owned() };
    let xrefs = find_xrefs(file, &model);
    if let Some(n) = bench_n {
        let _ = compile_drawing(&model, &tag, &xrefs); // warm-up
        let mut best = f64::INFINITY;
        for _ in 0..n.max(1) {
            let t = Instant::now();
            let d = compile_drawing(&model, &tag, &xrefs);
            best = best.min(t.elapsed().as_secs_f64() * 1000.0);
            drop(d);
        }
        println!("compile_drawing best of {} = {best:.2} ms", n.max(1));
        return ExitCode::SUCCESS;
    }
    let d = compile_drawing(&model, &tag, &xrefs);
    if json {
        println!("{}", to_json(&d));
    } else {
        println!("{file}: {} {} units {:?}", d.version, d.release, d.units);
        for s in &d.spaces {
            let (l, f, t) = s.batches.iter().fold((0, 0, 0), |a, b| (a.0 + b.lines.len(), a.1 + b.fills.len(), a.2 + b.texts.len()));
            println!("  {}: {} batches, {l} lines, {f} fills, {t} texts, {} viewports", s.name, s.batches.len(), s.viewports.len());
        }
        println!(
            "  layers {}, entities {}, prims {}, unsupported {:?}",
            d.layers.len(),
            d.stats.entities,
            d.stats.prims,
            d.stats.unsupported
        );
    }
    ExitCode::SUCCESS
}
