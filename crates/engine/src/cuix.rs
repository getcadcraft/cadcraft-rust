//! cuix.js — a CUIx customization file (a ZIP of XML parts and button pictures) read into My tools groups (0.5 DR.9).
//! Read only; the file is untrusted: sizes, counts and nesting are capped, no DOCTYPE or entity definitions are taken,
//! pictures must be real PNG or BMP. Written from the format as people publish it (a ZIP whose *.cui parts hold
//! CustSection › MenuGroup › MacroGroup › MenuMacro › Macro { Name, Command, SmallImage/LargeImage Name } and MenuRoot ›
//! ToolbarRoot › Toolbar › ToolbarButton › MenuItem › MacroRef MenuMacroID), never from a file out of a product install.
//! Spec gap (0.5's): shortcut keys and ribbon panels are not described in those sources; they are read leniently — any
//! MenuAccelerator with a MacroRef and a key string (an AcceleratorShortcutKey / Key / Shortcut child or attribute), and
//! any RibbonPanelSource with buttons carrying a MenuMacroID attribute.
//!
//! A BMP picture becomes a PNG with the same pixels; its compressed bytes are this build's own (the browser's zlib
//! writes others), so the parity harness compares the pictures, not their bytes.

use serde_json::{Value, json};

use crate::mytools::{Note, btoa, parse_macro};
use crate::shortcuts::norm_key;

/// `LIMITS`
pub const LIMIT_BYTES: usize = 32 << 20;
pub const LIMIT_ENTRIES: usize = 2000;
pub const LIMIT_ENTRY_BYTES: usize = 16 << 20;
pub const LIMIT_XML_CHARS: usize = 8 << 20;
pub const LIMIT_DEPTH: usize = 64;
pub const LIMIT_NODES: usize = 200_000;
pub const LIMIT_TOOLS: usize = 2000;
pub const LIMIT_GROUPS: usize = 200;
pub const LIMIT_ICON: usize = 64 << 10;

/// Why a file was refused: "zip" | "too_big" | "xml" | "doctype" | "not_cuix" (the page's `mytools.cuix.err.*`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CuixErr(pub &'static str);

/// a + b (saturating: an offset past the end only reads nothing).
fn ad(a: usize, b: usize) -> usize {
    a.saturating_add(b)
}

fn u16_at(b: &[u8], o: usize) -> usize {
    usize::from(b.get(o).copied().unwrap_or(0)) | (usize::from(b.get(o.wrapping_add(1)).copied().unwrap_or(0)) << 8)
}
fn u32_at(b: &[u8], o: usize) -> usize {
    let g = |k: usize| u32::from(b.get(o.wrapping_add(k)).copied().unwrap_or(0));
    (g(0) | (g(1) << 8) | (g(2) << 16) | (g(3) << 24)) as usize
}

/// `DecompressionStream('deflate-raw')` with a cap: None for a broken stream, junk after its end, or more than `cap`.
fn inflate_raw(data: &[u8], cap: usize) -> Result<Vec<u8>, CuixErr> {
    use miniz_oxide::inflate::TINFLStatus;
    use miniz_oxide::inflate::core::{DecompressorOxide, decompress, inflate_flags};
    let mut st = DecompressorOxide::new();
    let mut out: Vec<u8> = vec![0; 64 * 1024];
    let (mut pos_in, mut pos_out) = (0usize, 0usize);
    loop {
        let input = data.get(pos_in..).unwrap_or(&[]);
        let flags = inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;
        let (status, used_in, used_out) = decompress(&mut st, input, &mut out, pos_out, flags);
        pos_in = pos_in.saturating_add(used_in);
        pos_out = pos_out.saturating_add(used_out);
        if pos_out > cap {
            return Err(CuixErr("too_big"));
        }
        match status {
            TINFLStatus::Done => {
                if pos_in < data.len() {
                    return Err(CuixErr("zip"));
                }
                out.truncate(pos_out);
                return Ok(out);
            }
            TINFLStatus::HasMoreOutput => {
                let grow = out.len().saturating_mul(2).min(cap.saturating_add(64 * 1024));
                if grow <= out.len() {
                    return Err(CuixErr("too_big"));
                }
                out.resize(grow, 0);
            }
            _ => return Err(CuixErr("zip")),
        }
    }
}

/// `unzip(bytes)`: the entries of a ZIP (stored or deflated), by name, in the order the file first names them.
pub fn unzip(b: &[u8]) -> Result<Vec<(String, Vec<u8>)>, CuixErr> {
    if b.len() > LIMIT_BYTES {
        return Err(CuixErr("too_big"));
    }
    let mut e = None;
    if b.len() >= 22 {
        let hi = b.len().saturating_sub(22);
        let lo = hi.saturating_sub(65535);
        e = (lo..=hi).rev().find(|i| u32_at(b, *i) == 0x0605_4b50);
    }
    let e = e.ok_or(CuixErr("zip"))?;
    let n = u16_at(b, ad(e, 10));
    let cd = u32_at(b, ad(e, 16));
    if n > LIMIT_ENTRIES {
        return Err(CuixErr("too_big"));
    }
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut p = cd;
    let mut total = 0usize;
    for _ in 0..n {
        if p.saturating_add(46) > b.len() || u32_at(b, p) != 0x0201_4b50 {
            return Err(CuixErr("zip"));
        }
        let method = u16_at(b, ad(p, 10));
        let csize = u32_at(b, ad(p, 20));
        let usize_ = u32_at(b, ad(p, 24));
        let (nl, xl, cl) = (u16_at(b, ad(p, 28)), u16_at(b, ad(p, 30)), u16_at(b, ad(p, 32)));
        let lo = u32_at(b, ad(p, 42));
        let nb = b.get(ad(p, 46)..ad(ad(p, 46), nl).min(b.len())).unwrap_or(&[]);
        let name = String::from_utf8_lossy(nb).into_owned();
        p = ad(ad(ad(ad(p, 46), nl), xl), cl);
        if name.ends_with('/') {
            continue;
        }
        total = total.saturating_add(usize_);
        if usize_ > LIMIT_ENTRY_BYTES || total > LIMIT_BYTES {
            return Err(CuixErr("too_big"));
        }
        if lo.saturating_add(30) > b.len() || u32_at(b, lo) != 0x0403_4b50 {
            return Err(CuixErr("zip"));
        }
        let start = ad(ad(ad(lo, 30), u16_at(b, ad(lo, 26))), u16_at(b, ad(lo, 28)));
        let data = b.get(start.min(b.len())..start.saturating_add(csize).min(b.len())).unwrap_or(&[]);
        if data.len() != csize {
            return Err(CuixErr("zip"));
        }
        let bytes = match method {
            0 => data.to_vec(),
            8 => inflate_raw(data, LIMIT_ENTRY_BYTES)?,
            _ => continue,
        };
        let key = name.replace('\\', "/");
        match out.iter_mut().find(|x| x.0 == key) {
            Some(x) => x.1 = bytes,
            None => out.push((key, bytes)),
        }
    }
    Ok(out)
}

/// An element of the small XML reader.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct El {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<El>,
    pub text: String,
}
impl El {
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|a| a.0 == k).map(|a| a.1.as_str())
    }
    fn kid(&self, n: &str) -> Option<&El> {
        self.children.iter().find(|c| c.name == n)
    }
    fn kids<'a>(&'a self, n: &'a str) -> impl Iterator<Item = &'a El> + 'a {
        self.children.iter().filter(move |c| c.name == n)
    }
    /// Pre-order: this element, then each child's.
    fn walk<'a>(&'a self, out: &mut Vec<&'a El>) {
        let mut stack = vec![self];
        while let Some(e) = stack.pop() {
            out.push(e);
            for c in e.children.iter().rev() {
                stack.push(c);
            }
        }
    }
    fn all(&self) -> Vec<&El> {
        let mut v = Vec::new();
        self.walk(&mut v);
        v
    }
}
fn kid<'a>(e: Option<&'a El>, n: &str) -> Option<&'a El> {
    e.and_then(|e| e.kid(n))
}
fn text_of(e: Option<&El>) -> String {
    e.map(|e| cc_cad::js::trim(&e.text).to_string()).unwrap_or_default()
}

/// `unesc(s)`: the five entities and character references (anything else stays as written).
fn unesc(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let c: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while let Some(&ch) = c.get(i) {
        if ch == '&' {
            // &(#x[0-9a-f]+|#\d+|[a-z]+); case-insensitive
            let mut j = ad(i, 1);
            let body_ok = if c.get(j) == Some(&'#')
                && c.get(ad(j, 1)).is_some_and(|x| *x == 'x' || *x == 'X')
                && c.get(ad(j, 2)).is_some_and(char::is_ascii_hexdigit)
            {
                j = ad(j, 2);
                while c.get(j).is_some_and(char::is_ascii_hexdigit) {
                    j = ad(j, 1);
                }
                true
            } else if c.get(j) == Some(&'#') && c.get(ad(j, 1)).is_some_and(char::is_ascii_digit) {
                j = ad(j, 1);
                while c.get(j).is_some_and(char::is_ascii_digit) {
                    j = ad(j, 1);
                }
                true
            } else if c.get(j).is_some_and(char::is_ascii_alphabetic) {
                while c.get(j).is_some_and(char::is_ascii_alphabetic) {
                    j = ad(j, 1);
                }
                true
            } else {
                false
            };
            if body_ok && c.get(j) == Some(&';') {
                let e: String = c.get(ad(i, 1)..j).unwrap_or(&[]).iter().collect();
                let m: String = c.get(i..=j).unwrap_or(&[]).iter().collect();
                if let Some(num) = e.strip_prefix('#') {
                    let v = match num.strip_prefix(['x', 'X']) {
                        Some(h) => u64::from_str_radix(h.get(h.len().saturating_sub(16)..).unwrap_or(h), 16)
                            .ok()
                            .filter(|_| h.trim_start_matches('0').len() <= 16),
                        None => num.trim_start_matches('0').parse::<u64>().ok().or(if num.trim_start_matches('0').is_empty() {
                            Some(0)
                        } else {
                            None
                        }),
                    };
                    if let Some(v) = v.filter(|v| *v > 0 && *v < 0x11_0000) {
                        // (a lone surrogate, which JS keeps, has no Rust char: the replacement character)
                        out.push(u32::try_from(v).ok().and_then(char::from_u32).unwrap_or('\u{fffd}'));
                    }
                } else {
                    match e.to_ascii_lowercase().as_str() {
                        "lt" => out.push('<'),
                        "gt" => out.push('>'),
                        "amp" => out.push('&'),
                        "quot" => out.push('"'),
                        "apos" => out.push('\''),
                        // (0.5 looks the name up in a plain object: its prototype answers "constructor")
                        "constructor" => out.push_str("function Object() { [native code] }"),
                        _ => out.push_str(&m),
                    }
                }
                i = ad(j, 1);
                continue;
            }
        }
        out.push(ch);
        i = ad(i, 1);
    }
    out
}

fn name_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}
fn name_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b':' | b'-')
}

/// The attributes of a start tag after its name (`/([A-Za-z_][\w.:-]*)\s*=\s*("([^"]*)"|'([^']*)')/g`).
fn attrs_of(s: &str, out: &mut Vec<(String, String)>) {
    let b = s.as_bytes();
    let ws_at = |i: usize| -> usize {
        // JS \s at a byte position (multi-byte spaces too): its length, 0 if none
        s.get(i..).and_then(|t| t.chars().next()).filter(|c| ws(*c)).map_or(0, char::len_utf8)
    };
    let mut i = 0;
    while i < b.len() {
        if !b.get(i).copied().is_some_and(name_start) || !s.is_char_boundary(i) {
            i = ad(i, 1);
            continue;
        }
        let ns = i;
        let mut j = ad(i, 1);
        while b.get(j).copied().is_some_and(name_char) {
            j = ad(j, 1);
        }
        let name = s.get(ns..j).unwrap_or("").to_string();
        let mut k = j;
        loop {
            let w = ws_at(k);
            if w == 0 {
                break;
            }
            k = ad(k, w);
        }
        if b.get(k) == Some(&b'=') {
            k = ad(k, 1);
            loop {
                let w = ws_at(k);
                if w == 0 {
                    break;
                }
                k = ad(k, w);
            }
            if let Some(&q) = b.get(k).filter(|q| **q == b'"' || **q == b'\'')
                && let Some(e) = s.get(ad(k, 1)..).and_then(|t| t.find(char::from(q)))
            {
                let v = s.get(ad(k, 1)..ad(ad(k, 1), e)).unwrap_or("");
                let v = unesc(v);
                match out.iter_mut().find(|a| a.0 == name) {
                    Some(a) => a.1 = v,
                    None => out.push((name, v)),
                }
                i = ad(ad(k, 1), ad(e, 1));
                continue;
            }
        }
        i = ad(ns, 1);
    }
}

fn ws(c: char) -> bool {
    cc_cad::js::is_ws(c)
}

/// `parseXml(text)`: elements, attributes, text, CDATA; comments and <?…?> skipped; a DOCTYPE refused.
pub fn parse_xml(s: &str) -> Result<El, CuixErr> {
    if s.encode_utf16().count() > LIMIT_XML_CHARS {
        return Err(CuixErr("too_big"));
    }
    if s.to_ascii_uppercase().contains("<!DOCTYPE") {
        return Err(CuixErr("doctype"));
    }
    // the open elements: the root first; a closed one moves into its parent's children
    let mut stack: Vec<El> = vec![El { name: "#root".into(), ..El::default() }];
    let mut i = 0usize;
    let mut nodes = 0usize;
    let push_text = |stack: &mut Vec<El>, t: &str| {
        if let Some(top) = stack.last_mut() {
            top.text.push_str(t);
        }
    };
    while i < s.len() {
        let Some(lt) = s.get(i..).and_then(|t| t.find('<')).map(|k| ad(k, i)) else {
            push_text(&mut stack, &unesc(s.get(i..).unwrap_or("")));
            break;
        };
        if lt > i {
            push_text(&mut stack, &unesc(s.get(i..lt).unwrap_or("")));
        }
        let from = |k: usize, pat: &str| s.get(k..).and_then(|t| t.find(pat)).map(|x| ad(x, k));
        let rest = s.get(lt..).unwrap_or("");
        if rest.starts_with("<!--") {
            i = from(lt, "-->").map_or(s.len(), |e| ad(e, 3));
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            let e = from(lt, "]]>");
            let body = s.get(ad(lt, 9)..e.unwrap_or(s.len()).max(ad(lt, 9))).unwrap_or("");
            push_text(&mut stack, body);
            i = e.map_or(s.len(), |e| ad(e, 3));
            continue;
        }
        if rest.starts_with("<?") {
            i = from(lt, "?>").map_or(s.len(), |e| ad(e, 2));
            continue;
        }
        let gt = from(lt, ">").ok_or(CuixErr("xml"))?;
        let mut tag = s.get(ad(lt, 1)..gt).unwrap_or("");
        i = ad(gt, 1);
        if let Some(t) = tag.strip_prefix('/') {
            let name = cc_cad::js::trim(t);
            if stack.len() < 2 || stack.last().is_none_or(|e| e.name != name) {
                return Err(CuixErr("xml"));
            }
            if let Some(el) = stack.pop()
                && let Some(parent) = stack.last_mut()
            {
                parent.children.push(el);
            }
            continue;
        }
        let self_close = tag.ends_with('/');
        if self_close {
            tag = tag.get(..tag.len().saturating_sub(1)).unwrap_or("");
        }
        let tb = tag.as_bytes();
        if !tb.first().copied().is_some_and(name_start) {
            return Err(CuixErr("xml"));
        }
        let mut n = 1;
        while tb.get(n).copied().is_some_and(name_char) {
            n = ad(n, 1);
        }
        let mut el = El { name: tag.get(..n).unwrap_or("").to_string(), ..El::default() };
        attrs_of(tag.get(n..).unwrap_or(""), &mut el.attrs);
        nodes = ad(nodes, 1);
        if nodes > LIMIT_NODES {
            return Err(CuixErr("too_big"));
        }
        if self_close {
            if let Some(top) = stack.last_mut() {
                top.children.push(el);
            }
        } else {
            stack.push(el);
            if stack.len() > LIMIT_DEPTH {
                return Err(CuixErr("too_big"));
            }
        }
    }
    if stack.len() != 1 {
        return Err(CuixErr("xml"));
    }
    stack.pop().ok_or(CuixErr("xml"))
}

/// `bmpToPng(bytes)`: a PNG from an uncompressed 1/4/8/24/32-bit BMP of at most 256 × 256, or None.
pub fn bmp_to_png(b: &[u8]) -> Option<Vec<u8>> {
    if b.len() < 54 || b.first() != Some(&0x42) || b.get(1) != Some(&0x4d) {
        return None;
    }
    let off = u32_at(b, 10);
    let hs = u32_at(b, 14);
    // (the fields are 32-bit little-endian; width and height are signed)
    let w = i32::from_le_bytes([*b.get(18)?, *b.get(19)?, *b.get(20)?, *b.get(21)?]);
    let h0 = i32::from_le_bytes([*b.get(22)?, *b.get(23)?, *b.get(24)?, *b.get(25)?]);
    let bpp = u16_at(b, 28);
    let comp = u32_at(b, 30);
    let h = usize::try_from(h0.unsigned_abs()).ok()?;
    let bottom_up = h0 > 0;
    if w <= 0 || h == 0 || w > 256 || h > 256 || ![1, 4, 8, 24, 32].contains(&bpp) || (comp != 0 && comp != 3) {
        return None;
    }
    let w = usize::try_from(w).ok()?;
    let ncol = if bpp <= 8 {
        let n = u32_at(b, 46);
        if n == 0 { 1usize << bpp } else { n }
    } else {
        0
    };
    let pal = 14usize.saturating_add(hs);
    // (w, h ≤ 256 and bpp ≤ 32: these products are small)
    let stride = w.checked_mul(bpp)?.div_ceil(32).checked_mul(4)?;
    if off.saturating_add(stride.checked_mul(h)?) > b.len() {
        return None;
    }
    let at = |i: usize| b.get(i).copied().unwrap_or(0);
    let mut rgba = vec![0u8; w.checked_mul(h)?.checked_mul(4)?];
    let mask = (1usize << bpp).saturating_sub(1);
    for y in 0..h {
        let ry = if bottom_up { h.saturating_sub(1).saturating_sub(y) } else { y };
        let row = off.saturating_add(ry.saturating_mul(stride));
        for x in 0..w {
            let (r, g, bl, al);
            if bpp >= 24 {
                let q = row.saturating_add(x.saturating_mul(bpp / 8));
                bl = at(q);
                g = at(ad(q, 1));
                r = at(ad(q, 2));
                al = if bpp == 32 && at(ad(q, 3)) != 0 { at(ad(q, 3)) } else { 255 };
            } else {
                let bit = x.saturating_mul(bpp);
                let byte = usize::from(at(row.saturating_add(bit >> 3)));
                let shift = 8usize.saturating_sub(bpp).saturating_sub(bit & 7);
                let idx = (byte >> shift) & mask;
                if idx >= ncol {
                    return None;
                }
                let q = pal.saturating_add(idx.saturating_mul(4));
                bl = at(q);
                g = at(q.saturating_add(1));
                r = at(q.saturating_add(2));
                al = 255;
            }
            let o = y.saturating_mul(w).saturating_add(x).saturating_mul(4);
            if let Some(px) = rgba.get_mut(o..ad(o, 4)) {
                px.copy_from_slice(&[r, g, bl, al]);
            }
        }
    }
    Some(png_of(&rgba, w, h))
}

fn crc32(d: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &x in d {
        c ^= u32::from(x);
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    c ^ 0xffff_ffff
}

fn be32(n: usize) -> [u8; 4] {
    u32::try_from(n).unwrap_or(u32::MAX).to_be_bytes()
}

/// `pngOf(rgba, w, h)`: an 8-bit RGBA PNG (rows unfiltered, zlib-deflated).
fn png_of(rgba: &[u8], w: usize, h: usize) -> Vec<u8> {
    let rowb = w.saturating_mul(4);
    let mut raw = Vec::with_capacity(h.saturating_mul(ad(rowb, 1)));
    for y in 0..h {
        raw.push(0);
        let a = y.saturating_mul(rowb);
        raw.extend_from_slice(rgba.get(a..ad(a, rowb)).unwrap_or(&[]));
    }
    let z = cc_io_cad::plot::zlib(&raw);
    let chunk = |out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&be32(data.len()));
        let mut c = ty.to_vec();
        c.extend_from_slice(data);
        out.extend_from_slice(&c);
        out.extend_from_slice(&crc32(&c).to_be_bytes());
    };
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&be32(w));
    ihdr.extend_from_slice(&be32(h));
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut out = vec![137, 80, 78, 71, 13, 10, 26, 10];
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn is_png(d: &[u8]) -> bool {
    d.len() > 8 && d.starts_with(&[0x89, 0x50, 0x4e, 0x47])
}

/// A tool read from the file.
#[derive(Clone, Debug, PartialEq)]
pub struct CuixTool {
    pub label: String,
    pub macro_: String,
    pub icon: Option<String>,
    pub key: Option<String>,
}
/// A group read from the file (`shortcuts`: the keys' own group, named by the page).
#[derive(Clone, Debug, PartialEq)]
pub struct CuixGroup {
    pub name: String,
    pub tools: Vec<CuixTool>,
    pub shortcuts: bool,
}
/// `readCuix(bytes)`'s result.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cuix {
    pub groups: Vec<CuixGroup>,
    /// (alias, command), from a PGP part
    pub aliases: Vec<(String, String)>,
    /// command names CadCraft lacks, sorted
    pub missing: Vec<String>,
    pub notes: Vec<Note>,
}
impl Cuix {
    /// As cuix.js returns it (the parity harness).
    pub fn json(&self) -> Value {
        let tool = |t: &CuixTool| {
            let mut o = serde_json::Map::new();
            o.insert("label".into(), json!(t.label));
            o.insert("macro".into(), json!(t.macro_));
            if let Some(i) = &t.icon {
                o.insert("icon".into(), json!(i));
            }
            if let Some(k) = &t.key {
                o.insert("key".into(), json!(k));
            }
            Value::Object(o)
        };
        let groups: Vec<Value> = self
            .groups
            .iter()
            .map(|g| {
                let mut o = serde_json::Map::new();
                o.insert("name".into(), json!(g.name));
                o.insert("tools".into(), Value::Array(g.tools.iter().map(tool).collect()));
                if g.shortcuts {
                    o.insert("shortcuts".into(), json!(true));
                }
                Value::Object(o)
            })
            .collect();
        json!({
            "groups": groups,
            "aliases": self.aliases.iter().map(|(a, c)| json!({ "alias": a, "command": c })).collect::<Vec<_>>(),
            "missing": self.missing,
            "notes": self.notes.iter().map(Note::json).collect::<Vec<_>>(),
        })
    }
}

/// `/^\s*([A-Za-z0-9_-]{1,31})\s*,\s*\*\s*([A-Za-z0-9_.-]{1,64})\s*$/` on one line of a PGP part.
fn pgp_line(line: &str) -> Option<(String, String)> {
    let t = cc_cad::js::trim(line);
    let (a, rest) = t.split_once(',')?;
    let a = cc_cad::js::trim(a);
    let rest = cc_cad::js::trim_start(rest).strip_prefix('*')?;
    let c = cc_cad::js::trim(rest);
    let ok_a = (1..=31).contains(&a.len()) && a.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    let ok_c = (1..=64).contains(&c.len()) && c.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'));
    (ok_a && ok_c).then(|| (a.to_string(), c.to_string()))
}

/// `TextDecoder().decode(bytes)`: UTF-8 with replacement characters, a leading BOM dropped.
fn decode(d: &[u8]) -> String {
    let s = String::from_utf8_lossy(d);
    s.strip_prefix('\u{feff}').unwrap_or(&s).to_string()
}

/// `readCuix(bytes, { lookup, getvar })`: CUIx bytes → My tools groups, aliases, the commands CadCraft lacks, notes.
pub fn read_cuix(bytes: &[u8], lookup: &dyn Fn(&str) -> bool, getvar: &dyn Fn(&str) -> String) -> Result<Cuix, CuixErr> {
    let mut notes: Vec<Note> = Vec::new();
    let files = unzip(bytes)?;
    let mut trees: Vec<El> = Vec::new();
    for (n, d) in &files {
        let lower = n.to_ascii_lowercase();
        let xml = lower.ends_with(".xml") && !(lower.contains("[content_types]") || lower.ends_with(".rels"));
        if !(lower.ends_with(".cui") || xml) {
            continue;
        }
        match parse_xml(&decode(d)) {
            Ok(t) => trees.push(t),
            Err(_) => notes.push(Note { key: "mytools.note.part".into(), fills: vec![("part".into(), cc_cad::js::slice16(n, 80))] }),
        }
    }
    if !trees.iter().any(|t| t.all().iter().any(|e| e.name == "CustSection" || e.name == "MenuGroup")) {
        return Err(CuixErr("not_cuix"));
    }
    // pictures by file name (any folder, any case)
    let mut pics: Vec<(String, &Vec<u8>)> = Vec::new();
    for (n, d) in &files {
        let lower = n.to_lowercase();
        if lower.ends_with(".png") || lower.ends_with(".bmp") {
            let base = n.rsplit('/').next().unwrap_or(n).to_lowercase();
            match pics.iter_mut().find(|p| p.0 == base) {
                Some(p) => p.1 = d,
                None => pics.push((base, d)),
            }
        }
    }
    let icon_for = |name: Option<&str>| -> Option<String> {
        let name = name.filter(|n| !n.is_empty())?.to_lowercase();
        let get = |k: &str| pics.iter().find(|p| p.0 == k).map(|p| p.1);
        let d = get(&name).or_else(|| get(&format!("{name}.png"))).or_else(|| get(&format!("{name}.bmp")))?;
        let png = if is_png(d) { d.clone() } else { bmp_to_png(d)? };
        if png.len() > LIMIT_ICON {
            return None;
        }
        Some(format!("data:image/png;base64,{}", btoa(&png)))
    };
    // macros by UID: (name, command, small picture, large picture)
    struct M {
        name: String,
        command: String,
        small: Option<String>,
        large: Option<String>,
    }
    let mut macros: Vec<(String, M)> = Vec::new();
    for t in &trees {
        for e in t.all() {
            if e.name != "MenuMacro" {
                continue;
            }
            let Some(m) = e.kid("Macro") else { continue };
            let Some(uid) = e.attr("UID").filter(|u| !u.is_empty()) else { continue };
            let name = text_of(m.kid("Name"));
            let mm = M {
                name: if name.is_empty() { uid.to_string() } else { name },
                command: text_of(m.kid("Command")),
                small: m.kid("SmallImage").and_then(|x| x.attr("Name")).filter(|s| !s.is_empty()).map(str::to_string),
                large: m.kid("LargeImage").and_then(|x| x.attr("Name")).filter(|s| !s.is_empty()).map(str::to_string),
            };
            match macros.iter_mut().find(|x| x.0 == uid) {
                Some(x) => x.1 = mm,
                None => macros.push((uid.to_string(), mm)),
            }
        }
    }
    let mut missing: Vec<String> = Vec::new();
    let mut tool_count = 0usize;
    let mut tool_of = |label: &str, id: Option<&str>, notes: &mut Vec<Note>, missing: &mut Vec<String>| -> Option<CuixTool> {
        let Some(m) = id.and_then(|id| macros.iter().find(|x| x.0 == id)).map(|x| &x.1) else {
            notes.push(Note {
                key: "mytools.note.no_macro".into(),
                fills: vec![("id".into(), cc_cad::js::slice16(id.unwrap_or("undefined"), 60))],
            });
            return None;
        };
        tool_count = ad(tool_count, 1);
        if tool_count > LIMIT_TOOLS {
            return None;
        }
        let p = parse_macro(&m.command, getvar);
        for c in &p.commands {
            if !lookup(c) && !missing.contains(c) {
                missing.push(c.clone());
            }
        }
        notes.extend(p.notes);
        let icon = icon_for(m.small.as_deref()).or_else(|| icon_for(m.large.as_deref()));
        let label = if label.is_empty() { m.name.clone() } else { label.to_string() };
        Some(CuixTool { label: cc_cad::js::slice16(&label, 80), macro_: cc_cad::js::slice16(&m.command, 4000), icon, key: None })
    };
    let mut groups: Vec<CuixGroup> = Vec::new();
    for t in &trees {
        for e in t.all() {
            if groups.len() >= LIMIT_GROUPS {
                break;
            }
            if e.name == "Toolbar" {
                let mut tools = Vec::new();
                for bt in e.kids("ToolbarButton") {
                    if bt.attr("IsSeparator") == Some("true") {
                        continue;
                    }
                    let Some(r) = kid(bt.kid("MenuItem"), "MacroRef") else { continue };
                    if let Some(tool) = tool_of(&text_of(bt.kid("Name")), r.attr("MenuMacroID"), &mut notes, &mut missing) {
                        tools.push(tool);
                    }
                }
                if !tools.is_empty() {
                    let n = [text_of(e.kid("Name")), text_of(e.kid("Alias"))]
                        .into_iter()
                        .find(|s| !s.is_empty())
                        .unwrap_or_else(|| "Toolbar".into());
                    groups.push(CuixGroup { name: cc_cad::js::slice16(&n, 60), tools, shortcuts: false });
                }
            } else if e.name == "RibbonPanelSource" {
                let mut tools = Vec::new();
                for bt in e.all() {
                    let id =
                        bt.attr("MenuMacroID").filter(|s| !s.is_empty()).or_else(|| bt.kid("MacroRef").and_then(|r| r.attr("MenuMacroID")));
                    if id.is_none_or(str::is_empty) || std::ptr::eq(bt, e) {
                        continue;
                    }
                    let label = bt.attr("Text").filter(|s| !s.is_empty()).map(str::to_string).unwrap_or_else(|| text_of(bt.kid("Name")));
                    if let Some(tool) = tool_of(&label, id, &mut notes, &mut missing) {
                        tools.push(tool);
                    }
                }
                if !tools.is_empty() {
                    let n = [e.attr("Text").unwrap_or("").to_string(), text_of(e.kid("Name")), e.attr("UID").unwrap_or("").to_string()]
                        .into_iter()
                        .find(|s| !s.is_empty())
                        .unwrap_or_else(|| "Panel".into());
                    groups.push(CuixGroup { name: cc_cad::js::slice16(&n, 60), tools, shortcuts: false });
                }
            }
        }
    }
    // shortcut keys: a group of their own, each tool carrying its key
    let mut keyed = Vec::new();
    for t in &trees {
        for e in t.all() {
            if e.name != "MenuAccelerator" {
                continue;
            }
            let r = kid(e.kid("MenuItem"), "MacroRef").or_else(|| e.all().into_iter().find(|x| x.name == "MacroRef"));
            let key_text = [
                text_of(e.kid("AcceleratorShortcutKey")),
                text_of(e.kid("Key")),
                text_of(e.kid("Shortcut")),
                e.attr("Key").unwrap_or("").to_string(),
                e.attr("Shortcut").unwrap_or("").to_string(),
            ]
            .into_iter()
            .find(|s| !s.is_empty())
            .unwrap_or_default();
            let squeezed: String = key_text.chars().filter(|c| !ws(*c)).collect();
            let key = norm_key(&squeezed);
            let (Some(r), Some(key)) = (r, key) else {
                let k = cc_cad::js::slice16(&key_text, 40);
                notes.push(Note { key: "mytools.note.key".into(), fills: vec![("key".into(), if k.is_empty() { "?".into() } else { k })] });
                continue;
            };
            if let Some(mut tool) = tool_of(&text_of(e.kid("Name")), r.attr("MenuMacroID"), &mut notes, &mut missing) {
                tool.key = Some(key);
                keyed.push(tool);
            }
        }
    }
    if !keyed.is_empty() {
        groups.push(CuixGroup { name: "Shortcut keys".into(), tools: keyed, shortcuts: true });
    }
    // command aliases, when the package carries a PGP file (ALIAS, *COMMAND)
    let mut aliases = Vec::new();
    for (n, d) in &files {
        if !n.to_ascii_lowercase().ends_with(".pgp") {
            continue;
        }
        for line in decode(d).split('\n') {
            let line = line.strip_suffix('\r').unwrap_or(line);
            if let Some((a, c)) = pgp_line(line) {
                if !lookup(&c) && !missing.contains(&c.to_uppercase()) {
                    missing.push(c.to_uppercase());
                }
                aliases.push((a.to_uppercase(), c.to_uppercase()));
            }
        }
    }
    missing.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    Ok(Cuix { groups, aliases, missing, notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(entries: &[(&str, &[u8])]) -> Vec<u8> {
        crate::sheetset_io::zip(
            &entries.iter().map(|(n, d)| ((*n).to_string(), d.to_vec())).collect::<Vec<_>>(),
            crate::sheetset_io::DosTime { time: 0, date: 33 },
        )
    }

    #[test]
    fn a_small_cuix() {
        let cui = r#"<?xml version="1.0"?><CustSection><MenuGroup><MacroGroup>
          <MenuMacro UID="M1"><Macro><Name>Line it</Name><Command>^C^C_LINE</Command><SmallImage Name="ico"/></Macro></MenuMacro>
          <MenuMacro UID="M2"><Macro><Name>Fancy &amp; more</Name><Command>^C^CZAPIT</Command></Macro></MenuMacro>
        </MacroGroup><MenuRoot><ToolbarRoot><Toolbar><Name>Mine</Name>
          <ToolbarButton><Name>L</Name><MenuItem><MacroRef MenuMacroID="M1"/></MenuItem></ToolbarButton>
          <ToolbarButton IsSeparator="true"/>
          <ToolbarButton><Name></Name><MenuItem><MacroRef MenuMacroID="M2"/></MenuItem></ToolbarButton>
          <ToolbarButton><MenuItem><MacroRef MenuMacroID="NOPE"/></MenuItem></ToolbarButton>
        </Toolbar></ToolbarRoot><AcceleratorRoot>
          <MenuAccelerator><MenuItem><MacroRef MenuMacroID="M1"/></MenuItem><AcceleratorShortcutKey>CTRL + SHIFT + Q</AcceleratorShortcutKey></MenuAccelerator>
          <MenuAccelerator Key="??"/>
        </AcceleratorRoot></MenuRoot></MenuGroup></CustSection>"#;
        let png = [0x89u8, 0x50, 0x4e, 0x47, 13, 10, 26, 10, 0];
        let z = stored(&[("acad.cui", cui.as_bytes()), ("Images/ICO.PNG", &png), ("acad.pgp", b"ZZ, *ZAPIT\r\nL,*LINE\nbad line")]);
        let r = read_cuix(&z, &|c| c == "LINE", &|_| String::new()).unwrap();
        assert_eq!(r.groups.len(), 2);
        assert_eq!(r.groups[0].name, "Mine");
        assert_eq!(r.groups[0].tools.len(), 2);
        assert_eq!(r.groups[0].tools[1].label, "Fancy & more");
        assert!(r.groups[0].tools[0].icon.is_some());
        assert_eq!(r.groups[1].tools[0].key.as_deref(), Some("Ctrl+Shift+Q"));
        assert_eq!(r.missing, vec!["ZAPIT"]);
        assert_eq!(r.aliases.len(), 2);
        assert_eq!(r.notes.iter().map(|n| n.key.as_str()).collect::<Vec<_>>(), vec!["mytools.note.no_macro", "mytools.note.key"]);
        // hostile input
        assert_eq!(read_cuix(b"PK", &|_| true, &|_| String::new()), Err(CuixErr("zip")));
        assert_eq!(read_cuix(&stored(&[("a.cui", b"<x/>")]), &|_| true, &|_| String::new()), Err(CuixErr("not_cuix")));
        assert_eq!(parse_xml("<!DOCTYPE x><a/>"), Err(CuixErr("doctype")));
        assert_eq!(parse_xml("<a><b></a>"), Err(CuixErr("xml")));
        assert_eq!(parse_xml(&"<a>".repeat(70)), Err(CuixErr("too_big")));
        let r = parse_xml("<a k='1' j = \"&lt;&#65;&#x42;&zz;\"  bad x1 y=\"2\">t<![CDATA[<c>]]><!-- no --></a>").unwrap();
        let a = &r.children[0];
        assert_eq!(a.attr("j"), Some("<AB&zz;"));
        assert_eq!(a.attr("y"), Some("2"));
        assert_eq!(a.text, "t<c>");
    }

    #[test]
    fn bmp_pictures() {
        // a 2 × 1 24-bit BMP
        let mut b = vec![0u8; 54];
        b[0] = 0x42;
        b[1] = 0x4d;
        b[10] = 54;
        b[14] = 40;
        b[18] = 2;
        b[22] = 1;
        b[28] = 24;
        b.extend_from_slice(&[1, 2, 3, 4, 5, 6, 0, 0]);
        let png = bmp_to_png(&b).unwrap();
        assert!(is_png(&png));
        b[28] = 16;
        assert_eq!(bmp_to_png(&b), None);
    }
}
