//! SketchUp 2021+ files ("VFF": the SketchUp header, then a ZIP archive), port of `lib/skp/vff.js`.
//! Format knowledge from OpenSKP (MIT, see NOTICE), checked against the corpus by the JS authors; the code is ours.
//!
//! What is in the archive:
//!   model.dat                    the model as nested records (below)
//!   materials/<folder>/material.xml (+ the texture image beside it)   one per material, and one "Layer_<name>" per tag
//!   meta/meta.dat                small records; 0x6400 > 0x6D00 = the display unit ("Millimeter")
//!   meta/model_thumbnail.png     the preview (meta/preview_thumbnail.png = the same with axes)
//!
//! model.dat records: tag (2 bytes, written here big-endian so 0xC409 reads like the bytes "C4 09"), length (u32 LE),
//! payload. We only descend through known containers along known paths, so a crafted file cannot make us recurse:
//!   F401 (whole file) > F701 > D430 > D530 > C832*      materials: DC05 > DE05 id, CC32 name
//!                     > F801 > 983A > 993A > 8C3C*      tags: DC05 id, 8D3C name, 8E3C hidden (1), 8F3C > C832 > CC32 "Layer_<name>"
//!                     > F901 > 7017 > 7117 > 7C15*      definitions: 8813 entities, 7D15 guid, 7E15 name, 8315 kind (0 component, 1 group, 2 image)
//!                     > F601 > 8813                     the model's own entities
//!                     > 0702 > 6D60 > 6D61 > 7148*      scenes (counted only)
//! An entity list (8813):
//!   D007 > DC05 > DE05 = the definition's id (what instances point at)
//!   8913 > C409*  vertex: DC05 > DE05 id, C509 = 3 × f64
//!   8A13 > B80B*  edge: D007 {DC05 > DE05 id, D107 material, D207 tag, D307 flags}, B90B / BA0B vertex ids, BB0B curve
//!   8B13 > AC0D*  face: D007 {id, D107 front material, D207 tag, D307 flags, DC05 > DD05 … texture placement},
//!                 AD0D normal (3 × f64 + plane distance), AE0D > 9411* loops > 9511 > A00F* coedges {A10F edge id, A20F 0 = along the edge, 1 = against},
//!                 AF0D back material
//!   8C13 > 6419*            component instances
//!   8D13 > 4C1D > 6419      groups
//!   9013 > 401F > 6419      images
//!   instance 6419: D007 {DC05 > DE05 id, D107 material, D207 tag, D307 flags}, 6519 name, 6619 placement (13 × f64),
//!                  6719 definition id, 6819 guid
//!   9713/9613 curves (arcs: their segments are ordinary edges) · 9113 guide lines · 9213 guide points · 9913 dimensions and text
//!   8E13 / 9B13 / 9E13 / 9F13 / A013 bookkeeping (A013 = bounding box, 6 × f64)
//! Ids are little-endian unsigned integers of the record's length (1–6 bytes).
//! D307 display flags: base 0x06; +0x01 hidden, +0x08 soft, +0x10 smooth.
//!
//! Placement (6619): r00 r01 r02  r10 r11 r12  r20 r21 r22  tx ty tz  w — the first nine are the rotation/scale ROWS
//! (p' = R·p + t), w is the homogeneous divisor (SketchUp writes 0.5 for "scale × 2"). We return column-major 4×4.
//!
//! Speed (native): the definitions' entity lists are read in parallel (rayon) and merged in file order, with the
//! vertex cap, the first error and the notes applied exactly as the JS's sequential walk applies them; texture images
//! are unpacked in parallel after their size budget has been checked in file order.

use crate::messages::msg;
use crate::model::{Def, Edge, Face, Image, Inst, Layer, Material, P3, Texture};
use crate::read::{Body, Header};
use crate::util::{
    FxMap, Limits, Res, SkpError, damaged, err, hypot, js_trim, ok_num, parse_float, parse_int10, thousands, utf8_lossy, utf8_or_latin1,
};
use crate::zip::open_zip;

// Record tags (bytes as written in the file).
const FILE: u16 = 0xf401;
const MATS: u16 = 0xf701;
const MATS2: u16 = 0xd430;
const MATS3: u16 = 0xd530;
const MAT: u16 = 0xc832;
const MAT_NAME: u16 = 0xcc32;
const LAYERS: u16 = 0xf801;
const LAYERS2: u16 = 0x983a;
const LAYERS3: u16 = 0x993a;
const LAYER: u16 = 0x8c3c;
const LAYER_NAME: u16 = 0x8d3c;
const LAYER_HIDDEN: u16 = 0x8e3c;
const LAYER_MAT: u16 = 0x8f3c;
const DEFS: u16 = 0xf901;
const DEFS2: u16 = 0x7017;
const DEFS3: u16 = 0x7117;
const DEF: u16 = 0x7c15;
const DEF_GUID: u16 = 0x7d15;
const DEF_NAME: u16 = 0x7e15;
const DEF_KIND: u16 = 0x8315;
const ROOT: u16 = 0xf601;
const ENTS: u16 = 0x8813;
const SCENES: u16 = 0x0702;
const SCENES2: u16 = 0x6d60;
const SCENES3: u16 = 0x6d61;
const SCENE: u16 = 0x7148;
const INFO: u16 = 0xd007;
const IDBOX: u16 = 0xdc05;
const ID: u16 = 0xde05;
const ATTRS: u16 = 0xdd05;
const MAT_REF: u16 = 0xd107;
const LAYER_REF: u16 = 0xd207;
const FLAGS: u16 = 0xd307;
const VERTS: u16 = 0x8913;
const VERT: u16 = 0xc409;
const XYZ: u16 = 0xc509;
const EDGES: u16 = 0x8a13;
const EDGE: u16 = 0xb80b;
const EDGE_A: u16 = 0xb90b;
const EDGE_B: u16 = 0xba0b;
const FACES: u16 = 0x8b13;
const FACE: u16 = 0xac0d;
const NORMAL: u16 = 0xad0d;
const LOOPS: u16 = 0xae0d;
const LOOP: u16 = 0x9411;
const COEDGES: u16 = 0x9511;
const COEDGE: u16 = 0xa00f;
const CO_EDGE: u16 = 0xa10f;
const CO_DIR: u16 = 0xa20f;
const BACK_MAT: u16 = 0xaf0d;
const COMPS: u16 = 0x8c13;
const GROUPS: u16 = 0x8d13;
const GROUP: u16 = 0x4c1d;
const IMAGES: u16 = 0x9013;
const IMAGE: u16 = 0x401f;
const INST: u16 = 0x6419;
const INST_NAME: u16 = 0x6519;
const INST_M: u16 = 0x6619;
const INST_DEF: u16 = 0x6719;
const CURVES: u16 = 0x9713;
const CURVES2: u16 = 0x9613;
const GUIDE_LINES: u16 = 0x9113;
const GUIDE_POINTS: u16 = 0x9213;
const ANNOTATIONS: u16 = 0x9913;

/// Entity-list children that are bookkeeping, not drawing elements.
fn quiet(t: u16) -> bool {
    matches!(t, INFO | 0x8e13 | 0x9b13 | 0x9e13 | 0x9f13 | 0xa013 | CURVES | CURVES2)
}
const FLAG_HIDDEN: u8 = 0x01;
const FLAG_SOFT: u8 = 0x08;
const FLAG_SMOOTH: u8 = 0x10;
const NEWEST_KNOWN: f64 = 26.0;

// ---------------------------------------------------------------- model.dat walker

/// The records in [p, e) of one buffer. Strict: a record running past e = damaged (and the walk ends there).
/// Loose (optional data such as texture placement): stops quietly at the first record that does not fit.
struct Recs<'a> {
    b: &'a [u8],
    p: usize,
    e: usize,
    strict: bool,
}

impl Iterator for Recs<'_> {
    type Item = Res<(u16, usize, usize)>;
    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let p = self.p;
        if self.strict {
            if p >= self.e {
                return None;
            }
            if p.saturating_add(6) > self.e {
                self.p = self.e;
                return Some(Err(damaged("ccskp.damaged.record_header")));
            }
        } else if p.saturating_add(6) > self.e {
            return None;
        }
        let h = self.b.get(p..p.saturating_add(6))?;
        let tag = u16::from_be_bytes([*h.first()?, *h.get(1)?]);
        let n = u32::from_le_bytes([*h.get(2)?, *h.get(3)?, *h.get(4)?, *h.get(5)?]) as usize;
        let s = p.saturating_add(6);
        let z = s.saturating_add(n);
        if z > self.e {
            self.p = self.e;
            return if self.strict { Some(Err(damaged("ccskp.damaged.record_past"))) } else { None };
        }
        self.p = z;
        Some(Ok((tag, s, z)))
    }
}

#[derive(Clone, Copy)]
struct Tlv<'a> {
    b: &'a [u8],
}

impl<'a> Tlv<'a> {
    #[inline]
    fn each(&self, s: usize, e: usize) -> Recs<'a> {
        Recs { b: self.b, p: s, e, strict: true }
    }
    #[inline]
    fn each_loose(&self, s: usize, e: usize) -> Recs<'a> {
        Recs { b: self.b, p: s, e, strict: false }
    }
    /// First child with this tag → (start, end) or None (the walk stops at the hit, so later damage is not seen).
    fn find(&self, s: usize, e: usize, tag: u16, loose: bool) -> Res<Option<(usize, usize)>> {
        let it = if loose { self.each_loose(s, e) } else { self.each(s, e) };
        for r in it {
            let (t, a, z) = r?;
            if t == tag {
                return Ok(Some((a, z)));
            }
        }
        Ok(None)
    }
    /// Little-endian unsigned id of 1–6 bytes.
    #[inline]
    fn uint(&self, s: usize, e: usize) -> Res<u64> {
        let n = e.saturating_sub(s);
        if !(1..=6).contains(&n) {
            return Err(damaged("ccskp.damaged.id_size"));
        }
        let mut v = 0u64;
        for &c in self.b.get(s..e).unwrap_or(&[]).iter().rev() {
            v = (v << 8) | u64::from(c);
        }
        Ok(v)
    }
    #[inline]
    fn f64(&self, p: usize) -> f64 {
        match self.b.get(p..p.saturating_add(8)) {
            Some(s) => {
                let mut a = [0u8; 8];
                a.copy_from_slice(s);
                f64::from_le_bytes(a)
            }
            None => f64::NAN,
        }
    }
    #[inline]
    fn byte(&self, p: usize) -> u8 {
        self.b.get(p).copied().unwrap_or(0)
    }
    fn text(&self, s: usize, e: usize) -> String {
        utf8(self.b.get(s..e).unwrap_or(&[]))
    }
    /// An entity's own id: DC05 > DE05 inside [s, e) (an INFO record or the entity itself).
    fn id_of(&self, s: usize, e: usize) -> Res<Option<u64>> {
        let Some((a, z)) = self.find(s, e, IDBOX, false)? else { return Ok(None) };
        match self.find(a, z, ID, true)? {
            Some((x, y)) => Ok(Some(self.uint(x, y)?)),
            None => Ok(None),
        }
    }
}

/// `utf8(bytes)`: strict UTF-8 (BOM dropped) or Latin-1, NULs removed, JS-trimmed.
fn utf8(bytes: &[u8]) -> String {
    if bytes.is_ascii() {
        let s = bytes.iter().filter(|&&c| c != 0).map(|&c| char::from(c)).collect::<String>();
        let t = js_trim(&s);
        return if t.len() == s.len() { s } else { t.to_string() };
    }
    let s = utf8_or_latin1(bytes);
    let s: String = s.chars().filter(|&c| c != '\0').collect();
    js_trim(&s).to_string()
}

fn guid_text(b: &[u8]) -> Option<String> {
    if b.len() != 16 {
        return None;
    }
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(36);
    for (i, &c) in b.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        out.push(char::from(HEX.get(usize::from(c >> 4)).copied().unwrap_or(b'0')));
        out.push(char::from(HEX.get(usize::from(c & 15)).copied().unwrap_or(b'0')));
    }
    Some(out)
}

/// (key, optional extra fill, count)
type NoteEntry = (&'static str, Option<(&'static str, String)>, u64);

/// Counter of skipped things → one plain-English note per kind, in order of first appearance.
#[derive(Debug, Default, Clone)]
struct Notes {
    list: Vec<NoteEntry>,
}

impl Notes {
    fn add(&mut self, key: &'static str, n: u64) {
        self.add_fill(key, n, None);
    }
    fn add_fill(&mut self, key: &'static str, n: u64, fill: Option<(&'static str, String)>) {
        if n == 0 {
            return;
        }
        if let Some(e) = self.list.iter_mut().find(|e| e.0 == key && e.1 == fill) {
            e.2 = e.2.saturating_add(n);
        } else {
            self.list.push((key, fill, n));
        }
    }
    fn merge(&mut self, other: Notes) {
        for (k, f, n) in other.list {
            self.add_fill(k, n, f);
        }
    }
    fn texts(&self) -> Vec<String> {
        self.list
            .iter()
            .map(|(k, f, n)| {
                let ns = n.to_string();
                match f {
                    Some((fk, fv)) => msg(k, &[(fk, fv.as_str()), ("n", &ns)]),
                    None => msg(k, &[("n", &ns)]),
                }
            })
            .collect()
    }
}

// ---------------------------------------------------------------- material.xml

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// The `&…;` entities of XML attribute values (`unxml`).
fn unxml(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(rest.get(..i).unwrap_or(""));
        let tail = rest.get(i.saturating_add(1)..).unwrap_or("");
        let mut done: Option<(usize, String)> = None; // (bytes consumed after '&', replacement)
        for (name, rep) in [("lt;", "<"), ("gt;", ">"), ("amp;", "&"), ("apos;", "'"), ("quot;", "\"")] {
            if tail.starts_with(name) {
                done = Some((name.len(), rep.to_string()));
                break;
            }
        }
        if done.is_none() && tail.starts_with('#') {
            let body = tail.get(1..).unwrap_or("");
            let nd = body.bytes().take_while(u8::is_ascii_digit).count();
            if nd > 0 && body.as_bytes().get(nd) == Some(&b';') {
                let cp: f64 = body.get(..nd).unwrap_or("").parse().unwrap_or(f64::INFINITY);
                done = Some((nd.saturating_add(2), code_point(cp, tail.get(..nd.saturating_add(2)).unwrap_or(""))));
            } else if body.starts_with('x') {
                let hx = body.get(1..).unwrap_or("");
                let nh = hx.bytes().take_while(u8::is_ascii_hexdigit).count();
                if nh > 0 && hx.as_bytes().get(nh) == Some(&b';') {
                    let mut cp = 0f64;
                    for c in hx.get(..nh).unwrap_or("").chars() {
                        cp = cp * 16.0 + f64::from(c.to_digit(16).unwrap_or(0));
                    }
                    done = Some((nh.saturating_add(3), code_point(cp, tail.get(..nh.saturating_add(3)).unwrap_or(""))));
                }
            }
        }
        match done {
            Some((n, rep)) => {
                out.push_str(&rep);
                rest = tail.get(n..).unwrap_or("");
            }
            None => {
                out.push('&');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `String.fromCodePoint(cp)` when 0 ≤ cp ≤ 0x10FFFF, else the entity text kept (`&` + what follows). A lone
/// surrogate code point becomes U+FFFD (Rust text cannot hold one; JS would).
fn code_point(cp: f64, after_amp: &str) -> String {
    if (0.0..=1_114_111.0).contains(&cp) {
        let c = char::from_u32(f64_to_u32(cp)).unwrap_or(char::REPLACEMENT_CHARACTER);
        c.to_string()
    } else {
        format!("&{after_amp}")
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn f64_to_u32(v: f64) -> u32 {
    // callers pass an integer in 0..=0x10FFFF
    v as u32
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn byte_of(v: f64) -> u8 {
    // callers clamp to 0..=255 first (an integer: parseInt)
    v.clamp(0.0, 255.0) as u8
}

/// Attributes of the first `<prefix:name …>` element (`/<(?:[A-Za-z0-9_]+:)?name\b([^>]*)>/`), entity-decoded;
/// a repeated attribute keeps its last value (lookups search from the end).
fn xml_element(text: &str, name: &str) -> Option<Vec<(String, String)>> {
    let b = text.as_bytes();
    let tag_at = |ns: usize| -> Option<usize> {
        let ne = ns.checked_add(name.len())?;
        if b.get(ns..ne)? != name.as_bytes() {
            return None;
        }
        if text.get(ne..).and_then(|r| r.chars().next()).is_some_and(is_word) {
            return None;
        }
        Some(ne)
    };
    let mut from = 0usize;
    while let Some(off) = text.get(from..).and_then(|r| r.find('<')) {
        let lt = from.saturating_add(off);
        let after = lt.saturating_add(1);
        let run = b.get(after..).map_or(0, |r| r.iter().take_while(|c| c.is_ascii_alphanumeric() || **c == b'_').count());
        let mut hit = None;
        if run > 0 && b.get(after.saturating_add(run)) == Some(&b':') {
            hit = tag_at(after.saturating_add(run).saturating_add(1));
        }
        if hit.is_none() {
            hit = tag_at(after);
        }
        if let Some(ne) = hit
            && let Some(gt) = text.get(ne..).and_then(|r| r.find('>'))
        {
            return Some(attributes(text.get(ne..ne.saturating_add(gt)).unwrap_or("")));
        }
        from = after;
    }
    None
}

/// `/([A-Za-z_][\w:.-]*)\s*=\s*(?:"([^"]*)"|'([^']*)')/g` over an element's attribute text.
fn attributes(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let at = |k: usize| chars.get(k).map(|&(_, c)| c);
    let off = |k: usize| chars.get(k).map_or(s.len(), |&(o, _)| o);
    let mut i = 0usize;
    while i < chars.len() {
        let try_at = |i: usize| -> Option<(usize, String, String)> {
            let c = at(i)?;
            if !(c.is_ascii_alphabetic() || c == '_') {
                return None;
            }
            let mut j = i.saturating_add(1);
            while at(j).is_some_and(|c| is_word(c) || c == ':' || c == '.' || c == '-') {
                j = j.saturating_add(1);
            }
            let name = s.get(off(i)..off(j))?.to_string();
            let mut k = j;
            while at(k).is_some_and(crate::util::is_js_space) {
                k = k.saturating_add(1);
            }
            if at(k) != Some('=') {
                return None;
            }
            k = k.saturating_add(1);
            while at(k).is_some_and(crate::util::is_js_space) {
                k = k.saturating_add(1);
            }
            let q = at(k)?;
            if q != '"' && q != '\'' {
                return None;
            }
            let vs = k.saturating_add(1);
            let mut ve = vs;
            while at(ve).is_some_and(|c| c != q) {
                ve = ve.saturating_add(1);
            }
            at(ve)?;
            let value = unxml(s.get(off(vs)..off(ve))?);
            Some((ve.saturating_add(1), name, value))
        };
        match try_at(i) {
            Some((next, k, v)) => {
                out.push((k, v));
                i = next;
            }
            None => i = i.saturating_add(1),
        }
    }
    out
}

fn attr<'x>(a: &'x [(String, String)], k: &str) -> Option<&'x str> {
    a.iter().rev().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
}

#[derive(Debug, Clone)]
struct TexXml {
    file: String,
    width: Option<f64>,
    height: Option<f64>,
    path: String,
}

#[derive(Debug, Clone)]
struct MatXml {
    name: Option<String>,
    color: Option<[u8; 3]>,
    alpha: f64,
    texture: Option<TexXml>,
    folder: String,
}

fn read_material_xml(text: &str) -> Option<MatXml> {
    let mat = xml_element(text, "material")?;
    let int = |v: Option<&str>| -> Option<u8> {
        let n = parse_int10(v?)?;
        n.is_finite().then(|| byte_of(n))
    };
    let (r, g, b) = (int(attr(&mat, "colorRed")), int(attr(&mat, "colorGreen")), int(attr(&mat, "colorBlue")));
    let mut alpha = 1.0;
    if attr(&mat, "useTrans") == Some("1")
        && let Some(t) = attr(&mat, "trans").and_then(parse_float)
        && t.is_finite()
    {
        alpha = (1.0 - t).clamp(0.0, 1.0);
    }
    let tex = xml_element(text, "texture");
    let img = xml_element(text, "image");
    let num = |v: Option<&str>| -> Option<f64> { v.and_then(parse_float).filter(|n| n.is_finite() && *n > 0.0 && *n < 1e9) };
    fn nonempty(v: Option<&str>) -> Option<&str> {
        v.filter(|s| !s.is_empty())
    }
    Some(MatXml {
        name: attr(&mat, "name").map(str::to_string),
        color: match (r, g, b) {
            (Some(r), Some(g), Some(b)) => Some([r, g, b]),
            _ => None,
        },
        alpha,
        texture: tex.map(|t| TexXml {
            file: attr(&t, "textureFilename").unwrap_or("").to_string(),
            width: num(attr(&t, "xScale")),
            height: num(attr(&t, "yScale")),
            path: match &img {
                Some(i) => nonempty(attr(i, "path")).or(nonempty(attr(i, "file_name"))).unwrap_or("").to_string(),
                None => String::new(),
            },
        }),
        folder: String::new(),
    })
}

fn ends_with_ci(s: &str, suffix: &str) -> bool {
    s.len() >= suffix.len() && s.get(s.len().saturating_sub(suffix.len())..).is_some_and(|t| t.eq_ignore_ascii_case(suffix))
}

fn starts_with_ci(s: &str, prefix: &str) -> bool {
    s.get(..prefix.len()).is_some_and(|t| t.eq_ignore_ascii_case(prefix))
}

fn sniff_mime(b: &[u8], name: &str) -> &'static str {
    let at = |i: usize| b.get(i).copied();
    if b.len() >= 8 && b.starts_with(&[0x89, 0x50, 0x4e, 0x47]) {
        return "image/png";
    }
    if b.len() >= 3 && b.starts_with(&[0xff, 0xd8, 0xff]) {
        return "image/jpeg";
    }
    if b.len() >= 6 && b.starts_with(&[0x47, 0x49, 0x46]) {
        return "image/gif";
    }
    if b.len() >= 2 && b.starts_with(&[0x42, 0x4d]) {
        return "image/bmp";
    }
    if b.len() >= 4 && (b.starts_with(&[0x49, 0x49, 0x2a]) || (at(0) == Some(0x4d) && at(1) == Some(0x4d) && at(3) == Some(0x2a))) {
        return "image/tiff";
    }
    if b.len() >= 12 && b.get(8..12) == Some(b"WEBP") {
        return "image/webp";
    }
    if ends_with_ci(name, ".png") {
        "image/png"
    } else if ends_with_ci(name, ".jpg") || ends_with_ci(name, ".jpeg") {
        "image/jpeg"
    } else {
        "application/octet-stream"
    }
}

/// Normalise a path inside the archive ("materials/x/./a.jpg", "../y/b.jpg"); None when it climbs out.
fn join_path(folder: &str, rel: &str) -> Option<String> {
    let rel = rel.replace('\\', "/");
    let mut out: Vec<&str> = Vec::new();
    let head = if folder.is_empty() { None } else { Some(folder.split('/')) };
    for p in head.into_iter().flatten().chain(rel.split('/')) {
        if p.is_empty() || p == "." {
            continue;
        }
        if p == ".." {
            out.pop()?;
        } else {
            out.push(p);
        }
    }
    Some(out.join("/"))
}

/// The last segment of a path split on `/` or `\` (`s.split(/[\\/]/).pop()`).
fn last_segment(s: &str) -> &str {
    s.rsplit(['/', '\\']).next().unwrap_or("")
}

// ---------------------------------------------------------------- units (meta/meta.dat)

fn unit_word(word: &str) -> Option<&'static str> {
    if starts_with_ci(word, "inch") {
        Some("in")
    } else if starts_with_ci(word, "feet") || starts_with_ci(word, "foot") {
        Some("ft")
    } else if starts_with_ci(word, "milli") {
        Some("mm")
    } else if starts_with_ci(word, "centi") {
        Some("cm")
    } else if starts_with_ci(word, "meter") || starts_with_ci(word, "metre") {
        Some("m")
    } else {
        None
    }
}

/// 0x6400 > 0x6D00 text of meta.dat, or None.
fn read_units(meta: &[u8]) -> Option<String> {
    let (mut p, mut e, mut depth) = (0usize, meta.len(), 0u8);
    while p.saturating_add(6) <= e {
        let tag = u16::from_be_bytes([*meta.get(p)?, *meta.get(p.saturating_add(1))?]);
        let n = u32::from_le_bytes([
            *meta.get(p.saturating_add(2))?,
            *meta.get(p.saturating_add(3))?,
            *meta.get(p.saturating_add(4))?,
            *meta.get(p.saturating_add(5))?,
        ]) as usize;
        let s = p.saturating_add(6);
        let z = s.saturating_add(n);
        if z > e {
            return None;
        }
        if tag == 0x6400 && depth == 0 {
            (p, e, depth) = (s, z, 1);
            continue;
        }
        if tag == 0x6d00 && depth == 1 {
            return Some(utf8(meta.get(s..z)?));
        }
        p = z;
    }
    None
}

// ---------------------------------------------------------------- geometry helpers

/// Newell normal (not normalised) of a loop.
fn newell(l: &[P3]) -> P3 {
    let (mut nx, mut ny, mut nz) = (0.0, 0.0, 0.0);
    let n = l.len();
    for (i, a) in l.iter().enumerate() {
        let b = l.get(i.saturating_add(1).checked_rem(n).unwrap_or(0)).unwrap_or(a);
        nx += (a[1] - b[1]) * (a[2] + b[2]);
        ny += (a[2] - b[2]) * (a[0] + b[0]);
        nz += (a[0] - b[0]) * (a[1] + b[1]);
    }
    [nx, ny, nz]
}

/// Plane axes of a face with normal n: x = normalize(Z × n), y = n × x (vertical normals: X and ±Y).
fn plane_axes(n: &P3) -> (P3, P3) {
    let cl = hypot(&[n[0], n[1]]);
    if cl < 1e-9 {
        ([1.0, 0.0, 0.0], [0.0, if n[2] >= 0.0 { 1.0 } else { -1.0 }, 0.0])
    } else {
        let xr = [-n[1] / cl, n[0] / cl, 0.0];
        let yr = [n[1] * xr[2] - n[2] * xr[1], n[2] * xr[0] - n[0] * xr[2], n[0] * xr[1] - n[1] * xr[0]];
        (xr, yr)
    }
}

/// Inverse of a placement (texture → plane, row vectors), or None when singular (`!(|det| > 1e-12)`).
fn place_inverse(place: &[f64; 9]) -> Option<[f64; 9]> {
    let [a, b, c, d, e, f, g, h, i] = *place;
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    if det.is_nan() || det.abs() <= 1e-12 {
        return None;
    }
    let k = 1.0 / det;
    Some([
        (e * i - f * h) * k,
        (c * h - b * i) * k,
        (b * f - c * e) * k,
        (f * g - d * i) * k,
        (a * i - c * g) * k,
        (c * d - a * f) * k,
        (d * h - e * g) * k,
        (b * g - a * h) * k,
        (a * e - b * d) * k,
    ])
}

/// Texture coordinates of p (SketchUp's default projection, or through the face's placement matrix).
#[inline]
fn face_uv(p: &P3, xr: &P3, yr: &P3, inv: Option<&[f64; 9]>, tw: f64, th: f64) -> [f64; 2] {
    let px = p[0] * xr[0] + p[1] * xr[1] + p[2] * xr[2];
    let py = p[0] * yr[0] + p[1] * yr[1] + p[2] * yr[2];
    let Some(inv) = inv else { return [px / tw, py / th] };
    let u = px * inv[0] + py * inv[3] + inv[6];
    let v = px * inv[1] + py * inv[4] + inv[7];
    let mut q = px * inv[2] + py * inv[5] + inv[8];
    if q.abs() < 1e-12 {
        q = 1.0;
    }
    [u / q / tw, v / q / th]
}

// ---------------------------------------------------------------- the reader

/// Read a SketchUp 2021+ file (`readVff(bytes, header)`).
pub fn read_vff(bytes: &[u8], header: &Header, limits: &Limits) -> Res<Body> {
    // The ZIP starts at the first local header after the SketchUp header records.
    let mut zip_at = None;
    if let Some(last) = bytes.len().checked_sub(4) {
        let end = last.min(header.body_start.saturating_add(4096));
        let mut i = header.body_start;
        while i <= end {
            if bytes.get(i..i.saturating_add(4)) == Some(&[0x50, 0x4b, 3, 4]) {
                zip_at = Some(i);
                break;
            }
            i = i.saturating_add(1);
        }
    }
    let zip_at = zip_at.ok_or_else(|| damaged("ccskp.damaged.no_archive"))?;
    let zip = open_zip(bytes, zip_at, limits)?;
    let mut budget = 0u64;
    let model_name =
        zip.names.iter().find(|n| n.to_lowercase() == "model.dat").cloned().ok_or_else(|| damaged("ccskp.damaged.no_model_dat"))?;
    let md = zip.read(&model_name, &mut budget)?;

    // model.dat and the material descriptions are independent: read them side by side, then use them in the JS's
    // order (a model error wins over any material.xml error; the model's notes come first).
    let xml_names: Vec<(String, String)> =
        zip.names.iter().filter_map(|n| material_xml_folder(n).map(|f| (n.clone(), f.to_string()))).collect();
    let xml_budget = budget;
    let (model, xml_reads) = join(
        || {
            let mut notes = Notes::default();
            read_model_dat(&md, &mut notes, limits).map(|m| (m, notes))
        },
        || read_xmls(&zip, &xml_names),
    );
    let (mut model, mut notes) = match model {
        Ok(m) => m,
        // A file newer than any we know that we cannot follow is "too new", not "damaged".
        Err(e) if e.code == "damaged" && header.major > NEWEST_KNOWN => {
            return Err(err("too_new", "ccskp.too_new", &[("version", &header.version), ("newest", "2026")]));
        }
        Err(e) => return Err(e),
    };
    drop(md);
    budget = xml_budget;

    // Materials: the model's list (ids → names) joined to materials/*/material.xml by name, else by folder.
    let mut xml_by_name: FxMap<String, usize> = FxMap::default();
    let mut xml_by_folder: FxMap<String, usize> = FxMap::default();
    let mut xmls: Vec<MatXml> = Vec::new();
    for ((name, folder), read) in xml_names.iter().zip(xml_reads) {
        // the budget check in file order (a failed read adds nothing to it, as in the JS)
        let read = match zip.early_error(name) {
            Some(e) => Err(e),
            None if budget.saturating_add(zip.size(name)) > limits.inflated_bytes => Err(err("too_big", "ccskp.too_big.unpacked", &[])),
            None => read,
        };
        let info = match read {
            Ok(x) => {
                budget = budget.saturating_add(zip.size(name));
                x
            }
            Err(e) if e.code == "too_big" => return Err(e),
            Err(_) => None,
        };
        let Some(mut info) = info else {
            notes.add("ccskp.note.material_xml", 1);
            continue;
        };
        info.folder = format!("materials/{folder}");
        let ix = xmls.len();
        if let Some(n) = info.name.as_ref().filter(|n| !n.is_empty()) {
            xml_by_name.insert(n.clone(), ix);
        }
        xml_by_folder.insert(folder.clone(), ix);
        xmls.push(info);
    }

    // Each material's texture entry (decided in order, with its notes); the images are unpacked afterwards.
    let mut materials: Vec<Material> = Vec::with_capacity(model.materials.len());
    let mut tex_jobs: Vec<(usize, String, f64, f64)> = Vec::new(); // (material, entry, width, height)
    for mat_name in &model.materials {
        let x = xml_by_name.get(mat_name).or_else(|| xml_by_folder.get(mat_name)).and_then(|&i| xmls.get(i));
        let out = Material {
            name: Some(x.and_then(|x| x.name.clone().filter(|n| !n.is_empty())).unwrap_or_else(|| mat_name.clone())),
            color: x.and_then(|x| x.color),
            alpha: x.map_or(1.0, |x| x.alpha),
            texture: None,
        };
        if x.is_none() {
            notes.add("ccskp.note.material_no_xml", 1);
        }
        if let Some(x) = x
            && let Some(t) = &x.texture
        {
            let mut want: Vec<Option<String>> = Vec::new();
            if !t.path.is_empty() {
                want.push(join_path(&x.folder, &t.path));
            }
            if !t.file.is_empty() {
                want.push(join_path(&x.folder, last_segment(&t.file)));
            }
            let mut entry = want.into_iter().flatten().find(|w| !w.is_empty() && zip.has(w));
            if entry.is_none() {
                let pre = format!("{}/", x.folder);
                entry = zip.names.iter().find(|n| n.starts_with(&pre) && !ends_with_ci(n, ".xml") && !is_thumbnail_name(n)).cloned();
            }
            match (entry, t.width, t.height) {
                (None, _, _) => notes.add("ccskp.note.texture_missing", 1),
                (Some(_), None, _) | (Some(_), _, None) => notes.add("ccskp.note.texture_no_size", 1),
                (Some(e), Some(w), Some(h)) => tex_jobs.push((materials.len(), e, w, h)),
            }
        }
        materials.push(out);
    }
    // The size budget in file order (an unpacked entry is exactly its declared size), then unpack.
    let mut starts = Vec::with_capacity(tex_jobs.len());
    for (_, e, _, _) in &tex_jobs {
        starts.push(budget);
        budget = budget.saturating_add(zip.size(e));
    }
    let images = unpack_all(&zip, &tex_jobs, &starts);
    for ((mi, entry, w, h), img) in tex_jobs.iter().zip(images) {
        let img = img?;
        if let Some(m) = materials.get_mut(*mi) {
            m.texture = Some(Texture {
                name: entry.rsplit('/').next().unwrap_or("").to_string(),
                mime: sniff_mime(&img, entry),
                bytes: img,
                width: *w,
                height: *h,
            });
        }
    }

    // Tags: colour from their "Layer_<name>" material.xml.
    let layers: Vec<Layer> = model
        .layers
        .iter()
        .map(|l| Layer {
            name: Some(l.name.clone()),
            visible: Some(l.visible),
            color: l
                .mat_name
                .as_ref()
                .filter(|m| !m.is_empty())
                .and_then(|m| xml_by_name.get(m))
                .and_then(|&i| xmls.get(i))
                .and_then(|x| x.color),
        })
        .collect();

    // UVs for textured faces (front side), now the tile sizes are known.
    for d in &mut model.defs {
        for (f, extra) in d.def.faces.iter_mut().zip(&d.extra) {
            let Some(tex) = f.front.and_then(|i| materials.get(i)).and_then(|m| m.texture.as_ref()) else { continue };
            let (xr, yr) = plane_axes(&extra.n);
            let inv = extra.place.as_ref().and_then(place_inverse);
            f.uv = Some(
                f.loops.iter().map(|l| l.iter().map(|p| face_uv(p, &xr, &yr, inv.as_ref(), tex.width, tex.height)).collect()).collect(),
            );
        }
    }

    // Thumbnail and units.
    let mut thumbnail = None;
    for n in ["meta/model_thumbnail.png", "meta/preview_thumbnail.png"] {
        if zip.has(n) {
            let b = zip.read(n, &mut budget)?.into_owned();
            thumbnail = Some(Image { mime: sniff_mime(&b, n), bytes: b });
            break;
        }
    }
    let mut units = None;
    if zip.has("meta/meta.dat") {
        let meta = zip.read("meta/meta.dat", &mut budget)?;
        if let Some(word) = read_units(&meta).filter(|w| !w.is_empty()) {
            match unit_word(&word) {
                Some(u) => units = Some(u),
                None => notes.add_fill("ccskp.note.unit_unknown", 1, Some(("unit", word))),
            }
        }
    }
    let styles = zip.names.iter().filter(|n| is_style_xml(n)).count();
    notes.add("ccskp.note.styles", styles as u64);

    Ok(Body { materials, layers, defs: model.defs.into_iter().map(|d| d.def).collect(), root: 0, thumbnail, units, notes: notes.texts() })
}

/// `/^materials\/([^/]+)\/material\.xml$/i` → the folder.
fn material_xml_folder(name: &str) -> Option<&str> {
    if !starts_with_ci(name, "materials/") || !ends_with_ci(name, "/material.xml") {
        return None;
    }
    let mid = name.get("materials/".len()..name.len().checked_sub("/material.xml".len())?)?;
    (!mid.is_empty() && !mid.contains('/')).then_some(mid)
}

/// `/^styles\/[^/]+\/style\.xml$/i`.
fn is_style_xml(name: &str) -> bool {
    if !starts_with_ci(name, "styles/") || !ends_with_ci(name, "/style.xml") {
        return false;
    }
    let Some(e) = name.len().checked_sub("/style.xml".len()) else { return false };
    name.get("styles/".len()..e).is_some_and(|m| !m.is_empty() && !m.contains('/'))
}

/// `/\/thumbnail\.[a-z]+$/i`.
fn is_thumbnail_name(n: &str) -> bool {
    let Some(dot) = n.rfind('.') else { return false };
    let ext = n.get(dot.saturating_add(1)..).unwrap_or("");
    !ext.is_empty() && ext.bytes().all(|c| c.is_ascii_alphabetic()) && ends_with_ci(n.get(..dot).unwrap_or(""), "/thumbnail")
}

/// `a` and `b` side by side (rayon natively; one after the other on the web).
fn join<A: Send, B: Send>(a: impl FnOnce() -> A + Send, b: impl FnOnce() -> B + Send) -> (A, B) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        rayon::join(a, b)
    }
    #[cfg(target_arch = "wasm32")]
    {
        (a(), b())
    }
}

/// Every material.xml unpacked and parsed (Ok(None) = not a material description), without the size budget,
/// which the caller applies in file order.
fn read_xmls(zip: &crate::zip::Zip<'_>, names: &[(String, String)]) -> Vec<Res<Option<MatXml>>> {
    let one = |(n, _): &(String, String)| zip.read_unbudgeted(n, 0).map(|raw| read_material_xml(&utf8_lossy(&raw)));
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        if names.len() > 8 {
            return names.par_iter().with_min_len(8).map(one).collect();
        }
    }
    names.iter().map(one).collect()
}

fn unpack_all(zip: &crate::zip::Zip<'_>, jobs: &[(usize, String, f64, f64)], starts: &[u64]) -> Vec<Res<Vec<u8>>> {
    let one = |(j, s): (&(usize, String, f64, f64), &u64)| zip.read_unbudgeted(&j.1, *s).map(std::borrow::Cow::into_owned);
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        if jobs.len() > 1 {
            return jobs.par_iter().zip(starts.par_iter()).map(one).collect();
        }
    }
    jobs.iter().zip(starts.iter()).map(one).collect()
}

struct LayerRaw {
    name: String,
    visible: bool,
    mat_name: Option<String>,
}

/// Per-face data the UV pass needs (the JS keeps them as `_n` / `_place` on the face).
struct FaceExtra {
    n: P3,
    place: Option<[f64; 9]>,
}

struct DefOut {
    def: Def,
    extra: Vec<FaceExtra>,
}

struct ModelDat {
    materials: Vec<String>,
    layers: Vec<LayerRaw>,
    defs: Vec<DefOut>,
}

/// One definition record's own fields (`7C15`), read before its entity list.
struct DefHead {
    name: String,
    guid: Option<String>,
    kind: &'static str,
    ents: Option<(usize, usize)>,
    id: Option<u64>,
}

fn def_heads(tlv: &Tlv<'_>, recs: &[(usize, usize)]) -> Vec<Res<DefHead>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        if recs.len() > 256 {
            return recs.par_iter().with_min_len(64).map(|&(s, e)| def_head(tlv, s, e)).collect();
        }
    }
    recs.iter().map(|&(s, e)| def_head(tlv, s, e)).collect()
}

fn def_head(tlv: &Tlv<'_>, s: usize, e: usize) -> Res<DefHead> {
    let mut h = DefHead { name: String::new(), guid: None, kind: "component", ents: None, id: None };
    for r in tlv.each(s, e) {
        let (t, a, z) = r?;
        match t {
            ENTS => h.ents = Some((a, z)),
            DEF_NAME => h.name = tlv.text(a, z),
            DEF_GUID => h.guid = guid_text(tlv.b.get(a..z).unwrap_or(&[])),
            DEF_KIND if z > a => {
                let k = tlv.uint(a, z.min(a.saturating_add(4)))?;
                h.kind = match k {
                    1 => "group",
                    2 => "image",
                    _ => "component",
                };
            }
            _ => {}
        }
    }
    if let Some((a, z)) = h.ents {
        let info = tlv.find(a, z, INFO, false)?;
        h.id = match info {
            Some((x, y)) => tlv.id_of(x, y)?,
            None => None,
        };
    }
    Ok(h)
}

/// Parse model.dat → materials (id order), layers, defs (instances resolved), root = 0.
fn read_model_dat(md: &[u8], notes: &mut Notes, limits: &Limits) -> Res<ModelDat> {
    let tlv = Tlv { b: md };
    let mut top = (0usize, md.len());
    // One F401 wraps the whole file.
    let mut only = Vec::new();
    for r in tlv.each(0, md.len()) {
        only.push(r?);
        if only.len() > 1 {
            // keep walking for the strict checks, but only "exactly one" matters
        }
    }
    if let [(FILE, s, e)] = only.as_slice() {
        top = (*s, *e);
    }

    let mut mat_ids: FxMap<u64, usize> = FxMap::default();
    let mut materials: Vec<String> = Vec::new();
    let mut layer_ids: FxMap<u64, usize> = FxMap::default();
    let mut layers: Vec<LayerRaw> = Vec::new();
    let mut def_records: Vec<(usize, usize)> = Vec::new();
    let mut root_record: Option<(usize, usize)> = None;
    let mut scenes = 0u64;

    for r in tlv.each(top.0, top.1) {
        let (t, s, e) = r?;
        match t {
            MATS => {
                let Some(a) = tlv.find(s, e, MATS2, false)? else { continue };
                let Some(b) = tlv.find(a.0, a.1, MATS3, false)? else { continue };
                for r2 in tlv.each(b.0, b.1) {
                    let (t2, s2, e2) = r2?;
                    if t2 != MAT {
                        continue;
                    }
                    let id = tlv.id_of(s2, e2)?;
                    let nm = tlv.find(s2, e2, MAT_NAME, false)?;
                    let Some(id) = id else {
                        notes.add("ccskp.note.material_no_id", 1);
                        continue;
                    };
                    mat_ids.insert(id, materials.len());
                    materials.push(nm.map(|(x, y)| tlv.text(x, y)).unwrap_or_default());
                }
            }
            LAYERS => {
                let Some(a) = tlv.find(s, e, LAYERS2, false)? else { continue };
                let Some(b) = tlv.find(a.0, a.1, LAYERS3, false)? else { continue };
                for r2 in tlv.each(b.0, b.1) {
                    let (t2, s2, e2) = r2?;
                    if t2 != LAYER {
                        continue;
                    }
                    let id = tlv.id_of(s2, e2)?;
                    let nm = tlv.find(s2, e2, LAYER_NAME, false)?;
                    let hid = tlv.find(s2, e2, LAYER_HIDDEN, false)?;
                    let lm = tlv.find(s2, e2, LAYER_MAT, false)?;
                    let mut mat_name = None;
                    if let Some(lm) = lm
                        && let Some(c) = tlv.find(lm.0, lm.1, MAT, true)?
                        && let Some(cn) = tlv.find(c.0, c.1, MAT_NAME, true)?
                    {
                        mat_name = Some(tlv.text(cn.0, cn.1));
                    }
                    let Some(id) = id else {
                        notes.add("ccskp.note.tag_no_id", 1);
                        continue;
                    };
                    layer_ids.insert(id, layers.len());
                    let hidden = hid.is_some_and(|(x, y)| y > x && tlv.byte(x) == 1);
                    layers.push(LayerRaw { name: nm.map(|(x, y)| tlv.text(x, y)).unwrap_or_default(), visible: !hidden, mat_name });
                }
            }
            DEFS => {
                let Some(a) = tlv.find(s, e, DEFS2, false)? else { continue };
                let Some(b) = tlv.find(a.0, a.1, DEFS3, false)? else { continue };
                for r2 in tlv.each(b.0, b.1) {
                    let (t2, s2, e2) = r2?;
                    if t2 == DEF {
                        def_records.push((s2, e2));
                    }
                }
            }
            ROOT => root_record = tlv.find(s, e, ENTS, false)?,
            SCENES => {
                let Some(a) = tlv.find(s, e, SCENES2, true)? else { continue };
                let Some(b) = tlv.find(a.0, a.1, SCENES3, true)? else { continue };
                for (t2, _, _) in tlv.each_loose(b.0, b.1).flatten() {
                    if t2 == SCENE {
                        scenes = scenes.saturating_add(1);
                    }
                }
            }
            _ => {}
        }
    }
    let root_record = root_record.ok_or_else(|| damaged("ccskp.damaged.no_geometry"))?;
    notes.add("ccskp.note.scenes", scenes);

    // Tag 0 = Layer0 ("Untagged"), always first.
    match layers.iter().position(|l| l.name == "Layer0") {
        None => {
            layers.insert(0, LayerRaw { name: "Layer0".to_string(), visible: true, mat_name: Some("Layer_Layer0".to_string()) });
            for v in layer_ids.values_mut() {
                *v = v.saturating_add(1);
            }
        }
        Some(0) => {}
        Some(l0) => {
            let l = layers.remove(l0);
            layers.insert(0, l);
            for v in layer_ids.values_mut() {
                *v = if *v == l0 {
                    0
                } else if *v < l0 {
                    v.saturating_add(1)
                } else {
                    *v
                };
            }
        }
    }

    // Definitions: index 0 = the model, then each 7C15 in file order. Their headers are read first (cheap), the
    // entity lists in parallel, then everything is merged in the JS's order (errors, notes, the vertex cap).
    let ctx = Ctx { tlv, mat_ids: &mat_ids, layer_ids: &layer_ids };
    let heads = def_heads(&tlv, &def_records);
    // Which entity lists to read: the root, then every definition that got that far without an error.
    let mut jobs: Vec<(usize, usize)> = vec![root_record];
    let mut job_of: Vec<Option<usize>> = Vec::with_capacity(heads.len());
    for h in &heads {
        match h {
            Err(_) => break,
            Ok(DefHead { ents: Some(r), id: Some(_), .. }) => {
                job_of.push(Some(jobs.len()));
                jobs.push(*r);
            }
            Ok(_) => job_of.push(None),
        }
    }
    let mut results = read_lists(&ctx, &jobs, limits);

    let mut vertices = 0u64;
    let mut merge = |res: Option<EntResult>, notes: &mut Notes| -> Res<Got> {
        let res = res.ok_or_else(|| damaged("ccskp.damaged.no_geometry"))?;
        if vertices.saturating_add(res.verts) > limits.vertices {
            return Err(points_error(limits));
        }
        vertices = vertices.saturating_add(res.verts);
        let got = res.got?;
        notes.merge(res.notes);
        Ok(got)
    };
    let mut taken = results.iter_mut().map(Option::take);
    let root_got = merge(taken.next().flatten(), notes)?;
    let mut defs: Vec<(DefOut, Vec<RawInst>)> = Vec::with_capacity(heads.len().saturating_add(1));
    defs.push((
        DefOut {
            def: Def {
                name: "Model".to_string(),
                kind: "model",
                guid: None,
                faces: root_got.faces,
                edges: root_got.edges,
                soft: root_got.soft,
                insts: Vec::new(),
            },
            extra: root_got.extra,
        },
        root_got.insts,
    ));
    let mut def_id_to_index: FxMap<u64, usize> = FxMap::default();
    let mut job_results: Vec<Option<EntResult>> = taken.collect();
    for (i, h) in heads.into_iter().enumerate() {
        let h = h?;
        let Some(_) = h.ents else {
            notes.add("ccskp.note.def_no_geometry", 1);
            continue;
        };
        let Some(id) = h.id else {
            notes.add("ccskp.note.def_no_id", 1);
            continue;
        };
        if def_id_to_index.contains_key(&id) {
            return Err(damaged("ccskp.damaged.def_id_twice"));
        }
        def_id_to_index.insert(id, defs.len());
        let job = job_of.get(i).copied().flatten().and_then(|j| j.checked_sub(1));
        let res = job.and_then(|j| job_results.get_mut(j)).and_then(Option::take);
        let got = merge(res, notes)?;
        defs.push((
            DefOut {
                def: Def {
                    name: h.name,
                    kind: h.kind,
                    guid: h.guid,
                    faces: got.faces,
                    edges: got.edges,
                    soft: got.soft,
                    insts: Vec::new(),
                },
                extra: got.extra,
            },
            got.insts,
        ));
    }

    // Instances point at definition ids; resolve them now every definition is known.
    let mut out: Vec<DefOut> = Vec::with_capacity(defs.len());
    for (mut d, raw) in defs {
        for inst in raw {
            let Some(&di) = def_id_to_index.get(&inst.def_id) else {
                notes.add("ccskp.note.inst_missing_def", 1);
                continue;
            };
            d.def.insts.push(Inst { def: di, m: inst.m, name: inst.name, layer: inst.layer, hidden: inst.hidden, material: inst.material });
        }
        out.push(d);
    }
    check_nesting(&out, limits)?;
    Ok(ModelDat { materials, layers, defs: out })
}

fn points_error(limits: &Limits) -> SkpError {
    err("too_big", "ccskp.too_big.points", &[("max", &thousands(limits.vertices))])
}

/// Instances must not contain themselves; nesting ≤ LIMITS.depth. Iterative (no recursion a file can drive).
fn check_nesting(defs: &[DefOut], limits: &Limits) -> Res<()> {
    const UNKNOWN: i64 = -1;
    const ON_STACK: i64 = -2;
    let depth_limit = i64::try_from(limits.depth).unwrap_or(i64::MAX);
    let mut depth = vec![UNKNOWN; defs.len()];
    let insts_of = |d: usize| defs.get(d).map_or(&[][..], |x| x.def.insts.as_slice());
    let get = |depth: &Vec<i64>, i: usize| depth.get(i).copied().unwrap_or(UNKNOWN);
    for start in 0..defs.len() {
        if get(&depth, start) >= 0 {
            continue;
        }
        let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
        if let Some(x) = depth.get_mut(start) {
            *x = ON_STACK;
        }
        while let Some(&(d, i)) = stack.last() {
            let insts = insts_of(d);
            if let Some(inst) = insts.get(i) {
                if let Some(top) = stack.last_mut() {
                    top.1 = i.saturating_add(1);
                }
                let c = inst.def;
                let dc = get(&depth, c);
                if dc == ON_STACK {
                    return Err(damaged("ccskp.damaged.self_nested"));
                }
                if dc == UNKNOWN {
                    if stack.len() >= limits.depth {
                        return Err(nesting_error(limits));
                    }
                    if let Some(x) = depth.get_mut(c) {
                        *x = ON_STACK;
                    }
                    stack.push((c, 0));
                }
            } else {
                let mut m = 0i64;
                for inst in insts {
                    m = m.max(get(&depth, inst.def).saturating_add(1));
                }
                if m > depth_limit {
                    return Err(nesting_error(limits));
                }
                if let Some(x) = depth.get_mut(d) {
                    *x = m;
                }
                stack.pop();
            }
        }
    }
    Ok(())
}

fn nesting_error(limits: &Limits) -> SkpError {
    err("too_big", "ccskp.too_big.nesting", &[("levels", &limits.depth.to_string())])
}

// ---------------------------------------------------------------- entity lists

struct Ctx<'a> {
    tlv: Tlv<'a>,
    mat_ids: &'a FxMap<u64, usize>,
    layer_ids: &'a FxMap<u64, usize>,
}

struct RawInst {
    def_id: u64,
    m: [f64; 16],
    name: String,
    layer: Option<usize>,
    hidden: bool,
    material: Option<usize>,
}

#[derive(Default)]
struct Got {
    faces: Vec<Face>,
    extra: Vec<FaceExtra>,
    edges: Vec<Edge>,
    soft: Vec<[P3; 2]>,
    insts: Vec<RawInst>,
}

/// One entity list read on its own: the result, the vertices it counted (up to its error, if any) and its notes.
struct EntResult {
    got: Res<Got>,
    verts: u64,
    notes: Notes,
}

fn read_lists(ctx: &Ctx<'_>, jobs: &[(usize, usize)], limits: &Limits) -> Vec<Option<EntResult>> {
    let one = |&(s, e): &(usize, usize)| Some(read_entities(ctx, s, e, limits));
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        if jobs.len() > 1 {
            // Largest lists first, so one big list does not start last.
            let mut order: Vec<usize> = (0..jobs.len()).collect();
            order.sort_by_key(|&i| std::cmp::Reverse(jobs.get(i).map_or(0, |j| j.1.saturating_sub(j.0))));
            let mut done: Vec<(usize, Option<EntResult>)> =
                order.par_iter().with_max_len(1).map(|&i| (i, jobs.get(i).and_then(one))).collect();
            done.sort_by_key(|x| x.0);
            return done.into_iter().map(|x| x.1).collect();
        }
    }
    jobs.iter().map(one).collect()
}

struct Info {
    id: Option<u64>,
    mat: Option<usize>,
    layer: Option<usize>,
    flags: u8,
    box_: Option<(usize, usize)>,
}

const NO_INFO: Info = Info { id: None, mat: None, layer: Some(0), flags: 0, box_: None };

/// The D007 record of an entity → { id, mat, layer, flags, box }.
fn read_info(ctx: &Ctx<'_>, notes: &mut Notes, s: usize, e: usize) -> Res<Info> {
    let tlv = &ctx.tlv;
    let mut out = NO_INFO;
    for r in tlv.each(s, e) {
        let (t, a, z) = r?;
        match t {
            IDBOX => {
                if let Some((x, y)) = tlv.find(a, z, ID, true)? {
                    out.id = Some(tlv.uint(x, y)?);
                }
                out.box_ = Some((a, z));
            }
            MAT_REF if z > a => match ctx.mat_ids.get(&tlv.uint(a, z)?) {
                None => notes.add("ccskp.note.material_ref", 1),
                Some(&m) => out.mat = Some(m),
            },
            LAYER_REF if z > a => match ctx.layer_ids.get(&tlv.uint(a, z)?) {
                None => {
                    notes.add("ccskp.note.tag_ref", 1);
                    out.layer = None;
                }
                Some(&l) => out.layer = Some(l),
            },
            FLAGS if z > a => out.flags = tlv.byte(a),
            _ => {}
        }
    }
    Ok(out)
}

/// The front texture placement of a face: DC05 > DD05 > B136 > B236 > 1027 > 1127 > 1327 > 1527 (9 × f64).
fn read_placement(tlv: &Tlv<'_>, bx: (usize, usize)) -> Option<[f64; 9]> {
    let mut r = bx;
    for tag in [ATTRS, 0xb136, 0xb236, 0x1027, 0x1127, 0x1327, 0x1527] {
        r = tlv.find(r.0, r.1, tag, true).ok().flatten()?;
    }
    if r.1.saturating_sub(r.0) != 72 {
        return None;
    }
    let mut m = [0.0; 9];
    for (i, v) in m.iter_mut().enumerate() {
        let x = tlv.f64(r.0.saturating_add(i.saturating_mul(8)));
        if !x.is_finite() {
            return None;
        }
        *v = x;
    }
    Some(m)
}

struct RawFace {
    info: Option<Info>,
    normal: Option<P3>,
    /// this face's loops: a range of `Coedges::loops`
    loops: (usize, usize),
    back: Option<usize>,
}

/// Every face's coedges in one buffer (no allocation per loop): `loops[i]` = a range of `co`.
#[derive(Default)]
struct Coedges {
    co: Vec<(u64, bool)>,
    loops: Vec<(usize, usize)>,
}

/// An edge's end ids and their positions (None = no such vertex).
struct EdgeEnds {
    a: u64,
    b: u64,
    pa: Option<P3>,
    pb: Option<P3>,
}

fn edge_ends(edges: &[(u64, RawEdge)], verts: &FxMap<u64, P3>) -> Vec<EdgeEnds> {
    let one = |(_, ed): &(u64, RawEdge)| EdgeEnds { a: ed.a, b: ed.b, pa: verts.get(&ed.a).copied(), pb: verts.get(&ed.b).copied() };
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        if edges.len() > 16_384 {
            return edges.par_iter().with_min_len(4096).map(one).collect();
        }
    }
    edges.iter().map(one).collect()
}

/// What face assembly reads (shared, read-only, so chunks of faces can be assembled in parallel).
struct Assembly<'a> {
    edge_ix: &'a FxMap<u64, usize>,
    ends: &'a [EdgeEnds],
    coedges: &'a Coedges,
    tlv: &'a Tlv<'a>,
}

/// Faces assembled from one run of raw faces, in order.
#[derive(Default)]
struct Assembled {
    faces: Vec<Face>,
    extra: Vec<FaceExtra>,
    broken: u64,
    flipped: u64,
    /// edge indexes used by the faces that closed
    used: Vec<usize>,
}

const FACE_CHUNK: usize = 2048;

fn assemble_all(asm: &Assembly<'_>, raw: Vec<RawFace>) -> Vec<Assembled> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        if raw.len() > FACE_CHUNK {
            return raw.par_chunks(FACE_CHUNK).map(|c| assemble(asm, c)).collect();
        }
    }
    vec![assemble(asm, &raw)]
}

/// Coedges → vertex loops. A coedge runs from its edge's start (A20F 0) or end (1).
fn assemble(asm: &Assembly<'_>, raw: &[RawFace]) -> Assembled {
    let mut out = Assembled { faces: Vec::with_capacity(raw.len()), extra: Vec::with_capacity(raw.len()), ..Assembled::default() };
    let mut idx: Vec<usize> = Vec::new(); // edge index of each coedge of the face being assembled
    for f in raw {
        let face_loops = asm.coedges.loops.get(f.loops.0..f.loops.1).unwrap_or(&[]);
        let mut loops: Vec<Vec<P3>> = Vec::with_capacity(face_loops.len());
        let mut ok = !face_loops.is_empty();
        idx.clear();
        for &(cs, ce) in face_loops {
            let co = asm.coedges.co.get(cs..ce).unwrap_or(&[]);
            let mut lp: Vec<P3> = Vec::with_capacity(co.len());
            let mut prev_end: Option<u64> = None;
            let first_idx = idx.len();
            for &(eid, rev) in co {
                let Some((i, en)) = asm.edge_ix.get(&eid).and_then(|&i| asm.ends.get(i).map(|en| (i, en))) else {
                    ok = false;
                    break;
                };
                let (from, to, pfrom) = if rev { (en.b, en.a, en.pb) } else { (en.a, en.b, en.pa) };
                if prev_end.is_some_and(|p| p != from) {
                    ok = false;
                    break;
                }
                let Some(v) = pfrom else {
                    ok = false;
                    break;
                };
                lp.push(v);
                idx.push(i);
                prev_end = Some(to);
            }
            if !ok {
                break;
            }
            let closes = match (co.first(), idx.get(first_idx).and_then(|&i| asm.ends.get(i))) {
                (Some(&(_, r0)), Some(en)) => Some(if r0 { en.b } else { en.a }) == prev_end,
                _ => false,
            };
            if lp.len() < 3 || !closes {
                ok = false;
                break;
            }
            loops.push(lp);
        }
        if !ok {
            out.broken = out.broken.saturating_add(1);
            continue;
        }
        out.used.extend_from_slice(&idx);
        // The outer loop is the largest; SketchUp writes it first (kept as a check). Front = counter-clockwise about the normal.
        let n = match f.normal {
            Some(n) if (n[0] + n[1] + n[2]).is_finite() => n,
            _ => newell(loops.first().map_or(&[][..], Vec::as_slice)),
        };
        let mut nl = hypot(&n);
        if nl == 0.0 || nl.is_nan() {
            nl = 1.0;
        }
        let unit = [n[0] / nl, n[1] / nl, n[2] / nl];
        let area = |l: &[P3]| {
            let w = newell(l);
            (w[0] * unit[0] + w[1] * unit[1] + w[2] * unit[2]) / 2.0
        };
        // each loop's area as it stands (recomputed only for loops that get reversed)
        let mut areas: Vec<f64> = loops.iter().map(|l| area(l)).collect();
        let mut outer = 0usize;
        for i in 1..areas.len() {
            if areas.get(i).copied().unwrap_or(0.0).abs() > areas.get(outer).copied().unwrap_or(0.0).abs() {
                outer = i;
            }
        }
        if outer != 0 {
            let o = loops.remove(outer);
            loops.insert(0, o);
            let a = areas.remove(outer);
            areas.insert(0, a);
        }
        if areas.first().is_some_and(|&a| a < 0.0) {
            for l in &mut loops {
                l.reverse();
            }
            for (a, l) in areas.iter_mut().zip(&loops).skip(1) {
                *a = area(l);
            }
            out.flipped = out.flipped.saturating_add(1);
        }
        for (l, a) in loops.iter_mut().zip(&areas).skip(1) {
            if *a > 0.0 {
                l.reverse();
            }
        }
        let info = f.info.as_ref().unwrap_or(&NO_INFO);
        out.extra.push(FaceExtra { n: unit, place: info.box_.and_then(|b| read_placement(asm.tlv, b)) });
        out.faces.push(Face {
            loops,
            front: info.mat,
            back: f.back,
            layer: info.layer,
            uv: None,
            hidden: Some(info.flags & FLAG_HIDDEN != 0),
        });
    }
    out
}

/// One run of a record's children parsed on its own: the items in order, the notes, the first error (after which
/// nothing more was parsed).
struct ChildPart<T, S> {
    items: Vec<T>,
    /// what the run's parser shares across its items (faces: their coedges)
    state: S,
    notes: Notes,
    err: Res<()>,
}

/// Records bigger than this parse their children in parallel runs (native builds).
const PAR_RECORD: usize = 256 * 1024;

/// Parse the children of [a, z) whose tag is `want` with `f`, in order; big records in parallel runs, merged by the
/// caller in order, so errors and notes come out exactly as one sequential walk makes them.
fn children_parsed<T: Send, S: Default + Send>(
    tlv: &Tlv<'_>,
    a: usize,
    z: usize,
    want: u16,
    f: impl Fn(&mut S, &mut Notes, usize, usize) -> Res<T> + Sync,
) -> Vec<ChildPart<T, S>> {
    let run = |kids: &[(u16, usize, usize)], tail: Option<SkpError>| {
        let mut part = ChildPart { items: Vec::new(), state: S::default(), notes: Notes::default(), err: Ok(()) };
        for &(t, p, q) in kids {
            if t != want {
                continue;
            }
            match f(&mut part.state, &mut part.notes, p, q) {
                Ok(x) => part.items.push(x),
                Err(e) => {
                    part.err = Err(e);
                    return part;
                }
            }
        }
        if let Some(e) = tail {
            part.err = Err(e);
        }
        part
    };
    // the children's headers (a strict walk; a broken header ends the list with its error)
    let mut kids = Vec::new();
    let mut tail = None;
    for r in tlv.each(a, z) {
        match r {
            Ok(k) => kids.push(k),
            Err(e) => {
                tail = Some(e);
                break;
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        if z.saturating_sub(a) > PAR_RECORD && kids.len() > 64 {
            let n = rayon::current_num_threads().clamp(1, 64).saturating_mul(2);
            let size = kids.len().div_ceil(n).max(16);
            let chunks: Vec<&[(u16, usize, usize)]> = kids.chunks(size).collect();
            let last = chunks.len().saturating_sub(1);
            let tail = &tail;
            return chunks.into_par_iter().enumerate().map(|(i, c)| run(c, if i == last { tail.clone() } else { None })).collect();
        }
    }
    vec![run(&kids, tail)]
}

/// One edge record (B80B) → (id, edge), or None (noted) when its ends or id are missing.
fn parse_edge(ctx: &Ctx<'_>, notes: &mut Notes, p: usize, q: usize) -> Res<Option<(u64, RawEdge)>> {
    let tlv = &ctx.tlv;
    let (mut info, mut va, mut vb) = (None, None, None);
    for r3 in tlv.each(p, q) {
        let (t3, x, y) = r3?;
        match t3 {
            INFO => info = Some(read_info(ctx, notes, x, y)?),
            EDGE_A if y > x => va = Some(tlv.uint(x, y)?),
            EDGE_B if y > x => vb = Some(tlv.uint(x, y)?),
            _ => {}
        }
    }
    let (Some(info), Some(va), Some(vb)) = (info, va, vb) else {
        notes.add("ccskp.note.edge_no_ends", 1);
        return Ok(None);
    };
    let Some(id) = info.id else {
        notes.add("ccskp.note.edge_no_ends", 1);
        return Ok(None);
    };
    Ok(Some((id, RawEdge { a: va, b: vb, layer: info.layer, flags: info.flags })))
}

/// One face record (AC0D) → its raw fields; its coedges go into `coedges`.
fn parse_face(ctx: &Ctx<'_>, notes: &mut Notes, coedges: &mut Coedges, p: usize, q: usize) -> Res<RawFace> {
    let tlv = &ctx.tlv;
    let first_loop = coedges.loops.len();
    let mut face = RawFace { info: None, normal: None, loops: (first_loop, first_loop), back: None };
    for r3 in tlv.each(p, q) {
        let (t3, x, y) = r3?;
        match t3 {
            INFO => face.info = Some(read_info(ctx, notes, x, y)?),
            NORMAL if y.saturating_sub(x) >= 24 => {
                face.normal = Some([tlv.f64(x), tlv.f64(x.saturating_add(8)), tlv.f64(x.saturating_add(16))]);
            }
            BACK_MAT if y > x => match ctx.mat_ids.get(&tlv.uint(x, y)?) {
                None => notes.add("ccskp.note.material_ref", 1),
                Some(&m) => face.back = Some(m),
            },
            LOOPS => {
                for r4 in tlv.each(x, y) {
                    let (t4, u, w) = r4?;
                    if t4 != LOOP {
                        continue;
                    }
                    let co_start = coedges.co.len();
                    for r5 in tlv.each(u, w) {
                        let (t5, u2, w2) = r5?;
                        if t5 != COEDGES {
                            continue;
                        }
                        for r6 in tlv.each(u2, w2) {
                            let (t6, u3, w3) = r6?;
                            if t6 != COEDGE {
                                continue;
                            }
                            let (mut edge, mut rev) = (None, None);
                            for r7 in tlv.each(u3, w3) {
                                let (t7, u4, w4) = r7?;
                                if t7 == CO_EDGE && w4 > u4 {
                                    edge = Some(tlv.uint(u4, w4)?);
                                } else if t7 == CO_DIR && w4 > u4 {
                                    rev = Some(tlv.uint(u4, w4)? != 0);
                                }
                            }
                            let (Some(edge), Some(rev)) = (edge, rev) else {
                                return Err(damaged("ccskp.damaged.face_edge"));
                            };
                            coedges.co.push((edge, rev));
                        }
                    }
                    coedges.loops.push((co_start, coedges.co.len()));
                }
            }
            _ => {}
        }
    }
    face.loops.1 = coedges.loops.len();
    Ok(face)
}

struct RawEdge {
    a: u64,
    b: u64,
    layer: Option<usize>,
    flags: u8,
}

/// One entity list (8813) → faces, loose edges, soft edges, instances (definition ids unresolved).
fn read_entities(ctx: &Ctx<'_>, s: usize, e: usize, limits: &Limits) -> EntResult {
    let mut notes = Notes::default();
    let mut verts_n = 0u64;
    let got = read_entities_inner(ctx, s, e, limits, &mut notes, &mut verts_n);
    EntResult { got, verts: verts_n, notes }
}

fn read_instance(ctx: &Ctx<'_>, notes: &mut Notes, a: usize, z: usize, insts: &mut Vec<RawInst>) -> Res<()> {
    let tlv = &ctx.tlv;
    let (mut info, mut name, mut m13, mut def_id) = (None, String::new(), None::<[f64; 13]>, None);
    for r in tlv.each(a, z) {
        let (t, p, q) = r?;
        match t {
            INFO => info = Some(read_info(ctx, notes, p, q)?),
            INST_NAME => name = tlv.text(p, q),
            INST_M => {
                if q.saturating_sub(p) < 104 {
                    return Err(damaged("ccskp.damaged.placement_short"));
                }
                let mut m = [0.0; 13];
                for (i, v) in m.iter_mut().enumerate() {
                    *v = tlv.f64(p.saturating_add(i.saturating_mul(8)));
                }
                m13 = Some(m);
            }
            INST_DEF if q > p => def_id = Some(tlv.uint(p, q)?),
            _ => {}
        }
    }
    let Some(def_id) = def_id else {
        notes.add("ccskp.note.inst_no_def", 1);
        return Ok(());
    };
    let mut m = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    if let Some(m13) = m13 {
        let w = m13[12];
        if !(w.is_finite() && w.abs() > 1e-12) {
            return Err(damaged("ccskp.damaged.placement_scale"));
        }
        let k = 1.0 / w;
        if m13.iter().take(12).any(|v| !ok_num(v * k)) {
            return Err(damaged("ccskp.damaged.placement_numbers"));
        }
        // rows r0 = m13[0..2], r1 = m13[3..5], r2 = m13[6..8]; column-major out: column j = (r0[j], r1[j], r2[j]).
        m = [
            m13[0] * k,
            m13[3] * k,
            m13[6] * k,
            0.0,
            m13[1] * k,
            m13[4] * k,
            m13[7] * k,
            0.0,
            m13[2] * k,
            m13[5] * k,
            m13[8] * k,
            0.0,
            m13[9] * k,
            m13[10] * k,
            m13[11] * k,
            1.0,
        ];
    } else {
        notes.add("ccskp.note.inst_no_place", 1);
    }
    let (layer, hidden, material) = match &info {
        Some(i) => (i.layer, i.flags & FLAG_HIDDEN != 0, i.mat),
        None => (Some(0), false, None),
    };
    insts.push(RawInst { def_id, m, name, layer, hidden, material });
    Ok(())
}

fn read_entities_inner(ctx: &Ctx<'_>, s: usize, e: usize, limits: &Limits, notes: &mut Notes, verts_n: &mut u64) -> Res<Got> {
    let tlv = &ctx.tlv;
    let mut verts: FxMap<u64, P3> = FxMap::default();
    let mut edges: Vec<(u64, RawEdge)> = Vec::new(); // insertion order of first id, latest value
    let mut edge_ix: FxMap<u64, usize> = FxMap::default();
    let mut raw_faces: Vec<RawFace> = Vec::new();
    let mut coedges = Coedges::default();
    let mut insts: Vec<RawInst> = Vec::new();
    let mut unknown: Vec<(u16, u64)> = Vec::new();

    for r in tlv.each(s, e) {
        let (t, a, z) = r?;
        match t {
            VERTS => {
                verts.reserve(z.saturating_sub(a) / 48); // a vertex record is 47 bytes when its id has 1 byte
                for r2 in tlv.each(a, z) {
                    let (t2, p, q) = r2?;
                    if t2 != VERT {
                        continue;
                    }
                    let id = tlv.id_of(p, q)?;
                    let xyz = tlv.find(p, q, XYZ, false)?;
                    let (Some(id), Some(xyz)) = (id, xyz.filter(|x| x.1.saturating_sub(x.0) >= 24)) else {
                        return Err(damaged("ccskp.damaged.vertex_position"));
                    };
                    let v = [tlv.f64(xyz.0), tlv.f64(xyz.0.saturating_add(8)), tlv.f64(xyz.0.saturating_add(16))];
                    if !ok_num(v[0]) || !ok_num(v[1]) || !ok_num(v[2]) {
                        return Err(damaged("ccskp.damaged.vertex_coords"));
                    }
                    if verts_n.saturating_add(1) > limits.vertices {
                        *verts_n = verts_n.saturating_add(1);
                        return Err(points_error(limits));
                    }
                    *verts_n = verts_n.saturating_add(1);
                    verts.insert(id, v);
                }
            }
            EDGES => {
                for part in children_parsed(tlv, a, z, EDGE, |_: &mut (), n, p, q| parse_edge(ctx, n, p, q)) {
                    notes.merge(part.notes);
                    edge_ix.reserve(part.items.len());
                    edges.reserve(part.items.len());
                    for (id, ed) in part.items.into_iter().flatten() {
                        match edge_ix.get(&id) {
                            Some(&i) => {
                                if let Some(slot) = edges.get_mut(i) {
                                    slot.1 = ed;
                                }
                            }
                            None => {
                                edge_ix.insert(id, edges.len());
                                edges.push((id, ed));
                            }
                        }
                    }
                    part.err?;
                }
            }
            FACES => {
                for part in children_parsed(tlv, a, z, FACE, |co: &mut Coedges, n, p, q| parse_face(ctx, n, co, p, q)) {
                    notes.merge(part.notes);
                    let (lo, cb) = (coedges.loops.len(), coedges.co.len());
                    if lo == 0 && cb == 0 {
                        coedges = part.state;
                    } else {
                        coedges.co.extend_from_slice(&part.state.co);
                        coedges.loops.extend(part.state.loops.iter().map(|&(x, y)| (x.saturating_add(cb), y.saturating_add(cb))));
                    }
                    for mut f in part.items {
                        f.loops = (f.loops.0.saturating_add(lo), f.loops.1.saturating_add(lo));
                        raw_faces.push(f);
                    }
                    part.err?;
                }
            }
            COMPS => {
                for r2 in tlv.each(a, z) {
                    let (t2, p, q) = r2?;
                    if t2 == INST {
                        read_instance(ctx, notes, p, q, &mut insts)?;
                    }
                }
            }
            GROUPS | IMAGES => {
                let child = if t == GROUPS { GROUP } else { IMAGE };
                for r2 in tlv.each(a, z) {
                    let (t2, p, q) = r2?;
                    if t2 != child {
                        continue;
                    }
                    for r3 in tlv.each(p, q) {
                        let (t3, x, y) = r3?;
                        if t3 == INST {
                            read_instance(ctx, notes, x, y, &mut insts)?;
                        }
                    }
                }
            }
            GUIDE_LINES | GUIDE_POINTS => {
                let n = tlv.each_loose(a, z).count() as u64;
                notes.add(if t == GUIDE_LINES { "ccskp.note.guide_lines" } else { "ccskp.note.guide_points" }, n);
            }
            ANNOTATIONS => {
                let n = tlv.each_loose(a, z).count() as u64;
                notes.add("ccskp.note.annotations", n);
            }
            _ if !quiet(t) => match unknown.iter_mut().find(|u| u.0 == t) {
                Some(u) => u.1 = u.1.saturating_add(1),
                None => unknown.push((t, 1)),
            },
            _ => {}
        }
    }
    for (t, n) in unknown {
        notes.add_fill("ccskp.note.other_lists", n, Some(("record", format!("{t:04X}"))));
    }

    // Each edge's end points, looked up once (the JS looks them up per coedge; same values).
    let ends = edge_ends(&edges, &verts);
    drop(verts);
    let asm = Assembly { edge_ix: &edge_ix, ends: &ends, coedges: &coedges, tlv };
    let parts = assemble_all(&asm, raw_faces);
    let mut used = vec![false; ends.len()];
    let (mut broken, mut flipped) = (0u64, 0u64);
    let mut faces = Vec::new();
    let mut extra = Vec::new();
    for part in parts {
        broken = broken.saturating_add(part.broken);
        flipped = flipped.saturating_add(part.flipped);
        for i in part.used {
            if let Some(u) = used.get_mut(i) {
                *u = true;
            }
        }
        if faces.is_empty() {
            faces = part.faces;
            extra = part.extra;
        } else {
            faces.extend(part.faces);
            extra.extend(part.extra);
        }
    }
    notes.add("ccskp.note.face_open", broken);
    notes.add("ccskp.note.face_turned", flipped);

    let mut loose = Vec::new();
    let mut soft = Vec::new();
    let mut missing = 0u64;
    for (i, ((_, ed), en)) in edges.iter().zip(&ends).enumerate() {
        let (Some(a), Some(b)) = (en.pa, en.pb) else {
            missing = missing.saturating_add(1);
            continue;
        };
        if used.get(i).copied().unwrap_or(false) {
            if ed.flags & (FLAG_SOFT | FLAG_SMOOTH) != 0 {
                soft.push([a, b]);
            }
        } else {
            loose.push(Edge {
                a,
                b,
                layer: ed.layer,
                soft: ed.flags & FLAG_SOFT != 0,
                smooth: ed.flags & FLAG_SMOOTH != 0,
                hidden: ed.flags & FLAG_HIDDEN != 0,
            });
        }
    }
    notes.add("ccskp.note.edge_missing_end", missing);
    Ok(Got { faces, extra, edges: loose, soft, insts })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_helpers() {
        let t = r#"<?xml version="1.0"?><materialDocument><mat:material name="A&amp;B &#65;&#x42;" colorRed="200" colorGreen='100' colorBlue="50" trans="0.25" useTrans="1"><mat:texture textureFilename="C:\x\b.png" xScale="10" yScale="20"/></mat:material></materialDocument>"#;
        let m = read_material_xml(t).unwrap();
        assert_eq!(m.name.as_deref(), Some("A&B AB"));
        assert_eq!(m.color, Some([200, 100, 50]));
        assert!((m.alpha - 0.75).abs() < 1e-15);
        let tx = m.texture.unwrap();
        assert_eq!(tx.file, "C:\\x\\b.png");
        assert_eq!(tx.width, Some(10.0));
        assert_eq!(last_segment(&tx.file), "b.png");
        assert!(xml_element("<materialDocument>", "material").is_none());
        assert_eq!(attributes(r#" 1a="x" b = 'y' c="z"#), vec![("a".to_string(), "x".to_string()), ("b".to_string(), "y".to_string())]);
        assert_eq!(unxml("&#12a; &bogus; &#x41;"), "&#12a; &bogus; A");
        assert_eq!(join_path("materials/x", "../../../a"), None);
        assert_eq!(join_path("materials/x", ".\\a.png").as_deref(), Some("materials/x/a.png"));
        assert_eq!(material_xml_folder("Materials/Brick/Material.XML"), Some("Brick"));
        assert_eq!(material_xml_folder("materials/a/b/material.xml"), None);
        assert!(is_thumbnail_name("materials/_/thumbnail.jpg"));
        assert!(!is_thumbnail_name("materials/_/thumbnail.j1"));
        assert_eq!(unit_word("Millimeter"), Some("mm"));
        assert_eq!(unit_word("Metres"), Some("m"));
        assert_eq!(guid_text(&[0x1b; 16]).unwrap().len(), 36);
    }
}
