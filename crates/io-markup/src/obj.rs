//! PDF objects as MarkupCraft's writer (pdf-lib's role) holds them: an owned tree, the lexer that reads them, the
//! text they are written back as, and the text decoders (PDFDocEncoding, UTF-16BE) pdf-lib's `decodeText` uses.
//!
//! Strings keep their source form (a literal string's escaped bytes, a hex string's hex text) and are written back
//! unchanged, as pdf-lib does; names are kept decoded (one char per byte, `#xx` resolved) and escaped on writing.

use std::fmt::Write as _;

/// A PDF object.
#[derive(Debug, Clone, PartialEq)]
pub enum Obj {
    Null,
    Bool(bool),
    Num(f64),
    /// a literal string's contents between the parentheses, as written (escapes not resolved)
    Str(Vec<u8>),
    /// a hex string's text between the angle brackets, as written
    Hex(Vec<u8>),
    /// a string's bytes, already decoded (what the reading side hands over)
    Bytes(Vec<u8>),
    /// a name, decoded (each byte one char)
    Name(String),
    Arr(Vec<Obj>),
    Dict(Dict),
    Ref(u32, u16),
    Stream(Box<Stream>),
}

/// A dictionary (keys decoded, in source order).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Dict(pub Vec<(String, Obj)>);

/// A stream: its dictionary and its raw (still encoded) bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    pub dict: Dict,
    pub data: Vec<u8>,
}

impl Dict {
    pub fn new() -> Self {
        Dict(Vec::new())
    }
    pub fn get(&self, k: &str) -> Option<&Obj> {
        self.0.iter().find(|(x, _)| x == k).map(|(_, v)| v)
    }
    pub fn has(&self, k: &str) -> bool {
        self.0.iter().any(|(x, _)| x == k)
    }
    /// Set a key (keeps its place when present, else appends — a JS Map's set).
    pub fn set(&mut self, k: &str, v: Obj) {
        match self.0.iter_mut().find(|(x, _)| x == k) {
            Some(slot) => slot.1 = v,
            None => self.0.push((k.to_string(), v)),
        }
    }
    pub fn delete(&mut self, k: &str) {
        self.0.retain(|(x, _)| x != k);
    }
    pub fn with(mut self, k: &str, v: Obj) -> Self {
        self.set(k, v);
        self
    }
}

impl Obj {
    pub fn name(s: &str) -> Obj {
        Obj::Name(s.to_string())
    }
    pub fn num(x: f64) -> Obj {
        Obj::Num(x)
    }
    pub fn nums(xs: &[f64]) -> Obj {
        Obj::Arr(xs.iter().map(|x| Obj::Num(*x)).collect())
    }
    pub fn as_dict(&self) -> Option<&Dict> {
        match self {
            Obj::Dict(d) => Some(d),
            Obj::Stream(s) => Some(&s.dict),
            _ => None,
        }
    }
    pub fn as_dict_mut(&mut self) -> Option<&mut Dict> {
        match self {
            Obj::Dict(d) => Some(d),
            Obj::Stream(s) => Some(&mut s.dict),
            _ => None,
        }
    }
    pub fn as_name(&self) -> Option<&str> {
        match self {
            Obj::Name(n) => Some(n),
            _ => None,
        }
    }
    pub fn as_num(&self) -> Option<f64> {
        match self {
            Obj::Num(n) => Some(*n),
            _ => None,
        }
    }
    pub fn as_arr(&self) -> Option<&[Obj]> {
        match self {
            Obj::Arr(a) => Some(a),
            _ => None,
        }
    }
    pub fn as_ref(&self) -> Option<(u32, u16)> {
        match self {
            Obj::Ref(n, g) => Some((*n, *g)),
            _ => None,
        }
    }
    /// The bytes a string object stands for (escapes resolved; hex decoded).
    pub fn str_bytes(&self) -> Option<Vec<u8>> {
        match self {
            Obj::Str(raw) => Some(unescape_literal(raw)),
            Obj::Hex(h) => Some(hex_bytes(h)),
            Obj::Bytes(b) => Some(b.clone()),
            _ => None,
        }
    }
    pub fn is_string(&self) -> bool {
        matches!(self, Obj::Str(_) | Obj::Hex(_) | Obj::Bytes(_))
    }
    /// pdf-lib's `decodeText()` of a string (UTF-16BE with a BOM, else PDFDocEncoding).
    pub fn decode_text(&self) -> Option<String> {
        self.str_bytes().map(|b| decode_pdf_text_lib(&b))
    }
    /// A string object holding text as pdf-lib's `PDFHexString.fromText` writes it (UTF-16BE with a BOM).
    pub fn hex_text(s: &str) -> Obj {
        let mut h = String::from("FEFF");
        for u in s.encode_utf16() {
            let _ = write!(h, "{u:04X}");
        }
        Obj::Hex(h.into_bytes())
    }
    /// `PDFString.of(s)`: a literal string with this text as its raw contents (pdf-lib writes it unescaped).
    pub fn lit(s: &str) -> Obj {
        Obj::Str(s.bytes().collect())
    }
}

pub fn is_ws(b: u8) -> bool {
    matches!(b, 0 | 9 | 10 | 12 | 13 | 32)
}
pub fn is_delim(b: u8) -> bool {
    matches!(b, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}

/// A literal string's raw contents → its bytes.
pub fn unescape_literal(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0usize;
    while let Some(&c) = raw.get(i) {
        i = i.saturating_add(1);
        if c != b'\\' {
            // a bare CR or CR LF inside a literal string reads as LF
            if c == b'\r' {
                if raw.get(i) == Some(&b'\n') {
                    i = i.saturating_add(1);
                }
                out.push(b'\n');
            } else {
                out.push(c);
            }
            continue;
        }
        let Some(&e) = raw.get(i) else { break };
        i = i.saturating_add(1);
        match e {
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'(' | b')' | b'\\' => out.push(e),
            b'\r' => {
                if raw.get(i) == Some(&b'\n') {
                    i = i.saturating_add(1);
                }
            }
            b'\n' => {}
            b'0'..=b'7' => {
                let mut v = u32::from(e.wrapping_sub(b'0'));
                for _ in 0..2 {
                    match raw.get(i) {
                        Some(&d @ b'0'..=b'7') => {
                            v = (v << 3) | u32::from(d.wrapping_sub(b'0'));
                            i = i.saturating_add(1);
                        }
                        _ => break,
                    }
                }
                out.push((v & 0xff).to_le_bytes()[0]);
            }
            other => out.push(other),
        }
    }
    out
}

/// A hex string's text → bytes (whitespace ignored, an odd last digit padded with 0).
pub fn hex_bytes(h: &[u8]) -> Vec<u8> {
    let digits: Vec<u8> = h.iter().filter_map(|c| (*c as char).to_digit(16).and_then(|d| u8::try_from(d).ok())).collect();
    digits.chunks(2).map(|p| (p.first().copied().unwrap_or(0) << 4) | p.get(1).copied().unwrap_or(0)).collect()
}

/// PDFDocEncoding's 0x18–0x1F and 0x80–0xAD that differ from Latin-1 (pdf-lib's table).
fn pdfdoc_char(b: u8) -> char {
    const HIGH: [u16; 32] = [
        0x2022, 0x2020, 0x2021, 0x2026, 0x2014, 0x2013, 0x0192, 0x2044, 0x2039, 0x203A, 0x2212, 0x2030, 0x201E, 0x201C, 0x201D, 0x2018,
        0x2019, 0x201A, 0x2122, 0xFB01, 0xFB02, 0x0141, 0x0152, 0x0160, 0x0178, 0x017D, 0x0131, 0x0142, 0x0153, 0x0161, 0x017E, 0xFFFD,
    ];
    const LOW: [u16; 8] = [0x02D8, 0x02C7, 0x02C6, 0x02D9, 0x02DD, 0x02DB, 0x02DA, 0x02DC];
    let u = match b {
        0x18..=0x1f => LOW.get(usize::from(b.wrapping_sub(0x18))).copied().unwrap_or(0xFFFD),
        0x80..=0x9f => HIGH.get(usize::from(b.wrapping_sub(0x80))).copied().unwrap_or(0xFFFD),
        0xa0 => 0x20AC,
        0xad => 0xFFFD,
        _ => u16::from(b),
    };
    char::from_u32(u32::from(u)).unwrap_or('\u{FFFD}')
}

/// UTF-16 code units → text (lone surrogates become U+FFFD: a Rust string cannot hold them).
pub fn utf16_text(units: &[u16]) -> String {
    char::decode_utf16(units.iter().copied()).map(|r| r.unwrap_or('\u{FFFD}')).collect()
}

/// pdf-lib's `decodePDFRawString`: UTF-16BE when it starts with FE FF, else PDFDocEncoding.
pub fn decode_pdf_text_lib(b: &[u8]) -> String {
    if b.len() >= 2 && b.first() == Some(&0xfe) && b.get(1) == Some(&0xff) {
        let units: Vec<u16> = b
            .get(2..)
            .unwrap_or(&[])
            .chunks(2)
            .map(|p| u16::from_be_bytes([p.first().copied().unwrap_or(0), p.get(1).copied().unwrap_or(0)]))
            .collect();
        return utf16_text(&units);
    }
    b.iter().map(|c| pdfdoc_char(*c)).collect()
}

/// pdf-lib's `numberToString(num)`.
pub fn number_to_string(num: f64) -> String {
    use cc_markup::js::{JsMath, ns};
    let s = ns(num);
    if num.abs() < 1.0 {
        if let Some(ep) = s.split("e-").nth(1) {
            let e: i32 = ep.parse().unwrap_or(0);
            if e != 0 {
                let neg = num < 0.0;
                let mut n = if neg { -num } else { num };
                n *= 10f64.jpow(f64::from(e.saturating_sub(1)));
                let digits = ns(n);
                let tail = digits.get(2..).unwrap_or("");
                let zeros = "0".repeat(usize::try_from(e.saturating_sub(1)).unwrap_or(0));
                return format!("{}0.{zeros}{tail}", if neg { "-" } else { "" });
            }
        }
        s
    } else {
        let e: i32 = s.split('+').nth(1).and_then(|x| x.parse().ok()).unwrap_or(0);
        if e > 20 {
            let k = e.saturating_sub(20);
            let n = num / 10f64.jpow(f64::from(k));
            return format!("{}{}", ns(n), "0".repeat(usize::try_from(k).unwrap_or(0)));
        }
        s
    }
}

fn write_name(out: &mut Vec<u8>, n: &str) {
    out.push(b'/');
    for c in n.chars() {
        let b = u32::from(c);
        if !(0x21..=0x7e).contains(&b) || c == '#' || is_delim(u8::try_from(b).unwrap_or(0)) {
            let byte = u8::try_from(b.min(0xff)).unwrap_or(0xff);
            let _ = out.write_fmt(format_args!("#{byte:02X}"));
        } else {
            out.push(u8::try_from(b).unwrap_or(b'?'));
        }
    }
}

/// Write an object (direct form; streams write their dictionary and data).
pub fn write_obj(out: &mut Vec<u8>, o: &Obj, depth: usize) {
    if depth > 512 {
        out.extend_from_slice(b"null");
        return;
    }
    match o {
        Obj::Null => out.extend_from_slice(b"null"),
        Obj::Bool(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        Obj::Num(n) => out.extend_from_slice(number_to_string(*n).as_bytes()),
        Obj::Str(raw) => {
            out.push(b'(');
            out.extend_from_slice(raw);
            out.push(b')');
        }
        Obj::Hex(h) => {
            out.push(b'<');
            out.extend_from_slice(h);
            out.push(b'>');
        }
        Obj::Bytes(b) => {
            out.push(b'<');
            for c in b {
                out.extend_from_slice(format!("{c:02X}").as_bytes());
            }
            out.push(b'>');
        }
        Obj::Name(n) => write_name(out, n),
        Obj::Arr(a) => {
            out.extend_from_slice(b"[ ");
            for x in a {
                write_obj(out, x, depth.saturating_add(1));
                out.push(b' ');
            }
            out.push(b']');
        }
        Obj::Dict(d) => write_dict(out, d, depth),
        Obj::Ref(n, g) => {
            let _ = out.write_fmt(format_args!("{n} {g} R"));
        }
        Obj::Stream(s) => {
            let mut d = s.dict.clone();
            d.set("Length", Obj::Num(s.data.len() as f64));
            write_dict(out, &d, depth);
            out.extend_from_slice(b"\nstream\n");
            out.extend_from_slice(&s.data);
            out.extend_from_slice(b"\nendstream");
        }
    }
}

fn write_dict(out: &mut Vec<u8>, d: &Dict, depth: usize) {
    out.extend_from_slice(b"<<\n");
    for (k, v) in &d.0 {
        write_name(out, k);
        out.push(b' ');
        write_obj(out, v, depth.saturating_add(1));
        out.push(b'\n');
    }
    out.extend_from_slice(b">>");
}

/// Trait-like helper so `out.write_fmt` works on a byte vector.
trait WriteFmt {
    fn write_fmt(&mut self, a: std::fmt::Arguments<'_>) -> std::fmt::Result;
}
impl WriteFmt for Vec<u8> {
    fn write_fmt(&mut self, a: std::fmt::Arguments<'_>) -> std::fmt::Result {
        let s = std::fmt::format(a);
        self.extend_from_slice(s.as_bytes());
        Ok(())
    }
}

// ------------------------------------------------------------------------------------------------- the lexer

/// Nesting limit for arrays and dictionaries in one object (deeper data is refused, not recursed into).
pub const MAX_DEPTH: usize = 64;

/// A cursor over PDF bytes.
pub struct Lexer<'a> {
    pub b: &'a [u8],
    pub pos: usize,
    /// read numbers the way pdf.js's lexer does (digits accumulated in a double, then divided by a power of ten:
    /// not correctly rounded past 15 digits), for content streams read as pdf.js reads them
    pub pdfjs: bool,
}

impl<'a> Lexer<'a> {
    pub fn new(b: &'a [u8]) -> Self {
        Lexer { b, pos: 0, pdfjs: false }
    }
    /// A lexer reading numbers as pdf.js does (content streams of the pdf.js view).
    pub fn pdfjs(b: &'a [u8]) -> Self {
        Lexer { b, pos: 0, pdfjs: true }
    }
    /// pdf.js `Lexer.getNumber`.
    fn pdfjs_number(&mut self) -> f64 {
        let mut sign = 1.0;
        let mut divide_by = 0.0;
        match self.peek() {
            Some(b'-') => {
                sign = -1.0;
                self.pos = self.pos.saturating_add(1);
                if self.peek() == Some(b'-') {
                    self.pos = self.pos.saturating_add(1);
                }
            }
            Some(b'+') => self.pos = self.pos.saturating_add(1),
            _ => {}
        }
        while matches!(self.peek(), Some(b'\n' | b'\r')) {
            self.pos = self.pos.saturating_add(1);
        }
        if self.peek() == Some(b'.') {
            divide_by = 10.0;
            self.pos = self.pos.saturating_add(1);
        }
        let Some(d0) = self.peek().filter(u8::is_ascii_digit) else { return 0.0 };
        let mut base = f64::from(d0.wrapping_sub(b'0'));
        self.pos = self.pos.saturating_add(1);
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                if divide_by != 0.0 {
                    divide_by *= 10.0;
                }
                base = base * 10.0 + f64::from(c.wrapping_sub(b'0'));
            } else if c == b'.' {
                if divide_by == 0.0 {
                    divide_by = 1.0;
                } else {
                    break;
                }
            } else if c != b'-' {
                break;
            }
            self.pos = self.pos.saturating_add(1);
        }
        if divide_by != 0.0 {
            base /= divide_by;
        }
        sign * base
    }
    pub fn peek(&self) -> Option<u8> {
        self.b.get(self.pos).copied()
    }
    pub fn peek_at(&self, k: usize) -> Option<u8> {
        self.b.get(self.pos.saturating_add(k)).copied()
    }
    pub fn done(&self) -> bool {
        self.pos >= self.b.len()
    }
    pub fn bump(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.pos = self.pos.saturating_add(1);
        Some(c)
    }
    pub fn skip_ws(&mut self) {
        while self.peek().is_some_and(is_ws) {
            self.pos = self.pos.saturating_add(1);
        }
    }
    /// Whitespace and % comments.
    pub fn skip_ws_comments(&mut self) {
        loop {
            self.skip_ws();
            if self.peek() == Some(b'%') {
                while let Some(c) = self.peek() {
                    if c == b'\n' || c == b'\r' {
                        break;
                    }
                    self.pos = self.pos.saturating_add(1);
                }
            } else {
                break;
            }
        }
    }
    pub fn starts_with(&self, kw: &[u8]) -> bool {
        self.b.get(self.pos..).is_some_and(|r| r.starts_with(kw))
    }
    /// Consume a keyword when it is next.
    pub fn match_kw(&mut self, kw: &[u8]) -> bool {
        if self.starts_with(kw) {
            self.pos = self.pos.saturating_add(kw.len());
            true
        } else {
            false
        }
    }
    /// pdf-lib's `parseRawInt`.
    pub fn raw_int(&mut self) -> Option<u64> {
        let start = self.pos;
        let mut v: u64 = 0;
        while let Some(c) = self.peek().filter(u8::is_ascii_digit) {
            v = v.saturating_mul(10).saturating_add(u64::from(c.wrapping_sub(b'0')));
            self.pos = self.pos.saturating_add(1);
        }
        if self.pos == start { None } else { Some(v) }
    }
    fn raw_number(&mut self) -> f64 {
        if self.pdfjs {
            return self.pdfjs_number();
        }
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == b'.' || c == b'+' || c == b'-') {
            self.pos = self.pos.saturating_add(1);
        }
        let t = std::str::from_utf8(self.b.get(start..self.pos).unwrap_or(&[])).unwrap_or("");
        cc_markup::js::str_to_number(t)
    }
    fn name(&mut self) -> String {
        // after '/'
        let mut raw: Vec<u8> = Vec::new();
        while let Some(c) = self.peek() {
            if is_ws(c) || is_delim(c) {
                break;
            }
            raw.push(c);
            self.pos = self.pos.saturating_add(1);
        }
        let mut out = String::with_capacity(raw.len());
        let mut i = 0usize;
        while let Some(&c) = raw.get(i) {
            if c == b'#' {
                let h = raw
                    .get(i.saturating_add(1)..i.saturating_add(3))
                    .and_then(|p| std::str::from_utf8(p).ok())
                    .and_then(|p| u8::from_str_radix(p, 16).ok());
                if let Some(v) = h {
                    out.push(char::from(v));
                    i = i.saturating_add(3);
                    continue;
                }
            }
            out.push(char::from(c));
            i = i.saturating_add(1);
        }
        out
    }
    /// Read one object (no stream handling). None when the bytes are not an object.
    pub fn object(&mut self, depth: usize) -> Option<Obj> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.skip_ws_comments();
        let c = self.peek()?;
        match c {
            b'/' => {
                self.pos = self.pos.saturating_add(1);
                Some(Obj::Name(self.name()))
            }
            b'(' => {
                let start = self.pos.saturating_add(1);
                let mut lvl = 0i32;
                let mut esc = false;
                while let Some(b) = self.bump() {
                    if !esc {
                        if b == b'(' {
                            lvl = lvl.saturating_add(1);
                        }
                        if b == b')' {
                            lvl = lvl.saturating_sub(1);
                        }
                    }
                    if b == b'\\' {
                        esc = !esc;
                    } else if esc {
                        esc = false;
                    }
                    if lvl == 0 {
                        let end = self.pos.saturating_sub(1);
                        return Some(Obj::Str(self.b.get(start..end).unwrap_or(&[]).to_vec()));
                    }
                }
                None
            }
            b'<' if self.peek_at(1) == Some(b'<') => self.dict(depth).map(Obj::Dict),
            b'<' => {
                self.pos = self.pos.saturating_add(1);
                let start = self.pos;
                while self.peek().is_some_and(|b| b != b'>') {
                    self.pos = self.pos.saturating_add(1);
                }
                let h = self.b.get(start..self.pos).unwrap_or(&[]).to_vec();
                if self.bump() != Some(b'>') {
                    return None;
                }
                Some(Obj::Hex(h))
            }
            b'[' => {
                self.pos = self.pos.saturating_add(1);
                let mut a = Vec::new();
                loop {
                    self.skip_ws_comments();
                    match self.peek() {
                        Some(b']') => {
                            self.pos = self.pos.saturating_add(1);
                            return Some(Obj::Arr(a));
                        }
                        None => return None,
                        _ => a.push(self.object(depth.saturating_add(1))?),
                    }
                }
            }
            b't' if self.match_kw(b"true") => Some(Obj::Bool(true)),
            b'f' if self.match_kw(b"false") => Some(Obj::Bool(false)),
            b'n' if self.match_kw(b"null") => Some(Obj::Null),
            c if c.is_ascii_digit() || c == b'+' || c == b'-' || c == b'.' => {
                let start = self.pos;
                let first = self.raw_number();
                // N G R
                let save = self.pos;
                let is_int = self.b.get(start..save).is_some_and(|t| t.iter().all(u8::is_ascii_digit));
                if is_int {
                    self.skip_ws_comments();
                    if self.peek().is_some_and(|c| c.is_ascii_digit())
                        && let Some(g) = self.raw_int()
                    {
                        self.skip_ws_comments();
                        if self.peek() == Some(b'R') {
                            self.pos = self.pos.saturating_add(1);
                            return Some(Obj::Ref(
                                u32::try_from(cc_markup::js::f2u(first)).unwrap_or(u32::MAX),
                                u16::try_from(g).unwrap_or(u16::MAX),
                            ));
                        }
                    }
                }
                self.pos = save;
                Some(Obj::Num(first))
            }
            _ => None,
        }
    }
    /// `<< … >>`
    pub fn dict(&mut self, depth: usize) -> Option<Dict> {
        if !self.match_kw(b"<<") {
            return None;
        }
        let mut d = Dict::new();
        loop {
            self.skip_ws_comments();
            match self.peek() {
                Some(b'>') => {
                    if self.match_kw(b">>") {
                        return Some(d);
                    }
                    return None;
                }
                Some(b'/') => {
                    self.pos = self.pos.saturating_add(1);
                    let k = self.name();
                    let v = self.object(depth.saturating_add(1))?;
                    d.set(&k, v);
                }
                _ => return None,
            }
        }
    }
}
