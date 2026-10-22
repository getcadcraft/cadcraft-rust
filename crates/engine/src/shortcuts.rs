//! shortcuts.js — keys for any command (0.5 WL.18): one written form for a key combination, the keys CadCraft and the
//! app already use, and the stored keys read back as untrusted data. A key needs Ctrl (Cmd on a Mac counts as Ctrl) or
//! Alt, or is a free function key (F2, F4–F6, F9): plain letters go to the command line.

use serde_json::{Map, Value, json};

/// The store's name (localStorage `cadcraft.shortcuts` in 0.5; the window keeps the same JSON).
pub const SHORTCUTS_KEY: &str = "cadcraft.shortcuts";

/// Keys already taken by CadCraft, the app shell and the browser (`RESERVED`).
pub const RESERVED: &[&str] = &[
    "Ctrl+Z",
    "Ctrl+Y",
    "Ctrl+Shift+Z",
    "Ctrl+S",
    "Ctrl+Shift+S",
    "Ctrl+O",
    "Ctrl+A",
    "Ctrl+P",
    "Ctrl+C",
    "Ctrl+X",
    "Ctrl+V",
    "Ctrl+Shift+V",
    "Ctrl+D",
    "Ctrl+Shift+F",
    "Ctrl+,",
    "Ctrl+N",
    "Ctrl+T",
    "Ctrl+W",
    "Ctrl+Shift+T",
    "Ctrl+Shift+N",
    "Ctrl+Shift+W",
    "Ctrl+Q",
    "Ctrl+R",
    "Ctrl+Shift+R",
    "Ctrl+L",
    "Ctrl+F",
    "Ctrl+Tab",
    "Ctrl+Shift+Tab",
    "Ctrl+PageUp",
    "Ctrl+PageDown",
    "Alt+Left",
    "Alt+Right",
    "Alt+F4",
    "Ctrl+0",
    "Ctrl+=",
    "Ctrl+-",
    "F1",
    "F3",
    "F7",
    "F8",
    "F10",
    "F11",
    "F12",
];

fn named(u: &str) -> Option<&'static str> {
    Some(match u {
        "ESC" | "ESCAPE" => "Escape",
        "DEL" | "DELETE" => "Delete",
        "INS" | "INSERT" => "Insert",
        "HOME" => "Home",
        "END" => "End",
        "PGUP" | "PAGEUP" => "PageUp",
        "PGDN" | "PAGEDOWN" => "PageDown",
        "SPACE" => "Space",
        "TAB" => "Tab",
        "ENTER" | "RETURN" => "Enter",
        "LEFT" | "ARROWLEFT" => "Left",
        "RIGHT" | "ARROWRIGHT" => "Right",
        "UP" | "ARROWUP" => "Up",
        "DOWN" | "ARROWDOWN" => "Down",
        "BACKSPACE" => "Backspace",
        _ => return None,
    })
}

fn key_name(p: &str) -> Option<String> {
    let u = p.to_uppercase();
    let mut cs = u.chars();
    if let (Some(c), None) = (cs.next(), cs.next())
        && (c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return Some(u);
    }
    if let Some(n) = u.strip_prefix('F')
        && let Ok(k) = n.parse::<u32>()
        && (1..=12).contains(&k)
        && !n.starts_with('0')
        && !n.starts_with('+')
    {
        return Some(u);
    }
    if let Some(n) = named(&u) {
        return Some(n.into());
    }
    let mut cs = p.chars();
    if let (Some(c), None) = (cs.next(), cs.next())
        && ",.;'/\\[]=`-".contains(c)
    {
        return Some(p.into());
    }
    None
}

/// `raw.split(/\s*[+-]\s*(?=.)/)`: split at a "+" or "-" (with spaces round it) that has something after it.
fn split_parts(raw: &str) -> Vec<String> {
    let cs: Vec<char> = raw.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut i = 0usize;
    while i < cs.len() {
        // a separator: optional spaces, + or -, optional spaces, then at least one more character
        let mut j = i;
        while cs.get(j).is_some_and(|c| c.is_whitespace()) {
            j = j.saturating_add(1);
        }
        if cs.get(j).is_some_and(|c| *c == '+' || *c == '-') {
            let mut k = j.saturating_add(1);
            while cs.get(k).is_some_and(|c| c.is_whitespace()) {
                k = k.saturating_add(1);
            }
            // `(?=.)`: the lookahead after the (greedy, backtracking) spaces wants one non-newline character
            let mut kk = k;
            let mut ok = cs.get(kk).is_some_and(|c| *c != '\n');
            while !ok && kk > j.saturating_add(1) {
                kk = kk.saturating_sub(1);
                ok = cs.get(kk).is_some_and(|c| *c != '\n');
            }
            if ok {
                out.push(std::mem::take(&mut cur));
                i = kk;
                continue;
            }
        }
        if let Some(c) = cs.get(i) {
            cur.push(*c);
        }
        i = i.saturating_add(1);
    }
    out.push(cur);
    out
}

/// `normKey(s)`: 'ctrl+shift+l' / 'Shift-Ctrl-L' → 'Ctrl+Shift+L', or None (not a key CadCraft takes).
pub fn norm_key(s: &str) -> Option<String> {
    let raw = cc_cad::js::trim(s);
    if raw.is_empty() || raw.encode_utf16().count() > 40 {
        return None;
    }
    let mut parts: Vec<String> = split_parts(raw).into_iter().filter(|p| !p.is_empty()).collect();
    if raw.ends_with("++") || raw == "+" {
        parts.push("+".into());
    }
    let (mut ctrl, mut alt, mut shift) = (false, false, false);
    let mut key: Option<String> = None;
    for p in &parts {
        match p.to_uppercase().as_str() {
            "CTRL" | "CONTROL" | "CMD" | "COMMAND" | "META" | "MOD" => ctrl = true,
            "ALT" | "OPTION" | "OPT" => alt = true,
            "SHIFT" => shift = true,
            _ if key.is_none() => key = key_name(p),
            _ => return None,
        }
    }
    let key = key?;
    let mut v: Vec<String> = Vec::new();
    for (on, m) in [(ctrl, "Ctrl"), (alt, "Alt"), (shift, "Shift")] {
        if on {
            v.push(m.into());
        }
    }
    v.push(key);
    Some(v.join("+"))
}

/// `keyProblem(k, taken)`: why `k` can't be a key ('format' | 'plain' | 'reserved' | 'taken'), or None.
pub fn key_problem(k: &str, taken: &[(String, String)]) -> Option<&'static str> {
    let Some(n) = norm_key(k) else { return Some("format") };
    let parts: Vec<&str> = n.split('+').collect();
    let key = parts.last().copied().unwrap_or("");
    let mods = parts.get(..parts.len().saturating_sub(1)).unwrap_or(&[]);
    let fkey = key.strip_prefix('F').is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()));
    if !mods.contains(&"Ctrl") && !mods.contains(&"Alt") && !fkey {
        return Some("plain");
    }
    if RESERVED.contains(&n.as_str()) {
        return Some("reserved");
    }
    if taken.iter().any(|(t, _)| *t == n) {
        return Some("taken");
    }
    None
}

/// A refused stored key: the message key and its fills.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyError {
    pub key: String,
    pub fills: Vec<(String, String)>,
}

/// `cleanShortcuts(v, { known, taken })`: stored keys made safe → (key, text) in the stored order, and the refusals.
pub fn clean_shortcuts(v: &Value, known: &dyn Fn(&str) -> bool, taken: &[(String, String)]) -> (Vec<(String, String)>, Vec<KeyError>) {
    let mut keys: Vec<(String, String)> = Vec::new();
    let mut errors = Vec::new();
    let empty = Map::new();
    let src = match v {
        Value::Object(o) if o.get("format").and_then(Value::as_str) == Some("cadcraft-shortcuts") => {
            o.get("keys").and_then(Value::as_object).unwrap_or(&empty)
        }
        _ => &empty,
    };
    for (k0, t0) in src.iter().take(300) {
        let text0 = match t0 {
            Value::Null => String::new(),
            Value::String(s) => s.clone(),
            other => cc_cad::js::to_js_string(other),
        };
        let cleaned: String = text0.chars().map(|c| if (c as u32) < 0x20 || c == '\u{7f}' { ' ' } else { c }).collect();
        let text: String = cc_cad::js::slice16(cc_cad::js::trim(&cleaned), 200);
        let k = norm_key(k0);
        let all: Vec<(String, String)> = taken.iter().cloned().chain(keys.iter().cloned()).collect();
        let why = match &k {
            None => Some("format"),
            Some(k) => key_problem(k, &all),
        };
        if let Some(why) = why {
            let other = k.as_ref().and_then(|k| all.iter().find(|(t, _)| t == k).map(|(_, o)| o.clone())).unwrap_or_default();
            errors.push(KeyError {
                key: format!("keys.err.{why}"),
                fills: vec![("key".into(), cc_cad::js::slice16(k0, 40)), ("other".into(), other)],
            });
            continue;
        }
        let k = k.unwrap_or_default();
        let cmd = text.split_whitespace().next().unwrap_or("");
        if cmd.is_empty() || !known(cmd) {
            errors.push(KeyError {
                key: "keys.err.command".into(),
                fills: vec![("key".into(), k.clone()), ("cmd".into(), if cmd.is_empty() { "?".into() } else { cmd.into() })],
            });
            continue;
        }
        match keys.iter_mut().find(|(x, _)| *x == k) {
            Some(slot) => slot.1 = text,
            None => keys.push((k, text)),
        }
    }
    (keys, errors)
}

/// `shortcutsData(keys)`: what is stored.
pub fn shortcuts_data(keys: &[(String, String)]) -> Value {
    let m: Map<String, Value> = keys.iter().map(|(k, t)| (k.clone(), Value::String(t.clone()))).collect();
    json!({ "format": "cadcraft-shortcuts", "version": 1, "keys": m })
}
