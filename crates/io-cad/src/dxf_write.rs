//! dxf.js `writeDxf(doc)`: a `Doc` → DXF R12 (AC1009) ASCII, byte-identical to the JS writer, and its report
//! { notes, counts, extents }.
//!
//! Not written (needs ports this crate does not have yet): CadCraft's constraints block (gcons.js `forFile`: the Doc
//! keeps a file's parametric payload but does not resolve it) and the 3D model's block and 3DFACEs (model3d.js
//! `forFile3d`: the m3d kernel). A drawing with either is written without them.

use std::collections::{HashMap, HashSet};

use cc_cad::entity::{Entity, Geo, Insert};
use cc_cad::js;
use cc_cad::{Block, Doc};
use cc_cad_geom::geom::{
    TAU, clip_infinite, deg, ellipse_points, insert_matrix, m_apply, m_mul, m_rotate, m_translate, norm_angle, text_layout,
};
use cc_cad_geom::jsmath::JsMath;
use cc_cad_geom::pick::{empty_bounds, entity_bounds, valid_bounds};
use cc_geom::P2;
use cc_io_dwg::value::{Obj, V};

use crate::i18n::{self, Catalog};
use crate::jsv::{self, JsErr, Ov, R};
use crate::props;
use crate::small::round_precision;
use crate::text::{encode_text, len16, one_line, slice16};

/// `f(n)`: a number as R12 writes it (15 significant digits, "0.0" for |n| < 1e-12).
pub fn ff(n: f64) -> String {
    if n.abs() < 1e-12 {
        return "0.0".into();
    }
    if n.fract() == 0.0 && n.abs() < 1e15 {
        // an integer of at most 15 digits: toPrecision(15) keeps it as it is
        return js::num_to_string(n);
    }
    js::num_to_string(round_precision(n, 15))
}
/// `f(v)` for a value that may not be a number (the JS throws where it would call toPrecision on a non-number).
fn fv(v: Ov) -> R<String> {
    match v {
        Some(V::Num(n)) => Ok(ff(*n)),
        None => {
            if f64::NAN.abs() < 1e-12 {
                Ok("0.0".into())
            } else {
                Err(JsErr("Cannot read properties of undefined (reading 'toPrecision')".into()))
            }
        }
        Some(other) => {
            let n = jsv::to_num(Some(other)).abs();
            if n < 1e-12 { Ok("0.0".into()) } else { Err(JsErr("n.toPrecision is not a function".into())) }
        }
    }
}
fn zf(v: Ov) -> String {
    match jsv::fin(v) {
        Some(x) => ff(x),
        None => "0.0".into(),
    }
}
/// `String(v)` for a group value.
fn sv(v: &V) -> String {
    jsv::to_str(Some(v))
}

const TH_TYPES: [&str; 6] = ["line", "circle", "arc", "poly", "point", "text"];

/// Where an entity lives (dxf.js keys dimension block names by object identity).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Src {
    Model(usize),
    Layout(usize, usize),
    Block(String, usize),
}

enum DimBlock {
    Own(String),
    Made(Vec<Entity>),
}

/// A block's extra contents writer (`extra?.()`).
type Extra<'a, 'd> = &'a mut dyn FnMut(&mut W<'d>) -> R<()>;

#[derive(Default, Clone, Copy)]
struct O {
    top: bool,
    paper: bool,
}

struct W<'d> {
    doc: &'d Doc,
    lines: Vec<String>,
    counts: Obj,
    used: HashSet<String>,
    name_map: HashMap<String, String>,
    anon: u64,
    lt_used: Vec<(String, Vec<f64>)>,
    block_names: HashMap<String, String>,
    dim_name: HashMap<Src, String>,
    no_plot: Vec<String>,
    ext: [f64; 4],
    extents: Option<cc_cad_geom::geom::Bounds>,
    layer_keys_upper: HashSet<String>,
}

fn ev(e: &Entity) -> V {
    jsv::from_json(&e.to_value())
}

impl<'d> W<'d> {
    fn w(&mut self, code: i64, v: impl Into<String>) {
        self.lines.push(format!("{code:>3}"));
        self.lines.push(v.into());
    }
    fn wn(&mut self, code: i64, n: f64) {
        self.w(code, js::num_to_string(n));
    }
    fn wcode(&mut self, code: &V, v: String) {
        self.lines.push(format!("{:>3}", sv(code)));
        self.lines.push(v);
    }
    fn xy(&mut self, c: i64, p: P2) {
        self.w(c, ff(p.x));
        self.w(c.saturating_add(10), ff(p.y));
        self.w(c.saturating_add(20), "0.0");
    }
    fn note(&mut self, k: &str, n: f64) {
        let v = jsv::or(self.counts.get(k), 0.0) + n;
        self.counts.set(k.to_string(), v);
    }
    fn count(&self, k: &str) -> f64 {
        jsv::to_num(self.counts.get(k))
    }
    fn has_count(&self, k: &str) -> bool {
        self.counts.get(k).is_some_and(V::truthy)
    }

    /// `r12Name(name)`: a name R12 (and AutoCAD reading R12) accepts, unique.
    fn r12(&mut self, name: &str) -> String {
        if let Some(n) = self.name_map.get(name) {
            return n.clone();
        }
        let up = name.to_uppercase().replace('|', "$0$");
        let mut n = String::new();
        for c in up.chars() {
            if c.is_ascii_uppercase() || c.is_ascii_digit() || c == '$' || c == '_' || c == '-' {
                n.push(c);
            } else {
                for _ in 0..c.len_utf16() {
                    n.push('_');
                }
            }
        }
        let mut n: String = n.chars().take(31).collect();
        if n.is_empty() {
            n = "UNNAMED".into();
        }
        let stem: String = n.chars().take(27).collect();
        let mut k = 1u64;
        while self.used.contains(&n) {
            n = format!("{stem}_{k}");
            k = k.saturating_add(1);
        }
        self.used.insert(n.clone());
        self.name_map.insert(name.to_string(), n.clone());
        if n != name {
            self.note("renamed", 1.0);
        }
        n
    }
    fn anon_name(&mut self, prefix: &str) -> String {
        loop {
            self.anon = self.anon.saturating_add(1);
            let n = format!("{prefix}{}", self.anon);
            if !self.used.contains(&n) {
                self.used.insert(n.clone());
                return n;
            }
        }
    }
    fn layer_name(&mut self, e: &V) -> String {
        let l = jsv::get(Some(e), "layer");
        let n = if jsv::truthy(l) { jsv::to_str(l) } else { "0".into() };
        self.r12(&n)
    }

    /// `ltName(n)`: a linetype name → its R12 name (null by layer), recording the patterns to write.
    fn lt_name(&mut self, n: Ov) -> Option<String> {
        if !jsv::truthy(n) {
            return None;
        }
        let s = jsv::to_str(n);
        if s.eq_ignore_ascii_case("bylayer") {
            return None;
        }
        if s.eq_ignore_ascii_case("byblock") {
            return Some("BYBLOCK".into());
        }
        if s.eq_ignore_ascii_case("continuous") {
            return Some("CONTINUOUS".into());
        }
        let Some(pat) = cc_cad_geom::linetypes::pattern_of(self.doc, Some(&s)) else { return Some("CONTINUOUS".into()) };
        let mut r = String::new();
        for c in s.to_uppercase().chars() {
            if c.is_ascii_uppercase() || c.is_ascii_digit() || c == '$' || c == '_' || c == '-' {
                r.push(c);
            } else {
                for _ in 0..c.len_utf16() {
                    r.push('_');
                }
            }
        }
        let r: String = r.chars().take(31).collect();
        match self.lt_used.iter_mut().find(|(k, _)| *k == r) {
            Some(slot) => slot.1 = pat,
            None => self.lt_used.push((r.clone(), pat)),
        }
        Some(r)
    }

    /// `colorCode(c)`
    fn color_code(&mut self, c: Ov) -> Option<String> {
        match c {
            None | Some(V::Null) => None,
            Some(V::Num(n)) if *n == 0.0 => Some("0".into()),
            Some(V::Str(s)) => {
                self.note("truecolor", 1.0);
                Some(cc_cad::nearest_aci(s).to_string())
            }
            Some(v) => Some(sv(v)),
        }
    }

    fn drawing_extents(&mut self) -> cc_cad_geom::geom::Bounds {
        if let Some(b) = &self.extents {
            return cc_cad_geom::geom::Bounds { min_x: b.min_x, min_y: b.min_y, max_x: b.max_x, max_y: b.max_y };
        }
        let mut b = empty_bounds();
        for x in self.doc.model() {
            let q = entity_bounds(x, Some(&self.doc.blocks));
            if valid_bounds(&q) {
                b.min_x = js::min(b.min_x, q.min_x);
                b.min_y = js::min(b.min_y, q.min_y);
                b.max_x = js::max(b.max_x, q.max_x);
                b.max_y = js::max(b.max_y, q.max_y);
            }
        }
        let e = if valid_bounds(&b) && (b.max_x > b.min_x || b.max_y > b.min_y) {
            b
        } else {
            cc_cad_geom::geom::Bounds { min_x: -100.0, min_y: -100.0, max_x: 100.0, max_y: 100.0 }
        };
        self.extents = Some(cc_cad_geom::geom::Bounds { min_x: e.min_x, min_y: e.min_y, max_x: e.max_x, max_y: e.max_y });
        e
    }

    fn grow(&mut self, x: f64, y: f64, o: O) {
        if !o.top {
            return;
        }
        self.ext[0] = js::min(self.ext[0], x);
        self.ext[1] = js::min(self.ext[1], y);
        self.ext[2] = js::max(self.ext[2], x);
        self.ext[3] = js::max(self.ext[3], y);
    }

    fn head(&mut self, typ: &str, e: &V, o: O) -> R<()> {
        self.w(0, typ);
        let ln = self.layer_name(e);
        self.w(8, ln);
        if let Some(lt) = self.lt_name(jsv::get(Some(e), "lt")) {
            self.w(6, lt);
        }
        if let Some(c) = self.color_code(jsv::get(Some(e), "color")) {
            self.w(62, c);
        }
        if o.paper {
            self.wn(67, 1.0);
        }
        let th = jsv::get(Some(e), "th");
        if jsv::truthy(th) && TH_TYPES.contains(&jsv::to_str(jsv::get(Some(e), "type")).as_str()) && typ != "VERTEX" && typ != "SEQEND" {
            let s = fv(th)?;
            self.w(39, s);
        }
        let tr = jsv::get(Some(e), "tr");
        if jsv::fin(tr).is_some() || jsv::s(tr) == Some("ByBlock") {
            self.note("tr", 1.0);
        }
        Ok(())
    }

    /// `xd(e, o)`: CadCraft's own xdata and the Properties fields without a group code, one block per application.
    fn xd(&mut self, e: &V) -> R<()> {
        let eo = jsv::obj(Some(e)).cloned().unwrap_or_default();
        let x = props::props_xdata(&eo, true);
        let cc: Vec<String> = jsv::arr(x.get("CADCRAFT")).iter().map(|it| sv(&props::cv(it).1.cloned().unwrap_or(V::Null))).collect();
        if !cc.is_empty() {
            self.w(1001, "CADCRAFT");
            for v in cc {
                self.w(1000, encode_text(&v));
            }
        }
        for app in jsv::keys_js(&x) {
            if app == "CADCRAFT" {
                continue;
            }
            self.w(1001, app.to_uppercase());
            for it in jsv::arr(x.get(app)) {
                let (c, v) = props::cv(it);
                let code = c.cloned().unwrap_or(V::Null);
                let s = if jsv::num(c) == Some(1000.0) {
                    encode_text(&jsv::to_str(v))
                } else if jsv::num(c) == Some(1040.0) {
                    ff(jsv::to_num(v))
                } else {
                    jsv::to_str(v)
                };
                self.wcode(&code, s);
            }
        }
        if jsv::s(jsv::get(Some(e), "mat")).is_some_and(|m| !m.is_empty()) {
            self.note("material", 1.0);
        }
        Ok(())
    }

    /// `polyline(e, pts, closed, o, xdFn)`
    fn polyline(&mut self, e: &V, pts: &[V], closed: bool, o: O, xd: &mut dyn FnMut(&mut Self) -> R<()>) -> R<()> {
        let poly = jsv::s(jsv::get(Some(e), "type")) == Some("poly");
        let gw = if poly { jsv::fin(jsv::get(Some(e), "w")).filter(|w| *w > 0.0).unwrap_or(0.0) } else { 0.0 };
        let each = poly && pts.iter().any(|p| jsv::fin(jsv::get(Some(p), "sw")).is_some() || jsv::fin(jsv::get(Some(p), "ew")).is_some());
        self.head("POLYLINE", e, o)?;
        self.wn(66, 1.0);
        self.w(10, "0.0");
        self.w(20, "0.0");
        let z = if poly { zf(jsv::get(Some(e), "z")) } else { "0.0".into() };
        self.w(30, z.clone());
        self.wn(70, if closed { 1.0 } else { 0.0 });
        if gw != 0.0 {
            self.w(40, ff(gw));
            self.w(41, ff(gw));
        }
        xd(self)?;
        let ln = self.layer_name(e);
        for p in pts {
            self.w(0, "VERTEX");
            self.w(8, ln.clone());
            if o.paper {
                self.wn(67, 1.0);
            }
            let s = fv(jsv::get(Some(p), "x"))?;
            self.w(10, s);
            let s = fv(jsv::get(Some(p), "y"))?;
            self.w(20, s);
            self.w(30, z.clone());
            if each {
                let sw = jsv::fin(jsv::get(Some(p), "sw")).unwrap_or(gw);
                let ew = jsv::fin(jsv::get(Some(p), "ew")).unwrap_or(gw);
                self.w(40, ff(sw));
                self.w(41, ff(ew));
            }
            let b = jsv::get(Some(p), "b");
            if jsv::truthy(b) {
                let s = fv(b)?;
                self.w(42, s);
            }
        }
        self.w(0, "SEQEND");
        self.w(8, ln);
        if o.paper {
            self.wn(67, 1.0);
        }
        Ok(())
    }

    fn text_out(&mut self, e: &V, s: &str, x: f64, y: f64, o: O) -> R<()> {
        let g = |k: &str| jsv::get(Some(e), k);
        self.head("TEXT", e, o)?;
        self.w(10, ff(x));
        self.w(20, ff(y));
        self.w(30, zf(g("z")));
        let h = fv(g("h"))?;
        self.w(40, h);
        self.w(1, encode_text(s));
        if jsv::truthy(g("rot")) {
            self.w(50, ff(deg(jsv::to_num(g("rot")))));
        }
        if jsv::truthy(g("wf")) && !props::strict_eq(g("wf"), Some(&V::Num(1.0))) {
            let s = fv(g("wf"))?;
            self.w(41, s);
        }
        if jsv::truthy(g("obl")) {
            self.w(51, ff(deg(jsv::to_num(g("obl")))));
        }
        if jsv::truthy(g("style")) && !jsv::to_str(g("style")).eq_ignore_ascii_case("standard") {
            let n = self.r12(&jsv::to_str(g("style")));
            self.w(7, n);
        }
        let (h72, v73) = aligns(e);
        if h72 != 0 {
            self.wn(72, jsv::i2f(h72));
        }
        if h72 != 0 || v73 != 0 {
            self.w(11, ff(x));
            self.w(21, ff(y));
            self.w(31, zf(g("z")));
        }
        if v73 != 0 {
            self.wn(73, jsv::i2f(v73));
        }
        self.xd(e)
    }

    /// `attrValue(s)`: R12 strings stop at 255 characters.
    fn attr_value(&mut self, s: Ov) -> String {
        let enc = encode_text(&one_line(&jsv::str_nn(s)));
        if len16(&enc) <= 255 {
            return enc;
        }
        self.note("longattr", 1.0);
        let cut = slice16(&enc, 0, 255);
        trim_escape_tail(&cut)
    }

    /// `attribute(type, t, o, d)`: ATTDEF (in a block) or ATTRIB (after its INSERT).
    fn attribute(&mut self, typ: &str, t: &V, o: O, d: Option<&V>) -> R<()> {
        let g = |k: &str| jsv::get(Some(t), k);
        self.head(typ, t, o)?;
        let s = fv(g("x"))?;
        self.w(10, s);
        let s = fv(g("y"))?;
        self.w(20, s);
        self.w(30, "0.0");
        let s = fv(g("h"))?;
        self.w(40, s);
        let v = self.attr_value(g("s"));
        self.w(1, v);
        if let Some(d) = d {
            let p = jsv::get(Some(d), "prompt");
            let pv = if jsv::truthy(p) { p.cloned() } else { Some(V::from("")) };
            let v = self.attr_value(pv.as_ref());
            self.w(3, v);
        }
        let tag = g("tag");
        self.w(2, encode_text(&if jsv::truthy(tag) { jsv::to_str(tag) } else { "TAG".into() }));
        let fl = (if jsv::truthy(g("invisible")) { 1 } else { 0 })
            | (if d.is_some_and(|d| jsv::truthy(jsv::get(Some(d), "constant"))) { 2 } else { 0 })
            | (if d.is_some_and(|d| jsv::truthy(jsv::get(Some(d), "preset"))) { 8 } else { 0 });
        self.wn(70, f64::from(fl));
        if jsv::truthy(g("rot")) {
            self.w(50, ff(deg(jsv::to_num(g("rot")))));
        }
        if jsv::truthy(g("wf")) && !props::strict_eq(g("wf"), Some(&V::Num(1.0))) {
            let s = fv(g("wf"))?;
            self.w(41, s);
        }
        let (h72, v74) = aligns(t);
        if h72 != 0 {
            self.wn(72, jsv::i2f(h72));
        }
        if h72 != 0 || v74 != 0 {
            let s = fv(g("x"))?;
            self.w(11, s);
            let s = fv(g("y"))?;
            self.w(21, s);
            self.w(31, "0.0");
        }
        if v74 != 0 {
            self.wn(74, jsv::i2f(v74));
        }
        Ok(())
    }

    fn layout_xdata(&mut self, name: &str, paper: &V) -> R<()> {
        let g = |k: &str| jsv::get(Some(paper), k);
        self.w(1001, "CADCRAFT");
        self.w(1000, "LAYOUT");
        self.w(1000, slice16(&encode_text(name), 0, 255));
        self.w(1000, jsv::to_str(g("unit")));
        self.w(1000, jsv::str_or_empty(g("key")));
        for k in ["x0", "y0", "w", "h"] {
            let s = fv(g(k))?;
            self.w(1040, s);
        }
        if !self.no_plot.is_empty() {
            self.w(1000, "NOPLOT");
            for n in self.no_plot.clone() {
                self.w(1000, n);
            }
        }
        Ok(())
    }

    /// `frzNames(e)`: a viewport's frozen layers under their R12 names, only those the file has.
    fn frz_names(&mut self, e: &V) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for n in jsv::arr(jsv::get(Some(e), "frz")) {
            let Some(s) = jsv::s(Some(n)) else { continue };
            if !self.layer_keys_upper.contains(&s.to_uppercase()) {
                continue;
            }
            let r = self.r12(s);
            if !out.contains(&r) {
                out.push(r);
            }
        }
        out
    }

    fn viewport(&mut self, e: &V, id: f64, extra: Option<(&str, &V)>) -> R<()> {
        let g = |k: &str| jsv::get(Some(e), k);
        self.w(0, "VIEWPORT");
        let ln = self.layer_name(e);
        self.w(8, ln);
        if let Some(c) = self.color_code(g("color")) {
            self.w(62, c);
        }
        self.wn(67, 1.0);
        let s = fv(g("x"))?;
        self.w(10, s);
        let s = fv(g("y"))?;
        self.w(20, s);
        self.w(30, "0.0");
        let s = fv(g("w"))?;
        self.w(40, s);
        let s = fv(g("h"))?;
        self.w(41, s);
        let on68 = if matches!(g("on"), Some(V::Bool(false))) {
            0.0
        } else if id == 1.0 {
            1.0
        } else {
            id
        };
        self.wn(68, on68);
        self.wn(69, id);
        self.w(1001, "ACAD");
        self.w(1000, "MVIEW");
        self.w(1002, "{");
        self.wn(1070, 16.0);
        for v in ["0.0", "0.0", "0.0"] {
            let _ = v;
        }
        self.w(1010, "0.0");
        self.w(1020, "0.0");
        self.w(1030, "0.0");
        self.w(1010, "0.0");
        self.w(1020, "0.0");
        self.w(1030, "1.0");
        self.w(1040, ff(jsv::fin(g("tw")).unwrap_or(0.0)));
        self.w(1040, ff(jsv::to_num(g("h")) / jsv::to_num(g("s"))));
        let s = fv(g("cx"))?;
        self.w(1040, s);
        let s = fv(g("cy"))?;
        self.w(1040, s);
        self.w(1040, "50.0");
        self.w(1040, "0.0");
        self.w(1040, "0.0");
        for v in [0.0, 100.0, 1.0, 3.0, 0.0, 0.0, 0.0, 0.0] {
            self.wn(1070, v);
        }
        for v in ["0.0", "0.0", "0.0", "1.0", "1.0", "1.0", "1.0"] {
            self.w(1040, v);
        }
        self.wn(1070, 0.0);
        self.w(1002, "{");
        for n in self.frz_names(e) {
            self.w(1003, n);
        }
        self.w(1002, "}");
        self.w(1002, "}");
        if let Some((name, paper)) = extra {
            self.layout_xdata(name, paper)?;
        } else if jsv::truthy(g("locked")) {
            self.w(1001, "CADCRAFT");
            self.w(1000, "CCVPLOCK");
        }
        Ok(())
    }

    fn dimension(&mut self, ent: &Entity, e: &V, o: O, src: Option<&Src>) -> R<()> {
        let Some(name) = src.and_then(|s| self.dim_name.get(s)).cloned() else { return Ok(()) };
        self.head("DIMENSION", e, o)?;
        self.w(2, name);
        let tp = cc_cad_geom::dims::dim_text_point(ent);
        let mut ds = props::dim_defaults();
        if let Some(V::Obj(st)) = jsv::get(Some(e), "st") {
            ds.assign(st.clone());
        }
        let g = |k: &str| jsv::get(Some(e), k);
        let pt = |k: &str| -> R<P2> {
            let p = g(k);
            Ok(P2::new(jsv::to_num(jsv::prop(p, "x")?), jsv::to_num(jsv::prop(p, "y")?)))
        };
        let tp_req = || tp.ok_or_else(|| JsErr("Cannot read properties of undefined (reading 'x')".into()));
        let text = g("text");
        match jsv::s(g("kind")) {
            Some(k @ ("linear" | "aligned")) => {
                let (p1, p2, loc) = (pt("p1")?, pt("p2")?, pt("loc")?);
                let r = if k == "aligned" { (p2.y - p1.y).jatan2(p2.x - p1.x) } else { jsv::or(g("rot"), 0.0) };
                let (ux, uy) = (r.jcos(), r.jsin());
                let d2 = (p2.x - loc.x) * ux + (p2.y - loc.y) * uy;
                self.xy(10, P2::new(loc.x + ux * d2, loc.y + uy * d2));
                let t = tp_req()?;
                self.xy(11, t);
                self.wn(70, if k == "aligned" { 1.0 } else { 0.0 });
                if jsv::truthy(text) {
                    self.w(1, encode_text(&jsv::to_str(text)));
                }
                self.xy(13, p1);
                self.xy(14, p2);
                if k == "linear" {
                    self.w(50, ff(deg(norm_angle(jsv::or(g("rot"), 0.0)))));
                }
            }
            Some(k @ ("radius" | "diameter")) => {
                let (p1, c, loc) = (pt("p1")?, pt("c")?, pt("loc")?);
                let r = js::hypot(p1.x - c.x, p1.y - c.y);
                let dl = js::hypot(loc.x - c.x, loc.y - c.y);
                let a = if dl > 1e-12 { (loc.y - c.y).jatan2(loc.x - c.x) } else { (p1.y - c.y).jatan2(p1.x - c.x) };
                let near = P2::new(c.x + r * a.jcos(), c.y + r * a.jsin());
                let far = P2::new(c.x - r * a.jcos(), c.y - r * a.jsin());
                self.xy(10, if k == "radius" { c } else { far });
                let t = tp_req()?;
                self.xy(11, t);
                self.wn(70, if k == "radius" { 4.0 } else { 3.0 });
                if jsv::truthy(text) {
                    self.w(1, encode_text(&jsv::to_str(text)));
                }
                self.xy(15, near);
                self.w(40, "0.0");
            }
            Some("angular") => {
                let loc = pt("loc")?;
                self.xy(10, loc);
                let t = tp_req()?;
                self.xy(11, t);
                self.wn(70, 5.0);
                if jsv::truthy(text) {
                    self.w(1, encode_text(&jsv::to_str(text)));
                }
                let (p1, p2, c) = (pt("p1")?, pt("p2")?, pt("c")?);
                self.xy(13, p1);
                self.xy(14, p2);
                self.xy(15, c);
            }
            _ => {}
        }
        let dsn = g("dimstyle");
        let style = if jsv::truthy(dsn) && !jsv::to_str(dsn).eq_ignore_ascii_case("standard") {
            self.r12(&jsv::to_str(dsn))
        } else {
            "STANDARD".into()
        };
        self.w(3, style);
        self.w(1001, "ACAD");
        for (code, v) in props::dstyle_items(Some(&ds), true) {
            let s = if code == 1040 {
                ff(jsv::to_num(Some(&v)))
            } else if code == 1000 {
                encode_text(&sv(&v))
            } else {
                sv(&v)
            };
            self.w(code, s);
        }
        self.w(1001, "CADCRAFT");
        self.w(1000, "DIM");
        for k in ["dec", "adec", "zin"] {
            self.wn(1070, js::int32(jsv::to_num(ds.get(k))));
        }
        let lu = jsv::orv(ds.get("lunit"), V::Num(2.0));
        self.w(1070, sv(&lu));
        if jsv::truthy(g("trot")) {
            self.w(1000, format!("CCTROT:{}", jsv::to_str(g("trot"))));
        }
        for v in props::cc_dim_items(Some(&ds), true) {
            self.w(1000, encode_text(&v));
        }
        self.note("dims", 1.0);
        Ok(())
    }

    fn entity(&mut self, ent: &Entity, o: O, src: Option<&Src>) -> R<()> {
        let e = ev(ent);
        let g = |k: &str| jsv::get(Some(&e), k);
        let num = |k: &str| jsv::to_num(jsv::get(Some(&e), k));
        match ent.typ() {
            t @ ("xline" | "ray") => {
                let ext = self.drawing_extents();
                let (x, y, dx, dy) = (num("x"), num("y"), num("dx"), num("dy"));
                let tt = clip_infinite(P2::new(x, y), P2::new(dx, dy), &ext, t == "ray");
                self.note("infinite", 1.0);
                let Some((t0, t1)) = tt.filter(|t| t.1 > t.0) else { return Ok(()) };
                let a = P2::new(x + dx * t0, y + dy * t0);
                let b = P2::new(x + dx * t1, y + dy * t1);
                self.head("LINE", &e, o)?;
                self.w(10, ff(a.x));
                self.w(20, ff(a.y));
                self.w(30, "0.0");
                self.w(11, ff(b.x));
                self.w(21, ff(b.y));
                self.w(31, "0.0");
                self.grow(a.x, a.y, o);
                self.grow(b.x, b.y, o);
                self.xd(&e)?;
            }
            "line" => {
                self.head("LINE", &e, o)?;
                let s = fv(g("x1"))?;
                self.w(10, s);
                let s = fv(g("y1"))?;
                self.w(20, s);
                self.w(30, zf(g("z1")));
                let s = fv(g("x2"))?;
                self.w(11, s);
                let s = fv(g("y2"))?;
                self.w(21, s);
                self.w(31, zf(g("z2")));
                self.grow(num("x1"), num("y1"), o);
                self.grow(num("x2"), num("y2"), o);
                self.xd(&e)?;
            }
            "point" => {
                self.head("POINT", &e, o)?;
                let s = fv(g("x"))?;
                self.w(10, s);
                let s = fv(g("y"))?;
                self.w(20, s);
                self.w(30, zf(g("z")));
                self.grow(num("x"), num("y"), o);
                self.xd(&e)?;
            }
            t @ ("circle" | "arc") => {
                self.head(if t == "circle" { "CIRCLE" } else { "ARC" }, &e, o)?;
                let s = fv(g("cx"))?;
                self.w(10, s);
                let s = fv(g("cy"))?;
                self.w(20, s);
                self.w(30, zf(g("z")));
                let s = fv(g("r"))?;
                self.w(40, s);
                if t == "arc" {
                    self.w(50, ff(deg(norm_angle(num("a0")))));
                    self.w(51, ff(deg(norm_angle(num("a1")))));
                }
                let (cx, cy, r) = (num("cx"), num("cy"), num("r"));
                self.grow(cx - r, cy - r, o);
                self.grow(cx + r, cy + r, o);
                self.xd(&e)?;
            }
            "poly" => {
                let pts: Vec<V> = jsv::arr(g("pts")).to_vec();
                let ec = e.clone();
                self.polyline(&e, &pts, jsv::truthy(g("closed")), o, &mut |w: &mut Self| w.xd(&ec))?;
                for p in &pts {
                    self.grow(jsv::to_num(jsv::get(Some(p), "x")), jsv::to_num(jsv::get(Some(p), "y")), o);
                }
            }
            "ellipse" => {
                self.note("ellipse", 1.0);
                let pts: Vec<V> =
                    ellipse_points(ent, 72.0).iter().map(|p| V::Obj(cc_io_dwg::obj! { "x" => p.x, "y" => p.y, "b" => 0.0 })).collect();
                let closed = num("t1") - num("t0") >= TAU - 1e-9;
                self.polyline(&e, &pts, closed, o, &mut |_: &mut Self| Ok(()))?;
            }
            "hatch" => {
                if jsv::truthy(g("wipe")) {
                    return Ok(());
                }
                let rings: Vec<Vec<V>> = jsv::arr(g("loops"))
                    .iter()
                    .map(|l| {
                        let a = jsv::arr(Some(l));
                        let mut pts = Vec::new();
                        let mut k = 0usize;
                        while k.saturating_add(1) < a.len() {
                            let mut q = Obj::new();
                            q.set("x", a.get(k).cloned().unwrap_or(V::Null));
                            q.set("y", a.get(k.saturating_add(1)).cloned().unwrap_or(V::Null));
                            q.set("b", 0.0);
                            pts.push(V::Obj(q));
                            k = k.saturating_add(2);
                        }
                        pts
                    })
                    .collect();
                let q = if rings.len() == 1 && jsv::truthy(g("solid")) { rings.first().map(|r| open_ring(r)) } else { None };
                if let Some(q) = q.filter(|q| q.len() >= 3 && q.len() <= 4) {
                    self.head("SOLID", &e, o)?;
                    let p = |i: usize| q.get(i).map(|v| P2::new(jsv::to_num(jsv::get(Some(v), "x")), jsv::to_num(jsv::get(Some(v), "y"))));
                    let (a, b, c) = (
                        p(0).unwrap_or(P2::new(f64::NAN, f64::NAN)),
                        p(1).unwrap_or(P2::new(f64::NAN, f64::NAN)),
                        p(2).unwrap_or(P2::new(f64::NAN, f64::NAN)),
                    );
                    let d = p(3).unwrap_or(c);
                    self.xy(10, a);
                    self.xy(11, b);
                    self.xy(12, d);
                    self.xy(13, c);
                    return Ok(());
                }
                self.note("hatch", 1.0);
                for pts in &rings {
                    self.polyline(&e, pts, true, o, &mut |_: &mut Self| Ok(()))?;
                }
            }
            "text" => {
                if jsv::truthy(g("bgm")) {
                    self.note("textmask", 1.0);
                }
                let Geo::Text(t) = &ent.geo else {
                    self.note("other", 1.0);
                    return Ok(());
                };
                let lay = text_layout(t);
                if lay.lines.len() == 1 {
                    let s = jsv::to_str(g("s"));
                    self.text_out(&e, &s, t.x, t.y, o)?;
                    self.grow(t.x, t.y, o);
                    return Ok(());
                }
                self.note("mtext", 1.0);
                let m = m_mul(&m_translate(t.x, t.y), &m_rotate(js::or(t.rot, 0.0)));
                let [bx, _, bw, _] = lay.bx;
                let ax = match jsv::s(g("ha")) {
                    Some("center") => bx + bw / 2.0,
                    Some("right") => bx + bw,
                    _ => bx,
                };
                let mut eb = jsv::obj(Some(&e)).cloned().unwrap_or_default();
                eb.set("va", "baseline");
                let ebv = V::Obj(eb);
                for (k, s) in lay.lines.iter().enumerate() {
                    let p = m_apply(&m, P2::new(ax, lay.baselines.get(k).copied().unwrap_or(f64::NAN)));
                    self.text_out(&ebv, s, p.x, p.y, o)?;
                    self.grow(p.x, p.y, o);
                }
            }
            "insert" => {
                let block = jsv::to_str(g("block"));
                let blk = self.doc.blocks.get(&block);
                let has_attrs = jsv::obj(g("attrs")).is_some_and(|a| !a.is_empty());
                let atts: Vec<V> = match (blk, &ent.geo) {
                    (Some(b), Geo::Insert(ins)) if has_attrs => world_attribs(ins, b),
                    _ => Vec::new(),
                };
                self.head("INSERT", &e, o)?;
                if !atts.is_empty() {
                    self.wn(66, 1.0);
                }
                let bn = match self.block_names.get(&block) {
                    Some(n) if !n.is_empty() => n.clone(),
                    _ => self.r12(&block),
                };
                self.w(2, bn);
                let s = fv(g("x"))?;
                self.w(10, s);
                let s = fv(g("y"))?;
                self.w(20, s);
                self.w(30, zf(g("z")));
                if !props::strict_eq(Some(&jsv::nnv(g("sx"), V::Num(1.0))), Some(&V::Num(1.0))) {
                    let s = fv(g("sx"))?;
                    self.w(41, s);
                }
                if !props::strict_eq(Some(&jsv::nnv(g("sy"), V::Num(1.0))), Some(&V::Num(1.0))) {
                    let s = fv(g("sy"))?;
                    self.w(42, s);
                }
                if let Some(sz) = jsv::fin(g("sz")).filter(|s| *s != 0.0 && *s != 1.0) {
                    self.w(43, ff(sz));
                }
                if jsv::truthy(g("rot")) {
                    self.w(50, ff(deg(num("rot"))));
                }
                self.grow(num("x"), num("y"), o);
                self.xd(&e)?;
                if !atts.is_empty() {
                    for t in &atts {
                        self.attribute("ATTRIB", t, o, None)?;
                    }
                    self.w(0, "SEQEND");
                    let ln = self.layer_name(&e);
                    self.w(8, ln);
                    if o.paper {
                        self.wn(67, 1.0);
                    }
                }
            }
            "dim" => self.dimension(ent, &e, o, src)?,
            "viewport" => {
                let (x, y, w, h) = (num("x"), num("y"), num("w"), num("h"));
                let pts: Vec<V> = cc_cad_geom::geom::viewport_corners(x, y, w, h)
                    .iter()
                    .map(|p| V::Obj(cc_io_dwg::obj! { "x" => p.x, "y" => p.y, "b" => 0.0 }))
                    .collect();
                let ec = e.clone();
                self.polyline(&e, &pts, true, o, &mut |wr: &mut Self| {
                    let gg = |k: &str| jsv::get(Some(&ec), k);
                    wr.w(1001, "CADCRAFT");
                    wr.w(1000, "VIEWPORT");
                    let s = fv(gg("cx"))?;
                    wr.w(1040, s);
                    let s = fv(gg("cy"))?;
                    wr.w(1040, s);
                    let s = fv(gg("s"))?;
                    wr.w(1040, s);
                    let fz = wr.frz_names(&ec);
                    if !fz.is_empty() {
                        wr.w(1000, "FRZ");
                        for n in fz {
                            wr.w(1000, n);
                        }
                    }
                    Ok(())
                })?;
            }
            "mleader" => {
                self.note("mleader", 1.0);
                let parts = cc_cad_geom::corners::explode_entity(ent, Some(&self.doc.blocks)).unwrap_or_default();
                for p in &parts {
                    self.entity(p, o, None)?;
                }
            }
            _ => self.note("other", 1.0),
        }
        Ok(())
    }

    fn block(
        &mut self,
        name: &str,
        base: (Ov, Ov),
        ents: &[(Entity, Option<Src>)],
        extra: Option<Extra<'_, 'd>>,
        attdefs: Option<&[serde_json::Value]>,
        xref_path: Option<String>,
    ) -> R<()> {
        self.w(0, "BLOCK");
        self.w(8, "0");
        self.w(2, name);
        let flags = (if name.starts_with('*') { 1 } else { 0 })
            | (if attdefs.is_some_and(|a| !a.is_empty()) { 2 } else { 0 })
            | (if xref_path.is_some() { 4 } else { 0 });
        self.wn(70, f64::from(flags));
        let s = fv(base.0)?;
        self.w(10, s);
        let s = fv(base.1)?;
        self.w(20, s);
        self.w(30, "0.0");
        self.w(3, name);
        if let Some(p) = xref_path {
            self.w(1, slice16(&encode_text(&p), 0, 255));
        }
        if let Some(f) = extra {
            f(self)?;
        }
        for (e, src) in ents {
            self.entity(e, O::default(), src.as_ref())?;
        }
        for d in attdefs.unwrap_or(&[]) {
            let dv = jsv::from_json(d);
            let mut t = Obj::new();
            t.set("type", "text");
            if let V::Obj(o) = &dv {
                t.assign(o.clone());
            }
            let def = jsv::get(Some(&dv), "def");
            t.set("s", if jsv::truthy(def) { def.cloned().unwrap_or(V::Null) } else { V::from("") });
            self.attribute("ATTDEF", &V::Obj(t), O::default(), Some(&dv))?;
        }
        self.w(0, "ENDBLK");
        self.w(8, "0");
        Ok(())
    }
}

/// The R12 justification codes of a text (72) and its vertical one (73 / 74).
fn aligns(e: &V) -> (i64, i64) {
    let h = match jsv::s(jsv::get(Some(e), "ha")) {
        Some("center") => 1,
        Some("right") => 2,
        _ => 0,
    };
    let v = match jsv::s(jsv::get(Some(e), "va")) {
        Some("middle") => 2,
        Some("top") => 3,
        Some("bottom") => 1,
        _ => 0,
    };
    (h, v)
}

/// `enc.replace(/\\U\+[0-9A-F]{0,3}$|\\U?$|%%?$/, '')`: a cut escape at the end dropped.
fn trim_escape_tail(s: &str) -> String {
    let b = s.as_bytes();
    for i in 0..=b.len() {
        let tail = s.get(i..).unwrap_or("");
        let alt1 =
            tail.strip_prefix("\\U+").is_some_and(|r| r.len() <= 3 && r.bytes().all(|c| c.is_ascii_digit() || (b'A'..=b'F').contains(&c)));
        let alt2 = tail == "\\U" || tail == "\\";
        let alt3 = tail == "%%" || tail == "%";
        if alt1 || alt2 || alt3 {
            return s.get(..i).unwrap_or("").to_string();
        }
    }
    s.to_string()
}

/// `openRing(pts)`: a closed ring's distinct corners (the repeated closing point dropped).
fn open_ring(pts: &[V]) -> Vec<V> {
    let mut out = pts.to_vec();
    if out.len() > 1 {
        let n = |v: Option<&V>, k: &str| jsv::to_num(v.and_then(|v| jsv::get(Some(v), k)));
        let (f, l) = (out.first(), out.last());
        if js::hypot(n(f, "x") - n(l, "x"), n(f, "y") - n(l, "y")) < 1e-12 {
            out.pop();
        }
    }
    out
}

/// attrs.js `worldAttribs(e, blk)`: attribute texts in world coordinates (for writing ATTRIBs).
fn world_attribs(ins: &Insert, blk: &Block) -> Vec<V> {
    let m = insert_matrix(ins, blk);
    let defs: Vec<serde_json::Value> = blk.attdefs.as_deref().unwrap_or(&[]).to_vec();
    let mut attrs = ins.attrs.clone().unwrap_or_default();
    for d in &defs {
        if cc_cad::js::truthy(d.get("constant")) {
            let tag = d.get("tag").map(cc_cad::js::to_js_string).unwrap_or_else(|| "undefined".into());
            attrs.remove(&tag);
        }
    }
    let mut e2 = ins.clone();
    e2.attrs = Some(attrs);
    let mut b2 = Block::new(&blk.name, blk.base, Vec::new());
    b2.attdefs = Some(defs.into_iter().filter(|d| !cc_cad::js::truthy(d.get("constant"))).collect());
    cc_cad_geom::geom::attr_texts(&e2, Some(&b2), true)
        .into_iter()
        .map(|t| {
            let tag = t.extra.get("tag").cloned();
            let inv = cc_cad::js::truthy(t.extra.get("invisible"));
            let mut moved = cc_cad_geom::tx::tx_entity(&t, &m).to_value();
            if let Some(o) = moved.as_object_mut() {
                match tag {
                    Some(tg) => {
                        o.insert("tag".into(), tg);
                    }
                    None => {
                        o.remove("tag");
                    }
                }
                o.insert("invisible".into(), serde_json::Value::Bool(inv));
            }
            jsv::from_json(&moved)
        })
        .collect()
}

/// ucs.js `cleanUcs(v)` → (x, y, ang) or None (the world).
pub(crate) fn clean_ucs(v: Ov) -> Option<(f64, f64, f64)> {
    let o = jsv::obj(v)?;
    let (x, y, ang) = (jsv::to_num(o.get("x")), jsv::to_num(o.get("y")), jsv::to_num(o.get("ang")));
    if !(x.is_finite() && y.is_finite() && ang.is_finite()) || x.abs() > 1e15 || y.abs() > 1e15 {
        return None;
    }
    let a = norm_angle(ang);
    let world = x.abs() < 1e-12 && y.abs() < 1e-12 && js::min(a, TAU - a) < 1e-12;
    if world { None } else { Some((x, y, a)) }
}
/// ucs.js `ucsData(v)`
fn ucs_data(v: &V) -> Option<(f64, f64, f64)> {
    let o = jsv::obj(Some(v))?;
    let (x, y, ang) = (jsv::to_num(o.get("x")), jsv::to_num(o.get("y")), jsv::to_num(o.get("ang")));
    (x.is_finite() && y.is_finite() && ang.is_finite() && x.abs() <= 1e15 && y.abs() <= 1e15).then(|| (x, y, norm_angle(ang)))
}
/// The drawing's named UCSs as doc.load keeps them (`doc.ucsNamed`): (name, (x, y, ang)), at most 1000.
pub(crate) fn ucs_named(ucs_v: Option<&V>) -> Vec<(String, (f64, f64, f64))> {
    jsv::arr(jsv::get(ucs_v, "named"))
        .iter()
        .filter_map(|u| {
            let name = jsv::s(jsv::get(Some(u), "name"))?;
            if !ucs_name_ok(name) {
                return None;
            }
            Some((name.to_string(), ucs_data(u)?))
        })
        .take(1000)
        .collect()
}
/// ucs.js UCS_NAME_RE: 1–255 characters, none of `<>/\":;?*|,=` and backtick.
fn ucs_name_ok(s: &str) -> bool {
    let n = len16(s);
    (1..=255).contains(&n) && !s.chars().any(|c| "<>/\\\":;?*|,=`".contains(c))
}

/// `writeDxf(doc)` → (DXF R12 text, report { notes, counts, extents }), or the error the JS throws.
pub fn write_dxf(doc: &Doc) -> Result<(String, serde_json::Value), JsErr> {
    write_dxf_with(doc, Catalog::en())
}

/// `writeDxf` with a catalog for the report's notes.
#[allow(clippy::too_many_lines)]
pub fn write_dxf_with(doc: &Doc, cat: &Catalog) -> Result<(String, serde_json::Value), JsErr> {
    let mut w = W {
        doc,
        lines: Vec::new(),
        counts: Obj::new(),
        used: HashSet::new(),
        name_map: HashMap::new(),
        anon: 0,
        lt_used: Vec::new(),
        block_names: HashMap::new(),
        dim_name: HashMap::new(),
        no_plot: Vec::new(),
        ext: [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY],
        extents: None,
        layer_keys_upper: doc.layers.keys().map(|k| k.to_uppercase()).collect(),
    };
    for l in doc.layers.values() {
        w.r12(&l.name);
    }
    // which blocks to write: every named block; anonymous ones unless only a dimension draws them
    let model: Vec<&Entity> = doc.model().collect();
    let layouts: Vec<(usize, &cc_cad::Layout, Vec<&Entity>)> =
        doc.layouts.iter().enumerate().map(|(i, l)| (i, l, doc.space_entities(l.space).collect())).collect();
    let mut insert_used: HashSet<String> = HashSet::new();
    {
        let mut add = |e: &Entity| {
            if let Geo::Insert(i) = &e.geo {
                insert_used.insert(i.block.clone());
            } else if e.typ() == "insert"
                && let Some(b) = e.extra.get("block")
            {
                insert_used.insert(cc_cad::js::to_js_string(b));
            }
        };
        model.iter().for_each(|e| add(e));
        layouts.iter().for_each(|(_, _, l)| l.iter().for_each(|e| add(e)));
        doc.blocks.values().for_each(|b| b.entities.iter().for_each(&mut add));
    }
    for b in doc.blocks.values() {
        if b.name.starts_with('*') && !insert_used.contains(&b.name) {
            continue;
        }
        let n = if b.name.starts_with('*') { w.anon_name("*U") } else { w.r12(&b.name) };
        w.block_names.insert(b.name.clone(), n);
    }
    let mut dim_blocks: Vec<(String, DimBlock)> = Vec::new();
    {
        let mut collect = |w: &mut W, e: &Entity, src: Src| {
            let Geo::Dim(d) = &e.geo else { return };
            let own = d.block.as_ref().filter(|b| !b.is_empty() && doc.blocks.get(*b).is_some()).cloned();
            let name = w.anon_name("*D");
            w.dim_name.insert(src, name.clone());
            dim_blocks.push((
                name,
                match own {
                    Some(b) => DimBlock::Own(b),
                    None => DimBlock::Made(cc_cad_geom::dims::dim_geometry(e)),
                },
            ));
        };
        for (i, e) in model.iter().enumerate() {
            collect(&mut w, e, Src::Model(i));
        }
        for (li, _, list) in &layouts {
            for (i, e) in list.iter().enumerate() {
                collect(&mut w, e, Src::Layout(*li, i));
            }
        }
        for b in doc.blocks.values() {
            if w.block_names.contains_key(&b.name) {
                for (i, e) in b.entities.iter().enumerate() {
                    collect(&mut w, e, Src::Block(b.name.clone(), i));
                }
            }
        }
    }
    let extra_layouts: Vec<(usize, String)> = (1..doc.layouts.len()).map(|k| (k, format!("CCLAYOUT_{}", k.saturating_add(1)))).collect();
    for (_, n) in &extra_layouts {
        w.used.insert(n.clone());
    }
    // the 3D model (model3d.js): 3DFACEs on layer CC-3D + its exact copy in block CADCRAFT_3D
    let units = jsv::str_or_empty(jsv::from_json(&serde_json::Value::Object(doc.meta.clone())).as_obj().and_then(|m| m.get("units")));
    let m3 =
        if doc.faces3d.is_empty() { None } else { crate::m3d_file::for_file3d(&doc.faces3d, &units, &cat.t("model3d.faces_file", &[])) };
    if let Some(m3) = &m3 {
        w.used.insert(crate::model3d::M3D_BLOCK.into());
        if !doc.layers.contains_key(&m3.layer.to_string()) {
            w.r12(m3.layer);
        }
    }
    let add3d_layer = m3.as_ref().is_some_and(|m| !doc.layers.contains_key(&m.layer.to_string()));
    let no_plot: Vec<String> =
        doc.layers.values().filter(|l| !l.plot).map(|l| l.name.clone()).collect::<Vec<_>>().into_iter().map(|n| w.r12(&n)).collect();
    w.no_plot = no_plot;
    let mut st = props::dim_defaults();
    if let V::Obj(cd) = jsv::from_json(&serde_json::Value::Object(doc.current.dim.clone())) {
        st.assign(cd);
    }
    let first = layouts.first();
    let meta = jsv::from_json(&serde_json::Value::Object(doc.meta.clone()));
    let mg = |k: &str| jsv::get(Some(&meta), k).cloned();

    // HEADER
    w.w(0, "SECTION");
    w.w(2, "HEADER");
    w.w(9, "$ACADVER");
    w.w(1, "AC1009");
    w.w(9, "$DWGCODEPAGE");
    w.w(3, "ANSI_1252");
    w.w(9, "$INSBASE");
    w.w(10, "0.0");
    w.w(20, "0.0");
    w.w(30, "0.0");
    w.w(9, "$TEXTSIZE");
    w.w(40, ff(doc.current.text_height));
    let lts = mg("ltscale");
    if jsv::to_num(lts.as_ref()) > 0.0 && !props::strict_eq(lts.as_ref(), Some(&V::Num(1.0))) {
        w.w(9, "$LTSCALE");
        let s = fv(lts.as_ref())?;
        w.w(40, s);
    }
    let uc = unit_code(mg("units").as_ref());
    if uc != 0 {
        w.w(9, "$INSUNITS");
        w.wn(70, jsv::i2f(uc));
    }
    let ucs_v = doc.ucs.as_ref().map(jsv::from_json);
    if let Some((ux, uy, a)) = clean_ucs(jsv::get(ucs_v.as_ref(), "current")) {
        let a = js::or(Some(a), 0.0);
        w.w(9, "$UCSORG");
        w.w(10, ff(ux));
        w.w(20, ff(uy));
        w.w(30, "0.0");
        w.w(9, "$UCSXDIR");
        w.w(10, ff(a.jcos()));
        w.w(20, ff(a.jsin()));
        w.w(30, "0.0");
        w.w(9, "$UCSYDIR");
        w.w(10, ff(-a.jsin()));
        w.w(20, ff(a.jcos()));
        w.w(30, "0.0");
    }
    let lu = jsv::to_num(mg("lunits").as_ref());
    if (1.0..=5.0).contains(&lu) {
        w.w(9, "$LUNITS");
        w.wn(70, js::int32(lu));
        if let Some(p) = jsv::fin(mg("luprec").as_ref()) {
            w.w(9, "$LUPREC");
            w.wn(70, js::int32(p));
        }
    }
    for (name, key) in [
        ("$DIMSCALE", "scale"),
        ("$DIMASZ", "asz"),
        ("$DIMEXO", "exo"),
        ("$DIMDLI", "dli"),
        ("$DIMEXE", "exe"),
        ("$DIMTXT", "txt"),
        ("$DIMGAP", "gap"),
    ] {
        w.w(9, name);
        let s = fv(st.get(key))?;
        w.w(40, s);
    }
    w.w(9, "$DIMTAD");
    w.wn(70, js::int32(jsv::to_num(st.get("tad"))));
    w.w(9, "$DIMZIN");
    w.wn(70, js::int32(jsv::to_num(st.get("zin"))));
    w.w(9, "$TILEMODE");
    w.wn(70, 1.0);
    if let Some((_, l, _)) = first {
        let p = jsv::from_json(&serde_json::Value::Object(l.paper.clone()));
        let pg = |k: &str| jsv::get(Some(&p), k);
        w.w(9, "$PLIMMIN");
        let s = fv(pg("x0"))?;
        w.w(10, s);
        let s = fv(pg("y0"))?;
        w.w(20, s);
        w.w(9, "$PLIMMAX");
        w.w(10, ff(jsv::to_num(pg("x0")) + jsv::to_num(pg("w"))));
        w.w(20, ff(jsv::to_num(pg("y0")) + jsv::to_num(pg("h"))));
    }
    w.w(0, "ENDSEC");

    // TABLES
    w.w(0, "SECTION");
    w.w(2, "TABLES");
    let vports_v = doc.vports.as_ref().map(jsv::from_json);
    let panes = crate::vports::clean_panes(vports_v.as_ref());
    if panes.is_some() || (doc.view_twist != 0.0 && !doc.view_twist.is_nan()) {
        let mut b = empty_bounds();
        for e in &model {
            let eb = entity_bounds(e, Some(&doc.blocks));
            if valid_bounds(&eb) {
                b.min_x = js::min(b.min_x, eb.min_x);
                b.min_y = js::min(b.min_y, eb.min_y);
                b.max_x = js::max(b.max_x, eb.max_x);
                b.max_y = js::max(b.max_y, eb.max_y);
            }
        }
        let fb = if valid_bounds(&b) {
            ((b.min_x + b.max_x) / 2.0, (b.min_y + b.max_y) / 2.0, js::max_of([b.max_y - b.min_y, (b.max_x - b.min_x) / 1.6, 1e-6]) * 1.05)
        } else {
            (0.0, 0.0, 10.0)
        };
        let recs = crate::vports::vport_records(vports_v.as_ref(), Some(fb), doc.view_twist);
        w.w(0, "TABLE");
        w.w(2, "VPORT");
        w.wn(70, jsv::us2f(recs.len()));
        for r in &recs {
            w.w(0, "VPORT");
            w.w(2, "*ACTIVE");
            w.wn(70, 0.0);
            w.w(10, ff(r.x0));
            w.w(20, ff(r.y0));
            w.w(11, ff(r.x1));
            w.w(21, ff(r.y1));
            w.w(12, r.cx.map_or_else(|| "0.0".into(), ff));
            w.w(22, r.cy.map_or_else(|| "0.0".into(), ff));
            for (c, v) in [
                (13, "0.0"),
                (23, "0.0"),
                (14, "1.0"),
                (24, "1.0"),
                (15, "0.0"),
                (25, "0.0"),
                (16, "0.0"),
                (26, "0.0"),
                (36, "1.0"),
                (17, "0.0"),
                (27, "0.0"),
                (37, "0.0"),
            ] {
                w.w(c, v);
            }
            w.w(40, r.h.map_or_else(|| "0.0".into(), ff));
            w.w(41, ff(r.aspect));
            w.w(42, "50.0");
            w.w(43, "0.0");
            w.w(44, "0.0");
            w.w(50, "0.0");
            w.w(51, ff(deg(js::or(Some(r.tw), 0.0))));
            for (c, v) in [(71, 0.0), (72, 100.0), (73, 1.0), (74, 3.0), (75, 0.0), (76, 0.0), (77, 0.0), (78, 0.0)] {
                w.wn(c, v);
            }
            if let Some(s) = r.sheet.as_ref().filter(|s| !s.is_empty()) {
                w.w(1001, "CADCRAFT");
                w.w(1000, encode_text(&format!("PANESHEET:{}", slice16(s, 0, 245))));
            }
        }
        w.w(0, "ENDTAB");
    }
    // linetypes (0.4 C8): every one a layer or an object uses
    let mut layer_lt: HashMap<String, String> = HashMap::new();
    for l in doc.layers.values() {
        let lt = l.linetype.as_ref().map(|s| V::Str(s.clone()));
        let n = w.lt_name(lt.as_ref()).unwrap_or_else(|| "CONTINUOUS".into());
        layer_lt.insert(l.name.clone(), n);
    }
    let every: Vec<&Entity> = model
        .iter()
        .copied()
        .chain(layouts.iter().flat_map(|(_, _, l)| l.iter().copied()))
        .chain(doc.blocks.values().flat_map(|b| b.entities.iter()))
        .collect();
    for e in &every {
        let lt = match &e.lt {
            Some(Some(s)) => Some(V::Str(s.clone())),
            Some(None) => Some(V::Null),
            None => None,
        };
        w.lt_name(lt.as_ref());
        if let Some(lw) = &e.lw
            && lw.is_none_or(|x| x != -3.0)
        {
            w.note("lw", 1.0);
        }
        if let Some(lts) = e.extra.get("lts").and_then(serde_json::Value::as_f64)
            && lts > 0.0
            && lts != 1.0
        {
            w.note("lts", 1.0);
        }
    }
    for l in doc.layers.values() {
        if l.lineweight.is_some_and(|x| x >= 0.0) {
            w.note("lw", 1.0);
        }
    }
    w.w(0, "TABLE");
    w.w(2, "LTYPE");
    w.wn(70, 1.0 + jsv::us2f(w.lt_used.len()));
    w.w(0, "LTYPE");
    w.w(2, "CONTINUOUS");
    w.wn(70, 0.0);
    w.w(3, cat.t("dxf.solid_line", &[]));
    w.wn(72, 65.0);
    w.wn(73, 0.0);
    w.w(40, "0.0");
    for (n, pat) in w.lt_used.clone() {
        w.w(0, "LTYPE");
        w.w(2, n.clone());
        w.wn(70, 0.0);
        let desc = doc.linetypes.get(&n).map(|l| l.description.clone()).filter(|d| !d.is_empty()).unwrap_or_else(|| n.clone());
        w.w(3, desc);
        w.wn(72, 65.0);
        w.wn(73, jsv::us2f(pat.len()));
        w.w(40, ff(pat.iter().fold(0.0, |a, v| a + v.abs())));
        for v in &pat {
            w.w(49, ff(*v));
        }
    }
    w.w(0, "ENDTAB");
    w.w(0, "TABLE");
    w.w(2, "LAYER");
    w.wn(70, jsv::us2f(doc.layers.len()) + if add3d_layer { 1.0 } else { 0.0 });
    for l in doc.layers.values() {
        let mut c = match &l.color {
            cc_cad::Color::Str(s) => {
                w.note("truecolor", 1.0);
                f64::from(cc_cad::nearest_aci(s))
            }
            cc_cad::Color::Num(n) if *n != 0.0 && !n.is_nan() => *n,
            _ => 7.0,
        };
        if !l.on {
            c = -c.abs();
        }
        w.w(0, "LAYER");
        let n = w.r12(&l.name);
        w.w(2, n);
        let fl = (if l.frozen { 1 } else { 0 }) | (if l.new_vp_frozen.unwrap_or(false) { 2 } else { 0 }) | (if l.locked { 4 } else { 0 });
        w.wn(70, f64::from(fl));
        w.wn(62, c);
        w.w(6, layer_lt.get(&l.name).cloned().unwrap_or_default());
        if !l.plot {
            w.note("noplot", 1.0);
        }
    }
    if let Some(m3) = m3.as_ref().filter(|_| add3d_layer) {
        w.w(0, "LAYER");
        let n = w.r12(m3.layer);
        w.w(2, n);
        w.wn(70, 0.0);
        w.wn(62, 8.0);
        w.w(6, "CONTINUOUS");
    }
    w.w(0, "ENDTAB");
    // text styles (P.7): STANDARD, every one the drawing keeps and every name a text uses
    let mut text_style_out: Vec<(String, V)> = Vec::new();
    let put_style = |list: &mut Vec<(String, V)>, n: String, v: V| match list.iter_mut().find(|(k, _)| *k == n) {
        Some(slot) => slot.1 = v,
        None => list.push((n, v)),
    };
    for (n, v) in doc.text_styles.iter() {
        if !n.eq_ignore_ascii_case("standard") {
            let r = w.r12(n);
            put_style(&mut text_style_out, r, jsv::from_json(&serde_json::Value::Object(v.clone())));
        }
    }
    for e in &every {
        if e.typ() != "text" {
            continue;
        }
        let Some(s) = e.extra.get("style").filter(|s| cc_cad::js::truthy(Some(s))) else { continue };
        let s = cc_cad::js::to_js_string(s);
        if s.eq_ignore_ascii_case("standard") {
            continue;
        }
        let r = w.r12(&s);
        if !text_style_out.iter().any(|(k, _)| *k == r) {
            text_style_out.push((r, V::Obj(cc_io_dwg::obj! { "font" => "txt", "h" => 0.0, "wf" => 1.0, "obl" => 0.0 })));
        }
    }
    w.w(0, "TABLE");
    w.w(2, "STYLE");
    w.wn(70, 1.0 + jsv::us2f(text_style_out.len()));
    w.w(0, "STYLE");
    w.w(2, "STANDARD");
    w.wn(70, 0.0);
    w.w(40, "0.0");
    w.w(41, "1.0");
    w.w(50, "0.0");
    w.wn(71, 0.0);
    w.w(42, ff(doc.current.text_height));
    w.w(3, "txt");
    w.w(4, "");
    for (n, v) in &text_style_out {
        let g = |k: &str| jsv::get(Some(v), k);
        w.w(0, "STYLE");
        w.w(2, n.clone());
        w.wn(70, 0.0);
        w.w(40, ff(jsv::or(g("h"), 0.0)));
        w.w(41, ff(jsv::or(g("wf"), 1.0)));
        w.w(50, ff(deg(jsv::or(g("obl"), 0.0))));
        w.wn(71, 0.0);
        w.w(42, ff(doc.current.text_height));
        let font = if jsv::truthy(g("font")) { jsv::to_str(g("font")) } else { "txt".into() };
        w.w(3, encode_text(&font));
        w.w(4, "");
    }
    w.w(0, "ENDTAB");
    // named UCSs (DR.8)
    let ucs_named = ucs_named(ucs_v.as_ref());
    if !ucs_named.is_empty() {
        w.w(0, "TABLE");
        w.w(2, "UCS");
        w.wn(70, jsv::us2f(ucs_named.len()));
        for (name, (x, y, a)) in &ucs_named {
            let a = js::or(Some(*a), 0.0);
            w.w(0, "UCS");
            let n = w.r12(name);
            w.w(2, n);
            w.wn(70, 0.0);
            w.w(10, ff(*x));
            w.w(20, ff(*y));
            w.w(30, "0.0");
            w.w(11, ff(a.jcos()));
            w.w(21, ff(a.jsin()));
            w.w(31, "0.0");
            w.w(12, ff(-a.jsin()));
            w.w(22, ff(a.jcos()));
            w.w(32, "0.0");
        }
        w.w(0, "ENDTAB");
    }
    // named views (a sheet's one as a paper-space view: R12 does not say which sheet)
    let views: Vec<&cc_cad::View> = doc.views.iter().filter(|v| !v.name.is_empty() && v.w > 0.0 && v.h > 0.0).collect();
    if !views.is_empty() {
        w.w(0, "TABLE");
        w.w(2, "VIEW");
        w.wn(70, jsv::us2f(views.len()));
        let mut vn: HashSet<String> = HashSet::new();
        for v in &views {
            let mut n = w.r12(&v.name);
            let mut k = 2u64;
            while vn.contains(&n) {
                let base: String = w.r12(&v.name).chars().take(28).collect();
                n = format!("{base}_{k}");
                k = k.saturating_add(1);
            }
            vn.insert(n.clone());
            if v.layout.is_some() {
                w.note("paperView", 1.0);
            }
            w.w(0, "VIEW");
            w.w(2, n);
            w.wn(70, if v.layout.is_some() { 1.0 } else { 0.0 });
            w.w(40, ff(v.h));
            w.w(10, ff(v.cx));
            w.w(20, ff(v.cy));
            w.w(41, ff(v.w));
            for (c, s) in [
                (11, "0.0"),
                (21, "0.0"),
                (31, "1.0"),
                (12, "0.0"),
                (22, "0.0"),
                (32, "0.0"),
                (42, "50.0"),
                (43, "0.0"),
                (44, "0.0"),
                (50, "0.0"),
            ] {
                w.w(c, s);
            }
            w.wn(71, 0.0);
        }
        w.w(0, "ENDTAB");
    }
    let mut app_names: Vec<String> = vec!["ACAD".into(), "CADCRAFT".into()];
    for e in &every {
        if let V::Obj(o) = ev(e) {
            for a in jsv::keys_js(&props::props_xdata(&o, true)) {
                let up = a.to_uppercase();
                if !app_names.contains(&up) {
                    app_names.push(up);
                }
            }
        }
    }
    w.w(0, "TABLE");
    w.w(2, "APPID");
    w.wn(70, jsv::us2f(app_names.len()));
    for a in &app_names {
        w.w(0, "APPID");
        w.w(2, a.clone());
        w.wn(70, 0.0);
    }
    w.w(0, "ENDTAB");
    // dimension styles (P.6): STANDARD (the drawing's current settings) and every named one, with R12's variables
    let mut dim_style_out: Vec<(String, Obj)> = vec![("STANDARD".into(), st.clone())];
    let put = |list: &mut Vec<(String, Obj)>, n: String, v: Obj| match list.iter_mut().find(|(k, _)| *k == n) {
        Some(slot) => slot.1 = v,
        None => list.push((n, v)),
    };
    for (n, s2) in doc.dim_styles.iter() {
        if !n.eq_ignore_ascii_case("standard") {
            let r = w.r12(n);
            let v = match jsv::from_json(&serde_json::Value::Object(s2.clone())) {
                V::Obj(o) => o,
                _ => Obj::new(),
            };
            put(&mut dim_style_out, r, v);
        }
    }
    for e in &every {
        if e.typ() != "dim" {
            continue;
        }
        let Some(ds) = e.extra.get("dimstyle").filter(|s| cc_cad::js::truthy(Some(s))) else { continue };
        let ds = cc_cad::js::to_js_string(ds);
        if ds.eq_ignore_ascii_case("standard") {
            continue;
        }
        let r = w.r12(&ds);
        if !dim_style_out.iter().any(|(k, _)| *k == r) {
            dim_style_out.push((r, st.clone()));
        }
    }
    w.w(0, "TABLE");
    w.w(2, "DIMSTYLE");
    w.wn(70, jsv::us2f(dim_style_out.len()));
    const REAL_CODES: [f64; 14] = [40.0, 41.0, 42.0, 43.0, 44.0, 45.0, 47.0, 48.0, 140.0, 142.0, 143.0, 144.0, 146.0, 147.0];
    for (n, s2) in &dim_style_out {
        w.w(0, "DIMSTYLE");
        w.w(2, n.clone());
        w.wn(70, 0.0);
        for (code, v) in props::dimstyle_pairs(Some(s2), true) {
            let s = match &v {
                V::Str(s) => encode_text(s),
                V::Num(x) if x.fract() == 0.0 && x.is_finite() && !REAL_CODES.contains(&code) => js::num_to_string(*x),
                other => ff(jsv::to_num(Some(other))),
            };
            w.wn_code(code, s);
        }
        let asz = jsv::nnv(s2.get("asz"), st.get("asz").cloned().unwrap_or(V::Null));
        w.w(141, ff(jsv::or(Some(&asz), 2.5) / 2.0));
        let cc = props::cc_dim_items(Some(s2), true);
        if !cc.is_empty() {
            w.w(1001, "CADCRAFT");
            for v in cc {
                w.w(1000, encode_text(&v));
            }
        }
    }
    w.w(0, "ENDTAB");
    w.w(0, "ENDSEC");

    // BLOCKS
    w.w(0, "SECTION");
    w.w(2, "BLOCKS");
    for b in doc.blocks.values() {
        let Some(bn) = w.block_names.get(&b.name).cloned() else { continue };
        let root = b.xref.as_ref().is_some_and(|x| x.get("of").is_none());
        let link = if root && b.entities.is_empty() {
            let p = b.xref.as_ref().and_then(|x| x.get("path")).filter(|p| cc_cad::js::truthy(Some(p))).map(cc_cad::js::to_js_string);
            Some(p.unwrap_or_else(|| b.name.clone()))
        } else {
            None
        };
        if link.is_some() {
            w.note("xrefLink", 1.0);
        } else if root {
            w.note("xrefBound", 1.0);
        }
        let ents: Vec<(Entity, Option<Src>)> =
            b.entities.iter().enumerate().map(|(i, e)| (e.clone(), Some(Src::Block(b.name.clone(), i)))).collect();
        let (bx, by) = (V::Num(b.base.x), V::Num(b.base.y));
        w.block(&bn, (Some(&bx), Some(&by)), &ents, None, b.attdefs.as_deref(), link)?;
    }
    for (name, d) in std::mem::take(&mut dim_blocks) {
        let ents: Vec<(Entity, Option<Src>)> = match &d {
            DimBlock::Own(bn) => doc
                .blocks
                .get(bn)
                .map(|b| b.entities.iter().enumerate().map(|(i, e)| (e.clone(), Some(Src::Block(bn.clone(), i)))).collect())
                .unwrap_or_default(),
            DimBlock::Made(list) => list.iter().map(|e| (e.clone(), None)).collect(),
        };
        let z = V::Num(0.0);
        w.block(&name, (Some(&z), Some(&z)), &ents, None, None, None)?;
    }
    if let Some(m3) = &m3 {
        let z = V::Num(0.0);
        let pts = m3.points.clone();
        let mut extra = |wr: &mut W| -> R<()> {
            for strs in &pts {
                wr.w(0, "POINT");
                wr.w(8, "0");
                wr.w(10, "0.0");
                wr.w(20, "0.0");
                wr.w(30, "0.0");
                wr.w(1001, "CADCRAFT");
                for t in strs {
                    wr.w(1000, t.clone());
                }
            }
            Ok(())
        };
        w.block(crate::model3d::M3D_BLOCK, (Some(&z), Some(&z)), &[], Some(&mut extra), None, None)?;
    }
    for (k, name) in &extra_layouts {
        let Some((li, l, list)) = layouts.get(*k) else { continue };
        let p = jsv::from_json(&serde_json::Value::Object(l.paper.clone()));
        let ents: Vec<(Entity, Option<Src>)> = list.iter().enumerate().map(|(i, e)| ((*e).clone(), Some(Src::Layout(*li, i)))).collect();
        let z = V::Num(0.0);
        let lname = l.name.clone();
        let pc = p.clone();
        let mut extra = |wr: &mut W| -> R<()> {
            let g = |k: &str| jsv::get(Some(&pc), k);
            wr.w(0, "POINT");
            wr.w(8, "0");
            let s = fv(g("x0"))?;
            wr.w(10, s);
            let s = fv(g("y0"))?;
            wr.w(20, s);
            wr.w(30, "0.0");
            wr.layout_xdata(&lname, &pc)
        };
        w.block(name, (Some(&z), Some(&z)), &ents, Some(&mut extra), None, None)?;
    }
    w.w(0, "ENDSEC");

    // ENTITIES: the model, then the first layout in paper space
    w.w(0, "SECTION");
    w.w(2, "ENTITIES");
    for (i, e) in model.iter().enumerate() {
        w.entity(e, O { top: true, paper: false }, Some(&Src::Model(i)))?;
    }
    if let Some(m3) = &m3 {
        let ln = w.r12(m3.layer);
        for fc in &m3.faces {
            w.w(0, "3DFACE");
            w.w(8, ln.clone());
            for (k, p) in fc.pts.iter().enumerate() {
                let k = i64::try_from(k).unwrap_or(0);
                w.w(10i64.saturating_add(k), ff(p[0]));
                w.w(20i64.saturating_add(k), ff(p[1]));
                w.w(30i64.saturating_add(k), ff(p[2]));
            }
            if fc.invis != 0 {
                w.wn(70, f64::from(fc.invis));
            }
        }
    }
    if let Some((li, l, list)) = first {
        let p = jsv::from_json(&serde_json::Value::Object(l.paper.clone()));
        let pn = |k: &str| jsv::to_num(jsv::get(Some(&p), k));
        let cx = pn("x0") + pn("w") / 2.0;
        let cy = pn("y0") + pn("h") / 2.0;
        let mut vp = Obj::new();
        vp.set("layer", "0");
        vp.set("color", V::Null);
        vp.set("x", cx);
        vp.set("y", cy);
        vp.set_opt("w", jsv::get(Some(&p), "w").cloned());
        vp.set_opt("h", jsv::get(Some(&p), "h").cloned());
        vp.set("cx", cx);
        vp.set("cy", cy);
        vp.set("s", 1.0);
        w.viewport(&V::Obj(vp), 1.0, Some((&l.name, &p)))?;
        let mut id = 2.0;
        for (i, e) in list.iter().enumerate() {
            if e.typ() == "viewport" {
                w.viewport(&ev(e), id, None)?;
                id += 1.0;
            } else {
                w.entity(e, O { top: false, paper: true }, Some(&Src::Layout(*li, i)))?;
            }
        }
    }
    w.w(0, "ENDSEC");
    w.w(0, "EOF");

    // the report
    let mut notes: Vec<String> = Vec::new();
    let n_of = |w: &W, k: &str| i18n::n(w.count(k));
    if w.has_count("renamed") {
        notes.push(cat.t("dxf.save.renamed", &n_of(&w, "renamed")));
    }
    if w.has_count("infinite") {
        notes.push(cat.t("dxf.save.infinite", &n_of(&w, "infinite")));
    }
    let clip_n = model
        .iter()
        .copied()
        .chain(layouts.iter().flat_map(|(_, _, l)| l.iter().copied()))
        .filter(|e| {
            e.typ() == "insert"
                && e.extra.get("clip").and_then(|c| c.get("pts")).and_then(serde_json::Value::as_array).is_some_and(|a| a.len() >= 3)
        })
        .count();
    if clip_n > 0 {
        notes.push(cat.t("dxf.save.xclip", &i18n::n(jsv::us2f(clip_n))));
    }
    let vpovr = layouts
        .iter()
        .flat_map(|(_, _, l)| l.iter())
        .filter(|e| e.typ() == "viewport" && e.extra.get("lov").and_then(serde_json::Value::as_object).is_some_and(|o| !o.is_empty()))
        .count();
    if vpovr > 0 {
        notes.push(cat.t("dxf.save.vpovr", &i18n::n(jsv::us2f(vpovr))));
    }
    let pics = picture_refs(doc, &model, &layouts);
    if pics > 0 {
        notes.push(cat.t("dxf.save.pictures", &i18n::n(jsv::us2f(pics))));
    }
    if w.has_count("truecolor") {
        notes.push(cat.t("dxf.true_colours_were_saved", &[]));
    }
    for (k, key) in [("hatch", "dxf.save.hatch"), ("ellipse", "dxf.save.ellipse")] {
        if w.has_count(k) {
            notes.push(cat.t(key, &n_of(&w, k)));
        }
    }
    if w.has_count("mtext") {
        notes.push(cat.t("dxf.multi_line_text_was", &[]));
    }
    for (k, key) in [("textmask", "dxf.save.textmask"), ("longattr", "dxf.save.longattr")] {
        if w.has_count(k) {
            notes.push(cat.t(key, &n_of(&w, k)));
        }
    }
    if w.has_count("noplot") {
        let key = if first.is_some() { "dxf.save.noplot" } else { "dxf.save.noplot_nolayout" };
        notes.push(cat.t(key, &[("n", js::num_to_string(w.count("noplot"))), ("layers", w.no_plot.join(", "))]));
    }
    if !extra_layouts.is_empty() {
        let fname = first.map(|(_, l, _)| l.name.clone()).unwrap_or_default();
        let ls: Vec<String> = extra_layouts.iter().filter_map(|(k, _)| doc.layouts.get(*k)).map(|l| format!("\"{}\"", l.name)).collect();
        let bs: Vec<String> = extra_layouts.iter().map(|(_, n)| n.clone()).collect();
        notes.push(cat.t(
            "dxf.save.layouts",
            &[("n", jsv::us2f(extra_layouts.len()).to_string()), ("first", fname), ("layouts", ls.join(", ")), ("blocks", bs.join(", "))],
        ));
    }
    for (k, key) in [("mleader", "dxf.save.mleader"), ("xrefBound", "dwg.save.xref.bound"), ("xrefLink", "dwg.save.xref.link")] {
        if w.has_count(k) {
            notes.push(cat.t(key, &n_of(&w, k)));
        }
    }
    if w.has_count("lw") {
        notes.push(cat.t("dxf.save.lw", &[]));
    }
    for (k, key) in [("lts", "dxf.save.lts"), ("paperView", "dxf.save.paperview"), ("other", "dwg.save.other")] {
        if w.has_count(k) {
            notes.push(cat.t(key, &n_of(&w, k)));
        }
    }
    if w.has_count("tr") {
        notes.push(cat.t("dxf.save.tr", &[]));
    }
    if w.has_count("material") {
        notes.push(cat.t("dwg.save.material", &[]));
    }
    if let Some(m3) = &m3 {
        let n = jsv::us2f(m3.faces.len());
        w.counts.set("face3d", n);
        notes.push(cat.t(
            "dxf.save.model3d",
            &[("n", js::num_to_string(n)), ("layer", m3.layer.to_string()), ("block", crate::model3d::M3D_BLOCK.to_string())],
        ));
    }
    let mut report = Obj::new();
    report.set("notes", V::Arr(notes.into_iter().map(V::Str).collect()));
    report.set("counts", std::mem::take(&mut w.counts));
    report.set("extents", V::Obj(cc_io_dwg::obj! { "minX" => w.ext[0], "minY" => w.ext[1], "maxX" => w.ext[2], "maxY" => w.ext[3] }));
    let mut text = w.lines.join("\r\n");
    text.push_str("\r\n");
    Ok((text, jsv::to_json(&V::Obj(report))))
}

impl W<'_> {
    fn wn_code(&mut self, code: f64, s: String) {
        self.lines.push(format!("{:>3}", js::num_to_string(code)));
        self.lines.push(s);
    }
}

/// dxf.js `unitCode(u)`: a units name → $INSUNITS (0 for none).
fn unit_code(u: Ov) -> i64 {
    match jsv::to_str(u).as_str() {
        "inches" => 1,
        "feet" => 2,
        "miles" => 3,
        "millimeters" => 4,
        "centimeters" => 5,
        "meters" => 6,
        "kilometers" => 7,
        "microinches" => 8,
        "mils" => 9,
        "yards" => 10,
        _ => 0,
    }
}

/// pictures.js `pictureRefs(doc)`: references to picture blocks in the model and on sheets.
fn picture_refs(doc: &Doc, model: &[&Entity], layouts: &[(usize, &cc_cad::Layout, Vec<&Entity>)]) -> usize {
    let pic: HashSet<&str> = doc
        .blocks
        .values()
        .filter(|b| b.entities.iter().any(|e| e.typ() == "poly" && e.extra.get("pic").is_some_and(|p| cc_cad::js::truthy(Some(p)))))
        .map(|b| b.name.as_str())
        .collect();
    if pic.is_empty() {
        return 0;
    }
    model
        .iter()
        .copied()
        .chain(layouts.iter().flat_map(|(_, _, l)| l.iter().copied()))
        .filter(|e| matches!(&e.geo, Geo::Insert(i) if pic.contains(i.block.as_str())))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numbers_and_tails() {
        assert_eq!(ff(0.0), "0.0");
        assert_eq!(ff(1e-13), "0.0");
        assert_eq!(ff(0.1 + 0.2), "0.3");
        assert_eq!(ff(1.0), "1");
        assert_eq!(ff(-12.5), "-12.5");
        assert_eq!(ff(1e21), "1e+21");
        assert_eq!(trim_escape_tail("abc\\U+00"), "abc");
        assert_eq!(trim_escape_tail("abc%"), "abc");
        assert_eq!(trim_escape_tail("abc\\"), "abc");
        assert_eq!(trim_escape_tail("abc"), "abc");
    }
}
