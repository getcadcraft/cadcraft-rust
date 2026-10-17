//! plot.js — the plan on a sheet as a vector PDF, written by hand: papers, engineering scales, the plan sheet (frame,
//! title strip, north arrow, scale bar, map pictures), and the PDF writer (one or many pages, Helvetica in
//! WinAnsiEncoding, arcs as Béziers, dashes, fills, JPEG images as DCTDecode XObjects).
//!
//! The PDF's streams are deflated (zlib); the bytes then differ from the JS's (another deflate), so the parity
//! harness compares the inflated objects (what a PDF reader gets), not the compressed bytes.

use crate::font::{char_advance, cp1252, subst};
use cc_civil::i18n::t;
use cc_civil::js::{J, JsMath, log10, max, min, num_to_string, pad_start, pow, to_fixed};
use cc_civil::tr;
use serde_json::{Map, Value, json};
use std::f64::consts::PI;

/// A paper (mm).
#[derive(Clone, Copy, Debug)]
pub struct Paper {
    pub key: &'static str,
    pub w: f64,
    pub h: f64,
}
pub const PAPERS: [Paper; 7] = [
    Paper { key: "letter", w: 279.4, h: 215.9 },
    Paper { key: "tabloid", w: 431.8, h: 279.4 },
    Paper { key: "ansi-d", w: 863.6, h: 558.8 },
    Paper { key: "arch-d", w: 914.4, h: 609.6 },
    Paper { key: "a4", w: 297.0, h: 210.0 },
    Paper { key: "a3", w: 420.0, h: 297.0 },
    Paper { key: "a1", w: 841.0, h: 594.0 },
];
/// `paperBy(k)` (tabloid when unknown).
pub fn paper_by(k: &str) -> Paper {
    PAPERS.iter().copied().find(|p| p.key == k).unwrap_or(PAPERS[1])
}
/// `paper.name`.
pub fn paper_name(p: &Paper) -> String {
    t(&format!("plot.paper.{}", p.key), &[])
}

/// A scale `{ label, mmPerUnit }`.
#[derive(Clone, Debug)]
pub struct Scale {
    pub label: String,
    pub mm_per_unit: f64,
}
impl J for Scale {
    fn j(&self) -> Value {
        json!({ "label": self.label, "mmPerUnit": self.mm_per_unit.j() })
    }
}
/// `scalesFor(units)`.
pub fn scales_for(units: &str) -> Vec<Scale> {
    if units == "m" {
        [50.0, 100.0, 200.0, 250.0, 500.0, 1000.0, 2000.0, 2500.0, 5000.0, 10000.0]
            .iter()
            .map(|n| Scale { label: format!("1:{}", num_to_string(*n)), mm_per_unit: 1000.0 / n })
            .collect()
    } else {
        [5.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 100.0, 200.0, 300.0, 400.0, 500.0, 1000.0]
            .iter()
            .map(|n| Scale { label: format!("1\" = {}'", num_to_string(*n)), mm_per_unit: 25.4 / n })
            .collect()
    }
}

pub const MARGIN: f64 = 12.0;
pub const TITLE_H: f64 = 16.0;

/// `orient(paper, landscape)` → (w, h).
pub fn orient(p: &Paper, landscape: bool) -> (f64, f64) {
    if landscape { (max(p.w, p.h), min(p.w, p.h)) } else { (min(p.w, p.h), max(p.w, p.h)) }
}

/// Extents `{ minX, minY, maxX, maxY }`.
#[derive(Clone, Copy, Debug)]
pub struct Ext {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

/// `fitScale(units, ext, paper, landscape)`.
pub fn fit_scale(units: &str, ext: &Ext, paper: &Paper, landscape: bool) -> Scale {
    let (w, h) = orient(paper, landscape);
    let aw = w - 2.0 * MARGIN;
    let ah = h - 2.0 * MARGIN - TITLE_H;
    let ww = max(ext.max_x - ext.min_x, 1e-9);
    let hh = max(ext.max_y - ext.min_y, 1e-9);
    let list = scales_for(units);
    let mut sorted = list.clone();
    cc_civil::js::sort_by(&mut sorted, |a, b| b.mm_per_unit - a.mm_per_unit);
    for s in sorted {
        if ww * s.mm_per_unit <= aw && hh * s.mm_per_unit <= ah {
            return s;
        }
    }
    list.last().cloned().unwrap_or(Scale { label: String::new(), mm_per_unit: 1.0 })
}

/// `WIDTH[layer]`.
pub fn width_of(layer: &str) -> Option<f64> {
    Some(match layer {
        "C-TOPO-MAJR" => 0.35,
        "C-TOPO-MINR" => 0.13,
        "C-TOPO-TINN" => 0.09,
        "C-ROAD-CNTR" => 0.5,
        "C-GRAD-PADS" => 0.5,
        "C-GRAD-DAYL" => 0.35,
        "C-GRAD-SLOP" => 0.09,
        "C-TOPO-BRKL" => 0.25,
        "C-STRM-PIPE" => 0.5,
        "C-STRM-STRC" => 0.35,
        "C-STRM-PROB" => 0.5,
        _ => return None,
    })
}
/// `RGB[aci]`.
pub fn rgb_of(aci: i64) -> Option<&'static str> {
    Some(match aci {
        1 => "#c00000",
        2 => "#b09000",
        3 => "#008000",
        4 => "#007a8a",
        5 => "#1e48c8",
        6 => "#9a1f86",
        7 => "#000000",
        8 => "#808080",
        30 => "#c05000",
        _ => return None,
    })
}

/// A stroke.
#[derive(Clone, Debug, Default)]
pub struct Stroke {
    pub color: String,
    pub width: f64,
    pub pts: Vec<(f64, f64)>,
    pub closed: Option<bool>,
    /// (x, y, r, a0, a1)
    pub arc: Option<(f64, f64, f64, f64, f64)>,
    pub dash: Option<Vec<f64>>,
}
impl J for Stroke {
    fn j(&self) -> Value {
        let mut m = Map::new();
        m.insert("color".into(), self.color.j());
        m.insert("width".into(), self.width.j());
        if let Some((x, y, r, a0, a1)) = self.arc {
            m.insert("arc".into(), json!({ "x": x.j(), "y": y.j(), "r": r.j(), "a0": a0.j(), "a1": a1.j() }));
        } else {
            m.insert("pts".into(), Value::Array(self.pts.iter().map(|(x, y)| json!({ "x": x.j(), "y": y.j() })).collect()));
        }
        if let Some(c) = self.closed {
            m.insert("closed".into(), Value::Bool(c));
        }
        if let Some(d) = &self.dash {
            m.insert("dash".into(), d.j());
        }
        Value::Object(m)
    }
}
/// A text.
#[derive(Clone, Debug, Default)]
pub struct PText {
    pub color: String,
    pub x: f64,
    pub y: f64,
    pub h: f64,
    pub rot: f64,
    pub text: String,
    pub align: String,
    pub middle: Option<bool>,
}
impl J for PText {
    fn j(&self) -> Value {
        let mut o = json!({ "color": self.color, "x": self.x.j(), "y": self.y.j(), "h": self.h.j(), "rot": self.rot.j(), "text": self.text, "align": self.align });
        if let (Some(mi), Some(m)) = (self.middle, o.as_object_mut()) {
            m.insert("middle".into(), Value::Bool(mi));
        }
        o
    }
}
/// An image on the page (mm).
#[derive(Clone, Debug)]
pub struct PImage {
    pub jpeg: Vec<u8>,
    pub px_w: f64,
    pub px_h: f64,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}
/// A fill.
#[derive(Clone, Debug)]
pub struct Fill {
    pub color: String,
    pub pts: Vec<(f64, f64)>,
}
/// A clipped view.
#[derive(Clone, Debug, Default)]
pub struct View {
    pub clip: (f64, f64, f64, f64),
    pub strokes: Vec<Stroke>,
    pub points: Vec<(String, f64, f64)>,
    pub texts: Vec<PText>,
    pub images: Vec<PImage>,
    pub fills: Vec<Fill>,
}
/// A sheet: one view or several, then its chrome.
#[derive(Clone, Debug, Default)]
pub struct Sheet {
    pub w: f64,
    pub h: f64,
    pub name: String,
    /// `views` (several) — when None the sheet's own view is `single`
    pub views: Option<Vec<View>>,
    pub single: View,
    pub chrome: Vec<Stroke>,
    pub chrome_texts: Vec<PText>,
    /// more keys the JS puts on a sheet (stations, theta)
    pub extra: Vec<(String, Value)>,
}
fn view_j(v: &View, with_images: bool) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("clip".into(), json!({ "x0": v.clip.0.j(), "y0": v.clip.1.j(), "x1": v.clip.2.j(), "y1": v.clip.3.j() }));
    m.insert("strokes".into(), v.strokes.j());
    m.insert("points".into(), Value::Array(v.points.iter().map(|(c, x, y)| json!({ "color": c, "x": x.j(), "y": y.j() })).collect()));
    m.insert("texts".into(), v.texts.j());
    if with_images {
        m.insert(
            "images".into(),
            Value::Array(v.images.iter().map(|i| json!({ "jpeg": { "$base64": crate::civil_ops::b64(&i.jpeg) }, "pxW": i.px_w.j(), "pxH": i.px_h.j(), "x": i.x.j(), "y": i.y.j(), "w": i.w.j(), "h": i.h.j() })).collect()),
        );
    }
    m.insert("fills".into(), Value::Array(v.fills.iter().map(|f| json!({ "color": f.color, "pts": f.pts.iter().map(|(x, y)| json!({ "x": x.j(), "y": y.j() })).collect::<Vec<_>>() })).collect()));
    m
}
impl J for Sheet {
    fn j(&self) -> Value {
        let mut m = Map::new();
        m.insert("w".into(), self.w.j());
        m.insert("h".into(), self.h.j());
        m.insert("name".into(), self.name.j());
        match &self.views {
            Some(vs) => {
                m.insert("views".into(), Value::Array(vs.iter().map(|v| Value::Object(view_j(v, true))).collect()));
            }
            None => m.extend(view_j(&self.single, true)),
        }
        m.insert("chrome".into(), self.chrome.j());
        m.insert("chromeTexts".into(), self.chrome_texts.j());
        for (k, v) in &self.extra {
            m.insert(k.clone(), v.clone());
        }
        Value::Object(m)
    }
}

/// `sheetWindow(extents, { paper, landscape, scale })`.
pub fn sheet_window(ext: &Ext, paper: &str, landscape: bool, scale: &Scale) -> Ext {
    let (pw, ph) = orient(&paper_by(paper), landscape);
    let (fx0, fy0, fx1, fy1) = (MARGIN, MARGIN + TITLE_H, pw - MARGIN, ph - MARGIN);
    let s = scale.mm_per_unit;
    let cx = (ext.min_x + ext.max_x) / 2.0;
    let cy = (ext.min_y + ext.max_y) / 2.0;
    let hw = (fx1 - fx0) / 2.0 / s;
    let hh = (fy1 - fy0) / 2.0 / s;
    Ext { min_x: cx - hw, max_x: cx + hw, min_y: cy - hh, max_y: cy + hh }
}

/// The date the JS writes (`new Date().toISOString().slice(0, 10)`).
pub fn today() -> String {
    let ms = now_ms();
    let (y, m, d, _, _, _) = crate::landxml::date_parts(ms);
    format!("{}-{}-{}", pad_start(&y.to_string(), 4, '0'), pad_start(&m.to_string(), 2, '0'), pad_start(&d.to_string(), 2, '0'))
}
/// Milliseconds since 1970 (0 on targets without a clock).
pub fn now_ms() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() * 1000.0).floor()
    }
    #[cfg(target_arch = "wasm32")]
    {
        0.0
    }
}

/// A map picture for planSheet: (jpeg, pxW, pxH, box in project coordinates).
pub struct SceneImage {
    pub jpeg: Vec<u8>,
    pub px_w: f64,
    pub px_h: f64,
    pub bbox: Ext,
}

/// `planSheet(scene, { extents, paper, landscape, scale, title, units, notes, hidden, images })`.
#[allow(clippy::too_many_arguments)]
pub fn plan_sheet(
    items: &[Value],
    ext: &Ext,
    paper: &str,
    landscape: bool,
    scale: &Scale,
    title: Option<&str>,
    units: &str,
    notes: &[String],
    hidden: &[String],
    images: &[SceneImage],
) -> Sheet {
    let (pw, ph) = orient(&paper_by(paper), landscape);
    let frame = (MARGIN, MARGIN + TITLE_H, pw - MARGIN, ph - MARGIN);
    let s = scale.mm_per_unit;
    let cx = (ext.min_x + ext.max_x) / 2.0;
    let cy = (ext.min_y + ext.max_y) / 2.0;
    let ox = (frame.0 + frame.2) / 2.0 - cx * s;
    let oy = (frame.1 + frame.3) / 2.0 - cy * s;
    let tx = |x: f64, y: f64| (x * s + ox, y * s + oy);
    let aci_of = |layer: &str| crate::scene::LAYERS.iter().find(|l| l.0 == layer).map(|l| l.2);
    let n = |v: &Value, k: &str| cc_civil::js::numf(v.get(k));
    let mut view = View { clip: frame, ..View::default() };
    for it in items {
        let layer = cc_civil::js::string_of(it.get("layer"));
        if hidden.contains(&layer) {
            continue;
        }
        let color = aci_of(&layer).and_then(rgb_of).unwrap_or("#000000").to_string();
        let width = width_of(&layer).unwrap_or(0.18);
        match it.get("kind").and_then(Value::as_str) {
            Some("poly") => {
                let pts = cc_civil::js::arr(it.get("pts")).iter().map(|p| tx(n(p, "x"), n(p, "y"))).collect();
                view.strokes.push(Stroke {
                    color,
                    width,
                    pts,
                    closed: Some(cc_civil::js::truthy(it.get("closed"))),
                    arc: None,
                    dash: None,
                });
            }
            Some("arc") => {
                let c = it.get("c").cloned().unwrap_or(Value::Null);
                let (x, y) = tx(n(&c, "x"), n(&c, "y"));
                view.strokes.push(Stroke {
                    color,
                    width,
                    pts: Vec::new(),
                    closed: None,
                    arc: Some((x, y, n(it, "r") * s, n(it, "a0"), n(it, "a1"))),
                    dash: None,
                });
            }
            Some("point") => {
                let (x, y) = tx(n(it, "x"), n(it, "y"));
                view.points.push((color, x, y));
            }
            Some("text") => {
                let (x, y) = tx(n(it, "x"), n(it, "y"));
                let rot = n(it, "rot");
                view.texts.push(PText {
                    color,
                    x,
                    y,
                    h: n(it, "mm"),
                    rot: if rot != 0.0 && !rot.is_nan() { rot } else { 0.0 },
                    text: cc_civil::js::string_of(it.get("text")),
                    align: cc_civil::js::string_of(it.get("align")),
                    middle: it.get("middle").and_then(Value::as_bool),
                });
            }
            _ => {}
        }
    }
    let ink = "#000000".to_string();
    let mut chrome = Vec::new();
    let st = |w: f64, pts: Vec<(f64, f64)>, closed: Option<bool>| Stroke {
        color: "#000000".into(),
        width: w,
        pts,
        closed,
        arc: None,
        dash: None,
    };
    chrome.push(st(0.5, vec![(frame.0, MARGIN), (frame.2, MARGIN), (frame.2, frame.3), (frame.0, frame.3)], Some(true)));
    chrome.push(st(0.35, vec![(frame.0, frame.1), (frame.2, frame.1)], None));
    let title_s = title.map_or_else(|| t("plot.title", &[]), str::to_string);
    let mut line2 = vec![tr!("plot.scale", scale = scale.label.as_str())];
    line2.extend(notes.iter().cloned());
    let pt = |x: f64, y: f64, h: f64, text: String, align: &str| PText {
        color: ink.clone(),
        x,
        y,
        h,
        rot: 0.0,
        text,
        align: align.into(),
        middle: None,
    };
    let mut tt = vec![
        pt(frame.0 + 4.0, MARGIN + TITLE_H - 6.0, 4.0, title_s.clone(), "left"),
        pt(frame.0 + 4.0, MARGIN + 3.5, 2.2, line2.join("   ·   "), "left"),
        pt(frame.2 - 4.0, MARGIN + 3.5, 2.2, tr!("plot.made", date = today()), "right"),
    ];
    let na = (frame.2 - 12.0, frame.3 - 20.0);
    chrome.push(st(0.35, vec![(na.0, na.1 - 6.0), (na.0, na.1 + 8.0)], None));
    chrome.push(st(0.35, vec![(na.0 - 3.0, na.1 + 3.0), (na.0, na.1 + 8.0), (na.0 + 3.0, na.1 + 3.0)], None));
    tt.push(pt(na.0, na.1 + 10.0, 3.0, "N".into(), "center"));
    let want = 50.0 / s;
    let pw10 = pow(10.0, log10(want).floor());
    let len = [1.0, 2.0, 5.0, 10.0].iter().map(|m| m * pw10).rfind(|v| *v <= want).unwrap_or(pw10);
    let len = if len != 0.0 && !len.is_nan() { len } else { pw10 };
    let sb = (frame.2 - 4.0 - len * s, MARGIN + TITLE_H - 6.0);
    chrome.push(st(0.35, vec![(sb.0, sb.1 + 1.5), (sb.0, sb.1), (sb.0 + len * s, sb.1), (sb.0 + len * s, sb.1 + 1.5)], None));
    tt.push(pt(sb.0, sb.1 + 2.2, 2.0, "0".into(), "center"));
    tt.push(pt(sb.0 + len * s, sb.1 + 2.2, 2.0, format!("{}{}", num_to_string(len), if units == "m" { " m" } else { " ft" }), "center"));
    for im in images {
        if im.jpeg.is_empty() || !(im.px_w > 0.0 && im.px_h > 0.0) {
            continue;
        }
        let a = tx(im.bbox.min_x, im.bbox.min_y);
        let b = tx(im.bbox.max_x, im.bbox.max_y);
        view.images.push(PImage { jpeg: im.jpeg.clone(), px_w: im.px_w, px_h: im.px_h, x: a.0, y: a.1, w: b.0 - a.0, h: b.1 - a.1 });
    }
    Sheet { w: pw, h: ph, name: title_s, views: None, single: view, chrome, chrome_texts: tt, extra: Vec::new() }
}

// ---------------------------------------------------------------------------
// PDF

/// `num(v)`: 4 decimals, trailing zeros dropped, |v| < 5e-5 → 0, non-finite '0'.
pub fn num(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let s = to_fixed(if v.abs() < 5e-5 { 0.0 } else { v }, 4);
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}
#[allow(clippy::arithmetic_side_effects)] // bounded counters and indices (page numbers, padding, digit math)
fn rgb(hex: &str) -> String {
    let h: String =
        if hex.chars().count() == 4 { hex.chars().skip(1).flat_map(|c| [c, c]).collect() } else { hex.chars().skip(1).take(6).collect() };
    (0..3)
        .map(|i| {
            let part = h.get(i * 2..i * 2 + 2).unwrap_or("");
            num(parse_int_16(part) / 255.0)
        })
        .collect::<Vec<_>>()
        .join(" ")
}
/// `parseInt(s, 16)` (leading hex digits; none → NaN).
fn parse_int_16(s: &str) -> f64 {
    let mut v = f64::NAN;
    for c in s.chars() {
        let Some(d) = c.to_digit(16) else { break };
        v = if v.is_nan() { f64::from(d) } else { v * 16.0 + f64::from(d) };
    }
    v
}

/// `winAnsi(s)` → (escaped literal, width 1/1000 em, missing).
pub fn win_ansi(s: &str) -> (String, f64, usize) {
    let mut out = String::new();
    let mut width = 0.0f64;
    let mut missing = 0usize;
    for ch0 in s.chars() {
        let ch = subst(ch0);
        let mut code = u32::from(ch);
        if let Some(k) = cp1252(ch) {
            code = k;
        } else if code > 0xff || (0x7f..0xa0).contains(&code) || code < 0x20 {
            code = 0x3f;
            missing = missing.saturating_add(1);
            width += 556.0;
        }
        if code == 0x28 || code == 0x29 || code == 0x5c {
            out.push('\\');
            out.push(char::from_u32(code).unwrap_or('?'));
        } else if (0x20..0x7f).contains(&code) {
            out.push(char::from_u32(code).unwrap_or('?'));
        } else {
            out.push('\\');
            out.push_str(&format!("{code:03o}"));
        }
        if code != 0x3f || ch == '?' {
            width += f64::from(char_advance(ch));
        }
    }
    (out, width, missing)
}

/// `arcBeziers(cx, cy, r, a, b)`.
pub fn arc_beziers(cx: f64, cy: f64, r: f64, a: f64, b: f64) -> Vec<[f64; 6]> {
    let sweep = b - a;
    let n = max(1.0, (sweep.abs() / (PI / 2.0) - 1e-9).ceil());
    let n = if n.is_finite() { n.min(1e6) } else { 1.0 };
    let d = sweep / n;
    let k = 4.0 / 3.0 * (d / 4.0).jtan();
    let mut out = Vec::new();
    let mut i = 0.0;
    while i < n {
        let t0 = a + d * i;
        let t1 = t0 + d;
        let (c0, s0, c1, s1) = (t0.jcos(), t0.jsin(), t1.jcos(), t1.jsin());
        out.push([
            cx + r * (c0 - k * s0),
            cy + r * (s0 + k * c0),
            cx + r * (c1 + k * s1),
            cy + r * (s1 - k * c1),
            cx + r * c1,
            cy + r * s1,
        ]);
        i += 1.0;
    }
    out
}

fn page_content(sheet: &Sheet, mono: bool, image_names: &[String]) -> (Vec<u8>, usize) {
    let mut out: Vec<String> = Vec::new();
    let mut img = 0usize;
    let mut missing = 0usize;
    let col = |c: &str| if mono { "#000000".to_string() } else { c.to_string() };
    let k = to_fixed(72.0 / 25.4, 9);
    out.push(format!("{k} 0 0 {k} 0 0 cm"));
    out.push("1 J 1 j".into());
    let draw_stroke = |st: &Stroke, out: &mut Vec<String>| {
        out.push(format!("{} RG {} w", rgb(&col(&st.color)), num(st.width)));
        if let Some(d) = &st.dash {
            out.push(format!("[{}] 0 d", d.iter().map(|v| num(*v)).collect::<Vec<_>>().join(" ")));
        }
        if let Some((x, y, r, a0, a1)) = st.arc {
            out.push(format!("{} {} m", num(x + r * a0.jcos()), num(y + r * a0.jsin())));
            let b = if a1 < a0 { a1 + PI * 2.0 } else { a1 };
            for c in arc_beziers(x, y, r, a0, b) {
                out.push(format!("{} c", c.iter().map(|v| num(*v)).collect::<Vec<_>>().join(" ")));
            }
        } else {
            for (i, (x, y)) in st.pts.iter().enumerate() {
                out.push(format!("{} {}{}", num(*x), num(*y), if i > 0 { " l" } else { " m" }));
            }
            if st.closed == Some(true) {
                out.push("h".into());
            }
        }
        out.push("S".into());
        if st.dash.is_some() {
            out.push("[] 0 d".into());
        }
    };
    let draw_text = |tx: &PText, out: &mut Vec<String>, missing: &mut usize| {
        let (enc, w0, miss) = win_ansi(&tx.text);
        *missing = missing.saturating_add(miss);
        let w = w0 / 1000.0 * tx.h / 0.72;
        let fs = tx.h / 0.72;
        let c = tx.rot.jcos();
        let s = tx.rot.jsin();
        let lx = if tx.align == "center" {
            -w / 2.0
        } else if tx.align == "right" {
            -w
        } else {
            0.0
        };
        let ly = if tx.middle == Some(true) { -tx.h / 2.0 } else { 0.0 };
        let x = tx.x + lx * c - ly * s;
        let y = tx.y + lx * s + ly * c;
        out.push(format!(
            "{} rg BT /F1 1 Tf {} Tm ({}) Tj ET",
            rgb(&col(&tx.color)),
            [fs * c, fs * s, -fs * s, fs * c, x, y].map(num).join(" "),
            enc
        ));
    };
    let views: Vec<&View> = match &sheet.views {
        Some(vs) => vs.iter().collect(),
        None => vec![&sheet.single],
    };
    for v in views {
        out.push("q".into());
        let f = v.clip;
        out.push(format!("{} {} {} {} re W n", num(f.0), num(f.1), num(f.2 - f.0), num(f.3 - f.1)));
        for im in &v.images {
            let name = image_names.get(img).cloned().unwrap_or_default();
            img = img.saturating_add(1);
            out.push(format!("q {} cm /{} Do Q", [im.w, 0.0, 0.0, im.h, im.x, im.y].map(num).join(" "), name));
        }
        for fl in &v.fills {
            if fl.pts.len() < 3 {
                continue;
            }
            out.push(format!("{} rg", rgb(if mono { "#d9d9d9" } else { &fl.color })));
            for (i, (x, y)) in fl.pts.iter().enumerate() {
                out.push(format!("{} {}{}", num(*x), num(*y), if i > 0 { " l" } else { " m" }));
            }
            out.push("h f".into());
        }
        for st in &v.strokes {
            draw_stroke(st, &mut out);
        }
        for (c, x, y) in &v.points {
            out.push(format!("{} RG 0.18 w", rgb(&col(c))));
            let k = 0.6;
            out.push(format!(
                "{} {} m {} {} l {} {} m {} {} l S",
                num(x - k),
                num(y - k),
                num(x + k),
                num(y + k),
                num(x - k),
                num(y + k),
                num(x + k),
                num(y - k)
            ));
        }
        for tx in &v.texts {
            draw_text(tx, &mut out, &mut missing);
        }
        out.push("Q".into());
    }
    for st in &sheet.chrome {
        draw_stroke(st, &mut out);
    }
    for tx in &sheet.chrome_texts {
        draw_text(tx, &mut out, &mut missing);
    }
    (format!("{}\n", out.join("\n")).into_bytes(), missing)
}

fn images_of(sheet: &Sheet) -> Vec<&PImage> {
    match &sheet.views {
        Some(vs) => vs.iter().flat_map(|v| v.images.iter()).collect(),
        None => sheet.single.images.iter().collect(),
    }
}

struct Page<'a> {
    w: f64,
    h: f64,
    content: Vec<u8>,
    flate: bool,
    images: Vec<&'a PImage>,
}

fn deflate(b: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(b, 6)
}

/// `toPdf(sheet, { mono })` → (bytes, report).
pub fn to_pdf(sheet: &Sheet, mono: bool) -> (Vec<u8>, Value) {
    let imgs = images_of(sheet);
    let names: Vec<String> = (0..imgs.len()).map(|i| format!("Im{}", i.saturating_add(1))).collect();
    let (content, missing) = page_content(sheet, mono, &names);
    let packed = deflate(&content);
    let bytes = assemble(&[Page { w: sheet.w, h: sheet.h, content: packed, flate: true, images: imgs }], &sheet.name);
    let size = bytes.len();
    (bytes, json!({ "missing": missing, "size": size, "compressed": true }))
}

/// `toPdfPages(sheets, { mono, title })` → (bytes, report).
pub fn to_pdf_pages(sheets: &[Sheet], mono: bool, title: &str) -> (Vec<u8>, Value) {
    let mut pages = Vec::new();
    let mut missing = 0usize;
    for sh in sheets {
        let imgs = images_of(sh);
        let names: Vec<String> = (0..imgs.len()).map(|i| format!("Im{}", i.saturating_add(1))).collect();
        let (content, m) = page_content(sh, mono, &names);
        missing = missing.saturating_add(m);
        pages.push(Page { w: sh.w, h: sh.h, content: deflate(&content), flate: true, images: imgs });
    }
    let title = if title.is_empty() { sheets.first().map_or("", |s| s.name.as_str()) } else { title };
    let bytes = assemble(&pages, title);
    let size = bytes.len();
    let n = pages.len();
    (bytes, json!({ "missing": missing, "size": size, "compressed": true, "pages": n }))
}

fn assemble(pages: &[Page], title: &str) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut offsets: Vec<usize> = Vec::new();
    let pt = |mm: f64| num(mm * 72.0 / 25.4);
    let (y, mo, d, h, mi, s) = crate::landxml::date_parts(now_ms());
    let p2 = |x: i64| pad_start(&x.to_string(), 2, '0');
    let date = format!("D:{}{}{}{}{}{}Z", y, p2(mo), p2(d), p2(h), p2(mi), p2(s));
    let pdf_text = |s: &str| format!("({})", win_ansi(s).0);
    let page_obj = |i: usize| 5usize.saturating_add(i.saturating_mul(2));
    let mut next_img = 5usize.saturating_add(pages.len().saturating_mul(2));
    let img_objs: Vec<Vec<usize>> = pages
        .iter()
        .map(|pg| {
            pg.images
                .iter()
                .map(|_| {
                    let n = next_img;
                    next_img = next_img.saturating_add(1);
                    n
                })
                .collect()
        })
        .collect();
    let obj = |n: usize, body: &[&[u8]], out: &mut Vec<u8>, offsets: &mut Vec<usize>| {
        if offsets.len() <= n {
            offsets.resize(n.saturating_add(1), 0);
        }
        if let Some(o) = offsets.get_mut(n) {
            *o = out.len();
        }
        out.extend_from_slice(format!("{n} 0 obj\n").as_bytes());
        for b in body {
            out.extend_from_slice(b);
        }
        out.extend_from_slice(b"\nendobj\n");
    };
    out.extend_from_slice(b"%PDF-1.4\n");
    out.extend_from_slice(&[0x25, 0xe2, 0xe3, 0xcf, 0xd3, 0x0a]);
    obj(1, &[b"<< /Type /Catalog /Pages 2 0 R >>"], &mut out, &mut offsets);
    let kids: Vec<String> = (0..pages.len()).map(|i| format!("{} 0 R", page_obj(i))).collect();
    obj(2, &[format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), pages.len()).as_bytes()], &mut out, &mut offsets);
    obj(3, &[b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"], &mut out, &mut offsets);
    obj(
        4,
        &[format!("<< /Title {} /Producer (CivilCraft) /Creator (CivilCraft) /CreationDate ({}) >>", pdf_text(title), date).as_bytes()],
        &mut out,
        &mut offsets,
    );
    for (i, pg) in pages.iter().enumerate() {
        let io = img_objs.get(i).map(Vec::as_slice).unwrap_or(&[]);
        let xo = if io.is_empty() {
            String::new()
        } else {
            format!(
                " /XObject << {} >>",
                io.iter().enumerate().map(|(k, n)| format!("/Im{} {} 0 R", k.saturating_add(1), n)).collect::<Vec<_>>().join(" ")
            )
        };
        obj(
            page_obj(i),
            &[format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 3 0 R >>{} >> /Contents {} 0 R >>",
                pt(pg.w),
                pt(pg.h),
                xo,
                page_obj(i).saturating_add(1)
            )
            .as_bytes()],
            &mut out,
            &mut offsets,
        );
        let head = format!("<< /Length {}{} >>\nstream\n", pg.content.len(), if pg.flate { " /Filter /FlateDecode" } else { "" });
        obj(page_obj(i).saturating_add(1), &[head.as_bytes(), &pg.content, b"\nendstream"], &mut out, &mut offsets);
    }
    for (i, pg) in pages.iter().enumerate() {
        for (k, im) in pg.images.iter().enumerate() {
            let n = img_objs.get(i).and_then(|v| v.get(k)).copied().unwrap_or(0);
            let head = format!(
                "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
                num_to_string(im.px_w),
                num_to_string(im.px_h),
                im.jpeg.len()
            );
            obj(n, &[head.as_bytes(), &im.jpeg, b"\nendstream"], &mut out, &mut offsets);
        }
    }
    let n = next_img;
    let xref = out.len();
    let mut x = format!("xref\n0 {n}\n0000000000 65535 f \n");
    for o in offsets.iter().skip(1) {
        x.push_str(&format!("{:010} 00000 n \n", o));
    }
    out.extend_from_slice(x.as_bytes());
    out.extend_from_slice(format!("trailer\n<< /Size {n} /Root 1 0 R /Info 4 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
    out
}

/// The parity harness's view of a PDF (what a reader gets): every object's text with its streams inflated, the
/// creation date and the offsets left out; the report beside.
/// The objects of a PDF for comparison, and the size change from counting every Flate stream inflated
/// (deflate bytes differ between zlib builds and browsers; everything else must match).
pub fn pdf_view(bytes: &[u8]) -> (Value, i64) {
    let mut objs: Vec<Value> = Vec::new();
    let mut delta = 0i64;
    let len_i = |n: usize| i64::try_from(n).unwrap_or(i64::MAX);
    let mut i = 0usize;
    let find = |hay: &[u8], needle: &[u8], from: usize| -> Option<usize> {
        hay.get(from..).and_then(|h| h.windows(needle.len()).position(|w| w == needle)).map(|p| p.saturating_add(from))
    };
    while let Some(o) = find(bytes, b" 0 obj\n", i) {
        let start = o.saturating_add(7);
        let Some(end) = find(bytes, b"\nendobj\n", start) else { break };
        let body = bytes.get(start..end).unwrap_or(&[]);
        let text = match find(body, b"stream\n", 0) {
            Some(sp) if body.ends_with(b"\nendstream") => {
                let dict = String::from_utf8_lossy(body.get(..sp).unwrap_or(&[])).to_string();
                let data = body.get(sp.saturating_add(7)..body.len().saturating_sub(10)).unwrap_or(&[]);
                let dict_n = dict.split("/Length ").next().unwrap_or("").to_string();
                if dict.contains("/FlateDecode") {
                    let raw = miniz_oxide::inflate::decompress_to_vec_zlib(data).unwrap_or_default();
                    delta = delta
                        .saturating_add(len_i(raw.len()))
                        .saturating_sub(len_i(data.len()))
                        .saturating_add(len_i(raw.len().to_string().len()))
                        .saturating_sub(len_i(data.len().to_string().len()));
                    format!("{dict_n}|flate|{}", String::from_utf8_lossy(&raw))
                } else {
                    format!("{dict_n}|raw|{}", crate::civil_ops::sha16(data))
                }
            }
            _ => {
                let s = String::from_utf8_lossy(body).to_string();
                match s.find("/CreationDate (") {
                    Some(k) => s.get(..k).unwrap_or("").to_string(),
                    None => s,
                }
            }
        };
        objs.push(Value::String(text));
        i = end;
    }
    // the xref offset's own digits move with the stream sizes too
    if let Some(sx) = bytes.windows(10).rposition(|w| w == b"startxref\n") {
        let digits: String =
            bytes.get(sx.saturating_add(10)..).unwrap_or(&[]).iter().take_while(|c| c.is_ascii_digit()).map(|&c| char::from(c)).collect();
        if let Ok(n) = digits.parse::<i64>() {
            delta = delta.saturating_add(len_i(n.saturating_add(delta).to_string().len())).saturating_sub(len_i(digits.len()));
        }
    }
    (Value::Array(objs), delta)
}
