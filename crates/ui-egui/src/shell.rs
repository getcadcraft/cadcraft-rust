//! The 0.7 app shell's pieces, drawn the way CadCraft 0.7's `app-shell.css` / `cadcraft.css` draw them (measured in
//! docs/ui-notes.md): the app bar's mark, app swatch and name, its bordered buttons and the Paper | Model switch; the
//! panel grip (the dock header), section heads and link buttons; the status bar's toggles; the outlined action
//! button. Every colour comes from the theme tokens; every size is 0.7's in CSS pixels (egui points at 100 %).

use egui::{Align2, Color32, Pos2, Rect, Response, Sense, Stroke, Vec2};

use crate::theme::{Tokens, bold, head, semi};

/// The app bar's height (0.7 `.app-bar`: 48 px with its 1 px rule).
pub const APP_BAR_H: f32 = 48.0;
/// A panel grip's height (0.7 `.shell-grip`: 28 px).
pub const GRIP_H: f32 = 28.0;
/// The status bar's height (0.7 `.cc-status`: 32 px).
pub const STATUS_H: f32 = 32.0;

/// CadCraft's mark: a crosshair with a pick box at its centre (the site's and the apps' logo, drawn on a 20 px grid).
pub fn mark(p: &egui::Painter, r: Rect, c: Color32) {
    let s = r.width() / 20.0;
    let at = |x: f32, y: f32| Pos2::new(r.left() + x * s, r.top() + y * s);
    let w = Stroke::new((1.6 * s).max(1.0), c);
    p.line_segment([at(0.5, 10.0), at(6.0, 10.0)], w);
    p.line_segment([at(14.0, 10.0), at(19.5, 10.0)], w);
    p.line_segment([at(10.0, 0.5), at(10.0, 6.0)], w);
    p.line_segment([at(10.0, 14.0), at(10.0, 19.5)], w);
    p.rect_stroke(Rect::from_min_max(at(6.0, 6.0), at(14.0, 14.0)), 0.0, w, egui::StrokeKind::Middle);
}

/// The app's swatch: its layer colour in a square with a 1 px ink edge (0.7 `.layer-swatch`).
pub fn swatch(ui: &mut egui::Ui, t: &Tokens, size: f32, c: Color32) -> Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    ui.painter().rect(r, 1.0, c, Stroke::new(1.0, t.rule), egui::StrokeKind::Inside);
    resp
}

/// A word in a font, colour (one galley).
fn galley(ui: &egui::Ui, text: &str, font: egui::FontId, c: Color32) -> std::sync::Arc<egui::Galley> {
    ui.painter().layout_no_wrap(text.to_string(), font, c)
}

/// The app's name with its small raised ™ (0.7 `.app-bar-home`: 17 px bold). Clicking it goes to the start screen.
pub fn app_name(ui: &mut egui::Ui, t: &Tokens, name: &str, tm: bool) -> Response {
    let g = galley(ui, name, bold(17.0), t.text_strong);
    let tmg = galley(ui, "™", semi(8.5), t.text_strong);
    let w = g.size().x + if tm { tmg.size().x + 1.0 } else { 0.0 };
    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 28.0), Sense::click());
    let y = r.center().y - g.size().y / 2.0;
    let gw = g.size().x;
    ui.painter().galley(Pos2::new(r.left(), y), g, t.text_strong);
    if tm {
        ui.painter().galley(Pos2::new(r.left() + gw + 1.0, y + 1.0), tmg, t.text_strong);
    }
    if resp.hovered() {
        ui.painter().line_segment(
            [Pos2::new(r.left(), r.bottom() - 4.0), Pos2::new(r.left() + gw, r.bottom() - 4.0)],
            Stroke::new(1.0, t.text_strong),
        );
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A chip: the app's colour as text and a 1 px border (0.7 `.app-bar-area`, 28 px, 15 px semibold).
pub fn chip(ui: &mut egui::Ui, text: &str, c: Color32) -> Response {
    let g = galley(ui, text, semi(15.0), c);
    let (r, resp) = ui.allocate_exact_size(Vec2::new(g.size().x + 18.0, 28.0), Sense::hover());
    ui.painter().rect_stroke(r, 2.0, Stroke::new(1.0, c), egui::StrokeKind::Inside);
    ui.painter().galley(Pos2::new(r.left() + 9.0, r.center().y - g.size().y / 2.0), g, c);
    resp
}

/// An app-bar button: a 1 px ink border, an icon then a 15 px semibold label, 32 px tall (0.7 `.app-bar-btn`).
/// `on` keeps it pressed (a panel it opens is open).
pub fn bar_button(ui: &mut egui::Ui, t: &Tokens, icon: &str, label: &str, on: bool) -> Response {
    let g = galley(ui, label, semi(15.0), t.text_strong);
    let iw = if icon.is_empty() { 0.0 } else { 21.0 };
    let (r, resp) = ui.allocate_exact_size(Vec2::new(g.size().x + iw + 22.0, 32.0), Sense::click());
    let hov = resp.hovered();
    let fill = if on {
        t.row_selected
    } else if hov {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect(r, 2.0, fill, Stroke::new(1.0, t.rule), egui::StrokeKind::Inside);
    let mut x = r.left() + 11.0;
    if !icon.is_empty() {
        crate::ribbon::paint_icon(
            ui.painter(),
            Rect::from_min_size(Pos2::new(x, r.center().y - 7.5), Vec2::splat(15.0)),
            icon,
            t.text_strong,
        );
        x += iw;
    }
    ui.painter().galley(Pos2::new(x, r.center().y - g.size().y / 2.0), g, t.text_strong);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A plain word in the app bar (0.7 `.app-bar-links a`: 15 px, underlined on hover); `enabled` greys it out.
pub fn bar_link(ui: &mut egui::Ui, t: &Tokens, label: &str, enabled: bool) -> Response {
    let c = if enabled { t.text_dim } else { t.text_disabled };
    let g = galley(ui, label, semi(15.0), c);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (r, resp) = ui.allocate_exact_size(Vec2::new(g.size().x + 4.0, 28.0), sense);
    let base = r.center().y + g.size().y / 2.0 - 3.0;
    let gw = g.size().x;
    ui.painter().galley(Pos2::new(r.left() + 2.0, r.center().y - g.size().y / 2.0), g, c);
    if enabled && resp.hovered() {
        ui.painter().line_segment([Pos2::new(r.left() + 2.0, base + 2.0), Pos2::new(r.left() + 2.0 + gw, base + 2.0)], Stroke::new(1.0, c));
    }
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

/// A word in the app bar that opens a menu (the look of [`bar_link`], with a small ▾).
pub fn bar_menu<R>(ui: &mut egui::Ui, t: &Tokens, label: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> egui::Response {
    let text = egui::RichText::new(format!("{label} ▾")).font(semi(15.0)).color(t.text_dim);
    let b = egui::Button::new(text).frame(false).min_size(Vec2::new(0.0, 28.0));
    let (r, _) = egui::containers::menu::MenuButton::from_button(b).ui(ui, add);
    r.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The Paper | Model switch (0.7 `.space-toggle`: two 11 px semibold capitals in a 1 px ink box, the chosen one
/// filled with ink). Returns the side clicked when it is not the current one.
pub fn space_toggle(ui: &mut egui::Ui, t: &Tokens, paper: &str, model: &str, dark: bool) -> Option<bool> {
    let font = semi(11.0);
    let gp = galley(ui, &paper.to_uppercase(), font.clone(), t.text);
    let gm = galley(ui, &model.to_uppercase(), font, t.text);
    let (wp, wm) = (gp.size().x + 22.0, gm.size().x + 22.0);
    let (r, _) = ui.allocate_exact_size(Vec2::new(wp + wm + 2.0, 23.0), Sense::hover());
    let rp = Rect::from_min_size(r.min + Vec2::new(1.0, 1.0), Vec2::new(wp, 21.0));
    let rm = Rect::from_min_size(Pos2::new(rp.right(), rp.top()), Vec2::new(wm, 21.0));
    ui.painter().rect_stroke(r, 2.0, Stroke::new(1.0, t.rule), egui::StrokeKind::Inside);
    let mut out = None;
    for (rr, g, is_dark, id) in [(rp, gp, false, "space.paper"), (rm, gm, true, "space.model")] {
        let resp = ui.interact(rr, ui.make_persistent_id(id), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let on = dark == is_dark;
        let (fill, ink) = if on {
            (t.rule, t.panel_darker)
        } else if resp.hovered() {
            (t.hover, t.text)
        } else {
            (Color32::TRANSPARENT, t.text)
        };
        ui.painter().rect_filled(rr, 0.0, fill);
        // the galley was laid out in the text colour: paint it in the side's own
        ui.painter().galley_with_override_text_color(Pos2::new(rr.center().x - g.size().x / 2.0, rr.center().y - g.size().y / 2.0), g, ink);
        if resp.clicked() && !on {
            out = Some(is_dark);
        }
    }
    out
}

/// What a panel grip's buttons asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GripAct {
    None,
    Menu,
    Pin,
    Close,
}

/// A dock panel's grip (0.7 `.shell-grip`): the drag handle in the app's colour, the panel's name (12 px semibold,
/// ink-2), and ⋯ / pin / ✕ at the right; a 1 px line under it. `pin` / `close` = those buttons are shown.
pub fn grip(ui: &mut egui::Ui, t: &Tokens, app: Color32, name: &str, pin: bool, close: bool) -> GripAct {
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(Vec2::new(w, GRIP_H), Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, 0.0, t.panel);
    p.line_segment([Pos2::new(r.left(), r.bottom() - 0.5), Pos2::new(r.right(), r.bottom() - 0.5)], Stroke::new(1.0, t.line));
    // the handle: two columns of dots
    for i in 0..4 {
        for j in 0..2 {
            p.circle_filled(Pos2::new(r.left() + 10.0 + j as f32 * 3.0, r.top() + 9.0 + i as f32 * 3.5), 0.9, app);
        }
    }
    let g = galley(ui, name, semi(12.0), t.text_dim);
    let buttons = 1.0 + f32::from(u8::from(pin)) + f32::from(u8::from(close));
    let room = (w - 22.0 - 8.0 - buttons * 29.0).max(20.0);
    let gw = g.size().x;
    let el = galley(ui, "…", semi(12.0), t.text_dim);
    // a name too long is cut short with an ellipsis (`text-overflow: ellipsis`)
    let shown = if gw > room { room - el.size().x } else { room };
    let clip = Rect::from_min_size(Pos2::new(r.left() + 22.0, r.top()), Vec2::new(shown, GRIP_H));
    ui.painter().with_clip_rect(clip).galley(Pos2::new(r.left() + 22.0, r.center().y - g.size().y / 2.0), g, t.text_dim);
    if gw > room {
        ui.painter().galley(Pos2::new(clip.right(), r.center().y - el.size().y / 2.0), el, t.text_dim);
    }
    let mut act = GripAct::None;
    let mut x = r.right() - 4.0;
    for (icon, a) in [("x-lg", GripAct::Close), ("pin-angle", GripAct::Pin), ("three-dots", GripAct::Menu)] {
        if (a == GripAct::Pin && !pin) || (a == GripAct::Close && !close) {
            continue;
        }
        let br = Rect::from_min_size(Pos2::new(x - 26.0, r.top() + 3.0), Vec2::new(26.0, 22.0));
        x -= 29.0;
        let resp =
            ui.interact(br, ui.make_persistent_id(("grip", name, icon)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        if resp.hovered() {
            ui.painter().rect(br, 2.0, t.hover, Stroke::new(1.0, t.line), egui::StrokeKind::Inside);
        }
        crate::ribbon::paint_icon(ui.painter(), Rect::from_center_size(br.center(), Vec2::splat(13.0)), icon, t.text);
        if resp.clicked() {
            act = a;
        }
    }
    act
}

/// A panel section's head: the title (0.7 `.cc-h`: 15 px bold) and room on the right for link buttons.
pub fn section_head(ui: &mut egui::Ui, t: &Tokens, title: &str, links: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(22.0);
        ui.label(egui::RichText::new(title).font(head(15.0)).color(t.text_strong));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            links(ui);
        });
    });
}

/// A section's link button (0.7 `.cc-linkbtn`: 14 px semibold in the action colour, underlined), with an optional
/// leading icon.
pub fn link(ui: &mut egui::Ui, t: &Tokens, icon: &str, label: &str) -> Response {
    let g = galley(ui, label, semi(14.0), t.accent);
    let iw = if icon.is_empty() { 0.0 } else { 17.0 };
    let (r, resp) = ui.allocate_exact_size(Vec2::new(g.size().x + iw, 22.0), Sense::click());
    if !icon.is_empty() {
        crate::ribbon::paint_icon(
            ui.painter(),
            Rect::from_min_size(Pos2::new(r.left(), r.center().y - 7.0), Vec2::splat(14.0)),
            icon,
            t.accent,
        );
    }
    let gx = r.left() + iw;
    let gw = g.size().x;
    let base = r.center().y + g.size().y / 2.0 - 2.0;
    ui.painter().galley(Pos2::new(gx, r.center().y - g.size().y / 2.0), g, t.accent);
    let th = if resp.hovered() { 2.0 } else { 1.0 };
    ui.painter().line_segment([Pos2::new(gx, base), Pos2::new(gx + gw, base)], Stroke::new(th, t.accent));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Has nothing been placed in `ui` yet (a section head at the top of its panel gets no rule over it)?
fn at_top(ui: &egui::Ui) -> bool {
    ui.cursor().top() <= ui.max_rect().top() + 1.0
}

/// The rule between two panel sections (0.7 `.cc-sec`: padding .9rem 1rem, a 1 px line under it), drawn across the
/// panel's whole width (its clip rect) with the sections' 14 px padding on either side.
pub fn sec_break(ui: &mut egui::Ui, t: &Tokens) {
    ui.add_space(14.0);
    let clip = ui.clip_rect();
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().line_segment([Pos2::new(clip.left(), r.center().y), Pos2::new(clip.right(), r.center().y)], Stroke::new(1.0, t.line));
    ui.add_space(14.0);
}

/// A 1 px line rule across the current column only (`.cc-plot-opts` groups), with 8 px on either side.
pub fn sec_break_in(ui: &mut egui::Ui, t: &Tokens) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().line_segment([Pos2::new(r.left(), r.center().y), Pos2::new(r.right(), r.center().y)], Stroke::new(1.0, t.line));
    ui.add_space(8.0);
}

/// A panel section's head as every 0.5 / 0.7 app draws it (`.cc-h`, `.cv-h`, `.dc-h`, `.pc-h`, `.tc-h`, `.mc-h`,
/// `.bc-h`: 15 px, bold, 112 % wide, ink; .5rem under it). A head that does not open its panel starts a new
/// section: the 1 px rule across the panel goes over it.
pub fn h(ui: &mut egui::Ui, t: &Tokens, title: &str) -> Response {
    if !at_top(ui) {
        sec_break(ui, t);
    }
    let r = ui.label(egui::RichText::new(title).font(head(15.0)).color(t.text_strong));
    ui.add_space(4.0);
    r
}

/// A head inside a section (`.cv-h3` and the like: 14 px bold, .9rem over it, .4rem under it).
pub fn h3(ui: &mut egui::Ui, t: &Tokens, title: &str) -> Response {
    if !at_top(ui) {
        ui.add_space(10.0);
    }
    let r = ui.label(egui::RichText::new(title).font(bold(14.0)).color(t.text_strong));
    ui.add_space(2.0);
    r
}

/// A head that opens a section with no rule over it (the one before it drew its own: [`fold_row`]).
pub fn h_plain(ui: &mut egui::Ui, t: &Tokens, title: &str) -> Response {
    let r = ui.label(egui::RichText::new(title).font(head(15.0)).color(t.text_strong));
    ui.add_space(4.0);
    r
}

/// One folding row of a panel (MarkupCraft's `.mc-find-sec` / `.mc-layers-sec`: a `<details>` whose summary is the
/// browser's ▸ marker, an icon and a 14 px semibold title; a 1 px rule across the panel under the row).
#[allow(clippy::too_many_arguments)]
pub fn fold_row<R>(
    ui: &mut egui::Ui,
    t: &Tokens,
    id: impl std::hash::Hash + std::fmt::Debug,
    icon: &str,
    title: &str,
    tip: &str,
    force: Option<bool>,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    let id = ui.make_persistent_id(id);
    let mut st = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
    if let Some(o) = force {
        st.set_open(o);
    }
    ui.add_space(5.0);
    let g = galley(ui, title, semi(14.0), t.text_strong);
    let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::click());
    let mut resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if !tip.is_empty() {
        resp = resp.on_hover_text(tip);
    }
    let c = Pos2::new(r.left() + 4.0, r.center().y);
    let tri = if st.is_open() {
        vec![Pos2::new(c.x - 4.0, c.y - 2.5), Pos2::new(c.x + 4.0, c.y - 2.5), Pos2::new(c.x, c.y + 3.5)]
    } else {
        vec![Pos2::new(c.x - 2.5, c.y - 4.0), Pos2::new(c.x + 3.5, c.y), Pos2::new(c.x - 2.5, c.y + 4.0)]
    };
    ui.painter().add(egui::Shape::convex_polygon(tri, t.text_strong, Stroke::NONE));
    let mut x = r.left() + 14.0;
    if !icon.is_empty() {
        crate::ribbon::paint_icon(
            ui.painter(),
            Rect::from_min_size(Pos2::new(x, r.center().y - 7.0), Vec2::splat(14.0)),
            icon,
            t.text_strong,
        );
        x += 17.0;
    }
    ui.painter().galley(Pos2::new(x, r.center().y - g.size().y / 2.0), g, t.text_strong);
    if resp.clicked() {
        st.toggle(ui);
    }
    st.store(ui.ctx());
    let out = if st.is_open() {
        ui.add_space(4.0);
        Some(add(ui))
    } else {
        None
    };
    ui.add_space(5.0);
    let clip = ui.clip_rect();
    let (rr, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().line_segment([Pos2::new(clip.left(), rr.center().y), Pos2::new(clip.right(), rr.center().y)], Stroke::new(1.0, t.line));
    out
}

/// A whole panel section: its head ([`h`], a rule over it unless it opens the panel), then its body.
pub fn section<R>(ui: &mut egui::Ui, t: &Tokens, title: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    h(ui, t, title);
    add(ui)
}

/// A sub-head in a properties list (`.pc-props-sub`, `.dc-sub`, `.cc-psec` subs: 13 px bold, ink-2, .5rem over it).
pub fn sub(ui: &mut egui::Ui, t: &Tokens, title: &str) -> Response {
    if !at_top(ui) {
        ui.add_space(8.0);
    }
    ui.label(egui::RichText::new(title).font(bold(13.0)).color(t.text_dim))
}

/// A section that folds (`.dc-fold`: a `<details>` whose summary is the section head with a small chevron before
/// it, ⌄ open and › closed; the body not indented). It starts a new section as [`h`] does. Returns whether it is open.
pub fn fold<R>(
    ui: &mut egui::Ui,
    t: &Tokens,
    id: impl std::hash::Hash + std::fmt::Debug,
    title: &str,
    default_open: bool,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    fold_open(ui, t, id, title, default_open, None, add)
}

/// [`fold`] that `force` opens or closes this frame (egui's `CollapsingHeader::open`).
pub fn fold_open<R>(
    ui: &mut egui::Ui,
    t: &Tokens,
    id: impl std::hash::Hash + std::fmt::Debug,
    title: &str,
    default_open: bool,
    force: Option<bool>,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    if !at_top(ui) {
        sec_break(ui, t);
    }
    let id = ui.make_persistent_id(id);
    let mut st = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, default_open);
    if let Some(o) = force {
        st.set_open(o);
    }
    let open = st.is_open();
    let g = galley(ui, title, head(15.0), t.text_strong);
    let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), g.size().y.max(22.0)), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    // the chevron: a 7 px corner turned down (open) or right (closed), 1.5 px ink-2
    let c = Pos2::new(r.left() + 4.5, r.center().y);
    let s = 3.2;
    let pts = if open {
        [Pos2::new(c.x - s, c.y - s * 0.5), Pos2::new(c.x, c.y + s * 0.5), Pos2::new(c.x + s, c.y - s * 0.5)]
    } else {
        [Pos2::new(c.x - s * 0.5, c.y - s), Pos2::new(c.x + s * 0.5, c.y), Pos2::new(c.x - s * 0.5, c.y + s)]
    };
    ui.painter().line(pts.to_vec(), Stroke::new(1.5, t.text_dim));
    ui.painter().galley(Pos2::new(r.left() + 13.0, r.center().y - g.size().y / 2.0), g, t.text_strong);
    if resp.clicked() {
        st.toggle(ui);
    }
    st.store(ui.ctx());
    if st.is_open() {
        ui.add_space(8.0);
        Some(add(ui))
    } else {
        None
    }
}

/// A rule under a panel section (0.7 `.cc-sec`: 1 px line, full width of the panel).
pub fn section_rule(ui: &mut egui::Ui, t: &Tokens) {
    let full = ui.max_rect();
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().line_segment(
        [Pos2::new(full.left() - 16.0, r.center().y), Pos2::new(full.right() + 16.0, r.center().y)],
        Stroke::new(1.0, t.line),
    );
}

/// A status-bar toggle (0.7 `.cc-toggle`: 24 px, 13 px semibold, 1 px line border; on = filled with the action
/// colour).
pub fn toggle(ui: &mut egui::Ui, t: &Tokens, label: &str, on: bool) -> Response {
    let ink = if on { t.on_accent } else { t.text_dim };
    let g = galley(ui, label, semi(13.0), ink);
    let (r, resp) = ui.allocate_exact_size(Vec2::new(g.size().x + 16.0, 24.0), Sense::click());
    let (fill, edge) = if on {
        (t.accent, t.accent)
    } else if resp.hovered() {
        (t.hover, t.rule)
    } else {
        (Color32::TRANSPARENT, t.line)
    };
    ui.painter().rect(r, 2.0, fill, Stroke::new(1.0, edge), egui::StrokeKind::Inside);
    ui.painter().galley(Pos2::new(r.left() + 8.0, r.center().y - g.size().y / 2.0), g, ink);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The outlined action button (0.7 `.btn-line.btn-sm-cc`: 2 px action border, 15 px bold action text, 40 px); with
/// `filled` the solid one (`.btn-cc`: action fill, on-action text).
pub fn action_button(ui: &mut egui::Ui, t: &Tokens, icon: &str, label: &str, filled: bool) -> Response {
    let ink = if filled { t.on_accent } else { t.accent };
    let g = galley(ui, label, bold(15.0), ink);
    let iw = if icon.is_empty() { 0.0 } else { 22.0 };
    let (r, resp) = ui.allocate_exact_size(Vec2::new(g.size().x + iw + 32.0, 40.0), Sense::click());
    let fill = if filled {
        if resp.hovered() { crate::theme::mix(t.accent, t.rule, 0.85) } else { t.accent }
    } else if resp.hovered() {
        t.row_selected
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect(r, 2.0, fill, Stroke::new(2.0, t.accent), egui::StrokeKind::Inside);
    let mut x = r.left() + 16.0;
    if !icon.is_empty() {
        crate::ribbon::paint_icon(ui.painter(), Rect::from_min_size(Pos2::new(x, r.center().y - 8.0), Vec2::splat(16.0)), icon, ink);
        x += iw;
    }
    ui.painter().galley(Pos2::new(x, r.center().y - g.size().y / 2.0), g, ink);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A key cap in running text (0.7 `kbd`: 1 px ink border, 12 px semibold).
pub fn kbd(ui: &mut egui::Ui, t: &Tokens, key: &str) -> Response {
    let g = galley(ui, key, semi(12.0), t.text_strong);
    let (r, resp) = ui.allocate_exact_size(Vec2::new(g.size().x + 10.0, 19.0), Sense::hover());
    ui.painter().rect(r, 2.0, t.panel_darker, Stroke::new(1.0, t.rule), egui::StrokeKind::Inside);
    ui.painter().galley(Pos2::new(r.left() + 5.0, r.center().y - g.size().y / 2.0), g, t.text_strong);
    resp
}

/// A side dock (`egui::Panel::left` / `right`) with 0.7's edge toward the stage: `aside[data-shell-dock]`'s 1.5 px ink
/// rule (egui draws a panel's edge in the parent's separator stroke; inside, the panel keeps the theme's own).
pub fn side<R>(ui: &mut egui::Ui, panel: egui::Panel, add: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<R> {
    let t = tokens_of(ui.ctx());
    let keep = ui.style().visuals.widgets.noninteractive.bg_stroke;
    ui.style_mut().visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.5, t.rule);
    let r = panel.show(ui, |ui| {
        ui.style_mut().visuals.widgets.noninteractive.bg_stroke = keep;
        add(ui)
    });
    ui.style_mut().visuals.widgets.noninteractive.bg_stroke = keep;
    r
}

/// A frame for a dock panel's body (0.7 `.cc-panel`: sheet-2, the sections' 16 px gutter).
pub fn panel_frame(t: &Tokens) -> egui::Frame {
    egui::Frame::new().fill(t.panel).inner_margin(egui::Margin::ZERO)
}

/// The 16 px gutter inside a panel section.
pub fn section_frame() -> egui::Frame {
    egui::Frame::new().inner_margin(egui::Margin { left: 16, right: 16, top: 14, bottom: 14 })
}

/// A drawing tab under the canvas (0.7 `.cc-tab`: 29 px, 13 px semibold; the open one on the sheet colour with a
/// 2 px action rule under it).
pub fn tab(ui: &mut egui::Ui, t: &Tokens, label: &str, on: bool) -> Response {
    let ink = if on { t.text_strong } else { t.text_dim };
    let g = galley(ui, label, semi(13.0), ink);
    let (r, resp) = ui.allocate_exact_size(Vec2::new((g.size().x + 24.0).max(29.0), 29.0), Sense::click());
    if on {
        ui.painter().rect_filled(r, 0.0, t.panel_darker);
        ui.painter().rect_filled(Rect::from_min_max(Pos2::new(r.left(), r.bottom() - 2.0), r.max), 0.0, t.accent);
    } else if resp.hovered() {
        ui.painter().rect_filled(r, 0.0, t.hover);
    }
    ui.painter().galley(Pos2::new(r.center().x - g.size().x / 2.0, r.center().y - g.size().y / 2.0 - 1.0), g, ink);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The shell's own view state: the drafting grid (cadcraft.js `settings.grid`, on by default).
#[derive(Clone, Debug)]
pub struct Look {
    pub grid: bool,
    /// the Start drawing card has gone (0.5 `hideHint()`: a command ran, or the drawing has objects)
    pub hint_gone: bool,
    /// the Layers panel's New layer form: the name being typed (None: closed)
    pub new_layer: Option<String>,
}

impl Default for Look {
    fn default() -> Self {
        Look { grid: true, hint_gone: false, new_layer: None }
    }
}

/// A piece of a hint line: words, a key cap, or a bold word.
pub enum Frag {
    Text(String),
    Kbd(&'static str),
    Strong(&'static str),
}

/// The "Start drawing" card over an empty drawing (0.5 ui.inc `.cc-hint`: 16 px from the canvas corner, at most
/// 512 px wide, sheet-2 with a 1.5 px ink border; the heading bold, then a bulleted list where key caps sit in the
/// words). It takes no pointer input (`pointer-events: none`).
pub fn hint_card(ctx: &egui::Context, t: &Tokens, at: Pos2, max_w: f32, title: &str, items: &[Vec<Frag>]) {
    egui::Area::new(egui::Id::unique("cc-hint")).fixed_pos(at).order(egui::Order::Middle).interactable(false).show(ctx, |ui| {
        egui::Frame::new()
            .fill(t.panel)
            .stroke(Stroke::new(1.5, t.rule))
            .corner_radius(2)
            .inner_margin(egui::Margin { left: 16, right: 16, top: 12, bottom: 12 })
            .show(ui, |ui| {
                ui.set_max_width(max_w - 32.0);
                ui.label(egui::RichText::new(title).font(bold(14.0)).color(t.text_strong));
                ui.add_space(6.0);
                for item in items {
                    ui.horizontal(|ui| {
                        ui.add_space(4.0);
                        let (r, _) = ui.allocate_exact_size(Vec2::new(10.0, 22.0), Sense::hover());
                        ui.painter().circle_filled(Pos2::new(r.left() + 3.0, r.top() + 11.0), 2.2, t.text_strong);
                        ui.vertical(|ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::new(4.0, 3.0);
                                for f in item {
                                    match f {
                                        Frag::Text(s) => {
                                            for w in s.split_whitespace() {
                                                ui.label(egui::RichText::new(w).size(14.0).color(t.text_strong));
                                            }
                                        }
                                        Frag::Kbd(k) => {
                                            kbd(ui, t, k);
                                        }
                                        Frag::Strong(s) => {
                                            ui.label(egui::RichText::new(*s).font(bold(14.0)).color(t.text_strong));
                                        }
                                    }
                                }
                            });
                        });
                    });
                    ui.add_space(2.0);
                }
            });
    });
}

/// A panel note (0.7 `.cc-note`: 14 px, ink-2, 1.6 line height).
pub fn note(ui: &mut egui::Ui, t: &Tokens, text: &str) -> Response {
    ui.label(egui::RichText::new(text).size(14.0).line_height(Some(22.0)).color(t.text_dim))
}

/// One button of a 0.7 toolbar (`.cc-tool`) for [`toolbar`]: `id` comes back when it is clicked.
pub struct Tool {
    pub id: String,
    pub icon: &'static str,
    pub label: String,
    pub tip: String,
    pub on: bool,
    pub enabled: bool,
}

impl Tool {
    pub fn new(id: impl Into<String>, icon: &'static str, label: impl Into<String>) -> Tool {
        Tool { id: id.into(), icon, label: label.into(), tip: String::new(), on: false, enabled: true }
    }
    pub fn tip(mut self, tip: impl Into<String>) -> Tool {
        self.tip = tip.into();
        self
    }
    pub fn on(mut self, on: bool) -> Tool {
        self.on = on;
        self
    }
    pub fn enabled(mut self, enabled: bool) -> Tool {
        self.enabled = enabled;
        self
    }
}

/// A labelled group of toolbar buttons (`.cc-tool-group`).
pub struct ToolGroup {
    pub label: String,
    pub tools: Vec<Tool>,
}

/// 0.7 toolbar metrics (cadcraft.css): a group's padding (.25rem .35rem .3rem), the gap between its buttons (.1rem),
/// its height (label 22 px + buttons 48 px + padding), a button's least width (2.6rem).
pub const TB_PAD_X: f32 = 5.6;
pub const TB_GAP: f32 = 1.6;
pub const TB_GROUP_H: f32 = 79.0;
pub const TB_TOOL_MIN: f32 = 41.6;

/// A toolbar button's width for its label.
pub fn tool_width(ui: &egui::Ui, label: &str) -> f32 {
    let g = ui.painter().layout_no_wrap(label.to_string(), semi(11.5), Color32::WHITE);
    (g.size().x + 6.4 + 2.0).max(TB_TOOL_MIN).ceil()
}

/// Where each group goes in `width` (left to right, wrapping like 0.7's flex-wrap): (top-left, width) per group, and
/// the bar's height with its 1.5 px rule.
pub fn toolbar_layout(ui: &egui::Ui, groups: &[ToolGroup], width: f32) -> (Vec<(Pos2, f32)>, f32) {
    let (mut x, mut y) = (0.0_f32, 0.0_f32);
    let mut out = Vec::with_capacity(groups.len());
    for g in groups {
        let n = g.tools.len();
        let inner: f32 = g.tools.iter().map(|t| tool_width(ui, &t.label)).sum::<f32>() + TB_GAP * n.saturating_sub(1) as f32;
        let w = (2.0 * TB_PAD_X + inner + 1.0).ceil();
        if x > 0.0 && x + w > width {
            x = 0.0;
            y += TB_GROUP_H;
        }
        out.push((Pos2::new(x, y), w));
        x += w;
    }
    (out, y + TB_GROUP_H + 1.5)
}

/// One toolbar button in `r` (`.cc-tool`): the icon in the app's colour over the 11.5 px semibold label; a line border
/// and the sheet fill on hover, an ink border when pressed, 45 % strength when it cannot act.
#[allow(clippy::too_many_arguments)]
pub fn tool_button(
    ui: &mut egui::Ui,
    t: &Tokens,
    r: Rect,
    id: egui::Id,
    icon: &str,
    label: &str,
    app: Color32,
    on: bool,
    enabled: bool,
) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let resp = ui.interact(r, id, sense);
    let resp = if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp };
    let p = ui.painter();
    if on {
        p.rect(r, f32::from(t.radius), t.panel_darker, Stroke::new(1.0, t.rule), egui::StrokeKind::Inside);
    } else if enabled && resp.hovered() {
        p.rect(r, f32::from(t.radius), t.panel_darker, Stroke::new(1.0, t.line), egui::StrokeKind::Inside);
    }
    let (ic, tc) = if enabled { (app, t.text) } else { (crate::theme::mix(app, t.panel, 0.45), t.text_disabled) };
    crate::ribbon::paint_icon(p, Rect::from_min_size(Pos2::new(r.center().x - 9.0, r.top() + 5.0), Vec2::splat(18.0)), icon, ic);
    p.text(Pos2::new(r.center().x, r.top() + 34.0), Align2::CENTER_CENTER, label, semi(11.5), tc);
    resp
}

/// The height [`toolbar`] needs for `groups` in `width` (call before its panel, for the panel's size).
pub fn toolbar_height(ui: &egui::Ui, groups: &[ToolGroup], width: f32) -> f32 {
    toolbar_layout(ui, groups, width).1
}

/// A 0.7 toolbar inside the current panel (fill it with `t.app_bar`, no margin): the groups with their labels and
/// right-hand rules, wrapping onto rows, a 1.5 px ink rule under all. Returns every button's id and response (the
/// caller reads `clicked()`, or opens a menu under one with `egui::Popup::menu`).
pub fn toolbar(ui: &mut egui::Ui, t: &Tokens, app: Color32, salt: &str, groups: &[ToolGroup]) -> Vec<(String, Response)> {
    let (at, h) = toolbar_layout(ui, groups, ui.available_width());
    let (full, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::hover());
    let base = full.min;
    ui.painter().line_segment(
        [Pos2::new(full.left(), full.bottom() - 0.75), Pos2::new(full.right(), full.bottom() - 0.75)],
        Stroke::new(1.5, t.rule),
    );
    let mut out = Vec::new();
    for (gi, (g, (pos, w))) in groups.iter().zip(at).enumerate() {
        let gr = Rect::from_min_size(base + pos.to_vec2(), Vec2::new(w, TB_GROUP_H));
        let p = ui.painter();
        p.text(gr.min + Vec2::new(TB_PAD_X + 3.2, 5.0), Align2::LEFT_TOP, &g.label, semi(12.0), t.text_dim);
        p.line_segment([Pos2::new(gr.right() - 0.5, gr.top()), Pos2::new(gr.right() - 0.5, gr.bottom())], Stroke::new(1.0, t.line));
        let mut x = gr.left() + TB_PAD_X;
        for (ti, tool) in g.tools.iter().enumerate() {
            let tw = tool_width(ui, &tool.label);
            let r = Rect::from_min_size(Pos2::new(x, gr.top() + 26.0), Vec2::new(tw, 48.0));
            x += tw + TB_GAP;
            let id = ui.make_persistent_id((salt, gi, ti));
            let mut resp = tool_button(ui, t, r, id, tool.icon, &tool.label, app, tool.on, tool.enabled);
            if !tool.tip.is_empty() {
                resp = resp.on_hover_text(&tool.tip);
            }
            out.push((tool.id.clone(), resp));
        }
    }
    out
}

/// A word in the app bar that stays pressed while its panel or view is on (the action colour and a 2 px rule).
pub fn bar_toggle(ui: &mut egui::Ui, t: &Tokens, label: &str, on: bool) -> Response {
    let c = if on { t.accent } else { t.text_dim };
    let g = galley(ui, label, semi(15.0), c);
    let (r, resp) = ui.allocate_exact_size(Vec2::new(g.size().x + 4.0, 28.0), Sense::click());
    let gw = g.size().x;
    let base = r.center().y + g.size().y / 2.0 - 1.0;
    ui.painter().galley(Pos2::new(r.left() + 2.0, r.center().y - g.size().y / 2.0), g, c);
    if on || resp.hovered() {
        let w = if on { 2.0 } else { 1.0 };
        ui.painter().line_segment([Pos2::new(r.left() + 2.0, base + 2.0), Pos2::new(r.left() + 2.0 + gw, base + 2.0)], Stroke::new(w, c));
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A panel field's width that fills what is left of its row or column, as 0.7's `.dc-input`, `.cc-input` and the
/// other apps' panel inputs (`width: 100%; min-width: 0`).
pub const FILL: f32 = f32::INFINITY;

/// `width`, or for [`FILL`] what is left of the row (never under 40 px). A select that fills is made with
/// `ComboBox::from_id_salt` and `.truncate()`: `ComboBox::from_id_salt(id)` puts an empty label after the box with a gap
/// before it, and an untruncated long choice widens the box; either makes it wider than the column, and a dock panel
/// then grows frame after frame.
pub fn width_in(ui: &egui::Ui, width: f32) -> f32 {
    if width.is_finite() {
        width
    } else if ui.ctx().will_discard() {
        // see [`width_to_edge`]
        40.0
    } else {
        ui.available_width().max(40.0)
    }
}

/// The width that takes a field from where the next widget goes to the right edge of the region it is in, keeping
/// `reserve` for what follows it — also in a grid's middle column, whose cells egui sizes from the last frame.
pub fn width_to_edge(ui: &egui::Ui, reserve: f32) -> f32 {
    // a pass egui lays out again (a grid's first frame, when its columns are not known yet) sizes nothing: what it
    // measured would be kept as the column's width and widen the panel
    if ui.ctx().will_discard() {
        return 40.0;
    }
    (ui.max_rect().right() - ui.cursor().left() - reserve).max(40.0)
}

/// The width a select with a label after it (`ComboBox::new(id, label)`) gets to fill its row: the label and the gap
/// before it kept.
pub fn width_before_label(ui: &egui::Ui, label: &str) -> f32 {
    let w =
        egui::WidgetText::from(label).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Button).size().x;
    if ui.ctx().will_discard() {
        return 40.0;
    }
    (ui.available_width() - w - ui.spacing().item_spacing.x).max(40.0)
}

/// A one-line field as 0.7 draws them (`.cc-input`: the text centred in the box, 8 px in from the edge); every
/// panel's text fields come through here (the theme gives the sheet fill and the 1 px line border).
pub fn field(text: &mut dyn egui::TextBuffer) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(text).align(egui::Align2::LEFT_CENTER).margin(egui::Margin::symmetric(8, 4))
}

/// The theme's tokens as the last `Tokens::apply` left them (for widgets that get no tokens passed in).
pub fn tokens_of(ctx: &egui::Context) -> Tokens {
    ctx.data(|d| d.get_temp::<Tokens>(egui::Id::unique(TOKENS_KEY))).unwrap_or_else(|| Tokens::preset(crate::theme::Preset::Light))
}

pub(crate) const TOKENS_KEY: &str = "cc-shell-tokens";

/// The button size of the app showing: `.btn-sm-cc` (40 px, 15 px bold, .9rem in) in every app but DetailCraft, whose
/// panels and dialogs use `.btn-sm-dc` (2.1rem, 14 px, .75rem in). Set once a frame by the window ([`set_small_buttons`]).
fn btn_metrics(ctx: &egui::Context) -> (f32, egui::FontId, f32) {
    if ctx.data(|d| d.get_temp::<bool>(egui::Id::unique(BTN_DC_KEY))).unwrap_or(false) {
        (34.0, bold(14.0), 12.0)
    } else {
        (40.0, bold(15.0), 14.4)
    }
}

const BTN_DC_KEY: &str = "cc-shell-btn-dc";

/// DetailCraft's smaller buttons on (`.btn-sm-dc`) or off for this frame.
pub fn set_small_buttons(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique(BTN_DC_KEY), on));
}

/// A panel's or dialog's button as 0.7 draws it (`.btn-line.btn-sm-cc`): the action colour's text and 2 px border,
/// 40 px tall with 15 px bold words, the fill on hover; greyed out when the ui is disabled. Inside a menu it is the
/// menu's plain item.
pub struct Btn {
    text: egui::WidgetText,
}

impl Btn {
    pub fn new(text: impl Into<egui::WidgetText>) -> Btn {
        Btn { text: text.into() }
    }
}

impl egui::Widget for Btn {
    fn ui(self, ui: &mut egui::Ui) -> Response {
        if egui::containers::menu::is_in_menu(ui) {
            return ui.add(egui::Button::new(self.text));
        }
        let t = tokens_of(ui.ctx());
        let on = ui.is_enabled();
        let ink = if on { t.accent } else { t.text_disabled };
        let (h, font, pad) = btn_metrics(ui.ctx());
        let g = self.text.into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, font);
        let (r, resp) = ui.allocate_exact_size(Vec2::new(g.size().x + 2.0 * pad, h), Sense::click());
        let fill = if on && resp.hovered() { t.row_selected } else { Color32::TRANSPARENT };
        let edge = if on { t.accent } else { t.line };
        ui.painter().rect(r, 2.0, fill, Stroke::new(if on { 2.0 } else { 1.0 }, edge), egui::StrokeKind::Inside);
        ui.painter().galley_with_override_text_color(Pos2::new(r.left() + pad, r.center().y - g.size().y / 2.0), g, ink);
        if on { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
    }
}

impl Btn {
    /// The filled primary button (0.7 `.btn-cc`: the action fill, on-action text), for a dialog's main action
    /// (Download PDF, OK / Apply, Open).
    pub fn primary(text: impl Into<egui::WidgetText>) -> Primary {
        Primary { text: text.into() }
    }
}

/// See [`Btn::primary`].
pub struct Primary {
    text: egui::WidgetText,
}

impl egui::Widget for Primary {
    fn ui(self, ui: &mut egui::Ui) -> Response {
        let t = tokens_of(ui.ctx());
        let on = ui.is_enabled();
        let (h, font, pad) = btn_metrics(ui.ctx());
        let g = self.text.into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, font);
        let (r, resp) = ui.allocate_exact_size(Vec2::new(g.size().x + 2.0 * pad, h), Sense::click());
        let fill = if !on {
            crate::theme::mix(t.accent, t.panel, 0.45)
        } else if resp.hovered() {
            crate::theme::mix(t.accent, t.rule, 0.85)
        } else {
            t.accent
        };
        ui.painter().rect(r, 2.0, fill, Stroke::new(2.0, fill), egui::StrokeKind::Inside);
        ui.painter().galley_with_override_text_color(Pos2::new(r.left() + pad, r.center().y - g.size().y / 2.0), g, t.on_accent);
        if on { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
    }
}

/// A dialog as 0.7 shows them: a window with its own title row (the 17 px bold title at the left, ✕ at the right, a
/// 1 px rule under it) and, for a modal one, the page dimmed behind it. It takes egui::Window's builder calls.
pub struct Dialog<'a> {
    win: egui::Window<'a>,
    title: egui::WidgetText,
    open: Option<&'a mut bool>,
    titled: bool,
    modal: bool,
    /// the shell's own dialogs (Settings, Recent files, Recover): a 20 px head (`.shell-dlg-head h2`)
    big: bool,
    /// the caller placed the window (else a modal one opens in the middle of the page, as `showModal()` does)
    placed: bool,
    placed_pivot: bool,
}

impl<'a> Dialog<'a> {
    pub fn new(title: impl Into<egui::WidgetText>) -> Dialog<'a> {
        let title: egui::WidgetText = title.into();
        Dialog {
            win: egui::Window::new(title.clone()).title_bar(false),
            title,
            open: None,
            titled: true,
            modal: true,
            big: false,
            placed: false,
            placed_pivot: false,
        }
    }
    /// One of the shell's own dialogs (Settings, Recent files, Recover your work?): its title at 20 px.
    pub fn shell_head(mut self) -> Self {
        self.big = true;
        self
    }
    /// A panel-like window: no dimmed page behind it.
    pub fn modeless(mut self) -> Self {
        self.modal = false;
        self
    }
    pub fn open(mut self, open: &'a mut bool) -> Self {
        self.open = Some(open);
        self
    }
    /// `false`: no title row at all (the window draws its own).
    pub fn title_bar(mut self, on: bool) -> Self {
        self.titled = on;
        self
    }
    /// Kept for egui::Window's call shape: a dialog never folds.
    pub fn collapsible(self, _on: bool) -> Self {
        self
    }
    pub fn show<R>(self, ctx: &egui::Context, add: impl FnOnce(&mut egui::Ui) -> R) -> Option<egui::InnerResponse<Option<R>>> {
        let Dialog { mut win, title, open, titled, modal, big, placed, placed_pivot } = self;
        if modal && !placed {
            if !placed_pivot {
                win = win.pivot(egui::Align2::CENTER_CENTER);
            }
            win = win.default_pos(ctx.content_rect().center());
        }
        if open.as_deref() == Some(&false) {
            return None;
        }
        let t = tokens_of(ctx);
        if modal {
            // 0.7's ::backdrop: the page under the ink at 35 % (`color-mix(in srgb, var(--ink) 35%, transparent)`), on a
            // background layer above the panels, under the windows
            let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::unique("cc-dialog-backdrop")));
            p.rect_filled(ctx.content_rect(), 0.0, t.rule.gamma_multiply(0.35));
        }
        // 0.7's `.cv-dialog` / `.dc-dialog` …: sheet-2, a 1 px ink edge, radius 2; the head and foot 16 px in, 10 px over
        // and under, the body 16 px in
        let frame = egui::Frame::new().fill(t.panel).stroke(Stroke::new(1.0, t.rule)).corner_radius(2).inner_margin(egui::Margin {
            left: 16,
            right: 16,
            top: 11,
            bottom: 11,
        });
        let mut close = false;
        let r = win.frame(frame).show(ctx, |ui| {
            if titled {
                ui.horizontal(|ui| {
                    ui.set_min_height(34.0);
                    // `.cv-dlg-head .cv-h`: the app's section head, 15 px bold at 112 %
                    ui.label(egui::RichText::new(title.text()).font(head(if big { 20.0 } else { 15.0 })).color(t.text_strong));
                    if open.is_some() {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let (br, resp) = ui.allocate_exact_size(Vec2::new(32.0, 34.0), Sense::click());
                            let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                            if resp.hovered() {
                                ui.painter().rect(br, 2.0, t.hover, Stroke::new(1.0, t.line), egui::StrokeKind::Inside);
                            }
                            crate::ribbon::paint_icon(ui.painter(), Rect::from_center_size(br.center(), Vec2::splat(16.0)), "x-lg", t.text);
                            close = resp.clicked();
                        });
                    }
                });
                // the head's 1 px line rule across the window, then the body's 14 px
                let full = ui.max_rect();
                let (rr, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 11.0), Sense::hover());
                ui.painter().line_segment(
                    [Pos2::new(full.left() - 16.0, rr.bottom() - 0.5), Pos2::new(full.right() + 16.0, rr.bottom() - 0.5)],
                    Stroke::new(1.0, t.line),
                );
                // the body's 14 px (`.shell-dlg-body`); a shell dialog's tabs sit 8 px under the head
                ui.add_space(if big { 8.0 } else { 14.0 });
            }
            add(ui)
        });
        if close && let Some(o) = open {
            *o = false;
        }
        r
    }
}

/// The rule over a dialog's foot, across the window, with the foot's spacing (for a foot laid out by hand).
pub fn foot_rule(ui: &mut egui::Ui) {
    ui.add_space(14.0);
    foot_line(ui);
}

fn foot_line(ui: &mut egui::Ui) {
    let t = tokens_of(ui.ctx());
    let full = ui.max_rect();
    let (rr, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().line_segment(
        [Pos2::new(full.left() - 16.0, rr.center().y), Pos2::new(full.right() + 16.0, rr.center().y)],
        Stroke::new(1.0, t.line),
    );
    ui.add_space(10.0);
}

/// A dialog's foot (`.cv-dlg-foot` and the like): a 1 px line rule across the window, then the buttons at the right
/// with 12 px between them, in the order they are added (0.7: Cancel, then the main action at the far right).
pub fn foot<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    foot_split(ui, |_| {}, add)
}

/// [`foot`] with something at its left too (`justify-content: space-between`: Reset, Discard all, a status line).
pub fn foot_split<R>(ui: &mut egui::Ui, left: impl FnOnce(&mut egui::Ui), right: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.add_space(14.0);
    foot_flush_split(ui, left, right)
}

/// [`foot_split`] right under what is above it (a body that fills the window to its foot).
pub fn foot_flush_split<R>(ui: &mut egui::Ui, left: impl FnOnce(&mut egui::Ui), right: impl FnOnce(&mut egui::Ui) -> R) -> R {
    foot_line(ui);
    // the buttons' width, measured, puts them against the right edge (egui lays out left to right only): when it changes
    // (a new dialog, other words) the pass is laid out again before it is shown (`request_discard`), so even the first
    // frame has them at the right
    let id = ui.next_auto_id().with("cc-foot-w");
    let was: f32 = ui.ctx().data(|d| d.get_temp(id)).unwrap_or(0.0);
    ui.horizontal(|ui| {
        ui.set_min_height(40.0);
        ui.spacing_mut().item_spacing.x = 12.0;
        let x0 = ui.cursor().left();
        left(ui);
        let used = ui.cursor().left() - x0;
        // what shows of the row (a dialog whose body is wider than its window clips it at the window's margin)
        let room = ui.available_width().min(ui.clip_rect().right() - 16.0 - ui.cursor().left());
        ui.add_space((room - was - 1.0).max(0.0));
        let a = ui.cursor().left();
        let r = right(ui);
        let w = ui.cursor().left() - a - ui.spacing().item_spacing.x;
        if (w - was).abs() > 0.5 {
            ui.ctx().data_mut(|d| d.insert_temp(id, w));
            ui.ctx().request_discard("a dialog foot's buttons measured");
        }
        let _ = used;
        r
    })
    .inner
}

macro_rules! forward {
    ($($name:ident($($arg:ident: $ty:ty),*)),* $(,)?) => {
        impl<'a> Dialog<'a> {
            $(pub fn $name(mut self, $($arg: $ty),*) -> Self {
                self.win = self.win.$name($($arg),*);
                self
            })*
        }
    };
}

impl<'a> Dialog<'a> {
    pub fn default_pos(mut self, p: impl Into<Pos2>) -> Self {
        self.win = self.win.default_pos(p);
        self.placed = true;
        self
    }
    pub fn current_pos(mut self, p: impl Into<Pos2>) -> Self {
        self.win = self.win.current_pos(p);
        self.placed = true;
        self
    }
    pub fn fixed_pos(mut self, p: impl Into<Pos2>) -> Self {
        self.win = self.win.fixed_pos(p);
        self.placed = true;
        self
    }
    pub fn default_rect(mut self, r: Rect) -> Self {
        self.win = self.win.default_rect(r);
        self.placed = true;
        self
    }
    pub fn anchor(mut self, a: egui::Align2, o: impl Into<Vec2>) -> Self {
        self.win = self.win.anchor(a, o);
        self.placed = true;
        self
    }
    pub fn pivot(mut self, a: egui::Align2) -> Self {
        self.win = self.win.pivot(a);
        self.placed_pivot = true;
        self
    }
}

forward!(
    id(id: egui::Id),
    resizable(on: impl Into<egui::Vec2b>),
    default_width(w: f32),
    default_height(h: f32),
    default_size(s: impl Into<Vec2>),
    fixed_size(s: impl Into<Vec2>),
    min_width(w: f32),
    min_height(h: f32),
    max_width(w: f32),
    max_height(h: f32),
    constrain(on: bool),
    vscroll(on: bool),
    hscroll(on: bool),
    order(o: egui::Order),
    auto_sized(),
    frame(f: egui::Frame),
);

#[cfg(test)]
mod tests {
    use super::*;

    /// item 12 of docs/gaps.md: panel fields and selects fill their row to its right edge and not past it (past it, a
    /// dock panel grows frame after frame).
    #[test]
    fn filled_fields_and_selects_end_at_the_column_edge() {
        let ctx = egui::Context::default();
        crate::theme::Tokens::preset(crate::theme::Preset::Light).apply(&ctx, false);
        let got = std::cell::RefCell::new(Vec::new());
        for _ in 0..2 {
            got.borrow_mut().clear();
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.allocate_ui(Vec2::new(300.0, 300.0), |ui| {
                    ui.set_width(300.0);
                    let right = ui.max_rect().right();
                    ui.horizontal(|ui| {
                        ui.add_sized([112.0, 18.0], egui::Label::new("Position"));
                        let w = width_in(ui, FILL);
                        let text = "Centered on the line, a long choice";
                        let r = egui::ComboBox::from_id_salt("p").selected_text(text).width(w).truncate().show_ui(ui, |_| {});
                        got.borrow_mut().push((right, r.response.rect.right()));
                    });
                    ui.horizontal(|ui| {
                        ui.add_sized([112.0, 18.0], egui::Label::new("Name"));
                        let mut s = String::from("x");
                        let r = ui.add(field(&mut s).desired_width(width_in(ui, FILL)));
                        got.borrow_mut().push((right, r.rect.right()));
                    });
                    got.borrow_mut().push((right, ui.min_rect().right()));
                });
            });
            out.textures_delta.clear();
        }
        for (right, at) in got.borrow().iter() {
            assert!((at - right).abs() <= 0.5, "ends at {at}, the column at {right}: {:?}", got.borrow());
        }
    }

    /// item 12 of docs/gaps.md: a dialog foot's buttons sit against the right edge in the very first frame (the pass is
    /// laid out again once their width is known), Cancel then the main action, 12 px apart.
    #[test]
    fn a_dialog_foot_reaches_the_right_edge_in_its_first_frame() {
        let ctx = egui::Context::default();
        let got = std::cell::Cell::new((0.0f32, Rect::NOTHING, Rect::NOTHING));
        let input = || egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))), ..Default::default() };
        let mut out = ctx.run_ui(input(), |ui| {
            ui.allocate_ui(Vec2::new(400.0, 200.0), |ui| {
                ui.set_width(400.0);
                let right = ui.max_rect().right();
                let (a, b) = foot(ui, |ui| (ui.button("Cancel").rect, ui.button("Plot").rect));
                got.set((right, a, b));
            });
        });
        out.textures_delta.clear();
        let (right, cancel, main) = got.get();
        assert!((main.right() - right).abs() <= 1.5, "main action at {} of {right}", main.right());
        assert!(cancel.right() < main.left());
        assert!((main.left() - cancel.right() - 12.0).abs() < 0.5);
    }
}
