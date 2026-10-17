//! dbf.js — the dBASE III table of a shapefile (.dbf), read and written. Encodings through `encoding_rs` (the WHATWG
//! Encoding standard, what `TextDecoder` implements): the .cpg text when it names one, else the LDID byte, else UTF-8
//! when the text is valid UTF-8, else Windows-1252.

use crate::gjs::{u8_at, u16le, u32le};
use cc_civil::i18n::F;
use cc_civil::js::{G, J, Obj, is_ws, num_to_string, pad_start, round, slice16, str_to_number, to_exponential, to_fixed, to_js_string};
use cc_civil::tr;
use encoding_rs::Encoding;
use serde_json::{Map, Value};
use unicode_normalization::UnicodeNormalization;

/// A read field.
#[derive(Clone, Debug)]
pub struct DbfField {
    pub name: String,
    pub ty: &'static str,
    pub dbf_type: char,
    pub length: u8,
    pub decimals: u8,
    off: usize,
}
impl J for DbfField {
    fn j(&self) -> Value {
        cc_civil::jo! { "name" => self.name, "type" => self.ty, "dbfType" => self.dbf_type.to_string(), "length" => u32::from(self.length), "decimals" => u32::from(self.decimals) }
    }
}
/// readDbf's result.
#[derive(Clone, Debug, Default)]
pub struct Dbf {
    pub fields: Vec<DbfField>,
    pub records: Vec<Map<String, Value>>,
    pub encoding: String,
    pub encoding_from: String,
    pub notes: Vec<String>,
}
impl J for Dbf {
    fn j(&self) -> Value {
        Obj::new()
            .set("fields", &self.fields)
            .set("records", self.records.iter().map(|r| Value::Object(r.clone())).collect::<Vec<_>>())
            .set("encoding", &self.encoding)
            .set("encodingFrom", &self.encoding_from)
            .set("notes", &self.notes)
            .done()
    }
}

/// The JS's DbfError message.
#[derive(Clone, Debug)]
pub struct DbfError(pub String);

fn ci_eq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}
/// `cpgLabel(text)`: an encoding label from a .cpg text.
pub fn cpg_label(text: &str) -> Option<String> {
    let t = cc_civil::js::trim(text);
    if t.is_empty() {
        return None;
    }
    let l = t.to_ascii_lowercase();
    // /^(utf-?8|65001)$/i
    if l == "utf8" || l == "utf-8" || l == "65001" {
        return Some("utf-8".into());
    }
    // /^(ansi\s*)?(cp|windows-?)?(125[0-8])$/i (1252 included: the same answer)
    let mut rest: &str = &l;
    if let Some(r) = rest.strip_prefix("ansi") {
        rest = r.trim_start_matches(is_ws);
    }
    let rest2 = rest.strip_prefix("cp").or_else(|| rest.strip_prefix("windows-")).or_else(|| rest.strip_prefix("windows")).unwrap_or(rest);
    if rest2.len() == 4 && rest2.starts_with("125") && rest2.as_bytes().get(3).is_some_and(|c| (b'0'..=b'8').contains(c)) {
        return Some(format!("windows-{rest2}"));
    }
    // /^(iso-?)?8859-?(\d{1,2})$/i
    let r = l.strip_prefix("iso-").or_else(|| l.strip_prefix("iso")).unwrap_or(&l);
    if let Some(r) = r.strip_prefix("8859") {
        let d = r.strip_prefix('-').unwrap_or(r);
        if (1..=2).contains(&d.len()) && d.bytes().all(|c| c.is_ascii_digit()) {
            return Some(format!("iso-8859-{d}"));
        }
    }
    // /^(cp)?866$/i, koi8-r, gbk, big5, shift_jis, euc-kr
    let s = l.strip_prefix("cp").unwrap_or(&l);
    if s == "866" {
        return Some("ibm866".into());
    }
    if l == "koi8r" || l == "koi8-r" {
        return Some("koi8-r".into());
    }
    if s == "936" || l == "gbk" || l == "gb2312" {
        return Some("gbk".into());
    }
    if s == "950" || l == "big5" {
        return Some("big5".into());
    }
    if s == "932" || l == "shift_jis" || l == "shiftjis" {
        return Some("shift_jis".into());
    }
    if s == "949" || l == "euc-kr" || l == "euckr" {
        return Some("euc-kr".into());
    }
    let _ = ci_eq;
    None
}

fn ldid(b: u8) -> Option<&'static str> {
    Some(match b {
        0x57 | 0x03 => "windows-1252",
        0xc8 => "windows-1250",
        0xc9 => "windows-1251",
        0xcb => "windows-1253",
        0xca => "windows-1254",
        0x7d => "windows-1255",
        0x7e => "windows-1256",
        0x4d => "gbk",
        0x4e => "euc-kr",
        0x4f => "big5",
        0x13 => "shift_jis",
        0x26 => "ibm866",
        _ => return None,
    })
}

fn blank(b: &[u8], s: usize, e: usize) -> bool {
    (s..e).all(|i| matches!(b.get(i), Some(32 | 0) | None))
}
fn latin_trim(b: &[u8], s: usize, e: usize) -> String {
    let t: String = b.get(s..e.min(b.len())).unwrap_or(&[]).iter().map(|&c| char::from(c)).collect();
    cc_civil::js::trim(&t).to_string()
}
/// `TextDecoder(enc).decode(bytes)` (non-fatal; a UTF-8 BOM dropped).
pub fn decode(enc: &'static Encoding, b: &[u8]) -> String {
    enc.decode_with_bom_removal(b).0.into_owned()
}

/// `readDbf(bytes, { cpg, max })`.
pub fn read_dbf(b: &[u8], cpg: Option<&str>, max: usize) -> Result<Dbf, DbfError> {
    let mut notes = Vec::new();
    if b.len() < 33 {
        return Err(DbfError(tr!("dbf.short")));
    }
    let n = u32le(b, 4);
    let hlen = usize::from(u16le(b, 8));
    let rlen = usize::from(u16le(b, 10));
    if hlen < 33 || hlen > b.len() || rlen < 1 {
        return Err(DbfError(tr!("dbf.header")));
    }
    let mut enc: Option<String> = cpg.and_then(cpg_label);
    let mut from = if enc.is_some() { "cpg" } else { "" }.to_string();
    if let Some(c) = cpg
        && !c.is_empty()
        && enc.is_none()
    {
        notes.push(tr!("dbf.cpg.unknown", cpg = slice16(cc_civil::js::trim(c), 30)));
    }
    if enc.is_none()
        && let Some(l) = ldid(u8_at(b, 29))
    {
        enc = Some(l.into());
        from = "ldid".into();
    }
    let mut fields: Vec<DbfField> = Vec::new();
    let mut raw_names: Vec<&[u8]> = Vec::new();
    let mut off = 1usize;
    let mut p = 32usize;
    while p.saturating_add(32) <= hlen && u8_at(b, p) != 0x0d {
        let mut e = p;
        while e < p.saturating_add(11) && u8_at(b, e) != 0 {
            e = e.saturating_add(1);
        }
        raw_names.push(b.get(p..e).unwrap_or(&[]));
        let ft = char::from(u8_at(b, p.saturating_add(11)));
        let len = u8_at(b, p.saturating_add(16));
        let dec = u8_at(b, p.saturating_add(17));
        let ty = match ft {
            'N' | 'F' => "number",
            'L' => "bool",
            'D' => "date",
            _ => "text",
        };
        fields.push(DbfField { name: String::new(), ty, dbf_type: ft, length: len, decimals: dec, off });
        off = off.saturating_add(usize::from(len));
        if fields.len() > 2046 {
            return Err(DbfError(tr!("dbf.fields.many")));
        }
        p = p.saturating_add(32);
    }
    if off > rlen {
        return Err(DbfError(tr!("dbf.fields.fit")));
    }
    let avail = b.len().saturating_sub(hlen).checked_div(rlen).unwrap_or(0);
    let mut count = usize::try_from(n).unwrap_or(usize::MAX);
    if count > avail {
        count = avail;
        notes.push(tr!("dbf.cut", n = f64::from(n), avail = avail));
    }
    if count > max {
        count = max;
        notes.push(tr!("dbf.max", n = max));
    }
    if enc.is_none() {
        let end = hlen.saturating_add(count.saturating_mul(rlen).min(4_000_000)).min(b.len());
        let sample = b.get(hlen..end).unwrap_or(&[]);
        let utf = std::str::from_utf8(sample).is_ok() && std::str::from_utf8(b.get(32..hlen).unwrap_or(&[])).is_ok();
        enc = Some(if utf { "utf-8" } else { "windows-1252" }.into());
        from = "guess".into();
        if !utf {
            notes.push(tr!("dbf.noenc"));
        }
    }
    let mut enc_label = enc.unwrap_or_else(|| "windows-1252".into());
    let dec: &'static Encoding = match Encoding::for_label(enc_label.as_bytes()) {
        Some(e) => e,
        None => {
            enc_label = "windows-1252".into();
            encoding_rs::WINDOWS_1252
        }
    };
    let mut seen: Vec<String> = Vec::new();
    for (i, r) in raw_names.iter().enumerate() {
        let d = decode(dec, r);
        let mut nm = slice16(cc_civil::js::trim(&crate::gjs::drop_controls(&d)), 64);
        if nm.is_empty() {
            nm = format!("FIELD{}", i.saturating_add(1));
        }
        let mut k = nm.clone();
        let mut j = 1u64;
        while seen.contains(&k) {
            k = format!("{nm}_{j}");
            j = j.saturating_add(1);
        }
        seen.push(k.clone());
        if let Some(f) = fields.get_mut(i) {
            f.name = k;
        }
    }
    let mut records = Vec::new();
    let mut deleted = 0usize;
    let mut memo = false;
    for r in 0..count {
        let p = hlen.saturating_add(r.saturating_mul(rlen));
        if u8_at(b, p) == 0x2a {
            deleted = deleted.saturating_add(1);
            continue;
        }
        if u8_at(b, p) == 0x1a {
            break;
        }
        let mut o = Map::new();
        for f in &fields {
            let s = p.saturating_add(f.off);
            let e = s.saturating_add(usize::from(f.length));
            if f.dbf_type == 'M' {
                memo = true;
                o.insert(f.name.clone(), Value::Null);
                continue;
            }
            if blank(b, s, e) {
                o.insert(f.name.clone(), Value::Null);
                continue;
            }
            let v = match f.ty {
                "number" => {
                    let lt = latin_trim(b, s, e);
                    let x = str_to_number(&lt.replacen(',', ".", 1));
                    if lt.is_empty() || !x.is_finite() { Value::Null } else { crate::gjs::num_keep(x) }
                }
                "bool" => match char::from(u8_at(b, s)) {
                    'T' | 't' | 'Y' | 'y' => Value::Bool(true),
                    'F' | 'f' | 'N' | 'n' => Value::Bool(false),
                    _ => Value::Null,
                },
                "date" => {
                    let lt = latin_trim(b, s, e);
                    if lt.len() == 8 && lt.bytes().all(|c| c.is_ascii_digit()) {
                        Value::String(format!(
                            "{}-{}-{}",
                            lt.get(0..4).unwrap_or(""),
                            lt.get(4..6).unwrap_or(""),
                            lt.get(6..).unwrap_or("")
                        ))
                    } else {
                        Value::Null
                    }
                }
                _ => {
                    let d = decode(dec, b.get(s..e.min(b.len())).unwrap_or(&[]));
                    Value::String(cc_civil::js::trim(&cc_civil::js::strip_controls(&d, " ")).to_string())
                }
            };
            o.insert(f.name.clone(), v);
        }
        records.push(o);
    }
    if deleted > 0 {
        notes.push(tr!("dbf.deleted", n = deleted));
    }
    if memo {
        notes.push(tr!("dbf.memo"));
    }
    Ok(Dbf { fields, records, encoding: enc_label, encoding_from: from, notes })
}

/// `dbfNames(names)`: ≤ 10 characters, A–Z a–z 0–9 _, unique (case-blind).
pub fn dbf_names(names: &[String]) -> Vec<String> {
    let mut used: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for n in names {
        let mut base = String::new();
        for c in n.nfkd() {
            if c.is_ascii_alphanumeric() || c == '_' {
                base.push(c);
            } else {
                for _ in 0..c.len_utf16() {
                    base.push('_');
                }
            }
        }
        base.truncate(10);
        if base.is_empty() {
            base = "FIELD".into();
        }
        let mut k = base.clone();
        let mut j = 1u64;
        while used.contains(&k.to_uppercase()) {
            let s = format!("_{j}");
            j = j.saturating_add(1);
            let keep = 10usize.saturating_sub(s.len());
            k = format!("{}{}", base.get(..keep.min(base.len())).unwrap_or(""), s);
        }
        used.push(k.to_uppercase());
        out.push(k);
    }
    out
}

/// Cut UTF-8 bytes to ≤ max without splitting a character.
fn cut(u: &[u8], max: usize) -> &[u8] {
    if u.len() <= max {
        return u;
    }
    let mut e = max;
    while e > 0 && (u8_at(u, e) & 0xc0) == 0x80 {
        e = e.saturating_sub(1);
    }
    u.get(..e).unwrap_or(&[])
}
fn num_text(v: f64, dec: usize) -> String {
    if dec > 0 { to_fixed(v, dec) } else { num_to_string(round(v)) }
}

/// A field to write `{name, type}`.
#[derive(Clone, Debug)]
pub struct WField {
    pub name: String,
    pub ty: String,
}

/// `writeDbf(fields, records)` → (bytes, names). `date` = the local (year, month 1–12, day) the header carries.
pub fn write_dbf(fields: &[cc_civil::gis_state::Field], records: &[&Map<String, Value>], date: (i32, u32, u32)) -> (Vec<u8>, Vec<String>) {
    let names = dbf_names(&fields.iter().map(|f| f.name.clone()).collect::<Vec<_>>());
    struct Def {
        name: String,
        key: String,
        t: u8,
        length: usize,
        decimals: usize,
        kind: &'static str,
    }
    let defs: Vec<Def> = fields
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let name = names.g(i);
            let vals: Vec<Option<&Value>> = records.iter().map(|r| r.get(&f.name)).collect();
            match f.ty.as_str() {
                "number" => {
                    let nums: Vec<f64> = vals.iter().filter_map(|v| v.and_then(Value::as_f64)).filter(|x| x.is_finite()).collect();
                    let ints = nums.iter().all(|v| cc_civil::js::is_integer(*v) && v.abs() < 1e15);
                    let mut decimals = 0usize;
                    if !ints {
                        decimals = 1;
                        for &v in &nums {
                            let mut d = 0usize;
                            while d < 8 && (str_to_number(&to_fixed(v, d)) - v).abs() > v.abs() * 1e-14 {
                                d = d.saturating_add(1);
                            }
                            if d > decimals {
                                decimals = d;
                            }
                        }
                    }
                    let w = nums.iter().map(|&v| num_text(v, decimals).len()).fold(1usize, usize::max);
                    let length = 24usize.min(w.max(if decimals > 0 { decimals.saturating_add(3) } else { 1 }));
                    Def { name, key: f.name.clone(), t: b'N', length, decimals, kind: "number" }
                }
                "bool" => Def { name, key: f.name.clone(), t: b'L', length: 1, decimals: 0, kind: "bool" },
                "date" => Def { name, key: f.name.clone(), t: b'D', length: 8, decimals: 0, kind: "date" },
                _ => {
                    let len = vals
                        .iter()
                        .map(|v| match v {
                            None | Some(Value::Null) => 0,
                            Some(x) => cut(to_js_string(x).as_bytes(), 254).len(),
                        })
                        .fold(1usize, usize::max);
                    Def { name, key: f.name.clone(), t: b'C', length: len.min(254), decimals: 0, kind: "text" }
                }
            }
        })
        .collect();
    let rlen = defs.iter().fold(1usize, |s, d| s.saturating_add(d.length));
    let hlen = 32usize.saturating_add(defs.len().saturating_mul(32)).saturating_add(1);
    let total = hlen.saturating_add(rlen.saturating_mul(records.len())).saturating_add(1);
    let mut out = vec![0u8; total];
    let set = |out: &mut Vec<u8>, i: usize, v: u8| {
        if let Some(s) = out.get_mut(i) {
            *s = v;
        }
    };
    set(&mut out, 0, 0x03);
    set(&mut out, 1, u8::try_from((date.0.saturating_sub(1900)).rem_euclid(256)).unwrap_or(0));
    set(&mut out, 2, u8::try_from(date.1 % 256).unwrap_or(0));
    set(&mut out, 3, u8::try_from(date.2 % 256).unwrap_or(0));
    crate::gjs::put(&mut out, 4, &(u32::try_from(records.len()).unwrap_or(u32::MAX)).to_le_bytes());
    crate::gjs::put(&mut out, 8, &(u16::try_from(hlen % 65536).unwrap_or(0)).to_le_bytes());
    crate::gjs::put(&mut out, 10, &(u16::try_from(rlen % 65536).unwrap_or(0)).to_le_bytes());
    for (i, d) in defs.iter().enumerate() {
        let p = 32usize.saturating_add(i.saturating_mul(32));
        for (k, c) in d.name.encode_utf16().enumerate() {
            set(&mut out, p.saturating_add(k), u8::try_from(c & 0xff).unwrap_or(0));
        }
        set(&mut out, p.saturating_add(11), d.t);
        set(&mut out, p.saturating_add(16), u8::try_from(d.length & 0xff).unwrap_or(0));
        set(&mut out, p.saturating_add(17), u8::try_from(d.decimals & 0xff).unwrap_or(0));
    }
    set(&mut out, hlen.saturating_sub(1), 0x0d);
    for i in hlen..hlen.saturating_add(rlen.saturating_mul(records.len())) {
        set(&mut out, i, 0x20);
    }
    for (ri, r) in records.iter().enumerate() {
        let mut p = hlen.saturating_add(ri.saturating_mul(rlen)).saturating_add(1);
        for d in &defs {
            let v = r.get(&d.key);
            let mut s: Option<String> = None;
            let present = match v {
                None | Some(Value::Null) => false,
                Some(Value::String(x)) => !x.is_empty(),
                _ => true,
            };
            if present && let Some(v) = v {
                match d.kind {
                    "number" => {
                        if let Some(x) = v.as_f64().filter(|x| x.is_finite() && v.is_number()) {
                            let mut t = num_text(x, d.decimals);
                            if t.len() > d.length {
                                t = to_exponential(x, d.length.saturating_sub(7));
                            }
                            s = Some(t);
                        }
                    }
                    "bool" => {
                        s = Some(
                            match v {
                                Value::Bool(true) => "T",
                                Value::Bool(false) => "F",
                                _ => "?",
                            }
                            .into(),
                        )
                    }
                    "date" => {
                        let t = to_js_string(v);
                        let bt = t.as_bytes();
                        if bt.len() >= 10
                            && bt.get(0..4).is_some_and(|x| x.iter().all(u8::is_ascii_digit))
                            && bt.get(4) == Some(&b'-')
                            && bt.get(5..7).is_some_and(|x| x.iter().all(u8::is_ascii_digit))
                            && bt.get(7) == Some(&b'-')
                            && bt.get(8..10).is_some_and(|x| x.iter().all(u8::is_ascii_digit))
                        {
                            s = Some(format!("{}{}{}", t.get(0..4).unwrap_or(""), t.get(5..7).unwrap_or(""), t.get(8..10).unwrap_or("")));
                        }
                    }
                    _ => {
                        let t = to_js_string(v);
                        let u = cut(t.as_bytes(), d.length);
                        crate::gjs::put(&mut out, p, u);
                    }
                }
            }
            if let Some(s) = s {
                let pad = if d.kind == "number" { pad_start(&s, d.length, ' ') } else { s };
                for (k, c) in pad.encode_utf16().take(d.length).enumerate() {
                    set(&mut out, p.saturating_add(k), u8::try_from(c & 0xff).unwrap_or(0));
                }
            }
            p = p.saturating_add(d.length);
        }
    }
    let last = out.len().saturating_sub(1);
    set(&mut out, last, 0x1a);
    (out, names)
}

/// A fill value helper.
pub fn fv(n: usize) -> F {
    F::from(n)
}
