//! The sheet view's `takeoff.*` and `markup.*` commands beyond the first few (which stay in lib.rs): each is the action a
//! panel widget or a click runs, reachable headless (`cadcraft-shot --run ID=NAME`), the control channel and scripts.
//!
//! TakeoffCraft:
//! - `takeoff.select` name = item id: the current item (Draw uses it).
//! - `takeoff.set` name = `field=text`: a field of the current item's settings (see `TakeoffMode::item_field`).
//! - `takeoff.comp` name = `add` | `remove:<id>` | `<id>:<field>=<text>`: the item's assembly.
//! - `takeoff.itemact` name = up | down | delete | dup.
//! - `takeoff.tool` name = select | draw | cutout.
//! - `takeoff.deduct` name = on | off (no name: toggles).
//! - `takeoff.click` name = `x,y` (PDF user space of the open sheet): a click with the tool.
//! - `takeoff.mark` name = `select:<id>` | `field=text` (see `TakeoffMode::mark_field`).
//! - `takeoff.area` name = see `TakeoffMode::area_edit`.
//! - `takeoff.undo`, `takeoff.redo`.
//!
//! MarkupCraft (points in PDF user space of the open sheet):
//! - `pdf.zoom` factor = screen pixels per PDF point; `markup.clock` factor = ms (a fixed clock for ids and dates).
//! - `markup.down | move | up | dbl` name = `x,y[,shift]`: the pointer with the current tool (dbl = a double-click's
//!   second press); `markup.enter`, `markup.esc`.
//! - `markup.type` name = the editor's text; `markup.commit` (Ctrl+Enter).
//! - `markup.style` name = `key=value` (color, width, fill on|off|#hex, fillOpacity, opacity, fontSize): the
//!   selection, or the tool's next markups; `markup.prop` name = `field=value` (status, subject, layer, text,
//!   comment, label, stamp).
//! - `pdf.file` name = a file id ("f2"): open that file of the plan set (another PDF joins by opening it).
//! - `markup.compare` name = the old sheet's key ("f1:0") against the open sheet, `cloud`, `stop`.
//! - `markup.cal` name = the calibrate line's real distance; `markup.delete`, `markup.redo`, `markup.save`.
//! - `markup.edit` name = copy | cut | paste | paste-in-place | paste-at:x,y (the pointer's place) | duplicate |
//!   select-all | select-none.
//! - `markup.reply` name = the reply's text (the selected markup's thread); `markup.tab` name = the right panel's tab.
//! - `markup.rename` name = `number|title` of the open sheet (both empty: what the file says).
//! - `markup.pages` name = rotate-cw | rotate-ccw | move-up | move-down | insert-blank | add-blank | delete:PAGES |
//!   extract:PAGES (saved) | insert-pdf:PATH (PAGES as "1-3, 5").
//! - `markup.filter` name = `key=value` (sheet: "" | current | a sheet key; author, status, type, layer, space, field,
//!   sort); `markup.jump` name = a row of the list (0-based) or a markup id; `markup.export` name = csv | xlsx | pdf.

use crate::{Params, UiApp};

fn pt(spec: &str, what: &str) -> Result<[f64; 2], String> {
    let v: Vec<f64> = spec.split(',').filter_map(|x| x.trim().parse().ok()).filter(|x: &f64| x.is_finite()).collect();
    match v[..] {
        [x, y] => Ok([x, y]),
        _ => Err(format!("{what}: name = x,y (PDF user space)")),
    }
}

/// Run one of the sheet view's commands.
pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    let name = params.name.clone().unwrap_or_default();
    if id == "markup.save" {
        // Save marked-up PDF through the host's save
        let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64);
        let outs = app.pdf.as_mut().ok_or("markup.save: open a PDF first")?.save_files(now)?;
        let save = app.services.save.as_mut().ok_or("markup.save: saving is not available here")?;
        for (file, bytes) in outs {
            if let Some(to) = save(&file, bytes)? {
                app.message = format!("Saved {to}");
            }
        }
        return Ok(());
    }
    if id == "markup.pages"
        && let Some(path) = name.strip_prefix("insert-pdf:")
    {
        // the picked PDF, read through the host
        let read = app.services.read_path.as_mut().ok_or("markup.pages: reading files is not available here")?;
        let f = read(path)?;
        let p = app.pdf.as_mut().ok_or("markup.pages: open a PDF first")?;
        p.pages_op("insert-pdf", "", Some((&f.name, &f.bytes)))?;
        return Ok(());
    }
    let r = run_pdf(app, id, params, &name);
    // files the command made (exports, extracted pages) go to the host's save
    if let Some(p) = app.pdf.as_mut() {
        let outs = std::mem::take(&mut p.outbox);
        if !outs.is_empty() {
            let save = app.services.save.as_mut().ok_or_else(|| format!("{id}: saving is not available here"))?;
            for (file, bytes) in outs {
                save(&file, bytes)?;
            }
        }
    }
    r
}

fn run_pdf(app: &mut UiApp, id: &str, params: &Params, name: &str) -> Result<(), String> {
    let name = name.to_string();
    let p = app.pdf.as_mut().ok_or_else(|| format!("{id}: open a PDF first"))?;
    let tk = &mut p.takeoff;
    match id {
        "takeoff.select" => {
            if tk.tk.item(&name).is_none() {
                return Err(format!("takeoff.select: no item {name}"));
            }
            tk.item = Some(name);
            tk.pts.clear();
            tk.ded.clear();
            if tk.tool == "select" {
                tk.tool = "draw";
            }
            Ok(())
        }
        "takeoff.set" => {
            let (f, v) = name.split_once('=').ok_or("takeoff.set: name = field=text")?;
            tk.item_field(f, v)
        }
        "takeoff.comp" => tk.comp_edit(&name),
        "takeoff.itemact" => tk.item_action(&name),
        "takeoff.tool" => {
            let tool = match name.as_str() {
                "select" => "select",
                "draw" => "draw",
                "cutout" => "cutout",
                _ => return Err(format!("takeoff.tool: unknown tool {name} (select, draw, cutout)")),
            };
            tk.set_tool(tool);
            Ok(())
        }
        "takeoff.deduct" => {
            let on = match name.as_str() {
                "on" => true,
                "off" => false,
                _ => !tk.deduct,
            };
            tk.toggle_deduct(on);
            Ok(())
        }
        "takeoff.click" => {
            let q = pt(&name, id)?;
            let key = tk.sheet(p.page);
            tk.click(&key, q);
            Ok(())
        }
        "takeoff.mark" => {
            if let Some(m) = name.strip_prefix("select:") {
                tk.select((!m.is_empty()).then(|| m.to_string()));
                return Ok(());
            }
            let (f, v) = name.split_once('=').unwrap_or((name.as_str(), ""));
            tk.mark_field(f, v)
        }
        "takeoff.area" => tk.area_edit(&name),
        "takeoff.undo" => {
            tk.message = tk.tk.undo().unwrap_or_default();
            Ok(())
        }
        "takeoff.redo" => {
            tk.message = tk.tk.redo().unwrap_or_default();
            Ok(())
        }
        "markup.compare" => match name.as_str() {
            // name = the old sheet's key ("f1:0"), cloud (Cloud the changes) or stop
            "cloud" => p.compare_cloud(),
            "stop" => {
                p.compare_stop();
                Ok(())
            }
            old => p.compare_start(old),
        },
        "pdf.file" => {
            // open the project's file `name` ("f2", …), its first sheet
            if p.cur.id != name && !p.others.iter().any(|f| f.id == name) {
                return Err(format!("pdf.file: no file {name}"));
            }
            p.go_file(&name, 0);
            Ok(())
        }
        "pdf.zoom" => {
            let z = params.factor.filter(|z| z.is_finite() && *z > 0.0).ok_or("pdf.zoom: factor = screen pixels per PDF point")?;
            p.set_zoom(z);
            Ok(())
        }
        "markup.clock" => {
            let ms = params.factor.filter(|z| z.is_finite() && *z >= 0.0).ok_or("markup.clock: factor = ms since 1970")?;
            p.clock = Some(ms);
            Ok(())
        }
        "markup.down" | "markup.up" | "markup.move" | "markup.dbl" => {
            // name = "x,y" (PDF user space) with ",shift" for Shift held
            let parts: Vec<&str> = name.split(',').collect();
            let q = pt(&parts.iter().take(2).copied().collect::<Vec<_>>().join(","), id)?;
            let shift = parts.contains(&"shift");
            let s = p.screen_of(q);
            match id {
                "markup.down" => p.gesture_down(q, s, shift, false),
                "markup.dbl" => p.gesture_down(q, s, shift, true),
                "markup.up" => p.gesture_up(q, s, shift),
                _ => p.tools.mv(&p.store, q, s, shift),
            }
            Ok(())
        }
        "markup.enter" => {
            p.tick();
            let key = p.sheet_key();
            p.tools.enter(&mut p.store, &key);
            Ok(())
        }
        "markup.esc" => {
            p.tools.cancel();
            p.tools.cancel_edit();
            Ok(())
        }
        "markup.type" => {
            if p.tools.editing.is_none() {
                return Err("markup.type: no text is being edited".into());
            }
            p.tools.edit_input(&name);
            Ok(())
        }
        "markup.commit" => {
            p.tick();
            let key = p.sheet_key();
            p.tools.commit_edit(&mut p.store, &key);
            Ok(())
        }
        "markup.style" => {
            let (k, v) = name.split_once('=').ok_or("markup.style: name = key=value")?;
            p.tick();
            p.tools.style_edit(&mut p.store, k, v)
        }
        "markup.prop" => {
            let (k, v) = name.split_once('=').ok_or("markup.prop: name = field=value")?;
            p.prop(k, v)
        }
        "markup.cal" => {
            // the calibrate line's real distance (in the sheet's unit, else feet-and-inches), then Set scale
            let key = p.sheet_key();
            if let Some(u) = p.store.calibrations.get(&key).map(|c| c.unit.clone()) {
                p.tools.cal_unit = u;
            }
            p.tools.cal_text = name;
            if p.tools.apply_calibration(&mut p.store, &key) { Ok(()) } else { Err(p.tools.message.clone()) }
        }
        "markup.delete" => {
            p.tools.delete_selected(&mut p.store);
            Ok(())
        }
        "markup.redo" => {
            p.history(true);
            Ok(())
        }
        "markup.edit" => {
            match name.as_str() {
                "copy" => {
                    p.copy("copy");
                }
                "cut" => p.cut(),
                "paste" => p.paste("offset", None),
                "paste-in-place" => p.paste("place", None),
                "duplicate" => p.duplicate(),
                "select-all" => p.select_all(),
                "select-none" => p.tools.sel.clear(),
                other => {
                    let at = other.strip_prefix("paste-at:").ok_or_else(|| format!("markup.edit: unknown action {other}"))?;
                    let q = pt(at, id)?;
                    p.paste("pointer", Some(q));
                }
            }
            Ok(())
        }
        "markup.reply" => p.reply(&name),
        "markup.tab" => {
            // the right panel's tab
            p.tab = match name.as_str() {
                "props" => "props",
                "list" => "list",
                "compare" => "compare",
                _ => return Err(format!("markup.tab: unknown tab {name} (props, list, compare)")),
            };
            Ok(())
        }
        "markup.rename" => {
            let (n, t) = name.split_once('|').unwrap_or((name.as_str(), ""));
            p.rename_sheet(n, t);
            Ok(())
        }
        "markup.pages" => {
            let (k, arg) = name.split_once(':').unwrap_or((name.as_str(), ""));
            p.pages_op(k, arg, None)
        }
        "markup.filter" => {
            let (k, v) = name.split_once('=').ok_or("markup.filter: name = key=value")?;
            p.set_filter(k, v)
        }
        "markup.jump" => {
            let target = match name.parse::<usize>() {
                Ok(i) => p.list_rows().get(i).map(|m| m.id.clone()).ok_or_else(|| format!("markup.jump: the list has no row {i}"))?,
                Err(_) => name.clone(),
            };
            p.jump_to(&target)
        }
        "markup.export" => {
            if let Some(f) = p.export_list(&name)? {
                p.outbox.push(f);
            }
            Ok(())
        }
        _ => Err(format!("unknown command {id}")),
    }
}
