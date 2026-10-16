//! Entities: typed Rust forms of doc.js's plain objects, with the same JSON.
//!
//! Each type keeps its geometry fields typed ([`Geo`]); `id`, `layer`, `color`, `lt` and `lw` are typed on
//! [`Entity`]; every other key an object carries (add-on fields, `lts`, `tr`, `frz`, `ccref`, `tag`, `role`, …) rides
//! along untouched in `extra`, so `{ ...e, x1 }` in the JS keeps them exactly as here. JSON → [`Entity::from_value`],
//! [`Entity::to_value`] → JSON: absent keys stay absent, `null` stays `null`.
//!
//! An object of a known type whose geometry fields are missing or not numbers is kept as [`Geo::Other`] (all its keys
//! in `extra`): the geometry treats it like an unknown type (no primitives) where the JS would compute with NaN.

use cc_geom::P2;
use serde_json::{Map, Number, Value};

use crate::js;

pub type Obj = Map<String, Value>;

/// A number as JSON: integral values as integers, NaN / ±Infinity as "$NaN" / "$Inf" / "$-Inf" (the parity kit's
/// canonical spelling; a JS `JSON.stringify` would write null).
pub fn num(v: f64) -> Value {
    if v.is_nan() {
        return Value::String("$NaN".into());
    }
    if v.is_infinite() {
        return Value::String(if v > 0.0 { "$Inf" } else { "$-Inf" }.into());
    }
    if v.fract() == 0.0 && v.abs() < 9_007_199_254_740_992.0 {
        // integral and inside ±2^53: exact as an i64
        #[allow(clippy::cast_possible_truncation)]
        let i = v as i64;
        return Value::from(i);
    }
    Number::from_f64(v).map_or(Value::Null, Value::Number)
}
/// A point as JSON `{x, y}`.
pub fn pt(p: P2) -> Value {
    let mut m = Obj::new();
    m.insert("x".into(), num(p.x));
    m.insert("y".into(), num(p.y));
    Value::Object(m)
}
/// `{x, y}` from JSON (both numbers), else None.
pub fn pt_of(v: Option<&Value>) -> Option<P2> {
    let o = v?.as_object()?;
    Some(P2::new(o.get("x")?.as_f64()?, o.get("y")?.as_f64()?))
}

/// An entity's colour: `null` (by layer), a number (0 by block, 1–255 ACI, 256 by layer from files) or a string
/// ('#rrggbb', or a word such as 'bg').
#[derive(Clone, Debug, PartialEq)]
pub enum Color {
    Null,
    Num(f64),
    Str(String),
}
impl Color {
    pub fn from_value(v: &Value) -> Option<Color> {
        match v {
            Value::Null => Some(Color::Null),
            Value::Number(n) => n.as_f64().map(Color::Num),
            Value::String(s) => Some(Color::Str(s.clone())),
            _ => None,
        }
    }
    pub fn to_value(&self) -> Value {
        match self {
            Color::Null => Value::Null,
            Color::Num(n) => num(*n),
            Color::Str(s) => Value::String(s.clone()),
        }
    }
    /// `c ?? null` for an optional colour.
    pub fn or_null(c: &Option<Color>) -> Color {
        c.clone().unwrap_or(Color::Null)
    }
}

/// A polyline vertex `{x, y, b?}` plus any other keys (widths `sw` / `ew`, …).
#[derive(Clone, Debug, PartialEq)]
pub struct Vertex {
    pub x: f64,
    pub y: f64,
    pub b: Option<f64>,
    pub extra: Obj,
}
impl Vertex {
    pub fn new(x: f64, y: f64, b: f64) -> Self {
        Self { x, y, b: Some(b), extra: Obj::new() }
    }
    pub fn p(&self) -> P2 {
        P2::new(self.x, self.y)
    }
    /// `p.b || 0`
    pub fn bulge(&self) -> f64 {
        js::or(self.b, 0.0)
    }
    fn from_value(v: &Value) -> Option<Vertex> {
        let mut m = v.as_object()?.clone();
        let x = take_f(&mut m, "x")?;
        let y = take_f(&mut m, "y")?;
        let b = take_f(&mut m, "b");
        Some(Vertex { x, y, b, extra: m })
    }
    pub fn to_value(&self) -> Value {
        let mut m = Obj::new();
        m.insert("x".into(), num(self.x));
        m.insert("y".into(), num(self.y));
        if let Some(b) = self.b {
            m.insert("b".into(), num(b));
        }
        merge(&mut m, &self.extra);
        Value::Object(m)
    }
    pub fn num(&self, k: &str) -> Option<f64> {
        self.extra.get(k).and_then(Value::as_f64)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Circle {
    pub cx: f64,
    pub cy: f64,
    pub r: f64,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Arc {
    pub cx: f64,
    pub cy: f64,
    pub r: f64,
    pub a0: f64,
    pub a1: f64,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Poly {
    pub pts: Vec<Vertex>,
    pub closed: Option<bool>,
}
impl Poly {
    /// `!!e.closed`
    pub fn is_closed(&self) -> bool {
        self.closed.unwrap_or(false)
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Ellipse {
    pub cx: f64,
    pub cy: f64,
    pub mx: f64,
    pub my: f64,
    pub ratio: f64,
    pub t0: f64,
    pub t1: f64,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub x: f64,
    pub y: f64,
    pub h: f64,
    pub s: String,
    pub rot: Option<f64>,
    pub ha: Option<String>,
    pub va: Option<String>,
    pub wf: Option<f64>,
    pub ww: Option<f64>,
    pub ls: Option<f64>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Insert {
    pub block: String,
    pub x: f64,
    pub y: f64,
    pub sx: Option<f64>,
    pub sy: Option<f64>,
    pub rot: Option<f64>,
    pub attrs: Option<Obj>,
    pub attloc: Option<Obj>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Hatch {
    pub loops: Vec<Vec<f64>>,
    pub solid: Option<bool>,
}
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Dim {
    pub kind: Option<String>,
    pub p1: Option<P2>,
    pub p2: Option<P2>,
    pub c: Option<P2>,
    pub loc: Option<P2>,
    pub rot: Option<f64>,
    pub text: Option<String>,
    pub st: Option<Obj>,
    pub block: Option<String>,
}
impl Dim {
    pub fn kind(&self) -> &str {
        self.kind.as_deref().unwrap_or("")
    }
    /// `e[k]` for the point keys p1 p2 c loc.
    pub fn point(&self, k: &str) -> Option<P2> {
        match k {
            "p1" => self.p1,
            "p2" => self.p2,
            "c" => self.c,
            "loc" => self.loc,
            _ => None,
        }
    }
    pub fn set_point(&mut self, k: &str, p: P2) {
        match k {
            "p1" => self.p1 = Some(p),
            "p2" => self.p2 = Some(p),
            "c" => self.c = Some(p),
            "loc" => self.loc = Some(p),
            _ => {}
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Viewport {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub cx: f64,
    pub cy: f64,
    pub s: f64,
    pub tw: Option<f64>,
}
/// An xline or a ray: a point and a unit direction.
#[derive(Clone, Debug, PartialEq)]
pub struct Inf {
    pub x: f64,
    pub y: f64,
    pub dx: f64,
    pub dy: f64,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Mleader {
    pub pts: Vec<P2>,
    pub dir: Option<f64>,
    pub s: Option<String>,
    pub h: Option<f64>,
    pub ww: Option<f64>,
    pub ls: Option<f64>,
    pub st: Option<Obj>,
}

/// The geometry of an entity, by `type`.
#[derive(Clone, Debug, PartialEq)]
pub enum Geo {
    Line(Line),
    Circle(Circle),
    Arc(Arc),
    Poly(Poly),
    Ellipse(Ellipse),
    Text(Text),
    Point(Point),
    Insert(Insert),
    Hatch(Hatch),
    Dim(Dim),
    Viewport(Viewport),
    Xline(Inf),
    Ray(Inf),
    Mleader(Mleader),
    /// Any other type (or a known one with unusable fields): `type` and everything else live in `extra`.
    Other,
}

/// One drawing object.
#[derive(Clone, Debug, PartialEq)]
pub struct Entity {
    pub id: Option<u64>,
    pub layer: Option<String>,
    pub color: Option<Color>,
    /// `lt`: a linetype name, or null; None = absent (by layer)
    pub lt: Option<Option<String>>,
    /// `lw`: mm (-2 by block, -3 default), or null; None = absent (by layer)
    pub lw: Option<Option<f64>>,
    pub geo: Geo,
    pub extra: Obj,
}

fn take_f(m: &mut Obj, k: &str) -> Option<f64> {
    let v = m.get(k)?.as_f64()?;
    m.remove(k);
    Some(v)
}
fn take_s(m: &mut Obj, k: &str) -> Option<String> {
    let v = m.get(k)?.as_str()?.to_string();
    m.remove(k);
    Some(v)
}
fn take_b(m: &mut Obj, k: &str) -> Option<bool> {
    let v = m.get(k)?.as_bool()?;
    m.remove(k);
    Some(v)
}
fn take_o(m: &mut Obj, k: &str) -> Option<Obj> {
    let v = m.get(k)?.as_object()?.clone();
    m.remove(k);
    Some(v)
}
fn take_p(m: &mut Obj, k: &str) -> Option<P2> {
    let p = pt_of(m.get(k))?;
    m.remove(k);
    Some(p)
}
/// Copy the keys of `extra` that `m` does not have yet.
fn merge(m: &mut Obj, extra: &Obj) {
    for (k, v) in extra {
        if !m.contains_key(k) {
            m.insert(k.clone(), v.clone());
        }
    }
}
/// Where an entity writes its keys: a JSON object (`to_obj`) or another object model (the DWG saver builds its own
/// values straight from the entity). Keys come in `to_obj`'s order.
pub trait ObjSink {
    fn num(&mut self, k: &'static str, v: f64);
    fn num_opt(&mut self, k: &'static str, v: Option<f64>) {
        if let Some(v) = v {
            self.num(k, v);
        }
    }
    fn id(&mut self, k: &'static str, v: u64);
    fn string(&mut self, k: &'static str, v: &str);
    fn str_opt(&mut self, k: &'static str, v: Option<&str>) {
        if let Some(v) = v {
            self.string(k, v);
        }
    }
    fn boolean(&mut self, k: &'static str, v: bool);
    fn null(&mut self, k: &'static str);
    fn color(&mut self, k: &'static str, c: &Color);
    fn object(&mut self, k: &'static str, v: &Obj);
    fn point(&mut self, k: &'static str, p: P2);
    fn points(&mut self, k: &'static str, pts: &[P2]);
    fn vertices(&mut self, k: &'static str, pts: &[Vertex]);
    fn loops(&mut self, k: &'static str, loops: &[Vec<f64>]);
    /// the keys of `extra` not written yet
    fn merge(&mut self, extra: &Obj);
}

impl ObjSink for Obj {
    fn num(&mut self, k: &'static str, v: f64) {
        put(self, k, v);
    }
    fn id(&mut self, k: &'static str, v: u64) {
        self.insert(k.into(), Value::from(v));
    }
    fn string(&mut self, k: &'static str, v: &str) {
        self.insert(k.into(), Value::String(v.to_string()));
    }
    fn boolean(&mut self, k: &'static str, v: bool) {
        self.insert(k.into(), Value::Bool(v));
    }
    fn null(&mut self, k: &'static str) {
        self.insert(k.into(), Value::Null);
    }
    fn color(&mut self, k: &'static str, c: &Color) {
        self.insert(k.into(), c.to_value());
    }
    fn object(&mut self, k: &'static str, v: &Obj) {
        self.insert(k.into(), Value::Object(v.clone()));
    }
    fn point(&mut self, k: &'static str, p: P2) {
        self.insert(k.into(), pt(p));
    }
    fn points(&mut self, k: &'static str, pts: &[P2]) {
        self.insert(k.into(), Value::Array(pts.iter().map(|p| pt(*p)).collect()));
    }
    fn vertices(&mut self, k: &'static str, pts: &[Vertex]) {
        self.insert(k.into(), Value::Array(pts.iter().map(Vertex::to_value).collect()));
    }
    fn loops(&mut self, k: &'static str, loops: &[Vec<f64>]) {
        self.insert(k.into(), Value::Array(loops.iter().map(|l| Value::Array(l.iter().map(|v| num(*v)).collect())).collect()));
    }
    fn merge(&mut self, extra: &Obj) {
        merge(self, extra);
    }
}

fn put(m: &mut Obj, k: &str, v: f64) {
    m.insert(k.into(), num(v));
}
fn parse_geo(typ: &str, m: &mut Obj) -> Option<Geo> {
    Some(match typ {
        "line" => Geo::Line(Line { x1: take_f(m, "x1")?, y1: take_f(m, "y1")?, x2: take_f(m, "x2")?, y2: take_f(m, "y2")? }),
        "circle" => Geo::Circle(Circle { cx: take_f(m, "cx")?, cy: take_f(m, "cy")?, r: take_f(m, "r")? }),
        "arc" => {
            Geo::Arc(Arc { cx: take_f(m, "cx")?, cy: take_f(m, "cy")?, r: take_f(m, "r")?, a0: take_f(m, "a0")?, a1: take_f(m, "a1")? })
        }
        "poly" => {
            let pts = m.get("pts")?.as_array()?.iter().map(Vertex::from_value).collect::<Option<Vec<_>>>()?;
            m.remove("pts");
            Geo::Poly(Poly { pts, closed: take_b(m, "closed") })
        }
        "ellipse" => Geo::Ellipse(Ellipse {
            cx: take_f(m, "cx")?,
            cy: take_f(m, "cy")?,
            mx: take_f(m, "mx")?,
            my: take_f(m, "my")?,
            ratio: take_f(m, "ratio")?,
            t0: take_f(m, "t0")?,
            t1: take_f(m, "t1")?,
        }),
        "text" => Geo::Text(Text {
            x: take_f(m, "x")?,
            y: take_f(m, "y")?,
            h: take_f(m, "h")?,
            s: take_s(m, "s")?,
            rot: take_f(m, "rot"),
            ha: take_s(m, "ha"),
            va: take_s(m, "va"),
            wf: take_f(m, "wf"),
            ww: take_f(m, "ww"),
            ls: take_f(m, "ls"),
        }),
        "point" => Geo::Point(Point { x: take_f(m, "x")?, y: take_f(m, "y")? }),
        "insert" => Geo::Insert(Insert {
            block: take_s(m, "block")?,
            x: take_f(m, "x")?,
            y: take_f(m, "y")?,
            sx: take_f(m, "sx"),
            sy: take_f(m, "sy"),
            rot: take_f(m, "rot"),
            attrs: take_o(m, "attrs"),
            attloc: take_o(m, "attloc"),
        }),
        "hatch" => {
            let loops = m
                .get("loops")?
                .as_array()?
                .iter()
                .map(|l| l.as_array().and_then(|a| a.iter().map(Value::as_f64).collect::<Option<Vec<f64>>>()))
                .collect::<Option<Vec<_>>>()?;
            m.remove("loops");
            Geo::Hatch(Hatch { loops, solid: take_b(m, "solid") })
        }
        "dim" => Geo::Dim(Dim {
            kind: take_s(m, "kind"),
            p1: take_p(m, "p1"),
            p2: take_p(m, "p2"),
            c: take_p(m, "c"),
            loc: take_p(m, "loc"),
            rot: take_f(m, "rot"),
            text: take_s(m, "text"),
            st: take_o(m, "st"),
            block: take_s(m, "block"),
        }),
        "viewport" => Geo::Viewport(Viewport {
            x: take_f(m, "x")?,
            y: take_f(m, "y")?,
            w: take_f(m, "w")?,
            h: take_f(m, "h")?,
            cx: take_f(m, "cx")?,
            cy: take_f(m, "cy")?,
            s: take_f(m, "s")?,
            tw: take_f(m, "tw"),
        }),
        "xline" | "ray" => {
            let i = Inf { x: take_f(m, "x")?, y: take_f(m, "y")?, dx: take_f(m, "dx")?, dy: take_f(m, "dy")? };
            if typ == "xline" { Geo::Xline(i) } else { Geo::Ray(i) }
        }
        "mleader" => {
            let pts = m.get("pts")?.as_array()?.iter().map(|p| pt_of(Some(p))).collect::<Option<Vec<_>>>()?;
            m.remove("pts");
            Geo::Mleader(Mleader {
                pts,
                dir: take_f(m, "dir"),
                s: take_s(m, "s"),
                h: take_f(m, "h"),
                ww: take_f(m, "ww"),
                ls: take_f(m, "ls"),
                st: take_o(m, "st"),
            })
        }
        _ => return None,
    })
}

impl Geo {
    /// The `type` string.
    pub fn type_name(&self) -> Option<&'static str> {
        Some(match self {
            Geo::Line(_) => "line",
            Geo::Circle(_) => "circle",
            Geo::Arc(_) => "arc",
            Geo::Poly(_) => "poly",
            Geo::Ellipse(_) => "ellipse",
            Geo::Text(_) => "text",
            Geo::Point(_) => "point",
            Geo::Insert(_) => "insert",
            Geo::Hatch(_) => "hatch",
            Geo::Dim(_) => "dim",
            Geo::Viewport(_) => "viewport",
            Geo::Xline(_) => "xline",
            Geo::Ray(_) => "ray",
            Geo::Mleader(_) => "mleader",
            Geo::Other => return None,
        })
    }
    fn write<S: ObjSink>(&self, m: &mut S) {
        match self {
            Geo::Line(l) => {
                m.num("x1", l.x1);
                m.num("y1", l.y1);
                m.num("x2", l.x2);
                m.num("y2", l.y2);
            }
            Geo::Circle(c) => {
                m.num("cx", c.cx);
                m.num("cy", c.cy);
                m.num("r", c.r);
            }
            Geo::Arc(a) => {
                m.num("cx", a.cx);
                m.num("cy", a.cy);
                m.num("r", a.r);
                m.num("a0", a.a0);
                m.num("a1", a.a1);
            }
            Geo::Poly(p) => {
                if let Some(c) = p.closed {
                    m.boolean("closed", c);
                }
                m.vertices("pts", &p.pts);
            }
            Geo::Ellipse(e) => {
                for (k, v) in [("cx", e.cx), ("cy", e.cy), ("mx", e.mx), ("my", e.my), ("ratio", e.ratio), ("t0", e.t0), ("t1", e.t1)] {
                    m.num(k, v);
                }
            }
            Geo::Text(t) => {
                m.num("x", t.x);
                m.num("y", t.y);
                m.num("h", t.h);
                m.num_opt("rot", t.rot);
                m.string("s", &t.s);
                m.str_opt("ha", t.ha.as_deref());
                m.str_opt("va", t.va.as_deref());
                m.num_opt("wf", t.wf);
                m.num_opt("ww", t.ww);
                m.num_opt("ls", t.ls);
            }
            Geo::Point(p) => {
                m.num("x", p.x);
                m.num("y", p.y);
            }
            Geo::Insert(i) => {
                m.string("block", &i.block);
                m.num("x", i.x);
                m.num("y", i.y);
                m.num_opt("sx", i.sx);
                m.num_opt("sy", i.sy);
                m.num_opt("rot", i.rot);
                if let Some(a) = &i.attrs {
                    m.object("attrs", a);
                }
                if let Some(a) = &i.attloc {
                    m.object("attloc", a);
                }
            }
            Geo::Hatch(h) => {
                if let Some(s) = h.solid {
                    m.boolean("solid", s);
                }
                m.loops("loops", &h.loops);
            }
            Geo::Dim(d) => {
                m.str_opt("kind", d.kind.as_deref());
                for (k, p) in [("p1", d.p1), ("p2", d.p2), ("c", d.c), ("loc", d.loc)] {
                    if let Some(p) = p {
                        m.point(k, p);
                    }
                }
                m.num_opt("rot", d.rot);
                m.str_opt("text", d.text.as_deref());
                if let Some(st) = &d.st {
                    m.object("st", st);
                }
                m.str_opt("block", d.block.as_deref());
            }
            Geo::Viewport(v) => {
                for (k, x) in [("x", v.x), ("y", v.y), ("w", v.w), ("h", v.h), ("cx", v.cx), ("cy", v.cy), ("s", v.s)] {
                    m.num(k, x);
                }
                m.num_opt("tw", v.tw);
            }
            Geo::Xline(i) | Geo::Ray(i) => {
                m.num("x", i.x);
                m.num("y", i.y);
                m.num("dx", i.dx);
                m.num("dy", i.dy);
            }
            Geo::Mleader(l) => {
                m.points("pts", &l.pts);
                m.num_opt("dir", l.dir);
                m.str_opt("s", l.s.as_deref());
                m.num_opt("h", l.h);
                m.num_opt("ww", l.ww);
                m.num_opt("ls", l.ls);
                if let Some(st) = &l.st {
                    m.object("st", st);
                }
            }
            Geo::Other => {}
        }
    }
}

impl Entity {
    /// A new entity of this geometry with no id, layer or colour keys.
    pub fn new(geo: Geo) -> Self {
        Entity { id: None, layer: None, color: None, lt: None, lw: None, geo, extra: Obj::new() }
    }
    /// With `layer` and `color` set (most entities geometry makes carry both).
    pub fn with(geo: Geo, layer: Option<String>, color: Option<Color>) -> Self {
        Entity { layer, color, ..Entity::new(geo) }
    }

    /// From a JSON object (anything else → an Other with no keys).
    pub fn from_value(v: &Value) -> Entity {
        let Some(src) = v.as_object() else { return Entity::new(Geo::Other) };
        let mut m = src.clone();
        let id = match m.get("id") {
            Some(Value::Number(n)) => {
                n.as_u64().or_else(|| n.as_f64().filter(|f| f.fract() == 0.0 && *f >= 0.0 && *f < 9e15).map(f64_to_u64))
            }
            _ => None,
        };
        if id.is_some() {
            m.remove("id");
        }
        let layer = take_s(&mut m, "layer");
        let color = match m.get("color").map(Color::from_value) {
            Some(Some(c)) => {
                m.remove("color");
                Some(c)
            }
            _ => None,
        };
        let lt = match m.get("lt") {
            Some(Value::Null) => Some(None),
            Some(Value::String(s)) => Some(Some(s.clone())),
            _ => None,
        };
        if lt.is_some() {
            m.remove("lt");
        }
        let lw = match m.get("lw") {
            Some(Value::Null) => Some(None),
            Some(Value::Number(n)) => n.as_f64().map(Some),
            _ => None,
        };
        if lw.is_some() {
            m.remove("lw");
        }
        let typ = m.get("type").and_then(Value::as_str).map(str::to_string);
        let geo = match typ {
            Some(t) => {
                let mut g = m.clone();
                g.remove("type");
                match parse_geo(&t, &mut g) {
                    Some(geo) => {
                        m = g;
                        geo
                    }
                    None => Geo::Other,
                }
            }
            None => Geo::Other,
        };
        Entity { id, layer, color, lt, lw, geo, extra: m }
    }

    /// The JSON object.
    pub fn to_value(&self) -> Value {
        Value::Object(self.to_obj())
    }
    pub fn to_obj(&self) -> Obj {
        let mut m = Obj::new();
        self.write_to(&mut m);
        m
    }
    /// The keys of [`Entity::to_obj`], in the same order, into any [`ObjSink`].
    pub fn write_to<S: ObjSink>(&self, m: &mut S) {
        if let Some(t) = self.geo.type_name() {
            m.string("type", t);
        }
        if let Some(id) = self.id {
            m.id("id", id);
        }
        if let Some(l) = &self.layer {
            m.string("layer", l);
        }
        if let Some(c) = &self.color {
            m.color("color", c);
        }
        self.geo.write(m);
        match &self.lt {
            Some(Some(s)) => m.string("lt", s),
            Some(None) => m.null("lt"),
            None => {}
        }
        match self.lw {
            Some(Some(w)) => m.num("lw", w),
            Some(None) => m.null("lw"),
            None => {}
        }
        m.merge(&self.extra);
    }

    /// `e.type` ('' when it has none).
    pub fn typ(&self) -> &str {
        match self.geo.type_name() {
            Some(t) => t,
            None => self.extra.get("type").and_then(Value::as_str).unwrap_or(""),
        }
    }
    /// `e.layer` ('' when absent; doc.js always sets it).
    pub fn layer_str(&self) -> &str {
        self.layer.as_deref().unwrap_or("")
    }
    /// A number kept in `extra` (`e.k` when it is a number).
    pub fn num(&self, k: &str) -> Option<f64> {
        self.extra.get(k).and_then(Value::as_f64)
    }
    /// A value kept in `extra`.
    pub fn get(&self, k: &str) -> Option<&Value> {
        self.extra.get(k)
    }
    /// Is the type one of the unbounded construction lines (xline, ray)?
    pub fn unbounded(&self) -> bool {
        matches!(self.geo, Geo::Xline(_) | Geo::Ray(_))
    }
}

/// An id read from JSON (checked integral, non-negative and below 9e15 by the caller).
fn f64_to_u64(f: f64) -> u64 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let u = f as u64;
    u
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn round_trip() {
        for v in [
            json!({"type":"line","id":3,"layer":"0","color":null,"x1":0,"y1":1.5,"x2":2,"y2":3,"lt":"DASHED","foo":{"a":1}}),
            json!({"type":"poly","layer":"A","color":5,"closed":true,"pts":[{"x":0,"y":0,"b":0.5,"sw":1},{"x":1,"y":0}]}),
            json!({"type":"text","x":1,"y":2,"h":2.5,"s":"Hi","rot":null,"tag":"T"}),
            json!({"type":"line","x1":"a","y1":0,"x2":0,"y2":0}),
            json!({"type":"weird","q":[1,2]}),
            json!({"type":"dim","kind":"linear","p1":{"x":0,"y":0},"p2":{"x":1,"y":0},"loc":{"x":0,"y":1},"st":{"txt":3},"text":""}),
        ] {
            assert_eq!(Entity::from_value(&v).to_value(), v);
        }
        let o = Entity::from_value(&json!({"type":"line","x1":"a","y1":0,"x2":0,"y2":0}));
        assert_eq!(o.geo, Geo::Other);
        assert_eq!(o.typ(), "line");
    }
}
