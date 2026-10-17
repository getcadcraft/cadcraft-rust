//! The DOM-free halves of pages.js (page orders and typed page lists), blank.js (File › New: papers, grids, the
//! blank-set note) and bookmarks.js (bookmarks from data and from the sheets). The PDF work of the three (pdf-lib)
//! is in `cc-io-markup`.

use crate::i18n::{Fill, t, t0};
use crate::js::{num_to_string, round, slice16, to_number, trim};
use serde_json::Value;

// ---- pages.js ----

fn uniq_sorted(idx: &[f64], n: f64) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::new();
    for &i in idx {
        if i.fract() == 0.0 && i >= 0.0 && i < n && !out.contains(&i) {
            out.push(i);
        }
    }
    out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Most pages a page order is made for (a hostile `n` cannot ask for a giant list).
pub const MAX_ORDER: f64 = 1_000_000.0;

fn range(a: f64, b: f64) -> impl Iterator<Item = f64> {
    let mut i = a;
    std::iter::from_fn(move || {
        if i < b {
            let v = i;
            i += 1.0;
            Some(v)
        } else {
            None
        }
    })
}

/// `orderDelete(n, indices)` → old index per new page.
pub fn order_delete(n: f64, indices: &[f64]) -> Vec<f64> {
    let del = uniq_sorted(indices, n);
    range(0.0, n.min(MAX_ORDER)).filter(|i| !del.contains(i)).collect()
}

/// `orderMove(n, indices, to)`.
pub fn order_move(n: f64, indices: &[f64], to: f64) -> Vec<f64> {
    let mv = uniq_sorted(indices, n);
    let rest: Vec<f64> = range(0.0, n.min(MAX_ORDER)).filter(|i| !mv.contains(i)).collect();
    let at = crate::js::max(0.0, crate::js::min(rest.len() as f64, to));
    let at = if at.is_nan() { 0 } else { crate::js::f2usize(at) };
    let (a, b) = rest.split_at(at.min(rest.len()));
    a.iter().chain(mv.iter()).chain(b.iter()).copied().collect()
}

/// `orderInsert(n, at, count)` (None = an inserted page).
pub fn order_insert(n: f64, at: f64, count: f64) -> Vec<Option<f64>> {
    let a = crate::js::max(0.0, crate::js::min(n, at));
    let mut out: Vec<Option<f64>> = range(0.0, a.min(MAX_ORDER)).map(Some).collect();
    out.extend(range(0.0, count.min(MAX_ORDER)).map(|_| None));
    out.extend(range(0.0, (n - a).min(MAX_ORDER)).map(|i| Some(i + a)));
    out
}

/// `parsePageList(text, n)` → 0-based indices, or the error text.
pub fn parse_page_list(text: &str, n: f64) -> Result<Vec<f64>, String> {
    let s = trim(text);
    if s.is_empty() {
        return Err(t0("pages.err.list_empty"));
    }
    let mut out: Vec<f64> = Vec::new();
    for part in s.split(|c: char| c == ',' || c == ';' || crate::js::is_ws(c)).filter(|p| !p.is_empty()) {
        let bad = || t("pages.err.list_bad", &[("part", Fill::S(part.to_string()))]);
        // ^(\d+)?\s*(?:(-|–)\s*(\d+)?)?$ — the part holds no whitespace (it was split on it)
        let d1_len = part.bytes().take_while(u8::is_ascii_digit).count();
        let (d1, rest) = part.split_at(d1_len);
        let (dash, d3) = if rest.is_empty() {
            (false, "")
        } else if let Some(r) = rest.strip_prefix('-').or_else(|| rest.strip_prefix('–')) {
            if !r.bytes().all(|b| b.is_ascii_digit()) {
                return Err(bad());
            }
            (true, r)
        } else {
            return Err(bad());
        };
        if d1.is_empty() && d3.is_empty() {
            return Err(bad());
        }
        let a = if d1.is_empty() { 1.0 } else { crate::js::str_to_number(d1) };
        let b = if dash { if d3.is_empty() { n } else { crate::js::str_to_number(d3) } } else { a };
        if a < 1.0 || b < 1.0 || a > n || b > n {
            return Err(t("pages.err.list_range", &[("part", Fill::S(part.to_string())), ("n", Fill::N(n))]));
        }
        let (lo, hi) = (a.min(b), a.max(b));
        let mut i = lo;
        while i <= hi && out.len() < 1_000_000 {
            out.push(i - 1.0);
            i += 1.0;
        }
    }
    let mut seen: Vec<f64> = Vec::new();
    for i in out {
        if !seen.contains(&i) {
            seen.push(i);
        }
    }
    Ok(seen)
}

// ---- blank.js ----

/// Info key of a blank set.
pub const BLANK_KEY: &str = "MarkupCraftBlank";
const IN: f64 = 72.0;
const MM: f64 = 72.0 / 25.4;

/// One paper: key, width, height (pt, portrait), metric.
pub struct Paper {
    pub key: &'static str,
    pub w: f64,
    pub h: f64,
    pub metric: bool,
}

/// `PAPERS` in their order.
pub fn papers() -> [Paper; 7] {
    [
        Paper { key: "letter", w: 8.5 * IN, h: 11.0 * IN, metric: false },
        Paper { key: "tabloid", w: 11.0 * IN, h: 17.0 * IN, metric: false },
        Paper { key: "arch-d", w: 24.0 * IN, h: 36.0 * IN, metric: false },
        Paper { key: "arch-e", w: 36.0 * IN, h: 48.0 * IN, metric: false },
        Paper { key: "a4", w: 210.0 * MM, h: 297.0 * MM, metric: true },
        Paper { key: "a3", w: 297.0 * MM, h: 420.0 * MM, metric: true },
        Paper { key: "a1", w: 594.0 * MM, h: 841.0 * MM, metric: true },
    ]
}
pub const ORIENTS: [&str; 2] = ["portrait", "landscape"];
pub const GRIDS: [&str; 3] = ["none", "dot", "square"];
pub const MAX_PAGES: f64 = 20.0;

/// A cleaned blank-set request.
#[derive(Debug, Clone, PartialEq)]
pub struct BlankSpec {
    pub paper: &'static str,
    pub orient: &'static str,
    pub grid: &'static str,
    pub pages: f64,
}

fn paper_of(key: &str) -> Paper {
    papers().into_iter().find(|p| p.key == key).unwrap_or(Paper { key: "letter", w: 8.5 * IN, h: 11.0 * IN, metric: false })
}

/// `cleanBlank(raw)`.
pub fn clean_blank(raw: Option<&Value>) -> BlankSpec {
    let o = raw.and_then(Value::as_object);
    let get = |k: &str| o.and_then(|o| o.get(k));
    let s = |k: &str| get(k).and_then(Value::as_str);
    let pages = round(get("pages").map_or(f64::NAN, to_number));
    let paper = s("paper").and_then(|p| papers().into_iter().find(|x| x.key == p)).map_or("letter", |p| p.key);
    BlankSpec {
        paper,
        orient: s("orient").and_then(|v| ORIENTS.iter().find(|x| **x == v)).copied().unwrap_or("portrait"),
        grid: s("grid").and_then(|v| GRIDS.iter().find(|x| **x == v)).copied().unwrap_or("none"),
        pages: if pages.is_finite() { crate::js::max(1.0, crate::js::min(MAX_PAGES, pages)) } else { 1.0 },
    }
}

/// `paperSize(spec)` → [w, h] pt.
pub fn paper_size(spec: &BlankSpec) -> [f64; 2] {
    let p = paper_of(spec.paper);
    if spec.orient == "landscape" { [p.h, p.w] } else { [p.w, p.h] }
}

/// Whether a paper is metric.
pub fn paper_metric(key: &str) -> bool {
    paper_of(key).metric
}

fn f2(n: f64) -> String {
    num_to_string(round(n * 100.0) / 100.0)
}

/// `gridContent(w, h, grid, metric)`.
pub fn grid_content(w: f64, h: f64, grid: &str, metric: bool) -> String {
    if grid != "dot" && grid != "square" {
        return String::new();
    }
    let (step, margin, major) = if metric { (5.0 * MM, 10.0 * MM, 2.0) } else { (IN / 4.0, IN / 2.0, 4.0) };
    let nx = ((w - 2.0 * margin) / step + 1e-6).floor();
    let ny = ((h - 2.0 * margin) / step + 1e-6).floor();
    if !(nx >= 1.0) || !(ny >= 1.0) {
        return String::new();
    }
    // Deviation (hostile input only): a page that would need more than 4 million grid marks gets no grid
    // (the JS would try to build them all). Letter needs 1 271 dots, ARCH E 27 000.
    if (nx + 1.0) * (ny + 1.0) > 4.0e6 {
        return String::new();
    }
    let x0 = (w - nx * step) / 2.0;
    let y0 = (h - ny * step) / 2.0;
    let x1 = x0 + nx * step;
    let y1 = y0 + ny * step;
    let mut out: Vec<String> = vec!["q".into()];
    if grid == "square" {
        for heavy in [false, true] {
            out.push(if heavy { "0.62 G 0.5 w".into() } else { "0.82 G 0.3 w".into() });
            let mut i = 0.0;
            while i <= nx {
                if (i % major == 0.0) == heavy {
                    let x = f2(x0 + i * step);
                    out.push(format!("{x} {} m {x} {} l", f2(y0), f2(y1)));
                }
                i += 1.0;
            }
            let mut j = 0.0;
            while j <= ny {
                if (j % major == 0.0) == heavy {
                    let y = f2(y0 + j * step);
                    out.push(format!("{} {y} m {} {y} l", f2(x0), f2(x1)));
                }
                j += 1.0;
            }
            out.push("S".into());
        }
    } else {
        out.push("0.55 g".into());
        let mut i = 0.0;
        while i <= nx {
            let mut j = 0.0;
            while j <= ny {
                let d = if i % major == 0.0 && j % major == 0.0 { 1.6 } else { 1.0 };
                out.push(format!(
                    "{} {} {} {} re",
                    f2(x0 + i * step - d / 2.0),
                    f2(y0 + j * step - d / 2.0),
                    num_to_string(d),
                    num_to_string(d)
                ));
                j += 1.0;
            }
            i += 1.0;
        }
        out.push("f".into());
    }
    out.push("Q".into());
    out.join("\n")
}

/// What `blankInfo` reads back.
#[derive(Debug, Clone, PartialEq)]
pub struct BlankInfo {
    pub paper: &'static str,
    pub orient: &'static str,
    pub grid: &'static str,
    pub metric: bool,
}

/// `blankInfo(raw)` (raw = the Info value as text; None = absent).
pub fn blank_info(raw: Option<&str>) -> Option<BlankInfo> {
    let raw = raw.filter(|r| !r.is_empty())?;
    let v: Value = serde_json::from_str(raw).ok()?;
    if !(v.is_object() || v.is_array()) {
        return None;
    }
    let s = clean_blank(Some(&v));
    Some(BlankInfo { paper: s.paper, orient: s.orient, grid: s.grid, metric: paper_metric(s.paper) })
}

/// `untitledName(names, seq)` → (name, seq).
pub fn untitled_name(names: &[String], seq: f64) -> (String, f64) {
    let mut n = seq;
    for nm in names {
        let l = nm.to_ascii_lowercase();
        let Some(rest) = l.strip_prefix("untitled-") else { continue };
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            continue;
        }
        let tail = rest.get(digits..).unwrap_or("");
        if tail == ".pdf" || tail == " (marked up).pdf" {
            n = crate::js::max(n, crate::js::str_to_number(rest.get(..digits).unwrap_or("")));
        }
    }
    (format!("Untitled-{}.pdf", num_to_string(n + 1.0)), n + 1.0)
}

// ---- bookmarks.js ----

/// Most bookmarks kept.
pub const BOOKMARK_LIMIT: usize = 2000;

/// A bookmark: title, page (0-based, -1 = nowhere), children.
#[derive(Debug, Clone, PartialEq)]
pub struct Bookmark {
    pub title: String,
    pub page: f64,
    pub children: Vec<Bookmark>,
}

/// `plain(v)`: controls → spaces, trimmed, ≤ 200 UTF-16 units.
pub fn plain(v: &str) -> String {
    let s: String = v.chars().map(|c| if u32::from(c) <= 0x1f { ' ' } else { c }).collect();
    slice16(trim(&s), 200)
}

fn plain_value(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(x) => plain(&crate::js::to_js_string(x)),
    }
}

/// `cleanBookmarks(v)`.
pub fn clean_bookmarks(v: Option<&Value>, depth: u32) -> Vec<Bookmark> {
    let Some(a) = v.and_then(Value::as_array) else { return Vec::new() };
    a.iter()
        .take(BOOKMARK_LIMIT)
        .filter_map(|b| {
            let title = match b {
                Value::Object(o) => plain_value(o.get("title")),
                Value::Array(_) => String::new(),
                _ => return None,
            };
            if title.is_empty() {
                return None;
            }
            let o = b.as_object();
            let page = o.and_then(|o| o.get("page")).and_then(Value::as_f64).filter(|p| p.fract() == 0.0 && *p >= -1.0).unwrap_or(-1.0);
            let children = if depth < 2 { clean_bookmarks(o.and_then(|o| o.get("children")), depth.saturating_add(1)) } else { Vec::new() };
            Some(Bookmark { title, page, children })
        })
        .collect()
}

/// `fromSheets(pages)`.
pub fn from_sheets(pages: &[(f64, String)]) -> Vec<Bookmark> {
    pages.iter().map(|(index, title)| Bookmark { title: plain(title), page: *index, children: Vec::new() }).collect()
}

/// `count(items)`.
pub fn count_bookmarks(items: &[Bookmark]) -> usize {
    items.iter().map(|b| count_bookmarks(&b.children).saturating_add(1)).fold(0usize, usize::saturating_add)
}
