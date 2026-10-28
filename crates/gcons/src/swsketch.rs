//! swsketch.js forward (`docToSwb`): a CadCraft drawing as a SolidWorks VBA macro (.swb) that rebuilds the model
//! space as one sketch on the Front Plane — lines, circles, arcs, points, polylines (each segment a line or an arc,
//! joined by coincident relations), the geometric constraints as sketch relations, the dimensional ones as named
//! dimensions, the parameters as global variables and every expression as an equation. Coordinates in metres.
//!
//! The text is the JS's byte for byte (kdiff checks SWBOUT's download); numbers go through the JS's own formatting
//! (`toFixed`, `toPrecision`, `toExponential`), angles through V8's fdlibm `sin` / `cos` and msun `atan`. The macro
//! has not been run in SolidWorks (0.5 says so in the file and on the command line).

use cc_cad::Doc;
use cc_cad::entity::Geo;
use cc_cad::i18n::Catalog;
use cc_cad::js;
use cc_cad_geom::jsmath::JsMath;
use cc_part::expr::{self, Ast};
use cc_part::params;

use crate::con::Con;
use crate::gcons::{dim_entity_for, is_dim, model_get, params_of};
use crate::gsolve::{ref_kind, seg_count};

/// `out`: what the macro makes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub lines: usize,
    pub arcs: usize,
    pub circles: usize,
    pub points: usize,
    pub relations: usize,
    pub dimensions: usize,
    pub globals: usize,
    pub equations: usize,
}

/// `{ ok: true, text, bytes, notes, summary, counts }`.
#[derive(Clone, Debug)]
pub struct Swb {
    pub text: String,
    /// Windows-1252 as the JS writes it: a character over 255 becomes `?`
    pub bytes: Vec<u8>,
    pub notes: Vec<String>,
    pub summary: String,
    pub counts: Counts,
}

/// `{ ok: false, error, notes }`.
#[derive(Clone, Debug)]
pub struct SwbError {
    pub error: String,
    pub notes: Vec<String>,
}

/// UNIT_MM: drawing units → millimetres.
fn unit_mm(u: &str) -> Option<f64> {
    Some(match u {
        "" | "millimeters" => 1.0,
        "centimeters" => 10.0,
        "meters" => 1000.0,
        "inches" => 25.4,
        "feet" => 304.8,
        "yards" => 914.4,
        "mils" => 0.0254,
        "microinches" => 0.000_025_4,
        _ => return None,
    })
}
/// SW_UNIT: swLengthUnit_e (swMM 0, swCM 1, swMETER 2, swINCHES 3, swFEET 4).
fn sw_unit(u: &str) -> Option<u8> {
    Some(match u {
        "" | "millimeters" => 0,
        "centimeters" => 1,
        "meters" => 2,
        "inches" => 3,
        "feet" => 4,
        _ => return None,
    })
}
const SW_UNITS_LINEAR: u8 = 41;
const SW_DEFAULT_TEMPLATE_PART: u8 = 8;
const SW_INPUT_DIM_VAL_ON_CREATE: u8 = 10;
const SW_DETAILING_NO_OPTION_SPECIFIED: u8 = 0;

fn rel_name(t: &str) -> Option<&'static str> {
    Some(match t {
        "coincident" => "sgCOINCIDENT",
        "parallel" => "sgPARALLEL",
        "perpendicular" => "sgPERPENDICULAR",
        "tangent" => "sgTANGENT",
        "concentric" => "sgCONCENTRIC",
        "collinear" => "sgCOLINEAR",
        "equal" => "sgSAMELENGTH",
        "symmetric" => "sgSYMMETRIC",
        "fix" => "sgFIXED",
        _ => return None,
    })
}
fn sw_func(n: &str) -> Option<&'static str> {
    Some(match n {
        "sin" => "sin",
        "cos" => "cos",
        "tan" => "tan",
        "atan" => "atn",
        "sqrt" => "sqr",
        "abs" => "abs",
        "sign" => "sgn",
        "exp" => "exp",
        "int" => "int",
        _ => return None,
    })
}

/// `num(v)`: `Number(v.toPrecision(15)).toString()`, or `v.toExponential(14)` with a capital E when that has an exponent.
pub fn num(v: f64) -> String {
    // a number whose shortest form has at most 15 digits is its own toPrecision(15): no rounding to do
    let r = if v.is_finite() && shortest(v).0 <= 15 { v } else { cc_part::js::prec_num(v, 15) };
    let s = cc_part::js::num_str(r);
    if s.contains(['e', 'E']) { js::to_exponential(v, 14).replace('e', "E") } else { s }
}

/// The shortest round-trip form of a finite v: (significant digits, digits after the point).
fn shortest(v: f64) -> (usize, i64) {
    let e = format!("{:e}", v.abs());
    let (m, x) = e.split_once('e').unwrap_or((e.as_str(), "0"));
    let digits = m.bytes().filter(u8::is_ascii_digit).count();
    let exp: i64 = x.parse().unwrap_or(0);
    let d = i64::try_from(digits).unwrap_or(i64::MAX);
    (digits, d.saturating_sub(1).saturating_sub(exp).max(0))
}

/// `Number(v.toFixed(f))` for f ≤ 12: the integer n nearest v · 10^f (halves away from zero, as toFixed rounds |v|),
/// worked out exactly from v's binary form, then n · 10^−f read back correctly rounded. Outside |v| < 10^21 (toFixed's
/// exponent form), the text way.
fn fixed_num(v: f64, f: usize, _lim: f64) -> f64 {
    if v == 0.0 {
        return 0.0; // "0", −0 included
    }
    if !(v.is_finite() && v.abs() < 1e21) || f > 12 {
        return js::str_to_number(&js::to_fixed(v, f));
    }
    let bits = v.abs().to_bits();
    let exp = i64::try_from((bits >> 52) & 0x7ff).unwrap_or(0);
    let frac = bits & ((1u64 << 52) - 1);
    // |v| = m · 2^e
    let (m, e) = if exp == 0 { (frac, -1074i64) } else { (frac | (1u64 << 52), exp.saturating_sub(1075)) };
    let p10 = 10u128.pow(u32::try_from(f).unwrap_or(0));
    let n: u128 = if e >= 0 {
        // an integer below 10^21: times 10^20 at most stays under 2^128
        (u128::from(m) << u32::try_from(e).unwrap_or(0)).saturating_mul(p10)
    } else {
        let sh = u32::try_from(-e).unwrap_or(u32::MAX);
        let num = u128::from(m).saturating_mul(p10);
        if sh >= 128 {
            0
        } else {
            let q = num >> sh;
            let rem = num - (q << sh);
            // half or more rounds up (|v|'s halves go away from zero)
            if sh > 0 && rem >= (1u128 << (sh - 1)) { q + 1 } else { q }
        }
    };
    let r: f64 = format!("{n}e-{f}").parse().unwrap_or(f64::NAN);
    if v < 0.0 { -r } else { r }
}

/// `safe(s)`: `/[^\w.-]+/g` → `_`, at most 80 characters, else "Drawing1".
pub fn safe(s: &str) -> String {
    let r = underscore_runs(s);
    let r: String = r.chars().take(80).collect();
    if r.is_empty() { "Drawing1".into() } else { r }
}
/// `s.replace(/[^\w.-]+/g, '_')` (SWBOUT's file name too).
pub fn underscore_runs(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut run = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-' {
            out.push(c);
            run = false;
        } else if !run {
            out.push('_');
            run = true;
        }
    }
    out
}

/// `bulgeCentre(p, q, b)`.
fn bulge_centre(p: (f64, f64), q: (f64, f64), b: f64) -> (f64, f64) {
    let th = 4.0 * b.jatan();
    let c = js::hypot(q.0 - p.0, q.1 - p.1);
    let r = (c / (2.0 * (th / 2.0).jsin())).abs();
    let h = r * (th / 2.0).jcos() * if b > 0.0 { 1.0 } else { -1.0 };
    let ux = (q.0 - p.0) / c;
    let uy = (q.1 - p.1) / c;
    ((p.0 + q.0) / 2.0 - uy * h, (p.1 + q.1) / 2.0 + ux * h)
}

/// `swExpr(src, quote)` → the SolidWorks equation text, or the reason there is none.
pub fn sw_expr(tr: &Catalog, src: &str, quote: &dyn Fn(&str) -> String) -> Result<String, String> {
    let r = expr::parse(src, None).map_err(|e| e.message)?;
    let mut bad: Option<String> = None;
    let text = sw_node(tr, &r.ast, 0, quote, &mut bad, 0);
    match bad {
        Some(b) => Err(b),
        None => Ok(text),
    }
}

/// Operator precedence (`P`); an operator outside the table is `undefined` there (no parentheses ever).
fn prec(op: &str) -> Option<u8> {
    Some(match op {
        "cmp" => 0,
        "+" | "-" => 1,
        "*" | "/" | "%" => 2,
        "^" => 3,
        _ => return None,
    })
}

fn sw_node(tr: &Catalog, n: &Ast, ctx: u8, quote: &dyn Fn(&str) -> String, bad: &mut Option<String>, depth: usize) -> String {
    // expr.js bounds its nesting when it parses; this walk keeps a bound of its own
    if depth > 400 {
        bad.get_or_insert_with(|| tr.s("swsketch.unknown_expression"));
        return "0".into();
    }
    let d = depth.saturating_add(1);
    match n {
        Ast::Num { v, name } => {
            if *name == Some("pi") {
                "pi".into()
            } else {
                num(*v)
            }
        }
        Ast::Var { name, .. } => quote(name),
        Ast::Neg(a) => format!("-{}", sw_node(tr, a, 4, quote, bad, d)),
        Ast::Cmp { .. } => {
            bad.get_or_insert_with(|| tr.s("swsketch.comparisons_have_no_solidworks"));
            "0".into()
        }
        Ast::Bin { op, a, b, .. } => {
            if *op == "%" {
                bad.get_or_insert_with(|| tr.s("swsketch.remainder_operator_has_no"));
                return "0".into();
            }
            let p = prec(op);
            // `w(n.a, p)`: an undefined precedence is a ctx of undefined (never parenthesised below)
            let pa = p.unwrap_or(0);
            let pb = p.map_or(0, |p| if *op == "^" { p } else { p.saturating_add(1) });
            let s = format!("{} {} {}", sw_node(tr, a, pa, quote, bad, d), op, sw_node(tr, b, pb, quote, bad, d));
            match p {
                Some(p) if p < ctx => format!("({s})"),
                _ => s,
            }
        }
        Ast::Call { name, args, .. } => match sw_func(name) {
            None => {
                bad.get_or_insert_with(|| tr.t("swsketch.fn_noform", &[("fn", name.clone())]));
                "0".into()
            }
            Some(f) => {
                let parts: Vec<String> = args.iter().map(|a| sw_node(tr, a, 0, quote, bad, d)).collect();
                format!("{f}({})", parts.join(", "))
            }
        },
    }
}

/// The drawing's units as `doc.meta.units || ''`.
fn units_of(doc: &Doc) -> String {
    doc.meta.get("units").filter(|v| js::truthy(Some(v))).map(js::to_js_string).unwrap_or_default()
}

/// `wholeKey(e)`.
fn whole_key(e: Option<&cc_cad::entity::Entity>) -> &'static str {
    match e.map(|e| &e.geo) {
        Some(Geo::Line(_)) => "L",
        Some(Geo::Circle(_) | Geo::Arc(_)) => "R",
        Some(Geo::Point(_)) => "p",
        _ => "null",
    }
}

/// `docToSwb(doc, { name })`. `date`: today as YYYY-MM-DD (`new Date().toISOString().slice(0, 10)`; the engine reads
/// no clock itself).
pub fn doc_to_swb(tr: &Catalog, doc: &Doc, name: &str, date: &str) -> Result<Swb, SwbError> {
    let mut notes = Vec::new();
    let unit = units_of(doc);
    let k = unit_mm(&unit).unwrap_or(1.0);
    if unit_mm(&unit).is_none() {
        notes.push(tr.t("swsketch.units_nomatch", &[("unit", unit.clone())]));
    }
    let sw_u = sw_unit(&unit).unwrap_or(0);
    if sw_unit(&unit).is_none() {
        notes.push(tr.s("swsketch.part_mm"));
    }
    // drawing units → metres (the API's unit); solver noise below 1e-9 dropped
    let m = |v: f64| -> String {
        let a = fixed_num(v, 9, 1e6);
        num(fixed_num(a * k / 1000.0, 12, 1e3))
    };
    let ents: Vec<&cc_cad::entity::Entity> = doc.space_map(cc_cad::doc::MODEL).map(|s| s.values().collect()).unwrap_or_default();
    let mut out = Counts::default();
    let mut skipped = 0usize;

    // ---- geometry: every sketch entity gets an index in segs() or pts()
    let mut geo: Vec<String> = Vec::new();
    let mut seg_of: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut pt_of: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut joins: Vec<(String, String)> = Vec::new();
    let (mut ns, mut np) = (0usize, 0usize);
    let mut seg = |geo: &mut Vec<String>, stmt: String, what: String| -> usize {
        let i = ns;
        ns = ns.saturating_add(1);
        geo.push(format!("Set segs({i}) = {stmt}"));
        geo.push(format!("If segs({i}) Is Nothing Then LogFail \"{what}\""));
        i
    };
    for e in &ents {
        let id = e.id.map_or_else(|| "undefined".to_string(), |i| i.to_string());
        match &e.geo {
            Geo::Line(l) => {
                let i = seg(
                    &mut geo,
                    format!("swSkMgr.CreateLine({}, {}, 0, {}, {}, 0)", m(l.x1), m(l.y1), m(l.x2), m(l.y2)),
                    format!("line {id}"),
                );
                seg_of.insert(format!("{id}|L"), i);
                pt_of.insert(format!("{id}|p1"), format!("segs({i}).GetStartPoint2"));
                pt_of.insert(format!("{id}|p2"), format!("segs({i}).GetEndPoint2"));
                out.lines = out.lines.saturating_add(1);
            }
            Geo::Circle(c) => {
                let i =
                    seg(&mut geo, format!("swSkMgr.CreateCircleByRadius({}, {}, 0, {})", m(c.cx), m(c.cy), m(c.r)), format!("circle {id}"));
                seg_of.insert(format!("{id}|R"), i);
                pt_of.insert(format!("{id}|c"), format!("segs({i}).GetCenterPoint2"));
                out.circles = out.circles.saturating_add(1);
            }
            Geo::Arc(a) => {
                let s = (a.cx + a.r * a.a0.jcos(), a.cy + a.r * a.a0.jsin());
                let t = (a.cx + a.r * a.a1.jcos(), a.cy + a.r * a.a1.jsin());
                let i = seg(
                    &mut geo,
                    format!("swSkMgr.CreateArc({}, {}, 0, {}, {}, 0, {}, {}, 0, 1)", m(a.cx), m(a.cy), m(s.0), m(s.1), m(t.0), m(t.1)),
                    format!("arc {id}"),
                );
                seg_of.insert(format!("{id}|R"), i);
                pt_of.insert(format!("{id}|c"), format!("segs({i}).GetCenterPoint2"));
                pt_of.insert(format!("{id}|s"), format!("segs({i}).GetStartPoint2"));
                pt_of.insert(format!("{id}|e"), format!("segs({i}).GetEndPoint2"));
                out.arcs = out.arcs.saturating_add(1);
            }
            Geo::Point(p) => {
                let i = np;
                np = np.saturating_add(1);
                geo.push(format!("Set pts({i}) = swSkMgr.CreatePoint({}, {}, 0)", m(p.x), m(p.y)));
                geo.push(format!("If pts({i}) Is Nothing Then LogFail \"point {id}\""));
                pt_of.insert(format!("{id}|p"), format!("pts({i})"));
                out.points = out.points.saturating_add(1);
            }
            Geo::Poly(pl) => {
                let n = pl.pts.len();
                let mut first: Vec<usize> = Vec::new();
                for j in 0..seg_count(e) {
                    let (Some(p), Some(q)) = (pl.pts.get(j), pl.pts.get(j.saturating_add(1).checked_rem(n).unwrap_or(0))) else { break };
                    let b = p.bulge();
                    let i = if b.abs() < 1e-12 {
                        out.lines = out.lines.saturating_add(1);
                        seg(
                            &mut geo,
                            format!("swSkMgr.CreateLine({}, {}, 0, {}, {}, 0)", m(p.x), m(p.y), m(q.x), m(q.y)),
                            format!("polyline {id} segment {}", j.saturating_add(1)),
                        )
                    } else {
                        let c = bulge_centre((p.x, p.y), (q.x, q.y), b);
                        // CreateArc runs from its start to its end, counter-clockwise for +1, clockwise for −1
                        out.arcs = out.arcs.saturating_add(1);
                        seg(
                            &mut geo,
                            format!(
                                "swSkMgr.CreateArc({}, {}, 0, {}, {}, 0, {}, {}, 0, {})",
                                m(c.0),
                                m(c.1),
                                m(p.x),
                                m(p.y),
                                m(q.x),
                                m(q.y),
                                if b > 0.0 { 1 } else { -1 }
                            ),
                            format!("polyline {id} arc {}", j.saturating_add(1)),
                        )
                    };
                    seg_of.insert(format!("{id}|s{j}"), i);
                    first.push(i);
                }
                let seg_txt = |i: Option<&usize>| i.map_or_else(|| "undefined".to_string(), usize::to_string);
                for j in 0..n {
                    if j < first.len() {
                        pt_of.insert(format!("{id}|v{j}"), format!("segs({}).GetStartPoint2", seg_txt(first.get(j))));
                    } else {
                        let prev = j.checked_sub(1).and_then(|x| first.get(x));
                        pt_of.insert(format!("{id}|v{j}"), format!("segs({}).GetEndPoint2", seg_txt(prev)));
                    }
                }
                for w in first.windows(2) {
                    if let [a, b] = w {
                        joins.push((format!("segs({a}).GetEndPoint2"), format!("segs({b}).GetStartPoint2")));
                    }
                }
                if pl.is_closed()
                    && first.len() > 1
                    && let (Some(l), Some(f)) = (first.last(), first.first())
                {
                    joins.push((format!("segs({l}).GetEndPoint2"), format!("segs({f}).GetStartPoint2")));
                }
            }
            _ => skipped = skipped.saturating_add(1),
        }
    }
    if skipped > 0 {
        notes.push(tr.t("swsketch.skipped", &[("n", skipped.to_string())]));
    }
    if ns == 0 && np == 0 {
        return Err(SwbError { error: tr.s("swsketch.there_nothing_model_put"), notes });
    }

    // ---- relations
    let mut rel: Vec<String> = Vec::new();
    let ref_expr = |e: u64, k: &str| -> Option<String> {
        let key = format!("{e}|{k}");
        if let Some(p) = pt_of.get(&key) {
            return Some(p.clone());
        }
        seg_of.get(&key).map(|i| format!("segs({i})"))
    };
    let mut mid_pts: std::collections::HashMap<u64, String> = std::collections::HashMap::new();
    let sel = |exprs: &[String], rname: &str, what: &str| -> Vec<String> {
        let mut lines = vec!["swModel.ClearSelection2 True".to_string()];
        for (j, x) in exprs.iter().enumerate() {
            lines.push(format!("Pick {x}, {}", if j > 0 { "True" } else { "False" }));
        }
        lines.push(format!("Relate \"{rname}\", \"{what}\""));
        lines
    };
    for (a, b) in &joins {
        rel.extend(sel(&[a.clone(), b.clone()], "sgCOINCIDENT", "polyline vertex"));
        out.relations = out.relations.saturating_add(1);
    }
    let mut lost_rel = 0usize;
    let cons: &[Con] = &doc.constraints;
    for c in cons {
        if is_dim(c) {
            continue;
        }
        let mut exprs: Vec<Option<String>> = Vec::with_capacity(c.refs.len());
        for r in c.refs.iter() {
            let x = if r.k == "mid" {
                // a line's middle: a construction point held at the middle (SolidWorks has no middle point of its own)
                if let Some(p) = mid_pts.get(&r.e) {
                    Some(p.clone())
                } else {
                    match model_get(doc, r.e).map(|e| e.geo) {
                        Some(Geo::Line(l)) => {
                            let i = np;
                            np = np.saturating_add(1);
                            rel.push(format!(
                                "Set pts({i}) = swSkMgr.CreatePoint({}, {}, 0)",
                                m((l.x1 + l.x2) / 2.0),
                                m((l.y1 + l.y2) / 2.0)
                            ));
                            let li = seg_of.get(&format!("{}|L", r.e)).map_or_else(|| "undefined".to_string(), usize::to_string);
                            rel.push(format!(
                                "If Not pts({i}) Is Nothing Then pts({i}).Select4 False, Nothing: segs({li}).Select4 True, Nothing: Relate \"sgATMIDDLE\", \"middle of line {}\"",
                                r.e
                            ));
                            let p = format!("pts({i})");
                            mid_pts.insert(r.e, p.clone());
                            Some(p)
                        }
                        _ => None,
                    }
                }
            } else if r.k == "*" {
                ref_expr(r.e, whole_key(model_get(doc, r.e).as_ref()))
            } else {
                ref_expr(r.e, &r.k)
            };
            exprs.push(x);
        }
        if exprs.iter().any(Option::is_none) {
            lost_rel = lost_rel.saturating_add(1);
            continue;
        }
        let sig: String = c.refs.iter().filter_map(|r| model_get(doc, r.e).and_then(|e| ref_kind(&e, &r.k))).collect();
        let rname = match c.typ.as_str() {
            "horizontal" => Some(if sig == "PP" { "sgHORIZONTALPOINTS2D" } else { "sgHORIZONTAL2D" }),
            "vertical" => Some(if sig == "PP" { "sgVERTICALPOINTS2D" } else { "sgVERTICAL2D" }),
            t => rel_name(t),
        };
        let Some(rname) = rname else {
            lost_rel = lost_rel.saturating_add(1);
            continue;
        };
        let exprs: Vec<String> = exprs.into_iter().flatten().collect();
        rel.extend(sel(&exprs, rname, &format!("{} {}", c.typ, c.id)));
        out.relations = out.relations.saturating_add(1);
    }
    if lost_rel > 0 {
        notes.push(tr.t("swsketch.lost_rel", &[("n", lost_rel.to_string())]));
    }
    if cons.iter().any(|c| c.typ == "symmetric") {
        notes.push(tr.s("swsketch.symmetric"));
    }

    // ---- dimensions (named; their values set; expressions become equations below)
    let mut dims: Vec<String> = Vec::new();
    let mut sketch_dims: Vec<String> = Vec::new();
    let undef = |x: Option<String>| x.unwrap_or_else(|| "undefined".to_string());
    for c in cons {
        if !is_dim(c) {
            continue;
        }
        let exprs: Vec<Option<String>> = c.refs.iter().map(|r| ref_expr(r.e, &r.k)).collect();
        if exprs.iter().any(Option::is_none) {
            continue;
        }
        let at = dim_entity_for(doc, c, None)
            .and_then(|d| match d.geo {
                Geo::Dim(dm) => dm.loc,
                _ => None,
            })
            .map_or((0.0, 0.0), |p| (p.x, p.y));
        let call = match c.typ.as_str() {
            "hdist" => "swModel.AddHorizontalDimension2(",
            "vdist" => "swModel.AddVerticalDimension2(",
            "radius" => "swModel.AddRadialDimension2(",
            "diameter" => "swModel.AddDiameterDimension2(",
            _ => "swModel.AddDimension2(",
        };
        let pick_list: Vec<String> = if c.typ == "angle" && c.refs.len() == 1 {
            let e = c.refs.first().map(|r| r.e).unwrap_or_default();
            ["c", "s", "e"].iter().map(|k| undef(pt_of.get(&format!("{e}|{k}")).cloned())).collect()
        } else {
            exprs.into_iter().map(undef).collect()
        };
        let cv = c.obj.get("value").map_or(f64::NAN, js::to_number);
        let value = if c.typ == "angle" { num(cv * std::f64::consts::PI / 180.0) } else { m(cv) };
        let cname = c.obj.get("name").map_or_else(|| "undefined".to_string(), js::to_js_string);
        dims.push("swModel.ClearSelection2 True".into());
        for (j, x) in pick_list.iter().enumerate() {
            dims.push(format!("Pick {x}, {}", if j > 0 { "True" } else { "False" }));
        }
        dims.push(format!("Dimension {call}{}, {}, 0), \"{cname}\", {value}, \"{cname} ({})\"", m(at.0), m(at.1), c.typ));
        sketch_dims.push(cname);
        out.dimensions = out.dimensions.saturating_add(1);
    }

    // ---- global variables and equations (EquationMgr.Add2, index -1 = at the end)
    let mut eqs: Vec<String> = Vec::new();
    let p = params_of(doc);
    let ev = params::evaluate_vars(&p, None);
    let has_dim = |n: &str| sketch_dims.iter().any(|d| d == n);
    let quote = |n: &str| -> String { if has_dim(n) { format!("\"\"{n}@\" & sketchName & \"\"\"") } else { format!("\"\"{n}\"\"") } };
    for n in ev.order.iter().filter(|n| p.vars.iter().any(|v| &v.name == *n)) {
        let Some(v) = p.vars.iter().find(|x| &x.name == n) else { continue };
        let dim = has_dim(n);
        let lit = matches!(expr::parse(&v.expr, Some(&[])), Ok(r) if matches!(r.ast, Ast::Num { .. }));
        if dim && lit {
            continue; // a plain number: the dimension's own value
        }
        let value = || expr::format_number(ev.values.get(n).copied().unwrap_or(0.0));
        let text = match sw_expr(tr, &v.expr, &quote) {
            Ok(t) => t,
            Err(why) => {
                let key = if dim { "swsketch.eq_value.dim" } else { "swsketch.eq_value.param" };
                notes.push(tr.t(key, &[("name", n.clone()), ("why", why), ("value", value())]));
                value()
            }
        };
        eqs.push(format!("AddEquation \"{} = {text}\", \"{n}\"", quote(n)));
        if dim {
            out.equations = out.equations.saturating_add(1);
        } else {
            out.globals = out.globals.saturating_add(1);
        }
    }
    if !doc.layouts.is_empty() {
        notes.push(tr.s("swsketch.sheets_not_part_sketch"));
    }

    // ---- the macro
    let sn = safe(name);
    let mut lines: Vec<String> = vec![
        format!("' {sn}.swb - written by the CadCraft drafting app (getcadcraft.com) on {date}."),
        "' Run it in SolidWorks: Tools, Macro, Run, then choose this file. It makes a new part with one sketch on the Front Plane:".into(),
        format!(
            "' {} lines, {} arcs, {} circles, {} points, {} relations, {} dimensions, {} global variables, {} equations.",
            out.lines, out.arcs, out.circles, out.points, out.relations, out.dimensions, out.globals, out.equations
        ),
        format!("' Not yet tested in SolidWorks. Failures are written to {sn}-cadcraft.log next to this macro; the macro carries on."),
        "' Units: the part uses the drawing's length unit; the API takes metres, so every coordinate below is in metres.".into(),
    ];
    for s in ["Option Explicit", "Dim swApp As Object", "Dim swModel As Object", "Dim swSkMgr As Object", "Dim swEqnMgr As Object"] {
        lines.push(s.into());
    }
    lines.push(format!("Dim segs({}) As Object", ns.saturating_sub(1)));
    lines.push(format!("Dim pts({}) As Object", np.saturating_sub(1)));
    for s in HEAD_TAIL {
        lines.push((*s).into());
    }
    let mut subs: Vec<(String, Vec<String>)> = Vec::new();
    let mut chunked = |prefix: &str, stmts: &[String]| {
        for (ci, ch) in stmts.chunks(150).enumerate() {
            let nm = format!("{prefix}{}", ci.saturating_add(1));
            let mut body = vec!["    On Error Resume Next".to_string()];
            body.extend(ch.iter().map(|s| format!("    {s}")));
            body.push("    Err.Clear".into());
            subs.push((nm, body));
        }
    };
    chunked("Geometry", &geo);
    chunked("Relations", &rel);
    chunked("Dimensions", &dims);
    chunked("Equations", &eqs);
    for (nm, body) in &subs {
        lines.push(format!("Sub {nm}()"));
        lines.extend(body.iter().cloned());
        lines.push("End Sub".into());
        lines.push(String::new());
    }
    lines.extend(
        ["Sub main()", "    Dim tpl As String", "    Dim ok As Boolean", "    Dim feat As Object", "    Set swApp = Application.SldWorks"]
            .iter()
            .map(|s| (*s).to_string()),
    );
    lines.push(format!("    logPath = swApp.GetCurrentMacroPathFolder() & \"\\{sn}-cadcraft.log\""));
    lines.push(format!("    tpl = swApp.GetUserPreferenceStringValue({SW_DEFAULT_TEMPLATE_PART})"));
    lines.extend(
        [
            "    Set swModel = swApp.NewDocument(tpl, 0, 0, 0)",
            "    If swModel Is Nothing Then",
            "        MsgBox \"Could not make a new part from the default part template (\" & tpl & \"). Set one in Tools, Options, Default Templates.\"",
            "        Exit Sub",
            "    End If",
        ]
        .iter()
        .map(|s| (*s).to_string()),
    );
    lines.push(format!(
        "    ok = swModel.Extension.SetUserPreferenceInteger({SW_UNITS_LINEAR}, {SW_DETAILING_NO_OPTION_SPECIFIED}, {sw_u})"
    ));
    lines.push(format!("    swApp.SetUserPreferenceToggle {SW_INPUT_DIM_VAL_ON_CREATE}, False"));
    lines.extend(
        [
            "    Set swSkMgr = swModel.SketchManager",
            "    ok = swModel.Extension.SelectByID2(\"Front Plane\", \"PLANE\", 0, 0, 0, False, 0, Nothing, 0)",
            "    If Not ok Then LogFail \"selecting the Front Plane\"",
            "    swSkMgr.InsertSketch True",
            "    swSkMgr.AddToDB = True",
            "    swSkMgr.DisplayWhenAdded = False",
            "    Set feat = swSkMgr.ActiveSketch",
            "    sketchName = \"Sketch1\"",
            "    If Not feat Is Nothing Then sketchName = feat.Name",
        ]
        .iter()
        .map(|s| (*s).to_string()),
    );
    for (nm, _) in &subs {
        if nm.starts_with("Geometry") || nm.starts_with("Relations") || nm.starts_with("Dimensions") {
            lines.push(format!("    {nm}"));
        }
    }
    lines.extend(
        [
            "    swSkMgr.AddToDB = False",
            "    swSkMgr.DisplayWhenAdded = True",
            "    swSkMgr.InsertSketch True",
            "    Set swEqnMgr = swModel.GetEquationMgr()",
        ]
        .iter()
        .map(|s| (*s).to_string()),
    );
    for (nm, _) in &subs {
        if nm.starts_with("Equations") {
            lines.push(format!("    {nm}"));
        }
    }
    lines.extend(
        [
            "    ok = swModel.EditRebuild3()",
            "    swModel.ViewZoomtofit2",
            "    MsgBox \"CadCraft sketch: \" & done & \" relations, dimensions and equations made, \" & fails & \" could not be made\" & IIf(fails > 0, \" (see \" & logPath & \").\", \".\")",
            "End Sub",
            "",
        ]
        .iter()
        .map(|s| (*s).to_string()),
    );
    let mut text = lines.join("\r\n");
    text.push_str("\r\n");
    let bytes: Vec<u8> = text.chars().map(|ch| u8::try_from(u32::from(ch)).unwrap_or(63)).collect();
    let summary = tr.t(
        "swsketch.summary",
        &[
            ("lines", out.lines.to_string()),
            ("arcs", out.arcs.to_string()),
            ("circles", out.circles.to_string()),
            ("points", out.points.to_string()),
            ("relations", out.relations.to_string()),
            ("dimensions", out.dimensions.to_string()),
            ("globals", out.globals.to_string()),
            ("equations", out.equations.to_string()),
        ],
    );
    Ok(Swb { text, bytes, notes, summary, counts: out })
}

/// The fixed part of the head after the arrays: its globals and helper Subs.
const HEAD_TAIL: &[&str] = &[
    "Dim sketchName As String",
    "Dim logPath As String",
    "Dim fails As Long",
    "Dim done As Long",
    "",
    "Sub LogFail(what As String)",
    "    Dim f As Integer",
    "    fails = fails + 1",
    "    On Error Resume Next",
    "    f = FreeFile",
    "    Open logPath For Append As #f",
    "    Print #f, Now & \"  could not make: \" & what",
    "    Close #f",
    "    Err.Clear",
    "End Sub",
    "",
    "Sub Pick(obj As Object, append As Boolean)",
    "    On Error Resume Next",
    "    If obj Is Nothing Then Exit Sub",
    "    obj.Select4 append, Nothing",
    "    Err.Clear",
    "End Sub",
    "",
    "Sub Relate(kind As String, what As String)",
    "    On Error Resume Next",
    "    swModel.SketchAddConstraints kind",
    "    If Err.Number <> 0 Then LogFail what & \" (\" & kind & \")\" Else done = done + 1",
    "    Err.Clear",
    "    swModel.ClearSelection2 True",
    "End Sub",
    "",
    "Sub Dimension(dd As Object, dimName As String, v As Double, what As String)",
    "    Dim d As Object",
    "    On Error Resume Next",
    "    If dd Is Nothing Then LogFail what: Exit Sub",
    "    Set d = dd.GetDimension2(0)",
    "    If d Is Nothing Then LogFail what: Exit Sub",
    "    d.Name = dimName",
    "    d.SystemValue = v",
    "    If Err.Number <> 0 Then LogFail what & \" (name or value)\" Else done = done + 1",
    "    Err.Clear",
    "    swModel.ClearSelection2 True",
    "End Sub",
    "",
    "Sub AddEquation(text As String, what As String)",
    "    Dim r As Long",
    "    On Error Resume Next",
    "    r = swEqnMgr.Add2(-1, text, True)",
    "    If Err.Number <> 0 Or r < 0 Then LogFail \"equation for \" & what & \": \" & text Else done = done + 1",
    "    Err.Clear",
    "End Sub",
    "",
];

/// `new Date(ms).toISOString().slice(0, 10)`.
pub fn iso_date(ms: f64) -> String {
    let days = (ms / 86_400_000.0).floor();
    // civil from days (Howard Hinnant's algorithm), in f64 so any finite input stays in range
    let z = days + 719_468.0;
    let era = (z / 146_097.0).floor();
    let doe = z - era * 146_097.0;
    let yoe = ((doe - (doe / 1460.0).floor() + (doe / 36_524.0).floor() - (doe / 146_096.0).floor()) / 365.0).floor();
    let y = yoe + era * 400.0;
    let doy = doe - (365.0 * yoe + (yoe / 4.0).floor() - (yoe / 100.0).floor());
    let mp = ((5.0 * doy + 2.0) / 153.0).floor();
    let d = doy - ((153.0 * mp + 2.0) / 5.0).floor() + 1.0;
    let mo = if mp < 10.0 { mp + 3.0 } else { mp - 9.0 };
    let y = if mo <= 2.0 { y + 1.0 } else { y };
    format!("{:04}-{:02}-{:02}", y as i64, mo as i64, d as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_names_as_the_js_writes_them() {
        assert_eq!(num(0.1), "0.1");
        assert_eq!(num(1e-7), "1.00000000000000E-7");
        assert_eq!(num(0.30000000000000004), "0.3");
        assert_eq!(num(-0.0254), "-0.0254");
        assert_eq!(safe("My drawing (v2)"), "My_drawing_v2_");
        assert_eq!(safe(""), "Drawing1");
        assert_eq!(underscore_runs("a  b/c"), "a_b_c");
        assert_eq!(iso_date(1_760_184_000_000.0), "2025-10-11");
        assert_eq!(iso_date(0.0), "1970-01-01");
    }

    #[test]
    fn the_short_ways_are_the_long_ways() {
        // num and Number(toFixed) by their fast paths vs the full conversions, on numbers like a drawing's
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for i in 0..200_000u32 {
            let r = next();
            let mag = [1e-12, 1e-6, 0.001, 1.0, 37.0, 1e3, 99_999.0, 1e6, 3e7, 1e12][(r % 10) as usize];
            let frac = (next() >> 11) as f64 / (1u64 << 53) as f64;
            let v = match i % 4 {
                0 => (frac - 0.5) * mag,
                1 => ((frac - 0.5) * mag * 1000.0).round() / 1000.0,
                2 => ((frac - 0.5) * mag * 1e9).round() / 1e9,
                _ => frac * mag * 25.4 / 1000.0,
            };
            let long_num = {
                let s = cc_part::js::num_str(cc_part::js::prec_num(v, 15));
                if s.contains(['e', 'E']) { js::to_exponential(v, 14).replace('e', "E") } else { s }
            };
            assert_eq!(num(v), long_num, "num({v:e})");
            for (f, lim) in [(9usize, 1e6), (12, 1e3)] {
                let long = js::str_to_number(&js::to_fixed(v, f));
                assert_eq!(fixed_num(v, f, lim).to_bits(), long.to_bits(), "toFixed({v:e}, {f})");
            }
        }
    }

    #[test]
    fn a_line_and_a_circle() {
        let mut doc = Doc::new();
        doc.load(&serde_json::json!({ "entities": [
            { "id": 1, "type": "line", "layer": "0", "x1": 0, "y1": 0, "x2": 10, "y2": 0 },
            { "id": 2, "type": "circle", "layer": "0", "cx": 5, "cy": 5, "r": 2.5 },
            { "id": 3, "type": "text", "layer": "0", "x": 0, "y": 0, "s": "x", "h": 1 }
        ], "meta": { "units": "millimeters" } }));
        let cat = Catalog::new(&serde_json::json!({ "swsketch.skipped": "{n} skipped" }));
        let r = doc_to_swb(&cat, &doc, "T", "2026-01-02").map_err(|e| e.error);
        let Ok(r) = r else { panic!("no macro") };
        assert!(r.text.contains("Set segs(0) = swSkMgr.CreateLine(0, 0, 0, 0.01, 0, 0)\r\n"));
        assert!(r.text.contains("swSkMgr.CreateCircleByRadius(0.005, 0.005, 0, 0.0025)"));
        assert_eq!(r.notes, vec!["1 skipped".to_string()]);
        assert_eq!(r.counts.lines, 1);
        assert!(r.text.ends_with("End Sub\r\n\r\n"));
    }
}
