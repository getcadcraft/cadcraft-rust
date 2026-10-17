//! desckeys.js — description keys and line codes (0.4 V16).

use crate::js::{J, arr, locale_compare_num, slice16, str_to_number, strip_controls, trim, upper};
use crate::points::{Pt, split_ws};
use serde_json::Value;
use std::collections::HashMap;

pub const USES: [&str; 3] = ["points", "breakline", "off"];

/// A description key.
#[derive(Clone, Debug, PartialEq)]
pub struct DescKey {
    pub code: String,
    pub group: String,
    pub use_: String,
    pub label: String,
}
impl J for DescKey {
    fn j(&self) -> Value {
        crate::jo! { "code" => self.code, "group" => self.group, "use" => self.use_, "label" => self.label }
    }
}
/// Line codes.
#[derive(Clone, Debug, PartialEq)]
pub struct LineCodes {
    pub on: bool,
    pub begin: Vec<String>,
    pub end: Vec<String>,
    pub close: Vec<String>,
}
impl J for LineCodes {
    fn j(&self) -> Value {
        crate::jo! { "on" => self.on, "begin" => self.begin, "end" => self.end, "close" => self.close }
    }
}
/// `DEFAULT_LINE_CODES`.
pub fn default_line_codes() -> LineCodes {
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect();
    LineCodes { on: true, begin: v(&["B", "BEG", "START"]), end: v(&["E", "END"]), close: v(&["C", "CLO", "CLOSE"]) }
}

/// `tok(s, max)`: controls removed, trimmed, upper case, at most max units.
fn tok(s: Option<&Value>, max: usize) -> String {
    slice16(&upper(trim(&strip_controls(&crate::js::string_or_empty(s), ""))), max)
}
fn tok_str(s: &str, max: usize) -> String {
    slice16(&upper(trim(&strip_controls(s, ""))), max)
}

/// `cleanDescKeys(list)` (untrusted).
pub fn clean_desc_keys(list: Option<&Value>) -> Vec<DescKey> {
    let Some(list) = list.and_then(Value::as_array) else { return Vec::new() };
    let mut out = Vec::new();
    for k in list.iter().take(500) {
        if !(k.is_object() || k.is_array()) {
            continue;
        }
        let code: String = tok(k.get("code"), 40).chars().filter(|c| !crate::js::is_ws(*c)).collect();
        if code.is_empty() || !code.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || "*?_.-/#".contains(c)) {
            continue;
        }
        let group: String = tok(k.get("group"), 40).chars().filter(|c| !crate::js::is_ws(*c)).collect();
        let use_ = k.get("use").and_then(Value::as_str).filter(|u| USES.contains(u)).unwrap_or("").to_string();
        let label = slice16(trim(&strip_controls(&crate::js::string_or_empty(k.get("label")), " ")), 120);
        out.push(DescKey { code, group, use_, label });
    }
    out
}
/// Description keys as given (trusted).
pub fn read_desc_keys(list: Option<&Value>) -> Vec<DescKey> {
    arr(list)
        .iter()
        .map(|k| DescKey {
            code: crate::js::string_or_empty(k.get("code")),
            group: crate::js::string_or_empty(k.get("group")),
            use_: crate::js::string_or_empty(k.get("use")),
            label: crate::js::string_or_empty(k.get("label")),
        })
        .collect()
}

/// `cleanLineCodes(o)` (untrusted).
pub fn clean_line_codes(o: Option<&Value>) -> LineCodes {
    let d = default_line_codes();
    let Some(o) = o.filter(|o| o.is_object() || o.is_array()) else { return d };
    let list = |v: Option<&Value>, dv: &[String]| -> Vec<String> {
        let raw: Vec<String> = match v {
            Some(Value::Array(a)) => a.iter().map(|x| tok(Some(x), 12)).collect(),
            _ => {
                let s = crate::js::string_or_empty(v);
                s.split(|c: char| crate::js::is_ws(c) || c == ',' || c == ';').map(|x| tok_str(x, 12)).collect()
            }
        };
        let mut a: Vec<String> = Vec::new();
        for x in raw {
            if !x.is_empty() && x.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || "_.-".contains(c)) && !a.contains(&x) {
                a.push(x);
            }
        }
        if a.is_empty() {
            dv.to_vec()
        } else {
            a.truncate(10);
            a
        }
    };
    let begin = list(o.get("begin"), &d.begin);
    let mut end = list(o.get("end"), &d.end);
    let mut close = list(o.get("close"), &d.close);
    end.retain(|x| !begin.contains(x));
    close.retain(|x| !begin.contains(x) && !end.contains(x));
    LineCodes { on: o.get("on") != Some(&Value::Bool(false)), begin, end, close }
}
/// Line codes as given (trusted).
pub fn read_line_codes(o: &Value) -> LineCodes {
    let l = |k: &str| arr(o.get(k)).iter().map(|x| crate::js::string_or_empty(Some(x))).collect();
    LineCodes { on: o.get("on") != Some(&Value::Bool(false)), begin: l("begin"), end: l("end"), close: l("close") }
}

/// `parseDesc(desc, lc)` → (code, ctrl, rest).
pub fn parse_desc(desc: &str, lc: &LineCodes) -> (String, Option<&'static str>, String) {
    let words: Vec<String> = split_ws(desc).into_iter().filter(|w| !w.is_empty()).collect();
    let code = words.first().map(|w| upper(w)).unwrap_or_default();
    let mut ctrl = None;
    let mut at: i64 = -1;
    if lc.on && words.len() > 1 {
        let w = upper(words.get(1).map_or("", String::as_str));
        if lc.begin.contains(&w) {
            ctrl = Some("begin");
        } else if lc.end.contains(&w) {
            ctrl = Some("end");
        } else if lc.close.contains(&w) {
            ctrl = Some("close");
        }
        if ctrl.is_some() {
            at = 1;
        }
    }
    let rest: Vec<&str> =
        words.iter().enumerate().filter(|(i, _)| *i > 0 && i64::try_from(*i).unwrap_or(-1) != at).map(|(_, w)| w.as_str()).collect();
    (code, ctrl, rest.join(" "))
}

/// Case-blind glob match (`*` any run, `?` one character; everything else literal) of the whole code.
fn glob(pat: &[char], s: &[char]) -> bool {
    let (mut p, mut i) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    let eq = |a: char, b: char| a == b || a.to_uppercase().eq(b.to_uppercase());
    while i < s.len() {
        match pat.get(p) {
            Some('*') => {
                star = Some(p);
                mark = i;
                p = p.saturating_add(1);
            }
            Some(&c) if c == '?' || s.get(i).is_some_and(|&x| eq(c, x)) => {
                p = p.saturating_add(1);
                i = i.saturating_add(1);
            }
            _ => match star {
                Some(sp) => {
                    p = sp.saturating_add(1);
                    mark = mark.saturating_add(1);
                    i = mark;
                }
                None => return false,
            },
        }
    }
    while pat.get(p) == Some(&'*') {
        p = p.saturating_add(1);
    }
    p == pat.len()
}

/// `compileKeys(keys)` → index of the first key matching the code.
pub fn compile_keys(keys: &[DescKey]) -> impl Fn(&str) -> Option<usize> + '_ {
    let pats: Vec<Vec<char>> = keys.iter().map(|k| k.code.chars().collect()).collect();
    move |code: &str| {
        let s: Vec<char> = code.chars().collect();
        pats.iter().position(|p| glob(p, &s))
    }
}

/// `grouper(keys)` → point → group name (cached per code).
pub fn grouper(keys: &[DescKey]) -> impl Fn(&Pt) -> String + '_ {
    let matcher = compile_keys(keys);
    let cache: std::cell::RefCell<HashMap<String, String>> = std::cell::RefCell::new(HashMap::new());
    move |p: &Pt| {
        let code = upper(split_ws(&p.d).into_iter().next().as_deref().unwrap_or(""));
        if code.is_empty() {
            return "(no description)".to_string();
        }
        if let Some(g) = cache.borrow().get(&code) {
            return g.clone();
        }
        let g = match matcher(&code).and_then(|i| keys.get(i)) {
            Some(k) if !k.group.is_empty() => k.group.clone(),
            _ => code.clone(),
        };
        cache.borrow_mut().insert(code, g.clone());
        g
    }
}

fn by_number(a: &Pt, b: &Pt) -> f64 {
    let na = str_to_number(&a.n);
    let nb = str_to_number(&b.n);
    if na.is_finite() && nb.is_finite() {
        return na - nb;
    }
    locale_compare_num(&a.n, &b.n)
}

/// A figure (line made by codes).
#[derive(Clone, Debug)]
pub struct Figure {
    pub code: String,
    pub group: String,
    pub pts: Vec<Pt>,
    pub closed: bool,
    pub coded: bool,
}
impl J for Figure {
    fn j(&self) -> Value {
        crate::jo! { "code" => self.code, "group" => self.group, "pts" => self.pts, "closed" => self.closed, "coded" => self.coded }
    }
}

/// `figures(points, { keys, lineCodes, only })`.
pub fn figures(points: &[Pt], keys: &[DescKey], line_codes: &LineCodes, only: Option<&[String]>) -> Vec<Figure> {
    let group_of = grouper(keys);
    let mut order: Vec<String> = Vec::new();
    let mut by_code: HashMap<String, (String, Vec<(Pt, Option<&'static str>)>)> = HashMap::new();
    for p in points {
        let (code, ctrl, _) = parse_desc(&p.d, line_codes);
        if code.is_empty() {
            continue;
        }
        let g = group_of(p);
        if only.is_some_and(|o| !o.contains(&g)) {
            continue;
        }
        match by_code.get_mut(&code) {
            Some(e) => e.1.push((p.clone(), ctrl)),
            None => {
                order.push(code.clone());
                by_code.insert(code, (g, vec![(p.clone(), ctrl)]));
            }
        }
    }
    let mut out = Vec::new();
    for code in order {
        let Some((group, mut list)) = by_code.remove(&code) else { continue };
        crate::js::sort_by(&mut list, |a, b| by_number(&a.0, &b.0));
        let coded = list.iter().any(|x| x.1.is_some());
        let mut cur: Option<Vec<Pt>> = None;
        let flush = |cur: &mut Option<Vec<Pt>>, closed: bool, out: &mut Vec<Figure>| {
            if let Some(c) = cur.take()
                && c.len() >= 2
            {
                let cl = closed && c.len() >= 3;
                out.push(Figure { code: code.clone(), group: group.clone(), pts: c, closed: cl, coded });
            }
        };
        for (p, ctrl) in list {
            if ctrl == Some("begin") {
                flush(&mut cur, false, &mut out);
                cur = Some(vec![p]);
                continue;
            }
            cur.get_or_insert_with(Vec::new).push(p);
            if ctrl == Some("end") {
                flush(&mut cur, false, &mut out);
            } else if ctrl == Some("close") {
                flush(&mut cur, true, &mut out);
            }
        }
        flush(&mut cur, false, &mut out);
    }
    out
}

/// `keyStats(points, keys)` → how many points each key catches.
pub fn key_stats(points: &[Pt], keys: &[DescKey]) -> Vec<usize> {
    let matcher = compile_keys(keys);
    let mut n = vec![0usize; keys.len()];
    for p in points {
        let code = split_ws(&p.d).into_iter().next().unwrap_or_default();
        if code.is_empty() {
            continue;
        }
        if let Some(i) = matcher(&upper(&code))
            && let Some(c) = n.get_mut(i)
        {
            *c = c.saturating_add(1);
        }
    }
    n
}
