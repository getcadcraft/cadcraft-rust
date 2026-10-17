//! bcf.js's small XML reader (`parseXml`): elements, attributes, text; comments, CDATA and declarations skipped; no
//! DTDs or custom entities (refused, never expanded); at most 64 levels deep. Works on UTF-16 code units, as the JS
//! string does, so character references to lone surrogates and the exact error texts come out the same.

use cc_io_ifc::util::{is_js_space, trim16};

use crate::js::num_str;

pub const MAX_DEPTH: usize = 64;

/// UTF-16 of a Rust string.
pub fn u16s(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// A Rust string of UTF-16 (lone surrogates as U+FFFD; for messages and numbers only).
pub fn lossy(s: &[u16]) -> String {
    String::from_utf16_lossy(s)
}

pub fn index_of(h: &[u16], n: &[u16], from: usize) -> Option<usize> {
    if n.is_empty() {
        return Some(from.min(h.len()));
    }
    let last = h.len().checked_sub(n.len())?;
    let mut i = from;
    while i <= last {
        if h.get(i..i.saturating_add(n.len())) == Some(n) {
            return Some(i);
        }
        i = i.saturating_add(1);
    }
    None
}

fn starts_at(h: &[u16], n: &[u16], at: usize) -> bool {
    h.get(at..at.saturating_add(n.len())) == Some(n)
}

/// One element.
#[derive(Clone, Debug, Default)]
pub struct XNode {
    pub name: Vec<u16>,
    /// attributes in assignment order; a repeated name keeps the last value (a JS object property)
    pub attrs: Vec<(Vec<u16>, Vec<u16>)>,
    pub children: Vec<usize>,
    pub text: Vec<u16>,
}

impl XNode {
    /// `node.attrs[name]`
    pub fn attr(&self, name: &str) -> Option<&[u16]> {
        let n = u16s(name);
        self.attrs.iter().rev().find(|(k, _)| *k == n).map(|(_, v)| v.as_slice())
    }
}

/// A parsed document: node 0 is the root `#doc`.
#[derive(Clone, Debug, Default)]
pub struct Doc {
    pub nodes: Vec<XNode>,
}

impl Doc {
    pub fn node(&self, i: usize) -> Option<&XNode> {
        self.nodes.get(i)
    }
    /// `kid(n, name)`
    pub fn kid(&self, n: Option<usize>, name: &str) -> Option<usize> {
        let nn = u16s(name);
        let x = self.nodes.get(n?)?;
        x.children.iter().copied().find(|&c| self.nodes.get(c).is_some_and(|y| y.name == nn))
    }
    /// `kids(n, name)`
    pub fn kids(&self, n: Option<usize>, name: &str) -> Vec<usize> {
        let nn = u16s(name);
        let Some(x) = n.and_then(|n| self.nodes.get(n)) else { return Vec::new() };
        x.children.iter().copied().filter(|&c| self.nodes.get(c).is_some_and(|y| y.name == nn)).collect()
    }
    /// `txt(n, name)`: the child's text, trimmed ('' when there is none).
    pub fn txt(&self, n: Option<usize>, name: &str) -> Vec<u16> {
        match self.kid(n, name).and_then(|c| self.nodes.get(c)) {
            Some(c) => trim16(&c.text).to_vec(),
            None => Vec::new(),
        }
    }
    pub fn attr(&self, n: Option<usize>, name: &str) -> Option<&[u16]> {
        self.nodes.get(n?)?.attr(name)
    }
}

/// `.replace(/^.*:/, '')`: drop everything up to the last ':' before the first line terminator.
fn strip_ns(s: &[u16]) -> Vec<u16> {
    let lt = s.iter().position(|&c| matches!(c, 0x0A | 0x0D | 0x2028 | 0x2029)).unwrap_or(s.len());
    match s.get(..lt).and_then(|p| p.iter().rposition(|&c| c == u16::from(b':'))) {
        Some(c) => s.get(c.saturating_add(1)..).unwrap_or(&[]).to_vec(),
        None => s.to_vec(),
    }
}

/// `parseInt(digits, radix)` of an all-digit string (correctly rounded).
fn parse_int(d: &[u16], radix: u32) -> f64 {
    let s = lossy(d);
    if radix == 10 {
        return s.parse::<f64>().unwrap_or(f64::NAN);
    }
    let t = s.trim_start_matches('0');
    if t.len() <= 32 {
        return u128::from_str_radix(t, 16).map_or(f64::NAN, |v| {
            // u128 → f64 rounds to nearest (ties to even), as V8 does for a power-of-two radix
            #[allow(clippy::cast_precision_loss)]
            let f = v as f64;
            f
        });
    }
    // past 128 bits: far beyond any code point (only the error text depends on it)
    t.chars().fold(0.0, |a, c| a * 16.0 + f64::from(c.to_digit(16).unwrap_or(0)))
}

/// `unesc(s)`: the five entities and numeric character references; Err = fromCodePoint's RangeError.
pub fn unesc(s: &[u16]) -> Result<Vec<u16>, String> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0usize;
    let amp = u16::from(b'&');
    while i < s.len() {
        let c = s.get(i).copied().unwrap_or(0);
        if c != amp {
            out.push(c);
            i = i.saturating_add(1);
            continue;
        }
        let rest = s.get(i.saturating_add(1)..).unwrap_or(&[]);
        let is_hex = |c: u16| u8::try_from(c).is_ok_and(|b| b.is_ascii_hexdigit());
        let is_dig = |c: u16| (u16::from(b'0')..=u16::from(b'9')).contains(&c);
        let mut matched: Option<(usize, Vec<u16>)> = None;
        if rest.first() == Some(&u16::from(b'#')) {
            if rest.get(1) == Some(&u16::from(b'x')) {
                let n = rest.iter().skip(2).take_while(|&&c| is_hex(c)).count();
                if n > 0 && rest.get(n.saturating_add(2)) == Some(&u16::from(b';')) {
                    let v = parse_int(rest.get(2..n.saturating_add(2)).unwrap_or(&[]), 16);
                    matched = Some((n.saturating_add(3), code_point(v)?));
                }
            }
            if matched.is_none() {
                let n = rest.iter().skip(1).take_while(|&&c| is_dig(c)).count();
                if n > 0 && rest.get(n.saturating_add(1)) == Some(&u16::from(b';')) {
                    let v = parse_int(rest.get(1..n.saturating_add(1)).unwrap_or(&[]), 10);
                    matched = Some((n.saturating_add(2), code_point(v)?));
                }
            }
        } else {
            for (name, ch) in [("amp;", '&'), ("lt;", '<'), ("gt;", '>'), ("quot;", '"'), ("apos;", '\'')] {
                if starts_at(rest, &u16s(name), 0) {
                    matched = Some((name.len(), vec![u16::try_from(u32::from(ch)).unwrap_or(0)]));
                    break;
                }
            }
        }
        match matched {
            Some((len, rep)) => {
                out.extend_from_slice(&rep);
                i = i.saturating_add(1).saturating_add(len);
            }
            None => {
                out.push(c);
                i = i.saturating_add(1);
            }
        }
    }
    Ok(out)
}

/// `String.fromCodePoint(v)` as UTF-16 (a lone surrogate stays one unit).
fn code_point(v: f64) -> Result<Vec<u16>, String> {
    if !(0.0..=1_114_111.0).contains(&v) || v.fract() != 0.0 {
        return Err(format!("Invalid code point {}", num_str(v)));
    }
    // a whole number in 0..=0x10FFFF
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let cp = v as u32;
    if cp <= 0xFFFF {
        return Ok(vec![u16::try_from(cp).unwrap_or(0)]);
    }
    let c = cp.saturating_sub(0x1_0000);
    Ok(vec![u16::try_from(0xD800 | (c >> 10)).unwrap_or(0), u16::try_from(0xDC00 | (c & 0x3FF)).unwrap_or(0)])
}

fn top_err(what: &str) -> String {
    format!("Cannot read properties of undefined (reading '{what}')")
}

/// `parseXml(text)` → the document, or the error message the JS throws.
pub fn parse_xml(text: &[u16]) -> Result<Doc, String> {
    let mut doc = Doc { nodes: vec![XNode { name: u16s("#doc"), ..XNode::default() }] };
    let mut stack: Vec<usize> = vec![0];
    let s: &[u16] = if text.first() == Some(&0xFEFF) { text.get(1..).unwrap_or(&[]) } else { text };
    let mut i = 0usize;
    let lt_c = [u16::from(b'<')];
    macro_rules! top {
        ($what:expr) => {
            match stack.last().copied().and_then(|t| doc.nodes.get_mut(t)) {
                Some(n) => n,
                None => return Err(top_err($what)),
            }
        };
    }
    while i < s.len() {
        let Some(lt) = index_of(s, &lt_c, i) else {
            let t = unesc(s.get(i..).unwrap_or(&[]))?;
            top!("text").text.extend_from_slice(&t);
            break;
        };
        if lt > i {
            let t = unesc(s.get(i..lt).unwrap_or(&[]))?;
            top!("text").text.extend_from_slice(&t);
        }
        if starts_at(s, &u16s("<!--"), lt) {
            let Some(e) = index_of(s, &u16s("-->"), lt) else { return Err("unclosed comment".into()) };
            i = e.saturating_add(3);
            continue;
        }
        if starts_at(s, &u16s("<![CDATA["), lt) {
            let Some(e) = index_of(s, &u16s("]]>"), lt) else { return Err("unclosed CDATA".into()) };
            let from = lt.saturating_add(9);
            let part = if e > from { s.get(from..e).unwrap_or(&[]).to_vec() } else { Vec::new() };
            top!("text").text.extend_from_slice(&part);
            i = e.saturating_add(3);
            continue;
        }
        if starts_at(s, &u16s("<?"), lt) {
            let Some(e) = index_of(s, &u16s("?>"), lt) else { return Err("unclosed declaration".into()) };
            i = e.saturating_add(2);
            continue;
        }
        if starts_at(s, &u16s("<!"), lt) {
            return Err("DTDs are not read".into());
        }
        let Some(gt) = index_of(s, &[u16::from(b'>')], lt) else { return Err("unclosed tag".into()) };
        let tag = s.get(lt.saturating_add(1)..gt).unwrap_or(&[]);
        if tag.first() == Some(&u16::from(b'/')) {
            let name = strip_ns(trim16(tag.get(1..).unwrap_or(&[])));
            let top = stack.pop();
            if top.and_then(|t| doc.nodes.get(t)).is_none_or(|n| n.name != name) {
                return Err(format!("mismatched </{}>", lossy_keep(&name)));
            }
            i = gt.saturating_add(1);
            continue;
        }
        let selfc = tag.last() == Some(&u16::from(b'/'));
        let body = if selfc { tag.get(..tag.len().saturating_sub(1)).unwrap_or(&[]) } else { tag };
        // /^\s*([^\s/>]+)/
        let ws = body.iter().take_while(|&&c| is_js_space(u32::from(c))).count();
        let nlen =
            body.iter().skip(ws).take_while(|&&c| !(is_js_space(u32::from(c)) || c == u16::from(b'/') || c == u16::from(b'>'))).count();
        if nlen == 0 {
            return Err("bad tag".into());
        }
        let mlen = ws.saturating_add(nlen);
        let mut node = XNode { name: strip_ns(body.get(ws..mlen).unwrap_or(&[])), ..XNode::default() };
        attrs(body.get(mlen..).unwrap_or(&[]), &mut node.attrs)?;
        let ni = doc.nodes.len();
        top!("children").children.push(ni);
        doc.nodes.push(node);
        if !selfc {
            stack.push(ni);
            if stack.len() > MAX_DEPTH {
                return Err("too deeply nested".into());
            }
        }
        i = gt.saturating_add(1);
    }
    if stack.len() != 1 {
        let name = match stack.last().and_then(|t| doc.nodes.get(*t)) {
            Some(n) => n.name.clone(),
            None => return Err(top_err("name")),
        };
        return Err(format!("unclosed <{}>", lossy_keep(&name)));
    }
    Ok(doc)
}

/// A message part: the JS keeps lone surrogates in its message; they go out as U+FFFD here (spec gap: messages are
/// compared after JSON, where the JS's lone surrogate would print as an escape).
fn lossy_keep(s: &[u16]) -> String {
    lossy(s)
}

/// The attributes regex `([^\s=]+)\s*=\s*("([^"]*)"|'([^']*)')/g` over the rest of a tag. A failed start inside a run
/// of name characters fails the same way at every later start in that run, so the scan jumps over the run (linear;
/// the JS regex can take quadratic time on the same input but finds the same matches).
fn attrs(b: &[u16], out: &mut Vec<(Vec<u16>, Vec<u16>)>) -> Result<(), String> {
    let is_name = |c: u16| !(is_js_space(u32::from(c)) || c == u16::from(b'='));
    let mut p = 0usize;
    while p < b.len() {
        if !b.get(p).is_some_and(|&c| is_name(c)) {
            p = p.saturating_add(1);
            continue;
        }
        let run_end = p.saturating_add(b.iter().skip(p).take_while(|&&c| is_name(c)).count());
        let mut q = run_end;
        while b.get(q).is_some_and(|&c| is_js_space(u32::from(c))) {
            q = q.saturating_add(1);
        }
        let mut ok = None;
        if b.get(q) == Some(&u16::from(b'=')) {
            q = q.saturating_add(1);
            while b.get(q).is_some_and(|&c| is_js_space(u32::from(c))) {
                q = q.saturating_add(1);
            }
            if let Some(&quote) = b.get(q).filter(|&&c| c == u16::from(b'"') || c == u16::from(b'\''))
                && let Some(close) = b.iter().skip(q.saturating_add(1)).position(|&c| c == quote)
            {
                let vs = q.saturating_add(1);
                let ve = vs.saturating_add(close);
                ok = Some((b.get(vs..ve).unwrap_or(&[]).to_vec(), ve.saturating_add(1)));
            }
        }
        match ok {
            Some((val, next)) => {
                let name = strip_ns(b.get(p..run_end).unwrap_or(&[]));
                let v = unesc(&val)?;
                // `obj.__proto__ = 'text'` sets no own property
                if name != u16s("__proto__") {
                    out.push((name, v));
                }
                p = next;
            }
            None => p = run_end,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_documents() {
        let d = parse_xml(&u16s(
            "<?xml version=\"1.0\"?><a:Markup x='1' y=\"a&amp;b\"><Topic Guid=\"g\"><Title> t &lt;x&gt; </Title></Topic></a:Markup>",
        ))
        .unwrap();
        let m = d.kid(Some(0), "Markup");
        assert!(m.is_some());
        assert_eq!(lossy(d.attr(m, "y").unwrap()), "a&b");
        let t = d.kid(m, "Topic");
        assert_eq!(lossy(&d.txt(t, "Title")), "t <x>");
        assert_eq!(parse_xml(&u16s("<!DOCTYPE x [<!ENTITY a \"aaaa\">]><x>&a;</x>")).unwrap_err(), "DTDs are not read");
        assert_eq!(parse_xml(&u16s("<a></b>")).unwrap_err(), "mismatched </b>");
        assert_eq!(parse_xml(&u16s("<a>")).unwrap_err(), "unclosed <a>");
        assert_eq!(parse_xml(&u16s("<a>&#x110000;</a>")).unwrap_err(), "Invalid code point 1114112");
        assert_eq!(parse_xml(&u16s("<a>&#xD800;</a>")).unwrap().nodes[1].text, vec![0xD800]);
    }
}
