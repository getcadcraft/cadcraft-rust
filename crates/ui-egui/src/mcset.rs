//! Settings › ModelCraft (0.5's settings.inc, kept by modelcraft.js as `settings` in localStorage
//! `modelcraft.settings`): the controls (push-pull or modeler keys), scrolling (work it out, mouse wheel, trackpad),
//! the touch screen (two fingers pan or orbit), snapping and solids that stop where they touch, and the corner
//! indicator (view cube, axis icon, none). Words: ModelCraft's catalog (`set.*`). Changed through
//! `settings.set {name: "mc.<key>=<value>"}`; kept with the other settings (`settings.json` on the desktop, the
//! browser's storage on the web). The key style, Snap and Collide changed in the model's own menus and status bar are
//! the same settings (modelcraft.js setSetting), so they are kept too.

use serde_json::{Value, json};

use crate::UiApp;
use crate::modelcraft::mt;

/// modelcraft.js SET_DEFAULTS.
#[derive(Clone, Debug, PartialEq)]
pub struct McSet {
    /// "sketchup" (push-pull keys) | "blender" (modeler keys)
    pub keys: String,
    /// "auto" | "mouse" | "trackpad"
    pub scroll: String,
    /// "pan" | "orbit": what two fingers do (three do the other)
    pub touch2: String,
    pub snap: bool,
    pub collide: bool,
    /// "cube" | "axes" | "none"
    pub corner: String,
}

impl Default for McSet {
    fn default() -> Self {
        McSet { keys: "sketchup".into(), scroll: "auto".into(), touch2: "pan".into(), snap: true, collide: true, corner: "cube".into() }
    }
}

const SCROLLS: [&str; 3] = ["auto", "mouse", "trackpad"];
const TOUCH2: [&str; 2] = ["pan", "orbit"];
const CORNERS: [&str; 3] = ["cube", "axes", "none"];

impl McSet {
    /// As modelcraft.js stores them.
    pub fn json(&self) -> Value {
        json!({
            "keys": self.keys,
            "scroll": self.scroll,
            "touch2": self.touch2,
            "snap": self.snap,
            "collide": self.collide,
            "corner": self.corner,
        })
    }

    /// Read back with modelcraft.js's checks (anything else keeps the default).
    pub fn load(&mut self, v: &Value) {
        *self = McSet::default();
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("");
        if matches!(s("keys"), "blender" | "sketchup") {
            self.keys = s("keys").to_string();
        }
        if SCROLLS.contains(&s("scroll")) {
            self.scroll = s("scroll").to_string();
        }
        if TOUCH2.contains(&s("touch2")) {
            self.touch2 = s("touch2").to_string();
        }
        if let Some(b) = v.get("snap").and_then(Value::as_bool) {
            self.snap = b;
        }
        if let Some(b) = v.get("collide").and_then(Value::as_bool) {
            self.collide = b;
        }
        if CORNERS.contains(&s("corner")) {
            self.corner = s("corner").to_string();
        }
    }

    /// One setting by its stored name (`keys=blender`, `snap=false` …).
    pub fn set(&mut self, k: &str, v: &str) -> Result<(), String> {
        let bad = || format!("settings.set: mc.{k}={v} is not one of its values");
        let pick = |opts: &[&str]| if opts.contains(&v) { Ok(v.to_string()) } else { Err(bad()) };
        match k {
            "keys" => self.keys = pick(&["sketchup", "blender"])?,
            "scroll" => self.scroll = pick(&SCROLLS)?,
            "touch2" => self.touch2 = pick(&TOUCH2)?,
            "corner" => self.corner = pick(&CORNERS)?,
            "snap" | "collide" => {
                let b = match v {
                    "true" | "on" | "1" => true,
                    "false" | "off" | "0" => false,
                    _ => return Err(bad()),
                };
                if k == "snap" {
                    self.snap = b;
                } else {
                    self.collide = b;
                }
            }
            _ => return Err(format!("settings.set: no ModelCraft setting {k} (keys, scroll, touch2, snap, collide, corner)")),
        }
        Ok(())
    }
}

impl UiApp {
    /// Settings › ModelCraft as they stand: the open model's key style, Snap and Collide are the settings themselves.
    pub fn mcset_now(&self) -> McSet {
        let mut s = self.mcset.clone();
        if let Some(mc) = &self.mc {
            s.keys = mc.keys.clone();
            s.snap = mc.tools.snapping;
            s.collide = mc.tools.collide;
        }
        s
    }

    /// `settings.set mc.<k>=<v>`: kept, and the open model follows (the key style says so, as setKeys does).
    pub(crate) fn set_mc_setting(&mut self, k: &str, v: &str) -> Result<(), String> {
        let mut s = self.mcset_now();
        s.set(k, v)?;
        if let Some(mc) = self.mc.as_mut() {
            if k == "keys" {
                mc.set_keys(&s.keys);
            }
            mc.apply_settings(&s);
        }
        self.mcset = s;
        Ok(())
    }
}

impl crate::modelcraft::ModelView {
    /// modelcraft.js applySettings (the key style is set by set_keys, which also says so).
    pub fn apply_settings(&mut self, s: &McSet) {
        self.keys = s.keys.clone();
        self.tools.snapping = s.snap;
        self.tools.collide = s.collide;
        self.corner = s.corner.clone();
        self.scroll = s.scroll.clone();
        self.touch2 = s.touch2.clone();
    }
}

/// ModelCraft's sections of the Settings window (settings.inc's fieldsets), by their legend's key.
pub const SECTIONS: [&str; 5] = ["set.controls", "set.scroll", "set.touch2", "set.snap", "set.corner"];

/// A section's name (its legend).
pub fn section_title(key: &str) -> String {
    mt(key, &[])
}

/// One ModelCraft section of the Settings window (settings.inc; the window draws the head).
pub fn section(app: &UiApp, ui: &mut egui::Ui, key: &str, act: &mut Option<String>) {
    let t = app.tokens;
    let s = app.mcset_now();
    let hint = |ui: &mut egui::Ui, text: String| {
        ui.add_space(6.0);
        crate::shell::note(ui, &t, &text);
    };
    let radios = |ui: &mut egui::Ui, act: &mut Option<String>, key: &str, now: &str, opts: &[(&str, &str)]| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            for (v, label) in opts {
                if ui.radio(now == *v, mt(label, &[])).clicked() && now != *v {
                    *act = Some(format!("mc.{key}={v}"));
                }
            }
        });
    };
    match key {
        "set.controls" => {
            radios(ui, act, "keys", &s.keys, &[("sketchup", "set.controls.sketchup"), ("blender", "set.controls.blender")]);
            hint(ui, mt(if s.keys == "blender" { "app.keys.hint.blender" } else { "app.keys.hint.sketchup" }, &[]));
        }
        "set.scroll" => {
            radios(
                ui,
                act,
                "scroll",
                &s.scroll,
                &[("auto", "set.scroll.auto"), ("mouse", "set.scroll.mouse"), ("trackpad", "set.scroll.trackpad")],
            );
            hint(ui, mt("set.scroll.hint", &[]));
        }
        "set.touch2" => {
            radios(ui, act, "touch2", &s.touch2, &[("pan", "set.touch2.pan"), ("orbit", "set.touch2.orbit")]);
            hint(ui, mt("set.touch2.hint", &[]));
        }
        "set.snap" => {
            for (k, label, on) in [("snap", "set.snap.snap", s.snap), ("collide", "set.snap.collide", s.collide)] {
                let mut v = on;
                if ui.checkbox(&mut v, mt(label, &[])).changed() {
                    *act = Some(format!("mc.{k}={v}"));
                }
            }
            hint(ui, mt("set.snap.hint", &[]));
        }
        "set.corner" => {
            radios(ui, act, "corner", &s.corner, &[("cube", "set.corner.cube"), ("axes", "set.corner.axes"), ("none", "set.corner.none")]);
            hint(ui, mt("set.corner.hint", &[]));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::McSet;

    #[test]
    fn modelcraft_settings_round_trip_and_reject_bad_values() {
        let mut s = McSet::default();
        s.set("keys", "blender").unwrap();
        s.set("scroll", "trackpad").unwrap();
        s.set("touch2", "orbit").unwrap();
        s.set("snap", "false").unwrap();
        s.set("corner", "axes").unwrap();
        assert!(s.set("corner", "auto").is_err());
        assert!(s.set("nope", "1").is_err());
        let mut b = McSet::default();
        b.load(&s.json());
        assert_eq!(b, s);
        // a foreign value keeps the default, as modelcraft.js reads it
        b.load(&serde_json::json!({ "keys": "vim", "snap": "yes", "corner": "cube" }));
        assert_eq!(b, McSet::default());
    }
}
