//! Pictures in the saved PDF, as pdf-lib's `embedJpg` / `embedPng` write them: a JPEG goes in as it is (DCTDecode,
//! its size, depth and colour space from the frame header); a PNG is decoded to RGBA, its colour written as an RGB
//! image (FlateDecode) and its alpha, when any pixel is not opaque, as a soft mask. The reference is taken when the
//! picture is embedded, the objects are written at save time (`finish`), as pdf-lib does.
//! Spec gap: interlaced (Adam7) PNGs are not decoded here (the markup is left out of the save).

use crate::libdoc::{LibDoc, Ref, deflate, inflate};
use crate::obj::{Dict, Obj, Stream};
use cc_markup::imagestamp::ImageInfo;

/// Take a reference for a picture (written at save time).
pub fn embed(doc: &mut LibDoc, info: &ImageInfo) -> Option<Ref> {
    if info.mime == "image/png" {
        decode_png(&info.bytes)?;
    }
    let r = doc.next_ref();
    doc.pending_images.push((r, info.clone()));
    Some(r)
}

/// Write the pictures taken during this save.
pub fn finish(doc: &mut LibDoc) {
    let pending = std::mem::take(&mut doc.pending_images);
    for (r, info) in pending {
        if info.mime == "image/jpeg" {
            let (bits, w, h, comps) = jpeg_frame(&info.bytes).unwrap_or((8, info.width, info.height, 3));
            let mut d = Dict::new()
                .with("Type", Obj::name("XObject"))
                .with("Subtype", Obj::name("Image"))
                .with("BitsPerComponent", Obj::Num(f64::from(bits)))
                .with("Width", Obj::Num(f64::from(w)))
                .with("Height", Obj::Num(f64::from(h)))
                .with(
                    "ColorSpace",
                    Obj::name(match comps {
                        1 => "DeviceGray",
                        4 => "DeviceCMYK",
                        _ => "DeviceRGB",
                    }),
                )
                .with("Filter", Obj::name("DCTDecode"));
            if comps == 4 {
                d.set("Decode", Obj::nums(&[1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0]));
            }
            doc.put(r, Obj::Stream(Box::new(Stream { dict: d, data: info.bytes.clone() })));
            continue;
        }
        let Some((w, h, rgba)) = decode_png(&info.bytes) else { continue };
        let mut rgb = Vec::with_capacity((rgba.len() / 4).saturating_mul(3));
        let mut alpha = Vec::with_capacity(rgba.len() / 4);
        for px in rgba.chunks(4) {
            rgb.extend_from_slice(px.get(..3).unwrap_or(&[0, 0, 0]));
            alpha.push(px.get(3).copied().unwrap_or(255));
        }
        let opaque = alpha.iter().all(|a| *a == 255);
        let mut d = Dict::new()
            .with("Type", Obj::name("XObject"))
            .with("Subtype", Obj::name("Image"))
            .with("BitsPerComponent", Obj::Num(8.0))
            .with("Width", Obj::Num(f64::from(w)))
            .with("Height", Obj::Num(f64::from(h)))
            .with("ColorSpace", Obj::name("DeviceRGB"))
            .with("Filter", Obj::name("FlateDecode"));
        if !opaque {
            let sm = doc.register(Obj::Stream(Box::new(Stream {
                dict: Dict::new()
                    .with("Type", Obj::name("XObject"))
                    .with("Subtype", Obj::name("Image"))
                    .with("Height", Obj::Num(f64::from(h)))
                    .with("Width", Obj::Num(f64::from(w)))
                    .with("BitsPerComponent", Obj::Num(8.0))
                    .with("ColorSpace", Obj::name("DeviceGray"))
                    .with("Decode", Obj::nums(&[0.0, 1.0]))
                    .with("Filter", Obj::name("FlateDecode")),
                data: deflate(&alpha),
            })));
            d.set("SMask", Obj::Ref(sm.0, sm.1));
        }
        doc.put(r, Obj::Stream(Box::new(Stream { dict: d, data: deflate(&rgb) })));
    }
}

fn jpeg_frame(b: &[u8]) -> Option<(u8, u32, u32, u8)> {
    let mut p = 2usize;
    while p.saturating_add(9) < b.len() {
        if b.get(p) != Some(&0xff) {
            p = p.saturating_add(1);
            continue;
        }
        let mk = *b.get(p.saturating_add(1))?;
        if (0xc0..=0xcf).contains(&mk) && mk != 0xc4 && mk != 0xc8 && mk != 0xcc {
            let g = |k: usize| b.get(p.saturating_add(k)).copied().unwrap_or(0);
            return Some((g(4), u32::from(u16::from_be_bytes([g(7), g(8)])), u32::from(u16::from_be_bytes([g(5), g(6)])), g(9)));
        }
        let len = usize::from(u16::from_be_bytes([
            b.get(p.saturating_add(2)).copied().unwrap_or(0),
            b.get(p.saturating_add(3)).copied().unwrap_or(0),
        ]));
        p = p.saturating_add(2).saturating_add(len);
    }
    None
}

/// Most pixels a picture may have (8000 × 8000, the limit cleanImage puts on markups).
const MAX_PIXELS: u64 = 64_000_000;

/// A PNG → (width, height, RGBA bytes).
pub fn decode_png(b: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    if b.get(..8)? != [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a] {
        return None;
    }
    let mut p = 8usize;
    let (mut w, mut h, mut depth, mut ctype, mut interlace) = (0u32, 0u32, 0u8, 0u8, 0u8);
    let mut plte: Vec<u8> = Vec::new();
    let mut trns: Vec<u8> = Vec::new();
    let mut idat: Vec<u8> = Vec::new();
    while p.saturating_add(8) <= b.len() {
        let len = usize::try_from(u32::from_be_bytes(b.get(p..p.saturating_add(4))?.try_into().ok()?)).ok()?;
        let ty = b.get(p.saturating_add(4)..p.saturating_add(8))?;
        let data = b.get(p.saturating_add(8)..p.saturating_add(8).saturating_add(len))?;
        match ty {
            b"IHDR" => {
                w = u32::from_be_bytes(data.get(..4)?.try_into().ok()?);
                h = u32::from_be_bytes(data.get(4..8)?.try_into().ok()?);
                depth = *data.get(8)?;
                ctype = *data.get(9)?;
                interlace = *data.get(12)?;
            }
            b"PLTE" => plte = data.to_vec(),
            b"tRNS" => trns = data.to_vec(),
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
        p = p.saturating_add(12).saturating_add(len);
    }
    if w == 0 || h == 0 || interlace != 0 || u64::from(w).saturating_mul(u64::from(h)) > MAX_PIXELS {
        return None;
    }
    let channels: usize = match ctype {
        0 => 1,
        2 => 3,
        3 => 1,
        4 => 2,
        6 => 4,
        _ => return None,
    };
    if ![1u8, 2, 4, 8, 16].contains(&depth) {
        return None;
    }
    let raw = inflate(&idat)?;
    let wu = usize::try_from(w).ok()?;
    let hu = usize::try_from(h).ok()?;
    let bits_px = channels.saturating_mul(usize::from(depth));
    let stride = wu.saturating_mul(bits_px).div_ceil(8);
    let bpp = bits_px.div_ceil(8).max(1);
    let mut prev = vec![0u8; stride];
    let mut out = Vec::with_capacity(wu.saturating_mul(hu).saturating_mul(4));
    let mut pos = 0usize;
    let be16 = |v: &[u8], k: usize| -> Option<u16> { Some(u16::from_be_bytes([*v.get(k)?, *v.get(k.checked_add(1)?)?])) };
    for _ in 0..hu {
        let f = *raw.get(pos)?;
        let line = raw.get(pos.saturating_add(1)..pos.saturating_add(1).saturating_add(stride))?;
        pos = pos.saturating_add(1).saturating_add(stride);
        let mut cur = line.to_vec();
        for i in 0..stride {
            let back = i.checked_sub(bpp);
            let a = back.and_then(|j| cur.get(j).copied()).unwrap_or(0);
            let up = prev.get(i).copied().unwrap_or(0);
            let c = back.and_then(|j| prev.get(j).copied()).unwrap_or(0);
            let x = cur.get(i).copied().unwrap_or(0);
            let v = match f {
                0 => x,
                1 => x.wrapping_add(a),
                2 => x.wrapping_add(up),
                3 => x.wrapping_add(u8::try_from(u16::from(a).saturating_add(u16::from(up)) / 2).unwrap_or(0)),
                4 => {
                    let (ai, ui, ci) = (i16::from(a), i16::from(up), i16::from(c));
                    let pa = ui.saturating_sub(ci).saturating_abs();
                    let pb = ai.saturating_sub(ci).saturating_abs();
                    let pc = ai.saturating_add(ui).saturating_sub(ci.saturating_mul(2)).saturating_abs();
                    x.wrapping_add(if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        up
                    } else {
                        c
                    })
                }
                _ => return None,
            };
            if let Some(s) = cur.get_mut(i) {
                *s = v;
            }
        }
        // samples → RGBA
        let d = usize::from(depth);
        let max_v = (1u32 << u32::from(depth.min(16))).saturating_sub(1).max(1);
        let sample = |k: usize| -> u16 {
            match depth {
                16 => be16(&cur, k.saturating_mul(2)).unwrap_or(0),
                8 => u16::from(cur.get(k).copied().unwrap_or(0)),
                _ => {
                    let bit = k.saturating_mul(d);
                    let byte = cur.get(bit / 8).copied().unwrap_or(0);
                    let shift = 8usize.saturating_sub(d).saturating_sub(bit % 8);
                    let mask = u8::try_from(max_v).unwrap_or(0xff);
                    u16::from(byte.checked_shr(u32::try_from(shift).unwrap_or(0)).unwrap_or(0) & mask)
                }
            }
        };
        let to8 = |v: u16| -> u8 {
            match depth {
                16 => v.to_be_bytes()[0],
                8 => u8::try_from(v).unwrap_or(255),
                _ => u8::try_from(u32::from(v).saturating_mul(255).checked_div(max_v).unwrap_or(0)).unwrap_or(255),
            }
        };
        for x in 0..wu {
            let base = x.saturating_mul(channels);
            let at = |k: usize| sample(base.saturating_add(k));
            let px: [u8; 4] = match ctype {
                0 => {
                    let v = at(0);
                    let g = to8(v);
                    let tr = be16(&trns, 0) == Some(v);
                    [g, g, g, if tr { 0 } else { 255 }]
                }
                2 => {
                    let (r, g, bl) = (at(0), at(1), at(2));
                    let tr = be16(&trns, 0) == Some(r) && be16(&trns, 2) == Some(g) && be16(&trns, 4) == Some(bl);
                    [to8(r), to8(g), to8(bl), if tr { 0 } else { 255 }]
                }
                3 => {
                    let i = usize::from(at(0));
                    let k = i.saturating_mul(3);
                    let c = |j: usize| plte.get(k.saturating_add(j)).copied().unwrap_or(0);
                    [c(0), c(1), c(2), trns.get(i).copied().unwrap_or(255)]
                }
                4 => {
                    let g = to8(at(0));
                    [g, g, g, to8(at(1))]
                }
                _ => [to8(at(0)), to8(at(1)), to8(at(2)), to8(at(3))],
            };
            out.extend_from_slice(&px);
        }
        prev = cur;
    }
    Some((w, h, out))
}
