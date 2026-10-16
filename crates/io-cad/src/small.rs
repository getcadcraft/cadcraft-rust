//! The small helpers the importers share: gcons.js (CadCraft's xdata tags), xclip.js `clipFromFilter`, pictures.js
//! `pictureBlock`, sheets.js `matchPaper`.

use cc_cad::js;
use cc_cad_geom::jsmath::JsMath;
use cc_io_dwg::value::{Obj, V};

use crate::jsv::{self, Ov};

// ------------------------------------------------------------------------------------------------ gcons.js

pub const PARAM_BLOCK: &str = "CADCRAFT_PARAMETRIC";

/// `isParamBlock(name)`
pub fn is_param_block(name: Ov) -> bool {
    jsv::upper(&jsv::str_or_empty(name)) == PARAM_BLOCK
}

/// `/^PREFIX(\d{1,7}):(.*)$/s` on a string → (n, rest).
fn numbered<'a>(s: &'a str, prefix: &str) -> Option<(f64, &'a str)> {
    let rest = s.strip_prefix(prefix)?;
    let k = rest.find(':')?;
    let digits = rest.get(..k)?;
    if digits.is_empty() || digits.len() > 7 || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((js::str_to_number(digits), rest.get(k.saturating_add(1)..)?))
}

/// The pieces of a numbered payload in `cc[0]` of each entity, sorted by number (stable).
fn payload(ents: &[V], prefix: &str) -> Vec<String> {
    let mut parts: Vec<(f64, String)> = Vec::new();
    for e in ents {
        let cc = jsv::get(Some(e), "cc");
        if !matches!(cc, Some(V::Arr(_))) {
            continue;
        }
        let first = jsv::str_nn(jsv::at(cc, 0));
        if let Some((n, s)) = numbered(&first, prefix) {
            parts.push((n, s.to_string()));
        }
    }
    parts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    parts.into_iter().map(|p| p.1).collect()
}

/// `parametricText(ents)`: the payload text from the CADCRAFT_PARAMETRIC block's points.
pub fn parametric_text(ents: &[V]) -> String {
    payload(ents, "CCPARAM:").join("")
}

/// `read3dText(ents)`: CadCraft's 3D JSON from the CADCRAFT_3D block's points, or None.
pub fn read3d_text(ents: &[V]) -> Option<String> {
    let p = payload(ents, "CC3D:");
    if p.is_empty() { None } else { Some(p.join("")) }
}

/// `refKeyIn(strings)`: the CCREF key in CADCRAFT xdata strings.
pub fn ref_key_in(strings: &[V]) -> Option<String> {
    for s in strings {
        let st = jsv::to_str(Some(s));
        if let Some(d) = st.strip_prefix("CCREF:")
            && !d.is_empty()
            && d.len() <= 9
            && d.bytes().all(|c| c.is_ascii_digit())
        {
            return Some(d.to_string());
        }
    }
    None
}

/// `cadcraftString(xdata)`: the first string of an entity's CADCRAFT xdata, or None.
pub fn cadcraft_string(xdata: Ov) -> Option<String> {
    let x = jsv::obj(xdata)?;
    let k = jsv::keys_js(x).into_iter().find(|n| jsv::upper(n) == "CADCRAFT")?;
    let first = jsv::at(x.get(k), 0)?;
    if !matches!(first, V::Arr(_)) || jsv::num(jsv::at(Some(first), 0)) != Some(1000.0) {
        return None;
    }
    Some(jsv::str_nn(jsv::at(Some(first), 1)))
}

/// `tagFromXdata(conv, xdata)`: tag a converted entity with what CadCraft wrote in its CADCRAFT xdata.
pub fn tag_from_xdata(conv: &mut V, xdata: Ov) {
    let V::Obj(o) = conv else { return };
    let Some(first) = cadcraft_string(xdata) else { return };
    if let Some(key) = ref_key_in(&[V::Str(first.clone())]) {
        o.set("ccref", key);
    }
    if first.starts_with("CCPARAM:") {
        o.set("cc", V::Arr(vec![V::Str(first)]));
    }
}

// ------------------------------------------------------------------------------------------------ xclip.js

/// `clipFromFilter(o)`: a SPATIAL_FILTER → { pts } in block coordinates, or None.
pub fn clip_from_filter(o: Ov) -> Option<V> {
    let ps: Vec<(f64, f64)> = jsv::arr(jsv::get(o, "points"))
        .iter()
        .filter_map(|p| Some((jsv::fin(jsv::get(Some(p), "x"))?, jsv::fin(jsv::get(Some(p), "y"))?)))
        .collect();
    if ps.len() < 2 {
        return None;
    }
    let mat = |a: Ov| -> [f64; 12] {
        let id = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let list = jsv::arr(a);
        if !matches!(a, Some(V::Arr(_))) || list.len() != 12 {
            return id;
        }
        let mut m = [0.0; 12];
        for (slot, v) in m.iter_mut().zip(list) {
            match jsv::fin(Some(v)) {
                Some(x) => *slot = x,
                None => return id,
            }
        }
        m
    };
    let t = mat(jsv::get(o, "clipTransform"));
    let i = mat(jsv::get(o, "inverseBlockTransform"));
    let ap = |m: &[f64; 12], p: (f64, f64)| (m[0] * p.0 + m[1] * p.1 + m[3], m[4] * p.0 + m[5] * p.1 + m[7]);
    let pts: Vec<(f64, f64)> = match ps.as_slice() {
        [a, b] => vec![*a, (b.0, a.1), *b, (a.0, b.1)],
        _ => ps.clone(),
    };
    let out: Vec<V> = pts.into_iter().map(|p| ap(&i, ap(&t, p))).map(|(x, y)| jsv::p2(x, y)).collect();
    let mut r = Obj::new();
    r.set("pts", V::Arr(out));
    Some(V::Obj(r))
}

// ------------------------------------------------------------------------------------------------ pictures.js

/// `picCorners(pic)`: lower-left, lower-right, upper-right, upper-left.
pub fn pic_corners(pic: &Obj) -> [(f64, f64); 4] {
    let n = |k: &str| jsv::to_num(pic.get(k));
    let (x, y, w, h) = (n("x"), n("y"), n("w"), n("h"));
    let (wx, wy) = (n("ux") * w, n("uy") * w);
    let (hx, hy) = (n("vx") * h, n("vy") * h);
    [(x, y), (x + wx, y + wy), (x + wx + hx, y + wy + hy), (x + hx, y + hy)]
}

/// `pictureBlock(name, pic, clip)`: the block for a picture — frame + the file's name.
pub fn picture_block(name: &str, pic: Obj, clip: Option<&[V]>) -> Obj {
    let c = pic_corners(&pic);
    let clipped = clip.is_some_and(|c| c.len() >= 3);
    let pts: Vec<V> = match clip.filter(|c| c.len() >= 3) {
        Some(cl) => cl
            .iter()
            .map(|p| {
                let mut o = Obj::new();
                o.set("x", jsv::get(Some(p), "x").cloned().unwrap_or(V::Num(f64::NAN)));
                o.set("y", jsv::get(Some(p), "y").cloned().unwrap_or(V::Num(f64::NAN)));
                o.set("b", 0.0);
                V::Obj(o)
            })
            .collect(),
        None => c.iter().map(|(x, y)| V::Obj(cc_io_dwg::obj! { "x" => *x, "y" => *y, "b" => 0.0 })).collect(),
    };
    let w_len = js::hypot(c[1].0 - c[0].0, c[1].1 - c[0].1);
    let h_len = js::hypot(c[3].0 - c[0].0, c[3].1 - c[0].1);
    let rot = (c[1].1 - c[0].1).jatan2(c[1].0 - c[0].0);
    let full = jsv::str_or_empty(pic.get("file"));
    let base = full.rsplit(['\\', '/']).next().unwrap_or("").to_string();
    let file = if base.is_empty() { "image".to_string() } else { base };
    let th = js::max(1e-9, js::min(h_len / 8.0, w_len / js::max(4.0, jsv::us2f(crate::text::len16(&file)))));
    let k = th * 0.5;
    let tau = 2.0 * std::f64::consts::PI;
    let mut picv = pic;
    if clipped {
        picv.set("clipped", true);
    }
    let mut frame = Obj::new();
    frame.set("type", "poly");
    frame.set("layer", "0");
    frame.set("color", 0.0);
    frame.set("closed", true);
    frame.set("pts", V::Arr(pts));
    frame.set("pic", picv);
    let mut text = Obj::new();
    text.set("type", "text");
    text.set("layer", "0");
    text.set("color", 0.0);
    text.set("x", c[0].0 + (rot.jcos() - rot.jsin()) * k);
    text.set("y", c[0].1 + (rot.jsin() + rot.jcos()) * k);
    text.set("h", th);
    text.set("rot", ((rot % tau) + tau) % tau);
    text.set("s", file);
    text.set("ha", "left");
    text.set("va", "baseline");
    text.set("picName", true);
    let mut b = Obj::new();
    b.set("name", name);
    b.set("base", jsv::p2(0.0, 0.0));
    b.set("entities", V::Arr(vec![V::Obj(frame), V::Obj(text)]));
    b
}

// ------------------------------------------------------------------------------------------------ sheets.js

const PAPERS: [(&str, f64, f64, &str); 13] = [
    ("iso-a4", 297.0, 210.0, "mm"),
    ("iso-a3", 420.0, 297.0, "mm"),
    ("iso-a2", 594.0, 420.0, "mm"),
    ("iso-a1", 841.0, 594.0, "mm"),
    ("iso-a0", 1189.0, 841.0, "mm"),
    ("letter", 11.0, 8.5, "in"),
    ("legal", 14.0, 8.5, "in"),
    ("ansi-b", 17.0, 11.0, "in"),
    ("ansi-c", 22.0, 17.0, "in"),
    ("ansi-d", 34.0, 22.0, "in"),
    ("arch-c", 24.0, 18.0, "in"),
    ("arch-d", 36.0, 24.0, "in"),
    ("arch-e", 48.0, 36.0, "in"),
];

/// sheets.js UNIT_MM.
pub fn unit_mm(u: &str) -> Option<f64> {
    Some(match u {
        "millimeters" | "mm" => 1.0,
        "centimeters" => 10.0,
        "meters" => 1000.0,
        "kilometers" => 1e6,
        "inches" | "in" => 25.4,
        "feet" => 304.8,
        "yards" => 914.4,
        "miles" => 1_609_344.0,
        "microinches" => 25.4e-6,
        "mils" => 0.0254,
        _ => return None,
    })
}

/// `matchPaper(w, h, unit)`: the standard paper of w × h (either way round, within 0.5 %), its key.
pub fn match_paper(w: f64, h: f64, unit: Ov) -> Option<&'static str> {
    let k = jsv::s(unit).and_then(unit_mm).filter(|k| *k != 0.0).unwrap_or(1.0);
    let a = js::max(w, h) * k;
    let b = js::min(w, h) * k;
    PAPERS
        .iter()
        .find(|p| {
            let pk = unit_mm(p.3).unwrap_or(1.0);
            (p.1 * pk - a).abs() <= a * 0.005 && (p.2 * pk - b).abs() <= b * 0.005
        })
        .map(|p| p.0)
}

/// `Math.hypot(a, b, c)` as V8 computes it (Kahan-compensated, scaled by the largest).
pub fn hypot3(a: f64, b: f64, c: f64) -> f64 {
    let v = [a.abs(), b.abs(), c.abs()];
    if v.iter().any(|x| x.is_infinite()) {
        return f64::INFINITY;
    }
    if v.iter().any(|x| x.is_nan()) {
        return f64::NAN;
    }
    let max = v.iter().copied().fold(0.0f64, |m, x| if x > m { x } else { m });
    if max == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut comp = 0.0f64;
    for x in v {
        let n = x / max;
        let summand = n * n - comp;
        let pre = sum + summand;
        comp = (pre - sum) - summand;
        sum = pre;
    }
    sum.sqrt() * max
}

/// `Number(x.toPrecision(p))` (1 ≤ p ≤ 21): x rounded to p significant digits, half up on the exact value.
pub fn round_precision(x: f64, p: usize) -> f64 {
    if !x.is_finite() || x == 0.0 {
        return x;
    }
    match fast_round(x, p) {
        Some(v) => v,
        None => js::str_to_number(&js::to_exponential(x, p.saturating_sub(1))),
    }
}

/// `Number(x.toPrecision(p))` without the exact 1100-digit expansion `js::to_exponential` makes, from the shortest
/// round-trip digits s of x (Rust's `{:e}`). Rounding s at digit p gives the same p digits as rounding the exact value:
/// a rounding midpoint m (p + 1 digits, ending in 5) strictly between s and x would be a decimal at most as long as s
/// and closer to x, which the shortest digits rule out; so only s = m (k = p + 1 digits ending in 5) is ambiguous. When
/// s has at most p digits, the p-digit rounding r is at least as close to x as s, so r reads back as x — except next to
/// a power of two, where the interval below x is half as wide. The ambiguous cases return None (the exact path).
fn fast_round(x: f64, p: usize) -> Option<f64> {
    if !(1..=21).contains(&p) {
        return None;
    }
    let s = format!("{:e}", x.abs());
    let (mant, exp) = s.split_once('e')?;
    let digits: Vec<u8> = mant.bytes().filter(u8::is_ascii_digit).collect();
    let e: i32 = exp.parse().ok()?;
    if digits.len() <= p {
        // a power of two (no mantissa bits): the interval is lopsided
        return (x.to_bits() & 0x000f_ffff_ffff_ffff != 0).then_some(x);
    }
    let first = *digits.get(p)?;
    if first == b'5' && digits.len() == p.saturating_add(1) {
        return None;
    }
    let mut kept: Vec<u8> = digits.get(..p)?.to_vec();
    let mut e = e;
    if first >= b'5' {
        let mut i = kept.len();
        loop {
            if i == 0 {
                kept.insert(0, b'1');
                kept.truncate(p);
                e = e.checked_add(1)?;
                break;
            }
            i = i.checked_sub(1)?;
            let d = kept.get_mut(i)?;
            if *d == b'9' {
                *d = b'0';
            } else {
                *d = d.checked_add(1)?;
                break;
            }
        }
    }
    let mut t = String::with_capacity(p.saturating_add(8));
    if x < 0.0 {
        t.push('-');
    }
    t.push(char::from(*kept.first()?));
    t.push('.');
    t.extend(kept.get(1..)?.iter().map(|d| char::from(*d)));
    t.push('e');
    t.push_str(&e.to_string());
    t.parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn papers_and_numbers() {
        assert_eq!(match_paper(297.0, 210.0, Some(&V::from("mm"))), Some("iso-a4"));
        assert_eq!(match_paper(8.5, 11.0, Some(&V::from("in"))), Some("letter"));
        assert_eq!(match_paper(100.0, 50.0, Some(&V::from("mm"))), None);
        assert_eq!(round_precision(0.1 + 0.2, 15), 0.3);
        // the fast path agrees with the exact one (ties, carries, tiny and huge values)
        let mut x = 0.123_456_789_012_345_6_f64;
        for k in 0..1_500u32 {
            for v in [
                x,
                -x,
                x * 1e-300,
                x * 1e300,
                f64::from(k) / 8.0,
                f64::from(k) * 1.000_000_000_000_05,
                9.999_999_999_999_995 * f64::from(k + 1),
            ] {
                for p in [12usize, 15] {
                    let exact = if v == 0.0 { v } else { js::str_to_number(&js::to_exponential(v, p - 1)) };
                    assert_eq!(round_precision(v, p).to_bits(), exact.to_bits(), "{v:e} at {p}");
                }
            }
            x = (x * 7.31 + 0.137).fract() * 10f64.powi(i32::try_from(k % 30).unwrap() - 15);
        }
        assert_eq!(hypot3(2.0, 3.0, 6.0), 7.0);
    }

    #[test]
    fn fast_round_matches_the_exact_path_on_random_doubles() {
        // xorshift64 over raw bit patterns (every exponent), plus drawing-like coordinates with 16-17 digits
        let mut st: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = || {
            st ^= st << 13;
            st ^= st >> 7;
            st ^= st << 17;
            st
        };
        let n = if cfg!(debug_assertions) { 20_000 } else { 200_000 };
        for i in 0..n {
            let r = next();
            let v = if i % 2 == 0 { f64::from_bits(r) } else { (r >> 11) as f64 / 9_007_199_254_740_992.0 * 10f64.powi((i % 12) as i32) };
            if !v.is_finite() || v == 0.0 {
                continue;
            }
            for p in [12usize, 15] {
                let exact = js::str_to_number(&js::to_exponential(v, p - 1));
                assert_eq!(round_precision(v, p).to_bits(), exact.to_bits(), "{v:e} at {p}");
            }
        }
    }
}
