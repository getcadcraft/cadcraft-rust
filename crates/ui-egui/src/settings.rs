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
}

impl Default for Settings {
    fn default() -> Self {
        Settings { theme: "model".into(), zoom: 1.0, open: false }
    }
}

pub const UI_COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("settings.open", "Settings", "", "{} → opens Settings (language, theme, panel text size)"),
    ("settings.close", "Settings: done", "", "{}"),
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
    let theme_now = app.settings.theme.clone();
    let lang_now = cc_lang::locale();
    let zoom_now = app.settings.zoom;
    // CadCraft's sections make it long: it scrolls within the window
    let max_h = (ctx.content_rect().height() - 60.0).max(200.0);
    crate::shell::Dialog::new(title(app))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(340.0)
        .max_height(max_h)
        .default_height(max_h)
        .vscroll(true)
        .show(ctx, |ui| {
            ui.label(RichText::new(s("shell.settings.general")).strong().color(t.text_strong));
            ui.add_space(4.0);
            // language: every app (0.5 reloads the page; here the words change at once)
            ui.horizontal(|ui| {
                let lab = ui.label(RichText::new(s("shell.settings.lang.label")).color(t.text));
                egui::ComboBox::new("settings.lang", "").selected_text(lang_now.name()).show_ui(ui, |ui| {
                    for l in cc_lang::Locale::ALL {
                        if ui.selectable_label(l == lang_now, l.name()).clicked() {
                            act = Some(format!("lang={}", l.code()));
                        }
                    }
                });
                let _ = lab;
            });
            ui.label(RichText::new(t0(&STUDIO, "settings.lang.hint")).small().color(t.text_dim));
            ui.add_space(8.0);
            ui.label(RichText::new(s("shell.settings.theme")).color(t.text));
            ui.horizontal_wrapped(|ui| {
                for (v, k) in [
                    ("model", "shell.settings.theme.model"),
                    ("paper", "shell.settings.theme.paper"),
                    ("system", "shell.settings.theme.system"),
                ] {
                    if ui.radio(theme_now == v, s(k)).clicked() {
                        act = Some(format!("theme={v}"));
                    }
                }
            });
            // the three looks the Rust test version adds (CLAUDE.md's presets; plain English: 0.5 has no words for them)
            ui.horizontal_wrapped(|ui| {
                for (v, k) in [("medium-light", "Medium light"), ("medium-dark", "Medium dark"), ("contrast", "High contrast")] {
                    if ui.radio(theme_now == v, k).clicked() {
                        act = Some(format!("theme={v}"));
                    }
                }
            });
            ui.label(RichText::new(s("shell.settings.theme.hint")).small().color(t.text_dim));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(s("shell.settings.zoom")).color(t.text));
                let word = |name: &str, z: f32| {
                    let pct = (z * 100.0).round() as i32;
                    crate::lang::t(&SHELL, &format!("shell.settings.zoom.{name}"), &[("pct", pct.to_string())])
                };
                let cur = ZOOMS
                    .iter()
                    .find(|(_, z)| (*z - zoom_now).abs() < 0.001)
                    .map_or_else(|| format!("{:.0}%", zoom_now * 100.0), |(n, z)| word(n, *z));
                egui::ComboBox::new("settings.zoom", "").selected_text(cur).show_ui(ui, |ui| {
                    for (n, z) in ZOOMS {
                        if ui.selectable_label((z - zoom_now).abs() < 0.001, word(n, z)).clicked() {
                            act = Some(format!("zoom={z}"));
                        }
                    }
                });
            });
            // the app's own section (0.5's app-settings.inc includes the build's settings.inc)
            if app.active_app() == "cadcraft" {
                crate::cadset::section(app, ui, &mut act);
                crate::mytoolsdlg::section(app, ui, &mut input_act);
                crate::inputdlg::section(app, ui, &mut input_act);
            }
            if app.active_app() == "modelcraft" {
                crate::mcset::section(app, ui, &mut act);
            }
            ui.add_space(10.0);
            if ui.add(crate::shell::Btn::new(s("shell.settings.done"))).clicked() {
                done = true;
            }
        });
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
}
