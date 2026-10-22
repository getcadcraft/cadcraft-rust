//! The Properties panel's model (propdefs.js and propexpr.js): what each kind of object shows, the value every
//! selected object shares (or "varies"), how a typed entry reads, and the change — every object of a group in ONE
//! undo step ("PROPERTIES"); objects on locked layers are left alone and counted.
//!
//! Entities are edited as their JSON objects, as the JS does (`{ ...e, k: v }`, `put` dropping a key that equals its
//! default), then read back. Expressions (`=2*45`, `3'6"/2`) go through PartCraft's port of expr.js (the same file in
//! both 0.5 apps). Not ported: the Custom section of dynamic blocks (visibility state list and parameter read-outs) —
//! a dynamic block reference shows nothing there; add-on sections (P.13).

use cc_cad::entity::{Entity, Geo, Obj};
use cc_cad::{Doc, js, num};
use cc_cad_geom::dims::dim_measure;
use cc_cad_geom::dimstyle::{ARROWS, TOL_MODES, dim_extra, dim_style_text};
use cc_cad_geom::edit::{curve_length, entity_area};
use cc_cad_geom::geom::{PI, TAU, deg, norm_angle, rad};
use cc_cad_geom::hatch::pattern_names;
use cc_cad_geom::jsmath::JsMath;
use cc_cad_geom::units::{is_imperial_format, lunit_code, parse_length};
use serde_json::{Map, Value};

use crate::lang::Lang;

/// Section keys in panel order (labels `props.sec.<key>`).
pub const SECTIONS: [&str; 11] = ["general", "geometry", "custom", "text", "lines", "fit", "units", "alt", "tol", "pattern", "misc"];

/// A value as the panel shows it: what every object shares, or "varies".
#[derive(Clone, Debug, PartialEq)]
pub enum PVal {
    V(Value),
    Varies,
}

/// What `get` / `set` see: the drawing, the group's objects, the polyline vertex the panel is on.
pub struct PCtx<'a> {
    pub doc: &'a Doc,
    pub ents: &'a [Entity],
    pub vertex: i64,
    pub lang: &'a Lang,
}

type Get = Box<dyn Fn(&Obj, &PCtx) -> PVal>;
type Set = Box<dyn Fn(&Obj, &Value, &PCtx) -> Result<Obj, String>>;
type When = Box<dyn Fn(&Obj, &PCtx) -> bool>;
type Choices = Box<dyn Fn(&PCtx) -> Vec<(Value, String)>>;

/// One field of the panel (a propdefs.js descriptor).
pub struct Def {
    pub key: String,
    pub sec: &'static str,
    /// 'len' 'ang' 'num' 'int' 'area' 'text' 'textarea' 'bool' 'choice' 'layer' 'color' 'lt' 'lw' 'tr' 'mat' 'ro' 'ro-len'
    /// 'ro-area' 'vertex'
    pub kind: &'static str,
    pub label: Option<String>,
    pub title: Option<&'static str>,
    pub gt0: bool,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub one: bool,
    /// a point's (name, axis): the panel puts a pick button on its X field
    pub pt: Option<(&'static str, &'static str)>,
    /// a model length shown on a sheet (viewport view centre)
    pub model: bool,
    /// a paper length (viewport size)
    pub paper: bool,
    /// a colour field that offers "none" (hatch background)
    pub none: bool,
    pub types: Option<&'static [&'static str]>,
    pub choices: Option<Choices>,
    pub get: Get,
    pub set: Option<Set>,
    pub when: Option<When>,
}

impl Def {
    fn new(key: &str, sec: &'static str, kind: &'static str, get: Get) -> Def {
        Def {
            key: key.into(),
            sec,
            kind,
            label: None,
            title: None,
            gt0: false,
            min: None,
            max: None,
            one: false,
            pt: None,
            model: false,
            paper: false,
            none: false,
            types: None,
            choices: None,
            get,
            set: None,
            when: None,
        }
    }
    fn set(mut self, f: Set) -> Def {
        self.set = Some(f);
        self
    }
    fn when(mut self, f: When) -> Def {
        self.when = Some(f);
        self
    }
    fn gt0(mut self) -> Def {
        self.gt0 = true;
        self
    }
    fn min(mut self, v: f64) -> Def {
        self.min = Some(v);
        self
    }
    fn max(mut self, v: f64) -> Def {
        self.max = Some(v);
        self
    }
    fn title(mut self, t: &'static str) -> Def {
        self.title = Some(t);
        self
    }
    fn label(mut self, l: String) -> Def {
        self.label = Some(l);
        self
    }
    fn choices(mut self, f: Choices) -> Def {
        self.choices = Some(f);
        self
    }
}

// ---------------------------------------------------------------------------------------------------------------
// helpers: JS reads on entity objects

fn n_of(e: &Obj, k: &str) -> f64 {
    js::to_number_opt(e.get(k))
}
/// `num0(e[k])`: a finite number, else 0.
fn num0(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().filter(|x| x.is_finite()).unwrap_or(0.0),
        _ => 0.0,
    }
}
fn nv(v: f64) -> PVal {
    PVal::V(num(v))
}
fn val_num(v: &Value) -> f64 {
    js::to_number(v)
}
/// `put(e, k, v, dflt)`: absent when it equals its default (or is undefined / null).
fn put(e: &Obj, k: &str, v: Option<Value>, dflt: &Value) -> Obj {
    let mut n = e.clone();
    match v {
        None | Some(Value::Null) => {
            n.shift_remove(k);
        }
        Some(v) if js_eq(&v, dflt) => {
            n.shift_remove(k);
        }
        Some(v) => {
            n.insert(k.into(), v);
        }
    }
    n
}
/// `a === b` for JSON values (numbers by value).
fn js_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}
fn with(e: &Obj, k: &str, v: Value) -> Obj {
    let mut n = e.clone();
    n.insert(k.into(), v);
    n
}
fn s_of(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(v) => js::to_js_string(v),
    }
}
fn typ(e: &Obj) -> &str {
    e.get("type").and_then(Value::as_str).unwrap_or("")
}
fn ent_of(e: &Obj) -> Entity {
    Entity::from_value(&Value::Object(e.clone()))
}
fn truthy(e: &Obj, k: &str) -> bool {
    js::truthy(e.get(k))
}

pub struct PropErr(pub String);

fn perr(lang: &Lang, key: &str) -> String {
    lang.s(key)
}

// ---------------------------------------------------------------------------------------------------------------
// general (every type)

const TH_TYPES: &[&str] = &["line", "circle", "arc", "poly", "point", "text"];

fn general() -> Vec<Def> {
    let mut v = vec![
        Def::new(
            "layer",
            "general",
            "layer",
            Box::new(|e, _| PVal::V(e.get("layer").cloned().unwrap_or_else(|| Value::String("0".into())))),
        )
        .set(Box::new(|e, v, _| Ok(with(e, "layer", v.clone())))),
        Def::new("color", "general", "color", Box::new(|e, _| PVal::V(e.get("color").cloned().unwrap_or(Value::Null))))
            .set(Box::new(|e, v, _| Ok(with(e, "color", v.clone())))),
        Def::new("lt", "general", "lt", Box::new(|e, _| PVal::V(e.get("lt").cloned().unwrap_or(Value::Null))))
            .set(Box::new(|e, v, _| Ok(put(e, "lt", Some(v.clone()), &Value::Null)))),
        Def::new("lts", "general", "num", Box::new(|e, _| nv(if n_of(e, "lts") > 0.0 { n_of(e, "lts") } else { 1.0 })))
            .gt0()
            .set(Box::new(|e, v, _| Ok(put(e, "lts", Some(v.clone()), &num(1.0))))),
        Def::new("lw", "general", "lw", Box::new(|e, _| PVal::V(e.get("lw").cloned().unwrap_or(Value::Null))))
            .set(Box::new(|e, v, _| Ok(put(e, "lw", Some(v.clone()), &Value::Null)))),
        Def::new("tr", "general", "tr", Box::new(|e, _| PVal::V(e.get("tr").cloned().unwrap_or(Value::Null))))
            .set(Box::new(|e, v, _| Ok(put(e, "tr", Some(v.clone()), &Value::Null)))),
        Def::new("th", "general", "len", Box::new(|e, _| nv(num0(e.get("th")))))
            .set(Box::new(|e, v, _| Ok(put(e, "th", Some(v.clone()), &num(0.0))))),
        Def::new("mat", "general", "mat", Box::new(|e, _| PVal::V(e.get("mat").cloned().unwrap_or(Value::Null))))
            .set(Box::new(|e, v, _| Ok(put(e, "mat", Some(v.clone()), &Value::Null)))),
        Def::new("ps", "general", "ro", Box::new(|_, c| PVal::V(Value::String(c.lang.s("props.ps.bycolor"))))).title("props.ps.title"),
        Def::new(
            "url",
            "general",
            "text",
            Box::new(|e, _| PVal::V(Value::String(if truthy(e, "url") { s_of(e.get("url")) } else { String::new() }))),
        )
        .set(Box::new(|e, v, _| {
            let s: String = js::trim(&s_of(Some(v))).to_string();
            let s16: Vec<u16> = s.encode_utf16().take(2048).collect();
            Ok(put(e, "url", Some(Value::String(String::from_utf16_lossy(&s16))), &Value::String(String::new())))
        })),
    ];
    if let Some(d) = v.get_mut(6) {
        d.types = Some(TH_TYPES);
    }
    v
}

// ---------------------------------------------------------------------------------------------------------------
// points

fn point(name: &'static str, kx: &'static str, ky: &'static str, kz: Option<&'static str>, lang: &Lang) -> Vec<Def> {
    let axis = |a: &'static str, k: &'static str| {
        let mut d = Def::new(
            &format!("{name}.{a}"),
            "geometry",
            "len",
            Box::new(move |e, _| if a == "z" { nv(num0(e.get(k))) } else { PVal::V(e.get(k).cloned().unwrap_or(Value::Null)) }),
        )
        .set(Box::new(move |e, v, _| Ok(if a == "z" { put(e, k, Some(v.clone()), &num(0.0)) } else { with(e, k, v.clone()) })))
        .label(lang.s(&format!("props.f.{a}")));
        d.pt = Some((name, a));
        d
    };
    let mut out = vec![axis("x", kx), axis("y", ky)];
    if let Some(z) = kz {
        out.push(axis("z", z));
    }
    out
}

/// A point nested in a key (`e.p1.x`), for dimensions; `when` decides.
fn nested_point(name: &'static str, k: &'static str, when: fn(&Obj) -> bool, lang: &Lang) -> Vec<Def> {
    ["x", "y"]
        .into_iter()
        .map(|a| {
            let mut d = Def::new(
                &format!("{name}.{a}"),
                "geometry",
                "len",
                Box::new(move |e, _| PVal::V(e.get(k).and_then(|p| p.get(a)).cloned().unwrap_or(Value::Null))),
            )
            .set(Box::new(move |e, v, _| {
                let mut p = e.get(k).and_then(Value::as_object).cloned().unwrap_or_default();
                p.insert(a.into(), v.clone());
                let mut n = with(e, k, Value::Object(p));
                n.shift_remove("block");
                Ok(n)
            }))
            .when(Box::new(move |e, _| when(e)))
            .label(lang.s(&format!("props.f.{a}")));
            d.pt = Some((name, a));
            d
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------------------------
// measures

/// propdefs.js `measure(e)`: length, area and perimeter where they mean something.
pub fn measure(e: &Entity) -> (Option<f64>, Option<f64>, Option<f64>) {
    let len = curve_length(e);
    let mut area = None;
    let mut per = None;
    let closed_poly = matches!(&e.geo, Geo::Poly(p) if p.is_closed());
    if (matches!(e.typ(), "circle" | "ellipse" | "hatch") || closed_poly)
        && let Some((a, p, _)) = entity_area(e)
    {
        area = Some(a.abs());
        per = Some(p);
    }
    (len, area, per)
}

fn area_of(e: &Obj) -> f64 {
    entity_area(&ent_of(e)).map_or(0.0, |(a, _, _)| a).abs()
}

fn pts_of(e: &Obj) -> Vec<Obj> {
    e.get("pts").and_then(Value::as_array).map(|a| a.iter().filter_map(|p| p.as_object().cloned()).collect()).unwrap_or_default()
}

/// The centroid of a closed polyline's corners.
fn poly_centre(e: &Obj) -> (f64, f64) {
    let p = pts_of(e);
    let n = p.len();
    let (mut a, mut cx, mut cy) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let (Some(u), Some(v)) = (p.get(i), p.get(i.saturating_add(1).checked_rem(n).unwrap_or(0))) else { continue };
        let (ux, uy, vx, vy) = (n_of(u, "x"), n_of(u, "y"), n_of(v, "x"), n_of(v, "y"));
        let c = ux * vy - vx * uy;
        a += c;
        cx += (ux + vx) * c;
        cy += (uy + vy) * c;
    }
    if a.abs() < 1e-12 {
        let nn = f(n);
        return (p.iter().fold(0.0, |s, q| s + n_of(q, "x")) / nn, p.iter().fold(0.0, |s, q| s + n_of(q, "y")) / nn);
    }
    (cx / (3.0 * a), cy / (3.0 * a))
}
fn f(n: usize) -> f64 {
    cc_cad_geom::geom::f(n)
}

fn poly_to_area(e: &Obj, target: f64, lang: &Lang) -> Result<Obj, String> {
    let a0 = area_of(e);
    if !(a0 > 0.0) {
        return Err(perr(lang, "props.err.noarea"));
    }
    let k = (target / a0).sqrt();
    let (cx, cy) = poly_centre(e);
    let pts: Vec<Value> = pts_of(e)
        .into_iter()
        .map(|mut q| {
            let (x, y) = (n_of(&q, "x"), n_of(&q, "y"));
            q.insert("x".into(), num(cx + (x - cx) * k));
            q.insert("y".into(), num(cy + (y - cy) * k));
            for w in ["sw", "ew"] {
                if js::truthy(q.get(w)) {
                    let v = n_of(&q, w) * k;
                    q.insert(w.into(), num(v));
                }
            }
            Value::Object(q)
        })
        .collect();
    Ok(with(e, "pts", Value::Array(pts)))
}

fn param_to_angle(tp: f64, ratio: f64) -> f64 {
    norm_angle((ratio * tp.jsin()).jatan2(tp.jcos()))
}
fn angle_to_param(a: f64, ratio: f64) -> f64 {
    let r = if ratio == 0.0 || ratio.is_nan() { 1.0 } else { ratio };
    norm_angle((a.jsin() / r).jatan2(a.jcos()))
}

// ---------------------------------------------------------------------------------------------------------------
// per type

const JUST: [(&str, &str); 12] = [
    ("left", "baseline"),
    ("center", "baseline"),
    ("right", "baseline"),
    ("left", "top"),
    ("center", "top"),
    ("right", "top"),
    ("left", "middle"),
    ("center", "middle"),
    ("right", "middle"),
    ("left", "bottom"),
    ("center", "bottom"),
    ("right", "bottom"),
];
fn just_key(e: &Obj) -> String {
    let ha = if truthy(e, "ha") { s_of(e.get("ha")) } else { "left".into() };
    let va = if truthy(e, "va") { s_of(e.get("va")) } else { "baseline".into() };
    format!("{ha}|{va}")
}
/// `e.k ?? 1`
fn or1(e: &Obj, k: &str) -> f64 {
    match e.get(k) {
        None | Some(Value::Null) => 1.0,
        Some(v) => val_num(v),
    }
}
fn uniform(e: &Obj) -> bool {
    let x = or1(e, "sx");
    (or1(e, "sy") - x).abs() < 1e-12 && (or1(e, "sz") - x).abs() < 1e-12
}
fn vtx(e: &Obj, c: &PCtx) -> usize {
    let n = pts_of(e).len();
    let last = i64::try_from(n).unwrap_or(1).saturating_sub(1);
    let i = c.vertex.min(last).max(0);
    usize::try_from(i).unwrap_or(0)
}
fn vset(e: &Obj, c: &PCtx, k: &str, v: &Value) -> Obj {
    let i = vtx(e, c);
    let pts: Vec<Value> = pts_of(e)
        .into_iter()
        .enumerate()
        .map(|(j, mut q)| {
            if j == i {
                if val_num(v) == 0.0 && k != "b" {
                    q.shift_remove(k);
                } else {
                    q.insert(k.into(), v.clone());
                }
            }
            Value::Object(q)
        })
        .collect();
    with(e, "pts", Value::Array(pts))
}
fn text_styles(c: &PCtx) -> Vec<(Value, String)> {
    let mut names: Vec<String> = vec!["Standard".into()];
    for k in c.doc.text_styles.keys() {
        if !names.contains(k) {
            names.push(k.clone());
        }
    }
    for e in c.ents {
        if let Some(Value::String(s)) = e.get("style")
            && !s.is_empty()
            && !names.contains(s)
        {
            names.push(s.clone());
        }
    }
    names.into_iter().map(|n| (Value::String(n.clone()), n)).collect()
}
fn mst(e: &Obj) -> Obj {
    let mut m = Obj::new();
    m.insert("asz".into(), num(2.5));
    m.insert("dogleg".into(), num(2.5));
    m.insert("gap".into(), num(1.0));
    if let Some(Value::Object(st)) = e.get("st") {
        for (k, v) in st {
            m.insert(k.clone(), v.clone());
        }
    }
    m
}
fn zero_err(v: &Value, lang: &Lang) -> Result<(), String> {
    let x = val_num(v);
    if x == 0.0 || x.is_nan() { Err(perr(lang, "props.err.zero")) } else { Ok(()) }
}

fn type_defs(key: &str, c: &PCtx) -> Vec<Def> {
    let l = c.lang;
    let ro_len = |k: &str, g: fn(&Obj) -> f64| Def::new(k, "geometry", "ro-len", Box::new(move |e, _| nv(g(e))));
    match key {
        "line" => {
            let mut v = point("start", "x1", "y1", Some("z1"), l);
            v.extend(point("end", "x2", "y2", Some("z2"), l));
            v.push(ro_len("dx", |e| n_of(e, "x2") - n_of(e, "x1")));
            v.push(ro_len("dy", |e| n_of(e, "y2") - n_of(e, "y1")));
            v.push(ro_len("dz", |e| num0(e.get("z2")) - num0(e.get("z1"))));
            v.push(
                Def::new(
                    "len",
                    "geometry",
                    "len",
                    Box::new(|e, _| nv(js::hypot(n_of(e, "x2") - n_of(e, "x1"), n_of(e, "y2") - n_of(e, "y1")))),
                )
                .gt0()
                .set(Box::new(|e, v, _| {
                    let a = (n_of(e, "y2") - n_of(e, "y1")).jatan2(n_of(e, "x2") - n_of(e, "x1"));
                    let vv = val_num(v);
                    let n = with(e, "x2", num(n_of(e, "x1") + a.jcos() * vv));
                    Ok(with(&n, "y2", num(n_of(e, "y1") + a.jsin() * vv)))
                })),
            );
            v.push(
                Def::new(
                    "ang",
                    "geometry",
                    "ang",
                    Box::new(|e, _| nv(deg(norm_angle((n_of(e, "y2") - n_of(e, "y1")).jatan2(n_of(e, "x2") - n_of(e, "x1")))))),
                )
                .set(Box::new(|e, v, _| {
                    let len = js::hypot(n_of(e, "x2") - n_of(e, "x1"), n_of(e, "y2") - n_of(e, "y1"));
                    let a = rad(val_num(v));
                    let n = with(e, "x2", num(n_of(e, "x1") + a.jcos() * len));
                    Ok(with(&n, "y2", num(n_of(e, "y1") + a.jsin() * len)))
                })),
            );
            v
        }
        "circle" => {
            let mut v = point("center", "cx", "cy", Some("z"), l);
            v.push(
                Def::new("r", "geometry", "len", Box::new(|e, _| PVal::V(e.get("r").cloned().unwrap_or(Value::Null))))
                    .gt0()
                    .set(Box::new(|e, v, _| Ok(with(e, "r", v.clone())))),
            );
            v.push(
                Def::new("d", "geometry", "len", Box::new(|e, _| nv(n_of(e, "r") * 2.0)))
                    .gt0()
                    .set(Box::new(|e, v, _| Ok(with(e, "r", num(val_num(v) / 2.0))))),
            );
            v.push(
                Def::new("circ", "geometry", "len", Box::new(|e, _| nv(TAU * n_of(e, "r"))))
                    .gt0()
                    .set(Box::new(|e, v, _| Ok(with(e, "r", num(val_num(v) / TAU))))),
            );
            v.push(
                Def::new("area", "geometry", "area", Box::new(|e, _| nv(PI * n_of(e, "r") * n_of(e, "r"))))
                    .gt0()
                    .set(Box::new(|e, v, _| Ok(with(e, "r", num((val_num(v) / PI).sqrt()))))),
            );
            v
        }
        "arc" => {
            let sweep = |e: &Obj| js::or(Some(norm_angle(n_of(e, "a1") - n_of(e, "a0"))), TAU);
            let mut v = point("center", "cx", "cy", Some("z"), l);
            v.push(
                Def::new("r", "geometry", "len", Box::new(|e, _| PVal::V(e.get("r").cloned().unwrap_or(Value::Null))))
                    .gt0()
                    .set(Box::new(|e, v, _| Ok(with(e, "r", v.clone())))),
            );
            v.push(
                Def::new("a0", "geometry", "ang", Box::new(|e, _| nv(deg(norm_angle(n_of(e, "a0"))))))
                    .set(Box::new(|e, v, _| Ok(with(e, "a0", num(norm_angle(rad(val_num(v)))))))),
            );
            v.push(
                Def::new("a1", "geometry", "ang", Box::new(|e, _| nv(deg(norm_angle(n_of(e, "a1"))))))
                    .set(Box::new(|e, v, _| Ok(with(e, "a1", num(norm_angle(rad(val_num(v)))))))),
            );
            v.push(Def::new("sweep", "geometry", "ang", Box::new(move |e, _| nv(deg(sweep(e))))).gt0().max(360.0).set(Box::new(
                |e, v, c| {
                    if val_num(v) >= 360.0 - 1e-9 {
                        return Err(perr(c.lang, "props.err.fullarc"));
                    }
                    Ok(with(e, "a1", num(norm_angle(n_of(e, "a0") + rad(val_num(v))))))
                },
            )));
            v.push(Def::new("arclen", "geometry", "ro-len", Box::new(move |e, _| nv(n_of(e, "r") * sweep(e)))));
            v.push(Def::new("chord", "geometry", "len", Box::new(move |e, _| nv(2.0 * n_of(e, "r") * (sweep(e) / 2.0).jsin()))).gt0().set(
                Box::new(move |e, v, c| {
                    let s = (sweep(e) / 2.0).jsin();
                    if !(s.abs() > 1e-9) {
                        return Err(perr(c.lang, "props.err.nochord"));
                    }
                    Ok(with(e, "r", num((val_num(v) / (2.0 * s)).abs())))
                }),
            ));
            v.push(Def::new(
                "area",
                "geometry",
                "ro-area",
                Box::new(move |e, _| {
                    let th = sweep(e);
                    nv(n_of(e, "r") * n_of(e, "r") * (th - th.jsin()) / 2.0)
                }),
            ));
            v
        }
        "ellipse" => {
            let t0 = |e: &Obj| match e.get("t0") {
                None | Some(Value::Null) => 0.0,
                Some(v) => val_num(v),
            };
            let t1 = |e: &Obj| match e.get("t1") {
                None | Some(Value::Null) => TAU,
                Some(v) => val_num(v),
            };
            let mut v = point("center", "cx", "cy", Some("z"), l);
            v.push(Def::new("major", "geometry", "len", Box::new(|e, _| nv(js::hypot(n_of(e, "mx"), n_of(e, "my"))))).gt0().set(Box::new(
                |e, v, _| {
                    let a = js::hypot(n_of(e, "mx"), n_of(e, "my"));
                    let a = if a == 0.0 || a.is_nan() { 1.0 } else { a };
                    let n = with(e, "mx", num(n_of(e, "mx") / a * val_num(v)));
                    Ok(with(&n, "my", num(n_of(e, "my") / a * val_num(v))))
                },
            )));
            v.push(
                Def::new("minor", "geometry", "len", Box::new(|e, _| nv(js::hypot(n_of(e, "mx"), n_of(e, "my")) * n_of(e, "ratio"))))
                    .gt0()
                    .set(Box::new(|e, v, c| {
                        let a = js::hypot(n_of(e, "mx"), n_of(e, "my"));
                        if val_num(v) > a + 1e-12 {
                            return Err(perr(c.lang, "props.err.minor"));
                        }
                        Ok(with(e, "ratio", num(val_num(v) / a)))
                    })),
            );
            v.push(
                Def::new("ratio", "geometry", "num", Box::new(|e, _| PVal::V(e.get("ratio").cloned().unwrap_or(Value::Null))))
                    .gt0()
                    .max(1.0)
                    .set(Box::new(|e, v, _| Ok(with(e, "ratio", v.clone())))),
            );
            v.push(Def::new("axisang", "geometry", "ang", Box::new(|e, _| nv(deg(norm_angle(n_of(e, "my").jatan2(n_of(e, "mx"))))))).set(
                Box::new(|e, v, _| {
                    let a = js::hypot(n_of(e, "mx"), n_of(e, "my"));
                    let r = rad(val_num(v));
                    let n = with(e, "mx", num(r.jcos() * a));
                    Ok(with(&n, "my", num(r.jsin() * a)))
                }),
            ));
            v.push(
                Def::new("ea0", "geometry", "ang", Box::new(move |e, _| nv(deg(param_to_angle(t0(e), n_of(e, "ratio"))))))
                    .set(Box::new(|e, v, _| Ok(with(e, "t0", num(angle_to_param(rad(val_num(v)), n_of(e, "ratio"))))))),
            );
            v.push(
                Def::new(
                    "ea1",
                    "geometry",
                    "ang",
                    Box::new(move |e, _| {
                        let d = deg(param_to_angle(t1(e), n_of(e, "ratio")));
                        nv(if d != 0.0 && !d.is_nan() {
                            d
                        } else if t1(e) - t0(e) >= TAU - 1e-9 {
                            360.0
                        } else {
                            0.0
                        })
                    }),
                )
                .set(Box::new(move |e, v, _| {
                    let tp = angle_to_param(rad(val_num(v)), n_of(e, "ratio"));
                    Ok(with(e, "t1", num(if tp <= t0(e) + 1e-12 { tp + TAU } else { tp })))
                })),
            );
            v.push(Def::new("area", "geometry", "ro-area", Box::new(|e, _| nv(area_of(e)))));
            v
        }
        "point" => point("pos", "x", "y", Some("z"), l),
        "poly" => {
            let mut v = Vec::new();
            let mut vd = Def::new("vertex", "geometry", "vertex", Box::new(|e, c| nv(f(vtx(e, c)) + 1.0)));
            vd.one = true;
            v.push(vd);
            for a in ["x", "y"] {
                let mut d = Def::new(
                    &format!("vertex.{a}"),
                    "geometry",
                    "len",
                    Box::new(move |e, c| PVal::V(pts_of(e).get(vtx(e, c)).and_then(|q| q.get(a)).cloned().unwrap_or(Value::Null))),
                )
                .set(Box::new(move |e, v, c| {
                    let i = vtx(e, c);
                    let pts: Vec<Value> = pts_of(e)
                        .into_iter()
                        .enumerate()
                        .map(|(j, mut q)| {
                            if j == i {
                                q.insert(a.into(), v.clone());
                            }
                            Value::Object(q)
                        })
                        .collect();
                    Ok(with(e, "pts", Value::Array(pts)))
                }))
                .label(l.s(&format!("props.f.{a}")));
                d.one = true;
                d.pt = Some(("vertex", a));
                v.push(d);
            }
            for (k, w) in [("vsw", "sw"), ("vew", "ew")] {
                let mut d = Def::new(
                    k,
                    "geometry",
                    "len",
                    Box::new(move |e, c| {
                        let q = pts_of(e).get(vtx(e, c)).cloned().unwrap_or_default();
                        let x = match q.get(w) {
                            None | Some(Value::Null) => e.get("w").cloned(),
                            o => o.cloned(),
                        };
                        nv(num0(x.as_ref()))
                    }),
                )
                .min(0.0)
                .set(Box::new(move |e, v, c| Ok(vset(e, c, w, v))));
                d.one = true;
                v.push(d);
            }
            let mut vb = Def::new("vb", "geometry", "num", Box::new(|e, c| nv(num0(pts_of(e).get(vtx(e, c)).and_then(|q| q.get("b"))))))
                .title("props.f.vb.title")
                .set(Box::new(|e, v, c| Ok(vset(e, c, "b", v))));
            vb.one = true;
            v.push(vb);
            v.push(Def::new("gw", "geometry", "len", Box::new(|e, _| nv(num0(e.get("w"))))).min(0.0).set(Box::new(|e, v, _| {
                let mut n = put(e, "w", Some(v.clone()), &num(0.0));
                let pts: Vec<Value> = pts_of(e)
                    .into_iter()
                    .map(|mut q| {
                        q.shift_remove("sw");
                        q.shift_remove("ew");
                        Value::Object(q)
                    })
                    .collect();
                n.insert("pts".into(), Value::Array(pts));
                Ok(n)
            })));
            v.push(
                Def::new("z", "geometry", "len", Box::new(|e, _| nv(num0(e.get("z")))))
                    .label(l.s("props.f.elev"))
                    .set(Box::new(|e, v, _| Ok(put(e, "z", Some(v.clone()), &num(0.0))))),
            );
            v.push(
                Def::new("closed", "geometry", "bool", Box::new(|e, _| PVal::V(Value::Bool(truthy(e, "closed")))))
                    .set(Box::new(|e, v, _| Ok(with(e, "closed", Value::Bool(js::truthy(Some(v))))))),
            );
            v.push(Def::new("count", "geometry", "ro", Box::new(|e, _| PVal::V(Value::String(pts_of(e).len().to_string())))));
            v.push(Def::new("plen", "geometry", "ro-len", Box::new(|e, _| curve_length(&ent_of(e)).map_or(PVal::V(Value::Null), nv))));
            v.push(
                Def::new("area", "geometry", "area", Box::new(|e, _| nv(area_of(e))))
                    .gt0()
                    .when(Box::new(|e, _| truthy(e, "closed")))
                    .set(Box::new(|e, v, c| poly_to_area(e, val_num(v), c.lang))),
            );
            v
        }
        "text" => {
            let mut v = vec![
                Def::new("s", "text", "textarea", Box::new(|e, _| PVal::V(Value::String(s_of(e.get("s")))))).set(Box::new(|e, v, c| {
                    let s = s_of(Some(v));
                    if js::trim(&s).is_empty() {
                        return Err(perr(c.lang, "props.err.notext"));
                    }
                    Ok(with(e, "s", Value::String(s)))
                })),
                Def::new(
                    "style",
                    "text",
                    "choice",
                    Box::new(|e, _| PVal::V(Value::String(if truthy(e, "style") { s_of(e.get("style")) } else { "Standard".into() }))),
                )
                .choices(Box::new(text_styles))
                .set(Box::new(|e, v, _| Ok(put(e, "style", Some(v.clone()), &Value::String("Standard".into()))))),
                Def::new("just", "text", "choice", Box::new(|e, _| PVal::V(Value::String(just_key(e)))))
                    .choices(Box::new(|c| {
                        JUST.iter().map(|(h, v)| (Value::String(format!("{h}|{v}")), c.lang.s(&format!("props.just.{h}.{v}")))).collect()
                    }))
                    .set(Box::new(|e, v, _| {
                        let s = s_of(Some(v));
                        let mut it = s.splitn(2, '|');
                        let ha = it.next().unwrap_or("").to_string();
                        let va = it.next();
                        let n = with(e, "ha", Value::String(ha));
                        Ok(match va {
                            Some(va) => with(&n, "va", Value::String(va.into())),
                            None => {
                                let mut n = n;
                                n.shift_remove("va");
                                n
                            }
                        })
                    })),
                Def::new("h", "text", "len", Box::new(|e, _| PVal::V(e.get("h").cloned().unwrap_or(Value::Null))))
                    .gt0()
                    .set(Box::new(|e, v, _| Ok(with(e, "h", v.clone())))),
                Def::new("rot", "text", "ang", Box::new(|e, _| nv(deg(norm_angle(js::or(Some(n_of(e, "rot")), 0.0))))))
                    .set(Box::new(|e, v, _| Ok(put(e, "rot", Some(num(norm_angle(rad(val_num(v))))), &num(0.0))))),
                Def::new("wf", "text", "num", Box::new(|e, _| nv(if n_of(e, "wf") > 0.0 { n_of(e, "wf") } else { 1.0 })))
                    .gt0()
                    .max(100.0)
                    .set(Box::new(|e, v, _| Ok(put(e, "wf", Some(v.clone()), &num(1.0))))),
                Def::new("obl", "text", "ang", Box::new(|e, _| nv(deg(js::or(Some(n_of(e, "obl")), 0.0)))))
                    .min(-85.0)
                    .max(85.0)
                    .set(Box::new(|e, v, _| Ok(put(e, "obl", Some(num(rad(val_num(v)))), &num(0.0))))),
                Def::new("ww", "text", "len", Box::new(|e, _| nv(num0(e.get("ww")))))
                    .min(0.0)
                    .title("props.ww.title")
                    .set(Box::new(|e, v, _| Ok(put(e, "ww", Some(v.clone()), &num(0.0))))),
                Def::new("ls", "text", "num", Box::new(|e, _| nv(js::or(Some(n_of(e, "ls")), 1.0))))
                    .min(0.25)
                    .max(4.0)
                    .set(Box::new(|e, v, _| Ok(put(e, "ls", Some(v.clone()), &num(1.0))))),
                Def::new("ann", "text", "bool", Box::new(|e, _| PVal::V(Value::Bool(truthy(e, "ann"))))).title("props.f.ann.title").set(
                    Box::new(|e, v, _| Ok(put(e, "ann", if js::truthy(Some(v)) { Some(Value::Bool(true)) } else { None }, &num(0.0)))),
                ),
            ];
            v.extend(point("pos", "x", "y", Some("z"), l));
            v
        }
        "insert" => {
            let mut v = vec![Def::new(
                "block",
                "geometry",
                "ro",
                Box::new(|e, c| {
                    let dimsuffix = if truthy(e, "dim") { format!(" {}", c.lang.s("props.block.dim")) } else { String::new() };
                    PVal::V(Value::String(format!("{}{dimsuffix}", s_of(e.get("block")))))
                }),
            )];
            v.extend(point("ins", "x", "y", Some("z"), l));
            v.push(
                Def::new("rot", "geometry", "ang", Box::new(|e, _| nv(deg(norm_angle(js::or(Some(n_of(e, "rot")), 0.0))))))
                    .set(Box::new(|e, v, _| Ok(put(e, "rot", Some(num(norm_angle(rad(val_num(v))))), &num(0.0))))),
            );
            for k in ["sx", "sy"] {
                v.push(Def::new(k, "geometry", "num", Box::new(move |e, _| nv(or1(e, k)))).set(Box::new(move |e, v, c| {
                    zero_err(v, c.lang)?;
                    Ok(with(e, k, v.clone()))
                })));
            }
            v.push(Def::new("sz", "geometry", "num", Box::new(|e, _| nv(or1(e, "sz")))).set(Box::new(|e, v, c| {
                zero_err(v, c.lang)?;
                Ok(put(e, "sz", Some(v.clone()), &num(1.0)))
            })));
            v.push(
                Def::new("us", "geometry", "num", Box::new(|e, _| if uniform(e) { nv(or1(e, "sx")) } else { PVal::Varies }))
                    .title("props.f.us.title")
                    .set(Box::new(|e, v, c| {
                        zero_err(v, c.lang)?;
                        let n = with(&with(e, "sx", v.clone()), "sy", v.clone());
                        Ok(put(&n, "sz", Some(v.clone()), &num(1.0)))
                    })),
            );
            v.push(Def::new(
                "nents",
                "geometry",
                "ro",
                Box::new(|e, c| {
                    let n = c.doc.blocks.get(&s_of(e.get("block"))).map_or(0, |b| b.entities.len());
                    PVal::V(Value::String(n.to_string()))
                }),
            ));
            v.push(
                Def::new("dyn", "misc", "ro", Box::new(|_, c| PVal::V(Value::String(c.lang.s("props.dyn.none")))))
                    .title("props.dyn.title")
                    .when(Box::new(|e, c| !is_dynamic(c.doc, e))),
            );
            v.extend(dynamic_defs(c));
            v
        }
        "hatch" => {
            let known = |e: &Obj| pattern_names().contains(&s_of(e.get("pattern")).to_uppercase().as_str());
            let has_patlines = |e: &Obj| e.get("patlines").and_then(Value::as_array).is_some_and(|a| !a.is_empty());
            let mut v = vec![
                Def::new(
                    "pattern",
                    "pattern",
                    "choice",
                    Box::new(|e, _| {
                        PVal::V(Value::String(if truthy(e, "solid") {
                            "SOLID".into()
                        } else if truthy(e, "pattern") {
                            s_of(e.get("pattern")).to_uppercase()
                        } else {
                            "ANSI31".into()
                        }))
                    }),
                )
                .when(Box::new(move |e, _| !has_patlines(e) && (truthy(e, "solid") || !truthy(e, "pattern") || known(e))))
                .choices(Box::new(|_| {
                    let mut v = vec![(Value::String("SOLID".into()), "SOLID".to_string())];
                    v.extend(pattern_names().into_iter().map(|k| (Value::String(k.into()), k.to_string())));
                    v
                }))
                .set(Box::new(|e, v, _| {
                    let s = s_of(Some(v));
                    let mut n = with(e, "solid", Value::Bool(s == "SOLID"));
                    if s == "SOLID" {
                        n.shift_remove("pattern");
                    } else {
                        n.insert("pattern".into(), v.clone());
                        if matches!(n.get("pscale"), None | Some(Value::Null)) {
                            n.insert("pscale".into(), num(1.0));
                        }
                        if matches!(n.get("pangle"), None | Some(Value::Null)) {
                            n.insert("pangle".into(), num(0.0));
                        }
                    }
                    Ok(n)
                })),
                Def::new(
                    "patname",
                    "pattern",
                    "ro",
                    Box::new(|e, c| {
                        PVal::V(Value::String(if truthy(e, "pattern") { s_of(e.get("pattern")) } else { c.lang.s("props.hatch.custom") }))
                    }),
                )
                .when(Box::new(move |e, _| has_patlines(e) || (!truthy(e, "solid") && truthy(e, "pattern") && !known(e)))),
                Def::new("pscale", "pattern", "num", Box::new(|e, _| nv(if n_of(e, "pscale") > 0.0 { n_of(e, "pscale") } else { 1.0 })))
                    .gt0()
                    .when(Box::new(|e, _| !truthy(e, "solid")))
                    .set(Box::new(|e, v, _| Ok(with(e, "pscale", v.clone())))),
                Def::new("pangle", "pattern", "ang", Box::new(|e, _| nv(deg(js::or(Some(n_of(e, "pangle")), 0.0)))))
                    .when(Box::new(|e, _| !truthy(e, "solid")))
                    .set(Box::new(|e, v, _| Ok(with(e, "pangle", num(norm_angle(rad(val_num(v)))))))),
            ];
            for a in ["x", "y"] {
                let k: &'static str = if a == "x" { "ox" } else { "oy" };
                let mut d = Def::new(&format!("origin.{a}"), "pattern", "len", Box::new(move |e, _| nv(num0(e.get(k)))))
                    .when(Box::new(|e, _| !truthy(e, "solid")))
                    .set(Box::new(move |e, v, _| Ok(put(e, k, Some(v.clone()), &num(0.0)))))
                    .label(l.s(&format!("props.f.{a}")));
                d.pt = Some(("origin", a));
                v.push(d);
            }
            v.push(
                Def::new("assoc", "pattern", "bool", Box::new(|e, _| PVal::V(Value::Bool(truthy(e, "assoc")))))
                    .title("props.f.assoc.title")
                    .set(Box::new(|e, v, _| {
                        Ok(put(e, "assoc", if js::truthy(Some(v)) { Some(Value::Bool(true)) } else { None }, &num(0.0)))
                    })),
            );
            v.push(
                Def::new(
                    "island",
                    "pattern",
                    "choice",
                    Box::new(|e, _| PVal::V(Value::String(if truthy(e, "island") { s_of(e.get("island")) } else { "normal".into() }))),
                )
                .choices(Box::new(|c| {
                    ["normal", "outer", "ignore"]
                        .iter()
                        .map(|k| (Value::String((*k).into()), c.lang.s(&format!("props.island.{k}"))))
                        .collect()
                }))
                .set(Box::new(|e, v, _| Ok(put(e, "island", Some(v.clone()), &Value::String("normal".into()))))),
            );
            let mut bg = Def::new(
                "bg",
                "pattern",
                "color",
                Box::new(|e, _| PVal::V(e.get("bg").cloned().unwrap_or_else(|| Value::String("none".into())))),
            )
            // `put(e, 'bg', …)` with put's default 0: "by block" (0) drops the key too (propdefs.js)
            .set(Box::new(|e, v, _| Ok(put(e, "bg", if v.as_str() == Some("none") { None } else { Some(v.clone()) }, &num(0.0)))));
            bg.none = true;
            v.push(bg);
            v.push(
                Def::new("z", "geometry", "len", Box::new(|e, _| nv(num0(e.get("z")))))
                    .label(l.s("props.f.elev"))
                    .set(Box::new(|e, v, _| Ok(put(e, "z", Some(v.clone()), &num(0.0))))),
            );
            v.push(Def::new(
                "loops",
                "geometry",
                "ro",
                Box::new(|e, _| PVal::V(Value::String(e.get("loops").and_then(Value::as_array).map_or(0, Vec::len).to_string()))),
            ));
            v.push(Def::new("area", "geometry", "ro-area", Box::new(|e, _| nv(area_of(e)))));
            v
        }
        "mleader" => {
            let mut v = vec![
                Def::new("s", "text", "textarea", Box::new(|e, _| PVal::V(Value::String(s_of(e.get("s"))))))
                    .set(Box::new(|e, v, _| Ok(with(e, "s", Value::String(s_of(Some(v))))))),
                Def::new(
                    "h",
                    "text",
                    "len",
                    Box::new(|e, _| {
                        let h = n_of(e, "h");
                        if h != 0.0 && !h.is_nan() { nv(h) } else { PVal::V(mst(e).get("asz").cloned().unwrap_or(Value::Null)) }
                    }),
                )
                .gt0()
                .set(Box::new(|e, v, _| Ok(with(e, "h", v.clone())))),
                Def::new("ww", "text", "len", Box::new(|e, _| nv(num0(e.get("ww")))))
                    .min(0.0)
                    .title("props.ww.title")
                    .set(Box::new(|e, v, _| Ok(put(e, "ww", Some(v.clone()), &num(0.0))))),
                Def::new(
                    "dir",
                    "text",
                    "choice",
                    Box::new(|e, _| PVal::V(Value::String(if n_of(e, "dir") == -1.0 { "-1".into() } else { "1".into() }))),
                )
                .choices(Box::new(|c| {
                    vec![(Value::String("1".into()), c.lang.s("props.dir.right")), (Value::String("-1".into()), c.lang.s("props.dir.left"))]
                }))
                .set(Box::new(|e, v, _| Ok(with(e, "dir", num(if val_num(v) == -1.0 { -1.0 } else { 1.0 }))))),
            ];
            for (k, sk) in [("masz", "asz"), ("dogleg", "dogleg")] {
                v.push(
                    Def::new(k, "lines", "len", Box::new(move |e, _| PVal::V(mst(e).get(sk).cloned().unwrap_or(Value::Null))))
                        .min(0.0)
                        .set(Box::new(move |e, v, _| {
                            let mut st = mst(e);
                            st.insert(sk.into(), v.clone());
                            Ok(with(e, "st", Value::Object(st)))
                        })),
                );
            }
            for a in ["x", "y"] {
                let mut d = Def::new(
                    &format!("arrow.{a}"),
                    "geometry",
                    "len",
                    Box::new(move |e, _| {
                        PVal::V(
                            e.get("pts")
                                .and_then(Value::as_array)
                                .and_then(|p| p.first())
                                .and_then(|q| q.get(a))
                                .cloned()
                                .unwrap_or(Value::Null),
                        )
                    }),
                )
                .when(Box::new(|e, _| e.get("pts").and_then(Value::as_array).is_some_and(|p| !p.is_empty())))
                .set(Box::new(move |e, v, _| {
                    let pts: Vec<Value> = e
                        .get("pts")
                        .and_then(Value::as_array)
                        .map(|p| {
                            p.iter()
                                .enumerate()
                                .map(|(i, q)| {
                                    let mut q = q.as_object().cloned().unwrap_or_default();
                                    if i == 0 {
                                        q.insert(a.into(), v.clone());
                                    }
                                    Value::Object(q)
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    Ok(with(e, "pts", Value::Array(pts)))
                }))
                .label(l.s(&format!("props.f.{a}")));
                d.pt = Some(("arrow", a));
                v.push(d);
            }
            v.push(
                Def::new("z", "geometry", "len", Box::new(|e, _| nv(num0(e.get("z")))))
                    .label(l.s("props.f.elev"))
                    .set(Box::new(|e, v, _| Ok(put(e, "z", Some(v.clone()), &num(0.0))))),
            );
            v
        }
        "viewport" => {
            let mut v = Vec::new();
            for a in ["x", "y"] {
                let mut d = Def::new(
                    &format!("vcenter.{a}"),
                    "geometry",
                    "len",
                    Box::new(move |e, _| PVal::V(e.get(a).cloned().unwrap_or(Value::Null))),
                )
                .set(Box::new(move |e, v, _| Ok(with(e, a, v.clone()))))
                .label(l.s(&format!("props.f.{a}")));
                d.paper = true;
                d.pt = Some(("vcenter", a));
                v.push(d);
            }
            for (k, ek) in [("vw", "w"), ("vh", "h")] {
                let mut d = Def::new(k, "geometry", "len", Box::new(move |e, _| PVal::V(e.get(ek).cloned().unwrap_or(Value::Null))))
                    .gt0()
                    .set(Box::new(move |e, v, _| Ok(with(e, ek, v.clone()))));
                d.paper = true;
                v.push(d);
            }
            for (a, k) in [("x", "cx"), ("y", "cy")] {
                let mut d =
                    Def::new(&format!("vview.{a}"), "misc", "len", Box::new(move |e, _| PVal::V(e.get(k).cloned().unwrap_or(Value::Null))))
                        .set(Box::new(move |e, v, c| {
                            if truthy(e, "locked") {
                                return Err(perr(c.lang, "props.err.vplocked"));
                            }
                            Ok(with(e, k, v.clone()))
                        }))
                        .label(l.s(&format!("props.f.{a}")));
                d.model = true;
                v.push(d);
            }
            v.push(Def::new("on", "misc", "bool", Box::new(|e, _| PVal::V(Value::Bool(e.get("on") != Some(&Value::Bool(false)))))).set(
                Box::new(|e, v, _| Ok(put(e, "on", if js::truthy(Some(v)) { None } else { Some(Value::Bool(false)) }, &Value::Null))),
            ));
            v.push(
                Def::new("locked", "misc", "bool", Box::new(|e, _| PVal::V(Value::Bool(truthy(e, "locked")))))
                    .title("props.f.locked.title")
                    .set(Box::new(|e, v, _| {
                        Ok(put(e, "locked", if js::truthy(Some(v)) { Some(Value::Bool(true)) } else { None }, &Value::Null))
                    })),
            );
            v.push(
                Def::new(
                    "vname",
                    "misc",
                    "text",
                    Box::new(|e, _| PVal::V(Value::String(if truthy(e, "vname") { s_of(e.get("vname")) } else { String::new() }))),
                )
                .title("props.f.vname.title")
                .set(Box::new(|e, v, _| {
                    let s = js::trim(&s_of(Some(v))).to_string();
                    let s16: Vec<u16> = s.encode_utf16().take(255).collect();
                    let s = String::from_utf16_lossy(&s16);
                    Ok(put(e, "vname", if s.is_empty() { None } else { Some(Value::String(s)) }, &Value::Null))
                })),
            );
            v
        }
        "dim" => dim_defs(c),
        _ => Vec::new(),
    }
}

fn is_dynamic(doc: &Doc, e: &Obj) -> bool {
    let Some(b) = doc.blocks.get(&s_of(e.get("block"))) else { return false };
    let Some(d) = b.dyn_.as_ref().filter(|d| js::truthy(Some(d))) else { return false };
    if js::truthy(d.get("def")) {
        return true;
    }
    d.get("of").map(|of| s_of(Some(of))).and_then(|of| doc.blocks.get(&of)).is_some()
}

/// `dynDef(doc, e)`: the dynamic block definition a reference shows (its own block, or the one its copy is of).
fn dyn_def_name(doc: &Doc, e: &Obj) -> Option<String> {
    let b = doc.blocks.get(&s_of(e.get("block")))?;
    let d = b.dyn_.as_ref().filter(|d| js::truthy(Some(d)))?;
    if js::truthy(d.get("def")) {
        return Some(b.name.clone());
    }
    let of = s_of(d.get("of"));
    doc.blocks.get(&of).map(|x| x.name.clone())
}

/// dyn-blocks.js `visibilityOf(doc, ref)`: (label, states, current state).
pub fn visibility_of(doc: &Doc, e: &Obj) -> Option<(String, Vec<String>, Option<String>)> {
    if e.get("type").and_then(Value::as_str) != Some("insert") {
        return None;
    }
    let def = doc.blocks.get(&dyn_def_name(doc, e)?)?;
    let vis = def.dyn_.as_ref()?.get("vis").filter(|v| js::truthy(Some(v)))?;
    let states: Vec<String> = vis.get("states").and_then(Value::as_array)?.iter().map(|s| s_of(s.get("name"))).collect();
    if states.is_empty() {
        return None;
    }
    let label = [vis.get("label"), vis.get("name")].into_iter().find(|v| js::truthy(*v)).map(|v| s_of(v)).unwrap_or_default();
    let current = e.get("dyn").and_then(|d| d.get("state")).filter(|v| !v.is_null()).map(|v| s_of(Some(v)));
    Some((label, states, current))
}

/// propdefs.js `dynamicDefs(ctx)`: Properties › Custom for a dynamic block reference — the visibility state as a list,
/// then the parameter values the file holds, read-only.
fn dynamic_defs(c: &PCtx) -> Vec<Def> {
    let mut out = Vec::new();
    let Some(e0) = c.ents.first().map(Entity::to_obj) else { return out };
    let Some(def0) = dyn_def_name(c.doc, &e0) else { return out };
    let same = {
        let d0 = def0.clone();
        move |e: &Obj, c: &PCtx| dyn_def_name(c.doc, e).as_deref() == Some(d0.as_str())
    };
    if let Some((label, states, _)) = visibility_of(c.doc, &e0) {
        let lab = if label.is_empty() { c.lang.s("props.f.dyn.vis") } else { label };
        let same2 = same.clone();
        out.push(
            Def::new(
                "dyn.vis",
                "custom",
                "choice",
                Box::new(|e, c| PVal::V(Value::String(visibility_of(c.doc, e).and_then(|v| v.2).unwrap_or_default()))),
            )
            .label(lab)
            .title("props.f.dyn.vis.title")
            .when(Box::new(move |e, c| same2(e, c) && visibility_of(c.doc, e).is_some()))
            .choices(Box::new(move |_| states.iter().map(|n| (Value::String(n.clone()), n.clone())).collect()))
            // the change itself is apply_value's (dyn-blocks.js setVisibility inside the PROPERTIES step)
            .set(Box::new(|e, _, _| Ok(e.clone()))),
        );
    }
    let params: Vec<Value> = c
        .doc
        .blocks
        .get(&def0)
        .and_then(|b| b.dyn_.as_ref())
        .and_then(|d| d.get("params"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let values: Vec<Value> = e0.get("dyn").and_then(|d| d.get("values")).and_then(Value::as_array).cloned().unwrap_or_default();
    for v in values {
        let kind = s_of(v.get("kind"));
        if kind == "visibility" {
            continue;
        }
        let node = v.get("node").cloned().unwrap_or(Value::Null);
        // `new Map(params.map((p) => [p.node, p]))`: the last parameter of a node wins
        let p = params.iter().rev().find(|p| p.get("node").unwrap_or(&Value::Null) == &node);
        let name = p
            .and_then(|p| p.get("name"))
            .filter(|n| js::truthy(Some(n)))
            .map_or_else(|| c.lang.s(&format!("props.dyn.kind.{kind}")), |n| s_of(Some(n)));
        let key = format!("dyn.{}", s_of(Some(&node)));
        let pick = {
            let node = node.clone();
            move |e: &Obj| -> Option<Value> {
                e.get("dyn")
                    .and_then(|d| d.get("values"))
                    .and_then(Value::as_array)?
                    .iter()
                    .find(|x| x.get("node").unwrap_or(&Value::Null) == &node)
                    .cloned()
            }
        };
        let fin = |k: &str| v.get(k).and_then(Value::as_f64).is_some_and(f64::is_finite);
        let base = |d: Def| -> Def {
            let s = same.clone();
            d.title("props.dyn.ro.title").when(Box::new(move |e, c| s(e, c)))
        };
        if kind == "linear" && fin("distance") {
            let pk = pick.clone();
            out.push(base(
                Def::new(
                    &key,
                    "custom",
                    "ro-len",
                    Box::new(move |e, _| {
                        pk(e).and_then(|x| x.get("distance").filter(|d| !d.is_null()).cloned()).map_or(PVal::Varies, PVal::V)
                    }),
                )
                .label(name),
            ));
        } else if kind == "alignment" && fin("angle") {
            let pk = pick.clone();
            out.push(base(
                Def::new(
                    &key,
                    "custom",
                    "ro",
                    Box::new(move |e, _| match pk(e) {
                        Some(x) => {
                            let a = deg(norm_angle(js::to_number_opt(x.get("angle"))));
                            PVal::V(Value::String(format!("{}°", js::num_to_string(js::to_fixed(a, 4).parse::<f64>().unwrap_or(f64::NAN)))))
                        }
                        None => PVal::Varies,
                    }),
                )
                .label(name),
            ));
        } else if kind == "flip" && v.get("flipped").is_some() {
            let pk = pick.clone();
            out.push(base(
                Def::new(
                    &key,
                    "custom",
                    "ro",
                    Box::new(move |e, c| match pk(e) {
                        Some(x) => PVal::V(Value::String(c.lang.s(if js::truthy(x.get("flipped")) {
                            "props.dyn.flipped"
                        } else {
                            "props.dyn.not_flipped"
                        }))),
                        None => PVal::Varies,
                    }),
                )
                .label(name),
            ));
        } else if (kind == "point" || kind == "basepoint") && js::truthy(v.get("point")) {
            for a in ["x", "y"] {
                let pk = pick.clone();
                let lab = format!("{name} {}", c.lang.s(&format!("props.f.{a}")));
                out.push(base(
                    Def::new(
                        &format!("{key}.{a}"),
                        "custom",
                        "ro-len",
                        Box::new(move |e, _| {
                            pk(e)
                                .and_then(|x| x.get("point").and_then(|p| p.get(a)).filter(|d| !d.is_null()).cloned())
                                .map_or(PVal::Varies, PVal::V)
                        }),
                    )
                    .label(lab),
                ));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------
// dimensions

fn dst(e: &Obj) -> Obj {
    let mut m: Obj = cc_cad::doc::dim_defaults();
    for (k, v) in dim_extra() {
        m.insert(k, v);
    }
    if let Some(Value::Object(st)) = e.get("st") {
        for (k, v) in st {
            m.insert(k.clone(), v.clone());
        }
    }
    m
}
fn set_st(e: &Obj, k: &str, v: Value) -> Obj {
    let mut st = e.get("st").and_then(Value::as_object).cloned().unwrap_or_default();
    st.insert(k.into(), v);
    let mut n = with(e, "st", Value::Object(st));
    n.shift_remove("block");
    n
}
fn has_kind(e: &Obj) -> bool {
    truthy(e, "kind")
}
fn not_ang(e: &Obj) -> bool {
    s_of(e.get("kind")) != "angular"
}
fn is_ang(e: &Obj) -> bool {
    s_of(e.get("kind")) == "angular"
}
fn tol_in(e: &Obj, list: &[&str]) -> bool {
    list.contains(&s_of(dst(e).get("tol")).as_str())
}

fn dim_defs(c: &PCtx) -> Vec<Def> {
    let l = c.lang;
    // S(key, sec, kind, k, conv_number)
    let s = |key: &'static str, sec: &'static str, kind: &'static str, k: &'static str, conv: bool, when: fn(&Obj) -> bool| -> Def {
        Def::new(&format!("dim.{key}"), sec, kind, Box::new(move |e, _| PVal::V(dst(e).get(k).cloned().unwrap_or(Value::Null))))
            .set(Box::new(move |e, v, _| Ok(set_st(e, k, if conv { num(val_num(v)) } else { v.clone() }))))
            .when(Box::new(move |e, _| when(e)))
            .label(l.s(&format!("props.dim.{key}")))
    };
    let arrow_choices = || -> Choices {
        Box::new(|c: &PCtx| {
            ARROWS
                .iter()
                .map(|k| {
                    (
                        Value::String((*k).into()),
                        c.lang.s(&format!("props.arrow.{}", if k.is_empty() { "closedfilled".to_string() } else { k.to_lowercase() })),
                    )
                })
                .collect()
        })
    };
    let ints = |list: &'static [i64], pre: &'static str| -> Choices {
        Box::new(move |c: &PCtx| list.iter().map(|k| (num(*k as f64), c.lang.s(&format!("{pre}{k}")))).collect())
    };
    let mut v: Vec<Def> = Vec::new();
    v.push(
        Def::new(
            "dimstyle",
            "general",
            "choice",
            Box::new(|e, _| PVal::V(Value::String(if truthy(e, "dimstyle") { s_of(e.get("dimstyle")) } else { "Standard".into() }))),
        )
        .when(Box::new(|e, _| has_kind(e)))
        .choices(Box::new(|c| dim_style_names(c).into_iter().map(|n| (Value::String(n.clone()), n)).collect()))
        .set(Box::new(|e, v, c| {
            let name = s_of(Some(v));
            let st = c.doc.dim_styles.get(&name).cloned();
            let mut n =
                with(e, "st", Value::Object(st.unwrap_or_else(|| e.get("st").and_then(Value::as_object).cloned().unwrap_or_default())));
            if name == "Standard" {
                n.shift_remove("dimstyle");
            } else {
                n.insert("dimstyle".into(), Value::String(name));
            }
            n.shift_remove("block");
            Ok(n)
        })),
    );
    v.push(Def::new(
        "dimkind",
        "geometry",
        "ro",
        Box::new(|e, c| {
            PVal::V(Value::String(if has_kind(e) {
                c.lang.s(&format!("dimkind.{}", s_of(e.get("kind"))))
            } else {
                c.lang.t("props.dim.fromblock", &[("block", s_of(e.get("block")))])
            }))
        }),
    ));
    v.push(
        Def::new(
            "measured",
            "geometry",
            "ro",
            Box::new(|e, _| {
                let mut x = e.clone();
                x.insert("text".into(), Value::String(String::new()));
                let ent = ent_of(&x);
                PVal::V(Value::String(dim_style_text(&ent, dim_measure(&ent_of(e)))))
            }),
        )
        .when(Box::new(|e, _| has_kind(e))),
    );
    v.extend(nested_point("dimp1", "p1", |e| has_kind(e) && js::truthy(e.get("p1")), l));
    v.extend(nested_point(
        "dimp2",
        "p2",
        |e| has_kind(e) && js::truthy(e.get("p2")) && matches!(s_of(e.get("kind")).as_str(), "linear" | "aligned" | "angular"),
        l,
    ));
    v.extend(nested_point(
        "dimc",
        "c",
        |e| has_kind(e) && js::truthy(e.get("c")) && matches!(s_of(e.get("kind")).as_str(), "angular" | "radius" | "diameter"),
        l,
    ));
    v.extend(nested_point("dimloc", "loc", |e| has_kind(e) && js::truthy(e.get("loc")), l));
    // Lines & arrows
    v.push(s("blk1", "lines", "choice", "blk1", false, has_kind).choices(arrow_choices()));
    v.push(s("blk2", "lines", "choice", "blk2", false, has_kind).choices(arrow_choices()));
    v.push(s("asz", "lines", "len", "asz", false, has_kind).min(0.0));
    v.push(s("tsz", "lines", "len", "tsz", false, has_kind).min(0.0).title("props.dim.tsz.title"));
    v.push(s("clrd", "lines", "color", "clrd", false, has_kind));
    v.push(s("lwd", "lines", "lw", "lwd", false, has_kind));
    v.push(s("sd1", "lines", "bool", "sd1", false, has_kind));
    v.push(s("sd2", "lines", "bool", "sd2", false, has_kind));
    v.push(s("clre", "lines", "color", "clre", false, |e| has_kind(e) && not_ang(e)));
    v.push(s("lwe", "lines", "lw", "lwe", false, |e| has_kind(e) && not_ang(e)));
    v.push(s("se1", "lines", "bool", "se1", false, has_kind));
    v.push(s("se2", "lines", "bool", "se2", false, has_kind));
    v.push(s("exo", "lines", "len", "exo", false, has_kind).min(0.0));
    v.push(s("exe", "lines", "len", "exe", false, has_kind).min(0.0));
    // Text
    v.push(
        Def::new(
            "dim.text",
            "text",
            "text",
            Box::new(|e, _| PVal::V(Value::String(if truthy(e, "text") { s_of(e.get("text")) } else { String::new() }))),
        )
        .label(l.s("props.dim.text"))
        .title("props.dim.text.title")
        .when(Box::new(|e, _| has_kind(e)))
        .set(Box::new(|e, v, _| {
            let s = s_of(Some(v));
            let mut n = with(e, "text", Value::String(if js::trim(&s) == "<>" { String::new() } else { s }));
            n.shift_remove("block");
            Ok(n)
        })),
    );
    v.push(s("txsty", "text", "choice", "txsty", false, has_kind).choices(Box::new(text_styles)));
    v.push(s("clrt", "text", "color", "clrt", false, has_kind));
    v.push(s("txt", "text", "len", "txt", false, has_kind).gt0());
    v.push(s("gap", "text", "len", "gap", false, has_kind).min(0.0));
    v.push(s("tad", "text", "choice", "tad", true, has_kind).choices(ints(&[0, 1, 2, 4], "props.tad.")));
    v.push(s("just", "text", "choice", "just", true, |e| has_kind(e) && not_ang(e)).choices(ints(&[0, 1, 2], "props.djust.")));
    v.push(s("tih", "text", "bool", "tih", false, has_kind));
    v.push(
        Def::new("dim.trot", "text", "ang", Box::new(|e, _| nv(deg(js::or(Some(n_of(e, "trot")), 0.0)))))
            .label(l.s("props.dim.trot"))
            .when(Box::new(|e, _| has_kind(e)))
            .set(Box::new(|e, v, _| {
                let x = val_num(v);
                let mut n = put(e, "trot", if x != 0.0 && !x.is_nan() { Some(num(norm_angle(rad(x)))) } else { None }, &num(0.0));
                n.shift_remove("block");
                Ok(n)
            })),
    );
    v.push(s("tfill", "text", "choice", "tfill", true, has_kind).choices(ints(&[0, 1, 2], "props.tfill.")));
    v.push(s("tfillclr", "text", "color", "tfillclr", false, |e| has_kind(e) && js::int32(js::to_number_opt(dst(e).get("tfill"))) == 2.0));
    // Fit
    v.push(s("scale", "fit", "num", "scale", false, has_kind).gt0().title("props.dim.scale.title"));
    v.push(s("dli", "fit", "len", "dli", false, has_kind).min(0.0));
    v.push(s("atfit", "fit", "choice", "atfit", true, has_kind).choices(ints(&[0, 1, 2, 3], "props.atfit.")));
    v.push(s("tmove", "fit", "choice", "tmove", true, has_kind).choices(ints(&[0, 1, 2], "props.tmove.")));
    v.push(s("tix", "fit", "bool", "tix", false, has_kind));
    v.push(s("tofl", "fit", "bool", "tofl", false, has_kind));
    // Primary units
    let mut lunit =
        s("lunit", "units", "choice", "lunit", true, |e| has_kind(e) && not_ang(e)).choices(ints(&[1, 2, 3, 4, 5], "props.lunit."));
    lunit.get = Box::new(|e, _| {
        let x = dst(e).get("lunit").cloned();
        PVal::V(match x {
            Some(v) if js::truthy(Some(&v)) => v,
            _ => num(2.0),
        })
    });
    v.push(lunit);
    v.push(s("dec", "units", "int", "dec", false, |e| has_kind(e) && not_ang(e)).min(0.0).max(8.0));
    v.push(s("rnd", "units", "len", "rnd", false, |e| has_kind(e) && not_ang(e)).min(0.0));
    v.push(s("pre", "units", "text", "pre", false, has_kind));
    v.push(s("suf", "units", "text", "suf", false, has_kind));
    v.push(s("lfac", "units", "num", "lfac", false, |e| has_kind(e) && not_ang(e)).gt0().title("props.dim.lfac.title"));
    v.push(s("dsep", "units", "choice", "dsep", false, has_kind).choices(Box::new(|c| {
        vec![(Value::String(".".into()), c.lang.s("props.dsep.point")), (Value::String(",".into()), c.lang.s("props.dsep.comma"))]
    })));
    for which in ["lead", "trail"] {
        let bit = move |e: &Obj| -> i64 {
            match (which, is_ang(e)) {
                ("lead", true) => 1,
                ("lead", false) => 4,
                (_, true) => 2,
                (_, false) => 8,
            }
        };
        v.push(
            Def::new(
                &format!("dim.z{which}"),
                "units",
                "bool",
                Box::new(move |e, _| {
                    let k = if is_ang(e) { "azin" } else { "zin" };
                    let cur = js::int32(js::to_number_opt(dst(e).get(k)));
                    #[allow(clippy::cast_possible_truncation)]
                    let ci = cur as i64;
                    PVal::V(Value::Bool(ci & bit(e) != 0))
                }),
            )
            .label(l.s(&format!("props.dim.z{which}")))
            .when(Box::new(|e, _| has_kind(e)))
            .set(Box::new(move |e, v, _| {
                let k = if is_ang(e) { "azin" } else { "zin" };
                let cur = js::int32(js::to_number_opt(dst(e).get(k)));
                #[allow(clippy::cast_possible_truncation)]
                let ci = cur as i64;
                let nv_ = if js::truthy(Some(v)) { ci | bit(e) } else { ci & !bit(e) };
                #[allow(clippy::cast_precision_loss)]
                Ok(set_st(e, k, num(nv_ as f64)))
            })),
        );
    }
    v.push(s("aunit", "units", "choice", "aunit", true, is_ang).choices(ints(&[0, 1, 2, 3], "props.aunit.")));
    v.push(s("adec", "units", "int", "adec", false, is_ang).min(0.0).max(8.0));
    // Alternate units
    let alt_on = |e: &Obj| has_kind(e) && not_ang(e) && js::truthy(dst(e).get("alt"));
    v.push(s("alt", "alt", "bool", "alt", false, |e| has_kind(e) && not_ang(e)));
    v.push(s("altu", "alt", "choice", "altu", true, alt_on).choices(ints(&[1, 2, 3, 4, 5], "props.lunit.")));
    v.push(s("altd", "alt", "int", "altd", false, alt_on).min(0.0).max(8.0));
    v.push(s("altf", "alt", "num", "altf", false, alt_on).gt0().title("props.dim.altf.title"));
    v.push(s("altrnd", "alt", "num", "altrnd", false, alt_on).min(0.0));
    v.push(s("apre", "alt", "text", "apre", false, alt_on));
    v.push(s("asuf", "alt", "text", "asuf", false, alt_on));
    // Tolerances
    v.push(
        s("tol", "tol", "choice", "tol", false, has_kind)
            .choices(Box::new(|c| TOL_MODES.iter().map(|k| (Value::String((*k).into()), c.lang.s(&format!("props.tol.{k}")))).collect())),
    );
    v.push(s("tp", "tol", "len", "tp", false, |e| has_kind(e) && not_ang(e) && tol_in(e, &["sym", "dev", "lim"])));
    v.push(s("tp", "tol", "ang", "tp", false, |e| is_ang(e) && tol_in(e, &["sym", "dev", "lim"])));
    v.push(s("tm", "tol", "len", "tm", false, |e| has_kind(e) && not_ang(e) && tol_in(e, &["dev", "lim"])));
    v.push(s("tm", "tol", "ang", "tm", false, |e| is_ang(e) && tol_in(e, &["dev", "lim"])));
    v.push(s("tdec", "tol", "int", "tdec", false, |e| has_kind(e) && !tol_in(e, &["none", "basic"])).min(0.0).max(8.0));
    v.push(s("tfac", "tol", "num", "tfac", false, |e| has_kind(e) && tol_in(e, &["dev", "lim"])).gt0().max(2.0));
    v.push(
        s("tolj", "tol", "choice", "tolj", true, |e| has_kind(e) && tol_in(e, &["dev", "lim"])).choices(ints(&[0, 1, 2], "props.tolj.")),
    );
    v
}

/// The named dimension styles a drawing has, plus Standard and the names objects carry.
pub fn dim_style_names(c: &PCtx) -> Vec<String> {
    let mut names = vec!["Standard".to_string()];
    for k in c.doc.dim_styles.keys() {
        if !names.contains(k) {
            names.push(k.clone());
        }
    }
    for e in c.ents {
        if let Some(Value::String(s)) = e.get("dimstyle")
            && !s.is_empty()
            && !names.contains(s)
        {
            names.push(s.clone());
        }
    }
    names
}

// ---------------------------------------------------------------------------------------------------------------
// groups, values, changes

/// A group of the selection: every object (`all`, when there is more than one type) or one type.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub key: String,
    pub ids: Vec<u64>,
    pub label: String,
}

/// propdefs.js `groupsOf(ents)`.
pub fn groups_of(ents: &[Entity], lang: &Lang) -> Vec<Group> {
    let mut by: Vec<(String, Vec<u64>)> = Vec::new();
    for e in ents {
        let t = e.typ().to_string();
        let id = e.id.unwrap_or(0);
        match by.iter_mut().find(|(k, _)| *k == t) {
            Some((_, v)) => v.push(id),
            None => by.push((t, vec![id])),
        }
    }
    let mut groups: Vec<Group> = by
        .into_iter()
        .map(|(key, ids)| {
            let label = cc_cad::type_name(&Entity::new(cc_cad::Geo::Other).with_type(&key), &|k| lang.s(k));
            Group { key, ids, label }
        })
        .collect();
    if groups.len() > 1 {
        groups.insert(
            0,
            Group { key: "all".into(), ids: ents.iter().map(|e| e.id.unwrap_or(0)).collect(), label: lang.s("props.group.all") },
        );
    }
    groups
}

trait WithType {
    fn with_type(self, t: &str) -> Entity;
}
impl WithType for Entity {
    fn with_type(mut self, t: &str) -> Entity {
        self.extra.insert("type".into(), Value::String(t.into()));
        self
    }
}

/// `groupLabel(g)`: "Line (8)", "12 objects".
pub fn group_label(g: &Group, lang: &Lang) -> String {
    if g.key == "all" {
        lang.t("props.group.all.n", &[("n", g.ids.len().to_string())])
    } else {
        lang.t("props.group.type.n", &[("type", g.label.clone()), ("n", g.ids.len().to_string())])
    }
}

/// propdefs.js `defsFor(key, ents, ctx)`: the descriptors a group shows, in section order.
pub fn defs_for(key: &str, c: &PCtx) -> Vec<Def> {
    let types: Vec<&str> = {
        let mut v: Vec<&str> = Vec::new();
        for e in c.ents {
            if !v.contains(&e.typ()) {
                v.push(e.typ());
            }
        }
        v
    };
    let mut list: Vec<Def> = general().into_iter().filter(|d| d.types.is_none_or(|ts| types.iter().all(|t| ts.contains(t)))).collect();
    if key != "all" && types.len() == 1 {
        list.extend(type_defs(key, c));
    }
    let objs: Vec<Obj> = c.ents.iter().map(Entity::to_obj).collect();
    list.retain(|d| (!d.one || c.ents.len() == 1) && d.when.as_ref().is_none_or(|w| objs.iter().all(|e| w(e, c))));
    for d in &mut list {
        if d.label.is_none() {
            d.label = Some(c.lang.s(&format!("props.f.{}", d.key)));
        }
    }
    let order = |s: &str| SECTIONS.iter().position(|x| *x == s).unwrap_or(SECTIONS.len().saturating_add(1));
    let mut idx: Vec<(usize, Def)> = list.into_iter().enumerate().collect();
    idx.sort_by(|a, b| order(a.1.sec).cmp(&order(b.1.sec)).then(a.0.cmp(&b.0)));
    idx.into_iter().map(|(_, d)| d).collect()
}

/// `near(a, b)`.
fn near(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap_or(f64::NAN), y.as_f64().unwrap_or(f64::NAN));
            (x - y).abs() <= 1e-9 * js::max(1.0, js::max(x.abs(), y.abs()))
        }
        (Value::String(x), Value::String(y)) => x == y || (x.starts_with('#') && x.to_lowercase() == y.to_lowercase()),
        _ => a == b,
    }
}

/// propdefs.js `valueOf(def, ents, ctx)`.
pub fn value_of(d: &Def, c: &PCtx) -> PVal {
    let mut first: Option<Value> = None;
    for e in c.ents {
        let v = (d.get)(&e.to_obj(), c);
        let PVal::V(v) = v else { return PVal::Varies };
        match &first {
            None => first = Some(v),
            Some(f) if !near(f, &v) => return PVal::Varies,
            _ => {}
        }
    }
    first.map_or(PVal::V(Value::Null), PVal::V)
}

/// What a change did: `{ ok, changed, locked, error? }`.
#[derive(Clone, Debug, PartialEq)]
pub struct Applied {
    pub ok: bool,
    pub changed: usize,
    pub locked: usize,
    pub error: Option<String>,
}

/// propdefs.js `applyValue(doc, def, ents, v, ctx)`: one undo step; locked objects counted; a refusal changes nothing.
pub fn apply_value(doc: &mut Doc, key: &str, group: &str, ids: &[u64], v: &Value, vertex: i64, lang: &Lang) -> Applied {
    let ents: Vec<Entity> = ids.iter().filter_map(|id| doc.get(*id).cloned()).collect();
    let defs = {
        let c = PCtx { doc, ents: &ents, vertex, lang };
        defs_for(group, &c).into_iter().find(|d| d.key == key)
    };
    let Some(d) = defs else { return Applied { ok: false, changed: 0, locked: 0, error: Some(lang.s("props.err.readonly")) } };
    let Some(set) = d.set.as_ref() else { return Applied { ok: false, changed: 0, locked: 0, error: Some(lang.s("props.err.readonly")) } };
    // work out every new object first (a refusal leaves the drawing as it was), then one change
    let mut plan: Vec<(u64, Entity)> = Vec::new();
    let mut locked = 0usize;
    {
        let c = PCtx { doc, ents: &ents, vertex, lang };
        for e in &ents {
            let id = e.id.unwrap_or(0);
            let Some(cur) = doc.get(id) else { continue };
            if !doc.editable(cur) {
                locked = locked.saturating_add(1);
                continue;
            }
            let o = cur.to_obj();
            match set(&o, v, &c) {
                Err(msg) => return Applied { ok: false, changed: 0, locked, error: Some(msg) },
                // a new object every time (the JS counts it even when nothing in it changed)
                Ok(next) => plan.push((id, ent_of(&next))),
            }
        }
    }
    if key == "dyn.vis" {
        // dyn-blocks.js setVisibility per reference, inside the one PROPERTIES step (a refusal rolls it all back)
        let want = match v {
            Value::String(s) => s.clone(),
            other => js::to_js_string(other),
        };
        let ids: Vec<u64> = plan.iter().map(|(id, _)| *id).collect();
        let mut err: Option<String> = None;
        let mut changed = 0usize;
        let _ = doc.change("PROPERTIES", |d| {
            for id in ids {
                let Some(cur) = d.get(id).map(Entity::to_obj) else { continue };
                if visibility_of(d, &cur).and_then(|x| x.2).as_deref() == Some(want.as_str()) {
                    continue;
                }
                let space = d.space;
                match crate::cmd::visedit::set_visibility_checked(d, id, &want, space)? {
                    Ok(()) => changed = changed.saturating_add(1),
                    Err(code) => {
                        err = Some(lang.s(&format!("props.dyn.err.{code}")));
                        return Err(cc_cad::DocError::Failed(String::new()));
                    }
                }
            }
            Ok(())
        });
        return match err {
            Some(e) => Applied { ok: false, changed: 0, locked, error: Some(e) },
            None => Applied { ok: true, changed, locked, error: None },
        };
    }
    let changed = plan.len();
    let _ = doc.change("PROPERTIES", |d| {
        for (id, e) in plan {
            d.update(id, e)?;
        }
        Ok(())
    });
    Applied { ok: true, changed, locked, error: None }
}

// ---------------------------------------------------------------------------------------------------------------
// propexpr.js: what is typed into a number field

/// What `readValue` needs: the drawing's length format and units, the paper unit on a sheet, the limits.
#[derive(Clone, Debug, Default)]
pub struct ReadCtx {
    pub lunits: Option<f64>,
    pub units: String,
    pub paper: Option<String>,
    pub gt0: bool,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

/// Millimetres in one drawing unit (or the sheet's paper unit), or None.
pub fn mm_per_unit(c: &ReadCtx) -> Option<f64> {
    if let Some(p) = &c.paper {
        return Some(if p == "in" { 25.4 } else { 1.0 });
    }
    let u = c.units.to_lowercase();
    let v = match u.as_str() {
        "millimeters" => Some(1.0),
        "centimeters" => Some(10.0),
        "meters" => Some(1000.0),
        "inches" => Some(25.4),
        "feet" => Some(304.8),
        "decimeters" => Some(100.0),
        "kilometers" => Some(1e6),
        "miles" => Some(1_609_344.0),
        "yards" => Some(914.4),
        "microns" => Some(0.001),
        _ => None,
    };
    if v.is_some() {
        return v;
    }
    is_imperial_format(lunit_code(c.lunits.unwrap_or(2.0))).then_some(25.4)
}

/// `editText(v)`: full precision for editing (12 significant digits, no "-0").
pub fn edit_text(v: f64) -> String {
    if !v.is_finite() {
        return String::new();
    }
    let r = js::str_to_number(&cc_part::js::to_precision(v, 12));
    if r == 0.0 {
        return "0".into();
    }
    if r.abs() >= 1e-6 && r.abs() < 1e15 {
        let d = 11.0 - r.abs().log10().floor();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let digits = d.clamp(0.0, 100.0) as usize;
        js::num_to_string(js::str_to_number(&js::to_fixed(r, digits)))
    } else {
        js::num_to_string(r)
    }
}

fn finish(v: f64, kind: &str, c: &ReadCtx, lang: &Lang) -> Result<f64, String> {
    if !v.is_finite() {
        return Err(lang.s("props.err.number"));
    }
    let mut v = v;
    if kind == "int" {
        if (v - js::round(v)).abs() > 1e-9 {
            return Err(lang.s("props.err.int"));
        }
        v = js::round(v);
    }
    if c.gt0 && !(v > 0.0) {
        return Err(lang.s("props.err.gt0"));
    }
    if let Some(m) = c.min
        && v < m - 1e-12
    {
        return Err(lang.t("props.err.min", &[("min", edit_text(m))]));
    }
    if let Some(m) = c.max
        && v > m + 1e-12
    {
        return Err(lang.t("props.err.max", &[("max", edit_text(m))]));
    }
    Ok(v)
}

/// `NUM` = `(?:\d+(?:\.\d*)?|\.\d+)` at byte `i` → its end.
fn num_at(b: &[u8], i: usize) -> Option<usize> {
    let mut j = i;
    let digit = |k: usize| b.get(k).is_some_and(u8::is_ascii_digit);
    if digit(j) {
        while digit(j) {
            j = j.saturating_add(1);
        }
        if b.get(j) == Some(&b'.') {
            j = j.saturating_add(1);
            while digit(j) {
                j = j.saturating_add(1);
            }
        }
        Some(j)
    } else if b.get(j) == Some(&b'.') && digit(j.saturating_add(1)) {
        j = j.saturating_add(1);
        while digit(j) {
            j = j.saturating_add(1);
        }
        Some(j)
    } else {
        None
    }
}
fn is_plain(s: &str) -> bool {
    let b = s.as_bytes();
    let i = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    num_at(b, i) == Some(b.len())
}
fn is_exp_plain(s: &str) -> bool {
    // /^[-+]?\d+(\.\d+)?e[-+]?\d+$/i
    let b = s.as_bytes();
    let mut i = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    let digits = |i: &mut usize| {
        let s0 = *i;
        while b.get(*i).is_some_and(u8::is_ascii_digit) {
            *i = (*i).saturating_add(1);
        }
        *i > s0
    };
    if !digits(&mut i) {
        return false;
    }
    if b.get(i) == Some(&b'.') {
        i = i.saturating_add(1);
        if !digits(&mut i) {
            return false;
        }
    }
    if !matches!(b.get(i), Some(b'e' | b'E')) {
        return false;
    }
    i = i.saturating_add(1);
    if matches!(b.get(i), Some(b'+' | b'-')) {
        i = i.saturating_add(1);
    }
    digits(&mut i) && i == b.len()
}
fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while b.get(i).is_some_and(|c| c.is_ascii_whitespace()) {
        i = i.saturating_add(1);
    }
    i
}
fn sub(s: &str, a: usize, b: usize) -> f64 {
    s.get(a..b).map_or(f64::NAN, js::str_to_number)
}

/// The degrees/minutes/seconds form `NUM\s*[d°]\s*(?:NUM\s*')?\s*(?:NUM\s*")?` at byte i → (value, end).
fn dms_at(s: &str, i: usize) -> Option<(f64, usize)> {
    let b = s.as_bytes();
    let e1 = num_at(b, i)?;
    let mut j = skip_ws(b, e1);
    let d = sub(s, i, e1);
    if b.get(j) == Some(&b'd') {
        j = j.saturating_add(1);
    } else if s.get(j..).is_some_and(|r| r.starts_with('°')) {
        j = j.saturating_add('°'.len_utf8());
    } else {
        return None;
    }
    j = skip_ws(b, j);
    let mut v = d;
    if let Some(e2) = num_at(b, j) {
        let k = skip_ws(b, e2);
        if b.get(k) == Some(&b'\'') {
            v += sub(s, j, e2) / 60.0;
            j = k.saturating_add(1);
        }
    }
    j = skip_ws(b, j);
    if let Some(e3) = num_at(b, j) {
        let k = skip_ws(b, e3);
        if b.get(k) == Some(&b'"') {
            v += sub(s, j, e3) / 3600.0;
            j = k.saturating_add(1);
        }
    }
    Some((v, j))
}

/// Replace feet-inch and unit-suffixed literals by drawing units → (text, a unit was used).
fn lengths_to_units(src: &str, mm_u: Option<f64>) -> (String, bool) {
    let mut used = false;
    let put = |mm: f64, used: &mut bool| -> String {
        *used = true;
        match mm_u {
            Some(u) => format!("({})", edit_text(mm / u)),
            None => "(0)".into(),
        }
    };
    // pass 1: FT_IN — NUM\s*'\s*-?\s*(?:(\d+)(?:[\s-]+(\d+)\/(\d+))?|(\d+)\/(\d+)|(NUM))?\s*"?
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    let mut last = 0usize;
    while i < b.len() {
        let Some(e1) = num_at(b, i).filter(|_| i == 0 || !b.get(i.wrapping_sub(1)).is_some_and(|c| c.is_ascii_digit() || *c == b'.'))
        else {
            i = i.saturating_add(1);
            continue;
        };
        let mut j = skip_ws(b, e1);
        if b.get(j) != Some(&b'\'') {
            i = e1;
            continue;
        }
        let ft = sub(src, i, e1);
        j = skip_ws(b, j.saturating_add(1));
        if b.get(j) == Some(&b'-') {
            j = skip_ws(b, j.saturating_add(1));
        }
        let mut inch = 0.0;
        let mut bad = false;
        // (\d+)(?:[\s-]+(\d+)\/(\d+))? | (\d+)\/(\d+) | NUM
        let int_end = |k: usize| {
            let mut m = k;
            while b.get(m).is_some_and(u8::is_ascii_digit) {
                m = m.saturating_add(1);
            }
            m
        };
        let w_end = int_end(j);
        if w_end > j {
            // a whole number, then maybe a fraction
            let mut k = w_end;
            let mut matched_frac = false;
            let mut sep = k;
            while b.get(sep).is_some_and(|c| c.is_ascii_whitespace() || *c == b'-') {
                sep = sep.saturating_add(1);
            }
            if sep > k {
                let n_end = int_end(sep);
                if n_end > sep && b.get(n_end) == Some(&b'/') {
                    let d_end = int_end(n_end.saturating_add(1));
                    if d_end > n_end.saturating_add(1) {
                        let fd = sub(src, n_end.saturating_add(1), d_end);
                        if fd == 0.0 {
                            bad = true;
                        }
                        inch = sub(src, j, w_end) + sub(src, sep, n_end) / fd;
                        k = d_end;
                        matched_frac = true;
                    }
                }
            }
            if !matched_frac {
                // (\d+)\/(\d+) or a decimal NUM — the regex tries the whole-number branch first; with no fraction
                // after it the whole number stands (a decimal point after it is left to the NUM branch)
                if b.get(w_end) == Some(&b'/') {
                    let d_end = int_end(w_end.saturating_add(1));
                    if d_end > w_end.saturating_add(1) {
                        // the first alternative already matched the whole number alone: JS keeps "(\d+)" (wi) and stops
                    }
                }
                inch = sub(src, j, w_end);
                k = w_end;
            }
            j = k;
        } else if let Some(e) = num_at(b, j) {
            inch = sub(src, j, e);
            j = e;
        }
        j = skip_ws(b, j);
        if b.get(j) == Some(&b'"') {
            j = j.saturating_add(1);
        }
        if bad {
            i = j;
            continue;
        }
        out.push_str(src.get(last..i).unwrap_or(""));
        out.push_str(&put(ft * 304.8 + inch * 25.4, &mut used));
        last = j;
        i = j;
    }
    out.push_str(src.get(last..).unwrap_or(""));
    // pass 2: WITH_UNIT — NUM\s*(mm|cm|m|in|ft|"|')(?![A-Za-z0-9_])
    let s1 = out;
    let b = s1.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    let mut last = 0usize;
    while i < b.len() {
        let Some(e1) = num_at(b, i).filter(|_| i == 0 || !b.get(i.wrapping_sub(1)).is_some_and(|c| c.is_ascii_digit() || *c == b'.'))
        else {
            i = i.saturating_add(1);
            continue;
        };
        let j = skip_ws(b, e1);
        let rest = s1.get(j..).unwrap_or("");
        let unit = ["mm", "cm", "m", "in", "ft", "\"", "'"]
            .into_iter()
            .find(|u| rest.starts_with(u) && !rest.as_bytes().get(u.len()).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_'));
        let Some(u) = unit else {
            i = e1;
            continue;
        };
        let mm = match u {
            "mm" => 1.0,
            "cm" => 10.0,
            "m" => 1000.0,
            "in" | "\"" => 25.4,
            _ => 304.8,
        };
        out.push_str(s1.get(last..i).unwrap_or(""));
        out.push_str(&put(sub(&s1, i, e1) * mm, &mut used));
        last = j.saturating_add(u.len());
        i = last;
    }
    out.push_str(s1.get(last..).unwrap_or(""));
    (out, used)
}

fn dms_to_degrees(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    let mut last = 0usize;
    while i < b.len() {
        if i > 0 && b.get(i.wrapping_sub(1)).is_some_and(|c| c.is_ascii_digit() || *c == b'.') {
            i = i.saturating_add(1);
            continue;
        }
        match dms_at(src, i) {
            Some((v, end)) => {
                out.push_str(src.get(last..i).unwrap_or(""));
                out.push_str(&format!("({})", edit_text(v)));
                last = end;
                i = end;
            }
            None => i = i.saturating_add(1),
        }
    }
    out.push_str(src.get(last..).unwrap_or(""));
    out
}

fn expression(src: &str, kind: &str, c: &ReadCtx, lang: &Lang) -> Result<f64, String> {
    let mut s = src.to_string();
    if kind == "len" {
        let (r, used) = lengths_to_units(&s, mm_per_unit(c));
        if used && mm_per_unit(c).is_none() {
            return Err(lang.s("props.err.nounits"));
        }
        s = r;
    } else if kind == "ang" {
        s = dms_to_degrees(&s);
    }
    struct NoVars;
    impl cc_part::expr::Values for NoVars {
        fn get(&self, _name: &str) -> Option<f64> {
            None
        }
    }
    match cc_part::expr::compute(&s, &NoVars) {
        Ok((v, _)) => finish(v, kind, c, lang),
        Err(e) => Err(lang.t("props.err.expr", &[("why", e.message)])),
    }
}

/// propexpr.js `readValue(raw, kind, ctx)`.
pub fn read_value(raw: &str, kind: &str, c: &ReadCtx, lang: &Lang) -> Result<f64, String> {
    let s = js::trim(raw);
    if s.is_empty() {
        return Err(lang.s("props.err.empty"));
    }
    if s.encode_utf16().count() > 500 {
        return Err(lang.s("props.err.long"));
    }
    if let Some(body) = s.strip_prefix('=') {
        let body = js::trim(body);
        return if body.is_empty() { Err(lang.s("props.err.empty")) } else { expression(body, kind, c, lang) };
    }
    if kind == "len" {
        let lu = if c.paper.is_some() { 2.0 } else { c.lunits.unwrap_or(2.0) };
        if let Some(v) = parse_length(s, lu) {
            return finish(v, kind, c, lang);
        }
    } else if kind == "ang" {
        if is_plain(s) {
            return finish(crate::input::parse_float(s), kind, c, lang);
        }
        // ^([-+]?)(NUM)\s*(r|rad|g|grad)$ (i)
        let b = s.as_bytes();
        let neg = b.first() == Some(&b'-');
        let i0 = usize::from(matches!(b.first(), Some(b'+' | b'-')));
        if let Some(e1) = num_at(b, i0) {
            let j = skip_ws(b, e1);
            let u = s.get(j..).unwrap_or("").to_lowercase();
            if matches!(u.as_str(), "r" | "rad" | "g" | "grad") {
                let v = sub(s, i0, e1) * if u.starts_with('r') { 180.0 / PI } else { 0.9 };
                return finish(if neg { -v } else { v }, kind, c, lang);
            }
        }
        // ^([-+]?)\s*DMS$
        let i1 = skip_ws(b, i0);
        if let Some((v, end)) = dms_at(s, i1)
            && end == b.len()
        {
            return finish(if neg { -v } else { v }, kind, c, lang);
        }
    } else if is_plain(s) || is_exp_plain(s) {
        return finish(crate::input::parse_float(s), kind, c, lang);
    }
    expression(s, kind, c, lang)
}

/// The text a value shows as, for the harness and plain read-outs (`String(v)` for strings / bools, numbers as JS).
pub fn value_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Number(n) => n.as_f64().map(js::num_to_string).unwrap_or_default(),
        v => v.to_string(),
    }
}

#[allow(dead_code)]
fn type_of(e: &Obj) -> &str {
    typ(e)
}

#[allow(dead_code)]
fn map_of(v: &Value) -> Option<&Map<String, Value>> {
    v.as_object()
}
