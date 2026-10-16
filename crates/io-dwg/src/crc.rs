//! The checksums the reader checks (port of lib/dwg/crc.js, read side): the 16-bit table CRC the spec
//! calls "8-bit CRC" (2.14.1), CRC-32 (2.14.2), the R2004 page checksum (4.2) and the 4.1 magic sequence.

// CRC table builder, evaluated at compile time: i < 256 and k < 8, so the counters cannot overflow and an
// out-of-range index would fail the build rather than panic at run time.
#[allow(clippy::cast_possible_truncation, clippy::arithmetic_side_effects, clippy::indexing_slicing)]
const fn t16() -> [u16; 256] {
    let mut t = [0u16; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u16;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xA001 } else { c >> 1 };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}
// CRC table builder, evaluated at compile time: i < 256 and k < 8, so the counters cannot overflow and an
// out-of-range index would fail the build rather than panic at run time.
#[allow(clippy::cast_possible_truncation, clippy::arithmetic_side_effects, clippy::indexing_slicing)]
const fn t32() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}
static T16: [u16; 256] = t16();
static T32: [u32; 256] = t32();

pub const CRC8_SEED: u16 = 0xC0C1;

/// R13–R15 file header CRC XOR by locator count.
pub fn r13_header_xor(n: u32) -> u16 {
    match n {
        3 => 0xA598,
        4 => 0x8101,
        5 => 0x3CC4,
        6 => 0x8461,
        _ => 0,
    }
}

/// 2.14.1 over `bytes[from..to]` (out-of-range parts are skipped).
pub fn crc8(seed: u16, bytes: &[u8], from: usize, to: usize) -> u16 {
    let mut dx = seed;
    for &b in bytes.get(from..to.min(bytes.len())).unwrap_or(&[]) {
        dx = (dx >> 8) ^ T16.get(usize::from((u16::from(b) ^ dx) & 0xFF)).copied().unwrap_or(0);
    }
    dx
}

/// 2.14.2 CRC-32.
#[allow(clippy::cast_possible_truncation)] // masked to a byte
pub fn crc32(bytes: &[u8], seed: u32) -> u32 {
    let mut c = !seed;
    for &b in bytes {
        c = (c >> 8) ^ T32.get(((c ^ u32::from(b)) & 0xFF) as usize).copied().unwrap_or(0);
    }
    !c
}

/// 4.2 section page checksum (Adler-like) over `bytes[from..to]`.
pub fn page_checksum(seed: u32, bytes: &[u8], from: usize, to: usize) -> u32 {
    let mut s1 = seed & 0xFFFF;
    let mut s2 = (seed >> 16) & 0xFFFF;
    let data = bytes.get(from..to.min(bytes.len())).unwrap_or(&[]);
    for chunk in data.chunks(0x15B0) {
        for &b in chunk {
            s1 = s1.wrapping_add(u32::from(b));
            s2 = s2.wrapping_add(s1);
        }
        s1 %= 0xFFF1;
        s2 %= 0xFFF1;
    }
    (s2 << 16) | (s1 & 0xFFFF)
}

/// 4.1 magic sequence (randseed LCG): the file header mask.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // masked to a byte
pub fn r2004_magic(n: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(n);
    let mut s: i32 = 1;
    for _ in 0..n {
        s = s.wrapping_mul(0x343FD).wrapping_add(0x269EC3);
        out.push(((s >> 16) & 0xFF) as u8);
    }
    out
}

// ---------------------------------------------------------------- writer side (crc.js): R2007 checksums and CRCs

/// 5.4.1 R2007 data page checksum over `bytes[from..to]`.
pub fn r2007_page_checksum(seed: u32, bytes: &[u8], from: usize, to: usize) -> u32 {
    let data = bytes.get(from..to.min(bytes.len())).unwrap_or(&[]);
    let mut length = data.len();
    // seed = (seed + length) * 0x343fd + 0x269ec3 — 64-bit in the spec, only the low 32 bits are used
    let s = (u64::from(seed).wrapping_add(crate::util::usize_to_u64(length))).wrapping_mul(0x343FD).wrapping_add(0x269EC3);
    let mut s1 = crate::util::low_u32(s & 0xFFFF);
    let mut s2 = crate::util::low_u32((s >> 16) & 0xFFFF);
    let at = |k: usize| u32::from(data.get(k).copied().unwrap_or(0));
    let mut p = 0usize;
    let one = |k: usize, s1: &mut u32, s2: &mut u32| {
        *s1 = s1.wrapping_add(at(k));
        *s2 = s2.wrapping_add(*s1);
    };
    while length != 0 {
        let big = length.min(0x15B0);
        length = length.wrapping_sub(big); // big <= length
        let mut small = big >> 3;
        while small > 0 {
            small = small.wrapping_sub(1);
            for q in [6usize, 7, 4, 5, 2, 3, 0, 1] {
                one(p.wrapping_add(q), &mut s1, &mut s2);
            }
            p = p.wrapping_add(8);
        }
        let rem = big & 7;
        let order: &[usize] = match rem {
            1 => &[0],
            2 => &[0, 1],
            3 => &[0, 1, 2],
            4 => &[2, 3, 0, 1],
            5 => &[2, 3, 0, 1, 4],
            6 => &[2, 3, 0, 1, 4, 5],
            7 => &[2, 3, 0, 1, 4, 5, 6],
            _ => &[],
        };
        for &q in order {
            one(p.wrapping_add(q), &mut s1, &mut s2);
        }
        p = p.wrapping_add(rem);
        s1 %= 0xFFF1;
        s2 %= 0xFFF1;
    }
    (s2 << 16) | (s1 & 0xFFFF)
}

// 64-bit CRC tables (5.12), evaluated at compile time (i < 256, k < 8).
#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
const fn t64n() -> [u64; 256] {
    let p: u64 = 0x42F0_E1EB_A9EA_3693;
    let mut t = [0u64; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = (i as u64) << 56;
        let mut k = 0;
        while k < 8 {
            c = if c & (1u64 << 63) != 0 { (c << 1) ^ p } else { c << 1 };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}
#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
const fn t64m() -> [u64; 256] {
    let p: u64 = 0x95AC_9329_AC4B_C9B5;
    let mut t = [0u64; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u64;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { (c >> 1) ^ p } else { c >> 1 };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}
static T64N: [u64; 256] = t64n();
static T64M: [u64; 256] = t64m();

fn crc64(bytes: &[u8], seed: u64, mirrored: bool) -> u64 {
    let mut c = seed;
    let mut step = |b: u8| {
        if mirrored {
            c = T64M.get(usize::from(crate::util::u8_of(crate::util::low_u32(c ^ u64::from(b))))).copied().unwrap_or(0) ^ (c >> 8);
        } else {
            c = T64N.get(usize::from(crate::util::u8_of(crate::util::low_u32(u64::from(b) ^ (c >> 56))))).copied().unwrap_or(0) ^ (c << 8);
        }
    };
    let order = |n: usize| -> &'static [usize] {
        match n {
            1 => &[0],
            2 => &[0, 1],
            3 => &[0, 1, 2],
            4 => &[2, 3, 0, 1],
            5 => &[2, 3, 0, 1, 4],
            6 => &[2, 3, 0, 1, 4, 5],
            7 => &[2, 3, 0, 1, 4, 5, 6],
            _ => &[6, 7, 4, 5, 2, 3, 0, 1],
        }
    };
    let (chunks, rest) = bytes.as_chunks::<8>();
    for ch in chunks {
        for &k in order(8) {
            step(ch.get(k).copied().unwrap_or(0));
        }
    }
    if !rest.is_empty() {
        for &k in order(rest.len()) {
            step(rest.get(k).copied().unwrap_or(0));
        }
    }
    if mirrored { c } else { !c }
}
/// 5.12.1 the normal 64-bit CRC (with the spec's byte order).
pub fn crc64_normal(bytes: &[u8], seed: u64) -> u64 {
    crc64(bytes, seed, false)
}
/// 5.12.2 the mirrored 64-bit CRC.
pub fn crc64_mirrored(bytes: &[u8], seed: u64) -> u64 {
    crc64(bytes, seed, true)
}
/// 5.12 seed updater 1.
pub fn update_seed1(seed: u64, len: u64) -> u64 {
    let s = seed.wrapping_add(len).wrapping_mul(0x343FD).wrapping_add(0x269EC3);
    let s = s | s.wrapping_mul(0x343FDu64 << 32).wrapping_add(0x269EC3u64 << 32);
    !s
}
/// 5.12 seed updater 2.
pub fn update_seed2(seed: u64, len: u64) -> u64 {
    let s = seed.wrapping_add(len).wrapping_mul(0x343FD).wrapping_add(0x269EC3);
    let s = s.wrapping_mul((1u64 << 32).wrapping_add(0x343FD)).wrapping_add(len.wrapping_add(0x269EC3));
    !s
}

/// 5.11 CRC random encoding (crc.js CrcRandom).
#[derive(Clone, Debug)]
pub struct CrcRandom {
    table: Vec<u32>,
    index: usize,
    /// The 512 padding bytes (little-endian u32s).
    pub padding: Vec<u8>,
}
impl CrcRandom {
    pub fn new(seed: u64) -> Self {
        let mut t = vec![0u32; 0x270];
        let lo = crate::util::low_u32(seed);
        let hi = crate::util::low_u32(seed >> 32);
        let mut v = hi.wrapping_mul(0x343FD).wrapping_add(0x269EC3);
        if let Some(x) = t.get_mut(0) {
            *x = lo.wrapping_mul(0x343FD).wrapping_add(0x269EC3);
        }
        if let Some(x) = t.get_mut(1) {
            *x = v;
        }
        for i in 2..0x270u32 {
            v = ((v >> 30) ^ v).wrapping_mul(0x6C07_8965).wrapping_add(i);
            if let Some(x) = t.get_mut(crate::util::usize_from_u32(i)) {
                *x = v;
            }
        }
        let mut index = 0usize;
        let mut padding = Vec::with_capacity(0x200);
        for _ in 0..0x80 {
            if index >= 0x270 {
                index = 0;
            }
            padding.extend_from_slice(&t.get(index).copied().unwrap_or(0).to_le_bytes());
            index = index.wrapping_add(1);
        }
        CrcRandom { table: t, index, padding }
    }
    pub fn next64(&mut self) -> u64 {
        self.index = self.index.wrapping_add(2);
        if self.index >= 0x270 {
            self.index = 0;
        }
        let lo = self.table.get(self.index).copied().unwrap_or(0);
        let hi = self.table.get(self.index.wrapping_add(1) % 0x270).copied().unwrap_or(0);
        (u64::from(hi) << 32) | u64::from(lo)
    }
    pub fn encode(&mut self, value: u32) -> u64 {
        let r = self.next64();
        let mut lo = crate::util::low_u32(r & 0xDF7D_F7DF);
        let mut hi = crate::util::low_u32((r >> 32) & 0xF7DF_7DF7);
        for (bit, set) in [(0x200, 0x20), (0x100, 0x800), (0x80, 0x20000), (0x40, 0x80_0000), (0x20, 0x2000_0000)] {
            if value & bit != 0 {
                lo |= set;
            }
        }
        for (bit, set) in [(0x10, 0x08), (0x8, 0x200), (0x4, 0x8000), (0x2, 0x20_0000), (0x1, 0x800_0000)] {
            if value & bit != 0 {
                hi |= set;
            }
        }
        (u64::from(hi) << 32) | u64::from(lo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tables() {
        assert_eq!(T16[1], 0xC0C1);
        assert_eq!(crc32(b"123456789", 0), 0xCBF4_3926);
        assert_eq!(r2004_magic(4), vec![0x29, 0x23, 0xBE, 0x84]);
    }
    #[test]
    fn writer_crcs() {
        // the tables printed in 5.12.1 / 5.12.2 start so
        assert_eq!(T64N[1], 0x42F0_E1EB_A9EA_3693);
        assert_eq!(T64M[1], 0x7AD8_70C8_3035_8979);
        let b = [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
        assert_eq!(crc64_normal(&b, 5), 0x2b02_1860_45fa_c7b5);
        assert_eq!(crc64_mirrored(&b, 5), 0x63f3_cb71_be48_b6b9);
        assert_eq!(update_seed1(7, 11), 0xee19_20e3_ff9e_9972);
        assert_eq!(update_seed2(7, 11), 0xff9e_9834_ee19_20d8);
        let mut r = CrcRandom::new(0x5A17_C0DE);
        assert_eq!(r.next64(), 0x5053_1e1a_b537_1949);
        assert_eq!(r.encode(0), 0xe105_2d17_4224_540b);
        assert_eq!(r.encode(1023), 0xf973_e759_2bfb_cf6a);
        assert_eq!(r.padding[..8], [0x29, 0x54, 0xb4, 0x83, 0xc3, 0x9e, 0x26, 0x00]);
        let big: Vec<u8> = (0..6000u32).map(|i| crate::util::u8_of(i * 7)).collect();
        assert_eq!(r2007_page_checksum(0, &big, 0, big.len()), 0x0178_c490);
        assert_eq!(r2007_page_checksum(0, &[], 0, 0), 0x0026_9ec3);
        assert_eq!(r2007_page_checksum(3, &b, 0, b.len()), 0xba82_56db);
    }
}
