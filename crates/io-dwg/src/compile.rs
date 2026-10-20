//! The drawing model → the 2D display list: a line-by-line port of BimCraft's `dwg-compile.js` (`compileDrawing`).
//!
//! `compile_drawing(model, version_tag, xrefs)` turns a model (from [`crate::read_dwg`] or
//! [`crate::dxf_model::read_dxf_model`]) into a [`cc_drawing::Drawing`]: spaces (model first, then layouts in tab
//! order) of batches per layer × colour × line weight, each with polylines, fills and texts in the space's world
//! coordinates. Block inserts are expanded with their transforms (layer "0" and BYBLOCK colour inherit from the
//! insert, as in AutoCAD); dimensions and tables draw from their anonymous blocks. Unsupported entity types are
//! counted, never guessed at.
//!
//! JS semantics kept on purpose: `||` / `??` defaults, `undefined` → NaN in arithmetic, `Math.min/max` NaN
//! propagation, `Math.hypot` (V8's Kahan-compensated form), UTF-16 string lengths and `trim()`'s whitespace set, the
//! exact order of floating-point operations, the JS `Map` insertion order of batches, layers and images, and
//! `localeCompare(…, {numeric: true})` for the layer order (an ICU-root-like natural sort, see [`collate`]).
//! What JS would throw on (a missing point, runaway recursion) never panics here: missing numbers are NaN and the
//! recursion is bounded.

use std::collections::{HashMap, HashSet};
use std::f64::consts::TAU;
use std::rc::Rc;

use cc_drawing::{Batch, Bounds, Color, Drawing, Fill, ImageRef, Layer, Space, SpaceImage, SpaceKind, Stats, Text, Viewport, XrefInfo};

use crate::read::js_string;
use crate::util::to_i32;
use crate::value::{Obj, V};

const MAX_PRIMS: u64 = 3_000_000; // safety stop for runaway nesting
/// Bounds the recursion of tables / dimensions / multileader blocks (JS has no limit there and would overflow its
/// stack — such a file cannot be compiled by the JS at all).
const MAX_DEPTH: u32 = 1000;
/// The model's line weights are 1/100 mm (−1 by layer, −2 by block, −3 default) → mm.
pub const LW_DEFAULT: f64 = 0.25;
const LW_BYLAYER: f64 = -1.0;
const LW_BYBLOCK: f64 = -2.0;

type M = [f64; 6];
const IDENTITY: M = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `VERSIONS[tag]`
pub fn release_of(tag: &str) -> &'static str {
    match tag {
        "AC1009" => "R12",
        "AC1012" => "R13",
        "AC1014" => "R14",
        "AC1015" => "2000",
        "AC1018" => "2004",
        "AC1021" => "2007",
        "AC1024" => "2010",
        "AC1027" => "2013",
        "AC1032" => "2018",
        _ => "",
    }
}
/// `UNITS[key] || ''` (key = `String(INSUNITS)`).
fn units_of(v: Option<&V>) -> &'static str {
    let Some(v) = v else { return "" };
    match js_string(v).as_str() {
        "1" => "inches",
        "2" => "feet",
        "3" => "miles",
        "4" => "millimeters",
        "5" => "centimeters",
        "6" => "meters",
        "7" => "kilometers",
        "8" => "microinches",
        "9" => "mils",
        "10" => "yards",
        _ => "",
    }
}

// ---------------------------------------------------------------------------------------------------------------
// JS value helpers

/// JS `ToNumber` for the values a model holds (`undefined` → NaN).
fn num(v: Option<&V>) -> f64 {
    match v {
        None => f64::NAN,
        Some(V::Num(n)) => *n,
        Some(V::Bool(b)) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Some(V::Null) => 0.0,
        Some(V::Str(s)) => {
            let t = s.trim_matches(|c: char| u16::try_from(u32::from(c)).is_ok_and(js_space));
            if t.is_empty() { 0.0 } else { t.parse::<f64>().unwrap_or(f64::NAN) }
        }
        Some(V::Arr(_) | V::Obj(_) | V::Bytes(_)) => f64::NAN,
    }
}
fn truthy(v: Option<&V>) -> bool {
    v.is_some_and(V::truthy)
}
/// `v || d` in a numeric context.
fn or_n(v: Option<&V>, d: f64) -> f64 {
    if truthy(v) { num(v) } else { d }
}
/// `v ?? d` in a numeric context.
fn nn_n(v: Option<&V>, d: f64) -> f64 {
    match v {
        None | Some(V::Null) => d,
        _ => num(v),
    }
}
/// `o.k` (an object field of an optional object).
fn f<'a>(o: Option<&'a Obj>, k: &str) -> Option<&'a V> {
    o.and_then(|o| o.get(k))
}
fn fo<'a>(o: Option<&'a Obj>, k: &str) -> Option<&'a Obj> {
    f(o, k).and_then(V::as_obj)
}
/// `p.x`, `p.y` of a point value (missing → NaN).
fn pxy(p: Option<&V>) -> (f64, f64) {
    let o = p.and_then(V::as_obj);
    (num(f(o, "x")), num(f(o, "y")))
}
fn arr<'a>(o: Option<&'a Obj>, k: &str) -> &'a [V] {
    f(o, k).and_then(V::as_arr).map(Vec::as_slice).unwrap_or(&[])
}
/// `x & bit` with JS ToInt32.
fn bit(v: Option<&V>, b: i32) -> bool {
    to_i32(num(v)) & b != 0
}
fn is_num(v: Option<&V>, n: f64) -> bool {
    matches!(v, Some(V::Num(x)) if *x == n)
}

/// `Math.min` / `Math.max` (NaN propagates).
fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }
}
fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) }
}
/// V8's `Math.hypot(a, b)`: Kahan-compensated sum of squares scaled by the largest argument.
pub fn js_hypot(a: f64, b: f64) -> f64 {
    let (a, b) = (a.abs(), b.abs());
    if a.is_infinite() || b.is_infinite() {
        return f64::INFINITY;
    }
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    let max = a.max(b);
    if max == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut comp = 0.0f64;
    for v in [a, b] {
        let r = v / max;
        let s = r * r - comp;
        let p = sum + s;
        comp = (p - sum) - s;
        sum = p;
    }
    sum.sqrt() * max
}
/// `Math.round` (ties toward +∞).
fn js_round(x: f64) -> f64 {
    let r = x.floor();
    if x - r >= 0.5 { r + 1.0 } else { r }
}
/// JS `WhiteSpace` + `LineTerminator` (what `String.prototype.trim` removes), per UTF-16 unit.
pub fn js_space(u: u16) -> bool {
    matches!(u, 0x09..=0x0D | 0x20 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000 | 0xFEFF)
}
fn blank16(s: &[u16]) -> bool {
    s.iter().all(|&u| js_space(u))
}
fn u16s(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}
fn usize_f(n: usize) -> f64 {
    crate::util::usize_to_f64(n)
}

// ---------------------------------------------------------------------------------------------------------------
// Colour

/// `'#' + (n & 0xffffff).toString(16).padStart(6, '0')`
fn rgb_hex(n: f64) -> Color {
    Color::Rgb(crate::util::u32_from_i32(to_i32(n) & 0xFF_FFFF))
}
fn hsv(h: f64, s: f64, v: f64) -> Color {
    let f = |n: f64| {
        let k = (n + h / 60.0) % 6.0;
        v - v * s * js_max(0.0, js_min(js_min(k, 4.0 - k), 1.0))
    };
    let mut out = 0u32;
    for x in [f(5.0), f(3.0), f(1.0)] {
        let c = js_round(x * 255.0);
        let c = if (0.0..=255.0).contains(&c) { crate::util::to_u32(c) } else { 0 };
        out = (out << 8) | c;
    }
    Color::Rgb(out)
}
/// AutoCAD Color Index → `Ink` for 7 (and 255, ≤ 0, > 255), else a true colour.
pub fn aci_color(i: f64) -> Color {
    if i == 7.0 || i == 255.0 || i <= 0.0 || i > 255.0 {
        return Color::Ink;
    }
    let base = match i {
        1.0 => Some(0xff0000),
        2.0 => Some(0xffff00),
        3.0 => Some(0x00ff00),
        4.0 => Some(0x00ffff),
        5.0 => Some(0x0000ff),
        6.0 => Some(0xff00ff),
        8.0 => Some(0x808080),
        9.0 => Some(0xc0c0c0),
        _ => None,
    };
    if let Some(b) = base {
        return Color::Rgb(b);
    }
    if i >= 250.0 {
        let g = [0x333333, 0x505050, 0x696969, 0x828282, 0xbebebe];
        // i is 250..=254 here (255 is ink); a fraction has no entry in the JS table either (→ undefined).
        let k = crate::util::to_usize(i - 250.0);
        return Color::Rgb(g.get(k).copied().unwrap_or(0));
    }
    let hue = ((i - 10.0) / 10.0).floor() * 15.0;
    let j = (i - 10.0) % 10.0;
    let vs = [1.0, 1.0, 0.8, 0.8, 0.6, 0.6, 0.5, 0.5, 0.3, 0.3];
    let v = if j >= 0.0 && j == j.trunc() { vs.get(crate::util::to_usize(j)).copied().unwrap_or(f64::NAN) } else { f64::NAN };
    let s = if j % 2.0 == 0.0 { 1.0 } else { 0.5 };
    hsv(hue, s, v)
}
/// A table layer's colour.
fn layer_color(l: &Obj) -> Color {
    // `l.color || {index: 7}`; a colour that is not an object has neither rgb nor index → 7
    match l.obj("color") {
        Some(c) if c.has("rgb") => rgb_hex(num(c.get("rgb"))),
        Some(c) => aci_color(nn_n(c.get("index"), 7.0).abs()),
        None => aci_color(7.0),
    }
}
/// A layer prints unless its plot flag is off; DEFPOINTS never prints.
fn layer_plots(l: &Obj) -> bool {
    let name = if l.truthy("name") { l.get("name").map(js_string).unwrap_or_default() } else { String::new() };
    !matches!(l.get("plot"), Some(V::Bool(false))) && !name.eq_ignore_ascii_case("defpoints")
}
fn lw_of(v: f64) -> f64 {
    if (0.0..=211.0).contains(&v) { v / 100.0 } else { LW_DEFAULT }
}

// ---------------------------------------------------------------------------------------------------------------
// Geometry

fn translate(x: f64, y: f64) -> M {
    [1.0, 0.0, 0.0, 1.0, x, y]
}
fn scale(sx: f64, sy: f64) -> M {
    [sx, 0.0, 0.0, sy, 0.0, 0.0]
}
fn rotate(r: f64) -> M {
    let (c, s) = (r.cos(), r.sin());
    [c, s, -s, c, 0.0, 0.0]
}
fn mul(p: &M, q: &M) -> M {
    let [p0, p1, p2, p3, p4, p5] = *p;
    let [q0, q1, q2, q3, q4, q5] = *q;
    [p0 * q0 + p2 * q1, p1 * q0 + p3 * q1, p0 * q2 + p2 * q3, p1 * q2 + p3 * q3, p0 * q4 + p2 * q5 + p4, p1 * q4 + p3 * q5 + p5]
}
/// Entities drawn with a -Z extrusion are mirrored in X (arbitrary-axis rule, plan views).
fn ocs(m: &M, ext: Option<&V>) -> M {
    let ext = ext.filter(|v| v.truthy()).and_then(V::as_obj);
    if ext.is_some() && num(f(ext, "z")) < -0.5 { mul(m, &scale(-1.0, 1.0)) } else { *m }
}
fn tx(m: &M, pts: &[f64]) -> Vec<f64> {
    let mut out = Vec::with_capacity(pts.len());
    for c in pts.chunks(2) {
        let x = c.first().copied().unwrap_or(f64::NAN);
        let y = c.get(1).copied().unwrap_or(f64::NAN);
        out.push(m[0] * x + m[2] * y + m[4]);
        if c.len() > 1 {
            out.push(m[1] * x + m[3] * y + m[5]);
        }
    }
    out
}

/// Segments for an arc: about 5° each, 8 to 128 (NaN when the sweep is NaN, as in JS).
fn seg_count(sweep: f64) -> f64 {
    js_max(8.0, js_min(128.0, (sweep.abs() / TAU * 72.0).ceil()))
}
/// `while (sweep <= 0) sweep += TAU;` (bounded: JS would spin forever on −∞).
fn wrap_sweep(mut sweep: f64) -> f64 {
    let mut guard = 0u32;
    while sweep <= 0.0 {
        let next = sweep + TAU;
        if next == sweep || guard > 10_000_000 {
            break;
        }
        sweep = next;
        guard = guard.saturating_add(1);
    }
    sweep
}

fn arc_points(cx: f64, cy: f64, r: f64, a0: f64, a1: f64, ccw: bool) -> Vec<f64> {
    let mut sweep = wrap_sweep(if ccw { a1 - a0 } else { a0 - a1 });
    if a1 - a0 == TAU {
        sweep = TAU;
    }
    let n = seg_count(sweep);
    let dir = if ccw { 1.0 } else { -1.0 };
    let mut pts = Vec::new();
    let mut i = 0.0;
    while i <= n {
        let a = a0 + dir * sweep * (i / n);
        pts.push(cx + r * a.cos());
        pts.push(cy + r * a.sin());
        i += 1.0;
    }
    pts
}

fn ellipse_points(c: Option<&V>, maj: Option<&V>, ratio: f64, p0: f64, p1: f64) -> Vec<f64> {
    let (mx, my) = pxy(maj);
    let (ccx, ccy) = pxy(c);
    let len = js_hypot(mx, my);
    let rot = my.atan2(mx);
    let sweep = wrap_sweep(p1 - p0);
    let n = seg_count(sweep);
    let (cr, sr) = (rot.cos(), rot.sin());
    let mut pts = Vec::new();
    let mut i = 0.0;
    while i <= n {
        let t = p0 + sweep * (i / n);
        let x = len * t.cos();
        let y = len * ratio * t.sin();
        pts.push(ccx + x * cr - y * sr);
        pts.push(ccy + x * sr + y * cr);
        i += 1.0;
    }
    pts
}

/// Vertices with bulges (LWPOLYLINE, POLYLINE2D, hatch polyline paths) → points.
fn bulge_polyline(vs: &[&V], closed: bool) -> Vec<f64> {
    let mut pts = Vec::new();
    let n = vs.len();
    for i in 0..n {
        let a = vs.get(i).copied();
        let b = vs.get(i.saturating_add(1).checked_rem(n).unwrap_or(0)).copied();
        let (ax, ay) = pxy(a);
        pts.push(ax);
        pts.push(ay);
        if i.saturating_add(1) == n && !closed {
            break;
        }
        let (bx, by) = pxy(b);
        let bulge = or_n(f(a.and_then(V::as_obj), "bulge"), 0.0);
        if bulge.abs() > 1e-9 {
            let theta = 4.0 * bulge.atan();
            let chord = js_hypot(bx - ax, by - ay);
            if chord > 0.0 {
                let r = chord / (2.0 * (theta / 2.0).sin());
                let (mx, my) = ((ax + bx) / 2.0, (ay + by) / 2.0);
                let h = r * (theta / 2.0).cos();
                let ux = -(by - ay) / chord;
                let uy = (bx - ax) / chord;
                let (cx, cy) = (mx + ux * h, my + uy * h);
                let a0 = (ay - cy).atan2(ax - cx);
                let steps = seg_count(theta);
                let mut k = 1.0;
                while k < steps {
                    let t = a0 + theta * (k / steps);
                    pts.push(cx + r.abs() * t.cos());
                    pts.push(cy + r.abs() * t.sin());
                    k += 1.0;
                }
            }
        }
        if i.saturating_add(1) == n && closed {
            pts.push(bx);
            pts.push(by);
        }
    }
    pts
}
fn refs(a: &[V]) -> Vec<&V> {
    a.iter().collect()
}
fn flat_xy<'a>(it: impl Iterator<Item = Option<&'a V>>) -> Vec<f64> {
    let mut out = Vec::new();
    for p in it {
        let (x, y) = pxy(p);
        out.push(x);
        out.push(y);
    }
    out
}

/// Fit points when present, else the B-spline evaluated from its control points (de Boor).
fn spline_points(e: &Obj) -> Vec<f64> {
    let fit = e.arr("fit");
    if e.truthy("fit")
        && let Some(fit) = fit
        && fit.len() > 1
    {
        return flat_xy(fit.iter().map(Some));
    }
    let cp: &[V] = if e.truthy("control") { e.arr("control").map(Vec::as_slice).unwrap_or(&[]) } else { &[] };
    let k: Vec<f64> =
        if e.truthy("knots") { e.arr("knots").map(|a| a.iter().map(|v| num(Some(v))).collect()).unwrap_or_default() } else { Vec::new() };
    let p = or_n(e.get("degree"), 3.0);
    if cp.len() < 2 {
        return Vec::new();
    }
    if usize_f(k.len()) != usize_f(cp.len()) + p + 1.0 {
        return flat_xy(cp.iter().map(Some));
    }
    // here p is a whole number (lengths are); a negative one makes the JS throw — nothing to draw.
    if p < 0.0 {
        return Vec::new();
    }
    let p = crate::util::to_usize(p);
    let w: Option<Vec<f64>> =
        e.arr("weights").filter(|w| e.truthy("weights") && w.len() == cp.len()).map(|w| w.iter().map(|v| num(Some(v))).collect());
    let kk = |i: usize| k.get(i).copied().unwrap_or(f64::NAN);
    let lo = kk(p);
    let hi = kk(k.len().saturating_sub(p).saturating_sub(1));
    let n = 16usize.max(cp.len().saturating_mul(8));
    let nf = usize_f(n);
    let mut pts = Vec::with_capacity(n.saturating_add(1).saturating_mul(2));
    let last = cp.len().saturating_sub(1);
    for s in 0..=n {
        let sf = usize_f(s);
        let u = if s == n { hi - 1e-9 * (hi - lo) } else { lo + (hi - lo) * (sf / nf) };
        let mut span = p;
        while span < last && u >= kk(span.saturating_add(1)) {
            span = span.saturating_add(1);
        }
        let base = span.saturating_sub(p);
        let mut d: Vec<[f64; 3]> = Vec::with_capacity(p.saturating_add(1));
        for j in 0..=p {
            let q = cp.get(base.saturating_add(j));
            let wt = w.as_ref().map_or(1.0, |w| w.get(base.saturating_add(j)).copied().unwrap_or(f64::NAN));
            let (qx, qy) = pxy(q);
            d.push([qx * wt, qy * wt, wt]);
        }
        for r in 1..=p {
            for j in (r..=p).rev() {
                let i = base.saturating_add(j);
                let den = kk(i.saturating_add(p).saturating_sub(r).saturating_add(1)) - kk(i);
                let a = if den == 0.0 { 0.0 } else { (u - kk(i)) / den };
                let prev = d.get(j.saturating_sub(1)).copied().unwrap_or([f64::NAN; 3]);
                if let Some(dj) = d.get_mut(j) {
                    *dj = [(1.0 - a) * prev[0] + a * dj[0], (1.0 - a) * prev[1] + a * dj[1], (1.0 - a) * prev[2] + a * dj[2]];
                }
            }
        }
        let dp = d.get(p).copied().unwrap_or([f64::NAN; 3]);
        pts.push(dp[0] / dp[2]);
        pts.push(dp[1] / dp[2]);
    }
    pts
}

/// HATCH loops (polyline or edges: line, arc, ellipse, spline; angles in radians) → flat rings in the hatch's OCS.
fn hatch_rings(e: &Obj) -> Vec<Vec<f64>> {
    let mut rings = Vec::new();
    for lp in e.arr("loops").map(Vec::as_slice).unwrap_or(&[]) {
        let lp = lp.as_obj();
        if truthy(f(lp, "polyline")) {
            let pl = fo(lp, "polyline");
            let vs = if truthy(f(pl, "vertices")) { refs(arr(pl, "vertices")) } else { Vec::new() };
            rings.push(bulge_polyline(&vs, true));
            continue;
        }
        let mut pts: Vec<f64> = Vec::new();
        let edges = if truthy(f(lp, "edges")) { arr(lp, "edges") } else { &[] };
        for ed in edges {
            let Some(ed) = ed.as_obj() else { continue };
            let ccw_false = matches!(ed.get("ccw"), Some(V::Bool(false)));
            match ed.str("type") {
                Some("line") => {
                    let (sx, sy) = pxy(ed.get("start"));
                    let (ex, ey) = pxy(ed.get("end"));
                    pts.extend([sx, sy, ex, ey]);
                }
                Some("arc") => {
                    let (s, fa) = (num(ed.get("startAngle")), num(ed.get("endAngle")));
                    let (cx, cy) = pxy(ed.get("center"));
                    let r = num(ed.get("radius"));
                    pts.extend(if ccw_false { arc_points(cx, cy, r, -s, -fa, false) } else { arc_points(cx, cy, r, s, fa, true) });
                }
                Some("ellipse") => {
                    let (sa, ea) = (num(ed.get("startAngle")), num(ed.get("endAngle")));
                    let (s, fa) = if ccw_false { (-ea, -sa) } else { (sa, ea) };
                    pts.extend(ellipse_points(ed.get("center"), ed.get("majorAxis"), or_n(ed.get("ratio"), 1.0), s, fa));
                }
                Some("spline") => pts.extend(spline_points(ed)),
                _ => {}
            }
        }
        if pts.len() >= 6 {
            rings.push(pts);
        }
    }
    rings
}

// ---------------------------------------------------------------------------------------------------------------
// Text (UTF-16, as JS strings)

fn is_u(u: u16, c: u8) -> bool {
    u == u16::from(c)
}
fn in_set(u: u16, set: &[u8]) -> bool {
    set.iter().any(|&c| is_u(u, c))
}
fn hex_val(u: u16) -> Option<u16> {
    let c = char::from_u32(u32::from(u))?;
    c.to_digit(16).and_then(|d| u16::try_from(d).ok())
}

/// `plainText`: %%c %%d %%p %%% specials, %%u / %%o dropped, \U+XXXX escapes — three passes, as the JS regexes.
pub fn plain_text(s: &[u16]) -> Vec<u16> {
    let at = |v: &[u16], i: usize| v.get(i).copied().unwrap_or(0);
    // pass 1: /%%[cCdDpP%]/g
    let mut a = Vec::with_capacity(s.len());
    let mut i = 0usize;
    while i < s.len() {
        let c = at(s, i);
        if is_u(c, b'%') && is_u(at(s, i.saturating_add(1)), b'%') && in_set(at(s, i.saturating_add(2)), b"cCdDpP%") {
            let k = at(s, i.saturating_add(2));
            a.push(if in_set(k, b"cC") {
                0x00F8
            } else if in_set(k, b"dD") {
                0x00B0
            } else if in_set(k, b"pP") {
                0x00B1
            } else {
                u16::from(b'%')
            });
            i = i.saturating_add(3);
        } else {
            a.push(c);
            i = i.saturating_add(1);
        }
    }
    // pass 2: /%%[uUoO]/g → ''
    let mut b = Vec::with_capacity(a.len());
    i = 0;
    while i < a.len() {
        if is_u(at(&a, i), b'%') && is_u(at(&a, i.saturating_add(1)), b'%') && in_set(at(&a, i.saturating_add(2)), b"uUoO") {
            i = i.saturating_add(3);
        } else {
            b.push(at(&a, i));
            i = i.saturating_add(1);
        }
    }
    // pass 3: /\\U\+([0-9a-fA-F]{4})/g → String.fromCharCode(hex)
    let mut c = Vec::with_capacity(b.len());
    i = 0;
    while i < b.len() {
        if is_u(at(&b, i), b'\\') && is_u(at(&b, i.saturating_add(1)), b'U') && is_u(at(&b, i.saturating_add(2)), b'+') {
            let hs: Vec<Option<u16>> = (3..7).map(|k| b.get(i.saturating_add(k)).copied().and_then(hex_val)).collect();
            if hs.iter().all(Option::is_some) {
                let v = hs.iter().fold(0u16, |acc, h| (acc << 4) | h.unwrap_or(0));
                c.push(v);
                i = i.saturating_add(7);
                continue;
            }
        }
        c.push(at(&b, i));
        i = i.saturating_add(1);
    }
    c
}

/// One regex pass that replaces `\` + a char of `set` by `with` (or drops it when `with` is None).
fn esc_pass(s: &[u16], set: &[u8], with: Option<u16>) -> Vec<u16> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0usize;
    while i < s.len() {
        let c = s.get(i).copied().unwrap_or(0);
        if is_u(c, b'\\') && s.get(i.saturating_add(1)).is_some_and(|&n| in_set(n, set)) {
            if let Some(w) = with {
                out.push(w);
            }
            i = i.saturating_add(2);
        } else {
            out.push(c);
            i = i.saturating_add(1);
        }
    }
    out
}

/// MTEXT codes → plain lines: \P breaks, {\f…;} \H…; \C…; formatting dropped, stacked fractions as a/b.
pub fn mtext_lines(s: &[u16]) -> Vec<Vec<u16>> {
    let t = plain_text(s);
    let t = esc_pass(&t, b"P", Some(u16::from(b'\n')));
    let t = esc_pass(&t, b"~", Some(u16::from(b' ')));
    // /\\S([^;]*?)[#^/]([^;]*?);/g → '$1/$2'
    let mut a = Vec::with_capacity(t.len());
    let mut i = 0usize;
    while i < t.len() {
        let c = t.get(i).copied().unwrap_or(0);
        if is_u(c, b'\\') && t.get(i.saturating_add(1)).is_some_and(|&n| is_u(n, b'S')) {
            let p = i.saturating_add(2);
            let semi = t.get(p..).and_then(|r| r.iter().position(|&u| is_u(u, b';'))).map(|k| p.saturating_add(k));
            if let Some(semi) = semi {
                let sep = t.get(p..semi).and_then(|r| r.iter().position(|&u| in_set(u, b"#^/"))).map(|k| p.saturating_add(k));
                if let Some(q) = sep {
                    a.extend_from_slice(t.get(p..q).unwrap_or(&[]));
                    a.push(u16::from(b'/'));
                    a.extend_from_slice(t.get(q.saturating_add(1)..semi).unwrap_or(&[]));
                    i = semi.saturating_add(1);
                    continue;
                }
            }
        }
        a.push(c);
        i = i.saturating_add(1);
    }
    // /\\[ACcFfHhQqTtWwp][^;\\]*;/g → ''
    let mut b = Vec::with_capacity(a.len());
    i = 0;
    while i < a.len() {
        let c = a.get(i).copied().unwrap_or(0);
        if is_u(c, b'\\') && a.get(i.saturating_add(1)).is_some_and(|&n| in_set(n, b"ACcFfHhQqTtWwp")) {
            let p = i.saturating_add(2);
            let mut q = p;
            while a.get(q).is_some_and(|&u| !is_u(u, b';') && !is_u(u, b'\\')) {
                q = q.saturating_add(1);
            }
            if a.get(q).is_some_and(|&u| is_u(u, b';')) {
                i = q.saturating_add(1);
                continue;
            }
        }
        b.push(c);
        i = i.saturating_add(1);
    }
    let b = esc_pass(&b, b"LlOoKkNn", None);
    let b: Vec<u16> = b.into_iter().filter(|&u| !is_u(u, b'{') && !is_u(u, b'}')).collect();
    let b = esc_pass(&b, b"\\", Some(u16::from(b'\\')));
    b.split(|&u| is_u(u, b'\n')).map(<[u16]>::to_vec).collect()
}

// ---------------------------------------------------------------------------------------------------------------
// The compiler

#[derive(Clone, Copy)]
struct LayerInfo {
    color: Color,
    on: bool,
    lw: f64,
    plot: bool,
}

struct Tables<'e> {
    blocks: HashMap<&'e str, &'e Obj>,
    by_handle: HashMap<&'e str, (usize, &'e Obj)>,
    image_defs: HashMap<&'e str, &'e Obj>,
}

/// `modelParts(model)`: blocks by name and by block record handle, `*Model_Space`, IMAGEDEFs by handle.
fn model_parts(model: &Obj) -> (Tables<'_>, Option<(usize, &Obj)>) {
    let mut blocks = HashMap::new();
    let mut by_handle = HashMap::new();
    let mut space = None;
    for (i, b) in model.arr("blocks").map(Vec::as_slice).unwrap_or(&[]).iter().enumerate() {
        let Some(b) = b.as_obj() else { continue };
        if let Some(n) = b.str("name") {
            blocks.insert(n, b);
            if space.is_none() && n.eq_ignore_ascii_case("*model_space") {
                space = Some((i, b));
            }
        }
        if b.truthy("record")
            && let Some(r) = b.str("record")
        {
            by_handle.insert(r, (i, b));
        }
    }
    let mut image_defs = HashMap::new();
    for o in model.arr("objects").map(Vec::as_slice).unwrap_or(&[]) {
        if let Some(o) = o.as_obj()
            && o.str("type") == Some("IMAGEDEF")
            && let Some(h) = o.str("handle")
        {
            image_defs.insert(h, o);
        }
    }
    (Tables { blocks, by_handle, image_defs }, space)
}

struct XInfo<'e> {
    tbl: Tables<'e>,
    model: &'e [V],
    layers: HashMap<String, LayerInfo>,
    base: (f64, f64),
}

#[derive(Clone)]
struct Ctx<'e> {
    m: M,
    layer: Option<Rc<str>>,
    color: Option<Color>,
    layer_color: Option<Color>,
    lw: Option<f64>,
    depth: u32,
    tbl: &'e Tables<'e>,
    xp: Rc<str>,
    xlayers: Option<&'e HashMap<String, LayerInfo>>,
    attrs: Option<Rc<HashMap<String, V>>>,
}

/// What a batch is keyed by: layer × colour × line weight.
struct Out {
    layer: Rc<str>,
    color: Color,
    lw: f64,
}

struct SpaceState<'e> {
    batches: Vec<Batch>,
    index: HashMap<(Rc<str>, Color, u64), usize>,
    bounds: Bounds,
    viewports: Vec<&'e Obj>,
    pics: Vec<SpaceImage>,
}

impl SpaceState<'_> {
    /// The batch for layer × colour × line weight (made on first use, in that order, like the JS Map), handed to `f`.
    fn with(&mut self, o: &Out, f: impl FnOnce(&mut Batch)) {
        let key = (o.layer.clone(), o.color, (o.lw + 0.0).to_bits());
        let batches = &mut self.batches;
        let i = *self.index.entry(key).or_insert_with(|| {
            batches.push(Batch {
                layer: o.layer.to_string(),
                color: o.color,
                lw: o.lw,
                lines: Vec::new(),
                fills: Vec::new(),
                texts: Vec::new(),
            });
            batches.len().saturating_sub(1)
        });
        if let Some(b) = self.batches.get_mut(i) {
            f(b);
        }
    }
}

struct Compiler<'e> {
    layers: Vec<Layer>,
    layer_index: HashMap<String, usize>,
    stats: Stats,
    images: Vec<ImageRef>,
    image_index: HashMap<String, usize>,
    xinfo: &'e HashMap<String, XInfo<'e>>,
    /// One shared `Rc<str>` per layer name (batch keys and contexts clone it instead of allocating).
    names: HashMap<String, Rc<str>>,
}

/// MTEXT fields (`value, height, insert, attach, xAxis, lineSpacing, rotation`).
struct MText<'a> {
    value: Option<&'a V>,
    height: Option<&'a V>,
    insert: Option<&'a V>,
    attach: Option<&'a V>,
    x_axis: Option<&'a V>,
    line_spacing: Option<&'a V>,
    rotation: Option<&'a V>,
}
impl<'a> MText<'a> {
    fn of(e: &'a Obj) -> Self {
        MText {
            value: e.get("value"),
            height: e.get("height"),
            insert: e.get("insert"),
            attach: e.get("attach"),
            x_axis: e.get("xAxis"),
            line_spacing: e.get("lineSpacing"),
            rotation: e.get("rotation"),
        }
    }
}

fn type_name(e: &Obj) -> String {
    let t = e.get("type").map(js_string).unwrap_or_else(|| "undefined".into());
    if t == "UNKNOWN" {
        for k in ["dxfName", "className"] {
            if e.truthy(k)
                && let Some(v) = e.get(k)
            {
                return js_string(v);
            }
        }
        return "UNKNOWN".into();
    }
    t
}
fn is_dim(t: &str) -> bool {
    t.starts_with("DIMENSION") || t == "ARC_DIMENSION" || t == "LARGE_RADIAL_DIMENSION"
}
fn small_u8(x: f64) -> u8 {
    // ha 0..2 and va 0..3 in every file read so far; anything else (a damaged file) becomes 0.
    if (0.0..=255.0).contains(&x) && x == x.trunc() { u8::try_from(crate::util::to_u32(x)).unwrap_or(0) } else { 0 }
}
fn base_name(p: &str) -> String {
    p.rsplit(['/', '\\']).next().unwrap_or("").to_string()
}

impl<'e> Compiler<'e> {
    fn name_rc(&mut self, s: &str) -> Rc<str> {
        if let Some(r) = self.names.get(s) {
            return r.clone();
        }
        let r: Rc<str> = Rc::from(s);
        self.names.insert(s.to_string(), r.clone());
        r
    }
    fn skip(&mut self, t: &str) {
        let n = self.stats.unsupported.entry(t.to_string()).or_insert(0);
        *n = n.saturating_add(1);
    }
    fn layer_of(&mut self, name: &str, fallback: Option<&LayerInfo>) -> LayerInfo {
        if let Some(&i) = self.layer_index.get(name)
            && let Some(l) = self.layers.get(i)
        {
            return LayerInfo { color: l.color, on: l.on, lw: l.lw, plot: l.plot };
        }
        let li = fallback.copied().unwrap_or(LayerInfo { color: Color::Ink, on: true, lw: LW_DEFAULT, plot: true });
        self.layers.push(Layer { name: name.to_string(), color: li.color, on: li.on, lw: li.lw, plot: li.plot });
        self.layer_index.insert(name.to_string(), self.layers.len().saturating_sub(1));
        li
    }
    fn prim(&mut self) {
        self.stats.prims = self.stats.prims.saturating_add(1);
    }
    fn line(&mut self, sp: &mut SpaceState<'e>, out: &Out, m: &M, pts: &[f64]) {
        if pts.len() >= 4 {
            let t = tx(m, pts);
            sp.bounds.grow(&t);
            sp.with(out, |b| b.lines.push(t));
            self.prim();
        }
    }

    fn compile_space(&mut self, entities: &'e [V], root: &Ctx<'e>) -> SpaceState<'e> {
        let mut sp =
            SpaceState { batches: Vec::new(), index: HashMap::new(), bounds: Bounds::default(), viewports: Vec::new(), pics: Vec::new() };
        for e in entities {
            self.stats.entities = self.stats.entities.saturating_add(1);
            if let Some(e) = e.as_obj() {
                self.emit(&mut sp, e, root);
            }
        }
        sp
    }

    fn sub_ctx(ctx: &Ctx<'e>, m: M, out: &Out, lay: &LayerInfo, x: Option<(&'e XInfo<'e>, Rc<str>)>) -> Ctx<'e> {
        let (tbl, xp, xlayers) = match x {
            Some((x, prefix)) => (&x.tbl, prefix, Some(&x.layers)),
            None => (ctx.tbl, ctx.xp.clone(), ctx.xlayers),
        };
        Ctx {
            m,
            layer: Some(out.layer.clone()),
            color: Some(out.color),
            layer_color: Some(lay.color),
            lw: Some(out.lw),
            depth: ctx.depth.saturating_add(1),
            tbl,
            xp,
            xlayers,
            attrs: None,
        }
    }

    #[allow(clippy::too_many_lines)]
    fn emit(&mut self, sp: &mut SpaceState<'e>, e: &'e Obj, ctx: &Ctx<'e>) {
        if self.stats.prims > MAX_PRIMS {
            self.stats.truncated = true;
            return;
        }
        if e.truthy("invisible") || ctx.depth > MAX_DEPTH {
            return;
        }
        let src: std::borrow::Cow<'_, str> = match e.get("layer") {
            Some(V::Str(s)) if !s.is_empty() => std::borrow::Cow::Borrowed(s.as_str()),
            Some(v) if v.truthy() => std::borrow::Cow::Owned(js_string(v)),
            _ => std::borrow::Cow::Borrowed("0"),
        };
        let layer: Rc<str> = match &ctx.layer {
            Some(l) if src == "0" => l.clone(),
            _ if !ctx.xp.is_empty() => self.name_rc(&format!("{}{src}", ctx.xp)),
            _ => self.name_rc(&src),
        };
        let fallback = ctx.xlayers.and_then(|x| x.get(src.as_ref())).copied();
        let lay = self.layer_of(&layer, fallback.as_ref());
        let c = e.obj("color").filter(|_| e.truthy("color"));
        let color = match c {
            Some(c) if c.has("rgb") => rgb_hex(num(c.get("rgb"))),
            _ => {
                let idx = c.and_then(|c| c.get("index"));
                if is_num(idx, 0.0) {
                    ctx.color.unwrap_or(Color::Ink)
                } else if idx.is_none() || is_num(idx, 256.0) {
                    match ctx.layer_color {
                        Some(lc) if matches!(e.get("layer"), Some(V::Str(s)) if s == "0") => lc,
                        _ => lay.color,
                    }
                } else {
                    aci_color(num(idx))
                }
            }
        };
        let lwi = nn_n(e.get("lineweight"), LW_BYLAYER);
        let lw = if lwi == LW_BYLAYER {
            lay.lw
        } else if lwi == LW_BYBLOCK {
            ctx.lw.unwrap_or(LW_DEFAULT)
        } else {
            lw_of(lwi)
        };
        let m = ocs(&ctx.m, e.get("extrusion"));
        let out = Out { layer: layer.clone(), color, lw };
        let ty: std::borrow::Cow<'_, str> = match e.get("type") {
            Some(V::Str(s)) => std::borrow::Cow::Borrowed(s.as_str()),
            Some(v) => std::borrow::Cow::Owned(js_string(v)),
            None => std::borrow::Cow::Borrowed("undefined"),
        };

        match ty.as_ref() {
            "LINE" => {
                let (sx, sy) = pxy(e.get("start"));
                let (ex, ey) = pxy(e.get("end"));
                self.line(sp, &out, &m, &[sx, sy, ex, ey]);
            }
            "LWPOLYLINE" => {
                let vs = if e.truthy("vertices") { refs(e.arr("vertices").map(Vec::as_slice).unwrap_or(&[])) } else { Vec::new() };
                let pts = bulge_polyline(&vs, e.truthy("closed"));
                self.line(sp, &out, &m, &pts);
            }
            "POLYLINE_2D" | "POLYLINE_3D" => {
                let all = if e.truthy("vertices") { e.arr("vertices").map(Vec::as_slice).unwrap_or(&[]) } else { &[] };
                let vs: Vec<&V> = all.iter().filter(|v| !bit(f(v.as_obj(), "flags"), 16)).collect();
                let pts = bulge_polyline(&vs, e.truthy("closed"));
                self.line(sp, &out, &m, &pts);
            }
            "CIRCLE" => {
                let (cx, cy) = pxy(e.get("center"));
                let pts = arc_points(cx, cy, num(e.get("radius")), 0.0, TAU, true);
                self.line(sp, &out, &m, &pts);
            }
            "ARC" => {
                let (cx, cy) = pxy(e.get("center"));
                let pts = arc_points(cx, cy, num(e.get("radius")), num(e.get("startAngle")), num(e.get("endAngle")), true);
                self.line(sp, &out, &m, &pts);
            }
            "ELLIPSE" => {
                let pts = ellipse_points(
                    e.get("center"),
                    e.get("majorAxis"),
                    num(e.get("ratio")),
                    num(e.get("startParam")),
                    num(e.get("endParam")),
                );
                self.line(sp, &out, &m, &pts);
            }
            "SPLINE" => {
                let pts = spline_points(e);
                self.line(sp, &out, &m, &pts);
            }
            "LEADER" => {
                let vs = if e.truthy("vertices") { e.arr("vertices").map(Vec::as_slice).unwrap_or(&[]) } else { &[] };
                let pts = flat_xy(vs.iter().map(Some));
                self.line(sp, &out, &m, &pts);
            }
            "MLINE" => {
                let vs = if e.truthy("vertices") { e.arr("vertices").map(Vec::as_slice).unwrap_or(&[]) } else { &[] };
                let mut pts = flat_xy(vs.iter().map(|v| f(v.as_obj(), "point")));
                if e.truthy("closed") && !vs.is_empty() {
                    let (x, y) = pxy(f(vs.first().and_then(V::as_obj), "point"));
                    pts.extend([x, y]);
                }
                self.line(sp, &out, &m, &pts);
            }
            "SOLID" | "TRACE" => {
                let p = if e.truthy("points") { e.arr("points").map(Vec::as_slice).unwrap_or(&[]) } else { &[] };
                let g = |i: usize| p.get(i).filter(|v| v.truthy());
                // DXF order 1,2,3,4 → outline 1,2,4,3
                let ring_src = [g(0), g(1), g(3).or(g(2)), g(2)];
                let ring = tx(&m, &flat_xy(ring_src.into_iter().flatten().map(Some)));
                sp.bounds.grow(&ring);
                sp.with(&out, |b| b.fills.push(Fill { rings: vec![ring], pattern: false }));
                self.prim();
            }
            "3DFACE" => {
                let p = e.arr("points").map(Vec::as_slice).unwrap_or(&[]);
                let g = |i: usize| p.get(i);
                let p3 = g(3).filter(|v| v.truthy()).or(g(2));
                let pts = flat_xy([g(0), g(1), g(2), p3, g(0)].into_iter());
                self.line(sp, &out, &m, &pts);
            }
            "POLYLINE_PFACE" => {
                // seen from above: each face's visible edges; a negative index hides the edge it starts
                let vv = if e.truthy("vertices") { e.arr("vertices").map(Vec::as_slice).unwrap_or(&[]) } else { &[] };
                let faces = if e.truthy("faces") { e.arr("faces").map(Vec::as_slice).unwrap_or(&[]) } else { &[] };
                let mut done: HashSet<(u64, u64)> = HashSet::new();
                let vert = |i: f64| -> Option<&V> {
                    let k = i.abs() - 1.0;
                    if k >= 0.0 && k == k.trunc() { vv.get(crate::util::to_usize(k)).filter(|v| v.truthy()) } else { None }
                };
                for fc in faces {
                    let idx: Vec<f64> =
                        fc.as_arr().map(|a| a.iter().filter(|v| v.truthy()).map(|v| num(Some(v))).collect()).unwrap_or_default();
                    for k in 0..idx.len() {
                        let i = idx.get(k).copied().unwrap_or(f64::NAN);
                        let j = idx.get(k.saturating_add(1).checked_rem(idx.len()).unwrap_or(0)).copied().unwrap_or(f64::NAN);
                        if i < 0.0 {
                            continue;
                        }
                        let (Some(a), Some(b)) = (vert(i), vert(j)) else { continue };
                        let (lo, hi) = (js_min(i.abs(), j.abs()), js_max(i.abs(), j.abs()));
                        if !done.insert(((lo + 0.0).to_bits(), (hi + 0.0).to_bits())) {
                            continue; // an edge two faces share is drawn once
                        }
                        let (ax, ay) = pxy(Some(a));
                        let (bx, by) = pxy(Some(b));
                        self.line(sp, &out, &m, &[ax, ay, bx, by]);
                    }
                }
            }
            "POLYLINE_MESH" => {
                // seen from above: the M × N grid of lines (open or closed each way)
                let vv = if e.truthy("vertices") { e.arr("vertices").map(Vec::as_slice).unwrap_or(&[]) } else { &[] };
                let (mm, nn) = (or_n(e.get("m"), 0.0), or_n(e.get("n"), 0.0));
                if mm * nn > usize_f(vv.len()) || mm < 1.0 || nn < 1.0 || mm.is_nan() || nn.is_nan() || mm != mm.trunc() || nn != nn.trunc()
                {
                    self.skip("POLYLINE_MESH (damaged)");
                    return;
                }
                let (mu, nu) = (crate::util::to_usize(mm), crate::util::to_usize(nn));
                let at = |i: usize, j: usize| pxy(vv.get(i.saturating_mul(nu).saturating_add(j)));
                let cm = bit(e.get("flags"), 1);
                let cn = bit(e.get("flags"), 32);
                for i in 0..mu {
                    let mut r = Vec::with_capacity(nu.saturating_mul(2).saturating_add(2));
                    for j in 0..nu {
                        let (x, y) = at(i, j);
                        r.extend([x, y]);
                    }
                    if cn && nu > 2 {
                        let (x, y) = at(i, 0);
                        r.extend([x, y]);
                    }
                    self.line(sp, &out, &m, &r);
                }
                for j in 0..nu {
                    let mut r = Vec::with_capacity(mu.saturating_mul(2).saturating_add(2));
                    for i in 0..mu {
                        let (x, y) = at(i, j);
                        r.extend([x, y]);
                    }
                    if cm && mu > 2 {
                        let (x, y) = at(0, j);
                        r.extend([x, y]);
                    }
                    self.line(sp, &out, &m, &r);
                }
            }
            "HATCH" => {
                let rings: Vec<Vec<f64>> = hatch_rings(e).iter().map(|r| tx(&m, r)).filter(|r| r.len() >= 6).collect();
                if !rings.is_empty() {
                    for r in &rings {
                        sp.bounds.grow(r);
                    }
                    sp.with(&out, |b| b.fills.push(Fill { rings, pattern: !e.truthy("solid") }));
                    self.prim();
                }
            }
            "TEXT" => text(sp, &out, &m, e, None),
            "ATTRIB" => {
                if bit(e.get("flags"), 1) {
                    return; // invisible attribute
                }
                if num(e.get("attType")) > 1.0
                    && e.truthy("mtext")
                    && let Some(mt) = e.obj("mtext")
                {
                    mtext(sp, &out, &m, &MText::of(mt));
                } else {
                    text(sp, &out, &m, e, None);
                }
            }
            "MTEXT" => mtext(sp, &out, &m, &MText::of(e)),
            "INSERT" | "MINSERT" => self.insert(sp, e, ctx, &m, &out, &lay),
            "ACAD_TABLE" => {
                // Drawn from its anonymous block (*T…), placed like a block reference at the table's insertion point.
                let blk = if e.truthy("block") { e.str("block").and_then(|b| ctx.tbl.blocks.get(b)).copied() } else { None };
                let Some(blk) = blk else {
                    self.skip(&ty);
                    return;
                };
                let ip = e.obj("insert").filter(|_| e.truthy("insert"));
                let sc = e.obj("scale");
                let local = mul(
                    &mul(&translate(or_n(f(ip, "x"), 0.0), or_n(f(ip, "y"), 0.0)), &rotate(or_n(e.get("rotation"), 0.0))),
                    &scale(or_n(f(sc, "x"), 1.0), or_n(f(sc, "y"), 1.0)),
                );
                let sub = Self::sub_ctx(ctx, mul(&m, &local), &out, &lay, None);
                for be in blk.arr("entities").map(Vec::as_slice).unwrap_or(&[]) {
                    if let Some(be) = be.as_obj() {
                        self.emit(sp, be, &sub);
                    }
                }
            }
            "MULTILEADER" => self.mleader(sp, e, &m, ctx, &out, &lay),
            "IMAGE" => {
                let def = e.str("def").and_then(|d| ctx.tbl.image_defs.get(d)).copied();
                let size = e.obj("size");
                let (w, h, u, v, p) = (f(size, "x"), f(size, "y"), e.get("u"), e.get("v"), e.get("insert"));
                let Some(def) = def.filter(|_| truthy(w) && truthy(h) && truthy(u) && truthy(v) && truthy(p)) else {
                    self.skip("IMAGE (no picture file named)");
                    return;
                };
                let (w, h) = (num(w), num(h));
                let ((ux, uy), (vx, vy), (px, py)) = (pxy(u), pxy(v), pxy(p));
                // pixel (x right, y down) → world: the position is the image's lower-left corner
                let pm = mul(&m, &[ux, uy, -vx, -vy, px + h * vx, py + h * vy]);
                let file = if def.truthy("fileName") { def.get("fileName").map(js_string).unwrap_or_default() } else { String::new() };
                sp.pics.push(SpaceImage { m: pm, w, h, file: file.clone(), layer: layer.to_string(), clip: None, alpha: None });
                sp.bounds.grow(&tx(&pm, &[0.0, 0.0, w, 0.0, w, h, 0.0, h]));
                match self.image_index.get(&file) {
                    Some(&i) => {
                        if let Some(img) = self.images.get_mut(i) {
                            img.uses = img.uses.saturating_add(1);
                        }
                    }
                    None => {
                        self.images.push(ImageRef { name: base_name(&file), file: file.clone(), uses: 1 });
                        self.image_index.insert(file, self.images.len().saturating_sub(1));
                    }
                }
                self.prim();
            }
            "VIEWPORT" => {
                if ctx.depth == 0 {
                    sp.viewports.push(e);
                }
            }
            "ATTDEF" => {
                // only a multileader's block shows its attribute definitions, with the leader's values
                let v = ctx.attrs.as_ref().and_then(|a| e.str("handle").and_then(|h| a.get(h)));
                if let Some(v) = v.filter(|v| !matches!(v, V::Null)) {
                    text(sp, &out, &m, e, Some(v));
                }
            }
            "POINT" | "XLINE" | "RAY" => {} // not drawn on a sheet
            _ => {
                if is_dim(&ty) {
                    // drawn from its anonymous block (*D…), already in this space's coordinates
                    let blk = if e.truthy("block") { e.str("block").and_then(|b| ctx.tbl.blocks.get(b)).copied() } else { None };
                    let Some(blk) = blk else {
                        self.skip("DIMENSION");
                        return;
                    };
                    let ip = e.obj("insert").filter(|_| e.truthy("insert"));
                    let sub = Self::sub_ctx(ctx, mul(&m, &translate(or_n(f(ip, "x"), 0.0), or_n(f(ip, "y"), 0.0))), &out, &lay, None);
                    for be in blk.arr("entities").map(Vec::as_slice).unwrap_or(&[]) {
                        if let Some(be) = be.as_obj() {
                            self.emit(sp, be, &sub);
                        }
                    }
                } else {
                    let t = type_name(e);
                    self.skip(&t);
                }
            }
        }
    }

    fn insert(&mut self, sp: &mut SpaceState<'e>, e: &'e Obj, ctx: &Ctx<'e>, m: &M, out: &Out, lay: &LayerInfo) {
        let name = e.str("block").unwrap_or("");
        let Some(blk) = e.str("block").and_then(|b| ctx.tbl.blocks.get(b)).copied() else {
            self.skip("INSERT (missing block)");
            return;
        };
        if ctx.depth > 20 {
            self.skip("INSERT (nested too deep)");
            return;
        }
        let is_x = bit(blk.get("flags"), 4);
        let x: Option<&'e XInfo<'e>> = if is_x { self.xinfo.get(&name.to_lowercase()) } else { None };
        if is_x && x.is_none() {
            return; // linked drawing not loaded: listed in drawing.xrefs
        }
        let base = match x {
            Some(x) => x.base,
            None => {
                if blk.truthy("base") {
                    pxy(blk.get("base"))
                } else {
                    (0.0, 0.0)
                }
            }
        };
        let content: &'e [V] = match x {
            Some(x) => x.model,
            None => {
                if blk.truthy("entities") {
                    blk.arr("entities").map(Vec::as_slice).unwrap_or(&[])
                } else {
                    &[]
                }
            }
        };
        let sc = e.obj("scale");
        let (sx, sy) = (or_n(f(sc, "x"), 1.0), or_n(f(sc, "y"), 1.0));
        let cols = js_max(1.0, or_n(e.get("cols"), 1.0));
        let rows = js_max(1.0, or_n(e.get("rows"), 1.0));
        let (ix, iy) = pxy(e.get("insert"));
        let rot = or_n(e.get("rotation"), 0.0);
        let (cs, rs) = (or_n(e.get("colSpacing"), 0.0), or_n(e.get("rowSpacing"), 0.0));
        let prefix: Option<Rc<str>> = x.map(|_| Rc::from(format!("{name}|")));
        let drawn = content.iter().filter(|be| be.as_obj().and_then(|o| o.str("type")) != Some("ATTDEF")).count();
        let mut r = 0.0;
        'rows: while r < rows {
            let mut c = 0.0;
            while c < cols {
                if self.stats.prims > MAX_PRIMS {
                    // Every emit from here on returns at once (truncated); only the xref entity count still grows.
                    let left_here = (cols - c).ceil().max(0.0);
                    let left_rows = (rows - r - 1.0).ceil().max(0.0) * cols.ceil();
                    if x.is_some() {
                        let k = (left_here + left_rows) * usize_f(drawn);
                        self.stats.xref_entities = self.stats.xref_entities.saturating_add(crate::util::to_u32(k.min(4e9)).into());
                    }
                    if drawn > 0 {
                        self.stats.truncated = true;
                    }
                    break 'rows;
                }
                let local = mul(
                    &mul(&translate(ix, iy), &rotate(rot)),
                    &mul(&scale(sx, sy), &translate(-base.0 + c * cs / sx, -base.1 + r * rs / sy)),
                );
                let sub = Self::sub_ctx(ctx, mul(m, &local), out, lay, x.zip(prefix.clone()));
                for be in content {
                    let Some(bo) = be.as_obj() else {
                        if x.is_some() {
                            self.stats.xref_entities = self.stats.xref_entities.saturating_add(1);
                        }
                        continue;
                    };
                    if bo.str("type") != Some("ATTDEF") {
                        if x.is_some() {
                            self.stats.xref_entities = self.stats.xref_entities.saturating_add(1);
                        }
                        self.emit(sp, bo, &sub);
                    }
                }
                c += 1.0;
            }
            r += 1.0;
        }
        for a in if e.truthy("attribs") { e.arr("attribs").map(Vec::as_slice).unwrap_or(&[]) } else { &[] } {
            if let Some(a) = a.as_obj() {
                self.emit(sp, a, ctx); // attributes are placed in the parent's space
            }
        }
    }

    /// Multileader: leader lines (+ closed arrowhead and landing), then its text or block.
    fn mleader(&mut self, sp: &mut SpaceState<'e>, e: &'e Obj, m: &M, ctx: &Ctx<'e>, out: &Out, lay: &LayerInfo) {
        let cx = e.obj("context").filter(|_| e.truthy("context"));
        let arrow = match f(cx, "arrowSize") {
            None | Some(V::Null) => nn_n(e.get("arrowSize"), 0.0),
            v => num(v),
        };
        let roots = if truthy(f(cx, "roots")) { arr(cx, "roots") } else { &[] };
        for sec in roots {
            let sec = sec.as_obj();
            let last = f(sec, "connection").filter(|v| v.truthy());
            let lines = if truthy(f(sec, "lines")) { arr(sec, "lines") } else { &[] };
            for ll in lines {
                let ll = ll.as_obj();
                let pts_src = if truthy(f(ll, "points")) { arr(ll, "points") } else { &[] };
                let mut pts: Vec<&V> = pts_src.iter().filter(|v| v.truthy()).collect();
                if pts.is_empty() && last.is_none() {
                    continue;
                }
                if let Some(l) = last {
                    pts.push(l);
                }
                if !is_num(e.get("leaderType"), 0.0) {
                    let flat = flat_xy(pts.iter().map(|p| Some(*p)));
                    self.line(sp, out, m, &flat);
                }
                if arrow > 0.0 && pts.len() > 1 {
                    // closed filled arrowhead at the first point
                    let (ax, ay) = pxy(pts.first().copied());
                    let (bx0, by0) = pxy(pts.get(1).copied());
                    let d = js_hypot(bx0 - ax, by0 - ay);
                    if d > 0.0 {
                        let (ux, uy) = ((bx0 - ax) / d, (by0 - ay) / d);
                        let (bx, by) = (ax + ux * arrow, ay + uy * arrow);
                        let hw = arrow / 6.0;
                        let ring = tx(m, &[ax, ay, bx - uy * hw, by + ux * hw, bx + uy * hw, by - ux * hw]);
                        sp.bounds.grow(&ring);
                        sp.with(out, |b| b.fills.push(Fill { rings: vec![ring], pattern: false }));
                    }
                }
            }
            let dl = match f(sec, "landingDistance") {
                None | Some(V::Null) => f(sec, "doglegLength"),
                v => v,
            };
            if let Some(l) = last
                && e.truthy("doglegEnabled")
                && truthy(f(sec, "direction"))
                && truthy(dl)
            {
                let (lx, ly) = pxy(Some(l));
                let (dx, dy) = pxy(f(sec, "direction"));
                let dl = num(dl);
                self.line(sp, out, m, &[lx, ly, lx + dx * dl, ly + dy * dl]);
            }
        }
        let t = if truthy(f(cx, "hasText")) { f(cx, "text").filter(|v| v.truthy()).and_then(V::as_obj) } else { None };
        if let Some(t) = t
            && t.truthy("label")
            && t.truthy("location")
        {
            let mt = MText {
                value: t.get("label"),
                height: f(cx, "textHeight"),
                insert: t.get("location"),
                attach: t.get("alignment"),
                x_axis: t.get("direction"),
                line_spacing: t.get("lineSpacing"),
                rotation: None,
            };
            mtext(sp, out, m, &mt);
        }
        let bc = if truthy(f(cx, "hasBlock")) { f(cx, "block").filter(|v| v.truthy()).and_then(V::as_obj) } else { None };
        let key = bc.and_then(|bc| match bc.get("blockHandle") {
            None | Some(V::Null) => bc.get("handle"),
            v => v,
        });
        let blk = key.and_then(V::as_str).and_then(|k| ctx.tbl.by_handle.get(k)).map(|&(_, b)| b);
        if let (Some(bc), Some(blk)) = (bc, blk) {
            let t = bc.arr("matrix").filter(|_| bc.truthy("matrix"));
            let bm = match t {
                Some(t) if t.len() == 16 => {
                    let g = |i: usize| num(t.get(i));
                    [g(0), g(4), g(1), g(5), g(3), g(7)] // 4 × 4, row-major
                }
                _ => {
                    let (lx, ly) = pxy(bc.get("location"));
                    let s = bc.obj("scale");
                    mul(
                        &translate(lx, ly),
                        &mul(&rotate(or_n(bc.get("rotation"), 0.0)), &scale(or_n(f(s, "x"), 1.0), or_n(f(s, "y"), 1.0))),
                    )
                }
            };
            let base = if blk.truthy("base") { pxy(blk.get("base")) } else { (0.0, 0.0) };
            let mut sub = Self::sub_ctx(ctx, mul(m, &mul(&bm, &translate(-base.0, -base.1))), out, lay, None);
            let mut attrs = HashMap::new();
            for a in e.arr("blockLabels").filter(|_| e.truthy("blockLabels")).map(Vec::as_slice).unwrap_or(&[]) {
                let a = a.as_obj();
                if let Some(h) = f(a, "attdefHandle").and_then(V::as_str) {
                    attrs.insert(h.to_string(), f(a, "text").cloned().unwrap_or(V::Null));
                }
            }
            sub.attrs = Some(Rc::new(attrs));
            for be in blk.arr("entities").filter(|_| blk.truthy("entities")).map(Vec::as_slice).unwrap_or(&[]) {
                if let Some(be) = be.as_obj() {
                    self.emit(sp, be, &sub);
                }
            }
        }
        self.prim();
    }
}

/// TEXT / ATTRIB / ATTDEF (model fields: value, height, insert, align, hAlign, vAlign, rotation, generation).
fn text(sp: &mut SpaceState<'_>, out: &Out, m: &M, t: &Obj, value: Option<&V>) {
    let value = value.or_else(|| t.get("value"));
    let raw = if truthy(value) { value.map(js_string).unwrap_or_default() } else { String::new() };
    let s = plain_text(&u16s(&raw));
    if blank16(&s) || !t.truthy("height") {
        return;
    }
    let aligned = or_n(t.get("hAlign"), 0.0) != 0.0 || or_n(t.get("vAlign"), 0.0) != 0.0;
    let align = t.obj("align").filter(|_| t.truthy("align"));
    let p = if aligned && align.is_some() && (truthy(f(align, "x")) || truthy(f(align, "y"))) { t.get("align") } else { t.get("insert") };
    let hi = or_n(t.get("hAlign"), 0.0);
    // [0,1,2,0,1,0][hAlign]: left, center, right; aligned/fit → left; middle → center (out of range → 0)
    let ha = if (0.0..=5.0).contains(&hi) && hi == hi.trunc() {
        [0u8, 1, 2, 0, 1, 0].get(crate::util::to_usize(hi)).copied().unwrap_or(0)
    } else {
        0
    };
    let va = if is_num(t.get("hAlign"), 4.0) { 2 } else { small_u8(or_n(t.get("vAlign"), 0.0)) };
    let (px, py) = pxy(p);
    let mirror = to_i32(num(t.get("generation"))) & 2 == 2;
    place(sp, out, m, px, py, num(t.get("height")), or_n(t.get("rotation"), 0.0), s, ha, va, mirror);
}

/// MTEXT (model fields: value, height, insert, attach, xAxis, lineSpacing).
fn mtext(sp: &mut SpaceState<'_>, out: &Out, m: &M, e: &MText<'_>) {
    let raw = if truthy(e.value) { e.value.map(js_string).unwrap_or_default() } else { String::new() };
    let all = mtext_lines(&u16s(&raw));
    let n_all = all.len();
    let lines: Vec<Vec<u16>> =
        all.into_iter().enumerate().filter(|(i, l)| !blank16(l) || i.saturating_add(1) < n_all).map(|(_, l)| l).collect();
    if lines.is_empty() || !truthy(e.height) || !truthy(e.insert) {
        return;
    }
    let xa = e.x_axis.filter(|v| v.truthy()).and_then(V::as_obj);
    let rot = if xa.is_some() && (truthy(f(xa, "x")) || truthy(f(xa, "y"))) {
        num(f(xa, "y")).atan2(num(f(xa, "x")))
    } else {
        or_n(e.rotation, 0.0)
    };
    let ap = or_n(e.attach, 1.0) - 1.0;
    let ha = ap % 3.0;
    let row = (ap / 3.0).floor();
    let h = num(e.height);
    let gap = h * 1.667 * or_n(e.line_spacing, 1.0);
    let total = gap * (usize_f(lines.len()) - 1.0);
    // baseline of the first line, relative to the insertion point
    let top = if row == 0.0 {
        -h
    } else if row == 1.0 {
        total / 2.0 - h / 2.0
    } else {
        total
    };
    let (cr, sr) = (rot.cos(), rot.sin());
    let (ix, iy) = pxy(e.insert);
    let ha = small_u8(ha + 0.0);
    for (i, ln) in lines.into_iter().enumerate() {
        if blank16(&ln) {
            continue;
        }
        let dy = top - gap * usize_f(i);
        place(sp, out, m, ix - dy * sr, iy + dy * cr, h, rot, ln, ha, 0, false);
    }
}

/// Put one line of text into the batch, transformed by m (position, height and rotation).
#[allow(clippy::too_many_arguments)]
fn place(sp: &mut SpaceState<'_>, out: &Out, m: &M, x: f64, y: f64, h: f64, rot: f64, s: Vec<u16>, ha: u8, va: u8, mirror: bool) {
    let xx = m[0] * x + m[2] * y + m[4];
    let yy = m[1] * x + m[3] * y + m[5];
    let sx = js_hypot(m[0], m[1]);
    let sy = js_hypot(m[2], m[3]);
    let det = m[0] * m[3] - m[1] * m[2];
    let angle = m[1].atan2(m[0]) + if det < 0.0 { -rot } else { rot };
    let hh = h * sy;
    let len = usize_f(s.len());
    sp.with(out, |b| {
        b.texts.push(Text { x: xx, y: yy, h: hh, rot: angle, s: String::from_utf16_lossy(&s), ha, va, mirror: mirror != (det < 0.0) })
    });
    // Rough extent so fit-to-drawing includes text: assume 0.6 h per character.
    let syd = if sy != 0.0 && !sy.is_nan() { sy } else { 1.0 };
    let w = len * hh * 0.6 * (sx / syd);
    sp.bounds.grow(&[xx, yy, xx + w * angle.cos(), yy + w * angle.sin()]);
}

// ---------------------------------------------------------------------------------------------------------------
// The layer order: `a.name.localeCompare(b.name, undefined, { numeric: true })`

/// A collation element's primary weight, ordered like ICU's root collation: spaces and punctuation, then symbols,
/// then numbers (by value, `numeric: true`), then letters, then everything else by code point.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Prim {
    Sym(u8),
    /// (significant digit count, digits without leading zeros)
    Num(usize, String),
    Letter(u8),
    Other(u32),
}
const SYMBOLS: &str = " _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";

fn latin1_base(c: char) -> Option<(&'static str, bool)> {
    // (base letters, uppercase) for the accented Latin-1 letters
    let (b, up) = match c {
        'À'..='Å' => ("a", true),
        'à'..='å' => ("a", false),
        'Æ' => ("ae", true),
        'æ' => ("ae", false),
        'Ç' => ("c", true),
        'ç' => ("c", false),
        'È'..='Ë' => ("e", true),
        'è'..='ë' => ("e", false),
        'Ì'..='Ï' => ("i", true),
        'ì'..='ï' => ("i", false),
        'Ñ' => ("n", true),
        'ñ' => ("n", false),
        'Ò'..='Ö' | 'Ø' => ("o", true),
        'ò'..='ö' | 'ø' => ("o", false),
        'Ù'..='Ü' => ("u", true),
        'ù'..='ü' => ("u", false),
        'Ý' => ("y", true),
        'ý' | 'ÿ' => ("y", false),
        'ß' => ("ss", false),
        _ => return None,
    };
    Some((b, up))
}

/// (primary, secondary, tertiary) elements of a string.
fn collation_elements(s: &str) -> Vec<(Prim, u32, u8)> {
    let mut out = Vec::new();
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0usize;
    while i < cs.len() {
        let Some(&c) = cs.get(i) else { break };
        if c.is_ascii_digit() {
            let mut j = i;
            while cs.get(j).is_some_and(char::is_ascii_digit) {
                j = j.saturating_add(1);
            }
            let digits: String = cs.get(i..j).unwrap_or(&[]).iter().collect();
            let sig = digits.trim_start_matches('0').to_string();
            out.push((Prim::Num(sig.len(), sig), 0, 0));
            i = j;
            continue;
        }
        i = i.saturating_add(1);
        let cp = u32::from(c);
        if cp < 0x20 || (0x7F..=0x9F).contains(&cp) {
            continue; // control characters are ignorable
        }
        if c.is_ascii_alphabetic() {
            let l = c.to_ascii_lowercase();
            out.push((Prim::Letter(u8::try_from(u32::from(l)).unwrap_or(0)), 0, u8::from(c.is_ascii_uppercase())));
            continue;
        }
        if let Some(k) = SYMBOLS.find(c) {
            out.push((Prim::Sym(u8::try_from(k).unwrap_or(u8::MAX)), 0, 0));
            continue;
        }
        if let Some((base, up)) = latin1_base(c) {
            let sharp = c == 'ß';
            for (n, b) in base.chars().enumerate() {
                let sec = if base.len() == 1 { cp } else { 0 };
                let ter = u8::from(up) | if sharp && n == 0 { 2 } else { 0 };
                out.push((Prim::Letter(u8::try_from(u32::from(b)).unwrap_or(0)), sec, ter));
            }
            continue;
        }
        out.push((Prim::Other(cp), 0, 0));
    }
    out
}

/// A natural, ICU-root-like comparison (primary: letters case-insensitive, digit runs by value; then accents;
/// then case, lower before upper) — what `localeCompare(b, undefined, {numeric: true})` gives for layer names.
pub fn collate(a: &str, b: &str) -> std::cmp::Ordering {
    let (ea, eb) = (collation_elements(a), collation_elements(b));
    let p = ea.iter().map(|e| &e.0).cmp(eb.iter().map(|e| &e.0));
    if p.is_ne() {
        return p;
    }
    let s = ea.iter().map(|e| e.1).filter(|&x| x != 0).cmp(eb.iter().map(|e| e.1).filter(|&x| x != 0));
    if s.is_ne() {
        return s;
    }
    ea.iter().map(|e| e.2).cmp(eb.iter().map(|e| e.2))
}

// ---------------------------------------------------------------------------------------------------------------

fn layer_info(l: &Obj) -> LayerInfo {
    LayerInfo {
        color: layer_color(l),
        on: !l.truthy("off") && !l.truthy("frozen"),
        lw: lw_of(num(l.get("lineweight"))),
        plot: layer_plots(l),
    }
}

/// `compileDrawing(model, { versionTag, xrefs })`. `xrefs`: xref block name in lower case → that drawing's model.
pub fn compile_drawing(model: &Obj, version_tag: &str, xrefs: &[(String, Obj)]) -> Drawing {
    let (main, model_space) = model_parts(model);
    // Linked drawings: their blocks, model-space entities, layer colours and base point.
    let mut xinfo: HashMap<String, XInfo<'_>> = HashMap::new();
    for (key, xdb) in xrefs {
        let (tbl, space) = model_parts(xdb);
        let mut layers = HashMap::new();
        for l in xdb.obj("tables").and_then(|t| t.arr("layers")).map(Vec::as_slice).unwrap_or(&[]) {
            if let Some(l) = l.as_obj() {
                layers.insert(l.get("name").map(js_string).unwrap_or_else(|| "undefined".into()), layer_info(l));
            }
        }
        let ins = xdb.obj("header").and_then(|h| h.get("INSBASE")).filter(|v| v.truthy());
        let base = if ins.is_some() { pxy(ins) } else { (0.0, 0.0) };
        let content = space.map(|(_, s)| s.arr("entities").map(Vec::as_slice).unwrap_or(&[])).unwrap_or(&[]);
        xinfo.insert(key.clone(), XInfo { tbl, model: content, layers, base });
    }

    let mut c = Compiler {
        layers: Vec::new(),
        layer_index: HashMap::new(),
        stats: Stats::default(),
        images: Vec::new(),
        image_index: HashMap::new(),
        xinfo: &xinfo,
        names: HashMap::new(),
    };
    for l in model.obj("tables").and_then(|t| t.arr("layers")).map(Vec::as_slice).unwrap_or(&[]) {
        let Some(l) = l.as_obj() else { continue };
        let Some(name) = l.get("name").map(js_string) else { continue };
        let li = layer_info(l);
        let layer = Layer { name: name.clone(), color: li.color, on: li.on, lw: li.lw, plot: li.plot };
        match c.layer_index.get(&name) {
            Some(&i) => {
                if let Some(slot) = c.layers.get_mut(i) {
                    *slot = layer; // Map.set on an existing key keeps its place
                }
            }
            None => {
                c.layers.push(layer);
                c.layer_index.insert(name, c.layers.len().saturating_sub(1));
            }
        }
    }
    let xref_names: Vec<String> = model
        .arr("blocks")
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .filter_map(V::as_obj)
        .filter(|b| bit(b.get("flags"), 4))
        .filter_map(|b| b.str("name").map(str::to_string))
        .collect();

    let root = Ctx {
        m: IDENTITY,
        layer: None,
        color: None,
        layer_color: None,
        lw: None,
        depth: 0,
        tbl: &main,
        xp: Rc::from(""),
        xlayers: None,
        attrs: None,
    };

    // ---- spaces: model first, then layouts in tab order ----
    let mut spaces = Vec::new();
    if let Some((_, ms)) = model_space {
        let ents = ms.arr("entities").filter(|_| ms.truthy("entities")).map(Vec::as_slice).unwrap_or(&[]);
        let s = c.compile_space(ents, &root);
        spaces.push(Space {
            key: "model".into(),
            name: "Model".into(),
            kind: SpaceKind::Model,
            batches: s.batches,
            bounds: s.bounds,
            paper: None,
            paper_unit: None,
            viewports: Vec::new(),
            images: s.pics,
        });
    }
    let mut layouts: Vec<&Obj> = model
        .arr("layouts")
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .filter_map(V::as_obj)
        .filter(|l| {
            let n = if l.truthy("name") { l.get("name").map(js_string).unwrap_or_default() } else { String::new() };
            !n.eq_ignore_ascii_case("model")
        })
        .collect();
    layouts.sort_by(|a, b| nn_n(a.get("tabOrder"), 0.0).partial_cmp(&nn_n(b.get("tabOrder"), 0.0)).unwrap_or(std::cmp::Ordering::Equal));
    for l in layouts {
        let Some(&(ri, rec)) = l.str("block").and_then(|b| main.by_handle.get(b)) else { continue };
        if model_space.is_some_and(|(mi, _)| mi == ri) {
            continue;
        }
        let ents = rec.arr("entities").filter(|_| rec.truthy("entities")).map(Vec::as_slice).unwrap_or(&[]);
        let mut s = c.compile_space(ents, &root);
        let lim = l.obj("limits").filter(|_| l.truthy("limits"));
        let (lmin, lmax) = (fo(lim, "min").filter(|_| truthy(f(lim, "min"))), fo(lim, "max").filter(|_| truthy(f(lim, "max"))));
        let paper = match (lmin, lmax) {
            (Some(a), Some(b)) if num(b.get("x")) > num(a.get("x")) => {
                Some(Bounds { min_x: num(a.get("x")), min_y: num(a.get("y")), max_x: num(b.get("x")), max_y: num(b.get("y")) })
            }
            _ => None,
        };
        // The first viewport of every layout is the sheet itself (DXF: "paper space viewport"), not a window onto
        // the model — skip it; the rest show the model.
        let vps: Vec<Viewport> = s
            .viewports
            .iter()
            .skip(1)
            .filter(|v| num(v.get("width")) > 0.0 && num(v.get("height")) > 0.0 && num(v.get("viewHeight")) > 0.0)
            .map(|v| {
                let (cx, cy) = pxy(v.get("center"));
                let (vt, vc) = (v.obj("viewTarget"), v.obj("viewCenter"));
                let mut frozen: Vec<String> = Vec::new();
                for fz in v.arr("frozenLayers").filter(|_| v.truthy("frozenLayers")).map(Vec::as_slice).unwrap_or(&[]) {
                    let fz = js_string(fz);
                    if !frozen.contains(&fz) {
                        frozen.push(fz);
                    }
                }
                Viewport {
                    cx,
                    cy,
                    w: num(v.get("width")),
                    h: num(v.get("height")),
                    // Model point at the viewport centre: view target + view centre (2D plan views).
                    vx: or_n(f(vt, "x"), 0.0) + or_n(f(vc, "x"), 0.0),
                    vy: or_n(f(vt, "y"), 0.0) + or_n(f(vc, "y"), 0.0),
                    view_height: num(v.get("viewHeight")),
                    twist: or_n(v.get("twist"), 0.0),
                    frozen,
                }
            })
            .collect();
        let empty = s.batches.is_empty() && vps.is_empty();
        if empty && paper.is_none() {
            continue;
        }
        if let Some(p) = &paper {
            s.bounds.grow(&[p.min_x, p.min_y, p.max_x, p.max_y]);
        }
        // Paper units: AutoCAD sheets are in inches or millimetres; a sheet wider than 100 units is millimetres.
        let unit = if paper.as_ref().is_some_and(|p| p.max_x - p.min_x > 100.0) { "mm" } else { "in" };
        spaces.push(Space {
            key: format!("layout:{}", l.get("handle").map(js_string).unwrap_or_else(|| "undefined".into())),
            name: l.get("name").map(js_string).unwrap_or_default(),
            kind: SpaceKind::Paper,
            batches: s.batches,
            bounds: s.bounds,
            paper,
            paper_unit: Some(unit.into()),
            viewports: vps,
            images: s.pics,
        });
    }

    let mut layers = std::mem::take(&mut c.layers);
    layers.sort_by(|a, b| collate(&a.name, &b.name));
    Drawing {
        version: version_tag.to_string(),
        release: release_of(version_tag).to_string(),
        units: units_of(model.obj("header").and_then(|h| h.get("INSUNITS"))).to_string(),
        xrefs: xref_names.into_iter().map(|name| XrefInfo { loaded: xinfo.contains_key(&name.to_lowercase()), name }).collect(),
        images: std::mem::take(&mut c.images),
        layers,
        spaces,
        stats: std::mem::take(&mut c.stats),
    }
}

/// The drawing as JSON (the JS `compileDrawing` shape; non-finite numbers print as `null`, as `JSON.stringify`).
pub fn to_json(d: &Drawing) -> String {
    serde_json::to_string(d)
        .unwrap_or_else(|e| format!("{{\"$error\":{{\"code\":\"json\",\"message\":{}}}}}", V::Str(e.to_string()).to_json()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering::*;

    #[test]
    fn natural_order() {
        assert_eq!(collate("01", "1"), Equal);
        assert_eq!(collate("A", "a"), Greater);
        assert_eq!(collate("a", "B"), Less);
        assert_eq!(collate("x10", "x9"), Greater);
        assert_eq!(collate("a-b", "ab"), Less);
        assert_eq!(collate("_a", "a"), Less);
        assert_eq!(collate("Ab", "aB"), Greater);
        assert_eq!(collate("1.5", "1.10"), Less);
        assert_eq!(collate("-", "0"), Less);
        assert_eq!(collate("$", "1"), Less);
        assert_eq!(collate("1", "a"), Less);
        assert_eq!(collate("é", "e"), Greater);
        assert_eq!(collate("é", "f"), Less);
        assert_eq!(collate("E", "é"), Less);
        assert_eq!(collate("a1", "a-1"), Greater);
        assert_eq!(collate("ä", "ae"), Less);
        assert_eq!(collate("ß", "ss"), Greater);
        assert_eq!(collate("a\u{1}", "a"), Equal);
    }

    #[test]
    fn text_codes() {
        let s = |x: &str| String::from_utf16_lossy(&plain_text(&u16s(x)));
        assert_eq!(s("%%c10 %%d %%p %%%%"), "ø10 ° ± %%");
        assert_eq!(s("a%%ub\\U+00e9"), "abé");
        let l = mtext_lines(&u16s("{\\fArial|b0;A}\\Pb\\S1/2;\\H2.5x;c\\\\d"));
        let l: Vec<String> = l.iter().map(|x| String::from_utf16_lossy(x)).collect();
        assert_eq!(l, vec!["A".to_string(), "b1/2c\\d".to_string()]);
    }

    #[test]
    fn colours_and_hypot() {
        assert_eq!(aci_color(7.0), Color::Ink);
        assert_eq!(aci_color(1.0), Color::Rgb(0xff0000));
        assert_eq!(aci_color(30.0).css(), "#ff8000"); // JS aciColor(30)
        assert_eq!(aci_color(251.0).css(), "#505050");
        assert_eq!(js_hypot(3.0, 4.0), 5.0);
        assert!(js_min(1.0, f64::NAN).is_nan());
    }

    #[test]
    fn empty_model() {
        let d = compile_drawing(&Obj::new(), "AC1015", &[]);
        assert_eq!(d.release, "2000");
        assert!(d.spaces.is_empty());
    }
}
