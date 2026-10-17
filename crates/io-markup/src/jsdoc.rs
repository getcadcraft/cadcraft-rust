//! A PDF as MarkupCraft's reader sees it through pdf.js: opened with `hayro-syntax` (the cross-reference table, object
//! streams, repair of broken files and decryption, as pdf.js does them), objects handed out as owned [`Obj`] trees, and
//! the pieces of pdf.js's document API that `pdf-read.js` uses — pages with their `view` and `rotate`, page labels,
//! the Info dictionary's custom keys, the outline, and `stringToPDFString`.

use crate::obj::{Dict, Lexer, Obj, utf16_text};
use hayro_syntax::Pdf;
use hayro_syntax::object::{MaybeRef, Object, ObjectIdentifier};

/// Why a file could not be opened.
#[derive(Debug, Clone, PartialEq)]
pub enum OpenError {
    /// encrypted with a password we do not have (pdf.js PasswordException)
    Password,
    /// not readable as a PDF (pdf.js's message differs; this is ours)
    Read(String),
}

/// One page: its object and its dictionary.
#[derive(Debug, Clone)]
pub struct JsPage {
    pub id: (u32, u16),
    pub dict: Dict,
}

/// An opened document.
pub struct JsDoc {
    pdf: Pdf,
    pub pages: Vec<JsPage>,
    pub trailer: Option<Dict>,
}

/// Nesting limit when an object tree is copied out.
const MAX_DEPTH: usize = 48;

fn latin1(b: &[u8]) -> String {
    b.iter().map(|c| char::from(*c)).collect()
}

fn conv(o: Object<'_>, depth: usize) -> Obj {
    if depth > MAX_DEPTH {
        return Obj::Null;
    }
    match o {
        Object::Null(_) => Obj::Null,
        Object::Boolean(b) => Obj::Bool(b),
        Object::Number(n) => Obj::Num(n.as_f64()),
        Object::String(s) => Obj::Bytes(s.as_bytes().to_vec()),
        Object::Name(n) => Obj::Name(latin1(n.as_ref())),
        Object::Dict(d) => Obj::Dict(conv_dict(&d, depth)),
        Object::Array(a) => array_items(a.data(), depth),
        Object::Stream(s) => {
            let mut dict = conv_dict(s.dict(), depth);
            let data = s.decoded().map(|c| c.to_vec()).unwrap_or_default();
            dict.delete("Filter");
            dict.delete("DecodeParms");
            Obj::Stream(Box::new(crate::obj::Stream { dict, data }))
        }
    }
}

fn conv_maybe(m: MaybeRef<Object<'_>>, depth: usize) -> Obj {
    match m {
        MaybeRef::Ref(r) => Obj::Ref(u32::try_from(r.obj_number).unwrap_or(0), u16::try_from(r.gen_number).unwrap_or(0)),
        MaybeRef::NotRef(o) => conv(o, depth),
    }
}

/// An array's items, read from its bytes by this crate's lexer with pdf.js's number rules (hayro's array iterator
/// assumes items it skipped once always read back, and panics on a hostile file where they do not).
/// Spec gap: strings inside arrays of an encrypted file are left encrypted (dictionary values are decrypted).
fn array_items(data: &[u8], depth: usize) -> Obj {
    let mut lx = crate::obj::Lexer::pdfjs(data);
    let mut out = Vec::new();
    loop {
        lx.skip_ws_comments();
        if lx.done() || out.len() >= 1_000_000 {
            break;
        }
        match lx.object(depth.saturating_add(1)) {
            Some(o) => out.push(o),
            None => break,
        }
    }
    Obj::Arr(out)
}

fn conv_dict(d: &hayro_syntax::object::Dict<'_>, depth: usize) -> Dict {
    let mut out = Dict::new();
    // hayro's `entries()` unwraps each value; a value that does not read (a broken hex string) is left out instead
    let mut keys: Vec<_> = d.keys().collect();
    keys.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
    for k in keys {
        if let Some(v) = d.get_raw::<Object<'_>>(k.as_ref()) {
            out.set(&latin1(k.as_ref()), conv_maybe(v, depth.saturating_add(1)));
        }
    }
    out
}

/// The trailer dictionary of the newest section: `startxref` → a classic table's `trailer << … >>` or a
/// cross-reference stream's dictionary; else the last `trailer` keyword in the file.
pub fn find_trailer(b: &[u8]) -> Option<Dict> {
    let tail_start = b.len().saturating_sub(4096);
    let tail = b.get(tail_start..)?;
    let at = tail.windows(9).rposition(|w| w == b"startxref").map(|i| tail_start.saturating_add(i));
    if let Some(at) = at {
        let mut lx = Lexer::new(b);
        lx.pos = at.saturating_add(9);
        lx.skip_ws_comments();
        if let Some(off) = lx.raw_int().and_then(|o| usize::try_from(o).ok()) {
            let mut l2 = Lexer::new(b);
            l2.pos = off;
            l2.skip_ws_comments();
            if l2.starts_with(b"xref") {
                if let Some(t) = b.get(off..).and_then(|r| r.windows(7).position(|w| w == b"trailer")) {
                    l2.pos = off.saturating_add(t).saturating_add(7);
                    l2.skip_ws_comments();
                    if let Some(d) = l2.dict(0) {
                        return Some(d);
                    }
                }
            } else {
                let _ = l2.raw_int();
                l2.skip_ws_comments();
                let _ = l2.raw_int();
                l2.skip_ws_comments();
                if l2.match_kw(b"obj") {
                    l2.skip_ws_comments();
                    if let Some(d) = l2.dict(0) {
                        return Some(d);
                    }
                }
            }
        }
    }
    let t = b.windows(7).rposition(|w| w == b"trailer")?;
    let mut lx = Lexer::new(b);
    lx.pos = t.saturating_add(7);
    lx.skip_ws_comments();
    lx.dict(0)
}

impl JsDoc {
    /// `pdfjs.getDocument({data})`.
    pub fn open(bytes: &[u8]) -> Result<JsDoc, OpenError> {
        let pdf = match Pdf::new(bytes.to_vec()) {
            Ok(p) => p,
            Err(hayro_syntax::LoadPdfError::Decryption(_)) => return Err(OpenError::Password),
            Err(hayro_syntax::LoadPdfError::Invalid) => return Err(OpenError::Read("Invalid PDF structure.".into())),
        };
        let mut pages = Vec::new();
        for p in pdf.pages().iter() {
            let raw = p.raw();
            let id = raw.obj_id().map_or((0, 0), |i| (u32::try_from(i.obj_number).unwrap_or(0), u16::try_from(i.gen_number).unwrap_or(0)));
            pages.push(JsPage { id, dict: conv_dict(raw, 0) });
        }
        if pages.is_empty() {
            return Err(OpenError::Read("Invalid PDF structure.".into()));
        }
        Ok(JsDoc { pdf, pages, trailer: find_trailer(bytes) })
    }

    /// The object behind a reference (owned copy); Null when there is none.
    pub fn get(&self, id: (u32, u16)) -> Obj {
        let oid = ObjectIdentifier::new(i32::try_from(id.0).unwrap_or(-1), i32::from(id.1));
        match self.pdf.xref().get::<Object<'_>>(oid) {
            Some(o) => conv(o, 0),
            None => Obj::Null,
        }
    }

    /// `xref.fetchIfRef(o)`.
    pub fn resolve(&self, o: &Obj) -> Obj {
        match o {
            Obj::Ref(n, g) => self.get((*n, *g)),
            x => x.clone(),
        }
    }

    /// `dict.get(key)` (references resolved, one level).
    pub fn dget(&self, d: &Dict, key: &str) -> Option<Obj> {
        d.get(key).map(|v| self.resolve(v))
    }

    /// `dict.getArray(key)`: the array with its references resolved.
    pub fn dget_array(&self, d: &Dict, key: &str) -> Option<Vec<Obj>> {
        match self.dget(d, key)? {
            Obj::Arr(a) => Some(a.iter().map(|x| self.resolve(x)).collect()),
            _ => None,
        }
    }

    /// The catalog.
    pub fn catalog(&self) -> Dict {
        let root = self.pdf.xref().root_id();
        match self.get((u32::try_from(root.obj_number).unwrap_or(0), u16::try_from(root.gen_number).unwrap_or(0))) {
            Obj::Dict(d) => d,
            _ => Dict::new(),
        }
    }

    /// A page attribute found on the page or the nearest ancestor (pdf.js getInheritableProperty, closest wins).
    pub fn inherited(&self, page: &Dict, key: &str) -> Option<Obj> {
        let mut d = page.clone();
        for _ in 0..64 {
            if let Some(v) = d.get(key) {
                return Some(self.resolve(v));
            }
            match self.dget(&d, "Parent") {
                Some(Obj::Dict(p)) => d = p,
                _ => return None,
            }
        }
        None
    }

    fn number_rect(&self, o: Option<Obj>) -> Option<[f64; 4]> {
        let a = match o? {
            Obj::Arr(a) => a,
            _ => return None,
        };
        if a.len() != 4 {
            return None;
        }
        let v: Vec<f64> = a.iter().map(|x| self.resolve(x)).filter_map(|x| x.as_num()).collect();
        let [x0, y0, x1, y1] = <[f64; 4]>::try_from(v).ok()?;
        Some(normalize_rect([x0, y0, x1, y1]))
    }

    fn bounding_box(&self, page: &Dict, key: &str) -> Option<[f64; 4]> {
        let b = self.number_rect(self.inherited(page, key))?;
        if b[2] - b[0] > 0.0 && b[3] - b[1] > 0.0 { Some(b) } else { None }
    }

    /// `page.view`: the crop box within the media box.
    pub fn view(&self, page: &Dict) -> [f64; 4] {
        let media = self.bounding_box(page, "MediaBox").unwrap_or([0.0, 0.0, 612.0, 792.0]);
        let crop = self.bounding_box(page, "CropBox");
        if let Some(c) = crop
            && c != media
            && let Some(b) = intersect(c, media)
            && b[2] - b[0] > 0.0
            && b[3] - b[1] > 0.0
        {
            return b;
        }
        media
    }

    /// `page.rotate`.
    pub fn rotate(&self, page: &Dict) -> f64 {
        let r = match self.inherited(page, "Rotate") {
            Some(Obj::Num(n)) if n != 0.0 && !n.is_nan() => n,
            _ => 0.0,
        };
        if r % 90.0 != 0.0 {
            0.0
        } else if r >= 360.0 {
            r % 360.0
        } else if r < 0.0 {
            ((r % 360.0) + 360.0) % 360.0
        } else {
            r
        }
    }

    /// `doc.getPageLabels()` (None when there are none or they cannot be read).
    pub fn page_labels(&self) -> Option<Vec<String>> {
        let cat = self.catalog();
        let root = cat.get("PageLabels")?.clone();
        let mut map: Vec<(f64, Obj)> = Vec::new();
        let mut processed: Vec<(u32, u16)> = Vec::new();
        if let Obj::Ref(n, g) = root {
            processed.push((n, g));
        }
        let mut queue = vec![root];
        let mut qi = 0usize;
        while let Some(node) = queue.get(qi).cloned() {
            qi = qi.saturating_add(1);
            if qi > 100_000 {
                break;
            }
            let Obj::Dict(d) = self.resolve(&node) else { continue };
            if d.has("Kids") {
                let Some(Obj::Arr(kids)) = self.dget(&d, "Kids") else { continue };
                for k in kids {
                    if let Obj::Ref(n, g) = k {
                        if processed.contains(&(n, g)) {
                            return None; // "Duplicate entry" FormatError → no labels
                        }
                        processed.push((n, g));
                    }
                    queue.push(k);
                }
                continue;
            }
            let Some(Obj::Arr(entries)) = self.dget(&d, "Nums") else { continue };
            let mut i = 0usize;
            while i < entries.len() {
                let key = entries.get(i).map(|x| self.resolve(x));
                let val = entries.get(i.saturating_add(1)).map(|x| self.resolve(x)).unwrap_or(Obj::Null);
                if let Some(Obj::Num(k)) = key {
                    let k = if k == 0.0 { 0.0 } else { k };
                    match map.iter_mut().find(|(x, _)| *x == k) {
                        Some(slot) => slot.1 = val,
                        None => map.push((k, val)),
                    }
                }
                i = i.saturating_add(2);
            }
        }
        let n = self.pages.len();
        let mut out = Vec::with_capacity(n);
        let mut style: Option<String> = None;
        let mut prefix = String::new();
        let mut current = 1.0f64;
        for i in 0..n {
            #[allow(clippy::cast_precision_loss)]
            let fi = i as f64;
            if let Some((_, ld)) = map.iter().find(|(k, _)| *k == fi) {
                let Obj::Dict(ld) = ld else { return None };
                if ld.has("Type") && self.dget(ld, "Type").as_ref().and_then(Obj::as_name) != Some("PageLabel") {
                    return None;
                }
                style = if ld.has("S") {
                    match self.dget(ld, "S") {
                        Some(Obj::Name(s)) => Some(s),
                        _ => return None,
                    }
                } else {
                    None
                };
                prefix = if ld.has("P") {
                    match self.dget(ld, "P") {
                        Some(p) if p.is_string() => string_to_pdf_string(&p.str_bytes().unwrap_or_default()),
                        _ => return None,
                    }
                } else {
                    String::new()
                };
                current = if ld.has("St") {
                    match self.dget(ld, "St") {
                        Some(Obj::Num(s)) if s.fract() == 0.0 && s >= 1.0 => s,
                        _ => return None,
                    }
                } else {
                    1.0
                };
            }
            let label = match style.as_deref() {
                Some("D") => cc_markup::js::ns(current),
                Some(s @ ("R" | "r")) => roman(current, s == "r"),
                Some(s @ ("A" | "a")) => {
                    let li = current - 1.0;
                    let base = u32::from(s.as_bytes().first().copied().unwrap_or(b'A'));
                    let code = base.saturating_add(cc_markup::js::f2u(li % 26.0).try_into().unwrap_or(0));
                    let ch = char::from_u32(code).unwrap_or('?');
                    ch.to_string().repeat(cc_markup::js::f2usize((li / 26.0).floor() + 1.0).min(10_000))
                }
                None => String::new(),
                Some(_) => return None,
            };
            out.push(format!("{prefix}{label}"));
            current += 1.0;
        }
        Some(out)
    }

    /// The Info dictionary's custom keys (pdf.js `info.Custom`), in file order: strings decoded, numbers and booleans as
    /// they are, names as the name.
    pub fn info_custom(&self) -> Vec<(String, CustomValue)> {
        let mut out = Vec::new();
        let Some(info) = self.trailer.as_ref().and_then(|t| t.get("Info")).map(|i| self.resolve(i)) else { return out };
        let Obj::Dict(d) = info else { return out };
        for (k, v) in &d.0 {
            let v = self.resolve(v);
            let standard =
                matches!(k.as_str(), "Title" | "Author" | "Subject" | "Keywords" | "Creator" | "Producer" | "CreationDate" | "ModDate");
            if standard && v.is_string() {
                continue;
            }
            if k == "Trapped" && matches!(v, Obj::Name(_)) {
                continue;
            }
            let cv = match &v {
                Obj::Num(n) => CustomValue::Num(*n),
                Obj::Bool(b) => CustomValue::Bool(*b),
                Obj::Name(n) => CustomValue::Name(n.clone()),
                x if x.is_string() => CustomValue::Str(string_to_pdf_string(&x.str_bytes().unwrap_or_default())),
                _ => continue,
            };
            out.push((k.clone(), cv));
        }
        out
    }

    /// A page's index from its reference (`doc.getPageIndex(ref)`).
    pub fn page_index(&self, id: (u32, u16)) -> Option<usize> {
        self.pages.iter().position(|p| p.id == id)
    }

    /// The page's content: its content streams' decoded bytes, joined with a newline.
    pub fn content(&self, page: &Dict) -> Vec<u8> {
        let mut out = Vec::new();
        let parts: Vec<Obj> = match page.get("Contents") {
            Some(c) => match self.resolve(c) {
                Obj::Arr(a) => a,
                x => vec![x],
            },
            None => Vec::new(),
        };
        for p in parts {
            if let Obj::Stream(s) = self.resolve(&p) {
                out.extend_from_slice(&s.data);
                out.push(b'\n');
            }
        }
        out
    }
}

/// A custom Info value.
#[derive(Debug, Clone, PartialEq)]
pub enum CustomValue {
    Str(String),
    Num(f64),
    Bool(bool),
    Name(String),
}

/// `Util.normalizeRect`.
pub fn normalize_rect(r: [f64; 4]) -> [f64; 4] {
    let mut o = r;
    if r[0] > r[2] {
        o[0] = r[2];
        o[2] = r[0];
    }
    if r[1] > r[3] {
        o[1] = r[3];
        o[3] = r[1];
    }
    o
}

/// `Util.intersect`.
pub fn intersect(a: [f64; 4], b: [f64; 4]) -> Option<[f64; 4]> {
    use cc_markup::js::{max, min};
    let xl = max(min(a[0], a[2]), min(b[0], b[2]));
    let xh = min(max(a[0], a[2]), max(b[0], b[2]));
    if xl > xh {
        return None;
    }
    let yl = max(min(a[1], a[3]), min(b[1], b[3]));
    let yh = min(max(a[1], a[3]), max(b[1], b[3]));
    if yl > yh {
        return None;
    }
    Some([xl, yl, xh, yh])
}

fn roman(n: f64, lower: bool) -> String {
    const MAP: [&str; 30] = [
        "", "C", "CC", "CCC", "CD", "D", "DC", "DCC", "DCCC", "CM", "", "X", "XX", "XXX", "XL", "L", "LX", "LXX", "LXXX", "XC", "", "I",
        "II", "III", "IV", "V", "VI", "VII", "VIII", "IX",
    ];
    let n = cc_markup::js::f2u(n);
    let th = usize::try_from((n / 1000).min(10_000)).unwrap_or(0);
    let h = usize::try_from((n % 1000) / 100).unwrap_or(0);
    let t = usize::try_from((n % 100) / 10).unwrap_or(0);
    let o = usize::try_from(n % 10).unwrap_or(0);
    let s = format!(
        "{}{}{}{}",
        "M".repeat(th),
        MAP.get(h).copied().unwrap_or(""),
        MAP.get(10usize.saturating_add(t)).copied().unwrap_or(""),
        MAP.get(20usize.saturating_add(o)).copied().unwrap_or("")
    );
    if lower { s.to_lowercase() } else { s }
}

/// pdf.js `PDFStringTranslateTable` for 0x18–0x1F and 0x80–0xA0.
fn translate(b: u8) -> Option<char> {
    const LOW: [u32; 8] = [0x2d8, 0x2c7, 0x2c6, 0x2d9, 0x2dd, 0x2db, 0x2da, 0x2dc];
    const HIGH: [u32; 33] = [
        0x2022, 0x2020, 0x2021, 0x2026, 0x2014, 0x2013, 0x192, 0x2044, 0x2039, 0x203a, 0x2212, 0x2030, 0x201e, 0x201c, 0x201d, 0x2018,
        0x2019, 0x201a, 0x2122, 0xfb01, 0xfb02, 0x141, 0x152, 0x160, 0x178, 0x17d, 0x131, 0x142, 0x153, 0x161, 0x17e, 0, 0x20ac,
    ];
    let code = match b {
        0x18..=0x1f => LOW.get(usize::from(b.wrapping_sub(0x18))).copied().unwrap_or(0),
        0x80..=0xa0 => HIGH.get(usize::from(b.wrapping_sub(0x80))).copied().unwrap_or(0),
        _ => 0,
    };
    if code == 0 { None } else { char::from_u32(code) }
}

fn strip_escapes(s: &str) -> String {
    // .replaceAll(/\x1b[^\x1b]*(?:\x1b|$)/g, "")
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\u{1b}' {
            for d in it.by_ref() {
                if d == '\u{1b}' {
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// pdf.js `stringToPDFString(bytes)`.
pub fn string_to_pdf_string(b: &[u8]) -> String {
    let first = b.first().copied().unwrap_or(0);
    if first >= 0xef {
        let decoded: Option<String> = if b.len() >= 2 && first == 0xfe && b.get(1) == Some(&0xff) {
            let body = b.get(2..b.len() & !1).unwrap_or(&[]);
            let units: Vec<u16> =
                body.chunks(2).map(|p| u16::from_be_bytes([p.first().copied().unwrap_or(0), p.get(1).copied().unwrap_or(0)])).collect();
            decode_utf16_fatal(&units)
        } else if b.len() >= 2 && first == 0xff && b.get(1) == Some(&0xfe) {
            let body = b.get(2..b.len() & !1).unwrap_or(&[]);
            let units: Vec<u16> =
                body.chunks(2).map(|p| u16::from_le_bytes([p.first().copied().unwrap_or(0), p.get(1).copied().unwrap_or(0)])).collect();
            decode_utf16_fatal(&units)
        } else if b.len() >= 3 && first == 0xef && b.get(1) == Some(&0xbb) && b.get(2) == Some(&0xbf) {
            std::str::from_utf8(b.get(3..).unwrap_or(&[])).ok().map(str::to_string)
        } else {
            None
        };
        if let Some(d) = decoded {
            return if d.contains('\u{1b}') { strip_escapes(&d) } else { d };
        }
    }
    let mut out = String::with_capacity(b.len());
    let mut i = 0usize;
    while let Some(&c) = b.get(i) {
        if c == 0x1b {
            i = i.saturating_add(1);
            while b.get(i).is_some_and(|x| *x != 0x1b) {
                i = i.saturating_add(1);
            }
            i = i.saturating_add(1);
            continue;
        }
        out.push(translate(c).unwrap_or(char::from(c)));
        i = i.saturating_add(1);
    }
    out
}

fn decode_utf16_fatal(units: &[u16]) -> Option<String> {
    let mut s = String::new();
    for r in char::decode_utf16(units.iter().copied()) {
        s.push(r.ok()?);
    }
    Some(s)
}

/// `utf16_text` re-export for callers that decode UTF-16 by hand.
pub fn utf16(units: &[u16]) -> String {
    utf16_text(units)
}
