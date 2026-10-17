//! `a.localeCompare(b)` (ICU root collation in V8) for the strings MarkupCraft sorts: layer names.
//!
//! Spec gap (engine quirk): ICU's full collation tables are not reproduced. This follows their shape — whitespace,
//! then punctuation and symbols, then digits, then letters; letters compare by their base letter first (case and
//! accents ignored), then accents (plain before accented), then case (lower before upper) — which gives ICU's order
//! for ordinary names (letters, digits, spaces, accented Latin letters). Punctuation among itself sorts by code point
//! here, where ICU has its own order.

use std::cmp::Ordering;

fn base(c: char) -> char {
    let mut buf = [0u8; 4];
    let s: &str = c.encode_utf8(&mut buf);
    let _ = s;
    let cp = c as u32;
    if cp < 0x80 {
        return c.to_ascii_lowercase();
    }
    match crate::nfd::NFD_ASCII.binary_search_by(|(k, _)| k.cmp(&cp)) {
        Ok(i) => crate::nfd::NFD_ASCII.get(i).map_or(c, |(_, b)| char::from(*b).to_ascii_lowercase()),
        Err(_) => c.to_lowercase().next().unwrap_or(c),
    }
}

fn primary(c: char) -> (u8, u32) {
    if crate::js::is_ws(c) {
        (0, c as u32)
    } else if c.is_ascii_digit() {
        (2, c as u32)
    } else if c.is_alphabetic() {
        (3, base(c) as u32)
    } else if c.is_ascii() || c.is_ascii_punctuation() {
        (1, c as u32)
    } else {
        (4, c as u32)
    }
}

/// `a.localeCompare(b)` → -1, 0 or 1.
pub fn locale_compare(a: &str, b: &str) -> f64 {
    let pa: Vec<(u8, u32)> = a.chars().map(primary).collect();
    let pb: Vec<(u8, u32)> = b.chars().map(primary).collect();
    let mut ord = pa.cmp(&pb);
    if ord == Ordering::Equal {
        // accents: a letter that is its own base sorts first
        let sa: Vec<bool> = a.chars().map(|c| c.is_alphabetic() && base(c) != c.to_lowercase().next().unwrap_or(c)).collect();
        let sb: Vec<bool> = b.chars().map(|c| c.is_alphabetic() && base(c) != c.to_lowercase().next().unwrap_or(c)).collect();
        ord = sa.cmp(&sb);
    }
    if ord == Ordering::Equal {
        // case: lower before upper
        let ta: Vec<bool> = a.chars().map(char::is_uppercase).collect();
        let tb: Vec<bool> = b.chars().map(char::is_uppercase).collect();
        ord = ta.cmp(&tb);
    }
    match ord {
        Ordering::Less => -1.0,
        Ordering::Greater => 1.0,
        Ordering::Equal => 0.0,
    }
}

/// `a.localeCompare(b, undefined, {numeric: true})`: runs of ASCII digits compare by value, the rest as
/// [`locale_compare`]. Spec gap: an approximation of ICU's numeric collation (same answers on names like "A2" /
/// "A10", "1,250" / "300").
pub fn locale_compare_numeric(a: &str, b: &str) -> f64 {
    fn chunks(s: &str) -> Vec<(bool, &str)> {
        let mut out = Vec::new();
        let mut start = 0usize;
        let mut digit: Option<bool> = None;
        for (i, c) in s.char_indices() {
            let d = c.is_ascii_digit();
            if digit.is_some_and(|x| x != d) {
                out.push((digit.unwrap_or(false), s.get(start..i).unwrap_or("")));
                start = i;
            }
            digit = Some(d);
        }
        if let Some(d) = digit {
            out.push((d, s.get(start..).unwrap_or("")));
        }
        out
    }
    let (ca, cb) = (chunks(a), chunks(b));
    for (x, y) in ca.iter().zip(cb.iter()) {
        let r = if x.0 && y.0 {
            let (tx, ty) = (x.1.trim_start_matches('0'), y.1.trim_start_matches('0'));
            match tx.len().cmp(&ty.len()).then_with(|| tx.cmp(ty)) {
                Ordering::Less => -1.0,
                Ordering::Greater => 1.0,
                Ordering::Equal => 0.0,
            }
        } else {
            locale_compare(x.1, y.1)
        };
        if r != 0.0 {
            return r;
        }
    }
    match ca.len().cmp(&cb.len()) {
        Ordering::Less => -1.0,
        Ordering::Greater => 1.0,
        Ordering::Equal => locale_compare(a, b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn order() {
        let mut v = vec!["Mechanical", "architect", "Électrique", "Architect", "Electrical", "2nd floor"];
        v.sort_by(|a, b| locale_compare(a, b).partial_cmp(&0.0).unwrap_or(Ordering::Equal));
        assert_eq!(v, ["2nd floor", "architect", "Architect", "Electrical", "Électrique", "Mechanical"]);
    }
}
