//! pdf.js — a vector PDF 1.4 written by hand (one page per display list): Helvetica / Helvetica-Bold in
//! WinAnsiEncoding, content streams Flate-compressed when asked. `textWidth` (Helvetica metrics, bold ~5% wider).

use crate::display::{Item, List};
use crate::js::{cos, num_to_string, sin, to_fixed};

const K: f64 = 72.0 / 25.4;

/// `num(v)`: 4 decimals, trailing zeros dropped; non-finite → "0".
pub fn num(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let s = fixed4(if v.abs() < 5e-5 { 0.0 } else { v });
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

/// `x.toFixed(4)` without the exact 1100-digit expansion: from a correctly rounded 20-digit form, which can only
/// mislead when it sits on the rounding boundary (…5000… or …4999…); those few go the exact way.
fn fixed4(x: f64) -> String {
    let a = x.abs();
    // the common case (paper coordinates): a × 10⁴ is off by at most 1e-6 below 1e6, so a fraction clear of ½
    // rounds the same as the exact value
    if a < 1e6 {
        let y = a * 1e4;
        let f = y.floor();
        let frac = y - f;
        if (frac - 0.5).abs() > 1e-4 {
            let n = crate::display::sat_i64(if frac > 0.5 { f + 1.0 } else { f });
            let sign = if x < 0.0 { "-" } else { "" };
            return format!("{sign}{}.{:04}", n / 10000, n % 10000);
        }
    }
    if a >= 1e15 {
        return to_fixed(x, 4);
    }
    let s20 = format!("{a:.20}");
    let Some((ip, fp)) = s20.split_once('.') else { return to_fixed(x, 4) };
    let (Some(head), Some(&d5), Some(rest)) = (fp.get(..4), fp.as_bytes().get(4), fp.get(5..)) else { return to_fixed(x, 4) };
    if (d5 == b'5' && rest.bytes().all(|b| b == b'0')) || (d5 == b'4' && rest.bytes().all(|b| b == b'9')) {
        return to_fixed(x, 4);
    }
    let mut digits: Vec<u8> = ip.bytes().chain(head.bytes()).collect();
    if d5 >= b'5' {
        let mut carry = true;
        for d in digits.iter_mut().rev() {
            if *d == b'9' {
                *d = b'0';
            } else {
                *d = d.saturating_add(1);
                carry = false;
                break;
            }
        }
        if carry {
            digits.insert(0, b'1');
        }
    }
    let n = digits.len().saturating_sub(4);
    let mut out = String::with_capacity(digits.len().saturating_add(2));
    if x < 0.0 {
        out.push('-');
    }
    out.push_str(std::str::from_utf8(digits.get(..n).unwrap_or(&[])).unwrap_or("0"));
    out.push('.');
    out.push_str(std::str::from_utf8(digits.get(n..).unwrap_or(&[])).unwrap_or("0000"));
    out
}

/// `CP1252[ch]`.
pub fn cp1252(c: char) -> Option<u32> {
    Some(match c {
        '€' => 0x80,
        '‚' => 0x82,
        'ƒ' => 0x83,
        '„' => 0x84,
        '…' => 0x85,
        '†' => 0x86,
        '‡' => 0x87,
        'ˆ' => 0x88,
        '‰' => 0x89,
        'Š' => 0x8a,
        '‹' => 0x8b,
        'Œ' => 0x8c,
        'Ž' => 0x8e,
        '‘' => 0x91,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '•' => 0x95,
        '–' => 0x96,
        '—' => 0x97,
        '˜' => 0x98,
        '™' => 0x99,
        'š' => 0x9a,
        '›' => 0x9b,
        'œ' => 0x9c,
        'ž' => 0x9e,
        'Ÿ' => 0x9f,
        _ => return None,
    })
}

/// `winAnsi(s)` → (escaped literal, characters with no glyph).
pub fn win_ansi(s: &str) -> (String, usize) {
    let mut out = String::with_capacity(s.len());
    let mut missing = 0usize;
    for ch in s.chars() {
        let ch = cc_cad_geom::font::subst(ch);
        let mut code = u32::from(ch);
        if let Some(c) = cp1252(ch) {
            code = c;
        } else if code > 0xff || (0x7f..0xa0).contains(&code) || code < 0x20 {
            code = 0x3f;
            missing = missing.saturating_add(1);
        }
        if code == 0x28 || code == 0x29 || code == 0x5c {
            out.push('\\');
            out.push(char::from_u32(code).unwrap_or('?'));
        } else if (0x20..0x7f).contains(&code) {
            out.push(char::from_u32(code).unwrap_or('?'));
        } else {
            out.push_str(&format!("\\{code:03o}"));
        }
    }
    (out, missing)
}

/// `textWidth(s, h, bold)` in mm.
pub fn text_width(s: &str, h: f64, bold: bool) -> f64 {
    cc_cad_geom::font::advance(s) / 1000.0 * h * if bold { 1.05 } else { 1.0 }
}

/// `contentStream(list)` → (text, missing).
pub fn content_stream(list: &List) -> (String, usize) {
    let mut out: Vec<String> = Vec::with_capacity(list.items.len().saturating_mul(2).saturating_add(2));
    let mut missing = 0usize;
    out.push(format!("{} 0 0 {} 0 0 cm", num(K), num(K)));
    out.push("1 J 1 j 0 0 0 RG 0 0 0 rg".into());
    let mut lw: Option<f64> = None;
    let mut dash = String::new();
    let mut pen = |out: &mut Vec<String>, w: f64, d: &Option<Vec<f64>>| {
        // `w !== lw`: NaN never equals
        if lw.is_none_or(|l| l != w) {
            out.push(format!("{} w", num(w)));
            lw = Some(w);
        }
        let ds = match d {
            Some(v) => format!("[{}] 0 d", v.iter().map(|x| num(*x)).collect::<Vec<_>>().join(" ")),
            None => "[] 0 d".into(),
        };
        if ds != dash {
            out.push(ds.clone());
            dash = ds;
        }
    };
    for it in &list.items {
        match it {
            Item::Path { pts, w, dash: d, closed, fill } => {
                if pts.len() < 2 {
                    continue;
                }
                pen(&mut out, *w, d);
                if let Some(p0) = pts.first() {
                    out.push(format!("{} {} m", num(p0[0]), num(p0[1])));
                }
                for p in pts.iter().skip(1) {
                    out.push(format!("{} {} l", num(p[0]), num(p[1])));
                }
                out.push(
                    if *fill {
                        "h f"
                    } else if *closed {
                        "h S"
                    } else {
                        "S"
                    }
                    .into(),
                );
            }
            Item::Circle { c, r, w, dash: d, fill } => {
                pen(&mut out, *w, d);
                let ([cx, cy], r) = (*c, *r);
                let k = 0.5522847498 * r;
                let j = |v: [f64; 6]| v.iter().map(|x| num(*x)).collect::<Vec<_>>().join(" ");
                out.push(format!("{} {} m", num(cx + r), num(cy)));
                out.push(format!("{} c", j([cx + r, cy + k, cx + k, cy + r, cx, cy + r])));
                out.push(format!("{} c", j([cx - k, cy + r, cx - r, cy + k, cx - r, cy])));
                out.push(format!("{} c", j([cx - r, cy - k, cx - k, cy - r, cx, cy - r])));
                out.push(format!("{} c", j([cx + k, cy - r, cx + r, cy - k, cx + r, cy])));
                out.push(if fill.unwrap_or(false) { "h f" } else { "h S" }.into());
            }
            Item::Text { s, x, y, h, align, rot, bold } => {
                let (enc, miss) = win_ansi(s);
                missing = missing.saturating_add(miss);
                let wdt = text_width(s, *h, *bold);
                let dx = match *align {
                    "center" => -wdt / 2.0,
                    "right" => -wdt,
                    _ => 0.0,
                };
                let r = if *rot == 0.0 || rot.is_nan() { 0.0 } else { *rot };
                let (c, sn) = (cos(r), sin(r));
                let (xx, yy) = (x + dx * c, y + dx * sn);
                let m = [h * c, h * sn, -h * sn, h * c, xx, yy].iter().map(|v| num(*v)).collect::<Vec<_>>().join(" ");
                out.push(format!("BT /{} 1 Tf {m} Tm ({enc}) Tj ET", if *bold { "F2" } else { "F1" }));
            }
        }
    }
    (format!("{}\n", out.join("\n")), missing)
}

/// The UTC time a PDF's CreationDate carries.
#[derive(Clone, Copy, Debug)]
pub struct Utc {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

/// `toPdf(lists, { title, compress })` → (bytes, missing).
pub fn to_pdf(lists: &[List], title: &str, compress: bool, now: Utc) -> (Vec<u8>, usize) {
    let mut pages = Vec::with_capacity(lists.len());
    let mut missing = 0usize;
    for l in lists {
        let (text, m) = content_stream(l);
        missing = missing.saturating_add(m);
        let raw = text.into_bytes();
        let (content, flate) = if compress { (miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6), true) } else { (raw, false) };
        pages.push((l.w, l.h, content, flate));
    }
    (assemble(&pages, title, now), missing)
}

fn assemble(pages: &[(f64, f64, Vec<u8>, bool)], title: &str, now: Utc) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut offsets: Vec<usize> = Vec::new();
    let obj = |out: &mut Vec<u8>, offsets: &mut Vec<usize>, body: &[&[u8]], n: usize| {
        if offsets.len() <= n {
            offsets.resize(n.saturating_add(1), 0);
        }
        if let Some(slot) = offsets.get_mut(n) {
            *slot = out.len();
        }
        out.extend(format!("{n} 0 obj\n").as_bytes());
        for b in body {
            out.extend(*b);
        }
        out.extend(b"\nendobj\n");
    };
    let pt = |mm: f64| num(mm * K);
    let pad = |x: u32| format!("{x:02}");
    let date = format!(
        "D:{}{}{}{}{}{}Z",
        num_to_string(now.year as f64),
        pad(now.month),
        pad(now.day),
        pad(now.hour),
        pad(now.minute),
        pad(now.second)
    );
    let page_obj = |i: usize| 6usize.saturating_add(i.saturating_mul(2));
    out.extend(b"%PDF-1.4\n");
    out.extend([0x25u8, 0xe2, 0xe3, 0xcf, 0xd3, 0x0a]);
    obj(&mut out, &mut offsets, &[b"<< /Type /Catalog /Pages 2 0 R >>"], 1);
    let kids = (0..pages.len()).map(|i| format!("{} 0 R", page_obj(i))).collect::<Vec<_>>().join(" ");
    obj(&mut out, &mut offsets, &[format!("<< /Type /Pages /Kids [{kids}] /Count {} >>", pages.len()).as_bytes()], 2);
    obj(&mut out, &mut offsets, &[b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"], 3);
    obj(&mut out, &mut offsets, &[b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>"], 4);
    obj(
        &mut out,
        &mut offsets,
        &[format!("<< /Title ({}) /Producer (DetailCraft) /Creator (DetailCraft) /CreationDate ({date}) >>", win_ansi(title).0).as_bytes()],
        5,
    );
    for (i, (w, h, content, flate)) in pages.iter().enumerate() {
        let n = page_obj(i);
        let head = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {} 0 R >>",
            pt(*w),
            pt(*h),
            n.saturating_add(1)
        );
        obj(&mut out, &mut offsets, &[head.as_bytes()], n);
        let sh = format!("<< /Length {}{} >>\nstream\n", content.len(), if *flate { " /Filter /FlateDecode" } else { "" });
        obj(&mut out, &mut offsets, &[sh.as_bytes(), content, b"\nendstream"], n.saturating_add(1));
    }
    let count = 6usize.saturating_add(pages.len().saturating_mul(2));
    let xref = out.len();
    let mut x = format!("xref\n0 {count}\n0000000000 65535 f \n");
    for o in offsets.iter().skip(1) {
        x.push_str(&format!("{o:010} 00000 n \n"));
    }
    out.extend(x.as_bytes());
    out.extend(format!("trailer\n<< /Size {count} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn fixed4_matches_exact() {
        let mut v = 0.1234f64;
        for i in 0..5_000u32 {
            v = (v * 7.31 + f64::from(i) * 0.000_037).rem_euclid(5000.0) - 2500.0;
            for x in [v, v / 1000.0, v.round() / 32.0, (v * 1e4).round() / 1e4 + 0.00005, -v / 7.0] {
                assert_eq!(super::fixed4(x), crate::js::to_fixed(x, 4), "{x}");
            }
        }
        for x in [0.03125, -0.03125, 0.00005, 0.00015, 1.00005, 9999.99995, 0.0, -0.0, 1e14 + 0.5] {
            assert_eq!(super::fixed4(x), crate::js::to_fixed(x, 4), "{x}");
        }
    }
}
