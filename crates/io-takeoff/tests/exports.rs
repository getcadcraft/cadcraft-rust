//! Ported from the frozen Node test (bin/test-takeoffcraft.mjs at 594ab3e): "CSV", "XLSX" (unzipped and checked here;
//! the JS also re-calculates with openpyxl / LibreOffice, which stay JS-side oracles), the estimate-CSV round trip of
//! "Price library from CSV (T9)", "Bid summary and proposal PDF (T19)", "Reports (TK.4)", "Earthwork hand-off (TK.5)" and
//! the workbook half of "Markups (T22)". Text in written PDFs is read back with poppler's pdftotext when it is
//! installed (an oracle run as a program), else those checks are skipped and said.

mod common;
use common::*;

use cc_io_takeoff::export as e;
use cc_io_takeoff::xlsx::{self, CellV, Sheet, Stamp, XCell};
use cc_takeoff::bid;
use cc_takeoff::clipboard::{self, CopyCtx};
use cc_takeoff::jv;
use cc_takeoff::library::{self as lb, Cell};
use cc_takeoff::model::{self, Takeoff};
use serde_json::{Value, json};

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * 1f64.max(a.abs()).max(b.abs())
}
fn label(k: &str) -> String {
    format!("Sheet {}", k.rsplit(':').next().and_then(|n| n.parse::<f64>().ok()).unwrap_or(0.0) + 1.0)
}
fn pdftotext(bytes: &[u8], layout: bool) -> Option<String> {
    let dir = std::env::temp_dir().join(format!("tc-io-{}-{}", std::process::id(), bytes.len()));
    std::fs::write(&dir, bytes).ok()?;
    let mut cmd = std::process::Command::new("pdftotext");
    if layout {
        cmd.arg("-layout");
    }
    let out = cmd.arg(&dir).arg("-").output().ok();
    let _ = std::fs::remove_file(&dir);
    let out = out?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}
fn stamp() -> Stamp {
    Stamp::from_ms(1_791_892_800_000.0)
}

#[test]
fn csv_cells_and_estimate_csv() {
    let c = |s: &str| lb::csv_cell(&Cell::Str(s.into()));
    assert_eq!(c("=HYPERLINK(\"x\")"), "\"'=HYPERLINK(\"\"x\"\")\"");
    assert_eq!(c("+1"), "'+1");
    assert_eq!(c("-1"), "'-1");
    assert_eq!(c("@SUM"), "'@SUM");
    assert_eq!(lb::csv_cell(&Cell::Num(-1.0)), "-1");
    assert_eq!(c("a,b"), "\"a,b\"");
    assert_eq!(c("a\nb"), "\"a\nb\"");
    let t = kitchen_style();
    let est = model::base_estimate(&t.state);
    let csv = e::estimate_csv(&t.state, &est);
    let lines: Vec<&str> = csv.trim().split("\r\n").collect();
    assert_eq!(lines[0], e::estimate_head().join(","));
    assert_eq!(lines.len(), 1 + est.rows.len() + 1);
    assert!(lines[lines.len() - 1].starts_with("Job total,") && lines[lines.len() - 1].contains(",1597.87,"), "{}", lines[lines.len() - 1]);
    assert!(lines.iter().any(|l| l.contains("Formula error: The formula divides by zero.")));
    // the estimate CSV reads back as a price library (T9)
    let (entries, _) = lb::read_library_csv(&csv).unwrap();
    assert_eq!(entries[0]["components"][0]["formula"], "ceil(L / 1.333) + N");
    let mut t2 = Takeoff::default();
    let (lib, w) = lb::read_library_csv(
        "Name,Type,UOM,Waste,Material,Labor,Height,Component,Formula\nPartition type A,Linear,LF,10%,$2.10,3.333,9'-0\",,\n,Component,EA,,4.25,,,Studs 16 o.c.,ceil(L / 1.333) + N\n,Component,sheets,10,14.98,22,,Gypsum board 4x8,ceil(A * 2 / 32)\nVCT flooring,,SF,5,1.85,1.10,,,\nCarpet,area,SY,,27,,,,\nDuplex outlets,Count,EA,,3.50,45,,,\nWall in metres,Linear,m,,1,,2.7,,",
    )
    .unwrap();
    assert!(w.is_empty());
    t2.add_items(&lib.iter().map(lb::item_from_entry).collect::<Vec<_>>(), None).unwrap();
    let (ee, ew) = lb::read_library_csv(&e::estimate_csv(&t2.state, &model::base_estimate(&t2.state))).unwrap();
    assert!(
        ee.len() == 5 && ee[0]["components"][0]["formula"] == "ceil(L / 1.333) + N" && ee[0]["mat"] == 21000.0 && ee[2]["unit"] == "SY",
        "{ew:?}"
    );
}

fn sheet_xml(entries: &[(String, Vec<u8>)], i: usize) -> &str {
    let name = format!("xl/worksheets/sheet{i}.xml");
    std::str::from_utf8(&entries.iter().find(|(n, _)| *n == name).unwrap().1).unwrap()
}

#[test]
fn xlsx_workbook() {
    let t = kitchen_style();
    let est = model::base_estimate(&t.state);
    let mut both = Vec::new();
    for compress in [true, false] {
        let bytes = e::estimate_xlsx(&t.state, &label, compress, &stamp(), &stamp());
        let entries = unzip(&bytes);
        let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            &names[..7],
            &[
                "[Content_Types].xml",
                "_rels/.rels",
                "docProps/core.xml",
                "docProps/app.xml",
                "xl/workbook.xml",
                "xl/_rels/workbook.xml.rels",
                "xl/styles.xml"
            ]
        );
        let wb = std::str::from_utf8(&entries[4].1).unwrap();
        let sheet_names: Vec<&str> = wb.split("<sheet name=\"").skip(1).map(|s| s.split('"').next().unwrap()).collect();
        assert_eq!(&sheet_names[..4], &["Estimate", "By sheet", "Marks", "Scales"]);
        assert!(
            sheet_names[4..].iter().all(|n| ["Takeoff summary", "Material list", "Quote request", "By division"].contains(n)),
            "{sheet_names:?}"
        );
        let s1 = sheet_xml(&entries, 1);
        assert!(
            s1.contains("<c r=\"A1\" s=\"1\" t=\"inlineStr\"><is><t xml:space=\"preserve\">Item</t></is></c>") || s1.contains(">Item</t>")
        );
        let total_row = est.rows.len() + 3;
        assert!(s1.contains(&format!("<c r=\"K{total_row}\"")), "total row");
        assert!(s1.contains(&format!("<f>SUM(K2:K{})</f><v>1597.87</v>", est.rows.len() + 1)), "job total SUM with cached 1597.87");
        assert!(
            s1.contains("<c r=\"A2\" t=\"inlineStr\"><is><t xml:space=\"preserve\">Partition type A</t></is></c>")
                || s1.contains(">Partition type A</t>")
        );
        assert!(s1.contains(">Formula: "), "component notes carry the formula");
        assert!(sheet_xml(&entries, 3).contains("segment"), "marks sheet lists segments");
        assert!(
            sheet_xml(&entries, 4).contains("<c r=\"B2\" t=\"inlineStr\"><is><t xml:space=\"preserve\">1/4&quot; = 1'-0&quot;</t>"),
            "scales sheet names the preset"
        );
        // `unzip -t` (Info-ZIP, an independent reader) when installed
        if let Ok(out) = std::process::Command::new("unzip").arg("-v").output()
            && out.status.success()
        {
            let f = std::env::temp_dir().join(format!("tc-xlsx-{}-{compress}.xlsx", std::process::id()));
            std::fs::write(&f, &bytes).unwrap();
            let r = std::process::Command::new("unzip").arg("-t").arg(&f).output().unwrap();
            let _ = std::fs::remove_file(&f);
            assert!(String::from_utf8_lossy(&r.stdout).contains("No errors detected"));
        }
        both.push(entries);
    }
    assert_eq!(both[0], both[1], "deflated and stored hold the same entries");
    // names that would break XML or sheet names
    let odd = xlsx::write_xlsx(
        &[
            Sheet {
                name: "a/b:c*?[x]".into(),
                rows: vec![vec![
                    XCell::Str("<&>\"'".into()),
                    XCell::Obj { v: CellV::Str("x\u{1}y".into()), f: None, s: None },
                    XCell::Num(1.5),
                    XCell::Empty,
                    XCell::Num(-0.0),
                ]],
                cols: vec![],
                freeze: 0,
            },
            Sheet { name: "a/b:c*?[x]".into(), rows: vec![vec![XCell::Num(1.0)]], cols: vec![], freeze: 0 },
        ],
        "Takeoff",
        true,
        &stamp(),
        &stamp(),
    );
    let oe = unzip(&odd);
    let wb = std::str::from_utf8(&oe[4].1).unwrap();
    assert!(wb.contains("<sheet name=\"a b c   x\" ") && wb.contains("<sheet name=\"a b c   x 2\" "), "{wb}");
    let s1 = sheet_xml(&oe, 1);
    assert!(s1.contains(">&lt;&amp;&gt;&quot;'</t>"), "{s1}");
    assert!(s1.contains(">xy</t>"), "control characters dropped");
    assert_eq!(xlsx::crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(
        format!("{}{}{}{}{}", xlsx::col_name(0), xlsx::col_name(25), xlsx::col_name(26), xlsx::col_name(701), xlsx::col_name(702)),
        "AZAAZZAAA"
    );
}

#[test]
fn bid_summary_and_proposal() {
    let mut t = Takeoff::default();
    let hash = "9".repeat(64);
    t.add_file(&o(json!({ "hash": hash, "name": "bid.pdf", "size": 1, "pages": 1 })));
    let s0 = model::sheet_key(&hash, 0.0);
    t.set_scale(&s0, &preset("a1/4", "1/4\""), None).unwrap();
    let wall = item(&mut t, json!({ "name": "Partition", "type": "linear", "mat": rate("10"), "lab": rate("5") }));
    mark(&mut t, json!({ "item": wall, "sheet": s0, "pts": [{ "x": 0, "y": 0 }, { "x": 1800, "y": 0 }] }));
    let alt = t.add_area(Some(&json!("Alt 1 more wall"))).unwrap().unwrap();
    t.set_area_alt(&alt, Some("add")).unwrap();
    mark(&mut t, json!({ "item": wall, "sheet": s0, "pts": [{ "x": 0, "y": 100 }, { "x": 180, "y": 100 }], "area": alt }));
    t.set_bid(&o(json!({ "tax": 825, "overhead": 1000, "profit": 1000, "bond": 150, "to": "Owner LLC\n1 Main St", "by": "ACME Drywall", "incl": "- Taping and finishing\n- Cleanup", "excl": "Paint\nPermits", "terms": "Valid for 30 days." })), None).unwrap();
    let est = model::base_estimate(&t.state);
    assert!(bid::bid_summary(est.mat_total, est.lab_total, false, Some(&json!({})), &[]).lines.len() == 4);
    assert_eq!(bid::bid_summary(est.mat_total, est.lab_total, false, Some(&json!({})), &[]).total, 150000.0);
    let back = model::read_project(&t.to_json("T")).unwrap();
    assert!(back.state["bid"]["overhead"] == 1000.0 && back.state["bid"]["excl"] == "Paint\nPermits");
    let csv = e::estimate_csv(&t.state, &model::base_estimate(&t.state));
    let ls: Vec<&str> = csv.split("\r\n").collect();
    assert!(
        ls.iter().any(|l| l.starts_with("Bid summary"))
            && ls.iter().any(|l| l.starts_with("Overhead (10%)") || l.starts_with("Overhead,10%")),
        "{csv}"
    );
    assert!(ls.iter().any(|l| l.starts_with("Bid,") && l.ends_with(",1943.55,")), "{csv}");
    let bs = e::estimate_sheets(&t.state, &|k: &str| k.to_string(), &model::base_estimate(&t.state));
    let bsh = bs.iter().find(|s| s.name == "Bid summary").unwrap();
    assert!(jv::stringify(&Value::Array(bsh.rows.iter().flat_map(|r| r.iter().map(XCell::to_json)).collect())).contains("1943.55"));
    let pdf = cc_io_takeoff::proposal::proposal_pdf(&t.state, 1_791_892_800_000.0);
    let doc = cc_io_takeoff::pdfread::open(&pdf).unwrap();
    assert!(!cc_io_takeoff::pdfread::pages(&doc).is_empty());
    match pdftotext(&pdf, true) {
        None => eprintln!("pdftotext not installed: the proposal's text checks are skipped"),
        Some(txt) => {
            assert!(txt.contains("Proposal") && txt.contains("Owner LLC") && txt.contains("ACME Drywall") && txt.contains("2026-10-13"));
            let squash = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
            let flat = squash(&txt);
            assert!(flat.contains("Overhead (10 %) $158.25") && flat.contains("Bid $1,943.55"), "{flat}");
            assert!(flat.contains("Alt 1 more wall (add alternate) +$194."), "{flat}");
            let scope = &txt[txt.find("Scope").unwrap()..txt.find("Inclusions").unwrap()];
            assert!(squash(scope).starts_with("Scope Partition 100 LF") && !scope.contains('$'), "{scope}");
            assert!(
                txt.contains("Taping and finishing")
                    && flat.contains("• Paint")
                    && !flat.contains("- Taping")
                    && txt.contains("Valid for 30 days")
                    && txt.contains("accepted")
            );
        }
    }
}

#[test]
fn reports_tk4() {
    let mut t = Takeoff::default();
    let hash = "8".repeat(64);
    t.add_file(&o(json!({ "hash": hash, "name": "gc.pdf", "size": 1, "pages": 1 })));
    let s0 = model::sheet_key(&hash, 0.0);
    t.set_scale(&s0, &preset("a1/4", "1/4\""), None).unwrap();
    let ids = t
        .add_items(
            &[
                json!({ "name": "Wall A", "type": "linear", "unit": "LF", "heightFt": 10, "csi": "09 21 16", "lab": rate("5"), "components": [
                    { "name": "Gypsum board 4 x 8", "unit": "sheet", "formula": "ceil(A*2/32)", "waste": 1000, "mat": rate("12") }, { "name": "Drywall screws", "unit": "EA", "formula": "ceil(A*2*0.875)", "mat": rate("0.02") }] }),
                json!({ "name": "Wall B", "type": "linear", "unit": "LF", "heightFt": 8, "csi": "09 21 16", "components": [
                    { "name": "Gypsum board 4 x 8", "unit": "sheet", "formula": "ceil(A*2/32)", "waste": 1000, "mat": rate("12") }, { "name": "drywall screws ", "unit": "EA", "formula": "ceil(A*2*0.875)", "mat": rate("0.02") }] }),
                json!({ "name": "Ceiling", "type": "area", "unit": "SF", "csi": "09 21 16", "components": [{ "name": "Drywall screws", "unit": "EA", "formula": "ceil(A*1.094)", "mat": rate("0.02") }] }),
                json!({ "name": "Door", "type": "count", "unit": "EA", "mat": rate("250"), "lab": rate("80") }),
            ],
            None,
        )
        .unwrap();
    let (wa, wb, cl, dr) = (&ids[0], &ids[1], &ids[2], &ids[3]);
    mark(&mut t, json!({ "item": wa, "sheet": s0, "pts": [{ "x": 0, "y": 0 }, { "x": 720, "y": 0 }] }));
    mark(&mut t, json!({ "item": wb, "sheet": s0, "pts": [{ "x": 0, "y": 100 }, { "x": 360, "y": 100 }] }));
    mark(
        &mut t,
        json!({ "item": cl, "sheet": s0, "pts": [{ "x": 0, "y": 200 }, { "x": 360, "y": 200 }, { "x": 360, "y": 560 }, { "x": 0, "y": 560 }] }),
    );
    mark(&mut t, json!({ "item": dr, "sheet": s0, "pts": [{ "x": 5, "y": 5 }] }));
    mark(&mut t, json!({ "item": dr, "sheet": s0, "pts": [{ "x": 9, "y": 5 }] }));
    let est = model::base_estimate(&t.state);
    let ml = e::material_list(&t.state, &est);
    let board = ml.iter().find(|g| g.name == "Gypsum board 4 x 8").unwrap();
    let screws = ml.iter().find(|g| g.name.to_lowercase() == "drywall screws").unwrap();
    let door = ml.iter().find(|g| g.name == "Door").unwrap();
    assert!(
        ml.len() == 3
            && board.qty == 35.0
            && near(board.qty_w, 38.5)
            && board.mat_ext == 46200.0
            && board.items.join(",") == "Wall A,Wall B"
            && board.csi == "09 21 16"
    );
    assert!(screws.qty == 1418.0 && screws.mat_ext == 2836.0 && screws.items.len() == 3);
    assert!(door.qty == 2.0 && door.mat_ext == 50000.0 && ml.last().unwrap().name == "Door");
    let mcsv = e::material_csv(&t.state, &est);
    assert!(mcsv.starts_with("Part,CSI,Unit,Quantity,Qty with waste,Material,Used by"));
    assert!(mcsv.split("\r\n").any(|l| l.starts_with("Gypsum board 4 x 8,09 21 16,sheet,35,38.5,462,Wall A; Wall B")), "{mcsv}");
    assert!(mcsv.split("\r\n").any(|l| l.starts_with("Job total,,,,,990.36,")), "{mcsv}");
    let ts: Vec<String> = e::takeoff_summary(&t.state, &est)
        .iter()
        .map(|x| format!("{}:{}:{}:{}:{}:{}", x.name, x.csi, jv::num_str(x.qty), x.unit, jv::num_str(x.marks), jv::num_str(x.sheets)))
        .collect();
    assert_eq!(ts.join(" | "), "Wall A:09 21 16:40:LF:1:1 | Wall B:09 21 16:20:LF:1:1 | Ceiling:09 21 16:400:SF:1:1 | Door::2:EA:2:1");
    let bd = e::by_division(&t.state, &est);
    assert!(
        bd.len() == 2
            && bd[0].div == "09"
            && bd[0].title == "Finishes"
            && bd[0].items == 3.0
            && bd[0].mat == 49036.0
            && bd[0].lab == 20000.0
            && bd[1].div.is_empty()
            && bd[1].title == "No CSI number"
            && bd[1].total == 66000.0
    );
    assert_eq!(bd.iter().map(|g| g.total).sum::<f64>(), est.total);
    t.set_bid(&o(json!({ "burden": 3000, "tax": 825, "overhead": 1000, "profit": 1000, "bond": 150 })), None).unwrap();
    let sum = bid::bid_summary(est.mat_total, est.lab_total, est.incomplete, Some(&t.bid()), &[]);
    let s: Vec<String> = sum.lines.iter().map(|l| format!("{}:{}", l.key, jv::num_str(l.cents))).collect();
    assert_eq!(s.join(" "), "mat:99036 lab:36000 burden:10800 cost:145836 tax:8170 overhead:15401 profit:16941 bond:2795 total:189143");
    assert!(
        bid::bid_summary(est.mat_total, est.lab_total, false, Some(&json!({ "tax": 825 })), &[]).lines.iter().all(|l| l.key != "burden")
    );
    assert_eq!(bid::normal_bid(Some(&json!({ "burden": -5 })))["burden"], 0.0);
    assert_eq!(model::read_project(&t.to_json("T")).unwrap().state["bid"]["burden"], 3000.0);
    let sheets = e::estimate_sheets(&t.state, &|_: &str| "Sheet 1".to_string(), &model::base_estimate(&t.state));
    assert_eq!(
        sheets.iter().map(|x| x.name.as_str()).collect::<Vec<_>>().join("|"),
        "Estimate|By sheet|Marks|Scales|Bid summary|Takeoff summary|Material list|Quote request|By division"
    );
    let rows = &sheets[0].rows;
    let bid_row = rows
        .iter()
        .position(|r| {
            matches!(r.first(), Some(XCell::Str(s)) if s == "Bid") || r.first().map(XCell::to_json).is_some_and(|v| v["v"] == "Bid")
        })
        .unwrap();
    let k = rows[bid_row][10].to_json();
    let f = jv::to_string(k.get("f"));
    assert!(
        k["v"] == 1891.43
            && f.starts_with('K')
            && f.split('+').all(|p| p.starts_with('K') && p[1..].chars().all(|c| c.is_ascii_digit()))
            && f.contains('+'),
        "{k}"
    );
    // the quote request sheet: empty unit prices, live extensions and total
    let entries = unzip(&e::estimate_xlsx(&t.state, &|_: &str| "Sheet 1".to_string(), false, &stamp(), &stamp()));
    let qr = sheet_xml(&entries, 8);
    assert!(
        qr.contains(">Quote request: Takeoff</t>")
            && qr.contains("<c r=\"D4\"")
            && qr.contains("<f>ROUND(C4*D4,2)</f>")
            && qr.contains("<f>SUM(E4:E6)</f>"),
        "{qr}"
    );
}

#[test]
fn spot_elevations_tk5() {
    let mut t = Takeoff::default();
    let hash = "7".repeat(64);
    t.add_file(&o(json!({ "hash": hash, "name": "site.pdf", "size": 1, "pages": 1 })));
    let s0 = model::sheet_key(&hash, 0.0);
    let spot = item(&mut t, json!({ "name": "Existing grade", "type": "count" }));
    let wall = item(&mut t, json!({ "name": "Wall", "type": "linear" }));
    t.set_scale(&s0, &json!({ "method": "preset", "preset": "e20", "ftPerUnit": 20.0 / 72.0, "label": "1\" = 20'", "cal": null }), None)
        .unwrap();
    for (x, y, z) in [(72.0, 144.0, Some(101.25)), (360.0, 144.0, Some(-2.0)), (216.0, 432.0, Some(0.0)), (500.0, 500.0, None)] {
        let mut m = json!({ "item": spot, "sheet": s0, "pts": [{ "x": x, "y": y }] });
        if let Some(z) = z {
            m["z"] = json!(z);
        }
        mark(&mut t, m);
    }
    mark(&mut t, json!({ "item": wall, "sheet": s0, "pts": [{ "x": 0, "y": 0 }, { "x": 72, "y": 0 }], "z": 5 }));
    let back = model::read_project(&t.to_json("T")).unwrap().state;
    let zs: Vec<String> =
        back["marks"].as_array().unwrap().iter().map(|m| m.get("z").and_then(Value::as_f64).map_or("-".into(), jv::num_str)).collect();
    assert_eq!(zs.join(","), "101.25,-2,0,-,-");
    let mut bad = t.to_json("T");
    bad["marks"][0]["z"] = json!("12");
    bad["marks"][1]["z"] = json!(1e9);
    let bs = model::read_project(&bad).unwrap().state;
    assert!(bs["marks"][0].get("z").is_none() && bs["marks"][1].get("z").is_none() && bs["marks"][2]["z"] == 0.0);
    let id0 = jv::to_string(t.marks()[0].get("id"));
    t.update_mark(&id0, &o(json!({ "z": 99.5 })), None).unwrap();
    assert_eq!(t.marks()[0]["z"], 99.5);
    t.undo();
    assert_eq!(t.marks()[0]["z"], 101.25);
    let (csv, n) = e::spot_elevations_csv(&t.state, &s0, &|_: &str| "C-101".to_string()).unwrap();
    assert_eq!(n, 3);
    assert_eq!(
        csv,
        "Point,Northing,Easting,Elevation,Description\r\n1,40,20,101.25,Existing grade\r\n2,40,100,-2,Existing grade\r\n3,120,60,0,Existing grade\r\n"
    );
    let mut t2 = Takeoff::default();
    t2.add_file(&o(json!({ "hash": hash, "name": "site.pdf", "size": 1, "pages": 1 })));
    let sp2 = item(&mut t2, json!({ "name": "Grade", "type": "count" }));
    mark(&mut t2, json!({ "item": sp2, "sheet": s0, "pts": [{ "x": 1, "y": 1 }], "z": 3 }));
    assert!(e::spot_elevations_csv(&t2.state, &s0, &|_: &str| "C-101".to_string()).unwrap_err().contains("has no scale"));
    assert!(
        e::spot_elevations_csv(&Takeoff::default().state, &s0, &|_: &str| String::new())
            .unwrap_err()
            .contains("no count marks with an elevation")
    );
    let clip = clipboard::copy_marks(
        &t.state,
        &[id0],
        &CopyCtx { view: Some(json!({ "width": 612, "height": 792 })), user_unit: Some(json!(1)), source: String::new(), now: 0.0 },
    )
    .unwrap();
    assert_eq!(clip["marks"][0]["z"], 101.25);
}

#[test]
fn markups_marked_pdf_and_sheet() {
    let bytes = pdf(&[Page::new(612.0, 792.0, "")], "", &[]);
    let hash = "cd".repeat(32);
    let key = model::sheet_key(&hash, 0.0);
    let mut t = Takeoff::default();
    t.add_file(&o(json!({ "hash": hash, "name": "plan.pdf", "size": bytes.len(), "pages": 1 })));
    t.add_note(&o(
        json!({ "kind": "note", "sheet": key, "pts": [{ "x": 100, "y": 700 }], "text": "Verify the door swing\nwith the owner" }),
    ))
    .unwrap();
    let c1 = t
        .add_note(&o(
            json!({ "kind": "cloud", "sheet": key, "pts": [{ "x": 300, "y": 300 }, { "x": 200, "y": 400 }], "text": "Rev 2: wall moved" }),
        ))
        .unwrap()
        .unwrap();
    let h1 = t
        .add_note(&o(json!({ "kind": "highlight", "sheet": key, "pts": [{ "x": 50, "y": 50 }, { "x": 150, "y": 80 }], "text": "" })))
        .unwrap()
        .unwrap();
    assert!(t.state["notes"].as_array().unwrap().iter().all(|n| n["status"] == "open"));
    t.update_note(&c1, &o(json!({ "status": "answered" })), None).unwrap();
    let mut j = t.to_json("T");
    assert!(j["notes"].as_array().unwrap().len() == 3 && j["notes"][0]["pts"][0].is_array());
    j["notes"].as_array_mut().unwrap().extend([
        json!({ "id": "n99", "sheet": key, "kind": "scribble", "pts": [[0, 0]] }),
        json!({ "id": "n98", "sheet": format!("{}:0", "f".repeat(64)), "kind": "note", "pts": [[0, 0]] }),
    ]);
    let back = model::read_project(&j).unwrap();
    assert!(
        back.state["notes"].as_array().unwrap().len() == 3
            && back.state["notes"][1]["status"] == "answered"
            && jv::to_string(back.state["notes"][0].get("text")).contains('\n')
    );
    t.remove_note(&h1).unwrap();
    t.undo();
    assert_eq!(t.state["notes"].as_array().unwrap().len(), 3);
    let out = cc_io_takeoff::pdfmarks::pdf_with_marks(&bytes, &hash, &t.state, true).unwrap();
    assert_eq!(out.pages, 1);
    match pdftotext(&out.bytes, false) {
        None => eprintln!("pdftotext not installed: the marked PDF's text checks are skipped"),
        Some(txt) => assert!(
            txt.contains("OPEN")
                && txt.contains("Verify the door swing")
                && txt.contains("with the owner")
                && txt.contains("ANSWERED")
                && txt.contains("Rev 2: wall moved"),
            "{txt}"
        ),
    }
    let (mut cs, mut ms) = (0, 0);
    cc_io_takeoff::pdfmarks::cloud_path(0.0, 0.0, 100.0, 50.0, 10.0, &mut |op, _| if op == 'C' { cs += 1 } else { ms += 1 });
    assert!(ms == 1 && cs == 10 + 5 + 10 + 5, "{ms} {cs}");
    let sheets = e::estimate_sheets(&t.state, &|_: &str| "plan, page 1".to_string(), &model::base_estimate(&t.state));
    let mk = sheets.iter().find(|s| s.name == "Markups").unwrap();
    assert!(
        mk.rows.len() == 4
            && jv::stringify(&Value::Array(mk.rows.iter().flat_map(|r| r.iter().map(XCell::to_json)).collect())).contains("Answered")
    );
}

/// The workbook's zip entries carry the local time of the instant (xlsx.js `getHours()` …), its created date UTC:
/// takeoffcraft xlsx.js `zip(files, { date: new Date(1791810123000) })` under TZ=Asia/Kolkata (+330) and Etc/GMT+6 (−360).
#[test]
fn xlsx_zip_dates_are_local_time() {
    let t = 1_791_810_123_000.0;
    let dos = |z: &[u8]| (u16::from_le_bytes([z[10], z[11]]), u16::from_le_bytes([z[12], z[13]]));
    for (zone, want) in [
        (cc_clock::Zone::Fixed(330), (0x9401, 0x5d4c)),
        (cc_clock::Zone::Fixed(-360), (0x3841, 0x5d4c)),
        (cc_clock::Zone::Fixed(825), (0x15e1, 0x5d4d)),
    ] {
        let x = xlsx::write_xlsx(&[], "T", false, &Stamp::from_ms(t), &Stamp::local(t, zone));
        assert_eq!(dos(&x), want, "{zone:?}");
        assert!(String::from_utf8_lossy(&x).contains(">2026-10-12T13:02:03Z</dcterms:created>"));
    }
    // 1980-01-01T05:59:58Z is still 1979 in Denver: the year wraps as the JS's 32-bit shift does (not clamped)
    let x = xlsx::write_xlsx(
        &[],
        "T",
        false,
        &Stamp::from_ms(315_554_398_000.0),
        &Stamp::local(315_554_398_000.0, cc_clock::Zone::Fixed(-360)),
    );
    assert_eq!(dos(&x), (0xbf7d, 0xff9f));
}
