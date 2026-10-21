//! Timings of TakeoffCraft's M9.20–M9.23 actions (cadcraft-shot tkactbench; forks/tools/tkactbench.mjs runs the frozen
//! 0.5 JS the same way): on an old sheet and its revision at 1/8" = 1'-0", 200 count marks, 50 runs with an arc and an
//! area with a cut-out, then copy + paste in place, a corner drag, a typical group of every mark, 100 notes, Find
//! changes, Move takeoff here, the marked PDF, the 3D scene + GLB, delete every mark. Best of `runs`, ms, as JSON.

use std::collections::HashSet;
use std::fmt::Write as _;

use serde_json::{Value, json};

use crate::pdfview::PdfView;

type Act = fn(&mut PdfView, &str, &str) -> Result<(), String>;

fn fresh(a: (&str, &[u8]), b: (&str, &[u8])) -> Result<(PdfView, String, String), String> {
    let mut v = PdfView::open(a.0, a.1)?;
    v.mode = "takeoff";
    v.add_file(b.0, b.1)?;
    v.go_file("f1", 0);
    v.sync_takeoff();
    let ka = v.takeoff.cur_sheet.clone();
    let kb = cc_takeoff::model::sheet_key(v.others.first().map_or("", |f| f.hash()), 0.0);
    v.takeoff.set_preset(&ka, "a1/8", 1.0);
    let cnt = v.takeoff.add_item("Columns", "count").unwrap_or_default();
    let run = v.takeoff.add_item("Walls", "linear").unwrap_or_default();
    let ar = v.takeoff.add_item("Slab", "area").unwrap_or_default();
    let k2 = ka.clone();
    v.takeoff.tk.change("bench", move |s| {
        let mut list = Vec::new();
        for i in 0..200u32 {
            let (x, y) = (100.0 + f64::from(i % 20) * 50.0, 100.0 + f64::from(i / 20) * 50.0);
            list.push(json!({ "id": format!("b{i}"), "item": cnt, "sheet": k2, "kind": "count", "pts": [{ "x": x, "y": y }], "holes": [] }));
        }
        for i in 0..50u32 {
            let (x, y) = (120.0 + f64::from(i % 10) * 100.0, 130.0 + f64::from(i / 10) * 120.0);
            list.push(json!({ "id": format!("r{i}"), "item": run, "sheet": k2, "kind": "linear", "pts": [{ "x": x, "y": y, "b": 0.4 }, { "x": x + 50.0, "y": y }, { "x": x + 50.0, "y": y + 50.0 }], "holes": [] }));
        }
        list.push(json!({ "id": "a0", "item": ar, "sheet": k2, "kind": "area", "pts": [{ "x": 50, "y": 50 }, { "x": 1150, "y": 50 }, { "x": 1150, "y": 740 }, { "x": 50, "y": 740 }], "holes": [[{ "x": 500, "y": 300 }, { "x": 600, "y": 300 }, { "x": 600, "y": 400 }]] }));
        if let Some(Value::Array(m)) = s.get_mut("marks") {
            m.extend(list);
        }
        Ok(cc_takeoff::model::Ch::Commit(()))
    })?;
    Ok((v, ka, kb))
}

fn all(v: &PdfView, ka: &str) -> Vec<String> {
    v.takeoff
        .tk
        .marks()
        .iter()
        .filter(|m| m.get("sheet").and_then(Value::as_str) == Some(ka))
        .filter_map(|m| m.get("id").and_then(Value::as_str).map(str::to_string))
        .collect()
}

/// `cadcraft-shot tkactbench <old.pdf> <new.pdf> [--runs N]`.
pub fn bench(a: (&str, &[u8]), b: (&str, &[u8]), runs: usize) -> Result<String, String> {
    let acts: [(&str, Act); 9] = [
        ("copy_paste", |v, ka, _| {
            let ids = all(v, ka);
            v.takeoff.set_selection(&ids);
            v.takeoff.copy_selection(true);
            v.takeoff.start_paste(ka, true);
            let _ = cc_takeoff::model::quantities(&v.takeoff.tk.state, &cc_takeoff::model::QOpts::default());
            Ok(())
        }),
        ("drag_corner", |v, ka, _| {
            v.takeoff.set_sel(Some("a0".into()));
            v.takeoff.tool = "select";
            v.takeoff.press_drag(ka, [1150.0, 740.0], [1160.0, 760.0]);
            let _ = cc_takeoff::model::quantities(&v.takeoff.tk.state, &cc_takeoff::model::QOpts::default());
            Ok(())
        }),
        ("group", |v, ka, _| {
            let ids = all(v, ka);
            v.takeoff.tk.add_group(Some(&json!("Unit")), Some(&json!(4)), &ids)?;
            let _ = cc_takeoff::model::base_estimate(&v.takeoff.tk.state);
            Ok(())
        }),
        ("notes_100", |v, ka, _| {
            for i in 0..100u32 {
                let x = 10.0 + f64::from(i);
                let pts = if i % 2 == 1 { json!([{ "x": x, "y": 10 }]) } else { json!([{ "x": x, "y": 10 }, { "x": x + 30.0, "y": 40 }]) };
                let f = json!({ "kind": if i % 2 == 1 { "note" } else { "cloud" }, "sheet": ka, "pts": pts, "text": format!("n{i}") });
                v.takeoff.tk.add_note(f.as_object().ok_or("note")?)?;
            }
            Ok(())
        }),
        ("find_changes", |v, ka, _| {
            v.go_file("f2", 0);
            v.sync_takeoff();
            v.takeoff.compare_start(ka);
            v.takeoff_find_changes().map(|_| ()).ok_or_else(|| v.takeoff.message.clone())
        }),
        ("move_takeoff", |v, ka, kb| {
            let ids = all(v, ka);
            let flagged: HashSet<String> = ["b1", "r2"].iter().map(|s| (*s).to_string()).collect();
            let sc = v.takeoff.tk.scale_of(ka).cloned();
            let mv = |pts: &[Value]| -> Vec<Value> {
                let num = |p: &Value, k: &str| p.get(k).and_then(Value::as_f64).unwrap_or(0.0);
                let ps: Vec<cc_takeoff::geom::P> =
                    pts.iter().map(|p| cc_takeoff::geom::P::arc(num(p, "x"), num(p, "y"), num(p, "b"))).collect();
                cc_takeoff::overlay::move_pts(&ps, &cc_takeoff::overlay::IDENTITY).iter().map(crate::takeofftools::pt_json).collect()
            };
            v.takeoff.tk.carry_marks(&ids, kb, &mv, &flagged, sc.as_ref(), None)?;
            Ok(())
        }),
        ("marked_pdf", |v, _, _| v.takeoff_export("pdf", 0.0).map(|_| ())),
        ("scene3d_glb", |v, ka, _| {
            let p = v.cur.pages.first().ok_or("page")?;
            let view = cc_takeoff::scene3d::View { transform: Some(p.transform), width: p.width, height: p.height, step: None };
            let sc = cc_takeoff::scene3d::build_scene(&v.takeoff.tk.state, ka, &view)?;
            let _ = cc_io_takeoff::glb::scene_glb(&sc, "Takeoff");
            Ok(())
        }),
        ("delete_all", |v, ka, _| {
            let ids = all(v, ka);
            v.takeoff.tk.remove_marks(&ids).map(|_| ())
        }),
    ];
    let mut out = String::from("{");
    for (i, (name, f)) in acts.iter().enumerate() {
        let mut best = f64::INFINITY;
        for _ in 0..runs.max(1) {
            let (mut v, ka, kb) = fresh(a, b)?;
            let t0 = web_time::Instant::now();
            f(&mut v, &ka, &kb)?;
            best = best.min(t0.elapsed().as_secs_f64() * 1000.0);
        }
        let _ = write!(out, "{}\"{name}\":{best:.3}", if i > 0 { "," } else { "" });
    }
    out.push('}');
    Ok(out)
}
