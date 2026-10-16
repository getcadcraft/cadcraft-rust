//! step.js — ISO 10303-21 ("STEP physical file", the .ifc text format): an index built in one pass over the bytes, and a
//! parser for one instance's parameters, run only when that instance is read. Written from ISO 10303-21:2016 (clauses
//! 5–7: tokens, header section, data section, string encoding).
//!
//! `scan_step(bytes)` → [`StepIndex`] (`ids[i]`, `starts[i]` = the "(" after the keyword, `ends[i]` = the closing ";",
//! `types[i]` = index into `names`, "" for a complex instance); `parse_params(bytes, start, end)` → the parameter list;
//! `parse_complex(bytes, start, end)` → the parts of a complex instance "(A(..)B(..))".
//!
//! Positions are JS numbers in the original (`b[i]` past the end reads `undefined`): here they are `i64`s and a read
//! outside the buffer is −1, so every comparison the JS makes with `undefined` comes out the same.

use crate::util::{js_number, to_int32, trim16, upper16};
use crate::value::{JsStr, Value};

const C_HASH: i32 = 35;
const C_EQ: i32 = 61;
const C_SEMI: i32 = 59;
const C_LP: i32 = 40;
const C_RP: i32 = 41;
const C_COMMA: i32 = 44;
const C_QUOTE: i32 = 39;
const C_DQUOTE: i32 = 34;
const C_SLASH: i32 = 47;
const C_STAR: i32 = 42;
const C_DOLLAR: i32 = 36;
const C_DOT: i32 = 46;
const C_MINUS: i32 = 45;
const C_PLUS: i32 = 43;
const C_BSL: u8 = 92;

/// The deepest nesting of aggregates / typed values the parser follows (the JS engine runs out of stack somewhere
/// near this; here it is a fixed limit with the same error).
pub const MAX_NESTING: usize = 10_000;

/// A StepError's message ("… (at byte n)") or another error the JS would throw while parsing (RangeError texts).
#[derive(Clone, Debug, PartialEq)]
pub struct StepError(pub String);

impl StepError {
    fn at(msg: &str, offset: i64) -> StepError {
        StepError(format!("{msg} (at byte {offset})"))
    }
    fn plain(msg: &str) -> StepError {
        StepError(msg.to_string())
    }
}

impl std::fmt::Display for StepError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for StepError {}

type R<T> = Result<T, StepError>;

#[inline]
fn at(b: &[u8], i: i64) -> i32 {
    usize::try_from(i).ok().and_then(|k| b.get(k)).map_or(-1, |&c| i32::from(c))
}
#[inline]
fn inc(i: i64, n: i64) -> i64 {
    i.saturating_add(n)
}
#[inline]
fn is_space(c: i32) -> bool {
    c == 32 || c == 10 || c == 13 || c == 9 || c == 12 || c == 11
}
#[inline]
fn is_digit(c: i32) -> bool {
    (48..=57).contains(&c)
}
#[inline]
fn is_key_char(c: i32) -> bool {
    (65..=90).contains(&c) || (97..=122).contains(&c) || is_digit(c) || c == 95 || c == C_MINUS || c == 33
}
fn len_i64(b: &[u8]) -> i64 {
    i64::try_from(b.len()).unwrap_or(i64::MAX)
}

/// Skip blanks and comments from i; → the first other byte's offset.
fn skip(b: &[u8], mut i: i64, end: i64) -> i64 {
    loop {
        while i < end && is_space(at(b, i)) {
            i = inc(i, 1);
        }
        if inc(i, 1) < end && at(b, i) == C_SLASH && at(b, inc(i, 1)) == C_STAR {
            i = inc(i, 2);
            while inc(i, 1) < end && !(at(b, i) == C_STAR && at(b, inc(i, 1)) == C_SLASH) {
                i = inc(i, 1);
            }
            i = inc(i, 2);
            continue;
        }
        return i;
    }
}

const LO: u64 = 0x0101_0101_0101_0101;
const HI: u64 = 0x8080_8080_8080_8080;

/// The bytes of w equal to c, as high bits (the lowest set bit is exact; bits above it may be false positives).
#[inline]
fn eq_mask(w: u64, c: u8) -> u64 {
    let x = w ^ LO.wrapping_mul(u64::from(c));
    x.wrapping_sub(LO) & !x & HI
}

/// The first offset in s holding one of the bytes `cs` (eight bytes at a time).
#[inline]
fn find_any<const N: usize>(s: &[u8], cs: [u8; N]) -> Option<usize> {
    let (words, rest) = s.as_chunks::<8>();
    for (k, w) in words.iter().enumerate() {
        let w = u64::from_le_bytes(*w);
        let m = cs.iter().fold(0u64, |m, &c| m | eq_mask(w, c));
        if m != 0 {
            let byte = usize::try_from(m.trailing_zeros() / 8).unwrap_or(0);
            return Some(k.saturating_mul(8).saturating_add(byte));
        }
    }
    let base = words.len().saturating_mul(8);
    rest.iter().position(|c| cs.contains(c)).map(|p| base.saturating_add(p))
}

/// b[i..end) (clamped to the buffer).
#[inline]
fn tail(b: &[u8], i: i64, end: i64) -> &[u8] {
    match (usize::try_from(i), usize::try_from(end)) {
        (Ok(s), Ok(e)) => b.get(s..e.min(b.len())).unwrap_or(&[]),
        _ => &[],
    }
}

/// Offset just past a quoted string starting at i (b[i] is the quote).
fn past_string(b: &[u8], mut i: i64, end: i64) -> R<i64> {
    i = inc(i, 1);
    loop {
        i = match find_any(tail(b, i, end), *b"'") {
            Some(k) => inc(i, i64::try_from(k).unwrap_or(i64::MAX)),
            None => end.max(i),
        };
        if i >= end {
            return Err(StepError::at("A text value is not closed", i));
        }
        if inc(i, 1) < end && at(b, inc(i, 1)) == C_QUOTE {
            i = inc(i, 2);
            continue;
        }
        return Ok(inc(i, 1));
    }
}

/// Offset of the ";" ending the statement that continues at i (strings, binaries and comments skipped); −1 when none.
fn statement_end(b: &[u8], mut i: i64, end: i64) -> R<i64> {
    // the hot loop of the scan: jump straight to the next byte that matters
    while i < end {
        let rest = tail(b, i, end);
        if rest.is_empty() {
            break;
        }
        let Some(k) = find_any(rest, *b";'\"/") else {
            return Ok(-1);
        };
        i = inc(i, i64::try_from(k).unwrap_or(i64::MAX));
        let c = at(b, i);
        if c == C_SEMI {
            return Ok(i);
        }
        if c == C_QUOTE {
            i = past_string(b, i, end)?;
            continue;
        }
        if c == C_DQUOTE {
            i = inc(i, 1);
            while i < end && at(b, i) != C_DQUOTE {
                i = inc(i, 1);
            }
            i = inc(i, 1);
            continue;
        }
        if c == C_SLASH && inc(i, 1) < end && at(b, inc(i, 1)) == C_STAR {
            i = inc(i, 2);
            while inc(i, 1) < end && !(at(b, i) == C_STAR && at(b, inc(i, 1)) == C_SLASH) {
                i = inc(i, 1);
            }
            i = inc(i, 2);
            continue;
        }
        i = inc(i, 1);
    }
    Ok(-1)
}

/// The keyword at i (upper case; ASCII only) and the offset after it.
fn keyword_at(b: &[u8], i: i64, end: i64) -> (String, i64) {
    let mut j = i;
    while j < end && is_key_char(at(b, j)) {
        j = inc(j, 1);
    }
    let mut s = String::new();
    if let (Ok(a), Ok(z)) = (usize::try_from(i), usize::try_from(j))
        && let Some(sl) = b.get(a..z)
    {
        s.reserve(sl.len());
        for &c in sl {
            s.push(char::from(c.to_ascii_uppercase()));
        }
    }
    (s, j)
}

/// FNV-1a: the type-name table is hashed once per instance, with short keys.
#[derive(Default, Clone, Copy)]
pub struct Fnv(u64);
impl std::hash::Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        let mut h = if self.0 == 0 { 0xcbf2_9ce4_8422_2325 } else { self.0 };
        for &b in bytes {
            h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
        self.0 = h;
    }
}
pub type FnvMap<K, V> = std::collections::HashMap<K, V, std::hash::BuildHasherDefault<Fnv>>;

/// The keyword at i as raw bytes and the offset after it.
fn keyword_span(b: &[u8], i: i64, end: i64) -> (&[u8], i64) {
    let mut j = i;
    while j < end && is_key_char(at(b, j)) {
        j = inc(j, 1);
    }
    let sl = match (usize::try_from(i), usize::try_from(j)) {
        (Ok(a), Ok(z)) => b.get(a..z).unwrap_or(&[]),
        _ => &[],
    };
    (sl, j)
}

/// Find an ASCII token (upper case) at a statement boundary from `from`.
fn find_token(b: &[u8], token: &[u8], from: i64, end: i64) -> R<i64> {
    let t0 = token.first().map_or(-1, |&c| i32::from(c));
    let n = i64::try_from(token.len()).unwrap_or(0);
    let mut i = from;
    while i < end {
        i = skip(b, i, end);
        if i >= end {
            return Ok(-1);
        }
        if at(b, i) == t0 || at(b, i) == t0.saturating_add(32) {
            let mut ok = true;
            let mut k = 1i64;
            while k < n && ok {
                let c = at(b, inc(i, k));
                let want = usize::try_from(k).ok().and_then(|x| token.get(x)).map_or(-1, |&c| i32::from(c));
                ok = c == want || ((65..=90).contains(&want) && c == want.saturating_add(32));
                k = inc(k, 1);
            }
            let next = at(b, inc(i, n));
            if ok && !is_key_char(if next < 0 { 0 } else { next }) {
                return Ok(i);
            }
        }
        let e = statement_end(b, i, end)?;
        if e < 0 {
            return Ok(-1);
        }
        i = inc(e, 1);
    }
    Ok(-1)
}

/// The header section as step.js reads it.
#[derive(Clone, Debug, Default)]
pub struct Header {
    pub description: Option<Vec<Value>>,
    pub name: Option<Vec<Value>>,
    pub schema: Vec<JsStr>,
    /// keyword → parameters, in first-seen order (a repeated keyword replaces the value)
    pub raw: Vec<(String, Vec<Value>)>,
}

/// One damaged instance the scan skipped.
#[derive(Clone, Debug)]
pub struct ScanError {
    pub offset: i64,
    pub message: String,
}

/// The instance index of a STEP file.
#[derive(Clone, Debug, Default)]
pub struct StepIndex {
    pub count: usize,
    /// ids as an `Int32Array` holds them (wrapped)
    pub ids: Vec<i32>,
    pub starts: Vec<i64>,
    pub ends: Vec<i64>,
    /// index into `names` as a `Uint16Array` holds it (wrapped)
    pub types: Vec<u16>,
    pub names: Vec<Box<str>>,
    /// the largest id as read (a JS number, not wrapped)
    pub max_id: f64,
    pub header: Header,
    pub schema: JsStr,
    pub errors: Vec<ScanError>,
}

impl StepIndex {
    /// The keyword of record i ("" for a complex instance).
    pub fn type_name(&self, i: usize) -> &str {
        self.types.get(i).and_then(|&t| self.names.get(usize::from(t))).map_or("", |s| s)
    }
}

/// Index every instance of the DATA section(s). Fails only when the file is not Part 21 at all; a broken instance is
/// skipped and listed in `errors` (with its byte offset), as a damaged file still opens.
pub fn scan_step(b: &[u8]) -> R<StepIndex> {
    let end = len_i64(b);
    let mut i = skip(b, 0, end);
    if at(b, i) == 0xEF && at(b, inc(i, 1)) == 0xBB && at(b, inc(i, 2)) == 0xBF {
        i = skip(b, inc(i, 3), end); // a UTF-8 byte order mark
    }
    let (magic, _) = keyword_at(b, i, end);
    if magic != "ISO-10303-21" {
        return Err(StepError::plain("Not an IFC (STEP) file: it does not start with ISO-10303-21;"));
    }
    let mut header = Header::default();
    let hdr = find_token(b, b"HEADER", i, end)?;
    let data0 = find_token(b, b"DATA", i, end)?;
    if data0 < 0 {
        return Err(StepError::plain("This file has no DATA section."));
    }
    if hdr >= 0 && hdr < data0 {
        let s = statement_end(b, hdr, end)?;
        read_header(b, inc(s, 1), data0, &mut header)?;
    }

    // about one instance per 60 bytes; the vectors grow past that
    let cap = (b.len() / 60).clamp(1024, 1 << 24);
    let mut ix = StepIndex {
        ids: Vec::with_capacity(cap),
        starts: Vec::with_capacity(cap),
        ends: Vec::with_capacity(cap),
        types: Vec::with_capacity(cap),
        names: vec!["".into()],
        ..StepIndex::default()
    };
    let mut max_id = 0.0f64;

    // 1. statements in order (sequential: strings and comments decide where one ends); the instance lines' own
    // reading happens in step 2
    let mut items: Vec<Item> = Vec::with_capacity(cap);
    let mut back_jumps: std::collections::HashSet<i64> = std::collections::HashSet::new();
    i = inc(statement_end(b, data0, end)?, 1); // past "DATA;" (or "DATA(…);" in edition 3)
    while i < end {
        i = skip(b, i, end);
        if i >= end {
            break;
        }
        let c = at(b, i);
        if c != C_HASH {
            let (kw, next) = keyword_at(b, i, end);
            if kw == "ENDSEC" {
                // another DATA section may follow (edition 3); otherwise this is the end
                let more = find_token(b, b"DATA", next, end)?;
                if more < 0 {
                    break;
                }
                let at_endsec = i;
                i = inc(statement_end(b, more, end)?, 1);
                if i <= at_endsec {
                    // a "DATA" with no ";" sends the JS back to byte 0 (−1 + 1); the scan's only state is its position,
                    // so meeting this ENDSEC again means it never ends (the JS runs until its arrays overflow)
                    if !back_jumps.insert(at_endsec) {
                        return Err(StepError::plain("Invalid array length"));
                    }
                }
                continue;
            }
            let e = statement_end(b, i, end)?;
            items.push(Item::Err(i, "Unexpected text in the DATA section"));
            if e < 0 {
                break;
            }
            i = inc(e, 1);
            continue;
        }
        // the JS looks for the end after "#n " (blanks and comments end nowhere else, so from "#" is the same)
        let e = statement_end(b, i, end)?;
        if e < 0 {
            items.push(Item::Err(i, "An instance is not closed with ;"));
            break;
        }
        items.push(Item::Inst(i, e));
        i = inc(e, 1);
    }
    // 2. each instance's id, keyword and "(" (in parallel natively)
    let read = |it: &Item| -> Read<'_> {
        match *it {
            Item::Err(off, msg) => Read::Err(off, msg),
            Item::Inst(i, e) => read_instance(b, i, e, end),
        }
    };
    #[cfg(not(target_arch = "wasm32"))]
    let reads: Vec<Read<'_>> = {
        use rayon::prelude::*;
        items.par_iter().with_min_len(4096).map(read).collect()
    };
    #[cfg(target_arch = "wasm32")]
    let reads: Vec<Read<'_>> = items.iter().map(read).collect();
    drop(items);
    // 3. type names numbered in file order
    let mut by_hash: FnvMap<u64, usize> = FnvMap::default();
    let mut name_of = |raw: &[u8], h: u64, names: &mut Vec<Box<str>>| -> usize {
        if let Some(&t) = by_hash.get(&h)
            && names.get(t).is_some_and(|n| n.as_bytes().eq_ignore_ascii_case(raw))
        {
            return t;
        }
        // a new name (or a hash collision: then a plain search)
        if let Some(t) = names.iter().position(|n| !n.is_empty() && n.as_bytes().eq_ignore_ascii_case(raw)) {
            return t;
        }
        let t = names.len();
        names.push(String::from_utf8_lossy(&raw.to_ascii_uppercase()).into());
        by_hash.entry(h).or_insert(t);
        t
    };
    ix.ids.reserve(reads.len());
    for r in reads {
        match r {
            Read::Err(off, msg) => ix.errors.push(ScanError { offset: off, message: msg.into() }),
            Read::NoParen(off, raw, h) => {
                name_of(raw, h, &mut ix.names);
                ix.errors.push(ScanError {
                    offset: off,
                    message: format!("Expected ( after {}", String::from_utf8_lossy(&raw.to_ascii_uppercase())),
                });
            }
            Read::Ok { id, start, end: e, name } => {
                let t = match name {
                    Some((raw, h)) => name_of(raw, h, &mut ix.names),
                    None => 0,
                };
                ix.ids.push(to_int32(id));
                ix.starts.push(start);
                ix.ends.push(e);
                // a Uint16Array keeps the low 16 bits
                ix.types.push(u16::try_from(t & 0xFFFF).unwrap_or(0));
                if id > max_id {
                    max_id = id;
                }
            }
        }
    }
    ix.count = ix.ids.len();
    ix.max_id = max_id;
    ix.schema = header.schema.first().filter(|s| !s.is_empty()).cloned().unwrap_or(JsStr::Utf8("".into()));
    ix.header = header;
    Ok(ix)
}

/// A statement of the DATA section found by the sequential pass.
enum Item {
    Inst(i64, i64),
    Err(i64, &'static str),
}

/// What one instance line says about itself.
enum Read<'a> {
    Ok {
        id: f64,
        start: i64,
        end: i64,
        name: Option<(&'a [u8], u64)>,
    },
    /// the keyword was read (and is numbered) but no "(" follows
    NoParen(i64, &'a [u8], u64),
    Err(i64, &'static str),
}

/// FNV-1a of the upper-case name.
fn name_hash(raw: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &c in raw {
        h = (h ^ u64::from(c.to_ascii_uppercase())).wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// "#n=KEYWORD(" (or "#n=(" for a complex instance) of the statement b[i..=e].
fn read_instance(b: &[u8], i: i64, e: i64, end: i64) -> Read<'_> {
    let mut j = inc(i, 1);
    let mut id = 0.0f64;
    while j < end && is_digit(at(b, j)) {
        id = id * 10.0 + f64::from(at(b, j).saturating_sub(48));
        j = inc(j, 1);
    }
    j = skip(b, j, end);
    if j == inc(i, 1) || at(b, j) != C_EQ {
        return Read::Err(i, "Expected #n=");
    }
    j = skip(b, inc(j, 1), end);
    if at(b, j) == C_LP {
        return Read::Ok { id, start: j, end: e, name: None }; // complex instance (A(..)B(..))
    }
    let (raw, next) = keyword_span(b, j, end);
    if raw.is_empty() {
        return Read::Err(i, "Expected an entity name");
    }
    let h = name_hash(raw);
    let start = skip(b, next, end);
    if at(b, start) != C_LP {
        return Read::NoParen(i, raw, h);
    }
    Read::Ok { id, start, end: e, name: Some((raw, h)) }
}

fn read_header(b: &[u8], mut i: i64, end: i64, header: &mut Header) -> R<()> {
    while i < end {
        i = skip(b, i, end);
        if i >= end {
            break;
        }
        let (kw, next) = keyword_at(b, i, end);
        let e = statement_end(b, i, end)?;
        if e < 0 || kw == "ENDSEC" {
            break;
        }
        let p = skip(b, next, end);
        if at(b, p) == C_LP
            && let Ok(params) = parse_params(b, p, e)
        {
            if kw == "FILE_DESCRIPTION" {
                header.description = Some(params.clone());
            }
            if kw == "FILE_NAME" {
                header.name = Some(params.clone());
            }
            if kw == "FILE_SCHEMA" {
                header.schema = match params.first() {
                    Some(Value::List(l)) => l
                        .iter()
                        .filter_map(|v| if let Value::Str(s) = v { Some(JsStr::from_utf16(upper16(trim16(&s.units())))) } else { None })
                        .collect(),
                    _ => Vec::new(),
                };
            }
            if let Some(slot) = header.raw.iter_mut().find(|(k, _)| *k == kw) {
                slot.1 = params;
            } else {
                header.raw.push((kw, params));
            }
        }
        i = inc(e, 1);
    }
    Ok(())
}

/// The parameter list of one instance: b[start] is "(" and end the ";" offset (or any offset past the ")").
pub fn parse_params(b: &[u8], start: i64, end: i64) -> R<Vec<Value>> {
    let mut p = Parser { b, i: start, end };
    p.list()
}

/// One part of a complex instance.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub ty: String,
    pub params: Vec<Value>,
}

/// A complex instance "(A(…)B(…))" → its parts in file order.
pub fn parse_complex(b: &[u8], start: i64, end: i64) -> R<Vec<Part>> {
    let mut out = Vec::new();
    let mut i = skip(b, inc(start, 1), end);
    while i < end && at(b, i) != C_RP {
        let (kw, next) = keyword_at(b, i, end);
        let mut p = Parser { b, i: skip(b, next, end), end };
        let params = p.list()?;
        out.push(Part { ty: kw, params });
        i = skip(b, p.i, end);
    }
    Ok(out)
}

struct Parser<'a> {
    b: &'a [u8],
    i: i64,
    end: i64,
}

enum Step {
    /// just past "(": ")" closes, else a value
    AfterOpen,
    Value,
    AfterValue,
}

/// An aggregate being read: where its items start on the value stack (a typed value's inner list carries its name).
struct Frame {
    start: usize,
    typed: Option<Box<str>>,
}

thread_local! {
    /// The value stack every parse on this thread shares (aggregates are cut out of it in one allocation each).
    static STACK: std::cell::RefCell<Vec<Value>> = const { std::cell::RefCell::new(Vec::new()) };
}

impl Parser<'_> {
    fn fail(&self, msg: &str) -> StepError {
        StepError::at(msg, self.i)
    }

    /// The list at i (iterative: nesting is bounded by MAX_NESTING, not by the call stack).
    fn list(&mut self) -> R<Vec<Value>> {
        STACK.with(|cell| match cell.try_borrow_mut() {
            Ok(mut vals) => {
                vals.clear();
                let r = self.list_with(&mut vals);
                vals.clear();
                r
            }
            Err(_) => self.list_with(&mut Vec::new()),
        })
    }

    fn list_with(&mut self, vals: &mut Vec<Value>) -> R<Vec<Value>> {
        let b = self.b;
        self.i = skip(b, self.i, self.end);
        if at(b, self.i) != C_LP {
            return Err(self.fail("Expected ("));
        }
        self.i = inc(self.i, 1);
        let base = vals.len();
        let mut frames: Vec<Frame> = vec![Frame { start: base, typed: None }];
        let mut step = Step::AfterOpen;
        loop {
            match step {
                Step::AfterOpen => {
                    self.i = skip(b, self.i, self.end);
                    if at(b, self.i) == C_RP {
                        self.i = inc(self.i, 1);
                        if let Some(done) = close(&mut frames, vals) {
                            return Ok(done);
                        }
                        step = Step::AfterValue;
                    } else {
                        step = Step::Value;
                    }
                }
                Step::AfterValue => {
                    self.i = skip(b, self.i, self.end);
                    let c = at(b, self.i);
                    if c == C_COMMA {
                        self.i = inc(self.i, 1);
                        step = Step::Value;
                    } else if c == C_RP {
                        self.i = inc(self.i, 1);
                        if let Some(done) = close(&mut frames, vals) {
                            return Ok(done);
                        }
                        step = Step::AfterValue;
                    } else {
                        return Err(self.fail("Expected , or )"));
                    }
                }
                Step::Value => {
                    self.i = skip(b, self.i, self.end);
                    let c = at(b, self.i);
                    if c == C_LP {
                        if frames.len() >= MAX_NESTING {
                            return Err(StepError::plain("Maximum call stack size exceeded"));
                        }
                        frames.push(Frame { start: vals.len(), typed: None });
                        self.i = inc(self.i, 1);
                        step = Step::AfterOpen;
                        continue;
                    }
                    if c != C_DOLLAR
                        && c != C_STAR
                        && c != C_HASH
                        && c != C_QUOTE
                        && c != C_DOT
                        && c != C_DQUOTE
                        && !(is_digit(c) || c == C_MINUS || c == C_PLUS)
                        && is_key_char(c)
                    {
                        let (kw, next) = keyword_at(b, self.i, self.end);
                        self.i = skip(b, next, self.end);
                        if at(b, self.i) != C_LP {
                            return Err(self.fail(&format!("Expected ( after {kw}")));
                        }
                        if frames.len() >= MAX_NESTING {
                            return Err(StepError::plain("Maximum call stack size exceeded"));
                        }
                        frames.push(Frame { start: vals.len(), typed: Some(kw.into_boxed_str()) });
                        self.i = inc(self.i, 1);
                        step = Step::AfterOpen;
                        continue;
                    }
                    let v = self.scalar(c)?;
                    vals.push(v);
                    step = Step::AfterValue;
                }
            }
        }
    }
    /// A value that is not an aggregate or a typed value.
    fn scalar(&mut self, c: i32) -> R<Value> {
        let b = self.b;
        if c == C_DOLLAR {
            self.i = inc(self.i, 1);
            return Ok(Value::Null);
        }
        if c == C_STAR {
            self.i = inc(self.i, 1);
            return Ok(Value::Derived);
        }
        if c == C_HASH {
            let mut j = inc(self.i, 1);
            let mut n = 0.0f64;
            while is_digit(at(b, j)) {
                n = n * 10.0 + f64::from(at(b, j).saturating_sub(48));
                j = inc(j, 1);
            }
            if j == inc(self.i, 1) {
                return Err(self.fail("Expected a number after #"));
            }
            self.i = j;
            return Ok(Value::Ref(n));
        }
        if c == C_QUOTE {
            let s = inc(self.i, 1);
            let e = past_string(b, self.i, self.end)?;
            self.i = e;
            return Ok(Value::Str(decode_string(b, s, inc(e, -1))?));
        }
        if c == C_DOT {
            let mut j = inc(self.i, 1);
            while j < self.end && at(b, j) != C_DOT {
                j = inc(j, 1);
            }
            let raw: &[u8] = match (usize::try_from(inc(self.i, 1)), usize::try_from(j)) {
                (Ok(x), Ok(y)) => b.get(x..y.min(b.len())).unwrap_or(&[]),
                _ => &[],
            };
            self.i = inc(j, 1);
            if raw.is_ascii() {
                // ASCII: JS trim (tab … CR, space) and upper case need no UTF-16
                let ws = |c: &u8| matches!(c, 9..=13 | 32);
                let x = raw.iter().position(|c| !ws(c)).unwrap_or(raw.len());
                let y = raw.iter().rposition(|c| !ws(c)).map_or(x, |k| k.saturating_add(1));
                let up: String = raw.get(x..y).unwrap_or(&[]).iter().map(|&c| char::from(c.to_ascii_uppercase())).collect();
                return Ok(Value::Enum(up.into_boxed_str()));
            }
            let units: Vec<u16> = raw.iter().map(|&c| u16::from(c)).collect();
            return Ok(Value::Enum(String::from_utf16_lossy(&upper16(trim16(&units))).into_boxed_str()));
        }
        if c == C_DQUOTE {
            let mut j = inc(self.i, 1);
            let mut s = String::new();
            while j < self.end && at(b, j) != C_DQUOTE {
                s.push(char::from(u8::try_from(at(b, j)).unwrap_or(0)));
                j = inc(j, 1);
            }
            self.i = inc(j, 1);
            return Ok(Value::Bin(s.into_boxed_str()));
        }
        if is_digit(c) || c == C_MINUS || c == C_PLUS || c == C_DOT {
            return self.number();
        }
        // String.fromCharCode(undefined) is "\u0000"
        let ch = char::from(u8::try_from(c).unwrap_or(0));
        let mut q = String::new();
        crate::util::push_json_str(&mut q, &ch.to_string());
        Err(self.fail(&format!("Unexpected character {q}")))
    }

    fn number(&mut self) -> R<Value> {
        let b = self.b;
        let mut j = self.i;
        while j < self.end {
            let c = at(b, j);
            if is_digit(c) || c == C_MINUS || c == C_PLUS || c == C_DOT || c == 69 || c == 101 {
                j = inc(j, 1);
            } else {
                break;
            }
        }
        let tok: &[u8] = match (usize::try_from(self.i), usize::try_from(j)) {
            (Ok(x), Ok(y)) => b.get(x..y).unwrap_or(&[]),
            _ => &[],
        };
        self.i = j;
        // the token is ASCII (digits, signs, ".", "E")
        let s = std::str::from_utf8(tok).unwrap_or("");
        // "1." and "1.E5" are valid Part 21 reals; Number() takes both
        let n = if s.ends_with('.') {
            js_number(&format!("{s}0"))
        } else if let Some(k) = tok.windows(2).position(|w| w.first() == Some(&b'.') && matches!(w.get(1), Some(b'e' | b'E'))) {
            // only the first "." followed by an exponent mark (the JS regex has no g flag)
            js_number(&format!("{}.0{}", s.get(..k).unwrap_or(""), s.get(k.saturating_add(1)..).unwrap_or("")))
        } else {
            js_number(s)
        };
        if !n.is_finite() {
            return Err(self.fail(&format!("Bad number {s}")));
        }
        Ok(Value::Num(n))
    }
}

/// Close the top frame; → the finished top-level list when it was the outermost one.
fn close(frames: &mut Vec<Frame>, vals: &mut Vec<Value>) -> Option<Vec<Value>> {
    let f = frames.pop()?;
    let start = f.start.min(vals.len());
    if frames.is_empty() {
        return Some(vals.drain(start..).collect());
    }
    let v = match f.typed {
        Some(name) => {
            let inner = if vals.len().saturating_sub(start) == 1 {
                vals.pop().unwrap_or(Value::Null)
            } else {
                Value::List(vals.drain(start..).collect())
            };
            Value::Typed(Box::new((name, inner)))
        }
        None => Value::List(vals.drain(start..).collect()),
    };
    vals.push(v);
    None
}

/// The text of a string token b[s..e) (between the quotes): raw bytes → characters, then the Part 21 escapes.
pub fn decode_string(b: &[u8], s: i64, e: i64) -> R<JsStr> {
    let (Ok(su), Ok(eu)) = (usize::try_from(s), usize::try_from(e)) else { return Ok(JsStr::Utf8("".into())) };
    let bytes = b.get(su..eu.min(b.len())).unwrap_or(&[]);
    let mut ascii = true;
    let mut quote = false;
    let mut bsl = false;
    for &c in bytes {
        if c >= 128 {
            ascii = false;
        } else if c == b'\'' {
            quote = true;
        } else if c == C_BSL {
            bsl = true;
        }
    }
    if ascii && !quote && !bsl {
        // the common case: plain ASCII text
        return Ok(JsStr::Utf8(std::str::from_utf8(bytes).unwrap_or("").into()));
    }
    let mut t: String = if ascii {
        // ASCII: one byte per character
        std::str::from_utf8(bytes).unwrap_or("").to_string()
    } else {
        // Part 21 allows only 0x20–0x7E, but many exporters write UTF-8 (or Latin-1) as it is: take what decodes
        // (a TextDecoder drops a leading byte order mark)
        match std::str::from_utf8(bytes) {
            Ok(s) => s.strip_prefix('\u{FEFF}').unwrap_or(s).to_string(),
            Err(_) => bytes.iter().map(|&c| char::from(c)).collect(),
        }
    };
    if quote {
        t = t.replace("''", "'");
    }
    if bsl {
        let u: Vec<u16> = t.encode_utf16().collect();
        return Ok(JsStr::from_utf16(unescape_step(&u)?));
    }
    Ok(JsStr::Utf8(t.into_boxed_str()))
}

const BS: u16 = 92;

fn is_hex(c: u16) -> bool {
    matches!(c, 48..=57 | 65..=70 | 97..=102)
}
fn hex_val(s: &[u16]) -> u32 {
    s.iter().fold(0u32, |acc, &c| {
        let d = match c {
            48..=57 => u32::from(c).saturating_sub(48),
            65..=70 => u32::from(c).saturating_sub(55),
            97..=102 => u32::from(c).saturating_sub(87),
            _ => 0,
        };
        acc.wrapping_mul(16).wrapping_add(d)
    })
}

/// Find `pat` in `hay` from `from` (`String.prototype.indexOf`); −1 when absent.
fn index_of(hay: &[u16], pat: &[u16], from: usize) -> i64 {
    if pat.is_empty() || hay.len() < pat.len() {
        return -1;
    }
    let last = hay.len().saturating_sub(pat.len());
    let mut k = from;
    while k <= last {
        if hay.get(k..k.saturating_add(pat.len())) == Some(pat) {
            return i64::try_from(k).unwrap_or(-1);
        }
        k = k.saturating_add(1);
    }
    -1
}

/// \\ \S\c \P?\ \X\hh \X2\hhhh…\X0\ \X4\hhhhhhhh…\X0\ (ISO 10303-21 7.3.3). Unknown or broken directives stay as text.
pub fn unescape_step(t: &[u16]) -> R<Vec<u16>> {
    let n = t.len();
    let mut out: Vec<u16> = Vec::with_capacity(n);
    let mut upper_all: Option<Vec<u16>> = None;
    let mut i = 0usize;
    let g = |k: usize| t.get(k).copied();
    while i < n {
        let c = g(i).unwrap_or(0);
        if c != BS {
            out.push(c);
            i = i.saturating_add(1);
            continue;
        }
        let d = upper16(t.get(i..i.saturating_add(4).min(n)).unwrap_or(&[]));
        let starts = |p: &[u16]| d.len() >= p.len() && d.get(..p.len()) == Some(p);
        if g(i.saturating_add(1)) == Some(BS) {
            out.push(BS);
            i = i.saturating_add(2);
            continue;
        }
        if starts(&[BS, 83, BS]) && i.saturating_add(3) < n {
            out.push(g(i.saturating_add(3)).unwrap_or(0).wrapping_add(128));
            i = i.saturating_add(4);
            continue;
        }
        if starts(&[BS, 80]) && g(i.saturating_add(3)) == Some(BS) {
            i = i.saturating_add(4); // code page switch: Latin-1 assumed
            continue;
        }
        if starts(&[BS, 88, BS]) {
            let h = t.get(i.saturating_add(3)..i.saturating_add(5).min(n)).unwrap_or(&[]);
            if h.len() == 2 && h.iter().all(|&c| is_hex(c)) {
                out.push(u16::try_from(hex_val(h)).unwrap_or(0));
                i = i.saturating_add(5);
                continue;
            }
        }
        let x2 = d == [BS, 88, 50, BS];
        if x2 || d == [BS, 88, 52, BS] {
            let w = if x2 { 4usize } else { 8 };
            let up = upper_all.get_or_insert_with(|| upper16(t));
            let close = index_of(up, &[BS, 88, 48, BS], i.saturating_add(4));
            if close > 0 {
                let close_u = usize::try_from(close).unwrap_or(0);
                let a = i.saturating_add(4);
                let hex: &[u16] = if a < close_u { t.get(a..close_u.min(n)).unwrap_or(&[]) } else { &[] };
                if hex.len().is_multiple_of(w) && hex.iter().all(|&c| is_hex(c)) {
                    for chunk in hex.chunks(w) {
                        let cp = hex_val(chunk);
                        if w == 4 {
                            out.push(u16::try_from(cp).unwrap_or(0));
                        } else if cp > 0x10FFFF {
                            return Err(StepError(format!("Invalid code point {cp}")));
                        } else if let Some(ch) = char::from_u32(cp) {
                            let mut buf = [0u16; 2];
                            out.extend_from_slice(ch.encode_utf16(&mut buf));
                        } else {
                            // a surrogate code point: String.fromCodePoint gives it as a lone unit
                            out.push(u16::try_from(cp).unwrap_or(0));
                        }
                    }
                    i = close_u.saturating_add(4);
                    continue;
                }
            }
        }
        out.push(c);
        i = i.saturating_add(1);
    }
    Ok(out)
}

/// Every record's parse result, as the parity kit's ifc-step engine reads it.
pub enum Parsed {
    Params(Vec<Value>),
    Complex(Vec<Part>),
    Error(String),
}

/// Parse record i (complex or plain).
pub fn parse_record(b: &[u8], ix: &StepIndex, i: usize) -> Parsed {
    let (Some(&s), Some(&e)) = (ix.starts.get(i), ix.ends.get(i)) else { return Parsed::Error("no such record".into()) };
    if ix.type_name(i).is_empty() {
        match parse_complex(b, s, e) {
            Ok(p) => Parsed::Complex(p),
            Err(err) => Parsed::Error(err.0),
        }
    } else {
        match parse_params(b, s, e) {
            Ok(p) => Parsed::Params(p),
            Err(err) => Parsed::Error(err.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> R<Vec<Value>> {
        parse_params(s.as_bytes(), 0, i64::try_from(s.len()).unwrap())
    }
    fn st(s: &str) -> JsStr {
        JsStr::Utf8(s.into())
    }

    #[test]
    fn values() {
        let v = p("($,*,#12,'a''b',.T.,\"0AF\",IFCLABEL('x'),(1,2.5,-3.E2),1.)").unwrap();
        assert_eq!(v[0], Value::Null);
        assert_eq!(v[1], Value::Derived);
        assert_eq!(v[2], Value::Ref(12.0));
        assert_eq!(v[3], Value::Str(st("a'b")));
        assert_eq!(v[4], Value::Enum("T".into()));
        assert_eq!(v[5], Value::Bin("0AF".into()));
        assert_eq!(v[6], Value::Typed(Box::new(("IFCLABEL".into(), Value::Str(st("x"))))));
        assert_eq!(v[7], Value::List(vec![Value::Num(1.0), Value::Num(2.5), Value::Num(-300.0)].into()));
        assert_eq!(v[8], Value::Num(1.0));
    }

    #[test]
    fn errors_carry_offsets() {
        assert_eq!(p("(1 2)").unwrap_err().0, "Expected , or ) (at byte 3)");
        assert_eq!(p("(#)").unwrap_err().0, "Expected a number after # (at byte 1)");
        assert_eq!(p("(1.2.3)").unwrap_err().0, "Bad number 1.2.3 (at byte 6)");
        assert_eq!(p("(=)").unwrap_err().0, "Unexpected character \"=\" (at byte 1)");
        assert_eq!(p("('abc").unwrap_err().0, "A text value is not closed (at byte 5)");
    }

    #[test]
    fn escapes() {
        let s = |x: &str| decode_string(x.as_bytes(), 0, i64::try_from(x.len()).unwrap()).unwrap();
        assert_eq!(s("\\X2\\00E9\\X0\\t\\X\\E9\\\\"), st("\u{e9}t\u{e9}\\"));
        assert_eq!(s("\\S\\i"), st("\u{e9}"));
        assert_eq!(s("\\X4\\0001F600\\X0\\"), st("\u{1F600}"));
        assert_eq!(s("\\X2\\D800\\X0\\"), JsStr::Utf16(vec![0xD800].into()));
        assert_eq!(decode_string(b"\\X4\\12345678\\X0\\", 0, 17).unwrap_err().0, "Invalid code point 305419896");
        assert_eq!(s("\\PA\\B"), st("B"));
        assert_eq!(s("\\Q"), st("\\Q"));
    }

    #[test]
    fn scan() {
        let f = b"ISO-10303-21;\nHEADER;FILE_SCHEMA(('ifc4 '));ENDSEC;\nDATA;\n#1=IFCWALL($);\n#2=(A(1)B(2));\n#3 IFCX();\nENDSEC;\nEND-ISO-10303-21;\n";
        let ix = scan_step(f).unwrap();
        assert_eq!(ix.count, 2);
        assert_eq!(ix.schema, st("IFC4"));
        assert_eq!(ix.type_name(0), "IFCWALL");
        assert_eq!(ix.type_name(1), "");
        assert_eq!(ix.errors.len(), 1);
        assert_eq!(ix.errors[0].message, "Expected #n=");
        let parts = parse_complex(f, ix.starts[1], ix.ends[1]).unwrap();
        assert_eq!(parts[1].ty, "B");
        assert!(scan_step(b"HELLO").is_err());
    }

    #[test]
    fn a_data_section_that_loops_is_refused() {
        // fuzz find (ifc_read, timeout): ENDSEC, then a last "DATA" without ";" — the JS restarts at byte 0 forever
        let f = b"ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n#1=IFCX(1);\nENDSEC;\nDATA\n";
        assert_eq!(scan_step(f).unwrap_err().0, "Invalid array length");
    }

    #[test]
    fn deep_nesting_is_bounded() {
        let s = format!("{}{}", "(".repeat(MAX_NESTING + 5), ")".repeat(MAX_NESTING + 5));
        assert_eq!(p(&s).unwrap_err().0, "Maximum call stack size exceeded");
    }
}
