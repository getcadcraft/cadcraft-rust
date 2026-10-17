//! superelev.js — superelevation basics for a corridor (0.4 V18). Slopes are FALL-AWAY slopes: + = the lane falls
//! away from the crown (normal crown +NC both sides), − = it rises away from the crown.

use crate::alignment::{Alignment, Element};
use crate::js::{J, Obj, arr, max, min};
use crate::tr;
use serde_json::Value;

/// `{ on, delta, rate: [[radius ≥, e | null]…] }`.
#[derive(Clone, Debug, PartialEq)]
pub struct Super {
    pub on: bool,
    pub delta: f64,
    pub rate: Vec<(f64, Option<f64>)>,
}
impl J for Super {
    fn j(&self) -> Value {
        let rate: Vec<Value> = self.rate.iter().map(|(r, e)| Value::Array(vec![r.j(), e.j()])).collect();
        crate::jo! { "on" => self.on, "delta" => self.delta, "rate" => rate }
    }
}

/// `SUPER_DEFAULTS(units)`.
pub fn super_defaults(units: &str) -> Super {
    if units == "m" {
        Super {
            on: false,
            delta: 0.005,
            rate: vec![
                (2100.0, None),
                (1300.0, Some(0.02)),
                (800.0, Some(0.03)),
                (550.0, Some(0.04)),
                (380.0, Some(0.05)),
                (0.0, Some(0.06)),
            ],
        }
    } else {
        Super {
            on: false,
            delta: 0.005,
            rate: vec![
                (7000.0, None),
                (4300.0, Some(0.02)),
                (2600.0, Some(0.03)),
                (1800.0, Some(0.04)),
                (1250.0, Some(0.05)),
                (0.0, Some(0.06)),
            ],
        }
    }
}

fn finv(v: Option<&Value>) -> Option<f64> {
    crate::js::fin(v)
}

/// `cleanSuper(obj, units)` (untrusted: the project file).
pub fn clean_super(o: Option<&Value>, units: &str) -> Option<Super> {
    let o = o.filter(|o| o.is_object() || o.is_array())?;
    let d = super_defaults(units);
    let mut rate: Vec<(f64, Option<f64>)> = Vec::new();
    if let Some(list) = o.get("rate").and_then(Value::as_array) {
        for r in list {
            let Some(ra) = r.as_array() else { continue };
            let r0 = finv(ra.first());
            let e = ra.get(1);
            let e_ok = matches!(e, Some(Value::Null)) || finv(e).is_some_and(|x| (0.0..=0.2).contains(&x));
            if let Some(r0) = r0
                && (0.0..1e7).contains(&r0)
                && e_ok
            {
                rate.push((r0, finv(e)));
            }
        }
        rate.truncate(40);
    }
    crate::js::sort_by(&mut rate, |a, b| b.0 - a.0);
    let delta = finv(o.get("delta")).filter(|&x| x > 0.0 && x <= 0.05).unwrap_or(d.delta);
    Some(Super { on: o.get("on") == Some(&Value::Bool(true)), delta, rate: if rate.is_empty() { d.rate } else { rate } })
}

/// Read a `super` object as given (trusted input from the harness / a cleaned state).
pub fn read_super(o: &Value) -> Super {
    let rate = arr(o.get("rate"))
        .iter()
        .map(|r| (crate::js::numf(r.get(0)), r.get(1).filter(|e| !e.is_null()).map(|e| crate::js::numf(Some(e)))))
        .collect();
    Super { on: o.get("on") == Some(&Value::Bool(true)), delta: crate::js::numf(o.get("delta")), rate }
}

/// `rateFor(R, rate)` → e | null (normal crown).
pub fn rate_for(r: f64, rate: &[(f64, Option<f64>)]) -> Option<f64> {
    for (rr, e) in rate {
        if r >= *rr {
            return *e;
        }
    }
    rate.last().and_then(|x| x.1)
}

/// The transition stations of one curve.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stations {
    pub nc0: f64,
    pub lc0: f64,
    pub rc0: f64,
    pub fs0: f64,
    pub fs1: f64,
    pub rc1: f64,
    pub lc1: f64,
    pub nc1: f64,
}
impl J for Stations {
    fn j(&self) -> Value {
        // the JS object's key order: lc0, fs0, fs1, lc1, then nc0, nc1, rc0, rc1
        crate::jo! { "lc0" => self.lc0, "fs0" => self.fs0, "fs1" => self.fs1, "lc1" => self.lc1, "nc0" => self.nc0, "nc1" => self.nc1, "rc0" => self.rc0, "rc1" => self.rc1 }
    }
}
/// One superelevated curve.
#[derive(Clone, Debug)]
pub struct SCurve {
    pub i: usize,
    pub r: f64,
    pub left: bool,
    /// outside lane: 'L' or 'R'
    pub outside_l: bool,
    pub e: f64,
    pub lr: f64,
    pub lt: f64,
    pub pc: f64,
    pub pt: f64,
    pub stations: Stations,
}
impl J for SCurve {
    fn j(&self) -> Value {
        Obj::new()
            .set("i", self.i)
            .set("R", self.r)
            .set("dir", if self.left { "left" } else { "right" })
            .set("outside", if self.outside_l { "L" } else { "R" })
            .set("e", self.e)
            .set("Lr", self.lr)
            .set("Lt", self.lt)
            .set("pc", self.pc)
            .set("pt", self.pt)
            .set("stations", self.stations)
            .done()
    }
}
/// `superPlan(…)` → { curves, problems }.
#[derive(Clone, Debug, Default)]
pub struct Plan {
    pub curves: Vec<SCurve>,
    pub problems: Vec<String>,
}
impl J for Plan {
    fn j(&self) -> Value {
        crate::jo! { "curves" => self.curves, "problems" => self.problems }
    }
}

/// `superPlan(alignment, { super, nc, lane })`.
pub fn super_plan(al: &Alignment, sp: Option<&Super>, nc: f64, lane: f64) -> Plan {
    let mut curves: Vec<SCurve> = Vec::new();
    let mut problems = Vec::new();
    let Some(sp) = sp.filter(|s| s.on) else { return Plan { curves, problems } };
    if !al.ok() {
        return Plan { curves, problems };
    }
    let arcs = al.elements.iter().filter_map(|e| if let Element::Arc(a) = e { Some(a) } else { None });
    for (i, c) in arcs.enumerate() {
        let Some(e) = rate_for(c.r, &sp.rate) else { continue };
        if !(e > 0.0) {
            continue;
        }
        let left = c.ccw;
        let lr = (lane * e) / sp.delta;
        let lt = (nc / max(e, 1e-9)) * lr;
        let pc = c.sta;
        let pt = c.sta + c.len;
        let mut st = Stations {
            lc0: pc - (2.0 * lr) / 3.0,
            fs0: pc + lr / 3.0,
            fs1: pt - lr / 3.0,
            lc1: pt + (2.0 * lr) / 3.0,
            ..Stations::default()
        };
        st.nc0 = st.lc0 - lt;
        st.nc1 = st.lc1 + lt;
        st.rc0 = st.lc0 + (lr * min(1.0, nc / e));
        st.rc1 = st.lc1 - (lr * min(1.0, nc / e));
        if st.fs1 < st.fs0 {
            problems.push(tr!("se.short", n = i + 1, r = crate::js::round_fixed(c.r, 2)));
        }
        curves.push(SCurve { i: i + 1, r: c.r, left, outside_l: !left, e, lr, lt, pc, pt, stations: st });
    }
    for k in 1..curves.len() {
        let (a, b) = (curves.get(k - 1), curves.get(k));
        if let (Some(a), Some(b)) = (a, b)
            && b.stations.nc0 < a.stations.nc1
        {
            problems.push(tr!("se.overlap", a = a.i, b = b.i));
        }
    }
    Plan { curves, problems }
}

/// The outside lane's fall-away slope along one curve's transition (+NC outside it).
fn outside_at(c: &SCurve, sta: f64, nc: f64) -> f64 {
    let s = &c.stations;
    let e = c.e;
    let up = |x: f64| {
        if x <= s.nc0 {
            return nc;
        }
        if x <= s.lc0 {
            return nc - (nc * (x - s.nc0)) / max(c.lt, 1e-9);
        }
        -(e * (x - s.lc0)) / max(c.lr, 1e-9)
    };
    let down = |x: f64| {
        if x >= s.nc1 {
            return nc;
        }
        if x >= s.lc1 {
            return nc - (nc * (s.nc1 - x)) / max(c.lt, 1e-9);
        }
        -(e * (s.lc1 - x)) / max(c.lr, 1e-9)
    };
    max(max(up(sta), down(sta)), -e)
}

/// `slopesAt(plan, sta, nc)` → (L, R).
pub fn slopes_at(plan: &Plan, sta: f64, nc: f64) -> (f64, f64) {
    let (mut l, mut r) = (nc, nc);
    for c in &plan.curves {
        if sta <= c.stations.nc0 || sta >= c.stations.nc1 {
            continue;
        }
        let o = outside_at(c, sta, nc);
        let inside = max(nc, -o);
        if c.outside_l {
            l = o;
            r = inside;
        } else {
            r = o;
            l = inside;
        }
    }
    (l, r)
}

/// `keyStations(plan)`: every transition station (in each curve's key order).
pub fn key_stations(plan: &Plan) -> Vec<f64> {
    let mut out = Vec::new();
    for c in &plan.curves {
        let s = &c.stations;
        out.extend([s.lc0, s.fs0, s.fs1, s.lc1, s.nc0, s.nc1, s.rc0, s.rc1]);
    }
    out
}
