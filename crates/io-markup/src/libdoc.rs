//! A whole PDF read the way pdf-lib reads it (MarkupCraft's writer and the "extras" reader use pdf-lib): every
//! `N G obj … endobj` in file order (a later copy of an object replaces an earlier one), object streams unpacked,
//! cross-reference streams and tables skipped, the trailers' Root / Info / Encrypt / ID kept (later wins), and the
//! catalog recovered by scanning when Root is missing. Saving writes every object once, in object-number order, with a
//! classic cross-reference table — `save({useObjectStreams: false})`.

use crate::obj::{Dict, Lexer, Obj, Stream, write_obj};
use std::collections::BTreeMap;

pub type Ref = (u32, u16);

/// Most objects one file may hold (a hostile file cannot make the reader allocate without end).
pub const MAX_OBJECTS: usize = 4_000_000;
/// Most bytes an inflated stream may grow to.
pub const MAX_INFLATE: usize = 256 << 20;

/// A parsed document.
#[derive(Debug, Clone, Default)]
pub struct LibDoc {
    pub header: (u64, u64),
    pub objects: BTreeMap<Ref, Obj>,
    pub largest: u32,
    pub root: Option<Obj>,
    pub info: Option<Obj>,
    pub encrypt: Option<Obj>,
    pub id: Option<Obj>,
    /// images embedded during a save: their reference (taken at embed time) and their bytes, written at save time
    pub pending_images: Vec<(Ref, cc_markup::imagestamp::ImageInfo)>,
}

/// Inflate zlib data (FlateDecode), capped.
pub fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(data, MAX_INFLATE)
        .ok()
        .or_else(|| miniz_oxide::inflate::decompress_to_vec_with_limit(data.get(2..).unwrap_or(&[]), MAX_INFLATE).ok())
}

/// Deflate (zlib wrapper), as pdf-lib's `flateStream` writes.
pub fn deflate(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
}

/// The filters of a stream dictionary, as names.
pub fn filters(d: &Dict) -> Vec<String> {
    match d.get("Filter") {
        Some(Obj::Name(n)) => vec![n.clone()],
        Some(Obj::Arr(a)) => a.iter().filter_map(|x| x.as_name().map(str::to_string)).collect(),
        _ => Vec::new(),
    }
}

/// A stream's decoded bytes when its filters are ones this reader knows (Flate; none). pdf-lib also knows LZW,
/// ASCIIHex, ASCII85 and RunLength; the writer only needs to decode object streams, which are Flate in practice.
pub fn decode_stream(s: &Stream) -> Option<Vec<u8>> {
    let mut data = s.data.clone();
    for f in filters(&s.dict) {
        data = match f.as_str() {
            "FlateDecode" | "Fl" => inflate(&data)?,
            _ => return None,
        };
    }
    Some(data)
}

impl LibDoc {
    /// Parse the bytes. Err when there is no PDF header (pdf-lib's MissingPDFHeaderError) or no catalog at all.
    pub fn parse(bytes: &[u8]) -> Result<LibDoc, String> {
        let mut lx = Lexer::new(bytes);
        let mut doc = LibDoc::default();
        // header
        loop {
            if lx.done() {
                return Err("No PDF header found".into());
            }
            if lx.match_kw(b"%PDF-") {
                let major = lx.raw_int().unwrap_or(1);
                let _ = lx.match_kw(b".");
                let minor = lx.raw_int().unwrap_or(7);
                doc.header = (major, minor);
                break;
            }
            lx.pos = lx.pos.saturating_add(1);
        }
        let mut guard = 0usize;
        while !lx.done() {
            guard = guard.saturating_add(1);
            if guard > MAX_OBJECTS.saturating_mul(2) || doc.objects.len() > MAX_OBJECTS {
                break;
            }
            let before = lx.pos;
            lx.skip_ws_comments();
            while lx.peek().is_some_and(|c| c.is_ascii_digit()) {
                let start = lx.pos;
                if !doc.indirect_object(&mut lx) {
                    lx.pos = start;
                    if !doc.invalid_object(&mut lx) {
                        return Err("Failed to parse invalid PDF object".into());
                    }
                }
                lx.skip_ws_comments();
                skip_jibberish(&mut lx);
                if doc.objects.len() > MAX_OBJECTS {
                    break;
                }
            }
            // xref section
            lx.skip_ws_comments();
            if lx.match_kw(b"xref") {
                lx.skip_ws_comments();
                while lx.peek().is_some_and(|c| c.is_ascii_digit()) {
                    let _ = lx.raw_int();
                    lx.skip_ws_comments();
                    let _ = lx.raw_int();
                    lx.skip_ws_comments();
                    if matches!(lx.peek(), Some(b'n' | b'f')) {
                        lx.pos = lx.pos.saturating_add(1);
                    }
                    lx.skip_ws_comments();
                }
            }
            lx.skip_ws_comments();
            if lx.match_kw(b"trailer") {
                lx.skip_ws_comments();
                if let Some(d) = lx.dict(0) {
                    doc.take_trailer(&d);
                }
            }
            lx.skip_ws_comments();
            if lx.match_kw(b"startxref") {
                lx.skip_ws_comments();
                let _ = lx.raw_int();
                lx.skip_ws();
                let _ = lx.match_kw(b"%%EOF");
                lx.skip_ws_comments();
                let _ = lx.match_kw(b"%%EOF");
                lx.skip_ws_comments();
            }
            skip_jibberish(&mut lx);
            if lx.pos == before {
                // pdf-lib throws a StalledParserError here; move on by one byte instead (nothing else can read it)
                lx.pos = lx.pos.saturating_add(1);
            }
        }
        // recover the catalog
        let is_catalog = |o: Option<&Obj>| o.and_then(Obj::as_dict).and_then(|d| d.get("Type")).and_then(Obj::as_name) == Some("Catalog");
        if !is_catalog(doc.root.as_ref().map(|r| doc.lookup(r))) {
            let found: Vec<Ref> = doc.objects.iter().filter(|(_, o)| is_catalog(Some(o))).map(|(r, _)| *r).collect();
            if let Some(r) = found.last() {
                doc.root = Some(Obj::Ref(r.0, r.1));
            }
        }
        doc.objects.remove(&(0, 0));
        Ok(doc)
    }

    fn take_trailer(&mut self, d: &Dict) {
        let take = |k: &str, cur: &mut Option<Obj>| {
            if let Some(v) = d.get(k) {
                *cur = Some(v.clone());
            }
        };
        take("Root", &mut self.root);
        take("Encrypt", &mut self.encrypt);
        take("Info", &mut self.info);
        take("ID", &mut self.id);
    }

    fn assign(&mut self, r: Ref, o: Obj) {
        if r.0 > self.largest {
            self.largest = r.0;
        }
        self.objects.insert(r, o);
    }

    /// `N G obj <object> [stream…] endobj`; false when it does not parse (the caller retries it as invalid).
    fn indirect_object(&mut self, lx: &mut Lexer<'_>) -> bool {
        let Some(n) = lx.raw_int() else { return false };
        lx.skip_ws_comments();
        let Some(g) = lx.raw_int() else { return false };
        lx.skip_ws_comments();
        if !lx.match_kw(b"obj") {
            return false;
        }
        lx.skip_ws_comments();
        let Some(mut obj) = lx.object(0) else { return false };
        if let Obj::Dict(d) = &obj {
            let save = lx.pos;
            lx.skip_ws_comments();
            let has_stream = lx.match_kw(b"stream\r\n") || lx.match_kw(b"stream\n") || lx.match_kw(b"stream\r") || lx.match_kw(b"stream");
            if has_stream {
                let start = lx.pos;
                let mut end = None;
                if let Some(len) = d.get("Length").and_then(Obj::as_num) {
                    let e = start.saturating_add(cc_markup::js::f2usize(len));
                    let mut l2 = Lexer::new(lx.b);
                    l2.pos = e;
                    l2.skip_ws_comments();
                    if e <= lx.b.len() && l2.starts_with(b"endstream") {
                        end = Some(e);
                        lx.pos = l2.pos;
                    }
                }
                let end = match end {
                    Some(e) => e,
                    None => {
                        lx.pos = start;
                        match find_end_of_stream(lx) {
                            Some(e) => e,
                            None => return false,
                        }
                    }
                };
                let data = lx.b.get(start..end).unwrap_or(&[]).to_vec();
                obj = Obj::Stream(Box::new(Stream { dict: d.clone(), data }));
                lx.skip_ws_comments();
                let _ = lx.match_kw(b"endstream");
            } else {
                lx.pos = save;
            }
        }
        lx.skip_ws_comments();
        let _ = lx.match_kw(b"endobj");
        let r = (u32::try_from(n).unwrap_or(u32::MAX), u16::try_from(g).unwrap_or(u16::MAX));
        let ty = obj.as_dict().and_then(|d| d.get("Type")).and_then(Obj::as_name).map(str::to_string);
        match (&obj, ty.as_deref()) {
            (Obj::Stream(s), Some("ObjStm")) => self.object_stream(s),
            (Obj::Stream(s), Some("XRef")) => {
                let d = s.dict.clone();
                self.take_trailer(&d);
            }
            _ => self.assign(r, obj),
        }
        true
    }

    /// pdf-lib's tryToParseInvalidIndirectObject: the header, then everything up to `endobj` (kept as null here).
    fn invalid_object(&mut self, lx: &mut Lexer<'_>) -> bool {
        let n = lx.raw_int();
        lx.skip_ws_comments();
        let g = lx.raw_int();
        lx.skip_ws_comments();
        let ok = lx.match_kw(b"obj");
        if let (Some(n), Some(g), true) = (n, g, ok) {
            while !lx.done() {
                if lx.match_kw(b"endobj") {
                    self.assign((u32::try_from(n).unwrap_or(u32::MAX), u16::try_from(g).unwrap_or(u16::MAX)), Obj::Null);
                    return true;
                }
                lx.pos = lx.pos.saturating_add(1);
            }
        }
        // pdf-lib throws (MissingKeywordError / PDFInvalidObjectParsingError): the whole parse fails
        false
    }

    fn object_stream(&mut self, s: &Stream) {
        let Some(data) = decode_stream(s) else { return };
        let n = s.dict.get("N").and_then(Obj::as_num).map_or(0, cc_markup::js::f2usize);
        let first = s.dict.get("First").and_then(Obj::as_num).map_or(0, cc_markup::js::f2usize);
        let mut lx = Lexer::new(&data);
        let mut pairs = Vec::new();
        for _ in 0..n.min(MAX_OBJECTS) {
            lx.skip_ws_comments();
            let (Some(num), _) = (lx.raw_int(), ()) else { break };
            lx.skip_ws_comments();
            let Some(off) = lx.raw_int() else { break };
            pairs.push((num, off));
        }
        for (num, off) in pairs {
            let mut ol = Lexer::new(&data);
            ol.pos = first.saturating_add(usize::try_from(off).unwrap_or(usize::MAX));
            if let Some(o) = ol.object(0) {
                self.assign((u32::try_from(num).unwrap_or(u32::MAX), 0), o);
            }
        }
    }

    /// `context.lookup(v)`: a reference resolved (once); anything else as it is.
    pub fn lookup<'a>(&'a self, o: &'a Obj) -> &'a Obj {
        match o {
            Obj::Ref(n, g) => self.objects.get(&(*n, *g)).unwrap_or(&Obj::Null),
            x => x,
        }
    }
    pub fn get(&self, r: Ref) -> Option<&Obj> {
        self.objects.get(&r)
    }
    pub fn get_mut(&mut self, r: Ref) -> Option<&mut Obj> {
        self.objects.get_mut(&r)
    }

    /// `context.register(obj)` → its new reference.
    pub fn register(&mut self, o: Obj) -> Ref {
        let r = self.next_ref();
        self.objects.insert(r, o);
        r
    }
    /// `context.nextRef()`.
    pub fn next_ref(&mut self) -> Ref {
        self.largest = self.largest.saturating_add(1);
        (self.largest, 0)
    }
    /// `context.assign(ref, obj)`.
    pub fn put(&mut self, r: Ref, o: Obj) {
        self.assign(r, o);
    }

    /// Is the file encrypted (pdf-lib refuses to save it)?
    pub fn is_encrypted(&self) -> bool {
        self.encrypt.as_ref().is_some_and(|e| !matches!(self.lookup(e), Obj::Null))
    }

    /// The catalog's reference.
    pub fn root_ref(&self) -> Option<Ref> {
        self.root.as_ref().and_then(Obj::as_ref)
    }

    /// The page leaves in order (pdf-lib's page tree traversal: Kids depth first; /Type /Pages nodes are walked,
    /// /Type /Page leaves kept).
    pub fn pages(&self) -> Vec<Ref> {
        let mut out = Vec::new();
        let Some(root) = self.root.as_ref().map(|r| self.lookup(r)) else { return out };
        let Some(pages) = root.as_dict().and_then(|d| d.get("Pages")) else { return out };
        let mut seen = std::collections::HashSet::new();
        self.walk(pages, &mut out, &mut seen, 0);
        out
    }
    fn walk(&self, node: &Obj, out: &mut Vec<Ref>, seen: &mut std::collections::HashSet<Ref>, depth: usize) {
        if depth > 64 {
            return;
        }
        let Some(r) = node.as_ref() else { return };
        if !seen.insert(r) {
            return;
        }
        let Some(d) = self.lookup(node).as_dict() else { return };
        match d.get("Type").and_then(Obj::as_name) {
            Some("Pages") => {
                if let Some(kids) = d.get("Kids").map(|k| self.lookup(k)).and_then(Obj::as_arr) {
                    for k in kids {
                        self.walk(k, out, seen, depth.saturating_add(1));
                    }
                }
            }
            Some("Page") => out.push(r),
            _ => {}
        }
    }

    /// An inheritable page attribute (`MediaBox`, `Rotate`, `Resources`, …) through /Parent.
    pub fn inherited<'a>(&'a self, page: &'a Dict, key: &str) -> Option<&'a Obj> {
        let mut d = page;
        for _ in 0..64 {
            if let Some(v) = d.get(key) {
                return Some(self.lookup(v));
            }
            d = self.lookup(d.get("Parent")?).as_dict()?;
        }
        None
    }

    /// Write the document (every object, object-number order, classic xref, trailer with Size / Root / Encrypt /
    /// Info / ID).
    pub fn save(&self) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(format!("%PDF-{}.{}\n%\u{81}\u{81}\u{81}\u{81}\n\n", self.header.0, self.header.1).as_bytes());
        let mut offsets: Vec<(Ref, usize)> = Vec::with_capacity(self.objects.len());
        for (r, o) in &self.objects {
            offsets.push((*r, out.len()));
            out.extend_from_slice(format!("{} {} obj\n", r.0, r.1).as_bytes());
            write_obj(&mut out, o, 0);
            out.extend_from_slice(b"\nendobj\n\n");
        }
        let xref_at = out.len();
        let size = self.largest.saturating_add(1);
        out.extend_from_slice(b"xref\n");
        // one subsection per run of consecutive numbers, plus object 0
        out.extend_from_slice(b"0 1\n0000000000 65535 f \n");
        let mut i = 0usize;
        while i < offsets.len() {
            let mut j = i;
            while let (Some(a), Some(b)) = (offsets.get(j), offsets.get(j.saturating_add(1))) {
                if b.0.0 == a.0.0.saturating_add(1) {
                    j = j.saturating_add(1);
                } else {
                    break;
                }
            }
            if let Some(first) = offsets.get(i) {
                out.extend_from_slice(format!("{} {}\n", first.0.0, j.saturating_sub(i).saturating_add(1)).as_bytes());
            }
            for (r, off) in offsets.get(i..=j).unwrap_or(&[]) {
                out.extend_from_slice(format!("{off:010} {:05} n \n", r.1).as_bytes());
            }
            i = j.saturating_add(1);
        }
        let mut tr = Dict::new().with("Size", Obj::Num(f64::from(size)));
        if let Some(r) = &self.root {
            tr.set("Root", r.clone());
        }
        if let Some(e) = &self.encrypt {
            tr.set("Encrypt", e.clone());
        }
        if let Some(i) = &self.info {
            tr.set("Info", i.clone());
        }
        if let Some(id) = &self.id {
            tr.set("ID", id.clone());
        }
        out.extend_from_slice(b"trailer\n");
        write_obj(&mut out, &Obj::Dict(tr), 0);
        out.extend_from_slice(format!("\nstartxref\n{xref_at}\n%%EOF\n").as_bytes());
        out
    }
}

fn find_end_of_stream(lx: &mut Lexer<'_>) -> Option<usize> {
    let mut lvl = 1i32;
    let mut end = lx.pos;
    while !lx.done() {
        end = lx.pos;
        if lx.match_kw(b"stream") {
            lvl = lvl.saturating_add(1);
        } else if lx.match_kw(b"\r\nendstream") || lx.match_kw(b"\rendstream") || lx.match_kw(b"\nendstream") || lx.match_kw(b"endstream") {
            lvl = lvl.saturating_sub(1);
        } else {
            lx.pos = lx.pos.saturating_add(1);
        }
        if lvl == 0 {
            break;
        }
    }
    if lvl != 0 { None } else { Some(end) }
}

fn skip_jibberish(lx: &mut Lexer<'_>) {
    lx.skip_ws_comments();
    while !lx.done() {
        let at = lx.pos;
        let c = lx.peek().unwrap_or(0);
        if (b' '..=b'~').contains(&c) {
            if lx.starts_with(b"xref") || lx.starts_with(b"trailer") || lx.starts_with(b"startxref") {
                break;
            }
            if c.is_ascii_digit() {
                let n = lx.raw_int();
                let run_end = lx.pos;
                lx.skip_ws_comments();
                let g = lx.raw_int();
                lx.skip_ws_comments();
                let ok = n.is_some() && g.is_some() && lx.starts_with(b"obj");
                lx.pos = at;
                if ok {
                    break;
                }
                // a later start inside the same run of digits reads the same way: step over the run
                lx.pos = run_end;
                continue;
            }
        }
        lx.pos = at.saturating_add(1);
    }
}
