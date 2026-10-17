//! `a.localeCompare(b)` as Node's ICU (root / en-US) orders the text a takeoff holds: primary letters and digits with
//! punctuation and symbols before them (not ignored), then accents (secondary), then case, lower first (tertiary).
//! The order of the printable ASCII characters is ICU's (read from Node once); a non-ASCII letter sorts as its
//! decomposed base letter with its marks as accents. Spec gap: other scripts sort by code point after Latin, where ICU
//! has its own script order.
// Integer arithmetic here is small fixed-range collation weights on chars and positions within the compared strings: it cannot overflow, so the crate-wide
// `arithmetic_side_effects` deny is relaxed for this module only.
#![allow(clippy::arithmetic_side_effects)]

use std::cmp::Ordering;
use unicode_normalization::UnicodeNormalization;

/// The printable ASCII characters in ICU's order (letters: lower then upper of each, equal at the primary level).
const ORDER: &str = " _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$0123456789aAbBcCdDeEfFgGhHiIjJkKlLmMnNoOpPqQrRsStTuUvVwWxXyYzZ";

/// (primary, secondary, tertiary) collation elements of a string.
fn elements(s: &str) -> Vec<(u32, u32, u32)> {
    let mut out = Vec::new();
    for c in s.nfd() {
        if c.is_ascii() {
            if let Some(i) = ORDER.find(c) {
                let i = u32::try_from(i).unwrap_or(0);
                if c.is_ascii_alphabetic() {
                    // the two cases share one primary
                    let base = ORDER.find(c.to_ascii_lowercase()).map_or(i, |b| u32::try_from(b).unwrap_or(0));
                    out.push((base + 1, 5, u32::from(c.is_ascii_uppercase())));
                } else {
                    out.push((i + 1, 5, 0));
                }
            } else {
                // control characters: ignorable
                continue;
            }
        } else if ('\u{300}'..='\u{36F}').contains(&c) {
            // a combining mark: an accent (secondary weight above the common one)
            out.push((0, 6 + (c as u32 - 0x300), 0));
        } else {
            let lower: Vec<char> = c.to_lowercase().collect();
            let up = u32::from(lower.first().is_some_and(|l| *l != c));
            let base = lower.first().copied().unwrap_or(c) as u32;
            out.push((1000 + base, 5, up));
        }
    }
    out
}

/// `a.localeCompare(b)` → -1, 0, 1.
pub fn locale_compare(a: &str, b: &str) -> f64 {
    let (ea, eb) = (elements(a), elements(b));
    let level = |f: &dyn Fn(&(u32, u32, u32)) -> u32| -> Ordering {
        let xa: Vec<u32> = ea.iter().map(f).filter(|v| *v != 0).collect();
        let xb: Vec<u32> = eb.iter().map(f).filter(|v| *v != 0).collect();
        xa.cmp(&xb)
    };
    let o = level(&|e| e.0).then_with(|| level(&|e| e.1)).then_with(|| {
        let xa: Vec<u32> = ea.iter().map(|e| e.2).collect();
        let xb: Vec<u32> = eb.iter().map(|e| e.2).collect();
        xa.cmp(&xb)
    });
    match o {
        Ordering::Less => -1.0,
        Ordering::Equal => 0.0,
        Ordering::Greater => 1.0,
    }
}
