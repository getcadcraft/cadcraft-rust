//! link.js — sheet hyperlinks (0.4 K9) and sheet-callout references (K16).
//!
//! `cleanLink(v)` (the stored link text), `linkTarget(link, sheets)` (the sheet a link names: by sheet number, the
//! newest copy winning, else by title) and `calloutRefs(text, sheetNos)` (references like "3/A-501", "A-501",
//! "2 / A501" in a sheet's text whose number is one of `sheetNos`). The JS regular expression
//! `(?:\b(\d{1,3}|[A-Z]\d?)\s*\/\s*)?\b([A-Z]{1,3}[-.]?\d{1,3}(?:[.-]\d{1,3})?[A-Z]?)\b` (flags gi) is matched by hand
//! here, trying the alternatives in the order the JS backtracking engine tries them, over UTF-16 code units.

use crate::js::{is_ws, slice16, trim};
use crate::slip::sheet_no_key;

/// `cleanLink(v)`: control characters dropped, trimmed, at most 60 UTF-16 units.
pub fn clean_link(v: &str) -> String {
    let s: String = v.chars().filter(|c| u32::from(*c) > 0x1f).collect();
    slice16(trim(&s), 60)
}

/// A sheet as `linkTarget` sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkSheet {
    pub key: String,
    pub sheet_no: String,
    pub title: String,
}

/// `linkTarget(link, sheets)` → the index of the sheet it names.
pub fn link_target(link: &str, sheets: &[LinkSheet]) -> Option<usize> {
    let want = sheet_no_key(link);
    if want.is_empty() {
        return None;
    }
    let mut hit = None;
    for (i, s) in sheets.iter().enumerate() {
        if sheet_no_key(&s.sheet_no) == want {
            hit = Some(i);
        }
    }
    if hit.is_some() {
        return hit;
    }
    let l = trim(link).to_lowercase();
    for (i, s) in sheets.iter().enumerate() {
        if trim(&s.title).to_lowercase() == l {
            hit = Some(i);
        }
    }
    hit
}

/// One reference found by [`callout_refs`] (`start`/`end` in UTF-16 units, as the JS `m.index`).
#[derive(Debug, Clone, PartialEq)]
pub struct CalloutRef {
    pub start: usize,
    pub end: usize,
    pub r#ref: String,
    pub no: String,
}

fn is_word(c: u16) -> bool {
    u8::try_from(c).is_ok_and(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn is_alpha(c: Option<&u16>) -> bool {
    c.and_then(|c| u8::try_from(*c).ok()).is_some_and(|b| b.is_ascii_alphabetic())
}
fn is_digit(c: Option<&u16>) -> bool {
    c.and_then(|c| u8::try_from(*c).ok()).is_some_and(|b| b.is_ascii_digit())
}
fn is_space(c: Option<&u16>) -> bool {
    c.and_then(|c| char::from_u32(u32::from(*c))).is_some_and(is_ws)
}

/// `\b` at position k.
fn boundary(t: &[u16], k: usize) -> bool {
    let before = k.checked_sub(1).and_then(|j| t.get(j)).is_some_and(|c| is_word(*c));
    let after = t.get(k).is_some_and(|c| is_word(*c));
    before != after
}

/// Greedy run of `pred` from `k`, at most `max`.
fn run(t: &[u16], k: usize, max: usize, pred: fn(Option<&u16>) -> bool) -> usize {
    let mut n = 0usize;
    while n < max && pred(t.get(k.saturating_add(n))) {
        n = n.saturating_add(1);
    }
    n
}

/// Group 2 `[A-Z]{1,3}[-.]?\d{1,3}(?:[.-]\d{1,3})?[A-Z]?` then `\b`, from `j`: the end of the first way that matches.
fn group2(t: &[u16], j: usize) -> Option<usize> {
    let letters = run(t, j, 3, is_alpha);
    for nl in (1..=letters).rev() {
        let a = j.saturating_add(nl);
        let sep = t.get(a).is_some_and(|c| *c == u16::from(b'-') || *c == u16::from(b'.'));
        let seps: &[usize] = if sep { &[1, 0] } else { &[0] };
        for &ns in seps {
            let b = a.saturating_add(ns);
            let digits = run(t, b, 3, is_digit);
            for nd in (1..=digits).rev() {
                let c = b.saturating_add(nd);
                // (?:[.-]\d{1,3})? — present first, then absent
                let mut tails: Vec<usize> = Vec::new();
                if t.get(c).is_some_and(|x| *x == u16::from(b'.') || *x == u16::from(b'-')) {
                    let d2 = run(t, c.saturating_add(1), 3, is_digit);
                    for k in (1..=d2).rev() {
                        tails.push(c.saturating_add(1).saturating_add(k));
                    }
                }
                tails.push(c);
                for e in tails {
                    // [A-Z]? — present first, then absent; then \b
                    if is_alpha(t.get(e)) && boundary(t, e.saturating_add(1)) {
                        return Some(e.saturating_add(1));
                    }
                    if boundary(t, e) {
                        return Some(e);
                    }
                }
            }
        }
    }
    None
}

/// The whole pattern at position `i`: (group 2 start, end).
fn match_at(t: &[u16], i: usize) -> Option<(usize, usize)> {
    // with the detail-number prefix: \b (\d{1,3} | [A-Z]\d?) \s* / \s* \b
    if boundary(t, i) {
        let mut heads: Vec<usize> = Vec::new();
        let nd = run(t, i, 3, is_digit);
        for k in (1..=nd).rev() {
            heads.push(i.saturating_add(k));
        }
        if is_alpha(t.get(i)) {
            if is_digit(t.get(i.saturating_add(1))) {
                heads.push(i.saturating_add(2));
            }
            heads.push(i.saturating_add(1));
        }
        for h in heads {
            let s1 = h.saturating_add(run(t, h, usize::MAX, is_space));
            if t.get(s1) != Some(&u16::from(b'/')) {
                continue;
            }
            let after = s1.saturating_add(1);
            let j = after.saturating_add(run(t, after, usize::MAX, is_space));
            if boundary(t, j)
                && let Some(e) = group2(t, j)
            {
                return Some((j, e));
            }
        }
    }
    if boundary(t, i)
        && let Some(e) = group2(t, i)
    {
        return Some((i, e));
    }
    None
}

/// `calloutRefs(text, sheetNos)`.
pub fn callout_refs(text: &str, sheet_nos: &[String]) -> Vec<CalloutRef> {
    let mut known: Vec<(String, String)> = Vec::new();
    for n in sheet_nos.iter().filter(|n| !n.is_empty()) {
        let k = sheet_no_key(n);
        match known.iter_mut().find(|(kk, _)| *kk == k) {
            Some(slot) => slot.1 = n.clone(),
            None => known.push((k, n.clone())),
        }
    }
    let t: Vec<u16> = text.encode_utf16().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < t.len() {
        let Some((g2, end)) = match_at(&t, i) else {
            i = i.saturating_add(1);
            continue;
        };
        let g2s = String::from_utf16_lossy(t.get(g2..end).unwrap_or(&[]));
        let key = sheet_no_key(&g2s);
        if let Some((_, no)) = known.iter().find(|(k, _)| *k == key) {
            out.push(CalloutRef { start: i, end, r#ref: String::from_utf16_lossy(t.get(i..end).unwrap_or(&[])), no: no.clone() });
        }
        i = if end > i { end } else { i.saturating_add(1) };
    }
    out
}
