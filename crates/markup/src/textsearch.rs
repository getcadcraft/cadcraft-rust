//! textsearch.js — find words in the drawings' own text (0.4 K14).
//!
//! `textIndex(items)` (the page's text as one string with the span of every pdf.js text item), `findHits(index,
//! query, {matchCase, wholeWord, max})`, `rangeBox(index, start, end)` and `hitCloud(hit, opts)`. Positions are
//! UTF-16 units, as in the JS. The JS search is a regular expression with the `u` flag (and `i` unless matching
//! case): the query's words, escaped, joined by `\s+`, inside `(?<![\p{L}\p{N}])` … `(?![\p{L}\p{N}])` for whole
//! words. It is matched by hand here, code point by code point.
//! Spec gap: Unicode simple case folding and the `\p{L}` class are approximated with Rust's `to_lowercase` (single
//! character mappings, plus the folds that differ from lower case: ς ſ ϐ ϑ ϕ ϖ ϰ ϱ ϵ ẛ ι) and `is_alphabetic`
//! (which also counts combining vowel signs as letters); both agree with the JS on Latin, Greek and Cyrillic text.

use crate::js::{is_ws, len16};
use cc_cad::js::{max_of, min_of};

/// A text item as pdf.js `getTextContent` gives it.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub str: String,
    pub transform: Option<Vec<f64>>,
    pub width: f64,
    pub has_eol: bool,
}

/// A span of the index text: characters [start, end) are item `item`.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub item: usize,
}

/// `textIndex(items)`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Index {
    pub text: String,
    pub spans: Vec<Span>,
    pub items: Vec<Item>,
}

/// `textIndex(items)`.
pub fn text_index(items: Vec<Item>) -> Index {
    let mut text = String::new();
    let mut n16 = 0usize;
    let mut spans = Vec::new();
    for (k, it) in items.iter().enumerate() {
        let s = &it.str;
        if s.is_empty() {
            if it.has_eol && !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
                n16 = n16.saturating_add(1);
            }
            continue;
        }
        if !text.is_empty() && !text.chars().last().is_some_and(is_ws) {
            text.push(' ');
            n16 = n16.saturating_add(1);
        }
        let l = len16(s);
        spans.push(Span { start: n16, end: n16.saturating_add(l), item: k });
        text.push_str(s);
        n16 = n16.saturating_add(l);
        if it.has_eol {
            text.push('\n');
            n16 = n16.saturating_add(1);
        }
    }
    Index { text, spans, items }
}

/// The box (user space) of characters [i0, i1) of one item.
fn part_box(it: &Item, i0: f64, i1: f64) -> [f64; 4] {
    let t = it.transform.clone().unwrap_or_else(|| vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    let g = |i: usize| t.get(i).copied().unwrap_or(f64::NAN);
    let (a, b, c, d, e, f) = (g(0), g(1), g(2), g(3), g(4), g(5));
    let n = (len16(&it.str) as f64).max(1.0);
    let or1 = |x: f64| if x != 0.0 && !x.is_nan() { x } else { 1.0 };
    let len = or1(crate::js::hypot(a, b));
    let up = or1(crate::js::hypot(c, d));
    let (ux, uy, vx, vy) = (a / len, b / len, c / up, d / up);
    let size = up;
    let w = if it.width.is_finite() && it.width > 0.0 { it.width } else { n * size * 0.5 };
    let s0 = (w * i0) / n;
    let s1 = (w * i1) / n;
    let lo = -0.22 * size;
    let hi = 0.9 * size;
    let pts: Vec<[f64; 2]> =
        [[s0, lo], [s1, lo], [s1, hi], [s0, hi]].iter().map(|[s, t]| [e + ux * s + vx * t, f + uy * s + vy * t]).collect();
    [min_of(pts.iter().map(|p| p[0])), min_of(pts.iter().map(|p| p[1])), max_of(pts.iter().map(|p| p[0])), max_of(pts.iter().map(|p| p[1]))]
}

/// `rangeBox(index, start, end)`.
pub fn range_box(index: &Index, start: f64, end: f64) -> Option<[f64; 4]> {
    let mut bx: Option<[f64; 4]> = None;
    for sp in &index.spans {
        let (ss, se) = (sp.start as f64, sp.end as f64);
        if se <= start || ss >= end {
            continue;
        }
        let Some(it) = index.items.get(sp.item) else { continue };
        let b = part_box(it, crate::js::max(0.0, start - ss), crate::js::min(se, end) - ss);
        bx = Some(match bx {
            Some(o) => [crate::js::min(o[0], b[0]), crate::js::min(o[1], b[1]), crate::js::max(o[2], b[2]), crate::js::max(o[3], b[3])],
            None => b,
        });
    }
    bx
}

/// One hit.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub start: usize,
    pub end: usize,
    pub rect: [f64; 4],
    pub snippet: String,
}

fn fold(c: char) -> char {
    match c {
        'ς' => 'σ',
        'ſ' => 's',
        'ϐ' => 'β',
        'ϑ' => 'θ',
        'ϕ' => 'φ',
        'ϖ' => 'π',
        'ϰ' => 'κ',
        'ϱ' => 'ρ',
        'ϵ' => 'ε',
        'ẛ' => 'ṡ',
        '\u{1fbe}' => 'ι',
        _ => {
            let mut l = c.to_lowercase();
            match (l.next(), l.next()) {
                (Some(x), None) => x,
                _ => c,
            }
        }
    }
}

fn is_ln(c: char) -> bool {
    c.is_alphabetic() || c.is_numeric()
}

/// `findHits(index, query, {matchCase, wholeWord, max})`.
pub fn find_hits(index: &Index, query: &str, match_case: bool, whole_word: bool, max: f64) -> Vec<Hit> {
    let q = crate::js::trim(query);
    if q.is_empty() || index.text.is_empty() {
        return Vec::new();
    }
    let words: Vec<Vec<char>> = q.split(is_ws).filter(|w| !w.is_empty()).map(|w| w.chars().collect()).collect();
    let chars: Vec<char> = index.text.chars().collect();
    // UTF-16 offset of every code point (and the end)
    let mut off: Vec<usize> = Vec::with_capacity(chars.len().saturating_add(1));
    let mut o = 0usize;
    for c in &chars {
        off.push(o);
        o = o.saturating_add(c.len_utf16());
    }
    off.push(o);
    let total16 = o;
    let eq = |a: char, b: char| if match_case { a == b } else { a == b || fold(a) == fold(b) };
    let try_at = |i: usize| -> Option<usize> {
        if whole_word && i > 0 && chars.get(i.wrapping_sub(1)).is_some_and(|c| is_ln(*c)) {
            return None;
        }
        let mut k = i;
        for (wi, w) in words.iter().enumerate() {
            if wi > 0 {
                let mut n = 0usize;
                while chars.get(k).is_some_and(|c| is_ws(*c)) {
                    k = k.saturating_add(1);
                    n = n.saturating_add(1);
                }
                if n == 0 {
                    return None;
                }
            }
            for wc in w {
                if !chars.get(k).is_some_and(|c| eq(*c, *wc)) {
                    return None;
                }
                k = k.saturating_add(1);
            }
        }
        if whole_word && chars.get(k).is_some_and(|c| is_ln(*c)) {
            return None;
        }
        Some(k)
    };
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        let Some(k) = try_at(i) else {
            i = i.saturating_add(1);
            continue;
        };
        let start = off.get(i).copied().unwrap_or(0);
        let end = off.get(k).copied().unwrap_or(total16);
        i = if k > i { k } else { i.saturating_add(1) };
        let Some(rect) = range_box(index, start as f64, end as f64) else { continue };
        let a = start.saturating_sub(30);
        let z = end.saturating_add(30).min(total16);
        let mid = slice_utf16(&index.text, a, z);
        let mut snippet = String::new();
        if a > 0 {
            snippet.push('…');
        }
        snippet.push_str(&collapse_ws(&mid));
        if z < total16 {
            snippet.push('…');
        }
        out.push(Hit { start, end, rect, snippet });
        if out.len() as f64 >= max {
            break;
        }
    }
    out
}

/// `s.slice(a, z)` in UTF-16 units (a cut through a surrogate pair gives U+FFFD — the JS keeps the lone half).
pub fn slice_utf16(s: &str, a: usize, z: usize) -> String {
    let u: Vec<u16> = s.encode_utf16().collect();
    String::from_utf16_lossy(u.get(a.min(u.len())..z.min(u.len()).max(a.min(u.len()))).unwrap_or(&[]))
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for c in s.chars() {
        if is_ws(c) {
            if !in_ws {
                out.push(' ');
            }
            in_ws = true;
        } else {
            out.push(c);
            in_ws = false;
        }
    }
    out
}

/// `hitCloud(hit, {pad, rot, sheet, author, subject})` → cloudrect fields: (rot, rect).
pub fn hit_cloud_rect(rect: [f64; 4], pad: f64) -> [f64; 4] {
    [rect[0] - pad, rect[1] - pad, rect[2] + pad, rect[3] + pad]
}
