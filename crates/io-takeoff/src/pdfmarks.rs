//! `pdfmarks.js` — a copy of a PDF with the takeoff drawn in: runs stroked (deducted stretches dashed and thin),
//! areas filled at 22 % (even-odd, cut-outs open) with an outline, counts as dots, markups (highlights, clouds, notes
//! with their status and text), and a legend per marked page (item, colour, quantity on that sheet) upright in the
//! visual bottom-left whatever /Rotate says. The operators, numbers and text are pdf-lib's, so pdf.js reads the same
//! page from this file as from the JS's; the file is an incremental update of the original (its bytes untouched).

use std::collections::HashMap;

use hayro::hayro_interpret::hayro_syntax::Pdf;
use hayro::hayro_interpret::hayro_syntax::object::{Array, Dict, MaybeRef, Number, Object};
use serde_json::Value;

use cc_takeoff::geom::{self, Op, P};
use cc_takeoff::i18n::t;
use cc_takeoff::jsmath::{cos, sin, tan};
use cc_takeoff::jv;
use cc_takeoff::model::{self, QOpts};
use cc_takeoff::units;

use crate::pdfw::{Content, PO, Writer, copy_obj, dict_entries, id_parts, read_trailer};
use crate::stdfont;

/// Helvetica (WinAnsi) can't draw every character: anything else prints as "?" (one per UTF-16 unit, as the JS
/// regular expression replaces them).
pub fn win_ansi(s: &str) -> String {
    let mut out = String::new();
    for u in s.encode_utf16() {
        match char::from_u32(u32::from(u)) {
            Some(c) if stdfont::winansi(c).is_some() => out.push(c),
            _ => out.push('?'),
        }
    }
    out
}
/// bid.js's winAnsi (keeps newlines).
pub fn win_ansi_nl(s: &str) -> String {
    let mut out = String::new();
    for u in s.encode_utf16() {
        match char::from_u32(u32::from(u)) {
            Some('\n') => out.push('\n'),
            Some(c) if stdfont::winansi(c).is_some() => out.push(c),
            _ => out.push('?'),
        }
    }
    out
}

/// A standard font: Helvetica or Helvetica-Bold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Font {
    Regular,
    Bold,
}
impl Font {
    fn tables(self) -> (&'static [u16; 256], &'static [(u8, u8, i16)]) {
        match self {
            Font::Regular => (&stdfont::HELVETICA_W, stdfont::HELVETICA_KERN),
            Font::Bold => (&stdfont::HELVETICA_BOLD_W, stdfont::HELVETICA_BOLD_KERN),
        }
    }
    /// The WinAnsi codes of a text (the caller has made it WinAnsi).
    pub fn codes(self, s: &str) -> Vec<u8> {
        s.chars().filter_map(stdfont::winansi).collect()
    }
    /// pdf-lib's `widthOfTextAtSize`.
    pub fn width(self, s: &str, size: f64) -> f64 {
        let (w, kern) = self.tables();
        let codes = self.codes(s);
        let mut total = 0.0f64;
        for (i, c) in codes.iter().enumerate() {
            let gw = f64::from(w.get(usize::from(*c)).copied().unwrap_or(0));
            let gw = if gw == 0.0 { 250.0 } else { gw };
            let k = codes
                .get(i.saturating_add(1))
                .and_then(|r| kern.binary_search_by(|(a, b, _)| (*a, *b).cmp(&(*c, *r))).ok())
                .and_then(|i| kern.get(i))
                .map_or(0.0, |(_, _, k)| f64::from(*k));
            total += gw + k;
        }
        total * (size / 1000.0)
    }
}

/// A colour.
pub type Rgb = (f64, f64, f64);
pub fn hex_rgb(hex: &str) -> Rgb {
    let n = u32::from_str_radix(hex.get(1..).unwrap_or(""), 16).unwrap_or(0);
    (f64::from((n >> 16) & 255) / 255.0, f64::from((n >> 8) & 255) / 255.0, f64::from(n & 255) / 255.0)
}

/// A page being drawn on: its single added content stream and the resources it needs (pdf-lib's names).
pub struct Canvas {
    pub c: Content,
    pub ext_g_states: Vec<(String, PO)>,
    pub fonts: Vec<(Font, String)>,
    gs_n: usize,
}
impl Canvas {
    pub fn new() -> Self {
        Canvas { c: Content::default(), ext_g_states: Vec::new(), fonts: Vec::new(), gs_n: 0 }
    }
    /// The page's name of a font (added on first use).
    pub fn font(&mut self, f: Font) -> String {
        if let Some((_, n)) = self.fonts.iter().find(|(ff, _)| *ff == f) {
            return n.clone();
        }
        let n = format!("F{}", self.fonts.len().saturating_add(1));
        self.fonts.push((f, n.clone()));
        n
    }
    /// A graphics state for an opacity (pdf-lib's maybeEmbedGraphicsState).
    pub fn gs(&mut self, w: &mut Writer, ca: f64) -> String {
        self.gs_n = self.gs_n.saturating_add(1);
        let name = format!("GS{}", self.gs_n);
        let r = w.add(&PO::dict(vec![("Type", PO::name("ExtGState")), ("ca", PO::Num(ca))]));
        self.ext_g_states.push((name.clone(), r));
        name
    }
    pub fn rg(&mut self, c: Rgb) {
        self.c.op("rg", &[c.0, c.1, c.2]);
    }
    pub fn rg_stroke(&mut self, c: Rgb) {
        self.c.op("RG", &[c.0, c.1, c.2]);
    }
    /// pdf-lib's drawRectangle.
    #[allow(clippy::too_many_arguments)]
    pub fn rect(
        &mut self,
        w: &mut Writer,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        rot_deg: f64,
        color: Option<Rgb>,
        border: Option<Rgb>,
        border_width: f64,
        opacity: Option<f64>,
    ) {
        let gs = opacity.map(|o| self.gs(w, o));
        self.c.op("q", &[]);
        if let Some(g) = gs {
            self.c.op_name("gs", &g, &[]);
        }
        if let Some(c) = color {
            self.rg(c);
        }
        if let Some(b) = border {
            self.rg_stroke(b);
        }
        self.c.op("w", &[border_width]);
        self.c.dash(&[], 0.0);
        self.c.op("cm", &[1.0, 0.0, 0.0, 1.0, x, y]);
        let r = rot_deg * std::f64::consts::PI / 180.0;
        self.c.op("cm", &[cos(r), sin(r), -sin(r), cos(r), 0.0, 0.0]);
        self.c.op("cm", &[1.0, tan(0.0), tan(0.0), 1.0, 0.0, 0.0]);
        self.c.op("m", &[0.0, 0.0]);
        self.c.op("l", &[0.0, height]);
        self.c.op("l", &[width, height]);
        self.c.op("l", &[width, 0.0]);
        self.c.op("h", &[]);
        if color.is_some() && border_width != 0.0 && !border_width.is_nan() {
            self.c.op("B", &[]);
        } else if color.is_some() {
            self.c.op("f", &[]);
        } else if border.is_some() {
            self.c.op("S", &[]);
        } else {
            self.c.op("h", &[]);
        }
        self.c.op("Q", &[]);
    }
    /// pdf-lib's drawText (one line).
    #[allow(clippy::too_many_arguments)] // pdf-lib drawText options, one for one
    pub fn text(&mut self, s: &str, x: f64, y: f64, size: f64, font: Font, color: Rgb, rot_deg: f64) {
        let name = self.font(font);
        // cleanText then lineSplit: the callers pass one line of WinAnsi text
        let cleaned: String = s.replace(['\t', '\u{85}', '\u{2028}', '\u{2029}'], "    ").replace(['\u{8}', '\u{b}'], "");
        let lines: Vec<&str> = cleaned.split(['\n', '\u{c}', '\r', '\u{b}']).collect();
        self.c.op("q", &[]);
        self.c.op("BT", &[]);
        self.rg(color);
        self.c.op_name("Tf", &name, &[size]);
        self.c.op("TL", &[24.0]);
        let r = rot_deg * std::f64::consts::PI / 180.0;
        self.c.op("Tm", &[cos(r), sin(r) + tan(0.0), -sin(r) + tan(0.0), cos(r), x, y]);
        for l in lines {
            self.c.show_hex(&font.codes(l));
            self.c.op("T*", &[]);
        }
        self.c.op("ET", &[]);
        self.c.op("Q", &[]);
    }
    fn path(&mut self, pts: &[P], close: bool) {
        for op in geom::path_ops(pts, close) {
            match op {
                Op::M(p) => self.c.op("m", &[p.x, p.y]),
                Op::L(p) => self.c.op("l", &[p.x, p.y]),
                Op::C(a, b, p) => self.c.op("c", &[a.x, a.y, b.x, b.y, p.x, p.y]),
                Op::Z => self.c.op("h", &[]),
            }
        }
    }
}
impl Default for Canvas {
    fn default() -> Self {
        Self::new()
    }
}

/// Markup colours.
fn note_color(k: &str) -> Rgb {
    hex_rgb(match k {
        "open" => "#D9480F",
        "answered" => "#1F6FEB",
        "closed" => "#6E7781",
        "cloud" => "#D62728",
        _ => "#FFD400",
    })
}

/// A revision cloud round a rectangle: op('M', x, y) / op('C', 6 numbers).
pub fn cloud_path(x0: f64, y0: f64, x1: f64, y1: f64, d: f64, op: &mut dyn FnMut(char, &[f64])) {
    let corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    let cx = (x0 + x1) / 2.0;
    let cy = (y0 + y1) / 2.0;
    op('M', &[x0, y0]);
    for e in 0..4 {
        let [ax, ay] = corners.get(e).copied().unwrap_or([0.0, 0.0]);
        let [bx, by] = corners.get((e.saturating_add(1)) % 4).copied().unwrap_or([0.0, 0.0]);
        let len = cc_m3d::js::hypot2(bx - ax, by - ay);
        let nf = cc_m3d::js::max(1.0, cc_m3d::js::round(len / d));
        let nn = if nf.is_finite() && nf < 1e6 { nf } else { 1.0 };
        let (mut nx, mut ny) = (-(by - ay) / len, (bx - ax) / len);
        let mx = (ax + bx) / 2.0;
        let my = (ay + by) / 2.0;
        if nx * (mx - cx) + ny * (my - cy) < 0.0 {
            nx = -nx;
            ny = -ny;
        }
        let hgt = (len / nn) * 0.55;
        let mut i = 0.0;
        while i < nn {
            let px = ax + (bx - ax) * i / nn;
            let py = ay + (by - ay) * i / nn;
            let qx = ax + (bx - ax) * (i + 1.0) / nn;
            let qy = ay + (by - ay) * (i + 1.0) / nn;
            op('C', &[px + nx * hgt, py + ny * hgt, qx + nx * hgt, qy + ny * hgt, qx, qy]);
            i += 1.0;
        }
    }
}

/// `l.match(/.{1,48}(\s|$)|\S+/g) || ['']` on UTF-16 units.
fn wrap48(l: &str) -> Vec<String> {
    let u: Vec<u16> = l.encode_utf16().collect();
    let is_term = |c: u16| matches!(c, 0x0A | 0x0D | 0x2028 | 0x2029);
    let is_ws = |c: u16| char::from_u32(u32::from(c)).is_some_and(jv::is_space);
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < u.len() {
        let mut found = None;
        let max = 48.min(u.len().saturating_sub(i));
        for len in (1..=max).rev() {
            if u.get(i..i.saturating_add(len)).is_some_and(|s| s.iter().any(|c| is_term(*c))) {
                continue;
            }
            let end = i.saturating_add(len);
            if end == u.len() {
                found = Some(end);
                break;
            }
            if u.get(end).is_some_and(|c| is_ws(*c)) {
                found = Some(end.saturating_add(1));
                break;
            }
        }
        if found.is_none() && u.get(i).is_some_and(|c| !is_ws(*c)) {
            let mut e = i;
            while u.get(e).is_some_and(|c| !is_ws(*c)) {
                e = e.saturating_add(1);
            }
            found = Some(e);
        }
        match found {
            Some(e) => {
                out.push(String::from_utf16_lossy(u.get(i..e).unwrap_or(&[])));
                i = e;
            }
            None => i = i.saturating_add(1),
        }
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn note_label(cv: &mut Canvas, w: &mut Writer, nt: &Value, x: f64, y: f64, lw: f64) {
    let status = jv::to_string(nt.get("status"));
    let head = jv::upper(&t(&format!("note.status.{status}")));
    let mut text_lines: Vec<String> =
        jv::to_string_or_empty(nt.get("text").filter(|v| jv::truthy(Some(v)))).split('\n').flat_map(wrap48).collect();
    text_lines.truncate(8);
    let lines: Vec<String> = std::iter::once(head).chain(text_lines).map(|l| win_ansi(cc_m3d::js::trim(&l))).collect();
    let size = lw * 7.0;
    let lh = size * 1.3;
    let width = lines
        .iter()
        .enumerate()
        .map(|(i, l)| if i > 0 { Font::Regular } else { Font::Bold }.width(l, size))
        .fold(f64::NEG_INFINITY, cc_m3d::js::max)
        + size;
    let h = lines.len() as f64 * lh + size * 0.4;
    let kind = jv::to_string(nt.get("kind"));
    let border = note_color(if kind == "note" {
        &status
    } else if kind == "cloud" {
        "cloud"
    } else {
        &status
    });
    cv.rect(w, x, y - h, width, h, 0.0, Some((1.0, 1.0, 1.0)), Some(border), lw * 0.8, Some(0.92));
    for (i, l) in lines.iter().enumerate() {
        cv.text(
            l,
            x + size / 2.0,
            y - (i as f64 + 1.0) * lh + size * 0.15,
            size,
            if i > 0 { Font::Regular } else { Font::Bold },
            (0.1, 0.1, 0.12),
            0.0,
        );
    }
}

fn draw_notes(cv: &mut Canvas, w: &mut Writer, notes: &[&Value], lw: f64) {
    let hl = w.add(&PO::dict(vec![("Type", PO::name("ExtGState")), ("ca", PO::Num(0.35)), ("CA", PO::Num(1.0))]));
    cv.ext_g_states.push(("TcHl".into(), hl));
    cv.c.op("q", &[]);
    let mut post: Vec<(&Value, f64, f64)> = Vec::new();
    for nt in notes {
        let pts = model::pts_of(nt.get("pts"));
        let kind = jv::to_string(nt.get("kind"));
        if kind == "highlight" || kind == "cloud" {
            let (Some(a), Some(b)) = (pts.first(), pts.get(1)) else { continue };
            let (x0, y0, x1, y1) =
                (cc_m3d::js::min(a.x, b.x), cc_m3d::js::min(a.y, b.y), cc_m3d::js::max(a.x, b.x), cc_m3d::js::max(a.y, b.y));
            if kind == "highlight" {
                cv.c.op("q", &[]);
                cv.c.op_name("gs", "TcHl", &[]);
                cv.rg(note_color("highlight"));
                cv.c.op("m", &[x0, y0]);
                cv.c.op("l", &[x1, y0]);
                cv.c.op("l", &[x1, y1]);
                cv.c.op("l", &[x0, y1]);
                cv.c.op("h", &[]);
                cv.c.op("f", &[]);
                cv.c.op("Q", &[]);
                if jv::truthy(nt.get("text")) {
                    post.push((nt, x1 + lw * 3.0, y1));
                }
            } else {
                cv.c.op("q", &[]);
                cv.c.op("w", &[lw * 1.4]);
                cv.rg_stroke(note_color("cloud"));
                let c = &mut cv.c;
                cloud_path(x0, y0, x1, y1, lw * 9.0, &mut |op, v| if op == 'M' { c.op("m", v) } else { c.op("c", v) });
                cv.c.op("h", &[]);
                cv.c.op("S", &[]);
                cv.c.op("Q", &[]);
                post.push((nt, x1 + lw * 6.0, y1));
            }
        } else {
            let Some(p) = pts.first() else { continue };
            let (x, y) = (p.x, p.y);
            let r = lw * 5.0;
            let k = 0.5523 * r;
            cv.c.op("q", &[]);
            cv.rg(note_color(&jv::to_string(nt.get("status"))));
            dot(&mut cv.c, x, y, r, k);
            cv.c.op("Q", &[]);
            post.push((nt, x + r * 1.6, y + r));
        }
    }
    cv.c.op("Q", &[]);
    for (nt, x, y) in post {
        note_label(cv, w, nt, x, y, lw);
    }
}

fn dot(c: &mut Content, x: f64, y: f64, r: f64, k: f64) {
    c.op("m", &[x + r, y]);
    c.op("c", &[x + r, y + k, x + k, y + r, x, y + r]);
    c.op("c", &[x - k, y + r, x - r, y + k, x - r, y]);
    c.op("c", &[x - r, y - k, x - k, y - r, x, y - r]);
    c.op("c", &[x + k, y - r, x + r, y - k, x + r, y]);
    c.op("h", &[]);
    c.op("f", &[]);
}

/// pdf-lib's `getCropBox()` of a page: the (inherited) CropBox, else MediaBox, as x, y, width, height.
fn crop_box(pdf: &Pdf, page: &Dict<'_>) -> [f64; 4] {
    let arr = inherited(pdf, page, b"CropBox").or_else(|| inherited(pdf, page, b"MediaBox"));
    let v: Vec<f64> = arr.map(|a| a.iter::<Number>().take(4).map(|n| n.as_f64()).collect()).unwrap_or_default();
    match v.as_slice() {
        [a, b, c, d] => [*a, *b, *c - *a, *d - *b],
        _ => [0.0, 0.0, 612.0, 792.0],
    }
}
fn inherited<'a>(_pdf: &'a Pdf, page: &Dict<'a>, key: &[u8]) -> Option<Array<'a>> {
    let mut d = page.clone();
    for _ in 0..64 {
        if let Some(a) = d.get::<Array<'_>>(key) {
            return Some(a);
        }
        d = d.get::<Dict<'_>>(b"Parent".as_slice())?;
    }
    None
}
fn inherited_dict<'a>(page: &Dict<'a>, key: &[u8]) -> Option<Dict<'a>> {
    let mut d = page.clone();
    for _ in 0..64 {
        if let Some(a) = d.get::<Dict<'_>>(key) {
            return Some(a);
        }
        d = d.get::<Dict<'_>>(b"Parent".as_slice())?;
    }
    None
}
fn inherited_num(page: &Dict<'_>, key: &[u8]) -> Option<f64> {
    let mut d = page.clone();
    for _ in 0..64 {
        if let Some(n) = d.get::<Number>(key) {
            return Some(n.as_f64());
        }
        d = d.get::<Dict<'_>>(b"Parent".as_slice())?;
    }
    None
}

/// The result of drawing: bytes, pages marked, marks drawn.
#[derive(Debug, Clone)]
pub struct Marked {
    pub bytes: Vec<u8>,
    pub pages: usize,
    pub marks: usize,
}

/// `pdfWithMarks(PDFLib, bytes, fileHash, state, { legend })`.
pub fn pdf_with_marks(bytes: &[u8], file_hash: &str, state: &Value, legend: bool) -> Result<Marked, String> {
    let tr = read_trailer(bytes)?;
    if tr.encrypted {
        return Err(t("pdf.err.encrypted"));
    }
    let pdf = Pdf::new(bytes.to_vec()).map_err(|e| t("pdf.err.read").replace("{reason}", &format!("{e:?}")))?;
    let mut w = Writer::new(u32::try_from(tr.size).unwrap_or(1));
    let helv = w.add(&PO::dict(vec![
        ("Type", PO::name("Font")),
        ("Subtype", PO::name("Type1")),
        ("BaseFont", PO::name("Helvetica")),
        ("Encoding", PO::name("WinAnsiEncoding")),
    ]));
    let helv_b = w.add(&PO::dict(vec![
        ("Type", PO::name("Font")),
        ("Subtype", PO::name("Type1")),
        ("BaseFont", PO::name("Helvetica-Bold")),
        ("Encoding", PO::name("WinAnsiEncoding")),
    ]));
    let items: HashMap<String, &Value> =
        jv::arr(state.get("items")).map(|l| l.iter().map(|i| (jv::to_string(i.get("id")), i)).collect()).unwrap_or_default();
    let q = model::quantities(state, &QOpts::default());
    let mut pq: Option<(PO, PO)> = None;
    let (mut marked, mut drawn) = (0usize, 0usize);
    let all_marks = jv::arr(state.get("marks")).map_or(&[][..], Vec::as_slice);
    let all_notes = jv::arr(state.get("notes")).map_or(&[][..], Vec::as_slice);
    for (p, page) in pdf.pages().iter().enumerate() {
        let key = format!("{file_hash}:{p}");
        let marks: Vec<&Value> = all_marks
            .iter()
            .filter(|m| jv::str(m.get("sheet")) == Some(key.as_str()) && items.contains_key(&jv::to_string(m.get("item"))))
            .collect();
        let notes: Vec<&Value> = all_notes.iter().filter(|n| jv::str(n.get("sheet")) == Some(key.as_str())).collect();
        if marks.is_empty() && notes.is_empty() {
            continue;
        }
        let pd = page.raw();
        let bx = crop_box(&pdf, pd);
        let lw = cc_m3d::js::max(0.75, cc_m3d::js::min(bx[2], bx[3]) / 600.0);
        let mut cv = Canvas::new();
        let fill = w.add(&PO::dict(vec![("Type", PO::name("ExtGState")), ("ca", PO::Num(0.22)), ("CA", PO::Num(1.0))]));
        let dotg = w.add(&PO::dict(vec![("Type", PO::name("ExtGState")), ("ca", PO::Num(0.85)), ("CA", PO::Num(1.0))]));
        cv.ext_g_states.push(("TcFill".into(), fill));
        cv.ext_g_states.push(("TcDot".into(), dotg));
        cv.c.op("q", &[]);
        cv.c.op("J", &[1.0]);
        cv.c.op("j", &[1.0]);
        cv.c.op("w", &[lw]);
        for m in &marks {
            let it = items.get(&jv::to_string(m.get("item"))).copied();
            let color = hex_rgb(&it.map(|i| jv::to_string(i.get("color"))).unwrap_or_default());
            cv.rg_stroke(color);
            cv.rg(color);
            let pts = model::pts_of(m.get("pts"));
            match jv::str(m.get("kind")) {
                Some("linear") => {
                    let ded: Vec<f64> =
                        jv::arr(m.get("deducts")).map(|l| l.iter().map(|v| jv::to_number(Some(v))).collect()).unwrap_or_default();
                    let is_ded = |i: usize| ded.contains(&(i as f64));
                    for sp in geom::spans(&pts, &is_ded) {
                        if sp.deduct {
                            cv.c.op("q", &[]);
                            cv.c.op("w", &[lw * 0.6]);
                            cv.c.dash(&[lw * 4.0, lw * 3.0], 0.0);
                        }
                        cv.path(&sp.pts, false);
                        cv.c.op("S", &[]);
                        if sp.deduct {
                            cv.c.op("Q", &[]);
                        }
                    }
                }
                Some("area") => {
                    let holes = model::holes_of(m.get("holes"));
                    cv.c.op("q", &[]);
                    cv.c.op_name("gs", "TcFill", &[]);
                    cv.path(&pts, true);
                    for h in &holes {
                        cv.path(h, true);
                    }
                    cv.c.op("f*", &[]);
                    cv.c.op("Q", &[]);
                    cv.path(&pts, true);
                    for h in &holes {
                        cv.path(h, true);
                    }
                    cv.c.op("S", &[]);
                }
                _ => {
                    let r = lw * 3.5;
                    let k = 0.5523 * r;
                    let p0 = pts.first().copied().unwrap_or_default();
                    cv.c.op("q", &[]);
                    cv.c.op_name("gs", "TcDot", &[]);
                    dot(&mut cv.c, p0.x, p0.y, r, k);
                    cv.c.op("Q", &[]);
                }
            }
            drawn = drawn.saturating_add(1);
        }
        cv.c.op("Q", &[]);
        if !notes.is_empty() {
            draw_notes(&mut cv, &mut w, &notes, lw);
        }
        if legend && !marks.is_empty() {
            draw_legend(&mut cv, &mut w, pd, bx, &marks, &items, &q, &key);
        }
        // the page: [q, its own content…, Q, ours], resources with ours added
        let (push, pop) = match &pq {
            Some(x) => x.clone(),
            None => {
                let a = w.add_stream(vec![], b"q");
                let b = w.add_stream(vec![], b"Q");
                pq = Some((a.clone(), b.clone()));
                (a, b)
            }
        };
        let ours = w.add_stream(vec![], &cv.c.bytes);
        let mut contents: Vec<PO> = Vec::new();
        let mut has_contents = false;
        match pd.get_raw::<Object<'_>>(b"Contents".as_slice()) {
            Some(MaybeRef::Ref(r)) => {
                has_contents = true;
                let target = pdf
                    .xref()
                    .get::<Object<'_>>(hayro::hayro_interpret::hayro_syntax::object::ObjectIdentifier::new(r.obj_number, r.gen_number));
                match target {
                    Some(Object::Array(a)) => {
                        for x in a.raw_iter().take(100_000) {
                            let mut b = Vec::new();
                            copy_obj(&mut b, &x, 1);
                            contents.push(PO::Raw(b));
                        }
                    }
                    _ => {
                        let mut b = Vec::new();
                        copy_obj(&mut b, &MaybeRef::Ref(r), 1);
                        contents.push(PO::Raw(b));
                    }
                }
            }
            Some(MaybeRef::NotRef(Object::Array(a))) => {
                has_contents = true;
                for x in a.raw_iter().take(100_000) {
                    let mut b = Vec::new();
                    copy_obj(&mut b, &x, 1);
                    contents.push(PO::Raw(b));
                }
            }
            _ => {}
        }
        if has_contents {
            contents.insert(0, push);
            contents.push(pop);
        }
        contents.push(ours);
        let res = inherited_dict(pd, b"Resources");
        let mut res_entries: Vec<(String, PO)> =
            res.as_ref().map(|r| dict_entries(r, &["Font", "XObject", "ExtGState"])).unwrap_or_default();
        let sub = |k: &[u8]| -> Vec<(String, PO)> {
            res.as_ref().and_then(|r| r.get::<Dict<'_>>(k)).map(|d| dict_entries(&d, &[])).unwrap_or_default()
        };
        let mut fonts = sub(b"Font");
        for (f, n) in &cv.fonts {
            fonts.push((n.clone(), if *f == Font::Regular { helv.clone() } else { helv_b.clone() }));
        }
        let mut gss = sub(b"ExtGState");
        gss.extend(cv.ext_g_states.iter().cloned());
        res_entries.push(("Font".into(), PO::Dict(fonts)));
        res_entries.push(("XObject".into(), PO::Dict(sub(b"XObject"))));
        res_entries.push(("ExtGState".into(), PO::Dict(gss)));
        let mut pdict = dict_entries(pd, &["Contents", "Resources"]);
        pdict.push(("Contents".into(), PO::Arr(contents)));
        pdict.push(("Resources".into(), PO::Dict(res_entries)));
        if !pdict.iter().any(|(k, _)| k == "Annots") {
            pdict.push(("Annots".into(), PO::Arr(Vec::new())));
        }
        let (n, g) = pd.obj_id().and_then(id_parts).ok_or("a page is not an indirect object")?;
        let mut body = Vec::new();
        crate::pdfw::write(&mut body, &PO::Dict(pdict));
        w.put(n, g, body);
        marked = marked.saturating_add(1);
    }
    // Info: the original's entries with our Producer
    let mut info: Vec<(String, PO)> = tr
        .info
        .and_then(|(n, g)| {
            pdf.xref().get::<Dict<'_>>(hayro::hayro_interpret::hayro_syntax::object::ObjectIdentifier::new(
                i32::try_from(n).unwrap_or(0),
                i32::from(g),
            ))
        })
        .map(|d| dict_entries(&d, &["Producer"]))
        .unwrap_or_default();
    info.push(("Producer".into(), PO::Str(b"TakeoffCraft (pdf-lib)".to_vec())));
    let info_ref = w.add(&PO::Dict(info));
    let root = tr.root.ok_or("no catalog")?;
    let out = w.finish(bytes, vec![("Root", PO::Ref(root.0, root.1)), ("Info", info_ref), ("Prev", PO::Num(tr.startxref as f64))]);
    Ok(Marked { bytes: out, pages: marked, marks: drawn })
}

#[allow(clippy::too_many_arguments)]
fn draw_legend(
    cv: &mut Canvas,
    w: &mut Writer,
    pd: &Dict<'_>,
    bx: [f64; 4],
    marks: &[&Value],
    items: &HashMap<String, &Value>,
    q: &model::Quantities,
    key: &str,
) {
    let ang = inherited_num(pd, b"Rotate").unwrap_or(0.0);
    let rot = ((ang % 360.0) + 360.0) % 360.0;
    let (x0, y0, x1, y1) = (bx[0], bx[1], bx[0] + bx[2], bx[1] + bx[3]);
    let to_user = |vx: f64, vy: f64| -> (f64, f64) {
        if rot == 90.0 {
            (x1 - vy, y0 + vx)
        } else if rot == 180.0 {
            (x1 - vx, y1 - vy)
        } else if rot == 270.0 {
            (x0 + vy, y1 - vx)
        } else {
            (x0 + vx, y0 + vy)
        }
    };
    let fs = cc_m3d::js::max(6.0, cc_m3d::js::min(bx[2], bx[3]) / 120.0);
    let mut lines: Vec<(String, String)> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for m in marks {
        let id = jv::to_string(m.get("item"));
        if seen.contains(&id) {
            continue;
        }
        seen.push(id.clone());
        let Some(it) = items.get(&id) else { continue };
        let tq = q.get(&jv::to_string(it.get("id"))).and_then(|qq| qq.sheet(key));
        let qty = match tq {
            Some(tq) => {
                let ty = jv::to_string(it.get("type"));
                let mut parts = vec![format!(
                    "{} {}",
                    units::fixed(model::item_qty(it, tq), if ty == "count" { 0 } else { 2 }),
                    units::unit_label(&jv::to_string(it.get("unit")))
                )];
                for c in jv::arr(it.get("conds")).map_or(&[][..], Vec::as_slice) {
                    let ck = jv::to_string(c.get("kind"));
                    if let Some(ct) = tq.cond(&ck) {
                        parts.push(format!(
                            "{} {}",
                            units::fixed(model::cond_qty(c, Some(ct)), if ck == "count" { 0 } else { 2 }),
                            units::unit_label(&jv::to_string(c.get("unit")))
                        ));
                    }
                }
                parts.join(" + ")
            }
            None => t("pdf.legend.noscale"),
        };
        let name = jv::to_string(it.get("name"));
        let u: Vec<u16> = name.encode_utf16().collect();
        let nm = if u.len() > 60 {
            let mut v: Vec<u16> = u.iter().take(59).copied().collect();
            v.push(0x2026);
            v
        } else {
            u
        };
        // winAnsi on UTF-16 units (a cut surrogate pair becomes "?")
        let wn: String =
            nm.iter().map(|c| char::from_u32(u32::from(*c)).filter(|ch| stdfont::winansi(*ch).is_some()).unwrap_or('?')).collect();
        lines.push((jv::to_string(it.get("color")), format!("{wn}  {}", win_ansi(&qty))));
    }
    let title = win_ansi(&t("pdf.legend.title"));
    let pad = fs * 0.8;
    let lh = fs * 1.45;
    let sw = fs * 0.9;
    let widest = lines.iter().map(|(_, l)| sw + fs * 0.5 + Font::Regular.width(l, fs)).fold(Font::Bold.width(&title, fs), cc_m3d::js::max);
    let width = cc_m3d::js::min(widest + pad * 2.0, (if rot % 180.0 != 0.0 { bx[3] } else { bx[2] }) * 0.6);
    let height = pad * 2.0 + lh * (lines.len() as f64 + 1.0);
    let mm = fs * 1.5;
    let (ox, oy) = to_user(mm, mm);
    cv.rect(w, ox, oy, width, height, rot, Some((1.0, 1.0, 1.0)), Some((0.1, 0.12, 0.17)), 0.75, Some(0.92));
    let mut vy = mm + height - pad - fs;
    let (tx, ty) = to_user(mm + pad, vy);
    cv.text(&title, tx, ty, fs, Font::Bold, (0.1, 0.12, 0.17), rot);
    for (color, text) in &lines {
        vy -= lh;
        let (rx, ry) = to_user(mm + pad, vy - fs * 0.05);
        cv.rect(w, rx, ry, sw, sw * 0.8, rot, Some(hex_rgb(color)), None, 0.0, None);
        let (tx, ty) = to_user(mm + pad + sw + fs * 0.5, vy);
        cv.text(text, tx, ty, fs, Font::Regular, (0.1, 0.12, 0.17), rot);
    }
}
