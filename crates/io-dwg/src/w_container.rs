//! Lays the named sections out as a DWG file (port of lib/dwg/container-write.js): the R13–R15 layout (AC1015, ODA
//! spec ch. 3, 26) and the R2004 family (AC1018, AC1024, AC1027, AC1032: encrypted file header, data pages with
//! masked headers, LZ77 compression, section map and section page map, ch. 4, 6, 7, 8). R2007 is in w_r2007.rs.
//! The "Spec gap:" notes of the JS apply; the ones that decide bytes are repeated next to their code.

use crate::crc::{crc8 as crc16, crc32, page_checksum, r2004_magic};
use crate::util::{u32_of_usize, usize_to_f64};
use crate::w_bits::{BitWriter, ByteWriter};
use crate::w_lz2004::compress_r2004;

const FILEHDR_END: [u8; 16] = [0x95, 0xA0, 0x4E, 0x28, 0x99, 0x82, 0x1A, 0xE5, 0x5E, 0x41, 0xE0, 0x5F, 0x9D, 0x3A, 0x4D, 0x00];
const HEADER: [u8; 16] = [0xCF, 0x7B, 0x1F, 0x23, 0xFD, 0xDE, 0x38, 0xA9, 0x5F, 0x7C, 0x68, 0xB8, 0x4E, 0x6D, 0x33, 0x5F];
const HEADER_END: [u8; 16] = [0x30, 0x84, 0xE0, 0xDC, 0x02, 0x21, 0xC7, 0x56, 0xA0, 0x83, 0x97, 0x47, 0xB1, 0x92, 0xCC, 0xA0];
const CLASSES: [u8; 16] = [0x8D, 0xA1, 0xC4, 0xB8, 0xC4, 0xA9, 0xF8, 0xC5, 0xC0, 0xDC, 0xF4, 0x5F, 0xE7, 0xCF, 0xB6, 0x8A];
const CLASSES_END: [u8; 16] = [0x72, 0x5E, 0x3B, 0x47, 0x3B, 0x56, 0x07, 0x3A, 0x3F, 0x23, 0x0B, 0xA0, 0x18, 0x30, 0x49, 0x75];
const PREVIEW: [u8; 16] = [0x1F, 0x25, 0x6D, 0x07, 0xD4, 0x36, 0x28, 0x28, 0x9D, 0x57, 0xCA, 0x3F, 0x9D, 0x44, 0x10, 0x2B];
const PREVIEW_END: [u8; 16] = [0xE0, 0xDA, 0x92, 0xF8, 0x2B, 0xC9, 0xD7, 0xD7, 0x62, 0xA8, 0x35, 0xC0, 0x62, 0xBB, 0xEF, 0xD4];
const SECOND: [u8; 16] = [0xD4, 0x7B, 0x21, 0xCE, 0x28, 0x93, 0x9F, 0xBF, 0x53, 0x24, 0x40, 0x09, 0x12, 0x3C, 0xAA, 0x01];
const SECOND_END: [u8; 16] = [0x2B, 0x84, 0xDE, 0x31, 0xD7, 0x6C, 0x60, 0x40, 0xAC, 0xDB, 0xBF, 0xF6, 0xED, 0xC3, 0x55, 0xFE];

/// Aux-header / file-header version numbers (ch. 27) and the maintenance release written.
pub fn version_info(version: &str) -> (u32, u32) {
    match version {
        "AC1015" => (23, 6),
        "AC1018" => (25, 0),
        "AC1021" => (27, 0),
        "AC1024" => (29, 6),
        "AC1027" => (31, 8),
        "AC1032" => (33, 0),
        _ => (0, 0),
    }
}
/// The extra RL after the header/classes size (ch. 9, 10.2).
pub fn has_hi_size(version: &str) -> bool {
    let m = version_info(version).1;
    version == "AC1032" || ((version == "AC1024" || version == "AC1027") && m > 3)
}

/// A Julian date (day, ms).
#[derive(Clone, Copy, Debug, Default)]
pub struct Jd {
    pub day: f64,
    pub ms: f64,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Jds {
    pub created: Jd,
    pub updated: Jd,
}

/// What write.js hands the layouts.
#[derive(Clone, Debug, Default)]
pub struct Parts {
    pub header: Vec<u8>,
    pub classes: Vec<u8>,
    pub objects: Vec<(f64, Vec<u8>)>,
    pub measurement: bool,
    pub jd: Jds,
    pub handseed: f64,
    pub keys: Vec<f64>,
    pub acds: Option<Vec<u8>>,
}

/// Wrap header/class data: sentinel, RL size, [RL 0], data, CRC-16 (seed 0xC0C1 over size + data), end sentinel.
pub fn wrap_section(begin: &[u8], end: &[u8], data: &[u8], version: &str) -> Vec<u8> {
    let mut o = ByteWriter::new(data.len().saturating_add(64));
    o.raw(begin);
    let from = o.pos();
    o.rl_u(u32_of_usize(data.len()));
    if has_hi_size(version) {
        o.rl_u(0);
    }
    o.raw(data);
    let c = crc16(0xC0C1, &o.buf, from, o.buf.len());
    o.rs_i(i32::from(c));
    o.raw(end);
    o.buf
}
pub fn header_section(data: &[u8], version: &str) -> Vec<u8> {
    wrap_section(&HEADER, &HEADER_END, data, version)
}
pub fn classes_section(data: &[u8], version: &str) -> Vec<u8> {
    wrap_section(&CLASSES, &CLASSES_END, data, version)
}
/// An empty preview (14.2).
pub fn preview_section() -> Vec<u8> {
    let mut o = ByteWriter::new(64);
    o.raw(&PREVIEW);
    o.rl_u(1);
    o.rc8(0);
    o.raw(&PREVIEW_END);
    o.buf
}

/// Object map (ch. 23): pages of at most 2032 bytes, big-endian size and CRC; each page restarts at 0/0.
/// Spec gap (our R2000 writer): every page starts again from handle 0 and offset 0, as in AutoCAD's files.
pub fn object_map(map: &[(f64, f64)]) -> Vec<u8> {
    let mut out = ByteWriter::new(map.len().saturating_mul(6).saturating_add(64));
    let mut i = 0usize;
    loop {
        let mut sec = ByteWriter::new(2048);
        let start = i;
        let (mut last_h, mut last_loc) = (0.0f64, 0.0f64);
        while i < map.len() && sec.pos() < 2020 {
            let (h, loc) = map.get(i).copied().unwrap_or((0.0, 0.0));
            sec.mc(h - last_h);
            sec.mc(loc - last_loc);
            last_h = h;
            last_loc = loc;
            i = i.saturating_add(1);
        }
        let body = sec.buf;
        let size = body.len().saturating_add(2);
        let mut chunk = Vec::with_capacity(size);
        chunk.push(crate::util::u8_of(u32_of_usize(size >> 8)));
        chunk.push(crate::util::u8_of(u32_of_usize(size & 0xFF)));
        chunk.extend_from_slice(&body);
        let c = crc16(0xC0C1, &chunk, 0, chunk.len());
        out.raw(&chunk);
        out.rc8(crate::util::u8_of(u32::from(c >> 8)));
        out.rc8(crate::util::u8_of(u32::from(c & 0xFF)));
        if start == i {
            break;
        }
    }
    out.buf
}

/// Free-space table (21.1).
pub fn obj_free_space(count: usize, jd_update: Jd, objects_at: f64) -> Vec<u8> {
    let mut o = ByteWriter::new(64);
    o.rl_u(0);
    o.rl(usize_to_f64(count));
    o.rl(jd_update.day);
    o.rl(jd_update.ms);
    o.rl(objects_at);
    o.rc8(4);
    for v in [0x32u32, 0x64, 0x200, 0xFFFF_FFFF] {
        o.rl_u(v);
        o.rl_u(0);
    }
    o.buf
}

/// Template (22). Spec gap: from R2007 the empty description is one NUL character.
pub fn template_section(measurement: bool, version: &str) -> Vec<u8> {
    let mut o = ByteWriter::new(8);
    if version == "AC1015" || version == "AC1018" {
        o.rs_i(0);
    } else {
        o.rs_i(1);
        o.rs_i(0);
    }
    o.rs_i(i32::from(measurement));
    o.buf
}

/// Auxiliary header (27).
pub fn aux_header(version: &str, jd: &Jds, handseed: f64) -> Vec<u8> {
    let (code, maint) = version_info(version);
    let (code, maint) = (f64::from(code), f64::from(maint));
    let mut o = ByteWriter::new(160);
    o.raw(&[0xFF, 0x77, 0x01]);
    o.rs(code);
    o.rs(maint);
    o.rl_u(1);
    o.rl(-1.0);
    o.rs_i(1);
    o.rs_i(0);
    o.rl_u(0);
    o.rs(code);
    o.rs(maint);
    o.rs(code);
    o.rs(maint);
    for v in [0x0005, 0x0893, 0x0005, 0x0893, 0, 1] {
        o.rs_i(v);
    }
    for _ in 0..5 {
        o.rl_u(0);
    }
    o.rl(jd.created.day);
    o.rl(jd.created.ms);
    o.rl(jd.updated.day);
    o.rl(jd.updated.ms);
    o.rl(if handseed < 2_147_483_647.0 { handseed } else { -1.0 });
    o.rl_u(0);
    o.rs_i(0);
    o.rs_i(1);
    for v in [0u32, 0, 0, 1, 0, 0, 0, 0] {
        o.rl_u(v);
    }
    if version == "AC1032" {
        o.rs_i(0);
        o.rs_i(0);
        o.rs_i(0);
    }
    o.buf
}

/// Summary info (13). Spec gap: the length counts the terminating NUL.
pub fn summary_info(version: &str, jd: &Jds) -> Vec<u8> {
    let wide = version != "AC1018";
    let mut o = ByteWriter::new(256);
    for s in ["", "", "", "", "", "cc-dwg", "", ""] {
        let u: Vec<u16> = s.encode_utf16().collect();
        o.rs(usize_to_f64(u.len()) + 1.0);
        if wide {
            for &c in &u {
                o.rs_i(i32::from(c));
            }
            o.rs_i(0);
        } else {
            for &c in &u {
                o.rc8(crate::util::u8_of(u32::from(c)));
            }
            o.rc8(0);
        }
    }
    o.rl_u(0);
    o.rl_u(0);
    o.rl(jd.created.day);
    o.rl(jd.created.ms);
    o.rl(jd.updated.day);
    o.rl(jd.updated.ms);
    o.rs_i(0);
    o.rl_u(0);
    o.rl_u(0);
    o.buf
}

/// RevHistory (18).
pub fn rev_history(version: &str) -> Vec<u8> {
    let mut o = ByteWriter::new(16);
    if version == "AC1018" {
        for _ in 0..3 {
            o.rl_u(0);
        }
    } else {
        for v in [0u32, 0, 1, 0] {
            o.rl_u(v);
        }
    }
    o.buf
}

// ---------------------------------------------------------------- R13–R15 (AC1015)
pub fn layout_r2000(parts: &Parts) -> Result<Vec<u8>, String> {
    let version = "AC1015";
    let (code, maint) = version_info(version);
    let codepage = 30;
    let mut out = ByteWriter::new(1 << 16);
    let hdr_len = 0x19 + 6 * 9 + 2 + 16;
    out.zeros(hdr_len);
    let aux = aux_header(version, &parts.jd, parts.handseed);
    let ax_at = out.pos();
    out.raw(&aux);
    let pv_at = out.pos();
    out.raw(&preview_section());
    let hv_at = out.pos();
    let hv = header_section(&parts.header, version);
    out.raw(&hv);
    let cl_at = out.pos();
    let cl = classes_section(&parts.classes, version);
    out.raw(&cl);
    out.zeros(0x200); // padding (ch. 11)
    let obj_start = out.pos();
    let mut map: Vec<(f64, f64)> = Vec::with_capacity(parts.objects.len());
    for (h, data) in &parts.objects {
        map.push((*h, usize_to_f64(out.pos())));
        out.raw(data);
    }
    let om_at = out.pos();
    let om = object_map(&map);
    out.raw(&om);
    let fs_at = out.pos();
    let fs = obj_free_space(map.len(), parts.jd.updated, usize_to_f64(obj_start));
    out.raw(&fs);
    let loc: [(u32, usize, usize); 4] = [(0, hv_at, hv.len()), (1, cl_at, cl.len()), (2, om_at, om.len()), (3, fs_at, fs.len())];
    // second file header (ch. 26)
    let sh_at = out.pos();
    {
        let mut w = BitWriter::new(version);
        w.bl(usize_to_f64(sh_at));
        for c in [0x41u8, 0x43, 0x31, 0x30, 0x31, 0x35, 0, 0, 0, 0, 0] {
            w.rc8(c);
        }
        w.rc(f64::from(maint));
        w.rc8(1);
        w.bs(f64::from(code | (maint << 8)));
        w.rs_i(codepage);
        w.bs(6.0);
        let mut all: Vec<(u32, usize, usize)> = loc.to_vec();
        all.push((4, 0, 1));
        all.push((5, ax_at, aux.len()));
        for (n, at, size) in all {
            w.rc(f64::from(n));
            w.bl(usize_to_f64(at));
            w.bl(usize_to_f64(size));
        }
        w.bs(usize_to_f64(parts.keys.len()));
        for (i, &h) in parts.keys.iter().enumerate() {
            let bytes = crate::w_bits::handle_bytes(h);
            w.rc(usize_to_f64(bytes.len()));
            w.rc(usize_to_f64(i));
            for b in bytes {
                w.rc8(b);
            }
        }
        let data = w.bytes();
        out.raw(&SECOND);
        let size_at = out.pos();
        out.rl(usize_to_f64(data.len()) + 4.0 + 2.0 + 8.0);
        out.raw(&data);
        let c = crc16(0xC0C1, &out.buf, size_at, out.buf.len());
        out.rs_i(i32::from(c));
        out.zeros(8);
        out.raw(&SECOND_END);
    }
    let tpl_at = out.pos();
    let tpl = template_section(parts.measurement, "AC1015");
    out.raw(&tpl);
    // file header (3.2), written last
    let mut fh = ByteWriter::new(128);
    fh.raw(&[0x41, 0x43, 0x31, 0x30, 0x31, 0x35]);
    fh.raw(&[0, 0, 0, 0, 0, crate::util::u8_of(maint), 1]);
    fh.rl(usize_to_f64(pv_at));
    fh.rc(f64::from(code));
    fh.rc(f64::from(maint));
    fh.rs_i(codepage);
    fh.rl_u(6);
    let mut all: Vec<(u32, usize, usize)> = loc.to_vec();
    all.push((4, tpl_at, tpl.len()));
    all.push((5, ax_at, aux.len()));
    for (n, at, size) in all {
        fh.rc(f64::from(n));
        fh.rl(usize_to_f64(at));
        fh.rl(usize_to_f64(size));
    }
    let c = crc16(0, &fh.buf, 0, fh.buf.len()) ^ 0x8461;
    fh.rs_i(i32::from(c));
    fh.raw(&FILEHDR_END);
    if fh.buf.len() != hdr_len {
        return Err("DWG file header size mismatch".into());
    }
    if let Some(dst) = out.buf.get_mut(..hdr_len) {
        dst.copy_from_slice(&fh.buf);
    }
    Ok(out.buf)
}

// ---------------------------------------------------------------- R2004 family (AC1018, AC1024, AC1027, AC1032)
const PAGE_DATA: u32 = 0x4163_043B;
const PAGE_MAP: u32 = 0x4163_0E3B;
const SECTION_MAP: u32 = 0x4163_003B;

struct Page {
    id: u32,
    addr: usize,
    size: usize,
}
struct DescPage {
    id: u32,
    data_size: usize,
    start: usize,
}
struct Desc {
    name: &'static str,
    size: usize,
    max_size: u32,
    compressed: u32,
    id: u32,
    pages: Vec<DescPage>,
}

fn pad32(out: &mut ByteWriter, magic: &[u8]) {
    while !out.pos().is_multiple_of(0x20) {
        out.rc8(magic.get(out.pos() % 0x100).copied().unwrap_or(0));
    }
}

pub fn layout_r2004(parts: &Parts, version: &str) -> Result<Vec<u8>, String> {
    let (code, maint) = version_info(version);
    let codepage = 30;
    let magic = r2004_magic(0x100);
    // AcDb:AcDbObjects: RL 0x0DCA then the objects (20); the object map's offsets are relative to this section.
    let mut ob = ByteWriter::new(1 << 16);
    ob.rl_u(0x0DCA);
    let mut map: Vec<(f64, f64)> = Vec::with_capacity(parts.objects.len());
    for (h, data) in &parts.objects {
        map.push((*h, usize_to_f64(ob.pos())));
        ob.raw(data);
    }
    let mut sections: Vec<(&'static str, Vec<u8>, u32, usize)> =
        vec![("AcDb:SummaryInfo", summary_info(version, &parts.jd), 1, 0x100), ("AcDb:Preview", preview_section(), 1, 0x400)];
    if let Some(a) = &parts.acds {
        sections.push(("AcDb:AcDsPrototype_1b", a.clone(), 2, 0x7400));
    }
    sections.push(("AcDb:RevHistory", rev_history(version), 2, 0x7400));
    sections.push(("AcDb:AcDbObjects", ob.buf, 2, 0x7400));
    sections.push(("AcDb:ObjFreeSpace", obj_free_space(map.len(), parts.jd.updated, 0.0), 2, 0x7400));
    sections.push(("AcDb:Template", template_section(parts.measurement, version), 2, 0x7400));
    sections.push(("AcDb:Handles", object_map(&map), 2, 0x7400));
    sections.push(("AcDb:Classes", classes_section(&parts.classes, version), 2, 0x7400));
    sections.push(("AcDb:AuxHeader", aux_header(version, &parts.jd, parts.handseed), 2, 0x7400));
    sections.push(("AcDb:Header", header_section(&parts.header, version), 2, 0x7400));
    // section ids: "" is 0, the rest count down in section-map order (4.5)
    let mut map_order: Vec<&'static str> = Vec::new();
    if parts.acds.is_some() {
        map_order.push("AcDb:AcDsPrototype_1b");
    }
    map_order.extend([
        "AcDb:Preview",
        "AcDb:SummaryInfo",
        "AcDb:RevHistory",
        "AcDb:AcDbObjects",
        "AcDb:ObjFreeSpace",
        "AcDb:Template",
        "AcDb:Handles",
        "AcDb:Classes",
        "AcDb:AuxHeader",
        "AcDb:Header",
    ]);
    let id_of = |n: &str| -> u32 {
        let i = map_order.iter().position(|x| *x == n).unwrap_or(0);
        u32_of_usize(map_order.len().saturating_sub(i))
    };

    let mut out = ByteWriter::new(1 << 16);
    out.zeros(0x100);
    let mut pages: Vec<Page> = Vec::new();
    let mut descs: Vec<Desc> = Vec::new();
    let mut page_id: u32 = 0;
    for (name, data, compressed, max_size) in &sections {
        let id = id_of(name);
        let mut desc = Desc { name, size: data.len(), max_size: u32_of_usize(*max_size), compressed: *compressed, id, pages: Vec::new() };
        let mut start = 0usize;
        while start < data.len().max(1) {
            let end = data.len().min(start.saturating_add(*max_size));
            let logical = data.get(start..end).unwrap_or(&[]);
            let mut full = vec![0u8; *max_size]; // pages are stored / compressed whole (spec 4.5)
            if let Some(dst) = full.get_mut(..logical.len()) {
                dst.copy_from_slice(logical);
            }
            let payload = if *compressed == 2 { compress_r2004(&full, false) } else { full };
            let addr = out.pos();
            let page_size = (32usize.saturating_add(payload.len()).saturating_add(0x1F)) & !0x1F;
            let data_sum = page_checksum(0, &payload, 0, payload.len());
            let mut hdr = ByteWriter::new(32);
            hdr.rl_u(PAGE_DATA);
            hdr.rl_u(id);
            hdr.rl_u(u32_of_usize(payload.len()));
            hdr.rl_u(u32_of_usize(page_size));
            hdr.rll(usize_to_f64(start));
            hdr.rl_u(0);
            hdr.rl_u(data_sum);
            let mut h = hdr.buf;
            let hdr_sum = page_checksum(data_sum, &h, 0, h.len());
            if let Some(dst) = h.get_mut(24..28) {
                dst.copy_from_slice(&hdr_sum.to_le_bytes());
            }
            let mask = 0x4164_536B_u32 ^ crate::util::low_u32(crate::util::usize_to_u64(addr));
            for k in (0..32usize).step_by(4) {
                if let Some(q) = h.get_mut(k..k.saturating_add(4)) {
                    let v = u32::from_le_bytes([
                        q.first().copied().unwrap_or(0),
                        q.get(1).copied().unwrap_or(0),
                        q.get(2).copied().unwrap_or(0),
                        q.get(3).copied().unwrap_or(0),
                    ]) ^ mask;
                    q.copy_from_slice(&v.to_le_bytes());
                }
            }
            out.raw(&h);
            out.raw(&payload);
            pad32(&mut out, &magic);
            page_id = page_id.saturating_add(1);
            pages.push(Page { id: page_id, addr, size: out.pos().saturating_sub(addr) });
            desc.pages.push(DescPage { id: page_id, data_size: payload.len(), start });
            if data.is_empty() {
                break;
            }
            start = start.saturating_add(*max_size);
        }
        descs.push(desc);
    }

    // Spec gap: AutoCAD leaves two page numbers unused between the data pages and the two system pages.
    let section_map_id = page_id.saturating_add(3);
    let page_map_id = page_id.saturating_add(4);

    // section map (4.5)
    let mut sm = ByteWriter::new(4096);
    let n_all = 1usize.saturating_add(map_order.len());
    sm.rl_u(u32_of_usize(n_all));
    sm.rl_u(2);
    sm.rl_u(0x7400);
    sm.rl_u(0);
    sm.rl_u(u32_of_usize(n_all));
    let empty = Desc { name: "", size: 0, max_size: 0x7400, compressed: 2, id: 0, pages: Vec::new() };
    let mut all: Vec<&Desc> = vec![&empty];
    for n in &map_order {
        if let Some(d) = descs.iter().find(|d| d.name == *n) {
            all.push(d);
        }
    }
    for d in all {
        sm.rll(usize_to_f64(d.size));
        sm.rl_u(u32_of_usize(d.pages.len()));
        sm.rl_u(d.max_size);
        sm.rl_u(1);
        sm.rl_u(d.compressed);
        sm.rl_u(d.id);
        sm.rl_u(0);
        let mut nm = [0u8; 64];
        for (i, c) in d.name.bytes().enumerate().take(64) {
            if let Some(x) = nm.get_mut(i) {
                *x = c;
            }
        }
        sm.raw(&nm);
        for p in &d.pages {
            sm.rl_u(p.id);
            sm.rl_u(u32_of_usize(p.data_size));
            sm.rll(usize_to_f64(p.start));
        }
    }
    let sm_addr = out.pos();
    out.raw(&system_page(SECTION_MAP, &sm.buf));
    pad32(&mut out, &magic);
    pages.push(Page { id: section_map_id, addr: sm_addr, size: out.pos().saturating_sub(sm_addr) });

    // section page map (4.4): its own entry included; its size is known before it is written
    let pm_addr = out.pos();
    let pm_entries = |pm_size: usize| -> Vec<u8> {
        let mut pm = ByteWriter::new(pages.len().saturating_mul(8).saturating_add(16));
        for p in &pages {
            pm.rl_u(p.id);
            pm.rl_u(u32_of_usize(p.size));
        }
        pm.rl_u(page_map_id);
        pm.rl_u(u32_of_usize(pm_size));
        pm.buf
    };
    let mut pm_size = 0x20usize;
    let mut pm_bytes = Vec::new();
    for _ in 0..4 {
        // the page's size depends on its compressed length; settle it
        pm_bytes = system_page(PAGE_MAP, &pm_entries(pm_size));
        let s = (pm_bytes.len().saturating_add(0x1F)) & !0x1F;
        if s <= pm_size {
            break;
        }
        pm_size = s;
    }
    // a settled size larger than the bytes: pad, so the recorded size is the page's real length
    if pm_bytes.len() < pm_size {
        pm_bytes.resize(pm_size, 0);
    }
    out.raw(&pm_bytes);
    // Spec gap: the page map's page is not padded — the second copy of the file header follows it directly.
    let second_at = out.pos();

    // the file header (4.1): 0x6C bytes, encrypted, repeated at the end
    let mut fhd = ByteWriter::new(0x6C);
    fhd.raw(b"AcFssFcAJMB");
    fhd.rc8(0);
    for v in [0u32, 0x6C, 0x04, 0, 0, 0, 1] {
        fhd.rl_u(v);
    }
    fhd.rl_u(page_map_id); // last section page id
    fhd.rll(usize_to_f64(pm_addr) + usize_to_f64(pm_size) - 256.0); // last section page end address
    fhd.rll(usize_to_f64(second_at)); // second header address
    fhd.rl_u(0); // gap amount
    fhd.rl_u(page_id.saturating_add(2)); // section page amount
    fhd.rl_u(0x20);
    fhd.rl_u(0x80);
    fhd.rl_u(0x40);
    fhd.rl_u(page_map_id);
    fhd.rll(usize_to_f64(pm_addr) - 256.0);
    fhd.rl_u(section_map_id);
    fhd.rl_u(page_map_id); // section page array size
    fhd.rl_u(0); // gap array size
    fhd.rl_u(0); // CRC placeholder
    let mut plain = fhd.buf;
    let c = crc32(&plain, 0);
    if let Some(dst) = plain.get_mut(0x68..0x6C) {
        dst.copy_from_slice(&c.to_le_bytes());
    }
    let enc: Vec<u8> = plain.iter().enumerate().map(|(i, b)| b ^ magic.get(i).copied().unwrap_or(0)).collect();
    out.raw(&enc);

    let mut head = ByteWriter::new(0x100);
    head.raw(version.as_bytes());
    head.raw(&[0, 0, 0, 0, 0]);
    head.rc(f64::from(maint));
    head.rc8(3);
    let page_at = |name: &str| -> usize {
        let i = sections.iter().position(|s| s.0 == name).unwrap_or(0);
        pages.get(i).map_or(0, |p| p.addr)
    };
    head.rl(usize_to_f64(page_at("AcDb:Preview")) + 32.0);
    head.rc(f64::from(code));
    head.rc(f64::from(maint));
    head.rs_i(codepage);
    head.raw(&[0, 0, 0]);
    head.rl_u(0);
    head.rl_u(0);
    head.rl(usize_to_f64(page_at("AcDb:SummaryInfo")) + 32.0);
    head.rl_u(0);
    head.rl_u(0x80);
    let fill = 0x80usize.saturating_sub(head.pos());
    head.zeros(fill);
    head.raw(&enc);
    head.raw(magic.get(0xEC..0x100).unwrap_or(&[]));
    if let Some(dst) = out.buf.get_mut(..head.buf.len()) {
        dst.copy_from_slice(&head.buf);
    }
    Ok(out.buf)
}

/// A system page (4.3): 0x14-byte header, the compressed data, then the same header again with sizes and checksum
/// zero. Spec gap: the trailing copy of the header is what AutoCAD writes.
fn system_page(ty: u32, data: &[u8]) -> Vec<u8> {
    let z = compress_r2004(data, false);
    let mut h = ByteWriter::new(0x14);
    h.rl_u(ty);
    h.rl_u(u32_of_usize(data.len()));
    h.rl_u(u32_of_usize(z.len()));
    h.rl_u(2);
    h.rl_u(0);
    let mut hb = h.buf;
    let sum = page_checksum(page_checksum(0, &hb, 0, hb.len()), &z, 0, z.len());
    if let Some(dst) = hb.get_mut(16..20) {
        dst.copy_from_slice(&sum.to_le_bytes());
    }
    let mut o = ByteWriter::new(z.len().saturating_add(0x28));
    o.raw(&hb);
    o.raw(&z);
    for v in [ty, 0, 0, 2, 0] {
        o.rl_u(v);
    }
    o.buf
}
