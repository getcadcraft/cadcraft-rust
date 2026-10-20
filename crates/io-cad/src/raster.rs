//! Pictures in drawings (IMAGE, 0.5 WL.14): a picture file → RGBA pixels for the screen.
//!
//! The bytes are hostile (a file the person picked, or one found next to a drawing). Only PNG and JPEG are decoded
//! (sniffed by content, as pictures.js `sniffPicture`), through the `image` crate with `image::Limits`: at most 64 MB of
//! file, 100 million pixels (pictures.js's caps), 30 000 pixels a side and 512 MB of decoder allocations. A picture
//! larger than the screen can show is sampled down to `max_side` (the GPU's texture limit).

use std::io::Cursor;

use cc_cad::pictures::{MAX_BYTES, sniff_picture};

/// Decoded pixels: `w × h`, 4 bytes (RGBA, not premultiplied) each, row 0 at the top.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rgba {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

/// The largest side the decoder accepts.
pub const MAX_SIDE: u32 = 30_000;
/// The most the decoder may allocate.
pub const MAX_ALLOC: u64 = 512 * 1024 * 1024;

/// A PNG or JPEG → its pixels, at most `max_side` pixels a side (sampled down when larger). Errors are plain words.
pub fn decode(bytes: &[u8], max_side: u32) -> Result<Rgba, String> {
    if bytes.len() > MAX_BYTES {
        return Err("larger than 64 MB".into());
    }
    let Some((mime, _, _)) = sniff_picture(bytes) else { return Err("not a PNG or JPEG picture".into()) };
    let fmt = if mime == "image/png" { image::ImageFormat::Png } else { image::ImageFormat::Jpeg };
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_ALLOC);
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), fmt);
    reader.limits(limits);
    let img = reader.decode().map_err(|e| e.to_string())?;
    if f64::from(img.width()) * f64::from(img.height()) > cc_cad::pictures::MAX_PIXELS {
        return Err("more than 100 million pixels".into());
    }
    let side = max_side.clamp(1, MAX_SIDE);
    let img = if img.width() > side || img.height() > side { img.thumbnail(side, side) } else { img };
    let rgba = img.to_rgba8();
    Ok(Rgba { w: rgba.width(), h: rgba.height(), px: rgba.into_raw() })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// a 4 × 4 RGBA PNG
    const PNG4: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 4, 0, 0, 0, 4, 8, 6, 0, 0, 0, 169, 241, 158, 126, 0, 0, 0,
        25, 73, 68, 65, 84, 120, 156, 99, 96, 96, 56, 241, 255, 63, 195, 137, 255, 112, 26, 133, 3, 198, 132, 84, 0, 0, 196, 28, 36, 105,
        198, 78, 180, 214, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];

    #[test]
    fn decodes_png_and_samples_down() {
        let b = PNG4.to_vec();
        let r = decode(&b, 4096).unwrap();
        assert_eq!((r.w, r.h, r.px.len()), (4, 4, 64));
        let r = decode(&b, 2).unwrap();
        assert_eq!((r.w, r.h), (2, 2));
    }

    #[test]
    fn refuses_what_is_not_a_picture_or_too_big() {
        assert!(decode(b"hello", 100).is_err());
        // a PNG header claiming 30 001 × 1 pixels (over the side cap) with no data
        let mut b = PNG4.to_vec();
        b[16..20].copy_from_slice(&30_001u32.to_be_bytes());
        assert!(decode(&b, 100).is_err());
        // a damaged JPEG
        assert!(decode(&[0xff, 0xd8, 0xff, 0xc0, 0, 17, 8, 0, 2, 0, 3, 3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1, 0xff, 0xd9], 100).is_err());
    }
}
