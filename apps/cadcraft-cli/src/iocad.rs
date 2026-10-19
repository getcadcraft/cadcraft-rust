//! Dev commands for the drafting app's drawing import / export (cc-io-cad):
//!
//!   cadcraft-cli import <file.dwg|file.dxf> [--json] [--doc]   the doc.load input as JSON (`--doc`: the Doc
//!                                                              snapshot after loading it); `--bench N`: best-of-N
//!                                                              import time (DWG: import_dwg only, after read_dwg)
//!   cadcraft-cli dxf-out <in.dwg|in.dxf> <out.dxf>              import → Doc → DXF R12; prints the save report
//!   cadcraft-cli save <in.dwg|in.dxf> <2000|2004|2007|2010|2013|2018|dxf12|dxf2000> <out> [--created-ms N] [--guid-seq]
//!                    [--bench N]                                import → Doc → saveDwg / writeDxf / writeDxf2000;
//!                                                              prints the save report; --created-ms / --guid-seq fix
//!                                                              the DWG's creation time and GUIDs (as the parity
//!                                                              harness does in the JS); --bench N: best-of-N save time
//!
//! A JS exception the importer reproduces prints `{"$error":{"code":"js","message":…}}` (exit 1).

use std::process::ExitCode;
use std::time::Instant;

use cc_cad::Doc;
use cc_io_dwg::dxf_model::decode_dxf_bytes;
use cc_io_dwg::{ReadOpts, read_dwg};
use serde_json::Value;

fn err(code: &str, message: &str) -> ExitCode {
    println!("{}", serde_json::json!({ "$error": { "code": code, "message": message } }));
    ExitCode::from(1)
}

/// A DWG, DXF or JSON file → the doc.load input (DXF / JSON by extension; the DWG version tag = its first six bytes).
pub fn load_input(path: &str) -> Result<Value, (String, String)> {
    let bytes = std::fs::read(path).map_err(|e| ("io".to_string(), e.to_string()))?;
    if path.to_ascii_lowercase().ends_with(".json") {
        // a doc.load input as JSON (synthetic drawings: crates/io-cad/tests/data)
        return serde_json::from_slice(&bytes).map_err(|e| ("json".to_string(), e.to_string()));
    }
    if path.to_ascii_lowercase().ends_with(".dxf") {
        let text = decode_dxf_bytes(&bytes);
        return cc_io_cad::dxf_read::read_dxf(&text).map_err(|e| ("js".to_string(), e.0));
    }
    let tag = String::from_utf8_lossy(bytes.get(..6).unwrap_or(&bytes)).into_owned();
    let model = read_dwg(&bytes, &ReadOpts::default());
    cc_io_cad::import_dwg(&model, &tag).map_err(|e| ("js".to_string(), e.0))
}

pub fn import(rest: &[&str]) -> ExitCode {
    let doc = rest.contains(&"--doc");
    let bench = rest.iter().position(|a| *a == "--bench").and_then(|i| rest.get(i + 1)).and_then(|s| s.parse::<usize>().ok());
    let Some(file) = rest.iter().copied().find(|a| !a.starts_with('-') && a.parse::<usize>().is_err()) else {
        eprintln!("usage: cadcraft-cli import <file.dwg|file.dxf> [--json] [--doc] [--bench N]");
        return ExitCode::from(2);
    };
    if let Some(n) = bench {
        return bench_import(file, n);
    }
    let input = match load_input(file) {
        Ok(v) => v,
        Err((c, m)) => return err(&c, &m),
    };
    if doc {
        let mut d = Doc::new();
        d.load(&input);
        println!("{}", d.snapshot());
    } else {
        println!("{input}");
    }
    ExitCode::SUCCESS
}

fn bench_import(file: &str, n: usize) -> ExitCode {
    let bytes = match std::fs::read(file) {
        Ok(b) => b,
        Err(e) => return err("io", &e.to_string()),
    };
    let dxf = file.to_ascii_lowercase().ends_with(".dxf");
    let text = if dxf { decode_dxf_bytes(&bytes) } else { String::new() };
    let tag = String::from_utf8_lossy(bytes.get(..6).unwrap_or(&bytes)).into_owned();
    let model = if dxf { cc_io_dwg::Obj::new() } else { read_dwg(&bytes, &ReadOpts::default()) };
    let run = || {
        if dxf {
            let _ = cc_io_cad::dxf_read::read_dxf(&text);
        } else {
            let _ = cc_io_cad::import_dwg(&model, &tag);
        }
    };
    run();
    let mut best = f64::INFINITY;
    for _ in 0..n.max(1) {
        let t = Instant::now();
        run();
        best = best.min(t.elapsed().as_secs_f64() * 1000.0);
    }
    println!("{} best of {} = {best:.2} ms", if dxf { "read_dxf" } else { "import_dwg" }, n.max(1));
    ExitCode::SUCCESS
}

pub fn dxf_out(rest: &[&str]) -> ExitCode {
    let (Some(src), Some(out)) = (rest.first(), rest.get(1)) else {
        eprintln!("usage: cadcraft-cli dxf-out <in.dwg|in.dxf> <out.dxf>");
        return ExitCode::from(2);
    };
    let input = match load_input(src) {
        Ok(v) => v,
        Err((c, m)) => return err(&c, &m),
    };
    let mut d = Doc::new();
    d.load(&input);
    let (text, report) = match cc_io_cad::dxf_write::write_dxf(&d) {
        Ok(r) => r,
        Err(e) => return err("js", &e.0),
    };
    if let Err(e) = std::fs::write(out, text.as_bytes()) {
        return err("io", &e.to_string());
    }
    println!("{report}");
    ExitCode::SUCCESS
}

/// One save of a loaded drawing in `fmt`: (bytes, report).
fn save_as(d: &Doc, fmt: &str, opts: &cc_io_cad::dwg_save::SaveOpts) -> Result<(Vec<u8>, Value), String> {
    if let Some(v) = fmt.strip_prefix("model:") {
        // dev: the cc-dwg model saveDwg hands the writer, as JSON
        let o = cc_io_cad::dwg_save::SaveOpts { version: v.to_string(), ..opts.clone() };
        return cc_io_cad::dwg_save::build_model(d, &o, cc_io_cad::i18n::Catalog::en())
            .map(|b| (cc_io_cad::jsv::into_json(cc_io_dwg::V::Obj(b.model)).to_string().into_bytes(), Value::Null))
            .map_err(|e| e.0);
    }
    match fmt.to_ascii_lowercase().as_str() {
        "dxf12" | "dxf" => cc_io_cad::dxf_write::write_dxf(d).map(|(t, r)| (t.into_bytes(), r)).map_err(|e| e.0),
        "dxf2000" => cc_io_cad::dxf2000::write_dxf2000(d).map(|(t, r)| (t.into_bytes(), r)).map_err(|e| e.0),
        _ => cc_io_cad::dwg_save::save_dwg(d, opts).map(|s| (s.bytes, s.report)).map_err(|e| e.0),
    }
}

pub fn save(rest: &[&str]) -> ExitCode {
    let pos: Vec<&str> = {
        let mut out = Vec::new();
        let mut skip = false;
        for a in rest {
            if skip {
                skip = false;
            } else if matches!(*a, "--created-ms" | "--bench" | "--script") {
                skip = true;
            } else if !a.starts_with("--") {
                out.push(*a);
            }
        }
        out
    };
    let flag = |k: &str| rest.iter().position(|a| *a == k).and_then(|i| rest.get(i + 1)).copied();
    let (Some(src), Some(fmt), Some(out)) = (pos.first(), pos.get(1), pos.get(2)) else {
        eprintln!(
            "usage: cadcraft-cli save <in.dwg|in.dxf> <2000|2004|2007|2010|2013|2018|dxf12|dxf2000> <out> [--created-ms N] [--guid-seq] [--bench N] [--script cmds.json]"
        );
        return ExitCode::from(2);
    };
    let is_dxf = matches!(fmt.to_ascii_lowercase().as_str(), "dxf12" | "dxf" | "dxf2000");
    if !is_dxf && cc_io_cad::dwg_save::dwg_version_key(fmt.strip_prefix("model:").unwrap_or(fmt)).is_none() {
        eprintln!("unknown version {fmt}: 2000, 2004, 2007, 2010, 2013, 2018 (or AC10xx / R20xx), dxf12, dxf2000");
        return ExitCode::from(2);
    }
    let created_ms = match flag("--created-ms").map(str::parse::<f64>) {
        Some(Ok(v)) if v.is_finite() => Some(v),
        Some(_) => {
            eprintln!("--created-ms wants a number");
            return ExitCode::from(2);
        }
        None => None,
    };
    let opts = cc_io_cad::dwg_save::SaveOpts {
        version: (*fmt).to_string(),
        created_ms,
        guid_sequence: rest.contains(&"--guid-seq"),
        ..cc_io_cad::dwg_save::SaveOpts::default()
    };
    let input = match load_input(src) {
        Ok(v) => v,
        Err((c, m)) => return err(&c, &m),
    };
    let mut d = Doc::new();
    d.load(&input);
    // --script: a cmds script's inputs run on the drawing first (idiff.mjs --sheets)
    if let Some(sf) = flag("--script") {
        let script: Value =
            match std::fs::read(sf).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string())) {
                Ok(v) => v,
                Err(e) => return err("io", &e),
            };
        d = crate::cmds::run_inputs(d, &script).1;
    }
    if let Some(n) = flag("--bench").and_then(|s| s.parse::<usize>().ok()) {
        let _ = save_as(&d, fmt, &opts);
        let mut best = f64::INFINITY;
        for _ in 0..n.max(1) {
            let t = Instant::now();
            let _ = save_as(&d, fmt, &opts);
            best = best.min(t.elapsed().as_secs_f64() * 1000.0);
        }
        eprintln!("save {fmt} best of {} = {best:.2} ms", n.max(1));
    }
    let (bytes, report) = match save_as(&d, fmt, &opts) {
        Ok(r) => r,
        Err(e) => return err("js", &e),
    };
    if let Err(e) = std::fs::write(out, &bytes) {
        return err("io", &e.to_string());
    }
    println!("{report}");
    ExitCode::SUCCESS
}

/// `cadcraft-cli plot <in.dwg|in.dxf|in.json> <out.pdf> [--opts JSON] [--date-ms N] [--no-deflate] [--bench N]`: agents.js
/// `plotPdf` (plot.js) — opts `{ layout, window: {minX, minY, maxX, maxY}, paper, portrait, scale, mono, pen }`; prints
/// `{ bytes, sheet: { w, h, scale, fits }, missing, texts }` (or `{ "$error": … }`). `--no-deflate`: plain streams (the
/// form the parity harness compares byte for byte with the JS's).
pub fn plot(rest: &[&str]) -> ExitCode {
    let arg = |k: &str| rest.iter().position(|a| *a == k).and_then(|i| rest.get(i + 1)).copied();
    let files: Vec<&str> = {
        let mut v = Vec::new();
        let mut skip = false;
        for a in rest {
            if skip {
                skip = false;
                continue;
            }
            if matches!(*a, "--opts" | "--date-ms" | "--bench") {
                skip = true;
                continue;
            }
            if !a.starts_with("--") {
                v.push(*a);
            }
        }
        v
    };
    let (Some(input), Some(out)) = (files.first(), files.get(1)) else {
        eprintln!("usage: cadcraft-cli plot <in> <out.pdf> [--opts JSON] [--date-ms N] [--no-deflate] [--bench N]");
        return ExitCode::from(2);
    };
    let o: Value = match arg("--opts").map(serde_json::from_str::<Value>) {
        Some(Ok(v)) => v,
        Some(Err(e)) => return err("json", &e.to_string()),
        None => Value::Null,
    };
    let doc_in = match load_input(input) {
        Ok(v) => v,
        Err((c, m)) => return err(&c, &m),
    };
    let mut d = Doc::new();
    d.load(&doc_in);
    let s = |k: &str| o.get(k).and_then(Value::as_str).map(str::to_string);
    let window = o.get("window").filter(|w| w.is_object()).map(|w| {
        let g = |k: &str| w.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
        cc_cad_geom::geom::Bounds { min_x: g("minX"), min_y: g("minY"), max_x: g("maxX"), max_y: g("maxY") }
    });
    let opts = cc_io_cad::plot::PlotOpts {
        layout: s("layout").filter(|l| !l.is_empty()),
        window,
        paper: s("paper").unwrap_or_else(|| "iso-a3".into()),
        portrait: o.get("portrait").and_then(Value::as_bool).unwrap_or(false),
        scale: o.get("scale").and_then(|v| v.as_str().map(str::to_string).or_else(|| v.as_f64().map(|n| n.to_string()))),
        mono: o.get("mono").and_then(Value::as_bool) != Some(false),
        pen: o.get("pen").and_then(Value::as_f64).unwrap_or(0.25),
        date_ms: arg("--date-ms").and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0),
        deflate: !rest.contains(&"--no-deflate"),
    };
    if let Some(n) = arg("--bench").and_then(|v| v.parse::<usize>().ok()) {
        let mut best = f64::INFINITY;
        for _ in 0..n.max(1) {
            let t = Instant::now();
            let _ = cc_io_cad::plot::plot_pdf(&d, &opts);
            best = best.min(t.elapsed().as_secs_f64() * 1000.0);
        }
        println!("{}", serde_json::json!({ "ms": best }));
        return ExitCode::SUCCESS;
    }
    match cc_io_cad::plot::plot_pdf(&d, &opts) {
        Ok(p) => {
            if let Err(e) = std::fs::write(out, &p.pdf.bytes) {
                return err("io", &e.to_string());
            }
            println!(
                "{}",
                serde_json::json!({ "bytes": p.pdf.bytes.len(), "sheet": p.sheet_value(), "missing": p.pdf.missing, "texts": p.pdf.texts })
            );
            ExitCode::SUCCESS
        }
        Err(e) => err("plot", &e),
    }
}
