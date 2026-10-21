//! dwg-save.js: a CadCraft drawing → the cc-dwg model → DWG through cc-io-dwg's writer (`write_dwg_opts`), in AutoCAD
//! 2000 … 2018. `model_only` gives the model instead (dxf2000.rs writes it as an R2000 DXF).
//!
//! Constraints and parameters go out as gcons.js `forFile` writes them (CCREF xdata, block CADCRAFT_PARAMETRIC). Not
//! written here (as in dxf_write.rs): a 3D model CadCraft saved itself (only loose 3D faces, m3d_file.rs).

use cc_cad::entity::{Entity, Geo};
use cc_cad::js;
use cc_cad::{Block, Doc};
use cc_cad_geom::geom::{insert_matrix, m_apply, m_rotate, text_layout};
use cc_cad_geom::jsmath::JsMath;
use cc_cad_geom::pick::{empty_bounds, entity_bounds, valid_bounds};
use cc_geom::P2;
use cc_io_dwg::value::{Obj, V};

use crate::dyn_blocks::{DynSave, dyn_save};
use crate::i18n::{self, Catalog};
use crate::jsv::{self, JsErr, Ov, R};
use crate::props;
use crate::text::{one_line, slice16};

/// The versions Save as offers, newest first: (key, code).
pub const DWG_VERSIONS: [(&str, &str); 6] =
    [("2018", "AC1032"), ("2013", "AC1027"), ("2010", "AC1024"), ("2007", "AC1021"), ("2004", "AC1018"), ("2000", "AC1015")];

/// `dwgVersionKey(v)`: '2018', 'AC1032' or 'R2018' → '2018', else None.
pub fn dwg_version_key(v: &str) -> Option<&'static str> {
    let up = v.to_uppercase();
    let s = up.strip_prefix('R').unwrap_or(&up);
    DWG_VERSIONS.iter().find(|(k, c)| *k == s || *c == s).map(|(k, _)| *k)
}

const IMPERIAL: [&str; 6] = ["inches", "feet", "miles", "microinches", "mils", "yards"];

/// The standard hatch patterns (hatch.js PATTERNS, metric): (name, [(angle°, x, y, dx, dy, dashes)]).
#[rustfmt::skip]
#[allow(clippy::type_complexity)]
const PATTERNS: [(&str, &[(f64, f64, f64, f64, f64, &[f64])]); 16] = [
    ("ANSI31", &[(45.0, 0.0, 0.0, 0.0, 3.175, &[])]),
    ("ANSI32", &[(45.0, 0.0, 0.0, 0.0, 9.525, &[]), (45.0, 4.490128, 0.0, 0.0, 9.525, &[])]),
    ("ANSI33", &[(45.0, 0.0, 0.0, 0.0, 6.35, &[]), (45.0, 4.490128, 0.0, 0.0, 6.35, &[3.175, -1.5875])]),
    ("ANSI34", &[(45.0, 0.0, 0.0, 0.0, 19.05, &[]), (45.0, 4.490128, 0.0, 0.0, 19.05, &[]), (45.0, 8.980256, 0.0, 0.0, 19.05, &[]), (45.0, 13.470384, 0.0, 0.0, 19.05, &[])]),
    ("ANSI35", &[(45.0, 0.0, 0.0, 0.0, 6.35, &[]), (45.0, 4.490128, 0.0, 0.0, 6.35, &[7.9375, -1.5875, 0.0, -1.5875])]),
    ("ANSI36", &[(45.0, 0.0, 0.0, 5.55625, 3.175, &[7.9375, -1.5875, 0.0, -1.5875])]),
    ("ANSI37", &[(45.0, 0.0, 0.0, 0.0, 3.175, &[]), (135.0, 0.0, 0.0, 0.0, 3.175, &[])]),
    ("ANSI38", &[(45.0, 0.0, 0.0, 0.0, 3.175, &[]), (135.0, 0.0, 0.0, 6.35, 3.175, &[7.9375, -4.7625])]),
    ("LINE", &[(0.0, 0.0, 0.0, 0.0, 3.175, &[])]),
    ("NET", &[(0.0, 0.0, 0.0, 0.0, 3.175, &[]), (90.0, 0.0, 0.0, 0.0, 3.175, &[])]),
    ("NET3", &[(0.0, 0.0, 0.0, 0.0, 3.175, &[]), (60.0, 0.0, 0.0, 0.0, 3.175, &[]), (120.0, 0.0, 0.0, 0.0, 3.175, &[])]),
    ("DASH", &[(0.0, 0.0, 0.0, 3.175, 3.175, &[3.175, -3.175])]),
    ("SQUARE", &[(0.0, 0.0, 0.0, 0.0, 3.175, &[3.175, -3.175]), (90.0, 0.0, 0.0, 0.0, 3.175, &[3.175, -3.175])]),
    ("BRICK", &[(0.0, 0.0, 0.0, 0.0, 6.35, &[]), (90.0, 0.0, 0.0, 6.35, 6.35, &[6.35, -6.35]), (90.0, 3.175, 0.0, 6.35, 6.35, &[-6.35, 6.35])]),
    ("DOTS", &[(0.0, 0.0, 0.0, 0.79375, 1.5875, &[0.0, -1.5875])]),
    ("EARTH", &[
        (0.0, 0.0, 0.0, 6.35, 6.35, &[6.35, -6.35]), (0.0, 0.0, 2.38125, 6.35, 6.35, &[6.35, -6.35]), (0.0, 0.0, 4.7625, 6.35, 6.35, &[6.35, -6.35]),
        (90.0, 0.79375, 5.55625, 6.35, 6.35, &[6.35, -6.35]), (90.0, 3.175, 5.55625, 6.35, 6.35, &[6.35, -6.35]), (90.0, 5.55625, 5.55625, 6.35, 6.35, &[6.35, -6.35])]),
];
#[allow(clippy::type_complexity)]
fn pattern(name: &str) -> Option<&'static [(f64, f64, f64, f64, f64, &'static [f64])]> {
    PATTERNS.iter().find(|(n, _)| *n == name).map(|(_, l)| *l)
}

fn p3(x: Ov, y: Ov, z: f64) -> V {
    let mut o = Obj::new();
    o.set_opt("x", x.cloned());
    o.set_opt("y", y.cloned());
    o.set("z", z);
    V::Obj(o)
}
fn pn(x: f64, y: f64, z: f64) -> V {
    cc_io_dwg::value::p3(x, y, z)
}
fn n(v: f64) -> V {
    V::Num(v)
}
fn s(v: &str) -> V {
    V::from(v)
}
fn ev(e: &Entity) -> V {
    entity_v(e)
}

/// An entity as the dynamic value, with -0 kept in its typed fields (`Entity::to_value` writes them as 0; the DWG
/// writer keeps the sign bit, as the JS does).
pub fn entity_v(e: &Entity) -> V {
    let mut sink = VSink(Obj::with_capacity(12));
    e.write_to(&mut sink);
    V::Obj(sink.0)
}

/// `Entity::to_obj` built straight as a value of the saver's object model (no JSON in between; numbers keep their
/// sign, as the JS objects do).
struct VSink(Obj);

impl VSink {
    fn push(&mut self, k: &'static str, v: V) {
        self.0.0.push((std::borrow::Cow::Borrowed(k), v));
    }
}

fn xy(p: P2) -> V {
    let mut o = Obj::with_capacity(2);
    o.0.push((std::borrow::Cow::Borrowed("x"), V::Num(p.x)));
    o.0.push((std::borrow::Cow::Borrowed("y"), V::Num(p.y)));
    V::Obj(o)
}

impl cc_cad::entity::ObjSink for VSink {
    fn num(&mut self, k: &'static str, v: f64) {
        self.push(k, V::Num(v));
    }
    fn id(&mut self, k: &'static str, v: u64) {
        self.push(k, jsv::from_json(&serde_json::Value::from(v)));
    }
    fn string(&mut self, k: &'static str, v: &str) {
        self.push(k, V::Str(v.to_string()));
    }
    fn boolean(&mut self, k: &'static str, v: bool) {
        self.push(k, V::Bool(v));
    }
    fn null(&mut self, k: &'static str) {
        self.push(k, V::Null);
    }
    fn color(&mut self, k: &'static str, c: &cc_cad::Color) {
        self.push(
            k,
            match c {
                cc_cad::Color::Null => V::Null,
                cc_cad::Color::Num(n) => V::Num(*n),
                cc_cad::Color::Str(st) => V::Str(st.clone()),
            },
        );
    }
    fn object(&mut self, k: &'static str, v: &serde_json::Map<String, serde_json::Value>) {
        self.push(k, jsv::from_json_map(v));
    }
    fn point(&mut self, k: &'static str, p: P2) {
        self.push(k, xy(p));
    }
    fn points(&mut self, k: &'static str, pts: &[P2]) {
        self.push(k, V::Arr(pts.iter().map(|p| xy(*p)).collect()));
    }
    fn vertices(&mut self, k: &'static str, pts: &[cc_cad::entity::Vertex]) {
        let list = pts
            .iter()
            .map(|q| {
                let mut o = Obj::with_capacity(q.extra.len().saturating_add(3));
                o.0.push((std::borrow::Cow::Borrowed("x"), V::Num(q.x)));
                o.0.push((std::borrow::Cow::Borrowed("y"), V::Num(q.y)));
                if let Some(b) = q.b {
                    o.0.push((std::borrow::Cow::Borrowed("b"), V::Num(b)));
                }
                merge_json(&mut o, &q.extra);
                V::Obj(o)
            })
            .collect();
        self.push(k, V::Arr(list));
    }
    fn loops(&mut self, k: &'static str, loops: &[Vec<f64>]) {
        self.push(k, V::Arr(loops.iter().map(|l| V::Arr(l.iter().map(|x| V::Num(*x)).collect())).collect()));
    }
    fn merge(&mut self, extra: &serde_json::Map<String, serde_json::Value>) {
        merge_json(&mut self.0, extra);
    }
}

/// The keys of a JSON object that `o` does not have yet.
fn merge_json(o: &mut Obj, extra: &serde_json::Map<String, serde_json::Value>) {
    for (k, x) in extra {
        if !o.has(k) {
            o.0.push((std::borrow::Cow::Owned(k.clone()), jsv::from_json(x)));
        }
    }
}
fn to_entity(v: &V) -> Entity {
    Entity::from_value(&jsv::to_json(v))
}
fn is_standard(v: Ov) -> bool {
    jsv::to_str(v).eq_ignore_ascii_case("standard")
}
/// `/^(bylayer|byblock|continuous)$/i`
fn by_or_cont(s: &str) -> bool {
    ["bylayer", "byblock", "continuous"].iter().any(|x| s.eq_ignore_ascii_case(x))
}
/// trWord(tr): 'ByBlock' → 0x01000000; 0–90 % → 0x02000000 | alpha; else None.
fn tr_word(tr: Ov) -> Option<f64> {
    if jsv::s(tr) == Some("ByBlock") {
        return Some(16_777_216.0);
    }
    let t = jsv::fin(tr)?;
    let alpha = js::round(255.0 * (1.0 - js::max(0.0, js::min(90.0, t)) / 100.0));
    Some(33_554_432.0 + alpha)
}

/// The result of [`save_dwg`].
pub struct Saved {
    pub bytes: Vec<u8>,
    pub report: serde_json::Value,
}

/// Options of [`save_dwg`].
#[derive(Clone, Debug)]
pub struct SaveOpts {
    pub version: String,
    /// creation time, ms since 1970 (None: now)
    pub created_ms: Option<f64>,
    /// GUIDs `(16k + i) & 255` (the parity harness's stub) instead of random ones
    pub guid_sequence: bool,
    pub model_only: bool,
    pub explode_leaders: bool,
}
impl Default for SaveOpts {
    fn default() -> Self {
        SaveOpts { version: "2000".into(), created_ms: None, guid_sequence: false, model_only: false, explode_leaders: false }
    }
}

struct Sv<'d> {
    doc: &'d Doc,
    vkey: &'static str,
    model_only: bool,
    explode_leaders: bool,
    counts: Obj,
    used_lt: Vec<String>,
    lt_handles: Vec<(String, String)>,
    blocks: Vec<V>,
    ds: Option<DynSave>,
    vp_overrides: Vec<(String, Obj)>,
    clip_objects: Vec<V>,
    pic_defs: Vec<(String, String)>,
    pic_objects: Vec<V>,
    dim_n: u64,
    text_styles: Vec<(String, Obj)>,
    dim_styles_out: Vec<(String, Obj)>,
}

const H_ALIGN: [(&str, f64); 3] = [("left", 0.0), ("center", 1.0), ("right", 2.0)];
const V_ALIGN: [(&str, f64); 4] = [("baseline", 0.0), ("bottom", 1.0), ("middle", 2.0), ("top", 3.0)];
fn lookup(t: &[(&str, f64)], k: Ov) -> Option<f64> {
    let k = jsv::s(k)?;
    t.iter().find(|(n, _)| *n == k).map(|(_, v)| *v)
}

impl Sv<'_> {
    fn note(&mut self, k: &str, by: f64) {
        let v = jsv::or(self.counts.get(k), 0.0) + by;
        self.counts.set(k.to_string(), v);
    }
    fn color(&mut self, c: Ov) -> V {
        match c {
            None | Some(V::Null) => V::Obj(cc_io_dwg::obj! { "index" => 256.0 }),
            Some(V::Str(st)) => {
                self.note("truecolor", 1.0);
                let rgb = i32f(jsv::parse_int_radix16(st.get(1..).unwrap_or(""))) & 0xff_ffff;
                V::Obj(cc_io_dwg::obj! { "rgb" => jsv::i2f(rgb), "index" => f64::from(cc_cad::nearest_aci(st)) })
            }
            Some(v) => V::Obj(cc_io_dwg::obj! { "index" => v.clone() }),
        }
    }
    fn lt_of(&mut self, n: Ov) -> V {
        if let Some(st) = jsv::s(n).filter(|s| !s.is_empty())
            && !by_or_cont(st)
            && !self.used_lt.iter().any(|x| x == st)
        {
            self.used_lt.push(st.to_string());
        }
        jsv::orv(n, s("ByLayer"))
    }
    fn text_style(&mut self, name: Ov) -> V {
        let k = jsv::upper(&jsv::to_str(name));
        if !self.text_styles.iter().any(|(x, _)| *x == k) {
            self.text_styles.push((
                k.clone(),
                cc_io_dwg::obj! { "name" => jsv::to_str(name), "font" => "txt", "height" => 0.0, "widthFactor" => 1.0, "oblique" => 0.0 },
            ));
        }
        self.text_styles.iter().find(|(x, _)| *x == k).and_then(|(_, o)| o.get("name").cloned()).unwrap_or(V::Null)
    }
    fn ds_vars(&mut self, st: Option<&Obj>) -> Obj {
        let mut v = props::vars_of(st);
        if let Some(st) = st
            && jsv::truthy(st.get("txsty"))
            && !is_standard(st.get("txsty"))
        {
            let t = self.text_style(st.get("txsty"));
            v.set("DIMTXSTY", t);
        }
        v
    }
    fn dim_style_name(&mut self, nm: Ov) -> V {
        if !jsv::truthy(nm) || is_standard(nm) {
            return s("Standard");
        }
        let k = jsv::upper(&jsv::to_str(nm));
        if !self.dim_styles_out.iter().any(|(x, _)| *x == k) {
            let cd = obj_of_json(&self.doc.current.dim);
            let vars = self.ds_vars(Some(&cd));
            self.dim_styles_out.push((k.clone(), cc_io_dwg::obj! { "name" => jsv::to_str(nm), "vars" => vars }));
        }
        self.dim_styles_out.iter().find(|(x, _)| *x == k).and_then(|(_, o)| o.get("name").cloned()).unwrap_or(V::Null)
    }

    /// `textFields(e, value)`
    fn text_fields(&mut self, e: &V, value: V) -> Obj {
        let g = |k: &str| jsv::get(Some(e), k);
        let ha = lookup(&H_ALIGN, g("ha")).unwrap_or(0.0);
        let va = lookup(&V_ALIGN, g("va")).unwrap_or(0.0);
        let z = jsv::fin(g("z")).unwrap_or(0.0);
        let mut o = Obj::new();
        o.set("insert", p3(g("x"), g("y"), z));
        o.set("align", if ha != 0.0 || va != 0.0 { p3(g("x"), g("y"), z) } else { V::Null });
        o.set_opt("height", g("h").cloned());
        o.set("value", value);
        o.set("rotation", jsv::orv(g("rot"), n(0.0)));
        o.set("widthFactor", jsv::orv(g("wf"), n(1.0)));
        o.set("hAlign", ha);
        o.set("vAlign", va);
        if jsv::truthy(g("style")) && !is_standard(g("style")) {
            let t = self.text_style(g("style"));
            o.set("style", t);
        }
        if jsv::truthy(g("obl")) {
            o.set_opt("oblique", g("obl").cloned());
        }
        if z != 0.0 {
            o.set("elevation", z);
        }
        o
    }

    fn base(&mut self, e: &V) -> Obj {
        let g = |k: &str| jsv::get(Some(e), k);
        let tr = if self.vkey == "2000" { None } else { tr_word(g("tr")) };
        if self.vkey == "2000" && !jsv::nullish(g("tr")) {
            self.note("tr2000", 1.0);
        }
        // room for what the entity adds after (type, its geometry), without regrowing
        let mut b = Obj::with_capacity(12);
        b.set("layer", jsv::orv(g("layer"), s("0")));
        let c = self.color(g("color"));
        b.set("color", c);
        let lt = self.lt_of(g("lt"));
        b.set("linetype", lt);
        b.set("lineweight", lw100(g("lw"), -1.0));
        let lts = g("lts");
        if jsv::to_num(lts) > 0.0 && !props::strict_eq(lts, Some(&n(1.0))) {
            b.set_opt("ltscale", lts.cloned());
        }
        if let Some(t) = tr {
            b.set("transparency", t);
        }
        if jsv::truthy(g("th")) {
            b.set_opt("thickness", g("th").cloned());
        }
        b
    }

    /// `conv(e)`: one CadCraft entity → cc-dwg model entities.
    fn conv(&mut self, e: &V) -> R<Vec<V>> {
        let mut out = self.conv0(e)?;
        let empty = Obj::new();
        let x = props::props_xdata(jsv::obj(Some(e)).unwrap_or(&empty), self.vkey == "2000");
        if let Some(V::Obj(first)) = out.first_mut()
            && !x.is_empty()
        {
            let have = first.get("xdata").and_then(|v| jsv::obj(Some(v))).cloned();
            let both = match (have.as_ref().and_then(|h| h.get("CADCRAFT")), x.get("CADCRAFT")) {
                (Some(V::Arr(hc)), Some(V::Arr(ac))) => Some(hc.iter().chain(ac.iter()).cloned().collect::<Vec<V>>()),
                _ => None,
            };
            let mut add = x;
            if let Some(both) = both {
                add.set("CADCRAFT", V::Arr(both));
            }
            let mut merged = have.unwrap_or_default();
            merged.assign(add);
            first.set("xdata", merged);
        }
        if jsv::s(jsv::get(Some(e), "mat")).is_some_and(|m| !m.is_empty()) {
            self.note("material", 1.0);
        }
        Ok(out)
    }

    #[allow(clippy::too_many_lines)]
    fn conv0(&mut self, e: &V) -> R<Vec<V>> {
        let g = |k: &str| jsv::get(Some(e), k);
        let mut base = self.base(e);
        let z = jsv::fin(g("z")).unwrap_or(0.0);
        let typ = jsv::to_str(g("type"));
        // each arm runs alone: the base moves into its object
        let mk = |base: &mut Obj, t: &str| {
            let mut o = std::mem::take(base);
            o.set("type", t);
            o
        };
        Ok(match typ.as_str() {
            "line" => {
                let mut o = mk(&mut base, "LINE");
                o.set("start", p3(g("x1"), g("y1"), jsv::fin(g("z1")).unwrap_or(0.0)));
                o.set("end", p3(g("x2"), g("y2"), jsv::fin(g("z2")).unwrap_or(0.0)));
                vec![V::Obj(o)]
            }
            "circle" => {
                let mut o = mk(&mut base, "CIRCLE");
                o.set("center", p3(g("cx"), g("cy"), z));
                o.set_opt("radius", g("r").cloned());
                vec![V::Obj(o)]
            }
            "arc" => {
                let mut o = mk(&mut base, "ARC");
                o.set("center", p3(g("cx"), g("cy"), z));
                o.set_opt("radius", g("r").cloned());
                o.set_opt("startAngle", g("a0").cloned());
                o.set_opt("endAngle", g("a1").cloned());
                vec![V::Obj(o)]
            }
            "point" => {
                let mut o = mk(&mut base, "POINT");
                o.set("position", p3(g("x"), g("y"), z));
                vec![V::Obj(o)]
            }
            t @ ("xline" | "ray") => {
                let (dx, dy) = (jsv::to_num(g("dx")), jsv::to_num(g("dy")));
                let nn = js::hypot(dx, dy);
                if !(nn > 0.0) || jsv::fin(g("x")).is_none() || jsv::fin(g("y")).is_none() {
                    return Ok(Vec::new());
                }
                let mut o = mk(&mut base, if t == "xline" { "XLINE" } else { "RAY" });
                o.set("start", p3(g("x"), g("y"), z));
                o.set("direction", pn(dx / nn, dy / nn, 0.0));
                vec![V::Obj(o)]
            }
            "poly" => {
                let pts = jsv::arr(g("pts"));
                let each = pts.iter().any(|p| jsv::fin(jsv::get(Some(p), "sw")).is_some() || jsv::fin(jsv::get(Some(p), "ew")).is_some());
                let gw = jsv::fin(g("w")).filter(|w| *w > 0.0).unwrap_or(0.0);
                let vertices: Vec<V> = pts
                    .iter()
                    .map(|p| {
                        let gp = |k: &str| jsv::get(Some(p), k);
                        let mut o = Obj::new();
                        o.set_opt("x", gp("x").cloned());
                        o.set_opt("y", gp("y").cloned());
                        o.set("bulge", jsv::orv(gp("b"), n(0.0)));
                        if each {
                            o.set("startWidth", jsv::fin(gp("sw")).unwrap_or(gw));
                            o.set("endWidth", jsv::fin(gp("ew")).unwrap_or(gw));
                        }
                        V::Obj(o)
                    })
                    .collect();
                let mut o = mk(&mut base, "LWPOLYLINE");
                o.set("vertices", V::Arr(vertices));
                o.set("closed", jsv::truthy(g("closed")));
                if gw != 0.0 && !each {
                    o.set("constantWidth", gw);
                }
                if z != 0.0 {
                    o.set("elevation", z);
                }
                vec![V::Obj(o)]
            }
            "ellipse" => {
                let mut o = mk(&mut base, "ELLIPSE");
                o.set("center", p3(g("cx"), g("cy"), z));
                o.set("majorAxis", p3(g("mx"), g("my"), 0.0));
                o.set_opt("ratio", g("ratio").cloned());
                o.set_opt("startParam", g("t0").cloned());
                o.set_opt("endParam", g("t1").cloned());
                vec![V::Obj(o)]
            }
            "text" => vec![self.text(e, &base)],
            "hatch" => {
                if jsv::truthy(g("wipe")) {
                    Vec::new()
                } else {
                    self.hatch(e, &base)
                }
            }
            "insert" => self.insert(e, &mut base, z)?,
            "dim" => self.dimension(e, &base)?,
            "viewport" => vec![self.viewport(e, base, 0.0)],
            "mleader" => {
                if self.explode_leaders {
                    self.note("mleader", 1.0);
                    let ent = to_entity(e);
                    let parts = cc_cad_geom::corners::explode_entity(&ent, Some(&self.doc.blocks)).unwrap_or_default();
                    let mut out = Vec::new();
                    for p in &parts {
                        out.extend(self.conv(&ev(p))?);
                    }
                    out
                } else {
                    self.multileader(e, &base)
                }
            }
            _ => {
                self.note("other", 1.0);
                Vec::new()
            }
        })
    }

    fn insert(&mut self, e: &V, base: &mut Obj, z: f64) -> R<Vec<V>> {
        let g = |k: &str| jsv::get(Some(e), k);
        let block = jsv::to_str(g("block"));
        let blk = self.doc.blocks.get(&block);
        let ent = to_entity(e);
        let ins = match &ent.geo {
            Geo::Insert(i) => Some(i.clone()),
            _ => None,
        };
        if !self.model_only
            && let (Some(b), Some(i)) = (blk, ins.as_ref())
            && let Some(pic) = picture_placement(i, b)
        {
            self.note("picture", 1.0);
            let def = self.pic_def(&pic.file, &pic.size);
            let mut o = base.clone();
            o.set("type", "IMAGE");
            o.set("insert", pn(pic.insert.x, pic.insert.y, z));
            o.set("u", pn(pic.u.x, pic.u.y, 0.0));
            o.set("v", pn(pic.v.x, pic.v.y, 0.0));
            o.set("size", pic.size.clone());
            o.set("def", def);
            o.set("displayFlags", 7.0);
            o.set("brightness", 50.0);
            o.set("contrast", 50.0);
            o.set("fade", js::int32(pic.fade));
            match pic.clip {
                Some(c) => {
                    o.set("clipping", true);
                    o.set("clipType", 2.0);
                    o.set("clip", V::Arr(c.into_iter().map(|(x, y)| jsv::p2(x, y)).collect()));
                }
                None => {
                    o.set("clipping", false);
                    o.set("clip", V::Arr(Vec::new()));
                }
            }
            return Ok(vec![V::Obj(o)]);
        }
        let has_attrs = jsv::obj(g("attrs")).is_some_and(|a| !a.is_empty());
        let atts: Vec<V> = match (blk, ins.as_ref()) {
            (Some(b), Some(i)) if has_attrs => world_attribs(i, b),
            _ => Vec::new(),
        };
        let dr = if jsv::truthy(g("dyn")) {
            self.ds.as_mut().and_then(|ds| ds.reference(&block, g("dyn").unwrap_or(&V::Null), None))
        } else {
            None
        };
        let cp = clip_poly(e);
        let mut clip_ref: Option<Obj> = None;
        if let Some(cp) = cp {
            if self.model_only {
                clip_ref = Some(cc_io_dwg::obj! { "xclip" => V::Obj(cc_io_dwg::obj! { "pts" => V::Arr(cp) }) });
            } else if dr.as_ref().is_some_and(|d| d.1.as_ref().is_some_and(V::truthy)) {
                self.note("clipDyn", 1.0);
            } else {
                let fh = |len: usize| format!("CC{:X}X", 0x7F00_0000u64.saturating_add(u64::try_from(len).unwrap_or(0)));
                let l0 = self.clip_objects.len();
                let ih = dr.as_ref().map_or_else(|| format!("{}I", fh(l0)), |d| d.0.clone());
                let xd = fh(l0);
                let d2 = fh(l0.saturating_add(1));
                let sf = fh(l0.saturating_add(2));
                self.clip_objects.push(dict_obj(&xd, &ih, cc_io_dwg::obj! { "ACAD_FILTER" => d2.clone() }));
                self.clip_objects.push(dict_obj(&d2, &xd, cc_io_dwg::obj! { "SPATIAL" => sf.clone() }));
                let mut f = filter_of(&cp);
                f.set("handle", sf);
                f.set("owner", d2.clone());
                f.set("reactors", V::Arr(vec![V::Str(d2)]));
                self.clip_objects.push(V::Obj(f));
                clip_ref = Some(cc_io_dwg::obj! { "handle" => ih, "extDict" => xd });
            }
        }
        let mut atts_out = Vec::with_capacity(atts.len());
        for t in &atts {
            let gt = |k: &str| jsv::get(Some(t), k);
            let mut o = self.text_fields(t, V::Str(one_line(&jsv::str_nn(gt("s")))));
            let layer = if jsv::truthy(gt("layer")) {
                gt("layer").cloned()
            } else if jsv::truthy(g("layer")) {
                g("layer").cloned()
            } else {
                Some(s("0"))
            };
            o.set_opt("layer", layer);
            let c = self.color(gt("color"));
            o.set("color", c);
            o.set_opt("tag", gt("tag").cloned());
            o.set("flags", if jsv::truthy(gt("invisible")) { 1.0 } else { 0.0 });
            atts_out.push(V::Obj(o));
        }
        let mut o = base.clone();
        o.set("type", "INSERT");
        o.set_opt("block", g("block").cloned());
        o.set("insert", p3(g("x"), g("y"), z));
        let sz = jsv::fin(g("sz")).filter(|v| *v != 0.0).unwrap_or(1.0);
        o.set("scale", V::Obj(cc_io_dwg::obj! { "x" => jsv::nnv(g("sx"), n(1.0)), "y" => jsv::nnv(g("sy"), n(1.0)), "z" => sz }));
        o.set("rotation", jsv::orv(g("rot"), n(0.0)));
        o.set("attribs", V::Arr(atts_out));
        if let Some((h, xd)) = dr {
            o.set("handle", h);
            if let Some(x) = xd.filter(V::truthy) {
                o.set("extDict", x);
            }
        }
        if let Some(c) = clip_ref {
            o.assign(c);
        }
        Ok(vec![V::Obj(o)])
    }

    fn pic_def(&mut self, file: &V, size: &V) -> V {
        let full = jsv::str_or_empty(Some(file));
        let base = full.rsplit(['\\', '/']).next().unwrap_or("").to_lowercase();
        let k = if base.is_empty() { "image".to_string() } else { base };
        if !self.pic_defs.iter().any(|(x, _)| *x == k) {
            let h = format!("CC{:X}", 0x7A00_0000u64.saturating_add(u64::try_from(self.pic_defs.len()).unwrap_or(0)));
            self.pic_defs.push((k.clone(), h.clone()));
            let fname = if file.truthy() { jsv::to_str(Some(file)) } else { "image".into() };
            self.pic_objects.push(V::Obj(cc_io_dwg::obj! {
                "handle" => h, "type" => "IMAGEDEF", "fileName" => fname, "size" => size.clone(), "loaded" => true,
                "classVersion" => 0.0, "resUnits" => 0.0, "pixelSize" => jsv::p2(1.0, 1.0)
            }));
        }
        self.pic_defs.iter().find(|(x, _)| *x == k).map_or(V::Null, |(_, h)| V::Str(h.clone()))
    }

    /// `multileader(e, base)`: one leader line + text → MULTILEADER.
    fn multileader(&mut self, e: &V, base: &Obj) -> Vec<V> {
        let g = |k: &str| jsv::get(Some(e), k);
        let pts = jsv::arr(g("pts"));
        if !matches!(g("pts"), Some(V::Arr(_))) || pts.len() < 2 {
            self.note("other", 1.0);
            return Vec::new();
        }
        let ent = to_entity(e);
        let st = jsv::from_json(&serde_json::Value::Object(cc_cad_geom::leader::leader_style(
            g("st").and_then(|x| jsv::to_json(x).as_object().cloned()).as_ref(),
        )));
        let stn = |k: &str| jsv::to_num(jsv::get(Some(&st), k));
        let dir = if jsv::num(g("dir")) == Some(-1.0) { -1.0 } else { 1.0 };
        let l = cc_cad_geom::leader::landing(&ent);
        let h = if jsv::to_num(g("h")) > 0.0 { jsv::to_num(g("h")) } else { stn("asz") };
        let txt = cc_cad_geom::leader::leader_geometry(&ent).into_iter().find(|p| p.typ() == "text");
        let last = pts.last();
        let mz = jsv::fin(g("z")).unwrap_or(0.0);
        let label = jsv::str_nn(g("s"))
            .split('\n')
            .map(|l| l.replace('\\', "\\\\").replace('{', "\\{").replace('}', "\\}"))
            .collect::<Vec<_>>()
            .join("\\P");
        self.note("mleaderOut", 1.0);
        let mut cx = Obj::new();
        cx.set("scale", 1.0);
        cx.set("textHeight", h);
        cx.set_opt("arrowSize", jsv::get(Some(&st), "asz").cloned());
        cx.set_opt("landingGap", jsv::get(Some(&st), "gap").cloned());
        cx.set("leftAttach", 2.0);
        cx.set("rightAttach", 2.0);
        let ltxt = l.map_or((f64::NAN, f64::NAN), |l| (l.2.x, l.2.y));
        cx.set("contentBase", pn(ltxt.0, ltxt.1, 0.0));
        cx.set("hasText", txt.is_some());
        cx.set("hasBlock", false);
        if let Some(t) = &txt {
            let tv = t.to_value();
            let ww = jsv::to_num(g("ww"));
            cx.set(
                "text",
                V::Obj(cc_io_dwg::obj! {
                    "label" => label,
                    "location" => pn(tv.get("x").and_then(serde_json::Value::as_f64).unwrap_or(f64::NAN), tv.get("y").and_then(serde_json::Value::as_f64).unwrap_or(f64::NAN), mz),
                    "direction" => pn(1.0, 0.0, 0.0),
                    "width" => if ww > 0.0 { ww } else { 0.0 },
                    "height" => 0.0,
                    "lineSpacing" => jsv::orv(g("ls"), n(1.0)),
                    "alignment" => if dir == 1.0 { 1.0 } else { 3.0 }
                }),
            );
        }
        let lines: Vec<V> =
            pts.iter().take(pts.len().saturating_sub(1)).map(|q| p3(jsv::get(Some(q), "x"), jsv::get(Some(q), "y"), mz)).collect();
        let root = cc_io_dwg::obj! {
            "connection" => p3(jsv::get(last, "x"), jsv::get(last, "y"), mz),
            "direction" => pn(dir, 0.0, 0.0),
            "landingDistance" => jsv::get(Some(&st), "dogleg").cloned().unwrap_or(V::Null),
            "lines" => V::Arr(vec![V::Obj(cc_io_dwg::obj! { "points" => V::Arr(lines) })])
        };
        cx.set("roots", V::Arr(vec![V::Obj(root)]));
        let mut o = base.clone();
        o.set("type", "MULTILEADER");
        o.set("leaderType", 1.0);
        o.set("contentType", if txt.is_some() { 2.0 } else { 0.0 });
        o.set("landingEnabled", true);
        o.set("doglegEnabled", stn("dogleg") > 0.0);
        o.set_opt("landingDistance", jsv::get(Some(&st), "dogleg").cloned());
        o.set_opt("arrowSize", jsv::get(Some(&st), "asz").cloned());
        o.set("textLeftAttach", 2.0);
        o.set("textRightAttach", 2.0);
        o.set("scale", 1.0);
        o.set("context", cx);
        vec![V::Obj(o)]
    }

    /// `text(e, base)`: one line and no wrap width → TEXT; otherwise MTEXT.
    fn text(&mut self, e: &V, base: &Obj) -> V {
        let g = |k: &str| jsv::get(Some(e), k);
        let st = jsv::str_nn(g("s"));
        if !st.contains('\n') && !(jsv::to_num(g("ww")) > 0.0) && !jsv::truthy(g("bgm")) {
            let mut o = base.clone();
            o.set("type", "TEXT");
            o.assign(self.text_fields(e, V::Str(st)));
            return V::Obj(o);
        }
        if jsv::truthy(g("wf")) && !props::strict_eq(g("wf"), Some(&n(1.0))) {
            self.note("mtextwidth", 1.0);
        }
        let ent = to_entity(e);
        let (bw, bh) = match &ent.geo {
            Geo::Text(t) => {
                let l = text_layout(t);
                (l.bx[2], l.bx[3])
            }
            _ => (f64::NAN, f64::NAN),
        };
        let mut va = jsv::s(g("va")).filter(|v| !v.is_empty()).unwrap_or("baseline").to_string();
        let mut at = (jsv::to_num(g("x")), jsv::to_num(g("y")));
        let mut at_v: Option<(V, V)> = Some((g("x").cloned().unwrap_or(V::Null), g("y").cloned().unwrap_or(V::Null)));
        if !matches!(va.as_str(), "top" | "middle" | "bottom") {
            let d = m_apply(&m_rotate(jsv::or(g("rot"), 0.0)), P2::new(0.0, jsv::to_num(g("h"))));
            at = (at.0 + d.x, at.1 + d.y);
            at_v = None;
            va = "top".into();
        }
        let mut body =
            st.split('\n').map(|l| l.replace('\\', "\\\\").replace('{', "\\{").replace('}', "\\}")).collect::<Vec<_>>().join("\\P");
        if jsv::truthy(g("obl")) {
            let deg = jsv::to_num(g("obl")) * 180.0 / std::f64::consts::PI;
            body = format!("\\Q{};{body}", js::num_to_string(js::str_to_number(&js::to_fixed(deg, 6))));
        }
        let r = jsv::or(g("rot"), 0.0);
        let z = jsv::fin(g("z")).unwrap_or(0.0);
        let row = match va.as_str() {
            "top" => 0.0,
            "middle" => 1.0,
            _ => 2.0,
        };
        let col = lookup(&H_ALIGN, g("ha")).unwrap_or(0.0);
        let mut o = base.clone();
        o.set("type", "MTEXT");
        match at_v {
            Some((x, y)) => o.set("insert", p3(Some(&x), Some(&y), z)),
            None => o.set("insert", pn(at.0, at.1, z)),
        }
        o.set("xAxis", pn(r.jcos(), r.jsin(), 0.0));
        o.set("rotation", r);
        o.set_opt("height", g("h").cloned());
        o.set("width", if jsv::to_num(g("ww")) > 0.0 { jsv::to_num(g("ww")) } else { 0.0 });
        o.set("attach", row * 3.0 + col + 1.0);
        o.set("value", body);
        o.set("lineSpacing", jsv::orv(g("ls"), n(1.0)));
        o.set("lineSpacingStyle", 1.0);
        o.set("extWidth", bw);
        o.set("extHeight", bh);
        if jsv::truthy(g("style")) && !is_standard(g("style")) {
            let t = self.text_style(g("style"));
            o.set("style", t);
        }
        let m = self.mask_fields(g("bgm"));
        o.assign(m);
        V::Obj(o)
    }
    fn mask_fields(&mut self, m: Ov) -> Obj {
        let Some(mo) = jsv::obj(m) else { return Obj::new() };
        if self.vkey == "2000" && !self.model_only {
            self.note("mask2000", 1.0);
            return Obj::new();
        }
        let win = jsv::s(mo.get("color")) == Some("bg");
        let sc = jsv::fin(mo.get("scale")).filter(|x| (1.0..=5.0).contains(x)).unwrap_or(1.5);
        let color = if win { V::Obj(cc_io_dwg::obj! { "index" => 7.0 }) } else { self.color(Some(&jsv::nnv(mo.get("color"), n(7.0)))) };
        cc_io_dwg::obj! { "backgroundFlags" => if win { 3.0 } else { 1.0 }, "backgroundScale" => sc, "backgroundColor" => color, "backgroundTransparency" => 0.0 }
    }

    /// `hatch(e, base)`
    fn hatch(&mut self, e: &V, base: &Obj) -> Vec<V> {
        let g = |k: &str| jsv::get(Some(e), k);
        let rings: Vec<Vec<(V, V)>> = jsv::arr(g("loops"))
            .iter()
            .map(|l| {
                let a = jsv::arr(Some(l));
                let mut pts: Vec<(V, V)> = Vec::new();
                let mut k = 0usize;
                while k.saturating_add(1) < a.len() {
                    pts.push((a.get(k).cloned().unwrap_or(V::Null), a.get(k.saturating_add(1)).cloned().unwrap_or(V::Null)));
                    k = k.saturating_add(2);
                }
                open_ring(pts)
            })
            .filter(|r| r.len() >= 3)
            .collect();
        if rings.is_empty() {
            return Vec::new();
        }
        let z = jsv::fin(g("z")).unwrap_or(0.0);
        let solid = jsv::truthy(g("solid"));
        if solid
            && rings.len() == 1
            && rings.first().is_some_and(|r| r.len() <= 4)
            && !jsv::truthy(g("wipe"))
            && let Some(r) = rings.first()
        {
            let pt = |i: usize| r.get(i).map(|(x, y)| p3(Some(x), Some(y), z));
            let (a, b, c) = (pt(0).unwrap_or(V::Null), pt(1).unwrap_or(V::Null), pt(2).unwrap_or(V::Null));
            let d = pt(3).unwrap_or_else(|| c.clone());
            let mut o = base.clone();
            o.set("type", "SOLID");
            o.set("points", V::Arr(vec![a, b, d, c]));
            if z != 0.0 {
                o.set("elevation", z);
            }
            return vec![V::Obj(o)];
        }
        let loops: Vec<V> = rings
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let vs: Vec<V> = r.iter().map(|(x, y)| V::Obj(cc_io_dwg::obj! { "x" => x.clone(), "y" => y.clone(), "bulge" => 0.0 })).collect();
                V::Obj(cc_io_dwg::obj! { "flags" => if i > 0 { 0.0 } else { 1.0 }, "polyline" => V::Obj(cc_io_dwg::obj! { "closed" => true, "vertices" => V::Arr(vs) }) })
            })
            .collect();
        let island = match jsv::s(g("island")) {
            Some("outer") => 1.0,
            Some("ignore") => 2.0,
            _ => 0.0,
        };
        let mut more = Obj::new();
        if z != 0.0 {
            more.set("elevation", z);
        }
        more.set("style", island);
        let mut o = base.clone();
        o.set("type", "HATCH");
        if solid {
            o.set("pattern", "SOLID");
            o.set("solid", true);
            o.set("loops", V::Arr(loops));
            o.assign(more);
            return vec![V::Obj(o)];
        }
        let lines = self.pattern_lines(e);
        if lines.is_none() {
            self.note("hatchUnknown", 1.0);
        }
        let pname = jsv::upper(&jsv::to_str(Some(&jsv::orv(g("pattern"), s("USER")))));
        let pname: String = slice16(&pname, 0, 31);
        let known = pattern(&jsv::upper(&jsv::str_or_empty(g("pattern")))).is_some();
        o.set("pattern", pname);
        o.set("solid", false);
        o.set("patternType", if known { 1.0 } else { 2.0 });
        o.set("angle", jsv::orv(g("pangle"), n(0.0)));
        o.set("scale", if jsv::to_num(g("pscale")) > 0.0 { g("pscale").cloned().unwrap_or(n(1.0)) } else { n(1.0) });
        o.set("loops", V::Arr(loops));
        o.set("lines", V::Arr(lines.unwrap_or_default()));
        o.assign(more);
        vec![V::Obj(o)]
    }
    fn pattern_lines(&self, e: &V) -> Option<Vec<V>> {
        let g = |k: &str| jsv::get(Some(e), k);
        struct F {
            angle: f64,
            x: f64,
            y: f64,
            dx: f64,
            dy: f64,
            dashes: Vec<V>,
        }
        let pl = jsv::arr(g("patlines"));
        let fams: Vec<F> = if matches!(g("patlines"), Some(V::Arr(_))) && !pl.is_empty() {
            pl.iter()
                .map(|f| {
                    let gf = |k: &str| jsv::to_num(jsv::get(Some(f), k));
                    F {
                        angle: gf("angle"),
                        x: gf("x"),
                        y: gf("y"),
                        dx: gf("dx"),
                        dy: gf("dy"),
                        dashes: jsv::arr(jsv::get(Some(f), "dashes")).to_vec(),
                    }
                })
                .collect()
        } else {
            let def = pattern(&jsv::upper(&jsv::str_or_empty(g("pattern"))))?;
            let ent = to_entity(e);
            let k = (if jsv::to_num(g("pscale")) > 0.0 { jsv::to_num(g("pscale")) } else { 1.0 })
                / (if cc_cad_geom::hatch::hatch_is_metric_doc(Some(&ent), self.doc) { 1.0 } else { 25.4 });
            let turn = jsv::or(g("pangle"), 0.0);
            let (c, sn) = (turn.jcos(), turn.jsin());
            def.iter()
                .map(|f| F {
                    angle: f.0 + turn * 180.0 / std::f64::consts::PI,
                    x: (f.1 * c - f.2 * sn) * k,
                    y: (f.1 * sn + f.2 * c) * k,
                    dx: f.3 * k,
                    dy: f.4 * k,
                    dashes: f.5.iter().map(|d| V::Num(d * k)).collect(),
                })
                .collect()
        };
        let ox = jsv::fin(g("ox")).unwrap_or(0.0);
        let oy = jsv::fin(g("oy")).unwrap_or(0.0);
        Some(
            fams.iter()
                .map(|f| {
                    let a = f.angle * std::f64::consts::PI / 180.0;
                    let (ca, sa) = (a.jcos(), a.jsin());
                    V::Obj(cc_io_dwg::obj! {
                        "angle" => a,
                        "base" => jsv::p2(f.x + ox, f.y + oy),
                        "offset" => jsv::p2(f.dx * ca - f.dy * sa, f.dx * sa + f.dy * ca),
                        "dashes" => V::Arr(f.dashes.clone())
                    })
                })
                .collect(),
        )
    }

    /// `dimension(e, base)`: DIMENSION (its points in DXF meaning) + the unnamed block that draws it.
    fn dimension(&mut self, e: &V, base: &Obj) -> R<Vec<V>> {
        let g = |k: &str| jsv::get(Some(e), k);
        let ent = to_entity(e);
        let own = if jsv::truthy(g("block")) { self.doc.blocks.get(&jsv::to_str(g("block"))) } else { None };
        self.dim_n = self.dim_n.saturating_add(1);
        let name = format!("*D{}", self.dim_n);
        let parts: cc_cad::Ents = match own {
            Some(b) => b.entities.clone(),
            None => cc_cad_geom::dims::dim_geometry(&ent).into(),
        };
        let mut ents = Vec::new();
        for p in &parts {
            ents.extend(self.conv(&ev(p))?);
        }
        self.blocks.push(V::Obj(cc_io_dwg::obj! { "name" => name.clone(), "base" => pn(0.0, 0.0, 0.0), "entities" => V::Arr(ents) }));
        self.note("dim", 1.0);
        let mut st = props::dim_defaults();
        if let Some(V::Obj(o)) = g("st") {
            st.assign(o.clone());
        }
        let kind = jsv::to_str(g("kind"));
        let dim_type = match kind.as_str() {
            "linear" => 0.0,
            "aligned" => 1.0,
            "diameter" => 3.0,
            "radius" => 4.0,
            "angular" => 5.0,
            _ => 0.0,
        };
        let style = self.dim_style_name(g("dimstyle"));
        let tp = cc_cad_geom::dims::dim_text_point(&ent);
        let mut d = base.clone();
        d.set("type", "DIMENSION");
        d.set("dimType", dim_type);
        d.set("block", name.clone());
        d.set("style", style);
        d.set(
            "textMid",
            match tp {
                Some(p) => pn(p.x, p.y, 0.0),
                None => p3(jsv::get(g("loc"), "x"), jsv::get(g("loc"), "y"), 0.0),
            },
        );
        d.set("text", jsv::orv(g("text"), s("")));
        d.set("measurement", cc_cad_geom::dims::dim_measure(&ent));
        d.set("attach", 5.0);
        if jsv::truthy(g("trot")) {
            d.set_opt("textRotation", g("trot").cloned());
        }
        d.set("flags", jsv::i2f(i32f(dim_type) | 32 | 128));
        let pt = |k: &str| -> R<(f64, f64)> {
            let p = g(k);
            Ok((jsv::to_num(jsv::prop(p, "x")?), jsv::to_num(jsv::prop(p, "y")?)))
        };
        match kind.as_str() {
            "linear" | "aligned" => {
                let (p2, p1, loc) = (pt("p2")?, pt("p1")?, pt("loc")?);
                let r = if kind == "aligned" { (p2.1 - p1.1).jatan2(p2.0 - p1.0) } else { jsv::or(g("rot"), 0.0) };
                let (ux, uy) = (r.jcos(), r.jsin());
                let d2 = (p2.0 - loc.0) * ux + (p2.1 - loc.1) * uy;
                d.set("defPoint", pn(loc.0 + ux * d2, loc.1 + uy * d2, 0.0));
                d.set("xline1", pn(p1.0, p1.1, 0.0));
                d.set("xline2", pn(p2.0, p2.1, 0.0));
                d.set("angle", if kind == "linear" { r } else { 0.0 });
            }
            "radius" | "diameter" => {
                let (p1, c) = (pt("p1")?, pt("c")?);
                let loc = pt("loc")?;
                let r = js::hypot(p1.0 - c.0, p1.1 - c.1);
                let dl = js::hypot(loc.0 - c.0, loc.1 - c.1);
                let a = if dl > 1e-12 { (loc.1 - c.1).jatan2(loc.0 - c.0) } else { (p1.1 - c.1).jatan2(p1.0 - c.0) };
                let near = pn(c.0 + r * a.jcos(), c.1 + r * a.jsin(), 0.0);
                let far = pn(c.0 - r * a.jcos(), c.1 - r * a.jsin(), 0.0);
                d.set("defPoint", if kind == "radius" { pn(c.0, c.1, 0.0) } else { far });
                d.set("center", near);
                d.set("leaderLen", 0.0);
            }
            "angular" => {
                let (loc, p1, p2, c) = (pt("loc")?, pt("p1")?, pt("p2")?, pt("c")?);
                d.set("defPoint", pn(loc.0, loc.1, 0.0));
                d.set("xline1", pn(p1.0, p1.1, 0.0));
                d.set("xline2", pn(p2.0, p2.1, 0.0));
                d.set("center", pn(c.0, c.1, 0.0));
            }
            _ => {
                self.note("other", 1.0);
                let mut o = base.clone();
                o.set("type", "INSERT");
                o.set("block", name);
                o.set("insert", pn(0.0, 0.0, 0.0));
                return Ok(vec![V::Obj(o)]);
            }
        }
        let items = |list: Vec<(i64, V)>| V::Arr(list.into_iter().map(|(c, v)| V::Arr(vec![V::from(c), v])).collect());
        let mut xd = Obj::new();
        xd.set("ACAD", items(props::dstyle_items(Some(&st), false)));
        let cc = props::cc_dim_items(Some(&st), false);
        if !cc.is_empty() {
            xd.set("CADCRAFT", V::Arr(cc.into_iter().map(|x| V::Arr(vec![n(1000.0), V::Str(x)])).collect()));
        }
        d.set("xdata", xd);
        Ok(vec![V::Obj(d)])
    }

    /// `viewport(e, base, id)`: a sheet's viewport → VIEWPORT.
    fn viewport(&mut self, e: &V, base: Obj, id: f64) -> V {
        let g = |k: &str| jsv::get(Some(e), k);
        let status = 0x8060
            | (if jsv::truthy(g("locked")) { 16384 } else { 0 })
            | (if matches!(g("on"), Some(V::Bool(false))) { 131_072 } else { 0 });
        let frz: Vec<V> = jsv::arr(g("frz")).iter().filter(|x| jsv::s(Some(x)).is_some_and(|s| !s.is_empty())).cloned().collect();
        let lov = if self.model_only { None } else { jsv::obj(g("lov")).filter(|o| !o.is_empty()).cloned() };
        let vh =
            lov.as_ref().map(|_| format!("CC{:X}", 0x7C00_0000u64.saturating_add(u64::try_from(self.vp_overrides.len()).unwrap_or(0))));
        if let (Some(l), Some(h)) = (lov, vh.as_ref()) {
            self.vp_overrides.push((h.clone(), l));
        }
        let mut o = base;
        if let Some(h) = vh {
            o.set("handle", h);
        }
        if let Some(tw) = jsv::fin(g("tw")).filter(|t| *t != 0.0) {
            o.set("twist", tw);
        }
        o.set("type", "VIEWPORT");
        o.set("center", p3(g("x"), g("y"), 0.0));
        o.set_opt("width", g("w").cloned());
        o.set_opt("height", g("h").cloned());
        o.set("viewCenter", p3(g("cx"), g("cy"), 0.0));
        o.set("viewTarget", pn(0.0, 0.0, 0.0));
        let sc = jsv::to_num(g("s"));
        o.set("viewHeight", jsv::to_num(g("h")) / (if sc > 0.0 { sc } else { 1.0 }));
        o.set("id", id);
        o.set("status", f64::from(status));
        if !frz.is_empty() {
            o.set("frozenLayers", V::Arr(frz));
        }
        V::Obj(o)
    }
}

fn i32f(x: f64) -> i64 {
    jsv::i32_of(x)
}
fn lw100(w: Ov, absent: f64) -> V {
    match w {
        None | Some(V::Null) => n(absent),
        Some(v) => {
            let x = jsv::to_num(Some(v));
            if x < 0.0 { v.clone() } else { n(js::round(x * 100.0)) }
        }
    }
}
fn obj_of_json(m: &serde_json::Map<String, serde_json::Value>) -> Obj {
    match jsv::from_json(&serde_json::Value::Object(m.clone())) {
        V::Obj(o) => o,
        _ => Obj::new(),
    }
}
fn dict_obj(handle: &str, owner: &str, entries: Obj) -> V {
    V::Obj(cc_io_dwg::obj! {
        "handle" => handle, "owner" => owner, "reactors" => V::Arr(vec![V::from(owner)]), "type" => "DICTIONARY",
        "entries" => entries, "hardOwner" => 1.0, "cloning" => 1.0
    })
}
/// `openRing(pts)`: the repeated closing point dropped.
fn open_ring(mut pts: Vec<(V, V)>) -> Vec<(V, V)> {
    if pts.len() > 1 {
        let nx = |v: &V| jsv::to_num(Some(v));
        if let (Some(f), Some(l)) = (pts.first(), pts.last())
            && js::hypot(nx(&f.0) - nx(&l.0), nx(&f.1) - nx(&l.1)) < 1e-12
        {
            pts.pop();
        }
    }
    pts
}
/// xclip.js `clipPoly(ref)`
fn clip_poly(e: &V) -> Option<Vec<V>> {
    let c = jsv::get(Some(e), "clip").filter(|c| c.truthy())?;
    let pts: Vec<V> = jsv::arr(jsv::get(Some(c), "pts"))
        .iter()
        .filter(|p| p.truthy() && jsv::fin(jsv::get(Some(p), "x")).is_some() && jsv::fin(jsv::get(Some(p), "y")).is_some())
        .cloned()
        .collect();
    (pts.len() >= 3).then_some(pts)
}
/// xclip.js `filterOf(clip)`
fn filter_of(pts: &[V]) -> Obj {
    let id: Vec<V> = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0].iter().map(|x| V::Num(*x)).collect();
    cc_io_dwg::obj! {
        "type" => "SPATIAL_FILTER",
        "points" => V::Arr(pts.iter().map(|p| { let mut o = Obj::new(); o.set_opt("x", jsv::get(Some(p), "x").cloned()); o.set_opt("y", jsv::get(Some(p), "y").cloned()); V::Obj(o) }).collect()),
        "extrusion" => pn(0.0, 0.0, 1.0),
        "origin" => pn(0.0, 0.0, 0.0),
        "displayBoundary" => 1.0,
        "frontClip" => V::Null,
        "backClip" => V::Null,
        "inverseBlockTransform" => V::Arr(id.clone()),
        "clipTransform" => V::Arr(id)
    }
}

struct Pic {
    file: V,
    insert: P2,
    u: P2,
    v: P2,
    size: V,
    fade: f64,
    clip: Option<Vec<(f64, f64)>>,
}

/// pictures.js `picturePlacement(ref, block)`
fn picture_placement(ins: &cc_cad::entity::Insert, block: &Block) -> Option<Pic> {
    let fr_i = block.entities.iter().position(|e| e.typ() == "poly" && e.extra.get("pic").is_some_and(|p| cc_cad::js::truthy(Some(p))))?;
    if block.entities.iter().enumerate().any(|(i, e)| i != fr_i && !cc_cad::js::truthy(e.extra.get("picName"))) {
        return None;
    }
    let fr = block.entities.get(fr_i)?;
    let p = jsv::from_json(fr.extra.get("pic")?);
    let g = |k: &str| jsv::to_num(jsv::get(Some(&p), k));
    let m = insert_matrix(ins, block);
    let at = m_apply(&m, P2::new(g("x"), g("y")));
    let vec = |x: f64, y: f64| P2::new(m[0] * x + m[2] * y, m[1] * x + m[3] * y);
    let u = vec(g("ux"), g("uy"));
    let v = vec(g("vx"), g("vy"));
    let size = V::Obj(
        cc_io_dwg::obj! { "x" => jsv::get(Some(&p), "w").cloned().unwrap_or(V::Null), "y" => jsv::get(Some(&p), "h").cloned().unwrap_or(V::Null) },
    );
    let fade = jsv::fin(jsv::get(Some(&p), "fade")).unwrap_or(0.0);
    let mut clip = None;
    if jsv::truthy(jsv::get(Some(&p), "clipped")) {
        let det = u.x * v.y - u.y * v.x;
        if det.abs() > 1e-300
            && let Geo::Poly(poly) = &fr.geo
        {
            clip = Some(
                poly.pts
                    .iter()
                    .map(|q| {
                        let w = m_apply(&m, P2::new(q.x, q.y));
                        let (dx, dy) = (w.x - at.x, w.y - at.y);
                        let a = (dx * v.y - dy * v.x) / det;
                        let b = (u.x * dy - u.y * dx) / det;
                        (a - 0.5, g("h") - 0.5 - b)
                    })
                    .collect(),
            );
        }
    }
    Some(Pic { file: jsv::get(Some(&p), "file").cloned().unwrap_or(V::Null), insert: at, u, v, size, fade, clip })
}

/// attrs.js `worldAttribs(e, blk)` (as dynamic values).
fn world_attribs(ins: &cc_cad::entity::Insert, blk: &Block) -> Vec<V> {
    let m = insert_matrix(ins, blk);
    let defs: &[serde_json::Value] = blk.attdefs.as_deref().unwrap_or(&[]);
    let mut attrs = ins.attrs.clone().unwrap_or_default();
    for d in defs {
        if cc_cad::js::truthy(d.get("constant")) {
            let tag = d.get("tag").map(cc_cad::js::to_js_string).unwrap_or_else(|| "undefined".into());
            attrs.remove(&tag);
        }
    }
    let mut e2 = ins.clone();
    e2.attrs = Some(attrs);
    let mut b2 = Block::new(&blk.name, blk.base, Vec::new());
    b2.attdefs = Some(defs.iter().filter(|d| !cc_cad::js::truthy(d.get("constant"))).cloned().collect());
    cc_cad_geom::geom::attr_texts(&e2, Some(&b2), true)
        .into_iter()
        .map(|t| {
            let tag = t.extra.get("tag").cloned();
            let inv = cc_cad::js::truthy(t.extra.get("invisible"));
            let mut moved = entity_v(&cc_cad_geom::tx::tx_entity(&t, &m));
            if let V::Obj(o) = &mut moved {
                match tag {
                    Some(tg) => o.set("tag", jsv::from_json(&tg)),
                    None => {
                        o.remove("tag");
                    }
                }
                o.set("invisible", inv);
            }
            moved
        })
        .collect()
}

/// `activeVports(vports, ext, twist)`: the split screen → the VPORT table's *Active records.
fn active_vports(vports: Ov, ext: &cc_cad_geom::geom::Bounds, twist: f64) -> V {
    let fb = if valid_bounds(ext) {
        Some((
            (ext.min_x + ext.max_x) / 2.0,
            (ext.min_y + ext.max_y) / 2.0,
            js::max_of([ext.max_y - ext.min_y, (ext.max_x - ext.min_x) / 1.6, 1e-6]) * 1.05,
        ))
    } else {
        None
    };
    V::Arr(
        crate::vports::vport_records(vports, fb, twist)
            .into_iter()
            .map(|r| {
                let mut o = Obj::new();
                o.set("name", "*Active");
                o.set("lowerLeft", jsv::p2(r.x0, r.y0));
                o.set("upperRight", jsv::p2(r.x1, r.y1));
                o.set("aspect", r.aspect);
                if r.tw != 0.0 && !r.tw.is_nan() {
                    o.set("twist", r.tw);
                }
                if r.h.is_some_and(|h| h > 0.0) {
                    let mut c = Obj::new();
                    c.set_opt("x", r.cx.map(V::Num));
                    c.set_opt("y", r.cy.map(V::Num));
                    o.set("center", c);
                    o.set_opt("viewHeight", r.h.map(V::Num));
                }
                if let Some(sh) = r.sheet.filter(|s| !s.is_empty()) {
                    o.set("xdata", cc_io_dwg::obj! { "CADCRAFT" => V::Arr(vec![V::Arr(vec![n(1000.0), V::Str(format!("PANESHEET:{}", slice16(&sh, 0, 245)))])]) });
                }
                V::Obj(o)
            })
            .collect(),
    )
}

/// What [`build_model`] gives: the cc-dwg model, CadCraft's counts, the 3D faces written, the top-level entities.
pub struct Built {
    pub model: Obj,
    pub counts: Obj,
    pub m3_faces: usize,
    pub dyn_note: Option<(usize, String)>,
    pub entities_types: Vec<String>,
}

/// `saveDwg(doc, { modelOnly: true, … })`: the cc-dwg model a drawing saves as.
#[allow(clippy::too_many_lines)]
pub fn build_model(doc: &Doc, opts: &SaveOpts, cat: &Catalog) -> R<Built> {
    let vkey = dwg_version_key(&opts.version).unwrap_or("2000");
    let code = DWG_VERSIONS.iter().find(|(k, _)| *k == vkey).map_or("AC1015", |(_, c)| *c);
    let mut sv = Sv {
        doc,
        vkey,
        model_only: opts.model_only,
        explode_leaders: opts.explode_leaders,
        counts: Obj::new(),
        used_lt: Vec::new(),
        lt_handles: Vec::new(),
        blocks: Vec::new(),
        ds: None,
        vp_overrides: Vec::new(),
        clip_objects: Vec::new(),
        pic_defs: Vec::new(),
        pic_objects: Vec::new(),
        dim_n: 0,
        text_styles: Vec::new(),
        dim_styles_out: Vec::new(),
    };
    let mut layers: Vec<Obj> = Vec::new();
    for l in doc.layers.values() {
        let lc = match &l.color {
            cc_cad::Color::Num(x) if *x != 0.0 && !x.is_nan() => V::Num(*x),
            cc_cad::Color::Str(st) if !st.is_empty() => V::Str(st.clone()),
            _ => n(7.0),
        };
        let color = sv.color(Some(&lc));
        let lt_in = l.linetype.as_ref().filter(|x| !x.is_empty()).map_or_else(|| s("Continuous"), |x| V::Str(x.clone()));
        let lt = sv.lt_of(Some(&lt_in));
        let mut o = Obj::new();
        o.set("name", l.name.clone());
        o.set("color", color);
        o.set("off", !l.on);
        o.set("frozen", l.frozen);
        o.set("locked", l.locked);
        o.set("plot", l.plot);
        o.set("frozenInNewViewports", l.new_vp_frozen.unwrap_or(false));
        o.set("linetype", lt);
        o.set("lineweight", lw100(l.lineweight.map(V::Num).as_ref(), -3.0));
        layers.push(o);
    }
    let model_ents: Vec<&Entity> = doc.model().collect();
    let mut insert_used: Vec<String> = Vec::new();
    {
        let mut add = |e: &Entity| {
            if let Geo::Insert(i) = &e.geo
                && !insert_used.contains(&i.block)
            {
                insert_used.push(i.block.clone());
            }
        };
        model_ents.iter().for_each(|e| add(e));
        for l in &doc.layouts {
            doc.space_entities(l.space).for_each(&mut add);
        }
        doc.blocks.values().for_each(|b| b.entities.iter().for_each(&mut add));
    }
    if !opts.model_only {
        sv.ds = Some(dyn_save(doc));
    }
    let mut dyn_records: Vec<V> = Vec::new();
    let dropped = sv.ds.as_ref().map_or(0.0, |d| d.dropped);
    if dropped > 0.0 {
        sv.note("dynDropped", dropped);
    }
    // text and dimension styles: every one the drawing keeps
    for (nm, v) in doc.text_styles.iter() {
        let vo = obj_of_json(v);
        let g = |k: &str| vo.get(k);
        sv.text_styles.push((
            jsv::upper(nm),
            cc_io_dwg::obj! {
                "name" => nm.clone(),
                "font" => jsv::orv(g("font"), s("txt")),
                "height" => jsv::orv(g("h"), n(0.0)),
                "widthFactor" => jsv::orv(g("wf"), n(1.0)),
                "oblique" => jsv::orv(g("obl"), n(0.0))
            },
        ));
    }
    for (nm, st) in doc.dim_styles.iter() {
        let so = obj_of_json(st);
        let vars = sv.ds_vars(Some(&so));
        let k = jsv::upper(nm);
        let entry = cc_io_dwg::obj! { "name" => nm.clone(), "vars" => vars };
        match sv.dim_styles_out.iter_mut().find(|(x, _)| *x == k) {
            Some(slot) => slot.1 = entry,
            None => sv.dim_styles_out.push((k, entry)),
        }
    }
    for b in doc.blocks.values() {
        if b.name.starts_with('*') && !insert_used.contains(&b.name) {
            continue;
        }
        let bx = js::or(Some(b.base.x), 0.0);
        let by = js::or(Some(b.base.y), 0.0);
        let root = b.xref.as_ref().is_some_and(|x| cc_cad::js::truthy(Some(x)) && x.get("of").is_none());
        if root && b.entities.is_empty() {
            let path = b
                .xref
                .as_ref()
                .and_then(|x| x.get("path"))
                .filter(|p| cc_cad::js::truthy(Some(p)))
                .map_or_else(|| b.name.clone(), cc_cad::js::to_js_string);
            sv.blocks.push(V::Obj(cc_io_dwg::obj! { "name" => b.name.clone(), "base" => pn(bx, by, 0.0), "entities" => V::Arr(Vec::new()), "flags" => 4.0, "xrefPath" => path }));
            sv.note("xrefLink", 1.0);
            continue;
        }
        if root {
            sv.note("xrefBound", 1.0);
        }
        let mut attdefs: Vec<V> = Vec::new();
        for d in b.attdefs.as_deref().unwrap_or(&[]) {
            let dv = jsv::from_json(d);
            let gd = |k: &str| jsv::get(Some(&dv), k);
            let mut o = sv.text_fields(&dv, V::Str(one_line(&jsv::str_or_empty(gd("def")))));
            o.set("type", "ATTDEF");
            o.set("layer", jsv::orv(gd("layer"), s("0")));
            let c = sv.color(gd("color"));
            o.set("color", c);
            o.set_opt("tag", gd("tag").cloned());
            o.set("prompt", jsv::orv(gd("prompt"), s("")));
            let fl = (if jsv::truthy(gd("invisible")) { 1 } else { 0 })
                | (if jsv::truthy(gd("constant")) { 2 } else { 0 })
                | (if jsv::truthy(gd("preset")) { 8 } else { 0 });
            o.set("flags", f64::from(fl));
            attdefs.push(V::Obj(o));
        }
        let dynd = sv.ds.as_ref().and_then(|d| d.get(&b.name)).cloned();
        if let Some(dynd) = dynd {
            let ad: Vec<usize> = jsv::arr(dynd.get("ad")).iter().map(|x| jsv::f2us(jsv::to_num(Some(x)))).collect();
            let all: Vec<V> = jsv::arr(dynd.get("all")).to_vec();
            let hs: Vec<V> = jsv::arr(dynd.get("h")).to_vec();
            let shown: Vec<V> = jsv::arr(dynd.get("shown")).to_vec();
            let mut ents: Vec<V> = Vec::new();
            for (i, parts) in all.iter().enumerate() {
                let mut out: Vec<V> = match ad.iter().position(|x| *x == i) {
                    Some(k) => attdefs.get(k).cloned().into_iter().collect(),
                    None => {
                        let mut o = Vec::new();
                        for p in jsv::arr(Some(parts)) {
                            o.extend(sv.conv(p)?);
                        }
                        o
                    }
                };
                if let (Some(V::Obj(f)), Some(h)) = (out.first_mut(), hs.get(i).filter(|h| h.truthy())) {
                    f.set("handle", h.clone());
                }
                if !shown.get(i).is_some_and(V::truthy) {
                    for x in out.iter_mut() {
                        if let V::Obj(o) = x {
                            o.set("invisible", true);
                        }
                    }
                }
                ents.extend(out);
            }
            let record = sv.ds.as_ref().and_then(|d| d.record(&b.name)).map_or(V::Null, V::Str);
            sv.blocks.push(V::Obj(cc_io_dwg::obj! { "name" => b.name.clone(), "record" => record.clone(), "base" => pn(bx, by, 0.0), "entities" => V::Arr(ents) }));
            let mut r = cc_io_dwg::obj! { "handle" => record, "name" => b.name.clone() };
            if let Some(x) = dynd.get("x").filter(|x| x.truthy()) {
                r.set("extDict", x.clone());
            }
            if let Some(x) = dynd.get("xd").filter(|x| x.truthy()) {
                r.set("xdata", x.clone());
            }
            dyn_records.push(V::Obj(r));
            continue;
        }
        let mut ents = Vec::new();
        for e in &b.entities {
            ents.extend(sv.conv(&ev(e))?);
        }
        ents.extend(attdefs);
        sv.blocks.push(V::Obj(cc_io_dwg::obj! { "name" => b.name.clone(), "base" => pn(bx, by, 0.0), "entities" => V::Arr(ents) }));
    }
    // constraints + parameters (gcons.js): CADCRAFT xdata "CCREF:<key>" on each constrained entity, block CADCRAFT_PARAMETRIC
    let pf = cc_gcons::gcons::for_file(doc);
    let strings = |strs: &[String]| {
        V::Obj(cc_io_dwg::obj! { "CADCRAFT" => V::Arr(strs.iter().map(|x| V::Arr(vec![n(1000.0), V::Str(x.clone())])).collect()) })
    };
    let mut entities: Vec<V> = Vec::new();
    for e in &model_ents {
        let mut out = sv.conv(&ev(e))?;
        let key = pf.as_ref().and_then(|p| e.id.and_then(|id| p.refs.iter().find(|(i, _)| *i == id).map(|(_, k)| k.clone())));
        if let Some(key) = key
            && let Some(V::Obj(first)) = out.first_mut()
        {
            let mut xd = jsv::obj(first.get("xdata")).cloned().unwrap_or_default();
            xd.set("CADCRAFT", V::Arr(vec![V::Arr(vec![n(1000.0), V::Str(format!("CCREF:{key}"))])]));
            first.set("xdata", V::Obj(xd));
        }
        entities.extend(out);
    }
    if let Some(pf) = &pf {
        let pts: Vec<V> = pf
            .points
            .iter()
            .map(|strs| {
                V::Obj(cc_io_dwg::obj! { "type" => "POINT", "layer" => "0", "position" => pn(0.0, 0.0, 0.0), "xdata" => strings(strs) })
            })
            .collect();
        sv.blocks.push(V::Obj(
            cc_io_dwg::obj! { "name" => cc_gcons::gcons::PARAM_BLOCK, "base" => pn(0.0, 0.0, 0.0), "entities" => V::Arr(pts) },
        ));
    }
    // the 3D model (model3d.js): every face as 3DFACE on layer CC-3D, and its exact copy in block CADCRAFT_3D
    let units = jsv::str_or_empty(obj_of_json(&doc.meta).get("units"));
    let m3 =
        if doc.faces3d.is_empty() { None } else { crate::m3d_file::for_file3d(&doc.faces3d, &units, &cat.t("model3d.faces_file", &[])) };
    if let Some(m3) = &m3 {
        if !layers.iter().any(|l| jsv::s(l.get("name")) == Some(m3.layer)) {
            layers.push(cc_io_dwg::obj! { "name" => m3.layer, "color" => V::Obj(cc_io_dwg::obj! { "index" => 8.0 }), "off" => false, "frozen" => false, "locked" => false, "plot" => true });
        }
        for f in &m3.faces {
            entities.push(V::Obj(cc_io_dwg::obj! {
                "type" => "3DFACE", "layer" => m3.layer,
                "points" => V::Arr(f.pts.iter().map(|p| pn(p[0], p[1], p[2])).collect()),
                "invisibleEdges" => f64::from(f.invis)
            }));
        }
        let pts: Vec<V> = m3
            .points
            .iter()
            .map(|strs| {
                V::Obj(cc_io_dwg::obj! {
                    "type" => "POINT", "layer" => "0", "position" => pn(0.0, 0.0, 0.0),
                    "xdata" => V::Obj(cc_io_dwg::obj! { "CADCRAFT" => V::Arr(strs.iter().map(|x| V::Arr(vec![n(1000.0), V::Str(x.clone())])).collect()) })
                })
            })
            .collect();
        sv.blocks
            .push(V::Obj(cc_io_dwg::obj! { "name" => crate::model3d::M3D_BLOCK, "base" => pn(0.0, 0.0, 0.0), "entities" => V::Arr(pts) }));
    }
    // sheets: each layout is a paper space whose first viewport is the sheet itself
    let mut layouts_out: Vec<V> = vec![V::Obj(cc_io_dwg::obj! { "name" => "Model", "tabOrder" => 0.0 })];
    let mut spaces: Vec<V> = Vec::new();
    for (i, l) in doc.layouts.iter().enumerate() {
        let p = obj_of_json(&l.paper);
        let gp = |k: &str| p.get(k);
        let pn_ = |k: &str| jsv::to_num(p.get(k));
        let inch = jsv::s(gp("unit")) == Some("in");
        let mm = if inch { 25.4 } else { 1.0 };
        let record = format!("CC{}", i.saturating_add(1));
        let (x0, y0, w, h) = (pn_("x0"), pn_("y0"), pn_("w"), pn_("h"));
        let mut sheet_e = Obj::new();
        sheet_e.set("x", x0 + w / 2.0);
        sheet_e.set("y", y0 + h / 2.0);
        sheet_e.set_opt("w", gp("w").cloned());
        sheet_e.set_opt("h", gp("h").cloned());
        sheet_e.set("cx", x0 + w / 2.0);
        sheet_e.set("cy", y0 + h / 2.0);
        sheet_e.set("s", 1.0);
        let c0 = sv.color(None);
        let mut sheet = sv.viewport(&V::Obj(sheet_e), cc_io_dwg::obj! { "layer" => "0", "color" => c0 }, 1.0);
        if let V::Obj(so) = &mut sheet {
            let it = |c: f64, v: V| V::Arr(vec![n(c), v]);
            so.set(
                "xdata",
                cc_io_dwg::obj! { "CADCRAFT" => V::Arr(vec![
                    it(1000.0, s("LAYOUT")),
                    it(1000.0, V::Str(slice16(&l.name, 0, 255))),
                    it(1000.0, gp("unit").cloned().unwrap_or(V::Null)),
                    it(1000.0, jsv::orv(gp("key"), s(""))),
                    it(1040.0, gp("x0").cloned().unwrap_or(V::Null)),
                    it(1040.0, gp("y0").cloned().unwrap_or(V::Null)),
                    it(1040.0, gp("w").cloned().unwrap_or(V::Null)),
                    it(1040.0, gp("h").cloned().unwrap_or(V::Null)),
                ]) },
            );
        }
        let mut vp_id = 1.0;
        let mut ents = vec![sheet];
        for e in doc.space_entities(l.space) {
            let v = ev(e);
            if e.typ() == "viewport" {
                vp_id += 1.0;
                let layer = jsv::orv(jsv::get(Some(&v), "layer"), s("0"));
                let c = sv.color(jsv::get(Some(&v), "color"));
                ents.push(sv.viewport(&v, cc_io_dwg::obj! { "layer" => layer, "color" => c }, vp_id));
            } else {
                ents.extend(sv.conv(&v)?);
            }
        }
        let name = if i > 0 { format!("*Paper_Space{}", i.saturating_sub(1)) } else { "*Paper_Space".into() };
        spaces.push(V::Obj(
            cc_io_dwg::obj! { "name" => name, "record" => record.clone(), "base" => pn(0.0, 0.0, 0.0), "entities" => V::Arr(ents) },
        ));
        let pname = if jsv::truthy(gp("key")) {
            gp("key").cloned().unwrap_or(V::Null)
        } else {
            let ws = jsv::to_str(gp("w"));
            let hs = jsv::to_str(gp("h"));
            V::Str(if inch { format!("Custom_({ws}_x_{hs}_Inches)") } else { format!("Custom_({ws}_x_{hs}_MM)") })
        };
        layouts_out.push(V::Obj(cc_io_dwg::obj! {
            "name" => l.name.clone(),
            "tabOrder" => jsv::us2f(i) + 1.0,
            "block" => record,
            "paper" => V::Obj(cc_io_dwg::obj! {
                "name" => pname, "w" => w * mm, "h" => h * mm, "units" => if inch { 0.0 } else { 1.0 }, "rotation" => 0.0, "plotType" => 5.0,
                "scaleType" => 16.0, "scale" => 1.0, "left" => 0.0, "bottom" => 0.0, "right" => 0.0, "top" => 0.0
            }),
            "limits" => V::Obj(cc_io_dwg::obj! { "min" => jsv::p2(x0, y0), "max" => jsv::p2(x0 + w, y0 + h) }),
            "insbase" => pn(0.0, 0.0, 0.0)
        }));
    }
    let mut ext = empty_bounds();
    for e in &model_ents {
        let b = entity_bounds(e, Some(&doc.blocks));
        if valid_bounds(&b) {
            ext.min_x = js::min(ext.min_x, b.min_x);
            ext.min_y = js::min(ext.min_y, b.min_y);
            ext.max_x = js::max(ext.max_x, b.max_x);
            ext.max_y = js::max(ext.max_y, b.max_y);
        }
    }
    let meta = obj_of_json(&doc.meta);
    let mut header = Obj::new();
    header.set("INSUNITS", jsv::i2f(unit_code(&units)));
    header.set("MEASUREMENT", if IMPERIAL.contains(&units.as_str()) { 0.0 } else { 1.0 });
    header.set("LUNITS", jsv::orv(meta.get("lunits"), n(2.0)));
    if let Some(p) = jsv::fin(meta.get("luprec")) {
        header.set("LUPREC", p);
    }
    if let Some(ib) = meta.get("insbase").filter(|x| x.truthy()) {
        header.set("INSBASE", pn(jsv::or(jsv::get(Some(ib), "x"), 0.0), jsv::or(jsv::get(Some(ib), "y"), 0.0), 0.0));
    }
    let axes = |x: f64, y: f64, ang: f64| (pn(x, y, 0.0), pn(ang.jcos(), ang.jsin(), 0.0), pn(-ang.jsin(), ang.jcos(), 0.0));
    let ucs_v = doc.ucs.as_ref().map(jsv::from_json);
    if let Some((x, y, a)) = crate::dxf_write::clean_ucs(jsv::get(ucs_v.as_ref(), "current")) {
        let (o, xa, ya) = axes(x, y, a);
        header.set("UCSORG", o);
        header.set("UCSXDIR", xa);
        header.set("UCSYDIR", ya);
    }
    let ucs_table: Vec<V> = crate::dxf_write::ucs_named(ucs_v.as_ref())
        .into_iter()
        .map(|(nm, (x, y, a))| {
            let (o, xa, ya) = axes(x, y, a);
            V::Obj(cc_io_dwg::obj! { "name" => nm, "origin" => o, "xAxis" => xa, "yAxis" => ya })
        })
        .collect();
    if valid_bounds(&ext) {
        header.set("EXTMIN", pn(ext.min_x, ext.min_y, 0.0));
        header.set("EXTMAX", pn(ext.max_x, ext.max_y, 0.0));
    }
    let views: Vec<V> = doc
        .views
        .iter()
        .map(|v| {
            V::Obj(cc_io_dwg::obj! {
                "name" => v.name.clone(), "center" => jsv::p2(v.cx, v.cy), "width" => v.w, "height" => v.h,
                "target" => pn(0.0, 0.0, 0.0), "direction" => pn(0.0, 0.0, 1.0), "paperSpace" => v.layout.as_ref().is_some_and(|l| !l.is_empty())
            })
        })
        .collect();
    let paper_views = doc.views.iter().filter(|v| v.layout.as_ref().is_some_and(|l| !l.is_empty())).count();
    if paper_views > 0 {
        sv.note("paperView", jsv::us2f(paper_views));
    }
    // per-viewport layer overrides (0.5 VP.3): XRECORDs ADSK_XREC_LAYER_*_OVR in each layer's extension dictionary
    let mut ovr_objects: Vec<V> = Vec::new();
    if !sv.vp_overrides.is_empty() {
        let mut lt_h: Vec<(String, String)> = Vec::new();
        let mut k = 0u64;
        let vpo = sv.vp_overrides.clone();
        for l in layers.iter_mut() {
            let lname = jsv::upper(&jsv::to_str(l.get("name")));
            let mut entries = Obj::new();
            let mut recs: Vec<Obj> = Vec::new();
            for (kind, tag) in [
                ("COLOR", "ADSK_LYR_COLOR_OVERRIDE"),
                ("LINETYPE", "ADSK_LYR_LINETYPE_OVERRIDE"),
                ("LINEWT", "ADSK_LYR_LINEWT_OVERRIDE"),
                ("ALPHA", "ADSK_LYR_ALPHA_OVERRIDE"),
            ] {
                let mut data: Vec<V> = Vec::new();
                for (vh, lov) in &vpo {
                    let o = jsv::keys_js(lov).into_iter().find(|nm| jsv::upper(nm) == lname).and_then(|nm| lov.get(nm)).cloned();
                    let Some(o) = o.filter(V::truthy) else { continue };
                    let go = |k: &str| jsv::get(Some(&o), k);
                    let val: Option<(f64, V)> = match kind {
                        "COLOR" => match go("color") {
                            None | Some(V::Null) => None,
                            Some(V::Str(c)) => {
                                let rgb = i32f(jsv::parse_int_radix16(c.get(1..).unwrap_or(""))) & 0xff_ffff;
                                Some((420.0, V::Num(js::int32(jsv::i2f(0xC200_0000i64 | rgb)))))
                            }
                            Some(c) => Some((420.0, V::Num(js::int32(jsv::i2f(0xC300_0000i64 | (i32f(jsv::to_num(Some(c))) & 0xff)))))),
                        },
                        "LINETYPE" => {
                            if jsv::truthy(go("lt")) {
                                let ltn = jsv::to_str(Some(&sv.lt_of(go("lt"))));
                                let raw = jsv::to_str(go("lt"));
                                let h = match lt_h.iter().find(|(x, _)| *x == ltn) {
                                    Some((_, h)) => h.clone(),
                                    None => {
                                        let h = format!("CC{:X}", 0x7D00_0000u64.saturating_add(u64::try_from(lt_h.len()).unwrap_or(0)));
                                        match lt_h.iter_mut().find(|(x, _)| *x == raw) {
                                            Some(slot) => slot.1 = h,
                                            None => lt_h.push((raw.clone(), h)),
                                        }
                                        lt_h.iter().find(|(x, _)| *x == raw).map(|(_, h)| h.clone()).unwrap_or_default()
                                    }
                                };
                                Some((343.0, V::Str(h)))
                            } else {
                                None
                            }
                        }
                        "LINEWT" => jsv::fin(go("lw")).filter(|w| *w >= 0.0).map(|w| (91.0, V::Num(js::round(w * 100.0)))),
                        _ => {
                            if jsv::fin(go("tr")).is_some() {
                                tr_word(go("tr")).map(|w| (440.0, V::Num(js::int32(w))))
                            } else {
                                None
                            }
                        }
                    };
                    if let Some((c, v)) = val {
                        data.push(V::Arr(vec![n(335.0), V::Str(vh.clone())]));
                        data.push(V::Arr(vec![n(c), v]));
                    }
                }
                if !data.is_empty() {
                    let xh = format!("CC{:X}", 0x7E00_0000u64.saturating_add(k));
                    k = k.saturating_add(1);
                    entries.set(format!("ADSK_XREC_LAYER_{kind}_OVR"), xh.clone());
                    let mut d = vec![V::Arr(vec![n(102.0), V::Str(format!("{{{tag}"))])];
                    d.extend(data);
                    d.push(V::Arr(vec![n(102.0), s("}")]));
                    recs.push(cc_io_dwg::obj! { "handle" => xh, "type" => "XRECORD", "data" => V::Arr(d), "cloning" => 1.0 });
                }
            }
            if recs.is_empty() {
                continue;
            }
            let lh = format!("CC{:X}", 0x7E00_0000u64.saturating_add(k));
            k = k.saturating_add(1);
            let xd = format!("CC{:X}", 0x7E00_0000u64.saturating_add(k));
            k = k.saturating_add(1);
            l.set("handle", lh.clone());
            l.set("extDict", xd.clone());
            ovr_objects.push(V::Obj(cc_io_dwg::obj! {
                "handle" => xd.clone(), "owner" => lh.clone(), "reactors" => V::Arr(vec![V::Str(lh)]), "type" => "DICTIONARY",
                "entries" => entries, "hardOwner" => 1.0, "cloning" => 1.0
            }));
            for mut r in recs {
                r.set("owner", xd.clone());
                r.set("reactors", V::Arr(vec![V::Str(xd.clone())]));
                ovr_objects.push(V::Obj(r));
            }
        }
        for (name, lh) in lt_h {
            if !sv.used_lt.contains(&name) {
                sv.used_lt.push(name.clone());
            }
            match sv.lt_handles.iter_mut().find(|(x, _)| *x == name) {
                Some(slot) => slot.1 = lh,
                None => sv.lt_handles.push((name, lh)),
            }
        }
    }
    let linetypes: Vec<V> = sv
        .used_lt
        .iter()
        .map(|name| {
            let pat = cc_cad_geom::linetypes::pattern_of(doc, Some(name)).unwrap_or_default();
            let desc = doc.linetypes.get(name).map(|l| l.description.clone()).filter(|d| !d.is_empty()).unwrap_or_else(|| name.clone());
            let mut o = Obj::new();
            o.set("name", name.clone());
            if let Some((_, h)) = sv.lt_handles.iter().find(|(x, _)| x == name) {
                o.set("handle", h.clone());
            }
            o.set("description", desc);
            o.set("patternLength", pat.iter().fold(0.0, |a, v| a + v.abs()));
            o.set("elements", V::Arr(pat.iter().map(|l| V::Obj(cc_io_dwg::obj! { "length" => *l })).collect()));
            V::Obj(o)
        })
        .collect();
    let lts = meta.get("ltscale");
    if jsv::to_num(lts) > 0.0 && !props::strict_eq(lts, Some(&n(1.0))) {
        header.set_opt("LTSCALE", lts.cloned());
    }
    let cd = obj_of_json(&doc.current.dim);
    let std_vars = sv.ds_vars(Some(&cd));
    let mut dimstyles = vec![V::Obj(cc_io_dwg::obj! { "name" => "Standard", "vars" => std_vars })];
    dimstyles.extend(sv.dim_styles_out.iter().filter(|(_, d)| !is_standard(d.get("name"))).map(|(_, d)| V::Obj(d.clone())));
    let mut tables = Obj::new();
    tables.set("layers", V::Arr(layers.into_iter().map(V::Obj).collect()));
    tables.set("views", V::Arr(views));
    tables.set("linetypes", V::Arr(linetypes));
    tables.set("appids", V::Arr(vec![V::Obj(cc_io_dwg::obj! { "name" => "CADCRAFT" })]));
    tables.set("dimstyles", V::Arr(dimstyles));
    tables.set("styles", V::Arr(sv.text_styles.iter().map(|(_, o)| V::Obj(o.clone())).collect()));
    let vports_v = doc.vports.as_ref().map(jsv::from_json);
    let panes = crate::vports::clean_panes(vports_v.as_ref());
    if panes.is_some() || (doc.view_twist != 0.0 && !doc.view_twist.is_nan()) {
        tables.set("vports", active_vports(vports_v.as_ref(), &ext, doc.view_twist));
    }
    if !ucs_table.is_empty() {
        tables.set("ucs", V::Arr(ucs_table));
    }
    let top_types: Vec<String> = entities.iter().map(|e| jsv::to_str(jsv::get(Some(e), "type"))).collect();
    let mut blocks_all = vec![V::Obj(cc_io_dwg::obj! { "name" => "*Model_Space", "entities" => V::Arr(entities) })];
    blocks_all.extend(spaces);
    blocks_all.extend(std::mem::take(&mut sv.blocks));
    let mut model = Obj::new();
    model.set("header", header);
    model.set("tables", tables);
    model.set("blocks", V::Arr(blocks_all));
    model.set("layouts", V::Arr(layouts_out));
    let mut dyn_note = None;
    if !dyn_records.is_empty()
        && let Some(ds) = sv.ds.as_ref()
    {
        let defs = dyn_records
            .iter()
            .filter(|r| ds.get(&jsv::to_str(jsv::get(Some(r), "name"))).is_some_and(|d| jsv::truthy(d.get("def"))))
            .count();
        if let Some(V::Obj(t)) = model.get_mut("tables") {
            t.set("blockRecords", V::Arr(dyn_records.clone()));
        }
        model.set("objects", V::Arr(ds.objects()));
        model.set("classes", V::Arr(ds.classes.clone()));
        model.set("version", ds.version.clone());
        if let Some(m) = ds.maintenance {
            model.set("maintenance", m);
        }
        if !ds.version.is_empty() && ds.version != code {
            dyn_note = Some((defs, ds.version.clone()));
        }
    }
    if !sv.pic_objects.is_empty() || !ovr_objects.is_empty() || !sv.clip_objects.is_empty() {
        let mut objs = match model.remove("objects") {
            Some(V::Arr(a)) => a,
            _ => Vec::new(),
        };
        objs.extend(sv.pic_objects.iter().cloned());
        objs.extend(ovr_objects);
        objs.extend(sv.clip_objects.iter().cloned());
        model.set("objects", V::Arr(objs));
    }
    Ok(Built { model, counts: sv.counts, m3_faces: m3.as_ref().map_or(0, |m| m.faces.len()), dyn_note, entities_types: top_types })
}

/// dxf.js `unitCode(u)`
fn unit_code(u: &str) -> i64 {
    match u {
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

/// `saveDwg(doc, { version, created })` → the DWG bytes and the report { version, notes, counts, top, … }.
pub fn save_dwg(doc: &Doc, opts: &SaveOpts) -> Result<Saved, JsErr> {
    save_dwg_with(doc, opts, Catalog::en())
}

/// `saveDwg` with a catalog for the notes.
pub fn save_dwg_with(doc: &Doc, opts: &SaveOpts, cat: &Catalog) -> Result<Saved, JsErr> {
    let vkey = dwg_version_key(&opts.version).unwrap_or("2000");
    let code = DWG_VERSIONS.iter().find(|(k, _)| *k == vkey).map_or("AC1015", |(_, c)| *c);
    let built = build_model(doc, &SaveOpts { model_only: false, ..opts.clone() }, cat)?;
    let wo = cc_io_dwg::WriteOpts {
        version: code.to_string(),
        created_ms: opts.created_ms,
        guid: if opts.guid_sequence { cc_io_dwg::GuidMode::Sequence } else { cc_io_dwg::GuidMode::Random },
    };
    let (bytes, report) = cc_io_dwg::write_dwg_opts(&built.model, &wo).map_err(JsErr)?;
    let mut top = Obj::new();
    for t in &built.entities_types {
        let k = if t == "DIMENSION" { "DIMENSION".to_string() } else { t.clone() };
        let v = jsv::or(top.get(&k), 0.0) + 1.0;
        top.set(k, v);
    }
    let c = |k: &str| jsv::to_num(built.counts.get(k));
    let has = |k: &str| built.counts.get(k).is_some_and(V::truthy);
    let mut notes: Vec<V> = Vec::new();
    let mut push = |s: String| notes.push(V::Str(s));
    if has("truecolor") && vkey == "2000" {
        push(cat.t("dwg.save.truecolor", &[]));
    }
    if has("hatchUnknown") {
        push(cat.t("dwg.save.hatch.unknown", &i18n::n(c("hatchUnknown"))));
    }
    if has("mtextwidth") {
        push(cat.t("dwg.save.mtextwidth", &[]));
    }
    if has("paperView") {
        push(cat.t("dwg.save.paperview", &i18n::n(c("paperView"))));
    }
    if built.m3_faces > 0 {
        push(cat.t(
            "dwg.save.model3d",
            &[("n", built.m3_faces.to_string()), ("layer", crate::model3d::M3D_LAYER.into()), ("block", crate::model3d::M3D_BLOCK.into())],
        ));
    }
    for (k, key) in [("xrefBound", "dwg.save.xref.bound"), ("xrefLink", "dwg.save.xref.link"), ("other", "dwg.save.other")] {
        if has(k) {
            push(cat.t(key, &i18n::n(c(k))));
        }
    }
    if has("material") {
        push(cat.t("dwg.save.material", &[]));
    }
    if has("tr2000") {
        push(cat.t("dwg.save.tr2000", &[]));
    }
    for (k, key) in [("mask2000", "dwg.save.mask2000"), ("clipDyn", "dwg.save.clipdyn"), ("dynDropped", "dwg.save.dyn.dropped")] {
        if has(k) {
            push(cat.t(key, &i18n::n(c(k))));
        }
    }
    if let Some((defs, _)) = &built.dyn_note {
        let vname = cat.t("dwg.version", &[("v", vkey.to_string())]);
        push(cat.t("dwg.save.dyn.version", &[("n", defs.to_string()), ("v", vname)]));
    }
    let own: Vec<V> = jsv::arr(report.get("notes")).iter().filter(|t| !jsv::to_str(Some(t)).contains("true colour")).cloned().collect();
    notes.extend(own);
    let mut counts = jsv::obj(report.get("counts")).cloned().unwrap_or_default();
    counts.assign(built.counts.clone());
    let mut rep = report.clone();
    rep.set("version", vkey);
    rep.set("notes", V::Arr(notes));
    rep.set("counts", counts);
    rep.set("top", top);
    Ok(Saved { bytes, report: jsv::into_json(V::Obj(rep)) })
}
