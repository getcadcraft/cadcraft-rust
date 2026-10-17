//! A small PDF writer: objects, content streams with pdf-lib's number text (`numberToString`, quirks included, so
//! pdf.js reads back the very numbers it reads from pdf-lib's files), a new document from scratch, and an
//! incremental update of an existing file (the original bytes are kept; new and replaced objects are appended with
//! their own cross-reference section). Objects of the original are re-serialized from `hayro-syntax`'s parse.

use hayro::hayro_interpret::hayro_syntax::object::{Dict, MaybeRef, Object, ObjectIdentifier};

use cc_takeoff::jv;

/// A PDF object to write.
#[derive(Debug, Clone, PartialEq)]
pub enum PO {
    Null,
    Bool(bool),
    Num(f64),
    Name(String),
    /// a text string (written as a hex string of these bytes)
    Str(Vec<u8>),
    Arr(Vec<PO>),
    Dict(Vec<(String, PO)>),
    Ref(u32, u16),
    /// already serialized bytes (an object copied from the original file)
    Raw(Vec<u8>),
}
impl PO {
    pub fn name(s: &str) -> PO {
        PO::Name(s.to_string())
    }
    pub fn dict(entries: Vec<(&str, PO)>) -> PO {
        PO::Dict(entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }
    pub fn get(&self, k: &str) -> Option<&PO> {
        match self {
            PO::Dict(e) => e.iter().find(|(kk, _)| kk == k).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn set(&mut self, k: &str, v: PO) {
        if let PO::Dict(e) = self {
            if let Some(slot) = e.iter_mut().find(|(kk, _)| kk == k) {
                slot.1 = v;
            } else {
                e.push((k.to_string(), v));
            }
        }
    }
}

/// pdf-lib's `numberToString` (the text of a number in a PDF it writes).
pub fn number_to_string(num0: f64) -> String {
    let mut num = num0;
    let mut s = jv::num_str(num);
    if num.abs() < 1.0 {
        let t = jv::num_str(num);
        let e = t.split_once("e-").map(|(_, e)| jv::parse_int(e)).unwrap_or(0.0);
        if e != 0.0 && !e.is_nan() {
            let negative = num < 0.0;
            if negative {
                num *= -1.0;
            }
            num *= cc_takeoff::jsmath::pow(10.0, e - 1.0);
            let ns = jv::num_str(num);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let zeros = (e - 1.0).clamp(0.0, 400.0) as usize;
            s = format!("0.{}{}", "0".repeat(zeros), ns.get(2..).unwrap_or(""));
            if negative {
                s = format!("-{s}");
            }
        }
    } else {
        let t = jv::num_str(num);
        let mut e = t.split_once('+').map(|(_, e)| jv::parse_int(e)).unwrap_or(0.0);
        if e > 20.0 {
            e -= 20.0;
            num /= cc_takeoff::jsmath::pow(10.0, e);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let zeros = e.clamp(0.0, 400.0) as usize;
            s = format!("{}{}", jv::num_str(num), "0".repeat(zeros));
        }
    }
    s
}

fn name_bytes(out: &mut Vec<u8>, n: &[u8]) {
    out.push(b'/');
    for &c in n {
        let regular =
            (0x21..=0x7E).contains(&c) && !matches!(c, b'#' | b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%');
        if regular {
            out.push(c);
        } else {
            out.extend_from_slice(format!("#{c:02X}").as_bytes());
        }
    }
}
fn hex_string(out: &mut Vec<u8>, b: &[u8]) {
    out.push(b'<');
    for c in b {
        out.extend_from_slice(format!("{c:02X}").as_bytes());
    }
    out.push(b'>');
}

/// Serialize an object.
pub fn write(out: &mut Vec<u8>, o: &PO) {
    match o {
        PO::Null => out.extend_from_slice(b"null"),
        PO::Bool(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        PO::Num(x) => out.extend_from_slice(number_to_string(*x).as_bytes()),
        PO::Name(n) => name_bytes(out, n.as_bytes()),
        PO::Str(b) => hex_string(out, b),
        PO::Arr(l) => {
            out.push(b'[');
            for (i, x) in l.iter().enumerate() {
                if i > 0 {
                    out.push(b' ');
                }
                write(out, x);
            }
            out.push(b']');
        }
        PO::Dict(e) => {
            out.extend_from_slice(b"<<");
            for (k, v) in e {
                name_bytes(out, k.as_bytes());
                out.push(b' ');
                write(out, v);
                out.push(b'\n');
            }
            out.extend_from_slice(b">>");
        }
        PO::Ref(n, g) => out.extend_from_slice(format!("{n} {g} R").as_bytes()),
        PO::Raw(b) => out.extend_from_slice(b),
    }
}

/// A content stream being built: operators with pdf-lib's operand text.
#[derive(Debug, Clone, Default)]
pub struct Content {
    pub bytes: Vec<u8>,
}
impl Content {
    /// One operator with numeric operands.
    pub fn op(&mut self, name: &str, args: &[f64]) {
        for a in args {
            self.bytes.extend_from_slice(number_to_string(*a).as_bytes());
            self.bytes.push(b' ');
        }
        self.bytes.extend_from_slice(name.as_bytes());
        self.bytes.push(b'\n');
    }
    /// An operator with a name operand (`/GS1 gs`, `/F1 12 Tf`).
    pub fn op_name(&mut self, name: &str, n: &str, args: &[f64]) {
        name_bytes(&mut self.bytes, n.as_bytes());
        self.bytes.push(b' ');
        for a in args {
            self.bytes.extend_from_slice(number_to_string(*a).as_bytes());
            self.bytes.push(b' ');
        }
        self.bytes.extend_from_slice(name.as_bytes());
        self.bytes.push(b'\n');
    }
    /// `[a b] phase d`
    pub fn dash(&mut self, arr: &[f64], phase: f64) {
        self.bytes.push(b'[');
        let parts: Vec<String> = arr.iter().map(|v| number_to_string(*v)).collect();
        self.bytes.extend_from_slice(parts.join(" ").as_bytes());
        self.bytes.extend_from_slice(b"] ");
        self.bytes.extend_from_slice(number_to_string(phase).as_bytes());
        self.bytes.extend_from_slice(b" d\n");
    }
    /// `<hex> Tj`
    pub fn show_hex(&mut self, codes: &[u8]) {
        hex_string(&mut self.bytes, codes);
        self.bytes.extend_from_slice(b" Tj\n");
    }
}

/// A stream object: its dictionary (without Length) and data.
pub fn stream_obj(dict: Vec<(&str, PO)>, data: &[u8]) -> Vec<u8> {
    let mut d: Vec<(String, PO)> = dict.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    d.push(("Length".into(), PO::Num(data.len() as f64)));
    let mut out = Vec::new();
    write(&mut out, &PO::Dict(d));
    out.extend_from_slice(b"\nstream\n");
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// Objects appended to a file: (number, generation, serialized body).
#[derive(Debug, Default)]
pub struct Writer {
    pub objs: Vec<(u32, u16, Vec<u8>)>,
    pub next: u32,
}
impl Writer {
    pub fn new(first_free: u32) -> Self {
        Writer { objs: Vec::new(), next: first_free.max(1) }
    }
    /// A new object number.
    pub fn alloc(&mut self) -> u32 {
        let n = self.next;
        self.next = self.next.saturating_add(1);
        n
    }
    pub fn put(&mut self, n: u32, g: u16, body: Vec<u8>) {
        self.objs.push((n, g, body));
    }
    pub fn add(&mut self, o: &PO) -> PO {
        let n = self.alloc();
        let mut b = Vec::new();
        write(&mut b, o);
        self.put(n, 0, b);
        PO::Ref(n, 0)
    }
    pub fn add_stream(&mut self, dict: Vec<(&str, PO)>, data: &[u8]) -> PO {
        let n = self.alloc();
        self.put(n, 0, stream_obj(dict, data));
        PO::Ref(n, 0)
    }
    /// Write the objects after `base` (empty for a new file) with a cross-reference section and trailer.
    pub fn finish(mut self, base: &[u8], trailer: Vec<(&str, PO)>) -> Vec<u8> {
        let mut out = base.to_vec();
        if base.is_empty() {
            out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
        } else if !out.ends_with(b"\n") {
            out.push(b'\n');
        }
        self.objs.sort_by_key(|o| o.0);
        let mut offsets: Vec<(u32, u16, usize)> = Vec::new();
        for (n, g, body) in &self.objs {
            offsets.push((*n, *g, out.len()));
            out.extend_from_slice(format!("{n} {g} obj\n").as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref_at = out.len();
        out.extend_from_slice(b"xref\n");
        if base.is_empty() {
            out.extend_from_slice(b"0 1\n0000000000 65535 f \n");
        }
        for (n, g, off) in &offsets {
            out.extend_from_slice(format!("{n} 1\n{off:010} {g:05} n \n").as_bytes());
        }
        let mut t: Vec<(String, PO)> = trailer.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        t.insert(0, ("Size".into(), PO::Num(f64::from(self.next))));
        out.extend_from_slice(b"trailer\n");
        write(&mut out, &PO::Dict(t));
        out.extend_from_slice(format!("\nstartxref\n{xref_at}\n%%EOF\n").as_bytes());
        out
    }
}

// ---------------------------------------------------------------------------------------------------- the original

fn find_last(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (0..=hay.len().saturating_sub(needle.len())).rev().find(|&i| hay.get(i..i.saturating_add(needle.len())) == Some(needle))
}
fn find_from(hay: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (from..=hay.len().saturating_sub(needle.len())).find(|&i| hay.get(i..i.saturating_add(needle.len())) == Some(needle))
}
fn int_at(b: &[u8], mut i: usize) -> Option<(u64, usize)> {
    while b.get(i).is_some_and(u8::is_ascii_whitespace) {
        i = i.saturating_add(1);
    }
    let s = i;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i = i.saturating_add(1);
    }
    if i == s || i.saturating_sub(s) > 15 {
        return None;
    }
    std::str::from_utf8(b.get(s..i)?).ok()?.parse().ok().map(|v| (v, i))
}

/// What an incremental update needs of the original's last trailer.
#[derive(Debug, Clone, Default)]
pub struct Trailer {
    pub startxref: u64,
    pub size: u64,
    pub root: Option<(u32, u16)>,
    pub info: Option<(u32, u16)>,
    pub encrypted: bool,
}

fn key_ref(dict: &[u8], key: &[u8]) -> Option<(u32, u16)> {
    let mut at = 0usize;
    while let Some(i) = find_from(dict, at, key) {
        at = i.saturating_add(key.len());
        // the key must end here (not /InfoX)
        if dict.get(at).is_some_and(|c| c.is_ascii_alphanumeric()) {
            continue;
        }
        let (n, j) = int_at(dict, at)?;
        let (g, k) = int_at(dict, j)?;
        let mut k2 = k;
        while dict.get(k2).is_some_and(u8::is_ascii_whitespace) {
            k2 = k2.saturating_add(1);
        }
        if dict.get(k2) == Some(&b'R') {
            return Some((u32::try_from(n).ok()?, u16::try_from(g).ok()?));
        }
    }
    None
}
fn key_int(dict: &[u8], key: &[u8]) -> Option<u64> {
    let mut at = 0usize;
    while let Some(i) = find_from(dict, at, key) {
        at = i.saturating_add(key.len());
        if dict.get(at).is_some_and(|c| c.is_ascii_alphanumeric()) {
            continue;
        }
        return int_at(dict, at).map(|v| v.0);
    }
    None
}
fn dict_span(b: &[u8], from: usize) -> Option<&[u8]> {
    let s = find_from(b, from, b"<<")?;
    let mut depth = 0i32;
    let mut i = s;
    while i.saturating_add(1) < b.len() {
        match (b.get(i), b.get(i.saturating_add(1))) {
            (Some(b'<'), Some(b'<')) => {
                depth = depth.saturating_add(1);
                i = i.saturating_add(2);
            }
            (Some(b'>'), Some(b'>')) => {
                depth = depth.saturating_sub(1);
                i = i.saturating_add(2);
                if depth == 0 {
                    return b.get(s..i);
                }
            }
            _ => i = i.saturating_add(1),
        }
        if i.saturating_sub(s) > 1 << 20 {
            return None;
        }
    }
    None
}

/// The original's last trailer (classic `trailer` or a cross-reference stream's dictionary).
pub fn read_trailer(bytes: &[u8]) -> Result<Trailer, String> {
    let sx = find_last(bytes, b"startxref").ok_or("no startxref")?;
    let (off, _) = int_at(bytes, sx.saturating_add(9)).ok_or("bad startxref")?;
    let o = usize::try_from(off).map_err(|_| "bad startxref")?;
    let dict = if bytes.get(o..o.saturating_add(4)) == Some(b"xref") {
        let t = find_from(bytes, o, b"trailer").ok_or("no trailer")?;
        dict_span(bytes, t).ok_or("bad trailer")?
    } else {
        dict_span(bytes, o).ok_or("bad xref stream")?
    };
    Ok(Trailer {
        startxref: off,
        size: key_int(dict, b"/Size").unwrap_or(0),
        root: key_ref(dict, b"/Root"),
        info: key_ref(dict, b"/Info"),
        encrypted: find_from(dict, 0, b"/Encrypt").is_some(),
    })
}

/// Serialize an object of the original (references kept as references; depth-capped).
pub fn copy_obj(out: &mut Vec<u8>, o: &MaybeRef<Object<'_>>, depth: u32) {
    match o {
        MaybeRef::Ref(r) => {
            let (n, g) = ref_parts(r);
            out.extend_from_slice(format!("{n} {g} R").as_bytes());
        }
        MaybeRef::NotRef(obj) => copy_direct(out, obj, depth),
    }
}
fn ref_parts(r: &hayro::hayro_interpret::hayro_syntax::object::ObjRef) -> (i32, i32) {
    (r.obj_number, r.gen_number)
}
fn copy_direct(out: &mut Vec<u8>, obj: &Object<'_>, depth: u32) {
    if depth > 64 {
        out.extend_from_slice(b"null");
        return;
    }
    match obj {
        Object::Null(_) => out.extend_from_slice(b"null"),
        Object::Boolean(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        Object::Number(n) => {
            let v = n.as_f64();
            out.extend_from_slice(if v.is_finite() { jv::num_str(v) } else { "0".into() }.as_bytes());
        }
        Object::String(s) => hex_string(out, s.as_bytes()),
        Object::Name(n) => name_bytes(out, n),
        Object::Array(a) => {
            out.push(b'[');
            for (i, x) in a.raw_iter().take(1_000_000).enumerate() {
                if i > 0 {
                    out.push(b' ');
                }
                copy_obj(out, &x, depth.saturating_add(1));
            }
            out.push(b']');
        }
        Object::Dict(d) => copy_dict(out, d, &[], depth.saturating_add(1)),
        // a stream is always indirect in a valid file; a direct one is dropped
        Object::Stream(_) => out.extend_from_slice(b"null"),
    }
}
/// A dictionary of the original without some keys.
pub fn copy_dict(out: &mut Vec<u8>, d: &Dict<'_>, skip: &[&str], depth: u32) {
    out.extend_from_slice(b"<<");
    for (k, v) in d.entries().take(100_000) {
        if skip.iter().any(|s| s.as_bytes() == &*k) {
            continue;
        }
        name_bytes(out, &k);
        out.push(b' ');
        copy_obj(out, &v, depth);
        out.push(b'\n');
    }
    out.extend_from_slice(b">>");
}
/// The entries of a dictionary of the original as POs (values copied as raw bytes).
pub fn dict_entries(d: &Dict<'_>, skip: &[&str]) -> Vec<(String, PO)> {
    d.entries()
        .take(100_000)
        .filter(|(k, _)| !skip.iter().any(|s| s.as_bytes() == &**k))
        .map(|(k, v)| {
            let mut b = Vec::new();
            copy_obj(&mut b, &v, 1);
            (String::from_utf8_lossy(&k).to_string(), PO::Raw(b))
        })
        .collect()
}

/// An object identifier as written.
pub fn id_parts(id: ObjectIdentifier) -> Option<(u32, u16)> {
    Some((u32::try_from(id.obj_number).ok()?, u16::try_from(id.gen_number).ok()?))
}

/// The PDF date of a time (UTC): `D:YYYYMMDDHHmmSSZ`.
pub fn pdf_date(st: &crate::xlsx::Stamp) -> String {
    format!("D:{:04}{:02}{:02}{:02}{:02}{:02}Z", st.year, st.month, st.day, st.hour, st.min, st.sec)
}
