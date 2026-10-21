//! pictures.js `pdfImage` (0.5 WL.13): a drawing's picture file as a PDF image: a JPEG as it is (DCTDecode), a PNG
//! decoded here (`decodePng`: 8-bit grey, grey + alpha, RGB, RGBA and palette with tRNS, 16-bit taken to 8; interlaced
//! files refused) and deflated, its transparency as a soft mask. With a dpi cap a PNG printed denser than the cap is
//! box-filtered down to it (`shrink`); a JPEG stays as it is and says so (`kept`).
//!
//! The bytes are hostile: every offset is checked, the inflated data is capped at what the image needs, and a picture
//! over 100 million pixels is refused, as pictures.js does.

use cc_cad::pictures::{MAX_PIXELS, PicRec};

/// One image of the PDF: `{ w, h, filter, data, colors, smask?, shrunk?, kept? }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PdfImage {
    pub w: u32,
    pub h: u32,
    /// "DCTDecode" | "FlateDecode"
    pub filter: &'static str,
    pub data: Vec<u8>,
    /// 1 (grey) or 3 (RGB)
    pub colors: u8,
    /// the deflated alpha channel, when the picture has transparency
    pub smask: Option<Vec<u8>>,
    pub shrunk: bool,
    pub kept: bool,
}

/// pictures.js `jpegInfo`: a JPEG's size and number of colour components from its first SOF0–SOF2 marker.
pub fn jpeg_info(b: &[u8]) -> Option<(u32, u32, u8)> {
    let at = |i: usize| b.get(i).copied();
    let mut i = 2usize;
    while i.checked_add(9).is_some_and(|e| e < b.len()) {
        if at(i)? != 0xff {
            return None;
        }
        let m = at(i.checked_add(1)?)?;
        if m == 0xd8 || (0xd0..=0xd7).contains(&m) || m == 0x01 {
            i = i.checked_add(2)?;
            continue;
        }
        let len = usize::from(u16::from_be_bytes([at(i.checked_add(2)?)?, at(i.checked_add(3)?)?]));
        if (0xc0..=0xc2).contains(&m) {
            let h = u32::from(u16::from_be_bytes([at(i.checked_add(5)?)?, at(i.checked_add(6)?)?]));
            let w = u32::from(u16::from_be_bytes([at(i.checked_add(7)?)?, at(i.checked_add(8)?)?]));
            return Some((w, h, at(i.checked_add(9)?)?));
        }
        if m == 0xda || len < 2 {
            return None;
        }
        i = i.checked_add(2)?.checked_add(len)?;
    }
    None
}

/// A decoded PNG: `w × h`, RGB (3 bytes a pixel) and an alpha channel when some pixel is not opaque.
pub struct Decoded {
    pub w: u32,
    pub h: u32,
    pub rgb: Vec<u8>,
    pub alpha: Option<Vec<u8>>,
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?]))
}

/// pictures.js `decodePng`.
pub fn decode_png(b: &[u8]) -> Option<Decoded> {
    let mut i = 8usize;
    let (mut w, mut h, mut depth, mut ct, mut inter) = (0u32, 0u32, 0u8, 0u8, 0u8);
    let mut pal: Option<&[u8]> = None;
    let mut trns: Option<&[u8]> = None;
    let mut idat: Vec<u8> = Vec::new();
    while i.checked_add(12).is_some_and(|e| e <= b.len()) {
        let len = usize::try_from(be32(b, i)?).ok()?;
        let end = i.checked_add(12)?.checked_add(len)?;
        if end > b.len() {
            return None;
        }
        let ty = b.get(i.checked_add(4)?..i.checked_add(8)?)?;
        let d = b.get(i.checked_add(8)?..i.checked_add(8)?.checked_add(len)?)?;
        match ty {
            b"IHDR" => {
                w = be32(b, i.checked_add(8)?)?;
                h = be32(b, i.checked_add(12)?)?;
                depth = d.get(8).copied().unwrap_or(0);
                ct = d.get(9).copied().unwrap_or(0);
                inter = d.get(12).copied().unwrap_or(0);
            }
            b"PLTE" => pal = Some(d),
            b"tRNS" => trns = Some(d),
            b"IDAT" => idat.extend_from_slice(d),
            b"IEND" => break,
            _ => {}
        }
        i = end;
    }
    let ok_depth = depth == 8 || (depth == 16 && ct != 3);
    if w == 0 || h == 0 || f64::from(w) * f64::from(h) > MAX_PIXELS || inter != 0 || !ok_depth || ![0u8, 2, 3, 4, 6].contains(&ct) {
        return None;
    }
    let chans: usize = match ct {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        _ => 4,
    };
    let step = if depth == 16 { 2usize } else { 1 };
    let bpp = chans.checked_mul(step)?;
    let (wu, hu) = (usize::try_from(w).ok()?, usize::try_from(h).ok()?);
    let stride = wu.checked_mul(bpp)?;
    let need = hu.checked_mul(stride.checked_add(1)?)?;
    // the image needs `need` bytes; more is not read (a stream that inflates further is cut there)
    let raw = match miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&idat, need) {
        Ok(v) => v,
        Err(e) if e.status == miniz_oxide::inflate::TINFLStatus::HasMoreOutput => e.output,
        Err(_) => return None,
    };
    if raw.len() < need {
        return None;
    }
    let mut px = vec![0u8; hu.checked_mul(stride)?];
    for y in 0..hu {
        let row = y.checked_mul(stride.checked_add(1)?)?;
        let f = *raw.get(row)?;
        let src = raw.get(row.checked_add(1)?..row.checked_add(1)?.checked_add(stride)?)?;
        let (before, rest) = px.split_at_mut(y.checked_mul(stride)?);
        let out = rest.get_mut(..stride)?;
        let up: Option<&[u8]> = if y > 0 { before.get(before.len().checked_sub(stride)?..) } else { None };
        for x in 0..stride {
            let a = if x >= bpp { i32::from(*out.get(x.checked_sub(bpp)?)?) } else { 0 };
            let u = up.and_then(|r| r.get(x)).map_or(0, |v| i32::from(*v));
            let c = if x >= bpp { up.and_then(|r| r.get(x.wrapping_sub(bpp))).map_or(0, |v| i32::from(*v)) } else { 0 };
            let mut v = i32::from(*src.get(x)?);
            match f {
                0 => {}
                1 => v = v.wrapping_add(a),
                2 => v = v.wrapping_add(u),
                3 => v = v.wrapping_add(a.wrapping_add(u).wrapping_shr(1)),
                4 => {
                    let p = a.wrapping_add(u).wrapping_sub(c);
                    let (pa, pb, pc) =
                        (p.wrapping_sub(a).wrapping_abs(), p.wrapping_sub(u).wrapping_abs(), p.wrapping_sub(c).wrapping_abs());
                    v = v.wrapping_add(if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        u
                    } else {
                        c
                    });
                }
                _ => return None,
            }
            *out.get_mut(x)? = u8::try_from(v & 0xff).unwrap_or(0);
        }
    }
    let n = wu.checked_mul(hu)?;
    let mut rgb = vec![0u8; n.checked_mul(3)?];
    let mut alpha = if ct == 4 || ct == 6 || trns.is_some() { Some(vec![255u8; n]) } else { None };
    for k in 0..n {
        let o = k.checked_mul(bpp)?;
        let s = |j: usize| px.get(o.saturating_add(j.saturating_mul(step))).copied().unwrap_or(0);
        let r3 = k.checked_mul(3)?;
        let (r, g, bl) = match ct {
            3 => {
                let ix = usize::from(s(0));
                let p = |j: usize| pal.and_then(|p| p.get(ix.saturating_mul(3).saturating_add(j))).copied().unwrap_or(0);
                if let Some(a) = alpha.as_mut().and_then(|a| a.get_mut(k)) {
                    *a = trns.and_then(|t| t.get(ix)).copied().unwrap_or(255);
                }
                (p(0), p(1), p(2))
            }
            0 | 4 => {
                if ct == 4
                    && let Some(a) = alpha.as_mut().and_then(|a| a.get_mut(k))
                {
                    *a = s(1);
                }
                (s(0), s(0), s(0))
            }
            _ => {
                if ct == 6
                    && let Some(a) = alpha.as_mut().and_then(|a| a.get_mut(k))
                {
                    *a = s(3);
                }
                (s(0), s(1), s(2))
            }
        };
        if let Some(t) = rgb.get_mut(r3..r3.saturating_add(3)) {
            t.copy_from_slice(&[r, g, bl]);
        }
    }
    if alpha.as_ref().is_some_and(|a| a.iter().all(|v| *v == 255)) {
        alpha = None;
    }
    Some(Decoded { w, h, rgb, alpha })
}

/// pictures.js `shrink`: box-filter down to `w2 × h2` (each channel the truncated mean of its box).
pub fn shrink(img: &Decoded, w2: u32, h2: u32) -> Decoded {
    let (w, h) = (u64::from(img.w), u64::from(img.h));
    let (w2u, h2u) = (u64::from(w2.max(1)), u64::from(h2.max(1)));
    let n2 = usize::try_from(w2u.saturating_mul(h2u)).unwrap_or(0);
    let mut rgb = vec![0u8; n2.saturating_mul(3)];
    let mut alpha = img.alpha.as_ref().map(|_| vec![0u8; n2]);
    for y in 0..h2u {
        for x in 0..w2u {
            let div = |a: u64, b: u64| a.checked_div(b).unwrap_or(0);
            let x0 = div(x.saturating_mul(w), w2u);
            let x1 = (x0.saturating_add(1)).max(div(x.saturating_add(1).saturating_mul(w), w2u));
            let y0 = div(y.saturating_mul(h), h2u);
            let y1 = (y0.saturating_add(1)).max(div(y.saturating_add(1).saturating_mul(h), h2u));
            let (mut r, mut g, mut bl, mut a, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
            for yy in y0..y1 {
                for xx in x0..x1 {
                    let k = usize::try_from(yy.saturating_mul(w).saturating_add(xx)).unwrap_or(usize::MAX);
                    let px = |j: usize| u64::from(img.rgb.get(k.saturating_mul(3).saturating_add(j)).copied().unwrap_or(0));
                    r = r.saturating_add(px(0));
                    g = g.saturating_add(px(1));
                    bl = bl.saturating_add(px(2));
                    if let Some(al) = &img.alpha {
                        a = a.saturating_add(u64::from(al.get(k).copied().unwrap_or(0)));
                    }
                    n = n.saturating_add(1);
                }
            }
            let n = n.max(1);
            let o = usize::try_from(y.saturating_mul(w2u).saturating_add(x)).unwrap_or(usize::MAX);
            let v = |s: u64| u8::try_from(s.checked_div(n).unwrap_or(0)).unwrap_or(255);
            if let Some(t) = rgb.get_mut(o.saturating_mul(3)..o.saturating_mul(3).saturating_add(3)) {
                t.copy_from_slice(&[v(r), v(g), v(bl)]);
            }
            if let Some(al) = alpha.as_mut().and_then(|al| al.get_mut(o)) {
                *al = v(a);
            }
        }
    }
    Decoded { w: w2.max(1), h: h2.max(1), rgb, alpha }
}

/// A whole, non-negative f64 → u32 (saturating at u32::MAX; NaN → 1).
fn js_u32(v: f64) -> u32 {
    if !(v >= 1.0) {
        return 1;
    }
    if v >= f64::from(u32::MAX) {
        return u32::MAX;
    }
    format!("{v:.0}").parse::<u32>().unwrap_or(1)
}

fn deflate(b: &[u8]) -> Vec<u8> {
    crate::plot::zlib(b)
}

/// pictures.js `pdfImage(rec, { maxDpi, mmW, mmH })` → None when the file cannot go in (a CMYK or damaged JPEG, a
/// PNG this decoder does not take).
pub fn pdf_image(rec: &PicRec, max_dpi: f64, mm_w: f64, mm_h: f64) -> Option<PdfImage> {
    if rec.mime == "image/jpeg" {
        let (w, h, comps) = jpeg_info(&rec.bytes)?;
        if comps != 1 && comps != 3 {
            return None;
        }
        let dense = max_dpi > 0.0 && mm_w > 0.0 && f64::from(w) / (mm_w / 25.4) > max_dpi * 1.01;
        return Some(PdfImage {
            w,
            h,
            filter: "DCTDecode",
            data: rec.bytes.to_vec(),
            colors: comps,
            smask: None,
            shrunk: false,
            kept: dense,
        });
    }
    let mut img = decode_png(&rec.bytes)?;
    let mut shrunk = false;
    if max_dpi > 0.0 && mm_w > 0.0 && mm_h > 0.0 {
        let side = |mm: f64| -> u32 {
            let v = cc_cad::js::round(max_dpi * mm / 25.4).max(1.0);
            // a whole number ≥ 1 (dpi and size are finite and positive here); its decimal text parses back exactly
            js_u32(v)
        };
        let (w2, h2) = (side(mm_w), side(mm_h));
        if w2 < img.w && h2 < img.h {
            img = shrink(&img, w2, h2);
            shrunk = true;
        }
    }
    Some(PdfImage {
        w: img.w,
        h: img.h,
        filter: "FlateDecode",
        data: deflate(&img.rgb),
        colors: 3,
        smask: img.alpha.as_deref().map(deflate),
        shrunk,
        kept: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// a 4 × 4 RGBA PNG (opaque)
    const PNG4: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 4, 0, 0, 0, 4, 8, 6, 0, 0, 0, 169, 241, 158, 126, 0, 0, 0,
        25, 73, 68, 65, 84, 120, 156, 99, 96, 96, 56, 241, 255, 63, 195, 137, 255, 112, 26, 133, 3, 198, 132, 84, 0, 0, 196, 28, 36, 105,
        198, 78, 180, 214, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];

    #[test]
    fn decodes_and_shrinks() {
        let d = decode_png(PNG4).unwrap();
        assert_eq!((d.w, d.h, d.rgb.len()), (4, 4, 48));
        let s = shrink(&d, 2, 2);
        assert_eq!((s.w, s.h, s.rgb.len()), (2, 2, 12));
    }

    #[test]
    fn hostile_files_give_nothing() {
        assert!(decode_png(b"\x89PNG\r\n\x1a\n").is_none());
        let mut big = PNG4.to_vec();
        big[16..20].copy_from_slice(&100_000u32.to_be_bytes());
        big[20..24].copy_from_slice(&100_000u32.to_be_bytes());
        assert!(decode_png(&big).is_none());
        let mut cut = PNG4.to_vec();
        cut.truncate(40);
        assert!(decode_png(&cut).is_none());
        assert_eq!(jpeg_info(&[0xff, 0xd8, 0xff]), None);
        assert_eq!(jpeg_info(&[0xff, 0xd8, 0xff, 0xc0, 0, 17, 8, 0, 2, 0, 3, 3, 1, 0x22]), Some((3, 2, 3)));
    }
}
