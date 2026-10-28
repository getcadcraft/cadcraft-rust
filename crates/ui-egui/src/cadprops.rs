//! CadCraft's Properties panel for the drafting editor (props.js on cc-engine's propdefs / propexpr port): the group
//! picker when several kinds of object are selected ("All (12)", "Line (8)"), Quick select and Select objects, then
//! the fields of the group in foldable sections — General, Geometry (a pick button beside every point: `_PROPPICK`),
//! Custom (a dynamic block's visibility state and parameter values), Text, Pattern, the dimension sections, Misc —
//! and for one object its extra rows: a viewport's scale (Fit all, Show all, Window…), an attribute definition's tag,
//! prompt, default and modes, a reference's Attributes and its Library licence and credit. Each change is ONE undo
//! step through a `props.*` command; number fields read units, feet and inches and `=expressions` as 0.5 does; a
//! refused entry shows its reason and changes nothing. With nothing selected: the sheet (name, paper, orientation,
//! Plot…, Delete sheet), what new objects get (layer, colour, linetype, lineweight, text height, length format,
//! linetype scale, the current layer's linetype and lineweight, new dimensions), the View (centre, height, width:
//! typing moves the view) and Misc. Quick Properties (QPMODE) shows the most-used fields by the pointer.
//!
//! The head's buttons as props.js: Quick select, Select objects, PICKADD (what a click does to the selection: Settings ›
//! CadCraft's Clicking objects), the gear (P.12: which fields show for a kind of object, in the panel and in Quick
//! Properties) and Auto-hide (P.1: the panel folds to a strip at the window's edge and opens over the drawing while
//! the pointer is on it; PROPERTIESCLOSE turns it on, as 0.5's close()). The choices are kept with Settings.

use cc_engine::props::{PCtx, PVal, ReadCtx, apply_value, defs_for, edit_text, group_label, groups_of, read_value};
use egui::RichText;
use serde_json::Value;

use crate::{Params, UiApp};

/// The panel's state: open, the group shown, the polyline vertex, and the text being typed in each field.
#[derive(Default)]
pub struct CadProps {
    pub open: bool,
    pub group: Option<String>,
    pub vertex: i64,
    /// field key → text being edited (absent = shows the value)
    pub edits: std::collections::HashMap<String, String>,
    /// the reason the last entry was refused
    pub note: String,
    /// the selection the panel last showed (a new selection drops the edits and the vertex)
    seen: Vec<u64>,
    /// folded sections (props.js prefs.closed)
    pub closed: std::collections::HashSet<String>,
    /// Quick Properties on (QPMODE), the selection it was closed for, where it opened (screen)
    pub quick: bool,
    quick_dismissed: Vec<u64>,
    quick_at: Option<(Vec<u64>, egui::Pos2)>,
    /// _PROPPICK running for: (group, object ids, point name, vertex)
    pending_pick: Option<(String, Vec<u64>, String, i64)>,
    /// P.1: auto-hide on (the panel is a strip at the edge), and the strip's panel open over the drawing
    pub autohide: bool,
    pub flyout: bool,
    /// when the pointer left the open flyout (it folds 500 ms later)
    left_at: Option<f64>,
    /// P.12 (props.js pr.extra.hide / .qp): per kind of object, the fields left out of the panel and the fields
    /// Quick Properties shows (absent = its standard set)
    pub hide: std::collections::BTreeMap<String, Vec<String>>,
    pub qp: std::collections::BTreeMap<String, Vec<String>>,
    /// the gear's dialog: (kind, its rows)
    pub custom: Option<(String, Vec<CustomRow>)>,
    /// the kind's name for the list's title
    custom_name: String,
}

/// One row of P.12's list: (field, label, shown in the panel, shown in Quick Properties).
pub type CustomRow = (String, String, bool, bool);

impl CadProps {
    /// What the panel keeps between runs (props.js's prefs: auto-hide, the fields chosen per kind, Quick Properties on).
    pub fn prefs_json(&self) -> Value {
        serde_json::json!({ "autohide": self.autohide, "hide": self.hide, "qp": self.qp, "qpOn": self.quick })
    }
    /// Read back (untrusted: lists of short strings per kind; anything else is dropped).
    pub fn load_prefs(&mut self, v: &Value) {
        fn lists(v: Option<&Value>) -> std::collections::BTreeMap<String, Vec<String>> {
            let mut out = std::collections::BTreeMap::new();
            for (k, l) in v.and_then(Value::as_object).into_iter().flatten().take(64) {
                let Some(a) = l.as_array() else { continue };
                if k.len() > 40 {
                    continue;
                }
                let keys: Vec<String> =
                    a.iter().filter_map(Value::as_str).filter(|x| x.len() <= 40).take(200).map(str::to_string).collect();
                out.insert(k.clone(), keys);
            }
            out
        }
        self.autohide = v.get("autohide").and_then(Value::as_bool).unwrap_or(false);
        self.hide = lists(v.get("hide"));
        self.qp = lists(v.get("qp"));
        self.quick = v.get("qpOn").and_then(Value::as_bool).unwrap_or(false);
    }
}

/// UI commands of the panel.
pub const COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("props.open", "Properties", "Ctrl+1", "{} → opens the Properties panel (the PROPERTIES command does the same)"),
    ("props.close", "Close Properties", "", "{}"),
    ("props.group", "Properties: group", "", "{name: \"all\" | a type (line, circle, …)} → the group the panel shows"),
    ("props.vertex", "Properties: vertex", "", "{index: n} → the polyline vertex the panel shows (0 = the first)"),
    (
        "props.units",
        "Properties: drawing units",
        "",
        "{name: \"\" | millimeters | centimeters | meters | inches | feet} → what one drawing unit stands for; with solids the 3D model is scaled to stay on the drawing (one undo step)",
    ),
    (
        "props.set",
        "Properties: set a field",
        "",
        "{name: \"<key>=<value>\"} → sets the field on every object of the group (one undo step); number fields read text as typed (12'6\", 900mm, =2*45); choices take the value; colour: \"\" by layer, 0 by block, 1–255, #rrggbb",
    ),
    (
        "props.attr",
        "Properties: an attribute",
        "",
        "{name: \"<TAG>=<value>\"} → the one selected reference's attribute (one undo step ATTRIBUTE)",
    ),
    (
        "props.attdef",
        "Properties: an attribute definition",
        "",
        "{name: \"tag|prompt|def|invisible|constant|preset=<value>\"} → the one selected attribute definition (one undo step)",
    ),
    (
        "props.vpscale",
        "Properties: viewport scale",
        "",
        "{name: \"<n>\" (1:n) | \"fit\" (Fit all) | \"showall\" (Show all)} → the one selected viewport",
    ),
    (
        "props.sheet",
        "Properties: the sheet",
        "",
        "{name: \"name=<text>\" | \"paper=<key>\" | \"orient=landscape|portrait\" | \"delete\"} → the open sheet (one undo step each)",
    ),
    (
        "props.current",
        "Properties: what new objects get",
        "",
        "{name: \"layer|color|lt|lw|th|lunits|luprec|ltscale|laylt|laylw|dim.txt|dim.asz|dim.dec|dim.scale=<value>\"} → nothing selected: the settings for new objects (numbers read as typed)",
    ),
    ("props.view", "Properties: the view", "", "{name: \"cx|cy|h|w=<length>\"} → moves the view (its centre, height, width)"),
    (
        "props.pick",
        "Properties: pick a point",
        "",
        "{name: \"start|end|center|pos|ins|vertex|…\"} → asks for that point of the selected objects (_PROPPICK)",
    ),
    ("props.quick", "Quick Properties on / off", "", "{} → the most-used fields by the pointer when you select (QPMODE)"),
    ("props.fold", "Properties: fold a section", "", "{name: section key} → folds or unfolds it"),
    (
        "props.pickadd",
        "Properties: PICKADD",
        "",
        "{} → a click adds to the selection, or replaces it (the same setting as Settings › CadCraft › Clicking objects)",
    ),
    (
        "props.autohide",
        "Properties: auto-hide",
        "",
        "{name?: \"on\" | \"off\"} → the panel folds to the edge of the window (no name: switches)",
    ),
    (
        "props.custom",
        "Properties: choose the fields",
        "",
        "{name?: kind (\"all\", line, circle, …; default the group shown)} → opens the list of that kind's fields (selected objects of that kind needed)",
    ),
    (
        "props.custom.set",
        "Properties: choose a field",
        "",
        "{name: \"panel|qp:<field>=on|off\"} → in the open list: show the field in Properties / in Quick Properties",
    ),
    ("props.custom.ok", "Properties: fields chosen", "", "{} → keeps the list's choices for that kind"),
    ("props.custom.reset", "Properties: standard fields", "", "{} → back to the standard set for that kind"),
    ("props.custom.cancel", "Properties: close the list", "", "{}"),
];

fn lang() -> cc_engine::Lang {
    crate::edit::lang_en_pub()
}

/// A typed value for a non-number field (what the panel's selects hand over).
fn control_value(kind: &str, s: &str) -> Value {
    match kind {
        "bool" => Value::Bool(matches!(s, "true" | "1" | "yes" | "on")),
        "color" => {
            if s.is_empty() || s.eq_ignore_ascii_case("bylayer") {
                Value::Null
            } else if s == "none" {
                Value::String("none".into())
            } else if s.starts_with('#') {
                Value::String(s.into())
            } else {
                s.parse::<i64>().map_or(Value::Null, Value::from)
            }
        }
        "lt" | "mat" => {
            if s.is_empty() {
                Value::Null
            } else {
                Value::String(s.into())
            }
        }
        "lw" => {
            if s.is_empty() {
                Value::Null
            } else {
                s.parse::<f64>().map_or(Value::Null, cc_cad::num)
            }
        }
        "tr" => {
            if s.is_empty() {
                Value::Null
            } else if s == "ByBlock" {
                Value::String("ByBlock".into())
            } else {
                s.parse::<f64>().map_or(Value::Null, |v| cc_cad::num(v.round().clamp(0.0, 90.0)))
            }
        }
        _ => Value::String(s.into()),
    }
}

/// `props.*` commands.
pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    match id {
        // props.js open(): unfolds the auto-hidden panel over the drawing; close(): folds it (turning auto-hide on)
        "props.open" => {
            app.cadprops.open = true;
            if app.cadprops.autohide {
                app.cadprops.flyout = true;
                app.cadprops.left_at = None;
            }
        }
        "props.close" => {
            app.cadprops.open = true;
            if app.cadprops.autohide {
                app.cadprops.flyout = false;
            } else {
                set_autohide(app, true);
            }
        }
        "props.pickadd" => {
            app.cadset.pick_replace = !app.cadset.pick_replace;
            crate::cadset::push(app);
        }
        "props.autohide" => {
            let on = match params.name.as_deref() {
                Some("on") => true,
                Some("off") => false,
                None | Some("") => !app.cadprops.autohide,
                Some(x) => return Err(format!("props.autohide: {x} is not on or off")),
            };
            set_autohide(app, on);
        }
        "props.custom" => customize(app, params.name.as_deref())?,
        "props.custom.set" => {
            let spec = params.name.clone().unwrap_or_default();
            let (lhs, v) = spec.split_once('=').ok_or("props.custom.set: name = \"panel|qp:<field>=on|off\"")?;
            let (w, key) = lhs.split_once(':').ok_or("props.custom.set: name = \"panel|qp:<field>=on|off\"")?;
            let on = match v {
                "on" | "true" | "1" => true,
                "off" | "false" | "0" => false,
                _ => return Err(format!("props.custom.set: {v} is not on or off")),
            };
            let (_, rows) = app.cadprops.custom.as_mut().ok_or("props.custom.set: the list is not open")?;
            let row = rows.iter_mut().find(|r| r.0 == key).ok_or_else(|| format!("props.custom.set: no field {key} in the list"))?;
            match w {
                "panel" => row.2 = on,
                "qp" => row.3 = on,
                _ => return Err(format!("props.custom.set: {w} is not panel or qp")),
            }
        }
        "props.custom.ok" => {
            let (key, rows) = app.cadprops.custom.take().ok_or("props.custom.ok: the list is not open")?;
            app.cadprops.hide.insert(key.clone(), rows.iter().filter(|r| !r.2).map(|r| r.0.clone()).collect());
            app.cadprops.qp.insert(key, rows.iter().filter(|r| r.3).map(|r| r.0.clone()).collect());
            app.cadprops.quick_dismissed.clear();
        }
        "props.custom.reset" => {
            let (key, _) = app.cadprops.custom.take().ok_or("props.custom.reset: the list is not open")?;
            app.cadprops.hide.remove(&key);
            app.cadprops.qp.remove(&key);
            app.cadprops.quick_dismissed.clear();
        }
        "props.custom.cancel" => {
            app.cadprops.custom = None;
            app.cadprops.quick_dismissed.clear();
        }
        "props.group" => app.cadprops.group = params.name.clone(),
        "props.vertex" => app.cadprops.vertex = i64::try_from(params.index.unwrap_or(0)).unwrap_or(0),
        "props.units" => set_units(app, params.name.as_deref().unwrap_or(""))?,
        "props.set" => {
            let spec = params.name.clone().ok_or("props.set: name = \"<key>=<value>\" is required")?;
            let (key, raw) = spec.split_once('=').ok_or("props.set: name = \"<key>=<value>\"")?;
            set_field(app, key, raw)?;
        }
        "props.attr" | "props.attdef" | "props.vpscale" => one_object(app, id, params.name.as_deref().unwrap_or(""))?,
        "props.sheet" => sheet(app, params.name.as_deref().unwrap_or(""))?,
        "props.current" => {
            let spec = params.name.clone().ok_or("props.current: name = \"<key>=<value>\" is required")?;
            let (key, raw) = spec.split_once('=').ok_or("props.current: name = \"<key>=<value>\"")?;
            current(app, key, raw)?;
        }
        "props.view" => {
            let spec = params.name.clone().ok_or("props.view: name = \"<key>=<length>\" is required")?;
            let (key, raw) = spec.split_once('=').ok_or("props.view: name = \"<key>=<length>\"")?;
            view(app, key, raw)?;
        }
        "props.pick" => start_pick(app, params.name.as_deref().unwrap_or(""))?,
        "props.quick" => toggle_quick(app),
        "props.fold" => {
            let k = params.name.clone().unwrap_or_default();
            if !app.cadprops.closed.remove(&k) {
                app.cadprops.closed.insert(k);
            }
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

/// Set one field on the group the panel shows (a number field reads `raw` as typed; others take it as the value).
pub fn set_field(app: &mut UiApp, key: &str, raw: &str) -> Result<(), String> {
    let lang = lang();
    let vertex = app.cadprops.vertex;
    let want = app.cadprops.group.clone();
    let ed = app.editor.as_mut().ok_or("props.set: no drawing is open for editing")?;
    let sel = ed.hs.borrow().selection.clone();
    let (gkey, gids, value) = {
        let doc = ed.eng.doc();
        let ents: Vec<cc_cad::Entity> = sel.iter().filter_map(|id| doc.get(*id).cloned()).collect();
        if ents.is_empty() {
            return Err("props.set: nothing is selected".into());
        }
        let groups = groups_of(&ents, &lang);
        let g = want.as_ref().and_then(|k| groups.iter().find(|g| &g.key == k)).or(groups.first()).ok_or("props.set: no group")?;
        let gents: Vec<cc_cad::Entity> = g.ids.iter().filter_map(|id| doc.get(*id).cloned()).collect();
        let c = PCtx { doc: &doc, ents: &gents, vertex, lang: &lang };
        let defs = defs_for(&g.key, &c);
        let d = defs.iter().find(|d| d.key == key).ok_or(format!("props.set: the group has no field {key}"))?;
        let value = if matches!(d.kind, "len" | "ang" | "num" | "int" | "area") {
            let rc = ReadCtx {
                lunits: doc.meta.get("lunits").and_then(Value::as_f64),
                units: doc.meta.get("units").and_then(Value::as_str).unwrap_or("").to_string(),
                paper: if doc.layout().is_some() && !d.model { doc.layout().and_then(|l| l.unit()).map(str::to_string) } else { None },
                gt0: d.gt0,
                min: d.min,
                max: d.max,
            };
            read_value(raw, if d.kind == "area" { "num" } else { d.kind }, &rc, &lang).map(cc_cad::num)?
        } else {
            control_value(d.kind, raw)
        };
        (g.key.clone(), g.ids.clone(), value)
    };
    let r = {
        let mut doc = ed.eng.doc_mut();
        apply_value(&mut doc, key, &gkey, &gids, &value, vertex, &lang)
    };
    app.sync_editor();
    if let Some(e) = r.error {
        app.cadprops.note = e.clone();
        return Err(e);
    }
    app.cadprops.note = if r.locked > 0 { format!("{} on locked layers left as they were", r.locked) } else { String::new() };
    Ok(())
}

/// The units the panel offers (props.js unitsSelect).
const UNITS: [&str; 6] = ["", "millimeters", "centimeters", "meters", "inches", "feet"];

/// props.js unitsSelect's change: model3d.js setUnits, the status line, and the note about the scaled 3D model.
pub fn set_units(app: &mut UiApp, units: &str) -> Result<(), String> {
    if !UNITS.contains(&units) {
        return Err(format!("props.units: {units} is not one of the drawing units (\"\", millimeters, centimeters, meters, inches, feet)"));
    }
    let l = lang();
    let ed = app.editor.as_mut().ok_or("props.units: no drawing is open for editing")?;
    let (scaled, ratio) = cc_engine::cmd::solid3d::set_units(ed.eng.shared(), units);
    let name = l.s(&format!("props.units.{}", if units.is_empty() { "none" } else { units }));
    app.message = l.t(if scaled { "props.units.set.scaled" } else { "props.units.set" }, &[("units", name.clone())]);
    if scaled {
        let k: f64 = format!("{ratio:.5e}").parse().unwrap_or(ratio);
        let m = l.t("props.units.set.3d", &[("units", name), ("k", cc_cad::js::num_to_string(k))]);
        ed.hs.borrow_mut().push_log(&m);
    }
    app.sync_editor();
    Ok(())
}

/// `fmt(n)` of ui-util.js: 4 decimals, trailing zeros dropped.
fn fmt4(n: f64) -> String {
    cc_engine::input::fmt(n)
}

/// How a value reads in its field (props.js `show`).
fn show(kind: &str, v: &Value, doc: &cc_cad::Doc) -> String {
    let n = || v.as_f64();
    match kind {
        "len" | "ro-len" => n().map_or_else(String::new, |x| {
            let (code, prec) = cc_cad_geom::units::units_of(Some(&doc.meta));
            cc_cad_geom::units::format_length(x, f64::from(code), Some(&Value::from(prec)))
        }),
        "area" | "ro-area" => n().map_or_else(String::new, |x| {
            let (code, prec) = cc_cad_geom::units::units_of(Some(&doc.meta));
            let l = lang();
            cc_cad_geom::units::format_area(x, f64::from(code), Some(&Value::from(prec)), &|k| l.has(k).then(|| l.s(k)))
        }),
        "ang" => n().map_or_else(String::new, |x| format!("{}°", fmt4(x))),
        "num" | "int" => n().map_or_else(String::new, fmt4),
        _ => cc_engine::props::value_text(v),
    }
}

fn color_label(v: &Value, l: &cc_engine::Lang) -> String {
    match v {
        Value::Null => l.s("props.color.bylayer"),
        Value::String(s) if s == "none" => l.s("props.color.none"),
        Value::String(s) => s.clone(),
        Value::Number(n) => {
            let c = n.as_f64().unwrap_or(f64::NAN);
            if c == 0.0 {
                return l.s("props.color.byblock");
            }
            let names = [
                "app.aci.red",
                "app.aci.yellow",
                "app.aci.green",
                "app.aci.cyan",
                "app.aci.blue",
                "app.aci.magenta",
                "app.ink_black_white",
                "app.aci.gray",
                "app.light_gray",
            ];
            match usize::try_from(n.as_i64().unwrap_or(-1)).ok().and_then(|i| i.checked_sub(1)).and_then(|i| names.get(i)) {
                Some(k) => format!("{} ({})", l.s(k), js_num(c)),
                None => format!("Colour {}", js_num(c)),
            }
        }
        v => v.to_string(),
    }
}
fn js_num(x: f64) -> String {
    cc_cad::js::num_to_string(x)
}

fn value_key(kind: &str, v: &Value) -> String {
    match (kind, v) {
        (_, Value::Null) => String::new(),
        (_, Value::Bool(b)) => b.to_string(),
        _ => cc_engine::props::value_text(v),
    }
}

fn status(app: &mut UiApp, s: String) {
    app.cadprops.note.clone_from(&s);
    app.message = s;
}

/// The one selected object (the extra rows are for one object only).
fn single(app: &UiApp) -> Option<u64> {
    let ed = app.editor.as_ref()?;
    let sel = ed.hs.borrow().selection.clone();
    let d = ed.eng.doc();
    let ids: Vec<u64> = sel.into_iter().filter(|id| d.get(*id).is_some()).collect();
    match ids.as_slice() {
        [one] => Some(*one),
        _ => None,
    }
}

/// props.attr / props.attdef / props.vpscale on the one selected object.
fn one_object(app: &mut UiApp, id: &str, spec: &str) -> Result<(), String> {
    let oid = single(app).ok_or(format!("{id}: select one object"))?;
    let lang = lang();
    let msg = {
        let ed = app.editor.as_mut().ok_or("no drawing is open for editing")?;
        let mut doc = ed.eng.doc_mut();
        match id {
            "props.attr" => {
                let (tag, v) = spec.split_once('=').ok_or("props.attr: name = \"<TAG>=<value>\"")?;
                cc_engine::propextra::set_attribute(&mut doc, oid, tag, v)?
            }
            "props.attdef" => {
                let (k, v) = spec.split_once('=').ok_or("props.attdef: name = \"<field>=<value>\"")?;
                cc_engine::propextra::set_attdef(&mut doc, oid, k, v)?;
                None
            }
            _ => {
                cc_engine::propextra::set_viewport_scale(&mut doc, oid, spec)?;
                None
            }
        }
    };
    let _ = &lang;
    app.sync_editor();
    if let Some(m) = msg {
        status(app, m);
    }
    Ok(())
}

/// props.sheet: the open sheet's name, paper, orientation, or delete it.
fn sheet(app: &mut UiApp, spec: &str) -> Result<(), String> {
    let lang = lang();
    let r = {
        let ed = app.editor.as_mut().ok_or("no drawing is open for editing")?;
        let mut doc = ed.eng.doc_mut();
        let (k, v) = spec.split_once('=').unwrap_or((spec, ""));
        let portrait_now = doc.layout().is_some_and(|l| l.h().unwrap_or(0.0) > l.w().unwrap_or(0.0));
        match k {
            "name" => cc_engine::propextra::rename_sheet(&mut doc, v, &lang).map(|_| None),
            "paper" => cc_engine::propextra::set_sheet_paper(&mut doc, v, portrait_now, &lang),
            "orient" => {
                let key = doc.layout().and_then(|l| l.key()).unwrap_or("").to_string();
                cc_engine::propextra::set_sheet_paper(&mut doc, &key, v == "portrait", &lang)
            }
            "delete" => cc_engine::propextra::delete_sheet(&mut doc, &lang).map(Some),
            _ => return Err("props.sheet: name = name=… | paper=… | orient=… | delete".into()),
        }
    };
    app.sync_editor();
    match r {
        Ok(Some(m)) => status(app, m),
        Ok(None) => {}
        Err(e) => {
            status(app, e.clone());
            return Err(e);
        }
    }
    Ok(())
}

/// A typed number read as props.js `read` does (units, feet and inches, =expressions).
fn read_num(app: &UiApp, kind: &str, raw: &str, gt0: bool, min: Option<f64>, max: Option<f64>) -> Result<f64, String> {
    let ed = app.editor.as_ref().ok_or("no drawing is open for editing")?;
    let doc = ed.eng.doc();
    let rc = ReadCtx {
        lunits: doc.meta.get("lunits").and_then(Value::as_f64),
        units: doc.meta.get("units").and_then(Value::as_str).unwrap_or("").to_string(),
        paper: doc.layout().and_then(|l| l.unit()).map(str::to_string),
        gt0,
        min,
        max,
    };
    read_value(raw, kind, &rc, &lang())
}

/// props.current: what new objects get.
fn current(app: &mut UiApp, key: &str, raw: &str) -> Result<(), String> {
    let lang = lang();
    let v: Value = match key {
        "th" | "dim.txt" => cc_cad::num(read_num(app, "len", raw, true, None, None)?),
        "dim.asz" => cc_cad::num(read_num(app, "len", raw, false, Some(0.0), None)?),
        "dim.dec" => cc_cad::num(read_num(app, "int", raw, false, Some(0.0), Some(8.0))?),
        "ltscale" | "dim.scale" => cc_cad::num(read_num(app, "num", raw, true, None, None)?),
        "lunits" | "luprec" => cc_cad::num(raw.trim().parse::<f64>().map_err(|_| format!("props.current: {key} = a number"))?),
        "color" => control_value("color", raw),
        "lt" | "laylt" => control_value("lt", raw),
        "lw" | "laylw" => control_value("lw", raw),
        _ => Value::String(raw.to_string()),
    };
    let r = {
        let ed = app.editor.as_mut().ok_or("no drawing is open for editing")?;
        let mut doc = ed.eng.doc_mut();
        cc_engine::propextra::set_current(&mut doc, key, &v, &lang)?
    };
    app.sync_editor();
    if let Some(m) = r {
        status(app, m);
    }
    Ok(())
}

/// The view on screen: centre and size in drawing units.
fn view_now(app: &UiApp) -> Option<(f64, f64, f64, f64)> {
    let b = app.editor.as_ref()?.hs.borrow().view_box?;
    Some(((b.min_x + b.max_x) / 2.0, (b.min_y + b.max_y) / 2.0, b.max_x - b.min_x, b.max_y - b.min_y))
}

/// props.view: typing the view's centre, height or width moves it (props.js goTo).
fn view(app: &mut UiApp, key: &str, raw: &str) -> Result<(), String> {
    let (cx, cy, vw, vh) = view_now(app).ok_or("props.view: the view is not on screen yet")?;
    let gt0 = matches!(key, "h" | "w");
    let v = read_num(app, "len", raw, gt0, None, None)?;
    let aspect = if vh > 0.0 { vw / vh } else { 1.0 };
    let (x, y, w, h) = match key {
        "cx" => (v, cy, vw, vh),
        "cy" => (cx, v, vw, vh),
        "h" => (cx, cy, v * aspect, v),
        "w" => (cx, cy, v, v / aspect),
        _ => return Err("props.view: name = cx|cy|h|w=<length>".into()),
    };
    if let Some(ed) = app.editor.as_ref() {
        ed.hs.borrow_mut().zoom.push(crate::edit::Zoom::Window(
            cc_geom::P2::new(x - w / 2.0, y - h / 2.0),
            cc_geom::P2::new(x + w / 2.0, y + h / 2.0),
            true,
        ));
    }
    app.sync_editor();
    Ok(())
}

/// props.pick: the pick button beside a point — `_PROPPICK` asks for it.
fn start_pick(app: &mut UiApp, pt: &str) -> Result<(), String> {
    let lang = lang();
    let (gkey, gids) = group_now(app).ok_or("props.pick: nothing is selected")?;
    let has = {
        let ed = app.editor.as_ref().ok_or("no drawing is open for editing")?;
        let doc = ed.eng.doc();
        let ents: Vec<cc_cad::Entity> = gids.iter().filter_map(|id| doc.get(*id).cloned()).collect();
        let c = PCtx { doc: &doc, ents: &ents, vertex: app.cadprops.vertex, lang: &lang };
        defs_for(&gkey, &c).iter().any(|d| d.pt.is_some_and(|(n, _)| n == pt))
    };
    if !has {
        return Err(format!("props.pick: the group has no point {pt}"));
    }
    let label = point_label(pt, &lang);
    app.cadprops.pending_pick = Some((gkey, gids, pt.to_string(), app.cadprops.vertex));
    if let Some(ed) = app.editor.as_ref() {
        ed.hs.borrow_mut().prop_pick_label = Some(label);
    }
    app.run_ui_pub("cmd.line", &Params { name: Some("_PROPPICK".into()), ..Default::default() });
    Ok(())
}

/// propdefs.js `pointLabel(name)`.
fn point_label(name: &str, l: &cc_engine::Lang) -> String {
    let k = match name {
        "start" | "end" | "center" | "pos" | "ins" | "vertex" | "origin" | "arrow" | "vcenter" | "vview" | "dimp1" | "dimp2" | "dimc"
        | "dimloc" => format!("props.pt.{name}"),
        _ => "props.pt.pos".into(),
    };
    l.s(&k)
}

/// props.js `setAutohide(on)`: the panel folds to a strip (or comes back), and the status bar says so.
fn set_autohide(app: &mut UiApp, on: bool) {
    app.cadprops.autohide = on;
    app.cadprops.open = true;
    app.cadprops.flyout = false;
    app.cadprops.left_at = None;
    // props.js status(): the status bar only (not the panel's note)
    app.message = lang().s(if on { "props.autohide.on" } else { "props.autohide.off" });
}

/// props.js `customize(key)`: the kind's fields with two ticks each (in Properties, in Quick Properties).
fn customize(app: &mut UiApp, key: Option<&str>) -> Result<(), String> {
    let ed = app.editor.as_ref().ok_or("props.custom: no drawing is open for editing")?;
    let key = match key.filter(|k| !k.is_empty()) {
        Some(k) => k.to_string(),
        None => group_now(app).map(|(k, _)| k).ok_or("props.custom: nothing is selected")?,
    };
    let sel = ed.hs.borrow().selection.clone();
    let doc = ed.eng.doc();
    let ents: Vec<cc_cad::Entity> = sel.iter().filter_map(|id| doc.get(*id).cloned()).filter(|e| key == "all" || e.typ() == key).collect();
    if ents.is_empty() {
        return Err(format!("props.custom: no selected object is a {key}"));
    }
    let l = lang();
    let c = PCtx { doc: &doc, ents: &ents, vertex: 0, lang: &l };
    let defs = defs_for(&key, &c);
    let hide: Vec<String> = app.cadprops.hide.get(&key).cloned().unwrap_or_default();
    let qk: Vec<String> = app.cadprops.qp.get(&key).cloned().unwrap_or_else(|| qp_keys(&key).iter().map(|k| (*k).to_string()).collect());
    let rows = defs
        .iter()
        .map(|d| {
            let lab = d.label.clone().unwrap_or_else(|| d.key.clone());
            let label = match d.pt {
                Some((name, _)) => format!("{} {lab}", point_label(name, &l)),
                None => lab,
            };
            (d.key.clone(), label, !hide.contains(&d.key), qk.contains(&d.key))
        })
        .collect();
    // the title's name: "All" or the kind's (props.js TYPE_NAMES)
    let name = if key == "all" {
        l.s("props.group.all")
    } else {
        groups_of(&ents, &l).into_iter().find(|g| g.key == key).map(|g| g.label).unwrap_or_else(|| key.clone())
    };
    drop(doc);
    app.cadprops.custom_name = name;
    app.cadprops.custom = Some((key, rows));
    Ok(())
}

/// P.12 `shownIn('panel')`: the kind's fields less the ones left out.
fn shown_in_panel<'a>(
    hide: &std::collections::BTreeMap<String, Vec<String>>,
    key: &str,
    defs: &'a [cc_engine::props::Def],
) -> Vec<&'a cc_engine::props::Def> {
    let off = hide.get(key);
    defs.iter().filter(|d| off.is_none_or(|h| !h.contains(&d.key))).collect()
}

fn toggle_quick(app: &mut UiApp) {
    app.cadprops.quick = !app.cadprops.quick;
    app.cadprops.quick_dismissed.clear();
    app.cadprops.quick_at = None;
    let m = lang().s(if app.cadprops.quick { "props.qp.on" } else { "props.qp.off" });
    status(app, m);
}

/// After every input: QPMODE's toggle, and the point _PROPPICK picked.
pub fn sync(app: &mut UiApp) {
    let Some(ed) = app.editor.as_ref() else { return };
    let (qp, picked) = {
        let mut h = ed.hs.borrow_mut();
        (std::mem::take(&mut h.qp_toggle), h.prop_picked.take())
    };
    if qp {
        toggle_quick(app);
    }
    if let Some(p) = picked
        && let Some((g, ids, pt, vertex)) = app.cadprops.pending_pick.take()
    {
        let lang = lang();
        let err = app.editor.as_mut().and_then(|ed| {
            let mut doc = ed.eng.doc_mut();
            cc_engine::propextra::pick_point(&mut doc, &g, &ids, &pt, (p.x, p.y), vertex, &lang)
        });
        if let Some(e) = err {
            status(app, e);
        }
    }
}

/// The group the panel shows: (key, ids).
fn group_now(app: &UiApp) -> Option<(String, Vec<u64>)> {
    let ed = app.editor.as_ref()?;
    let sel = ed.hs.borrow().selection.clone();
    let doc = ed.eng.doc();
    let ents: Vec<cc_cad::Entity> = sel.iter().filter_map(|id| doc.get(*id).cloned()).collect();
    let l = lang();
    let groups = groups_of(&ents, &l);
    let g = app.cadprops.group.as_ref().and_then(|k| groups.iter().find(|g| &g.key == k)).or(groups.first())?;
    Some((g.key.clone(), g.ids.clone()))
}

/// The choices a select of this kind offers: (value text for the command, label).
fn kind_choices(kind: &str, none: bool, doc: &cc_cad::Doc, l: &cc_engine::Lang) -> Vec<(String, String)> {
    match kind {
        "layer" => {
            let mut v: Vec<String> = doc.layers.keys().cloned().collect();
            v.sort();
            v.into_iter().map(|n| (n.clone(), n)).collect()
        }
        "color" => {
            let mut v = Vec::new();
            if none {
                v.push(("none".to_string(), l.s("props.color.none")));
            }
            v.push((String::new(), l.s("props.color.bylayer")));
            v.push(("0".into(), l.s("props.color.byblock")));
            for i in 1..=9i64 {
                v.push((i.to_string(), color_label(&Value::from(i), l)));
            }
            v
        }
        "lt" => {
            let mut v = vec![
                (String::new(), l.s("lt.bylayer")),
                ("ByBlock".into(), l.s("lt.byblock")),
                ("Continuous".into(), l.s("lt.continuous")),
            ];
            for n in doc.linetypes.keys() {
                if !n.eq_ignore_ascii_case("continuous") {
                    v.push((n.clone(), n.clone()));
                }
            }
            for n in cc_cad_geom::linetypes::standard_names() {
                if !v.iter().any(|(k, _)| k.eq_ignore_ascii_case(&n)) {
                    v.push((n.clone(), n));
                }
            }
            v
        }
        "lw" => {
            let mut v = vec![(String::new(), l.s("lt.bylayer")), ("-2".into(), l.s("lt.byblock")), ("-3".into(), l.s("lw.default"))];
            for w in cc_cad_geom::linetypes::LINEWEIGHTS {
                v.push((js_num(w), l.t("lw.mm", &[("mm", cc_cad::js::to_fixed(w, 2))])));
            }
            v
        }
        "tr" => {
            let mut v = vec![(String::new(), l.s("props.tr.bylayer")), ("ByBlock".into(), l.s("props.tr.byblock"))];
            for n in (0..=90).step_by(10) {
                v.push((n.to_string(), l.t("props.tr.pct", &[("n", n.to_string())])));
            }
            v
        }
        "mat" => vec![
            (String::new(), l.s("props.mat.bylayer")),
            ("ByBlock".into(), l.s("props.mat.byblock")),
            ("Global".into(), l.s("props.mat.global")),
        ],
        _ => Vec::new(),
    }
}

/// The choices a field offers.
fn select_choices(d: &cc_engine::props::Def, c: &PCtx, l: &cc_engine::Lang) -> Vec<(String, String)> {
    match d.kind {
        "layer" | "color" | "lt" | "lw" | "tr" | "mat" => kind_choices(d.kind, d.none, c.doc, l),
        _ => d
            .choices
            .as_ref()
            .map(|f| f(c).into_iter().map(|(v, lab)| (cc_engine::props::value_text(&v), lab)).collect())
            .unwrap_or_default(),
    }
}

// ------------------------------------------------------------------------------------------------- widgets

/// A small icon button drawn on a 20 × 20 grid (the UI font has no such glyphs): "pick" (a crosshair), "funnel"
/// (Quick select), "cursor" (Select objects).
fn icon_button(ui: &mut egui::Ui, icon: &str, tip: &str, color: egui::Color32) -> bool {
    let (rect, r) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
    let r = r.on_hover_text(tip);
    if r.hovered() {
        ui.painter().rect_filled(rect, 2.0, ui.visuals().widgets.hovered.bg_fill);
    }
    let k = rect.width() / 20.0;
    let at = |x: f32, y: f32| rect.min + egui::vec2(x * k, y * k);
    let st = egui::Stroke::new(1.3, color);
    let p = ui.painter();
    match icon {
        "pick" => {
            p.circle_stroke(at(10.0, 10.0), 5.0 * k, st);
            p.line_segment([at(10.0, 2.0), at(10.0, 7.0)], st);
            p.line_segment([at(10.0, 13.0), at(10.0, 18.0)], st);
            p.line_segment([at(2.0, 10.0), at(7.0, 10.0)], st);
            p.line_segment([at(13.0, 10.0), at(18.0, 10.0)], st);
        }
        "plus-square" | "1-square" => {
            p.rect_stroke(egui::Rect::from_min_max(at(3.0, 3.0), at(17.0, 17.0)), 2.0 * k, st, egui::StrokeKind::Middle);
            if icon == "plus-square" {
                p.line_segment([at(10.0, 6.5), at(10.0, 13.5)], st);
                p.line_segment([at(6.5, 10.0), at(13.5, 10.0)], st);
            } else {
                p.add(egui::Shape::line(vec![at(8.0, 8.0), at(10.5, 6.0), at(10.5, 14.0)], st));
            }
        }
        "gear" => {
            p.circle_stroke(at(10.0, 10.0), 2.5 * k, st);
            p.circle_stroke(at(10.0, 10.0), 5.5 * k, st);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                p.line_segment([at(10.0 + 5.5 * a.cos(), 10.0 + 5.5 * a.sin()), at(10.0 + 7.5 * a.cos(), 10.0 + 7.5 * a.sin())], st);
            }
        }
        "pin" | "pin-fill" => {
            // a push pin (filled: the panel is kept open; outline: it folds to the edge)
            let body = vec![at(7.5, 4.0), at(12.5, 4.0), at(13.0, 10.0), at(7.0, 10.0)];
            if icon == "pin-fill" {
                p.add(egui::Shape::convex_polygon(body.clone(), color, egui::Stroke::NONE));
            }
            p.add(egui::Shape::closed_line(body, st));
            p.line_segment([at(6.0, 3.0), at(14.0, 3.0)], st);
            p.line_segment([at(4.5, 10.5), at(15.5, 10.5)], st);
            p.line_segment([at(10.0, 10.5), at(10.0, 18.0)], st);
        }
        "funnel" => {
            p.add(egui::Shape::closed_line(
                vec![at(3.0, 4.0), at(17.0, 4.0), at(11.5, 10.5), at(11.5, 16.0), at(8.5, 17.5), at(8.5, 10.5)],
                st,
            ));
        }
        _ => {
            p.add(egui::Shape::closed_line(
                vec![at(5.0, 3.0), at(15.0, 12.0), at(10.5, 12.5), at(13.0, 17.5), at(11.0, 18.5), at(8.5, 13.5), at(5.0, 16.5)],
                st,
            ));
        }
    }
    r.clicked()
}

/// A label column of fixed width, then the control in what is left of the row.
fn row(ui: &mut egui::Ui, t: &crate::theme::Tokens, label: &str, tip: Option<String>, add: impl FnOnce(&mut egui::Ui, egui::Id, f32)) {
    ui.horizontal(|ui| {
        let lab = ui
            .allocate_ui_with_layout(egui::vec2(104.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_min_width(104.0);
                ui.add(egui::Label::new(RichText::new(label).color(t.text)).truncate())
            })
            .inner;
        if let Some(tip) = tip {
            lab.clone().on_hover_text(tip);
        }
        let w = (ui.available_width() - 8.0).max(40.0);
        add(ui, lab.id, w);
    });
}

/// A text / number field: Enter (or leaving a multi-line one) hands back the text when it changed; Esc puts the value
/// back. `full` is the text shown while editing (full precision).
#[allow(clippy::too_many_arguments)]
fn text_field(
    ui: &mut egui::Ui,
    edits: &mut std::collections::HashMap<String, String>,
    key: &str,
    shown: &str,
    full: Option<String>,
    hint: &str,
    multi: bool,
    enabled: bool,
    w: f32,
    lab: egui::Id,
) -> Option<String> {
    let editing = edits.contains_key(key);
    let mut buf = edits.get(key).cloned().unwrap_or_else(|| shown.to_string());
    let te = if multi { egui::TextEdit::multiline(&mut buf).desired_rows(2) } else { egui::TextEdit::singleline(&mut buf) };
    let r = ui.add(te.desired_width(w).hint_text(hint).interactive(enabled)).labelled_by(lab);
    if r.gained_focus()
        && !editing
        && let Some(f) = full
    {
        buf = f;
    }
    if r.has_focus() || r.changed() {
        edits.insert(key.to_string(), buf.clone());
    }
    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let commit = !esc && (enter || (r.lost_focus() && multi));
    if r.lost_focus() {
        edits.remove(key);
    }
    (commit && enabled && buf != shown).then_some(buf)
}

/// A select: the label shown, the choices → the value picked.
fn combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    current: &str,
    choices: &[(String, String)],
    key: Option<&str>,
    w: f32,
    enabled: bool,
) -> Option<String> {
    let mut out = None;
    ui.add_enabled_ui(enabled, |ui| {
        egui::ComboBox::new(egui::IdSalt::new(id), "").width((w - 16.0).max(30.0)).selected_text(current).show_ui(ui, |ui| {
            for (k, lab) in choices {
                if ui.selectable_label(key.is_some_and(|x| k.eq_ignore_ascii_case(x)), lab).clicked() {
                    out = Some(k.clone());
                }
            }
        });
    });
    out
}

/// What one field row does when it is used.
enum Act {
    Set(String, String),
    Vertex(i64),
    Pick(String),
}

/// One descriptor's row (proppanel.js `field`), with the point sub-heading and its pick button before an X field.
#[allow(clippy::too_many_arguments)]
fn field_rows(
    ui: &mut egui::Ui,
    t: &crate::theme::Tokens,
    l: &cc_engine::Lang,
    d: &cc_engine::props::Def,
    c: &PCtx,
    edits: &mut std::collections::HashMap<String, String>,
    prefix: &str,
    vertex: i64,
    pick: bool,
    acts: &mut Vec<Act>,
) {
    if pick && let Some((name, "x")) = d.pt {
        ui.horizontal(|ui| {
            ui.label(RichText::new(point_label(name, l)).color(t.text_dim).small());
            if icon_button(ui, "pick", &l.t("props.pick.title", &[("what", point_label(name, l).to_lowercase())]), t.text) {
                acts.push(Act::Pick(name.to_string()));
            }
        });
    }
    let v = cc_engine::props::value_of(d, c);
    let label = d.label.clone().unwrap_or_else(|| d.key.clone());
    let varies = v == PVal::Varies;
    let val = match &v {
        PVal::V(x) => x.clone(),
        PVal::Varies => Value::Null,
    };
    let ro = d.set.is_none();
    let doc = c.doc;
    let ekey = format!("{prefix}{}", d.key);
    row(ui, t, &label, d.title.map(|k| l.s(k)), |ui, lab, w| match d.kind {
        "ro" | "ro-len" | "ro-area" => {
            let txt = if varies { l.s("props.varies") } else { show(d.kind, &val, doc) };
            ui.label(RichText::new(txt).color(t.text_dim)).labelled_by(lab);
        }
        "bool" => {
            let mut on = val.as_bool().unwrap_or(false);
            let r = ui.checkbox(&mut on, if varies { l.s("props.varies") } else { String::new() }).labelled_by(lab);
            if r.changed() && !ro {
                acts.push(Act::Set(d.key.clone(), on.to_string()));
            }
        }
        "vertex" => {
            let n = val.as_f64().unwrap_or(1.0);
            let total =
                c.ents.first().map(cc_cad::Entity::to_obj).and_then(|o| o.get("pts").and_then(Value::as_array).map(Vec::len)).unwrap_or(0);
            if ui.small_button("◀").on_hover_text(l.s("props.vertex.prev")).clicked() {
                acts.push(Act::Vertex(vertex.saturating_sub(1).max(0)));
            }
            ui.label(format!("{} {}", js_num(n), l.t("props.vertex.of", &[("n", total.to_string())])));
            if ui.small_button("▶").on_hover_text(l.s("props.vertex.next")).clicked() {
                let last = i64::try_from(total).unwrap_or(1).saturating_sub(1).max(0);
                acts.push(Act::Vertex(vertex.saturating_add(1).min(last)));
            }
        }
        "len" | "ang" | "num" | "int" | "area" | "text" | "textarea" => {
            let shown = if varies {
                String::new()
            } else if matches!(d.kind, "text" | "textarea") {
                cc_engine::props::value_text(&val)
            } else {
                show(d.kind, &val, doc)
            };
            let full = matches!(d.kind, "len" | "ang" | "num" | "int" | "area").then(|| val.as_f64().map(edit_text)).flatten();
            let hint = if varies { l.s("props.varies") } else { String::new() };
            if let Some(txt) = text_field(ui, edits, &ekey, &shown, full, &hint, d.kind == "textarea", !ro, w, lab) {
                acts.push(Act::Set(d.key.clone(), txt));
            }
        }
        _ => {
            let choices = select_choices(d, c, l);
            let key = value_key(d.kind, &val);
            let current = if varies {
                l.s("props.varies")
            } else if d.kind == "color" {
                color_label(&val, l)
            } else {
                choices.iter().find(|(k, _)| k.eq_ignore_ascii_case(&key)).map_or_else(|| key.clone(), |(_, lab)| lab.clone())
            };
            if let Some(k) = combo(ui, ("props_sel", &ekey), &current, &choices, (!varies).then_some(key.as_str()), w, !ro) {
                acts.push(Act::Set(d.key.clone(), k));
            }
        }
    });
}

/// A foldable section header that remembers whether it is open → open?
fn section(
    ui: &mut egui::Ui,
    t: &crate::theme::Tokens,
    closed: &std::collections::HashSet<String>,
    key: &str,
    label: &str,
    folds: &mut Vec<String>,
) -> bool {
    let open = !closed.contains(key);
    ui.add_space(3.0);
    let r = ui
        .add(egui::Button::new(RichText::new(format!("{} {label}", if open { "▾" } else { "▸" })).strong().color(t.text_dim)).frame(false));
    if r.clicked() {
        folds.push(key.to_string());
    }
    open
}

/// Everything the panel asks for after drawing (applied once egui is done with the frame).
#[derive(Default)]
struct Asks {
    acts: Vec<Act>,
    group: Option<String>,
    close: bool,
    folds: Vec<String>,
    run: Vec<(String, String)>,
}

/// The panel (a right-hand side panel while a drawing is open for editing; with auto-hide a strip at the edge whose
/// panel opens over the drawing while the pointer is on it).
pub fn panel(app: &mut UiApp, ui: &mut egui::Ui) {
    let t = app.tokens;
    let mut asks = Asks::default();
    let frame = egui::Frame::new().fill(t.panel).inner_margin(6);
    if !app.cadprops.autohide {
        egui::Panel::right("cad_props").default_size(270.0).size_range(200.0..=440.0).resizable(true).frame(frame).show(ui, |ui| {
            body(app, ui, &mut asks);
        });
        apply(app, asks);
        return;
    }
    // auto-hide (props.js P.1): the strip, and the panel over the drawing while the pointer is on either
    let now = ui.input(|i| i.time);
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let mut strip_rect = egui::Rect::NOTHING;
    let mut strip_click = false;
    egui::Panel::right("cad_props_strip").exact_size(24.0).resizable(false).frame(egui::Frame::new().fill(t.panel_darker)).show(ui, |ui| {
        let rect = ui.max_rect();
        let r = ui.allocate_rect(rect, egui::Sense::click()).on_hover_text(lang().s("props.autohide.open"));
        strip_rect = rect;
        strip_click = r.clicked();
        let p = ui.painter();
        let col = if r.hovered() { t.accent } else { t.text };
        // the icon (sliders) and the panel's name read upwards, as the 0.5 strip
        let c = egui::pos2(rect.center().x, rect.min.y + 16.0);
        for (i, x) in [-4.0_f32, 0.0, 4.0].iter().enumerate() {
            p.line_segment([egui::pos2(c.x + x, c.y - 6.0), egui::pos2(c.x + x, c.y + 6.0)], egui::Stroke::new(1.2, col));
            let ky = [-2.0_f32, 3.0, -4.0].get(i).copied().unwrap_or(0.0);
            p.rect_filled(egui::Rect::from_center_size(egui::pos2(c.x + x, c.y + ky), egui::vec2(3.5, 2.5)), 0.5, col);
        }
        let g = p.layout_no_wrap(lang().s("props.title"), egui::FontId::proportional(12.0), col);
        let w = g.size().x;
        let at = egui::pos2(rect.center().x - g.size().y / 2.0, rect.min.y + 32.0 + w);
        p.add(egui::epaint::TextShape::new(at, g, col).with_angle(-std::f32::consts::FRAC_PI_2));
    });
    if strip_click || pointer.is_some_and(|q| strip_rect.contains(q)) {
        app.cadprops.flyout = true;
        app.cadprops.left_at = None;
    }
    if !app.cadprops.flyout {
        apply(app, asks);
        return;
    }
    let width = 280.0;
    let top = strip_rect.min.y;
    let area = egui::Area::new(egui::Id::unique("cad_props_flyout"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(strip_rect.min.x - width, top))
        .show(ui.ctx(), |ui| {
            frame.show(ui, |ui| {
                ui.set_width(width - 12.0);
                ui.set_height((strip_rect.height() - 12.0).max(80.0));
                body(app, ui, &mut asks);
            });
        });
    // props.js closeFlyout: folds 500 ms after the pointer leaves (not while one of its fields is being typed in, or
    // the field list is open)
    let inside = pointer.is_some_and(|q| area.response.rect.contains(q) || strip_rect.contains(q));
    let typing = !app.cadprops.edits.is_empty();
    if inside || typing || app.cadprops.custom.is_some() {
        app.cadprops.left_at = None;
    } else {
        let since = *app.cadprops.left_at.get_or_insert(now);
        if now - since > 0.5 {
            app.cadprops.flyout = false;
            app.cadprops.left_at = None;
        } else {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
    apply(app, asks);
}

/// The panel's contents (docked, or over the drawing when auto-hidden).
fn body(app: &mut UiApp, ui: &mut egui::Ui, asks: &mut Asks) {
    let t = app.tokens;
    let l = lang();
    let Some(ed) = app.editor.as_ref() else { return };
    let sel = ed.hs.borrow().selection.clone();
    if sel != app.cadprops.seen {
        app.cadprops.seen = sel.clone();
        app.cadprops.edits.clear();
        app.cadprops.vertex = 0;
        app.cadprops.note.clear();
    }
    let doc = ed.eng.doc();
    let ents: Vec<cc_cad::Entity> = sel.iter().filter_map(|id| doc.get(*id).cloned()).collect();
    let head = if ents.is_empty() {
        match doc.layout() {
            Some(lay) => l.t("props.sheet.h", &[("name", lay.name.clone())]),
            None => l.s("props.current"),
        }
    } else if ents.len() == 1 {
        ents.first().map(|e| type_name(e, &l)).unwrap_or_default()
    } else {
        l.t("props.n.objects", &[("n", ents.len().to_string())])
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(head).strong().color(t.text_strong));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("×").on_hover_text("PROPERTIESCLOSE").clicked() {
                asks.close = true;
            }
            // props.js head(): … PICKADD, the gear (a selection), Auto-hide
            let ah = app.cadprops.autohide;
            let pin_tip = l.s(if ah { "props.btn.autohide.on" } else { "props.btn.autohide.off" });
            if icon_button(ui, if ah { "pin" } else { "pin-fill" }, &pin_tip, if ah { t.accent } else { t.text }) {
                asks.run.push(("props.autohide".into(), String::new()));
            }
            if !ents.is_empty() && icon_button(ui, "gear", &l.s("props.btn.custom"), t.text) {
                asks.run.push(("props.custom".into(), String::new()));
            }
            let add = !app.cadset.pick_replace;
            let add_tip = l.s(if add { "props.btn.pickadd.on" } else { "props.btn.pickadd.off" });
            if icon_button(ui, if add { "plus-square" } else { "1-square" }, &add_tip, if add { t.accent } else { t.text }) {
                asks.run.push(("props.pickadd".into(), String::new()));
            }
            if icon_button(ui, "cursor", &l.s("props.btn.select"), t.text) {
                asks.run.push(("cmd.line".into(), "SELECT".into()));
            }
            if icon_button(ui, "funnel", &l.s("props.btn.qselect"), t.text) {
                asks.run.push(("cmd.line".into(), "QSELECT".into()));
            }
        });
    });
    ui.separator();
    if !app.cadprops.note.is_empty() {
        ui.label(RichText::new(&app.cadprops.note).color(egui::Color32::from_rgb(0xe0, 0x80, 0x60)));
    }
    let closed = app.cadprops.closed.clone();
    if ents.is_empty() {
        let edits = &mut app.cadprops.edits;
        let vb = ed.hs.borrow().view_box;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            none_selected(ui, &t, &l, &doc, vb, edits, &closed, asks);
        });
        return;
    }
    let groups = groups_of(&ents, &l);
    let gkey = app
        .cadprops
        .group
        .clone()
        .filter(|k| groups.iter().any(|g| &g.key == k))
        .or_else(|| groups.first().map(|g| g.key.clone()))
        .unwrap_or_default();
    if groups.len() > 1 {
        let cur_label = groups.iter().find(|g| g.key == gkey).map(|g| group_label(g, &l)).unwrap_or_default();
        egui::ComboBox::new("props_group", "").selected_text(cur_label).show_ui(ui, |ui| {
            for g in &groups {
                if ui.selectable_label(g.key == gkey, group_label(g, &l)).clicked() {
                    asks.group = Some(g.key.clone());
                }
            }
        });
    }
    let gids: Vec<u64> = groups.iter().find(|g| g.key == gkey).map(|g| g.ids.clone()).unwrap_or_default();
    let gents: Vec<cc_cad::Entity> = gids.iter().filter_map(|id| doc.get(*id).cloned()).collect();
    let vertex = app.cadprops.vertex;
    let c = PCtx { doc: &doc, ents: &gents, vertex, lang: &l };
    let all_defs = defs_for(&gkey, &c);
    // P.12: the fields chosen for this kind
    let defs = shown_in_panel(&app.cadprops.hide, &gkey, &all_defs);
    let one = (gents.len() == 1).then(|| gents.first().cloned()).flatten();
    let edits = &mut app.cadprops.edits;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for key in ALL_SECTIONS {
            let ds: Vec<&cc_engine::props::Def> = defs.iter().copied().filter(|d| d.sec == key).collect();
            let extra = one.as_ref().is_some_and(|e| has_extra(&doc, e, key));
            if ds.is_empty() && !extra {
                continue;
            }
            if !section(ui, &t, &closed, key, &l.s(&format!("props.sec.{key}")), &mut asks.folds) {
                continue;
            }
            if let Some(e) = one.as_ref() {
                extra_rows(ui, &t, &l, &doc, e, key, true, edits, asks);
            }
            for d in ds {
                field_rows(ui, &t, &l, d, &c, edits, "p:", vertex, true, &mut asks.acts);
            }
            if let Some(e) = one.as_ref() {
                extra_rows(ui, &t, &l, &doc, e, key, false, edits, asks);
            }
        }
        if gkey == "all" {
            let parts: Vec<String> =
                groups.iter().filter(|g| g.key != "all").map(|g| format!("{} {}", g.ids.len(), g.label.to_lowercase())).collect();
            ui.label(RichText::new(parts.join(", ")).small().color(t.text_dim));
        }
    });
}

/// P.12's list (props.js `customize`): every field of the kind with a tick for Properties and one for Quick
/// Properties; Done keeps them, "Back to the standard set" forgets them.
pub fn custom_window(app: &mut UiApp, ctx: &egui::Context) {
    let Some((_, rows)) = app.cadprops.custom.clone() else { return };
    let l = lang();
    let t = app.tokens;
    let name = app.cadprops.custom_name.clone();
    let mut run: Vec<(&str, Option<String>)> = Vec::new();
    let mut open = true;
    egui::Window::new(l.t("props.custom.title", &[("type", name)]))
        .id(egui::Id::unique("props_custom"))
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(380.0)
        .show(ctx, |ui| {
            ui.label(RichText::new(l.s("props.custom.text")).small().color(t.text_dim));
            ui.add_space(4.0);
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                egui::Grid::new("props_custom_grid").striped(true).num_columns(3).show(ui, |ui| {
                    ui.label(RichText::new(l.s("props.custom.field")).strong());
                    ui.label(RichText::new(l.s("props.title")).strong());
                    ui.label(RichText::new(l.s("props.qp.title")).strong());
                    ui.end_row();
                    for (k, label, panel, qp) in &rows {
                        let lab = ui.label(label);
                        let (mut a, mut b) = (*panel, *qp);
                        if ui
                            .checkbox(&mut a, "")
                            .labelled_by(lab.id)
                            .on_hover_text(l.t("props.custom.inpanel", &[("what", label.clone())]))
                            .changed()
                        {
                            run.push(("props.custom.set", Some(format!("panel:{k}={}", if a { "on" } else { "off" }))));
                        }
                        if ui
                            .checkbox(&mut b, "")
                            .labelled_by(lab.id)
                            .on_hover_text(l.t("props.custom.inqp", &[("what", label.clone())]))
                            .changed()
                        {
                            run.push(("props.custom.set", Some(format!("qp:{k}={}", if b { "on" } else { "off" }))));
                        }
                        ui.end_row();
                    }
                });
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.link(l.s("props.custom.reset")).clicked() {
                    run.push(("props.custom.reset", None));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(l.s("props.custom.ok")).clicked() {
                        run.push(("props.custom.ok", None));
                    }
                    if ui.button(l.s("props.custom.cancel")).clicked() {
                        run.push(("props.custom.cancel", None));
                    }
                });
            });
        });
    if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        run.push(("props.custom.cancel", None));
    } else if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
        run.push(("props.custom.ok", None));
    }
    for (id, name) in run {
        app.run_ui_pub(id, &Params { name, ..Default::default() });
        if app.cadprops.custom.is_none() {
            break;
        }
    }
}

/// Quick Properties' window (drawn whether or not the panel is open).
pub fn quick_window(app: &mut UiApp, ctx: &egui::Context) {
    let mut asks = Asks::default();
    quick(app, ctx, &mut asks);
    apply(app, asks);
}

/// doc.js `typeName(e)`: what one object is called ("Line", "Block reference").
fn type_name(e: &cc_cad::Entity, l: &cc_engine::Lang) -> String {
    groups_of(std::slice::from_ref(e), l).into_iter().find(|g| g.key != "all").map(|g| g.label).unwrap_or_default()
}

/// The order sections show in (props.js ALL_SECTIONS, with the sheet and the current settings for nothing selected).
const ALL_SECTIONS: [&str; 13] =
    ["general", "geometry", "custom", "text", "lines", "fit", "units", "alt", "tol", "pattern", "misc", "attrs", "library"];

fn has_extra(doc: &cc_cad::Doc, e: &cc_cad::Entity, sec: &str) -> bool {
    match sec {
        "geometry" | "misc" if matches!(e.geo, cc_cad::Geo::Viewport(_)) && doc.layout().is_some() => true,
        "text" => is_attdef(e),
        "attrs" => !cc_engine::propextra::attribute_rows(doc, e).0.is_empty(),
        "library" => cc_engine::propextra::attribute_rows(doc, e).1.is_some(),
        "misc" => {
            e.typ() == "dim"
                && e.get("block").is_some_and(|b| cc_cad::js::truthy(Some(b)))
                && e.get("kind").is_some_and(|b| cc_cad::js::truthy(Some(b)))
        }
        _ => false,
    }
}

fn is_attdef(e: &cc_cad::Entity) -> bool {
    matches!(e.geo, cc_cad::Geo::Text(_)) && e.get("attdef").and_then(|a| a.get("tag")).is_some_and(Value::is_string)
}

/// props.js `extrasFor(e)`: one object's extra rows in a section (`before`: rows that come before the fields).
#[allow(clippy::too_many_arguments)]
fn extra_rows(
    ui: &mut egui::Ui,
    t: &crate::theme::Tokens,
    l: &cc_engine::Lang,
    doc: &cc_cad::Doc,
    e: &cc_cad::Entity,
    sec: &str,
    before: bool,
    edits: &mut std::collections::HashMap<String, String>,
    asks: &mut Asks,
) {
    if let cc_cad::Geo::Viewport(vp) = &e.geo
        && let Some(lay) = doc.layout()
    {
        let unit = lay.unit().unwrap_or("mm").to_string();
        let locked = e.get("locked").is_some_and(|v| cc_cad::js::truthy(Some(v)));
        let lock_tip = locked.then(|| l.s("props.err.vplocked"));
        if sec == "geometry" && before {
            let list = cc_io_cad::plot::scale_list(doc);
            let cur_n = cc_io_cad::plot::n_of(doc, &unit, vp.s);
            let known = list.iter().find(|s| (s.n - cur_n).abs() < 1e-6 * cur_n);
            let current = known.map_or_else(|| cc_io_cad::plot::scale_label(doc, &unit, vp.s), |s| s.label.clone());
            let mut choices: Vec<(String, String)> = list.iter().map(|s| (js_num(s.n), s.label.clone())).collect();
            choices.push(("fit".into(), l.s("props.vp.fitall")));
            row(ui, t, &l.s("props.vp.scale"), lock_tip.clone(), |ui, _, w| {
                if let Some(k) = combo(ui, "props_vpscale", &current, &choices, known.map(|s| js_num(s.n)).as_deref(), w, !locked) {
                    asks.run.push(("props.vpscale".into(), k));
                }
            });
        }
        if sec == "misc" && !before {
            ui.horizontal(|ui| {
                ui.add_enabled_ui(!locked, |ui| {
                    if ui.link(l.s("props.vp.showall")).clicked() {
                        asks.run.push(("props.vpscale".into(), "showall".into()));
                    }
                    if ui.link(l.s("props.vp.window")).clicked() {
                        asks.run.push(("cmd.line".into(), "VPVIEW".into()));
                    }
                });
            });
            let unit_name = l.s(if unit == "in" { "props.unit.in" } else { "props.unit.mm" });
            let model = doc
                .meta
                .get("units")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map_or_else(|| l.t("props.vp.nounits", &[("unit", unit_name.clone())]), str::to_string);
            ui.label(RichText::new(l.t("props.vp.units", &[("model", model), ("paper", unit_name)])).small().color(t.text_dim));
        }
    }
    if sec == "text" && before && is_attdef(e) {
        let a = e.get("attdef").cloned().unwrap_or(Value::Null);
        let s = |k: &str| a.get(k).map(cc_cad::js::to_js_string).unwrap_or_default();
        ui.label(RichText::new(l.s("props.att.modes")).small().color(t.text_dim));
        for (k, key) in [("tag", "props.att.tag"), ("prompt", "props.att.prompt"), ("def", "props.att.def")] {
            let shown = s(k);
            row(ui, t, &l.s(key), None, |ui, lab, w| {
                if let Some(v) = text_field(ui, edits, &format!("attdef:{k}"), &shown, None, "", false, true, w, lab) {
                    asks.run.push(("props.attdef".into(), format!("{k}={v}")));
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            for (k, key) in [("invisible", "opt.invisible"), ("constant", "opt.constant"), ("preset", "opt.preset")] {
                let mut on = a.get(k).is_some_and(|v| cc_cad::js::truthy(Some(v)));
                if ui.checkbox(&mut on, l.s(key)).changed() {
                    asks.run.push(("props.attdef".into(), format!("{k}={on}")));
                }
            }
        });
    }
    if sec == "attrs" && !before {
        let (rows, _) = cc_engine::propextra::attribute_rows(doc, e);
        for a in rows {
            let mut label = a.tag.clone();
            if a.invisible {
                label.push(' ');
                label.push_str(&l.s("props.att.hidden"));
            }
            row(ui, t, &label, Some(a.tag.clone()), |ui, lab, w| {
                if a.locked {
                    ui.label(RichText::new(if a.value.is_empty() { "–".to_string() } else { a.value.clone() }).color(t.text_dim))
                        .labelled_by(lab);
                } else if let Some(v) = text_field(ui, edits, &format!("attr:{}", a.tag), &a.value, None, "", false, true, w, lab) {
                    asks.run.push(("props.attr".into(), format!("{}={v}", a.tag)));
                }
            });
        }
    }
    if sec == "library"
        && !before
        && let (_, Some((licence, credit))) = cc_engine::propextra::attribute_rows(doc, e)
    {
        {
            row(ui, t, &l.s("props.lib.license"), None, |ui, _, _| {
                ui.label(if licence.is_empty() { "–".to_string() } else { licence });
            });
            if !credit.is_empty() {
                ui.label(RichText::new(credit).small().color(t.text_dim));
            }
            if ui.link(l.s("props.lib.updates")).clicked() {
                asks.run.push(("lib.open".into(), String::new()));
            }
        }
    }
    if sec == "misc" && !before && e.typ() == "dim" && has_extra(doc, e, "misc") && !matches!(e.geo, cc_cad::Geo::Viewport(_)) {
        ui.label(RichText::new(l.s("props.dim.imported")).small().color(t.text_dim));
    }
}

/// props.js `renderNone()`: nothing selected — the sheet, what new objects get, the View, Misc.
#[allow(clippy::too_many_arguments)]
fn none_selected(
    ui: &mut egui::Ui,
    t: &crate::theme::Tokens,
    l: &cc_engine::Lang,
    doc: &cc_cad::Doc,
    vb: Option<cc_cad_geom::geom::Bounds>,
    edits: &mut std::collections::HashMap<String, String>,
    closed: &std::collections::HashSet<String>,
    asks: &mut Asks,
) {
    let lay = doc.layout();
    let unit_name = |u: &str| l.s(if u == "in" { "props.unit.in" } else { "props.unit.mm" });
    if let Some(lay) = lay
        && section(ui, t, closed, "sheet", &l.s("props.sec.sheet"), &mut asks.folds)
    {
        row(ui, t, &l.s("props.sheet.name"), None, |ui, lab, w| {
            if let Some(v) = text_field(ui, edits, "sheet:name", &lay.name, None, "", false, true, w, lab) {
                asks.run.push(("props.sheet".into(), format!("name={v}")));
            }
        });
        let key = lay.key().unwrap_or("");
        let std = cc_io_cad::plot::paper_by_key(key);
        let mut papers: Vec<(String, String)> = Vec::new();
        if std.is_none() {
            let n = |k: &str| lay.paper.get(k).and_then(Value::as_f64).unwrap_or(0.0);
            papers.push((
                String::new(),
                l.t("props.sheet.custom", &[("w", fmt4(n("w"))), ("h", fmt4(n("h"))), ("unit", lay.unit().unwrap_or("").to_string())]),
            ));
        }
        for p in cc_io_cad::plot::PAPERS.iter() {
            papers.push((p.key.to_string(), p.label.to_string()));
        }
        let cur = papers.iter().find(|(k, _)| k == key || (std.is_none() && k.is_empty())).map(|x| x.1.clone()).unwrap_or_default();
        row(ui, t, &l.s("props.sheet.paper"), None, |ui, _, w| {
            if let Some(k) = combo(ui, "props_sheet_paper", &cur, &papers, Some(if std.is_some() { key } else { "" }), w, true) {
                asks.run.push(("props.sheet".into(), format!("paper={k}")));
            }
        });
        let portrait = lay.h().unwrap_or(0.0) > lay.w().unwrap_or(0.0);
        let orients = [("landscape".to_string(), l.s("props.sheet.landscape")), ("portrait".to_string(), l.s("props.sheet.portrait"))];
        let cur = if portrait { l.s("props.sheet.portrait") } else { l.s("props.sheet.landscape") };
        row(ui, t, &l.s("props.sheet.orient"), None, |ui, _, w| {
            if let Some(k) = combo(ui, "props_sheet_orient", &cur, &orients, Some(if portrait { "portrait" } else { "landscape" }), w, true)
            {
                asks.run.push(("props.sheet".into(), format!("orient={k}")));
            }
        });
        ui.horizontal(|ui| {
            if ui.button(l.s("props.sheet.plot")).clicked() {
                asks.run.push(("cmd.line".into(), "PLOT".into()));
            }
            if ui.link(RichText::new(l.s("props.sheet.delete")).color(egui::Color32::from_rgb(0xe0, 0x60, 0x50))).clicked() {
                asks.run.push(("props.sheet".into(), "delete".into()));
            }
        });
    }
    let mut more: Vec<(String, String)> = Vec::new();
    if section(ui, t, closed, "current", &l.s("props.sec.current"), &mut asks.folds) {
        let c = &doc.current;
        let mut sel = |ui: &mut egui::Ui, key: &str, label: String, kind: &str, val: Value, cur_label: String| {
            let choices = kind_choices(kind, false, doc, l);
            let k = value_key(kind, &val);
            row(ui, t, &label, None, |ui, _, w| {
                if let Some(v) = combo(ui, ("props_cur", key), &cur_label, &choices, Some(k.as_str()), w, true) {
                    asks.run.push(("props.current".into(), format!("{key}={v}")));
                }
            });
        };
        sel(ui, "layer", l.s("props.f.layer"), "layer", Value::String(c.layer.clone()), c.layer.clone());
        sel(ui, "color", l.s("props.f.color"), "color", c.color.to_value(), color_label(&c.color.to_value(), l));
        let lt = c.linetype.clone().map_or(Value::Null, Value::String);
        sel(ui, "lt", l.s("props.lt"), "lt", lt.clone(), c.linetype.clone().unwrap_or_else(|| l.s("lt.bylayer")));
        let lw = c.lineweight.map_or(Value::Null, cc_cad::num);
        let lw_label = c.lineweight.map_or_else(
            || l.s("lt.bylayer"),
            |w| {
                if w == -2.0 {
                    l.s("lt.byblock")
                } else if w == -3.0 {
                    l.s("lw.default")
                } else {
                    l.t("lw.mm", &[("mm", cc_cad::js::to_fixed(w, 2))])
                }
            },
        );
        sel(ui, "lw", l.s("props.lw"), "lw", lw, lw_label);
        let mut num_row = |ui: &mut egui::Ui, key: &str, label: String, kind: &str, v: f64, tip: Option<String>| {
            let shown = show(kind, &cc_cad::num(v), doc);
            row(ui, t, &label, tip, |ui, lab, w| {
                if let Some(txt) = text_field(ui, edits, &format!("cur:{key}"), &shown, Some(edit_text(v)), "", false, true, w, lab) {
                    asks.run.push(("props.current".into(), format!("{key}={txt}")));
                }
            });
        };
        num_row(ui, "th", l.s("props.cur.textheight"), "len", c.text_height, None);
        if lay.is_none() {
            // props.js unitsSelect: what one drawing unit stands for (with solids, the 3D model follows)
            let cur_u = doc.meta.get("units").filter(|v| cc_cad::js::truthy(Some(v))).map(cc_cad::js::to_js_string).unwrap_or_default();
            let uname = |u: &str| l.s(&format!("props.units.{}", if u.is_empty() { "none" } else { u }));
            let mut units: Vec<(String, String)> = Vec::new();
            if !UNITS.contains(&cur_u.as_str()) {
                units.push((cur_u.clone(), cur_u.clone()));
            }
            units.extend(UNITS.iter().map(|u| ((*u).to_string(), uname(u))));
            let ucur = if UNITS.contains(&cur_u.as_str()) { uname(&cur_u) } else { cur_u.clone() };
            row(ui, t, &l.s("props.cur.units"), Some(l.s("props.cur.units.title")), |ui, _, w| {
                if let Some(v) = combo(ui, "props_cur_units", &ucur, &units, Some(cur_u.as_str()), w, true)
                    && v != cur_u
                {
                    more.push(("props.units".into(), v));
                }
            });
            let (code, prec) = cc_cad_geom::units::units_of(Some(&doc.meta));
            let formats: Vec<(String, String)> =
                [1, 2, 3, 4, 5].iter().map(|k| (k.to_string(), cc_engine::cmd::units::length_format_name(l, *k))).collect();
            let fcur = cc_engine::cmd::units::length_format_name(l, code);
            row(ui, t, &l.s("props.lunits"), Some(l.s("props.lunits.title")), |ui, _, w| {
                if let Some(v) = combo(ui, "props_cur_lunits", &fcur, &formats, Some(&code.to_string()), w, true) {
                    more.push(("props.current".into(), format!("lunits={v}")));
                }
            });
            let frac = code == 4 || code == 5;
            let precs: Vec<(String, String)> = (0..=8)
                .map(|n: i32| {
                    let lab = if frac {
                        if n > 0 { format!("1/{}", 2i64.pow(n.unsigned_abs())) } else { "1".into() }
                    } else if n > 0 {
                        format!("0.{}1", "0".repeat(usize::try_from(n - 1).unwrap_or(0)))
                    } else {
                        "0".into()
                    };
                    (n.to_string(), lab)
                })
                .collect();
            let pcur = precs.iter().find(|(k, _)| *k == prec.to_string()).map(|x| x.1.clone()).unwrap_or_default();
            row(ui, t, &l.s("props.luprec"), None, |ui, _, w| {
                if let Some(v) = combo(ui, "props_cur_luprec", &pcur, &precs, Some(&prec.to_string()), w, true) {
                    more.push(("props.current".into(), format!("luprec={v}")));
                }
            });
            let lts = doc.meta.get("ltscale").and_then(Value::as_f64).filter(|v| *v > 0.0).unwrap_or(1.0);
            num_row(ui, "ltscale", l.s("props.ltscale"), "num", lts, Some(l.s("props.ltscale.title")));
        }
        ui.label(RichText::new(l.t("props.layer.sub", &[("layer", c.layer.clone())])).small().color(t.text_dim));
        let layer = doc.layers.get(&c.layer);
        let llt = layer.and_then(|x| x.linetype.clone()).unwrap_or_else(|| "Continuous".into());
        let llw = layer.and_then(|x| x.lineweight).unwrap_or(-3.0);
        let mut sel2 = |ui: &mut egui::Ui, key: &str, label: String, kind: &str, val: Value, cur_label: String| {
            let choices: Vec<(String, String)> =
                kind_choices(kind, false, doc, l).into_iter().filter(|(k, _)| !k.is_empty() && k != "ByBlock" && k != "-2").collect();
            let k = value_key(kind, &val);
            row(ui, t, &label, None, |ui, _, w| {
                if let Some(v) = combo(ui, ("props_lay", key), &cur_label, &choices, Some(k.as_str()), w, true) {
                    asks.run.push(("props.current".into(), format!("{key}={v}")));
                }
            });
        };
        sel2(ui, "laylt", l.s("props.lt"), "lt", Value::String(llt.clone()), llt.clone());
        let llw_label = if llw == -3.0 { l.s("lw.default") } else { l.t("lw.mm", &[("mm", cc_cad::js::to_fixed(llw, 2))]) };
        sel2(ui, "laylw", l.s("props.lw"), "lw", cc_cad::num(llw), llw_label);
        ui.label(RichText::new(l.s("props.cur.newdims")).small().color(t.text_dim));
        let dim = |k: &str, d: f64| c.dim.get(k).and_then(Value::as_f64).unwrap_or(d);
        let lunit = dim("lunit", 2.0);
        let mut num_row2 = |ui: &mut egui::Ui, key: &str, label: String, kind: &str, v: f64, tip: Option<String>| {
            let shown = show(kind, &cc_cad::num(v), doc);
            row(ui, t, &label, tip, |ui, lab, w| {
                if let Some(txt) = text_field(ui, edits, &format!("cur:{key}"), &shown, Some(edit_text(v)), "", false, true, w, lab) {
                    asks.run.push(("props.current".into(), format!("{key}={txt}")));
                }
            });
        };
        num_row2(ui, "dim.txt", l.s("props.dim.txt"), "len", dim("txt", 2.5), None);
        num_row2(ui, "dim.asz", l.s("props.dim.asz"), "len", dim("asz", 2.5), None);
        let dec_label = if lunit == 4.0 || lunit == 5.0 { l.s("props.dim.frac") } else { l.s("props.dim.dec") };
        num_row2(ui, "dim.dec", dec_label, "int", dim("dec", 4.0), None);
        num_row2(ui, "dim.scale", l.s("props.dim.scale"), "num", dim("scale", 1.0), Some(l.s("props.dim.scale.title")));
        let note = match lay {
            Some(lay) => l.t("props.cur.onsheet", &[("unit", unit_name(lay.unit().unwrap_or("mm")))]),
            None => l.s("props.cur.hint"),
        };
        ui.label(RichText::new(note).small().color(t.text_dim));
    }
    asks.run.append(&mut more);
    if section(ui, t, closed, "view", &l.s("props.sec.view"), &mut asks.folds)
        && let Some(b) = vb
    {
        let (cx, cy, vw, vh) = ((b.min_x + b.max_x) / 2.0, (b.min_y + b.max_y) / 2.0, b.max_x - b.min_x, b.max_y - b.min_y);
        for (key, label, v) in [("cx", "props.view.cx", cx), ("cy", "props.view.cy", cy)] {
            let shown = show("len", &cc_cad::num(v), doc);
            row(ui, t, &l.s(label), None, |ui, lab, w| {
                if let Some(txt) = text_field(ui, edits, &format!("view:{key}"), &shown, Some(edit_text(v)), "", false, true, w, lab) {
                    asks.run.push(("props.view".into(), format!("{key}={txt}")));
                }
            });
        }
        row(ui, t, &l.s("props.view.cz"), None, |ui, _, _| {
            ui.label(RichText::new(show("len", &cc_cad::num(0.0), doc)).color(t.text_dim));
        });
        for (key, label, v) in [("h", "props.view.h", vh), ("w", "props.view.w", vw)] {
            let shown = show("len", &cc_cad::num(v), doc);
            row(ui, t, &l.s(label), None, |ui, lab, w| {
                if let Some(txt) = text_field(ui, edits, &format!("view:{key}"), &shown, Some(edit_text(v)), "", false, true, w, lab) {
                    asks.run.push(("props.view".into(), format!("{key}={txt}")));
                }
            });
        }
    }
    if section(ui, t, closed, "misc", &l.s("props.sec.misc"), &mut asks.folds) {
        row(ui, t, &l.s("props.misc.ucs"), None, |ui, _, _| {
            ui.label(l.s("props.misc.world"));
        });
        row(ui, t, &l.s("props.misc.annoscale"), Some(l.s("props.misc.annoscale.title")), |ui, _, _| {
            ui.label("1:1");
        });
    }
}

/// props.js `QP_DEFAULT`: the fields Quick Properties shows per type.
fn qp_keys(key: &str) -> &'static [&'static str] {
    match key {
        "line" => &["layer", "color", "len", "ang"],
        "circle" => &["layer", "color", "r", "area"],
        "arc" => &["layer", "color", "r", "sweep", "arclen"],
        "ellipse" => &["layer", "color", "major", "minor"],
        "poly" => &["layer", "color", "closed", "gw", "plen", "area"],
        "text" => &["layer", "color", "s", "h", "rot", "just"],
        "insert" => &["layer", "color", "block", "rot", "us"],
        "hatch" => &["layer", "color", "pattern", "pscale", "pangle"],
        "dim" => &["layer", "color", "dimstyle", "measured", "dim.text", "dim.dec"],
        "mleader" => &["layer", "color", "s", "h"],
        "point" => &["layer", "color", "pos.x", "pos.y"],
        "viewport" => &["layer", "on", "locked", "vw", "vh"],
        _ => &["layer", "color", "lt"],
    }
}

/// Quick Properties (P.10): the most-used fields by the pointer when objects are selected and no command runs.
fn quick(app: &mut UiApp, ctx: &egui::Context, asks: &mut Asks) {
    let Some(ed) = app.editor.as_ref() else { return };
    let sel = ed.hs.borrow().selection.clone();
    if !app.cadprops.quick || sel.is_empty() || ed.eng.running().is_some() || app.cadprops.quick_dismissed == sel {
        app.cadprops.quick_at = None;
        return;
    }
    let t = app.tokens;
    let l = lang();
    let doc = ed.eng.doc();
    let ents: Vec<cc_cad::Entity> = sel.iter().filter_map(|id| doc.get(*id).cloned()).collect();
    if ents.is_empty() {
        return;
    }
    let groups = groups_of(&ents, &l);
    let Some(g) = app.cadprops.group.as_ref().and_then(|k| groups.iter().find(|g| &g.key == k)).or(groups.first()) else { return };
    let gents: Vec<cc_cad::Entity> = g.ids.iter().filter_map(|id| doc.get(*id).cloned()).collect();
    let c = PCtx { doc: &doc, ents: &gents, vertex: app.cadprops.vertex, lang: &l };
    let all = defs_for(&g.key, &c);
    // P.12 `shownIn('quick')`: the fields chosen for Quick Properties, else its standard set
    let defs: Vec<&cc_engine::props::Def> = match app.cadprops.qp.get(&g.key) {
        Some(keys) => keys.iter().filter_map(|k| all.iter().find(|d| &d.key == k)).collect(),
        None => qp_keys(&g.key).iter().filter_map(|k| all.iter().find(|d| d.key == *k)).collect(),
    };
    // beside the pointer where the selection was made
    let at = match &app.cadprops.quick_at {
        Some((s, p)) if *s == sel => *p,
        _ => {
            // props.js placeQuick: right of and below the pointer, else to its left / kept above the bottom edge
            let r = ctx.content_rect();
            let c = ctx.pointer_latest_pos().unwrap_or_else(|| r.center());
            let (w, h) = (256.0, 200.0);
            let mut p = c + egui::vec2(24.0, 24.0);
            if p.x + w > r.max.x - 8.0 {
                p.x = (c.x - 24.0 - w).max(r.min.x + 8.0);
            }
            if p.y + h > r.max.y - 8.0 {
                p.y = (r.max.y - 8.0 - h).max(r.min.y + 8.0);
            }
            app.cadprops.quick_at = Some((sel.clone(), p));
            p
        }
    };
    let name = if ents.len() == 1 { ents.first().map(|e| type_name(e, &l)).unwrap_or_default() } else { group_label(g, &l) };
    let mut close = false;
    let edits = &mut app.cadprops.edits;
    egui::Window::new(name)
        .id(egui::Id::unique("cad_quick_props"))
        .title_bar(true)
        .collapsible(false)
        .resizable(false)
        .default_width(240.0)
        .current_pos(at)
        .constrain(true)
        .show(ctx, |ui| {
            if groups.len() > 1 {
                egui::ComboBox::new("qp_group", "").selected_text(group_label(g, &l)).show_ui(ui, |ui| {
                    for x in &groups {
                        if ui.selectable_label(x.key == g.key, group_label(x, &l)).clicked() {
                            asks.group = Some(x.key.clone());
                        }
                    }
                });
            }
            for d in defs {
                field_rows(ui, &t, &l, d, &c, edits, "qp:", 0, false, &mut asks.acts);
            }
            if ui.small_button(l.s("props.qp.close")).clicked() {
                close = true;
            }
        });
    if close {
        app.cadprops.quick_dismissed = sel;
    }
}

fn apply(app: &mut UiApp, asks: Asks) {
    if asks.close {
        app.run_ui_pub("props.close", &Params::default());
    }
    for k in asks.folds {
        app.run_ui_pub("props.fold", &Params { name: Some(k), ..Default::default() });
    }
    if let Some(g) = asks.group {
        app.run_ui_pub("props.group", &Params { name: Some(g), ..Default::default() });
    }
    for a in asks.acts {
        match a {
            Act::Set(k, raw) => app.run_ui_pub("props.set", &Params { name: Some(format!("{k}={raw}")), ..Default::default() }),
            Act::Vertex(v) => app.run_ui_pub("props.vertex", &Params { index: usize::try_from(v).ok(), ..Default::default() }),
            Act::Pick(n) => app.run_ui_pub("props.pick", &Params { name: Some(n), ..Default::default() }),
        }
    }
    for (id, name) in asks.run {
        app.run_ui_pub(&id, &Params { name: Some(name), ..Default::default() });
    }
}
