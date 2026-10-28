//! The input styles' engine (0.5's shell `app-input.js`, IP.2–IP.5 and WL.18's carry) over CadCraft's registry
//! (cc-ui-egui ribbon.rs: `input-cadcraft.js`): profiles as data — a layout (toolbar, menu bar or ribbon), menus, the ribbon's
//! tabs and quick row, keys (sequences such as "W A"), typed words, text keys and the mouse — a person's own styles
//! (copies of a built-in with their changes), `.ccinput.json` export and import checked as untrusted, the key
//! matcher, and the state the window keeps (0.5's localStorage `cadcraft.input`). DOM-free and window-free: the
//! Settings section, the Customize dialog, the bar and the keys are in cc-ui-egui (ribbon.rs, inputdlg.rs).
//!
//! Spec gaps (written where they apply): a name cut in the middle of a UTF-16 pair loses the whole character (the
//! JS would keep half of it); JS objects put integer-like keys first ("1" before "A"): kept by [`JsMap`].

use serde_json::{Map, Value, json};

use cc_cad::js::{is_ws, slice16, to_js_string, to_number, trim};

pub const FORMAT: &str = "cadcraft-input";
pub const VERSION: i64 = 1;
pub const EXT: &str = ".ccinput.json";
pub const LAYOUTS: [&str; 3] = ["toolbar", "menubar", "ribbon"];
pub const BUTTONS: [&str; 3] = ["left", "middle", "right"];
pub const ACTIONS_3D: [&str; 4] = ["orbit", "pan", "zoom", "none"];
/// app-input.js `LIMITS`.
pub mod limits {
    pub const NAME: usize = 40;
    pub const LABEL: usize = 40;
    pub const MENUS: usize = 16;
    pub const ITEMS: usize = 80;
    pub const TABS: usize = 12;
    pub const PANELS: usize = 12;
    pub const KEYS_PER_COMMAND: usize = 3;
    pub const WORDS: usize = 4;
    pub const WORD: usize = 16;
    pub const MACROS: usize = 300;
    pub const MACRO: usize = 200;
    pub const CUSTOM: usize = 20;
    pub const BYTES: usize = 200_000;
}
pub const FAMILIES: [&str; 4] = ["own", "menus", "ribbon", "classic"];

const MODS: [&str; 3] = ["Ctrl", "Alt", "Shift"];
const NAMED: [&str; 27] = [
    "F1",
    "F2",
    "F3",
    "F4",
    "F5",
    "F6",
    "F7",
    "F8",
    "F9",
    "F10",
    "F11",
    "F12",
    "Delete",
    "Backspace",
    "Escape",
    "Enter",
    "Tab",
    "Space",
    "Insert",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "ArrowUp",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
];
const ALIAS: [(&str, &str); 17] = [
    ("esc", "Escape"),
    ("del", "Delete"),
    ("ins", "Insert"),
    ("return", "Enter"),
    ("pgup", "PageUp"),
    ("pgdn", "PageDown"),
    ("up", "ArrowUp"),
    ("down", "ArrowDown"),
    ("left", "ArrowLeft"),
    ("right", "ArrowRight"),
    (" ", "Space"),
    ("spacebar", "Space"),
    ("cmd", "Ctrl"),
    ("control", "Ctrl"),
    ("meta", "Ctrl"),
    ("mod", "Ctrl"),
    ("option", "Alt"),
];
pub const PUNCT: [&str; 11] = ["-", "=", "[", "]", ";", "'", ",", ".", "/", "\\", "`"];

// ---------------------------------------------------------------------------------------------- JS object order

/// Is `k` an array index (JS puts such keys first, in ascending order)?
fn index_key(k: &str) -> Option<u64> {
    if k.is_empty() || k.len() > 10 || !k.bytes().all(|b| b.is_ascii_digit()) || (k.len() > 1 && k.starts_with('0')) {
        return None;
    }
    k.parse::<u64>().ok().filter(|n| *n < 4_294_967_295)
}

/// A JS object of string keys: integer-like keys first (ascending), then insertion order; setting a key that is there
/// keeps its place.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct JsMap<V>(pub Vec<(String, V)>);

impl<V: Clone> JsMap<V> {
    pub fn new() -> Self {
        JsMap(Vec::new())
    }
    pub fn get(&self, k: &str) -> Option<&V> {
        self.0.iter().find(|(x, _)| x == k).map(|(_, v)| v)
    }
    pub fn has(&self, k: &str) -> bool {
        self.0.iter().any(|(x, _)| x == k)
    }
    pub fn set(&mut self, k: &str, v: V) {
        if let Some(slot) = self.0.iter_mut().find(|(x, _)| x == k) {
            slot.1 = v;
            return;
        }
        match index_key(k) {
            Some(n) => {
                let at = self.0.iter().position(|(x, _)| index_key(x).is_none_or(|m| m > n)).unwrap_or(self.0.len());
                self.0.insert(at, (k.to_string(), v));
            }
            None => self.0.push((k.to_string(), v)),
        }
    }
    /// A JS `Map`'s set: a new key goes last whatever it looks like.
    pub fn insert_last(&mut self, k: &str, v: V) {
        match self.0.iter_mut().find(|(x, _)| x == k) {
            Some(slot) => slot.1 = v,
            None => self.0.push((k.to_string(), v)),
        }
    }
    pub fn remove(&mut self, k: &str) {
        self.0.retain(|(x, _)| x != k);
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// `Object.entries(o)` of a parsed JSON object (or array: its indexes), in JS order.
fn js_entries(v: &Value) -> Vec<(String, &Value)> {
    match v {
        Value::Array(a) => a.iter().enumerate().map(|(i, x)| (i.to_string(), x)).collect(),
        Value::Object(m) => {
            let mut idx: Vec<(u64, String, &Value)> = Vec::new();
            let mut rest: Vec<(String, &Value)> = Vec::new();
            for (k, x) in m {
                match index_key(k) {
                    Some(n) => idx.push((n, k.clone(), x)),
                    None => rest.push((k.clone(), x)),
                }
            }
            idx.sort_by_key(|e| e.0);
            idx.into_iter().map(|(_, k, x)| (k, x)).chain(rest).collect()
        }
        Value::String(s) => s.chars().enumerate().map(|(i, _)| (i.to_string(), v)).collect(),
        _ => Vec::new(),
    }
}

fn is_object(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Object(_) | Value::Array(_)))
}

// ---------------------------------------------------------------------------------------------- keys

/// `normCombo(s)`: one key with its modifiers, written the one way ("Ctrl+Shift+V", "F9", "W"), or None.
pub fn norm_combo(s: &str) -> Option<String> {
    let raw = trim(s);
    if raw.is_empty() || raw.encode_utf16().count() > 40 {
        return None;
    }
    let parts: Vec<&str> = if raw == "+" { vec!["+"] } else { raw.split('+').map(trim).collect() };
    if parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    let mut mods = [false; 3];
    let mut key: Option<String> = None;
    for p in parts {
        let low = p.to_lowercase();
        let a = ALIAS.iter().find(|(k, _)| *k == low).map(|(_, v)| *v);
        let want = a.map_or_else(|| low.clone(), str::to_lowercase);
        if let Some(i) = MODS.iter().position(|m| m.to_lowercase() == want) {
            if let Some(slot) = mods.get_mut(i) {
                *slot = true;
            }
            continue;
        }
        if key.is_some() {
            return None;
        }
        let mut ch = p.chars();
        key = if let Some(a) = a {
            Some(a.to_string())
        } else if let (Some(c), None) = (ch.next(), ch.next())
            && c.is_ascii_alphanumeric()
        {
            Some(c.to_ascii_uppercase().to_string())
        } else if PUNCT.contains(&p) {
            Some(p.to_string())
        } else {
            NAMED.iter().find(|n| n.to_lowercase() == low).map(|n| (*n).to_string())
        };
        key.as_ref()?;
    }
    let key = key?;
    let mut out: Vec<String> = MODS.iter().zip(mods).filter(|(_, on)| *on).map(|(m, _)| (*m).to_string()).collect();
    out.push(key);
    Some(out.join("+"))
}

/// `normSeq(s)`: a sequence ("W A", "WA", "Ctrl+K Ctrl+S") written the one way ("W A"), or None. Two to four letters
/// written together are letters in a row; anything else is split on white space.
pub fn norm_seq(s: &str) -> Option<String> {
    let t = trim(s);
    if t.is_empty() {
        return None;
    }
    let letters = (2..=4).contains(&t.len()) && t.bytes().all(|b| b.is_ascii_alphabetic());
    let parts: Vec<String> = if letters {
        t.to_ascii_uppercase().chars().map(|c| c.to_string()).collect()
    } else {
        t.split(is_ws).filter(|p| !p.is_empty()).map(str::to_string).collect()
    };
    if parts.len() > 4 {
        return None;
    }
    let mut out = Vec::new();
    for p in parts {
        out.push(norm_combo(&p)?);
    }
    Some(out.join(" "))
}

/// `normSeq` of a JSON value (anything but a string = None).
fn norm_seq_v(v: &Value) -> Option<String> {
    v.as_str().and_then(norm_seq)
}

/// `showSeq(seq)`: letters run together ("WA"), anything else spaced.
pub fn show_seq(seq: &str) -> String {
    let p: Vec<&str> = seq.split(' ').collect();
    let plain = p.iter().all(|x| x.len() == 1 && x.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()));
    p.join(if plain { "" } else { " " })
}

/// `isTyping(combo)`: a letter or digit with no Ctrl / Alt.
pub fn is_typing(combo: &str) -> bool {
    let k = combo.strip_prefix("Shift+").unwrap_or(combo);
    k.len() == 1 && k.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

/// app-input.js `clean(v, n)`: control characters → spaces, white space runs → one space, trimmed, cut to n.
fn clean(v: &Value, n: usize) -> String {
    let Some(s) = v.as_str() else { return String::new() };
    let mut out = String::new();
    let mut space = false;
    for c in s.chars() {
        let c = if (c as u32) < 0x20 || c == '\u{7f}' { ' ' } else { c };
        if is_ws(c) {
            space = true;
            continue;
        }
        if space && !out.is_empty() {
            out.push(' ');
        }
        space = false;
        out.push(c);
    }
    slice16(&out, n)
}

/// validateProfile's `str(v, n)`: control characters removed, trimmed, cut to n.
fn str_of(v: &Value, n: usize) -> String {
    let Some(s) = v.as_str() else { return String::new() };
    let kept: String = s.chars().filter(|c| !((*c as u32) < 0x20 || *c == '\u{7f}')).collect();
    slice16(trim(&kept), n)
}

// ---------------------------------------------------------------------------------------------- profiles

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Menu {
    pub label: String,
    pub items: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Default)]
pub struct RPanel {
    pub label: String,
    pub items: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Default)]
pub struct RTab {
    pub label: String,
    pub panels: Vec<RPanel>,
}

/// The mouse (`cleanMouse`'s shape: every field present and in range).
#[derive(Clone, Debug, PartialEq)]
pub struct Mouse {
    /// 1 or -1 (the wheel the other way)
    pub wheel: i8,
    /// 0.25 – 4
    pub zoom_speed: f64,
    /// buttons that pan 2D views
    pub pan2d: Vec<String>,
    /// what each button (left, middle, right) does in 3D views; None = each view keeps its own
    pub orbit3d: Option<[String; 3]>,
}

pub fn mouse_default() -> Mouse {
    Mouse {
        wheel: 1,
        zoom_speed: 1.0,
        pan2d: vec!["middle".into(), "right".into()],
        orbit3d: Some(["orbit".into(), "zoom".into(), "pan".into()]),
    }
}

/// A style (built-in or a person's own). `None` = the field is absent (a person's own then takes its base's).
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub id: String,
    /// a built-in's text key, or a person's own name
    pub name: String,
    pub base: Option<String>,
    pub builtin: bool,
    /// added or saved in this run (addCustom / Customize: the JS object then carries `builtin: false` until it is
    /// read back)
    pub added: bool,
    pub layout: String,
    /// absent in the CadCraft and Ribbon built-ins (an export then has no `commandLine`)
    pub command_line: Option<bool>,
    pub menus: Option<Vec<Menu>>,
    pub bar: Option<Vec<String>>,
    pub quick: Option<Vec<String>>,
    pub tabs: Option<Vec<RTab>>,
    /// built-ins only: command → label key
    pub labels: Vec<(String, String)>,
    pub keys: Option<JsMap<Vec<String>>>,
    pub typed: Option<JsMap<Vec<String>>>,
    pub macros: Option<JsMap<String>>,
    pub mouse: Option<Mouse>,
}

impl Default for Profile {
    fn default() -> Self {
        Profile {
            id: String::new(),
            name: String::new(),
            base: None,
            builtin: false,
            added: false,
            layout: "toolbar".into(),
            command_line: None,
            menus: None,
            bar: None,
            quick: None,
            tabs: None,
            labels: Vec::new(),
            keys: None,
            typed: None,
            macros: None,
            mouse: None,
        }
    }
}

/// One command of a registry, as the engine needs it.
#[derive(Clone, Debug)]
pub struct RegCmd {
    pub id: String,
    pub def_keys: Vec<String>,
    pub fixed: bool,
    pub key_only: bool,
    pub optional: bool,
}

/// The registry and its built-in styles.
#[derive(Clone, Debug)]
pub struct Registry {
    pub commands: Vec<RegCmd>,
    pub styles: Vec<Profile>,
    pub family: Vec<(String, String)>,
    pub macros: bool,
}

impl Registry {
    pub fn known(&self, id: &str) -> bool {
        self.commands.iter().any(|c| c.id == id)
    }
    pub fn builtin(&self, id: &str) -> Option<&Profile> {
        self.styles.iter().find(|s| s.id == id)
    }
    pub fn own(&self) -> &str {
        self.styles.first().map_or("", |s| s.id.as_str())
    }
}

fn mouse_json(m: &Mouse) -> Value {
    let mut o = Map::new();
    o.insert("wheel".into(), json!(m.wheel));
    o.insert("zoomSpeed".into(), jnum(m.zoom_speed));
    o.insert("pan2d".into(), json!(m.pan2d));
    o.insert(
        "orbit3d".into(),
        match &m.orbit3d {
            Some([l, mi, r]) => json!({"left": l, "middle": mi, "right": r}),
            None => Value::Null,
        },
    );
    Value::Object(o)
}

/// A number as JSON.stringify writes it (integers without ".0").
fn jnum(x: f64) -> Value {
    #[allow(clippy::cast_possible_truncation)] // whole and within ±9e15: exact as an i64
    if x.is_finite() && x.fract() == 0.0 && x.abs() < 9.0e15 { json!(x as i64) } else { json!(x) }
}

fn menus_json(m: &[Menu]) -> Value {
    Value::Array(m.iter().map(|x| json!({"label": x.label, "items": x.items})).collect())
}
fn tabs_json(t: &[RTab]) -> Value {
    Value::Array(
        t.iter()
            .map(|x| json!({"label": x.label, "panels": x.panels.iter().map(|p| json!({"label": p.label, "items": p.items})).collect::<Vec<_>>()}))
            .collect(),
    )
}
fn lists_json(m: &JsMap<Vec<String>>) -> Value {
    Value::Object(m.0.iter().map(|(k, v)| (k.clone(), json!(v))).collect())
}

impl Profile {
    /// The profile as the JS keeps it (validateProfile's key order; `builtin` only when true).
    pub fn to_json(&self) -> Value {
        let mut o = Map::new();
        o.insert("format".into(), json!(FORMAT));
        o.insert("version".into(), json!(VERSION));
        o.insert("id".into(), json!(self.id));
        o.insert("name".into(), json!(self.name));
        if let Some(b) = &self.base {
            o.insert("base".into(), json!(b));
        }
        o.insert("layout".into(), json!(self.layout));
        if let Some(c) = self.command_line {
            o.insert("commandLine".into(), json!(c));
        }
        self.put_parts(&mut o);
        if self.builtin || self.added {
            o.insert("builtin".into(), json!(self.builtin));
        }
        Value::Object(o)
    }
    fn put_parts(&self, o: &mut Map<String, Value>) {
        if let Some(m) = &self.menus {
            o.insert("menus".into(), menus_json(m));
        }
        if let Some(b) = &self.bar {
            o.insert("bar".into(), json!(b));
        }
        if let Some(q) = &self.quick {
            o.insert("quick".into(), json!(q));
        }
        if let Some(t) = &self.tabs {
            o.insert("tabs".into(), tabs_json(t));
        }
        if let Some(k) = &self.keys {
            o.insert("keys".into(), lists_json(k));
        }
        if let Some(t) = &self.typed {
            o.insert("typed".into(), lists_json(t));
        }
        if let Some(m) = &self.macros {
            o.insert("macros".into(), Value::Object(m.0.iter().map(|(k, v)| (k.clone(), json!(v))).collect()));
        }
        if let Some(m) = &self.mouse {
            o.insert("mouse".into(), mouse_json(m));
        }
    }
    /// The label key of a command in this style (its override, else `input.cmd.<id>`).
    pub fn label_key(&self, id: &str) -> String {
        self.labels.iter().find(|(c, _)| c == id).map_or_else(|| format!("input.cmd.{id}"), |(_, k)| k.clone())
    }
}

/// `cleanMouse(m)`.
pub fn clean_mouse(m: Option<&Value>) -> Mouse {
    let o = m.filter(|v| is_object(Some(v)));
    let get = |k: &str| o.and_then(|v| v.get(k));
    let pan2d = match get("pan2d") {
        Some(Value::Array(a)) => {
            let mut out: Vec<String> = Vec::new();
            for b in a.iter().filter_map(Value::as_str).filter(|b| BUTTONS.contains(b)) {
                if !out.iter().any(|x| x == b) {
                    out.push(b.to_string());
                }
            }
            out
        }
        _ => mouse_default().pan2d,
    };
    let def3 = ["orbit", "zoom", "pan"];
    let orbit3d = match get("orbit3d") {
        Some(v @ (Value::Object(_) | Value::Array(_))) => {
            let one = |i: usize, b: &str| -> String {
                let x = v.get(b).and_then(Value::as_str).filter(|a| ACTIONS_3D.contains(a));
                x.map_or_else(|| def3.get(i).copied().unwrap_or("none").to_string(), str::to_string)
            };
            Some([one(0, "left"), one(1, "middle"), one(2, "right")])
        }
        _ => None,
    };
    let zs = get("zoomSpeed").map_or(f64::NAN, to_number);
    Mouse {
        wheel: if get("wheel").and_then(Value::as_f64) == Some(-1.0) { -1 } else { 1 },
        zoom_speed: if zs.is_finite() { zs.clamp(0.25, 4.0) } else { 1.0 },
        pan2d,
        orbit3d,
    }
}

/// `resolve(p, builtins)`: a person's own profile with what it leaves out taken from its base built-in.
pub fn resolve(p: &Profile, reg: &Registry) -> Profile {
    let base = p.base.as_deref().and_then(|b| reg.builtin(b));
    let Some(base) = base else {
        let mut out = p.clone();
        out.mouse = Some(p.mouse.clone().unwrap_or_else(|| clean_mouse(None)));
        return out;
    };
    let pick = |a: &Option<Vec<String>>, b: &Option<Vec<String>>| a.clone().or_else(|| b.clone());
    Profile {
        id: p.id.clone(),
        name: p.name.clone(),
        base: p.base.clone(),
        builtin: false,
        added: p.added,
        layout: p.layout.clone(),
        command_line: p.command_line.or(base.command_line),
        menus: p.menus.clone().or_else(|| base.menus.clone()),
        bar: pick(&p.bar, &base.bar),
        quick: pick(&p.quick, &base.quick),
        tabs: p.tabs.clone().or_else(|| base.tabs.clone()),
        labels: base.labels.clone(),
        keys: p.keys.clone().or_else(|| base.keys.clone()),
        typed: p.typed.clone().or_else(|| base.typed.clone()),
        macros: p.macros.clone().or_else(|| base.macros.clone()),
        mouse: Some(p.mouse.clone().or_else(|| base.mouse.clone()).unwrap_or_else(|| clean_mouse(None))),
    }
}

/// `bindings(p)`: sequence → command (the first command wins a clash; a JS Map: insertion order).
pub fn bindings(p: &Profile) -> JsMap<String> {
    let mut m = JsMap::new();
    for (id, list) in p.keys.iter().flat_map(|k| k.0.iter()) {
        for s in list {
            if let Some(q) = norm_seq(s)
                && !m.has(&q)
            {
                m.insert_last(&q, id.clone());
            }
        }
    }
    m
}

/// `macroMap(p)`: sequence → the text it types (WL.18).
pub fn macro_map(p: &Profile) -> JsMap<String> {
    let mut m = JsMap::new();
    for (k, text) in p.macros.iter().flat_map(|x| x.0.iter()) {
        if let Some(q) = norm_seq(k)
            && !text.is_empty()
            && !m.has(&q)
        {
            m.insert_last(&q, text.clone());
        }
    }
    m
}

/// One problem of `conflicts(p, registry)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Conflict {
    /// "same" | "fixed" | "prefix"
    pub kind: &'static str,
    pub seq: String,
    pub longer: Option<String>,
    pub ids: Vec<String>,
}

/// `conflicts(p, registry)`: the same sequence on two commands (or a command and a text key), a fixed key taken, a
/// key that also starts a longer sequence.
pub fn conflicts(p: &Profile, reg: &Registry) -> Vec<Conflict> {
    let mut fixed: JsMap<String> = JsMap::new();
    for c in reg.commands.iter().filter(|c| c.fixed) {
        for k in &c.def_keys {
            let q = norm_seq(k).unwrap_or_default();
            fixed.set(&q, c.id.clone());
        }
    }
    // a Map keeps insertion order whatever the key (not a plain object)
    let mut by: Vec<(String, Vec<String>)> = Vec::new();
    let mut put = |q: Option<String>, id: String| {
        let Some(q) = q else { return };
        match by.iter_mut().find(|(s, _)| *s == q) {
            Some((_, ids)) => {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            None => by.push((q, vec![id])),
        }
    };
    for (id, list) in p.keys.iter().flat_map(|k| k.0.iter()) {
        for s in list {
            put(norm_seq(s), id.clone());
        }
    }
    for (k, text) in p.macros.iter().flat_map(|m| m.0.iter()) {
        put(norm_seq(k), format!("text:{text}"));
    }
    let mut out = Vec::new();
    for (seq, ids) in &by {
        if ids.len() > 1 {
            out.push(Conflict { kind: "same", seq: seq.clone(), longer: None, ids: ids.clone() });
        }
        if let Some(f) = fixed.get(seq)
            && !ids.contains(f)
        {
            let mut all = ids.clone();
            all.push(f.clone());
            out.push(Conflict { kind: "fixed", seq: seq.clone(), longer: None, ids: all });
        }
    }
    for (a, ia) in &by {
        for (b, ib) in &by {
            if a != b && b.starts_with(&format!("{a} ")) {
                let mut ids = ia.clone();
                ids.extend(ib.iter().cloned());
                out.push(Conflict { kind: "prefix", seq: a.clone(), longer: Some(b.clone()), ids });
            }
        }
    }
    out
}

/// `placeOf(p, id)`: the menu ("<label>"), "@bar", "@quick", "<tab>\0<panel>", or "" (keys only).
pub fn place_of(p: &Profile, id: &str) -> String {
    let has = |l: &[String]| l.iter().any(|x| x == id);
    if p.layout == "menubar" || p.layout == "ribbon" {
        for m in p.menus.iter().flatten() {
            if has(&m.items) {
                return m.label.clone();
            }
        }
    }
    if p.layout == "menubar" && p.bar.as_deref().is_some_and(has) {
        return "@bar".into();
    }
    if p.layout == "ribbon" && p.quick.as_deref().is_some_and(has) {
        return "@quick".into();
    }
    if p.layout == "ribbon" {
        for t in p.tabs.iter().flatten() {
            for pn in &t.panels {
                if has(&pn.items) {
                    return format!("{}\u{0}{}", t.label, pn.label);
                }
            }
        }
    }
    String::new()
}

/// `unplaced(p, registry)`: commands a menu-bar or ribbon profile gives no place.
pub fn unplaced(p: &Profile, reg: &Registry) -> Vec<String> {
    if p.layout == "toolbar" {
        return Vec::new();
    }
    reg.commands.iter().filter(|c| !c.key_only && !c.optional && place_of(p, &c.id).is_empty()).map(|c| c.id.clone()).collect()
}

/// `moveCommand(p, id, where)`: to a menu ("<i>"; a ribbon's File menu "m<i>") or a ribbon panel ("<tab>/<panel>");
/// "" = no place (keys only).
pub fn move_command(p: &Profile, id: &str, place: &str) -> Profile {
    let mut q = p.clone();
    for m in q.menus.iter_mut().flatten() {
        m.items.retain(|x| x != id);
    }
    if let Some(b) = q.bar.as_mut() {
        b.retain(|x| x != id);
    }
    for t in q.tabs.iter_mut().flatten() {
        for pn in &mut t.panels {
            pn.items.retain(|x| x != id);
        }
    }
    if !place.is_empty() {
        if q.layout == "menubar" || (q.layout == "ribbon" && place.starts_with('m')) {
            let i = js_index(place.strip_prefix('m').unwrap_or(place));
            if let Some(m) = i.and_then(|i| q.menus.as_mut().and_then(|ms| ms.get_mut(i))) {
                m.items.push(id.to_string());
            }
        } else if q.layout == "ribbon" {
            let mut it = place.split('/');
            let (ti, pi) = (it.next().and_then(js_index), it.next().and_then(js_index));
            if let (Some(ti), Some(pi)) = (ti, pi)
                && let Some(pn) = q.tabs.as_mut().and_then(|t| t.get_mut(ti)).and_then(|t| t.panels.get_mut(pi))
            {
                pn.items.push(id.to_string());
            }
        }
    }
    tidy(q)
}

/// `Number(s)` as an index (whole, not negative), else None.
fn js_index(s: &str) -> Option<usize> {
    let n = cc_cad::js::str_to_number(s);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // whole, 0 ≤ n < 1e9
    (n.is_finite() && n >= 0.0 && n.fract() == 0.0 && n < 1e9).then_some(n as usize)
}

/// No separator first, last or twice in a row.
fn tidy(mut q: Profile) -> Profile {
    for m in q.menus.iter_mut().flatten() {
        let a = std::mem::take(&mut m.items);
        let n = a.len();
        for (i, x) in a.iter().enumerate() {
            let prev = i.checked_sub(1).and_then(|j| a.get(j));
            if x != "-" || (i > 0 && i < n.saturating_sub(1) && prev.is_some_and(|p| p != "-")) {
                m.items.push(x.clone());
            }
        }
    }
    q
}

/// `setKeys(p, id, list)`: each normalised, bad ones dropped, at most three.
pub fn set_keys(p: &Profile, id: &str, list: &[String]) -> Profile {
    let mut q = p.clone();
    if id == "__proto__" {
        // `q.keys[id] = …` would set the object's prototype: no key
        q.keys = Some(q.keys.unwrap_or_default());
        return q;
    }
    let mut keys = q.keys.take().unwrap_or_default();
    let mut seqs: Vec<String> = Vec::new();
    for s in list.iter().filter_map(|s| norm_seq(s)) {
        if !seqs.contains(&s) {
            seqs.push(s);
        }
    }
    seqs.truncate(limits::KEYS_PER_COMMAND);
    if seqs.is_empty() {
        keys.remove(id);
    } else {
        keys.set(id, seqs);
    }
    q.keys = Some(keys);
    q
}

/// `setMacro(p, key, text)`: set (text not empty) or remove a text key; None when `key` is no key.
pub fn set_macro(p: &Profile, key: &str, text: &str) -> Option<Profile> {
    let k = norm_seq(key)?;
    let mut q = p.clone();
    let mut m = q.macros.take().unwrap_or_default();
    m.0.retain(|(x, _)| norm_seq(x).as_deref() != Some(k.as_str()));
    let t = clean(&json!(text), limits::MACRO);
    if !t.is_empty() {
        m.set(&k, t);
    }
    q.macros = (!m.is_empty()).then_some(m);
    Some(q)
}

/// What `validateProfile` gives: the profile (id ""), and the problems found.
pub type Checked = Result<(Profile, Vec<String>), Vec<String>>;

/// `validateProfile(obj, registry, builtins, {macros})`: a profile from a file, an add-on or another window, checked as
/// untrusted.
pub fn validate_profile(obj: &Value, reg: &Registry) -> Checked {
    let Value::Object(o) = obj else { return Err(vec!["not-a-profile".into()]) };
    if o.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err(vec!["format".into()]);
    }
    if o.get("version").and_then(Value::as_f64) != Some(1.0) {
        return Err(vec!["version".into()]);
    }
    let mut problems: Vec<String> = Vec::new();
    let base = o.get("base").and_then(Value::as_str).filter(|b| reg.builtin(b).is_some()).map(str::to_string);
    if cc_cad::js::truthy(o.get("base")) && base.is_none() {
        problems.push("base".into());
    }
    let layout = match o.get("layout").and_then(Value::as_str).filter(|l| LAYOUTS.contains(l)) {
        Some(l) => l.to_string(),
        None => base.as_deref().and_then(|b| reg.builtin(b)).map_or_else(|| "toolbar".to_string(), |b| b.layout.clone()),
    };
    let mut out = Profile {
        name: str_of(o.get("name").unwrap_or(&Value::Null), limits::NAME),
        base: base.clone(),
        layout,
        command_line: Some(o.get("commandLine") == Some(&Value::Bool(true))),
        ..Profile::default()
    };
    let ids = |list: Option<&Value>, max: usize, problems: &mut Vec<String>| -> Vec<String> {
        let Some(Value::Array(a)) = list else { return Vec::new() };
        let mut r: Vec<String> = Vec::new();
        for x in a.iter().take(max) {
            match x.as_str() {
                Some("-") => r.push("-".into()),
                Some(s) if reg.known(s) => {
                    if !r.iter().any(|y| y == s) {
                        r.push(s.to_string());
                    }
                }
                _ => problems.push(format!("unknown:{}", str_of(&json!(to_js_string(x)), 40))),
            }
        }
        r
    };
    let label = |v: Option<&Value>| -> String {
        let s = str_of(v.unwrap_or(&Value::Null), limits::LABEL);
        if s.is_empty() { "?".into() } else { s }
    };
    // `x && x.label`: a falsy x gives x itself, never a string
    let field = |x: &Value, k: &str| -> Option<Value> { if cc_cad::js::truthy(Some(x)) { x.get(k).cloned() } else { None } };
    if let Some(Value::Array(ms)) = o.get("menus") {
        let mut menus = Vec::new();
        for m in ms.iter().take(limits::MENUS) {
            let lab = label(field(m, "label").as_ref());
            let items = ids(field(m, "items").as_ref(), limits::ITEMS, &mut problems);
            menus.push(Menu { label: lab, items });
        }
        out.menus = Some(menus);
    }
    if matches!(o.get("bar"), Some(Value::Array(_))) {
        out.bar = Some(ids(o.get("bar"), limits::ITEMS, &mut problems));
    }
    if matches!(o.get("quick"), Some(Value::Array(_))) {
        out.quick = Some(ids(o.get("quick"), 12, &mut problems).into_iter().filter(|x| x != "-").collect());
    }
    if let Some(Value::Array(ts)) = o.get("tabs") {
        let mut tabs = Vec::new();
        for tb in ts.iter().take(limits::TABS) {
            let lab = label(field(tb, "label").as_ref());
            let mut panels = Vec::new();
            if let Some(Value::Array(ps)) = field(tb, "panels") {
                for pn in ps.iter().take(limits::PANELS) {
                    let pl = label(field(pn, "label").as_ref());
                    let items = ids(field(pn, "items").as_ref(), limits::ITEMS, &mut problems).into_iter().filter(|x| x != "-").collect();
                    panels.push(RPanel { label: pl, items });
                }
            }
            tabs.push(RTab { label: lab, panels });
        }
        out.tabs = Some(tabs);
    }
    if let Some(k @ (Value::Object(_) | Value::Array(_))) = o.get("keys") {
        let mut keys = JsMap::new();
        for (id, list) in js_entries(k) {
            if !reg.known(&id) {
                problems.push(format!("unknown:{}", str_of(&json!(id), 40)));
                continue;
            }
            if reg.commands.iter().any(|c| c.fixed && c.id == id) {
                continue;
            }
            let seqs: Vec<Option<String>> = match list {
                Value::Array(a) => a.iter().take(limits::KEYS_PER_COMMAND).map(norm_seq_v).collect(),
                _ => Vec::new(),
            };
            if seqs.iter().any(Option::is_none) {
                problems.push(format!("key:{id}"));
            }
            let mut good: Vec<String> = Vec::new();
            for s in seqs.into_iter().flatten() {
                if !good.contains(&s) {
                    good.push(s);
                }
            }
            if !good.is_empty() {
                keys.set(&id, good);
            }
        }
        out.keys = Some(keys);
    }
    if let Some(t @ (Value::Object(_) | Value::Array(_))) = o.get("typed") {
        let mut typed = JsMap::new();
        for (id, list) in js_entries(t) {
            if !reg.known(&id) {
                problems.push(format!("unknown:{}", str_of(&json!(id), 40)));
                continue;
            }
            let mut words: Vec<String> = Vec::new();
            if let Value::Array(a) = list {
                for w in a.iter().take(limits::WORDS) {
                    let w = str_of(w, limits::WORD).to_uppercase();
                    let ok = w.chars().next().is_some_and(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
                        && w.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_' || c == '-');
                    if ok && !words.contains(&w) {
                        words.push(w);
                    }
                }
            }
            if !words.is_empty() {
                typed.set(&id, words);
            }
        }
        out.typed = Some(typed);
    }
    if let Some(m @ Value::Object(_)) = o.get("macros") {
        if !reg.macros {
            problems.push("macros".into());
        } else {
            let mut macros = JsMap::new();
            for (k, text) in js_entries(m).into_iter().take(limits::MACROS) {
                let q = norm_seq(&k);
                let tx = clean(text, limits::MACRO);
                match q {
                    Some(q) if !tx.is_empty() => {
                        if !macros.has(&q) {
                            macros.set(&q, tx);
                        }
                    }
                    _ => problems.push(format!("key:{}", str_of(&json!(k), 40))),
                }
            }
            out.macros = Some(macros);
        }
    }
    if o.contains_key("mouse") {
        out.mouse = Some(clean_mouse(o.get("mouse")));
    }
    for c in conflicts(&out, reg) {
        problems.push(format!("conflict:{}", c.seq));
    }
    let mut seen: Vec<String> = Vec::new();
    for p in problems {
        if !seen.contains(&p) {
            seen.push(p);
        }
    }
    Ok((out, seen))
}

/// `exportProfile(p)`: the text of a `.ccinput.json`.
pub fn export_profile(p: &Profile) -> String {
    let mut o = Map::new();
    o.insert("format".into(), json!(FORMAT));
    o.insert("version".into(), json!(VERSION));
    o.insert("name".into(), json!(p.name));
    if let Some(b) = &p.base {
        o.insert("base".into(), json!(b));
    }
    o.insert("layout".into(), json!(p.layout));
    if let Some(c) = p.command_line {
        o.insert("commandLine".into(), json!(c));
    }
    p.put_parts(&mut o);
    let mut s = serde_json::to_string_pretty(&Value::Object(o)).unwrap_or_default();
    s.push('\n');
    s
}

/// `importProfile(text, …)`: the size limit and JSON errors as problems, then [`validate_profile`].
pub fn import_profile(text: &str, reg: &Registry) -> Checked {
    if text.encode_utf16().count() > limits::BYTES {
        return Err(vec!["size".into()]);
    }
    match serde_json::from_str::<Value>(text) {
        Ok(v) => validate_profile(&v, reg),
        Err(_) => Err(vec!["json".into()]),
    }
}

/// `exportName(name)`: a safe file name, "<name>.ccinput.json".
pub fn export_name(name: &str) -> String {
    // [^\w .()-]+ removed (\w = ASCII letters, digits, _), white space runs → one space
    let kept: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ' ' | '.' | '(' | ')' | '-')).collect();
    let mut one = String::new();
    for c in kept.chars() {
        if c == ' ' && one.ends_with(' ') {
            continue;
        }
        one.push(c);
    }
    let n = slice16(trim(&one), 60);
    format!("{}{EXT}", if n.is_empty() { "style" } else { &n })
}

// ---------------------------------------------------------------------------------------------- keys at run time

/// What a key press means for a profile (`KeyMatcher.press`).
#[derive(Clone, Debug, PartialEq)]
pub enum Press {
    /// run the command, swallow the key
    Run(String),
    /// type the text at the command line (WL.18)
    Text(String),
    /// the profile keeps the app's own key: let the app handle it
    Native(String),
    /// the start of a longer sequence: swallow and wait
    Wait(String),
    /// the app's own key for a command this profile moved elsewhere: swallow, say where it went
    Blocked(String),
}

/// `KeyMatcher`.
#[derive(Clone, Debug)]
pub struct KeyMatcher {
    map: JsMap<String>,
    text: JsMap<String>,
    def: Vec<(String, String)>,
    mine: Vec<(String, Vec<String>)>,
    pub buf: Vec<String>,
}

impl KeyMatcher {
    pub fn new(p: &Profile, reg: &Registry) -> Self {
        let map = bindings(p);
        let text = macro_map(p);
        let mut def: Vec<(String, String)> = Vec::new();
        for c in &reg.commands {
            for k in &c.def_keys {
                if let Some(q) = norm_seq(k)
                    && !def.iter().any(|(s, _)| *s == q)
                {
                    def.push((q, c.id.clone()));
                }
            }
        }
        let mut mine: Vec<(String, Vec<String>)> = Vec::new();
        for (seq, id) in &map.0 {
            match mine.iter_mut().find(|(i, _)| i == id) {
                Some((_, l)) => l.push(seq.clone()),
                None => mine.push((id.clone(), vec![seq.clone()])),
            }
        }
        KeyMatcher { map, text, def, mine, buf: Vec::new() }
    }
    pub fn reset(&mut self) {
        self.buf.clear();
    }
    /// The sequences this profile gives a command.
    pub fn keys_of(&self, id: &str) -> Vec<String> {
        self.mine.iter().find(|(i, _)| i == id).map(|(_, l)| l.clone()).unwrap_or_default()
    }
    pub fn has_text(&self) -> bool {
        !self.text.is_empty()
    }
    fn def_of(&self, seq: &str) -> Option<&String> {
        self.def.iter().find(|(s, _)| s == seq).map(|(_, id)| id)
    }
    fn try_buf(&self, buf: &[String]) -> Option<Press> {
        let seq = buf.join(" ");
        let id = self.map.get(&seq);
        let tx = self.text.get(&seq);
        let pre = format!("{seq} ");
        let longer = self.map.0.iter().map(|(k, _)| k).chain(self.text.0.iter().map(|(k, _)| k)).any(|k| k.starts_with(&pre));
        if let Some(t) = tx
            && !longer
        {
            return Some(Press::Text(t.clone()));
        }
        if let Some(id) = id
            && !longer
        {
            return Some(if self.def_of(&seq) == Some(id) { Press::Native(id.clone()) } else { Press::Run(id.clone()) });
        }
        if longer {
            return Some(Press::Wait(seq));
        }
        None
    }
    pub fn press(&mut self, combo: &str) -> Option<Press> {
        if combo.is_empty() {
            return None;
        }
        let mut b = self.buf.clone();
        b.push(combo.to_string());
        let mut r = self.try_buf(&b);
        if r.is_none() && !self.buf.is_empty() {
            self.buf.clear();
            r = self.try_buf(&[combo.to_string()]);
        }
        if matches!(r, Some(Press::Wait(_))) {
            self.buf.push(combo.to_string());
            return r;
        }
        self.buf.clear();
        if r.is_some() {
            return r;
        }
        match self.def_of(combo) {
            Some(d) if !self.map.has(combo) => Some(Press::Blocked(d.clone())),
            _ => None,
        }
    }
}

/// `matchTyped(p, word)`: exact words first, then a unique start.
pub fn match_typed(p: &Profile, word: &str) -> Option<String> {
    let w = trim(word).to_uppercase();
    if w.is_empty() {
        return None;
    }
    let entries: Vec<&(String, Vec<String>)> = p.typed.iter().flat_map(|t| t.0.iter()).collect();
    if let Some((id, _)) = entries.iter().find(|(_, words)| words.contains(&w)) {
        return Some(id.clone());
    }
    let hits: Vec<&&(String, Vec<String>)> = entries.iter().filter(|(_, words)| words.iter().any(|x| x.starts_with(&w))).collect();
    if hits.len() == 1 { hits.first().map(|h| h.0.clone()) } else { None }
}

// ---------------------------------------------------------------------------------------------- the kept state

/// What the window keeps (`readState` / `writeState`: localStorage `cadcraft.input` in 0.5).
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub style: String,
    pub custom: Vec<Profile>,
    pub tab: usize,
    pub family: String,
    pub family_at: f64,
    pub mine: Vec<(String, String)>,
    pub carried: Vec<String>,
}

impl State {
    pub fn new(reg: &Registry) -> State {
        State {
            style: reg.own().to_string(),
            custom: Vec::new(),
            tab: 0,
            family: String::new(),
            family_at: 0.0,
            mine: Vec::new(),
            carried: Vec::new(),
        }
    }
}

/// `readState(text, reg)`.
pub fn read_state(v: &Value, reg: &Registry) -> State {
    let mut st = State::new(reg);
    let Value::Object(o) = v else { return st };
    if let Some(Value::Array(cs)) = o.get("custom") {
        for c in cs.iter().take(limits::CUSTOM) {
            let id = c.get("id").and_then(Value::as_str).unwrap_or("");
            let id_ok = id.len() > 3 && id.len() <= 9 && id.starts_with("my-") && id.bytes().skip(3).all(|b| b.is_ascii_digit());
            if let Ok((mut p, _)) = validate_profile(c, reg)
                && id_ok
                && !st.custom.iter().any(|x| x.id == id)
            {
                p.id = id.to_string();
                st.custom.push(p);
            }
        }
    }
    let valid = |id: &str, st: &State| reg.builtin(id).is_some() || st.custom.iter().any(|c| c.id == id);
    if let Some(s) = o.get("style").and_then(Value::as_str)
        && valid(s, &st)
    {
        st.style = s.to_string();
    }
    if let Some(t) = o.get("tab").and_then(Value::as_f64)
        && t.fract() == 0.0
        && (0.0..20.0).contains(&t)
    {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // whole, 0 ≤ t < 20
        {
            st.tab = t as usize;
        }
    }
    if let Some(f) = o.get("family").and_then(Value::as_str).filter(|f| FAMILIES.contains(f)) {
        st.family = f.to_string();
    }
    if let Some(a) = o.get("familyAt").and_then(Value::as_f64).filter(|a| a.is_finite() && *a > 0.0) {
        st.family_at = a;
    }
    if let Some(Value::Object(m)) = o.get("mine") {
        for f in FAMILIES {
            if let Some(id) = m.get(f).and_then(Value::as_str)
                && valid(id, &st)
            {
                st.mine.push((f.to_string(), id.to_string()));
            }
        }
    }
    if let Some(Value::Array(c)) = o.get("carried") {
        st.carried = c
            .iter()
            .filter_map(Value::as_str)
            .filter(|x| {
                !x.is_empty() && x.len() <= 60 && x.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-'))
            })
            .take(10)
            .map(str::to_string)
            .collect();
    }
    st
}

/// `writeState(st)`.
pub fn write_state(st: &State) -> Value {
    let mut o = Map::new();
    o.insert("style".into(), json!(st.style));
    o.insert("custom".into(), Value::Array(st.custom.iter().map(Profile::to_json).collect()));
    o.insert("tab".into(), json!(st.tab));
    if !st.family.is_empty() {
        o.insert("family".into(), json!(st.family));
    }
    if st.family_at > 0.0 {
        o.insert("familyAt".into(), jnum(st.family_at));
    }
    if !st.mine.is_empty() {
        o.insert("mine".into(), Value::Object(st.mine.iter().map(|(k, v)| (k.clone(), json!(v))).collect()));
    }
    if !st.carried.is_empty() {
        o.insert("carried".into(), json!(st.carried));
    }
    Value::Object(o)
}

/// `addCustom(st, p)`: a person's own profile (id my-<n>; at most 20, the oldest goes). → its id.
pub fn add_custom(st: &mut State, p: &Profile) -> String {
    let mut n = 1u32;
    while st.custom.iter().any(|c| c.id == format!("my-{n}")) {
        n = n.saturating_add(1);
    }
    let mut q = p.clone();
    q.id = format!("my-{n}");
    q.builtin = false;
    q.added = true;
    st.custom.push(q);
    while st.custom.len() > limits::CUSTOM {
        st.custom.remove(0);
    }
    format!("my-{n}")
}

/// `rawStyle(st, reg, id)`.
pub fn raw_style<'a>(st: &'a State, reg: &'a Registry, id: &str) -> Option<&'a Profile> {
    reg.builtin(id).or_else(|| st.custom.iter().find(|c| c.id == id))
}

/// `familyOf(reg, st, id)`.
pub fn family_of(reg: &Registry, st: &State, id: &str) -> String {
    let Some(p) = raw_style(st, reg, id) else { return String::new() };
    let b = if reg.builtin(id).is_some() { Some(id.to_string()) } else { p.base.clone() };
    let f = b.and_then(|b| reg.family.iter().find(|(s, _)| *s == b).map(|(_, f)| f.clone())).unwrap_or_default();
    if FAMILIES.contains(&f.as_str()) { f } else { String::new() }
}

/// `pickStyle(st, reg, id, now)`: remembered as the person's member of its family.
pub fn pick_style(st: &mut State, reg: &Registry, id: &str, now: f64) {
    if raw_style(st, reg, id).is_none() {
        return;
    }
    let f = family_of(reg, st, id);
    st.style = id.to_string();
    st.tab = 0;
    if !f.is_empty() {
        match st.mine.iter_mut().find(|(k, _)| *k == f) {
            Some(slot) => slot.1 = id.to_string(),
            None => st.mine.push((f, id.to_string())),
        }
    }
    st.family_at = st.family_at.max(now);
}

/// `carryShortcuts(data)`: CadCraft's SHORTCUTS keys (`{format: "cadcraft-shortcuts", version: 1, keys: {key: text}}`)
/// as text keys. → (macros, how many were dropped).
pub fn carry_shortcuts(data: &Value) -> (JsMap<String>, usize) {
    let mut macros = JsMap::new();
    let mut dropped = 0usize;
    let src = data
        .get("keys")
        .filter(|k| matches!(k, Value::Object(_)) && data.get("format").and_then(Value::as_str) == Some("cadcraft-shortcuts"));
    for (k, text) in src.map(js_entries).unwrap_or_default().into_iter().take(limits::MACROS) {
        // shortcuts.js writes arrows as Left / Right / Up / Down and allows "Ctrl-L"
        let mut key = k.clone();
        for d in ["Left", "Right", "Up", "Down"] {
            if (key == d || key.ends_with(&format!("+{d}"))) && !key.ends_with(&format!("Arrow{d}")) {
                let cut = key.len().saturating_sub(d.len());
                key = format!("{}Arrow{d}", key.get(..cut).unwrap_or(""));
                break;
            }
        }
        let mut dashed = String::new();
        let ch: Vec<char> = key.chars().collect();
        for (i, c) in ch.iter().enumerate() {
            // /-(?=.)/: a dash with any character but a line end after it
            let next = ch.get(i.saturating_add(1)).is_some_and(|n| !matches!(n, '\n' | '\r' | '\u{2028}' | '\u{2029}'));
            dashed.push(if *c == '-' && next { '+' } else { *c });
        }
        let q = norm_seq(&dashed);
        let tx = clean(text, limits::MACRO);
        match q {
            Some(q) if !tx.is_empty() && !q.contains(' ') => macros.set(&q, tx),
            _ => dropped = dropped.saturating_add(1),
        }
    }
    (macros, dropped)
}

/// `carryInto(st, reg, oldData, name)`: once, the old SHORTCUTS keys into a person's own profile over the app's own
/// style (picked when that style was in use). → the carried profile's id.
pub fn carry_into(st: &mut State, reg: &Registry, from: &str, old: &Value, name: &str) -> Option<String> {
    if st.carried.iter().any(|c| c == from) {
        return None;
    }
    st.carried.push(from.to_string());
    let (macros, _) = carry_shortcuts(old);
    if macros.is_empty() {
        return None;
    }
    let base = reg.own().to_string();
    let layout = reg.builtin(&base).map_or_else(|| "toolbar".to_string(), |b| b.layout.clone());
    let n = clean(&json!(name), limits::NAME);
    let p = Profile {
        name: if n.is_empty() { "My keys".into() } else { n },
        base: Some(base.clone()),
        layout,
        macros: Some(macros),
        ..Profile::default()
    };
    let id = add_custom(st, &p);
    if st.style == base {
        st.style = id.clone();
    }
    Some(id)
}

// ---------------------------------------------------------------------------------------------- parity cases

fn press_json(p: Option<Press>) -> Value {
    match p {
        None => Value::Null,
        Some(Press::Run(x)) => json!({ "run": x }),
        Some(Press::Text(x)) => json!({ "text": x }),
        Some(Press::Native(x)) => json!({ "native": x }),
        Some(Press::Wait(x)) => json!({ "wait": x }),
        Some(Press::Blocked(x)) => json!({ "blocked": x }),
    }
}

fn conflicts_json(cs: &[Conflict]) -> Value {
    Value::Array(
        cs.iter()
            .map(|c| match &c.longer {
                Some(l) => json!({ "kind": c.kind, "seq": c.seq, "longer": l, "ids": c.ids }),
                None => json!({ "kind": c.kind, "seq": c.seq, "ids": c.ids }),
            })
            .collect(),
    )
}

/// `readState`'s input: a string is parsed first (a damaged one = nothing kept).
fn state_input(v: &Value) -> Value {
    match v {
        Value::String(s) => serde_json::from_str(s).unwrap_or(Value::Null),
        x => x.clone(),
    }
}

/// One case of forks/tools/inputdiff-cadrest.mjs (`cadcraft-shot inputcases`); `tr` = the catalog (a built-in's name).
pub fn run_case(c: &Value, reg: &Registry, tr: &dyn Fn(&str) -> String) -> Value {
    let name_of = |p: &Profile| if p.builtin { tr(&p.name) } else { p.name.clone() };
    let export_cur = |p: &Profile| {
        let mut q = p.clone();
        q.name = name_of(p);
        q.base = if p.builtin { Some(p.id.clone()) } else { p.base.clone() };
        export_profile(&q)
    };
    let ids: Vec<String> = reg.commands.iter().map(|c| c.id.clone()).collect();
    match c.get("kind").and_then(Value::as_str).unwrap_or("") {
        "seq" => {
            let s = c.get("s").and_then(Value::as_str);
            let q = s.and_then(norm_seq);
            json!({ "seq": q, "combo": s.and_then(norm_combo), "show": q.as_deref().map(show_seq) })
        }
        "validate" => match validate_profile(c.get("obj").unwrap_or(&Value::Null), reg) {
            Err(problems) => json!({ "ok": false, "problems": problems }),
            Ok((p, problems)) => {
                let mut pr = p.clone();
                pr.id = "my-1".into();
                let r = resolve(&pr, reg);
                json!({
                    "ok": true,
                    "problems": problems,
                    "profile": p.to_json(),
                    "export": export_profile(&r),
                    "conflicts": conflicts_json(&conflicts(&p, reg)),
                    "unplaced": unplaced(&r, reg),
                    "place": ids.iter().take(40).map(|id| place_of(&r, id)).collect::<Vec<_>>(),
                })
            }
        },
        "import" => match import_profile(c.get("text").and_then(Value::as_str).unwrap_or(""), reg) {
            Err(problems) => json!({ "ok": false, "problems": problems, "profile": null }),
            Ok((p, problems)) => json!({ "ok": true, "problems": problems, "profile": p.to_json() }),
        },
        "matcher" => {
            let p = match c.get("style") {
                // STYLES[x] for an unknown x: undefined, and resolve(undefined) = null — a matcher with no keys
                Some(Value::String(s)) => {
                    Some(reg.builtin(s).cloned().unwrap_or_else(|| Profile { layout: String::new(), ..Profile::default() }))
                }
                Some(v) => validate_profile(v, reg).ok().map(|x| x.0),
                None => None,
            };
            let Some(p) = p else { return Value::Null };
            let mut m = KeyMatcher::new(&resolve(&p, reg), reg);
            let out: Vec<Value> = c
                .get("presses")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|k| press_json(m.press(k.as_str().unwrap_or(""))))
                .collect();
            json!({ "out": out, "keysOf": ids.iter().take(60).map(|id| m.keys_of(id)).collect::<Vec<_>>() })
        }
        "ops" => {
            let Some(b) = c.get("style").and_then(Value::as_str).and_then(|s| reg.builtin(s)) else { return Value::Null };
            let mut p = resolve(b, reg);
            let mut outs = Vec::new();
            for o in c.get("ops").and_then(Value::as_array).into_iter().flatten() {
                // a property name is String(x) (an id of 3 is the key "3"); a missing one is ""
                let g = |k: &str| o.get(k).filter(|v| !v.is_null()).map(to_js_string).unwrap_or_default();
                match g("op").as_str() {
                    "keys" => {
                        let list: Vec<String> = o
                            .get("list")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect();
                        p = set_keys(&p, &g("id"), &list);
                    }
                    "move" => p = move_command(&p, &g("id"), &g("where")),
                    "macro" => p = set_macro(&p, &g("key"), &g("text")).unwrap_or(p),
                    _ => {}
                }
                outs.push(conflicts(&p, reg).len());
            }
            json!({ "outs": outs, "export": export_cur(&p), "unplaced": unplaced(&p, reg) })
        }
        "state" => write_state(&read_state(&state_input(c.get("v").unwrap_or(&Value::Null)), reg)),
        "carry" => {
            let data = c.get("data").cloned().unwrap_or(Value::Null);
            let (macros, dropped) = carry_shortcuts(&data);
            let mut st = read_state(&state_input(c.get("state").unwrap_or(&Value::Null)), reg);
            let name = c.get("name").and_then(Value::as_str).unwrap_or("");
            let id = carry_into(&mut st, reg, "cadcraft.shortcuts", &data, name);
            json!({
                "r": { "macros": Value::Object(macros.0.iter().map(|(k, v)| (k.clone(), json!(v))).collect()), "dropped": dropped },
                "state": write_state(&st),
                "carried": id,
            })
        }
        "typed" => match validate_profile(c.get("obj").unwrap_or(&Value::Null), reg) {
            Ok((p, _)) => json!(match_typed(&p, c.get("word").and_then(Value::as_str).unwrap_or(""))),
            Err(_) => Value::Null,
        },
        "name" => json!(export_name(c.get("name").and_then(Value::as_str).unwrap_or(""))),
        "builtin" => match c.get("style").and_then(Value::as_str).and_then(|s| reg.builtin(s)) {
            Some(b) => json!(export_cur(&resolve(b, reg))),
            None => Value::Null,
        },
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg() -> Registry {
        let c = |id: &str, keys: &[&str], fixed: bool| RegCmd {
            id: id.into(),
            def_keys: keys.iter().map(|k| (*k).to_string()).collect(),
            fixed,
            key_only: false,
            optional: false,
        };
        let commands = vec![c("file.save", &["Ctrl+S"], false), c("draw.line", &[], false), c("edit.delete", &["Delete"], true)];
        let mut keys = JsMap::new();
        keys.set("file.save", vec!["Ctrl+S".to_string()]);
        let own = Profile { id: "own".into(), name: "input.style.own".into(), builtin: true, keys: Some(keys), ..Profile::default() };
        Registry { commands, styles: vec![own], family: vec![("own".into(), "own".into())], macros: true }
    }

    #[test]
    fn keys_are_written_one_way() {
        assert_eq!(norm_seq("ctrl+shift+v").as_deref(), Some("Ctrl+Shift+V"));
        assert_eq!(norm_seq("wa").as_deref(), Some("W A"));
        assert_eq!(norm_seq("Ctrl+K   Ctrl+S").as_deref(), Some("Ctrl+K Ctrl+S"));
        assert_eq!(norm_seq("cmd+o").as_deref(), Some("Ctrl+O"));
        assert_eq!(norm_seq("A B C D E"), None);
        assert_eq!(norm_seq("Ctrl++"), None);
        assert_eq!(norm_seq("F13"), None);
        assert_eq!(show_seq("W A"), "WA");
        assert_eq!(show_seq("Ctrl+K Ctrl+S"), "Ctrl+K Ctrl+S");
        let mut m = JsMap::new();
        m.set("B", 1);
        m.set("2", 2);
        m.set("A", 3);
        m.set("1", 4);
        assert_eq!(m.0.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["1", "2", "B", "A"], "JS object order");
    }

    #[test]
    fn a_file_is_checked_and_the_matcher_takes_sequences() {
        let r = reg();
        let text = r#"{"format":"cadcraft-input","version":1,"name":" Mine\n","base":"own","keys":{"draw.line":["W A","bad key"],
            "nope":["X"],"edit.delete":["D"]},"macros":{"ctrl+alt+l":"LINE  0,0"},"mouse":{"wheel":-1,"zoomSpeed":9}}"#;
        let (p, problems) = import_profile(text, &r).unwrap();
        assert_eq!(p.name, "Mine");
        assert_eq!(problems, ["key:draw.line", "unknown:nope"]);
        assert_eq!(p.mouse.as_ref().map(|m| (m.wheel, m.zoom_speed)), Some((-1, 4.0)));
        assert!(p.keys.as_ref().is_some_and(|k| !k.has("edit.delete")), "Delete never moves");
        let mut m = KeyMatcher::new(&resolve(&p, &r), &r);
        assert_eq!(m.press("W"), Some(Press::Wait("W".into())));
        assert_eq!(m.press("A"), Some(Press::Run("draw.line".into())));
        assert_eq!(m.press("Ctrl+Alt+L"), Some(Press::Text("LINE 0,0".into())));
        // the file's own keys replace the style's: Ctrl+S is no longer the save key
        assert_eq!(m.press("Ctrl+S"), Some(Press::Blocked("file.save".into())));
        assert_eq!(import_profile("{", &r), Err(vec!["json".to_string()]));
        assert_eq!(import_profile(r#"{"format":"x"}"#, &r), Err(vec!["format".to_string()]));
        // exported, it reads back the same
        let (back, _) = import_profile(&export_profile(&p), &r).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn the_kept_state_and_the_carry() {
        let r = reg();
        let mut st = State::new(&r);
        let old = json!({"format": "cadcraft-shortcuts", "version": 1, "keys": {"Ctrl-L": "LINE", "Left": "PAN", "A B": "X"}});
        let id = carry_into(&mut st, &r, "cadcraft.shortcuts", &old, "My keys").unwrap();
        assert_eq!(st.style, id);
        let p = raw_style(&st, &r, &id).unwrap();
        assert_eq!(
            p.macros.as_ref().map(|m| m.0.clone()),
            Some(vec![("Ctrl+L".into(), "LINE".into()), ("ArrowLeft".into(), "PAN".into())])
        );
        assert_eq!(carry_into(&mut st, &r, "cadcraft.shortcuts", &old, "x"), None, "once");
        // read back as the JS reads it (the carried style gains `commandLine: false`), then stable
        let once = read_state(&write_state(&st), &r);
        assert_eq!(once.custom.len(), 1);
        assert_eq!(read_state(&write_state(&once), &r), once);
        assert_eq!(read_state(&json!({"style": "nope", "tab": 99}), &r), State::new(&r));
        assert_eq!(export_name("a/b: c"), "ab c.ccinput.json");
    }
}
