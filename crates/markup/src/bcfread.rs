//! bcf-read.js — BCF import (0.4 K7): `unzipAll`, `readBcf` (BCF 2.0 / 2.1 / 3.0 topics, comments, cameras;
//! untrusted: sizes and counts capped, text trimmed) and `planImport` (what an import changes: replies and statuses
//! onto our own markups, notes for new topics).
//!
//! The JS reads the XML with regular expressions; each one is matched by hand here in the order its backtracking
//! engine tries things (the comments at each matcher say which expression it is).

use crate::js::{parse_date, slice16, to_iso_s, trim};
use crate::model::{Markup, STATUSES};
use crate::slip::num_value;
use serde_json::{Map, Value, json};

const MAX_FILES: usize = 5000;
const MAX_TOPICS: usize = 2000;
const MAX_XML: usize = 2 * 1024 * 1024;
/// Most bytes one entry may inflate to: anything over [`MAX_XML`] is never read as text, so a bigger entry is only
/// marked too big (the JS inflates it whole).
const MAX_INFLATE: usize = MAX_XML + 1;

/// One entry of [`unzip_all`].
#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    Bytes(Vec<u8>),
    /// larger than any text the reader looks at
    TooBig,
    /// unreadable (another method, or bad deflate data)
    Null,
}

/// `unzipAll(bytes)` → entries by name in the JS Map's order; Err("zip") when it is not a zip.
pub fn unzip_all(bytes: &[u8]) -> Result<Vec<(String, Entry)>, &'static str> {
    let rd16 = |p: usize| -> usize {
        usize::from(u16::from_le_bytes([bytes.get(p).copied().unwrap_or(0), bytes.get(p.saturating_add(1)).copied().unwrap_or(0)]))
    };
    let rd32 = |p: usize| -> usize {
        let b = |k: usize| bytes.get(p.saturating_add(k)).copied().unwrap_or(0);
        usize::try_from(u32::from_le_bytes([b(0), b(1), b(2), b(3)])).unwrap_or(usize::MAX)
    };
    let len = bytes.len();
    let mut e = len.checked_sub(22).ok_or("zip")?;
    loop {
        if rd32(e) == 0x0605_4b50 {
            break;
        }
        e = e.checked_sub(1).ok_or("zip")?;
    }
    let n = rd16(e.saturating_add(10)).min(MAX_FILES);
    let mut p = rd32(e.saturating_add(16));
    let mut out: Vec<(String, Entry)> = Vec::new();
    for _ in 0..n {
        if p.saturating_add(46) > len {
            break;
        }
        if rd32(p) != 0x0201_4b50 {
            break;
        }
        let method = rd16(p.saturating_add(10));
        let csize = rd32(p.saturating_add(20));
        let name_len = rd16(p.saturating_add(28));
        let extra = rd16(p.saturating_add(30));
        let comment = rd16(p.saturating_add(32));
        let local = rd32(p.saturating_add(42));
        let ns = p.saturating_add(46);
        let name = String::from_utf8_lossy(bytes.get(ns.min(len)..ns.saturating_add(name_len).min(len)).unwrap_or(&[])).into_owned();
        p = p.saturating_add(46).saturating_add(name_len).saturating_add(extra).saturating_add(comment);
        if local.saturating_add(30) > len {
            continue;
        }
        let start = local.saturating_add(30).saturating_add(rd16(local.saturating_add(26))).saturating_add(rd16(local.saturating_add(28)));
        let raw = bytes.get(start.min(len)..start.saturating_add(csize).min(len)).unwrap_or(&[]);
        if name.ends_with('/') {
            continue;
        }
        let entry = match method {
            0 if raw.len() > MAX_XML => Entry::TooBig,
            0 => Entry::Bytes(raw.to_vec()),
            8 => match miniz_oxide::inflate::decompress_to_vec_with_limit(raw, MAX_INFLATE) {
                Ok(d) => Entry::Bytes(d),
                Err(e) if matches!(e.status, miniz_oxide::inflate::TINFLStatus::HasMoreOutput) => Entry::TooBig,
                Err(_) => Entry::Null,
            },
            _ => Entry::Null,
        };
        match out.iter_mut().find(|(k, _)| *k == name) {
            Some(slot) => slot.1 = entry,
            None => out.push((name, entry)),
        }
    }
    Ok(out)
}

// ---- the small XML reader ----

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `unxml(s)`: CDATA kept as text, the five entities and numeric references decoded. Err = the RangeError the JS
/// throws for a reference past U+10FFFF.
pub fn unxml(s: &str) -> Result<String, String> {
    // <!\[CDATA\[([\s\S]*?)\]\]> → its text with < > & escaped (decoded again below)
    let mut a = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("<![CDATA[") {
        let after = rest.get(i.saturating_add(9)..).unwrap_or("");
        let Some(j) = after.find("]]>") else { break };
        a.push_str(rest.get(..i).unwrap_or(""));
        for c in after.get(..j).unwrap_or("").chars() {
            match c {
                '<' => a.push_str("&lt;"),
                '>' => a.push_str("&gt;"),
                '&' => a.push_str("&amp;"),
                c => a.push(c),
            }
        }
        rest = after.get(j.saturating_add(3)..).unwrap_or("");
    }
    a.push_str(rest);
    // &(#x[0-9a-f]+|#\d+|lt|gt|amp|quot|apos); (case-insensitive)
    let mut out = String::with_capacity(a.len());
    let mut rest = a.as_str();
    while let Some(i) = rest.find('&') {
        out.push_str(rest.get(..i).unwrap_or(""));
        let tail = rest.get(i.saturating_add(1)..).unwrap_or("");
        let mut done = None;
        let lt = tail.to_ascii_lowercase();
        if let Some(h) = lt.strip_prefix("#x") {
            let n = h.bytes().take_while(u8::is_ascii_hexdigit).count();
            if n > 0 && h.as_bytes().get(n) == Some(&b';') {
                let v = h
                    .get(..n)
                    .map_or(f64::NAN, |d| d.bytes().fold(0.0, |acc, b| acc * 16.0 + f64::from(char::from(b).to_digit(16).unwrap_or(0))));
                done = Some((v, n.saturating_add(3)));
            }
        }
        if done.is_none()
            && let Some(d) = lt.strip_prefix('#')
        {
            let n = d.bytes().take_while(u8::is_ascii_digit).count();
            if n > 0 && d.as_bytes().get(n) == Some(&b';') {
                done = Some((crate::js::str_to_number(d.get(..n).unwrap_or("")), n.saturating_add(2)));
            }
        }
        if let Some((v, used)) = done {
            if !(0.0..=1_114_111.0).contains(&v) || v.fract() != 0.0 {
                return Err(format!("Invalid code point {}", crate::js::num_to_string(v)));
            }
            // a lone surrogate (the JS keeps it) becomes U+FFFD
            out.push(char::from_u32(crate::js::f2u(v).try_into().unwrap_or(0xfffd)).unwrap_or('\u{fffd}'));
            rest = tail.get(used..).unwrap_or("");
            continue;
        }
        let named = [("lt;", '<'), ("gt;", '>'), ("amp;", '&'), ("quot;", '"'), ("apos;", '\'')];
        if let Some((k, c)) = named.iter().find(|(k, _)| lt.starts_with(k)) {
            out.push(*c);
            rest = tail.get(k.len()..).unwrap_or("");
            continue;
        }
        out.push('&');
        rest = tail;
    }
    out.push_str(rest);
    Ok(out)
}

/// `(?:\w+:)?NAME` at `k` (after a `<` or `</`) → the end of NAME.
fn qname_at(x: &str, k: usize, name: &str) -> Option<usize> {
    let tail = x.get(k..)?;
    let w = tail.chars().take_while(|c| is_word(*c)).map(char::len_utf8).sum::<usize>();
    if w > 0 && tail.as_bytes().get(w) == Some(&b':') {
        let after = k.saturating_add(w).saturating_add(1);
        if x.get(after..).is_some_and(|t| t.starts_with(name)) {
            return Some(after.saturating_add(name.len()));
        }
    }
    if tail.starts_with(name) { Some(k.saturating_add(name.len())) } else { None }
}

/// `<(?:\w+:)?NAME(?:\s[^>]*[^/])?>([\s\S]*?)</(?:\w+:)?NAME>` from byte `from` → (match start, content start,
/// content end, match end).
fn tag_match(x: &str, name: &str, from: usize) -> Option<(usize, usize, usize, usize)> {
    let b = x.as_bytes();
    let mut i = from;
    while let Some(off) = x.get(i..).and_then(|t| t.find('<')) {
        let s = i.saturating_add(off);
        i = s.saturating_add(1);
        let Some(k) = qname_at(x, s.saturating_add(1), name) else { continue };
        let cs = match x.get(k..).and_then(|t| t.chars().next()) {
            Some('>') => Some(k.saturating_add(1)),
            Some(c) if crate::js::is_ws(c) => {
                let r = k.saturating_add(c.len_utf8());
                match x.get(r..).and_then(|t| t.find('>')) {
                    None => None,
                    Some(off) => {
                        let lim = r.saturating_add(off);
                        if b.get(lim.saturating_add(1)) == Some(&b'>') {
                            Some(lim.saturating_add(2))
                        } else if lim > r && b.get(lim.saturating_sub(1)) != Some(&b'/') {
                            Some(lim.saturating_add(1))
                        } else {
                            None
                        }
                    }
                }
            }
            _ => None,
        };
        let Some(cs) = cs else { continue };
        let mut j = cs;
        while let Some(off) = x.get(j..).and_then(|t| t.find("</")) {
            let c0 = j.saturating_add(off);
            if let Some(e) = qname_at(x, c0.saturating_add(2), name)
                && b.get(e) == Some(&b'>')
            {
                return Some((s, cs, c0, e.saturating_add(1)));
            }
            j = c0.saturating_add(1);
        }
    }
    None
}

/// `tagText(x, name)`.
pub fn tag_text(x: &str, name: &str) -> Result<String, String> {
    match tag_match(x, name, 0) {
        Some((_, cs, ce, _)) => Ok(trim(&unxml(x.get(cs..ce).unwrap_or(""))?).to_string()),
        None => Ok(String::new()),
    }
}

/// `tagAll(x, name)`.
pub fn tag_all(x: &str, name: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some((_, cs, ce, e)) = tag_match(x, name, from) {
        out.push(trim(&unxml(x.get(cs..ce).unwrap_or(""))?).to_string());
        from = e;
    }
    Ok(out)
}

/// `attr(tag, name)`: `\sNAME\s*=\s*"([^"]*)"`, else the single-quoted form.
pub fn attr(tag: &str, name: &str) -> Result<String, String> {
    for q in ['"', '\''] {
        for (i, c) in tag.char_indices() {
            if !crate::js::is_ws(c) {
                continue;
            }
            let mut k = i.saturating_add(c.len_utf8());
            if !tag.get(k..).is_some_and(|t| t.starts_with(name)) {
                continue;
            }
            k = k.saturating_add(name.len());
            let skip = |k: usize| {
                k.saturating_add(tag.get(k..).unwrap_or("").chars().take_while(|c| crate::js::is_ws(*c)).map(char::len_utf8).sum::<usize>())
            };
            k = skip(k);
            if tag.as_bytes().get(k) != Some(&b'=') {
                continue;
            }
            k = skip(k.saturating_add(1));
            if tag.get(k..).and_then(|t| t.chars().next()) != Some(q) {
                continue;
            }
            let vs = k.saturating_add(1);
            let Some(off) = tag.get(vs..).and_then(|t| t.find(q)) else { continue };
            return unxml(tag.get(vs..vs.saturating_add(off)).unwrap_or(""));
        }
    }
    Ok(String::new())
}

/// `cut(s, n)`.
fn cut(s: &str, n: usize) -> String {
    let t: String = s.chars().filter(|c| !matches!(u32::from(*c), 0..=8 | 0x0b | 0x0c | 0x0e..=0x1f)).collect();
    slice16(&t, n)
}

fn is_guid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36 && b.iter().enumerate().all(|(i, c)| if matches!(i, 8 | 13 | 18 | 23) { *c == b'-' } else { c.is_ascii_hexdigit() })
}

/// A comment of a topic.
#[derive(Debug, Clone, PartialEq)]
pub struct Comment {
    pub guid: String,
    pub date: String,
    pub author: String,
    pub text: String,
}

/// A topic read by [`read_bcf`].
#[derive(Debug, Clone, PartialEq)]
pub struct Topic {
    pub guid: String,
    pub kind: String,
    pub status: String,
    pub title: String,
    pub description: String,
    pub labels: Vec<String>,
    pub created: String,
    pub author: String,
    pub assignee: String,
    pub due: String,
    pub file: String,
    pub comments: Vec<Comment>,
    pub camera: Option<[f64; 2]>,
}

/// What [`read_bcf`] gives.
#[derive(Debug, Clone, PartialEq)]
pub struct Bcf {
    pub version: String,
    pub project: String,
    pub topics: Vec<Topic>,
    pub skipped: usize,
}

fn decode(b: &[u8]) -> String {
    let s = String::from_utf8_lossy(b);
    s.strip_prefix('\u{feff}').unwrap_or(&s).to_string()
}

/// `<Comment\s[^>]*Guid\s*=\s*["'][^"']+["'][^>]*>` from `from` → (start, end).
fn comment_start(x: &str, from: usize) -> Option<(usize, usize)> {
    let mut i = from;
    while let Some(off) = x.get(i..).and_then(|t| t.find("<Comment")) {
        let s = i.saturating_add(off);
        i = s.saturating_add(1);
        let after = s.saturating_add(8);
        let Some(c) = x.get(after..).and_then(|t| t.chars().next()) else { continue };
        if !crate::js::is_ws(c) {
            continue;
        }
        let j0 = after.saturating_add(c.len_utf8());
        let lim = x.get(j0..).and_then(|t| t.find('>')).map_or(x.len(), |o| j0.saturating_add(o));
        // [^>]* greedy: the last "Guid" in the tag that completes the expression wins
        let region = x.get(j0..lim).unwrap_or("");
        let mut cands: Vec<usize> = region.match_indices("Guid").map(|(k, _)| j0.saturating_add(k)).collect();
        cands.reverse();
        for g in cands {
            let ws = |k: usize| {
                k.saturating_add(x.get(k..).unwrap_or("").chars().take_while(|c| crate::js::is_ws(*c)).map(char::len_utf8).sum::<usize>())
            };
            let mut k = ws(g.saturating_add(4));
            if x.as_bytes().get(k) != Some(&b'=') {
                continue;
            }
            k = ws(k.saturating_add(1));
            if !matches!(x.as_bytes().get(k), Some(b'"' | b'\'')) {
                continue;
            }
            let vs = k.saturating_add(1);
            let Some(vlen) = x.get(vs..).and_then(|t| t.find(['"', '\''])) else { continue };
            if vlen == 0 {
                continue;
            }
            let qe = vs.saturating_add(vlen).saturating_add(1);
            let Some(gt) = x.get(qe..).and_then(|t| t.find('>')) else { continue };
            return Some((s, qe.saturating_add(gt).saturating_add(1)));
        }
    }
    None
}

/// `(/<(?:OrthogonalCamera|PerspectiveCamera)>[\s\S]*?<CameraViewPoint>([\s\S]*?)<\/CameraViewPoint>/.exec(vx) || [])[1]`
fn camera_point(vx: &str) -> Option<&str> {
    let a = vx.find("<OrthogonalCamera>").map(|i| (i, 18));
    let b = vx.find("<PerspectiveCamera>").map(|i| (i, 19));
    let (s, l) = match (a, b) {
        (Some(a), Some(b)) => {
            if a.0 <= b.0 {
                a
            } else {
                b
            }
        }
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => return None,
    };
    let from = s.saturating_add(l);
    let cv = vx.get(from..)?.find("<CameraViewPoint>").map(|o| from.saturating_add(o).saturating_add(17))?;
    let ce = vx.get(cv..)?.find("</CameraViewPoint>").map(|o| cv.saturating_add(o))?;
    vx.get(cv..ce)
}

/// `vpName.replace(/^.*\//, '')`: everything up to the last '/' on the first line goes.
fn base_name(s: &str) -> &str {
    let line_end = s.find(['\n', '\r', '\u{2028}', '\u{2029}']).unwrap_or(s.len());
    match s.get(..line_end).and_then(|l| l.rfind('/')) {
        Some(i) => s.get(i.saturating_add(1)..).unwrap_or(""),
        None => s,
    }
}

/// `readBcf(bytes)`; Err("zip") for a file that is not a zip, or the RangeError text of a bad character reference.
pub fn read_bcf(bytes: &[u8]) -> Result<Bcf, String> {
    let files = unzip_all(bytes).map_err(str::to_string)?;
    let text = |name: &str| -> String {
        match files.iter().find(|(k, _)| k == name).map(|(_, e)| e) {
            Some(Entry::Bytes(b)) if b.len() <= MAX_XML => decode(b),
            _ => String::new(),
        }
    };
    let vtext = text("bcf.version");
    let vtag = vtext
        .find("<Version")
        .and_then(|i| vtext.get(i..).and_then(|t| t.find('>')).map(|j| vtext.get(i..i.saturating_add(j).saturating_add(1)).unwrap_or("")));
    let version = attr(vtag.unwrap_or(""), "VersionId")?;
    let project = tag_text(&text("project.bcfp"), "Name")?;
    let mut folders: Vec<String> = Vec::new();
    for (k, _) in &files {
        if let Some((f, rest)) = k.split_once('/')
            && !f.is_empty()
            && rest.eq_ignore_ascii_case("markup.bcf")
            && !folders.iter().any(|x| x == f)
        {
            folders.push(f.to_string());
        }
    }
    let mut topics = Vec::new();
    let mut skipped = 0usize;
    for f in folders.iter().take(MAX_TOPICS) {
        let x = text(&format!("{f}/markup.bcf"));
        // /<Topic\b[^>]*>/
        let mut top_tag = "";
        let mut from = 0usize;
        while let Some(off) = x.get(from..).and_then(|t| t.find("<Topic")) {
            let s = from.saturating_add(off);
            from = s.saturating_add(1);
            let next = x.get(s.saturating_add(6)..).and_then(|t| t.chars().next());
            if next.is_some_and(is_word) {
                continue;
            }
            if let Some(gt) = x.get(s..).and_then(|t| t.find('>')) {
                top_tag = x.get(s..s.saturating_add(gt).saturating_add(1)).unwrap_or("");
                break;
            }
        }
        let g = attr(top_tag, "Guid")?;
        let guid = if g.is_empty() { f.clone() } else { g };
        if x.is_empty() || top_tag.is_empty() || !is_guid(&guid) {
            skipped = skipped.saturating_add(1);
            continue;
        }
        let topic_xml = x.find(top_tag).and_then(|i| x.get(i..)).unwrap_or("");
        let mut starts: Vec<(usize, usize)> = Vec::new();
        let mut from = 0usize;
        while let Some((s, e)) = comment_start(&x, from) {
            starts.push((s, e));
            from = e;
        }
        let mut comments = Vec::new();
        for (i, (s, e)) in starts.iter().enumerate() {
            let end = starts.get(i.saturating_add(1)).map_or(x.len(), |n| n.0);
            let block = x.get(*e..end.max(*e)).unwrap_or("");
            // /<Comment>([\s\S]*?)<\/Comment>/
            let Some(b0) = block.find("<Comment>") else { continue };
            let bs = b0.saturating_add(9);
            let Some(be) = block.get(bs..).and_then(|t| t.find("</Comment>")) else { continue };
            let body = block.get(bs..bs.saturating_add(be)).unwrap_or("");
            comments.push(Comment {
                guid: attr(x.get(*s..*e).unwrap_or(""), "Guid")?,
                date: tag_text(block, "Date")?,
                author: cut(&tag_text(block, "Author")?, 120),
                text: cut(trim(&unxml(body)?), 20000),
            });
        }
        let vp = tag_text(&x, "Viewpoint")?;
        let vp_name = if vp.is_empty() { "viewpoint.bcfv".to_string() } else { vp };
        let vx = text(&format!("{f}/{}", base_name(&vp_name)));
        let (cx, cy) = match camera_point(&vx) {
            Some(cam) => (crate::js::str_to_number(&tag_text(cam, "X")?), crate::js::str_to_number(&tag_text(cam, "Y")?)),
            None => (f64::NAN, f64::NAN),
        };
        let or_tag = |a: String, t: &str| -> Result<String, String> { if a.is_empty() { tag_text(topic_xml, t) } else { Ok(a) } };
        let mut labels: Vec<String> = tag_all(topic_xml, "Labels")?;
        labels.extend(tag_all(topic_xml, "Label")?);
        topics.push(Topic {
            guid: guid.to_lowercase(),
            kind: cut(&or_tag(attr(top_tag, "TopicType")?, "TopicType")?, 120),
            status: cut(&or_tag(attr(top_tag, "TopicStatus")?, "TopicStatus")?, 60),
            title: cut(&tag_text(topic_xml, "Title")?, 500),
            description: cut(&tag_text(topic_xml, "Description")?, 20000),
            labels: labels.iter().map(|l| cut(l, 200)).take(50).collect(),
            created: tag_text(topic_xml, "CreationDate")?,
            author: cut(&tag_text(topic_xml, "CreationAuthor")?, 120),
            assignee: cut(&tag_text(topic_xml, "AssignedTo")?, 120),
            due: slice16(&tag_text(topic_xml, "DueDate")?, 10),
            file: cut(&tag_text(&x, "Filename")?, 260),
            comments: comments.into_iter().take(1000).collect(),
            camera: if cx.is_finite() && cy.is_finite() { Some([cx, cy]) } else { None },
        });
    }
    skipped = skipped.saturating_add(folders.len().saturating_sub(MAX_TOPICS));
    Ok(Bcf { version, project, topics, skipped })
}

/// A [`Bcf`] as the JS object.
pub fn bcf_value(b: &Bcf) -> Value {
    json!({
        "version": b.version,
        "project": b.project,
        "skipped": b.skipped,
        "topics": b.topics.iter().map(|t| json!({
            "guid": t.guid, "type": t.kind, "status": t.status, "title": t.title, "description": t.description,
            "labels": t.labels, "created": t.created, "author": t.author, "assignee": t.assignee, "due": t.due, "file": t.file,
            "comments": t.comments.iter().map(|c| json!({"guid": c.guid, "date": c.date, "author": c.author, "text": c.text})).collect::<Vec<_>>(),
            "camera": t.camera.map_or(Value::Null, |c| json!({"x": num_value(c[0]), "y": num_value(c[1])})),
        })).collect::<Vec<_>>(),
    })
}

fn s_of(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).unwrap_or("").to_string()
}

/// A [`Bcf`] from the JS object (the harness hands `readBcf`'s result to `planImport`).
pub fn bcf_of(v: &Value) -> Bcf {
    let topics =
        v.get("topics")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|t| Topic {
                        guid: s_of(t.get("guid")),
                        kind: s_of(t.get("type")),
                        status: s_of(t.get("status")),
                        title: s_of(t.get("title")),
                        description: s_of(t.get("description")),
                        labels: t
                            .get("labels")
                            .and_then(Value::as_array)
                            .map(|l| l.iter().map(|x| s_of(Some(x))).collect())
                            .unwrap_or_default(),
                        created: s_of(t.get("created")),
                        author: s_of(t.get("author")),
                        assignee: s_of(t.get("assignee")),
                        due: s_of(t.get("due")),
                        file: s_of(t.get("file")),
                        comments: t
                            .get("comments")
                            .and_then(Value::as_array)
                            .map(|cs| {
                                cs.iter()
                                    .map(|c| Comment {
                                        guid: c.get("guid").map(crate::js::to_js_string).unwrap_or_else(|| "undefined".into()),
                                        date: s_of(c.get("date")),
                                        author: s_of(c.get("author")),
                                        text: s_of(c.get("text")),
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                        camera: t.get("camera").filter(|c| c.is_object()).map(|c| {
                            [c.get("x").map_or(f64::NAN, crate::js::to_number), c.get("y").map_or(f64::NAN, crate::js::to_number)]
                        }),
                    })
                    .collect()
            })
            .unwrap_or_default();
    Bcf { version: s_of(v.get("version")), project: s_of(v.get("project")), topics, skipped: 0 }
}

/// A sheet as `planImport` sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanSheet {
    pub key: String,
    pub title: String,
    pub sheet_no: String,
    pub file: String,
    pub view: [f64; 4],
}

fn iso(d: &str) -> String {
    let t = parse_date(d);
    if t.is_finite() { to_iso_s(t).unwrap_or_default() } else { String::new() }
}

fn status_of(s: &str) -> String {
    let v = trim(s);
    let lv = v.to_lowercase();
    if let Some(hit) = STATUSES.iter().find(|x| x.to_lowercase() == lv) {
        return (*hit).to_string();
    }
    if matches!(v.to_ascii_lowercase().as_str(), "closed" | "resolved" | "done") { "Completed".into() } else { String::new() }
}

fn norm(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| !(crate::js::is_ws(*c) || matches!(c, '.' | '_' | '-'))).collect()
}

/// `planImport(bcf, {markups, guidOf, replyGuids, sheets, current, author})` → the JS result object.
pub fn plan_import(
    bcf: &Bcf,
    markups: &[&Markup],
    guid_of: &[(String, String)],
    reply_guids: &[String],
    sheets: &[PlanSheet],
    current: &str,
    author: &str,
) -> Value {
    let k = 0.0254 / 72.0;
    let mut updates: Vec<Value> = Vec::new();
    let mut adds: Vec<Value> = Vec::new();
    let (mut matched, mut added, mut unplaced, mut replies) = (0usize, 0usize, 0usize, 0usize);
    let mut stack: Vec<(String, f64)> = Vec::new();
    for tp in &bcf.topics {
        let reps: Vec<Value> = tp
            .comments
            .iter()
            .filter(|c| !reply_guids.contains(&c.guid.to_lowercase()) && !c.text.is_empty())
            .map(|c| json!({"id": format!("bcf-{}", c.guid.to_lowercase()), "author": c.author, "date": iso(&c.date), "text": c.text, "kind": "comment"}))
            .collect();
        let id = guid_of.iter().rev().find(|(g, _)| *g == tp.guid).map(|(_, id)| id.clone());
        let st = status_of(&tp.status);
        if let Some(id) = id.filter(|id| !id.is_empty())
            && let Some(m) = markups.iter().rev().find(|m| m.id == id)
        {
            let mut patch = Map::new();
            if !reps.is_empty() {
                let mut all: Vec<Value> = match crate::json::markup_value(m).get("replies") {
                    Some(Value::Array(a)) => a.clone(),
                    _ => Vec::new(),
                };
                all.extend(reps.iter().cloned());
                patch.insert("replies".into(), Value::Array(all));
            }
            if !st.is_empty() && st != m.status {
                patch.insert("status".into(), Value::String(st.clone()));
            }
            if !patch.is_empty() {
                updates.push(json!({"id": id, "patch": patch}));
            }
            matched = matched.saturating_add(1);
            replies = replies.saturating_add(reps.len());
            continue;
        }
        let named = sheets
            .iter()
            .find(|s| {
                tp.labels.iter().any(|l| {
                    let nl = norm(l);
                    !nl.is_empty() && (nl == norm(&s.title) || nl == norm(&s.sheet_no))
                })
            })
            .or_else(|| if tp.file.is_empty() { None } else { sheets.iter().find(|s| norm(&s.file) == norm(&tp.file)) });
        let Some(sheet) = named.or_else(|| sheets.iter().find(|s| s.key == current)).or_else(|| sheets.first()) else {
            unplaced = unplaced.saturating_add(1);
            continue;
        };
        let [x0, y0, x1, y1] = sheet.view;
        let mut p = None;
        if let (Some(c), Some(_)) = (tp.camera, named) {
            let q = [c[0] / k, c[1] / k];
            if q[0] >= x0 && q[0] <= x1 && q[1] >= y0 && q[1] <= y1 {
                p = Some(q);
            }
        }
        let p = match p {
            Some(p) => p,
            None => {
                let i = match stack.iter_mut().find(|(key, _)| *key == sheet.key) {
                    Some(slot) => {
                        let i = slot.1;
                        slot.1 += 1.0;
                        i
                    }
                    None => {
                        stack.push((sheet.key.clone(), 1.0));
                        0.0
                    }
                };
                unplaced = unplaced.saturating_add(1);
                [x0 + 36.0 + (i % 10.0) * 28.0, y1 - 54.0 - (i / 10.0).floor() * 28.0]
            }
        };
        let mut parts: Vec<String> = vec![tp.title.clone(), tp.description.clone()];
        if !tp.assignee.is_empty() || !tp.due.is_empty() {
            parts.push([tp.assignee.as_str(), tp.due.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(", "));
        }
        let comment = parts.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n");
        let mut add = json!({
            "sheet": sheet.key, "type": "note", "pts": [[num_value(p[0]), num_value(p[1])]], "comment": comment,
            "subject": if tp.kind.is_empty() { "BCF".to_string() } else { tp.kind.clone() },
            "author": if tp.author.is_empty() { author.to_string() } else { tp.author.clone() },
            "status": if st.is_empty() { "Open".to_string() } else { st.clone() },
            "replies": reps, "bcfGuid": tp.guid,
        });
        let created = iso(&tp.created);
        if !created.is_empty()
            && let Value::Object(o) = &mut add
        {
            o.insert("created".into(), Value::String(created));
        }
        replies = replies.saturating_add(add.get("replies").and_then(Value::as_array).map_or(0, Vec::len));
        adds.push(add);
        added = added.saturating_add(1);
    }
    json!({"updates": updates, "adds": adds, "matched": matched, "added": added, "unplaced": unplaced, "replies": replies})
}
