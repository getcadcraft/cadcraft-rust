//! stair.js — a straight steel stair from its rise, going and width (0.4 D23): `cleanStair` (the checks a stored or
//! pasted stair goes through) and `stairGeometry` (risers, treads, stringers, welds, IBC 2021 1011.5.2 and Blondel
//! checks). Millimetres, z up.

use crate::geom::V3;
use crate::i18n::{F, t0};
use crate::js::{Obj, atan, atan2, cos, min, num, round, sin, string_of, to_js_string};
use crate::profiles::P;
use serde_json::{Map, Value};
use std::f64::consts::PI;

/// A stair as `cleanStair` leaves it (plus id and phase when stored).
#[derive(Clone, Debug, PartialEq)]
pub struct Stair {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub dir: f64,
    pub rise: f64,
    pub going: f64,
    pub width: f64,
    pub max_riser: f64,
    pub stringer: String,
    pub tread_t: f64,
    pub nosing: f64,
    pub material: String,
    pub phase: f64,
}
impl Stair {
    /// The fields `cleanStair` returns, in its order.
    pub fn fields(&self) -> Map<String, Value> {
        let mut m = Map::new();
        for (k, v) in [
            ("x", self.x),
            ("y", self.y),
            ("z", self.z),
            ("dir", self.dir),
            ("rise", self.rise),
            ("going", self.going),
            ("width", self.width),
            ("maxRiser", self.max_riser),
        ] {
            m.insert(k.into(), Value::from(v));
        }
        m.insert("stringer".into(), Value::from(self.stringer.clone()));
        m.insert("tread".into(), Obj::new().set("t", self.tread_t).set("nosing", self.nosing).done());
        m.insert("material".into(), Value::from(self.material.clone()));
        m
    }
    /// A stored stair read back.
    pub fn from_value(v: &Value) -> Stair {
        let g = |k: &str| crate::js::f(v.get(k));
        let tr = v.get("tread");
        Stair {
            id: crate::js::s(v.get("id")).to_string(),
            x: g("x"),
            y: g("y"),
            z: g("z"),
            dir: g("dir"),
            rise: g("rise"),
            going: g("going"),
            width: g("width"),
            max_riser: g("maxRiser"),
            stringer: crate::js::s(v.get("stringer")).to_string(),
            tread_t: crate::js::f(tr.and_then(|t| t.get("t"))),
            nosing: crate::js::f(tr.and_then(|t| t.get("nosing"))),
            material: crate::js::s(v.get("material")).to_string(),
            phase: crate::js::or(crate::js::f(v.get("phase")), 1.0),
        }
    }
}

/// `STAIR_DEFAULTS`.
pub fn stair_defaults() -> Value {
    serde_json::json!({ "dir": 0, "rise": 3000, "going": 280, "width": 1000, "maxRiser": 178, "stringer": "C10x15.3", "tread": { "t": 6, "nosing": 25 }, "material": "A36" })
}

fn numr(v: f64, lo: f64, hi: f64) -> Option<f64> {
    if v.is_finite() && v >= lo && v <= hi { Some(v) } else { None }
}

/// `cleanStair(s)` → the stair, or the name of the first bad field.
pub fn clean_stair(s: &Value) -> Result<Stair, String> {
    match s {
        Value::Object(o) => clean_fields(o),
        // an array is an object to the JS: every field is absent, so the defaults are used
        Value::Array(_) => clean_fields(&Map::new()),
        _ => Err("stair".into()),
    }
}

fn clean_fields(o: &Map<String, Value>) -> Result<Stair, String> {
    let nz = Value::from(0);
    let get = |k: &str, d: &Value| -> f64 { num(Some(crate::js::nullish(o.get(k), d))) };
    let tread = o.get("tread").and_then(Value::as_object);
    let tget = |k: &str, d: f64| -> f64 {
        match tread.and_then(|t| t.get(k)) {
            None | Some(Value::Null) => d,
            Some(v) => num(Some(v)),
        }
    };
    let x = numr(get("x", &nz), -1e7, 1e7);
    let y = numr(get("y", &nz), -1e7, 1e7);
    let z = numr(get("z", &nz), -1e7, 1e7);
    let dir = numr(get("dir", &Value::from(0)), -3600.0, 3600.0);
    let rise = numr(get("rise", &Value::from(3000)), 300.0, 10000.0);
    let going = numr(get("going", &Value::from(280)), 150.0, 600.0);
    let width = numr(get("width", &Value::from(1000)), 400.0, 5000.0);
    let max_riser = numr(get("maxRiser", &Value::from(178)), 100.0, 300.0);
    let stringer = crate::js::slice16(
        crate::js::trim(&match o.get("stringer") {
            None | Some(Value::Null) => "C10x15.3".to_string(),
            Some(v) => to_js_string(v),
        }),
        40,
    );
    let tt = numr(tget("t", 6.0), 3.0, 50.0);
    let nosing = numr(tget("nosing", 25.0), 0.0, 100.0);
    let material = {
        let raw = match o.get("material") {
            None | Some(Value::Null) => "A36".to_string(),
            Some(v) => to_js_string(v),
        };
        let c = crate::js::slice16(crate::js::trim(&crate::js::strip_c0(&raw)), 30);
        if c.is_empty() { "A36".to_string() } else { c }
    };
    for (k, v) in [("x", x), ("y", y), ("z", z), ("dir", dir), ("rise", rise), ("going", going), ("width", width), ("maxRiser", max_riser)]
    {
        if v.is_none() {
            return Err(k.into());
        }
    }
    for (k, v) in [("t", tt), ("nosing", nosing)] {
        if v.is_none() {
            return Err(k.into());
        }
    }
    if stringer.is_empty() {
        return Err("stringer".into());
    }
    let n = |v: Option<f64>| v.unwrap_or(f64::NAN);
    let d = n(dir);
    Ok(Stair {
        id: String::new(),
        x: n(x),
        y: n(y),
        z: n(z),
        dir: ((d % 360.0) + 360.0) % 360.0,
        rise: n(rise),
        going: n(going),
        width: n(width),
        max_riser: n(max_riser),
        stringer,
        tread_t: n(tt),
        nosing: n(nosing),
        material,
        phase: 1.0,
    })
}

/// A stair part (a stringer or a tread).
#[derive(Clone, Debug)]
pub struct StairPart {
    pub key: String,
    pub kind: &'static str, // `type`
    pub role: String,
    pub profile_name: String,
    pub material: String,
    pub start: V3,
    pub end: V3,
    pub rotation: f64,
    pub attach_to: Option<String>,
}
/// A stair weld.
#[derive(Clone, Debug)]
pub struct StairWeld {
    pub between: [String; 2],
    pub size: f64,
    pub length: f64,
    pub r#where: String,
}
/// A stair check: code, the text key and its fills.
#[derive(Clone, Debug)]
pub struct StairProblem {
    pub code: &'static str,
    pub text: &'static str,
    pub fills: Vec<(&'static str, F)>,
}
/// `stairGeometry(s, profileOf)`.
#[derive(Clone, Debug)]
pub struct StairGeo {
    pub n: f64,
    pub riser: f64,
    pub treads: f64,
    pub going: f64,
    pub run: f64,
    pub pitch: f64,
    pub blondel: f64,
    pub parts: Vec<StairPart>,
    pub welds: Vec<StairWeld>,
    pub problems: Vec<StairProblem>,
}

const R: f64 = PI / 180.0;
/// The tread count cap: rise ≤ 10 000 and maxRiser ≥ 100 give at most 100 risers; a stair built from unchecked
/// input stops there instead of allocating without bound.
const MAX_TREADS: f64 = 10_000.0;

pub fn stair_geometry(s: &Stair, profile_of: &dyn Fn(&str) -> Option<P>) -> StairGeo {
    let mut problems = Vec::new();
    let n = crate::js::max(2.0, (s.rise / s.max_riser - 1e-9).ceil());
    let r = s.rise / n;
    let g = s.going;
    let treads = n - 1.0;
    let run = treads * g;
    let pitch = atan2(r, g) / R;
    let (c, sn) = (cos(s.dir * R), sin(s.dir * R));
    let fwd = [c, sn, 0.0];
    let side = [-sn, c, 0.0];
    let pp = |along: f64, across: f64, up: f64| -> V3 {
        [s.x + fwd[0] * along + side[0] * across, s.y + fwd[1] * along + side[1] * across, s.z + up]
    };
    let prof = profile_of(&s.stringer);
    if prof.is_none() {
        problems.push(StairProblem { code: "stringer", text: "stair.prob.stringer", fills: vec![("name", F::from(s.stringer.as_str()))] });
    }
    let is_pl = prof.as_ref().is_some_and(|p| p.kind == "PL");
    let sw = prof.as_ref().map_or(100.0, |p| if is_pl { p.t() } else { p.width });
    let sd = prof.as_ref().map_or(250.0, |p| if is_pl { p.b() } else { p.depth });
    let k = r / g;
    let cos_a = cos(atan(k));
    let drop = (sd / 2.0 - 50.0) / cos_a;
    if sd / 2.0 - 50.0 < s.tread_t + 10.0 {
        problems.push(StairProblem { code: "shallow", text: "stair.prob.shallow", fills: vec![("name", F::from(s.stringer.as_str()))] });
    }
    let axis_z = |along: f64| k * (along + g) - drop;
    let a0 = drop / k - g;
    let a1 = (s.rise + drop) / k - g;
    let half = s.width / 2.0 + sw / 2.0;
    let mut parts = Vec::new();
    for (key, side_sign, role) in [("L", 1.0, t0("stair.role.left")), ("R", -1.0, t0("stair.role.right"))] {
        let rotation = if prof.as_ref().is_some_and(|p| p.kind == "C") {
            if side_sign > 0.0 { 180.0 } else { 0.0 }
        } else if is_pl {
            90.0
        } else {
            0.0
        };
        parts.push(StairPart {
            key: format!("stringer{key}"),
            kind: "stringer",
            role,
            profile_name: s.stringer.clone(),
            material: s.material.clone(),
            start: pp(a0, side_sign * half, axis_z(a0)),
            end: pp(a1, side_sign * half, axis_z(a1)),
            rotation,
            attach_to: if key == "L" { None } else { Some("stringerL".into()) },
        });
    }
    let depth = g + s.nosing;
    let mut i = 1.0;
    while i <= treads && i <= MAX_TREADS {
        let mid = (i - 1.0) * g - s.nosing + depth / 2.0;
        parts.push(StairPart {
            key: format!("tread{}", crate::js::num_to_string(i)),
            kind: "plate",
            role: t0("stair.role.tread"),
            profile_name: format!("PL{}x{}", crate::js::num_to_string(s.tread_t), crate::js::num_to_string(round(depth * 10.0) / 10.0)),
            material: s.material.clone(),
            start: pp(mid, s.width / 2.0, i * r - s.tread_t / 2.0),
            end: pp(mid, -s.width / 2.0, i * r - s.tread_t / 2.0),
            rotation: 0.0,
            attach_to: Some("stringerL".into()),
        });
        i += 1.0;
    }
    let mut welds = Vec::new();
    let mut i = 1.0;
    while i <= treads && i <= MAX_TREADS {
        for sg in ["stringerL", "stringerR"] {
            welds.push(StairWeld {
                between: [sg.to_string(), format!("tread{}", crate::js::num_to_string(i))],
                size: min(6.0, s.tread_t),
                length: 2.0 * depth,
                r#where: t0("stair.weld.where"),
            });
        }
        i += 1.0;
    }
    if r > 178.0 + 1e-6 || r < 102.0 - 1e-6 {
        problems.push(StairProblem { code: "riser", text: "stair.prob.riser", fills: vec![("r", F::N(round(r * 10.0) / 10.0))] });
    }
    if g + 1e-6 < 279.0 {
        problems.push(StairProblem { code: "going", text: "stair.prob.going", fills: vec![("g", F::N(g))] });
    }
    let bl = 2.0 * r + g;
    if bl < 600.0 || bl > 650.0 {
        problems.push(StairProblem { code: "blondel", text: "stair.prob.blondel", fills: vec![("v", F::N(round(bl)))] });
    }
    StairGeo { n, riser: r, treads, going: g, run, pitch, blondel: bl, parts, welds, problems }
}

/// `String(x)` of a value for the error fills elsewhere.
pub fn text(v: Option<&Value>) -> String {
    string_of(v)
}
