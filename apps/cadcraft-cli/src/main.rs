//! cadcraft-cli: the headless CadCraft (Rust test version).
//!
//!   cadcraft-cli info <file.dwg>                 version, objects, counts, notes (human text; --json for JSON)
//!   cadcraft-cli convert --json <file.dwg> [-o out.json]   the drawing model as JSON (the JS readDwg shape)
//!   cadcraft-cli parity <engine> <file>          fork-parity contract: engine output as JSON (engines: dwg, skp, ifc-step, ifc-geom)
//!   cadcraft-cli bench <engine> <file> --runs N  fork-parity contract: in-process timings after one warm-up
//!   cadcraft-cli compile <file.dwg|file.dxf> [--json] [--bench N]   dev: the 2D display list (BimCraft's compileDrawing);
//!                                                --bench N: best-of-N compile time after one warm-up
//!   cadcraft-cli parity dxf <file.dxf>           dev: readDxfModel's model as JSON
//!   cadcraft-cli cadgeom <cases.json>            dev: the drafting kernel's parity cases (forks/tools/gdiff.mjs) → JSON results
//!   cadcraft-cli cadgeom --doc <script.json>     dev: a document-ops script, one snapshot per step → JSON
//!   cadcraft-cli write <in.dwg> <AC10xx> <out.dwg> [--created-ms N] [--guid-seq] [--bench N]
//!   cadcraft-cli write --model <model.json> <AC10xx> <out.dwg> [...]
//!   cadcraft-cli dynvis <case.json>              dev: dynamic-block visibility parameter writer on {raw, states}:
//!                                                {exact, fields (as JSON), encoded, rewritten | error}
//!                                                read (our reader, or a model JSON), write with our writer (writeDwg),
//!                                                print the report as JSON; --created-ms / --guid-seq fix the creation
//!                                                time and the GUIDs (the parity harness does the same in the JS);
//!                                                --bench N: best-of-N write time after one warm-up
//!   cadcraft-cli cmds <script.json>              dev: drive the command line with a stub host (forks/tools/kdiff.mjs) → JSON
//!   cadcraft-cli import <file.dwg|file.dxf> [--doc]  dev: the drafting app's doc.load input (or Doc snapshot), iocad.rs
//!   cadcraft-cli dxf-out <in.dwg|in.dxf> <out.dxf>   dev: import → Doc → DXF R12 (writeDxf), iocad.rs
//!   cadcraft-cli manifold <cases.json> [--bench N]   dev: boolean scripts through cc-manifold (forks/tools/mfdiff.mjs) → JSON
//!   cadcraft-cli m3d <script.json> | --cases <cases.json> | --bench <script.json> [--runs N]
//!                                                dev: the 3D model kernel (cc-m3d) on op scripts (forks/tools/mdiff.mjs), m3d.rs
//!   cadcraft-cli m3dtools <script.json> | --bench <bench.json> [--runs N]
//!                                                dev: ModelCraft's tools (cc-m3d-tools) on scripted sessions
//!                                                (forks/tools/tdiff.mjs), m3dtools.rs
//!   cadcraft-cli save <in.dwg|in.dxf> <2000|…|2018|dxf12|dxf2000> <out> [--created-ms N] [--guid-seq]
//!                                                    import → Doc → saveDwg / writeDxf / writeDxf2000, iocad.rs
//!
//! Exit codes: 0 ok, 1 refused file / error, 2 usage, 3 engine not ported yet.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

mod cmds;
mod ifc;
mod iocad;
mod m3d;
mod m3dtools;
mod manifold;
mod skp;

use std::process::ExitCode;
use std::time::Instant;

use cc_io_dwg::compile::{compile_drawing, to_json};
use cc_io_dwg::dxf_model::{decode_dxf_bytes, try_read_dxf_model};
use cc_io_dwg::{GuidMode, Obj, ReadOpts, V, WriteOpts, read_dwg, write_dwg_opts};

fn usage() -> ExitCode {
    eprintln!(
        "usage: cadcraft-cli info [--json] <file.dwg> | convert --json <file.dwg> [-o out.json] | parity <engine> <file> | bench <engine> <file> [--runs N] | compile <file.dwg|file.dxf> [--json] [--bench N] | write (<in.dwg> | --model <model.json>) <AC10xx> <out.dwg> [--created-ms N] [--guid-seq] [--bench N] | dynvis <case.json>"
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
        "cadgeom" => cadgeom(&rest),
        "write" => write(&rest),
        "dynvis" => dynvis(&rest),
        "cmds" => cmds::cmds(&rest),
        "import" => iocad::import(&rest),
        "dxf-out" => iocad::dxf_out(&rest),
        "save" => iocad::save(&rest),
        "manifold" => manifold::manifold(&rest),
        "m3d" => m3d::m3d(&rest),
        "m3dtools" => m3dtools::m3dtools(&rest),
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
    if *engine == "skp" {
        return skp::parity(file);
    }
    if let Some(code) = ifc::parity(engine, file) {
        return code;
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
    let runs: usize = rest.iter().position(|a| *a == "--runs").and_then(|i| rest.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(5);
    if *engine == "skp" {
        return skp::bench(file, runs);
    }
    if let Some(code) = ifc::bench(engine, file, runs) {
        return code;
    }
    if *engine != "dwg" {
        return ExitCode::from(3);
    }
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

fn write(rest: &[&str]) -> ExitCode {
    let opt = |k: &str| rest.iter().position(|a| *a == k).and_then(|i| rest.get(i + 1)).copied();
    let model_json = opt("--model");
    let created = opt("--created-ms").and_then(|s| s.parse::<f64>().ok());
    let bench_n = opt("--bench").and_then(|s| s.parse::<usize>().ok());
    let guid = if rest.contains(&"--guid-seq") { GuidMode::Sequence } else { GuidMode::Random };
    let skip: Vec<&str> = [model_json, opt("--created-ms"), opt("--bench")].into_iter().flatten().collect();
    let pos: Vec<&str> = rest.iter().copied().filter(|a| !a.starts_with("--") && !skip.contains(a)).collect();
    let (input, version, out) = match (model_json, pos.as_slice()) {
        (Some(m), [v, o]) => (m, *v, *o),
        (None, [i, v, o]) => (*i, *v, *o),
        _ => return usage(),
    };
    let bytes = match read_file(input) {
        Ok(b) => b,
        Err(c) => return c,
    };
    let model = if model_json.is_some() {
        match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(v) => match cc_io_dwg::w_js::from_json(&v, 0) {
                V::Obj(o) => o,
                _ => {
                    print_error("refused", "the model JSON is not an object");
                    return ExitCode::from(1);
                }
            },
            Err(e) => {
                print_error("refused", &e.to_string());
                return ExitCode::from(1);
            }
        }
    } else {
        read_dwg(&bytes, &ReadOpts::default())
    };
    let opts = WriteOpts { version: version.to_string(), created_ms: created, guid };
    if let Some(n) = bench_n {
        let _ = write_dwg_opts(&model, &opts); // warm-up
        let mut best = f64::INFINITY;
        for _ in 0..n.max(1) {
            let t = Instant::now();
            let r = write_dwg_opts(&model, &opts);
            best = best.min(t.elapsed().as_secs_f64() * 1000.0);
            drop(r);
        }
        println!("{{\"write_ms\":{best:.3}}}");
        return ExitCode::SUCCESS;
    }
    match write_dwg_opts(&model, &opts) {
        Ok((data, report)) => {
            if let Err(e) = std::fs::write(out, &data) {
                print_error("io", &e.to_string());
                return ExitCode::from(1);
            }
            println!("{}", cc_io_dwg::w_js::js_order(V::Obj(report)).to_json());
            ExitCode::SUCCESS
        }
        Err(e) => {
            print_error("refused", &e);
            ExitCode::from(1)
        }
    }
}

fn dynvis(rest: &[&str]) -> ExitCode {
    use cc_io_dwg::w_dynblock::{encode_visibility, reencodes_exactly, rewrite_visibility, visibility_fields};
    let Some(file) = rest.first() else { return usage() };
    let bytes = match read_file(file) {
        Ok(b) => b,
        Err(c) => return c,
    };
    let case = match serde_json::from_slice::<serde_json::Value>(&bytes) {
        Ok(v) => cc_io_dwg::w_js::from_json(&v, 0),
        Err(e) => {
            print_error("refused", &e.to_string());
            return ExitCode::from(1);
        }
    };
    let raw = case.as_obj().and_then(|o| o.obj("raw")).cloned().unwrap_or_default();
    let mut out = Obj::new();
    out.set("exact", reencodes_exactly(&raw));
    match visibility_fields(&raw) {
        Ok(f) => {
            let st: Vec<V> = f
                .states
                .iter()
                .map(|s| {
                    let hs = |l: &[(f64, String)]| V::Arr(l.iter().map(|(c, x)| V::Arr(vec![V::Num(*c), V::from(x.as_str())])).collect());
                    let mut o = Obj::new();
                    o.set("name", s.name.as_str());
                    o.set("entities", hs(&s.entities));
                    o.set("elements", hs(&s.elements));
                    V::Obj(o)
                })
                .collect();
            let mut fo = Obj::new();
            fo.set("name", f.name.as_str());
            fo.set("label", f.label.as_str());
            fo.set("all", f.all.len());
            fo.set("states", st);
            out.set("fields", fo);
            if let Ok(e) = encode_visibility(&f, f.ver) {
                out.set("encoded", e);
            }
        }
        Err(e) => out.set("fieldsError", e),
    }
    if let Some(states) = case.as_obj().and_then(|o| o.get("states")) {
        match rewrite_visibility(&raw, states) {
            Ok(r) => out.set("rewritten", r),
            Err(e) => out.set("rewriteError", e),
        }
    }
    println!("{}", V::Obj(out).to_json());
    ExitCode::SUCCESS
}

/// Dev: run the drafting kernel's parity cases, or (`--doc`) a document-ops script, and print the results as JSON.
fn cadgeom(rest: &[&str]) -> ExitCode {
    let doc_mode = rest.contains(&"--doc");
    let Some(file) = rest.iter().find(|a| !a.starts_with("--")) else { return usage() };
    let bytes = match read_file(file) {
        Ok(b) => b,
        Err(c) => return c,
    };
    let v: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => {
            print_error("json", &e.to_string());
            return ExitCode::from(1);
        }
    };
    let out = if doc_mode {
        let steps = v.as_array().cloned().unwrap_or_default();
        cc_cad::script::run(&steps)
    } else {
        cc_cad_geom::parity::run_cases(&v)
    };
    println!("{}", serde_json::Value::Array(out));
    ExitCode::SUCCESS
}
