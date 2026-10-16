//! What a tool asks the view to draw over the model (the JS's `host.overlay(o)` objects), as data: markers, lines,
//! polygons, faces, grips, a window rectangle, hovered and collected picks, dimension previews, a section plane, and
//! the tip. A field the JS leaves out is `None`; one it sets to `null` is `Some(None)`. [`Overlay::to_json`] writes
//! exactly the JS object (the parity harness compares the two).

use cc_m3d::ops::Sel;
use cc_m3d::vec::P3;
use serde_json::{Map, Value, json};

use crate::outj::{num, p3, pts};

/// A snap marker: where, and which kind (its colour).
#[derive(Clone, Debug, PartialEq)]
pub struct Marker {
    pub p: P3,
    pub kind: String,
}

/// A line to draw (`kind` picks the colour; `dash` dashed).
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub a: P3,
    pub b: P3,
    pub kind: String,
    pub dash: bool,
}
impl Line {
    pub fn new(a: P3, b: P3, kind: &str) -> Line {
        Line { a, b, kind: kind.to_string(), dash: false }
    }
    pub fn dashed(a: P3, b: P3, kind: &str) -> Line {
        Line { a, b, kind: kind.to_string(), dash: true }
    }
}

/// The window / crossing rectangle (screen pixels).
#[derive(Clone, Debug, PartialEq)]
pub struct RectOv {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
    pub crossing: bool,
}

/// A dimension being placed (the view lays it out with infer's layouts).
#[derive(Clone, Debug, PartialEq)]
pub enum DimOv {
    /// a radius / diameter: the curve's centre, radius and normal
    Rad { kind: String, p: P3, r: f64, n: Option<P3>, off: P3 },
    /// an angle at o between the lines to a and b
    Ang { o: P3, a: P3, b: P3, off: P3 },
    /// a length from a to b
    Lin { a: P3, b: P3, off: P3 },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overlay {
    pub marker: Option<Option<Marker>>,
    pub tip: Option<String>,
    pub lines: Option<Vec<Line>>,
    pub poly: Option<Vec<P3>>,
    pub poly_kind: Option<String>,
    pub faces: Option<Vec<[P3; 3]>>,
    pub points: Option<Vec<P3>>,
    pub grips: Option<Vec<P3>>,
    pub hot_grip: Option<f64>,
    pub rect: Option<RectOv>,
    pub hover: Option<Option<Sel>>,
    pub many: Option<Vec<Sel>>,
    pub dims: Option<Vec<DimOv>>,
    /// a section plane being placed: (p, n)
    pub section: Option<(P3, P3)>,
}

fn sel_json(s: &Sel) -> Value {
    json!({"t": s.t, "id": num(s.id)})
}

impl Overlay {
    pub fn to_json(&self) -> Value {
        let mut o = Map::new();
        if let Some(m) = &self.marker {
            o.insert("marker".into(), m.as_ref().map_or(Value::Null, |m| json!({"p": p3(&m.p), "kind": m.kind})));
        }
        if let Some(t) = &self.tip {
            o.insert("tip".into(), Value::String(t.clone()));
        }
        if let Some(ls) = &self.lines {
            let v: Vec<Value> = ls
                .iter()
                .map(|l| {
                    let mut x = Map::new();
                    x.insert("a".into(), p3(&l.a));
                    x.insert("b".into(), p3(&l.b));
                    x.insert("kind".into(), Value::String(l.kind.clone()));
                    if l.dash {
                        x.insert("dash".into(), Value::Bool(true));
                    }
                    Value::Object(x)
                })
                .collect();
            o.insert("lines".into(), Value::Array(v));
        }
        if let Some(p) = &self.poly {
            o.insert("poly".into(), pts(p));
        }
        if let Some(k) = &self.poly_kind {
            o.insert("polyKind".into(), Value::String(k.clone()));
        }
        if let Some(fs) = &self.faces {
            o.insert("faces".into(), Value::Array(fs.iter().map(|t| pts(t)).collect()));
        }
        if let Some(p) = &self.points {
            o.insert("points".into(), pts(p));
        }
        if let Some(g) = &self.grips {
            o.insert("grips".into(), pts(g));
        }
        if let Some(h) = self.hot_grip {
            o.insert("hotGrip".into(), num(h));
        }
        if let Some(r) = &self.rect {
            o.insert("rect".into(), json!({"x0": num(r.x0), "y0": num(r.y0), "x1": num(r.x1), "y1": num(r.y1), "crossing": r.crossing}));
        }
        if let Some(h) = &self.hover {
            o.insert("hover".into(), h.as_ref().map_or(Value::Null, sel_json));
        }
        if let Some(m) = &self.many {
            o.insert("many".into(), Value::Array(m.iter().map(sel_json).collect()));
        }
        if let Some(ds) = &self.dims {
            let v = ds
                .iter()
                .map(|d| match d {
                    DimOv::Rad { kind, p, r, n, off } => {
                        json!({"kind": kind, "c": {"p": p3(p), "r": num(*r), "n": n.as_ref().map_or(Value::Null, p3)}, "off": p3(off)})
                    }
                    DimOv::Ang { o, a, b, off } => json!({"kind": "ang", "o": p3(o), "a": p3(a), "b": p3(b), "off": p3(off)}),
                    DimOv::Lin { a, b, off } => json!({"a": p3(a), "b": p3(b), "off": p3(off)}),
                })
                .collect();
            o.insert("dims".into(), Value::Array(v));
        }
        if let Some((p, n)) = &self.section {
            o.insert("section".into(), json!({"p": p3(p), "n": p3(n)}));
        }
        Value::Object(o)
    }
}
