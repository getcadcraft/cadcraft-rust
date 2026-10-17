//! rebar.js — reinforcing bars in a rectangular pad footing: bar sizes (ASTM A615 #3–#11, BS 4449 H8–H40), hooks
//! (ACI 318-19 Table 25.3.1 / BS 8666:2020), the bottom mat both ways and starter bars, cut lengths, bar marks and the
//! bending schedule. Millimetres.

use crate::geom::V3;
use crate::i18n::{F, t, t0};
use crate::js::{Obj, Res, atan2, cos, fixed_num, hypot, max, min, num, num_to_string, round, sin, string_of, to_fixed, truthy};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::f64::consts::PI;
use std::sync::OnceLock;

/// A bar size.
#[derive(Clone, Debug)]
pub struct Bar {
    pub name: String,
    pub num: f64,
    pub db: f64,
    pub mass: f64,
    pub area: f64,
    pub bend_d: f64,
    pub set: &'static str,
    pub mark_pre: String,
    pub metric: String,
    pub r: f64,
    pub pp: f64,
    pub pub_: Option<[f64; 3]>,
}

pub const BAR_SOURCE: &str = "ASTM A615/A615M Table 1";
pub const BAR_SOURCE_BS: &str = "BS 4449 B500, BS 8666:2020";

const A615: [(&str, f64, f64, f64); 9] = [
    ("#3", 0.375, 0.376, 0.11),
    ("#4", 0.500, 0.668, 0.20),
    ("#5", 0.625, 1.043, 0.31),
    ("#6", 0.750, 1.502, 0.44),
    ("#7", 0.875, 2.044, 0.60),
    ("#8", 1.000, 2.670, 0.79),
    ("#9", 1.128, 3.400, 1.00),
    ("#10", 1.270, 4.303, 1.27),
    ("#11", 1.410, 5.313, 1.56),
];
const BS8666: [[f64; 3]; 10] = [
    [6.0, 12.0, 110.0],
    [8.0, 16.0, 115.0],
    [10.0, 20.0, 120.0],
    [12.0, 24.0, 125.0],
    [16.0, 32.0, 130.0],
    [20.0, 70.0, 190.0],
    [25.0, 87.0, 240.0],
    [32.0, 112.0, 305.0],
    [40.0, 140.0, 380.0],
    [50.0, 175.0, 475.0],
];

/// `BARS` in insertion order.
pub fn bars() -> &'static [Bar] {
    static B: OnceLock<Vec<Bar>> = OnceLock::new();
    B.get_or_init(|| {
        let mut out = Vec::new();
        for (n, d, w, a) in A615 {
            let nn = crate::js::str_to_number(n.get(1..).unwrap_or(""));
            out.push(Bar {
                name: n.into(),
                num: nn,
                db: d * 25.4,
                mass: w * 1.48816394,
                area: a * 645.16,
                bend_d: (if nn <= 8.0 { 6.0 } else { 8.0 }) * d * 25.4,
                set: "astm",
                mark_pre: n.get(1..).unwrap_or("").into(),
                metric: format!("#{}", num_to_string(round(d * 25.4))),
                r: f64::NAN,
                pp: f64::NAN,
                pub_: Some([d, w, a]),
            });
        }
        for [d, r, p] in BS8666 {
            if d < 8.0 || d > 40.0 {
                continue;
            }
            let area = PI * d * d / 4.0;
            let name = format!("H{}", num_to_string(d));
            out.push(Bar {
                name: name.clone(),
                num: d,
                db: d,
                mass: area * 7850.0 * 1e-6,
                area,
                bend_d: 2.0 * r,
                set: "bs",
                mark_pre: name.clone(),
                metric: name,
                r,
                pp: p,
                pub_: None,
            });
        }
        out
    })
}
/// `BARS.get(name)`.
pub fn bar(name: &str) -> Option<&'static Bar> {
    static IDX: OnceLock<HashMap<String, usize>> = OnceLock::new();
    let idx = IDX.get_or_init(|| bars().iter().enumerate().map(|(i, b)| (b.name.clone(), i)).collect());
    idx.get(name).and_then(|&i| bars().get(i))
}
/// `BAR_SETS`.
pub fn bar_set(set: &str) -> Vec<String> {
    bars().iter().filter(|b| b.set == set).map(|b| b.name.clone()).collect()
}
/// `barLabel(name, naming)`.
pub fn bar_label(name: &str, naming: &str) -> String {
    match bar(name) {
        Some(b) if naming == "metric" => b.metric.clone(),
        _ => name.to_string(),
    }
}
/// `shapeCode(shape, standard)`.
pub fn shape_code(shape: &str, standard: &str) -> String {
    if standard == "bs8666" {
        return match shape {
            "S" => "00",
            "L" => "11",
            "U" => "21",
            x => x,
        }
        .to_string();
    }
    if standard == "aci" {
        return if shape == "U" { "2".into() } else { String::new() };
    }
    shape.to_string()
}

/// Out-to-out dims of a bar shape ({A}, {A, B}, {A, B, C}).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Dims {
    pub a: f64,
    pub b: Option<f64>,
    pub c: Option<f64>,
}
impl Dims {
    pub fn get(&self, k: &str) -> Option<f64> {
        match k {
            "A" => Some(self.a),
            "B" => self.b,
            "C" => self.c,
            _ => None,
        }
    }
    pub fn j(&self) -> Value {
        Obj::new().set("A", self.a).opt("B", self.b).opt("C", self.c).done()
    }
}
/// `scheduleDims(shape, dims, standard)`.
pub fn schedule_dims(shape: &str, dims: &Dims, standard: &str) -> (Option<f64>, Option<f64>, Option<f64>) {
    if (standard == "bs8666" || standard == "aci") && shape == "U" {
        return (dims.b, Some(dims.a), dims.c);
    }
    (Some(dims.a), dims.b, dims.c)
}

/// The bottom mat.
#[derive(Clone, Debug, PartialEq)]
pub struct Mat {
    pub bar_x: String,
    pub s_x: f64,
    pub bar_y: String,
    pub s_y: f64,
    pub cover: f64,
    pub hooks: bool,
}
/// Starter bars.
#[derive(Clone, Debug, PartialEq)]
pub struct Starters {
    pub on: bool,
    pub bar: String,
    pub nx: f64,
    pub ny: f64,
    pub sx: f64,
    pub sy: f64,
    pub projection: f64,
    pub hook: bool,
}
/// A footing as `cleanFooting` leaves it (plus the model's id and phase when stored).
#[derive(Clone, Debug, PartialEq)]
pub struct Footing {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub top: f64,
    pub l: f64,
    pub b: f64,
    pub h: f64,
    pub rotation: f64,
    pub concrete: String,
    pub mat: Mat,
    pub starters: Starters,
    pub phase: f64,
}

impl Mat {
    pub fn j(&self) -> Value {
        Obj::new()
            .set("barX", self.bar_x.as_str())
            .set("sX", self.s_x)
            .set("barY", self.bar_y.as_str())
            .set("sY", self.s_y)
            .set("cover", self.cover)
            .set("hooks", self.hooks)
            .done()
    }
}
impl Starters {
    pub fn j(&self) -> Value {
        Obj::new()
            .set("on", self.on)
            .set("bar", self.bar.as_str())
            .set("nx", self.nx)
            .set("ny", self.ny)
            .set("sx", self.sx)
            .set("sy", self.sy)
            .set("projection", self.projection)
            .set("hook", self.hook)
            .done()
    }
}
impl Footing {
    /// The fields `cleanFooting` returns, in its order.
    pub fn fields(&self) -> Map<String, Value> {
        let mut m = Map::new();
        for (k, v) in
            [("x", self.x), ("y", self.y), ("top", self.top), ("L", self.l), ("B", self.b), ("H", self.h), ("rotation", self.rotation)]
        {
            m.insert(k.into(), Value::from(v));
        }
        m.insert("concrete".into(), Value::from(self.concrete.clone()));
        m.insert("mat".into(), self.mat.j());
        m.insert("starters".into(), self.starters.j());
        m
    }
    /// A stored footing (validated by the model) read back.
    pub fn from_value(v: &Value) -> Footing {
        let g = |k: &str| crate::js::f(v.get(k));
        let m = v.get("mat");
        let st = v.get("starters");
        let gm = |k: &str| crate::js::f(m.and_then(|m| m.get(k)));
        let gs = |k: &str| crate::js::f(st.and_then(|m| m.get(k)));
        Footing {
            id: crate::js::s(v.get("id")).to_string(),
            x: g("x"),
            y: g("y"),
            top: g("top"),
            l: g("L"),
            b: g("B"),
            h: g("H"),
            rotation: g("rotation"),
            concrete: crate::js::s(v.get("concrete")).to_string(),
            mat: Mat {
                bar_x: string_of(m.and_then(|m| m.get("barX"))),
                s_x: gm("sX"),
                bar_y: string_of(m.and_then(|m| m.get("barY"))),
                s_y: gm("sY"),
                cover: gm("cover"),
                hooks: truthy(m.and_then(|m| m.get("hooks"))),
            },
            starters: Starters {
                on: truthy(st.and_then(|m| m.get("on"))),
                bar: string_of(st.and_then(|m| m.get("bar"))),
                nx: gs("nx"),
                ny: gs("ny"),
                sx: gs("sx"),
                sy: gs("sy"),
                projection: gs("projection"),
                hook: truthy(st.and_then(|m| m.get("hook"))),
            },
            phase: crate::js::or(crate::js::f(v.get("phase")), 1.0),
        }
    }
}

/// `num(v, lo, hi)`.
fn numr(v: Option<&Value>, lo: f64, hi: f64) -> Option<f64> {
    let n = num(v);
    if n.is_finite() && n >= lo && n <= hi { Some(n) } else { None }
}
fn field<'a>(o: Option<&'a Value>, k: &str) -> Option<&'a Value> {
    o.and_then(Value::as_object).and_then(|m| m.get(k))
}

/// `cleanFooting(f)` → the footing, or the error text.
pub fn clean_footing(f: &Value) -> Res<Footing> {
    if !(f.is_object() || f.is_array()) {
        return Err(t0("rebar.err.not-footing"));
    }
    let mut vals = [0.0f64; 7];
    let specs: [(&str, f64, f64); 7] = [
        ("x", -1e7, 1e7),
        ("y", -1e7, 1e7),
        ("top", -1e6, 1e6),
        ("L", 300.0, 50000.0),
        ("B", 300.0, 50000.0),
        ("H", 150.0, 10000.0),
        ("rotation", -360.0, 360.0),
    ];
    for (i, (k, lo, hi)) in specs.iter().enumerate() {
        match numr(field(Some(f), k), *lo, *hi) {
            Some(v) => {
                if let Some(slot) = vals.get_mut(i) {
                    *slot = v;
                }
            }
            None => {
                let key = match *k {
                    "L" => Some("rebar.err.length"),
                    "B" => Some("rebar.err.width"),
                    "H" => Some("rebar.err.depth"),
                    _ => None,
                };
                return Err(match key {
                    Some(key) => t(key, &[("lo", F::N(*lo)), ("hi", F::N(*hi))]),
                    None => t("rebar.err.position", &[("name", F::from(*k))]),
                });
            }
        }
    }
    let concrete = crate::js::clean_str(&crate::js::string_or_empty(field(Some(f), "concrete")), 30);
    let concrete = if concrete.is_empty() { "4000 psi".to_string() } else { concrete };
    // `f.mat || {}`: a truthy non-object gives no fields
    let m = field(Some(f), "mat").filter(|v| truthy(Some(v)));
    let s = field(Some(f), "starters").filter(|v| truthy(Some(v)));
    let mat = Mat {
        bar_x: string_of(field(m, "barX")),
        s_x: numr(field(m, "sX"), 50.0, 1000.0).unwrap_or(f64::NAN),
        bar_y: string_of(field(m, "barY")),
        s_y: numr(field(m, "sY"), 50.0, 1000.0).unwrap_or(f64::NAN),
        cover: numr(field(m, "cover"), 20.0, 300.0).unwrap_or(f64::NAN),
        hooks: truthy(field(m, "hooks")),
    };
    if bar(&mat.bar_x).is_none() || bar(&mat.bar_y).is_none() {
        return Err(t0("rebar.err.bar-size"));
    }
    if mat.s_x.is_nan() || mat.s_y.is_nan() {
        return Err(t0("rebar.err.spacing"));
    }
    if mat.cover.is_nan() {
        return Err(t0("rebar.err.cover"));
    }
    let mut st = Starters {
        on: truthy(field(s, "on")),
        bar: string_of(field(s, "bar")),
        nx: numr(field(s, "nx"), 1.0, 20.0).unwrap_or(f64::NAN),
        ny: numr(field(s, "ny"), 1.0, 20.0).unwrap_or(f64::NAN),
        sx: numr(field(s, "sx"), 50.0, 5000.0).unwrap_or(f64::NAN),
        sy: numr(field(s, "sy"), 50.0, 5000.0).unwrap_or(f64::NAN),
        projection: numr(field(s, "projection"), 0.0, 5000.0).unwrap_or(f64::NAN),
        hook: truthy(field(s, "hook")),
    };
    if bar(&st.bar).is_none() {
        return Err(t0("rebar.err.starter-size"));
    }
    if [st.nx, st.ny, st.sx, st.sy, st.projection].iter().any(|v| v.is_nan()) {
        return Err(t0("rebar.err.starters"));
    }
    st.nx = round(st.nx);
    st.ny = round(st.ny);
    let [x, y, top, l, b, h, rotation] = vals;
    let footing = Footing { id: String::new(), x, y, top, l, b, h, rotation, concrete, mat, starters: st, phase: 1.0 };
    if let Some(hard) = footing_problems(&footing).into_iter().find(|p| p.hard) {
        return Err(hard.text);
    }
    Ok(footing)
}

/// A footing check.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub text: String,
    pub hard: bool,
}
impl Problem {
    pub fn j(&self) -> Value {
        let o = Obj::new();
        let o = if self.hard { o.set("hard", true) } else { o };
        o.set("text", self.text.as_str()).done()
    }
}

fn r1(v: f64) -> f64 {
    fixed_num(v, 1)
}

fn bar_or_first(name: &str) -> &'static Bar {
    // the model only stores footings whose bars are in the table; the first bar stands in for the impossible case
    bar(name).or_else(|| bars().first()).unwrap_or_else(|| unreachable_bar())
}
fn unreachable_bar() -> &'static Bar {
    static B: OnceLock<Bar> = OnceLock::new();
    B.get_or_init(|| Bar {
        name: String::new(),
        num: f64::NAN,
        db: f64::NAN,
        mass: f64::NAN,
        area: f64::NAN,
        bend_d: f64::NAN,
        set: "",
        mark_pre: String::new(),
        metric: String::new(),
        r: f64::NAN,
        pp: f64::NAN,
        pub_: None,
    })
}

/// `footingProblems(f)`.
pub fn footing_problems(f: &Footing) -> Vec<Problem> {
    let mut out = Vec::new();
    let (bx, by, c) = (bar_or_first(&f.mat.bar_x), bar_or_first(&f.mat.bar_y), f.mat.cover);
    if f.l - 2.0 * c <= bx.db * 2.0 || f.b - 2.0 * c <= by.db * 2.0 {
        out.push(Problem { hard: true, text: t0("rebar.warn.no-room") });
    }
    if f.h - 2.0 * c <= bx.db + by.db {
        out.push(Problem { hard: true, text: t0("rebar.warn.shallow") });
    }
    let min_clear = |b: &Bar| if b.set == "bs" { max(20.0, b.db) } else { max(25.4, b.db) };
    let lx = layout(f.b - 2.0 * c - bx.db, f.mat.s_x);
    let ly = layout(f.l - 2.0 * c - by.db, f.mat.s_y);
    if lx.1 - bx.db < min_clear(bx) - 1e-6 {
        out.push(Problem {
            hard: false,
            text: t(
                "rebar.warn.clear",
                &[("bar", F::from(f.mat.bar_x.as_str())), ("spacing", F::N(r1(lx.1))), ("min", F::N(r1(min_clear(bx))))],
            ),
        });
    }
    if ly.1 - by.db < min_clear(by) - 1e-6 {
        out.push(Problem {
            hard: false,
            text: t(
                "rebar.warn.clear",
                &[("bar", F::from(f.mat.bar_y.as_str())), ("spacing", F::N(r1(ly.1))), ("min", F::N(r1(min_clear(by))))],
            ),
        });
    }
    if f.mat.hooks {
        let room = f.h - 2.0 * c;
        for b in [bx, by] {
            if hook_leg(b) > room + 1e-6 {
                out.push(Problem {
                    hard: false,
                    text: t(
                        "rebar.warn.hooks",
                        &[("bar", F::from(b.name.as_str())), ("need", F::N(r1(hook_leg(b)))), ("room", F::N(r1(room)))],
                    ),
                });
            }
        }
    }
    if f.starters.on {
        let s = &f.starters;
        let sb = bar_or_first(&s.bar);
        let ex = (s.nx - 1.0) * s.sx / 2.0 + sb.db / 2.0;
        let ey = (s.ny - 1.0) * s.sy / 2.0 + sb.db / 2.0;
        if ex > f.l / 2.0 - c || ey > f.b / 2.0 - c {
            out.push(Problem { hard: true, text: t("rebar.warn.starters-fit", &[("cover", F::N(c))]) });
        }
        if s.hook && max(ex, ey) + hook_leg(sb) - sb.db / 2.0 > max(f.l, f.b) / 2.0 - c {
            out.push(Problem { hard: false, text: t0("rebar.warn.starter-hooks") });
        }
        if s.nx > 1.0 && s.sx - sb.db < min_clear(sb) {
            out.push(Problem { hard: false, text: t0("rebar.warn.starter-clear") });
        }
        if s.ny > 1.0 && s.sy - sb.db < min_clear(sb) {
            out.push(Problem { hard: false, text: t0("rebar.warn.starter-clear") });
        }
    }
    out
}

/// `layout(w, s)` → (n, spacing).
pub fn layout(w: f64, s: f64) -> (f64, f64) {
    let n = max(2.0, (w / s - 1e-9).ceil() + 1.0);
    (n, w / (n - 1.0))
}
/// `hookLeg(b)`.
pub fn hook_leg(b: &Bar) -> f64 {
    if b.set == "bs" { b.pp } else { b.bend_d / 2.0 + b.db + 12.0 * b.db }
}

/// `cutLength(shape, dims, b)`.
pub fn cut_length(shape: &str, dims: &Dims, b: &Bar) -> Res<f64> {
    let k = b.bend_d / 2.0 + b.db;
    let rc = b.bend_d / 2.0 + b.db / 2.0;
    let gb = dims.b.unwrap_or(f64::NAN);
    let gc = dims.c.unwrap_or(f64::NAN);
    if shape == "S" {
        return Ok(dims.a);
    }
    if b.set == "bs" {
        if shape == "L" {
            return Ok(dims.a + gb - 0.5 * b.r - b.db);
        }
        if shape == "U" {
            return Ok(dims.a + gb + gc - b.r - 2.0 * b.db);
        }
    }
    if shape == "L" {
        return Ok(dims.a + gb - 2.0 * k + PI / 2.0 * rc);
    }
    if shape == "U" {
        return Ok(dims.a + gb + gc - 4.0 * k + PI * rc);
    }
    Err(t("rebar.err.shape", &[("shape", F::from(shape))]))
}

/// One centerline segment in the bar's plane.
#[derive(Clone, Copy, Debug)]
pub enum Seg {
    L { a: [f64; 2], b: [f64; 2] },
    A { c: [f64; 2], r: f64, a: [f64; 2], m: [f64; 2], b: [f64; 2] },
}
impl Seg {
    pub fn a(&self) -> [f64; 2] {
        match self {
            Seg::L { a, .. } | Seg::A { a, .. } => *a,
        }
    }
    pub fn j(&self) -> Value {
        let p = |q: &[f64; 2]| Value::Array(vec![q[0].into(), q[1].into()]);
        match self {
            Seg::L { a, b } => Obj::new().set("k", "L").set("a", p(a)).set("b", p(b)).done(),
            Seg::A { c, r, a, m, b } => {
                Obj::new().set("k", "A").set("c", p(c)).set("r", *r).set("a", p(a)).set("m", p(m)).set("b", p(b)).done()
            }
        }
    }
}

/// `centerline2d(shape, dims, b)`.
pub fn centerline2d(shape: &str, dims: &Dims, b: &Bar) -> Vec<Seg> {
    let h = b.db / 2.0;
    let rc = b.bend_d / 2.0 + h;
    let a = dims.a;
    let arc = |c: [f64; 2], a0: f64, a1: f64| Seg::A {
        c,
        r: rc,
        a: [c[0] + rc * cos(a0), c[1] + rc * sin(a0)],
        m: [c[0] + rc * cos((a0 + a1) / 2.0), c[1] + rc * sin((a0 + a1) / 2.0)],
        b: [c[0] + rc * cos(a1), c[1] + rc * sin(a1)],
    };
    if shape == "S" {
        return vec![Seg::L { a: [0.0, h], b: [a, h] }];
    }
    let mut segs = Vec::new();
    if shape == "U" {
        let c1 = [h + rc, h + rc];
        segs.push(Seg::L { a: [h, dims.b.unwrap_or(f64::NAN) - 0.0], b: [h, h + rc] });
        segs.push(arc(c1, PI, 1.5 * PI));
        segs.push(Seg::L { a: [h + rc, h], b: [a - h - rc, h] });
    } else {
        segs.push(Seg::L { a: [0.0, h], b: [a - h - rc, h] });
    }
    let c2 = [a - h - rc, h + rc];
    segs.push(arc(c2, 1.5 * PI, 2.0 * PI));
    let leg_end = if shape == "U" { dims.c } else { dims.b }.unwrap_or(f64::NAN);
    segs.push(Seg::L { a: [a - h, h + rc], b: [a - h, leg_end] });
    segs
}
/// `centerlineLength(segs)`.
pub fn centerline_length(segs: &[Seg]) -> f64 {
    let mut s = 0.0;
    for g in segs {
        s += match g {
            Seg::L { a, b } => hypot(b[0] - a[0], b[1] - a[1]),
            Seg::A { r, .. } => r * PI / 2.0,
        };
    }
    s
}

/// One placement of a bar's centerline: world = origin + s·sDir + t·tDir.
#[derive(Clone, Copy, Debug)]
pub struct BarSet {
    pub origin: V3,
    pub s_dir: V3,
    pub t_dir: V3,
}
/// A group of identical bars in a footing.
#[derive(Clone, Debug)]
pub struct BarGroup {
    pub role: String,
    pub bar: String,
    pub shape: &'static str,
    pub dims: Dims,
    pub count: f64,
    pub spacing: Option<f64>,
    pub length: f64,
    pub sets: Vec<BarSet>,
}
impl BarGroup {
    pub fn j(&self) -> Value {
        let sets: Vec<Value> = self
            .sets
            .iter()
            .map(|s| Obj::new().set("origin", s.origin.to_vec()).set("sDir", s.s_dir.to_vec()).set("tDir", s.t_dir.to_vec()).done())
            .collect();
        Obj::new()
            .set("role", self.role.as_str())
            .set("bar", self.bar.as_str())
            .set("shape", self.shape)
            .set("dims", self.dims.j())
            .set("count", self.count)
            .opt("spacing", self.spacing)
            .set("length", self.length)
            .set("sets", Value::Array(sets))
            .done()
    }
}

/// The 2^31 placement cap (the JS has none; a stored footing allows at most 20 × 20 starters and a mat of
/// (50 000 mm / 50 mm) bars, so this never cuts).
const MAX_BARS: f64 = 100_000.0;

/// `footingBars(f)`.
pub fn footing_bars(f: &Footing) -> Vec<BarGroup> {
    let mut out = Vec::new();
    let rot = crate::js::or(f.rotation, 0.0) * PI / 180.0;
    let ex = [cos(rot), sin(rot), 0.0];
    let ey = [-sin(rot), cos(rot), 0.0];
    let ez = [0.0, 0.0, 1.0];
    let pp = |lx: f64, ly: f64, z: f64| -> V3 { [f.x + ex[0] * lx + ey[0] * ly, f.y + ex[1] * lx + ey[1] * ly, z] };
    let bottom = f.top - f.h;
    let c = f.mat.cover;
    let (bx, by) = (bar_or_first(&f.mat.bar_x), bar_or_first(&f.mat.bar_y));
    let mut layer = |b: &Bar, along_x: bool, run_len: f64, across_len: f64, s: f64, z_bottom: f64, role: String| {
        let a = run_len - 2.0 * c;
        let hooks = f.mat.hooks;
        let room = f.h - 2.0 * c - (z_bottom - (bottom + c));
        let bb = if hooks { min(hook_leg(b), room) } else { 0.0 };
        let shape = if hooks { "U" } else { "S" };
        let dims = if hooks { Dims { a, b: Some(bb), c: Some(bb) } } else { Dims { a, b: None, c: None } };
        let lay = layout(across_len - 2.0 * c - b.db, s);
        let mut sets = Vec::new();
        let mut i = 0.0;
        while i < lay.0 && i < MAX_BARS {
            let off = -across_len / 2.0 + c + b.db / 2.0 + i * lay.1;
            let o = if along_x { pp(-run_len / 2.0 + c, off, z_bottom) } else { pp(off, -run_len / 2.0 + c, z_bottom) };
            sets.push(BarSet { origin: o, s_dir: if along_x { ex } else { ey }, t_dir: ez });
            i += 1.0;
        }
        let length = cut_length(shape, &dims, b).unwrap_or(f64::NAN);
        out.push(BarGroup { role, bar: b.name.clone(), shape, dims, count: lay.0, spacing: Some(lay.1), length, sets });
    };
    layer(bx, true, f.l, f.b, f.mat.s_x, bottom + c, t0("rebar.role.bottom-long"));
    layer(by, false, f.b, f.l, f.mat.s_y, bottom + c + bx.db, t0("rebar.role.bottom-short"));
    if f.starters.on {
        let s = &f.starters;
        let b = bar_or_first(&s.bar);
        let z0 = bottom + c + bx.db + by.db;
        let a = f.top + s.projection - z0;
        let shape = if s.hook { "L" } else { "S" };
        let dims = if s.hook { Dims { a, b: Some(hook_leg(b)), c: None } } else { Dims { a, b: None, c: None } };
        let mut sets = Vec::new();
        let mut i = 0.0;
        while i < s.nx && i < MAX_BARS {
            let mut j = 0.0;
            while j < s.ny && j < MAX_BARS {
                let lx = (i - (s.nx - 1.0) / 2.0) * s.sx;
                let ly = (j - (s.ny - 1.0) / 2.0) * s.sy;
                let neg = |v: V3| -> V3 { [-v[0], -v[1], -v[2]] };
                let mut hd = if lx.abs() >= ly.abs() {
                    if lx >= 0.0 { ex } else { neg(ex) }
                } else if ly >= 0.0 {
                    ey
                } else {
                    neg(ey)
                };
                if lx.abs() < 1e-9 && ly.abs() < 1e-9 {
                    hd = ex;
                }
                let top = pp(lx, ly, z0 + a);
                sets.push(BarSet {
                    origin: [top[0] - hd[0] * b.db / 2.0, top[1] - hd[1] * b.db / 2.0, top[2]],
                    s_dir: [0.0, 0.0, -1.0],
                    t_dir: hd,
                });
                j += 1.0;
            }
            i += 1.0;
        }
        let length = cut_length(shape, &dims, b).unwrap_or(f64::NAN);
        out.push(BarGroup {
            role: t0("rebar.role.starters"),
            bar: s.bar.clone(),
            shape,
            dims,
            count: s.nx * s.ny,
            spacing: None,
            length,
            sets,
        });
    }
    out
}

/// `barPolyline(group, set, seg)`.
pub fn bar_polyline(group: &BarGroup, set: &BarSet, seg: usize) -> Vec<V3> {
    let b = bar_or_first(&group.bar);
    let segs = centerline2d(group.shape, &group.dims, b);
    let w = |q: [f64; 2]| -> V3 {
        let (s, t) = (q[0], q[1]);
        [
            set.origin[0] + set.s_dir[0] * s + set.t_dir[0] * t,
            set.origin[1] + set.s_dir[1] * s + set.t_dir[1] * t,
            set.origin[2] + set.s_dir[2] * s + set.t_dir[2] * t,
        ]
    };
    let mut pts = Vec::new();
    if let Some(first) = segs.first() {
        pts.push(w(first.a()));
    }
    for g in &segs {
        match g {
            Seg::L { b, .. } => pts.push(w(*b)),
            Seg::A { c, r, a, b, .. } => {
                let a0 = atan2(a[1] - c[1], a[0] - c[0]);
                let a1 = atan2(b[1] - c[1], b[0] - c[0]);
                let mut d = a1 - a0;
                if d < -PI {
                    d += 2.0 * PI;
                }
                if d > PI {
                    d -= 2.0 * PI;
                }
                for i in 1..=seg {
                    let ang = a0 + d * i as f64 / seg as f64;
                    pts.push(w([c[0] + r * cos(ang), c[1] + r * sin(ang)]));
                }
            }
        }
    }
    pts
}

/// `footingVolume(f)` (m³).
pub fn footing_volume(f: &Footing) -> f64 {
    f.l * f.b * f.h * 1e-9
}

/// A bending schedule row.
#[derive(Clone, Debug)]
pub struct SchedRow {
    pub mark: String,
    pub bar: String,
    pub shape: &'static str,
    pub dims: Dims,
    pub count: f64,
    pub length: f64,
    pub mass: f64,
    pub roles: Vec<String>,
    pub total_length: f64,
    pub unit_weight: f64,
    pub total_weight: f64,
}
impl SchedRow {
    pub fn j(&self) -> Value {
        Obj::new()
            .set("mark", self.mark.as_str())
            .set("bar", self.bar.as_str())
            .set("shape", self.shape)
            .set("dims", self.dims.j())
            .set("count", self.count)
            .set("length", self.length)
            .set("mass", self.mass)
            .set("roles", self.roles.clone())
            .set("totalLength", self.total_length)
            .set("unitWeight", self.unit_weight)
            .set("totalWeight", self.total_weight)
            .done()
    }
}
/// `bendingSchedule(footings)` → rows and, per footing (in order), its groups with their marks.
pub struct Schedule {
    pub rows: Vec<SchedRow>,
    pub by_footing: Vec<(String, Vec<(BarGroup, String)>)>,
}
pub fn bending_schedule(footings: &[Footing]) -> Schedule {
    let mut marks: HashMap<String, usize> = HashMap::new();
    let mut rows: Vec<SchedRow> = Vec::new();
    let mut by_footing: Vec<(String, Vec<(BarGroup, String)>)> = Vec::new();
    let mut counters: HashMap<String, f64> = HashMap::new();
    for f in footings {
        let mut list = Vec::new();
        for g in footing_bars(f) {
            let b = bar_or_first(&g.bar);
            let mut key = vec![g.bar.clone(), g.shape.to_string()];
            for k in ["A", "B", "C"] {
                key.push(g.dims.get(k).map_or_else(String::new, |v| num_to_string(round(v))));
            }
            let key = key.join("|");
            let i = match marks.get(&key) {
                Some(&i) => i,
                None => {
                    let n = counters.get(&b.mark_pre).copied().unwrap_or(0.0) + 1.0;
                    counters.insert(b.mark_pre.clone(), n);
                    rows.push(SchedRow {
                        mark: format!("{}F{}", b.mark_pre, num_to_string(n)),
                        bar: g.bar.clone(),
                        shape: g.shape,
                        dims: g.dims,
                        count: 0.0,
                        length: g.length,
                        mass: b.mass,
                        roles: vec![],
                        total_length: 0.0,
                        unit_weight: 0.0,
                        total_weight: 0.0,
                    });
                    marks.insert(key, rows.len().saturating_sub(1));
                    rows.len().saturating_sub(1)
                }
            };
            let mark = if let Some(row) = rows.get_mut(i) {
                row.count += g.count;
                if !row.roles.contains(&g.role) {
                    row.roles.push(g.role.clone());
                }
                row.mark.clone()
            } else {
                String::new()
            };
            list.push((g, mark));
        }
        // a Map keyed by footing id: a repeated id keeps its first place and takes the later list
        match by_footing.iter_mut().find(|(id, _)| *id == f.id) {
            Some(slot) => slot.1 = list,
            None => by_footing.push((f.id.clone(), list)),
        }
    }
    for r in &mut rows {
        r.total_length = r.length * r.count;
        r.unit_weight = r.mass * r.length / 1000.0;
        r.total_weight = r.unit_weight * r.count;
    }
    Schedule { rows, by_footing }
}

/// `scheduleTable(rows, {codes, naming})` (CSV rows, header first).
pub fn schedule_table(rows: &[SchedRow], codes: &str, naming: &str) -> Vec<Vec<String>> {
    let mm = |v: Option<f64>| v.map_or_else(String::new, |v| num_to_string(round(v)));
    let head: Vec<String> =
        ["mark", "size", "shape", "a", "b", "c", "count", "cut", "total-length", "kg-m", "weight-each", "total-weight", "used-for"]
            .iter()
            .map(|k| t0(&format!("rebar.col.{k}")))
            .collect();
    let mut out = vec![head];
    for r in rows {
        let (a, b, c) = schedule_dims(r.shape, &r.dims, codes);
        out.push(vec![
            r.mark.clone(),
            bar_label(&r.bar, naming),
            shape_code(r.shape, codes),
            mm(a),
            mm(b),
            mm(c),
            num_to_string(r.count),
            mm(Some(r.length)),
            to_fixed(r.total_length / 1000.0, 2),
            to_fixed(r.mass, 3),
            to_fixed(r.unit_weight, 2),
            to_fixed(r.total_weight, 1),
            r.roles.join("; "),
        ]);
    }
    let total = rows.iter().fold(0.0, |s, r| s + r.total_weight);
    let count = rows.iter().fold(0.0, |s, r| s + r.count);
    let tl = rows.iter().fold(0.0, |s, r| s + r.total_length);
    out.push(vec![
        t0("rebar.total"),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        num_to_string(count),
        String::new(),
        to_fixed(tl / 1000.0, 2),
        String::new(),
        String::new(),
        to_fixed(total, 1),
        String::new(),
    ]);
    out
}

/// `FOOTING_DEFAULTS` as the JS object.
pub fn footing_defaults() -> Value {
    serde_json::json!({
        "x": 0, "y": 0, "top": 0, "L": 2000, "B": 2000, "H": 600, "rotation": 0, "concrete": "4000 psi",
        "mat": { "barX": "#5", "sX": 250, "barY": "#5", "sY": 250, "cover": 75, "hooks": true },
        "starters": { "on": true, "bar": "#6", "nx": 2, "ny": 2, "sx": 250, "sy": 250, "projection": 600, "hook": true },
    })
}

/// The JSON of a bar size.
pub fn bar_json(b: &Bar) -> Value {
    let mut o = Obj::new()
        .set("name", b.name.as_str())
        .set("num", b.num)
        .set("db", b.db)
        .set("mass", b.mass)
        .set("area", b.area)
        .set("bendD", b.bend_d);
    if let Some([d, w, a]) = b.pub_ {
        o.put("pub", Obj::new().set("d", d).set("w", w).set("a", a).done());
    }
    if b.set == "bs" {
        o.put("r", b.r);
        o.put("P", b.pp);
    }
    o.set("set", b.set).set("markPre", b.mark_pre.as_str()).set("metric", b.metric.as_str()).done()
}
