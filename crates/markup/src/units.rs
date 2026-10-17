//! units.js — real-world lengths for the measure tool: calibrations `{mmPerPt, unit, label}`, `parseLength` (what
//! people type on a drawing), `formatLength` (feet-inches to 1/16"), areas.
//!
//! The JS regular expressions are matched by hand here, with the same backtracking order where it decides a capture
//! (`ft_in_parts`), so every input reads the same.

use crate::i18n::{Fill, t};
use crate::js::{hypot, is_ws, lower, ns, round, to_fixed, trim};

pub const MM_PER_IN: f64 = 25.4;
pub const MM_PER_PT: f64 = MM_PER_IN / 72.0;

/// A sheet's calibration.
#[derive(Debug, Clone, PartialEq)]
pub struct Cal {
    pub mm_per_pt: f64,
    pub unit: String,
    pub label: String,
}

/// Names `Object.prototype` answers for: `UNIT_MM[name]` (and other plain-object lookups in the JS) is truthy for these
/// (a function or an object), not undefined. Spec gap (JS quirk, kept for parity): such a "unit" passes the checks
/// that only ask for truthiness.
pub const PROTO_NAMES: [&str; 12] = [
    "constructor",
    "__defineGetter__",
    "__defineSetter__",
    "hasOwnProperty",
    "__lookupGetter__",
    "__lookupSetter__",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toString",
    "valueOf",
    "__proto__",
    "toLocaleString",
];

/// `UNIT_MM[unit]`: `Some(Some(mm))` for a unit, `Some(None)` for an `Object.prototype` name (truthy, but not a
/// number), `None` when undefined.
pub fn unit_mm(unit: &str) -> Option<Option<f64>> {
    match unit {
        "ft-in" | "ft" => Some(Some(304.8)),
        "in" => Some(Some(25.4)),
        "m" => Some(Some(1000.0)),
        "mm" => Some(Some(1.0)),
        "cm" => Some(Some(10.0)),
        u if PROTO_NAMES.contains(&u) => Some(None),
        _ => None,
    }
}

/// The units a calibration may use (UNITS ids) with their names from the language file.
pub fn units() -> Vec<(&'static str, String)> {
    ["ft-in", "ft", "in", "m", "mm"].iter().map(|u| (*u, t(&format!("unit.{u}"), &[]))).collect()
}

/// SCALE_PRESETS: (group name, [(label, ratio)], unit).
pub type PresetGroup = (String, Vec<(&'static str, f64)>, &'static str);

/// SCALE_PRESETS groups.
pub fn scale_presets() -> Vec<PresetGroup> {
    vec![
        (
            t("scale.group.architectural", &[]),
            vec![
                ("1/32\" = 1'-0\"", 384.0),
                ("1/16\" = 1'-0\"", 192.0),
                ("3/32\" = 1'-0\"", 128.0),
                ("1/8\" = 1'-0\"", 96.0),
                ("3/16\" = 1'-0\"", 64.0),
                ("1/4\" = 1'-0\"", 48.0),
                ("3/8\" = 1'-0\"", 32.0),
                ("1/2\" = 1'-0\"", 24.0),
                ("3/4\" = 1'-0\"", 16.0),
                ("1\" = 1'-0\"", 12.0),
                ("1 1/2\" = 1'-0\"", 8.0),
                ("3\" = 1'-0\"", 4.0),
            ],
            "ft-in",
        ),
        (
            t("scale.group.engineering", &[]),
            vec![
                ("1\" = 10'", 120.0),
                ("1\" = 20'", 240.0),
                ("1\" = 30'", 360.0),
                ("1\" = 40'", 480.0),
                ("1\" = 50'", 600.0),
                ("1\" = 60'", 720.0),
                ("1\" = 100'", 1200.0),
            ],
            "ft",
        ),
        (
            t("scale.group.metric", &[]),
            vec![
                ("1:1", 1.0),
                ("1:5", 5.0),
                ("1:10", 10.0),
                ("1:20", 20.0),
                ("1:25", 25.0),
                ("1:50", 50.0),
                ("1:100", 100.0),
                ("1:200", 200.0),
                ("1:250", 250.0),
                ("1:500", 500.0),
                ("1:1000", 1000.0),
            ],
            "mm",
        ),
    ]
}

/// `presetCalibration(label, ratio, unit)`.
pub fn preset_calibration(label: &str, ratio: f64, unit: &str) -> Cal {
    let u = if unit == "mm" && ratio >= 100.0 { "m" } else { unit };
    Cal { mm_per_pt: MM_PER_PT * ratio, unit: u.to_string(), label: label.to_string() }
}

/// `calibrationFrom(ptLength, realMm, unit)`.
pub fn calibration_from(pt_length: f64, real_mm: f64, unit: &str) -> Option<Cal> {
    if !(pt_length > 0.0) || !(real_mm > 0.0) || unit_mm(unit).is_none() {
        return None;
    }
    let mm_per_pt = real_mm / pt_length;
    Some(Cal {
        mm_per_pt,
        unit: unit.to_string(),
        label: format!("Calibrated: {} over {} in", format_length(real_mm, unit), to_fixed(pt_length / 72.0, 2)),
    })
}

/// `\d*\.?\d+` over the whole string.
fn is_decimal(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() || !b.iter().all(|c| c.is_ascii_digit() || *c == b'.') {
        return false;
    }
    let dots = b.iter().filter(|c| **c == b'.').count();
    match dots {
        0 => true,
        1 => b.last() != Some(&b'.'),
        _ => false,
    }
}

fn parse_float(s: &str) -> f64 {
    let mut t = s.to_string();
    if t.starts_with('.') {
        t.insert(0, '0');
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

/// `/^\d+$/`
fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit())
}

fn skip_ws(s: &str) -> &str {
    s.trim_start_matches(is_ws)
}

/// units.js `num(s)`: a plain number, a fraction (1/2) or a mixed number (6 1/2).
fn num(s: &str) -> f64 {
    let s = trim(s);
    if s.is_empty() {
        return f64::NAN;
    }
    // FRACTION: ^(\d+)\s*\/\s*(\d+)$
    if let Some((a, b)) = s.split_once('/') {
        let (a, b) = (a.trim_end_matches(is_ws), skip_ws(b));
        if all_digits(a) && all_digits(b) {
            let d = parse_float(b);
            return if d != 0.0 { parse_float(a) / d } else { f64::NAN };
        }
        // mixed: ^(\d+(?:\.\d+)?)\s+(\d+)\s*\/\s*(\d+)$
        let first_end = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
        let first = s.get(..first_end).unwrap_or("");
        let rest = s.get(first_end..).unwrap_or("");
        let ok_first = match first.split_once('.') {
            None => all_digits(first),
            Some((x, y)) => all_digits(x) && all_digits(y),
        };
        if ok_first && rest.starts_with(is_ws) {
            let r = skip_ws(rest);
            if let Some((p, q)) = r.split_once('/') {
                let (p, q) = (p.trim_end_matches(is_ws), skip_ws(q));
                if all_digits(p) && all_digits(q) {
                    let d = parse_float(q);
                    return if d != 0.0 { parse_float(first) + parse_float(p) / d } else { f64::NAN };
                }
            }
        }
    }
    if is_decimal(s) { parse_float(s) } else { f64::NAN }
}

fn pos(v: f64) -> Option<f64> {
    if v.is_finite() && v > 0.0 { Some(v) } else { None }
}

/// `[\d\s./]`
fn in_inch_class(c: char) -> bool {
    c.is_ascii_digit() || c == '.' || c == '/' || is_ws(c)
}

/// Split off the maximal run of `[0-9.]` at the start; Some when it is a `\d*\.?\d+` number.
fn lead_number(s: &str) -> Option<(&str, &str)> {
    let k = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    let n = s.get(..k)?;
    if is_decimal(n) { Some((n, s.get(k..).unwrap_or(""))) } else { None }
}

/// Leading whitespace run lengths in bytes at each candidate count (greedy first: max..=0), as byte offsets.
fn ws_prefix_offsets(s: &str) -> Vec<usize> {
    let mut offs = vec![0usize];
    for (i, c) in s.char_indices() {
        if !is_ws(c) {
            break;
        }
        offs.push(i.saturating_add(c.len_utf8()));
    }
    offs.reverse();
    offs
}

/// `\s*-?\s*` then `rest_ok(remainder)` in the regex engine's backtracking order; the first remainder accepted wins.
fn middle_then<T>(s: &str, mut rest_ok: impl FnMut(&str) -> Option<T>) -> Option<T> {
    for k1 in ws_prefix_offsets(s) {
        let a = s.get(k1..).unwrap_or("");
        let dash_opts: Vec<&str> = if let Some(r) = a.strip_prefix('-') { vec![r, a] } else { vec![a] };
        for b in dash_opts {
            for k2 in ws_prefix_offsets(b) {
                if let Some(v) = rest_ok(b.get(k2..).unwrap_or("")) {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// The optional inch group `(?:([\d\s./]+?)\s*")?$` on a remainder: Some(Some(capture)) when the group matches,
/// Some(None) when it is skipped and the remainder is empty, None when neither.
fn inch_group(r: &str) -> Option<Option<String>> {
    // lazy content: the shortest prefix (≥ 1 character) after which only whitespace and a final '"' remain
    let chars = r.char_indices().peekable();
    for (i, c) in chars {
        if !in_inch_class(c) {
            break;
        }
        let end = i.saturating_add(c.len_utf8());
        let rest = skip_ws(r.get(end..).unwrap_or(""));
        if rest == "\"" {
            return Some(Some(r.get(..end).unwrap_or("").to_string()));
        }
    }
    if r.is_empty() { Some(None) } else { None }
}

/// `^(?:(\d*\.?\d+)\s*')?\s*-?\s*(?:([\d\s./]+?)\s*")?$` → (feet capture, inch capture) when it matches.
fn ft_in_parts(s: &str) -> Option<(Option<String>, Option<String>)> {
    // with the feet group first (it is greedy-optional), then without it
    if let Some((n, rest)) = lead_number(s) {
        let r = skip_ws(rest);
        if let Some(after) = r.strip_prefix('\'')
            && let Some(inch) = middle_then(after, inch_group)
        {
            return Some((Some(n.to_string()), inch));
        }
    }
    middle_then(s, inch_group).map(|inch| (None, inch))
}

/// `^(\d*\.?\d+)\s*'\s*-?\s*([\d\s./]+)$` → (feet, inches).
fn ft_no_mark(s: &str) -> Option<(String, String)> {
    let (n, rest) = lead_number(s)?;
    let after = skip_ws(rest).strip_prefix('\'')?;
    middle_then(after, |r: &str| if !r.is_empty() && r.chars().all(in_inch_class) { Some(r.to_string()) } else { None })
        .map(|inch| (n.to_string(), inch))
}

/// `.replace(/\s*(words…)\b\.?/g, rep)` with JS's ASCII `\b`.
fn replace_words(s: &str, words: &[&str], rep: char) -> String {
    let is_word = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    'outer: while i < s.len() {
        let here = s.get(i..).unwrap_or("");
        let after_ws = skip_ws(here);
        let ws_len = here.len().saturating_sub(after_ws.len());
        for w in words {
            if let Some(rest) = after_ws.strip_prefix(w)
                && !is_word(rest.chars().next())
            {
                let mut end = i.saturating_add(ws_len).saturating_add(w.len());
                if rest.starts_with('.') {
                    end = end.saturating_add(1);
                }
                out.push(rep);
                i = end;
                continue 'outer;
            }
        }
        let c = here.chars().next().unwrap_or(' ');
        out.push(c);
        i = i.saturating_add(c.len_utf8());
    }
    out
}

/// `parseLength(text, unit)` → millimeters (None when it is not a length).
pub fn parse_length(text: &str, unit: &str) -> Option<f64> {
    let mut s = lower(trim(text));
    s = s.replace(['\u{2019}', '\u{2032}'], "'").replace(['\u{201d}', '\u{2033}'], "\"").replace("''", "\"").replace(',', ".");
    if s.is_empty() {
        return None;
    }
    // metric with a suffix
    if let Some((n, rest)) = lead_number(&s) {
        let u = skip_ws(rest);
        const METRIC: [&str; 15] = [
            "mm",
            "cm",
            "m",
            "meter",
            "meters",
            "metre",
            "metres",
            "millimeter",
            "millimeters",
            "millimetre",
            "millimetres",
            "centimeter",
            "centimeters",
            "centimetre",
            "centimetres",
        ];
        if METRIC.contains(&u) {
            let k = if u.starts_with("mm") || u.starts_with("milli") {
                1.0
            } else if u.starts_with('c') {
                10.0
            } else {
                1000.0
            };
            return pos(parse_float(n) * k);
        }
    }
    // feet and inches
    s = replace_words(&s, &["feet", "foot", "ft"], '\'');
    s = replace_words(&s, &["inches", "inch", "in"], '"');
    if let Some((ft_s, in_s)) = ft_in_parts(&s)
        && (ft_s.is_some() || in_s.is_some())
        && s.contains(['\'', '"'])
    {
        let ft = ft_s.as_deref().map_or(0.0, parse_float);
        let inch = in_s.as_deref().map_or(0.0, num);
        if !ft.is_finite() || !inch.is_finite() {
            return None;
        }
        return pos(ft * 304.8 + inch * 25.4);
    }
    if let Some((f, i)) = ft_no_mark(&s) {
        let inch = num(&i);
        return if inch.is_finite() { pos(parse_float(&f) * 304.8 + inch * 25.4) } else { None };
    }
    let n = num(&s);
    if !n.is_finite() {
        return None;
    }
    let k = match unit_mm(unit) {
        Some(Some(k)) => k,
        Some(None) => f64::NAN,
        None => 1.0,
    };
    pos(n * k)
}

/// `trim(v, d)` = `String(Number(v.toFixed(d)))`.
fn trim_num(v: f64, d: usize) -> String {
    ns(crate::js::str_to_number(&to_fixed(v, d)))
}

/// `formatLength(mm, unit)`.
pub fn format_length(mm: f64, unit: &str) -> String {
    if !mm.is_finite() {
        return String::new();
    }
    match unit {
        "ft-in" => {
            let mut sixteenths = round((mm / 25.4) * 16.0);
            let neg = sixteenths < 0.0;
            sixteenths = sixteenths.abs();
            let ft = (sixteenths / 192.0).floor();
            let mut rest = sixteenths - ft * 192.0;
            let inch = (rest / 16.0).floor();
            rest -= inch * 16.0;
            let mut frac = String::new();
            if rest != 0.0 {
                let (mut n, mut d) = (rest, 16.0);
                while n % 2.0 == 0.0 {
                    n /= 2.0;
                    d /= 2.0;
                }
                frac = format!(" {}/{}", ns(n), ns(d));
            }
            format!("{}{}'-{}{}\"", if neg { "-" } else { "" }, ns(ft), ns(inch), frac)
        }
        "ft" => format!("{}'", to_fixed(mm / 304.8, 2)),
        "in" => format!("{}\"", trim_num(mm / 25.4, 2)),
        "m" => t("len.m", &[("v", Fill::S(to_fixed(mm / 1000.0, 3)))]),
        "mm" => t("len.mm", &[("v", Fill::N(round(mm)))]),
        _ => t("len.mm", &[("v", Fill::S(trim_num(mm, 2)))]),
    }
}

/// `measure(ptLength, cal)`.
pub fn measure(pt_length: f64, cal: Option<&Cal>) -> String {
    match cal {
        Some(c) if c.mm_per_pt > 0.0 => format_length(pt_length * c.mm_per_pt, &c.unit),
        _ => String::new(),
    }
}

/// `formatArea(mm2, unit)`.
pub fn format_area(mm2: f64, unit: &str) -> String {
    if !mm2.is_finite() {
        return String::new();
    }
    match unit {
        "ft-in" | "ft" => t("area.sq_ft", &[("v", Fill::S(to_fixed(mm2 / 92903.04, 2)))]),
        "in" => t("area.sq_in", &[("v", Fill::S(to_fixed(mm2 / 645.16, 1)))]),
        "m" => t("area.sq_m", &[("v", Fill::S(to_fixed(mm2 / 1e6, 3)))]),
        "mm" => t("area.sq_mm", &[("v", Fill::N(round(mm2)))]),
        _ => t("area.sq_m", &[("v", Fill::S(to_fixed(mm2 / 1e6, 3)))]),
    }
}

/// `polygonAreaPerimeter(pts)` → (area, perimeter) in points² / points.
pub fn polygon_area_perimeter(pts: &[[f64; 2]]) -> (f64, f64) {
    let mut a = 0.0;
    let mut p = 0.0;
    let n = pts.len();
    for i in 0..n {
        let (Some(q), Some(r)) = (pts.get(i), pts.get(crate::js::next_i(i, n))) else { continue };
        a += q[0] * r[1] - r[0] * q[1];
        p += hypot(r[0] - q[0], r[1] - q[1]);
    }
    (a.abs() / 2.0, p)
}

/// `measureArea(pts, cal, holes)` → (area, perimeter) strings ('' with no calibration).
pub fn measure_area(pts: &[[f64; 2]], cal: Option<&Cal>, holes: &[Vec<[f64; 2]>]) -> (String, String) {
    let Some(c) = cal.filter(|c| c.mm_per_pt > 0.0) else { return (String::new(), String::new()) };
    if pts.len() < 3 {
        return (String::new(), String::new());
    }
    let (mut area, mut perimeter) = polygon_area_perimeter(pts);
    for h in holes {
        let (ha, hp) = polygon_area_perimeter(h);
        area -= ha;
        perimeter += hp;
    }
    area = crate::js::max(0.0, area);
    (format_area(area * c.mm_per_pt * c.mm_per_pt, &c.unit), format_length(perimeter * c.mm_per_pt, &c.unit))
}

/// `validCalibration(c)` for a typed calibration.
pub fn valid_calibration(c: &Cal) -> bool {
    c.mm_per_pt.is_finite() && c.mm_per_pt > 0.0 && c.mm_per_pt < 1e7 && unit_mm(&c.unit).is_some()
}

/// `validCalibration(c)` for parsed JSON (mmPerPt must be a number, unit a known key).
pub fn valid_calibration_json(v: &serde_json::Value) -> bool {
    let Some(o) = v.as_object() else { return false };
    let mm = crate::js::finite(o.get("mmPerPt"));
    let unit_ok = match o.get("unit") {
        Some(u) => unit_mm(&crate::js::to_js_string(u)).is_some(),
        None => unit_mm("undefined").is_some(),
    };
    mm.is_some_and(|m| m > 0.0 && m < 1e7) && unit_ok
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ft(s: &str) -> Option<f64> {
        parse_length(s, "ft-in").map(|v| crate::js::str_to_number(&to_fixed(v / 304.8, 6)))
    }
    #[test]
    fn lengths() {
        assert_eq!(ft("24'-0\""), Some(24.0));
        assert_eq!(ft("12' 6 1/2\""), Some(crate::js::str_to_number(&to_fixed(12.0 + 6.5 / 12.0, 6))));
        assert_eq!(ft("6\""), Some(0.5));
        assert_eq!(ft("1/2\""), Some(crate::js::str_to_number(&to_fixed(0.5 / 12.0, 6))));
        assert_eq!(ft("12.5'"), Some(12.5));
        assert_eq!(ft("12.5 ft"), Some(12.5));
        assert_eq!(ft("3 ft 6 in"), Some(3.5));
        assert_eq!(ft("12'6"), Some(12.5));
        assert_eq!(parse_length("3.5 m", "ft-in"), Some(3500.0));
        assert_eq!(parse_length("900 mm", "ft-in"), Some(900.0));
        assert_eq!(parse_length("90 cm", "ft-in"), Some(900.0));
        assert_eq!(parse_length("16", "ft-in"), Some(16.0 * 304.8));
        assert_eq!(parse_length("16", "m"), Some(16000.0));
        for bad in ["abc", "", "-3", "0"] {
            assert_eq!(parse_length(bad, "ft-in"), None, "{bad}");
        }
        assert_eq!(format_length(304.8 * 16.0, "ft-in"), "16'-0\"");
        assert_eq!(format_length(304.8 * 12.0 + 25.4 * 6.5, "ft-in"), "12'-6 1/2\"");
        assert_eq!(format_length(3500.0, "m"), "3.500 m");
        assert_eq!(format_length(900.4, "mm"), "900 mm");
        let quarter = preset_calibration("1/4\" = 1'-0\"", 48.0, "ft-in");
        assert_eq!(measure(288.0, Some(&quarter)), "16'-0\"");
        assert_eq!(measure(72.0, Some(&preset_calibration("1:100", 100.0, "mm"))), "2.540 m");
        let cal = calibration_from(288.0, 304.8 * 16.0, "ft-in");
        assert!(cal.as_ref().is_some_and(|c| (c.mm_per_pt - quarter.mm_per_pt).abs() < 1e-9 && valid_calibration(c)));
        assert!(!valid_calibration(&Cal { mm_per_pt: -1.0, unit: "ft".into(), label: String::new() }));
        assert!(!valid_calibration(&Cal { mm_per_pt: 1.0, unit: "parsec".into(), label: String::new() }));
    }
    #[test]
    fn areas() {
        let quarter = preset_calibration("1/4\" = 1'-0\"", 48.0, "ft-in");
        let v = measure_area(&[[100.0, 100.0], [388.0, 100.0], [388.0, 244.0], [100.0, 244.0]], Some(&quarter), &[]);
        assert_eq!(v, ("128.00 sq ft".to_string(), "48'-0\"".to_string()));
        let m = preset_calibration("1:100", 100.0, "mm");
        assert_eq!(measure_area(&[[0.0, 0.0], [72.0, 0.0], [0.0, 72.0]], Some(&m), &[]).0, "3.226 sq m");
        assert_eq!(format_area(92903.04 * 2.5, "ft"), "2.50 sq ft");
        assert_eq!(format_area(645.16 * 3.0, "in"), "3.0 sq in");
    }
}
