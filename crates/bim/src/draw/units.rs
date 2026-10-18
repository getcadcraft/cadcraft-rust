//! model-units.js (and libitem.js `parseLength`) — lengths typed in and shown in feet and inches or millimetres; the
//! wall tool's typed input; areas, angles and roof pitches as text. Everything inside the tools is millimetres. The
//! JS's regular expressions are matched by hand (no regex engine): each matcher says which pattern it stands for.

use crate::draw::m;
use crate::js::{num_str, str_num, to_fixed};

const INCH: f64 = 25.4;

/// JS `\s` (white space and line terminators).
fn is_ws(c: char) -> bool {
    cc_cad::js::is_ws(c)
}

/// A tiny cursor over the text (characters).
struct Cur<'a> {
    s: &'a [char],
    i: usize,
}

impl Cur<'_> {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }
    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.i = self.i.saturating_add(1);
            true
        } else {
            false
        }
    }
    fn ws(&mut self) -> usize {
        let start = self.i;
        while self.peek().is_some_and(is_ws) {
            self.i = self.i.saturating_add(1);
        }
        self.i.saturating_sub(start)
    }
    /// `\d+`
    fn digits(&mut self) -> Option<String> {
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i = self.i.saturating_add(1);
        }
        (self.i > start).then(|| self.s.get(start..self.i).map(|x| x.iter().collect()).unwrap_or_default())
    }
    /// `\d+(?:\.\d+)?`
    fn number(&mut self) -> Option<String> {
        let mut n = self.digits()?;
        let save = self.i;
        if self.eat('.') {
            match self.digits() {
                Some(d) => {
                    n.push('.');
                    n.push_str(&d);
                }
                None => self.i = save,
            }
        }
        Some(n)
    }
    fn at_end(&self) -> bool {
        self.i >= self.s.len()
    }
}

fn unit_mm(u: &str) -> Option<f64> {
    Some(match u {
        "mm" => 1.0,
        "cm" => 10.0,
        "m" => 1000.0,
        "in" => 25.4,
        "ft" => 304.8,
        _ => return None,
    })
}

/// libitem.js `parseLength(s, mmPerUnit)`: "3.81 m", "12'-6 1/2\"", a bare number × mmPerUnit; NaN when it is none.
pub fn parse_length(s: &str, mm_per_unit: f64) -> f64 {
    let t: String = crate::js::trim(s)
        .chars()
        .map(|c| match c {
            '\u{2032}' | '\u{2019}' => '\'',
            '\u{2033}' | '\u{201d}' => '"',
            c => c,
        })
        .collect::<String>()
        .to_lowercase();
    if t.is_empty() {
        return f64::NAN;
    }
    let ch: Vec<char> = t.chars().collect();
    // ^(-?\d+(?:\.\d+)?)\s*(mm|cm|m|in|ft)$
    {
        let mut c = Cur { s: &ch, i: 0 };
        let neg = c.eat('-');
        if let Some(n) = c.number() {
            c.ws();
            let rest: String = ch.get(c.i..).map(|x| x.iter().collect()).unwrap_or_default();
            if let Some(f) = unit_mm(&rest) {
                return str_num(&format!("{}{n}", if neg { "-" } else { "" })) * f;
            }
        }
    }
    // the feet-inches pattern, with a ' or " somewhere in the text
    if (t.contains('\'') || t.contains('"'))
        && let Some(g) = feet_inches(&ch)
        && (g.0.is_some() || g.1.is_some() || g.4.is_some())
    {
        let num = |x: &Option<String>| x.as_deref().map_or(0.0, str_num);
        let ft = num(&g.0);
        let mut inch = num(&g.1);
        if let (Some(a), Some(b)) = (&g.2, &g.3) {
            inch += str_num(a) / str_num(b);
        }
        if let (Some(a), Some(b)) = (&g.4, &g.5) {
            inch += str_num(a) / str_num(b);
        }
        return (ft * 12.0 + inch) * 25.4;
    }
    // ^-?\d+(\.\d+)?$
    {
        let mut c = Cur { s: &ch, i: 0 };
        c.eat('-');
        if c.number().is_some() && c.at_end() {
            return str_num(&t) * mm_per_unit;
        }
    }
    f64::NAN
}

type Groups = (Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>);
type InchGroups = (Option<String>, Option<String>, Option<String>, Option<String>, Option<String>);
type FeetFound = Option<(Option<String>, Option<(String, String)>)>;

/// `^(?:(\d+(?:\.\d+)?)\s*'\s*-?\s*)?(?:(\d+(?:\.\d+)?)(?:\s+(\d+)\/(\d+))?|(\d+)\/(\d+))?\s*(?:"|in)?$`
fn feet_inches(ch: &[char]) -> Option<Groups> {
    let tail = |c: &mut Cur<'_>| -> bool {
        c.ws();
        if !c.eat('"') && c.peek() == Some('i') && c.s.get(c.i.saturating_add(1)) == Some(&'n') {
            c.i = c.i.saturating_add(2);
        }
        c.at_end()
    };
    let inch_part = |start: usize| -> Option<InchGroups> {
        // A: a number, with a fraction after white space
        let mut c = Cur { s: ch, i: start };
        if let Some(n) = c.number() {
            let after_n = c.i;
            if c.ws() > 0
                && let Some(a) = c.digits()
                && c.eat('/')
                && let Some(b) = c.digits()
            {
                let mut c2 = Cur { s: ch, i: c.i };
                if tail(&mut c2) {
                    return Some((Some(n), Some(a), Some(b), None, None));
                }
            }
            let mut c2 = Cur { s: ch, i: after_n };
            if tail(&mut c2) {
                return Some((Some(n), None, None, None, None));
            }
        }
        // B: a fraction alone
        let mut c = Cur { s: ch, i: start };
        if let Some(a) = c.digits()
            && c.eat('/')
            && let Some(b) = c.digits()
        {
            let mut c2 = Cur { s: ch, i: c.i };
            if tail(&mut c2) {
                return Some((None, None, None, Some(a), Some(b)));
            }
        }
        // none
        let mut c2 = Cur { s: ch, i: start };
        if tail(&mut c2) {
            return Some((None, None, None, None, None));
        }
        None
    };
    // with the feet group
    let mut c = Cur { s: ch, i: 0 };
    if let Some(ft) = c.number() {
        c.ws();
        if c.eat('\'') {
            c.ws();
            c.eat('-');
            c.ws();
            if let Some(g) = inch_part(c.i) {
                return Some((Some(ft), g.0, g.1, g.2, g.3, g.4));
            }
        }
    }
    inch_part(0).map(|g| (None, g.0, g.1, g.2, g.3, g.4))
}

/// `fmtLen(mm, units, { unit })`: feet-inches to 1/16" (fractions reduced), or whole millimetres (0.1 below 10).
pub fn fmt_len(mm: f64, units: &str) -> String {
    fmt_len_u(mm, units, false)
}

pub fn fmt_len_u(mm: f64, units: &str, unit: bool) -> String {
    if !mm.is_finite() {
        return String::new();
    }
    if units == "mm" {
        let r = if mm.abs() < 10.0 { m::round(mm * 10.0) / 10.0 } else { m::round(mm) };
        let r = if r == 0.0 { 0.0 } else { r };
        return format!("{}{}", num_str(r), if unit { " mm" } else { "" });
    }
    let neg = mm < 0.0;
    let mut sixteenths = m::round(mm.abs() / INCH * 16.0);
    let ft = (sixteenths / 192.0).floor();
    sixteenths -= ft * 192.0;
    let inch = (sixteenths / 16.0).floor();
    let mut frac = sixteenths - inch * 16.0;
    let mut den = 16.0;
    while frac != 0.0 && frac % 2.0 == 0.0 {
        frac /= 2.0;
        den /= 2.0;
    }
    let whole = ft != 0.0 || inch != 0.0 || frac == 0.0;
    let mut in_text = String::new();
    if whole {
        in_text.push_str(&num_str(inch));
    }
    if frac != 0.0 {
        if whole {
            in_text.push(' ');
        }
        in_text.push_str(&format!("{}/{}", num_str(frac), num_str(den)));
    }
    in_text.push('"');
    let s = if ft != 0.0 { format!("{}'-{in_text}", num_str(ft)) } else { in_text };
    format!("{}{s}", if neg && sixteenths + ft != 0.0 { "-" } else { "" })
}

/// `parseLen(text, units)` → mm (NaN when it is not a length).
pub fn parse_len(text: &str, units: &str) -> f64 {
    let s = crate::js::trim(text);
    if s.is_empty() {
        return f64::NAN;
    }
    let v = parse_length(s, if units == "mm" { 1.0 } else { INCH });
    if v.is_finite() {
        return v;
    }
    let ch: Vec<char> = s.chars().collect();
    // ^(-?\d+(?:\.\d+)?)\s*'\s*-?\s*(\d+(?:\.\d+)?)?(?:\s+(\d+)\/(\d+))?\s*"?$
    {
        let mut c = Cur { s: &ch, i: 0 };
        let neg = c.eat('-');
        if let Some(n) = c.number() {
            c.ws();
            if c.eat('\'') {
                c.ws();
                c.eat('-');
                c.ws();
                let rest = c.i;
                let tail = |i: usize| {
                    let mut c2 = Cur { s: &ch, i };
                    c2.ws();
                    c2.eat('"');
                    c2.at_end()
                };
                let mut found: FeetFound = None;
                let mut c1 = Cur { s: &ch, i: rest };
                let n2 = c1.number();
                let after2 = c1.i;
                // with the number, with / without the fraction; then without the number
                let mut tries: Vec<(Option<String>, usize)> = Vec::new();
                if n2.is_some() {
                    tries.push((n2.clone(), after2));
                }
                tries.push((None, rest));
                for (num, at) in tries {
                    let mut c3 = Cur { s: &ch, i: at };
                    if c3.ws() > 0
                        && let Some(a) = c3.digits()
                        && c3.eat('/')
                        && let Some(b) = c3.digits()
                        && tail(c3.i)
                    {
                        found = Some((num, Some((a, b))));
                        break;
                    }
                    if tail(at) {
                        found = Some((num, None));
                        break;
                    }
                }
                if let Some((num, fr)) = found {
                    let m1 = str_num(&format!("{}{n}", if neg { "-" } else { "" }));
                    let sg = m::sign(if m1 != 0.0 && !m1.is_nan() { m1 } else { 1.0 });
                    let f = fr.map_or(0.0, |(a, b)| str_num(&a) / str_num(&b));
                    return sg * (m1.abs() * 12.0 + num.as_deref().map_or(0.0, str_num) + f) * INCH;
                }
            }
        }
    }
    if units != "mm" {
        // ^(\d+)\s+(\d+)\/(\d+)$
        let mut c = Cur { s: &ch, i: 0 };
        if let Some(w) = c.digits()
            && c.ws() > 0
            && let Some(a) = c.digits()
            && c.eat('/')
            && let Some(b) = c.digits()
            && c.at_end()
        {
            return (str_num(&w) + str_num(&a) / str_num(&b)) * INCH;
        }
        // ^(-?)\s*(\d+)\/(\d+)$
        let mut c = Cur { s: &ch, i: 0 };
        let neg = c.eat('-');
        c.ws();
        if let Some(a) = c.digits()
            && c.eat('/')
            && let Some(b) = c.digits()
            && c.at_end()
        {
            let d = str_num(&b);
            if d != 0.0 {
                return (if neg { -1.0 } else { 1.0 }) * str_num(&a) / d * INCH;
            }
        }
    }
    f64::NAN
}

/// What the wall tool takes from the keyboard.
#[derive(Clone, Debug, PartialEq)]
pub enum Typed {
    Len(f64),
    Polar(f64, f64),
    Rel(f64, f64),
    Abs(f64, f64),
}

fn split_pair(t: &str) -> Option<(String, String)> {
    let i = t.find([',', ';'])?;
    let a = crate::js::trim(t.get(..i)?).to_string();
    let b = crate::js::trim(t.get(i.saturating_add(1)..)?).to_string();
    (!a.is_empty() && !b.is_empty()).then_some((a, b))
}

/// `parseTyped(text, units)`
pub fn parse_typed(text: &str, units: &str) -> Option<Typed> {
    let s = crate::js::trim(text);
    if s.is_empty() {
        return None;
    }
    let pair = |t: &str| -> Option<(f64, f64)> {
        let (a, b) = split_pair(t)?;
        let (x, y) = (parse_len(&a, units), parse_len(&b, units));
        (x.is_finite() && y.is_finite()).then_some((x, y))
    };
    if let Some(rest) = s.strip_prefix('@') {
        return pair(rest).map(|(x, y)| Typed::Rel(x, y));
    }
    if let Some(lt) = s.find('<').filter(|&i| i > 0) {
        let len = parse_len(s.get(..lt).unwrap_or(""), units);
        let a = crate::js::trim(s.get(lt.saturating_add(1)..).unwrap_or(""));
        let a = a.strip_suffix('°').unwrap_or(a);
        let angle = str_num(a);
        return (len.is_finite() && len > 0.0 && angle.is_finite()).then_some(Typed::Polar(len, angle));
    }
    if s.contains([',', ';']) {
        return pair(s).map(|(x, y)| Typed::Abs(x, y));
    }
    let len = parse_len(s, units);
    (len.is_finite() && len > 0.0).then_some(Typed::Len(len))
}

/// An angle in degrees as text, to 0.1°.
pub fn fmt_angle(deg: f64) -> String {
    let mut a = m::round(((deg % 360.0) + 360.0) % 360.0 * 10.0) / 10.0;
    if a == 360.0 {
        a = 0.0;
    }
    format!("{}°", num_str(a))
}

/// mm² → m² (to 0.01) or ft² (to 0.1), with the unit.
pub fn fmt_area(mm2: f64, units: &str) -> String {
    if units == "mm" {
        return format!("{} m²", to_fixed(m::round(mm2 / 1e4) / 100.0, 2));
    }
    format!("{} ft²", to_fixed(m::round(mm2 / (304.8 * 304.8) * 10.0) / 10.0, 1))
}

/// A roof pitch typed as a rise over 12 or in degrees → degrees (NaN).
pub fn parse_pitch(text: &str) -> f64 {
    let s0 = crate::js::trim(text);
    let s = crate::js::trim(s0.strip_suffix('°').unwrap_or(s0));
    let ch: Vec<char> = s.chars().collect();
    let mut c = Cur { s: &ch, i: 0 };
    if let Some(a) = c.number() {
        c.ws();
        let sep = c.eat('/') || c.eat(':') || {
            let lo: String = ch.get(c.i..c.i.saturating_add(2)).map(|x| x.iter().collect::<String>().to_lowercase()).unwrap_or_default();
            if lo == "in" {
                c.i = c.i.saturating_add(2);
                true
            } else {
                false
            }
        };
        if sep {
            c.ws();
            if let Some(b) = c.number()
                && c.at_end()
            {
                let run = str_num(&b);
                return if run > 0.0 { m::atan(str_num(&a) / run) * 180.0 / std::f64::consts::PI } else { f64::NAN };
            }
        }
    }
    let d = str_num(s);
    if d.is_finite() && !s.is_empty() { d } else { f64::NAN }
}

/// A pitch as text: a rise over 12 to 1/4 (ftin), else degrees to 0.1°.
pub fn fmt_pitch(deg: f64, units: &str) -> String {
    if units == "ftin" {
        let r = m::round(m::tan(deg * std::f64::consts::PI / 180.0) * 12.0 * 4.0) / 4.0;
        return format!("{}/12", num_str(r));
    }
    format!("{}°", num_str(m::round(deg * 10.0) / 10.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() <= 0.01
    }

    #[test]
    fn the_model_test_units_section() {
        assert_eq!(fmt_len(3810.0, "ftin"), "12'-6\"");
        assert_eq!(fmt_len(12.7, "ftin"), "1/2\"");
        assert_eq!(fmt_len(304.8 * 10.0 + 6.35, "ftin"), "10'-0 1/4\"");
        assert_eq!(fmt_len(-12.7, "ftin"), "-1/2\"");
        assert_eq!(fmt_len(3810.4, "mm"), "3810");
        assert_eq!(fmt_len(4.25, "mm"), "4.3");
        assert_eq!(fmt_len_u(3000.0, "mm", true), "3000 mm");
        assert!(near(parse_len("12'6\"", "ftin"), 3810.0) && near(parse_len("12' 6", "ftin"), 3810.0));
        assert!(near(parse_len("12'-6 1/2\"", "ftin"), 3822.7) && near(parse_len("6", "ftin"), 152.4));
        assert!(near(parse_len("6 1/2", "ftin"), 165.1) && near(parse_len("1/2", "ftin"), 12.7));
        assert!(
            near(parse_len("3000", "mm"), 3000.0) && near(parse_len("3.81 m", "ftin"), 3810.0) && near(parse_len("12 ft", "mm"), 3657.6)
        );
        assert!(parse_len("abc", "mm").is_nan());
        assert_eq!(parse_typed("3000", "mm"), Some(Typed::Len(3000.0)));
        assert_eq!(parse_typed("3000<90", "mm"), Some(Typed::Polar(3000.0, 90.0)));
        assert_eq!(parse_typed("@1200,-300", "mm"), Some(Typed::Rel(1200.0, -300.0)));
        assert_eq!(parse_typed("1200, 3400", "mm"), Some(Typed::Abs(1200.0, 3400.0)));
        assert_eq!(parse_typed("x", "ftin"), None);
        assert_eq!(parse_typed("-5", "mm"), None);
        assert!(matches!(parse_typed("@12',0", "ftin"), Some(Typed::Rel(dx, _)) if near(dx, 3657.6)));
        assert!(near(parse_pitch("6/12"), 26.5650511771));
        assert_eq!(fmt_pitch(26.565051177077986, "ftin"), "6/12");
        assert_eq!(fmt_area(5800.0 * 3800.0, "mm"), "22.04 m²");
    }
}
