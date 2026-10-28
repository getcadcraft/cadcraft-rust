//! Settings (0.5's shell Settings dialog, app-settings.js, and Studio's): the language of every app, the theme, the
//! panel text size. Words: the shell catalog (`shell.settings.*`) in an app, Studio's (`settings.*`) on the start
//! screen. Every change is a command (`settings.open`, `settings.set {name: "lang=es-MX" | "theme=paper|model|system" |
//! "zoom=1.15"}`, `settings.close`), so it works headless; the hosts keep [`Settings`] between runs
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
        "{name: \"lang=en|es-MX\" | \"theme=model|paper|system\" | \"zoom=<0.5–2>\" | CadCraft's: \"cad.cursor=small|medium|large\" | \"cad.paste=lower-left|center\" | \"cad.pick=add|replace\" | \"cad.lasso=lasso|box\" | \"cad.tabs=full|number\"} → applies at once",
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
                    if !matches!(v, "model" | "paper" | "system") {
                        return Err(format!("settings.set: theme {v} is not one of model, paper, system"));
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
                _ if k.starts_with("cad.") => {
                    app.cadset.set(k.trim_start_matches("cad."), v)?;
                    crate::cadset::push(app);
                }
                _ => return Err(format!("settings.set: no setting {k} (lang, theme, zoom, cad.*)")),
            }
            Ok(())
        }
        _ => Err(format!("unknown command {id}")),
    }
}

impl UiApp {
    /// Settings as the hosts keep them (`{"lang": "es-MX", "theme": "paper", "zoom": 1.15}`).
    pub fn settings_json(&self) -> Value {
        json!({
            "lang": cc_lang::locale().code(),
            "theme": self.settings.theme,
            "zoom": self.settings.zoom,
            "cadcraft": self.cadset.json(),
            "cadprops": self.cadprops.prefs_json(),
        })
    }

    /// Settings read back (a damaged or foreign value keeps the default).
    pub fn load_settings(&mut self, v: &Value) {
        if let Some(l) = v.get("lang").and_then(Value::as_str).and_then(cc_lang::Locale::parse) {
            self.set_locale(l);
        }
        if let Some(th) = v.get("theme").and_then(Value::as_str).filter(|s| matches!(*s, "model" | "paper" | "system")) {
            self.settings.theme = th.to_string();
            self.apply_theme_setting(None);
        }
        if let Some(z) = v.get("zoom").and_then(Value::as_f64).filter(|z| (0.5..=2.0).contains(z)) {
            self.settings.zoom = z as f32;
        }
        if let Some(c) = v.get("cadcraft") {
            self.cadset.load(c);
        }
        if let Some(p) = v.get("cadprops") {
            self.cadprops.load_prefs(p);
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
        let dark = match self.settings.theme.as_str() {
            "paper" => false,
            "system" => system_dark.unwrap_or(self.preset == Preset::Dark),
            _ => true,
        };
        let want = if dark { Preset::Dark } else { Preset::Light };
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
    let mut done = false;
    let theme_now = app.settings.theme.clone();
    let lang_now = cc_lang::locale();
    let zoom_now = app.settings.zoom;
    egui::Window::new(title(app)).open(&mut open).collapsible(false).resizable(false).default_width(340.0).show(ctx, |ui| {
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
        }
        ui.add_space(10.0);
        if ui.button(s("shell.settings.done")).clicked() {
            done = true;
        }
    });
    if let Some(a) = act
        && let Err(e) = app.run("settings.set", &Params { name: Some(a), ..Default::default() })
    {
        app.error = Some(e);
    }
    if done || !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
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
