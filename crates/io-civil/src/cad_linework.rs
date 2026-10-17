//! cad-linework.js — a DWG or DXF drawing (cc-io-dwg's model: `read_dwg` / `read_dxf_model`) as GIS-style linework
//! layers: per CAD layer up to three layers (lines, closed shapes, points) with LAYER / TYPE / ELEV fields. Model space
//! only; blocks placed up to 8 deep; curves every 5°; OCS by the arbitrary axis algorithm; at most 5 M vertices.

use cc_civil::gis_state::{Feature, Field, Geom, Pos};
use cc_civil::i18n::F;
use cc_civil::js::{G, J, JsMath, hypot, hypot3, int32, locale_compare, max, min, num_to_string};
use cc_civil::tr;
use cc_io_dwg::{Obj, V};
use serde_json::{Map, Value};

pub const MAX_DEPTH: usize = 8;
pub const MAX_VERTICES: usize = 5_000_000;
const STEP: f64 = std::f64::consts::PI / 36.0;
const TAU: f64 = 2.0 * std::f64::consts::PI;

fn insunits(code: &V) -> Option<(&'static str, f64)> {
    // INSUNITS[header.INSUNITS]: the value as a property key (a number or its text)
    let key = match code {
        V::Num(n) => num_to_string(*n),
        V::Str(s) => s.clone(),
        V::Bool(b) => b.to_string(),
        _ => return None,
    };
    Some(match key.as_str() {
        "1" => ("in", 0.0254),
        "2" => ("ft", 0.3048),
        "3" => ("mi", 1609.344),
        "4" => ("mm", 0.001),
        "5" => ("cm", 0.01),
        "6" => ("m", 1.0),
        "7" => ("km", 1000.0),
        "8" => ("microinch", 2.54e-8),
        "9" => ("mil", 2.54e-5),
        "10" => ("yd", 0.9144),
        "14" => ("dm", 0.1),
        "21" => ("ftUS", 1200.0 / 3937.0),
        _ => return None,
    })
}

/// A V as JSON (the model's own values).
pub fn v_json(v: &V) -> Value {
    match v {
        V::Null => Value::Null,
        V::Bool(b) => Value::Bool(*b),
        V::Num(n) => crate::gjs::num_keep(*n),
        V::Str(s) => Value::String(s.clone()),
        V::Arr(a) => Value::Array(a.iter().map(v_json).collect()),
        V::Obj(o) => Value::Object(o.iter().map(|(k, x)| (k.to_string(), v_json(x))).collect()),
        V::Bytes(b) => Value::String(crate::gjs::b64(b)),
    }
}

/// `cadUnits(header)` → { code, label, metres | null }.
#[derive(Clone, Debug)]
pub struct CadUnits {
    pub code: Value,
    pub label: String,
    pub metres: Option<f64>,
}
impl J for CadUnits {
    fn j(&self) -> Value {
        cc_civil::jo! { "code" => self.code, "label" => self.label, "metres" => self.metres }
    }
}
pub fn cad_units(header: Option<&Obj>) -> CadUnits {
    let code = header.and_then(|h| h.get("INSUNITS"));
    match code.and_then(insunits) {
        Some((label, m)) => CadUnits { code: code.map_or(Value::Null, v_json), label: label.into(), metres: Some(m) },
        None => CadUnits {
            code: match code {
                None | Some(V::Null) => Value::from(0),
                Some(c) => v_json(c),
            },
            label: String::new(),
            metres: None,
        },
    }
}

/// `unitScale(header, projectUnit)`.
#[derive(Clone, Debug)]
pub struct UnitScale {
    pub scale: f64,
    pub from: String,
    pub to: String,
    pub note: Option<String>,
}
impl J for UnitScale {
    fn j(&self) -> Value {
        cc_civil::jo! { "scale" => self.scale, "from" => self.from, "to" => self.to, "note" => self.note }
    }
}
pub fn unit_scale(header: Option<&Obj>, project_unit: &str) -> UnitScale {
    let u = cad_units(header);
    let to = if project_unit == "m" { "m" } else { "ft" };
    let unit_word = || tr!(if to == "m" { "cad.unit.m" } else { "cad.unit.ft" });
    let Some(metres) = u.metres else {
        return UnitScale { scale: 1.0, from: String::new(), to: to.into(), note: Some(tr!("cad.units.unknown", unit = unit_word())) };
    };
    if to == "ft" && (u.label == "ft" || u.label == "ftUS") {
        return UnitScale {
            scale: 1.0,
            from: u.label.clone(),
            to: to.into(),
            note: if u.label == "ft" { Some(tr!("cad.units.feet")) } else { None },
        };
    }
    if to == "m" && u.label == "m" {
        return UnitScale { scale: 1.0, from: "m".into(), to: to.into(), note: None };
    }
    let proj_m = if to == "m" { 1.0 } else { 1200.0 / 3937.0 };
    let scale =
        if to == "ft" && ["in", "mi", "yd", "mil", "microinch"].contains(&u.label.as_str()) { metres / 0.3048 } else { metres / proj_m };
    let shown = cc_civil::js::str_to_number(&cc_m3d::js::to_precision(scale, 10));
    UnitScale {
        scale,
        from: u.label.clone(),
        to: to.into(),
        note: Some(tr!("cad.units.scaled", from = u.label.as_str(), to = unit_word(), scale = shown)),
    }
}

// ---------------------------------------------------------------- 3D affine transforms (row-major 3×4)
type M = [f64; 12];
fn mul(a: &M, b: &M) -> M {
    let row = |r0: f64, r1: f64, r2: f64, r3: f64| {
        [
            r0 * b[0] + r1 * b[4] + r2 * b[8],
            r0 * b[1] + r1 * b[5] + r2 * b[9],
            r0 * b[2] + r1 * b[6] + r2 * b[10],
            r0 * b[3] + r1 * b[7] + r2 * b[11] + r3,
        ]
    };
    let x = row(a[0], a[1], a[2], a[3]);
    let y = row(a[4], a[5], a[6], a[7]);
    let z = row(a[8], a[9], a[10], a[11]);
    [x[0], x[1], x[2], x[3], y[0], y[1], y[2], y[3], z[0], z[1], z[2], z[3]]
}
/// `(v | 0) & bit`.
pub fn flag(v: f64, bit: i64) -> bool {
    cc_civil::js::i64_of(int32(v)).is_some_and(|x| x & bit != 0)
}
fn apply(m: &M, x: f64, y: f64, z: f64) -> [f64; 3] {
    [m[0] * x + m[1] * y + m[2] * z + m[3], m[4] * x + m[5] * y + m[6] * z + m[7], m[8] * x + m[9] * y + m[10] * z + m[11]]
}
fn translate(x: f64, y: f64, z: f64) -> M {
    [1.0, 0.0, 0.0, x, 0.0, 1.0, 0.0, y, 0.0, 0.0, 1.0, z]
}

fn num(o: Option<&V>) -> f64 {
    match o {
        Some(V::Num(n)) => *n,
        Some(V::Bool(b)) => f64::from(u8::from(*b)),
        _ => f64::NAN,
    }
}
/// `v || 0`.
fn or0(o: Option<&V>) -> f64 {
    let x = match o {
        Some(V::Num(n)) => *n,
        Some(V::Bool(true)) => 1.0,
        Some(V::Str(s)) if !s.is_empty() => cc_civil::js::str_to_number(s),
        _ => 0.0,
    };
    if x.is_nan() { 0.0 } else { x }
}
/// `v ?? d`.
fn or_null(o: Option<&V>, d: f64) -> f64 {
    match o {
        None | Some(V::Null) => d,
        x => num(x),
    }
}
fn get<'a>(o: &'a Obj, k: &str) -> Option<&'a V> {
    o.get(k)
}

/// `ocs(ext)`: OCS → WCS for an extrusion direction; None for the plain +Z (or no direction).
pub fn ocs(ext: Option<&V>) -> Option<M> {
    let e = ext?.as_obj()?;
    let (x, y, z) = (num(e.get("x")), num(e.get("y")), num(e.get("z")));
    if !x.is_finite() || !y.is_finite() || !z.is_finite() {
        return None;
    }
    let l = hypot3(x, y, z);
    if l == 0.0 {
        return None;
    }
    let az = [x / l, y / l, z / l];
    if az[0].abs() < 1e-12 && az[1].abs() < 1e-12 && az[2] > 0.0 {
        return None;
    }
    let cross = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let mut ax = if az[0].abs() < 1.0 / 64.0 && az[1].abs() < 1.0 / 64.0 { cross([0.0, 1.0, 0.0], az) } else { cross([0.0, 0.0, 1.0], az) };
    let n = hypot3(ax[0], ax[1], ax[2]);
    ax = ax.map(|v| v / n);
    let ay = cross(az, ax);
    Some([ax[0], ay[0], az[0], 0.0, ax[1], ay[1], az[1], 0.0, ax[2], ay[2], az[2], 0.0])
}
fn with_ocs(m: &M, ext: Option<&V>) -> M {
    match ocs(ext) {
        Some(o) => mul(m, &o),
        None => *m,
    }
}

// ---------------------------------------------------------------- curves → vertices

/// A JS loop count, capped (the JS would run out of memory first on such input).
fn count_of(n: f64) -> usize {
    if n.is_nan() { 0 } else { cc_civil::js::usize_of(n.min(MAX_VERTICES as f64 + 1.0)).unwrap_or(0) }
}

fn arc_pts(cx: f64, cy: f64, r: f64, a0: f64, a1: f64, z: f64) -> Vec<Pos> {
    let mut sweep = a1 - a0;
    let mut guard = 0u32;
    while sweep <= 0.0 && guard < 100_000 {
        sweep += TAU;
        guard = guard.saturating_add(1);
    }
    let n = max(2.0, (sweep / STEP).ceil());
    let mut out = Vec::new();
    if !(0.0 <= n) {
        return out;
    }
    let nn = count_of(n);
    for i in 0..=nn {
        let a = a0 + (sweep * i as f64) / n;
        out.push(vec![cx + r * a.jcos(), cy + r * a.jsin(), z]);
    }
    out
}

struct Vx {
    x: f64,
    y: f64,
    bulge: f64,
}
fn bulged_pts(vs: &[Vx], closed: bool, z: f64) -> Vec<Pos> {
    let mut out = Vec::new();
    let n = vs.len();
    for i in 0..n {
        let Some(a) = vs.get(i) else { break };
        out.push(vec![a.x, a.y, z]);
        if i.saturating_add(1) == n && !closed {
            break;
        }
        let Some(b) = vs.get(i.saturating_add(1).checked_rem(n).unwrap_or(0)) else { break };
        let bu = a.bulge;
        if bu == 0.0 || !bu.is_finite() {
            continue;
        }
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let c = hypot(dx, dy);
        if c == 0.0 || c.is_nan() {
            continue;
        }
        let theta = 4.0 * bu.jatan();
        let r = c / (2.0 * (theta / 2.0).jsin());
        let mx = (a.x + b.x) / 2.0;
        let my = (a.y + b.y) / 2.0;
        let h = r * (theta / 2.0).jcos();
        let cx = mx - (dy / c) * h;
        let cy = my + (dx / c) * h;
        let a0 = (a.y - cy).jatan2(a.x - cx);
        let steps = max(1.0, (theta.abs() / STEP).ceil());
        let zb = z;
        let sn = count_of(steps);
        for k in 1..sn {
            let ang = a0 + (theta * k as f64) / steps;
            let rr = r.abs();
            out.push(vec![cx + rr * ang.jcos(), cy + rr * ang.jsin(), z + ((zb - z) * k as f64) / steps]);
        }
    }
    out
}

fn pt3(q: Option<&V>) -> Pos {
    let o = q.and_then(V::as_obj);
    vec![num(o.and_then(|o| o.get("x"))), num(o.and_then(|o| o.get("y"))), or0(o.and_then(|o| o.get("z")))]
}
fn list<'a>(e: &'a Obj, k: &str) -> &'a [V] {
    e.get(k).and_then(V::as_arr).map_or(&[], Vec::as_slice)
}

/// `splinePts(sp)`: de Boor on the control points (rational too), else a smooth curve through the fit points.
pub fn spline_pts(sp: &Obj) -> Vec<Pos> {
    let p_list = list(sp, "control");
    let p = cc_civil::js::usize_of(int32(num(sp.get("degree")))).unwrap_or(0);
    let pi32 = int32(num(sp.get("degree")));
    let u: Vec<f64> = list(sp, "knots").iter().map(|v| num(Some(v))).collect();
    let np = p_list.len();
    if np as f64 > pi32 && pi32 >= 1.0 && u.len() == np.saturating_add(p).saturating_add(1) {
        let w: Option<Vec<f64>> = {
            let wl = list(sp, "weights");
            if !wl.is_empty() && wl.len() == np { Some(wl.iter().map(|v| num(Some(v))).collect()) } else { None }
        };
        let u0 = u.g(p);
        let u1 = u.g(np);
        let n = min(4000.0, max(16.0, (np.saturating_sub(p) as f64) * 32.0));
        let nn = count_of(n);
        let mut out = Vec::with_capacity(nn.saturating_add(1));
        let ctrl: Vec<Pos> = p_list.iter().map(|q| pt3(Some(q))).collect();
        for i in 0..=nn {
            let uu = if i == nn { u1 } else { u0 + ((u1 - u0) * i as f64) / n };
            let mut k = p;
            while k < np.saturating_sub(1) && uu >= u.g(k.saturating_add(1)) {
                k = k.saturating_add(1);
            }
            let mut d: Vec<[f64; 4]> = Vec::with_capacity(p.saturating_add(1));
            for j in 0..=p {
                let idx = k.saturating_add(j).saturating_sub(p);
                let q = ctrl.get(idx).cloned().unwrap_or_else(|| vec![f64::NAN; 3]);
                let wv = w.as_ref().map_or(1.0, |w| w.g(idx));
                d.push([q.g(0) * wv, q.g(1) * wv, q.g(2) * wv, wv]);
            }
            for r in 1..=p {
                let mut j = p;
                while j >= r {
                    let i0 = k.saturating_add(j).saturating_sub(p);
                    let den = u.g(i0.saturating_add(p).saturating_add(1).saturating_sub(r)) - u.g(i0);
                    let a = if den != 0.0 && !den.is_nan() { (uu - u.g(i0)) / den } else { 0.0 };
                    let prev = d.get(j.wrapping_sub(1)).copied().unwrap_or([f64::NAN; 4]);
                    if let Some(dj) = d.get_mut(j) {
                        for (x, pv) in dj.iter_mut().zip(prev) {
                            *x = (1.0 - a) * pv + a * *x;
                        }
                    }
                    if j == 0 {
                        break;
                    }
                    j = j.saturating_sub(1);
                }
            }
            let q = d.get(p).copied().unwrap_or([f64::NAN; 4]);
            out.push(vec![q[0] / q[3], q[1] / q[3], q[2] / q[3]]);
        }
        return out;
    }
    let f: Vec<Pos> = list(sp, "fit").iter().map(|q| pt3(Some(q))).collect();
    if f.len() == 2 {
        return f;
    }
    if f.len() > 2 {
        return through_fit(&f);
    }
    if np >= 2 { p_list.iter().map(|q| pt3(Some(q))).collect() } else { Vec::new() }
}

fn through_fit(f: &[Pos]) -> Vec<Pos> {
    let first = f.first().cloned().unwrap_or_default();
    let second = f.get(1).cloned().unwrap_or_default();
    let last = f.last().cloned().unwrap_or_default();
    let before_last = f.get(f.len().saturating_sub(2)).cloned().unwrap_or_default();
    let mut out = vec![first.clone()];
    let mut p: Vec<Pos> = Vec::with_capacity(f.len().saturating_add(2));
    p.push((0..3).map(|i| 2.0 * first.g(i) - second.g(i)).collect());
    p.extend(f.iter().cloned());
    p.push((0..3).map(|i| 2.0 * last.g(i) - before_last.g(i)).collect());
    let tj = |a: &Pos, b: &Pos| {
        let d = hypot3(b.g(0) - a.g(0), b.g(1) - a.g(1), b.g(2) - a.g(2)).sqrt();
        if d != 0.0 && !d.is_nan() { d } else { 1e-9 }
    };
    for i in 1..p.len().saturating_sub(2) {
        let (p0, p1, p2, p3) = (gp(&p, i.saturating_sub(1)), gp(&p, i), gp(&p, i.saturating_add(1)), gp(&p, i.saturating_add(2)));
        let t1 = tj(&p0, &p1);
        let t2 = t1 + tj(&p1, &p2);
        let t3 = t2 + tj(&p2, &p3);
        for k in 1..=8 {
            let u = t1 + ((t2 - t1) * f64::from(k)) / 8.0;
            let l = |a: &Pos, b: &Pos, ta: f64, tb: f64| -> Pos {
                (0..a.len()).map(|j| ((tb - u) * a.g(j) + (u - ta) * b.g(j)) / (tb - ta)).collect()
            };
            let a1 = l(&p0, &p1, 0.0, t1);
            let a2 = l(&p1, &p2, t1, t2);
            let a3 = l(&p2, &p3, t2, t3);
            let b1 = l(&a1, &a2, 0.0, t2);
            let b2 = l(&a2, &a3, t1, t3);
            out.push(l(&b1, &b2, t1, t2));
        }
    }
    out
}
fn gp(p: &[Pos], i: usize) -> Pos {
    p.get(i).cloned().unwrap_or_default()
}

struct Shape {
    parts: Vec<Vec<Pos>>,
    point: Option<Pos>,
    closed: bool,
    wcs: bool,
}

fn dedupe(r: Vec<Pos>) -> Vec<Pos> {
    let mut out: Vec<Pos> = Vec::new();
    for p in r {
        let same = out.last().is_some_and(|l| l.g(0) == p.g(0) && l.g(1) == p.g(1) && l.g(2) == p.g(2));
        if !same {
            out.push(p);
        }
    }
    if out.len() > 1 {
        let (a, b) = (out.first().cloned().unwrap_or_default(), out.last().cloned().unwrap_or_default());
        if a.g(0) == b.g(0) && a.g(1) == b.g(1) && a.g(2) == b.g(2) {
            out.pop();
        }
    }
    out
}

fn shape_of(e: &Obj, ty: &str) -> Option<Shape> {
    let sh = |parts: Vec<Vec<Pos>>, closed: bool, wcs: bool| Some(Shape { parts, point: None, closed, wcs });
    match ty {
        "LINE" => sh(vec![vec![pt3(get(e, "start")), pt3(get(e, "end"))]], false, true),
        "POINT" => Some(Shape { parts: Vec::new(), point: Some(pt3(get(e, "position"))), closed: false, wcs: true }),
        "CIRCLE" => {
            let c = get(e, "center").and_then(V::as_obj);
            let mut pts = arc_pts(
                num(c.and_then(|c| c.get("x"))),
                num(c.and_then(|c| c.get("y"))),
                num(get(e, "radius")),
                0.0,
                TAU,
                or0(c.and_then(|c| c.get("z"))),
            );
            pts.pop();
            sh(vec![pts], true, false)
        }
        "ARC" => {
            let c = get(e, "center").and_then(V::as_obj);
            sh(
                vec![arc_pts(
                    num(c.and_then(|c| c.get("x"))),
                    num(c.and_then(|c| c.get("y"))),
                    num(get(e, "radius")),
                    num(get(e, "startAngle")),
                    num(get(e, "endAngle")),
                    or0(c.and_then(|c| c.get("z"))),
                )],
                false,
                false,
            )
        }
        "ELLIPSE" => {
            let c = pt3(get(e, "center"));
            let mo = get(e, "majorAxis").and_then(V::as_obj);
            let (mx, my, mz) = (num(mo.and_then(|m| m.get("x"))), num(mo.and_then(|m| m.get("y"))), num(mo.and_then(|m| m.get("z"))));
            let r = num(get(e, "ratio"));
            let ext = get(e, "extrusion").and_then(V::as_obj);
            let nv = if ocs(get(e, "extrusion")).is_some() {
                [num(ext.and_then(|x| x.get("x"))), num(ext.and_then(|x| x.get("y"))), num(ext.and_then(|x| x.get("z")))]
            } else {
                [0.0, 0.0, 1.0]
            };
            let l = hypot3(nv[0], nv[1], nv[2]);
            let n = nv.map(|v| v / l);
            let mn = [n[1] * mz - n[2] * my, n[2] * mx - n[0] * mz, n[0] * my - n[1] * mx].map(|v| v * r);
            let s0 = or_null(get(e, "startParam"), 0.0);
            let mut s1 = or_null(get(e, "endParam"), TAU);
            let mut guard = 0u32;
            while s1 <= s0 && guard < 100_000 {
                s1 += TAU;
                guard = guard.saturating_add(1);
            }
            let full = (s1 - s0 - TAU).abs() < 1e-9;
            let k = max(4.0, ((s1 - s0) / STEP).ceil());
            let kk = count_of(k);
            let mut out = Vec::new();
            if 0.0 <= k {
                for i in 0..=kk {
                    let s = s0 + ((s1 - s0) * i as f64) / k;
                    out.push(vec![
                        c.g(0) + mx * s.jcos() + mn[0] * s.jsin(),
                        c.g(1) + my * s.jcos() + mn[1] * s.jsin(),
                        c.g(2) + mz * s.jcos() + mn[2] * s.jsin(),
                    ]);
                }
            }
            if full {
                out.pop();
            }
            sh(vec![out], full, true)
        }
        "LWPOLYLINE" | "POLYLINE_2D" => {
            let z = or0(get(e, "elevation"));
            let vs: Vec<Vx> = list(e, "vertices")
                .iter()
                .filter_map(V::as_obj)
                .filter(|v| ty == "LWPOLYLINE" || !flag(num(v.get("flags")), 16))
                .map(|v| Vx { x: num(v.get("x")), y: num(v.get("y")), bulge: or0(v.get("bulge")) })
                .collect();
            let closed = get(e, "closed").is_some_and(V::truthy);
            sh(vec![bulged_pts(&vs, closed, z)], closed, false)
        }
        "POLYLINE_3D" => {
            let vs: Vec<Pos> = list(e, "vertices")
                .iter()
                .filter(|v| v.as_obj().is_none_or(|o| !flag(num(o.get("flags")), 16)))
                .map(|v| pt3(Some(v)))
                .collect();
            sh(vec![vs], get(e, "closed").is_some_and(V::truthy), true)
        }
        "SPLINE" => sh(vec![spline_pts(e)], get(e, "closed").is_some_and(V::truthy), true),
        "SOLID" | "TRACE" => {
            let el = or0(get(e, "elevation"));
            let q: Vec<Pos> = list(e, "points")
                .iter()
                .map(|p| {
                    let o = p.as_obj();
                    let z = if el != 0.0 { el } else { or0(o.and_then(|o| o.get("z"))) };
                    vec![num(o.and_then(|o| o.get("x"))), num(o.and_then(|o| o.get("y"))), z]
                })
                .collect();
            if q.len() != 4 {
                return None;
            }
            let ring = vec![gp(&q, 0), gp(&q, 1), gp(&q, 3), gp(&q, 2)];
            sh(vec![dedupe(ring)], true, false)
        }
        "3DFACE" => {
            let q: Vec<Pos> = list(e, "points").iter().map(|p| pt3(Some(p))).collect();
            if q.len() >= 3 { sh(vec![dedupe(q)], true, true) } else { None }
        }
        _ => None,
    }
}

const LINEWORK: [&str; 14] = [
    "LINE",
    "POINT",
    "CIRCLE",
    "ARC",
    "ELLIPSE",
    "LWPOLYLINE",
    "POLYLINE_2D",
    "POLYLINE_3D",
    "SPLINE",
    "SOLID",
    "TRACE",
    "3DFACE",
    "INSERT",
    "MINSERT",
];

/// One linework layer of the result.
#[derive(Clone, Debug)]
pub struct CadLayer {
    pub cad_layer: String,
    pub name: String,
    pub geom: &'static str,
    pub has_z: bool,
    pub features: Vec<Feature>,
    pub file: String,
}
impl J for CadLayer {
    fn j(&self) -> Value {
        let fields = vec![
            Field { name: "LAYER".into(), ty: "text".into() },
            Field { name: "TYPE".into(), ty: "text".into() },
            Field { name: "ELEV".into(), ty: "number".into() },
        ];
        cc_civil::jo! {
            "cadLayer" => self.cad_layer, "name" => self.name, "geom" => self.geom, "hasZ" => self.has_z, "fields" => fields,
            "features" => self.features, "source" => cc_civil::jo! { "file" => self.file, "format" => "CAD", "crs" => Value::Null },
        }
    }
}
/// A CAD layer's summary.
#[derive(Clone, Debug)]
pub struct CadLayerInfo {
    pub name: String,
    pub on: bool,
    pub frozen: bool,
    pub lines: usize,
    pub polygons: usize,
    pub points: usize,
    pub z: bool,
}
impl J for CadLayerInfo {
    fn j(&self) -> Value {
        cc_civil::jo! { "name" => self.name, "on" => self.on, "frozen" => self.frozen, "lines" => self.lines, "polygons" => self.polygons, "points" => self.points, "z" => self.z }
    }
}
/// cadLinework's result.
#[derive(Clone, Debug)]
pub struct Linework {
    pub layers: Vec<CadLayer>,
    pub cad_layers: Vec<CadLayerInfo>,
    pub skipped: Vec<(String, usize)>,
    pub units: CadUnits,
    pub scale: f64,
    pub notes: Vec<String>,
}
impl J for Linework {
    fn j(&self) -> Value {
        let mut sk = Map::new();
        for (k, n) in &self.skipped {
            sk.insert(k.clone(), Value::from(*n as u64));
        }
        cc_civil::jo! { "layers" => self.layers, "cadLayers" => self.cad_layers, "skipped" => Value::Object(sk), "units" => self.units, "scale" => self.scale, "notes" => self.notes }
    }
}

#[derive(Default)]
struct ByLayer {
    lines: Vec<Feature>,
    polygons: Vec<Feature>,
    points: Vec<Feature>,
}

struct Walker<'a> {
    blocks: std::collections::HashMap<String, &'a Obj>,
    keep: Option<Vec<String>>,
    by: Vec<(String, ByLayer)>,
    skipped: Vec<(String, usize)>,
    vertices: usize,
    depth_hit: usize,
    cut: bool,
}
impl Walker<'_> {
    fn skip(&mut self, k: &str) {
        match self.skipped.iter_mut().find(|(a, _)| a == k) {
            Some(s) => s.1 = s.1.saturating_add(1),
            None => self.skipped.push((k.to_string(), 1)),
        }
    }
    fn add(&mut self, layer: &str, kind: u8, f: Feature) {
        let i = match self.by.iter().position(|(n, _)| n == layer) {
            Some(i) => i,
            None => {
                self.by.push((layer.to_string(), ByLayer::default()));
                self.by.len().saturating_sub(1)
            }
        };
        if let Some((_, b)) = self.by.get_mut(i) {
            match kind {
                0 => b.lines.push(f),
                1 => b.polygons.push(f),
                _ => b.points.push(f),
            }
        }
    }
}

fn props(layer: &str, ty: &str, pts: &[[f64; 3]]) -> Map<String, Value> {
    let z0 = pts.first().map_or(f64::NAN, |p| p[2]);
    let same = !pts.is_empty() && pts.iter().all(|p| (p[2] - z0).abs() <= 1e-9 * max(1.0, z0.abs()));
    let mut m = Map::new();
    m.insert("LAYER".into(), Value::String(layer.into()));
    m.insert("TYPE".into(), Value::String(ty.into()));
    m.insert("ELEV".into(), if same && z0.is_finite() && z0 != 0.0 { crate::gjs::num_keep(z0) } else { Value::Null });
    m
}
fn shape(pts: &[[f64; 3]]) -> Vec<Pos> {
    let flat = pts.iter().all(|p| p[2] == 0.0 || p[2].abs() < 1e-12);
    if flat { pts.iter().map(|p| vec![p[0], p[1]]).collect() } else { pts.iter().map(|p| p.to_vec()).collect() }
}

fn walk(w: &mut Walker, ents: &[V], m: &M, inherit: Option<&str>, depth: usize) {
    for ev in ents {
        if w.cut {
            return;
        }
        let Some(e) = ev.as_obj() else { continue };
        if e.get("invisible").is_some_and(V::truthy) {
            continue;
        }
        let elayer = e.get("layer");
        let layer = match (elayer, inherit) {
            (Some(V::Str(s)), Some(inh)) if s == "0" => inh.to_string(),
            (None | Some(V::Null), _) => "0".to_string(),
            (Some(v), _) => match v {
                V::Str(s) => s.clone(),
                V::Num(n) => num_to_string(*n),
                V::Bool(b) => b.to_string(),
                _ => "[object Object]".into(),
            },
        };
        let ty = match e.get("type") {
            Some(V::Str(s)) => s.clone(),
            None => "undefined".into(),
            Some(V::Num(n)) => num_to_string(*n),
            Some(_) => "[object]".into(),
        };
        if !LINEWORK.contains(&ty.as_str()) {
            w.skip(&ty);
            continue;
        }
        if ty == "INSERT" || ty == "MINSERT" {
            let bname = match e.get("block") {
                Some(V::Str(s)) => s.to_uppercase(),
                Some(V::Num(n)) if *n != 0.0 && !n.is_nan() => num_to_string(*n).to_uppercase(),
                _ => String::new(),
            };
            let Some(b) = w.blocks.get(&bname).copied() else {
                w.skip("INSERT_NO_BLOCK");
                continue;
            };
            if b.get("xrefPath").is_some_and(V::truthy) || flag(num(b.get("flags")), 4) {
                w.skip("XREF");
                continue;
            }
            if depth >= MAX_DEPTH {
                w.depth_hit = w.depth_hit.saturating_add(1);
                continue;
            }
            let s = e.get("scale").filter(|v| v.truthy()).and_then(V::as_obj);
            let (sx, sy, sz) = match s {
                Some(s) => (or_null(s.get("x"), 1.0), or_null(s.get("y"), 1.0), or_null(s.get("z"), 1.0)),
                None => (1.0, 1.0, 1.0),
            };
            let r = or0(e.get("rotation"));
            let ins = e.get("insert").filter(|v| v.truthy()).and_then(V::as_obj);
            let (ix, iy, iz) = match ins {
                Some(o) => (num(o.get("x")), num(o.get("y")), or0(o.get("z"))),
                None => (0.0, 0.0, 0.0),
            };
            let base = b.get("base").filter(|v| v.truthy()).and_then(V::as_obj);
            let (bx, by, bz) = match base {
                Some(o) => (num(o.get("x")), num(o.get("y")), or0(o.get("z"))),
                None => (0.0, 0.0, 0.0),
            };
            let cnt = |k: &str| -> usize {
                if ty != "MINSERT" {
                    return 1;
                }
                let v = int32(num(e.get(k)));
                let v = if v != 0.0 { v } else { 1.0 };
                cc_civil::js::usize_of(max(1.0, min(1000.0, v))).unwrap_or(1)
            };
            let (cols, rows) = (cnt("cols"), cnt("rows"));
            let (c, sn) = (r.jcos(), r.jsin());
            let children = b.get("entities").and_then(V::as_arr).map_or(&[][..], Vec::as_slice);
            for ci in 0..cols {
                for ri in 0..rows {
                    let step = translate(ci as f64 * or0(e.get("colSpacing")), ri as f64 * or0(e.get("rowSpacing")), 0.0);
                    let rot = [c, -sn, 0.0, 0.0, sn, c, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
                    let scl = [sx, 0.0, 0.0, 0.0, 0.0, sy, 0.0, 0.0, 0.0, 0.0, sz, 0.0];
                    let inner = mul(&rot, &mul(&step, &mul(&scl, &translate(-bx, -by, -bz))));
                    let mm = mul(&mul(&with_ocs(m, e.get("extrusion")), &translate(ix, iy, iz)), &inner);
                    walk(w, children, &mm, Some(&layer), depth.saturating_add(1));
                    if w.cut {
                        return;
                    }
                }
            }
            continue;
        }
        let Some(sh) = shape_of(e, &ty) else {
            w.skip(&ty);
            continue;
        };
        let mm = if sh.wcs { *m } else { with_ocs(m, e.get("extrusion")) };
        if let Some(k) = &w.keep
            && !k.contains(&layer)
        {
            continue;
        }
        if let Some(p) = &sh.point {
            let q = apply(&mm, p.g(0), p.g(1), p.g(2));
            if !q.iter().all(|v| v.is_finite()) {
                continue;
            }
            let g = Geom::Pt(shape(&[q]).into_iter().next().unwrap_or_default());
            let f = Feature { g: Some(g), p: props(&layer, &ty, &[q]) };
            w.add(&layer, 2, f);
            w.vertices = w.vertices.saturating_add(1);
            continue;
        }
        for part in &sh.parts {
            let pts: Vec<[f64; 3]> =
                part.iter().map(|q| apply(&mm, q.g(0), q.g(1), q.g(2))).filter(|p| p.iter().all(|v| v.is_finite())).collect();
            if pts.len() < 2 || (sh.closed && pts.len() < 3) {
                continue;
            }
            w.vertices = w.vertices.saturating_add(pts.len());
            if w.vertices > MAX_VERTICES {
                w.cut = true;
                return;
            }
            let s2 = shape(&pts);
            if sh.closed {
                let mut ring = s2.clone();
                if let Some(f0) = s2.first() {
                    ring.push(f0.clone());
                }
                w.add(&layer, 1, Feature { g: Some(Geom::Pg(vec![vec![ring]])), p: props(&layer, &ty, &pts) });
            } else {
                w.add(&layer, 0, Feature { g: Some(Geom::Ln(vec![s2])), p: props(&layer, &ty, &pts) });
            }
        }
    }
}

fn has_z(fs: &[Feature]) -> bool {
    fs.iter().any(|f| f.g.as_ref().is_some_and(crate::gis_geom::has_z))
}

/// `cadLinework(model, { file, projectUnit, scale, layers })`.
pub fn cad_linework(model: &Obj, file: &str, project_unit: &str, scale: Option<f64>, only: Option<&[String]>) -> Linework {
    let mut notes = Vec::new();
    let header = model.get("header").and_then(V::as_obj);
    let us = unit_scale(header, project_unit);
    let k = match scale {
        Some(s) if s.is_finite() && s > 0.0 => s,
        _ => us.scale,
    };
    if scale.is_none()
        && let Some(n) = &us.note
    {
        notes.push(n.clone());
    }
    let mut blocks = std::collections::HashMap::new();
    for b in model.get("blocks").and_then(V::as_arr).map_or(&[][..], Vec::as_slice) {
        if let Some(o) = b.as_obj() {
            let name = match o.get("name") {
                Some(V::Str(s)) => s.to_uppercase(),
                Some(V::Num(n)) => num_to_string(*n),
                None => "UNDEFINED".into(),
                Some(_) => "[OBJECT OBJECT]".into(),
            };
            blocks.insert(name, o);
        }
    }
    let Some(ms) = blocks.get("*MODEL_SPACE").copied() else {
        return Linework {
            layers: Vec::new(),
            cad_layers: Vec::new(),
            skipped: Vec::new(),
            units: cad_units(header),
            scale: k,
            notes: vec![tr!("cad.no.model")],
        };
    };
    let mut table: Vec<(String, &Obj)> = Vec::new();
    for l in model.get("tables").and_then(V::as_obj).and_then(|t| t.get("layers")).and_then(V::as_arr).map_or(&[][..], Vec::as_slice) {
        if let Some(o) = l.as_obj() {
            let n = match o.get("name") {
                Some(V::Str(s)) => s.clone(),
                _ => continue,
            };
            match table.iter_mut().find(|(a, _)| *a == n) {
                Some(slot) => slot.1 = o,
                None => table.push((n, o)),
            }
        }
    }
    let mut w =
        Walker { blocks, keep: only.map(<[String]>::to_vec), by: Vec::new(), skipped: Vec::new(), vertices: 0, depth_hit: 0, cut: false };
    let top = [k, 0.0, 0.0, 0.0, 0.0, k, 0.0, 0.0, 0.0, 0.0, k, 0.0];
    let ents = ms.get("entities").and_then(V::as_arr).map_or(&[][..], Vec::as_slice);
    walk(&mut w, ents, &top, None, 0);
    if w.depth_hit > 0 {
        notes.push(tr!("cad.depth", n = w.depth_hit, max = MAX_DEPTH));
    }
    if w.cut {
        notes.push(tr!("cad.cut", max = cc_civil::units::grouped(MAX_VERTICES as f64, 0)));
    }
    let skipped_n: usize = w.skipped.iter().map(|(_, n)| *n).sum();
    if skipped_n > 0 {
        let list: Vec<String> = w.skipped.iter().map(|(k2, v)| format!("{k2} {v}")).collect();
        notes.push(tr!("cad.skipped", n = skipped_n, list = list.join(", ")));
    }
    let mut names: Vec<String> = w.by.iter().map(|(n, _)| n.clone()).collect();
    cc_civil::js::sort_by(&mut names, |a, b| locale_compare(a, b));
    let mut out = Vec::new();
    let mut cad_layers = Vec::new();
    for name in names {
        let Some(pos) = w.by.iter().position(|(n, _)| *n == name) else { continue };
        let b = std::mem::take(&mut w.by.get_mut(pos).map(|x| &mut x.1).map(std::mem::take).unwrap_or_default());
        let tl = table.iter().find(|(n, _)| *n == name).map(|(_, o)| *o);
        let z = has_z(&b.lines) || has_z(&b.polygons) || has_z(&b.points);
        let on = match tl {
            Some(l) => {
                l.get("on") != Some(&V::Bool(false)) && !l.get("off").is_some_and(V::truthy) && !l.get("frozen").is_some_and(V::truthy)
            }
            None => true,
        };
        cad_layers.push(CadLayerInfo {
            name: name.clone(),
            on,
            frozen: tl.is_some_and(|l| l.get("frozen").is_some_and(V::truthy)),
            lines: b.lines.len(),
            polygons: b.polygons.len(),
            points: b.points.len(),
            z,
        });
        let kinds: Vec<(Vec<Feature>, &'static str, &'static str)> =
            [(b.lines, "ln", ""), (b.polygons, "pg", "cad.closed"), (b.points, "pt", "cad.points")]
                .into_iter()
                .filter(|(f, _, _)| !f.is_empty())
                .collect();
        let many = kinds.len() > 1;
        for (fs, geom, suffix) in kinds {
            let nm =
                if many && !suffix.is_empty() { cc_civil::i18n::t(suffix, &[("layer", F::from(name.as_str()))]) } else { name.clone() };
            out.push(CadLayer { cad_layer: name.clone(), name: nm, geom, has_z: has_z(&fs), features: fs, file: file.to_string() });
        }
    }
    Linework { layers: out, cad_layers, skipped: w.skipped, units: cad_units(header), scale: k, notes }
}
