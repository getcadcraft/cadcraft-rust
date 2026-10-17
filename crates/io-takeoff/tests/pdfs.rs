//! Ported from the frozen Node test (bin/test-takeoffcraft.mjs at 594ab3e): "Takeoff inside the PDF (T10)", "The
//! kitchen plan sheet (public fixture PDF)" and "Automatic counting" (on the fixture and on self-generated sheets: the
//! stamp sheet, any angle, across scales), page labels. The JS draws its generated sheets with pdf-lib and reads them
//! with pdf.js; here they are written by tests/common (same geometry) and read with hayro. The kitchen fixture
//! (getcadcraft.com/fixtures/pdf, public) and the Carey set (testfiles, a client file: never copied into any repo) are
//! read in place when present (`TAKEOFF_KITCHEN_PDF`, `TAKEOFF_CAREY_PDF` override); those tests skip when absent.
//! Text placement checks of the marked PDF (legend upright on rotated pages, legend words) go through pdf.js in the JS
//! and are covered by forks/tools/todiff.mjs, which reads both files back with the frozen pdf.js.

mod common;
use common::*;

use cc_io_takeoff::embed;
use cc_io_takeoff::pdf::{match_files, sheet_title};
use cc_io_takeoff::pdfmarks::pdf_with_marks;
use cc_io_takeoff::pdfread::{self, Doc};
use cc_takeoff::geom::{self, P};
use cc_takeoff::jv;
use cc_takeoff::matcher::{self as mt, Cand};
use cc_takeoff::model::{self, QOpts, Takeoff};
use cc_takeoff::snap::{Kinds, SnapIndex};
use cc_takeoff::units as u;
use serde_json::{Value, json};
use std::path::PathBuf;

fn near(a: f64, b: f64, t: f64) -> bool {
    (a - b).abs() <= t * 1f64.max(a.abs()).max(b.abs())
}

/// Kept candidates at `th` with their centres in PDF user space.
fn kept(search: &pdfread::Search, th: f64) -> Vec<(Cand, (f64, f64))> {
    let cands: Vec<Cand> = search.cands.iter().map(|(c, _)| c.clone()).collect();
    mt::suppress(&cands, th, 0.25).into_iter().filter_map(|c| search.cands.iter().find(|(d, _)| *d == c).cloned()).collect()
}
/// (found, true positives) against centres within `tol`.
fn score(list: &[(Cand, (f64, f64))], truth: &[(f64, f64)], tol: f64) -> (usize, usize) {
    let mut used = vec![false; truth.len()];
    let mut tp = 0;
    for (_, (x, y)) in list {
        if let Some(i) = truth.iter().enumerate().position(|(j, t)| !used[j] && (t.0 - x).hypot(t.1 - y) <= tol) {
            used[i] = true;
            tp += 1;
        }
    }
    (list.len(), tp)
}
const RIGHT: [f64; 4] = [0.0, 90.0, 180.0, 270.0];

#[test]
fn page_labels_rotation_and_titles() {
    let pages: Vec<Page> = (0..3).map(|_| Page::new(612.0, 792.0, "")).collect();
    let bytes = pdf(&pages, "/PageLabels << /Nums [0 << /S /D /P (A-) /St 101 >>] >>", &[]);
    let doc = pdfread::open(&bytes).unwrap();
    assert_eq!(doc.hash, pdfread::sha256_hex(&bytes));
    let ps = pdfread::pages(&doc);
    assert_eq!(ps.iter().map(|p| p.number.as_str()).collect::<Vec<_>>().join(","), "A-101,A-102,A-103");
    let pv = |p: &pdfread::PageInfo| json!({ "index": p.index, "number": p.number });
    assert_eq!(sheet_title("Arch set.pdf", &pv(&ps[1]), false), "A-102");
    assert_eq!(sheet_title("Arch set.pdf", &pv(&ps[1]), true), "A-102 — Arch set");
    let rot: Vec<Page> = [0, 90, 180, 270].iter().map(|r| Page { rotate: *r, ..Page::new(612.0, 792.0, "") }).collect();
    let rd = pdfread::open(&pdf(&rot, "", &[])).unwrap();
    let rp = pdfread::pages(&rd);
    assert_eq!(rp.iter().map(|p| p.rotate).collect::<Vec<_>>(), vec![0.0, 90.0, 180.0, 270.0]);
    assert!(rp[1].width == 792.0 && rp[1].height == 612.0 && rp[0].number.is_empty());
    assert_eq!(sheet_title("rot.pdf", &pv(&rp[0]), false), "rot, page 1");
    // hostile bytes never panic
    for junk in [&b""[..], b"%PDF-1.7\n", b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 1 0 R >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF"] {
        if let Ok(d) = pdfread::open(junk) {
            let _ = pdfread::pages(&d);
            let _ = pdfread::render_ink(&d, 0, 1.0);
            let _ = pdfread::page_linework(&d, 0, 1000);
            let _ = embed::read_embedded(&d);
        }
    }
}

#[test]
fn auto_count_stamp_sheet() {
    let (bytes, centres) = stamp_sheet();
    let doc = pdfread::open(&bytes).unwrap();
    let f = centres[0];
    let run = pdfread::search_sheet(&doc, 0, [f.0 - 17.0, f.1 - 11.0, f.0 + 17.0, f.1 + 11.0], &RIGHT, 0, 1.0).unwrap();
    let at75 = kept(&run, 0.75);
    let (_, tp) = score(&at75, &centres, 6.0);
    assert_eq!(tp, 16, "all 16 stamps found at 75%");
    let extra: Vec<_> = at75.iter().filter(|(_, (x, y))| !centres.iter().any(|t| (t.0 - x).hypot(t.1 - y) < 6.0)).collect();
    assert!(extra.iter().all(|(_, (_, y))| (y - 569.0).abs() < 3.0), "the only extras are the near-duplicates: {extra:?}");
    let at84 = kept(&run, 0.84);
    assert_eq!(score(&at84, &centres, 6.0), (16, 16), "84%: precision 1, recall 1");
    let mut rots: Vec<usize> = RIGHT.iter().map(|r| at84.iter().filter(|(c, _)| c.rot == *r).count()).collect();
    rots.sort_unstable();
    assert_eq!(rots, vec![4, 4, 4, 4]);
}

#[test]
fn auto_count_any_angle_and_scales() {
    let placed = [(100.0, 400.0, 0.0), (300.0, 420.0, 37.0), (520.0, 380.0, 123.0), (400.0, 200.0, 200.0)];
    let content: String = placed.iter().map(|(x, y, r)| symbol(*x, *y, *r, 1.0, 1.2, true)).collect();
    let want: Vec<(f64, f64)> = placed.iter().map(|(x, y, r)| symbol_centre(*x, *y, *r, 1.0)).collect();
    let doc = pdfread::open(&pdf(&[Page::new(792.0, 612.0, content)], "", &[])).unwrap();
    let bx = [98.0, 398.0, 132.0, 420.0];
    let right = pdfread::search_sheet(&doc, 0, bx, &RIGHT, 0, 1.0).unwrap();
    assert_eq!(score(&kept(&right, 0.75), &want, 6.0).1, 1, "right angles only: the turned stamps are not found");
    let any = pdfread::search_sheet(&doc, 0, bx, &mt::any_angles(), 0, 1.0).unwrap();
    let ka = kept(&any, 0.75);
    assert_eq!(
        score(&ka, &want, 6.0),
        (4, 4),
        "any angle: all four, nothing else: {:?}",
        ka.iter().map(|(c, p)| (c.rot, c.score, *p)).collect::<Vec<_>>()
    );
    let r1 = cc_takeoff::matcher::Img { data: vec![1.0, 0.0], w: 2, h: 1 };
    assert_eq!(mt::rotate(&r1, 90.0).data, mt::rot90(&r1).data);
    let r45 = mt::rotate(&cc_takeoff::matcher::Img { data: vec![1.0; 100], w: 10, h: 10 }, 45.0);
    assert!(r45.w == 15 && r45.h == 15);
    // page 2 at half the scale
    let p1 = Page::new(612.0, 792.0, symbol(200.0, 400.0, 0.0, 1.0, 1.2, true));
    let p2 = Page::new(612.0, 792.0, symbol(300.0, 300.0, 0.0, 0.5, 1.2, true) + &symbol(100.0, 500.0, 0.0, 0.5, 1.2, true));
    let two = pdfread::open(&pdf(&[p1, p2], "", &[])).unwrap();
    let b1 = [198.0, 398.0, 232.0, 420.0];
    let same = pdfread::search_sheet(&two, 1, b1, &RIGHT, 0, 1.0).unwrap();
    assert!(kept(&same, 0.75).is_empty());
    let scaled = pdfread::search_sheet(&two, 1, b1, &RIGHT, 0, 2.0).unwrap();
    assert_eq!(score(&kept(&scaled, 0.75), &[(307.5, 304.5), (107.5, 504.5)], 4.0), (2, 2));
}

#[test]
fn takeoff_inside_the_pdf() {
    let bytes = pdf(&[Page::new(612.0, 792.0, "2 w 100 100 200 100 re S"), Page::new(792.0, 612.0, "")], "", &[("notes.txt", b"hello")]);
    let hash = "ab".repeat(32);
    let mut t = Takeoff::default();
    t.add_file(&o(json!({ "hash": hash, "name": "set.pdf", "size": bytes.len(), "pages": 2 })));
    let it = item(&mut t, json!({ "name": "Wall", "type": "linear" }));
    mark(&mut t, json!({ "item": it, "sheet": model::sheet_key(&hash, 1.0), "pts": [{ "x": 0, "y": 0 }, { "x": 72, "y": 0 }] }));
    let once = embed::embed_takeoff(&bytes, &t.to_json("T"), &hash, 0.0).unwrap();
    assert!(once.starts_with(&bytes), "an incremental update: the original bytes come first, untouched");
    let d1 = pdfread::open(&once).unwrap();
    let (sf, tk) = embed::read_embedded(&d1).unwrap();
    assert!(sf == hash && tk["marks"].as_array().unwrap().len() == 1 && model::read_project(&tk).is_ok());
    let ps = pdfread::pages(&d1);
    assert!(ps.len() == 2 && ps[1].view == [0.0, 0.0, 792.0, 612.0]);
    assert_eq!(embed::attachments(&d1).len(), 2);
    mark(&mut t, json!({ "item": it, "sheet": model::sheet_key(&hash, 0.0), "pts": [{ "x": 0, "y": 0 }, { "x": 36, "y": 0 }] }));
    let twice = embed::embed_takeoff(&once, &t.to_json("T"), &hash, 0.0).unwrap();
    let d2 = pdfread::open(&twice).unwrap();
    let mut names: Vec<String> = embed::attachments(&d2).into_iter().map(|(n, _)| n).collect();
    names.sort();
    assert_eq!(names.join(","), "notes.txt,takeoff.takeoffcraft.json");
    assert_eq!(embed::read_embedded(&d2).unwrap().1["marks"].as_array().unwrap().len(), 2);
    assert!(embed::read_embedded(&pdfread::open(&bytes).unwrap()).is_none());
    let bad = pdf(
        &[Page::new(612.0, 792.0, "")],
        "",
        &[(embed::EMBED_NAME, br#"{"format":"takeoffcraft-embedded","standsFor":"nothex","takeoff":{}}"#)],
    );
    assert!(embed::read_embedded(&pdfread::open(&bad).unwrap()).is_none());
}

fn fixture(var: &str, default: &str) -> Option<(PathBuf, Vec<u8>)> {
    let p = std::env::var_os(var).map_or_else(|| PathBuf::from(default), PathBuf::from);
    let b = std::fs::read(&p).ok()?;
    Some((p, b))
}
fn kitchen() -> Option<(PathBuf, Vec<u8>)> {
    fixture("TAKEOFF_KITCHEN_PDF", "/var/www/CADCRAFT/getcadcraft.com/fixtures/pdf/kitchen-plan-sheet.pdf")
}
fn has_seg(lw: &cc_takeoff::snap::Linework, a: (f64, f64), b: (f64, f64)) -> bool {
    let n2 = |x: f64, y: f64, p: (f64, f64)| (x - p.0).abs() < 0.01 && (y - p.1).abs() < 0.01;
    lw.segs.chunks(4).take(lw.n).any(|s| (n2(s[0], s[1], a) && n2(s[2], s[3], b)) || (n2(s[0], s[1], b) && n2(s[2], s[3], a)))
}
fn curve_starts(doc: &Doc) -> usize {
    let lw = pdfread::page_linework(doc, 0, cc_takeoff::snap::MAX_SEGS).unwrap();
    lw.curve.iter().take(lw.n).filter(|f| **f & 2 != 0).count()
}

#[test]
fn kitchen_plan_sheet() {
    let Some((path, bytes)) = kitchen() else {
        eprintln!("kitchen fixture not found: skipped");
        return;
    };
    let doc = pdfread::open(&bytes).unwrap();
    let ps = pdfread::pages(&doc);
    assert_eq!(ps.len(), 1);
    assert!(ps[0].view == [0.0, 0.0, 1296.0, 1728.0] && ps[0].user_unit == 1.0 && ps[0].rotate == 0.0);
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(sheet_title(&name, &json!({ "index": 0, "number": ps[0].number }), false), "kitchen-plan-sheet, page 1");
    let lw = pdfread::page_linework(&doc, 0, cc_takeoff::snap::MAX_SEGS).unwrap();
    assert!(
        has_seg(&lw, (930.48, 1157.28), (930.48, 1328.28))
            && has_seg(&lw, (1220.76, 1157.28), (930.48, 1157.28))
            && has_seg(&lw, (924.48, 1334.28), (924.48, 1151.28))
    );
    assert!(lw.n > 5000 && !lw.truncated);
    let idx = SnapIndex::new(&lw);
    let c1 = idx.snap(931.6, 1158.3, 3.0, Kinds::default()).unwrap();
    let c2 = idx.snap(929.7, 1327.1, 3.0, Kinds::default()).unwrap();
    assert!(c1.kind == "end" && near(c1.x, 930.48, 1e-6) && near(c1.y, 1157.28, 1e-6), "{c1:?}");
    assert!(c2.kind == "end" && near(c2.x, 930.48, 1e-6) && near(c2.y, 1328.28, 1e-6), "{c2:?}");
    let ft = u::parse_length("9'-6\"", "ft").unwrap();
    let (snap_fpu, _) = u::calibrate(c1.x, c1.y, c2.x, c2.y, ft).unwrap();
    assert!(near(snap_fpu, 1.0 / 18.0, 1e-6));
    let mid = idx.snap(1076.0, 1158.0, 3.0, Kinds::default()).unwrap();
    assert!(mid.kind == "mid" && near(mid.x, (930.48 + 1220.76) / 2.0, 1e-6));
    let (fpu, _) = u::calibrate(930.48, 1157.28, 930.48, 1328.28, ft).unwrap();
    assert!(near(fpu, u::preset_ft_per_unit("a1/4", 1.0).unwrap(), 1e-12));
    assert_eq!(u::match_preset(fpu, 1.0, 0.002).map(|(p, _)| p.label), Some("1/4\" = 1'-0\""));
    assert_eq!(u::format_ft_in((930.48 - 924.48) * fpu, 16.0), "0'-4\"");
    assert_eq!(u::format_ft_in((1220.76 - 930.48) * fpu, 16.0), "16'-1 1/2\"");
    assert_eq!(u::format_ft_in((1334.28 - 1151.28) * fpu, 16.0), "10'-2\"");
    let inside = [P::new(930.48, 1157.28), P::new(1220.76, 1157.28), P::new(1220.76, 1328.28), P::new(930.48, 1328.28)];
    let outside = [P::new(924.48, 1151.28), P::new(1226.76, 1151.28), P::new(1226.76, 1334.28), P::new(924.48, 1334.28)];
    assert!(near(geom::polygon_area(&inside) * fpu * fpu, 153.21, 1e-3) && near(geom::polygon_area(&outside) * fpu * fpu, 170.73, 1e-3));
    // a takeoff on it
    let mut t = Takeoff::default();
    t.add_file(&o(json!({ "hash": doc.hash, "name": name, "size": doc.size, "pages": 1 })));
    let sk = model::sheet_key(&doc.hash, 0.0);
    t.set_scale(&sk, &json!({ "method": "calibrated", "preset": null, "ftPerUnit": fpu, "label": u::describe_scale(fpu, 1.0), "cal": { "a": [930.48, 1157.28], "b": [930.48, 1328.28], "ft": 9.5, "text": "9'-6\"" } }), None).unwrap();
    let pj = |ps: &[P]| Value::Array(ps.iter().map(|p| json!({ "x": p.x, "y": p.y })).collect());
    let panel = item(&mut t, json!({ "name": "Walk-in panel (freezer)", "type": "linear", "heightFt": 9, "mat": rate("48") }));
    let mut run = outside.to_vec();
    run.push(outside[0]);
    mark(&mut t, json!({ "item": panel, "sheet": sk, "pts": pj(&run) }));
    t.add_component(&panel, &o(json!({ "name": "Panels 4' wide", "unit": "EA", "formula": "ceil(L / 4)" }))).unwrap();
    let fl = item(&mut t, json!({ "name": "Freezer floor", "type": "area", "mat": rate("12.5"), "waste": 500 }));
    mark(&mut t, json!({ "item": fl, "sheet": sk, "pts": pj(&inside), "holes": [rect(1000.0, 1200.0, 18.0, 18.0)] }));
    let doors = item(&mut t, json!({ "name": "Doors", "type": "count", "mat": rate("850") }));
    mark(&mut t, json!({ "item": doors, "sheet": sk, "pts": [{ "x": 300, "y": 1500 }] }));
    mark(&mut t, json!({ "item": doors, "sheet": sk, "pts": [{ "x": 600, "y": 1300 }] }));
    let q = model::quantities(&t.state, &QOpts::default());
    assert!(near(q.get(&panel).unwrap().tot.length_ft, 2.0 * (302.28 + 183.0) / 18.0, 1e-9));
    assert!(near(q.get(&fl).unwrap().tot.area_sqft, (290.28 * 171.0 - 18.0 * 18.0) / 324.0, 1e-9));
    assert_eq!(q.get(&doors).unwrap().tot.count, 2.0);
    let mut re = Takeoff::default();
    re.load(&serde_json::from_str(&jv::stringify(&t.to_json("T"))).unwrap()).unwrap();
    let m = match_files(re.state["files"].as_array().unwrap(), &[json!({ "hash": doc.hash, "name": name, "size": doc.size })]);
    assert!(
        m["matched"].get(&doc.hash).is_some() && m["missing"].as_array().unwrap().is_empty() && m["changed"].as_array().unwrap().is_empty()
    );
    assert_eq!(jv::stringify(&model::estimate_all(&re.state).to_json()), jv::stringify(&model::estimate_all(&t.state).to_json()));
    let miss = match_files(re.state["files"].as_array().unwrap(), &[json!({ "hash": "f".repeat(64), "name": name, "size": 5 })]);
    assert!(miss["changed"].as_array().unwrap().len() == 1 && miss["matched"].as_object().unwrap().is_empty());
    // the marked PDF copy
    let out = pdf_with_marks(&bytes, &doc.hash, &t.state, true).unwrap();
    assert!(out.pages == 1 && out.marks == 4);
    let rd = pdfread::open(&out.bytes).unwrap();
    let rp = pdfread::pages(&rd);
    assert!(rp.len() == 1 && rp[0].view == [0.0, 0.0, 1296.0, 1728.0]);
    let lw2 = pdfread::page_linework(&rd, 0, cc_takeoff::snap::MAX_SEGS).unwrap();
    assert!(lw2.n > lw.n, "{} → {}", lw.n, lw2.n);
    assert!(has_seg(&lw2, (924.48, 1151.28), (1226.76, 1151.28)), "the panel run at its own coordinates");
    // arcs in the marked copy are curves: a circle = 4 Béziers for the fill + 4 for the outline
    let mut at = Takeoff::default();
    at.add_file(&o(json!({ "hash": doc.hash, "name": name, "size": doc.size, "pages": 1 })));
    at.set_scale(&sk, &json!({ "method": "preset", "preset": "a1/4", "ftPerUnit": 1.0 / 18.0, "label": "1/4\"", "cal": null }), None)
        .unwrap();
    let tbl = item(&mut at, json!({ "name": "Round table", "type": "area" }));
    mark(&mut at, json!({ "item": tbl, "sheet": sk, "pts": [{ "x": 1060, "y": 1240, "b": 1 }, { "x": 1000, "y": 1240, "b": 1 }] }));
    let arc = pdf_with_marks(&bytes, &doc.hash, &at.state, true).unwrap();
    assert_eq!(curve_starts(&pdfread::open(&arc.bytes).unwrap()) - curve_starts(&doc), 8);
    // automatic counting: 22 shelving units counted by hand
    let shelves = [
        (1062.2, 1006.5),
        (1138.7, 1006.5),
        (1062.2, 1130.45),
        (1138.7, 1130.45),
        (1066.7, 1177.0),
        (1145.5, 1177.0),
        (1007.6, 1309.0),
        (1073.6, 1309.0),
        (1147.2, 1309.0),
        (835.2, 1320.75),
        (1198.7, 1018.2),
        (1198.7, 1094.8),
        (750.8, 1068.2),
        (750.8, 1145.9),
        (750.8, 1222.5),
        (750.9, 1295.5),
        (892.3, 1145.9),
        (892.3, 1222.5),
        (892.5, 1295.5),
        (1202.1, 1197.0),
        (1202.1, 1273.5),
        (950.0, 1288.0),
    ];
    let srun = pdfread::search_sheet(&doc, 0, [731.6, 1108.7, 770.0, 1183.1], &RIGHT, 0, 1.0).unwrap();
    for th in [0.6, 0.65, 0.7, 0.75] {
        assert_eq!(score(&kept(&srun, th), &shelves, 12.0), (22, 22), "shelving at {th}");
    }
    let (f90, tp90) = score(&kept(&srun, 0.9), &shelves, 12.0);
    assert!(f90 == tp90 && tp90 < 22, "90%: precise, misses some ({tp90} of 22)");
}

#[test]
fn carey_auto_count() {
    let Some((_, bytes)) = fixture("TAKEOFF_CAREY_PDF", "") else {
        eprintln!("Carey set not found: skipped");
        return;
    };
    let doc = pdfread::open(&bytes).unwrap();
    assert_eq!(pdfread::pages(&doc)[0].view, [0.0, 0.0, 1296.0, 1728.0]);
    let run = pdfread::search_sheet(&doc, 0, [731.6, 1108.7, 770.0, 1183.1], &RIGHT, 0, 1.0).unwrap();
    // the same 22 shelving units the JS keeps at the default 75% (forks/tools/todiff.mjs matches them one to one)
    assert_eq!(kept(&run, 0.75).len(), 22);
}
