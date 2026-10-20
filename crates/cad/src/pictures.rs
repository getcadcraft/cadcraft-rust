//! pictures.js (0.5 WL.14): the picture files a drawing's pictures use, kept with the drawing while it is open.
//!
//! A picture itself is an unnamed block "*I<n>" whose frame (a closed poly) carries `pic` (see cc-io-cad's
//! `small::picture_block`); this module is the store of the files (`doc.pictures`: key → record) and the sniffing of
//! PNG / JPEG headers. File bytes are hostile: only the headers are read here (the image is decoded by the window,
//! with limits), and a file over 64 MB or 100 million pixels is refused.

use std::sync::Arc;

use crate::omap::OMap;

/// The largest picture file taken.
pub const MAX_BYTES: usize = 64 * 1024 * 1024;
/// The most pixels a picture may have.
pub const MAX_PIXELS: f64 = 100e6;

/// One stored picture file: `{ name, bytes, mime, w, h }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PicRec {
    /// the file's base name as it was given
    pub name: String,
    pub bytes: Arc<[u8]>,
    pub mime: &'static str,
    pub w: u32,
    pub h: u32,
}

/// The store: key (the file's base name, lower case) → record, in the order they came.
pub type Pictures = OMap<String, Arc<PicRec>>;

/// `pictureKey(file)`: the key a picture file is known by: its base name, lower case ("C:\\Logos\\Logo.PNG" →
/// "logo.png").
pub fn picture_key(file: &str) -> String {
    base_name(file).to_lowercase()
}

/// The part after the last `\` or `/`.
pub fn base_name(file: &str) -> &str {
    file.rsplit(['\\', '/']).next().unwrap_or("")
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?]))
}

/// JPEG frame header (SOF0–SOF2) → (w, h, components).
pub fn jpeg_info(b: &[u8]) -> Option<(u32, u32, u8)> {
    let mut i: usize = 2;
    while i.checked_add(9).is_some_and(|e| e < b.len()) {
        if *b.get(i)? != 0xff {
            return None;
        }
        let m = *b.get(i.checked_add(1)?)?;
        if m == 0xd8 || (0xd0..=0xd7).contains(&m) || m == 0x01 {
            i = i.checked_add(2)?;
            continue;
        }
        let len = (usize::from(*b.get(i.checked_add(2)?)?) << 8) | usize::from(*b.get(i.checked_add(3)?)?);
        if (0xc0..=0xc2).contains(&m) {
            let h = (u32::from(*b.get(i.checked_add(5)?)?) << 8) | u32::from(*b.get(i.checked_add(6)?)?);
            let w = (u32::from(*b.get(i.checked_add(7)?)?) << 8) | u32::from(*b.get(i.checked_add(8)?)?);
            return Some((w, h, *b.get(i.checked_add(9)?)?));
        }
        if m == 0xda || len < 2 {
            return None;
        }
        i = i.checked_add(2)?.checked_add(len)?;
    }
    None
}

/// `sniffPicture(bytes)`: (mime, w, h) when the bytes are a PNG or a JPEG we can read (by content, not by name).
pub fn sniff_picture(b: &[u8]) -> Option<(&'static str, u32, u32)> {
    if b.len() > 24 && b.starts_with(&[0x89, 0x50, 0x4e, 0x47]) {
        let (w, h) = (be32(b, 16)?, be32(b, 20)?);
        return (w > 0 && h > 0 && f64::from(w) * f64::from(h) <= MAX_PIXELS).then_some(("image/png", w, h));
    }
    if b.len() > 4 && b.starts_with(&[0xff, 0xd8]) {
        let (w, h, _) = jpeg_info(b)?;
        return (f64::from(w) * f64::from(h) <= MAX_PIXELS).then_some(("image/jpeg", w, h));
    }
    None
}

/// `addPicture(doc, name, bytes)` on a store: keep a picture file → the record, or the catalog key of why not
/// (`pic.too_big`, `pic.not_picture`).
pub fn add_picture(store: &mut Pictures, name: &str, bytes: &[u8]) -> Result<Arc<PicRec>, &'static str> {
    if bytes.len() > MAX_BYTES {
        return Err("pic.too_big");
    }
    let Some((mime, w, h)) = sniff_picture(bytes) else { return Err("pic.not_picture") };
    let rec = Arc::new(PicRec { name: base_name(name).to_string(), bytes: Arc::from(bytes), mime, w, h });
    store.insert(picture_key(name), Arc::clone(&rec));
    Ok(rec)
}

/// `findPicture(doc, file)`: the stored file for a picture — the exact name, else a stored file whose name contains it
/// (exports prefix the project).
pub fn find_picture<'a>(store: &'a Pictures, file: &str) -> Option<&'a Arc<PicRec>> {
    let k = picture_key(file);
    if k.is_empty() {
        return None;
    }
    if let Some(r) = store.get(&k) {
        return Some(r);
    }
    let stem = strip_ext(&k);
    if stem.chars().count() < 3 {
        return None;
    }
    store.iter().find(|(key, _)| key.contains(stem)).map(|(_, r)| r)
}

/// `.replace(/\.[a-z0-9]+$/, '')` on a lower-case key.
pub fn strip_ext(k: &str) -> &str {
    match k.rfind('.') {
        Some(i) => {
            let ext = k.get(i.saturating_add(1)..).unwrap_or("");
            if !ext.is_empty() && ext.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) { k.get(..i).unwrap_or(k) } else { k }
        }
        None => k,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R'];
        b.extend(w.to_be_bytes());
        b.extend(h.to_be_bytes());
        b.extend([8, 6, 0, 0, 0]);
        b
    }

    #[test]
    fn sniffs_and_stores_by_base_name() {
        let mut s = Pictures::new();
        assert_eq!(sniff_picture(&png(3, 2)), Some(("image/png", 3, 2)));
        assert_eq!(sniff_picture(&png(0, 2)), None);
        assert_eq!(sniff_picture(&png(20000, 20000)), None);
        assert_eq!(add_picture(&mut s, "x.png", b"nope"), Err("pic.not_picture"));
        let r = add_picture(&mut s, "C:\\Logos\\Logo.PNG", &png(3, 2)).map(|r| r.name.clone());
        assert_eq!(r.as_deref(), Ok("Logo.PNG"));
        assert!(find_picture(&s, "d:/other/LOGO.png").is_some());
        assert!(find_picture(&s, "go.png").is_none());
        assert!(find_picture(&s, "Log.jpg").is_some());
    }

    #[test]
    fn jpeg_header_walk_is_bounded() {
        assert_eq!(jpeg_info(&[0xff, 0xd8, 0xff, 0xc0, 0, 17, 8, 0, 2, 0, 3, 3, 0, 0]), Some((3, 2, 3)));
        assert_eq!(jpeg_info(&[0xff, 0xd8, 0xff, 0xe0, 0xff, 0xff, 0, 0, 0, 0, 0, 0]), None);
        assert_eq!(jpeg_info(&[0xff, 0xd8]), None);
    }
}
