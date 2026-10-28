//! Settings › Menus, keys and mouse for CadCraft (0.5's shell `app-input-ui.js` over [`cc_engine::inputprof`]): the
//! styles (the three built-ins and a person's own), Customize… (a copy of a built-in, or one's own style edited: each
//! command's place in the menus / ribbon and its keys, recorded by pressing them; the mouse: wheel direction and
//! speed, which buttons pan the drawing, what each button does in 3D), Export (a `.ccinput.json`: desktop save, web
//! download), Import (checked as untrusted: what is not known is dropped and counted), Delete. The style's keys run
//! their commands (a sequence such as "W A" waits for its second key), the app's own key for a command the style
//! moved says where it went, text keys type at the command line (WL.18; CadCraft's SHORTCUTS keys are carried into a
//! style of one's own once). Every action is a UI command (`input.*`), so it works headless.
//!
//! Not here: "Use this style in every app" (IP.4: the other apps of this build have no input styles yet).

use egui::RichText;
use serde_json::Value;

use crate::ribbon::registry;
use crate::{Params, UiApp};
use cc_engine::inputprof::{self as ip, Press, Profile};

/// UI commands of the section and the Customize dialog.
pub const COMMANDS: &[(&str, &str, &str, &str)] = &[
    (
        "input.customize",
        "Menus, keys and mouse: Customize",
        "",
        "{} → the list of commands, places and keys of the style in use (a built-in is saved as a copy)",
    ),
    ("input.custom.name", "Customize: name", "", "{name: text} → the name the style is saved under"),
    (
        "input.custom.keys",
        "Customize: a command's keys",
        "",
        "{name: \"<command id>=<keys>|<keys>…\"} → its keys (\"Ctrl+K\", \"W A\"; empty = none; at most 3)",
    ),
    (
        "input.custom.place",
        "Customize: a command's place",
        "",
        "{name: \"<command id>=<where>\"} → \"\" keys only, \"<menu index>\" (menu bar), \"m<menu index>\" (a ribbon's File menu), \"<tab>/<panel>\"",
    ),
    (
        "input.custom.mouse",
        "Customize: the mouse",
        "",
        "{name: \"wheel=1|-1\" | \"speed=<0.25–4>\" | \"pan2d=<left,middle,right>\" | \"orbit3d=own|set\" | \"orbit3d.<left|middle|right>=orbit|pan|zoom|none\"}",
    ),
    ("input.custom.filter", "Customize: filter", "", "{name: text} → the commands whose name holds it"),
    ("input.custom.tab", "Customize: keys or mouse", "", "{name: keys|mouse}"),
    (
        "input.custom.record",
        "Customize: record a key",
        "",
        "{name: command id} → the next key pressed (or two in a row) is added to its keys",
    ),
    ("input.custom.save", "Customize: save", "", "{} → saves (refused while two keys clash)"),
    ("input.custom.cancel", "Customize: cancel", "", "{}"),
    ("input.export", "Menus, keys and mouse: Export", "", "{} → the style in use as a .ccinput.json (desktop: saved; web: downloaded)"),
    (
        "input.import",
        "Menus, keys and mouse: Import",
        "",
        "{name?: the file's text} → one of your own styles from a .ccinput.json (no name: the file picker)",
    ),
    ("input.delete", "Menus, keys and mouse: Delete", "", "{} → deletes the style in use (one of your own)"),
    ("input.key", "Press a key", "", "{name: key (\"Ctrl+K\", \"W\")} → as the style in use takes it"),
];

/// The Customize dialog's state (app-input-ui.js `customize()`).
#[derive(Clone, Debug)]
pub struct Draft {
    /// the style being changed, resolved
    pub p: Profile,
    /// it is a built-in: saving makes a copy of one's own
    pub builtin: bool,
    pub name: String,
    pub filter: String,
    pub mouse_tab: bool,
    /// recording a key for this command: (id, keys pressed so far, when the last came (s))
    pub recording: Option<(String, Vec<String>, f64)>,
}

/// app-input-ui.js `PASS`: keys a style never takes.
const PASS: [&str; 11] =
    ["Escape", "Enter", "Tab", "Delete", "Backspace", "Space", "Shift+Tab", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"];

fn shell(k: &str, vars: &[(&str, String)]) -> String {
    crate::lang::shell(k, vars)
}

/// app-input-ui.js `tx(k)`: a text key's words (the app's catalog, then the shell's), else the text as it is.
pub fn tx(k: &str) -> String {
    let w = crate::edit::lang_text(k, &[]);
    if w != k {
        return w;
    }
    let s = shell(k, &[]);
    if s != k { s } else { k.to_string() }
}

/// `nameOf(p)`.
pub fn name_of(p: &Profile) -> String {
    if p.builtin { tx(&p.name) } else { p.name.clone() }
}

fn now_ms() -> f64 {
    cc_clock::now_ms()
}

impl UiApp {
    /// The style in use, resolved.
    pub fn input_profile(&self) -> Profile {
        let reg = registry();
        let raw = ip::raw_style(&self.ribbon.st, reg, &self.ribbon.st.style).or_else(|| reg.styles.first());
        raw.map(|p| ip::resolve(p, reg)).unwrap_or_default()
    }
    /// The label of a command in the style in use.
    pub fn input_label(&self, p: &Profile, id: &str) -> String {
        tx(&p.label_key(id))
    }
}

/// `setStyle(id)`.
fn set_style(app: &mut UiApp, id: &str) -> Result<(), String> {
    let reg = registry();
    let Some(raw) = ip::raw_style(&app.ribbon.st, reg, id) else {
        let ids: Vec<String> = reg.styles.iter().map(|s| s.id.clone()).chain(app.ribbon.st.custom.iter().map(|c| c.id.clone())).collect();
        return Err(format!("input.style: name is one of {}", ids.join(", ")));
    };
    let name = name_of(raw);
    ip::pick_style(&mut app.ribbon.st, reg, id, now_ms());
    app.ribbon.open = None;
    app.ribbon.dirty = true;
    app.ribbon.matcher = None;
    app.message = shell("shell.input.now", &[("style", name)]);
    Ok(())
}

/// `importText(text)`.
fn import_text(app: &mut UiApp, text: &str) -> Result<(), String> {
    let reg = registry();
    match ip::import_profile(text, reg) {
        Err(_) => {
            app.message = shell("shell.input.import.bad", &[]);
            Err(app.message.clone())
        }
        Ok((mut p, problems)) => {
            if p.name.is_empty() {
                p.name = shell("shell.input.import.unnamed", &[]);
            }
            let id = ip::add_custom(&mut app.ribbon.st, &p);
            set_style(app, &id)?;
            app.message = if problems.is_empty() {
                shell("shell.input.import.ok", &[("name", p.name.clone())])
            } else {
                shell("shell.input.import.some", &[("name", p.name.clone()), ("n", problems.len().to_string())])
            };
            Ok(())
        }
    }
}

/// A file the picker read for Import (bytes: size checked first).
pub fn import_file(app: &mut UiApp, bytes: &[u8]) {
    if bytes.len() > ip::limits::BYTES {
        app.message = shell("shell.input.import.big", &[]);
        return;
    }
    let _ = import_text(app, &String::from_utf8_lossy(bytes));
}

/// The `input.*` commands of this module.
pub fn run(app: &mut UiApp, id: &str, params: &Params) -> Result<(), String> {
    let reg = registry();
    let name = params.name.clone().unwrap_or_default();
    let draft = |app: &mut UiApp| -> Result<(), String> {
        if app.ribbon.draft.is_none() {
            return Err(format!("{id}: Customize is not open (input.customize)"));
        }
        Ok(())
    };
    match id {
        "input.style" => set_style(app, &name)?,
        "input.customize" => {
            let p = app.input_profile();
            let builtin = ip::raw_style(&app.ribbon.st, reg, &app.ribbon.st.style).is_some_and(|r| r.builtin);
            let nm = if builtin { shell("shell.input.my", &[("style", name_of(&p))]) } else { p.name.clone() };
            app.ribbon.draft = Some(Draft { p, builtin, name: nm, filter: String::new(), mouse_tab: false, recording: None });
        }
        "input.custom.name" => {
            draft(app)?;
            if let Some(d) = app.ribbon.draft.as_mut() {
                d.name = cc_cad::js::slice16(&name, ip::limits::NAME);
            }
        }
        "input.custom.filter" => {
            draft(app)?;
            if let Some(d) = app.ribbon.draft.as_mut() {
                d.filter = name;
            }
        }
        "input.custom.tab" => {
            draft(app)?;
            if let Some(d) = app.ribbon.draft.as_mut() {
                d.mouse_tab = name == "mouse";
            }
        }
        "input.custom.keys" => {
            draft(app)?;
            let (cid, list) = name.split_once('=').ok_or("input.custom.keys: name = \"<command id>=<keys>|<keys>\"")?;
            let c = reg.commands.iter().find(|c| c.id == cid).ok_or_else(|| format!("input.custom.keys: no command {cid}"))?;
            if c.fixed {
                return Err(format!("input.custom.keys: {cid}'s key never moves"));
            }
            let keys: Vec<String> = list.split('|').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect();
            if let Some(bad) = keys.iter().find(|k| ip::norm_seq(k).is_none()) {
                return Err(format!("input.custom.keys: {bad} is not a key"));
            }
            if let Some(d) = app.ribbon.draft.as_mut() {
                d.p = ip::set_keys(&d.p, cid, &keys);
            }
        }
        "input.custom.place" => {
            draft(app)?;
            let (cid, place) = name.split_once('=').ok_or("input.custom.place: name = \"<command id>=<where>\"")?;
            if !reg.known(cid) {
                return Err(format!("input.custom.place: no command {cid}"));
            }
            if let Some(d) = app.ribbon.draft.as_mut() {
                d.p = ip::move_command(&d.p, cid, place);
            }
        }
        "input.custom.mouse" => {
            draft(app)?;
            let (k, v) = name.split_once('=').ok_or("input.custom.mouse: name = \"<setting>=<value>\"")?;
            let Some(d) = app.ribbon.draft.as_mut() else { return Ok(()) };
            let mut m = d.p.mouse.clone().unwrap_or_else(|| ip::clean_mouse(None));
            match k {
                "wheel" => m.wheel = if v == "-1" { -1 } else { 1 },
                "speed" => {
                    let s: f64 = v.trim().parse().map_err(|_| format!("input.custom.mouse: speed {v} is not a number"))?;
                    if !(0.25..=4.0).contains(&s) {
                        return Err(format!("input.custom.mouse: speed {v} is outside 0.25–4"));
                    }
                    m.zoom_speed = s;
                }
                "pan2d" => {
                    let want: Vec<&str> = v.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
                    if let Some(b) = want.iter().find(|b| !ip::BUTTONS.contains(b)) {
                        return Err(format!("input.custom.mouse: {b} is not left, middle or right"));
                    }
                    m.pan2d = ip::BUTTONS.iter().filter(|b| want.contains(b)).map(|b| (*b).to_string()).collect();
                }
                "orbit3d" => m.orbit3d = if v == "own" { None } else { ip::mouse_default().orbit3d },
                _ if k.starts_with("orbit3d.") => {
                    let b = k.trim_start_matches("orbit3d.");
                    let i = ip::BUTTONS.iter().position(|x| *x == b).ok_or_else(|| format!("input.custom.mouse: no button {b}"))?;
                    if !ip::ACTIONS_3D.contains(&v) {
                        return Err(format!("input.custom.mouse: {v} is not orbit, pan, zoom or none"));
                    }
                    let mut o = m.orbit3d.clone().or_else(|| ip::mouse_default().orbit3d).unwrap_or_default();
                    if let Some(slot) = o.get_mut(i) {
                        *slot = v.to_string();
                    }
                    m.orbit3d = Some(o);
                }
                _ => return Err(format!("input.custom.mouse: no setting {k}")),
            }
            d.p.mouse = Some(m);
        }
        "input.custom.record" => {
            draft(app)?;
            let c = reg.commands.iter().find(|c| c.id == name).ok_or_else(|| format!("input.custom.record: no command {name}"))?;
            if c.fixed {
                return Err(format!("input.custom.record: {name}'s key never moves"));
            }
            if let Some(d) = app.ribbon.draft.as_mut() {
                d.recording = Some((name, Vec::new(), 0.0));
            }
        }
        "input.custom.save" => {
            draft(app)?;
            let Some(d) = app.ribbon.draft.clone() else { return Ok(()) };
            if !ip::conflicts(&d.p, reg).is_empty() {
                return Err("input.custom.save: two keys clash (see the list)".into());
            }
            let cleaned: String = d.name.chars().filter(|c| !((*c as u32) < 0x20 || *c == '\u{7f}')).collect();
            let mut nm = cc_cad::js::slice16(cc_cad::js::trim(&cleaned), ip::limits::NAME);
            if nm.is_empty() {
                nm = shell("shell.input.my", &[("style", name_of(&d.p))]);
            }
            let mut p = d.p.clone();
            p.name.clone_from(&nm);
            p.builtin = false;
            p.added = true;
            p.base = if d.builtin { Some(d.p.id.clone()) } else { d.p.base.clone() };
            p.labels = Vec::new();
            if d.builtin {
                let nid = ip::add_custom(&mut app.ribbon.st, &p);
                ip::pick_style(&mut app.ribbon.st, reg, &nid, now_ms());
            } else {
                let cur = app.ribbon.st.style.clone();
                p.id.clone_from(&cur);
                for c in &mut app.ribbon.st.custom {
                    if c.id == cur {
                        *c = p.clone();
                    }
                }
            }
            app.ribbon.draft = None;
            app.ribbon.dirty = true;
            app.ribbon.matcher = None;
            app.message = shell("shell.input.dlg.saved", &[("style", nm)]);
        }
        "input.custom.cancel" => app.ribbon.draft = None,
        "input.export" => {
            let p = app.input_profile();
            let mut q = p.clone();
            q.name = name_of(&p);
            q.base = if p.builtin { Some(p.id.clone()) } else { p.base.clone() };
            let text = ip::export_profile(&q);
            let file = ip::export_name(&q.name);
            match app.services.save.as_mut() {
                Some(save) => {
                    save(&file, text.into_bytes())?;
                }
                None => return Err("input.export: this window cannot save files".into()),
            }
        }
        "input.import" => {
            if !name.is_empty() {
                return import_text(app, &name);
            }
            let pick = app.services.pick_files.as_mut().ok_or("input.import: this window cannot open files")?;
            match pick("input", false) {
                Some(files) => {
                    if let Some(f) = files.into_iter().next() {
                        import_file(app, &f.bytes);
                    }
                }
                None => app.ribbon.importing = true,
            }
        }
        "input.delete" => {
            let cur = app.ribbon.st.style.clone();
            let Some(i) = app.ribbon.st.custom.iter().position(|c| c.id == cur) else {
                return Err("input.delete: the style in use is a built-in".into());
            };
            let base = app.ribbon.st.custom.get(i).and_then(|c| c.base.clone()).unwrap_or_else(|| reg.own().to_string());
            app.ribbon.st.mine.retain(|(_, v)| *v != cur);
            app.ribbon.st.custom.remove(i);
            let to = if reg.builtin(&base).is_some() { base } else { reg.own().to_string() };
            set_style(app, &to)?;
        }
        "input.key" => {
            let combo = ip::norm_combo(&name).ok_or_else(|| format!("input.key: {name} is not a key"))?;
            press_combo(app, &combo, true);
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

/// Every frame: WL.18's carry (once), Import's file from the browser's picker, the wait for a sequence's second key,
/// a key being recorded.
pub fn sync(app: &mut UiApp, now: f64) {
    if !app.ribbon.carry_checked && app.editor.is_some() {
        // CadCraft's SHORTCUTS keys (kept as 0.5's `cadcraft.shortcuts`) go into a style of one's own once
        app.ribbon.carry_checked = true;
        let old = app.winset.prefs_json().get("shortcuts").cloned().unwrap_or(Value::Null);
        let name = shell("shell.input.carry.name", &[("app", "CadCraft".into())]);
        if let Some(id) = ip::carry_into(&mut app.ribbon.st, registry(), "cadcraft.shortcuts", &old, &name) {
            let n = ip::raw_style(&app.ribbon.st, registry(), &id).and_then(|p| p.macros.as_ref()).map_or(0, |m| m.len());
            let style = ip::raw_style(&app.ribbon.st, registry(), &id).map(name_of).unwrap_or_default();
            app.message = shell("shell.input.carry.done", &[("n", n.to_string()), ("style", style)]);
        }
        app.ribbon.dirty = true;
    }
    if app.ribbon.importing
        && let Some(files) = app.services.picked.as_ref().and_then(|p| p.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take())
    {
        app.ribbon.importing = false;
        if let Some(f) = files.into_iter().next() {
            import_file(app, &f.bytes);
        }
    }
    if app.ribbon.wait_until.is_some_and(|t| now > t) {
        app.ribbon.wait_until = None;
        if let Some(m) = app.ribbon.matcher.as_mut() {
            m.1.reset();
        }
        app.message.clear();
    }
    // a single key, unless a second follows within 900 ms
    let done = app.ribbon.draft.as_ref().and_then(|d| d.recording.as_ref()).is_some_and(|(_, got, t)| !got.is_empty() && now - t > 0.9);
    if done {
        finish_recording(app, true);
    }
}

fn finish_recording(app: &mut UiApp, ok: bool) {
    let Some(d) = app.ribbon.draft.as_mut() else { return };
    let Some((id, got, _)) = d.recording.take() else { return };
    if ok && !got.is_empty() {
        let mut keys = d.p.keys.as_ref().and_then(|k| k.get(&id).cloned()).unwrap_or_default();
        keys.push(got.join(" "));
        d.p = ip::set_keys(&d.p, &id, &keys);
    }
}

/// An egui key as app-input.js `eventCombo` writes it ("Ctrl+Z", "W", "F9"), or None.
pub fn combo_of(key: egui::Key, m: egui::Modifiers) -> Option<String> {
    use egui::Key as K;
    let punct = match key {
        K::Minus => Some("-"),
        K::Equals => Some("="),
        K::OpenBracket => Some("["),
        K::CloseBracket => Some("]"),
        K::Semicolon => Some(";"),
        K::Quote => Some("'"),
        K::Comma => Some(","),
        K::Period => Some("."),
        K::Slash => Some("/"),
        K::Backslash => Some("\\"),
        K::Backtick => Some("`"),
        _ => None,
    };
    let name: String = if let Some(p) = punct {
        // a shifted punctuation key types another character, which no style takes
        if m.shift {
            return None;
        }
        p.to_string()
    } else {
        match key {
            K::ArrowLeft => "ArrowLeft".into(),
            K::ArrowRight => "ArrowRight".into(),
            K::ArrowUp => "ArrowUp".into(),
            K::ArrowDown => "ArrowDown".into(),
            K::Space => "Space".into(),
            K::Escape => "Escape".into(),
            K::Tab => "Tab".into(),
            K::Enter => "Enter".into(),
            K::Backspace => "Backspace".into(),
            K::Delete => "Delete".into(),
            K::Insert => "Insert".into(),
            K::Home => "Home".into(),
            K::End => "End".into(),
            K::PageUp => "PageUp".into(),
            K::PageDown => "PageDown".into(),
            other => {
                let n = other.name();
                let one = n.chars().count() == 1 && n.chars().all(|c| c.is_ascii_alphanumeric());
                let f = n.strip_prefix('F').and_then(|x| x.parse::<u8>().ok()).is_some_and(|x| (1..=12).contains(&x));
                if one || f { n.to_ascii_uppercase() } else { return None }
            }
        }
    };
    let mut parts: Vec<String> = Vec::new();
    if m.ctrl || m.command || m.mac_cmd {
        parts.push("Ctrl".into());
    }
    if m.alt {
        parts.push("Alt".into());
    }
    if m.shift {
        parts.push("Shift".into());
    }
    parts.push(name);
    Some(parts.join("+"))
}

/// A key as the style in use takes it (app-input-ui.js's keydown listener). `forced`: from `input.key` (no focus
/// checks). → true when the style took it (the window must not see it).
pub fn press_combo(app: &mut UiApp, combo: &str, forced: bool) -> bool {
    let reg = registry();
    let p = app.input_profile();
    let sig = format!("{}#{}", app.ribbon.st.style, app.ribbon.st.custom.len());
    if app.ribbon.matcher.as_ref().is_none_or(|(s, _)| *s != sig) {
        app.ribbon.matcher = Some((sig, ip::KeyMatcher::new(&p, reg)));
    }
    let Some((_, m)) = app.ribbon.matcher.as_mut() else { return false };
    // the app's own style, unchanged: nothing at all happens
    if p.id == reg.own() && !m.has_text() {
        return false;
    }
    if PASS.contains(&combo) {
        m.reset();
        return false;
    }
    let plain = !(combo.starts_with("Ctrl+") || combo.starts_with("Alt+") || (combo.starts_with('F') && combo.len() > 1));
    // a running command, or a word typed at the command line, keeps plain keys
    let busy = app.editor.as_ref().is_some_and(|e| e.eng.busy() || !e.line.is_empty());
    if plain && busy && !forced {
        return false;
    }
    let r = m.press(combo);
    app.ribbon.wait_until = None;
    match r {
        None | Some(Press::Native(_)) => false,
        Some(Press::Wait(seq)) => {
            app.message = shell("shell.input.key.wait", &[("keys", ip::show_seq(&seq))]);
            app.ribbon.wait_until = Some(app.ribbon.clock + 1.5);
            true
        }
        Some(Press::Text(text)) => {
            app.message = shell("shell.input.key.text", &[("text", text.clone())]);
            crate::winsetdlg::run_shortcut_text(app, combo, &text);
            true
        }
        Some(Press::Run(id)) => {
            app.message.clear();
            if let Err(e) = app.run("input.run", &Params { name: Some(id), ..Default::default() }) {
                app.message = e;
            }
            true
        }
        Some(Press::Blocked(id)) => {
            let label = app.input_label(&p, &id);
            let k = m_keys_of(app, &id);
            app.message = match k {
                Some(k) => shell("shell.input.key.moved", &[("cmd", label), ("keys", ip::show_seq(&k)), ("style", name_of(&p))]),
                None => shell("shell.input.key.none", &[("cmd", label), ("style", name_of(&p)), ("app", "CadCraft".into())]),
            };
            true
        }
    }
}

fn m_keys_of(app: &UiApp, id: &str) -> Option<String> {
    app.ribbon.matcher.as_ref().and_then(|(_, m)| m.keys_of(id).first().cloned())
}

/// The window's raw input, before egui sees it: a key being recorded in Customize, then the style's keys. A key the
/// style takes is removed (with the text it would type).
pub fn key_hook(app: &mut UiApp, raw: &mut egui::RawInput) {
    if app.editor.is_none() {
        return;
    }
    let now = raw.time.unwrap_or(app.ribbon.clock);
    app.ribbon.clock = now;
    let mut i = 0;
    while i < raw.events.len() {
        let Some(egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. }) = raw.events.get(i).cloned() else {
            i += 1;
            continue;
        };
        let combo = combo_of(key, modifiers);
        // Customize: recording a command's key
        if let Some(d) = app.ribbon.draft.as_mut()
            && let Some((_, got, t)) = d.recording.as_mut()
        {
            raw.events.remove(i);
            drop_text_after(raw, i);
            let Some(c) = combo else { continue };
            if c == "Escape" && got.is_empty() {
                finish_recording(app, false);
                continue;
            }
            if PASS.contains(&c.as_str()) && c != "Delete" && c != "Backspace" && c != "Space" && got.is_empty() {
                finish_recording(app, false);
                continue;
            }
            let quick = c.starts_with("Ctrl+") || c.starts_with("Alt+") || (c.starts_with('F') && c.len() > 1);
            got.push(c);
            *t = now;
            if got.len() >= 2 || quick {
                finish_recording(app, true);
            }
            continue;
        }
        if app.ribbon.draft.is_some() || app.ribbon.keys_blocked {
            i += 1;
            continue;
        }
        let Some(c) = combo else {
            i += 1;
            continue;
        };
        if press_combo(app, &c, false) {
            raw.events.remove(i);
            drop_text_after(raw, i);
        } else {
            i += 1;
        }
    }
}

/// The text a removed key press would have typed (egui sends it after the key).
fn drop_text_after(raw: &mut egui::RawInput, i: usize) {
    if matches!(raw.events.get(i), Some(egui::Event::Text(t)) if t.chars().count() == 1) {
        raw.events.remove(i);
    }
}

/// Settings › Menus, keys and mouse: the styles, a word on the one in use, Customize… / Export / Import / Delete.
pub fn section(app: &UiApp, ui: &mut egui::Ui, act: &mut Vec<(&'static str, Option<String>)>) {
    let reg = registry();
    let t = app.tokens;
    ui.add_space(8.0);
    ui.separator();
    ui.label(RichText::new(shell("shell.input.legend", &[])).strong().color(t.text_strong));
    let cur = app.ribbon.st.style.clone();
    let list: Vec<&Profile> = reg.styles.iter().chain(app.ribbon.st.custom.iter()).collect();
    ui.horizontal_wrapped(|ui| {
        for p in &list {
            if ui.radio(p.id == cur, name_of(p)).clicked() && p.id != cur {
                act.push(("input.style", Some(p.id.clone())));
            }
        }
    });
    let p = app.input_profile();
    let raw_builtin = reg.builtin(&cur).is_some();
    let hint = if raw_builtin {
        tx(&format!("input.style.{cur}.hint"))
    } else {
        let base = p.base.as_deref().and_then(|b| reg.builtin(b)).map(name_of).unwrap_or_default();
        shell("shell.input.custom.hint", &[("base", base)])
    };
    ui.label(RichText::new(hint).small().color(t.text_dim));
    ui.horizontal_wrapped(|ui| {
        if ui.button(shell("shell.input.customize", &[])).clicked() {
            act.push(("input.customize", None));
        }
        if ui.button(shell("shell.input.export", &[])).clicked() {
            act.push(("input.export", None));
        }
        if ui.button(shell("shell.input.import", &[])).clicked() {
            act.push(("input.import", None));
        }
        if !raw_builtin && ui.button(shell("shell.input.delete", &[])).clicked() {
            act.push(("input.delete", None));
        }
    });
}

/// The Customize dialog (drawn while open).
pub fn window(app: &mut UiApp, ctx: &egui::Context) {
    let Some(d) = app.ribbon.draft.clone() else { return };
    let reg = registry();
    let t = app.tokens;
    let mut act: Vec<(&'static str, Option<String>)> = Vec::new();
    let mut open = true;
    let label = |id: &str| tx(&d.p.label_key(id));
    let conflicts = ip::conflicts(&d.p, reg);
    let unplaced = ip::unplaced(&d.p, reg);
    egui::Window::new(shell("shell.input.dlg.title", &[("style", name_of(&d.p))]))
        .id(egui::Id::unique("input_customize"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size(egui::vec2(640.0, 560.0))
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.label(
                RichText::new(shell(if d.builtin { "shell.input.dlg.copy" } else { "shell.input.dlg.edit" }, &[]))
                    .small()
                    .color(t.text_dim),
            );
            ui.horizontal(|ui| {
                let lab = ui.label(shell("shell.input.dlg.name", &[]));
                let mut n = d.name.clone();
                if ui
                    .add(egui::TextEdit::singleline(&mut n).char_limit(ip::limits::NAME).desired_width(260.0))
                    .labelled_by(lab.id)
                    .changed()
                {
                    act.push(("input.custom.name", Some(n)));
                }
            });
            ui.horizontal(|ui| {
                if ui.selectable_label(!d.mouse_tab, shell("shell.input.dlg.tab.keys", &[])).clicked() {
                    act.push(("input.custom.tab", Some("keys".into())));
                }
                if ui.selectable_label(d.mouse_tab, shell("shell.input.dlg.tab.mouse", &[])).clicked() {
                    act.push(("input.custom.tab", Some("mouse".into())));
                }
            });
            ui.separator();
            if d.mouse_tab {
                mouse_pane(ui, &t, &d, &mut act);
            } else {
                let mut f = d.filter.clone();
                if ui.add(egui::TextEdit::singleline(&mut f).hint_text(shell("shell.input.dlg.filter", &[])).desired_width(260.0)).changed()
                {
                    act.push(("input.custom.filter", Some(f)));
                }
                let lab = |x: &String| if let Some(s) = x.strip_prefix("text:") { s.to_string() } else { label(x) };
                for c in conflicts.iter().take(6) {
                    let msg = if c.kind == "prefix" {
                        shell(
                            "shell.input.dlg.prefix",
                            &[("keys", ip::show_seq(&c.seq)), ("longer", ip::show_seq(c.longer.as_deref().unwrap_or("")))],
                        )
                    } else {
                        shell(
                            "shell.input.dlg.clash",
                            &[("keys", ip::show_seq(&c.seq)), ("cmds", c.ids.iter().map(lab).collect::<Vec<_>>().join(", "))],
                        )
                    };
                    ui.label(RichText::new(format!("⚠ {msg}")).color(egui::Color32::from_rgb(0xe0, 0x80, 0x60)));
                }
                if !unplaced.is_empty() {
                    let cmds: Vec<String> = unplaced.iter().take(5).map(|x| label(x)).collect();
                    ui.label(
                        RichText::new(shell("shell.input.dlg.unplaced", &[("n", unplaced.len().to_string()), ("cmds", cmds.join(", "))]))
                            .small()
                            .color(t.text_dim),
                    );
                }
                keys_table(ui, &t, &d, &label, &mut act);
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(conflicts.is_empty(), egui::Button::new(shell("shell.input.dlg.save", &[]))).clicked() {
                        act.push(("input.custom.save", None));
                    }
                    if ui.button(shell("shell.input.dlg.cancel", &[])).clicked() {
                        act.push(("input.custom.cancel", None));
                    }
                });
            });
        });
    if !open || (d.recording.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Escape))) {
        act.push(("input.custom.cancel", None));
    }
    for (id, name) in act {
        if let Err(e) = app.run(id, &Params { name, ..Default::default() }) {
            app.message = e;
        }
    }
}

/// The commands by group: each one's place and keys.
fn keys_table(
    ui: &mut egui::Ui,
    t: &crate::theme::Tokens,
    d: &Draft,
    label: &dyn Fn(&str) -> String,
    act: &mut Vec<(&'static str, Option<String>)>,
) {
    let reg = registry();
    let q = d.filter.trim().to_lowercase();
    // the places a command can go: nowhere (keys only), a menu, a ribbon panel
    let mut places: Vec<(String, String)> = vec![(String::new(), shell("shell.input.dlg.nowhere", &[]))];
    if d.p.layout == "menubar" {
        for (i, m) in d.p.menus.iter().flatten().enumerate() {
            places.push((i.to_string(), tx(&m.label)));
        }
    }
    if d.p.layout == "ribbon" {
        for (i, m) in d.p.menus.iter().flatten().enumerate() {
            places.push((format!("m{i}"), tx(&m.label)));
        }
        for (ti, tb) in d.p.tabs.iter().flatten().enumerate() {
            for (pi, pn) in tb.panels.iter().enumerate() {
                places.push((format!("{ti}/{pi}"), format!("{} › {}", tx(&tb.label), tx(&pn.label))));
            }
        }
    }
    let place_val = |id: &str| -> String {
        if d.p.layout == "menubar" {
            return d.p.menus.iter().flatten().position(|m| m.items.iter().any(|x| x == id)).map(|i| i.to_string()).unwrap_or_default();
        }
        if d.p.layout == "ribbon" {
            if let Some(i) = d.p.menus.iter().flatten().position(|m| m.items.iter().any(|x| x == id)) {
                return format!("m{i}");
            }
            for (ti, tb) in d.p.tabs.iter().flatten().enumerate() {
                for (pi, pn) in tb.panels.iter().enumerate() {
                    if pn.items.iter().any(|x| x == id) {
                        return format!("{ti}/{pi}");
                    }
                }
            }
        }
        String::new()
    };
    egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, true]).show(ui, |ui| {
        egui::Grid::new("input_keys_grid").num_columns(3).striped(true).spacing(egui::vec2(10.0, 3.0)).show(ui, |ui| {
            ui.label(RichText::new(shell("shell.input.dlg.cmd", &[])).strong());
            ui.label(RichText::new(shell("shell.input.dlg.place", &[])).strong());
            ui.label(RichText::new(shell("shell.input.dlg.keys", &[])).strong());
            ui.end_row();
            for g in crate::ribbon::GROUPS {
                let cmds: Vec<&ip::RegCmd> = reg
                    .commands
                    .iter()
                    .filter(|c| crate::ribbon::cmd(&c.id).is_some_and(|x| x.group == *g))
                    .filter(|c| q.is_empty() || label(&c.id).to_lowercase().contains(&q) || c.id.contains(&q))
                    .collect();
                if cmds.is_empty() {
                    continue;
                }
                ui.label(RichText::new(tx(&format!("input.grp.{g}"))).strong().color(t.text_strong));
                ui.end_row();
                for c in cmds {
                    ui.label(label(&c.id));
                    if d.p.layout == "toolbar" || c.key_only {
                        ui.label(
                            RichText::new(if d.p.layout == "toolbar" { shell("shell.input.dlg.toolbar", &[]) } else { "—".into() })
                                .small()
                                .color(t.text_dim),
                        );
                    } else {
                        let now = place_val(&c.id);
                        let shown = places.iter().find(|(v, _)| *v == now).map(|(_, l)| l.clone()).unwrap_or_default();
                        egui::ComboBox::new(format!("input_place_{}", c.id), "").selected_text(shown).width(170.0).show_ui(ui, |ui| {
                            for (v, l) in &places {
                                if ui.selectable_label(*v == now, l).clicked() && *v != now {
                                    act.push(("input.custom.place", Some(format!("{}={v}", c.id))));
                                }
                            }
                        });
                    }
                    ui.horizontal(|ui| {
                        if c.fixed {
                            let k: Vec<String> = c.def_keys.iter().map(|k| ip::show_seq(k)).collect();
                            ui.label(RichText::new(k.join(", ")).monospace());
                            ui.label(RichText::new(shell("shell.input.dlg.fixed", &[])).small().color(t.text_dim));
                            return;
                        }
                        let keys = d.p.keys.as_ref().and_then(|k| k.get(&c.id).cloned()).unwrap_or_default();
                        for (i, k) in keys.iter().enumerate() {
                            let r = ui
                                .small_button(format!("{} ×", ip::show_seq(k)))
                                .on_hover_text(shell("shell.input.dlg.removekey", &[("keys", ip::show_seq(k))]));
                            if r.clicked() {
                                let rest: Vec<String> = keys.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, x)| x.clone()).collect();
                                act.push(("input.custom.keys", Some(format!("{}={}", c.id, rest.join("|")))));
                            }
                        }
                        let rec = d.recording.as_ref().filter(|(id, _, _)| *id == c.id);
                        if let Some((_, got, _)) = rec {
                            let txt = if got.is_empty() {
                                shell("shell.input.dlg.press", &[])
                            } else {
                                format!("{}…", got.iter().map(|g| ip::show_seq(g)).collect::<Vec<_>>().join(" "))
                            };
                            ui.label(RichText::new(txt).color(t.accent));
                        } else if keys.len() < ip::limits::KEYS_PER_COMMAND && ui.link(shell("shell.input.dlg.addkey", &[])).clicked() {
                            act.push(("input.custom.record", Some(c.id.clone())));
                        }
                    });
                    ui.end_row();
                }
            }
        });
    });
}

/// The mouse: the wheel, which buttons pan the drawing, what each button does in 3D.
fn mouse_pane(ui: &mut egui::Ui, t: &crate::theme::Tokens, d: &Draft, act: &mut Vec<(&'static str, Option<String>)>) {
    let m = d.p.mouse.clone().unwrap_or_else(|| ip::clean_mouse(None));
    let set = |act: &mut Vec<(&'static str, Option<String>)>, s: String| act.push(("input.custom.mouse", Some(s)));
    ui.label(RichText::new(shell("shell.input.mouse.wheel", &[])).strong());
    ui.horizontal(|ui| {
        let word = |w: i8| shell(if w < 0 { "shell.input.mouse.wheel.flip" } else { "shell.input.mouse.wheel.normal" }, &[]);
        egui::ComboBox::new("input_wheel", "").selected_text(word(m.wheel)).show_ui(ui, |ui| {
            for w in [1i8, -1] {
                if ui.selectable_label(m.wheel == w, word(w)).clicked() && m.wheel != w {
                    set(act, format!("wheel={w}"));
                }
            }
        });
        let lab = ui.label(shell("shell.input.mouse.speed", &[]));
        let mut s = m.zoom_speed;
        if ui.add(egui::Slider::new(&mut s, 0.25..=4.0).step_by(0.25).suffix("×")).labelled_by(lab.id).changed() {
            set(act, format!("speed={s}"));
        }
    });
    ui.add_space(6.0);
    ui.label(RichText::new(shell("shell.input.mouse.pan2d", &[])).strong());
    ui.horizontal(|ui| {
        for b in ip::BUTTONS {
            let mut on = m.pan2d.iter().any(|x| x == b);
            if ui.checkbox(&mut on, shell(&format!("shell.input.mouse.{b}"), &[])).changed() {
                let mut l: Vec<&str> = ip::BUTTONS.iter().copied().filter(|x| *x != b && m.pan2d.iter().any(|y| y == x)).collect();
                if on {
                    l.push(b);
                }
                let l: Vec<&str> = ip::BUTTONS.iter().copied().filter(|x| l.contains(x)).collect();
                set(act, format!("pan2d={}", l.join(",")));
            }
        }
    });
    ui.label(RichText::new(shell("shell.input.mouse.pan2d.hint", &[])).small().color(t.text_dim));
    ui.add_space(6.0);
    ui.label(RichText::new(shell("shell.input.mouse.3d", &[])).strong());
    let mut own = m.orbit3d.is_none();
    if ui.checkbox(&mut own, shell("shell.input.mouse.3d.own", &[])).changed() {
        set(act, format!("orbit3d={}", if own { "own" } else { "set" }));
    }
    let o = m.orbit3d.clone().or_else(|| ip::mouse_default().orbit3d).unwrap_or_default();
    ui.add_enabled_ui(m.orbit3d.is_some(), |ui| {
        ui.horizontal(|ui| {
            for (i, b) in ip::BUTTONS.iter().enumerate() {
                let now = o.get(i).cloned().unwrap_or_default();
                let lab = ui.label(shell(&format!("shell.input.mouse.{b}"), &[]));
                egui::ComboBox::new(format!("input_3d_{b}"), "")
                    .selected_text(shell(&format!("shell.input.mouse.act.{now}"), &[]))
                    .show_ui(ui, |ui| {
                        for a in ip::ACTIONS_3D {
                            if ui.selectable_label(now == a, shell(&format!("shell.input.mouse.act.{a}"), &[])).clicked() && now != a {
                                set(act, format!("orbit3d.{b}={a}"));
                            }
                        }
                    })
                    .response
                    .labelled_by(lab.id);
            }
        });
    });
    ui.label(RichText::new(shell("shell.input.mouse.3d.hint", &[])).small().color(t.text_dim));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(app: &mut UiApp, id: &str, name: &str) -> Result<(), String> {
        app.run(id, &Params { name: (!name.is_empty()).then(|| name.to_string()), ..Default::default() })
    }

    /// M4.55: a built-in copied with Customize, its keys and places, the keys at work, export / import / delete.
    #[test]
    fn customize_keys_export_import_delete() {
        cc_lang::with(cc_lang::Locale::En, || {
            let mut app = UiApp::default();
            app.run("file.new", &Params::default()).unwrap();
            assert!(run(&mut app, "input.custom.keys", "draw.line=W A").is_err(), "Customize is not open");
            run(&mut app, "input.style", "drafting").unwrap();
            run(&mut app, "input.customize", "").unwrap();
            assert!(run(&mut app, "input.custom.keys", "edit.delete=D").is_err(), "Delete never moves");
            assert!(run(&mut app, "input.custom.keys", "draw.line=not a key").is_err());
            run(&mut app, "input.custom.keys", "draw.line=W A").unwrap();
            run(&mut app, "input.custom.keys", "file.save=").unwrap();
            run(&mut app, "input.custom.place", "draw.line=").unwrap();
            run(&mut app, "input.custom.mouse", "wheel=-1").unwrap();
            run(&mut app, "input.custom.mouse", "pan2d=right").unwrap();
            assert!(run(&mut app, "input.custom.mouse", "speed=9").is_err());
            run(&mut app, "input.custom.name", "Mine").unwrap();
            // a clash refuses Save
            run(&mut app, "input.custom.keys", "draw.circle=W A").unwrap();
            assert!(run(&mut app, "input.custom.save", "").is_err());
            run(&mut app, "input.custom.keys", "draw.circle=").unwrap();
            run(&mut app, "input.custom.save", "").unwrap();
            assert_eq!(app.ribbon.st.style, "my-1");
            let p = app.input_profile();
            assert_eq!(p.name, "Mine");
            assert_eq!(p.base.as_deref(), Some("drafting"));
            assert_eq!(p.mouse.as_ref().map(|m| (m.wheel, m.pan2d.clone())), Some((-1, vec!["right".to_string()])));
            assert!(ip::place_of(&p, "draw.line").is_empty(), "keys only now");
            // the keys: W then A runs LINE; Ctrl+S moved away says so
            run(&mut app, "input.key", "W").unwrap();
            assert!(app.message.contains('W'), "{}", app.message);
            run(&mut app, "input.key", "A").unwrap();
            assert_eq!(app.editor.as_ref().and_then(|e| e.eng.running()), Some("LINE"));
            app.editor.as_mut().unwrap().escape();
            run(&mut app, "input.key", "Ctrl+S").unwrap();
            assert!(app.message.contains("Mine"), "{}", app.message);
            // export → import: the same style again, as a second one of my own
            let text = {
                let mut q = p.clone();
                q.name = name_of(&p);
                ip::export_profile(&q)
            };
            run(&mut app, "input.import", &text).unwrap();
            assert_eq!(app.ribbon.st.style, "my-2");
            assert_eq!(app.input_profile().keys, p.keys);
            assert!(run(&mut app, "input.import", "{not json").is_err());
            // delete: back to its base
            run(&mut app, "input.delete", "").unwrap();
            assert_eq!(app.ribbon.st.style, "drafting");
            assert!(run(&mut app, "input.delete", "").is_err(), "a built-in");
            // kept: the state reads back
            let mut b = UiApp::default();
            b.load_settings(&app.settings_json());
            assert_eq!(b.ribbon.st.custom.len(), 1);
        });
    }
}
