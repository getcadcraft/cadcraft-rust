//! The R2007 (AC1021) container (port of lib/dwg/r2007-write.js; ODA spec ch. 5): Reed-Solomon (255,239) and
//! (255,251) encoding with interleaving, the CRC random encoder, 64-bit CRCs, system pages, data pages and the
//! RS-coded file header.
//!
//! Spec gap: RS generator roots are α¹…α^(n−k); a codeword is stored as the k data bytes followed by the n−k parity
//!   bytes, lowest-degree parity first, with data byte i the coefficient of x^(n−k+i).
//! Spec gap: the literal-copy and CRC byte orders apply as tables (5.10.1, 5.12).

use crate::crc::{CrcRandom, crc64_mirrored, crc64_normal, r2007_page_checksum, update_seed1, update_seed2};
use crate::util::{u32_of_usize, usize_to_f64, usize_to_u64};
use crate::w_bits::ByteWriter;
use crate::w_container::{
    Parts, aux_header, classes_section, header_section, obj_free_space, object_map, preview_section, rev_history, summary_info,
    template_section, version_info,
};
use crate::w_lz2004::js_hash;

// ---------------------------------------------------------------- GF(256) and Reed-Solomon (5.13)
pub struct Rs {
    pub k: usize,
    pub nk: usize,
    tab: Vec<[u8; 256]>,
}
impl Rs {
    fn new(poly: u32, nk: usize, k: usize) -> Self {
        let mut exp = [0u8; 512];
        let mut log = [0u8; 256];
        let mut x: u32 = 1;
        for i in 0..255usize {
            if let Some(e) = exp.get_mut(i) {
                *e = crate::util::u8_of(x);
            }
            if let Some(l) = log.get_mut(crate::util::usize_from_u32(x & 0xFF)) {
                *l = crate::util::u8_of(u32_of_usize(i));
            }
            x <<= 1;
            if x & 0x100 != 0 {
                x ^= poly;
            }
        }
        for i in 255..512usize {
            let v = exp.get(i.wrapping_sub(255)).copied().unwrap_or(0);
            if let Some(e) = exp.get_mut(i) {
                *e = v;
            }
        }
        let mul = |a: u8, b: u8| -> u8 {
            if a == 0 || b == 0 {
                return 0;
            }
            let s = usize::from(log.get(usize::from(a)).copied().unwrap_or(0))
                .wrapping_add(usize::from(log.get(usize::from(b)).copied().unwrap_or(0)));
            exp.get(s).copied().unwrap_or(0)
        };
        let mut g: Vec<u8> = vec![1];
        for i in 1..=nk {
            let r = exp.get(i % 255).copied().unwrap_or(0);
            let mut ng = vec![0u8; g.len().saturating_add(1)];
            for (j, &gj) in g.iter().enumerate() {
                if let Some(x) = ng.get_mut(j) {
                    *x ^= mul(gj, r);
                }
                if let Some(x) = ng.get_mut(j.saturating_add(1)) {
                    *x ^= gj;
                }
            }
            g = ng;
        }
        let tab = g
            .iter()
            .take(nk)
            .map(|&c| {
                let mut t = [0u8; 256];
                for (v, x) in t.iter_mut().enumerate() {
                    *x = mul(crate::util::u8_of(u32_of_usize(v)), c);
                }
                t
            })
            .collect();
        Rs { k, nk, tab }
    }
    /// parity (nk bytes, lowest degree first) of k data bytes, data[i] ↔ x^(nk+i)
    fn enc(&self, data: &[u8], from: usize) -> Vec<u8> {
        let nk = self.nk;
        let mut rem = vec![0u8; nk];
        let mut i = self.k;
        while i > 0 {
            i = i.wrapping_sub(1);
            let fb = data.get(from.saturating_add(i)).copied().unwrap_or(0) ^ rem.get(nk.wrapping_sub(1)).copied().unwrap_or(0);
            let mut j = nk.wrapping_sub(1);
            while j > 0 {
                let v = rem.get(j.wrapping_sub(1)).copied().unwrap_or(0)
                    ^ self.tab.get(j).map_or(0, |t| t.get(usize::from(fb)).copied().unwrap_or(0));
                if let Some(x) = rem.get_mut(j) {
                    *x = v;
                }
                j = j.wrapping_sub(1);
            }
            if let Some(x) = rem.get_mut(0) {
                *x = self.tab.first().map_or(0, |t| t.get(usize::from(fb)).copied().unwrap_or(0));
            }
        }
        rem
    }
}
pub fn rs_sys() -> Rs {
    Rs::new(0x169, 16, 239)
}
pub fn rs_data() -> Rs {
    Rs::new(0x11D, 4, 251)
}

/// RS-encode `data` (5.13.1–2), short last blocks padded with `pad()` bytes.
pub fn rs_encode(data: &[u8], code: &Rs, interleaved: bool, pad: &mut dyn FnMut() -> u8) -> Vec<u8> {
    let (k, nk) = (code.k, code.nk);
    let blocks = data.len().div_ceil(k).max(1);
    let mut full = vec![0u8; blocks.saturating_mul(k)];
    if let Some(dst) = full.get_mut(..data.len()) {
        dst.copy_from_slice(data);
    }
    for x in full.iter_mut().skip(data.len()) {
        *x = pad();
    }
    let mut out = vec![0u8; blocks.saturating_mul(255)];
    for b in 0..blocks {
        let par = code.enc(&full, b.saturating_mul(k));
        if interleaved {
            for i in 0..k {
                if let Some(x) = out.get_mut(i.saturating_mul(blocks).saturating_add(b)) {
                    *x = full.get(b.saturating_mul(k).saturating_add(i)).copied().unwrap_or(0);
                }
            }
            for (i, &p) in par.iter().enumerate() {
                if let Some(x) = out.get_mut(k.saturating_add(i).saturating_mul(blocks).saturating_add(b)) {
                    *x = p;
                }
            }
        } else {
            let src = full.get(b.saturating_mul(k)..b.saturating_mul(k).saturating_add(k)).unwrap_or(&[]);
            if let Some(dst) = out.get_mut(b.saturating_mul(k)..b.saturating_mul(k).saturating_add(src.len())) {
                dst.copy_from_slice(src);
            }
            let at = blocks.saturating_mul(k).saturating_add(b.saturating_mul(nk));
            if let Some(dst) = out.get_mut(at..at.saturating_add(par.len())) {
                dst.copy_from_slice(&par);
            }
        }
    }
    out
}

/// The CRC random encoder plus a cycling reader over its padding table (5.11).
pub struct RandomEncoder {
    pub r: CrcRandom,
    pub seed: u64,
    pad_pos: usize,
}
impl RandomEncoder {
    pub fn new(seed: u64) -> Self {
        RandomEncoder { r: CrcRandom::new(seed), seed, pad_pos: 0 }
    }
    pub fn pad_byte(&mut self) -> u8 {
        let b = self.r.padding.get(self.pad_pos).copied().unwrap_or(0);
        let n = self.r.padding.len().max(1);
        self.pad_pos = self.pad_pos.saturating_add(1).checked_rem(n).unwrap_or(0);
        b
    }
}
/// The check-data Encode (5.2.1.1.5): rotate left by control & 0x1F.
pub fn rot(value: u64, control: u64) -> u64 {
    let s = crate::util::low_u32(control & 0x1F);
    value.rotate_left(s)
}

// ---------------------------------------------------------------- R2007 compression (inverse of 5.10)
const LIT: [&[(usize, usize)]; 33] = [
    &[],
    &[(1, 0)],
    &[(1, 1), (1, 0)],
    &[(1, 2), (1, 1), (1, 0)],
    &[(1, 0), (1, 1), (1, 2), (1, 3)],
    &[(1, 4), (4, 0)],
    &[(1, 5), (4, 1), (1, 0)],
    &[(2, 5), (4, 1), (1, 0)],
    &[(4, 0), (4, 4)],
    &[(1, 8), (8, 0)],
    &[(1, 9), (8, 1), (1, 0)],
    &[(2, 9), (8, 1), (1, 0)],
    &[(4, 8), (8, 0)],
    &[(1, 12), (4, 8), (8, 0)],
    &[(1, 13), (4, 9), (8, 1), (1, 0)],
    &[(2, 13), (4, 9), (8, 1), (1, 0)],
    &[(8, 8), (8, 0)],
    &[(8, 9), (1, 8), (8, 0)],
    &[(1, 17), (16, 1), (1, 0)],
    &[(3, 16), (16, 0)],
    &[(4, 16), (16, 0)],
    &[(1, 20), (4, 16), (16, 0)],
    &[(2, 20), (4, 16), (16, 0)],
    &[(3, 20), (4, 16), (16, 0)],
    &[(8, 16), (16, 0)],
    &[(8, 17), (1, 16), (16, 0)],
    &[(1, 25), (8, 17), (1, 16), (16, 0)],
    &[(2, 25), (8, 17), (1, 16), (16, 0)],
    &[(4, 24), (8, 16), (16, 0)],
    &[(1, 28), (4, 24), (8, 16), (16, 0)],
    &[(2, 28), (4, 24), (8, 16), (16, 0)],
    &[(1, 30), (4, 26), (8, 18), (16, 2), (2, 0)],
    &[(16, 16), (16, 0)],
];
/// PERM[n][outIndex] = source index within an n-byte block.
pub fn perm_table() -> Vec<Vec<usize>> {
    let mut p: Vec<Option<Vec<usize>>> = vec![None; 33];
    fn build(n: usize, p: &mut Vec<Option<Vec<usize>>>, depth: usize) -> Vec<usize> {
        if let Some(Some(v)) = p.get(n) {
            return v.clone();
        }
        let mut out = Vec::new();
        if depth < 8 {
            for &(k, off) in LIT.get(n).copied().unwrap_or(&[]) {
                if k == 1 {
                    out.push(off);
                } else {
                    for s in build(k, p, depth.saturating_add(1)) {
                        out.push(off.saturating_add(s));
                    }
                }
            }
        }
        if let Some(slot) = p.get_mut(n) {
            *slot = Some(out.clone());
        }
        out
    }
    for n in 1..=32 {
        build(n, &mut p, 0);
    }
    p.into_iter().map(Option::unwrap_or_default).collect()
}

/// Compress with the R2007 scheme (5.10, read in reverse). See r2007-write.js for the Spec gap notes (a stream that
/// starts with 1–7 literals begins 0x20 0x00 0x00 n; the match offsets per instruction form).
pub fn compress_r2007(src: &[u8], perm: &[Vec<usize>]) -> Vec<u8> {
    let n = src.len();
    let mut out = ByteWriter::new(n.saturating_add(n >> 2).saturating_add(64));
    const HB: u32 = 15;
    let mut head = vec![-1i64; 1usize << HB];
    let mut prev = vec![-1i64; n];
    let insert = |i: usize, head: &mut Vec<i64>, prev: &mut Vec<i64>| {
        if i.saturating_add(2) < n {
            let h = js_hash(src, i, HB);
            if let (Some(p), Some(hh)) = (prev.get_mut(i), head.get(h)) {
                *p = *hh;
            }
            if let Some(hh) = head.get_mut(h) {
                *hh = i64::try_from(i).unwrap_or(-1);
            }
        }
    };
    let mut tokens: Vec<(usize, usize, usize)> = Vec::new();
    let mut i = 0usize;
    while i < n {
        let (mut best_len, mut best_dist) = (0usize, 0usize);
        if i.saturating_add(3) <= n {
            let mut c = head.get(js_hash(src, i, HB)).copied().unwrap_or(-1);
            let mut chain = 0usize;
            while c >= 0 && chain < 48 {
                chain = chain.saturating_add(1);
                let cu = usize::try_from(c).unwrap_or(0);
                let d = i.saturating_sub(cu);
                if d > 0xF000 {
                    break;
                }
                if src.get(cu) == src.get(i) && src.get(cu.saturating_add(best_len)) == src.get(i.saturating_add(best_len)) {
                    let max = n.saturating_sub(i).min(0x100 + 0xFFFF);
                    let l = crate::w_lz2004::common_len(src, cu, i, max);
                    if l > best_len {
                        best_len = l;
                        best_dist = d;
                    }
                }
                c = prev.get(cu).copied().unwrap_or(-1);
            }
        }
        if best_len >= 3 && i != 0 {
            tokens.push((i, best_len, best_dist));
            for k in i..i.saturating_add(best_len) {
                insert(k, &mut head, &mut prev);
            }
            i = i.saturating_add(best_len);
        } else {
            insert(i, &mut head, &mut prev);
            i = i.saturating_add(1);
        }
    }
    // literals: the decoder maps source block bytes to output positions by PERM; write the inverse
    let literal = |a: usize, b: usize, out: &mut ByteWriter| {
        let mut p = a;
        while b.saturating_sub(p) >= 32 {
            let mut blk = [0u8; 32];
            for (o, &s) in perm.get(32).map(Vec::as_slice).unwrap_or(&[]).iter().enumerate() {
                if let Some(x) = blk.get_mut(s) {
                    *x = src.get(p.saturating_add(o)).copied().unwrap_or(0);
                }
            }
            out.raw(&blk);
            p = p.saturating_add(32);
        }
        if b > p {
            let m = b.saturating_sub(p);
            let mut blk = vec![0u8; m];
            for (o, &s) in perm.get(m).map(Vec::as_slice).unwrap_or(&[]).iter().enumerate() {
                if let Some(x) = blk.get_mut(s) {
                    *x = src.get(p.saturating_add(o)).copied().unwrap_or(0);
                }
            }
            out.raw(&blk);
        }
    };
    let lit_len_op = |len: usize, out: &mut ByteWriter| {
        // len >= 8
        if len <= 22 {
            out.rc(usize_to_f64(len.saturating_sub(8)));
            return;
        }
        out.rc8(0x0F);
        let mut r = len.saturating_sub(0x17);
        if r < 0xFF {
            out.rc(usize_to_f64(r));
            return;
        }
        out.rc8(0xFF);
        r = r.saturating_sub(0xFF);
        while r >= 0xFFFF {
            out.rs_i(0xFFFF);
            r = r.saturating_sub(0xFFFF);
        }
        out.rs(usize_to_f64(r));
    };
    let first_end = tokens.first().map_or(n, |t| t.0);
    if first_end >= 8 {
        lit_len_op(first_end, &mut out);
        literal(0, first_end, &mut out);
    } else {
        out.raw(&[0x20, 0, 0, crate::util::u8_of(u32_of_usize(first_end & 7))]);
        literal(0, first_end, &mut out);
    }
    let mut after_match = false;
    for (t, &(start, len, dist)) in tokens.iter().enumerate() {
        let next = tokens.get(t.saturating_add(1)).map_or(n, |x| x.0);
        let lit = next.saturating_sub(start.saturating_add(len));
        let lit3 = if (1..=7).contains(&lit) { lit } else { 0 };
        let d1 = dist.saturating_sub(1);
        let f = |x: usize| usize_to_f64(x);
        // a default-form opcode 0xFx right after a match would read as the 0xF0 escape: length 15 only after a literal
        if (len <= 14 || (len == 15 && !after_match)) && d1 < 512 {
            out.rc(f((len << 4) | (d1 & 15)));
            out.rc(f(((d1 >> 4) << 3) | lit3));
        } else if len <= 18 && d1 < 8192 {
            out.rc(f(0x10 | len.saturating_sub(3)));
            out.rc(f(d1 & 0xFF));
            out.rc(f(((d1 >> 8) << 3) | lit3));
        } else if (0x13..=0x32).contains(&len) && d1 < 4096 {
            let mut l = len.saturating_sub(0x13);
            let hi = l >= 16;
            if hi {
                l = l.saturating_sub(16);
            }
            out.rc(f(if after_match { 0xF0 | l } else { l }));
            out.rc(f(d1 & 0xFF));
            out.rc(f(((d1 >> 8) << 3) | (if hi { 0x80 } else { 0 }) | lit3));
        } else if len < 0x100 && dist <= 0xFFFF {
            out.rc(f(0x20 | (len & 7)));
            out.rc(f(dist & 0xFF));
            out.rc(f(dist >> 8));
            out.rc(f((len & 0xF8) | lit3));
        } else {
            let l = len.saturating_sub(0x100);
            out.rc(f(0x28 | (l & 7)));
            out.rc(f(d1 & 0xFF));
            out.rc(f(d1 >> 8));
            out.rc(f((l >> 3) & 0xFF));
            out.rc(f(((l >> 11) << 3) | lit3));
        }
        if lit >= 8 {
            lit_len_op(lit, &mut out);
        }
        literal(start.saturating_add(len), next, &mut out);
        after_match = lit == 0;
    }
    out.buf
}

// ---------------------------------------------------------------- the file
fn hash_of(name: &str) -> u64 {
    match name {
        "AcDb:SummaryInfo" => 0x717A_060F,
        "AcDb:Preview" => 0x40AA_0473,
        "AcDb:RevHistory" => 0x60A2_05B3,
        "AcDb:AcDbObjects" => 0x674C_05A9,
        "AcDb:ObjFreeSpace" => 0x77E2_061F,
        "AcDb:Template" => 0x4A14_04CE,
        "AcDb:Handles" => 0x3F6E_0450,
        "AcDb:Classes" => 0x3F54_045F,
        "AcDb:AuxHeader" => 0x54F0_050A,
        "AcDb:Header" => 0x32B8_03D9,
        _ => 0,
    }
}
fn pad8(b: &[u8]) -> Vec<u8> {
    let n = b.len().saturating_add(7) & !7;
    let mut o = b.to_vec();
    o.resize(n, 0);
    o
}
fn align20(n: usize) -> usize {
    n.saturating_add(0x1F) & !0x1F
}
/// 5.3.1
pub fn system_page_size(data_size: usize) -> usize {
    let aligned = data_size.saturating_add(7) & !7;
    let p = (aligned.saturating_mul(2).saturating_add(238) / 239).saturating_mul(255);
    if p < 0x400 { 0x400 } else { align20(p) }
}

struct SysPage {
    bytes: Vec<u8>,
    size: usize,
    factor: usize,
    comp_size: usize,
    crc_uncompressed: u64,
    crc_compressed: u64,
}
fn system_page(data: &[u8], rnd: &mut RandomEncoder, rs: &Rs, perm: &[Vec<usize>]) -> SysPage {
    let size = system_page_size(data.len());
    let z = compress_r2007(data, perm);
    let stored: &[u8] = if z.len() < data.len() { &z } else { data };
    let d8 = pad8(stored);
    let max_pre = (size / 255).saturating_mul(239);
    let factor = max_pre.checked_div(d8.len()).unwrap_or(1).max(1);
    let mut buf = Vec::with_capacity(d8.len().saturating_mul(factor));
    for _ in 0..factor {
        buf.extend_from_slice(&d8);
    }
    let rsb = rs_encode(&buf, rs, true, &mut || rnd.pad_byte());
    let mut out = vec![0u8; size.max(rsb.len())];
    if let Some(dst) = out.get_mut(..rsb.len()) {
        dst.copy_from_slice(&rsb);
    }
    for x in out.iter_mut().skip(rsb.len()) {
        *x = rnd.pad_byte();
    }
    out.truncate(size);
    SysPage {
        bytes: out,
        size,
        factor,
        comp_size: stored.len(),
        crc_uncompressed: crc64_mirrored(data, update_seed1(0, usize_to_u64(data.len()))),
        crc_compressed: crc64_mirrored(stored, update_seed1(0, usize_to_u64(stored.len()))),
    }
}

struct DPage {
    id: u64,
    bytes: Vec<u8>,
    size: usize,
}
struct SecPage {
    offset: usize,
    size: usize,
    id: u64,
    uncomp: usize,
    comp: usize,
    checksum: u32,
    crc: u64,
}

pub fn layout_r2007(parts: &Parts) -> Result<Vec<u8>, String> {
    let version = "AC1021";
    let (code, maint) = version_info(version);
    let mut rnd = RandomEncoder::new(0x5A17_C0DE);
    let perm = perm_table();
    let rs_s = rs_sys();
    let rs_d = rs_data();
    // AcDb:AcDbObjects — RL 0x0DCA then the objects (as R2004)
    let mut ob = ByteWriter::new(1 << 16);
    ob.rl_u(0x0DCA);
    let mut map: Vec<(f64, f64)> = Vec::with_capacity(parts.objects.len());
    for (h, data) in &parts.objects {
        map.push((*h, usize_to_f64(ob.pos())));
        ob.raw(data);
    }
    // [name, data, encoding (1 = plain, 4 = RS interleaved), max page size]
    // Spec gap: encoding 1 pages are stored without Reed-Solomon coding.
    let mut preview = preview_section();
    preview.extend_from_slice(&[0u8; 32]); // compatibility: 32 zero bytes after the end sentinel (see the JS)
    let mut sections: Vec<(&'static str, Vec<u8>, u32, usize)> = vec![
        ("AcDb:SummaryInfo", summary_info(version, &parts.jd), 1, 0x80),
        ("AcDb:Preview", preview, 1, 0x400),
        ("AcDb:RevHistory", rev_history(version), 4, 0x1000),
        ("AcDb:AcDbObjects", ob.buf, 4, 0xF800),
        ("AcDb:ObjFreeSpace", obj_free_space(map.len(), parts.jd.updated, 0.0), 4, 0xF800),
        ("AcDb:Template", template_section(parts.measurement, version), 4, 0x400),
        ("AcDb:Handles", object_map(&map), 4, 0xF800),
        ("AcDb:Classes", classes_section(&parts.classes, version), 4, 0xF800),
        ("AcDb:AuxHeader", aux_header(version, &parts.jd, parts.handseed), 4, 0x800),
        ("AcDb:Header", header_section(&parts.header, version), 4, 0x800),
    ];
    if let Some(s0) = sections.first_mut()
        && s0.1.len() > 0x80
    {
        s0.3 = (s0.1.len().saturating_add(0x7F)) & !0x7F;
    }
    // data pages
    let mut data_pages: Vec<DPage> = Vec::new();
    let mut descs: Vec<(&'static str, usize, usize, u32, Vec<SecPage>)> = Vec::new();
    let mut pid: u64 = 0;
    for (name, data, encoding, max_size) in &sections {
        let (encoding, max_size) = (*encoding, *max_size);
        let mut pages: Vec<SecPage> = Vec::new();
        let mut add_page = |offset: usize, chunk: &[u8], comp: &[u8], size_field: usize, rnd: &mut RandomEncoder| {
            let checksum = r2007_page_checksum(0, chunk, 0, chunk.len());
            let crc = crc64_mirrored(comp, update_seed1(0, usize_to_u64(comp.len())));
            let stored = if encoding == 4 {
                rs_encode(&pad8(comp), &rs_d, true, &mut || rnd.pad_byte())
            } else {
                let mut st = vec![0u8; max_size.saturating_add(0x20)];
                if let Some(dst) = st.get_mut(..chunk.len()) {
                    dst.copy_from_slice(chunk);
                }
                let from = chunk.len().saturating_add(7) & !7;
                for x in st.iter_mut().skip(from) {
                    *x = rnd.pad_byte();
                }
                st
            };
            let size = align20(stored.len());
            let mut bytes = vec![0u8; size];
            if let Some(dst) = bytes.get_mut(..stored.len()) {
                dst.copy_from_slice(&stored);
            }
            pid = pid.saturating_add(1);
            data_pages.push(DPage { id: pid, bytes, size });
            pages.push(SecPage { offset, size: size_field, id: pid, uncomp: chunk.len(), comp: comp.len(), checksum, crc });
        };
        let mut start = 0usize;
        while start < data.len() || (start == 0 && data.is_empty()) {
            let chunk = data.get(start..data.len().min(start.saturating_add(max_size))).unwrap_or(&[]);
            let z = if encoding == 4 { compress_r2007(chunk, &perm) } else { chunk.to_vec() };
            if encoding != 4 || z.len() < chunk.len() || chunk.len() <= 248 {
                // compressed when it shrinks (5.4); a page that does not is stored as it is
                let comp: &[u8] = if z.len() < chunk.len() { &z } else { chunk };
                let size_field = if chunk.len() == max_size { max_size } else { max_size.max(chunk.len()) };
                add_page(start, chunk, comp, size_field, &mut rnd);
            } else {
                // Spec gap, found by experiment: a stretch that does not compress is split into pages of one RS block
                // (248 bytes).
                let mut s0 = 0usize;
                while s0 < chunk.len() {
                    let piece = chunk.get(s0..chunk.len().min(s0.saturating_add(248))).unwrap_or(&[]);
                    add_page(start.saturating_add(s0), piece, piece, piece.len(), &mut rnd);
                    s0 = s0.saturating_add(248);
                }
            }
            if data.is_empty() {
                break;
            }
            start = start.saturating_add(max_size);
        }
        descs.push((name, data.len(), max_size, encoding, pages));
    }
    let (sm_id1, sm_id2, pm_id1, pm_id2) = (pid.saturating_add(1), pid.saturating_add(2), pid.saturating_add(3), pid.saturating_add(4));
    // section map (5.2). Spec gap: the empty last section is left out but counted (see the JS).
    let mut sm = ByteWriter::new(4096);
    for (name, size, max_size, encoding, pages) in &descs {
        let name_len = if name.is_empty() { 0 } else { name.len().saturating_add(1).saturating_mul(2) };
        sm.rll_u(usize_to_u64(*size));
        sm.rll_u(usize_to_u64(*max_size));
        sm.rll_u(0);
        sm.rll_u(if name.is_empty() { 0 } else { hash_of(name) });
        sm.rll_u(usize_to_u64(name_len));
        sm.rll_u(0);
        sm.rll_u(u64::from(*encoding));
        sm.rll_u(usize_to_u64(pages.len()));
        if !name.is_empty() {
            for c in name.encode_utf16() {
                sm.rs_i(i32::from(c));
            }
            sm.rs_i(0);
        }
        for p in pages {
            sm.rll_u(usize_to_u64(p.offset));
            sm.rll_u(usize_to_u64(p.size));
            sm.rll_u(p.id);
            sm.rll_u(usize_to_u64(p.uncomp));
            sm.rll_u(usize_to_u64(p.comp));
            sm.rll_u(u64::from(p.checksum));
            sm.rll_u(p.crc);
        }
    }
    let sm_data = sm.buf;
    let sm_page1 = system_page(&sm_data, &mut rnd, &rs_s, &perm);
    let sm_page2 = system_page(&sm_data, &mut rnd, &rs_s, &perm);
    // page map: (size, id) pairs in file order — page map 1, page map 2, data pages, section maps
    let n_pages = 4usize.saturating_add(data_pages.len());
    let pm_size = system_page_size(n_pages.saturating_mul(16));
    let mut pm = ByteWriter::new(n_pages.saturating_mul(16));
    pm.rll_u(usize_to_u64(pm_size));
    pm.rll_u(pm_id1);
    pm.rll_u(usize_to_u64(pm_size));
    pm.rll_u(pm_id2);
    for p in &data_pages {
        pm.rll_u(usize_to_u64(p.size));
        pm.rll_u(p.id);
    }
    pm.rll_u(usize_to_u64(sm_page1.size));
    pm.rll_u(sm_id1);
    pm.rll_u(usize_to_u64(sm_page2.size));
    pm.rll_u(sm_id2);
    let pm_data = pm.buf;
    let pm_page1 = system_page(&pm_data, &mut rnd, &rs_s, &perm);
    let pm_page2 = system_page(&pm_data, &mut rnd, &rs_s, &perm);
    if pm_page1.size != pm_size {
        return Err("R2007 page map size".into());
    }

    // assemble: meta 0x80, header 0x400, page maps, data pages, section maps, header copy
    let mut out = ByteWriter::new(0x480usize.saturating_add(pm_size.saturating_mul(2)));
    out.zeros(0x480);
    out.raw(&pm_page1.bytes);
    out.raw(&pm_page2.bytes);
    let mut page_at: Vec<(u64, usize)> = Vec::with_capacity(data_pages.len());
    for p in &data_pages {
        page_at.push((p.id, out.pos()));
        out.raw(&p.bytes);
    }
    out.raw(&sm_page1.bytes);
    out.raw(&sm_page2.bytes);
    let header2_at = out.pos();
    let file_size = header2_at.saturating_add(0x400);

    // the file header (5.2, 5.2.1): fields, CRCs, check data, RS
    let crc_seed: u64 = 0;
    let sections_map_crc_seed = rnd.r.encode(0);
    let pages_map_crc_seed = rnd.r.encode(0);
    let random1 = rnd.r.next64();
    let random2 = rnd.r.next64();
    let encoded_crc_seed = rnd.r.encode(0);
    let b64 = |arr: &[u64]| -> Vec<u8> {
        let mut o = Vec::with_capacity(arr.len().saturating_mul(8));
        for v in arr {
            o.extend_from_slice(&v.to_le_bytes());
        }
        o
    };
    let n0 = rot(random1, random2);
    let n1 = rot(n0, n0);
    let n2 = rot(random2, n1);
    let n3 = rot(n2, n2);
    let n4 = rot(random1, n3);
    let n5 = rot(n4, n4);
    let n6 = rot(n5, n5);
    let n7 = rot(n6, n6);
    let normal_crc = crc64_normal(&b64(&[n0, n1, n2, n3, n4, n5, n6, n7]), !random2);
    let m0 = rot(random1, random2);
    let m1 = rot(normal_crc, m0);
    let m2 = rot(random2, m1);
    let m3 = rot(normal_crc, m2);
    let m4 = rot(random1, m3);
    let m5 = rot(normal_crc, m4);
    let m6 = rot(random2, m5);
    let m7 = rot(m6, m6);
    let mirrored_crc = crc64_mirrored(&b64(&[m0, m1, m2, m3, m4, m5, m6, m7]), !random1);
    let crc_seed_encoded = rnd.r.encode(0);
    let u = usize_to_u64;
    let fields: [u64; 34] = [
        0x70,
        u(file_size),
        pm_page1.crc_compressed,
        u(pm_page1.factor),
        pages_map_crc_seed,
        u(pm_size), // page map 2 offset (relative to page map 1)
        pm_id2,
        0, // page map offset
        pm_id1,
        u(header2_at.saturating_sub(0x480)),
        u(pm_page1.comp_size),
        u(pm_data.len()),
        u(n_pages),
        pm_id2,
        0x20,
        0x40,
        pm_page1.crc_uncompressed,
        0xF800,
        4,
        1,
        u(descs.len()).saturating_add(1), // "number of sections + 1"
        sm_page1.crc_uncompressed,
        u(sm_page1.comp_size),
        sm_id2,
        sm_id1,
        u(sm_data.len()),
        sm_page1.crc_compressed,
        u(sm_page1.factor),
        sections_map_crc_seed,
        0x60100,
        crc_seed,
        crc_seed_encoded,
        rnd.seed,
        0, // header CRC placeholder
    ];
    let mut hd = b64(&fields);
    let hcrc = crc64_normal(&hd, update_seed2(0, u(hd.len())));
    if let Some(dst) = hd.get_mut(0x108..0x110) {
        dst.copy_from_slice(&hcrc.to_le_bytes());
    }
    // 5.2.1.3–5: compressed (negated size when it does not shrink); the block repeated to fill 3 × 239 bytes
    let hz0 = compress_r2007(&hd, &perm);
    let raw_hd = hz0.len() >= hd.len();
    let hz: &[u8] = if raw_hd { &hd } else { &hz0 };
    let data_crc = crc64_normal(hz, update_seed2(0, u(hz.len())));
    let seq0 = rnd.r.next64();
    let seq1 = rot(seq0, seq0);
    let seq_bytes = b64(&[seq0, seq1]);
    let seq_crc = crc64_normal(&seq_bytes, update_seed1(0, 16));
    let mut blk = ByteWriter::new(0x140);
    blk.rll_u(seq_crc);
    blk.rll_u(seq0);
    blk.rll_u(data_crc);
    blk.rl(if raw_hd { -usize_to_f64(hd.len()) } else { usize_to_f64(hz.len()) });
    blk.rl_u(0);
    blk.raw(hz);
    while !blk.pos().is_multiple_of(8) {
        let b = rnd.pad_byte();
        blk.rc8(b);
    }
    let block = blk.buf;
    let mut pre = vec![0u8; 3 * 239];
    let mut p = 0usize;
    if !block.is_empty() {
        while p.saturating_add(block.len()) <= pre.len() {
            if let Some(dst) = pre.get_mut(p..p.saturating_add(block.len())) {
                dst.copy_from_slice(&block);
            }
            p = p.saturating_add(block.len());
        }
    }
    for x in pre.iter_mut().skip(p) {
        *x = rnd.pad_byte();
    }
    let rs_hdr = rs_encode(&pre, &rs_s, true, &mut || rnd.pad_byte());
    let mut page = vec![0u8; 0x400];
    if let Some(dst) = page.get_mut(..rs_hdr.len().min(0x400)) {
        dst.copy_from_slice(rs_hdr.get(..rs_hdr.len().min(0x400)).unwrap_or(&[]));
    }
    for x in page.iter_mut().take(0x3D8).skip(rs_hdr.len()) {
        *x = rnd.pad_byte();
    }
    let chk = b64(&[normal_crc, mirrored_crc, random1, random2, encoded_crc_seed]);
    if let Some(dst) = page.get_mut(0x3D8..0x400) {
        dst.copy_from_slice(chk.get(..0x28).unwrap_or(&[0u8; 0x28]));
    }
    if let Some(dst) = out.buf.get_mut(0x80..0x480) {
        dst.copy_from_slice(&page);
    }
    out.raw(&page); // the copy at the end (5.2.1.8)

    // meta data (5.2)
    let at_of =
        |i: usize| -> f64 { data_pages.get(i).and_then(|d| page_at.iter().find(|x| x.0 == d.id)).map_or(0.0, |x| usize_to_f64(x.1)) };
    let mut meta = ByteWriter::new(0x80);
    meta.raw(version.as_bytes());
    meta.raw(&[0, 0, 0, 0, 0]);
    meta.rc(f64::from(maint));
    meta.rc8(3);
    meta.rl(at_of(1)); // preview address (the page)
    meta.rc(f64::from(code));
    meta.rc(f64::from(maint));
    meta.rs_i(30);
    meta.raw(&[0, 0, 0]);
    meta.rl_u(0);
    meta.rl_u(0);
    meta.rl(at_of(0)); // summary info address
    meta.rl_u(0);
    meta.rl_u(0x80);
    meta.rl_u(0);
    if let Some(dst) = out.buf.get_mut(..meta.buf.len()) {
        dst.copy_from_slice(&meta.buf);
    }
    Ok(out.buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn r2007_compression_round_trips() {
        let perm = perm_table();
        let mut src = Vec::new();
        for i in 0..5000u32 {
            src.push(crate::util::u8_of((i * 13) % 11 + (i / 50) % 3));
        }
        let z = compress_r2007(&src, &perm);
        let back = crate::r2007::decompress_r21(&z, 0, z.len(), src.len()).expect("decompresses");
        assert_eq!(back, src);
    }
}
