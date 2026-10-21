//! `cadcraft-cli markup`: MarkupCraft's engine (cc-markup, cc-io-markup) — the parity harness's side of
//! forks/tools/mkdiff.mjs.
//!
//!   cadcraft-cli markup <script.json>                       store / module op script → one record per step (JSON)
//!   cadcraft-cli markup read <file.pdf> [--file-id f1] [--name x.pdf] [--no-text] [--now ms] [--count n]
//!                                                           openPdf → the file record (JSON)
//!   cadcraft-cli markup write <in.pdf> <opts.json> <out.pdf>  writeMarkedUp → out.pdf; prints the report
//!   cadcraft-cli markup bcf <topics.json> <out.bcfzip>       buildBcf → the zip; prints {count, bytes}
//!   cadcraft-cli markup text <file.pdf>                     getTextContent items per page (JSON)
//!   cadcraft-cli markup bench <open|write|shapes|bcf> <args…> [--runs N]   ms per run (best of N)
//!   cadcraft-cli markup pages | blank | summary | segments | find …   see markup2.rs

use cc_io_markup::read::{Bookmark, OpenOpts, open_pdf, record_json};
use cc_io_markup::write::{Flatten, LayerIn, WriteOpts, WritePage, write_marked_up};
use cc_markup::model::Env;
use cc_markup::shapes::LegendRow;
use serde_json::Value;
use std::process::ExitCode;
use std::time::Instant;

fn read_bytes(path: &str) -> Result<Vec<u8>, ExitCode> {
    std::fs::read(path).map_err(|e| {
        crate::print_error("io", &e.to_string());
        ExitCode::from(1)
    })
}

fn read_json(path: &str) -> Result<Value, ExitCode> {
    let bytes = read_bytes(path)?;
    serde_json::from_slice(&bytes).map_err(|e| {
        crate::print_error("json", &e.to_string());
        ExitCode::from(1)
    })
}

fn value_of<'a>(rest: &[&'a str], k: &str) -> Option<&'a str> {
    rest.iter().position(|a| *a == k).and_then(|i| rest.get(i + 1)).copied()
}

fn usage() -> ExitCode {
    eprintln!(
        "usage: cadcraft-cli markup <script.json> | read <pdf> | write <pdf> <opts.json> <out.pdf> | bcf <topics.json> <out> | text <pdf> | bench …"
    );
    ExitCode::from(2)
}

pub fn markup(rest: &[&str]) -> ExitCode {
    match rest.first().copied() {
        Some("read") => read(rest.get(1..).unwrap_or(&[])),
        Some("write") => write(rest.get(1..).unwrap_or(&[])),
        Some("bcf") => bcf(rest.get(1..).unwrap_or(&[])),
        Some("text") => text(rest.get(1..).unwrap_or(&[])),
        Some("bench") => bench(rest.get(1..).unwrap_or(&[])),
        Some("pages") => crate::markup2::pages(rest.get(1..).unwrap_or(&[])),
        Some("blank") => crate::markup2::blank(rest.get(1..).unwrap_or(&[])),
        Some("summary") => crate::markup2::summary(rest.get(1..).unwrap_or(&[])),
        Some("segments") => crate::markup2::segments(rest.get(1..).unwrap_or(&[])),
        Some("find") => crate::markup2::find(rest.get(1..).unwrap_or(&[])),
        Some(file) => {
            let v = match read_json(file) {
                Ok(v) => v,
                Err(c) => return c,
            };
            println!("{}", cc_markup::script::run_in(&v, cc_clock::Zone::System));
            ExitCode::SUCCESS
        }
        None => usage(),
    }
}

fn env_from(rest: &[&str]) -> Env {
    let mut env = Env { zone: cc_clock::Zone::System, ..Env::default() };
    if let Some(n) = value_of(rest, "--now").and_then(|s| s.parse::<f64>().ok()) {
        env.now_ms = n;
    }
    if let Some(c) = value_of(rest, "--count").and_then(|s| s.parse::<u64>().ok()) {
        env.count = c;
    }
    env
}

fn read(rest: &[&str]) -> ExitCode {
    let Some(file) = rest.first() else { return usage() };
    let bytes = match read_bytes(file) {
        Ok(b) => b,
        Err(c) => return c,
    };
    let opts = OpenOpts {
        file_id: value_of(rest, "--file-id").unwrap_or("f1").to_string(),
        name: value_of(rest, "--name").unwrap_or("x.pdf").to_string(),
        read_text: !rest.contains(&"--no-text"),
    };
    let mut env = env_from(rest);
    match open_pdf(&mut env, &bytes, &opts) {
        Ok(rec) => {
            println!("{}", record_json(&rec));
            ExitCode::SUCCESS
        }
        Err(e) => {
            crate::print_error("refused", &e);
            ExitCode::from(1)
        }
    }
}

fn bookmarks_of(v: &Value) -> Vec<Bookmark> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|b| Bookmark {
                    title: b.get("title").and_then(Value::as_str).unwrap_or("").to_string(),
                    page: b.get("page").and_then(Value::as_f64).unwrap_or(-1.0),
                    children: b.get("children").map(bookmarks_of).unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The `writeMarkedUp` options from JSON (the harness writes the same object the JS gets, with legendOf / linkPage as
/// tables).
pub fn write_opts(v: &Value) -> WriteOpts {
    let pages = v
        .get("pages")
        .and_then(Value::as_array)
        .map(|ps| {
            ps.iter()
                .map(|p| WritePage {
                    index: p.get("index").and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).unwrap_or(usize::MAX),
                    markups: p
                        .get("markups")
                        .and_then(Value::as_array)
                        .map(|a| a.iter().map(cc_markup::json::markup_from_value).collect())
                        .unwrap_or_default(),
                    cal: p.get("cal").filter(|c| c.is_object()).and_then(cc_markup::json::cal_of),
                    cal_json: p.get("cal").filter(|c| c.is_object()).cloned(),
                    badge: p.get("badge").and_then(Value::as_f64).unwrap_or(0.0),
                    sheet_name: p.get("sheetName").cloned(),
                    cal_cleared: p.get("calCleared").is_some_and(|x| cc_markup::js::truthy(Some(x))),
                    legends: p
                        .get("legends")
                        .and_then(Value::as_object)
                        .map(|o| {
                            o.iter()
                                .map(|(k, rows)| {
                                    let rows = rows
                                        .as_array()
                                        .map(|a| {
                                            a.iter()
                                                .map(|r| LegendRow {
                                                    color: r.get("color").and_then(Value::as_str).unwrap_or("").to_string(),
                                                    label: r.get("label").and_then(Value::as_str).unwrap_or("").to_string(),
                                                    n: r.get("n").and_then(Value::as_f64).unwrap_or(0.0),
                                                })
                                                .collect()
                                        })
                                        .unwrap_or_default();
                                    (k.clone(), rows)
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default();
    let flatten = match v.get("flatten") {
        Some(Value::String(s)) if s == "all" => Flatten::All,
        Some(Value::Array(a)) => Flatten::Some(a.iter().filter_map(Value::as_str).map(str::to_string).collect()),
        _ => Flatten::None,
    };
    WriteOpts {
        pages,
        remove_refs: v
            .get("removeRefs")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default(),
        labels: v.get("labels").and_then(Value::as_array).cloned(),
        flatten,
        layers: v
            .get("layers")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|l| LayerIn {
                        name: l.get("name").cloned().unwrap_or(Value::Null),
                        hidden: cc_markup::js::truthy(l.get("hidden")),
                        locked: cc_markup::js::truthy(l.get("locked")),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        link_pages: v
            .get("linkPages")
            .and_then(Value::as_object)
            .map(|o| o.iter().filter_map(|(k, x)| x.as_f64().map(|n| (k.clone(), n))).collect())
            .unwrap_or_default(),
        columns: v.get("columns").cloned(),
        bookmarks: v.get("bookmarks").filter(|b| b.is_array()).map(bookmarks_of),
        now_ms: v.get("now").and_then(Value::as_f64).unwrap_or(0.0),
    }
}

fn write(rest: &[&str]) -> ExitCode {
    let (Some(input), Some(optf), Some(out)) = (rest.first(), rest.get(1), rest.get(2)) else { return usage() };
    let bytes = match read_bytes(input) {
        Ok(b) => b,
        Err(c) => return c,
    };
    let v = match read_json(optf) {
        Ok(v) => v,
        Err(c) => return c,
    };
    match write_marked_up(&bytes, &write_opts(&v)) {
        Ok((pdf, r)) => {
            if let Err(e) = std::fs::write(out, &pdf) {
                crate::print_error("io", &e.to_string());
                return ExitCode::from(1);
            }
            println!(
                r#"{{"written":{},"flattened":{},"missing":{},"pages":{},"replies":{},"photos":{}}}"#,
                r.written, r.flattened, r.missing, r.pages, r.replies, r.photos
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            crate::print_error("refused", &e);
            ExitCode::from(1)
        }
    }
}

fn bcf(rest: &[&str]) -> ExitCode {
    let (Some(input), Some(out)) = (rest.first(), rest.get(1)) else { return usage() };
    let mut script = match read_json(input) {
        Ok(v) => v,
        Err(c) => return c,
    };
    if let Some(o) = script.as_object_mut() {
        o.insert("op".into(), Value::from("bcf"));
    }
    let wrapped = serde_json::json!({ "env": { "now": script.get("now").cloned().unwrap_or(Value::from(0)) }, "steps": [script] });
    let recs: Value = serde_json::from_str(&cc_markup::script::run_in(&wrapped, cc_clock::Zone::System)).unwrap_or(Value::Null);
    let r = recs.as_array().and_then(|a| a.first()).and_then(|x| x.get("r")).cloned().unwrap_or(Value::Null);
    let b64 = r.get("bytes").and_then(|b| b.get("$base64")).and_then(Value::as_str).unwrap_or("");
    let zip = cc_markup::imagestamp::b64_decode(b64);
    if let Err(e) = std::fs::write(out, &zip) {
        crate::print_error("io", &e.to_string());
        return ExitCode::from(1);
    }
    println!(r#"{{"count":{},"bytes":{}}}"#, r.get("count").cloned().unwrap_or(Value::Null), zip.len());
    ExitCode::SUCCESS
}

fn text(rest: &[&str]) -> ExitCode {
    let Some(file) = rest.first() else { return usage() };
    let bytes = match read_bytes(file) {
        Ok(b) => b,
        Err(c) => return c,
    };
    let doc = match cc_io_markup::jsdoc::JsDoc::open(&bytes) {
        Ok(d) => d,
        Err(e) => {
            crate::print_error("refused", &format!("{e:?}"));
            return ExitCode::from(1);
        }
    };
    let pages: Vec<Value> = (0..doc.pages.len())
        .map(|i| {
            Value::Array(
                cc_io_markup::text::text_content(&doc, i)
                    .into_iter()
                    .map(|t| serde_json::json!({ "str": t.str, "transform": t.transform, "width": t.width, "height": t.height, "hasEOL": t.has_eol }))
                    .collect(),
            )
        })
        .collect();
    println!("{}", cc_markup::json::stringify(&Value::Array(pages)));
    ExitCode::SUCCESS
}

fn bench(rest: &[&str]) -> ExitCode {
    let runs: usize = value_of(rest, "--runs").and_then(|s| s.parse().ok()).unwrap_or(5).clamp(1, 1000);
    let time = |f: &mut dyn FnMut()| -> f64 {
        f();
        let mut best = f64::INFINITY;
        for _ in 0..runs {
            let t = Instant::now();
            f();
            best = best.min(t.elapsed().as_secs_f64() * 1000.0);
        }
        best
    };
    let ms = match rest.first().copied() {
        Some("open") => {
            let Some(file) = rest.get(1) else { return usage() };
            let Ok(bytes) = read_bytes(file) else { return ExitCode::from(1) };
            let opts = OpenOpts { file_id: "f1".into(), name: "x.pdf".into(), read_text: true };
            time(&mut || {
                let mut env = Env::default();
                let _ = open_pdf(&mut env, &bytes, &opts);
            })
        }
        Some("write") => {
            let (Some(file), Some(optf)) = (rest.get(1), rest.get(2)) else { return usage() };
            let Ok(bytes) = read_bytes(file) else { return ExitCode::from(1) };
            let Ok(v) = read_json(optf) else { return ExitCode::from(1) };
            let o = write_opts(&v);
            time(&mut || {
                let _ = write_marked_up(&bytes, &o);
            })
        }
        Some("shapes") => {
            let Some(f) = rest.get(1) else { return usage() };
            let Ok(v) = read_json(f) else { return ExitCode::from(1) };
            let ms: Vec<cc_markup::Markup> = v
                .get("markups")
                .and_then(Value::as_array)
                .map(|a| a.iter().map(cc_markup::json::markup_from_value).collect())
                .unwrap_or_default();
            let env = cc_markup::script::shape_env(v.get("env"));
            time(&mut || {
                let mut n = 0usize;
                for m in &ms {
                    n = n.wrapping_add(cc_markup::shapes::shapes(m, &env).len());
                }
                std::hint::black_box(n);
            })
        }
        Some("bcf") => {
            let Some(f) = rest.get(1) else { return usage() };
            let Ok(mut v) = read_json(f) else { return ExitCode::from(1) };
            if let Some(o) = v.as_object_mut() {
                o.insert("op".into(), Value::from("bcf"));
            }
            let wrapped = serde_json::json!({ "env": { "now": 0 }, "steps": [v] });
            time(&mut || {
                std::hint::black_box(cc_markup::script::run_in(&wrapped, cc_clock::Zone::System));
            })
        }
        _ => return usage(),
    };
    println!(r#"{{"ms":{ms}}}"#);
    ExitCode::SUCCESS
}
