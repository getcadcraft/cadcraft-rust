//! Test helpers: a small PDF writer for self-generated fixtures (pages with content streams, page labels, attachments),
//! a zip reader for the XLSX, and the takeoff the frozen Node test builds in "Quantities, wall area, waste and cost".
#![allow(dead_code)]

use cc_takeoff::jv::Obj;
use cc_takeoff::model::Takeoff;
use cc_takeoff::units as u;
use serde_json::{Value, json};
use std::fmt::Write as _;

// ------------------------------------------------------------------------------------------------------------ PDF

/// One page: size, /Rotate and its content stream (PDF operators, uncompressed). Font /F1 is Helvetica.
pub struct Page {
    pub w: f64,
    pub h: f64,
    pub rotate: i32,
    pub content: String,
}
impl Page {
    pub fn new(w: f64, h: f64, content: impl Into<String>) -> Self {
        Page { w, h, rotate: 0, content: content.into() }
    }
}

fn pdf_string(s: &str) -> String {
    let mut o = String::from("(");
    for c in s.chars() {
        match c {
            '(' | ')' | '\\' => {
                o.push('\\');
                o.push(c);
            }
            _ => o.push(c),
        }
    }
    o.push(')');
    o
}

/// A PDF: pages, extra catalog entries (raw PDF, e.g. "/PageLabels << ... >>") and attachments (name, bytes).
pub fn pdf(pages: &[Page], catalog_extra: &str, attachments: &[(&str, &[u8])]) -> Vec<u8> {
    let mut objs: Vec<Vec<u8>> = Vec::new();
    let add = |objs: &mut Vec<Vec<u8>>, body: Vec<u8>| {
        objs.push(body);
        objs.len()
    };
    // 1 catalog, 2 pages, 3 font: fixed numbers, filled in at the end
    for _ in 0..3 {
        add(&mut objs, Vec::new());
    }
    objs[2] = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec();
    let mut kids = Vec::new();
    for p in pages {
        let mut st = format!("<< /Length {} >>\nstream\n", p.content.len()).into_bytes();
        st.extend_from_slice(p.content.as_bytes());
        st.extend_from_slice(b"\nendstream");
        let c = add(&mut objs, st);
        let body = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Rotate {} /Resources << /Font << /F1 3 0 R >> >> /Contents {c} 0 R >>",
            p.w, p.h, p.rotate
        );
        kids.push(add(&mut objs, body.into_bytes()));
    }
    objs[1] = format!(
        "<< /Type /Pages /Kids [{}] /Count {} >>",
        kids.iter().map(|k| format!("{k} 0 R")).collect::<Vec<_>>().join(" "),
        kids.len()
    )
    .into_bytes();
    let mut names = String::new();
    for (name, data) in attachments {
        let mut st = format!("<< /Type /EmbeddedFile /Length {} >>\nstream\n", data.len()).into_bytes();
        st.extend_from_slice(data);
        st.extend_from_slice(b"\nendstream");
        let ef = add(&mut objs, st);
        let spec = add(
            &mut objs,
            format!("<< /Type /Filespec /F {} /UF {} /EF << /F {ef} 0 R >> >>", pdf_string(name), pdf_string(name)).into_bytes(),
        );
        let _ = write!(names, "{} {spec} 0 R ", pdf_string(name));
    }
    let mut cat = format!("<< /Type /Catalog /Pages 2 0 R {catalog_extra}");
    if !attachments.is_empty() {
        let _ = write!(cat, " /Names << /EmbeddedFiles << /Names [{names}] >> >>");
    }
    cat.push_str(" >>");
    objs[0] = cat.into_bytes();
    let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offs = Vec::new();
    for (i, b) in objs.iter().enumerate() {
        offs.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(b);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offs {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

/// The symbol of the frozen test's generated sheets, as pdf-lib's drawSvgPath draws
/// 'M0,0 L30,0 L30,-18 L0,-18 Z M4,-4 L14,-14 M20,-4 L26,-4 L26,-10 Z' at (x, y), turned `rot`°, scaled `s`.
pub fn symbol(x: f64, y: f64, rot: f64, s: f64, width: f64, triangle: bool) -> String {
    let (c, sn) = (rot.to_radians().cos(), rot.to_radians().sin());
    let tri = if triangle { " 20 4 m 26 4 l 26 10 l h" } else { "" };
    format!(
        "q 1 0 0 1 {x} {y} cm {c:.9} {sn:.9} {:.9} {c:.9} 0 0 cm {s} 0 0 {s} 0 0 cm {width} w 0 0 0 RG 0 0 m 30 0 l 30 18 l 0 18 l h 4 4 m 14 14 l{tri} S Q\n",
        -sn
    )
}
/// A centre of a symbol (15, 9 in its own frame).
pub fn symbol_centre(x: f64, y: f64, rot: f64, s: f64) -> (f64, f64) {
    let (c, sn) = (rot.to_radians().cos(), rot.to_radians().sin());
    (x + s * (15.0 * c - 9.0 * sn), y + s * (15.0 * sn + 9.0 * c))
}
fn circle(cx: f64, cy: f64, r: f64) -> String {
    let k = 0.552_284_749_8 * r;
    format!(
        "{} {cy} m {} {} {} {} {cx} {} c {} {} {} {} {} {cy} c {} {} {} {} {cx} {} c {} {} {} {} {} {cy} c S\n",
        cx + r,
        cx + r,
        cy + k,
        cx + k,
        cy + r,
        cy + r,
        cx - k,
        cy + r,
        cx - r,
        cy + k,
        cx - r,
        cx - r,
        cy - k,
        cx - k,
        cy - r,
        cy - r,
        cx + k,
        cy - r,
        cx + r,
        cy - k,
        cx + r
    )
}

/// The frozen test's stamp sheet: 16 stamps (4 at each of 0/90/180/270°), boxes, near-duplicates (no triangle),
/// circles and a line of text. → bytes and the 16 centres.
pub fn stamp_sheet() -> (Vec<u8>, Vec<(f64, f64)>) {
    let mut c = String::new();
    let mut centres = Vec::new();
    for (j, r) in [0.0, 90.0, 180.0, 270.0].iter().enumerate() {
        for i in 0..4 {
            let (x, y) = (90.0 + f64::from(i) * 160.0 + j as f64 * 17.0, 110.0 + j as f64 * 120.0);
            c.push_str(&symbol(x, y, *r, 1.0, 1.2, true));
            centres.push(symbol_centre(x, y, *r, 1.0));
        }
    }
    for i in 0..4 {
        let fi = f64::from(i);
        let _ = writeln!(c, "1.2 w 0 0 0 RG {} 40 30 18 re S", 150.0 + fi * 170.0);
        c.push_str(&symbol(165.0 + fi * 170.0, 560.0, 0.0, 1.0, 1.2, false));
        c.push_str("1.2 w 0 0 0 RG\n");
        c.push_str(&circle(760.0, 100.0 + fi * 120.0, 12.0));
    }
    c.push_str("BT /F1 10 Tf 300 590 Td (Symbol legend: see sheet A-001) Tj ET\n");
    (pdf(&[Page::new(792.0, 612.0, c)], "", &[]), centres)
}

// ------------------------------------------------------------------------------------------------------------ zip

/// The entries of a zip (local headers in order; stored or deflated).
pub fn unzip(b: &[u8]) -> Vec<(String, Vec<u8>)> {
    let u16at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]) as usize;
    let u32at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let mut out = Vec::new();
    let mut i = 0;
    while i + 30 <= b.len() && u32at(i) == 0x0403_4b50 {
        let method = u16at(i + 8);
        let crc = u32at(i + 14);
        let csize = u32at(i + 18) as usize;
        let usize_ = u32at(i + 22) as usize;
        let (nl, xl) = (u16at(i + 26), u16at(i + 28));
        let name = String::from_utf8(b[i + 30..i + 30 + nl].to_vec()).unwrap();
        let start = i + 30 + nl + xl;
        let raw = &b[start..start + csize];
        let data = match method {
            0 => raw.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec(raw).unwrap(),
            m => panic!("method {m}"),
        };
        assert_eq!(data.len(), usize_, "{name}: size");
        assert_eq!(cc_io_takeoff::xlsx::crc32(&data), crc, "{name}: CRC");
        out.push((name, data));
        i = start + csize;
    }
    out
}

// ------------------------------------------------------------------------------------------------------------ takeoffs

pub fn o(v: Value) -> Obj {
    v.as_object().cloned().unwrap_or_default()
}
pub fn rate(s: &str) -> f64 {
    u::parse_rate(s).unwrap()
}
pub fn rect(x: f64, y: f64, w: f64, h: f64) -> Value {
    json!([{ "x": x, "y": y }, { "x": x + w, "y": y }, { "x": x + w, "y": y + h }, { "x": x, "y": y + h }])
}
pub fn preset(k: &str, label: &str) -> Value {
    json!({ "method": "preset", "preset": k, "ftPerUnit": u::preset_ft_per_unit(k, 1.0).unwrap(), "label": label, "cal": null })
}
pub fn item(t: &mut Takeoff, v: Value) -> String {
    t.add_item(&o(v)).unwrap().unwrap()
}
pub fn mark(t: &mut Takeoff, v: Value) -> String {
    t.add_mark(&o(v)).unwrap().unwrap()
}

/// The takeoff of "Quantities, wall area, waste and cost" (job total $1,597.87, one formula error, one unscaled mark).
pub fn kitchen_style() -> Takeoff {
    let mut t = Takeoff::default();
    let hash = "a".repeat(64);
    t.add_file(&o(json!({ "hash": hash, "name": "set.pdf", "size": 100, "pages": 2 })));
    let s0 = cc_takeoff::model::sheet_key(&hash, 0.0);
    let s1 = cc_takeoff::model::sheet_key(&hash, 1.0);
    t.set_scale(&s0, &preset("a1/4", "1/4\" = 1'-0\""), None).unwrap();
    let wall = item(
        &mut t,
        json!({ "name": "Partition type A", "type": "linear", "heightFt": 9, "waste": 1000, "mat": rate("2.10"), "lab": rate("3.333") }),
    );
    mark(&mut t, json!({ "item": wall, "sheet": s0, "pts": [{ "x": 0, "y": 0 }, { "x": 36, "y": 0 }, { "x": 36, "y": 54 }] }));
    mark(&mut t, json!({ "item": wall, "sheet": s0, "pts": [{ "x": 100, "y": 100 }, { "x": 280, "y": 100 }] }));
    t.add_component(&wall, &o(json!({ "name": "Studs 16\" o.c.", "unit": "EA", "formula": "ceil(L / 1.333) + N", "mat": rate("4.25") })))
        .unwrap();
    t.add_component(&wall, &o(json!({ "name": "Gypsum board 4x8", "unit": "sheets", "formula": "ceil(A * 2 / 32)", "waste": 1000, "mat": rate("14.98"), "lab": rate("22") }))).unwrap();
    t.add_component(&wall, &o(json!({ "name": "Broken", "unit": "EA", "formula": "L / (H - 9)", "mat": 10000 }))).unwrap();
    let floor = item(
        &mut t,
        json!({ "name": "VCT flooring", "type": "area", "unit": "SF", "waste": 500, "mat": rate("1.85"), "lab": rate("1.1") }),
    );
    mark(&mut t, json!({ "item": floor, "sheet": s0, "pts": rect(0.0, 0.0, 360.0, 180.0), "holes": [rect(36.0, 36.0, 36.0, 36.0)] }));
    t.add_component(&floor, &o(json!({ "name": "Base cove", "unit": "LF", "formula": "P", "mat": rate("0.89") }))).unwrap();
    let sy = item(&mut t, json!({ "name": "Carpet", "type": "area", "unit": "SY", "mat": rate("27") }));
    mark(&mut t, json!({ "item": sy, "sheet": s0, "pts": rect(0.0, 0.0, 54.0, 54.0) }));
    let dev = item(&mut t, json!({ "name": "Duplex outlets", "type": "count", "lab": rate("45"), "mat": rate("3.5") }));
    for i in 0..7 {
        mark(&mut t, json!({ "item": dev, "sheet": s0, "pts": [{ "x": f64::from(i) * 10.0, "y": 5 }] }));
    }
    mark(&mut t, json!({ "item": dev, "sheet": s1, "pts": [{ "x": 1, "y": 1 }] }));
    mark(&mut t, json!({ "item": wall, "sheet": s1, "pts": [{ "x": 0, "y": 0 }, { "x": 100, "y": 0 }] }));
    t
}
