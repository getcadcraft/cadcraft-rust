//! The sections of MarkupCraft 0.5's Node test (`bin/test-markupcraft.mjs`) that only need the DOM-free modules,
//! ported check for check (labels kept). PDF sections are in cc-io-markup's tests; exact step-by-step parity with
//! the frozen JS is `forks/tools/mkdiff.mjs`.

use cc_markup::bcf::{Topic, ZipFile, build_bcf, guid_for, unzip, zip_with};
use cc_markup::bcfread::{PlanSheet, plan_import, read_bcf};
use cc_markup::chest::{default_chest, export_chest, merge_chest, parse_chest};
use cc_markup::clip::{PlaceOpts, clean_clip, clean_clip_text, clip_bounds, make_clip, place_clip};
use cc_markup::columns::{clean_columns, clean_fields, compare_by, field_ok, new_column_key};
use cc_markup::compare::{MixColors, alignment, combine};
use cc_markup::csv::{Cell, SheetRef, cell, markup_rows, punch_rows, to_csv};
use cc_markup::font::{text_width, win_ansi_hex, wrap_text};
use cc_markup::geom::{apply, apply_grip, grips, inv, move_by, pick, pick_window, similarity, simplify};
use cc_markup::model::{Env, Markup, Store, geometry_ok, make_markup, stamp_color};
use cc_markup::pages::{
    blank_info, clean_blank, clean_bookmarks, count_bookmarks, from_sheets, grid_content, order_delete, order_insert, order_move, papers,
    parse_page_list, untitled_name,
};
use cc_markup::shapes::{Item, PathItem, ShapeEnv, TextItem, cloud_path, list_bounds, place_text, shapes};
use cc_markup::slip::{SlipSheet, carry_markups, match_sheets, superseded_stamp};
use cc_markup::spaces::{legend_rows, rows_key, sketch_point, space_of};
use cc_markup::units::{Cal, calibration_from, format_length, measure, measure_area, parse_length, preset_calibration, valid_calibration};
use serde_json::{Map, Value, json};

fn near(a: f64, b: f64, e: f64) -> bool {
    (a - b).abs() <= e
}
fn near_arr(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| near(*x, *y, 1e-3))
}
fn fnums(v: &Value) -> Vec<f64> {
    v.as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect()).unwrap_or_default()
}
fn obj(v: Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap()
}
fn env() -> Env {
    Env { now_ms: 1_791_882_000_000.0, count: 0 }
}
fn mk(e: &mut Env, v: Value) -> Markup {
    make_markup(e, &obj(v))
}
fn quarter() -> Cal {
    preset_calibration("1/4\" = 1'-0\"", 48.0, "ft-in")
}
const S: &str = "f1:0";
const AUTHOR: &str = "José Müller";

fn all(e: &mut Env) -> Vec<Markup> {
    let a = AUTHOR;
    [
        json!({ "sheet": S, "type": "rect", "rect": [100, 100, 300, 200], "author": a, "comment": "Check clearance", "style": { "fill": "#ffee88", "fillOpacity": 0.4 } }),
        json!({ "sheet": S, "type": "ellipse", "rect": [1000, 120, 1180, 260], "author": a }),
        json!({ "sheet": S, "type": "cloudrect", "rect": [400, 400, 600, 500], "author": a, "status": "Accepted", "punch": { "n": 1, "assignee": "Mechanical sub", "due": "2026-11-01", "status": "Open" } }),
        json!({ "sheet": S, "type": "cloud", "pts": [[720, 400], [900, 380], [950, 520], [760, 560]], "author": a, "status": "Rejected" }),
        json!({ "sheet": S, "type": "polygon", "pts": [[150, 400], [260, 380], [280, 480]], "author": a }),
        json!({ "sheet": S, "type": "polyline", "pts": [[1050, 700], [1150, 760], [1100, 840]], "author": a }),
        json!({ "sheet": S, "type": "line", "pts": [[100, 700], [280, 700]], "author": a }),
        json!({ "sheet": S, "type": "arrow", "pts": [[650, 900], [760, 760]], "author": a }),
        json!({ "sheet": S, "type": "pen", "pts": [[900, 1500], [940, 1540], [990, 1500]], "author": a }),
        json!({ "sheet": S, "type": "highlight", "pts": [[450, 1000], [700, 1000]], "author": a }),
        json!({ "sheet": S, "type": "callout", "rect": [700, 1240, 890, 1290], "pts": [[620, 1180], [690, 1265]], "text": "Move prep sink 6\" east (coordinate with plumbing) ✓", "author": a }),
        json!({ "sheet": S, "type": "text", "rect": [150, 1580, 400, 1610], "text": "Verify door swing at walk-in cooler", "author": a }),
        json!({ "sheet": S, "type": "note", "pts": [[1150, 1100]], "comment": "Confirm hood size", "author": a }),
        json!({ "sheet": S, "type": "stamp", "rect": [950, 1600, 1150, 1670], "label": "Revise and resubmit", "author": a, "style": { "color": stamp_color("Revise and resubmit") } }),
        json!({ "sheet": S, "type": "measure", "pts": [[400, 640], [688, 640]], "author": a }),
    ]
    .into_iter()
    .map(|v| mk(e, v))
    .collect()
}

#[test]
fn text_metrics_and_winansi() {
    assert!(near(text_width("Hello", 10.0, false), 22.78, 1e-6), "Helvetica width of \"Hello\" at 10 pt");
    assert!(text_width("APPROVED", 10.0, true) > text_width("APPROVED", 10.0, false), "bold is wider");
    let wrapped = wrap_text("Move prep sink six inches east and coordinate with plumbing", 12.0, 120.0, false);
    assert!(wrapped.len() > 1 && wrapped.iter().all(|l| text_width(l, 12.0, false) <= 120.01), "wraps to the width");
    assert_eq!(wrap_text("a\nb", 12.0, 0.0, false).len(), 2, "hard line breaks kept");
    assert!(
        wrap_text("Supercalifragilisticexpialidocious", 12.0, 40.0, false).iter().all(|l| text_width(l, 12.0, false) <= 40.01),
        "a long word breaks by character"
    );
    let (hex, missing) = win_ansi_hex("Café – “ok” ✓");
    assert_eq!((hex.as_str(), missing), ("<436166e9209620936f6b94203f>", 1), "WinAnsi: é, en dash, curly quotes; ✓ → ?");
}

#[test]
fn lengths_and_scales() {
    let ft = |s: &str, u: &str| parse_length(s, u).map(|v| ((v / 304.8) * 1e6).round() / 1e6);
    assert_eq!(ft("24'-0\"", "ft-in"), Some(24.0));
    assert_eq!(ft("12' 6 1/2\"", "ft-in"), Some(((12.0 + 6.5 / 12.0) * 1e6_f64).round() / 1e6));
    assert!(ft("6\"", "ft-in") == Some(0.5) && ft("1/2\"", "ft-in") == Some(((0.5 / 12.0) * 1e6_f64).round() / 1e6), "6\" and 1/2\"");
    assert!(ft("12.5'", "ft-in") == Some(12.5) && ft("12.5 ft", "ft-in") == Some(12.5) && ft("3 ft 6 in", "ft-in") == Some(3.5));
    assert_eq!(ft("12'6", "ft-in"), Some(12.5), "12'6 (no inch mark)");
    assert!(
        parse_length("3.5 m", "ft-in") == Some(3500.0)
            && parse_length("900 mm", "ft-in") == Some(900.0)
            && parse_length("90 cm", "ft-in") == Some(900.0)
    );
    assert!(
        parse_length("16", "ft-in") == Some(16.0 * 304.8) && parse_length("16", "m") == Some(16000.0),
        "bare number takes the chosen unit"
    );
    assert!(["abc", "", "-3", "0"].iter().all(|s| parse_length(s, "ft-in").is_none()), "rejects nonsense, zero and negatives");
    assert!(format_length(304.8 * 16.0, "ft-in") == "16'-0\"" && format_length(304.8 * 12.0 + 25.4 * 6.5, "ft-in") == "12'-6 1/2\"");
    assert!(format_length(3500.0, "m") == "3.500 m" && format_length(900.4, "mm") == "900 mm", "formats metric");
    let q = quarter();
    assert_eq!(measure(288.0, Some(&q)), "16'-0\"");
    assert_eq!(measure(72.0, Some(&preset_calibration("1:100", 100.0, "mm"))), "2.540 m");
    let cal = calibration_from(288.0, 304.8 * 16.0, "ft-in").unwrap();
    assert!(near(cal.mm_per_pt, q.mm_per_pt, 1e-9) && valid_calibration(&cal), "two points + 16'-0\" = the 1/4\" scale");
    let bad = |mm: f64, u: &str| Cal { mm_per_pt: mm, unit: u.into(), label: String::new() };
    assert!(!valid_calibration(&bad(-1.0, "ft")) && !valid_calibration(&bad(1.0, "parsec")), "bad calibrations rejected");
}

#[test]
fn store_edits_undo_redo() {
    let mut st = Store::new();
    st.env = env();
    let a = st.change("Add", |tx| tx.add_fields(&obj(json!({ "sheet": "s1", "type": "rect", "rect": [10, 10, 50, 40] })))).unwrap();
    let b = st.change("Add", |tx| tx.add_fields(&obj(json!({ "sheet": "s1", "type": "line", "pts": [[0, 0], [10, 10]] })))).unwrap();
    assert!(st.all().len() == 2 && st.on_sheet("s1").len() == 2, "two markups");
    let before = st.get(&a.id).unwrap().clone();
    let patch = move_by(&before, 5.0, 5.0);
    st.change("Move", |tx| Ok(tx.update(&a.id, &patch, true))).unwrap();
    assert!(near_arr(&st.get(&a.id).unwrap().rect.unwrap(), &[15.0, 15.0, 55.0, 45.0]), "update replaces the object");
    st.change("Delete", |tx| {
        tx.remove(&a.id);
        Ok(())
    })
    .unwrap();
    assert!(st.get(&a.id).is_none(), "removed");
    st.undo();
    assert!(st.get(&a.id).is_some() && st.all()[0].id == a.id, "undo restores it in its place in the drawing order");
    st.undo();
    assert!(near_arr(&st.get(&a.id).unwrap().rect.unwrap(), &[10.0, 10.0, 50.0, 40.0]), "undo the move");
    st.redo();
    st.redo();
    assert!(st.get(&a.id).is_none() && st.get(&b.id).is_some(), "redo twice");
    st.change("Scale", |tx| {
        tx.set_calibration("s1", Some(quarter()));
        Ok(())
    })
    .unwrap();
    st.change("Punch", |tx| {
        let n = tx.punch_next();
        tx.update(&b.id, &obj(json!({ "punch": { "n": n, "assignee": "A", "due": "", "status": "Open" } })), true);
        tx.set_punch_next(n + 1.0);
        Ok(())
    })
    .unwrap();
    assert!(st.calibrations.get("s1").is_some() && st.punch_next == 2.0 && st.punch_items().len() == 1, "calibration and punch numbering");
    st.undo();
    st.undo();
    assert!(st.calibrations.get("s1").is_none() && st.punch_next == 1.0 && st.get(&b.id).unwrap().punch.is_none(), "both undo");
    assert!(st.dirty_sheets.contains("s1"), "sheet marked as changed");
    let mut e = env();
    assert!(mk(&mut e, json!({ "type": "rect", "rect": [5, 5, 1, 1], "status": "Nope" })).status == "Open", "makeMarkup normalises status");
    assert!(near_arr(&mk(&mut e, json!({ "type": "rect", "rect": [5, 5, 1, 1] })).rect.unwrap(), &[1.0, 1.0, 5.0, 5.0]), "and rect");
    assert!(
        !geometry_ok(&mk(&mut e, json!({ "type": "polygon", "pts": [[0, 0], [1, 1]] })))
            && geometry_ok(&mk(&mut e, json!({ "type": "note", "pts": [[1, 2]] }))),
        "geometryOk"
    );
}

fn text_of(list: &[Item]) -> Option<&TextItem> {
    list.iter().find_map(|i| if let Item::Text(t) = i { Some(t) } else { None })
}

#[test]
fn display_lists() {
    let mut e = env();
    let all = all(&mut e);
    let env9 = ShapeEnv { cal: Some(quarter()), badge: 9.0, legend: None };
    for m in &all {
        let list = shapes(m, &env9);
        let b = list_bounds(&list).unwrap();
        assert!(!list.is_empty() && b[2] > b[0] && b[3] > b[1], "display list for {}", m.kind);
    }
    let cloud = list_bounds(&shapes(&all[2], &ShapeEnv::default())).unwrap();
    assert!(cloud[0] < 400.0 && cloud[1] < 400.0 && cloud[2] > 600.0 && cloud[3] > 500.0, "cloud scallops bulge outward");
    let path = |d| {
        Item::Path(PathItem {
            d,
            stroke: Some("#000000".into()),
            fill: None,
            w: 0.0,
            alpha: 1.0,
            falpha: 1.0,
            cap: "butt",
            join: "miter",
            blend: None,
            dash: None,
            rule: None,
            nopick: false,
            badge: false,
        })
    };
    let bw = list_bounds(&[path(cloud_path(&[[0.0, 0.0], [0.0, 100.0], [100.0, 100.0], [100.0, 0.0]], 20.0))]).unwrap();
    let bc = list_bounds(&[path(cloud_path(&[[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]], 20.0))]).unwrap();
    assert!(bw[0] < -5.0 && bc[0] < -5.0 && near(bw[0], bc[0], 1.0), "outward for both windings");
    let cal_only = ShapeEnv { cal: Some(quarter()), ..ShapeEnv::default() };
    assert_eq!(text_of(&shapes(&all[14], &cal_only)).unwrap().lines[0], "16'-0\"", "measure label from the sheet scale");
    assert_eq!(text_of(&shapes(&all[14], &ShapeEnv::default())).unwrap().lines[0], "Not calibrated", "measure without a scale says so");
    let badge = ShapeEnv { badge: 9.0, ..ShapeEnv::default() };
    assert!(
        shapes(&all[2], &badge).iter().any(|i| matches!(i, Item::Text(t) if t.badge && t.lines[0] == "1")),
        "punch number drawn with the markup"
    );
    let ti = |lines: Vec<&str>, bbox, rot| TextItem {
        lines: lines.into_iter().map(String::from).collect(),
        bbox,
        size: 10.0,
        bold: false,
        color: "#000000".into(),
        rot,
        align: "left",
        valign: "top",
        pad: 2.0,
        lh: 0.0,
        alpha: 1.0,
        badge: false,
    };
    let (l0, a0) = place_text(&ti(vec!["a", "b"], [0.0, 0.0, 100.0, 50.0], 0.0));
    assert!(a0 == 0.0 && near(l0[0].x, 2.0, 1e-6) && l0[0].y < 50.0 && l0[1].y < l0[0].y, "text reads down the box (rot 0)");
    let (l9, a9) = place_text(&ti(vec!["a"], [0.0, 0.0, 50.0, 100.0], 90.0));
    assert!(a9 == 90.0 && near(l9[0].y, 2.0, 1e-6) && l9[0].x > 0.0, "rotated page: text frame turns with it");
}

#[test]
fn grips_moving_hit_testing() {
    let mut e = env();
    let r = mk(&mut e, json!({ "type": "rect", "rect": [0, 0, 100, 50], "sheet": S }));
    let gs = grips(&r);
    assert!(gs.len() == 8 && gs.iter().all(|g| g.kind == "box"), "8 box grips");
    let g = |k: &str| gs.iter().find(|g| g.key == k).unwrap().clone();
    let rect_of = |p: cc_markup::geom::GripPatch| match p {
        cc_markup::geom::GripPatch::Rect(r) => r.to_vec(),
        cc_markup::geom::GripPatch::Pts(p) => p.into_iter().flatten().collect(),
    };
    assert!(near_arr(&rect_of(apply_grip(&r, &g("se"), [150.0, -20.0], false)), &[0.0, -20.0, 150.0, 50.0]), "se grip");
    assert!(near_arr(&rect_of(apply_grip(&r, &g("se"), [200.0, 30.0], true)), &[0.0, -50.0, 200.0, 50.0]), "Shift keeps proportions");
    assert!(near_arr(&rect_of(apply_grip(&r, &g("w"), [120.0, 0.0], false)), &[100.0, 0.0, 120.0, 50.0]), "dragging past flips cleanly");
    let ln = mk(&mut e, json!({ "type": "line", "pts": [[0, 0], [100, 0]], "sheet": S }));
    let v1 = grips(&ln).into_iter().find(|g| g.key == "v1").unwrap();
    let p = rect_of(apply_grip(&ln, &v1, [100.0, 90.0], true));
    assert!(p[2].round() == 95.0 && p[3].round() == 95.0, "Shift snaps a vertex to 45°");
    let pen = mk(&mut e, json!({ "type": "pen", "pts": [[0, 0], [10, 10], [20, 0]], "sheet": S }));
    let ge = grips(&pen).into_iter().find(|g| g.key == "e").unwrap();
    assert!(
        near_arr(&rect_of(apply_grip(&pen, &ge, [40.0, 5.0], false)), &[0.0, 0.0, 20.0, 10.0, 40.0, 0.0]),
        "freehand scales from its box"
    );
    assert_eq!(simplify(&[[0.0, 0.0], [1.0, 0.01], [2.0, 0.0], [3.0, 0.02], [4.0, 0.0]], 0.1).len(), 2, "freehand strokes are simplified");
    let all = all(&mut e);
    let list: Vec<&Markup> = all.iter().collect();
    let env_of = |_: &Markup| ShapeEnv { cal: Some(quarter()), badge: 9.0, legend: None };
    let kind = |p: [f64; 2]| pick(&list, p, 3.0, &env_of).map(|m| m.kind.clone());
    assert_eq!(kind([705.0, 830.0]).as_deref(), Some("arrow"), "pick: on the arrow");
    assert_eq!(kind([200.0, 150.0]).as_deref(), Some("rect"), "pick: inside a filled rectangle");
    assert_eq!(kind([1090.0, 190.0]), None, "pick: inside an unfilled ellipse misses");
    assert_eq!(kind([1001.0, 190.0]).as_deref(), Some("ellipse"), "pick: on the ellipse outline");
    assert_eq!(kind([300.0, 1595.0]).as_deref(), Some("text"), "pick: inside a text box");
    let w: Vec<String> = pick_window(&list, [90.0, 90.0, 310.0, 210.0], false, &env_of).iter().map(|m| m.kind.clone()).collect();
    assert_eq!(w, vec!["rect"], "window: fully inside");
    assert!(pick_window(&list, [250.0, 150.0, 260.0, 160.0], true, &env_of).iter().any(|m| m.kind == "rect"), "crossing: touching");
    let t = similarity([0.0, 0.0], [100.0, 0.0], [10.0, 20.0], [10.0, 220.0]).unwrap();
    assert!(
        near_arr(&apply(&t, [100.0, 0.0]), &[10.0, 220.0])
            && near_arr(&apply(&t, [50.0, 0.0]), &[10.0, 120.0])
            && near_arr(&apply(&inv(&t), [10.0, 220.0]), &[100.0, 0.0])
    );
    assert!(similarity([0.0, 0.0], [0.0, 0.0], [1.0, 1.0], [2.0, 2.0]).is_none(), "two equal points refuse to align");
}

#[test]
fn sheet_compare() {
    let px = |r: u8, g: u8, b: u8| [r, g, b, 255];
    let a: Vec<u8> = [px(0, 0, 0), px(0, 0, 0), px(255, 255, 255), px(255, 255, 255)].concat();
    let b: Vec<u8> = [px(0, 0, 0), px(255, 255, 255), px(0, 0, 0), px(255, 255, 255)].concat();
    let mut out = vec![0u8; 16];
    let colors = MixColors { bg: [255.0, 255.0, 255.0], same: [100.0, 100.0, 100.0], old: [200.0, 0.0, 0.0], new: [0.0, 0.0, 200.0] };
    let stats = combine(&a, &b, &mut out, &colors, 0.5);
    assert_eq!(&out[0..3], &[100, 100, 100]);
    assert_eq!(&out[4..7], &[200, 0, 0]);
    assert_eq!(&out[8..11], &[0, 0, 200]);
    assert_eq!(&out[12..15], &[255, 255, 255]);
    assert!(stats.same == 1 && stats.changed_old == 1 && stats.changed_new == 1, "counts");
    combine(&a, &b, &mut out, &colors, 1.0);
    assert!(out[4..7] == [255, 255, 255] && out[0..3] == [0, 0, 200], "balance 1 = new sheet only");
    assert!(
        near_arr(&alignment(Some([1.0, 0.0, 0.0, 1.0, 5.0, 5.0]), 1.0, -2.0), &[1.0, 0.0, 0.0, 1.0, 6.0, 3.0]),
        "nudges add to the two-point fit"
    );
}

#[test]
fn tool_chest_untrusted_imports() {
    let e = env();
    let (tools, skipped) = parse_chest(&e, &export_chest(&e, &default_chest())).unwrap();
    assert!(tools.len() == default_chest().len() && skipped == 0, "export → import round trip");
    let evil = json!({ "format": "markupcraft-tool-chest", "version": 1, "tools": [
        { "name": "<img src=x onerror=alert(1)>", "type": "rect", "style": { "color": "javascript:alert(1)", "width": 1e9, "opacity": -5 } },
        { "name": "x", "type": "foreign" }, { "name": "y", "type": "__proto__" }, null, "str",
        { "id": "../../etc", "name": "Stamp", "type": "stamp", "label": "A".repeat(500), "style": { "color": "#ABCDEF", "fill": "red" } },
    ] })
    .to_string();
    let (r, sk) = parse_chest(&e, &evil).unwrap();
    assert!(r.len() == 2 && sk == 4, "bad tools skipped");
    assert!(
        r[0]["style"]["color"] == "#d0021b" && r[0]["style"]["width"] == 72.0 && r[0]["style"]["opacity"] == 0.05,
        "colors and numbers clamped"
    );
    assert_eq!(r[0]["name"], "<img src=x onerror=alert(1)>", "names stay text");
    assert!(
        r[1]["label"].as_str().unwrap().len() == 60
            && r[1]["style"]["fill"].is_null()
            && r[1]["style"]["color"] == "#abcdef"
            && r[1]["id"].as_str().unwrap().starts_with("tool-")
    );
    let threw = ["nope", "{\"a\":1}", "[]"].iter().filter(|t| parse_chest(&e, t).is_err()).count();
    assert_eq!(threw, 2, "not JSON / not a chest → plain error");
    let m = merge_chest(&[json!({ "id": "a", "name": "1" })], &[json!({ "id": "a", "name": "2" }), json!({ "id": "b", "name": "3" })]);
    assert_eq!(m.iter().map(|t| t["name"].as_str().unwrap()).collect::<Vec<_>>().join(","), "2,3", "merge replaces by id");
}

#[test]
fn clipboard() {
    let mut st = Store::new();
    st.env = env();
    let (a, b) = ("f1:0", "f2:3");
    let mut e0 = env();
    let rect = mk(
        &mut e0,
        json!({ "sheet": a, "type": "rect", "rect": [100, 100, 200, 160], "author": "Ann", "status": "Accepted", "punch": { "n": 4, "assignee": "GC", "due": "", "status": "Open" }, "replies": [{ "author": "Bo", "text": "ok", "kind": "comment" }], "style": { "color": "#1e48c8", "width": 3 } }),
    );
    let cloud = mk(
        &mut e0,
        json!({ "sheet": a, "type": "cloud", "pts": [[300, 300], [360, 300], [330, 350]], "text": "Revise", "comment": "see RFI 12", "author": "Ann" }),
    );
    let locked = mk(&mut e0, json!({ "sheet": a, "type": "foreign", "rect": [0, 0, 5, 5], "locked": true }));
    st.change("Draw", |tx| {
        tx.add(rect.clone())?;
        tx.add(cloud.clone())?;
        tx.add(locked.clone())
    })
    .unwrap();
    let c = make_clip(&st.env, &[&rect, &cloud, &locked], "tab-1", a).unwrap();
    assert!(c.items.len() == 2 && c.sheet == a, "a copy keeps the markups, never a read-only one");
    assert!(
        !c.items[0].contains_key("author")
            && !c.items[0].contains_key("id")
            && !c.items[0].contains_key("replies")
            && !c.items[0].contains_key("punch")
    );
    assert!(make_clip(&st.env, &[&locked], "", "").is_none() && make_clip(&st.env, &[], "", "").is_none(), "nothing copyable → no copy");
    let text = cc_markup::json::stringify(&cc_markup::clip::clip_value(&c));
    let mut e = e0;
    let back = clean_clip_text(&mut e, &text).unwrap();
    assert!(back.items.len() == 2 && back.skipped == Some(0) && back.items == c.items, "stored copy reads back the same");
    assert!(near_arr(&clip_bounds(&c.items).unwrap(), &[100.0, 100.0, 360.0, 350.0]), "bounds of the copy");
    let placed = place_clip(
        &mut e,
        &back,
        &PlaceOpts { sheet: b.into(), author: "Cy".into(), rot: Some(90.0), delta: Some([0.0, 0.0]), center: None },
    );
    assert!(
        placed.len() == 2
            && placed.iter().all(|m| m.sheet.as_deref() == Some(b)
                && m.author == "Cy"
                && m.status == "Open"
                && m.punch.is_none()
                && m.replies.is_empty()
                && !m.locked
                && m.rot == 90.0)
    );
    assert!(placed[0].id != rect.id && placed[1].id != cloud.id && placed[0].id != placed[1].id, "pasted markups get new ids");
    assert!(placed[0].rect == rect.rect && placed[1].pts == cloud.pts, "paste in place keeps the sheet coordinates");
    assert!(
        placed[0].style.color == "#1e48c8"
            && placed[0].style.width == 3.0
            && placed[1].text == "Revise"
            && placed[1].comment == "see RFI 12"
    );
    let before = st.all().len();
    st.change("Paste 2 markups", |tx| {
        for m in &placed {
            tx.add(m.clone())?;
        }
        Ok(())
    })
    .unwrap();
    assert!(st.all().len() == before + 2 && st.on_sheet(b).len() == 2, "paste adds them to the target sheet");
    st.undo();
    assert!(st.all().len() == before && st.on_sheet(b).is_empty(), "one Ctrl+Z takes the whole paste away");
    st.redo();
    assert_eq!(st.on_sheet(b).len(), 2, "redo puts it back");
    let off = place_clip(&mut e, &back, &PlaceOpts { sheet: a.into(), delta: Some([12.0, -12.0]), ..PlaceOpts::default() });
    assert!(near_arr(&off[0].rect.unwrap(), &[112.0, 88.0, 212.0, 148.0]), "beside the copy: moved by the step");
    let at_p = place_clip(&mut e, &back, &PlaceOpts { sheet: a.into(), center: Some([1000.0, 500.0]), ..PlaceOpts::default() });
    let items: Vec<Map<String, Value>> = at_p.iter().map(|m| cc_markup::json::markup_value(m).as_object().cloned().unwrap()).collect();
    let b_at = clip_bounds(&items).unwrap();
    assert!(
        near((b_at[0] + b_at[2]) / 2.0, 1000.0, 1e-6)
            && near((b_at[1] + b_at[3]) / 2.0, 500.0, 1e-6)
            && near(b_at[2] - b_at[0], 260.0, 1e-6)
    );
    let evil = json!({ "format": "markupcraft-clip", "v": 1, "source": "x".repeat(500), "sheet": a, "at": "2026", "items": [
        { "type": "rect", "rect": [0, 0, "x", 1] }, { "type": "foreign", "rect": [0, 0, 1, 1] },
        { "type": "__proto__", "rect": [0, 0, 1, 1] }, { "type": "constructor" }, null, "str", 7,
        { "type": "line", "pts": [[0, 0]] }, { "type": "polygon", "pts": [[0, 0], [1, 1], [null, 2]] }, { "type": "rect", "rect": [0, 0, 2e7, 1] },
        { "type": "rect", "rect": [10, 10, 20, 20], "rot": 45, "id": "keep-me", "author": "Mallory", "locked": true, "status": "Accepted",
          "text": "a\u{0}b<script>", "label": "L".repeat(900), "style": { "color": "red;x", "fill": "url(x)", "width": 1e9, "opacity": -1, "fontSize": "big" } },
    ] });
    let r = clean_clip(&mut e, &evil).unwrap();
    assert!(r.items.len() == 1 && r.skipped == Some(10), "bad items left out, the good one kept");
    let g = &r.items[0];
    let st_ = g["style"].as_object().unwrap();
    assert!(
        g["rot"] == 0.0
            && st_["color"] == "#d0021b"
            && st_["fill"].is_null()
            && st_["width"] == 200.0
            && st_["opacity"] == 0.05
            && st_["fontSize"] == 12.0
    );
    assert!(g["text"] == "ab<script>" && g["label"].as_str().unwrap().len() == 200 && r.source.len() == 80);
    let gp = &place_clip(&mut e, &r, &PlaceOpts { sheet: a.into(), author: "Me".into(), ..PlaceOpts::default() })[0];
    assert!(
        gp.id != "keep-me" && gp.author == "Me" && !gp.locked && gp.status == "Open",
        "a copy cannot choose its id, author, lock or status"
    );
    let nones = [
        "nope",
        "{}",
        "{\"format\":\"markupcraft-clip\",\"v\":2,\"items\":[{\"type\":\"rect\",\"rect\":[0,0,1,1]}]}",
        "{\"format\":\"markupcraft-tool-chest\",\"v\":1,\"items\":[]}",
        "{\"format\":\"markupcraft-clip\",\"v\":1,\"items\":[]}",
        "null",
        "[]",
    ];
    assert_eq!(nones.iter().filter(|t| clean_clip_text(&mut e, t).is_none()).count(), 7, "no paste");
    let many = json!({ "format": "markupcraft-clip", "v": 1, "items": (0..2500).map(|i| json!({ "type": "note", "pts": [[i, i]] })).collect::<Vec<_>>() });
    assert_eq!(clean_clip(&mut e, &many).unwrap().items.len(), 2000, "at most 2000 markups in one paste");
}

fn sheet_ref() -> SheetRef {
    SheetRef { title: "K-101".into(), file: "kitchen-plan-sheet.pdf".into(), page: Some(Cell::N(1.0)) }
}

#[test]
fn csv() {
    assert!(
        cell(&"=HYPERLINK(\"x\")".into()) == "\"'=HYPERLINK(\"\"x\"\")\""
            && cell(&"+1".into()) == "'+1"
            && cell(&"a,b".into()) == "\"a,b\""
            && cell(&"plain".into()) == "plain"
    );
    let mut e = env();
    let all = all(&mut e);
    let refs: Vec<&Markup> = all.iter().collect();
    let rows = markup_rows(&refs, &|_| Some(sheet_ref()), &|_| String::new(), None, &[]);
    assert!(rows.len() == all.len() + 1 && rows[0][0] == Cell::S("Punch no.".into()), "markups CSV: header + one row each");
    let pr = punch_rows(&refs, &|_| Some(sheet_ref()));
    assert!(pr.len() == 2 && pr[1][0] == Cell::N(1.0) && pr[1][6] == Cell::S("Mechanical sub".into()), "punch CSV");
    assert_eq!(to_csv(&[vec!["a".into(), "b".into()], vec!["1".into(), "2".into()]]), "\u{feff}a,b\r\n1,2\r\n", "BOM + CRLF");
}

#[test]
fn page_orders_and_lists() {
    assert_eq!(order_delete(4.0, &[1.0, 3.0]), vec![0.0, 2.0]);
    assert_eq!(order_move(4.0, &[3.0], 1.0), vec![0.0, 3.0, 1.0, 2.0]);
    assert_eq!(order_insert(3.0, 1.0, 2.0), vec![Some(0.0), None, None, Some(1.0), Some(2.0)]);
    assert_eq!(parse_page_list("1-2, 4 6-", 7.0).unwrap(), vec![0.0, 1.0, 3.0, 5.0, 6.0], "typed page lists: ranges, open ends");
    assert!(parse_page_list("9", 7.0).is_err() && parse_page_list("x", 7.0).is_err() && parse_page_list("", 7.0).is_err(), "refusals");
}

fn pg(key: &str, no: &str, file: f64, page: f64, w: f64, h: f64, rotate: f64) -> SlipSheet {
    SlipSheet { key: key.into(), no: no.into(), file, page, width: w, height: h, view: [0.0, 0.0, w, h], rotate }
}

#[test]
fn slip_sheet() {
    let p = |k: &str, n: &str, f: f64, pa: f64| pg(k, n, f, pa, 1296.0, 1728.0, 0.0);
    let olds = [
        p("a:0", "A-101", 0.0, 0.0),
        p("a:1", "A-102", 0.0, 1.0),
        p("b:0", "A-101", 1.0, 0.0),
        p("s:0", "", 2.0, 0.0),
        p("s:1", "", 2.0, 1.0),
    ];
    let news = [
        p("n:0", "a101", 3.0, 0.0),
        p("n:1", "A 102", 3.0, 1.0),
        p("n:2", "A-103", 3.0, 2.0),
        p("x:0", "", 4.0, 0.0),
        p("x:1", "", 4.0, 1.0),
    ];
    let pairs = match_sheets(&olds, &news);
    let by = |k: &str| pairs.iter().find(|x| x.new_key == k).map(|x| (x.old_key.clone(), x.by)).unwrap();
    assert!(
        by("n:0").0.as_deref() == Some("b:0") && by("n:1").0.as_deref() == Some("a:1") && by("n:0").1 == Some("number"),
        "match by sheet number"
    );
    assert_eq!(by("n:2").0, None, "a sheet with a new number has nothing to replace");
    assert!(
        by("x:0").0.as_deref() == Some("s:0") && by("x:1").0.as_deref() == Some("s:1") && by("x:1").1 == Some("order"),
        "scans fall back to page order"
    );
    let mut e = env();
    let src = [
        mk(
            &mut e,
            json!({ "sheet": "a:1", "type": "cloudrect", "rect": [100, 100, 300, 200], "author": AUTHOR, "status": "Open", "punch": { "n": 4, "assignee": "GC", "due": "", "status": "Open" }, "replies": [{ "author": "Ann", "text": "Still open", "kind": "comment" }] }),
        ),
        mk(
            &mut e,
            json!({ "sheet": "a:1", "type": "text", "rect": [400, 400, 600, 430], "text": "Done", "author": AUTHOR, "status": "Completed" }),
        ),
        mk(&mut e, json!({ "sheet": "a:1", "type": "foreign", "rect": [10, 10, 20, 20], "locked": true })),
    ];
    let refs: Vec<&Markup> = src.iter().collect();
    let c1 = carry_markups(&refs, "n:1", Some(&olds[1]), Some(&news[1]), false);
    assert!(
        c1.items.len() == 2
            && c1.left == 1
            && c1.scaled == 0
            && c1.items.iter().all(|m| m["sheet"] == "n:1" && !m.contains_key("id"))
            && fnums(&c1.items[0]["rect"]) == src[0].rect.unwrap().to_vec()
    );
    assert!(
        c1.items[0]["replies"].as_array().unwrap().len() == 1
            && c1.items[0]["replies"][0]["text"] == "Still open"
            && c1.items[0]["punch"]["n"] == 4.0
            && c1.sources[0] == src[0].id
    );
    let c2 = carry_markups(&refs, "n:1", Some(&olds[1]), Some(&pg("n:1", "A-102", 3.0, 1.0, 648.0, 864.0, 0.0)), true);
    assert!(
        c2.items.len() == 1 && c2.closed == 1 && c2.scaled == 1 && c2.items[0]["rect"][2] == 150.0,
        "skip closed; a smaller page scales"
    );
    let mut st = Store::new();
    st.env = e;
    st.load(src.to_vec(), Vec::new(), Vec::new(), None, None);
    let stamp = superseded_stamp(&olds[1], "Superseded", AUTHOR, Some("a:1"));
    st.change("slip", |tx| {
        for (k, it) in c1.items.iter().enumerate() {
            tx.add_fields(it)?;
            if it.get("punch").is_some_and(|p| !p.is_null()) {
                tx.update(&c1.sources[k], &obj(json!({ "punch": null })), true);
            }
        }
        tx.add_fields(&stamp)?;
        Ok(())
    })
    .unwrap();
    assert!(
        st.on_sheet("n:1").len() == 2
            && st.get(&src[0].id).unwrap().punch.is_none()
            && st.on_sheet("a:1").iter().any(|m| m.kind == "stamp" && m.label == "Superseded")
    );
    st.undo();
    assert!(
        st.on_sheet("n:1").is_empty() && st.get(&src[0].id).unwrap().punch.as_ref().unwrap().n == 4.0 && st.on_sheet("a:1").len() == 3,
        "one undo puts everything back"
    );
    for rot in [0.0, 90.0, 180.0, 270.0] {
        let s = superseded_stamp(&pg("r", "", 0.0, 0.0, 612.0, 792.0, rot), "Superseded", "", Some("r"));
        let r: Vec<f64> = s["rect"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        let (x0, y0, x1, y1) = (r[0], r[1], r[2], r[3]);
        let top = if rot == 0.0 {
            y1 > 700.0
        } else if rot == 90.0 {
            x0 < 100.0
        } else if rot == 180.0 {
            y0 < 100.0
        } else {
            x1 > 512.0
        };
        let long = if rot % 180.0 != 0.0 { y1 - y0 > x1 - x0 } else { x1 - x0 > y1 - y0 };
        assert!(
            s["rot"] == rot && x0 >= 0.0 && y0 >= 0.0 && x1 <= 612.0 && y1 <= 792.0 && top && long,
            "Superseded across the top at {rot}°"
        );
    }
}

#[test]
fn layers() {
    let mut st = Store::new();
    st.env = env();
    let mut e = env();
    let a =
        mk(&mut e, json!({ "sheet": "L:0", "type": "rect", "rect": [50, 50, 150, 120], "author": AUTHOR, "layer": "  Mechanical \u{7}" }));
    let b =
        mk(&mut e, json!({ "sheet": "L:0", "type": "cloudrect", "rect": [300, 300, 400, 380], "author": AUTHOR, "layer": "Architect" }));
    let c = mk(&mut e, json!({ "sheet": "L:0", "type": "text", "rect": [60, 400, 260, 430], "text": "no layer", "author": AUTHOR }));
    let layers = json!([{ "name": "Electrical", "hidden": true }, { "name": "", "hidden": true }, "junk"]);
    st.load(vec![a.clone(), b.clone(), c.clone()], Vec::new(), Vec::new(), Some(&layers), None);
    assert!(
        a.layer == "Mechanical" && st.layer_names() == vec!["Architect", "Electrical", "Mechanical"],
        "layer names are cleaned and listed"
    );
    st.set_layer("Architect", None, Some(true));
    st.set_layer("Mechanical", Some(true), None);
    let g = |id: &str| st.get(id).unwrap().clone();
    assert!(
        !st.shown(&g(&a.id)) && st.shown(&g(&b.id)) && !st.open(&g(&b.id)) && st.open(&g(&c.id)),
        "hidden layers do not show; locked layers do not pick"
    );
    let clip = make_clip(&st.env, &[&a], "", "").unwrap();
    assert_eq!(
        place_clip(&mut e, &clip, &PlaceOpts { sheet: "L:1".into(), ..PlaceOpts::default() })[0].layer,
        "Mechanical",
        "copy and paste keep the layer"
    );
}

#[test]
fn arcs_and_angles() {
    use cc_markup::arcs::{angle_at, arc_beziers, arc_length, arc_through, format_angle};
    let pi = std::f64::consts::PI;
    let q = arc_through([100.0, 0.0], [0.0, 100.0], [-100.0, 0.0]).unwrap();
    assert!(
        near(q.c[0], 0.0, 1e-6) && near(q.c[1], 0.0, 1e-6) && near(q.r, 100.0, 1e-6) && near(q.sweep, pi, 1e-6),
        "a half turn counterclockwise"
    );
    assert!(near(arc_through([-100.0, 0.0], [0.0, 100.0], [100.0, 0.0]).unwrap().sweep, -pi, 1e-6), "the other way runs clockwise");
    assert!(near(arc_through([100.0, 0.0], [0.0, -100.0], [0.0, 100.0]).unwrap().sweep.abs(), 1.5 * pi, 1e-6), "three quarters of a turn");
    assert!(arc_through([0.0, 0.0], [5.0, 5.0], [10.0, 10.0]).is_none(), "three points in a line make no arc");
    let bz = arc_beziers(&q);
    let mid = |b: &[[f64; 2]; 4], t: f64| {
        let u = 1.0 - t;
        [0, 1].map(|k| u * u * u * b[0][k] + 3.0 * u * u * t * b[1][k] + 3.0 * u * t * t * b[2][k] + t * t * t * b[3][k])
    };
    let err = bz.iter().map(|b| (mid(b, 0.5)[0].hypot(mid(b, 0.5)[1]) - 100.0).abs()).fold(0.0, f64::max);
    assert!(bz.len() == 2 && err < 0.03, "Bézier pieces stay on the circle");
    assert!(
        near(arc_length(Some(&q)), pi * 100.0, 1e-6)
            && near(angle_at([10.0, 0.0], [0.0, 0.0], [0.0, 10.0]), 90.0, 1e-6)
            && near(angle_at([10.0, 0.0], [0.0, 0.0], [-10.0, 0.0]), 180.0, 1e-6)
            && format_angle(37.46, 1) == "37.5°"
    );
    let mut e = env();
    let am = mk(&mut e, json!({ "sheet": "A:0", "type": "arc", "pts": [[100, 100], [200, 180], [300, 100]], "author": AUTHOR }));
    let gm = mk(&mut e, json!({ "sheet": "A:0", "type": "angle", "pts": [[400, 100], [300, 300], [500, 300]], "author": AUTHOR }));
    let al = shapes(&am, &ShapeEnv::default());
    let gl = shapes(&gm, &ShapeEnv::default());
    let has_c = matches!(&al[0], Item::Path(p) if p.d.iter().any(|s| matches!(s, cc_markup::shapes::Seg::C(..))));
    let p = gm.pts.clone().unwrap();
    assert!(
        al.len() == 1 && has_c && text_of(&gl).unwrap().lines[0] == format_angle(angle_at(p[0], p[1], p[2]), 1),
        "arc as curves, angle with its value"
    );
    assert!(grips(&am).len() == 3 && grips(&gm).len() == 3, "three vertex grips each");
}

#[test]
fn pictures() {
    use cc_markup::imagestamp::{b64_encode, clean_image, fit_box, image_info, place_rect};
    // a 120 × 60 PNG header (IHDR) and a JPEG frame header
    let mut png =
        vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0, 120, 0, 0, 0, 60, 8, 6, 0, 0, 0];
    png.extend_from_slice(&[0, 0, 0, 0]);
    let jpg: Vec<u8> = vec![
        0xff, 0xd8, 0xff, 0xe0, 0, 16, b'J', b'F', b'I', b'F', 0, 1, 1, 0, 0, 1, 0, 1, 0, 0, 0xff, 0xc0, 0, 17, 8, 0, 60, 0, 120, 3, 1,
        0x22, 0, 2, 0x11, 1, 3, 0x11, 1, 0xff, 0xd9,
    ];
    let pu = format!("data:image/png;base64,{}", b64_encode(&png));
    let ju = format!("data:image/jpeg;base64,{}", b64_encode(&jpg));
    let (pi, ji) = (image_info(&pu).unwrap(), image_info(&ju).unwrap());
    assert!(pi.width == 120 && pi.height == 60 && ji.width == 120 && ji.height == 60, "PNG and JPEG sizes read from their headers");
    let fake = format!("data:image/png;base64,{}", b64_encode(b"not a png at all, really"));
    assert!(
        clean_image("data:image/svg+xml;base64,PHN2Zz4=").is_empty()
            && clean_image(&fake).is_empty()
            && clean_image("javascript:alert(1)").is_empty()
            && clean_image(&pu) == pu
    );
    let fb = fit_box(120.0, 60.0, 216.0, 864.0);
    assert!(
        fb[0].round() == 216.0 && fb[1].round() == 108.0 && place_rect([100.0, 100.0], 40.0, 20.0, 90.0) == [90.0, 80.0, 110.0, 120.0],
        "fit keeps proportions; a turned page turns the box"
    );
    let mut e = env();
    let im =
        mk(&mut e, json!({ "sheet": "I:0", "type": "image", "rect": [100, 600, 316, 708], "img": pu, "label": "Logo", "author": AUTHOR }));
    let mut no_img = im.clone();
    no_img.img = String::new();
    assert!(
        geometry_ok(&im) && !geometry_ok(&no_img) && shapes(&im, &ShapeEnv::default()).iter().any(|i| matches!(i, Item::Image(_))),
        "an image needs its picture"
    );
    let clip = make_clip(&e, &[&im], "", "").unwrap();
    assert_eq!(
        place_clip(&mut e, &clip, &PlaceOpts { sheet: "I:1".into(), ..PlaceOpts::default() })[0].img,
        pu,
        "copy and paste keep the picture"
    );
}

#[test]
fn summary_workbook() {
    let mut e = env();
    let mut ms = all(&mut e);
    for (i, m) in ms.iter_mut().enumerate() {
        m.sheet = Some(if i < 8 { "S:0" } else { "S:1" }.into());
        m.replies = if i == 2 {
            vec![
                cc_markup::model::clean_reply(
                    &mut e,
                    &json!({ "id": "r1", "author": "Ann", "text": "Looks right", "kind": "comment", "date": "2026-10-12T10:00:00Z" }),
                )
                .unwrap(),
            ]
        } else {
            Vec::new()
        };
    }
    let refs: Vec<&Markup> = ms.iter().collect();
    let sheet_of = |m: &Markup| {
        Some(if m.sheet.as_deref() == Some("S:0") {
            SheetRef { title: "A-101 — Plan".into(), file: "set.pdf".into(), page: Some(Cell::N(1.0)) }
        } else {
            SheetRef { title: "A-102 — Details".into(), file: "set.pdf".into(), page: Some(Cell::N(2.0)) }
        })
    };
    let measure_of = |m: &Markup| if m.kind == "measure" { "12'-0\"".to_string() } else { String::new() };
    let sheets = cc_markup::summary::summary_sheets(&refs, &sheet_of, &measure_of, None, &[]);
    let rows = |i: usize| sheets[i]["rows"].as_array().unwrap().len();
    assert!(
        sheets.len() == 3 && rows(0) == ms.len() + 1 && rows(1) == 4 && rows(2) == 2,
        "three sheets: every markup, a row per sheet + total, the punch list"
    );
    let total = sheets[1]["rows"].as_array().unwrap().last().unwrap().clone();
    assert!(
        total[3]["v"] == ms.len() as f64
            && total[3]["f"] == "SUM(D2:D3)"
            && total[4]["v"].as_f64().unwrap() + total[5]["v"].as_f64().unwrap() == ms.len() as f64,
        "the totals are live formulas"
    );
    let x = cc_markup::xlsx::write_xlsx(&sheets, "Summary", false, e.now_ms);
    let files = unzip(&x).unwrap();
    let sh1 = String::from_utf8(files.iter().find(|f| f.name == "xl/worksheets/sheet1.xml").unwrap().data.clone()).unwrap();
    assert!(
        files.iter().any(|f| f.name == "xl/workbook.xml")
            && sh1.contains("Move prep sink 6&quot; east")
            && sh1.contains("Looks right")
            && files.iter().all(|f| f.crc_ok)
    );
}

#[test]
fn bcf_import() {
    let mut e = env();
    let all = all(&mut e);
    let mut ms = vec![all[2].clone(), all[10].clone()];
    for m in &mut ms {
        m.sheet = Some("B:0".into());
        m.replies = vec![cc_markup::model::clean_reply(&mut e, &json!({ "id": "r1", "author": "Ann", "text": "Check the hood", "kind": "comment", "date": "2026-10-12T10:00:00Z", "status": "" })).unwrap()];
    }
    let topics: Vec<Topic<'_>> = ms
        .iter()
        .map(|m| Topic {
            markup: m,
            sheet_title: "A-101 — Plan".into(),
            file_name: "set.pdf".into(),
            value: String::new(),
            bbox: Some(m.rect.unwrap_or([600.0, 1200.0, 900.0, 1300.0])),
            png: None,
        })
        .collect();
    let (bytes, _) = build_bcf(&topics, "Example kitchen", e.now_ms, e.now_ms);
    let ours = read_bcf(&bytes).unwrap();
    assert!(
        ours.version == "2.1"
            && ours.project == "Example kitchen"
            && ours.topics.len() == 2
            && ours.topics.iter().all(|t| t.comments.len() == 1 && t.camera.is_some()),
        "our own export reads back"
    );
    let guid_of: Vec<(String, String)> = ms.iter().map(|m| (guid_for(&format!("topic:{}", m.id)), m.id.clone())).collect();
    let reply_guids: Vec<String> =
        ms.iter().flat_map(|m| m.replies.iter().map(|r| guid_for(&format!("comment:{}:{}", m.id, r.id))).collect::<Vec<_>>()).collect();
    let sheets = vec![
        PlanSheet {
            key: "B:0".into(),
            title: "A-101 — Plan".into(),
            sheet_no: "A-101".into(),
            file: "set.pdf".into(),
            view: [0.0, 0.0, 1296.0, 1728.0],
        },
        PlanSheet {
            key: "B:1".into(),
            title: "A-102 — Details".into(),
            sheet_no: "A-102".into(),
            file: "set.pdf".into(),
            view: [0.0, 0.0, 1296.0, 1728.0],
        },
    ];
    let refs: Vec<&Markup> = ms.iter().collect();
    let same = plan_import(&ours, &refs, &guid_of, &reply_guids, &sheets, "B:1", "");
    assert!(
        same["matched"] == 2 && same["updates"].as_array().unwrap().is_empty() && same["adds"].as_array().unwrap().is_empty(),
        "importing our own export again changes nothing"
    );
    let g0 = guid_for(&format!("topic:{}", ms[0].id));
    let g1 = "0f3c2a9e-1111-4a2b-9c3d-123456789abc";
    let mkx = |g: &str, title: &str, status: &str, cg: &str| {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Markup><Topic Guid=\"{g}\" TopicType=\"Issue\" TopicStatus=\"{status}\"><Title>{title}</Title><Labels><Label>A-102</Label></Labels><CreationDate>2026-10-13T08:00:00Z</CreationDate><CreationAuthor>bim@example.com</CreationAuthor><Description><![CDATA[Duct <clash> & beam]]></Description><Comments><Comment Guid=\"{cg}\"><Date>2026-10-13T09:00:00Z</Date><Author>Ben</Author><Comment>Fixed in model rev &amp; 3 &#8212; ok</Comment></Comment></Comments><Viewpoints><ViewPoint Guid=\"{g}\"><Viewpoint>vp.bcfv</Viewpoint></ViewPoint></Viewpoints></Topic></Markup>"
        )
    };
    let vp = "<?xml version=\"1.0\"?><VisualizationInfo><PerspectiveCamera><CameraViewPoint><X>12.5</X><Y>-3</Y><Z>4</Z></CameraViewPoint></PerspectiveCamera></VisualizationInfo>";
    let f = |n: &str, d: String| ZipFile { name: n.into(), data: d.into_bytes() };
    let z = zip_with(
        &[
            f("bcf.version", "<Version VersionId=\"3.0\"/>".into()),
            f(&format!("{g0}/markup.bcf"), mkx(&g0, "Hood", "Closed", "aaaaaaaa-0000-4000-8000-000000000001")),
            f(&format!("{g1}/markup.bcf"), mkx(g1, "Duct clash", "Active", "aaaaaaaa-0000-4000-8000-000000000002")),
            f(&format!("{g1}/vp.bcfv"), vp.into()),
            f("evil/markup.bcf", "<Markup><Topic Guid=\"../../x\">".into()),
        ],
        e.now_ms,
        true,
    );
    let theirs = read_bcf(&z).unwrap();
    let t1 = theirs.topics.iter().find(|t| t.guid == g1).unwrap();
    assert!(
        theirs.version == "3.0"
            && theirs.topics.len() == 2
            && theirs.skipped == 1
            && t1.description == "Duct <clash> & beam"
            && t1.comments[0].text == "Fixed in model rev & 3 — ok"
            && t1.camera.map(|c| c[0]) == Some(12.5)
    );
    let sheets_b0 = sheets.clone();
    let plan = plan_import(&theirs, &refs, &guid_of, &reply_guids, &sheets_b0, "B:0", "Me");
    let up = &plan["updates"][0];
    assert!(
        plan["matched"] == 1
            && up["id"] == ms[0].id.as_str()
            && up["patch"]["status"] == "Completed"
            && up["patch"]["replies"].as_array().unwrap().len() == 2
            && up["patch"]["replies"][1]["author"] == "Ben"
    );
    let add = &plan["adds"][0];
    assert!(
        plan["added"] == 1
            && add["sheet"] == "B:1"
            && add["type"] == "note"
            && add["comment"].as_str().unwrap().contains("Duct clash\nDuct <clash> & beam")
            && add["status"] == "Open"
            && plan["unplaced"] == 1
    );
    assert_eq!(read_bcf(&[1, 2, 3]).unwrap_err(), "zip", "not a zip → refused");
}

#[test]
fn area_cut_outs() {
    use cc_markup::holes::{clean_holes, hole_problem};
    let outer = vec![[100.0, 100.0], [388.0, 100.0], [388.0, 244.0], [100.0, 244.0]];
    let hole = vec![[150.0, 150.0], [222.0, 150.0], [222.0, 222.0], [150.0, 222.0]];
    assert_eq!(hole_problem(&outer, &[], &hole), None, "a cut-out inside the area is accepted");
    assert_eq!(hole_problem(&outer, &[], &[[90.0, 150.0], [198.0, 150.0], [198.0, 198.0]]), Some("hole.err.outside"));
    assert_eq!(
        hole_problem(&outer, std::slice::from_ref(&hole), &[[160.0, 160.0], [250.0, 160.0], [250.0, 220.0], [160.0, 220.0]]),
        Some("hole.err.overlap")
    );
    assert_eq!(hole_problem(&outer, &[], &[[200.0, 150.0], [260.0, 210.0], [260.0, 150.0], [200.0, 210.0]]), Some("hole.err.cross"));
    assert_eq!(hole_problem(&outer, &[], &[[1.0, 1.0], [2.0, 2.0]]), Some("hole.err.points"));
    let (area, perimeter) = measure_area(&outer, Some(&quarter()), std::slice::from_ref(&hole));
    assert!(area == "112.00 sq ft" && perimeter == "64'-0\"", "128 − 16 = 112 sq ft; the perimeter runs round the cut-out too");
    let mut e = env();
    let am = mk(&mut e, json!({ "sheet": "H:0", "type": "area", "pts": outer, "holes": [hole], "author": AUTHOR }));
    let fill = shapes(&am, &ShapeEnv { cal: Some(quarter()), ..ShapeEnv::default() })
        .into_iter()
        .find_map(|i| if let Item::Path(p) = i { if p.fill.is_some() { Some(p) } else { None } } else { None })
        .unwrap();
    assert!(
        fill.rule == Some("evenodd") && fill.d.iter().filter(|s| matches!(s, cc_markup::shapes::Seg::M(..))).count() == 2,
        "one even-odd path"
    );
    assert_eq!(move_by(&am, 10.0, 5.0)["holes"][0][0], json!([160.0, 155.0]), "moving carries the cut-outs");
    let clip = make_clip(&e, &[&am], "", "").unwrap();
    assert_eq!(
        place_clip(&mut e, &clip, &PlaceOpts { sheet: "H:1".into(), delta: Some([1.0, 1.0]), ..PlaceOpts::default() })[0]
            .holes
            .as_ref()
            .unwrap()[0][0],
        [151.0, 151.0]
    );
    assert_eq!(
        clean_holes(Some(&json!([[[0, 0], [1, 0], [1, "x"]], [[0, 0], [1, 0], [1, 1]], "junk"]))).len(),
        1,
        "cut-outs from a file are checked"
    );
}

#[test]
fn sheet_links_and_callouts() {
    use cc_markup::link::{LinkSheet, callout_refs, link_target};
    let s = |k: &str, n: &str, t: &str| LinkSheet { key: k.into(), sheet_no: n.into(), title: t.into() };
    let sh = vec![
        s("L:0", "A-101", "A-101 — Plan"),
        s("L:1", "A-501", "A-501 — Details"),
        s("M:0", "A501", "A501 — Details rev"),
        s("L:2", "", "Cover"),
    ];
    let key = |l: &str| link_target(l, &sh).map(|i| sh[i].key.clone());
    assert!(
        key("a-501").as_deref() == Some("M:0") && key("Cover").as_deref() == Some("L:2") && key("Z-9").is_none(),
        "a link finds its sheet"
    );
    let refs = callout_refs("SEE 3/A-501 AND DETAIL 2 / A501. ALSO A-999 AND A-101.", &["A-101".into(), "A-501".into()]);
    let got: Vec<String> = refs.iter().map(|r| format!("{}>{}", r.r#ref, r.no)).collect();
    assert_eq!(got.join("|"), "3/A-501>A-501|2 / A501>A-501|A-101>A-101", "callouts read with or without a detail number");
}

#[test]
fn spaces_sketch_legend() {
    let mut e = env();
    let wing = mk(
        &mut e,
        json!({ "sheet": "P:0", "type": "space", "pts": [[0, 0], [600, 0], [600, 600], [0, 600]], "label": "East wing", "author": AUTHOR }),
    );
    let room = mk(
        &mut e,
        json!({ "sheet": "P:0", "type": "space", "pts": [[100, 100], [300, 100], [300, 300], [100, 300]], "label": "Kitchen", "author": AUTHOR }),
    );
    let in_room = mk(&mut e, json!({ "sheet": "P:0", "type": "cloudrect", "rect": [150, 150, 250, 250], "author": AUTHOR }));
    let in_wing = mk(&mut e, json!({ "sheet": "P:0", "type": "note", "pts": [[500, 500]], "author": AUTHOR }));
    let out = mk(&mut e, json!({ "sheet": "P:0", "type": "rect", "rect": [700, 700, 800, 800], "author": AUTHOR }));
    let bo = |m: &Markup| list_bounds(&shapes(m, &ShapeEnv::default()));
    let sp = [&wing, &room];
    assert!(
        space_of(&in_room, &sp, &bo) == "Kitchen"
            && space_of(&in_wing, &sp, &bo) == "East wing"
            && space_of(&out, &sp, &bo).is_empty()
            && space_of(&room, &sp, &bo).is_empty()
    );
    let sfn = |m: &Markup| space_of(m, &sp, &bo);
    let rows = markup_rows(&[&in_room, &out], &|_| Some(sheet_ref()), &|_| String::new(), Some(&sfn), &[]);
    assert!(
        rows[0].last() == Some(&Cell::S("Space".into()))
            && rows[1].last() == Some(&Cell::S("Kitchen".into()))
            && rows[2].last() == Some(&Cell::S(String::new()))
    );
    assert_eq!(markup_rows(&[&in_room], &|_| None, &|_| String::new(), None, &[])[0].len(), 17, "without spaces the CSV keeps its columns");
    let q = quarter();
    let close = |p: [f64; 2], x: [f64; 2]| (p[0] - x[0]).hypot(p[1] - x[1]) < 1e-6;
    assert!(close(sketch_point([100.0, 100.0], [500.0, 100.0], "16'", Some(&q), 0.0).unwrap(), [388.0, 100.0]));
    assert!(close(sketch_point([100.0, 100.0], [500.0, 100.0], "8 < 90", Some(&q), 0.0).unwrap(), [100.0, 244.0]));
    assert!(close(sketch_point([100.0, 100.0], [0.0, 0.0], "4'@0", Some(&q), 90.0).unwrap(), [100.0, 172.0]));
    assert!(close(
        sketch_point([0.0, 0.0], [1.0, 0.0], "1 m", Some(&preset_calibration("1:100", 100.0, "mm")), 0.0).unwrap(),
        [1000.0 / (25.4 / 72.0 * 100.0), 0.0]
    ));
    assert!(
        sketch_point([0.0, 0.0], [1.0, 0.0], "abc", Some(&q), 0.0) == Err("sketch.err.length")
            && sketch_point([0.0, 0.0], [1.0, 0.0], "4<x", Some(&q), 0.0) == Err("sketch.err.angle")
    );
    assert!(
        sketch_point([0.0, 0.0], [0.0, 0.0], "4", Some(&q), 0.0) == Err("sketch.err.direction")
            && sketch_point([0.0, 0.0], [1.0, 0.0], "4", None, 0.0) == Err("sketch.err.scale")
    );
    let lm = |e: &mut Env, t: &str, sheet: &str, extra: Value| {
        let mut f =
            obj(json!({ "sheet": sheet, "type": t, "rect": [10, 10, 50, 50], "pts": [[10, 10], [60, 60], [90, 10]], "author": AUTHOR }));
        f.extend(obj(extra));
        make_markup(e, &f)
    };
    let ms = [
        lm(&mut e, "cloudrect", "G:0", json!({})),
        lm(&mut e, "cloudrect", "G:0", json!({})),
        lm(&mut e, "cloudrect", "G:0", json!({ "subject": "Question", "style": { "color": "#1e48c8" } })),
        lm(&mut e, "count", "G:0", json!({ "label": "Outlets" })),
        lm(&mut e, "rect", "G:1", json!({})),
        lm(&mut e, "space", "G:0", json!({ "label": "Room" })),
    ];
    let refs: Vec<&Markup> = ms.iter().collect();
    let name = |m: &Markup| cc_markup::model::type_name(&m.kind);
    let rows = legend_rows(&refs, Some("G:0"), "sheet", &name);
    assert_eq!(rows.iter().map(|r| format!("{}={}", r.label, r.n)).collect::<Vec<_>>(), vec!["Count=3", "Cloud=2", "Question=1"]);
    assert!(legend_rows(&refs, Some("G:0"), "set", &name).iter().any(|r| r.label == "Rectangle"), "the whole set when asked");
    assert_ne!(rows_key(&rows), rows_key(&rows[1..]), "the rows key changes with the counts");
}

#[test]
fn custom_columns() {
    let cols = clean_columns(Some(
        &json!([{ "key": "c1", "name": "Trade", "type": "choice", "choices": ["Mechanical", "Electrical", "Mechanical", ""] }, { "key": "c2", "name": " Cost ", "type": "number" }, { "key": "c3", "name": "Due", "type": "date" }, { "key": "x", "name": "bad key" }, { "key": "c4", "name": "" }, "junk"]),
    ));
    assert!(
        cols.len() == 3 && cols[0].choices == vec!["Mechanical", "Electrical"] && cols[1].name == "Cost",
        "columns from a file are checked"
    );
    assert!(
        field_ok(&cols[1], "1,250.5").is_empty()
            && field_ok(&cols[1], "lots") == "col.err.number"
            && field_ok(&cols[2], "2026-11-01").is_empty()
            && field_ok(&cols[2], "Nov 1") == "col.err.date"
            && field_ok(&cols[0], "Plumbing") == "col.err.choice"
            && field_ok(&cols[0], "").is_empty()
    );
    let mut e = env();
    let mut ms: Vec<Markup> = ["20", "3", "", "100"]
        .iter()
        .map(|v| {
            mk(&mut e, json!({ "type": "note", "pts": [[0, 0]], "fields": if v.is_empty() { json!({}) } else { json!({ "c2": v }) } }))
        })
        .collect();
    ms.sort_by(|a, b| compare_by(&cols[1], a, b).partial_cmp(&0.0).unwrap());
    assert_eq!(
        ms.iter().map(|m| m.fields.get("c2").cloned().unwrap_or_else(|| "-".into())).collect::<Vec<_>>().join(","),
        "3,20,100,-",
        "numbers sort as numbers, empty values last"
    );
    let f = clean_fields(Some(&json!({ "c1": "Electrical", "c2": "\u{7} 12 ", "zz": "no", "c9": "" })));
    assert_eq!(f.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>(), vec!["c1=Electrical", "c2=12"], "field values cleaned");
    assert_eq!(new_column_key(&cols), "c4", "a new column gets an unused key");
}

#[test]
fn compare_by_vector_paths() {
    use cc_markup::vecdiff::{clusters, diff_segments};
    let rect =
        |x: f64, y: f64, w: f64, h: f64| vec![[x, y, x + w, y], [x + w, y, x + w, y + h], [x + w, y + h, x, y + h], [x, y + h, x, y]];
    let circle = |cx: f64, cy: f64, r: f64| {
        (0..16)
            .map(|i| {
                let a0 = f64::from(i) / 16.0 * std::f64::consts::TAU;
                let a1 = f64::from(i + 1) / 16.0 * std::f64::consts::TAU;
                [cx + r * a0.cos(), cy + r * a0.sin(), cx + r * a1.cos(), cy + r * a1.sin()]
            })
            .collect::<Vec<_>>()
    };
    let mut base = rect(100.0, 100.0, 400.0, 300.0);
    base.push([600.0, 100.0, 900.0, 100.0]);
    base.extend(circle(1000.0, 600.0, 50.0));
    let mut added = base.clone();
    added.push([600.0, 400.0, 800.0, 400.0]);
    let d1 = diff_segments(&base, &added, 1.5, None, 2.0);
    let add_len: f64 = d1.added.iter().map(|s| (s[2] - s[0]).hypot(s[3] - s[1])).sum();
    assert!(
        d1.removed.is_empty() && (add_len - 200.0).abs() < 6.0 && d1.added.iter().all(|s| (s[1] - 400.0).abs() < 1.0),
        "an added line is found, and only it"
    );
    let mut shorter = rect(100.0, 100.0, 400.0, 300.0);
    shorter.push([600.0, 100.0, 750.0, 100.0]);
    shorter.extend(circle(1000.0, 600.0, 50.0));
    let d2 = diff_segments(&base, &shorter, 1.5, None, 2.0);
    let rem_x: Vec<f64> = d2.removed.iter().flat_map(|s| [s[0], s[2]]).collect();
    assert!(
        d2.added.is_empty()
            && rem_x.iter().copied().fold(f64::INFINITY, f64::min) > 745.0
            && rem_x.iter().copied().fold(0.0, f64::max) <= 900.1,
        "a shortened line shows only its missing part"
    );
    let moved: Vec<[f64; 4]> = base.iter().map(|s| [s[0] + 6.0, s[1] - 4.0, s[2] + 6.0, s[3] - 4.0]).collect();
    let raw = diff_segments(&base, &moved, 1.5, None, 2.0);
    let al = diff_segments(&base, &moved, 1.5, Some(&[1.0, 0.0, 0.0, 1.0, 6.0, -4.0]), 2.0);
    assert!(
        raw.removed.len() > 10 && al.removed.is_empty() && al.added.is_empty(),
        "with the compare alignment a moved sheet shows no changes"
    );
    let mut segs = d1.added.clone();
    segs.extend([[100.0, 700.0, 110.0, 700.0], [112.0, 700.0, 120.0, 700.0], [400.0, 800.0, 401.0, 800.0]]);
    let boxes = clusters(&segs, 18.0, 6.0);
    assert!(
        boxes.len() == 2 && boxes.iter().any(|b| b[0] >= 599.0 && b[2] <= 801.0) && boxes.iter().any(|b| b[0] == 100.0 && b[2] == 120.0),
        "changes group into boxes; a speck is left out"
    );
}

#[test]
fn bookmarks_data() {
    let items = clean_bookmarks(
        Some(
            &json!([{ "title": "Architectural", "page": 0, "children": [{ "title": "A-101 — Plan", "page": 1, "children": [] }, { "title": "A-102 — Détails", "page": 2, "children": [] }] }, { "title": "Mechanical", "page": 3, "children": [] }]),
        ),
        0,
    );
    assert_eq!(count_bookmarks(&items), 4, "counted with children");
    let fs = from_sheets(&[(0.0, "A-101 — Plan".into())]);
    assert!(fs.len() == 1 && fs[0].title == "A-101 — Plan" && fs[0].page == 0.0 && fs[0].children.is_empty(), "built from the sheets");
    assert_eq!(
        clean_bookmarks(Some(&json!([{ "title": " x\u{7} ", "page": "no" }, { "title": "" }, 5])), 0).len(),
        1,
        "bookmarks from data are checked"
    );
    assert_eq!(clean_bookmarks(Some(&json!([{ "title": "a", "page": 2.5 }])), 0)[0].page, -1.0);
}

#[test]
fn blank_pages_data() {
    let want = [
        ("letter", 612.0, 792.0),
        ("tabloid", 792.0, 1224.0),
        ("arch-d", 1728.0, 2592.0),
        ("arch-e", 2592.0, 3456.0),
        ("a4", 595.28, 841.89),
        ("a3", 841.89, 1190.55),
        ("a1", 1683.78, 2383.94),
    ];
    let ps = papers();
    assert!(
        ps.iter().zip(want.iter()).all(|(p, w)| p.key == w.0 && near(p.w, w.1, 0.01) && near(p.h, w.2, 0.01)),
        "seven papers, sizes in points"
    );
    let c = clean_blank(Some(&json!({ "paper": "zz", "orient": "up", "grid": "hex", "pages": 99 })));
    assert!(c.paper == "letter" && c.orient == "portrait" && c.grid == "none" && c.pages == 20.0);
    assert!(
        clean_blank(Some(&json!({ "pages": 0 }))).pages == 1.0
            && clean_blank(None).pages == 1.0
            && clean_blank(Some(&json!({ "pages": "x" }))).pages == 1.0,
        "pages 1 to 20"
    );
    let gc = grid_content(612.0, 792.0, "square", false);
    assert!(
        gc.matches(" m ").count() == 72
            && gc.contains("0.62 G 0.5 w")
            && gc.contains("36 36 m 36 756 l")
            && gc.contains("36 756 m 576 756 l"),
        "square grid: 1/4 in pitch on a 1/2 in margin"
    );
    let dc = grid_content(595.28, 841.89, "dot", true);
    assert!(dc.matches(" re").count() == 39 * 56 && grid_content(612.0, 792.0, "none", false).is_empty(), "metric dot grid: 39 × 56 on A4");
    assert!(
        blank_info(Some("{\"paper\":\"evil\",\"grid\":\"square\"}")).unwrap().paper == "letter"
            && blank_info(Some("not json")).is_none()
            && blank_info(Some("")).is_none()
            && blank_info(Some("[1]")).unwrap().grid == "none"
    );
    assert_eq!(untitled_name(&[], 0.0).0, "Untitled-1.pdf");
    assert_eq!(untitled_name(&["Untitled-1.pdf".into(), "plans.pdf".into()], 0.0).0, "Untitled-2.pdf");
    assert_eq!(untitled_name(&["Untitled-4 (marked up).pdf".into()], 2.0).0, "Untitled-5.pdf");
    assert_eq!(untitled_name(&[], 3.0).0, "Untitled-4.pdf");
    let keys = [
        "new.title",
        "new.text",
        "new.paper",
        "new.paper.inch",
        "new.paper.metric",
        "new.orient",
        "new.grid",
        "new.grid.hint",
        "new.pages",
        "new.err.pages",
        "new.ok",
        "tool.new",
        "sheets.add_blank",
        "pages.done.add_blank",
    ];
    assert!(keys.iter().all(|k| cc_markup::i18n::has(k)), "the New dialog's words are in the language file");
}

#[test]
fn language_files() {
    for t in [
        "pen",
        "highlight",
        "line",
        "arrow",
        "arc",
        "rect",
        "ellipse",
        "polyline",
        "polygon",
        "cloudrect",
        "cloud",
        "callout",
        "text",
        "note",
        "stamp",
        "measure",
        "angle",
        "area",
        "count",
        "space",
        "legend",
        "image",
        "foreign",
    ] {
        assert!(cc_markup::i18n::has(&format!("type.{t}")), "type.{t}");
    }
    assert_eq!(cc_markup::model::type_name("cloud"), "Cloud (freeform)");
    assert_eq!(cc_markup::model::stamp_label("Approved"), "Approved");
    assert_eq!(cc_markup::model::status_name("Accepted"), "Accepted");
    assert_eq!(stamp_color("Approved"), "#1e8c3a");
}

/// A style number saved as JSON null (a NaN in the JS) counts as 0 in arithmetic, as the JS's null does, and is
/// written back as null (regression: a FreeText re-saved from another program's DA wrapped per character).
#[test]
fn style_null_numbers() {
    let m = cc_markup::json::markup_from_value(
        &json!({ "id": "m1", "sheet": "f1:1", "type": "text", "rect": [305.75, 62, 505.75, 102], "style": { "color": "#ff0000", "width": 0, "fill": null, "fillOpacity": 0.25, "opacity": 0.3, "fontSize": null }, "text": "Their text" }),
    );
    assert_eq!(m.style.font_size, 0.0);
    let t = text_of(&shapes(&m, &ShapeEnv::default())).cloned().unwrap();
    assert_eq!(t.lines, vec!["Their text"], "one line, not one per character");
    assert!(cc_markup::json::markup_json(&m, true).contains("\"fontSize\":null"), "written back as null");
}
