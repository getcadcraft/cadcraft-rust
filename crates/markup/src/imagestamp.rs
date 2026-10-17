//! imagestamp.js — image stamps and signatures: `imageInfo` (PNG / JPEG only, header-checked, ≤ 3 MB),
//! `cleanImage`, `fitBox`, `placeRect`.

pub const MAX_IMAGE_BYTES: usize = 3 * 1024 * 1024;

/// What `imageInfo` reads.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageInfo {
    pub mime: &'static str,
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// `^data:(image\/(?:png|jpeg));base64,([A-Za-z0-9+/]+={0,2})$` → (mime, base64 text).
fn split_data_url(src: &str) -> Option<(&'static str, &str)> {
    let (mime, rest) = if let Some(r) = src.strip_prefix("data:image/png;base64,") {
        ("image/png", r)
    } else {
        let r = src.strip_prefix("data:image/jpeg;base64,")?;
        ("image/jpeg", r)
    };
    let body = rest.trim_end_matches('=');
    let pad = rest.len().saturating_sub(body.len());
    if body.is_empty() || pad > 2 || !body.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/') {
        return None;
    }
    Some((mime, rest))
}

/// Base64 (standard alphabet) like Node's `Buffer.from(b64, 'base64')` / `atob` on valid text.
pub fn b64_decode(s: &str) -> Vec<u8> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => u32::from(c.wrapping_sub(b'A')),
            b'a'..=b'z' => u32::from(c.wrapping_sub(b'a')).wrapping_add(26),
            b'0'..=b'9' => u32::from(c.wrapping_sub(b'0')).wrapping_add(52),
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    };
    let mut out = Vec::with_capacity((s.len() / 4).saturating_mul(3).saturating_add(3));
    let mut acc = 0u32;
    let mut bits = 0u32;
    for c in s.bytes() {
        let Some(v) = val(c) else { continue };
        acc = (acc << 6) | v;
        bits = bits.wrapping_add(6);
        if bits >= 8 {
            bits = bits.wrapping_sub(8);
            out.push(((acc >> bits) & 0xff).to_le_bytes()[0]);
        }
    }
    out
}

/// Base64 encode (standard alphabet, padded).
pub fn b64_encode(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(b.len().div_ceil(3).saturating_mul(4));
    for ch in b.chunks(3) {
        let n = ch.len();
        let x = (u32::from(ch.first().copied().unwrap_or(0)) << 16)
            | (u32::from(ch.get(1).copied().unwrap_or(0)) << 8)
            | u32::from(ch.get(2).copied().unwrap_or(0));
        for (k, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if k <= n {
                let i = usize::try_from((x >> shift) & 63).unwrap_or(0);
                out.push(char::from(A.get(i).copied().unwrap_or(b'A')));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `imageInfo(src)`.
pub fn image_info(src: &str) -> Option<ImageInfo> {
    let (mime, b64) = split_data_url(src)?;
    // (m[2].length * 3) / 4 > MAX_IMAGE_BYTES
    if (b64.len() as f64 * 3.0) / 4.0 > MAX_IMAGE_BYTES as f64 {
        return None;
    }
    let bytes = b64_decode(b64);
    let at = |i: usize| bytes.get(i).copied();
    let a = |i: usize, k: usize| at(i.saturating_add(k)).unwrap_or(0);
    if mime == "image/png" {
        if bytes.len() < 24 || at(0) != Some(0x89) || at(1) != Some(0x50) || at(2) != Some(0x4e) || at(3) != Some(0x47) {
            return None;
        }
        let be = |i: usize| -> u32 { u32::from_be_bytes([a(i, 0), a(i, 1), a(i, 2), a(i, 3)]) };
        let (width, height) = (be(16), be(20));
        return Some(ImageInfo { mime, bytes, width, height });
    }
    if at(0) != Some(0xff) || at(1) != Some(0xd8) {
        return None;
    }
    let mut p = 2usize;
    while p.saturating_add(9) < bytes.len() {
        if at(p) != Some(0xff) {
            p = p.saturating_add(1);
            continue;
        }
        let mk = a(p, 1);
        if (0xc0..=0xcf).contains(&mk) && mk != 0xc4 && mk != 0xc8 && mk != 0xcc {
            let height = u32::from(u16::from_be_bytes([a(p, 5), a(p, 6)]));
            let width = u32::from(u16::from_be_bytes([a(p, 7), a(p, 8)]));
            return Some(ImageInfo { mime, bytes, width, height });
        }
        let len = usize::from(u16::from_be_bytes([a(p, 2), a(p, 3)]));
        p = p.saturating_add(2).saturating_add(len);
    }
    None
}

/// `cleanImage(src)` → the data URL, or '' when it is not an acceptable image.
pub fn clean_image(src: &str) -> String {
    match image_info(src) {
        Some(i) if i.width > 0 && i.height > 0 && i.width <= 8000 && i.height <= 8000 => src.to_string(),
        _ => String::new(),
    }
}

/// `cleanImage(v)` for an untrusted JSON value (`String(src || '')`).
pub fn clean_image_value(v: Option<&serde_json::Value>) -> String {
    if crate::js::truthy(v) { clean_image(&v.map(crate::js::to_js_string).unwrap_or_default()) } else { String::new() }
}

/// `fitBox(w, h, maxW, maxH)`.
pub fn fit_box(w: f64, h: f64, max_w: f64, max_h: f64) -> [f64; 2] {
    let k = crate::js::min(crate::js::min(max_w / w, max_h / h), 1e9);
    [w * k, h * k]
}

/// `placeRect(c, w, h, rot)`.
pub fn place_rect(c: [f64; 2], w: f64, h: f64, rot: f64) -> [f64; 4] {
    let (dx, dy) = if rot % 180.0 != 0.0 && !(rot % 180.0).is_nan() { (h, w) } else { (w, h) };
    [c[0] - dx / 2.0, c[1] - dy / 2.0, c[0] + dx / 2.0, c[1] + dy / 2.0]
}
