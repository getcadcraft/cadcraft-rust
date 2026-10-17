//! font.js — Helvetica and Helvetica-Bold advance widths (1/1000 em), text width, word wrapping, and the WinAnsi
//! (Windows-1252) encoder for text written into PDF appearance streams.

use crate::js::{is_ws, round, trim, trim_end};
use crate::nfd::NFD_ASCII;

/// Advance widths for printable ASCII 0x20–0x7E.
const HELV: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278,
    278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722,
    667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556,
    556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];
const HELV_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 333,
    333, 584, 584, 584, 611, 975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778, 722, 667, 611, 722,
    667, 944, 667, 667, 611, 333, 278, 333, 584, 556, 333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611, 611,
    611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];

/// Windows-1252 codes 0x80–0x9F (the PDF's WinAnsiEncoding).
pub fn cp1252(c: char) -> Option<u8> {
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

fn wide(code: u32) -> Option<u16> {
    Some(match code {
        0x80 => 556,
        0x85 | 0x89 | 0x8c | 0x97 | 0x99 | 0xc6 => 1000,
        0x91 | 0x92 => 222,
        0x93 | 0x94 => 333,
        0x95 => 350,
        0x96 => 556,
        0x9c => 944,
        0xa0 => 278,
        0xa1 => 333,
        0xa6 => 260,
        0xa7 => 556,
        0xa9 | 0xae => 737,
        0xaa => 370,
        0xab => 556,
        0xac => 584,
        0xad => 333,
        0xb0 => 400,
        0xb1 => 584,
        0xb2..=0xb4 => 333,
        0xb5 => 556,
        0xb6 => 537,
        0xb7 => 278,
        0xb8 | 0xb9 => 333,
        0xba => 365,
        0xbb => 556,
        0xbc..=0xbe => 834,
        0xbf => 611,
        0xd7 => 584,
        0xd8 => 778,
        0xdf => 611,
        0xe6 => 889,
        0xf7 => 584,
        0xf8 => 611,
        _ => return None,
    })
}

/// Characters drawn as a close Windows-1252 one.
pub fn subst(c: char) -> char {
    match c {
        '\u{2300}' => '\u{d8}',
        '\u{2212}' => '-',
        '\u{2032}' => '\'',
        '\u{2033}' => '"',
        '\t' | '\u{a0}' => ' ',
        c => c,
    }
}

/// `ch.normalize('NFD')[0]` when that is printable ASCII.
fn nfd_ascii(c: char) -> Option<u32> {
    let cp = c as u32;
    if (32..127).contains(&cp) {
        return Some(cp);
    }
    NFD_ASCII.binary_search_by(|(k, _)| k.cmp(&cp)).ok().and_then(|i| NFD_ASCII.get(i)).map(|(_, b)| u32::from(*b))
}

fn table(bold: bool, code: u32) -> f64 {
    let t = if bold { &HELV_BOLD } else { &HELV };
    let i = usize::try_from(code.saturating_sub(32)).unwrap_or(0);
    f64::from(t.get(i).copied().unwrap_or(556))
}

/// `charAdvance(ch, bold)`.
pub fn char_advance(ch: char, bold: bool) -> f64 {
    let ch = subst(ch);
    let code = ch as u32;
    if (32..127).contains(&code) {
        return table(bold, code);
    }
    let w = match cp1252(ch) {
        Some(b) => wide(u32::from(b)),
        None => wide(code),
    };
    if let Some(w) = w {
        return if bold { round(f64::from(w) * 1.04) } else { f64::from(w) };
    }
    if let Some(b) = nfd_ascii(ch) {
        return table(bold, b);
    }
    if code >= 0x2e80 { 1000.0 } else { 556.0 }
}

/// `textWidth(s, size, bold)`.
pub fn text_width(s: &str, size: f64, bold: bool) -> f64 {
    let mut w = 0.0;
    for ch in s.chars() {
        w += char_advance(ch, bold);
    }
    (w / 1000.0) * size
}

/// `s.split(/(\s+)/)` without the empty strings: runs of whitespace and of other characters, in order.
fn ws_tokens(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut cur: Option<bool> = None;
    for (i, c) in s.char_indices() {
        let w = is_ws(c);
        match cur {
            Some(k) if k == w => {}
            Some(_) => {
                out.push(s.get(start..i).unwrap_or(""));
                start = i;
                cur = Some(w);
            }
            None => cur = Some(w),
        }
    }
    if start < s.len() {
        out.push(s.get(start..).unwrap_or(""));
    }
    out
}

/// `wrapText(text, size, maxWidth, bold)`: lines that fit (words wrap; a word wider than a line breaks by character);
/// hard line breaks kept; `maxWidth <= 0` (or NaN) → no wrapping.
pub fn wrap_text(text: &str, size: f64, max_width: f64, bold: bool) -> Vec<String> {
    let norm = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = Vec::new();
    for para in norm.split('\n') {
        if !(max_width > 0.0) {
            out.push(para.to_string());
            continue;
        }
        let fits = |s: &str| text_width(s, size, bold) <= max_width;
        let mut line = String::new();
        for tok in ws_tokens(para) {
            if tok.chars().all(is_ws) {
                if !line.is_empty() {
                    line.push_str(tok);
                }
                continue;
            }
            let joined = format!("{line}{tok}");
            if fits(trim_end(&joined)) {
                line = joined;
                continue;
            }
            if !trim(&line).is_empty() {
                out.push(trim_end(&line).to_string());
                line.clear();
            }
            if fits(tok) {
                line = tok.to_string();
                continue;
            }
            for ch in tok.chars() {
                if !line.is_empty() {
                    let mut probe = line.clone();
                    probe.push(ch);
                    if !fits(&probe) {
                        out.push(std::mem::take(&mut line));
                    }
                }
                line.push(ch);
            }
        }
        out.push(trim_end(&line).to_string());
    }
    out
}

/// `winAnsiByte(ch)`: the WinAnsi byte, or 63 ('?') when the built-in fonts cannot show it.
pub fn win_ansi_byte(ch: char) -> u8 {
    let ch = subst(ch);
    if let Some(b) = cp1252(ch) {
        return b;
    }
    let code = ch as u32;
    if (32..127).contains(&code) || (0xa0..=0xff).contains(&code) {
        return u8::try_from(code).unwrap_or(63);
    }
    match nfd_ascii(ch) {
        Some(b) => u8::try_from(b).unwrap_or(63),
        None => 63,
    }
}

/// `winAnsiHex(s)` → (`<hex>`, how many characters became '?').
pub fn win_ansi_hex(s: &str) -> (String, usize) {
    use std::fmt::Write as _;
    let mut hex = String::with_capacity(s.len().saturating_mul(2).saturating_add(2));
    hex.push('<');
    let mut missing = 0usize;
    for ch in s.chars() {
        let b = win_ansi_byte(ch);
        if b == 63 && ch != '?' {
            missing = missing.saturating_add(1);
        }
        let _ = write!(hex, "{b:02x}");
    }
    hex.push('>');
    (hex, missing)
}

/// The bytes of `winAnsiHex` (what a PDF reader decodes the hex string to).
pub fn win_ansi_bytes(s: &str) -> Vec<u8> {
    s.chars().map(win_ansi_byte).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metrics() {
        assert!((text_width("Hello", 10.0, false) - 22.78).abs() < 1e-9);
        assert!(text_width("APPROVED", 10.0, true) > text_width("APPROVED", 10.0, false));
        let w = wrap_text("Move prep sink six inches east and coordinate with plumbing", 12.0, 120.0, false);
        assert!(w.len() > 1 && w.iter().all(|l| text_width(l, 12.0, false) <= 120.01));
        assert_eq!(wrap_text("a\nb", 12.0, 0.0, false).len(), 2);
        assert!(wrap_text("Supercalifragilisticexpialidocious", 12.0, 40.0, false).iter().all(|l| text_width(l, 12.0, false) <= 40.01));
        assert_eq!(win_ansi_hex("Café – “ok” ✓"), ("<436166e9209620936f6b94203f>".to_string(), 1));
    }
}
