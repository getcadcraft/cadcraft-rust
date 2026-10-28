//! The drawing viewer's details (BimCraft 0.5's DWG view: bimcraft.js `openDrawing` + dwg-view.js): print view (black
//! on white, layers that never print hidden, line weights; P), the Drawing panel (file facts, linked drawings and
//! pictures with "Add linked drawings…" / "Add pictures…" through the platform picker — on the web the browser's
//! picker, so a DWG's pictures and links are found there too), Find (the drawing's texts; a hit is shown ringed, as
//! `focusModelPoint` does), touch (one finger pans, two pinch-zoom about their middle and pan, a double tap fits),
//! fit on F. Words: BimCraft's catalog (`dwg.*`, `info.*`) and CadCraft's Find dialog (`dlg.find.*`).
//!
//! Commands (`viewer.*`): `viewer.print`, `viewer.info`, `viewer.find {name: text}`, `viewer.find.set {name:
//! "case=true" | "whole=true"}`, `viewer.find.show {index}`, `viewer.find.close`, `viewer.add {name: "drawing" |
//! "pictures"}`, `viewer.mark.clear`.

use std::collections::HashMap;

use egui::RichText;

use crate::lang::{self, BIMCRAFT, CADCRAFT};
use crate::{ImportedFile, Params, UiApp};

/// The viewer's own state.
#[derive(Debug, Default)]
pub struct Viewer {
    /// print view (dwg-view.js `setPrint`)
    pub print: bool,
    /// the Drawing panel (right)
    pub info: bool,
    pub find: Option<Find>,
    /// a ringed point: (space index, x, y)
    pub mark: Option<(usize, f64, f64)>,
    /// linked drawings added by hand (name → bytes), looked up before the drawing's folder
    pub added: HashMap<String, Vec<u8>>,
    /// web: the picker is open for "drawing" or "pictures"
    pub pick: Option<&'static str>,
    /// the pictures that could not be decoded (picture key)
    pub unreadable: Vec<String>,
}

/// Find over the drawing's texts.
#[derive(Debug, Default, Clone)]
pub struct Find {
    pub query: String,
    pub case: bool,
    pub whole: bool,
    pub hits: Vec<Hit>,
    pub ran: bool,
}

/// A text that matched: where it is and what it says.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub space: usize,
    pub x: f64,
    pub y: f64,
    pub s: String,
}

pub const UI_COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("viewer.print", "Print view", "P", "{} → black on white with line weights; layers that never print are hidden (again: back)"),
    ("viewer.info", "Drawing panel", "", "{} → shows or hides the drawing's facts, linked drawings and pictures"),
    ("viewer.find", "Find text", "Ctrl+F", "{name: text} → lists the drawing's texts that hold it (every space)"),
    ("viewer.find.set", "Find: options", "", "{name: \"case=true|false\" | \"whole=true|false\"} → runs the search again"),
    ("viewer.find.show", "Find: show a hit", "", "{index} → goes to that text (its space) and rings it"),
    ("viewer.find.close", "Find: close", "", "{}"),
    (
        "viewer.add",
        "Add linked drawings or pictures",
        "",
        "{name: \"drawing\" | \"pictures\"} → the platform picker; the files found are used at once",
    ),
    ("viewer.mark.clear", "Clear the ring", "", "{}"),
];

/// Is the plain viewer showing (a drawing open, not for editing)?
pub fn active(app: &UiApp) -> bool {
    app.doc.is_some() && app.editor.is_none()
}

/// `num(n)`: en-US grouping (ui-util.js, whatever the language).
pub fn num(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `bytes(n)` (ui-util.js).
pub fn bytes(n: usize) -> String {
    let f = n as f64;
    if n < 1024 {
        format!("{n} B")
    } else if n < 1_048_576 {
        format!("{:.0} KB", f / 1024.0)
    } else if n < 1_073_741_824 {
        format!("{:.1} MB", f / 1_048_576.0)
    } else {
        format!("{:.2} GB", f / 1_073_741_824.0)
    }
}

/// The status bar's cursor readout (bimcraft.js `coord`, `dwg.coords`).
pub fn coords(x: f64, y: f64) -> String {
    let c = |v: f64| {
        if v.abs() >= 1e5 {
            format!("{v:.0}")
        } else if v.abs() >= 100.0 {
            format!("{v:.2}")
        } else {
            format!("{v:.3}")
        }
    };
    lang::t(BIMCRAFT, "dwg.coords", &[("x", c(x)), ("y", c(y))])
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Does `text` hold `q` (findtext.js `findRegExp` without wildcards: match case, whole words)?
pub fn matches(text: &str, q: &str, case: bool, whole: bool) -> bool {
    if q.is_empty() {
        return false;
    }
    let (t, q) = if case { (text.to_string(), q.to_string()) } else { (text.to_lowercase(), q.to_lowercase()) };
    let mut from = 0usize;
    while let Some(i) = t.get(from..).and_then(|r| r.find(&q)) {
        let at = from.saturating_add(i);
        let end = at.saturating_add(q.len());
        let before_ok = t.get(..at).and_then(|b| b.chars().next_back()).is_none_or(|c| !is_word(c));
        let after_ok = t.get(end..).and_then(|a| a.chars().next()).is_none_or(|c| !is_word(c));
        if !whole || (before_ok && after_ok) {
            return true;
        }
        from = at.saturating_add(t.get(at..).and_then(|r| r.chars().next()).map_or(1, char::len_utf8));
    }
    false
}

/// The drawing's texts holding the query (model first, then the sheets in tab order), at most 2,000.
pub fn search(d: &cc_drawing::Drawing, f: &Find) -> Vec<Hit> {
    let mut out = Vec::new();
    for (si, sp) in d.spaces.iter().enumerate() {
        for b in &sp.batches {
            for tx in &b.texts {
                if out.len() >= 2000 {
                    return out;
                }
                if matches(&tx.s, &f.query, f.case, f.whole) {
                    out.push(Hit { space: si, x: tx.x, y: tx.y, s: tx.s.clone() });
                }
            }
        }
    }
    out
}

pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    match id {
        "viewer.print" => app.viewer.print = !app.viewer.print,
        "viewer.info" => app.viewer.info = !app.viewer.info,
        "viewer.mark.clear" => app.viewer.mark = None,
        "viewer.find" => {
            let q = params.name.clone().unwrap_or_default();
            if q.chars().count() > 500 {
                return Err("viewer.find: at most 500 characters".into());
            }
            let f = app.viewer.find.get_or_insert_with(Find::default);
            f.query = q;
            refind(app);
        }
        "viewer.find.set" => {
            let spec = params.name.clone().ok_or("viewer.find.set: name = \"case=true\" or \"whole=true\"")?;
            let (k, v) = spec.split_once('=').ok_or("viewer.find.set: name = \"<option>=true|false\"")?;
            let on = match v {
                "true" | "1" | "on" => true,
                "false" | "0" | "off" => false,
                _ => return Err(format!("viewer.find.set: {v} is not true or false")),
            };
            let f = app.viewer.find.get_or_insert_with(Find::default);
            match k {
                "case" => f.case = on,
                "whole" => f.whole = on,
                _ => return Err(format!("viewer.find.set: no option {k} (case, whole)")),
            }
            refind(app);
        }
        "viewer.find.show" => {
            let i = params
                .index
                .or_else(|| params.name.as_deref().and_then(|n| n.trim().parse().ok()))
                .ok_or("viewer.find.show: index is required")?;
            let h = app.viewer.find.as_ref().and_then(|f| f.hits.get(i)).cloned().ok_or_else(|| format!("viewer.find.show: no hit {i}"))?;
            focus(app, h.space, h.x, h.y);
            app.message = h.s;
        }
        "viewer.find.close" => {
            app.viewer.find = None;
            app.viewer.mark = None;
        }
        "viewer.add" => {
            let kind: &'static str = match params.name.as_deref() {
                Some("drawing") => "drawing",
                Some("pictures") | None => "pictures",
                Some(o) => return Err(format!("viewer.add: {o} is not drawing or pictures")),
            };
            if app.doc.is_none() {
                return Err("viewer.add: open a drawing first".into());
            }
            let Some(pick) = app.services.pick_files.as_mut() else { return Err("viewer.add: no file picker here".into()) };
            match pick(kind, true) {
                Some(files) => add_files(app, files),
                None => app.viewer.pick = Some(kind),
            }
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

fn refind(app: &mut UiApp) {
    let Some(d) = app.doc.as_ref() else { return };
    let Some(f) = app.viewer.find.as_mut() else { return };
    f.hits = search(&d.drawing, f);
    f.ran = true;
    app.viewer.mark = None;
    if !f.query.is_empty() {
        app.message = lang::t(&CADCRAFT, "dlg.find.results", &[("n", f.hits.len().to_string())]);
    }
}

/// dwg-view.js `focusModelPoint` for a point in a space: zoomed in a little and ringed.
pub fn focus(app: &mut UiApp, space: usize, x: f64, y: f64) {
    let Some(d) = app.doc.as_ref() else { return };
    let Some(sp) = d.drawing.spaces.get(space) else { return };
    let paper = sp.kind == cc_drawing::SpaceKind::Paper;
    let b = if paper { sp.paper.unwrap_or(sp.bounds) } else { sp.bounds };
    let span = (b.max_x - b.min_x).max(b.max_y - b.min_y) / if paper { 8.0 } else { 30.0 };
    app.space = space;
    let (w, h) = app.canvas_px;
    let mut v = app.views.get(space).copied().flatten().unwrap_or_default();
    v.width = w;
    v.height = h;
    v.zoom = w.min(h) / if span.is_finite() && span > 0.0 { span } else { 1.0 };
    v.cx = x;
    v.cy = y;
    if let Some(slot) = app.views.get_mut(space) {
        *slot = Some(v);
    }
    app.pending_fit = false;
    app.viewer.mark = Some((space, x, y));
}

/// Files the person added for the open drawing: pictures go to the picture store, drawings become its linked
/// drawings (the drawing is read again with them).
pub fn add_files(app: &mut UiApp, files: Vec<ImportedFile>) {
    let mut relink = false;
    for f in files {
        let lower = f.name.to_lowercase();
        if lower.ends_with(".dwg") || lower.ends_with(".dxf") {
            let stem = cc_cad::pictures::base_name(&f.name);
            let stem = stem.rsplit_once('.').map_or(stem, |(s, _)| s).to_lowercase();
            app.viewer.added.insert(stem, f.bytes);
            relink = true;
        } else if let Err(e) = cc_cad::pictures::add_picture(&mut app.viewer_pictures, &f.name, &f.bytes) {
            app.message = format!("{}: {}", f.name, lang::cad(e));
        }
    }
    if relink && let Some(src) = app.source.clone() {
        let added = app.viewer.added.clone();
        let dir = src.dir.clone();
        let mut read = app.services.read_xref.take();
        let res = crate::document::open_with(&src.name, &src.bytes, &mut |x| {
            added.get(&x.to_lowercase()).cloned().or_else(|| match (read.as_mut(), dir.as_deref()) {
                (Some(r), Some(d)) => r(d, x),
                _ => None,
            })
        });
        app.services.read_xref = read;
        let (space, hidden, views) = (app.space, app.hidden.clone(), app.views.clone());
        app.loaded(res);
        if let Some(d) = app.doc.as_ref()
            && views.len() == d.drawing.spaces.len()
        {
            app.space = space;
            app.hidden = hidden;
            app.views = views;
            app.pending_fit = false;
        }
    }
}

/// A file dropped while the viewer shows a drawing: a picture or one of its missing linked drawings is added to it
/// (true), anything else opens as usual (false).
pub fn dropped(app: &mut UiApp, f: &ImportedFile) -> bool {
    if !active(app) {
        return false;
    }
    let lower = f.name.to_lowercase();
    let Some(d) = app.doc.as_ref() else { return false };
    let stem = lower.rsplit_once('.').map_or(lower.as_str(), |(s, _)| s).to_string();
    let link =
        (lower.ends_with(".dwg") || lower.ends_with(".dxf")) && d.drawing.xrefs.iter().any(|x| !x.loaded && x.name.to_lowercase() == stem);
    let picture = d.drawing.images.iter().any(|im| cc_cad::pictures::picture_key(&im.file) == cc_cad::pictures::picture_key(&f.name));
    if link || picture {
        add_files(app, vec![f.clone()]);
        return true;
    }
    false
}

/// Every frame: the web picker's files, and the viewer's keys (P print view, F fit, Ctrl+F find).
pub fn frame(app: &mut UiApp, ctx: &egui::Context) {
    if app.viewer.pick.is_some()
        && let Some(slot) = app.services.picked.as_ref()
    {
        let got = slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
        if let Some(files) = got {
            app.viewer.pick = None;
            add_files(app, files);
        }
    }
    if !active(app) {
        return;
    }
    let typing = ctx.memory(|m| m.focused().is_some());
    let (p, f, find) = ctx.input(|i| {
        (
            !i.modifiers.any() && i.key_pressed(egui::Key::P),
            !i.modifiers.any() && i.key_pressed(egui::Key::F),
            i.modifiers.command && i.key_pressed(egui::Key::F),
        )
    });
    if find {
        app.viewer.find.get_or_insert_with(Find::default);
    }
    if typing {
        return;
    }
    if p {
        app.run_ui("viewer.print", &Params::default());
    }
    if f {
        app.run_ui("view.fit", &Params::default());
    }
}

/// Touch on the canvas (dwg-view.js 0.4 S.8): two fingers pinch-zoom about their middle and pan with it (one finger
/// pans as a drag does; a double tap fits as a double click does). Returns true while two fingers are down.
pub fn touch(ui: &egui::Ui, rect: egui::Rect, view: &mut cc_render::View2D, ppp: f64) -> bool {
    let Some(mt) = ui.input(|i| i.multi_touch()) else { return false };
    let c = mt.center_pos;
    if !rect.contains(c) {
        return false;
    }
    let sx = f64::from(c.x - rect.min.x) * ppp;
    let sy = f64::from(c.y - rect.min.y) * ppp;
    let z = f64::from(mt.zoom_delta);
    if z.is_finite() && z > 0.0 && (z - 1.0).abs() > 1e-6 {
        view.zoom_at(sx, sy, z);
    }
    let d = mt.translation_delta;
    if d.x != 0.0 || d.y != 0.0 {
        view.pan(f64::from(d.x) * ppp, f64::from(d.y) * ppp);
    }
    true
}

/// The Drawing panel (right): bimcraft.js's info list and props (hint, linked drawings, pictures, not drawn).
pub fn info_panel(app: &mut UiApp, ui: &mut egui::Ui) {
    if !app.viewer.info || !active(app) {
        return;
    }
    let t = app.tokens;
    let Some(d) = app.doc.as_ref() else { return };
    let dr = &d.drawing;
    let w = |k: &str| lang::bim(k);
    let sheets = dr.spaces.iter().filter(|s| s.kind == cc_drawing::SpaceKind::Paper).count();
    let format = if dr.release.is_empty() {
        dr.version.clone()
    } else {
        lang::t(BIMCRAFT, "dwg.format.release", &[("release", dr.release.clone()), ("tag", dr.version.clone())])
    };
    let mut rows: Vec<(String, String)> = vec![
        (w("info.file"), d.name.clone()),
        (w("info.size"), bytes(d.bytes)),
        (w("info.format"), format),
        (w("info.units"), dr.units.clone()),
        (w("info.entities"), num(dr.stats.entities)),
        (w("info.layers"), num(dr.layers.len() as u64)),
        (w("info.sheets"), if sheets > 0 { num(sheets as u64) } else { w("info.sheets.none") }),
    ];
    if !dr.xrefs.is_empty() {
        let loaded = dr.xrefs.iter().filter(|x| x.loaded).count();
        rows.push((
            w("info.linked"),
            lang::t(BIMCRAFT, "dwg.linked.loaded", &[("loaded", loaded.to_string()), ("total", dr.xrefs.len().to_string())]),
        ));
    }
    rows.retain(|(_, v)| !v.is_empty());
    let pictures: Vec<(String, Option<&'static str>)> = dr
        .images
        .iter()
        .map(|im| {
            let key = cc_cad::pictures::picture_key(&im.file);
            let st = if app.viewer.unreadable.contains(&key) {
                Some("dwg.img.unreadable")
            } else if cc_cad::pictures::find_picture(&app.viewer_pictures, &im.file).is_some() {
                None
            } else {
                Some("dwg.img.notfound")
            };
            (if im.name.is_empty() { cc_cad::pictures::base_name(&im.file).to_string() } else { im.name.clone() }, st)
        })
        .collect();
    let xrefs: Vec<(String, bool)> = dr.xrefs.iter().map(|x| (x.name.clone(), x.loaded)).collect();
    let notdrawn: Vec<(String, u64)> = dr.stats.unsupported.iter().map(|(k, v)| (k.clone(), *v)).collect();
    let truncated = dr.stats.truncated;
    let mut act: Option<(&str, &str)> = None;
    let frame = egui::Frame::new().fill(t.panel).inner_margin(8);
    egui::Panel::right("viewer_info").default_size(270.0).size_range(200.0..=420.0).resizable(true).frame(frame).show(ui, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.label(RichText::new(w("info.h.drawing")).strong().color(t.text_strong));
            egui::Grid::new("viewer_info_grid").num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
                for (k, v) in &rows {
                    ui.label(RichText::new(k).small().color(t.text_dim));
                    ui.label(RichText::new(v).small().color(t.text));
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
            ui.label(RichText::new(w("dwg.hint")).small().color(t.text_dim));
            let pill = |ui: &mut egui::Ui, ok: bool, text: String| {
                let c = if ok { egui::Color32::from_rgb(0x3c, 0xb0, 0x60) } else { egui::Color32::from_rgb(0xe0, 0x6c, 0x5c) };
                ui.label(RichText::new(text).small().color(c));
            };
            if !xrefs.is_empty() {
                ui.separator();
                ui.label(RichText::new(w("dwg.linked.title")).strong().color(t.text_strong));
                for (n, ok) in &xrefs {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(format!("{n}.dwg")).small().color(t.text));
                        pill(ui, *ok, w(if *ok { "dwg.linked.shown" } else { "dwg.linked.notfound" }));
                    });
                }
                if xrefs.iter().any(|x| !x.1) {
                    ui.label(RichText::new(w("dwg.linked.missing.file")).small().color(t.text_dim));
                    if ui.button(w("dwg.linked.add")).clicked() {
                        act = Some(("viewer.add", "drawing"));
                    }
                }
            }
            if !pictures.is_empty() {
                ui.separator();
                ui.label(RichText::new(w("dwg.images.title")).strong().color(t.text_strong));
                for (n, st) in &pictures {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(n).small().color(t.text));
                        pill(ui, st.is_none(), w(st.unwrap_or("dwg.img.shown")));
                    });
                }
                if pictures.iter().any(|p| p.1.is_some()) {
                    ui.label(RichText::new(w("dwg.img.missing.file")).small().color(t.text_dim));
                    if ui.button(w("dwg.img.add")).clicked() {
                        act = Some(("viewer.add", "pictures"));
                    }
                }
            }
            if !notdrawn.is_empty() {
                ui.separator();
                ui.label(RichText::new(w("dwg.notdrawn.title")).strong().color(t.text_strong));
                egui::Grid::new("viewer_notdrawn").num_columns(2).show(ui, |ui| {
                    for (k, n) in &notdrawn {
                        ui.label(RichText::new(k).small().color(t.text));
                        ui.label(RichText::new(num(*n)).small().color(t.text_dim));
                        ui.end_row();
                    }
                });
                ui.label(RichText::new(w("dwg.notdrawn.note")).small().color(t.text_dim));
            }
            if truncated {
                ui.label(RichText::new(w("dwg.truncated")).small().color(egui::Color32::from_rgb(0xe0, 0x6c, 0x5c)));
            }
        });
    });
    if let Some((id, name)) = act
        && let Err(e) = app.run(id, &Params { name: Some(name.into()), ..Default::default() })
    {
        app.error = Some(e);
    }
}

/// The Find panel (left of the drawing, under the layers): the query, match case, whole words, the hits.
pub fn find_panel(app: &mut UiApp, ui: &mut egui::Ui) {
    if !active(app) {
        return;
    }
    let Some(f) = app.viewer.find.clone() else { return };
    let t = app.tokens;
    let c = |k: &str| lang::cad(k);
    let mut q = f.query.clone();
    let mut act: Option<(String, Params)> = None;
    let frame = egui::Frame::new().fill(t.panel).inner_margin(8);
    egui::Panel::right("viewer_find").default_size(260.0).size_range(200.0..=420.0).resizable(true).frame(frame).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(c("dlg.find.what")).strong().color(t.text_strong));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("×").on_hover_text(c("dlg.find.close")).clicked() {
                    act = Some(("viewer.find.close".into(), Params::default()));
                }
            });
        });
        let id = ui.make_persistent_id("viewer.find.q");
        let r = ui.add(egui::TextEdit::singleline(&mut q).id(id).desired_width(f32::INFINITY));
        if !f.ran && q.is_empty() {
            r.request_focus();
        }
        if r.changed() {
            act = Some(("viewer.find".into(), Params { name: Some(q.clone()), ..Default::default() }));
        }
        let (mut case, mut whole) = (f.case, f.whole);
        if ui.checkbox(&mut case, c("dlg.find.case")).changed() {
            act = Some(("viewer.find.set".into(), Params { name: Some(format!("case={case}")), ..Default::default() }));
        }
        if ui.checkbox(&mut whole, c("dlg.find.whole")).changed() {
            act = Some(("viewer.find.set".into(), Params { name: Some(format!("whole={whole}")), ..Default::default() }));
        }
        if f.ran && !f.query.is_empty() {
            ui.label(RichText::new(lang::t(&CADCRAFT, "dlg.find.results", &[("n", f.hits.len().to_string())])).small().color(t.text_dim));
        }
        ui.separator();
        let names: Vec<String> = app
            .doc
            .as_ref()
            .map(|d| {
                d.drawing
                    .spaces
                    .iter()
                    .map(|s| if s.kind == cc_drawing::SpaceKind::Model { lang::cad("app.tab.model") } else { s.name.clone() })
                    .collect()
            })
            .unwrap_or_default();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (i, h) in f.hits.iter().enumerate() {
                let on = app.viewer.mark.is_some_and(|(s, x, y)| s == h.space && x == h.x && y == h.y);
                let label = format!("{}  ·  {}", h.s.chars().take(80).collect::<String>(), names.get(h.space).cloned().unwrap_or_default());
                if ui.selectable_label(on, RichText::new(label).small()).clicked() {
                    act = Some(("viewer.find.show".into(), Params { index: Some(i), ..Default::default() }));
                }
            }
        });
    });
    if let Some((id, p)) = act
        && let Err(e) = app.run(&id, &p)
    {
        app.error = Some(e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_rules() {
        assert!(matches("Walk-in COOLER", "cooler", false, false));
        assert!(!matches("Walk-in COOLER", "cooler", true, false));
        assert!(matches("K-101 sink", "sink", false, true));
        assert!(!matches("sinks", "sink", false, true));
        assert!(matches("sinks and sink", "sink", false, true));
        assert!(!matches("anything", "", false, false));
        assert!(matches("ÉTAGE 2", "étage", false, true));
        assert_eq!(num(1_234_567), "1,234,567");
        assert_eq!(num(12), "12");
        assert_eq!(bytes(1536), "2 KB");
        assert_eq!(bytes(5_000_000), "4.8 MB");
    }

    #[test]
    fn viewer_commands_on_the_sample() {
        cc_lang::with(cc_lang::Locale::En, || {
            let mut app = UiApp::default();
            app.run("file.sample", &Params::default()).unwrap();
            app.canvas_px = (1600.0, 1000.0);
            app.run("viewer.print", &Params::default()).unwrap();
            assert!(app.viewer.print);
            app.run("viewer.find", &Params { name: Some("K-".into()), ..Default::default() }).unwrap();
            let n = app.viewer.find.as_ref().map_or(0, |f| f.hits.len());
            assert!(n > 0, "the sample's texts hold K-");
            app.run("viewer.find.show", &Params { index: Some(0), ..Default::default() }).unwrap();
            assert!(app.viewer.mark.is_some());
            assert!(app.run("viewer.find.show", &Params { index: Some(99_999), ..Default::default() }).is_err());
            assert!(app.run("viewer.find.set", &Params { name: Some("case=maybe".into()), ..Default::default() }).is_err());
            app.run("viewer.find.close", &Params::default()).unwrap();
            assert!(app.viewer.find.is_none() && app.viewer.mark.is_none());
            // a picture added by hand is used (the sample names its logo); a stranger file is refused with a word
            app.services.pick_files =
                Some(Box::new(|_, _| Some(vec![ImportedFile { name: "notes.txt".into(), bytes: b"hello".to_vec(), dir: None }])));
            app.run("viewer.add", &Params { name: Some("pictures".into()), ..Default::default() }).unwrap();
            assert!(app.viewer_pictures.is_empty());
        });
    }
}
