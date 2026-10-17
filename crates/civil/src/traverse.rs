//! traverse.js — a traverse from field angles or bearings and distances, its closure and the compass (Bowditch)
//! adjustment (0.4 V22).

use crate::geo::P2;
use crate::i18n::t;
use crate::js::{J, JsMath, Obj, exec, hypot, num_to_string, replace, round, slice16, str_to_number, strip_controls, trim};
use crate::parcels::{LineErr, parse_angle, parse_bearing, split_crlf};
use crate::points::split_ws;
use crate::units::{bearing, dms, fixed};
use crate::{re, tr};
use serde_json::Value;
use std::f64::consts::PI;

const D2R: f64 = PI / 180.0;
fn az2dir(az: f64) -> f64 {
    (90.0 - az) * D2R
}
/// `dir2az(d)`.
pub fn dir2az(d: f64) -> f64 {
    let mut a = 90.0 - d / D2R;
    a %= 360.0;
    if a < 0.0 { a + 360.0 } else { a }
}

/// A leg: an angle right (degrees, angles mode) or a direction (radians, bearings mode), a distance, a name.
#[derive(Clone, Debug)]
pub struct Leg {
    pub angle: Option<f64>,
    pub dir: Option<f64>,
    pub dist: f64,
    pub name: String,
}
impl J for Leg {
    fn j(&self) -> Value {
        Obj::new().opt("dir", self.dir).opt("angle", self.angle).set("dist", self.dist).set("name", &self.name).done()
    }
}
pub fn read_leg(v: &Value) -> Leg {
    Leg {
        angle: v.get("angle").filter(|x| !x.is_null()).map(|x| crate::js::numf(Some(x))),
        dir: v.get("dir").filter(|x| !x.is_null()).map(|x| crate::js::numf(Some(x))),
        dist: crate::js::numf(v.get("dist")),
        name: crate::js::string_or_empty(v.get("name")),
    }
}

/// `parseTraverse(text, mode)` → (legs, errors, closingAngle).
pub fn parse_traverse(text: &str, mode: &str) -> (Vec<Leg>, Vec<LineErr>, Option<f64>) {
    let mut legs = Vec::new();
    let mut errors = Vec::new();
    let mut closing = None;
    for (i, raw) in split_crlf(text).into_iter().take(2000).enumerate() {
        let l1 = replace(re!(r"\b(?:feet|foot|ft|meters?|metres?)\b\.?", i), raw, " ", true);
        let l2: String = l1.chars().map(|c| if c == ';' || c == ',' { ' ' } else { c }).collect();
        let line = trim(&l2).to_string();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let n = i.saturating_add(1);
        if let Some(cl) = exec(re!(r"^close\s+(.+)$", i), &line) {
            if mode != "angles" {
                errors.push(LineErr { line: n, msg: t("tv.err.closebearing", &[]) });
                continue;
            }
            match parse_angle(cl.get(1).cloned().flatten().as_deref().unwrap_or("")) {
                Some(a) if a < 360.0 => closing = Some(a),
                _ => errors.push(LineErr { line: n, msg: t("tv.err.angle", &[]) }),
            }
            continue;
        }
        let ang: f64;
        let rest: String;
        if mode == "bearings" {
            let Some(m) = exec(re!(r"^(.*?[EW]|AZ(?:IMUTH)?\s*\S+)\s+(.+)$", i), &line) else {
                errors.push(LineErr { line: n, msg: t("tv.err.read", &[]) });
                continue;
            };
            let Some(a) = parse_bearing(m.get(1).cloned().flatten().as_deref().unwrap_or("")) else {
                errors.push(LineErr { line: n, msg: t("tv.err.bearing", &[]) });
                continue;
            };
            ang = a;
            rest = m.get(2).cloned().flatten().unwrap_or_default();
        } else {
            // the angle: one token or three numbers before the distance
            let tok = split_ws(&line);
            let is_num = |x: &str| {
                let (a, b) = x.split_once('.').map_or((x, None), |(a, b)| (a, Some(b)));
                !a.is_empty()
                    && a.bytes().all(|c| c.is_ascii_digit())
                    && b.is_none_or(|b| !b.is_empty() && b.bytes().all(|c| c.is_ascii_digit()))
            };
            let three = if tok.len() >= 4 && tok.iter().take(3).all(|x| is_num(x)) { 3 } else { 1 };
            let a = parse_angle(&tok.iter().take(three).cloned().collect::<Vec<_>>().join(" "));
            rest = tok.iter().skip(three).cloned().collect::<Vec<_>>().join(" ");
            match a {
                Some(a) if a < 360.0 => ang = a,
                _ => {
                    errors.push(LineErr { line: n, msg: t("tv.err.angle", &[]) });
                    continue;
                }
            }
        }
        let m2 = exec(re!(r"^(\d+(?:\.\d+)?)\s*'?\s*(.*)$"), trim(&rest));
        let dist = m2.as_ref().and_then(|m| m.get(1).cloned().flatten()).map_or(f64::NAN, |d| str_to_number(&d));
        let Some(m2) = m2.filter(|_| dist > 0.0) else {
            errors.push(LineErr { line: n, msg: t("tv.err.dist", &[]) });
            continue;
        };
        let name = slice16(trim(&strip_controls(m2.get(2).cloned().flatten().as_deref().unwrap_or(""), " ")), 40);
        legs.push(if mode == "bearings" {
            Leg { angle: None, dir: Some(ang), dist, name }
        } else {
            Leg { angle: Some(ang), dir: None, dist, name }
        });
    }
    (legs, errors, closing)
}

/// A traverse station.
#[derive(Clone, Debug)]
pub struct Station {
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub dir: f64,
    pub dist: f64,
    pub run: f64,
    pub cx: f64,
    pub cy: f64,
    pub ax: f64,
    pub ay: f64,
}
impl J for Station {
    fn j(&self) -> Value {
        crate::jo! {
            "name" => self.name, "x" => self.x, "y" => self.y, "dir" => self.dir, "dist" => self.dist, "run" => self.run,
            "cx" => self.cx, "cy" => self.cy, "ax" => self.ax, "ay" => self.ay,
        }
    }
}
/// `runTraverse(…)` result.
#[derive(Clone, Debug)]
pub struct Traverse {
    pub stations: Vec<Station>,
    pub misclosure: Option<f64>,
    pub per_angle: f64,
    pub dx: f64,
    pub dy: f64,
    pub dist: f64,
    pub dir: f64,
    pub length: f64,
    pub precision: f64,
    pub problems: Vec<String>,
    pub start: P2,
    /// None = 'loop'
    pub close: Option<P2>,
}
impl J for Traverse {
    fn j(&self) -> Value {
        crate::jo! {
            "stations" => self.stations,
            "angular" => crate::jo! { "misclosure" => self.misclosure, "perAngle" => self.per_angle },
            "linear" => crate::jo! { "dx" => self.dx, "dy" => self.dy, "dist" => self.dist, "dir" => self.dir },
            "length" => self.length, "precision" => self.precision, "problems" => self.problems, "start" => self.start,
            "close" => match self.close { Some(p) => p.j(), None => Value::String("loop".into()) },
        }
    }
}

/// `runTraverse({ start, mode, backsight, legs, close, closingAngle })`.
pub fn run_traverse(start: P2, mode: &str, backsight: f64, legs: &[Leg], close: Option<P2>, closing_angle: Option<f64>) -> Traverse {
    let mut problems = Vec::new();
    let n = legs.len();
    let mut misclosure = None;
    let mut per_angle = 0.0;
    let dirs: Vec<f64> = if mode == "angles" {
        let a: Vec<f64> = legs.iter().map(|l| l.angle.unwrap_or(f64::NAN)).collect();
        let ag = |i: usize| a.get(i).copied().unwrap_or(f64::NAN);
        if close.is_none()
            && let Some(ca) = closing_angle
            && n >= 2
        {
            let mut az = backsight + ag(0);
            for i in 1..n {
                az += 180.0 + ag(i);
            }
            let again = az + 180.0 + ca;
            let first = backsight + ag(0);
            let mis = ((((again - first) % 360.0) + 540.0) % 360.0) - 180.0;
            misclosure = Some(mis);
            per_angle = -mis / n as f64;
        }
        let mut dirs = Vec::with_capacity(n);
        let mut az = backsight + ag(0);
        dirs.push(az2dir(az));
        for i in 1..n {
            az += 180.0 + ag(i) + per_angle;
            dirs.push(az2dir(az));
        }
        dirs
    } else {
        legs.iter().map(|l| l.dir.unwrap_or(f64::NAN)).collect()
    };
    let mut st: Vec<Station> = Vec::new();
    let (mut x, mut y, mut length) = (start.x, start.y, 0.0);
    for (i, l) in legs.iter().enumerate() {
        let d = dirs.get(i).copied().unwrap_or(f64::NAN);
        x += l.dist * d.jcos();
        y += l.dist * d.jsin();
        length += l.dist;
        st.push(Station { name: l.name.clone(), x, y, dir: d, dist: l.dist, run: length, cx: 0.0, cy: 0.0, ax: 0.0, ay: 0.0 });
    }
    let end = close.unwrap_or(start);
    let dx = x - end.x;
    let dy = y - end.y;
    let dist = hypot(dx, dy);
    for s in &mut st {
        s.cx = -dx * (s.run / length);
        s.cy = -dy * (s.run / length);
        s.ax = s.x + s.cx;
        s.ay = s.y + s.cy;
    }
    if n < 2 {
        problems.push(t("tv.few", &[]));
    }
    Traverse {
        stations: st,
        misclosure,
        per_angle,
        dx,
        dy,
        dist,
        dir: dy.jatan2(dx),
        length,
        precision: if dist > 1e-12 { length / dist } else { f64::INFINITY },
        problems,
        start,
        close,
    }
}

/// `traverseReport(r, { units })` → CSV.
pub fn traverse_report(r: &Traverse, units: &str) -> String {
    let u = if units == "m" { "m" } else { "ft" };
    let q = |v: &str| -> String {
        let b = v.as_bytes();
        let num_start = b.first().is_some_and(u8::is_ascii_digit) || (b.first() == Some(&b'-') && b.get(1).is_some_and(u8::is_ascii_digit));
        if num_start {
            v.to_string()
        } else if matches!(b.first(), Some(b'=' | b'+' | b'-' | b'@' | b'\t' | b'\r')) {
            format!("'{v}")
        } else {
            v.to_string()
        }
    };
    let c = |v: String| if v.contains(['"', ',', '\r', '\n']) { format!("\"{}\"", v.replace('"', "\"\"")) } else { v };
    let mut rows: Vec<Vec<String>> = vec![
        vec![
            t("tv.r.angular", &[]),
            match r.misclosure {
                None => String::new(),
                Some(m) => format!("{}{}", if m < 0.0 { "-" } else { "" }, dms(m.abs())),
            },
        ],
        vec![tr!("tv.r.linear", u = u), fixed(r.dist, 4), bearing(r.dir)],
        vec![tr!("tv.r.length", u = u), fixed(r.length, 3)],
        vec![
            t("tv.r.precision", &[]),
            if r.precision.is_finite() { format!("1 : {}", num_to_string(round(r.precision))) } else { t("tv.r.exact", &[]) },
        ],
        vec![],
        ["tv.r.station", "tv.r.dir", "tv.r.dist", "tv.r.n", "tv.r.e", "tv.r.cn", "tv.r.ce", "tv.r.an", "tv.r.ae"]
            .iter()
            .map(|k| t(k, &[]))
            .collect(),
        vec![
            t("tv.r.start", &[]),
            String::new(),
            String::new(),
            fixed(r.start.y, 4),
            fixed(r.start.x, 4),
            "0.0000".into(),
            "0.0000".into(),
            fixed(r.start.y, 4),
            fixed(r.start.x, 4),
        ],
    ];
    for (i, s) in r.stations.iter().enumerate() {
        rows.push(vec![
            if s.name.is_empty() { (i + 1).to_string() } else { s.name.clone() },
            bearing(s.dir),
            fixed(s.dist, 3),
            fixed(s.y, 4),
            fixed(s.x, 4),
            fixed(s.cy, 4),
            fixed(s.cx, 4),
            fixed(s.ay, 4),
            fixed(s.ax, 4),
        ]);
    }
    let lines: Vec<String> = rows.into_iter().map(|r2| r2.iter().map(|v| c(q(v))).collect::<Vec<_>>().join(",")).collect();
    format!("{}\r\n", lines.join("\r\n"))
}
