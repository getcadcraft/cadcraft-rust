//! ifc-import.js — an IFC2X3 / IFC4 file read with cc-ifc and turned into a DetailCraft model: steel parts whose body
//! is one extruded solid of a profile DetailCraft can draw (catalog match within 0.5 mm, else a custom profile),
//! storeys → levels, grid axes parallel to X / Y → grids, round openings → holes, full-width end rectangles → copes,
//! fasteners → loose bolts; everything else counted by class with the reason. The lines are read the way the JS reads
//! them through cc-ifc's IfcAPI (`GetLine(mid, id, false)`), here through cc-bim's line view of cc-io-ifc.

use cc_bim::api::{AV, Line, Prim, get_line};
use cc_detail::geom::{V3, add, cross, dot, frame, len, mul, norm, sub, to_local, voff_for};
use cc_detail::i18n::{F, t, t0};
use cc_detail::js::{atan2, clean_str, locale_compare, max_of, min_of, num_to_string, round, str_to_number, to_js_string};
use cc_detail::model::{Model, empty_state};
use cc_detail::profiles::{catalog_get, catalog_has, catalog_rows, clean_custom, get_profile_str, plate_name};
use cc_io_ifc::model::{IfcFile, id_key};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::f64::consts::PI;

const MAX_MEMBERS: usize = 20000;
const TOL: f64 = 0.5;

/// A 4×4 affine as columns X, Y, Z, O (and the scale of a transformation operator).
#[derive(Clone, Copy, Debug)]
struct M4 {
    x: V3,
    y: V3,
    z: V3,
    o: V3,
    scale: Option<f64>,
}
fn i4() -> M4 {
    M4 { x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], z: [0.0, 0.0, 1.0], o: [0.0, 0.0, 0.0], scale: None }
}
fn or0(v: f64) -> f64 {
    if v == 0.0 || v.is_nan() { 0.0 } else { v }
}
fn apply(m: &M4, p: V3) -> V3 {
    add(add(add(m.o, mul(m.x, p[0])), mul(m.y, p[1])), mul(m.z, or0(p[2])))
}
fn apply_dir(m: &M4, d: V3) -> V3 {
    add(add(mul(m.x, d[0]), mul(m.y, d[1])), mul(m.z, or0(d[2])))
}
fn compose(a: &M4, b: &M4) -> M4 {
    M4 { x: apply_dir(a, b.x), y: apply_dir(a, b.y), z: apply_dir(a, b.z), o: apply(a, b.o), scale: None }
}
fn round3(v: f64) -> f64 {
    round(v * 1000.0) / 1000.0 + 0.0
}
fn r1(v: f64) -> String {
    num_to_string(round(v * 10.0) / 10.0)
}

/// `x?.value` as the JS sees an attribute: undefined, null, a primitive, or the attribute itself (a list).
#[derive(Clone, Debug)]
enum JV {
    Undef,
    Null,
    P(Prim),
    List(usize),
}
fn val(a: &Option<AV>) -> JV {
    match a {
        None => JV::Undef,
        Some(AV::Null) => JV::Null,
        Some(AV::Derived) => JV::Null,
        Some(AV::Handle(n)) => JV::P(Prim::Num(*n)),
        Some(AV::Val(p)) => JV::P(p.clone()),
        Some(AV::List(l)) => JV::List(l.len()),
    }
}
fn prim_truthy(p: &Prim) -> bool {
    match p {
        Prim::Undef | Prim::Null => false,
        Prim::Bool(b) => *b,
        Prim::Num(n) => *n != 0.0 && !n.is_nan(),
        Prim::Str(s) => !s.is_empty(),
        Prim::Arr(_) | Prim::Obj => true,
    }
}
fn prim_num(p: &Prim) -> f64 {
    match p {
        Prim::Undef => f64::NAN,
        Prim::Null => 0.0,
        Prim::Bool(b) => f64::from(u8::from(*b)),
        Prim::Num(n) => *n,
        Prim::Str(s) => str_to_number(s),
        Prim::Arr(a) => match a.as_slice() {
            [] => 0.0,
            [x] => str_to_number(&x.to_js_string()),
            _ => f64::NAN,
        },
        Prim::Obj => f64::NAN,
    }
}
/// `Number(val(x))`.
fn num(a: &Option<AV>) -> f64 {
    match val(a) {
        JV::Undef => f64::NAN,
        JV::Null => 0.0,
        JV::P(p) => prim_num(&p),
        JV::List(n) => {
            if n == 0 {
                0.0
            } else {
                f64::NAN
            }
        }
    }
}
/// `String(val(x) || '')` (the text the JS hands to cleanText).
fn text(a: &Option<AV>) -> String {
    match val(a) {
        JV::P(p) if prim_truthy(&p) => p.to_js_string(),
        JV::List(n) => vec!["[object Object]"; n].join(","),
        _ => String::new(),
    }
}
/// `ref(x)`: a handle's id.
fn rf(a: &Option<AV>) -> Option<f64> {
    match a {
        Some(AV::Handle(n)) => Some(*n),
        _ => None,
    }
}
fn truthy(a: &Option<AV>) -> bool {
    matches!(a, Some(x) if !matches!(x, AV::Null))
}
fn items(a: &Option<AV>) -> Result<Vec<AV>, String> {
    match a {
        None | Some(AV::Null) => Ok(vec![]),
        Some(AV::List(l)) => Ok(l.clone()),
        Some(_) => Err("TypeError: object is not iterable".into()),
    }
}
fn cls(l: Option<&Line>) -> String {
    match l {
        Some(l) if l.class().starts_with("Ifc") => l.class().to_string(),
        _ => "IfcEntity".into(),
    }
}

struct Imp<'f, 'a> {
    f: &'f IfcFile<'a>,
    to_mm: f64,
    warnings: Vec<String>,
    place_cache: HashMap<u64, M4>,
}

type R<T> = Result<T, String>;

impl<'f, 'a> Imp<'f, 'a> {
    fn line(&self, id: Option<f64>) -> R<Option<Line<'f, 'a>>> {
        let Some(id) = id else { return Ok(None) };
        get_line(self.f, id).map_err(|e| e.0)
    }
    fn ids(&self, up: &str) -> Vec<f64> {
        self.f.of_type(up, false)
    }
    fn coords(&self, id: Option<f64>) -> R<V3> {
        let p = self.line(id)?;
        let c: Vec<f64> = match p.and_then(|p| p.get("Coordinates")) {
            Some(AV::List(l)) => l.iter().map(|v| num(&Some(v.clone()))).collect(),
            _ => vec![],
        };
        let g = |i: usize| or0(c.get(i).copied().unwrap_or(f64::NAN));
        Ok([g(0), g(1), g(2)])
    }
    fn dir_of(&self, id: Option<f64>) -> R<V3> {
        let p = self.line(id)?;
        let c: Vec<f64> = match p.and_then(|p| p.get("DirectionRatios")) {
            Some(AV::List(l)) => l.iter().map(|v| num(&Some(v.clone()))).collect(),
            _ => vec![],
        };
        let g = |i: usize| or0(c.get(i).copied().unwrap_or(f64::NAN));
        Ok([g(0), g(1), g(2)])
    }
    fn axis3(&self, id: Option<f64>) -> R<M4> {
        let Some(a) = self.line(id)? else { return Ok(i4()) };
        let o = self.coords(rf(&a.get("Location")))?;
        if cls(Some(&a)) == "IfcAxis2Placement2D" {
            let x = if truthy(&a.get("RefDirection")) { norm(self.dir_of(rf(&a.get("RefDirection")))?) } else { [1.0, 0.0, 0.0] };
            return Ok(M4 { x, y: [-x[1], x[0], 0.0], z: [0.0, 0.0, 1.0], o, scale: None });
        }
        let z = if truthy(&a.get("Axis")) { norm(self.dir_of(rf(&a.get("Axis")))?) } else { [0.0, 0.0, 1.0] };
        let mut x = if truthy(&a.get("RefDirection")) {
            self.dir_of(rf(&a.get("RefDirection")))?
        } else if z[0].abs() > 0.9 {
            [0.0, 0.0, 1.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        x = norm(sub(x, mul(z, dot(x, z))));
        if len(x) < 0.5 {
            x = norm(cross(if z[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [0.0, 1.0, 0.0] }, z));
        }
        Ok(M4 { x, y: cross(z, x), z, o, scale: None })
    }
    fn placement(&mut self, id: Option<f64>, depth: u32) -> R<M4> {
        let Some(idv) = id.filter(|v| *v != 0.0 && !v.is_nan()) else { return Ok(i4()) };
        if depth > 50 {
            return Ok(i4());
        }
        if let Some(m) = self.place_cache.get(&id_key(idv)) {
            return Ok(*m);
        }
        let lp = self.line(Some(idv))?;
        let mut m = i4();
        match lp {
            Some(lp) if cls(Some(&lp)) == "IfcLocalPlacement" => {
                let rel = rf(&lp.get("RelativePlacement"));
                let local = if rel.is_some() { self.axis3(rel)? } else { i4() };
                let parent = rf(&lp.get("PlacementRelTo"));
                m = if parent.is_some() {
                    let pm = self.placement(parent, depth.saturating_add(1))?;
                    compose(&pm, &local)
                } else {
                    local
                };
            }
            Some(lp) => {
                let c = cls(Some(&lp));
                self.warnings.push(t("ifcimp.warn.placement", &[("cls", F::from(c))]));
            }
            None => {}
        }
        self.place_cache.insert(id_key(idv), m);
        Ok(m)
    }
    fn cart_op(&self, id: Option<f64>) -> R<M4> {
        let Some(op) = self.line(id)? else { return Ok(i4()) };
        let s = match op.get("Scale") {
            None | Some(AV::Null) => 1.0,
            v => num(&v),
        };
        let x = if truthy(&op.get("Axis1")) { norm(self.dir_of(rf(&op.get("Axis1")))?) } else { [1.0, 0.0, 0.0] };
        let mut z = if truthy(&op.get("Axis3")) { norm(self.dir_of(rf(&op.get("Axis3")))?) } else { [0.0, 0.0, 1.0] };
        let y = if truthy(&op.get("Axis2")) { norm(self.dir_of(rf(&op.get("Axis2")))?) } else { norm(cross(z, x)) };
        if !truthy(&op.get("Axis3")) {
            z = cross(x, y);
        }
        Ok(M4 { x: mul(x, s), y: mul(y, s), z: mul(z, s), o: self.coords(rf(&op.get("LocalOrigin")))?, scale: Some(s) })
    }
}

/// One extruded solid of an element (through mapped items and boolean results), or why not.
struct Body<'f, 'a> {
    solid: Line<'f, 'a>,
    m: M4,
    clipped: bool,
}
enum Found<'f, 'a> {
    Solid(Line<'f, 'a>, M4, bool),
    Bad(Option<String>),
}

fn walk<'f, 'a>(imp: &Imp<'f, 'a>, id: Option<f64>, m: M4, d: u32, found: &mut Vec<Found<'f, 'a>>, clipped: &mut bool) -> R<()> {
    let Some(it) = imp.line(id)? else { return Ok(()) };
    if d > 10 {
        return Ok(());
    }
    let c = cls(Some(&it));
    if c == "IfcExtrudedAreaSolid" || c == "IfcExtrudedAreaSolidTapered" {
        found.push(Found::Solid(it, m, c != "IfcExtrudedAreaSolid"));
    } else if c == "IfcBooleanClippingResult" || c == "IfcBooleanResult" {
        *clipped = true;
        walk(imp, rf(&it.get("FirstOperand")), m, d.saturating_add(1), found, clipped)?;
    } else if c == "IfcMappedItem" {
        let src = imp.line(rf(&it.get("MappingSource")))?;
        let origin = match &src {
            Some(s) => imp.axis3(rf(&s.get("MappingOrigin")))?,
            None => i4(),
        };
        let target = imp.cart_op(rf(&it.get("MappingTarget")))?;
        if let Some(s) = target.scale
            && (s - 1.0).abs() > 1e-9
        {
            found.push(Found::Bad(None));
            return Ok(());
        }
        let rp = match &src {
            Some(s) => imp.line(rf(&s.get("MappedRepresentation")))?,
            None => None,
        };
        let subs = match &rp {
            Some(r) => items(&r.get("Items"))?,
            None => vec![],
        };
        let mm = compose(&compose(&m, &target), &origin);
        for sb in subs {
            walk(imp, rf(&Some(sb)), mm, d.saturating_add(1), found, clipped)?;
        }
    } else {
        found.push(Found::Bad(Some(c)));
    }
    Ok(())
}

fn re_test_ci(s: &str, words: &[&str]) -> bool {
    let l = s.to_lowercase();
    words.iter().any(|w| l.contains(w))
}

fn body_solid<'f, 'a>(imp: &Imp<'f, 'a>, el: &Line<'f, 'a>) -> R<Result<Body<'f, 'a>, String>> {
    let Some(pds) = imp.line(rf(&el.get("Representation")))? else { return Ok(Err(t0("ifcimp.why.no-shape"))) };
    let mut reps: Vec<Line> = Vec::new();
    for r in items(&pds.get("Representations"))? {
        if let Some(l) = imp.line(rf(&Some(r)))? {
            reps.push(l);
        }
    }
    let rep = reps
        .iter()
        .find(|r| text(&r.get("RepresentationIdentifier")).eq_ignore_ascii_case("body"))
        .or_else(|| {
            reps.iter().find(|r| re_test_ci(&text(&r.get("RepresentationType")), &["swept", "solid", "brep", "clipping", "csg", "mapped"]))
        })
        .or_else(|| reps.first());
    let Some(rep) = rep else { return Ok(Err(t0("ifcimp.why.no-body"))) };
    let mut found = Vec::new();
    let mut clipped = false;
    for it in items(&rep.get("Items"))? {
        walk(imp, rf(&Some(it)), i4(), 0, &mut found, &mut clipped)?;
    }
    if let Some(Found::Bad(b)) = found.iter().find(|f| matches!(f, Found::Bad(_))) {
        return Ok(Err(match b {
            None => t0("ifcimp.why.scaled"),
            Some(c) => t("ifcimp.why.not-extrusion", &[("cls", F::from(c.as_str()))]),
        }));
    }
    if found.len() != 1 {
        return Ok(Err(if found.is_empty() {
            t0("ifcimp.why.no-extrusion")
        } else {
            t("ifcimp.why.solids", &[("n", F::N(found.len() as f64))])
        }));
    }
    match found.pop() {
        Some(Found::Solid(s, m, tapered)) => {
            if tapered {
                return Ok(Err(t0("ifcimp.why.tapered")));
            }
            Ok(Ok(Body { solid: s, m, clipped }))
        }
        _ => Ok(Err(t0("ifcimp.why.no-extrusion"))),
    }
}

/// A profile read from the file.
struct Prof {
    kind: &'static str,
    pname: String,
    pos: M4,
    dims: Vec<(&'static str, f64)>,
}
impl Prof {
    fn d(&self, k: &str) -> f64 {
        self.dims.iter().find(|(n, _)| *n == k).map_or(f64::NAN, |(_, v)| *v)
    }
}

fn rectangle_of(imp: &Imp, id: Option<f64>) -> R<Option<(V3, V3, V3, f64, f64)>> {
    let Some(cv) = imp.line(id)? else { return Ok(None) };
    let c = cls(Some(&cv));
    let mut pts: Vec<V3> = Vec::new();
    if c == "IfcPolyline" {
        for p in items(&cv.get("Points"))? {
            pts.push(imp.coords(rf(&Some(p)))?);
        }
    } else if c == "IfcIndexedPolyCurve" && !truthy(&cv.get("Segments")) {
        let list = imp.line(rf(&cv.get("Points")))?;
        if let Some(l) = list {
            for p in items(&l.get("CoordList"))? {
                let mut v: Vec<f64> = match p {
                    AV::List(q) => q.iter().map(|x| num(&Some(x.clone()))).collect(),
                    _ => return Err("TypeError: p.map is not a function".into()),
                };
                v.push(0.0);
                let g = |i: usize| v.get(i).copied().unwrap_or(f64::NAN);
                pts.push([g(0), g(1), g(2)]);
            }
        }
    } else {
        return Ok(None);
    }
    if pts.len() > 1
        && let (Some(a), Some(b)) = (pts.first(), pts.last())
        && len(sub(*a, *b)) < 1e-6
    {
        pts.pop();
    }
    let [p0, p1, p2, p3] = pts.as_slice() else { return Ok(None) };
    let ps = [*p0, *p1, *p2, *p3];
    let e: Vec<V3> = [sub(ps[1], ps[0]), sub(ps[2], ps[1]), sub(ps[3], ps[2]), sub(ps[0], ps[3])].to_vec();
    for i in 0..4 {
        let (a, b) = (e.get(i).copied().unwrap_or([0.0; 3]), e.get(i.wrapping_add(1) % 4).copied().unwrap_or([0.0; 3]));
        if dot(norm(a), norm(b)).abs() > 1e-4 {
            return Ok(None);
        }
    }
    let c = mul(ps.iter().fold([0.0, 0.0, 0.0], |s, p| add(s, *p)), 0.25);
    let e0 = e.first().copied().unwrap_or([0.0; 3]);
    let e1 = e.get(1).copied().unwrap_or([0.0; 3]);
    Ok(Some((c, norm(e0), norm(cross([0.0, 0.0, 1.0], norm(e0))), len(e0) * imp.to_mm, len(e1) * imp.to_mm)))
}

fn read_profile(imp: &Imp, pd: &Line) -> R<Result<Prof, String>> {
    let c = cls(Some(pd));
    let pname = clean_str(&text(&pd.get("ProfileName")), 40);
    let pos = if truthy(&pd.get("Position")) { imp.axis3(rf(&pd.get("Position")))? } else { i4() };
    let n = |k: &str| num(&pd.get(k)) * imp.to_mm;
    Ok(Ok(match c.as_str() {
        "IfcIShapeProfileDef" => Prof {
            kind: "I",
            pname,
            pos,
            dims: vec![
                ("d", n("OverallDepth")),
                ("bf", n("OverallWidth")),
                ("tw", n("WebThickness")),
                ("tf", n("FlangeThickness")),
                ("r", if truthy(&pd.get("FilletRadius")) { n("FilletRadius") } else { 0.0 }),
            ],
        },
        "IfcUShapeProfileDef" => Prof {
            kind: "C",
            pname,
            pos,
            dims: vec![("d", n("Depth")), ("bf", n("FlangeWidth")), ("tw", n("WebThickness")), ("tf", n("FlangeThickness"))],
        },
        "IfcLShapeProfileDef" => {
            let d = n("Depth");
            let w = if truthy(&pd.get("Width")) { n("Width") } else { d };
            if (d - w).abs() > TOL {
                return Ok(Err(t("ifcimp.why.unequal-angle", &[("w", F::from(r1(w))), ("d", F::from(r1(d)))])));
            }
            Prof { kind: "L", pname, pos, dims: vec![("b", d), ("t", n("Thickness"))] }
        }
        "IfcRectangleHollowProfileDef" => {
            Prof { kind: "RHS", pname, pos, dims: vec![("h", n("YDim")), ("b", n("XDim")), ("t", n("WallThickness"))] }
        }
        "IfcCircleHollowProfileDef" => Prof { kind: "CHS", pname, pos, dims: vec![("D", 2.0 * n("Radius")), ("t", n("WallThickness"))] },
        "IfcRectangleProfileDef" => Prof { kind: "RECT", pname, pos, dims: vec![("x", n("XDim")), ("y", n("YDim"))] },
        "IfcArbitraryClosedProfileDef" => {
            let Some((cc, x, y, w, h)) = rectangle_of(imp, rf(&pd.get("OuterCurve")))? else { return Ok(Err(t0("ifcimp.why.not-rect"))) };
            Prof { kind: "RECT", pname, pos: M4 { x, y, z: [0.0, 0.0, 1.0], o: cc, scale: None }, dims: vec![("x", w), ("y", h)] }
        }
        _ => return Ok(Err(t("ifcimp.why.profile-kind", &[("cls", F::from(c.as_str()))]))),
    }))
}

fn close(a: Option<f64>, b: f64) -> bool {
    a.is_some_and(|a| (a - b).abs() <= TOL)
}
fn catalog_match(kind: &str, d: &Prof, pname: &str) -> Option<String> {
    let fit = |p: &cc_detail::sections::Shape| -> bool {
        match kind {
            "I" => p.kind == "I" && close(p.d, d.d("d")) && close(p.bf, d.d("bf")) && close(p.tw, d.d("tw")) && close(p.tf, d.d("tf")),
            "C" => p.kind == "C" && close(p.d, d.d("d")) && close(p.bf, d.d("bf")) && close(p.tw, d.d("tw")) && close(p.tf, d.d("tf")),
            "L" => p.kind == "L" && close(p.b, d.d("b")) && close(p.t, d.d("t")),
            "RHS" => {
                p.kind == "RHS"
                    && close(p.t, d.d("t"))
                    && ((close(p.h, d.d("h")) && close(p.b, d.d("b"))) || (close(p.h, d.d("b")) && close(p.b, d.d("h"))))
            }
            "CHS" => p.kind == "CHS" && close(p.dd, d.d("D")) && close(p.t, d.d("t")),
            _ => false,
        }
    };
    if !pname.is_empty() {
        let key: String = pname.to_uppercase().chars().filter(|c| !cc_cad::js::is_ws(*c)).collect();
        if let Some(p) = catalog_get(&key)
            && fit(&p.s)
        {
            return Some(p.name.clone());
        }
    }
    catalog_rows().iter().find(|p| fit(p)).map(|p| p.name.clone())
}

/// `custom(customs, pname, p, fallback)`.
fn custom(customs: &mut Vec<(String, Value)>, pname: &str, p: &[(&str, Value)], fallback: &str) -> Option<String> {
    let entries: Vec<Value> = p
        .iter()
        .map(|(k, v)| match v.as_f64() {
            Some(x) if v.is_number() => json!([k, cc_detail::js::jnum(round(x * 100.0) / 100.0)]),
            _ => json!([k, v]),
        })
        .collect();
    let key = cc_detail::js::stringify(&Value::Array(entries));
    if let Some((_, r)) = customs.iter().find(|(k, _)| *k == key) {
        return r.get("name").and_then(Value::as_str).map(str::to_string);
    }
    let base0 = clean_str(if pname.is_empty() { fallback } else { pname }, 34);
    let base = if base0.is_empty() { fallback.to_string() } else { base0 };
    let taken = |nm: &str, customs: &Vec<(String, Value)>| {
        let up = nm.to_uppercase();
        catalog_has(&up)
            || {
                nm.as_bytes().get(..2).is_some_and(|b| b.eq_ignore_ascii_case(b"PL")) && {
                    let rest = nm.get(2..).unwrap_or("").trim_start_matches(cc_cad::js::is_ws);
                    rest.chars().next().is_some_and(|c| c.is_ascii_digit())
                }
            }
            || customs.iter().any(|(_, c)| c.get("name").and_then(Value::as_str).is_some_and(|n| n.to_uppercase() == up))
    };
    let mut name = base.clone();
    let mut i = 2u32;
    if taken(&name, customs) {
        name = format!("{base} (file)");
    }
    while taken(&name, customs) && i < 100_000 {
        name = format!("{base} ({i})");
        i = i.saturating_add(1);
    }
    let mut obj = Map::new();
    for (k, v) in p {
        obj.insert((*k).to_string(), v.clone());
    }
    obj.insert("name".into(), Value::from(name.clone()));
    let r = clean_custom(&Value::Object(obj), false).ok()?;
    customs.push((key, r));
    Some(name)
}

fn material_map(imp: &Imp) -> R<HashMap<u64, String>> {
    fn name_of(imp: &Imp, id: Option<f64>, d: u32) -> R<String> {
        let Some(m) = imp.line(id)? else { return Ok(String::new()) };
        if d > 4 {
            return Ok(String::new());
        }
        let c = cls(Some(&m));
        let first = |k: &str| -> R<Option<f64>> { Ok(items(&m.get(k))?.first().and_then(|x| rf(&Some(x.clone())))) };
        let d1 = d.saturating_add(1);
        Ok(match c.as_str() {
            "IfcMaterial" => text(&m.get("Name")),
            "IfcMaterialProfileSetUsage" | "IfcMaterialProfileSetUsageTapering" => name_of(imp, rf(&m.get("ForProfileSet")), d1)?,
            "IfcMaterialLayerSetUsage" => name_of(imp, rf(&m.get("ForLayerSet")), d1)?,
            "IfcMaterialProfileSet" => name_of(imp, first("MaterialProfiles")?, d1)?,
            "IfcMaterialLayerSet" => name_of(imp, first("MaterialLayers")?, d1)?,
            "IfcMaterialConstituentSet" => name_of(imp, first("MaterialConstituents")?, d1)?,
            "IfcMaterialProfile" | "IfcMaterialLayer" | "IfcMaterialConstituent" => name_of(imp, rf(&m.get("Material")), d1)?,
            "IfcMaterialList" => name_of(imp, first("Materials")?, d1)?,
            _ => String::new(),
        })
    }
    let mut out = HashMap::new();
    for id in imp.ids("IFCRELASSOCIATESMATERIAL") {
        let Some(r) = imp.line(Some(id))? else {
            return Err("TypeError: Cannot read properties of null (reading 'RelatingMaterial')".into());
        };
        let nm = name_of(imp, rf(&r.get("RelatingMaterial")), 0)?;
        if nm.is_empty() {
            continue;
        }
        for o in items(&r.get("RelatedObjects"))? {
            if let Some(k) = rf(&Some(o)) {
                out.insert(id_key(k), nm.clone());
            }
        }
    }
    Ok(out)
}

/// One steel member found in the file.
struct Mem {
    kind: &'static str,
    profile: String,
    material: String,
    start: V3,
    end: V3,
    rotation: f64,
    position: &'static str,
    name: String,
    cls: String,
    clipped: bool,
    src: f64,
}

/// `importIfc(WebIFC, api, bytes, { name })` → { state, report }, or the refusal.
pub fn import_ifc(bytes: &[u8], fallback_name: &str) -> R<Value> {
    let f = IfcFile::open(bytes).map_err(|_| t0("ifc.err.open"))?;
    convert(&f, fallback_name)
}

fn convert(f: &IfcFile, fallback_name: &str) -> R<Value> {
    let schema = f.schema_name.to_string();
    let su = schema.to_uppercase();
    if !(su.starts_with("IFC2X3") || su.starts_with("IFC4")) {
        let s = if schema.is_empty() { t0("ifcimp.schema.unknown") } else { schema.clone() };
        return Err(t("ifcimp.err.schema", &[("schema", F::from(s))]));
    }
    let mut imp = Imp { f, to_mm: 1000.0, warnings: vec![], place_cache: HashMap::new() };
    // units
    let mut unit_name = "metre".to_string();
    let prefix = |p: &str| -> Option<f64> {
        Some(match p {
            "EXA" => 1e18,
            "PETA" => 1e15,
            "TERA" => 1e12,
            "GIGA" => 1e9,
            "MEGA" => 1e6,
            "KILO" => 1e3,
            "HECTO" => 100.0,
            "DECA" => 10.0,
            "DECI" => 0.1,
            "CENTI" => 0.01,
            "MILLI" => 1e-3,
            "MICRO" => 1e-6,
            "NANO" => 1e-9,
            _ => return None,
        })
    };
    let si_scale = |u: Option<&Line>| -> Option<f64> {
        let u = u?;
        if !matches!(val(&u.get("Name")), JV::P(Prim::Str(ref s)) if s == "METRE") {
            return None;
        }
        Some(if truthy(&u.get("Prefix")) { prefix(&text_any(&u.get("Prefix"))).unwrap_or(1.0) } else { 1.0 })
    };
    if let Some(ua) = imp.ids("IFCUNITASSIGNMENT").first().copied() {
        let ul = imp.line(Some(ua))?;
        let units = match &ul {
            Some(l) => items(&l.get("Units"))?,
            None => return Err("TypeError: Cannot read properties of null (reading 'Units')".into()),
        };
        for r in units {
            let u = imp.line(rf(&Some(r)))?;
            let Some(u) = u else { continue };
            if !matches!(val(&u.get("UnitType")), JV::P(Prim::Str(ref s)) if s == "LENGTHUNIT") {
                continue;
            }
            if let Some(s) = si_scale(Some(&u)) {
                imp.to_mm = s * 1000.0;
                unit_name =
                    format!("{}metre", if truthy(&u.get("Prefix")) { text_any(&u.get("Prefix")).to_lowercase() } else { String::new() });
            } else if truthy(&u.get("ConversionFactor")) {
                let mwu = imp.line(rf(&u.get("ConversionFactor")))?;
                let fval = mwu.as_ref().map_or(f64::NAN, |m| num(&m.get("ValueComponent")));
                let base_l = match &mwu {
                    Some(m) => imp.line(rf(&m.get("UnitComponent")))?,
                    None => None,
                };
                let base = si_scale(base_l.as_ref());
                match base {
                    Some(b) if fval.is_finite() => {
                        imp.to_mm = fval * b * 1000.0;
                        let nm = text(&u.get("Name"));
                        unit_name = if nm.is_empty() { t0("ifcimp.unit.converted") } else { nm }.to_lowercase();
                    }
                    _ => imp.warnings.push(t0("ifcimp.warn.unit")),
                }
            }
        }
    }
    let to_mm = imp.to_mm;

    // the new model
    let projects = imp.ids("IFCPROJECT");
    let proj_name = match projects.first() {
        Some(p) => match imp.line(Some(*p))? {
            Some(l) => clean_str(&text(&l.get("Name")), 80),
            None => return Err("TypeError: Cannot read properties of null (reading 'Name')".into()),
        },
        None => String::new(),
    };
    let nm = if !proj_name.is_empty() {
        proj_name
    } else if !fallback_name.is_empty() {
        fallback_name.to_string()
    } else {
        t0("ifcimp.name.default")
    };
    let mut model = Model::new(empty_state(&nm));
    let mut converted: Vec<(String, f64)> = Vec::new();
    let mut not_converted: Vec<(String, f64)> = Vec::new();
    fn skip(nc: &mut Vec<(String, f64)>, c: &str, why: &str) {
        let k = format!("{c}|{why}");
        match nc.iter_mut().find(|(x, _)| *x == k) {
            Some(e) => e.1 += 1.0,
            None => nc.push((k, 1.0)),
        }
    }
    fn got(cv: &mut Vec<(String, f64)>, c: &str) {
        match cv.iter_mut().find(|(x, _)| x == c) {
            Some(e) => e.1 += 1.0,
            None => cv.push((c.to_string(), 1.0)),
        }
    }

    // levels
    let mut levels: Vec<(String, f64)> = Vec::new();
    for id in imp.ids("IFCBUILDINGSTOREY") {
        let Some(s) = imp.line(Some(id))? else {
            return Err("TypeError: Cannot read properties of null (reading 'ObjectPlacement')".into());
        };
        let m = if truthy(&s.get("ObjectPlacement")) { Some(imp.placement(rf(&s.get("ObjectPlacement")), 0)?) } else { None };
        let mut z: Option<f64> = m.map(|m| m.o[2] * to_mm);
        let elev = s.get("Elevation");
        if (z.is_none() || (z == Some(0.0) && truthy(&elev))) && truthy(&elev) {
            z = Some(num(&elev) * to_mm);
        }
        let mut nm = clean_str(&text(&s.get("Name")), 40);
        if nm.is_empty() {
            nm = t("ifcimp.level.default", &[("n", F::N(levels.len() as f64 + 1.0))]);
        }
        let mut guard = 0u32;
        while levels.iter().any(|l| l.0 == nm) && guard < 1000 {
            nm = format!("{} ({})", cc_detail::js::slice16(&nm, 36), num_to_string(levels.len() as f64 + 1.0));
            guard = guard.saturating_add(1);
        }
        if let Some(z) = z.filter(|z| z.is_finite()) {
            levels.push((nm, round3(z)));
        }
    }

    // grids
    let (mut gx, mut gy): (Vec<(String, f64)>, Vec<(String, f64)>) = (vec![], vec![]);
    let mut skew_axes = 0u32;
    for id in imp.ids("IFCGRID") {
        let Some(g) = imp.line(Some(id))? else { continue };
        let m = if truthy(&g.get("ObjectPlacement")) { imp.placement(rf(&g.get("ObjectPlacement")), 0)? } else { i4() };
        for k in ["UAxes", "VAxes", "WAxes"] {
            for a in items(&g.get(k))? {
                let ax = imp.line(rf(&Some(a)))?;
                let cv = match &ax {
                    Some(ax) => imp.line(rf(&ax.get("AxisCurve")))?,
                    None => None,
                };
                let mut pts: Vec<V3> = Vec::new();
                if let Some(cv) = &cv
                    && cls(Some(cv)) == "IfcPolyline"
                {
                    for p in items(&cv.get("Points"))? {
                        let q = apply(&m, imp.coords(rf(&Some(p)))?);
                        pts.push(q.map(|v| v * to_mm));
                    }
                }
                let (Some(first), Some(last), true) = (pts.first(), pts.last(), pts.len() >= 2) else {
                    skew_axes = skew_axes.saturating_add(1);
                    continue;
                };
                let dv = norm(sub(*last, *first));
                let tag0 = ax.as_ref().map_or(String::new(), |a| clean_str(&text(&a.get("AxisTag")), 8));
                let tag = if tag0.is_empty() { "?".to_string() } else { tag0 };
                if dv[1].abs() > 0.9999 {
                    gx.push((tag, round3(first[0])));
                } else if dv[0].abs() > 0.9999 {
                    gy.push((tag, round3(first[1])));
                } else {
                    skew_axes = skew_axes.saturating_add(1);
                }
            }
        }
    }
    if skew_axes > 0 {
        skip(&mut not_converted, "IfcGridAxis", &t0("ifcimp.why.grid-skew"));
    }
    let tidy = |mut list: Vec<(String, f64)>| -> Value {
        list.sort_by(|a, b| cc_detail::js::ord(a.1 - b.1));
        let mut out: Vec<(String, f64)> = Vec::new();
        for (label0, at) in list {
            if out.iter().any(|o| (o.1 - at).abs() < 0.5) {
                continue;
            }
            let mut label = label0.clone();
            let mut k = 2u32;
            while out.iter().any(|o| o.0 == label) && k < 100_000 {
                label = format!("{}'{k}", cc_detail::js::slice16(&label0, 6));
                k = k.saturating_add(1);
            }
            out.push((label, at));
        }
        out.truncate(60);
        Value::Array(out.into_iter().map(|(l, a)| json!({ "label": l, "at": cc_detail::js::jnum(a) })).collect())
    };

    // steel
    let steel: [(&str, &'static str); 8] = [
        ("IFCBEAM", "beam"),
        ("IFCBEAMSTANDARDCASE", "beam"),
        ("IFCCOLUMN", "column"),
        ("IFCCOLUMNSTANDARDCASE", "column"),
        ("IFCMEMBER", "brace"),
        ("IFCMEMBERSTANDARDCASE", "brace"),
        ("IFCPLATE", "plate"),
        ("IFCPLATESTANDARDCASE", "plate"),
    ];
    let material_of = material_map(&imp)?;
    let mut steel_ids: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut members: Vec<Mem> = Vec::new();
    let mut customs: Vec<(String, Value)> = Vec::new();
    for (ty_up, ty) in steel {
        for id in imp.ids(ty_up) {
            steel_ids.insert(id_key(id));
            let Some(el) = imp.line(Some(id))? else {
                return Err("TypeError: Cannot read properties of null (reading 'Representation')".into());
            };
            let c = cls(Some(&el));
            if members.len() >= MAX_MEMBERS {
                skip(&mut not_converted, &c, &t("ifcimp.why.too-many", &[("max", F::N(MAX_MEMBERS as f64))]));
                continue;
            }
            let b = match body_solid(&imp, &el)? {
                Ok(b) => b,
                Err(why) => {
                    skip(&mut not_converted, &c, &why);
                    continue;
                }
            };
            let pd = imp.line(rf(&b.solid.get("SweptArea")))?;
            let prof = match &pd {
                Some(pd) => read_profile(&imp, pd)?,
                None => Err(t0("ifcimp.why.no-profile")),
            };
            let prof = match prof {
                Ok(p) => p,
                Err(why) => {
                    skip(&mut not_converted, &c, &why);
                    continue;
                }
            };
            let mel = if truthy(&el.get("ObjectPlacement")) { imp.placement(rf(&el.get("ObjectPlacement")), 0)? } else { i4() };
            let spos = if truthy(&b.solid.get("Position")) { imp.axis3(rf(&b.solid.get("Position")))? } else { i4() };
            let msolid = compose(&compose(&mel, &b.m), &spos);
            let mprof = compose(&msolid, &prof.pos);
            let o = mul(apply(&msolid, prof.pos.o), to_mm);
            let ext = norm(apply_dir(&msolid, imp.dir_of(rf(&b.solid.get("ExtrudedDirection")))?));
            let depth = num(&b.solid.get("Depth")) * to_mm;
            let mut uu = norm(mprof.x);
            let mut vv = norm(mprof.y);
            if dot(ext, uu).abs() > 1e-4 || dot(ext, vv).abs() > 1e-4 {
                skip(&mut not_converted, &c, &t0("ifcimp.why.not-square"));
                continue;
            }
            if !(depth > 0.0) {
                skip(&mut not_converted, &c, &t0("ifcimp.why.zero-length"));
                continue;
            }
            let mut start = o;
            let mut end = add(o, mul(ext, depth));
            if dot(cross(uu, vv), ext) < 0.0 {
                std::mem::swap(&mut start, &mut end);
            }
            let mut profile_name: Option<String>;
            let mut voff = 0.0;
            let mut position = "middle";
            if prof.kind == "RECT" {
                let mut sizes = [(prof.d("x"), uu), (prof.d("y"), vv), (depth, ext)];
                sizes.sort_by(|a, c| cc_detail::js::ord(a.0 - c.0));
                let [thin, mid2, long] = sizes;
                if ty == "plate" || thin.0 < 0.25 * mid2.0 {
                    let cc = add(o, mul(ext, depth / 2.0));
                    let (x_dir, v_dir) = (long.1, thin.1);
                    let u_dir = cross(v_dir, x_dir);
                    start = sub(cc, mul(x_dir, long.0 / 2.0));
                    end = add(cc, mul(x_dir, long.0 / 2.0));
                    uu = u_dir;
                    vv = v_dir;
                    let pn = plate_name(thin.0, mid2.0);
                    if get_profile_str(&pn, &[]).is_none() {
                        skip(&mut not_converted, &c, &t0("ifcimp.why.plate-range"));
                        continue;
                    }
                    profile_name = Some(pn);
                } else {
                    let key = format!("FLAT {}x{}", r1(prof.d("x")), r1(prof.d("y")));
                    profile_name = custom(
                        &mut customs,
                        &prof.pname,
                        &[("kind", Value::from("FLAT")), ("b", jn(prof.d("x"))), ("t", jn(prof.d("y")))],
                        &key,
                    );
                }
            } else {
                profile_name = catalog_match(prof.kind, &prof, &prof.pname);
                if profile_name.is_none() {
                    let mut p: Vec<(&str, Value)> = vec![("kind", Value::from(prof.kind))];
                    for (k, v) in &prof.dims {
                        p.push((k, jn(*v)));
                    }
                    if prof.kind == "I"
                        && !(prof.d("r") >= 0.0)
                        && let Some(e) = p.iter_mut().find(|(k, _)| *k == "r")
                    {
                        e.1 = Value::from(0);
                    }
                    let fallback = format!("{} {}", prof.kind, prof.dims.iter().map(|(_, v)| r1(*v)).collect::<Vec<_>>().join("x"));
                    profile_name = custom(&mut customs, &prof.pname, &p, &fallback);
                }
                let off = prof.pos.o.map(|v| v * to_mm);
                if off[0].abs() < 0.01 && off[1] != 0.0 {
                    let cl: Vec<Value> = customs.iter().map(|(_, c)| c.clone()).collect();
                    let dd = profile_name.as_deref().and_then(|n| get_profile_str(n, &cl)).map_or(0.0, |p| p.depth);
                    if (off[1] + dd / 2.0).abs() < 0.01 {
                        position = "top";
                        voff = off[1];
                    } else if (off[1] - dd / 2.0).abs() < 0.01 {
                        position = "bottom";
                        voff = off[1];
                    }
                }
            }
            let Some(profile_name) = profile_name else {
                skip(&mut not_converted, &c, &t0("ifcimp.why.profile-hold"));
                continue;
            };
            if voff != 0.0 && !voff.is_nan() {
                start = sub(start, mul(vv, voff));
                end = sub(end, mul(vv, voff));
            }
            let f0 = frame(start, end, 0.0);
            let rot = atan2(dot(uu, f0.v), dot(uu, f0.u)) * 180.0 / PI;
            let f1 = frame(start, end, rot);
            if len(sub(f1.u, uu)) > 1e-4 || len(sub(f1.v, vv)) > 1e-4 {
                skip(&mut not_converted, &c, &t0("ifcimp.why.axes"));
                continue;
            }
            if start.iter().chain(&end).any(|v| !v.is_finite() || v.abs() > 1e7) {
                skip(&mut not_converted, &c, &t0("ifcimp.why.far"));
                continue;
            }
            let nm0 = text(&el.get("Name"));
            let nm = if nm0.is_empty() { text(&el.get("Tag")) } else { nm0 };
            members.push(Mem {
                kind: ty,
                profile: profile_name,
                material: clean_str(material_of.get(&id_key(id)).map_or("", String::as_str), 30),
                start: start.map(round3),
                end: end.map(round3),
                rotation: round3(((rot % 360.0) + 360.0) % 360.0),
                position,
                name: clean_str(&nm, 40),
                cls: c,
                clipped: b.clipped,
                src: id,
            });
        }
    }

    // openings and fasteners
    let mut handled: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut voids_of: HashMap<u64, f64> = HashMap::new();
    for rid in imp.ids("IFCRELVOIDSELEMENT") {
        let r = imp.line(Some(rid))?;
        let (el, op) = match &r {
            Some(r) => (rf(&r.get("RelatingBuildingElement")), rf(&r.get("RelatedOpeningElement"))),
            None => (None, None),
        };
        if let (Some(el), Some(op)) = (el.filter(|v| *v != 0.0), op.filter(|v| *v != 0.0)) {
            voids_of.entry(id_key(op)).or_insert(el);
        }
    }
    let mut opening_ids = imp.ids("IFCOPENINGELEMENT");
    opening_ids.extend(imp.ids("IFCOPENINGSTANDARDCASE"));
    let mut fastener_ids = imp.ids("IFCMECHANICALFASTENER");
    fastener_ids.extend(imp.ids("IFCFASTENER"));
    for x in opening_ids.iter().chain(&fastener_ids) {
        handled.insert(id_key(*x));
    }

    // everything else, counted by class
    {
        let ix = &f.ix;
        let mut counts = vec![0usize; ix.names.len()];
        for &tt in &ix.types {
            if let Some(c) = counts.get_mut(usize::from(tt)) {
                *c = c.saturating_add(1);
            }
        }
        let mut seen: Vec<&str> = Vec::new();
        for (k, up) in ix.names.iter().enumerate() {
            let up: &str = up;
            if up.is_empty() || counts.get(k).copied().unwrap_or(0) == 0 || seen.contains(&up) {
                continue;
            }
            seen.push(up);
            let is_el = ["IFC4X3", "IFC4", "IFC2X3"].iter().any(|s| {
                let sc = cc_io_ifc::schema::schema_for(s).0;
                sc.has(up) && sc.is_a(up, "IFCPRODUCT")
            });
            let pretty = f.schema.pretty(up).unwrap_or(up).to_uppercase();
            if !is_el || matches!(pretty.as_str(), "IFCSITE" | "IFCBUILDING" | "IFCBUILDINGSTOREY" | "IFCGRID" | "IFCPROJECT") {
                continue;
            }
            let list: Vec<f64> =
                imp.ids(up).into_iter().filter(|x| !steel_ids.contains(&id_key(*x)) && !handled.contains(&id_key(*x))).collect();
            let Some(first) = list.first() else { continue };
            let c = cls(imp.line(Some(*first))?.as_ref());
            let why = t0(match c.as_str() {
                "IfcElementAssembly" => "ifcimp.why.assembly",
                "IfcFooting" => "ifcimp.why.footings",
                "IfcReinforcingBar" => "ifcimp.why.rebar",
                _ => "ifcimp.why.not-steel",
            });
            for _ in 0..list.len() {
                skip(&mut not_converted, &c, &why);
            }
        }
    }

    let mut clipped_n = 0u32;
    let undo = t0("ifcimp.undo");
    let grids_v = json!({ "x": tidy(gx), "y": tidy(gy) });
    let levels_v = Value::Array(levels.iter().map(|(n, z)| json!({ "name": n, "z": cc_detail::js::jnum(*z) })).collect());
    let mut inner_err: Option<String> = None;
    let res = model.change(&undo, |tx| {
        if !levels.is_empty() {
            tx.set_levels(Some(&levels_v))?;
        }
        tx.set_grids(Some(&grids_v))?;
        for (_, c) in &customs {
            tx.add_profile(Some(c))?;
        }
        let mut member_of: HashMap<u64, Value> = HashMap::new();
        for m in &members {
            let mut o = Map::new();
            o.insert("type".into(), Value::from(m.kind));
            o.insert("profile".into(), Value::from(m.profile.clone()));
            if !m.material.is_empty() {
                o.insert("material".into(), Value::from(m.material.clone()));
            }
            o.insert("start".into(), cc_detail::js::p3v(m.start));
            o.insert("end".into(), cc_detail::js::p3v(m.end));
            o.insert("rotation".into(), cc_detail::js::jnum(m.rotation));
            o.insert("position".into(), Value::from(m.position));
            o.insert("name".into(), Value::from(m.name.clone()));
            match tx.add_member(Some(&Value::Object(o))) {
                Ok(nid) => {
                    member_of.insert(id_key(m.src), nid);
                    got(&mut converted, &m.cls);
                    if m.clipped {
                        clipped_n = clipped_n.saturating_add(1);
                    }
                }
                Err(e) => skip(&mut not_converted, &m.cls, &e),
            }
        }
        let mut step = |imp: &mut Imp, tx: &mut cc_detail::model::Tx| -> R<()> {
            for oid in &opening_ids {
                let op = imp.line(Some(*oid))?;
                let c = cls(op.as_ref());
                let host = voids_of.get(&id_key(*oid)).copied();
                let mid2 = host.and_then(|h| member_of.get(&id_key(h)).cloned());
                let Some(mid2) = mid2 else {
                    skip(&mut not_converted, &c, &t0(if host.is_none() { "ifcimp.why.opening-alone" } else { "ifcimp.why.opening-host" }));
                    continue;
                };
                let Some(op) = op else { continue };
                let e = match extrusion_of(imp, &op)? {
                    Ok(e) => e,
                    Err(why) => {
                        skip(&mut not_converted, &c, &why);
                        continue;
                    }
                };
                let m = cc_detail::js::arr(tx.s.get("members")).iter().find(|x| x.get("id") == Some(&mid2)).cloned().unwrap_or(Value::Null);
                let mp = cc_detail::js::s(m.get("profile")).to_string();
                let custom_list = cc_detail::js::arr(tx.s.get("profiles")).to_vec();
                let Some(pr) = get_profile_str(&mp, &custom_list) else { return Err("TypeError: Cannot read properties of null (reading 'depth')".into()) };
                let fr = frame(cc_detail::js::p3_of(m.get("start")), cc_detail::js::p3_of(m.get("end")), cc_detail::js::f(m.get("rotation")));
                let voff = voff_for(cc_detail::js::s(m.get("position")), pr.depth);
                let (ax, au, av) = (dot(e.ext, fr.x), dot(e.ext, fr.u), dot(e.ext, fr.v));
                match e.prof {
                    EProf::Circle(rr) => {
                        let mid = add(e.o, mul(e.ext, e.depth / 2.0));
                        let l = to_local(&fr, mid, voff);
                        let k = pr.kind.as_str();
                        if ax.abs() > 0.02 {
                            skip(&mut not_converted, &c, &t0("ifcimp.why.hole-slanted"));
                            continue;
                        }
                        if k == "CHS" {
                            skip(&mut not_converted, &c, &t0("ifcimp.why.hole-tube"));
                            continue;
                        }
                        let mut face: Option<&str> = None;
                        let mut y = 0.0;
                        if au.abs() > 0.98 && k != "PL" {
                            face = Some("web");
                            y = l.v;
                        } else if av.abs() > 0.98 {
                            face = Some(if k == "PL" {
                                "plate"
                            } else if k == "L" {
                                "bottom"
                            } else if l.v > 0.0 {
                                "top"
                            } else {
                                "bottom"
                            });
                            y = l.u;
                        }
                        let Some(face) = face else {
                            skip(&mut not_converted, &c, &t0("ifcimp.why.hole-face"));
                            continue;
                        };
                        if l.x < -0.5 || l.x > fr.len + 0.5 {
                            skip(&mut not_converted, &c, &t0("ifcimp.why.hole-off"));
                            continue;
                        }
                        let d = round(2.0 * rr * 10.0) / 10.0;
                        let (x, yy) = (round3(l.x), round3(y));
                        let dup = cc_detail::js::arr(m.get("holes")).iter().any(|h| {
                            h.get("face").and_then(Value::as_str) == Some(face)
                                && (cc_detail::js::f(h.get("x")) - x).abs() < 0.5
                                && (cc_detail::js::f(h.get("y")) - yy).abs() < 0.5
                                && (cc_detail::js::f(h.get("d")) - d).abs() < 0.05
                        });
                        if !dup {
                            let h = json!({ "face": face, "x": cc_detail::js::jnum(x), "y": cc_detail::js::jnum(yy), "d": cc_detail::js::jnum(d) });
                            if let Err(err) = tx.add_hole(Some(&mid2), Some(&h)) {
                                skip(&mut not_converted, &c, &err);
                                continue;
                            }
                        }
                        got(&mut converted, &c);
                    }
                    EProf::Rect { x: px, y: py, xa, ya } => {
                        let mut pts = Vec::new();
                        for s0 in [0.0, e.depth] {
                            for a in [-1.0, 1.0] {
                                for b in [-1.0, 1.0] {
                                    pts.push(to_local(&fr, add(add(add(e.o, mul(e.ext, s0)), mul(xa, a * px / 2.0)), mul(ya, b * py / 2.0)), voff));
                                }
                            }
                        }
                        let lo = |k: char| min_of(pts.iter().map(|q| match k { 'x' => q.x, 'u' => q.u, _ => q.v }));
                        let hi = |k: char| max_of(pts.iter().map(|q| match k { 'x' => q.x, 'u' => q.u, _ => q.v }));
                        let (l, h2, w2) = (fr.len, pr.depth / 2.0, pr.width / 2.0);
                        let ok_kind = pr.kind == "I" || pr.kind == "C";
                        let at_start = lo('x') <= 0.5 && hi('x') > 0.5;
                        let at_end = hi('x') >= l - 0.5 && lo('x') < l - 0.5;
                        let from_top = hi('v') >= h2 - 0.5 && lo('v') > -h2 + 0.5;
                        let from_bottom = lo('v') <= -h2 + 0.5 && hi('v') < h2 - 0.5;
                        if !ok_kind || lo('u') > -w2 + 0.5 || hi('u') < w2 - 0.5 || at_start == at_end || from_top == from_bottom {
                            skip(&mut not_converted, &c, &t0("ifcimp.why.opening-rect"));
                            continue;
                        }
                        let length = round((if at_start { cc_detail::js::min(hi('x'), l) } else { l - cc_detail::js::max(lo('x'), 0.0) }) * 2.0) / 2.0;
                        let depth = round((if from_top { h2 - lo('v') } else { hi('v') + h2 }) * 2.0) / 2.0;
                        let cut = json!({ "kind": "cope", "end": if at_start { "start" } else { "end" }, "side": if from_top { "top" } else { "bottom" }, "length": cc_detail::js::jnum(length), "depth": cc_detail::js::jnum(depth) });
                        match tx.add_cut(Some(&mid2), Some(&cut)) {
                            Ok(_) => got(&mut converted, &c),
                            Err(err) => skip(&mut not_converted, &c, &err),
                        }
                    }
                }
            }
            for fid in &fastener_ids {
                let Some(fe) = imp.line(Some(*fid))? else { continue };
                let c = cls(Some(&fe));
                let e = extrusion_of(imp, &fe)?;
                let mel = if truthy(&fe.get("ObjectPlacement")) { imp.placement(rf(&fe.get("ObjectPlacement")), 0)? } else { i4() };
                let (head, axis) = match &e {
                    Err(_) => (mul(mel.o, to_mm), norm(mel.z)),
                    Ok(e) => (e.o, e.ext),
                };
                let nd = fe.get("NominalDiameter");
                let d = if truthy(&nd) {
                    num(&nd) * to_mm
                } else {
                    match &e {
                        Ok(Ext { prof: EProf::Circle(r), .. }) => 2.0 * r,
                        _ => f64::NAN,
                    }
                };
                let nl = fe.get("NominalLength");
                let ll = if truthy(&nl) {
                    num(&nl) * to_mm
                } else {
                    match &e {
                        Ok(e) => e.depth,
                        Err(_) => f64::NAN,
                    }
                };
                if !(d > 0.0) || !(ll > 0.0) {
                    skip(&mut not_converted, &c, &t0("ifcimp.why.bolt-size"));
                    continue;
                }
                let pt = text(&fe.get("PredefinedType"));
                let kind = if pt.to_uppercase().contains("ANCHOR") { "ANCHORBOLT" } else { "BOLT" };
                let g0 = text(&fe.get("ObjectType"));
                let grade = clean_str(if g0.is_empty() { material_of.get(&id_key(*fid)).map_or("", String::as_str) } else { &g0 }, 20);
                let b = json!({
                    "d": cc_detail::js::jnum(round(d * 10.0) / 10.0), "length": cc_detail::js::jnum(round(ll * 10.0) / 10.0), "grade": grade, "kind": kind,
                    "head": cc_detail::js::p3v(head.map(round3)), "axis": cc_detail::js::p3v(axis),
                });
                match tx.add_bolt(Some(&b)) {
                    Ok(_) => got(&mut converted, &c),
                    Err(err) => skip(&mut not_converted, &c, &err),
                }
            }
            Ok(())
        };
        if let Err(e) = step(&mut imp, tx) {
            inner_err = Some(e.clone());
            return Err(e);
        }
        Ok(())
    });
    if let Err(e) = res {
        return Err(inner_err.unwrap_or(e));
    }
    if clipped_n > 0 {
        imp.warnings.push(t("ifcimp.warn.clipped", &[("n", F::N(f64::from(clipped_n)))]));
    }
    let mut not_list: Vec<(String, f64, Option<String>)> = not_converted
        .into_iter()
        .map(|(k, n)| {
            let mut parts = k.split('|');
            let c = parts.next().unwrap_or("").to_string();
            let why = parts.next().map(str::to_string);
            (c, n, why)
        })
        .collect();
    not_list.sort_by(|a, b| {
        let c = b.1 - a.1;
        cc_detail::js::ord(if c == 0.0 || c.is_nan() { locale_compare(&a.0, &b.0) } else { c })
    });
    let st = &model.state;
    let ng = cc_detail::js::arr(st.get("grids").and_then(|g| g.get("x")))
        .len()
        .saturating_add(cc_detail::js::arr(st.get("grids").and_then(|g| g.get("y"))).len());
    let mut conv = Map::new();
    for (k, n) in &converted {
        conv.insert(k.clone(), cc_detail::js::jnum(*n));
    }
    let mut warnings: Vec<String> = Vec::new();
    for w in imp.warnings {
        if !warnings.contains(&w) {
            warnings.push(w);
        }
    }
    let report = json!({
        "schema": schema, "unit": unit_name, "toMm": cc_detail::js::jnum(to_mm), "converted": conv,
        "notConverted": not_list.iter().map(|(c, n, w)| { let mut o = Map::new(); o.insert("cls".into(), Value::from(c.clone())); o.insert("n".into(), cc_detail::js::jnum(*n)); if let Some(w) = w { o.insert("why".into(), Value::from(w.clone())); } Value::Object(o) }).collect::<Vec<_>>(),
        "grids": ng, "levels": levels.len(),
        "customProfiles": customs.iter().map(|(_, c)| c.get("name").cloned().unwrap_or(Value::Null)).collect::<Vec<_>>(),
        "warnings": warnings,
    });
    Ok(json!({ "state": model.state, "report": report }))
}

fn jn(v: f64) -> Value {
    cc_detail::js::jnum(v)
}
/// `String(val(x))` of an enum-like attribute.
fn text_any(a: &Option<AV>) -> String {
    match val(a) {
        JV::Undef => "undefined".into(),
        JV::Null => "null".into(),
        JV::P(p) => p.to_js_string(),
        JV::List(n) => vec!["[object Object]"; n].join(","),
    }
}

enum EProf {
    Circle(f64),
    Rect { x: f64, y: f64, xa: V3, ya: V3 },
}
struct Ext {
    o: V3,
    ext: V3,
    depth: f64,
    prof: EProf,
}
fn extrusion_of(imp: &mut Imp, el: &Line) -> R<Result<Ext, String>> {
    let b = match body_solid(imp, el)? {
        Ok(b) => b,
        Err(w) => return Ok(Err(w)),
    };
    let Some(pd) = imp.line(rf(&b.solid.get("SweptArea")))? else { return Ok(Err(t0("ifcimp.why.no-profile"))) };
    let mel = if truthy(&el.get("ObjectPlacement")) { imp.placement(rf(&el.get("ObjectPlacement")), 0)? } else { i4() };
    let spos = if truthy(&b.solid.get("Position")) { imp.axis3(rf(&b.solid.get("Position")))? } else { i4() };
    let ms = compose(&compose(&mel, &b.m), &spos);
    let c = cls(Some(&pd));
    let pos = if truthy(&pd.get("Position")) { imp.axis3(rf(&pd.get("Position")))? } else { i4() };
    let o = mul(apply(&ms, pos.o), imp.to_mm);
    let ext = norm(apply_dir(&ms, imp.dir_of(rf(&b.solid.get("ExtrudedDirection")))?));
    let depth = num(&b.solid.get("Depth")) * imp.to_mm;
    let n = |k: &str| num(&pd.get(k)) * imp.to_mm;
    if c == "IfcCircleProfileDef" {
        return Ok(Ok(Ext { o, ext, depth, prof: EProf::Circle(n("Radius")) }));
    }
    if c == "IfcRectangleProfileDef" {
        return Ok(Ok(Ext {
            o,
            ext,
            depth,
            prof: EProf::Rect { x: n("XDim"), y: n("YDim"), xa: norm(apply_dir(&ms, pos.x)), ya: norm(apply_dir(&ms, pos.y)) },
        }));
    }
    Ok(Err(t("ifcimp.why.opening-shape", &[("cls", F::from(c.as_str()))])))
}

/// `reportLines(report)`.
pub fn report_lines(report: &Value) -> Vec<String> {
    let conv: Vec<(String, f64)> = report
        .get("converted")
        .and_then(Value::as_object)
        .map_or(vec![], |m| m.iter().map(|(k, v)| (k.clone(), cc_detail::js::f(Some(v)))).collect());
    let mut lines = Vec::new();
    let to_mm = cc_detail::js::f(report.get("toMm"));
    let factor = str_to_number(&cc_m3d::js::to_precision(to_mm, 6));
    lines.push(t(
        "ifcimp.rep.schema",
        &[
            ("schema", F::from(to_js_string(report.get("schema").unwrap_or(&Value::Null)))),
            ("unit", F::from(to_js_string(report.get("unit").unwrap_or(&Value::Null)))),
            ("factor", F::N(factor)),
        ],
    ));
    let what = if conv.is_empty() {
        t0("ifcimp.rep.no-steel")
    } else {
        conv.iter().map(|(k, n)| format!("{} {k}", num_to_string(*n))).collect::<Vec<_>>().join(", ")
    };
    lines.push(t(
        "ifcimp.rep.brought",
        &[
            ("what", F::from(what)),
            ("levels", F::N(cc_detail::js::f(report.get("levels")))),
            ("grids", F::N(cc_detail::js::f(report.get("grids")))),
        ],
    ));
    let cp = cc_detail::js::arr(report.get("customProfiles"));
    if !cp.is_empty() {
        lines.push(t(
            "ifcimp.rep.custom",
            &[("list", F::from(cp.iter().map(|v| cc_detail::js::string_of(Some(v))).collect::<Vec<_>>().join(", ")))],
        ));
    }
    let g = |k: &str| conv.iter().find(|(n, _)| n == k).map_or(0.0, |x| x.1);
    let (holes, bolts) = (g("IfcOpeningElement") + g("IfcOpeningStandardCase"), g("IfcMechanicalFastener") + g("IfcFastener"));
    if holes != 0.0 || bolts != 0.0 {
        lines.push(t("ifcimp.rep.holes", &[("holes", F::N(holes)), ("bolts", F::N(bolts))]));
    }
    lines
}
