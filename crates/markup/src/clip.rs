//! clip.js — MarkupCraft's clipboard: copied markups, ready to paste on any sheet. A clip is
//! `{format: 'markupcraft-clip', v: 1, source, sheet, at, items}`; items keep only what draws a markup. Whatever is
//! read back (another tab, localStorage) is untrusted and checked field by field (`cleanClip`).

use crate::holes::clean_holes;
use crate::imagestamp::clean_image_value;
use crate::js::{len16, slice16, str_or_empty, to_iso};
use crate::model::{Env, Markup, P, geometry_ok, is_type, make_markup};
use serde_json::{Map, Value};

pub const CLIP_FORMAT: &str = "markupcraft-clip";
pub const CLIP_KEY: &str = "markupcraft.clipboard";
pub const CLIP_MAX: usize = 2000;
const LIMIT: f64 = 1e7;

/// A clip item: what draws a markup (`itemOf`), kept as the JSON object the JS builds.
pub type ClipItem = Map<String, Value>;

/// A clip.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    pub source: String,
    pub sheet: String,
    pub at: String,
    pub items: Vec<ClipItem>,
    /// `skipped` (cleanClip only)
    pub skipped: Option<usize>,
}

/// clip.js `str(v, max)`: control characters (not tab / LF / CR) and DEL dropped, ≤ max UTF-16 units.
fn cstr(v: Option<&Value>, max: usize) -> String {
    let s: String = str_or_empty(v)
        .chars()
        .filter(|c| {
            let u = *c as u32;
            !((u < 0x20 && u != 0x09 && u != 0x0a && u != 0x0d) || u == 0x7f)
        })
        .collect();
    slice16(&s, max)
}

fn fin(v: &Value) -> bool {
    v.as_f64().is_some_and(|x| x.is_finite() && x.abs() < LIMIT)
}
fn ok_pts(p: &Value) -> bool {
    p.as_array().is_some_and(|a| a.len() <= 100_000 && a.iter().all(|q| q.as_array().is_some_and(|q| q.len() == 2 && q.iter().all(fin))))
}
fn ok_rect(r: &Value) -> bool {
    r.as_array().is_some_and(|a| a.len() == 4 && a.iter().all(fin))
}
fn clamp_n(v: Option<&Value>, lo: f64, hi: f64) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|x| x.is_finite()).map(|x| crate::js::max(lo, crate::js::min(hi, x)))
}

fn num_v(x: f64) -> Value {
    serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number)
}
fn pts_v(p: &[P]) -> Value {
    crate::geom::pts_value(p)
}

/// `itemOf(m)`.
pub fn item_of(m: &Markup) -> ClipItem {
    let mv = crate::json::markup_value(m);
    let mut o = Map::new();
    o.insert("type".into(), Value::String(m.kind.clone()));
    o.insert("pts".into(), m.pts.as_ref().map_or(Value::Null, |p| pts_v(p)));
    o.insert("rect".into(), m.rect.as_ref().map_or(Value::Null, |r| crate::geom::nums_value(r)));
    o.insert("rot".into(), num_v(if m.rot == 0.0 || m.rot.is_nan() { 0.0 } else { m.rot }));
    o.insert("style".into(), mv.get("style").cloned().unwrap_or(Value::Object(Map::new())));
    o.insert("text".into(), Value::String(m.text.clone()));
    o.insert("label".into(), Value::String(m.label.clone()));
    o.insert("subject".into(), Value::String(m.subject.clone()));
    o.insert("comment".into(), Value::String(m.comment.clone()));
    o.insert("layer".into(), Value::String(m.layer.clone()));
    o.insert("link".into(), Value::String(m.link.clone()));
    if let Some(l) = mv.get("legend") {
        o.insert("legend".into(), l.clone());
    }
    o.insert("fields".into(), mv.get("fields").cloned().unwrap_or(Value::Object(Map::new())));
    o.insert("img".into(), Value::String(if m.kind == "image" { m.img.clone() } else { String::new() }));
    if let Some(h) = m.holes.as_ref().filter(|h| !h.is_empty()) {
        o.insert("holes".into(), Value::Array(h.iter().map(|r| pts_v(r)).collect()));
    }
    if let Some(x) = &m.xd {
        // JSON.parse(JSON.stringify(m.xd))
        o.insert("xd".into(), serde_json::from_str(&crate::json::stringify(x)).unwrap_or(Value::Null));
    }
    o
}

/// `makeClip(markups, {source, sheet})` (read-only and foreign markups left out).
pub fn make_clip(env: &Env, markups: &[&Markup], source: &str, sheet: &str) -> Option<Clip> {
    let items: Vec<ClipItem> =
        markups.iter().filter(|m| !m.locked && m.kind != "foreign" && is_type(&m.kind)).take(CLIP_MAX).map(|m| item_of(m)).collect();
    if items.is_empty() {
        return None;
    }
    Some(Clip {
        source: cstr(Some(&Value::from(source)), 80),
        sheet: cstr(Some(&Value::from(sheet)), 80),
        at: to_iso(env.now_ms).unwrap_or_default(),
        items,
        skipped: None,
    })
}

/// `cleanItem(d)`: one untrusted item → a clean item, or None.
pub fn clean_item(env: &mut Env, d: &Value) -> Option<ClipItem> {
    let o = d.as_object()?;
    let ty = o.get("type").and_then(Value::as_str)?;
    if !is_type(ty) || ty == "foreign" {
        return None;
    }
    let pts = o.get("pts").filter(|v| !v.is_null());
    if pts.is_some_and(|p| !ok_pts(p)) {
        return None;
    }
    let rect = o.get("rect").filter(|v| !v.is_null());
    if rect.is_some_and(|r| !ok_rect(r)) {
        return None;
    }
    let empty = Map::new();
    let s = o.get("style").and_then(Value::as_object).unwrap_or(&empty);
    let mut style = Map::new();
    if let Some(Value::String(c)) = s.get("color")
        && crate::js::is_hex_color(c)
    {
        style.insert("color".into(), Value::String(crate::js::lower(c)));
    }
    match s.get("fill") {
        Some(Value::Null) => {
            style.insert("fill".into(), Value::Null);
        }
        Some(Value::String(f)) if crate::js::is_hex_color(f) => {
            style.insert("fill".into(), Value::String(crate::js::lower(f)));
        }
        _ => {}
    }
    for (k, lo, hi) in [("width", 0.0, 200.0), ("fillOpacity", 0.0, 1.0), ("opacity", 0.05, 1.0), ("fontSize", 2.0, 300.0)] {
        if let Some(v) = clamp_n(s.get(k), lo, hi) {
            style.insert(k.into(), num_v(v));
        }
    }
    let rot = o.get("rot").and_then(Value::as_f64).filter(|r| [0.0, 90.0, 180.0, 270.0].contains(r)).unwrap_or(0.0);
    let mut f = Map::new();
    f.insert("sheet".into(), Value::from(""));
    f.insert("type".into(), Value::from(ty));
    f.insert("pts".into(), pts.cloned().unwrap_or(Value::Null));
    f.insert("rect".into(), rect.cloned().unwrap_or(Value::Null));
    f.insert("rot".into(), num_v(rot));
    f.insert("style".into(), Value::Object(style));
    f.insert("text".into(), Value::String(cstr(o.get("text"), 20000)));
    f.insert("label".into(), Value::String(cstr(o.get("label"), 200)));
    f.insert("subject".into(), Value::String(cstr(o.get("subject"), 200)));
    f.insert("comment".into(), Value::String(cstr(o.get("comment"), 20000)));
    f.insert("layer".into(), Value::String(cstr(o.get("layer"), 60)));
    f.insert("link".into(), Value::String(cstr(o.get("link"), 60)));
    if let Some(l) = o.get("legend") {
        f.insert("legend".into(), l.clone());
    }
    if let Some(x) = o.get("fields") {
        f.insert("fields".into(), x.clone());
    }
    f.insert("img".into(), Value::String(if ty == "image" { clean_image_value(o.get("img")) } else { String::new() }));
    if ty == "area" {
        f.insert("holes".into(), pts_holes(&clean_holes(o.get("holes"))));
    }
    if let Some(x) = o.get("xd") {
        f.insert("xd".into(), x.clone());
    }
    let m = make_markup(env, &f);
    if geometry_ok(&m) { Some(item_of(&m)) } else { None }
}

fn pts_holes(h: &[Vec<P>]) -> Value {
    Value::Array(h.iter().map(|r| pts_v(r)).collect())
}

/// `cleanClip(raw)`: untrusted text → a clean clip, or None.
pub fn clean_clip_text(env: &mut Env, raw: &str) -> Option<Clip> {
    if len16(raw) > 64_000_000 {
        return None;
    }
    let d: Value = serde_json::from_str(raw).ok()?;
    clean_clip(env, &d)
}

/// `cleanClip(obj)` for a parsed object.
pub fn clean_clip(env: &mut Env, d: &Value) -> Option<Clip> {
    let o = d.as_object()?;
    if o.get("format").and_then(Value::as_str) != Some(CLIP_FORMAT) || o.get("v").and_then(Value::as_f64) != Some(1.0) {
        return None;
    }
    let Some(Value::Array(items)) = o.get("items") else { return None };
    let kept: Vec<ClipItem> = items.iter().take(CLIP_MAX).filter_map(|it| clean_item(env, it)).collect();
    if kept.is_empty() {
        return None;
    }
    let skipped = items.len().min(CLIP_MAX).saturating_sub(kept.len());
    Some(Clip {
        source: cstr(o.get("source"), 80),
        sheet: cstr(o.get("sheet"), 80),
        at: cstr(o.get("at"), 40),
        items: kept,
        skipped: Some(skipped),
    })
}

fn item_pts(it: &ClipItem) -> Option<Vec<P>> {
    it.get("pts").filter(|v| !v.is_null()).map(crate::model::pts_of)
}
fn item_rect(it: &ClipItem) -> Option<[f64; 4]> {
    it.get("rect").filter(|v| !v.is_null()).map(crate::model::rect_of)
}

/// `clipBounds(items)`.
pub fn clip_bounds(items: &[ClipItem]) -> Option<[f64; 4]> {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut add = |x: f64, y: f64| {
        x0 = crate::js::min(x0, x);
        y0 = crate::js::min(y0, y);
        x1 = crate::js::max(x1, x);
        y1 = crate::js::max(y1, y);
    };
    for m in items {
        if let Some(r) = item_rect(m) {
            add(r[0], r[1]);
            add(r[2], r[3]);
        }
        if let Some(p) = item_pts(m) {
            for q in p {
                add(q[0], q[1]);
            }
        }
    }
    if x0 <= x1 { Some([x0, y0, x1, y1]) } else { None }
}

/// Where a paste goes.
#[derive(Debug, Clone, Default)]
pub struct PlaceOpts {
    pub sheet: String,
    pub author: String,
    pub rot: Option<f64>,
    pub delta: Option<[f64; 2]>,
    pub center: Option<[f64; 2]>,
}

/// `placeClip(clip, opts)` → the new markups.
pub fn place_clip(env: &mut Env, clip: &Clip, opts: &PlaceOpts) -> Vec<Markup> {
    if clip.items.is_empty() {
        return Vec::new();
    }
    let [mut dx, mut dy] = opts.delta.unwrap_or([0.0, 0.0]);
    if let Some(c) = opts.center
        && let Some(b) = clip_bounds(&clip.items)
    {
        dx = c[0] - (b[0] + b[2]) / 2.0;
        dy = c[1] - (b[1] + b[3]) / 2.0;
    }
    let mut out = Vec::with_capacity(clip.items.len());
    for it in &clip.items {
        let mut f = it.clone();
        f.insert("sheet".into(), Value::String(opts.sheet.clone()));
        f.insert("author".into(), Value::String(opts.author.clone()));
        if let Some(r) = opts.rot {
            f.insert("rot".into(), num_v(r));
        }
        let pts = item_pts(it).map(|p| p.iter().map(|q| [q[0] + dx, q[1] + dy]).collect::<Vec<P>>());
        f.insert("pts".into(), pts.as_ref().map_or(Value::Null, |p| pts_v(p)));
        let rect = item_rect(it).map(|r| [r[0] + dx, r[1] + dy, r[2] + dx, r[3] + dy]);
        f.insert("rect".into(), rect.as_ref().map_or(Value::Null, |r| crate::geom::nums_value(r)));
        match it.get("holes").and_then(Value::as_array) {
            Some(h) => {
                let moved: Vec<Vec<P>> =
                    h.iter().map(|r| crate::model::pts_of(r).iter().map(|q| [q[0] + dx, q[1] + dy]).collect()).collect();
                f.insert("holes".into(), pts_holes(&moved));
            }
            None => {
                f.remove("holes");
            }
        }
        out.push(make_markup(env, &f));
    }
    out
}

/// A clip as JSON (what goes to localStorage).
pub fn clip_value(c: &Clip) -> Value {
    let mut o = Map::new();
    o.insert("format".into(), Value::from(CLIP_FORMAT));
    o.insert("v".into(), num_v(1.0));
    o.insert("source".into(), Value::String(c.source.clone()));
    o.insert("sheet".into(), Value::String(c.sheet.clone()));
    o.insert("at".into(), Value::String(c.at.clone()));
    o.insert("items".into(), Value::Array(c.items.iter().cloned().map(Value::Object).collect()));
    if let Some(s) = c.skipped {
        #[allow(clippy::cast_precision_loss)]
        o.insert("skipped".into(), num_v(s as f64));
    }
    Value::Object(o)
}
