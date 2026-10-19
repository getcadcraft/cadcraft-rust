//! QSELECT and SELECTSIMILAR (qselect.js) and FIND (findtext.js), with their commands (commands.js).
//!
//! The window's Quick select and Find dialogs (cadcraft.js `quickSelectDialog` / `findDialog`) call the same
//! [`quick_select`] / [`find_matches`] / [`apply_matches`]; the commands ask on the command line when the host has no
//! dialog (as the browser's command line does under test).

use cc_cad::Doc;
use cc_cad::entity::{Color, Entity, Geo};
use cc_cad::js;
use cc_cad_geom::edit::{curve_length, entity_area};
use cc_cad_geom::geom::js_keys;
use cc_cad_geom::units::parse_length;
use serde_json::Value;

use crate::engine::{Ctx, IdSet, Opts, R, Reply, Sel};
use crate::input::{Def, parse_float};

/// The object types QSELECT offers, in order (`QS_TYPES`).
pub const QS_TYPES: [&str; 10] = ["line", "poly", "circle", "arc", "ellipse", "text", "point", "insert", "hatch", "dim"];
/// The properties (`QS_PROPS` keys, in order).
pub const QS_PROPS: [&str; 9] = ["layer", "color", "height", "contents", "block", "radius", "length", "area", "pattern"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropKind {
    Text,
    Number,
    Color,
    Choice,
}

/// `QS_PROPS[k]`: the types it applies to (None = every type) and its kind.
pub fn prop_def(k: &str) -> Option<(Option<&'static [&'static str]>, PropKind)> {
    Some(match k {
        "layer" => (None, PropKind::Choice),
        "color" => (None, PropKind::Color),
        "height" => (Some(&["text"]), PropKind::Number),
        "contents" => (Some(&["text"]), PropKind::Text),
        "block" => (Some(&["insert"]), PropKind::Choice),
        "radius" => (Some(&["circle", "arc"]), PropKind::Number),
        "length" => (Some(&["line", "arc", "poly"]), PropKind::Number),
        "area" => (Some(&["circle", "poly", "ellipse", "hatch"]), PropKind::Number),
        "pattern" => (Some(&["hatch"]), PropKind::Choice),
        _ => return None,
    })
}

/// A property's value (or a rule's value).
#[derive(Clone, Debug, PartialEq)]
pub enum QsVal {
    Null,
    Num(f64),
    Str(String),
}
impl QsVal {
    /// `String(v)`
    pub fn js_string(&self) -> String {
        match self {
            QsVal::Null => "null".into(),
            QsVal::Num(n) => js::num_to_string(*n),
            QsVal::Str(s) => s.clone(),
        }
    }
}

fn color_val(c: &Option<Color>) -> QsVal {
    match c {
        None | Some(Color::Null) => QsVal::Null,
        Some(Color::Num(n)) => QsVal::Num(*n),
        Some(Color::Str(s)) => QsVal::Str(s.clone()),
    }
}
fn finite(v: Option<f64>) -> QsVal {
    v.filter(|x| x.is_finite()).map_or(QsVal::Null, QsVal::Num)
}
/// `QS_PROPS.pattern.get(e)`
fn pattern_of(e: &Entity) -> String {
    if let Geo::Hatch(h) = &e.geo
        && h.solid == Some(true)
    {
        return "SOLID".into();
    }
    if js::truthy(e.extra.get("solid")) {
        return "SOLID".into();
    }
    e.extra.get("pattern").filter(|p| js::truthy(Some(p))).map(js::to_js_string).unwrap_or_default().to_uppercase()
}

/// `QS_PROPS[k].get(e)`
pub fn prop_value(e: &Entity, k: &str) -> QsVal {
    match k {
        "layer" => QsVal::Str(e.layer.clone().unwrap_or_else(|| "0".into())),
        "color" => color_val(&e.color),
        "height" => finite(if let Geo::Text(t) = &e.geo { Some(t.h) } else { e.num("h") }),
        "contents" => QsVal::Str(match &e.geo {
            Geo::Text(t) => t.s.clone(),
            Geo::Mleader(m) => m.s.clone().unwrap_or_default(),
            _ => e.extra.get("s").filter(|v| !v.is_null()).map(js::to_js_string).unwrap_or_default(),
        }),
        "block" => match &e.geo {
            Geo::Insert(i) => QsVal::Str(i.block.clone()),
            _ => e.extra.get("block").and_then(Value::as_str).map_or(QsVal::Null, |s| QsVal::Str(s.into())),
        },
        "radius" => finite(match &e.geo {
            Geo::Circle(c) => Some(c.r),
            Geo::Arc(a) => Some(a.r),
            _ => e.num("r"),
        }),
        "length" => finite(curve_length(e)),
        "area" => finite(entity_area(e).map(|a| a.0)),
        "pattern" => QsVal::Str(pattern_of(e)),
        _ => QsVal::Null,
    }
}

/// `propsFor(type)`
pub fn props_for(typ: &str) -> Vec<&'static str> {
    QS_PROPS.iter().copied().filter(|k| prop_def(k).is_some_and(|(t, _)| t.is_none_or(|t| typ != "any" && t.contains(&typ)))).collect()
}

fn same_color(a: &QsVal, b: &QsVal) -> bool {
    match (a, b) {
        (QsVal::Null, QsVal::Null) => true,
        (QsVal::Num(x), QsVal::Num(y)) => x == y,
        (QsVal::Str(x), QsVal::Str(y)) => x == y || x.to_lowercase() == y.to_lowercase(),
        _ => false,
    }
}

/// A rule (spec): type ('any' | an entity type), property, operator ('=' '<>' '>' '<' '*' 'all'), value, exclude,
/// within (the current selection when "Apply to" is the selection), append.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub typ: String,
    pub prop: String,
    pub op: String,
    pub value: QsVal,
    pub exclude: bool,
    pub within: Option<Vec<u64>>,
    pub append: bool,
}

/// ICU-like `localeCompare(…, { numeric: true })`: digit runs by value, letters before case, case last.
pub fn locale_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    locale_cmp_with(a, b, true)
}

/// ICU-like `localeCompare` without the numeric option (digits compare one by one).
pub fn locale_cmp_plain(a: &str, b: &str) -> std::cmp::Ordering {
    locale_cmp_with(a, b, false)
}

fn locale_cmp_with(a: &str, b: &str, numeric: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    fn class(c: char) -> u8 {
        if c.is_whitespace() || c.is_ascii_punctuation() {
            0
        } else if c.is_ascii_digit() {
            1
        } else if c.is_alphabetic() {
            2
        } else {
            3
        }
    }
    let (ac, bc): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let (mut i, mut j) = (0usize, 0usize);
    let mut tertiary = Ordering::Equal;
    while i < ac.len() && j < bc.len() {
        let (x, y) = (ac.get(i).copied().unwrap_or(' '), bc.get(j).copied().unwrap_or(' '));
        if numeric && x.is_ascii_digit() && y.is_ascii_digit() {
            let si = i;
            while ac.get(i).is_some_and(char::is_ascii_digit) {
                i = i.saturating_add(1);
            }
            let sj = j;
            while bc.get(j).is_some_and(char::is_ascii_digit) {
                j = j.saturating_add(1);
            }
            let na: String = ac.get(si..i).unwrap_or(&[]).iter().collect::<String>().trim_start_matches('0').to_string();
            let nb: String = bc.get(sj..j).unwrap_or(&[]).iter().collect::<String>().trim_start_matches('0').to_string();
            let o = na.len().cmp(&nb.len()).then_with(|| na.cmp(&nb));
            if o != Ordering::Equal {
                return o;
            }
            continue;
        }
        let o = class(x).cmp(&class(y));
        if o != Ordering::Equal {
            return o;
        }
        let (lx, ly) = (x.to_lowercase().next().unwrap_or(x), y.to_lowercase().next().unwrap_or(y));
        let o = lx.cmp(&ly);
        if o != Ordering::Equal {
            return o;
        }
        if tertiary == Ordering::Equal && x != y {
            // lower case first
            tertiary = if x.is_lowercase() { Ordering::Less } else { Ordering::Greater };
        }
        i = i.saturating_add(1);
        j = j.saturating_add(1);
    }
    (ac.len().saturating_sub(i)).cmp(&bc.len().saturating_sub(j)).then(tertiary)
}

fn lower(c: char) -> char {
    let mut l = c.to_lowercase();
    match (l.next(), l.next()) {
        (Some(x), None) => x,
        _ => c,
    }
}

/// A wildcard pattern (* any run, ? one character), case-insensitive, whole string.
pub fn wild_match(pat: &str, s: &str) -> bool {
    let p: Vec<char> = pat.chars().map(lower).collect();
    let t: Vec<char> = s.chars().map(lower).collect();
    // iterative glob with one backtrack point (linear-ish, no recursion)
    let (mut i, mut j) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while j < t.len() {
        match p.get(i) {
            Some('*') => {
                star = Some((i, j));
                i = i.saturating_add(1);
            }
            Some(c) if *c == '?' || Some(c) == t.get(j) => {
                i = i.saturating_add(1);
                j = j.saturating_add(1);
            }
            _ => match star {
                Some((si, sj)) => {
                    i = si.saturating_add(1);
                    j = sj.saturating_add(1);
                    star = Some((si, j));
                }
                None => return false,
            },
        }
    }
    while p.get(i) == Some(&'*') {
        i = i.saturating_add(1);
    }
    i == p.len()
}

fn of_type(e: &Entity, typ: &str) -> bool {
    typ == "any" || e.typ() == typ
}

/// `a.toLowerCase() === b` where b is already lower case, without allocating.
fn eq_lower(a: &str, b_lower: &str) -> bool {
    a.chars().flat_map(char::to_lowercase).eq(b_lower.chars())
}

/// The value's text, lower-cased once per rule.
struct Prepared {
    w: String,
    w_lower: String,
}

fn matches(e: &Entity, spec: &Spec, pre: &Prepared) -> bool {
    if !of_type(e, &spec.typ) {
        return false;
    }
    if spec.op == "all" {
        return true;
    }
    let Some((types, kind)) = prop_def(&spec.prop) else { return false };
    if types.is_some_and(|t| !t.contains(&e.typ())) {
        return false;
    }
    // the layer (the usual rule): read in place
    if spec.prop == "layer" && matches!(spec.op.as_str(), "=" | "<>") {
        let l = e.layer.as_deref().unwrap_or("0");
        let eq = eq_lower(l, &pre.w_lower);
        return if spec.op == "=" { eq } else { !eq };
    }
    let v = prop_value(e, &spec.prop);
    if v == QsVal::Null {
        return kind == PropKind::Color && spec.value == QsVal::Null && (spec.op == "=" || spec.op == "*");
    }
    match kind {
        PropKind::Color => {
            let eq = same_color(&v, &spec.value);
            if spec.op == "<>" { !eq } else { eq }
        }
        PropKind::Number => {
            let x = match &spec.value {
                QsVal::Num(n) => *n,
                QsVal::Str(s) => js::str_to_number(s),
                QsVal::Null => 0.0,
            };
            let QsVal::Num(v) = v else { return false };
            if !x.is_finite() {
                return false;
            }
            let tol = 1e-9 * js::max(1.0, x.abs());
            match spec.op.as_str() {
                "=" => (v - x).abs() <= tol,
                "<>" => (v - x).abs() > tol,
                ">" => v > x + tol,
                "<" => v < x - tol,
                _ => false,
            }
        }
        PropKind::Text | PropKind::Choice => {
            let s = v.js_string();
            let w = &pre.w;
            match spec.op.as_str() {
                "=" => eq_lower(&s, &pre.w_lower),
                "<>" => !eq_lower(&s, &pre.w_lower),
                "*" => wild_match(w, &s),
                ">" => locale_cmp(&s, w) == std::cmp::Ordering::Greater,
                "<" => locale_cmp(&s, w) == std::cmp::Ordering::Less,
                _ => false,
            }
        }
    }
}

/// `quickSelect(entities, spec)` → the ids that match (exclude: that do not), in drawing order.
pub fn quick_select(entities: &[&Entity], spec: &Spec) -> Vec<u64> {
    let within: Option<std::collections::HashSet<u64>> = spec.within.as_ref().map(|w| w.iter().copied().collect());
    let w = if spec.value == QsVal::Null { String::new() } else { spec.value.js_string() };
    let pre = Prepared { w_lower: w.chars().flat_map(char::to_lowercase).collect(), w };
    let mut out = Vec::new();
    for e in entities {
        let Some(id) = e.id else { continue };
        if within.as_ref().is_some_and(|w| !w.contains(&id)) {
            continue;
        }
        let m = matches(e, spec, &pre);
        if if spec.exclude { !m } else { m } {
            out.push(id);
        }
    }
    out
}

/// `valuesOf(entities, type, prop)`: the values present, for the dialog's pick list (sorted as the JS sorts them).
pub fn values_of(entities: &[&Entity], typ: &str, prop: &str) -> Vec<QsVal> {
    let Some((types, _)) = prop_def(prop) else { return Vec::new() };
    let mut keys: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut vals: Vec<QsVal> = Vec::new();
    for e in entities {
        if !of_type(e, typ) || types.is_some_and(|t| !t.contains(&e.typ())) {
            continue;
        }
        let v = prop_value(e, prop);
        // typeof v + ':' + String(v)
        let ty = match v {
            QsVal::Num(_) => "number",
            QsVal::Str(_) => "string",
            QsVal::Null => "object",
        };
        let k = format!("{ty}:{}", v.js_string());
        if keys.insert(k) {
            vals.push(v);
        }
    }
    vals.sort_by(|a, b| locale_cmp(&a.js_string(), &b.js_string()));
    vals
}

/// The types QSELECT offers for these objects: 'any', then each type present.
pub fn types_present(entities: &[&Entity]) -> Vec<&'static str> {
    let mut seen = [false; QS_TYPES.len()];
    for e in entities {
        let t = e.typ();
        if let Some(i) = QS_TYPES.iter().position(|q| *q == t)
            && let Some(s) = seen.get_mut(i)
        {
            *s = true;
        }
    }
    let mut v = vec!["any"];
    v.extend(QS_TYPES.iter().zip(seen).filter(|(_, s)| *s).map(|(t, _)| *t));
    v
}

/// SELECTSIMILAR's properties (`SIMILAR`), in order, and which are on by default.
pub const SIMILAR: [(&str, bool); 5] = [("layer", true), ("color", true), ("name", true), ("pattern", true), ("height", false)];

/// `similarTo(entities, refs, on)`
pub fn similar_to(entities: &[&Entity], refs: &[Entity], on: &[bool; 5]) -> Vec<u64> {
    let [layer, color, name, pattern, height] = *on;
    let lay = |e: &Entity| e.layer.clone().unwrap_or_else(|| "0".into());
    let h = |e: &Entity| if let Geo::Text(t) = &e.geo { t.h } else { e.num("h").unwrap_or(f64::NAN) };
    let mut out = Vec::new();
    for e in entities {
        for r in refs {
            if e.typ() != r.typ() {
                continue;
            }
            if layer && lay(e) != lay(r) {
                continue;
            }
            if color && !same_color(&color_val(&e.color), &color_val(&r.color)) {
                continue;
            }
            if name && e.typ() == "insert" && prop_value(e, "block") != prop_value(r, "block") {
                continue;
            }
            if pattern && e.typ() == "hatch" && pattern_of(e) != pattern_of(r) {
                continue;
            }
            if height && e.typ() == "text" && (h(e) - h(r)).abs() > 1e-9 * js::max(1.0, h(r)) {
                continue;
            }
            if let Some(id) = e.id {
                out.push(id);
            }
            break;
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------- FIND

/// What to find (findtext.js spec).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FindSpec {
    pub find: String,
    /// None = only find
    pub replace: Option<String>,
    pub match_case: bool,
    pub whole: bool,
    pub wildcard: bool,
    /// text, attr, dim
    pub kinds: [bool; 3],
}
impl FindSpec {
    pub fn new(find: &str, replace: Option<String>) -> Self {
        FindSpec { find: find.into(), replace, kinds: [true; 3], ..FindSpec::default() }
    }
}

/// One hit.
#[derive(Clone, Debug, PartialEq)]
pub struct FindMatch {
    pub id: u64,
    /// 'text' | 'attr' | 'dim'
    pub kind: &'static str,
    pub tag: Option<String>,
    pub before: String,
    pub after: Option<String>,
    pub count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Tok {
    Ch(char),
    Any,
    Run,
}

fn word_char(c: Option<&char>) -> bool {
    c.is_some_and(|c| c.is_alphanumeric() || *c == '_')
}

/// A search compiled once (findtext.js `findRegExp`): the pattern's tokens, lower-cased when case does not count.
struct Finder {
    toks: Vec<Tok>,
    /// no wildcard in it: a plain run of characters
    plain: Option<Vec<char>>,
    case: bool,
    whole: bool,
}
impl Finder {
    fn new(spec: &FindSpec) -> Finder {
        let toks: Vec<Tok> = spec
            .find
            .chars()
            .map(|c| match c {
                '*' if spec.wildcard => Tok::Run,
                '?' if spec.wildcard => Tok::Any,
                c => Tok::Ch(if spec.match_case { c } else { lower(c) }),
            })
            .collect();
        let plain = toks.iter().map(|t| if let Tok::Ch(c) = t { Some(*c) } else { None }).collect::<Option<Vec<char>>>();
        Finder { toks, plain, case: spec.match_case, whole: spec.whole }
    }
    fn eq(&self, x: char, pat: char) -> bool {
        if self.case { x == pat } else { x == pat || lower(x) == pat }
    }
}

/// The search: every match as (start, end) char ranges, non-overlapping, left to right (`String.match(/…/g)`).
fn find_ranges(f: &Finder, s: &[char]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    if f.toks.is_empty() {
        return out;
    }
    let boundary_ok = |a: usize, b: usize| !f.whole || (!(a > 0 && word_char(s.get(a.saturating_sub(1)))) && !word_char(s.get(b)));
    if let Some(p) = &f.plain {
        // a plain run: compare in place (no backtracking)
        let m = p.len();
        let mut i = 0usize;
        while i.saturating_add(m) <= s.len() {
            let hit = s.get(i..i.saturating_add(m)).is_some_and(|w| w.iter().zip(p).all(|(x, c)| f.eq(*x, *c)));
            if hit && boundary_ok(i, i.saturating_add(m)) {
                out.push((i, i.saturating_add(m)));
                i = i.saturating_add(m.max(1));
                if out.len() > 100_000 {
                    break;
                }
            } else {
                i = i.saturating_add(1);
            }
        }
        return out;
    }
    let toks = &f.toks;
    // lazy match of toks at s[i..] → end (shortest first, as `*?`); a bounded backtracking walk
    let try_at = |i: usize| -> Option<usize> {
        let mut stack: Vec<(usize, usize)> = vec![(0, i)];
        let mut budget = 200_000usize;
        while let Some((ti, si)) = stack.pop() {
            budget = budget.saturating_sub(1);
            if budget == 0 {
                return None;
            }
            match toks.get(ti) {
                None => {
                    if !f.whole || !word_char(s.get(si)) {
                        return Some(si);
                    }
                }
                Some(Tok::Ch(c)) => {
                    if s.get(si).is_some_and(|x| f.eq(*x, *c)) {
                        stack.push((ti.saturating_add(1), si.saturating_add(1)));
                    }
                }
                Some(Tok::Any) => {
                    if s.get(si).is_some_and(|x| *x != '\n') {
                        stack.push((ti.saturating_add(1), si.saturating_add(1)));
                    }
                }
                Some(Tok::Run) => {
                    // longer runs pushed first so the shortest is tried first
                    let mut end = si;
                    while s.get(end).is_some_and(|x| *x != '\n') {
                        end = end.saturating_add(1);
                    }
                    let mut k = end;
                    loop {
                        stack.push((ti.saturating_add(1), k));
                        if k == si {
                            break;
                        }
                        k = k.saturating_sub(1);
                    }
                }
            }
        }
        None
    };
    let mut i = 0usize;
    while i <= s.len() {
        if f.whole && i > 0 && word_char(s.get(i.saturating_sub(1))) {
            i = i.saturating_add(1);
            continue;
        }
        match try_at(i) {
            Some(end) => {
                out.push((i, end));
                i = if end > i { end } else { i.saturating_add(1) };
            }
            None => i = i.saturating_add(1),
        }
        if out.len() > 100_000 {
            break;
        }
    }
    out
}

/// One text tested → (count, replaced text when replacing).
/// A plain search over the text in place → byte ranges of the hits (no copy of the text).
fn plain_ranges(f: &Finder, p: &[char], s: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let Some(first) = p.first() else { return out };
    let word_before = |at: usize| s.get(..at).and_then(|x| x.chars().next_back()).is_some_and(|c| c.is_alphanumeric() || c == '_');
    let mut from = 0usize;
    while from < s.len() {
        let Some(rest) = s.get(from..) else { break };
        // the next place the first character occurs
        let Some((off, _)) = rest.char_indices().find(|(_, c)| f.eq(*c, *first)) else { break };
        let at = from.saturating_add(off);
        let mut it = s.get(at..).unwrap_or("").char_indices();
        let mut end = at;
        let mut ok = true;
        for pc in p {
            match it.next() {
                Some((i, c)) if f.eq(c, *pc) => end = at.saturating_add(i).saturating_add(c.len_utf8()),
                _ => {
                    ok = false;
                    break;
                }
            }
        }
        if ok
            && f.whole
            && (word_before(at) || s.get(end..).and_then(|x| x.chars().next()).is_some_and(|c| c.is_alphanumeric() || c == '_'))
        {
            ok = false;
        }
        if ok {
            out.push((at, end));
            from = end.max(at.saturating_add(1));
            if out.len() > 100_000 {
                break;
            }
        } else {
            // one character on
            from = at.saturating_add(s.get(at..).and_then(|x| x.chars().next()).map_or(1, char::len_utf8));
        }
    }
    out
}

fn test_text_with(f: &Finder, replace: Option<&String>, s: &str) -> Option<(usize, Option<String>)> {
    if let Some(p) = &f.plain {
        let rs = plain_ranges(f, p, s);
        if rs.is_empty() {
            return None;
        }
        let after = replace.map(|rep| {
            let mut out = String::with_capacity(s.len());
            let mut at = 0usize;
            for (a, b) in &rs {
                out.push_str(s.get(at..*a).unwrap_or(""));
                out.push_str(rep);
                at = *b;
            }
            out.push_str(s.get(at..).unwrap_or(""));
            out
        });
        return Some((rs.len(), after));
    }
    let cs: Vec<char> = s.chars().collect();
    let rs = find_ranges(f, &cs);
    if rs.is_empty() {
        return None;
    }
    let after = replace.map(|rep| {
        let mut out = String::new();
        let mut at = 0usize;
        for (a, b) in &rs {
            out.extend(cs.get(at..*a).unwrap_or(&[]));
            out.push_str(rep);
            at = *b;
        }
        out.extend(cs.get(at..).unwrap_or(&[]));
        out
    });
    Some((rs.len(), after))
}

#[cfg(test)]
fn test_text(spec: &FindSpec, s: &str) -> Option<(usize, Option<String>)> {
    test_text_with(&Finder::new(spec), spec.replace.as_ref(), s)
}

/// `findMatches(entities, spec)`
pub fn find_matches<'a>(entities: impl IntoIterator<Item = &'a Entity>, spec: &FindSpec) -> Vec<FindMatch> {
    if spec.find.is_empty() {
        return Vec::new();
    }
    let [kt, ka, kd] = spec.kinds;
    let finder = Finder::new(spec);
    let test_text = |_: &FindSpec, s: &str| test_text_with(&finder, spec.replace.as_ref(), s);
    let mut out = Vec::new();
    for e in entities {
        let Some(id) = e.id else { continue };
        match &e.geo {
            Geo::Text(_) | Geo::Mleader(_) if kt => {
                let s: &str = match &e.geo {
                    Geo::Text(t) => &t.s,
                    Geo::Mleader(m) => m.s.as_deref().unwrap_or(""),
                    _ => "",
                };
                if let Some((count, after)) = test_text(spec, s) {
                    out.push(FindMatch { id, kind: "text", tag: None, before: s.to_string(), after, count });
                }
            }
            Geo::Insert(i) if ka => {
                let Some(attrs) = &i.attrs else { continue };
                // Object.keys order: array-index keys first (only when there are any)
                let keys: Vec<&String> = if attrs.keys().any(|k| k.bytes().next().is_some_and(|b| b.is_ascii_digit())) {
                    js_keys(attrs)
                } else {
                    attrs.keys().collect()
                };
                for tag in keys {
                    let hit = match attrs.get(tag.as_str()) {
                        Some(Value::String(v)) => test_text(spec, v).map(|h| (h, v.clone())),
                        Some(v) => {
                            let v = js::to_js_string(v);
                            test_text(spec, &v).map(|h| (h, v))
                        }
                        None => None,
                    };
                    if let Some(((count, after), v)) = hit {
                        out.push(FindMatch { id, kind: "attr", tag: Some(tag.clone()), before: v, after, count });
                    }
                }
            }
            Geo::Dim(d) if kd => {
                let Some(text) = d.text.as_ref().filter(|t| !t.is_empty()) else { continue };
                let parts: Vec<&str> = text.split("<>").collect();
                let hits: Vec<Option<(usize, Option<String>)>> = parts.iter().map(|p| test_text(spec, p)).collect();
                if hits.iter().any(Option::is_some) {
                    let count = hits.iter().map(|h| h.as_ref().map_or(0, |x| x.0)).sum();
                    let after = spec.replace.as_ref().map(|_| {
                        parts
                            .iter()
                            .zip(&hits)
                            .map(|(p, h)| h.as_ref().and_then(|x| x.1.clone()).unwrap_or_else(|| (*p).to_string()))
                            .collect::<Vec<_>>()
                            .join("<>")
                    });
                    out.push(FindMatch { id, kind: "dim", tag: None, before: text.clone(), after, count });
                }
            }
            _ => {}
        }
    }
    out
}

/// `applyMatches(doc, matches)` (inside a change) → the number of objects changed.
pub fn apply_matches(doc: &mut Doc, ms: &[FindMatch]) -> Result<usize, cc_cad::DocError> {
    // grouped by object in first-hit order (a Map), indexed so many hits stay linear
    let mut by_id: Vec<(u64, Vec<&FindMatch>)> = Vec::new();
    let mut index: std::collections::HashMap<u64, usize> = std::collections::HashMap::new();
    for m in ms {
        if m.after.is_none() {
            continue;
        }
        match index.get(&m.id) {
            Some(i) => {
                if let Some((_, v)) = by_id.get_mut(*i) {
                    v.push(m);
                }
            }
            None => {
                index.insert(m.id, by_id.len());
                by_id.push((m.id, vec![m]));
            }
        }
    }
    let mut n = 0usize;
    for (id, list) in by_id {
        let Some(e) = doc.get(id) else { continue };
        if !doc.editable(e) {
            continue;
        }
        let mut next = e.clone();
        for m in list {
            let after = m.after.clone().unwrap_or_default();
            match (m.kind, &mut next.geo) {
                ("text", Geo::Text(t)) => t.s = after,
                ("text", Geo::Mleader(ml)) => ml.s = Some(after),
                ("dim", Geo::Dim(d)) => {
                    d.text = Some(after);
                    d.block = None;
                }
                ("attr", Geo::Insert(i)) => {
                    let mut a = i.attrs.clone().unwrap_or_default();
                    if let Some(tag) = &m.tag {
                        a.insert(tag.clone(), Value::String(after));
                    }
                    i.attrs = Some(a);
                }
                _ => {}
            }
        }
        if let Geo::Text(t) = &next.geo
            && js::trim(&t.s).is_empty()
        {
            continue;
        }
        doc.update(id, next)?;
        n = n.saturating_add(1);
    }
    Ok(n)
}

// ---------------------------------------------------------------------------------------------------------------- commands

/// `qsTypeLabel(type)`
pub fn type_label(c: &crate::lang::Lang, ty: &str) -> String {
    if ty == "any" { c.s("qs.any") } else { c.s(&format!("type.{ty}")) }
}

/// The value typed for a rule (commands.js QSELECT's command line).
pub fn typed_value(kind: PropKind, raw: &str, lunits: f64) -> QsVal {
    match kind {
        PropKind::Number => QsVal::Num(parse_length(raw, lunits).unwrap_or_else(|| parse_float(raw))),
        PropKind::Color => {
            let l = raw.to_lowercase();
            if raw.is_empty() || l == "bylayer" || l == "by layer" {
                QsVal::Null
            } else if l == "byblock" || l == "by block" {
                QsVal::Num(0.0)
            } else if raw.starts_with('#') {
                QsVal::Str(raw.into())
            } else {
                QsVal::Num(parse_int(raw))
            }
        }
        _ => QsVal::Str(raw.into()),
    }
}

/// `parseInt(s, 10)`
fn parse_int(s: &str) -> f64 {
    let t = js::trim(s);
    let (neg, rest) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return f64::NAN;
    }
    let v = js::str_to_number(&digits);
    if neg { -v } else { v }
}

fn editable_pool(doc: &Doc) -> Vec<&Entity> {
    doc.entities().filter(|e| doc.editable(e)).collect()
}

pub async fn qselect(c: Ctx) -> R {
    if c.sh().host.borrow_mut().quick_select() {
        return Ok(());
    }
    let (types, lunits) = {
        let d = c.doc();
        (types_present(&editable_pool(&d)), d.meta.get("lunits").map_or(2.0, js::to_number))
    };
    let lang = c.sh().lang.clone();
    let typ = loop {
        let list = types.iter().map(|t| type_label(&lang, t)).collect::<Vec<_>>().join(", ");
        let r = c
            .string(c.tv("cmd.qs.type", &[("list", list)]), Opts { default: Some(Def::Str(type_label(&lang, "any"))), ..Opts::default() })
            .await?;
        let v = match r {
            Reply::Str(s) => js::trim(&s).to_lowercase(),
            _ => String::new(),
        };
        if let Some(t) = types.iter().find(|t| type_label(&lang, t).to_lowercase() == v || **t == v) {
            break *t;
        }
        c.print(&c.t("cmd.qs.type.bad"));
    };
    let props = props_for(typ);
    let prop = loop {
        let list = props.iter().map(|k| c.t(&format!("qs.prop.{k}"))).collect::<Vec<_>>().join(", ");
        let r = c
            .string(c.tv("cmd.qs.prop", &[("list", list)]), Opts { default: Some(Def::Str(c.t("qs.prop.layer"))), ..Opts::default() })
            .await?;
        let v = match r {
            Reply::Str(s) => js::trim(&s).to_lowercase(),
            _ => String::new(),
        };
        if let Some(k) = props.iter().find(|k| c.t(&format!("qs.prop.{k}")).to_lowercase() == v || **k == v) {
            break *k;
        }
        c.print(&c.t("cmd.qs.prop.bad"));
    };
    let options =
        vec![c.opt("=", "="), c.opt("<>", "<>"), c.opt(">", ">"), c.opt("<", "<"), c.opt("*", "*"), c.opt("ALL", &c.t("qs.op.all"))];
    let k = c.keyword(c.t("cmd.qs.op"), Opts { options, default: Some(Def::Str("=".into())), ..Opts::default() }).await?;
    let raw_op = match k {
        Reply::Opt(o) | Reply::Str(o) => o,
        _ => String::new(),
    };
    let op = if raw_op == "ALL" { "all".to_string() } else { raw_op };
    let mut value = QsVal::Null;
    if op != "all" {
        let r = c.string(c.t("cmd.qs.value"), Opts::default()).await?;
        let raw = match r {
            Reply::Str(s) => js::trim(&s).to_string(),
            _ => String::new(),
        };
        let kind = prop_def(prop).map_or(PropKind::Text, |p| p.1);
        value = typed_value(kind, &raw, lunits);
    }
    let spec = Spec { typ: typ.into(), prop: prop.into(), op, value, exclude: false, within: None, append: false };
    let found = {
        let d = c.doc();
        quick_select(&editable_pool(&d), &spec)
    };
    c.set_selection(&found);
    c.print(&c.tv("cmd.qs.done", &[("n", found.len().to_string())]));
    Ok(())
}

pub async fn selectsimilar(c: Ctx) -> R {
    let on0 = c.st().memo.similar_on.unwrap_or([true, true, true, true, false]);
    c.st().memo.similar_on = Some(on0);
    let ids: IdSet;
    loop {
        let r = c.select_with(Some(c.t("cmd.sim.select")), false, vec![c.opt("SE", &c.t("opt.settings"))]).await?;
        match r {
            Sel::Opt(o) if o == "SE" => {}
            Sel::Opt(_) => {
                ids = IdSet::default();
                break;
            }
            Sel::Set(s) => {
                ids = s;
                break;
            }
        }
        loop {
            let on = c.st().memo.similar_on.unwrap_or(on0);
            let list: Vec<String> =
                SIMILAR.iter().zip(on.iter()).filter(|(_, o)| **o).map(|((k, _), _)| c.t(&format!("sim.{k}"))).collect();
            let list = if list.is_empty() { c.t("cmd.ma.nothing") } else { list.join(", ") };
            c.print(&c.tv("cmd.sim.now", &[("list", list)]));
            let options = SIMILAR.iter().map(|(k, _)| c.opt(&k.get(..2).unwrap_or(k).to_uppercase(), &c.t(&format!("sim.{k}")))).collect();
            let k = c.keyword(c.t("cmd.ma.toggle"), Opts { options, none: true, ..Opts::default() }).await?;
            let Reply::Opt(k) = k else { break };
            let Some(i) = SIMILAR.iter().position(|(key, _)| key.get(..2).unwrap_or(key).to_uppercase() == k) else { break };
            let mut on = on;
            if let Some(x) = on.get_mut(i) {
                *x = !*x;
            }
            c.st().memo.similar_on = Some(on);
        }
    }
    if ids.is_empty() {
        return Ok(());
    }
    let on = c.st().memo.similar_on.unwrap_or(on0);
    let found = {
        let d = c.doc();
        let refs: Vec<Entity> = ids.0.iter().filter_map(|id| d.get(*id).cloned()).collect();
        similar_to(&editable_pool(&d), &refs, &on)
    };
    c.set_selection(&found);
    c.print(&c.tv("cmd.sim.done", &[("n", found.len().to_string())]));
    Ok(())
}

/// FIND's scope: the selection when there is one, else everything (editable objects only).
/// → the selected objects (editable), or None = every editable object.
pub fn find_scope(doc: &Doc, selection: &[u64]) -> Option<Vec<u64>> {
    let sel: Vec<u64> = selection.iter().copied().filter(|id| doc.get(*id).is_some()).collect();
    if sel.is_empty() { None } else { Some(sel.into_iter().filter(|id| doc.get(*id).is_some_and(|e| doc.editable(e))).collect()) }
}

/// The objects a FIND scope covers.
pub fn scope_pool<'a>(doc: &'a Doc, scope: Option<&[u64]>) -> Vec<&'a Entity> {
    match scope {
        Some(ids) => ids.iter().filter_map(|id| doc.get(*id)).collect(),
        None => editable_pool(doc),
    }
}

/// Run a FIND spec over the scope: select what is found, or replace (one undo step labelled FIND) → the line to print.
pub fn run_find(c: &Ctx, scope: Option<&[u64]>, spec: &FindSpec, replace: bool) -> R {
    let ms = {
        let d = c.doc();
        match scope {
            Some(ids) => find_matches(ids.iter().filter_map(|id| d.get(*id)), spec),
            None => find_matches(d.entities().filter(|e| d.editable(e)), spec),
        }
    };
    if ms.is_empty() {
        c.print(&c.tv("cmd.find.none", &[("find", spec.find.clone())]));
        return Ok(());
    }
    let total: usize = ms.iter().map(|m| m.count).sum();
    if replace {
        let n = c.change("FIND", |d| apply_matches(d, &ms))?;
        c.print(&c.tv("cmd.find.replaced", &[("n", total.to_string()), ("m", n.to_string())]));
        return Ok(());
    }
    let ids = IdSet::from_vec(ms.iter().map(|m| m.id).collect());
    c.set_selection(&ids.0);
    c.print(&c.tv("cmd.find.found", &[("n", total.to_string()), ("m", ids.len().to_string())]));
    Ok(())
}

pub async fn find(c: Ctx) -> R {
    let scope = {
        let sel = c.sh().host.borrow().selection();
        find_scope(&c.doc(), &sel)
    };
    if c.sh().host.borrow_mut().find_dialog() {
        return Ok(());
    }
    let f = match c.string(c.t("cmd.find.what"), Opts::default()).await? {
        Reply::Str(s) => s,
        _ => String::new(),
    };
    if f.is_empty() {
        return Ok(());
    }
    let rep = match c.string(c.t("cmd.find.with"), Opts::default()).await? {
        Reply::Str(s) => s,
        _ => String::new(),
    };
    let replace = !rep.is_empty();
    let spec = FindSpec::new(&f, replace.then_some(rep));
    run_find(&c, scope.as_deref(), &spec, replace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards_and_find() {
        assert!(wild_match("a*c", "ABBC") && wild_match("a?c", "abc") && !wild_match("a?c", "ac") && wild_match("*", ""));
        let spec = FindSpec::new("ab", Some("X".into()));
        assert_eq!(test_text(&spec, "Abcab ab"), Some((3, Some("XcX X".into()))));
        let mut w = FindSpec::new("cat", None);
        w.whole = true;
        assert_eq!(test_text(&w, "cat cats cat_ bobcat cat").map(|x| x.0), Some(2));
        let mut wc = FindSpec::new("a*c", None);
        wc.wildcard = true;
        assert_eq!(test_text(&wc, "abcabc").map(|x| x.0), Some(2));
    }

    #[test]
    fn locale_compare_is_natural() {
        use std::cmp::Ordering::*;
        assert_eq!(locale_cmp("A2", "A10"), Less);
        assert_eq!(locale_cmp("b", "A"), Greater);
        assert_eq!(locale_cmp("a", "A"), Less);
        assert_eq!(locale_cmp("abc", "ab"), Greater);
    }
}
