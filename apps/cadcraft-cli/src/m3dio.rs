//! `cadcraft-cli m3dio`: ModelCraft's file formats (cc-io-m3d) — the parity harness's side of forks/tools/iodiff.mjs.
//!
//!   cadcraft-cli m3dio export <model.json> <format> <out> [--name N] [--date ISO] [--mtl file.mtl]
//!                [--version AC1018 --created-ms N --guid-seq]
//!       formats: glb obj mtl stl dae ifc dxf dwg dwgmodel modelcraft; prints {"report": …} (dxf, dwg) or {}
//!   cadcraft-cli m3dio import <file> [--format obj|gltf|glb|dae|dxf|dwg|dwgjson|skp|skpjson] [--unit u] [--name N]
//!                [--into model.json]
//!       the app's import: into a new (or the given) model, placed at the root in one change; prints
//!       {"model": toJSON + images, "report": …, "units": …} or {"error": message}
//!   cadcraft-cli m3dio solids <script.json>       {init, steps: [{op: solid, path, kind, keep, cut} | {op: undo|redo}]}
//!                                                  → one record per step: {op, ret | err, json}
//!   cadcraft-cli m3dio print3d <model.json> [--path ids] [--sel json] [--name N] [--stl out] [--3mf out]
//!   cadcraft-cli m3dio print3d --tris <tris.json> [--stl out] [--3mf out]   {tris, opts?, weld?, header?, name?, app?}
//!   cadcraft-cli m3dio bench export <model.json> <format> | bench import <file> [--format f] [--unit u]
//!                | bench solids <script.json> | bench print3d <model.json>   [--runs N]   → {"best", "median", "runs"}

use std::process::ExitCode;
use std::time::Instant;

use cc_io_m3d::print3d::{CheckOpts, Mesh, Tri, mesh_from_triangles, mesh_value, print_check, stl_binary, three_mf, weld};
use cc_io_m3d::{Error, native};
use cc_m3d::json::Out;
use cc_m3d::{Model, Sel};
use cc_m3d_files::{decode_input, run_import};
use serde_json::{Value, json};

fn print_err(code: &str, m: &str) -> ExitCode {
    crate::print_error(code, m);
    ExitCode::from(1)
}

fn opt<'a>(rest: &[&'a str], k: &str) -> Option<&'a str> {
    rest.iter().position(|a| *a == k).and_then(|i| rest.get(i + 1)).copied()
}

/// Positional arguments (flags and their values left out).
fn positional<'a>(rest: &[&'a str]) -> Vec<&'a str> {
    const WITH_VALUE: [&str; 14] = [
        "--name",
        "--date",
        "--mtl",
        "--version",
        "--created-ms",
        "--format",
        "--unit",
        "--into",
        "--path",
        "--sel",
        "--stl",
        "--3mf",
        "--runs",
        "--tris",
    ];
    let mut out = Vec::new();
    let mut skip = false;
    for a in rest {
        if skip {
            skip = false;
            continue;
        }
        if WITH_VALUE.contains(a) {
            skip = true;
            continue;
        }
        if a.starts_with("--") {
            continue;
        }
        out.push(*a);
    }
    out
}

fn read_json(path: &str) -> Result<Value, ExitCode> {
    let bytes = std::fs::read(path).map_err(|e| print_err("io", &e.to_string()))?;
    serde_json::from_slice(&bytes).map_err(|e| print_err("json", &e.to_string()))
}

fn load_model(path: &str) -> Result<Model, ExitCode> {
    let j = read_json(path)?;
    native::load_native(&j).map_err(|e| print_err("model", &e.to_string()))
}

fn err_text(e: &Error) -> String {
    match e {
        Error::Msg(m) => m.clone(),
        Error::Type => "$TypeError".into(),
    }
}

fn print_value(v: &Value) {
    let mut o = Out::new(true);
    o.value(v);
    println!("{}", o.s);
}

// ---------------------------------------------------------------------------------------------------------------- export

/// One export → (bytes, report JSON).
fn export_one(model: &mut Model, format: &str, rest: &[&str]) -> Result<(Vec<u8>, Value), String> {
    let o = cc_m3d_files::ExportOpts {
        name: opt(rest, "--name").map(str::to_string),
        date: opt(rest, "--date").map(str::to_string),
        mtl: opt(rest, "--mtl").map(str::to_string),
        version: opt(rest, "--version").map(str::to_string),
        created_ms: opt(rest, "--created-ms").and_then(|s| s.parse().ok()),
        guid_seq: rest.contains(&"--guid-seq"),
    };
    cc_m3d_files::export(model, format, &o)
}

fn export(rest: &[&str]) -> ExitCode {
    let pos = positional(rest);
    let (Some(file), Some(format), Some(out)) = (pos.first(), pos.get(1), pos.get(2)) else {
        eprintln!("usage: cadcraft-cli m3dio export <model.json> <format> <out>");
        return ExitCode::from(2);
    };
    let mut model = match load_model(file) {
        Ok(m) => m,
        Err(c) => return c,
    };
    match export_one(&mut model, format, rest) {
        Ok((bytes, rep)) => {
            if let Err(e) = std::fs::write(out, bytes) {
                return print_err("io", &e.to_string());
            }
            print_value(&rep);
            ExitCode::SUCCESS
        }
        Err(e) => {
            print_value(&json!({ "error": e }));
            ExitCode::SUCCESS
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------- import

/// cc-io-skp's model → cc-io-m3d's copy of its shape.
/// The file's kind from `--format` or its extension.
fn format_of(file: &str, rest: &[&str]) -> String {
    if let Some(f) = opt(rest, "--format") {
        return f.to_ascii_lowercase();
    }
    std::path::Path::new(file).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

/// The app's name for an import (or `--name`).
fn import_name(file: &str, rest: &[&str]) -> String {
    opt(rest, "--name").map_or_else(|| cc_m3d_files::import_name(file), str::to_string)
}

fn import(rest: &[&str]) -> ExitCode {
    let pos = positional(rest);
    let Some(file) = pos.first() else {
        eprintln!("usage: cadcraft-cli m3dio import <file> [--format f] [--unit u]");
        return ExitCode::from(2);
    };
    let format = format_of(file, rest);
    let bytes = match std::fs::read(file) {
        Ok(b) => b,
        Err(e) => return print_err("io", &e.to_string()),
    };
    let name = import_name(file, rest);
    let mut model = match opt(rest, "--into") {
        Some(p) => match load_model(p) {
            Ok(m) => m,
            Err(c) => return c,
        },
        None => Model::new(),
    };
    let input = match decode_input(&format, bytes) {
        Ok(i) => i,
        Err(e) => {
            print_value(&json!({ "error": e }));
            return ExitCode::SUCCESS;
        }
    };
    match run_import(&mut model, &format, &input, &name, opt(rest, "--unit")) {
        Ok(x) => {
            let model_json = native::model_canon(&mut model);
            let mut o = Out::new(true);
            o.raw("{\"model\":");
            o.raw(&model_json);
            o.raw(",\"def\":");
            o.str(&x.def);
            o.raw(",\"report\":");
            o.value(&Value::Object(x.report));
            o.raw(",\"units\":");
            match &x.units {
                Some(u) => o.str(u),
                None => o.raw("null"),
            }
            o.ch('}');
            println!("{}", o.s);
        }
        Err(e) => print_value(&json!({ "error": err_text(&e) })),
    }
    ExitCode::SUCCESS
}

// ---------------------------------------------------------------------------------------------------------------- solids

fn ids(v: Option<&Value>) -> Vec<f64> {
    v.and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default()
}

fn run_solids(script: &Value, records: bool) -> Result<Vec<String>, ExitCode> {
    let mut model = match script.get("init") {
        Some(j) => native::load_native(j).map_err(|e| print_err("model", &e.to_string()))?,
        None => Model::new(),
    };
    let mut out = Vec::new();
    for st in script.get("steps").and_then(Value::as_array).map_or(&[][..], Vec::as_slice) {
        let op = st.get("op").and_then(Value::as_str).unwrap_or("");
        let r: Result<Value, Error> = match op {
            "solid" => cc_io_m3d::solids::solid_op(
                &mut model,
                &ids(st.get("path")),
                st.get("kind").and_then(Value::as_str).unwrap_or(""),
                &ids(st.get("keep")),
                &ids(st.get("cut")),
            )
            .map(|r| json!({ "iid": r.iid, "faces": r.faces, "volume": r.volume, "closed": r.closed })),
            "isSolid" => {
                let ctx = model.context(&ids(st.get("path"))).map(|c| c.def);
                Ok(Value::Bool(
                    ctx.is_some_and(|d| cc_io_m3d::solids::is_solid(&model, &d, st.get("iid").and_then(Value::as_f64).unwrap_or(f64::NAN))),
                ))
            }
            "undo" => model.undo().map(|l| l.map_or(Value::Null, Value::String)),
            "redo" => model.redo().map(|l| l.map_or(Value::Null, Value::String)),
            _ => Err(Error::Msg(format!("unknown op {op}"))),
        };
        if records {
            let mut o = Out::new(true);
            o.raw("{\"op\":");
            o.str(op);
            match &r {
                Ok(v) => {
                    o.raw(",\"ret\":");
                    o.value(v);
                }
                Err(e) => {
                    o.raw(",\"err\":");
                    o.str(&err_text(e));
                }
            }
            o.raw(",\"undo\":");
            o.num(model.undo_stack.len() as f64);
            o.raw(",\"json\":");
            o.raw(&native::model_canon(&mut model));
            o.ch('}');
            out.push(o.s);
        }
    }
    Ok(out)
}

fn solids(rest: &[&str]) -> ExitCode {
    let pos = positional(rest);
    let Some(file) = pos.first() else { return ExitCode::from(2) };
    let script = match read_json(file) {
        Ok(v) => v,
        Err(c) => return c,
    };
    match run_solids(&script, true) {
        Ok(recs) => {
            println!("[{}]", recs.join(","));
            ExitCode::SUCCESS
        }
        Err(c) => c,
    }
}

// ---------------------------------------------------------------------------------------------------------------- print3d

fn tris_of(v: &Value) -> Vec<Tri> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|t| {
                    let p = |i: usize| -> [f64; 3] {
                        let q = t.get("p").and_then(Value::as_array).and_then(|a| a.get(i));
                        let g = |k: usize| q.and_then(Value::as_array).and_then(|a| a.get(k)).and_then(Value::as_f64).unwrap_or(f64::NAN);
                        [g(0), g(1), g(2)]
                    };
                    Tri { p: [p(0), p(1), p(2)], color: t.get("color").and_then(Value::as_str).map(str::to_string) }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn check_opts(v: Option<&Value>) -> CheckOpts {
    let mut o = CheckOpts::default();
    if let Some(v) = v {
        if let Some(x) = v.get("tol").and_then(Value::as_f64) {
            o.tol = x;
        }
        if let Some(x) = v.get("overhang").and_then(Value::as_f64) {
            o.overhang = x;
        }
        if let Some(x) = v.get("maxTests").and_then(Value::as_f64) {
            o.max_tests = x;
        }
        if let Some(b) = v.get("bed").and_then(Value::as_array).filter(|b| b.len() == 3) {
            let g = |i: usize| b.get(i).and_then(Value::as_f64).unwrap_or(f64::NAN);
            o.bed = Some([g(0), g(1), g(2)]);
        }
    }
    o
}

fn print3d(rest: &[&str]) -> ExitCode {
    let (tris, what, opts, name, header, app, weld_only) = if let Some(tf) = opt(rest, "--tris") {
        let j = match read_json(tf) {
            Ok(v) => v,
            Err(c) => return c,
        };
        let tris = tris_of(j.get("tris").unwrap_or(&Value::Null));
        let name = j.get("name").and_then(Value::as_str).unwrap_or("model").to_string();
        let header = j.get("header").and_then(Value::as_str).unwrap_or("binary STL, millimeters").to_string();
        let app = j.get("app").and_then(Value::as_str).unwrap_or("CadCraft").to_string();
        (tris, "tris", check_opts(j.get("opts")), name, header, app, j.get("weld").and_then(Value::as_bool).unwrap_or(false))
    } else {
        let pos = positional(rest);
        let Some(file) = pos.first() else { return ExitCode::from(2) };
        let model = match load_model(file) {
            Ok(m) => m,
            Err(c) => return c,
        };
        let path: Vec<f64> = opt(rest, "--path").map(|p| p.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or_default();
        let sel: Vec<Sel> = opt(rest, "--sel")
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
            .and_then(|v| v.as_array().cloned())
            .map(|a| {
                a.iter()
                    .map(|s| {
                        Sel::new(s.get("t").and_then(Value::as_str).unwrap_or(""), s.get("id").and_then(Value::as_f64).unwrap_or(f64::NAN))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let (tris, what) = cc_io_m3d::printmc::print_triangles(&model, &path, &sel);
        let name = opt(rest, "--name").unwrap_or("model").to_string();
        (tris, what, CheckOpts::default(), name.clone(), format!("ModelCraft {name}"), "ModelCraft".to_string(), false)
    };
    let tri_json: Vec<Value> = tris
        .iter()
        .map(|t| json!({ "p": t.p.iter().map(|p| p.iter().map(|&x| cc_m3d::file::jn(x)).collect::<Vec<_>>()).collect::<Vec<_>>(), "color": t.color }))
        .collect();
    let base = mesh_from_triangles(&tris);
    let (rep_v, mesh): (Value, Mesh) = if weld_only {
        let m = weld(&base, opts.tol);
        (Value::Null, m)
    } else {
        let r = print_check(&base, &opts);
        let mut v = r.value.clone();
        v.insert("mesh".into(), mesh_value(&r.mesh));
        (Value::Object(v), r.mesh)
    };
    if let Some(p) = opt(rest, "--stl")
        && let Err(e) = std::fs::write(p, stl_binary(&mesh, &header))
    {
        return print_err("io", &e.to_string());
    }
    if let Some(p) = opt(rest, "--3mf")
        && let Err(e) = std::fs::write(p, three_mf(&mesh, &name, &app))
    {
        return print_err("io", &e.to_string());
    }
    print_value(&json!({ "what": what, "tris": tri_json, "report": rep_v, "mesh": mesh_value(&mesh) }));
    ExitCode::SUCCESS
}

// ---------------------------------------------------------------------------------------------------------------- bench

fn time_runs(runs: usize, mut f: impl FnMut()) -> Value {
    if !NO_WARMUP.load(std::sync::atomic::Ordering::Relaxed) {
        f(); // warm-up
    }
    let mut ms = Vec::with_capacity(runs);
    for _ in 0..runs {
        let t = Instant::now();
        f();
        ms.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let mut sorted = ms.clone();
    sorted.sort_by(f64::total_cmp);
    let best = sorted.first().copied().unwrap_or(f64::NAN);
    let median = sorted.get(sorted.len() / 2).copied().unwrap_or(best);
    json!({ "runs": ms, "best": best, "median": median })
}

/// `bench … --no-warmup`: time the runs only (for cases that take minutes).
static NO_WARMUP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn bench(rest: &[&str]) -> ExitCode {
    NO_WARMUP.store(rest.contains(&"--no-warmup"), std::sync::atomic::Ordering::Relaxed);
    let runs: usize = opt(rest, "--runs").and_then(|s| s.parse().ok()).unwrap_or(5).clamp(1, 1000);
    let pos = positional(rest);
    let kind = pos.first().copied().unwrap_or("");
    let tail: Vec<&str> = rest.iter().skip_while(|a| **a != kind).skip(1).copied().collect();
    let tpos = positional(&tail);
    match kind {
        "export" => {
            let (Some(file), Some(format)) = (tpos.first(), tpos.get(1)) else { return ExitCode::from(2) };
            let model = match load_model(file) {
                Ok(m) => m,
                Err(c) => return c,
            };
            let mut m = model;
            if let Err(e) = export_one(&mut m, format, &tail) {
                return print_err("export", &e);
            }
            let v = time_runs(runs, || {
                // the JS bench times toDwgModel's object, not its JSON text: time the model alone here too
                if *format == "dwgmodel" {
                    drop(cc_io_m3d::cad::to_dwg_model(&m));
                } else {
                    drop(export_one(&mut m, format, &tail));
                }
            });
            print_value(&v);
        }
        "import" => {
            let Some(file) = tpos.first() else { return ExitCode::from(2) };
            let format = format_of(file, &tail);
            let bytes = match std::fs::read(file) {
                Ok(b) => b,
                Err(e) => return print_err("io", &e.to_string()),
            };
            let name = import_name(file, &tail);
            let unit = opt(&tail, "--unit");
            let v = time_runs(runs, || {
                // decoding counts: the JS bench reads the same bytes with its own reader (readDwg, readSkp, TextDecoder)
                if let Ok(input) = decode_input(&format, bytes.clone()) {
                    let mut model = Model::new();
                    let r = run_import(&mut model, &format, &input, &name, unit);
                    drop(r);
                }
            });
            print_value(&v);
        }
        "solids" => {
            let Some(file) = tpos.first() else { return ExitCode::from(2) };
            let script = match read_json(file) {
                Ok(v) => v,
                Err(c) => return c,
            };
            let v = time_runs(runs, || {
                let r = run_solids(&script, false);
                drop(r);
            });
            print_value(&v);
        }
        "print3d" => {
            let Some(file) = tpos.first() else { return ExitCode::from(2) };
            let model = match load_model(file) {
                Ok(m) => m,
                Err(c) => return c,
            };
            let v = time_runs(runs, || {
                let (tris, _) = cc_io_m3d::printmc::print_triangles(&model, &[], &[]);
                let r = print_check(&mesh_from_triangles(&tris), &CheckOpts::default());
                drop(r);
            });
            print_value(&v);
        }
        _ => {
            eprintln!("usage: cadcraft-cli m3dio bench export|import|solids|print3d …");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}

pub fn m3dio(rest: &[&str]) -> ExitCode {
    let sub = rest.first().copied().unwrap_or("");
    let tail: Vec<&str> = rest.iter().skip(1).copied().collect();
    match sub {
        "export" => export(&tail),
        "import" => import(&tail),
        "solids" => solids(&tail),
        "print3d" => print3d(&tail),
        "bench" => bench(&tail),
        _ => {
            eprintln!("usage: cadcraft-cli m3dio export|import|solids|print3d|bench …");
            ExitCode::from(2)
        }
    }
}
