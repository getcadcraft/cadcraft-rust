//! plot.js (+ the parts of sheets.js and agents.js it needs): a sheet → a vector PDF.
//!
//! - [`layout_sheet`] a layout at 1:1, its viewports showing the model; [`model_sheet`] the model on a paper at 1:n or
//!   fitted; [`display_list`] what to draw, in millimetres on the sheet; [`to_pdf`] / [`to_pdf_pages`] the PDF bytes;
//!   [`plot_pdf`] agents.js `plotPdf` (the MCP tool `drawing_plot`).
//! - Lines are vectors (arcs as Béziers), fills keep their holes (even-odd), text is Helvetica in WinAnsiEncoding
//!   (nothing embedded; other characters print as "?" and the report counts them). Pictures whose files the drawing
//!   keeps (`doc.pictures`) go in as image XObjects under the linework, clipped to their frames ([`crate::pdfimage`]);
//!   image data is always deflated (the JS writes no picture without CompressionStream).
//! - The content streams are deflated (zlib level 6, zlib-rs); the JS uses the browser's CompressionStream, so the
//!   compressed bytes differ. With `deflate: false` both write the same uncompressed file byte for byte (the parity
//!   harness `forks/tools/plotdiff.mjs` compares that form, and that the deflated one inflates to it).

use cc_cad::entity::{Entity, Geo};
use cc_cad::{Doc, js};
use cc_cad_geom::font::{CP1252, char_advance, subst};
use cc_cad_geom::geom::{
    Bounds, FONT_EM, M, Prim, clip_infinite, m_apply, m_mul, text_corners, text_layout, viewport_corners, viewport_matrix,
};
use cc_cad_geom::jsmath::JsMath;
use cc_cad_geom::pick::{empty_bounds, grow_pt, prim_bounds, valid_bounds};
use cc_cad_geom::walk::{Frozen, Opts, PicOut, WalkOpts, WalkOut, frozen_set, walk};
use std::sync::Arc;

use cc_cad::pictures::{PicRec, find_picture};

use crate::pdfimage::{PdfImage, pdf_image};
use cc_geom::P2;
use serde_json::{Value, json};

/// Pen widths the Plot dialog offers (mm).
pub const PEN_WIDTHS: [f64; 6] = [0.13, 0.18, 0.25, 0.35, 0.5, 0.7];
/// mm round a model plot (most printers cannot print to the edge)
const MARGIN: f64 = 5.0;

// ---------------------------------------------------------------------------------------------------------------
// sheets.js: papers, units, scales

/// A paper (landscape, w ≥ h) in its unit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaperDef {
    pub key: &'static str,
    pub label: &'static str,
    pub w: f64,
    pub h: f64,
    pub unit: &'static str,
}

/// sheets.js `PAPERS` (labels in English; Letter, Legal and ANSI B come from the catalog in the JS).
pub const PAPERS: [PaperDef; 13] = [
    PaperDef { key: "iso-a4", label: "ISO A4 (297 × 210 mm)", w: 297.0, h: 210.0, unit: "mm" },
    PaperDef { key: "iso-a3", label: "ISO A3 (420 × 297 mm)", w: 420.0, h: 297.0, unit: "mm" },
    PaperDef { key: "iso-a2", label: "ISO A2 (594 × 420 mm)", w: 594.0, h: 420.0, unit: "mm" },
    PaperDef { key: "iso-a1", label: "ISO A1 (841 × 594 mm)", w: 841.0, h: 594.0, unit: "mm" },
    PaperDef { key: "iso-a0", label: "ISO A0 (1189 × 841 mm)", w: 1189.0, h: 841.0, unit: "mm" },
    PaperDef { key: "letter", label: "sheets.paper.letter", w: 11.0, h: 8.5, unit: "in" },
    PaperDef { key: "legal", label: "sheets.paper.legal", w: 14.0, h: 8.5, unit: "in" },
    PaperDef { key: "ansi-b", label: "sheets.paper.ansi_b", w: 17.0, h: 11.0, unit: "in" },
    PaperDef { key: "ansi-c", label: "ANSI C (22 × 17 in)", w: 22.0, h: 17.0, unit: "in" },
    PaperDef { key: "ansi-d", label: "ANSI D (34 × 22 in)", w: 34.0, h: 22.0, unit: "in" },
    PaperDef { key: "arch-c", label: "ARCH C (24 × 18 in)", w: 24.0, h: 18.0, unit: "in" },
    PaperDef { key: "arch-d", label: "ARCH D (36 × 24 in)", w: 36.0, h: 24.0, unit: "in" },
    PaperDef { key: "arch-e", label: "ARCH E (48 × 36 in)", w: 48.0, h: 36.0, unit: "in" },
];

pub fn paper_by_key(k: &str) -> Option<&'static PaperDef> {
    PAPERS.iter().find(|p| p.key == k)
}
/// A paper's label: catalog words for the three that have them (`t` looks a key up).
pub fn paper_label(p: &PaperDef, t: &dyn Fn(&str) -> String) -> String {
    if p.label.starts_with("sheets.") { t(p.label) } else { p.label.to_string() }
}

/// sheets.js `UNIT_MM`: millimetres per unit.
pub fn unit_mm(u: &str) -> Option<f64> {
    Some(match u {
        "millimeters" | "mm" => 1.0,
        "centimeters" => 10.0,
        "meters" => 1000.0,
        "kilometers" => 1e6,
        "inches" | "in" => 25.4,
        "feet" => 304.8,
        "yards" => 914.4,
        "miles" => 1_609_344.0,
        "microinches" => 25.4e-6,
        "mils" => 0.0254,
        _ => return None,
    })
}
fn doc_units(doc: &Doc) -> String {
    doc.meta.get("units").and_then(Value::as_str).unwrap_or("").to_string()
}
pub fn is_imperial(doc: &Doc) -> bool {
    matches!(doc_units(doc).as_str(), "inches" | "feet" | "yards" | "miles" | "microinches" | "mils")
}
/// Model units in millimetres (unitless = the paper's unit).
pub fn model_mm(doc: &Doc, paper_unit: &str) -> f64 {
    unit_mm(&doc_units(doc)).filter(|v| *v != 0.0).or_else(|| unit_mm(paper_unit)).unwrap_or(1.0)
}
/// Paper units per model unit for a scale 1:n.
pub fn s_of(doc: &Doc, paper_unit: &str, n: f64) -> f64 {
    model_mm(doc, paper_unit) / unit_mm(paper_unit).unwrap_or(f64::NAN) / n
}
/// The n of 1:n for a viewport scale s.
pub fn n_of(doc: &Doc, paper_unit: &str, s: f64) -> f64 {
    model_mm(doc, paper_unit) / unit_mm(paper_unit).unwrap_or(f64::NAN) / s
}

const METRIC: [f64; 20] =
    [0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 25.0, 50.0, 100.0, 200.0, 250.0, 500.0, 1000.0, 1250.0, 2000.0, 2500.0, 5000.0, 10000.0];
const ARCH: [(&str, f64); 17] = [
    ("1/16\" = 1'-0\"", 192.0),
    ("3/32\" = 1'-0\"", 128.0),
    ("1/8\" = 1'-0\"", 96.0),
    ("3/16\" = 1'-0\"", 64.0),
    ("1/4\" = 1'-0\"", 48.0),
    ("3/8\" = 1'-0\"", 32.0),
    ("1/2\" = 1'-0\"", 24.0),
    ("3/4\" = 1'-0\"", 16.0),
    ("1\" = 1'-0\"", 12.0),
    ("1-1/2\" = 1'-0\"", 8.0),
    ("3\" = 1'-0\"", 4.0),
    ("1\" = 10'", 120.0),
    ("1\" = 20'", 240.0),
    ("1\" = 30'", 360.0),
    ("1\" = 40'", 480.0),
    ("1\" = 50'", 600.0),
    ("1\" = 100'", 1200.0),
];

/// `Number(n.toPrecision(d)).toString()`.
pub fn fmt_prec(n: f64, d: usize) -> String {
    if !n.is_finite() {
        return js::num_to_string(n);
    }
    let s = format!("{:.*e}", d.saturating_sub(1), n);
    js::num_to_string(js::str_to_number(&s))
}
fn ratio_label(n: f64) -> String {
    if n >= 1.0 { format!("1:{}", fmt_prec(n, 6)) } else { format!("{}:1", fmt_prec(1.0 / n, 6)) }
}

/// A standard scale: 1:n and how it reads.
#[derive(Clone, Debug, PartialEq)]
pub struct ScaleDef {
    pub n: f64,
    pub label: String,
}
/// sheets.js `scaleList(doc)`: architectural and engineering ones first for imperial drawings.
pub fn scale_list(doc: &Doc) -> Vec<ScaleDef> {
    let list: Vec<ScaleDef> = METRIC.iter().map(|&n| ScaleDef { n, label: ratio_label(n) }).collect();
    if !is_imperial(doc) {
        return list;
    }
    let mut arch: Vec<ScaleDef> = ARCH.iter().map(|(l, n)| ScaleDef { n: *n, label: (*l).to_string() }).collect();
    arch.sort_by(|a, b| a.n.partial_cmp(&b.n).unwrap_or(std::cmp::Ordering::Equal));
    arch.extend(list);
    arch
}
/// sheets.js `scaleLabel`.
pub fn scale_label(doc: &Doc, paper_unit: &str, s: f64) -> String {
    let n = n_of(doc, paper_unit, s);
    scale_list(doc).into_iter().find(|x| (x.n - n).abs() < 1e-6 * n).map_or_else(|| ratio_label(n), |x| x.label)
}

/// A layout's paper as stored: { key, unit, w, h, x0, y0 }.
#[derive(Clone, Debug, PartialEq)]
pub struct Paper {
    pub key: String,
    pub unit: String,
    pub w: f64,
    pub h: f64,
    pub x0: f64,
    pub y0: f64,
}
/// sheets.js `makePaper(key, portrait)` (ISO A3 for an unknown key).
pub fn make_paper(key: &str, portrait: bool) -> Paper {
    let p = paper_by_key(key).unwrap_or(&PAPERS[1]);
    Paper {
        key: p.key.into(),
        unit: p.unit.into(),
        w: if portrait { p.h } else { p.w },
        h: if portrait { p.w } else { p.h },
        x0: 0.0,
        y0: 0.0,
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Sheets

/// One view of a sheet: a clip (mm) or none, the matrix to mm, the entities, the layers frozen in it.
pub struct SheetView {
    pub clip: Option<Vec<P2>>,
    pub m: Option<M>,
    pub entities: Vec<Entity>,
    pub frozen: Option<Frozen>,
}
/// A model plot's scale: s (mm per model unit), n, how it reads, whether the area fits.
#[derive(Clone, Debug, PartialEq)]
pub struct PlotScale {
    pub s: f64,
    pub n: f64,
    pub label: String,
    pub fits: bool,
}
pub struct Sheet {
    pub w: f64,
    pub h: f64,
    pub views: Vec<SheetView>,
    pub name: String,
    pub scale: Option<PlotScale>,
}

fn paper_mm_of(p: &cc_cad::Obj) -> (f64, f64, f64, f64, f64) {
    let g = |k: &str| p.get(k).map_or(f64::NAN, js::to_number);
    let unit = p.get("unit").map(js::to_js_string).unwrap_or_default();
    let k = unit_mm(&unit).unwrap_or(f64::NAN);
    (k, g("w"), g("h"), g("x0"), g("y0"))
}

/// plot.js `layoutSheet(doc, layout)`: a layout at 1:1; each viewport that is on shows the model.
pub fn layout_sheet(doc: &Doc, layout: &cc_cad::Layout) -> Sheet {
    let (k, w, h, x0, y0) = paper_mm_of(&layout.paper);
    let to_mm: M = [k, 0.0, 0.0, k, -x0 * k, -y0 * k];
    let model: Vec<Entity> = doc.model().cloned().collect();
    let mut views = Vec::new();
    for e in doc.space_entities(layout.space) {
        let Geo::Viewport(v) = &e.geo else { continue };
        if e.get("on") == Some(&Value::Bool(false)) {
            continue;
        }
        let clip = viewport_corners(v.x, v.y, v.w, v.h).iter().map(|q| m_apply(&to_mm, *q)).collect();
        views.push(SheetView {
            clip: Some(clip),
            m: Some(m_mul(&to_mm, &viewport_matrix(v.x, v.y, v.cx, v.cy, v.s, v.tw))),
            entities: model.clone(),
            frozen: frozen_set(e),
        });
    }
    views.push(SheetView { clip: None, m: Some(to_mm), entities: doc.space_entities(layout.space).cloned().collect(), frozen: None });
    Sheet { w: w * k, h: h * k, views, name: layout.name.clone(), scale: None }
}

/// plot.js `modelSheet`: the model window `area` on a paper, at 1:n (`n`) or fitted (None), centred.
pub fn model_sheet(doc: &Doc, area: &Bounds, paper: &str, portrait: bool, n: Option<f64>) -> Sheet {
    let pp = make_paper(paper, portrait);
    let k = unit_mm(&pp.unit).unwrap_or(1.0);
    let (ww, hh) = (pp.w * k, pp.h * k);
    let aw = js::max(area.max_x - area.min_x, 1e-9);
    let ah = js::max(area.max_y - area.min_y, 1e-9);
    let s_fit = js::min((ww - 2.0 * MARGIN) / aw, (hh - 2.0 * MARGIN) / ah);
    let s = match n {
        Some(n) if n != 0.0 && !n.is_nan() => s_of(doc, "mm", n),
        _ => s_fit,
    };
    let cx = (area.min_x + area.max_x) / 2.0;
    let cy = (area.min_y + area.max_y) / 2.0;
    let m: M = [s, 0.0, 0.0, s, ww / 2.0 - cx * s, hh / 2.0 - cy * s];
    let clip = vec![P2::new(MARGIN, MARGIN), P2::new(ww - MARGIN, MARGIN), P2::new(ww - MARGIN, hh - MARGIN), P2::new(MARGIN, hh - MARGIN)];
    let nn = n_of(doc, "mm", s);
    let fixed = matches!(n, Some(n) if n != 0.0 && !n.is_nan());
    let label = if fixed {
        scale_label(doc, "mm", s)
    } else {
        format!("{} (fitted)", if nn >= 1.0 { format!("1:{}", fmt_prec(nn, 4)) } else { format!("{}:1", fmt_prec(1.0 / nn, 4)) })
    };
    let name = match doc.meta.get("name") {
        Some(v) if js::truthy(Some(v)) => js::to_js_string(v),
        _ => "Drawing1".into(),
    };
    Sheet {
        w: ww,
        h: hh,
        views: vec![SheetView { clip: Some(clip), m: Some(m), entities: doc.model().cloned().collect(), frozen: None }],
        name,
        scale: Some(PlotScale { s, n: nn, label, fits: s <= s_fit * (1.0 + 1e-9) }),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Display list

pub struct Fill {
    pub color: String,
    pub loops: Vec<Vec<f64>>,
    pub alpha: f64,
}
pub struct Stroke {
    pub color: String,
    pub lw: f64,
    pub alpha: f64,
    pub prims: Vec<Prim>,
}
pub struct TextItem {
    pub color: String,
    pub e: Entity,
    pub alpha: f64,
}
pub struct Mask {
    pub color: String,
    pub ring: Vec<f64>,
}
pub struct ListView {
    pub clip: Option<Vec<P2>>,
    /// pictures whose files the drawing has, in drawing order (sheet millimetres)
    pub pictures: Vec<PicOut>,
    pub fills: Vec<Fill>,
    pub masks: Vec<Mask>,
    pub strokes: Vec<Stroke>,
    pub points: Vec<(String, f64, f64)>,
    pub texts: Vec<TextItem>,
}
pub struct DisplayList {
    pub w: f64,
    pub h: f64,
    pub name: String,
    pub views: Vec<ListView>,
    /// `pictureFiles`: picture file → its stored record (first use order)
    pub picture_files: Vec<(String, Arc<PicRec>)>,
}

/// `mix(a, b, t)`: two '#rrggbb' colours mixed.
fn mix(a: &str, b: &str, t: f64) -> String {
    let p = |h: &str| -> [f64; 3] { [1usize, 3, 5].map(|i| h.get(i..i.saturating_add(2)).map_or(f64::NAN, parse_hex)) };
    let (x, y) = (p(a), p(b));
    let mut out = String::from("#");
    for (xa, ya) in x.iter().zip(y.iter()) {
        let v = js::round(xa * (1.0 - t) + ya * t);
        out.push_str(&hex2(v));
    }
    out
}
/// `parseInt(s, 16)` for two hex digits (NaN when the first is not one; stops at the first non-digit).
fn parse_hex(s: &str) -> f64 {
    let mut v = f64::NAN;
    for ch in s.chars() {
        let Some(d) = ch.to_digit(16) else { break };
        v = if v.is_nan() { f64::from(d) } else { v * 16.0 + f64::from(d) };
    }
    v
}
/// `v.toString(16).padStart(2, '0')` for a rounded colour channel.
fn hex2(v: f64) -> String {
    if !v.is_finite() {
        return "NaN".into();
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let i = v.clamp(0.0, 1.0e9) as u64;
    format!("{i:02x}")
}

fn alpha_of(o: Option<Opts>) -> f64 {
    match o.and_then(|o| o.alpha) {
        Some(a) if a.is_finite() => js::max(0.0, js::min(1.0, a)),
        _ => 1.0,
    }
}

struct Collect<'a> {
    doc: &'a Doc,
    pictures: Vec<PicOut>,
    files: &'a mut Vec<(String, Arc<PicRec>)>,
    mono: bool,
    area: Option<Bounds>,
    fills: Vec<Fill>,
    masks: Vec<Mask>,
    strokes: Vec<Stroke>,
    /// (colour, lineweight, alpha) of each stroke group: the JS keys on `String(lw)` and `String(a)`, which are equal
    /// exactly when the numbers are (−0 and 0 alike, every NaN alike): compared as normalised bits
    keys: Vec<(String, u64, u64)>,
    points: Vec<(String, f64, f64)>,
    texts: Vec<TextItem>,
    _p: std::marker::PhantomData<&'a ()>,
}
impl Collect<'_> {
    fn ink(&self, c: &str) -> String {
        if self.mono || c == "ink" { "#000000".into() } else { c.to_string() }
    }
    /// Push prims onto the stroke group of (colour, lineweight, alpha), made on first use (insertion order).
    fn stroke_push(&mut self, color: String, lw: f64, a: f64, prims: impl IntoIterator<Item = Prim>) {
        let bits = |x: f64| {
            if x.is_nan() {
                f64::NAN.to_bits()
            } else if x == 0.0 {
                0
            } else {
                x.to_bits()
            }
        };
        let (lwk, ak) = (bits(if lw >= 0.0 { lw } else { -3.0 }), bits(a));
        let i = match self.keys.iter().position(|k| k.1 == lwk && k.2 == ak && k.0 == color) {
            Some(i) => i,
            None => {
                self.keys.push((color.clone(), lwk, ak));
                self.strokes.push(Stroke { color, lw: if lw >= 0.0 { lw } else { -3.0 }, alpha: a, prims: Vec::new() });
                self.strokes.len().saturating_sub(1)
            }
        };
        if let Some(s) = self.strokes.get_mut(i) {
            s.prims.extend(prims);
        }
    }
}

impl WalkOut for Collect<'_> {
    fn stroke(&mut self, color: &str, prims: Vec<Prim>, lw: f64, o: Option<Opts>) {
        let c = self.ink(color);
        let a = alpha_of(o);
        self.stroke_push(c, lw, a, prims);
    }
    fn fill(&mut self, color: &str, loops: Vec<Vec<f64>>, solid: bool, o: Option<Opts>) {
        let a = alpha_of(o);
        if solid {
            let wipe = o.is_some_and(|o| o.wipe);
            self.fills.push(Fill {
                color: if wipe { "#ffffff".into() } else { self.ink(color) },
                loops,
                alpha: if wipe { 1.0 } else { a },
            });
        } else {
            let c = self.ink(color);
            self.fills.push(Fill { color: mix(&c, "#ffffff", 0.82), loops: loops.clone(), alpha: a });
            let mut l2 = Vec::new();
            for l in &loops {
                let mut i = 0usize;
                while let (Some(x1), Some(y1), Some(x2), Some(y2)) =
                    (l.get(i), l.get(i.saturating_add(1)), l.get(i.saturating_add(2)), l.get(i.saturating_add(3)))
                {
                    l2.push(Prim::l(*x1, *y1, *x2, *y2));
                    i = i.saturating_add(2);
                }
            }
            self.stroke_push(c, -3.0, a, l2);
        }
    }
    fn text(&mut self, color: &str, e: &Entity, o: Option<Opts>) {
        self.texts.push(TextItem { color: self.ink(color), e: e.clone(), alpha: alpha_of(o) });
    }
    fn point(&mut self, color: &str, x: f64, y: f64) {
        self.points.push((self.ink(color), x, y));
    }
    fn has_mask(&self) -> bool {
        true
    }
    fn mask(&mut self, color: &str, ring: Vec<f64>, wipe: bool) {
        let c = if wipe || self.mono { "#ffffff".to_string() } else { self.ink(color) };
        self.masks.push(Mask { color: c, ring });
    }
    /// 0.5 WL.13/14: only pictures whose file CadCraft has
    fn picture(&mut self, p: &PicOut) {
        if let Some(rec) = find_picture(&self.doc.pictures, &p.file) {
            self.pictures.push(p.clone());
            match self.files.iter_mut().find(|(f, _)| *f == p.file) {
                Some(slot) => slot.1 = rec.clone(),
                None => self.files.push((p.file.clone(), rec.clone())),
            }
        }
    }
    fn has_infinite(&self) -> bool {
        true
    }
    fn infinite(&mut self, color: &str, e: &Entity, lw: f64, o: Option<Opts>) {
        let (Geo::Xline(i) | Geo::Ray(i)) = &e.geo else { return };
        let Some(area) = self.area else { return };
        let ray = matches!(e.geo, Geo::Ray(_));
        if let Some((t0, t1)) = clip_infinite(P2::new(i.x, i.y), P2::new(i.dx, i.dy), &area, ray)
            && t1 > t0
        {
            let c = self.ink(color);
            let a = alpha_of(o);
            self.stroke_push(c, lw, a, [Prim::l(i.x + i.dx * t0, i.y + i.dy * t0, i.x + i.dx * t1, i.y + i.dy * t1)]);
        }
    }
}

/// plot.js `displayList(doc, sheet, { mono })`.
pub fn display_list(doc: &Doc, sheet: &Sheet, mono: bool) -> DisplayList {
    let mut files: Vec<(String, Arc<PicRec>)> = Vec::new();
    let views = sheet
        .views
        .iter()
        .map(|v| {
            let area = match &v.clip {
                Some(c) => {
                    let mut b = empty_bounds();
                    for q in c {
                        b = Bounds {
                            min_x: js::min(b.min_x, q.x),
                            min_y: js::min(b.min_y, q.y),
                            max_x: js::max(b.max_x, q.x),
                            max_y: js::max(b.max_y, q.y),
                        };
                    }
                    Some(b)
                }
                None if sheet.w > 0.0 && sheet.h > 0.0 => Some(Bounds { min_x: 0.0, min_y: 0.0, max_x: sheet.w, max_y: sheet.h }),
                None => None,
            };
            let mut out = Collect {
                doc,
                pictures: Vec::new(),
                files: &mut files,
                mono,
                area,
                fills: Vec::new(),
                masks: Vec::new(),
                strokes: Vec::new(),
                keys: Vec::new(),
                points: Vec::new(),
                texts: Vec::new(),
                _p: std::marker::PhantomData,
            };
            walk(doc, &v.entities, &mut out, &WalkOpts { m: v.m, plot: true, frozen: v.frozen.as_ref(), max_dashes: None });
            ListView {
                clip: v.clip.clone(),
                pictures: out.pictures,
                fills: out.fills,
                masks: out.masks,
                strokes: out.strokes,
                points: out.points,
                texts: out.texts,
            }
        })
        .collect();
    DisplayList { w: sheet.w, h: sheet.h, name: sheet.name.clone(), views, picture_files: files }
}

/// plot.js `extentsOf(doc)`: the model area a plot covers when asked for its extents.
pub fn extents_of(doc: &Doc) -> Option<Bounds> {
    let sheet = Sheet {
        w: 0.0,
        h: 0.0,
        views: vec![SheetView { clip: None, m: None, entities: doc.model().cloned().collect(), frozen: None }],
        name: String::new(),
        scale: None,
    };
    let list = display_list(doc, &sheet, true);
    let mut b = empty_bounds();
    let v = list.views.first()?;
    for s in &v.strokes {
        for q in &s.prims {
            prim_bounds(q, &mut b);
        }
    }
    for f in &v.fills {
        for l in &f.loops {
            let mut i = 0usize;
            while let (Some(x), Some(y)) = (l.get(i), l.get(i.saturating_add(1))) {
                grow_pt(&mut b, *x, *y);
                i = i.saturating_add(2);
            }
        }
    }
    for t in &v.texts {
        if let Geo::Text(tx) = &t.e.geo {
            for p in text_corners(tx) {
                grow_pt(&mut b, p.x, p.y);
            }
        }
    }
    for (_, x, y) in &v.points {
        grow_pt(&mut b, *x, *y);
    }
    valid_bounds(&b).then_some(b)
}

// ---------------------------------------------------------------------------------------------------------------
// PDF

/// A stroke's width on paper (mm): its lineweight, 0 as the thinnest line (0.05 mm), the default as the plot's pen.
fn pen_for(lw: f64, pen: f64) -> f64 {
    if lw >= 0.0 { js::max(lw, 0.05) } else { pen }
}

/// plot.js `num(v)`: 4 decimals, trailing zeros dropped.
pub fn num(v: f64) -> String {
    let mut s = String::new();
    push_num(&mut s, v);
    s
}

/// [`num`] appended to `out`, without the float formatter (its exact mode falls back to bignum arithmetic, which made
/// number text most of a plot's time). y = |v| × 10⁴ is off the exact product by at most half an ulp, so when its
/// fraction is clearly away from ½ the nearest whole number is the toFixed(4) digits; exactly halfway (|v| = m / 32, so
/// y is exact) toFixed rounds away from zero; anything closer to ½ than the error, or too big, takes the exact path.
pub fn push_num(out: &mut String, v: f64) {
    use std::fmt::Write as _;
    if !v.is_finite() || v.abs() < 5e-5 {
        out.push('0');
        return;
    }
    let y = v.abs() * 10000.0;
    let n = if y < 4.0e15 {
        let fl = y.floor();
        let e = y - fl;
        if (e - 0.5).abs() > y * 4.0 * f64::EPSILON {
            Some(if e > 0.5 { fl + 1.0 } else { fl })
        } else if e == 0.5 && (v.abs() * 32.0).fract() == 0.0 {
            Some(fl + 1.0)
        } else {
            None
        }
    } else {
        None
    };
    let Some(n) = n else {
        let start = out.len();
        out.push_str(&js::to_fixed(v, 4));
        if out.get(start..).is_some_and(|t| t.contains('.')) {
            let keep = out.trim_end_matches('0').trim_end_matches('.').len();
            out.truncate(keep);
        }
        return;
    };
    // a whole number in [1, 4e15]: exact in u64
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = n as u64;
    if v < 0.0 {
        out.push('-');
    }
    let (ip, fp) = (n / 10000, n % 10000);
    // writing to a String cannot fail
    let _ = write!(out, "{ip}");
    if fp > 0 {
        let mut f = fp;
        let mut w = 4usize;
        while f % 10 == 0 {
            f /= 10;
            w = w.saturating_sub(1);
        }
        let _ = write!(out, ".{f:0w$}");
    }
}
fn rgb(hex: &str) -> String {
    let h: String =
        if hex.chars().count() == 4 { hex.chars().skip(1).flat_map(|c| [c, c]).collect() } else { hex.chars().skip(1).take(6).collect() };
    [0usize, 2, 4].iter().map(|&i| num(h.get(i..i.saturating_add(2)).map_or(f64::NAN, parse_hex) / 255.0)).collect::<Vec<_>>().join(" ")
}

/// A string as an escaped PDF literal in WinAnsiEncoding, its width (1/1000 em) and how many characters had no glyph.
pub fn win_ansi(s: &str) -> (String, f64, usize) {
    let mut out = String::new();
    let mut width = 0.0;
    let mut missing = 0usize;
    for ch0 in s.chars() {
        let ch = subst(ch0);
        let mut code = u32::from(ch);
        if let Some((_, c)) = CP1252.iter().find(|(c, _)| *c == ch) {
            code = u32::from(*c);
        } else if code > 0xff || (0x7f..0xa0).contains(&code) || code < 0x20 {
            code = 0x3f;
            missing = missing.saturating_add(1);
            width += 556.0;
        }
        if code == 0x28 || code == 0x29 || code == 0x5c {
            out.push('\\');
            out.push(char::from_u32(code).unwrap_or('?'));
        } else if (0x20..0x7f).contains(&code) {
            out.push(char::from_u32(code).unwrap_or('?'));
        } else {
            out.push_str(&format!("\\{code:03o}"));
        }
        if code != 0x3f || ch == '?' {
            width += char_advance(ch);
        }
    }
    (out, width, missing)
}

/// Cubic Béziers for an arc from angle a to b (either direction), ≤ 90° each: [x1 y1 x2 y2 x3 y3].
pub fn arc_beziers(cx: f64, cy: f64, r: f64, a: f64, b: f64) -> Vec<[f64; 6]> {
    let sweep = b - a;
    let nf = js::max(1.0, ((sweep.abs() / (std::f64::consts::PI / 2.0)) - 1e-9).ceil());
    let n = if nf.is_finite() { nf.min(1.0e6) } else { 1.0 };
    let d = sweep / n;
    let k = 4.0 / 3.0 * (d / 4.0).jtan();
    let mut out = Vec::new();
    let mut i = 0.0;
    while i < n {
        let t0 = a + d * i;
        let t1 = t0 + d;
        let (c0, s0, c1, s1) = (t0.jcos(), t0.jsin(), t1.jcos(), t1.jsin());
        out.push([
            cx + r * (c0 - k * s0),
            cy + r * (s0 + k * c0),
            cx + r * (c1 + k * s1),
            cy + r * (s1 - k * c1),
            cx + r * c1,
            cy + r * s1,
        ]);
        i += 1.0;
    }
    out
}

struct Ops {
    /// the content stream, one operator per line
    text: String,
    alphas: Vec<f64>,
    sc: Option<String>,
    fc: Option<String>,
    ga: f64,
}
impl Ops {
    fn new() -> Ops {
        Ops { text: String::new(), alphas: Vec::new(), sc: None, fc: None, ga: 1.0 }
    }
    fn reset(&mut self) {
        self.sc = None;
        self.fc = None;
        self.ga = 1.0;
    }
    fn raw(&mut self, s: String) {
        self.text.push_str(&s);
        self.text.push('\n');
    }
    /// "x y op" without a temporary string per number.
    fn xy(&mut self, x: f64, y: f64, op: &str) {
        push_num(&mut self.text, x);
        self.text.push(' ');
        push_num(&mut self.text, y);
        self.text.push(' ');
        self.text.push_str(op);
        self.text.push('\n');
    }
    fn alpha(&mut self, a: f64) {
        let a = if a.is_finite() { js::max(0.0, js::min(1.0, js::round(a * 1000.0) / 1000.0)) } else { 1.0 };
        if a == self.ga {
            return;
        }
        self.ga = a;
        let k = match self.alphas.iter().position(|x| *x == a) {
            Some(k) => k,
            None => {
                self.alphas.push(a);
                self.alphas.len().saturating_sub(1)
            }
        };
        self.raw(format!("/GA{k} gs"));
    }
    fn stroke_color(&mut self, hex: &str) {
        if self.sc.as_deref() != Some(hex) {
            self.sc = Some(hex.into());
            let c = rgb(hex);
            self.raw(format!("{c} RG"));
        }
    }
    fn fill_color(&mut self, hex: &str) {
        if self.fc.as_deref() != Some(hex) {
            self.fc = Some(hex.into());
            let c = rgb(hex);
            self.raw(format!("{c} rg"));
        }
    }
    fn poly(&mut self, pts: &[P2]) {
        let Some(f) = pts.first() else { return };
        self.xy(f.x, f.y, "m");
        for p in pts.iter().skip(1) {
            self.xy(p.x, p.y, "l");
        }
        self.raw("h".into());
    }
    fn path(&mut self, ps: &[Prim]) {
        let (mut lx, mut ly) = (f64::INFINITY, f64::INFINITY);
        for q in ps {
            if q.is_l() {
                if (q.x1 - lx).abs() > 1e-9 || (q.y1 - ly).abs() > 1e-9 {
                    self.xy(q.x1, q.y1, "m");
                }
                self.xy(q.x2, q.y2, "l");
                lx = q.x2;
                ly = q.y2;
            } else {
                let rev = q.rev();
                let (from, to) = if rev { (q.a1, q.a0) } else { (q.a0, q.a1) };
                let sx = q.cx + q.r * from.jcos();
                let sy = q.cy + q.r * from.jsin();
                if (sx - lx).abs() > 1e-9 || (sy - ly).abs() > 1e-9 {
                    self.xy(sx, sy, "m");
                }
                for c in arc_beziers(q.cx, q.cy, q.r, from, to) {
                    for v in c {
                        push_num(&mut self.text, v);
                        self.text.push(' ');
                    }
                    self.text.push_str("c\n");
                }
                lx = q.cx + q.r * to.jcos();
                ly = q.cy + q.r * to.jsin();
            }
        }
    }
    /// One text entity (mm), Helvetica → how many characters printed as "?".
    fn text(&mut self, e: &Entity, color: &str) -> usize {
        let Geo::Text(t) = &e.geo else { return 0 };
        let lay = text_layout(t);
        let fs = t.h * FONT_EM;
        let wf = match t.wf {
            Some(w) if w != 0.0 && !w.is_nan() => w,
            _ => 1.0,
        };
        let rot = js::or(t.rot, 0.0);
        let (c, s) = (rot.jcos(), rot.jsin());
        let obl = e.num("obl").unwrap_or(0.0);
        let mut missing = 0usize;
        self.fill_color(color);
        for (i, line) in lay.lines.iter().enumerate() {
            if line.is_empty() {
                continue;
            }
            let (str_, width, miss) = win_ansi(line);
            missing = missing.saturating_add(miss);
            let w = width / 1000.0 * fs * wf;
            let lx = match t.ha.as_deref() {
                Some("center") => -w / 2.0,
                Some("right") => -w,
                _ => 0.0,
            };
            let ly = lay.baselines.get(i).copied().unwrap_or(f64::NAN);
            let x = t.x + lx * c - ly * s;
            let y = t.y + lx * s + ly * c;
            let k = if obl != 0.0 && !obl.is_nan() { fs * obl.jtan() } else { 0.0 };
            let m = [fs * wf * c, fs * wf * s, k * c - fs * s, k * s + fs * c, x, y].iter().map(|v| num(*v)).collect::<Vec<_>>().join(" ");
            self.raw(format!("BT /F1 1 Tf {m} Tm ({str_}) Tj ET"));
        }
        missing
    }
}

/// A zlib stream at level 6 (what the browser's CompressionStream('deflate') writes; the bytes differ, the inflated
/// data does not): zlib-rs, or miniz_oxide should zlib-rs refuse.
pub(crate) fn zlib(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; zlib_rs::compress_bound(data.len()).saturating_add(64)];
    let cfg = zlib_rs::DeflateConfig {
        level: 6,
        method: zlib_rs::Method::Deflated,
        window_bits: 15,
        mem_level: 8,
        strategy: zlib_rs::Strategy::Default,
    };
    let (got, rc) = zlib_rs::compress_slice(&mut out, data, cfg);
    if rc != zlib_rs::ReturnCode::Ok {
        return miniz_oxide::deflate::compress_to_vec_zlib(data, 6);
    }
    let n = got.len();
    out.truncate(n);
    out
}

/// Raw deflate at level 6 (CompressionStream('deflate-raw'); the bytes differ from the browser's, the data not).
pub fn deflate_raw(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; zlib_rs::compress_bound(data.len()).saturating_add(64)];
    let cfg = zlib_rs::DeflateConfig {
        level: 6,
        method: zlib_rs::Method::Deflated,
        window_bits: -15,
        mem_level: 8,
        strategy: zlib_rs::Strategy::Default,
    };
    let (got, rc) = zlib_rs::compress_slice(&mut out, data, cfg);
    if rc != zlib_rs::ReturnCode::Ok {
        return miniz_oxide::deflate::compress_to_vec(data, 6);
    }
    let n = got.len();
    out.truncate(n);
    out
}

/// Raw inflate, at most 1 GB out (None: damaged or bigger).
pub fn inflate_raw(data: &[u8]) -> Option<Vec<u8>> {
    miniz_oxide::inflate::decompress_to_vec_with_limit(data, 1 << 30).ok()
}

/// Every page's content stream deflated: on all cores natively (the pages are independent), one by one on the web.
fn deflate_pages(pages: &mut [Page]) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        pages.par_iter_mut().for_each(|p| {
            p.content = zlib(&p.content);
            p.flate = true;
        });
    }
    #[cfg(target_arch = "wasm32")]
    for p in pages.iter_mut() {
        p.content = zlib(&p.content);
        p.flate = true;
    }
}

/// One page of a PDF being assembled.
struct Page {
    w: f64,
    h: f64,
    content: Vec<u8>,
    flate: bool,
    missing: usize,
    texts: usize,
    alphas: Vec<f64>,
    name: String,
    /// the images this page draws (indices into the file's images, first use order)
    xobjects: Vec<usize>,
    pictures: usize,
    shrunk: usize,
    kept: usize,
}

/// An image of the file: what it was made from (a picture used on several pages is stored once).
struct FileImage {
    img: PdfImage,
    rec: Arc<PicRec>,
    max_dpi: f64,
}

fn page_of(list: &DisplayList, pen: f64, deflate: bool, images: &mut Vec<FileImage>, max_dpi: f64) -> Page {
    let mut out = Ops::new();
    let mut missing = 0usize;
    let mut n_texts = 0usize;
    let (mut n_pics, mut shrunk, mut kept) = (0usize, 0usize, 0usize);
    let mut xobjects: Vec<usize> = Vec::new();
    let k = js::to_fixed(72.0 / 25.4, 9);
    out.raw(format!("{k} 0 0 {k} 0 0 cm"));
    for v in &list.views {
        out.raw("q".into());
        out.reset();
        if let Some(c) = &v.clip {
            out.poly(c);
            out.raw("W n".into());
        }
        for p in &v.pictures {
            let (mm_w, mm_h) = (js::hypot(p.ux, p.uy) * p.w, js::hypot(p.vx, p.vy) * p.h);
            let Some(rec) = list.picture_files.iter().find(|(f, _)| *f == p.file).map(|(_, r)| r.clone()) else { continue };
            let k = match images.iter().position(|im| Arc::ptr_eq(&im.rec, &rec) && im.max_dpi == max_dpi) {
                Some(k) => k,
                None => {
                    let Some(img) = pdf_image(&rec, max_dpi, mm_w, mm_h) else { continue };
                    if img.shrunk {
                        shrunk = shrunk.saturating_add(1);
                    }
                    if img.kept {
                        kept = kept.saturating_add(1);
                    }
                    images.push(FileImage { img, rec, max_dpi });
                    images.len().saturating_sub(1)
                }
            };
            if !xobjects.contains(&k) {
                xobjects.push(k);
            }
            out.raw("q".into());
            if p.clip.len() >= 6 {
                let (x0, y0) = (p.clip.first().copied().unwrap_or(0.0), p.clip.get(1).copied().unwrap_or(0.0));
                out.raw(format!("{} {} m", num(x0), num(y0)));
                let mut i = 2usize;
                while let (Some(x), Some(y)) = (p.clip.get(i), p.clip.get(i.saturating_add(1))) {
                    out.raw(format!("{} {} l", num(*x), num(*y)));
                    i = i.saturating_add(2);
                }
                out.raw("h W n".into());
            }
            out.alpha(p.alpha);
            let m = [p.ux * p.w, p.uy * p.w, p.vx * p.h, p.vy * p.h, p.x, p.y]
                .iter()
                .map(|x| if x.is_finite() { js::to_fixed(*x, 6) } else { "0".into() })
                .collect::<Vec<_>>()
                .join(" ");
            out.raw(format!("{m} cm /Im{k} Do"));
            out.raw("Q".into());
            out.alpha(1.0);
            n_pics = n_pics.saturating_add(1);
        }
        out.raw(format!("1 J 1 j {} w", num(pen)));
        for f in &v.fills {
            out.alpha(f.alpha);
            out.fill_color(&f.color);
            for l in &f.loops {
                if l.len() < 6 {
                    continue;
                }
                let (Some(x0), Some(y0)) = (l.first(), l.get(1)) else { continue };
                out.raw(format!("{} {} m", num(*x0), num(*y0)));
                let mut i = 2usize;
                while let (Some(x), Some(y)) = (l.get(i), l.get(i.saturating_add(1))) {
                    out.raw(format!("{} {} l", num(*x), num(*y)));
                    i = i.saturating_add(2);
                }
                out.raw("h".into());
            }
            out.raw("f*".into());
        }
        let mut width = pen;
        for s in &v.strokes {
            let w = pen_for(s.lw, pen);
            if w != width {
                out.raw(format!("{} w", num(w)));
                width = w;
            }
            out.alpha(s.alpha);
            out.stroke_color(&s.color);
            out.path(&s.prims);
            out.raw("S".into());
        }
        if width != pen {
            out.raw(format!("{} w", num(pen)));
        }
        out.alpha(1.0);
        for mk in &v.masks {
            let l = &mk.ring;
            if l.len() < 6 {
                continue;
            }
            out.fill_color(&mk.color);
            let (Some(x0), Some(y0)) = (l.first(), l.get(1)) else { continue };
            out.raw(format!("{} {} m", num(*x0), num(*y0)));
            let mut i = 2usize;
            while let (Some(x), Some(y)) = (l.get(i), l.get(i.saturating_add(1))) {
                out.raw(format!("{} {} l", num(*x), num(*y)));
                i = i.saturating_add(2);
            }
            out.raw("h f".into());
        }
        for (color, x, y) in &v.points {
            out.stroke_color(color);
            out.raw(format!("{} {} m {} {} l S", num(*x), num(*y), num(*x), num(*y)));
        }
        for t in &v.texts {
            out.alpha(t.alpha);
            missing = missing.saturating_add(out.text(&t.e, &t.color));
            n_texts = n_texts.saturating_add(1);
        }
        out.raw("Q".into());
    }
    let text = std::mem::take(&mut out.text);
    let content = text.into_bytes();
    let (content, flate) = if deflate { (zlib(&content), true) } else { (content, false) };
    // (to_pdf_pages asks for plain content and deflates its pages together, on several threads)
    Page {
        w: list.w,
        h: list.h,
        content,
        flate,
        missing,
        texts: n_texts,
        alphas: out.alphas,
        name: String::new(),
        xobjects,
        pictures: n_pics,
        shrunk,
        kept,
    }
}

/// `D:YYYYMMDDHHMMSSZ` for a UTC time in ms (the JS formats `new Date()` in UTC the same way).
fn pdf_date(ms: f64) -> String {
    let secs = if ms.is_finite() { (ms / 1000.0).floor().clamp(-1.0e13, 1.0e13) } else { 0.0 };
    let days = (secs / 86_400.0).floor();
    let rem = secs - days * 86_400.0;
    // civil from days (H. Hinnant), in f64: every value is a whole number far below 2^53
    let z = days + 719_468.0;
    let era = (z / 146_097.0).floor();
    let doe = z - era * 146_097.0;
    let yoe = ((doe - (doe / 1460.0).floor() + (doe / 36_524.0).floor() - (doe / 146_096.0).floor()) / 365.0).floor();
    let doy = doe - (365.0 * yoe + (yoe / 4.0).floor() - (yoe / 100.0).floor());
    let mp = ((5.0 * doy + 2.0) / 153.0).floor();
    let d = doy - ((153.0 * mp + 2.0) / 5.0).floor() + 1.0;
    let m = if mp < 10.0 { mp + 3.0 } else { mp - 9.0 };
    let y = yoe + era * 400.0 + if m <= 2.0 { 1.0 } else { 0.0 };
    let hh = (rem / 3600.0).floor();
    let mi = ((rem - hh * 3600.0) / 60.0).floor();
    let ss = rem - hh * 3600.0 - mi * 60.0;
    let i = |v: f64| js::num_to_string(v);
    let p2 = |v: f64| format!("{:0>2}", i(v));
    format!("D:{}{}{}{}{}{}Z", i(y), p2(m), p2(d), p2(hh), p2(mi), p2(ss))
}

fn assemble(pages: &[Page], title: &str, date_ms: f64, images: &[FileImage]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut offsets: Vec<usize> = Vec::new();
    let obj = |out: &mut Vec<u8>, offsets: &mut Vec<usize>, n: usize, body: &[&[u8]]| {
        if offsets.len() <= n {
            offsets.resize(n.saturating_add(1), 0);
        }
        if let Some(o) = offsets.get_mut(n) {
            *o = out.len();
        }
        out.extend_from_slice(format!("{n} 0 obj\n").as_bytes());
        for b in body {
            out.extend_from_slice(b);
        }
        out.extend_from_slice(b"\nendobj\n");
    };
    let pt = |mm: f64| num(mm * 72.0 / 25.4);
    let pdf_text = |s: &str| format!("({})", win_ansi(s).0);
    out.extend_from_slice(b"%PDF-1.4\n");
    out.extend_from_slice(&[0x25, 0xe2, 0xe3, 0xcf, 0xd3, 0x0a]);
    let n = pages.len();
    let outlines = n > 1;
    let first = 6usize;
    let page_no = |i: usize| first.saturating_add(i.saturating_mul(2));
    let item_no = |i: usize| first.saturating_add(n.saturating_mul(2)).saturating_add(i);
    let img_base = if outlines { item_no(n) } else { page_no(n) };
    // pictures (0.5 WL.13): after everything else, each image followed by its soft mask when it has one
    let mut img_no: Vec<usize> = Vec::with_capacity(images.len());
    {
        let mut k = img_base;
        for im in images {
            img_no.push(k);
            k = k.saturating_add(if im.img.smask.is_some() { 2 } else { 1 });
        }
    }
    let cat = format!("<< /Type /Catalog /Pages 2 0 R{} >>", if outlines { " /Outlines 5 0 R /PageMode /UseOutlines" } else { "" });
    obj(&mut out, &mut offsets, 1, &[cat.as_bytes()]);
    let kids = (0..n).map(|i| format!("{} 0 R", page_no(i))).collect::<Vec<_>>().join(" ");
    obj(&mut out, &mut offsets, 2, &[format!("<< /Type /Pages /Kids [{kids}] /Count {n} >>").as_bytes()]);
    obj(&mut out, &mut offsets, 3, &[b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"]);
    let info = format!("<< /Title {} /Producer (CadCraft) /Creator (CadCraft) /CreationDate ({}) >>", pdf_text(title), pdf_date(date_ms));
    obj(&mut out, &mut offsets, 4, &[info.as_bytes()]);
    let ol = if outlines {
        format!("<< /Type /Outlines /First {} 0 R /Last {} 0 R /Count {n} >>", item_no(0), item_no(n.saturating_sub(1)))
    } else {
        "<< >>".into()
    };
    obj(&mut out, &mut offsets, 5, &[ol.as_bytes()]);
    for (i, p) in pages.iter().enumerate() {
        let gs = if p.alphas.is_empty() {
            String::new()
        } else {
            let items = p
                .alphas
                .iter()
                .enumerate()
                .map(|(k, a)| format!("/GA{k} << /Type /ExtGState /CA {} /ca {} >>", num(*a), num(*a)))
                .collect::<Vec<_>>()
                .join(" ");
            format!(" /ExtGState << {items} >>")
        };
        let xo = if p.xobjects.is_empty() {
            String::new()
        } else {
            let items =
                p.xobjects.iter().map(|k| format!("/Im{k} {} 0 R", img_no.get(*k).copied().unwrap_or(0))).collect::<Vec<_>>().join(" ");
            format!(" /XObject << {items} >>")
        };
        let pg = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 3 0 R >>{gs}{xo} >> /Contents {} 0 R >>",
            pt(p.w),
            pt(p.h),
            page_no(i).saturating_add(1)
        );
        obj(&mut out, &mut offsets, page_no(i), &[pg.as_bytes()]);
        let head = format!("<< /Length {}{} >>\nstream\n", p.content.len(), if p.flate { " /Filter /FlateDecode" } else { "" });
        obj(&mut out, &mut offsets, page_no(i).saturating_add(1), &[head.as_bytes(), &p.content, b"\nendstream"]);
    }
    if outlines {
        for (i, p) in pages.iter().enumerate() {
            let prev = if i > 0 { format!(" /Prev {} 0 R", item_no(i.saturating_sub(1))) } else { String::new() };
            let next = if i.saturating_add(1) < n { format!(" /Next {} 0 R", item_no(i.saturating_add(1))) } else { String::new() };
            let body = format!("<< /Title {} /Parent 5 0 R{prev}{next} /Dest [{} 0 R /Fit] >>", pdf_text(&p.name), page_no(i));
            obj(&mut out, &mut offsets, item_no(i), &[body.as_bytes()]);
        }
    }
    for (i, im) in images.iter().enumerate() {
        let no = img_no.get(i).copied().unwrap_or(0);
        let g = &im.img;
        let head = format!(
            "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /{} /BitsPerComponent 8 /Filter /{}{} /Length {} >>\nstream\n",
            g.w,
            g.h,
            if g.colors == 1 { "DeviceGray" } else { "DeviceRGB" },
            g.filter,
            if g.smask.is_some() { format!(" /SMask {} 0 R", no.saturating_add(1)) } else { String::new() },
            g.data.len()
        );
        obj(&mut out, &mut offsets, no, &[head.as_bytes(), &g.data, b"\nendstream"]);
        if let Some(sm) = &g.smask {
            let head = format!(
                "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n",
                g.w,
                g.h,
                sm.len()
            );
            obj(&mut out, &mut offsets, no.saturating_add(1), &[head.as_bytes(), sm, b"\nendstream"]);
        }
    }
    let total = match (img_no.last(), images.last()) {
        (Some(no), Some(im)) => no.saturating_add(if im.img.smask.is_some() { 2 } else { 1 }),
        _ => img_base,
    };
    let xref = out.len();
    let mut x = format!("xref\n0 {total}\n0000000000 65535 f \n");
    for k in 1..total {
        x.push_str(&format!("{:010} 00000 n \n", offsets.get(k).copied().unwrap_or(0)));
    }
    out.extend_from_slice(x.as_bytes());
    out.extend_from_slice(format!("trailer\n<< /Size {total} /Root 1 0 R /Info 4 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
    out
}

/// What a PDF holds: bytes and its report { missing, texts, size, compressed, pictures, shrunk, kept }.
pub struct Pdf {
    pub bytes: Vec<u8>,
    pub missing: usize,
    pub texts: usize,
    pub compressed: bool,
    /// pictures drawn, PNGs shrunk to the dpi cap, JPEGs kept denser than it
    pub pictures: usize,
    pub shrunk: usize,
    pub kept: usize,
}

/// plot.js `toPdf(list, { pen, title })`; `date_ms` is the CreationDate; `deflate` false writes plain streams.
pub fn to_pdf(list: &DisplayList, pen: f64, title: &str, date_ms: f64, deflate: bool) -> Pdf {
    to_pdf_dpi(list, pen, title, date_ms, deflate, 0.0)
}

/// plot.js `toPdf(list, { pen, title, maxDpi })`: pictures capped at `max_dpi` on paper (0 = their own resolution).
pub fn to_pdf_dpi(list: &DisplayList, pen: f64, title: &str, date_ms: f64, deflate: bool, max_dpi: f64) -> Pdf {
    let mut images = Vec::new();
    let page = page_of(list, pen, deflate, &mut images, max_dpi);
    let (missing, texts, compressed, pictures, shrunk, kept) =
        (page.missing, page.texts, page.flate, page.pictures, page.shrunk, page.kept);
    let bytes = assemble(&[page], title, date_ms, &images);
    Pdf { bytes, missing, texts, compressed, pictures, shrunk, kept }
}

/// plot.js `toPdfPages(lists, { pen, title, names })`: one page per sheet, with a bookmark each.
pub fn to_pdf_pages(lists: &[DisplayList], pen: f64, title: &str, names: &[String], date_ms: f64, deflate: bool) -> Pdf {
    let mut images = Vec::new();
    let mut pages: Vec<Page> = lists.iter().map(|l| page_of(l, pen, false, &mut images, 0.0)).collect();
    if deflate {
        deflate_pages(&mut pages);
    }
    for (i, p) in pages.iter_mut().enumerate() {
        p.name = names
            .get(i)
            .filter(|s| !s.is_empty())
            .cloned()
            .or_else(|| lists.get(i).map(|l| l.name.clone()).filter(|s| !s.is_empty()))
            .unwrap_or_else(|| format!("Page {}", i.saturating_add(1)));
    }
    let missing = pages.iter().map(|p| p.missing).fold(0usize, usize::saturating_add);
    let texts = pages.iter().map(|p| p.texts).fold(0usize, usize::saturating_add);
    let compressed = deflate;
    let sum = |f: fn(&Page) -> usize| pages.iter().map(f).fold(0usize, usize::saturating_add);
    let (pictures, shrunk, kept) = (sum(|p| p.pictures), sum(|p| p.shrunk), sum(|p| p.kept));
    let bytes = assemble(&pages, title, date_ms, &images);
    Pdf { bytes, missing, texts, compressed, pictures, shrunk, kept }
}

// ---------------------------------------------------------------------------------------------------------------
// agents.js plotPdf / parseScale

/// agents.js `parseScale(doc, s)`: None = fit; Some(n) for 1:n; Err for an unknown scale.
pub fn parse_scale(doc: &Doc, s: Option<&str>) -> Result<Option<f64>, String> {
    let Some(s) = s else { return Ok(None) };
    if s.is_empty() || s.get(..3).is_some_and(|p| p.eq_ignore_ascii_case("fit")) {
        return Ok(None);
    }
    let st = js::trim(s);
    if let Some((a, b)) = st.split_once(':') {
        let (a, b) = (js::trim(a), js::trim(b));
        let ok = |x: &str| {
            !x.is_empty()
                && x.split_once('.').map_or(x.chars().all(|c| c.is_ascii_digit()), |(i, f)| {
                    !i.is_empty() && i.chars().all(|c| c.is_ascii_digit()) && !f.is_empty() && f.chars().all(|c| c.is_ascii_digit())
                })
        };
        if ok(a) && ok(b) {
            let (x, y) = (js::str_to_number(a), js::str_to_number(b));
            if x > 0.0 && y > 0.0 {
                return Ok(Some(y / x));
            }
        }
    }
    let norm = |x: &str| -> String {
        x.chars()
            .filter(|c| !js::is_ws(*c))
            .map(|c| match c {
                '’' | '′' => '\'',
                '”' | '″' => '"',
                c => c,
            })
            .collect::<String>()
            .to_lowercase()
    };
    if let Some(hit) = scale_list(doc).into_iter().find(|o| norm(&o.label) == norm(st)) {
        return Ok(Some(hit.n));
    }
    Err(format!("Unknown scale \"{st}\". Use 1:n, an architectural scale such as 1/4\" = 1'-0\", or fit."))
}

/// agents.js `plotPdf` options.
#[derive(Clone, Debug)]
pub struct PlotOpts {
    pub layout: Option<String>,
    pub window: Option<Bounds>,
    pub paper: String,
    pub portrait: bool,
    pub scale: Option<String>,
    pub mono: bool,
    pub pen: f64,
    pub date_ms: f64,
    pub deflate: bool,
}
impl Default for PlotOpts {
    fn default() -> Self {
        PlotOpts {
            layout: None,
            window: None,
            paper: "iso-a3".into(),
            portrait: false,
            scale: None,
            mono: true,
            pen: 0.25,
            date_ms: 0.0,
            deflate: true,
        }
    }
}

/// What `plot_pdf` made: the PDF, the sheet (mm), its scale label, whether it fits, unprintable characters.
pub struct Plotted {
    pub pdf: Pdf,
    pub w: f64,
    pub h: f64,
    pub scale: Option<String>,
    pub fits: bool,
}
impl Plotted {
    /// The tool result's sheet part: `{ w, h, scale, fits }` (w / h to 9 decimals, agents.js `num`).
    pub fn sheet_value(&self) -> Value {
        let n = |v: f64| if v.is_finite() { js::round(v * 1e9) / 1e9 } else { v };
        json!({ "w": cc_cad::num(n(self.w)), "h": cc_cad::num(n(self.h)), "scale": self.scale, "fits": self.fits })
    }
}

/// agents.js `plotPdf(doc, opts)`: a sheet (layout) or the model (extents or a window) → a PDF.
pub fn plot_pdf(doc: &Doc, o: &PlotOpts) -> Result<Plotted, String> {
    let sheet = if let Some(layout) = &o.layout {
        let Some(lay) = doc.layouts.iter().find(|l| l.name.to_lowercase() == layout.to_lowercase()) else {
            let names = doc.layouts.iter().map(|l| l.name.clone()).collect::<Vec<_>>().join(", ");
            return Err(format!("There is no sheet \"{layout}\". Sheets: {}.", if names.is_empty() { "none".into() } else { names }));
        };
        layout_sheet(doc, lay)
    } else {
        if paper_by_key(&o.paper).is_none() {
            return Err(format!("Papers: {}.", PAPERS.iter().map(|p| p.key).collect::<Vec<_>>().join(", ")));
        }
        let bx = match &o.window {
            Some(w) => Some(Bounds {
                min_x: js::min(w.min_x, w.max_x),
                min_y: js::min(w.min_y, w.max_y),
                max_x: js::max(w.min_x, w.max_x),
                max_y: js::max(w.min_y, w.max_y),
            }),
            None => extents_of(doc),
        };
        let Some(bx) = bx else { return Err("There is nothing to plot.".into()) };
        let n = parse_scale(doc, o.scale.as_deref())?;
        model_sheet(doc, &bx, &o.paper, o.portrait, n)
    };
    let list = display_list(doc, &sheet, o.mono);
    let name = match doc.meta.get("name") {
        Some(v) if js::truthy(Some(v)) => js::to_js_string(v),
        _ => "Drawing1".into(),
    };
    let title = match &o.layout {
        Some(l) => format!("{name} – {l}"),
        None => name,
    };
    let pen = if o.pen != 0.0 && !o.pen.is_nan() { o.pen } else { 0.25 };
    let pdf = to_pdf(&list, pen, &title, o.date_ms, o.deflate);
    let (scale, fits) = match &sheet.scale {
        Some(s) => (Some(s.label.clone()), s.fits),
        None => (None, true),
    };
    Ok(Plotted { pdf, w: list.w, h: list.h, scale, fits })
}
