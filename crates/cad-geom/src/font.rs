//! The one text face CadCraft draws with: Helvetica metrics (font.js). Arial has the same widths; the screen and the
//! plotted PDFs both use them, so text boxes, wrapping and centring line up.

use crate::nfd::NFD_ASCII;

/// Helvetica advance widths (1/1000 em) for the printable ASCII range.
#[rustfmt::skip]
const HELV: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556,
    278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667,
    611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833,
    556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

/// Windows-1252 codes 0x80–0x9F (the PDF's WinAnsiEncoding).
pub const CP1252: [(char, u8); 27] = [
    ('€', 0x80),
    ('‚', 0x82),
    ('ƒ', 0x83),
    ('„', 0x84),
    ('…', 0x85),
    ('†', 0x86),
    ('‡', 0x87),
    ('ˆ', 0x88),
    ('‰', 0x89),
    ('Š', 0x8a),
    ('‹', 0x8b),
    ('Œ', 0x8c),
    ('Ž', 0x8e),
    ('‘', 0x91),
    ('’', 0x92),
    ('“', 0x93),
    ('”', 0x94),
    ('•', 0x95),
    ('–', 0x96),
    ('—', 0x97),
    ('˜', 0x98),
    ('™', 0x99),
    ('š', 0x9a),
    ('›', 0x9b),
    ('œ', 0x9c),
    ('ž', 0x9e),
    ('Ÿ', 0x9f),
];

/// Widths of characters above 0x7F (by Windows-1252 code) that differ from their base letter.
#[rustfmt::skip]
const WIDE: &[(u32, u16)] = &[
    (0x80, 556), (0x85, 1000), (0x89, 1000), (0x8c, 1000), (0x91, 222), (0x92, 222), (0x93, 333), (0x94, 333), (0x95, 350),
    (0x96, 556), (0x97, 1000), (0x99, 1000), (0x9c, 944), (0xa0, 278), (0xa1, 333), (0xa6, 260), (0xa7, 556), (0xa9, 737),
    (0xaa, 370), (0xab, 556), (0xac, 584), (0xad, 333), (0xae, 737), (0xb0, 400), (0xb1, 584), (0xb2, 333), (0xb3, 333),
    (0xb4, 333), (0xb5, 556), (0xb6, 537), (0xb7, 278), (0xb8, 333), (0xb9, 333), (0xba, 365), (0xbb, 556), (0xbc, 834),
    (0xbd, 834), (0xbe, 834), (0xbf, 611), (0xc6, 1000), (0xd7, 584), (0xd8, 778), (0xdf, 611), (0xe6, 889), (0xf7, 584),
    (0xf8, 611),
];

/// Characters drawn as a close Windows-1252 one (⌀ as Ø, the true minus as a hyphen …).
pub fn subst(c: char) -> char {
    match c {
        '⌀' => 'Ø',
        '−' => '-',
        '′' => '\'',
        '″' => '"',
        '\t' => ' ',
        _ => c,
    }
}

fn helv(code: u32) -> Option<f64> {
    if (32..127).contains(&code) { HELV.get(usize::try_from(code - 32).ok()?).map(|w| f64::from(*w)) } else { None }
}
fn wide(code: u32) -> Option<f64> {
    WIDE.iter().find(|(c, _)| *c == code).map(|(_, w)| f64::from(*w))
}

/// Width of one character in 1/1000 em.
pub fn char_advance(ch: char) -> f64 {
    let ch = subst(ch);
    let code = u32::from(ch);
    if let Some(w) = helv(code) {
        return w;
    }
    let w = match CP1252.iter().find(|(c, _)| *c == ch) {
        Some((_, k)) => wide(u32::from(*k)),
        None => wide(code),
    };
    if let Some(w) = w {
        return w;
    }
    let base = u16::try_from(code)
        .ok()
        .and_then(|c| NFD_ASCII.binary_search_by_key(&c, |(k, _)| *k).ok())
        .and_then(|i| NFD_ASCII.get(i))
        .map(|(_, b)| u32::from(*b));
    if let Some(w) = base.and_then(helv) {
        return w;
    }
    if code >= 0x2e80 { 1000.0 } else { 556.0 }
}

/// Width of a string in 1/1000 em.
pub fn advance(s: &str) -> f64 {
    s.chars().map(char_advance).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn widths() {
        assert_eq!(advance("Hi"), 722.0 + 222.0);
        assert_eq!(char_advance('é'), 556.0);
        assert_eq!(char_advance('€'), 556.0);
        assert_eq!(char_advance('—'), 1000.0);
        assert_eq!(char_advance('中'), 1000.0);
        assert_eq!(char_advance('\t'), 278.0);
        assert_eq!(char_advance('≠'), 584.0);
    }
}
