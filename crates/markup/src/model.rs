//! model.js — the markups of an open plan set, with undo / redo.
//!
//! A markup lives on one sheet (`"<fileId>:<pageIndex>"`) in PDF user space (points, y up). Every edit goes through
//! [`Store::change`]: `tx.add` / `tx.update` / `tx.remove` / `tx.set_calibration` / `tx.set_punch_next` /
//! `tx.set_sheet_name`; one change = one undo step. Markups are typed here ([`Markup`]); where the JS builds one from
//! a loose object (`makeMarkup(fields)`) the fields come as parsed JSON and go through the same JS coercions.
//!
//! Time and ids come from an [`Env`] the caller passes in (the clock and the random words `newId` uses), so a session
//! replays exactly.

use crate::addons::clean_addon_data;
use crate::columns::{Column, clean_columns, clean_fields};
use crate::i18n::{Fill, t};
use crate::imagestamp::clean_image_value;
use crate::js::{as_str, is_c0, is_id, max, min, radix36, slice16, str_or_empty, to_iso_s, to_js_string, to_number, trim, truthy};
use crate::units::{Cal, PROTO_NAMES};
use indexmap::{IndexMap, IndexSet};
use serde_json::{Map, Value};

pub type P = [f64; 2];

/// Markup types (the JS `TYPES` keys, in order).
pub const TYPE_IDS: [&str; 23] = [
    "pen",
    "highlight",
    "line",
    "arrow",
    "arc",
    "rect",
    "ellipse",
    "polyline",
    "polygon",
    "cloudrect",
    "cloud",
    "callout",
    "text",
    "note",
    "stamp",
    "image",
    "measure",
    "angle",
    "area",
    "count",
    "space",
    "legend",
    "foreign",
];
pub const STATUSES: [&str; 4] = ["Open", "Accepted", "Rejected", "Completed"];
pub const PUNCH_STATUSES: [&str; 4] = ["Open", "In progress", "Ready for review", "Closed"];
pub const STAMPS: [&str; 6] = ["Approved", "Approved as noted", "Revise and resubmit", "Rejected", "Reviewed", "Void"];

/// `Object.hasOwn(TYPES, t)`.
pub fn is_type(t: &str) -> bool {
    TYPE_IDS.contains(&t)
}
/// `TYPES[t]` is truthy: a type, or a name `Object.prototype` answers for (JS quirk, kept: see units::PROTO_NAMES).
pub fn types_truthy(t: &str) -> bool {
    is_type(t) || PROTO_NAMES.contains(&t)
}
/// `TYPES[t].name`.
pub fn type_name(t: &str) -> String {
    crate::i18n::t0(&format!("type.{t}"))
}
/// `TYPES[t]?.subject || ''`.
pub fn type_subject(t: &str) -> String {
    if t == "foreign" || !is_type(t) { String::new() } else { crate::i18n::t0(&format!("subject.{t}")) }
}

fn key_of(s: &str) -> String {
    // String(s).toLowerCase().replace(/[^a-z0-9]+/g, '_')
    let l = crate::js::lower(s);
    let mut out = String::new();
    let mut run = false;
    for c in l.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
            run = false;
        } else if !run {
            out.push('_');
            run = true;
        }
    }
    out
}
/// `statusName(s)`.
pub fn status_name(s: &str) -> String {
    if STATUSES.contains(&s) { crate::i18n::t0(&format!("status.{}", key_of(s))) } else { s.to_string() }
}
/// `punchStatusName(s)`.
pub fn punch_status_name(s: &str) -> String {
    if PUNCH_STATUSES.contains(&s) { crate::i18n::t0(&format!("punch.status.{}", key_of(s))) } else { s.to_string() }
}
/// `stampLabel(s)`.
pub fn stamp_label(s: &str) -> String {
    if STAMPS.contains(&s) { crate::i18n::t0(&format!("stamp.{}", key_of(s))) } else { s.to_string() }
}
/// `stamps()`.
pub fn stamps() -> Vec<String> {
    STAMPS.iter().map(|s| stamp_label(s)).collect()
}

/// A markup's style. `extra` keeps keys the model does not know (an edit may set them; they travel unchanged).
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    pub color: String,
    pub width: f64,
    pub fill: Option<String>,
    pub fill_opacity: f64,
    pub opacity: f64,
    pub font_size: f64,
    /// `style.arc`: the cloud scallop length, when set
    pub arc: Option<f64>,
    pub extra: Map<String, Value>,
    /// Which numeric fields were JSON `null` (bits: width 1, fillOpacity 2, opacity 4, fontSize 8). JS keeps the
    /// `null`, which counts as 0 in arithmetic and is written back as `null`; the field holds 0 and the bit is set.
    pub nulls: u8,
}

/// `defaultStyle(type)`.
pub fn default_style(kind: &str) -> Style {
    let s = |color: &str, width: f64, fill: Option<&str>, fill_opacity: f64, opacity: f64, font_size: f64| Style {
        color: color.into(),
        width,
        fill: fill.map(str::to_string),
        fill_opacity,
        opacity,
        font_size,
        arc: None,
        extra: Map::new(),
        nulls: 0,
    };
    match kind {
        "highlight" => s("#f5e000", 14.0, None, 0.25, 0.45, 12.0),
        "pen" => s("#d0021b", 1.5, None, 0.25, 1.0, 12.0),
        "text" => s("#d0021b", 0.0, None, 0.25, 1.0, 12.0),
        "callout" => s("#d0021b", 1.0, None, 0.25, 1.0, 12.0),
        "note" => s("#f2c230", 1.0, None, 0.25, 1.0, 12.0),
        "stamp" => s("#1e8c3a", 2.5, None, 0.25, 1.0, 18.0),
        "image" => s("#d0021b", 0.0, None, 0.25, 1.0, 12.0),
        "legend" => s("#17202b", 1.0, Some("#ffffff"), 0.92, 1.0, 9.0),
        "space" => s("#1e8c3a", 1.0, Some("#1e8c3a"), 0.06, 1.0, 11.0),
        "measure" | "angle" => s("#1e48c8", 1.0, None, 0.25, 1.0, 10.0),
        "area" => s("#1e48c8", 1.0, Some("#1e48c8"), 0.15, 1.0, 10.0),
        "count" => s("#c25e00", 1.5, None, 0.25, 1.0, 9.0),
        "cloud" | "cloudrect" => s("#d0021b", 1.5, None, 0.25, 1.0, 12.0),
        _ => s("#d0021b", 2.0, None, 0.25, 1.0, 12.0),
    }
}

impl Style {
    /// `{ ...this, ...patch }` for a parsed style object.
    pub fn merge(&mut self, patch: &Map<String, Value>) {
        for (k, v) in patch {
            self.set(k, v);
        }
    }
    /// One key of a style object.
    pub fn set(&mut self, k: &str, v: &Value) {
        let bit = match k {
            "width" => 1u8,
            "fillOpacity" => 2,
            "opacity" => 4,
            "fontSize" => 8,
            _ => 0,
        };
        if bit != 0 {
            if v.is_null() {
                self.nulls |= bit;
            } else {
                self.nulls &= !bit;
            }
        }
        // JSON null counts as 0 in the JS arithmetic (and is written back as null: `nulls`)
        let num = |v: &Value| if v.is_null() { 0.0 } else { v.as_f64().unwrap_or(f64::NAN) };
        match k {
            "color" => self.color = to_js_string(v),
            "width" => self.width = num(v),
            "fill" => self.fill = if v.is_null() { None } else { Some(to_js_string(v)) },
            "fillOpacity" => self.fill_opacity = num(v),
            "opacity" => self.opacity = num(v),
            "fontSize" => self.font_size = num(v),
            "arc" => self.arc = if v.is_null() { None } else { Some(num(v)) },
            _ => {
                self.extra.insert(k.to_string(), v.clone());
            }
        }
    }
}

/// `stampColor(label)`.
pub fn stamp_color(label: &str) -> &'static str {
    const COLORS: [&str; 6] = ["#1e8c3a", "#1e8c3a", "#c25e00", "#c4122f", "#1e48c8", "#c4122f"];
    let l = crate::js::lower(label);
    if let Some(i) = STAMPS.iter().position(|s| crate::js::lower(s) == l || crate::js::lower(&stamp_label(s)) == l) {
        return COLORS.get(i).copied().unwrap_or("#9a1f86");
    }
    if l.starts_with("approved") {
        "#1e8c3a"
    } else if l.starts_with("revise") {
        "#c25e00"
    } else if l.starts_with("rejected") {
        "#c4122f"
    } else {
        "#9a1f86"
    }
}

/// A punch-list item.
#[derive(Debug, Clone, PartialEq)]
pub struct Punch {
    pub n: f64,
    pub assignee: String,
    pub due: String,
    pub status: String,
}

/// The annotation a markup was read from.
#[derive(Debug, Clone, PartialEq)]
pub struct Src {
    pub r#ref: String,
    pub subtype: String,
}

/// One reply of a markup's thread.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub id: String,
    pub author: String,
    pub date: String,
    pub text: String,
    pub kind: String,
    pub status: String,
    pub photo: Option<String>,
}

/// A markup with every field present (makeMarkup's shape).
#[derive(Debug, Clone, PartialEq)]
pub struct Markup {
    pub id: String,
    pub sheet: Option<String>,
    pub kind: String,
    pub pts: Option<Vec<P>>,
    pub rect: Option<[f64; 4]>,
    pub rot: f64,
    pub style: Style,
    pub text: String,
    pub label: String,
    pub img: String,
    /// `legend.scope` (legend markups only)
    pub legend: Option<String>,
    pub holes: Option<Vec<Vec<P>>>,
    pub layer: String,
    pub fields: IndexMap<String, String>,
    pub xd: Option<Value>,
    pub link: String,
    pub subject: String,
    pub comment: String,
    pub author: String,
    pub created: String,
    pub modified: String,
    pub status: String,
    pub punch: Option<Punch>,
    pub locked: bool,
    pub src: Option<Src>,
    pub replies: Vec<Reply>,
    /// keys an edit set that the model does not know (kept so the object reads back as the JS one)
    pub extra: Map<String, Value>,
}

impl Markup {
    /// The sheet key ('' when unset).
    pub fn sheet(&self) -> &str {
        self.sheet.as_deref().unwrap_or("")
    }
    /// `m.pts || []`.
    pub fn pts(&self) -> &[P] {
        self.pts.as_deref().unwrap_or(&[])
    }
}

/// The clock and the random words `newId` uses (Date.now and crypto.getRandomValues in the JS).
#[derive(Debug, Clone)]
pub struct Env {
    /// milliseconds since 1970 (`Date.now()`)
    pub now_ms: f64,
    /// how many ids have been made (`seq` and the source of the random words)
    pub count: u64,
}

impl Default for Env {
    fn default() -> Self {
        Self { now_ms: 1_791_806_400_000.0, count: 0 }
    }
}

impl Env {
    /// `nowIso()`.
    pub fn now_iso(&self) -> String {
        to_iso_s(self.now_ms).unwrap_or_default()
    }
    /// The two random words of the k-th id. The parity harness hands the JS the same words (its
    /// `crypto.getRandomValues` stand-in); a real session would draw them from the OS.
    pub fn words(k: u64) -> (u32, u32) {
        let k32 = u32::try_from(k & 0xffff_ffff).unwrap_or(0);
        let a = k32.wrapping_mul(0x9E37_79B1).wrapping_add(0x12345);
        let b = (k32 ^ 0x5bd1_e995).wrapping_mul(0x85eb_ca6b).wrapping_add(7);
        (a, b)
    }
    /// `newId()`: 'mc-' + time + '-' + random + sequence (base 36).
    pub fn new_id(&mut self) -> String {
        let (a, b) = Self::words(self.count);
        let seq = self.count;
        self.count = self.count.saturating_add(1);
        let now = crate::js::f2u(self.now_ms);
        let b36 = radix36(u64::from(b));
        format!("mc-{}-{}{}{}", radix36(now), radix36(u64::from(a)), slice16(&b36, 4), radix36(seq))
    }
}

/// `normRect(r)`.
pub fn norm_rect(r: [f64; 4]) -> [f64; 4] {
    [min(r[0], r[2]), min(r[1], r[3]), max(r[0], r[2]), max(r[1], r[3])]
}

/// `cleanLayerName(v)`: plain text, trimmed, ≤ 60 characters.
pub fn clean_layer_name(v: Option<&Value>) -> String {
    let s: String = str_or_empty(v).chars().filter(|c| !is_c0(*c)).collect();
    slice16(trim(&s), 60)
}
/// `cleanLayerName(s)` for text.
pub fn clean_layer_str(s: &str) -> String {
    let s: String = s.chars().filter(|c| !is_c0(*c)).collect();
    slice16(trim(&s), 60)
}

/// `[+p[0], +p[1]]` for each point of a JSON list (non-arrays read as `[NaN, NaN]`).
pub fn pts_of(v: &Value) -> Vec<P> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|p| {
                    let g = |i: usize| p.as_array().and_then(|q| q.get(i)).map_or(f64::NAN, to_number);
                    [g(0), g(1)]
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The four numbers of a JSON rect (`+r[i]`; missing ones NaN).
pub fn rect_of(v: &Value) -> [f64; 4] {
    let g = |i: usize| v.as_array().and_then(|a| a.get(i)).map_or(f64::NAN, to_number);
    [g(0), g(1), g(2), g(3)]
}

/// `cleanReply(r)` → None when it is not a usable reply.
pub fn clean_reply(env: &mut Env, r: &Value) -> Option<Reply> {
    let o = r.as_object()?;
    let kind = if o.get("kind").and_then(Value::as_str) == Some("status") { "status" } else { "comment" };
    let status = match o.get("status").and_then(Value::as_str) {
        Some(s) if kind == "status" && STATUSES.contains(&s) => s.to_string(),
        _ => String::new(),
    };
    let text = slice16(&str_or_empty(o.get("text")), 20000);
    let photo = if kind == "comment" { clean_image_value(o.get("photo")) } else { String::new() };
    if kind == "comment" && trim(&text).is_empty() && photo.is_empty() {
        return None;
    }
    if kind == "status" && status.is_empty() {
        return None;
    }
    let id = match o.get("id") {
        Some(Value::String(s)) if is_id(s, 80) => s.clone(),
        _ => env.new_id(),
    };
    let author = slice16(&str_or_empty(o.get("author")), 120);
    let date = if truthy(o.get("date")) { o.get("date").map(to_js_string).unwrap_or_default() } else { env.now_iso() };
    Some(Reply { id, author, date, text, kind: kind.to_string(), status, photo: if photo.is_empty() { None } else { Some(photo) } })
}

/// `statusText(status, author)`.
pub fn status_text(status: &str, author: &str) -> String {
    if author.is_empty() {
        t("reply.status", &[("status", Fill::S(status_name(status)))])
    } else {
        t("reply.status.by", &[("status", Fill::S(status_name(status))), ("author", Fill::from(author))])
    }
}

/// `statusReply(status, author)`.
pub fn status_reply(env: &mut Env, status: &str, author: &str) -> Option<Reply> {
    let mut o = Map::new();
    o.insert("kind".into(), Value::from("status"));
    o.insert("status".into(), Value::from(status));
    o.insert("author".into(), Value::from(author));
    o.insert("date".into(), Value::from(env.now_iso()));
    o.insert("text".into(), Value::from(status_text(status, author)));
    clean_reply(env, &Value::Object(o))
}

/// `makeMarkup(fields)`: a new markup with every field present.
pub fn make_markup(env: &mut Env, f: &Map<String, Value>) -> Markup {
    let kind = f.get("type").map(to_js_string).unwrap_or_else(|| "undefined".into());
    let now = env.now_iso();
    let id = if truthy(f.get("id")) { f.get("id").map(to_js_string).unwrap_or_default() } else { env.new_id() };
    let sheet = f.get("sheet").and_then(Value::as_str).map(str::to_string);
    let pts = if truthy(f.get("pts")) { f.get("pts").map(pts_of) } else { None };
    let rect = if truthy(f.get("rect")) { f.get("rect").map(|r| norm_rect(rect_of(r))) } else { None };
    let rot = f.get("rot").map_or(0.0, to_number);
    let rot = if rot == 0.0 || rot.is_nan() { 0.0 } else { rot };
    let mut style = default_style(&kind);
    if let Some(Value::Object(s)) = f.get("style") {
        style.merge(s);
    }
    let nn = |k: &str| -> String {
        match f.get(k) {
            None | Some(Value::Null) => String::new(),
            Some(v) => to_js_string(v),
        }
    };
    let img = if kind == "image" { nn("img") } else { String::new() };
    let legend = if kind == "legend" {
        let scope = f.get("legend").and_then(Value::as_object).and_then(|l| l.get("scope")).and_then(Value::as_str);
        Some(if scope == Some("set") { "set".to_string() } else { "sheet".to_string() })
    } else {
        None
    };
    let holes = match f.get("holes") {
        Some(Value::Array(h)) if kind == "area" => Some(h.iter().map(pts_of).collect()),
        _ => None,
    };
    let link: String = nn("link").chars().filter(|c| !is_c0(*c)).collect();
    let link = slice16(trim(&link), 60);
    let subject = match f.get("subject") {
        None | Some(Value::Null) => type_subject(&kind),
        Some(v) => to_js_string(v),
    };
    let created = if truthy(f.get("created")) { nn("created") } else { now.clone() };
    let modified = if truthy(f.get("modified")) {
        nn("modified")
    } else if truthy(f.get("created")) {
        nn("created")
    } else {
        now
    };
    let status = match f.get("status").and_then(Value::as_str) {
        Some(s) if STATUSES.contains(&s) => s.to_string(),
        _ => "Open".to_string(),
    };
    let punch = if truthy(f.get("punch")) {
        let p = f.get("punch").and_then(Value::as_object);
        let g = |k: &str| p.and_then(|p| p.get(k));
        let n = g("n").map_or(f64::NAN, to_number);
        Some(Punch {
            n: if n == 0.0 || n.is_nan() { 0.0 } else { n },
            assignee: if truthy(g("assignee")) { g("assignee").map(to_js_string).unwrap_or_default() } else { String::new() },
            due: if truthy(g("due")) { g("due").map(to_js_string).unwrap_or_default() } else { String::new() },
            status: match g("status").and_then(Value::as_str) {
                Some(s) if PUNCH_STATUSES.contains(&s) => s.to_string(),
                _ => "Open".to_string(),
            },
        })
    } else {
        None
    };
    let src = if truthy(f.get("src")) { f.get("src").and_then(src_of) } else { None };
    let replies = match f.get("replies") {
        Some(Value::Array(list)) => list.iter().filter_map(|r| clean_reply(env, r)).collect(),
        _ => Vec::new(),
    };
    Markup {
        id,
        sheet,
        kind,
        pts,
        rect,
        rot,
        style,
        text: nn("text"),
        label: nn("label"),
        img,
        legend,
        holes,
        layer: clean_layer_name(f.get("layer")),
        fields: clean_fields(f.get("fields")),
        xd: clean_addon_data(f.get("xd")),
        link,
        subject,
        comment: nn("comment"),
        author: nn("author"),
        created,
        modified,
        status,
        punch,
        locked: truthy(f.get("locked")),
        src,
        replies,
        extra: Map::new(),
    }
}

fn src_of(v: &Value) -> Option<Src> {
    let o = v.as_object()?;
    Some(Src { r#ref: str_or_empty(o.get("ref")), subtype: str_or_empty(o.get("subtype")) })
}

const RECT_TYPES: [&str; 8] = ["rect", "ellipse", "text", "stamp", "image", "legend", "cloudrect", "callout"];

/// `MIN_PTS[type]`: `Some(Some(n))`, `Some(None)` for an `Object.prototype` name (truthy, compares as NaN), `None`.
fn min_pts(t: &str) -> Option<Option<usize>> {
    Some(Some(match t {
        "space" | "arc" | "angle" | "polygon" | "cloud" | "area" => 3,
        "pen" | "highlight" | "note" | "callout" | "count" => 1,
        "line" | "arrow" | "measure" | "polyline" => 2,
        x if PROTO_NAMES.contains(&x) => return Some(None),
        _ => return None,
    }))
}

/// `geometryOk(m)`.
pub fn geometry_ok(m: &Markup) -> bool {
    let fin = |v: f64| v.is_finite() && v.abs() < 1e7;
    if RECT_TYPES.contains(&m.kind.as_str()) && !m.rect.is_some_and(|r| r.iter().all(|v| fin(*v))) {
        return false;
    }
    if m.kind == "image" && !(m.img.starts_with("data:image/png;base64,") || m.img.starts_with("data:image/jpeg;base64,")) {
        return false;
    }
    if let Some(h) = &m.holes
        && !(h.len() <= 200 && h.iter().all(|r| r.len() >= 3 && r.iter().all(|p| fin(p[0]) && fin(p[1]))))
    {
        return false;
    }
    if let Some(need) = min_pts(&m.kind) {
        let Some(pts) = &m.pts else { return false };
        if need.is_some_and(|n| pts.len() < n) || pts.len() > 100_000 {
            return false;
        }
        if !pts.iter().all(|p| fin(p[0]) && fin(p[1])) {
            return false;
        }
    }
    true
}

/// A sheet number and title typed by hand.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetName {
    pub number: String,
    pub title: String,
}

/// A layer's view state.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LayerState {
    pub hidden: bool,
    pub locked: bool,
}

/// One recorded edit of an undo step.
#[derive(Debug, Clone)]
pub enum Op {
    Add { id: String, after: Box<Markup>, sheet: String },
    Update { id: String, before: Box<Markup>, after: Box<Markup>, sheet: String },
    Remove { id: String, before: Box<Markup>, order: usize, sheet: String },
    Cal { sheet: String, before: Option<Cal>, after: Option<Cal> },
    Punch { before: f64, after: f64 },
    Name { sheet: String, before: Option<SheetName>, after: Option<SheetName> },
}

impl Op {
    pub fn kind(&self) -> &'static str {
        match self {
            Op::Add { .. } => "add",
            Op::Update { .. } => "update",
            Op::Remove { .. } => "remove",
            Op::Cal { .. } => "cal",
            Op::Punch { .. } => "punch",
            Op::Name { .. } => "name",
        }
    }
    /// `o.sheet` (the sheet it marks changed; '' = none)
    pub fn sheet(&self) -> &str {
        match self {
            Op::Add { sheet, .. }
            | Op::Update { sheet, .. }
            | Op::Remove { sheet, .. }
            | Op::Cal { sheet, .. }
            | Op::Name { sheet, .. } => sheet,
            Op::Punch { .. } => "",
        }
    }
}

/// One undo step.
#[derive(Debug, Clone)]
pub struct Step {
    pub label: String,
    pub ops: Vec<Op>,
}

/// Errors a store edit can raise (the JS throws these).
#[derive(Debug, Clone, PartialEq)]
pub enum StoreError {
    /// `tx.add` with an id the store already has ("duplicate markup id")
    Duplicate,
    /// a value the JS would choke on (TypeError)
    Type(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Duplicate => f.write_str("duplicate markup id"),
            StoreError::Type(s) => write!(f, "TypeError: {s}"),
        }
    }
}

/// The markups of an open plan set.
#[derive(Debug, Clone, Default)]
pub struct Store {
    /// id → markup (insertion order = drawing order)
    pub markups: IndexMap<String, Markup>,
    /// sheet key → calibration
    pub calibrations: IndexMap<String, Cal>,
    pub punch_next: f64,
    /// sheet key → number and title typed by hand
    pub sheet_names: IndexMap<String, SheetName>,
    pub undo_stack: Vec<Step>,
    pub redo_stack: Vec<Step>,
    pub version: u64,
    /// sheets changed since the last save of their file
    pub dirty_sheets: IndexSet<String>,
    /// layer name → view state (not undoable)
    pub layers: IndexMap<String, LayerState>,
    /// the set's custom columns (not undoable)
    pub columns: Vec<Column>,
    pub env: Env,
}

/// The transaction handed to [`Store::change`].
pub struct Tx<'a> {
    store: &'a mut Store,
    ops: Vec<Op>,
}

impl Tx<'_> {
    /// `tx.add(m)`: a full markup is cloned, anything else goes through makeMarkup.
    pub fn add_fields(&mut self, f: &Map<String, Value>) -> Result<Markup, StoreError> {
        let full = truthy(f.get("id")) && truthy(f.get("type")) && truthy(f.get("style"));
        let mk = if full { crate::json::markup_from_value(&Value::Object(f.clone())) } else { make_markup(&mut self.store.env, f) };
        self.add(mk)
    }
    /// `tx.add(markup)` for a typed markup (cloned).
    pub fn add(&mut self, mk: Markup) -> Result<Markup, StoreError> {
        if self.store.markups.contains_key(&mk.id) {
            return Err(StoreError::Duplicate);
        }
        let mk = crate::json::clone_markup(&mk);
        self.store.markups.insert(mk.id.clone(), mk.clone());
        self.ops.push(Op::Add { id: mk.id.clone(), after: Box::new(mk.clone()), sheet: mk.sheet().to_string() });
        Ok(mk)
    }
    /// `tx.update(id, patch, {touch})`.
    pub fn update(&mut self, id: &str, patch: &Map<String, Value>, touch: bool) -> Option<Markup> {
        let old = self.store.markups.get(id)?;
        let before = old.clone();
        let mut m = old.clone();
        for (k, v) in patch {
            if k == "style" {
                continue;
            }
            crate::json::set_key(&mut m, k, v);
        }
        if let Some(sv) = patch.get("style") {
            if truthy(Some(sv)) {
                let mut st = before.style.clone();
                if let Some(o) = sv.as_object() {
                    st.merge(o);
                }
                m.style = st;
            } else {
                crate::json::set_key(&mut m, "style", sv);
            }
        }
        if truthy(patch.get("rect"))
            && let Some(r) = m.rect
        {
            m.rect = Some(norm_rect(r));
        }
        if touch && !patch.contains_key("modified") {
            m.modified = self.store.env.now_iso();
        }
        let m = crate::json::clone_markup(&m);
        self.store.markups.insert(id.to_string(), m.clone());
        self.ops.push(Op::Update {
            id: id.to_string(),
            before: Box::new(before),
            after: Box::new(m.clone()),
            sheet: m.sheet().to_string(),
        });
        Some(m)
    }
    /// `tx.remove(id)`.
    pub fn remove(&mut self, id: &str) {
        let Some(order) = self.store.markups.get_index_of(id) else { return };
        let Some(m) = self.store.markups.shift_remove(id) else { return };
        let sheet = m.sheet().to_string();
        self.ops.push(Op::Remove { id: id.to_string(), before: Box::new(m), order, sheet });
    }
    /// `tx.setCalibration(sheet, cal)`.
    pub fn set_calibration(&mut self, sheet: &str, cal: Option<Cal>) {
        let before = self.store.calibrations.get(sheet).cloned();
        match &cal {
            Some(c) => {
                self.store.calibrations.insert(sheet.to_string(), c.clone());
            }
            None => {
                self.store.calibrations.shift_remove(sheet);
            }
        }
        self.ops.push(Op::Cal { sheet: sheet.to_string(), before, after: cal });
    }
    /// `tx.setPunchNext(n)`.
    pub fn set_punch_next(&mut self, n: f64) {
        self.ops.push(Op::Punch { before: self.store.punch_next, after: n });
        self.store.punch_next = n;
    }
    /// `tx.setSheetName(sheet, name)`.
    pub fn set_sheet_name(&mut self, sheet: &str, name: Option<SheetName>) {
        let before = self.store.sheet_names.get(sheet).cloned();
        match &name {
            Some(n) => {
                self.store.sheet_names.insert(sheet.to_string(), n.clone());
            }
            None => {
                self.store.sheet_names.shift_remove(sheet);
            }
        }
        self.ops.push(Op::Name { sheet: sheet.to_string(), before, after: name });
    }
    /// `tx.get(id)`.
    pub fn get(&self, id: &str) -> Option<&Markup> {
        self.store.markups.get(id)
    }
    /// The store's clock and ids (for makeMarkup inside a change).
    pub fn env(&mut self) -> &mut Env {
        &mut self.store.env
    }
    /// The punch counter as it stands.
    pub fn punch_next(&self) -> f64 {
        self.store.punch_next
    }
}

impl Store {
    pub fn new() -> Self {
        Self { punch_next: 1.0, ..Default::default() }
    }

    fn emit(&mut self) {
        self.version = self.version.saturating_add(1);
    }

    /// `layerNames()`: the layers made here and the ones markups name, in `localeCompare` order.
    pub fn layer_names(&self) -> Vec<String> {
        let mut names: IndexSet<String> = self.layers.keys().cloned().collect();
        for m in self.markups.values() {
            if !m.layer.is_empty() {
                names.insert(m.layer.clone());
            }
        }
        let mut v: Vec<String> = names.into_iter().collect();
        cc_m3d::js::sort_by(&mut v, |a, b| crate::collate::locale_compare(a, b));
        v
    }
    /// `layerState(name)`.
    pub fn layer_state(&self, name: &str) -> LayerState {
        self.layers.get(name).copied().unwrap_or_default()
    }
    /// `setLayer(name, patch)`.
    pub fn set_layer(&mut self, name: &str, hidden: Option<bool>, locked: Option<bool>) {
        let n = clean_layer_str(name);
        if n.is_empty() {
            return;
        }
        let mut st = self.layer_state(&n);
        if let Some(h) = hidden {
            st.hidden = h;
        }
        if let Some(l) = locked {
            st.locked = l;
        }
        self.layers.insert(n, st);
        self.emit();
    }
    /// `dropLayer(name)`.
    pub fn drop_layer(&mut self, name: &str) {
        if self.layers.shift_remove(name).is_some() {
            self.emit();
        }
    }
    /// `setColumns(cols)`.
    pub fn set_columns(&mut self, cols: Option<&Value>) {
        self.columns = clean_columns(cols);
        self.emit();
    }
    /// `shown(m)`.
    pub fn shown(&self, m: &Markup) -> bool {
        m.layer.is_empty() || !self.layer_state(&m.layer).hidden
    }
    /// `open(m)`.
    pub fn open(&self, m: &Markup) -> bool {
        self.shown(m) && (m.layer.is_empty() || !self.layer_state(&m.layer).locked)
    }

    pub fn get(&self, id: &str) -> Option<&Markup> {
        self.markups.get(id)
    }
    /// `onSheet(sheet)`.
    pub fn on_sheet(&self, sheet: &str) -> Vec<&Markup> {
        self.markups.values().filter(|m| m.sheet.as_deref() == Some(sheet)).collect()
    }
    /// `all()`.
    pub fn all(&self) -> Vec<&Markup> {
        self.markups.values().collect()
    }

    /// `change(label, fn)`: run one edit; one undo step when anything changed. An error (the JS throws) leaves what
    /// was already done in place and records no step, as the JS does.
    pub fn change<R>(&mut self, label: &str, f: impl FnOnce(&mut Tx<'_>) -> Result<R, StoreError>) -> Result<R, StoreError> {
        let mut tx = Tx { store: self, ops: Vec::new() };
        let res = f(&mut tx);
        let ops = std::mem::take(&mut tx.ops);
        let r = res?;
        if !ops.is_empty() {
            for o in &ops {
                if !o.sheet().is_empty() {
                    self.dirty_sheets.insert(o.sheet().to_string());
                }
            }
            self.undo_stack.push(Step { label: label.to_string(), ops });
            if self.undo_stack.len() > 500 {
                self.undo_stack.remove(0);
            }
            self.redo_stack.clear();
            self.emit();
        }
        Ok(r)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// `undo()` → the label undone.
    pub fn undo(&mut self) -> Option<String> {
        let step = self.undo_stack.pop()?;
        for o in step.ops.iter().rev() {
            self.apply(o, true);
        }
        let label = step.label.clone();
        self.redo_stack.push(step);
        self.emit();
        Some(label)
    }

    /// `redo()` → the label redone.
    pub fn redo(&mut self) -> Option<String> {
        let step = self.redo_stack.pop()?;
        for o in &step.ops {
            self.apply(o, false);
        }
        let label = step.label.clone();
        self.undo_stack.push(step);
        self.emit();
        Some(label)
    }

    fn apply(&mut self, o: &Op, back: bool) {
        if !o.sheet().is_empty() {
            self.dirty_sheets.insert(o.sheet().to_string());
        }
        match o {
            Op::Add { id, after, .. } => {
                if back {
                    self.markups.shift_remove(id);
                } else {
                    self.markups.insert(id.clone(), (**after).clone());
                }
            }
            Op::Remove { id, before, order, .. } => {
                if back {
                    let at = (*order).min(self.markups.len());
                    self.markups.shift_remove(&before.id);
                    let at = at.min(self.markups.len());
                    self.markups.shift_insert(at, before.id.clone(), (**before).clone());
                } else {
                    self.markups.shift_remove(id);
                }
            }
            Op::Update { id, before, after, .. } => {
                let v = if back { before } else { after };
                self.markups.insert(id.clone(), (**v).clone());
            }
            Op::Cal { sheet, before, after } => match if back { before } else { after } {
                Some(c) => {
                    self.calibrations.insert(sheet.clone(), c.clone());
                }
                None => {
                    self.calibrations.shift_remove(sheet);
                }
            },
            Op::Name { sheet, before, after } => match if back { before } else { after } {
                Some(n) => {
                    self.sheet_names.insert(sheet.clone(), n.clone());
                }
                None => {
                    self.sheet_names.shift_remove(sheet);
                }
            },
            Op::Punch { before, after } => self.punch_next = if back { *before } else { *after },
        }
    }

    /// `load(markups, calibrations, sheetNames, layers, columns)`: one file's sheets (not undoable).
    pub fn load(
        &mut self,
        markups: Vec<Markup>,
        calibrations: Vec<(String, Cal)>,
        sheet_names: Vec<(String, SheetName)>,
        layers: Option<&Value>,
        columns: Option<&Value>,
    ) {
        for c in clean_columns(columns) {
            if !self.columns.iter().any(|x| x.key == c.key || x.name == c.name) {
                self.columns.push(c);
            }
        }
        if let Some(Value::Array(ls)) = layers {
            for l in ls {
                let name = l.as_object().and_then(|o| o.get("name"));
                let n = clean_layer_name(name);
                if !n.is_empty() && !self.layers.contains_key(&n) {
                    let o = l.as_object();
                    self.layers.insert(
                        n,
                        LayerState { hidden: truthy(o.and_then(|o| o.get("hidden"))), locked: truthy(o.and_then(|o| o.get("locked"))) },
                    );
                }
            }
        }
        for m in markups {
            self.markups.insert(m.id.clone(), m);
        }
        for (k, v) in calibrations {
            self.calibrations.insert(k, v);
        }
        for (k, v) in sheet_names {
            self.sheet_names.insert(k, v);
        }
        let mut mx = 0.0f64;
        for m in self.markups.values() {
            if let Some(p) = &m.punch
                && p.n > mx
            {
                mx = p.n;
            }
        }
        self.punch_next = max(self.punch_next, mx + 1.0);
        self.emit();
    }

    /// `dropSheets(prefix)`: forget one file's sheets (not undoable; clears the history).
    pub fn drop_sheets(&mut self, prefix: &str) {
        self.markups.retain(|_, m| !m.sheet().starts_with(prefix));
        self.calibrations.retain(|k, _| !k.starts_with(prefix));
        self.sheet_names.retain(|k, _| !k.starts_with(prefix));
        if prefix.is_empty() {
            self.layers.clear();
            self.columns.clear();
        }
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.emit();
    }

    /// `punchItems()`: markups on the punch list, by number.
    pub fn punch_items(&self) -> Vec<&Markup> {
        let mut v: Vec<&Markup> = self.markups.values().filter(|m| m.punch.is_some()).collect();
        let n = |m: &Markup| m.punch.as_ref().map_or(f64::NAN, |p| p.n);
        cc_m3d::js::sort_by(&mut v, |a, b| n(a) - n(b));
        v
    }
}

/// `typeof v === 'string'` helper re-exported for the modules that read loose objects.
pub fn str_field<'a>(o: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    as_str(o.get(k))
}
