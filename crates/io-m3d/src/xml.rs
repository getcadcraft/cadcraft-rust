//! xml.js — a small, strict XML reader for untrusted files (Collada). Only the five predefined entities and numeric
//! character references are decoded; a DOCTYPE is skipped and anything it declares is never expanded (no entity
//! bombs, no external fetches). Mismatched or unclosed tags fail with a plain message. The tree is kept in an arena
//! (no recursion anywhere: deep files cannot overflow the stack); at most `max_elements` elements.
//!
//! Text positions: the JS works on UTF-16 strings, this on UTF-8 bytes. Every delimiter it looks for is ASCII, so
//! the same cuts fall on the same characters.

use crate::Res;
use crate::i18n::{err, err_with, t};
use crate::num::is_space;

/// One element: local name (any "prefix:" dropped), attributes by local name (a repeated name keeps its first place
/// and its last value, as a JS object does), children (arena indices, in order), and its own character data.
#[derive(Clone, Debug, Default)]
pub struct El {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<usize>,
    pub text: String,
}

impl El {
    /// `el.attrs[k]`.
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
}

/// A parsed document: `els[root]` is the root element.
#[derive(Clone, Debug, Default)]
pub struct Doc {
    pub els: Vec<El>,
    pub root: usize,
}

impl Doc {
    /// The element at an index (a fresh empty one for a bad index: callers only hand out indices of this arena).
    pub fn el(&self, i: usize) -> &El {
        static EMPTY: El = El { name: String::new(), attrs: Vec::new(), children: Vec::new(), text: String::new() };
        self.els.get(i).unwrap_or(&EMPTY)
    }
    /// `child(el, name)`.
    pub fn child(&self, el: Option<usize>, name: &str) -> Option<usize> {
        let e = el?;
        self.el(e).children.iter().copied().find(|&c| self.el(c).name == name)
    }
    /// `kids(el, name)`.
    pub fn kids(&self, el: Option<usize>, name: &str) -> Vec<usize> {
        match el {
            None => Vec::new(),
            Some(e) => self.el(e).children.iter().copied().filter(|&c| self.el(c).name == name).collect(),
        }
    }
    /// `walkXML(el, fn)`: depth first, without recursion.
    pub fn walk(&self, el: usize, mut f: impl FnMut(usize)) {
        let mut st = vec![el];
        while let Some(e) = st.pop() {
            f(e);
            for &c in self.el(e).children.iter().rev() {
                st.push(c);
            }
        }
    }
}

/// `decode(s)`: the five predefined entities and numeric references (`n > 0 && n <= 0x10ffff`, else nothing).
/// Spec gap (JS-engine quirk): a reference to a lone surrogate (U+D800–U+DFFF) gives U+FFFD, since a Rust string cannot
/// hold a lone surrogate.
pub fn decode(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    let mut last = 0usize;
    while let Some(off) = b.get(i..).and_then(|r| r.iter().position(|&c| c == b'&')) {
        let amp = i.saturating_add(off);
        let rest = b.get(amp.saturating_add(1)..).unwrap_or(&[]);
        // the entity body runs to the first ';' (the regex's alternatives never contain one)
        let mut rep: Option<(String, usize)> = None;
        let named = [("lt", "<"), ("gt", ">"), ("amp", "&"), ("quot", "\""), ("apos", "'")];
        for (n, r) in named {
            if rest.starts_with(n.as_bytes()) && rest.get(n.len()) == Some(&b';') {
                rep = Some((r.to_string(), n.len().saturating_add(2)));
                break;
            }
        }
        if rep.is_none() && rest.first() == Some(&b'#') {
            let (hexa, start) = if rest.get(1) == Some(&b'x') { (true, 2usize) } else { (false, 1usize) };
            let digits = rest.get(start..).unwrap_or(&[]);
            let k = digits.iter().take_while(|c| if hexa { c.is_ascii_hexdigit() } else { c.is_ascii_digit() }).count();
            if k > 0 && digits.get(k) == Some(&b';') {
                let mut n = 0.0f64;
                for &c in digits.get(..k).unwrap_or(&[]) {
                    let d = char::from(c).to_digit(if hexa { 16 } else { 10 }).unwrap_or(0);
                    n = n * if hexa { 16.0 } else { 10.0 } + f64::from(d);
                }
                let txt = if n > 0.0 && n <= 1_114_111.0 {
                    // a whole number in 1 ..= 0x10FFFF
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let cp = n as u32;
                    char::from_u32(cp).unwrap_or('\u{FFFD}').to_string()
                } else {
                    String::new()
                };
                rep = Some((txt, start.saturating_add(k).saturating_add(2)));
            }
        }
        match rep {
            Some((r, used)) => {
                out.push_str(s.get(last..amp).unwrap_or(""));
                out.push_str(&r);
                last = amp.saturating_add(used);
                i = last;
            }
            None => i = amp.saturating_add(1),
        }
    }
    out.push_str(s.get(last..).unwrap_or(""));
    out
}

/// `local(n)`: the part after the first ':'.
fn local(n: &str) -> String {
    match n.find(':') {
        Some(i) => n.get(i.saturating_add(1)..).unwrap_or("").to_string(),
        None => n.to_string(),
    }
}

/// `str.slice(0, 40)` in UTF-16 units.
fn s40(s: &str) -> String {
    cc_m3d::js::slice16(s, 40)
}

/// The attributes of a start tag body after its name: `ATTR` = /([^\s=/>]+)\s*=\s*(?:"([^"]*)"|'([^']*)')/g run from
/// `from`, searching forward as `RegExp.exec` does (unmatched text is skipped).
fn attrs(body: &str, from: usize, el: &mut El) {
    let b = body.as_bytes();
    let is_name = |c: char| !is_space(c) && c != '=' && c != '/' && c != '>';
    let mut pos = from;
    while pos < b.len() {
        let Some(tail) = body.get(pos..) else {
            pos = pos.saturating_add(1);
            continue;
        };
        // the name: the longest run of name characters here (a shorter one would be followed by a name character)
        let name_len: usize = tail.chars().take_while(|&c| is_name(c)).map(char::len_utf8).sum();
        let ch_len = tail.chars().next().map_or(1, char::len_utf8);
        if name_len == 0 {
            pos = pos.saturating_add(ch_len);
            continue;
        }
        let after = tail.get(name_len..).unwrap_or("");
        let a2 = after.trim_start_matches(is_space);
        let matched = a2.strip_prefix('=').and_then(|a3| {
            let a4 = a3.trim_start_matches(is_space);
            let q = a4.chars().next()?;
            if q != '"' && q != '\'' {
                return None;
            }
            let inner = a4.get(1..)?;
            let end = inner.find(q)?;
            let val = inner.get(..end)?;
            let consumed = tail.len().saturating_sub(inner.len()).saturating_add(end).saturating_add(1);
            Some((val.to_string(), consumed))
        });
        match matched {
            Some((val, consumed)) => {
                let name = local(tail.get(..name_len).unwrap_or(""));
                let v = decode(&val);
                match el.attrs.iter_mut().find(|(n, _)| *n == name) {
                    Some(slot) => slot.1 = v,
                    None => el.attrs.push((name, v)),
                }
                pos = pos.saturating_add(consumed);
            }
            None => pos = pos.saturating_add(ch_len),
        }
    }
}

/// `parseXML(text, {maxElements})` → the document (its root element).
pub fn parse_xml(text: &str, max_elements: usize) -> Res<Doc> {
    let fail = |m: &str| Err(err_with("m3d.xml.bad", &[("why", m)]));
    let b = text.as_bytes();
    let l = b.len();
    let mut els: Vec<El> = Vec::new();
    let mut texts: Vec<Vec<String>> = Vec::new();
    // stack of open elements (arena indices); the document is the empty stack
    let mut stack: Vec<usize> = Vec::new();
    let mut root: Option<usize> = None;
    let mut pos = if text.starts_with('\u{feff}') { 3 } else { 0 };
    let mut count = 0usize;
    let find = |from: usize, pat: &str| -> Option<usize> { text.get(from..).and_then(|r| r.find(pat)).map(|i| i.saturating_add(from)) };
    let add_text = |stack: &[usize], texts: &mut Vec<Vec<String>>, s: String| {
        if let Some(&top) = stack.last()
            && let Some(t) = texts.get_mut(top)
        {
            t.push(s);
        }
    };
    while pos < l {
        let Some(lt) = find(pos, "<") else {
            add_text(&stack, &mut texts, decode(text.get(pos..).unwrap_or("")));
            break;
        };
        if lt > pos {
            add_text(&stack, &mut texts, decode(text.get(pos..lt).unwrap_or("")));
        }
        let at = text.get(lt..).unwrap_or("");
        if at.starts_with("<?") {
            let Some(e) = find(lt.saturating_add(2), "?>") else { return fail(&t("m3d.xml.an_unclosed_processing_instruction", &[])) };
            pos = e.saturating_add(2);
            continue;
        }
        if at.starts_with("<!--") {
            let Some(e) = find(lt.saturating_add(4), "-->") else { return fail(&t("m3d.xml.an_unclosed_comment", &[])) };
            pos = e.saturating_add(3);
            continue;
        }
        if at.starts_with("<![CDATA[") {
            let Some(e) = find(lt.saturating_add(9), "]]>") else { return fail(&t("m3d.xml.an_unclosed_cdata_section", &[])) };
            add_text(&stack, &mut texts, text.get(lt.saturating_add(9)..e).unwrap_or("").to_string());
            pos = e.saturating_add(3);
            continue;
        }
        if at.starts_with("<!") {
            // DOCTYPE (with an optional internal subset): skipped, nothing in it is used
            let mut i = lt.saturating_add(2);
            let mut depth = 0i64;
            let mut q = 0u8;
            while i < l {
                let c = b.get(i).copied().unwrap_or(0);
                if q != 0 {
                    if c == q {
                        q = 0;
                    }
                } else if c == b'"' || c == b'\'' {
                    q = c;
                } else if c == b'[' {
                    depth = depth.saturating_add(1);
                } else if c == b']' {
                    depth = depth.saturating_sub(1);
                } else if c == b'>' && depth <= 0 {
                    break;
                }
                i = i.saturating_add(1);
            }
            if i >= l {
                return fail(&t("m3d.xml.an_unclosed_declaration", &[]));
            }
            pos = i.saturating_add(1);
            continue;
        }
        if b.get(lt.saturating_add(1)) == Some(&b'/') {
            let Some(e) = find(lt.saturating_add(2), ">") else { return fail(&t("m3d.xml.an_unclosed_end_tag", &[])) };
            let name = local(cc_m3d::js::trim(text.get(lt.saturating_add(2)..e).unwrap_or("")));
            let top = stack.pop();
            let ok = top.is_some_and(|tp| els.get(tp).is_some_and(|x| x.name == name));
            if !ok {
                return fail(&format!("</{}> does not match its start tag", s40(&name)));
            }
            if let Some(tp) = top
                && let (Some(el), Some(tx)) = (els.get_mut(tp), texts.get_mut(tp))
            {
                el.text = std::mem::take(tx).concat();
            }
            pos = e.saturating_add(1);
            continue;
        }
        // start tag: find its end outside quotes
        let mut i = lt.saturating_add(1);
        let mut q = 0u8;
        while i < l {
            let c = b.get(i).copied().unwrap_or(0);
            if q != 0 {
                if c == q {
                    q = 0;
                }
            } else if c == b'"' || c == b'\'' {
                q = c;
            } else if c == b'>' {
                break;
            }
            i = i.saturating_add(1);
        }
        if i >= l {
            return fail(&t("m3d.xml.an_unclosed_start_tag", &[]));
        }
        let mut body = text.get(lt.saturating_add(1)..i).unwrap_or("");
        let self_close = body.ends_with('/');
        if self_close {
            body = body.get(..body.len().saturating_sub(1)).unwrap_or("");
        }
        let name_len: usize = body.chars().take_while(|&c| !is_space(c) && c != '/' && c != '>').map(char::len_utf8).sum();
        if name_len == 0 {
            return fail(&t("m3d.xml.a_tag_without_a_name", &[]));
        }
        let mut el = El { name: local(body.get(..name_len).unwrap_or("")), ..El::default() };
        attrs(body, name_len, &mut el);
        count = count.saturating_add(1);
        if count > max_elements {
            return Err(err_with("m3d.xml.toobig", &[("n", &cc_m3d::js::num_str(max_elements as f64))]));
        }
        let idx = els.len();
        match stack.last() {
            None => {
                if root.is_some() {
                    return fail(&t("m3d.xml.more_than_one_root_element", &[]));
                }
                root = Some(idx);
            }
            Some(&p) => {
                if let Some(pe) = els.get_mut(p) {
                    pe.children.push(idx);
                }
            }
        }
        els.push(el);
        texts.push(Vec::new());
        if !self_close {
            stack.push(idx);
        }
        pos = i.saturating_add(1);
    }
    if let Some(&top) = stack.last() {
        let n = els.get(top).map(|e| s40(&e.name)).unwrap_or_default();
        return fail(&format!("<{n}> is never closed"));
    }
    let Some(root) = root else { return fail(&t("m3d.xml.no_root_element", &[])) };
    let _ = err;
    Ok(Doc { els, root })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities_and_doctype() {
        let d = parse_xml(
            "<?xml version=\"1.0\"?><!DOCTYPE a [<!ENTITY x \"BOOM\"><!ENTITY y SYSTEM \"file:///etc/passwd\">]><a t=\"&lt;&#65;&#x42;\">&x;&y;<![CDATA[<raw>]]>&amp;</a>",
            100,
        )
        .unwrap();
        let r = d.el(d.root);
        assert_eq!(r.text, "&x;&y;<raw>&");
        assert_eq!(r.attr("t"), Some("<AB"));
    }

    #[test]
    fn refusals() {
        assert!(parse_xml("<COLLADA><asset></COLLADA>", 100).is_err());
        assert!(parse_xml("solid cube", 100).is_err());
        assert!(parse_xml("<a/><b/>", 100).is_err());
        assert!(parse_xml("<a><b/><c/></a>", 2).is_err());
    }

    #[test]
    fn attributes_skip_garbage() {
        let d = parse_xml("<a x:b = 'q' junk c=\"1\" c=\"2\"/>", 10).unwrap();
        let r = d.el(d.root);
        assert_eq!(r.attrs, vec![("b".to_string(), "q".to_string()), ("c".to_string(), "2".to_string())]);
    }
}
