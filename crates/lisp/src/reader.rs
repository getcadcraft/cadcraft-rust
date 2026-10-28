//! LISP source text → forms (reader.js), without recursion: a list nested a hundred thousand deep reads with a work
//! stack, not the call stack.
//!
//! Syntax (AutoLISP reference): `( ) ' . "strings"` with `\\ \" \n \r \t \e` and `\nnn` (octal), `;` comments to the
//! end of the line and `;| inline |;` comments, integers (32-bit; larger ones read as reals), reals (`1.` `.5` `1e3`),
//! symbols (any other run of characters, read in upper case). A `( … . x)` dotted pair needs spaces round the dot.

use crate::syms::Syms;
use crate::value::{LErr, MSG_STACK, R, V, from_vec, lstr};

/// The deepest nesting the reader takes (0.5's reader recursed until V8's stack ran out).
pub const MAX_DEPTH: usize = 100_000;

/// JS `\s` (WhiteSpace and LineTerminator) on one UTF-16 unit.
pub fn is_ws(c: u16) -> bool {
    matches!(c, 0x09..=0x0D | 0x20 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000 | 0xFEFF)
}
fn is_delim(c: u16) -> bool {
    is_ws(c) || matches!(c, 0x28 | 0x29 | 0x27 | 0x22 | 0x3B)
}

/// UTF-8, or Windows-1252 when the bytes are not UTF-8 (older .lsp files); a leading BOM dropped.
pub fn decode_source(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => {
            // TextDecoder drops one BOM, the reader's replace another
            let s = s.strip_prefix('\u{FEFF}').unwrap_or(s);
            s.strip_prefix('\u{FEFF}').unwrap_or(s).to_string()
        }
        Err(_) => bytes.iter().map(|&b| cp1252(b)).collect(),
    }
}

/// WHATWG windows-1252: bytes 0x80–0x9F to their characters (the five unassigned ones to C1 controls).
fn cp1252(b: u8) -> char {
    const HI: [u16; 32] = [
        0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x8D, 0x017D, 0x8F, 0x90,
        0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
    ];
    if (0x80..0xA0).contains(&b) {
        let k = usize::from(b.saturating_sub(0x80));
        return HI.get(k).and_then(|&u| char::from_u32(u32::from(u))).unwrap_or('\u{FFFD}');
    }
    char::from(b)
}

enum Open {
    List(Vec<V>),
    /// after `( a b .`: waiting for the tail
    Dot(Vec<V>),
    Quote,
}

/// The reader over one text.
pub struct Src<'a> {
    s: &'a [u16],
    pub i: usize,
}

impl<'a> Src<'a> {
    pub fn new(s: &'a [u16]) -> Self {
        Src { s, i: 0 }
    }
    fn at(&self, k: usize) -> Option<u16> {
        self.s.get(k).copied()
    }
    fn end(&self) -> bool {
        self.i >= self.s.len()
    }
    /// Skip white space and comments.
    pub fn skip(&mut self) {
        loop {
            while self.at(self.i).is_some_and(is_ws) {
                self.i = self.i.saturating_add(1);
            }
            if self.at(self.i) != Some(0x3B) {
                return;
            }
            if self.at(self.i.saturating_add(1)) == Some(0x7C) {
                // ;| inline comment |;
                let from = self.i.saturating_add(2);
                let end = (from..self.s.len().saturating_sub(1))
                    .find(|&k| self.at(k) == Some(0x7C) && self.at(k.saturating_add(1)) == Some(0x3B));
                self.i = end.map_or(self.s.len(), |e| e.saturating_add(2));
            } else {
                while self.at(self.i).is_some_and(|c| c != 0x0A) {
                    self.i = self.i.saturating_add(1);
                }
            }
        }
    }

    /// The next form; None at the end.
    pub fn form(&mut self, syms: &mut Syms) -> R<Option<V>> {
        let mut stack: Vec<Open> = Vec::new();
        loop {
            self.skip();
            if self.end() {
                if stack.is_empty() {
                    return Ok(None);
                }
                return Err(LErr::incomplete("malformed list on input"));
            }
            let ch = self.at(self.i).unwrap_or(0);
            let mut value: V;
            match stack.last_mut() {
                Some(Open::List(items)) if ch == 0x29 => {
                    self.i = self.i.saturating_add(1);
                    value = from_vec(std::mem::take(items), V::Nil);
                    stack.pop();
                }
                Some(Open::List(items)) if ch == 0x2E && !items.is_empty() && self.at(self.i.saturating_add(1)).is_none_or(is_delim) => {
                    self.i = self.i.saturating_add(1);
                    let items = std::mem::take(items);
                    stack.pop();
                    stack.push(Open::Dot(items));
                    continue;
                }
                _ => {
                    if ch == 0x28 {
                        self.i = self.i.saturating_add(1);
                        if stack.len() >= MAX_DEPTH {
                            return Err(LErr::new(MSG_STACK));
                        }
                        stack.push(Open::List(Vec::new()));
                        continue;
                    }
                    if ch == 0x29 {
                        self.i = self.i.saturating_add(1);
                        if stack.is_empty() {
                            return Err(LErr::new("extra right paren on input"));
                        }
                        return Err(LErr::incomplete("malformed list on input"));
                    }
                    if ch == 0x27 {
                        self.i = self.i.saturating_add(1);
                        if stack.len() >= MAX_DEPTH {
                            return Err(LErr::new(MSG_STACK));
                        }
                        stack.push(Open::Quote);
                        continue;
                    }
                    value = if ch == 0x22 { V::Str(self.string()?.into()) } else { self.token(syms)? };
                }
            }
            // hand the value to what is open
            loop {
                match stack.pop() {
                    None => return Ok(Some(value)),
                    Some(Open::List(mut items)) => {
                        items.push(value);
                        stack.push(Open::List(items));
                        break;
                    }
                    Some(Open::Quote) => value = V::cons(V::Sym(syms.quote()), V::cons(value, V::Nil)),
                    Some(Open::Dot(items)) => {
                        self.skip();
                        if self.at(self.i) != Some(0x29) {
                            return Err(LErr::new("malformed list on input"));
                        }
                        self.i = self.i.saturating_add(1);
                        value = from_vec(items, value);
                    }
                }
            }
        }
    }

    fn string(&mut self) -> R<Vec<u16>> {
        let mut out: Vec<u16> = Vec::new();
        self.i = self.i.saturating_add(1);
        loop {
            let Some(c) = self.at(self.i) else { return Err(LErr::incomplete("malformed string on input")) };
            self.i = self.i.saturating_add(1);
            if c == 0x22 {
                return Ok(out);
            }
            if c != 0x5C {
                out.push(c);
                continue;
            }
            let e = self.at(self.i);
            self.i = self.i.saturating_add(1);
            match e {
                Some(0x6E) => out.push(0x0A),
                Some(0x72) => out.push(0x0D),
                Some(0x74) => out.push(0x09),
                Some(0x65) => out.push(0x1B),
                Some(d @ 0x30..=0x37) => {
                    let mut v = u32::from(d.saturating_sub(0x30));
                    let mut n = 1_u32;
                    while n < 3 {
                        match self.at(self.i) {
                            Some(x @ 0x30..=0x37) => {
                                v = v.saturating_mul(8).saturating_add(u32::from(x.saturating_sub(0x30)));
                                self.i = self.i.saturating_add(1);
                                n = n.saturating_add(1);
                            }
                            _ => break,
                        }
                    }
                    // at most 0o777 = 511
                    out.push(u16::try_from(v).unwrap_or(0));
                }
                Some(x) => out.push(x),
                None => {}
            }
        }
    }

    fn token(&mut self, syms: &mut Syms) -> R<V> {
        let mut j = self.i;
        while self.at(j).is_some_and(|c| !is_delim(c)) {
            j = j.saturating_add(1);
        }
        let tok = self.s.get(self.i..j).unwrap_or(&[]);
        self.i = j;
        atom(tok, syms)
    }

    /// The text after the reader's position.
    pub fn rest(&self) -> &'a [u16] {
        self.s.get(self.i..).unwrap_or(&[])
    }
}

fn is_digit(c: u16) -> bool {
    (0x30..=0x39).contains(&c)
}

/// /^[+-]?\d+$/
fn int_like(t: &[u16]) -> bool {
    let b = if matches!(t.first(), Some(0x2B | 0x2D)) { t.get(1..).unwrap_or(&[]) } else { t };
    !b.is_empty() && b.iter().all(|&c| is_digit(c))
}
/// /^[+-]?(?:\d+\.\d*|\.\d+|\d+)(?:e[+-]?\d+)?$/i
fn real_like(t: &[u16]) -> bool {
    let mut i = usize::from(matches!(t.first(), Some(0x2B | 0x2D)));
    let digits = |i: &mut usize| {
        let s = *i;
        while t.get(*i).copied().is_some_and(is_digit) {
            *i = (*i).saturating_add(1);
        }
        *i > s
    };
    let int = digits(&mut i);
    if t.get(i) == Some(&0x2E) {
        i = i.saturating_add(1);
        let frac = digits(&mut i);
        if !int && !frac {
            return false;
        }
    } else if !int {
        return false;
    }
    if matches!(t.get(i), Some(0x65 | 0x45)) {
        i = i.saturating_add(1);
        if matches!(t.get(i), Some(0x2B | 0x2D)) {
            i = i.saturating_add(1);
        }
        if !digits(&mut i) {
            return false;
        }
    }
    i == t.len()
}

/// A token's atom: integer, real, nil or a symbol.
pub fn atom(tok: &[u16], syms: &mut Syms) -> R<V> {
    let text = String::from_utf16_lossy(tok);
    if int_like(tok) {
        let n = cc_cad::js::str_to_number(&text);
        if (-2_147_483_648.0..=2_147_483_647.0).contains(&n) {
            return Ok(V::Int(crate::value::to_i32(n)));
        }
        return Ok(V::Real(n));
    }
    if real_like(tok) {
        return Ok(V::Real(cc_cad::js::str_to_number(&text)));
    }
    if text.eq_ignore_ascii_case("nil") {
        return Ok(V::Nil);
    }
    Ok(V::Sym(syms.intern(&text)?))
}

/// Every form of a text (load, APPLOAD); a broken text is an error (nothing of it runs).
pub fn read_all(text: &[u16], syms: &mut Syms) -> R<Vec<V>> {
    let mut src = Src::new(text);
    let mut out = Vec::new();
    loop {
        match src.form(syms)? {
            Some(f) => out.push(f),
            None => {
                src.skip();
                if src.end() {
                    break;
                }
            }
        }
    }
    Ok(out)
}

/// The first form only (the read function) and the text after it.
pub fn read_one(text: &[u16], syms: &mut Syms) -> R<(V, Vec<u16>)> {
    let mut src = Src::new(text);
    let f = src.form(syms)?;
    Ok((f.unwrap_or(V::Nil), src.rest().to_vec()))
}

/// Is this text a complete set of forms (the command line keeps reading lines until the parens balance)?
pub fn balanced(text: &str) -> bool {
    let mut syms = Syms::new();
    match read_all(&lstr(text), &mut syms) {
        Ok(_) => true,
        Err(e) => !e.incomplete,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(s: &str) -> R<Vec<V>> {
        let mut syms = Syms::new();
        read_all(&lstr(s), &mut syms)
    }

    #[test]
    fn errors_as_reader_js() {
        let err = |s: &str| read(s).err().map(|e| (e.msg, e.incomplete));
        assert_eq!(err("(a b"), Some(("malformed list on input".into(), true)));
        assert_eq!(err(")").map(|e| e.0), Some("extra right paren on input".into()));
        assert_eq!(err("(a . b c)"), Some(("malformed list on input".into(), false)));
        assert_eq!(err("\"abc"), Some(("malformed string on input".into(), true)));
        assert!(!balanced("(defun f ("));
        assert!(balanced("(+ 1 2) ; done"));
        assert!(balanced(")"));
    }

    #[test]
    fn deep_nesting_reads_without_recursion() {
        let s = "(".repeat(50_000) + &")".repeat(50_000);
        assert_eq!(read(&s).map(|v| v.len()), Ok(1));
    }

    #[test]
    fn atoms() {
        let mut syms = Syms::new();
        let v = read_all(&lstr("1 -0 2147483648 1. .5 1e3 nil abc +5 ."), &mut syms).unwrap_or_default();
        assert!(matches!(v.first(), Some(V::Int(1))));
        assert!(matches!(v.get(1), Some(V::Int(0))));
        assert!(matches!(v.get(2), Some(V::Real(x)) if *x == 2_147_483_648.0));
        assert!(matches!(v.get(3), Some(V::Real(x)) if *x == 1.0));
        assert!(matches!(v.get(5), Some(V::Real(x)) if *x == 1000.0));
        assert!(matches!(v.get(6), Some(V::Nil)));
        assert!(matches!(v.get(8), Some(V::Int(5))));
        assert!(matches!(v.get(9), Some(V::Sym(_))));
    }
}
