//! CadCraft Studio's start screen (the 0.5 hub: studio/hub/hub.js + index.html), shown while no document is open:
//! the eight apps as tiles with their layer colours, role and short line (the site catalog's words) and their sample,
//! new-document presets, recent files from every app, help, and the footer line. Words come from the hub's own
//! language file (`assets/lang/studio/en.json`) and the catalog (`assets/lang/catalog/en.json`), both 0.5's.
//!
//! Every button is a command (`hub.open`, `hub.sample`, `hub.recent`, `file.new`, `file.open`), so the screen works
//! headless too.

use std::sync::OnceLock;

use egui::{Color32, RichText};
use serde_json::Value;

use crate::{Params, UiApp, theme::Tokens};

/// The eight apps in lineup order: (slug, name, layer colour light, layer colour dark).
pub const APPS: [(&str, &str, [u8; 3], [u8; 3]); 8] = [
    ("cadcraft", "CadCraft", [0xB3, 0x12, 0x2E], [0xFF, 0x5A, 0x6E]),
    ("bimcraft", "BimCraft", [0x1E, 0x48, 0xC8], [0x6E, 0x95, 0xFF]),
    ("modelcraft", "ModelCraft", [0xA8, 0x4A, 0x00], [0xFF, 0x9A, 0x3C]),
    ("markupcraft", "MarkupCraft", [0x9A, 0x1F, 0x86], [0xF0, 0x6B, 0xDC]),
    ("takeoffcraft", "TakeoffCraft", [0x8A, 0x62, 0x00], [0xF2, 0xC2, 0x30]),
    ("civilcraft", "CivilCraft", [0x17, 0x70, 0x3A], [0x4B, 0xD3, 0x7B]),
    ("detailcraft", "DetailCraft", [0x5B, 0x3C, 0xC4], [0xA7, 0x8B, 0xFA]),
    ("partcraft", "PartCraft", [0x00, 0x70, 0x7E], [0x3F, 0xD6, 0xE6]),
];

/// The command that opens an app's sample in this build (None: the app has no sample here yet).
pub fn sample_command(slug: &str) -> Option<&'static str> {
    match slug {
        "cadcraft" => Some("file.sample"),
        "bimcraft" => Some("file.sample.ifc"),
        "modelcraft" => Some("view.model.sample"),
        "civilcraft" => Some("civil.sample"),
        "detailcraft" => Some("detail.sample"),
        "partcraft" => Some("part.sample"),
        _ => None,
    }
}

fn catalog(locale: &str) -> &'static Value {
    static EN: OnceLock<Value> = OnceLock::new();
    static ES: OnceLock<Value> = OnceLock::new();
    let parse = |s: &str| serde_json::from_str(s).unwrap_or(Value::Null);
    if locale.starts_with("es") {
        ES.get_or_init(|| parse(include_str!("../../../assets/lang/catalog/es-MX.json")))
    } else {
        EN.get_or_init(|| parse(include_str!("../../../assets/lang/catalog/en.json")))
    }
}

fn hub_lang(locale: &str) -> &'static Value {
    static EN: OnceLock<Value> = OnceLock::new();
    static ES: OnceLock<Value> = OnceLock::new();
    let parse = |s: &str| serde_json::from_str(s).unwrap_or(Value::Null);
    if locale.starts_with("es") {
        ES.get_or_init(|| parse(include_str!("../../../assets/lang/studio/es-MX.json")))
    } else {
        EN.get_or_init(|| parse(include_str!("../../../assets/lang/studio/en.json")))
    }
}

/// The hub's word for a key (English when the locale lacks it; the key itself when nothing has it).
pub fn t(locale: &str, key: &str) -> String {
    let get = |v: &Value| v.get(key).and_then(Value::as_str).map(str::to_string);
    get(hub_lang(locale)).or_else(|| get(hub_lang("en"))).unwrap_or_else(|| key.to_string())
}

/// `t(key, {n})` with a plural form.
pub fn tn(locale: &str, key: &str, n: usize) -> String {
    let forms = hub_lang(locale).get(key).or_else(|| hub_lang("en").get(key));
    let s = match forms {
        Some(Value::Object(o)) => {
            o.get(if n == 1 { "one" } else { "other" }).or_else(|| o.get("other")).and_then(Value::as_str).unwrap_or(key).to_string()
        }
        Some(Value::String(s)) => s.clone(),
        _ => key.to_string(),
    };
    s.replace("{n}", &n.to_string())
}

/// An app's catalog word (role / short / tagline).
pub fn word(locale: &str, slug: &str, field: &str) -> String {
    let k = format!("catalog.{slug}.{field}");
    let get = |v: &Value| v.get(&k).and_then(Value::as_str).map(str::to_string);
    get(catalog(locale)).or_else(|| get(catalog("en"))).unwrap_or_default()
}

/// A file the person opened or saved, newest first (the hub's recent list).
#[derive(Debug, Clone, PartialEq)]
pub struct Recent {
    pub name: String,
    /// the folder it came from (desktop), so it can be opened again
    pub dir: Option<String>,
    pub app: &'static str,
    /// milliseconds since 1970
    pub when: f64,
}

/// Milliseconds since 1970 (0 where the platform gives no clock).
pub fn now_ms() -> f64 {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
}

/// The hub's own state.
#[derive(Debug, Clone, Default)]
pub struct Hub {
    pub recent: Vec<Recent>,
    /// the recent list's app filter ("" = all apps)
    pub filter: String,
    /// language (en, es-MX)
    pub locale: String,
    pub show_help: bool,
}

pub const MAX_RECENT: usize = 30;

impl Hub {
    /// Note a file that was opened (the newest goes first; the same file moves up).
    pub fn note(&mut self, name: &str, dir: Option<&str>, app: &'static str, when: f64) {
        if app == "studio" {
            return;
        }
        self.recent.retain(|r| !(r.name == name && r.dir.as_deref() == dir));
        self.recent.insert(0, Recent { name: name.to_string(), dir: dir.map(str::to_string), app, when });
        self.recent.truncate(MAX_RECENT);
    }
    pub fn loc(&self) -> &str {
        if self.locale.is_empty() { "en" } else { &self.locale }
    }
}

/// The app that reads a file, by its name (the hub's routing: drawings → CadCraft, models → BimCraft …; a PDF goes to
/// MarkupCraft, the same as the fork's sheet view opens it).
pub fn app_for(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit_once('.').map_or("", |(_, e)| e);
    if lower.ends_with(".takeoff.json") {
        return "takeoffcraft";
    }
    if lower.ends_with(".partcraft.json") || ext == "pcscript" {
        return "partcraft";
    }
    match ext {
        "dwg" | "dxf" => "cadcraft",
        "ifc" => "bimcraft",
        "modelcraft" | "skp" | "obj" | "glb" | "gltf" | "dae" | "stl" => "modelcraft",
        "pdf" => "markupcraft",
        "csv" | "txt" | "xml" | "landxml" => "civilcraft",
        _ => "cadcraft",
    }
}

/// Relative time, in the hub's sense ("2 hours ago").
fn when_text(ms: f64, now: f64) -> String {
    let s = ((now - ms) / 1000.0).max(0.0);
    if s < 45.0 {
        return "just now".into();
    }
    let (n, unit) = if s < 3600.0 {
        ((s / 60.0).round(), "minute")
    } else if s < 86_400.0 {
        ((s / 3600.0).round(), "hour")
    } else {
        ((s / 86_400.0).round(), "day")
    };
    format!("{n} {unit}{} ago", if n == 1.0 { "" } else { "s" })
}

fn layer(slug: &str, dark: bool) -> Color32 {
    APPS.iter().find(|a| a.0 == slug).map_or(Color32::GRAY, |a| {
        let c = if dark { a.3 } else { a.2 };
        Color32::from_rgb(c[0], c[1], c[2])
    })
}

/// Draw the start screen; actions go through `app.run`.
pub fn ui(app: &mut UiApp, ui: &mut egui::Ui) {
    let t0: Tokens = app.tokens;
    let dark = app.preset == crate::theme::Preset::Dark;
    let loc = app.hub.loc().to_string();
    let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64);
    let act: std::cell::RefCell<Option<(String, Params)>> = std::cell::RefCell::new(None);
    egui::CentralPanel::default().frame(egui::Frame::new().fill(t0.canvas_bg).inner_margin(egui::Margin::symmetric(24, 18))).show(
        ui,
        |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(t(&loc, "studio.name")).size(22.0).strong().color(t0.text_strong));
                    ui.label(RichText::new("Rust test version").small().color(t0.text_dim));
                });
                ui.add_space(10.0);
                let wide = ui.available_width() > 980.0;
                let side_w = if wide { 300.0 } else { ui.available_width() };
                let main_w = if wide { ui.available_width() - side_w - 24.0 } else { ui.available_width() };
                let main = |ui: &mut egui::Ui| {
                    ui.set_width(main_w);
                    ui.label(RichText::new(t(&loc, "apps.title")).size(15.0).strong().color(t0.text_strong));
                    ui.add_space(6.0);
                    let cols = ((main_w / 230.0).floor() as usize).clamp(1, 4);
                    let tile_w = (main_w - 12.0 * (cols as f32 - 1.0)) / cols as f32;
                    for row in APPS.chunks(cols) {
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 12.0;
                            for (slug, name, _, _) in row {
                                let c = layer(slug, dark);
                                let tile = egui::vec2(tile_w, 150.0);
                                ui.allocate_ui_with_layout(tile, egui::Layout::top_down(egui::Align::Min), |ui| {
                                    egui::Frame::new()
                                        .fill(t0.panel)
                                        .stroke(egui::Stroke::new(1.0, t0.divider))
                                        .corner_radius(t0.radius)
                                        .inner_margin(10)
                                        .show(ui, |ui| {
                                            ui.set_width(tile_w - 22.0);
                                            ui.set_min_height(128.0);
                                            ui.horizontal(|ui| {
                                                let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                                                ui.painter().rect_filled(r, 2.0, c);
                                                ui.label(RichText::new(*name).strong().size(15.0).color(t0.text_strong));
                                            });
                                            ui.label(RichText::new(word(&loc, slug, "role")).color(c));
                                            ui.label(RichText::new(word(&loc, slug, "short")).small().color(t0.text_dim));
                                            ui.add_space(4.0);
                                            ui.horizontal(|ui| {
                                                match sample_command(slug) {
                                                    Some(_) => {
                                                        if ui
                                                            .button(t(&loc, "tile.open"))
                                                            .on_hover_text(format!("{name}: open its sample"))
                                                            .clicked()
                                                        {
                                                            *act.borrow_mut() = Some((
                                                                "hub.sample".into(),
                                                                Params { name: Some((*slug).into()), ..Default::default() },
                                                            ));
                                                        }
                                                    }
                                                    None => {
                                                        if ui
                                                            .button(t(&loc, "open.button"))
                                                            .on_hover_text(format!("{name}: open a file"))
                                                            .clicked()
                                                        {
                                                            *act.borrow_mut() = Some(("file.open".into(), Params::default()));
                                                        }
                                                    }
                                                }
                                                if *slug == "cadcraft"
                                                    && ui
                                                        .button("New drawing")
                                                        .on_hover_text("An empty drawing to draft in (Ctrl+N)")
                                                        .clicked()
                                                {
                                                    *act.borrow_mut() = Some(("file.new".into(), Params::default()));
                                                }
                                            });
                                        });
                                });
                            }
                        });
                        ui.add_space(12.0);
                    }
                    ui.add_space(6.0);
                    egui::Frame::new().fill(t0.panel_darker).corner_radius(t0.radius).inner_margin(10).show(ui, |ui| {
                        ui.set_width(main_w - 20.0);
                        ui.label(RichText::new("SuperCadCraft").strong().color(t0.text_dim));
                        ui.label(RichText::new(t(&loc, "upcoming.role")).color(t0.text_dim));
                        ui.label(RichText::new(word(&loc, "supercadcraft", "short")).small().color(t0.text_dim));
                        ui.label(RichText::new(t(&loc, "upcoming.tag")).small().italics().color(t0.text_dim));
                    });
                    ui.add_space(16.0);
                    ui.label(RichText::new(t(&loc, "recent.title")).size(15.0).strong().color(t0.text_strong));
                    let recent = app.hub.recent.clone();
                    if recent.is_empty() {
                        ui.label(RichText::new(t(&loc, "recent.empty")).color(t0.text_dim));
                    } else {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(t(&loc, "recent.filter")).small().color(t0.text_dim));
                            let chip = |ui: &mut egui::Ui, key: &str, label: String| {
                                if ui.selectable_label(app.hub.filter == key, label).clicked() {
                                    *act.borrow_mut() =
                                        Some(("hub.filter".into(), Params { name: Some(key.into()), ..Default::default() }));
                                }
                            };
                            chip(ui, "", t(&loc, "recent.all"));
                            for (slug, name, _, _) in APPS {
                                if recent.iter().any(|r| r.app == slug) {
                                    chip(ui, slug, name.to_string());
                                }
                            }
                        });
                        for (i, r) in recent.iter().enumerate().filter(|(_, r)| app.hub.filter.is_empty() || r.app == app.hub.filter) {
                            ui.horizontal(|ui| {
                                let (sw, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                                ui.painter().rect_filled(sw, 1.0, layer(r.app, dark));
                                let can = r.dir.is_some();
                                let resp = ui.add_enabled(can, egui::Button::new(RichText::new(&r.name).color(t0.text)).frame(false));
                                if resp.clicked() {
                                    *act.borrow_mut() = Some(("hub.recent".into(), Params { index: Some(i), ..Default::default() }));
                                }
                                if !can {
                                    resp.on_disabled_hover_text(t(&loc, "recent.noreopen"));
                                }
                                ui.label(
                                    RichText::new(t(&loc, "recent.opened").replace("{when}", &when_text(r.when, now)))
                                        .small()
                                        .color(t0.text_dim),
                                );
                            });
                        }
                    }
                };
                let side = |ui: &mut egui::Ui| {
                    ui.set_width(side_w);
                    egui::Frame::new()
                        .fill(t0.panel)
                        .stroke(egui::Stroke::new(1.0, t0.divider))
                        .corner_radius(t0.radius)
                        .inner_margin(12)
                        .show(ui, |ui| {
                            ui.set_width(side_w - 26.0);
                            ui.label(RichText::new(t(&loc, "drop.title")).strong().color(t0.text_strong));
                            ui.label(RichText::new(t(&loc, "drop.body")).small().color(t0.text));
                            if ui.button(t(&loc, "open.button")).on_hover_text(t(&loc, "open.title")).clicked() {
                                *act.borrow_mut() = Some(("file.open".into(), Params::default()));
                            }
                        });
                    ui.add_space(12.0);
                    egui::Frame::new()
                        .fill(t0.panel)
                        .stroke(egui::Stroke::new(1.0, t0.divider))
                        .corner_radius(t0.radius)
                        .inner_margin(12)
                        .show(ui, |ui| {
                            ui.set_width(side_w - 26.0);
                            ui.label(RichText::new(t(&loc, "help.title")).strong().color(t0.text_strong));
                            for k in ["start", "open", "recent", "assistants"] {
                                egui::CollapsingHeader::new(RichText::new(t(&loc, &format!("help.{k}.title"))).color(t0.text))
                                    .id_salt(("hub-help", k))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new(t(&loc, &format!("help.{k}.body"))).small().color(t0.text_dim));
                                    });
                            }
                            ui.hyperlink_to("getcadcraft.com/tutorials", "https://getcadcraft.com/tutorials");
                        });
                };
                if wide {
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| main(ui));
                        ui.add_space(24.0);
                        ui.vertical(|ui| side(ui));
                    });
                } else {
                    main(ui);
                    ui.add_space(16.0);
                    side(ui);
                }
                ui.add_space(18.0);
                ui.separator();
                ui.label(RichText::new(t(&loc, "foot.line")).small().color(t0.text_dim));
            });
        },
    );
    if let Some((id, p)) = act.into_inner()
        && let Err(e) = app.run(&id, &p)
    {
        app.error = Some(e);
    }
}

/// The app slug as a static string (recent lists read back from a file).
pub fn app_static(s: &str) -> &'static str {
    APPS.iter().map(|a| a.0).find(|a| *a == s).unwrap_or("cadcraft")
}

/// The hub's commands: `hub.sample` {name: app}, `hub.recent` {index}, `hub.filter` {name: app or ""},
/// `hub.lang` {name: en | es-MX}, `hub.home` (back to the start screen: everything open is closed).
pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    match id {
        "hub.sample" => {
            let slug = params.name.clone().ok_or("hub.sample: name = an app")?;
            let cmd =
                sample_command(&slug).ok_or_else(|| format!("hub.sample: {} has no sample in this version yet", app_title_of(&slug)))?;
            app.run(cmd, &Params::default())
        }
        "hub.recent" => {
            let i = params.index.ok_or("hub.recent: index is required")?;
            let r = app.hub.recent.get(i).cloned().ok_or_else(|| format!("hub.recent: no recent file {i}"))?;
            let dir = r.dir.clone().ok_or_else(|| t(app.hub.loc(), "recent.noreopen"))?;
            let read = app.services.read_path.as_mut().ok_or_else(|| t("en", "recent.noreopen"))?;
            let path = std::path::Path::new(&dir).join(&r.name).display().to_string();
            match read(&path) {
                Ok(f) => {
                    app.open_file(&f);
                    Ok(())
                }
                Err(_) => {
                    let loc = app.hub.loc().to_string();
                    app.hub.recent.retain(|x| x != &r);
                    Err(t(&loc, "recent.gone"))
                }
            }
        }
        "hub.filter" => {
            app.hub.filter = params.name.clone().unwrap_or_default();
            Ok(())
        }
        "hub.lang" => {
            let l = params.name.clone().unwrap_or_else(|| "en".into());
            if l != "en" && l != "es-MX" {
                return Err(format!("hub.lang: {l} is not one of en, es-MX"));
            }
            app.hub.locale = l;
            Ok(())
        }
        "hub.home" => {
            app.doc = None;
            app.editor = None;
            app.source = None;
            app.pdf = None;
            app.mc = None;
            app.model3 = None;
            app.scene3 = None;
            app.ifc3 = None;
            app.ifc_groups.clear();
            app.civil = None;
            app.detail = None;
            app.part = None;
            app.error = None;
            app.views.clear();
            app.space = 0;
            app.message = String::new();
            Ok(())
        }
        other => Err(format!("unknown command {other}")),
    }
}

fn app_title_of(slug: &str) -> &'static str {
    APPS.iter().find(|a| a.0 == slug).map_or("This app", |a| a.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hub_words_come_from_the_05_catalogs() {
        assert_eq!(t("en", "studio.name"), "CadCraft Studio");
        assert_ne!(t("es-MX", "recent.title"), t("en", "recent.title"));
        for (slug, ..) in APPS {
            assert!(!word("en", slug, "role").is_empty(), "{slug}");
        }
    }

    #[test]
    fn recent_files_and_hub_commands() {
        let mut app = UiApp::default();
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/samples");
        let bytes = std::fs::read(format!("{dir}/kitchen-equipment.dwg")).unwrap();
        app.open_file(&crate::ImportedFile { name: "kitchen-equipment.dwg".into(), bytes, dir: Some(dir.into()) });
        assert_eq!(app.hub.recent.len(), 1);
        assert_eq!(app.hub.recent[0].app, "cadcraft");
        assert_eq!(app.active_app(), "cadcraft");
        app.run("hub.home", &Params::default()).unwrap();
        assert_eq!(app.active_app(), "studio");
        app.run("hub.sample", &Params { name: Some("civilcraft".into()), ..Default::default() }).unwrap();
        assert_eq!(app.active_app(), "civilcraft");
        assert!(app.run("hub.lang", &Params { name: Some("xx".into()), ..Default::default() }).is_err());
        assert!(app.run("hub.recent", &Params { index: Some(99), ..Default::default() }).is_err());
        for i in 0..40 {
            app.hub.note(&format!("f{i}.dwg"), None, "cadcraft", 0.0);
        }
        assert_eq!(app.hub.recent.len(), MAX_RECENT);
    }
}
