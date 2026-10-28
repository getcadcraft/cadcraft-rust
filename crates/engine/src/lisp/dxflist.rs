//! CadCraft objects as AutoLISP's DXF lists, and back (dxflist.js). The group codes are DXF's; values follow entget's
//! conventions: angles in radians, points as (code x y z), entity names for -1 / -2 / 330.
//!
//! The objects are handled as their JSON (doc.js's plain objects: [`cc_cad::entity::Entity::to_obj`]), as the JS does
//! with `{ ...old }` and assignments, then read back into typed objects.

use cc_cad::Layer;
use cc_cad::entity::Obj;
use cc_cad::js;
use cc_cad_geom::jsmath::JsMath;
use cc_lisp::host::{Pairs, Pv};
use cc_lisp::value::EId;
use serde_json::{Value, json};

const TAU: f64 = std::f64::consts::PI * 2.0;
fn na(a: f64) -> f64 {
    ((a % TAU) + TAU) % TAU
}
/// `+v || 0`
fn n0(v: Option<&Value>) -> f64 {
    let x = js::to_number_opt(v);
    if x.is_nan() { 0.0 } else { x }
}
fn p3(x: Option<&Value>, y: Option<&Value>) -> Pv {
    Pv::List(vec![Pv::Num(n0(x)), Pv::Num(n0(y)), Pv::Num(0.0)])
}
fn num_pv(v: Option<&Value>) -> Pv {
    match v {
        None | Some(Value::Null) => Pv::Nil,
        Some(Value::Number(n)) => Pv::Num(n.as_f64().unwrap_or(f64::NAN)),
        Some(Value::String(s)) => Pv::Str(s.clone()),
        Some(Value::Bool(true)) => Pv::T,
        Some(Value::Bool(false)) => Pv::Nil,
        Some(other) => Pv::Str(js::to_js_string(other)),
    }
}
fn i(n: f64) -> Pv {
    Pv::Int(cc_lisp::value::to_i32(n))
}
/// A finite number of the object (`Number.isFinite(e[k])`).
fn fin(e: &Obj, k: &str) -> Option<f64> {
    js::finite(e.get(k))
}
fn truthy(e: &Obj, k: &str) -> bool {
    js::truthy(e.get(k))
}
fn sub_pt(e: &Obj, k: &str) -> Option<(Option<Value>, Option<Value>)> {
    let o = e.get(k)?;
    Some((o.get("x").cloned(), o.get("y").cloned()))
}

/// The DXF type name of a CadCraft object.
pub fn dxf_type(e: &Obj) -> String {
    match e.get("type").and_then(Value::as_str).unwrap_or("") {
        "line" => "LINE".into(),
        "circle" => "CIRCLE".into(),
        "arc" => "ARC".into(),
        "poly" => "LWPOLYLINE".into(),
        "ellipse" => "ELLIPSE".into(),
        "text" => {
            let multi = truthy(e, "ww") || e.get("s").map(js::to_js_string).unwrap_or_default().contains('\n');
            if multi { "MTEXT".into() } else { "TEXT".into() }
        }
        "point" => "POINT".into(),
        "insert" => "INSERT".into(),
        "hatch" => "HATCH".into(),
        "dim" => "DIMENSION".into(),
        "viewport" => "VIEWPORT".into(),
        "mleader" => "MULTILEADER".into(),
        "face3d" => "3DFACE".into(),
        other => other.to_uppercase(),
    }
}
fn subclass(t: &str) -> Option<&'static str> {
    Some(match t {
        "LINE" => "AcDbLine",
        "CIRCLE" | "ARC" => "AcDbCircle",
        "LWPOLYLINE" => "AcDbPolyline",
        "ELLIPSE" => "AcDbEllipse",
        "TEXT" => "AcDbText",
        "MTEXT" => "AcDbMText",
        "POINT" => "AcDbPoint",
        "INSERT" => "AcDbBlockReference",
        "HATCH" => "AcDbHatch",
        "DIMENSION" => "AcDbDimension",
        "VIEWPORT" => "AcDbViewport",
        "MULTILEADER" => "AcDbMLeader",
        _ => return None,
    })
}
/// The object's class name (vla ObjectName).
pub fn object_name(e: &Obj) -> String {
    let t = dxf_type(e);
    if t == "ARC" {
        return "AcDbArc".into();
    }
    subclass(&t).unwrap_or("AcDbEntity").to_string()
}

pub fn ha_code(s: Option<&str>) -> Option<f64> {
    Some(match s? {
        "left" => 0.0,
        "center" => 1.0,
        "right" => 2.0,
        _ => return None,
    })
}
pub fn va_code(s: Option<&str>) -> Option<f64> {
    Some(match s? {
        "baseline" => 0.0,
        "bottom" => 1.0,
        "middle" => 2.0,
        "top" => 3.0,
        _ => return None,
    })
}
fn attach(ha: Option<&str>, va: Option<&str>) -> f64 {
    let v = match va.unwrap_or("top") {
        "top" => 0.0,
        "middle" => 3.0,
        "bottom" | "baseline" => 6.0,
        _ => 0.0,
    };
    let h = match ha.unwrap_or("left") {
        "left" => 1.0,
        "center" => 2.0,
        "right" => 3.0,
        _ => 1.0,
    };
    v + h
}

/// What toPairs adds around the object's own groups.
#[derive(Default)]
pub struct PairCtx {
    pub ename: Option<EId>,
    pub owner: Option<EId>,
    pub handle: Option<String>,
    pub space: Option<String>,
}

/// An object → its DXF list (entget).
pub fn to_pairs(e: &Obj, ctx: &PairCtx) -> Pairs {
    let ty = dxf_type(e);
    let mut out: Pairs = Vec::new();
    if let Some(n) = &ctx.ename {
        out.push((-1, Pv::E(n.clone())));
    }
    out.push((0, Pv::Str(ty.clone())));
    if let Some(o) = &ctx.owner {
        out.push((330, Pv::E(o.clone())));
    }
    if let Some(h) = ctx.handle.as_ref().filter(|h| !h.is_empty()) {
        out.push((5, Pv::Str(h.clone())));
    }
    out.push((100, Pv::Str("AcDbEntity".into())));
    if let Some(sp) = ctx.space.as_ref().filter(|s| !s.is_empty()) {
        if sp != "Model" {
            out.push((67, Pv::Int(1)));
        }
        out.push((410, Pv::Str(sp.clone())));
    }
    out.push((
        8,
        match e.get("layer") {
            None | Some(Value::Null) => Pv::Str("0".into()),
            Some(v) => num_pv(Some(v)),
        },
    ));
    if truthy(e, "lt") {
        out.push((6, num_pv(e.get("lt"))));
    }
    match e.get("color") {
        Some(Value::Number(n)) if n.as_f64() != Some(256.0) => out.push((62, i(n.as_f64().unwrap_or(0.0)))),
        Some(Value::String(s))
            if s.len() == 7 && s.starts_with('#') && s.get(1..).is_some_and(|h| h.chars().all(|c| c.is_ascii_hexdigit())) =>
        {
            out.push((62, Pv::Int(7)));
            out.push((420, Pv::Int(i32::from_str_radix(s.get(1..).unwrap_or("0"), 16).unwrap_or(0))));
        }
        _ => {}
    }
    if let Some(lw) = fin(e, "lw") {
        out.push((370, i(if lw < 0.0 { lw } else { js::round(lw * 100.0) })));
    }
    if truthy(e, "lts") && e.get("lts").and_then(Value::as_f64) != Some(1.0) {
        out.push((48, num_pv(e.get("lts"))));
    }
    if let Some(s) = subclass(&ty) {
        out.push((100, Pv::Str(s.into())));
    }
    let g = |k: &str| e.get(k);
    let z = || Pv::List(vec![Pv::Num(0.0), Pv::Num(0.0), Pv::Num(1.0)]);
    match e.get("type").and_then(Value::as_str).unwrap_or("") {
        "line" => {
            out.push((10, p3(g("x1"), g("y1"))));
            out.push((11, p3(g("x2"), g("y2"))));
            out.push((210, z()));
        }
        "circle" => {
            out.push((10, p3(g("cx"), g("cy"))));
            out.push((40, num_pv(g("r"))));
            out.push((210, z()));
        }
        "arc" => {
            out.push((10, p3(g("cx"), g("cy"))));
            out.push((40, num_pv(g("r"))));
            out.push((100, Pv::Str("AcDbArc".into())));
            out.push((50, Pv::Num(na(js::to_number_opt(g("a0"))))));
            out.push((51, Pv::Num(na(js::to_number_opt(g("a1"))))));
            out.push((210, z()));
        }
        "poly" => {
            let pts: Vec<Value> = g("pts").and_then(Value::as_array).cloned().unwrap_or_default();
            out.push((90, i(pts.len() as f64)));
            out.push((70, Pv::Int(i32::from(truthy(e, "closed")))));
            out.push((43, Pv::Num(fin(e, "w").unwrap_or(0.0))));
            out.push((38, Pv::Num(0.0)));
            for p in &pts {
                out.push((10, Pv::List(vec![Pv::Num(n0(p.get("x"))), Pv::Num(n0(p.get("y")))])));
                out.push((40, Pv::Num(js::finite(p.get("sw")).unwrap_or(0.0))));
                out.push((41, Pv::Num(js::finite(p.get("ew")).unwrap_or(0.0))));
                out.push((42, Pv::Num(n0(p.get("b")))));
                out.push((91, Pv::Int(0)));
            }
            out.push((210, z()));
        }
        "ellipse" => {
            out.push((10, p3(g("cx"), g("cy"))));
            out.push((11, p3(g("mx"), g("my"))));
            out.push((210, z()));
            out.push((40, num_pv(g("ratio"))));
            out.push((41, g("t0").filter(|v| !v.is_null()).map_or(Pv::Num(0.0), |v| num_pv(Some(v)))));
            out.push((42, g("t1").filter(|v| !v.is_null()).map_or(Pv::Num(TAU), |v| num_pv(Some(v)))));
        }
        "point" => {
            out.push((10, p3(g("x"), g("y"))));
            out.push((210, z()));
            out.push((50, Pv::Num(0.0)));
        }
        "text" => {
            let rot = js::or(js::finite(g("rot")).or_else(|| g("rot").and_then(Value::as_f64)), 0.0);
            if ty == "MTEXT" {
                out.push((10, p3(g("x"), g("y"))));
                out.push((40, num_pv(g("h"))));
                out.push((41, if truthy(e, "ww") { num_pv(g("ww")) } else { Pv::Num(0.0) }));
                out.push((71, i(attach(g("ha").and_then(Value::as_str), g("va").and_then(Value::as_str)))));
                out.push((72, Pv::Int(1)));
                let s = g("s").filter(|v| !v.is_null()).map(js::to_js_string).unwrap_or_default();
                out.push((1, Pv::Str(s.replace('\n', "\\P"))));
                out.push((7, Pv::Str("Standard".into())));
                out.push((210, z()));
                out.push((11, Pv::List(vec![Pv::Num(rot.jcos()), Pv::Num(rot.jsin()), Pv::Num(0.0)])));
                out.push((50, Pv::Num(na(rot))));
                out.push((73, Pv::Int(1)));
                out.push((44, if truthy(e, "ls") { num_pv(g("ls")) } else { Pv::Num(1.0) }));
            } else {
                let h = ha_code(g("ha").and_then(Value::as_str)).unwrap_or(0.0);
                let v = va_code(g("va").and_then(Value::as_str)).unwrap_or(0.0);
                out.push((10, p3(g("x"), g("y"))));
                out.push((40, num_pv(g("h"))));
                out.push((1, Pv::Str(g("s").filter(|v| !v.is_null()).map(js::to_js_string).unwrap_or_default())));
                out.push((50, Pv::Num(na(rot))));
                out.push((41, if truthy(e, "wf") { num_pv(g("wf")) } else { Pv::Num(1.0) }));
                out.push((51, if truthy(e, "obl") { num_pv(g("obl")) } else { Pv::Num(0.0) }));
                out.push((7, Pv::Str("Standard".into())));
                out.push((71, Pv::Int(0)));
                out.push((72, i(h)));
                out.push((11, if h != 0.0 || v != 0.0 { p3(g("x"), g("y")) } else { p3(None, None) }));
                out.push((210, z()));
                out.push((100, Pv::Str("AcDbText".into())));
                out.push((73, i(v)));
            }
        }
        "insert" => {
            let has_att = g("attrs").and_then(Value::as_object).is_some_and(|o| !o.is_empty());
            if has_att {
                out.push((66, Pv::Int(1)));
            }
            out.push((2, num_pv(g("block"))));
            out.push((10, p3(g("x"), g("y"))));
            let sx = g("sx").filter(|v| !v.is_null()).map_or(Pv::Num(1.0), |v| num_pv(Some(v)));
            out.push((41, sx.clone()));
            out.push((42, g("sy").filter(|v| !v.is_null()).map_or(Pv::Num(1.0), |v| num_pv(Some(v)))));
            out.push((43, g("sz").filter(|v| !v.is_null()).map_or(sx, |v| num_pv(Some(v)))));
            let rot = js::or(g("rot").and_then(Value::as_f64), 0.0);
            out.push((50, Pv::Num(na(rot))));
            out.push((70, Pv::Int(0)));
            out.push((71, Pv::Int(0)));
            out.push((44, Pv::Num(0.0)));
            out.push((45, Pv::Num(0.0)));
            out.push((210, z()));
        }
        "hatch" => {
            let solid = truthy(e, "solid");
            out.push((10, p3(None, None)));
            out.push((210, z()));
            out.push((
                2,
                if solid {
                    Pv::Str("SOLID".into())
                } else if truthy(e, "pattern") {
                    num_pv(g("pattern"))
                } else {
                    Pv::Str("ANSI31".into())
                },
            ));
            out.push((70, Pv::Int(i32::from(solid))));
            out.push((71, Pv::Int(0)));
            let loops: Vec<Value> = g("loops").and_then(Value::as_array).cloned().unwrap_or_default();
            out.push((91, i(loops.len() as f64)));
            for lp in &loops {
                let a = lp.as_array().cloned().unwrap_or_default();
                let n = a.len() / 2;
                out.push((92, Pv::Int(2)));
                out.push((72, Pv::Int(0)));
                out.push((73, Pv::Int(1)));
                out.push((93, i(n as f64)));
                for k in 0..n {
                    let x = a.get(k.saturating_mul(2)).map_or(Pv::Nil, |v| num_pv(Some(v)));
                    let y = a.get(k.saturating_mul(2).saturating_add(1)).map_or(Pv::Nil, |v| num_pv(Some(v)));
                    out.push((10, Pv::List(vec![x, y])));
                }
                out.push((97, Pv::Int(0)));
            }
            out.push((75, Pv::Int(1)));
            out.push((76, Pv::Int(1)));
            if !solid {
                out.push((52, if truthy(e, "pangle") { num_pv(g("pangle")) } else { Pv::Num(0.0) }));
                out.push((41, if truthy(e, "pscale") { num_pv(g("pscale")) } else { Pv::Num(1.0) }));
            }
        }
        "dim" => {
            let kind = match g("kind").and_then(Value::as_str).unwrap_or("") {
                "linear" => 0,
                "aligned" => 1,
                "angular" => 5,
                "diameter" => 3,
                "radius" => 4,
                _ => 0,
            };
            if truthy(e, "block") {
                out.push((2, num_pv(g("block"))));
            }
            let loc = sub_pt(e, "loc");
            out.push((
                10,
                match &loc {
                    Some((x, y)) => p3(x.as_ref(), y.as_ref()),
                    None => p3(None, None),
                },
            ));
            out.push((70, Pv::Int(kind | 32)));
            out.push((1, g("text").filter(|v| !v.is_null()).map_or(Pv::Str(String::new()), |v| num_pv(Some(v)))));
            out.push((3, Pv::Str("Standard".into())));
            for (code, k) in [(13, "p1"), (14, "p2"), (15, "c")] {
                if g(k).is_some_and(|v| js::truthy(Some(v))) {
                    let (x, y) = sub_pt(e, k).unwrap_or((None, None));
                    out.push((code, p3(x.as_ref(), y.as_ref())));
                }
            }
            if kind <= 1 {
                out.push((50, Pv::Num(na(js::or(g("rot").and_then(Value::as_f64), 0.0)))));
            }
        }
        "viewport" => {
            out.push((10, p3(g("x"), g("y"))));
            out.push((40, num_pv(g("w"))));
            out.push((41, num_pv(g("h"))));
            out.push((12, p3(g("cx"), g("cy"))));
            let h = js::to_number_opt(g("h"));
            let s = js::to_number_opt(g("s"));
            out.push((45, Pv::Num(if truthy(e, "s") { h / s } else { h })));
            out.push((68, Pv::Int(if g("on") == Some(&Value::Bool(false)) { 0 } else { 1 })));
        }
        "mleader" => {
            if let Some(pts) = g("pts").and_then(Value::as_array).filter(|a| !a.is_empty()) {
                for p in pts {
                    out.push((10, p3(p.get("x"), p.get("y"))));
                }
            }
            out.push((304, Pv::Str(g("s").filter(|v| !v.is_null()).map(js::to_js_string).unwrap_or_default())));
            out.push((41, if truthy(e, "h") { num_pv(g("h")) } else { Pv::Num(2.5) }));
        }
        _ => {}
    }
    out
}

// ---- reading pairs ----

/// `val(v)`: an integer as a number.
fn val(v: &Pv) -> Pv {
    match v {
        Pv::Int(n) => Pv::Num(f64::from(*n)),
        other => other.clone(),
    }
}
/// The first value of a code.
pub fn get(pairs: &Pairs, code: i32) -> Option<Pv> {
    pairs.iter().find(|(c, _)| *c == code).map(|(_, v)| val(v))
}
fn has(pairs: &Pairs, code: i32) -> bool {
    pairs.iter().any(|(c, _)| *c == code)
}
/// `num(pairs, code, d)`: a finite number, else d.
fn num(pairs: &Pairs, code: i32, d: f64) -> f64 {
    match get(pairs, code) {
        Some(Pv::Num(n)) if n.is_finite() => n,
        _ => d,
    }
}
/// A point (two or more finite numbers) of a code.
fn pt2(pairs: &Pairs, code: i32) -> Option<(f64, f64)> {
    match get(pairs, code)? {
        Pv::List(xs) if xs.len() >= 2 => {
            let ns: Option<Vec<f64>> = xs.iter().map(|x| if let Pv::Num(n) = x { n.is_finite().then_some(*n) } else { None }).collect();
            let ns = ns?;
            Some((ns.first().copied()?, ns.get(1).copied()?))
        }
        _ => None,
    }
}
/// `String(v)` of a plain value.
pub fn js_str(v: &Pv) -> String {
    match v {
        Pv::Nil => "null".into(),
        Pv::T => "true".into(),
        Pv::Num(n) => js::num_to_string(*n),
        Pv::Int(n) => n.to_string(),
        Pv::Str(s) => s.clone(),
        Pv::List(xs) => xs.iter().map(|x| if matches!(x, Pv::Nil) { String::new() } else { js_str(x) }).collect::<Vec<_>>().join(","),
        _ => "[object Object]".into(),
    }
}
/// `+v` of a plain value.
fn js_num(v: &Pv) -> f64 {
    match v {
        Pv::Nil => 0.0,
        Pv::T => 1.0,
        Pv::Num(n) => *n,
        Pv::Int(n) => f64::from(*n),
        Pv::Str(s) => js::str_to_number(s),
        Pv::List(xs) => match xs.as_slice() {
            [] => 0.0,
            [x] => js::str_to_number(&js_str(x)),
            _ => f64::NAN,
        },
        _ => f64::NAN,
    }
}
fn js_truthy(v: &Pv) -> bool {
    match v {
        Pv::Nil => false,
        Pv::Num(n) => *n != 0.0 && !n.is_nan(),
        Pv::Str(s) => !s.is_empty(),
        _ => true,
    }
}
fn to_json(v: &Pv) -> Value {
    match v {
        Pv::Nil => Value::Null,
        Pv::T => Value::Bool(true),
        Pv::Num(n) => cc_cad::entity::num(*n),
        Pv::Int(n) => json!(*n),
        Pv::Str(s) => Value::String(s.clone()),
        Pv::List(xs) => Value::Array(xs.iter().map(to_json).collect()),
        _ => Value::Null,
    }
}
fn set_num(e: &mut Obj, k: &str, v: f64) {
    e.insert(k.into(), cc_cad::entity::num(v));
}

/// What fromPairs needs besides the pairs.
#[derive(Default)]
pub struct FromCtx<'a> {
    pub layer: Option<String>,
    pub color: Option<Value>,
    pub text_height: Option<f64>,
    pub block_exists: Option<&'a dyn Fn(&str) -> bool>,
    /// entmake (no old object): the current colour is given (`ctx.color ?? null`)
    pub has_color: bool,
}

/// Pairs → a CadCraft object. `old` (entmod) gives the fields not mentioned; without it (entmake) the type's required
/// codes must be there.
pub fn from_pairs(pairs: &Pairs, old: Option<&Obj>, ctx: &FromCtx) -> Result<Obj, String> {
    let old_type = old.map(dxf_type);
    let t0 = match get(pairs, 0) {
        Some(v) => v,
        None => old_type.clone().map_or(Pv::Str(String::new()), Pv::Str),
    };
    let ty = if js_truthy(&t0) { js_str(&t0).to_uppercase() } else { String::new() };
    if ty.is_empty() {
        return Err("no entity type".into());
    }
    if let Some(o) = old {
        let ot = dxf_type(o);
        let is_text = o.get("type").and_then(Value::as_str) == Some("text");
        if ty != ot && !(ty == "TEXT" && is_text) && !(ty == "MTEXT" && is_text) {
            return Err("entity type changed".into());
        }
    }
    let mut e: Obj = old.cloned().unwrap_or_default();
    // common
    if has(pairs, 8) {
        match get(pairs, 8) {
            Some(Pv::Str(l)) if !l.is_empty() => {
                e.insert("layer".into(), Value::String(l));
            }
            _ => return Err("bad layer".into()),
        }
    } else if old.is_none() {
        e.insert("layer".into(), Value::String(ctx.layer.clone().unwrap_or_else(|| "0".into())));
    }
    if has(pairs, 420) {
        let c = cc_lisp::value::to_i32(js_num(&get(pairs, 420).unwrap_or(Pv::Nil))) & 0x00ff_ffff;
        e.insert("color".into(), Value::String(format!("#{c:06x}")));
    } else if has(pairs, 62) {
        let c = js_num(&get(pairs, 62).unwrap_or(Pv::Nil));
        let v = if c == 256.0 {
            Value::Null
        } else if c.abs() == 0.0 {
            json!(0)
        } else {
            cc_cad::entity::num(js::min(255.0, c.abs()))
        };
        e.insert("color".into(), v);
    } else if old.is_none() {
        e.insert("color".into(), ctx.color.clone().unwrap_or(Value::Null));
    }
    if has(pairs, 6) {
        let lt = get(pairs, 6).unwrap_or(Pv::Nil);
        let s = js_str(&lt);
        if s.eq_ignore_ascii_case("bylayer") {
            e.remove("lt");
        } else if s.eq_ignore_ascii_case("continuous") {
            e.insert("lt".into(), Value::String("Continuous".into()));
        } else if s.eq_ignore_ascii_case("byblock") {
            e.insert("lt".into(), Value::String("ByBlock".into()));
        } else {
            e.insert("lt".into(), to_json(&lt));
        }
    }
    if has(pairs, 370) {
        let lw = js_num(&get(pairs, 370).unwrap_or(Pv::Nil));
        if lw == -1.0 {
            e.remove("lw");
        } else {
            set_num(&mut e, "lw", if lw < 0.0 { lw } else { lw / 100.0 });
        }
    }
    if has(pairs, 48) {
        let s = js_num(&get(pairs, 48).unwrap_or(Pv::Nil));
        if s > 0.0 && s != 1.0 {
            set_num(&mut e, "lts", s);
        } else {
            e.remove("lts");
        }
    }
    let need = |code: i32| -> Result<(), String> {
        if old.is_none() && !has(pairs, code) {
            return Err(format!("missing group {code}"));
        }
        Ok(())
    };
    let oldn = |k: &str| old.and_then(|o| o.get(k)).and_then(Value::as_f64);
    let oldp = |kx: &str, ky: &str| old.map(|o| (js::to_number_opt(o.get(kx)), js::to_number_opt(o.get(ky))));
    let nullref = || "Cannot read properties of null (reading 'x')".to_string();
    match ty.as_str() {
        "LINE" => {
            need(10)?;
            need(11)?;
            e.insert("type".into(), json!("line"));
            let a = pt2(pairs, 10).or_else(|| oldp("x1", "y1")).ok_or_else(nullref)?;
            let b = pt2(pairs, 11).or_else(|| oldp("x2", "y2")).ok_or_else(nullref)?;
            set_num(&mut e, "x1", a.0);
            set_num(&mut e, "y1", a.1);
            set_num(&mut e, "x2", b.0);
            set_num(&mut e, "y2", b.1);
        }
        "CIRCLE" | "ARC" => {
            need(10)?;
            need(40)?;
            if ty == "ARC" {
                need(50)?;
                need(51)?;
            }
            e.insert("type".into(), json!(if ty == "ARC" { "arc" } else { "circle" }));
            let c = pt2(pairs, 10).or_else(|| oldp("cx", "cy")).ok_or_else(nullref)?;
            set_num(&mut e, "cx", c.0);
            set_num(&mut e, "cy", c.1);
            let r = num(pairs, 40, oldn("r").unwrap_or(1.0));
            set_num(&mut e, "r", r);
            if ty == "ARC" {
                set_num(&mut e, "a0", na(num(pairs, 50, oldn("a0").unwrap_or(0.0))));
                set_num(&mut e, "a1", na(num(pairs, 51, oldn("a1").unwrap_or(0.0))));
            }
            if !(r > 0.0) {
                return Err("bad radius".into());
            }
        }
        "LWPOLYLINE" => {
            e.insert("type".into(), json!("poly"));
            if has(pairs, 10) {
                let mut pts: Vec<Obj> = Vec::new();
                for (c, v0) in pairs {
                    let v = val(v0);
                    match (c, &v) {
                        (10, Pv::List(xs)) => {
                            let x = xs.first().map_or(f64::NAN, js_num);
                            let y = xs.get(1).map_or(f64::NAN, js_num);
                            let mut p = Obj::new();
                            p.insert("x".into(), cc_cad::entity::num(x));
                            p.insert("y".into(), cc_cad::entity::num(y));
                            p.insert("b".into(), json!(0));
                            if !x.is_finite() || !y.is_finite() {
                                return Err("bad vertices".into());
                            }
                            pts.push(p);
                        }
                        (42, _) if !pts.is_empty() => {
                            let b = js_num(&v);
                            if let Some(p) = pts.last_mut() {
                                p.insert("b".into(), cc_cad::entity::num(if b.is_nan() { 0.0 } else { b }));
                            }
                        }
                        (40 | 41, _) if !pts.is_empty() && js_truthy(&v) => {
                            if let Some(p) = pts.last_mut() {
                                p.insert(if *c == 40 { "sw" } else { "ew" }.into(), cc_cad::entity::num(js_num(&v)));
                            }
                        }
                        _ => {}
                    }
                }
                if pts.len() < 2 {
                    return Err("bad vertices".into());
                }
                e.insert("pts".into(), Value::Array(pts.into_iter().map(Value::Object).collect()));
            } else {
                need(10)?;
            }
            if has(pairs, 70) {
                let f = cc_lisp::value::to_i32(js_num(&get(pairs, 70).unwrap_or(Pv::Nil)));
                e.insert("closed".into(), Value::Bool(f & 1 == 1));
            } else if old.is_none() {
                e.insert("closed".into(), Value::Bool(false));
            }
            if has(pairs, 43) {
                let w = js_num(&get(pairs, 43).unwrap_or(Pv::Nil));
                if w > 0.0 {
                    set_num(&mut e, "w", w);
                } else {
                    e.remove("w");
                }
            }
        }
        "ELLIPSE" => {
            need(10)?;
            need(11)?;
            need(40)?;
            e.insert("type".into(), json!("ellipse"));
            let c = pt2(pairs, 10).or_else(|| oldp("cx", "cy")).ok_or_else(nullref)?;
            let m = pt2(pairs, 11).or_else(|| oldp("mx", "my")).ok_or_else(nullref)?;
            set_num(&mut e, "cx", c.0);
            set_num(&mut e, "cy", c.1);
            set_num(&mut e, "mx", m.0);
            set_num(&mut e, "my", m.1);
            set_num(&mut e, "ratio", num(pairs, 40, oldn("ratio").unwrap_or(1.0)));
            set_num(&mut e, "t0", num(pairs, 41, oldn("t0").unwrap_or(0.0)));
            set_num(&mut e, "t1", num(pairs, 42, oldn("t1").unwrap_or(TAU)));
        }
        "POINT" => {
            need(10)?;
            e.insert("type".into(), json!("point"));
            let p = pt2(pairs, 10).or_else(|| oldp("x", "y")).ok_or_else(nullref)?;
            set_num(&mut e, "x", p.0);
            set_num(&mut e, "y", p.1);
        }
        "TEXT" => {
            need(1)?;
            need(10)?;
            e.insert("type".into(), json!("text"));
            let h72 = if has(pairs, 72) {
                js_num(&get(pairs, 72).unwrap_or(Pv::Nil))
            } else if let Some(o) = old {
                ha_code(o.get("ha").and_then(Value::as_str)).unwrap_or(0.0)
            } else {
                0.0
            };
            let v73 = if has(pairs, 73) {
                js_num(&get(pairs, 73).unwrap_or(Pv::Nil))
            } else if let Some(o) = old {
                va_code(o.get("va").and_then(Value::as_str)).unwrap_or(0.0)
            } else {
                0.0
            };
            let aligned = h72 != 0.0 || v73 != 0.0;
            let p =
                if aligned && has(pairs, 11) { pt2(pairs, 11) } else { pt2(pairs, 10).or_else(|| oldp("x", "y")) }.ok_or_else(nullref)?;
            set_num(&mut e, "x", p.0);
            set_num(&mut e, "y", p.1);
            let h = num(pairs, 40, oldn("h").or(ctx.text_height).unwrap_or(2.5));
            set_num(&mut e, "h", h);
            let s = if has(pairs, 1) {
                Value::String(js_str(&get(pairs, 1).unwrap_or(Pv::Nil)))
            } else {
                old.and_then(|o| o.get("s").cloned()).unwrap_or(Value::Null)
            };
            e.insert("s".into(), s);
            set_num(&mut e, "rot", na(num(pairs, 50, oldn("rot").unwrap_or(0.0))));
            set_num(&mut e, "wf", js::or(Some(num(pairs, 41, oldn("wf").unwrap_or(1.0))), 1.0));
            let has_ = ["left", "center", "right", "left", "center", "left"];
            let vas = ["baseline", "bottom", "middle", "top"];
            let idx =
                |x: f64, n: usize| -> Option<usize> { if x.fract() == 0.0 && x >= 0.0 { js_index(x).filter(|k| *k < n) } else { None } };
            e.insert("ha".into(), json!(idx(h72, 6).and_then(|k| has_.get(k)).copied().unwrap_or("left")));
            e.insert("va".into(), json!(idx(v73, 4).and_then(|k| vas.get(k)).copied().unwrap_or("baseline")));
            if h72 == 4.0 {
                e.insert("va".into(), json!("middle"));
            }
            if has(pairs, 51) {
                let o = get(pairs, 51).unwrap_or(Pv::Nil);
                if js_truthy(&o) {
                    e.insert("obl".into(), to_json(&o));
                } else {
                    e.remove("obl");
                }
            }
            if !(h > 0.0) {
                return Err("bad text height".into());
            }
        }
        "MTEXT" => {
            need(1)?;
            need(10)?;
            e.insert("type".into(), json!("text"));
            let p = pt2(pairs, 10).or_else(|| oldp("x", "y")).ok_or_else(nullref)?;
            let raw = if has(pairs, 1) {
                let pre: String = pairs.iter().filter(|(c, _)| *c == 3).map(|(_, v)| js_str(&val(v))).collect();
                pre + &js_str(&get(pairs, 1).unwrap_or(Pv::Nil))
            } else {
                old.and_then(|o| o.get("s")).map(js::to_js_string).unwrap_or_else(|| "undefined".into())
            };
            let s = strip_mtext(&raw.replace("\\P", "\n"));
            let att = if has(pairs, 71) {
                js_num(&get(pairs, 71).unwrap_or(Pv::Nil))
            } else if let Some(o) = old {
                attach(o.get("ha").and_then(Value::as_str), o.get("va").and_then(Value::as_str))
            } else {
                1.0
            };
            set_num(&mut e, "x", p.0);
            set_num(&mut e, "y", p.1);
            let h = num(pairs, 40, oldn("h").or(ctx.text_height).unwrap_or(2.5));
            set_num(&mut e, "h", h);
            e.insert("s".into(), Value::String(s));
            let rot = if has(pairs, 11) {
                let q = pt2(pairs, 11).ok_or_else(nullref)?;
                q.1.jatan2(q.0)
            } else {
                num(pairs, 50, oldn("rot").unwrap_or(0.0))
            };
            set_num(&mut e, "rot", na(rot));
            let m3 = (att - 1.0) % 3.0;
            let d3 = ((att - 1.0) / 3.0).floor();
            let ha = ["left", "center", "right"];
            let va = ["top", "middle", "bottom"];
            let pick = |x: f64, list: &[&'static str]| -> &'static str {
                if x.fract() == 0.0 && x >= 0.0 { js_index(x).and_then(|k| list.get(k)).copied().unwrap_or("") } else { "" }
            };
            let h_ = pick(m3, &ha);
            let v_ = pick(d3, &va);
            e.insert("ha".into(), json!(if h_.is_empty() { "left" } else { h_ }));
            e.insert("va".into(), json!(if v_.is_empty() { "top" } else { v_ }));
            let ww = num(pairs, 41, oldn("ww").unwrap_or(0.0));
            if ww > 0.0 {
                set_num(&mut e, "ww", ww);
            } else {
                e.remove("ww");
            }
            if !(h > 0.0) {
                return Err("bad text height".into());
            }
        }
        "INSERT" => {
            need(2)?;
            need(10)?;
            e.insert("type".into(), json!("insert"));
            let p = pt2(pairs, 10).or_else(|| oldp("x", "y")).ok_or_else(nullref)?;
            let name = if has(pairs, 2) {
                js_str(&get(pairs, 2).unwrap_or(Pv::Nil))
            } else {
                old.and_then(|o| o.get("block")).map(js::to_js_string).unwrap_or_else(|| "undefined".into())
            };
            if let Some(f) = ctx.block_exists
                && !f(&name)
            {
                return Err(format!("no block {name}"));
            }
            e.insert("block".into(), Value::String(name));
            set_num(&mut e, "x", p.0);
            set_num(&mut e, "y", p.1);
            set_num(&mut e, "sx", js::or(Some(num(pairs, 41, oldn("sx").unwrap_or(1.0))), 1.0));
            let sy_d = oldn("sy").unwrap_or(if has(pairs, 41) { num(pairs, 41, 1.0) } else { 1.0 });
            set_num(&mut e, "sy", js::or(Some(num(pairs, 42, sy_d)), 1.0));
            set_num(&mut e, "rot", na(num(pairs, 50, oldn("rot").unwrap_or(0.0))));
        }
        "HATCH" => {
            if old.is_some() {
                if has(pairs, 2) && !truthy(&e, "solid") {
                    e.insert("pattern".into(), Value::String(js_str(&get(pairs, 2).unwrap_or(Pv::Nil))));
                }
                if has(pairs, 52) {
                    set_num(&mut e, "pangle", num(pairs, 52, 0.0));
                }
                if has(pairs, 41) {
                    set_num(&mut e, "pscale", num(pairs, 41, 1.0));
                }
            } else {
                let mut loops: Vec<Vec<f64>> = Vec::new();
                for (c, v0) in pairs {
                    match (c, val(v0)) {
                        (93, _) => loops.push(Vec::new()),
                        (10, Pv::List(xs)) => {
                            if let Some(cur) = loops.last_mut() {
                                cur.push(xs.first().map_or(f64::NAN, js_num));
                                cur.push(xs.get(1).map_or(f64::NAN, js_num));
                            }
                        }
                        _ => {}
                    }
                }
                if loops.is_empty() || loops.iter().any(|l| l.len() < 6) {
                    return Err("hatch boundary not given".into());
                }
                let p2 = get(pairs, 2).unwrap_or(Pv::Str("SOLID".into()));
                let solid = js_str(&p2).to_uppercase() == "SOLID" || matches!(get(pairs, 70), Some(Pv::Num(n)) if n == 1.0);
                e.insert("type".into(), json!("hatch"));
                e.insert("solid".into(), Value::Bool(solid));
                e.insert(
                    "loops".into(),
                    Value::Array(loops.into_iter().map(|l| Value::Array(l.into_iter().map(cc_cad::entity::num).collect())).collect()),
                );
                if !solid {
                    e.insert("pattern".into(), Value::String(js_str(&get(pairs, 2).unwrap_or(Pv::Nil))));
                    set_num(&mut e, "pscale", num(pairs, 41, 1.0));
                    set_num(&mut e, "pangle", num(pairs, 52, 0.0));
                }
            }
        }
        "VIEWPORT" => {
            if old.is_none() {
                return Err("make viewports with MVIEW".into());
            }
            if let Some(c) = pt2(pairs, 10) {
                set_num(&mut e, "x", c.0);
                set_num(&mut e, "y", c.1);
            }
            if let Some(v) = pt2(pairs, 12) {
                set_num(&mut e, "cx", v.0);
                set_num(&mut e, "cy", v.1);
            }
            if has(pairs, 40) {
                let w = num(pairs, 40, js::to_number_opt(e.get("w")));
                set_num(&mut e, "w", w);
            }
            if has(pairs, 41) {
                let h = num(pairs, 41, js::to_number_opt(e.get("h")));
                set_num(&mut e, "h", h);
            }
            if has(pairs, 45) && num(pairs, 45, 0.0) > 0.0 {
                let h = js::to_number_opt(e.get("h"));
                set_num(&mut e, "s", h / num(pairs, 45, 0.0));
            }
        }
        "DIMENSION" => {
            if old.is_none() {
                return Err("make dimensions with the DIM commands".into());
            }
            if has(pairs, 1) {
                e.insert("text".into(), Value::String(js_str(&get(pairs, 1).unwrap_or(Pv::Nil))));
                e.remove("block");
            }
            for (code, k) in [(10, "loc"), (13, "p1"), (14, "p2"), (15, "c")] {
                if let Some(p) = pt2(pairs, code)
                    && e.get(k).is_some_and(|v| js::truthy(Some(v)))
                {
                    e.insert(k.into(), json!({ "x": cc_cad::entity::num(p.0), "y": cc_cad::entity::num(p.1) }));
                    e.remove("block");
                }
            }
        }
        _ => {
            if old.is_none() {
                return Err(format!("cannot make {ty}"));
            }
        }
    }
    Ok(e)
}

/// MTEXT's text: `\X…;` codes and braces dropped.
fn strip_mtext(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;
    while let Some(&ch) = c.get(i) {
        if ch == '\\'
            && c.get(i.saturating_add(1)).is_some_and(char::is_ascii_alphabetic)
            && let Some(end) = (i.saturating_add(2)..c.len()).find(|&k| c.get(k) == Some(&';'))
        {
            i = end.saturating_add(1);
            continue;
        }
        if ch != '{' && ch != '}' {
            out.push(ch);
        }
        i = i.saturating_add(1);
    }
    out
}

// ---- ssget filters ----

fn rel(op: &str, a: &Pv, b: &Pv) -> Option<bool> {
    let lt = |x: &Pv, y: &Pv| -> Option<bool> {
        match (x, y) {
            (Pv::Str(p), Pv::Str(q)) => Some(p.encode_utf16().collect::<Vec<u16>>() < q.encode_utf16().collect::<Vec<u16>>()),
            _ => {
                let (p, q) = (js_num(x), js_num(y));
                if p.is_nan() || q.is_nan() { None } else { Some(p < q) }
            }
        }
    };
    let eqv = |x: &Pv, y: &Pv| -> bool {
        match (x, y) {
            (Pv::Str(p), Pv::Str(q)) => p == q,
            (Pv::Num(p), Pv::Num(q)) => p == q,
            _ => false,
        }
    };
    Some(match op {
        "=" => eqv(a, b),
        "!=" | "/=" | "<>" => !eqv(a, b),
        "<" => lt(a, b) == Some(true),
        "<=" => lt(b, a) == Some(false),
        ">" => lt(b, a) == Some(true),
        ">=" => lt(a, b) == Some(false),
        "*" => true,
        _ => return None,
    })
}
fn test_one(pairs: &Pairs, code: i32, want: &Pv, op: Option<&str>) -> Result<bool, cc_lisp::LErr> {
    let mut have: Vec<Pv> = pairs.iter().filter(|(c, _)| *c == code).map(|(_, v)| val(v)).collect();
    if have.is_empty() {
        match code {
            62 => have.push(Pv::Num(256.0)),
            8 => have.push(Pv::Str("0".into())),
            6 => have.push(Pv::Str("BYLAYER".into())),
            67 => have.push(Pv::Num(0.0)),
            _ => {}
        }
    }
    let w = val(want);
    for h in &have {
        let hit = match (&w, h) {
            (Pv::Str(ws), Pv::Str(hs)) => match op {
                Some(o) => rel(o, &Pv::Str(hs.to_uppercase()), &Pv::Str(ws.to_uppercase())).unwrap_or(false),
                None => cc_lisp::wild::wcmatch(
                    &cc_lisp::value::upper16(&cc_lisp::value::lstr(hs)),
                    &cc_lisp::value::upper16(&cc_lisp::value::lstr(ws)),
                )?,
            },
            (Pv::Num(_), Pv::Num(_)) => rel(op.unwrap_or("="), h, &w).unwrap_or_else(|| rel("=", h, &w).unwrap_or(false)),
            (Pv::List(ws), Pv::List(hs)) => {
                let ops: Vec<String> = op.unwrap_or("=").split(',').map(|s| js::trim(s).to_string()).collect();
                ws.iter().enumerate().all(|(k, x)| {
                    let o = ops.get(k.min(ops.len().saturating_sub(1))).map_or("=", String::as_str);
                    let hv = hs.get(k).cloned().unwrap_or(Pv::Num(0.0));
                    rel(o, &val(&hv), &val(x)).unwrap_or_else(|| rel("=", &val(&hv), &val(x)).unwrap_or(false))
                })
            }
            _ => false,
        };
        if hit {
            return Ok(true);
        }
    }
    Ok(false)
}
/// Does an object (its pairs) pass an ssget filter list (-4 "<AND" … groups and relational tests)?
pub fn match_filter(pairs: &Pairs, filter: &Pairs) -> Result<bool, cc_lisp::LErr> {
    let mut i = 0usize;
    let r = seq(pairs, filter, &mut i, None, 0)?;
    Ok(r.iter().all(|&b| b))
}
fn seq(pairs: &Pairs, filter: &Pairs, i: &mut usize, end: Option<&str>, depth: u32) -> Result<Vec<bool>, cc_lisp::LErr> {
    let mut res = Vec::new();
    if depth > 1000 {
        return Ok(res);
    }
    while let Some((code, v)) = filter.get(*i) {
        if *code == -4 {
            let op = js::trim(&js_str(&val(v)).to_uppercase()).to_string();
            if end == Some(op.as_str()) {
                *i = i.saturating_add(1);
                return Ok(res);
            }
            if matches!(op.as_str(), "<AND" | "<OR" | "<XOR" | "<NOT") {
                *i = i.saturating_add(1);
                let kind = op.get(1..).unwrap_or("").to_string();
                let inner = seq(pairs, filter, i, Some(&format!("{kind}>")), depth.saturating_add(1))?;
                res.push(match kind.as_str() {
                    "AND" => inner.iter().all(|&b| b),
                    "OR" => inner.iter().any(|&b| b),
                    "XOR" => inner.iter().filter(|&&b| b).count() == 1,
                    _ => !inner.iter().all(|&b| b),
                });
                continue;
            }
            *i = i.saturating_add(1);
            if let Some((c2, v2)) = filter.get(*i) {
                *i = i.saturating_add(1);
                res.push(test_one(pairs, *c2, v2, Some(&op))?);
            }
            continue;
        }
        *i = i.saturating_add(1);
        res.push(test_one(pairs, *code, v, None)?);
    }
    Ok(res)
}

// ---- table records ----

pub fn layer_pairs(l: &Layer) -> Pairs {
    let v = l.to_value();
    let color = v.get("color").and_then(Value::as_f64).unwrap_or(7.0);
    let sign = if !l.on { -1.0 } else { 1.0 };
    let lw = l.lineweight.filter(|w| w.is_finite() && *w >= 0.0).map_or(-3.0, |w| js::round(w * 100.0));
    vec![
        (0, Pv::Str("LAYER".into())),
        (2, Pv::Str(l.name.clone())),
        (70, Pv::Int(i32::from(l.frozen) | if l.locked { 4 } else { 0 })),
        (62, i(sign * color)),
        (6, Pv::Str(l.linetype.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "Continuous".into()))),
        (290, Pv::Int(i32::from(l.plot))),
        (370, i(lw)),
    ]
}
pub fn block_pairs(b: &cc_cad::Block, first: Option<EId>) -> Pairs {
    let xref = b.xref.as_ref().filter(|x| x.is_object());
    let flags = i32::from(b.name.starts_with('*'))
        | if b.attdefs.as_ref().is_some_and(|a| !a.is_empty()) { 2 } else { 0 }
        | if xref.is_some() { 4 } else { 0 }
        | if xref.is_some_and(|x| js::truthy(x.get("of"))) { 16 } else { 0 };
    let mut out = vec![
        (0, Pv::Str("BLOCK".into())),
        (2, Pv::Str(b.name.clone())),
        (70, Pv::Int(flags)),
        (10, Pv::List(vec![Pv::Num(b.base.x), Pv::Num(b.base.y), Pv::Num(0.0)])),
    ];
    if let Some(f) = first {
        out.push((-2, Pv::E(f)));
    }
    if let Some(p) = xref.and_then(|x| x.get("path")).filter(|p| js::truthy(Some(p))) {
        out.push((1, num_pv(Some(p))));
    }
    out
}
pub fn ltype_pairs(name: &str, description: &str, pattern: &[f64]) -> Pairs {
    let mut out = vec![
        (0, Pv::Str("LTYPE".into())),
        (2, Pv::Str(name.into())),
        (70, Pv::Int(0)),
        (3, Pv::Str(description.into())),
        (72, Pv::Int(65)),
        (73, i(pattern.len() as f64)),
        (40, Pv::Num(pattern.iter().fold(0.0, |s, x| s + x.abs()))),
    ];
    out.extend(pattern.iter().map(|x| (49, Pv::Num(*x))));
    out
}
pub fn view_pairs(v: &cc_cad::View) -> Pairs {
    vec![
        (0, Pv::Str("VIEW".into())),
        (2, Pv::Str(v.name.clone())),
        (70, Pv::Int(i32::from(v.layout.as_ref().is_some_and(|l| !l.is_empty())))),
        (40, Pv::Num(v.h)),
        (10, Pv::List(vec![Pv::Num(v.cx), Pv::Num(v.cy)])),
        (41, Pv::Num(v.w)),
        (11, Pv::List(vec![Pv::Num(0.0), Pv::Num(0.0), Pv::Num(1.0)])),
        (12, Pv::List(vec![Pv::Num(0.0), Pv::Num(0.0), Pv::Num(0.0)])),
    ]
}
pub fn name_pairs(table: &str, name: &str) -> Pairs {
    vec![(0, Pv::Str(table.into())), (2, Pv::Str(name.into())), (70, Pv::Int(0))]
}

/// A whole, non-negative number as an index (None past usize).
fn js_index(x: f64) -> Option<usize> {
    if x.fract() != 0.0 || !(0.0..9.0e15).contains(&x) {
        return None;
    }
    format!("{x:.0}").parse().ok()
}
