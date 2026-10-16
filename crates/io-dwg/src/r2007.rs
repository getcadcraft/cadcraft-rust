//! The R2007 (AC1021) container (port of lib/dwg/r2007.js; spec ch. 5): Reed-Solomon decoding, the R21
//! LZ77 variant, file header, page map, section map and data pages.

use std::collections::HashMap;

use crate::bits::{Res, err};
use crate::container::{Container, plausible_size};
use crate::messages::msg;

struct Field {
    exp: [u8; 512],
    log: [i16; 256],
}

fn make_field(poly: u32) -> Field {
    let mut exp = [0u8; 512];
    let mut log = [0i16; 256];
    let mut x: u32 = 1;
    for i in 0..255usize {
        if let Some(e) = exp.get_mut(i) {
            *e = u8::try_from(x & 0xFF).unwrap_or(0);
        }
        if let Some(l) = log.get_mut(x as usize) {
            *l = i16::try_from(i).unwrap_or(0);
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
    if let Some(l) = log.get_mut(0) {
        *l = -1;
    }
    Field { exp, log }
}

fn fields() -> &'static (Field, Field) {
    static F: std::sync::OnceLock<(Field, Field)> = std::sync::OnceLock::new();
    F.get_or_init(|| (make_field(0x169), make_field(0x11D)))
}

impl Field {
    fn e(&self, i: usize) -> u32 {
        u32::from(self.exp.get(i).copied().unwrap_or(0))
    }
    fn l(&self, a: u32) -> i32 {
        i32::from(self.log.get(a as usize).copied().unwrap_or(0))
    }
    fn mul(&self, a: u32, b: u32) -> u32 {
        if a == 0 || b == 0 {
            return 0;
        }
        let s = self.l(a).wrapping_add(self.l(b));
        self.e(usize::try_from(s).unwrap_or(0))
    }
    fn exp_mod(&self, i: i64) -> u32 {
        self.e(usize::try_from(i.rem_euclid(255)).unwrap_or(0))
    }
}

// GF(256) kernel: i <= nk <= 255, so `i % 255` and the loop counter are bounded by constants.
#[allow(clippy::arithmetic_side_effects)]
fn syndromes(f: &Field, c: &[u32; 255], nk: usize) -> Option<Vec<u32>> {
    let mut s = vec![0u32; nk.saturating_add(1)];
    let mut zero = true;
    for i in 1..=nk {
        let a = f.e(i % 255);
        let mut v = 0;
        for &cj in c {
            v = f.mul(v, a) ^ cj;
        }
        if let Some(x) = s.get_mut(i) {
            *x = v;
        }
        if v != 0 {
            zero = false;
        }
    }
    if zero { None } else { Some(s) }
}

/// Berlekamp–Massey + Chien + Forney on one reversed codeword; returns (ok, corrected).
// GF(256) kernel: every index and counter is bounded by nk <= 255 or by lam/bb lengths <= 2 * 255 + 1.
#[allow(clippy::arithmetic_side_effects)]
fn rs_decode_reversed(c: &mut [u32; 255], k: usize, sys: bool) -> (bool, usize) {
    let (fs, fd) = fields();
    let f = if sys { fs } else { fd };
    let nk = 255usize.saturating_sub(k);
    let Some(s) = syndromes(f, c, nk) else { return (true, 0) };
    let sg = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut lam: Vec<u32> = vec![1];
    let mut bb: Vec<u32> = vec![1];
    let mut l: usize = 0;
    let mut m: usize = 1;
    let mut b: u32 = 1;
    for n in 0..nk {
        let mut d = sg(n + 1);
        for i in 1..=l {
            d ^= f.mul(lam.get(i).copied().unwrap_or(0), sg((n + 1).saturating_sub(i)));
        }
        if d == 0 {
            m += 1;
            continue;
        }
        let coef = f.mul(d, f.exp_mod(255 - i64::from(f.l(b))));
        let t = lam.clone();
        let need = bb.len() + m;
        while lam.len() < need {
            lam.push(0);
        }
        for (i, &bi) in bb.iter().enumerate() {
            if let Some(x) = lam.get_mut(i + m) {
                *x ^= f.mul(coef, bi);
            }
        }
        if 2 * l <= n {
            l = n + 1 - l;
            bb = t;
            b = d;
            m = 1;
        } else {
            m += 1;
        }
    }
    let mut errs = Vec::new();
    for j in 0..255usize {
        let xinv = f.exp_mod(255 - (254 - i64::try_from(j).unwrap_or(0)));
        let mut v = 0;
        for i in (0..lam.len()).rev() {
            v = f.mul(v, xinv) ^ lam.get(i).copied().unwrap_or(0);
        }
        if v == 0 {
            errs.push(j);
        }
    }
    if errs.len() != l || l * 2 > nk {
        return (false, 0);
    }
    let mut om = vec![0u32; nk];
    for i in 0..nk {
        let mut acc = 0;
        for j in 0..=i.min(lam.len().saturating_sub(1)) {
            acc ^= f.mul(lam.get(j).copied().unwrap_or(0), sg(i - j + 1));
        }
        if let Some(x) = om.get_mut(i) {
            *x = acc;
        }
    }
    for &j in &errs {
        let xinv = f.exp_mod(255 - (254 - i64::try_from(j).unwrap_or(0)));
        let mut num = 0;
        for i in (0..nk).rev() {
            num = f.mul(num, xinv) ^ om.get(i).copied().unwrap_or(0);
        }
        let mut den = 0;
        let mut i = 1;
        while i < lam.len() {
            den ^= f.mul(lam.get(i).copied().unwrap_or(0), f.exp_mod(i64::from(f.l(xinv)) * (i64::try_from(i).unwrap_or(0) - 1)));
            i += 2;
        }
        if den == 0 {
            return (false, 0);
        }
        let e = f.mul(num, f.exp_mod(255 - i64::from(f.l(den))));
        if let Some(x) = c.get_mut(j) {
            *x ^= e;
        }
    }
    if syndromes(f, c, nk).is_some() { (false, 0) } else { (true, errs.len()) }
}

#[derive(Default)]
struct Stats {
    bad: usize,
    corrected: usize,
}

/// Interleaved codewords: byte i of block b at `from + i*blocks + b`. Returns the data bytes.
fn deinterleave(src: &[u8], from: usize, blocks: usize, k: usize, sys: bool, stats: Option<&mut Stats>) -> Vec<u8> {
    let mut out = vec![0u8; blocks.saturating_mul(k)];
    let mut stats = stats;
    for bl in 0..blocks {
        let mut cw = [0u8; 255];
        for (i, x) in cw.iter_mut().enumerate() {
            *x = src.get(from.saturating_add(i.saturating_mul(blocks)).saturating_add(bl)).copied().unwrap_or(0);
        }
        if let Some(st) = stats.as_deref_mut() {
            // reversed word: byte j <-> x^(254-j)
            let mut c = [0u32; 255];
            for (i, x) in c.iter_mut().enumerate() {
                *x = u32::from(cw.get(254usize.saturating_sub(i)).copied().unwrap_or(0)); // i < 255
            }
            let (ok, n) = rs_decode_reversed(&mut c, k, sys);
            if !ok {
                st.bad = st.bad.saturating_add(1);
            } else {
                st.corrected = st.corrected.saturating_add(n);
                if n > 0 {
                    for (i, x) in cw.iter_mut().enumerate() {
                        *x = u8::try_from(c.get(254usize.saturating_sub(i)).copied().unwrap_or(0) & 0xFF).unwrap_or(0);
                    }
                }
            }
        }
        for i in 0..k {
            if let Some(o) = out.get_mut(bl.saturating_mul(k).saturating_add(i)) {
                *o = cw.get(i).copied().unwrap_or(0);
            }
        }
    }
    out
}

/// 5.10.1 literal copy order for runs of 1..32 bytes.
fn lit_order() -> &'static Vec<Vec<usize>> {
    static T: std::sync::OnceLock<Vec<Vec<usize>>> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let table: [&[(usize, usize)]; 33] = [
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
        fn expand(n: usize, table: &[&[(usize, usize)]; 33], depth: u32) -> Vec<usize> {
            if n == 1 || depth > 8 {
                return vec![0];
            }
            let mut out = Vec::new();
            for &(k, i) in table.get(n).copied().unwrap_or(&[]) {
                if k == n {
                    out.extend((0..k).map(|j| i.saturating_add(j))); // table values <= 32
                    continue;
                }
                out.extend(expand(k, table, depth.saturating_add(1)).into_iter().map(|j| i.saturating_add(j)));
            }
            out
        }
        let mut r = vec![Vec::new()];
        for n in 1..=32 {
            r.push(expand(n, &table, 0));
        }
        r
    })
}

/// 5.10 decompression of `src[from..from+len]` into `out_size` bytes.
pub fn decompress_r21(src: &[u8], from: usize, len: usize, out_size: usize) -> Res<Vec<u8>> {
    let mut out = vec![0u8; out_size];
    let end = from.saturating_add(len);
    let mut ip = from;
    let mut op = 0usize;
    let ord = lit_order();
    let at = |i: usize| u32::from(src.get(i).copied().unwrap_or(0));
    let lit_copy = |ip: &mut usize, op: &mut usize, mut n: usize, out: &mut Vec<u8>| {
        while n > 0 {
            let k = n.min(32);
            let o = ord.get(k).map(Vec::as_slice).unwrap_or(&[]);
            let mut j = 0;
            while j < k && *op < out_size {
                let srci = ip.saturating_add(o.get(j).copied().unwrap_or(0));
                if let Some(x) = out.get_mut(*op) {
                    *x = src.get(srci).copied().unwrap_or(0);
                }
                // op < out_size and j < k <= 32
                *op = op.saturating_add(1);
                j = j.saturating_add(1);
            }
            *ip = ip.saturating_add(k);
            n = n.saturating_sub(k); // k <= n
        }
    };
    let lit_length = |ip: &mut usize, oc: u32| -> usize {
        let mut length = (oc as usize).saturating_add(8); // oc <= 255
        if length == 0x17 {
            let mut n = at(*ip);
            *ip = ip.saturating_add(1);
            length = length.saturating_add(n as usize);
            if n == 0xFF {
                loop {
                    n = at(*ip) | (at(ip.saturating_add(1)) << 8);
                    *ip = ip.saturating_add(2);
                    length = length.saturating_add(n as usize);
                    if n != 0xFFFF || *ip > end.saturating_add(4) {
                        break;
                    }
                }
            }
        }
        length
    };
    let mut length: usize = 0;
    let mut oc = at(ip);
    if oc >> 4 == 2 {
        ip = ip.saturating_add(3);
        length = (at(ip) & 7) as usize;
        ip = ip.saturating_add(1);
    }
    let mut pending: Option<u32> = None;
    while ip < end && op < out_size {
        if length == 0 {
            let c = match pending.take() {
                Some(p) => p,
                None => {
                    let c = at(ip);
                    ip = ip.saturating_add(1);
                    c
                }
            };
            length = lit_length(&mut ip, c);
        }
        lit_copy(&mut ip, &mut op, length, &mut out);
        if ip >= end {
            break;
        }
        oc = at(ip);
        ip = ip.saturating_add(1);
        let mut offset: usize = 0;
        let mut count: usize = 0;
        // every operand below is a masked byte, so the sums stay far below usize::MAX
        let instr = |oc: &mut u32, ip: &mut usize, offset: &mut usize, count: &mut usize| {
            let mut next = || {
                let v = at(*ip);
                *ip = ip.saturating_add(1);
                v
            };
            match *oc >> 4 {
                0 => {
                    *count = ((*oc & 0xF) as usize).saturating_add(0x13);
                    *offset = next() as usize;
                    *oc = next();
                    *count = count.saturating_add(((*oc >> 3) & 0x10) as usize);
                    *offset = offset.saturating_add((((*oc & 0x78) << 5) as usize).saturating_add(1));
                }
                1 => {
                    *count = ((*oc & 0xF) as usize).saturating_add(3);
                    *offset = next() as usize;
                    *oc = next();
                    *offset = offset.saturating_add((((*oc & 0xF8) << 5) as usize).saturating_add(1));
                }
                2 => {
                    *offset = next() as usize;
                    *offset |= ((next() << 8) & 0xFF00) as usize;
                    *count = (*oc & 7) as usize;
                    if *oc & 8 == 0 {
                        *oc = next();
                        *count = count.saturating_add((*oc & 0xF8) as usize);
                    } else {
                        *offset = offset.saturating_add(1);
                        *count = count.saturating_add((next() << 3) as usize);
                        *oc = next();
                        *count = count.saturating_add((((*oc & 0xF8) << 8) as usize).saturating_add(0x100));
                    }
                }
                _ => {
                    *count = (*oc >> 4) as usize;
                    *offset = (*oc & 15) as usize;
                    *oc = next();
                    *offset = offset.saturating_add((((*oc & 0xF8) << 1) as usize).saturating_add(1));
                }
            }
        };
        instr(&mut oc, &mut ip, &mut offset, &mut count);
        loop {
            let Some(mut s) = op.checked_sub(offset) else {
                return err(msg("ccdwg.err.r2007_lz", &[]));
            };
            let mut i = 0;
            while i < count && op < out_size {
                let b = out.get(s).copied().unwrap_or(0);
                if let Some(x) = out.get_mut(op) {
                    *x = b;
                }
                // op < out_size, s < op, i < count
                op = op.saturating_add(1);
                s = s.saturating_add(1);
                i = i.saturating_add(1);
            }
            length = (oc & 7) as usize;
            if length != 0 || ip >= end {
                break;
            }
            oc = at(ip);
            ip = ip.saturating_add(1);
            if oc >> 4 == 0 {
                pending = Some(oc);
                break;
            }
            if oc >> 4 == 15 {
                oc &= 15;
            }
            instr(&mut oc, &mut ip, &mut offset, &mut count);
        }
    }
    Ok(out)
}

fn u64_at(b: &[u8], at: usize) -> f64 {
    let mut a = [0u8; 8];
    if let Some(s) = b.get(at..at.saturating_add(8)) {
        a.copy_from_slice(s);
    }
    crate::util::i64_to_f64(i64::try_from(u64::from_le_bytes(a)).unwrap_or(i64::MAX))
}
fn i64_at(b: &[u8], at: usize) -> f64 {
    let mut a = [0u8; 8];
    if let Some(s) = b.get(at..at.saturating_add(8)) {
        a.copy_from_slice(s);
    }
    crate::util::i64_to_f64(i64::from_le_bytes(a))
}
fn u16_at(b: &[u8], at: usize) -> u32 {
    u32::from(b.get(at).copied().unwrap_or(0)) | (u32::from(b.get(at.saturating_add(1)).copied().unwrap_or(0)) << 8)
}
fn us(n: f64) -> usize {
    crate::util::to_usize(n)
}

pub fn read_r2007_container(b: &[u8], version: &str, ver: u32) -> Res<Container> {
    let mut notes = Vec::new();
    let maintenance = u32::from(b.get(0x0B).copied().unwrap_or(0));
    let codepage_num = u16_at(b, 0x13);
    let security = u16_at(b, 0x18) | (u16_at(b, 0x1A) << 16);
    if security & 1 != 0 {
        notes.push(msg("ccdwg.read.password", &[]));
    }
    if b.len() < 0x480 {
        return err(msg("ccdwg.open.r2007_short", &[]));
    }
    let mut stats = Stats::default();
    let hdr = deinterleave(b, 0x80, 3, 239, true, Some(&mut stats));
    if stats.bad > 0 {
        notes.push(msg("ccdwg.read.r2007_header_bad", &[]));
    } else if stats.corrected > 0 {
        notes.push(msg("ccdwg.read.r2007_corrected", &[("n", &stats.corrected.to_string())]));
    }
    let compr_len = i32::from_le_bytes([
        hdr.get(0x18).copied().unwrap_or(0),
        hdr.get(0x19).copied().unwrap_or(0),
        hdr.get(0x1A).copied().unwrap_or(0),
        hdr.get(0x1B).copied().unwrap_or(0),
    ]);
    let fh = if compr_len < 0 {
        let n = usize::try_from(compr_len.unsigned_abs()).unwrap_or(0);
        hdr.get(0x20..0x20usize.saturating_add(n)).map(<[u8]>::to_vec).unwrap_or_default()
    } else {
        decompress_r21(&hdr, 0x20, usize::try_from(compr_len).unwrap_or(0), 0x110)?
    };
    let pm_corr = u64_at(&fh, 0x18);
    let pm_off = u64_at(&fh, 0x38);
    let pm_csize = u64_at(&fh, 0x50);
    let pm_usize = u64_at(&fh, 0x58);
    let sm_csize = u64_at(&fh, 0xB0);
    let sm_id = u64_at(&fh, 0xC0);
    let sm_usize = u64_at(&fh, 0xC8);
    let sm_corr = u64_at(&fh, 0xD8);
    let pm =
        load_sys_page(b, 0x480usize.saturating_add(us(pm_off)), pm_csize, pm_usize, pm_corr, &mut notes, &msg("ccdwg.part.page_map", &[]))?;
    let mut pages: HashMap<i64, (f64, f64)> = HashMap::new(); // |id| → (offset, size)
    let mut offset = 0.0f64;
    let mut p = 0usize;
    // p + 16 <= pm.len() inside the loops below, so the offsets cannot overflow
    while p.saturating_add(16) <= pm.len() {
        let size = i64_at(&pm, p);
        let id = i64_at(&pm, p.saturating_add(8));
        pages.insert(crate::util::to_i64(id.abs()), (offset, size));
        offset += size;
        p = p.saturating_add(16);
    }
    let Some(&(sm_off, _)) = pages.get(&crate::util::to_i64(sm_id)) else {
        return err(msg("ccdwg.open.r2007_no_section_map", &[]));
    };
    let sm = load_sys_page(
        b,
        0x480usize.saturating_add(us(sm_off)),
        sm_csize,
        sm_usize,
        sm_corr,
        &mut notes,
        &msg("ccdwg.part.section_map", &[]),
    )?;
    let mut sections: Vec<(String, Vec<u8>)> = Vec::new();
    let mut p = 0usize;
    while p.saturating_add(0x40) <= sm.len() {
        let data_size = u64_at(&sm, p);
        let max_size = u64_at(&sm, p.saturating_add(8));
        let encryption = u64_at(&sm, p.saturating_add(0x10));
        let name_len = us(u64_at(&sm, p.saturating_add(0x20)));
        let encoding = u64_at(&sm, p.saturating_add(0x30));
        let num_pages = u64_at(&sm, p.saturating_add(0x38));
        p = p.saturating_add(0x40);
        let mut units = Vec::new();
        let mut i = 0usize;
        while i.saturating_add(1) < name_len {
            units.push(u16::try_from(u16_at(&sm, p.saturating_add(i))).unwrap_or(0));
            i = i.saturating_add(2);
        }
        while units.last() == Some(&0) {
            units.pop();
        }
        let name = String::from_utf16_lossy(&units);
        p = p.saturating_add(name_len);
        let mut list = Vec::new();
        let mut k = 0.0;
        while k < num_pages && p.saturating_add(56) <= sm.len() {
            list.push(PageRef {
                offset: u64_at(&sm, p),
                id: u64_at(&sm, p.saturating_add(16)),
                usize: u64_at(&sm, p.saturating_add(24)),
                csize: u64_at(&sm, p.saturating_add(32)),
            });
            p = p.saturating_add(56);
            k += 1.0;
        }
        if name.is_empty() {
            continue;
        }
        if encryption == 1.0 {
            notes.push(msg("ccdwg.read.section_encrypted", &[("name", &name)]));
            continue;
        }
        match read_data_section21(b, &pages, &list, data_size, encoding, &name, &mut notes) {
            Ok(d) => sections.push((name, d)),
            Err(e) => notes.push(msg("ccdwg.read.section_failed", &[("name", &name), ("reason", &e.0)])),
        }
        let _ = max_size;
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

struct PageRef {
    offset: f64,
    id: f64,
    usize: f64,
    csize: f64,
}

/// 5.3 system page.
fn load_sys_page(b: &[u8], at: usize, csize: f64, usize_: f64, factor: f64, notes: &mut Vec<String>, what: &str) -> Res<Vec<u8>> {
    plausible_size(usize_, b.len())?;
    plausible_size(csize * factor, b.len())?;
    let aligned = us(csize).saturating_add(7) & !7;
    let blocks = (crate::util::usize_to_f64(aligned) * factor / 239.0).ceil();
    let blocks = us(blocks);
    if at.saturating_add(blocks.saturating_mul(255)) > b.len() {
        return err(msg("ccdwg.open.r2007_past_end", &[("what", what)]));
    }
    let mut stats = Stats::default();
    let data = deinterleave(b, at, blocks, 239, true, Some(&mut stats));
    if stats.bad > 0 {
        notes.push(msg("ccdwg.read.uncorrectable", &[("what", what)]));
    }
    if csize < usize_ {
        return decompress_r21(&data, 0, us(csize), us(usize_));
    }
    Ok(data.get(..us(usize_).min(data.len())).map(<[u8]>::to_vec).unwrap_or_default())
}

/// 5.4 data pages.
fn read_data_section21(
    b: &[u8],
    pages: &HashMap<i64, (f64, f64)>,
    list: &[PageRef],
    size: f64,
    encoding: f64,
    name: &str,
    notes: &mut Vec<String>,
) -> Res<Vec<u8>> {
    plausible_size(size, b.len())?;
    let size = us(size);
    let mut out = vec![0u8; size];
    let mut bad = 0usize;
    for pg in list {
        let Some(&(loc_off, _)) = pages.get(&crate::util::to_i64(pg.id)) else {
            bad = bad.saturating_add(1);
            continue;
        };
        let at = 0x480usize.saturating_add(us(loc_off));
        let aligned = (us(pg.csize).saturating_add(7)) & !7;
        let blocks = aligned.div_ceil(251);
        let data = if encoding == 4.0 {
            deinterleave(b, at, blocks, 251, false, None)
        } else {
            let mut d = vec![0u8; blocks.saturating_mul(251)];
            for k in 0..blocks {
                for i in 0..251 {
                    if let Some(x) = d.get_mut(k.saturating_mul(251).saturating_add(i)) {
                        *x = b.get(at.saturating_add(k.saturating_mul(255).saturating_add(i))).copied().unwrap_or(0);
                    }
                }
            }
            d
        };
        plausible_size(pg.usize, b.len())?;
        let page = if pg.csize < pg.usize {
            decompress_r21(&data, 0, us(pg.csize), us(pg.usize))?
        } else {
            data.get(..us(pg.usize).min(data.len())).map(<[u8]>::to_vec).unwrap_or_default()
        };
        let start = us(pg.offset);
        for (i, &v) in page.iter().enumerate() {
            match out.get_mut(start.saturating_add(i)) {
                Some(x) => *x = v,
                None => break,
            }
        }
    }
    if bad > 0 {
        notes.push(msg("ccdwg.read.pages_missing", &[("n", &bad.to_string()), ("name", name)]));
    }
    Ok(out)
}
