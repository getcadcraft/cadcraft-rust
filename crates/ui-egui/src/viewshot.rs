//! The picture of the view an assistant asks for (`view_screenshot`, 0.5 agents.js: "A PNG of the drawing as it is on
//! screen (up to 1600 px on the long side)"): the window's last frame (egui's viewport screenshot) cropped to the view the
//! app drew last (the drawing canvas or the 3D view, noted by [`note_view_rect`]) and scaled down to `max` pixels on its
//! long side, as the JS draws its canvas into a smaller one.

use egui::{ColorImage, Rect};

const KEY: &str = "cc-view-rect";

/// The view's rectangle (points) this frame: the 2D canvas and the 3D views call it as they paint.
pub fn note_view_rect(ctx: &egui::Context, rect: Rect) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique(KEY), rect));
}

/// The view's rectangle as noted last.
pub fn view_rect(ctx: &egui::Context) -> Option<Rect> {
    ctx.data(|d| d.get_temp::<Rect>(egui::Id::unique(KEY)))
}

/// The part of `shot` (physical pixels) under `rect` (points, `ppp` pixels a point), at most `max` pixels on its long
/// side (box-averaged when smaller) → (PNG bytes, width, height). None for an empty crop.
pub fn crop_png(shot: &ColorImage, rect: Rect, ppp: f32, max: u32) -> Option<(Vec<u8>, u32, u32)> {
    let [sw, sh] = shot.size;
    let px = |v: f32, lim: usize| -> usize {
        let p = (v * ppp).round();
        if p.is_finite() && p > 0.0 { (p as usize).min(lim) } else { 0 }
    };
    let (x0, y0, x1, y1) = (px(rect.min.x, sw), px(rect.min.y, sh), px(rect.max.x, sw), px(rect.max.y, sh));
    let (w0, h0) = (x1.checked_sub(x0)?, y1.checked_sub(y0)?);
    if w0 == 0 || h0 == 0 {
        return None;
    }
    let k = (f64::from(max.max(1)) / w0.max(h0) as f64).min(1.0);
    let w = ((w0 as f64 * k).round() as usize).max(1);
    let h = ((h0 as f64 * k).round() as usize).max(1);
    let mut rgba = Vec::with_capacity(w.saturating_mul(h).saturating_mul(4));
    for y in 0..h {
        let (sy0, sy1) = (y0 + y * h0 / h, (y0 + (y + 1) * h0 / h).max(y0 + y * h0 / h + 1));
        for x in 0..w {
            let (sx0, sx1) = (x0 + x * w0 / w, (x0 + (x + 1) * w0 / w).max(x0 + x * w0 / w + 1));
            let (mut acc, mut n) = ([0u32; 4], 0u32);
            for yy in sy0..sy1.min(sh) {
                for xx in sx0..sx1.min(sw) {
                    if let Some(c) = shot.pixels.get(yy * sw + xx) {
                        let a = c.to_srgba_unmultiplied();
                        for (s, v) in acc.iter_mut().zip(a) {
                            *s += u32::from(v);
                        }
                        n += 1;
                    }
                }
            }
            let n = n.max(1);
            rgba.extend(acc.iter().map(|s| u8::try_from(s / n).unwrap_or(255)));
        }
    }
    let png = cc_io_markup::picture::encode_png(u32::try_from(w).ok()?, u32::try_from(h).ok()?, &rgba)?;
    Some((png, u32::try_from(w).ok()?, u32::try_from(h).ok()?))
}

/// base64 (standard, padded): an MCP image's `data`.
pub fn b64(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(b.len() / 3 * 4 + 4);
    for c in b.chunks(3) {
        let v = (u32::from(c.first().copied().unwrap_or(0)) << 16)
            | (u32::from(c.get(1).copied().unwrap_or(0)) << 8)
            | u32::from(c.get(2).copied().unwrap_or(0));
        let ch = |sh: u32| char::from(A.get(((v >> sh) & 63) as usize).copied().unwrap_or(b'A'));
        s.push(ch(18));
        s.push(ch(12));
        s.push(if c.len() > 1 { ch(6) } else { '=' });
        s.push(if c.len() > 2 { ch(0) } else { '=' });
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crop_scales_to_its_long_side() {
        let img = ColorImage::new([200, 100], vec![egui::Color32::from_rgb(10, 20, 30); 200 * 100]);
        let (png, w, h) = crop_png(&img, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 50.0)), 2.0, 50).expect("png");
        assert_eq!((w, h), (50, 25));
        assert!(png.starts_with(b"\x89PNG"));
        assert_eq!(b64(b"ab"), "YWI=");
        assert!(crop_png(&img, Rect::from_min_max(egui::pos2(300.0, 0.0), egui::pos2(400.0, 50.0)), 1.0, 50).is_none());
    }
}
