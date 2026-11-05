//! Theme tokens: one flat struct of colour roles; widgets and the canvas read only these.
//!
//! The values are CadCraft's own (the 0.7 apps' `tokens.css`, measured in docs/ui-notes.md): paper space is the light
//! look (the plotted sheet), model space the dark one (the drafting canvas). Every app owns one layer colour (its
//! icons, its swatch, the card edge on the start screen); links, pressed toggles and focus use the action blue.

use egui::Color32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    /// the app bar, the toolbar and the panels (0.7 `--sheet-2`)
    pub app_bar: Color32,
    pub panel: Color32,
    /// the stage behind them, hover fills, inactive tabs (0.7 `--sheet`)
    pub panel_darker: Color32,
    /// light rules inside panels (0.7 `--grid`)
    pub divider: Color32,
    /// control borders and section rules (0.7 `--line`)
    pub line: Color32,
    /// the bars' strong rules and app-bar button borders (0.7 `--ink`)
    pub rule: Color32,
    pub text: Color32,
    pub text_strong: Color32,
    /// notes, labels (0.7 `--ink-2`)
    pub text_dim: Color32,
    pub text_disabled: Color32,
    /// links, pressed toggles, the open tab's underline, focus (0.7 `--action`)
    pub accent: Color32,
    /// text on an accent fill (0.7 `--on-action`)
    pub on_accent: Color32,
    pub row_selected: Color32,
    /// a text field's fill
    pub input: Color32,
    /// a hovered button's fill
    pub hover: Color32,
    pub focus_ring: Color32,
    /// the eight app layer colours: red, ochre, green, cyan, blue, magenta, orange, violet
    pub layers: [Color32; 8],
    /// model space background
    pub canvas_bg: Color32,
    /// the canvas grid (0.7 `--grid`)
    pub canvas_grid: Color32,
    /// around a sheet
    pub desk: Color32,
    /// the sheet itself
    pub sheet: Color32,
    /// drawing "ink" (ACI 7) on the canvas
    pub ink: Color32,
    /// ink on a sheet
    pub sheet_ink: Color32,
    pub viewport_frame: Color32,
    pub radius: u8,
}

/// The five looks: 0.7's paper (Light) and model (Dark) spaces, two medium steps between them, and High Contrast
/// (black chrome, white text, yellow selection and focus).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Dark,
    MediumDark,
    MediumLight,
    Light,
    HighContrast,
}

impl Preset {
    pub const ALL: [Preset; 5] = [Preset::Light, Preset::MediumLight, Preset::MediumDark, Preset::Dark, Preset::HighContrast];

    /// Dark chrome (model space): the 3D views and the canvas draw in their dark colours.
    pub fn is_dark(self) -> bool {
        matches!(self, Preset::Dark | Preset::MediumDark | Preset::HighContrast)
    }

    /// The Settings value for this look (`theme=…`).
    pub fn setting(self) -> &'static str {
        match self {
            Preset::Dark => "model",
            Preset::MediumDark => "medium-dark",
            Preset::MediumLight => "medium-light",
            Preset::Light => "paper",
            Preset::HighContrast => "contrast",
        }
    }

    pub fn from_setting(s: &str) -> Option<Preset> {
        Preset::ALL.into_iter().find(|p| p.setting() == s)
    }
}

/// The app slugs in the order of `Tokens::layers` they take their colour from (the website's Catalog::LAYERS).
pub const APP_LAYER: &[(&str, usize)] = &[
    ("cadcraft", 0),
    ("takeoffcraft", 1),
    ("civilcraft", 2),
    ("partcraft", 3),
    ("bimcraft", 4),
    ("markupcraft", 5),
    ("modelcraft", 6),
    ("detailcraft", 7),
];

const fn hex(v: u32) -> Color32 {
    Color32::from_rgb(((v >> 16) & 0xFF) as u8, ((v >> 8) & 0xFF) as u8, (v & 0xFF) as u8)
}

/// `a` over `b` at `f` (0 = b, 1 = a), as CSS color-mix in sRGB.
pub fn mix(a: Color32, b: Color32, f: f32) -> Color32 {
    let m = |x: u8, y: u8| (f32::from(x) * f + f32::from(y) * (1.0 - f)).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// One look's base values (named after 0.7's CSS custom properties).
struct Base {
    sheet: u32,
    sheet2: u32,
    ink: u32,
    ink2: u32,
    grid: u32,
    line: u32,
    action: u32,
    on_action: u32,
    layers: [u32; 8],
    dark: bool,
}

const LAYERS_PAPER: [u32; 8] = [0xB3122E, 0x8A6200, 0x17703A, 0x00707E, 0x1E48C8, 0x9A1F86, 0xA84A00, 0x5B3CC4];
const LAYERS_MODEL: [u32; 8] = [0xFF5A6E, 0xF2C230, 0x4BD37B, 0x3FD6E6, 0x6E95FF, 0xF06BDC, 0xFF9A3C, 0xA78BFA];

impl Tokens {
    pub fn preset(p: Preset) -> Tokens {
        let b = match p {
            Preset::Light => Base {
                sheet: 0xF3F6F8,
                sheet2: 0xFFFFFF,
                ink: 0x17202B,
                ink2: 0x34404E,
                grid: 0xD5DDE5,
                line: 0x7A8796,
                action: 0x1E48C8,
                on_action: 0xFFFFFF,
                layers: LAYERS_PAPER,
                dark: false,
            },
            Preset::MediumLight => Base {
                sheet: 0xE2E8ED,
                sheet2: 0xEDF1F4,
                ink: 0x131B25,
                ink2: 0x2C3744,
                grid: 0xC3CDD7,
                line: 0x6B7887,
                action: 0x1A3FB0,
                on_action: 0xFFFFFF,
                layers: [0x9E0F28, 0x735200, 0x125E30, 0x005C68, 0x1A3FB0, 0x861A74, 0x8F3F00, 0x4F33AD],
                dark: false,
            },
            Preset::MediumDark => Base {
                sheet: 0x2A313A,
                sheet2: 0x343C47,
                ink: 0xEEF2F6,
                ink2: 0xCED6DF,
                grid: 0x47525F,
                line: 0x7F8C9B,
                action: 0x86A6FF,
                on_action: 0x1D232B,
                layers: LAYERS_MODEL,
                dark: true,
            },
            Preset::Dark => Base {
                sheet: 0x1D232B,
                sheet2: 0x252D37,
                ink: 0xE8EEF4,
                ink2: 0xC3CDD8,
                grid: 0x36414E,
                line: 0x6C7A8A,
                action: 0x6E95FF,
                on_action: 0x1D232B,
                layers: LAYERS_MODEL,
                dark: true,
            },
            Preset::HighContrast => Base {
                sheet: 0x000000,
                sheet2: 0x000000,
                ink: 0xFFFFFF,
                ink2: 0xFFFFFF,
                grid: 0x6A6A6A,
                line: 0xFFFFFF,
                action: 0xFFD400,
                on_action: 0x000000,
                layers: [0xFF7A8A, 0xFFD400, 0x5CE68C, 0x4FE3F2, 0x8FB0FF, 0xFF8AE6, 0xFFAA55, 0xBBA4FF],
                dark: true,
            },
        };
        let (sheet, sheet2, ink, action) = (hex(b.sheet), hex(b.sheet2), hex(b.ink), hex(b.action));
        Tokens {
            app_bar: sheet2,
            panel: sheet2,
            panel_darker: sheet,
            divider: hex(b.grid),
            line: hex(b.line),
            rule: ink,
            text: ink,
            text_strong: ink,
            text_dim: hex(b.ink2),
            // 0.7 `:disabled { opacity: .45 }`
            text_disabled: mix(ink, sheet2, 0.45),
            accent: action,
            on_accent: hex(b.on_action),
            row_selected: if p == Preset::HighContrast {
                mix(action, sheet2, 0.45)
            } else {
                mix(action, sheet2, if b.dark { 0.32 } else { 0.16 })
            },
            input: sheet,
            hover: if b.dark { mix(ink, sheet2, 0.08) } else { sheet },
            focus_ring: action,
            layers: b.layers.map(hex),
            canvas_bg: sheet,
            canvas_grid: hex(b.grid),
            desk: if b.dark { mix(Color32::BLACK, sheet, 0.25) } else { hex(b.grid) },
            sheet: if b.dark { sheet2 } else { Color32::WHITE },
            ink,
            sheet_ink: ink,
            viewport_frame: hex(b.line),
            radius: 2,
        }
    }

    /// The app's own layer colour (Studio and an unknown slug: the action blue).
    pub fn app(&self, slug: &str) -> Color32 {
        APP_LAYER.iter().find(|(s, _)| *s == slug).and_then(|(_, i)| self.layers.get(*i).copied()).unwrap_or(self.accent)
    }

    /// Fill egui's visuals from the tokens (once per theme change).
    pub fn apply(&self, ctx: &egui::Context, dark: bool) {
        use egui::{CornerRadius, Stroke};
        let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        v.panel_fill = self.panel;
        v.window_fill = self.panel;
        v.window_stroke = Stroke::new(1.0, self.rule);
        v.window_corner_radius = CornerRadius::same(self.radius);
        v.menu_corner_radius = CornerRadius::same(self.radius);
        v.extreme_bg_color = self.input;
        v.text_edit_bg_color = Some(self.input);
        v.faint_bg_color = self.panel_darker;
        v.code_bg_color = self.panel_darker;
        v.selection.bg_fill = self.row_selected;
        v.selection.stroke = Stroke::new(1.0, self.text_strong);
        v.hyperlink_color = self.accent;
        v.override_text_color = Some(self.text);
        v.weak_text_color = Some(self.text_dim);
        v.warn_fg_color = self.layers[6];
        v.error_fg_color = self.layers[0];
        let r = CornerRadius::same(self.radius);
        let w = &mut v.widgets;
        // labels, separators, frames
        w.noninteractive.bg_fill = self.panel;
        w.noninteractive.weak_bg_fill = self.panel;
        w.noninteractive.bg_stroke = Stroke::new(1.0, self.divider);
        w.noninteractive.fg_stroke = Stroke::new(1.0, self.text);
        // buttons, combo boxes, check boxes at rest: 0.7's `.cc-select` / `.cc-input` (sheet fill, 1 px line border)
        w.inactive.bg_fill = self.input;
        w.inactive.weak_bg_fill = self.input;
        w.inactive.bg_stroke = Stroke::new(1.0, self.line);
        w.inactive.fg_stroke = Stroke::new(1.0, self.text);
        // hovered: the border goes to ink (`.cc-tool:hover`, `.app-bar-btn:hover`)
        w.hovered.bg_fill = self.hover;
        w.hovered.weak_bg_fill = self.hover;
        w.hovered.bg_stroke = Stroke::new(1.0, self.rule);
        w.hovered.fg_stroke = Stroke::new(1.0, self.text_strong);
        w.hovered.expansion = 0.0;
        // pressed and open
        w.active.bg_fill = self.row_selected;
        w.active.weak_bg_fill = self.row_selected;
        w.active.bg_stroke = Stroke::new(1.0, self.accent);
        w.active.fg_stroke = Stroke::new(1.0, self.text_strong);
        w.active.expansion = 0.0;
        w.open.bg_fill = self.hover;
        w.open.weak_bg_fill = self.hover;
        w.open.bg_stroke = Stroke::new(1.0, self.rule);
        w.open.fg_stroke = Stroke::new(1.0, self.text_strong);
        for x in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
            x.corner_radius = r;
        }
        v.window_shadow = egui::Shadow::NONE;
        v.popup_shadow = egui::Shadow::NONE;
        v.striped = false;
        ctx.set_visuals(v);
        // the tokens for widgets that get none passed in (shell::Btn)
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique(crate::shell::TOKENS_KEY), *self));
        ctx.all_styles_mut(|s| {
            use egui::{FontFamily, FontId, TextStyle};
            s.text_styles = [
                (TextStyle::Small, FontId::new(12.0, FontFamily::Proportional)),
                (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
                (TextStyle::Button, FontId::new(14.0, FontFamily::Proportional)),
                // window titles and headings: 0.7's dialog heading (17 px bold)
                (TextStyle::Heading, FontId::new(17.0, FontFamily::Name(BOLD.into()))),
                (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
            ]
            .into();
            s.spacing.item_spacing = egui::vec2(8.0, 4.0);
            s.spacing.button_padding = egui::vec2(8.0, 3.0);
            s.spacing.interact_size = egui::vec2(40.0, 24.0);
            s.spacing.menu_margin = egui::Margin::same(4);
            s.spacing.window_margin = egui::Margin::same(16);
        });
    }
}

/// Relative luminance contrast ratio (WCAG).
pub fn contrast(a: Color32, b: Color32) -> f64 {
    fn lum(c: Color32) -> f64 {
        let f = |u: u8| {
            let s = f64::from(u) / 255.0;
            if s <= 0.03928 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
    }
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Font families beside egui's two: Archivo at 0.7's weights (600 for labels and buttons, 700 for headings) and
/// Archivo's 125 % width for the start screen's display type.
pub const SEMI: &str = "archivo-600";
pub const BOLD: &str = "archivo-700";
pub const WIDE: &str = "archivo-wide";
/// Archivo 700 at 112 % width: 0.7's panel section heads (`.cc-h`: `font-stretch: 112%`).
pub const HEAD: &str = "archivo-head";

/// Archivo SemiBold at `size` (0.7's tool labels, app-bar buttons, toggles).
pub fn semi(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(SEMI.into()))
}

/// Archivo Bold at `size` (0.7's headings, the app name, the command prompt).
pub fn bold(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(BOLD.into()))
}

/// Archivo Bold at 125 % width (the website's and the start screen's headings).
pub fn wide(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(WIDE.into()))
}

/// Archivo bold as a family (keeps the text's size): what `RichText::strong()` means in 0.7 (`<strong>`, `font-weight:
/// 700`); egui's strong only changes the colour.
pub fn bold_family() -> egui::FontFamily {
    egui::FontFamily::Name(BOLD.into())
}

/// The panel section heads' face (Archivo 700 at 112 % width).
pub fn head(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(HEAD.into()))
}

/// The UI fonts, all OFL and bundled (no system fonts): Archivo (0.7's face: one variable file, used at weight 500
/// for text, 600, 700 and 700 at 125 % width), Inter as the fallback for scripts Archivo lacks, JetBrains Mono for
/// code and values.
pub fn install_fonts(ctx: &egui::Context) {
    use egui::epaint::text::{FontTweak, VariationCoords};
    static ARCHIVO: &[u8] = include_bytes!("../../../assets/fonts/Archivo-Variable.ttf");
    let mut f = egui::FontDefinitions::empty();
    let archivo = |wght: f32, wdth: f32| {
        let tweak = FontTweak { coords: VariationCoords::new([("wght", wght), ("wdth", wdth)]), ..FontTweak::default() };
        std::sync::Arc::new(egui::FontData::from_static(ARCHIVO).tweak(tweak))
    };
    f.font_data.insert("archivo".into(), archivo(500.0, 100.0));
    f.font_data.insert(SEMI.into(), archivo(600.0, 100.0));
    f.font_data.insert(BOLD.into(), archivo(700.0, 100.0));
    f.font_data.insert(WIDE.into(), archivo(700.0, 125.0));
    f.font_data.insert(HEAD.into(), archivo(700.0, 112.0));
    f.font_data.insert(
        "inter".into(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../../../assets/fonts/Inter-Regular.ttf"))),
    );
    f.font_data.insert(
        "jbmono".into(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf"))),
    );
    let fam = |first: &str| vec![first.to_string(), "inter".into(), "jbmono".into()];
    f.families.insert(egui::FontFamily::Proportional, fam("archivo"));
    f.families.insert(egui::FontFamily::Monospace, vec!["jbmono".into(), "archivo".into(), "inter".into()]);
    for n in [SEMI, BOLD, WIDE, HEAD] {
        f.families.insert(egui::FontFamily::Name(n.into()), fam(n));
    }
    ctx.set_fonts(f);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presets_have_readable_text() {
        for p in Preset::ALL {
            let t = Tokens::preset(p);
            // WCAG: 4.5:1 for body text, 3:1 for icons and borders
            assert!(contrast(t.text, t.panel) >= 4.5, "{p:?} text");
            assert!(contrast(t.text_dim, t.panel) >= 4.5, "{p:?} dim");
            assert!(contrast(t.accent, t.panel) >= 4.5, "{p:?} links");
            assert!(contrast(t.on_accent, t.accent) >= 4.5, "{p:?} pressed toggles");
            assert!(contrast(t.ink, t.canvas_bg) >= 4.5, "{p:?} ink");
            assert!(contrast(t.line, t.panel) >= 3.0, "{p:?} borders");
            for (i, c) in t.layers.iter().enumerate() {
                assert!(contrast(*c, t.panel) >= 3.0, "{p:?} layer {i} icons");
            }
            assert_eq!(Preset::from_setting(p.setting()), Some(p));
        }
    }

    #[test]
    fn every_app_has_its_layer_colour() {
        let t = Tokens::preset(Preset::Light);
        assert_eq!(t.app("cadcraft"), hex(0xB3122E));
        assert_eq!(t.app("bimcraft"), hex(0x1E48C8));
        assert_eq!(t.app("studio"), t.accent);
        let d = Tokens::preset(Preset::Dark);
        assert_eq!(d.app("modelcraft"), hex(0xFF9A3C));
    }
}
