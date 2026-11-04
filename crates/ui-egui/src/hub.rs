//! CadCraft Studio's start screen (the 0.5 hub: studio/hub/hub.js + index.html), shown while no document is open:
//! the eight apps as tiles with their layer colours, role and short line (the site catalog's words) and their sample,
//! new-document presets, recent files from every app, help, and the footer line. Words come from the hub's own
//! language file (`assets/lang/studio/en.json`) and the catalog (`assets/lang/catalog/en.json`), both 0.5's.
//!
//! Every button is a command (`hub.open`, `hub.sample`, `hub.recent`, `file.new`, `file.open`), so the screen works
//! headless too.

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

/// The language now, as a tag.
pub fn loc() -> &'static str {
    cc_lang::locale().code()
}

fn lang_of(locale: &str) -> cc_lang::Locale {
    cc_lang::Locale::parse(locale).unwrap_or_default()
}

/// The hub's word for a key (English when the locale lacks it; the key itself when nothing has it).
pub fn t(locale: &str, key: &str) -> String {
    crate::lang::STUDIO.get_in(lang_of(locale), key).and_then(Value::as_str).map_or_else(|| key.to_string(), str::to_string)
}

/// `t(key, {n})` with a plural form.
pub fn tn(locale: &str, key: &str, n: usize) -> String {
    let s = match crate::lang::STUDIO.get_in(lang_of(locale), key) {
        Some(Value::Object(o)) => cc_lang::plural(o, Some(&n.to_string())).unwrap_or(key).to_string(),
        Some(Value::String(s)) => s.clone(),
        _ => key.to_string(),
    };
    s.replace("{n}", &n.to_string())
}

/// An app's catalog word (role / short / tagline).
pub fn word(locale: &str, slug: &str, field: &str) -> String {
    let k = format!("catalog.{slug}.{field}");
    crate::lang::SITE.get_in(lang_of(locale), &k).and_then(Value::as_str).map(str::to_string).unwrap_or_default()
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
    /// The language now (the hub's switch sets it for every app: [`cc_lang::set`]).
    pub fn loc(&self) -> &'static str {
        cc_lang::locale().code()
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

/// Relative time, as host.js `whenText` with `Intl.RelativeTimeFormat('en', { numeric: 'auto' })`: "now", "5 minutes
/// ago", "2 hours ago", "yesterday", "3 days ago"; a week or more away, the date on `zone`'s wall clock ("Oct 12,
/// 2026": `toLocaleDateString('en', { dateStyle: 'medium' })`). In es-MX, as `Intl.RelativeTimeFormat('es-MX')`:
/// "ahora", "hace 5 minutos", "ayer", "anteayer", "dentro de 2 horas", and the date as "2 oct 2026".
pub fn when(ms: f64, now: f64, zone: cc_clock::Zone) -> String {
    when_text(ms, now, zone)
}

fn when_text(ms: f64, now: f64, zone: cc_clock::Zone) -> String {
    if cc_lang::is_es() {
        return when_text_es(ms, now, zone);
    }
    // Math.round: halves toward +∞
    let js_round = |x: f64| (x + 0.5).floor();
    let diff = js_round((ms - now) / 1000.0);
    let abs = diff.abs();
    let rel = |n: f64, unit: &str| {
        let k = n.abs();
        let units = format!("{k} {unit}{}", if k == 1.0 { "" } else { "s" });
        if n < 0.0 { format!("{units} ago") } else { format!("in {units}") }
    };
    if !diff.is_finite() {
        return String::new();
    }
    if abs < 45.0 {
        "now".into()
    } else if abs < 3600.0 {
        rel(js_round(diff / 60.0), "minute")
    } else if abs < 86_400.0 {
        rel(js_round(diff / 3600.0), "hour")
    } else if abs < 7.0 * 86_400.0 {
        let n = js_round(diff / 86_400.0);
        if n == -1.0 {
            "yesterday".into()
        } else if n == 1.0 {
            "tomorrow".into()
        } else {
            rel(n, "day")
        }
    } else {
        cc_markup::js::short_date_en(zone.local_ms(ms))
    }
}

fn when_text_es(ms: f64, now: f64, zone: cc_clock::Zone) -> String {
    let js_round = |x: f64| (x + 0.5).floor();
    let diff = js_round((ms - now) / 1000.0);
    if !diff.is_finite() {
        return String::new();
    }
    let abs = diff.abs();
    let rel = |n: f64, one: &str, many: &str| {
        let k = n.abs();
        let units = format!("{k} {}", if k == 1.0 { one } else { many });
        if n < 0.0 { format!("hace {units}") } else { format!("dentro de {units}") }
    };
    if abs < 45.0 {
        "ahora".into()
    } else if abs < 3600.0 {
        rel(js_round(diff / 60.0), "minuto", "minutos")
    } else if abs < 86_400.0 {
        rel(js_round(diff / 3600.0), "hora", "horas")
    } else if abs < 7.0 * 86_400.0 {
        let n = js_round(diff / 86_400.0);
        match n {
            -1.0 => "ayer".into(),
            -2.0 => "anteayer".into(),
            1.0 => "mañana".into(),
            2.0 => "pasado mañana".into(),
            _ => rel(n, "día", "días"),
        }
    } else {
        // toLocaleDateString('es-MX', { dateStyle: 'medium' }) → "2 oct 2026"
        const MESES: [&str; 12] = ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic"];
        let en = cc_markup::js::short_date_en(zone.local_ms(ms));
        // "Oct 2, 2026" → its parts
        let mut it = en.split([' ', ',']).filter(|x| !x.is_empty());
        let (m, d, y) = (it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or(""));
        const EN: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        let mes = EN.iter().position(|x| *x == m).and_then(|i| MESES.get(i)).copied().unwrap_or(m);
        format!("{d} {mes} {y}")
    }
}

fn layer(slug: &str, dark: bool) -> Color32 {
    APPS.iter().find(|a| a.0 == slug).map_or(Color32::GRAY, |a| {
        let c = if dark { a.3 } else { a.2 };
        Color32::from_rgb(c[0], c[1], c[2])
    })
}

/// Draw the start screen; actions go through `app.run`. Laid out as the 0.7 website's apps page (/apps): the drafting
/// grid behind a 1200 px column, the eyebrow and the wide heading, then "Your apps" between dimension ticks, a card
/// per app (sheet-2, 1 px ink edge, 4 px app colour at the left, the swatch, the wide name™, the role, the short line,
/// a drawing of its work, the action button), SuperCadCraft, recent files and help.
pub fn ui(app: &mut UiApp, ui: &mut egui::Ui) {
    use crate::shell;
    use crate::theme::{bold, semi, wide};
    let t0: Tokens = app.tokens;
    let dark = app.preset.is_dark();
    let loc = app.hub.loc().to_string();
    let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64);
    let zone = app.zone;
    let act: std::cell::RefCell<Option<(String, Params)>> = std::cell::RefCell::new(None);
    let set = |id: &str, p: Params| *act.borrow_mut() = Some((id.to_string(), p));
    egui::CentralPanel::default().frame(egui::Frame::new().fill(t0.panel_darker)).show(ui, |ui| {
        paper_grid(ui.painter(), ui.max_rect(), &t0, dark);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let full = ui.available_width();
            let col = full.min(1200.0 + 48.0);
            let side = ((full - col) / 2.0).max(0.0);
            ui.horizontal_top(|ui| {
                ui.add_space(side);
                ui.vertical(|ui| {
                    egui::Frame::new().inner_margin(egui::Margin { left: 24, right: 24, top: 48, bottom: 48 }).show(ui, |ui| {
                        let w = col - 48.0;
                        ui.set_width(w);
                        ui.spacing_mut().item_spacing.y = 12.0;
                        // the eyebrow, the heading, the lead line (site.css .eyebrow / h1 / .lead)
                        ui.label(RichText::new("Rust test version").font(semi(17.0)).color(t0.text_dim));
                        ui.label(
                            RichText::new(t(&loc, "studio.name")).font(wide(if w > 700.0 { 52.0 } else { 36.0 })).color(t0.text_strong),
                        );
                        ui.label(RichText::new(t(&loc, "foot.line")).size(20.0).line_height(Some(31.0)).color(t0.text_dim));
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 12.0;
                            if shell::action_button(ui, &t0, "folder2-open", &t(&loc, "open.button"), true)
                                .on_hover_text(t(&loc, "open.title"))
                                .clicked()
                            {
                                set("file.open", Params::default());
                            }
                            if shell::action_button(ui, &t0, "", &crate::lang::t0(&crate::lang::CADCRAFT, "app.new_drawing"), false)
                                .on_hover_text(crate::lang::t0(&crate::lang::CADCRAFT, "input.cmd.file.new"))
                                .clicked()
                            {
                                set("file.new", Params::default());
                            }
                            if shell::action_button(ui, &t0, "gear", &t(&loc, "settings.button"), false)
                                .on_hover_text(t(&loc, "settings.title"))
                                .clicked()
                            {
                                set("settings.open", Params::default());
                            }
                        });
                        ui.label(RichText::new(t(&loc, "drop.body")).size(15.0).color(t0.text_dim));
                        ui.add_space(28.0);
                        dim_divider(ui, &t0, w, &t(&loc, "apps.title"));
                        ui.add_space(20.0);
                        // the cards: two columns as the site (one on a narrow window)
                        let cols = if w >= 760.0 { 2 } else { 1 };
                        let gap = 20.0;
                        let cw = (w - gap * (cols as f32 - 1.0)) / cols as f32;
                        for row in APPS.chunks(cols) {
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = gap;
                                for (slug, name, _, _) in row {
                                    let c = layer(slug, dark);
                                    ui.vertical(|ui| {
                                        ui.set_width(cw);
                                        card(ui, &t0, c, cw, |ui| {
                                            ui.spacing_mut().item_spacing.y = 10.0;
                                            shell::swatch(ui, &t0, 28.0, c);
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 2.0;
                                                ui.label(RichText::new(*name).font(wide(26.0)).color(t0.text_strong));
                                                ui.label(RichText::new("™").font(semi(12.0)).color(t0.text_strong));
                                            });
                                            ui.label(RichText::new(word(&loc, slug, "role")).font(semi(17.0)).color(t0.text_dim));
                                            ui.label(
                                                RichText::new(word(&loc, slug, "short")).size(15.0).line_height(Some(24.0)).color(t0.text),
                                            );
                                            thumb(ui, &t0, slug, c, cw - 48.0);
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 12.0;
                                                let use_it = format!("{} {name}", t(&loc, "tile.open"));
                                                match sample_command(slug) {
                                                    Some(_) => {
                                                        if shell::action_button(ui, &t0, "play-fill", &use_it, true)
                                                            .on_hover_text(word(&loc, slug, "tagline"))
                                                            .clicked()
                                                        {
                                                            set("hub.sample", Params { name: Some((*slug).into()), ..Default::default() });
                                                        }
                                                    }
                                                    None => {
                                                        if shell::action_button(ui, &t0, "folder2-open", &t(&loc, "open.button"), true)
                                                            .on_hover_text(t(&loc, "open.title"))
                                                            .clicked()
                                                        {
                                                            set("file.open", Params::default());
                                                        }
                                                    }
                                                }
                                                if *slug == "cadcraft"
                                                    && shell::action_button(
                                                        ui,
                                                        &t0,
                                                        "",
                                                        &crate::lang::t0(&crate::lang::CADCRAFT, "app.new_drawing"),
                                                        false,
                                                    )
                                                    .on_hover_text(crate::lang::t0(&crate::lang::CADCRAFT, "input.cmd.file.new"))
                                                    .clicked()
                                                {
                                                    set("file.new", Params::default());
                                                }
                                            });
                                        });
                                    });
                                }
                            });
                            ui.add_space(gap - 12.0);
                        }
                        // SuperCadCraft, still to come (site.css .app-wide, in the ink colour)
                        card(ui, &t0, t0.line, w, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("SuperCadCraft").font(wide(26.0)).color(t0.text_dim));
                                ui.label(RichText::new(t(&loc, "upcoming.tag")).font(semi(13.0)).italics().color(t0.text_dim));
                            });
                            ui.label(RichText::new(t(&loc, "upcoming.role")).font(semi(17.0)).color(t0.text_dim));
                            ui.label(RichText::new(word(&loc, "supercadcraft", "short")).size(15.0).color(t0.text_dim));
                        });
                        ui.add_space(28.0);
                        dim_divider(ui, &t0, w, &t(&loc, "recent.title"));
                        ui.add_space(12.0);
                        let recent = app.hub.recent.clone();
                        if recent.is_empty() {
                            ui.label(RichText::new(t(&loc, "recent.empty")).size(15.0).color(t0.text_dim));
                        } else {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(t(&loc, "recent.filter")).font(semi(14.0)).color(t0.text_dim));
                                let chip = |ui: &mut egui::Ui, key: &str, label: String| {
                                    if shell::toggle(ui, &t0, &label, app.hub.filter == key).clicked() {
                                        set("hub.filter", Params { name: Some(key.into()), ..Default::default() });
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
                                    shell::swatch(ui, &t0, 14.0, layer(r.app, dark));
                                    let can = r.dir.is_some();
                                    let resp = ui.add_enabled(
                                        can,
                                        egui::Button::new(RichText::new(&r.name).font(semi(15.0)).color(t0.text)).frame(false),
                                    );
                                    if resp.clicked() {
                                        set("hub.recent", Params { index: Some(i), ..Default::default() });
                                    }
                                    if !can {
                                        resp.on_disabled_hover_text(t(&loc, "recent.noreopen"));
                                    }
                                    ui.label(
                                        RichText::new(t(&loc, "recent.opened").replace("{when}", &when_text(r.when, now, zone)))
                                            .size(14.0)
                                            .color(t0.text_dim),
                                    );
                                });
                            }
                        }
                        ui.add_space(28.0);
                        dim_divider(ui, &t0, w, &t(&loc, "help.title"));
                        ui.add_space(12.0);
                        for k in ["start", "open", "recent", "assistants"] {
                            egui::CollapsingHeader::new(
                                RichText::new(t(&loc, &format!("help.{k}.title"))).font(bold(15.0)).color(t0.text_strong),
                            )
                            .id_salt(("hub-help", k))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(t(&loc, &format!("help.{k}.body"))).size(15.0).line_height(Some(24.0)).color(t0.text_dim),
                                );
                            });
                        }
                        ui.hyperlink_to(RichText::new("getcadcraft.com/tutorials").font(semi(15.0)), "https://getcadcraft.com/tutorials");
                        ui.add_space(28.0);
                        let (r, _) = ui.allocate_exact_size(egui::vec2(w, 1.0), egui::Sense::hover());
                        ui.painter().line_segment([r.left_center(), r.right_center()], egui::Stroke::new(1.0, t0.rule));
                        ui.label(RichText::new(t(&loc, "foot.line")).size(14.0).color(t0.text_dim));
                    });
                });
            });
        });
    });
    if let Some((id, p)) = act.into_inner()
        && let Err(e) = app.run(&id, &p)
    {
        app.error = Some(e);
    }
}

/// The website's drafting paper (site.css body): 8 px and 40 px grid lines in paper space, the same grid as dots in
/// model space. One 40 × 40 tile is drawn once per look and repeated by the GPU (one quad per frame).
fn paper_grid(p: &egui::Painter, r: egui::Rect, t: &Tokens, dark: bool) {
    let minor = t.canvas_grid.gamma_multiply(0.45);
    let major = t.canvas_grid.gamma_multiply(0.95);
    let key = egui::Id::unique(("cc-paper-tile", dark, minor, major));
    let ctx = p.ctx();
    let tex = ctx.data(|d| d.get_temp::<egui::TextureHandle>(key)).unwrap_or_else(|| {
        let n = 40usize;
        let mut img = egui::ColorImage::filled([n, n], Color32::TRANSPARENT);
        for y in 0..n {
            for x in 0..n {
                let c = if dark {
                    // dots: 1.25 px at every 40 px, 0.8 px at every 8 px (one pixel each at 100 %)
                    if x == 0 && y == 0 || (x <= 1 && y <= 1 && (x + y) <= 1) {
                        major
                    } else if x % 8 == 0 && y % 8 == 0 {
                        minor
                    } else {
                        continue;
                    }
                } else if x == 0 || y == 0 {
                    major
                } else if x % 8 == 0 || y % 8 == 0 {
                    minor
                } else {
                    continue;
                };
                if let Some(px) = img.pixels.get_mut(y * n + x) {
                    *px = c;
                }
            }
        }
        let h = ctx.load_texture("cc-paper-tile", img, egui::TextureOptions::NEAREST_REPEAT);
        ctx.data_mut(|d| d.insert_temp(key, h.clone()));
        h
    });
    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(r.width() / 40.0, r.height() / 40.0));
    p.image(tex.id(), r, uv, Color32::WHITE);
}

/// The website's dimension-line divider (site.css .dim-divider): a 1 px ink line with ticks at both ends, the label
/// (20 px bold, 112 % wide) in the middle.
fn dim_divider(ui: &mut egui::Ui, t: &Tokens, w: f32, label: &str) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(w, 30.0), egui::Sense::hover());
    let p = ui.painter();
    let g = p.layout_no_wrap(label.to_string(), crate::theme::bold(20.0), t.text_strong);
    let (cx, cy) = (r.center().x, r.center().y);
    let half = g.size().x / 2.0 + 16.0;
    let s = egui::Stroke::new(1.0, t.rule);
    p.line_segment([egui::pos2(r.left(), cy), egui::pos2(cx - half, cy)], s);
    p.line_segment([egui::pos2(cx + half, cy), egui::pos2(r.right(), cy)], s);
    for x in [r.left() + 0.5, r.right() - 0.5] {
        p.line_segment([egui::pos2(x, cy - 8.5), egui::pos2(x, cy + 8.5)], s);
        p.line_segment([egui::pos2(x - 4.0, cy + 4.0), egui::pos2(x + 4.0, cy - 4.0)], egui::Stroke::new(2.0, t.rule));
    }
    p.galley(egui::pos2(cx - g.size().x / 2.0, cy - g.size().y / 2.0), g, t.text_strong);
}

/// One card (site.css .app-card): sheet-2, a 1 px ink edge, 4 px of the app's colour at the left, 20 px inside.
fn card(ui: &mut egui::Ui, t: &Tokens, c: Color32, w: f32, add: impl FnOnce(&mut egui::Ui)) {
    let r = egui::Frame::new()
        .fill(t.panel)
        .stroke(egui::Stroke::new(1.0, t.rule))
        .corner_radius(t.radius)
        .inner_margin(egui::Margin { left: 24, right: 20, top: 20, bottom: 20 })
        .show(ui, |ui| {
            ui.set_width(w - 46.0);
            add(ui);
        });
    let rr = r.response.rect;
    ui.painter().rect_filled(egui::Rect::from_min_max(rr.min, egui::pos2(rr.left() + 4.0, rr.bottom())), 0.0, c);
}

/// A small drawing of each app's work in its card (the site's thumbnails, drawn here in code): sheet fill, a 1 px grid
/// edge, line art in the app's colour.
fn thumb(ui: &mut egui::Ui, t: &Tokens, slug: &str, c: Color32, w: f32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(w, 132.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect(r, 2.0, t.panel_darker, egui::Stroke::new(1.0, t.divider), egui::StrokeKind::Inside);
    // a 200 × 100 design box in the middle
    let s = ((r.height() - 24.0) / 100.0).min((r.width() - 24.0) / 200.0);
    let o = egui::pos2(r.center().x - 100.0 * s, r.center().y - 50.0 * s);
    let at = |x: f32, y: f32| egui::pos2(o.x + x * s, o.y + y * s);
    let st = egui::Stroke::new(1.5, c);
    let thin = egui::Stroke::new(1.0, c);
    let line = |pts: &[(f32, f32)], k: egui::Stroke| p.add(egui::Shape::line(pts.iter().map(|(x, y)| at(*x, *y)).collect(), k));
    let rect = |x0: f32, y0: f32, x1: f32, y1: f32, k: egui::Stroke| {
        p.rect_stroke(egui::Rect::from_min_max(at(x0, y0), at(x1, y1)), 0.0, k, egui::StrokeKind::Middle);
    };
    match slug {
        "cadcraft" => {
            // a bracket in elevation with a hole, its centre lines and a dimension
            line(&[(20.0, 90.0), (20.0, 10.0), (40.0, 10.0), (40.0, 70.0), (120.0, 70.0), (120.0, 90.0), (20.0, 90.0)], st);
            p.circle_stroke(at(150.0, 45.0), 18.0 * s, st);
            line(&[(150.0, 20.0), (150.0, 70.0)], thin);
            line(&[(125.0, 45.0), (175.0, 45.0)], thin);
            line(&[(20.0, 98.0), (120.0, 98.0)], thin);
            line(&[(20.0, 94.0), (20.0, 100.0)], thin);
            line(&[(120.0, 94.0), (120.0, 100.0)], thin);
        }
        "bimcraft" => {
            // a house in section: roof, walls, a floor, a stair
            line(&[(20.0, 40.0), (100.0, 5.0), (180.0, 40.0)], egui::Stroke::new(3.0, c));
            rect(35.0, 35.0, 165.0, 95.0, st);
            line(&[(35.0, 65.0), (165.0, 65.0)], egui::Stroke::new(3.0, c));
            for i in 0..6 {
                let x = 120.0 + i as f32 * 7.0;
                let y = 95.0 - i as f32 * 5.0;
                line(&[(x, y), (x, y - 5.0), (x + 7.0, y - 5.0)], thin);
            }
            rect(60.0, 75.0, 75.0, 95.0, thin);
        }
        "modelcraft" => {
            // a box pushed up from a rectangle, in isometric
            let iso = |x: f32, y: f32, z: f32| (100.0 + (x - y) * 0.866, 70.0 + (x + y) * 0.5 - z);
            let pts = |v: &[(f32, f32, f32)]| v.iter().map(|(x, y, z)| iso(*x, *y, *z)).collect::<Vec<_>>();
            line(&pts(&[(0.0, 0.0, 0.0), (60.0, 0.0, 0.0), (60.0, 40.0, 0.0), (0.0, 40.0, 0.0), (0.0, 0.0, 0.0)]), thin);
            line(&pts(&[(0.0, 0.0, 50.0), (60.0, 0.0, 50.0), (60.0, 40.0, 50.0), (0.0, 40.0, 50.0), (0.0, 0.0, 50.0)]), st);
            for (x, y) in [(60.0, 0.0), (60.0, 40.0), (0.0, 40.0)] {
                line(&pts(&[(x, y, 0.0), (x, y, 50.0)]), st);
            }
        }
        "markupcraft" => {
            // a sheet with a revision cloud and a callout
            rect(30.0, 5.0, 170.0, 95.0, thin);
            let mut v = Vec::new();
            for i in 0..=24 {
                let a = i as f32 / 24.0 * std::f32::consts::TAU;
                let rr = 22.0 + 3.0 * (a * 8.0).sin().abs();
                v.push((90.0 + rr * 1.6 * a.cos(), 50.0 + rr * a.sin()));
            }
            line(&v, st);
            line(&[(126.0, 40.0), (150.0, 22.0), (165.0, 22.0)], st);
        }
        "takeoffcraft" => {
            // a plan with one room measured (hatched) and counted points
            rect(20.0, 10.0, 180.0, 90.0, st);
            line(&[(100.0, 10.0), (100.0, 90.0)], st);
            for i in 0..8 {
                let x = 20.0 + i as f32 * 10.0;
                line(&[(x, 90.0), (x + 10.0, 80.0)], thin);
            }
            for (x, y) in [(130.0, 30.0), (160.0, 30.0), (130.0, 70.0), (160.0, 70.0)] {
                p.circle_filled(at(x, y), 3.0 * s.max(0.6), c);
            }
        }
        "civilcraft" => {
            // contours and a road alignment with its stations
            for k in 0..4 {
                let off = k as f32 * 12.0;
                let v: Vec<(f32, f32)> = (0..=20)
                    .map(|i| {
                        let x = 10.0 + i as f32 * 9.0;
                        (x, 20.0 + off + 8.0 * ((x / 30.0).sin()))
                    })
                    .collect();
                line(&v, thin);
            }
            line(&[(10.0, 92.0), (80.0, 75.0), (140.0, 80.0), (190.0, 60.0)], egui::Stroke::new(3.0, c));
            for (x, y) in [(45.0, 83.5), (110.0, 77.5), (165.0, 70.0)] {
                p.circle_stroke(at(x, y), 3.0 * s.max(0.6), thin);
            }
        }
        "detailcraft" => {
            // a steel I-section beside a beam in elevation with bolts
            line(
                &[
                    (20.0, 10.0),
                    (70.0, 10.0),
                    (70.0, 18.0),
                    (49.0, 18.0),
                    (49.0, 82.0),
                    (70.0, 82.0),
                    (70.0, 90.0),
                    (20.0, 90.0),
                    (20.0, 82.0),
                    (41.0, 82.0),
                    (41.0, 18.0),
                    (20.0, 18.0),
                    (20.0, 10.0),
                ],
                st,
            );
            rect(90.0, 30.0, 185.0, 70.0, st);
            line(&[(90.0, 36.0), (185.0, 36.0)], thin);
            line(&[(90.0, 64.0), (185.0, 64.0)], thin);
            for x in [100.0, 112.0, 163.0, 175.0] {
                p.circle_stroke(at(x, 50.0), 3.0 * s.max(0.6), thin);
            }
        }
        _ => {
            // PartCraft: a plate with a slot and a bolt pattern, rounded corners
            p.rect_stroke(egui::Rect::from_min_max(at(30.0, 10.0), at(170.0, 90.0)), 10.0 * s, st, egui::StrokeKind::Middle);
            for (x, y) in [(50.0, 30.0), (150.0, 30.0), (50.0, 70.0), (150.0, 70.0)] {
                p.circle_stroke(at(x, y), 6.0 * s, st);
            }
            p.rect_stroke(egui::Rect::from_min_max(at(80.0, 44.0), at(120.0, 56.0)), 6.0 * s, st, egui::StrokeKind::Middle);
        }
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
        "hub.clear" => {
            app.hub.recent.clear();
            Ok(())
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
            app.set_locale(cc_lang::Locale::parse(&l).unwrap_or_default());
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

    /// host.js `whenText(now + d, now, 'en')` under TZ=Asia/Kolkata (Node 22).
    #[test]
    fn when_text_is_the_js_one() {
        let now = 1_791_810_123_000.0;
        let z = cc_clock::Zone::Fixed(330);
        for (d, want) in [
            (-10e3, "now"),
            (-44.4e3, "now"),
            (-45e3, "1 minute ago"),
            (-90e3, "1 minute ago"),
            (-150e3, "2 minutes ago"),
            (-3599e3, "60 minutes ago"),
            (-3600e3, "1 hour ago"),
            (-5400e3, "1 hour ago"),
            (-86399e3, "24 hours ago"),
            (-86400e3, "yesterday"),
            (-129600e3, "yesterday"),
            (-172800e3, "2 days ago"),
            (-596160e3, "7 days ago"),
            (-604800e3, "Oct 5, 2026"),
            (120e3, "in 2 minutes"),
            (86400e3, "tomorrow"),
            (-3456000e3, "Sep 2, 2026"),
        ] {
            assert_eq!(when_text(now + d, now, z), want, "{d}");
        }
        // 2026-10-01T20:00Z is already Oct 2 in Kolkata
        assert_eq!(when_text(1_790_884_800_000.0, now, z), "Oct 2, 2026");
        assert_eq!(when_text(1_790_884_800_000.0, now, cc_clock::Zone::Utc), "Oct 1, 2026");
    }

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
