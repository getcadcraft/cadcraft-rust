//! script.js — "PartCraft script" (`.pcscript`, format 1): a line-oriented text that builds a part (statements at the
//! left edge, indented settings, sketch geometry and relations, `= expr` equations) and the writer that turns any part
//! back into one (positions to 9 decimals, numbers to 12 significant digits unless `exact`). Everything read is checked
//! again by `readPart`, its equations applied and its sketches solved; errors carry line numbers. Limits: 32 Mi UTF-16
//! units, 400 000 lines, 2 Mi units per line, 2000 features, 5000 geometry and 10 000 relations per sketch, 25 errors.
//!
//! A broken `json …` value is refused with V8's `JSON.parse` reason (v8json.rs). Spec gap: JSON that V8 reads but
//! serde_json does not (lone surrogate escapes, numbers past f64) is refused with serde_json's reason.

use crate::expr;
use crate::i18n::{F, t};
use crate::js;
use crate::jv;
use crate::params::{self, Params, Var};
use crate::params_part;
use crate::part;
use crate::sketch;
use crate::solver;
use serde_json::Value;
use std::collections::HashSet;

pub const SCRIPT_VERSION: f64 = 1.0;
pub const LIMIT_BYTES: usize = 32 * 1024 * 1024;
pub const LIMIT_LINES: usize = 400_000;
pub const LIMIT_LINE: usize = 2 * 1024 * 1024;
pub const LIMIT_ERRORS: usize = 25;

const TOP: [&str; 8] = ["pcscript", "part", "param", "rollback", "drawing", "study", "material", "configs"];
const RESERVED_WORDS: [&str; 7] = ["yes", "no", "true", "false", "none", "null", "json"];

/// One script problem: the line (1-based) and the message.
#[derive(Clone, Debug)]
pub struct Problem {
    pub line: Option<usize>,
    pub message: String,
}
/// `ScriptError`: the problems, and their joined message.
#[derive(Clone, Debug)]
pub struct ScriptError {
    pub errors: Vec<Problem>,
}
impl ScriptError {
    pub fn message(&self) -> String {
        self.errors
            .iter()
            .map(|e| match e.line {
                Some(l) => t("xch.script.at_line", &[("line", F::from(l)), ("message", F::S(e.message.clone()))]),
                None => e.message.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    pub fn to_json(&self) -> Value {
        jv::obj(vec![
            ("message", self.message().into()),
            (
                "errors",
                Value::Array(
                    self.errors
                        .iter()
                        .map(|e| {
                            let mut o = jv::obj(vec![("message", e.message.as_str().into())]);
                            if let Some(l) = e.line {
                                jv::set(&mut o, "line", jv::unum(l));
                            }
                            o
                        })
                        .collect(),
                ),
            ),
        ])
    }
}

fn clip(s: &str) -> String {
    js::slice16(s, 40)
}
fn is_ws(c: char) -> bool {
    js::is_ws(c)
}
fn is_line_term(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// `stripComment(s)`.
fn strip_comment(s: &str) -> &str {
    let mut q = false;
    let mut prev: Option<char> = None;
    let mut it = s.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if q {
            if c == '\\' {
                it.next();
                prev = Some('\\');
                continue;
            } else if c == '"' {
                q = false;
            }
            prev = Some(c);
            continue;
        }
        if c == '"' {
            q = true;
        } else if c == '#' && (i == 0 || prev.is_some_and(is_ws)) {
            return s.get(..i).unwrap_or("");
        }
        prev = Some(c);
    }
    s
}

struct Line {
    n: usize,
    indent: bool,
    body: String,
}

fn lex_lines(text: &str, errors: &mut Vec<Problem>) -> Result<Vec<Line>, ScriptError> {
    if js::len16(text) > LIMIT_BYTES {
        return Err(ScriptError {
            errors: vec![Problem { line: None, message: t("xch.script.max_mb", &[("mb", F::N(js::f(LIMIT_BYTES) / 1_048_576.0))]) }],
        });
    }
    let src = text.strip_prefix('\u{feff}').unwrap_or(text);
    // split(/\r\n|\n|\r/)
    let mut raw: Vec<&str> = Vec::new();
    let mut start = 0usize;
    let b = src.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        match b.get(i) {
            Some(b'\r') => {
                raw.push(src.get(start..i).unwrap_or(""));
                i = if b.get(i.saturating_add(1)) == Some(&b'\n') { i.saturating_add(2) } else { i.saturating_add(1) };
                start = i;
            }
            Some(b'\n') => {
                raw.push(src.get(start..i).unwrap_or(""));
                i = i.saturating_add(1);
                start = i;
            }
            _ => i = i.saturating_add(1),
        }
    }
    raw.push(src.get(start..).unwrap_or(""));
    if raw.len() > LIMIT_LINES {
        return Err(ScriptError {
            errors: vec![Problem { line: None, message: t("xch.script.max_lines", &[("n", F::from(LIMIT_LINES))]) }],
        });
    }
    let mut out = Vec::new();
    for (i, line) in raw.iter().enumerate() {
        let n = i.saturating_add(1);
        if js::len16(line) > LIMIT_LINE {
            errors.push(Problem { line: Some(n), message: t("xch.script.line_too_long", &[]) });
            continue;
        }
        if line.chars().any(|c| matches!(c, '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' | '\u{7f}')) {
            errors.push(Problem { line: Some(n), message: t("xch.script.control_chars", &[]) });
            continue;
        }
        let body = strip_comment(line).trim_end_matches(is_ws);
        if js::trim(body).is_empty() {
            continue;
        }
        out.push(Line { n, indent: body.chars().next().is_some_and(is_ws), body: js::trim(body).to_string() });
    }
    Ok(out)
}

#[derive(Clone, Debug)]
struct Tok {
    tok: Value,
    rest: String,
    quoted: bool,
    punct: bool,
}
impl Tok {
    fn s(&self) -> String {
        js::string_of(Some(&self.tok))
    }
}

/// `token(s)`.
fn token(s: &str) -> Result<Option<Tok>, String> {
    let s = s.trim_start_matches(is_ws);
    let Some(first) = s.chars().next() else { return Ok(None) };
    if first == '"' {
        let b = s.as_bytes();
        let mut i = 1usize;
        while i < b.len() {
            match b.get(i) {
                Some(b'\\') => {
                    i = i.saturating_add(2);
                    continue;
                }
                Some(b'"') => break,
                _ => i = i.saturating_add(1),
            }
        }
        if i >= b.len() {
            return Err(t("xch.script.quote_open", &[]));
        }
        let lit = s.get(..i.saturating_add(1)).unwrap_or("");
        let Ok(tok) = serde_json::from_str::<Value>(lit) else { return Err(t("xch.script.quote_escape", &[])) };
        return Ok(Some(Tok { tok, rest: s.get(i.saturating_add(1)..).unwrap_or("").to_string(), quoted: true, punct: false }));
    }
    let end = s.char_indices().find(|(_, c)| is_ws(*c) || matches!(c, ',' | '[' | ']' | '=')).map_or(s.len(), |(i, _)| i);
    if end == 0 {
        let l = first.len_utf8();
        return Ok(Some(Tok {
            tok: Value::String(first.to_string()),
            rest: s.get(l..).unwrap_or("").to_string(),
            quoted: false,
            punct: true,
        }));
    }
    Ok(Some(Tok {
        tok: Value::String(s.get(..end).unwrap_or("").to_string()),
        rest: s.get(end..).unwrap_or("").to_string(),
        quoted: false,
        punct: false,
    }))
}
/// `tokens(s)`.
fn tokens(s: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = s.to_string();
    let mut guard = 0usize;
    while let Some(tk) = token(&cur)? {
        guard = guard.saturating_add(1);
        if guard > 10_000_000 {
            break;
        }
        if !(tk.punct && tk.s() == ",") {
            out.push(tk.s());
        }
        cur = tk.rest;
    }
    Ok(out)
}

/// `splitMods(s)`.
fn split_mods(s: &str) -> Result<(String, Vec<String>), String> {
    let mut q = false;
    let mut depth: i64 = 0;
    let mut it = s.char_indices();
    while let Some((i, c)) = it.next() {
        if q {
            if c == '\\' {
                it.next();
            } else if c == '"' {
                q = false;
            }
            continue;
        }
        match c {
            '"' => q = true,
            '(' => depth = depth.saturating_add(1),
            ')' => depth = depth.saturating_sub(1),
            '[' if depth == 0 => {
                if !s.ends_with(']') {
                    return Err(t("xch.script.mods_end", &[]));
                }
                let inner_end = s.len().saturating_sub(1);
                let inner =
                    if i.saturating_add(1) <= inner_end { js::trim(s.get(i.saturating_add(1)..inner_end).unwrap_or("")) } else { "" };
                let mods = if inner.is_empty() {
                    Vec::new()
                } else {
                    inner.split(';').map(|x| js::trim(x).to_string()).filter(|x| !x.is_empty()).collect()
                };
                return Ok((js::trim(s.get(..i).unwrap_or("")).to_string(), mods));
            }
            _ => {}
        }
    }
    Ok((js::trim(s).to_string(), Vec::new()))
}

/// `splitTopCommas(s)`.
fn split_top_commas(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut depth: i64 = 0;
    let mut cur = String::new();
    for c in s.chars() {
        if c == '(' {
            depth = depth.saturating_add(1);
        }
        if c == ')' {
            depth = depth.saturating_sub(1);
        }
        if c == ',' && depth == 0 {
            out.push(js::trim(&cur).to_string());
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    if !js::trim(&cur).is_empty() || !out.is_empty() {
        out.push(js::trim(&cur).to_string());
    }
    out
}

/// `keyValue(s)`: /^(\S+)\s*(.*)$/ on the trimmed text.
fn key_value(s: &str) -> (String, String) {
    let s = js::trim(s);
    let kend = s.char_indices().find(|(_, c)| is_ws(*c)).map_or(s.len(), |(i, _)| i);
    if kend == 0 {
        return (String::new(), String::new());
    }
    let key = s.get(..kend).unwrap_or("");
    let rest = s.get(kend..).unwrap_or("");
    // \s* then (.*) to the end: `.` stops at line terminators, so try every split of the whitespace run
    let ws: Vec<(usize, char)> = rest.char_indices().take_while(|(_, c)| is_ws(*c)).collect();
    let mut cands: Vec<usize> = ws.iter().map(|(i, _)| *i).collect();
    cands.push(ws.last().map_or(0, |(i, c)| i.saturating_add(c.len_utf8())));
    cands.reverse();
    for at in cands {
        let tail = rest.get(at..).unwrap_or("");
        if !tail.chars().any(is_line_term) {
            return (key.to_string(), js::trim(tail).to_string());
        }
    }
    (String::new(), String::new())
}

/// `boolWord(v)`.
fn bool_word(v: &str) -> Option<bool> {
    match v {
        "yes" | "true" => Some(true),
        "no" | "false" => Some(false),
        _ => None,
    }
}
/// `topEquals(s)`: the index of a lone `=` outside quotes.
fn top_equals(s: &str) -> Option<usize> {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let mut q = false;
    let mut i = 0usize;
    while let Some(&(at, c)) = chars.get(i) {
        if q {
            if c == '\\' {
                i = i.saturating_add(1);
            } else if c == '"' {
                q = false;
            }
            i = i.saturating_add(1);
            continue;
        }
        if c == '"' {
            q = true;
        } else if c == '=' {
            let next = chars.get(i.saturating_add(1)).map(|x| x.1);
            let prev = if i > 0 { chars.get(i.saturating_sub(1)).map(|x| x.1) } else { None };
            if next != Some('=') && !matches!(prev, Some('=' | '<' | '>' | '!')) {
                return Some(at);
            }
        }
        i = i.saturating_add(1);
    }
    None
}
/// `jsonValue(s)`.
fn json_value(s: &str) -> Result<Value, String> {
    let v = js::trim(s);
    if !v.starts_with("json") {
        return Err(t("xch.script.json_form", &[]));
    }
    let body = js::trim(v.get(4..).unwrap_or(""));
    serde_json::from_str::<Value>(body).map_err(|e| {
        let why = crate::v8json::parse_error(body).unwrap_or_else(|| e.to_string());
        t("xch.script.json_bad", &[("why", F::S(js::slice16(&why, 120)))])
    })
}
/// `unquote(v)`.
fn unquote(v: &str) -> Result<String, String> {
    let s = js::trim(v);
    if s.starts_with('"') {
        let tk = token(s)?.ok_or_else(|| t("xch.script.quote_open", &[]))?;
        if !js::trim(&tk.rest).is_empty() {
            return Err(t("xch.script.after_quote", &[]));
        }
        return Ok(tk.s());
    }
    Ok(s.to_string())
}
fn one_token(v: &str, what: &str) -> Result<String, String> {
    let tk = tokens(v)?;
    match tk.as_slice() {
        [one] => Ok(one.clone()),
        _ => Err(t("xch.script.one_name", &[("key", F::from(what))])),
    }
}
fn word_ok(v: &str) -> bool {
    !v.is_empty() && v.bytes().all(|c| c.is_ascii_alphanumeric() || b"_.:@~#+-".contains(&c))
}
fn bare_ok(s: &str) -> bool {
    let b = s.as_bytes();
    b.first().is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_')
        && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-')
}
fn key_ok(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 41
        && b.first().is_some_and(u8::is_ascii_alphabetic)
        && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'-')
}
fn geo_id_ok(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 41
        && b.first().is_some_and(u8::is_ascii_alphabetic)
        && b.iter().all(|c| c.is_ascii_alphanumeric() || b"_:-".contains(c))
}
fn entity_word(s: &str) -> bool {
    let b = s.as_bytes();
    b.first().is_some_and(u8::is_ascii_alphabetic) && b.iter().all(|c| c.is_ascii_alphanumeric() || b"_:-".contains(c))
}

/// `genericValue(v, k)`.
fn generic_value(v: &str, k: &str) -> Result<Value, String> {
    if v.is_empty() {
        return Ok(true.into());
    }
    if let Some(b) = bool_word(v) {
        return Ok(b.into());
    }
    if v == "none" || v == "null" {
        return Ok(Value::Null);
    }
    if v.starts_with("json ") || v == "json" {
        return json_value(v);
    }
    if v.starts_with('"') {
        return unquote(v).map(Value::String);
    }
    if v.starts_with('=') {
        return Err(t("xch.script.no_equation", &[("key", F::from(k))]));
    }
    if let Some(l) = expr::literal(v) {
        return Ok(jv::num(l));
    }
    if word_ok(v) {
        return Ok(Value::String(v.to_string()));
    }
    Err(t("xch.script.bad_value", &[("key", F::from(k))]))
}

struct Stmt {
    kw: Option<String>,
    rest: String,
    line: usize,
    props: Vec<(usize, String)>,
}

struct Ctx<'a> {
    errors: &'a mut Vec<Problem>,
}
impl Ctx<'_> {
    fn fail(&mut self, line: Option<usize>, message: String) -> Result<(), ScriptError> {
        self.errors.push(Problem { line, message });
        if self.errors.len() >= LIMIT_ERRORS {
            return Err(ScriptError { errors: self.errors.clone() });
        }
        Ok(())
    }
}

/// What a script read gives.
#[derive(Clone, Debug)]
pub struct Read {
    pub part: Value,
    pub warnings: Vec<Problem>,
}

/// `scriptToPart(text, { readFile })`.
pub fn script_to_part(text: &str, read_file: Option<&dyn Fn(&str) -> Result<String, String>>) -> Result<Read, ScriptError> {
    let mut errors: Vec<Problem> = Vec::new();
    let lines = lex_lines(text, &mut errors)?;
    let mut cx = Ctx { errors: &mut errors };
    // pass 1
    let mut stmts: Vec<Stmt> = Vec::new();
    for l in &lines {
        if l.indent {
            match stmts.last_mut() {
                Some(st) => st.props.push((l.n, l.body.clone())),
                None => cx.fail(Some(l.n), t("xch.script.orphan", &[]))?,
            }
            continue;
        }
        let tk = match token(&l.body) {
            Ok(Some(tk)) => tk,
            Ok(None) => continue,
            Err(e) => {
                cx.fail(Some(l.n), e)?;
                continue;
            }
        };
        let kw = tk.s();
        if !TOP.contains(&kw.as_str()) && !part::known_type(&kw) {
            let types = part::TYPES.join(", ");
            cx.fail(
                Some(l.n),
                t("xch.script.not_statement", &[("word", F::S(clip(&kw))), ("statements", F::S(TOP.join(", "))), ("types", F::S(types))]),
            )?;
            stmts.push(Stmt { kw: None, rest: String::new(), line: l.n, props: Vec::new() });
            continue;
        }
        stmts.push(Stmt { kw: Some(kw), rest: js::trim(&tk.rest).to_string(), line: l.n, props: Vec::new() });
    }
    let mut raw = jv::obj(vec![
        ("format", part::FORMAT.into()),
        ("version", jv::num(part::VERSION)),
        ("name", "Part1".into()),
        ("units", "mm".into()),
        ("resolution", 64.into()),
        ("seq", 1.into()),
        ("rollback", Value::Null),
        ("features", Value::Array(Vec::new())),
    ]);
    let mut prm = Params::default();
    let mut param_line: Vec<(String, usize)> = Vec::new();
    let mut rollback_stmt: Option<usize> = None;
    let mut feat_stmts: Vec<usize> = Vec::new();
    for (si, st) in stmts.iter().enumerate() {
        let Some(kw) = &st.kw else { continue };
        let r: Result<(), String> = (|| {
            match kw.as_str() {
                "pcscript" => {
                    let v = js::str_num(&st.rest);
                    if !js::is_integer(v) || v < 1.0 {
                        cx.fail(Some(st.line), t("xch.script.version_word", &[])).map_err(|e| e.message())?;
                    } else if v > SCRIPT_VERSION {
                        cx.fail(Some(st.line), t("xch.script.newer", &[("v", F::N(v)), ("reads", F::N(SCRIPT_VERSION))]))
                            .map_err(|e| e.message())?;
                    }
                    no_props(st, &mut cx).map_err(|e| e.message())?;
                }
                "part" => {
                    if let Some(tk) = token(&st.rest)? {
                        jv::set(&mut raw, "name", js::slice16(&js::strip_ctrl(&tk.s()), 120).into());
                    }
                    for (n, body) in &st.props {
                        let (k, v) = key_value(body);
                        if k == "resolution" {
                            match expr::literal(&v) {
                                Some(x) if js::is_integer(x) && (8.0..=512.0).contains(&x) => jv::set(&mut raw, "resolution", jv::num(x)),
                                _ => cx.fail(Some(*n), t("xch.script.resolution", &[])).map_err(|e| e.message())?,
                            }
                        } else if k == "units" {
                            if v != "mm" {
                                cx.fail(Some(*n), t("xch.script.units", &[])).map_err(|e| e.message())?;
                            }
                        } else {
                            cx.fail(Some(*n), t("xch.script.not_part_setting", &[("word", F::S(clip(&k)))])).map_err(|e| e.message())?;
                        }
                    }
                }
                "param" => {
                    // /^([^\s=]+)\s*=\s*(.*)$/
                    let rest = &st.rest;
                    let nend = rest.char_indices().find(|(_, c)| is_ws(*c) || *c == '=').map_or(rest.len(), |(i, _)| i);
                    let after = rest.get(nend..).unwrap_or("").trim_start_matches(is_ws);
                    let Some(ex) = after.strip_prefix('=').filter(|_| nend > 0) else {
                        cx.fail(Some(st.line), t("xch.script.param_form", &[])).map_err(|e| e.message())?;
                        return Ok(());
                    };
                    let ex_full = ex.trim_start_matches(is_ws);
                    if ex_full.chars().any(is_line_term) {
                        cx.fail(Some(st.line), t("xch.script.param_form", &[])).map_err(|e| e.message())?;
                        return Ok(());
                    }
                    let name = rest.get(..nend).unwrap_or("").to_string();
                    if let Some(bad) = params::name_problem(Some(&Value::String(name.clone())), Some(&prm), None) {
                        cx.fail(Some(st.line), bad).map_err(|e| e.message())?;
                        return Ok(());
                    }
                    if let Err(e) = expr::parse(ex_full, None) {
                        cx.fail(Some(st.line), t("xch.script.named_error", &[("name", F::S(name.clone())), ("error", F::S(e.message))]))
                            .map_err(|e| e.message())?;
                        return Ok(());
                    }
                    if prm.vars.len() >= params::LIMIT_VARS {
                        cx.fail(Some(st.line), t("xch.script.max_params", &[("n", F::from(params::LIMIT_VARS))]))
                            .map_err(|e| e.message())?;
                        return Ok(());
                    }
                    let mut v = Var { name: name.clone(), expr: js::trim(ex_full).to_string(), note: None };
                    for (n, body) in &st.props {
                        let (k, val) = key_value(body);
                        if k == "note" {
                            v.note = Some(js::slice16(&js::strip_ctrl(&unquote(&val)?), params::LIMIT_NOTE));
                        } else {
                            cx.fail(Some(*n), t("xch.script.not_param_setting", &[("word", F::S(clip(&k)))])).map_err(|e| e.message())?;
                        }
                    }
                    prm.vars.push(v);
                    match param_line.iter_mut().find(|(k, _)| *k == name) {
                        Some(e) => e.1 = st.line,
                        None => param_line.push((name, st.line)),
                    }
                }
                "rollback" => {
                    if rollback_stmt.is_some() {
                        cx.fail(Some(st.line), t("xch.script.rollback_twice", &[])).map_err(|e| e.message())?;
                    }
                    rollback_stmt = Some(si);
                    no_props(st, &mut cx).map_err(|e| e.message())?;
                }
                "drawing" | "material" | "configs" => {
                    let v = json_value(&st.rest)?;
                    jv::set(&mut raw, kw, v);
                    no_props(st, &mut cx).map_err(|e| e.message())?;
                }
                "study" => {
                    let tk = token(&st.rest)?;
                    let ok = tk.as_ref().is_some_and(|tk| crate::re::study_key(&tk.s()));
                    let Some(tk) = tk.filter(|_| ok) else {
                        cx.fail(Some(st.line), t("xch.script.study_form", &[("form", F::from("study <key> json {…}"))]))
                            .map_err(|e| e.message())?;
                        return Ok(());
                    };
                    let v = json_value(js::trim(&tk.rest))?;
                    if raw.get("studies").is_none() {
                        jv::set(&mut raw, "studies", jv::obj(vec![]));
                    }
                    if let Some(s) = raw.get_mut("studies") {
                        jv::set(s, &tk.s(), v);
                    }
                    no_props(st, &mut cx).map_err(|e| e.message())?;
                }
                _ => feat_stmts.push(si),
            }
            Ok(())
        })();
        if let Err(e) = r {
            // JS: a fail() that throws inside the statement's try is caught and failed again (one more entry)
            cx.fail(Some(st.line), e)?;
        }
    }
    if feat_stmts.len() > 2000
        && let Some(&si) = feat_stmts.get(2000)
    {
        cx.fail(stmts.get(si).map(|s| s.line), t("xch.script.max_features", &[]))?;
    }
    let ev = params::evaluate_vars(&prm, None);
    for (name, msg) in ev.errors.iter() {
        let ln = param_line.iter().find(|(k, _)| k == name).map(|(_, l)| *l);
        cx.fail(ln, t("xch.script.named_error", &[("name", F::from(name)), ("error", F::S(msg.clone()))]))?;
    }
    let vals = ev.values.clone();
    let names: Vec<String> = prm.vars.iter().map(|v| v.name.clone()).collect();
    let calc = |src: &str, what: &str| -> Result<f64, String> {
        let r = expr::parse(src, Some(&names))
            .map_err(|e| t("xch.script.named_error", &[("name", F::from(what)), ("error", F::S(e.message))]))?;
        expr::evaluate(&r.ast, &vals).map_err(|e| t("xch.script.named_error", &[("name", F::from(what)), ("error", F::S(e.message))]))
    };

    // ids and names
    let mut tmp = part::new_part("Part1");
    let mut max_n: f64 = 0.0;
    struct Meta {
        si: usize,
        id: String,
        f: Value,
    }
    let mut metas: Vec<Meta> = Vec::new();
    for &si in &feat_stmts {
        let Some(st) = stmts.get(si) else { continue };
        let id_prop = st.props.iter().find(|(_, b)| b.starts_with("id") && b.chars().nth(2).is_some_and(is_ws));
        let mut id: Option<String> = None;
        if let Some((n, body)) = id_prop {
            let v = js::trim(body.get(2..).unwrap_or("")).to_string();
            if !crate::re::feature_id(&v) {
                cx.fail(Some(*n), t("xch.script.feature_id", &[]))?;
            } else if metas.iter().any(|m| m.id == v) {
                cx.fail(Some(*n), t("xch.script.id_twice", &[("id", F::S(v.clone()))]))?;
            } else {
                id = Some(v);
            }
        }
        let id = match id {
            Some(i) => i,
            None => {
                let mut i = format!("f{}", js::num_str(max_n + 1.0));
                let mut guard = 0usize;
                while metas.iter().any(|m| m.id == i) && guard < 100_000 {
                    i = format!("f{}", js::num_str(js::str_num(i.get(1..).unwrap_or("0")) + 1.0));
                    guard = guard.saturating_add(1);
                }
                i
            }
        };
        max_n = js::max(max_n, js::str_num(id.get(1..).unwrap_or("0")));
        let mut name: Option<String> = None;
        match token(&st.rest) {
            Ok(Some(tk)) => {
                name = Some(tk.s());
                if !js::trim(&tk.rest).is_empty()
                    && let Err(se) = cx.fail(Some(st.line), t("xch.script.after_name", &[("text", F::S(clip(js::trim(&tk.rest))))]))
                {
                    cx.fail(Some(st.line), se.message())?;
                }
            }
            Ok(None) => {}
            Err(e) => cx.fail(Some(st.line), e)?,
        }
        let props = name.as_ref().filter(|n| !n.is_empty()).map_or_else(|| jv::obj(vec![]), |n| jv::obj(vec![("name", n.as_str().into())]));
        let kw = st.kw.clone().unwrap_or_default();
        let mut f = part::add_feature(&mut tmp, &kw, &props)
            .unwrap_or_else(|_| jv::obj(vec![("type", kw.as_str().into()), ("name", "undefined1".into())]));
        jv::set(&mut f, "id", id.as_str().into());
        metas.push(Meta { si, id, f });
    }
    let by_name = |tok: &str, metas: &[Meta]| -> Vec<String> {
        metas.iter().filter(|m| jv::is_str(m.f.get("name"), tok)).map(|m| m.id.clone()).collect()
    };
    let fref = |tok: &str, metas: &[Meta]| -> Result<String, String> {
        let hits = by_name(tok, metas);
        if hits.len() == 1 {
            return Ok(hits.first().cloned().unwrap_or_default());
        }
        if hits.len() > 1 {
            let ids = hits.iter().cloned().reduce(|a, b| t("xch.script.or", &[("a", F::S(a)), ("b", F::S(b))])).unwrap_or_default();
            return Err(t("xch.script.name_twice", &[("name", F::S(clip(tok))), ("ids", F::S(ids))]));
        }
        if metas.iter().any(|m| m.id == tok) {
            return Ok(tok.to_string());
        }
        Err(t("xch.script.no_feature", &[("name", F::S(clip(tok)))]))
    };

    // pass 3: features
    let mut links: Vec<(String, String, usize)> = Vec::new();
    let mut sketch_lines: Vec<(String, usize)> = Vec::new();
    let mut feats: Vec<Value> = Vec::new();
    for m in &metas {
        let Some(st) = stmts.get(m.si) else { continue };
        let mut feat = m.f.clone();
        let fty = jv::s(&m.f, "type").to_string();
        let mut done: HashSet<String> = HashSet::new();
        let mut listed: HashSet<String> = HashSet::new();
        let mut sk = SkBuild::default();
        for (n, body) in &st.props {
            let r: Result<(), String> = (|| {
                if body.starts_with("id") && body.chars().nth(2).is_some_and(is_ws) {
                    return Ok(());
                }
                if fty == "sketch" {
                    return sketch_prop(&mut feat, &mut sk, *n, body, &calc, &|tok| fref(tok, &metas));
                }
                let (k, v) = key_value(body);
                if !key_ok(&k) {
                    return Err(t("xch.script.not_setting_name", &[("word", F::S(clip(&k)))]));
                }
                let once = k != "edge" && k != "face";
                if once && done.contains(&k) {
                    return Err(t("xch.script.set_twice", &[("key", F::S(k.clone()))]));
                }
                done.insert(k.clone());
                feature_prop(&mut feat, &k, &v, *n, &mut listed, &mut links, &calc, &|tok| fref(tok, &metas), read_file)
            })();
            if let Err(e) = r {
                cx.fail(Some(*n), e)?;
            }
        }
        if fty == "sketch" {
            if let Err(e) = finish_sketch(&mut feat, sk, &mut cx, &mut links) {
                cx.fail(Some(st.line), e)?;
            }
            sketch_lines.push((m.id.clone(), st.line));
        }
        feats.push(feat);
    }
    if !cx.errors.is_empty() {
        return Err(ScriptError { errors: cx.errors.clone() });
    }
    for f in &mut feats {
        jv::del(f, "_file");
    }
    jv::set(&mut raw, "features", Value::Array(feats.clone()));
    if let Some(si) = rollback_stmt
        && let Some(st) = stmts.get(si)
    {
        let v = js::trim(&st.rest).to_string();
        if !v.is_empty() && v != "end" {
            match unquote(&v).and_then(|u| fref(&u, &metas)) {
                Ok(id) => {
                    let i = feats.iter().position(|f| jv::is_str(f.get("id"), &id));
                    jv::set(&mut raw, "rollback", i.map_or(jv::num(-1.0), jv::unum));
                }
                Err(e) => cx.fail(Some(st.line), e)?,
            }
        }
    }
    prm.links = links.iter().map(|(target, ex, _)| params::Link { target: target.clone(), expr: ex.clone() }).collect();
    if !prm.vars.is_empty() || !prm.links.is_empty() {
        jv::set(&mut raw, "params", prm.to_json());
    }
    if !cx.errors.is_empty() {
        return Err(ScriptError { errors: cx.errors.clone() });
    }
    let mut pt = match part::read_part(&raw) {
        Ok(p) => p,
        Err(e) => {
            let fname = e.find(": ").and_then(|i| e.get(..i)).filter(|s| !s.is_empty()).map(str::to_string);
            let mline =
                fname.and_then(|n| metas.iter().find(|x| jv::is_str(x.f.get("name"), &n)).and_then(|x| stmts.get(x.si)).map(|s| s.line));
            let link_hit = e.find("drives \"").and_then(|i| {
                let r = e.get(i.saturating_add(8)..)?;
                r.find('"').and_then(|j| r.get(..j)).map(str::to_string)
            });
            let line = mline.or_else(|| link_hit.and_then(|lh| links.iter().find(|(tg, _, _)| *tg == lh).map(|(_, _, l)| *l)));
            return Err(ScriptError { errors: vec![Problem { line, message: e }] });
        }
    };
    let mut warnings = Vec::new();
    let ap = params_part::apply_part_params(&mut pt, None);
    let tg = params_part::targets(&pt);
    for er in &ap.errors {
        let ln = param_line
            .iter()
            .find(|(k, _)| *k == er.at)
            .map(|(_, l)| *l)
            .or_else(|| links.iter().find(|(tg, _, _)| *tg == er.at).map(|(_, _, l)| *l))
            .or_else(|| metas.iter().find(|x| jv::is_str(x.f.get("name"), &er.at)).and_then(|x| stmts.get(x.si)).map(|s| s.line));
        let label = tg.get(&er.at).map(|x| x.label.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| er.at.clone());
        cx.errors.push(Problem {
            line: ln,
            message: t("xch.script.named_error", &[("name", F::S(label)), ("error", F::S(er.message.clone()))]),
        });
    }
    if !cx.errors.is_empty() {
        return Err(ScriptError { errors: cx.errors.clone() });
    }
    let n = part::features(&pt).len();
    for i in 0..n {
        let Some(f) = pt.get_mut("features").and_then(|f| f.get_mut(i)) else { continue };
        if jv::s(f, "type") != "sketch" {
            continue;
        }
        let fid = js::str_of(f.get("id"));
        let fname = js::str_of(f.get("name"));
        let Some(sk) = f.get_mut("sketch") else { continue };
        let r = solver::solve_sketch(sk, None, None);
        let ln = sketch_lines.iter().find(|(k, _)| *k == fid).map(|(_, l)| *l);
        let mut ids = r.conflicts.clone();
        ids.extend(r.redundant.iter().cloned());
        if !r.ok {
            warnings.push(Problem {
                line: ln,
                message: t("xch.script.unsolvable", &[("name", F::S(fname)), ("list", F::S(describe_ids(sk, &ids)))]),
            });
        } else if r.status == "over" {
            warnings.push(Problem {
                line: ln,
                message: t("xch.script.over_defined", &[("name", F::S(fname)), ("list", F::S(describe_ids(sk, &ids)))]),
            });
        }
    }
    Ok(Read { part: pt, warnings })
}

fn describe_ids(sk: &Value, ids: &[String]) -> String {
    let mut seen: Vec<String> = Vec::new();
    for id in ids {
        if !seen.contains(id) {
            seen.push(id.clone());
        }
    }
    let s = seen
        .iter()
        .map(|id| match sketch::constraints(sk).iter().find(|k| jv::is_str(k.get("id"), id)) {
            Some(c) => {
                let refs: Vec<String> = jv::arr(c.get("refs")).iter().map(|v| js::str_of(Some(v))).collect();
                format!("{} ({})", describe_with(c), refs.join(" "))
            }
            None => id.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ");
    if s.is_empty() { t("xch.script.its_relations", &[]) } else { s }
}
/// `S.describe(c, formatNumber)`.
fn describe_with(c: &Value) -> String {
    let ty = jv::s(c, "type");
    match sketch::def_of(ty) {
        None => ty.to_string(),
        Some(Err(())) => "undefined".into(),
        Some(Ok(d)) => {
            if !d.dim {
                return t(d.label_key, &[]);
            }
            let v = expr::format_number(jv::n(c, "value"));
            match ty {
                "angle" => t("model.sketch.describe.angle", &[("value", F::S(v))]),
                "diameter" => t("model.sketch.describe.diameter", &[("value", F::S(v))]),
                "radius" => t("model.sketch.describe.radius", &[("value", F::S(v))]),
                "offset" => t("model.sketch.describe.offset", &[("value", F::S(v))]),
                _ => t("model.sketch.describe.dim", &[("label", F::S(t(d.label_key, &[]))), ("value", F::S(v))]),
            }
        }
    }
}

fn no_props(st: &Stmt, cx: &mut Ctx) -> Result<(), ScriptError> {
    for (n, _) in &st.props {
        cx.fail(Some(*n), t("xch.script.no_settings", &[("kw", F::S(st.kw.clone().unwrap_or_default()))]))?;
    }
    Ok(())
}

fn vec_of(s: &str, n: usize, calc: &dyn Fn(&str, &str) -> Result<f64, String>) -> Result<Vec<f64>, String> {
    let mut parts = split_top_commas(s);
    if parts.len() == 1 {
        // split(/\s+/) on a trimmed string: runs of whitespace are one separator ("" stays one empty part)
        parts = if js::trim(s).is_empty() {
            vec![String::new()]
        } else {
            js::trim(s).split(is_ws).filter(|p| !p.is_empty()).map(str::to_string).collect()
        };
    }
    if parts.len() != n || parts.iter().any(String::is_empty) {
        return Err(t("xch.script.expected_numbers", &[("n", F::from(n))]));
    }
    let mut out = Vec::with_capacity(n);
    for (i, x) in parts.iter().enumerate() {
        out.push(calc(x, &t("xch.script.what.number", &[("n", F::from(i.saturating_add(1)))]))?);
    }
    Ok(out)
}

fn read_plane_s(
    v: &str,
    calc: &dyn Fn(&str, &str) -> Result<f64, String>,
    fref: &dyn Fn(&str) -> Result<String, String>,
) -> Result<Value, String> {
    let s = js::trim(v);
    if s.starts_with("json ") {
        return json_value(s);
    }
    let tk = tokens(s)?;
    let g = |i: usize| tk.get(i).map_or("", String::as_str);
    if tk.len() == 1 && matches!(g(0), "front" | "top" | "right") {
        return Ok(jv::obj(vec![("kind", "base".into()), ("name", g(0).into())]));
    }
    if g(0) == "face" && g(1) == "named" && tk.len() == 3 {
        let feat = crate::re::name_features(g(2)).into_iter().next().unwrap_or_default();
        return Ok(jv::obj(vec![("kind", "face".into()), ("feature", feat.into()), ("face", "named".into()), ("name", g(2).into())]));
    }
    if g(0) == "face" && (g(2) == "start" || g(2) == "end") && tk.len() == 3 {
        return Ok(jv::obj(vec![("kind", "face".into()), ("feature", fref(g(1))?.into()), ("face", g(2).into())]));
    }
    if g(0) == "face" && g(2) == "side" && tk.len() == 4 {
        return Ok(jv::obj(vec![
            ("kind", "face".into()),
            ("feature", fref(g(1))?.into()),
            ("face", "side".into()),
            ("entity", g(3).into()),
        ]));
    }
    if g(0) == "plane" && tk.len() == 2 {
        return Ok(jv::obj(vec![("kind", "ref".into()), ("feature", fref(g(1))?.into())]));
    }
    if g(0) == "fixed"
        && let Some((a, b)) = split_lazy(s, "fixed", "normal")
    {
        return Ok(jv::obj(vec![("kind", "fixed".into()), ("o", jv::nums(&vec_of(&a, 3, calc)?)), ("n", jv::nums(&vec_of(&b, 3, calc)?))]));
    }
    Err(t(
        "xch.script.plane_form",
        &[(
            "form",
            F::from(
                "front, top, right, face <feature> start|end, face <feature> side <curve>, face named <face>, plane <plane feature>, fixed x, y, z normal x, y, z",
            ),
        )],
    ))
}

/// /^fixed\s+(.+?)\s+normal\s+(.+)$/ (word = "normal" / "dir").
fn split_lazy(s: &str, head: &str, word: &str) -> Option<(String, String)> {
    let rest = s.strip_prefix(head)?;
    let ws = rest.chars().take_while(|c| is_ws(*c)).map(char::len_utf8).sum::<usize>();
    if ws == 0 {
        return None;
    }
    let body = rest.get(ws..)?;
    if body.chars().any(is_line_term) {
        return None;
    }
    // the shortest first group: scan for "\s+word\s+" with at least one char before
    let idx: Vec<(usize, char)> = body.char_indices().collect();
    for (k, &(i, c)) in idx.iter().enumerate() {
        if k == 0 || !is_ws(c) {
            continue;
        }
        let first = body.get(..i)?;
        let after_ws = body.get(i..)?.trim_start_matches(is_ws);
        if let Some(r) = after_ws.strip_prefix(word) {
            let w2 = r.chars().take_while(|c| is_ws(*c)).map(char::len_utf8).sum::<usize>();
            let second = r.get(w2..).unwrap_or("");
            if w2 > 0 && !second.is_empty() && !first.is_empty() {
                return Some((first.to_string(), second.to_string()));
            }
        }
    }
    None
}

fn read_dir_s(
    v: &str,
    calc: &dyn Fn(&str, &str) -> Result<f64, String>,
    fref: &dyn Fn(&str) -> Result<String, String>,
) -> Result<Value, String> {
    let s = js::trim(v);
    if s.starts_with("json ") {
        return json_value(s);
    }
    let tk = tokens(s)?;
    let g = |i: usize| tk.get(i).map_or("", String::as_str);
    if tk.len() == 1 && matches!(g(0), "x" | "y" | "z") {
        return Ok(jv::obj(vec![("kind", "world".into()), ("axis", g(0).into())]));
    }
    if g(0) == "line" && tk.len() == 3 {
        return Ok(jv::obj(vec![("kind", "line".into()), ("sketch", fref(g(1))?.into()), ("entity", g(2).into())]));
    }
    if g(0) == "axis-of" && tk.len() == 2 {
        return Ok(jv::obj(vec![("kind", "axis".into()), ("feature", fref(g(1))?.into())]));
    }
    if g(0) == "ref" && tk.len() == 2 {
        return Ok(jv::obj(vec![("kind", "ref".into()), ("feature", fref(g(1))?.into())]));
    }
    if g(0) == "fixed"
        && let Some((a, b)) = split_lazy(s, "fixed", "dir")
    {
        return Ok(jv::obj(vec![("kind", "fixed".into()), ("o", jv::nums(&vec_of(&a, 3, calc)?)), ("d", jv::nums(&vec_of(&b, 3, calc)?))]));
    }
    Err(t(
        "xch.script.dir_form",
        &[("form", F::from("x, y, z, line <sketch> <line>, axis-of <revolve>, ref <axis or plane feature>, fixed x, y, z dir x, y, z"))],
    ))
}

#[allow(clippy::too_many_arguments)]
fn feature_prop(
    feat: &mut Value,
    k: &str,
    v: &str,
    line: usize,
    listed: &mut HashSet<String>,
    links: &mut Vec<(String, String, usize)>,
    calc: &dyn Fn(&str, &str) -> Result<f64, String>,
    fref: &dyn Fn(&str) -> Result<String, String>,
    read_file: Option<&dyn Fn(&str) -> Result<String, String>>,
) -> Result<(), String> {
    let ty = jv::s(feat, "type").to_string();
    if k == "suppressed" {
        let b = if v.is_empty() { Some(true) } else { bool_word(v) };
        let Some(b) = b else { return Err(t("xch.script.suppressed", &[])) };
        if b {
            jv::set(feat, "suppressed", true.into());
        } else {
            jv::del(feat, "suppressed");
        }
        return Ok(());
    }
    if k == "sketch" && matches!(ty.as_str(), "extrude" | "revolve" | "hole" | "sweep") {
        let id = fref(&one_token(v, "sketch")?)?;
        jv::set(feat, "sketch", id.into());
        return Ok(());
    }
    if k == "seeds" && matches!(ty.as_str(), "lpattern" | "cpattern" | "mirror") {
        let seeds = if js::trim(v) == "body" && ty == "mirror" {
            Value::String("body".into())
        } else {
            let mut out = Vec::new();
            for tk in tokens(v)? {
                out.push(Value::String(fref(&tk)?));
            }
            Value::Array(out)
        };
        let empty = seeds.as_array().is_some_and(Vec::is_empty);
        jv::set(feat, "seeds", seeds);
        if empty {
            return Err(t("xch.script.seeds", &[]));
        }
        return Ok(());
    }
    if (k == "dir" && ty == "lpattern") || (k == "axis" && ty == "cpattern") {
        let d = read_dir_s(v, calc, fref)?;
        jv::set(feat, k, d);
        return Ok(());
    }
    if k == "axis" && ty == "revolve" {
        let tk = tokens(v)?;
        if tk.len() == 1 && tk.first().map(String::as_str) == Some("first") {
            jv::set(feat, "axis", Value::Null);
        } else if tk.len() == 1 && !matches!(tk.first().map(String::as_str), Some("x" | "y" | "z")) {
            jv::set(feat, "axis", tk.first().cloned().unwrap_or_default().into());
        } else {
            let d = read_dir_s(v, calc, fref)?;
            jv::set(feat, "axis", d);
        }
        return Ok(());
    }
    if k == "plane" && ty == "mirror" {
        let p = read_plane_s(v, calc, fref)?;
        jv::set(feat, "plane", p);
        return Ok(());
    }
    if (k == "edge" || k == "face") && v != "none" && !v.starts_with("json") {
        let key = format!("{k}s");
        if !listed.contains(&key) {
            jv::set(feat, &key, Value::Array(Vec::new()));
            listed.insert(key.clone());
        }
        if jv::arr(feat.get(&key)).len() >= 2000 {
            return Err(t(if key == "edges" { "xch.script.max_edges" } else { "xch.script.max_faces" }, &[]));
        }
        let item = if k == "edge" { read_edge(v, calc)? } else { read_face(v, calc)? };
        if let Some(a) = feat.get_mut(&key).and_then(Value::as_array_mut) {
            a.push(item);
        }
        return Ok(());
    }
    if k == "step-file" && ty == "import" {
        let path = unquote(v)?;
        let Some(rf) = read_file else { return Err(t("xch.script.needs_step", &[("file", F::S(clip(&path)))])) };
        let txt = rf(&path).map_err(|e| t("xch.script.cannot_read", &[("file", F::S(clip(&path))), ("why", F::S(e))]))?;
        jv::set(feat, "step", txt.into());
        if !jv::truthy(feat.get("_file")) {
            let base = path.rsplit(['\\', '/']).next().unwrap_or("");
            jv::set(feat, "file", js::slice16(base, 200).into());
        }
        return Ok(());
    }
    if k == "file" && ty == "import" {
        jv::set(feat, "file", unquote(v)?.into());
        jv::set(feat, "_file", true.into());
        return Ok(());
    }
    if let Some(spec) = params_part::fields_of(&ty).into_iter().find(|f| f.field == k) {
        return number_prop(feat, k, v, line, spec, links, calc);
    }
    let gv = generic_value(v, k)?;
    jv::set(feat, k, gv);
    Ok(())
}

fn number_prop(
    feat: &mut Value,
    k: &str,
    v: &str,
    line: usize,
    spec: &params_part::Field,
    links: &mut Vec<(String, String, usize)>,
    calc: &dyn Fn(&str, &str) -> Result<f64, String>,
) -> Result<(), String> {
    let mut ex = js::trim(v).to_string();
    let mut linked = false;
    if let Some(r) = ex.strip_prefix('=') {
        ex = js::trim(r).to_string();
        linked = true;
    }
    if ex.is_empty() {
        return Err(t("xch.script.needs_value", &[("key", F::from(k))]));
    }
    let lit = if linked { None } else { expr::literal(&ex) };
    if let Some(l) = lit {
        jv::set(feat, k, jv::num(l));
        return Ok(());
    }
    if ex == "none" && !linked {
        jv::set(feat, k, Value::Null);
        return Ok(());
    }
    let mut val = calc(&ex, k)?;
    if spec.integer {
        val = js::round(val);
    }
    let tg = params::Target {
        key: String::new(),
        label: format!("{} {}", js::str_of(feat.get("name")), params_part::label(spec)),
        kind: spec.kind,
        min: spec.min,
        max: spec.max,
        integer: spec.integer,
        feature: String::new(),
        field: None,
        constraint: None,
        dim: None,
    };
    if let Some(vp) = params::value_problem(&tg, val) {
        return Err(t("xch.script.expr_error", &[("expr", F::S(ex)), ("error", F::S(vp))]));
    }
    jv::set(feat, k, jv::num(val));
    links.push((format!("{}.{k}", js::str_of(feat.get("id"))), ex, line));
    Ok(())
}

fn read_edge(v: &str, calc: &dyn Fn(&str, &str) -> Result<f64, String>) -> Result<Value, String> {
    let (head, mods) = split_mods(v)?;
    let tk = tokens(&head)?;
    if tk.len() != 2 {
        return Err(t("xch.script.edge_form", &[("form", F::from("edge <face> <face> [at x, y, z]"))]));
    }
    let mut e = jv::obj(vec![("faces", jv::strs(&tk))]);
    for m in &mods {
        let (mk, mv) = key_value(m);
        if mk == "at" {
            jv::set(&mut e, "at", jv::nums(&vec_of(&mv, 3, calc)?));
        } else {
            return Err(t("xch.script.not_edge_setting", &[("word", F::S(clip(&mk)))]));
        }
    }
    Ok(e)
}
fn read_face(v: &str, calc: &dyn Fn(&str, &str) -> Result<f64, String>) -> Result<Value, String> {
    let (head, mods) = split_mods(v)?;
    let tk = tokens(&head)?;
    if tk.len() != 1 {
        return Err(t("xch.script.face_form", &[("form", F::from("face <face name> [at x, y, z]"))]));
    }
    let mut r = jv::obj(vec![("name", tk.first().cloned().unwrap_or_default().into())]);
    for m in &mods {
        let (mk, mv) = key_value(m);
        if mk == "at" {
            jv::set(&mut r, "at", jv::nums(&vec_of(&mv, 3, calc)?));
        } else {
            return Err(t("xch.script.not_face_setting", &[("word", F::S(clip(&mk)))]));
        }
    }
    Ok(r)
}

#[derive(Default)]
struct SkBuild {
    started: bool,
    entities: Vec<Value>,
    constraints: Vec<SkCon>,
    ids: Vec<String>,
}
struct SkCon {
    id: Option<String>,
    ty: String,
    refs: Vec<String>,
    line: usize,
    value: Option<f64>,
    link: Option<String>,
    sign: Option<Option<f64>>,
    kind: Option<Option<String>>,
    off: Option<Vec<f64>>,
    at: Option<Vec<f64>>,
    fix_value: Option<Vec<f64>>,
    of: Option<String>,
}
impl SkBuild {
    fn has(&self, id: &str) -> bool {
        id == "O" || self.ids.iter().any(|x| x == id)
    }
}

fn sketch_prop(
    feat: &mut Value,
    sk: &mut SkBuild,
    line: usize,
    body: &str,
    calc: &dyn Fn(&str, &str) -> Result<f64, String>,
    fref: &dyn Fn(&str) -> Result<String, String>,
) -> Result<(), String> {
    sk.started = true;
    let (head, mods) = split_mods(body)?;
    let mut tk = token(&head)?;
    let mut label: Option<String> = None;
    if let Some(t0) = tk.clone()
        && !t0.quoted
        && t0.s().ends_with(':')
        && js::len16(&t0.s()) > 1
    {
        let s0 = t0.s();
        label = Some(s0.get(..s0.len().saturating_sub(1)).unwrap_or("").to_string());
        tk = token(&t0.rest)?;
    }
    let Some(tk) = tk else { return Err(t("xch.script.empty_sketch_line", &[])) };
    let kw = tk.s();
    let rest = js::trim(&tk.rest).to_string();
    if kw == "on" {
        if label.is_some() || !mods.is_empty() {
            return Err(t("xch.script.on_plane_only", &[]));
        }
        let p = read_plane_s(&rest, calc, fref)?;
        jv::set(feat, "plane", p);
        return Ok(());
    }
    if kw == "seq" {
        return Ok(());
    }
    if matches!(kw.as_str(), "point" | "line" | "circle" | "arc" | "spline") {
        if label.is_some() {
            return Err(t("xch.script.geometry_label", &[]));
        }
        if sk.entities.len() >= 5000 {
            return Err(t("xch.script.max_geometry", &[]));
        }
        let Some(it) = token(&rest)? else {
            let ex = format!("{}1", kw.chars().next().unwrap_or('x'));
            return Err(t("xch.script.needs_name", &[("kw", F::S(kw.clone())), ("example", F::S(ex))]));
        };
        let id = it.s();
        if !geo_id_ok(&id) || id == "O" {
            return Err(t("xch.script.bad_geometry_name", &[("name", F::S(clip(&id)))]));
        }
        if sk.has(&id) {
            return Err(t("xch.script.geometry_twice", &[("name", F::S(id))]));
        }
        let after = js::trim(&it.rest).to_string();
        let mut e = jv::obj(vec![("id", id.as_str().into()), ("type", kw.as_str().into())]);
        let pt = |tok: &str, sk: &SkBuild| -> Result<String, String> {
            if !sk.entities.iter().any(|x| jv::is_str(x.get("id"), tok) && jv::s(x, "type") == "point") && tok != "O" {
                return Err(t("xch.script.not_point", &[("name", F::from(tok))]));
            }
            Ok(tok.to_string())
        };
        match kw.as_str() {
            "point" => {
                let xy = vec_of(&after, 2, calc)?;
                jv::set(&mut e, "x", jv::num(xy.first().copied().unwrap_or(f64::NAN)));
                jv::set(&mut e, "y", jv::num(xy.get(1).copied().unwrap_or(f64::NAN)));
            }
            "line" => {
                let a = tokens(&after)?;
                if a.len() != 2 {
                    return Err(t("xch.script.line_form", &[("form", F::from("line <id> <point> <point>"))]));
                }
                jv::set(&mut e, "p1", pt(a.first().map_or("", String::as_str), sk)?.into());
                jv::set(&mut e, "p2", pt(a.get(1).map_or("", String::as_str), sk)?.into());
            }
            "circle" => {
                let a = token(&after)?;
                let Some(a) = a.filter(|a| !js::trim(&a.rest).is_empty()) else {
                    return Err(t("xch.script.circle_form", &[("form", F::from("circle <id> <centre point> <radius>"))]));
                };
                jv::set(&mut e, "c", pt(&a.s(), sk)?.into());
                jv::set(&mut e, "r", jv::num(calc(js::trim(&a.rest), &t("xch.script.what.radius", &[]))?));
            }
            "arc" => {
                let a = tokens(&after)?;
                if a.len() != 3 {
                    return Err(t("xch.script.arc_form", &[("form", F::from("arc <id> <centre> <start> <end>"))]));
                }
                jv::set(&mut e, "c", pt(a.first().map_or("", String::as_str), sk)?.into());
                jv::set(&mut e, "s", pt(a.get(1).map_or("", String::as_str), sk)?.into());
                jv::set(&mut e, "e", pt(a.get(2).map_or("", String::as_str), sk)?.into());
            }
            _ => {
                let a = tokens(&after)?;
                let k0 = a.first().map_or("", String::as_str);
                if k0 != "fit" && k0 != "control" {
                    return Err(t("xch.script.spline_form", &[("form", F::from("spline <id> fit|control <points…>"))]));
                }
                jv::set(&mut e, "kind", k0.into());
                let mut pts = Vec::new();
                for p in a.iter().skip(1) {
                    pts.push(Value::String(pt(p, sk)?));
                }
                jv::set(&mut e, "pts", Value::Array(pts));
            }
        }
        for m in &mods {
            let (mk, mv) = key_value(m);
            if mk == "construction" && mv.is_empty() {
                jv::set(&mut e, "construction", true.into());
            } else if kw == "spline" && mk == "closed" && mv.is_empty() {
                jv::set(&mut e, "closed", true.into());
            } else if kw == "spline" && (mk == "a0" || mk == "a1") {
                jv::set(&mut e, &mk, jv::num(calc(&mv, &mk)?));
            } else if kw == "spline" && mk == "knots" {
                let parts: Vec<String> = split_top_commas(&mv)
                    .iter()
                    .flat_map(|x| x.split(is_ws).map(str::to_string).collect::<Vec<_>>())
                    .filter(|x| !x.is_empty())
                    .collect();
                let mut ks = Vec::new();
                for (i, x) in parts.iter().enumerate() {
                    ks.push(calc(x, &t("xch.script.what.knot", &[("n", F::from(i.saturating_add(1)))]))?);
                }
                jv::set(&mut e, "knots", jv::nums(&ks));
            } else {
                return Err(t("xch.script.not_geometry_setting", &[("word", F::S(clip(m))), ("kw", F::S(kw.clone()))]));
            }
        }
        sk.entities.push(e);
        sk.ids.push(id);
        let _ = line;
        return Ok(());
    }
    let Some(Ok(def)) = sketch::def_of(&kw) else {
        let list = sketch::CONSTRAINTS.iter().map(|d| d.key).collect::<Vec<_>>().join(", ");
        return Err(t("xch.script.not_sketch_word", &[("word", F::S(clip(&kw))), ("list", F::S(list))]));
    };
    if sk.constraints.len() >= 10000 {
        return Err(t("xch.script.max_relations", &[]));
    }
    let dlabel = t(def.label_key, &[]);
    let (mut ref_part, mut val_part, mut linked) = (rest.clone(), None::<String>, false);
    if let Some(eq) = top_equals(&rest) {
        ref_part = rest.get(..eq).unwrap_or("").to_string();
        val_part = Some(js::trim(rest.get(eq.saturating_add(1)..).unwrap_or("")).to_string());
        linked = true;
    }
    let mut toks = tokens(&ref_part)?;
    let mut refs = Vec::new();
    while toks.first().is_some_and(|t0| sk.has(t0)) {
        refs.push(toks.remove(0));
    }
    if !linked && !toks.is_empty() {
        val_part = Some(toks.join(" "));
    } else if let Some(t0) = toks.first() {
        return Err(t("xch.script.not_geometry", &[("name", F::S(t0.clone()))]));
    }
    let mut c = SkCon {
        id: None,
        ty: kw.clone(),
        refs,
        line,
        value: None,
        link: None,
        sign: None,
        kind: None,
        off: None,
        at: None,
        fix_value: None,
        of: None,
    };
    if let Some(lb) = &label {
        if !geo_id_ok(lb) || sk.has(lb) {
            return Err(t("xch.script.bad_label", &[("label", F::S(clip(lb)))]));
        }
        c.id = Some(lb.clone());
        sk.ids.push(lb.clone());
    }
    if let Some(vp) = &val_part {
        if !def.dim {
            return Err(t("xch.script.no_value", &[("label", F::S(dlabel.clone()))]));
        }
        if vp.is_empty() {
            return Err(t("xch.script.value_missing", &[]));
        }
        let lit = if linked { None } else { expr::literal(vp) };
        if let Some(l) = lit {
            c.value = Some(l);
        } else if !linked {
            return Err(t("xch.script.not_number", &[("text", F::S(clip(vp)))]));
        } else {
            c.value = Some(calc(vp, &dlabel)?);
            c.link = Some(vp.clone());
        }
        let v = c.value.unwrap_or(f64::NAN);
        if !(v >= 0.0) || (kw == "angle" && v > 180.0) || ((kw == "radius" || kw == "diameter") && v <= 0.0) {
            return Err(t(
                if kw == "angle" { "xch.script.bad_angle" } else { "xch.script.bad_size" },
                &[("label", F::S(dlabel.clone())), ("value", F::S(expr::format_number(v)))],
            ));
        }
    }
    for m in &mods {
        let (mk, mv) = key_value(m);
        if mk == "side" {
            if mv == "none" {
                c.sign = Some(None);
            } else if mv == "1" || mv == "+1" || mv == "-1" {
                c.sign = Some(Some(js::str_num(&mv)));
            } else {
                return Err(t("xch.script.side", &[]));
            }
        } else if (mk == "ext" || mk == "int") && mv.is_empty() {
            c.kind = Some(Some(mk.clone()));
        } else if mk == "kind" && mv == "none" {
            c.kind = Some(None);
        } else if mk == "label" {
            c.off = Some(vec_of(&mv, 2, calc)?);
        } else if mk == "at" {
            c.at = Some(vec_of(&mv, 2, calc)?);
        } else if mk == "value" && kw == "fix" {
            let mut vals = Vec::new();
            for (i, x) in split_top_commas(&mv).iter().enumerate() {
                vals.push(calc(x, &t("xch.script.what.value", &[("n", F::from(i.saturating_add(1)))]))?);
            }
            c.fix_value = Some(vals);
        } else if mk == "of" && kw == "offsetlink" {
            c.of = Some(mv.clone());
        } else {
            let key = if kw == "fix" {
                "xch.script.not_setting_fix"
            } else if kw == "offsetlink" {
                "xch.script.not_setting_offsetlink"
            } else {
                "xch.script.not_setting_relation"
            };
            return Err(t(key, &[("word", F::S(clip(m))), ("label", F::S(js::lower(&dlabel)))]));
        }
    }
    sk.constraints.push(c);
    Ok(())
}

fn finish_sketch(feat: &mut Value, sk: SkBuild, cx: &mut Ctx, links: &mut Vec<(String, String, usize)>) -> Result<(), String> {
    let mut ids: Vec<String> = vec!["O".to_string()];
    ids.extend(sk.ids.iter().cloned());
    let mut n: f64 = 0.0;
    for id in &ids {
        let digits: String = id.chars().rev().take_while(char::is_ascii_digit).collect::<Vec<_>>().into_iter().rev().collect();
        if !digits.is_empty() {
            n = js::max(n, js::str_num(&digits));
        }
    }
    let mut used: HashSet<String> = ids.iter().cloned().collect();
    let mut cons = sk.constraints;
    for c in &mut cons {
        if c.id.is_none() {
            let mut guard = 0u32;
            loop {
                n += 1.0;
                guard = guard.saturating_add(1);
                if !used.contains(&format!("k{}", js::num_str(n))) || guard > 1_000_000 {
                    break;
                }
            }
            let id = format!("k{}", js::num_str(n));
            used.insert(id.clone());
            c.id = Some(id);
        }
    }
    let mut geo = jv::obj(vec![
        ("seq", jv::num(n + 1.0)),
        ("entities", Value::Array(sk.entities.clone())),
        ("constraints", Value::Array(Vec::new())),
    ]);
    let fid = js::str_of(feat.get("id"));
    for c in &cons {
        let ln = c.line;
        let m = sketch::match_sig(&geo, &c.ty, &c.refs)?;
        let Some((sig, mrefs)) = m else {
            let lab = sketch::label_of(&c.ty).unwrap_or_default();
            let msg = if c.refs.is_empty() {
                t("xch.script.not_apply_nothing", &[("label", F::S(lab))])
            } else {
                t("xch.script.not_apply", &[("label", F::S(lab)), ("refs", F::S(c.refs.join(", ")))])
            };
            cx.fail(Some(ln), msg).map_err(|e| e.message())?;
            continue;
        };
        let cid = c.id.clone().unwrap_or_default();
        let mut k = jv::obj(vec![("id", cid.as_str().into()), ("type", c.ty.as_str().into()), ("refs", jv::strs(&mrefs))]);
        let meas = sketch::measure(&geo, &c.ty, &mrefs, Some(&sig));
        if c.ty == "fix" {
            let v = match &c.fix_value {
                Some(fv) => jv::nums(fv),
                None => meas.value.as_ref().map_or(Value::Null, sketch::MVal::to_json),
            };
            jv::set(&mut k, "value", v);
        } else if sketch::is_dim(&c.ty) {
            let v = match c.value {
                Some(x) => jv::num(x),
                None => meas.value.as_ref().map_or(Value::Null, sketch::MVal::to_json),
            };
            jv::set(&mut k, "value", v);
        }
        let sign = match &c.sign {
            Some(None) => None,
            Some(Some(s)) => Some(*s),
            None => meas.sign,
        };
        if let Some(s) = sign {
            jv::set(&mut k, "sign", jv::num(s));
        }
        let kind = match &c.kind {
            Some(None) => None,
            Some(Some(x)) => Some(x.clone()),
            None => meas.kind.map(str::to_string),
        };
        if let Some(x) = kind {
            jv::set(&mut k, "kind", x.into());
        }
        if let Some(a) = &c.at {
            jv::set(&mut k, "at", jv::nums(a));
        }
        if let Some(o) = &c.off {
            jv::set(&mut k, "off", jv::nums(o));
        }
        if c.ty == "offsetlink" {
            match &c.of {
                Some(of) if !of.is_empty() => jv::set(&mut k, "of", of.as_str().into()),
                _ => {
                    cx.fail(Some(ln), t("xch.script.offsetlink_of", &[("form", F::from("[of <label>]"))])).map_err(|e| e.message())?;
                    continue;
                }
            }
        }
        if let Some(l) = &c.link {
            links.push((format!("{fid}.{cid}"), l.clone(), ln));
        }
        if let Some(v) = geo.get_mut("constraints").and_then(Value::as_array_mut) {
            v.push(k);
        }
    }
    jv::set(feat, "sketch", geo);
    Ok(())
}

// ================================================================= writing

/// `partToScript(part, { exact, inlineStep, comment })` → (text, files [(name, text)]).
pub fn part_to_script(pt: &Value, exact: bool, inline_step: bool, comment: bool) -> (String, Vec<(String, String)>) {
    let num = |v: f64| if exact { js::num_str(if v == 0.0 { 0.0 } else { v }) } else { expr::format_number(v) };
    let pos = |v: f64| if exact { num(v) } else { expr::format_number(if v.abs() < 1e12 { js::fixed_num(v, 9) } else { v }) };
    let vec_s = |a: &Value| jv::arr(Some(a)).iter().map(|x| pos(jv::as_num(Some(x)).unwrap_or(f64::NAN))).collect::<Vec<_>>().join(", ");
    let mut out: Vec<String> = Vec::new();
    let mut files: Vec<(String, String)> = Vec::new();
    let links: Vec<(String, String)> = jv::arr(pt.get("params").and_then(|p| p.get("links")))
        .iter()
        .map(|l| (js::str_of(l.get("target")), js::str_of(l.get("expr"))))
        .collect();
    let link = |k: &str| links.iter().rev().find(|(t0, _)| t0 == k).map(|(_, e)| e.clone());
    let feats = part::features(pt);
    let name_count = |n: &str| feats.iter().filter(|f| jv::is_str(f.get("name"), n)).count();
    let ids: HashSet<String> = feats.iter().map(|f| js::str_of(f.get("id"))).collect();
    let tok =
        |s: &str| if bare_ok(s) && !RESERVED_WORDS.contains(&s) { s.to_string() } else { jv::stringify(&Value::String(s.to_string())) };
    let fref = |id: &str| -> String {
        let Some(f) = feats.iter().find(|g| jv::is_str(g.get("id"), id)) else { return id.to_string() };
        let nm = js::str_of(f.get("name"));
        if name_count(&nm) == 1 && !(ids.contains(&nm) && nm != id) && !matches!(nm.as_str(), "body" | "end" | "first" | "named") {
            tok(&nm)
        } else {
            id.to_string()
        }
    };
    let str_val = |s: &str| {
        if word_ok(s) && !RESERVED_WORDS.contains(&s) && expr::literal(s).is_none() && !s.starts_with(['=', '-']) {
            s.to_string()
        } else {
            jv::stringify(&Value::String(s.to_string()))
        }
    };
    let generic = |v: &Value| -> String {
        match v {
            Value::Bool(true) => "yes".into(),
            Value::Bool(false) => "no".into(),
            Value::Null => "none".into(),
            Value::String(s) if jv::nonfinite(v).is_none() => str_val(s),
            x if jv::as_num(Some(x)).is_some() => num(jv::as_num(Some(x)).unwrap_or(f64::NAN)),
            x => format!("json {}", jv::stringify(x)),
        }
    };
    let keys_of = |v: &Value| -> String {
        let mut k: Vec<String> = v.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
        k.sort_by(|a, b| js::cmp16(a, b));
        k.join(",")
    };
    let plane_s = |p: &Value| -> String {
        if !jv::truthy(Some(p)) || !jv::is_obj_like(Some(p)) {
            return format!("json {}", jv::stringify(p));
        }
        let keys = keys_of(p);
        let kind = jv::s(p, "kind");
        let face = jv::s(p, "face");
        if kind == "base" && keys == "kind,name" {
            return js::str_of(p.get("name"));
        }
        if kind == "face" && (face == "start" || face == "end") && keys == "face,feature,kind" {
            return format!("face {} {face}", fref(&js::str_of(p.get("feature"))));
        }
        if kind == "face"
            && face == "side"
            && keys == "entity,face,feature,kind"
            && p.get("entity").and_then(Value::as_str).is_some_and(entity_word)
        {
            return format!("face {} side {}", fref(&js::str_of(p.get("feature"))), js::str_of(p.get("entity")));
        }
        if kind == "face"
            && face == "named"
            && keys == "face,feature,kind,name"
            && p.get("name").and_then(Value::as_str).is_some_and(|n| !n.is_empty() && !n.chars().any(is_ws))
        {
            return format!("face named {}", js::str_of(p.get("name")));
        }
        if kind == "ref" && keys == "feature,kind" {
            return format!("plane {}", fref(&js::str_of(p.get("feature"))));
        }
        if kind == "fixed" && keys == "kind,n,o" {
            return format!("fixed {} normal {}", vec_s(p.get("o").unwrap_or(&Value::Null)), vec_s(p.get("n").unwrap_or(&Value::Null)));
        }
        format!("json {}", jv::stringify(p))
    };
    let dir_s = |d: &Value| -> String {
        if !jv::truthy(Some(d)) || !jv::is_obj_like(Some(d)) {
            return format!("json {}", jv::stringify(d));
        }
        let keys = keys_of(d);
        let kind = jv::s(d, "kind");
        if kind == "world" && keys == "axis,kind" {
            return js::str_of(d.get("axis"));
        }
        if kind == "line" && keys == "entity,kind,sketch" && d.get("entity").and_then(Value::as_str).is_some_and(entity_word) {
            return format!("line {} {}", fref(&js::str_of(d.get("sketch"))), js::str_of(d.get("entity")));
        }
        if kind == "axis" && keys == "feature,kind" {
            return format!("axis-of {}", fref(&js::str_of(d.get("feature"))));
        }
        if kind == "ref" && keys == "feature,kind" {
            return format!("ref {}", fref(&js::str_of(d.get("feature"))));
        }
        if kind == "fixed" && keys == "d,kind,o" {
            return format!("fixed {} dir {}", vec_s(d.get("o").unwrap_or(&Value::Null)), vec_s(d.get("d").unwrap_or(&Value::Null)));
        }
        format!("json {}", jv::stringify(d))
    };
    let plain = |n: &str| !n.is_empty() && !n.chars().any(is_ws) && !n.contains('[') && !n.contains('"');
    let is_edge_ref = |e: &Value| {
        jv::truthy(Some(e))
            && jv::as_arr(e.get("faces")).is_some_and(|f| f.len() == 2 && f.iter().all(|n| n.as_str().is_some_and(plain)))
            && e.as_object()
                .is_some_and(|o| o.iter().all(|(k, v)| k == "faces" || (k == "at" && jv::as_arr(Some(v)).is_some_and(|a| a.len() == 3))))
    };
    let is_face_ref = |r: &Value| {
        jv::truthy(Some(r))
            && r.get("name").and_then(Value::as_str).is_some_and(plain)
            && r.as_object()
                .is_some_and(|o| o.iter().all(|(k, v)| k == "name" || (k == "at" && jv::as_arr(Some(v)).is_some_and(|a| a.len() == 3))))
    };
    let pname = js::str_of(pt.get("name"));
    if comment {
        out.push(format!("# {pname} — a PartCraft script. A line at the left edge starts a statement; indented lines are its settings."));
    }
    out.push(format!("pcscript {}", js::num_str(SCRIPT_VERSION)));
    out.push(format!("part {}", jv::stringify(pt.get("name").unwrap_or(&Value::Null))));
    out.push(format!("  resolution {}", js::string_of(pt.get("resolution"))));
    let pvars = jv::arr(pt.get("params").and_then(|p| p.get("vars")));
    if !pvars.is_empty() {
        out.push(String::new());
        for v in pvars {
            out.push(format!("param {} = {}", js::string_of(v.get("name")), js::string_of(v.get("expr"))));
            if jv::truthy(v.get("note")) {
                let note = js::str_of(v.get("note"));
                let quote =
                    note.contains(['#', '"', '\\']) || note.chars().next().is_some_and(is_ws) || note.chars().last().is_some_and(is_ws);
                out.push(format!("  note {}", if quote { jv::stringify(&Value::String(note)) } else { note }));
            }
        }
    }
    let mut max_n: f64 = 0.0;
    for f in feats {
        out.push(String::new());
        let fty = js::str_of(f.get("type"));
        let fid = js::str_of(f.get("id"));
        out.push(format!("{fty} {}", tok(&js::str_of(f.get("name")))));
        let want = format!("f{}", js::num_str(max_n + 1.0));
        if fid != want {
            out.push(format!("  id {fid}"));
        }
        max_n = js::max(max_n, js::str_num(fid.get(1..).unwrap_or("")));
        if jv::truthy(f.get("suppressed")) {
            out.push("  suppressed".into());
        }
        if fty == "sketch" {
            write_sketch(f, &mut out, &plane_s, &vec_s, &num, &pos, &link);
            continue;
        }
        if let Some(o) = f.as_object() {
            for (k, v) in o {
                if matches!(k.as_str(), "id" | "type" | "name" | "suppressed") {
                    continue;
                }
                let key = format!("{fid}.{k}");
                if let Some(l) = link(&key) {
                    out.push(format!("  {k} = {l}"));
                    continue;
                }
                if k == "sketch"
                    && let Some(sv) = v.as_str()
                {
                    out.push(format!("  sketch {}", fref(sv)));
                    continue;
                }
                if k == "seeds" && matches!(fty.as_str(), "lpattern" | "cpattern" | "mirror") {
                    let s = if jv::is_str(Some(v), "body") {
                        "body".to_string()
                    } else if jv::as_arr(Some(v)).is_some_and(|a| !a.is_empty()) {
                        jv::arr(Some(v)).iter().map(|x| fref(&js::str_of(Some(x)))).collect::<Vec<_>>().join(", ")
                    } else {
                        generic(v)
                    };
                    out.push(format!("  seeds {s}"));
                    continue;
                }
                if (k == "dir" && fty == "lpattern") || (k == "axis" && fty == "cpattern") {
                    out.push(format!("  {k} {}", dir_s(v)));
                    continue;
                }
                if k == "axis" && fty == "revolve" {
                    let s = if v.is_null() {
                        "first".to_string()
                    } else if let Some(a) = v.as_str().filter(|a| entity_word(a) && !matches!(*a, "x" | "y" | "z" | "first")) {
                        a.to_string()
                    } else {
                        dir_s(v)
                    };
                    out.push(format!("  axis {s}"));
                    continue;
                }
                if k == "plane" && fty == "mirror" {
                    out.push(format!("  plane {}", plane_s(v)));
                    continue;
                }
                if k == "edges"
                    && let Some(a) = jv::as_arr(Some(v)).filter(|a| !a.is_empty() && a.iter().all(is_edge_ref))
                {
                    for e in a {
                        let fs = jv::arr(e.get("faces"));
                        let at = e.get("at").filter(|a| jv::truthy(Some(a))).map_or_else(String::new, |a| format!(" [at {}]", vec_s(a)));
                        out.push(format!("  edge {} {}{at}", js::str_of(fs.first()), js::str_of(fs.get(1))));
                    }
                    continue;
                }
                if k == "faces"
                    && let Some(a) = jv::as_arr(Some(v)).filter(|a| !a.is_empty() && a.iter().all(is_face_ref))
                {
                    for r in a {
                        let at = r.get("at").filter(|a| jv::truthy(Some(a))).map_or_else(String::new, |a| format!(" [at {}]", vec_s(a)));
                        out.push(format!("  face {}{at}", js::str_of(r.get("name"))));
                    }
                    continue;
                }
                if fty == "import" && k == "step" {
                    if inline_step {
                        out.push(format!("  step {}", jv::stringify(v)));
                        continue;
                    }
                    let src = if jv::truthy(f.get("file")) { js::str_of(f.get("file")) } else { js::str_of(f.get("name")) };
                    let lower = src.to_lowercase();
                    let stem = if lower.ends_with(".step") {
                        src.get(..src.len().saturating_sub(5)).unwrap_or("").to_string()
                    } else if lower.ends_with(".stp") {
                        src.get(..src.len().saturating_sub(4)).unwrap_or("").to_string()
                    } else {
                        src.clone()
                    };
                    let mut base = String::new();
                    let mut in_run = false;
                    for c in stem.chars() {
                        if c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '.' | '-') {
                            base.push(c);
                            in_run = false;
                        } else if !in_run {
                            base.push('_');
                            in_run = true;
                        }
                    }
                    let mut base = js::slice16(&base, 80);
                    if base.is_empty() {
                        base = fid.clone();
                    }
                    let mut name = format!("{base}.step");
                    let mut i = 2u32;
                    while files.iter().any(|(n, _)| *n == name) {
                        name = format!("{base} ({i}).step");
                        i = i.saturating_add(1);
                    }
                    files.push((name.clone(), js::str_of(Some(v))));
                    out.push(format!("  step-file {}", jv::stringify(&Value::String(name))));
                    continue;
                }
                if fty == "import" && k == "file" {
                    out.push(format!("  file {}", jv::stringify(v)));
                    continue;
                }
                if v.is_boolean()
                    && params_part::fields_of(&fty).iter().all(|s| s.field != k)
                    && matches!(k.as_str(), "reverse" | "through" | "outward")
                {
                    if jv::is_true(Some(v)) {
                        out.push(format!("  {k}"));
                    }
                    continue;
                }
                out.push(format!("  {k} {}", generic(v)));
            }
        }
    }
    let n = js::f(feats.len());
    if let Some(r) = jv::as_num(pt.get("rollback"))
        && r < n
    {
        let id = js::idx(r).and_then(|i| feats.get(i)).map(|f| js::str_of(f.get("id"))).unwrap_or_default();
        out.push(String::new());
        out.push(format!("rollback {}", fref(&id)));
    }
    if jv::truthy(pt.get("material")) {
        out.push(String::new());
        out.push(format!("material json {}", jv::stringify(pt.get("material").unwrap_or(&Value::Null))));
    }
    if pt.get("configs").and_then(|c| c.get("rows")).and_then(Value::as_array).is_some_and(|r| !r.is_empty()) {
        out.push(String::new());
        out.push(format!("configs json {}", jv::stringify(pt.get("configs").unwrap_or(&Value::Null))));
    }
    if jv::truthy(pt.get("drawing")) {
        out.push(String::new());
        out.push(format!("drawing json {}", jv::stringify(pt.get("drawing").unwrap_or(&Value::Null))));
    }
    if let Some(st) = pt.get("studies").filter(|s| jv::truthy(Some(s))).and_then(Value::as_object) {
        for (k, v) in st {
            out.push(String::new());
            out.push(format!("study {k} json {}", jv::stringify(v)));
        }
    }
    (format!("{}\n", out.join("\n")), files)
}

#[allow(clippy::too_many_arguments)]
fn write_sketch(
    f: &Value,
    out: &mut Vec<String>,
    plane_s: &dyn Fn(&Value) -> String,
    vec_s: &dyn Fn(&Value) -> String,
    num: &dyn Fn(f64) -> String,
    pos: &dyn Fn(f64) -> String,
    link: &dyn Fn(&str) -> Option<String>,
) {
    out.push(format!("  on {}", plane_s(f.get("plane").unwrap_or(&Value::Null))));
    let sk = f.get("sketch").cloned().unwrap_or(Value::Null);
    let fid = js::str_of(f.get("id"));
    let mut sk_w = sk.clone();
    if let Some(ents) = sk_w.get_mut("entities").and_then(Value::as_array_mut) {
        for e in ents.iter_mut() {
            if jv::s(e, "type") == "point" {
                let (x, y) = (jv::n(e, "x"), jv::n(e, "y"));
                jv::set(e, "x", jv::num(js::str_num(&pos(x))));
                jv::set(e, "y", jv::num(js::str_num(&pos(y))));
            }
            if jv::s(e, "type") == "circle" {
                let r = jv::n(e, "r");
                jv::set(e, "r", jv::num(js::str_num(&pos(r))));
            }
        }
    }
    let labelled: HashSet<String> =
        sketch::constraints(&sk).iter().filter(|c| jv::s(c, "type") == "offsetlink").map(|c| js::string_of(c.get("of"))).collect();
    for e in sketch::entities(&sk) {
        let mut mods: Vec<String> = Vec::new();
        if jv::truthy(e.get("construction")) {
            mods.push("construction".into());
        }
        let id = js::string_of(e.get("id"));
        let g = |k: &str| js::string_of(e.get(k));
        let body = match jv::s(e, "type") {
            "point" => format!("point {id} {}, {}", pos(jv::n(e, "x")), pos(jv::n(e, "y"))),
            "line" => format!("line {id} {} {}", g("p1"), g("p2")),
            "circle" => format!("circle {id} {} {}", g("c"), pos(jv::n(e, "r"))),
            "arc" => format!("arc {id} {} {} {}", g("c"), g("s"), g("e")),
            "spline" => {
                let pts: Vec<String> = jv::arr(e.get("pts")).iter().map(|v| js::string_of(Some(v))).collect();
                if jv::truthy(e.get("closed")) {
                    mods.insert(0, "closed".into());
                }
                for k in ["a0", "a1"] {
                    if let Some(a) = jv::finite(e.get(k)) {
                        mods.push(format!("{k} {}", num(a)));
                    }
                }
                if jv::truthy(e.get("knots")) {
                    mods.push(format!(
                        "knots {}",
                        jv::arr(e.get("knots")).iter().map(|x| num(jv::as_num(Some(x)).unwrap_or(f64::NAN))).collect::<Vec<_>>().join(", ")
                    ));
                }
                format!("spline {id} {} {}", g("kind"), pts.join(" "))
            }
            other => format!("# unknown geometry {}", if e.get("type").is_some() { other.to_string() } else { "undefined".into() }),
        };
        out.push(format!("  {body}{}", if mods.is_empty() { String::new() } else { format!(" [{}]", mods.join("; ")) }));
    }
    for c in sketch::constraints(&sk) {
        let ty = jv::s(c, "type");
        let cid = js::string_of(c.get("id"));
        let refs: Vec<String> = jv::arr(c.get("refs")).iter().map(|v| js::str_of(Some(v))).collect();
        let mut body = format!("{}{ty} {}", if labelled.contains(&cid) { format!("{cid}: ") } else { String::new() }, refs.join(" "));
        let key = format!("{fid}.{cid}");
        if sketch::is_dim(ty) {
            match link(&key) {
                Some(l) => body.push_str(&format!(" = {l}")),
                None => body.push_str(&format!(" {}", num(jv::n(c, "value")))),
            }
        }
        let mut mods: Vec<String> = Vec::new();
        let meas = sketch::measure(&sk_w, ty, &refs, None);
        let sign = jv::as_num(c.get("sign"));
        if sign != meas.sign {
            mods.push(match c.get("sign") {
                None => "side none".into(),
                Some(v) => format!("side {}", js::string_of(Some(v))),
            });
        }
        let kind = c.get("kind").map(|k| js::string_of(Some(k)));
        if kind.as_deref() != meas.kind {
            mods.push(kind.unwrap_or_else(|| "kind none".into()));
        }
        if ty == "fix"
            && let Some(cv) = jv::as_arr(c.get("value"))
        {
            let mv: Option<Vec<f64>> = match &meas.value {
                Some(sketch::MVal::A(a)) => Some(a.clone()),
                _ => None,
            };
            let same = mv.as_ref().is_some_and(|m| {
                m.len() == cv.len()
                    && m.iter()
                        .zip(cv.iter())
                        .all(|(x, y)| (x - jv::as_num(Some(y)).unwrap_or(f64::NAN)).abs() <= 1e-9 * js::max(1.0, x.abs()))
            });
            if !same {
                mods.push(format!(
                    "value {}",
                    cv.iter().map(|x| pos(jv::as_num(Some(x)).unwrap_or(f64::NAN))).collect::<Vec<_>>().join(", ")
                ));
            }
        }
        if let Some(a) = c.get("at").filter(|a| jv::truthy(Some(a))) {
            mods.push(format!("at {}", vec_s(a)));
        }
        if let Some(o) = c.get("off").filter(|a| jv::truthy(Some(a))) {
            mods.push(format!("label {}", vec_s(o)));
        }
        if ty == "offsetlink" {
            mods.push(format!("of {}", js::string_of(c.get("of"))));
        }
        out.push(format!("  {body}{}", if mods.is_empty() { String::new() } else { format!(" [{}]", mods.join("; ")) }));
    }
}

/// `looksLikeScript(text)`.
pub fn looks_like_script(text: &str) -> bool {
    let s = text.strip_prefix('\u{feff}').unwrap_or(text);
    if js::trim_start(s).starts_with('{') {
        return false;
    }
    for line in s.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).take(50) {
        let b = js::trim(strip_comment(line));
        if b.is_empty() {
            continue;
        }
        let kw = b.split(is_ws).next().unwrap_or("");
        return TOP.contains(&kw) || part::known_type(kw);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLATE: &str = "pcscript 1\npart \"Plate\"\nparam w = 120\nsketch S\n  on top\n  point a 0, 0\n  point b 120, 0\n  point c 120, 60\n  point d 0, 60\n  line l1 a b\n  line l2 b c\n  line l3 c d\n  line l4 d a\n  coincident a O\n  horizontal l1\n  horizontal l3\n  vertical l2\n  vertical l4\n  distance l1 = w\n  distance l4 60\nextrude E\n  sketch S\n  depth = w / 15\n";

    #[test]
    fn reads_writes_and_reads_again() {
        let r = script_to_part(PLATE, None).expect("reads");
        assert!(r.warnings.is_empty());
        let links = jv::arr(r.part.get("params").and_then(|p| p.get("links")));
        assert_eq!(links.len(), 2);
        let (text, files) = part_to_script(&r.part, false, false, true);
        assert!(files.is_empty() && text.contains("  depth = w / 15"));
        let again = script_to_part(&text, None).expect("reads again");
        assert_eq!(part_to_script(&again.part, false, false, true).0, text);
        assert!(looks_like_script(&text) && !looks_like_script("{\"format\":\"partcraft\"}"));
    }

    #[test]
    fn errors_carry_lines_and_hostile_text_is_refused() {
        let e = script_to_part("extrude E1\n  sketch Nope\n", None).expect_err("refused");
        assert_eq!(e.errors.first().and_then(|p| p.line), Some(2));
        let e = script_to_part("extrude E\n  op json {\"a\":\n", None).expect_err("refused");
        assert!(e.message().contains("Unexpected end of JSON input"));
        for bad in [
            "\"",
            "  [",
            "sketch S\n  x: [",
            "param = =",
            "rollback \"",
            "\u{feff}\r\r\n",
            "pcscript 1e999",
            "sketch S\n  point p 1, 2 [",
            "part \"\\ud800\"",
        ] {
            let _ = script_to_part(bad, None);
        }
        let many: String = (0..40).map(|i| format!("bogus{i}\n")).collect();
        let e = script_to_part(&many, None).expect_err("refused");
        assert!(e.errors.len() <= LIMIT_ERRORS + 1);
    }
}
