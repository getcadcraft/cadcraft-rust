//! More `cadcraft-cli markup` subcommands (the 0.4 K-items that touch PDF files):
//!
//!   cadcraft-cli markup pages <rotate|delete|move|insert|blank|extract> <in.pdf> <opts.json> <out.pdf>
//!       opts: {indices, delta, to, at, labels, size, grid: {grid, metric}, src: "<other.pdf>", now}; prints {count} or
//!       {error} (the out file is written only on success)
//!   cadcraft-cli markup blank <opts.json> <out.pdf>          blankPdf: opts {raw, title, now}
//!   cadcraft-cli markup summary <opts.json> <out.pdf>        summaryPdf: opts {markups, sheets, measure, title, date,
//!                                                           now}; prints {missing, pages}
//!   cadcraft-cli markup segments <file.pdf> [--max N]        vecdiff pageSegments of every page (JSON)
//!   cadcraft-cli markup find <file.pdf> <opts.json>          textIndex + findHits on each page's own text: opts
//!                                                           {query, matchCase, wholeWord, max}

use cc_markup::csv::{Cell, SheetRef};
use serde_json::{Value, json};
use std::process::ExitCode;

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

fn write_out(path: &str, bytes: &[u8]) -> Result<(), ExitCode> {
    std::fs::write(path, bytes).map_err(|e| {
        crate::print_error("io", &e.to_string());
        ExitCode::from(1)
    })
}

fn nums(v: Option<&Value>) -> Vec<f64> {
    v.and_then(Value::as_array).map(|a| a.iter().map(cc_markup::js::to_number).collect()).unwrap_or_default()
}

fn usage() -> ExitCode {
    eprintln!(
        "usage: cadcraft-cli markup pages <op> <in.pdf> <opts.json> <out.pdf> | blank <opts.json> <out.pdf> | summary <opts.json> <out.pdf> | segments <pdf> | find <pdf> <opts.json>"
    );
    ExitCode::from(2)
}

pub fn pages(rest: &[&str]) -> ExitCode {
    let (Some(op), Some(inp), Some(opts), Some(out)) = (rest.first(), rest.get(1), rest.get(2), rest.get(3)) else { return usage() };
    let bytes = match read_bytes(inp) {
        Ok(b) => b,
        Err(c) => return c,
    };
    let o = match read_json(opts) {
        Ok(v) => v,
        Err(c) => return c,
    };
    let labels: Option<Vec<String>> =
        o.get("labels").and_then(Value::as_array).map(|a| a.iter().map(|x| x.as_str().map_or_else(String::new, str::to_string)).collect());
    let lab = labels.as_deref();
    let idx = nums(o.get("indices"));
    let now = o.get("now").and_then(Value::as_f64).unwrap_or(0.0);
    use cc_io_markup::pages as p;
    let res: Result<(Vec<u8>, Option<usize>), String> = match *op {
        "rotate" => p::rotate_pages(&bytes, &idx, o.get("delta").map_or(f64::NAN, cc_markup::js::to_number)).map(|b| (b, None)),
        "delete" => p::delete_pages(&bytes, &idx, lab).map(|b| (b, None)),
        "move" => p::move_pages(&bytes, &idx, o.get("to").map_or(f64::NAN, cc_markup::js::to_number), lab).map(|b| (b, None)),
        "insert" => {
            let src = match o.get("src").and_then(Value::as_str).map(read_bytes) {
                Some(Ok(b)) => b,
                Some(Err(c)) => return c,
                None => return usage(),
            };
            let ix = o.get("indices").filter(|v| v.is_array()).map(|_| idx.clone());
            p::insert_pages(&bytes, &src, o.get("at").map_or(f64::NAN, cc_markup::js::to_number), ix.as_deref(), lab)
                .map(|(b, n)| (b, Some(n)))
        }
        "blank" => {
            let size = o.get("size").and_then(Value::as_array).map(|a| {
                let g = |i: usize| a.get(i).map_or(f64::NAN, cc_markup::js::to_number);
                [g(0), g(1)]
            });
            let grid = o
                .get("grid")
                .filter(|g| g.is_object())
                .map(|g| (g.get("grid").and_then(Value::as_str).unwrap_or("").to_string(), cc_markup::js::truthy(g.get("metric"))));
            p::insert_blank(
                &bytes,
                o.get("at").map_or(f64::NAN, cc_markup::js::to_number),
                size,
                lab,
                grid.as_ref().map(|(g, m)| (g.as_str(), *m)),
            )
            .map(|b| (b, None))
        }
        "extract" => p::extract_pages(&bytes, &idx, now).map(|b| (b, None)),
        _ => return usage(),
    };
    match res {
        Ok((b, n)) => {
            if let Err(c) = write_out(out, &b) {
                return c;
            }
            println!("{}", json!({ "count": n }));
        }
        Err(e) => println!("{}", json!({ "error": e })),
    }
    ExitCode::SUCCESS
}

pub fn blank(rest: &[&str]) -> ExitCode {
    let (Some(opts), Some(out)) = (rest.first(), rest.get(1)) else { return usage() };
    let o = match read_json(opts) {
        Ok(v) => v,
        Err(c) => return c,
    };
    let b = cc_io_markup::pages::blank_pdf(
        o.get("raw"),
        o.get("title").and_then(Value::as_str).unwrap_or(""),
        o.get("now").and_then(Value::as_f64).unwrap_or(0.0),
    );
    if let Err(c) = write_out(out, &b) {
        return c;
    }
    println!("{}", json!({ "bytes": b.len() }));
    ExitCode::SUCCESS
}

pub fn summary(rest: &[&str]) -> ExitCode {
    let (Some(opts), Some(out)) = (rest.first(), rest.get(1)) else { return usage() };
    let o = match read_json(opts) {
        Ok(v) => v,
        Err(c) => return c,
    };
    let ms: Vec<cc_markup::Markup> =
        o.get("markups").and_then(Value::as_array).map(|a| a.iter().map(cc_markup::json::markup_from_value).collect()).unwrap_or_default();
    let refs: Vec<&cc_markup::Markup> = ms.iter().collect();
    let sheets = o.get("sheets").and_then(Value::as_object).cloned().unwrap_or_default();
    let sheet_of = |m: &cc_markup::Markup| {
        m.sheet.as_ref().and_then(|k| sheets.get(k)).map(|v| SheetRef {
            title: v.get("title").and_then(Value::as_str).unwrap_or("").to_string(),
            file: v.get("file").and_then(Value::as_str).unwrap_or("").to_string(),
            page: v.get("page").map(|p| match p {
                Value::Number(n) => Cell::N(n.as_f64().unwrap_or(f64::NAN)),
                x => Cell::S(x.as_str().unwrap_or("").to_string()),
            }),
        })
    };
    let meas = o.get("measure").and_then(Value::as_object).cloned().unwrap_or_default();
    let measure_of = |m: &cc_markup::Markup| meas.get(&m.id).and_then(Value::as_str).unwrap_or("").to_string();
    let now = o.get("now").and_then(Value::as_f64).unwrap_or(0.0);
    let date = o.get("date").and_then(Value::as_str).map_or(now, cc_markup::js::parse_date);
    let r =
        cc_io_markup::summary::summary_pdf(&refs, &sheet_of, &measure_of, o.get("title").and_then(Value::as_str).unwrap_or(""), date, now);
    if let Err(c) = write_out(out, &r.bytes) {
        return c;
    }
    println!("{}", json!({ "missing": r.missing, "pages": r.pages }));
    ExitCode::SUCCESS
}

fn open_js(path: &str) -> Result<cc_io_markup::jsdoc::JsDoc, ExitCode> {
    let bytes = read_bytes(path)?;
    cc_io_markup::jsdoc::JsDoc::open(&bytes).map_err(|e| {
        crate::print_error("refused", &format!("{e:?}"));
        ExitCode::from(1)
    })
}

pub fn segments(rest: &[&str]) -> ExitCode {
    let Some(file) = rest.first() else { return usage() };
    let doc = match open_js(file) {
        Ok(d) => d,
        Err(c) => return c,
    };
    let max =
        rest.iter().position(|a| *a == "--max").and_then(|i| rest.get(i + 1)).and_then(|s| s.parse::<usize>().ok()).unwrap_or(400_000);
    let mut o = cc_m3d::json::Out::new(true);
    o.ch('[');
    for i in 0..doc.pages.len() {
        if i > 0 {
            o.ch(',');
        }
        let segs = cc_markup::vecdiff::page_segments(&cc_io_markup::ops::page_ops(&doc, i), max);
        o.ch('[');
        for (k, s) in segs.iter().enumerate() {
            if k > 0 {
                o.ch(',');
            }
            o.nums(s);
        }
        o.ch(']');
    }
    o.ch(']');
    println!("{}", o.s);
    ExitCode::SUCCESS
}

pub fn find(rest: &[&str]) -> ExitCode {
    let (Some(file), Some(opts)) = (rest.first(), rest.get(1)) else { return usage() };
    let doc = match open_js(file) {
        Ok(d) => d,
        Err(c) => return c,
    };
    let o = match read_json(opts) {
        Ok(v) => v,
        Err(c) => return c,
    };
    let q = o.get("query").and_then(Value::as_str).unwrap_or("");
    let max = o.get("max").and_then(Value::as_f64).unwrap_or(2000.0);
    let mut pages = Vec::new();
    for i in 0..doc.pages.len() {
        let items: Vec<cc_markup::textsearch::Item> = cc_io_markup::text::text_content(&doc, i)
            .into_iter()
            .map(|t| cc_markup::textsearch::Item { str: t.str, transform: Some(t.transform.to_vec()), width: t.width, has_eol: t.has_eol })
            .collect();
        let idx = cc_markup::textsearch::text_index(items);
        let hits = cc_markup::textsearch::find_hits(
            &idx,
            q,
            cc_markup::js::truthy(o.get("matchCase")),
            cc_markup::js::truthy(o.get("wholeWord")),
            max,
        );
        pages.push(json!({
            "text": idx.text,
            "hits": hits.iter().map(|h| json!({"start": h.start, "end": h.end, "rect": h.rect.to_vec(), "snippet": h.snippet})).collect::<Vec<_>>(),
        }));
    }
    let mut out = cc_m3d::json::Out::new(true);
    out.value(&Value::Array(pages));
    println!("{}", out.s);
    ExitCode::SUCCESS
}
