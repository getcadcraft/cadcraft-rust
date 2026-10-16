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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tables() {
        assert_eq!(T16[1], 0xC0C1);
        assert_eq!(crc32(b"123456789", 0), 0xCBF4_3926);
        assert_eq!(r2004_magic(4), vec![0x29, 0x23, 0xBE, 0x84]);
    }
}
