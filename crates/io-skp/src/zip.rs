//! The smallest ZIP reader cc-skp needs (port of `lib/skp/zip.js`, written from the PKWARE APPNOTE).
//!
//! Reads the central directory only (local headers are just used to find the data). Stored (0) and deflate (8);
//! the JS inflates with `DecompressionStream('deflate-raw')`, this port with `zlib-rs` (the fastest of the
//! pure-Rust inflaters we measured: 16 ms vs 21 ms for miniz_oxide on a 16 MB model.dat) into one buffer, with the
//! same outcomes: a broken, cut-off or over-long stream (bytes after its end) is `damaged`, more output than the
//! entry declares is `too_big`. Refused with `SkpError`: ZIP64 and encrypted entries (`unsupported`), anything that
//! points outside the bytes (`damaged`), entries bigger than `entry_bytes` or a running total over `inflated_bytes`
//! (`too_big`). Zip bombs: besides those caps the output buffer never grows past what deflate can produce from the
//! entry's compressed bytes (1032:1), so a lying size field cannot make us allocate. Entry names are only used as
//! lookup keys (nothing is written to disk), so a name with `..` cannot escape anything; vff.rs normalises the paths
//! a material points at and refuses ones that climb out of the archive.

use std::borrow::Cow;
use std::collections::HashMap;

use zlib_rs::{Inflate, InflateFlush, Status};

use crate::util::{Limits, Res, damaged, damaged_name, err, round_pos, utf8_or_latin1};

const SIG_LOCAL: u32 = 0x0403_4b50;
const SIG_CENTRAL: u32 = 0x0201_4b50;
const SIG_END: u32 = 0x0605_4b50;
const SIG_END64_LOCATOR: u32 = 0x0706_4b50;

/// Deflate's best case is about 1032:1 (258-byte matches at 2 bits each); 1033 leaves room.
const MAX_RATIO: usize = 1033;

#[inline]
fn rd16(b: &[u8], p: usize) -> u16 {
    match b.get(p..p.saturating_add(2)) {
        Some(&[x, y]) => u16::from_le_bytes([x, y]),
        _ => 0,
    }
}

#[inline]
fn rd32(b: &[u8], p: usize) -> u32 {
    match b.get(p..p.saturating_add(4)) {
        Some(&[a, c, d, e]) => u32::from_le_bytes([a, c, d, e]),
        _ => 0,
    }
}

/// CRC-32 (IEEE, as ZIP uses it).
pub fn crc32(bytes: &[u8]) -> u32 {
    crc32fast::hash(bytes)
}

#[derive(Debug, Clone)]
struct Entry {
    flags: u16,
    method: u16,
    crc: u32,
    csize: u32,
    usize: u32,
    local: usize,
}

/// An open archive (`openZip(bytes, start)`).
#[derive(Debug)]
pub struct Zip<'a> {
    bytes: &'a [u8],
    start: usize,
    /// Entry names in central-directory order (first of each name only).
    pub names: Vec<String>,
    entries: HashMap<String, Entry>,
    limits: Limits,
}

/// Open the ZIP archive that starts at `start` in `bytes` (which may carry other bytes before it, like the SketchUp
/// header: offsets in the directory are taken relative to wherever the directory turns out to be).
pub fn open_zip<'a>(bytes: &'a [u8], start: usize, limits: &Limits) -> Res<Zip<'a>> {
    let b = bytes;
    let len = b.len();

    // End of central directory: 22 bytes + up to 65535 bytes of comment, searched backwards.
    let mut eocd: Option<usize> = None;
    if let Some(hi) = len.checked_sub(22) {
        let lo = start.max(hi.saturating_sub(65535));
        let mut i = hi;
        while i >= lo {
            if rd32(b, i) == SIG_END && i.saturating_add(22).saturating_add(usize::from(rd16(b, i.saturating_add(20)))) <= len {
                eocd = Some(i);
                break;
            }
            match i.checked_sub(1) {
                Some(n) => i = n,
                None => break,
            }
        }
    }
    let eocd = eocd.ok_or_else(|| damaged("ccskp.damaged.no_end"))?;
    if eocd >= 20 && rd32(b, eocd.saturating_sub(20)) == SIG_END64_LOCATOR {
        return Err(err("unsupported", "ccskp.unsupported.zip64", &[]));
    }
    let disk = rd16(b, eocd.saturating_add(4));
    let count = rd16(b, eocd.saturating_add(10));
    let cd_size = rd32(b, eocd.saturating_add(12));
    let cd_offset = rd32(b, eocd.saturating_add(16));
    if count == 0xffff || cd_size == 0xffff_ffff || cd_offset == 0xffff_ffff {
        return Err(err("unsupported", "ccskp.unsupported.zip64", &[]));
    }
    if disk != 0 {
        return Err(err("unsupported", "ccskp.unsupported.multipart", &[]));
    }
    // Where the directory really is, versus where it says it is = the archive's offset inside the file.
    let cd_start = eocd.checked_sub(cd_size as usize).filter(|&s| s >= start).ok_or_else(|| damaged("ccskp.damaged.dir_outside"))?;
    let base = cd_start.checked_sub(cd_offset as usize).ok_or_else(|| damaged("ccskp.damaged.dir_outside"))?;

    let mut entries: HashMap<String, Entry> = HashMap::with_capacity(usize::from(count));
    let mut names = Vec::with_capacity(usize::from(count));
    let mut p = cd_start;
    for _ in 0..count {
        if p.saturating_add(46) > eocd || rd32(b, p) != SIG_CENTRAL {
            return Err(damaged("ccskp.damaged.dir_broken"));
        }
        let flags = rd16(b, p.saturating_add(8));
        let method = rd16(b, p.saturating_add(10));
        let crc = rd32(b, p.saturating_add(16));
        let csize = rd32(b, p.saturating_add(20));
        let usize_ = rd32(b, p.saturating_add(24));
        let name_len = usize::from(rd16(b, p.saturating_add(28)));
        let extra_len = usize::from(rd16(b, p.saturating_add(30)));
        let comment_len = usize::from(rd16(b, p.saturating_add(32)));
        let local = rd32(b, p.saturating_add(42));
        let rec_end = p.saturating_add(46).saturating_add(name_len).saturating_add(extra_len).saturating_add(comment_len);
        if rec_end > eocd {
            return Err(damaged("ccskp.damaged.dir_broken"));
        }
        let ns = p.saturating_add(46);
        let name = utf8_or_latin1(b.get(ns..ns.saturating_add(name_len)).unwrap_or(&[]));
        if csize == 0xffff_ffff || usize_ == 0xffff_ffff || local == 0xffff_ffff {
            return Err(err("unsupported", "ccskp.unsupported.zip64", &[]));
        }
        if !entries.contains_key(&name) {
            entries.insert(name.clone(), Entry { flags, method, crc, csize, usize: usize_, local: base.saturating_add(local as usize) });
            names.push(name);
        }
        p = rec_end;
    }
    Ok(Zip { bytes, start, names, entries, limits: *limits })
}

impl<'a> Zip<'a> {
    pub fn has(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    /// The refusals `read` makes before it looks at the budget (encrypted, bigger than one entry may be).
    pub fn early_error(&self, name: &str) -> Option<crate::util::SkpError> {
        let e = self.entries.get(name)?;
        if e.flags & 1 != 0 {
            return Some(err("unsupported", "ccskp.unsupported.encrypted", &[]));
        }
        if u64::from(e.usize) > self.limits.entry_bytes {
            let mb = round_pos(self.limits.entry_bytes as f64 / 1_048_576.0).to_string();
            return Some(err("too_big", "ccskp.too_big.part", &[("mb", &mb)]));
        }
        None
    }

    /// Where an entry's bytes are and how to unpack them, after every check `read` makes before unpacking
    /// (budget included). `budget` = bytes unpacked so far for this file.
    fn locate(&self, name: &str, budget: u64) -> Res<(&'a [u8], Entry)> {
        let e = self.entries.get(name).ok_or_else(|| damaged_name("ccskp.damaged.entry_missing", name))?.clone();
        if e.flags & 1 != 0 {
            return Err(err("unsupported", "ccskp.unsupported.encrypted", &[]));
        }
        if u64::from(e.usize) > self.limits.entry_bytes {
            let mb = round_pos(self.limits.entry_bytes as f64 / 1_048_576.0).to_string();
            return Err(err("too_big", "ccskp.too_big.part", &[("mb", &mb)]));
        }
        if budget.saturating_add(u64::from(e.usize)) > self.limits.inflated_bytes {
            return Err(err("too_big", "ccskp.too_big.unpacked", &[]));
        }
        let b = self.bytes;
        let lh = e.local;
        if lh < self.start || lh.saturating_add(30) > b.len() || rd32(b, lh) != SIG_LOCAL {
            return Err(damaged_name("ccskp.damaged.entry_outside", name));
        }
        let data_start = lh
            .saturating_add(30)
            .saturating_add(usize::from(rd16(b, lh.saturating_add(26))))
            .saturating_add(usize::from(rd16(b, lh.saturating_add(28))));
        let data_end = data_start.saturating_add(e.csize as usize);
        if data_end > b.len() {
            return Err(damaged_name("ccskp.damaged.entry_past_end", name));
        }
        let raw = b.get(data_start..data_end).ok_or_else(|| damaged_name("ccskp.damaged.entry_past_end", name))?;
        Ok((raw, e))
    }

    /// Unpack one entry (`await zip.read(name, budget)`); `budget` grows by the unpacked size on success.
    pub fn read(&self, name: &str, budget: &mut u64) -> Res<Cow<'a, [u8]>> {
        let out = self.read_unbudgeted(name, *budget)?;
        *budget = budget.saturating_add(out.len() as u64);
        Ok(out)
    }

    /// `read` without touching the budget (the caller adds the size; used to unpack entries in parallel after the
    /// budget has been checked in file order).
    pub fn read_unbudgeted(&self, name: &str, budget: u64) -> Res<Cow<'a, [u8]>> {
        let (raw, e) = self.locate(name, budget)?;
        let out: Cow<'a, [u8]> = match e.method {
            0 => {
                if e.csize != e.usize {
                    return Err(damaged_name("ccskp.damaged.entry_size", name));
                }
                Cow::Borrowed(raw)
            }
            8 => {
                let cap = u64::from(e.usize).min(self.limits.entry_bytes);
                Cow::Owned(inflate_raw(raw, cap)?)
            }
            m => return Err(err("unsupported", "ccskp.unsupported.method", &[("name", name), ("method", &m.to_string())])),
        };
        if out.len() != e.usize as usize {
            return Err(damaged_name("ccskp.damaged.entry_unpacked", name));
        }
        if crc32(&out) != e.crc {
            return Err(damaged_name("ccskp.damaged.entry_crc", name));
        }
        Ok(out)
    }

    /// The declared unpacked size of an entry (0 when missing), `zip.size(name)`.
    pub fn size(&self, name: &str) -> u64 {
        self.entries.get(name).map_or(0, |e| u64::from(e.usize))
    }
}

/// Inflate raw deflate data; `too_big` as soon as the output passes `cap` bytes, `damaged` for a broken or cut-off
/// stream or bytes after its end (what `DecompressionStream` reports as "Trailing junk").
pub fn inflate_raw(data: &[u8], cap: u64) -> Res<Vec<u8>> {
    let too_big = || {
        let mb = round_pos(cap as f64 / 1_048_576.0).to_string();
        err("too_big", "ccskp.too_big.part_unpacks", &[("mb", &mb)])
    };
    // One byte more than allowed tells "exactly cap" from "more than cap"; never more than deflate can make.
    let want = usize::try_from(cap).unwrap_or(usize::MAX).saturating_add(1);
    let bound = data.len().saturating_mul(MAX_RATIO).saturating_add(64);
    let size = want.min(bound);
    let mut out = vec![0u8; size];
    let mut z = Inflate::new(false, 15);
    let status = z.decompress(data, &mut out, InflateFlush::Finish);
    let written = usize::try_from(z.total_out()).unwrap_or(usize::MAX).min(out.len());
    let consumed = usize::try_from(z.total_in()).unwrap_or(usize::MAX);
    if written as u64 > cap {
        return Err(too_big());
    }
    match status {
        Ok(Status::StreamEnd) => {
            if consumed != data.len() {
                return Err(damaged("ccskp.damaged.inflate")); // bytes after the end of the stream
            }
            out.truncate(written);
            Ok(out)
        }
        // stopped with the buffer full: more output than deflate could make from these bytes
        Ok(_) if written == out.len() => Err(too_big()),
        // cut off, or broken
        _ => Err(damaged("ccskp.damaged.inflate")),
    }
}
