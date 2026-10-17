//! Fuzz MarkupCraft's PDF side: any bytes must give a file record (annotations read back as markups, threads,
//! calibrations, labels, bookmarks, title blocks from the page text) or an error, never a panic, hang or OOM; a file
//! that opens is then saved again (the pdf-lib side: parse, drop the annotations taken over, write) and its first
//! page's vector segments and text are read.
#![no_main]

use cc_io_markup::read::{OpenOpts, open_pdf};
use cc_io_markup::write::{WriteOpts, WritePage, write_marked_up};
use cc_markup::model::Env;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut env = Env { now_ms: 1_791_882_000_000.0, count: 0 };
    let Ok(rec) = open_pdf(&mut env, data, &OpenOpts { file_id: "f".into(), name: "x.pdf".into(), read_text: true }) else { return };
    let pages: Vec<WritePage> = rec
        .pages
        .iter()
        .take(4)
        .map(|p| WritePage {
            index: p.index,
            markups: rec.markups.iter().filter(|m| m.sheet.as_deref() == Some(p.key.as_str())).cloned().collect(),
            ..WritePage::default()
        })
        .collect();
    let out = write_marked_up(data, &WriteOpts { pages, remove_refs: rec.remove_refs.clone(), now_ms: env.now_ms, ..WriteOpts::default() });
    std::hint::black_box(out.map(|(b, _)| b.len()).unwrap_or(0));
    if let Ok(doc) = cc_io_markup::jsdoc::JsDoc::open(data) {
        let segs = cc_markup::vecdiff::page_segments(&cc_io_markup::ops::page_ops(&doc, 0), 100_000);
        let text = cc_io_markup::text::text_content(&doc, 0);
        std::hint::black_box((segs.len(), text.len()));
    }
});
