//! Text escapes, ported from the regular expressions of dxf.js (`decodeText`, `mtextPlain`, `encodeText`) and
//! dwg-import.js (`plain`), one left-to-right global replace at a time, exactly as `String.prototype.replace` runs them.
//!
//! Strings are UTF-8 here; a `\U+XXXX` escape that names half of a UTF-16 pair is joined with its other half when the
//! two escapes are adjacent (as JS joins the code units). A lone half becomes U+FFFD (JS keeps the lone surrogate,
//! which no UTF-8 string can hold: a known difference, only reachable with malformed text).

/// One global replace: at each position `m(rest)` → Some((matched byte length, replacement)) or None (advance one char).
fn replace_all(s: &str, m: impl Fn(&str) -> Option<(usize, String)>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < s.len() {
        let rest = s.get(i..).unwrap_or("");
        if let Some((n, rep)) = m(rest) {
            out.push_str(&rep);
            i = i.saturating_add(n);
            if n == 0 {
                // an empty match: keep the char and move on (none of the patterns here match empty)
                if let Some(c) = rest.chars().next() {
                    out.push(c);
                    i = i.saturating_add(c.len_utf8());
                }
            }
            continue;
        }
        match rest.chars().next() {
            Some(c) => {
                out.push(c);
                i = i.saturating_add(c.len_utf8());
            }
            None => break,
        }
    }
    out
}

/// `%%` + one of `letters` (ASCII) at the start of `rest`.
fn pct(rest: &str, letters: &[u8], rep: &str) -> Option<(usize, String)> {
    let b = rest.as_bytes();
    if b.first() == Some(&b'%') && b.get(1) == Some(&b'%') && b.get(2).is_some_and(|c| letters.contains(c)) {
        return Some((3, rep.to_string()));
    }
    None
}

/// `.replace(/\\U\+([0-9a-fA-F]{4})/g, (_, h) => String.fromCharCode(parseInt(h, 16)))`
fn unicode_escapes(s: &str) -> String {
    if !s.contains("\\U+") {
        return s.to_string();
    }
    let mut units: Vec<u16> = Vec::with_capacity(s.len());
    let mut i = 0usize;
    let b = s.as_bytes();
    while i < s.len() {
        if b.get(i) == Some(&b'\\') && b.get(i.saturating_add(1)) == Some(&b'U') && b.get(i.saturating_add(2)) == Some(&b'+') {
            let hex = s.get(i.saturating_add(3)..i.saturating_add(7));
            if let Some(h) = hex.filter(|h| h.len() == 4 && h.bytes().all(|c| c.is_ascii_hexdigit()))
                && let Ok(u) = u16::from_str_radix(h, 16)
            {
                units.push(u);
                i = i.saturating_add(7);
                continue;
            }
        }
        let Some(c) = s.get(i..).and_then(|r| r.chars().next()) else { break };
        let mut buf = [0u16; 2];
        units.extend_from_slice(c.encode_utf16(&mut buf));
        i = i.saturating_add(c.len_utf8());
    }
    String::from_utf16_lossy(&units)
}

/// dxf.js `decodeText`: %%c ⌀, %%d °, %%p ±, %%u/%%o/%%k toggles dropped, %%% → %, \U+XXXX.
pub fn decode_text(s: &str) -> String {
    let s = replace_all(s, |r| pct(r, b"cC", "\u{2300}"));
    let s = replace_all(&s, |r| pct(r, b"dD", "\u{b0}"));
    let s = replace_all(&s, |r| pct(r, b"pP", "\u{b1}"));
    let s = replace_all(&s, |r| pct(r, b"uUoOkK", ""));
    let s = replace_all(&s, |r| r.starts_with("%%%").then(|| (3, "%".to_string())));
    unicode_escapes(&s)
}

/// dwg-import.js `plain`: as decode_text, but %%k is kept.
pub fn plain(s: &str) -> String {
    let s = replace_all(s, |r| pct(r, b"cC", "\u{2300}"));
    let s = replace_all(&s, |r| pct(r, b"dD", "\u{b0}"));
    let s = replace_all(&s, |r| pct(r, b"pP", "\u{b1}"));
    let s = replace_all(&s, |r| pct(r, b"uUoO", ""));
    let s = replace_all(&s, |r| r.starts_with("%%%").then(|| (3, "%".to_string())));
    unicode_escapes(&s)
}

/// dxf.js `mtextPlain`: MTEXT formatting codes → plain text with \n line breaks.
pub fn mtext_plain(s: &str) -> String {
    let s = decode_text(s);
    let s = replace_all(&s, |r| r.starts_with("\\P").then(|| (2, "\n".to_string())));
    // /\\[fFcCHhQqTtWwAap][^;]*;/g
    let s = replace_all(&s, |r| {
        let b = r.as_bytes();
        if b.first() != Some(&b'\\') || !b.get(1).is_some_and(|c| b"fFcCHhQqTtWwAap".contains(c)) {
            return None;
        }
        let semi = r.get(2..)?.find(';')?;
        Some((semi.saturating_add(3), String::new()))
    });
    // /\^I|\t/g
    let s = replace_all(&s, |r| {
        if r.starts_with("^I") {
            Some((2, " ".to_string()))
        } else if r.starts_with('\t') {
            Some((1, " ".to_string()))
        } else {
            None
        }
    });
    // /\\S([^;^/#]*)[\^/#]([^;]*);/g → '$1/$2'
    let s = replace_all(&s, |r| {
        if !r.starts_with("\\S") {
            return None;
        }
        let body = r.get(2..)?;
        let k = body.find([';', '^', '/', '#'])?;
        let sep = body.as_bytes().get(k)?;
        if *sep == b';' {
            return None;
        }
        let g1 = body.get(..k)?;
        let after = body.get(k.saturating_add(1)..)?;
        let e = after.find(';')?;
        let g2 = after.get(..e)?;
        Some((2usize.saturating_add(k).saturating_add(1).saturating_add(e).saturating_add(1), format!("{g1}/{g2}")))
    });
    // /\\[LlOoKkX]/g
    let s = replace_all(&s, |r| {
        let b = r.as_bytes();
        (b.first() == Some(&b'\\') && b.get(1).is_some_and(|c| b"LlOoKkX".contains(c))).then(|| (2, String::new()))
    });
    let s = replace_all(&s, |r| r.starts_with("\\~").then(|| (2, " ".to_string())));
    let s = replace_all(&s, |r| r.starts_with("\\\\").then(|| (2, "\u{0}".to_string())));
    let s: String = s.chars().filter(|c| *c != '{' && *c != '}').collect();
    let s = s.replace('\u{0}', "\\");
    cc_cad::js::trim(&s).to_string()
}

/// dxf.js `encodeText`: plain text → R12 DXF text (non-ASCII as \U+XXXX per UTF-16 unit, the symbols back to %% codes).
pub fn encode_text(s: &str) -> String {
    let s = s.replace('\u{2300}', "%%c").replace('\u{b0}', "%%d").replace('\u{b1}', "%%p");
    let mut out = String::with_capacity(s.len());
    for u in s.encode_utf16() {
        if (0x20..=0x7e).contains(&u) {
            out.push(char::from(u8::try_from(u).unwrap_or(b'?')));
        } else {
            out.push_str(&format!("\\U+{u:04X}"));
        }
    }
    out
}

/// attrs.js `oneLine`: `/[\r\n]+/g` → ' '.
pub fn one_line(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_run = false;
    for c in s.chars() {
        if c == '\r' || c == '\n' {
            if !in_run {
                out.push(' ');
            }
            in_run = true;
        } else {
            in_run = false;
            out.push(c);
        }
    }
    out
}

/// `s.replace(/^<>$/, '')`.
pub fn drop_measure_marker(s: String) -> String {
    if s == "<>" { String::new() } else { s }
}

/// JS `s.slice(a, b)` in UTF-16 code units (pairs cut in half become U+FFFD, a known difference).
pub fn slice16(s: &str, a: usize, b: usize) -> String {
    let u: Vec<u16> = s.encode_utf16().collect();
    let b = b.min(u.len());
    let a = a.min(b);
    String::from_utf16_lossy(u.get(a..b).unwrap_or(&[]))
}
/// JS `s.length` (UTF-16 code units).
pub fn len16(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escapes() {
        assert_eq!(decode_text("%%c10 %%d %%p1 %%uX%%u %%%"), "\u{2300}10 \u{b0} \u{b1}1 X %");
        assert_eq!(decode_text("%%%c"), "%\u{2300}");
        assert_eq!(decode_text("\\U+00e9\\U+D83D\\U+DE00"), "\u{e9}\u{1F600}");
        assert_eq!(mtext_plain("{\\fArial|b0;Hello}\\PWorld \\S1/2; x\\~y \\\\"), "Hello\nWorld 1/2 x y \\");
        assert_eq!(mtext_plain("\\H2.5x;\\A1;Text"), "Text");
        assert_eq!(mtext_plain("\\S1#4;"), "1/4");
        assert_eq!(encode_text("\u{2300}5 \u{e9}"), "%%c5 \\U+00E9");
        assert_eq!(encode_text("\u{1F600}"), "\\U+D83D\\U+DE00");
        assert_eq!(one_line("a\r\n\nb\rc"), "a b c");
    }
}
