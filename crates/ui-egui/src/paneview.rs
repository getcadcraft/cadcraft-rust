//! The split screen on the window (cadcraft.js's pane handling over panes.rs): SPLIT / VPORTS in the model ask for a
//! layout, LINKVIEWS links the panes' views, a press or the wheel in another pane makes it the active one (the first
//! click only does that, as AutoCAD), the dividers drag (a double click puts one back in the middle). The active pane
//! is the window's live view (`UiApp::views[space]`): the commands, picks, snaps and zooms work there and it gets the
//! crosshair; the others are drawn from their records, with the selection and the rubber band where they show the
//! same space. The panes go into the saved file (its VPORT table) and come back when it opens for editing.
//!
//! Colours: the theme's (desk between the panes, the accent round the active one, dividers round the others).

use cc_render::View2D;
use egui::Rect;
use serde_json::Value;

use crate::panes::{self, Live, Split, Vs};
use crate::{Params, UiApp, canvas, cloudview};

/// The window's split screen.
#[derive(Default)]
pub struct PaneUi {
    pub split: Split,
    /// the panes a file was saved with, restored once the drawing has a view (after its first fit)
    pub pending: Option<Value>,
    /// physical pixels per point of the last frame
    pub ppp: f64,
    /// the primary press that made a pane active: it only does that (cadcraft.js `down = null`) until released
    swallow: bool,
}

/// UI commands (the control channel, scripts): what the window's own controls do.
pub const COMMANDS: &[(&str, &str, &str, &str)] = &[
    (
        "panes.activate",
        "Make a pane active",
        "",
        "{index (or name): integer} → that pane gets the crosshair (a click into it does the same)",
    ),
    (
        "panes.divider",
        "Move a divider",
        "",
        "{name: \"fx=<0–1>\" | \"fy=<0–1>\" (or name \"fx\" | \"fy\" with factor)} → where the vertical / horizontal divider sits",
    ),
];

fn ppp(app: &UiApp) -> f64 {
    if app.panes.ppp > 0.0 { app.panes.ppp } else { 1.0 }
}

/// The window's view as view.js keeps it (CSS px).
fn vs_of(v: &View2D, ppp: f64) -> Vs {
    Vs { cx: v.cx, cy: v.cy, zoom: v.zoom / ppp, tw: -v.twist }
}
fn view_of(vs: &Vs, r: &panes::Rect, ppp: f64) -> View2D {
    View2D { cx: vs.cx, cy: vs.cy, zoom: vs.zoom * ppp, twist: -vs.tw, width: r.w * ppp, height: r.h * ppp }
}

/// The live view (the active pane): the open space, its view, its rectangle.
fn live(app: &UiApp) -> Live {
    let p = ppp(app);
    let s = &app.panes.split;
    let rect =
        s.panes.as_ref().and_then(|q| q.list.get(q.active)).and_then(|q| q.rect).unwrap_or(panes::Rect { x: 0.0, y: 0.0, w: s.w, h: s.h });
    let vs = app.views.get(app.space).copied().flatten().map(|v| vs_of(&v, p)).unwrap_or_default();
    Live { space: app.space, vs, rect }
}

/// Run `f` on the split screen and the live view, then show what it left: the active pane's space opens and its view
/// becomes the window's.
fn with_live(app: &mut UiApp, f: impl FnOnce(&mut Split, &mut Live)) {
    let from = app.space;
    let had_view = app.views.get(from).is_some_and(Option::is_some);
    let mut l = live(app);
    f(&mut app.panes.split, &mut l);
    if l.space != from {
        app.run_ui("space.select", &Params { index: Some(l.space), ..Default::default() });
    }
    let v = view_of(&l.vs, &l.rect, ppp(app));
    if (had_view || l.space != from)
        && let Some(slot) = app.views.get_mut(l.space)
    {
        *slot = Some(v);
    }
}

/// The number of sheets of the drawing being edited.
fn sheets(app: &UiApp) -> usize {
    app.editor.as_ref().map_or(0, |ed| ed.eng.doc().layouts.len())
}

/// After each input to the editor: SPLIT / VPORTS's layout, LINKVIEWS.
pub fn requests(app: &mut UiApp) {
    let Some(ed) = app.editor.as_ref() else {
        if app.panes.split.panes.is_some() || app.panes.pending.is_some() {
            app.panes = PaneUi { ppp: app.panes.ppp, ..PaneUi::default() };
        }
        return;
    };
    let (req, linked) = {
        let mut h = ed.hs.borrow_mut();
        (h.panes_req.take(), h.linked)
    };
    app.panes.split.linked = linked;
    if let Some(layout) = req {
        // cadcraft.js host.setPanes: the 3D tab closes
        if crate::cad3dtab::showing(app) {
            app.run_ui("tab3d.close", &Params::default());
        }
        with_live(app, |s, l| s.set_layout(&layout, l));
    }
}

/// The panes on screen now (for the control channel and tests): the state saved into the file.
pub fn state(app: &mut UiApp) -> Value {
    let l = live(app);
    let names: Vec<String> =
        app.editor.as_ref().map(|ed| ed.eng.doc().layouts.iter().map(|l| l.name.clone()).collect()).unwrap_or_default();
    let entered = entered_view(app);
    app.panes.split.state(&l, &|i| names.get(i.wrapping_sub(1)).cloned(), entered)
}

/// MSPACE: the active pane works in a sheet's viewport — that sheet (its tab) and its view.
fn entered_view(app: &UiApp) -> Option<(usize, Vs)> {
    let ed = app.editor.as_ref()?;
    let (space, _) = ed.hs.borrow().entered?;
    let i = ed.eng.doc().layouts.iter().position(|l| l.space == space)?.saturating_add(1);
    let v = app.views.get(i).copied().flatten()?;
    Some((i, vs_of(&v, ppp(app))))
}

/// cadcraft.js's save: the panes on screen go into the file (its VPORT table); one pane: the model view's turn.
pub fn to_doc(app: &mut UiApp) {
    let st = state(app);
    // one pane: the model view's turn (the live one in the model, the one kept for it elsewhere)
    let tw = if app.panes.split.panes.is_some() { 0.0 } else { app.views.first().copied().flatten().map_or(0.0, |v| -v.twist) };
    if let Some(ed) = app.editor.as_ref() {
        let mut d = ed.eng.doc_mut();
        d.vports = (!st.is_null()).then_some(st);
        d.view_twist = if tw == 0.0 { 0.0 } else { tw };
    }
}

/// A drawing opened for editing: the split screen it was saved with comes back once it has a view; one pane saved
/// turned turns the model view (cadcraft.js's open).
pub fn from_doc(app: &mut UiApp) {
    app.panes.split.panes = None;
    app.panes.pending = app.editor.as_ref().map(|ed| ed.eng.doc().vports.clone().unwrap_or(Value::Null));
}

/// UI commands of the panes.
pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    if app.panes.split.panes.is_none() {
        return Err(format!("{id}: the drawing area is not split (SPLIT or VPORTS in the model)"));
    }
    match id {
        "panes.activate" => {
            let i = params
                .index
                .or_else(|| params.name.as_deref().and_then(|n| n.trim().parse().ok()))
                .ok_or("panes.activate: index is required")?;
            let n = app.panes.split.panes.as_ref().map_or(0, |p| p.list.len());
            if i >= n {
                return Err(format!("panes.activate: index {i} is out of range (0..{n})"));
            }
            activate(app, i);
        }
        "panes.divider" => {
            let name = params.name.clone().unwrap_or_default();
            let (key, f) = match name.split_once('=') {
                Some((k, v)) => (k.trim().to_string(), v.trim().parse::<f64>().ok()),
                None => (name, params.factor),
            };
            let f = f.ok_or("panes.divider: a number 0–1 is required")?;
            if key != "fx" && key != "fy" {
                return Err("panes.divider: name is \"fx\" or \"fy\"".into());
            }
            with_live(app, |s, l| s.set_split(&key, f, l));
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

fn activate(app: &mut UiApp, i: usize) {
    let n = sheets(app);
    with_live(app, |s, l| {
        s.activate(i, n, l);
    });
}

fn abs(full: Rect, r: &panes::Rect) -> Rect {
    Rect::from_min_size(full.min + egui::vec2(r.x as f32, r.y as f32), egui::vec2(r.w as f32, r.h as f32))
}

/// Before the canvas draws: the area's size, a file's panes, the dividers and the other panes' presses and wheels.
/// → the active pane's rectangle (the whole area without panes).
pub fn begin(app: &mut UiApp, ui: &egui::Ui, full: Rect) -> Rect {
    if app.editor.is_none() {
        return full;
    }
    app.panes.ppp = f64::from(ui.ctx().pixels_per_point());
    let (w, h) = (f64::from(full.width()), f64::from(full.height()));
    if app.panes.split.w != w || app.panes.split.h != h {
        with_live(app, |s, l| {
            s.sync(l);
            s.w = w;
            s.h = h;
            s.layout(l);
        });
    }
    if app.panes.pending.is_some() && app.views.get(app.space).is_some_and(Option::is_some) {
        let v = app.panes.pending.take().unwrap_or(Value::Null);
        let names: Vec<String> =
            app.editor.as_ref().map(|ed| ed.eng.doc().layouts.iter().map(|l| l.name.clone()).collect()).unwrap_or_default();
        let c = panes::clean_panes(&v);
        with_live(app, |s, l| {
            s.restore(c.as_ref(), &|n: &str| names.iter().position(|x| x == n).map(|i| i.saturating_add(1)), l);
        });
    }
    let Some(p) = app.panes.split.panes.clone() else {
        cloudview::keep_slots(1);
        return full;
    };
    ui.painter().rect_filled(full, 0.0, app.tokens.desk);
    // the dividers: drag one, or double-click it back to the middle
    for d in panes::dividers(p.layout, p.fx, p.fy, w, h) {
        let g = panes::GAP as f32;
        let r = if d.vertical {
            Rect::from_min_size(full.min + egui::vec2(d.x as f32 - g, d.y as f32), egui::vec2(2.0 * g, d.len as f32))
        } else {
            Rect::from_min_size(full.min + egui::vec2(d.x as f32, d.y as f32 - g), egui::vec2(d.len as f32, 2.0 * g))
        };
        let resp = ui.interact(r, egui::Id::unique(("cc-pane-divider", d.key)), egui::Sense::click_and_drag());
        let resp = resp.on_hover_cursor(if d.vertical { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical });
        let key = d.key;
        if resp.double_clicked() {
            with_live(app, |s, l| s.set_split(key, 0.5, l));
        } else if resp.dragged()
            && let Some(q) = resp.interact_pointer_pos()
        {
            let f = if d.vertical { f64::from(q.x - full.min.x) / w } else { f64::from(q.y - full.min.y) / h };
            with_live(app, |s, l| s.set_split(key, f, l));
        }
    }
    // a press (any button) or the wheel in another pane makes it the active one
    let (scrolled, mid_or_right) =
        ui.input(|i| (i.smooth_scroll_delta.y != 0.0 || i.zoom_delta() != 1.0, i.pointer.middle_down() || i.pointer.secondary_down()));
    let l = live(app);
    let mut to = None;
    for (i, q) in app.panes.split.shown(&l).iter().enumerate() {
        let Some(r) = q.rect else { continue };
        if i == p.active {
            continue;
        }
        let resp = ui.interact(abs(full, &r), egui::Id::unique(("cc-pane-in", i)), egui::Sense::click_and_drag());
        if resp.is_pointer_button_down_on() || (resp.hovered() && scrolled) {
            to = Some(i);
        }
    }
    if let Some(i) = to {
        if ui.input(|i| i.pointer.primary_down()) {
            app.panes.swallow = true;
        }
        activate(app, i);
    }
    // the press that made a pane active goes on panning it with the middle or right button
    let active = app.panes.split.panes.as_ref().map_or(0, |p| p.active);
    if mid_or_right && ui.ctx().dragged_id() == Some(egui::Id::unique(("cc-pane-in", active))) {
        let d = ui.input(|i| i.pointer.delta());
        if d != egui::Vec2::ZERO {
            with_live(app, |_, l| l.vs.pan_by(f64::from(d.x), f64::from(d.y)));
        }
    }
    let l = live(app);
    app.panes.split.follow_linked(&l);
    abs(full, &l.rect)
}

/// After the active pane: the other panes (each its own view and space; the selection and the rubber band where they
/// show the open space) and a frame round each pane, the active one in the accent.
pub fn end(app: &mut UiApp, ui: &egui::Ui, full: Rect) {
    if app.editor.is_none() || app.panes.split.panes.is_none() {
        return;
    }
    let p = ppp(app);
    let l = live(app);
    let shown = app.panes.split.shown(&l);
    let active = app.panes.split.panes.as_ref().map_or(0, |q| q.active);
    let pictures = app.picture_textures(ui.ctx());
    let mut k = 0u32;
    for (i, q) in shown.iter().enumerate() {
        let Some(r) = q.rect else { continue };
        if i == active {
            continue;
        }
        k = k.saturating_add(1);
        let rect = abs(full, &r);
        let view = view_of(&q.vs, &r, p);
        let (Some(doc), Some(ed)) = (app.doc.as_ref(), app.editor.as_ref()) else { return };
        if doc.drawing.spaces.get(q.space).is_none() {
            continue;
        }
        let underlay = cloudview::underlay_slot(ui.ctx(), ed, rect, p, &doc.drawing, q.space, &view, k as usize);
        let frame = canvas::Frame {
            underlay: &underlay,
            pictures: &pictures,
            drawing: &doc.drawing,
            geometry: &doc.geometry,
            id: doc.id,
            space: q.space,
            view,
            hidden: &app.hidden,
            tokens: &app.tokens,
            line_px: p as f32,
            print: false,
            lw_display: ed.hs.borrow().lwt,
            layer_handles: Some(&doc.layer_handles),
            mark: None,
            slot_base: k.saturating_mul(64),
            grid: app.look.grid,
        };
        canvas::paint(ui, rect, &frame);
        if q.space == app.space {
            app.edit_overlays(ui, rect, &view, p, false);
        }
        corner_icon(app, ui, rect, &view, q.space);
    }
    cloudview::keep_slots(k as usize + 1);
    let t = app.tokens;
    let painter = ui.painter_at(full);
    for (i, q) in shown.iter().enumerate() {
        let Some(r) = q.rect else { continue };
        let on = i == active;
        // 0.7: the active pane's 2 px action edge, the others' 1 px line
        let stroke = if on { egui::Stroke::new(2.0, t.accent) } else { egui::Stroke::new(1.0, t.line) };
        painter.rect_stroke(abs(full, &r), 0.0, stroke, egui::StrokeKind::Inside);
    }
}

/// Settings › CadCraft › Corner indicator: the axis icon at the bottom left of a pane showing the model (automatic or
/// axis icon), turned with the pane's view and the UCS (view.js `drawUcs`).
pub fn corner_icon(app: &UiApp, ui: &egui::Ui, rect: Rect, view: &View2D, space: usize) {
    if space != 0 || !app.cadset.axis_icon_2d() {
        return;
    }
    let Some(ed) = app.editor.as_ref() else { return };
    if ed.hs.borrow().entered.is_some() {
        return;
    }
    let u = cc_engine::ucs::active_ucs(&ed.eng.doc());
    let ang = u.map_or(0.0, |u| u.ang);
    crate::cadset::axis_icon(&ui.painter_at(rect), rect, ang + view.twist, u.is_none(), &app.tokens);
}

/// Is the press that made a pane active still going (the canvas leaves it alone: no pick, no window)? Ends with its
/// release.
pub fn swallowing(app: &mut UiApp, ui: &egui::Ui) -> bool {
    if !app.panes.swallow {
        return false;
    }
    if !ui.input(|i| i.pointer.any_down()) {
        app.panes.swallow = false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// M4.57: SPLIT / VPORTS / LINKVIEWS on the window; the panes into the saved file and back when it opens.
    #[test]
    fn split_screen_saved_and_opened() {
        cc_lang::with(cc_lang::Locale::En, || {
            let mut app = UiApp::default();
            app.run("file.new", &Params::default()).unwrap();
            let run = |app: &mut UiApp, id: &str, name: &str| app.run(id, &Params { name: Some(name.into()), ..Default::default() });
            for t in ["LINE", "0,0", "100,50", ""] {
                run(&mut app, "cmd.line", t).unwrap();
            }
            // the window's drawing area and a view (as the first frame leaves them)
            app.panes.ppp = 1.0;
            app.panes.split.w = 800.0;
            app.panes.split.h = 600.0;
            app.views[0] = Some(View2D { cx: 50.0, cy: 25.0, zoom: 4.0, twist: 0.0, width: 800.0, height: 600.0 });
            assert!(run(&mut app, "panes.activate", "1").is_err(), "not split");
            for t in ["SPLIT", "2", "V"] {
                run(&mut app, "cmd.line", t).unwrap();
            }
            assert_eq!(app.panes.split.panes.as_ref().map(|p| p.list.len()), Some(2));
            assert_eq!(app.views[0].map(|v| v.width), Some(398.0));
            run(&mut app, "panes.activate", "1").unwrap();
            assert!(run(&mut app, "panes.activate", "5").is_err());
            run(&mut app, "panes.divider", "fx=0.25").unwrap();
            assert!(run(&mut app, "panes.divider", "fz=0.25").is_err());
            for t in ["LINKVIEWS", "ON"] {
                run(&mut app, "cmd.line", t).unwrap();
            }
            assert!(app.panes.split.linked);
            let st = state(&mut app);
            assert_eq!(st["layout"], "2v");
            assert_eq!(st["active"], 1);
            assert_eq!(st["fx"], 0.25);
            // saved: the VPORT table holds both panes, the active one first
            let got: Arc<Mutex<Vec<u8>>> = Arc::default();
            let g2 = Arc::clone(&got);
            app.services.save = Some(Box::new(move |_n, b| {
                *g2.lock().unwrap() = b;
                Ok(Some("x".into()))
            }));
            app.run("file.save", &Params { name: Some("dxf2000".into()), ..Default::default() }).unwrap();
            let text = String::from_utf8(got.lock().unwrap().clone()).unwrap();
            let back = cc_io_cad::dxf_read::read_dxf(&text).unwrap();
            assert_eq!(back["vports"]["layout"], "2v");
            // opened again: the panes come back once the drawing has a view
            app.open_bytes("p.dxf", text.as_bytes());
            app.run("file.edit", &Params::default()).unwrap();
            assert!(app.panes.split.panes.is_none() && app.panes.pending.is_some());
            app.views[0] = Some(View2D { cx: 0.0, cy: 0.0, zoom: 1.0, twist: 0.0, width: 800.0, height: 600.0 });
            let pending = app.panes.pending.take().unwrap();
            let c = panes::clean_panes(&pending);
            with_live(&mut app, |s, l| {
                s.restore(c.as_ref(), &|_| None, l);
            });
            assert_eq!(state(&mut app)["active"], 1);
            // SPLIT SI ends it; a new drawing has one pane
            for t in ["SPLIT", "SI"] {
                run(&mut app, "cmd.line", t).unwrap();
            }
            assert!(app.panes.split.panes.is_none());
            assert_eq!(state(&mut app), Value::Null);
        });
    }
}
