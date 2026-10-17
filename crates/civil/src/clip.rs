//! clip.js — copying and pasting survey points: the clipboard record (untrusted on the way in), spreadsheet rows,
//! renumbering.

use crate::i18n::t;
use crate::js::{J, num_to_string, round_fixed, slice16, strip_controls, trim};
use crate::points::{Parsed, Pt, parse_points};
use crate::tr;
use serde_json::Value;

pub const CLIP_FORMAT: &str = "civilcraft-clip";
pub const MAX_CLIP: usize = 200_000;
const MAX_COORD: f64 = 1e9;

fn clean(s: Option<&Value>, n: usize) -> String {
    slice16(trim(&strip_controls(&crate::js::string_or_empty(s), " ")), n)
}
fn fin(v: Option<&Value>) -> Option<f64> {
    crate::js::fin(v).filter(|x| x.abs() < MAX_COORD)
}

/// A spreadsheet cell.
fn cell(s: &str) -> String {
    let b = s.as_bytes();
    let formula = matches!(b.first(), Some(b'=' | b'+' | b'-' | b'@'));
    let numeric =
        b.first().is_some_and(u8::is_ascii_digit) || (matches!(b.first(), Some(b'+' | b'-')) && b.get(1).is_some_and(u8::is_ascii_digit));
    let t = if formula && !numeric { format!("'{s}") } else { s.to_string() };
    if t.contains(['"', ',', '\r', '\n']) { format!("\"{}\"", t.replace('"', "\"\"")) } else { t }
}
fn num(v: Option<f64>) -> String {
    match v {
        None => String::new(),
        Some(x) => num_to_string(round_fixed(x, 3)),
    }
}

/// `pointsText(points, order)`.
pub fn points_text(points: &[Pt], order: &str) -> String {
    let pene = order == "PENZD";
    let rows: Vec<String> = points
        .iter()
        .map(|p| {
            [
                cell(&p.n),
                if pene { num(Some(p.x)) } else { num(Some(p.y)) },
                if pene { num(Some(p.y)) } else { num(Some(p.x)) },
                num(p.z),
                cell(&p.d),
            ]
            .join(",")
        })
        .collect();
    format!("{}\r\n", rows.join("\r\n"))
}

/// A clipboard record.
#[derive(Clone, Debug)]
pub struct Clip {
    pub units: String,
    pub at: f64,
    pub source: String,
    pub points: Vec<Pt>,
    pub csv: String,
}

/// `makeClip(points, { units, order, source })` (with `at` given: the JS reads the clock).
pub fn make_clip(points: &[Pt], units: &str, order: &str, source: &str, at: f64) -> Value {
    let take: Vec<Pt> = points.iter().take(MAX_CLIP).cloned().collect();
    let pts: Vec<Value> = take.iter().map(|p| Value::Array(vec![p.n.j(), p.x.j(), p.y.j(), p.z.j(), p.d.j()])).collect();
    crate::jo! {
        "format" => CLIP_FORMAT, "v" => 1.0, "units" => if units == "m" { "m" } else { "ft" }, "at" => at,
        "source" => slice16(trim(&strip_controls(source, " ")), 40), "pts" => pts, "csv" => points_text(&take, order),
    }
}

/// `readClip(obj)` (untrusted) → a clean clip or None.
pub fn read_clip(obj: &Value) -> Option<Clip> {
    if !obj.is_object()
        || obj.get("format").and_then(Value::as_str) != Some(CLIP_FORMAT)
        || obj.get("v").and_then(Value::as_f64) != Some(1.0)
        || !obj.get("pts").is_some_and(Value::is_array)
    {
        return None;
    }
    let mut seen = std::collections::HashSet::new();
    let mut pts = Vec::new();
    for r in crate::js::arr(obj.get("pts")).iter().take(MAX_CLIP) {
        let Some(ra) = r.as_array() else { continue };
        let (Some(x), Some(y)) = (fin(ra.get(1)), fin(ra.get(2))) else { continue };
        let n = clean(ra.first(), 40);
        if n.is_empty() || seen.contains(&n) {
            continue;
        }
        seen.insert(n.clone());
        let d0 = clean(ra.get(4), 200);
        let d = strip_quote(&d0);
        pts.push(Pt { n, x, y, z: fin(ra.get(3)), d, blk: None });
    }
    if pts.is_empty() {
        return None;
    }
    Some(Clip {
        units: if obj.get("units").and_then(Value::as_str) == Some("m") { "m".into() } else { "ft".into() },
        at: fin(obj.get("at")).unwrap_or(0.0),
        source: clean(obj.get("source"), 40),
        points: pts,
        csv: obj.get("csv").and_then(Value::as_str).map(|s| slice16(s, 50_000_000)).unwrap_or_default(),
    })
}
/// `.replace(/^'(?=[=+\-@])/, '')`.
fn strip_quote(s: &str) -> String {
    let b = s.as_bytes();
    if b.first() == Some(&b'\'') && matches!(b.get(1), Some(b'=' | b'+' | b'-' | b'@')) {
        s.get(1..).unwrap_or("").to_string()
    } else {
        s.to_string()
    }
}
impl J for Clip {
    fn j(&self) -> Value {
        crate::jo! { "format" => CLIP_FORMAT, "v" => 1.0, "units" => self.units, "at" => self.at, "source" => self.source, "points" => self.points, "csv" => self.csv }
    }
}

/// `textPoints(text, order)` → (points, parse result).
pub fn text_points(text: &str, order: &str) -> (Vec<Pt>, Parsed) {
    let text = slice16(text, 50_000_000);
    let mut res = parse_points(&text, "auto", order);
    for p in &mut res.points {
        p.d = strip_quote(&p.d);
    }
    (res.points.clone(), res)
}

fn same_text(a: &str, b: &str) -> bool {
    let norm = |s: &str| trim(&s.replace("\r\n", "\n").replace('\r', "\n")).to_string();
    norm(a) == norm(b)
}

/// `choosePaste(own, systemText, order)` → { kind, points, units?, why } (+ res for text).
pub fn choose_paste(own: Option<&Clip>, system_text: Option<&str>, order: &str) -> Value {
    let has_text = system_text.is_some_and(|s| !trim(s).is_empty());
    if let Some(st) = system_text.filter(|_| has_text)
        && !own.is_some_and(|o| same_text(st, &o.csv))
    {
        let (points, res) = text_points(st, order);
        if !points.is_empty() {
            return crate::jo! { "kind" => "text", "points" => points, "res" => res, "why" => t(if own.is_some() { "clip.why.newer" } else { "clip.why.text" }, &[]) };
        }
        if own.is_none() {
            let why =
                if res.bad_count > 0 { res.bad.first().map(|b| b.why.clone()).unwrap_or_default() } else { t("clip.why.norows", &[]) };
            return crate::jo! { "kind" => "none", "points" => Vec::<Value>::new(), "why" => tr!("clip.why.unread", why = why) };
        }
    }
    if let Some(o) = own {
        return crate::jo! { "kind" => "clip", "points" => o.points, "units" => o.units, "why" => t("clip.why.own", &[]) };
    }
    crate::jo! { "kind" => "none", "points" => Vec::<Value>::new(), "why" => t("clip.why.nothing", &[]) }
}

fn whole(n: &str) -> Option<f64> {
    (!n.is_empty() && n.len() <= 15 && n.bytes().all(|b| b.is_ascii_digit())).then(|| crate::js::str_to_number(n))
}

/// `renumber(incoming, existing, { mode, start })` → (points, changed [[old, new]], kept).
pub fn renumber(incoming: &[Pt], existing: &[String], mode: &str, start: Option<f64>) -> (Vec<Pt>, Vec<(String, String)>, usize) {
    let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut high = 0.0f64;
    for n in existing {
        used.insert(n.clone());
        if let Some(w) = whole(n)
            && w > high
        {
            high = w;
        }
    }
    let mut out: Vec<Option<Pt>> = vec![None; incoming.len()];
    let mut changed = Vec::new();
    let mut todo = Vec::new();
    let mut kept = 0usize;
    for (i, p) in incoming.iter().enumerate() {
        let n = p.n.clone();
        if mode != "always" && !used.contains(&n) {
            used.insert(n.clone());
            if let Some(slot) = out.get_mut(i) {
                *slot = Some(Pt { n: n.clone(), ..p.clone() });
            }
            kept = kept.saturating_add(1);
            if let Some(w) = whole(&n)
                && w > high
            {
                high = w;
            }
        } else {
            todo.push(i);
        }
    }
    let mut next = match start {
        Some(s) if crate::js::is_integer(s) && s > 0.0 => s,
        _ => high + 1.0,
    };
    for i in todo {
        let Some(p) = incoming.get(i) else { continue };
        let mut guard = 0u64;
        while used.contains(&num_to_string(next)) && guard < 100_000_000 {
            next += 1.0;
            guard = guard.saturating_add(1);
        }
        let n = num_to_string(next);
        next += 1.0;
        used.insert(n.clone());
        changed.push((p.n.clone(), n.clone()));
        if let Some(slot) = out.get_mut(i) {
            *slot = Some(Pt { n, ..p.clone() });
        }
    }
    (out.into_iter().flatten().collect(), changed, kept)
}

/// "10–18" style ranges of whole numbers, at most `max` pieces.
fn ranges(list: &[String], max: usize) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < list.len() {
        let mut j = i;
        let w = list.get(i).and_then(|s| whole(s));
        if w.is_some() {
            while j + 1 < list.len() {
                let (a, b) = (list.get(j).and_then(|s| whole(s)), list.get(j + 1).and_then(|s| whole(s)));
                match (a, b) {
                    (Some(a), Some(b)) if b == a + 1.0 => j += 1,
                    _ => break,
                }
            }
        }
        let first = list.get(i).cloned().unwrap_or_default();
        parts.push(if j > i { format!("{}–{}", first, list.get(j).cloned().unwrap_or_default()) } else { first });
        i = j + 1;
    }
    if parts.len() > max { format!("{} …", parts.iter().take(max).cloned().collect::<Vec<_>>().join(", ")) } else { parts.join(", ") }
}

/// `renumberText(changed, kept)`.
pub fn renumber_text(changed: &[(String, String)], kept: usize) -> String {
    let mut bits = Vec::new();
    if kept > 0 {
        bits.push(tr!("clip.kept", n = kept));
    }
    if !changed.is_empty() {
        let was: Vec<String> = changed.iter().map(|c| c.0.clone()).collect();
        let now: Vec<String> = changed.iter().map(|c| c.1.clone()).collect();
        bits.push(tr!("clip.changed", n = changed.len(), was = ranges(&was, 4), now = ranges(&now, 4)));
    }
    bits.join("; ")
}

/// `moveBy(points, dx, dy)`.
pub fn move_by(points: &[Pt], dx: f64, dy: f64) -> Vec<Pt> {
    points.iter().map(|p| Pt { x: p.x + dx, y: p.y + dy, ..p.clone() }).collect()
}
/// `centre(points)`.
pub fn centre(points: &[Pt]) -> crate::geo::P2 {
    let (mut a, mut b, mut c, mut d) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in points {
        a = crate::js::min(a, p.x);
        b = crate::js::min(b, p.y);
        c = crate::js::max(c, p.x);
        d = crate::js::max(d, p.y);
    }
    if points.is_empty() { crate::geo::p2(0.0, 0.0) } else { crate::geo::p2((a + c) / 2.0, (b + d) / 2.0) }
}
