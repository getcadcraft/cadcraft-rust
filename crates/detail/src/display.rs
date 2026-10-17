//! The display lists drawings, shop drawings and reports are made of (pdf.js's input): paths, circles and texts on a
//! landscape sheet, mm, y up.

use crate::js::{Obj, jnum};
use serde_json::Value;

/// One item.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Path {
        pts: Vec<[f64; 2]>,
        w: f64,
        dash: Option<Vec<f64>>,
        closed: bool,
        fill: bool,
    },
    /// `fill`: drawings' circles carry it, shop drawings' do not (None).
    Circle {
        c: [f64; 2],
        r: f64,
        w: f64,
        dash: Option<Vec<f64>>,
        fill: Option<bool>,
    },
    Text {
        s: String,
        x: f64,
        y: f64,
        h: f64,
        align: &'static str,
        rot: f64,
        bold: bool,
    },
}

fn pts_json(p: &[[f64; 2]]) -> Value {
    Value::Array(p.iter().map(|q| Value::Array(vec![jnum(q[0]), jnum(q[1])])).collect())
}
fn dash_json(d: &Option<Vec<f64>>) -> Value {
    match d {
        Some(v) => Value::Array(v.iter().map(|x| jnum(*x)).collect()),
        None => Value::Null,
    }
}
impl Item {
    pub fn j(&self) -> Value {
        match self {
            Item::Path { pts, w, dash, closed, fill } => Obj::new()
                .set("k", "path")
                .set("pts", pts_json(pts))
                .set("w", *w)
                .set("dash", dash_json(dash))
                .set("closed", *closed)
                .set("fill", *fill)
                .done(),
            Item::Circle { c, r, w, dash, fill } => Obj::new()
                .set("k", "circle")
                .set("c", pts_json(&[*c]).as_array().and_then(|a| a.first().cloned()).unwrap_or(Value::Null))
                .set("r", *r)
                .set("w", *w)
                .set("dash", dash_json(dash))
                .opt("fill", *fill)
                .done(),
            Item::Text { s, x, y, h, align, rot, bold } => Obj::new()
                .set("k", "text")
                .set("s", s.as_str())
                .set("x", *x)
                .set("y", *y)
                .set("h", *h)
                .set("align", *align)
                .set("rot", *rot)
                .set("bold", *bold)
                .done(),
        }
    }
}

/// A pen: width and dash.
#[derive(Clone, Debug, PartialEq)]
pub struct Pen {
    pub w: f64,
    pub dash: Option<Vec<f64>>,
}
impl Pen {
    pub fn new(w: f64) -> Pen {
        Pen { w, dash: None }
    }
    pub fn dashed(w: f64, d: &[f64]) -> Pen {
        Pen { w, dash: Some(d.to_vec()) }
    }
}

/// Text options.
#[derive(Clone, Copy, Debug, Default)]
pub struct To {
    pub align: &'static str,
    pub rot: f64,
    pub bold: bool,
}
impl To {
    pub fn left() -> To {
        To { align: "left", rot: 0.0, bold: false }
    }
    pub fn center() -> To {
        To { align: "center", rot: 0.0, bold: false }
    }
    pub fn right() -> To {
        To { align: "right", rot: 0.0, bold: false }
    }
    pub fn bold(mut self) -> To {
        self.bold = true;
        self
    }
    pub fn rot(mut self, r: f64) -> To {
        self.rot = r;
        self
    }
}

/// A sheet: size, paper key and items.
#[derive(Clone, Debug, PartialEq)]
pub struct List {
    pub w: f64,
    pub h: f64,
    pub items: Vec<Item>,
}

/// Reads a display list from JSON (the harness and the CLI hand them over).
pub fn list_of(v: &Value) -> List {
    let f = |v: Option<&Value>| v.and_then(Value::as_f64).unwrap_or(f64::NAN);
    let pt = |v: Option<&Value>| {
        let a = crate::js::arr(v);
        [f(a.first()), f(a.get(1))]
    };
    let dash = |v: Option<&Value>| v.and_then(Value::as_array).map(|a| a.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect());
    let mut items = Vec::new();
    for it in crate::js::arr(v.get("items")) {
        let b = |k: &str| crate::js::truthy(it.get(k));
        match it.get("k").and_then(Value::as_str) {
            Some("path") => items.push(Item::Path {
                pts: crate::js::arr(it.get("pts")).iter().map(|p| pt(Some(p))).collect(),
                w: it.get("w").and_then(Value::as_f64).unwrap_or(0.25),
                dash: dash(it.get("dash")),
                closed: b("closed"),
                fill: b("fill"),
            }),
            Some("circle") => items.push(Item::Circle {
                c: pt(it.get("c")),
                r: f(it.get("r")),
                w: it.get("w").and_then(Value::as_f64).unwrap_or(0.25),
                dash: dash(it.get("dash")),
                fill: it.get("fill").map(|x| crate::js::truthy(Some(x))),
            }),
            Some("text") => items.push(Item::Text {
                s: crate::js::string_of(it.get("s")),
                x: f(it.get("x")),
                y: f(it.get("y")),
                h: f(it.get("h")),
                align: match it.get("align").and_then(Value::as_str) {
                    Some("center") => "center",
                    Some("right") => "right",
                    _ => "left",
                },
                rot: it.get("rot").and_then(Value::as_f64).unwrap_or(0.0),
                bold: b("bold"),
            }),
            _ => {}
        }
    }
    List { w: f(v.get("w")), h: f(v.get("h")), items }
}

/// A JS number as an integer: saturating, NaN → 0 (the inputs are dates and counts).
#[allow(clippy::cast_possible_truncation)]
pub fn sat_i64(x: f64) -> i64 {
    x.clamp(-9.0e15, 9.0e15) as i64
}
