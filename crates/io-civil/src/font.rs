//! font.js — Helvetica metrics (the one face CivilCraft labels with; Arial has the same widths), Windows-1252.

use unicode_normalization::UnicodeNormalization;

const HELV: [u32; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278,
    278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722,
    667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556,
    556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

/// `CP1252[ch]`: the Windows-1252 code of a character in 0x80–0x9F.
pub fn cp1252(c: char) -> Option<u32> {
    Some(match c {
        '€' => 0x80,
        '‚' => 0x82,
        'ƒ' => 0x83,
        '„' => 0x84,
        '…' => 0x85,
        '†' => 0x86,
        '‡' => 0x87,
        'ˆ' => 0x88,
        '‰' => 0x89,
        'Š' => 0x8a,
        '‹' => 0x8b,
        'Œ' => 0x8c,
        'Ž' => 0x8e,
        '‘' => 0x91,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '•' => 0x95,
        '–' => 0x96,
        '—' => 0x97,
        '˜' => 0x98,
        '™' => 0x99,
        'š' => 0x9a,
        '›' => 0x9b,
        'œ' => 0x9c,
        'ž' => 0x9e,
        'Ÿ' => 0x9f,
        _ => return None,
    })
}
fn wide(code: u32) -> Option<u32> {
    Some(match code {
        0x80 => 556,
        0x85 => 1000,
        0x89 => 1000,
        0x8c => 1000,
        0x91 => 222,
        0x92 => 222,
        0x93 => 333,
        0x94 => 333,
        0x95 => 350,
        0x96 => 556,
        0x97 => 1000,
        0x99 => 1000,
        0x9c => 944,
        0xa0 => 278,
        0xa1 => 333,
        0xa6 => 260,
        0xa7 => 556,
        0xa9 => 737,
        0xaa => 370,
        0xab => 556,
        0xac => 584,
        0xad => 333,
        0xae => 737,
        0xb0 => 400,
        0xb1 => 584,
        0xb2 => 333,
        0xb3 => 333,
        0xb4 => 333,
        0xb5 => 556,
        0xb6 => 537,
        0xb7 => 278,
        0xb8 => 333,
        0xb9 => 333,
        0xba => 365,
        0xbb => 556,
        0xbc => 834,
        0xbd => 834,
        0xbe => 834,
        0xbf => 611,
        0xc6 => 1000,
        0xd7 => 584,
        0xd8 => 778,
        0xdf => 611,
        0xe6 => 889,
        0xf7 => 584,
        0xf8 => 611,
        _ => return None,
    })
}
/// `SUBST[ch] || ch`.
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

/// `charAdvance(ch)` in 1/1000 em.
#[allow(clippy::arithmetic_side_effects)] // bounded counters and indices (page numbers, padding, digit math)
pub fn char_advance(c: char) -> u32 {
    let c = subst(c);
    let code = u32::from(c);
    if (32..127).contains(&code) {
        return HELV.get((code - 32) as usize).copied().unwrap_or(556);
    }
    let w = match cp1252(c) {
        Some(k) => wide(k),
        None => wide(code),
    };
    if let Some(w) = w {
        return w;
    }
    // ch.normalize('NFD')[0]: the first UTF-16 unit of the decomposition
    let b = c.to_string().nfd().next().map_or(code, u32::from);
    let b = if b > 0xffff { 0xd800 + ((b - 0x10000) >> 10) } else { b };
    if (32..127).contains(&b) {
        return HELV.get((b - 32) as usize).copied().unwrap_or(556);
    }
    if code >= 0x2e80 { 1000 } else { 556 }
}

/// `advance(s)`.
pub fn advance(s: &str) -> u32 {
    s.chars().map(char_advance).fold(0u32, u32::saturating_add)
}
