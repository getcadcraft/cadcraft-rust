//! mytools.js and macro.js — buttons of your own (0.5 DR.9): the "My tools" groups on CadCraft's toolbar, kept as
//! `cadcraft.mytools` (`{ format: 'cadcraft-mytools', version: 1, groups: [{ name, tools: [{ id, label, macro, icon?,
//! key? }] }] }`), exported and imported as JSON and filled from a CUIx file ([`crate::cuix`]); and button macros in
//! AutoCAD's menu macro syntax read into steps the command line runs. Everything that comes in is untrusted: sizes are
//! capped, icons must be real PNG or SVG pictures of at most 64 KB, keys must be keys CadCraft takes and not taken.

use serde_json::{Map, Value, json};

use crate::shortcuts::{key_problem, norm_key};

/// The store's name (localStorage `cadcraft.mytools` in 0.5; the window keeps the same JSON).
pub const MYTOOLS_KEY: &str = "cadcraft.mytools";
/// `MYTOOLS_LIMITS`
pub const LIMIT_GROUPS: usize = 50;
pub const LIMIT_TOOLS: usize = 500;
pub const LIMIT_LABEL: usize = 80;
pub const LIMIT_MACRO: usize = 4000;
pub const LIMIT_NAME: usize = 60;
pub const LIMIT_ICON: usize = 64 * 1024;

/// A message: its catalog key and fills.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub key: String,
    pub fills: Vec<(String, String)>,
}
impl Note {
    fn new(key: &str, fills: &[(&str, String)]) -> Note {
        Note { key: key.into(), fills: fills.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect() }
    }
    /// As JSON (`{ key, fills? }`).
    pub fn json(&self) -> Value {
        let mut o = Map::new();
        o.insert("key".into(), json!(self.key));
        if !self.fills.is_empty() {
            o.insert("fills".into(), Value::Object(self.fills.iter().map(|(k, v)| (k.clone(), json!(v))).collect()));
        }
        Value::Object(o)
    }
    /// The fills as the catalog takes them.
    pub fn vars(&self) -> Vec<(&str, String)> {
        self.fills.iter().map(|(k, v)| (k.as_str(), v.clone())).collect()
    }
}

/// One button.
#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    pub id: String,
    pub label: String,
    pub macro_: String,
    pub icon: Option<String>,
    pub key: Option<String>,
}
/// One group of buttons.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub name: String,
    pub tools: Vec<Tool>,
}
/// The person's tools.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tools {
    pub groups: Vec<Group>,
}

/// Where new tool ids come from: `'t' + Date.now().toString(36) + (seq++).toString(36)`.
#[derive(Clone, Debug, Default)]
pub struct Ids {
    pub now_ms: f64,
    pub seq: u64,
}
impl Ids {
    fn next(&mut self) -> String {
        let s = self.seq;
        self.seq = self.seq.saturating_add(1);
        // (whole milliseconds since 1970, as a decimal string: well inside u64)
        let now = if self.now_ms.is_finite() && self.now_ms >= 0.0 && self.now_ms < 9e15 { self.now_ms.floor() } else { 0.0 };
        let now: u64 = format!("{now:.0}").parse().unwrap_or(0);
        format!("t{}{}", radix36(now), radix36(s))
    }
}
fn radix36(mut n: u64) -> String {
    const D: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(D.get((n % 36) as usize).copied().unwrap_or(b'0'));
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// `clean(s, n)`: control characters to spaces, trimmed, at most n UTF-16 units.
fn clean(v: Option<&Value>, n: usize) -> String {
    let s = match v {
        None | Some(Value::Null) => String::new(),
        Some(x) => cc_cad::js::to_js_string(x),
    };
    let s: String = s.chars().map(|c| if (c as u32) < 0x20 || c == '\u{7f}' { ' ' } else { c }).collect();
    cc_cad::js::slice16(cc_cad::js::trim(&s), n)
}

/// `^[A-Za-z0-9_-]{1,40}$`
fn good_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 40 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// `atob` (forgiving base64): None where it throws.
pub fn atob(s: &str) -> Option<Vec<u8>> {
    let mut t: Vec<u8> = s.bytes().filter(|b| !matches!(b, b'\t' | b'\n' | b'\x0c' | b'\r' | b' ')).collect();
    if t.len().is_multiple_of(4) {
        if t.ends_with(b"==") {
            t.truncate(t.len().saturating_sub(2));
        } else if t.ends_with(b"=") {
            t.truncate(t.len().saturating_sub(1));
        }
    }
    if t.len() % 4 == 1 {
        return None;
    }
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let val = |b: u8| -> Option<u32> { A.iter().position(|x| *x == b).and_then(|i| u32::try_from(i).ok()) };
    let mut out = Vec::with_capacity((t.len() / 4).saturating_mul(3).saturating_add(3));
    let (mut buf, mut bits) = (0u32, 0u32);
    for b in t {
        buf = (buf << 6) | val(b)?;
        bits = bits.saturating_add(6);
        if bits >= 8 {
            bits = bits.saturating_sub(8);
            out.push(((buf >> bits) & 0xff) as u8);
        }
        buf &= (1u32 << bits).saturating_sub(1);
    }
    Some(out)
}
/// `btoa` of bytes.
pub fn btoa(d: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(d.len().div_ceil(3).saturating_mul(4));
    for c in d.chunks(3) {
        let b = [c.first().copied().unwrap_or(0), c.get(1).copied().unwrap_or(0), c.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (k, sh) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if k <= c.len() {
                out.push(char::from(A.get(((n >> sh) & 63) as usize).copied().unwrap_or(b'A')));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `bytesOfDataUrl(u)`: (mime, bytes) of `data:image/png|svg+xml;base64,…`.
fn bytes_of_data_url(u: &str) -> Option<(&'static str, Vec<u8>)> {
    let (mime, rest) = match u.strip_prefix("data:image/png;base64,") {
        Some(r) => ("image/png", r),
        None => ("image/svg+xml", u.strip_prefix("data:image/svg+xml;base64,")?),
    };
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=') {
        return None;
    }
    Some((mime, atob(rest)?))
}

/// JS `\s` at a char.
fn ws(c: char) -> bool {
    cc_cad::js::is_ws(c)
}

/// `/^(<\?xml[^>]*>\s*)?(<!--[\s\S]*?-->\s*)*<svg[\s>]/i` on the start of a picture's text (a comment's lazy body
/// gives way to a later "-->" when what follows the first one does not lead to `<svg`).
fn svg_head(s: &str) -> bool {
    let c: Vec<char> = s.chars().collect();
    let at =
        |i: usize, lit: &str| lit.chars().enumerate().all(|(k, ch)| c.get(i.saturating_add(k)).map(char::to_ascii_lowercase) == Some(ch));
    let skip_ws = |mut i: usize| {
        while c.get(i).is_some_and(|x| ws(*x)) {
            i = i.saturating_add(1);
        }
        i
    };
    // rest(i): (comment \s*)* <svg[\s>] from i — memoised (each start is tried once)
    let mut memo: Vec<Option<bool>> = vec![None; c.len().saturating_add(2)];
    fn rest(
        i: usize,
        c: &[char],
        at: &dyn Fn(usize, &str) -> bool,
        skip_ws: &dyn Fn(usize) -> usize,
        memo: &mut Vec<Option<bool>>,
    ) -> bool {
        if let Some(Some(v)) = memo.get(i) {
            return *v;
        }
        let mut ok = at(i, "<svg") && c.get(i.saturating_add(4)).is_some_and(|x| ws(*x) || *x == '>');
        if !ok && at(i, "<!--") {
            let mut j = i.saturating_add(4);
            while j.saturating_add(3) <= c.len() {
                if at(j, "-->") && rest(skip_ws(j.saturating_add(3)), c, at, skip_ws, memo) {
                    ok = true;
                    break;
                }
                j = j.saturating_add(1);
            }
        }
        if let Some(m) = memo.get_mut(i) {
            *m = Some(ok);
        }
        ok
    }
    if at(0, "<?xml") {
        let mut j = 5;
        while c.get(j).is_some_and(|x| *x != '>') {
            j = j.saturating_add(1);
        }
        if c.get(j) == Some(&'>') && rest(skip_ws(j.saturating_add(1)), &c, &at, &skip_ws, &mut memo) {
            return true;
        }
    }
    rest(0, &c, &at, &skip_ws, &mut memo)
}

/// `checkIcon(u)`: "png" | "svg" when the data: URL holds a real PNG or SVG of at most 64 KB.
pub fn check_icon(u: &Value) -> Option<&'static str> {
    let u = u.as_str()?;
    if u.encode_utf16().count() as f64 > LIMIT_ICON as f64 * 1.4 + 64.0 {
        return None;
    }
    let (mime, b) = bytes_of_data_url(u)?;
    if b.len() > LIMIT_ICON {
        return None;
    }
    if mime == "image/png" {
        return (b.len() > 8 && b.starts_with(&[0x89, 0x50, 0x4e, 0x47])).then_some("png");
    }
    let head = String::from_utf8_lossy(b.get(..1024).unwrap_or(&b)).into_owned();
    // TextDecoder drops a leading BOM; the pattern's replace(/^﻿/) again, then trim
    let head = head.strip_prefix('\u{feff}').unwrap_or(&head);
    let head = head.strip_prefix('\u{feff}').unwrap_or(head);
    svg_head(cc_cad::js::trim(head)).then_some("svg")
}

/// `iconFromBytes(bytes)`: a picked file → a data: URL, or None when it is not a PNG or an SVG of at most 64 KB.
pub fn icon_from_bytes(b: &[u8]) -> Option<String> {
    if b.len() > LIMIT_ICON {
        return None;
    }
    let png = b.first() == Some(&0x89) && b.get(1) == Some(&0x50);
    let u = format!("data:{};base64,{}", if png { "image/png" } else { "image/svg+xml" }, btoa(b));
    check_icon(&Value::String(u.clone())).map(|_| u)
}

/// `cleanTools(v)`: untrusted data (a file, the store, a CUIx import) made safe; what is dropped is said.
pub fn clean_tools(v: &Value, ids: &mut Ids) -> (Tools, Vec<Note>) {
    let mut errors = Vec::new();
    let mut data = Tools::default();
    let ok = v.is_object()
        && v.get("format").and_then(Value::as_str) == Some("cadcraft-mytools")
        && v.get("groups").is_some_and(Value::is_array);
    if !ok {
        if cc_cad::js::truthy(Some(v)) {
            errors.push(Note::new("mytools.err.format", &[]));
        }
        return (data, errors);
    }
    let mut taken: Vec<(String, String)> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut n = 0usize;
    let groups = v.get("groups").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    for g in groups.iter().take(LIMIT_GROUPS) {
        let Some(tools) = g.as_object().and_then(|_| g.get("tools")).and_then(Value::as_array) else { continue };
        let name = clean(g.get("name"), LIMIT_NAME);
        let mut group = Group { name: if name.is_empty() { "My tools".into() } else { name }, tools: Vec::new() };
        for t in tools {
            n = n.saturating_add(1);
            if n > LIMIT_TOOLS {
                errors.push(Note::new("mytools.err.too_many", &[]));
                break;
            }
            if !(t.is_object() || t.is_array()) {
                continue;
            }
            let label = clean(t.get("label"), LIMIT_LABEL);
            let macro_ = match t.get("macro") {
                None | Some(Value::Null) => String::new(),
                Some(x) => cc_cad::js::slice16(&cc_cad::js::to_js_string(x), LIMIT_MACRO),
            };
            if label.is_empty() || cc_cad::js::trim(&macro_).is_empty() {
                errors.push(Note::new("mytools.err.tool", &[("label", if label.is_empty() { "?".into() } else { label.clone() })]));
                continue;
            }
            let id = match t.get("id").and_then(Value::as_str) {
                Some(s) if good_id(s) && !seen.iter().any(|x| x == s) => s.to_string(),
                _ => ids.next(),
            };
            seen.push(id.clone());
            let mut tool = Tool { id: id.clone(), label: label.clone(), macro_, icon: None, key: None };
            match t.get("icon") {
                None | Some(Value::Null) => {}
                Some(Value::String(s)) if s.is_empty() => {}
                Some(icon) => {
                    if check_icon(icon).is_some() {
                        tool.icon = icon.as_str().map(str::to_string);
                    } else {
                        errors.push(Note::new("mytools.err.icon", &[("label", label.clone())]));
                    }
                }
            }
            let key_v = t.get("key");
            if cc_cad::js::truthy(key_v) {
                let k = key_v.and_then(Value::as_str).and_then(norm_key);
                let why = match &k {
                    Some(k) => key_problem(k, &taken),
                    None => Some("format"),
                };
                match (why, k) {
                    (Some(why), _) => {
                        let raw = key_v.map(cc_cad::js::to_js_string).unwrap_or_default();
                        errors.push(Note::new(
                            &format!("mytools.err.key.{why}"),
                            &[("label", label.clone()), ("key", cc_cad::js::slice16(&raw, 40))],
                        ));
                    }
                    (None, Some(k)) => {
                        taken.push((k.clone(), id.clone()));
                        tool.key = Some(k);
                    }
                    (None, None) => {}
                }
            }
            group.tools.push(tool);
        }
        data.groups.push(group);
    }
    (data, errors)
}

impl Tools {
    /// As stored and exported (key order as 0.5 writes it).
    pub fn json(&self) -> Value {
        let groups: Vec<Value> = self
            .groups
            .iter()
            .map(|g| {
                let tools: Vec<Value> = g
                    .tools
                    .iter()
                    .map(|t| {
                        let mut o = Map::new();
                        o.insert("id".into(), json!(t.id));
                        o.insert("label".into(), json!(t.label));
                        o.insert("macro".into(), json!(t.macro_));
                        if let Some(i) = &t.icon {
                            o.insert("icon".into(), json!(i));
                        }
                        if let Some(k) = &t.key {
                            o.insert("key".into(), json!(k));
                        }
                        Value::Object(o)
                    })
                    .collect();
                json!({ "name": g.name, "tools": tools })
            })
            .collect();
        json!({ "format": "cadcraft-mytools", "version": 1, "groups": groups })
    }
    /// Every tool with a key: (key, group index, tool index); a later one wins a key (`keyMap`).
    pub fn key_map(&self) -> Vec<(String, usize, usize)> {
        let mut out: Vec<(String, usize, usize)> = Vec::new();
        for (gi, g) in self.groups.iter().enumerate() {
            for (ti, t) in g.tools.iter().enumerate() {
                if let Some(k) = &t.key {
                    match out.iter_mut().find(|x| x.0 == *k) {
                        Some(x) => *x = (k.clone(), gi, ti),
                        None => out.push((k.clone(), gi, ti)),
                    }
                }
            }
        }
        out
    }
    /// The tool with this id.
    pub fn find(&self, id: &str) -> Option<(usize, usize)> {
        self.groups.iter().enumerate().find_map(|(gi, g)| g.tools.iter().position(|t| t.id == id).map(|ti| (gi, ti)))
    }
}

/// `JSON.stringify(v, null, 2)` of a JSON value with its keys in order.
pub fn pretty(v: &Value, ind: usize, out: &mut String) {
    let pad = |n: usize, out: &mut String| out.extend(std::iter::repeat_n(' ', n));
    match v {
        Value::Object(o) if !o.is_empty() => {
            out.push_str("{\n");
            for (i, (k, x)) in o.iter().enumerate() {
                pad(ind.saturating_add(2), out);
                out.push_str(&Value::String(k.clone()).to_string());
                out.push_str(": ");
                pretty(x, ind.saturating_add(2), out);
                if i.saturating_add(1) < o.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            pad(ind, out);
            out.push('}');
        }
        Value::Array(a) if !a.is_empty() => {
            out.push_str("[\n");
            for (i, x) in a.iter().enumerate() {
                pad(ind.saturating_add(2), out);
                pretty(x, ind.saturating_add(2), out);
                if i.saturating_add(1) < a.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            pad(ind, out);
            out.push(']');
        }
        Value::Object(_) => out.push_str("{}"),
        Value::Array(_) => out.push_str("[]"),
        Value::Number(n) => out.push_str(&cc_cad::js::num_to_string(n.as_f64().unwrap_or(f64::NAN))),
        x => out.push_str(&x.to_string()),
    }
}

/// `exportTools(data)`: the JSON text of a My tools file.
pub fn export_tools(data: &Tools, ids: &mut Ids) -> String {
    let (d, _) = clean_tools(&data.json(), ids);
    let mut s = String::new();
    pretty(&d.json(), 0, &mut s);
    s.push('\n');
    s
}

/// `mergeTools(data, groups)`: imported groups added after the ones there (a name already used gets " (2)"); then
/// cleaned again (ids made unique, key clashes dropped with a note).
pub fn merge_tools(data: &Tools, groups: &[Value], ids: &mut Ids) -> (Tools, Vec<Note>) {
    let mut out: Vec<Value> = data.json().get("groups").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut names: Vec<String> = data.groups.iter().map(|g| g.name.to_uppercase()).collect();
    for g in groups {
        let c = clean(g.get("name"), LIMIT_NAME);
        let base = if c.is_empty() { "My tools".to_string() } else { c };
        let mut name = base.clone();
        let mut k = 2u64;
        while names.contains(&name.to_uppercase()) {
            name = format!("{base} ({k})");
            k = k.saturating_add(1);
        }
        names.push(name.to_uppercase());
        out.push(json!({ "name": name, "tools": g.get("tools").cloned().unwrap_or(Value::Null) }));
    }
    clean_tools(&json!({ "format": "cadcraft-mytools", "version": 1, "groups": out }), ids)
}

// ------------------------------------------------------------------------------------------------ macro.js

/// The longest macro read (`MAX`).
const MACRO_MAX: usize = 4000;

/// One step of a button macro.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// ^C: Esc
    Cancel,
    /// type the text and press Enter
    Send(String),
    /// Enter alone
    Enter,
    /// \: wait for the person's own answer
    Pause,
    /// ^O ortho, ^G grid
    Toggle(&'static str),
}

/// `parseMacro(text)`'s result.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Macro {
    pub steps: Vec<Step>,
    /// a leading *: run again each time it ends, until Esc
    pub repeat: bool,
    /// the commands it starts (upper case, prefixes off), each once
    pub commands: Vec<String>,
    pub notes: Vec<Note>,
}
impl Macro {
    /// As macro.js returns it (the parity harness).
    pub fn json(&self) -> Value {
        let steps: Vec<Value> = self
            .steps
            .iter()
            .map(|s| match s {
                Step::Cancel => json!({ "kind": "cancel" }),
                Step::Send(t) => json!({ "kind": "send", "text": t }),
                Step::Enter => json!({ "kind": "enter" }),
                Step::Pause => json!({ "kind": "pause" }),
                Step::Toggle(w) => json!({ "kind": "toggle", "what": w }),
            })
            .collect();
        json!({ "steps": steps, "repeat": self.repeat, "commands": self.commands, "notes": self.notes.iter().map(Note::json).collect::<Vec<_>>() })
    }
}

/// `diesel(src, getvar)`: the small DIESEL subset; Err(name) on anything else ("syntax" for a call left open).
pub fn diesel(src: &str, getvar: &dyn Fn(&str) -> String) -> Result<String, String> {
    let s: Vec<char> = src.chars().collect();
    let mut i = 0usize;
    fn expr(s: &[char], i: &mut usize, gv: &dyn Fn(&str) -> String) -> Result<String, String> {
        let mut out = String::new();
        while let Some(&c) = s.get(*i) {
            if c == '$' && s.get(i.saturating_add(1)) == Some(&'(') {
                out.push_str(&call(s, i, gv)?);
                continue;
            }
            if c == ',' || c == ')' {
                break;
            }
            out.push(c);
            *i = i.saturating_add(1);
        }
        Ok(out)
    }
    fn call(s: &[char], i: &mut usize, gv: &dyn Fn(&str) -> String) -> Result<String, String> {
        *i = i.saturating_add(2);
        let mut args: Vec<String> = Vec::new();
        loop {
            args.push(expr(s, i, gv)?);
            match s.get(*i) {
                Some(',') => *i = i.saturating_add(1),
                Some(')') => {
                    *i = i.saturating_add(1);
                    break;
                }
                _ => return Err("syntax".into()),
            }
        }
        let name = cc_cad::js::trim(&args.remove(0)).to_lowercase();
        let a = |k: usize| args.get(k).cloned();
        match name.as_str() {
            "getvar" => Ok(gv(&cc_cad::js::trim(&a(0).unwrap_or_default()).to_uppercase())),
            "if" => {
                let c = a(0).unwrap_or_default();
                let c = cc_cad::js::trim(&c);
                Ok(if !c.is_empty() && c != "0" { a(1) } else { a(2) }.unwrap_or_default())
            }
            "=" => {
                let n = |v: Option<String>| v.map_or(f64::NAN, |x| cc_cad::js::str_to_number(&x));
                Ok(if n(a(0)) == n(a(1)) { "1" } else { "0" }.into())
            }
            "eq" => {
                let sv = |v: Option<String>| v.unwrap_or_else(|| "undefined".into());
                Ok(if sv(a(0)) == sv(a(1)) { "1" } else { "0" }.into())
            }
            _ => Err(name),
        }
    }
    let r = expr(&s, &mut i, getvar)?;
    let rest: String = s.get(i..).unwrap_or(&[]).iter().collect();
    Ok(r + &rest)
}

/// `.replace(/\+\s*\r?\n\s*/g, '')`: a "+" at a line's end joins the lines.
fn join_plus(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while let Some(&ch) = c.get(i) {
        if ch == '+' {
            let mut j = i.saturating_add(1);
            while c.get(j).is_some_and(|x| ws(*x)) {
                j = j.saturating_add(1);
            }
            if c.get(i.saturating_add(1)..j).is_some_and(|run| run.contains(&'\n')) {
                i = j;
                continue;
            }
        }
        out.push(ch);
        i = i.saturating_add(1);
    }
    out
}

/// `parseMacro(text, { getvar })`
pub fn parse_macro(text: &str, getvar: &dyn Fn(&str) -> String) -> Macro {
    let mut notes = Vec::new();
    let src = join_plus(&cc_cad::js::slice16(text, MACRO_MAX)).replace("\r\n", ";").replace('\n', ";");
    let low3: String = src.chars().take(3).collect::<String>().to_ascii_lowercase();
    let src = if low3 == "$m=" {
        match diesel(&src.chars().skip(3).collect::<String>(), getvar) {
            Ok(s) => s,
            Err(name) => return Macro { notes: vec![diesel_note(&name)], ..Macro::default() },
        }
    } else if src.contains("$(") {
        match diesel(&src, getvar) {
            Ok(s) => s,
            Err(name) => return Macro { notes: vec![diesel_note(&name)], ..Macro::default() },
        }
    } else {
        src
    };
    let mut m = Macro::default();
    let mut src: Vec<char> = src.chars().collect();
    if src.first() == Some(&'*') {
        m.repeat = true;
        src.remove(0);
    }
    let word = |w: &str| w.trim_start_matches(['\'', '_', '.']).to_string();
    let mut buf = String::new();
    let mut at_command = true;
    let mut commands: Vec<String> = Vec::new();
    let mut flush = |enter: bool, buf: &mut String, at_command: &mut bool, steps: &mut Vec<Step>| {
        if !buf.is_empty() {
            let mut txt = std::mem::take(buf);
            if *at_command {
                txt = word(&txt);
                if !txt.is_empty() {
                    commands.push(txt.to_uppercase());
                }
            }
            steps.push(Step::Send(txt));
            *at_command = false;
        } else if enter {
            steps.push(Step::Enter);
        }
    };
    let mut i = 0;
    while let Some(&ch) = src.get(i) {
        if ch == '^' && i.saturating_add(1) < src.len() {
            let k: String = src.get(i.saturating_add(1)).map(|c| c.to_uppercase().collect()).unwrap_or_default();
            i = i.saturating_add(2);
            match k.as_str() {
                "C" => {
                    flush(false, &mut buf, &mut at_command, &mut m.steps);
                    m.steps.push(Step::Cancel);
                    at_command = true;
                }
                "M" => flush(true, &mut buf, &mut at_command, &mut m.steps),
                "O" => {
                    flush(false, &mut buf, &mut at_command, &mut m.steps);
                    m.steps.push(Step::Toggle("ortho"));
                }
                "G" => {
                    flush(false, &mut buf, &mut at_command, &mut m.steps);
                    m.steps.push(Step::Toggle("grid"));
                }
                "P" | "I" | "Q" => {}
                _ => notes.push(Note::new("mytools.note.control", &[("key", format!("^{k}"))])),
            }
            continue;
        }
        if ch == ';' || ch == ' ' {
            flush(true, &mut buf, &mut at_command, &mut m.steps);
        } else if ch == '\\' {
            flush(false, &mut buf, &mut at_command, &mut m.steps);
            m.steps.push(Step::Pause);
        } else {
            buf.push(ch);
        }
        i = i.saturating_add(1);
    }
    flush(false, &mut buf, &mut at_command, &mut m.steps);
    drop(flush);
    let mut uniq: Vec<String> = Vec::new();
    for c in commands {
        if !uniq.contains(&c) {
            uniq.push(c);
        }
    }
    m.commands = uniq;
    m.notes = notes;
    m
}

fn diesel_note(name: &str) -> Note {
    Note::new("mytools.note.diesel", &[("name", if name.is_empty() { "?".into() } else { name.to_string() })])
}

/// The parity harness's case runner (`cadcraft-shot mytoolscases`, forks/tools/mytoolsdiff-cadpanes.mjs): one id
/// sequence for every case, in order. Kinds: `macro {text, vars}` · `clean {v}` · `merge {data, groups}` · `export {v}` ·
/// `icon {u}` · `iconBytes {b64}` · `cuix {b64, vars, known}`; each answers what the 0.5 function returns.
pub fn run_case(c: &Value, ids: &mut Ids) -> Value {
    let vars = c.get("vars").cloned().unwrap_or(Value::Null);
    let getvar = |n: &str| vars.get(n).and_then(Value::as_str).unwrap_or("").to_string();
    let known: Vec<String> = c
        .get("known")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    let lookup = |n: &str| known.iter().any(|k| *k == n.to_uppercase());
    let b64 = |k: &str| c.get(k).and_then(Value::as_str).and_then(atob).unwrap_or_default();
    let errs = |e: &[Note]| Value::Array(e.iter().map(Note::json).collect());
    match c.get("kind").and_then(Value::as_str).unwrap_or("") {
        "macro" => parse_macro(c.get("text").and_then(Value::as_str).unwrap_or(""), &getvar).json(),
        "clean" => {
            let (d, e) = clean_tools(c.get("v").unwrap_or(&Value::Null), ids);
            json!({ "data": d.json(), "errors": errs(&e) })
        }
        "merge" => {
            let (d, _) = clean_tools(c.get("data").unwrap_or(&Value::Null), ids);
            let groups = c.get("groups").and_then(Value::as_array).cloned().unwrap_or_default();
            let (m, e) = merge_tools(&d, &groups, ids);
            json!({ "data": m.json(), "errors": errs(&e) })
        }
        "export" => {
            // exportTools(v): v cleaned once, written with two-space indents and a newline
            let (d, _) = clean_tools(c.get("v").unwrap_or(&Value::Null), ids);
            let mut out = String::new();
            pretty(&d.json(), 0, &mut out);
            out.push('\n');
            json!(out)
        }
        "icon" => check_icon(c.get("u").unwrap_or(&Value::Null)).map_or(Value::Null, |k| json!(k)),
        "iconBytes" => icon_from_bytes(&b64("b64")).map_or(Value::Null, |u| json!(u)),
        "cuix" => match crate::cuix::read_cuix(&b64("b64"), &lookup, &getvar) {
            Ok(r) => r.json(),
            Err(e) => json!({ "error": e.0 }),
        },
        _ => json!({ "error": "kind" }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gv(n: &str) -> String {
        match n {
            "ORTHOMODE" => "1".into(),
            "CLAYER" => "Walls".into(),
            _ => String::new(),
        }
    }

    #[test]
    fn macros() {
        let m = parse_macro("^C^C_line \\\\;", &gv);
        assert_eq!(m.steps, vec![Step::Cancel, Step::Cancel, Step::Send("line".into()), Step::Pause, Step::Pause, Step::Enter]);
        assert_eq!(m.commands, vec!["LINE"]);
        let m = parse_macro("*^C^C.CIRCLE;0,0;5", &gv);
        assert!(m.repeat);
        assert_eq!(m.steps.last(), Some(&Step::Send("5".into())));
        let m = parse_macro("$M=$(if,$(getvar,orthomode),^O)LINE", &gv);
        assert_eq!(m.steps.first(), Some(&Step::Toggle("ortho")));
        let m = parse_macro("$M=$(substr,a,1)", &gv);
        assert_eq!(m.notes[0].fills[0].1, "substr");
        let m = parse_macro("$(if,1", &gv);
        assert_eq!(m.notes[0].fills[0].1, "syntax");
        let m = parse_macro("LINE +\n 0,0", &gv);
        assert_eq!(m.steps, vec![Step::Send("LINE".into()), Step::Send("0,0".into())]);
        let m = parse_macro("^Z", &gv);
        assert_eq!(m.notes[0].key, "mytools.note.control");
        // deep nesting stays within the 4000-character cap
        let deep = "$(if,".repeat(1990);
        let m = parse_macro(&deep, &gv);
        assert_eq!(m.notes.len(), 1);
    }

    #[test]
    fn tools_cleaned() {
        let mut ids = Ids { now_ms: 1_700_000_000_000.0, seq: 0 };
        let v = json!({ "format": "cadcraft-mytools", "groups": [
            { "name": "  A\u{1}b ", "tools": [
                { "label": "One", "macro": "^C^CLINE", "key": "ctrl+shift+l", "icon": "data:image/png;base64,iVBORw0KGgoAAAA=" },
                { "label": "Two", "macro": "CIRCLE", "key": "Ctrl+Shift+L", "id": "bad id" },
                { "label": "", "macro": "x" },
                { "label": "Svg", "macro": "x", "icon": "data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=" },
                { "label": "Bad", "macro": "x", "icon": "data:image/svg+xml;base64,PHA+" },
                7
            ] },
            { "tools": "no" },
        ] });
        let (d, errs) = clean_tools(&v, &mut ids);
        assert_eq!(d.groups.len(), 1);
        assert_eq!(d.groups[0].name, "A b");
        let t = &d.groups[0].tools;
        assert_eq!(t.len(), 4);
        assert_eq!(t[0].key.as_deref(), Some("Ctrl+Shift+L"));
        assert!(t[0].icon.is_some() && t[2].icon.is_some() && t[3].icon.is_none());
        assert_eq!(t[1].key, None);
        assert!(t[1].id.starts_with('t'));
        let keys: Vec<&str> = errs.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(keys, vec!["mytools.err.key.taken", "mytools.err.tool", "mytools.err.icon"]);
        let (m, _) = merge_tools(&d, &[json!({ "name": "a b", "tools": [{ "label": "Z", "macro": "Z" }] })], &mut ids);
        assert_eq!(m.groups[1].name, "a b (2)");
        assert!(export_tools(&m, &mut ids).starts_with("{\n  \"format\": \"cadcraft-mytools\",\n  \"version\": 1,"));
        assert_eq!(clean_tools(&json!(null), &mut ids).1.len(), 0);
        assert_eq!(clean_tools(&json!("x"), &mut ids).1.len(), 1);
        assert_eq!(atob("YWJj"), Some(b"abc".to_vec()));
        assert_eq!(atob("YWJ"), Some(b"ab".to_vec()));
        assert_eq!(atob("Y"), None);
        assert_eq!(btoa(b"ab"), "YWI=");
    }
}
