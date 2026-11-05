//! BimCraft's Edit actions on the IFC model (bimcraft.js "Edit menu": `whyNot`, `act`, `copyPicked`, `copyElements`,
//! `startPaste`, `pastePlacement`, `pasteElementsAt`, `commitPaste`, `pasteInPlace`, `deletePicked`, Select all / none)
//! and Swap item… (`libraryBlock`, `doSwap`), on `cc_bim::draw::clip` (clipboard.js) and the editor's `placeLike`,
//! `swapItem`, `removeElements`, `snapshot`, `pasteElements`.
//!
//! One placed library item is copied as a placement clip (it pastes into any model: like its source in the model it
//! came from, else from the kept library item); anything else, several elements, or a Cut as an elements clip whose
//! snapshot this window keeps (it pastes into the model it came from). Paste follows the pointer on the plan (the
//! placing tool of M7.15: R turns, Tab snaps to a wall, a click pastes, Esc stops); Paste in place goes beside the
//! original, or where it was cut from. The clip lives in this window's memory; its text goes to the system clipboard.
//! Ctrl/Shift + click adds an element to the pick or takes it out; Select all picks what is shown.
//!
//! Everything is a command: `bim.act {name: copy | cut | paste | paste-in-place | duplicate | delete | select-all |
//! select-none}`, `bim.pick {name: "<id>" | "+<id>" | ""}` (bimedit.rs), `bim.swap.pick` (Swap item…; the Library's "Swap … to
//! this" is `bim.lib.swap`).

use std::sync::Arc;

use cc_bim::draw::clip::{self, Box3, Clip, ClipMeta, ElRow, ElementsCtx, PlacementCtx};
use cc_bim::draw::place::{PasteOpts, PlaceOpts, PlacementRow, Placer, Snap};
use serde_json::Value as J;

use crate::bimplace::{At, LibEntry};
use crate::bimtools::bt;
use crate::{Params, UiApp};

/// The Edit commands (id, label, shortcut, params doc).
pub const COMMANDS: &[(&str, &str, &str, &str)] = &[
    (
        "bim.act",
        "Edit the IFC model",
        "Ctrl+C / Ctrl+X / Ctrl+V / Ctrl+Shift+V / Ctrl+D / Delete / Ctrl+A / Esc",
        "{name: \"copy\" | \"cut\" | \"paste\" | \"paste-in-place\" | \"duplicate\" | \"delete\" | \"select-all\" | \"select-none\"} → as the toolbar's Edit buttons (an action that cannot act says why)",
    ),
    ("bim.swap.pick", "Swap item…", "", "{} → the picked placed item is the one the Library's \"Swap … to this\" changes"),
];

/// A clip and, for an elements clip, the snapshot this window keeps.
#[derive(Clone, Debug)]
pub struct Held {
    pub clip: Clip,
    pub snap: Option<Arc<Snap>>,
    pub pasted: u32,
}

/// Classes that are not copied or deleted (bimcraft.js SPATIAL), in any letter case.
fn spatial(cls: &str) -> bool {
    let up = cls.to_ascii_uppercase();
    matches!(
        up.as_str(),
        "IFCPROJECT"
            | "IFCSITE"
            | "IFCBUILDING"
            | "IFCBUILDINGSTOREY"
            | "IFCSPACE"
            | "IFCFACILITY"
            | "IFCFACILITYPART"
            | "IFCBRIDGE"
            | "IFCROAD"
            | "IFCRAILWAY"
            | "IFCMARINEFACILITY"
            | "IFCSPATIALZONE"
            | "IFCEXTERNALSPATIALELEMENT"
    )
}

fn status(app: &mut UiApp, text: String) {
    app.message = text;
    app.error = None;
}

fn now_iso(app: &UiApp) -> String {
    let now = app.bimh.now();
    let ms = if now.is_finite() { now.rem_euclid(1000.0).floor() } else { 0.0 };
    format!("{}.{:0>3}Z", cc_bim::draw::editor::iso_seconds(now), cc_bim::js::num_str(ms))
}

/// This open model's session tag (a clip copied from it pastes "like" its source).
pub fn model_tag(app: &mut UiApp) -> String {
    if app.bimh.model_tag.is_empty() {
        app.bimh.tag_seq = app.bimh.tag_seq.wrapping_add(1);
        let n = cc_clock::now_ms();
        let n = if n.is_finite() && n > 0.0 { n.floor() } else { 0.0 };
        app.bimh.model_tag = format!("t{}-{}", cc_bim::js::num_str(n), app.bimh.tag_seq);
    }
    app.bimh.model_tag.clone()
}

/// What is picked now: the elements Ctrl / Shift + click gathered, else the one picked.
pub fn picks(app: &UiApp) -> Vec<f64> {
    if app.bimh.picks.is_empty() { app.picked_product().into_iter().collect() } else { app.bimh.picks.clone() }
}

/// The class of a line of the model being edited ("IfcWall").
fn class_of(app: &mut UiApp, id: f64) -> String {
    let Ok(d) = crate::bimedit::doc(app) else { return String::new() };
    let now = d.ed.now(id);
    d.ed.api.line(now).ok().flatten().map(|l| l.class.to_string()).unwrap_or_default()
}

/// The class of a picked element without making the editor (the toolbar asks every frame; opening a file never pays
/// for the editor): the 3D view's own list first, the model being edited else.
fn class_cheap(app: &mut UiApp, id: f64) -> String {
    if app.bimh.ed.is_none()
        && let (Some((_, products)), groups) = (app.ifc3.as_ref(), &app.ifc_groups)
        && let Some(i) = products.iter().position(|p| *p == id)
        && let Some((_, ty)) = groups.get(i)
    {
        return ty.clone();
    }
    class_of(app, id)
}

/// `editable()`: the picked elements this model can delete or copy (storeys, spaces and the project cannot).
fn editable(app: &mut UiApp) -> Vec<f64> {
    picks(app).into_iter().filter(|&id| !spatial(&class_cheap(app, id))).collect()
}

fn ifc_open(app: &UiApp) -> bool {
    app.ifc3_bytes().is_some()
}

/// The element a placement clip was copied from, when it is still in this open model as that same item.
fn clip_source(app: &mut UiApp, c: &Clip) -> Option<PlacementRow> {
    if c.kind != "placement" || c.model != app.bimh.model_tag || app.bimh.model_tag.is_empty() {
        return None;
    }
    let d = app.bimh.ed.as_mut()?;
    let id = d.ed.now(c.id);
    let rows = Placer::new(&mut d.ed.api).ok()?.placements().ok()?;
    rows.into_iter().find(|p| p.id == id && clip::lib_key(p) == c.key)
}

/// `whyNot(name)`: '' = it can act now.
pub fn why_not(app: &mut UiApp, name: &str) -> String {
    let ifc = ifc_open(app);
    let dwg = !ifc && crate::viewer::active(app);
    let placing = app.bimh.draw.placing.is_some();
    let n_picks = picks(app).len();
    match name {
        "copy" => {
            if dwg {
                return bt("edit.dwg.notbuilt", &[]);
            }
            if !ifc {
                return bt("edit.need.ifc", &[]);
            }
            if n_picks == 0 {
                return bt("edit.need.pick.one", &[]);
            }
            if editable(app).is_empty() { bt("edit.refused.spatial", &[]) } else { String::new() }
        }
        "paste" | "paste-in-place" => {
            if !ifc {
                return bt("edit.need.ifc.paste", &[]);
            }
            let Some(h) = app.bimh.clip.clone() else { return bt("edit.paste.nothing", &[]) };
            let c = &h.clip;
            if c.kind == "elements" {
                if c.model != model_tag(app) {
                    let f = if c.file.is_empty() { "?".to_string() } else { c.file.clone() };
                    return bt("edit.paste.othermodel", &[("file", &f)]);
                }
                return if placing { bt("edit.finish.placing", &[]) } else { String::new() };
            }
            if clip_source(app, c).is_none() && c.item.is_none() {
                return if c.file.is_empty() {
                    bt("edit.paste.noitem.other", &[("name", &c.name)])
                } else {
                    bt("edit.paste.noitem", &[("name", &c.name), ("file", &c.file)])
                };
            }
            if placing { bt("edit.finish.placing", &[]) } else { String::new() }
        }
        "duplicate" | "cut" | "delete" => {
            if dwg {
                return bt("edit.dwg.notbuilt", &[]);
            }
            if !ifc {
                return bt("edit.need.ifc", &[]);
            }
            if n_picks == 0 {
                return bt("edit.need.pick", &[]);
            }
            if editable(app).is_empty() {
                return bt("edit.refused.spatial", &[]);
            }
            if placing { bt("edit.finish.placing", &[]) } else { String::new() }
        }
        "select-all" => {
            if ifc {
                String::new()
            } else {
                bt("edit.need.ifc", &[])
            }
        }
        "select-none" => {
            if ifc && (n_picks > 0 || placing) {
                String::new()
            } else {
                bt("edit.nothing.picked", &[])
            }
        }
        _ => String::new(),
    }
}

/// Run an Edit command.
pub fn run(app: &mut UiApp, id: &str, p: &Params) -> Result<(), String> {
    let name = p.name.clone().unwrap_or_default();
    match id {
        "bim.act" => act(app, &name),
        "bim.swap.pick" => swap_pick(app),
        other => Err(format!("unknown command {other}")),
    }
}

/// Nothing picked (and the pick list emptied).
pub fn pick_none(app: &mut UiApp) {
    app.bimh.picks.clear();
    app.select_product_none();
}

/// Ctrl / Shift + click: an element added to the pick, or taken out.
pub fn toggle_pick(app: &mut UiApp, id: f64) {
    let mut list = picks(app);
    match list.iter().position(|x| *x == id) {
        Some(i) => {
            list.remove(i);
        }
        None => list.push(id),
    }
    set_picks(app, list);
}

/// The pick set to `list` (one: its properties; several: how many, by class).
pub fn set_picks(app: &mut UiApp, list: Vec<f64>) {
    match list.as_slice() {
        [] => pick_none(app),
        [one] => {
            app.bimh.picks.clear();
            app.select_product(*one);
        }
        _ => {
            app.select_product_none();
            const SEL: [u8; 4] = [70, 130, 240, 255];
            let set: std::collections::HashSet<u64> = list.iter().map(|x| x.to_bits()).collect();
            if let (Some((scene, _)), Some((_, products))) = (app.scene3.as_mut(), app.ifc3.as_ref()) {
                scene.set_tints(&|k| products.get(k).is_some_and(|p| set.contains(&p.to_bits())).then_some(SEL));
            }
            app.bimh.picks = list;
        }
    }
}

/// `showMany()`: several elements picked — how many, by class (Properties shows one element at a time).
pub fn many_panel(app: &mut UiApp, ui: &mut egui::Ui) -> bool {
    if app.bimh.picks.len() < 2 {
        return false;
    }
    let t = app.tokens;
    let list = app.bimh.picks.clone();
    let mut by: Vec<(String, usize)> = Vec::new();
    for id in &list {
        let c = class_of(app, *id);
        let c = c.strip_prefix("Ifc").unwrap_or(&c).to_string();
        let c = if c.is_empty() { bt("pick.many.unknown", &[]) } else { c };
        match by.iter_mut().find(|x| x.0 == c) {
            Some(e) => e.1 += 1,
            None => by.push((c, 1)),
        }
    }
    by.sort_by_key(|x| std::cmp::Reverse(x.1));
    ui.label(
        egui::RichText::new(bt("pick.many", &[("n", &list.len().to_string())]))
            .strong()
            .family(crate::theme::bold_family())
            .color(t.text_strong),
    );
    ui.label(egui::RichText::new(bt("pick.many.hint", &[])).small().color(t.text_dim));
    ui.label(egui::RichText::new(bt("pick.many.classes", &[])).strong().family(crate::theme::bold_family()));
    egui::Grid::new("bim_many").num_columns(2).show(ui, |ui| {
        for (c, n) in by {
            ui.label(c);
            ui.label(crate::bimedit::grouped(n));
            ui.end_row();
        }
    });
    true
}

/// `act(name)`
fn act(app: &mut UiApp, name: &str) -> Result<(), String> {
    // drawn walls, doors and windows (model-ui.js `edit`): their own delete; copy not yet
    if crate::bimplan::edit_drawn(app, name)? {
        return Ok(());
    }
    let why = why_not(app, name);
    match name {
        "select-none" => {
            if app.bimh.draw.placing.is_some() {
                crate::bimplace::stop(app);
                status(app, bt("place.stopped", &[]));
                return Ok(());
            }
            pick_none(app);
            Ok(())
        }
        "copy" => {
            if why.is_empty() {
                copy_picked(app, false, false)?;
            } else {
                status(app, why);
            }
            Ok(())
        }
        "paste" | "paste-in-place" => {
            if !why.is_empty() {
                status(app, why);
                return Ok(());
            }
            let Some(h) = app.bimh.clip.clone() else { return Ok(()) };
            if name == "paste" {
                start_paste(app, &h.clip, false);
                Ok(())
            } else {
                paste_in_place(app, &h.clip)
            }
        }
        "duplicate" => {
            if !why.is_empty() {
                status(app, why);
                return Ok(());
            }
            if copy_picked(app, true, false)?
                && let Some(h) = app.bimh.clip.clone()
            {
                start_paste(app, &h.clip, true);
            }
            Ok(())
        }
        "select-all" => {
            if !why.is_empty() {
                status(app, why);
                return Ok(());
            }
            if app.bimh.draw.placing.is_some() {
                crate::bimplace::stop(app);
            }
            let keys = shown_products(app);
            let n = keys.len();
            set_picks(app, keys);
            status(app, bt("edit.selectall.done", &[("n", &n.to_string())]));
            Ok(())
        }
        "delete" | "cut" => {
            if !why.is_empty() {
                status(app, why);
                return Ok(());
            }
            delete_picked(app, name == "cut")
        }
        "" => Err("bim.act: copy | cut | paste | paste-in-place | duplicate | delete | select-all | select-none".into()),
        _ => {
            status(app, if why.is_empty() { bt("edit.notbuilt", &[]) } else { why });
            Ok(())
        }
    }
}

/// Every element shown in the 3D view (`viewer.shownKeys()`), each once.
fn shown_products(app: &UiApp) -> Vec<f64> {
    let Some((_, products)) = app.ifc3.as_ref() else { return Vec::new() };
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (i, p) in products.iter().enumerate() {
        if app.instance_hidden3(i) {
            continue;
        }
        if seen.insert(p.to_bits()) {
            out.push(*p);
        }
    }
    out
}

/// `rememberItem(entry)`: items placed, swapped in or updated in this window (Paste in another model).
pub fn remember_item(app: &mut UiApp, e: &LibEntry) {
    let key = match (e.item.get("uuid").and_then(J::as_str).filter(|s| !s.is_empty()), e.item.get("version")) {
        (Some(u), v) => format!("{u}@{}", v.map_or_else(|| "undefined".to_string(), json_string)),
        (None, _) => format!("sha:{}", e.sha256),
    };
    let lib = if !e.library.is_empty() {
        e.library.clone()
    } else if e.item.get("uuid").and_then(J::as_str).is_some_and(|s| !s.is_empty()) {
        "librarycraft".into()
    } else {
        "file".into()
    };
    let kept = LibEntry { library: lib, ..e.clone() };
    app.bimh.lib_items.retain(|(k, _)| *k != key);
    app.bimh.lib_items.push((key, kept));
    // a window's worth: the oldest go first
    if app.bimh.lib_items.len() > 500 {
        app.bimh.lib_items.remove(0);
    }
}

fn json_string(v: &J) -> String {
    match v {
        J::String(s) => s.clone(),
        J::Number(n) => cc_bim::js::num_str(n.as_f64().unwrap_or(f64::NAN)),
        J::Null => "null".into(),
        J::Bool(b) => b.to_string(),
        _ => "[object Object]".into(),
    }
}

/// The text Copy puts on the system clipboard for one element (clipboard.js `elementText`), from Properties.
fn element_text(app: &UiApp) -> String {
    let groups = &app.sel_props;
    let row = |g: &cc_io_ifc::props::PropGroup, k: &str| g.rows.iter().find(|r| r.0 == k).map(|r| r.1.clone()).unwrap_or_default();
    let mut lines = Vec::new();
    if let Some(el) = groups.first() {
        let cls = row(el, "Class");
        let name = row(el, "Name");
        let name = if name.is_empty() { bt("clip.unnamed", &[]) } else { name };
        lines.push(bt("clip.text.head", &[("cls", cls.strip_prefix("Ifc").unwrap_or(&cls)), ("name", &name)]));
        for (label, k) in [
            ("Object type", "Object type"),
            ("Tag", "Tag"),
            (bt("clip.text.description", &[]).as_str(), "Description"),
            ("GlobalId", "GlobalId"),
        ] {
            let v = row(el, k);
            if !v.is_empty() {
                lines.push(bt("clip.text.row", &[("label", label), ("value", &v)]));
            }
        }
    }
    for g in groups.iter().skip(1) {
        let rows: Vec<_> = g.rows.iter().filter(|r| !r.1.is_empty()).collect();
        if rows.is_empty() {
            continue;
        }
        lines.push(String::new());
        lines.push(g.title.clone());
        for r in rows {
            lines.push(format!("  {}", bt("clip.text.row", &[("label", &r.0), ("value", &r.1)])));
        }
    }
    lines.join("\n") + "\n"
}

/// `copyPicked(quiet, isCut)`: one placed library item → a placement clip; anything else → an elements clip.
fn copy_picked(app: &mut UiApp, quiet: bool, is_cut: bool) -> Result<bool, String> {
    let now = now_iso(app);
    let list = picks(app);
    let placement = list.len() == 1 && app.bimh.picks.is_empty() && app.sel_props.iter().any(|g| g.title == "CadCraft_Placement");
    if is_cut || !placement {
        return copy_elements(app, &now, quiet, is_cut);
    }
    let Some(id) = list.first().copied() else { return Ok(false) };
    app.bimh.copy_text = Some(element_text(app));
    let tag = model_tag(app);
    let file = app.ifc_name().to_string();
    let d = crate::bimedit::doc(app)?;
    let mut pl = Placer::new(&mut d.ed.api).map_err(|e| e.0)?;
    let rows = pl.placements().unwrap_or_default();
    let Some(p) = rows.into_iter().find(|x| x.id == id) else {
        return Err(bt("edit.copy.noplacement", &[]));
    };
    let st = pl.info().ok().and_then(|ss| ss.into_iter().find(|s| Some(s.id) == p.storey));
    let unit = pl.w.unit_mm;
    let key = clip::lib_key(&p);
    let kept = app.bimh.lib_items.iter().find(|(k, _)| *k == key).map(|(_, e)| e.clone());
    let bx = match &kept {
        Some(e) => Some(clip::box_from_item(&e.item)),
        None => clip::box_from_equipment(&p, unit),
    };
    let c = clip::placement_clip(
        &p,
        &PlacementCtx {
            model: tag,
            file,
            storey_name: st.as_ref().map(|s| s.name.clone()).unwrap_or_default(),
            storey_elevation: st.as_ref().map(|s| s.elevation),
            item: kept.as_ref().map(|e| e.item.clone()),
            meta: kept.as_ref().map(|e| ClipMeta {
                library: e.library.clone(),
                sha256: e.sha256.clone(),
                source_file: e.source_file.clone(),
            }),
            bx,
            now,
        },
    );
    if !quiet {
        let msg = if c.item_number.is_empty() {
            bt("edit.copy.item", &[("name", &c.name)])
        } else {
            bt("edit.copy.item.no", &[("name", &c.name), ("no", &c.item_number)])
        };
        status(app, msg);
    }
    app.bimh.clip = Some(Held { clip: c, snap: None, pasted: 0 });
    Ok(true)
}

/// `copyElements(now, quiet, isCut)`: the snapshot kept in memory, the anchor and box from its frame.
fn copy_elements(app: &mut UiApp, now: &str, quiet: bool, is_cut: bool) -> Result<bool, String> {
    let list = editable(app);
    if list.is_empty() {
        status(app, bt("edit.refused.spatial", &[]));
        return Ok(false);
    }
    let tag = model_tag(app);
    let file = app.ifc_name().to_string();
    let single_text = (list.len() == 1 && app.bimh.picks.is_empty()).then(|| element_text(app));
    let d = crate::bimedit::doc(app)?;
    let snap = d.ed.snapshot(&list).map_err(|e| e.0)?;
    let mut pl = Placer::new(&mut d.ed.api).map_err(|e| e.0)?;
    let st = snap.storey.and_then(|sid| pl.info().ok().and_then(|ss| ss.into_iter().find(|s| s.id == sid)));
    let (mut ax, mut ay, mut rot, mut bx) = (0.0, 0.0, 0.0, None);
    if let Some(f) = snap.frame {
        (ax, ay, rot) = (f.x, f.y, f.rot);
        bx = Some(Box3 { x0: f.bx[0], y0: f.bx[1], z0: f.bx[2], x1: f.bx[3], y1: f.bx[4], z1: f.bx[5] });
    } else if let Some(w) = snap.elements.iter().find(|e| e.top).and_then(|e| e.world) {
        // no geometry to measure: where the first element is placed
        (ax, ay) = (w[12] * pl.w.unit_mm, w[13] * pl.w.unit_mm);
    }
    let mut rows = Vec::new();
    for &id in &list {
        let l = d.ed.api.line(d.ed.now(id)).ok().flatten();
        let name = l.as_ref().map(|l| l.a("Name").text_or_empty()).unwrap_or_default();
        let cls = l.as_ref().map(|l| l.class.to_string()).unwrap_or_default();
        let guid = l.as_ref().map(|l| l.a("GlobalId").text_or_empty()).unwrap_or_default();
        rows.push(ElRow { id, name, cls, guid });
    }
    let c = clip::elements_clip(
        &rows,
        &ElementsCtx {
            model: tag,
            file,
            now: now.to_string(),
            storey: snap.storey,
            storey_name: st.as_ref().map(|s| s.name.clone()).unwrap_or_default(),
            storey_elevation: st.as_ref().map(|s| s.elevation),
            x: ax,
            y: ay,
            rot,
            bx,
            cut: is_cut,
        },
    );
    app.bimh.copy_text = Some(single_text.unwrap_or_else(|| {
        rows.iter()
            .map(|r| {
                let name = if r.name.is_empty() { bt("prop.unnamed", &[]) } else { r.name.clone() };
                let guid = if r.guid.is_empty() { String::new() } else { format!(" ({})", r.guid) };
                format!("{}: {name}{guid}", r.cls.strip_prefix("Ifc").unwrap_or(&r.cls))
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    }));
    let n = list.len();
    app.bimh.clip = Some(Held { clip: c, snap: Some(Arc::new(snap)), pasted: 0 });
    if !quiet {
        status(app, bt("edit.copy.done", &[("n", &n.to_string())]));
    }
    Ok(true)
}

/// The storey a paste starts on (`pickStorey(paste, storeys, paste.model === modelTag)`): the plan shows it.
pub fn paste_storey(app: &mut UiApp, c: &Clip) {
    let tag = model_tag(app);
    let Ok(d) = crate::bimedit::doc(app) else { return };
    let Ok(ss) = Placer::new(&mut d.ed.api).and_then(|mut p| p.info()) else { return };
    let Some((id, how)) = clip::pick_storey(c, &ss, c.model == tag) else { return };
    if how == "first" {
        return;
    }
    let levels = app.bimh.draw.data.as_ref().map(|s| s.levels.clone()).unwrap_or_default();
    if let Some(i) = levels.iter().position(|l| l.id == id) {
        app.run_ui("bim.level", &Params { name: Some(i.to_string()), ..Default::default() });
    }
}

/// `startPaste(c, dup)`: the placing tool with the clip's ghost; each click pastes another copy until Esc.
fn start_paste(app: &mut UiApp, c: &Clip, dup: bool) {
    let item = clip::ghost_item(c);
    let meta = c.meta.clone().unwrap_or_default();
    let entry =
        LibEntry { item: Arc::new(item), name: c.name.clone(), library: meta.library, sha256: meta.sha256, source_file: meta.source_file };
    crate::bimplace::start_with(app, entry, Some(c.clone()));
    if app.bimh.draw.placing.is_none() {
        return;
    }
    let msg = if c.kind == "elements" {
        bt(if dup { "edit.duplicate.start" } else { "edit.paste.start" }, &[("n", &c.count.to_string())])
    } else {
        bt(if dup { "edit.dup.item" } else { "edit.paste.item" }, &[("name", &c.name)])
    };
    status(app, msg);
}

/// `pastePlacement(c, at)`: like its source in the model it came from, else from the kept library item → (id, number).
fn paste_placement(app: &mut UiApp, c: &Clip, storey: f64, x: f64, y: f64, rot: f64) -> Result<(f64, String), String> {
    let src = clip_source(app, c);
    let d = crate::bimedit::doc(app)?;
    let no = cc_bim::draw::place::next_item_number(&mut d.ed.api).map_err(|e| e.0)?;
    let opts =
        PlaceOpts { storey, x, y, rot, item_number: no.clone(), quantity: c.quantity, notes: c.notes.clone(), ..PlaceOpts::default() };
    if let Some(src) = src {
        let id = d.ed.place_like(src.id, opts, &c.name).map_err(|e| e.0)?;
        return Ok((id, no));
    }
    let Some(item) = c.item.clone() else {
        return Err(why_not(app, "paste"));
    };
    let h = c.bx.map_or(900.0, |b| cc_bim::draw::m::round(b.z1 - b.z0));
    let has_body = item.get("body").is_some_and(|b| !matches!(b, J::Null | J::Bool(false)));
    let meta = c.meta.clone().unwrap_or_default();
    let opts = PlaceOpts {
        library: meta.library.clone(),
        sha256: meta.sha256.clone(),
        source_file: meta.source_file.clone(),
        stand_in_height: if has_body { 0.0 } else { cc_bim::draw::m::max(1.0, h) },
        ..opts
    };
    let r = d.ed.place_item(item.clone(), opts).map_err(|e| e.0)?;
    remember_item(app, &LibEntry { item, name: c.name.clone(), library: meta.library, sha256: meta.sha256, source_file: meta.source_file });
    Ok((r.id, no))
}

/// `pasteElementsAt(c, at, how)`: copies of the snapshot moved from the clip's anchor to `at`.
fn paste_elements_at(app: &mut UiApp, storey: Option<f64>, x: f64, y: f64, rot: f64, how: &str) -> Result<(), String> {
    let tag = model_tag(app);
    let Some(h) = app.bimh.clip.clone() else { return Ok(()) };
    let c = &h.clip;
    let Some(snap) = h.snap.clone().filter(|_| c.model == tag) else {
        let w = why_not(app, "paste");
        status(app, w);
        return Ok(());
    };
    let (dx, dy, r, about) = clip::paste_move(c, x, y, rot);
    let label = if c.count == 1 {
        format!("{} {}", c.cls.strip_prefix("Ifc").unwrap_or(&c.cls), c.name)
    } else {
        bt("edit.many", &[("n", &c.count.to_string())])
    };
    let opts = PasteOpts { storey, dx, dy, rot: r, about: Some(about), keep_numbers: c.cut && h.pasted == 0 };
    let d = crate::bimedit::doc(app)?;
    let res = d.ed.paste_elements(snap, opts, &label).map_err(|e| e.0)?;
    if let Some(hh) = app.bimh.clip.as_mut() {
        hh.pasted = hh.pasted.saturating_add(1);
    }
    crate::bimedit::after_change(app, res.ids.first().copied(), true);
    if res.ids.len() > 1 {
        set_picks(app, res.ids.clone());
    }
    let key = match how {
        "place" => "edit.paste.inplace",
        "beside" => "edit.paste.beside",
        _ => "edit.paste.done",
    };
    status(app, bt(key, &[("n", &res.ids.len().to_string())]));
    Ok(())
}

/// `commitPaste(c, at)`: a click while pasting.
pub fn commit_paste(app: &mut UiApp, at: At) -> Result<(), String> {
    let Some(pl) = app.bimh.draw.placing.as_ref() else { return Ok(()) };
    let Some(c) = pl.paste.as_deref().cloned() else { return Ok(()) };
    let storey = pl.storey;
    if c.kind == "elements" {
        paste_elements_at(app, Some(storey), at.x, at.y, at.rot, "click")?;
    } else {
        let (id, no) = paste_placement(app, &c, storey, at.x, at.y, at.rot)?;
        crate::bimedit::after_change(app, Some(id), true);
        status(app, bt("edit.pasted.item", &[("name", &c.name), ("no", &no)]));
    }
    crate::bimplace::refresh_faces(app);
    if let Some(p) = app.bimh.draw.placing.as_mut() {
        p.at = None;
    }
    Ok(())
}

/// `pasteInPlace(c)`: same X, Y and turn; beside the original when it is still on that storey.
fn paste_in_place(app: &mut UiApp, c: &Clip) -> Result<(), String> {
    if c.kind == "elements" {
        let still = {
            let ids = c.ids.clone();
            let products: Vec<f64> = app.ifc3.as_ref().map(|(_, p)| p.clone()).unwrap_or_default();
            let d = crate::bimedit::doc(app)?;
            ids.iter().any(|id| products.contains(&d.ed.now(*id)))
        };
        let (x, y, rot) = if still { clip::beside_spot(c) } else { (c.x, c.y, c.rot) };
        return paste_elements_at(app, c.storey, x, y, rot, if still { "beside" } else { "place" });
    }
    let tag = model_tag(app);
    let src = clip_source(app, c);
    let d = crate::bimedit::doc(app)?;
    let storeys = Placer::new(&mut d.ed.api).and_then(|mut p| p.info()).map_err(|e| e.0)?;
    let Some((sid, how)) = clip::pick_storey(c, &storeys, src.is_some() || c.model == tag) else {
        return Err(bt("edit.paste.nostoreys", &[]));
    };
    let (mut x, mut y, mut rot, mut beside) = (c.x, c.y, c.rot, false);
    if src.as_ref().is_some_and(|s| s.storey == Some(sid)) {
        (x, y, rot) = clip::beside_spot(c);
        beside = true;
    }
    let (id, no) = paste_placement(app, c, sid, x, y, rot)?;
    crate::bimedit::after_change(app, Some(id), true);
    let wher = storeys.iter().find(|s| s.id == sid).map_or_else(|| bt("edit.paste.thestorey", &[]), |s| s.name.clone());
    let how_text = match how {
        "name" => format!(" {}", bt("edit.paste.how.name", &[])),
        "elevation" => format!(" {}", bt("edit.paste.how.elevation", &[])),
        "first" => format!(" {}", bt("edit.paste.how.first", &[])),
        _ => String::new(),
    };
    let key = if beside { "edit.pasted.inplace.beside" } else { "edit.pasted.inplace.same" };
    status(app, bt(key, &[("name", &c.name), ("no", &no), ("storey", &wher), ("how", &how_text)]));
    Ok(())
}

/// `deletePicked(isCut)`: the picked elements with their parts and openings — or Cut: copy them first. One undo step.
fn delete_picked(app: &mut UiApp, is_cut: bool) -> Result<(), String> {
    let list = editable(app);
    let refused = picks(app).len().saturating_sub(list.len());
    if is_cut && !copy_picked(app, true, true)? {
        return Ok(());
    }
    let label = match list.as_slice() {
        [one] => {
            let d = crate::bimedit::doc(app)?;
            let l = d.ed.api.line(d.ed.now(*one)).ok().flatten();
            let cls = l.as_ref().map(|l| l.class.to_string()).unwrap_or_default();
            let name = l
                .as_ref()
                .map(|l| l.a("Name").text_or_empty())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| format!("#{}", cc_bim::js::num_str(*one)));
            format!("{} {name}", cls.strip_prefix("Ifc").unwrap_or(&cls))
        }
        _ => bt("edit.many", &[("n", &list.len().to_string())]),
    };
    let d = crate::bimedit::doc(app)?;
    let r = d.ed.remove_elements(&list, &label).map_err(|e| e.0)?;
    pick_none(app);
    crate::bimedit::after_change(app, None, true);
    let extra = r.removed.len().saturating_sub(list.len());
    let n = list.len().to_string();
    let mut msg = bt(if is_cut { "edit.cut.done" } else { "edit.delete.done" }, &[("n", &n)]);
    if extra > 0 {
        msg.push(' ');
        msg.push_str(&bt("edit.delete.parts", &[("n", &extra.to_string())]));
    }
    if refused > 0 {
        msg.push(' ');
        msg.push_str(&bt("edit.refused.some", &[("n", &refused.to_string())]));
    }
    status(app, msg);
    Ok(())
}

/// "Swap item…" (`libraryBlock`): the picked placed item is the one the Library's "Swap … to this" changes.
fn swap_pick(app: &mut UiApp) -> Result<(), String> {
    let Some(id) = app.picked_product().filter(|_| app.bimh.picks.is_empty()) else { return Err(bt("edit.need.pick.one", &[])) };
    if !app.sel_props.iter().any(|g| g.title == "CadCraft_Placement") {
        return Err(bt("place.err.not_placed", &[]));
    }
    let name = app.sel_props.first().and_then(|g| g.rows.iter().find(|r| r.0 == "Name")).map(|r| r.1.clone()).filter(|n| !n.is_empty());
    let name = name.unwrap_or_else(|| bt("lib.swap.theitem", &[]));
    app.bimh.swap_target = Some((id, name.clone()));
    app.bimh.lib.open = true;
    status(app, bt("lib.swap.pick", &[("name", &name)]));
    Ok(())
}

/// `doSwap(entry)`: the swap target becomes the entry's item (place, number, quantity and notes kept).
pub fn do_swap(app: &mut UiApp, e: LibEntry) -> Result<(), String> {
    let Some((id, from)) = app.bimh.swap_target.clone() else { return Ok(()) };
    let opts =
        PlaceOpts { library: e.library.clone(), sha256: e.sha256.clone(), source_file: e.source_file.clone(), ..PlaceOpts::default() };
    let d = crate::bimedit::doc(app)?;
    let r = d.ed.swap_item(id, e.item.clone(), opts, Some(&from), "swap").map_err(|x| x.0)?;
    app.bimh.swap_target = None;
    remember_item(app, &e);
    crate::bimedit::after_change(app, Some(id), true);
    let mut msg = bt("swap.done", &[("from", &from), ("to", &e.name)]);
    if let Some((a, b)) = r.class_changed {
        msg.push(' ');
        msg.push_str(&bt("swap.class", &[("from", &a), ("to", &b)]));
    }
    status(app, msg);
    Ok(())
}

/// The library item behind the picked element (`libraryBlock`): licence, attribution, where from, version; Swap item….
pub fn library_block(app: &mut UiApp, ui: &mut egui::Ui) {
    if !app.bimh.picks.is_empty() {
        return;
    }
    let groups = &app.sel_props;
    let set = groups
        .iter()
        .find(|g| g.title == "CadCraft_LibraryItem")
        .or_else(|| groups.iter().find(|g| g.title == "CadCraft_LibraryItem (type)"));
    let Some(set) = set else { return };
    let v = |n: &str| set.rows.iter().find(|r| r.0 == n).map(|r| r.1.clone()).unwrap_or_default();
    let pl = groups.iter().find(|g| g.title == "CadCraft_Placement");
    let no = pl.and_then(|g| g.rows.iter().find(|r| r.0 == "ItemNumber")).map(|r| r.1.clone()).filter(|x| !x.is_empty());
    let t = app.tokens;
    let title = match &no {
        Some(n) => bt("lib.block.title.no", &[("no", n)]),
        None => bt("lib.block.title", &[]),
    };
    let lic = v("License");
    let from = match v("Library").as_str() {
        "librarycraft" => "LibraryCraft".to_string(),
        "file" => {
            let f = v("SourceFile");
            if f.is_empty() { bt("lib.block.from.file", &[]) } else { bt("lib.block.from.file.name", &[("file", &f)]) }
        }
        other => other.to_string(),
    };
    let mut swap = false;
    ui.label(egui::RichText::new(title).strong().family(crate::theme::bold_family()).color(t.text_strong));
    egui::Grid::new("bim_libblock").num_columns(2).show(ui, |ui| {
        ui.label(egui::RichText::new(bt("lib.block.license", &[])).color(t.text_dim));
        ui.label(if lic.is_empty() { bt("lib.block.license.none", &[]) } else { crate::bimlib::license_label(&lic) });
        ui.end_row();
        if !v("Attribution").is_empty() {
            ui.label(egui::RichText::new(bt("lib.block.attribution", &[])).color(t.text_dim));
            ui.label(v("Attribution"));
            ui.end_row();
        }
        ui.label(egui::RichText::new(bt("lib.block.from", &[])).color(t.text_dim));
        ui.label(from);
        ui.end_row();
        let ver = v("ItemVersion");
        if !ver.is_empty() && ver != "0" {
            ui.label(egui::RichText::new(bt("lib.block.version", &[])).color(t.text_dim));
            ui.label(ver);
            ui.end_row();
        }
    });
    if pl.is_some() && ui.add(crate::shell::Btn::new(bt("lib.swap.button", &[]))).clicked() {
        swap = true;
    }
    ui.separator();
    if swap {
        app.run_ui("bim.swap.pick", &Params::default());
    }
}

/// The Edit group's buttons and the More menu (ui.inc: Undo, Redo, Copy, Paste, More), each with why it cannot act.
pub fn more_menu(app: &mut UiApp, ui: &mut egui::Ui) {
    let mut go: Option<&'static str> = None;
    for (name, key, kbd) in [
        ("cut", "ui.cut", "Ctrl+X"),
        ("copy", "ui.copy", "Ctrl+C"),
        ("paste", "ui.paste", "Ctrl+V"),
        ("paste-in-place", "ui.paste.inplace", "Ctrl+Shift+V"),
        ("duplicate", "ui.duplicate", "Ctrl+D"),
        ("delete", "ui.delete", "Delete"),
        ("", "", ""),
        ("select-all", "ui.selectall", "Ctrl+A"),
        ("select-none", "ui.selectnone", "Esc"),
    ] {
        if name.is_empty() {
            ui.separator();
            continue;
        }
        let why = why_not(app, name);
        let title = match name {
            "cut" => bt("edit.cut.title", &[]),
            "copy" => bt("menu.copy.title", &[]),
            "paste" => bt("menu.paste.title", &[]),
            "paste-in-place" => bt("menu.pasteinplace.title", &[]),
            "duplicate" => bt("edit.duplicate.title", &[]),
            "delete" => bt("edit.delete.title", &[]),
            "select-all" => bt("edit.selectall.title", &[]),
            _ => bt("menu.selectnone.title", &[]),
        };
        let r = ui
            .add_enabled(why.is_empty(), egui::Button::new(bt(key, &[])).shortcut_text(kbd))
            .on_hover_text(title)
            .on_disabled_hover_text(why);
        if r.clicked() {
            go = Some(name);
        }
    }
    if let Some(n) = go {
        ui.close();
        app.run_ui("bim.act", &Params { name: Some(n.into()), ..Default::default() });
    }
}

/// The Edit keys while the IFC model is shown (not while typing): Ctrl+C / X / V / Shift+V / D / A, Delete.
pub fn keys(app: &mut UiApp, ctx: &egui::Context) {
    if ctx.memory(|m| m.focused().is_some()) || !ifc_open(app) {
        return;
    }
    let mut name: Option<&'static str> = None;
    ctx.input(|i| {
        for e in &i.events {
            match e {
                egui::Event::Copy => name = Some("copy"),
                egui::Event::Cut => name = Some("cut"),
                egui::Event::Paste(_) => name = Some(if i.modifiers.shift { "paste-in-place" } else { "paste" }),
                egui::Event::Key { key: egui::Key::D, pressed: true, modifiers, .. } if modifiers.command && !modifiers.shift => {
                    name = Some("duplicate")
                }
                egui::Event::Key { key: egui::Key::A, pressed: true, modifiers, .. } if modifiers.command && !modifiers.shift => {
                    name = Some("select-all")
                }
                egui::Event::Key { key: egui::Key::Delete, pressed: true, modifiers, .. } if !modifiers.shift && !modifiers.command => {
                    name = Some("delete");
                }
                _ => {}
            }
        }
    });
    // Draw's keys take Delete there (bimplan.rs, the same action)
    if name == Some("delete") && (picks(app).is_empty() || app.bimh.draw.on) {
        return;
    }
    if let Some(n) = name {
        app.run_ui("bim.act", &Params { name: Some(n.into()), ..Default::default() });
    }
}

#[cfg(test)]
mod tests {
    use crate::{Params, UiApp};

    fn run(app: &mut UiApp, id: &str, name: &str) -> Result<(), String> {
        app.run(id, &Params { name: (!name.is_empty()).then(|| name.into()), ..Default::default() })
    }
    fn placed(app: &mut UiApp) -> Vec<f64> {
        let d = crate::bimedit::doc(app).unwrap();
        cc_bim::draw::place::Placer::new(&mut d.ed.api).unwrap().placements().unwrap().iter().map(|p| p.id).collect()
    }

    #[test]
    fn a_placed_item_is_copied_pasted_swapped_and_deleted() {
        let mut app = UiApp::default();
        for (id, n) in [("bim.clock", "1791979200000"), ("bim.new", "Clip|2|3000 mm"), ("bim.units", "mm")] {
            run(&mut app, id, n).unwrap();
        }
        // nothing picked: Copy says why, nothing breaks
        run(&mut app, "bim.act", "copy").unwrap();
        assert!(app.message.contains("Pick"), "{}", app.message);
        run(&mut app, "bim.act", "paste").unwrap();
        let item = br#"{"format":"cadcraft-library-item","format_version":1,"name":"Desk","ifc":{"class":"IfcFurniture"},
            "body":{"type":"box","x":-600,"y":-700,"z":0,"w":1200,"d":700,"h":740},
            "plan":{"entities":[{"type":"poly","closed":true,"pts":[[-600,-700],[600,-700],[600,0],[-600,0]]}]}}"#;
        app.open_bytes("desk.item.json", item);
        run(&mut app, "bim.point", "3000,2000").unwrap();
        run(&mut app, "bim.place.ok", "").unwrap();
        run(&mut app, "bim.place.stop", "").unwrap();
        let ids = placed(&mut app);
        assert_eq!(ids.len(), 1);
        run(&mut app, "bim.pick", &cc_bim::js::num_str(ids[0])).unwrap();
        run(&mut app, "bim.act", "copy").unwrap();
        assert!(app.message.contains("Desk"), "{}", app.message);
        // Paste in place: beside the original (the same spot would put two in one place)
        run(&mut app, "bim.act", "paste-in-place").unwrap();
        assert!(app.message.contains("beside"), "{}", app.message);
        // Paste: a click pastes another, Esc stops
        run(&mut app, "bim.act", "paste").unwrap();
        assert!(app.bimh.draw.placing.as_ref().is_some_and(|p| p.paste.is_some()));
        run(&mut app, "bim.point", "1000,1000").unwrap();
        run(&mut app, "bim.place.stop", "").unwrap();
        assert_eq!(placed(&mut app).len(), 3);
        // Swap item…: the Library's file item takes its place, its number kept
        run(&mut app, "bim.pick", &cc_bim::js::num_str(ids[0])).unwrap();
        run(&mut app, "bim.swap.pick", "").unwrap();
        assert!(app.bimh.swap_target.is_some());
        app.bimh.lib.file_cards.clear();
        let csv =
            crate::ImportedFile { name: "t.csv".into(), bytes: b"Name,Width (in),Depth (in)\nWork table,72,30\n".to_vec(), dir: None };
        crate::bimlib::read_file(&mut app, &csv).unwrap();
        run(&mut app, "bim.lib.open", "1").unwrap();
        run(&mut app, "bim.lib.swap", "").unwrap();
        assert!(app.message.contains("Work table"), "{}", app.message);
        // several picked: Delete takes them in one change; Undo brings them back
        let all = placed(&mut app);
        run(&mut app, "bim.pick", &cc_bim::js::num_str(all[0])).unwrap();
        run(&mut app, "bim.pick", &format!("+{}", cc_bim::js::num_str(all[1]))).unwrap();
        assert_eq!(super::picks(&app).len(), 2);
        run(&mut app, "bim.act", "delete").unwrap();
        assert_eq!(placed(&mut app).len(), 1);
        run(&mut app, "bim.undo", "").unwrap();
        assert_eq!(placed(&mut app).len(), 3);
        // the clip stays when another model opens; a storey cannot be copied
        crate::bimedit::opened(&mut app);
        assert!(app.bimh.clip.is_some());
    }
}
