//! titleblock.js — a sheet's number and title from the page's own text (pdf.js `getTextContent` items placed in the
//! viewed sheet). The JS regular expressions are matched here by a small backtracking matcher that tries the
//! alternatives in the same order, so the same capture wins.

use crate::js::{hypot, is_ws, len16, slice16, trim, upper};

/// A text item: its string, its text matrix (with the font size) and its width in text space (pdf.js units).
#[derive(Debug, Clone, PartialEq)]
pub struct TextPiece {
    pub str: String,
    pub transform: [f64; 6],
    pub width: f64,
}

/// The scale-1 viewport of a page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub transform: [f64; 6],
    pub width: f64,
    pub height: f64,
}

/// The result: number and title (either may be '').
#[derive(Debug, Clone, PartialEq)]
pub struct TitleBlock {
    pub number: String,
    pub title: String,
}

// ---- a tiny regex: every way a pattern can end, in the engine's order of preference ----

fn is_word(c: Option<&char>) -> bool {
    c.is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
}

/// Case-insensitive (ASCII) literal at `pos`.
fn lit(s: &[char], pos: usize, l: &str) -> Vec<usize> {
    let mut i = pos;
    for c in l.chars() {
        match s.get(i) {
            Some(x) if x.eq_ignore_ascii_case(&c) => i = i.saturating_add(1),
            _ => return Vec::new(),
        }
    }
    vec![i]
}
/// `\s*` (greedy: longest first).
fn ws_star(s: &[char], pos: usize) -> Vec<usize> {
    let mut e = pos;
    while s.get(e).is_some_and(|c| is_ws(*c)) {
        e = e.saturating_add(1);
    }
    (pos..=e).rev().collect()
}
/// `\s+`
fn ws_plus(s: &[char], pos: usize) -> Vec<usize> {
    let mut v = ws_star(s, pos);
    v.retain(|e| *e > pos);
    v
}
fn seq(s: &[char], ends: Vec<usize>, next: impl Fn(&[char], usize) -> Vec<usize>) -> Vec<usize> {
    ends.into_iter().flat_map(|e| next(s, e)).collect()
}
fn opt(s: &[char], pos: usize, f: impl Fn(&[char], usize) -> Vec<usize>) -> Vec<usize> {
    let mut v = f(s, pos);
    v.push(pos);
    v
}
fn alts(s: &[char], pos: usize, words: &[&str]) -> Vec<usize> {
    words.iter().flat_map(|w| lit(s, pos, w)).collect()
}
fn one_of(s: &[char], pos: usize, set: &[char]) -> Vec<usize> {
    if s.get(pos).is_some_and(|c| set.contains(c)) { vec![pos.saturating_add(1)] } else { Vec::new() }
}

/// The NUMBER capture `[A-Z]{1,3}[-.]?\d{1,3}(?:[.-]\d{1,3})?[A-Z]?` over the whole of `s` (ASCII letters, any case).
fn cap_match(s: &[char]) -> bool {
    let n = s.len();
    let mut i = 0usize;
    while i < n && s.get(i).is_some_and(char::is_ascii_alphabetic) {
        i = i.saturating_add(1);
    }
    if !(1..=3).contains(&i) {
        return false;
    }
    if s.get(i).is_some_and(|c| *c == '-' || *c == '.') && s.get(i.saturating_add(1)).is_some_and(char::is_ascii_digit) {
        i = i.saturating_add(1);
    }
    let d0 = i;
    while i < n && s.get(i).is_some_and(char::is_ascii_digit) {
        i = i.saturating_add(1);
    }
    if !(1..=3).contains(&(i.saturating_sub(d0))) {
        return false;
    }
    if s.get(i).is_some_and(|c| *c == '.' || *c == '-') && s.get(i.saturating_add(1)).is_some_and(char::is_ascii_digit) {
        let j0 = i.saturating_add(1);
        let mut j = j0;
        while j < n && s.get(j).is_some_and(char::is_ascii_digit) {
            j = j.saturating_add(1);
        }
        if (1..=3).contains(&(j.saturating_sub(j0))) {
            // the optional group taken (greedy); if what follows fails, the engine would try without it, which then
            // fails too ('.' / '-' is neither a letter nor the end)
            i = j;
        } else {
            return false;
        }
    }
    if s.get(i).is_some_and(char::is_ascii_alphabetic) {
        i = i.saturating_add(1);
    }
    i == n
}

/// The ends of `sheet(?:\s*(?:no\.?|number|#))?\s*[:.]?\s*`.
fn sheet_prefix(s: &[char]) -> Vec<usize> {
    let after_sheet = lit(s, 0, "sheet");
    let mut out = Vec::new();
    for e in after_sheet {
        let g = opt(s, e, |s, p| seq(s, ws_star(s, p), |s, q| alts(s, q, &["no.", "no", "number", "#"])));
        let w2 = seq(s, g, ws_star);
        let c = seq(s, w2, |s, p| opt(s, p, |s, q| one_of(s, q, &[':', '.'])));
        out.extend(seq(s, c, ws_star));
    }
    out
}

/// `NUMBER.exec(text)` → the captured sheet number.
pub fn number_capture(text: &str) -> Option<String> {
    let s: Vec<char> = text.chars().collect();
    let mut ends = sheet_prefix(&s);
    ends.push(0);
    for e in ends {
        let rest = s.get(e..).unwrap_or(&[]);
        if cap_match(rest) {
            return Some(rest.iter().collect());
        }
    }
    None
}

/// `BOILER.test(text)`.
fn boiler(text: &str) -> bool {
    let s: Vec<char> = text.chars().collect();
    let s = s.as_slice();
    let by = |s: &[char], p: usize| seq(s, ws_star(s, p), |s, q| lit(s, q, "by"));
    let mut ends: Vec<usize> = Vec::new();
    // sheet(\s*(no\.?|number|#|title))?
    for e in lit(s, 0, "sheet") {
        ends.extend(opt(s, e, |s, p| seq(s, ws_star(s, p), |s, q| alts(s, q, &["no.", "no", "number", "#", "title"]))));
    }
    ends.extend(lit(s, 0, "scale"));
    ends.extend(lit(s, 0, "date"));
    for w in ["drawn", "checked", "designed"] {
        for e in lit(s, 0, w) {
            ends.extend(opt(s, e, by));
        }
    }
    for e in lit(s, 0, "project") {
        ends.extend(opt(s, e, |s, p| seq(s, ws_star(s, p), |s, q| alts(s, q, &["no.", "no", "number", "name"]))));
    }
    for e in lit(s, 0, "job") {
        ends.extend(opt(s, e, |s, p| seq(s, ws_star(s, p), |s, q| alts(s, q, &["no.", "no"]))));
    }
    ends.extend(lit(s, 0, "title"));
    ends.extend(alts(s, 0, &["revisions", "revision"]));
    ends.extend(alts(s, 0, &["rev.", "rev"]));
    for e in alts(s, 0, &["issued", "issue"]) {
        ends.extend(opt(s, e, |s, p| seq(s, ws_star(s, p), |s, q| lit(s, q, "for"))));
    }
    ends.extend(alts(s, 0, &["no.", "no", "by", "description"]));
    for e in lit(s, 0, "approved") {
        ends.extend(opt(s, e, by));
    }
    ends.extend(alts(s, 0, &["seal", "client", "owner", "architect", "engineer", "consultant"]));
    for e in lit(s, 0, "key") {
        ends.extend(seq(s, ws_star(s, e), |s, q| lit(s, q, "plan")));
    }
    ends.extend(lit(s, 0, "north"));
    for e in lit(s, 0, "not") {
        let a = seq(s, ws_plus(s, e), |s, q| lit(s, q, "for"));
        ends.extend(seq(s, a, |s, q| seq(s, ws_plus(s, q), |s, r| lit(s, r, "construction"))));
    }
    for e in lit(s, 0, "for") {
        ends.extend(seq(s, ws_plus(s, e), |s, q| lit(s, q, "construction")));
    }
    // \b
    ends.into_iter().any(|e| is_word(e.checked_sub(1).and_then(|k| s.get(k))) != is_word(s.get(e)))
}

fn digits_run(s: &[char], i: usize) -> usize {
    let mut j = i;
    while s.get(j).is_some_and(char::is_ascii_digit) {
        j = j.saturating_add(1);
    }
    j.saturating_sub(i)
}

/// `/^\d{1,2}[/.-]\d{1,2}[/.-]\d{2,4}$/`
fn is_date(t: &str) -> bool {
    let s: Vec<char> = t.chars().collect();
    let sep = |c: Option<&char>| c.is_some_and(|c| matches!(c, '/' | '.' | '-'));
    let a = digits_run(&s, 0);
    if !(1..=2).contains(&a) || !sep(s.get(a)) {
        return false;
    }
    let b0 = a.saturating_add(1);
    let b = digits_run(&s, b0);
    if !(1..=2).contains(&b) || !sep(s.get(b0.saturating_add(b))) {
        return false;
    }
    let c0 = b0.saturating_add(b).saturating_add(1);
    let c = digits_run(&s, c0);
    (2..=4).contains(&c) && c0.saturating_add(c) == s.len()
}

/// `/^\d+\s*\/\s*\d+"?\s*=/` or `/^1\s*:\s*\d+$/`
fn is_scale(t: &str) -> bool {
    let s: Vec<char> = t.chars().collect();
    let skip = |mut i: usize| {
        while s.get(i).is_some_and(|c| is_ws(*c)) {
            i = i.saturating_add(1);
        }
        i
    };
    let a = digits_run(&s, 0);
    if a > 0 {
        let i = skip(a);
        if s.get(i) == Some(&'/') {
            let j = skip(i.saturating_add(1));
            let b = digits_run(&s, j);
            if b > 0 {
                let mut k = j.saturating_add(b);
                if s.get(k) == Some(&'"') {
                    k = k.saturating_add(1);
                }
                if s.get(skip(k)) == Some(&'=') {
                    return true;
                }
            }
        }
    }
    if s.first() == Some(&'1') {
        let i = skip(1);
        if s.get(i) == Some(&':') {
            let j = skip(i.saturating_add(1));
            let b = digits_run(&s, j);
            return b > 0 && j.saturating_add(b) == s.len();
        }
    }
    false
}

/// `isWording(text)`: some letters, not boilerplate, not a date, number or sheet number.
pub fn is_wording(text: &str) -> bool {
    let t = trim(text);
    let n = len16(t);
    if !(3..=120).contains(&n) {
        return false;
    }
    if t.chars().filter(char::is_ascii_alphabetic).count() < 3 {
        return false;
    }
    if boiler(t) || number_capture(t).is_some() || is_date(t) || is_scale(t) {
        return false;
    }
    true
}

#[derive(Debug, Clone)]
struct Piece {
    str: String,
    x: f64,
    y: f64,
    size: f64,
    horizontal: bool,
    width: f64,
}

#[derive(Debug, Clone)]
struct Line {
    text: String,
    x: f64,
    y: f64,
    size: f64,
    width: f64,
    horizontal: bool,
}

/// `text.replace(/\s+/g, ' ').trim()`
fn squash(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut ws = false;
    for c in s.chars() {
        if is_ws(c) {
            if !ws {
                out.push(' ');
            }
            ws = true;
        } else {
            out.push(c);
            ws = false;
        }
    }
    trim(&out).to_string()
}

fn join_lines(pieces: &[Piece]) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    let mut hz: Vec<&Piece> = pieces.iter().filter(|p| p.horizontal).collect();
    cc_m3d::js::sort_by(&mut hz, |p, q| {
        let d = p.y - q.y;
        if d != 0.0 && !d.is_nan() { d } else { p.x - q.x }
    });
    for p in hz {
        let found = out.iter_mut().find(|l| {
            l.horizontal
                && (l.y - p.y).abs() < p.size * 0.35
                && (l.size - p.size).abs() <= p.size * 0.15
                && p.x - (l.x + l.width) < p.size * 1.2
                && p.x >= l.x - p.size * 0.2
        });
        match found {
            Some(line) => {
                let gap = p.x - (line.x + line.width);
                if gap > p.size * 0.15 && !line.text.ends_with(' ') && !p.str.starts_with(' ') {
                    line.text.push(' ');
                }
                line.text.push_str(&p.str);
                line.width = crate::js::max(line.width, p.x + p.width - line.x);
            }
            None => out.push(Line { text: p.str.clone(), x: p.x, y: p.y, size: p.size, width: p.width, horizontal: true }),
        }
    }
    for p in pieces.iter().filter(|p| !p.horizontal) {
        out.push(Line { text: p.str.clone(), x: p.x, y: p.y, size: p.size, width: 0.0, horizontal: false });
    }
    for l in &mut out {
        l.text = squash(&l.text);
    }
    out
}

/// `readTitleBlock(items, viewport)` → None when the page has no usable text.
pub fn read_title_block(items: &[TextPiece], vp: &Viewport) -> Option<TitleBlock> {
    let t = vp.transform;
    let (w, h) = (vp.width, vp.height);
    let mut pieces = Vec::new();
    for it in items {
        if trim(&it.str).is_empty() {
            continue;
        }
        let [a, b, c, d, e, f] = it.transform;
        let x = t[0] * e + t[2] * f + t[4];
        let y = t[1] * e + t[3] * f + t[5];
        let dx = t[0] * a + t[2] * b;
        let dy = t[1] * a + t[3] * b;
        let size = hypot(t[0] * c + t[2] * d, t[1] * c + t[3] * d);
        if !(size > 0.5) {
            continue;
        }
        let horizontal = dy.abs() < dx.abs() * 0.2 && dx > 0.0;
        let iw = if it.width == 0.0 || it.width.is_nan() { 0.0 } else { it.width };
        let width = iw * hypot(t[0], t[1]);
        #[allow(clippy::cast_precision_loss)]
        let fallback = len16(&it.str) as f64 * size * 0.5;
        pieces.push(Piece {
            str: it.str.clone(),
            x,
            y,
            size,
            horizontal,
            width: if width == 0.0 || width.is_nan() { fallback } else { width },
        });
    }
    if pieces.is_empty() {
        return None;
    }
    let lines = join_lines(&pieces);
    let region = |l: &Line| -> f64 {
        let cx = l.x + if l.horizontal { l.width / 2.0 } else { 0.0 };
        let cy = l.y;
        let right = cx > w * 0.6;
        let bottom = cy > h * 0.6;
        if right && bottom {
            3.0
        } else if cx > w * 0.75 || cy > h * 0.8 {
            2.0
        } else {
            1.0
        }
    };
    let mut best: Option<(usize, String, f64)> = None;
    for (i, l) in lines.iter().enumerate() {
        let Some(n) = number_capture(trim(&l.text)) else { continue };
        let score = l.size * region(l);
        if best.as_ref().is_none_or(|b| score > b.2) {
            best = Some((i, upper(&n), score));
        }
    }
    let (ax, ay, aw) = match &best {
        Some((i, ..)) => lines.get(*i).map_or((0.0, 0.0, 0.0), |l| (l.x, l.y, l.width)),
        None => (w * 0.85, h * 0.9, 0.0),
    };
    let best_line = best.as_ref().map(|b| b.0);
    let best_region = best_line.and_then(|i| lines.get(i)).map(region);
    let near = |l: &Line| ((l.x + l.width / 2.0) - (ax + aw / 2.0)).abs() < w * 0.3 && (l.y - ay).abs() < h * 0.35;
    let dist = |l: &Line| hypot(l.x - ax, l.y - ay);
    let mut cands: Vec<usize> = (0..lines.len())
        .filter(|&i| {
            let Some(l) = lines.get(i) else { return false };
            Some(i) != best_line && near(l) && is_wording(&l.text) && best_region.is_none_or(|br| region(l) >= crate::js::min(2.0, br))
        })
        .collect();
    let mut title = String::new();
    if !cands.is_empty() {
        let get = |i: usize| lines.get(i);
        cc_m3d::js::sort_by(&mut cands, |&p, &q| {
            let (Some(p), Some(q)) = (get(p), get(q)) else { return 0.0 };
            let d = q.size - p.size;
            if d != 0.0 && !d.is_nan() { d } else { dist(p) - dist(q) }
        });
        let top_i = cands.first().copied().unwrap_or(0);
        if let Some(top) = get(top_i) {
            let twin: Vec<usize> = cands
                .iter()
                .copied()
                .filter(|&i| {
                    let Some(l) = get(i) else { return false };
                    i != top_i
                        && (l.size - top.size).abs() <= top.size * 0.1
                        && (l.y - top.y).abs() <= top.size * 1.7
                        && l.x < top.x + top.width
                        && l.x + l.width > top.x
                })
                .collect();
            let mut parts: Vec<usize> = vec![top_i];
            parts.extend(twin.into_iter().take(1));
            cc_m3d::js::sort_by(&mut parts, |&p, &q| match (get(p), get(q)) {
                (Some(p), Some(q)) => p.y - q.y,
                _ => 0.0,
            });
            let joined: Vec<String> = parts.iter().filter_map(|&i| get(i)).map(|l| trim(&l.text).to_string()).collect();
            title = slice16(&squash_inner(&joined.join(" ")), 120);
        }
    }
    if best.is_none() && title.is_empty() {
        return None;
    }
    Some(TitleBlock { number: best.map(|b| b.1).unwrap_or_default(), title })
}

/// `.replace(/\s+/g, ' ')` (no trim).
fn squash_inner(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut ws = false;
    for c in s.chars() {
        if is_ws(c) {
            if !ws {
                out.push(' ');
            }
            ws = true;
        } else {
            out.push(c);
            ws = false;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numbers_and_words() {
        assert_eq!(number_capture("A-101").as_deref(), Some("A-101"));
        assert_eq!(number_capture("SHEET NO. A-101").as_deref(), Some("A-101"));
        assert_eq!(number_capture("SHEET NO1").as_deref(), Some("NO1"));
        assert_eq!(number_capture("KA1-1").as_deref(), Some("KA1-1"));
        assert_eq!(number_capture("M-201A").as_deref(), Some("M-201A"));
        assert_eq!(number_capture("A1.01").as_deref(), Some("A1.01"));
        assert!(number_capture("ABCD1").is_none());
        assert!(is_wording("FIRST FLOOR PLAN"));
        assert!(!is_wording("SCALE: 1/4\" = 1'-0\""));
        assert!(!is_wording("09/21/2016"));
        assert!(!is_wording("A-101"));
        assert!(!is_wording("DRAWN BY"));
    }
}
