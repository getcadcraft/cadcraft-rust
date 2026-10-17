//! A page's text the way pdf.js `getTextContent()` hands it over (MarkupCraft reads sheet numbers and titles from it):
//! the content stream is walked with the text state (fonts, matrices, spacing, form XObjects), glyphs are laid out
//! with the font's widths, and runs are joined into items — with pdf.js's rules for when a run continues, when a
//! space is inserted and when a new item starts (`compareWithLastPosition`, `addFakeSpaces`, `flushTextContentItem`).
//!
//! Fonts: simple fonts (Type1, TrueType, MMType1) with /Widths, /Encoding (base + /Differences), /ToUnicode; the
//! standard 14 Helvetica family without /Widths takes Helvetica's widths (font.js tables, by character); Type0 fonts
//! read two-byte codes with /W, /DW and /ToUnicode. Spec gap: Type3 fonts, vertical writing, embedded font programs'
//! own widths, bidi reordering and pdf.js's Unicode normalisation are not reproduced (a page using them can read
//! differently); MarkupCraft only needs the words of title blocks.

use crate::jsdoc::JsDoc;
use crate::obj::{Dict, Lexer, Obj};
use cc_markup::js::hypot;

/// One text item.
#[derive(Debug, Clone, PartialEq)]
pub struct TextItem {
    pub str: String,
    pub transform: [f64; 6],
    pub width: f64,
    pub height: f64,
    pub has_eol: bool,
}

type M = [f64; 6];
const IDENT: M = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `Util.transform(m1, m2)`.
fn mul(m1: &M, m2: &M) -> M {
    [
        m1[0] * m2[0] + m1[2] * m2[1],
        m1[1] * m2[0] + m1[3] * m2[1],
        m1[0] * m2[2] + m1[2] * m2[3],
        m1[1] * m2[2] + m1[3] * m2[3],
        m1[0] * m2[4] + m1[2] * m2[5] + m1[4],
        m1[1] * m2[4] + m1[3] * m2[5] + m1[5],
    ]
}

#[derive(Debug, Clone)]
struct Font {
    /// the font's name (BaseFont) — pdf.js compares it when the font resource changes
    name: String,
    two_byte: bool,
    first_char: u32,
    widths: Vec<f64>,
    cid_widths: Vec<(u32, u32, f64)>,
    default_width: f64,
    missing_width: f64,
    to_unicode: Vec<(u32, String)>,
    encoding: Vec<(u32, char)>,
    helvetica: Option<bool>,
}

#[derive(Debug, Clone)]
struct State {
    ctm: M,
    tm: M,
    tlm: M,
    font: Option<std::rc::Rc<Font>>,
    font_name: String,
    font_size: f64,
    char_spacing: f64,
    word_spacing: f64,
    h_scale: f64,
    leading: f64,
    rise: f64,
}

impl State {
    fn new() -> Self {
        State {
            ctm: IDENT,
            tm: IDENT,
            tlm: IDENT,
            font: None,
            font_name: String::new(),
            font_size: 0.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            h_scale: 1.0,
            leading: 0.0,
            rise: 0.0,
        }
    }
    fn translate_tm(&mut self, x: f64, y: f64) {
        let m = &mut self.tm;
        m[4] += m[0] * x + m[2] * y;
        m[5] += m[1] * x + m[3] * y;
    }
    fn translate_tlm(&mut self, x: f64, y: f64) {
        let m = &mut self.tlm;
        m[4] += m[0] * x + m[2] * y;
        m[5] += m[1] * x + m[3] * y;
    }
    fn carriage_return(&mut self) {
        self.translate_tlm(0.0, -self.leading);
        self.tm = self.tlm;
    }
    fn text_transform(&self) -> M {
        let tsm = [self.font_size * self.h_scale, 0.0, 0.0, self.font_size, 0.0, self.rise];
        mul(&self.ctm, &mul(&self.tm, &tsm))
    }
}

#[derive(Debug, Clone, Default)]
struct Chunk {
    initialized: bool,
    str: Vec<String>,
    total_width: f64,
    total_height: f64,
    width: f64,
    height: f64,
    prev_transform: Option<M>,
    prev_rise: f64,
    advance_scale: f64,
    space_in_flow_min: f64,
    space_in_flow_max: f64,
    tracking_space_min: f64,
    negative_space_max: f64,
    not_a_space: f64,
    transform: M,
    font_name: String,
    has_eol: bool,
}

struct Ctx<'a> {
    doc: &'a JsDoc,
    view: [f64; 4],
    items: Vec<TextItem>,
    chunk: Chunk,
    last2: [bool; 2],
    last_pos: usize,
    cur_font_size: f64,
    cur_font_name: String,
    cur_font_base: String,
    have_cur: bool,
    fonts: Vec<(String, std::rc::Rc<Font>)>,
    budget: usize,
}

/// `[' ', ' ']` ring: true = the char was a space.
impl Ctx<'_> {
    fn save_last_char(&mut self, is_space: bool) -> bool {
        let next = (self.last_pos.saturating_add(1)) % 2;
        let ret = !self.last2.get(self.last_pos).copied().unwrap_or(true) && self.last2.get(next).copied().unwrap_or(true);
        if let Some(s) = self.last2.get_mut(self.last_pos) {
            *s = is_space;
        }
        self.last_pos = next;
        ret
    }
    fn should_add_whitespace(&self) -> bool {
        !self.last2.get(self.last_pos).copied().unwrap_or(true)
            && self.last2.get((self.last_pos.saturating_add(1)) % 2).copied().unwrap_or(true)
    }
    fn reset_last_chars(&mut self) {
        self.last2 = [true, true];
        self.last_pos = 0;
    }
    fn push_whitespace(&mut self, width: f64, height: f64, transform: Option<M>) {
        let t = transform.or(self.chunk.prev_transform).unwrap_or(IDENT);
        self.items.push(TextItem { str: " ".into(), transform: t, width, height, has_eol: false });
    }
    fn ensure(&mut self, st: &State) {
        if self.chunk.initialized {
            return;
        }
        let trm = st.text_transform();
        self.chunk.transform = trm;
        self.chunk.font_name = st.font_name.clone();
        self.chunk.width = 0.0;
        self.chunk.total_width = 0.0;
        self.chunk.height = hypot(trm[2], trm[3]);
        self.chunk.total_height = self.chunk.height;
        let scale_line = hypot(st.tlm[0], st.tlm[1]);
        let scale_ctm = hypot(st.ctm[0], st.ctm[1]);
        self.chunk.advance_scale = scale_ctm * scale_line;
        let fs = st.font_size;
        self.chunk.tracking_space_min = fs * 0.102;
        self.chunk.not_a_space = fs * 0.03;
        self.chunk.negative_space_max = fs * -0.2;
        self.chunk.space_in_flow_min = fs * 0.102;
        self.chunk.space_in_flow_max = fs * 0.6;
        self.chunk.has_eol = false;
        self.chunk.initialized = true;
    }
    fn update_advance_scale(&mut self, st: &State) {
        if !self.chunk.initialized {
            return;
        }
        let f = hypot(st.ctm[0], st.ctm[1]) * hypot(st.tlm[0], st.tlm[1]);
        if f == self.chunk.advance_scale {
            return;
        }
        self.chunk.total_width += self.chunk.width * self.chunk.advance_scale;
        self.chunk.width = 0.0;
        self.chunk.advance_scale = f;
    }
    fn flush(&mut self) {
        if !self.chunk.initialized {
            return;
        }
        self.chunk.total_width += self.chunk.width * self.chunk.advance_scale;
        let s: String = self.chunk.str.concat();
        self.items.push(TextItem {
            str: s,
            transform: self.chunk.transform,
            width: self.chunk.total_width.abs(),
            height: self.chunk.total_height.abs(),
            has_eol: self.chunk.has_eol,
        });
        self.chunk.initialized = false;
        self.chunk.str.clear();
    }
    fn append_eol(&mut self, st: &State) {
        self.reset_last_chars();
        if self.chunk.initialized {
            self.chunk.has_eol = true;
            self.flush();
        } else {
            self.items.push(TextItem { str: String::new(), transform: st.text_transform(), width: 0.0, height: 0.0, has_eol: true });
        }
    }
    fn add_fake_spaces(&mut self, width: f64, transf: Option<M>, orient: f64, st: &State) -> bool {
        if orient * self.chunk.space_in_flow_min <= width && width <= orient * self.chunk.space_in_flow_max {
            if self.chunk.initialized {
                self.reset_last_chars();
                self.chunk.str.push(" ".into());
            }
            return false;
        }
        self.flush();
        self.reset_last_chars();
        let t = transf.unwrap_or_else(|| st.text_transform());
        self.push_whitespace(width.abs(), 0.0, Some(t));
        true
    }
    fn compare_with_last_position(&mut self, glyph_width: f64, st: &State) -> bool {
        let cur = st.text_transform();
        let (mut px, mut py) = (cur[4], cur[5]);
        let vb = self.view;
        if px + glyph_width < vb[0] || px > vb[2] || py < vb[1] || py > vb[3] {
            return false;
        }
        let Some(prev) = self.chunk.prev_transform else { return true };
        if st.font.is_none() {
            return true;
        }
        let (mut lx, mut ly) = (prev[4], prev[5]);
        if lx == px && ly == py {
            return true;
        }
        let rotate = if cur[0] != 0.0 && !cur[0].is_nan() && cur[1] == 0.0 && cur[2] == 0.0 {
            if cur[0] > 0.0 { 0 } else { 180 }
        } else if cur[1] != 0.0 && !cur[1].is_nan() && cur[0] == 0.0 && cur[3] == 0.0 {
            if cur[1] > 0.0 { 90 } else { 270 }
        } else {
            -1
        };
        match rotate {
            0 => {}
            90 => {
                (px, py) = (py, px);
                (lx, ly) = (ly, lx);
            }
            180 => {
                (px, py, lx, ly) = (-px, -py, -lx, -ly);
            }
            270 => {
                (px, py) = (-py, -px);
                (lx, ly) = (-ly, -lx);
            }
            _ => {
                let inv = |x: f64, y: f64, m: &M| {
                    let s = hypot(m[0], m[1]);
                    ((m[0] * x + m[1] * y) / s, (m[2] * x + m[3] * y) / s)
                };
                (px, py) = inv(px, py, &cur);
                (lx, ly) = inv(lx, ly, &prev);
            }
        }
        let advance_x = (px - lx) / self.chunk.advance_scale;
        let advance_y = py - ly;
        let w = if self.chunk.width != 0.0 && !self.chunk.width.is_nan() { self.chunk.width } else { self.chunk.total_width };
        let orient = cc_markup::js::sign(w);
        if advance_x < orient * self.chunk.negative_space_max {
            if advance_y.abs() > 0.5 * self.chunk.height {
                self.append_eol(st);
                return true;
            }
            self.reset_last_chars();
            self.flush();
            return true;
        }
        let rise_delta = st.rise - self.chunk.prev_rise;
        let adv_y_corr = if rise_delta == 0.0 { advance_y } else { advance_y - cur[3] / st.font_size * rise_delta };
        if adv_y_corr.abs() > self.chunk.height {
            self.append_eol(st);
            return true;
        }
        if advance_x <= orient * self.chunk.not_a_space {
            self.reset_last_chars();
        }
        if advance_x <= orient * self.chunk.tracking_space_min {
            if self.should_add_whitespace() {
                self.reset_last_chars();
                self.flush();
                self.push_whitespace(advance_x.abs(), 0.0, None);
            } else {
                self.chunk.width += advance_x;
            }
        } else if !self.add_fake_spaces(advance_x, self.chunk.prev_transform, orient, st) {
            if self.chunk.str.is_empty() {
                self.reset_last_chars();
                self.push_whitespace(advance_x.abs(), 0.0, None);
            } else {
                self.chunk.width += advance_x;
            }
        }
        if advance_y.abs() > self.chunk.height * 0.25 {
            self.flush();
        }
        true
    }

    fn build(&mut self, st: &mut State, chars: Option<&[u8]>, extra_spacing: f64) {
        let Some(font) = st.font.clone() else { return };
        if self.have_cur && (self.cur_font_size != st.font_size || (self.cur_font_name != st.font_name && self.cur_font_base != font.name))
        {
            self.flush();
        }
        self.have_cur = true;
        self.cur_font_size = st.font_size;
        self.cur_font_name = st.font_name.clone();
        self.cur_font_base = font.name.clone();
        let Some(chars) = chars else {
            let cs = st.char_spacing + extra_spacing;
            if cs != 0.0 && !cs.is_nan() {
                st.translate_tm(cs * st.h_scale, 0.0);
            }
            return;
        };
        let glyphs = font.glyphs(chars);
        let scale = 0.001 * st.font_size;
        let n = glyphs.len();
        for (i, (code, uni, gw)) in glyphs.into_iter().enumerate() {
            self.budget = self.budget.saturating_sub(1);
            if self.budget == 0 {
                return;
            }
            let mut char_spacing = st.char_spacing + if i.saturating_add(1) == n { extra_spacing } else { 0.0 };
            let mut scaled = gw * scale;
            if code == 0x20 {
                char_spacing += st.word_spacing;
            }
            let is_ws = !uni.is_empty() && uni.chars().all(cc_markup::js::is_ws);
            if is_ws {
                char_spacing += scaled;
                st.translate_tm(char_spacing * st.h_scale, 0.0);
                self.save_last_char(true);
                continue;
            }
            if !self.compare_with_last_position(scaled, st) {
                st.translate_tm(scaled * st.h_scale, 0.0);
                continue;
            }
            self.ensure(st);
            scaled *= st.h_scale;
            st.translate_tm(scaled, 0.0);
            self.chunk.width += scaled;
            if scaled != 0.0 && !scaled.is_nan() {
                self.chunk.prev_transform = Some(st.text_transform());
                self.chunk.prev_rise = st.rise;
            }
            if self.save_last_char(false) {
                self.chunk.str.push(" ".into());
            }
            self.chunk.str.push(uni);
            if char_spacing != 0.0 && !char_spacing.is_nan() {
                st.translate_tm(char_spacing * st.h_scale, 0.0);
            }
        }
    }
}

impl Font {
    /// (code, unicode, width in 1/1000 em) per glyph.
    fn glyphs(&self, b: &[u8]) -> Vec<(u32, String, f64)> {
        let mut out = Vec::new();
        if self.two_byte {
            for p in b.chunks(2) {
                let code = (u32::from(p.first().copied().unwrap_or(0)) << 8) | u32::from(p.get(1).copied().unwrap_or(0));
                let w = self.cid_widths.iter().find(|(a, z, _)| *a <= code && code <= *z).map_or(self.default_width, |(_, _, w)| *w);
                out.push((code, self.unicode(code), w));
            }
        } else {
            for c in b {
                let code = u32::from(*c);
                let uni = self.unicode(code);
                let w = if !self.widths.is_empty() {
                    match code.checked_sub(self.first_char).and_then(|k| self.widths.get(usize::try_from(k).ok()?)) {
                        Some(w) => *w,
                        None => self.missing_width,
                    }
                } else if let Some(bold) = self.helvetica {
                    uni.chars().next().map_or(0.0, |ch| cc_markup::font::char_advance(ch, bold))
                } else {
                    self.missing_width
                };
                out.push((code, uni, w));
            }
        }
        out
    }
    fn unicode(&self, code: u32) -> String {
        if let Some((_, s)) = self.to_unicode.iter().find(|(c, _)| *c == code) {
            return s.clone();
        }
        if let Some((_, ch)) = self.encoding.iter().find(|(c, _)| *c == code) {
            return ch.to_string();
        }
        if self.two_byte {
            return String::new();
        }
        // WinAnsi-ish fallback
        let b = u8::try_from(code).unwrap_or(b'?');
        win_ansi_char(b).map(|c| c.to_string()).unwrap_or_default()
    }
}

fn win_ansi_char(b: u8) -> Option<char> {
    const HIGH: [u32; 32] = [
        0x20ac, 0, 0x201a, 0x192, 0x201e, 0x2026, 0x2020, 0x2021, 0x2c6, 0x2030, 0x160, 0x2039, 0x152, 0, 0x17d, 0, 0, 0x2018, 0x2019,
        0x201c, 0x201d, 0x2022, 0x2013, 0x2014, 0x2dc, 0x2122, 0x161, 0x203a, 0x153, 0, 0x17e, 0x178,
    ];
    match b {
        0x20..=0x7e | 0xa0..=0xff => Some(char::from(b)),
        0x80..=0x9f => HIGH.get(usize::from(b.wrapping_sub(0x80))).and_then(|u| if *u == 0 { None } else { char::from_u32(*u) }),
        _ => None,
    }
}

/// Glyph names a /Differences array uses for Latin text (Adobe Glyph List, the WinAnsi and StandardEncoding set).
fn glyph_unicode(name: &str) -> Option<char> {
    if let Some(hex) = name.strip_prefix("uni") {
        return u32::from_str_radix(hex.get(..4)?, 16).ok().and_then(char::from_u32);
    }
    if name.chars().count() == 1 {
        return name.chars().next();
    }
    let c = match name {
        "space" => ' ',
        "exclam" => '!',
        "quotedbl" => '"',
        "numbersign" => '#',
        "dollar" => '$',
        "percent" => '%',
        "ampersand" => '&',
        "quotesingle" => '\'',
        "quoteright" => '\u{2019}',
        "parenleft" => '(',
        "parenright" => ')',
        "asterisk" => '*',
        "plus" => '+',
        "comma" => ',',
        "hyphen" | "minus" => '-',
        "period" => '.',
        "slash" => '/',
        "zero" => '0',
        "one" => '1',
        "two" => '2',
        "three" => '3',
        "four" => '4',
        "five" => '5',
        "six" => '6',
        "seven" => '7',
        "eight" => '8',
        "nine" => '9',
        "colon" => ':',
        "semicolon" => ';',
        "less" => '<',
        "equal" => '=',
        "greater" => '>',
        "question" => '?',
        "at" => '@',
        "bracketleft" => '[',
        "backslash" => '\\',
        "bracketright" => ']',
        "asciicircum" => '^',
        "underscore" => '_',
        "grave" | "quoteleft" => '\u{2018}',
        "braceleft" => '{',
        "bar" => '|',
        "braceright" => '}',
        "asciitilde" => '~',
        "bullet" => '\u{2022}',
        "endash" => '\u{2013}',
        "emdash" => '\u{2014}',
        "quotedblleft" => '\u{201c}',
        "quotedblright" => '\u{201d}',
        "degree" => '\u{b0}',
        "eacute" => '\u{e9}',
        _ => return None,
    };
    Some(c)
}

fn std_encoding_name(name: &str) -> Vec<(u32, char)> {
    // MacRoman / Standard for ASCII agree with WinAnsi; only WinAnsi's upper half is filled in
    let _ = name;
    Vec::new()
}

/// Parse a ToUnicode CMap's bfchar / bfrange entries.
fn parse_to_unicode(data: &[u8]) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    let mut lx = Lexer::pdfjs(data);
    let mut toks: Vec<Obj> = Vec::new();
    let hex_code = |o: &Obj| -> Option<u32> {
        let b = o.str_bytes()?;
        if b.len() > 4 {
            return None;
        }
        Some(b.iter().fold(0u32, |a, c| (a << 8) | u32::from(*c)))
    };
    let hex_text = |o: &Obj| -> Option<String> {
        let b = o.str_bytes()?;
        let units: Vec<u16> =
            b.chunks(2).map(|p| u16::from_be_bytes([p.first().copied().unwrap_or(0), p.get(1).copied().unwrap_or(0)])).collect();
        Some(crate::obj::utf16_text(&units))
    };
    let mut guard = 0usize;
    while !lx.done() {
        guard = guard.saturating_add(1);
        if guard > 2_000_000 || out.len() > 70_000 {
            break;
        }
        lx.skip_ws_comments();
        let start = lx.pos;
        if let Some(o) = lx.object(0) {
            toks.push(o);
            continue;
        }
        lx.pos = start;
        let mut kw = Vec::new();
        while let Some(c) = lx.peek() {
            if crate::obj::is_ws(c) || crate::obj::is_delim(c) {
                break;
            }
            kw.push(c);
            lx.pos = lx.pos.saturating_add(1);
        }
        if kw.is_empty() {
            lx.pos = lx.pos.saturating_add(1);
            continue;
        }
        match kw.as_slice() {
            b"endbfchar" => {
                for p in toks.chunks(2) {
                    if let (Some(c), Some(t)) = (p.first().and_then(hex_code), p.get(1).and_then(hex_text)) {
                        out.push((c, t));
                    }
                }
                toks.clear();
            }
            b"endbfrange" => {
                for p in toks.chunks(3) {
                    let (Some(a), Some(z)) = (p.first().and_then(hex_code), p.get(1).and_then(hex_code)) else { continue };
                    match p.get(2) {
                        Some(Obj::Arr(list)) => {
                            for (k, t) in list.iter().enumerate() {
                                let code = a.saturating_add(u32::try_from(k).unwrap_or(0));
                                if code > z {
                                    break;
                                }
                                if let Some(t) = hex_text(t) {
                                    out.push((code, t));
                                }
                            }
                        }
                        Some(o) => {
                            let Some(base) = o.str_bytes() else { continue };
                            let units: Vec<u16> = base
                                .chunks(2)
                                .map(|q| u16::from_be_bytes([q.first().copied().unwrap_or(0), q.get(1).copied().unwrap_or(0)]))
                                .collect();
                            for (k, code) in (a..=z).enumerate().take(65_536) {
                                let mut u = units.clone();
                                if let Some(last) = u.last_mut() {
                                    *last = last.wrapping_add(u16::try_from(k).unwrap_or(0));
                                }
                                out.push((code, crate::obj::utf16_text(&u)));
                            }
                        }
                        None => {}
                    }
                }
                toks.clear();
            }
            _ => toks.clear(),
        }
    }
    out
}

fn load_font(doc: &JsDoc, fd: &Dict) -> Font {
    let sub = doc.dget(fd, "Subtype").and_then(|s| s.as_name().map(str::to_string)).unwrap_or_default();
    let base = doc.dget(fd, "BaseFont").and_then(|s| s.as_name().map(str::to_string)).unwrap_or_default();
    let to_unicode = match doc.dget(fd, "ToUnicode") {
        Some(Obj::Stream(s)) => parse_to_unicode(&s.data),
        _ => Vec::new(),
    };
    let nums = |o: Option<Obj>| -> Vec<f64> {
        match o {
            Some(Obj::Arr(a)) => a.iter().map(|x| doc.resolve(x).as_num().unwrap_or(0.0)).collect(),
            _ => Vec::new(),
        }
    };
    if sub == "Type0" {
        let desc = doc.dget_array(fd, "DescendantFonts").and_then(|a| a.into_iter().next()).map(|d| doc.resolve(&d));
        let mut cid_widths = Vec::new();
        let mut dw = 1000.0;
        if let Some(Obj::Dict(dd)) = &desc {
            if let Some(Obj::Num(d)) = doc.dget(dd, "DW") {
                dw = d;
            }
            if let Some(Obj::Arr(w)) = doc.dget(dd, "W") {
                let w: Vec<Obj> = w.iter().map(|x| doc.resolve(x)).collect();
                let mut i = 0usize;
                while i < w.len() && cid_widths.len() < 70_000 {
                    let a = w.get(i).and_then(Obj::as_num).map_or(0, cc_markup::js::f2u);
                    match w.get(i.saturating_add(1)) {
                        Some(Obj::Arr(list)) => {
                            for (k, x) in list.iter().enumerate() {
                                let c = u32::try_from(a.saturating_add(u64::try_from(k).unwrap_or(0))).unwrap_or(u32::MAX);
                                cid_widths.push((c, c, doc.resolve(x).as_num().unwrap_or(dw)));
                            }
                            i = i.saturating_add(2);
                        }
                        Some(Obj::Num(z)) => {
                            let wv = w.get(i.saturating_add(2)).and_then(Obj::as_num).unwrap_or(dw);
                            cid_widths.push((u32::try_from(a).unwrap_or(0), u32::try_from(cc_markup::js::f2u(*z)).unwrap_or(0), wv));
                            i = i.saturating_add(3);
                        }
                        _ => break,
                    }
                }
            }
        }
        return Font {
            name: base,
            two_byte: true,
            first_char: 0,
            widths: Vec::new(),
            cid_widths,
            default_width: dw,
            missing_width: dw,
            to_unicode,
            encoding: Vec::new(),
            helvetica: None,
        };
    }
    let first_char = doc.dget(fd, "FirstChar").and_then(|x| x.as_num()).map_or(0, |x| u32::try_from(cc_markup::js::f2u(x)).unwrap_or(0));
    let widths = nums(doc.dget(fd, "Widths"));
    let missing_width = match doc.dget(fd, "FontDescriptor") {
        Some(Obj::Dict(d)) => doc.dget(&d, "MissingWidth").and_then(|x| x.as_num()).unwrap_or(0.0),
        _ => 0.0,
    };
    let mut encoding: Vec<(u32, char)> = Vec::new();
    match doc.dget(fd, "Encoding") {
        Some(Obj::Name(n)) => encoding = std_encoding_name(&n),
        Some(Obj::Dict(ed)) => {
            if let Some(Obj::Arr(diffs)) = doc.dget(&ed, "Differences") {
                let mut code = 0u32;
                for d in diffs {
                    match doc.resolve(&d) {
                        Obj::Num(n) => code = u32::try_from(cc_markup::js::f2u(n)).unwrap_or(0),
                        Obj::Name(g) => {
                            if let Some(c) = glyph_unicode(&g) {
                                encoding.push((code, c));
                            }
                            code = code.saturating_add(1);
                        }
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
    let stripped = base.split('+').next_back().unwrap_or(&base).to_string();
    let helvetica = if stripped.starts_with("Helvetica") || stripped.starts_with("Arial") { Some(stripped.contains("Bold")) } else { None };
    // other standard fonts without /Widths: Helvetica's widths are the closest the reader has
    let helvetica = helvetica.or(if widths.is_empty() { Some(stripped.contains("Bold")) } else { None });
    Font {
        name: base,
        two_byte: false,
        first_char,
        widths,
        cid_widths: Vec::new(),
        default_width: 1000.0,
        missing_width,
        to_unicode,
        encoding,
        helvetica,
    }
}

/// Read one content stream's operators.
fn run(ctx: &mut Ctx<'_>, data: &[u8], resources: &Dict, st: &mut State, depth: usize) {
    let mut lx = Lexer::pdfjs(data);
    let mut args: Vec<Obj> = Vec::new();
    let mut stack: Vec<State> = Vec::new();
    while !lx.done() {
        if ctx.budget == 0 {
            return;
        }
        ctx.budget = ctx.budget.saturating_sub(1);
        lx.skip_ws_comments();
        let start = lx.pos;
        if let Some(o) = lx.object(0) {
            if args.len() < 64 {
                args.push(o);
            }
            continue;
        }
        lx.pos = start;
        let mut kw = Vec::new();
        while let Some(c) = lx.peek() {
            if crate::obj::is_ws(c) || crate::obj::is_delim(c) {
                break;
            }
            kw.push(c);
            lx.pos = lx.pos.saturating_add(1);
        }
        if kw.is_empty() {
            lx.pos = lx.pos.saturating_add(1);
            args.clear();
            continue;
        }
        let n = |i: usize| args.get(i).and_then(Obj::as_num).unwrap_or(0.0);
        match kw.as_slice() {
            b"q" => {
                if stack.len() < 256 {
                    stack.push(st.clone());
                }
            }
            b"Q" => {
                if let Some(s) = stack.pop() {
                    *st = s;
                }
            }
            b"cm" if args.len() >= 6 => {
                let m = [n(0), n(1), n(2), n(3), n(4), n(5)];
                st.ctm = mul(&st.ctm, &m);
            }
            b"BT" => {
                st.tm = IDENT;
                st.tlm = IDENT;
            }
            b"Tf" if args.len() >= 2 => {
                let name = args.first().and_then(Obj::as_name).unwrap_or("").to_string();
                let size = n(1);
                if !(st.font.is_some() && name == st.font_name && size == st.font_size) {
                    st.font_name = name.clone();
                    st.font_size = size;
                    let f = match ctx.fonts.iter().find(|(k, _)| *k == name) {
                        Some((_, f)) => Some(f.clone()),
                        None => {
                            let fonts = ctx.doc.dget(resources, "Font");
                            let fd = fonts.as_ref().and_then(|f| f.as_dict()).and_then(|f| ctx.doc.dget(f, &name));
                            match fd {
                                Some(Obj::Dict(d)) => {
                                    let font = std::rc::Rc::new(load_font(ctx.doc, &d));
                                    ctx.fonts.push((name.clone(), font.clone()));
                                    Some(font)
                                }
                                _ => None,
                            }
                        }
                    };
                    st.font = f.or_else(|| {
                        Some(std::rc::Rc::new(Font {
                            name: String::new(),
                            two_byte: false,
                            first_char: 0,
                            widths: Vec::new(),
                            cid_widths: Vec::new(),
                            default_width: 1000.0,
                            missing_width: 0.0,
                            to_unicode: Vec::new(),
                            encoding: Vec::new(),
                            helvetica: Some(false),
                        }))
                    });
                }
            }
            b"Ts" => st.rise = n(0),
            b"Tz" => st.h_scale = n(0) / 100.0,
            b"TL" => st.leading = n(0),
            b"Td" => {
                st.translate_tlm(n(0), n(1));
                st.tm = st.tlm;
            }
            b"TD" => {
                st.leading = -n(1);
                st.translate_tlm(n(0), n(1));
                st.tm = st.tlm;
            }
            b"T*" => st.carriage_return(),
            b"Tm" if args.len() >= 6 => {
                let m = [n(0), n(1), n(2), n(3), n(4), n(5)];
                st.tm = m;
                st.tlm = m;
                ctx.update_advance_scale(st);
            }
            b"Tc" => st.char_spacing = n(0),
            b"Tw" => st.word_spacing = n(0),
            b"Tj" => {
                if let Some(b) = args.first().and_then(Obj::str_bytes) {
                    ctx.build(st, Some(&b), 0.0);
                }
            }
            b"'" => {
                st.carriage_return();
                if let Some(b) = args.first().and_then(Obj::str_bytes) {
                    ctx.build(st, Some(&b), 0.0);
                }
            }
            b"\"" => {
                st.word_spacing = n(0);
                st.char_spacing = n(1);
                st.carriage_return();
                if let Some(b) = args.get(2).and_then(Obj::str_bytes) {
                    ctx.build(st, Some(&b), 0.0);
                }
            }
            b"TJ" => {
                let factor = -st.font_size / 1000.0;
                let mut buf: Vec<u8> = Vec::new();
                if let Some(Obj::Arr(items)) = args.first() {
                    for it in items {
                        if let Some(b) = it.str_bytes() {
                            buf.extend_from_slice(&b);
                        } else if let Some(x) = it.as_num()
                            && x != 0.0
                        {
                            let s = std::mem::take(&mut buf);
                            ctx.build(st, Some(&s), x * factor);
                        }
                    }
                }
                if !buf.is_empty() {
                    ctx.build(st, Some(&buf), 0.0);
                }
            }
            b"Do" if depth < 12 => {
                ctx.flush();
                let name = args.first().and_then(Obj::as_name).unwrap_or("").to_string();
                let xo = ctx.doc.dget(resources, "XObject");
                let x = xo.as_ref().and_then(|x| x.as_dict()).and_then(|x| ctx.doc.dget(x, &name));
                if let Some(Obj::Stream(s)) = x
                    && ctx.doc.dget(&s.dict, "Subtype").as_ref().and_then(Obj::as_name) == Some("Form")
                {
                    let mut st2 = st.clone();
                    if let Some(Obj::Arr(m)) = ctx.doc.dget(&s.dict, "Matrix") {
                        let v: Vec<f64> = m.iter().map(|x| ctx.doc.resolve(x).as_num().unwrap_or(f64::NAN)).collect();
                        if let Ok(mm) = <[f64; 6]>::try_from(v)
                            && mm.iter().all(|x| !x.is_nan())
                        {
                            st2.ctm = mul(&st2.ctm, &mm);
                        }
                    }
                    let res = match ctx.doc.dget(&s.dict, "Resources") {
                        Some(Obj::Dict(r)) => r,
                        _ => resources.clone(),
                    };
                    let saved_chunk = std::mem::take(&mut ctx.chunk);
                    let saved_fonts = std::mem::take(&mut ctx.fonts);
                    let saved_last = (ctx.last2, ctx.last_pos, ctx.have_cur);
                    ctx.reset_last_chars();
                    ctx.have_cur = false;
                    run(ctx, &s.data, &res, &mut st2, depth.saturating_add(1));
                    ctx.flush();
                    ctx.chunk = saved_chunk;
                    ctx.fonts = saved_fonts;
                    (ctx.last2, ctx.last_pos, ctx.have_cur) = saved_last;
                }
            }
            b"BI" => {
                // inline image: skip to EI
                while !lx.done() {
                    if lx.match_kw(b"EI") && lx.peek().is_none_or(crate::obj::is_ws) {
                        break;
                    }
                    lx.pos = lx.pos.saturating_add(1);
                }
            }
            _ => {}
        }
        args.clear();
    }
}

/// `page.getTextContent().items` of one page.
pub fn text_content(doc: &JsDoc, page_index: usize) -> Vec<TextItem> {
    let Some(page) = doc.pages.get(page_index) else { return Vec::new() };
    let data = doc.content(&page.dict);
    let resources = match doc.inherited(&page.dict, "Resources") {
        Some(Obj::Dict(d)) => d,
        _ => Dict::new(),
    };
    let mut ctx = Ctx {
        doc,
        view: doc.view(&page.dict),
        items: Vec::new(),
        chunk: Chunk::default(),
        last2: [true, true],
        last_pos: 0,
        cur_font_size: 0.0,
        cur_font_name: String::new(),
        cur_font_base: String::new(),
        have_cur: false,
        fonts: Vec::new(),
        budget: 5_000_000,
    };
    let mut st = State::new();
    run(&mut ctx, &data, &resources, &mut st, 0);
    ctx.flush();
    ctx.items
}

/// The scale-1 viewport of a page (pdf.js `new PageViewport({viewBox, userUnit, scale: 1, rotation})`):
/// (transform, width, height).
pub fn viewport(view: [f64; 4], rotate: f64, user_unit: f64) -> ([f64; 6], f64, f64) {
    let scale = user_unit;
    let cx = (view[2] + view[0]) / 2.0;
    let cy = (view[3] + view[1]) / 2.0;
    let mut r = rotate % 360.0;
    if r < 0.0 {
        r += 360.0;
    }
    let (ra, rb, rc, rd) = if r == 180.0 {
        (-1.0, 0.0, 0.0, 1.0)
    } else if r == 90.0 {
        (0.0, 1.0, 1.0, 0.0)
    } else if r == 270.0 {
        (0.0, -1.0, -1.0, 0.0)
    } else {
        (1.0, 0.0, 0.0, -1.0)
    };
    let (ox, oy, width, height) = if ra == 0.0 {
        ((cy - view[1]).abs() * scale, (cx - view[0]).abs() * scale, (view[3] - view[1]) * scale, (view[2] - view[0]) * scale)
    } else {
        ((cx - view[0]).abs() * scale, (cy - view[1]).abs() * scale, (view[2] - view[0]) * scale, (view[3] - view[1]) * scale)
    };
    let t =
        [ra * scale, rb * scale, rc * scale, rd * scale, ox - ra * scale * cx - rc * scale * cy, oy - rb * scale * cx - rd * scale * cy];
    (t, width, height)
}
