//! The DWG bit stream reader (port of lib/dwg/bits-read.js; ODA spec v5.4.1 ch. 2, 2.11–2.13, 20.1).
//!
//! `BitReader::new(buf, ver, cp)` reads bits MSB first between `pos` and `end` (bit positions). Every read
//! checks the limit and returns `Err(DwgError)` instead of reading past it ("read past the end (a > b bits)").
//! From R2007 `TV` reads from the string stream set with [`BitReader::set_str`].
//!
//! Return types follow what the JavaScript reader gets: `BS` → i32 (signed 16 for the full form),
//! `BL` → i32, `RL` → u32, `BLL`/`RLL`/`H` → f64 (JS numbers), doubles f64.

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)] // each cast is masked

use crate::messages::msg;
use crate::value::{Obj, V, p2, p3};

/// An error while reading; the text is what the JS reader puts in `e.message`.
#[derive(Debug, Clone, PartialEq)]
pub struct DwgError(pub String);

impl std::fmt::Display for DwgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for DwgError {}

pub type Res<T> = Result<T, DwgError>;

pub fn err<T>(s: impl Into<String>) -> Res<T> {
    Err(DwgError(s.into()))
}

// ---------------------------------------------------------------- code pages

/// The text decoder of a DWGCODEPAGE (bits-read.js codepageDecoder).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Codepage {
    pub num: i32,
    pub name: &'static str,
    pub known: bool,
    enc: Enc,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Enc {
    Cp1252,
    Latin1,
    Other(&'static encoding_rs::Encoding),
}

/// Spec gap (bits-read.js): DWGCODEPAGE numbering found by experiment with the ODA File Converter.
pub const CODEPAGES: [&str; 46] = [
    "UNDEFINED",
    "ASCII",
    "ISO8859-1",
    "ISO8859-2",
    "ISO8859-3",
    "ISO8859-4",
    "ISO8859-5",
    "ISO8859-6",
    "ISO8859-7",
    "ISO8859-8",
    "ISO8859-9",
    "DOS437",
    "DOS850",
    "DOS852",
    "DOS855",
    "DOS857",
    "DOS860",
    "DOS861",
    "DOS863",
    "DOS864",
    "DOS865",
    "DOS869",
    "DOS932",
    "MAC-ROMAN",
    "BIG5",
    "KSC5601",
    "JOHAB",
    "DOS866",
    "ANSI_1250",
    "ANSI_1251",
    "ANSI_1252",
    "GB2312",
    "ANSI_1253",
    "ANSI_1254",
    "ANSI_1255",
    "ANSI_1256",
    "ANSI_1257",
    "ANSI_874",
    "ANSI_932",
    "ANSI_936",
    "ANSI_949",
    "ANSI_950",
    "ANSI_1361",
    "ANSI_1200",
    "ANSI_1258",
    "CNT",
];

fn label_of(name: &str) -> Option<&'static encoding_rs::Encoding> {
    use encoding_rs as e;
    Some(match name {
        "ANSI_1250" => e::WINDOWS_1250,
        "ANSI_1251" => e::WINDOWS_1251,
        "ANSI_1253" => e::WINDOWS_1253,
        "ANSI_1254" => e::WINDOWS_1254,
        "ANSI_1255" => e::WINDOWS_1255,
        "ANSI_1256" => e::WINDOWS_1256,
        "ANSI_1257" => e::WINDOWS_1257,
        "ANSI_1258" => e::WINDOWS_1258,
        "ANSI_874" => e::WINDOWS_874,
        "ANSI_932" | "DOS932" => e::SHIFT_JIS,
        "ANSI_936" | "GB2312" => e::GBK,
        "ANSI_949" | "KSC5601" => e::EUC_KR,
        "ANSI_950" | "BIG5" => e::BIG5,
        "DOS866" => e::IBM866,
        "ISO8859-2" => e::ISO_8859_2,
        "ISO8859-3" => e::ISO_8859_3,
        "ISO8859-4" => e::ISO_8859_4,
        "ISO8859-5" => e::ISO_8859_5,
        "ISO8859-6" => e::ISO_8859_6,
        "ISO8859-7" => e::ISO_8859_7,
        "ISO8859-8" => e::ISO_8859_8,
        // WHATWG maps iso-8859-9 to windows-1254
        "ISO8859-9" => e::WINDOWS_1254,
        "MAC-ROMAN" => e::MACINTOSH,
        _ => return None,
    })
}

impl Codepage {
    pub fn new(num: i32) -> Codepage {
        let name = usize::try_from(num).ok().and_then(|i| CODEPAGES.get(i)).copied();
        match name {
            None | Some("ANSI_1252" | "UNDEFINED" | "ASCII" | "ISO8859-1") => {
                Codepage { num, name: name.unwrap_or("ANSI_1252"), known: name.is_some(), enc: Enc::Cp1252 }
            }
            Some(n) => match label_of(n) {
                Some(e) => Codepage { num, name: n, known: true, enc: Enc::Other(e) },
                None => Codepage { num, name: n, known: false, enc: Enc::Cp1252 },
            },
        }
    }
    /// Latin-1 (the BitReader default when no code page is given).
    pub fn latin1() -> Codepage {
        Codepage { num: -1, name: "latin1", known: true, enc: Enc::Latin1 }
    }
    /// Bytes → text, with "\U+XXXX" escapes decoded (codepageDecoder).
    pub fn decode(&self, b: &[u8]) -> String {
        let s = match self.enc {
            Enc::Latin1 => b.iter().map(|&c| char::from(c)).collect(),
            Enc::Cp1252 => b.iter().map(|&c| cp1252(c)).collect(),
            Enc::Other(e) => {
                if b.iter().any(|&c| c > 0x7F) {
                    e.decode_without_bom_handling(b).0.into_owned()
                } else {
                    b.iter().map(|&c| char::from(c)).collect()
                }
            }
        };
        if matches!(self.enc, Enc::Latin1) { s } else { unescape_u(s) }
    }
}

fn cp1252(c: u8) -> char {
    const HI: [u16; 32] = [
        0x20AC, 0, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0, 0x017D, 0, 0, 0x2018, 0x2019,
        0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0, 0x017E, 0x0178,
    ];
    if (0x80..0xA0).contains(&c) {
        let h = HI.get(usize::from(c.wrapping_sub(0x80))).copied().unwrap_or(0);
        if h != 0 {
            return char::from_u32(u32::from(h)).unwrap_or('\u{FFFD}');
        }
    }
    char::from(c)
}

/// "\U+XXXX" → the character (only upper-case U, as the JS regex; a lone surrogate becomes U+FFFD).
pub fn unescape_u(s: String) -> String {
    if !s.contains("\\U+") {
        return s;
    }
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    let mut last = 0usize;
    // i < b.len() throughout, so none of these additions can overflow.
    while i.saturating_add(7) <= b.len() {
        let (i3, i7) = (i.saturating_add(3), i.saturating_add(7));
        if b.get(i..i3) == Some(b"\\U+") && b.get(i3..i7).is_some_and(|h| h.iter().all(u8::is_ascii_hexdigit)) {
            out.push_str(s.get(last..i).unwrap_or_default());
            let hex = s.get(i3..i7).unwrap_or("0");
            let code = u32::from_str_radix(hex, 16).unwrap_or(0xFFFD);
            out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
            i = i7;
            last = i;
        } else {
            i = i.saturating_add(1);
        }
    }
    out.push_str(s.get(last..).unwrap_or_default());
    out
}

/// UTF-16 code units → String (lone surrogates → U+FFFD; JS keeps them, JSON then escapes them).
pub fn from_utf16(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

// ---------------------------------------------------------------- the reader

#[derive(Clone, Debug)]
pub struct BitReader<'a> {
    pub buf: &'a [u8],
    pub pos: usize,
    pub end: usize,
    /// 12 14 15 18 21 24 27 32
    pub version: u32,
    pub cp: Codepage,
    /// R2007+: the string stream (pos, end) TV reads from.
    pub str_pos: Option<(usize, usize)>,
    /// The reference code of the last handle read (2.13).
    pub hcode: u32,
}

impl<'a> BitReader<'a> {
    pub fn new(buf: &'a [u8], version: u32, cp: Codepage) -> Self {
        BitReader { buf, pos: 0, end: buf.len().saturating_mul(8), version, cp, str_pos: None, hcode: 0 }
    }
    /// A reader on bits [start, end).
    pub fn range(buf: &'a [u8], version: u32, cp: Codepage, start: usize, end: usize) -> Self {
        BitReader { buf, pos: start, end, version, cp, str_pos: None, hcode: 0 }
    }
    pub fn set_str(&mut self, pos: usize, end: usize) {
        self.str_pos = Some((pos, end));
    }
    fn overrun<T>(&self, n: usize) -> Res<T> {
        let at = self.pos.saturating_add(n).to_string();
        let end = self.end.to_string();
        err(msg("ccdwg.err.read_past_end", &[("at", &at), ("end", &end)]))
    }
    pub fn bits_left(&self) -> i64 {
        crate::util::to_i64(self.end as f64 - self.pos as f64)
    }
    pub fn align(&mut self) {
        self.pos = self.pos.saturating_add(7) & !7;
    }
    #[inline]
    fn byte(&self, i: usize) -> u32 {
        u32::from(self.buf.get(i).copied().unwrap_or(0))
    }

    #[inline]
    pub fn b(&mut self) -> Res<u32> {
        let p = self.pos;
        if p >= self.end {
            return self.overrun(1);
        }
        // p < end, so p + 1 cannot overflow; p & 7 <= 7.
        self.pos = p.saturating_add(1);
        Ok((self.byte(p >> 3) >> (7usize.wrapping_sub(p & 7))) & 1)
    }
    /// B as a JS boolean (`!!r.B()`).
    pub fn bool(&mut self) -> Res<bool> {
        Ok(self.b()? != 0)
    }
    #[inline]
    pub fn bb(&mut self) -> Res<u32> {
        let p = self.pos;
        if p.saturating_add(2) > self.end {
            return self.overrun(2);
        }
        self.pos = p.saturating_add(2);
        let s = p & 7;
        let i = p >> 3;
        if s < 7 {
            return Ok((self.byte(i) >> (6usize.wrapping_sub(s))) & 3);
        }
        Ok(((self.byte(i) & 1) << 1) | (self.byte(i.saturating_add(1)) >> 7))
    }
    /// 2.1 3B: up to three bits, stopping at a zero bit.
    pub fn b3(&mut self) -> Res<u32> {
        let mut v = 0;
        for _ in 0..3 {
            let b = self.b()?;
            v = (v << 1) | b;
            if b == 0 {
                break;
            }
        }
        Ok(v)
    }
    /// n bits as a number (JS `(v << 1) | b`, 32-bit).
    pub fn bits(&mut self, n: u32) -> Res<i32> {
        let mut v: i32 = 0;
        for _ in 0..n {
            v = (v.wrapping_shl(1)) | self.b()? as i32;
        }
        Ok(v)
    }
    #[inline]
    pub fn rc(&mut self) -> Res<u32> {
        let p = self.pos;
        if p.saturating_add(8) > self.end {
            return self.overrun(8);
        }
        self.pos = p.saturating_add(8);
        let i = p >> 3;
        let s = p & 7;
        Ok(if s != 0 { ((self.byte(i) << s) | (self.byte(i.saturating_add(1)) >> (8usize.wrapping_sub(s)))) & 0xFF } else { self.byte(i) })
    }
    pub fn rs(&mut self) -> Res<i32> {
        let a = self.rc()?;
        let b = self.rc()?;
        Ok(i32::from(((b << 8) | a) as u16 as i16))
    }
    pub fn rsu(&mut self) -> Res<u32> {
        let a = self.rc()?;
        let b = self.rc()?;
        Ok((b << 8) | a)
    }
    pub fn rl(&mut self) -> Res<u32> {
        let a = self.rc()?;
        let b = self.rc()?;
        let c = self.rc()?;
        let d = self.rc()?;
        Ok(a | (b << 8) | (c << 16) | (d << 24))
    }
    pub fn rls(&mut self) -> Res<i32> {
        Ok(self.rl()? as i32)
    }
    pub fn rll(&mut self) -> Res<f64> {
        let lo = self.rl()?;
        let hi = self.rl()?;
        Ok(f64::from(hi) * 4294967296.0 + f64::from(lo))
    }
    pub fn rd(&mut self) -> Res<f64> {
        let mut b = [0u8; 8];
        for x in &mut b {
            *x = self.rc()? as u8;
        }
        Ok(f64::from_le_bytes(b))
    }
    pub fn bytes(&mut self, n: usize) -> Res<Vec<u8>> {
        if self.pos.saturating_add(n.saturating_mul(8)) > self.end {
            return self.overrun(n.saturating_mul(8));
        }
        if self.pos & 7 == 0 {
            let i = self.pos >> 3;
            // pos + n * 8 <= end was checked above.
            self.pos = self.pos.saturating_add(n.saturating_mul(8));
            return Ok(self.buf.get(i..i.saturating_add(n)).map(<[u8]>::to_vec).unwrap_or_else(|| vec![0; n]));
        }
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(self.rc()? as u8);
        }
        Ok(out)
    }

    /// 2.2 BITSHORT.
    pub fn bs(&mut self) -> Res<i32> {
        match self.bb()? {
            0 => self.rs(),
            1 => Ok(self.rc()? as i32),
            2 => Ok(0),
            _ => Ok(256),
        }
    }
    /// 2.3 BITLONG.
    pub fn bl(&mut self) -> Res<i32> {
        match self.bb()? {
            0 => self.rls(),
            1 => Ok(self.rc()? as i32),
            2 => Ok(0),
            _ => err(msg("ccdwg.err.bitlong", &[])),
        }
    }
    /// 2.4 BITLONGLONG (a JS number: bytes summed as doubles).
    pub fn bll(&mut self) -> Res<f64> {
        let n = self.bits(3)?;
        let mut v = 0.0;
        let mut m = 1.0;
        for _ in 0..n {
            v += f64::from(self.rc()?) * m;
            m *= 256.0;
        }
        Ok(v)
    }
    /// 2.5 BITDOUBLE.
    pub fn bd(&mut self) -> Res<f64> {
        match self.bb()? {
            0 => self.rd(),
            1 => Ok(1.0),
            2 => Ok(0.0),
            _ => err(msg("ccdwg.err.bitdouble", &[])),
        }
    }
    /// 2.9 BitDouble with default.
    pub fn dd(&mut self, def: f64) -> Res<f64> {
        match self.bb()? {
            0 => Ok(def),
            1 => {
                let mut b = def.to_le_bytes();
                for x in b.iter_mut().take(4) {
                    *x = self.rc()? as u8;
                }
                Ok(f64::from_le_bytes(b))
            }
            2 => {
                let mut b = def.to_le_bytes();
                if let Some(x) = b.get_mut(4) {
                    *x = self.rc()? as u8;
                }
                if let Some(x) = b.get_mut(5) {
                    *x = self.rc()? as u8;
                }
                for x in b.iter_mut().take(4) {
                    *x = self.rc()? as u8;
                }
                Ok(f64::from_le_bytes(b))
            }
            _ => self.rd(),
        }
    }
    pub fn rd2(&mut self) -> Res<V> {
        let x = self.rd()?;
        Ok(p2(x, self.rd()?))
    }
    pub fn rd3(&mut self) -> Res<V> {
        let x = self.rd()?;
        let y = self.rd()?;
        Ok(p3(x, y, self.rd()?))
    }
    pub fn bd2(&mut self) -> Res<V> {
        let x = self.bd()?;
        Ok(p2(x, self.bd()?))
    }
    pub fn bd3(&mut self) -> Res<V> {
        let x = self.bd()?;
        let y = self.bd()?;
        Ok(p3(x, y, self.bd()?))
    }
    /// Raw triple for code that needs the numbers.
    pub fn bd3_raw(&mut self) -> Res<[f64; 3]> {
        let x = self.bd()?;
        let y = self.bd()?;
        Ok([x, y, self.bd()?])
    }
    pub fn rd2_raw(&mut self) -> Res<[f64; 2]> {
        let x = self.rd()?;
        Ok([x, self.rd()?])
    }
    pub fn rd3_raw(&mut self) -> Res<[f64; 3]> {
        let x = self.rd()?;
        let y = self.rd()?;
        Ok([x, y, self.rd()?])
    }
    /// DD3 with a default point.
    pub fn dd3(&mut self, d: [f64; 3]) -> Res<V> {
        let x = self.dd(d[0])?;
        let y = self.dd(d[1])?;
        Ok(p3(x, y, self.dd(d[2])?))
    }
    /// 2.8 BitExtrusion.
    pub fn be(&mut self) -> Res<V> {
        if self.version >= 15 && self.b()? != 0 {
            return Ok(p3(0.0, 0.0, 1.0));
        }
        self.bd3()
    }
    /// 2.10 BitThickness.
    pub fn bt(&mut self) -> Res<f64> {
        if self.version >= 15 && self.b()? != 0 {
            return Ok(0.0);
        }
        self.bd()
    }
    /// 2.12 object type.
    pub fn ot(&mut self) -> Res<i32> {
        if self.version < 24 {
            return self.bs();
        }
        match self.bb()? {
            0 => Ok(self.rc()? as i32),
            1 => Ok((self.rc()? as i32).wrapping_add(0x1F0)), // rc <= 255
            _ => Ok(self.rsu()? as i32),
        }
    }
    /// 2.13 handle reference → absolute handle (a JS number). `self.hcode` keeps the code.
    pub fn h(&mut self, reference: f64) -> Res<f64> {
        let b = self.rc()?;
        let code = b >> 4;
        let n = b & 15;
        let mut v = 0.0f64;
        for _ in 0..n {
            v = v * 256.0 + f64::from(self.rc()?);
        }
        self.hcode = code;
        Ok(match code {
            6 => reference + 1.0,
            8 => reference - 1.0,
            0xA => reference + v,
            0xC => reference - v,
            _ => v,
        })
    }
    /// T: BS length, then code-page bytes (trailing NULs dropped).
    pub fn t(&mut self) -> Res<String> {
        let n = self.bs()?;
        if n <= 0 {
            return Ok(String::new());
        }
        let b = self.bytes(crate::util::usize_from_i32(n))?;
        let mut s: &[u8] = &b;
        while let [rest @ .., 0] = s {
            s = rest;
        }
        Ok(self.cp.decode(s))
    }
    /// TU: BS character count, then UTF-16LE (trailing NULs dropped; "\U+XXXX" decoded before R2018).
    pub fn tu(&mut self) -> Res<String> {
        let n = (self.bs()? & 0xFFFF) as usize;
        if n == 0 {
            return Ok(String::new());
        }
        // n <= 0xFFFF, so n * 16 cannot overflow.
        if self.pos.saturating_add(n.saturating_mul(16)) > self.end {
            return self.overrun(n.saturating_mul(16));
        }
        let mut codes: Vec<u16> = Vec::with_capacity(n);
        for _ in 0..n {
            let lo = self.rc()?;
            codes.push((lo | (self.rc()? << 8)) as u16);
        }
        while codes.last() == Some(&0) {
            codes.pop();
        }
        let s = from_utf16(&codes);
        Ok(if self.version < 32 { unescape_u(s) } else { s })
    }
    /// TV: T up to R2004, TU from the string stream (when set) from R2007.
    pub fn tv(&mut self) -> Res<String> {
        if self.version >= 21 {
            if let Some((p, e)) = self.str_pos {
                let mut s = BitReader::range(self.buf, self.version, self.cp, p, e);
                let out = s.tu();
                self.str_pos = Some((s.pos, s.end));
                return out;
            }
            return self.tu();
        }
        self.t()
    }
    /// 2.11 CMC → the model colour.
    pub fn cmc(&mut self) -> Res<Obj> {
        if self.version < 18 {
            return Ok(crate::obj! { "index" => self.bs()? });
        }
        let index = self.bs()?;
        let rgb = self.bl()? as u32;
        let flags = self.rc()?;
        let mut c = colour_from_rgb_word(index, rgb);
        if flags & 1 != 0 {
            c.set("name", self.tv()?);
        }
        if flags & 2 != 0 {
            c.set("book", self.tv()?);
        }
        Ok(c)
    }
    /// 2.11 ENC (entity colour, R2004+): (colour, transparency word, has book handle).
    pub fn enc(&mut self) -> Res<(Obj, Option<u32>, bool)> {
        if self.version < 18 {
            return Ok((crate::obj! { "index" => self.bs()? }, None, false));
        }
        let v = (self.bs()? & 0xFFFF) as u32;
        let flags = v & 0xFF00;
        let index = (v & 0x1FF) as i32;
        let mut has_book = false;
        let color = if flags & 0x8000 != 0 {
            let rgb = self.bl()? as u32;
            if flags & 0x4000 != 0 {
                has_book = true;
            }
            colour_from_rgb_word(index, rgb)
        } else {
            crate::obj! { "index" => v & 0xFFF }
        };
        let tr = if flags & 0x2000 != 0 { Some(self.bl()? as u32) } else { None };
        Ok((color, tr, has_book))
    }
}

/// The 32-bit colour word of CMC/ENC: top byte = method.
pub fn colour_from_rgb_word(index: i32, word: u32) -> Obj {
    match word >> 24 {
        0xC2 => crate::obj! { "rgb" => word & 0xFF_FFFF },
        0xC3 => crate::obj! { "index" => word & 0xFF },
        0xC0 => crate::obj! { "index" => 256 },
        0xC1 => crate::obj! { "index" => 0 },
        0xC8 => crate::obj! { "index" => 257 },
        _ => crate::obj! { "index" => index & 0x1FF },
    }
}

// ---------------------------------------------------------------- byte-level modular numbers (2.6, 2.7)

/// Modular char, signed (2.6) at `pos` → (value, next position). Past the end reads as 0 (JS `undefined`).
pub fn read_mc(b: &[u8], mut pos: usize) -> (f64, usize) {
    let mut v = 0.0f64;
    let mut shift = 0i32;
    loop {
        let byte = b.get(pos).copied().unwrap_or(0);
        pos = pos.saturating_add(1);
        if byte & 0x80 != 0 {
            v += f64::from(byte & 0x7F) * 2f64.powi(shift);
            shift = shift.saturating_add(7);
            if pos > b.len().saturating_add(16) {
                return (v, pos);
            }
            continue;
        }
        let neg = byte & 0x40 != 0;
        v += f64::from(byte & 0x3F) * 2f64.powi(shift);
        return (if neg { -v } else { v }, pos);
    }
}
/// Modular char, unsigned (R2010+ handle-stream size).
pub fn read_mcu(b: &[u8], mut pos: usize) -> (f64, usize) {
    let mut v = 0.0f64;
    let mut shift = 0i32;
    loop {
        let byte = b.get(pos).copied().unwrap_or(0);
        pos = pos.saturating_add(1);
        v += f64::from(byte & 0x7F) * 2f64.powi(shift);
        shift = shift.saturating_add(7);
        if byte & 0x80 == 0 || pos > b.len().saturating_add(16) {
            return (v, pos);
        }
    }
}
/// Modular short (2.7).
pub fn read_ms(b: &[u8], mut pos: usize) -> (f64, usize) {
    let mut v = 0.0f64;
    let mut shift = 0i32;
    loop {
        let w = u32::from(b.get(pos).copied().unwrap_or(0)) | (u32::from(b.get(pos.saturating_add(1)).copied().unwrap_or(0)) << 8);
        pos = pos.saturating_add(2);
        v += f64::from(w & 0x7FFF) * 2f64.powi(shift);
        shift = shift.saturating_add(15);
        if w & 0x8000 == 0 || pos > b.len().saturating_add(16) {
            return (v, pos);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bit_codes() {
        // BB=01 then RC 0x12 → BS 0x12
        let buf = [0b0100_0100, 0b1000_0000];
        let mut r = BitReader::new(&buf, 15, Codepage::latin1());
        assert_eq!(r.bs().unwrap(), 0x12);
        let mut r = BitReader::new(&[0b1000_0000], 15, Codepage::latin1());
        assert_eq!(r.bd().unwrap(), 0.0);
        let mut r = BitReader::new(&[0xFF], 15, Codepage::latin1());
        r.pos = 6;
        assert!(r.rc().is_err());
        assert_eq!(read_mc(&[0x81, 0x01], 0), (129.0, 2));
        assert_eq!(read_mc(&[0x41], 0), (-1.0, 1));
        assert_eq!(read_ms(&[0x01, 0x80, 0x01, 0x00], 0), (1.0 + 32768.0, 4));
    }
    #[test]
    fn text() {
        assert_eq!(unescape_u("a\\U+00E9b".into()), "aéb");
        assert_eq!(Codepage::new(30).decode(&[0x80, b'x']), "€x");
    }
}
