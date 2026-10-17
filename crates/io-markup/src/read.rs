//! pdf-read.js — open a PDF of the plan set and read its annotations back as markups: pages (view, rotation, page
//! label, title block from the page's text → sheet number and name), annotations through the pdf.js view
//! ([`crate::annots`]), the keys pdf.js does not hand out through the pdf-lib view ([`crate::libdoc`]: /NM /Subj /IC /CA
//! /BE /IT /CL /RD /L /LE /Vertices /InkList /DA and our private `/MarkupCraft` JSON), threads (replies, replies to
//! replies, review states), calibrations (ours or another program's /VP /Measure), sheet names typed by hand, layers,
//! columns, blank-page info and bookmarks. Everything read is untrusted and checked.

use crate::annots::{AnnotData, page_annotations};
use crate::jsdoc::{CustomValue, JsDoc, OpenError, string_to_pdf_string};
use crate::libdoc::LibDoc;
use crate::obj::{Dict, Obj};
use crate::{
    BLANK_KEY, PRIVATE_CAL_KEY, PRIVATE_COLUMNS_KEY, PRIVATE_KEY, PRIVATE_LAYERS_KEY, PRIVATE_PHOTO_KEY, PRIVATE_SHEET_KEY, refkey,
};
use cc_markup::i18n::{Fill, t, t0};
use cc_markup::js::{date_utc, len16, slice16, str_or_empty, to_iso_s, to_number, trim, truthy};
use cc_markup::model::{Env, Markup, STATUSES, clean_reply, geometry_ok, make_markup, status_text, types_truthy};
use cc_markup::titleblock::{TextPiece, TitleBlock, Viewport, read_title_block};
use cc_markup::units::Cal;
use indexmap::IndexSet;
use serde_json::{Map, Value, json};

pub const EDITABLE_SUBTYPES: [&str; 9] = ["Square", "Circle", "Line", "PolyLine", "Polygon", "Ink", "FreeText", "Text", "Stamp"];

/// `sheetInfo(label, fileName, index, tb, manual)`.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetInfo {
    pub label: String,
    pub sheet_no: String,
    pub name: String,
    pub title: String,
    pub from: &'static str,
}

fn is_page_number(s: &str) -> bool {
    (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())) || (!s.is_empty() && s.chars().all(|c| "ivxlcdmIVXLCDM".contains(c)))
}

/// `sheetInfo`.
pub fn sheet_info(
    label: Option<&str>,
    file_name: &str,
    index: usize,
    tb: Option<&TitleBlock>,
    manual: Option<&(String, String)>,
) -> SheetInfo {
    let lab = label.map(trim).unwrap_or("").to_string();
    let fname = if file_name.is_empty() { t0("file.untitled") } else { file_name.to_string() };
    let base = {
        let l = fname.to_lowercase();
        if l.ends_with(".pdf") && fname.is_char_boundary(fname.len().saturating_sub(4)) {
            fname.get(..fname.len().saturating_sub(4)).unwrap_or("").to_string()
        } else {
            fname.clone()
        }
    };
    let is_number = is_page_number(&lab);
    let lab_no = if !lab.is_empty()
        && !is_number
        && lab.chars().any(|c| c.is_ascii_alphabetic())
        && lab.chars().any(|c| c.is_ascii_digit())
        && len16(&lab) <= 24
    {
        lab.clone()
    } else {
        String::new()
    };
    let lab_name = if !lab.is_empty() && !is_number && lab_no.is_empty() { lab.clone() } else { String::new() };
    let m_no = manual.map(|m| trim(&m.0).to_string()).unwrap_or_default();
    let m_title = manual.map(|m| trim(&m.1).to_string()).unwrap_or_default();
    let tb_no = tb.map(|t| t.number.clone()).unwrap_or_default();
    let tb_title = tb.map(|t| t.title.clone()).unwrap_or_default();
    let sheet_no = [m_no, lab_no.clone(), tb_no.clone()].into_iter().find(|s| !s.is_empty()).unwrap_or_default();
    let name = [m_title, lab_name.clone(), tb_title.clone()].into_iter().find(|s| !s.is_empty()).unwrap_or_default();
    let from = if manual.is_some_and(|m| !m.0.is_empty() || !m.1.is_empty()) {
        "manual"
    } else if !lab_no.is_empty() || !lab_name.is_empty() {
        "label"
    } else if tb.is_some_and(|t| !t.number.is_empty() || !t.title.is_empty()) {
        "title block"
    } else {
        "file"
    };
    let title = if !sheet_no.is_empty() && !name.is_empty() {
        t("sheet.title", &[("number", Fill::from(&sheet_no)), ("name", Fill::from(&name))])
    } else if !sheet_no.is_empty() || !name.is_empty() {
        if sheet_no.is_empty() { name.clone() } else { sheet_no.clone() }
    } else {
        #[allow(clippy::cast_precision_loss)]
        let n = index as f64 + 1.0;
        t("sheet.title.page", &[("file", Fill::S(base)), ("n", Fill::N(n))])
    };
    SheetInfo { label: lab, sheet_no, name, title, from }
}

/// `hexColor(c, fallback)` for a pdf.js colour.
pub fn hex_color(c: Option<[u8; 3]>, fallback: &str) -> String {
    match c {
        None => fallback.to_string(),
        Some(c) => format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]),
    }
}

/// `hexColor` for numbers already multiplied (`hex01`): JS `Math.round`, clamp, hex.
fn hex_numbers(v: &[f64], fallback: &str) -> String {
    if v.len() < 3 {
        return fallback.to_string();
    }
    let mut s = String::from("#");
    for x in v.iter().take(3) {
        let r = cc_markup::js::max(0.0, cc_markup::js::min(255.0, cc_markup::js::round(*x)));
        if r.is_nan() {
            s.push_str("NaN");
        } else {
            s.push_str(&format!("{:02x}", cc_markup::js::f2u(r)));
        }
    }
    s
}

/// `hex01(arr)` for a plain value.
fn hex01(v: Option<&Value>) -> Option<String> {
    let a = v?.as_array()?;
    if a.len() < 3 {
        return None;
    }
    let nums: Vec<f64> = a.iter().map(|x| to_number(x) * 255.0).collect();
    Some(hex_numbers(&nums, "#d0021b"))
}

/// `pdfDate("D:20261010123000-05'00'")` → ISO, or ''.
pub fn pdf_date(s: &str) -> String {
    let b = s.as_bytes();
    let mut i = if s.starts_with("D:") { 2 } else { 0 };
    let digits = |i: &mut usize, n: usize| -> Option<f64> {
        let part = b.get(*i..i.saturating_add(n))?;
        if part.len() == n && part.iter().all(u8::is_ascii_digit) {
            *i = i.saturating_add(n);
            Some(part.iter().fold(0.0, |a, c| a * 10.0 + f64::from(c.saturating_sub(b'0'))))
        } else {
            None
        }
    };
    let Some(y) = digits(&mut i, 4) else { return String::new() };
    let mut parts = [1.0, 1.0, 0.0, 0.0, 0.0];
    for p in parts.iter_mut() {
        match digits(&mut i, 2) {
            Some(v) => *p = v,
            None => break,
        }
    }
    let z = b.get(i).copied().filter(|c| matches!(c, b'Z' | b'z' | b'+' | b'-'));
    if z.is_some() {
        i = i.saturating_add(1);
    }
    let oh = digits(&mut i, 2).unwrap_or(0.0);
    if b.get(i) == Some(&b'\'') {
        i = i.saturating_add(1);
    }
    let om = digits(&mut i, 2).unwrap_or(0.0);
    let mut t = date_utc(y, parts[0] - 1.0, parts[1], parts[2], parts[3], parts[4], 0.0);
    if let Some(c @ (b'+' | b'-')) = z {
        t -= (if c == b'+' { 1.0 } else { -1.0 }) * (oh * 60.0 + om) * 60000.0;
    }
    if t.is_nan() || t.abs() > 8.64e15 {
        return String::new();
    }
    to_iso_s(t).unwrap_or_default()
}

/// `plain(lib, obj, depth)`: a pdf-lib object as plain data.
pub fn plain(o: Option<&Obj>, depth: usize) -> Value {
    let Some(o) = o else { return Value::Null };
    if depth > 4 {
        return Value::Null;
    }
    match o {
        Obj::Num(n) => serde_json::Number::from_f64(*n).map_or(Value::Null, Value::Number),
        Obj::Name(n) => Value::String(n.clone()),
        Obj::Str(_) | Obj::Hex(_) | Obj::Bytes(_) => Value::String(o.decode_text().unwrap_or_default()),
        Obj::Bool(b) => Value::Bool(*b),
        Obj::Arr(a) => Value::Array(a.iter().map(|x| plain(Some(x), depth.saturating_add(1))).collect()),
        Obj::Ref(n, g) => json!({ "ref": refkey(*n, *g) }),
        Obj::Dict(d) => {
            let mut m = Map::new();
            for (k, v) in &d.0 {
                m.insert(k.clone(), plain(Some(v), depth.saturating_add(1)));
            }
            Value::Object(m)
        }
        Obj::Null | Obj::Stream(_) => Value::Null,
    }
}

/// `parseRef("12R3")` → (12, 3).
pub fn parse_refkey(id: &str) -> Option<(u32, u16)> {
    let (n, g) = id.split_once('R')?;
    if n.is_empty() || !n.bytes().all(|c| c.is_ascii_digit()) || !g.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((n.parse().ok()?, if g.is_empty() { 0 } else { g.parse().ok()? }))
}

/// `extrasOf(lib, ctx, id)`.
pub fn extras_of(lib: &LibDoc, id: &str) -> Map<String, Value> {
    let mut out = Map::new();
    let Some(r) = parse_refkey(id) else { return out };
    let Some(d) = lib.get(r).and_then(|o| if let Obj::Dict(d) = o { Some(d) } else { None }) else { return out };
    for k in
        [PRIVATE_PHOTO_KEY, "NM", "Subj", "IC", "CA", "BE", "IT", "CL", "RD", "L", "LE", "Vertices", "InkList", "Name", "DA", PRIVATE_KEY]
    {
        if let Some(v) = d.get(k) {
            out.insert(k.to_string(), plain(Some(lib.lookup(v)), 0));
        }
    }
    out
}

/// `daSize(da)`.
fn da_size(da: &str) -> f64 {
    let b = da.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if b.get(i).is_some_and(|c| c.is_ascii_digit() || *c == b'.') {
            let s = i;
            while b.get(i).is_some_and(|c| c.is_ascii_digit() || *c == b'.') {
                i = i.saturating_add(1);
            }
            let run = da.get(s..i).unwrap_or("");
            let mut j = i;
            let ws0 = j;
            while da.get(j..).and_then(|r| r.chars().next()).is_some_and(cc_markup::js::is_ws) {
                j = j.saturating_add(da.get(j..).and_then(|r| r.chars().next()).map_or(1, char::len_utf8));
            }
            if j > ws0 && b.get(j..j.saturating_add(2)) == Some(b"Tf") {
                let v = cc_markup::js::str_to_number(run);
                return cc_markup::js::max(4.0, cc_markup::js::min(144.0, v));
            }
        } else {
            i = i.saturating_add(1);
        }
    }
    12.0
}

fn fin(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|x| x.is_finite())
}

/// `fromPrivate(json, a, sheet)`: one of our own markups from its private JSON (None when not usable).
pub fn from_private(env: &mut Env, json_text: &str, a_rect: Option<[f64; 4]>, sheet: &str) -> Option<Markup> {
    let d: Value = serde_json::from_str(json_text).ok()?;
    let o = d.as_object()?;
    if o.get("v").and_then(Value::as_f64) != Some(1.0) {
        return None;
    }
    let ty = o.get("type").and_then(Value::as_str)?;
    if !types_truthy(ty) || ty == "foreign" {
        return None;
    }
    let ok_pts = |p: &Value| {
        p.as_array()
            .is_some_and(|a| a.iter().all(|q| q.as_array().is_some_and(|q| q.len() == 2 && q.iter().all(|x| fin(Some(x)).is_some()))))
    };
    let ok_rect = |r: &Value| r.as_array().is_some_and(|a| a.len() == 4 && a.iter().all(|x| fin(Some(x)).is_some()));
    let present = |k: &str| o.get(k).filter(|v| !v.is_null());
    if present("pts").is_some_and(|p| !ok_pts(p)) || present("rect").is_some_and(|r| !ok_rect(r)) {
        return None;
    }
    if let (Some(ar), Some(rect)) = (o.get("ar").filter(|r| ok_rect(r)), a_rect) {
        let moved = ar.as_array().is_some_and(|a| {
            a.iter().enumerate().any(|(i, v)| (v.as_f64().unwrap_or(f64::NAN) - rect.get(i).copied().unwrap_or(f64::NAN)).abs() > 0.75)
        });
        if moved {
            return None;
        }
    }
    let empty = Map::new();
    let style = o.get("style").and_then(Value::as_object).unwrap_or(&empty);
    let mut clean = Map::new();
    if let Some(Value::String(c)) = style.get("color")
        && cc_markup::js::is_hex_color(c)
    {
        clean.insert("color".into(), Value::String(c.clone()));
    }
    match style.get("fill") {
        Some(Value::Null) => {
            clean.insert("fill".into(), Value::Null);
        }
        Some(Value::String(f)) if cc_markup::js::is_hex_color(f) => {
            clean.insert("fill".into(), Value::String(f.clone()));
        }
        _ => {}
    }
    for (k, lo, hi) in [("width", 0.0, 200.0), ("fillOpacity", 0.0, 1.0), ("opacity", 0.05, 1.0), ("fontSize", 2.0, 300.0)] {
        if let Some(v) = fin(style.get(k)) {
            clean.insert(k.into(), num(cc_markup::js::max(lo, cc_markup::js::min(hi, v))));
        }
    }
    let s = |k: &str| Value::String(str_or_empty(o.get(k)));
    let or_empty = |k: &str| {
        Value::String(if truthy(o.get(k)) { o.get(k).map(cc_markup::js::to_js_string).unwrap_or_default() } else { String::new() })
    };
    let mut f = Map::new();
    if let Some(Value::String(id)) = o.get("id")
        && cc_markup::js::is_id(id, 80)
    {
        f.insert("id".into(), Value::String(id.clone()));
    }
    f.insert("sheet".into(), Value::from(sheet));
    f.insert("type".into(), Value::from(ty));
    f.insert("pts".into(), if truthy(o.get("pts")) { o.get("pts").cloned().unwrap_or(Value::Null) } else { Value::Null });
    f.insert("rect".into(), if truthy(o.get("rect")) { o.get("rect").cloned().unwrap_or(Value::Null) } else { Value::Null });
    let rot = o.get("rot").and_then(Value::as_f64).filter(|r| [0.0, 90.0, 180.0, 270.0].contains(r)).unwrap_or(0.0);
    f.insert("rot".into(), num(rot));
    f.insert("style".into(), Value::Object(clean));
    for k in ["text", "label", "subject", "comment", "author"] {
        f.insert(k.into(), s(k));
    }
    f.insert("created".into(), or_empty("created"));
    f.insert("modified".into(), or_empty("modified"));
    if let Some(st) = o.get("status") {
        f.insert("status".into(), st.clone());
    }
    f.insert("punch".into(), o.get("punch").filter(|p| p.is_object()).cloned().unwrap_or(Value::Null));
    f.insert("layer".into(), o.get("layer").filter(|v| v.is_string()).cloned().unwrap_or(Value::from("")));
    f.insert(
        "img".into(),
        Value::String(if ty == "image" { cc_markup::imagestamp::clean_image_value(o.get("img")) } else { String::new() }),
    );
    f.insert("link".into(), o.get("link").filter(|v| v.is_string()).cloned().unwrap_or(Value::from("")));
    if let Some(l) = o.get("legend") {
        f.insert("legend".into(), l.clone());
    }
    if let Some(x) = o.get("fields") {
        f.insert("fields".into(), x.clone());
    }
    if ty == "area" {
        let h = cc_markup::holes::clean_holes(o.get("holes"));
        f.insert("holes".into(), Value::Array(h.iter().map(|r| cc_markup::geom::pts_value(r)).collect()));
    }
    if let Some(x) = o.get("xd") {
        f.insert("xd".into(), x.clone());
    }
    let mk = make_markup(env, &f);
    if geometry_ok(&mk) { Some(mk) } else { None }
}

/// A number as a JSON value; NaN / ±Infinity as "$NaN" / "$Inf" / "$-Inf", which the model reads back as those
/// numbers (JSON null would read as 0, the way the JS treats a null) and the canonical writer prints as the JS does.
fn num(x: f64) -> Value {
    cc_markup::slip::num_value(x)
}

/// A reply read from the file.
#[derive(Debug, Clone)]
pub struct RawReply {
    pub r#ref: String,
    pub to: String,
    pub nm: String,
    pub model: String,
    pub state: String,
    pub author: String,
    pub text: String,
    pub date: String,
    pub photo: String,
}

/// What one annotation becomes.
#[derive(Debug)]
pub enum Annot {
    Markup(Box<Markup>, bool),
    Markups(Vec<Markup>),
    Foreign(Box<Markup>),
    Reply(RawReply),
}

/// `pairs(flat)`.
fn pairs(v: Option<&Value>) -> Vec<Value> {
    let empty = Vec::new();
    let a = v.and_then(Value::as_array).unwrap_or(&empty);
    let mut out = Vec::new();
    let mut i = 0usize;
    while i.saturating_add(1) < a.len() {
        out.push(json!([num(a.get(i).map_or(f64::NAN, to_number)), num(a.get(i.saturating_add(1)).map_or(f64::NAN, to_number))]));
        i = i.saturating_add(2);
    }
    out
}

fn first_line(s: &str) -> &str {
    s.split('\n').next().unwrap_or("")
}
fn rest_lines(s: &str) -> String {
    s.split('\n').skip(1).collect::<Vec<_>>().join("\n")
}

/// `/^(.*):\s*\d+\s*$/.exec(line)` → the group name.
fn count_label(line: &str) -> Option<String> {
    let term = line.find(['\r', '\u{2028}', '\u{2029}']).unwrap_or(line.len());
    let prefix_zone = line.get(..term).unwrap_or("");
    let mut colons: Vec<usize> = prefix_zone.match_indices(':').map(|(i, _)| i).collect();
    colons.reverse();
    for c in colons {
        let rest = line.get(c.saturating_add(1)..).unwrap_or("");
        let r = rest.trim_start_matches(cc_markup::js::is_ws);
        let d = r.trim_start_matches(|ch: char| ch.is_ascii_digit());
        if d.len() < r.len() && d.chars().all(cc_markup::js::is_ws) {
            return Some(line.get(..c).unwrap_or("").to_string());
        }
    }
    None
}

/// `annotToMarkup(a, x, sheet, rot)`.
pub fn annot_to_markup(env: &mut Env, a: &AnnotData, x: &Map<String, Value>, sheet: &str, rot: f64) -> Option<Annot> {
    let sub = a.subtype.clone()?;
    if sub.is_empty() || ["Popup", "Link", "Widget"].contains(&sub.as_str()) {
        return None;
    }
    if sub == "FileAttachment" && x.contains_key(PRIVATE_PHOTO_KEY) {
        return None;
    }
    let title = a.title.clone().unwrap_or_default();
    if let (Some(Some(to)), "Text") = (&a.in_reply_to, sub.as_str())
        && !to.is_empty()
    {
        if a.reply_type.as_deref().is_some_and(|r| !r.is_empty() && r != "R") {
            return None;
        }
        let md = pdf_date(a.modification_date.as_deref().unwrap_or(""));
        return Some(Annot::Reply(RawReply {
            r#ref: a.id.clone(),
            to: to.clone(),
            nm: x.get("NM").and_then(Value::as_str).unwrap_or("").to_string(),
            model: a.state_model.clone().unwrap_or_default(),
            state: a.state.clone().unwrap_or_default(),
            author: title,
            text: a.contents.clone(),
            date: if md.is_empty() { pdf_date(a.creation_date.as_deref().unwrap_or("")) } else { md },
            photo: x.get(PRIVATE_PHOTO_KEY).and_then(Value::as_str).unwrap_or("").to_string(),
        }));
    }
    let md = pdf_date(a.modification_date.as_deref().unwrap_or(""));
    let cd = pdf_date(a.creation_date.as_deref().unwrap_or(""));
    let subject = x.get("Subj").and_then(Value::as_str).unwrap_or("").to_string();
    let mut base = Map::new();
    base.insert("sheet".into(), Value::from(sheet));
    base.insert("rot".into(), num(rot));
    base.insert("author".into(), Value::from(title.clone()));
    base.insert("comment".into(), Value::from(a.contents.clone()));
    base.insert("subject".into(), Value::from(subject.clone()));
    base.insert("created".into(), Value::from(if cd.is_empty() { md.clone() } else { cd }));
    base.insert("modified".into(), Value::from(md));
    let src = json!({ "ref": a.id, "subtype": sub });
    base.insert("src".into(), src.clone());
    if EDITABLE_SUBTYPES.contains(&sub.as_str())
        && let Some(Value::String(pj)) = x.get(PRIVATE_KEY)
        && let Some(mut m) = from_private(env, pj, Some(a.rect), sheet)
    {
        m.src = Some(cc_markup::model::Src { r#ref: a.id.clone(), subtype: sub.clone() });
        return Some(Annot::Markup(Box::new(m), true));
    }
    let hidden = cc_markup::js::f2u(a.flags) & 2 != 0;
    let color = hex_color(a.color, "#d0021b");
    let width = if a.border_width.is_finite() { a.border_width } else { 1.0 };
    let opacity = fin(x.get("CA")).map_or(1.0, |c| cc_markup::js::max(0.05, cc_markup::js::min(1.0, c)));
    let fill = hex01(x.get("IC"));
    let style = json!({ "color": color, "width": num(width), "opacity": num(opacity), "fill": fill, "fillOpacity": num(if fill.is_some() { 1.0 } else { 0.25 }) });
    let cloudy = x.get("BE").and_then(|b| b.get("S")).and_then(Value::as_str) == Some("C") && truthy(x.get("BE"));
    let it = x.get("IT").and_then(Value::as_str).unwrap_or("");
    let with = |extra: Vec<(&str, Value)>| -> Map<String, Value> {
        let mut f = base.clone();
        for (k, v) in extra {
            f.insert(k.into(), v);
        }
        f
    };
    let rect_v = cc_markup::geom::nums_value(&a.rect);
    let foreign = |env: &mut Env| {
        let f = with(vec![
            ("type", Value::from("foreign")),
            ("rect", rect_v.clone()),
            ("locked", Value::Bool(true)),
            ("subject", Value::from(if subject.is_empty() { sub.clone() } else { subject.clone() })),
            ("style", json!({ "color": color })),
        ]);
        Some(Annot::Foreign(Box::new(make_markup(env, &f))))
    };
    if hidden {
        return None;
    }
    let style_nofill = {
        let mut s = style.clone();
        if let Some(o) = s.as_object_mut() {
            o.insert("fill".into(), Value::Null);
        }
        s
    };
    let rd_of = || -> [f64; 4] {
        match x.get("RD").and_then(Value::as_array) {
            Some(a) if a.len() == 4 => {
                let g = |i: usize| a.get(i).map_or(f64::NAN, to_number);
                [g(0), g(1), g(2), g(3)]
            }
            _ => [0.0; 4],
        }
    };
    match sub.as_str() {
        "Square" | "Circle" => {
            let rd = rd_of();
            let r = a.rect;
            let mut rect = [r[0] + rd[0] + width / 2.0, r[1] + rd[3] + width / 2.0, r[2] - rd[2] - width / 2.0, r[3] - rd[1] - width / 2.0];
            if rect[2] <= rect[0] || rect[3] <= rect[1] {
                return foreign(env);
            }
            let ty = if sub == "Circle" {
                "ellipse"
            } else if cloudy {
                "cloudrect"
            } else {
                "rect"
            };
            if cloudy {
                let inset = cc_markup::js::max(4.0, width * 6.0 + 8.0) * 0.5;
                rect[0] += inset;
                rect[1] += inset;
                rect[2] -= inset;
                rect[3] -= inset;
            }
            let f = with(vec![("type", Value::from(ty)), ("rect", cc_markup::geom::nums_value(&rect)), ("style", style)]);
            Some(Annot::Markup(Box::new(make_markup(env, &f)), false))
        }
        "Line" => {
            let l: Option<Vec<Value>> = match x.get("L").and_then(Value::as_array) {
                Some(l) if l.len() == 4 => Some(l.clone()),
                _ => a.line_coordinates.map(|c| c.iter().map(|v| num(*v)).collect()),
            };
            let Some(l) = l.filter(|l| l.len() == 4) else { return foreign(env) };
            let le: Vec<String> =
                x.get("LE").and_then(Value::as_array).map(|a| a.iter().map(cc_markup::js::to_js_string).collect()).unwrap_or_default();
            let is_dim = it == "LineDimension";
            let arrow_end = le.get(1).is_some_and(|s| s.contains("Arrow"));
            let arrow_start = le.first().is_some_and(|s| s.contains("Arrow"));
            let g = |i: usize| l.get(i).cloned().unwrap_or(Value::Null);
            let mut pts = vec![json!([g(0), g(1)]), json!([g(2), g(3)])];
            if arrow_start && !arrow_end {
                pts.reverse();
            }
            let ty = if is_dim {
                "measure"
            } else if arrow_end || arrow_start {
                "arrow"
            } else {
                "line"
            };
            let f = with(vec![
                ("type", Value::from(ty)),
                ("pts", Value::Array(pts)),
                ("style", style_nofill),
                ("text", Value::from(if is_dim { first_line(&a.contents).to_string() } else { String::new() })),
                ("comment", Value::from(if is_dim { rest_lines(&a.contents) } else { a.contents.clone() })),
            ]);
            Some(Annot::Markup(Box::new(make_markup(env, &f)), false))
        }
        "PolyLine" | "Polygon" => {
            let pts = match x.get("Vertices") {
                Some(v @ Value::Array(_)) => pairs(Some(v)),
                _ => match &a.vertices {
                    Some(v) => pairs(Some(&cc_markup::geom::nums_value(v))),
                    None => Vec::new(),
                },
            };
            if pts.len() < 2 {
                return foreign(env);
            }
            let ty = if sub == "PolyLine" {
                "polyline"
            } else if it == "PolygonDimension" && pts.len() >= 3 {
                "area"
            } else if cloudy {
                "cloud"
            } else {
                "polygon"
            };
            let f = with(vec![
                ("type", Value::from(ty)),
                ("pts", Value::Array(pts)),
                ("style", if sub == "PolyLine" { style_nofill } else { style }),
            ]);
            Some(Annot::Markup(Box::new(make_markup(env, &f)), false))
        }
        "Ink" => {
            let lists: Vec<Value> = match x.get("InkList") {
                Some(Value::Array(l)) => l.clone(),
                _ => a.ink_lists.as_ref().map(|ls| ls.iter().map(|l| cc_markup::geom::nums_value(l)).collect()).unwrap_or_default(),
            };
            let strokes: Vec<Vec<Value>> = lists.iter().map(|l| pairs(Some(l))).filter(|p| !p.is_empty()).collect();
            if strokes.is_empty() {
                return foreign(env);
            }
            if it == "CountMarks" {
                let marks: Vec<Value> = strokes
                    .iter()
                    .map(|ring0| {
                        let pt = |v: &Value, i: usize| v.get(i).and_then(Value::as_f64).unwrap_or(f64::NAN);
                        let n0 = ring0.len();
                        let closes = n0 > 2
                            && ring0
                                .first()
                                .zip(ring0.last())
                                .is_some_and(|(a, b)| cc_markup::js::hypot(pt(a, 0) - pt(b, 0), pt(a, 1) - pt(b, 1)) < 1e-6);
                        let ring: &[Value] = if closes { ring0.get(..n0.saturating_sub(1)).unwrap_or(&[]) } else { ring0 };
                        #[allow(clippy::cast_precision_loss)]
                        let k = ring.len() as f64;
                        let sx = ring.iter().fold(0.0, |t, p| t + pt(p, 0));
                        let sy = ring.iter().fold(0.0, |t, p| t + pt(p, 1));
                        json!([num(sx / k), num(sy / k)])
                    })
                    .collect();
                let lab = count_label(first_line(&a.contents));
                let f = with(vec![
                    ("type", Value::from("count")),
                    ("pts", Value::Array(marks)),
                    ("label", Value::from(lab.unwrap_or_else(|| t0("count.default")))),
                    ("comment", Value::from(rest_lines(&a.contents))),
                    ("style", json!({ "color": color, "width": num(width) })),
                ]);
                return Some(Annot::Markup(Box::new(make_markup(env, &f)), false));
            }
            let hl = it == "InkHighlight" || (opacity < 0.7 && width >= 6.0);
            let ms = strokes
                .into_iter()
                .enumerate()
                .map(|(i, pts)| {
                    let f = with(vec![
                        ("src", if i > 0 { Value::Null } else { src.clone() }),
                        ("type", Value::from(if hl { "highlight" } else { "pen" })),
                        ("pts", Value::Array(pts)),
                        ("style", style_nofill.clone()),
                    ]);
                    make_markup(env, &f)
                })
                .collect();
            Some(Annot::Markups(ms))
        }
        "FreeText" => {
            let r = a.rect;
            let rd = rd_of();
            let rect = [r[0] + rd[0], r[1] + rd[3], r[2] - rd[2], r[3] - rd[1]];
            if rect[2] - rect[0] < 1.0 || rect[3] - rect[1] < 1.0 {
                return foreign(env);
            }
            let mut s = style.clone();
            if let Some(o) = s.as_object_mut() {
                o.insert("fontSize".into(), num(da_size(x.get("DA").map(str_val).unwrap_or_default().as_str())));
                o.insert("width".into(), num(if it == "FreeTextCallout" { cc_markup::js::max(0.5, width) } else { width }));
            }
            let text = a.contents.clone();
            if it == "FreeTextCallout"
                && let Some(cl) = x.get("CL").and_then(Value::as_array).filter(|c| c.len() >= 4)
            {
                let cl = pairs(Some(&Value::Array(cl.clone())));
                let pts: Vec<Value> = if cl.len() >= 3 { cl.into_iter().take(2).collect() } else { cl.into_iter().take(1).collect() };
                let f = with(vec![
                    ("type", Value::from("callout")),
                    ("rect", cc_markup::geom::nums_value(&rect)),
                    ("pts", Value::Array(pts)),
                    ("text", Value::from(text)),
                    ("comment", Value::from("")),
                    ("style", s),
                ]);
                return Some(Annot::Markup(Box::new(make_markup(env, &f)), false));
            }
            let f = with(vec![
                ("type", Value::from("text")),
                ("rect", cc_markup::geom::nums_value(&rect)),
                ("text", Value::from(text)),
                ("comment", Value::from("")),
                ("style", s),
            ]);
            Some(Annot::Markup(Box::new(make_markup(env, &f)), false))
        }
        "Text" => {
            let r = a.rect;
            let c = if color == "#000000" { "#f2c230".to_string() } else { color.clone() };
            let f = with(vec![("type", Value::from("note")), ("pts", json!([[num(r[0]), num(r[3])]])), ("style", json!({ "color": c }))]);
            Some(Annot::Markup(Box::new(make_markup(env, &f)), false))
        }
        _ => foreign(env),
    }
}

fn str_val(v: &Value) -> String {
    cc_markup::js::to_js_string(v)
}

/// `calFromMeasure(meas)`.
pub fn cal_from_measure(meas: Option<&Value>) -> Option<Cal> {
    let m = meas?.as_object()?;
    if m.get("Subtype").and_then(Value::as_str) != Some("RL") {
        return None;
    }
    let x0 = m.get("X").and_then(Value::as_array).and_then(|a| a.first()).filter(|x| truthy(Some(x)))?;
    let c = x0.get("C").map_or(f64::NAN, to_number);
    let u_raw = str_or_empty(x0.get("U"));
    let u: String = cc_markup::js::lower(trim(&u_raw)).chars().filter(|c| *c != '.' && !cc_markup::js::is_ws(*c)).collect();
    let mm = match u.as_str() {
        "ft" | "'" | "feet" => 304.8,
        "in" | "\"" | "inch" => 25.4,
        "m" => 1000.0,
        "cm" => 10.0,
        "mm" => 1.0,
        x if cc_markup::units::PROTO_NAMES.contains(&x) => f64::NAN,
        _ => return None,
    };
    if !(c > 0.0) {
        return None;
    }
    let unit = match u.as_str() {
        "ft" | "'" | "feet" => "ft-in".to_string(),
        "in" | "\"" | "inch" => "in".to_string(),
        "cm" => "mm".to_string(),
        x => x.to_string(),
    };
    let label = if truthy(m.get("R")) { m.get("R").map(str_val).unwrap_or_default() } else { t0("cal.from_file") };
    Some(Cal { mm_per_pt: c * mm, unit, label: slice16(&label, 120) })
}

/// One page of the record.
#[derive(Debug, Clone)]
pub struct PageRec {
    pub index: usize,
    pub key: String,
    pub info: SheetInfo,
    pub tb: Option<TitleBlock>,
    pub view: [f64; 4],
    pub rotate: f64,
    pub annot_mode: &'static str,
}

/// A bookmark.
#[derive(Debug, Clone)]
pub struct Bookmark {
    pub title: String,
    pub page: f64,
    pub children: Vec<Bookmark>,
}

/// The file record `openPdf` gives (minus the bytes and the pdf.js document).
#[derive(Debug, Clone)]
pub struct Record {
    pub id: String,
    pub name: String,
    pub size: usize,
    pub pages: Vec<PageRec>,
    pub markups: Vec<Markup>,
    pub calibrations: Vec<(String, Cal)>,
    pub sheet_names: Vec<(String, (String, String))>,
    pub remove_refs: IndexSet<String>,
    pub layers: Value,
    pub columns: Value,
    pub blank: Option<Value>,
    pub bookmarks: Vec<Bookmark>,
    pub notes: Vec<String>,
    pub foreign_count: usize,
    pub cleaned: bool,
    pub reply_count: usize,
}

/// `blankInfo(raw)`.
pub fn blank_info(raw: Option<&CustomValue>) -> Option<Value> {
    let s = match raw? {
        CustomValue::Str(s) => s.clone(),
        CustomValue::Num(n) => cc_markup::js::ns(*n),
        CustomValue::Bool(b) => b.to_string(),
        CustomValue::Name(_) => "[object Object]".into(),
    };
    if s.is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(&s).ok()?;
    let o = v.as_object()?;
    let paper = o
        .get("paper")
        .and_then(Value::as_str)
        .filter(|p| ["letter", "tabloid", "arch-d", "arch-e", "a4", "a3", "a1"].contains(p))
        .unwrap_or("letter");
    let orient = o.get("orient").and_then(Value::as_str).filter(|p| ["portrait", "landscape"].contains(p)).unwrap_or("portrait");
    let grid = o.get("grid").and_then(Value::as_str).filter(|p| ["none", "dot", "square"].contains(p)).unwrap_or("none");
    let metric = ["a4", "a3", "a1"].contains(&paper);
    Some(json!({ "paper": paper, "orient": orient, "grid": grid, "metric": metric }))
}

fn custom_get<'a>(c: &'a [(String, CustomValue)], k: &str) -> Option<&'a CustomValue> {
    c.iter().find(|(x, _)| x == k).map(|(_, v)| v)
}
fn custom_truthy(v: Option<&CustomValue>) -> bool {
    match v {
        None => false,
        Some(CustomValue::Str(s)) => !s.is_empty(),
        Some(CustomValue::Num(n)) => *n != 0.0 && !n.is_nan(),
        Some(CustomValue::Bool(b)) => *b,
        Some(CustomValue::Name(_)) => true,
    }
}
fn custom_json(v: Option<&CustomValue>) -> Option<Value> {
    let s = match v? {
        CustomValue::Str(s) if s.is_empty() => return None,
        CustomValue::Str(s) => s.clone(),
        CustomValue::Num(n) if *n == 0.0 || n.is_nan() => return None,
        CustomValue::Num(n) => cc_markup::js::ns(*n),
        CustomValue::Bool(false) => return None,
        CustomValue::Bool(true) => "true".into(),
        CustomValue::Name(_) => "[object Object]".into(),
    };
    Some(serde_json::from_str(&s).unwrap_or(Value::Null)).filter(|v| !v.is_null() || s == "null")
}

/// The outline (`readOutline`).
fn read_outline(doc: &JsDoc) -> Vec<Bookmark> {
    let cat = doc.catalog();
    let Some(Obj::Dict(outlines)) = doc.dget(&cat, "Outlines") else { return Vec::new() };
    let Some(Obj::Ref(fn_, fg)) = outlines.get("First").cloned() else { return Vec::new() };
    // pdf.js builds the tree breadth first with a seen-set; each parent keeps its children in Next order
    struct Node {
        title: String,
        dest: Option<Obj>,
        kids: Vec<usize>,
    }
    let mut nodes: Vec<Node> = Vec::new();
    let mut roots: Vec<usize> = Vec::new();
    let mut seen: Vec<(u32, u16)> = vec![(fn_, fg)];
    let mut queue: std::collections::VecDeque<((u32, u16), Option<usize>)> = std::collections::VecDeque::new();
    queue.push_back(((fn_, fg), None));
    while let Some((r, parent)) = queue.pop_front() {
        if nodes.len() > 100_000 {
            break;
        }
        let Obj::Dict(d) = doc.get(r) else { continue };
        let title = match doc.dget(&d, "Title") {
            Some(t) if t.is_string() => string_to_pdf_string(&t.str_bytes().unwrap_or_default()),
            _ => String::new(),
        };
        let dest = outline_dest(doc, &d);
        let idx = nodes.len();
        nodes.push(Node { title, dest, kids: Vec::new() });
        match parent {
            Some(p) => {
                if let Some(n) = nodes.get_mut(p) {
                    n.kids.push(idx);
                }
            }
            None => roots.push(idx),
        }
        if let Some(Obj::Ref(n, g)) = d.get("First")
            && !seen.contains(&(*n, *g))
        {
            seen.push((*n, *g));
            queue.push_back(((*n, *g), Some(idx)));
        }
        if let Some(Obj::Ref(n, g)) = d.get("Next")
            && !seen.contains(&(*n, *g))
        {
            seen.push((*n, *g));
            queue.push_back(((*n, *g), parent));
        }
    }
    let mut count = 0usize;
    fn walk(doc: &JsDoc, nodes: &[Node], list: &[usize], depth: usize, count: &mut usize) -> Vec<Bookmark> {
        let mut out = Vec::new();
        for &i in list {
            if *count >= 2000 {
                break;
            }
            *count = count.saturating_add(1);
            let Some(n) = nodes.get(i) else { continue };
            let title: String = n.title.chars().map(|c| if (c as u32) < 0x20 { ' ' } else { c }).collect();
            let page = page_of(doc, n.dest.as_ref());
            let children = if depth < 2 { walk(doc, nodes, &n.kids, depth.saturating_add(1), count) } else { Vec::new() };
            out.push(Bookmark { title: slice16(trim(&title), 200), page, children });
        }
        out
    }
    walk(doc, &nodes, &roots, 0, &mut count)
}

fn outline_dest(doc: &JsDoc, d: &Dict) -> Option<Obj> {
    if let Some(dest) = doc.dget(d, "Dest") {
        return Some(dest);
    }
    if let Some(Obj::Dict(a)) = doc.dget(d, "A")
        && doc.dget(&a, "S").as_ref().and_then(Obj::as_name) == Some("GoTo")
    {
        return doc.dget(&a, "D");
    }
    None
}

fn named_dest(doc: &JsDoc, name: &[u8]) -> Option<Obj> {
    let cat = doc.catalog();
    if let Some(Obj::Dict(dests)) = doc.dget(&cat, "Dests") {
        let key: String = name.iter().map(|c| char::from(*c)).collect();
        if let Some(v) = doc.dget(&dests, &key) {
            return Some(v);
        }
    }
    let Some(Obj::Dict(names)) = doc.dget(&cat, "Names") else { return None };
    let root = names.get("Dests")?.clone();
    let mut queue = vec![root];
    let mut qi = 0usize;
    while let Some(n) = queue.get(qi).cloned() {
        qi = qi.saturating_add(1);
        if qi > 10_000 {
            break;
        }
        let Obj::Dict(d) = doc.resolve(&n) else { continue };
        if let Some(Obj::Arr(kids)) = doc.dget(&d, "Kids") {
            queue.extend(kids);
            continue;
        }
        if let Some(Obj::Arr(entries)) = doc.dget(&d, "Names") {
            let mut i = 0usize;
            while i.saturating_add(1) < entries.len() {
                let k = entries.get(i).map(|x| doc.resolve(x)).and_then(|x| x.str_bytes());
                if k.as_deref() == Some(name) {
                    return entries.get(i.saturating_add(1)).map(|x| doc.resolve(x));
                }
                i = i.saturating_add(2);
            }
        }
    }
    None
}

fn page_of(doc: &JsDoc, dest: Option<&Obj>) -> f64 {
    let mut d = match dest {
        Some(x) => x.clone(),
        None => return -1.0,
    };
    for _ in 0..3 {
        match &d {
            Obj::Name(n) => d = named_dest(doc, &n.bytes().collect::<Vec<u8>>()).unwrap_or(Obj::Null),
            x if x.is_string() => d = named_dest(doc, &x.str_bytes().unwrap_or_default()).unwrap_or(Obj::Null),
            Obj::Dict(dd) => d = doc.dget(dd, "D").unwrap_or(Obj::Null),
            _ => break,
        }
    }
    let Obj::Arr(a) = d else { return -1.0 };
    match a.first() {
        Some(Obj::Num(n)) if *n != 0.0 => *n,
        Some(Obj::Ref(n, g)) => doc.page_index((*n, *g)).map_or(-1.0, |i| {
            #[allow(clippy::cast_precision_loss)]
            let f = i as f64;
            f
        }),
        _ => -1.0,
    }
}

/// Options of [`open_pdf`].
#[derive(Debug, Clone)]
pub struct OpenOpts {
    pub file_id: String,
    pub name: String,
    pub read_text: bool,
}

/// `openPdf(env, bytes, {fileId, name, readText})` → the record, or the plain-language error.
pub fn open_pdf(env: &mut Env, bytes: &[u8], opts: &OpenOpts) -> Result<Record, String> {
    let doc = match JsDoc::open(bytes) {
        Ok(d) => d,
        Err(OpenError::Password) => return Err(t("open.err.password", &[("file", Fill::from(&opts.name))])),
        Err(OpenError::Read(m)) => return Err(t("open.err.read", &[("file", Fill::from(&opts.name)), ("error", Fill::S(m))])),
    };
    let labels = doc.page_labels();
    let custom = doc.info_custom();
    let ours = custom_truthy(custom_get(&custom, PRIVATE_KEY));
    let columns = custom_json(custom_get(&custom, PRIVATE_COLUMNS_KEY)).unwrap_or_else(|| Value::Array(Vec::new()));
    let layers = match custom_json(custom_get(&custom, PRIVATE_LAYERS_KEY)) {
        Some(Value::Array(a)) => Value::Array(a.into_iter().take(500).filter(|l| l.is_object() || l.is_array()).collect()),
        _ => Value::Array(Vec::new()),
    };
    let blank = blank_info(custom_get(&custom, BLANK_KEY));
    let mut pages: Vec<PageRec> = Vec::new();
    let mut raw: Vec<Vec<AnnotData>> = Vec::new();
    for (i, pg) in doc.pages.iter().enumerate() {
        let view = doc.view(&pg.dict);
        let rotate_raw = doc.rotate(&pg.dict);
        let tb = if opts.read_text {
            let uu = match doc.dget(&pg.dict, "UserUnit") {
                Some(Obj::Num(u)) if u > 0.0 => u,
                _ => 1.0,
            };
            let (tr, w, h) = crate::text::viewport(view, rotate_raw, uu);
            let items: Vec<TextPiece> = crate::text::text_content(&doc, i)
                .into_iter()
                .map(|t| TextPiece { str: t.str, transform: t.transform, width: t.width })
                .collect();
            read_title_block(&items, &Viewport { transform: tr, width: w, height: h })
        } else {
            None
        };
        let label = labels.as_ref().and_then(|l| l.get(i)).map(String::as_str);
        let info = sheet_info(label, &opts.name, i, tb.as_ref(), None);
        let rotate = ((rotate_raw % 360.0) + 360.0) % 360.0;
        pages.push(PageRec { index: i, key: format!("{}:{i}", opts.file_id), info, tb, view, rotate, annot_mode: "enable" });
        raw.push(page_annotations(&doc, i));
    }
    let mut rec = Record {
        id: opts.file_id.clone(),
        name: opts.name.clone(),
        size: bytes.len(),
        pages,
        markups: Vec::new(),
        calibrations: Vec::new(),
        sheet_names: Vec::new(),
        remove_refs: IndexSet::new(),
        layers,
        columns,
        blank,
        bookmarks: read_outline(&doc),
        notes: Vec::new(),
        foreign_count: 0,
        cleaned: false,
        reply_count: 0,
    };
    let need_lib = ours || raw.iter().any(|l| l.iter().any(|a| a.subtype.as_deref().is_some_and(|s| EDITABLE_SUBTYPES.contains(&s))));
    if !need_lib {
        return Ok(rec);
    }
    let mut ldoc = LibDoc::parse(bytes).ok();
    let lib_pages: Vec<(u32, u16)> = ldoc.as_ref().map(LibDoc::pages).unwrap_or_default();
    let n = rec.pages.len();
    let mut has_foreign = vec![false; n];
    let mut has_editable = vec![false; n];
    let mut replies: Vec<RawReply> = Vec::new();
    let mut by_ref: Vec<(String, usize)> = Vec::new();
    let mut from_priv: Vec<bool> = Vec::new();
    for i in 0..n {
        let Some(key) = rec.pages.get(i).map(|p| p.key.clone()) else { continue };
        let rot = rec.pages.get(i).map_or(0.0, |p| p.rotate);
        if let Some(ld) = &ldoc {
            let node = lib_pages.get(i).and_then(|r| ld.get(*r)).and_then(Obj::as_dict).cloned();
            if let Some(node) = &node {
                let cal_raw = plain(node.get(PRIVATE_CAL_KEY), 0);
                let cal: Option<Value> = cal_raw.as_str().and_then(|s| serde_json::from_str(s).ok());
                if let Value::String(nr) = plain(node.get(PRIVATE_SHEET_KEY), 0)
                    && let Ok(nm) = serde_json::from_str::<Value>(&nr)
                {
                    let g = |k: &str| match nm.get(k) {
                        None | Some(Value::Null) => String::new(),
                        Some(v) => cc_markup::js::to_js_string(v),
                    };
                    let clean = (trim(&slice16(&g("number"), 40)).to_string(), trim(&slice16(&g("title"), 200)).to_string());
                    if !clean.0.is_empty() || !clean.1.is_empty() {
                        rec.sheet_names.retain(|(k, _)| *k != key);
                        rec.sheet_names.push((key.clone(), clean.clone()));
                        if let Some(pg) = rec.pages.get_mut(i) {
                            let label = labels.as_ref().and_then(|l| l.get(i)).map(String::as_str);
                            pg.info = sheet_info(label, &opts.name, i, pg.tb.as_ref(), Some(&clean));
                        }
                    }
                }
                if cal.as_ref().is_some_and(cc_markup::units::valid_calibration_json) {
                    let c = cal.unwrap_or(Value::Null);
                    let label = if truthy(c.get("label")) { c.get("label").map(str_val).unwrap_or_default() } else { String::new() };
                    rec.calibrations.push((
                        key.clone(),
                        Cal {
                            mm_per_pt: c.get("mmPerPt").and_then(Value::as_f64).unwrap_or(f64::NAN),
                            unit: str_or_empty(c.get("unit")),
                            label: slice16(&label, 120),
                        },
                    ));
                } else {
                    let vp = plain(node.get("VP").map(|v| ld.lookup(v)), 0);
                    if let Value::Array(vps) = vp {
                        let meas = vps.iter().find(|v| truthy(v.get("Measure")) && truthy(Some(v))).and_then(|v| v.get("Measure"));
                        if let Some(c) = cal_from_measure(meas) {
                            rec.calibrations.push((key.clone(), c));
                        }
                    }
                }
            }
        }
        let annots = raw.get(i).cloned().unwrap_or_default();
        for a in &annots {
            let x = ldoc.as_ref().map(|ld| extras_of(ld, &a.id)).unwrap_or_default();
            let r = annot_to_markup(env, a, &x, &key, rot);
            let Some(r) = r else {
                if a.subtype.as_deref() == Some("Widget")
                    && a.has_appearance
                    && let Some(h) = has_foreign.get_mut(i)
                {
                    *h = true;
                }
                continue;
            };
            match r {
                Annot::Reply(rp) => replies.push(rp),
                Annot::Foreign(m) => {
                    if let Some(h) = has_foreign.get_mut(i) {
                        *h = true;
                    }
                    by_ref.retain(|(k, _)| *k != a.id);
                    by_ref.push((a.id.clone(), rec.markups.len()));
                    rec.markups.push(*m);
                    from_priv.push(false);
                    rec.foreign_count = rec.foreign_count.saturating_add(1);
                }
                Annot::Markup(m, fp) => {
                    if let Some(s) = m.src.as_ref().filter(|s| !s.r#ref.is_empty()) {
                        let k = s.r#ref.clone();
                        by_ref.retain(|(x, _)| *x != k);
                        by_ref.push((k, rec.markups.len()));
                    }
                    rec.markups.push(*m);
                    from_priv.push(fp);
                    rec.remove_refs.insert(a.id.clone());
                    if let Some(h) = has_editable.get_mut(i) {
                        *h = true;
                    }
                }
                Annot::Markups(ms) => {
                    for m in ms {
                        if let Some(s) = m.src.as_ref().filter(|s| !s.r#ref.is_empty()) {
                            let k = s.r#ref.clone();
                            by_ref.retain(|(x, _)| *x != k);
                            by_ref.push((k, rec.markups.len()));
                        }
                        rec.markups.push(m);
                        from_priv.push(false);
                    }
                    rec.remove_refs.insert(a.id.clone());
                    if let Some(h) = has_editable.get_mut(i) {
                        *h = true;
                    }
                }
            }
        }
    }
    // threads
    let reply_to: Vec<(String, String)> = replies.iter().map(|r| (r.r#ref.clone(), r.to.clone())).collect();
    let root_of = |r: &str| -> Option<usize> {
        let mut k = r.to_string();
        let mut n = 0usize;
        while let Some((_, to)) = reply_to.iter().rev().find(|(x, _)| *x == k) {
            if n >= 50 {
                break;
            }
            n = n.saturating_add(1);
            k = to.clone();
        }
        by_ref.iter().rev().find(|(x, _)| *x == k).map(|(_, i)| *i)
    };
    cc_m3d::js::sort_by(&mut replies, |p, q| cc_markup::collate::locale_compare(&p.date, &q.date));
    for rp in &replies {
        let Some(mi) = root_of(&rp.to) else { continue };
        rec.remove_refs.insert(rp.r#ref.clone());
        rec.reply_count = rec.reply_count.saturating_add(1);
        let review = cc_markup::js::lower(&rp.model) == "review";
        let raw_state: String = {
            let mut it = rp.state.chars();
            match it.next() {
                Some(c) => format!("{}{}", cc_markup::js::upper(&c.to_string()), cc_markup::js::lower(it.as_str())),
                None => String::new(),
            }
        };
        let st = if STATUSES.contains(&raw_state.as_str()) {
            raw_state.clone()
        } else if review && (raw_state == "None" || raw_state == "Cancelled") {
            "Open".to_string()
        } else {
            String::new()
        };
        let fp = from_priv.get(mi).copied().unwrap_or(false);
        let Some(m) = rec.markups.get_mut(mi) else { continue };
        let utf16_ge = |a: &str, b: &str| a.encode_utf16().cmp(b.encode_utf16()) != std::cmp::Ordering::Less;
        let foreign_src = m.src.as_ref().is_some_and(|s| !s.subtype.is_empty()) && !fp;
        if review && !st.is_empty() && (m.modified.is_empty() || rp.date.is_empty() || utf16_ge(&rp.date, &m.modified) || foreign_src) {
            m.status = st.clone();
        }
        if !rp.nm.is_empty() && rp.nm == format!("{}-state", m.id) {
            continue;
        }
        let mut ro = Map::new();
        if !rp.nm.is_empty() {
            ro.insert("id".into(), Value::from(rp.nm.clone()));
        }
        ro.insert("author".into(), Value::from(rp.author.clone()));
        ro.insert("date".into(), Value::from(if rp.date.is_empty() { m.modified.clone() } else { rp.date.clone() }));
        let text = if !rp.text.is_empty() {
            rp.text.clone()
        } else if !st.is_empty() {
            status_text(&st, "")
        } else {
            String::new()
        };
        ro.insert("text".into(), Value::from(text));
        ro.insert("kind".into(), Value::from(if review && !st.is_empty() { "status" } else { "comment" }));
        ro.insert("status".into(), Value::from(st));
        ro.insert("photo".into(), Value::from(rp.photo.clone()));
        if let Some(r) = clean_reply(env, &Value::Object(ro))
            && !m.replies.iter().any(|x| x.id == r.id)
        {
            m.replies.push(r);
        }
    }
    let mixed = has_editable.iter().zip(&has_foreign).any(|(e, f)| *e && *f);
    if mixed {
        if let Some(ld) = ldoc.as_mut() {
            for i in 0..n {
                if !has_editable.get(i).copied().unwrap_or(false) {
                    continue;
                }
                let Some(pr) = lib_pages.get(i).copied() else { continue };
                let arr = ld.get(pr).and_then(Obj::as_dict).and_then(|d| d.get("Annots")).map(|a| ld.lookup(a).clone());
                let Some(Obj::Arr(items)) = arr else { continue };
                let keep: Vec<Obj> = items
                    .into_iter()
                    .filter(|it| {
                        let Obj::Ref(n, g) = it else { return true };
                        if rec.remove_refs.contains(&refkey(*n, *g)) {
                            return false;
                        }
                        let irt = ld.get((*n, *g)).and_then(|o| if let Obj::Dict(d) = o { d.get("IRT") } else { None });
                        !matches!(irt, Some(Obj::Ref(a, b)) if rec.remove_refs.contains(&refkey(*a, *b)))
                    })
                    .collect();
                if let Some(d) = ld.get_mut(pr).and_then(Obj::as_dict_mut) {
                    d.set("Annots", Obj::Arr(keep));
                }
            }
            let cleaned = ld.save();
            if JsDoc::open(&cleaned).is_ok() {
                rec.cleaned = true;
            } else {
                rec.notes.push(t0("open.note.twice"));
            }
        } else {
            for i in 0..n {
                if has_editable.get(i).copied().unwrap_or(false)
                    && let Some(p) = rec.pages.get_mut(i)
                {
                    p.annot_mode = "disable";
                }
            }
        }
    } else {
        for i in 0..n {
            if has_editable.get(i).copied().unwrap_or(false)
                && let Some(p) = rec.pages.get_mut(i)
            {
                p.annot_mode = "disable";
            }
        }
    }
    if ldoc.is_none() {
        rec.notes.push(t0("open.note.extras"));
    }
    Ok(rec)
}

/// The record as JSON (canonical numbers).
pub fn record_json(rec: &Record) -> String {
    use cc_m3d::json::Out;
    let mut o = Out::new(true);
    o.raw("{\"id\":");
    o.str(&rec.id);
    o.raw(",\"name\":");
    o.str(&rec.name);
    o.raw(",\"size\":");
    o.num(rec.size as f64);
    o.raw(",\"pages\":[");
    for (i, p) in rec.pages.iter().enumerate() {
        if i > 0 {
            o.ch(',');
        }
        o.raw("{\"index\":");
        o.num(p.index as f64);
        o.raw(",\"key\":");
        o.str(&p.key);
        o.raw(",\"label\":");
        o.str(&p.info.label);
        o.raw(",\"sheetNo\":");
        o.str(&p.info.sheet_no);
        o.raw(",\"name\":");
        o.str(&p.info.name);
        o.raw(",\"title\":");
        o.str(&p.info.title);
        o.raw(",\"from\":");
        o.str(p.info.from);
        o.raw(",\"tb\":");
        match &p.tb {
            Some(tb) => {
                o.raw("{\"number\":");
                o.str(&tb.number);
                o.raw(",\"title\":");
                o.str(&tb.title);
                o.ch('}');
            }
            None => o.raw("null"),
        }
        o.raw(",\"view\":");
        o.nums(&p.view);
        o.raw(",\"rotate\":");
        o.num(p.rotate);
        o.raw(",\"width\":");
        o.num(p.view[2] - p.view[0]);
        o.raw(",\"height\":");
        o.num(p.view[3] - p.view[1]);
        o.raw(",\"annotMode\":");
        o.str(p.annot_mode);
        o.ch('}');
    }
    o.raw("],\"markups\":[");
    for (i, m) in rec.markups.iter().enumerate() {
        if i > 0 {
            o.ch(',');
        }
        cc_markup::json::write_markup(&mut o, m);
    }
    o.raw("],\"calibrations\":{");
    for (i, (k, c)) in rec.calibrations.iter().enumerate() {
        if i > 0 {
            o.ch(',');
        }
        o.key(k);
        cc_markup::json::write_cal(&mut o, c);
    }
    o.raw("},\"sheetNames\":{");
    for (i, (k, (n, t))) in rec.sheet_names.iter().enumerate() {
        if i > 0 {
            o.ch(',');
        }
        o.key(k);
        o.raw("{\"number\":");
        o.str(n);
        o.raw(",\"title\":");
        o.str(t);
        o.ch('}');
    }
    o.raw("},\"removeRefs\":");
    o.strs(&rec.remove_refs.iter().cloned().collect::<Vec<_>>());
    o.raw(",\"layers\":");
    o.value(&rec.layers);
    o.raw(",\"columns\":");
    o.value(&rec.columns);
    o.raw(",\"blank\":");
    match &rec.blank {
        Some(b) => o.value(b),
        None => o.raw("null"),
    }
    o.raw(",\"bookmarks\":");
    fn bm(o: &mut Out, list: &[Bookmark]) {
        o.ch('[');
        for (i, b) in list.iter().enumerate() {
            if i > 0 {
                o.ch(',');
            }
            o.raw("{\"title\":");
            o.str(&b.title);
            o.raw(",\"page\":");
            o.num(b.page);
            o.raw(",\"children\":");
            bm(o, &b.children);
            o.ch('}');
        }
        o.ch(']');
    }
    bm(&mut o, &rec.bookmarks);
    o.raw(",\"notes\":");
    o.strs(&rec.notes);
    o.raw(",\"foreignCount\":");
    o.num(rec.foreign_count as f64);
    o.raw(",\"cleaned\":");
    o.bool(rec.cleaned);
    o.raw(",\"replyCount\":");
    o.num(rec.reply_count as f64);
    o.ch('}');
    o.s
}
