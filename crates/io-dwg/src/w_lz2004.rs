//! The R2004-family LZ77 compressor (port of lib/dwg/lz-r2004-write.js; ODA spec 4.7). Used for R2004, R2010,
//! R2013 and R2018 sections. See the JS for the opcode table and its "Spec gap:" notes:
//!
//! Spec gap: the "compOffset" the opcodes carry is the back distance minus one.
//! Spec gap: a Literal Length byte 0x01–0x0F means value + 3 (so 0x0F = 18), the 0x00 form covers 19 and up.
//! Spec gap: opcodes 0x10–0x1F are one family: length = (op & 7) + 2, op & 7 = 0 meaning "Long Compression Offset
//!   + 7", and op & 8 adds 0x4000 to the offset; 0x11 is the terminator.

const MIN_MATCH: usize = 3;
const MAX_DIST: usize = 0x3FFF + 0x3FFF + 1;
const HASH_BITS: u32 = 15;
const MAX_CHAIN: usize = 64;

/// The JS hash: `((a << 16 | b << 8 | c) * 2654435761) >>> (32 - bits)` with the product as a double.
#[inline]
pub fn js_hash(src: &[u8], i: usize, bits: u32) -> usize {
    let a = u32::from(src.get(i).copied().unwrap_or(0));
    let b = u32::from(src.get(i.saturating_add(1)).copied().unwrap_or(0));
    let c = u32::from(src.get(i.saturating_add(2)).copied().unwrap_or(0));
    let x = (a << 16) | (b << 8) | c;
    let p = f64::from(x) * 2_654_435_761.0; // < 2^56: an integer-valued double, rounded as JS rounds it
    let low = crate::util::low_u32(crate::util::f64_to_u64(p));
    crate::util::usize_from_u32(low >> (32u32.wrapping_sub(bits)))
}

/// Compress `src`. `literal_only` writes one literal run.
pub fn compress_r2004(src: &[u8], literal_only: bool) -> Vec<u8> {
    let n = src.len();
    if n < 4 {
        // the first literal run must be at least 4 bytes long: pad with zeros (readers take the logical size)
        let mut p = src.to_vec();
        p.resize(4, 0);
        return compress_r2004(&p, literal_only);
    }
    let mut out = Out { buf: Vec::with_capacity(n.saturating_add(n >> 3).saturating_add(64)) };
    if literal_only {
        out.literal_length(n);
        out.buf.extend_from_slice(src);
        out.buf.push(0x11);
        return out.buf;
    }
    let mut head = vec![-1i64; 1usize << HASH_BITS];
    let mut prev = vec![-1i64; n];
    let insert = |i: usize, head: &mut Vec<i64>, prev: &mut Vec<i64>| {
        if i.saturating_add(2) < n {
            let h = js_hash(src, i, HASH_BITS);
            if let (Some(p), Some(hh)) = (prev.get_mut(i), head.get(h)) {
                *p = *hh;
            }
            if let Some(hh) = head.get_mut(h) {
                *hh = i64::try_from(i).unwrap_or(-1);
            }
        }
    };
    let best = |pos: usize, head: &Vec<i64>, prev: &Vec<i64>| -> Option<(usize, usize)> {
        if pos.saturating_add(MIN_MATCH) > n {
            return None;
        }
        let mut cand = head.get(js_hash(src, pos, HASH_BITS)).copied().unwrap_or(-1);
        let (mut best_len, mut best_dist, mut chain) = (0usize, 0usize, 0usize);
        let max_len = n.saturating_sub(pos);
        while cand >= 0 && chain < MAX_CHAIN {
            chain = chain.saturating_add(1);
            let c = usize::try_from(cand).unwrap_or(0);
            let dist = pos.saturating_sub(c);
            if dist > MAX_DIST {
                break;
            }
            if src.get(c.saturating_add(best_len)) == src.get(pos.saturating_add(best_len)) && src.get(c) == src.get(pos) {
                let l = common_len(src, c, pos, max_len);
                if l > best_len && encodable(l, dist) {
                    best_len = l;
                    best_dist = dist;
                    if l == max_len {
                        break;
                    }
                }
            }
            cand = prev.get(c).copied().unwrap_or(-1);
        }
        if best_len >= MIN_MATCH { Some((best_len, best_dist)) } else { None }
    };
    let mut tokens: Vec<(usize, usize, usize)> = Vec::new();
    let mut i = 4usize; // the first 4 bytes are always literal
    for k in 0..4usize.min(n) {
        insert(k, &mut head, &mut prev);
    }
    while i < n {
        let Some(m) = best(i, &head, &prev) else {
            insert(i, &mut head, &mut prev);
            i = i.saturating_add(1);
            continue;
        };
        insert(i, &mut head, &mut prev);
        let m2 = if i.saturating_add(1) < n { best(i.saturating_add(1), &head, &prev) } else { None };
        let (mut at, mut len, mut dist) = (i, m.0, m.1);
        if let Some(m2) = m2
            && m2.0 > m.0
        {
            at = i.saturating_add(1);
            len = m2.0;
            dist = m2.1;
            insert(at, &mut head, &mut prev);
        }
        tokens.push((at, len, dist));
        for k in at.saturating_add(1)..at.saturating_add(len) {
            insert(k, &mut head, &mut prev);
        }
        i = at.saturating_add(len);
    }
    // emit
    let first = tokens.first().map_or(n, |t| t.0);
    out.literal_length(first);
    out.buf.extend_from_slice(src.get(..first).unwrap_or(&[]));
    for (t, &(start, len, dist)) in tokens.iter().enumerate() {
        let end = start.saturating_add(len);
        let next_start = tokens.get(t.saturating_add(1)).map_or(n, |x| x.0);
        let lit_count = next_start.saturating_sub(end);
        out.matched(len, dist, lit_count);
        if lit_count > 3 {
            out.literal_length(lit_count);
        }
        out.buf.extend_from_slice(src.get(end..next_start).unwrap_or(&[]));
    }
    out.buf.push(0x11);
    out.buf
}

/// The length of the common run of `src[a..]` and `src[b..]`, at most `max`.
#[inline]
pub fn common_len(src: &[u8], a: usize, b: usize, max: usize) -> usize {
    let x = src.get(a..).unwrap_or(&[]);
    let y = src.get(b..).unwrap_or(&[]);
    x.iter().zip(y).take(max).take_while(|(p, q)| p == q).count()
}

fn encodable(len: usize, dist: usize) -> bool {
    let co = dist.saturating_sub(1);
    if co <= 0x3FFF {
        return len >= 3;
    }
    if co <= 0x3FFF + 0x3FFF {
        return len >= 4;
    }
    false
}

struct Out {
    buf: Vec<u8>,
}
#[allow(clippy::cast_possible_truncation)] // every value below is masked or bounded to a byte first
impl Out {
    fn byte(&mut self, v: usize) {
        self.buf.push((v & 0xFF) as u8);
    }
    /// Literal Length (≥ 4; a shorter run cannot occur: the first run is ≥ 4, later ones ≤ 3 ride in the opcode).
    fn literal_length(&mut self, v: usize) {
        if v == 0 {
            return;
        }
        if v <= 18 {
            self.byte(v.saturating_sub(3));
            return;
        }
        self.byte(0);
        let mut r = v.saturating_sub(3 + 0x0F);
        while r > 0xFF {
            self.byte(0);
            r = r.saturating_sub(0xFF);
        }
        self.byte(r);
    }
    fn long_value(&mut self, v: usize) {
        if v <= 0xFF {
            self.byte(v);
            return;
        }
        self.byte(0);
        let mut r = v.saturating_sub(0xFF);
        while r > 0xFF {
            self.byte(0);
            r = r.saturating_sub(0xFF);
        }
        self.byte(r);
    }
    fn two_byte(&mut self, off: usize, lit_code: usize) {
        self.byte(((off & 0x3F) << 2) | lit_code);
        self.byte(off >> 6);
    }
    fn matched(&mut self, len: usize, dist: usize, lit_count: usize) {
        let lc = if lit_count <= 3 { lit_count } else { 0 };
        let co = dist.saturating_sub(1);
        if co <= 0x3FF && len <= 14 {
            self.byte((len.saturating_add(1) << 4) | ((co & 3) << 2) | lc);
            self.byte(co >> 2);
        } else if co <= 0x3FFF {
            if len <= 33 {
                self.byte(0x1E_usize.saturating_add(len));
            } else {
                self.byte(0x20);
                self.long_value(len.saturating_sub(0x21));
            }
            self.two_byte(co, lc);
        } else {
            let mut c2 = co.saturating_sub(0x3FFF);
            let mut hi = 0;
            if c2 > 0x3FFF {
                c2 = c2.saturating_sub(0x4000);
                hi = 8;
            }
            if len <= 9 {
                self.byte(0x10 | hi | len.saturating_sub(2));
            } else {
                self.byte(0x10 | hi);
                self.long_value(len.saturating_sub(9));
            }
            self.two_byte(c2, lc);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_with_the_reader() {
        let mut src = Vec::new();
        for i in 0..20000u32 {
            src.push(crate::util::u8_of((i * 31) % 7 + (i / 100) % 5));
        }
        let z = compress_r2004(&src, false);
        let back = crate::lz2004::decompress_r2004(&z, 0, z.len(), src.len()).expect("decompresses");
        assert_eq!(back, src);
    }
}
