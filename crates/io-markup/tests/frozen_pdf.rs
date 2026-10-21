//! The PDF sections of MarkupCraft 0.5's Node test (`bin/test-markupcraft.mjs`), ported on PDFs made here (blank
//! sets, page trees with labels, the summary report for text) instead of the client and fixture files: save and read
//! back, page operations and flattening, layers, bookmarks, blank pages, the PDF summary, find text and vector
//! segments. Parity with the frozen pdf.js / pdf-lib on real files is `forks/tools/mkpdf.mjs`.

use cc_io_markup::libdoc::LibDoc;
use cc_io_markup::obj::{Dict, Obj};
use cc_io_markup::pages::{
    blank_pdf, create, delete_pages, extract_pages, insert_blank, insert_page, insert_pages, move_pages, new_page, rotate_pages,
};
use cc_io_markup::read::{OpenOpts, Record, open_pdf};
use cc_io_markup::write::{Flatten, LayerIn, WriteOpts, WritePage, write_marked_up};
use cc_markup::model::{Env, Markup, make_markup};
use cc_markup::units::{Cal, preset_calibration};
use serde_json::{Map, Value, json};

const NOW: f64 = 1_791_882_000_000.0;
const AUTHOR: &str = "José Müller";

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap()
}
fn mk(e: &mut Env, v: Value) -> Markup {
    make_markup(e, &obj(v))
}
fn quarter() -> Cal {
    preset_calibration("1/4\" = 1'-0\"", 48.0, "ft-in")
}
fn env() -> Env {
    Env { now_ms: NOW, count: 0, ..Env::default() }
}
fn open(bytes: &[u8], file_id: &str) -> Record {
    let mut e = Env { now_ms: NOW, count: 100_000, ..Env::default() };
    open_pdf(&mut e, bytes, &OpenOpts { file_id: file_id.into(), name: "x.pdf".into(), read_text: true }).unwrap()
}
fn page(index: usize, markups: Vec<Markup>, cal: Option<Cal>) -> WritePage {
    WritePage { index, markups, cal, ..WritePage::default() }
}
fn write(bytes: &[u8], pages: Vec<WritePage>) -> (Vec<u8>, cc_io_markup::write::Report) {
    write_marked_up(bytes, &WriteOpts { pages, now_ms: NOW, ..WriteOpts::default() }).unwrap()
}
fn width(r: &Record, i: usize) -> f64 {
    r.pages[i].view[2] - r.pages[i].view[0]
}

/// A plain PDF with these page sizes and, when given, page labels (as /P prefixes).
fn plain_pdf(sizes: &[(f64, f64)], labels: Option<&[&str]>) -> Vec<u8> {
    let mut doc: LibDoc = create(NOW);
    for (k, (w, h)) in sizes.iter().enumerate() {
        let p = new_page(&mut doc, *w, *h);
        insert_page(&mut doc, k, p).unwrap();
    }
    if let Some(ls) = labels {
        let mut nums = Vec::new();
        for (i, l) in ls.iter().enumerate() {
            nums.push(Obj::Num(i as f64));
            nums.push(Obj::Dict(Dict::new().with("P", Obj::hex_text(l))));
        }
        let cr = doc.root_ref().unwrap();
        doc.get_mut(cr).and_then(Obj::as_dict_mut).unwrap().set("PageLabels", Obj::Dict(Dict::new().with("Nums", Obj::Arr(nums))));
    }
    doc.save()
}

fn all(e: &mut Env, s: &str) -> Vec<Markup> {
    let a = AUTHOR;
    [
        json!({ "sheet": s, "type": "rect", "rect": [100, 100, 300, 200], "author": a, "comment": "Check clearance", "style": { "fill": "#ffee88", "fillOpacity": 0.4 } }),
        json!({ "sheet": s, "type": "ellipse", "rect": [1000, 120, 1180, 260], "author": a }),
        json!({ "sheet": s, "type": "cloudrect", "rect": [400, 400, 600, 500], "author": a, "status": "Accepted", "punch": { "n": 1, "assignee": "Mechanical sub", "due": "2026-11-01", "status": "Open" } }),
        json!({ "sheet": s, "type": "cloud", "pts": [[720, 400], [900, 380], [950, 520], [760, 560]], "author": a, "status": "Rejected" }),
        json!({ "sheet": s, "type": "polygon", "pts": [[150, 400], [260, 380], [280, 480]], "author": a }),
        json!({ "sheet": s, "type": "polyline", "pts": [[1050, 700], [1150, 760], [1100, 840]], "author": a }),
        json!({ "sheet": s, "type": "line", "pts": [[100, 700], [280, 700]], "author": a }),
        json!({ "sheet": s, "type": "arrow", "pts": [[650, 900], [760, 760]], "author": a }),
        json!({ "sheet": s, "type": "pen", "pts": [[900, 1500], [940, 1540], [990, 1500]], "author": a }),
        json!({ "sheet": s, "type": "highlight", "pts": [[450, 1000], [700, 1000]], "author": a }),
        json!({ "sheet": s, "type": "callout", "rect": [700, 1240, 890, 1290], "pts": [[620, 1180], [690, 1265]], "text": "Move prep sink 6\" east (coordinate with plumbing) ✓", "author": a }),
        json!({ "sheet": s, "type": "text", "rect": [150, 1580, 400, 1610], "text": "Verify door swing at walk-in cooler", "author": a }),
        json!({ "sheet": s, "type": "note", "pts": [[1150, 1100]], "comment": "Confirm hood size", "author": a }),
        json!({ "sheet": s, "type": "stamp", "rect": [950, 1600, 1150, 1670], "label": "Revise and resubmit", "author": a }),
        json!({ "sheet": s, "type": "measure", "pts": [[400, 640], [688, 640]], "author": a }),
        json!({ "sheet": s, "type": "area", "pts": [[100, 1000], [388, 1000], [388, 1144], [100, 1144]], "holes": [[[150, 1050], [222, 1050], [222, 1122], [150, 1122]]], "author": a }),
        json!({ "sheet": s, "type": "count", "pts": [[200, 1300], [240, 1300], [280, 1300]], "label": "Outlets", "author": a }),
        json!({ "sheet": s, "type": "rect", "rect": [500, 1300, 600, 1350], "author": a, "replies": [{ "id": "r1", "author": "Ann", "text": "Looks right", "kind": "comment", "date": "2026-10-12T10:00:00Z" }, { "id": "r2", "kind": "status", "status": "Accepted", "author": "Bo", "date": "2026-10-12T11:00:00Z", "text": "Status set" }] }),
    ]
    .into_iter()
    .map(|v| mk(e, v))
    .collect()
}

#[test]
fn save_into_a_plan_sheet_and_read_it_back() {
    let base = blank_pdf(Some(&json!({ "paper": "arch-d", "grid": "square", "pages": 1 })), "", NOW);
    let mut e = env();
    let ms = all(&mut e, "f1:0");
    let (saved, report) = write(&base, vec![WritePage { badge: 9.0, ..page(0, ms.clone(), Some(quarter())) }]);
    assert_eq!(report.written, ms.len(), "wrote every markup as an annotation");
    assert_eq!(report.missing, 1, "one character outside WinAnsi counted (✓)");
    let back = open(&saved, "f1");
    assert_eq!(back.markups.len(), ms.len(), "every markup reopens editable");
    for m in &ms {
        let r = back.markups.iter().find(|x| x.id == m.id).unwrap();
        assert!(
            r.kind == m.kind && r.rect == m.rect && r.pts == m.pts && r.text == m.text && r.comment == m.comment && r.status == m.status,
            "{} reopens equal",
            m.kind
        );
    }
    let area = back.markups.iter().find(|m| m.kind == "area").unwrap();
    assert_eq!(area.holes.as_ref().unwrap().len(), 1, "cut-outs reopen");
    let th = back.markups.iter().find(|m| m.replies.len() == 2).unwrap();
    assert!(th.replies[0].text == "Looks right" && th.replies[1].kind == "status", "the thread reopens");
    assert!(back.calibrations.iter().any(|(k, c)| k == "f1:0" && (c.mm_per_pt - quarter().mm_per_pt).abs() < 1e-12), "the scale reopens");
    // saving again replaces our annotations, no duplicates
    let (again, _) = write_marked_up(
        &saved,
        &WriteOpts {
            pages: vec![page(0, back.markups.clone(), None)],
            remove_refs: back.remove_refs.clone(),
            now_ms: NOW,
            ..WriteOpts::default()
        },
    )
    .unwrap();
    assert_eq!(open(&again, "f1").markups.len(), ms.len(), "save again → no duplicates");
}

#[test]
fn page_operations_and_flattening() {
    let base = plain_pdf(&[(612.0, 792.0), (792.0, 612.0), (1296.0, 1728.0)], Some(&["A-1", "A-2", "A-3"]));
    let mut e = env();
    let mkp = |e: &mut Env, sheet: &str, n: u32| {
        vec![
            mk(e, json!({ "sheet": sheet, "type": "rect", "rect": [50, 50, 150, 120], "author": AUTHOR, "comment": format!("page {n}") })),
            mk(e, json!({ "sheet": sheet, "type": "text", "rect": [60, 200, 260, 230], "text": format!("On page {n}"), "author": AUTHOR })),
        ]
    };
    let m0 = mkp(&mut e, "p:0", 1);
    let m2 = mkp(&mut e, "p:2", 3);
    let labels: Vec<String> = ["A-1", "A-2", "A-3"].iter().map(|s| s.to_string()).collect();
    let (baked, _) = write(&base, vec![page(0, m0.clone(), None), page(1, vec![], None), page(2, m2.clone(), Some(quarter()))]);
    let by_page = |r: &Record| -> Vec<String> {
        r.pages
            .iter()
            .map(|pg| {
                let mut v: Vec<String> = r
                    .markups
                    .iter()
                    .filter(|m| m.sheet.as_deref() == Some(pg.key.as_str()))
                    .map(|m| if m.comment.is_empty() { m.text.clone() } else { m.comment.clone() })
                    .collect();
                v.sort();
                v.join("|")
            })
            .collect()
    };
    let label_list = |r: &Record| r.pages.iter().map(|p| p.info.label.clone()).collect::<Vec<_>>().join(",");
    let moved = open(&move_pages(&baked, &[2.0], 0.0, Some(&labels)).unwrap(), "p");
    assert_eq!(label_list(&moved), "A-3,A-1,A-2", "move: page labels follow the pages");
    assert_eq!(by_page(&moved), vec!["On page 3|page 3", "On page 1|page 1", ""], "move: each page keeps its markups");
    assert!(
        moved.calibrations.iter().any(|(k, _)| k == "p:0")
            && !moved.calibrations.iter().any(|(k, _)| k == "p:2")
            && width(&moved, 0) == 1296.0,
        "the scale and size travel too"
    );
    assert!(moved.markups.iter().all(|m| m0.iter().chain(m2.iter()).any(|o| o.id == m.id)), "move: markup ids are kept");
    let del = open(&delete_pages(&baked, &[0.0], Some(&labels)).unwrap(), "p");
    assert!(
        del.pages.len() == 2 && label_list(&del) == "A-2,A-3" && by_page(&del) == vec!["", "On page 3|page 3"],
        "delete: the page and its markups go"
    );
    assert_eq!(
        delete_pages(&baked, &[0.0, 1.0, 2.0], None).unwrap_err(),
        "A PDF needs at least one page: delete fewer pages, or close the file.",
        "deleting every page is refused"
    );
    let (ins, count) = insert_pages(&baked, &base, 1.0, Some(&[1.0]), Some(&labels)).unwrap();
    let ins_r = open(&ins, "p");
    assert!(
        count == 1 && ins_r.pages.len() == 4 && width(&ins_r, 1) == 792.0 && label_list(&ins_r) == "A-1,2,A-2,A-3",
        "insert from a PDF: numbered by its place"
    );
    let blank = open(&insert_blank(&baked, 3.0, None, Some(&labels), None).unwrap(), "p");
    assert!(
        blank.pages.len() == 4 && width(&blank, 3) == 1296.0 && blank.pages[3].view[3] - blank.pages[3].view[1] == 1728.0,
        "blank page: the size of the page before it"
    );
    let mut turned = m2.clone();
    turned[1].rot = 90.0;
    let (baked90, _) = write(&base, vec![page(0, m0.clone(), None), page(2, turned, Some(quarter()))]);
    let rotated = open(&rotate_pages(&baked90, &[2.0], 90.0).unwrap(), "p");
    let rt = rotated.markups.iter().find(|m| m.kind == "text" && m.sheet.as_deref() == Some("p:2")).unwrap();
    assert!(
        rotated.pages[2].rotate == 90.0 && rotated.pages[0].rotate == 0.0 && rt.rot == 90.0 && rt.rect == m2[1].rect,
        "rotate: the page turns, the markup stays put"
    );
    let back = rotate_pages(&rotate_pages(&baked, &[0.0], -90.0).unwrap(), &[0.0], -270.0).unwrap();
    assert_eq!(open(&back, "p").pages[0].rotate, 0.0, "rotations add up and wrap at 360°");
    let ex = open(&extract_pages(&baked, &[2.0, 0.0], NOW).unwrap(), "p");
    assert!(
        ex.pages.len() == 2 && width(&ex, 0) == 1296.0 && by_page(&ex) == vec!["On page 3|page 3", "On page 1|page 1"],
        "extract: those pages in that order, markups editable"
    );
    // flattening
    let (flat, fr) = write_marked_up(
        &base,
        &WriteOpts { pages: vec![page(0, m0.clone(), None)], flatten: Flatten::All, now_ms: NOW, ..WriteOpts::default() },
    )
    .unwrap();
    let flat_r = open(&flat, "p");
    assert!(fr.flattened == 2 && fr.written == 0 && flat_r.markups.is_empty(), "flatten: nothing editable on reopen");
    let mut one = indexmap::IndexSet::new();
    one.insert(m0[0].id.clone());
    let (some, sr) = write_marked_up(
        &base,
        &WriteOpts { pages: vec![page(0, m0.clone(), None)], flatten: Flatten::Some(one), now_ms: NOW, ..WriteOpts::default() },
    )
    .unwrap();
    let some_r = open(&some, "p");
    assert!(
        sr.flattened == 1 && sr.written == 1 && some_r.markups.len() == 1 && some_r.markups[0].id == m0[1].id,
        "flatten chosen markups only"
    );
}

#[test]
fn markup_layers_in_the_pdf() {
    let base = plain_pdf(&[(612.0, 792.0)], None);
    let mut e = env();
    let a = mk(&mut e, json!({ "sheet": "L:0", "type": "rect", "rect": [50, 50, 150, 120], "author": AUTHOR, "layer": "Mechanical" }));
    let b =
        mk(&mut e, json!({ "sheet": "L:0", "type": "cloudrect", "rect": [300, 300, 400, 380], "author": AUTHOR, "layer": "Architect" }));
    let c = mk(&mut e, json!({ "sheet": "L:0", "type": "text", "rect": [60, 400, 260, 430], "text": "no layer", "author": AUTHOR }));
    let layers = vec![
        LayerIn { name: json!("Architect"), hidden: false, locked: true },
        LayerIn { name: json!("Mechanical"), hidden: true, locked: false },
        LayerIn { name: json!("Électrique"), hidden: false, locked: false },
    ];
    let (bytes, _) = write_marked_up(
        &base,
        &WriteOpts { pages: vec![page(0, vec![a.clone(), b, c.clone()], None)], layers, now_ms: NOW, ..WriteOpts::default() },
    )
    .unwrap();
    let re = open(&bytes, "L");
    assert!(
        re.markups.iter().find(|m| m.id == a.id).unwrap().layer == "Mechanical"
            && re.markups.iter().find(|m| m.id == c.id).unwrap().layer.is_empty(),
        "each markup on its layer"
    );
    assert_eq!(
        cc_markup::json::stringify(&re.layers),
        "[{\"name\":\"Architect\",\"hidden\":false,\"locked\":true},{\"name\":\"Mechanical\",\"hidden\":true,\"locked\":false},{\"name\":\"Électrique\",\"hidden\":false,\"locked\":false}]",
        "the layers with their state"
    );
}

#[test]
fn bookmarks_written_and_read() {
    use cc_io_markup::read::Bookmark;
    let base = plain_pdf(&[(612.0, 792.0); 4], None);
    let bm = |t: &str, p: f64, c: Vec<Bookmark>| Bookmark { title: t.into(), page: p, children: c };
    let items = vec![
        bm("Architectural", 0.0, vec![bm("A-101 — Plan", 1.0, vec![]), bm("A-102 — Détails", 2.0, vec![])]),
        bm("Mechanical", 3.0, vec![]),
    ];
    let (bytes, _) =
        write_marked_up(&base, &WriteOpts { pages: vec![], bookmarks: Some(items), now_ms: NOW, ..WriteOpts::default() }).unwrap();
    let back = open(&bytes, "b").bookmarks;
    let flat: Vec<String> = back
        .iter()
        .flat_map(|b| {
            std::iter::once(format!("{}@{}", b.title, b.page)).chain(b.children.iter().map(|c| format!("  {}@{}", c.title, c.page)))
        })
        .collect();
    assert_eq!(
        flat,
        vec!["Architectural@0", "  A-101 — Plan@1", "  A-102 — Détails@2", "Mechanical@3"],
        "bookmarks read back the same (nesting, pages, accents)"
    );
    let (gone, _) =
        write_marked_up(&bytes, &WriteOpts { pages: vec![], bookmarks: Some(vec![]), now_ms: NOW, ..WriteOpts::default() }).unwrap();
    assert!(open(&gone, "b").bookmarks.is_empty(), "an empty list removes the outline");
}

#[test]
fn blank_pages_file_new() {
    let want = [
        ("letter", 612.0, 792.0),
        ("tabloid", 792.0, 1224.0),
        ("arch-d", 1728.0, 2592.0),
        ("arch-e", 2592.0, 3456.0),
        ("a4", 595.28, 841.89),
        ("a3", 841.89, 1190.55),
        ("a1", 1683.78, 2383.94),
    ];
    for (paper, w, h) in want {
        for orient in ["portrait", "landscape"] {
            let b = blank_pdf(Some(&json!({ "paper": paper, "orient": orient, "grid": "none", "pages": 1 })), "", NOW);
            let rec = open(&b, "b");
            let (ew, eh) = if orient == "landscape" { (h, w) } else { (w, h) };
            assert!(
                rec.pages.len() == 1
                    && (width(&rec, 0) - ew).abs() < 0.01
                    && (rec.pages[0].view[3] - eh).abs() < 0.01
                    && rec.pages[0].rotate == 0.0,
                "{paper} {orient} opens at its size"
            );
        }
    }
    let many = open(&blank_pdf(Some(&json!({ "paper": "a3", "orient": "landscape", "grid": "dot", "pages": 20 })), "", NOW), "m");
    assert!(
        many.pages.len() == 20
            && many.pages.iter().all(|p| (p.view[2] - 1190.55).abs() < 0.01)
            && many.markups.is_empty()
            && many.calibrations.is_empty(),
        "20 pages, no markups and no scale"
    );
    assert_eq!(
        many.blank.as_ref().map(cc_markup::json::stringify).as_deref(),
        Some("{\"paper\":\"a3\",\"orient\":\"landscape\",\"grid\":\"dot\",\"metric\":true}"),
        "the file says it is a blank set"
    );
    let blank_bytes =
        blank_pdf(Some(&json!({ "paper": "tabloid", "orient": "landscape", "grid": "square", "pages": 2 })), "Untitled-3", NOW);
    let mut e = env();
    let marks = [
        mk(
            &mut e,
            json!({ "sheet": "u:0", "type": "cloudrect", "rect": [200, 200, 500, 400], "author": AUTHOR, "comment": "on a blank sheet", "style": { "color": "#e02020", "width": 3 } }),
        ),
        mk(
            &mut e,
            json!({ "sheet": "u:0", "type": "text", "rect": [600, 300, 900, 360], "text": "Sketch: new door here", "author": AUTHOR }),
        ),
        mk(&mut e, json!({ "sheet": "u:1", "type": "measure", "pts": [[100, 100], [388, 100]], "author": AUTHOR })),
    ];
    let (saved, report) = write(&blank_bytes, vec![page(0, marks[..2].to_vec(), None), page(1, marks[2..].to_vec(), Some(quarter()))]);
    let back = open(&saved, "u");
    assert!(report.written == 3 && back.markups.len() == 3, "markups saved onto the blank pages reopen editable");
    assert!(
        back.calibrations.iter().any(|(k, _)| k == "u:1")
            && !back.calibrations.iter().any(|(k, _)| k == "u:0")
            && back.blank.as_ref().is_some_and(|b| b["grid"] == "square"),
        "the blank-set note survives the save"
    );
    let added = open(&insert_blank(&saved, 2.0, None, None, Some(("square", false))).unwrap(), "u");
    assert!(
        added.pages.len() == 3 && (width(&added, 2) - 1224.0).abs() < 0.01 && added.markups.len() == 3,
        "add a blank page: the set's paper, the markups stay"
    );
}

#[test]
fn markup_summary_pdf() {
    use cc_markup::csv::{Cell, SheetRef};
    let mut e = env();
    let one = all(&mut e, "S:0");
    let ms: Vec<Markup> = (0..5).flat_map(|_| one.clone()).collect();
    let refs: Vec<&Markup> = ms.iter().collect();
    let sheet_of = |_: &Markup| Some(SheetRef { title: "A-101 — Plan".into(), file: "set.pdf".into(), page: Some(Cell::N(1.0)) });
    let r = cc_io_markup::summary::summary_pdf(
        &refs,
        &sheet_of,
        &|_| String::new(),
        "Example kitchen",
        cc_markup::js::parse_date("2026-10-13T00:00:00Z"),
        NOW,
    );
    let doc = cc_io_markup::jsdoc::JsDoc::open(&r.bytes).unwrap();
    let text = |i: usize| cc_io_markup::text::text_content(&doc, i).into_iter().map(|t| t.str).collect::<Vec<_>>().join(" ");
    let first = text(0);
    let last = text(doc.pages.len() - 1);
    assert!(
        r.pages >= 2
            && doc.pages.len() == r.pages
            && first.contains("Example kitchen")
            && first.contains(&format!("2026-10-13, {} markups", ms.len()))
            && first.contains("Text or comment")
            && last.contains(&format!("Page {} of {}", r.pages, r.pages)),
        "the PDF report: title block, header, page numbers"
    );
    assert!(r.missing > 0 && first.contains("Move prep sink"), "characters outside WinAnsi are counted (✓ in a callout)");
    // find text in it (textsearch.js on the page's own text)
    let items: Vec<cc_markup::textsearch::Item> = cc_io_markup::text::text_content(&doc, 0)
        .into_iter()
        .map(|t| cc_markup::textsearch::Item { str: t.str, transform: Some(t.transform.to_vec()), width: t.width, has_eol: t.has_eol })
        .collect();
    let idx = cc_markup::textsearch::text_index(items);
    let hits = cc_markup::textsearch::find_hits(&idx, "example KITCHEN", false, false, 2000.0);
    assert!(hits.len() == 1 && hits[0].rect[1] > 500.0, "find: one hit, in the title block");
    assert!(
        cc_markup::textsearch::find_hits(&idx, "KITCHEN", true, false, 2000.0).is_empty()
            && cc_markup::textsearch::find_hits(&idx, "(a+", false, false, 2000.0).is_empty(),
        "match case; regex characters are safe"
    );
    // vector segments: the table rules are paths
    let segs = cc_markup::vecdiff::page_segments(&cc_io_markup::ops::page_ops(&doc, 0), 400_000);
    assert!(segs.len() > 5 && segs.iter().all(|s| (s[1] - s[3]).abs() < 1e-9), "the table's rules come out as horizontal segments");
}

#[test]
fn compare_by_vector_paths_in_pdfs() {
    let mk_grid = |grid: &str| blank_pdf(Some(&json!({ "paper": "letter", "grid": grid, "pages": 1 })), "", NOW);
    let a = cc_io_markup::jsdoc::JsDoc::open(&mk_grid("square")).unwrap();
    let b = cc_io_markup::jsdoc::JsDoc::open(&mk_grid("none")).unwrap();
    let sa = cc_markup::vecdiff::page_segments(&cc_io_markup::ops::page_ops(&a, 0), 400_000);
    let sb = cc_markup::vecdiff::page_segments(&cc_io_markup::ops::page_ops(&b, 0), 400_000);
    assert!(sa.len() == 72 && sb.is_empty(), "a square grid is 72 segments, a plain page none");
    let d = cc_markup::vecdiff::diff_segments(&sa, &sb, 1.5, None, 2.0);
    assert!(!d.removed.is_empty() && d.added.is_empty(), "the grid shows as removed");
}
