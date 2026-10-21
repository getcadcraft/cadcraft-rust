//! The sheet view's `takeoff.*` and `markup.*` commands beyond the first few (which stay in lib.rs): each is the action a
//! panel widget or a click runs, reachable headless (`cadcraft-shot --run ID=NAME`), the control channel and scripts.
//!
//! TakeoffCraft:
//! - `takeoff.select` name = item id: the current item (Draw uses it).
//! - `takeoff.set` name = `field=text`: a field of the current item's settings (see `TakeoffMode::item_field`).
//! - `takeoff.comp` name = `add` | `remove:<id>` | `<id>:<field>=<text>`: the item's assembly.
//! - `takeoff.itemact` name = up | down | delete | dup.
//! - `takeoff.tool` name = select | draw | cutout | circle | cutcircle | note | cloud | highlight.
//! - `takeoff.note` name = add:TEXT (the asked text of a new markup; `\n` = a new line) | cancel | select:<id> |
//!   text:TEXT | status:open|answered|closed | remove | open-only:on|off.
//! - `takeoff.group` name = new:NAME|REPEAT (a typical group of the selected marks) | cancel | rename:<id>:NAME |
//!   repeat:<id>:N | select:<id> | delete:<id>.
//! - `takeoff.arc` name = on | off (no name: toggles): arc mode (R).
//! - `takeoff.drag` name = `x1,y1:x2,y2`: a press, the pointer moved, the release (Select drags a corner of the
//!   selected mark or the middle of one of its arcs).
//! - `takeoff.compare` name = `<file>:<page>` (the older sheet, e.g. "f1:0", over the open one) | align (the next four
//!   clicks: old, new, old, new) | find (Find changes) | move (Move takeoff here; a scales-differ question is answered
//!   "Move them anyway") | close. `takeoff.flag` name = next (the next flagged mark); `takeoff.mark=flag.checked`.
//! - `takeoff.view3d` name = on | off (no name: toggles) | pick:<mark id>[,shift] (a click on its object) | glb |
//!   stack:on|off | levels:<file>:<page>=<length>;… (a blank length takes the sheet out of the stack).
//! - `takeoff.edit` name = copy | cut | paste | paste-in-place | duplicate | select-all | select-none | delete | escape.
//! - `takeoff.deduct` name = on | off (no name: toggles).
//! - `takeoff.click` name = `x,y[,shift]` (PDF user space of the open sheet): a click with the tool.
//! - `takeoff.mark` name = `select:<id>` | `field=text` (see `TakeoffMode::mark_field`).
//! - `takeoff.area` name = see `TakeoffMode::area_edit`.
//! - `takeoff.undo`, `takeoff.redo`.
//!
//! MarkupCraft (points in PDF user space of the open sheet):
//! - `pdf.zoom` factor = screen pixels per PDF point; `markup.clock` factor = ms (a fixed clock for ids and dates;
//!   `app.tz` fixes the zone of the local-time fields: zip dates, list dates).
//! - `markup.down | move | up | dbl` name = `x,y[,shift]`: the pointer with the current tool (dbl = a double-click's
//!   second press); `markup.enter`, `markup.esc`.
//! - `markup.type` name = the editor's text; `markup.commit` (Ctrl+Enter).
//! - `markup.style` name = `key=value` (color, width, fill on|off|#hex, fillOpacity, opacity, fontSize): the
//!   selection, or the tool's next markups; `markup.prop` name = `field=value` (status, subject, layer, text,
//!   comment, label, stamp).
//! - `pdf.file` name = a file id ("f2"): open that file of the plan set (another PDF joins by opening it).
//! - `markup.compare` name = the old sheet's key ("f1:0") against the open sheet, `cloud`, `stop`, `mode:picture|lines`,
//!   `nudge:x,y` (steps), `step:N` (pt), `align` (the next four clicks), `reset`, `balance:0–100`, `set:f1>f2` (compare
//!   two sets), `set-cloud`.
//! - `markup.export=bcf` (the listed markups as BCF 2.1); `markup.bcf` name = PATH (import a BCF file).
//! - `markup.cal` name = the calibrate line's real distance; `markup.delete`, `markup.redo`, `markup.save`.
//! - `markup.edit` name = copy | cut | paste | paste-in-place | paste-at:x,y (the pointer's place) | duplicate |
//!   select-all | select-none.
//! - `markup.reply` name = the reply's text (the selected markup's thread); `markup.tab` name = the right panel's tab.
//! - `markup.chest` name = save:NAME (the selected markup's look) | use:I | remove:I | import:PATH | export.
//! - `markup.cutout` name = start (in the selected area) | remove; `markup.space` name = start | name:NAME (names the
//!   outline just finished) | cancel; `markup.legend` name = x,y (its top-right corner; else the view's);
//!   `markup.link` name = follow (the selected markup's link) | back; `markup.punch` name = add | assignee=… | due=… |
//!   status=… | remove (the selected markup) | export.
//! - `markup.picture` name = PATH (a PNG or JPEG for the stamp tool); `markup.signature` name = PATH (a PNG used as the
//!   signature, as the signature pad makes it); `markup.photo` name = `PATH|caption` (a photo on the thread).
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
    // commands that read a file by its path, through the host
    let file_arg = match id {
        "markup.picture" | "markup.signature" | "markup.bcf" => Some(name.as_str()),
        "markup.photo" => Some(name.split_once('|').map_or(name.as_str(), |(a, _)| a)),
        "markup.chest" => name.strip_prefix("import:"),
        _ => None,
    };
    if let Some(path) = file_arg {
        let read = app.services.read_path.as_mut().ok_or_else(|| format!("{id}: reading files is not available here"))?;
        let f = read(path)?;
        let p = app.pdf.as_mut().ok_or_else(|| format!("{id}: open a PDF first"))?;
        match id {
            "markup.picture" => p.set_picture(&f.name, &f.bytes),
            "markup.signature" => {
                let src =
                    cc_markup::imagestamp::clean_image(&format!("data:image/png;base64,{}", cc_markup::imagestamp::b64_encode(&f.bytes)));
                p.set_signature(&src)?;
            }
            "markup.photo" => p.photo_reply(&f.name, &f.bytes, name.split_once('|').map_or("", |(_, c)| c))?,
            "markup.bcf" => p.import_bcf(&f.name, &f.bytes),
            _ => p.import_chest(&f.name, &f.bytes),
        }
        if p.tools.prefs_dirty {
            p.tools.prefs_dirty = false;
            app.mc_prefs = p.tools.prefs_json();
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
    if let Some(p) = app.pdf.as_mut() {
        p.sync_takeoff();
        // the 3D view follows every change, as 0.5's model listener rebuilds it
        if p.d3.is_some() && id.starts_with("takeoff.") && id != "takeoff.view3d" {
            p.update3d();
        }
    }
    if let Some(p) = app.pdf.as_mut()
        && p.tools.prefs_dirty
    {
        p.tools.prefs_dirty = false;
        app.mc_prefs = p.tools.prefs_json();
    }
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
    p.sync_takeoff();
    let tk = &mut p.takeoff;
    if id.starts_with("takeoff.") {
        // each command says its own status line (or none)
        tk.message.clear();
    }
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
                "circle" => "circle",
                "cutcircle" => "cutcircle",
                "note" => "note",
                "cloud" => "cloud",
                "highlight" => "highlight",
                _ => {
                    return Err(format!(
                        "takeoff.tool: unknown tool {name} (select, draw, cutout, circle, cutcircle, note, cloud, highlight)"
                    ));
                }
            };
            tk.set_tool(tool);
            Ok(())
        }
        "takeoff.arc" => {
            let on = match name.as_str() {
                "on" => true,
                "off" => false,
                _ => !tk.arc,
            };
            tk.toggle_arc(on);
            Ok(())
        }
        "takeoff.drag" => {
            let (a, b) = name.split_once(':').ok_or("takeoff.drag: name = x1,y1:x2,y2 (PDF user space)")?;
            let (a, b) = (pt(a, id)?, pt(b, id)?);
            let key = tk.cur_sheet.clone();
            tk.press_drag(&key, a, b);
            Ok(())
        }
        "takeoff.note" => tk.note_edit(&name),
        "takeoff.group" => tk.group_edit(&name),
        "takeoff.compare" => p.takeoff_compare(&name),
        "takeoff.view3d" => p.takeoff_3d(&name),
        "takeoff.flag" => p.takeoff_flag(&name),
        "takeoff.edit" => {
            let key = tk.cur_sheet.clone();
            tk.edit(&key, &name)
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
            // name = "x,y" with ",shift" for Shift held
            let shift = name.ends_with(",shift");
            let q = pt(name.trim_end_matches(",shift"), id)?;
            let key = tk.cur_sheet.clone();
            tk.click_mod(&key, q, shift);
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
        "takeoff.undo" | "takeoff.redo" => {
            tk.history(id == "takeoff.redo");
            Ok(())
        }
        "markup.compare" => match name.as_str() {
            // name = the old sheet's key ("f1:0"), cloud (Cloud the changes) or stop, and the line-up
            "cloud" => p.compare_cloud(),
            "stop" => {
                p.compare_stop();
                Ok(())
            }
            "align" => {
                p.compare_align();
                Ok(())
            }
            "reset" => p.compare_reset(),
            "set-cloud" => p.cloud_set(),
            n if n.starts_with("mode:") => p.compare_mode(n.trim_start_matches("mode:")),
            n if n.starts_with("step:") => {
                p.cmp.step = n
                    .trim_start_matches("step:")
                    .parse::<f64>()
                    .ok()
                    .filter(|x| x.is_finite() && *x > 0.0)
                    .ok_or("markup.compare: step:N (pt)")?;
                Ok(())
            }
            n if n.starts_with("balance:") => {
                let b = n
                    .trim_start_matches("balance:")
                    .parse::<f64>()
                    .ok()
                    .filter(|x| x.is_finite())
                    .ok_or("markup.compare: balance:0–100")?;
                p.cmp.balance = (b / 100.0).clamp(0.0, 1.0);
                Ok(())
            }
            n if n.starts_with("nudge:") => {
                let q = pt(n.trim_start_matches("nudge:"), id)?;
                p.compare_nudge(q[0], q[1])
            }
            n if n.starts_with("set:") => {
                let (o, nw) = n.trim_start_matches("set:").split_once('>').ok_or("markup.compare: set:OLD>NEW (file ids)")?;
                p.compare_sets(o, nw)
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
        "markup.down" | "markup.up" | "markup.move" | "markup.dbl"
            if p.dlg.is_some() && matches!(p.dlg, Some(crate::markupedit::Dialog::SpaceName { .. })) =>
        {
            Err(format!("{id}: name the space first (markup.space=name:NAME)"))
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
            p.take_space();
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
                "chest" => "chest",
                "punch" => "punch",
                "compare" => "compare",
                _ => return Err(format!("markup.tab: unknown tab {name} (props, list, punch, chest, compare)")),
            };
            Ok(())
        }
        "markup.cutout" => match name.as_str() {
            "start" => p.start_cutout(),
            "remove" => p.remove_cutouts(),
            _ => Err(format!("markup.cutout: unknown action {name} (start, remove)")),
        },
        "markup.space" => {
            p.take_space();
            match name.as_str() {
                "start" => {
                    p.start_space();
                    Ok(())
                }
                "cancel" => {
                    p.dlg = None;
                    Ok(())
                }
                n => {
                    let v =
                        n.strip_prefix("name:").ok_or_else(|| format!("markup.space: unknown action {n} (start, name:NAME, cancel)"))?;
                    let Some(crate::markupedit::Dialog::SpaceName { pts, .. }) = p.dlg.clone() else {
                        return Err("markup.space: no space outline is waiting for its name".into());
                    };
                    p.add_space(pts, v)?;
                    p.dlg = None;
                    Ok(())
                }
            }
        }
        "markup.legend" => {
            let at = if name.is_empty() { None } else { Some(pt(&name, id)?) };
            p.add_legend(at);
            Ok(())
        }
        "markup.link" => match name.as_str() {
            "follow" => {
                let sel = p.tools.selected().map(str::to_string).ok_or("markup.link: select one markup")?;
                p.follow_link(&sel)
            }
            "back" => {
                p.go_back();
                Ok(())
            }
            _ => Err(format!("markup.link: unknown action {name} (follow, back)")),
        },
        "markup.punch" => match name.as_str() {
            "add" => {
                let sel = p.tools.sel.clone();
                p.add_to_punch(&sel);
                Ok(())
            }
            "export" => {
                p.export_punch();
                Ok(())
            }
            other => {
                let sel = p.tools.selected().map(str::to_string).ok_or("markup.punch: select one markup")?;
                let (f, v) = other.split_once('=').unwrap_or((other, ""));
                p.punch_edit(&sel, f, v)
            }
        },
        "markup.chest" => {
            let (k, arg) = name.split_once(':').unwrap_or((name.as_str(), ""));
            let idx = || arg.parse::<usize>().map_err(|_| format!("markup.chest: {k}:I takes a tool number"));
            match k {
                "save" => p.save_to_chest(arg),
                "use" => p.use_tool(idx()?),
                "remove" => p.remove_tool(idx()?),
                "export" => {
                    p.export_chest();
                    Ok(())
                }
                _ => Err(format!("markup.chest: unknown action {k} (save, use, remove, import, export)")),
            }
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
            if name == "bcf" {
                p.export_bcf();
                return Ok(());
            }
            if let Some(f) = p.export_list(&name)? {
                p.outbox.push(f);
            }
            Ok(())
        }
        _ => Err(format!("unknown command {id}")),
    }
}
