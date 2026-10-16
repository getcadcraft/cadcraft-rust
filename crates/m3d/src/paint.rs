//! Paint (0.4 M8, 0.5 RC.7 / RC.11 (b)): a colour '#rrggbb', a library material 'mat:<id>' or an imported texture
//! 'img:<id>|ux,uy,uz,u0,vx,vy,vz,v0|#rrggbb' in a face's or placement's `color` field. The JS's regular expressions
//! are written out as small matchers with the same rules (`\d` = ASCII digits; the `i` flag where the JS has it).

use crate::def::Def;
use crate::error::{Error, Res, msg};
use crate::js;
use crate::vec::P3;

/// `isColor(c)`: /^#[0-9a-f]{6}$/i
pub fn is_color(c: &str) -> bool {
    let b = c.as_bytes();
    b.len() == 7 && b.first() == Some(&b'#') && b.iter().skip(1).all(u8::is_ascii_hexdigit)
}
/// `isMaterialPaint(c)`: /^mat:[a-z0-9][a-z0-9-]{0,39}$/
pub fn is_material_paint(c: &str) -> bool {
    let Some(rest) = c.strip_prefix("mat:") else { return false };
    let b = rest.as_bytes();
    let ok = |x: &u8| x.is_ascii_lowercase() || x.is_ascii_digit();
    !b.is_empty() && b.len() <= 40 && b.first().is_some_and(ok) && b.iter().skip(1).all(|x| ok(x) || *x == b'-')
}

/// NUM = -?\d+(?:\.\d+)?(?:e[-+]?\d+)? at b[i..] (case-insensitive e) → the index after it.
fn num_at(b: &[u8], mut i: usize) -> Option<usize> {
    let digits = |b: &[u8], mut i: usize| {
        let s = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        (i > s).then_some(i)
    };
    if b.get(i) == Some(&b'-') {
        i += 1;
    }
    i = digits(b, i)?;
    if b.get(i) == Some(&b'.')
        && let Some(j) = digits(b, i + 1)
    {
        i = j;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(b.get(j), Some(b'-' | b'+')) {
            j += 1;
        }
        if let Some(k) = digits(b, j) {
            i = k;
        }
    }
    Some(i)
}

/// `isImagePaint(c)`: at most 200 long and /^img:[a-z0-9]{1,24}\|(?:NUM,){7}NUM\|(?:#[0-9a-f]{6})?$/i
pub fn is_image_paint(c: &str) -> bool {
    if c.encode_utf16().count() > 200 {
        return false;
    }
    let b = c.as_bytes();
    if b.len() < 4 || !b.get(..4).is_some_and(|p| p.eq_ignore_ascii_case(b"img:")) {
        return false;
    }
    let mut i = 4;
    let s = i;
    while b.get(i).is_some_and(u8::is_ascii_alphanumeric) {
        i += 1;
    }
    if i == s || i - s > 24 || b.get(i) != Some(&b'|') {
        return false;
    }
    i += 1;
    for k in 0..8 {
        let Some(j) = num_at(b, i) else { return false };
        i = j;
        let want = if k < 7 { b',' } else { b'|' };
        if b.get(i) != Some(&want) {
            return false;
        }
        i += 1;
    }
    let rest = b.get(i..).unwrap_or(&[]);
    rest.is_empty() || (rest.len() == 7 && rest.first() == Some(&b'#') && rest.iter().skip(1).all(u8::is_ascii_hexdigit))
}
pub fn is_paint(c: &str) -> bool {
    is_color(c) || is_material_paint(c) || is_image_paint(c)
}
/// `imageId(c)`: c.slice(4, c.indexOf('|')).
pub fn image_id(c: &str) -> String {
    let u: Vec<u16> = c.encode_utf16().collect();
    // indexOf -1 makes slice(4, -1): up to the last unit
    let end = match u.iter().position(|&x| x == u16::from(b'|')) {
        Some(i) => i,
        None => u.len().saturating_sub(1),
    };
    String::from_utf16_lossy(u.get(4..end).unwrap_or(&[]))
}
/// `paintHex(c)` with the app's colour lookups (`material(id)`, `image(id)`; None = not registered).
pub fn paint_hex(c: &str, material: &dyn Fn(&str) -> Option<String>, image: &dyn Fn(&str) -> Option<String>) -> String {
    if is_material_paint(c) {
        return material(c.get(4..).unwrap_or("")).filter(|s| !s.is_empty()).unwrap_or_else(|| "#c8c4bc".into());
    }
    if is_image_paint(c) {
        let tail = c.rfind('|').and_then(|i| c.get(i + 1..)).unwrap_or("");
        if !tail.is_empty() {
            return tail.to_string();
        }
        return image(&image_id(c)).filter(|s| !s.is_empty()).unwrap_or_else(|| "#c8c4bc".into());
    }
    c.to_string()
}
/// Bytes → base64.
pub fn to_base64(u8s: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let ch = |n: u32| char::from(A.get(usize::try_from(n & 63).unwrap_or(0)).copied().unwrap_or(b'A'));
    let mut out = String::new();
    for c in u8s.chunks(3) {
        let a = u32::from(c.first().copied().unwrap_or(0));
        let b = c.get(1).map(|&x| u32::from(x));
        let cc = c.get(2).map(|&x| u32::from(x));
        let n = (a << 16) | (b.unwrap_or(0) << 8) | cc.unwrap_or(0);
        out.push(ch(n >> 18));
        out.push(ch(n >> 12));
        out.push(if b.is_none() { '=' } else { ch(n >> 6) });
        out.push(if cc.is_none() { '=' } else { ch(n) });
    }
    out
}
/// `r7(x)`: x to 7 significant digits (−0 → 0).
pub fn r7(x: f64) -> f64 {
    let s = js::to_precision(x, 7);
    let v = if x.is_finite() { s.parse::<f64>().unwrap_or(f64::NAN) } else { x };
    if v == 0.0 { 0.0 } else { v }
}
/// The paint for image `id` placed by u = U·p + U[3], v = V·p + V[3]; `avg` its colour from afar.
pub fn image_paint(id: &str, u: &[f64], v: &[f64], avg: Option<&str>) -> String {
    let nums: Vec<String> = u.iter().chain(v.iter()).map(|&x| js::num_str(r7(x))).collect();
    let a = avg.filter(|a| is_color(a)).map(str::to_ascii_lowercase).unwrap_or_default();
    format!("img:{id}|{}|{a}", nums.join(","))
}
/// Where point p falls on an image paint's texture: [u, v] in copies of the image.
pub fn image_uv(c: &str, p: P3) -> Res<[f64; 2]> {
    let Some(mid) = c.split('|').nth(1) else { return Err(Error::Type) };
    let k: Vec<f64> = mid.split(',').map(str_number).collect();
    let g = |i: usize| k.get(i).copied().unwrap_or(f64::NAN);
    Ok([g(0) * p[0] + g(1) * p[1] + g(2) * p[2] + g(3), g(4) * p[0] + g(5) * p[1] + g(6) * p[2] + g(7)])
}
/// `Number(s)` for the decimal numbers an image paint holds ('' → 0, junk → NaN).
fn str_number(s: &str) -> f64 {
    let t = js::trim(s);
    if t.is_empty() {
        return 0.0;
    }
    match t {
        "Infinity" | "+Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        _ if t.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-')) => t.parse::<f64>().unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}
/// The image paint that puts texture coordinates `uvs` at the points `pts` of a flat face (normal n). → None when
/// degenerate.
pub fn image_paint_fit(id: &str, pts: &[P3], uvs: &[[f64; 2]], n: P3, avg: Option<&str>) -> Option<String> {
    let np = pts.len();
    let uv = |i: usize| uvs.get(i).copied().unwrap_or([f64::NAN; 2]);
    let pt = |i: usize| pts.get(i).copied().unwrap_or([f64::NAN; 3]);
    let mut i = 0usize;
    while i + 2 < np {
        let mut j = i + 1;
        while j + 1 < np {
            for k in j + 1..np {
                let (pi, pj, pk) = (pt(i), pt(j), pt(k));
                let a = [[pi[0], pi[1], pi[2], 1.0], [pj[0], pj[1], pj[2], 1.0], [pk[0], pk[1], pk[2], 1.0], [n[0], n[1], n[2], 0.0]];
                // 4×4 Gaussian elimination with partial pivoting
                let solve = |b: [f64; 4]| -> Option<[f64; 4]> {
                    let mut m = [[0.0f64; 5]; 4];
                    for (q, row) in m.iter_mut().enumerate() {
                        let ar = a.get(q).copied().unwrap_or([0.0; 4]);
                        *row = [ar[0], ar[1], ar[2], ar[3], b.get(q).copied().unwrap_or(0.0)];
                    }
                    let g = |m: &[[f64; 5]; 4], r: usize, c: usize| m.get(r).and_then(|x| x.get(c)).copied().unwrap_or(f64::NAN);
                    for c in 0..4 {
                        let mut piv = c;
                        for r in c + 1..4 {
                            if g(&m, r, c).abs() > g(&m, piv, c).abs() {
                                piv = r;
                            }
                        }
                        if g(&m, piv, c).abs() < 1e-12 {
                            return None;
                        }
                        m.swap(c, piv);
                        let pr = m.get(c).copied().unwrap_or([f64::NAN; 5]);
                        for (r, row) in m.iter_mut().enumerate() {
                            if r != c {
                                let f = row.get(c).copied().unwrap_or(f64::NAN) / pr.get(c).copied().unwrap_or(f64::NAN);
                                for (x, p) in row.iter_mut().zip(pr.iter()).skip(c) {
                                    *x -= f * p;
                                }
                            }
                        }
                    }
                    Some([g(&m, 0, 4) / g(&m, 0, 0), g(&m, 1, 4) / g(&m, 1, 1), g(&m, 2, 4) / g(&m, 2, 2), g(&m, 3, 4) / g(&m, 3, 3)])
                };
                let u = solve([uv(i)[0], uv(j)[0], uv(k)[0], 0.0]);
                let v = solve([uv(i)[1], uv(j)[1], uv(k)[1], 0.0]);
                if let (Some(u), Some(v)) = (u, v)
                    && u.iter().chain(v.iter()).all(|x| x.is_finite())
                {
                    return Some(image_paint(id, &u, &v, avg));
                }
            }
            j += 1;
        }
        i += 1;
    }
    None
}
/// Solid colours offered in the Colors panel: [name, #rrggbb].
pub const PALETTE: [(&str, &str); 16] = [
    ("White", "#f4f4f1"),
    ("Light gray", "#c9c9c5"),
    ("Gray", "#8a8a86"),
    ("Charcoal", "#3b3b3b"),
    ("Red", "#c0392b"),
    ("Orange", "#e67e22"),
    ("Yellow", "#f1c40f"),
    ("Green", "#2e9d57"),
    ("Teal", "#138d85"),
    ("Sky", "#7fb8e6"),
    ("Blue", "#2b5fbd"),
    ("Purple", "#7d4ab0"),
    ("Pink", "#d9628f"),
    ("Brown", "#7a5236"),
    ("Wood", "#b98552"),
    ("Sand", "#d8c8a2"),
];
/// Paint faces with colour or material c (None = the default).
pub fn paint_faces(def: &mut Def, fids: &[f64], c: Option<&str>) -> Res<()> {
    if let Some(c) = c
        && !is_paint(c)
    {
        return Err(msg("m3d.model.that_is_not_a_colour"));
    }
    for &id in fids {
        let Some(f) = def.faces.get_mut(id) else { continue };
        f.color = c.map(str::to_ascii_lowercase);
    }
    Ok(())
}
/// Paint a placement: its faces without a colour of their own show c (None = the default).
pub fn paint_instance(def: &mut Def, iid: f64, c: Option<&str>) -> Res<()> {
    if let Some(c) = c
        && !is_paint(c)
    {
        return Err(msg("m3d.model.that_is_not_a_colour"));
    }
    if let Some(inst) = def.insts.get_mut(iid) {
        inst.color = c.map(str::to_ascii_lowercase);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paints() {
        assert!(is_color("#A0b0c0"));
        assert!(!is_color("#a0b0c"));
        assert!(is_material_paint("mat:oak-1"));
        assert!(!is_material_paint("mat:Oak"));
        assert!(is_image_paint("img:t1x|1,0,0,0,0,1,0,-2.5e-3|#aabbcc"));
        assert!(is_image_paint("IMG:T1|1,0,0,0,0,1,0,0|"));
        assert!(!is_image_paint("img:t1|1,0,0,0,0,1,0|"));
        assert!(!is_image_paint("img:t1|1.,0,0,0,0,1,0,0|"));
        assert_eq!(to_base64(b"hi!?"), "aGkhPw==");
        assert_eq!(image_id("img:abc|1"), "abc");
    }
}
