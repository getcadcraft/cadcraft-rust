//! `cadcraft-cli takeoff`: TakeoffCraft's engine (cc-takeoff, cc-io-takeoff) — the parity harness's side of
//! forks/tools/todiff.mjs, the exports, auto count and the marked PDF.
//!
//!   cadcraft-cli takeoff <script.json>                    run {labels?, steps: [{op, …} | {fn, args}]} → {records}
//!   cadcraft-cli takeoff --bench <script.json> [--runs N] ms per run of the script (records built, not printed)
//!   cadcraft-cli takeoff export <project.json> <csv|sheets-csv|material-csv|xlsx|xlsx-stored|sheets|glb> <out>
//!                         [--now MS] [--labels labels.json] [--sheet KEY]
//!   cadcraft-cli takeoff match <image.ink> <template.ink> [--floor F] [--any]   NCC search on ink rasters → candidates
//!   cadcraft-cli takeoff count <file.pdf> --page N --box x0,y0,x1,y1 [--any] [--template-page N] [--scale-by K]
//!                         render with hayro, cut the template, search → {w, cands: [{…, center}]}
//!   cadcraft-cli takeoff marks <file.pdf> <project.json> <hash> <out.pdf> [--no-legend]   the marked PDF copy
//!   cadcraft-cli takeoff proposal <project.json> <out.pdf> [--date ISO]                  the proposal PDF
//!   cadcraft-cli takeoff embed <file.pdf> <project.json> <hash> <out.pdf> [--now MS]       the takeoff inside the PDF
//!   cadcraft-cli takeoff bench <estimate|xlsx|formula|count> <input> [--runs N]          timings (best of N, warm)
//!
//! An `.ink` raster file: "INK1", width and height (u32 LE), then w × h f32 LE (0 paper … 1 ink).

use std::process::ExitCode;
use std::time::Instant;

use serde_json::{Value, json};

use cc_io_takeoff::xlsx::Stamp;
use cc_io_takeoff::{export, script};
use cc_takeoff::jv;
use cc_takeoff::matcher::{self, Img};
use cc_takeoff::model;

fn read_json(path: &str) -> Result<Value, ExitCode> {
    let bytes = std::fs::read(path).map_err(|e| {
        crate::print_error("io", &e.to_string());
        ExitCode::from(1)
    })?;
    let text = String::from_utf8_lossy(&bytes);
    jv::parse_json(&text, 512).map_err(|e| {
        crate::print_error("json", &e);
        ExitCode::from(1)
    })
}

fn write_out(path: &str, bytes: &[u8]) -> ExitCode {
    match std::fs::write(path, bytes) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            crate::print_error("io", &e.to_string());
            ExitCode::from(1)
        }
    }
}

/// Read an `.ink` raster.
pub fn read_ink(path: &str) -> Result<Img, String> {
    let b = std::fs::read(path).map_err(|e| e.to_string())?;
    if b.get(..4) != Some(b"INK1") {
        return Err("not an INK1 raster".into());
    }
    let u = |i: usize| b.get(i..i + 4).and_then(|s| <[u8; 4]>::try_from(s).ok()).map_or(0, u32::from_le_bytes);
    let (w, h) = (u(4) as usize, u(8) as usize);
    let n = w.checked_mul(h).filter(|n| *n <= matcher::MAX_PIXELS).ok_or("raster too large")?;
    let body = b.get(12..12 + n * 4).ok_or("raster truncated")?;
    let data = body.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
    Ok(Img { data, w, h })
}

fn flag<'a>(rest: &[&'a str], k: &str) -> Option<&'a str> {
    rest.iter().position(|a| *a == k).and_then(|i| rest.get(i + 1)).copied()
}

pub fn takeoff(rest: &[&str]) -> ExitCode {
    match rest.first().copied() {
        Some("export") => return export_cmd(rest.get(1..).unwrap_or(&[])),
        Some("match") => return match_cmd(rest.get(1..).unwrap_or(&[])),
        Some("count") => return crate::takeoff_pdf::count_cmd(rest.get(1..).unwrap_or(&[])),
        Some("marks") => return crate::takeoff_pdf::marks_cmd(rest.get(1..).unwrap_or(&[])),
        Some("proposal") => return crate::takeoff_pdf::proposal_cmd(rest.get(1..).unwrap_or(&[])),
        Some("embed") => return crate::takeoff_pdf::embed_cmd(rest.get(1..).unwrap_or(&[])),
        Some("bench") => return bench_cmd(rest.get(1..).unwrap_or(&[])),
        Some("pdfinfo") => return crate::takeoff_pdf::pdfinfo_cmd(rest.get(1..).unwrap_or(&[])),
        Some("snap") => return crate::takeoff_pdf::snap_cmd(rest.get(1..).unwrap_or(&[])),
        _ => {}
    }
    if let Some(file) = flag(rest, "--bench") {
        let v = match read_json(file) {
            Ok(v) => v,
            Err(c) => return c,
        };
        let runs: usize = flag(rest, "--runs").and_then(|s| s.parse().ok()).unwrap_or(5).clamp(1, 1000);
        let _ = script::run_in(&v, cc_clock::Zone::System);
        let mut ms = Vec::with_capacity(runs);
        for _ in 0..runs {
            let t = Instant::now();
            let r = script::run_in(&v, cc_clock::Zone::System);
            ms.push(t.elapsed().as_secs_f64() * 1000.0);
            drop(r);
        }
        println!("{}", json!({ "runs": ms, "best": ms.iter().copied().fold(f64::INFINITY, f64::min) }));
        return ExitCode::SUCCESS;
    }
    let Some(file) = rest.iter().find(|a| !a.starts_with("--")) else {
        eprintln!(
            "usage: cadcraft-cli takeoff <script.json> | --bench <script.json> | export | match | count | marks | proposal | embed | bench"
        );
        return ExitCode::from(2);
    };
    let v = match read_json(file) {
        Ok(v) => v,
        Err(c) => return c,
    };
    println!("{}", jv::stringify(&script::run_in(&v, cc_clock::Zone::System)));
    ExitCode::SUCCESS
}

fn load_state(path: &str) -> Result<Value, ExitCode> {
    let j = read_json(path)?;
    model::read_project(&j).map(|r| r.state).map_err(|e| {
        crate::print_error("project", &e);
        ExitCode::from(1)
    })
}

fn export_cmd(rest: &[&str]) -> ExitCode {
    let (Some(proj), Some(kind), Some(out)) = (rest.first(), rest.get(1), rest.get(2)) else {
        eprintln!(
            "usage: cadcraft-cli takeoff export <project.json> <csv|sheets-csv|material-csv|xlsx|xlsx-stored|sheets|glb|spots> <out> [--now MS] [--labels labels.json] [--sheet KEY]"
        );
        return ExitCode::from(2);
    };
    let state = match load_state(proj) {
        Ok(s) => s,
        Err(c) => return c,
    };
    let labels = match flag(rest, "--labels") {
        Some(p) => match read_json(p) {
            Ok(v) => v,
            Err(c) => return c,
        },
        None => Value::Null,
    };
    let lab = |k: &str| labels.get(k).map_or_else(|| k.to_string(), |v| jv::to_string(Some(v)));
    let now: f64 = flag(rest, "--now").and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let est = model::base_estimate(&state);
    let bytes: Vec<u8> = match *kind {
        "csv" => export::estimate_csv(&state, &est).into_bytes(),
        "sheets-csv" => export::sheets_csv(&state, &lab).into_bytes(),
        "material-csv" => export::material_csv(&state, &est).into_bytes(),
        "xlsx" => export::estimate_xlsx(&state, &lab, true, &Stamp::from_ms(now), &Stamp::local(now, cc_clock::Zone::System)),
        "xlsx-stored" => export::estimate_xlsx(&state, &lab, false, &Stamp::from_ms(now), &Stamp::local(now, cc_clock::Zone::System)),
        "sheets" => jv::stringify(&export::sheets_json(&export::estimate_sheets(&state, &lab, &est))).into_bytes(),
        "spots" => match export::spot_elevations_csv(&state, flag(rest, "--sheet").unwrap_or(""), &lab) {
            Ok((csv, _)) => csv.into_bytes(),
            Err(why) => {
                crate::print_error("spots", &why);
                return ExitCode::from(1);
            }
        },
        "glb" => {
            let sheet = flag(rest, "--sheet").unwrap_or("");
            match cc_takeoff::scene3d::build_scene(&state, sheet, &cc_takeoff::scene3d::View::default()) {
                Ok(sc) => cc_io_takeoff::glb::scene_glb(&sc, "TakeoffCraft"),
                Err(e) => {
                    crate::print_error("scene", &e);
                    return ExitCode::from(1);
                }
            }
        }
        _ => {
            eprintln!("unknown export kind {kind}");
            return ExitCode::from(2);
        }
    };
    write_out(out, &bytes)
}

fn cands_json(c: &[matcher::Cand]) -> Value {
    Value::Array(c.iter().map(|c| json!({ "x": jv::n(c.x), "y": jv::n(c.y), "w": jv::n(c.w), "h": jv::n(c.h), "rot": jv::n(c.rot), "score": jv::n(c.score) })).collect())
}

fn match_cmd(rest: &[&str]) -> ExitCode {
    let (Some(img), Some(tpl)) = (rest.first(), rest.get(1)) else {
        eprintln!("usage: cadcraft-cli takeoff match <image.ink> <template.ink> [--floor F] [--any]");
        return ExitCode::from(2);
    };
    let (img, tpl) = match (read_ink(img), read_ink(tpl)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => {
            crate::print_error("ink", &e);
            return ExitCode::from(1);
        }
    };
    let floor: f64 = flag(rest, "--floor").and_then(|s| s.parse().ok()).unwrap_or(0.5);
    let rots = if rest.contains(&"--any") { matcher::any_angles() } else { vec![0.0, 90.0, 180.0, 270.0] };
    let t = Instant::now();
    let c = matcher::find_symbol(&img, &tpl, &rots, floor, matcher::BLUR, None, None).unwrap_or_default();
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    println!("{}", jv::stringify(&json!({ "cands": cands_json(&c), "ms": ms })));
    ExitCode::SUCCESS
}

fn bench_cmd(rest: &[&str]) -> ExitCode {
    let (Some(what), Some(input)) = (rest.first(), rest.get(1)) else {
        eprintln!("usage: cadcraft-cli takeoff bench <estimate|xlsx|formula|count> <input> [--runs N]");
        return ExitCode::from(2);
    };
    let runs: usize = flag(rest, "--runs").and_then(|s| s.parse().ok()).unwrap_or(5).clamp(1, 1000);
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
    let best = match *what {
        "estimate" | "xlsx" => {
            let state = match load_state(input) {
                Ok(s) => s,
                Err(c) => return c,
            };
            if *what == "estimate" {
                time(&mut || {
                    let e = model::estimate_all(&state);
                    std::hint::black_box(e);
                })
            } else {
                let lab = |k: &str| k.to_string();
                time(&mut || {
                    let b = export::estimate_xlsx(&state, &lab, true, &Stamp::from_ms(0.0), &Stamp::from_ms(0.0));
                    std::hint::black_box(b);
                })
            }
        }
        "formula" => {
            let v = match read_json(input) {
                Ok(v) => v,
                Err(c) => return c,
            };
            let list: Vec<(String, Vec<(String, f64)>)> = jv::arr(Some(&v))
                .map_or(&[][..], Vec::as_slice)
                .iter()
                .map(|c| {
                    let vals = jv::obj(c.get("vars"))
                        .map(|o| o.iter().map(|(k, x)| (k.clone(), jv::to_number(Some(x)))).collect())
                        .unwrap_or_default();
                    (jv::to_string(c.get("src")), vals)
                })
                .collect();
            time(&mut || {
                let mut acc = 0.0;
                for (src, vals) in &list {
                    if let Ok(x) = cc_takeoff::formula::compute(src, vals) {
                        acc += x;
                    }
                }
                std::hint::black_box(acc);
            })
        }
        "count" => return crate::takeoff_pdf::bench_count(input, rest, runs),
        _ => {
            eprintln!("unknown bench {what}");
            return ExitCode::from(2);
        }
    };
    println!("{}", json!({ "best": best, "runs": runs }));
    ExitCode::SUCCESS
}
