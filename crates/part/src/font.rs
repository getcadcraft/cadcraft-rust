//! font.js — Helvetica metrics for PartCraft's drawing PDFs (widths in 1/1000 em; the PDF uses the reader's built-in
//! Helvetica, WinAnsi): `charAdvance`, `advance`, the Windows-1252 table and the substitutions.

use unicode_normalization::UnicodeNormalization;

const HELV: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278,
    278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722,
    667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556,
    556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

/// `CP1252[ch]`: Windows-1252 codes 0x80–0x9F.
pub fn cp1252(ch: char) -> Option<u32> {
    Some(match ch {
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

fn wide(code: u32) -> Option<f64> {
    Some(match code {
        0x80 => 556.0,
        0x85 | 0x89 | 0x8c | 0x97 | 0x99 | 0xc6 => 1000.0,
        0x91 | 0x92 => 222.0,
        0x93 | 0x94 | 0xa1 | 0xad | 0xb2 | 0xb3 | 0xb4 | 0xb8 | 0xb9 => 333.0,
        0x95 => 350.0,
        0x96 | 0xa7 | 0xab | 0xb5 | 0xbb => 556.0,
        0x9c => 944.0,
        0xa0 | 0xb7 => 278.0,
        0xa6 => 260.0,
        0xa9 | 0xae => 737.0,
        0xaa => 370.0,
        0xac | 0xb1 | 0xd7 | 0xf7 => 584.0,
        0xb0 => 400.0,
        0xb6 => 537.0,
        0xba => 365.0,
        0xbc..=0xbe => 834.0,
        0xbf | 0xdf | 0xf8 => 611.0,
        0xd8 => 778.0,
        0xe6 => 889.0,
        _ => return None,
    })
}

/// `SUBST[ch] || ch`.
pub fn subst(ch: char) -> char {
    match ch {
        '⌀' => 'Ø',
        '−' => '-',
        '′' => '\'',
        '″' => '"',
        '\t' => ' ',
        c => c,
    }
}

fn helv(code: u32) -> Option<f64> {
    if (32..127).contains(&code) { HELV.get(usize::try_from(code.saturating_sub(32)).ok()?).map(|&w| f64::from(w)) } else { None }
}

/// `charAdvance(ch)`.
pub fn char_advance(ch: char) -> f64 {
    let ch = subst(ch);
    let code = u32::from(ch);
    if let Some(w) = helv(code) {
        return w;
    }
    let w = match cp1252(ch) {
        Some(c) => wide(c),
        None => wide(code),
    };
    if let Some(w) = w {
        return w;
    }
    if let Some(b) = std::iter::once(ch).nfd().next()
        && let Some(w) = helv(u32::from(b))
    {
        return w;
    }
    if code >= 0x2e80 { 1000.0 } else { 556.0 }
}

/// `advance(s)`.
pub fn advance(s: &str) -> f64 {
    s.chars().map(char_advance).sum()
}
