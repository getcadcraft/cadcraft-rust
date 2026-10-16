//! Small helpers shared by the reader: base64, bit slices, hex, and the numeric conversions the
//! JavaScript reader gets for free (every cast lives here, checked, so the parser modules need none).

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap, clippy::cast_precision_loss)]

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding (Node's `Buffer.toString('base64')`).
pub fn b64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3).saturating_mul(4));
    for chunk in data.chunks(3) {
        let b0 = u32::from(chunk.first().copied().unwrap_or(0));
        let b1 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b2 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let n = (b0 << 16) | (b1 << 8) | b2;
        let ch = |s: u32| char::from(B64.get(((n >> s) & 63) as usize).copied().unwrap_or(b'A'));
        out.push(ch(18));
        out.push(ch(12));
        out.push(if chunk.len() > 1 { ch(6) } else { '=' });
        out.push(if chunk.len() > 2 { ch(0) } else { '=' });
    }
    out
}

/// Decode base64 (lenient: skips characters outside the alphabet, like Node).
pub fn unb64(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len().div_euclid(4).saturating_mul(3));
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for c in s.bytes() {
        let v = match c {
            // the match arms bound c, so these stay inside 0..64
            b'A'..=b'Z' => c.wrapping_sub(b'A'),
            b'a'..=b'z' => c.wrapping_sub(b'a').wrapping_add(26),
            b'0'..=b'9' => c.wrapping_sub(b'0').wrapping_add(52),
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => continue,
        };
        acc = (acc << 6) | u32::from(v);
        // bits stays in 0..14
        bits = bits.wrapping_add(6);
        if bits >= 8 {
            bits = bits.wrapping_sub(8);
            out.push(((acc >> bits) & 0xFF) as u8);
        }
    }
    out
}

/// Bits [from, to) of `buf` as bytes starting at bit 0 (MSB first), the last byte's unused bits zero
/// (bits-util.js sliceBits).
pub fn slice_bits(buf: &[u8], from: usize, to: usize) -> Vec<u8> {
    let n = to.saturating_sub(from);
    let len = n.div_ceil(8);
    let mut out = vec![0u8; len];
    let sh = (from & 7) as u32;
    let b0 = from >> 3;
    for (i, o) in out.iter_mut().enumerate() {
        let a = u32::from(buf.get(b0.saturating_add(i)).copied().unwrap_or(0));
        if sh == 0 {
            *o = a as u8;
        } else {
            let b = u32::from(buf.get(b0.saturating_add(i).saturating_add(1)).copied().unwrap_or(0));
            *o = (((a << sh) | (b >> (8u32.wrapping_sub(sh)))) & 0xFF) as u8; // sh in 1..8
        }
    }
    if n & 7 != 0
        && let Some(last) = out.last_mut()
    {
        *last &= ((0xFFu32 << (8u32.wrapping_sub((n & 7) as u32))) & 0xFF) as u8; // n & 7 in 1..8
    }
    out
}

/// Uppercase hex of bytes, two digits each.
pub fn hex_bytes(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len().saturating_mul(2));
    for x in b {
        s.push_str(&format!("{x:02X}"));
    }
    s
}

/// JS `n.toString(16).toUpperCase()` for an integer-valued number (negative keeps its sign).
pub fn hex_of(n: f64) -> String {
    if !n.is_finite() {
        return "NAN".into();
    }
    let neg = n < 0.0;
    let a = n.abs();
    let s = if a < 18446744073709551615.0 { format!("{:X}", a as u64) } else { format!("{a}") };
    if neg { format!("-{s}") } else { s }
}

/// read.js handleHex: 0 → '', else uppercase hex.
pub fn handle_hex(n: f64) -> String {
    if n == 0.0 || n.is_nan() { String::new() } else { hex_of(n) }
}

/// parseInt(hex, 16) for handle strings ('' → 0; stops at the first non-hex digit; NaN → 0).
pub fn handle_num(h: &str) -> f64 {
    let mut v = 0.0f64;
    let mut any = false;
    let (neg, digits) = match h.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, h),
    };
    for c in digits.chars() {
        match c.to_digit(16) {
            Some(d) => {
                v = v * 16.0 + f64::from(d);
                any = true;
            }
            None => break,
        }
    }
    if !any {
        return 0.0;
    }
    if neg { -v } else { v }
}

/// f64 → i64 the way the JS reader uses numbers as map keys (truncating; NaN → 0; saturating).
pub fn to_i64(n: f64) -> i64 {
    n as i64
}
/// f64 → usize for counts and indexes (negative / NaN → 0).
pub fn to_usize(n: f64) -> usize {
    if n > 0.0 { n as usize } else { 0 }
}
pub fn i64_to_f64(n: i64) -> f64 {
    n as f64
}
pub fn usize_to_f64(n: usize) -> f64 {
    n as f64
}
/// JS `x | 0` (ToInt32).
pub fn to_i32(n: f64) -> i32 {
    if !n.is_finite() {
        return 0;
    }
    let m = n.trunc() % 4294967296.0;
    let u = if m < 0.0 { m + 4294967296.0 } else { m } as u64 as u32;
    u as i32
}
/// JS `x >>> 0` (ToUint32).
pub fn to_u32(n: f64) -> u32 {
    to_i32(n) as u32
}
pub fn u32_from_i32(n: i32) -> u32 {
    n as u32
}
pub fn i32_from_u32(n: u32) -> i32 {
    n as i32
}
pub fn usize_from_u32(n: u32) -> usize {
    n as usize
}
pub fn usize_from_i32(n: i32) -> usize {
    if n > 0 { n as usize } else { 0 }
}
pub fn usize_from_u64(n: u64) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}
pub fn u8_of(n: u32) -> u8 {
    (n & 0xFF) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn base64_round_trip() {
        for s in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar"] {
            assert_eq!(unb64(&b64(s)), s);
        }
        assert_eq!(b64(b"foobar"), "Zm9vYmFy");
        assert_eq!(b64(b"fo"), "Zm8=");
    }
    #[test]
    fn bits_and_hex() {
        assert_eq!(slice_bits(&[0b1010_1010, 0b1100_0000], 1, 10), vec![0b0101_0101, 0b1000_0000]);
        assert_eq!(handle_hex(0.0), "");
        assert_eq!(handle_hex(255.0), "FF");
        assert_eq!(handle_num("1F"), 31.0);
        assert_eq!(to_i32(4294967295.0), -1);
        assert_eq!(to_u32(-1.0), 4294967295);
    }
}

/// A fast hasher for integer keys (handles): a multiply-rotate mix, not DoS-resistant (keys come from one file
/// and only shape performance, never correctness).
#[derive(Default, Clone, Copy, Debug)]
pub struct IntHasher(u64);
impl std::hash::Hasher for IntHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0.rotate_left(5) ^ u64::from(b)).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
    }
    fn write_i64(&mut self, n: i64) {
        self.0 = (n as u64 ^ self.0).wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(29);
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = (n ^ self.0).wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(29);
    }
    fn write_usize(&mut self, n: usize) {
        self.write_u64(n as u64);
    }
}
pub type IntMap<K, V> = std::collections::HashMap<K, V, std::hash::BuildHasherDefault<IntHasher>>;
pub type IntSet<K> = std::collections::HashSet<K, std::hash::BuildHasherDefault<IntHasher>>;
