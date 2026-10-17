//! `cadcraft-cli takeoff count | marks | proposal | embed | pdfinfo | snap`: the PDF side of TakeoffCraft
//! (cc-io-takeoff): pages rendered with hayro for auto count, the marked PDF copy, the proposal, the takeoff inside the
//! PDF, the page facts openPdf gives and snapping to a page's linework.

use std::process::ExitCode;
use std::time::Instant;

use serde_json::{Value, json};

use cc_io_takeoff::{pdfmarks, pdfread};
use cc_takeoff::jv;
use cc_takeoff::matcher;
use cc_takeoff::model;

fn flag<'a>(rest: &[&'a str], k: &str) -> Option<&'a str> {
    rest.iter().position(|a| *a == k).and_then(|i| rest.get(i + 1)).copied()
}
fn read(path: &str) -> Result<Vec<u8>, ExitCode> {
    std::fs::read(path).map_err(|e| {
        crate::print_error("io", &e.to_string());
        ExitCode::from(1)
    })
}
fn read_state(path: &str) -> Result<Value, ExitCode> {
    let b = read(path)?;
    let j = jv::parse_json(&String::from_utf8_lossy(&b), 512).map_err(|e| {
        crate::print_error("json", &e);
        ExitCode::from(1)
    })?;
    model::read_project(&j).map(|r| r.state).map_err(|e| {
        crate::print_error("project", &e);
        ExitCode::from(1)
    })
}
fn open(path: &str) -> Result<pdfread::Doc, ExitCode> {
    let b = read(path)?;
    pdfread::open(&b).map_err(|e| {
        crate::print_error("pdf", &e);
        ExitCode::from(1)
    })
}
fn write_ink(path: &str, img: &matcher::Img) -> std::io::Result<()> {
    let mut b = Vec::with_capacity(12 + img.data.len() * 4);
    b.extend_from_slice(b"INK1");
    b.extend_from_slice(&u32::try_from(img.w).unwrap_or(0).to_le_bytes());
    b.extend_from_slice(&u32::try_from(img.h).unwrap_or(0).to_le_bytes());
    for v in &img.data {
        b.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, b)
}

pub fn count_cmd(rest: &[&str]) -> ExitCode {
    let Some(file) = rest.first() else {
        eprintln!(
            "usage: cadcraft-cli takeoff count <file.pdf> --page N --box x0,y0,x1,y1 [--any] [--template-page N] [--scale-by K] [--dump-ink prefix]"
        );
        return ExitCode::from(2);
    };
    let doc = match open(file) {
        Ok(d) => d,
        Err(c) => return c,
    };
    let page: usize = flag(rest, "--page").and_then(|s| s.parse().ok()).unwrap_or(0);
    let tp: usize = flag(rest, "--template-page").and_then(|s| s.parse().ok()).unwrap_or(page);
    let scale_by: f64 = flag(rest, "--scale-by").and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let bx: Vec<f64> = flag(rest, "--box").map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect()).unwrap_or_default();
    let [x0, y0, x1, y1] = bx.as_slice() else {
        eprintln!("--box x0,y0,x1,y1");
        return ExitCode::from(2);
    };
    let rots = if rest.contains(&"--any") { matcher::any_angles() } else { vec![0.0, 90.0, 180.0, 270.0] };
    let t = Instant::now();
    match pdfread::search_sheet(&doc, page, [*x0, *y0, *x1, *y1], &rots, tp, scale_by) {
        Ok(s) => {
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            if let Some(prefix) = flag(rest, "--dump-ink") {
                let _ = write_ink(&format!("{prefix}.image.ink"), &s.image);
                let _ = write_ink(&format!("{prefix}.template.ink"), &s.template);
            }
            let cands: Vec<Value> = s
                .cands
                .iter()
                .map(|(c, (cx, cy))| json!({ "x": jv::n(c.x), "y": jv::n(c.y), "w": jv::n(c.w), "h": jv::n(c.h), "rot": jv::n(c.rot), "score": jv::n(c.score), "sheet": "s", "center": [jv::n(*cx), jv::n(*cy)] }))
                .collect();
            println!(
                "{}",
                jv::stringify(
                    &json!({ "w": jv::n(s.w), "cands": cands, "ms": ms, "image": [s.image.w, s.image.h], "template": [s.template.w, s.template.h] })
                )
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            crate::print_error("count", &e);
            ExitCode::from(1)
        }
    }
}

pub fn marks_cmd(rest: &[&str]) -> ExitCode {
    let (Some(file), Some(proj), Some(hash), Some(out)) = (rest.first(), rest.get(1), rest.get(2), rest.get(3)) else {
        eprintln!("usage: cadcraft-cli takeoff marks <file.pdf> <project.json> <hash> <out.pdf> [--no-legend]");
        return ExitCode::from(2);
    };
    let bytes = match read(file) {
        Ok(b) => b,
        Err(c) => return c,
    };
    let state = match read_state(proj) {
        Ok(s) => s,
        Err(c) => return c,
    };
    match pdfmarks::pdf_with_marks(&bytes, hash, &state, !rest.contains(&"--no-legend")) {
        Ok(m) => {
            if let Err(e) = std::fs::write(out, &m.bytes) {
                crate::print_error("io", &e.to_string());
                return ExitCode::from(1);
            }
            println!("{}", json!({ "pages": m.pages, "marks": m.marks }));
            ExitCode::SUCCESS
        }
        Err(e) => {
            crate::print_error("marks", &e);
            ExitCode::from(1)
        }
    }
}

pub fn proposal_cmd(rest: &[&str]) -> ExitCode {
    let (Some(proj), Some(out)) = (rest.first(), rest.get(1)) else {
        eprintln!("usage: cadcraft-cli takeoff proposal <project.json> <out.pdf> [--date ISO-ms]");
        return ExitCode::from(2);
    };
    let state = match read_state(proj) {
        Ok(s) => s,
        Err(c) => return c,
    };
    let ms: f64 = flag(rest, "--date").and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let bytes = cc_io_takeoff::proposal::proposal_pdf(&state, ms);
    match std::fs::write(out, bytes) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            crate::print_error("io", &e.to_string());
            ExitCode::from(1)
        }
    }
}

pub fn embed_cmd(rest: &[&str]) -> ExitCode {
    let (Some(file), Some(proj), Some(hash), Some(out)) = (rest.first(), rest.get(1), rest.get(2), rest.get(3)) else {
        eprintln!("usage: cadcraft-cli takeoff embed <file.pdf> <takeoff.json> <hash> <out.pdf> [--now MS]");
        return ExitCode::from(2);
    };
    let bytes = match read(file) {
        Ok(b) => b,
        Err(c) => return c,
    };
    let tj = match read(proj).map(|b| jv::parse_json(&String::from_utf8_lossy(&b), 512)) {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            crate::print_error("json", &e);
            return ExitCode::from(1);
        }
        Err(c) => return c,
    };
    let now: f64 = flag(rest, "--now").and_then(|s| s.parse().ok()).unwrap_or(0.0);
    match cc_io_takeoff::embed::embed_takeoff(&bytes, &tj, hash, now) {
        Ok(b) => match std::fs::write(out, b) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                crate::print_error("io", &e.to_string());
                ExitCode::from(1)
            }
        },
        Err(e) => {
            crate::print_error("embed", &e);
            ExitCode::from(1)
        }
    }
}

/// `takeoff pdfinfo <file.pdf>`: what openPdf gives (hash, size, pages), plus embedded takeoff when there is one.
pub fn pdfinfo_cmd(rest: &[&str]) -> ExitCode {
    let Some(file) = rest.first() else {
        eprintln!("usage: cadcraft-cli takeoff pdfinfo <file.pdf>");
        return ExitCode::from(2);
    };
    let bytes = match read(file) {
        Ok(b) => b,
        Err(c) => return c,
    };
    let doc = match pdfread::open(&bytes) {
        Ok(d) => d,
        Err(e) => {
            crate::print_error("pdf", &e);
            return ExitCode::from(1);
        }
    };
    let pages: Vec<Value> = pdfread::pages(&doc)
        .iter()
        .map(|p| {
            json!({
                "index": p.index, "label": p.label, "number": p.number,
                "view": p.view.iter().map(|v| jv::n(*v)).collect::<Vec<_>>(), "rotate": jv::n(p.rotate), "userUnit": jv::n(p.user_unit),
                "width": jv::n(p.width), "height": jv::n(p.height), "transform": p.transform.iter().map(|v| jv::n(*v)).collect::<Vec<_>>(),
            })
        })
        .collect();
    let embedded = cc_io_takeoff::embed::read_embedded(&doc).map_or(Value::Null, |(s, t)| json!({ "standsFor": s, "takeoff": t }));
    println!("{}", jv::stringify(&json!({ "hash": doc.hash, "size": doc.size, "pages": pages, "embedded": embedded })));
    ExitCode::SUCCESS
}

/// `takeoff snap <file.pdf> --page N <queries.json>`: the page's linework count and snap answers.
pub fn snap_cmd(rest: &[&str]) -> ExitCode {
    let (Some(file), Some(qf)) = (rest.first(), rest.iter().skip(1).find(|a| a.ends_with(".json"))) else {
        eprintln!("usage: cadcraft-cli takeoff snap <file.pdf> --page N <queries.json>");
        return ExitCode::from(2);
    };
    let doc = match open(file) {
        Ok(d) => d,
        Err(c) => return c,
    };
    let page: usize = flag(rest, "--page").and_then(|s| s.parse().ok()).unwrap_or(0);
    let qs = match read(qf).map(|b| jv::parse_json(&String::from_utf8_lossy(&b), 64)) {
        Ok(Ok(v)) => v,
        _ => return ExitCode::from(1),
    };
    let lw = match pdfread::page_linework(&doc, page, cc_takeoff::snap::MAX_SEGS) {
        Ok(l) => l,
        Err(e) => {
            crate::print_error("snap", &e);
            return ExitCode::from(1);
        }
    };
    let idx = cc_takeoff::snap::SnapIndex::new(&lw);
    let hits: Vec<Value> = jv::arr(Some(&qs))
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .map(|q| {
            idx.snap(jv::to_number(q.get("x")), jv::to_number(q.get("y")), jv::to_number(q.get("tol")), cc_takeoff::snap::Kinds::default())
                .map_or(Value::Null, |h| json!({ "x": jv::n(h.x), "y": jv::n(h.y), "kind": h.kind, "d": jv::n(h.d) }))
        })
        .collect();
    println!("{}", jv::stringify(&json!({ "n": lw.n, "truncated": lw.truncated, "hits": hits })));
    ExitCode::SUCCESS
}

pub fn bench_count(input: &str, rest: &[&str], runs: usize) -> ExitCode {
    let doc = match open(input) {
        Ok(d) => d,
        Err(c) => return c,
    };
    let page: usize = flag(rest, "--page").and_then(|s| s.parse().ok()).unwrap_or(0);
    let bx: Vec<f64> = flag(rest, "--box").map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect()).unwrap_or_default();
    let [x0, y0, x1, y1] = bx.as_slice() else {
        eprintln!("--box x0,y0,x1,y1");
        return ExitCode::from(2);
    };
    let rots = vec![0.0, 90.0, 180.0, 270.0];
    let mut best = f64::INFINITY;
    let mut render = f64::INFINITY;
    for _ in 0..=runs {
        let t = Instant::now();
        let s = pdfread::search_sheet(&doc, page, [*x0, *y0, *x1, *y1], &rots, page, 1.0);
        best = best.min(t.elapsed().as_secs_f64() * 1000.0);
        if let Ok(s) = s {
            let t2 = Instant::now();
            let _ = pdfread::render_ink(&doc, page, s.w);
            render = render.min(t2.elapsed().as_secs_f64() * 1000.0);
        }
    }
    println!("{}", json!({ "best": best, "render": render, "runs": runs }));
    ExitCode::SUCCESS
}
