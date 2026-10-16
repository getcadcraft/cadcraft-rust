//! dxf.js `readDxf(text)`: ASCII DXF → what `doc.load` takes. Multileaders, pictures and wipeouts come through the
//! drawing model the DWG import uses (cc-io-dwg's `dxf_model` + [`crate::dwg_import::extras_from`]), as in the JS.

use std::collections::{HashMap, HashSet};

use cc_cad::js;
use cc_cad_geom::geom::{m_apply, m_mul, m_rotate, m_translate, norm_angle, rad};
use cc_cad_geom::jsmath::JsMath;
use cc_geom::P2;
use cc_io_dwg::value::{Obj, V};

use crate::attrs::{self, AttdefOpts, BlockInfo};
use crate::dwg_import::{EXTRA_TYPES, mask_of};
use crate::i18n::{self, Catalog};
use crate::jsv::{self, JsErr, R};
use crate::props;
use crate::small;
use crate::text::{decode_text, mtext_plain};

const TAU: f64 = std::f64::consts::PI * 2.0;
const PI: f64 = std::f64::consts::PI;
/// Copies of one INSERT array at most (the JS has no cap).
const MAX_ARRAY: f64 = 100_000.0;
/// Pieces of one hatch arc edge at most (the JS has no cap).
const MAX_STEPS: f64 = 1_000_000.0;
/// Reads past the end of a hatch's codes (each gives NaN in the JS, which would loop as long as the count says).
const MAX_EXHAUSTED: usize = 100_000;

type Pair<'a> = (f64, &'a str);
type G<'a> = [Pair<'a>];

/// `val(g, code)`: the first value of a code, trimmed.
fn val<'a>(g: &G<'a>, code: f64) -> Option<&'a str> {
    g.iter().find(|(c, _)| *c == code).map(|(_, v)| js::trim(v))
}
/// `num(g, code, d)`
fn num(g: &G, code: f64, d: f64) -> f64 {
    match val(g, code) {
        None => d,
        Some(v) => jsv::parse_float(v),
    }
}
fn num0(g: &G, code: f64) -> f64 {
    num(g, code, 0.0)
}
/// `parseInt(val(g, code) ?? d, 10)`
fn int(g: &G, code: f64, d: &str) -> f64 {
    jsv::parse_int(val(g, code).unwrap_or(d))
}
fn has(g: &G, code: f64) -> bool {
    g.iter().any(|(c, _)| *c == code)
}
fn i32f(x: f64) -> i64 {
    jsv::i32_of(x)
}
/// `'#' + (parseInt(v, 10) & 0xffffff).toString(16).padStart(6, '0')`
fn true_color(v: &str) -> String {
    format!("#{:06x}", i32f(jsv::parse_int(v)) & 0xff_ffff)
}

/// `xdata(g)`: XDATA by application { APP: [[code, value]…] } (values trimmed).
fn xdata(g: &G) -> Obj {
    let mut x = Obj::new();
    let mut app: Option<String> = None;
    for (c, v) in g {
        if *c == 1001.0 {
            let a = jsv::upper(js::trim(v));
            x.set(a.clone(), V::Arr(Vec::new()));
            app = Some(a);
        } else if let Some(a) = app.as_ref().filter(|a| !a.is_empty())
            && *c >= 1000.0
            && let Some(V::Arr(list)) = x.get_mut(a)
        {
            list.push(V::Arr(vec![V::Num(*c), V::Str(js::trim(v).to_string())]));
        }
    }
    x
}
/// `xd.APP` as a list (absent → None).
fn app_list<'a>(x: &'a Obj, app: &str) -> Option<&'a [V]> {
    match x.get(app) {
        Some(V::Arr(a)) => Some(a.as_slice()),
        _ => None,
    }
}
fn values_of(list: &[V]) -> Vec<V> {
    list.iter().map(|it| props::cv(it).1.cloned().unwrap_or(V::Null)).collect()
}

/// `xlist(list, name)`: the list inside an ACAD "<name> { … }" xdata section.
fn xlist(list: Option<&[V]>, name: &str) -> Option<Vec<(f64, V)>> {
    let list = list.unwrap_or(&[]);
    let k = list.iter().position(|it| {
        let (c, v) = props::cv(it);
        jsv::num(c) == Some(1000.0) && jsv::s(v) == Some(name)
    })?;
    let mut out = Vec::new();
    let mut depth = 0i64;
    for it in list.iter().skip(k.saturating_add(1)) {
        let (c, v) = props::cv(it);
        let c = jsv::to_num(c);
        let v = v.cloned().unwrap_or(V::Null);
        if c == 1002.0 {
            let open = jsv::s(Some(&v)) == Some("{");
            depth = depth.saturating_add(if open { 1 } else { -1 });
            if depth == 0 {
                break;
            }
            if open && depth == 1 {
                continue;
            }
        }
        out.push((c, v));
    }
    Some(out)
}

/// `fixStyle(st)`
fn fix_style(st: &mut Obj) {
    if !(jsv::to_num(st.get("scale")) > 0.0) {
        st.set("scale", 1.0);
    }
    if let Some(u) = st.get("unit").cloned() {
        let u = js::round(jsv::to_num(Some(&u)));
        if st.get("lunit").is_none() {
            st.set(
                "lunit",
                if u == 6.0 {
                    4.0
                } else if u == 7.0 {
                    5.0
                } else {
                    u
                },
            );
        }
        st.remove("unit");
    }
    if let Some(l) = st.get("lunit").cloned() {
        let u = js::round(jsv::to_num(Some(&l)));
        if (1.0..=5.0).contains(&u) {
            st.set("lunit", u);
        } else if u == 6.0 {
            st.set("lunit", 2.0);
        } else {
            st.remove("lunit");
        }
    }
    for k in ["dec", "adec", "tad", "zin"] {
        let v = js::max(0.0, js::round(jsv::or(st.get(k), 0.0)));
        st.set(k, v);
    }
    if jsv::to_num(st.get("dec")) > 8.0 {
        st.set("dec", 8.0);
    }
}

fn skip(rep: &mut Obj, t: &str) {
    if !matches!(rep.get("skipped"), Some(V::Obj(_))) {
        rep.set("skipped", Obj::new());
    }
    if let Some(V::Obj(sk)) = rep.get_mut("skipped") {
        let n = jsv::or(sk.get(t), 0.0) + 1.0;
        sk.set(t.to_string(), n);
    }
}
fn inc(rep: &mut Obj, k: &'static str) {
    let n = jsv::or(rep.get(k), 0.0) + 1.0;
    rep.set(k, n);
}
fn new_report() -> Obj {
    let mut r = Obj::new();
    r.set("read", 0.0);
    r.set("skipped", Obj::new());
    r.set("paperSpace", 0.0);
    r.set("notes", V::Arr(Vec::new()));
    r
}

/// `/^standard$/i`
fn is_standard(s: &str) -> bool {
    s.eq_ignore_ascii_case("standard")
}

/// `styleOf(g)`: an object's own linetype (6), lineweight (370) and linetype scale (48).
fn style_of(g: &G, o: &mut Obj) {
    if let Some(lt) = val(g, 6.0).filter(|l| !l.is_empty() && !l.eq_ignore_ascii_case("bylayer")) {
        let v = if lt.eq_ignore_ascii_case("byblock") {
            "ByBlock"
        } else if lt.eq_ignore_ascii_case("continuous") {
            "Continuous"
        } else {
            lt
        };
        o.set("lt", v);
    }
    if let Some(lw) = val(g, 370.0) {
        let n = jsv::parse_int(lw);
        if n == -2.0 || n == -3.0 {
            o.set("lw", n);
        } else if (0.0..=211.0).contains(&n) {
            o.set("lw", n / 100.0);
        }
    }
    let s = val(g, 48.0).map_or(f64::NAN, jsv::parse_float);
    if s > 0.0 && (s - 1.0).abs() > 1e-12 {
        o.set("lts", s);
    }
}

/// `common(g)`: layer, colour, linetype, lineweight, transparency, thickness.
fn common(g: &G) -> Obj {
    let mut color = V::Null;
    if let Some(tc) = val(g, 420.0) {
        color = V::Str(true_color(tc));
    } else if let Some(aci) = val(g, 62.0) {
        let n = jsv::parse_int(aci);
        color = if n == 256.0 { V::Null } else { V::Num(n.abs()) };
    }
    let mut o = Obj::new();
    o.set("layer", val(g, 8.0).filter(|l| !l.is_empty()).unwrap_or("0"));
    o.set("color", color);
    style_of(g, &mut o);
    if let Some(tr) = props::tr_from(val(g, 440.0).map(V::from).as_ref()) {
        o.set("tr", tr);
    }
    let th = num0(g, 39.0);
    if th != 0.0 && !th.is_nan() {
        o.set("th", th);
    }
    o
}

const TH_TYPES: [&str; 6] = ["line", "circle", "arc", "poly", "point", "text"];

/// `hatchPattern(g)`: a pattern HATCH's scale, angle and own pattern lines (hatch.js's shape).
fn hatch_pattern(g: &G, out: &mut Obj) {
    let sc = num(g, 41.0, 1.0);
    let an = num0(g, 52.0);
    if sc > 0.0 && sc != 1.0 {
        out.set("pscale", sc);
    }
    if an != 0.0 && !an.is_nan() {
        out.set("pangle", norm_angle(rad(an)));
    }
    let Some(k) = g.iter().position(|(c, _)| *c == 78.0) else { return };
    struct L {
        angle: f64,
        x: f64,
        y: f64,
        ox: f64,
        oy: f64,
        dashes: Vec<f64>,
    }
    let mut lines: Vec<L> = Vec::new();
    for (c, v) in g.iter().skip(k.saturating_add(1)) {
        let x = jsv::parse_float(v);
        if *c == 53.0 {
            lines.push(L { angle: x, x: 0.0, y: 0.0, ox: 0.0, oy: 0.0, dashes: Vec::new() });
            continue;
        }
        let Some(cur) = lines.last_mut() else { break };
        match *c {
            c if c == 43.0 => cur.x = x,
            c if c == 44.0 => cur.y = x,
            c if c == 45.0 => cur.ox = x,
            c if c == 46.0 => cur.oy = x,
            c if c == 49.0 => cur.dashes.push(x),
            c if c == 79.0 => {}
            _ => break,
        }
    }
    let ok: Vec<&L> =
        lines.iter().filter(|l| [l.angle, l.x, l.y, l.ox, l.oy].iter().chain(l.dashes.iter()).all(|v| v.is_finite())).collect();
    if !ok.is_empty() && ok.len() <= 64 {
        let pl: Vec<V> = ok
            .iter()
            .map(|l| {
                let a = l.angle * PI / 180.0;
                let (ca, sa) = (a.jcos(), a.jsin());
                let mut o = Obj::new();
                o.set("angle", l.angle);
                o.set("x", l.x);
                o.set("y", l.y);
                o.set("dx", l.ox * ca + l.oy * sa);
                o.set("dy", -l.ox * sa + l.oy * ca);
                o.set("dashes", V::Arr(l.dashes.iter().take(32).map(|d| V::Num(*d)).collect()));
                V::Obj(o)
            })
            .collect();
        out.set("patlines", V::Arr(pl));
    }
}

/// `arcInto(ring, p, q, b)`
fn arc_into(ring: &mut Vec<f64>, p: (f64, f64), q: (f64, f64), b: f64) {
    let theta = 4.0 * b.jatan();
    let steps = js::max(2.0, (16.0 * theta.abs() / PI).ceil());
    let c = js::hypot(q.0 - p.0, q.1 - p.1);
    let r = (c / (2.0 * (theta / 2.0).jsin())).abs();
    let (mx, my) = ((p.0 + q.0) / 2.0, (p.1 + q.1) / 2.0);
    let h = r * (theta / 2.0).jcos() * (if b > 0.0 { 1.0 } else { -1.0 });
    let (ux, uy) = ((q.0 - p.0) / c, (q.1 - p.1) / c);
    let (cx, cy) = (mx - uy * h, my + ux * h);
    let a0 = (p.1 - cy).jatan2(p.0 - cx);
    let n = jsv::f2us(steps.min(MAX_STEPS));
    for s in 1..n {
        let a = a0 + theta * jsv::us2f(s) / steps;
        ring.push(cx + r * a.jcos());
        ring.push(cy + r * a.jsin());
    }
}

/// `hatchLoops(g, rep)`: the boundary paths in order (the codes repeat, so position matters).
fn hatch_loops(g: &G, rep: &mut Obj, cat: &Catalog) -> Vec<Vec<f64>> {
    let Some(k0) = g.iter().position(|(c, _)| *c == 91.0) else { return Vec::new() };
    let mut loops = Vec::new();
    let n = g.get(k0).map_or(f64::NAN, |(_, v)| jsv::parse_int(v));
    let mut k = k0.saturating_add(1);
    let mut exhausted = 0usize;
    let next = |k: &mut usize, code: f64, ex: &mut usize| -> f64 {
        while *k < g.len() && g.get(*k).is_some_and(|(c, _)| *c != code) {
            *k = k.saturating_add(1);
        }
        match g.get(*k) {
            Some((_, v)) => {
                *k = k.saturating_add(1);
                jsv::parse_float(v)
            }
            None => {
                *ex = ex.saturating_add(1);
                f64::NAN
            }
        }
    };
    let mut p = 0.0;
    while p < n && k < g.len() {
        p += 1.0;
        let flags = next(&mut k, 92.0, &mut exhausted);
        let mut ring: Vec<f64> = Vec::new();
        if i32f(flags) & 2 != 0 {
            let has_bulge = next(&mut k, 72.0, &mut exhausted) == 1.0;
            let closed = next(&mut k, 73.0, &mut exhausted) == 1.0;
            let cnt = next(&mut k, 93.0, &mut exhausted);
            let mut vs: Vec<(f64, f64, f64)> = Vec::new();
            let mut j = 0.0;
            while j < cnt && exhausted <= MAX_EXHAUSTED {
                j += 1.0;
                let x = next(&mut k, 10.0, &mut exhausted);
                let y = next(&mut k, 20.0, &mut exhausted);
                let b = if has_bulge { next(&mut k, 42.0, &mut exhausted) } else { 0.0 };
                vs.push((x, y, b));
            }
            let len = vs.len();
            for j in 0..len {
                let Some(a) = vs.get(j).copied() else { continue };
                let b2 = vs.get(jsv::wrap_next(j, len)).copied().unwrap_or(a);
                ring.push(a.0);
                ring.push(a.1);
                if a.2.abs() > 1e-12 && (closed || j.saturating_add(1) < len) {
                    arc_into(&mut ring, (a.0, a.1), (b2.0, b2.1), a.2);
                }
            }
            if let Some(v0) = vs.first() {
                ring.push(v0.0);
                ring.push(v0.1);
            }
        } else {
            let edges = next(&mut k, 93.0, &mut exhausted);
            let mut ok = true;
            let mut j = 0.0;
            while j < edges && exhausted <= MAX_EXHAUSTED {
                j += 1.0;
                let t = next(&mut k, 72.0, &mut exhausted);
                if t == 1.0 {
                    let x1 = next(&mut k, 10.0, &mut exhausted);
                    let y1 = next(&mut k, 20.0, &mut exhausted);
                    let x2 = next(&mut k, 11.0, &mut exhausted);
                    let y2 = next(&mut k, 21.0, &mut exhausted);
                    ring.extend([x1, y1, x2, y2]);
                } else if t == 2.0 {
                    let cx = next(&mut k, 10.0, &mut exhausted);
                    let cy = next(&mut k, 20.0, &mut exhausted);
                    let r = next(&mut k, 40.0, &mut exhausted);
                    let s = rad(next(&mut k, 50.0, &mut exhausted));
                    let e = rad(next(&mut k, 51.0, &mut exhausted));
                    let ccw = next(&mut k, 73.0, &mut exhausted) == 1.0;
                    let (mut a0, mut a1) = (s, e);
                    if !ccw {
                        a0 = -s;
                        a1 = -e;
                    }
                    if a1 <= a0 {
                        a1 += TAU;
                    }
                    let steps = js::max(4.0, (32.0 * (a1 - a0) / TAU).ceil());
                    let sn = jsv::f2us(steps.min(MAX_STEPS));
                    if !steps.is_nan() {
                        for q in 0..=sn {
                            let a = a0 + (a1 - a0) * jsv::us2f(q) / steps;
                            ring.push(cx + r * a.jcos());
                            ring.push(cy + r * a.jsin());
                        }
                    }
                } else {
                    ok = false;
                    break;
                }
            }
            if !ok {
                skip(rep, &cat.t("dxf.hatch_boundary_ellipse_spline", &[]));
                continue;
            }
        }
        if ring.len() >= 6 {
            loops.push(ring);
        }
    }
    loops
}

/// `splinePoints(g)`: fit points when given, else the B-spline evaluated from control points and knots.
fn spline_points(g: &G) -> R<Vec<(f64, f64)>> {
    let mut ctrl: Vec<(f64, f64)> = Vec::new();
    let mut fit: Vec<(f64, f64)> = Vec::new();
    let mut knots: Vec<f64> = Vec::new();
    let mut weights: Vec<f64> = Vec::new();
    for (c, v) in g {
        match *c {
            c if c == 10.0 => ctrl.push((jsv::parse_float(v), 0.0)),
            c if c == 20.0 => {
                if let Some(l) = ctrl.last_mut() {
                    l.1 = jsv::parse_float(v);
                }
            }
            c if c == 11.0 => fit.push((jsv::parse_float(v), 0.0)),
            c if c == 21.0 => {
                if let Some(l) = fit.last_mut() {
                    l.1 = jsv::parse_float(v);
                }
            }
            c if c == 40.0 => knots.push(jsv::parse_float(v)),
            c if c == 41.0 => weights.push(jsv::parse_float(v)),
            _ => {}
        }
    }
    let deg = int(g, 71.0, "3");
    let n = jsv::us2f(ctrl.len());
    if n > deg && jsv::us2f(knots.len()) == n + deg + 1.0 {
        if deg < 0.0 {
            return Err(JsErr("Cannot read properties of undefined (reading 'x')".into()));
        }
        // a degree this high would run for minutes in the JS: the control points stand in (hostile files only)
        if deg <= 200.0 {
            return Ok(de_boor(&ctrl, &knots, jsv::f2us(deg), &weights));
        }
        return Ok(ctrl);
    }
    Ok(if fit.len() >= 2 { fit } else { ctrl })
}

fn de_boor(pts: &[(f64, f64)], u_: &[f64], p: usize, w_: &[f64]) -> Vec<(f64, f64)> {
    let n = pts.len();
    let uk = |i: usize| u_.get(i).copied().unwrap_or(f64::NAN);
    let (lo, hi) = (uk(p), uk(n));
    let steps = js::min(400.0, jsv::us2f(n) * 8.0);
    let su = jsv::f2us(steps);
    let mut out = Vec::with_capacity(su.saturating_add(1));
    for s in 0..=su {
        let sf = jsv::us2f(s);
        let u = if s == su { hi - 1e-12 * jsv::or(Some(&V::Num(hi)), 1.0).abs() } else { lo + (hi - lo) * sf / steps };
        let mut k = p;
        while k < n.saturating_sub(1) && uk(k.saturating_add(1)) <= u {
            k = k.saturating_add(1);
        }
        let mut d: Vec<(f64, f64, f64)> = Vec::with_capacity(p.saturating_add(1));
        for j in 0..=p {
            let idx = k.saturating_sub(p).saturating_add(j);
            let w = w_.get(idx).copied().unwrap_or(1.0);
            let q = pts.get(idx).copied().unwrap_or((f64::NAN, f64::NAN));
            d.push((q.0 * w, q.1 * w, w));
        }
        for r in 1..=p {
            let mut j = p;
            while j >= r {
                let i = k.saturating_sub(p).saturating_add(j);
                let den = uk(i.saturating_add(p).saturating_sub(r).saturating_add(1)) - uk(i);
                let a = if den != 0.0 && !den.is_nan() { (u - uk(i)) / den } else { 0.0 };
                let prev = d.get(j.saturating_sub(1)).copied().unwrap_or((f64::NAN, f64::NAN, f64::NAN));
                if let Some(dj) = d.get_mut(j) {
                    *dj = ((1.0 - a) * prev.0 + a * dj.0, (1.0 - a) * prev.1 + a * dj.1, (1.0 - a) * prev.2 + a * dj.2);
                }
                if j == 0 {
                    break;
                }
                j = j.saturating_sub(1);
                if j < r {
                    break;
                }
            }
        }
        let l = d.get(p).copied().unwrap_or((f64::NAN, f64::NAN, f64::NAN));
        out.push((l.0 / l.2, l.1 / l.2));
    }
    out
}

/// `dimUsable(d)`: can CadCraft draw this dimension itself?
fn dim_usable(d: &Obj) -> bool {
    let pt = |k: &str| d.get(k).map(|p| (jsv::to_num(jsv::get(Some(p), "x")), jsv::to_num(jsv::get(Some(p), "y"))));
    let pts: Vec<(f64, f64)> = ["p1", "p2", "c", "loc"].iter().filter_map(|k| pt(k)).collect();
    if !pts.iter().all(|p| p.0.is_finite() && p.1.is_finite()) {
        return false;
    }
    let far = |a: Option<(f64, f64)>, b: Option<(f64, f64)>| match (a, b) {
        (Some(a), Some(b)) => js::hypot(b.0 - a.0, b.1 - a.1) > 1e-12,
        _ => false,
    };
    match jsv::s(d.get("kind")) {
        Some("linear" | "aligned") => far(pt("p1"), pt("p2")) && jsv::nn(d.get("rot"), 0.0).is_finite(),
        Some("radius" | "diameter") => far(pt("c"), pt("p1")),
        _ => far(pt("c"), pt("p1")) && far(pt("c"), pt("p2")) && far(pt("c"), pt("loc")),
    }
}

/// A LAYOUT object (R2000+).
struct LayoutObj {
    name: String,
    tab: f64,
    record: String,
    min: (f64, f64),
    max: (f64, f64),
    paper_mm: (f64, f64),
    units: f64,
    rot: f64,
}

/// One follower of a POLYLINE / INSERT (VERTEX or ATTRIB).
struct Follow<'p, 't> {
    typ: &'t str,
    g: &'p G<'t>,
}

/// `Object.values(header[var])[0]`: array-index codes ascending first, then the rest in insertion order.
fn first_value(v: &[(f64, String)]) -> Option<&String> {
    let idx = v.iter().filter(|(c, _)| is_index_key(*c)).min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    match idx {
        Some((_, s)) => Some(s),
        None => v.first().map(|(_, s)| s),
    }
}
fn is_index_key(c: f64) -> bool {
    c >= 0.0 && c.fract() == 0.0 && c < 4_294_967_295.0
}

struct Reader<'p, 't> {
    pairs: &'p [Pair<'t>],
    i: usize,
    cat: &'p Catalog,
    entities: Vec<V>,
    layers: Vec<V>,
    blocks: Vec<V>,
    layouts: Vec<V>,
    dim_style: Obj,
    meta: Obj,
    report: Obj,
    /// the keys readDxf adds to `out` after the first ones (dimStyles, textStyles, linetypes, views, ucs, …)
    extra: Obj,
    vports_raw: Option<Vec<V>>,
    views: Option<Vec<(Obj, bool)>>,
    header: Vec<(String, Vec<(f64, String)>)>,
    dim_styles: HashMap<String, Obj>,
    block_records: HashMap<String, String>,
    style_handles: HashMap<String, Option<String>>,
    layout_objs: Vec<LayoutObj>,
    paper_blocks: Vec<(String, Vec<V>)>,
    paper_raw: Vec<V>,
    no_plot: HashSet<String>,
    layer_by_h: HashMap<String, String>,
    dict_by_h: HashMap<String, HashMap<String, String>>,
    filter_by_h: HashMap<String, V>,
}

impl<'p, 't> Reader<'p, 't> {
    fn at(&self) -> (f64, &'t str) {
        self.pairs.get(self.i).copied().unwrap_or((0.0, "EOF"))
    }
    fn at_is(&self, c: f64, v: &str) -> bool {
        let (ac, av) = self.at();
        ac == c && js::trim(av) == v
    }
    fn more(&self) -> bool {
        self.i < self.pairs.len()
    }
    fn step(&mut self) {
        self.i = self.i.saturating_add(1);
    }
    /// `group()`: the pairs up to the next 0 code.
    fn group(&mut self) -> &'p G<'t> {
        let start = self.i;
        while self.more() && self.pairs.get(self.i).is_some_and(|(c, _)| *c != 0.0) {
            self.step();
        }
        self.pairs.get(start..self.i).unwrap_or(&[])
    }
    fn header_get(&self, name: &str) -> Option<&Vec<(f64, String)>> {
        self.header.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }
    fn header_code(&self, name: &str, code: f64) -> Option<&str> {
        self.header_get(name).and_then(|v| v.iter().find(|(c, _)| *c == code).map(|(_, s)| s.as_str()))
    }

    fn skip_section(&mut self) {
        while self.more() && !self.at_is(0.0, "ENDSEC") {
            self.step();
        }
        self.step();
    }

    fn read_header(&mut self) {
        let mut var = String::new();
        while self.more() && !self.at_is(0.0, "ENDSEC") {
            let (c, v) = self.at();
            if c == 9.0 {
                var = js::trim(v).to_string();
            } else {
                let tv = js::trim(v);
                if var == "$ACADVER" && c == 1.0 {
                    self.meta.set("version", tv);
                } else if var == "$INSUNITS" && c == 70.0 {
                    let n = js::str_to_number(v);
                    self.meta.set("units", if n == 0.0 { "" } else { crate::dwg_import::units_name(Some(&V::Num(n))) });
                } else if var == "$LTSCALE" && c == 40.0 && jsv::parse_float(v) > 0.0 && jsv::parse_float(v) != 1.0 {
                    self.meta.set("ltscale", jsv::parse_float(v));
                } else if var == "$LUNITS" && c == 70.0 && {
                    let n = js::str_to_number(v);
                    (1.0..=5.0).contains(&n) && n != 2.0
                } {
                    self.meta.set("lunits", js::str_to_number(v));
                } else if let Some(key) = header_dimvar(&var) {
                    self.dim_style.set(key, jsv::parse_float(v));
                }
                let slot = match self.header.iter_mut().position(|(n, _)| *n == var) {
                    Some(p) => p,
                    None => {
                        self.header.push((var.clone(), Vec::new()));
                        self.header.len().saturating_sub(1)
                    }
                };
                if let Some((_, codes)) = self.header.get_mut(slot) {
                    match codes.iter_mut().find(|(k, _)| *k == c || (k.is_nan() && c.is_nan())) {
                        Some(e) => e.1 = tv.to_string(),
                        None => codes.push((c, tv.to_string())),
                    }
                }
            }
            self.step();
        }
        self.step();
        fix_style(&mut self.dim_style);
    }

    fn read_tables(&mut self) {
        while self.more() && !self.at_is(0.0, "ENDSEC") {
            let (c0, v0) = self.at();
            let t = if c0 == 0.0 { js::trim(v0) } else { "" };
            match t {
                "LAYER" => {
                    self.step();
                    let g = self.group();
                    if let Some(name) = val(g, 2.0).filter(|n| !n.is_empty()) {
                        let col = int(g, 62.0, "7");
                        let flags = i32f(int(g, 70.0, "0"));
                        let lw = int(g, 370.0, "-3");
                        if let Some(lh) = val(g, 5.0).filter(|h| !h.is_empty()) {
                            self.layer_by_h.insert(jsv::upper(lh), name.to_string());
                        }
                        let mut o = Obj::new();
                        o.set("name", name);
                        o.set(
                            "color",
                            match val(g, 420.0) {
                                Some(tc) => V::Str(true_color(tc)),
                                None => {
                                    let a = col.abs();
                                    V::Num(if a != 0.0 && !a.is_nan() { a } else { 7.0 })
                                }
                            },
                        );
                        o.set("on", col >= 0.0);
                        o.set("frozen", flags & 1 != 0);
                        o.set("locked", flags & 4 != 0);
                        if flags & 2 != 0 {
                            o.set("newVpFrozen", true);
                        }
                        o.set("plot", val(g, 290.0) != Some("0"));
                        o.set("linetype", val(g, 6.0).filter(|l| !l.is_empty()).unwrap_or("Continuous"));
                        o.set("lineweight", if (0.0..=211.0).contains(&lw) { lw / 100.0 } else { -3.0 });
                        self.layers.push(V::Obj(o));
                    }
                }
                "DIMSTYLE" => {
                    self.step();
                    let g = self.group();
                    if let Some(name) = val(g, 2.0).filter(|n| !n.is_empty()) {
                        let mut st = self.dim_style.clone();
                        for (code, key) in DIMVAR_CODES {
                            if let Some(x) = val(g, code) {
                                st.set(key, jsv::parse_float(x));
                            }
                        }
                        props::st_from_codes(
                            &|c| {
                                let cf = jsv::i2f(c);
                                if c == 3 || c == 4 {
                                    g.iter().find(|(k, _)| *k == cf).map(|(_, v)| V::from(*v))
                                } else {
                                    val(g, cf).map(V::from)
                                }
                            },
                            &mut st,
                        );
                        if has(g, 1001.0) {
                            let x = xdata(g);
                            let strs: Vec<String> = app_list(&x, "CADCRAFT")
                                .unwrap_or(&[])
                                .iter()
                                .map(|it| decode_text(&jsv::to_str(props::cv(it).1)))
                                .collect();
                            props::cc_dim_from(&strs, &mut st);
                        }
                        let txn =
                            val(g, 340.0).filter(|h| !h.is_empty()).and_then(|h| self.style_handles.get(&jsv::upper(h)).cloned().flatten());
                        if let Some(txn) = txn.filter(|n| !n.is_empty() && !is_standard(n)) {
                            st.set("txsty", txn);
                        }
                        fix_style(&mut st);
                        self.dim_styles.insert(jsv::upper(name), st.clone());
                        if !is_standard(name) && i32f(int(g, 70.0, "0")) & 16 == 0 {
                            push_extra(&mut self.extra, "dimStyles", V::Arr(vec![V::from(name), V::Obj(st)]));
                        }
                    }
                }
                "STYLE" => {
                    self.step();
                    let g = self.group();
                    let name = val(g, 2.0);
                    let fl = i32f(int(g, 70.0, "0"));
                    if let Some(h) = val(g, 5.0).filter(|h| !h.is_empty()) {
                        self.style_handles.insert(jsv::upper(h), name.map(str::to_string));
                    }
                    if let Some(name) = name.filter(|n| !n.is_empty() && fl & 1 == 0 && fl & 16 == 0 && !is_standard(n)) {
                        let mut o = Obj::new();
                        o.set("font", val(g, 3.0).unwrap_or(""));
                        o.set("h", num0(g, 40.0));
                        o.set("wf", jsv::pn_or(Some(&V::Num(num(g, 41.0, 1.0))), 1.0));
                        o.set("obl", rad(num0(g, 50.0)));
                        push_extra(&mut self.extra, "textStyles", V::Arr(vec![V::from(name), V::Obj(o)]));
                    }
                }
                "LTYPE" => {
                    self.step();
                    let g = self.group();
                    let name = val(g, 2.0);
                    let pattern: Vec<V> = g
                        .iter()
                        .filter(|(c, _)| *c == 49.0)
                        .map(|(_, v)| jsv::parse_float(v))
                        .filter(|x| x.is_finite())
                        .map(V::Num)
                        .collect();
                    if let Some(name) = name.filter(|n| {
                        !n.is_empty()
                            && !pattern.is_empty()
                            && !["continuous", "bylayer", "byblock"].iter().any(|x| n.eq_ignore_ascii_case(x))
                    }) {
                        let mut o = Obj::new();
                        o.set("name", name);
                        o.set("description", val(g, 3.0).unwrap_or(""));
                        o.set("pattern", V::Arr(pattern));
                        push_extra(&mut self.extra, "linetypes", V::Obj(o));
                    }
                }
                "VIEW" => {
                    self.step();
                    let g = self.group();
                    let name = val(g, 2.0);
                    let (w, h) = (num0(g, 41.0), num0(g, 40.0));
                    if let Some(name) = name.filter(|n| !n.is_empty())
                        && w > 0.0
                        && h > 0.0
                    {
                        let mut o = Obj::new();
                        o.set("name", name);
                        o.set("cx", num0(g, 10.0) + num0(g, 12.0));
                        o.set("cy", num0(g, 20.0) + num0(g, 22.0));
                        o.set("w", w);
                        o.set("h", h);
                        let paper = i32f(int(g, 70.0, "0")) & 1 != 0;
                        if self.views.is_none() && !self.extra.has("views") {
                            self.extra.set("views", V::Null);
                        }
                        self.views.get_or_insert_with(Vec::new).push((o, paper));
                    }
                }
                "VPORT" => {
                    self.step();
                    let g = self.group();
                    if val(g, 2.0).unwrap_or("").eq_ignore_ascii_case("*active") {
                        let x = xdata(g);
                        let sheet = app_list(&x, "CADCRAFT")
                            .unwrap_or(&[])
                            .iter()
                            .map(|it| decode_text(&jsv::to_str(jsv::at(Some(it), 1))))
                            .find(|s| s.starts_with("PANESHEET:"));
                        let mut o = Obj::new();
                        o.set("x0", num0(g, 10.0));
                        o.set("y0", num0(g, 20.0));
                        o.set("x1", num0(g, 11.0));
                        o.set("y1", num0(g, 21.0));
                        o.set("cx", num0(g, 12.0));
                        o.set("cy", num0(g, 22.0));
                        o.set("h", num0(g, 40.0));
                        o.set("tw", rad(num0(g, 51.0)));
                        if let Some(s) = sheet {
                            o.set("sheet", crate::text::slice16(&s, 10, usize::MAX));
                        }
                        self.vports_raw.get_or_insert_with(Vec::new).push(V::Obj(o));
                    }
                }
                "UCS" => {
                    self.step();
                    let g = self.group();
                    let name = val(g, 2.0);
                    let (x, y) = (num0(g, 11.0), num0(g, 21.0));
                    if let Some(name) = name.filter(|n| !n.is_empty())
                        && ((x != 0.0 && !x.is_nan()) || (y != 0.0 && !y.is_nan()))
                    {
                        let u =
                            V::Obj(cc_io_dwg::obj! { "name" => name, "x" => num0(g, 10.0), "y" => num0(g, 20.0), "ang" => y.jatan2(x) });
                        ucs_mut(&mut self.extra, |o| {
                            if let Some(V::Arr(n)) = o.get_mut("named") {
                                n.push(u);
                            }
                        });
                    }
                }
                "BLOCK_RECORD" => {
                    self.step();
                    let g = self.group();
                    if let (Some(h), Some(n)) = (val(g, 5.0).filter(|h| !h.is_empty()), val(g, 2.0).filter(|n| !n.is_empty())) {
                        self.block_records.insert(jsv::upper(h), n.to_string());
                    }
                }
                _ => self.step(),
            }
        }
        self.step();
    }

    fn read_blocks(&mut self) -> R<()> {
        while self.more() && !self.at_is(0.0, "ENDSEC") {
            if !self.at_is(0.0, "BLOCK") {
                self.step();
                continue;
            }
            self.step();
            let g = self.group();
            let name = val(g, 2.0).unwrap_or("").to_string();
            let flags = i32f(int(g, 70.0, "0"));
            let base = jsv::p2(num0(g, 10.0), num0(g, 20.0));
            let mut throwaway = new_report();
            let all = self.read_entity_list(&mut throwaway, false, true)?.0;
            let mut ents = Vec::new();
            let mut attdefs = Vec::new();
            for e in all {
                if jsv::s(jsv::get(Some(&e), "type")) == Some("attdef") {
                    attdefs.push(jsv::get(Some(&e), "def").cloned().unwrap_or(V::Null));
                } else {
                    ents.push(e);
                }
            }
            if js::trim(self.at().1) == "ENDBLK" {
                self.step();
                self.group();
            }
            let nv = V::Str(name.clone());
            if small::is_param_block(Some(&nv)) {
                self.extra.set("parametric", small::parametric_text(&ents));
                continue;
            }
            if crate::model3d::is_m3d_block(Some(&nv)) {
                if let Some(t) = small::read3d_text(&ents) {
                    self.extra.set("model3d", t);
                }
                if let Some(v) = crate::model3d::read3d_view(&ents) {
                    self.extra.set("view3d", v);
                }
                continue;
            }
            let lower = name.to_ascii_lowercase();
            if let Some(rest) = lower.strip_prefix("*paper_space")
                && !rest.is_empty()
                && rest.bytes().all(|c| c.is_ascii_digit())
            {
                let up = jsv::upper(&name);
                match self.paper_blocks.iter_mut().find(|(n, _)| *n == up) {
                    Some(slot) => slot.1 = ents,
                    None => self.paper_blocks.push((up, ents)),
                }
                continue;
            }
            if ["*model_space", "*paper_space", "$model_space", "$paper_space"].iter().any(|p| lower.starts_with(p)) {
                continue;
            }
            if flags & 4 != 0 {
                let path = val(g, 1.0).filter(|p| !p.is_empty()).unwrap_or(&name).to_string();
                let mut b = Obj::new();
                b.set("name", name.clone());
                b.set("base", base);
                b.set("entities", V::Arr(Vec::new()));
                b.set("xref", V::Obj(cc_io_dwg::obj! { "path" => path, "loaded" => false }));
                self.blocks.push(V::Obj(b));
                inc(&mut self.report, "xrefs");
                continue;
            }
            let cclayout = lower.strip_prefix("cclayout_").is_some_and(|r| !r.is_empty() && r.bytes().all(|c| c.is_ascii_digit()));
            if cclayout && let Some(lay) = layout_from_block(&ents) {
                self.layouts.push(V::Obj(lay));
                continue;
            }
            let mut b = Obj::new();
            b.set("name", name);
            b.set("base", base);
            b.set("entities", V::Arr(ents));
            if !attdefs.is_empty() {
                b.set("attdefs", V::Arr(attdefs));
            }
            self.blocks.push(V::Obj(b));
        }
        self.step();
        Ok(())
    }

    fn read_objects(&mut self) {
        while self.more() && !self.at_is(0.0, "ENDSEC") {
            if self.at_is(0.0, "LAYOUT") {
                self.step();
                let g = self.group();
                let k = g.iter().position(|(c, v)| *c == 100.0 && js::trim(v) == "AcDbLayout");
                let (lg, pg) = match k {
                    Some(k) => (g.get(k..).unwrap_or(&[]), g.get(..k).unwrap_or(&[])),
                    None => (g, g),
                };
                self.layout_objs.push(LayoutObj {
                    name: val(lg, 1.0).filter(|n| !n.is_empty()).unwrap_or("Layout").to_string(),
                    tab: int(lg, 71.0, "0"),
                    record: jsv::upper(val(lg, 330.0).unwrap_or("")),
                    min: (num0(lg, 10.0), num0(lg, 20.0)),
                    max: (num0(lg, 11.0), num0(lg, 21.0)),
                    paper_mm: (num0(pg, 44.0), num0(pg, 45.0)),
                    units: int(pg, 72.0, "-1"),
                    rot: int(pg, 73.0, "0"),
                });
            } else if self.at_is(0.0, "DICTIONARY") || self.at_is(0.0, "SPATIAL_FILTER") {
                let typ = js::trim(self.at().1).to_string();
                self.step();
                let g = self.group();
                let hd = jsv::upper(val(g, 5.0).unwrap_or(""));
                if typ == "DICTIONARY" {
                    let mut e: HashMap<String, String> = HashMap::new();
                    for w in g.windows(2) {
                        if let [(c1, v1), (c2, v2)] = w
                            && *c1 == 3.0
                            && *c2 == 350.0
                        {
                            e.insert(js::trim(v1).to_string(), jsv::upper(js::trim(v2)));
                        }
                    }
                    self.dict_by_h.insert(hd, e);
                } else {
                    let k0 = g.iter().position(|(c, _)| *c == 70.0);
                    let mut pts = Vec::new();
                    let mut k = k0.map_or(0, |k| k.saturating_add(1));
                    while k.saturating_add(1) < g.len() && g.get(k).is_some_and(|(c, _)| *c == 10.0) {
                        let x = g.get(k).map_or(f64::NAN, |(_, v)| jsv::parse_float(v));
                        let y = g.get(k.saturating_add(1)).map_or(f64::NAN, |(_, v)| jsv::parse_float(v));
                        pts.push(jsv::p2(x, y));
                        k = k.saturating_add(2);
                    }
                    let m: Vec<V> = g.iter().filter(|(c, _)| *c == 40.0).map(|(_, v)| V::Num(jsv::parse_float(v))).collect();
                    let mut f = Obj::new();
                    f.set("points", V::Arr(pts));
                    if m.len() == 24 {
                        f.set("inverseBlockTransform", V::Arr(m.get(..12).unwrap_or(&[]).to_vec()));
                        f.set("clipTransform", V::Arr(m.get(12..).unwrap_or(&[]).to_vec()));
                    }
                    self.filter_by_h.insert(hd, V::Obj(f));
                }
            } else {
                self.step();
            }
        }
        self.step();
    }

    fn read_followers(&mut self) -> Vec<Follow<'p, 't>> {
        let mut subs = Vec::new();
        loop {
            let (c, v) = self.at();
            let t = js::trim(v);
            if !(self.more() && c == 0.0 && (t == "VERTEX" || t == "ATTRIB")) {
                break;
            }
            self.step();
            let g = self.group();
            subs.push(Follow { typ: t, g });
        }
        if self.at_is(0.0, "SEQEND") {
            self.step();
            self.group();
        }
        subs
    }

    /// `readEntityList(report, top, inBlock)` → (model, paper).
    fn read_entity_list(&mut self, rep: &mut Obj, top: bool, in_block: bool) -> R<(Vec<V>, Vec<V>)> {
        let mut model = Vec::new();
        let mut paper = Vec::new();
        while self.more() {
            let (c, v) = self.at();
            let typ = js::trim(v);
            if c != 0.0 {
                self.step();
                continue;
            }
            if typ == "ENDSEC" || typ == "ENDBLK" {
                break;
            }
            self.step();
            let g = self.group();
            let follow =
                if typ == "POLYLINE" || (typ == "INSERT" && val(g, 66.0) == Some("1")) { Some(self.read_followers()) } else { None };
            let in_paper = top && val(g, 67.0) == Some("1");
            let ctx = Ctx { dim_styles: &self.dim_styles, dim_style: &self.dim_style, cat: self.cat };
            let Some(mut made) = entity_from(typ, g, follow.as_deref(), rep, Some(&ctx))? else { continue };
            if let V::Obj(o) = &mut made
                && has(g, 1001.0)
            {
                let x = xdata(g);
                let vals = values_of(app_list(&x, "CADCRAFT").unwrap_or(&[]));
                if let Some(k) = small::ref_key_in(&vals) {
                    o.set("ccref", k);
                }
            }
            if jsv::s(jsv::get(Some(&made), "type")) == Some("attdef") && !in_block {
                skip(rep, &self.cat.t("dxf.attdef_outside_block", &[]));
                continue;
            }
            for e in jsv::flat1(made) {
                if in_paper {
                    paper.push(e);
                } else {
                    model.push(e);
                }
            }
            if in_paper {
                inc(rep, "paperSpace");
            } else {
                inc(rep, "read");
            }
        }
        Ok((model, paper))
    }
}

/// HEADER_DIMVARS
fn header_dimvar(name: &str) -> Option<&'static str> {
    Some(match name {
        "$DIMSCALE" => "scale",
        "$DIMASZ" => "asz",
        "$DIMEXO" => "exo",
        "$DIMDLI" => "dli",
        "$DIMEXE" => "exe",
        "$DIMTXT" => "txt",
        "$DIMGAP" => "gap",
        "$DIMTAD" => "tad",
        "$DIMZIN" => "zin",
        "$DIMDEC" => "dec",
        "$DIMADEC" => "adec",
        "$DIMLUNIT" => "lunit",
        "$DIMUNIT" => "unit",
        _ => return None,
    })
}
/// DIMVAR_CODES in `Object.entries` order (integer keys ascending).
const DIMVAR_CODES: [(f64, &str); 13] = [
    (40.0, "scale"),
    (41.0, "asz"),
    (42.0, "exo"),
    (43.0, "dli"),
    (44.0, "exe"),
    (77.0, "tad"),
    (78.0, "zin"),
    (140.0, "txt"),
    (147.0, "gap"),
    (179.0, "adec"),
    (270.0, "unit"),
    (271.0, "dec"),
    (277.0, "lunit"),
];

/// `(out.k ||= []).push(v)`
fn push_extra(extra: &mut Obj, k: &'static str, v: V) {
    if !matches!(extra.get(k), Some(V::Arr(_))) {
        extra.set(k, V::Arr(Vec::new()));
    }
    if let Some(V::Arr(a)) = extra.get_mut(k) {
        a.push(v);
    }
}
/// `(out.ucs ||= { current: null, named: [] })`
fn ucs_mut(extra: &mut Obj, f: impl FnOnce(&mut Obj)) {
    if !extra.get("ucs").is_some_and(V::truthy) {
        extra.set("ucs", V::Obj(cc_io_dwg::obj! { "current" => V::Null, "named" => V::Arr(Vec::new()) }));
    }
    if let Some(V::Obj(o)) = extra.get_mut("ucs") {
        f(o);
    }
}

/// What entityFrom reads besides the group: the dimension styles so far.
struct Ctx<'a> {
    dim_styles: &'a HashMap<String, Obj>,
    dim_style: &'a Obj,
    cat: &'a Catalog,
}

/// `layoutFromBlock(ents)`: a CCLAYOUT_n block (CadCraft's extra layouts in R12) → a layout, or None.
fn layout_from_block(ents: &[V]) -> Option<Obj> {
    let is_mark = |e: &V| {
        let cc = jsv::get(Some(e), "cc");
        jsv::truthy(cc) && jsv::s(jsv::at(cc, 0)) == Some("LAYOUT")
    };
    let mi = ents.iter().position(is_mark)?;
    let mark = ents.get(mi)?;
    let cc = jsv::get(Some(mark), "cc");
    let c = |i: usize| jsv::at(cc, i);
    let mut paper = Obj::new();
    paper.set("unit", if jsv::s(c(2)) == Some("in") { "in" } else { "mm" });
    paper.set_opt("key", c(3).filter(|k| k.truthy()).cloned());
    paper.set("x0", jsv::pn_or(c(4), 0.0));
    paper.set("y0", jsv::pn_or(c(5), 0.0));
    paper.set("w", jsv::to_num(c(6)));
    paper.set("h", jsv::to_num(c(7)));
    let entities: Vec<V> = ents
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != mi)
        .map(|(_, e)| {
            let ecc = jsv::get(Some(e), "cc");
            let pts = jsv::arr(jsv::get(Some(e), "pts"));
            if jsv::truthy(ecc)
                && jsv::s(jsv::at(ecc, 0)) == Some("VIEWPORT")
                && jsv::s(jsv::get(Some(e), "type")) == Some("poly")
                && pts.len() >= 4
            {
                let xs: Vec<f64> = pts.iter().map(|p| jsv::to_num(jsv::get(Some(p), "x"))).collect();
                let ys: Vec<f64> = pts.iter().map(|p| jsv::to_num(jsv::get(Some(p), "y"))).collect();
                let (x1, x2, y1, y2) = (
                    js::min_of(xs.iter().copied()),
                    js::max_of(xs.iter().copied()),
                    js::min_of(ys.iter().copied()),
                    js::max_of(ys.iter().copied()),
                );
                let frz: Vec<V> = if jsv::s(jsv::at(ecc, 4)) == Some("FRZ") {
                    jsv::arr(ecc).iter().skip(5).filter(|x| x.truthy()).cloned().collect()
                } else {
                    Vec::new()
                };
                let mut o = Obj::new();
                o.set("type", "viewport");
                o.set_opt("layer", jsv::get(Some(e), "layer").cloned());
                o.set_opt("color", jsv::get(Some(e), "color").cloned());
                o.set("x", (x1 + x2) / 2.0);
                o.set("y", (y1 + y2) / 2.0);
                o.set("w", x2 - x1);
                o.set("h", y2 - y1);
                o.set("cx", jsv::to_num(jsv::at(ecc, 1)));
                o.set("cy", jsv::to_num(jsv::at(ecc, 2)));
                o.set("s", jsv::to_num(jsv::at(ecc, 3)));
                if !frz.is_empty() {
                    o.set("frz", V::Arr(frz));
                }
                return V::Obj(o);
            }
            let mut c = jsv::obj(Some(e)).cloned().unwrap_or_default();
            c.remove("cc");
            V::Obj(c)
        })
        .collect();
    let mut l = Obj::new();
    l.set("name", decode_text(&jsv::to_str(Some(&jsv::orv(c(1), V::from("Layout"))))));
    l.set("paper", paper);
    l.set("entities", V::Arr(entities));
    Some(l)
}

/// `splitViewports(list)`: the first viewport (the sheet itself) is dropped; it may carry CadCraft's layout data.
fn split_viewports(list: &[V]) -> (Vec<V>, Option<V>) {
    let mut overall: Option<V> = None;
    let mut entities = Vec::new();
    for e in list {
        let vp = jsv::s(jsv::get(Some(e), "type")) == Some("viewport");
        if vp && (jsv::truthy(jsv::get(Some(e), "overall")) || (overall.is_none() && jsv::num(jsv::get(Some(e), "id")) == Some(1.0))) {
            if overall.is_none() {
                overall = Some(e.clone());
            }
            continue;
        }
        if vp {
            let mut v = jsv::obj(Some(e)).cloned().unwrap_or_default();
            v.remove("id");
            v.remove("overall");
            v.remove("cc");
            entities.push(V::Obj(v));
            continue;
        }
        entities.push(e.clone());
    }
    (entities, overall)
}

fn ptv(g: &G, c: f64) -> V {
    jsv::p2(num0(g, c), num0(g, c + 10.0))
}

/// `entityFrom(type, g, follow, rep, ctx)`: entityFrom0 + the Properties fields the file keeps.
fn entity_from(typ: &str, g: &G, follow: Option<&[Follow]>, rep: &mut Obj, ctx: Option<&Ctx>) -> R<Option<V>> {
    if val(g, 60.0) == Some("1") && typ != "ATTDEF" && typ != "VIEWPORT" {
        inc(rep, "hidden");
        return Ok(None);
    }
    let Some(mut made) = entity_from0(typ, g, follow, rep, ctx)? else { return Ok(None) };
    let one_type = match &made {
        V::Arr(a) => match a.first() {
            Some(V::Obj(o)) => jsv::to_str(o.get("type")),
            _ => return Ok(Some(made)),
        },
        V::Obj(o) => jsv::to_str(o.get("type")),
        _ => String::new(),
    };
    if one_type == "attdef" || one_type == "face3d" {
        return Ok(Some(made));
    }
    let mut add = if has(g, 1001.0) { props::props_from_xdata(Some(&V::Obj(xdata(g)))) } else { Obj::new() };
    if one_type != "hatch" {
        for k in ["bg", "ox", "oy", "assoc"] {
            add.remove(k);
        }
    }
    let zc = match typ {
        "CIRCLE" | "ARC" | "POINT" | "ELLIPSE" | "INSERT" | "MTEXT" | "TEXT" | "ATTRIB" | "POLYLINE" | "HATCH" => Some(30.0),
        "LWPOLYLINE" => Some(38.0),
        _ => None,
    };
    if let Some(z) = zc {
        let n = num0(g, z);
        if n != 0.0 && !n.is_nan() {
            add.set("z", n);
        }
    }
    if typ == "TEXT" || typ == "ATTRIB" || typ == "MTEXT" {
        if let Some(st) = val(g, 7.0).filter(|s| !s.is_empty() && !is_standard(s)) {
            add.set("style", st);
        }
        let o51 = num0(g, 51.0);
        if typ != "MTEXT" && o51 != 0.0 && !o51.is_nan() {
            add.set("obl", rad(o51));
        }
        if typ == "MTEXT" {
            let raw: String = g.iter().filter(|(c, _)| *c == 3.0 || *c == 1.0).map(|(_, v)| *v).collect();
            if let Some(q) = q_number(&raw)
                && q != 0.0
                && !q.is_nan()
            {
                add.set("obl", rad(q));
            }
        }
    }
    if typ == "INSERT" {
        let sz = num(g, 43.0, 1.0);
        if sz != 0.0 && !sz.is_nan() && sz != 1.0 {
            add.set("sz", sz);
        }
        if let Some(xi) = g.iter().position(|(c, v)| *c == 102.0 && js::trim(v) == "{ACAD_XDICTIONARY")
            && let Some((c, v)) = g.get(xi.saturating_add(1))
            && *c == 360.0
        {
            add.set("_xd", jsv::upper(js::trim(v)));
        }
    }
    if typ == "HATCH" {
        let isl = int(g, 75.0, "0");
        if isl == 1.0 {
            add.set("island", "outer");
        } else if isl == 2.0 {
            add.set("island", "ignore");
        }
        if val(g, 71.0) == Some("1") {
            add.set("assoc", true);
        }
    }
    if typ == "DIMENSION" && one_type == "dim" {
        if let Some(ds) = val(g, 3.0).filter(|s| !s.is_empty() && !is_standard(s)) {
            add.set("dimstyle", ds);
        }
        let n = num0(g, 53.0);
        if n != 0.0 && !n.is_nan() {
            add.set("trot", norm_angle(rad(n)));
        }
    }
    if typ == "VIEWPORT" {
        let stt = jsv::u32_of(int(g, 90.0, "0"));
        if stt & 16384 != 0 {
            add.set("locked", true);
        }
        if stt & 131_072 != 0 || val(g, 68.0) == Some("0") {
            add.set("on", false);
        }
        let x = xdata(g);
        if app_list(&x, "CADCRAFT").unwrap_or(&[]).iter().any(|it| jsv::s(props::cv(it).1) == Some("CCVPLOCK")) {
            add.set("locked", true);
        }
    }
    let ox = jsv::or(add.get("ox"), 0.0);
    let oy = jsv::or(add.get("oy"), 0.0);
    let shift = jsv::truthy(add.get("ox")) || jsv::truthy(add.get("oy"));
    let mut fix = |e: &mut V| {
        let V::Obj(o) = e else { return };
        o.assign(add.clone());
        let et = jsv::to_str(o.get("type"));
        if !TH_TYPES.contains(&et.as_str()) {
            o.remove("th");
        }
        if typ == "LINE" {
            o.remove("z");
            let z1 = num0(g, 30.0);
            if z1 != 0.0 && !z1.is_nan() {
                o.set("z1", z1);
            }
            let z2 = num0(g, 31.0);
            if z2 != 0.0 && !z2.is_nan() {
                o.set("z2", z2);
            }
        }
        if et == "hatch"
            && shift
            && let Some(V::Arr(pl)) = o.get_mut("patlines")
        {
            for l in pl.iter_mut() {
                if let V::Obj(l) = l {
                    let x = jsv::to_num(l.get("x")) - ox;
                    let y = jsv::to_num(l.get("y")) - oy;
                    l.set("x", x);
                    l.set("y", y);
                }
            }
        }
    };
    match &mut made {
        V::Arr(a) => a.iter_mut().for_each(&mut fix),
        other => fix(other),
    }
    Ok(Some(made))
}

/// `/^\\Q(-?[\d.]+);/.exec(raw)` → `+q[1]`.
fn q_number(raw: &str) -> Option<f64> {
    let rest = raw.strip_prefix("\\Q")?;
    let (neg, body) = match rest.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, rest),
    };
    let n = body.bytes().take_while(|c| c.is_ascii_digit() || *c == b'.').count();
    if n == 0 || body.as_bytes().get(n) != Some(&b';') {
        return None;
    }
    Some(js::str_to_number(&format!("{}{}", if neg { "-" } else { "" }, body.get(..n).unwrap_or(""))))
}

fn mk(t: &str, base: &Obj) -> Obj {
    let mut o = Obj::new();
    o.set("type", t);
    o.assign(base.clone());
    o
}

#[allow(clippy::too_many_lines)]
fn entity_from0(typ: &str, g: &G, follow: Option<&[Follow]>, rep: &mut Obj, ctx: Option<&Ctx>) -> R<Option<V>> {
    let base = common(g);
    let f = num(g, 230.0, 1.0) < 0.0;
    let xf = |x: f64| if f { -x } else { x };
    let xd = if has(g, 1001.0) { xdata(g) } else { Obj::new() };
    let cc: Option<Vec<V>> = app_list(&xd, "CADCRAFT").map(values_of);
    let cat = ctx.map_or(Catalog::en(), |c| c.cat);
    Ok(match typ {
        "LINE" => {
            let mut o = mk("line", &base);
            o.set("x1", num0(g, 10.0));
            o.set("y1", num0(g, 20.0));
            o.set("x2", num0(g, 11.0));
            o.set("y2", num0(g, 21.0));
            Some(V::Obj(o))
        }
        "POINT" => {
            let mut o = mk("point", &base);
            o.set("x", num0(g, 10.0));
            o.set("y", num0(g, 20.0));
            if let Some(cc) = cc {
                o.set("cc", V::Arr(cc));
            }
            Some(V::Obj(o))
        }
        "XLINE" | "RAY" => {
            let (dx, dy) = (num0(g, 11.0), num0(g, 21.0));
            let n = js::hypot(dx, dy);
            if !(n > 1e-12) {
                skip(rep, typ);
                return Ok(None);
            }
            let mut o = mk(if typ == "XLINE" { "xline" } else { "ray" }, &base);
            o.set("x", num0(g, 10.0));
            o.set("y", num0(g, 20.0));
            o.set("dx", dx / n);
            o.set("dy", dy / n);
            Some(V::Obj(o))
        }
        "3DFACE" => {
            let mut o = mk("face3d", &base);
            let pts: Vec<V> = (0..4u8)
                .map(|k| {
                    let kf = f64::from(k);
                    V::Arr(vec![V::Num(num0(g, 10.0 + kf)), V::Num(num0(g, 20.0 + kf)), V::Num(num0(g, 30.0 + kf))])
                })
                .collect();
            o.set("pts", V::Arr(pts));
            Some(V::Obj(o))
        }
        "CIRCLE" => {
            let mut o = mk("circle", &base);
            o.set("cx", xf(num0(g, 10.0)));
            o.set("cy", num0(g, 20.0));
            o.set("r", num0(g, 40.0));
            Some(V::Obj(o))
        }
        "ARC" => {
            let (mut a0, mut a1) = (rad(num0(g, 50.0)), rad(num0(g, 51.0)));
            if f {
                (a0, a1) = (PI - a1, PI - a0);
            }
            let mut o = mk("arc", &base);
            o.set("cx", xf(num0(g, 10.0)));
            o.set("cy", num0(g, 20.0));
            o.set("r", num0(g, 40.0));
            o.set("a0", norm_angle(a0));
            o.set("a1", norm_angle(a1));
            Some(V::Obj(o))
        }
        "LWPOLYLINE" => {
            let mut pts: Vec<Obj> = Vec::new();
            let mut wide = false;
            for (c, v) in g {
                if *c == 10.0 {
                    pts.push(cc_io_dwg::obj! { "x" => xf(jsv::parse_float(v)), "y" => 0.0, "b" => 0.0 });
                } else if let Some(last) = pts.last_mut() {
                    if *c == 20.0 {
                        last.set("y", jsv::parse_float(v));
                    } else if *c == 42.0 {
                        last.set("b", (if f { -1.0 } else { 1.0 }) * jsv::parse_float(v));
                    } else if *c == 40.0 || *c == 41.0 {
                        let x = jsv::parse_float(v);
                        if x > 0.0 {
                            wide = true;
                        }
                        last.set(if *c == 40.0 { "sw" } else { "ew" }, if x > 0.0 { x } else { 0.0 });
                    }
                }
            }
            if !wide {
                for p in &mut pts {
                    p.remove("sw");
                    p.remove("ew");
                }
            }
            let cw = num0(g, 43.0);
            if pts.len() < 2 {
                return Ok(None);
            }
            let mut o = mk("poly", &base);
            o.set("pts", V::Arr(pts.into_iter().map(V::Obj).collect()));
            o.set("closed", i32f(int(g, 70.0, "0")) & 1 == 1);
            if cw > 0.0 && !wide {
                o.set("w", cw);
            }
            Some(V::Obj(o))
        }
        "POLYLINE" => {
            let flags = i32f(int(g, 70.0, "0"));
            if flags & (16 | 64) != 0 {
                skip(rep, "POLYLINE (mesh)");
                return Ok(None);
            }
            let (dsw, dew) = (num0(g, 40.0), num0(g, 41.0));
            let fl = follow.unwrap_or(&[]);
            let mut pts: Vec<(f64, f64, f64, f64, f64)> = fl
                .iter()
                .filter(|s| s.typ == "VERTEX" && i32f(int(s.g, 70.0, "0")) & 16 == 0)
                .map(|s| {
                    (
                        xf(num0(s.g, 10.0)),
                        num0(s.g, 20.0),
                        (if f { -1.0 } else { 1.0 }) * num0(s.g, 42.0),
                        num(s.g, 40.0, dsw),
                        num(s.g, 41.0, dew),
                    )
                })
                .collect();
            let wide = pts.iter().any(|p| p.3 > 0.0 || p.4 > 0.0);
            let first_sw = pts.first().map_or(f64::NAN, |p| p.3);
            let one = wide && pts.iter().all(|p| p.3 == first_sw && p.4 == first_sw);
            let keep_w = wide && !one;
            if pts.len() < 2 {
                return Ok(None);
            }
            let mut o = mk("poly", &base);
            o.set(
                "pts",
                V::Arr(
                    pts.drain(..)
                        .map(|p| {
                            let mut q = cc_io_dwg::obj! { "x" => p.0, "y" => p.1, "b" => p.2 };
                            if keep_w {
                                q.set("sw", p.3);
                                q.set("ew", p.4);
                            }
                            V::Obj(q)
                        })
                        .collect(),
                ),
            );
            o.set("closed", flags & 1 == 1);
            if one {
                let w =
                    if dsw != 0.0 && !dsw.is_nan() { dsw } else { num0(fl.iter().find(|s| s.typ == "VERTEX").map_or(&[], |s| s.g), 40.0) };
                o.set("w", w);
            }
            if let Some(cc) = cc {
                o.set("cc", V::Arr(cc));
            }
            Some(V::Obj(o))
        }
        "TEXT" | "ATTRIB" | "ATTDEF" => {
            let h72 = int(g, 72.0, "0");
            let v73 = int(g, if typ == "TEXT" { 73.0 } else { 74.0 }, "0");
            let aligned = h72 != 0.0 || v73 != 0.0;
            let ha = if h72 == 1.0 || h72 == 4.0 {
                "center"
            } else if h72 == 2.0 {
                "right"
            } else {
                "left"
            };
            let va = if h72 == 4.0 || v73 == 2.0 {
                "middle"
            } else if v73 == 3.0 {
                "top"
            } else if v73 == 1.0 {
                "bottom"
            } else {
                "baseline"
            };
            let second = aligned && h72 != 3.0 && h72 != 5.0;
            let s = decode_text(val(g, 1.0).unwrap_or(""));
            let mut t = mk("text", &base);
            t.set("x", xf(num0(g, if second { 11.0 } else { 10.0 })));
            t.set("y", num0(g, if second { 21.0 } else { 20.0 }));
            t.set("h", jsv::pn_or(Some(&V::Num(num(g, 40.0, 2.5))), 2.5));
            t.set("rot", norm_angle(rad(num0(g, 50.0))));
            t.set("wf", jsv::pn_or(Some(&V::Num(num(g, 41.0, 1.0))), 1.0));
            t.set("s", s.clone());
            t.set("ha", ha);
            t.set("va", va);
            if typ != "TEXT" {
                let fl = i32f(int(g, 70.0, "0"));
                let tag = val(g, 2.0).unwrap_or("");
                if tag.is_empty() {
                    return Ok(None);
                }
                if typ == "ATTDEF" {
                    let d = attrs::attdef(
                        &t,
                        AttdefOpts {
                            tag: V::from(tag),
                            prompt: decode_text(val(g, 3.0).unwrap_or("")),
                            def: s,
                            invisible: fl & 1 != 0,
                            constant: fl & 2 != 0,
                            preset: fl & 8 != 0,
                        },
                    );
                    return Ok(Some(V::Obj(cc_io_dwg::obj! { "type" => "attdef", "def" => d })));
                }
                t.set("tag", tag);
                if fl & 1 != 0 {
                    t.set("invisible", true);
                }
                return Ok(Some(V::Obj(t)));
            }
            if s.is_empty() {
                return Ok(None);
            }
            Some(V::Obj(t))
        }
        "MTEXT" => {
            let mut s = String::new();
            for (c, v) in g {
                if *c == 3.0 {
                    s.push_str(v);
                }
            }
            s.push_str(val(g, 1.0).unwrap_or(""));
            let s = mtext_plain(&s);
            if s.is_empty() {
                return Ok(None);
            }
            let att = jsv::pn_or(Some(&V::Num(int(g, 71.0, "1"))), 1.0);
            let mut rot = rad(num0(g, 50.0));
            if val(g, 11.0).is_some() {
                rot = num0(g, 21.0).jatan2(num0(g, 11.0));
            }
            let (ww, ls) = (num0(g, 41.0), num(g, 44.0, 1.0));
            let mut o = mk("text", &base);
            o.set("x", num0(g, 10.0));
            o.set("y", num0(g, 20.0));
            o.set("h", jsv::pn_or(Some(&V::Num(num(g, 40.0, 2.5))), 2.5));
            o.set("rot", norm_angle(rot));
            o.set("s", s);
            o.set_opt("ha", pick3((att - 1.0) % 3.0, ["left", "center", "right"]));
            o.set_opt("va", pick3(((att - 1.0) / 3.0).floor(), ["top", "middle", "bottom"]));
            if ww > 0.0 {
                o.set("ww", ww);
            }
            if ls > 0.0 && ls != 1.0 {
                o.set("ls", ls);
            }
            let mut m = Obj::new();
            m.set("backgroundFlags", int(g, 90.0, "0"));
            m.set("backgroundScale", num(g, 45.0, 1.5));
            m.set(
                "backgroundColor",
                match val(g, 421.0) {
                    Some(v) => V::Obj(cc_io_dwg::obj! { "rgb" => jsv::parse_int(v) }),
                    None => V::Obj(cc_io_dwg::obj! { "index" => int(g, 63.0, "7") }),
                },
            );
            mask_of(&m, &mut o);
            Some(V::Obj(o))
        }
        "ELLIPSE" => {
            let mut o = mk("ellipse", &base);
            o.set("cx", num0(g, 10.0));
            o.set("cy", num0(g, 20.0));
            o.set("mx", num0(g, 11.0));
            o.set("my", num0(g, 21.0));
            o.set("ratio", num(g, 40.0, 1.0));
            o.set("t0", num(g, 41.0, 0.0));
            o.set("t1", num(g, 42.0, TAU));
            Some(V::Obj(o))
        }
        "SPLINE" => {
            let pts = spline_points(g)?;
            if pts.len() < 2 {
                skip(rep, "SPLINE");
                return Ok(None);
            }
            inc(rep, "splines");
            let mut o = mk("poly", &base);
            o.set("pts", V::Arr(pts.iter().map(|p| V::Obj(cc_io_dwg::obj! { "x" => p.0, "y" => p.1, "b" => 0.0 })).collect()));
            o.set("closed", false);
            Some(V::Obj(o))
        }
        "SOLID" | "TRACE" => {
            let c: Vec<(f64, f64)> =
                [(10.0, 20.0), (11.0, 21.0), (13.0, 23.0), (12.0, 22.0)].iter().map(|(a, b)| (xf(num0(g, *a)), num0(g, *b))).collect();
            let mut flat: Vec<V> = c.iter().flat_map(|p| [V::Num(p.0), V::Num(p.1)]).collect();
            if let Some(p) = c.first() {
                flat.push(V::Num(p.0));
                flat.push(V::Num(p.1));
            }
            let mut o = mk("hatch", &base);
            o.set("solid", true);
            o.set("loops", V::Arr(vec![V::Arr(flat)]));
            Some(V::Obj(o))
        }
        "HATCH" => {
            let loops = hatch_loops(g, rep, cat);
            if loops.is_empty() {
                return Ok(None);
            }
            let mut h = mk("hatch", &base);
            let solid = val(g, 70.0) == Some("1");
            h.set("solid", solid);
            h.set("pattern", val(g, 2.0).unwrap_or(""));
            h.set("loops", V::Arr(loops.iter().map(|l| V::Arr(l.iter().map(|x| V::Num(*x)).collect())).collect()));
            if !solid {
                hatch_pattern(g, &mut h);
            }
            Some(V::Obj(h))
        }
        "INSERT" => {
            let Some(name) = val(g, 2.0).filter(|n| !n.is_empty()) else { return Ok(None) };
            let (mut rot, mut sx, mut x) = (rad(num0(g, 50.0)), num(g, 41.0, 1.0), num0(g, 10.0));
            if f {
                x = -x;
                rot = PI - rot;
                sx = -sx;
            }
            let rot = norm_angle(rot);
            let mut ins = mk("insert", &base);
            ins.set("block", name);
            ins.set("x", x);
            ins.set("y", num0(g, 20.0));
            ins.set("sx", sx);
            ins.set("sy", num(g, 42.0, 1.0));
            ins.set("rot", rot);
            let cols = js::max(1.0, int(g, 70.0, "1"));
            let rows = js::max(1.0, int(g, 71.0, "1"));
            let mut out: Vec<V> = Vec::new();
            if cols * rows > 1.0 {
                let (dc, dr) = (num0(g, 44.0), num0(g, 45.0));
                let m = m_mul(&m_translate(x, num0(g, 20.0)), &m_rotate(rot));
                let mut r = 0.0;
                let mut n = 0.0;
                'rows: while r < rows {
                    let mut c = 0.0;
                    while c < cols {
                        if n >= MAX_ARRAY {
                            break 'rows;
                        }
                        let p = m_apply(&m, P2::new(c * dc, r * dr));
                        let mut o = ins.clone();
                        o.set("x", p.x);
                        o.set("y", p.y);
                        out.push(V::Obj(o));
                        c += 1.0;
                        n += 1.0;
                    }
                    r += 1.0;
                }
            } else {
                out.push(V::Obj(ins));
            }
            let mut atts = Vec::new();
            for a in follow.unwrap_or(&[]) {
                if a.typ == "ATTRIB"
                    && let Some(t) = entity_from("ATTRIB", a.g, None, rep, None)?
                    && t.truthy()
                {
                    atts.push(t);
                }
            }
            if let Some(V::Obj(first)) = out.first_mut() {
                first.set("_attribs", V::Arr(atts));
            }
            Some(V::Arr(out))
        }
        "DIMENSION" => {
            if jsv::to_str(base.get("layer")).eq_ignore_ascii_case("*LayerNameForDynamicConstraint") {
                skip(rep, &cat.t("dxf.dimension_parametric_constraint", &[]));
                return Ok(None);
            }
            let block_name = val(g, 2.0);
            let kind = i32f(int(g, 70.0, "0")) & 7;
            let sname = jsv::upper(val(g, 3.0).filter(|s| !s.is_empty()).unwrap_or("STANDARD"));
            let mut st = match ctx {
                Some(c) => c.dim_styles.get(&sname).cloned().unwrap_or_else(|| c.dim_style.clone()),
                None => crate::props::dim_defaults(),
            };
            if let Some(ds) = xlist(app_list(&xd, "ACAD"), "DSTYLE") {
                let mut m: Vec<(f64, V)> = Vec::new();
                let mut k = 0usize;
                while k.saturating_add(1) < ds.len() {
                    if let (Some((c, v)), Some((_, next))) = (ds.get(k), ds.get(k.saturating_add(1)))
                        && *c == 1070.0
                    {
                        let key = js::round(jsv::to_num(Some(v)));
                        match m.iter_mut().find(|(kk, _)| *kk == key) {
                            Some(slot) => slot.1 = next.clone(),
                            None => m.push((key, next.clone())),
                        }
                    }
                    k = k.saturating_add(2);
                }
                props::st_from_codes(&|c| m.iter().find(|(k, _)| *k == jsv::i2f(c)).map(|(_, v)| v.clone()), &mut st);
            }
            if let Some(cc) = cc.as_ref() {
                if jsv::s(cc.first()) == Some("DIM") {
                    st.set("dec", jsv::to_num(cc.get(1)));
                    st.set("adec", jsv::to_num(cc.get(2)));
                    st.set("zin", jsv::to_num(cc.get(3)));
                    if cc.get(4).is_some() {
                        st.set("lunit", jsv::to_num(cc.get(4)));
                    }
                }
                let strs: Vec<String> = cc.iter().map(|x| jsv::to_str(Some(x))).collect();
                props::cc_dim_from(&strs, &mut st);
                if let Some(tr) = strs.iter().find(|x| x.starts_with("CCTROT:")) {
                    let n = js::str_to_number(crate::text::slice16(tr, 7, usize::MAX).as_str());
                    if n.is_finite() && n != 0.0 {
                        st.set("_trot", n);
                    }
                }
            }
            fix_style(&mut st);
            let text = crate::text::drop_measure_marker(mtext_plain(val(g, 1.0).unwrap_or("")));
            let mut d: Option<Obj> = None;
            if kind == 0 || kind == 1 {
                let mut o = Obj::new();
                o.set("kind", if kind == 1 { "aligned" } else { "linear" });
                o.set("p1", ptv(g, 13.0));
                o.set("p2", ptv(g, 14.0));
                o.set("loc", ptv(g, 10.0));
                o.set("rot", norm_angle(rad(num0(g, 50.0))));
                d = Some(o);
            } else if kind == 4 {
                let mut o = Obj::new();
                o.set("kind", "radius");
                o.set("c", ptv(g, 10.0));
                o.set("p1", ptv(g, 15.0));
                o.set("loc", ptv(g, 11.0));
                d = Some(o);
            } else if kind == 3 {
                let (a, b) = ((num0(g, 15.0), num0(g, 25.0)), (num0(g, 10.0), num0(g, 20.0)));
                let mut o = Obj::new();
                o.set("kind", "diameter");
                o.set("c", jsv::p2((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0));
                o.set("p1", jsv::p2(a.0, a.1));
                o.set("loc", ptv(g, 11.0));
                d = Some(o);
            } else if kind == 5 {
                let mut o = Obj::new();
                o.set("kind", "angular");
                o.set("c", ptv(g, 15.0));
                o.set("p1", ptv(g, 13.0));
                o.set("p2", ptv(g, 14.0));
                o.set("loc", ptv(g, 10.0));
                d = Some(o);
            } else if kind == 2 {
                let l = |a: f64, b: f64| [num0(g, a), num0(g, a + 10.0), num0(g, b), num0(g, b + 10.0)];
                let loc = P2::new(num0(g, 16.0), num0(g, 26.0));
                if let Some((c, p1, p2)) = cc_cad_geom::dims::angular_from_lines(l(13.0, 14.0), l(15.0, 10.0), loc) {
                    let mut o = Obj::new();
                    o.set("kind", "angular");
                    o.set("c", jsv::p2(c.x, c.y));
                    o.set("p1", jsv::p2(p1.x, p1.y));
                    o.set("p2", jsv::p2(p2.x, p2.y));
                    o.set("loc", ptv(g, 16.0));
                    d = Some(o);
                }
            }
            let trot = st.remove("_trot");
            if let Some(d) = d.filter(dim_usable) {
                let mut o = mk("dim", &base);
                o.assign(d);
                o.set("text", text);
                o.set("st", st);
                if let Some(b) = block_name.filter(|b| !b.is_empty()) {
                    o.set("block", b);
                }
                if let Some(t) = trot.filter(V::truthy) {
                    o.set("trot", t);
                }
                return Ok(Some(V::Obj(o)));
            }
            let Some(b) = block_name.filter(|b| !b.is_empty()) else {
                skip(rep, "DIMENSION");
                return Ok(None);
            };
            let mut o = mk("insert", &base);
            o.set("block", b);
            o.set("x", 0.0);
            o.set("y", 0.0);
            o.set("sx", 1.0);
            o.set("sy", 1.0);
            o.set("rot", 0.0);
            o.set("dim", true);
            Some(V::Obj(o))
        }
        "VIEWPORT" => {
            let mut v = mk("viewport", &base);
            let (x, y, w, h) = (num0(g, 10.0), num0(g, 20.0), num0(g, 40.0), num0(g, 41.0));
            v.set("x", x);
            v.set("y", y);
            v.set("w", w);
            v.set("h", h);
            v.set("id", int(g, 69.0, "0"));
            let mut vc: Option<(f64, f64)> = None;
            let mut vh = 0.0;
            let mv = xlist(app_list(&xd, "ACAD"), "MVIEW");
            if val(g, 45.0).is_some() {
                vh = num0(g, 45.0);
                vc = Some((num0(g, 12.0) + num0(g, 17.0), num0(g, 22.0) + num0(g, 27.0)));
            } else if let Some(mv) = mv.as_ref() {
                let reals: Vec<f64> = mv.iter().filter(|(c, _)| *c == 1040.0).map(|(_, x)| jsv::to_num(Some(x))).collect();
                let tgt = mv.iter().find(|(c, _)| *c == 1010.0);
                let tgy = mv.iter().find(|(c, _)| *c == 1020.0);
                vh = reals.get(1).copied().unwrap_or(f64::NAN);
                vc = Some((
                    reals.get(2).copied().unwrap_or(f64::NAN) + tgt.map_or(0.0, |t| jsv::to_num(Some(&t.1))),
                    reals.get(3).copied().unwrap_or(f64::NAN) + tgy.map_or(0.0, |t| jsv::to_num(Some(&t.1))),
                ));
                let mut frz: Vec<V> = Vec::new();
                for (c, n) in mv.iter() {
                    if *c == 1003.0 && n.truthy() && !frz.iter().any(|x| crate::props::strict_eq(Some(x), Some(n))) {
                        frz.push(n.clone());
                    }
                }
                if !frz.is_empty() {
                    v.set("frz", V::Arr(frz));
                }
            }
            let fh: Vec<V> =
                g.iter().filter(|(c, _)| *c == 331.0).map(|(_, x)| js::trim(x)).filter(|x| !x.is_empty()).map(V::from).collect();
            if !fh.is_empty() {
                v.set("_frzH", V::Arr(fh));
            }
            match vc {
                Some(c) if vh > 0.0 && h > 0.0 => {
                    v.set("cx", c.0);
                    v.set("cy", c.1);
                    v.set("s", h / vh);
                }
                _ => {
                    v.set("cx", x);
                    v.set("cy", y);
                    v.set("s", 1.0);
                }
            }
            let tw = if val(g, 51.0).is_some() {
                rad(num0(g, 51.0))
            } else {
                mv.as_ref().and_then(|mv| mv.iter().find(|(c, _)| *c == 1040.0).map(|(_, x)| jsv::to_num(Some(x)))).unwrap_or(f64::NAN)
            };
            if tw.is_finite() && tw.abs() > 1e-12 {
                v.set("tw", tw);
            }
            if let Some(cc) = cc.as_ref()
                && jsv::s(cc.first()) == Some("LAYOUT")
            {
                v.set("overall", true);
                let ci = |i: usize| cc.get(i);
                let mut o = Obj::new();
                o.set("name", decode_text(&jsv::to_str(Some(&jsv::orv(ci(1), V::from(""))))));
                o.set("unit", if jsv::s(ci(2)) == Some("in") { "in" } else { "mm" });
                o.set_opt("key", ci(3).cloned());
                o.set("x0", jsv::to_num(ci(4)));
                o.set("y0", jsv::to_num(ci(5)));
                o.set("w", jsv::to_num(ci(6)));
                o.set("h", jsv::to_num(ci(7)));
                o.set("noplot", V::Arr(if jsv::s(ci(8)) == Some("NOPLOT") { cc.iter().skip(9).cloned().collect() } else { Vec::new() }));
                v.set("cc", o);
            }
            Some(V::Obj(v))
        }
        _ => {
            skip(rep, typ);
            None
        }
    })
}

/// `['a', 'b', 'c'][i]` (undefined unless i is 0, 1 or 2).
fn pick3(i: f64, names: [&str; 3]) -> Option<V> {
    if i == 0.0 {
        Some(V::from(names[0]))
    } else if i == 1.0 {
        Some(V::from(names[1]))
    } else if i == 2.0 {
        Some(V::from(names[2]))
    } else {
        None
    }
}

/// `/\n\s*0\r?\n(MULTILEADER|IMAGE|WIPEOUT)\r?\n/.test(text)`
fn has_extras(text: &str) -> bool {
    let b = text.as_bytes();
    for (p, _) in text.match_indices('\n') {
        let rest = text.get(p.saturating_add(1)..).unwrap_or("");
        let t = rest.trim_start_matches(js::is_ws);
        let Some(t) = t.strip_prefix('0') else { continue };
        let t = t.strip_prefix('\r').unwrap_or(t);
        let Some(t) = t.strip_prefix('\n') else { continue };
        for w in EXTRA_TYPES {
            if let Some(after) = t.strip_prefix(w) {
                let after = after.strip_prefix('\r').unwrap_or(after);
                if after.starts_with('\n') {
                    return true;
                }
            }
        }
    }
    let _ = b;
    false
}

/// `readDxf(text)` → the doc.load input as JSON, or the error the JS throws.
pub fn read_dxf(text: &str) -> Result<serde_json::Value, JsErr> {
    Ok(jsv::into_json(V::Obj(read_dxf_v(text, Catalog::en())?)))
}

/// `readDxf(text)` on dynamic values.
pub fn read_dxf_v(text: &str, cat: &Catalog) -> R<Obj> {
    let bin = cat.t("dxf.autocad_binary_dxf", &[]);
    if text.starts_with(&bin) {
        return Err(JsErr(cat.t("dxf.binary_dxf_save_ascii", &[])));
    }
    // text.split(/\r?\n/)
    let mut raw: Vec<&str> = text.split('\n').collect();
    let last = raw.len().saturating_sub(1);
    for (i, l) in raw.iter_mut().enumerate() {
        if i < last {
            *l = l.strip_suffix('\r').unwrap_or(l);
        }
    }
    let mut pairs: Vec<Pair> = Vec::with_capacity(raw.len() / 2);
    let mut i = 0usize;
    while i.saturating_add(1) < raw.len() {
        let code = jsv::parse_int(raw.get(i).copied().unwrap_or(""));
        if code.is_nan() {
            return Err(JsErr(cat.t("dxf.read.bad_group_code", &[("line", i.saturating_add(1).to_string())])));
        }
        pairs.push((code, raw.get(i.saturating_add(1)).copied().unwrap_or("")));
        i = i.saturating_add(2);
    }
    let mut dim_style = props::dim_defaults();
    let _ = &mut dim_style;
    let mut r = Reader {
        pairs: &pairs,
        i: 0,
        cat,
        entities: Vec::new(),
        layers: Vec::new(),
        blocks: Vec::new(),
        layouts: Vec::new(),
        dim_style,
        meta: cc_io_dwg::obj! { "units" => "", "version" => "" },
        report: new_report(),
        extra: Obj::new(),
        vports_raw: None,
        views: None,
        header: Vec::new(),
        dim_styles: HashMap::new(),
        block_records: HashMap::new(),
        style_handles: HashMap::new(),
        layout_objs: Vec::new(),
        paper_blocks: Vec::new(),
        paper_raw: Vec::new(),
        no_plot: HashSet::new(),
        layer_by_h: HashMap::new(),
        dict_by_h: HashMap::new(),
        filter_by_h: HashMap::new(),
    };
    while r.more() {
        let (c, v) = r.at();
        if c == 0.0 && js::trim(v) == "SECTION" {
            let name = r.pairs.get(r.i.saturating_add(1)).map_or("", |p| js::trim(p.1)).to_string();
            r.i = r.i.saturating_add(2);
            match name.as_str() {
                "HEADER" => r.read_header(),
                "TABLES" => r.read_tables(),
                "BLOCKS" => r.read_blocks()?,
                "ENTITIES" => {
                    let mut rep = std::mem::take(&mut r.report);
                    let res = r.read_entity_list(&mut rep, true, false);
                    r.report = rep;
                    let (model, paper) = res?;
                    r.entities.extend(model);
                    r.paper_raw = paper;
                }
                "OBJECTS" => r.read_objects(),
                _ => r.skip_section(),
            }
        } else if c == 0.0 && js::trim(v) == "EOF" {
            break;
        } else {
            r.step();
        }
    }
    {
        // every $DIM… header variable CadCraft has a key for, on top of the ones read above
        let mut vars = Obj::new();
        for (k, v) in &r.header {
            if k.starts_with("$DIM")
                && let Some(first) = first_value(v)
            {
                vars.set(k.get(1..).unwrap_or("").to_string(), first.as_str());
            }
        }
        props::st_from_vars(Some(&V::Obj(vars)), &mut r.dim_style);
        fix_style(&mut r.dim_style);
    }
    if let Some(ib) = r.header_get("$INSBASE") {
        let n = |c: f64| jsv::pn_or(ib.iter().find(|(k, _)| *k == c).map(|(_, s)| V::from(s.as_str())).as_ref(), 0.0);
        let p = jsv::p2(n(10.0), n(20.0));
        r.meta.set("insbase", p);
    }
    if r.meta.get("lunits").is_some_and(V::truthy)
        && let Some(lp) = r.header_get("$LUPREC")
    {
        let v = lp.iter().find(|(k, _)| *k == 70.0).map_or(f64::NAN, |(_, s)| js::str_to_number(s));
        if (0.0..=8.0).contains(&v) {
            r.meta.set("luprec", v);
        }
    }
    // attributes: values and positions relative to their block, now that every block is known
    let mut info: BlockInfo = HashMap::new();
    for b in &r.blocks {
        if let Some(bo) = jsv::obj(Some(b)) {
            info.insert(jsv::to_str(bo.get("name")), attrs::block_info(bo));
        }
    }
    {
        let Reader { entities, paper_raw, paper_blocks, blocks, layouts, dict_by_h, filter_by_h, .. } = &mut r;
        let mut lists: Vec<&mut Vec<V>> = vec![entities, paper_raw];
        for (_, l) in paper_blocks.iter_mut() {
            lists.push(l);
        }
        for b in blocks.iter_mut() {
            if let V::Obj(bo) = b
                && let Some(V::Arr(l)) = bo.get_mut("entities")
            {
                lists.push(l);
            }
        }
        for l in layouts.iter_mut() {
            if let V::Obj(lo) = l
                && let Some(V::Arr(x)) = lo.get_mut("entities")
            {
                lists.push(x);
            }
        }
        attrs::resolve_attribs(&mut lists, &info);
        // XCLIP (WL.16): each reference that names an extension dictionary → ACAD_FILTER → SPATIAL gets its clip
        let dict = &*dict_by_h;
        let filt = &*filter_by_h;
        for list in lists.iter_mut() {
            for e in list.iter_mut() {
                let V::Obj(o) = e else { continue };
                let Some(xdv) = o.get("_xd").filter(|x| x.truthy()).cloned() else { continue };
                let xk = jsv::to_str(Some(&xdv));
                let af = dict.get(&xk).and_then(|d| d.get("ACAD_FILTER")).cloned().unwrap_or_default();
                let sp = dict.get(&af).and_then(|d| d.get("SPATIAL")).cloned().unwrap_or_default();
                let f = filt.get(&sp);
                o.remove("_xd");
                if let Some(c) = f.and_then(|f| small::clip_from_filter(Some(f))) {
                    o.set("clip", c);
                }
            }
        }
    }
    build_layouts(&mut r);
    let mut out = Obj::new();
    let vports = r.vports_raw.take();
    if let Some(vr) = vports.as_ref() {
        let v = crate::vports::vports_from(vr);
        let tw = vr.first().and_then(|x| jsv::fin(jsv::get(Some(x), "tw")));
        match v {
            Some(v) => r.extra.set("vports", v),
            None => {
                if let Some(tw) = tw.filter(|t| t.abs() > 1e-12) {
                    r.extra.set("viewTwist", tw);
                }
            }
        }
    }
    if let (Some(org), Some(xdir)) = (r.header_get("$UCSORG").cloned(), r.header_get("$UCSXDIR").cloned()) {
        let n =
            |v: &Vec<(f64, String)>, c: f64| jsv::pn_or(v.iter().find(|(k, _)| *k == c).map(|(_, s)| V::from(s.as_str())).as_ref(), 0.0);
        let (x, y, ox, oy) = (n(&xdir, 10.0), n(&xdir, 20.0), n(&org, 10.0), n(&org, 20.0));
        if (x != 0.0 || y != 0.0) && (ox != 0.0 || oy != 0.0 || y.jatan2(x).abs() > 1e-12) {
            let cur = V::Obj(cc_io_dwg::obj! { "x" => ox, "y" => oy, "ang" => y.jatan2(x) });
            ucs_mut(&mut r.extra, |o| o.set("current", cur));
        }
    }
    out.set("entities", V::Arr(std::mem::take(&mut r.entities)));
    out.set("layers", V::Arr(std::mem::take(&mut r.layers)));
    out.set("blocks", V::Arr(std::mem::take(&mut r.blocks)));
    out.set("layouts", V::Arr(std::mem::take(&mut r.layouts)));
    out.set("dimStyle", std::mem::take(&mut r.dim_style));
    out.set("meta", std::mem::take(&mut r.meta));
    out.set("report", std::mem::take(&mut r.report));
    let extra = std::mem::take(&mut r.extra);
    for (k, v) in extra.0 {
        if matches!(v, V::Null) && k == "views" {
            continue;
        }
        out.0.push((k, v));
    }
    if has_extras(text) {
        merge_extras(&mut out, text, cat);
    }
    if let Some(V::Obj(rep)) = out.get_mut("report")
        && rep.get("xrefs").is_some_and(V::truthy)
    {
        let n = jsv::to_num(rep.get("xrefs"));
        let s = cat.t("dwg.read.xrefs", &i18n::n(n));
        if let Some(V::Arr(notes)) = rep.get_mut("notes") {
            notes.push(V::Str(s));
        }
    }
    crate::model3d::split3d(&mut out, cat);
    Ok(out)
}

/// `buildLayouts()`: from LAYOUT objects (R2000+), else the single paper space of an R12 file.
fn build_layouts(r: &mut Reader) {
    let units = jsv::to_str(r.meta.get("units"));
    let measurement_in = r.header_code("$MEASUREMENT", 70.0) == Some("0");
    let guess_unit = || -> &'static str {
        if !units.is_empty() {
            if ["inches", "feet", "yards", "miles", "mils", "microinches"].contains(&units.as_str()) { "in" } else { "mm" }
        } else if measurement_in {
            "in"
        } else {
            "mm"
        }
    };
    let mut no_plot = std::mem::take(&mut r.no_plot);
    let layer_by_h = &r.layer_by_h;
    let mut from_list = |name: &str, list: &[V], lim: Option<((f64, f64), (f64, f64))>, unit: &str| -> Obj {
        let (mut entities, overall) = split_viewports(list);
        for e in entities.iter_mut() {
            let V::Obj(o) = e else { continue };
            if jsv::s(o.get("type")) != Some("viewport") || !o.get("_frzH").is_some_and(V::truthy) {
                continue;
            }
            let mut frz: Vec<V> = Vec::new();
            let mut add = |v: V| {
                if !frz.iter().any(|x| crate::props::strict_eq(Some(x), Some(&v))) {
                    frz.push(v);
                }
            };
            for x in jsv::arr(o.get("frz")) {
                add(x.clone());
            }
            for h in jsv::arr(o.get("_frzH")) {
                if let Some(n) = layer_by_h.get(&jsv::upper(&jsv::to_str(Some(h)))).filter(|n| !n.is_empty()) {
                    add(V::Str(n.clone()));
                }
            }
            o.remove("_frzH");
            if frz.is_empty() {
                o.remove("frz");
            } else {
                o.set("frz", V::Arr(frz));
            }
        }
        let cc = overall.as_ref().and_then(|o| jsv::get(Some(o), "cc")).filter(|c| c.truthy()).cloned();
        for n in jsv::arr(jsv::get(cc.as_ref(), "noplot")) {
            no_plot.insert(jsv::upper(&jsv::to_str(Some(n))));
        }
        let mut paper = Obj::new();
        if let Some(cc) = cc.as_ref() {
            let g = |k: &str| jsv::get(Some(cc), k).cloned();
            paper.set_opt("w", g("w"));
            paper.set_opt("h", g("h"));
            paper.set_opt("x0", g("x0"));
            paper.set_opt("y0", g("y0"));
            paper.set_opt("unit", g("unit"));
            paper.set_opt("key", g("key").filter(V::truthy));
        } else if let Some((mn, mx)) = lim {
            paper.set("w", mx.0 - mn.0);
            paper.set("h", mx.1 - mn.1);
            paper.set("x0", mn.0);
            paper.set("y0", mn.1);
            paper.set("unit", unit);
        } else {
            paper.set("w", 420.0);
            paper.set("h", 297.0);
            paper.set("x0", 0.0);
            paper.set("y0", 0.0);
            paper.set("unit", "mm");
        }
        if !(jsv::to_num(paper.get("w")) > 0.0 && jsv::to_num(paper.get("h")) > 0.0) {
            match overall.as_ref() {
                Some(o) => {
                    let n = |k: &str| jsv::to_num(jsv::get(Some(o), k));
                    paper.set_opt("w", jsv::get(Some(o), "w").cloned());
                    paper.set_opt("h", jsv::get(Some(o), "h").cloned());
                    paper.set("x0", n("x") - n("w") / 2.0);
                    paper.set("y0", n("y") - n("h") / 2.0);
                }
                None => {
                    paper.set("w", 420.0);
                    paper.set("h", 297.0);
                    paper.set("x0", 0.0);
                    paper.set("y0", 0.0);
                    paper.set("unit", "mm");
                }
            }
        }
        if !paper.get("key").is_some_and(V::truthy)
            && let Some(std) = small::match_paper(jsv::to_num(paper.get("w")), jsv::to_num(paper.get("h")), paper.get("unit"))
        {
            paper.set("key", std);
        }
        let mut l = Obj::new();
        let cname = jsv::get(cc.as_ref(), "name").filter(|n| n.truthy()).cloned();
        l.set("name", cname.unwrap_or_else(|| V::from(name)));
        l.set("paper", paper);
        l.set("entities", V::Arr(entities));
        l
    };
    let own = std::mem::take(&mut r.layouts);
    if !r.layout_objs.is_empty() {
        let mut paper_layouts: Vec<&LayoutObj> = r.layout_objs.iter().filter(|l| !l.name.eq_ignore_ascii_case("model")).collect();
        paper_layouts.sort_by(|a, b| {
            let d = a.tab - b.tab;
            if d < 0.0 {
                std::cmp::Ordering::Less
            } else if d > 0.0 {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        for l in paper_layouts {
            let rec = r.block_records.get(&l.record).cloned().unwrap_or_default();
            let empty: Vec<V> = Vec::new();
            let list: &[V] = if rec.eq_ignore_ascii_case("*paper_space") {
                &r.paper_raw
            } else {
                let up = jsv::upper(&rec);
                r.paper_blocks.iter().find(|(n, _)| *n == up).map_or(&empty, |(_, l)| l)
            };
            let unit = if l.units == 0.0 {
                "in"
            } else if l.units == 1.0 {
                "mm"
            } else {
                guess_unit()
            };
            let mut lim = (l.max.0 > l.min.0 && l.max.1 > l.min.1).then_some((l.min, l.max));
            if lim.is_none() && l.paper_mm.0 > 0.0 {
                let k = if unit == "in" { 25.4 } else { 1.0 };
                let odd = {
                    let m = l.rot % 2.0;
                    m != 0.0 && !m.is_nan()
                };
                let (w, h) = if odd { (l.paper_mm.1, l.paper_mm.0) } else { (l.paper_mm.0, l.paper_mm.1) };
                lim = Some(((0.0, 0.0), (w / k, h / k)));
            }
            let lay = from_list(&l.name, list, lim, unit);
            r.layouts.push(V::Obj(lay));
        }
    } else if !r.paper_raw.is_empty() {
        let mn = r.header_get("$PLIMMIN").cloned();
        let mx = r.header_get("$PLIMMAX").cloned();
        let lim = match (mn, mx) {
            (Some(mn), Some(mx)) => {
                let n = |v: &Vec<(f64, String)>, c: f64| match v.iter().find(|(k, _)| *k == c) {
                    Some((_, s)) => js::str_to_number(s),
                    None => f64::NAN,
                };
                let l = ((n(&mn, 10.0), n(&mn, 20.0)), (n(&mx, 10.0), n(&mx, 20.0)));
                (l.1.0 > l.0.0).then_some(l)
            }
            _ => None,
        };
        let raw = r.paper_raw.clone();
        let lay = from_list("Layout1", &raw, lim, guess_unit());
        r.layouts.push(V::Obj(lay));
    }
    r.layouts.extend(own);
    if !r.layouts.is_empty() {
        r.report.set("layouts", jsv::us2f(r.layouts.len()));
    }
    if let Some(views) = r.views.take() {
        let first = r.layouts.first().and_then(|l| jsv::get(Some(l), "name")).cloned();
        let has_layouts = !r.layouts.is_empty();
        let list: Vec<V> = views
            .into_iter()
            .filter(|(_, paper)| !*paper || has_layouts)
            .map(|(mut v, paper)| {
                if paper {
                    v.set_opt("layout", first.clone());
                }
                V::Obj(v)
            })
            .collect();
        r.extra.set("views", V::Arr(list));
    }
    for l in r.layers.iter_mut() {
        if let V::Obj(o) = l {
            let up = jsv::upper(&jsv::to_str(o.get("name")));
            if up == "DEFPOINTS" || no_plot.contains(&up) {
                o.set("plot", false);
            }
        }
    }
    r.no_plot = no_plot;
}

/// `mergeExtras()` (0.4 C6): multileaders, pictures and wipeouts through the drawing model the DWG import uses.
fn merge_extras(out: &mut Obj, text: &str, cat: &Catalog) {
    let x = match cc_io_dwg::dxf_model::try_read_dxf_model(text) {
        Ok(m) => crate::dwg_import::extras_from(&m, cat).map_err(|e| e.0),
        Err(e) => Err(e),
    };
    let x = match x {
        Ok(x) => x,
        Err(msg) => {
            if let Some(V::Obj(rep)) = out.get_mut("report")
                && let Some(V::Arr(notes)) = rep.get_mut("notes")
            {
                notes.push(V::Str(msg));
            }
            return;
        }
    };
    if let Some(V::Obj(rep)) = out.get_mut("report")
        && let Some(V::Obj(sk)) = rep.get_mut("skipped")
    {
        for t in EXTRA_TYPES {
            sk.remove(t);
        }
    }
    let names: HashSet<String> = jsv::arr(out.get("blocks")).iter().map(|b| jsv::upper(&jsv::to_str(jsv::get(Some(b), "name")))).collect();
    if let Some(V::Arr(blocks)) = out.get_mut("blocks") {
        for b in x.anon {
            if !names.contains(&jsv::upper(&jsv::to_str(jsv::get(Some(&b), "name")))) {
                blocks.push(b);
            }
        }
    }
    if let Some(V::Arr(ents)) = out.get_mut("entities") {
        ents.extend(x.model);
    }
    if let Some(V::Arr(blocks)) = out.get_mut("blocks") {
        for (name, list) in x.blocks {
            let up = jsv::upper(&jsv::to_str(Some(&name)));
            if let Some(V::Obj(b)) = blocks.iter_mut().find(|b| jsv::upper(&jsv::to_str(jsv::get(Some(b), "name"))) == up)
                && let Some(V::Arr(e)) = b.get_mut("entities")
            {
                e.extend(list);
            }
        }
    }
    if let Some(V::Arr(layouts)) = out.get_mut("layouts") {
        for (name, list) in x.layouts {
            if let Some(V::Obj(l)) = layouts.iter_mut().find(|l| crate::props::strict_eq(jsv::get(Some(l), "name"), Some(&name)))
                && let Some(V::Arr(e)) = l.get_mut("entities")
            {
                e.extend(list);
            }
        }
    }
    if let Some(V::Obj(rep)) = out.get_mut("report")
        && let Some(V::Arr(notes)) = rep.get_mut("notes")
    {
        notes.extend(x.notes);
    }
}
