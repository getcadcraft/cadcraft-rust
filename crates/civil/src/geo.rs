//! Plain point types shared by the modules, and their JSON forms (the JS objects `{x, y}` / `{x, y, z}`).

use crate::js::{J, fin, numf};
use serde_json::Value;

/// `{x, y}`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct P2 {
    pub x: f64,
    pub y: f64,
}
/// `{x, y, z}`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct P3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}
/// `{x, y, z?}` — a corner that may carry its own elevation (`Number.isFinite(p.z)` decides); `z: None` = no key.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct PZ {
    pub x: f64,
    pub y: f64,
    pub z: Option<f64>,
}

pub const fn p2(x: f64, y: f64) -> P2 {
    P2 { x, y }
}
pub const fn p3(x: f64, y: f64, z: f64) -> P3 {
    P3 { x, y, z }
}

impl P3 {
    pub fn xy(&self) -> P2 {
        P2 { x: self.x, y: self.y }
    }
}
impl PZ {
    pub fn xy(&self) -> P2 {
        P2 { x: self.x, y: self.y }
    }
    /// `Number.isFinite(p.z) ? p.z : d`.
    pub fn zor(&self, d: f64) -> f64 {
        match self.z {
            Some(z) if z.is_finite() => z,
            _ => d,
        }
    }
}

/// Anything with a plan position.
pub trait XY {
    fn x(&self) -> f64;
    fn y(&self) -> f64;
}
impl XY for P2 {
    fn x(&self) -> f64 {
        self.x
    }
    fn y(&self) -> f64 {
        self.y
    }
}
impl XY for P3 {
    fn x(&self) -> f64 {
        self.x
    }
    fn y(&self) -> f64 {
        self.y
    }
}
impl XY for PZ {
    fn x(&self) -> f64 {
        self.x
    }
    fn y(&self) -> f64 {
        self.y
    }
}

impl J for P2 {
    fn j(&self) -> Value {
        crate::jo! { "x" => self.x, "y" => self.y }
    }
}
impl J for P3 {
    fn j(&self) -> Value {
        crate::jo! { "x" => self.x, "y" => self.y, "z" => self.z }
    }
}
impl J for PZ {
    fn j(&self) -> Value {
        let mut m = serde_json::Map::new();
        m.insert("x".into(), self.x.j());
        m.insert("y".into(), self.y.j());
        if let Some(z) = self.z {
            m.insert("z".into(), z.j());
        }
        Value::Object(m)
    }
}

/// `{x, y}` from JSON (numbers as given; anything else NaN, as JS arithmetic on `undefined`).
pub fn read_p2(v: &Value) -> P2 {
    P2 { x: numf(v.get("x")), y: numf(v.get("y")) }
}
pub fn read_p3(v: &Value) -> P3 {
    P3 { x: numf(v.get("x")), y: numf(v.get("y")), z: numf(v.get("z")) }
}
/// `{x, y, z?}`: z kept when it is a number (finite or not), absent otherwise.
pub fn read_pz(v: &Value) -> PZ {
    PZ { x: numf(v.get("x")), y: numf(v.get("y")), z: v.get("z").filter(|z| !z.is_null()).map(|z| numf(Some(z))) }
}
pub fn read_p2s(v: Option<&Value>) -> Vec<P2> {
    crate::js::arr(v).iter().map(read_p2).collect()
}
pub fn read_p3s(v: Option<&Value>) -> Vec<P3> {
    crate::js::arr(v).iter().map(read_p3).collect()
}
pub fn read_pzs(v: Option<&Value>) -> Vec<PZ> {
    crate::js::arr(v).iter().map(read_pz).collect()
}
/// `q && fin(q.x) && fin(q.y) ? {x, y} : null`.
pub fn clean_xy(q: &Value) -> Option<P2> {
    Some(P2 { x: fin(q.get("x"))?, y: fin(q.get("y"))? })
}
/// `q && fin(q.x) && fin(q.y) && fin(q.z) ? {x, y, z} : null`.
pub fn clean_xyz(q: &Value) -> Option<P3> {
    Some(P3 { x: fin(q.get("x"))?, y: fin(q.get("y"))?, z: fin(q.get("z"))? })
}
