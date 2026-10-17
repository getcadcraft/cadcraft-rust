//! pdf-write.js — save a marked-up copy of a PDF. The ORIGINAL bytes are read the way pdf-lib reads them
//! ([`LibDoc`]); the annotations MarkupCraft took over (and their replies and pop-ups) are dropped; each markup is
//! written as a real annotation with an appearance stream drawn from the same display list as the screen
//! ([`appearance_content`]), its standard keys and the private `/MarkupCraft` JSON; threads become reply annotations;
//! calibrations go on the page (/VP + /MarkupCraftCal), sheet names typed by hand into /MarkupCraftSheet and
//! /PageLabels, markup layers into optional content groups, bookmarks into the outline. New objects are numbered in the
//! order pdf-lib numbers them, so the written file reads back with the same annotation ids.
//!
//! Not byte-identical to pdf-lib's output (object layout, compression, string forms differ); what pdf.js and pdf-lib
//! read back from it is the same — checked by forks/tools/mkdiff.mjs --pdf.

use crate::libdoc::{LibDoc, Ref, deflate};
use crate::obj::{Dict, Obj, Stream};
use crate::read::Bookmark;
use crate::{
    PRIVATE_CAL_KEY, PRIVATE_COLUMNS_KEY, PRIVATE_KEY, PRIVATE_LAYERS_KEY, PRIVATE_LINK_KEY, PRIVATE_PHOTO_KEY, PRIVATE_SHEET_KEY, refkey,
};
use cc_m3d::json::Out;
use cc_markup::font::win_ansi_hex;
use cc_markup::i18n::{Fill, t, t0};
use cc_markup::js::{JsMath, round, to_fixed};
use cc_markup::model::{Markup, Reply, status_text, type_subject};
use cc_markup::shapes::{Item, LegendRow, Seg, ShapeEnv, TextItem, frame, place_text, shapes};
use cc_markup::units::{Cal, measure_area};
use indexmap::IndexSet;
use serde_json::Value;
use std::f64::consts::PI;

/// pdf-write.js `n(v)`.
pub fn n(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let s = to_fixed3(if v.abs() < 1e-6 { 0.0 } else { v });
    let t = s.trim_end_matches('0');
    let t = t.strip_suffix('.').unwrap_or(t);
    if t.is_empty() || t == "-" { "0".into() } else { t.to_string() }
}

/// `x.toFixed(3)`, fast: the exact value of the double rounded half up (on the magnitude) to thousandths with integer
/// arithmetic, the same digits as [`to_fixed`] (which writes out the whole decimal expansion: a third of the save time).
pub fn to_fixed3(x: f64) -> String {
    if x.is_nan() || x.abs() >= 1e21 {
        return to_fixed(x, 3);
    }
    let bits = x.abs().to_bits();
    let exp = i32::try_from((bits >> 52) & 0x7ff).unwrap_or(0);
    let frac = bits & ((1u64 << 52) - 1);
    let (m, e) = if exp == 0 { (frac, -1074) } else { (frac | (1u64 << 52), exp.saturating_sub(1075)) };
    let v = u128::from(m).saturating_mul(1000);
    let n: u128 = if e >= 0 {
        v.checked_shl(e.unsigned_abs()).unwrap_or(u128::MAX)
    } else {
        let sh = e.unsigned_abs();
        if sh >= 128 { 0 } else { v.saturating_add(1u128 << sh.saturating_sub(1)) >> sh }
    };
    format!("{}{}.{:03}", if x < 0.0 { "-" } else { "" }, n / 1000, n % 1000)
}

/// pdf-write.js `rgb(hex)`.
pub fn rgb(hex: Option<&str>) -> [f64; 3] {
    let h = hex.unwrap_or("");
    if h == "paper" {
        return [1.0, 1.0, 1.0];
    }
    let body = h.strip_prefix('#').unwrap_or(h);
    if body.len() == 6 && body.bytes().all(|b| b.is_ascii_hexdigit()) {
        let v = u32::from_str_radix(body, 16).unwrap_or(0);
        let c = |s: u32| f64::from((v >> s) & 255) / 255.0;
        return [c(16), c(8), c(0)];
    }
    [0.0, 0.0, 0.0]
}

/// A graphics state the appearance needs.
#[derive(Debug, Clone, PartialEq)]
pub struct GState {
    pub ca_stroke: f64,
    pub ca_fill: f64,
    pub bm: Option<&'static str>,
}

/// `appearanceContent(list)` → (content, gstates by name in order, characters outside WinAnsi).
pub fn appearance_content(list: &[Item]) -> (String, Vec<(String, GState)>, usize) {
    let mut ops: Vec<String> = Vec::new();
    let mut gstates: Vec<(String, GState)> = Vec::new();
    let mut keys: Vec<(String, String)> = Vec::new();
    let mut missing = 0usize;
    let mut gs = |ca: f64, fa: f64, bm: Option<&'static str>, gstates: &mut Vec<(String, GState)>| -> String {
        let key = format!("{}/{}/{}", n(ca), n(fa), bm.unwrap_or(""));
        if let Some((_, name)) = keys.iter().find(|(k, _)| *k == key) {
            return name.clone();
        }
        let name = format!("GS{}", keys.len());
        keys.push((key, name.clone()));
        gstates.push((name.clone(), GState { ca_stroke: ca, ca_fill: fa, bm }));
        name
    };
    let col = |c: [f64; 3]| format!("{} {} {}", n(c[0]), n(c[1]), n(c[2]));
    for it in list {
        match it {
            Item::Path(p) => {
                let do_fill = p.fill.as_ref().is_some_and(|f| !f.is_empty()) && p.falpha > 0.0;
                let do_stroke = p.stroke.as_ref().is_some_and(|f| !f.is_empty()) && p.w > 0.0;
                if !do_fill && !do_stroke {
                    continue;
                }
                ops.push("q".into());
                let a = if do_stroke { p.alpha } else { 1.0 };
                let fa = if do_fill { p.falpha } else { 1.0 };
                if a < 1.0 || fa < 1.0 || p.blend.is_some() {
                    let name = gs(a, fa, if p.blend == Some("multiply") { Some("Multiply") } else { None }, &mut gstates);
                    ops.push(format!("/{name} gs"));
                }
                if do_fill {
                    ops.push(format!("{} rg", col(rgb(p.fill.as_deref()))));
                }
                if do_stroke {
                    ops.push(format!("{} RG", col(rgb(p.stroke.as_deref()))));
                    ops.push(format!("{} w", n(p.w)));
                    ops.push(format!("{} J", if p.cap == "round" { 1 } else { 0 }));
                    ops.push(format!("{} j", if p.join == "round" { 1 } else { 0 }));
                    if let Some(d) = &p.dash {
                        ops.push(format!("[{}] 0 d", d.iter().map(|x| n(*x)).collect::<Vec<_>>().join(" ")));
                    }
                }
                let path: Vec<String> =
                    p.d.iter()
                        .map(|c| match *c {
                            Seg::M(x, y) => format!("{} {} m", n(x), n(y)),
                            Seg::L(x, y) => format!("{} {} l", n(x), n(y)),
                            Seg::C(a, b, c2, d2, e, f) => format!("{} {} {} {} {} {} c", n(a), n(b), n(c2), n(d2), n(e), n(f)),
                            Seg::Z => "h".into(),
                        })
                        .collect();
                ops.push(path.join(" "));
                let eo = if p.rule == Some("evenodd") { "*" } else { "" };
                ops.push(if do_fill && do_stroke {
                    format!("B{eo}")
                } else if do_fill {
                    format!("f{eo}")
                } else {
                    "S".into()
                });
                ops.push("Q".into());
            }
            Item::Text(ti) => {
                if ti.lines.iter().all(String::is_empty) {
                    continue;
                }
                let (lines, angle) = place_text(ti);
                let rad = (angle * PI) / 180.0;
                let c = round(rad.jcos() * 1e6) / 1e6;
                let s = round(rad.jsin() * 1e6) / 1e6;
                ops.push("q".into());
                if ti.alpha < 1.0 {
                    let name = gs(ti.alpha, ti.alpha, None, &mut gstates);
                    ops.push(format!("/{name} gs"));
                }
                ops.push("BT".into());
                ops.push(format!("/{} {} Tf", if ti.bold { "F2" } else { "F1" }, n(ti.size)));
                ops.push(format!("{} rg", col(rgb(Some(&ti.color)))));
                for ln in lines {
                    if ln.s.is_empty() {
                        continue;
                    }
                    let (hex, miss) = win_ansi_hex(&ln.s);
                    missing = missing.saturating_add(miss);
                    ops.push(format!("{} {} {} {} {} {} Tm {hex} Tj", n(c), n(s), n(-s), n(c), n(ln.x), n(ln.y)));
                }
                ops.push("ET".into());
                ops.push("Q".into());
            }
            Item::Image(_) => {}
        }
    }
    (ops.join("\n"), gstates, missing)
}

/// One number format of a /Measure dictionary.
struct NumFmt {
    u: &'static str,
    c: f64,
    d: f64,
    f: &'static str,
}

/// `measureSpec(cal)`.
fn measure_spec(cal: &Cal) -> (String, Vec<NumFmt>, Vec<NumFmt>, Vec<NumFmt>) {
    let per = cal.mm_per_pt;
    let r = if cal.label.is_empty() { t0("cal.calibrated") } else { cal.label.clone() };
    let nf = |u, c, d, f| NumFmt { u, c, d, f };
    match cal.unit.as_str() {
        "ft-in" => (
            r,
            vec![nf("ft", per / 304.8, 1.0, "D")],
            vec![nf("'", 1.0, 1.0, "T"), nf("\"", 12.0, 16.0, "F")],
            vec![nf("sq ft", 1.0, 100.0, "D")],
        ),
        "ft" => (r, vec![nf("ft", per / 304.8, 100.0, "D")], vec![nf("ft", 1.0, 100.0, "D")], vec![nf("sq ft", 1.0, 100.0, "D")]),
        "in" => (r, vec![nf("in", per / 25.4, 100.0, "D")], vec![nf("in", 1.0, 100.0, "D")], vec![nf("sq in", 1.0, 100.0, "D")]),
        "m" => (r, vec![nf("m", per / 1000.0, 1000.0, "D")], vec![nf("m", 1.0, 1000.0, "D")], vec![nf("sq m", 1.0, 100.0, "D")]),
        _ => (r, vec![nf("mm", per, 1.0, "D")], vec![nf("mm", 1.0, 1.0, "D")], vec![nf("sq mm", 1.0, 1.0, "D")]),
    }
}

fn strip_parens(s: &str) -> String {
    s.chars().filter(|c| !matches!(c, '(' | ')' | '\\')).collect()
}

fn measure_obj(cal: &Cal) -> Obj {
    let (r, x, d, a) = measure_spec(cal);
    let fmt = |f: &NumFmt, clean: bool| {
        Obj::Dict(
            Dict::new()
                .with("Type", Obj::name("NumberFormat"))
                .with("U", Obj::lit(&if clean { strip_parens(f.u) } else { f.u.to_string() }))
                .with("C", Obj::Num(f.c))
                .with("D", Obj::Num(f.d))
                .with("F", Obj::name(f.f)),
        )
    };
    Obj::Dict(
        Dict::new()
            .with("Type", Obj::name("Measure"))
            .with("Subtype", Obj::name("RL"))
            .with("R", Obj::lit(&strip_parens(&r)))
            .with("X", Obj::Arr(x.iter().map(|f| fmt(f, false)).collect()))
            .with("D", Obj::Arr(d.iter().map(|f| fmt(f, true)).collect()))
            .with("A", Obj::Arr(a.iter().map(|f| fmt(f, false)).collect())),
    )
}

/// `camel(s)`.
fn camel(s: &str) -> String {
    let cleaned: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || c == ' ' { c } else { ' ' }).collect();
    let words: Vec<String> = cc_markup::js::trim(&cleaned)
        .split(|c: char| cc_markup::js::is_ws(c))
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut ch = w.chars();
            match ch.next() {
                Some(f) => format!("{}{}", f.to_uppercase(), cc_markup::js::lower(ch.as_str())),
                None => String::new(),
            }
        })
        .collect();
    let j = words.concat();
    if j.is_empty() { "Stamp".into() } else { j }
}

/// `privateData(m, rect)`: the private JSON (JSON.stringify of the object pdf-write.js builds).
pub fn private_data(m: &Markup, rect: [f64; 4]) -> String {
    let mv = cc_markup::json::markup_value(m);
    let mut o = Out::new(false);
    o.raw("{\"v\":1,\"id\":");
    o.str(&m.id);
    o.raw(",\"type\":");
    o.str(&m.kind);
    for k in ["pts", "rect", "rot", "style", "text", "label", "subject", "comment", "author", "created", "modified", "status", "punch"] {
        o.ch(',');
        o.key(k);
        o.value(mv.get(k).unwrap_or(&Value::Null));
    }
    if !m.layer.is_empty() {
        o.raw(",\"layer\":");
        o.str(&m.layer);
    }
    if m.kind == "image" {
        o.raw(",\"img\":");
        o.str(&m.img);
    }
    if !m.link.is_empty() {
        o.raw(",\"link\":");
        o.str(&m.link);
    }
    if let Some(l) = mv.get("legend") {
        o.raw(",\"legend\":");
        o.value(l);
    }
    if !m.fields.is_empty() {
        o.raw(",\"fields\":");
        o.value(mv.get("fields").unwrap_or(&Value::Null));
    }
    if m.holes.as_ref().is_some_and(|h| !h.is_empty()) {
        o.raw(",\"holes\":");
        o.value(mv.get("holes").unwrap_or(&Value::Null));
    }
    if let Some(x) = &m.xd {
        o.raw(",\"xd\":");
        o.value(x);
    }
    o.raw(",\"ar\":");
    o.nums(&rect.map(|v| round(v * 1000.0) / 1000.0));
    o.ch('}');
    o.s
}

/// `boundsOfList(list, m)`.
fn bounds_of_list(list: &[Item], m: &Markup) -> Option<[f64; 4]> {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut add = |x: f64, y: f64, h: f64| {
        if x - h < x0 {
            x0 = x - h;
        }
        if y - h < y0 {
            y0 = y - h;
        }
        if x + h > x1 {
            x1 = x + h;
        }
        if y + h > y1 {
            y1 = y + h;
        }
    };
    for it in list {
        match it {
            Item::Text(TextItem { bbox, .. }) | Item::Image(cc_markup::shapes::ImageItem { bbox, .. }) => {
                add(bbox[0], bbox[1], 0.0);
                add(bbox[2], bbox[3], 0.0);
            }
            Item::Path(p) => {
                let stroked = p.stroke.as_ref().is_some_and(|s| !s.is_empty());
                let h = if stroked { p.w / 2.0 + if p.join == "miter" { p.w } else { 0.0 } } else { 0.0 };
                for c in &p.d {
                    match *c {
                        Seg::M(x, y) | Seg::L(x, y) => add(x, y, h),
                        Seg::C(a, b, c2, d2, e, f) => {
                            add(a, b, h);
                            add(c2, d2, h);
                            add(e, f, h);
                        }
                        Seg::Z => {}
                    }
                }
            }
        }
    }
    if !x0.is_finite() {
        return m.rect;
    }
    Some([x0, y0, x1, y1])
}

/// Which markups are drawn into the page.
#[derive(Debug, Clone, Default)]
pub enum Flatten {
    #[default]
    None,
    All,
    Some(IndexSet<String>),
}

/// One page of a save.
#[derive(Debug, Clone, Default)]
pub struct WritePage {
    pub index: usize,
    pub markups: Vec<Markup>,
    pub cal: Option<Cal>,
    /// the calibration object as the page holds it (JSON.stringify(pg.cal) goes into /MarkupCraftCal)
    pub cal_json: Option<Value>,
    pub badge: f64,
    pub sheet_name: Option<Value>,
    pub cal_cleared: bool,
    /// `legendOf(m)` answers by markup id
    pub legends: Vec<(String, Vec<LegendRow>)>,
}

/// A markup layer.
#[derive(Debug, Clone)]
pub struct LayerIn {
    pub name: Value,
    pub hidden: bool,
    pub locked: bool,
}

/// `writeMarkedUp` options.
#[derive(Debug, Clone, Default)]
pub struct WriteOpts {
    pub pages: Vec<WritePage>,
    pub remove_refs: IndexSet<String>,
    pub labels: Option<Vec<Value>>,
    pub flatten: Flatten,
    pub layers: Vec<LayerIn>,
    /// `linkPage(link)` answers: link → page index
    pub link_pages: Vec<(String, f64)>,
    pub columns: Option<Value>,
    pub bookmarks: Option<Vec<Bookmark>>,
    /// `new Date()` (ms)
    pub now_ms: f64,
}

/// What the save did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub written: usize,
    pub flattened: usize,
    pub missing: usize,
    pub pages: usize,
    pub replies: usize,
    pub photos: usize,
}

/// `PDFString.fromDate(new Date(iso))` (an unreadable date = now).
fn pdf_date_obj(iso: &str, now: f64) -> Obj {
    let t0_ = cc_markup::js::parse_date(iso);
    let t = if t0_.is_nan() { now } else { t0_ };
    let (y, mo, d, h, mi, s, _) = cc_markup::js::civil(t);
    let year = if y < 0 { format!("-{:03}", y.unsigned_abs()) } else { format!("{y:04}") };
    Obj::lit(&format!("D:{year}{mo:02}{d:02}{h:02}{mi:02}{s:02}Z"))
}

fn txt(s: &str) -> Obj {
    Obj::hex_text(s)
}

fn nm_clean(id: &str) -> String {
    id.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')).collect()
}

/// pdf-lib's `addRandomSuffix` generator (SimpleRNG seeded with 1).
struct Rng(f64);
impl Rng {
    fn next_int(&mut self) -> f64 {
        let x = self.0.jsin() * 10000.0;
        self.0 += 1.0;
        x - x.floor()
    }
}

/// `writeMarkedUp(lib, bytes, opts)` → (the new file, report), or the plain-language error.
pub fn write_marked_up(bytes: &[u8], opts: &WriteOpts) -> Result<(Vec<u8>, Report), String> {
    let mut doc = LibDoc::parse(bytes)?;
    if doc.is_encrypted() {
        return Err(t0("save.err.encrypted"));
    }
    let now = opts.now_ms;
    let mut report = Report::default();
    let mut rng = Rng(1.0);
    let helv = doc.register(Obj::Dict(
        Dict::new()
            .with("Type", Obj::name("Font"))
            .with("Subtype", Obj::name("Type1"))
            .with("BaseFont", Obj::name("Helvetica"))
            .with("Encoding", Obj::name("WinAnsiEncoding")),
    ));
    let helvb = doc.register(Obj::Dict(
        Dict::new()
            .with("Type", Obj::name("Font"))
            .with("Subtype", Obj::name("Type1"))
            .with("BaseFont", Obj::name("Helvetica-Bold"))
            .with("Encoding", Obj::name("WinAnsiEncoding")),
    ));
    let flate = |data: &[u8], dict: Dict| -> Obj {
        Obj::Stream(Box::new(Stream { dict: dict.with("Filter", Obj::name("FlateDecode")), data: deflate(data) }))
    };
    let empty_ap = doc.register(flate(
        b"",
        Dict::new()
            .with("Type", Obj::name("XObject"))
            .with("Subtype", Obj::name("Form"))
            .with("FormType", Obj::Num(1.0))
            .with("BBox", Obj::nums(&[0.0, 0.0, 1.0, 1.0])),
    ));
    let pdf_pages = doc.pages();
    let refo = |r: Ref| Obj::Ref(r.0, r.1);

    // layers → optional content groups
    let mut layer_list: Vec<(String, bool, bool)> = Vec::new();
    let markup_layers: Vec<LayerIn> = opts
        .pages
        .iter()
        .flat_map(|p| p.markups.iter().map(|m| LayerIn { name: Value::String(m.layer.clone()), hidden: false, locked: false }))
        .collect();
    for l in opts.layers.iter().chain(markup_layers.iter()) {
        let nm = if cc_markup::js::truthy(Some(&l.name)) { cc_markup::js::to_js_string(&l.name) } else { String::new() };
        let nm = cc_markup::js::trim(&nm).to_string();
        if !nm.is_empty() && !layer_list.iter().any(|(x, _, _)| *x == nm) {
            layer_list.push((nm, l.hidden, l.locked));
        }
    }
    let mut ocg_of: Vec<(String, Ref)> = Vec::new();
    {
        let cat_ref = doc.root_ref();
        let cat = cat_ref.and_then(|r| doc.get(r)).and_then(Obj::as_dict).cloned().unwrap_or_default();
        let ocp = cat.get("OCProperties").map(|o| doc.lookup(o).clone()).and_then(|o| if let Obj::Dict(d) = o { Some(d) } else { None });
        let is_ours = |doc: &LibDoc, r: &Obj| -> bool {
            match r {
                Obj::Ref(a, b) => {
                    doc.get((*a, *b)).and_then(|o| if let Obj::Dict(d) = o { Some(d.has(PRIVATE_KEY)) } else { None }).unwrap_or(false)
                }
                _ => false,
            }
        };
        let keep_arr = |doc: &LibDoc, arr: Option<&Obj>| -> Vec<Obj> {
            match arr.map(|a| doc.lookup(a)) {
                Some(Obj::Arr(a)) => a.iter().filter(|x| !is_ours(doc, x)).cloned().collect(),
                _ => Vec::new(),
            }
        };
        let d0 = ocp
            .as_ref()
            .and_then(|o| o.get("D"))
            .map(|d| doc.lookup(d).clone())
            .and_then(|o| if let Obj::Dict(d) = o { Some(d) } else { None });
        let mut ocgs = ocp.as_ref().map(|o| keep_arr(&doc, o.get("OCGs"))).unwrap_or_default();
        let has_order = d0.as_ref().and_then(|d| d.get("Order")).map(|o| matches!(doc.lookup(o), Obj::Arr(_))).unwrap_or(false);
        let mut order = if has_order { keep_arr(&doc, d0.as_ref().and_then(|d| d.get("Order"))) } else { ocgs.clone() };
        let mut off = d0.as_ref().map(|d| keep_arr(&doc, d.get("OFF"))).unwrap_or_default();
        for (name, hidden, _) in &layer_list {
            let r = doc
                .register(Obj::Dict(Dict::new().with("Type", Obj::name("OCG")).with("Name", txt(name)).with(PRIVATE_KEY, Obj::Bool(true))));
            ocg_of.push((name.clone(), r));
            ocgs.push(refo(r));
            order.push(refo(r));
            if *hidden {
                off.push(refo(r));
            }
        }
        if let Some(cr) = cat_ref {
            if !ocgs.is_empty() {
                let mut d = d0.clone().unwrap_or_default();
                if d0.is_none() {
                    d.set("Name", txt("MarkupCraft"));
                }
                d.set("Order", Obj::Arr(order));
                if off.is_empty() {
                    d.delete("OFF");
                } else {
                    d.set("OFF", Obj::Arr(off));
                }
                // pdf-lib changes the existing /D dictionary in place: an indirect one is updated too
                if let Some(Obj::Ref(a, b)) = ocp.as_ref().and_then(|o| o.get("D")) {
                    let rr = (*a, *b);
                    if matches!(doc.get(rr), Some(Obj::Dict(_))) {
                        doc.put(rr, Obj::Dict(d.clone()));
                    }
                }
                if let Some(c) = doc.get_mut(cr).and_then(Obj::as_dict_mut) {
                    c.set("OCProperties", Obj::Dict(Dict::new().with("OCGs", Obj::Arr(ocgs)).with("D", Obj::Dict(d))));
                }
            } else if ocp.is_some()
                && let Some(c) = doc.get_mut(cr).and_then(Obj::as_dict_mut)
            {
                c.delete("OCProperties");
            }
        }
    }

    let mut push_gs: Option<Ref> = None;
    let mut normalized: Vec<Ref> = Vec::new();
    let mut pop_gs: Option<Ref> = None;
    for pg in &opts.pages {
        let Some(&page_ref) = pdf_pages.get(pg.index) else { continue };
        report.pages = report.pages.saturating_add(1);
        let page_dict = doc.get(page_ref).and_then(Obj::as_dict).cloned().unwrap_or_default();
        let mut keep: Vec<Obj> = Vec::new();
        if let Some(Obj::Arr(old)) = page_dict.get("Annots").map(|a| doc.lookup(a).clone()) {
            for item in old {
                let key = item.as_ref().map(|(a, b)| refkey(a, b));
                if key.as_ref().is_some_and(|k| opts.remove_refs.contains(k)) {
                    continue;
                }
                let dict = match &item {
                    Obj::Ref(a, b) => doc.get((*a, *b)).and_then(|o| if let Obj::Dict(d) = o { Some(d.clone()) } else { None }),
                    Obj::Dict(d) => Some(d.clone()),
                    _ => None,
                };
                if let Some(d) = &dict {
                    let sub = d.get("Subtype").and_then(Obj::as_name);
                    if d.has(PRIVATE_LINK_KEY) || (sub == Some("FileAttachment") && d.has(PRIVATE_PHOTO_KEY)) {
                        continue;
                    }
                    if let Some(Obj::Ref(a, b)) = d.get("IRT")
                        && opts.remove_refs.contains(&refkey(*a, *b))
                    {
                        continue;
                    }
                    if sub == Some("Popup")
                        && let Some(Obj::Ref(a, b)) = d.get("Parent")
                        && opts.remove_refs.contains(&refkey(*a, *b))
                    {
                        continue;
                    }
                }
                keep.push(item);
            }
        }
        let env = ShapeEnv { cal: pg.cal.clone(), badge: pg.badge, legend: None };
        let mut content_ref: Option<Ref> = None;
        let mut page_ops: Vec<String> = Vec::new();
        for m in &pg.markups {
            if m.kind == "foreign" || m.locked {
                let parent =
                    m.src.as_ref().and_then(|s| crate::read::parse_refkey(&s.r#ref)).filter(|r| matches!(doc.get(*r), Some(Obj::Dict(_))));
                write_thread(&mut doc, &mut keep, &mut report, m, parent, m.rect, page_ref, empty_ap, now);
                continue;
            }
            let legend_rows = if m.kind == "legend" { pg.legends.iter().find(|(id, _)| *id == m.id).map(|(_, r)| r.clone()) } else { None };
            let list = shapes(m, &ShapeEnv { legend: legend_rows.clone(), ..env.clone() });
            let Some(b) = bounds_of_list(&list, m) else { continue };
            let mut rect = [b[0] - 1.0, b[1] - 1.0, b[2] + 1.0, b[3] + 1.0];
            let wv = if m.style.width == 0.0 || m.style.width.is_nan() { 0.0 } else { m.style.width };
            let min_d = 2.0 * wv + 2.0;
            let grow = min_d - (rect[2] - rect[0]);
            if grow > 0.0 {
                rect[0] -= grow / 2.0;
                rect[2] += grow / 2.0;
            }
            let grow = min_d - (rect[3] - rect[1]);
            if grow > 0.0 {
                rect[1] -= grow / 2.0;
                rect[3] += grow / 2.0;
            }
            let no_images: Vec<Item> = list.iter().filter(|i| !matches!(i, Item::Image(_))).cloned().collect();
            let (mut content, gstates, miss) = appearance_content(&no_images);
            let mut img_ref: Option<Ref> = None;
            if m.kind == "image" {
                let Some(info) = cc_markup::imagestamp::image_info(&m.img) else { continue };
                let Some(r) = crate::image::embed(&mut doc, &info) else { continue };
                img_ref = Some(r);
                let f = frame(if m.rot == 0.0 || m.rot.is_nan() { 0.0 } else { m.rot }, m.rect.unwrap_or([0.0; 4]));
                let cm = [f.u[0] * f.w, f.u[1] * f.w, -f.v[0] * f.h, -f.v[1] * f.h, f.o[0] + f.v[0] * f.h, f.o[1] + f.v[1] * f.h]
                    .map(|v| cc_markup::js::ns(cc_markup::js::str_to_number(&to_fixed(v, 4))));
                content = format!("q {} cm /MCImg Do Q\n{content}", cm.join(" "));
            }
            report.missing = report.missing.saturating_add(miss);
            let mut res = Dict::new().with("Font", Obj::Dict(Dict::new().with("F1", refo(helv)).with("F2", refo(helvb))));
            if let Some(r) = img_ref {
                res.set("XObject", Obj::Dict(Dict::new().with("MCImg", refo(r))));
            }
            if !gstates.is_empty() {
                let mut eg = Dict::new();
                for (name, g) in &gstates {
                    let mut gd =
                        Dict::new().with("Type", Obj::name("ExtGState")).with("CA", Obj::Num(g.ca_stroke)).with("ca", Obj::Num(g.ca_fill));
                    if let Some(bm) = g.bm {
                        gd.set("BM", Obj::name(bm));
                    }
                    eg.set(name, Obj::Dict(gd));
                }
                res.set("ExtGState", Obj::Dict(eg));
            }
            let ap_ref = doc.register(flate(
                content.as_bytes(),
                Dict::new()
                    .with("Type", Obj::name("XObject"))
                    .with("Subtype", Obj::name("Form"))
                    .with("FormType", Obj::Num(1.0))
                    .with("BBox", Obj::nums(&rect))
                    .with("Resources", Obj::Dict(res)),
            ));
            let s = &m.style;
            let color = rgb(Some(&s.color));
            let mut d = Dict::new()
                .with("Type", Obj::name("Annot"))
                .with("Rect", Obj::nums(&rect))
                .with("F", Obj::Num(4.0))
                .with("P", refo(page_ref))
                .with("NM", Obj::lit(&nm_clean(&m.id)))
                .with("T", txt(&m.author))
                .with("Subj", txt(if !m.subject.is_empty() { &m.subject } else { "" }))
                .with("M", pdf_date_obj(&m.modified, now))
                .with("CreationDate", pdf_date_obj(&m.created, now))
                .with("C", Obj::nums(&color))
                .with("CA", Obj::Num(cc_markup::js::max(0.0, cc_markup::js::min(1.0, s.opacity))))
                .with(
                    "BS",
                    Obj::Dict(
                        Dict::new()
                            .with("Type", Obj::name("Border"))
                            .with("W", Obj::Num(cc_markup::js::max(0.0, wv)))
                            .with("S", Obj::name("S")),
                    ),
                )
                .with("AP", Obj::Dict(Dict::new().with("N", refo(ap_ref))));
            if m.subject.is_empty() {
                d.set("Subj", txt(&type_subject(&m.kind)));
            }
            d.set(PRIVATE_KEY, txt(&private_data(m, rect)));
            let area_v =
                if m.kind == "area" { Some(measure_area(m.pts(), pg.cal.as_ref(), m.holes.as_deref().unwrap_or(&[]))) } else { None };
            let join = |parts: &[&str]| parts.iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join("\n");
            let contents = match m.kind.as_str() {
                "text" | "callout" => m.text.clone(),
                "measure" | "angle" => {
                    let first = list
                        .iter()
                        .find_map(|i| if let Item::Text(t) = i { Some(t.lines.first().cloned().unwrap_or_default()) } else { None })
                        .unwrap_or_default();
                    join(&[&first, &m.comment])
                }
                "area" => {
                    let (a, p) = area_v.clone().unwrap_or_default();
                    let v = if a.is_empty() {
                        String::new()
                    } else {
                        t("pdf.contents.area", &[("area", Fill::S(a)), ("perimeter", Fill::S(p))])
                    };
                    join(&[&v, &m.comment])
                }
                "count" => {
                    let group = if m.label.is_empty() { t0("count.default") } else { m.label.clone() };
                    let tag = t("count.tag", &[("group", Fill::S(group)), ("n", Fill::from(m.pts().len()))]);
                    join(&[&tag, &m.comment])
                }
                "stamp" => join(&[&m.label, &m.comment]),
                _ => m.comment.clone(),
            };
            d.set("Contents", txt(&contents));
            let ic = s.fill.as_ref().filter(|f| !f.is_empty() && f.as_str() != "paper").map(|f| rgb(Some(f)));
            let half = wv / 2.0;
            let flat_pts = |pts: &[[f64; 2]]| Obj::nums(&pts.iter().flat_map(|p| [p[0], p[1]]).collect::<Vec<_>>());
            match m.kind.as_str() {
                "rect" | "cloudrect" | "ellipse" => {
                    d.set("Subtype", Obj::name(if m.kind == "ellipse" { "Circle" } else { "Square" }));
                    let r = m.rect.unwrap_or([f64::NAN; 4]);
                    let rd = [r[0] - half - rect[0], rect[3] - (r[3] + half), rect[2] - (r[2] + half), r[1] - half - rect[1]]
                        .map(|v| cc_markup::js::max(0.0, v));
                    d.set("RD", Obj::nums(&rd));
                    if let Some(c) = ic {
                        d.set("IC", Obj::nums(&c));
                    }
                    if m.kind == "cloudrect" {
                        d.set("BE", Obj::Dict(Dict::new().with("S", Obj::name("C")).with("I", Obj::Num(1.0))));
                    }
                }
                "polygon" | "cloud" => {
                    d.set("Subtype", Obj::name("Polygon"));
                    d.set("Vertices", flat_pts(m.pts()));
                    if let Some(c) = ic {
                        d.set("IC", Obj::nums(&c));
                    }
                    if m.kind == "cloud" {
                        d.set("BE", Obj::Dict(Dict::new().with("S", Obj::name("C")).with("I", Obj::Num(1.0))));
                    }
                }
                "area" => {
                    d.set("Subtype", Obj::name("Polygon"));
                    d.set("Vertices", flat_pts(m.pts()));
                    if let Some(c) = ic {
                        d.set("IC", Obj::nums(&c));
                    }
                    d.set("IT", Obj::name("PolygonDimension"));
                    if let Some(c) = &pg.cal {
                        d.set("Measure", measure_obj(c));
                    }
                }
                "count" => {
                    d.set("Subtype", Obj::name("Ink"));
                    d.set("IT", Obj::name("CountMarks"));
                    let r = cc_markup::js::max(3.0, s.font_size * 0.8);
                    let rings: Vec<Obj> = m
                        .pts()
                        .iter()
                        .map(|p| {
                            let mut ring = Vec::new();
                            for k in 0..=12 {
                                let a = (f64::from(k) / 12.0) * PI * 2.0;
                                ring.push(p[0] + a.jcos() * r);
                                ring.push(p[1] + a.jsin() * r);
                            }
                            Obj::nums(&ring)
                        })
                        .collect();
                    d.set("InkList", Obj::Arr(rings));
                }
                "polyline" | "angle" => {
                    d.set("Subtype", Obj::name("PolyLine"));
                    d.set("Vertices", flat_pts(m.pts()));
                }
                "arc" => {
                    d.set("Subtype", Obj::name("PolyLine"));
                    let pts = m.pts();
                    let arc = if let [a, b, c] = pts { cc_markup::arcs::arc_through(*a, *b, *c) } else { None };
                    let verts: Vec<f64> = match arc {
                        Some(arc) => {
                            let k = cc_markup::js::max(8.0, (arc.sweep.abs() / (PI / 32.0)).ceil());
                            let k = cc_markup::js::f2u(k).min(100_000);
                            #[allow(clippy::cast_precision_loss)]
                            let kf = k as f64;
                            (0..=k)
                                .flat_map(|i| {
                                    #[allow(clippy::cast_precision_loss)]
                                    let a = arc.a0 + (arc.sweep * i as f64) / kf;
                                    [arc.c[0] + arc.r * a.jcos(), arc.c[1] + arc.r * a.jsin()]
                                })
                                .collect()
                        }
                        None => {
                            let a = pts.first().copied().unwrap_or([f64::NAN; 2]);
                            let z = pts.last().copied().unwrap_or([f64::NAN; 2]);
                            vec![a[0], a[1], z[0], z[1]]
                        }
                    };
                    d.set("Vertices", Obj::nums(&verts));
                }
                "line" | "arrow" | "measure" => {
                    d.set("Subtype", Obj::name("Line"));
                    let p = m.pts();
                    let g = |i: usize| p.get(i).copied().unwrap_or([f64::NAN; 2]);
                    d.set("L", Obj::nums(&[g(0)[0], g(0)[1], g(1)[0], g(1)[1]]));
                    d.set(
                        "LE",
                        if m.kind == "measure" {
                            Obj::Arr(vec![Obj::name("Butt"), Obj::name("Butt")])
                        } else {
                            Obj::Arr(vec![Obj::name("None"), Obj::name(if m.kind == "arrow" { "ClosedArrow" } else { "None" })])
                        },
                    );
                    if m.kind == "arrow" {
                        d.set("IC", Obj::nums(&color));
                    }
                    if m.kind == "measure" {
                        d.set("IT", Obj::name("LineDimension"));
                        d.set("Cap", Obj::Bool(true));
                        if let Some(c) = &pg.cal {
                            d.set("Measure", measure_obj(c));
                        }
                    }
                }
                "pen" | "highlight" => {
                    d.set("Subtype", Obj::name("Ink"));
                    d.set("InkList", Obj::Arr(vec![flat_pts(m.pts())]));
                    if m.kind == "highlight" {
                        d.set("IT", Obj::name("InkHighlight"));
                    }
                }
                "text" | "callout" => {
                    d.set("Subtype", Obj::name("FreeText"));
                    d.set("DA", Obj::lit(&format!("/Helv {} Tf {} {} {} rg", n(s.font_size), n(color[0]), n(color[1]), n(color[2]))));
                    d.set("DS", Obj::lit(&format!("font: Helvetica {}pt; color: {}", n(s.font_size), s.color)));
                    d.set("Q", Obj::Num(0.0));
                    let r = m.rect.unwrap_or([f64::NAN; 4]);
                    let rd = [r[0] - rect[0], rect[3] - r[3], rect[2] - r[2], r[1] - rect[1]].map(|v| cc_markup::js::max(0.0, v));
                    d.set("RD", Obj::nums(&rd));
                    if m.kind == "callout" && !m.pts().is_empty() {
                        let lead = list.iter().find_map(|i| match i {
                            Item::Path(p)
                                if p.stroke.as_ref().is_some_and(|x| !x.is_empty()) && p.fill.as_ref().is_none_or(|x| x.is_empty()) =>
                            {
                                Some(p)
                            }
                            _ => None,
                        });
                        let cl: Vec<[f64; 2]> = lead
                            .map(|p| {
                                p.d.iter()
                                    .filter_map(|c| match *c {
                                        Seg::M(x, y) | Seg::L(x, y) => Some([x, y]),
                                        _ => None,
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        let mut v = vec![m.pts().first().copied().unwrap_or([f64::NAN; 2])];
                        if cl.len() == 3 {
                            v.extend(cl.get(1..3).unwrap_or(&[]));
                        } else {
                            v.extend(cl.get(1..).unwrap_or(&[]));
                        }
                        d.set("IT", Obj::name("FreeTextCallout"));
                        d.set("CL", flat_pts(&v));
                        d.set("LE", Obj::name("ClosedArrow"));
                    }
                    if let Some(c) = ic {
                        d.set("IC", Obj::nums(&c));
                    }
                }
                "note" => {
                    d.set("Subtype", Obj::name("Text"));
                    d.set("Name", Obj::name("Comment"));
                    d.set("Open", Obj::Bool(false));
                    d.set("Contents", txt(&join(&[&m.text, &m.comment])));
                }
                "legend" => {
                    d.set("Subtype", Obj::name("Square"));
                    let mut parts = vec![if m.label.is_empty() { t0("legend.title") } else { m.label.clone() }];
                    for r in legend_rows.iter().flatten() {
                        parts.push(format!("{}: {}", r.label, cc_markup::js::ns(r.n)));
                    }
                    d.set("Contents", txt(&parts.join("\n")));
                }
                "space" => {
                    d.set("Subtype", Obj::name("Polygon"));
                    d.set("Vertices", flat_pts(m.pts()));
                    d.set(
                        "BS",
                        Obj::Dict(
                            Dict::new()
                                .with("Type", Obj::name("Border"))
                                .with("W", Obj::Num(cc_markup::js::max(0.0, wv)))
                                .with("S", Obj::name("D"))
                                .with("D", Obj::nums(&[6.0, 4.0])),
                        ),
                    );
                    d.set("Contents", txt(&join(&[&m.label, &m.comment])));
                }
                "image" => {
                    d.set("Subtype", Obj::name("Stamp"));
                    d.set("Name", Obj::name(&camel(if m.label.is_empty() { "Image" } else { &m.label })));
                    d.set("Contents", txt(&join(&[&m.label, &m.comment])));
                }
                "stamp" => {
                    d.set("Subtype", Obj::name("Stamp"));
                    let std = match cc_markup::js::lower(&m.label).as_str() {
                        "approved" => Some("Approved"),
                        "rejected" => Some("NotApproved"),
                        "void" => Some("Void"),
                        "reviewed" => Some("Reviewed"),
                        _ => None,
                    };
                    d.set("Name", Obj::name(&std.map_or_else(|| camel(&m.label), str::to_string)));
                }
                _ => continue,
            }
            let sub = d.get("Subtype").and_then(Obj::as_name).unwrap_or("").to_string();
            if sub != "Line" && sub != "FreeText" && d.has("LE") {
                d.delete("LE");
            }
            let flat = match &opts.flatten {
                Flatten::All => true,
                Flatten::Some(set) => set.contains(&m.id),
                Flatten::None => false,
            };
            if flat {
                // pdf-lib's page.node.newXObject + pushOperators: the page is normalised once (contents wrapped in q … Q,
                // resources copied onto the page), then a content stream of its own draws each flattened appearance
                let key = format!("MCFlat-{}", cc_markup::js::ns((rng.next_int() * 1e10).floor()));
                normalize_page(&mut doc, page_ref, &mut push_gs, &mut pop_gs, &mut normalized);
                set_xobject(&mut doc, page_ref, &key, ap_ref);
                if content_ref.is_none() {
                    let r = doc.register(Obj::Null);
                    content_ref = Some(r);
                    if let Some(pd) = doc.get_mut(page_ref).and_then(Obj::as_dict_mut) {
                        let mut arr = match pd.get("Contents") {
                            Some(Obj::Arr(a)) => a.clone(),
                            _ => Vec::new(),
                        };
                        arr.push(refo(r));
                        pd.set("Contents", Obj::Arr(arr));
                    }
                }
                page_ops.push("q".into());
                page_ops.push(format!("/{key} Do"));
                page_ops.push("Q".into());
                report.flattened = report.flattened.saturating_add(1);
                continue;
            }
            if let Some((_, r)) = ocg_of.iter().find(|(nm, _)| !m.layer.is_empty() && *nm == m.layer) {
                d.set("OC", refo(*r));
            }
            let annot_ref = doc.register(Obj::Dict(d));
            if !m.link.is_empty()
                && let Some((_, lp)) = opts.link_pages.iter().find(|(l, _)| *l == m.link)
                && lp.fract() == 0.0
                && *lp >= 0.0
                && let Some(&target) = pdf_pages.get(cc_markup::js::f2usize(*lp))
            {
                let link = doc.register(Obj::Dict(
                    Dict::new()
                        .with("Type", Obj::name("Annot"))
                        .with("Subtype", Obj::name("Link"))
                        .with("Rect", Obj::nums(&rect))
                        .with("Border", Obj::nums(&[0.0, 0.0, 0.0]))
                        .with("F", Obj::Num(4.0))
                        .with("P", refo(page_ref))
                        .with(
                            "A",
                            Obj::Dict(Dict::new().with("S", Obj::name("GoTo")).with("D", Obj::Arr(vec![refo(target), Obj::name("Fit")]))),
                        )
                        .with(PRIVATE_LINK_KEY, Obj::Bool(true)),
                ));
                keep.push(refo(link));
            }
            keep.push(refo(annot_ref));
            report.written = report.written.saturating_add(1);
            write_thread(&mut doc, &mut keep, &mut report, m, Some(annot_ref), Some(rect), page_ref, empty_ap, now);
        }
        if let Some(cr) = content_ref {
            let mut body = String::new();
            for op in &page_ops {
                body.push_str(op);
                body.push('\n');
            }
            doc.put(
                cr,
                Obj::Stream(Box::new(Stream {
                    dict: Dict::new().with("Filter", Obj::name("FlateDecode")),
                    data: deflate(body.as_bytes()),
                })),
            );
        }
        let media = media_box(&doc, page_ref);
        if let Some(pd) = doc.get_mut(page_ref).and_then(Obj::as_dict_mut) {
            pd.set("Annots", Obj::Arr(keep));
            if let Some(c) = &pg.cal {
                let bbox = media.map(|m| [m[0], m[1], m[0] + (m[2] - m[0]), m[1] + (m[3] - m[1])]).unwrap_or([0.0; 4]);
                pd.set(
                    "VP",
                    Obj::Arr(vec![Obj::Dict(
                        Dict::new()
                            .with("Type", Obj::name("Viewport"))
                            .with("BBox", Obj::nums(&bbox))
                            .with("Name", Obj::lit("MarkupCraft"))
                            .with("Measure", measure_obj(c)),
                    )]),
                );
                let cj =
                    pg.cal_json.clone().unwrap_or_else(|| serde_json::json!({ "mmPerPt": c.mm_per_pt, "unit": c.unit, "label": c.label }));
                pd.set(PRIVATE_CAL_KEY, txt(&cc_markup::json::stringify(&cj)));
            } else if pg.cal_cleared {
                pd.delete(PRIVATE_CAL_KEY);
            }
            match &pg.sheet_name {
                Some(v) if cc_markup::js::truthy(Some(v)) => pd.set(PRIVATE_SHEET_KEY, txt(&cc_markup::json::stringify(v))),
                _ => pd.delete(PRIVATE_SHEET_KEY),
            }
        }
    }
    if let Some(b) = &opts.bookmarks {
        write_outline(&mut doc, &pdf_pages, b);
    }
    if let Some(labels) = &opts.labels
        && labels.len() == pdf_pages.len()
    {
        let mut nums = Vec::new();
        for (i, l) in labels.iter().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            nums.push(Obj::Num(i as f64));
            let s = match l {
                Value::Null => String::new(),
                x => cc_markup::js::to_js_string(x),
            };
            let s = if s.is_empty() { i.saturating_add(1).to_string() } else { s };
            nums.push(Obj::Dict(Dict::new().with("P", txt(&s))));
        }
        if let Some(cr) = doc.root_ref()
            && let Some(c) = doc.get_mut(cr).and_then(Obj::as_dict_mut)
        {
            c.set("PageLabels", Obj::Dict(Dict::new().with("Nums", Obj::Arr(nums))));
        }
    }
    // the Info dictionary (made when missing)
    let info_ref = match doc.info.clone() {
        Some(Obj::Ref(a, b)) if matches!(doc.get((a, b)), Some(Obj::Dict(_))) => (a, b),
        _ => {
            let r = doc.register(Obj::Dict(Dict::new()));
            doc.info = Some(Obj::Ref(r.0, r.1));
            r
        }
    };
    if let Some(info) = doc.get_mut(info_ref).and_then(Obj::as_dict_mut) {
        info.set(PRIVATE_KEY, Obj::lit("1"));
        match &opts.columns {
            Some(Value::Array(a)) if !a.is_empty() => {
                info.set(PRIVATE_COLUMNS_KEY, txt(&cc_markup::json::stringify(&Value::Array(a.clone()))))
            }
            _ => info.delete(PRIVATE_COLUMNS_KEY),
        }
        if layer_list.is_empty() {
            info.delete(PRIVATE_LAYERS_KEY);
        } else {
            let v = Value::Array(layer_list.iter().map(|(nm, h, l)| serde_json::json!({ "name": nm, "hidden": h, "locked": l })).collect());
            info.set(PRIVATE_LAYERS_KEY, txt(&cc_markup::json::stringify(&v)));
        }
        info.set("ModDate", pdf_date_obj("", now));
    }
    crate::image::finish(&mut doc);
    Ok((doc.save(), report))
}

/// The page's media box (inherited), as numbers.
fn media_box(doc: &LibDoc, page: Ref) -> Option<[f64; 4]> {
    let d = doc.get(page).and_then(Obj::as_dict)?;
    let mb = doc.inherited(d, "MediaBox")?;
    let a = mb.as_arr()?;
    let v: Vec<f64> = a.iter().map(|x| doc.lookup(x).as_num().unwrap_or(f64::NAN)).collect();
    <[f64; 4]>::try_from(v).ok()
}

/// pdf-lib's PDFPageLeaf.normalize (only what changes the file): contents wrapped in q … Q streams, resources and
/// their Font / XObject / ExtGState dictionaries on the page.
fn normalize_page(doc: &mut LibDoc, page: Ref, push_gs: &mut Option<Ref>, pop_gs: &mut Option<Ref>, done: &mut Vec<Ref>) {
    if done.contains(&page) {
        return;
    }
    done.push(page);
    let Some(pd) = doc.get(page).and_then(Obj::as_dict).cloned() else { return };
    let mut pd = pd;
    let contents = pd.get("Contents").cloned();
    if let Some(Obj::Ref(a, b)) = &contents
        && matches!(doc.get((*a, *b)), Some(Obj::Stream(_)))
    {
        pd.set("Contents", Obj::Arr(vec![Obj::Ref(*a, *b)]));
    }
    if let Some(Obj::Arr(mut arr)) = pd.get("Contents").map(|c| doc.lookup(c).clone()) {
        let push = *push_gs.get_or_insert_with(|| {
            doc.register(Obj::Stream(Box::new(Stream {
                dict: Dict::new().with("Filter", Obj::name("FlateDecode")),
                data: deflate(b"q\n"),
            })))
        });
        let pop = *pop_gs.get_or_insert_with(|| {
            doc.register(Obj::Stream(Box::new(Stream {
                dict: Dict::new().with("Filter", Obj::name("FlateDecode")),
                data: deflate(b"Q\n"),
            })))
        });
        arr.insert(0, Obj::Ref(push.0, push.1));
        arr.push(Obj::Ref(pop.0, pop.1));
        pd.set("Contents", Obj::Arr(arr));
    }
    let res = doc.inherited(&pd, "Resources").and_then(|r| if let Obj::Dict(d) = r { Some(d.clone()) } else { None }).unwrap_or_default();
    let mut res = res;
    for k in ["Font", "XObject", "ExtGState"] {
        let v =
            res.get(k).map(|x| doc.lookup(x).clone()).and_then(|x| if let Obj::Dict(d) = x { Some(d) } else { None }).unwrap_or_default();
        res.set(k, Obj::Dict(v));
    }
    pd.set("Resources", Obj::Dict(res));
    if !pd.has("Annots") {
        pd.set("Annots", Obj::Arr(Vec::new()));
    }
    doc.put(page, Obj::Dict(pd));
}

fn set_xobject(doc: &mut LibDoc, page: Ref, key: &str, r: Ref) {
    if let Some(pd) = doc.get_mut(page).and_then(Obj::as_dict_mut) {
        let mut res = pd.get("Resources").and_then(|x| if let Obj::Dict(d) = x { Some(d.clone()) } else { None }).unwrap_or_default();
        let mut xo = res.get("XObject").and_then(|x| if let Obj::Dict(d) = x { Some(d.clone()) } else { None }).unwrap_or_default();
        xo.set(key, Obj::Ref(r.0, r.1));
        res.set("XObject", Obj::Dict(xo));
        pd.set("Resources", Obj::Dict(res));
    }
}

/// A markup's thread as reply annotations (`writeThread`).
#[allow(clippy::too_many_arguments)]
fn write_thread(
    doc: &mut LibDoc,
    keep: &mut Vec<Obj>,
    report: &mut Report,
    m: &Markup,
    parent: Option<Ref>,
    rect: Option<[f64; 4]>,
    page: Ref,
    empty_ap: Ref,
    now: f64,
) {
    let Some(parent) = parent else { return };
    let mut replies: Vec<Reply> = m.replies.clone();
    if !m.status.is_empty() && m.status != "Open" && !replies.iter().any(|r| r.kind == "status") {
        replies.push(Reply {
            id: format!("{}-state", m.id),
            author: m.author.clone(),
            date: m.modified.clone(),
            text: status_text(&m.status, &m.author),
            kind: "status".into(),
            status: m.status.clone(),
            photo: None,
        });
    }
    let r0 = rect.unwrap_or([0.0, 0.0, 16.0, 16.0]);
    let at = [r0[0], r0[3] - 16.0, r0[0] + 16.0, r0[3]];
    for r in &replies {
        let mut d = Dict::new()
            .with("Type", Obj::name("Annot"))
            .with("Subtype", Obj::name("Text"))
            .with("Rect", Obj::nums(&at))
            .with("F", Obj::Num(4.0))
            .with("P", Obj::Ref(page.0, page.1))
            .with("IRT", Obj::Ref(parent.0, parent.1))
            .with("RT", Obj::name("R"))
            .with("Name", Obj::name("Comment"))
            .with("Open", Obj::Bool(false))
            .with("T", txt(&r.author))
            .with("M", pdf_date_obj(&r.date, now))
            .with("CreationDate", pdf_date_obj(&r.date, now))
            .with("Contents", txt(&r.text))
            .with("NM", Obj::lit(&nm_clean(&r.id)))
            .with("AP", Obj::Dict(Dict::new().with("N", Obj::Ref(empty_ap.0, empty_ap.1))));
        if r.kind == "status" {
            d.set("StateModel", Obj::lit("Review"));
            d.set("State", Obj::lit(if r.status == "Open" { "None" } else { &r.status }));
        }
        let info = r.photo.as_deref().and_then(cc_markup::imagestamp::image_info);
        if let (Some(info), Some(photo)) = (info, &r.photo) {
            d.set(PRIVATE_PHOTO_KEY, Obj::lit(photo));
            let ext = if info.mime == "image/png" { "png" } else { "jpg" };
            let ef = doc.register(Obj::Stream(Box::new(Stream {
                dict: Dict::new()
                    .with("Type", Obj::name("EmbeddedFile"))
                    .with("Subtype", Obj::name(info.mime))
                    .with("Params", Obj::Dict(Dict::new().with("Size", Obj::Num(info.bytes.len() as f64))))
                    .with("Filter", Obj::name("FlateDecode")),
                data: deflate(&info.bytes),
            })));
            let base = if r.text.is_empty() { "photo".to_string() } else { r.text.clone() };
            let squashed = squash_bad(&base);
            let tail: String = {
                let v: Vec<char> = r.id.chars().collect();
                v.get(v.len().saturating_sub(6)..).unwrap_or(&[]).iter().collect()
            };
            let fname = txt(&format!("{}-{}.{}", cc_markup::js::slice16(cc_markup::js::trim(&squashed), 60), tail, ext));
            #[allow(clippy::cast_precision_loss)]
            let k = report.photos as f64;
            report.photos = report.photos.saturating_add(1);
            let fa = doc.register(Obj::Dict(
                Dict::new()
                    .with("Type", Obj::name("Annot"))
                    .with("Subtype", Obj::name("FileAttachment"))
                    .with("Rect", Obj::nums(&[at[0] + 18.0 + k * 2.0, at[1], at[2] + 18.0 + k * 2.0, at[3]]))
                    .with("F", Obj::Num(4.0))
                    .with("P", Obj::Ref(page.0, page.1))
                    .with("Name", Obj::name("Paperclip"))
                    .with("T", txt(&r.author))
                    .with("Contents", txt(&r.text))
                    .with("M", pdf_date_obj(&r.date, now))
                    .with(PRIVATE_PHOTO_KEY, Obj::Bool(true))
                    .with(
                        "FS",
                        Obj::Dict(
                            Dict::new()
                                .with("Type", Obj::name("Filespec"))
                                .with("F", fname.clone())
                                .with("UF", fname)
                                .with("EF", Obj::Dict(Dict::new().with("F", Obj::Ref(ef.0, ef.1)))),
                        ),
                    ),
            ));
            keep.push(Obj::Ref(fa.0, fa.1));
        }
        let rr = doc.register(Obj::Dict(d));
        keep.push(Obj::Ref(rr.0, rr.1));
        report.replies = report.replies.saturating_add(1);
    }
}

/// `.replace(/[\\/:*?"<>|\u0000-\u001f]+/g, ' ')`: each run of those characters becomes one space.
fn squash_bad(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_run = false;
    for c in s.chars() {
        let bad = matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || (c as u32) < 0x20;
        if bad {
            if !in_run {
                out.push(' ');
            }
            in_run = true;
        } else {
            out.push(c);
            in_run = false;
        }
    }
    out
}

/// `writeOutline(lib, pdfDoc, items)`.
fn write_outline(doc: &mut LibDoc, pages: &[Ref], items: &[Bookmark]) {
    let Some(cr) = doc.root_ref() else { return };
    if let Some(c) = doc.get_mut(cr).and_then(Obj::as_dict_mut) {
        c.delete("Outlines");
    }
    if items.is_empty() {
        return;
    }
    let root = doc.next_ref();
    fn build(doc: &mut LibDoc, pages: &[Ref], list: &[Bookmark], parent: Ref, depth: usize) -> (Vec<Ref>, usize) {
        let refs: Vec<Ref> = list.iter().map(|_| doc.next_ref()).collect();
        let mut total = 0usize;
        for (i, b) in list.iter().enumerate() {
            let kids = if depth < 16 {
                build(doc, pages, &b.children, refs.get(i).copied().unwrap_or(parent), depth.saturating_add(1))
            } else {
                (Vec::new(), 0)
            };
            let mut d = Dict::new().with("Title", Obj::hex_text(&b.title)).with("Parent", Obj::Ref(parent.0, parent.1));
            if i > 0
                && let Some(p) = refs.get(i.saturating_sub(1))
            {
                d.set("Prev", Obj::Ref(p.0, p.1));
            }
            if let Some(nx) = refs.get(i.saturating_add(1)) {
                d.set("Next", Obj::Ref(nx.0, nx.1));
            }
            if let (Some(f), Some(l)) = (kids.0.first(), kids.0.last()) {
                d.set("First", Obj::Ref(f.0, f.1));
                d.set("Last", Obj::Ref(l.0, l.1));
                #[allow(clippy::cast_precision_loss)]
                d.set("Count", Obj::Num(kids.1 as f64));
            }
            if b.page.fract() == 0.0
                && b.page >= 0.0
                && let Some(pg) = pages.get(cc_markup::js::f2usize(b.page))
            {
                d.set("Dest", Obj::Arr(vec![Obj::Ref(pg.0, pg.1), Obj::name("Fit")]));
            }
            if let Some(r) = refs.get(i) {
                doc.put(*r, Obj::Dict(d));
            }
            total = total.saturating_add(1).saturating_add(kids.1);
        }
        (refs, total)
    }
    let (top, total) = build(doc, pages, items, root, 0);
    let mut d = Dict::new().with("Type", Obj::name("Outlines"));
    if let (Some(f), Some(l)) = (top.first(), top.last()) {
        d.set("First", Obj::Ref(f.0, f.1));
        d.set("Last", Obj::Ref(l.0, l.1));
    }
    #[allow(clippy::cast_precision_loss)]
    d.set("Count", Obj::Num(total as f64));
    doc.put(root, Obj::Dict(d));
    if let Some(c) = doc.get_mut(cr).and_then(Obj::as_dict_mut) {
        c.set("Outlines", Obj::Ref(root.0, root.1));
    }
}
