//! dwg-import.js: a drawing model (cc-dwg's readDwg, or dxf-model.js's readDxfModel) → what `doc.load` takes:
//! { entities, layers, blocks, layouts, dimStyle, meta, linetypes, views, vports, ucs?, textStyles, dimStyles, report… }.
//!
//! Ported line by line; the JS's object identity tricks become explicit: multileaders / pictures / wipeouts carry a
//! private mark ([`EXTRA_MARK`]) where the JS keeps a WeakSet, and a dynamic block's `dyn.all` is re-synced with its
//! shown entities after attributes are resolved (the JS shares the objects). Where the JS would throw (a property read
//! on undefined), the import returns that error ([`JsErr`]).

use std::collections::{HashMap, HashSet};

use cc_cad::js;
use cc_cad_geom::geom::norm_angle;
use cc_cad_geom::jsmath::JsMath;
use cc_io_dwg::value::{Obj, V};

use crate::attrs::{self, AttdefOpts, BlockInfo};
use crate::i18n::{self, Catalog};
use crate::jsv::{self, JsErr, Ov, R};
use crate::props;
use crate::small;
use crate::text::{mtext_plain, plain};

const TAU: f64 = std::f64::consts::PI * 2.0;
const PI: f64 = std::f64::consts::PI;

/// The private key that marks a multileader / picture / wipeout (dwg-import.js's EXTRA WeakSet).
pub const EXTRA_MARK: &str = "\u{1}extra";
/// Types `extrasFrom` takes from a DXF's drawing model.
pub const EXTRA_TYPES: [&str; 3] = ["MULTILEADER", "IMAGE", "WIPEOUT"];
/// Copies of one MINSERT at most (the JS has no cap; a hostile row × column count would never finish).
const MAX_ARRAY: f64 = 100_000.0;
/// Pieces of one hatch arc / ellipse edge at most (the JS has no cap).
const MAX_STEPS: f64 = 1_000_000.0;

/// `VERSIONS[tag]`
pub fn release_of(tag: &str) -> &'static str {
    match tag {
        "AC1009" => "R12",
        "AC1012" => "R13",
        "AC1014" => "R14",
        "AC1015" => "2000",
        "AC1018" => "2004",
        "AC1021" => "2007",
        "AC1024" => "2010",
        "AC1027" => "2013",
        "AC1032" => "2018",
        _ => "",
    }
}
/// `UNITS[v]` (a property read: the key is `String(v)`).
pub fn units_name(v: Ov) -> &'static str {
    match jsv::to_str(v).as_str() {
        "1" => "inches",
        "2" => "feet",
        "3" => "miles",
        "4" => "millimeters",
        "5" => "centimeters",
        "6" => "meters",
        "7" => "kilometers",
        "8" => "microinches",
        "9" => "mils",
        "10" => "yards",
        _ => "",
    }
}

/// `'#' + (n & 0xffffff).toString(16).padStart(6, '0')`
pub fn hex(n: Ov) -> String {
    let w = js::int32(jsv::to_num(n));
    format!("#{:06x}", jsv::f2i(w) & 0xff_ffff)
}

/// `color(e)`: an object's colour → '#rrggbb', 0 (by block), 1–255, or null (by layer).
pub fn color_of(e: &Obj) -> V {
    let c = e.get("color").filter(|c| c.truthy());
    if let Some(rgb) = jsv::get(c, "rgb") {
        return V::Str(hex(Some(rgb)));
    }
    let i = jsv::get(c, "index");
    if jsv::num(i) == Some(0.0) {
        return V::Num(0.0);
    }
    let n = jsv::to_num(i);
    if i.is_some() && (1.0..=255.0).contains(&n) && !matches!(i, Some(V::Null)) {
        return i.cloned().unwrap_or(V::Null);
    }
    V::Null
}

/// `xdataOf(xdata, app)`: one application's xdata list (names compared without case).
pub fn xdata_of<'a>(xdata: Ov<'a>, app: &str) -> &'a [V] {
    let Some(x) = jsv::obj(xdata) else { return &[] };
    let Some(k) = jsv::keys_js(x).into_iter().find(|n| jsv::upper(n) == app) else { return &[] };
    match x.get(k) {
        Some(V::Arr(a)) => a.as_slice(),
        _ => &[],
    }
}

/// `/^standard$/i.test(String(v))`
fn is_standard(v: Ov) -> bool {
    jsv::to_str(v).eq_ignore_ascii_case("standard")
}

/// `styleName(n)`: a text style name other than Standard → { style }.
fn style_name(n: Ov, o: &mut Obj) {
    if let Some(s) = jsv::s(n).filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("standard")) {
        o.set("style", s);
    }
}

/// `maskOf(e)`: MTEXT's background mask → { bgm: { color, scale } } (WL.8).
pub fn mask_of(e: &Obj, o: &mut Obj) {
    let f = jsv::i32_of(jsv::to_num(e.get("backgroundFlags")));
    if f & 1 == 0 {
        return;
    }
    let c = e.get("backgroundColor").filter(|c| c.truthy());
    let color = if f & 2 != 0 {
        V::from("bg")
    } else if let Some(rgb) = jsv::get(c, "rgb") {
        V::Str(hex(Some(rgb)))
    } else {
        let a = jsv::nn(jsv::get(c, "index"), 7.0).abs();
        V::Num(if a != 0.0 && !a.is_nan() { a } else { 7.0 })
    };
    let sc = jsv::to_num(e.get("backgroundScale"));
    let mut b = Obj::new();
    b.set("color", color);
    b.set("scale", if sc.is_finite() && (1.0..=5.0).contains(&sc) { sc } else { 1.5 });
    o.set("bgm", b);
}

/// `lunitOk(st)`: DIMLUNIT 1–5 kept (6 → 2), anything else dropped.
pub fn lunit_ok(st: &mut Obj) {
    let Some(l) = st.get("lunit") else { return };
    let v = js::round(jsv::to_num(Some(l)));
    if (1.0..=5.0).contains(&v) {
        st.set("lunit", v);
    } else if v == 6.0 {
        st.set("lunit", 2.0);
    } else {
        st.remove("lunit");
    }
}

/// `lwMm(v)`: 1/100 mm → mm; -2 / -3 kept; anything else (by layer) None.
fn lw_mm(v: Ov) -> Option<f64> {
    match jsv::num(v) {
        Some(x) if x == -2.0 || x == -3.0 => Some(x),
        Some(x) if x.is_finite() && x >= 0.0 => Some(x / 100.0),
        _ => None,
    }
}

/// `textOf(e, base, X)`: TEXT, ATTRIB or ATTDEF → a text entity (value may be empty), or None without a height.
fn text_of(e: &Obj, base: &Obj, flip: bool) -> R<Option<Obj>> {
    if !jsv::truthy(e.get("height")) || !jsv::truthy(e.get("insert")) {
        return Ok(None);
    }
    let s = plain(&jsv::str_or_empty(e.get("value")));
    let h = jsv::or(e.get("hAlign"), 0.0);
    let v = jsv::or(e.get("vAlign"), 0.0);
    let aligned = h != 0.0 || v != 0.0;
    let al = e.get("align");
    let ap = aligned && h != 3.0 && h != 5.0 && jsv::truthy(al) && (jsv::truthy(jsv::get(al, "x")) || jsv::truthy(jsv::get(al, "y")));
    let p = if ap { al } else { e.get("insert") };
    let mut t = Obj::new();
    t.set("type", "text");
    t.assign(base.clone());
    t.set_opt("x", xf(flip, jsv::prop(p, "x")?));
    t.set_opt("y", jsv::pv(p, "y")?);
    t.set_opt("h", e.get("height").cloned());
    t.set("rot", norm_angle(jsv::or(e.get("rotation"), 0.0)));
    t.set("wf", jsv::orv(e.get("widthFactor"), V::Num(1.0)));
    t.set("s", s);
    t.set(
        "ha",
        if h == 1.0 || h == 4.0 {
            "center"
        } else if h == 2.0 {
            "right"
        } else {
            "left"
        },
    );
    t.set(
        "va",
        if h == 4.0 || v == 2.0 {
            "middle"
        } else if v == 3.0 {
            "top"
        } else if v == 1.0 {
            "bottom"
        } else {
            "baseline"
        },
    );
    style_name(e.get("style"), &mut t);
    if let Some(o) = jsv::fin(e.get("oblique")).filter(|o| o.abs() > 1e-12) {
        t.set("obl", o);
    }
    Ok(Some(t))
}

/// `X(x)`: OCS x, negated when the extrusion points down (undefined stays undefined unless negated).
fn xf(flip: bool, v: Ov) -> Option<V> {
    if flip { Some(V::Num(-jsv::to_num(v))) } else { v.cloned() }
}

/// `attdefOf(e)`: a block's ATTDEF → an attribute definition (flags 1 invisible, 2 constant, 8 preset).
pub fn attdef_of(e: &Obj) -> Option<Obj> {
    if !jsv::truthy(e.get("tag")) {
        return None;
    }
    let flip = jsv::to_num(jsv::get(e.get("extrusion"), "z")) < -0.5 && jsv::truthy(e.get("extrusion"));
    let mut base = Obj::new();
    base.set("layer", jsv::orv(e.get("layer"), V::from("0")));
    base.set("color", color_of(e));
    let t = text_of(e, &base, flip).ok().flatten()?;
    let fl = jsv::i32_of(jsv::or(e.get("flags"), 0.0));
    let def = jsv::to_str(t.get("s"));
    Some(attrs::attdef(
        &t,
        AttdefOpts {
            tag: e.get("tag").cloned().unwrap_or(V::Null),
            prompt: plain(&jsv::str_or_empty(e.get("prompt"))),
            def,
            invisible: fl & 1 != 0,
            constant: fl & 2 != 0,
            preset: fl & 8 != 0,
        },
    ))
}

/// `splinePoints(e)`: fit points when given, else the B-spline sampled from control points and knots.
pub fn spline_points(e: &Obj) -> R<Vec<(Option<V>, Option<V>)>> {
    let fit = jsv::arr(e.get("fit"));
    if jsv::truthy(e.get("fit")) && fit.len() > 1 {
        return Ok(fit.iter().map(|p| (jsv::get(Some(p), "x").cloned(), jsv::get(Some(p), "y").cloned())).collect());
    }
    let cp = if jsv::truthy(e.get("control")) { jsv::arr(e.get("control")) } else { &[] };
    let k = if jsv::truthy(e.get("knots")) { jsv::arr(e.get("knots")) } else { &[] };
    let p = jsv::or(e.get("degree"), 3.0);
    if cp.len() < 2 {
        return Ok(Vec::new());
    }
    let raw = || cp.iter().map(|p| (jsv::get(Some(p), "x").cloned(), jsv::get(Some(p), "y").cloned())).collect();
    if jsv::us2f(k.len()) != jsv::us2f(cp.len()) + p + 1.0 {
        return Ok(raw());
    }
    if p < 0.0 {
        return Err(JsErr("Cannot read properties of undefined (reading '0')".into()));
    }
    // a degree this high means thousands of knots per point: the JS would run for minutes (left as the control points)
    if p > 200.0 {
        return Ok(raw());
    }
    let pu = jsv::f2us(p);
    let ws = jsv::arr(e.get("weights"));
    let w = (jsv::truthy(e.get("weights")) && ws.len() == cp.len()).then_some(ws);
    let kn = |i: usize| k.get(i).map_or(f64::NAN, |v| jsv::to_num(Some(v)));
    let lo = kn(pu);
    let hi = kn(k.len().saturating_sub(pu).saturating_sub(1));
    let n = js::min(400.0, js::max(16.0, jsv::us2f(cp.len()) * 8.0));
    let nu = jsv::f2us(n);
    let mut out = Vec::with_capacity(nu.saturating_add(1));
    for s in 0..=nu {
        let sf = jsv::us2f(s);
        let u = if s == nu { hi - 1e-9 * (hi - lo) } else { lo + (hi - lo) * (sf / n) };
        let mut span = pu;
        while span < cp.len().saturating_sub(1) && u >= kn(span.saturating_add(1)) {
            span = span.saturating_add(1);
        }
        let mut d: Vec<[f64; 3]> = Vec::with_capacity(pu.saturating_add(1));
        for j in 0..=pu {
            let idx = span.saturating_sub(pu).saturating_add(j);
            let q = cp.get(idx);
            let wt = match w {
                Some(w) => jsv::to_num(w.get(idx)),
                None => 1.0,
            };
            d.push([jsv::to_num(jsv::get(q, "x")) * wt, jsv::to_num(jsv::get(q, "y")) * wt, wt]);
        }
        for r in 1..=pu {
            let mut j = pu;
            while j >= r {
                let i = span.saturating_sub(pu).saturating_add(j);
                let den = kn(i.saturating_add(pu).saturating_sub(r).saturating_add(1)) - kn(i);
                let a = if den == 0.0 { 0.0 } else { (u - kn(i)) / den };
                let prev = d.get(j.saturating_sub(1)).copied().unwrap_or([f64::NAN; 3]);
                if let Some(dj) = d.get_mut(j) {
                    *dj = [(1.0 - a) * prev[0] + a * dj[0], (1.0 - a) * prev[1] + a * dj[1], (1.0 - a) * prev[2] + a * dj[2]];
                }
                j = j.saturating_sub(1);
                if j == 0 {
                    break;
                }
            }
        }
        let last = d.get(pu).copied().unwrap_or([f64::NAN; 3]);
        out.push((Some(V::Num(last[0] / last[2])), Some(V::Num(last[1] / last[2]))));
    }
    Ok(out)
}

/// `arcBetween(pts, p, q, b)`: the points of a bulged segment (not its ends).
fn arc_between(pts: &mut Vec<f64>, p: (f64, f64), q: (f64, f64), b: f64) {
    let theta = 4.0 * b.jatan();
    let c = js::hypot(q.0 - p.0, q.1 - p.1);
    if c == 0.0 || c.is_nan() {
        return;
    }
    let r = c / (2.0 * (theta / 2.0).jsin());
    let (mx, my) = ((p.0 + q.0) / 2.0, (p.1 + q.1) / 2.0);
    let h = r * (theta / 2.0).jcos();
    let ux = -(q.1 - p.1) / c;
    let uy = (q.0 - p.0) / c;
    let cx = mx + ux * h;
    let cy = my + uy * h;
    let a0 = (p.1 - cy).jatan2(p.0 - cx);
    let n = js::max(2.0, (16.0 * theta.abs() / PI).ceil());
    let nu = jsv::f2us(n.min(MAX_STEPS));
    for k in 1..nu {
        let t = a0 + theta * jsv::us2f(k) / n;
        pts.push(cx + r.abs() * t.jcos());
        pts.push(cy + r.abs() * t.jsin());
    }
}

/// `while (sweep <= 0) sweep += TAU` (bounded: a sweep that adding 2π cannot lift is left as it is).
fn lift(mut sweep: f64) -> f64 {
    let mut guard = 0u32;
    while sweep <= 0.0 {
        let next = sweep + TAU;
        guard = guard.saturating_add(1);
        if next == sweep || guard > 1_000_000 {
            break;
        }
        sweep = next;
    }
    sweep
}

fn num_at(o: Ov, k: &str) -> R<f64> {
    Ok(jsv::to_num(jsv::prop(o, k)?))
}

/// `hatchLoops(e)`: a HATCH's boundary paths → rings [x, y, …].
fn hatch_loops(e: &Obj) -> R<Vec<Vec<f64>>> {
    let mut rings = Vec::new();
    for path in jsv::arr(e.get("loops")) {
        let mut pts: Vec<f64> = Vec::new();
        let pl = jsv::get(Some(path), "polyline");
        if jsv::truthy(pl) {
            let vs = jsv::arr(jsv::get(pl, "vertices"));
            let n = vs.len();
            for i in 0..n {
                let a = vs.get(i);
                let b = vs.get(jsv::wrap_next(i, n));
                let ap = (num_at(a, "x")?, num_at(a, "y")?);
                pts.push(ap.0);
                pts.push(ap.1);
                let bulge = jsv::or(jsv::get(a, "bulge"), 0.0);
                if bulge.abs() > 1e-9 {
                    let bp = (num_at(b, "x")?, num_at(b, "y")?);
                    arc_between(&mut pts, ap, bp, jsv::to_num(jsv::get(a, "bulge")));
                }
            }
            if n > 0 {
                let v0 = vs.first();
                pts.push(num_at(v0, "x")?);
                pts.push(num_at(v0, "y")?);
            }
        } else {
            for ed in jsv::arr(jsv::get(Some(path), "edges")) {
                let g = |k: &str| jsv::get(Some(ed), k);
                match jsv::s(g("type")) {
                    Some("line") => {
                        let (s, t) = (g("start"), g("end"));
                        pts.extend([num_at(s, "x")?, num_at(s, "y")?, num_at(t, "x")?, num_at(t, "y")?]);
                    }
                    Some("arc") => {
                        let cw = matches!(g("ccw"), Some(V::Bool(false)));
                        let mut s = jsv::to_num(g("startAngle"));
                        let mut t = jsv::to_num(g("endAngle"));
                        if cw {
                            s = -s;
                            t = -t;
                        }
                        let sweep = lift(if cw { s - t } else { t - s });
                        let dir = if cw { -1.0 } else { 1.0 };
                        let n = js::max(4.0, (32.0 * sweep / TAU).ceil());
                        let c = g("center");
                        let (cx, cy, r) = (num_at(c, "x")?, num_at(c, "y")?, jsv::to_num(g("radius")));
                        let nu = jsv::f2us(n.min(MAX_STEPS));
                        for q in 0..=nu {
                            let a = s + dir * sweep * jsv::us2f(q) / n;
                            pts.push(cx + r * a.jcos());
                            pts.push(cy + r * a.jsin());
                        }
                    }
                    Some("ellipse") => {
                        let m = g("majorAxis");
                        let (mx, my) = (num_at(m, "x")?, num_at(m, "y")?);
                        let l = js::hypot(mx, my);
                        let len = if l != 0.0 && !l.is_nan() { l } else { 1.0 };
                        let ratio = jsv::or(g("ratio"), 1.0);
                        let rot = my.jatan2(mx);
                        let st = jsv::to_num(g("startAngle"));
                        let sweep = lift(jsv::to_num(g("endAngle")) - st);
                        let n = js::max(8.0, (48.0 * sweep / TAU).ceil());
                        let c = g("center");
                        let (cx, cy) = (num_at(c, "x")?, num_at(c, "y")?);
                        let nu = jsv::f2us(n.min(MAX_STEPS));
                        for q in 0..=nu {
                            let t = st + sweep * jsv::us2f(q) / n;
                            let x = len * t.jcos();
                            let y = len * ratio * t.jsin();
                            pts.push(cx + x * rot.jcos() - y * rot.jsin());
                            pts.push(cy + x * rot.jsin() + y * rot.jcos());
                        }
                    }
                    Some("spline") => {
                        let mut o = Obj::new();
                        for k in ["fit", "control", "knots", "degree", "weights"] {
                            o.set_opt(k, g(k).cloned());
                        }
                        for (x, y) in spline_points(&o)? {
                            pts.push(jsv::to_num(x.as_ref()));
                            pts.push(jsv::to_num(y.as_ref()));
                        }
                    }
                    _ => {}
                }
            }
        }
        if pts.len() >= 6 {
            rings.push(pts);
        }
    }
    Ok(rings)
}

fn nums(v: &[f64]) -> V {
    V::Arr(v.iter().map(|x| V::Num(*x)).collect())
}
fn pt(x: Option<V>, y: Option<V>) -> V {
    let mut o = Obj::new();
    o.set_opt("x", x);
    o.set_opt("y", y);
    V::Obj(o)
}
fn ptb(x: Option<V>, y: Option<V>) -> V {
    let mut o = Obj::new();
    o.set_opt("x", x);
    o.set_opt("y", y);
    o.set("b", 0.0);
    V::Obj(o)
}

/// The importer's state (the closure variables of importDwg).
struct Imp<'a> {
    db: &'a Obj,
    cat: &'a Catalog,
    report: Obj,
    lt_by_handle: HashMap<String, Option<V>>,
    layer_by_handle: HashMap<String, Option<V>>,
    vp_overrides: HashMap<String, Obj>,
    dim_style: Obj,
    dim_styles: HashMap<String, Obj>,
    blocks: Vec<V>,
    anon_n: u64,
    taken: HashSet<String>,
    image_defs: Vec<(V, &'a V)>,
    obj_by_h: HashMap<String, &'a V>,
    block_list: &'a [V],
}

impl<'a> Imp<'a> {
    fn inc(&mut self, k: &'static str, by: f64) {
        let n = jsv::or(self.report.get(k), 0.0) + by;
        self.report.set(k, n);
    }
    fn skip(&mut self, t: &str) {
        if !matches!(self.report.get("skipped"), Some(V::Obj(_))) {
            self.report.set("skipped", Obj::new());
        }
        if let Some(V::Obj(sk)) = self.report.get_mut("skipped") {
            let n = jsv::or(sk.get(t), 0.0) + 1.0;
            sk.set(t.to_string(), n);
        }
    }
    fn t(&self, key: &str) -> String {
        self.cat.t(key, &[])
    }

    /// `ltName(v)`: a linetype handle or name → its name (null for none).
    fn lt_name(&self, v: Ov) -> Option<String> {
        if jsv::nullish(v) || jsv::s(v) == Some("") {
            return None;
        }
        let key = jsv::upper(&jsv::to_str(v));
        match self.lt_by_handle.get(&key) {
            Some(Some(n)) if n.truthy() => Some(jsv::to_str(Some(n))),
            _ => Some(jsv::to_str(v)),
        }
    }

    fn free_name(&mut self, pre: &str) -> String {
        loop {
            self.anon_n = self.anon_n.saturating_add(1);
            let n = format!("{pre}{}", self.anon_n);
            let up = jsv::upper(&n);
            if !self.taken.contains(&up) {
                self.taken.insert(up);
                return n;
            }
        }
    }

    /// `styleOf(e)`: an object's own linetype, lineweight and linetype scale (absent = by layer).
    fn style_of(&self, e: &Obj, o: &mut Obj) {
        if let Some(n) = self.lt_name(e.get("linetype")).filter(|n| !n.is_empty() && !n.eq_ignore_ascii_case("bylayer")) {
            let lt = if n.eq_ignore_ascii_case("byblock") {
                "ByBlock".to_string()
            } else if n.eq_ignore_ascii_case("continuous") {
                "Continuous".to_string()
            } else {
                n
            };
            o.set("lt", lt);
        }
        if let Some(w) = lw_mm(e.get("lineweight")) {
            o.set("lw", w);
        }
        if let Some(s) = jsv::fin(e.get("ltscale")).filter(|s| *s > 0.0 && (s - 1.0).abs() > 1e-12) {
            o.set("lts", s);
        }
    }

    /// `convert(e)`: one model entity → CadCraft's (an object, or an array for a MINSERT), CadCraft's xdata keys kept.
    fn convert(&mut self, e: &Obj) -> R<Option<V>> {
        let mut c = self.convert_base(e)?;
        if let Some(cv) = c.as_mut() {
            small::tag_from_xdata(cv, e.get("xdata"));
        }
        let mut c = self.with_props(c, e);
        let et = jsv::s(e.get("type"));
        if et == Some("INSERT") {
            let one = match c.as_mut() {
                Some(V::Arr(a)) => a.first_mut(),
                Some(x) => Some(x),
                None => None,
            };
            if let Some(V::Obj(one)) = one
                && jsv::s(one.get("type")) == Some("insert")
            {
                if jsv::truthy(e.get("dyn"))
                    && let Some(d) = crate::dyn_blocks::dyn_ref_from_dwg(e)
                {
                    one.set("dyn", d);
                }
                if jsv::truthy(e.get("extDict")) {
                    let by = |h: Ov| self.obj_by_h.get(&jsv::to_str(h)).copied();
                    let d1 = by(e.get("extDict"));
                    let d2 = by(jsv::get(jsv::get(d1, "entries"), "ACAD_FILTER"));
                    let f = by(jsv::get(jsv::get(d2, "entries"), "SPATIAL"));
                    if let Some(f) = f
                        && jsv::s(jsv::get(Some(f), "type")) == Some("SPATIAL_FILTER")
                        && let Some(clip) = small::clip_from_filter(Some(f))
                    {
                        one.set("clip", clip);
                    }
                }
            }
        }
        Ok(c)
    }

    /// `withProps(c, e)`: transparency, thickness, Z, and what xdata holds.
    fn with_props(&self, mut c: Option<V>, e: &Obj) -> Option<V> {
        let one_type = match c.as_ref() {
            None => return c,
            Some(V::Arr(a)) if a.is_empty() => return c,
            Some(V::Arr(a)) => match a.first() {
                Some(V::Obj(o)) => jsv::to_str(o.get("type")),
                _ => return c,
            },
            Some(V::Obj(o)) => jsv::to_str(o.get("type")),
            Some(x) if !x.truthy() => return c,
            Some(_) => String::new(),
        };
        if one_type == "face3d" {
            return c;
        }
        let mut add = Obj::new();
        if let Some(tr) = props::tr_from(e.get("transparency")) {
            add.set("tr", tr);
        }
        if let Some(th) = jsv::fin(e.get("thickness")).filter(|t| *t != 0.0)
            && ["line", "circle", "arc", "poly", "point", "text"].contains(&one_type.as_str())
        {
            add.set("th", th);
        }
        add.assign(props::props_from_xdata(e.get("xdata")));
        if one_type != "hatch" {
            for k in ["bg", "ox", "oy", "assoc"] {
                add.remove(k);
            }
        }
        let z = z_of(e, &one_type);
        if z != 0.0 && !z.is_nan() {
            add.set("z", z);
        }
        let each = |c: &mut Option<V>, f: &mut dyn FnMut(&mut Obj)| match c {
            Some(V::Arr(a)) => {
                for x in a.iter_mut() {
                    if let V::Obj(o) = x {
                        f(o);
                    }
                }
            }
            Some(V::Obj(o)) => f(o),
            _ => {}
        };
        each(&mut c, &mut |o: &mut Obj| o.assign(add.clone()));
        if one_type == "line" {
            let z1 = jsv::get(e.get("start"), "z").cloned();
            let z2 = jsv::get(e.get("end"), "z").cloned();
            each(&mut c, &mut |o: &mut Obj| {
                o.remove("z");
                if z1.as_ref().is_some_and(V::truthy) {
                    o.set_opt("z1", z1.clone());
                }
                if z2.as_ref().is_some_and(V::truthy) {
                    o.set_opt("z2", z2.clone());
                }
            });
        }
        if one_type == "hatch" && (jsv::truthy(add.get("ox")) || jsv::truthy(add.get("oy"))) {
            let ox = jsv::or(add.get("ox"), 0.0);
            let oy = jsv::or(add.get("oy"), 0.0);
            let first = match c.as_mut() {
                Some(V::Arr(a)) => a.first_mut(),
                Some(x) => Some(x),
                None => None,
            };
            if let Some(V::Obj(one)) = first
                && let Some(V::Arr(pl)) = one.get_mut("patlines")
            {
                for l in pl.iter_mut() {
                    if let V::Obj(l) = l {
                        let x = jsv::to_num(l.get("x")) - ox;
                        let y = jsv::to_num(l.get("y")) - oy;
                        l.set("x", x);
                        l.set("y", y);
                    }
                }
            }
        }
        c
    }

    fn convert_base(&mut self, e: &Obj) -> R<Option<V>> {
        if jsv::truthy(e.get("invisible")) {
            return Ok(None);
        }
        let mut base = Obj::new();
        base.set("layer", jsv::orv(e.get("layer"), V::from("0")));
        base.set("color", color_of(e));
        self.style_of(e, &mut base);
        let ext = e.get("extrusion");
        let f = jsv::truthy(ext) && jsv::to_num(jsv::get(ext, "z")) < -0.5;
        let et = jsv::to_str(e.get("type"));
        let typ = if et.starts_with("DIMENSION_") { "DIMENSION".to_string() } else { et.clone() };
        let mk = |t: &str, base: &Obj| {
            let mut o = Obj::new();
            o.set("type", t);
            o.assign(base.clone());
            o
        };
        let g = |k: &str| e.get(k);
        Ok(match typ.as_str() {
            "LINE" => {
                let mut o = mk("line", &base);
                o.set_opt("x1", jsv::pv(g("start"), "x")?);
                o.set_opt("y1", jsv::pv(g("start"), "y")?);
                o.set_opt("x2", jsv::pv(g("end"), "x")?);
                o.set_opt("y2", jsv::pv(g("end"), "y")?);
                Some(V::Obj(o))
            }
            "POINT" => {
                let mut o = mk("point", &base);
                o.set("x", jsv::nnv(jsv::get(g("position"), "x"), V::Num(0.0)));
                o.set("y", jsv::nnv(jsv::get(g("position"), "y"), V::Num(0.0)));
                Some(V::Obj(o))
            }
            "CIRCLE" => {
                let mut o = mk("circle", &base);
                o.set_opt("cx", xf(f, jsv::prop(g("center"), "x")?));
                o.set_opt("cy", jsv::pv(g("center"), "y")?);
                o.set_opt("r", g("radius").cloned());
                Some(V::Obj(o))
            }
            "ARC" => {
                let mut a0 = jsv::to_num(g("startAngle"));
                let mut a1 = jsv::to_num(g("endAngle"));
                if f {
                    (a0, a1) = (PI - a1, PI - a0);
                }
                let mut o = mk("arc", &base);
                o.set_opt("cx", xf(f, jsv::prop(g("center"), "x")?));
                o.set_opt("cy", jsv::pv(g("center"), "y")?);
                o.set_opt("r", g("radius").cloned());
                o.set("a0", norm_angle(a0));
                o.set("a1", norm_angle(a1));
                Some(V::Obj(o))
            }
            "LWPOLYLINE" | "POLYLINE_2D" => {
                let vs: Vec<&V> =
                    jsv::arr(g("vertices")).iter().filter(|v| jsv::i32_of(jsv::to_num(jsv::get(Some(v), "flags"))) & 16 == 0).collect();
                if vs.len() < 2 {
                    return Ok(None);
                }
                let cw = if jsv::to_num(g("constantWidth")) > 0.0 { g("constantWidth").cloned() } else { None };
                let each = cw.is_none()
                    && vs
                        .iter()
                        .any(|v| jsv::to_num(jsv::get(Some(v), "startWidth")) > 0.0 || jsv::to_num(jsv::get(Some(v), "endWidth")) > 0.0);
                let wd = |x: Ov| jsv::fin(x).filter(|x| *x > 0.0).unwrap_or(0.0);
                let mut o = mk("poly", &base);
                o.set("closed", jsv::truthy(g("closed")));
                if let Some(cw) = cw {
                    o.set("w", cw);
                }
                let sgn = if f { -1.0 } else { 1.0 };
                let mut pts = Vec::with_capacity(vs.len());
                for v in vs {
                    let mut p = Obj::new();
                    p.set_opt("x", xf(f, jsv::prop(Some(v), "x")?));
                    p.set_opt("y", jsv::get(Some(v), "y").cloned());
                    p.set("b", sgn * jsv::or(jsv::get(Some(v), "bulge"), 0.0));
                    if each {
                        p.set("sw", wd(jsv::get(Some(v), "startWidth")));
                        p.set("ew", wd(jsv::get(Some(v), "endWidth")));
                    }
                    pts.push(V::Obj(p));
                }
                o.set("pts", V::Arr(pts));
                Some(V::Obj(o))
            }
            "POLYLINE_3D" => {
                let vs: Vec<&V> =
                    jsv::arr(g("vertices")).iter().filter(|v| jsv::i32_of(jsv::to_num(jsv::get(Some(v), "flags"))) & 16 == 0).collect();
                if vs.len() < 2 {
                    return Ok(None);
                }
                let mut o = mk("poly", &base);
                o.set("closed", jsv::truthy(g("closed")));
                let mut pts = Vec::with_capacity(vs.len());
                for v in vs {
                    pts.push(ptb(jsv::pv(Some(v), "x")?, jsv::get(Some(v), "y").cloned()));
                }
                o.set("pts", V::Arr(pts));
                Some(V::Obj(o))
            }
            "ELLIPSE" => {
                let mut o = mk("ellipse", &base);
                o.set_opt("cx", jsv::pv(g("center"), "x")?);
                o.set_opt("cy", jsv::pv(g("center"), "y")?);
                o.set_opt("mx", jsv::pv(g("majorAxis"), "x")?);
                o.set_opt("my", jsv::pv(g("majorAxis"), "y")?);
                o.set("ratio", jsv::orv(g("ratio"), V::Num(1.0)));
                o.set("t0", jsv::nnv(g("startParam"), V::Num(0.0)));
                o.set("t1", jsv::nnv(g("endParam"), V::Num(TAU)));
                Some(V::Obj(o))
            }
            "SPLINE" => {
                let pts = spline_points(e)?;
                if pts.len() < 2 {
                    self.skip("SPLINE");
                    return Ok(None);
                }
                self.inc("splines", 1.0);
                let mut o = mk("poly", &base);
                o.set("closed", false);
                o.set("pts", V::Arr(pts.into_iter().map(|(x, y)| ptb(x, y)).collect()));
                Some(V::Obj(o))
            }
            "TEXT" | "ATTRIB" => {
                let t = text_of(e, &base, f)?;
                if et == "ATTRIB" {
                    return Ok(match t {
                        Some(mut t) if jsv::truthy(g("tag")) => {
                            t.set_opt("tag", g("tag").cloned());
                            if jsv::i32_of(jsv::to_num(g("flags"))) & 1 != 0 {
                                t.set("invisible", true);
                            }
                            Some(V::Obj(t))
                        }
                        _ => None,
                    });
                }
                match t {
                    Some(t) if !js::trim(&jsv::to_str(t.get("s"))).is_empty() => Some(V::Obj(t)),
                    _ => None,
                }
            }
            "MTEXT" => {
                let raw = jsv::str_or_empty(g("value"));
                let s = mtext_plain(&raw);
                if s.is_empty() || !jsv::truthy(g("height")) {
                    return Ok(None);
                }
                let xa = g("xAxis");
                let rot = if jsv::truthy(xa) && (jsv::truthy(jsv::get(xa, "x")) || jsv::truthy(jsv::get(xa, "y"))) {
                    jsv::to_num(jsv::get(xa, "y")).jatan2(jsv::to_num(jsv::get(xa, "x")))
                } else {
                    0.0
                };
                let ap = jsv::or(g("attach"), 1.0) - 1.0;
                let ww = if jsv::to_num(g("width")) > 0.0 { g("width").cloned() } else { None };
                let lsn = jsv::to_num(g("lineSpacing"));
                let ls = if lsn > 0.0 && lsn != 1.0 { g("lineSpacing").cloned() } else { None };
                let obl = q_oblique(&raw);
                let mut o = mk("text", &base);
                o.set_opt("x", jsv::pv(g("insert"), "x")?);
                o.set_opt("y", jsv::pv(g("insert"), "y")?);
                o.set_opt("h", g("height").cloned());
                o.set("rot", norm_angle(rot));
                o.set("s", s);
                o.set_opt("ha", pick3(ap % 3.0, ["left", "center", "right"]));
                o.set_opt("va", pick3((ap / 3.0).floor(), ["top", "middle", "bottom"]));
                if let Some(w) = ww {
                    o.set("ww", w);
                }
                if let Some(l) = ls {
                    o.set("ls", l);
                }
                style_name(g("style"), &mut o);
                if obl != 0.0 {
                    o.set("obl", obl);
                }
                mask_of(e, &mut o);
                Some(V::Obj(o))
            }
            "SOLID" | "TRACE" => {
                let p = jsv::arr(g("points"));
                let c3 = p.get(3).filter(|q| q.truthy()).or(p.get(2));
                let cs: Vec<&V> = [p.first(), p.get(1), c3, p.get(2)].into_iter().flatten().filter(|q| q.truthy()).collect();
                let Some(c0) = cs.first() else { return Err(JsErr("c[0] is not iterable".into())) };
                let mut flat: Vec<V> = Vec::new();
                for q in &cs {
                    flat.push(xf(f, jsv::get(Some(q), "x")).unwrap_or(V::Null));
                    flat.push(jsv::get(Some(q), "y").cloned().unwrap_or(V::Null));
                }
                flat.push(xf(f, jsv::get(Some(c0), "x")).unwrap_or(V::Null));
                flat.push(jsv::get(Some(c0), "y").cloned().unwrap_or(V::Null));
                let mut o = mk("hatch", &base);
                o.set("solid", true);
                o.set("loops", V::Arr(vec![V::Arr(flat)]));
                Some(V::Obj(o))
            }
            "HATCH" => {
                let loops = hatch_loops(e)?;
                if loops.is_empty() {
                    let k = self.t("dwg-import.hatch_boundary_not_read");
                    self.skip(&k);
                    return Ok(None);
                }
                let mut h = mk("hatch", &base);
                h.set("solid", jsv::truthy(g("solid")));
                h.set("pattern", jsv::orv(g("pattern"), V::from("")));
                h.set("loops", V::Arr(loops.iter().map(|l| nums(l)).collect()));
                if jsv::num(g("style")) == Some(1.0) {
                    h.set("island", "outer");
                } else if jsv::num(g("style")) == Some(2.0) {
                    h.set("island", "ignore");
                }
                if jsv::truthy(g("associative")) {
                    h.set("assoc", true);
                }
                if !jsv::truthy(g("solid")) {
                    if jsv::to_num(g("scale")) > 0.0 {
                        h.set_opt("pscale", g("scale").cloned());
                    }
                    if jsv::truthy(g("angle")) {
                        h.set("pangle", norm_angle(jsv::to_num(g("angle"))));
                    }
                    let pl: Vec<&V> = jsv::arr(g("patternLines"))
                        .iter()
                        .filter(|l| {
                            let gl = |k: &str| jsv::get(Some(l), k);
                            [
                                gl("angle"),
                                jsv::get(gl("base"), "x"),
                                jsv::get(gl("base"), "y"),
                                jsv::get(gl("offset"), "x"),
                                jsv::get(gl("offset"), "y"),
                            ]
                            .iter()
                            .all(|v| jsv::fin(*v).is_some())
                        })
                        .collect();
                    if !pl.is_empty() && pl.len() <= 64 {
                        let lines: Vec<V> = pl
                            .iter()
                            .map(|l| {
                                let gl = |k: &str| jsv::get(Some(l), k);
                                let ang = jsv::to_num(gl("angle"));
                                let (ca, sa) = (ang.jcos(), ang.jsin());
                                let (ox, oy) = (jsv::to_num(jsv::get(gl("offset"), "x")), jsv::to_num(jsv::get(gl("offset"), "y")));
                                let dashes: Vec<V> =
                                    jsv::arr(gl("dashes")).iter().filter(|d| jsv::fin(Some(d)).is_some()).take(32).cloned().collect();
                                let mut o = Obj::new();
                                o.set("angle", ang * 180.0 / PI);
                                o.set_opt("x", jsv::get(gl("base"), "x").cloned());
                                o.set_opt("y", jsv::get(gl("base"), "y").cloned());
                                o.set("dx", ox * ca + oy * sa);
                                o.set("dy", -ox * sa + oy * ca);
                                o.set("dashes", V::Arr(dashes));
                                V::Obj(o)
                            })
                            .collect();
                        h.set("patlines", V::Arr(lines));
                    }
                }
                Some(V::Obj(h))
            }
            "INSERT" | "MINSERT" => {
                if !jsv::truthy(g("block")) {
                    return Ok(None);
                }
                let ins_p = g("insert");
                let mut x = jsv::to_num(jsv::prop(ins_p, "x")?);
                let x_raw = jsv::prop(ins_p, "x")?.cloned();
                let mut rot = jsv::or(g("rotation"), 0.0);
                let sc = g("scale");
                let mut sx = jsv::orv(jsv::get(sc, "x"), V::Num(1.0));
                let mut xv = x_raw;
                if f {
                    x = -x;
                    xv = Some(V::Num(x));
                    rot = PI - rot;
                    sx = V::Num(-jsv::to_num(Some(&sx)));
                }
                let mut ins = mk("insert", &base);
                ins.set_opt("block", g("block").cloned());
                ins.set_opt("x", xv);
                ins.set_opt("y", jsv::pv(ins_p, "y")?);
                ins.set("sx", sx);
                ins.set("sy", jsv::orv(jsv::get(sc, "y"), V::Num(1.0)));
                let rot = norm_angle(rot);
                ins.set("rot", rot);
                let sz = jsv::get(sc, "z");
                if jsv::truthy(sz) && !props::strict_eq(sz, Some(&V::Num(1.0))) {
                    ins.set_opt("sz", sz.cloned());
                }
                let cols = js::max(1.0, jsv::or(g("cols"), 1.0));
                let rows = js::max(1.0, jsv::or(g("rows"), 1.0));
                let (c, s) = (rot.jcos(), rot.jsin());
                let mut out: Vec<V> = Vec::new();
                let ix = jsv::to_num(ins.get("x"));
                let iy = jsv::to_num(ins.get("y"));
                let cs = jsv::or(g("colSpacing"), 0.0);
                let rs = jsv::or(g("rowSpacing"), 0.0);
                let mut r = 0.0;
                let mut n = 0.0;
                'rows: while r < rows {
                    let mut k = 0.0;
                    while k < cols {
                        if n >= MAX_ARRAY {
                            break 'rows;
                        }
                        let dx = k * cs;
                        let dy = r * rs;
                        let mut o = ins.clone();
                        o.set("x", ix + dx * c - dy * s);
                        o.set("y", iy + dx * s + dy * c);
                        out.push(V::Obj(o));
                        k += 1.0;
                        n += 1.0;
                    }
                    r += 1.0;
                }
                let mut atts = Vec::new();
                for a in jsv::arr(g("attribs")) {
                    let Some(ao) = jsv::obj(Some(a)) else {
                        return Err(JsErr("Cannot read properties of null (reading 'invisible')".into()));
                    };
                    if jsv::truthy(ao.get("invisible")) {
                        continue;
                    }
                    let mut copy = ao.clone();
                    copy.set("type", "ATTRIB");
                    if let Some(t) = self.convert(&copy)? {
                        atts.push(t);
                    }
                }
                if let Some(V::Obj(first)) = out.first_mut() {
                    first.set("_attribs", V::Arr(atts));
                }
                Some(V::Arr(out))
            }
            "3DFACE" => crate::model3d::face3d_from_dwg(e, &base),
            "MULTILEADER" => {
                let m = self.multileader(e, &base)?;
                m.map(mark)
            }
            "IMAGE" | "WIPEOUT" => {
                let m = self.picture(e, &base);
                m.map(mark)
            }
            "MLINE" => {
                let mut vs = Vec::new();
                for v in jsv::arr(g("vertices")) {
                    let p = jsv::prop(Some(v), "point")?;
                    if p.is_some_and(V::truthy) && jsv::fin(jsv::get(p, "x")).is_some() {
                        vs.push(p);
                    }
                }
                if vs.len() >= 2 {
                    let mut o = mk("poly", &base);
                    o.set("closed", jsv::truthy(g("closed")));
                    o.set("pts", V::Arr(vs.iter().map(|v| ptb(jsv::get(*v, "x").cloned(), jsv::get(*v, "y").cloned())).collect()));
                    Some(V::Obj(o))
                } else {
                    None
                }
            }
            "DIMENSION" => {
                if jsv::str_or_empty(g("layer")).eq_ignore_ascii_case("*LayerNameForDynamicConstraint") {
                    let k = self.t("dwg-import.dimension_parametric_constraint");
                    self.skip(&k);
                    return Ok(None);
                }
                if let Some(d) = self.dimension(e) {
                    let mut o = mk("dim", &base);
                    o.assign(d);
                    if jsv::truthy(g("block")) {
                        o.set_opt("block", g("block").cloned());
                    }
                    return Ok(Some(V::Obj(o)));
                }
                if !jsv::truthy(g("block")) {
                    let k = self.t("dwg-import.dimension_no_block_draw");
                    self.skip(&k);
                    return Ok(None);
                }
                self.inc("dimBlocks", 1.0);
                Some(V::Obj(block_ref(&base, g("block"), true)))
            }
            "ARC_DIMENSION" | "LARGE_RADIAL_DIMENSION" => {
                if !jsv::truthy(g("block")) {
                    self.skip(&format!("{et} (no block to draw it from)"));
                    return Ok(None);
                }
                self.inc("dimBlocks", 1.0);
                Some(V::Obj(block_ref(&base, g("block"), true)))
            }
            "ACAD_TABLE" => {
                if !jsv::truthy(g("block")) {
                    let k = self.t("dwg-import.acad_table_no_block");
                    self.skip(&k);
                    return Ok(None);
                }
                Some(V::Obj(block_ref(&base, g("block"), false)))
            }
            "VIEWPORT" => self.viewport(e, &base),
            "XLINE" | "RAY" => {
                let d = g("direction");
                let n = if jsv::truthy(d) { js::hypot(jsv::to_num(jsv::get(d, "x")), jsv::to_num(jsv::get(d, "y"))) } else { 0.0 };
                let st = g("start");
                if !jsv::truthy(st) || jsv::fin(jsv::get(st, "x")).is_none() || !(n > 1e-12) {
                    self.skip(&et);
                    return Ok(None);
                }
                let mut o = mk(if typ == "XLINE" { "xline" } else { "ray" }, &base);
                o.set_opt("x", jsv::get(st, "x").cloned());
                o.set_opt("y", jsv::get(st, "y").cloned());
                o.set("dx", jsv::to_num(jsv::get(d, "x")) / n);
                o.set("dy", jsv::to_num(jsv::get(d, "y")) / n);
                Some(V::Obj(o))
            }
            "ATTDEF" => None,
            _ => {
                let k =
                    if et == "UNKNOWN" { jsv::to_str(Some(&jsv::orv(g("dxfName"), V::from("UNKNOWN")))) } else { jsv::to_str(g("type")) };
                self.skip(&k);
                None
            }
        })
    }

    fn viewport(&self, e: &Obj, base: &Obj) -> Option<V> {
        let g = |k: &str| e.get(k);
        let c = g("center").filter(|c| c.truthy())?;
        let t = g("viewTarget").filter(|t| t.truthy());
        let dc = g("viewCenter").filter(|t| t.truthy()).unwrap_or(c);
        let w = jsv::to_num(g("width"));
        let h = jsv::to_num(g("height"));
        if !(w > 0.0) || !(h > 0.0) {
            return None;
        }
        let vh = jsv::to_num(g("viewHeight"));
        let s = if vh > 0.0 { h / vh } else { 1.0 };
        let mut vp = Obj::new();
        vp.set("type", "viewport");
        vp.assign(base.clone());
        vp.set_opt("x", jsv::get(Some(c), "x").cloned());
        vp.set_opt("y", jsv::get(Some(c), "y").cloned());
        vp.set_opt("w", g("width").cloned());
        vp.set_opt("h", g("height").cloned());
        let tx = match t {
            Some(t) => jsv::or(jsv::get(Some(t), "x"), 0.0),
            None => 0.0,
        };
        let ty = match t {
            Some(t) => jsv::or(jsv::get(Some(t), "y"), 0.0),
            None => 0.0,
        };
        vp.set("cx", jsv::to_num(jsv::get(Some(dc), "x")) + tx);
        vp.set("cy", jsv::to_num(jsv::get(Some(dc), "y")) + ty);
        vp.set("s", s);
        if let Some(tw) = jsv::fin(g("twist")).filter(|t| t.abs() > 1e-12) {
            vp.set("tw", tw);
        }
        let stt = jsv::u32_of(jsv::to_num(g("status")));
        if stt & 16384 != 0 {
            vp.set("locked", true);
        }
        if stt & 131_072 != 0 {
            vp.set("on", false);
        }
        let mut frz: Vec<String> = Vec::new();
        for h in jsv::arr(g("frozenLayers")) {
            let key = jsv::upper(&jsv::str_or_empty(Some(h)));
            if let Some(Some(V::Str(n))) = self.layer_by_handle.get(&key)
                && !n.is_empty()
                && !frz.contains(n)
            {
                frz.push(n.clone());
            }
        }
        if !frz.is_empty() {
            vp.set("frz", V::Arr(frz.into_iter().map(V::Str).collect()));
        }
        if let Some(lov) = self.vp_overrides.get(&jsv::upper(&jsv::str_or_empty(g("handle")))) {
            vp.set("lov", lov.clone());
        }
        let cc: Vec<Option<&V>> = xdata_of(g("xdata"), "CADCRAFT").iter().map(|it| props::cv(it).1).collect();
        let cci = |i: usize| cc.get(i).copied().flatten();
        if jsv::s(cci(0)) == Some("LAYOUT") && [4, 5, 6, 7].iter().all(|i| jsv::to_num(cci(*i)).is_finite()) {
            let mut o = Obj::new();
            o.set("unit", if jsv::s(cci(2)) == Some("in") { "in" } else { "mm" });
            o.set("key", jsv::str_or_empty(cci(3)));
            o.set("x0", jsv::to_num(cci(4)));
            o.set("y0", jsv::to_num(cci(5)));
            o.set("w", jsv::to_num(cci(6)));
            o.set("h", jsv::to_num(cci(7)));
            vp.set("cc", o);
        }
        Some(V::Obj(vp))
    }

    /// `dimension(e)`: a DWG dimension's defining points → a CadCraft dimension, or None.
    fn dimension(&self, e: &Obj) -> Option<Obj> {
        let g = |k: &str| e.get(k);
        let t = jsv::nn(g("dimType"), 0.0);
        let p = |v: Ov| -> Option<(f64, f64)> {
            if !jsv::truthy(v) {
                return None;
            }
            Some((jsv::fin(jsv::get(v, "x"))?, jsv::fin(jsv::get(v, "y"))?))
        };
        let mut st = self.dim_styles.get(&jsv::upper(&jsv::str_or_empty(g("style")))).cloned().unwrap_or_else(|| self.dim_style.clone());
        let ds = props::dstyle_map(xdata_of(g("xdata"), "ACAD"));
        if !ds.is_empty() {
            props::st_from_codes(&|c| ds.iter().find(|(k, _)| *k == jsv::i2f(c)).and_then(|(_, v)| v.clone()), &mut st);
        }
        let ccs: Vec<String> = xdata_of(g("xdata"), "CADCRAFT").iter().map(|it| jsv::to_str(props::cv(it).1)).collect();
        props::cc_dim_from(&ccs, &mut st);
        if !(jsv::to_num(st.get("scale")) > 0.0) {
            st.set("scale", 1.0);
        }
        lunit_ok(&mut st);
        let meas = jsv::num(g("measurement")).filter(|m| *m > 0.0);
        let close = |v: f64| match meas {
            None => true,
            Some(m) => (v - m).abs() <= 1e-6 * js::max(1.0, m),
        };
        let text = crate::text::drop_measure_marker(mtext_plain(&jsv::str_or_empty(g("text"))));
        let tp = p(g("textMid"));
        let text_at = tp.filter(|t| t.0 != 0.0 || t.1 != 0.0);
        let dist = |a: (f64, f64), b: (f64, f64)| js::hypot(b.0 - a.0, b.1 - a.1);
        let pp = |q: (f64, f64)| jsv::p2(q.0, q.1);
        let mut d = Obj::new();
        if t == 0.0 || t == 1.0 {
            let (p1, p2, q2) = (p(g("xline1"))?, p(g("xline2"))?, p(g("defPoint"))?);
            if dist(p1, p2) < 1e-12 {
                return None;
            }
            if t == 1.0 {
                if !close(dist(p1, p2)) {
                    return None;
                }
                d.set("kind", "aligned");
                d.set("p1", pp(p1));
                d.set("p2", pp(p2));
                d.set("loc", pp(q2));
            } else {
                let mut cands = Vec::new();
                if dist(p2, q2) > 1e-9 {
                    cands.push(norm_angle((q2.1 - p2.1).jatan2(q2.0 - p2.0) + PI / 2.0));
                }
                cands.push(norm_angle(jsv::or(g("angle"), 0.0)));
                cands.push(0.0);
                cands.push(PI / 2.0);
                let rot = cands.into_iter().find(|r| close(((p2.0 - p1.0) * r.jcos() + (p2.1 - p1.1) * r.jsin()).abs()))?;
                d.set("kind", "linear");
                d.set("p1", pp(p1));
                d.set("p2", pp(p2));
                d.set("loc", pp(q2));
                d.set("rot", rot);
            }
        } else if t == 4.0 || t == 3.0 {
            let (a, p1) = (p(g("defPoint"))?, p(g("center"))?);
            let c = if t == 4.0 { a } else { ((a.0 + p1.0) / 2.0, (a.1 + p1.1) / 2.0) };
            let r = dist(c, p1);
            if r < 1e-12 || !close(if t == 4.0 { r } else { 2.0 * r }) {
                return None;
            }
            d.set("kind", if t == 4.0 { "radius" } else { "diameter" });
            d.set("c", pp(c));
            d.set("p1", pp(p1));
            d.set("loc", pp(text_at.unwrap_or(p1)));
        } else if t == 5.0 {
            let (c, p1, p2, loc) = (p(g("center")), p(g("xline1")), p(g("xline2")), p(g("defPoint")));
            match (c, p1, p2, loc) {
                (Some(c), Some(p1), Some(p2), Some(loc)) if dist(c, p1) > 1e-12 && dist(c, p2) > 1e-12 && dist(c, loc) > 1e-12 => {
                    d.set("kind", "angular");
                    d.set("c", pp(c));
                    d.set("p1", pp(p1));
                    d.set("p2", pp(p2));
                    d.set("loc", pp(loc));
                }
                _ => return None,
            }
        } else {
            return None;
        }
        d.set("text", text);
        d.set("st", st);
        if jsv::truthy(g("style")) && !is_standard(g("style")) {
            d.set("dimstyle", jsv::to_str(g("style")));
        }
        if let Some(tr) = jsv::fin(g("textRotation")).filter(|t| t.abs() > 1e-12) {
            d.set("trot", norm_angle(tr));
        }
        Some(d)
    }

    /// `multileader(e, base)` (0.4 C6).
    fn multileader(&mut self, e: &Obj, base: &Obj) -> R<Option<V>> {
        let g = |k: &str| e.get(k);
        let cx = g("context").filter(|c| c.truthy());
        let gc = |k: &str| jsv::get(cx, k);
        let roots = jsv::arr(gc("roots"));
        let ptv = |q: Ov| -> R<V> { Ok(pt(jsv::pv(q, "x")?, jsv::pv(q, "y")?)) };
        let ctext = gc("text");
        let label = if jsv::truthy(gc("hasText")) && jsv::truthy(jsv::get(ctext, "label")) {
            mtext_plain(&jsv::to_str(jsv::get(ctext, "label")))
        } else {
            String::new()
        };
        let asz = jsv::nnv(gc("arrowSize"), jsv::nnv(g("arrowSize"), V::Num(0.0)));
        let h = if jsv::to_num(gc("textHeight")) > 0.0 {
            gc("textHeight").cloned().unwrap_or(V::Null)
        } else {
            jsv::orv(Some(&asz), V::Num(2.5))
        };
        let r0 = roots.first();
        let lines0 = jsv::get(r0, "lines");
        let l0 = jsv::at(lines0, 0);
        let lt = g("leaderType");
        if roots.len() == 1
            && jsv::arr(lines0).len() == 1
            && matches!(lines0, Some(V::Arr(_)))
            && !jsv::truthy(gc("hasBlock"))
            && !label.is_empty()
            && !props::strict_eq(lt, Some(&V::Num(2.0)))
            && jsv::truthy(jsv::get(r0, "connection"))
            && jsv::truthy(jsv::get(l0, "points"))
            && !jsv::arr(jsv::get(l0, "points")).is_empty()
        {
            let mut pts = Vec::new();
            for q in jsv::arr(jsv::get(l0, "points")) {
                pts.push(ptv(Some(q))?);
            }
            pts.push(ptv(jsv::get(r0, "connection"))?);
            let dir = if jsv::nn(jsv::get(jsv::get(r0, "direction"), "x"), 1.0) < 0.0 { -1.0 } else { 1.0 };
            let dogleg = if !matches!(g("doglegEnabled"), Some(V::Bool(false))) {
                jsv::nnv(
                    jsv::get(r0, "landingDistance"),
                    jsv::nnv(jsv::get(r0, "doglegLength"), jsv::nnv(g("landingDistance"), V::Num(0.0))),
                )
            } else {
                V::Num(0.0)
            };
            let tw = jsv::get(ctext, "width");
            let ww = if jsv::to_num(tw) > 0.0 { tw.cloned() } else { None };
            let tls = jsv::get(ctext, "lineSpacing");
            let lsn = jsv::to_num(tls);
            let ls = if lsn > 0.0 && lsn != 1.0 { tls.cloned() } else { None };
            self.inc("mleaders", 1.0);
            let mut o = Obj::new();
            o.set("type", "mleader");
            o.assign(base.clone());
            o.set("pts", V::Arr(pts));
            o.set("dir", dir);
            o.set("s", label);
            o.set("h", h);
            if let Some(w) = ww {
                o.set("ww", w);
            }
            if let Some(l) = ls {
                o.set("ls", l);
            }
            let mut st = Obj::new();
            st.set("asz", asz);
            st.set("dogleg", dogleg);
            if let Some(gap) = jsv::fin(gc("landingGap")) {
                st.set("gap", gap);
            }
            o.set("st", st);
            return Ok(Some(V::Obj(o)));
        }
        let mut parts: Vec<V> = Vec::new();
        let part = |t: &str| {
            let mut o = Obj::new();
            o.set("type", t);
            o.set("layer", "0");
            o.set("color", 0.0);
            o
        };
        let aszn = jsv::to_num(Some(&asz));
        for r in roots {
            let conn = jsv::get(Some(r), "connection");
            for l in jsv::arr(jsv::get(Some(r), "lines")) {
                let mut pts: Vec<V> = Vec::new();
                for q in jsv::arr(jsv::get(Some(l), "points")) {
                    pts.push(ptv(Some(q))?);
                }
                if jsv::truthy(conn) {
                    pts.push(ptv(conn)?);
                }
                if pts.len() < 2 {
                    continue;
                }
                if !props::strict_eq(lt, Some(&V::Num(0.0))) {
                    let mut o = part("poly");
                    o.set("closed", false);
                    o.set(
                        "pts",
                        V::Arr(
                            pts.iter()
                                .map(|p| {
                                    let mut q = jsv::obj(Some(p)).cloned().unwrap_or_default();
                                    q.set("b", 0.0);
                                    V::Obj(q)
                                })
                                .collect(),
                        ),
                    );
                    parts.push(V::Obj(o));
                }
                let xy = |p: Option<&V>| (jsv::to_num(jsv::get(p, "x")), jsv::to_num(jsv::get(p, "y")));
                let (a, b) = (xy(pts.first()), xy(pts.get(1)));
                let d = js::hypot(b.0 - a.0, b.1 - a.1);
                if aszn > 0.0 && d > 0.0 {
                    let (ux, uy) = ((b.0 - a.0) / d, (b.1 - a.1) / d);
                    let (bx, by) = (a.0 + ux * aszn, a.1 + uy * aszn);
                    let hw = aszn / 6.0;
                    let mut o = part("hatch");
                    o.set("solid", true);
                    o.set("loops", V::Arr(vec![nums(&[a.0, a.1, bx - uy * hw, by + ux * hw, bx + uy * hw, by - ux * hw, a.0, a.1])]));
                    parts.push(V::Obj(o));
                }
            }
            let dl = jsv::nnv(
                jsv::get(Some(r), "landingDistance"),
                jsv::nnv(jsv::get(Some(r), "doglegLength"), g("landingDistance").cloned().unwrap_or(V::Null)),
            );
            let rd = jsv::get(Some(r), "direction");
            if jsv::truthy(conn) && jsv::truthy(g("doglegEnabled")) && jsv::truthy(rd) && dl.truthy() {
                let dln = jsv::to_num(Some(&dl));
                let (cxn, cyn) = (jsv::to_num(jsv::get(conn, "x")), jsv::to_num(jsv::get(conn, "y")));
                let mut o = part("line");
                o.set_opt("x1", jsv::get(conn, "x").cloned());
                o.set_opt("y1", jsv::get(conn, "y").cloned());
                o.set("x2", cxn + jsv::to_num(jsv::get(rd, "x")) * dln);
                o.set("y2", cyn + jsv::to_num(jsv::get(rd, "y")) * dln);
                parts.push(V::Obj(o));
            }
        }
        if !label.is_empty() && jsv::truthy(jsv::get(ctext, "location")) {
            let ap = js::max(0.0, jsv::or(jsv::get(ctext, "alignment"), 1.0) - 1.0);
            let td = jsv::get(ctext, "direction");
            let rot = if jsv::truthy(td) && (jsv::truthy(jsv::get(td, "x")) || jsv::truthy(jsv::get(td, "y"))) {
                jsv::to_num(jsv::get(td, "y")).jatan2(jsv::to_num(jsv::get(td, "x")))
            } else {
                0.0
            };
            let loc = jsv::get(ctext, "location");
            let mut o = part("text");
            o.set_opt("x", jsv::get(loc, "x").cloned());
            o.set_opt("y", jsv::get(loc, "y").cloned());
            o.set("h", h.clone());
            o.set("rot", norm_angle(rot));
            o.set("s", label.clone());
            o.set_opt("ha", pick3(ap % 3.0, ["left", "center", "right"]));
            o.set("va", "top");
            if jsv::to_num(jsv::get(ctext, "width")) > 0.0 {
                o.set_opt("ww", jsv::get(ctext, "width").cloned());
            }
            parts.push(V::Obj(o));
        }
        let bc = if jsv::truthy(gc("hasBlock")) { gc("block").filter(|b| b.truthy()) } else { None };
        if let Some(bc) = bc {
            let bh = jsv::get(Some(bc), "blockHandle").filter(|v| !matches!(v, V::Null)).or(jsv::get(Some(bc), "handle"));
            let src = self
                .block_list
                .iter()
                .find(|b| props::strict_eq(jsv::get(Some(b), "record"), bh))
                .or_else(|| self.block_list.iter().find(|b| props::strict_eq(jsv::get(Some(b), "name"), jsv::get(Some(bc), "name"))));
            if let Some(src) = src {
                let mut tags: Vec<(Option<&V>, Option<&V>)> = Vec::new();
                for x in jsv::arr(jsv::get(Some(src), "entities")) {
                    if jsv::s(jsv::get(Some(x), "type")) == Some("ATTDEF") {
                        let hk = jsv::get(Some(x), "handle");
                        let tg = jsv::get(Some(x), "tag");
                        if let Some(slot) = tags.iter_mut().find(|(k, _)| props::strict_eq(*k, hk)) {
                            slot.1 = tg;
                        } else {
                            tags.push((hk, tg));
                        }
                    }
                }
                let mut attrs_o = Obj::new();
                for a in jsv::arr(g("blockLabels")) {
                    let ah = jsv::get(Some(a), "attdefHandle");
                    if let Some((_, tg)) = tags.iter().find(|(k, _)| props::strict_eq(*k, ah)) {
                        attrs_o.set(jsv::to_str(*tg), mtext_plain(&jsv::str_nn(jsv::get(Some(a), "text"))));
                    }
                }
                let t = jsv::get(Some(bc), "matrix");
                let m16 = matches!(t, Some(V::Arr(a)) if a.len() == 16);
                let sx = jsv::orv(jsv::get(jsv::get(Some(bc), "scale"), "x"), V::Num(1.0));
                let sy = jsv::orv(jsv::get(jsv::get(Some(bc), "scale"), "y"), V::Num(1.0));
                let (atx, aty) = if m16 {
                    (jsv::at(t, 3).cloned(), jsv::at(t, 7).cloned())
                } else {
                    let l = jsv::get(Some(bc), "location");
                    (jsv::pv(l, "x")?, jsv::pv(l, "y")?)
                };
                let rot = if m16 {
                    jsv::to_num(jsv::at(t, 4)).jatan2(jsv::to_num(jsv::at(t, 0)))
                } else {
                    jsv::or(jsv::get(Some(bc), "rotation"), 0.0)
                };
                let mut o = part("insert");
                o.set_opt("block", jsv::get(Some(src), "name").cloned());
                o.set_opt("x", atx);
                o.set_opt("y", aty);
                o.set("sx", sx);
                o.set("sy", sy);
                o.set("rot", norm_angle(rot));
                if !attrs_o.is_empty() {
                    o.set("attrs", attrs_o);
                }
                parts.push(V::Obj(o));
            }
        }
        if parts.is_empty() {
            self.skip("MULTILEADER");
            return Ok(None);
        }
        let name = self.free_name("*L");
        let mut b = Obj::new();
        b.set("name", name.clone());
        b.set("base", jsv::p2(0.0, 0.0));
        b.set("entities", V::Arr(parts));
        self.blocks.push(V::Obj(b));
        self.inc("mleaderBlocks", 1.0);
        Ok(Some(V::Obj(block_ref(base, Some(&V::Str(name)), false))))
    }

    /// `picture(e, base)`: an IMAGE → its frame + file name as one unnamed block; a WIPEOUT → its frame (0.4 C6).
    fn picture(&mut self, e: &Obj, base: &Obj) -> Option<V> {
        let g = |k: &str| e.get(k);
        let (u, v, p) = (g("u"), g("v"), g("insert"));
        let size = g("size");
        let w = jsv::to_num(jsv::get(size, "x"));
        let h = jsv::to_num(jsv::get(size, "y"));
        if !jsv::truthy(u) || !jsv::truthy(v) || !jsv::truthy(p) || !(w > 0.0) || !(h > 0.0) {
            let k = jsv::to_str(g("type"));
            self.skip(&k);
            return None;
        }
        let n = |o: Ov, k: &str| jsv::to_num(jsv::get(o, k));
        let (px, py, ux, uy, vx, vy) = (n(p, "x"), n(p, "y"), n(u, "x"), n(u, "y"), n(v, "x"), n(v, "y"));
        let wp = |qx: f64, qy: f64| {
            V::Obj(cc_io_dwg::obj! {
                "x" => px + (qx + 0.5) * ux + (h - 0.5 - qy) * vx,
                "y" => py + (qx + 0.5) * uy + (h - 0.5 - qy) * vy,
                "b" => 0.0
            })
        };
        let clip_list = jsv::arr(g("clip"));
        let user_clip = jsv::truthy(g("clipping")) && matches!(g("clip"), Some(V::Arr(_))) && clip_list.len() >= 2;
        let mut clip: Vec<(f64, f64)> = if user_clip {
            clip_list.iter().map(|q| (n(Some(q), "x"), n(Some(q), "y"))).collect()
        } else {
            vec![(-0.5, -0.5), (w - 0.5, h - 0.5)]
        };
        if let [a, b] = clip.as_slice() {
            clip = vec![(a.0, a.1), (b.0, a.1), (b.0, b.1), (a.0, b.1)];
        }
        let pts: Vec<V> = clip.iter().map(|q| wp(q.0, q.1)).collect();
        if jsv::s(g("type")) == Some("WIPEOUT") {
            self.inc("wipeouts", 1.0);
            let mut o = Obj::new();
            o.set("type", "poly");
            o.set("closed", true);
            o.set("pts", V::Arr(pts));
            o.assign(base.clone());
            return Some(V::Obj(o));
        }
        let def = self.image_defs.iter().rev().find(|(h, _)| props::strict_eq(Some(h), g("def"))).map(|(_, d)| *d);
        let fname = js::trim(&jsv::str_or_empty(jsv::get(def, "fileName"))).to_string();
        let file = if fname.is_empty() { "image".to_string() } else { fname };
        let mut pic = Obj::new();
        pic.set("file", file);
        pic.set_opt("x", jsv::get(p, "x").cloned());
        pic.set_opt("y", jsv::get(p, "y").cloned());
        pic.set_opt("ux", jsv::get(u, "x").cloned());
        pic.set_opt("uy", jsv::get(u, "y").cloned());
        pic.set_opt("vx", jsv::get(v, "x").cloned());
        pic.set_opt("vy", jsv::get(v, "y").cloned());
        pic.set_opt("w", jsv::get(size, "x").cloned());
        pic.set_opt("h", jsv::get(size, "y").cloned());
        if jsv::to_num(g("fade")) > 0.0 {
            pic.set_opt("fade", g("fade").cloned());
        }
        let name = self.free_name("*I");
        let blk = small::picture_block(&name, pic, if user_clip { Some(&pts) } else { None });
        self.blocks.push(V::Obj(blk));
        self.inc("images", 1.0);
        Some(V::Obj(block_ref(base, Some(&V::Str(name)), false)))
    }
}

/// `['a', 'b', 'c'][i]` (undefined unless i is 0, 1 or 2).
fn pick3(i: f64, names: [&str; 3]) -> Option<V> {
    if i == 0.0 {
        Some(V::from(names[0]))
    } else if i == 1.0 {
        Some(V::from(names[1]))
    } else if i == 2.0 {
        Some(V::from(names[2]))
    } else {
        None
    }
}

/// `/^\\Q(-?[\d.]+);/.exec(raw)` → the oblique angle in radians (0 when absent or zero).
fn q_oblique(raw: &str) -> f64 {
    let Some(rest) = raw.strip_prefix("\\Q") else { return 0.0 };
    let (neg, body) = match rest.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, rest),
    };
    let n = body.bytes().take_while(|c| c.is_ascii_digit() || *c == b'.').count();
    if n == 0 || body.as_bytes().get(n) != Some(&b';') {
        return 0.0;
    }
    let lit = format!("{}{}", if neg { "-" } else { "" }, body.get(..n).unwrap_or(""));
    let v = js::str_to_number(&lit);
    if v.is_finite() && v != 0.0 { v * PI / 180.0 } else { 0.0 }
}

fn mark(v: V) -> V {
    match v {
        V::Obj(mut o) => {
            o.set(EXTRA_MARK, true);
            V::Obj(o)
        }
        other => other,
    }
}

/// `{ type: 'insert', ...base, block, x: 0, y: 0, sx: 1, sy: 1, rot: 0, dim? }`
fn block_ref(base: &Obj, block: Ov, dim: bool) -> Obj {
    let mut o = Obj::new();
    o.set("type", "insert");
    o.assign(base.clone());
    o.set_opt("block", block.cloned());
    o.set("x", 0.0);
    o.set("y", 0.0);
    o.set("sx", 1.0);
    o.set("sy", 1.0);
    o.set("rot", 0.0);
    if dim {
        o.set("dim", true);
    }
    o
}

/// `zOf(e, type)`: the object's elevation as CadCraft keeps it.
fn z_of(e: &Obj, typ: &str) -> f64 {
    let fin = |v: Ov| jsv::fin(v).unwrap_or(0.0);
    let g = |k: &str| e.get(k);
    match jsv::s(g("type")).unwrap_or("") {
        "CIRCLE" | "ARC" | "ELLIPSE" => fin(jsv::get(g("center"), "z")),
        "POINT" => fin(jsv::get(g("position"), "z")),
        "TEXT" | "ATTRIB" => {
            let z = fin(jsv::get(g("insert"), "z"));
            if z != 0.0 { z } else { fin(g("elevation")) }
        }
        "MTEXT" => fin(jsv::get(g("insert"), "z")),
        "INSERT" if typ == "insert" => fin(jsv::get(g("insert"), "z")),
        "LWPOLYLINE" | "POLYLINE_2D" | "HATCH" => fin(g("elevation")),
        "MULTILEADER" if typ == "mleader" => {
            fin(jsv::get(jsv::at(jsv::get(g("context"), "roots"), 0).and_then(|r| jsv::get(Some(r), "connection")), "z"))
        }
        _ => 0.0,
    }
}

/// `layerOverrides(db, ltName)` (0.5 VP.3): viewport handle → { LayerName: { color?, lt?, lw?, tr? } }.
fn layer_overrides(db: &Obj, lt_name: &dyn Fn(Ov) -> Option<String>) -> HashMap<String, Obj> {
    let mut out: HashMap<String, Obj> = HashMap::new();
    let mut objs: HashMap<String, &V> = HashMap::new();
    for o in jsv::arr(db.get("objects")) {
        objs.insert(jsv::upper(&jsv::to_str(jsv::get(Some(o), "handle"))), o);
    }
    let mut put = |vp: Ov, layer: &str, k: &str, v: V| {
        let key = jsv::upper(&jsv::str_or_empty(vp));
        if key.is_empty() {
            return;
        }
        let m = out.entry(key).or_default();
        if !matches!(m.get(layer), Some(V::Obj(_))) {
            m.set(layer.to_string(), Obj::new());
        }
        if let Some(V::Obj(lo)) = m.get_mut(layer) {
            lo.set(k.to_string(), v);
        }
    };
    for l in jsv::arr(jsv::get(db.get("tables"), "layers")) {
        let ed = jsv::get(Some(l), "extDict");
        let d = if jsv::truthy(ed) { objs.get(&jsv::upper(&jsv::to_str(ed))).copied() } else { None };
        let lname = jsv::to_str(jsv::get(Some(l), "name"));
        let Some(V::Obj(entries)) = jsv::get(d, "entries") else { continue };
        for name in jsv::keys_js(entries) {
            let which = name
                .strip_prefix("ADSK_XREC_LAYER_")
                .and_then(|r| r.strip_suffix("_OVR"))
                .filter(|w| matches!(*w, "COLOR" | "LINETYPE" | "LINEWT" | "ALPHA"));
            let Some(which) = which else { continue };
            let Some(x) = objs.get(&jsv::upper(&jsv::to_str(entries.get(name)))).copied() else { continue };
            if jsv::s(jsv::get(Some(x), "type")) != Some("XRECORD") {
                continue;
            }
            let mut vp: Option<V> = None;
            for it in jsv::arr(jsv::get(Some(x), "data")) {
                let (c, v) = props::cv(it);
                let code = jsv::num(c);
                if code == Some(335.0) {
                    vp = v.cloned();
                    continue;
                }
                if !vp.as_ref().is_some_and(V::truthy) {
                    continue;
                }
                let vpr = vp.as_ref();
                if which == "COLOR" && (code == Some(420.0) || code == Some(62.0)) {
                    let w = jsv::u32_of(jsv::to_num(v));
                    let how = if code == Some(62.0) { 0xc3 } else { w >> 24 };
                    if how == 0xc2 {
                        put(vpr, &lname, "color", V::Str(format!("#{:06x}", w & 0xff_ffff)));
                    } else if how == 0xc3 || code == Some(62.0) {
                        let i = if code == Some(62.0) { jsv::to_num(v).abs() } else { jsv::i2f(i64::try_from(w & 0xff).unwrap_or(0)) };
                        if (1.0..=255.0).contains(&i) {
                            put(vpr, &lname, "color", V::Num(i));
                        }
                    }
                } else if which == "LINETYPE" && code == Some(343.0) {
                    if let Some(n) = lt_name(v).filter(|n| !n.is_empty()) {
                        put(vpr, &lname, "lt", V::Str(n));
                    }
                } else if which == "LINEWT" && code == Some(91.0) {
                    let n = jsv::to_num(v);
                    if (0.0..=211.0).contains(&n) {
                        put(vpr, &lname, "lw", V::Num(n / 100.0));
                    }
                } else if which == "ALPHA" && code == Some(440.0) {
                    if let Some(tr) = props::tr_from(Some(&V::Num(jsv::to_num(v)))) {
                        put(vpr, &lname, "tr", tr);
                    }
                } else {
                    continue;
                }
                vp = None;
            }
        }
    }
    out
}

/// `splitScreen(list)`: the VPORT table's *Active records → doc.vports or null.
fn split_screen(list: Ov) -> V {
    let recs: Vec<V> = jsv::arr(list)
        .iter()
        .filter(|v| v.truthy() && jsv::str_or_empty(jsv::get(Some(v), "name")).eq_ignore_ascii_case("*active"))
        .map(|v| {
            let g = |k: &str| jsv::get(Some(v), k);
            let sheet =
                xdata_of(g("xdata"), "CADCRAFT").iter().map(|it| jsv::to_str(props::cv(it).1)).find(|s| s.starts_with("PANESHEET:"));
            let mut o = Obj::new();
            o.set_opt("x0", jsv::get(g("lowerLeft"), "x").cloned());
            o.set_opt("y0", jsv::get(g("lowerLeft"), "y").cloned());
            o.set_opt("x1", jsv::get(g("upperRight"), "x").cloned());
            o.set_opt("y1", jsv::get(g("upperRight"), "y").cloned());
            o.set_opt("cx", jsv::get(g("viewCenter"), "x").filter(|x| !matches!(x, V::Null)).or(jsv::get(g("center"), "x")).cloned());
            o.set_opt("cy", jsv::get(g("viewCenter"), "y").filter(|x| !matches!(x, V::Null)).or(jsv::get(g("center"), "y")).cloned());
            o.set_opt("h", g("viewHeight").cloned());
            o.set("tw", jsv::fin(g("twist")).unwrap_or(0.0));
            if let Some(s) = sheet {
                o.set("sheet", crate::text::slice16(&s, 10, usize::MAX));
            }
            V::Obj(o)
        })
        .collect();
    crate::vports::vports_from(&recs).map_or(V::Null, V::Obj)
}

/// `ucsFrom(H, table, vports)` (DR.8).
fn ucs_from(h: Ov, table: Ov, vports: Ov, out: &mut Obj) {
    let ucs_of = |o: Ov, x: Ov| -> Option<(f64, f64, f64)> {
        if !jsv::truthy(o) || !jsv::truthy(x) {
            return None;
        }
        let (ox, oy) = (jsv::fin(jsv::get(o, "x"))?, jsv::fin(jsv::get(o, "y"))?);
        if !jsv::truthy(jsv::get(x, "x")) && !jsv::truthy(jsv::get(x, "y")) {
            return None;
        }
        Some((ox, oy, jsv::to_num(jsv::get(x, "y")).jatan2(jsv::to_num(jsv::get(x, "x")))))
    };
    let mut current = ucs_of(jsv::get(h, "UCSORG"), jsv::get(h, "UCSXDIR"));
    if let Some(c) = current
        && c.0.abs() < 1e-12
        && c.1.abs() < 1e-12
        && c.2.abs() < 1e-12
    {
        current = None;
    }
    let named: Vec<V> = jsv::arr(table)
        .iter()
        .filter_map(|u| {
            let c = ucs_of(jsv::get(Some(u), "origin"), jsv::get(Some(u), "xAxis"))?;
            let name = jsv::get(Some(u), "name").filter(|n| n.truthy())?;
            Some(V::Obj(cc_io_dwg::obj! { "name" => jsv::to_str(Some(name)), "x" => c.0, "y" => c.1, "ang" => c.2 }))
        })
        .collect();
    let act = jsv::arr(vports).iter().find(|v| v.truthy() && jsv::str_or_empty(jsv::get(Some(v), "name")).eq_ignore_ascii_case("*active"));
    let tw = jsv::fin(act.and_then(|a| jsv::get(Some(a), "twist"))).unwrap_or(0.0);
    if current.is_some() || !named.is_empty() {
        let cur = current.map_or(V::Null, |c| V::Obj(cc_io_dwg::obj! { "x" => c.0, "y" => c.1, "ang" => c.2 }));
        out.set("ucs", V::Obj(cc_io_dwg::obj! { "current" => cur, "named" => V::Arr(named) }));
    }
    if tw != 0.0 {
        out.set("viewTwist", tw);
    }
}

/// `namedViews(list, layouts)`: the VIEW table → doc.views.
fn named_views(list: Ov, layouts: &[V]) -> V {
    let mut out = Vec::new();
    for v in jsv::arr(list) {
        let g = |k: &str| jsv::get(Some(v), k);
        let c = g("center");
        if !v.truthy()
            || !jsv::truthy(g("name"))
            || !(jsv::to_num(g("width")) > 0.0)
            || !(jsv::to_num(g("height")) > 0.0)
            || jsv::fin(jsv::get(c, "x")).is_none()
            || jsv::fin(jsv::get(c, "y")).is_none()
        {
            continue;
        }
        let t = g("target").filter(|t| t.truthy());
        let on_paper = jsv::truthy(g("paperSpace"));
        if on_paper && layouts.is_empty() {
            continue;
        }
        let mut o = Obj::new();
        o.set("name", jsv::to_str(g("name")));
        o.set("cx", jsv::to_num(jsv::get(c, "x")) + jsv::or(jsv::get(t, "x"), 0.0));
        o.set("cy", jsv::to_num(jsv::get(c, "y")) + jsv::or(jsv::get(t, "y"), 0.0));
        o.set_opt("w", g("width").cloned());
        o.set_opt("h", g("height").cloned());
        if on_paper {
            o.set_opt("layout", layouts.first().and_then(|l| jsv::get(Some(l), "name")).cloned());
        }
        out.push(V::Obj(o));
    }
    V::Arr(out)
}

/// `lengthFormat(H)`: $LUNITS / $LUPREC → meta.lunits / luprec (decimal left out).
fn length_format(h: Ov, meta: &mut Obj) {
    let lu = js::round(jsv::to_num(jsv::get(h, "LUNITS")));
    let pr = js::round(jsv::to_num(jsv::get(h, "LUPREC")));
    if (1.0..=5.0).contains(&lu) && lu != 2.0 {
        meta.set("lunits", lu);
        if (0.0..=8.0).contains(&pr) {
            meta.set("luprec", pr);
        }
    }
}

/// `importDwg(db, { versionTag })` → the doc.load input as JSON (private marks left out), or the JS exception.
pub fn import_dwg(db: &Obj, version_tag: &str) -> Result<serde_json::Value, JsErr> {
    Ok(jsv::into_json(V::Obj(import_dwg_v(db, version_tag, Catalog::en())?)))
}

/// `importDwg` on dynamic values (multileaders / pictures / wipeouts keep their [`EXTRA_MARK`]).
pub fn import_dwg_v(db: &Obj, version_tag: &str, cat: &Catalog) -> R<Obj> {
    let tables = db.get("tables");
    let tget = |k: &str| jsv::get(tables, k);
    let mut report = Obj::new();
    report.set("read", 0.0);
    report.set("skipped", Obj::new());
    report.set("paperSpace", 0.0);
    report.set("notes", V::Arr(Vec::new()));
    report.set("splines", 0.0);

    // linetypes (0.4 C8)
    let mut lt_by_handle: HashMap<String, Option<V>> = HashMap::new();
    for l in jsv::arr(tget("linetypes")) {
        lt_by_handle.insert(jsv::upper(&jsv::str_or_empty(jsv::get(Some(l), "handle"))), jsv::get(Some(l), "name").cloned());
    }
    let linetypes: Vec<V> = jsv::arr(tget("linetypes"))
        .iter()
        .filter(|l| {
            let n = jsv::get(Some(l), "name");
            let ns = jsv::to_str(n);
            jsv::truthy(n)
                && !["byblock", "bylayer", "continuous"].iter().any(|x| ns.eq_ignore_ascii_case(x))
                && jsv::truthy(jsv::get(Some(l), "elements"))
                && !jsv::arr(jsv::get(Some(l), "elements")).is_empty()
        })
        .map(|l| {
            let g = |k: &str| jsv::get(Some(l), k);
            let pattern: Vec<V> = jsv::arr(g("elements")).iter().map(|x| V::Num(jsv::pn_or(jsv::get(Some(x), "length"), 0.0))).collect();
            let mut o = Obj::new();
            o.set_opt("name", g("name").cloned());
            o.set("description", jsv::orv(g("description"), V::from("")));
            o.set("pattern", V::Arr(pattern));
            V::Obj(o)
        })
        .collect();
    let mut layer_by_handle: HashMap<String, Option<V>> = HashMap::new();
    for l in jsv::arr(tget("layers")) {
        layer_by_handle.insert(jsv::upper(&jsv::str_or_empty(jsv::get(Some(l), "handle"))), jsv::get(Some(l), "name").cloned());
    }

    let mut imp = Imp {
        db,
        cat,
        report,
        lt_by_handle,
        layer_by_handle,
        vp_overrides: HashMap::new(),
        dim_style: Obj::new(),
        dim_styles: HashMap::new(),
        blocks: Vec::new(),
        anon_n: 0,
        taken: HashSet::new(),
        image_defs: Vec::new(),
        obj_by_h: HashMap::new(),
        block_list: jsv::arr(db.get("blocks")),
    };
    let vpo = layer_overrides(db, &|v| imp.lt_name(v));
    imp.vp_overrides = vpo;
    let layers: Vec<V> = jsv::arr(tget("layers"))
        .iter()
        .map(|l| {
            let g = |k: &str| jsv::get(Some(l), k);
            let c = g("color").filter(|c| c.truthy());
            let color = if let Some(rgb) = jsv::get(c, "rgb") {
                V::Str(hex(Some(rgb)))
            } else {
                let a = jsv::nn(jsv::get(c, "index"), 7.0).abs();
                V::Num(if a != 0.0 && !a.is_nan() { a } else { 7.0 })
            };
            let lw = lw_mm(g("lineweight"));
            let mut o = Obj::new();
            o.set_opt("name", g("name").cloned());
            o.set("color", color);
            o.set("on", !jsv::truthy(g("off")));
            o.set("frozen", jsv::truthy(g("frozen")));
            o.set("locked", jsv::truthy(g("locked")));
            o.set("plot", !matches!(g("plot"), Some(V::Bool(false))) && jsv::upper(&jsv::to_str(g("name"))) != "DEFPOINTS");
            o.set("linetype", imp.lt_name(g("linetype")).filter(|n| !n.is_empty()).unwrap_or_else(|| "Continuous".into()));
            o.set(
                "lineweight",
                match lw {
                    None => -3.0,
                    Some(-2.0) => -3.0,
                    Some(w) => w,
                },
            );
            if jsv::truthy(g("frozenInNewViewports")) {
                o.set("newVpFrozen", true);
            }
            V::Obj(o)
        })
        .collect();

    // dimension styles: the header's settings, then each named style
    let hv = db.get("header");
    let style_from = |src: Ov, base: &Obj| -> Obj {
        let mut st = base.clone();
        props::st_from_vars(src, &mut st);
        if !(jsv::to_num(st.get("scale")) > 0.0) {
            st.set("scale", 1.0);
        }
        for k in ["dec", "adec", "tad", "zin"] {
            let v = js::max(0.0, js::min(8.0, js::round(jsv::or(st.get(k), 0.0))));
            st.set(k, v);
        }
        lunit_ok(&mut st);
        st
    };
    let dim_style = style_from(hv, &props::dim_defaults());
    let mut dim_styles: HashMap<String, Obj> = HashMap::new();
    for d in jsv::arr(tget("dimstyles")) {
        let vars = jsv::get(Some(d), "vars").filter(|v| v.truthy());
        dim_styles.insert(jsv::upper(&jsv::to_str(jsv::get(Some(d), "name"))), style_from(vars, &dim_style));
    }
    for d in jsv::arr(tget("dimstyles")) {
        let key = jsv::upper(&jsv::to_str(jsv::get(Some(d), "name")));
        let tx = jsv::get(jsv::get(Some(d), "vars"), "DIMTXSTY_NAME");
        if let Some(st) = dim_styles.get_mut(&key)
            && jsv::truthy(tx)
            && !is_standard(tx)
        {
            st.set("txsty", jsv::to_str(tx));
        }
    }
    let named_dim_styles: Vec<V> = jsv::arr(tget("dimstyles"))
        .iter()
        .filter(|d| {
            let n = jsv::get(Some(d), "name");
            jsv::truthy(n) && !is_standard(n) && jsv::i32_of(jsv::to_num(jsv::get(Some(d), "flags"))) & 16 == 0
        })
        .map(|d| {
            let n = jsv::to_str(jsv::get(Some(d), "name"));
            let st = dim_styles.get(&jsv::upper(&n)).cloned().map_or(V::Null, V::Obj);
            V::Arr(vec![V::Str(n), st])
        })
        .collect();
    let text_styles: Vec<V> = jsv::arr(tget("styles"))
        .iter()
        .filter(|s| {
            let g = |k: &str| jsv::get(Some(s), k);
            let fl = jsv::i32_of(jsv::to_num(g("flags")));
            jsv::truthy(g("name")) && !jsv::truthy(g("shapeFile")) && fl & 1 == 0 && fl & 16 == 0 && !is_standard(g("name"))
        })
        .map(|s| {
            let g = |k: &str| jsv::get(Some(s), k);
            let mut o = Obj::new();
            o.set("font", jsv::str_or_empty(g("font")));
            o.set("h", jsv::fin(g("height")).unwrap_or(0.0));
            o.set("wf", if jsv::to_num(g("widthFactor")) > 0.0 { g("widthFactor").cloned().unwrap_or(V::Num(1.0)) } else { V::Num(1.0) });
            o.set("obl", jsv::fin(g("oblique")).unwrap_or(0.0));
            V::Arr(vec![V::Str(jsv::to_str(g("name"))), V::Obj(o)])
        })
        .collect();
    imp.dim_style = dim_style.clone();
    imp.dim_styles = dim_styles;

    // model space and every sheet are blocks too, in file order
    let block_list = imp.block_list;
    let model_i = block_list.iter().position(|b| jsv::to_str(jsv::get(Some(b), "name")).eq_ignore_ascii_case("*model_space"));
    for b in block_list {
        imp.taken.insert(jsv::upper(&jsv::to_str(jsv::get(Some(b), "name"))));
    }
    for o in jsv::arr(db.get("objects")) {
        if jsv::s(jsv::get(Some(o), "type")) == Some("IMAGEDEF") {
            imp.image_defs.push((jsv::get(Some(o), "handle").cloned().unwrap_or(V::Null), o));
        }
        imp.obj_by_h.insert(jsv::to_str(jsv::get(Some(o), "handle")), o);
    }
    // image_defs lookup takes the last of equal handles (a Map), see picture()
    let records = jsv::arr(tget("blockRecords"));
    let mut parametric: Option<String> = None;
    for b in block_list {
        let Some(bo) = jsv::obj(Some(b)) else { continue };
        let name = jsv::to_str(bo.get("name"));
        let lname = name.to_ascii_lowercase();
        if lname.starts_with("*model_space") || lname.starts_with("*paper_space") {
            continue;
        }
        let ents = jsv::arr(bo.get("entities"));
        if small::is_param_block(bo.get("name")) {
            let mut conv = Vec::new();
            for e in ents {
                if let Some(eo) = jsv::obj(Some(e))
                    && let Some(c) = imp.convert(eo)?
                    && c.truthy()
                {
                    conv.push(c);
                }
            }
            parametric = Some(small::parametric_text(&conv));
            continue;
        }
        let base_v = {
            let mut o = Obj::new();
            o.set("x", jsv::orv(jsv::get(bo.get("base"), "x"), V::Num(0.0)));
            o.set("y", jsv::orv(jsv::get(bo.get("base"), "y"), V::Num(0.0)));
            V::Obj(o)
        };
        let def_name = if jsv::truthy(bo.get("dynamicOf")) {
            block_list
                .iter()
                .find(|x| props::strict_eq(jsv::get(Some(x), "record"), bo.get("dynamicOf")))
                .and_then(|x| jsv::get(Some(x), "name"))
                .filter(|n| n.truthy())
                .map(|n| jsv::to_str(Some(n)))
        } else {
            None
        };
        let record =
            records.iter().find(|r| props::strict_eq(jsv::get(Some(r), "handle"), bo.get("record"))).and_then(|r| jsv::obj(Some(r)));
        let mut conv_err: Option<JsErr> = None;
        let dyn_b = {
            let imp_ref = &mut imp;
            let mut conv = |e: &Obj| -> Option<V> {
                if conv_err.is_some() {
                    return None;
                }
                match imp_ref.convert(e) {
                    Ok(c) => c,
                    Err(err) => {
                        conv_err = Some(err);
                        None
                    }
                }
            };
            crate::dyn_blocks::dyn_block_from_dwg(bo, &mut conv, &attdef_of, def_name, record)
        };
        if let Some(err) = conv_err {
            return Err(err);
        }
        if let Some(db_) = dyn_b {
            let mut o = Obj::new();
            o.set("name", bo.get("name").cloned().unwrap_or(V::Null));
            o.set("base", base_v);
            o.set("entities", V::Arr(db_.entities));
            if !db_.attdefs.is_empty() {
                o.set("attdefs", V::Arr(db_.attdefs));
            }
            let is_def = db_.dyn_.has("def");
            o.set("dyn", db_.dyn_);
            imp.blocks.push(V::Obj(o));
            imp.inc("dynamicBlocks", if is_def { 1.0 } else { 0.0 });
            continue;
        }
        let mut out_ents = Vec::new();
        let mut attdefs = Vec::new();
        for e in ents {
            let Some(eo) = jsv::obj(Some(e)) else { continue };
            if jsv::s(eo.get("type")) == Some("ATTDEF") {
                if let Some(d) = attdef_of(eo) {
                    attdefs.push(V::Obj(d));
                }
                continue;
            }
            if let Some(c) = imp.convert(eo)? {
                out_ents.extend(jsv::flat1(c));
            }
        }
        let xref = jsv::i32_of(jsv::to_num(bo.get("flags"))) & 4 != 0;
        let mut o = Obj::new();
        o.set("name", bo.get("name").cloned().unwrap_or(V::Null));
        o.set("base", base_v);
        o.set("entities", V::Arr(if xref { Vec::new() } else { out_ents }));
        if !attdefs.is_empty() && !xref {
            o.set("attdefs", V::Arr(attdefs));
        }
        if xref {
            imp.inc("xrefs", 1.0);
            let path = jsv::to_str(Some(&jsv::orv(bo.get("xrefPath"), bo.get("name").cloned().unwrap_or(V::Null))));
            o.set("xref", V::Obj(cc_io_dwg::obj! { "path" => path, "loaded" => false }));
        }
        imp.blocks.push(V::Obj(o));
    }

    let mut entities: Vec<V> = Vec::new();
    if let Some(mi) = model_i {
        for e in jsv::arr(block_list.get(mi).and_then(|b| jsv::get(Some(b), "entities"))) {
            let Some(eo) = jsv::obj(Some(e)) else { continue };
            if let Some(c) = imp.convert(eo)?
                && c.truthy()
            {
                entities.extend(jsv::flat1(c));
                imp.inc("read", 1.0);
            }
        }
    }
    let layout_objs: Vec<&V> = jsv::arr(db.get("layouts"))
        .iter()
        .filter(|l| !jsv::str_or_empty(jsv::get(Some(l), "name")).eq_ignore_ascii_case("model") && jsv::truthy(jsv::get(Some(l), "block")))
        .collect();
    // paperOf: layout block handle → entity list (one list per distinct handle)
    let mut paper_keys: Vec<V> = Vec::new();
    for l in &layout_objs {
        let k = jsv::get(Some(l), "block").cloned().unwrap_or(V::Null);
        if !paper_keys.iter().any(|x| props::strict_eq(Some(x), Some(&k))) {
            paper_keys.push(k);
        }
    }
    let mut paper_lists: Vec<Vec<V>> = paper_keys.iter().map(|_| Vec::new()).collect();
    for (bi, b) in block_list.iter().enumerate() {
        let rec = jsv::get(Some(b), "record");
        let Some(pi) = paper_keys.iter().position(|k| props::strict_eq(Some(k), rec)) else { continue };
        if Some(bi) == model_i {
            continue;
        }
        for e in jsv::arr(jsv::get(Some(b), "entities")) {
            let Some(eo) = jsv::obj(Some(e)) else {
                imp.inc("paperSpace", 1.0);
                continue;
            };
            let c = imp.convert(eo)?;
            if let Some(c) = c
                && c.truthy()
                && let Some(list) = paper_lists.get_mut(pi)
            {
                list.extend(jsv::flat1(c));
            }
            imp.inc("paperSpace", 1.0);
        }
    }
    // attribute values: positions relative to their block (attrs.js)
    let mut info: BlockInfo = HashMap::new();
    for b in &imp.blocks {
        if let Some(bo) = jsv::obj(Some(b)) {
            info.insert(jsv::to_str(bo.get("name")), attrs::block_info(bo));
        }
    }
    {
        let mut lists: Vec<&mut Vec<V>> = Vec::new();
        lists.push(&mut entities);
        for b in imp.blocks.iter_mut() {
            if let V::Obj(bo) = b
                && let Some(V::Arr(l)) = bo.get_mut("entities")
            {
                lists.push(l);
            }
        }
        for l in paper_lists.iter_mut() {
            lists.push(l);
        }
        attrs::resolve_attribs(&mut lists, &info);
    }
    for b in imp.blocks.iter_mut() {
        sync_dyn_all(b);
    }
    let note = |imp: &mut Imp, k: &str, key: &str| {
        let n = jsv::to_num(imp.report.get(k));
        if imp.report.get(k).is_some_and(V::truthy) {
            let s = imp.cat.t(key, &i18n::n(n));
            if let Some(V::Arr(notes)) = imp.report.get_mut("notes") {
                notes.push(V::Str(s));
            }
        }
    };
    note(&mut imp, "splines", "dwg.read.splines");
    note(&mut imp, "xrefs", "dwg.read.xrefs");
    note(&mut imp, "mleaderBlocks", "dwg.read.mleaderblocks");
    note(&mut imp, "wipeouts", "dwg.read.wipeouts");
    note(&mut imp, "images", "dwg.read.images");
    note(&mut imp, "dimBlocks", "dwg.read.dimblocks");

    // layouts, in tab order; the first viewport of each is the sheet itself
    let insunits = jsv::get(hv, "INSUNITS");
    let meas = jsv::get(hv, "MEASUREMENT");
    let inches =
        jsv::num(meas) == Some(0.0) || (meas.is_none() && jsv::num(insunits).is_some_and(|u| [1.0, 2.0, 3.0, 8.0, 9.0, 10.0].contains(&u)));
    let mut sorted = layout_objs.clone();
    sorted.sort_by(|a, b| {
        let d = jsv::nn(jsv::get(Some(a), "tabOrder"), 0.0) - jsv::nn(jsv::get(Some(b), "tabOrder"), 0.0);
        if d < 0.0 {
            std::cmp::Ordering::Less
        } else if d > 0.0 {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    let mut layouts: Vec<V> = Vec::new();
    for l in sorted {
        let g = |k: &str| jsv::get(Some(l), k);
        let k = g("block");
        let mut list: Vec<V> =
            paper_keys.iter().position(|x| props::strict_eq(Some(x), k)).and_then(|i| paper_lists.get(i)).cloned().unwrap_or_default();
        let first_vp = list.iter().position(|e| jsv::s(jsv::get(Some(e), "type")) == Some("viewport"));
        let first = first_vp.map(|i| list.remove(i));
        let lim = g("limits");
        let mn = jsv::get(lim, "min").filter(|v| v.truthy());
        let mx = jsv::get(lim, "max").filter(|v| v.truthy());
        let pnum = |p: Option<&V>, k: &str| match p {
            Some(p) => jsv::to_num(jsv::get(Some(p), k)),
            None => 0.0,
        };
        let unit = if inches { "in" } else { "mm" };
        let (mut x0, mut y0, mut w, mut h) = (pnum(mn, "x"), pnum(mn, "y"), pnum(mx, "x") - pnum(mn, "x"), pnum(mx, "y") - pnum(mn, "y"));
        let mut unit_v = V::from(unit);
        let own = first.as_ref().and_then(|f| jsv::get(Some(f), "cc")).filter(|c| c.truthy());
        let ow = jsv::to_num(jsv::get(own, "w"));
        let oh = jsv::to_num(jsv::get(own, "h"));
        if own.is_some() && ow > 0.0 && oh > 0.0 {
            unit_v = jsv::get(own, "unit").cloned().unwrap_or(V::Null);
            x0 = jsv::to_num(jsv::get(own, "x0"));
            y0 = jsv::to_num(jsv::get(own, "y0"));
            w = ow;
            h = oh;
        } else if !(w > 0.0 && h > 0.0) {
            match first.as_ref() {
                Some(f) => {
                    let fn_ = |k: &str| jsv::to_num(jsv::get(Some(f), k));
                    x0 = fn_("x") - fn_("w") / 2.0;
                    y0 = fn_("y") - fn_("h") / 2.0;
                    w = fn_("w");
                    h = fn_("h");
                }
                None => {
                    x0 = 0.0;
                    y0 = 0.0;
                    w = if inches { 11.0 } else { 297.0 };
                    h = if inches { 8.5 } else { 210.0 };
                }
            }
        }
        let mut paper = Obj::new();
        paper.set("unit", unit_v.clone());
        paper.set("x0", x0);
        paper.set("y0", y0);
        paper.set("w", w);
        paper.set("h", h);
        let std = small::match_paper(w, h, Some(&unit_v));
        let own_key = jsv::get(own, "key").filter(|k| k.truthy());
        if let (Some(ok), Some(s)) = (own_key, std)
            && jsv::s(Some(ok)) == Some(s)
        {
            paper.set("key", ok.clone());
        } else if let Some(s) = std {
            paper.set("key", s);
        }
        let mut lo = Obj::new();
        lo.set("name", jsv::orv(g("name"), V::from("Layout")));
        lo.set("paper", paper);
        lo.set("entities", V::Arr(list));
        layouts.push(V::Obj(lo));
    }
    if !layouts.is_empty() {
        imp.inc("layouts", jsv::us2f(layouts.len()));
    }

    let mut meta = Obj::new();
    meta.set("version", version_tag);
    meta.set("release", release_of(version_tag));
    meta.set("units", units_name(insunits));
    let ib = jsv::get(hv, "INSBASE");
    if jsv::truthy(ib)
        && let Some(x) = jsv::fin(jsv::get(ib, "x"))
    {
        meta.set("insbase", V::Obj(cc_io_dwg::obj! { "x" => x, "y" => jsv::orv(jsv::get(ib, "y"), V::Num(0.0)) }));
    }
    let lts = jsv::get(hv, "LTSCALE");
    if jsv::to_num(lts) > 0.0 && !props::strict_eq(lts, Some(&V::Num(1.0))) {
        meta.set_opt("ltscale", lts.cloned());
    }
    length_format(hv, &mut meta);

    let mut drawing = Obj::new();
    drawing.set("entities", V::Arr(entities));
    drawing.set("layers", V::Arr(layers));
    drawing.set("blocks", V::Arr(std::mem::take(&mut imp.blocks)));
    let views = named_views(tget("views"), &layouts);
    drawing.set("layouts", V::Arr(layouts));
    drawing.set("dimStyle", dim_style);
    if let Some(p) = parametric.filter(|p| !p.is_empty()) {
        drawing.set("parametric", p);
    }
    drawing.set("linetypes", V::Arr(linetypes));
    drawing.set("meta", meta);
    drawing.set("views", views);
    drawing.set("vports", split_screen(tget("vports")));
    ucs_from(hv, tget("ucs"), tget("vports"), &mut drawing);
    drawing.set("textStyles", V::Arr(text_styles));
    drawing.set("dimStyles", V::Arr(named_dim_styles));
    if imp.report.get("dynamicBlocks").is_some_and(V::truthy) {
        drawing.set("dynSrc", crate::dyn_blocks::dyn_source(db));
    }
    drawing.set("report", std::mem::take(&mut imp.report));
    let _ = imp.db;
    crate::model3d::read3d_from_dwg(db, &mut drawing, cat);
    Ok(drawing)
}

/// A dynamic block's `dyn.all`: its shown parts are the very objects of `entities` in the JS (attributes resolved on
/// one are resolved on the other); copy them back after resolveAttribs.
fn sync_dyn_all(b: &mut V) {
    let V::Obj(bo) = b else { return };
    if !bo.get("dyn").is_some_and(V::truthy) {
        return;
    }
    let ents: Vec<V> = jsv::arr(bo.get("entities")).to_vec();
    let Some(V::Obj(d)) = bo.get_mut("dyn") else { return };
    let shown: Vec<bool> = jsv::arr(d.get("shown")).iter().map(V::truthy).collect();
    let Some(V::Arr(all)) = d.get_mut("all") else { return };
    let mut k = 0usize;
    for (i, parts) in all.iter_mut().enumerate() {
        if !shown.get(i).copied().unwrap_or(false) {
            continue;
        }
        if let V::Arr(p) = parts {
            for slot in p.iter_mut() {
                if let Some(e) = ents.get(k) {
                    *slot = e.clone();
                }
                k = k.saturating_add(1);
            }
        }
    }
}

/// `extrasFrom(db)`: only the multileaders, pictures and wipeouts of a drawing model, as dxf.js merges them.
pub struct Extras {
    pub model: Vec<V>,
    /// (block name, its extra entities), in block order (a later block of the same name replaces the list)
    pub blocks: Vec<(V, Vec<V>)>,
    pub layouts: Vec<(V, Vec<V>)>,
    pub anon: Vec<V>,
    pub notes: Vec<V>,
}

fn is_extra(e: &V) -> bool {
    jsv::get(Some(e), EXTRA_MARK).is_some()
}

/// `extrasFrom(db)`
pub fn extras_from(db: &Obj, cat: &Catalog) -> R<Extras> {
    let r = import_dwg_v(db, "", cat)?;
    let pick = |list: Ov| -> Vec<V> { jsv::arr(list).iter().filter(|e| is_extra(e)).cloned().collect() };
    let notes: Vec<V> = jsv::arr(jsv::get(r.get("report"), "notes"))
        .iter()
        .filter(|n| {
            let s = jsv::to_str(Some(n)).to_lowercase();
            s.contains("multileader") || s.contains("wipeout") || s.contains("picture")
        })
        .cloned()
        .collect();
    let model = pick(r.get("entities"));
    let mut blocks: Vec<(V, Vec<V>)> = Vec::new();
    for b in jsv::arr(r.get("blocks")) {
        let x = pick(jsv::get(Some(b), "entities"));
        if !x.is_empty() {
            let name = jsv::get(Some(b), "name").cloned().unwrap_or(V::Null);
            if let Some(slot) = blocks.iter_mut().find(|(n, _)| props::strict_eq(Some(n), Some(&name))) {
                slot.1 = x;
            } else {
                blocks.push((name, x));
            }
        }
    }
    let mut layouts: Vec<(V, Vec<V>)> = Vec::new();
    for l in jsv::arr(r.get("layouts")) {
        let x = pick(jsv::get(Some(l), "entities"));
        if !x.is_empty() {
            let name = jsv::get(Some(l), "name").cloned().unwrap_or(V::Null);
            if let Some(slot) = layouts.iter_mut().find(|(n, _)| props::strict_eq(Some(n), Some(&name))) {
                slot.1 = x;
            } else {
                layouts.push((name, x));
            }
        }
    }
    let mut anon_names: Vec<V> = Vec::new();
    for e in model.iter().chain(blocks.iter().flat_map(|(_, l)| l.iter())).chain(layouts.iter().flat_map(|(_, l)| l.iter())) {
        if jsv::s(jsv::get(Some(e), "type")) == Some("insert") {
            let b = jsv::get(Some(e), "block").cloned().unwrap_or(V::Null);
            if !anon_names.iter().any(|x| props::strict_eq(Some(x), Some(&b))) {
                anon_names.push(b);
            }
        }
    }
    let anon: Vec<V> = jsv::arr(r.get("blocks"))
        .iter()
        .filter(|b| {
            let n = jsv::get(Some(b), "name");
            anon_names.iter().any(|x| props::strict_eq(Some(x), n))
        })
        .cloned()
        .collect();
    Ok(Extras { model, blocks, layouts, anon, notes })
}
