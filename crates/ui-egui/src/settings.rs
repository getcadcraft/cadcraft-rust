//! Settings (0.5's shell Settings dialog, app-settings.js, and Studio's): the language of every app, the theme, the
//! panel text size. Words: the shell catalog (`shell.settings.*`) in an app, Studio's (`settings.*`) on the start
//! screen. Every change is a command (`settings.open`, `settings.set {name: "lang=es-MX" |
//! "theme=paper|model|system|medium-dark|medium-light|contrast" | "zoom=1.15"}`, `settings.close`), so it works headless; the hosts keep [`Settings`] between runs
//! ([`UiApp::settings_json`] / [`UiApp::load_settings`]: `settings.json` on the desktop, the browser's storage on the
//! web).

use egui::RichText;
use serde_json::{Value, json};

use crate::lang::{SHELL, STUDIO, t0};
use crate::theme::Preset;
use crate::{Params, UiApp};

/// The panel text sizes 0.5 offers (Settings › Panel text size), as zoom factors. Spec gap: the PHP page's list is
/// not in the frozen build; these follow its labels (tiny … largest) within its 0.5–2 range.
pub const ZOOMS: [(&str, f32); 7] =
    [("tiny", 0.75), ("small", 0.85), ("compact", 0.9), ("normal", 1.0), ("large", 1.15), ("larger", 1.3), ("largest", 1.5)];

/// What Settings keeps.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// "model" (dark), "paper" (light), "system"
    pub theme: String,
    pub zoom: f32,
    /// the dialog is open
    pub open: bool,
    /// the dialog's tab (0.7 `.shell-tabs`: the app's settings, or the global ones) and the section shown in it
    pub global: bool,
    pub sec: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { theme: "model".into(), zoom: 1.0, open: false, global: false, sec: String::new() }
    }
}

/// The global tab's sections (the shell's words): the language, the theme, the panel text size.
const GLOBAL_SECTIONS: [&str; 3] = ["shell.settings.lang", "shell.settings.theme", "shell.settings.zoom"];

/// The app tab's sections for the app showing, (key, name): CadCraft's settings.inc fieldsets, My tools, the input
/// styles; ModelCraft's. The other apps keep their settings in their own windows (none here).
fn app_sections(app: &UiApp) -> Vec<(String, String)> {
    match app.active_app() {
        "cadcraft" => {
            let mut v: Vec<(String, String)> =
                crate::cadset::SECTIONS.iter().map(|k| ((*k).to_string(), crate::cadset::section_title(k))).collect();
            v.push(("mytools".into(), crate::mytoolsdlg::section_title()));
            v.push(("input".into(), crate::lang::shell("shell.input.legend", &[])));
            v
        }
        "modelcraft" => crate::mcset::SECTIONS.iter().map(|k| ((*k).to_string(), crate::mcset::section_title(k))).collect(),
        // settings.inc's Drawing fieldset: Snap and Snap to 45° (the others are not in this version)
        "takeoffcraft" => vec![("set.drawing".into(), cc_takeoff::i18n::t("set.drawing"))],
        _ => Vec::new(),
    }
}

pub const UI_COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("settings.open", "Settings", "", "{} → opens Settings (language, theme, panel text size)"),
    ("settings.close", "Settings: done", "", "{}"),
    (
        "settings.show",
        "Settings: show a section",
        "",
        "{name: \"app\" | \"global\" | \"app:<section>\" | \"global:<section>\"} → opens Settings on that tab (and section: set.cursor, mytools, input, shell.settings.theme …)",
    ),
    (
        "settings.reset",
        "Settings: reset the app's settings",
        "",
        "{} → the app's own settings back to their defaults (0.5's Reset <app> settings)",
    ),
    (
        "settings.set",
        "Settings: change one",
        "",
        "{name: \"lang=en|es-MX\" | \"theme=model|paper|system|medium-dark|medium-light|contrast\" | \"zoom=<0.5–2>\" | CadCraft's: \"cad.cursor=small|medium|large\" | \"cad.paste=lower-left|center\" | \"cad.pick=add|replace\" | \"cad.lasso=lasso|box\" | \"cad.corner=auto|cube|axes|none\" | \"cad.tabs=full|number\" | ModelCraft's: \"mc.keys=sketchup|blender\" | \"mc.scroll=auto|mouse|trackpad\" | \"mc.touch2=pan|orbit\" | \"mc.snap=true|false\" | \"mc.collide=true|false\" | \"mc.corner=cube|axes|none\"} → applies at once",
    ),
];

pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    match id {
        "settings.open" => {
            app.settings.open = true;
            Ok(())
        }
        "settings.close" => {
            app.settings.open = false;
            Ok(())
        }
        "settings.show" => {
            let spec = params.name.clone().unwrap_or_default();
            let (tab, sec) = spec.split_once(':').unwrap_or((spec.as_str(), ""));
            let global = match tab {
                "global" => true,
                "app" => false,
                _ => return Err(format!("settings.show: {tab} is not app or global")),
            };
            let list: Vec<String> = if global {
                GLOBAL_SECTIONS.iter().map(|k| (*k).to_string()).collect()
            } else {
                app_sections(app).into_iter().map(|x| x.0).collect()
            };
            if !sec.is_empty() && !list.iter().any(|k| k == sec) {
                return Err(format!("settings.show: no section {sec} (one of {})", list.join(", ")));
            }
            app.settings.open = true;
            app.settings.global = global;
            app.settings.sec = sec.to_string();
            Ok(())
        }
        "settings.reset" => {
            match app.active_app() {
                "cadcraft" => {
                    app.cadset.reset();
                    crate::cadset::push(app);
                }
                "modelcraft" => {
                    let d = crate::mcset::McSet::default();
                    if let Some(mc) = app.mc.as_mut() {
                        mc.set_keys(&d.keys);
                        mc.apply_settings(&d);
                    }
                    app.mcset = d;
                }
                "takeoffcraft" => {
                    app.tk_snap = (true, false);
                    app.seed_tk_snap();
                }
                _ => {}
            }
            Ok(())
        }
        "settings.set" => {
            let spec = params.name.clone().ok_or("settings.set: name = \"<setting>=<value>\" is required")?;
            let (k, v) = spec.split_once('=').ok_or("settings.set: name = \"<setting>=<value>\"")?;
            match k {
                "lang" => {
                    let l = cc_lang::Locale::parse(v).ok_or_else(|| format!("settings.set: lang {v} is not one of en, es-MX"))?;
                    app.set_locale(l);
                }
                "theme" => {
                    if v != "system" && Preset::from_setting(v).is_none() {
                        return Err(format!(
                            "settings.set: theme {v} is not one of model, paper, system, medium-dark, medium-light, contrast"
                        ));
                    }
                    app.settings.theme = v.to_string();
                    app.apply_theme_setting(None);
                }
                "zoom" => {
                    let z: f32 = v.trim().parse().map_err(|_| format!("settings.set: zoom {v} is not a number"))?;
                    if !(0.5..=2.0).contains(&z) {
                        return Err(format!("settings.set: zoom {v} is outside 0.5–2"));
                    }
                    app.settings.zoom = z;
                }
                _ if k.starts_with("mc.") => {
                    app.set_mc_setting(k.trim_start_matches("mc."), v)?;
                }
                _ if k.starts_with("cad.") => {
                    app.cadset.set(k.trim_start_matches("cad."), v)?;
                    crate::cadset::push(app);
                }
                _ => return Err(format!("settings.set: no setting {k} (lang, theme, zoom, cad.*, mc.*)")),
            }
            Ok(())
        }
        _ => Err(format!("unknown command {id}")),
    }
}

impl UiApp {
    /// Settings as the hosts keep them (`{"lang": "es-MX", "theme": "paper", "zoom": 1.15, "helpCtx": null}`; helpCtx as
    /// help-panel.js keeps it: null, "all" or {os, browser, input}).
    pub fn settings_json(&self) -> Value {
        json!({
            "lang": cc_lang::locale().code(),
            "theme": self.settings.theme,
            "zoom": self.settings.zoom,
            "cadcraft": self.cadset.json(),
            "modelcraft": self.mcset_now().json(),
            "cadprops": self.cadprops.prefs_json(),
            "cadcraftInput": self.ribbon.prefs(),
            "helpCtx": self.help.saved,
            "cadcraftMyTools": crate::mytoolsdlg::prefs(self),
            "detailcraft": self.detail.as_ref().map_or_else(|| self.dc_prefs.clone(), |d| d.settings.json()),
            "takeoffcraft": self.tk_prefs_json(),
        })
    }

    /// Settings read back (a damaged or foreign value keeps the default).
    pub fn load_settings(&mut self, v: &Value) {
        if let Some(l) = v.get("lang").and_then(Value::as_str).and_then(cc_lang::Locale::parse) {
            self.set_locale(l);
        }
        if let Some(th) = v.get("theme").and_then(Value::as_str).filter(|s| *s == "system" || Preset::from_setting(s).is_some()) {
            self.settings.theme = th.to_string();
            self.apply_theme_setting(None);
        }
        if let Some(z) = v.get("zoom").and_then(Value::as_f64).filter(|z| (0.5..=2.0).contains(z)) {
            self.settings.zoom = z as f32;
        }
        if let Some(c) = v.get("cadcraft") {
            self.cadset.load(c);
            // the open drawing takes the kept drafting toggles at the next frame
            if let Some(ed) = self.editor.as_mut() {
                ed.draft_seeded = false;
            }
        }
        if let Some(m) = v.get("modelcraft") {
            self.mcset.load(m);
            if let Some(mc) = self.mc.as_mut() {
                mc.apply_settings(&self.mcset);
            }
        }
        if let Some(p) = v.get("cadprops") {
            self.cadprops.load_prefs(p);
        }
        if let Some(p) = v.get("cadcraftInput") {
            self.ribbon.load_prefs(p);
        }
        // the help's reader context (help-panel.js's cadcraft.help.ctx)
        self.help.load_ctx(v.get("helpCtx").unwrap_or(&Value::Null));
        if let Some(p) = v.get("cadcraftMyTools") {
            crate::mytoolsdlg::load_prefs(self, p);
        }
        if let Some(p) = v.get("takeoffcraft").filter(|p| p.is_object()) {
            // takeoffcraft.js loadSettings: only booleans count
            let b = |k: &str, d: bool| p.get(k).and_then(Value::as_bool).unwrap_or(d);
            self.tk_snap = (b("snap", true), b("snap45", false));
            self.seed_tk_snap();
        }
        if let Some(p) = v.get("detailcraft").filter(|p| p.is_object()) {
            self.dc_prefs = p.clone();
            if let Some(d) = self.detail.as_mut() {
                let before = d.settings.clone();
                d.settings = crate::detailexport::Settings::load(p);
                d.settings_loaded(&before);
            }
        }
    }

    /// TakeoffCraft's kept settings (takeoffcraft.js's `snap`, `snap45`), from the open sheets when there are some.
    fn tk_prefs_json(&self) -> Value {
        let (snap, s45) = self.pdf.as_ref().map_or(self.tk_snap, |p| (!p.takeoff.snap.off, p.takeoff.snap.s45));
        json!({"snap": snap, "snap45": s45})
    }

    /// The kept Snap / Snap to 45° handed to the sheets just opened.
    pub(crate) fn seed_tk_snap(&mut self) {
        let (snap, s45) = self.tk_snap;
        if let Some(p) = self.pdf.as_mut() {
            p.takeoff.snap.off = !snap;
            p.takeoff.snap.s45 = s45;
        }
    }

    /// Show the app in a theme (the 3D model view is rebuilt in its colours).
    pub(crate) fn set_preset(&mut self, p: Preset) {
        self.preset = p;
        self.tokens = crate::theme::Tokens::preset(p);
        self.theme_dirty = true;
        if self.mc.is_none()
            && let Some(model) = self.model3.take()
        {
            let view = self.scene3.as_ref().map(|(_, v)| *v);
            self.show_model(model, "Model");
            if let (Some(v), Some((_, nv))) = (view, self.scene3.as_mut()) {
                let style = nv.style;
                *nv = v;
                nv.style = style;
            }
        }
    }

    /// The theme the setting asks for ("system": `dark` = the platform's, when it says).
    pub(crate) fn apply_theme_setting(&mut self, system_dark: Option<bool>) {
        let want = match self.settings.theme.as_str() {
            "system" => {
                if system_dark.unwrap_or(self.preset.is_dark()) {
                    Preset::Dark
                } else {
                    Preset::Light
                }
            }
            v => Preset::from_setting(v).unwrap_or(Preset::Dark),
        };
        if want != self.preset {
            self.set_preset(want);
        }
    }
}

/// The word for Settings' title and button in the app showing (Studio's own on the start screen).
pub fn title(app: &UiApp) -> String {
    match app.active_app() {
        "studio" => t0(&STUDIO, "settings.title"),
        slug => crate::lang::t(&SHELL, "shell.settings.title", &[("app", app_name(slug).into())]),
    }
}

fn app_name(slug: &str) -> &'static str {
    crate::hub::APPS.iter().find(|a| a.0 == slug).map_or("CadCraft", |a| a.1)
}

/// A tab of the Settings dialog (`.shell-tabs [role=tab]`).
fn settings_tab(ui: &mut egui::Ui, t: &crate::theme::Tokens, layer: egui::Color32, word: &str, on: bool) -> egui::Response {
    let ink = if on { t.text } else { t.text_dim };
    let g = ui.painter().layout_no_wrap(word.to_string(), crate::theme::semi(15.0), ink);
    let (r, resp) = ui.allocate_exact_size(egui::vec2(g.size().x + 26.0, 36.0), egui::Sense::click());
    if on {
        ui.painter().rect(
            r,
            egui::CornerRadius { nw: 2, ne: 2, sw: 0, se: 0 },
            t.panel,
            egui::Stroke::new(1.0, t.line),
            egui::StrokeKind::Inside,
        );
        ui.painter().rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(r.width(), 1.5)), 0.0, layer);
        // the open tab sits on the row's rule
        ui.painter().line_segment(
            [egui::pos2(r.left() + 1.0, r.bottom() - 0.5), egui::pos2(r.right() - 1.0, r.bottom() - 0.5)],
            egui::Stroke::new(1.5, t.panel),
        );
    }
    ui.painter().galley(egui::pos2(r.left() + 13.0, r.center().y - g.size().y / 2.0), g, ink);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The Settings window (drawn every frame while open).
pub fn window(app: &mut UiApp, ctx: &egui::Context) {
    if ctx.zoom_factor() != app.settings.zoom {
        ctx.set_zoom_factor(app.settings.zoom);
    }
    if app.settings.theme == "system" {
        let sys = ctx.system_theme().map(|th| th == egui::Theme::Dark);
        app.apply_theme_setting(sys);
    }
    if !app.settings.open {
        return;
    }
    let t = app.tokens;
    let s = |k: &str| t0(&SHELL, k);
    let mut open = true;
    let mut act: Option<String> = None;
    let mut input_act: Vec<(&'static str, Option<String>)> = Vec::new();
    let mut done = false;
    let mut reset = false;
    let theme_now = app.settings.theme.clone();
    let lang_now = cc_lang::locale();
    let zoom_now = app.settings.zoom;
    let slug = app.active_app();
    let app_secs = app_sections(app);
    // 0.7's sections dialog: the tabs (<App> | Global) over a list of sections at the left and the one picked at the
    // right; an app with no settings here has the global tab only
    let global = app.settings.global || app_secs.is_empty();
    let list: Vec<(String, String)> =
        if global { GLOBAL_SECTIONS.iter().map(|k| ((*k).to_string(), s(k))).collect() } else { app_secs.clone() };
    let sec = list.iter().find(|x| x.0 == app.settings.sec).or(list.first()).cloned().unwrap_or_default();
    let mut pick: Option<(bool, String)> = None;
    let layer = t.app(slug);
    let full = ctx.content_rect();
    let w = (full.width() - 32.0).clamp(320.0, 832.0);
    let h = (full.height() - 40.0).clamp(240.0, 760.0);
    crate::shell::Dialog::new(title(app))
        .shell_head()
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .fixed_size([w - 34.0, h - 24.0])
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(full.center())
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            // the tabs (`.shell-tabs`: 15 px semibold ink-2; the chosen one on sheet-2 in a line box, the app's colour
            // along its top; a 1 px line under the row)
            if !app_secs.is_empty() {
                let row = ui.max_rect();
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let app_word = if slug == "studio" { s("shell.settings.general") } else { app_name(slug).to_string() };
                    for (g, word) in [(false, app_word), (true, "Global".to_string())] {
                        if settings_tab(ui, &t, layer, &word, g == global).clicked() && g != global {
                            pick = Some((g, String::new()));
                        }
                    }
                });
                let y = ui.cursor().top() - 0.5;
                ui.painter()
                    .line_segment([egui::pos2(row.left() - 16.0, y), egui::pos2(row.right() + 16.0, y)], egui::Stroke::new(1.0, t.line));
            }
            let body_h = (ui.available_height() - 51.0).max(120.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                // the section list (`.shell-sec-nav`: 13rem on the sheet colour, a 1 px line at its right)
                let (nav, _) = ui.allocate_exact_size(egui::vec2(208.0, body_h), egui::Sense::hover());
                let nav = egui::Rect::from_min_max(egui::pos2(nav.left() - 16.0, nav.top()), nav.max);
                ui.painter().rect_filled(nav, 0.0, t.panel_darker);
                ui.painter().line_segment(
                    [egui::pos2(nav.right() - 0.5, nav.top()), egui::pos2(nav.right() - 0.5, nav.bottom())],
                    egui::Stroke::new(1.0, t.line),
                );
                let mut y = nav.top() + 10.0;
                for (k, name) in &list {
                    let g = ui.painter().layout(name.clone(), crate::theme::semi(14.0), t.text, nav.width() - 32.0);
                    let rh = g.size().y + 22.0;
                    let r = egui::Rect::from_min_size(egui::pos2(nav.left(), y), egui::vec2(nav.width() - 1.0, rh));
                    let resp = ui
                        .interact(r, ui.make_persistent_id(("settings.sec", k)), egui::Sense::click())
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    let on = *k == sec.0;
                    if on || resp.hovered() {
                        ui.painter().rect_filled(r, 0.0, t.panel);
                    }
                    if on {
                        ui.painter().rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(2.0, rh)), 0.0, layer);
                    }
                    let font = if on { crate::theme::semi(14.0) } else { egui::FontId::proportional(14.0) };
                    let g =
                        ui.painter().layout(name.clone(), font, if on || resp.hovered() { t.text } else { t.text_dim }, nav.width() - 32.0);
                    ui.painter().galley(egui::pos2(r.left() + 14.0, r.top() + 11.0), g, t.text);
                    if resp.clicked() && !on {
                        pick = Some((global, k.clone()));
                    }
                    y += rh + 2.0;
                }
                // the section (`.shell-pane`: .9rem 1.1rem; its legend 20 px bold at 112 %)
                ui.add_space(18.0);
                ui.vertical(|ui| {
                    ui.set_width((ui.available_width() - 2.0).max(100.0));
                    egui::ScrollArea::vertical().id_salt(("settings.pane", &sec.0)).max_height(body_h).auto_shrink([false, false]).show(
                        ui,
                        |ui| {
                            ui.spacing_mut().item_spacing.y = 6.0;
                            ui.add_space(14.0);
                            if !global && slug != "studio" {
                                crate::shell::note(
                                    ui,
                                    &t,
                                    &crate::lang::t(&SHELL, "shell.settings.app_only", &[("app", app_name(slug).into())]),
                                );
                                ui.add_space(14.0);
                            }
                            ui.label(RichText::new(&sec.1).font(crate::theme::head(20.0)).color(t.text_strong));
                            ui.add_space(8.0);
                            match sec.0.as_str() {
                                "shell.settings.lang" => {
                                    let lab = ui.label(RichText::new(s("shell.settings.lang.label")).color(t.text));
                                    egui::ComboBox::from_id_salt("settings.lang").width(260.0).selected_text(lang_now.name()).show_ui(
                                        ui,
                                        |ui| {
                                            for l in cc_lang::Locale::ALL {
                                                if ui.selectable_label(l == lang_now, l.name()).clicked() {
                                                    act = Some(format!("lang={}", l.code()));
                                                }
                                            }
                                        },
                                    );
                                    let _ = lab;
                                    ui.add_space(6.0);
                                    crate::shell::note(ui, &t, &t0(&STUDIO, "settings.lang.hint"));
                                }
                                "shell.settings.theme" => {
                                    // the three looks the Rust test version adds are plain English (0.5 has no words for them)
                                    for (v, k) in [
                                        ("model", s("shell.settings.theme.model")),
                                        ("paper", s("shell.settings.theme.paper")),
                                        ("system", s("shell.settings.theme.system")),
                                        ("medium-light", "Medium light".to_string()),
                                        ("medium-dark", "Medium dark".to_string()),
                                        ("contrast", "High contrast".to_string()),
                                    ] {
                                        if ui.radio(theme_now == v, k).clicked() {
                                            act = Some(format!("theme={v}"));
                                        }
                                    }
                                    ui.add_space(6.0);
                                    crate::shell::note(ui, &t, &s("shell.settings.theme.hint"));
                                }
                                "shell.settings.zoom" => {
                                    let word = |name: &str, z: f32| {
                                        let pct = (z * 100.0).round() as i32;
                                        crate::lang::t(&SHELL, &format!("shell.settings.zoom.{name}"), &[("pct", pct.to_string())])
                                    };
                                    let cur = ZOOMS
                                        .iter()
                                        .find(|(_, z)| (*z - zoom_now).abs() < 0.001)
                                        .map_or_else(|| format!("{:.0}%", zoom_now * 100.0), |(n, z)| word(n, *z));
                                    egui::ComboBox::from_id_salt("settings.zoom").width(260.0).selected_text(cur).show_ui(ui, |ui| {
                                        for (n, z) in ZOOMS {
                                            if ui.selectable_label((z - zoom_now).abs() < 0.001, word(n, z)).clicked() {
                                                act = Some(format!("zoom={z}"));
                                            }
                                        }
                                    });
                                }
                                "mytools" => crate::mytoolsdlg::section(app, ui, &mut input_act),
                                "input" => crate::inputdlg::section(app, ui, &mut input_act),
                                "set.drawing" if slug == "takeoffcraft" => {
                                    let tt = cc_takeoff::i18n::t;
                                    let (snap, s45) = app.tk_snap;
                                    for (cmd, on, label, hint) in [
                                        ("takeoff.snap", snap, "settings.snap", "settings.snap.hint"),
                                        ("takeoff.snap45", s45, "set.snap45", "set.snap45.hint"),
                                    ] {
                                        let mut v = on;
                                        if ui.checkbox(&mut v, tt(label)).changed() {
                                            input_act.push((cmd, Some(if v { "on".into() } else { "off".into() })));
                                        }
                                        crate::shell::note(ui, &t, &tt(hint));
                                        ui.add_space(6.0);
                                    }
                                }
                                k if slug == "cadcraft" => crate::cadset::section(app, ui, k, &mut act),
                                k if slug == "modelcraft" => crate::mcset::section(app, ui, k, &mut act),
                                _ => {}
                            }
                        },
                    );
                });
            });
            // the foot (`.shell-dlg-foot`: space-between): Reset <app> settings at the left, Done at the right
            crate::shell::foot_flush_split(
                ui,
                |ui| {
                    if !app_secs.is_empty() {
                        let word = crate::lang::t(&SHELL, "shell.settings.reset", &[("app", app_name(slug).into())]);
                        if ui.add(crate::shell::Btn::new(word)).clicked() {
                            reset = true;
                        }
                    }
                },
                |ui| {
                    if ui.add(crate::shell::Btn::primary(s("shell.settings.done"))).clicked() {
                        done = true;
                    }
                },
            );
        });
    if let Some((g, k)) = pick {
        app.settings.global = g;
        app.settings.sec = k;
    }
    if reset {
        let _ = app.run("settings.reset", &Params::default());
    }
    if let Some(a) = act
        && let Err(e) = app.run("settings.set", &Params { name: Some(a), ..Default::default() })
    {
        app.error = Some(e);
    }
    for (id, name) in input_act {
        if let Err(e) = app.run(id, &Params { name, ..Default::default() }) {
            app.message = e;
        }
    }
    if done || !open || (app.ribbon.draft.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Escape))) {
        app.settings.open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_reject_bad_values() {
        cc_lang::with(cc_lang::Locale::En, || {
            let mut app = UiApp::default();
            let set = |app: &mut UiApp, v: &str| app.run("settings.set", &Params { name: Some(v.into()), ..Default::default() });
            set(&mut app, "lang=es-MX").unwrap();
            assert_eq!(cc_lang::locale(), cc_lang::Locale::EsMx);
            set(&mut app, "theme=paper").unwrap();
            assert_eq!(app.preset, Preset::Light);
            set(&mut app, "zoom=1.3").unwrap();
            assert!(set(&mut app, "zoom=9").is_err());
            assert!(set(&mut app, "lang=fr").is_err());
            assert!(set(&mut app, "theme=pink").is_err());
            assert!(set(&mut app, "nothing").is_err());
            let v = app.settings_json();
            assert_eq!(v["lang"], "es-MX");
            let mut b = UiApp::default();
            cc_lang::set(cc_lang::Locale::En);
            b.load_settings(&v);
            assert_eq!(cc_lang::locale(), cc_lang::Locale::EsMx);
            assert_eq!(b.settings.theme, "paper");
            assert!((b.settings.zoom - 1.3).abs() < 1e-6);
            b.load_settings(&json!({"lang": 5, "zoom": "x", "theme": "pink"}));
            assert_eq!(b.settings.theme, "paper");
            assert_eq!(title(&b), crate::lang::t0(&STUDIO, "settings.title"));
        });
    }

    #[test]
    fn sections_and_reset() {
        cc_lang::with(cc_lang::Locale::En, || {
            let mut app = UiApp::default();
            let run = |app: &mut UiApp, id: &str, v: &str| app.run(id, &Params { name: Some(v.into()), ..Default::default() });
            // the start screen has the global sections only
            run(&mut app, "settings.show", "global:shell.settings.theme").unwrap();
            assert!(app.settings.open && app.settings.global && app.settings.sec == "shell.settings.theme");
            assert!(run(&mut app, "settings.show", "app:set.cursor").is_err());
            app.run("file.new", &Params::default()).unwrap();
            run(&mut app, "settings.show", "app:set.cursor").unwrap();
            assert!(!app.settings.global);
            assert!(run(&mut app, "settings.show", "app:nope").is_err());
            assert!(run(&mut app, "settings.show", "elsewhere").is_err());
            // Reset CadCraft settings: cadcraft.js's six back to their defaults, the snap kinds kept
            run(&mut app, "settings.set", "cad.cursor=large").unwrap();
            run(&mut app, "settings.set", "cad.tabs=number").unwrap();
            run(&mut app, "edit.snapkind", "tan").unwrap();
            app.sync_editor();
            app.run("settings.reset", &Params::default()).unwrap();
            assert_eq!((app.cadset.cursor.as_str(), app.cadset.tabs_number), ("medium", false));
            assert!(app.cadset.draft.kinds.contains(&"tan"));
        });
    }
}
