//! Settings › CadCraft (0.5's settings.inc, read and kept by cadcraft.js): the cursor's size (crosshair, pick box and
//! how far a click reaches), the paste point, what a click does to the selection (the Properties panel's PICKADD
//! button is the same setting), press and drag (lasso or rectangle, LASSOMODE's setting) and the sheet tabs (full
//! names, or sheet numbers only: sheetnum.js). Words: CadCraft's catalog (`set.*`). Changed through
//! `settings.set {name: "cad.<key>=<value>"}`; kept with the other settings (`settings.json` on the desktop, the
//! browser's storage on the web), as 0.5 keeps `cadcraft.settings` in localStorage.
//!
//! Not here: the corner indicator (0.5's axis icon on the Model tab, or the view cube there) and My tools.

use egui::RichText;
use serde_json::{Value, json};

use crate::UiApp;

/// view.js `CURSOR_SIZES`: (crosshair arm, pick box side, pick reach) in screen px.
pub const CURSOR_SIZES: [(&str, f32, f32, f64); 3] = [("small", 20.0, 7.0, 5.0), ("medium", 40.0, 9.0, 6.0), ("large", 90.0, 13.0, 8.0)];

/// What Settings › CadCraft keeps (cadcraft.js `settings` minus the drafting toggles).
#[derive(Clone, Debug, PartialEq)]
pub struct CadSet {
    /// "small" | "medium" | "large"
    pub cursor: String,
    /// a pasted copy hangs from its centre (else its lower-left corner)
    pub paste_center: bool,
    /// a click replaces the selection (else it adds: PICKADD on)
    pub pick_replace: bool,
    /// press and drag draws a rectangle (else a lasso)
    pub lasso_box: bool,
    /// sheet tabs show sheet numbers only
    pub tabs_number: bool,
    /// the lasso setting last handed to the editor (LASSOMODE changes it there)
    pushed_lasso: Option<bool>,
}

impl Default for CadSet {
    fn default() -> Self {
        CadSet {
            cursor: "medium".into(),
            paste_center: false,
            pick_replace: false,
            lasso_box: false,
            tabs_number: false,
            pushed_lasso: None,
        }
    }
}

impl CadSet {
    /// (arm, box, reach) of the cursor setting.
    pub fn cursor_spec(&self) -> (f32, f32, f64) {
        CURSOR_SIZES.iter().find(|c| c.0 == self.cursor).map_or((40.0, 9.0, 6.0), |c| (c.1, c.2, c.3))
    }
    /// As cadcraft.js stores them.
    pub fn json(&self) -> Value {
        json!({
            "cursor": self.cursor,
            "pasteBase": if self.paste_center { "center" } else { "lower-left" },
            "pick": if self.pick_replace { "replace" } else { "add" },
            "lasso": if self.lasso_box { "box" } else { "lasso" },
            "tabs": if self.tabs_number { "number" } else { "full" },
        })
    }
    /// Read back (cadcraft.js's checks: anything else keeps the default).
    pub fn load(&mut self, v: &Value) {
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("");
        let c = s("cursor");
        self.cursor = if CURSOR_SIZES.iter().any(|x| x.0 == c) { c.to_string() } else { "medium".into() };
        self.paste_center = s("pasteBase") == "center";
        self.pick_replace = s("pick") == "replace";
        self.lasso_box = s("lasso") == "box";
        self.tabs_number = s("tabs") == "number";
        self.pushed_lasso = None;
    }
    /// `settings.set {name: "cad.<key>=<value>"}`.
    pub fn set(&mut self, key: &str, v: &str) -> Result<(), String> {
        let bad = |allowed: &str| format!("settings.set: cad.{key} {v} is not one of {allowed}");
        match key {
            "cursor" if CURSOR_SIZES.iter().any(|x| x.0 == v) => self.cursor = v.to_string(),
            "cursor" => return Err(bad("small, medium, large")),
            "paste" => self.paste_center = choose(v, "center", "lower-left").ok_or_else(|| bad("lower-left, center"))?,
            "pick" => self.pick_replace = choose(v, "replace", "add").ok_or_else(|| bad("add, replace"))?,
            "lasso" => self.lasso_box = choose(v, "box", "lasso").ok_or_else(|| bad("lasso, box"))?,
            "tabs" => self.tabs_number = choose(v, "number", "full").ok_or_else(|| bad("full, number"))?,
            _ => return Err(format!("settings.set: no setting cad.{key} (cursor, paste, pick, lasso, tabs)")),
        }
        Ok(())
    }
}

fn choose(v: &str, yes: &str, no: &str) -> Option<bool> {
    if v == yes {
        Some(true)
    } else if v == no {
        Some(false)
    } else {
        None
    }
}

/// Hand the settings to the open drawing (every frame): what a click does, the paste point, the pick reach; the
/// lasso setting goes both ways (LASSOMODE sets it in the editor).
pub fn push(app: &mut UiApp) {
    let Some(ed) = app.editor.as_ref() else { return };
    let (_, _, reach) = app.cadset.cursor_spec();
    let mut h = ed.hs.borrow_mut();
    if app.cadset.pushed_lasso.is_some_and(|was| was != h.lasso_box) {
        app.cadset.lasso_box = h.lasso_box;
    }
    h.lasso_box = app.cadset.lasso_box;
    app.cadset.pushed_lasso = Some(h.lasso_box);
    h.pick_replace = app.cadset.pick_replace;
    h.paste_center = app.cadset.paste_center;
    h.pick_px = reach;
}

fn tr(k: &str) -> String {
    crate::edit::lang_text(k, &[])
}

/// The CadCraft section of the Settings window: each choice asks for `settings.set` through `act`.
pub fn section(app: &UiApp, ui: &mut egui::Ui, act: &mut Option<String>) {
    let t = app.tokens;
    let c = &app.cadset;
    let mut group = |ui: &mut egui::Ui, legend: &str, key: &str, now: &str, opts: &[(&str, &str)], hint: &str| {
        ui.add_space(6.0);
        ui.label(RichText::new(tr(legend)).color(t.text));
        ui.horizontal_wrapped(|ui| {
            for (v, label) in opts {
                if ui.radio(now == *v, tr(label)).clicked() && now != *v {
                    *act = Some(format!("cad.{key}={v}"));
                }
            }
        });
        ui.label(RichText::new(tr(hint)).small().color(t.text_dim));
    };
    ui.add_space(8.0);
    ui.separator();
    ui.label(RichText::new("CadCraft").strong().color(t.text_strong));
    group(
        ui,
        "set.cursor",
        "cursor",
        &c.cursor,
        &[("small", "set.small"), ("medium", "set.medium"), ("large", "set.large")],
        "set.crosshair_pick_box_drawing",
    );
    group(
        ui,
        "set.paste_point",
        "paste",
        if c.paste_center { "center" } else { "lower-left" },
        &[("lower-left", "set.lower_left_corner"), ("center", "set.center")],
        "set.where_pasted_copy_hangs",
    );
    group(
        ui,
        "set.clicking_objects",
        "pick",
        if c.pick_replace { "replace" } else { "add" },
        &[("add", "set.adds_selection"), ("replace", "set.replaces_selection")],
        "set.no_command_running_adds",
    );
    group(
        ui,
        "set.lasso",
        "lasso",
        if c.lasso_box { "box" } else { "lasso" },
        &[("lasso", "set.lasso.lasso"), ("box", "set.lasso.box")],
        "set.lasso.hint",
    );
    group(
        ui,
        "set.tabs",
        "tabs",
        if c.tabs_number { "number" } else { "full" },
        &[("full", "set.tabs.full"), ("number", "set.tabs.number")],
        "set.tabs.hint",
    );
}

/// sheetnum.js `sheetNumberOf(name, set)`: the open sheet set's number for that layout when it has one, else the
/// first word of the name when it reads like a sheet number (A-101, A1.01, S-2.3, E101, 12, M-201A), else None.
pub fn sheet_number_of(name: &str, set: &[(String, String)]) -> Option<String> {
    let n = name.trim_matches(|c: char| crate::cadset::js_space(c));
    if let Some((_, num)) = set.iter().find(|(lay, num)| lay == n && !num.is_empty()) {
        return Some(num.clone());
    }
    let ch: Vec<char> = n.chars().collect();
    number_prefix(&ch).map(|e| ch.get(..e).unwrap_or(&[]).iter().collect())
}

/// JS `\s` (String.prototype.trim's white space and line terminators).
pub fn js_space(c: char) -> bool {
    matches!(
        c,
        '\u{9}'..='\u{d}'
            | ' '
            | '\u{a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
            | '\u{feff}'
    )
}

/// The regex `^([A-Z]{0,4}[-.]?\d{1,5}(?:[.-]\d{1,4}){0,3}[A-Z]?)(?=$|[\s_:—–-]+\S)` (flag i) with JS's
/// backtracking order: the end of group 1, or None.
fn number_prefix(ch: &[char]) -> Option<usize> {
    let letter = |i: usize| ch.get(i).is_some_and(char::is_ascii_alphabetic);
    let digit = |i: usize| ch.get(i).is_some_and(char::is_ascii_digit);
    let sep = |i: usize| ch.get(i).is_some_and(|c| *c == '-' || *c == '.');
    // the lookahead after the group
    let ahead = |i: usize| -> bool {
        if i >= ch.len() {
            return true;
        }
        let mut j = i;
        while ch.get(j).is_some_and(|c| js_space(*c) || matches!(c, '_' | ':' | '—' | '–' | '-')) {
            j += 1;
            if ch.get(j).is_some_and(|c| !js_space(*c)) {
                return true;
            }
        }
        false
    };
    // [A-Z]?, then the lookahead
    let tail = |i: usize| -> Option<usize> {
        if letter(i) && ahead(i + 1) {
            return Some(i + 1);
        }
        ahead(i).then_some(i)
    };
    // (?:[.-]\d{1,4}){0,3} from i, `left` groups still allowed, greedy with backtracking
    fn groups(
        i: usize,
        left: u8,
        sep: &dyn Fn(usize) -> bool,
        digit: &dyn Fn(usize) -> bool,
        tail: &dyn Fn(usize) -> Option<usize>,
    ) -> Option<usize> {
        if left > 0 && sep(i) {
            let mut max = 0;
            while max < 4 && digit(i + 1 + max) {
                max += 1;
            }
            for k in (1..=max).rev() {
                if let Some(e) = groups(i + 1 + k, left - 1, sep, digit, tail) {
                    return Some(e);
                }
            }
        }
        tail(i)
    }
    let mut letters = 0;
    while letters < 4 && letter(letters) {
        letters += 1;
    }
    for a in (0..=letters).rev() {
        let starts: &[usize] = if sep(a) { &[a + 1, a] } else { &[a] };
        for &b in starts {
            let mut max = 0;
            while max < 5 && digit(b + max) {
                max += 1;
            }
            for k in (1..=max).rev() {
                if let Some(e) = groups(b + k, 3, &sep, &digit, &tail) {
                    return Some(e);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheet_numbers_as_sheetnum_js() {
        let n = |s: &str| sheet_number_of(s, &[]);
        // the cases sheetnum.js names, and what its regex gives (node, the frozen file)
        for (name, want) in [
            ("A-101 Floor plan", Some("A-101")),
            ("A1.01 - Site", Some("A1.01")),
            ("S-2.3", Some("S-2.3")),
            ("E101 Power", Some("E101")),
            ("12", Some("12")),
            ("M-201A Mechanical", Some("M-201A")),
            ("Floor plan", None),
            ("Layout1", None),
            ("A-101_Plan", Some("A-101")),
            ("A-101:", None),
            ("A-101 ", Some("A-101")),
            ("ABCDE1", None),
            ("A.1.2.3.4", Some("A.1.2.3.4")),
            ("1.2.3.4 x", Some("1.2.3.4")),
            ("X-12-34A — Detail", Some("X-12-34A")),
            ("  E-5   ", Some("E-5")),
            // checked against the JS on 20,000 random names (4,776 of them numbers) while porting
            ("b1-0é", Some("b1")),
            ("Q9.0_x", Some("Q9.0")),
        ] {
            assert_eq!(n(name).as_deref(), want, "{name}");
        }
        let set = vec![("Plan".to_string(), "A-1".to_string())];
        assert_eq!(sheet_number_of("Plan", &set).as_deref(), Some("A-1"));
    }

    #[test]
    fn settings_check_their_values() {
        let mut c = CadSet::default();
        c.set("cursor", "large").unwrap();
        c.set("pick", "replace").unwrap();
        c.set("paste", "center").unwrap();
        c.set("lasso", "box").unwrap();
        c.set("tabs", "number").unwrap();
        assert!(c.set("cursor", "huge").is_err());
        assert!(c.set("pick", "maybe").is_err());
        assert!(c.set("colour", "red").is_err());
        let mut d = CadSet::default();
        d.load(&c.json());
        assert_eq!(d, CadSet { pushed_lasso: None, ..c.clone() });
        d.load(&json!({"cursor": 3, "pick": "REPLACE"}));
        assert_eq!(d, CadSet::default());
        assert_eq!(c.cursor_spec(), (90.0, 13.0, 8.0));
    }
}
