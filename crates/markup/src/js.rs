//! JavaScript semantics MarkupCraft's modules lean on: number text (`String(x)`, `toFixed`), `Math.round` /
//! `Math.hypot`, JS whitespace and `trim`, UTF-16 `slice`, ToNumber / ToString of parsed JSON, and the `Date` pieces
//! (ISO text in and out, `Date.UTC`, the English short date `toLocaleDateString` gives). The number and string helpers
//! are re-exported from `cc-cad::js` / `cc-m3d::js` (ported and checked there); the dates are here.

pub use cc_cad::js::{num_to_string, str_to_number, to_js_string, to_number, to_number_opt, truthy};
pub use cc_m3d::js::{hypot2, is_ws, max, min, radix36, round, sign, slice16, to_fixed, trim};
pub use cc_m3d::jsmath::JsMath;

use serde_json::Value;

/// `String(x)` for a number.
#[inline]
pub fn ns(x: f64) -> String {
    num_to_string(x)
}

/// `Math.hypot(a, b)`.
#[inline]
pub fn hypot(a: f64, b: f64) -> f64 {
    hypot2(a, b)
}

/// `s.trimEnd()`.
pub fn trim_end(s: &str) -> &str {
    s.trim_end_matches(is_ws)
}
/// `s.trimStart()`.
pub fn trim_start(s: &str) -> &str {
    s.trim_start_matches(is_ws)
}

/// `s.length` in UTF-16 code units.
pub fn len16(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// `s.toUpperCase()` (Unicode full case mapping, as V8 does it).
pub fn upper(s: &str) -> String {
    s.to_uppercase()
}
/// `s.toLowerCase()`.
pub fn lower(s: &str) -> String {
    s.to_lowercase()
}

/// `Number.isFinite(v)` for a JSON value (a number, not a string).
pub fn finite(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|x| x.is_finite())
}

/// `typeof v === 'string'` → the string.
pub fn as_str(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str)
}

/// `String(v ?? '')`: '' for absent / null, else ToString.
pub fn str_or_empty(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(x) => to_js_string(x),
    }
}

/// `v || d` for a string-ish value: the value's text when truthy, else `d`.
pub fn or_str(v: Option<&Value>, d: &str) -> String {
    if truthy(v) { v.map(to_js_string).unwrap_or_default() } else { d.to_string() }
}

/// Drop the characters of a class (`/[...]/g` replace with ''), keeping the rest.
pub fn drop_chars(s: &str, f: impl Fn(char) -> bool) -> String {
    s.chars().filter(|c| !f(*c)).collect()
}

/// `\u0000-\u001f`
#[inline]
pub fn is_c0(c: char) -> bool {
    (c as u32) < 0x20
}
/// `\u0000-\u0008\u000b\u000c\u000e-\u001f` (control characters other than tab, LF, CR)
#[inline]
pub fn is_ctl_keep_ws(c: char) -> bool {
    let u = c as u32;
    u < 0x20 && u != 0x09 && u != 0x0a && u != 0x0d
}

/// `/^[A-Za-z0-9._-]{1,max}$/.test(s)`
pub fn is_id(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

/// `/^#[0-9a-f]{6}$/i.test(s)`
pub fn is_hex_color(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 7 && b.first() == Some(&b'#') && b.iter().skip(1).all(u8::is_ascii_hexdigit)
}

// ------------------------------------------------------------------------------------------------- dates

const MS_PER_DAY: f64 = 86_400_000.0;

fn floor_div(a: f64, b: f64) -> f64 {
    (a / b).floor()
}

/// Days from 1970-01-01 to the first of a month (proleptic Gregorian; `m` 0-based, may be out of 0..11).
fn days_from_civil(y: f64, m: f64) -> f64 {
    // MakeDay: normalise the month into the year first
    let ym = y + floor_div(m, 12.0);
    let mn = m - floor_div(m, 12.0) * 12.0;
    let y2 = if mn < 2.0 { ym - 1.0 } else { ym };
    let era = floor_div(y2, 400.0);
    let yoe = y2 - era * 400.0;
    let mp = (mn + 10.0) % 12.0;
    let doy = floor_div(153.0 * mp + 2.0, 5.0);
    let doe = yoe * 365.0 + floor_div(yoe, 4.0) - floor_div(yoe, 100.0) + doy;
    era * 146_097.0 + doe - 719_468.0
}

/// `Date.UTC(y, m, d, h, mi, s, ms)` (years 0–99 mean 1900–1999, as the spec says).
pub fn date_utc(y: f64, m: f64, d: f64, h: f64, mi: f64, s: f64, ms: f64) -> f64 {
    if ![y, m, d, h, mi, s, ms].iter().all(|v| v.is_finite()) {
        return f64::NAN;
    }
    let yi = y.trunc();
    let y = if (0.0..=99.0).contains(&yi) { 1900.0 + yi } else { yi };
    let day = days_from_civil(y, m.trunc()) + d.trunc() - 1.0;
    let t = day * MS_PER_DAY + h.trunc() * 3_600_000.0 + mi.trunc() * 60_000.0 + s.trunc() * 1000.0 + ms.trunc();
    time_clip(t)
}

fn time_clip(t: f64) -> f64 {
    if !t.is_finite() || t.abs() > 8.64e15 { f64::NAN } else { t + 0.0 }
}

/// (year, month 1–12, day, hour, minute, second, ms) of a time value (UTC).
pub fn civil(t: f64) -> (i64, u32, u32, u32, u32, u32, u32) {
    let days = (t / MS_PER_DAY).floor();
    let ms_day = t - days * MS_PER_DAY;
    let z = days + 719_468.0;
    let era = (z / 146_097.0).floor();
    let doe = z - era * 146_097.0;
    let yoe = ((doe - (doe / 1460.0).floor() + (doe / 36524.0).floor() - (doe / 146_096.0).floor()) / 365.0).floor();
    let y = yoe + era * 400.0;
    let doy = doe - (365.0 * yoe + (yoe / 4.0).floor() - (yoe / 100.0).floor());
    let mp = ((5.0 * doy + 2.0) / 153.0).floor();
    let d = doy - ((153.0 * mp + 2.0) / 5.0).floor() + 1.0;
    let m = if mp < 10.0 { mp + 3.0 } else { mp - 9.0 };
    let y = if m <= 2.0 { y + 1.0 } else { y };
    let secs = (ms_day / 1000.0).floor();
    let msr = ms_day - secs * 1000.0;
    let to_u = |v: f64| -> u32 { u32::try_from(f2u(v)).unwrap_or(0) };
    let ya = i64::try_from(f2u(y.abs())).unwrap_or(0);
    let yi = if y.is_finite() && y.abs() < 1.0e9 { if y < 0.0 { ya.saturating_neg() } else { ya } } else { 0 };
    (yi, to_u(m), to_u(d), to_u((secs / 3600.0).floor()), to_u(((secs % 3600.0) / 60.0).floor()), to_u(secs % 60.0), to_u(msr))
}

/// `new Date(t).toISOString()` (None for an invalid time, where the JS throws).
pub fn to_iso(t: f64) -> Option<String> {
    if !t.is_finite() {
        return None;
    }
    let (y, mo, d, h, mi, s, ms) = civil(t);
    let year = if (0..=9999).contains(&y) {
        format!("{y:04}")
    } else if y < 0 {
        format!("-{:06}", y.unsigned_abs())
    } else {
        format!("+{y:06}")
    };
    Some(format!("{year}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{ms:03}Z"))
}

/// `toISOString().replace(/\.\d+Z$/, 'Z')`.
pub fn to_iso_s(t: f64) -> Option<String> {
    to_iso(t).map(|s| match s.rfind('.') {
        Some(i) if s.ends_with('Z') => format!("{}Z", s.get(..i).unwrap_or("")),
        _ => s,
    })
}

/// `new Date(s)` → time value (NaN when V8 would give an Invalid Date). Reads the ISO forms V8 reads (`YYYY`,
/// `YYYY-MM`, `YYYY-MM-DD`, `±YYYYYY-…`, `T` / `t` / space, `HH:mm[:ss[.f…]]`, `Z` / `z` / `±HH[:]mm`; local times are
/// taken as UTC: the parity harness runs with TZ=UTC), with V8's day overflow (Feb 30 → Mar 2).
/// Spec gap (engine quirk): V8's legacy fallback forms ("Oct 10 2026", "1/2/2026") are not read here: NaN.
pub fn parse_date(s: &str) -> f64 {
    let b = trim(s).as_bytes();
    let mut i = 0usize;
    let num = |i: &mut usize, min: usize, max: usize| -> Option<f64> {
        let start = *i;
        let mut v = 0.0f64;
        while *i < b.len() && i.saturating_sub(start) < max {
            match b.get(*i) {
                Some(c) if c.is_ascii_digit() => {
                    v = v * 10.0 + f64::from(c.saturating_sub(b'0'));
                    *i = i.saturating_add(1);
                }
                _ => break,
            }
        }
        if i.saturating_sub(start) < min { None } else { Some(v) }
    };
    let year = match b.first() {
        Some(b'+') | Some(b'-') => {
            let neg = b.first() == Some(&b'-');
            i = 1;
            let y = match num(&mut i, 6, 6) {
                Some(y) => y,
                None => return f64::NAN,
            };
            if neg && y == 0.0 {
                return f64::NAN;
            }
            if neg { -y } else { y }
        }
        _ => match num(&mut i, 4, 4) {
            Some(y) => y,
            None => return f64::NAN,
        },
    };
    let mut month = 1.0;
    let mut day = 1.0;
    let mut date_only = true;
    if b.get(i) == Some(&b'-') {
        i = i.saturating_add(1);
        month = match num(&mut i, 1, 2) {
            Some(m) => m,
            None => return f64::NAN,
        };
        if b.get(i) == Some(&b'-') {
            i = i.saturating_add(1);
            day = match num(&mut i, 1, 2) {
                Some(d) => d,
                None => return f64::NAN,
            };
        }
    }
    let (mut h, mut mi, mut sec, mut ms) = (0.0, 0.0, 0.0, 0.0);
    let mut offset_min = 0.0;
    if matches!(b.get(i), Some(b'T' | b't' | b' ')) {
        date_only = false;
        i = i.saturating_add(1);
        h = match num(&mut i, 2, 2) {
            Some(v) => v,
            None => return f64::NAN,
        };
        if b.get(i) != Some(&b':') {
            return f64::NAN;
        }
        i = i.saturating_add(1);
        mi = match num(&mut i, 2, 2) {
            Some(v) => v,
            None => return f64::NAN,
        };
        if b.get(i) == Some(&b':') {
            i = i.saturating_add(1);
            sec = match num(&mut i, 2, 2) {
                Some(v) => v,
                None => return f64::NAN,
            };
            if b.get(i) == Some(&b'.') {
                i = i.saturating_add(1);
                let start = i;
                let mut k = 0usize;
                while let Some(c) = b.get(i).filter(|c| c.is_ascii_digit()) {
                    if k < 3 {
                        ms = ms * 10.0 + f64::from(c.saturating_sub(b'0'));
                        k = k.saturating_add(1);
                    }
                    i = i.saturating_add(1);
                }
                if i == start {
                    return f64::NAN;
                }
                while k < 3 {
                    ms *= 10.0;
                    k = k.saturating_add(1);
                }
            }
        }
        match b.get(i) {
            Some(b'Z' | b'z') => i = i.saturating_add(1),
            Some(&c @ (b'+' | b'-')) => {
                i = i.saturating_add(1);
                let oh = match num(&mut i, 2, 2) {
                    Some(v) => v,
                    None => return f64::NAN,
                };
                if b.get(i) == Some(&b':') {
                    i = i.saturating_add(1);
                }
                let om = match num(&mut i, 2, 2) {
                    Some(v) => v,
                    None => return f64::NAN,
                };
                offset_min = (oh * 60.0 + om) * if c == b'+' { 1.0 } else { -1.0 };
            }
            _ => {}
        }
    }
    if i != b.len() {
        return f64::NAN;
    }
    let _ = date_only;
    if !(1.0..=12.0).contains(&month) || !(1.0..=31.0).contains(&day) || h > 24.0 || mi > 59.0 || sec > 59.0 {
        return f64::NAN;
    }
    if h == 24.0 && (mi != 0.0 || sec != 0.0 || ms != 0.0) {
        return f64::NAN;
    }
    // Date.UTC's 0–99 → 1900s rule does not apply to parsed years
    let dayn = days_from_civil(year, month - 1.0) + day - 1.0;
    let t = dayn * MS_PER_DAY + h * 3_600_000.0 + mi * 60_000.0 + sec * 1000.0 + ms - offset_min * 60_000.0;
    time_clip(t)
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// `d.toLocaleDateString('en', { month: 'short', day: 'numeric', year: 'numeric', timeZone: 'UTC' })` →
/// "Oct 10, 2026". Spec gap: years before 1 (Intl writes an era) print as plain numbers here.
pub fn short_date_en(t: f64) -> String {
    let (y, mo, d, ..) = civil(t);
    let m = MONTHS.get(usize::try_from(mo.saturating_sub(1)).unwrap_or(0)).copied().unwrap_or("Jan");
    format!("{m} {d}, {y}")
}

/// A float the caller has range-checked (or clamps here: NaN and negatives → 0, above 9·10^15 → 9·10^15) as u64.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn f2u(x: f64) -> u64 {
    if x.is_finite() && x >= 0.0 { x.min(9.0e15) as u64 } else { 0 }
}
/// [`f2u`] as usize.
pub fn f2usize(x: f64) -> usize {
    usize::try_from(f2u(x)).unwrap_or(usize::MAX)
}
/// `(i + 1) % n` for an index in 0..n.
#[inline]
pub fn next_i(i: usize, n: usize) -> usize {
    let j = i.saturating_add(1);
    if j >= n { 0 } else { j }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dates() {
        let t = parse_date("2026-10-10T12:30:00Z");
        assert_eq!(to_iso(t).as_deref(), Some("2026-10-10T12:30:00.000Z"));
        assert_eq!(to_iso(parse_date("2026-02-30")).as_deref(), Some("2026-03-02T00:00:00.000Z"));
        assert!(parse_date("2026-13-01").is_nan());
        assert_eq!(to_iso(parse_date("2026-10-10T10:00:00+05:30")).as_deref(), Some("2026-10-10T04:30:00.000Z"));
        assert_eq!(to_iso(parse_date("2026-10-10T24:00:00Z")).as_deref(), Some("2026-10-11T00:00:00.000Z"));
        assert_eq!(to_iso(date_utc(2026.0, 12.0, 1.0, 0.0, 0.0, 0.0, 0.0)).as_deref(), Some("2027-01-01T00:00:00.000Z"));
        assert_eq!(to_iso(date_utc(50.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0)).as_deref(), Some("1950-01-01T00:00:00.000Z"));
        assert_eq!(to_iso(-62198755200000.0).as_deref(), Some("-000001-01-01T00:00:00.000Z"));
        assert_eq!(short_date_en(t), "Oct 10, 2026");
    }
}
