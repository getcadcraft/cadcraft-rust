//! "Recover your work?" (0.5 app-autosave.js `recoverDialog`): the copies windows that stopped left behind, each with
//! its name, when it was last kept and its size, Recover and Discard; Discard all and Not now below. Enter recovers the
//! first copy, Esc is Not now. Every button runs a `recovery.*` command (recover.rs).

use egui::RichText;

use crate::recover::{self, st};
use crate::{Params, UiApp};

/// Draw the dialog when it is open.
pub fn show(app: &mut UiApp, ctx: &egui::Context) {
    let Some(d) = app.recovery.dlg.as_ref() else { return };
    let tok = app.tokens;
    let t = app.recovery.clock.unwrap_or_else(cc_clock::now_ms);
    let zone = app.zone;
    let title = st("shell.autosave.title", &[]);
    let lede = recover::lede(d.n);
    let hint = recover::hint(app);
    let rows: Vec<(String, String, Option<String>, bool)> = d
        .rows
        .iter()
        .map(|r| (recover::row_name(&r.rec), recover::row_meta(&r.rec, t, zone), r.msg.clone(), r.gone_at.is_none()))
        .collect();
    let mut act: Option<(&'static str, usize)> = None;
    let mut open = true;
    egui::Window::new(RichText::new(title).strong().color(tok.text_strong))
        .id(egui::Id::unique("recover_dlg"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(460.0)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
            ui.label(RichText::new(lede).color(tok.text));
            ui.add_space(4.0);
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                let mut live = 0usize;
                for (name, meta, msg, alive) in &rows {
                    egui::Frame::new()
                        .fill(tok.panel_darker)
                        .stroke(egui::Stroke::new(1.0, tok.divider))
                        .corner_radius(tok.radius)
                        .inner_margin(egui::Margin::symmetric(8, 6))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(name).strong().color(tok.text_strong));
                                    ui.label(RichText::new(meta).small().color(tok.text_dim));
                                });
                                if *alive {
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.button(st("shell.autosave.discard", &[])).clicked() {
                                            act = Some(("recovery.discard", live));
                                        }
                                        let rb = egui::Button::new(
                                            RichText::new(format!("↺ {}", st("shell.autosave.recover", &[]))).color(tok.text_strong),
                                        )
                                        .fill(tok.accent);
                                        if ui.add(rb).clicked() {
                                            act = Some(("recovery.recover", live));
                                        }
                                    });
                                }
                            });
                            if let Some(m) = msg {
                                ui.label(RichText::new(m).small().color(tok.text));
                            }
                        });
                    if *alive {
                        live = live.saturating_add(1);
                    }
                    ui.add_space(2.0);
                }
            });
            ui.add_space(4.0);
            ui.label(RichText::new(hint).small().color(tok.text_dim));
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(st("shell.autosave.discard_all", &[])).clicked() {
                    act = Some(("recovery.discard_all", 0));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let later = egui::Button::new(RichText::new(st("shell.autosave.later", &[])).color(tok.text_strong)).fill(tok.accent);
                    if ui.add(later).clicked() {
                        act = Some(("recovery.later", 0));
                    }
                });
            });
        });
    // the dialog takes Enter and Esc before the command line sees them
    let esc = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    let enter = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    if !open || esc {
        act = Some(("recovery.later", 0));
    } else if act.is_none() && enter && rows.iter().any(|r| r.3) {
        act = Some(("recovery.recover", 0));
    }
    if let Some((id, index)) = act {
        app.run_ui_pub(id, &Params { index: Some(index), ..Default::default() });
    }
}
