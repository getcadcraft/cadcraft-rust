//! `cadcraft-shot recovery <scenario.json> <out-dir>`: recovery copies driven headless for forks/tools/recoverdiff.mjs
//! (no GPU). Windows share one in-memory store; the clock is fixed per step. Steps (a JSON array of objects):
//!
//!   {"window": "s1"}  start (or switch to) the window with that session id     {"t": ms}  the clock, every window
//!   {"open": path}  open a DWG / DXF for editing     {"new": true}     {"cmd": "LINE"}  typed, Enter   {"cmds": [...]}
//!   {"undo": true}  {"save": "2018" | "dxf2000" | …}  {"frame": true}  the recovery step (write what is due, sweep)
//!   {"crash": true}  the window stops without a word   {"exit": true}  quits in order (pending copies written)
//!   {"offer": true}  the dialog → its lines      {"recover": i} {"discard": i} {"discardAll": true} {"later": true}
//!   {"dump": true}   the store: each record and its data (written to <out-dir>/<n>.bin), and the open drawing as
//!                    DWG 2018 (created now, GUIDs in sequence) to <out-dir>/<n>-editor.dwg
//!
//! Output: one JSON object per step that says something, as a JSON array on stdout.

use std::cell::RefCell;
use std::rc::Rc;

use cc_ui_egui::{Params, UiApp};
use serde_json::{Value, json};

struct Win {
    id: String,
    app: UiApp,
}

pub fn run(args: &[String]) -> Result<(), String> {
    let (Some(script), Some(out)) = (args.get(1), args.get(2)) else {
        return Err("usage: cadcraft-shot recovery <scenario.json> <out-dir>".into());
    };
    let steps: Vec<Value> =
        serde_json::from_slice(&std::fs::read(script).map_err(|e| format!("{script}: {e}"))?).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let store: Rc<RefCell<cc_recovery::MemState>> = Rc::default();
    let running: Rc<RefCell<Vec<String>>> = Rc::default();
    let mut wins: Vec<Win> = Vec::new();
    let mut cur: Option<usize> = None;
    let mut t = 0.0f64;
    let mut results: Vec<Value> = Vec::new();
    let mut dumps = 0usize;
    let p = |name: Option<String>, index: Option<usize>| Params { name, index, ..Default::default() };
    for (k, s) in steps.iter().enumerate() {
        let get = |key: &str| s.get(key);
        if let Some(v) = get("t").and_then(Value::as_f64) {
            t = v;
            for w in &mut wins {
                w.app.recovery.clock = Some(t);
            }
        }
        if let Some(id) = get("window").and_then(Value::as_str) {
            cur = Some(match wins.iter().position(|w| w.id == id) {
                Some(i) => i,
                None => {
                    let mut app = UiApp::default();
                    app.services.recovery = Some(cc_recovery::memory_on(Rc::clone(&store)));
                    app.recovery.session = Some(id.to_string());
                    app.recovery.clock = Some(t);
                    app.recovery.guid_seq = true;
                    app.recovery.auto_offer = false;
                    let r = Rc::clone(&running);
                    app.recovery.alive = Some(Rc::new(move || r.borrow().clone()));
                    running.borrow_mut().push(id.to_string());
                    // the store starts with the window (0.5: new Autosave() at page load)
                    cc_ui_egui::recover::frame(&mut app);
                    wins.push(Win { id: id.to_string(), app });
                    wins.len() - 1
                }
            });
            continue;
        }
        let Some(i) = cur else { continue };
        let Some(w) = wins.get_mut(i) else { continue };
        let app = &mut w.app;
        let mut res: Option<Value> = None;
        let t0 = std::time::Instant::now();
        let r: Result<(), String> = (|| {
            if let Some(path) = get("open").and_then(Value::as_str) {
                let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
                let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                app.open_bytes(&name, &bytes);
                if let Some(e) = app.error.clone() {
                    return Err(e);
                }
                app.run("file.edit", &Params::default())?;
            } else if get("new").is_some() {
                app.run("file.new", &Params::default())?;
            } else if let Some(c) = get("cmd").and_then(Value::as_str) {
                app.run("cmd.line", &p(Some(c.into()), None))?;
            } else if let Some(cs) = get("cmds").and_then(Value::as_array) {
                for c in cs.iter().filter_map(Value::as_str) {
                    app.run("cmd.line", &p(Some(c.into()), None))?;
                }
            } else if get("undo").is_some() {
                app.run("edit.undo", &Params::default())?;
            } else if let Some(f) = get("save").and_then(Value::as_str) {
                app.services.save = Some(Box::new(|n, _| Ok(Some(n.to_string()))));
                app.run("file.save", &p(Some(f.into()), None))?;
            } else if get("frame").is_some() {
                app.run("recovery.sweep", &Params::default())?;
                res = Some(json!({"step": k, "frame": true, "writeMs": app.recovery.last_write_ms}));
            } else if get("offer").is_some() {
                app.run("recovery.offer", &Params::default())?;
                res = Some(json!({"step": k, "dialog": cc_ui_egui::recover::dialog_lines(app)}));
            } else if let Some(n) = get("recover").and_then(Value::as_u64) {
                app.run("recovery.recover", &p(None, usize::try_from(n).ok()))?;
                let rows: Vec<Value> = app.recovery.dlg.as_ref().map(|d| d.rows.iter().map(|r| json!(r.msg)).collect()).unwrap_or_default();
                let meta = app.recovery.meta().map(cc_ui_egui::recover::Meta::to_json);
                res = Some(
                    json!({"step": k, "recovered": true, "rows": rows, "open": app.recovery.dlg.is_some(), "meta": meta, "tab": app.recovery.tab_id()}),
                );
            } else if let Some(n) = get("discard").and_then(Value::as_u64) {
                app.run("recovery.discard", &p(None, usize::try_from(n).ok()))?;
            } else if get("discardAll").is_some() {
                app.run("recovery.discard_all", &Params::default())?;
            } else if get("later").is_some() {
                app.run("recovery.later", &Params::default())?;
            } else if get("exit").is_some() {
                app.run("recovery.exit", &Params::default())?;
            }
            Ok(())
        })();
        if let Err(e) = r {
            res = Some(json!({"step": k, "error": e}));
        }
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        if get("crash").is_some() || get("exit").is_some() {
            let id = w.id.clone();
            running.borrow_mut().retain(|x| *x != id);
            wins.remove(i);
            cur = None;
        }
        if get("dump").is_some() {
            let mut recs = Vec::new();
            for (key, val) in store.borrow().items.iter() {
                if let Some(k2) = key.strip_prefix("i:") {
                    let rec: Value = serde_json::from_slice(val).unwrap_or(Value::Null);
                    let file = format!("{dumps}-{}.bin", recs.len());
                    let data =
                        store.borrow().items.iter().find(|(kk, _)| *kk == format!("d:{k2}")).map(|(_, v)| v.clone()).unwrap_or_default();
                    std::fs::write(std::path::Path::new(out).join(&file), &data).map_err(|e| e.to_string())?;
                    recs.push(json!({"rec": rec, "data": file}));
                }
            }
            let mut ed_file = Value::Null;
            if let Some(ed) = cur.and_then(|i| wins.get(i)).and_then(|w| w.app.editor.as_ref()) {
                let o = cc_io_cad::dwg_save::SaveOpts {
                    version: "2018".into(),
                    created_ms: Some(t),
                    guid_sequence: true,
                    ..Default::default()
                };
                if let Ok(s) = cc_io_cad::dwg_save::save_dwg(&ed.eng.doc(), &o) {
                    let f = format!("{dumps}-editor.dwg");
                    std::fs::write(std::path::Path::new(out).join(&f), s.bytes).map_err(|e| e.to_string())?;
                    ed_file = json!({"file": f, "dirty": ed.eng.doc().dirty(), "name": ed.name});
                }
            }
            dumps += 1;
            res = Some(json!({"step": k, "store": recs, "editor": ed_file}));
        }
        if let Some(mut v) = res {
            if let Some(o) = v.as_object_mut() {
                o.insert("ms".into(), json!(ms));
            }
            results.push(v);
        }
    }
    println!("{}", Value::Array(results));
    Ok(())
}

/// `--seed-recovery N`: copies of N drawings left by a window that "crashed" (for screenshots of the dialog), in `store`.
pub fn seed(store: &Rc<RefCell<cc_recovery::MemState>>, n: usize, now: f64) {
    let names = ["Kitchen plan", "Site layout", "Bracket detail", "Drawing1"];
    for k in 0..n {
        let mut app = UiApp::default();
        app.services.recovery = Some(cc_recovery::memory_on(Rc::clone(store)));
        app.recovery.session = Some(format!("seed{k}"));
        // no heartbeat: the window counts as stopped at once
        app.recovery.alive = Some(Rc::new(Vec::new));
        let ago = [240_000.0, 7_400_000.0, 3.0 * 86_400_000.0, 30_000.0];
        let at = now - ago.get(k).copied().unwrap_or(1.0e6);
        app.recovery.clock = Some(at - 2000.0);
        app.recovery.auto_offer = false;
        let _ = app.run("file.new", &Params::default());
        if let Some(ed) = app.editor.as_mut() {
            ed.name = names.get(k).copied().unwrap_or("Drawing1").to_string();
        }
        for i in 0..(k + 1) * 40 {
            let x = (i % 20) as f64 * 10.0;
            let y = (i / 20) as f64 * 10.0;
            for l in ["CIRCLE".to_string(), format!("{x},{y}"), "4".into()] {
                let _ = app.run("cmd.line", &Params { name: Some(l), ..Default::default() });
            }
        }
        app.recovery.clock = Some(at);
        cc_ui_egui::recover::frame(&mut app);
    }
}
