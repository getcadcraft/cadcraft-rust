//! xml.js — a small XML reader and writer (LandXML, KML, map capabilities). Namespace prefixes are dropped from
//! element names; comments, processing instructions, DOCTYPE and CDATA are handled; only the five predefined entities
//! and numeric character references are expanded (no DTD, no external fetches); nesting is capped (`maxDepth`).

use cc_civil::i18n::t;
use cc_civil::js::{J, is_ws};
use cc_civil::tr;
use serde_json::Value;

/// An element `{ name, attrs, children, text }` (attributes in the order the JS object keeps them).
#[derive(Clone, Debug, Default)]
pub struct El {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<El>,
    pub text: String,
}
impl El {
    /// `el.attrs[k]`.
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str())
    }
    fn set_attr(&mut self, k: String, v: String) {
        match self.attrs.iter_mut().find(|(a, _)| *a == k) {
            Some(slot) => slot.1 = v,
            None => self.attrs.push((k, v)),
        }
    }
}
impl J for El {
    fn j(&self) -> Value {
        let mut attrs = serde_json::Map::new();
        for (k, v) in &self.attrs {
            attrs.insert(k.clone(), Value::String(v.clone()));
        }
        cc_civil::jo! { "name" => self.name, "attrs" => Value::Object(attrs), "children" => self.children, "text" => self.text }
    }
}

/// `kids(el, name)`.
pub fn kids<'a>(el: Option<&'a El>, name: &str) -> Vec<&'a El> {
    el.map_or_else(Vec::new, |e| e.children.iter().filter(|c| c.name == name).collect())
}
/// `kid(el, name)`.
pub fn kid<'a>(el: Option<&'a El>, name: &str) -> Option<&'a El> {
    el.and_then(|e| e.children.iter().find(|c| c.name == name))
}
/// `textOf(el)`.
pub fn text_of(el: Option<&El>) -> String {
    el.map_or_else(String::new, |e| cc_civil::js::trim(&e.text).to_string())
}

fn decode(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(rest.get(..i).unwrap_or(""));
        let tail = rest.get(i..).unwrap_or("");
        // &(#x[0-9a-fA-F]+|#\d+|[a-zA-Z]+);
        let body = tail.get(1..).unwrap_or("");
        let (len, rep): (usize, Option<String>) = if let Some(h) = body.strip_prefix("#x") {
            let n = h.bytes().take_while(u8::is_ascii_hexdigit).count();
            if n > 0 && h.as_bytes().get(n) == Some(&b';') {
                let cp = h.get(..n).map_or(f64::NAN, |d| d.bytes().fold(0.0, |a, c| a * 16.0 + f64::from(hexval(c))));
                (n.saturating_add(3), Some(from_code_point(cp)))
            } else {
                (0, None)
            }
        } else if let Some(d) = body.strip_prefix('#') {
            let n = d.bytes().take_while(u8::is_ascii_digit).count();
            if n > 0 && d.as_bytes().get(n) == Some(&b';') {
                let cp = d.get(..n).map_or(f64::NAN, |x| x.bytes().fold(0.0, |a, c| a * 10.0 + f64::from(c.saturating_sub(b'0'))));
                (n.saturating_add(2), Some(from_code_point(cp)))
            } else {
                (0, None)
            }
        } else {
            let n = body.bytes().take_while(u8::is_ascii_alphabetic).count();
            if n > 0 && body.as_bytes().get(n) == Some(&b';') {
                let name = body.get(..n).unwrap_or("");
                let r = match name {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" => "'",
                    _ => "",
                };
                if r.is_empty() {
                    (n.saturating_add(1), Some(tail.get(..n.saturating_add(2)).unwrap_or("").to_string()))
                } else {
                    (n.saturating_add(1), Some(r.to_string()))
                }
            } else {
                (0, None)
            }
        };
        match rep {
            Some(r) => {
                out.push_str(&r);
                rest = tail.get(len.saturating_add(1)..).unwrap_or("");
            }
            None => {
                out.push('&');
                rest = tail.get(1..).unwrap_or("");
            }
        }
    }
    out.push_str(rest);
    out
}
fn hexval(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c.saturating_sub(b'0'),
        b'a'..=b'f' => c.saturating_sub(b'a').saturating_add(10),
        b'A'..=b'F' => c.saturating_sub(b'A').saturating_add(10),
        _ => 0,
    }
}
/// `String.fromCodePoint(cp)`, '' when it throws. Spec gap: a lone surrogate (U+D800–U+DFFF) is a valid JS string
/// but not Rust text; it becomes U+FFFD here.
fn from_code_point(cp: f64) -> String {
    if !(0.0..=1_114_111.0).contains(&cp) {
        return String::new();
    }
    let n = cc_civil::js::usize_of(cp).and_then(|n| u32::try_from(n).ok()).unwrap_or(0);
    char::from_u32(n).map_or_else(|| "\u{fffd}".to_string(), |c| c.to_string())
}

fn local(n: &str) -> &str {
    match n.find(':') {
        Some(i) => n.get(i.saturating_add(1)..).unwrap_or(""),
        None => n,
    }
}

/// An XML error (its message is the JS's).
#[derive(Clone, Debug)]
pub struct XmlError(pub String);
impl std::fmt::Display for XmlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn ch_at(s: &str, j: usize) -> Option<char> {
    s.get(j..).and_then(|r| r.chars().next())
}
fn adv(s: &str, j: usize) -> usize {
    j.saturating_add(ch_at(s, j).map_or(1, char::len_utf8))
}

/// `parseXml(text, { maxDepth })` → the document element.
pub fn parse_xml(s: &str, max_depth: usize) -> Result<El, XmlError> {
    let n = s.len();
    let mut i = 0usize;
    let mut stack: Vec<El> = vec![El { name: "#doc".into(), ..El::default() }];
    let append_text = |el: &mut El, t: &str| {
        if el.name != "#doc" {
            el.text.push_str(&decode(t));
        }
    };
    while i < n {
        let lt = s.get(i..).and_then(|r| r.find('<')).map(|k| k.saturating_add(i));
        let Some(lt) = lt else {
            if let Some(top) = stack.last_mut() {
                append_text(top, s.get(i..).unwrap_or(""));
            }
            break;
        };
        if lt > i
            && let Some(top) = stack.last_mut()
        {
            append_text(top, s.get(i..lt).unwrap_or(""));
        }
        let at = s.get(lt..).unwrap_or("");
        if at.starts_with("<!--") {
            let e = at.get(4..).and_then(|r| r.find("-->"));
            let Some(e) = e else { return Err(XmlError(t("xml.err.comment", &[]))) };
            i = lt.saturating_add(4).saturating_add(e).saturating_add(3);
            continue;
        }
        if at.starts_with("<![CDATA[") {
            let e = at.get(9..).and_then(|r| r.find("]]>"));
            let Some(e) = e else { return Err(XmlError(t("xml.err.cdata", &[]))) };
            if let Some(top) = stack.last_mut() {
                top.text.push_str(at.get(9..e.saturating_add(9)).unwrap_or(""));
            }
            i = lt.saturating_add(9).saturating_add(e).saturating_add(3);
            continue;
        }
        if at.starts_with("<?") {
            let e = at.get(2..).and_then(|r| r.find("?>"));
            let Some(e) = e else { return Err(XmlError(t("xml.err.pi", &[]))) };
            i = lt.saturating_add(2).saturating_add(e).saturating_add(2);
            continue;
        }
        if at.starts_with("<!") {
            // DOCTYPE (possibly with an internal subset in [ ])
            let mut j = lt.saturating_add(2);
            let mut depth = 0i64;
            while j < n {
                match s.as_bytes().get(j) {
                    Some(b'[') => depth = depth.saturating_add(1),
                    Some(b']') => depth = depth.saturating_sub(1),
                    Some(b'>') if depth <= 0 => break,
                    _ => {}
                }
                j = j.saturating_add(1);
            }
            i = j.saturating_add(1);
            continue;
        }
        if s.as_bytes().get(lt.saturating_add(1)) == Some(&b'/') {
            let e = at.find('>').map(|k| k.saturating_add(lt));
            let Some(e) = e else { return Err(XmlError(t("xml.err.endtag", &[]))) };
            let name = local(cc_civil::js::trim(s.get(lt.saturating_add(2)..e).unwrap_or(""))).to_string();
            if stack.len() <= 1 {
                return Err(XmlError(tr!("xml.err.mismatch", tag = format!("</{name}>"))));
            }
            let el = stack.pop().unwrap_or_default();
            if el.name != name {
                return Err(XmlError(tr!("xml.err.mismatch.open", tag = format!("</{name}>"), open = format!("<{}>", el.name))));
            }
            if let Some(top) = stack.last_mut() {
                top.children.push(el);
            }
            i = e.saturating_add(1);
            continue;
        }
        // start tag
        let mut j = lt.saturating_add(1);
        while j < n && !ch_at(s, j).is_some_and(|c| is_ws(c) || c == '/' || c == '>') {
            j = adv(s, j);
        }
        let mut el = El { name: local(s.get(lt.saturating_add(1)..j).unwrap_or("")).to_string(), ..El::default() };
        if el.name.is_empty() {
            return Err(XmlError(t("xml.err.empty", &[])));
        }
        loop {
            while j < n && ch_at(s, j).is_some_and(is_ws) {
                j = adv(s, j);
            }
            if j >= n {
                return Err(XmlError(tr!("xml.err.tag", tag = format!("<{}>", el.name))));
            }
            let b = s.as_bytes();
            if b.get(j) == Some(&b'>') {
                j = j.saturating_add(1);
                stack.push(el);
                if stack.len() > max_depth {
                    return Err(XmlError(t("xml.err.deep", &[])));
                }
                break;
            }
            if b.get(j) == Some(&b'/') && b.get(j.saturating_add(1)) == Some(&b'>') {
                j = j.saturating_add(2);
                if let Some(top) = stack.last_mut() {
                    top.children.push(el);
                }
                break;
            }
            let mut k = j;
            while k < n && !ch_at(s, k).is_some_and(|c| is_ws(c) || c == '=' || c == '/' || c == '>') {
                k = adv(s, k);
            }
            let an = s.get(j..k).unwrap_or("").to_string();
            while k < n && ch_at(s, k).is_some_and(is_ws) {
                k = adv(s, k);
            }
            if b.get(k) != Some(&b'=') {
                return Err(XmlError(tr!("xml.err.novalue", tag = format!("<{}>", el.name))));
            }
            k = k.saturating_add(1);
            while k < n && ch_at(s, k).is_some_and(is_ws) {
                k = adv(s, k);
            }
            let q = b.get(k).copied();
            if q != Some(b'"') && q != Some(b'\'') {
                return Err(XmlError(tr!("xml.err.unquoted", tag = format!("<{}>", el.name))));
            }
            let qc = char::from(q.unwrap_or(b'"'));
            let e = s.get(k.saturating_add(1)..).and_then(|r| r.find(qc)).map(|x| x.saturating_add(k).saturating_add(1));
            let Some(e) = e else { return Err(XmlError(tr!("xml.err.attr", tag = format!("<{}>", el.name)))) };
            if !an.starts_with("xmlns") {
                el.set_attr(local(&an).to_string(), decode(s.get(k.saturating_add(1)..e).unwrap_or("")));
            }
            j = e.saturating_add(1);
        }
        i = j;
    }
    if stack.len() != 1 {
        let name = stack.last().map(|e| e.name.clone()).unwrap_or_default();
        return Err(XmlError(tr!("xml.err.ends", tag = format!("<{name}>"))));
    }
    let root = stack.pop().unwrap_or_default();
    root.children.into_iter().next().ok_or_else(|| XmlError(t("xml.err.none", &[])))
}

/// `esc(s)`: escapes text / attribute values and drops characters XML cannot hold.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let u = u32::from(c);
        if u <= 8 || u == 0x0b || u == 0x0c || (0x0e..=0x1f).contains(&u) || u == 0xfffe || u == 0xffff {
            continue;
        }
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse() {
        let e =
            parse_xml("<?xml version=\"1.0\"?><a:R x='1' y=\"&lt;&#65;&#x42;&bogus;\"><B>t &amp; u</B><!-- c --><C/></a:R>", 256).unwrap();
        assert_eq!(e.name, "R");
        assert_eq!(e.attr("y"), Some("<AB&bogus;"));
        assert_eq!(text_of(kid(Some(&e), "B")), "t & u");
        assert!(parse_xml("<a><b></a>", 256).is_err());
        assert!(parse_xml(&"<a>".repeat(300), 256).is_err());
    }
}
