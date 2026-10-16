//! Find the named sections of a DWG file (port of lib/dwg/container-read.js; spec ch. 3, 4, 6–8, 26).

use std::collections::HashMap;

use crate::bits::{Res, err};
use crate::crc::{crc8, crc32, page_checksum, r13_header_xor, r2004_magic};
use crate::lz2004::decompress_r2004;
use crate::messages::msg;

pub const SENTINEL_HEADER: [u8; 16] = [0xCF, 0x7B, 0x1F, 0x23, 0xFD, 0xDE, 0x38, 0xA9, 0x5F, 0x7C, 0x68, 0xB8, 0x4E, 0x6D, 0x33, 0x5F];
pub const SENTINEL_CLASSES: [u8; 16] = [0x8D, 0xA1, 0xC4, 0xB8, 0xC4, 0xA9, 0xF8, 0xC5, 0xC0, 0xDC, 0xF4, 0x5F, 0xE7, 0xCF, 0xB6, 0x8A];

/// The sections of a file. `objects_absolute`: R13–R15, object map offsets are file positions and
/// "AcDb:AcDbObjects" is the whole file.
#[derive(Debug, Default)]
pub struct Container {
    pub version: String,
    pub ver: u32,
    pub maintenance: u32,
    pub codepage_num: i32,
    pub notes: Vec<String>,
    pub sections: Vec<(String, Vec<u8>)>,
    pub objects_absolute: bool,
}

impl Container {
    pub fn section(&self, name: &str) -> Option<&[u8]> {
        self.sections.iter().find(|(n, _)| n == name).map(|(_, d)| d.as_slice())
    }
}

/// A file this reader cannot open (UnsupportedDwg in the JS reader); the text is the reason.
pub fn version_num(v: &str) -> Option<u32> {
    Some(match v {
        "AC1012" => 12,
        "AC1014" => 14,
        "AC1015" => 15,
        "AC1018" => 18,
        "AC1021" => 21,
        "AC1024" => 24,
        "AC1027" => 27,
        "AC1032" => 32,
        _ => return None,
    })
}

/// Sizes from a damaged map can be absurd (container-util.js plausibleSize).
pub fn plausible_size(n: f64, file_len: usize) -> Res<f64> {
    let cap = (crate::util::usize_to_f64(file_len) * 64.0).max(f64::from(1u32 << 26));
    if n.is_nan() || n < 0.0 || n > cap {
        return err(format!("implausible size {} (damaged map)", js(n)));
    }
    Ok(n)
}
fn js(n: f64) -> String {
    let mut s = String::new();
    crate::value::js_num(n, &mut s);
    s
}

pub(crate) fn u16_at(b: &[u8], at: usize) -> u32 {
    u32::from(b.get(at).copied().unwrap_or(0)) | (u32::from(b.get(at.saturating_add(1)).copied().unwrap_or(0)) << 8)
}
pub(crate) fn u32_at(b: &[u8], at: usize) -> u32 {
    u16_at(b, at) | (u16_at(b, at.saturating_add(2)) << 16)
}
fn u64_at(b: &[u8], at: usize) -> f64 {
    f64::from(u32_at(b, at)) + f64::from(u32_at(b, at.saturating_add(4))) * 4294967296.0
}
fn ascii(b: &[u8], from: usize, n: usize) -> String {
    (0..n).map(|i| char::from(b.get(from.saturating_add(i)).copied().unwrap_or(0))).collect()
}

pub fn read_container(bytes: &[u8]) -> Res<Container> {
    if bytes.len() < 0x80 {
        return err(msg("ccdwg.open.too_short", &[("n", &bytes.len().to_string())]));
    }
    let version = ascii(bytes, 0, 6);
    let Some(ver) = version_num(&version) else {
        let looks = (version.starts_with("AC10") && version.chars().skip(4).all(|c| c.is_ascii_digit()))
            || (version.starts_with("AC1.") || version.starts_with("AC2."))
            || version.starts_with("MC0.0");
        if looks {
            return err(msg("ccdwg.open.version", &[("version", &version)]));
        }
        let start: String = version.chars().map(|c| if (' '..='~').contains(&c) { c } else { '?' }).collect();
        return err(msg("ccdwg.open.not_dwg", &[("start", &start)]));
    };
    if ver <= 15 {
        return read_r13(bytes, &version, ver);
    }
    if ver == 21 {
        return crate::r2007::read_r2007_container(bytes, &version, ver);
    }
    read_r2004(bytes, &version, ver)
}

fn read_r13(b: &[u8], version: &str, ver: u32) -> Res<Container> {
    let mut notes = Vec::new();
    let maintenance = u32::from(b.get(0x0B).copied().unwrap_or(0));
    let codepage_num = u16_at(b, 0x13);
    let n = u32_at(b, 0x15);
    if n > 16 {
        return err(msg("ccdwg.open.r13_header", &[("n", &n.to_string())]));
    }
    let mut loc: HashMap<u8, (usize, usize)> = HashMap::new();
    let n = crate::util::usize_from_u32(n);
    // n <= 16, so these offsets are small constants
    for i in 0..n {
        let at = i.saturating_mul(9).saturating_add(0x19);
        let id = b.get(at).copied().unwrap_or(0);
        loc.insert(
            id,
            (crate::util::usize_from_u32(u32_at(b, at.saturating_add(1))), crate::util::usize_from_u32(u32_at(b, at.saturating_add(5)))),
        );
    }
    let crc_at = n.saturating_mul(9).saturating_add(0x19);
    let want = u16_at(b, crc_at);
    let got = u32::from(crc8(0, b, 0, crc_at) ^ r13_header_xor(u32::try_from(n).unwrap_or(0)));
    if want != got {
        notes.push(msg("ccdwg.read.file_header_crc", &[]));
    }
    let sec = |k: u8| -> Option<Vec<u8>> {
        let &(at, size) = loc.get(&k)?;
        let end = at.checked_add(size)?;
        if end <= b.len() { b.get(at..end).map(<[u8]>::to_vec) } else { None }
    };
    let mut sections = Vec::new();
    let header = sec(0);
    let handles = sec(2);
    if header.is_none() || handles.is_none() {
        return err(msg("ccdwg.open.r13_locators", &[]));
    }
    if let Some(h) = header {
        sections.push(("AcDb:Header".to_string(), h));
    }
    if let Some(c) = sec(1) {
        sections.push(("AcDb:Classes".to_string(), c));
    }
    if let Some(h) = handles {
        sections.push(("AcDb:Handles".to_string(), h));
    }
    sections.push(("AcDb:AcDbObjects".to_string(), b.to_vec()));
    for (k, name) in [(3u8, "AcDb:ObjFreeSpace"), (4, "AcDb:Template"), (5, "AcDb:AuxHeader")] {
        if loc.contains_key(&k)
            && let Some(d) = sec(k)
        {
            sections.push((name.to_string(), d));
        }
    }
    Ok(Container {
        version: version.to_string(),
        ver,
        maintenance,
        codepage_num: i32::try_from(codepage_num).unwrap_or(0),
        notes,
        sections,
        objects_absolute: true,
    })
}

struct PageLoc {
    addr: usize,
}

fn read_r2004(b: &[u8], version: &str, ver: u32) -> Res<Container> {
    let mut notes = Vec::new();
    let maintenance = u32::from(b.get(0x0B).copied().unwrap_or(0));
    let codepage_num = u16_at(b, 0x13);
    if u32_at(b, 0x18) & 1 != 0 {
        notes.push(msg("ccdwg.read.password", &[]));
    }
    let magic = r2004_magic(0x6C);
    let h: Vec<u8> =
        (0..0x6Cusize).map(|i| b.get(i.saturating_add(0x80)).copied().unwrap_or(0) ^ magic.get(i).copied().unwrap_or(0)).collect();
    if ascii(&h, 0, 11) != "AcFssFcAJMB" {
        return err(msg("ccdwg.open.no_id", &[("version", version)]));
    }
    {
        let want = u32_at(&h, 0x68);
        let mut tmp = h.clone();
        for x in tmp.iter_mut().skip(0x68) {
            *x = 0;
        }
        if crc32(&tmp, 0) != want {
            notes.push(msg("ccdwg.read.file_header_crc32", &[]));
        }
    }
    let page_map_addr = crate::util::to_usize(u64_at(&h, 0x54) + 256.0);
    let section_map_id = u32_at(&h, 0x5C);
    let pm = read_system_page(b, page_map_addr, 0x4163_0E3B, &mut notes, &msg("ccdwg.part.page_map", &[]))?;
    let mut pages: HashMap<i64, PageLoc> = HashMap::new();
    let mut page_count = 0usize;
    let mut addr: usize = 0x100;
    let mut i = 0usize;
    while i.saturating_add(8) <= pm.len() {
        let num = i64::from(i32::from_le_bytes(u32_at(&pm, i).to_le_bytes()));
        let size = crate::util::usize_from_u32(u32_at(&pm, i.saturating_add(4)));
        i = i.saturating_add(8);
        if num < 0 {
            i = i.saturating_add(16);
        } else {
            if pages.insert(num, PageLoc { addr }).is_none() {
                page_count = page_count.saturating_add(1);
            }
        }
        addr = addr.saturating_add(size);
    }
    let Some(sm) = pages.get(&i64::from(section_map_id)) else {
        return err(msg("ccdwg.open.no_section_map", &[("version", version), ("id", &section_map_id.to_string())]));
    };
    let sm_data = read_system_page(b, sm.addr, 0x4163_003B, &mut notes, &msg("ccdwg.part.section_map", &[]))?;
    let count = u32_at(&sm_data, 0);
    let mut sections = Vec::new();
    let mut p = 0x14usize;
    let mut s = 0u32;
    // p + 0x60 <= sm_data.len() inside the loop, so the offsets below cannot overflow
    while s < count && p.saturating_add(0x60) <= sm_data.len() {
        s = s.saturating_add(1);
        let size = u64_at(&sm_data, p);
        let page_count_s = crate::util::usize_from_u32(u32_at(&sm_data, p.saturating_add(8)));
        let max_size = f64::from(u32_at(&sm_data, p.saturating_add(0x0C)));
        let compressed = u32_at(&sm_data, p.saturating_add(0x14));
        let encrypted = u32_at(&sm_data, p.saturating_add(0x1C));
        let mut name = String::new();
        for k in 0..64usize {
            let c = sm_data.get(p.saturating_add(0x20).saturating_add(k)).copied().unwrap_or(0);
            if c == 0 {
                break;
            }
            name.push(char::from(c));
        }
        p = p.saturating_add(0x60);
        if page_count_s > page_count.saturating_add(1) || p.saturating_add(page_count_s.saturating_mul(16)) > sm_data.len() {
            notes.push(msg("ccdwg.read.section_map_damaged", &[("n", &page_count_s.to_string())]));
            break;
        }
        let mut list = Vec::with_capacity(page_count_s);
        for _ in 0..page_count_s {
            list.push(i64::from(u32_at(&sm_data, p)));
            p = p.saturating_add(16);
        }
        if name.is_empty() {
            continue;
        }
        if encrypted == 1 {
            notes.push(msg("ccdwg.read.section_encrypted", &[("name", &name)]));
            continue;
        }
        match read_data_section(b, &pages, &list, size, max_size, compressed == 2, &name, &mut notes) {
            Ok(d) => sections.push((name, d)),
            Err(e) => notes.push(msg("ccdwg.read.section_failed", &[("name", &name), ("reason", &e.0)])),
        }
    }
    Ok(Container {
        version: version.to_string(),
        ver,
        maintenance,
        codepage_num: i32::try_from(codepage_num).unwrap_or(0),
        notes,
        sections,
        objects_absolute: false,
    })
}

/// 4.3 system section page.
fn read_system_page(b: &[u8], addr: usize, typ: u32, notes: &mut Vec<String>, what: &str) -> Res<Vec<u8>> {
    if addr.saturating_add(0x14) > b.len() {
        return err(msg("ccdwg.open.past_end", &[("what", what)]));
    }
    let t = u32_at(b, addr);
    if t != typ {
        return err(msg("ccdwg.open.page_type", &[("what", what), ("type", &format!("{t:x}"))]));
    }
    // addr + 0x14 + csize <= b.len() is checked before any use of the sums below
    let usize_ = crate::util::to_usize(plausible_size(f64::from(u32_at(b, addr.saturating_add(4))), b.len())?);
    let csize = crate::util::usize_from_u32(u32_at(b, addr.saturating_add(8)));
    if addr.saturating_add(0x14).saturating_add(csize) > b.len() {
        return err(msg("ccdwg.open.runs_past", &[("what", what)]));
    }
    let ctype = u32_at(b, addr.saturating_add(12));
    let sum = u32_at(b, addr.saturating_add(16));
    let from = addr.saturating_add(0x14);
    let to = from.saturating_add(csize);
    let mut hdr = b.get(addr..from).map(<[u8]>::to_vec).unwrap_or_default();
    for x in hdr.iter_mut().skip(16) {
        *x = 0;
    }
    let check = page_checksum(page_checksum(0, &hdr, 0, hdr.len()), b, from, to);
    if check != sum {
        notes.push(msg("ccdwg.read.system_checksum", &[("what", what)]));
    }
    if ctype == 2 {
        return decompress_r2004(b, from, to, usize_);
    }
    let mut out = b.get(from..from.saturating_add(usize_).min(b.len())).map(<[u8]>::to_vec).unwrap_or_default();
    out.resize(out.len().min(usize_), 0);
    Ok(out)
}

/// 4.5/4.6 a data section.
#[allow(clippy::too_many_arguments)]
fn read_data_section(
    b: &[u8],
    pages: &HashMap<i64, PageLoc>,
    list: &[i64],
    size: f64,
    max_size: f64,
    compressed: bool,
    name: &str,
    notes: &mut Vec<String>,
) -> Res<Vec<u8>> {
    plausible_size(size, b.len())?;
    plausible_size(max_size, b.len())?;
    let size_u = crate::util::to_usize(size);
    let mut out = vec![0u8; size_u];
    let mut bad = 0usize;
    for num in list {
        let Some(loc) = pages.get(num) else {
            bad = bad.saturating_add(1);
            continue;
        };
        let at = loc.addr;
        if at.saturating_add(32) > b.len() {
            bad = bad.saturating_add(1);
            continue;
        }
        let mask = 0x4164_536Bu32 ^ u32::try_from(at).unwrap_or(0);
        let hh = |k: usize| u32_at(b, at.saturating_add(k)) ^ mask; // at + 32 <= b.len()
        let ptype = hh(0);
        let data_size = crate::util::usize_from_u32(hh(8));
        let start = f64::from(hh(16)) + f64::from(hh(20)) * 4294967296.0;
        let data_sum = hh(0x1C);
        if ptype != 0x4163_043B {
            bad = bad.saturating_add(1);
            continue;
        }
        let from = at.saturating_add(32);
        let to = from.saturating_add(data_size);
        if to > b.len() {
            bad = bad.saturating_add(1);
            continue;
        }
        if page_checksum(0, b, from, to) != data_sum {
            notes.push(msg("ccdwg.read.page_checksum", &[("name", name)]));
        }
        let logical = max_size.min(size - start);
        if logical <= 0.0 {
            continue;
        }
        let logical = crate::util::to_usize(logical);
        let start = crate::util::to_usize(start);
        let page;
        let src: &[u8] = if compressed {
            page = decompress_r2004(b, from, to, crate::util::to_usize(max_size))?;
            &page
        } else {
            b.get(from..).unwrap_or_default()
        };
        for i in 0..logical {
            match out.get_mut(start.saturating_add(i)) {
                Some(x) => *x = src.get(i).copied().unwrap_or(0),
                None => break,
            }
        }
    }
    if bad > 0 {
        notes.push(msg("ccdwg.read.pages_bad", &[("n", &bad.to_string()), ("name", name)]));
    }
    Ok(out)
}
