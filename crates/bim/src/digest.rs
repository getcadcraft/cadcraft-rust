//! Digests of large typed arrays for the parity harness: two FNV-1a 32-bit hashes over the little-endian bytes (the
//! harness computes the same over the JS's Float32Array / Uint32Array buffers), with the length.

use serde_json::{Map, Value as J};

const P: u32 = 16_777_619;

/// The digest of bytes → `{ n, h }` (h: 16 hex digits).
pub fn bytes_iter(n: usize, it: impl Iterator<Item = u8>) -> J {
    let mut h1: u32 = 0x811c_9dc5;
    let mut h2: u32 = 0x050c_5d1f;
    for b in it {
        h1 = (h1 ^ u32::from(b)).wrapping_mul(P);
        h2 = (h2 ^ u32::from(b)).wrapping_mul(P);
        h2 = h2.rotate_left(5);
    }
    let mut o = Map::new();
    o.insert("n".into(), J::from(n));
    o.insert("h".into(), J::String(format!("{h1:08x}{h2:08x}")));
    J::Object(o)
}

pub fn bytes(b: &[u8]) -> J {
    bytes_iter(b.len(), b.iter().copied())
}

pub fn f32s(v: &[f32]) -> J {
    bytes_iter(v.len(), v.iter().flat_map(|x| x.to_bits().to_le_bytes()))
}

pub fn u32s(v: &[u32]) -> J {
    bytes_iter(v.len(), v.iter().flat_map(|x| x.to_le_bytes()))
}
