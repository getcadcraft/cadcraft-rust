//! units.js — drawing units, stations and number formatting.
//!
//! A project is in US feet ('ft') or metres ('m'); stations: feet use 100 ft stations ("12+34.56"), metres 1000 m
//! ("1+234.567"). Grades are fractions (0.02) shown in percent; side slopes H:V.

use crate::i18n::t;
use crate::js::{JsMath, num_to_string, pad_start, round, str_to_number, to_fixed};

/// One unit's facts (`UNITS.ft` / `UNITS.m`).
#[derive(Clone, Copy, Debug)]
pub struct Unit {
    pub key: &'static str,
    pub short: &'static str,
    pub area: &'static str,
    pub vol: &'static str,
    pub vol_per: f64,
    pub sta_len: f64,
    pub sta_dec: usize,
    pub dec: usize,
    pub landxml: &'static str,
}
impl Unit {
    /// `u.name` (a getter through t()).
    pub fn name(&self) -> String {
        t(if self.key == "m" { "units.m" } else { "units.ft" }, &[])
    }
}
pub const FT: Unit = Unit {
    key: "ft",
    short: "ft",
    area: "sq ft",
    vol: "cu yd",
    vol_per: 27.0,
    sta_len: 100.0,
    sta_dec: 2,
    dec: 2,
    landxml: "USSurveyFoot",
};
pub const M: Unit =
    Unit { key: "m", short: "m", area: "sq m", vol: "cu m", vol_per: 1.0, sta_len: 1000.0, sta_dec: 3, dec: 3, landxml: "meter" };

/// `unitOf(u)`: 'm' → metres, anything else → feet.
pub fn unit_of(u: &str) -> &'static Unit {
    if u == "m" { &M } else { &FT }
}

/// `METRES_PER[name]`.
pub fn metres_per(name: &str) -> Option<f64> {
    Some(match name {
        "USSurveyFoot" => 1200.0 / 3937.0,
        "foot" | "ft" => 0.3048,
        "meter" | "m" | "metre" => 1.0,
        "millimeter" => 0.001,
        "centimeter" => 0.01,
        "kilometer" => 1000.0,
        _ => return None,
    })
}

/// `formatStation(sta, unit, dec)`.
pub fn format_station(sta: f64, unit: &str, dec: Option<usize>) -> String {
    if !sta.is_finite() {
        return String::new();
    }
    let u = unit_of(unit);
    let d = dec.unwrap_or(u.sta_dec);
    let neg = sta < 0.0;
    let r = str_to_number(&to_fixed(sta.abs(), d));
    let big = (r / u.sta_len + 1e-9).floor();
    let mut rest = r - big * u.sta_len;
    if rest < 0.0 {
        rest = 0.0;
    }
    let w = num_to_string(u.sta_len).len().saturating_sub(1);
    let width = w.saturating_add(if d > 0 { d.saturating_add(1) } else { 0 });
    let rest_str = pad_start(&to_fixed(rest, d), width, '0');
    format!("{}{}+{}", if neg { "-" } else { "" }, num_to_string(big), rest_str)
}

/// `isNum(s)` for text: `/^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/` on the trimmed text.
pub fn is_num_str(s: &str) -> bool {
    let b = crate::js::trim(s).as_bytes();
    let mut i = 0usize;
    if matches!(b.first(), Some(b'+' | b'-')) {
        i = 1;
    }
    let digits = |i: &mut usize| {
        let s = *i;
        while b.get(*i).is_some_and(u8::is_ascii_digit) {
            *i = i.saturating_add(1);
        }
        i.saturating_sub(s)
    };
    let n1 = digits(&mut i);
    if n1 > 0 {
        if b.get(i) == Some(&b'.') {
            i = i.saturating_add(1);
            digits(&mut i);
        }
    } else {
        if b.get(i) != Some(&b'.') {
            return false;
        }
        i = i.saturating_add(1);
        if digits(&mut i) == 0 {
            return false;
        }
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i = i.saturating_add(1);
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i = i.saturating_add(1);
        }
        if digits(&mut i) == 0 {
            return false;
        }
    }
    i == b.len()
}

/// `parseStation(s, unit)` for text.
pub fn parse_station(s: &str, unit: &str) -> Option<f64> {
    let t: String = crate::js::trim(s).chars().filter(|c| !crate::js::is_ws(*c)).collect();
    if t.is_empty() {
        return None;
    }
    // ^([+-]?)(\d+)\+(\d+(?:\.\d*)?)$
    let (sign, body) = match t.as_bytes().first() {
        Some(b'+') => ("+", t.get(1..).unwrap_or("")),
        Some(b'-') => ("-", t.get(1..).unwrap_or("")),
        _ => ("", t.as_str()),
    };
    if let Some((a, b)) = body.split_once('+') {
        let a_ok = !a.is_empty() && a.bytes().all(|c| c.is_ascii_digit());
        let (bi, bf) = b.split_once('.').map_or((b, None), |(x, y)| (x, Some(y)));
        let b_ok = !bi.is_empty() && bi.bytes().all(|c| c.is_ascii_digit()) && bf.is_none_or(|f| f.bytes().all(|c| c.is_ascii_digit()));
        if a_ok && b_ok {
            let v = str_to_number(a) * unit_of(unit).sta_len + str_to_number(b);
            return Some(if sign == "-" { -v } else { v });
        }
    }
    if is_num_str(&t) { Some(str_to_number(&t)) } else { None }
}

/// `fixed(n, d)`: fixed decimals, tiny values shown as 0.
pub fn fixed(n: f64, d: usize) -> String {
    if !n.is_finite() {
        return String::new();
    }
    let lim = 0.5 * 10f64.jpow(-(d as f64));
    let v = if n.abs() < lim { 0.0 } else { n };
    to_fixed(v, d)
}

/// `n.toLocaleString('en-US', {minimumFractionDigits: d, maximumFractionDigits: d})`: ICU rounds the shortest
/// decimal form of the number half away from zero, then groups the integer digits by three.
pub fn grouped(n: f64, d: usize) -> String {
    if !n.is_finite() {
        return String::new();
    }
    let neg = n < 0.0 || (n == 0.0 && n.is_sign_negative());
    let s = shortest_fixed(n.abs(), d);
    let (ip, fp) = s.split_once('.').map_or((s.as_str(), ""), |(a, b)| (a, b));
    let mut out = String::new();
    let digits: Vec<char> = ip.chars().collect();
    for (i, c) in digits.iter().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*c);
    }
    if d > 0 {
        out.push('.');
        out.push_str(fp);
    }
    if neg { format!("-{out}") } else { out }
}

/// The shortest decimal digits of x (≥ 0) rounded half away from zero to d decimals, as "ddd.ff".
fn shortest_fixed(x: f64, d: usize) -> String {
    let dg = cc_cad::js::shortest_digits(x);
    let digits: Vec<u8> = if x == 0.0 { vec![b'0'] } else { dg.digits().to_vec() };
    let n = if x == 0.0 { 1 } else { dg.n }; // x = 0.d1d2… × 10^n
    // integer digits and fraction digits as one digit string with the point after `int_len`
    let mut all: Vec<u8> = Vec::new();
    let int_len: usize = if n <= 0 {
        all.resize(usize::try_from(n.unsigned_abs()).unwrap_or(0).saturating_add(1), b'0');
        all.extend_from_slice(&digits);
        1
    } else {
        let nu = usize::try_from(n).unwrap_or(0);
        all.extend_from_slice(&digits);
        if all.len() < nu {
            all.resize(nu, b'0');
        }
        nu
    };
    let keep = int_len.saturating_add(d);
    let round_up = all.get(keep).is_some_and(|&c| c >= b'5');
    all.truncate(keep);
    while all.len() < keep {
        all.push(b'0');
    }
    let mut int_len = int_len;
    if round_up {
        let mut carry = true;
        for c in all.iter_mut().rev() {
            if *c == b'9' {
                *c = b'0';
            } else {
                *c = c.saturating_add(1);
                carry = false;
                break;
            }
        }
        if carry {
            all.insert(0, b'1');
            int_len = int_len.saturating_add(1);
        }
    }
    let ip: String = all.get(..int_len).unwrap_or(&[]).iter().map(|&c| char::from(c)).collect();
    let ip = {
        let t = ip.trim_start_matches('0');
        if t.is_empty() { "0".to_string() } else { t.to_string() }
    };
    let fp: String = all.get(int_len..).unwrap_or(&[]).iter().map(|&c| char::from(c)).collect();
    if d > 0 { format!("{ip}.{fp}") } else { ip }
}

/// `pct(g, d)`: "+2.00%".
pub fn pct(g: f64, d: usize) -> String {
    if !g.is_finite() {
        return String::new();
    }
    format!("{}{}%", if g * 100.0 >= 0.0 { "+" } else { "" }, fixed(g * 100.0, d))
}

/// `volumeText(v, unit, d)`.
pub fn volume_text(v: f64, unit: &str, d: usize) -> String {
    let u = unit_of(unit);
    format!("{} {}", grouped(v / u.vol_per, d), u.vol)
}
/// `areaText(a, unit, d)`.
pub fn area_text(a: f64, unit: &str, d: usize) -> String {
    format!("{} {}", grouped(a, d), unit_of(unit).area)
}

/// `bearing(a)`: a direction angle (radians, 0 = east, CCW) → "N 45°12'30\" E".
pub fn bearing(a: f64) -> String {
    let mut az = (90.0 - a * 180.0 / std::f64::consts::PI) % 360.0;
    if az < 0.0 {
        az += 360.0;
    }
    let (q, ang) = if az <= 90.0 {
        (("N", "E"), az)
    } else if az <= 180.0 {
        (("S", "E"), 180.0 - az)
    } else if az <= 270.0 {
        (("S", "W"), az - 180.0)
    } else {
        (("N", "W"), 360.0 - az)
    };
    format!("{} {} {}", q.0, dms(ang), q.1)
}
/// `dms(deg)`: 45°12'30".
pub fn dms(deg: f64) -> String {
    let mut s = round(deg.abs() * 3600.0);
    let d = (s / 3600.0).floor();
    s -= d * 3600.0;
    let m = (s / 60.0).floor();
    s -= m * 60.0;
    format!("{}°{}'{}\"", num_to_string(d), pad_start(&num_to_string(m), 2, '0'), pad_start(&num_to_string(s), 2, '0'))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stations() {
        assert_eq!(format_station(1234.56, "ft", None), "12+34.56");
        assert_eq!(format_station(1234.567, "m", None), "1+234.567");
        assert_eq!(format_station(-50.0, "ft", None), "-0+50.00");
        assert_eq!(parse_station("12+34.56", "ft"), Some(1234.56));
        assert_eq!(parse_station("-0+50", "ft"), Some(-50.0));
        assert_eq!(parse_station("x", "ft"), None);
    }
    #[test]
    fn grouping() {
        assert_eq!(grouped(1234567.891, 1), "1,234,567.9");
        assert_eq!(grouped(1.005, 2), "1.01");
        assert_eq!(grouped(0.0, 0), "0");
    }
}
