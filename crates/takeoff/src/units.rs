//! `units.js` — lengths, scales and money. The canonical length is the FOOT (areas in square feet); PDF user space is
//! 72 units per inch (× /UserUnit), so a sheet's scale is `ftPerUnit`. Unit rates are integers in 1/10000 $, extended
//! amounts integer cents, waste integer basis points.

use crate::i18n::{t, tf};
use crate::jv as jsm;

pub const M_PER_FT: f64 = 0.3048;
pub const SQM_PER_SQFT: f64 = 0.09290304;

/// `{ ok: true, ft } | { ok: false, error }`.
pub type LenResult = Result<f64, String>;

fn is_az(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_ascii_lowercase())
}

/// `s.replace(/(?<![a-z])(w1|w2|w3)(?![a-z])\.?/g, to)` for the WORDS table.
fn replace_words(s: &str, words: &[&str], to: char) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < chars.len() {
        let prev = if i == 0 { None } else { chars.get(i.wrapping_sub(1)).copied() };
        let mut hit = None;
        if !is_az(prev) {
            for w in words {
                let wl = w.chars().count();
                let end = i.saturating_add(wl);
                if end <= chars.len()
                    && chars.get(i..end).is_some_and(|sl| sl.iter().copied().eq(w.chars()))
                    && !is_az(chars.get(end).copied())
                {
                    hit = Some(end);
                    break;
                }
            }
        }
        match hit {
            Some(mut end) => {
                if chars.get(end) == Some(&'.') {
                    end = end.saturating_add(1);
                }
                out.push(to);
                i = end;
            }
            None => {
                if let Some(c) = chars.get(i) {
                    out.push(*c);
                }
                i = i.saturating_add(1);
            }
        }
    }
    out
}

/// Match `^(\d+(?:\.\d*)?|\.\d+)` at the start of `b` → its byte length.
pub(crate) fn num_lit(b: &[u8]) -> Option<usize> {
    let mut i = 0usize;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i = i.saturating_add(1);
    }
    if i > 0 {
        if b.get(i) == Some(&b'.') {
            i = i.saturating_add(1);
            while b.get(i).is_some_and(u8::is_ascii_digit) {
                i = i.saturating_add(1);
            }
        }
        return Some(i);
    }
    if b.first() == Some(&b'.') && b.get(1).is_some_and(u8::is_ascii_digit) {
        i = 1;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i = i.saturating_add(1);
        }
        return Some(i);
    }
    None
}
/// Match `^(\d+(?:\.\d+)?|\.\d+)` (no bare trailing dot).
pub(crate) fn num_lit_strict(b: &[u8]) -> Option<usize> {
    let mut i = 0usize;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i = i.saturating_add(1);
    }
    if i > 0 {
        if b.get(i) == Some(&b'.') && b.get(i.saturating_add(1)).is_some_and(u8::is_ascii_digit) {
            i = i.saturating_add(1);
            while b.get(i).is_some_and(u8::is_ascii_digit) {
                i = i.saturating_add(1);
            }
        }
        return Some(i);
    }
    if b.first() == Some(&b'.') && b.get(1).is_some_and(u8::is_ascii_digit) {
        i = 1;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i = i.saturating_add(1);
        }
        return Some(i);
    }
    None
}

/// Parse a typed distance into feet (`parseLength(text, { defaultUnit })`).
pub fn parse_length(text: &str, default_unit: &str) -> LenResult {
    let s0 = jsm::lower(cc_m3d::js::trim(text));
    if s0.is_empty() {
        return Err(t("len.err.empty"));
    }
    let mut s: String = s0
        .chars()
        .map(|c| match c {
            '′' | '’' | '‘' | '`' => '\'',
            '″' | '“' | '”' => '"',
            c => c,
        })
        .collect();
    s = s.replace("''", "\"");
    s = s.chars().map(|c| if matches!(c, '−' | '–' | '—') { '-' } else { c }).filter(|c| *c != ',').collect();
    s = replace_words(&s, &["feet", "foot", "ft"], '\'');
    s = replace_words(&s, &["inches", "inch", "in"], '"');
    let s = collapse_ws(&s);
    if s.starts_with('-') {
        return Err(t("len.err.positive"));
    }
    // metric: one number and a unit
    let b = s.as_bytes();
    if let Some(nl) = num_lit(b) {
        let mut j = nl;
        while b.get(j) == Some(&b' ') {
            j = j.saturating_add(1);
        }
        let unit = s.get(j..).unwrap_or("");
        if unit == "mm" || unit == "cm" || unit == "m" {
            let v = crate::jv::parse_float(s.get(..nl).unwrap_or(""));
            let meters = if unit == "mm" {
                v / 1000.0
            } else if unit == "cm" {
                v / 100.0
            } else {
                v
            };
            return positive(meters / M_PER_FT);
        }
    }
    if s.chars().any(|c| c.is_ascii_lowercase()) {
        return Err(t("len.err.units"));
    }
    let Some(tokens) = tokenize(&s) else { return Err(t("len.err.read")) };
    let ft = imperial(&tokens, default_unit)?;
    positive(ft)
}

/// `s.replace(/\s+/g, ' ').trim()`.
pub(crate) fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut sp = false;
    for c in s.chars() {
        if jsm::is_space(c) {
            if !sp {
                out.push(' ');
            }
            sp = true;
        } else {
            out.push(c);
            sp = false;
        }
    }
    cc_m3d::js::trim(&out).to_string()
}

fn positive(ft: f64) -> LenResult {
    if !ft.is_finite() {
        return Err(t("len.err.read.short"));
    }
    if ft <= 0.0 {
        return Err(t("len.err.positive"));
    }
    Ok(ft)
}

#[derive(Debug, Clone, Copy)]
enum Tok {
    Foot,
    Inch,
    Frac(f64),
    Num(f64, bool),
}

fn tokenize(s: &str) -> Option<Vec<Tok>> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        let c = b.get(i).copied().unwrap_or(0);
        if c == b' ' || c == b'-' {
            i = i.saturating_add(1);
            continue;
        }
        if c == b'\'' {
            out.push(Tok::Foot);
            i = i.saturating_add(1);
            continue;
        }
        if c == b'"' {
            out.push(Tok::Inch);
            i = i.saturating_add(1);
            continue;
        }
        let rest = b.get(i..).unwrap_or(&[]);
        let n1 = num_lit(rest)?;
        let a = s.get(i..i.saturating_add(n1)).unwrap_or("");
        // (?:\s*\/\s*(num))?
        let mut j = n1;
        while rest.get(j) == Some(&b' ') {
            j = j.saturating_add(1);
        }
        let mut frac = None;
        if rest.get(j) == Some(&b'/') {
            let mut k = j.saturating_add(1);
            while rest.get(k) == Some(&b' ') {
                k = k.saturating_add(1);
            }
            if let Some(n2) = num_lit(rest.get(k..).unwrap_or(&[])) {
                frac = Some((k, n2));
            }
        }
        match frac {
            Some((k, n2)) => {
                let den_s = s.get(i.saturating_add(k)..i.saturating_add(k).saturating_add(n2)).unwrap_or("");
                if a.contains('.') || den_s.contains('.') {
                    return None;
                }
                let den = crate::jv::parse_int(den_s);
                if den == 0.0 {
                    return None;
                }
                out.push(Tok::Frac(crate::jv::parse_int(a) / den));
                i = i.saturating_add(k).saturating_add(n2);
            }
            None => {
                out.push(Tok::Num(crate::jv::parse_float(a), !a.contains('.')));
                i = i.saturating_add(n1);
            }
        }
    }
    Some(out)
}

fn imperial(tk: &[Tok], default_unit: &str) -> LenResult {
    let mut i = 0usize;
    let bad = || Err(t("len.err.read"));
    let value = |i: &mut usize| -> Option<f64> {
        let a = tk.get(*i)?;
        match *a {
            Tok::Frac(v) => {
                *i = i.saturating_add(1);
                Some(v)
            }
            Tok::Num(v, is_int) => {
                *i = i.saturating_add(1);
                if let Some(Tok::Frac(f)) = tk.get(*i) {
                    if !is_int {
                        return None;
                    }
                    *i = i.saturating_add(1);
                    return Some(v + f);
                }
                Some(v)
            }
            _ => None,
        }
    };
    let Some(first) = value(&mut i) else { return bad() };
    if matches!(tk.get(i), Some(Tok::Foot)) {
        i = i.saturating_add(1);
        let mut inches = 0.0;
        if i < tk.len() {
            let Some(v) = value(&mut i) else { return bad() };
            inches = v;
            if matches!(tk.get(i), Some(Tok::Inch)) {
                i = i.saturating_add(1);
            }
        }
        if i != tk.len() {
            return bad();
        }
        return Ok(first + inches / 12.0);
    }
    if matches!(tk.get(i), Some(Tok::Inch)) {
        i = i.saturating_add(1);
        if i != tk.len() {
            return bad();
        }
        return Ok(first / 12.0);
    }
    if i != tk.len() {
        return bad();
    }
    to_ft(first, default_unit)
}

/// `toFt(v, unit)`. Errors on an unknown unit (the JS throws: a programming error).
pub fn to_ft(v: f64, unit: &str) -> LenResult {
    match unit {
        "ft" => Ok(v),
        "in" => Ok(v / 12.0),
        "m" => Ok(v / M_PER_FT),
        "cm" => Ok(v / 100.0 / M_PER_FT),
        "mm" => Ok(v / 1000.0 / M_PER_FT),
        _ => Err(format!("Unknown unit {unit}")),
    }
}

/// Feet-inches to the nearest 1/denom inch.
pub fn format_ft_in(ft: f64, denom: f64) -> String {
    if !ft.is_finite() {
        return String::new();
    }
    let neg = ft < 0.0;
    let n = cc_m3d::js::round(ft.abs() * 12.0 * denom + 1e-9);
    let per_foot = 12.0 * denom;
    let feet = (n / per_foot).floor();
    let rem = n - feet * per_foot;
    let inch = (rem / denom).floor();
    let mut num = rem - inch * denom;
    let mut den = denom;
    while num != 0.0 && num % 2.0 == 0.0 && den % 2.0 == 0.0 {
        num /= 2.0;
        den /= 2.0;
    }
    let zero = feet == 0.0 && inch == 0.0 && num == 0.0;
    let ns = crate::jv::num_str;
    let mut out = String::new();
    if neg && !zero {
        out.push('-');
    }
    out.push_str(&ns(feet));
    out.push_str("'-");
    out.push_str(&ns(inch));
    if num != 0.0 {
        out.push(' ');
        out.push_str(&ns(num));
        out.push('/');
        out.push_str(&ns(den));
    }
    out.push('"');
    out
}
/// `formatFtIn(ft)` (1/16").
pub fn ft_in(ft: f64) -> String {
    format_ft_in(ft, 16.0)
}

/// A length for display in a chosen unit.
pub fn format_length(ft: f64, unit: &str, digits: i32) -> String {
    if !ft.is_finite() {
        return String::new();
    }
    match unit {
        "ftin" => ft_in(ft),
        "ft" => fixed(ft, digits) + "'",
        "in" => fixed(ft * 12.0, digits) + "\"",
        "m" => fixed(ft * M_PER_FT, 3) + " m",
        "mm" => fixed(ft * M_PER_FT * 1000.0, 0) + " mm",
        _ => fixed(ft, digits),
    }
}

/// Commas between groups of three in a run of digits: `s.replace(/\B(?=(\d{3})+(?!\d))/g, ',')`.
pub fn group3(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let word = |c: Option<&char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_');
    let mut out = String::with_capacity(s.len().saturating_add(8));
    for (p, c) in chars.iter().enumerate() {
        if p > 0 {
            let before = word(chars.get(p.wrapping_sub(1)));
            let after = word(chars.get(p));
            if before == after {
                // the run of digits starting at p
                let run = chars.iter().skip(p).take_while(|c| c.is_ascii_digit()).count();
                if run >= 3 && run % 3 == 0 {
                    out.push(',');
                }
            }
        }
        out.push(*c);
    }
    out
}

/// Grouped fixed-point number: 1234.5 → "1,234.50".
pub fn fixed(v: f64, digits: i32) -> String {
    if !v.is_finite() {
        return String::new();
    }
    let r = round_to(v, digits);
    let f = usize::try_from(digits.max(0)).unwrap_or(0);
    let txt = cc_m3d::js::to_fixed(r.abs(), f);
    let mut parts = txt.splitn(2, '.');
    let i = parts.next().unwrap_or("");
    let fr = parts.next().unwrap_or("");
    let mut out = String::new();
    if r < 0.0 {
        out.push('-');
    }
    out.push_str(&group3(i));
    if !fr.is_empty() {
        out.push('.');
        out.push_str(fr);
    }
    out
}

/// Half away from zero, tolerant of binary noise.
pub fn round_to(v: f64, digits: i32) -> f64 {
    let f = crate::jsmath::pow(10.0, f64::from(digits));
    let y = v.abs() * f;
    let r = (y + 0.5 + 1e-9 * cc_m3d::js::max(1.0, y)).floor();
    (if v < 0.0 { -r } else { r }) / f
}

/// An item unit: what a quantity is reported in.
#[derive(Debug, Clone, Copy)]
pub struct Unit {
    pub id: &'static str,
    pub kind: &'static str,
    pub label: &'static str,
    pub name_key: &'static str,
    pub per_ft: f64,
    pub per_sqft: f64,
    pub system: &'static str,
}
pub const UNITS: [Unit; 6] = [
    Unit { id: "LF", kind: "linear", label: "LF", name_key: "unit.lf.name", per_ft: 1.0, per_sqft: f64::NAN, system: "imperial" },
    Unit { id: "m", kind: "linear", label: "m", name_key: "unit.m.name", per_ft: M_PER_FT, per_sqft: f64::NAN, system: "metric" },
    Unit { id: "SF", kind: "area", label: "SF", name_key: "unit.sf.name", per_ft: f64::NAN, per_sqft: 1.0, system: "imperial" },
    Unit { id: "SY", kind: "area", label: "SY", name_key: "unit.sy.name", per_ft: f64::NAN, per_sqft: 1.0 / 9.0, system: "imperial" },
    Unit { id: "m²", kind: "area", label: "m²", name_key: "unit.m2.name", per_ft: f64::NAN, per_sqft: SQM_PER_SQFT, system: "metric" },
    Unit { id: "EA", kind: "count", label: "EA", name_key: "unit.ea.name", per_ft: f64::NAN, per_sqft: f64::NAN, system: "any" },
];
/// `UNITS[id]`.
pub fn unit(id: &str) -> Option<&'static Unit> {
    UNITS.iter().find(|u| u.id == id)
}
/// `UNITS[id].label` (the id itself when unknown: never reached for a normalised item).
pub fn unit_label(id: &str) -> String {
    unit(id).map_or_else(|| id.to_string(), |u| u.label.to_string())
}
/// `UNITS_FOR[type]`.
pub fn units_for(ty: &str) -> &'static [&'static str] {
    match ty {
        "linear" => &["LF", "m"],
        "area" => &["SF", "SY", "m²"],
        "count" => &["EA"],
        _ => &[],
    }
}
/// `DEFAULT_UNIT[type]`.
pub fn default_unit(ty: &str) -> &'static str {
    match ty {
        "area" => "SF",
        "count" => "EA",
        _ => "LF",
    }
}
/// The length unit that goes with an item unit: LF/SF/SY → ft, m/m² → m.
pub fn length_unit_of(u: &str) -> &'static str {
    if unit(u).is_some_and(|x| x.system == "metric") { "m" } else { "ft" }
}
/// The area unit that goes with an item unit.
pub fn area_unit_of(u: &str) -> &'static str {
    if unit(u).is_some_and(|x| x.system == "metric") { "m²" } else { "SF" }
}
/// `ftTo(ft, lu)`.
pub fn ft_to(ft: f64, lu: &str) -> f64 {
    if lu == "m" { ft * M_PER_FT } else { ft }
}
/// `sqftTo(sf, au)`.
pub fn sqft_to(sf: f64, au: &str) -> f64 {
    let k = unit(au).map(|u| u.per_sqft).filter(|k| !k.is_nan()).unwrap_or(1.0);
    sf * k
}

/// A preset drawing scale. ratio = real inches per paper inch.
#[derive(Debug, Clone, Copy)]
pub struct Preset {
    pub key: &'static str,
    pub label: &'static str,
    pub ratio: f64,
    pub group: &'static str,
}
pub const PRESETS: [Preset; 30] = [
    Preset { key: "a1/16", label: "1/16\" = 1'-0\"", ratio: 192.0, group: "Architectural" },
    Preset { key: "a3/32", label: "3/32\" = 1'-0\"", ratio: 128.0, group: "Architectural" },
    Preset { key: "a1/8", label: "1/8\" = 1'-0\"", ratio: 96.0, group: "Architectural" },
    Preset { key: "a3/16", label: "3/16\" = 1'-0\"", ratio: 64.0, group: "Architectural" },
    Preset { key: "a1/4", label: "1/4\" = 1'-0\"", ratio: 48.0, group: "Architectural" },
    Preset { key: "a3/8", label: "3/8\" = 1'-0\"", ratio: 32.0, group: "Architectural" },
    Preset { key: "a1/2", label: "1/2\" = 1'-0\"", ratio: 24.0, group: "Architectural" },
    Preset { key: "a3/4", label: "3/4\" = 1'-0\"", ratio: 16.0, group: "Architectural" },
    Preset { key: "a1", label: "1\" = 1'-0\"", ratio: 12.0, group: "Architectural" },
    Preset { key: "a1-1/2", label: "1 1/2\" = 1'-0\"", ratio: 8.0, group: "Architectural" },
    Preset { key: "a3", label: "3\" = 1'-0\"", ratio: 4.0, group: "Architectural" },
    Preset { key: "e10", label: "1\" = 10'", ratio: 120.0, group: "Engineering" },
    Preset { key: "e20", label: "1\" = 20'", ratio: 240.0, group: "Engineering" },
    Preset { key: "e30", label: "1\" = 30'", ratio: 360.0, group: "Engineering" },
    Preset { key: "e40", label: "1\" = 40'", ratio: 480.0, group: "Engineering" },
    Preset { key: "e50", label: "1\" = 50'", ratio: 600.0, group: "Engineering" },
    Preset { key: "e60", label: "1\" = 60'", ratio: 720.0, group: "Engineering" },
    Preset { key: "e100", label: "1\" = 100'", ratio: 1200.0, group: "Engineering" },
    Preset { key: "m1", label: "1:1", ratio: 1.0, group: "Metric" },
    Preset { key: "m2", label: "1:2", ratio: 2.0, group: "Metric" },
    Preset { key: "m5", label: "1:5", ratio: 5.0, group: "Metric" },
    Preset { key: "m10", label: "1:10", ratio: 10.0, group: "Metric" },
    Preset { key: "m20", label: "1:20", ratio: 20.0, group: "Metric" },
    Preset { key: "m25", label: "1:25", ratio: 25.0, group: "Metric" },
    Preset { key: "m50", label: "1:50", ratio: 50.0, group: "Metric" },
    Preset { key: "m100", label: "1:100", ratio: 100.0, group: "Metric" },
    Preset { key: "m200", label: "1:200", ratio: 200.0, group: "Metric" },
    Preset { key: "m250", label: "1:250", ratio: 250.0, group: "Metric" },
    Preset { key: "m500", label: "1:500", ratio: 500.0, group: "Metric" },
    Preset { key: "m1000", label: "1:1000", ratio: 1000.0, group: "Metric" },
];
/// `PRESETS.get(key)`.
pub fn preset(key: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.key == key)
}

/// Real feet per PDF user-space unit for a preset.
///
/// # Errors
/// "Unknown scale …" for a key that is not a preset (the JS throws).
pub fn preset_ft_per_unit(key: &str, user_unit: f64) -> Result<f64, String> {
    let p = preset(key).ok_or_else(|| format!("Unknown scale {key}"))?;
    Ok((user_unit / 72.0) * p.ratio / 12.0)
}

/// Two picked points and the real distance → `{ ftPerUnit, units }`.
pub fn calibrate(ax: f64, ay: f64, bx: f64, by: f64, real_ft: f64) -> Result<(f64, f64), String> {
    let d = cc_m3d::js::hypot2(bx - ax, by - ay);
    if !(d > 0.5) {
        return Err(t("cal.err.near"));
    }
    if !(real_ft > 0.0) || !real_ft.is_finite() {
        return Err(t("len.err.positive"));
    }
    Ok((real_ft / d, d))
}

/// The preset a scale matches within tol (relative), with its error.
pub fn match_preset(ft_per_unit: f64, user_unit: f64, tol: f64) -> Option<(&'static Preset, f64)> {
    let mut best: Option<(&'static Preset, f64)> = None;
    for p in &PRESETS {
        let s = (user_unit / 72.0) * p.ratio / 12.0;
        let err = (ft_per_unit - s).abs() / s;
        if err <= tol && best.is_none_or(|(_, e)| err < e) {
            best = Some((p, err));
        }
    }
    best
}

/// `"1" = 4'"` style ratio for any scale.
pub fn describe_scale(ft_per_unit: f64, user_unit: f64) -> String {
    let ft_per_paper_inch = ft_per_unit * 72.0 / user_unit;
    let f = fixed(ft_per_paper_inch, 3);
    // .replace(/\.?0+$/, '')
    let trimmed = strip_zeros(&f);
    format!("1\" = {trimmed}'")
}
/// `s.replace(/\.?0+$/, '')`.
pub fn strip_zeros(s: &str) -> String {
    let t = s.trim_end_matches('0');
    if t.len() == s.len() {
        return s.to_string();
    }
    t.strip_suffix('.').unwrap_or(t).to_string()
}

fn only_digits_dot(s: &str) -> bool {
    // /^\d*(\.\d*)?$/ and not '.'
    let mut dot = false;
    for c in s.chars() {
        if c == '.' {
            if dot {
                return false;
            }
            dot = true;
        } else if !c.is_ascii_digit() {
            return false;
        }
    }
    s != "."
}

/// `"$1,234.5"` → rate4.
pub fn parse_rate(text: &str) -> Result<f64, String> {
    let s: String = cc_m3d::js::trim(text).chars().filter(|c| !(*c == '$' || *c == ',' || jsm::is_space(*c))).collect();
    if s.is_empty() {
        return Ok(0.0);
    }
    if !only_digits_dot(&s) {
        return Err(t("rate.err.read"));
    }
    let v = crate::jv::str_to_number(&s);
    if !v.is_finite() || v > 1e9 {
        return Err(t("rate.err.big"));
    }
    Ok(cc_m3d::js::round(round_to(v, 4) * 10000.0))
}
/// `"7.5"` or `"7.5%"` → basis points.
pub fn parse_percent(text: &str) -> Result<f64, String> {
    let s: String = cc_m3d::js::trim(text).chars().filter(|c| !(*c == '%' || jsm::is_space(*c))).collect();
    if s.is_empty() {
        return Ok(0.0);
    }
    if !only_digits_dot(&s) {
        return Err(t("pct.err.read"));
    }
    let v = crate::jv::str_to_number(&s);
    if v > 1000.0 {
        return Err(t("pct.err.big"));
    }
    Ok(cc_m3d::js::round(round_to(v, 2) * 100.0))
}
/// Integer cents → "$1,234.56".
pub fn format_cents(c: f64) -> String {
    let neg = c < 0.0;
    let a = cc_m3d::js::round(c).abs();
    let whole = crate::jv::num_str((a / 100.0).floor());
    let cents = crate::jv::num_str(a % 100.0);
    let cents = if jsm::len16(&cents) < 2 { format!("{}{cents}", "0".repeat(2usize.saturating_sub(jsm::len16(&cents)))) } else { cents };
    format!("{}${}.{}", if neg { "-" } else { "" }, group3(&whole), cents)
}
/// `s.replace(/(\.\d\d\d*?)0+$/, '$1')`.
fn trim_rate_zeros(s: &str) -> String {
    let Some(dot) = s.find('.') else { return s.to_string() };
    let frac = s.get(dot.saturating_add(1)..).unwrap_or("");
    if frac.len() < 3 || !frac.bytes().all(|b| b.is_ascii_digit()) {
        // the regex needs \.\d\d then digits up to the end; with a non-digit in the fraction it cannot match at the end
        if !frac.bytes().all(|b| b.is_ascii_digit()) {
            return s.to_string();
        }
        return s.to_string();
    }
    let keep = frac.trim_end_matches('0');
    let keep_len = keep.len().max(2);
    if keep_len >= frac.len() {
        return s.to_string();
    }
    format!("{}{}", s.get(..=dot).unwrap_or(""), frac.get(..keep_len).unwrap_or(""))
}
/// A unit rate (1/10000 $) → "$3.85", "$0.385", "$0.3851".
pub fn format_rate(rate4: f64) -> String {
    let s = trim_rate_zeros(&cc_m3d::js::to_fixed(rate4 / 10000.0, 4));
    // '$' + s.replace(/^(\d+)/, grouped)
    let digits = s.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return format!("${s}");
    }
    format!("${}{}", group3(s.get(..digits).unwrap_or("")), s.get(digits..).unwrap_or(""))
}
/// `rateInput(rate4)`.
pub fn rate_input(rate4: f64) -> String {
    trim_rate_zeros(&cc_m3d::js::to_fixed(rate4 / 10000.0, 4))
}
/// `percentInput(bp)`.
pub fn percent_input(bp: f64) -> String {
    crate::jv::num_str(bp / 100.0)
}
/// `formatPercent(bp)`.
pub fn format_percent(bp: f64) -> String {
    crate::jv::num_str(bp / 100.0) + "%"
}

/// Extended amount in cents: quantity × unit rate, rounded half away from zero to the cent.
pub fn extend_cents(qty: f64, rate4: f64) -> f64 {
    cc_m3d::js::round(round_to(qty * rate4 / 100.0, 0))
}
/// Quantity with waste.
pub fn with_waste(qty: f64, bp: f64) -> f64 {
    qty * (1.0 + bp / 10000.0)
}

pub const MAX_PITCH: f64 = 144.0;

/// Read a pitch → rise per 12, or an error key.
pub fn parse_pitch(text: &str) -> Result<f64, String> {
    let s = jsm::lower(cc_m3d::js::trim(text)).replace(',', ".");
    if s.is_empty() {
        return Ok(0.0);
    }
    let b = s.as_bytes();
    if let Some(nl) = num_lit_strict(b) {
        // ^num \s* (°|deg|degrees?) $
        let rest = s.get(nl..).unwrap_or("");
        let r2 = rest.trim_start_matches(jsm::is_space);
        if matches!(r2, "°" | "deg" | "degree" | "degrees") {
            let a = crate::jv::parse_float(s.get(..nl).unwrap_or(""));
            if !(0.0..85.0).contains(&a) {
                return Err("pitch.err.angle".into());
            }
            return Ok(libm::tan(a * std::f64::consts::PI / 180.0) * 12.0);
        }
        // ^num (?:\s*(?::|\/|in)\s*(\d+(?:\.\d+)?))? $
        let rise = crate::jv::parse_float(s.get(..nl).unwrap_or(""));
        let run = if rest.is_empty() {
            Some(12.0)
        } else {
            let r = rest.trim_start_matches(jsm::is_space);
            let after = r.strip_prefix(':').or_else(|| r.strip_prefix('/')).or_else(|| r.strip_prefix("in"));
            match after {
                Some(a) => {
                    let a = a.trim_start_matches(jsm::is_space);
                    let ab = a.as_bytes();
                    // \d+(?:\.\d+)?
                    let mut k = 0usize;
                    while ab.get(k).is_some_and(u8::is_ascii_digit) {
                        k = k.saturating_add(1);
                    }
                    if k > 0 && ab.get(k) == Some(&b'.') && ab.get(k.saturating_add(1)).is_some_and(u8::is_ascii_digit) {
                        k = k.saturating_add(1);
                        while ab.get(k).is_some_and(u8::is_ascii_digit) {
                            k = k.saturating_add(1);
                        }
                    }
                    if k > 0 && k == ab.len() { Some(crate::jv::parse_float(a)) } else { None }
                }
                None => None,
            }
        };
        let Some(run) = run else { return Err("pitch.err.read".into()) };
        if !(run > 0.0) {
            return Err("pitch.err.read".into());
        }
        let r12 = rise / run * 12.0;
        if !(r12 <= MAX_PITCH) {
            return Err("pitch.err.steep".into());
        }
        return Ok(r12);
    }
    Err("pitch.err.read".into())
}
/// `"6:12"`, `"6.5:12"`, `""` for flat.
pub fn format_pitch(rise: f64) -> String {
    if rise > 0.0 { crate::jv::num_str(cc_m3d::js::round(rise * 100.0) / 100.0) + ":12" } else { String::new() }
}
/// Surface length or area per plan unit at this pitch.
pub fn slope_factor(rise: f64) -> f64 {
    (1.0 + crate::jsmath::pow(rise / 12.0, 2.0)).sqrt()
}
/// Hip / valley true length per plan length.
pub fn hip_factor(rise: f64) -> f64 {
    (1.0 + crate::jsmath::pow(rise / 12.0, 2.0) / 2.0).sqrt()
}

/// The words for an item type ("Linear"…).
pub fn type_label(ty: &str) -> String {
    t(&format!("type.{ty}"))
}
/// `tf` re-exported for the other modules' convenience.
pub fn words(key: &str, vars: &[(&str, crate::i18n::F)]) -> String {
    tf(key, vars)
}
