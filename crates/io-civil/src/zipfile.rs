//! zipfile.js — a small ZIP reader and writer. Untrusted input: every offset and size is checked against the file
//! before use, an entry claiming more than `max` bytes (200 MB) is refused, the inflated output stops at its stated
//! size (a zip bomb stops there), encrypted entries, ZIP64 and other methods are refused. Entry names come back with
//! `\` turned into `/`; callers never use them as paths.
//!
//! Spec gap: the JS deflates through the browser's / Node's `CompressionStream('deflate-raw')` (Chromium's zlib, whose
//! matcher differs from every other zlib); this port deflates with zlib-rs at the same level, so a deflated entry's
//! bytes differ while its contents, CRC and size are the same (the harness compares what reading the archive gives).

use crate::gjs::{put, u16le, u32le};
use cc_civil::js::J;
use cc_civil::tr;
use serde_json::Value;

const MAX_ENTRIES: usize = 20000;
pub const MAX_ENTRY: u64 = 200 * 1024 * 1024;

/// The JS ZipError message.
#[derive(Clone, Debug)]
pub struct ZipError(pub String);

fn table() -> &'static [u32; 256] {
    static T: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u32; 256];
        for (n, slot) in t.iter_mut().enumerate() {
            let mut c = u32::try_from(n).unwrap_or(0);
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        t
    })
}
/// `crc32(bytes)`.
pub fn crc32(b: &[u8]) -> u32 {
    let t = table();
    let mut crc = !0u32;
    for &x in b {
        crc = t.get(usize::from(u8::try_from(crc & 0xff).unwrap_or(0) ^ x)).copied().unwrap_or(0) ^ (crc >> 8);
    }
    !crc
}

/// A central directory entry.
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub size: u32,
    pub csize: u32,
    pub method: u16,
    pub offset: u32,
    pub crc: u32,
    pub flags: u16,
    pub dir: bool,
}
impl J for Entry {
    fn j(&self) -> Value {
        cc_civil::jo! {
            "name" => self.name, "size" => self.size, "csize" => self.csize, "method" => u32::from(self.method),
            "offset" => self.offset, "crc" => self.crc, "flags" => u32::from(self.flags), "dir" => self.dir,
        }
    }
}

/// `listZip(bytes)`.
pub fn list_zip(b: &[u8]) -> Result<Vec<Entry>, ZipError> {
    if b.len() < 22 {
        return Err(ZipError(tr!("zip.short")));
    }
    let mut eocd: Option<usize> = None;
    let top = b.len().saturating_sub(22);
    let low = b.len().saturating_sub(22).saturating_sub(65535);
    let mut i = top;
    loop {
        if u32le(b, i) == 0x0605_4b50 {
            eocd = Some(i);
            break;
        }
        if i == low {
            break;
        }
        i = i.saturating_sub(1);
    }
    let Some(eocd) = eocd else { return Err(ZipError(tr!("zip.noend"))) };
    let count = usize::from(u16le(b, eocd.saturating_add(10)));
    let cd_size = u32le(b, eocd.saturating_add(12));
    let cd_off = u32le(b, eocd.saturating_add(16));
    if count == 0xffff || cd_off == 0xffff_ffff {
        return Err(ZipError(tr!("zip.zip64")));
    }
    if count > MAX_ENTRIES {
        return Err(ZipError(tr!("zip.many", n = count, max = MAX_ENTRIES)));
    }
    let (cd_off, cd_size) = (cd_off as usize, cd_size as usize);
    if cd_off.saturating_add(cd_size) > eocd || cd_off > b.len() {
        return Err(ZipError(tr!("zip.outside")));
    }
    let mut out = Vec::new();
    let mut p = cd_off;
    for _ in 0..count {
        if p.saturating_add(46) > eocd || u32le(b, p) != 0x0201_4b50 {
            return Err(ZipError(tr!("zip.list")));
        }
        let flags = u16le(b, p.saturating_add(8));
        let method = u16le(b, p.saturating_add(10));
        let crc = u32le(b, p.saturating_add(16));
        let csize = u32le(b, p.saturating_add(20));
        let size = u32le(b, p.saturating_add(24));
        let nl = usize::from(u16le(b, p.saturating_add(28)));
        let xl = usize::from(u16le(b, p.saturating_add(30)));
        let cl = usize::from(u16le(b, p.saturating_add(32)));
        let offset = u32le(b, p.saturating_add(42));
        let next = p.saturating_add(46).saturating_add(nl).saturating_add(xl).saturating_add(cl);
        if next > eocd {
            return Err(ZipError(tr!("zip.list")));
        }
        let raw = b.get(p.saturating_add(46)..p.saturating_add(46).saturating_add(nl)).unwrap_or(&[]);
        let enc = if flags & 0x800 != 0 { encoding_rs::UTF_8 } else { encoding_rs::WINDOWS_1252 };
        let name = enc.decode_without_bom_handling(raw).0.replace('\\', "/");
        let dir = name.ends_with('/');
        out.push(Entry { name, size, csize, method, offset, crc, flags, dir });
        p = next;
    }
    Ok(out)
}

/// `unzipEntry(bytes, entry, { max })`.
pub fn unzip_entry(b: &[u8], e: &Entry, max: u64) -> Result<Vec<u8>, ZipError> {
    let file = e.name.as_str();
    if e.flags & 1 != 0 {
        return Err(ZipError(tr!("zip.encrypted", file = file)));
    }
    if u64::from(e.size) > max || e.csize as usize > b.len() {
        let mb = cc_civil::js::round(f64::from(e.size) / 1_048_576.0);
        let mx = cc_civil::js::round(max as f64 / 1_048_576.0);
        return Err(ZipError(tr!("zip.big", file = file, mb = mb, max = mx)));
    }
    let off = e.offset as usize;
    if off.saturating_add(30) > b.len() || u32le(b, off) != 0x0403_4b50 {
        return Err(ZipError(tr!("zip.entry", file = file)));
    }
    let start = off
        .saturating_add(30)
        .saturating_add(usize::from(u16le(b, off.saturating_add(26))))
        .saturating_add(usize::from(u16le(b, off.saturating_add(28))));
    let csize = e.csize as usize;
    if start.saturating_add(csize) > b.len() {
        return Err(ZipError(tr!("zip.cut", file = file)));
    }
    let data = b.get(start..start.saturating_add(csize)).unwrap_or(&[]);
    let out = match e.method {
        0 => {
            if e.csize != e.size {
                return Err(ZipError(tr!("zip.entry", file = file)));
            }
            data.to_vec()
        }
        8 => {
            let size = e.size as usize;
            let mut buf = vec![0u8; size.saturating_add(1)];
            let (got, rc) = zlib_rs::decompress_slice(&mut buf, data, zlib_rs::InflateConfig { window_bits: -15 });
            let n = got.len();
            if n > size {
                return Err(ZipError(tr!("zip.bomb")));
            }
            if rc != zlib_rs::ReturnCode::Ok {
                return Err(ZipError(tr!("zip.inflate")));
            }
            buf.truncate(n);
            if n != size {
                return Err(ZipError(tr!("zip.size", file = file)));
            }
            buf
        }
        m => return Err(ZipError(tr!("zip.method", file = file, method = i64::from(m)))),
    };
    if crc32(&out) != e.crc {
        return Err(ZipError(tr!("zip.crc", file = file)));
    }
    Ok(out)
}

/// A local date and time for the entries' DOS stamps.
#[derive(Clone, Copy, Debug)]
pub struct DosDate {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

fn deflate_raw(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = vec![0u8; zlib_rs::compress_bound(data.len()).saturating_add(64)];
    let cfg = zlib_rs::DeflateConfig {
        level: 6,
        method: zlib_rs::Method::Deflated,
        window_bits: -15,
        mem_level: 8,
        strategy: zlib_rs::Strategy::Default,
    };
    let (got, rc) = zlib_rs::compress_slice(&mut out, data, cfg);
    if rc != zlib_rs::ReturnCode::Ok {
        return None;
    }
    let n = got.len();
    out.truncate(n);
    Some(out)
}

/// `zip([{ name, data }], { deflate, date })`.
pub fn zip(files: &[(String, Vec<u8>)], deflate: bool, date: DosDate) -> Vec<u8> {
    let dos_time = ((date.hour << 11) | (date.minute << 5) | (date.second >> 1)) & 0xffff;
    // ((year − 1980) << 9) in 32-bit two's complement, as the JS: a year before 1980 wraps (1979 → 0xfe00 | …), not 0
    let dos_date = ((date.year.saturating_sub(1980).cast_unsigned().wrapping_shl(9)) | (date.month << 5) | date.day) & 0xffff;
    let (dos_time, dos_date) = (u16::try_from(dos_time).unwrap_or(0), u16::try_from(dos_date).unwrap_or(0));
    let mut locals: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    let mut off: u32 = 0;
    for (name, data) in files {
        let nb = name.as_bytes();
        let crc = crc32(data);
        let mut body: &[u8] = data;
        let mut method = 0u16;
        let packed;
        if deflate
            && data.len() > 64
            && let Some(c) = deflate_raw(data)
            && c.len() < data.len()
        {
            packed = c;
            body = &packed;
            method = 8;
        }
        let nl = u16::try_from(nb.len()).unwrap_or(u16::MAX);
        let blen = u32::try_from(body.len()).unwrap_or(u32::MAX);
        let dlen = u32::try_from(data.len()).unwrap_or(u32::MAX);
        let mut h = [0u8; 30];
        put(&mut h, 0, &0x0403_4b50u32.to_le_bytes());
        put(&mut h, 4, &20u16.to_le_bytes());
        put(&mut h, 6, &0x800u16.to_le_bytes());
        put(&mut h, 8, &method.to_le_bytes());
        put(&mut h, 10, &dos_time.to_le_bytes());
        put(&mut h, 12, &dos_date.to_le_bytes());
        put(&mut h, 14, &crc.to_le_bytes());
        put(&mut h, 18, &blen.to_le_bytes());
        put(&mut h, 22, &dlen.to_le_bytes());
        put(&mut h, 26, &nl.to_le_bytes());
        let mut c = [0u8; 46];
        put(&mut c, 0, &0x0201_4b50u32.to_le_bytes());
        put(&mut c, 4, &20u16.to_le_bytes());
        put(&mut c, 6, &20u16.to_le_bytes());
        put(&mut c, 8, &0x800u16.to_le_bytes());
        put(&mut c, 10, &method.to_le_bytes());
        put(&mut c, 12, &dos_time.to_le_bytes());
        put(&mut c, 14, &dos_date.to_le_bytes());
        put(&mut c, 16, &crc.to_le_bytes());
        put(&mut c, 20, &blen.to_le_bytes());
        put(&mut c, 24, &dlen.to_le_bytes());
        put(&mut c, 28, &nl.to_le_bytes());
        put(&mut c, 42, &off.to_le_bytes());
        locals.extend_from_slice(&h);
        locals.extend_from_slice(nb);
        locals.extend_from_slice(body);
        central.extend_from_slice(&c);
        central.extend_from_slice(nb);
        off = off.wrapping_add(30).wrapping_add(u32::from(nl)).wrapping_add(blen);
    }
    let mut e = [0u8; 22];
    let n = u16::try_from(files.len()).unwrap_or(u16::MAX);
    put(&mut e, 0, &0x0605_4b50u32.to_le_bytes());
    put(&mut e, 8, &n.to_le_bytes());
    put(&mut e, 10, &n.to_le_bytes());
    put(&mut e, 12, &(u32::try_from(central.len()).unwrap_or(u32::MAX)).to_le_bytes());
    put(&mut e, 16, &off.to_le_bytes());
    let mut out = locals;
    out.extend_from_slice(&central);
    out.extend_from_slice(&e);
    out
}
