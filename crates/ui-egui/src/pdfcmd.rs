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
        _ => Err(format!("unknown command {id}")),
    }
}
