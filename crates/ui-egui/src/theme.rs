//! Theme tokens: one flat struct of colour roles; widgets and the canvas read only these.

use egui::Color32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub app_bar: Color32,
    pub panel: Color32,
    pub panel_darker: Color32,
    pub divider: Color32,
    pub text: Color32,
    pub text_strong: Color32,
    pub text_dim: Color32,
    pub accent: Color32,
    pub row_selected: Color32,
    /// model space background
    pub canvas_bg: Color32,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Dark,
    Light,
}

impl Tokens {
    pub fn preset(p: Preset) -> Tokens {
        // CadCraft's own values: neutral chrome, the brand's blueprint blue as the accent.
        match p {
            Preset::Dark => Tokens {
                app_bar: Color32::from_rgb(0x26, 0x28, 0x2b),
                panel: Color32::from_rgb(0x2e, 0x30, 0x34),
                panel_darker: Color32::from_rgb(0x24, 0x26, 0x29),
                divider: Color32::from_rgb(0x44, 0x47, 0x4c),
                text: Color32::from_rgb(0xd6, 0xd8, 0xdb),
                text_strong: Color32::from_rgb(0xf4, 0xf5, 0xf6),
                text_dim: Color32::from_rgb(0x9a, 0x9e, 0xa4),
                accent: Color32::from_rgb(0x6f, 0x9f, 0xd8),
                row_selected: Color32::from_rgb(0x2f, 0x4a, 0x6b),
                canvas_bg: Color32::from_rgb(0x1b, 0x1c, 0x1e),
                desk: Color32::from_rgb(0x3a, 0x3c, 0x40),
                sheet: Color32::from_rgb(0x1f, 0x21, 0x24),
                ink: Color32::from_rgb(0xf0, 0xf0, 0xf0),
                sheet_ink: Color32::from_rgb(0xf0, 0xf0, 0xf0),
                viewport_frame: Color32::from_rgb(0x55, 0x59, 0x5f),
                radius: 2,
            },
            Preset::Light => Tokens {
                app_bar: Color32::from_rgb(0xe9, 0xea, 0xec),
                panel: Color32::from_rgb(0xf3, 0xf4, 0xf5),
                panel_darker: Color32::from_rgb(0xe2, 0xe4, 0xe7),
                divider: Color32::from_rgb(0xc8, 0xcb, 0xcf),
                text: Color32::from_rgb(0x2a, 0x2c, 0x30),
                text_strong: Color32::from_rgb(0x0f, 0x10, 0x12),
                text_dim: Color32::from_rgb(0x5d, 0x61, 0x67),
                accent: Color32::from_rgb(0x1f, 0x5f, 0xa8),
                row_selected: Color32::from_rgb(0xc9, 0xdc, 0xf2),
                canvas_bg: Color32::from_rgb(0xfa, 0xfa, 0xf8),
                desk: Color32::from_rgb(0xc4, 0xc7, 0xcb),
                sheet: Color32::from_rgb(0xff, 0xff, 0xff),
                ink: Color32::from_rgb(0x10, 0x10, 0x10),
                sheet_ink: Color32::from_rgb(0x10, 0x10, 0x10),
                viewport_frame: Color32::from_rgb(0x9a, 0x9e, 0xa4),
                radius: 2,
            },
        }
    }

    /// Fill egui's visuals from the tokens (once per theme change).
    pub fn apply(&self, ctx: &egui::Context, dark: bool) {
        let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        v.panel_fill = self.panel;
        v.window_fill = self.panel;
        v.extreme_bg_color = self.panel_darker;
        v.faint_bg_color = self.panel_darker;
        v.selection.bg_fill = self.row_selected;
        v.hyperlink_color = self.accent;
        v.override_text_color = Some(self.text);
        let r = egui::CornerRadius::same(self.radius);
        for w in
            [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open]
        {
            w.corner_radius = r;
        }
        v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, self.divider);
        v.window_shadow = egui::Shadow::NONE;
        v.popup_shadow = egui::Shadow::NONE;
        ctx.set_visuals(v);
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

/// The UI fonts: Inter (proportional) and JetBrains Mono (monospace), both OFL, bundled (no system fonts).
pub fn install_fonts(ctx: &egui::Context) {
    let mut f = egui::FontDefinitions::empty();
    f.font_data.insert(
        "inter".into(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../../../assets/fonts/Inter-Regular.ttf"))),
    );
    f.font_data.insert(
        "jbmono".into(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf"))),
    );
    f.families.insert(egui::FontFamily::Proportional, vec!["inter".into(), "jbmono".into()]);
    f.families.insert(egui::FontFamily::Monospace, vec!["jbmono".into(), "inter".into()]);
    ctx.set_fonts(f);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presets_have_readable_text() {
        for p in [Preset::Dark, Preset::Light] {
            let t = Tokens::preset(p);
            assert!(contrast(t.text, t.panel) >= 4.5, "{p:?} text");
            assert!(contrast(t.text_dim, t.panel) >= 3.0, "{p:?} dim");
            assert!(contrast(t.ink, t.canvas_bg) >= 4.5, "{p:?} ink");
        }
    }
}
