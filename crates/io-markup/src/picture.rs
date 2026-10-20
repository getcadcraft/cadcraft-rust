//! Pictures for MarkupCraft's picture stamps and reply photos (markupcraft.js `pictureFrom` and the signature pad,
//! 0.4 K5 / K20): a picked file is hostile input. Only PNG and JPEG are taken (by their magic bytes); the header is read
//! first under `image::Limits`, then the picture is decoded under the same limits, so a forged size cannot make a huge
//! allocation. A picture at most 1600 px on its longer side (and small enough for `cleanImage`) is kept byte for byte
//! (no loss; the JS re-encodes it through a canvas, which no other program can reproduce); a bigger one is scaled to
//! 1600 px and written again (JPEG stays JPEG at quality 90, anything else PNG), as the JS does. The result is a data URL
//! that passes `cleanImage`. The signature pad's strokes become a transparent PNG trimmed to the ink.

use std::io::Cursor;

use ::image::{ImageFormat, ImageReader, Limits};
use cc_markup::imagestamp::{MAX_IMAGE_BYTES, b64_decode, b64_encode, clean_image};

/// The longer side a picture is scaled to (markupcraft.js `pictureFrom`).
pub const MAX_SIDE: u32 = 1600;
/// The biggest file read as a picture.
pub const MAX_FILE: usize = 64 * 1024 * 1024;
/// Header sizes above this are refused before decoding.
const MAX_DIM: u32 = 16_384;
/// The most memory a decode may take.
const MAX_ALLOC: u64 = 512 * 1024 * 1024;

fn limits() -> Limits {
    let mut l = Limits::default();
    l.max_image_width = Some(MAX_DIM);
    l.max_image_height = Some(MAX_DIM);
    l.max_alloc = Some(MAX_ALLOC);
    l
}

/// The format of a picture by its magic bytes (PNG or JPEG only).
fn format_of(b: &[u8]) -> Option<ImageFormat> {
    if b.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        Some(ImageFormat::Png)
    } else if b.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(ImageFormat::Jpeg)
    } else {
        None
    }
}

/// `round(v * k)` as a pixel count of at least 1 (v ≤ MAX_DIM, 0 < k ≤ 1).
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped to 1..=MAX_DIM first
fn scaled(v: u32, k: f64) -> u32 {
    (f64::from(v) * k).round().clamp(1.0, f64::from(MAX_DIM)) as u32
}

/// A picked file → a PNG or JPEG data URL for a stamp or a photo, or the reason it cannot be one.
pub fn picture_from(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > MAX_FILE {
        return Err("too big".into());
    }
    let fmt = format_of(bytes).ok_or("not a PNG or JPEG")?;
    let mut reader = ImageReader::with_format(Cursor::new(bytes), fmt);
    reader.limits(limits());
    let (w, h) = reader.into_dimensions().map_err(|e| e.to_string())?;
    if w == 0 || h == 0 {
        return Err("empty picture".into());
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), fmt);
    reader.limits(limits());
    let img = reader.decode().map_err(|e| e.to_string())?;
    let mime = if fmt == ImageFormat::Jpeg { "image/jpeg" } else { "image/png" };
    if w.max(h) <= MAX_SIDE && bytes.len() <= MAX_IMAGE_BYTES {
        let url = clean_image(&format!("data:{mime};base64,{}", b64_encode(bytes)));
        if !url.is_empty() {
            return Ok(url);
        }
    }
    let k = (f64::from(MAX_SIDE) / f64::from(w.max(h))).min(1.0);
    let small = img.resize_exact(scaled(w, k), scaled(h, k), ::image::imageops::FilterType::Triangle);
    let mut out = Cursor::new(Vec::new());
    if fmt == ImageFormat::Jpeg {
        let enc = ::image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90);
        small.to_rgb8().write_with_encoder(enc).map_err(|e| e.to_string())?;
    } else {
        small.to_rgba8().write_to(&mut out, ImageFormat::Png).map_err(|e| e.to_string())?;
    }
    let url = clean_image(&format!("data:{mime};base64,{}", b64_encode(out.get_ref())));
    if url.is_empty() { Err("the picture is too big".into()) } else { Ok(url) }
}

/// A picture data URL → (width, height, RGBA pixels) for the screen, under the same limits (None when it cannot be read).
pub fn decode_rgba(src: &str) -> Option<(u32, u32, Vec<u8>)> {
    let b64 = src.split_once(";base64,")?.1;
    let bytes = b64_decode(b64);
    let fmt = format_of(&bytes)?;
    let mut reader = ImageReader::with_format(Cursor::new(&bytes), fmt);
    reader.limits(limits());
    let img = reader.decode().ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    Some((w, h, img.into_raw()))
}

/// RGBA pixels → a PNG file (BCF snapshots).
pub fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let img = ::image::RgbaImage::from_raw(w, h, rgba.to_vec())?;
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// The signature pad (520 × 180, drawn at 2×): strokes in pad coordinates → a transparent PNG data URL trimmed to the ink
/// with 6 px round it, the ink #1b2a4a with round caps 2.6 wide. None when nothing was drawn.
pub fn signature_png(strokes: &[Vec<[f64; 2]>]) -> Option<String> {
    const W: f64 = 520.0;
    const H: f64 = 180.0;
    const LW: f64 = 2.6;
    let pts = strokes.iter().flatten().filter(|p| p[0].is_finite() && p[1].is_finite());
    let mut ink: Option<[f64; 4]> = None;
    for p in pts {
        let [x, y] = [p[0].clamp(0.0, W), p[1].clamp(0.0, H)];
        ink = Some(ink.map_or([x, y, x, y], |i| [i[0].min(x), i[1].min(y), i[2].max(x), i[3].max(y)]));
    }
    let ink = ink?;
    let pad = 6.0;
    let (x0, y0, x1, y1) = ((ink[0] - pad).max(0.0), (ink[1] - pad).max(0.0), (ink[2] + pad).min(W), (ink[3] + pad).min(H));
    let (pw, ph) = (scaled_px((x1 - x0) * 2.0), scaled_px((y1 - y0) * 2.0));
    let mut img = ::image::RgbaImage::new(pw, ph);
    let r = LW; // half the width at 2×
    for s in strokes {
        let seg: Vec<[f64; 2]> = s.iter().map(|p| [(p[0] - x0) * 2.0, (p[1] - y0) * 2.0]).collect();
        let pairs: Vec<([f64; 2], [f64; 2])> = match seg.as_slice() {
            [one] => vec![(*one, *one)],
            _ => seg.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).collect(),
        };
        for (a, b) in pairs {
            let (bx0, by0) = (a[0].min(b[0]) - r - 1.0, a[1].min(b[1]) - r - 1.0);
            let (bx1, by1) = (a[0].max(b[0]) + r + 1.0, a[1].max(b[1]) + r + 1.0);
            for py in px_range(by0, by1, ph) {
                for px in px_range(bx0, bx1, pw) {
                    let c = [f64::from(px) + 0.5, f64::from(py) + 0.5];
                    let d = dist_seg(c, a, b);
                    let cover = (r + 0.5 - d).clamp(0.0, 1.0);
                    if cover > 0.0 {
                        let p = img.get_pixel_mut(px, py);
                        let alpha = f64::from(p.0[3]) / 255.0;
                        let na = alpha.max(cover);
                        *p = ::image::Rgba([0x1b, 0x2a, 0x4a, to_u8(na * 255.0)]);
                    }
                }
            }
        }
    }
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png).ok()?;
    let url = clean_image(&format!("data:image/png;base64,{}", b64_encode(out.get_ref())));
    (!url.is_empty()).then_some(url)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped to 1..=2000 first
fn scaled_px(v: f64) -> u32 {
    v.round().clamp(1.0, 2000.0) as u32
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped to 0..=255 first
fn to_u8(v: f64) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

/// The pixel rows (or columns) between a and b, inside 0..n.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped to 0..=n first
fn px_range(a: f64, b: f64, n: u32) -> std::ops::Range<u32> {
    let lo = a.floor().clamp(0.0, f64::from(n)) as u32;
    let hi = b.ceil().clamp(0.0, f64::from(n)) as u32;
    lo..hi
}

fn dist_seg(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    (p[0] - (a[0] + t * dx)).hypot(p[1] - (a[1] + t * dy))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = ::image::RgbaImage::from_pixel(w, h, ::image::Rgba([10, 20, 30, 255]));
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn a_small_picture_is_kept_as_it_is_and_a_big_one_scaled() {
        let small = png(40, 20);
        let url = picture_from(&small).unwrap();
        assert_eq!(b64_decode(url.split_once(";base64,").unwrap().1), small);
        let big = png(3200, 100);
        let url = picture_from(&big).unwrap();
        let info = cc_markup::imagestamp::image_info(&url).unwrap();
        assert_eq!((info.width, info.height), (1600, 50));
        assert_eq!(decode_rgba(&url).map(|(w, h, _)| (w, h)), Some((1600, 50)));
    }

    #[test]
    fn hostile_pictures_are_refused() {
        assert!(picture_from(b"GIF89a").is_err());
        assert!(picture_from(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0]).is_err());
        // a header claiming 60000 × 60000 pixels: refused before anything is allocated
        let mut forged = png(1, 1);
        forged[16..20].copy_from_slice(&60_000u32.to_be_bytes());
        forged[20..24].copy_from_slice(&60_000u32.to_be_bytes());
        assert!(picture_from(&forged).is_err());
        assert!(decode_rgba("data:image/png;base64,AAAA").is_none());
    }

    #[test]
    fn a_signature_is_trimmed_to_its_ink() {
        assert!(signature_png(&[]).is_none());
        let url = signature_png(&[vec![[100.0, 50.0], [200.0, 80.0]], vec![[150.0, 90.0]]]).unwrap();
        let info = cc_markup::imagestamp::image_info(&url).unwrap();
        assert_eq!((info.width, info.height), (224, 104));
    }
}
