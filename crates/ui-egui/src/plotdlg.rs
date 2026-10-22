//! CadCraft's Plot dialog (cadcraft.js plotDialog over plot.js, ported in cc-io-cad `plot`): a sheet at full size, or
//! the model (everything, what is on screen, or a picked window) on a paper, landscape or portrait, at a standard scale
//! or fitted, black or in the drawing's colours, with a pen width; a preview of the sheet; Save PDF writes the vector
//! PDF where the person chooses (the browser build downloads it) and prints the 0.5 line ("Plotted … (… bytes, … mm)").
//! Every control is a `plot.*` command, so the CLI and assistants reach the same plot.
//!
//! Pictures plot at their own resolution or capped at 600 / 300 / 150 dpi (PNGs shrunk, JPEGs kept and said so). With
//! two or more sheets, Publish closes the dialog and runs PUBLISH for every sheet with the dialog's ink and pen.
//!
//! Open to print shows the PDF in the system's PDF viewer (the web build: a new tab, downloaded when the tab is blocked).

use cc_io_cad::plot::{PAPERS, PEN_WIDTHS, display_list, extents_of, layout_sheet, model_sheet, paper_label, scale_list, to_pdf_dpi};
use egui::{Color32, RichText};

use crate::{Params, UiApp};

/// The dialog's settings (cadcraft.js plotPrefs + plotState.win).
pub struct PlotDlg {
    pub open: bool,
    /// "extents" | "display" | "window"
    pub area: String,
    pub win: Option<cc_cad_geom::geom::Bounds>,
    pub paper: String,
    pub portrait: bool,
    /// 1:n, or None = fit to paper
    pub scale: Option<f64>,
    pub mono: bool,
    pub pen: f64,
    /// pictures: 0 = their own resolution, else a cap in dpi on paper (600, 300, 150)
    pub dpi: u32,
    /// the last result line
    pub status: String,
    /// paper chosen by the person (else the drawing's default: letter for inches / feet, ISO A4 otherwise)
    paper_set: bool,
}
impl Default for PlotDlg {
    fn default() -> Self {
        PlotDlg {
            open: false,
            area: "extents".into(),
            win: None,
            paper: "iso-a4".into(),
            portrait: false,
            scale: None,
            mono: true,
            pen: 0.25,
            dpi: 0,
            status: String::new(),
            paper_set: false,
        }
    }
}

/// UI commands of the dialog.
pub const COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("plot.open", "Plot to PDF", "Ctrl+P", "{} → opens the Plot dialog (the PLOT command does the same)"),
    ("plot.close", "Close the Plot dialog", "", "{}"),
    (
        "plot.set",
        "Plot: set an option",
        "",
        "{name: \"area=extents|display|window\" | \"paper=<key>\" | \"orient=landscape|portrait\" | \"scale=<n>|\" (1:n, empty = fit) | \"ink=mono|color\" | \"pen=<mm>\" | \"dpi=0|600|300|150\" (pictures; 0 = their own resolution) | \"window=minX,minY,maxX,maxY\"}",
    ),
    ("plot.save", "Plot: save the PDF", "", "{name?: file name} → plots with the dialog's settings and saves the PDF"),
    (
        "plot.print",
        "Open to print",
        "",
        "{} → plots with the dialog's settings and opens the PDF to print (desktop: the PDF viewer; web: a new tab), then closes the dialog",
    ),
    ("plot.publish", "Plot: publish every sheet", "", "{} → closes the dialog and runs PUBLISH (all sheets, one PDF) with its ink and pen"),
];

fn t(k: &str) -> String {
    crate::edit::lang_text(k, &[])
}
fn tv(k: &str, vars: &[(&str, String)]) -> String {
    crate::edit::lang_text(k, vars)
}
fn fmt(n: f64) -> String {
    cc_engine::input::fmt(n)
}

/// `plot.*` commands.
pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    match id {
        "plot.open" => {
            app.editor.as_ref().ok_or("plot.open: no drawing is open for editing")?;
            open(app);
        }
        "plot.close" => app.plot.open = false,
        "plot.set" => {
            let spec = params.name.clone().ok_or("plot.set: name = \"<option>=<value>\" is required")?;
            let (k, v) = spec.split_once('=').ok_or("plot.set: name = \"<option>=<value>\"")?;
            let p = &mut app.plot;
            match k {
                "area" if matches!(v, "extents" | "display" | "window") => {
                    p.area = v.into();
                    if v != "window" {
                        p.win = None;
                    }
                }
                "paper" if cc_io_cad::plot::paper_by_key(v).is_some() => {
                    p.paper = v.into();
                    p.paper_set = true;
                }
                "orient" if matches!(v, "landscape" | "portrait") => p.portrait = v == "portrait",
                "scale" => {
                    p.scale = if v.is_empty() {
                        None
                    } else {
                        Some(v.parse::<f64>().ok().filter(|n| *n > 0.0).ok_or("plot.set: scale = a number > 0, or empty (fit)")?)
                    }
                }
                "ink" if matches!(v, "mono" | "color") => p.mono = v == "mono",
                "dpi" if matches!(v, "0" | "600" | "300" | "150") => p.dpi = v.parse().unwrap_or(0),
                "pen" => p.pen = v.parse::<f64>().ok().filter(|w| (0.05..=2.0).contains(w)).ok_or("plot.set: pen = 0.05 … 2 mm")?,
                "window" => {
                    let n: Vec<f64> = v.split(',').filter_map(|x| x.trim().parse::<f64>().ok()).collect();
                    let [a, b, c, d] = n.as_slice() else { return Err("plot.set: window = minX,minY,maxX,maxY".into()) };
                    if !((a - c).abs() > 1e-9 && (b - d).abs() > 1e-9) {
                        return Err("plot.set: the window has no area".into());
                    }
                    p.win = Some(cc_cad_geom::geom::Bounds { min_x: a.min(*c), min_y: b.min(*d), max_x: a.max(*c), max_y: b.max(*d) });
                    p.area = "window".into();
                }
                _ => return Err(format!("plot.set: {k} = {v} is not an option")),
            }
        }
        "plot.save" => save(app, params.name.as_deref())?,
        "plot.print" => open_print(app)?,
        "plot.publish" => {
            app.editor.as_ref().ok_or("plot.publish: no drawing is open for editing")?;
            // cadcraft.js: the dialog closes (PLOT ends), then PUBLISH runs; Enter answers "all sheets"
            app.plot.open = false;
            app.run_ui_pub("cmd.line", &Params { name: Some("PUBLISH".into()), ..Default::default() });
            let asking = app.editor.as_ref().is_some_and(|e| e.eng.running() == Some("PUBLISH") && e.eng.expects().is_some());
            if asking {
                app.run_ui_pub("cmd.line", &Params { name: Some(String::new()), ..Default::default() });
            }
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

fn open(app: &mut UiApp) {
    let units = app
        .editor
        .as_ref()
        .map(|e| e.eng.doc().meta.get("units").and_then(serde_json::Value::as_str).unwrap_or("").to_string())
        .unwrap_or_default();
    let p = &mut app.plot;
    if !p.paper_set {
        p.paper = if matches!(units.as_str(), "inches" | "feet") { "letter".into() } else { "iso-a4".into() };
    }
    p.status.clear();
    p.open = true;
}

/// The sheet the dialog would plot, and the line that says what it is (cadcraft.js currentSheet).
fn current_sheet(app: &UiApp) -> (Option<cc_io_cad::plot::Sheet>, String) {
    let Some(ed) = app.editor.as_ref() else { return (None, String::new()) };
    let doc = ed.eng.doc();
    if let Some(lay) = doc.layout() {
        return (Some(layout_sheet(&doc, lay)), String::new());
    }
    let p = &app.plot;
    let area = match p.area.as_str() {
        "window" => p.win,
        "display" => ed.hs.borrow().view_box,
        _ => extents_of(&doc),
    };
    let Some(area) = area else {
        return (None, t(if p.area == "window" { "app.pick_window_plot" } else { "app.there_nothing_plot_yet" }));
    };
    let sheet = model_sheet(&doc, &area, &p.paper, p.portrait, p.scale);
    let info = match &sheet.scale {
        Some(sc) => {
            let mm = |v: f64| fmt(cc_cad::js::round(v * 10.0) / 10.0);
            tv(
                if sc.fits { "app.plot.area" } else { "app.plot.area.toobig" },
                &[("scale", sc.label.clone()), ("w", mm((area.max_x - area.min_x) * sc.s)), ("h", mm((area.max_y - area.min_y) * sc.s))],
            )
        }
        None => String::new(),
    };
    (Some(sheet), info)
}

/// Plot with the dialog's settings and save the PDF (`name` = the file name; default the drawing's name).
pub fn save(app: &mut UiApp, name: Option<&str>) -> Result<(), String> {
    out(app, name, false)
}

/// "Open to print": plot and show the PDF where it can be printed (cadcraft.js plotUi.open: the browser's new tab; the
/// desktop's PDF viewer), then close the dialog.
pub fn open_print(app: &mut UiApp) -> Result<(), String> {
    out(app, None, true)
}

fn out(app: &mut UiApp, name: Option<&str>, to_print: bool) -> Result<(), String> {
    let (sheet, info) = current_sheet(app);
    let sheet = sheet.ok_or(if info.is_empty() { "There is nothing to plot.".to_string() } else { info })?;
    let ed = app.editor.as_ref().ok_or("plot.save: no drawing is open for editing")?;
    let (bytes, list_w, list_h, title, scale_label, missing, pics) = {
        let doc = ed.eng.doc();
        let list = display_list(&doc, &sheet, app.plot.mono);
        let dname = match doc.meta.get("name") {
            Some(v) if cc_cad::js::truthy(Some(v)) => cc_cad::js::to_js_string(v),
            _ => "Drawing1".into(),
        };
        let lay = doc.layout().map(|l| l.name.clone());
        let title = match &lay {
            Some(l) => format!("{dname} – {l}"),
            None => dname.clone(),
        };
        let date = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() * 1000.0).floor();
        let pdf = to_pdf_dpi(&list, app.plot.pen, &title, date, true, f64::from(app.plot.dpi));
        let file = format!("{dname}{}.pdf", lay.map(|l| format!("-{l}")).unwrap_or_default());
        (pdf.bytes, list.w, list.h, file, sheet.scale.as_ref().map(|s| s.label.clone()), pdf.missing, (pdf.pictures, pdf.shrunk, pdf.kept))
    };
    let mut clean: String = name
        .map_or(title, str::to_string)
        .chars()
        .map(|c| if matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect();
    if !clean.to_ascii_lowercase().ends_with(".pdf") {
        clean.push_str(".pdf");
    }
    let n = bytes.len();
    let mut shown = None;
    if to_print {
        let open = app.services.open_pdf.as_mut().ok_or("Opening a PDF to print is not available here.")?;
        shown = Some(open(&clean, &bytes)?);
    } else {
        let saved = match app.services.save.as_mut() {
            Some(save) => save(&clean, bytes)?,
            None => return Err("Saving is not available here.".into()),
        };
        if saved.is_none() {
            app.plot.status = "Not saved.".into();
            return Ok(());
        }
    }
    let mut vars = vec![("name", clean.clone()), ("bytes", group3(n)), ("w", fmt(list_w)), ("h", fmt(list_h))];
    let key = match scale_label {
        Some(s) => {
            vars.push(("scale", s));
            "app.plot.done.scale"
        }
        None => "app.plot.done",
    };
    let line = tv(key, &vars);
    app.plot.status = line.clone();
    if let Some(ed) = app.editor.as_mut() {
        let mut hs = ed.hs.borrow_mut();
        hs.log.push_back(line);
        if missing > 0 {
            hs.log.push_back(format!("  {}", tv("app.plot.missing_chars", &[("n", missing.to_string())])));
        }
        let (n, shrunk, kept) = pics;
        if n > 0 {
            let cap = if shrunk > 0 { app.plot.dpi.to_string() } else { String::new() };
            hs.log.push_back(format!("  {}", tv("app.plot.pictures", &[("n", n.to_string()), ("cap", cap)])));
        }
        if kept > 0 {
            hs.log.push_back(format!("  {}", tv("app.plot.pictures_jpeg", &[("n", kept.to_string())])));
        }
        match shown {
            // the web build's new tab says 0.5's words; the desktop's viewer its own
            Some(true) if cfg!(target_arch = "wasm32") => hs.log.push_back(t("app.pdf_opened_new_tab")),
            Some(true) => hs.log.push_back("  The PDF opened in your PDF viewer: print it from there.".into()),
            Some(false) => hs.log.push_back(t("app.browser_blocked_new_tab")),
            None => {}
        }
    }
    if shown.is_some() {
        app.plot.open = false;
    }
    Ok(())
}

/// `n.toLocaleString('en-US')`.
fn group3(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// The dialog window.
pub fn window(app: &mut UiApp, ctx: &egui::Context) {
    if !app.plot.open || app.editor.is_none() {
        return;
    }
    let tok = app.tokens;
    let (sheet, info) = current_sheet(app);
    let on_sheet = app.editor.as_ref().is_some_and(|e| e.eng.doc().layout().is_some());
    let mut act: Vec<(String, String)> = Vec::new();
    let mut do_save = false;
    let mut do_print = false;
    let mut publish = false;
    let sheets = app.editor.as_ref().map_or(0, |e| e.eng.doc().layouts.len());
    let mut close = false;
    let mut pick = false;
    let mut open = true;
    egui::Window::new(t("ui.plot_pdf"))
        .open(&mut open)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .collapsible(false)
        .show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(230.0);
                    if on_sheet {
                        let lay = app.editor.as_ref().and_then(|e| e.eng.doc().layout().map(|l| (l.name.clone(), l.paper.clone())));
                        if let Some((name, paper)) = lay {
                            let key = paper.get("key").and_then(serde_json::Value::as_str).unwrap_or("");
                            let label = cc_io_cad::plot::paper_by_key(key).map_or_else(
                                || {
                                    let n = |k: &str| paper.get(k).and_then(serde_json::Value::as_f64).unwrap_or(0.0);
                                    format!(
                                        "{} × {} {}",
                                        fmt(n("w")),
                                        fmt(n("h")),
                                        paper.get("unit").and_then(serde_json::Value::as_str).unwrap_or("")
                                    )
                                },
                                |p| paper_label(p, &t),
                            );
                            ui.label(RichText::new(tv("app.plot.sheet", &[("name", name), ("paper", label)])).color(tok.text_dim));
                        }
                    } else {
                        ui.label(RichText::new(t("app.plots_model_onto_sheet")).color(tok.text_dim));
                        let p = &app.plot;
                        egui::Grid::new("plot_opts").num_columns(2).show(ui, |ui| {
                            ui.label(t("ui.area"));
                            ui.horizontal(|ui| {
                                let areas = [
                                    ("extents", t("ui.everything_extents")),
                                    ("display", t("ui.what_screen")),
                                    ("window", t("ui.a_window")),
                                ];
                                let cur = areas.iter().find(|(k, _)| *k == p.area).map(|(_, l)| l.clone()).unwrap_or_default();
                                egui::ComboBox::new("plot_area", "").selected_text(cur).show_ui(ui, |ui| {
                                    for (k, l) in &areas {
                                        if ui.selectable_label(*k == p.area, l).clicked() {
                                            act.push(("area".into(), (*k).into()));
                                        }
                                    }
                                });
                                if p.area == "window" && ui.small_button(t("ui.pick")).clicked() {
                                    pick = true;
                                }
                            });
                            ui.end_row();
                            ui.label(t("ui.paper"));
                            let cur = cc_io_cad::plot::paper_by_key(&p.paper).map(|x| paper_label(x, &t)).unwrap_or_default();
                            egui::ComboBox::new("plot_paper", "").selected_text(cur).show_ui(ui, |ui| {
                                for x in &PAPERS {
                                    if ui.selectable_label(x.key == p.paper, paper_label(x, &t)).clicked() {
                                        act.push(("paper".into(), x.key.into()));
                                    }
                                }
                            });
                            ui.end_row();
                            ui.label(t("ui.orientation"));
                            egui::ComboBox::new("plot_orient", "")
                                .selected_text(t(if p.portrait { "ui.portrait" } else { "ui.landscape" }))
                                .show_ui(ui, |ui| {
                                    if ui.selectable_label(!p.portrait, t("ui.landscape")).clicked() {
                                        act.push(("orient".into(), "landscape".into()));
                                    }
                                    if ui.selectable_label(p.portrait, t("ui.portrait")).clicked() {
                                        act.push(("orient".into(), "portrait".into()));
                                    }
                                });
                            ui.end_row();
                            ui.label(t("ui.scale"));
                            let list = app.editor.as_ref().map(|e| scale_list(&e.eng.doc())).unwrap_or_default();
                            let cur = match p.scale {
                                None => t("app.fit_paper"),
                                Some(n) => list.iter().find(|x| x.n == n).map_or_else(|| format!("1:{}", fmt(n)), |x| x.label.clone()),
                            };
                            egui::ComboBox::new("plot_scale", "").selected_text(cur).show_ui(ui, |ui| {
                                if ui.selectable_label(p.scale.is_none(), t("app.fit_paper")).clicked() {
                                    act.push(("scale".into(), String::new()));
                                }
                                for x in &list {
                                    if ui.selectable_label(p.scale == Some(x.n), &x.label).clicked() {
                                        act.push(("scale".into(), cc_cad::js::num_to_string(x.n)));
                                    }
                                }
                            });
                            ui.end_row();
                        });
                    }
                    let p = &app.plot;
                    egui::Grid::new("plot_ink").num_columns(2).show(ui, |ui| {
                        ui.label(t("ui.ink"));
                        egui::ComboBox::new("plot_inkc", "")
                            .selected_text(t(if p.mono { "ui.black_monochrome" } else { "ui.drawing_colours" }))
                            .show_ui(ui, |ui| {
                                if ui.selectable_label(p.mono, t("ui.black_monochrome")).clicked() {
                                    act.push(("ink".into(), "mono".into()));
                                }
                                if ui.selectable_label(!p.mono, t("ui.drawing_colours")).clicked() {
                                    act.push(("ink".into(), "color".into()));
                                }
                            });
                        ui.end_row();
                        ui.label(t("ui.line_weight"));
                        egui::ComboBox::new("plot_pen", "").selected_text(format!("{} mm", cc_cad::js::to_fixed(p.pen, 2))).show_ui(
                            ui,
                            |ui| {
                                for w in PEN_WIDTHS {
                                    if ui.selectable_label(p.pen == w, format!("{} mm", cc_cad::js::to_fixed(w, 2))).clicked() {
                                        act.push(("pen".into(), cc_cad::js::num_to_string(w)));
                                    }
                                }
                            },
                        );
                        ui.end_row();
                        ui.label(t("ui.plot.pictures"));
                        let dpi_label =
                            |d: u32| if d == 0 { t("ui.plot.pictures.own") } else { tv("ui.plot.dpi", &[("n", d.to_string())]) };
                        egui::ComboBox::new("plot_dpi", "").selected_text(dpi_label(p.dpi)).show_ui(ui, |ui| {
                            for d in [0u32, 600, 300, 150] {
                                if ui.selectable_label(p.dpi == d, dpi_label(d)).clicked() {
                                    act.push(("dpi".into(), d.to_string()));
                                }
                            }
                        });
                        ui.end_row();
                    });
                    if !info.is_empty() {
                        ui.label(RichText::new(&info).color(tok.text_dim));
                    }
                    if !p.status.is_empty() {
                        ui.label(RichText::new(&p.status).color(tok.text));
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if sheets >= 2 && ui.button(t("plot.publish")).on_hover_text(t("plot.publish.title")).clicked() {
                            publish = true;
                        }
                        if ui.add_enabled(sheet.is_some(), egui::Button::new(t("ui.download_pdf"))).clicked() {
                            do_save = true;
                        }
                        if ui.add_enabled(sheet.is_some(), egui::Button::new(t("ui.open_print"))).clicked() {
                            do_print = true;
                        }
                        if ui.button(t("ui.close")).clicked() {
                            close = true;
                        }
                    });
                });
                // the preview: the sheet in white with the plotted lines, as tall as the paper's shape asks (the
                // dialog is as tall as its contents, no taller)
                let list = match (sheet.as_ref(), app.editor.as_ref()) {
                    (Some(sheet), Some(ed)) => Some(display_list(&ed.eng.doc(), sheet, app.plot.mono)),
                    _ => None,
                };
                let pw = 360.0_f32;
                let ph = list
                    .as_ref()
                    .filter(|l| l.w > 0.0 && l.h > 0.0)
                    .map_or(220.0, |l| ((f64::from(pw) - 24.0) * l.h / l.w + 24.0) as f32)
                    .clamp(160.0, 380.0);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(pw, ph), egui::Sense::hover());
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 0.0, tok.panel_darker);
                if let Some(list) = list.as_ref() {
                    preview(&painter, rect, list, app.plot.pen);
                }
            });
        });
    if !open || close {
        app.plot.open = false;
    }
    for (k, v) in act {
        app.run_ui_pub("plot.set", &Params { name: Some(format!("{k}={v}")), ..Default::default() });
    }
    if pick {
        // the dialog steps aside; _PLOTWIN asks the two corners, then opens it again
        app.plot.open = false;
        app.run_ui_pub("cmd.line", &Params { name: Some("_PLOTWIN".into()), ..Default::default() });
    }
    if do_save {
        app.run_ui_pub("plot.save", &Params::default());
    }
    if do_print {
        app.run_ui_pub("plot.print", &Params::default());
    }
    if publish {
        app.run_ui_pub("plot.publish", &Params::default());
    }
}

fn hex(c: &str) -> Color32 {
    let h = c.trim_start_matches('#');
    let v = u32::from_str_radix(h.get(..6).unwrap_or("000000"), 16).unwrap_or(0);
    Color32::from_rgb(((v >> 16) & 255) as u8, ((v >> 8) & 255) as u8, (v & 255) as u8)
}

/// drawPreview on the egui painter: fills, strokes (arcs as short pieces), points; text as its baseline.
fn preview(painter: &egui::Painter, rect: egui::Rect, list: &cc_io_cad::plot::DisplayList, pen: f64) {
    if !(list.w > 0.0 && list.h > 0.0) {
        return;
    }
    let pad = 12.0;
    let k = ((f64::from(rect.width()) - 2.0 * pad) / list.w).min((f64::from(rect.height()) - 2.0 * pad) / list.h);
    if k.is_nan() || k <= 0.0 {
        return;
    }
    let ox = f64::from(rect.min.x) + (f64::from(rect.width()) - list.w * k) / 2.0;
    let oy = f64::from(rect.min.y) + (f64::from(rect.height()) - list.h * k) / 2.0;
    let to = |x: f64, y: f64| egui::pos2((ox + x * k) as f32, (oy + (list.h - y) * k) as f32);
    let paper = egui::Rect::from_min_max(to(0.0, list.h), to(list.w, 0.0));
    painter.rect_filled(paper, 0.0, Color32::WHITE);
    let clip = painter.with_clip_rect(paper);
    for v in &list.views {
        let vp = match &v.clip {
            Some(c) => {
                let (mut a, mut b) = (egui::pos2(f32::MAX, f32::MAX), egui::pos2(f32::MIN, f32::MIN));
                for q in c {
                    let p = to(q.x, q.y);
                    a = a.min(p);
                    b = b.max(p);
                }
                clip.with_clip_rect(egui::Rect::from_min_max(a, b))
            }
            None => clip.clone(),
        };
        for f in &v.fills {
            for l in &f.loops {
                let pts: Vec<egui::Pos2> = l.as_chunks::<2>().0.iter().map(|[x, y]| to(*x, *y)).collect();
                if pts.len() >= 3 {
                    vp.add(egui::Shape::convex_polygon(pts, hex(&f.color).gamma_multiply(0.5), egui::Stroke::NONE));
                }
            }
        }
        for s in &v.strokes {
            let w = ((if s.lw >= 0.0 { s.lw.max(0.05) } else { pen }) * k).max(0.6) as f32;
            let st = egui::Stroke::new(w, hex(&s.color));
            for q in &s.prims {
                if q.is_l() {
                    vp.line_segment([to(q.x1, q.y1), to(q.x2, q.y2)], st);
                } else {
                    let (a0, a1) = if q.rev() { (q.a1, q.a0) } else { (q.a0, q.a1) };
                    let n = (((a1 - a0).abs() / 0.15).ceil() as usize).clamp(2, 256);
                    let pts: Vec<egui::Pos2> = (0..=n)
                        .map(|i| {
                            let a = a0 + (a1 - a0) * (i as f64) / (n as f64);
                            to(q.cx + q.r * a.cos(), q.cy + q.r * a.sin())
                        })
                        .collect();
                    vp.add(egui::Shape::line(pts, st));
                }
            }
        }
        for (c, x, y) in &v.points {
            vp.circle_filled(to(*x, *y), 1.0, hex(c));
        }
        for tx in &v.texts {
            if let cc_cad::Geo::Text(tt) = &tx.e.geo {
                let px = (tt.h * k) as f32;
                if px > 3.0 {
                    let p = to(tt.x, tt.y);
                    vp.text(
                        p - egui::vec2(0.0, px),
                        egui::Align2::LEFT_TOP,
                        tt.s.lines().next().unwrap_or(""),
                        egui::FontId::proportional(px * 1.2),
                        hex(&tx.color),
                    );
                }
            }
        }
    }
    painter.rect_stroke(paper, 0.0, egui::Stroke::new(1.0, Color32::from_gray(0x55)), egui::StrokeKind::Middle);
}
