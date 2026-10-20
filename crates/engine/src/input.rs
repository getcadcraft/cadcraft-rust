//! What the command line reads: typed points (`x,y` · `@dx,dy` · `@d<a` · `d<a` · a bare distance toward the cursor),
//! keyword options (their capital letters or in full), numbers, and how a question is shown (`promptText`).
//! commands.js `parsePoint`, `matchOption`, `opt`, `selectOptions`, `fmt`, `promptText`.

use cc_cad::js;
use cc_cad_geom::geom::{angle_of, deg, dist, norm_angle, polar, rad};
use cc_cad_geom::units::parse_length;
use cc_geom::P2;

use crate::lang::Lang;
use crate::ucs::{Ucs, ang_of, to_world, vec_to_world};

/// A keyword option: `key` is typed, `label` shown (translated), `en` the English label (scripts type it in any
/// language); hidden options are accepted but not listed.
#[derive(Clone, Debug, PartialEq)]
pub struct Opt {
    pub key: String,
    pub label: String,
    pub en: Option<String>,
    pub hidden: bool,
}

/// `opt(key, label)`: the prompt shows opt.<word> from the catalog when it has one ("Close" → opt.close).
pub fn opt(lang: &Lang, key: &str, label: &str) -> Opt {
    let word: String = label.to_lowercase().chars().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit()).collect();
    let k = format!("opt.{word}");
    let shown = if !word.is_empty() && lang.has(&k) { lang.s(&k) } else { label.to_string() };
    // same word: keep the casing that marks the letters (CLose)
    let label_out = if shown.to_lowercase() == label.to_lowercase() { label.to_string() } else { shown };
    Opt { key: key.into(), label: label_out, en: Some(label.into()), hidden: false }
}

/// A hidden option with no English label (CIRCLE TTT).
pub fn hidden(key: &str, label: &str) -> Opt {
    Opt { key: key.into(), label: label.into(), en: None, hidden: true }
}

/// What "Select objects" takes besides clicks (not shown in the prompt; ? lists them).
pub fn select_options(lang: &Lang) -> Vec<Opt> {
    let mut v: Vec<Opt> = [
        ("P", "sel.opt.previous"),
        ("F", "sel.opt.fence"),
        ("W", "sel.opt.window"),
        ("C", "sel.opt.crossing"),
        ("R", "sel.opt.remove"),
        ("A", "sel.opt.add"),
    ]
    .iter()
    .map(|(k, l)| Opt { key: (*k).into(), label: lang.s(l), en: None, hidden: true })
    .collect();
    v.push(hidden("?", "?"));
    v
}

/// `matchOption(typed, options)`: the key exactly, else a label (translated or English) the typed text begins.
pub fn match_option<'a>(typed: &str, options: &'a [Opt]) -> Option<&'a Opt> {
    let u = typed.to_uppercase();
    let starts = |s: &str| s.to_uppercase().starts_with(&u);
    let n = u.encode_utf16().count();
    options.iter().find(|o| o.key.to_uppercase() == u).or_else(|| {
        options.iter().find(|o| n >= 1 && n >= o.key.encode_utf16().count() && (starts(&o.label) || o.en.as_deref().is_some_and(starts)))
    })
}

/// `fmt(n)`: 4 decimals, trailing zeros dropped, tiny → '0'.
pub fn fmt(n: f64) -> String {
    if n.abs() < 1e-10 {
        return "0".into();
    }
    js::num_to_string(js::str_to_number(&js::to_fixed(n, 4)))
}

/// JS `parseFloat(s)`: the longest number at the start (after white space), NaN when there is none.
pub fn parse_float(s: &str) -> f64 {
    let t = js::trim_start(s);
    let b = t.as_bytes();
    let mut i = 0usize;
    if matches!(b.first(), Some(b'+' | b'-')) {
        i = i.saturating_add(1);
    }
    if t.get(i..).is_some_and(|r| r.starts_with("Infinity")) {
        return if b.first() == Some(&b'-') { f64::NEG_INFINITY } else { f64::INFINITY };
    }
    let digits = |i: &mut usize| {
        let s0 = *i;
        while b.get(*i).is_some_and(u8::is_ascii_digit) {
            *i = (*i).saturating_add(1);
        }
        *i > s0
    };
    let mut any = digits(&mut i);
    if b.get(i) == Some(&b'.') {
        let mut j = i.saturating_add(1);
        if digits(&mut j) || any {
            any = true;
            i = j;
        }
    }
    if !any {
        return f64::NAN;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let mut j = i.saturating_add(1);
        if matches!(b.get(j), Some(b'+' | b'-')) {
            j = j.saturating_add(1);
        }
        if digits(&mut j) {
            i = j;
        }
    }
    t.get(..i).map_or(f64::NAN, js::str_to_number)
}

/// The default a question shows and Enter takes.
#[derive(Clone, Debug, PartialEq)]
pub enum Def {
    Num(f64),
    Str(String),
    /// a point default (`{ default: { x, y } }`): Enter answers it; a point prompt shows none
    Point(cc_geom::P2),
}

/// `promptText()` of a question.
pub fn prompt_text(prompt: &str, kind_angle: bool, kind_point: bool, options: &[Opt], default: Option<&Def>, ucs_ang: f64) -> String {
    let shown: Vec<String> =
        options
            .iter()
            .filter(|o| !o.hidden)
            .map(|o| {
                if o.label.to_uppercase().starts_with(&o.key.to_uppercase()) { o.label.clone() } else { format!("{} ({})", o.label, o.key) }
            })
            .collect();
    let d_opt = match default {
        Some(Def::Str(s)) => options.iter().find(|o| o.en.as_deref() == Some(s.as_str()) && o.en.as_deref() != Some(o.key.as_str())),
        _ => None,
    };
    let def = match default {
        Some(d) if !kind_point || d_opt.is_some() => {
            let s = if kind_angle {
                match d {
                    Def::Num(a) => fmt(deg(norm_angle(a - ucs_ang))),
                    Def::Str(_) | Def::Point(_) => fmt(f64::NAN),
                }
            } else {
                match (d, d_opt) {
                    (Def::Num(n), _) => fmt(*n),
                    (_, Some(o)) => o.label.clone(),
                    (Def::Str(s), None) => s.clone(),
                    (Def::Point(_), None) => "[object Object]".into(),
                }
            };
            format!(" <{s}>")
        }
        _ => String::new(),
    };
    let opts = if shown.is_empty() { String::new() } else { format!(" [{}]", shown.join("/")) };
    format!("{prompt}{opts}{def}:")
}

/// `/^[-+]?(\d+\.?\d*|\.\d+)(e[-+]?\d+)?$/i` (with_exp) or the same without the exponent.
pub fn is_plain_number(s: &str, with_exp: bool) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    if matches!(b.first(), Some(b'+' | b'-')) {
        i = 1;
    }
    let digits = |i: &mut usize| {
        let st = *i;
        while b.get(*i).is_some_and(u8::is_ascii_digit) {
            *i = i.saturating_add(1);
        }
        *i > st
    };
    if b.get(i) == Some(&b'.') {
        i = i.saturating_add(1);
        if !digits(&mut i) {
            return false;
        }
    } else {
        if !digits(&mut i) {
            return false;
        }
        if b.get(i) == Some(&b'.') {
            i = i.saturating_add(1);
            digits(&mut i);
        }
    }
    if with_exp && matches!(b.get(i), Some(b'e' | b'E')) {
        let mut j = i.saturating_add(1);
        if matches!(b.get(j), Some(b'+' | b'-')) {
            j = j.saturating_add(1);
        }
        if digits(&mut j) {
            i = j;
        } else {
            return false;
        }
    }
    i == b.len()
}

/// `parseFloat` of text that passed [`is_plain_number`].
pub fn plain_number(s: &str) -> f64 {
    s.parse::<f64>().unwrap_or(f64::NAN)
}

/// `zoomFactor(s)`: "2", "2x", ".5X" → the factor (> 0), else None.
pub fn zoom_factor(s: &str) -> Option<f64> {
    let t = js::trim(s);
    let body = t.strip_suffix(['x', 'X']).map(js::trim_end).unwrap_or(t);
    if body.is_empty() || body.starts_with(['+', '-']) || !is_plain_number(body, false) {
        return None;
    }
    let v = plain_number(body);
    if v > 0.0 { Some(v) } else { None }
}

/// `s.replace(/\s*([,<@#*])\s*/g, '$1')` after trimming.
fn squeeze(t: &str) -> String {
    let mut out = String::new();
    let mut skip_ws = false;
    for ch in js::trim(t).chars() {
        if matches!(ch, ',' | '<' | '@' | '#' | '*') {
            while out.ends_with(js::is_ws) {
                out.pop();
            }
            out.push(ch);
            skip_ws = true;
        } else if skip_ws && js::is_ws(ch) {
            continue;
        } else {
            skip_ws = false;
            out.push(ch);
        }
    }
    out
}

const SPECIAL: [char; 5] = [',', '<', '@', '#', '*'];

/// Where a typed point is read: the drawing's length format, the last point, the rubber-band base, the cursor.
pub struct PointCtx {
    pub lunits: f64,
    pub ucs: Option<Ucs>,
    pub last_point: Option<P2>,
    pub base: Option<P2>,
    pub cursor: Option<P2>,
}

/// `parsePoint(text)` → a world point, or None.
pub fn parse_point(t: &str, c: &PointCtx) -> Option<P2> {
    let mut s = squeeze(t);
    // a * after @ / # or first = world coordinates
    let mut world = false;
    for pre in ["@*", "#*", "*"] {
        if let Some(rest) = s.strip_prefix(pre) {
            world = true;
            s = format!("{}{rest}", pre.trim_end_matches('*'));
            break;
        }
    }
    let u = if world { None } else { c.ucs };
    let l = |x: &str| parse_length(x, c.lunits);
    let ang = |x: &str| if is_plain_number(x, false) { Some(js::str_to_number(x)) } else { None };
    let part_ok = |x: &str| !x.is_empty() && !x.contains(SPECIAL);
    let zero = P2::new(0.0, 0.0);
    // ^([@#])?([^,<@#*]+),([^,<@#*]+)$
    {
        let (pre, rest) = match s.chars().next() {
            Some(ch @ ('@' | '#')) => (Some(ch), s.get(1..).unwrap_or("")),
            _ => (None, s.as_str()),
        };
        if let Some((a, b)) = rest.split_once(',')
            && part_ok(a)
            && part_ok(b)
        {
            let (x, y) = (l(a)?, l(b)?);
            if pre == Some('@') {
                let base = c.last_point.unwrap_or(zero);
                let d = vec_to_world(u, P2::new(x, y));
                return Some(P2::new(base.x + d.x, base.y + d.y));
            }
            return Some(to_world(u, P2::new(x, y)));
        }
    }
    // ^(@)?([^,<@#*]+)<([^,<@#*]+)$
    {
        let (at, rest) = match s.strip_prefix('@') {
            Some(r) => (true, r),
            None => (false, s.as_str()),
        };
        if let Some((a, b)) = rest.split_once('<')
            && part_ok(a)
            && part_ok(b)
        {
            let (d, a) = (l(a)?, ang(b)?);
            let from = if at { c.last_point.unwrap_or(zero) } else { to_world(u, zero) };
            return Some(polar(from, rad(a) + ang_of(u), d));
        }
    }
    let d = l(&s)?;
    let base = c.base?;
    let cur = c.cursor?;
    if dist(base, cur) < 1e-12 {
        return None;
    }
    Some(polar(base, angle_of(base, cur), d))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> PointCtx {
        PointCtx {
            lunits: 2.0,
            ucs: None,
            last_point: Some(P2::new(10.0, 0.0)),
            base: Some(P2::new(0.0, 0.0)),
            cursor: Some(P2::new(0.0, 5.0)),
        }
    }
    #[test]
    fn typed_points() {
        let c = ctx();
        assert_eq!(parse_point("3,4", &c), Some(P2::new(3.0, 4.0)));
        assert_eq!(parse_point(" @ 1 , 2 ", &c), Some(P2::new(11.0, 2.0)));
        assert_eq!(parse_point("#3,4", &c), Some(P2::new(3.0, 4.0)));
        let p = parse_point("@10<90", &c).unwrap();
        assert!((p.x - 10.0).abs() < 1e-9 && (p.y - 10.0).abs() < 1e-9);
        let p = parse_point("7", &c).unwrap();
        assert!(p.x.abs() < 1e-9 && (p.y - 7.0).abs() < 1e-9);
        assert_eq!(parse_point("xyz", &c), None);
        assert_eq!(parse_point("#5<30", &c), None);
        assert_eq!(parse_point("1,2,3", &c), None);
    }
    #[test]
    fn numbers_and_options() {
        assert!(is_plain_number("1.5e3", true) && !is_plain_number("1.5e3", false));
        assert!(is_plain_number("-.5", false) && is_plain_number("5.", false) && !is_plain_number(".", false));
        assert_eq!(zoom_factor("2x"), Some(2.0));
        assert_eq!(zoom_factor(" .5 X"), Some(0.5));
        assert_eq!(zoom_factor("0"), None);
        assert_eq!(fmt(1.23456), "1.2346");
        assert_eq!(fmt(2.5), "2.5");
        let l = Lang::default();
        let o = vec![opt(&l, "C", "Close"), opt(&l, "U", "Undo")];
        assert_eq!(match_option("c", &o).map(|o| o.key.as_str()), Some("C"));
        assert_eq!(match_option("und", &o).map(|o| o.key.as_str()), Some("U"));
        assert_eq!(match_option("x", &o), None);
        assert_eq!(prompt_text("Next", false, true, &o, None, 0.0), "Next [Close/Undo]:");
    }
}
