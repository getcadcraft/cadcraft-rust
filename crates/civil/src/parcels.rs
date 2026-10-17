//! parcels.js — parcels (0.4 V21): areas, bearing and distance labels, closure of deed calls, a legal description.
//! A corner's `b` is the bulge of the course that starts there (DXF style: tan(Δ/4), + = curving left).

use crate::geo::P2;
use crate::i18n::t;
use crate::js::{J, JsMath, Obj, exec, hypot, is_integer, replace, sign, slice16, str_to_number, strip_controls, test, trim, upper};
use crate::units::{bearing, dms, fixed, grouped, unit_of};
use crate::{re, tr};
use serde_json::Value;
use std::f64::consts::PI;

const TAU: f64 = PI * 2.0;
fn wrap(a: f64) -> f64 {
    let a = a % TAU;
    if a < 0.0 { a + TAU } else { a }
}

/// A parcel corner `{ x, y, b? }`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Corner {
    pub x: f64,
    pub y: f64,
    pub b: Option<f64>,
}
impl Corner {
    /// `p.b` truthy.
    pub fn bulge(&self) -> Option<f64> {
        self.b.filter(|b| *b != 0.0 && !b.is_nan())
    }
}
impl J for Corner {
    fn j(&self) -> Value {
        Obj::new().set("x", self.x).set("y", self.y).opt("b", self.b).done()
    }
}
impl crate::geo::XY for Corner {
    fn x(&self) -> f64 {
        self.x
    }
    fn y(&self) -> f64 {
        self.y
    }
}
crate::und_default!(Corner);

/// A parcel.
#[derive(Clone, Debug, PartialEq)]
pub struct Parcel {
    pub id: Option<String>,
    pub name: String,
    pub pts: Vec<Corner>,
    pub pob: Option<String>,
}
impl J for Parcel {
    fn j(&self) -> Value {
        Obj::new().set("id", &self.id).set("name", &self.name).set("pts", &self.pts).opt("pob", self.pob.as_ref()).done()
    }
}
fn read_corner(p: &Value) -> Corner {
    Corner {
        x: crate::js::numf(p.get("x")),
        y: crate::js::numf(p.get("y")),
        b: p.get("b").filter(|b| !b.is_null()).map(|b| crate::js::numf(Some(b))),
    }
}
/// A parcel as given (trusted).
pub fn read_parcel(o: &Value) -> Parcel {
    Parcel {
        id: o.get("id").and_then(Value::as_str).map(str::to_string),
        name: crate::js::string_or_empty(o.get("name")),
        pts: crate::js::arr(o.get("pts")).iter().map(read_corner).collect(),
        pob: o.get("pob").and_then(Value::as_str).map(str::to_string),
    }
}
pub fn read_corners(v: Option<&Value>) -> Vec<Corner> {
    crate::js::arr(v).iter().map(read_corner).collect()
}

fn fin(v: Option<&Value>) -> Option<f64> {
    crate::js::fin(v)
}
/// `cleanParcel(o)` (untrusted).
pub fn clean_parcel(o: &Value) -> Option<Parcel> {
    if !o.is_object() {
        return None;
    }
    let pts_v = o.get("pts")?.as_array()?;
    let pts: Vec<Corner> = pts_v
        .iter()
        .filter_map(|p| {
            let x = fin(p.get("x"))?;
            let y = fin(p.get("y"))?;
            (x.abs() < 1e9 && y.abs() < 1e9).then_some((x, y, p))
        })
        .take(5000)
        .map(|(x, y, p)| {
            let b = fin(p.get("b")).filter(|&b| b != 0.0 && b.abs() <= 1.0);
            Corner { x, y, b }
        })
        .collect();
    if pts.len() < 2 || (pts.len() == 2 && !pts.iter().any(|p| p.bulge().is_some())) {
        return None;
    }
    let pob = slice16(trim(&strip_controls(&crate::js::string_or_empty(o.get("pob")), " ")), 200);
    let id_s = crate::js::string_of(o.get("id"));
    let id_ok = id_s.len() >= 2
        && id_s.len() <= 10
        && id_s.starts_with('r')
        && id_s.get(1..).is_some_and(|d| d.bytes().all(|b| b.is_ascii_digit()));
    let name = slice16(trim(&strip_controls(&crate::js::string_or_empty(o.get("name")), " ")), 120);
    Some(Parcel {
        id: id_ok.then_some(id_s),
        name: if name.is_empty() { "Parcel".into() } else { name },
        pts,
        pob: (!pob.is_empty()).then_some(pob),
    })
}

// ------------------------------------------------------------------ bearings

/// The numbers `\d+(?:\.\d+)?` in a text.
fn numbers(s: &str) -> Vec<f64> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        if b.get(i).is_some_and(u8::is_ascii_digit) {
            let st = i;
            while b.get(i).is_some_and(u8::is_ascii_digit) {
                i = i.saturating_add(1);
            }
            if b.get(i) == Some(&b'.') && b.get(i.saturating_add(1)).is_some_and(u8::is_ascii_digit) {
                i = i.saturating_add(1);
                while b.get(i).is_some_and(u8::is_ascii_digit) {
                    i = i.saturating_add(1);
                }
            }
            out.push(str_to_number(s.get(st..i).unwrap_or("")));
        } else {
            i = i.saturating_add(1);
        }
    }
    out
}

/// `parseAngle(x)` → degrees | None.
pub fn parse_angle(x: &str) -> Option<f64> {
    let n = numbers(x);
    if n.is_empty() || n.len() > 3 {
        return None;
    }
    let d = n.first().copied().unwrap_or(0.0);
    let m = n.get(1).copied().unwrap_or(0.0);
    let sec = n.get(2).copied().unwrap_or(0.0);
    if m >= 60.0 || sec >= 60.0 || (n.len() > 1 && !is_integer(d)) || (n.len() > 2 && !is_integer(m)) {
        return None;
    }
    Some(d + m / 60.0 + sec / 3600.0)
}

/// `parseBearing(s)` → direction (radians, 0 = east, CCW) | None.
pub fn parse_bearing(s: &str) -> Option<f64> {
    let s0 = upper(trim(s));
    let s1: String = s0
        .chars()
        .map(|c| match c {
            '’' | '′' => '\'',
            '”' | '″' => '"',
            'º' => '°',
            _ => c,
        })
        .collect();
    let s = s1.as_str();
    if s.is_empty() || s.encode_utf16().count() > 60 {
        return None;
    }
    if let Some(az) = exec(re!(r"^AZ(?:IMUTH)?\s*(.+)$"), s) {
        let a = parse_angle(az.get(1).cloned().flatten().as_deref().unwrap_or(""))?;
        return if a > 360.0 { None } else { Some(wrap(((90.0 - a) * PI) / 180.0)) };
    }
    let m = exec(re!(r"^([NS])\s*(.*?)\s*([EW])$"), s)?;
    let g = |i: usize| m.get(i).cloned().flatten().unwrap_or_default();
    let a = parse_angle(&g(2))?;
    if a > 90.0 {
        return None;
    }
    let azd = if g(1) == "N" {
        if g(3) == "E" { a } else { 360.0 - a }
    } else if g(3) == "E" {
        180.0 - a
    } else {
        180.0 + a
    };
    Some(wrap(((90.0 - azd) * PI) / 180.0))
}

// ------------------------------------------------------------------ deed calls

/// A deed call.
#[derive(Clone, Copy, Debug)]
pub enum Call {
    Line { dir: f64, dist: f64 },
    Curve { r: f64, delta: f64, turn: f64, chord_dir: Option<f64> },
}
impl J for Call {
    fn j(&self) -> Value {
        match self {
            Call::Line { dir, dist } => crate::jo! { "kind" => "line", "dir" => *dir, "dist" => *dist },
            Call::Curve { r, delta, turn, chord_dir } => {
                crate::jo! { "kind" => "curve", "R" => *r, "delta" => *delta, "turn" => *turn, "chordDir" => *chord_dir }
            }
        }
    }
}
pub fn read_call(v: &Value) -> Call {
    let n = |k: &str| crate::js::numf(v.get(k));
    if v.get("kind").and_then(Value::as_str) == Some("curve") {
        Call::Curve {
            r: n("R"),
            delta: n("delta"),
            turn: n("turn"),
            chord_dir: v.get("chordDir").filter(|c| !c.is_null()).map(|c| crate::js::numf(Some(c))),
        }
    } else {
        Call::Line { dir: n("dir"), dist: n("dist") }
    }
}
/// A line error `{ line, msg }`.
#[derive(Clone, Debug)]
pub struct LineErr {
    pub line: usize,
    pub msg: String,
}
impl J for LineErr {
    fn j(&self) -> Value {
        crate::jo! { "line" => self.line, "msg" => self.msg }
    }
}

/// `text.split(/\r?\n/)`.
pub fn split_crlf(s: &str) -> Vec<&str> {
    s.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect()
}

/// `parseCalls(text)` → (calls, errors).
pub fn parse_calls(text: &str) -> (Vec<Call>, Vec<LineErr>) {
    let mut calls = Vec::new();
    let mut errors = Vec::new();
    for (i, raw) in split_crlf(text).into_iter().take(2000).enumerate() {
        let l1 = replace(re!(r"^\s*(?:thence|then)\b[\s,]*", i), raw, "", false);
        let l2 = replace(re!(r"\b(?:feet|foot|ft|meters?|metres?|m)\b\.?", i), &l1, " ", true);
        let l3: String = l2.chars().map(|c| if c == ';' || c == ',' { ' ' } else { c }).collect();
        let line = trim(&l3).to_string();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let n = i.saturating_add(1);
        if test(re!(r"^(?:C|CURVE)\b", i), &line) {
            let rest = replace(re!(r"^(?:C|CURVE)\b", i), &line, " ", false);
            let num = |m: Option<Vec<Option<String>>>| m.and_then(|g| g.get(1).cloned().flatten()).map(|s| str_to_number(&s));
            let r = num(exec(re!(r"\bR(?:ADIUS)?\s*=?\s*(\d+(?:\.\d+)?)", i), &rest));
            let l = num(exec(re!(r"\bL(?:ENGTH)?\s*=?\s*(\d+(?:\.\d+)?)", i), &rest));
            let dm = exec(re!(r#"\b(?:D|DELTA|Δ)\s*=?\s*([0-9°'".\-\s]+?)(?=\s+(?:LEFT|RIGHT|CB|R\b|L\b)|\s*$)"#, i), &rest);
            let turn = if test(re!(r"\bLEFT\b", i), &rest) {
                1.0
            } else if test(re!(r"\bRIGHT\b", i), &rest) {
                -1.0
            } else {
                0.0
            };
            let cb = exec(re!(r"\bCB\s*(.+?)\s*$", i), &rest);
            let mut delta: Option<f64> = None;
            if let Some(dm) = dm {
                delta = parse_angle(dm.get(1).cloned().flatten().as_deref().unwrap_or("")).map(|a| (a * PI) / 180.0);
            }
            let Some(r) = r.filter(|&r| r > 0.0) else {
                errors.push(LineErr { line: n, msg: t("pc.err.radius", &[]) });
                continue;
            };
            if turn == 0.0 {
                errors.push(LineErr { line: n, msg: t("pc.err.turn", &[]) });
                continue;
            }
            if let Some(l) = l.filter(|&l| l > 0.0) {
                delta = Some(l / r);
            }
            let Some(delta) = delta.filter(|&d| d > 0.0 && d < TAU) else {
                errors.push(LineErr { line: n, msg: t("pc.err.delta", &[]) });
                continue;
            };
            let mut chord_dir = None;
            if let Some(cb) = cb {
                chord_dir = parse_bearing(cb.get(1).cloned().flatten().as_deref().unwrap_or(""));
                if chord_dir.is_none() {
                    errors.push(LineErr { line: n, msg: t("pc.err.bearing", &[]) });
                    continue;
                }
            }
            calls.push(Call::Curve { r, delta, turn, chord_dir });
            continue;
        }
        let m = exec(re!(r"^(.*?[EW])\s+(\d+(?:\.\d+)?)\s*'?\s*$", i), &line)
            .or_else(|| exec(re!(r#"^(AZ(?:IMUTH)?\s*[0-9°'".\-\s]+?)\s+(\d+(?:\.\d+)?)\s*'?\s*$"#, i), &line));
        let Some(m) = m else {
            errors.push(LineErr { line: n, msg: t("pc.err.read", &[]) });
            continue;
        };
        let g = |k: usize| m.get(k).cloned().flatten().unwrap_or_default();
        let dir = parse_bearing(&g(1));
        let dist = str_to_number(&g(2));
        let Some(dir) = dir else {
            errors.push(LineErr { line: n, msg: t("pc.err.bearing", &[]) });
            continue;
        };
        if !(dist > 0.0) {
            errors.push(LineErr { line: n, msg: t("pc.err.dist", &[]) });
            continue;
        }
        calls.push(Call::Line { dir, dist });
    }
    (calls, errors)
}

/// `runCalls(start, calls)` result.
#[derive(Clone, Debug)]
pub struct Run {
    pub pts: Vec<Corner>,
    pub dx: f64,
    pub dy: f64,
    pub dist: f64,
    pub dir: f64,
    pub length: f64,
    pub precision: f64,
}
impl J for Run {
    fn j(&self) -> Value {
        crate::jo! {
            "pts" => self.pts, "misclosure" => crate::jo! { "dx" => self.dx, "dy" => self.dy, "dist" => self.dist, "dir" => self.dir },
            "length" => self.length, "precision" => self.precision,
        }
    }
}
/// `runCalls(start, calls)`.
pub fn run_calls(start: P2, calls: &[Call]) -> Run {
    let mut pts = vec![Corner { x: start.x, y: start.y, b: None }];
    let mut p = start;
    let mut heading: Option<f64> = None;
    let mut length = 0.0;
    for c in calls {
        match *c {
            Call::Line { dir, dist } => {
                p = P2 { x: p.x + dist * dir.jcos(), y: p.y + dist * dir.jsin() };
                heading = Some(dir);
                length += dist;
                pts.push(Corner { x: p.x, y: p.y, b: None });
            }
            Call::Curve { r, delta, turn, chord_dir } => {
                let cd = chord_dir.unwrap_or_else(|| heading.map_or(0.0, |h| h + (turn * delta) / 2.0));
                let chord = 2.0 * r * (delta / 2.0).jsin();
                if let Some(last) = pts.last_mut() {
                    last.b = Some(turn * (delta / 4.0).jtan());
                }
                p = P2 { x: p.x + chord * cd.jcos(), y: p.y + chord * cd.jsin() };
                heading = Some(cd + (turn * delta) / 2.0);
                length += r * delta;
                pts.push(Corner { x: p.x, y: p.y, b: None });
            }
        }
    }
    let dx = p.x - start.x;
    let dy = p.y - start.y;
    let dist = hypot(dx, dy);
    Run { pts, dx, dy, dist, dir: dy.jatan2(dx), length, precision: if dist > 1e-9 { length / dist } else { f64::INFINITY } }
}

// ------------------------------------------------------------------ courses, area

/// An arc course's facts.
#[derive(Clone, Copy, Debug)]
pub struct ArcFacts {
    pub r: f64,
    pub l: f64,
    pub delta: f64,
    pub turn: f64,
    pub chord_dir: f64,
    pub chord: f64,
    pub dir_in: f64,
    pub dir_out: f64,
    pub c: P2,
}
fn arc_of(a: &Corner, b: &Corner, bulge: f64) -> ArcFacts {
    let chord = hypot(b.x - a.x, b.y - a.y);
    let delta = 4.0 * bulge.abs().jatan();
    let turn = if bulge > 0.0 { 1.0 } else { -1.0 };
    let r = chord / (2.0 * (delta / 2.0).jsin());
    let chord_dir = (b.y - a.y).jatan2(b.x - a.x);
    let dir_in = chord_dir - (turn * delta) / 2.0;
    let dir_out = chord_dir + (turn * delta) / 2.0;
    let to_c = dir_in + (turn * PI) / 2.0;
    ArcFacts {
        r,
        l: r * delta,
        delta,
        turn,
        chord_dir,
        chord,
        dir_in,
        dir_out,
        c: P2 { x: a.x + r * to_c.jcos(), y: a.y + r * to_c.jsin() },
    }
}
/// A course.
#[derive(Clone, Copy, Debug)]
pub enum Course {
    Line { from: Corner, to: Corner, dir: f64, dist: f64 },
    Curve { from: Corner, to: Corner, a: ArcFacts },
}
impl J for Course {
    fn j(&self) -> Value {
        match self {
            Course::Line { from, to, dir, dist } => {
                crate::jo! { "kind" => "line", "from" => *from, "to" => *to, "dir" => *dir, "dist" => *dist }
            }
            Course::Curve { from, to, a } => Obj::new()
                .set("kind", "curve")
                .set("from", *from)
                .set("to", *to)
                .set("R", a.r)
                .set("L", a.l)
                .set("delta", a.delta)
                .set("turn", a.turn)
                .set("chordDir", a.chord_dir)
                .set("chord", a.chord)
                .set("dirIn", a.dir_in)
                .set("dirOut", a.dir_out)
                .set("c", a.c)
                .done(),
        }
    }
}
/// `courses(pts)`.
pub fn courses(pts: &[Corner]) -> Vec<Course> {
    let n = pts.len();
    (0..n)
        .map(|i| {
            let a = pts.get(i).copied().unwrap_or_default();
            let b = pts.get((i + 1) % n).copied().unwrap_or_default();
            match a.bulge() {
                Some(bu) => Course::Curve { from: a, to: b, a: arc_of(&a, &b, bu) },
                None => Course::Line { from: a, to: b, dir: (b.y - a.y).jatan2(b.x - a.x), dist: hypot(b.x - a.x, b.y - a.y) },
            }
        })
        .collect()
}
/// `parcelArea(pts)` (curves exact).
pub fn parcel_area(pts: &[Corner]) -> f64 {
    let n = pts.len();
    let mut a = 0.0;
    for i in 0..n {
        let p = pts.get(i).copied().unwrap_or_default();
        let q = pts.get((i + 1) % n).copied().unwrap_or_default();
        a += p.x * q.y - q.x * p.y;
        if let Some(b) = p.bulge() {
            let c = arc_of(&p, &q, b);
            a += 2.0 * sign(b) * ((c.r * c.r) / 2.0) * (c.delta - c.delta.jsin());
        }
    }
    (a / 2.0).abs()
}
/// `perimeter(pts)`.
pub fn perimeter(pts: &[Corner]) -> f64 {
    courses(pts).iter().fold(0.0, |m, c| {
        m + match c {
            Course::Line { dist, .. } => *dist,
            Course::Curve { a, .. } => a.l,
        }
    })
}

// ------------------------------------------------------------------ the legal description

fn len_word(units: &str) -> String {
    t(if units == "m" { "pc.lw.m" } else { "pc.lw.ft" }, &[])
}
/// `areaWords(a, units)`.
pub fn area_words(a: f64, units: &str) -> String {
    if units == "m" {
        tr!("pc.area.m", a = grouped(a, 0), ha = fixed(a / 10000.0, 4))
    } else {
        tr!("pc.area.ft", a = grouped(a, 0), ac = fixed(a / 43560.0, 3))
    }
}
/// `legalDescription(parcel, { units, pob })`.
pub fn legal_description(parcel: &Parcel, units: &str, pob: Option<&str>) -> String {
    let pts = &parcel.pts;
    let cs = courses(pts);
    let lw = len_word(units);
    let d = |v: f64| format!("{} {}", fixed(v, 2), lw);
    let p0 = pts.first().copied().unwrap_or(Corner { x: f64::NAN, y: f64::NAN, b: None });
    let start = match pob.filter(|p| !p.is_empty()) {
        Some(p) => p.to_string(),
        None => tr!("pc.pob.at", n = fixed(p0.y, 3), e = fixed(p0.x, 3)),
    };
    let nc = cs.len();
    let mut lines = vec![parcel.name.clone(), String::new(), format!("{};", tr!("pc.begin", at = start))];
    for (i, c) in cs.iter().enumerate() {
        let last = i + 1 == nc;
        let s = match c {
            Course::Line { dir, dist, .. } => tr!("pc.call.line", b = bearing(*dir), d = d(*dist)),
            Course::Curve { a, .. } => tr!(
                if a.turn > 0.0 { "pc.call.left" } else { "pc.call.right" },
                r = d(a.r),
                l = d(a.l),
                delta = dms((a.delta * 180.0) / PI),
                cb = bearing(a.chord_dir),
                cd = d(a.chord)
            ),
        };
        lines.push(format!("{}{}", tr!("pc.thence", call = s), if last { format!(" {}.", t("pc.to.pob", &[])) } else { ";".into() }));
    }
    lines.push(String::new());
    lines.push(tr!("pc.containing", area = area_words(parcel_area(pts), units)));
    lines.join("\n")
}

// ------------------------------------------------------------------ plan labels

fn signed_area(pts: &[Corner]) -> f64 {
    let n = pts.len();
    let mut a = 0.0;
    for i in 0..n {
        let p = pts.get(i).copied().unwrap_or_default();
        let q = pts.get((i + 1) % n).copied().unwrap_or_default();
        a += p.x * q.y - q.x * p.y;
    }
    a / 2.0
}
/// `centroid(pts)`.
pub fn centroid(pts: &[Corner]) -> P2 {
    let a = signed_area(pts);
    let n = pts.len();
    if a.abs() < 1e-12 {
        return P2 { x: pts.iter().fold(0.0, |m, p| m + p.x) / n as f64, y: pts.iter().fold(0.0, |m, p| m + p.y) / n as f64 };
    }
    let (mut cx, mut cy) = (0.0, 0.0);
    for i in 0..n {
        let p = pts.get(i).copied().unwrap_or_default();
        let q = pts.get((i + 1) % n).copied().unwrap_or_default();
        let k = p.x * q.y - q.x * p.y;
        cx += (p.x + q.x) * k;
        cy += (p.y + q.y) * k;
    }
    P2 { x: cx / (6.0 * a), y: cy / (6.0 * a) }
}

/// `parcelLabels(parcel, { units, unitPerMm })` → scene items (JSON, as scene.js reads them).
pub fn parcel_labels(parcel: &Parcel, units: &str, unit_per_mm: f64) -> Vec<Value> {
    let mut items = Vec::new();
    let pts = &parcel.pts;
    let cs = courses(pts);
    let u = unit_of(units);
    let ft = units != "m";
    let len = |v: f64| format!("{}{}", fixed(v, 2), if ft { "'" } else { " m" });
    let ccw = signed_area(pts) > 0.0;
    for c in &cs {
        match c {
            Course::Line { from, to, .. } => items.push(crate::jo! { "layer" => "V-PROP", "kind" => "poly", "pts" => vec![*from, *to] }),
            Course::Curve { from, to, a } => {
                let a0 = (from.y - a.c.y).jatan2(from.x - a.c.x);
                let a1 = (to.y - a.c.y).jatan2(to.x - a.c.x);
                items.push(crate::jo! { "layer" => "V-PROP", "kind" => "arc", "c" => a.c, "r" => a.r, "a0" => if a.turn > 0.0 { a0 } else { a1 }, "a1" => if a.turn > 0.0 { a1 } else { a0 } });
            }
        }
        let (mid, dir) = match c {
            Course::Line { from, to, dir, .. } => (P2 { x: (from.x + to.x) / 2.0, y: (from.y + to.y) / 2.0 }, *dir),
            Course::Curve { from, a, .. } => {
                let am = (from.y - a.c.y).jatan2(from.x - a.c.x) + (a.turn * a.delta) / 2.0;
                (P2 { x: a.c.x + a.r * am.jcos(), y: a.c.y + a.r * am.jsin() }, am + (a.turn * PI) / 2.0)
            }
        };
        let out = dir + if ccw { -PI / 2.0 } else { PI / 2.0 };
        let off = 1.6 * unit_per_mm;
        let mut rot = dir;
        if rot.jcos() < -1e-9 || (rot.jcos().abs() <= 1e-9 && rot.jsin() < 0.0) {
            rot += PI;
        }
        let text = match c {
            Course::Line { dir, dist, .. } => format!("{}  {}", bearing(*dir), len(*dist)),
            Course::Curve { a, .. } => format!("R={}  L={}  Delta={}", len(a.r), len(a.l), dms((a.delta * 180.0) / PI)),
        };
        items.push(crate::jo! {
            "layer" => "V-PROP-TEXT", "kind" => "text", "x" => mid.x + out.jcos() * off, "y" => mid.y + out.jsin() * off, "rot" => rot,
            "mm" => 1.6, "align" => "center", "middle" => true, "text" => text,
        });
    }
    let ctr = centroid(pts);
    let a = parcel_area(pts);
    items.push(crate::jo! {
        "layer" => "V-PROP-TEXT", "kind" => "text", "x" => ctr.x, "y" => ctr.y + 1.6 * unit_per_mm, "rot" => 0.0, "mm" => 2.2,
        "align" => "center", "middle" => true, "text" => parcel.name, "mask" => true,
    });
    let area_t = format!(
        "{} {}  {}",
        grouped(a, 0),
        u.area,
        if ft { format!("{} ac", fixed(a / 43560.0, 3)) } else { format!("{} ha", fixed(a / 10000.0, 4)) }
    );
    items.push(crate::jo! {
        "layer" => "V-PROP-TEXT", "kind" => "text", "x" => ctr.x, "y" => ctr.y - 1.6 * unit_per_mm, "rot" => 0.0, "mm" => 1.8,
        "align" => "center", "middle" => true, "text" => area_t, "mask" => true,
    });
    items
}
