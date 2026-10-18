//! model-pdf.js — the plan of a level as a vector PDF (no library): walls filled grey with one heavy outline round
//! them, other cut elements lighter, what lies below in thin grey, doors and windows (arcs as short lines), room names
//! and areas, dimensions with ticks, a title strip with the scale. Helvetica in WinAnsiEncoding; characters outside it
//! print as "?" and are counted.

use crate::draw::m::{self, V2};
use crate::draw::plan::{Dim, Scene};
use crate::js::to_fixed;

const PT: f64 = 72.0 / 25.4;

/// Helvetica advance widths (1/1000 em), characters 32–126.
const HW: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278,
    278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722,
    667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556,
    556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

fn width(s: &str, size: f64) -> f64 {
    let mut w = 0.0;
    for ch in s.chars() {
        let mut b = [0u16; 2];
        let c = ch.encode_utf16(&mut b).first().copied().unwrap_or(0);
        w += if (32..=126).contains(&c) { f64::from(HW.get(usize::from(c.wrapping_sub(32))).copied().unwrap_or(556)) } else { 556.0 };
    }
    w / 1000.0 * size
}

/// Text as a PDF literal in WinAnsi; others → "?" (counted).
fn lit(s: &str) -> (String, usize) {
    let mut out = String::from("(");
    let mut missing = 0usize;
    for ch in s.chars() {
        let mapped: Option<u32> = match ch {
            '²' => Some(0xb2),
            '³' => Some(0xb3),
            '°' => Some(0xb0),
            '×' => Some(0xd7),
            '—' => Some(0x97),
            '–' => Some(0x96),
            '’' => Some(0x92),
            '“' => Some(0x93),
            '”' => Some(0x94),
            '…' => Some(0x85),
            _ => None,
        };
        let mut c = mapped.unwrap_or(u32::from(ch));
        if c > 0xff || ((0x7f..0xa0).contains(&c) && mapped.is_none()) || c < 0x20 {
            c = 0x3f;
            missing = missing.saturating_add(1);
        }
        if c == 0x28 || c == 0x29 || c == 0x5c {
            out.push('\\');
            out.push(char::from_u32(c).unwrap_or('?'));
        } else if (0x20..0x7f).contains(&c) {
            out.push(char::from_u32(c).unwrap_or('?'));
        } else {
            out.push_str(&format!("\\{c:03o}"));
        }
    }
    out.push(')');
    (out, missing)
}

/// `n(v)`: 3 decimals, trailing zeros off ("0" for tiny values).
fn n(v: f64) -> String {
    let s = to_fixed(if v.abs() < 5e-5 { 0.0 } else { v }, 3);
    let t = if let Some(dot) = s.find('.') {
        let tail = s.get(dot..).unwrap_or("");
        if tail.chars().skip(1).all(|c| c == '0') { s.get(..dot).unwrap_or("").to_string() } else { s.trim_end_matches('0').to_string() }
    } else {
        // no decimal point (to_fixed always has one for 3 places): trailing zeros of the integer go as the regex takes them
        s.trim_end_matches('0').to_string()
    };
    if t.is_empty() { "0".into() } else { t }
}

/// What planPdf made.
#[derive(Clone, Debug)]
pub struct PlanPdf {
    pub bytes: Vec<u8>,
    pub label: String,
    pub ratio: f64,
    pub missing: usize,
}

fn paper(p: &str) -> (f64, f64) {
    match p {
        "a4" => (297.0, 210.0),
        "tabloid" => (431.8, 279.4),
        "a3" => (420.0, 297.0),
        _ => (279.4, 215.9),
    }
}

fn scales(units: &str) -> Vec<(f64, &'static str)> {
    if units == "mm" {
        vec![(20.0, "1:20"), (25.0, "1:25"), (50.0, "1:50"), (100.0, "1:100"), (200.0, "1:200"), (500.0, "1:500"), (1000.0, "1:1000")]
    } else {
        vec![
            (16.0, "3/4\" = 1'-0\""),
            (24.0, "1/2\" = 1'-0\""),
            (32.0, "3/8\" = 1'-0\""),
            (48.0, "1/4\" = 1'-0\""),
            (64.0, "3/16\" = 1'-0\""),
            (96.0, "1/8\" = 1'-0\""),
            (192.0, "1/16\" = 1'-0\""),
            (384.0, "1/32\" = 1'-0\""),
        ]
    }
}

fn bounds(scene: &Scene) -> [f64; 4] {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    let mut g = |x: f64, y: f64| {
        if x < b[0] {
            b[0] = x;
        }
        if x > b[2] {
            b[2] = x;
        }
        if y < b[1] {
            b[1] = y;
        }
        if y > b[3] {
            b[3] = y;
        }
    };
    for w in &scene.walls {
        for q in &w.poly {
            g(q[0], q[1]);
        }
    }
    for c in scene.cut.iter().chain(scene.below.iter()) {
        for q in c.segs.chunks(2) {
            if let [x, y] = q {
                g(*x, *y);
            }
        }
    }
    for s in &scene.symbols {
        for q in &s.sym.poly {
            g(q[0], q[1]);
        }
    }
    for d in &scene.dims {
        g(d.a[0], d.a[1]);
        g(d.b[0], d.b[1]);
    }
    if !b[0].is_finite() {
        return [0.0, 0.0, 10000.0, 10000.0];
    }
    let pad = m::max(b[2] - b[0], b[3] - b[1]) * 0.06 + 600.0;
    [b[0] - pad, b[1] - pad, b[2] + pad, b[3] + pad]
}

/// `planPdf(scene, { title, lines, paper, units, scale })`
pub fn plan_pdf(scene: &Scene, title: &str, lines: &[String], paper_name: &str, units: &str, scale: Option<f64>) -> PlanPdf {
    let (pw, ph) = paper(paper_name);
    let (margin, strip) = (12.0, 16.0);
    let bb = bounds(scene);
    let (w, h) = (m::max(1.0, bb[2] - bb[0]), m::max(1.0, bb[3] - bb[1]));
    let room = [pw - 2.0 * margin, ph - 2.0 * margin - strip];
    let list = scales(units);
    let sc: (f64, String) = match scale {
        Some(r) => list.iter().find(|x| x.0 == r).map_or_else(|| (r, format!("1:{}", crate::js::num_str(r))), |x| (x.0, x.1.to_string())),
        None => match list.iter().find(|(r, _)| w / r <= room[0] && h / r <= room[1]) {
            Some(x) => (x.0, x.1.to_string()),
            None => {
                let r = m::max(w / room[0], h / room[1]).ceil();
                (r, format!("1:{}", crate::js::num_str(r)))
            }
        },
    };
    let k = 1.0 / sc.0;
    let ox = margin + (room[0] - w * k) / 2.0 - bb[0] * k;
    let oy = margin + strip + (room[1] - h * k) / 2.0 - bb[1] * k;
    let pp = |p: V2| [ox + p[0] * k, oy + p[1] * k];
    let mut ops: Vec<String> = vec!["1 J 1 j".into()];
    let mut missing = 0usize;
    let pen = |ops: &mut Vec<String>, wd: f64, gray: f64, dash: Option<[f64; 2]>| {
        let d = match dash {
            Some(d) => format!("[{}] 0 d", d.iter().map(|x| n(x * PT)).collect::<Vec<_>>().join(" ")),
            None => "[] 0 d".into(),
        };
        ops.push(format!("{} w {} G {d}", n(wd * PT), n(gray)));
    };
    let path = |ops: &mut Vec<String>, pts: &[V2], close: bool| {
        for (i, p) in pts.iter().enumerate() {
            let q = pp(*p);
            ops.push(format!("{} {}{}", n(q[0] * PT), n(q[1] * PT), if i > 0 { " l" } else { " m" }));
        }
        if close {
            ops.push("h".into());
        }
    };
    let seg = |ops: &mut Vec<String>, a: &[f64]| {
        for &[x0, y0, x1, y1] in a.as_chunks::<4>().0 {
            path(ops, &[[x0, y0], [x1, y1]], false);
        }
    };
    let text =
        |ops: &mut Vec<String>, missing: &mut usize, s: &str, x: f64, y: f64, size: f64, align: &str, rot: f64, bold: bool, gray: f64| {
            let (l, ms) = lit(s);
            *missing = missing.saturating_add(ms);
            let wd = width(s, size);
            let dx = match align {
                "center" => -wd / 2.0,
                "right" => -wd,
                _ => 0.0,
            };
            let (c, sn) = (m::cos(rot), m::sin(rot));
            let tx = x * PT + dx * c;
            let ty = y * PT + dx * sn;
            ops.push(format!(
                "BT /{} {} Tf {} g {} Tm {l} Tj ET",
                if bold { "F2" } else { "F1" },
                n(size),
                n(gray),
                [c, sn, -sn, c, tx, ty].iter().map(|v| n(*v)).collect::<Vec<_>>().join(" ")
            ));
        };
    // below the cut: thin grey
    pen(&mut ops, 0.13, 0.45, None);
    for b in &scene.below {
        seg(&mut ops, &b.segs);
    }
    ops.push("S".into());
    // other cut elements: light fill, outline
    for c in &scene.cut {
        if !c.loops.is_empty() {
            ops.push("0.75 g".into());
            for l in &c.loops {
                let pts: Vec<V2> = l.as_chunks::<2>().0.to_vec();
                path(&mut ops, &pts, true);
            }
            ops.push("f*".into());
        }
        pen(&mut ops, 0.25, 0.0, None);
        seg(&mut ops, &c.segs);
        ops.push("S".into());
    }
    // walls: grey fill, heavy outline
    for wl in &scene.walls {
        // `w.pieces || [w.poly]`: the scene's walls always carry pieces (an empty list draws nothing)
        for p in &wl.pieces {
            if p.is_empty() {
                continue;
            }
            ops.push("0.55 g".into());
            path(&mut ops, p, true);
            ops.push("f".into());
            if scene.wall_edges.is_none() {
                pen(&mut ops, 0.5, 0.0, None);
                path(&mut ops, p, true);
                ops.push("S".into());
            }
        }
    }
    if let Some(edges) = &scene.wall_edges {
        pen(&mut ops, 0.5, 0.0, None);
        for p in edges {
            path(&mut ops, p, true);
        }
        ops.push("S".into());
    }
    // doors, windows, roofs
    for s in &scene.symbols {
        let sy = &s.sym;
        for p in &sy.fill {
            ops.push("1 g".into());
            path(&mut ops, p, true);
            ops.push("f".into());
        }
        pen(&mut ops, 0.35, 0.0, None);
        let flat: Vec<f64> = sy.lines.iter().flat_map(|l| l.iter().copied()).collect();
        seg(&mut ops, &flat);
        ops.push("S".into());
        pen(&mut ops, 0.18, 0.0, None);
        for a in &sy.arcs {
            let steps = m::max(8.0, ((a.a1 - a.a0).abs() / (std::f64::consts::PI / 32.0)).ceil());
            let ns = if steps.is_finite() && steps < 1e6 { usize::try_from(crate::model::u32_of(steps)).unwrap_or(8) } else { 8 };
            let mut pts = Vec::with_capacity(ns.saturating_add(1));
            for i in 0..=ns {
                let t = a.a0 + (a.a1 - a.a0) * (i as f64) / steps;
                pts.push([a.c[0] + a.r * m::cos(t), a.c[1] + a.r * m::sin(t)]);
            }
            path(&mut ops, &pts, false);
        }
        let flat: Vec<f64> = sy.thin.iter().flat_map(|l| l.iter().copied()).collect();
        seg(&mut ops, &flat);
        ops.push("S".into());
        if !sy.dashed.is_empty() {
            pen(&mut ops, 0.18, 0.0, Some([1.5, 1.0]));
            let flat: Vec<f64> = sy.dashed.iter().flat_map(|l| l.iter().copied()).collect();
            seg(&mut ops, &flat);
            ops.push("S".into());
        }
    }
    // dimensions
    for d in &scene.dims {
        dim(&mut ops, &mut missing, d, k, &pp, &pen, &path, &text);
    }
    // room labels
    for r in &scene.rooms {
        if r.label.is_empty() {
            continue;
        }
        let q = pp(r.at);
        let nl = r.label.len() as f64;
        for (i, s) in r.label.iter().enumerate() {
            let fi = i as f64;
            text(
                &mut ops,
                &mut missing,
                s,
                q[0],
                q[1] - (fi - (nl - 1.0) / 2.0) * 3.4,
                if i > 0 { 6.5 } else { 7.5 },
                "center",
                0.0,
                i == 0,
                0.0,
            );
        }
    }
    // title strip
    pen(&mut ops, 0.35, 0.0, None);
    path(
        &mut ops,
        &[[(margin - ox) / k, (margin + strip - 2.0 - oy) / k], [(pw - margin - ox) / k, (margin + strip - 2.0 - oy) / k]],
        false,
    );
    ops.push("S".into());
    text(&mut ops, &mut missing, title, margin, margin + 7.0, 11.0, "left", 0.0, true, 0.0);
    if !lines.is_empty() {
        text(&mut ops, &mut missing, &lines.join("   "), margin, margin + 2.0, 7.0, "left", 0.0, false, 0.25);
    }
    text(&mut ops, &mut missing, &sc.1, pw - margin, margin + 7.0, 9.0, "right", 0.0, true, 0.0);
    PlanPdf { bytes: assemble(&ops.join("\n"), pw * PT, ph * PT, title), label: sc.1, ratio: sc.0, missing }
}

type PenFn<'a> = dyn Fn(&mut Vec<String>, f64, f64, Option<[f64; 2]>) + 'a;
type PathFn<'a> = dyn Fn(&mut Vec<String>, &[V2], bool) + 'a;
type TextFn<'a> = dyn Fn(&mut Vec<String>, &mut usize, &str, f64, f64, f64, &str, f64, bool, f64) + 'a;

#[allow(clippy::too_many_arguments)]
fn dim(
    ops: &mut Vec<String>,
    missing: &mut usize,
    d: &Dim,
    k: f64,
    pp: &dyn Fn(V2) -> V2,
    pen: &PenFn<'_>,
    path: &PathFn<'_>,
    text: &TextFn<'_>,
) {
    let l = m::hypot(d.b[0] - d.a[0], d.b[1] - d.a[1]);
    if l < 1.0 {
        return;
    }
    let u = [(d.b[0] - d.a[0]) / l, (d.b[1] - d.a[1]) / l];
    let nn = [-u[1], u[0]];
    let off = match d.off_mm {
        Some(o) => o / k,
        None => d.off.filter(|o| *o != 0.0 && !o.is_nan()).unwrap_or(0.0),
    };
    let a = [d.a[0] + nn[0] * off, d.a[1] + nn[1] * off];
    let b = [d.b[0] + nn[0] * off, d.b[1] + nn[1] * off];
    pen(ops, 0.13, 0.0, None);
    path(ops, &[a, b], false);
    let ext = 1.5 / k;
    let gap = 0.8 / k;
    let sgn = m::sign(off);
    let sg = if sgn != 0.0 && !sgn.is_nan() { sgn } else { 1.0 };
    path(ops, &[[d.a[0] + nn[0] * gap * sg, d.a[1] + nn[1] * gap * sg], [a[0] + nn[0] * ext * sg, a[1] + nn[1] * ext * sg]], false);
    path(ops, &[[d.b[0] + nn[0] * gap * sg, d.b[1] + nn[1] * gap * sg], [b[0] + nn[0] * ext * sg, b[1] + nn[1] * ext * sg]], false);
    let tk = 1.0 / k;
    let tick = [(u[0] - nn[0]) * tk, (u[1] - nn[1]) * tk];
    for q in [a, b] {
        path(ops, &[[q[0] - tick[0], q[1] - tick[1]], [q[0] + tick[0], q[1] + tick[1]]], false);
    }
    ops.push("S".into());
    let mut rot = m::atan2(u[1], u[0]);
    let half = std::f64::consts::PI / 2.0;
    if rot > half + 1e-9 || rot <= -half {
        rot += std::f64::consts::PI;
    }
    let mid = pp([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]);
    let up = [-m::sin(rot), m::cos(rot)];
    if width(&d.text, 6.0) / PT * 1.1 < l * k {
        text(ops, missing, &d.text, mid[0] + up[0] * 0.8, mid[1] + up[1] * 0.8, 6.0, "center", rot, false, 0.0);
    }
}

/// The file: catalog, pages, one page, its content, two fonts, info.
fn assemble(content: &str, w: f64, h: f64, title: &str) -> Vec<u8> {
    let enc = |s: &str| -> Vec<u8> {
        s.chars()
            .map(|ch| {
                let mut b = [0u16; 2];
                let u = ch.encode_utf16(&mut b).first().copied().unwrap_or(0);
                u.to_le_bytes()[0]
            })
            .collect()
    };
    let body = enc(content);
    let t = lit(title).0;
    let objs: Vec<Option<String>> = vec![
        Some("<< /Type /Catalog /Pages 2 0 R >>".into()),
        Some("<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into()),
        Some(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> /Contents 4 0 R >>",
            n(w),
            n(h)
        )),
        None,
        Some("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into()),
        Some("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".into()),
        Some(format!("<< /Title {t} /Producer (BimCraft) >>")),
    ];
    let mut out: Vec<u8> = Vec::new();
    out.extend(enc("%PDF-1.4\n%\u{e2}\u{e3}\u{cf}\u{d3}\n"));
    let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offs.push(out.len());
        match o {
            None => {
                out.extend(enc(&format!("{} 0 obj\n<< /Length {} >>\nstream\n", i.wrapping_add(1), body.len())));
                out.extend_from_slice(&body);
                out.extend(enc("\nendstream\nendobj\n"));
            }
            Some(s) => out.extend(enc(&format!("{} 0 obj\n{s}\nendobj\n", i.wrapping_add(1)))),
        }
    }
    let xref = out.len();
    let mut tail = format!("xref\n0 {}\n0000000000 65535 f \n", objs.len().wrapping_add(1));
    for o in offs {
        tail.push_str(&format!("{o:010} 00000 n \n"));
    }
    tail.push_str(&format!("trailer\n<< /Size {} /Root 1 0 R /Info 7 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len().wrapping_add(1)));
    out.extend(enc(&tail));
    out
}
