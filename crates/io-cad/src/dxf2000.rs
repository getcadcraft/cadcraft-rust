//! dxf2000.js: a CadCraft drawing → ASCII DXF R2000 (AC1015), written from the model DWG saving builds (dwg_save.rs
//! with `model_only` + `explode_leaders`), so a DXF 2000 keeps what a DWG keeps. Every object has a handle and its owner.

use std::collections::HashMap;

use cc_cad::Doc;
use cc_cad::entity::Geo;
use cc_cad::js;
use cc_io_dwg::value::{Obj, V};

use crate::dwg_save::{SaveOpts, build_model};
use crate::i18n::{self, Catalog};
use crate::jsv::{self, JsErr, Ov};
use crate::props;

fn deg(r: f64) -> f64 {
    (r * 180.0) / std::f64::consts::PI
}
/// `f(n)` for a number.
fn fnum(n: f64) -> String {
    if n.is_finite() { crate::dxf_write::ff(n) } else { "0.0".into() }
}
/// `f(v)` for any value (`Number.isFinite`: only numbers count).
fn fo(v: Ov) -> String {
    match v {
        Some(V::Num(n)) => fnum(*n),
        _ => "0.0".into(),
    }
}
/// `deg(v)` with JS's coercion.
fn degv(v: Ov) -> f64 {
    deg(jsv::to_num(v))
}
/// `v || d` → f
fn fo_or(v: Ov, d: f64) -> String {
    if jsv::truthy(v) { fo(v) } else { fnum(d) }
}
/// `enc(s)`: ASCII as is, CR/LF runs as one space, every other UTF-16 unit as \U+XXXX.
fn enc(v: Ov) -> String {
    let s = if jsv::nullish(v) { String::new() } else { jsv::to_str(v) };
    let mut out = String::with_capacity(s.len());
    let mut in_nl = false;
    for u in s.encode_utf16() {
        if u == 13 || u == 10 {
            if !in_nl {
                out.push(' ');
            }
            in_nl = true;
            continue;
        }
        in_nl = false;
        if (0x20..=0x7e).contains(&u) {
            out.push(char::from(u8::try_from(u).unwrap_or(b'?')));
        } else {
            out.push_str(&format!("\\U+{u:04X}"));
        }
    }
    out
}
fn enc_s(s: &str) -> String {
    enc(Some(&V::from(s)))
}
fn is_standard(v: Ov) -> bool {
    jsv::to_str(v).eq_ignore_ascii_case("standard")
}
/// `String(v)`
fn sv(v: Ov) -> String {
    jsv::to_str(v)
}
fn int32s(v: Ov) -> String {
    js::num_to_string(js::int32(jsv::to_num(v)))
}
fn block_name(n: Ov) -> String {
    sv(n).replace('|', "$0$")
}
fn layer_name(n: Ov) -> String {
    let s = if jsv::truthy(n) { sv(n) } else { "0".into() };
    s.replace('|', "$0$")
}
fn hex(n: u64) -> String {
    format!("{n:X}")
}

#[derive(Clone, Default)]
struct Rec {
    br: String,
    block: String,
    endblk: String,
    layout: Option<String>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Ms,
    Space,
    Plain,
}
struct Clip {
    own: String,
    xd: String,
    d2: String,
    sf: String,
    pts: Vec<V>,
}

struct W {
    /// the file so far: every line followed by CR LF (the JS joins an array of lines)
    out: String,
    seed: u64,
    lt_names: Vec<String>,
    layer_h: HashMap<String, String>,
    clips: Vec<Clip>,
    skipped: f64,
}

impl W {
    fn w(&mut self, code: i64, v: impl AsRef<str>) {
        self.code(code);
        self.out.push_str(v.as_ref());
        self.out.push_str("\r\n");
    }
    /// a group code line, right-aligned in 3 columns
    fn code(&mut self, code: i64) {
        use std::fmt::Write as _;
        let _ = write!(self.out, "{code:>3}\r\n");
    }
    /// a code given as text (xdata: `String(code)`, padded like `${code}`.padStart(3))
    fn w_str_code(&mut self, cs: &str, v: &str) {
        use std::fmt::Write as _;
        let _ = write!(self.out, "{cs:>3}\r\n");
        self.out.push_str(v);
        self.out.push_str("\r\n");
    }
    /// `f(n)` written straight into the file
    fn wf(&mut self, code: i64, n: f64) {
        self.code(code);
        if n.is_finite() {
            crate::dxf_write::push_ff(&mut self.out, n);
        } else {
            self.out.push_str("0.0");
        }
        self.out.push_str("\r\n");
    }
    /// `f(v)`
    fn wfo(&mut self, code: i64, v: Ov) {
        match v {
            Some(V::Num(n)) => self.wf(code, *n),
            _ => self.w(code, "0.0"),
        }
    }
    /// `f(v || d)`
    fn wfo_or(&mut self, code: i64, v: Ov, d: f64) {
        if jsv::truthy(v) { self.wfo(code, v) } else { self.wf(code, d) }
    }
    fn wn(&mut self, code: i64, v: f64) {
        self.w(code, js::num_to_string(v));
    }
    fn wv(&mut self, code: i64, v: Ov) {
        self.w(code, sv(v));
    }
    fn h(&mut self) -> String {
        let s = hex(self.seed);
        self.seed = self.seed.saturating_add(1);
        s
    }
    fn lt_name(&self, n: Ov) -> Option<String> {
        if !jsv::truthy(n) {
            return None;
        }
        let s = sv(n);
        if s.eq_ignore_ascii_case("bylayer") {
            None
        } else if s.eq_ignore_ascii_case("byblock") {
            Some("ByBlock".into())
        } else if s.eq_ignore_ascii_case("continuous") {
            Some("Continuous".into())
        } else if self.lt_names.contains(&jsv::upper(&s)) {
            Some(s)
        } else {
            None
        }
    }
    fn table(&mut self, name: &str, th: &str, n: usize) {
        self.w(0, "TABLE");
        self.w(2, name);
        self.w(5, th);
        self.w(330, "0");
        self.w(100, "AcDbSymbolTable");
        self.wn(70, jsv::us2f(n));
    }
    fn entry(&mut self, typ: &str, eh: &str, th: &str, sub: &str, name: &str, flags: f64) {
        self.w(0, typ);
        self.w(if typ == "DIMSTYLE" { 105 } else { 5 }, eh);
        self.w(330, th);
        self.w(100, "AcDbSymbolTableRecord");
        self.w(100, sub);
        self.w(2, name);
        self.wn(70, flags);
    }
    fn p(&mut self, c: i64, p: Ov) {
        let g = |k: &str| jsv::get(p, k);
        self.wfo_or(c, g("x"), 0.0);
        self.wfo_or(c.saturating_add(10), g("y"), 0.0);
        self.wfo_or(c.saturating_add(20), g("z"), 0.0);
    }
    fn head(&mut self, typ: &str, e: &V, owner: &str, paper: bool) {
        let g = |k: &str| jsv::get(Some(e), k);
        let own = self.h();
        self.w(0, typ);
        self.w(5, own.clone());
        let pts = jsv::get(g("xclip"), "pts");
        if matches!(pts, Some(V::Arr(a)) if a.len() >= 3) {
            let (xd, d2, sf) = (self.h(), self.h(), self.h());
            self.clips.push(Clip { own, xd: xd.clone(), d2, sf, pts: jsv::arr(pts).to_vec() });
            self.w(102, "{ACAD_XDICTIONARY");
            self.w(360, xd);
            self.w(102, "}");
        }
        self.w(330, owner);
        self.w(100, "AcDbEntity");
        if paper {
            self.w(67, "1");
        }
        let ln = layer_name(Some(&jsv::orv(g("layer"), V::from("0"))));
        self.w(8, enc_s(&ln));
        if let Some(lt) = self.lt_name(g("linetype")) {
            self.w(6, lt);
        }
        let c = g("color");
        if c.is_some_and(V::truthy) {
            let idx = jsv::get(c, "index");
            if idx.is_some() && !props::strict_eq(idx, Some(&V::Num(256.0))) {
                self.wv(62, idx);
            }
            let rgb = jsv::get(c, "rgb");
            if rgb.is_some() {
                self.wn(420, jsv::i2f(jsv::i32_of(jsv::to_num(rgb)) & 0xff_ffff));
            }
        }
        if let Some(lw) = jsv::fin(g("lineweight"))
            && lw != -1.0
        {
            self.wn(370, lw);
        }
        let lts = g("ltscale");
        if jsv::to_num(lts) > 0.0 && !props::strict_eq(lts, Some(&V::Num(1.0))) {
            self.wfo(48, lts);
        }
        if jsv::fin(g("transparency")).is_some() {
            self.w(440, int32s(g("transparency")));
        }
    }
    fn th(&mut self, e: &V) {
        let t = jsv::get(Some(e), "thickness");
        if jsv::truthy(t) {
            self.wfo(39, t);
        }
    }
    fn xdata(&mut self, e: &V) {
        let Some(x) = jsv::obj(jsv::get(Some(e), "xdata")) else { return };
        for app in jsv::keys_js(x) {
            self.w(1001, jsv::upper(app));
            for it in jsv::arr(x.get(app)) {
                let code = jsv::at(Some(it), 0);
                let v = jsv::at(Some(it), 1);
                let cs = sv(code);
                let c = jsv::num(code);
                let out = match c {
                    Some(1000.0) => enc(v),
                    Some(1040.0) => fnum(jsv::to_num(v)),
                    Some(1070.0) => int32s(v),
                    _ => sv(v),
                };
                self.w_str_code(&cs, &out);
            }
        }
    }
    fn text_body(&mut self, e: &V) {
        let g = |k: &str| jsv::get(Some(e), k);
        self.th(e);
        self.p(10, g("insert"));
        self.wfo(40, g("height"));
        self.w(1, enc(g("value")));
        if jsv::truthy(g("rotation")) {
            self.wf(50, degv(g("rotation")));
        }
        if jsv::truthy(g("widthFactor")) && !props::strict_eq(g("widthFactor"), Some(&V::Num(1.0))) {
            self.wfo(41, g("widthFactor"));
        }
        if jsv::truthy(g("oblique")) {
            self.wf(51, degv(g("oblique")));
        }
        self.w(7, if jsv::truthy(g("style")) { enc(g("style")) } else { "Standard".into() });
        if jsv::truthy(g("hAlign")) {
            self.wv(72, g("hAlign"));
        }
        if jsv::truthy(g("align")) && (jsv::truthy(g("hAlign")) || jsv::truthy(g("vAlign"))) {
            self.p(11, g("align"));
        }
    }

    #[allow(clippy::too_many_lines)]
    fn entity(&mut self, e: &V, owner: &str, paper: bool) {
        let g = |k: &str| jsv::get(Some(e), k);
        let typ = sv(g("type"));
        match typ.as_str() {
            "LINE" => {
                self.head("LINE", e, owner, paper);
                self.w(100, "AcDbLine");
                self.th(e);
                self.p(10, g("start"));
                self.p(11, g("end"));
            }
            "CIRCLE" | "ARC" => {
                self.head(&typ, e, owner, paper);
                self.w(100, "AcDbCircle");
                self.th(e);
                self.p(10, g("center"));
                self.wfo(40, g("radius"));
                if typ == "ARC" {
                    self.w(100, "AcDbArc");
                    self.wf(50, degv(g("startAngle")));
                    self.wf(51, degv(g("endAngle")));
                }
            }
            "POINT" => {
                self.head("POINT", e, owner, paper);
                self.w(100, "AcDbPoint");
                self.p(10, g("position"));
                self.th(e);
            }
            "XLINE" | "RAY" => {
                self.head(&typ, e, owner, paper);
                self.w(100, if typ == "XLINE" { "AcDbXline" } else { "AcDbRay" });
                self.p(10, g("start"));
                self.p(11, g("direction"));
            }
            "LWPOLYLINE" => {
                let vs = jsv::arr(g("vertices"));
                let each = vs.iter().any(|v| jsv::truthy(jsv::get(Some(v), "startWidth")) || jsv::truthy(jsv::get(Some(v), "endWidth")));
                self.head("LWPOLYLINE", e, owner, paper);
                self.w(100, "AcDbPolyline");
                self.wn(90, jsv::us2f(vs.len()));
                self.w(70, if jsv::truthy(g("closed")) { "1" } else { "0" });
                self.w(43, if each { fnum(0.0) } else { fo_or(g("constantWidth"), 0.0) });
                if jsv::truthy(g("elevation")) {
                    self.wfo(38, g("elevation"));
                }
                self.th(e);
                for v in vs {
                    let gv = |k: &str| jsv::get(Some(v), k);
                    self.wfo(10, gv("x"));
                    self.wfo(20, gv("y"));
                    if each {
                        self.wfo_or(40, gv("startWidth"), 0.0);
                        self.wfo_or(41, gv("endWidth"), 0.0);
                    }
                    if jsv::truthy(gv("bulge")) {
                        self.wfo(42, gv("bulge"));
                    }
                }
            }
            "ELLIPSE" => {
                let maj0 = g("majorAxis").cloned();
                let ratio_v = g("ratio").cloned();
                let s0v = jsv::nnv(g("startParam"), V::Num(0.0));
                let s1v = jsv::nnv(g("endParam"), V::Num(2.0 * std::f64::consts::PI));
                let ratio = jsv::to_num(ratio_v.as_ref());
                let (maj, r_out, s0, s1) = if ratio > 1.0 {
                    let mx = jsv::to_num(jsv::get(maj0.as_ref(), "x"));
                    let my = jsv::to_num(jsv::get(maj0.as_ref(), "y"));
                    let h = std::f64::consts::PI / 2.0;
                    (
                        Some(cc_io_dwg::value::p3(-my * ratio, mx * ratio, 0.0)),
                        fnum(1.0 / ratio),
                        fnum(jsv::to_num(Some(&s0v)) - h),
                        fnum(jsv::to_num(Some(&s1v)) - h),
                    )
                } else {
                    (maj0, fo(ratio_v.as_ref()), fo(Some(&s0v)), fo(Some(&s1v)))
                };
                self.head("ELLIPSE", e, owner, paper);
                self.w(100, "AcDbEllipse");
                self.p(10, g("center"));
                self.p(11, maj.as_ref());
                self.w(40, r_out);
                self.w(41, s0);
                self.w(42, s1);
            }
            "TEXT" => {
                self.head("TEXT", e, owner, paper);
                self.w(100, "AcDbText");
                self.text_body(e);
                self.w(100, "AcDbText");
                if jsv::truthy(g("vAlign")) {
                    self.wv(73, g("vAlign"));
                }
            }
            "MTEXT" => {
                self.head("MTEXT", e, owner, paper);
                self.w(100, "AcDbMText");
                self.p(10, g("insert"));
                self.wfo(40, g("height"));
                self.wfo_or(41, g("width"), 0.0);
                self.w(71, if jsv::truthy(g("attach")) { sv(g("attach")) } else { "1".into() });
                self.w(72, "1");
                let v = enc(g("value"));
                let len = v.len();
                let mut i = 0usize;
                while i.saturating_add(250) < len {
                    self.w(3, v.get(i..i.saturating_add(250)).unwrap_or(""));
                    i = i.saturating_add(250);
                }
                let last = (len.saturating_sub(1) / 250).saturating_mul(250);
                self.w(1, v.get(last..).unwrap_or(""));
                self.w(7, if jsv::truthy(g("style")) { enc(g("style")) } else { "Standard".into() });
                if jsv::truthy(g("xAxis")) {
                    self.p(11, g("xAxis"));
                }
                self.wfo_or(44, g("lineSpacing"), 1.0);
                if jsv::i32_of(jsv::to_num(g("backgroundFlags"))) & 1 != 0 {
                    let dflt = V::Obj(cc_io_dwg::obj! { "index" => 7.0 });
                    let bc = if jsv::truthy(g("backgroundColor")) { g("backgroundColor").cloned().unwrap_or(V::Null) } else { dflt };
                    self.wv(90, g("backgroundFlags"));
                    let bi = jsv::nnv(jsv::get(Some(&bc), "index"), V::Num(7.0));
                    self.wv(63, Some(&bi));
                    let rgb = jsv::get(Some(&bc), "rgb");
                    if rgb.is_some() {
                        self.wn(421, jsv::i2f(jsv::i32_of(jsv::to_num(rgb)) & 0xff_ffff));
                    }
                    self.wfo_or(45, g("backgroundScale"), 1.5);
                    self.w(441, int32s(g("backgroundTransparency")));
                }
            }
            "ATTDEF" => {
                self.head("ATTDEF", e, owner, paper);
                self.w(100, "AcDbText");
                self.text_body(e);
                self.w(100, "AcDbAttributeDefinition");
                self.w(3, enc(g("prompt")));
                self.w(2, enc(g("tag")));
                self.w(70, int32s(g("flags")));
                if jsv::truthy(g("vAlign")) {
                    self.wv(74, g("vAlign"));
                }
            }
            "INSERT" => {
                self.head("INSERT", e, owner, paper);
                self.w(100, "AcDbBlockReference");
                let atts = jsv::arr(g("attribs"));
                if !atts.is_empty() {
                    self.w(66, "1");
                }
                self.w(2, enc_s(&block_name(g("block"))));
                self.p(10, g("insert"));
                let sc = g("scale");
                for (c, k) in [(41, "x"), (42, "y"), (43, "z")] {
                    let v = jsv::nnv(jsv::get(sc, k), V::Num(1.0));
                    self.wfo(c, Some(&v));
                }
                if jsv::truthy(g("rotation")) {
                    self.wf(50, degv(g("rotation")));
                }
                self.xdata(e);
                if !atts.is_empty() {
                    let ih = hex(self.seed.saturating_sub(1));
                    for a in atts {
                        let mut a2 = jsv::obj(Some(a)).cloned().unwrap_or_default();
                        let layer = if jsv::truthy(a2.get("layer")) { a2.get("layer").cloned() } else { g("layer").cloned() };
                        a2.set("layer", layer.unwrap_or(V::Null));
                        let a2 = V::Obj(a2);
                        self.head("ATTRIB", &a2, &ih, paper);
                        self.w(100, "AcDbText");
                        self.text_body(a);
                        self.w(100, "AcDbAttribute");
                        self.w(2, enc(jsv::get(Some(a), "tag")));
                        self.w(70, int32s(jsv::get(Some(a), "flags")));
                        if jsv::truthy(jsv::get(Some(a), "vAlign")) {
                            self.wv(74, jsv::get(Some(a), "vAlign"));
                        }
                    }
                    let mut s = Obj::new();
                    s.set("layer", g("layer").cloned().unwrap_or(V::Null));
                    self.head("SEQEND", &V::Obj(s), &ih, paper);
                }
                return;
            }
            "SOLID" | "3DFACE" => {
                self.head(&typ, e, owner, paper);
                self.w(100, if typ == "SOLID" { "AcDbTrace" } else { "AcDbFace" });
                for (i, p) in jsv::arr(g("points")).iter().enumerate() {
                    self.p(10i64.saturating_add(i64::try_from(i).unwrap_or(0)), Some(p));
                }
                if typ == "3DFACE" && jsv::truthy(g("invisibleEdges")) {
                    self.wv(70, g("invisibleEdges"));
                }
            }
            "HATCH" => {
                self.head("HATCH", e, owner, paper);
                self.w(100, "AcDbHatch");
                self.w(10, "0.0");
                self.w(20, "0.0");
                self.wfo_or(30, g("elevation"), 0.0);
                self.w(210, "0.0");
                self.w(220, "0.0");
                self.w(230, "1.0");
                let pat = jsv::orv(g("pattern"), V::from("SOLID"));
                self.w(2, enc(Some(&pat)));
                self.w(70, if jsv::truthy(g("solid")) { "1" } else { "0" });
                self.w(71, "0");
                let loops = jsv::arr(g("loops"));
                self.wn(91, jsv::us2f(loops.len()));
                for l in loops {
                    let v = jsv::arr(jsv::get(jsv::get(Some(l), "polyline"), "vertices"));
                    let bul = v.iter().any(|q| jsv::truthy(jsv::get(Some(q), "bulge")));
                    self.wn(92, jsv::i2f(jsv::i32_of(jsv::to_num(jsv::get(Some(l), "flags"))) | 2));
                    self.w(72, if bul { "1" } else { "0" });
                    self.w(73, "1");
                    self.wn(93, jsv::us2f(v.len()));
                    for q in v {
                        self.wfo(10, jsv::get(Some(q), "x"));
                        self.wfo(20, jsv::get(Some(q), "y"));
                        if bul {
                            self.wfo_or(42, jsv::get(Some(q), "bulge"), 0.0);
                        }
                    }
                    self.w(97, "0");
                }
                self.w(75, int32s(g("style")));
                let pt = jsv::nnv(g("patternType"), V::Num(1.0));
                self.wv(76, Some(&pt));
                if !jsv::truthy(g("solid")) {
                    self.wf(52, deg(jsv::or(g("angle"), 0.0)));
                    self.wfo_or(41, g("scale"), 1.0);
                    self.w(77, "0");
                    let lines = jsv::arr(g("lines"));
                    self.wn(78, jsv::us2f(lines.len()));
                    for ln in lines {
                        let gl = |k: &str| jsv::get(Some(ln), k);
                        self.wf(53, degv(gl("angle")));
                        self.wfo(43, jsv::get(gl("base"), "x"));
                        self.wfo(44, jsv::get(gl("base"), "y"));
                        self.wfo(45, jsv::get(gl("offset"), "x"));
                        self.wfo(46, jsv::get(gl("offset"), "y"));
                        let d = jsv::arr(gl("dashes"));
                        self.wn(79, jsv::us2f(d.len()));
                        for x in d {
                            self.wfo(49, Some(x));
                        }
                    }
                }
                self.w(98, "0");
            }
            "DIMENSION" => {
                self.head("DIMENSION", e, owner, paper);
                self.w(100, "AcDbDimension");
                self.w(2, enc_s(&block_name(g("block"))));
                self.p(10, g("defPoint"));
                self.p(11, g("textMid"));
                let fl = jsv::nnv(g("flags"), g("dimType").cloned().unwrap_or(V::Null));
                self.wv(70, Some(&fl));
                if jsv::truthy(g("text")) {
                    self.w(1, enc(g("text")));
                }
                if jsv::truthy(g("textRotation")) {
                    self.wf(53, degv(g("textRotation")));
                }
                self.w(3, if jsv::truthy(g("style")) && !is_standard(g("style")) { enc(g("style")) } else { "Standard".into() });
                let k = jsv::i32_of(jsv::to_num(g("dimType"))) & 7;
                match k {
                    0 | 1 => {
                        self.w(100, "AcDbAlignedDimension");
                        self.p(13, g("xline1"));
                        self.p(14, g("xline2"));
                        if k == 0 {
                            self.wf(50, deg(jsv::or(g("angle"), 0.0)));
                            self.w(100, "AcDbRotatedDimension");
                        }
                    }
                    4 | 3 => {
                        self.w(100, if k == 4 { "AcDbRadialDimension" } else { "AcDbDiametricDimension" });
                        self.p(15, g("center"));
                        self.w(40, "0.0");
                    }
                    5 => {
                        self.w(100, "AcDb3PointAngularDimension");
                        self.p(13, g("xline1"));
                        self.p(14, g("xline2"));
                        self.p(15, g("center"));
                    }
                    _ => {}
                }
            }
            "VIEWPORT" => {
                let st = jsv::u32_of(jsv::to_num(Some(&jsv::nnv(g("status"), V::Num(32864.0)))));
                self.head("VIEWPORT", e, owner, true);
                self.w(100, "AcDbViewport");
                self.p(10, g("center"));
                self.wfo(40, g("width"));
                self.wfo(41, g("height"));
                let id1 = jsv::num(g("id")) == Some(1.0);
                self.w(
                    68,
                    if st & 131_072 != 0 {
                        "0"
                    } else if id1 {
                        "1"
                    } else {
                        "2"
                    },
                );
                let idv = jsv::orv(g("id"), V::Num(2.0));
                self.wv(69, Some(&idv));
                self.wfo_or(12, jsv::get(g("viewCenter"), "x"), 0.0);
                self.wfo_or(22, jsv::get(g("viewCenter"), "y"), 0.0);
                for (c, v) in [
                    (13, "0.0"),
                    (23, "0.0"),
                    (14, "10.0"),
                    (24, "10.0"),
                    (15, "10.0"),
                    (25, "10.0"),
                    (16, "0.0"),
                    (26, "0.0"),
                    (36, "1.0"),
                    (17, "0.0"),
                    (27, "0.0"),
                    (37, "0.0"),
                    (42, "50.0"),
                    (43, "0.0"),
                    (44, "0.0"),
                ] {
                    self.w(c, v);
                }
                self.wfo(45, g("viewHeight"));
                self.w(50, "0.0");
                self.wf(51, deg(jsv::or(g("twist"), 0.0)));
                self.w(72, "1000");
                for nm in jsv::arr(g("frozenLayers")) {
                    if let Some(lh) = self.layer_h.get(&jsv::upper(&sv(Some(nm)))).cloned() {
                        self.w(331, lh);
                    }
                }
                self.w(90, st.to_string());
                for (c, v) in [
                    (1, ""),
                    (281, "0"),
                    (71, "1"),
                    (74, "0"),
                    (110, "0.0"),
                    (120, "0.0"),
                    (130, "0.0"),
                    (111, "1.0"),
                    (121, "0.0"),
                    (131, "0.0"),
                    (112, "0.0"),
                    (122, "1.0"),
                    (132, "0.0"),
                    (79, "0"),
                    (146, "0.0"),
                ] {
                    self.w(c, v);
                }
            }
            _ => {
                self.skipped += 1.0;
                return;
            }
        }
        self.xdata(e);
    }
    fn dict(&mut self, dh: &str, owner: &str, items: &[(String, String)]) {
        self.w(0, "DICTIONARY");
        self.w(5, dh);
        self.w(330, owner);
        self.w(100, "AcDbDictionary");
        self.w(281, "1");
        for (k, v) in items {
            self.w(3, k.clone());
            self.w(350, v.clone());
        }
    }
}

struct Lay {
    name: String,
    tab: f64,
    space: usize,
    src: Option<Obj>,
    h: String,
}

/// `writeDxf2000(doc)` → (DXF R2000 text, report { notes, counts }).
pub fn write_dxf2000(doc: &Doc) -> Result<(String, serde_json::Value), JsErr> {
    write_dxf2000_with(doc, Catalog::current())
}

/// `writeDxf2000` with a catalog for the notes.
#[allow(clippy::too_many_lines)]
pub fn write_dxf2000_with(doc: &Doc, cat: &Catalog) -> Result<(String, serde_json::Value), JsErr> {
    let built = build_model(doc, &SaveOpts { model_only: true, explode_leaders: true, ..SaveOpts::default() }, cat)?;
    let model = V::Obj(built.model);
    let counts = built.counts;
    let tables = jsv::get(Some(&model), "tables");
    let tget = |k: &str| jsv::get(tables, k);
    let mut w =
        W { out: String::with_capacity(1 << 20), seed: 1, lt_names: Vec::new(), layer_h: HashMap::new(), clips: Vec::new(), skipped: 0.0 };
    let names = [
        "vportT", "ltypeT", "layerT", "styleT", "viewT", "ucsT", "appidT", "dimT", "brT", "vport", "byBlock", "byLayer", "cont", "style",
        "dim", "nod", "groups", "layoutsD", "mlsD", "mls",
    ];
    let mut hh: HashMap<&str, String> = HashMap::new();
    for nm in names {
        let h = w.h();
        hh.insert(nm, h);
    }
    let hk = |k: &str| hh.get(k).cloned().unwrap_or_default();

    // blocks: the model, the sheets (*Paper_Space…), the rest
    let blocks = jsv::arr(jsv::get(Some(&model), "blocks"));
    let name_of = |b: &V| sv(jsv::get(Some(b), "name"));
    let is_space = |n: &str| n.strip_prefix("*Paper_Space").is_some_and(|r| r.bytes().all(|c| c.is_ascii_digit()));
    let ms_i = blocks.iter().position(|b| name_of(b) == "*Model_Space");
    let placeholder = V::Obj(cc_io_dwg::obj! { "name" => "*Paper_Space", "entities" => V::Arr(Vec::new()) });
    let mut list: Vec<(&V, Kind)> = Vec::new();
    if let Some(i) = ms_i
        && let Some(b) = blocks.get(i)
    {
        list.push((b, Kind::Ms));
    }
    let spaces: Vec<&V> = blocks.iter().filter(|b| is_space(&name_of(b))).collect();
    if spaces.is_empty() {
        list.push((&placeholder, Kind::Space));
    } else {
        list.extend(spaces.into_iter().map(|b| (b, Kind::Space)));
    }
    list.extend(blocks.iter().enumerate().filter(|(i, b)| Some(*i) != ms_i && !is_space(&name_of(b))).map(|(_, b)| (b, Kind::Plain)));
    let mut rec: Vec<(String, Rec)> = Vec::new();
    for (b, _) in &list {
        let r = Rec { br: w.h(), block: w.h(), endblk: w.h(), layout: None };
        let n = name_of(b);
        match rec.iter_mut().find(|(x, _)| *x == n) {
            Some(slot) => slot.1 = r,
            None => rec.push((n, r)),
        }
    }
    let space_idx: Vec<usize> = list.iter().enumerate().filter(|(_, (_, k))| *k == Kind::Space).map(|(i, _)| i).collect();
    let first_space = space_idx.first().copied().unwrap_or(0);
    let ms_idx = list.iter().position(|(_, k)| *k == Kind::Ms);
    // layouts: Model + one per sheet; each sheet's paper space by its record key
    let paper_layouts: Vec<Obj> = jsv::arr(jsv::get(Some(&model), "layouts"))
        .iter()
        .filter(|l| !sv(jsv::get(Some(l), "name")).eq_ignore_ascii_case("model"))
        .filter_map(|l| jsv::obj(Some(l)).cloned())
        .collect();
    let mut layouts: Vec<Lay> = vec![Lay { name: "Model".into(), tab: 0.0, space: ms_idx.unwrap_or(0), src: None, h: String::new() }];
    for (i, l) in paper_layouts.iter().enumerate() {
        let by_record = l.get("block").and_then(|bk| {
            space_idx
                .iter()
                .rev()
                .copied()
                .find(|si| list.get(*si).is_some_and(|(b, _)| props::strict_eq(jsv::get(Some(b), "record"), Some(bk))))
        });
        let sp = by_record.or_else(|| space_idx.get(i).copied()).unwrap_or(first_space);
        layouts.push(Lay { name: sv(l.get("name")), tab: jsv::us2f(i) + 1.0, space: sp, src: Some(l.clone()), h: String::new() });
    }
    if layouts.len() == 1 {
        layouts.push(Lay { name: "Layout1".into(), tab: 1.0, space: first_space, src: None, h: String::new() });
    }
    for l in layouts.iter_mut() {
        l.h = w.h();
        let sn = list.get(l.space).map(|(b, _)| name_of(b)).unwrap_or_default();
        if let Some((_, r)) = rec.iter_mut().find(|(x, _)| *x == sn) {
            r.layout = Some(l.h.clone());
        }
    }
    let rec_of = |n: &str| rec.iter().find(|(x, _)| x == n).map(|(_, r)| r.clone()).unwrap_or_default();

    // names
    w.lt_names = vec!["BYLAYER".into(), "BYBLOCK".into(), "CONTINUOUS".into()];
    for l in jsv::arr(tget("linetypes")) {
        let u = jsv::upper(&sv(jsv::get(Some(l), "name")));
        if !w.lt_names.contains(&u) {
            w.lt_names.push(u);
        }
    }
    let mut apps: Vec<String> = vec!["ACAD".into(), "CADCRAFT".into()];
    fn collect(ents: &[V], apps: &mut Vec<String>) {
        for e in ents {
            if let Some(x) = jsv::obj(jsv::get(Some(e), "xdata")) {
                for k in jsv::keys_js(x) {
                    let u = jsv::upper(k);
                    if !apps.contains(&u) {
                        apps.push(u);
                    }
                }
            }
            collect(jsv::arr(jsv::get(Some(e), "attribs")), apps);
        }
    }
    for b in blocks {
        collect(jsv::arr(jsv::get(Some(b), "entities")), &mut apps);
    }

    // HEADER
    let hd = jsv::get(Some(&model), "header");
    let hg = |k: &str| jsv::get(hd, k);
    w.w(0, "SECTION");
    w.w(2, "HEADER");
    w.w(9, "$ACADVER");
    w.w(1, "AC1015");
    w.w(9, "$DWGCODEPAGE");
    w.w(3, "ANSI_1252");
    w.w(9, "$HANDSEED");
    w.code(5);
    let handseed_at = w.out.len();
    w.out.push_str("FFFF\r\n");
    w.w(9, "$INSBASE");
    w.wfo_or(10, jsv::get(hg("INSBASE"), "x"), 0.0);
    w.wfo_or(20, jsv::get(hg("INSBASE"), "y"), 0.0);
    w.w(30, "0.0");
    if jsv::truthy(hg("EXTMIN")) && jsv::truthy(hg("EXTMAX")) {
        w.w(9, "$EXTMIN");
        w.wfo(10, jsv::get(hg("EXTMIN"), "x"));
        w.wfo(20, jsv::get(hg("EXTMIN"), "y"));
        w.w(30, "0.0");
        w.w(9, "$EXTMAX");
        w.wfo(10, jsv::get(hg("EXTMAX"), "x"));
        w.wfo(20, jsv::get(hg("EXTMAX"), "y"));
        w.w(30, "0.0");
    }
    w.w(9, "$INSUNITS");
    w.w(70, int32s(hg("INSUNITS")));
    w.w(9, "$MEASUREMENT");
    w.wv(70, Some(&jsv::nnv(hg("MEASUREMENT"), V::Num(1.0))));
    w.w(9, "$LUNITS");
    w.wv(70, Some(&jsv::orv(hg("LUNITS"), V::Num(2.0))));
    if jsv::truthy(hg("UCSORG")) && jsv::truthy(hg("UCSXDIR")) {
        let xd = hg("UCSXDIR");
        let yd = if jsv::truthy(hg("UCSYDIR")) {
            hg("UCSYDIR").cloned().unwrap_or(V::Null)
        } else {
            jsv::p2(-jsv::to_num(jsv::get(xd, "y")), jsv::to_num(jsv::get(xd, "x")))
        };
        for (k, p) in [("$UCSORG", hg("UCSORG")), ("$UCSXDIR", xd), ("$UCSYDIR", Some(&yd))] {
            w.w(9, k);
            w.wfo(10, jsv::get(p, "x"));
            w.wfo(20, jsv::get(p, "y"));
            w.w(30, "0.0");
        }
    }
    if jsv::fin(hg("LUPREC")).is_some() {
        w.w(9, "$LUPREC");
        w.w(70, int32s(hg("LUPREC")));
    }
    w.w(9, "$LTSCALE");
    w.wfo_or(40, hg("LTSCALE"), 1.0);
    w.w(9, "$CLAYER");
    w.w(8, "0");
    w.w(9, "$TILEMODE");
    w.w(70, "1");
    w.w(0, "ENDSEC");
    w.w(0, "SECTION");
    w.w(2, "CLASSES");
    w.w(0, "ENDSEC");

    // TABLES
    w.w(0, "SECTION");
    w.w(2, "TABLES");
    let e0 = if jsv::truthy(hg("EXTMIN")) { hg("EXTMIN").cloned().unwrap_or(V::Null) } else { jsv::p2(0.0, 0.0) };
    let e1 = if jsv::truthy(hg("EXTMAX")) { hg("EXTMAX").cloned().unwrap_or(V::Null) } else { jsv::p2(12.0, 9.0) };
    let ex = |p: &V, k: &str| jsv::to_num(jsv::get(Some(p), k));
    let vh0 = js::max_of([ex(&e1, "y") - ex(&e0, "y"), (ex(&e1, "x") - ex(&e0, "x")) / 1.5, 1.0]) * 1.05;
    let actives: Vec<V> = match tget("vports") {
        Some(V::Arr(a)) if !a.is_empty() => a.clone(),
        _ => vec![V::Obj(cc_io_dwg::obj! { "lowerLeft" => jsv::p2(0.0, 0.0), "upperRight" => jsv::p2(1.0, 1.0), "aspect" => 1.5 })],
    };
    let (vport_t, vport_h) = (hk("vportT"), hk("vport"));
    w.table("VPORT", &vport_t, actives.len());
    for (k, v) in actives.iter().enumerate() {
        let gv = |n: &str| jsv::get(Some(v), n);
        let eh = if k > 0 { w.h() } else { vport_h.clone() };
        w.entry("VPORT", &eh, &vport_t, "AcDbViewportTableRecord", "*Active", 0.0);
        let c = if jsv::truthy(gv("center")) {
            gv("center").cloned().unwrap_or(V::Null)
        } else {
            jsv::p2((ex(&e0, "x") + ex(&e1, "x")) / 2.0, (ex(&e0, "y") + ex(&e1, "y")) / 2.0)
        };
        w.wfo(10, jsv::get(gv("lowerLeft"), "x"));
        w.wfo(20, jsv::get(gv("lowerLeft"), "y"));
        w.wfo(11, jsv::get(gv("upperRight"), "x"));
        w.wfo(21, jsv::get(gv("upperRight"), "y"));
        w.wfo(12, jsv::get(Some(&c), "x"));
        w.wfo(22, jsv::get(Some(&c), "y"));
        for (cd, s) in [
            (13, "0.0"),
            (23, "0.0"),
            (14, "1.0"),
            (24, "1.0"),
            (15, "1.0"),
            (25, "1.0"),
            (16, "0.0"),
            (26, "0.0"),
            (36, "1.0"),
            (17, "0.0"),
            (27, "0.0"),
            (37, "0.0"),
        ] {
            w.w(cd, s);
        }
        w.w(40, if jsv::to_num(gv("viewHeight")) > 0.0 { fo(gv("viewHeight")) } else { fnum(vh0) });
        w.w(41, if jsv::to_num(gv("aspect")) > 0.0 { fo(gv("aspect")) } else { fnum(1.5) });
        for (cd, s) in [(42, "50.0"), (43, "0.0"), (44, "0.0"), (50, "0.0")] {
            w.w(cd, s);
        }
        w.wf(51, deg(jsv::or(gv("twist"), 0.0)));
        for (cd, s) in [(71, "0"), (72, "1000"), (73, "1"), (74, "3"), (75, "0"), (76, "0"), (77, "0"), (78, "0")] {
            w.w(cd, s);
        }
        if let Some(x) = jsv::obj(gv("xdata")) {
            for app in jsv::keys_js(x) {
                w.w(1001, jsv::upper(app));
                for it in jsv::arr(x.get(app)) {
                    let code = jsv::at(Some(it), 0);
                    let val = jsv::at(Some(it), 1);
                    let out = if jsv::num(code) == Some(1000.0) { enc(val) } else { sv(val) };
                    let cs = sv(code);
                    w.w_str_code(&cs, &out);
                }
            }
        }
    }
    w.w(0, "ENDTAB");
    let lts = jsv::arr(tget("linetypes"));
    let ltype_t = hk("ltypeT");
    w.table("LTYPE", &ltype_t, lts.len().saturating_add(3));
    for (eh, nm, d) in [(hk("byBlock"), "ByBlock", ""), (hk("byLayer"), "ByLayer", ""), (hk("cont"), "Continuous", "Solid line")] {
        w.entry("LTYPE", &eh, &ltype_t, "AcDbLinetypeTableRecord", nm, 0.0);
        w.w(3, d);
        w.w(72, "65");
        w.w(73, "0");
        w.w(40, "0.0");
    }
    for l in lts {
        let gl = |k: &str| jsv::get(Some(l), k);
        let eh = w.h();
        w.entry("LTYPE", &eh, &ltype_t, "AcDbLinetypeTableRecord", &sv(gl("name")), 0.0);
        let d = if jsv::truthy(gl("description")) { gl("description") } else { gl("name") };
        w.w(3, enc(d));
        w.w(72, "65");
        let els = jsv::arr(gl("elements"));
        w.wn(73, jsv::us2f(els.len()));
        w.wfo(40, gl("patternLength"));
        for x in els {
            w.wfo(49, jsv::get(Some(x), "length"));
            w.w(74, "0");
        }
    }
    w.w(0, "ENDTAB");
    let ls = jsv::arr(tget("layers"));
    let has_zero = ls.iter().any(|l| jsv::s(jsv::get(Some(l), "name")) == Some("0"));
    let layer_t = hk("layerT");
    w.table("LAYER", &layer_t, ls.len().saturating_add(if has_zero { 0 } else { 1 }));
    if !has_zero {
        let eh = w.h();
        w.entry("LAYER", &eh, &layer_t, "AcDbLayerTableRecord", "0", 0.0);
        w.w(62, "7");
        w.w(6, "Continuous");
        w.w(370, "-3");
        w.w(390, "0");
    }
    for l in ls {
        let gl = |k: &str| jsv::get(Some(l), k);
        let lh = w.h();
        w.layer_h.insert(jsv::upper(&sv(gl("name"))), lh.clone());
        let flags = (if jsv::truthy(gl("frozen")) { 1 } else { 0 })
            | (if jsv::truthy(gl("frozenInNewViewports")) { 2 } else { 0 })
            | (if jsv::truthy(gl("locked")) { 4 } else { 0 });
        w.entry("LAYER", &lh, &layer_t, "AcDbLayerTableRecord", &enc_s(&layer_name(gl("name"))), f64::from(flags));
        let ci = jsv::nnv(jsv::get(gl("color"), "index"), V::Num(7.0));
        let ci2 = if ci.truthy() { ci } else { V::Num(7.0) };
        if jsv::truthy(gl("off")) {
            w.wn(62, -jsv::to_num(Some(&ci2)).abs());
        } else {
            w.wv(62, Some(&ci2));
        }
        let rgb = jsv::get(gl("color"), "rgb");
        if rgb.is_some() {
            w.wn(420, jsv::i2f(jsv::i32_of(jsv::to_num(rgb)) & 0xff_ffff));
        }
        let lt = w.lt_name(gl("linetype")).unwrap_or_else(|| "Continuous".into());
        w.w(6, lt);
        if matches!(gl("plot"), Some(V::Bool(false))) {
            w.w(290, "0");
        }
        match jsv::fin(gl("lineweight")) {
            Some(x) => w.wn(370, x),
            None => w.w(370, "-3"),
        }
        w.w(390, "0");
    }
    w.w(0, "ENDTAB");
    // text styles: Standard and every one the drawing keeps or an object names
    let styles_in: Vec<&V> = jsv::arr(tget("styles"))
        .iter()
        .filter(|x| jsv::truthy(jsv::get(Some(x), "name")) && !is_standard(jsv::get(Some(x), "name")))
        .collect();
    let mut style_h: Vec<(String, String)> = vec![("STANDARD".into(), hk("style"))];
    for x in &styles_in {
        let k = jsv::upper(&sv(jsv::get(Some(x), "name")));
        let h = w.h();
        match style_h.iter_mut().find(|(n, _)| *n == k) {
            Some(slot) => slot.1 = h,
            None => style_h.push((k, h)),
        }
    }
    let sh_of = |k: &str| style_h.iter().find(|(n, _)| n == k).map(|(_, h)| h.clone());
    let style_t = hk("styleT");
    w.table("STYLE", &style_t, styles_in.len().saturating_add(1));
    w.entry("STYLE", &hk("style"), &style_t, "AcDbTextStyleTableRecord", "Standard", 0.0);
    for (c, s) in [(40, "0.0"), (41, "1.0"), (50, "0.0"), (71, "0"), (42, "2.5"), (3, "txt"), (4, "")] {
        w.w(c, s);
    }
    for x in &styles_in {
        let gx = |k: &str| jsv::get(Some(x), k);
        let eh = sh_of(&jsv::upper(&sv(gx("name")))).unwrap_or_default();
        w.entry("STYLE", &eh, &style_t, "AcDbTextStyleTableRecord", &enc(gx("name")), 0.0);
        w.wfo_or(40, gx("height"), 0.0);
        w.wfo_or(41, gx("widthFactor"), 1.0);
        w.wf(50, deg(jsv::or(gx("oblique"), 0.0)));
        w.w(71, "0");
        w.w(42, "2.5");
        let font = jsv::orv(gx("font"), V::from("txt"));
        w.w(3, enc(Some(&font)));
        w.w(4, "");
    }
    w.w(0, "ENDTAB");
    let views = jsv::arr(tget("views"));
    let view_t = hk("viewT");
    w.table("VIEW", &view_t, views.len());
    for v in views {
        let gv = |k: &str| jsv::get(Some(v), k);
        let eh = w.h();
        w.entry("VIEW", &eh, &view_t, "AcDbViewTableRecord", &enc(gv("name")), if jsv::truthy(gv("paperSpace")) { 1.0 } else { 0.0 });
        w.wfo(40, gv("height"));
        w.wfo(10, jsv::get(gv("center"), "x"));
        w.wfo(20, jsv::get(gv("center"), "y"));
        w.wfo(41, gv("width"));
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
            (71, "0"),
            (281, "0"),
            (72, "0"),
        ] {
            w.w(c, s);
        }
    }
    w.w(0, "ENDTAB");
    let ucs_t: Vec<&V> = jsv::arr(tget("ucs"))
        .iter()
        .filter(|u| {
            u.truthy()
                && jsv::truthy(jsv::get(Some(u), "name"))
                && jsv::truthy(jsv::get(Some(u), "origin"))
                && jsv::truthy(jsv::get(Some(u), "xAxis"))
        })
        .collect();
    let ucs_th = hk("ucsT");
    w.table("UCS", &ucs_th, ucs_t.len());
    for u in ucs_t {
        let gu = |k: &str| jsv::get(Some(u), k);
        let eh = w.h();
        w.entry("UCS", &eh, &ucs_th, "AcDbUCSTableRecord", &enc(gu("name")), 0.0);
        let ya = if jsv::truthy(gu("yAxis")) {
            gu("yAxis").cloned().unwrap_or(V::Null)
        } else {
            jsv::p2(-jsv::to_num(jsv::get(gu("xAxis"), "y")), jsv::to_num(jsv::get(gu("xAxis"), "x")))
        };
        for (c, q) in [(10i64, gu("origin")), (11, gu("xAxis")), (12, Some(&ya))] {
            w.wfo_or(c, jsv::get(q, "x"), 0.0);
            w.wfo_or(c.saturating_add(10), jsv::get(q, "y"), 0.0);
            w.w(c.saturating_add(20), "0.0");
        }
        w.w(79, "0");
        w.w(146, "0.0");
    }
    w.w(0, "ENDTAB");
    let app_h: Vec<(String, String)> = apps.iter().map(|a| (a.clone(), w.h())).collect();
    let appid_t = hk("appidT");
    w.table("APPID", &appid_t, app_h.len());
    for (a, ah) in &app_h {
        w.entry("APPID", ah, &appid_t, "AcDbRegAppTableRecord", a, 0.0);
    }
    w.w(0, "ENDTAB");
    // dimension styles: every variable, one for one
    let dims_in: Vec<V> = match tget("dimstyles") {
        Some(V::Arr(a)) => a.clone(),
        _ => vec![V::Obj(cc_io_dwg::obj! { "name" => "Standard", "vars" => V::Obj(Obj::new()) })],
    };
    let dim_t = hk("dimT");
    w.table("DIMSTYLE", &dim_t, dims_in.len());
    for (k, d) in dims_in.iter().enumerate() {
        let gd = |n: &str| jsv::get(Some(d), n);
        let std = is_standard(gd("name"));
        let eh = if std && k == 0 { hk("dim") } else { w.h() };
        let nm = if std { "Standard".to_string() } else { enc(gd("name")) };
        w.entry("DIMSTYLE", &eh, &dim_t, "AcDbDimStyleTableRecord", &nm, 0.0);
        let vars = if jsv::truthy(gd("vars")) { gd("vars").cloned().unwrap_or(V::Null) } else { V::Obj(Obj::new()) };
        let mut st = Obj::new();
        props::st_from_vars(Some(&vars), &mut st);
        for (code, v) in props::dimstyle_pairs(Some(&st), false) {
            let out = match &v {
                V::Str(_) => enc(Some(&v)),
                V::Num(x) if x.is_finite() && x.fract() == 0.0 => js::num_to_string(*x),
                _ => fo(Some(&v)),
            };
            let cs = js::num_to_string(code);
            w.w_str_code(&cs, &out);
        }
        let tx = jsv::get(Some(&vars), "DIMTXSTY");
        let tk = jsv::upper(&if jsv::truthy(tx) { sv(tx) } else { "Standard".into() });
        let th = sh_of(&tk).unwrap_or_else(|| hk("style"));
        w.w(340, th);
    }
    w.w(0, "ENDTAB");
    let br_t = hk("brT");
    w.table("BLOCK_RECORD", &br_t, rec.len());
    for (b, _) in &list {
        let r = rec_of(&name_of(b));
        w.entry("BLOCK_RECORD", &r.br, &br_t, "AcDbBlockTableRecord", &enc_s(&block_name(jsv::get(Some(b), "name"))), 0.0);
        w.w(340, r.layout.clone().unwrap_or_else(|| "0".into()));
    }
    w.w(0, "ENDTAB");
    w.w(0, "ENDSEC");

    // BLOCKS: every record's BLOCK / ENDBLK; plain blocks and the other sheets hold their objects here
    w.w(0, "SECTION");
    w.w(2, "BLOCKS");
    for (i, (b, kind)) in list.iter().enumerate() {
        let n = name_of(b);
        let r = rec_of(&n);
        let is_paper = *kind == Kind::Space;
        let name = enc_s(&block_name(jsv::get(Some(b), "name")));
        w.w(0, "BLOCK");
        w.w(5, r.block.clone());
        w.w(330, r.br.clone());
        w.w(100, "AcDbEntity");
        if is_paper {
            w.w(67, "1");
        }
        w.w(8, "0");
        w.w(100, "AcDbBlockBegin");
        w.w(2, name.clone());
        let un = jsv::upper(&n);
        let anon = n.starts_with('*') && un != "*MODEL_SPACE" && un != "*PAPER_SPACE" && !is_paper;
        let ents = jsv::arr(jsv::get(Some(b), "entities"));
        let has_att = ents.iter().any(|x| jsv::s(jsv::get(Some(x), "type")) == Some("ATTDEF"));
        let xref = jsv::i32_of(jsv::to_num(jsv::get(Some(b), "flags"))) & 4 != 0;
        w.wn(70, f64::from((if anon { 1 } else { 0 }) | (if has_att { 2 } else { 0 }) | (if xref { 4 } else { 0 })));
        w.p(10, jsv::get(Some(b), "base"));
        w.w(3, name);
        w.w(1, if xref { enc(Some(&jsv::orv(jsv::get(Some(b), "xrefPath"), V::from("")))) } else { String::new() });
        if *kind != Kind::Ms && i != first_space && !xref {
            for e in ents {
                w.entity(e, &r.br, is_paper);
            }
        }
        w.w(0, "ENDBLK");
        w.w(5, r.endblk.clone());
        w.w(330, r.br.clone());
        w.w(100, "AcDbEntity");
        if is_paper {
            w.w(67, "1");
        }
        w.w(8, "0");
        w.w(100, "AcDbBlockEnd");
    }
    w.w(0, "ENDSEC");
    // ENTITIES: the model and the first sheet
    w.w(0, "SECTION");
    w.w(2, "ENTITIES");
    if let Some((b, _)) = ms_idx.and_then(|i| list.get(i)) {
        let br = rec_of(&name_of(b)).br;
        for e in jsv::arr(jsv::get(Some(b), "entities")) {
            w.entity(e, &br, false);
        }
    }
    if let Some((b, _)) = list.get(first_space) {
        let br = rec_of(&name_of(b)).br;
        for e in jsv::arr(jsv::get(Some(b), "entities")) {
            w.entity(e, &br, true);
        }
    }
    w.w(0, "ENDSEC");

    // OBJECTS: the named-object dictionary, groups, layouts, the Standard multiline style
    w.w(0, "SECTION");
    w.w(2, "OBJECTS");
    let (nod, groups, layouts_d, mls_d, mls) = (hk("nod"), hk("groups"), hk("layoutsD"), hk("mlsD"), hk("mls"));
    w.dict(
        &nod,
        "0",
        &[("ACAD_GROUP".into(), groups.clone()), ("ACAD_LAYOUT".into(), layouts_d.clone()), ("ACAD_MLINESTYLE".into(), mls_d.clone())],
    );
    w.dict(&groups, &nod, &[]);
    let litems: Vec<(String, String)> = layouts.iter().map(|l| (enc_s(&l.name), l.h.clone())).collect();
    w.dict(&layouts_d, &nod, &litems);
    w.dict(&mls_d, &nod, &[("Standard".into(), mls.clone())]);
    let clips = std::mem::take(&mut w.clips);
    for c in &clips {
        w.dict(&c.xd, &c.own, &[("ACAD_FILTER".into(), c.d2.clone())]);
        w.dict(&c.d2, &c.xd, &[("SPATIAL".into(), c.sf.clone())]);
        w.w(0, "SPATIAL_FILTER");
        w.w(5, c.sf.clone());
        w.w(102, "{ACAD_REACTORS");
        w.w(330, c.d2.clone());
        w.w(102, "}");
        w.w(330, c.d2.clone());
        w.w(100, "AcDbFilter");
        w.w(100, "AcDbSpatialFilter");
        w.wn(70, jsv::us2f(c.pts.len()));
        for p in &c.pts {
            w.wfo(10, jsv::get(Some(p), "x"));
            w.wfo(20, jsv::get(Some(p), "y"));
        }
        for (cd, s) in [(210, "0.0"), (220, "0.0"), (230, "1.0"), (11, "0.0"), (21, "0.0"), (31, "0.0"), (71, "1"), (72, "0"), (73, "0")] {
            w.w(cd, s);
        }
        for _ in 0..2 {
            for i in 0..12 {
                w.w(40, if i % 5 == 0 { "1.0" } else { "0.0" });
            }
        }
    }
    w.w(0, "MLINESTYLE");
    w.w(5, mls.clone());
    w.w(330, mls_d.clone());
    w.w(100, "AcDbMlineStyle");
    w.w(2, "Standard");
    w.w(70, "0");
    w.w(3, "");
    w.w(62, "256");
    w.w(51, "90.0");
    w.w(52, "90.0");
    w.w(71, "2");
    for o in [0.5, -0.5] {
        w.wf(49, o);
        w.w(62, "256");
        w.w(6, "BYLAYER");
    }
    for l in &layouts {
        let src = l.src.clone().map(V::Obj);
        let p = jsv::get(src.as_ref(), "paper").filter(|x| x.truthy()).cloned().unwrap_or_else(|| V::Obj(Obj::new()));
        let lim = jsv::get(src.as_ref(), "limits")
            .filter(|x| x.truthy())
            .cloned()
            .unwrap_or_else(|| V::Obj(cc_io_dwg::obj! { "min" => jsv::p2(0.0, 0.0), "max" => jsv::p2(12.0, 9.0) }));
        let paper = l.tab > 0.0;
        let gp = |k: &str| jsv::get(Some(&p), k);
        w.w(0, "LAYOUT");
        w.w(5, l.h.clone());
        w.w(102, "{ACAD_REACTORS");
        w.w(330, layouts_d.clone());
        w.w(102, "}");
        w.w(330, layouts_d.clone());
        w.w(100, "AcDbPlotSettings");
        w.w(1, "");
        w.w(2, "none_device");
        w.w(4, enc(Some(&jsv::orv(gp("name"), V::from("")))));
        w.w(6, "");
        for c in [40, 41, 42, 43] {
            w.w(c, "0.0");
        }
        w.wfo_or(44, gp("w"), 215.9);
        w.wfo_or(45, gp("h"), 279.4);
        for c in [46, 47, 48, 49, 140, 141] {
            w.w(c, "0.0");
        }
        w.w(142, "1.0");
        w.w(143, "1.0");
        w.w(70, if paper { "688" } else { "1712" });
        let un = jsv::nnv(gp("units"), V::Num(if paper { 1.0 } else { 0.0 }));
        w.wv(72, Some(&un));
        w.w(73, "0");
        w.w(74, if paper { "5" } else { "0" });
        w.w(7, "");
        w.w(75, if paper { "16" } else { "0" });
        w.w(147, "1.0");
        w.w(148, "0.0");
        w.w(149, "0.0");
        w.w(100, "AcDbLayout");
        w.w(1, enc_s(&l.name));
        w.w(70, "1");
        w.wn(71, l.tab);
        w.wfo(10, jsv::get(jsv::get(Some(&lim), "min"), "x"));
        w.wfo(20, jsv::get(jsv::get(Some(&lim), "min"), "y"));
        w.wfo(11, jsv::get(jsv::get(Some(&lim), "max"), "x"));
        w.wfo(21, jsv::get(jsv::get(Some(&lim), "max"), "y"));
        for (c, s) in [
            (12, "0.0"),
            (22, "0.0"),
            (32, "0.0"),
            (14, "0.0"),
            (24, "0.0"),
            (34, "0.0"),
            (15, "0.0"),
            (25, "0.0"),
            (35, "0.0"),
            (146, "0.0"),
            (13, "0.0"),
            (23, "0.0"),
            (33, "0.0"),
            (16, "1.0"),
            (26, "0.0"),
            (36, "0.0"),
            (17, "0.0"),
            (27, "1.0"),
            (37, "0.0"),
            (76, "0"),
        ] {
            w.w(c, s);
        }
        let sn = list.get(l.space).map(|(b, _)| name_of(b)).unwrap_or_default();
        w.w(330, rec_of(&sn).br);
    }
    w.w(0, "ENDSEC");
    w.w(0, "EOF");
    let seed = hex(w.seed);
    if w.out.get(handseed_at..handseed_at.saturating_add(4)) == Some("FFFF") {
        w.out.replace_range(handseed_at..handseed_at.saturating_add(4), &seed);
    }

    let mut counts_out = counts.clone();
    if w.skipped > 0.0 {
        counts_out.set("skipped", w.skipped);
    }
    let c = |k: &str| jsv::to_num(counts_out.get(k));
    let has = |k: &str| counts_out.get(k).is_some_and(V::truthy);
    let mut notes: Vec<String> = Vec::new();
    if has("mleader") {
        notes.push(cat.t("dxf2000.save.mleader", &i18n::n(c("mleader"))));
    }
    let pics = picture_refs(doc);
    if pics > 0 {
        notes.push(cat.t("dxf.save.pictures", &i18n::n(jsv::us2f(pics))));
    }
    let vpovr = doc
        .layouts
        .iter()
        .flat_map(|l| doc.space_entities(l.space))
        .filter(|e| e.typ() == "viewport" && e.extra.get("lov").and_then(serde_json::Value::as_object).is_some_and(|o| !o.is_empty()))
        .count();
    if vpovr > 0 {
        notes.push(cat.t("dxf.save.vpovr", &i18n::n(jsv::us2f(vpovr))));
    }
    if has("material") {
        notes.push(cat.t("dwg.save.material", &[]));
    }
    if has("tr2000") {
        notes.push(cat.t("dwg.save.tr2000", &[]));
    }
    if has("xrefBound") {
        notes.push(cat.t("dwg.save.xref.bound", &i18n::n(c("xrefBound"))));
    }
    if has("xrefLink") {
        notes.push(cat.t("dwg.save.xref.link", &i18n::n(c("xrefLink"))));
    }
    if has("skipped") || has("other") {
        let n = jsv::or(counts_out.get("skipped"), 0.0) + jsv::or(counts_out.get("other"), 0.0);
        notes.push(cat.t("dwg.save.other", &i18n::n(n)));
    }
    let text = std::mem::take(&mut w.out);
    let mut rep = Obj::new();
    rep.set("notes", V::Arr(notes.into_iter().map(V::Str).collect()));
    rep.set("counts", counts_out);
    Ok((text, jsv::into_json(V::Obj(rep))))
}

/// pictures.js `pictureRefs(doc)`
fn picture_refs(doc: &Doc) -> usize {
    let pic: Vec<&str> = doc
        .blocks
        .values()
        .filter(|b| b.entities.iter().any(|e| e.typ() == "poly" && e.extra.get("pic").is_some_and(|p| js::truthy(Some(p)))))
        .map(|b| b.name.as_str())
        .collect();
    if pic.is_empty() {
        return 0;
    }
    doc.model()
        .chain(doc.layouts.iter().flat_map(|l| doc.space_entities(l.space)))
        .filter(|e| matches!(&e.geo, Geo::Insert(i) if pic.contains(&i.block.as_str())))
        .count()
}
