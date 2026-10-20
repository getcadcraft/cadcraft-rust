//! ifc-place.js `IfcPlacer.place` — a library item (libitem.js format 1, docs/LIBRARY-SPEC.md §1) placed into the open
//! IFC model, one for one with the frozen JS: one type per item (uuid@version, or the sha256 of an item from a file)
//! holding the representation maps (Body: IFC4 IfcTriangulatedFaceSet, IFC2X3 IfcFacetedBrep / IfcShellBasedSurfaceModel;
//! FootPrint: an IfcGeometricCurveSet of the plan's polylines; Clearance: boxes), shared by every placement of it; each
//! placement an occurrence with an IfcLocalPlacement relative to its storey, IfcMappedItems of the type's maps, the type
//! and storey relations and the psets CadCraft_Placement, CadCraft_LibraryItem, CadCraft_Equipment,
//! CadCraft_SourceAttributes. Also `nextItemNumber`, `boxFromPlan` / `planBounds` (libitem.js) and the helpers.
//!
//! The item is untrusted JSON, read the way the JS reads it (missing fields are `undefined`, numbers by `Number(…)`);
//! meshes are capped (indices checked against the vertex count) so a hostile item cannot make the writer panic.

use std::collections::HashMap;

use cc_io_ifc::model::Id;
use serde_json::Value as J;

use crate::api::{JsErr, R};
use crate::draw::m;
use crate::draw::writer::{Journal, Row, W, row};
use crate::js::num_str;
use crate::util::{apply4, inv4};
use crate::wapi::{Api, JV, P};

const D2R: f64 = std::f64::consts::PI / 180.0;
/// The most vertices / triangles a placed mesh may have (a hostile item cannot ask for more).
const MAX_VERTS: usize = 2_000_000;

/// item.ifc.class → [IFC4 occurrence, IFC4 type, IFC2X3 occurrence, IFC2X3 type].
const CLASSES: [(&str, [&str; 4]); 6] = [
    ("IfcFurniture", ["IFCFURNITURE", "IFCFURNITURETYPE", "IFCFURNISHINGELEMENT", "IFCFURNITURETYPE"]),
    ("IfcFurnishingElement", ["IFCFURNISHINGELEMENT", "IFCFURNISHINGELEMENTTYPE", "IFCFURNISHINGELEMENT", "IFCFURNISHINGELEMENTTYPE"]),
    (
        "IfcBuildingElementProxy",
        ["IFCBUILDINGELEMENTPROXY", "IFCBUILDINGELEMENTPROXYTYPE", "IFCBUILDINGELEMENTPROXY", "IFCBUILDINGELEMENTPROXYTYPE"],
    ),
    ("IfcSanitaryTerminal", ["IFCSANITARYTERMINAL", "IFCSANITARYTERMINALTYPE", "IFCFLOWTERMINAL", "IFCSANITARYTERMINALTYPE"]),
    ("IfcElectricAppliance", ["IFCELECTRICAPPLIANCE", "IFCELECTRICAPPLIANCETYPE", "IFCFLOWTERMINAL", "IFCELECTRICAPPLIANCETYPE"]),
    ("IfcLightFixture", ["IFCLIGHTFIXTURE", "IFCLIGHTFIXTURETYPE", "IFCFLOWTERMINAL", "IFCLIGHTFIXTURETYPE"]),
];
pub const PSET_PLACEMENT: &str = "CadCraft_Placement";
pub const PSET_LIBRARY: &str = "CadCraft_LibraryItem";
pub const PSET_EQUIPMENT: &str = "CadCraft_Equipment";
pub const PSET_SOURCE: &str = "CadCraft_SourceAttributes";

/// What a placement asks for (`pl.place(item, opts)`): x, y in model mm (Z up, the file's own origin), rot in degrees.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlaceOpts {
    pub storey: Id,
    pub x: f64,
    pub y: f64,
    pub rot: f64,
    pub item_number: String,
    pub quantity: f64,
    pub notes: String,
    pub library: String,
    pub sha256: String,
    pub source_file: String,
    pub stand_in_height: f64,
}

/// What a placement made.
#[derive(Clone, Debug)]
pub struct Placed {
    pub id: Id,
    pub type_id: Id,
    pub type_created: bool,
    pub journal: Journal,
}

// ---------------------------------------------------------------------------------------------- the item, as JS reads it

fn get<'a>(v: &'a J, k: &str) -> Option<&'a J> {
    v.as_object().and_then(|o| o.get(k)).filter(|x| !x.is_null())
}
/// JS truthiness of a JSON value (missing = undefined).
fn truthy(v: Option<&J>) -> bool {
    match v {
        None | Some(J::Null) => false,
        Some(J::Bool(b)) => *b,
        Some(J::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Some(J::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}
/// `Number(v)`
fn num(v: Option<&J>) -> f64 {
    match v {
        None => f64::NAN,
        Some(J::Null) => 0.0,
        Some(J::Bool(b)) => f64::from(u8::from(*b)),
        Some(J::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(J::String(s)) => crate::js::str_num(s),
        Some(J::Array(a)) => match a.as_slice() {
            [] => 0.0,
            [x] => crate::js::str_num(&js_string(x)),
            _ => f64::NAN,
        },
        Some(J::Object(_)) => f64::NAN,
    }
}
/// `String(v)`
fn js_string(v: &J) -> String {
    match v {
        J::Null => "null".into(),
        J::Bool(b) => b.to_string(),
        J::Number(n) => num_str(n.as_f64().unwrap_or(f64::NAN)),
        J::String(s) => s.clone(),
        J::Array(a) => a.iter().map(|x| if x.is_null() { String::new() } else { js_string(x) }).collect::<Vec<_>>().join(","),
        J::Object(_) => "[object Object]".into(),
    }
}
/// `x || ''` as text
fn or_empty(v: Option<&J>) -> String {
    if truthy(v) { v.map(js_string).unwrap_or_default() } else { String::new() }
}
/// A JSON value as the typed value's plain value (`this.T(type, v)`).
fn pval(v: Option<&J>) -> P {
    match v {
        None => P::Undef,
        Some(J::Null) => P::Null,
        Some(J::Bool(b)) => P::Bool(*b),
        Some(J::Number(n)) => P::Num(n.as_f64().unwrap_or(f64::NAN)),
        Some(J::String(s)) => P::s(s),
        Some(J::Array(_) | J::Object(_)) => P::Obj,
    }
}
/// `this.L(s)`: null / undefined / '' → null, else an IfcLabel of `String(s)` (first 255 UTF-16 units).
fn label(w: &W<'_>, v: Option<&J>) -> JV {
    match v {
        None | Some(J::Null) => JV::Null,
        Some(J::String(s)) if s.is_empty() => JV::Null,
        Some(x) => w.l(&js_string(x)),
    }
}
/// `this.X(s)`
fn text(w: &W<'_>, v: Option<&J>) -> JV {
    match v {
        None | Some(J::Null) => JV::Null,
        Some(J::String(s)) if s.is_empty() => JV::Null,
        Some(x) => w.x(&js_string(x)),
    }
}
/// `s || null`
fn or_null(v: Option<&J>) -> Option<&J> {
    if truthy(v) { v } else { None }
}
fn nums(v: Option<&J>) -> Vec<f64> {
    v.and_then(J::as_array).map(|a| a.iter().map(|x| num(Some(x))).collect()).unwrap_or_default()
}
fn cap(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}
fn norm360(a: f64) -> f64 {
    let x = a % 360.0;
    if x < 0.0 { x + 360.0 } else { x }
}
/// ifc-place.js `round(v, d = 6)` (−0 stays −0, as `Math.round(v * 10 ** d) / 10 ** d`)
fn round_p(v: f64, d: i32) -> f64 {
    let p = 10f64.powi(d);
    m::round(v * p) / p
}
/// A whole count from a float (NaN and negatives → 0; capped far above anything placed).
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn to_usize(x: f64) -> usize {
    if x.is_finite() && x > 0.0 { x.min(1e15) as usize } else { 0 }
}
/// libitem.js `r4`
fn r4(v: f64) -> f64 {
    m::round(v * 10000.0) / 10000.0
}

/// `classesFor(item, schema)` → (the class used, occurrence, type)
pub fn classes_for(item: &J, v4: bool) -> (&'static str, &'static str, &'static str) {
    let mut c = get(item, "ifc").and_then(|i| get(i, "class")).map_or_else(|| "IfcBuildingElementProxy".to_string(), js_string);
    if c == "IfcFlowTerminal" {
        let cat = or_empty(get(item, "category"));
        c = if cat.starts_with("electrical/") { "IfcLightFixture".into() } else { "IfcSanitaryTerminal".into() };
    }
    let found = CLASSES.iter().find(|(k, _)| *k == c);
    let (used, rowv) = found.map_or(("IfcBuildingElementProxy", &CLASSES[2].1), |(k, r)| (*k, r));
    let [o4, t4, o3, t3] = *rowv;
    if v4 { (used, o4, t4) } else { (used, o3, t3) }
}

// ----------------------------------------------------------------------------------------------- plan geometry

/// libitem.js `entityPoints(e)` (bounds) and ifc-place.js `planPolylines` share the sampling.
fn arc_pts(cx: f64, cy: f64, r: f64, a0: f64, a1: f64) -> Vec<[f64; 2]> {
    let mut s = a1 - a0;
    let mut guard = 0u32;
    while s <= 0.0 && guard < 100_000 {
        s += 360.0;
        guard = guard.saturating_add(1);
    }
    if !s.is_finite() || s <= 0.0 {
        return Vec::new();
    }
    let n = m::max(4.0, (s / 10.0).ceil()).min(100_000.0);
    let nn = to_usize(n);
    (0..=nn)
        .map(|i| {
            let a = (a0 + s * i as f64 / n) * D2R;
            [cx + r * m::cos(a), cy + r * m::sin(a)]
        })
        .collect()
}
fn ellipse_pts(e: &J) -> Vec<[f64; 2]> {
    let (t0, t1) = (num(get(e, "t0")), num(get(e, "t1")));
    let mut s = t1 - t0;
    let mut guard = 0u32;
    while s <= 0.0 && guard < 100_000 {
        s += 360.0;
        guard = guard.saturating_add(1);
    }
    if !s.is_finite() || s <= 0.0 {
        return Vec::new();
    }
    let (mx, my, ratio, cx, cy) = (num(get(e, "mx")), num(get(e, "my")), num(get(e, "ratio")), num(get(e, "cx")), num(get(e, "cy")));
    let len = m::hypot(mx, my);
    let rot = m::atan2(my, mx);
    let (c, sn) = (m::cos(rot), m::sin(rot));
    let n = m::max(8.0, (s / 10.0).ceil()).min(100_000.0);
    let nn = to_usize(n);
    (0..=nn)
        .map(|i| {
            let t = (t0 + s * i as f64 / n) * D2R;
            let (x, y) = (len * m::cos(t), len * ratio * m::sin(t));
            [cx + x * c - y * sn, cy + x * sn + y * c]
        })
        .collect()
}
fn pts_of(e: &J) -> Vec<Vec<f64>> {
    get(e, "pts").and_then(J::as_array).map(|a| a.iter().map(|p| nums(Some(p))).collect()).unwrap_or_default()
}
fn at2(p: &[f64]) -> [f64; 2] {
    [p.first().copied().unwrap_or(f64::NAN), p.get(1).copied().unwrap_or(f64::NAN)]
}
/// libitem.js `bulgeSample(a, b, bulge)`
fn bulge_sample(a: [f64; 2], b: [f64; 2], bulge: f64) -> Vec<[f64; 2]> {
    let theta = 4.0 * m::atan(bulge);
    let chord = m::hypot(b[0] - a[0], b[1] - a[1]);
    if !(chord > 0.0) {
        return Vec::new();
    }
    let r = chord / (2.0 * m::sin(theta / 2.0));
    let (mx, my) = ((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0);
    let h = r * m::cos(theta / 2.0);
    let (ux, uy) = (-(b[1] - a[1]) / chord, (b[0] - a[0]) / chord);
    let (cx, cy) = (mx + ux * h, my + uy * h);
    let a0 = m::atan2(a[1] - cy, a[0] - cx);
    let steps = m::max(2.0, (theta.abs() / (10.0 * D2R)).ceil()).min(100_000.0);
    let n = to_usize(steps);
    (1..n)
        .map(|k| {
            let t = a0 + theta * k as f64 / steps;
            [cx + r.abs() * m::cos(t), cy + r.abs() * m::sin(t)]
        })
        .collect()
}
/// libitem.js `entityPoints(e)`
fn entity_points(e: &J) -> Vec<[f64; 2]> {
    let n = |k: &str| num(get(e, k));
    match get(e, "type").and_then(J::as_str).unwrap_or("") {
        "line" => vec![[n("x1"), n("y1")], [n("x2"), n("y2")]],
        "circle" => vec![[n("x") - n("r"), n("y") - n("r")], [n("x") + n("r"), n("y") + n("r")]],
        "arc" => arc_pts(n("x"), n("y"), n("r"), n("a0"), n("a1")),
        "poly" => {
            let pts = pts_of(e);
            let closed = truthy(get(e, "closed"));
            let cnt = pts.len();
            let mut out = Vec::new();
            for (i, a) in pts.iter().enumerate() {
                out.push(at2(a));
                if i.saturating_add(1) == cnt && !closed {
                    break;
                }
                let b = pts.get(m::nx(i, cnt)).map_or([f64::NAN; 2], |b| at2(b));
                let bulge = a.get(2).copied().filter(|x| *x != 0.0 && !x.is_nan()).unwrap_or(0.0);
                if bulge.abs() > 1e-9 {
                    out.extend(bulge_sample(at2(a), b, bulge));
                }
            }
            out
        }
        "ellipse" => ellipse_pts(e),
        "point" | "text" => vec![[n("x"), n("y")]],
        "hatch" => get(e, "loops")
            .and_then(J::as_array)
            .map(|ls| ls.iter().flat_map(|l| nums(Some(l)).as_chunks::<2>().0.to_vec()).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}
fn entities(plan: Option<&J>) -> &[J] {
    plan.and_then(|p| get(p, "entities")).and_then(J::as_array).map_or(&[], Vec::as_slice)
}

/// libitem.js `planBounds(plan)` → [minX, minY, maxX, maxY]
pub fn plan_bounds(plan: Option<&J>) -> Option<[f64; 4]> {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for e in entities(plan) {
        for [x, y] in entity_points(e) {
            if !x.is_finite() || !y.is_finite() {
                continue;
            }
            if x < b[0] {
                b[0] = x;
            }
            if x > b[2] {
                b[2] = x;
            }
            if y < b[1] {
                b[1] = y;
            }
            if y > b[3] {
                b[3] = y;
            }
        }
    }
    (b[0] <= b[2]).then_some(b)
}

/// A box body: x, y, z, w, d, h (mm).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxB {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
    pub d: f64,
    pub h: f64,
}

/// libitem.js `boxFromPlan(plan, h, z = 0)`
pub fn box_from_plan(plan: Option<&J>, h: f64, z: f64) -> Option<BoxB> {
    let b = plan_bounds(plan)?;
    if !(h > 0.0) {
        return None;
    }
    let (w, d) = (b[2] - b[0], b[3] - b[1]);
    if !(w > 0.0) || !(d > 0.0) {
        return None;
    }
    Some(BoxB { x: r4(b[0]), y: r4(b[1]), z: r4(z), w: r4(w), d: r4(d), h: r4(h) })
}

/// `planPolylines(plan)`: the footprint's polylines (text and points left out).
pub fn plan_polylines(plan: Option<&J>) -> Vec<Vec<[f64; 2]>> {
    let mut out = Vec::new();
    for e in entities(plan) {
        let n = |k: &str| num(get(e, k));
        match get(e, "type").and_then(J::as_str).unwrap_or("") {
            "line" => out.push(vec![[n("x1"), n("y1")], [n("x2"), n("y2")]]),
            "circle" => out.push(arc_pts(n("x"), n("y"), n("r"), 0.0, 360.0)),
            "arc" => out.push(arc_pts(n("x"), n("y"), n("r"), n("a0"), n("a1"))),
            "ellipse" => out.push(ellipse_pts(e)),
            "poly" => {
                let pts = pts_of(e);
                let closed = truthy(get(e, "closed"));
                let cnt = pts.len();
                let mut ps = Vec::new();
                for (i, a) in pts.iter().enumerate() {
                    let pa = at2(a);
                    ps.push(pa);
                    if i.saturating_add(1) == cnt && !closed {
                        break;
                    }
                    let b = pts.get(m::nx(i, cnt)).map_or([f64::NAN; 2], |b| at2(b));
                    let bulge = a.get(2).copied().filter(|x| *x != 0.0 && !x.is_nan()).unwrap_or(0.0);
                    if bulge.abs() > 1e-9 {
                        // ifc-place.js's own arc of the bulge (its centre on the other side from libitem.js's)
                        let th = 4.0 * m::atan(bulge);
                        let ch = m::hypot(b[0] - pa[0], b[1] - pa[1]);
                        if ch > 0.0 {
                            let r = ch / (2.0 * m::sin(th / 2.0));
                            let (mx, my) = ((pa[0] + b[0]) / 2.0, (pa[1] + b[1]) / 2.0);
                            let h = r * m::cos(th / 2.0);
                            let cx = mx - (b[1] - pa[1]) / ch * h;
                            let cy = my + (b[0] - pa[0]) / ch * h;
                            let a0 = m::atan2(pa[1] - cy, pa[0] - cx);
                            let steps = m::max(2.0, (th.abs() / (10.0 * D2R)).ceil()).min(100_000.0);
                            let sn = to_usize(steps);
                            for k in 1..sn {
                                let t = a0 + th * k as f64 / steps;
                                ps.push([cx + r.abs() * m::cos(t), cy + r.abs() * m::sin(t)]);
                            }
                        }
                    }
                }
                if closed && let Some(f) = pts.first() {
                    ps.push(at2(f));
                }
                out.push(ps);
            }
            "hatch" => {
                for l in get(e, "loops").and_then(J::as_array).map_or(&[][..], Vec::as_slice) {
                    let v = nums(Some(l));
                    let mut p: Vec<[f64; 2]> = v.as_chunks::<2>().0.to_vec();
                    // `l.length` odd: the JS reads l[i + 1] past the end (undefined → NaN)
                    if v.len() % 2 == 1
                        && let Some(&x) = v.last()
                    {
                        p.push([x, f64::NAN]);
                    }
                    p.push([v.first().copied().unwrap_or(f64::NAN), v.get(1).copied().unwrap_or(f64::NAN)]);
                    out.push(p);
                }
            }
            _ => {}
        }
    }
    out
}

/// `boxMesh(b)` → (positions, indices)
pub fn box_mesh(b: &BoxB) -> (Vec<f64>, Vec<usize>) {
    let (x0, y0, z0) = (b.x, b.y, b.z);
    let (x1, y1, z1) = (b.x + b.w, b.y + b.d, z0 + b.h);
    let positions = vec![x0, y0, z0, x1, y0, z0, x1, y1, z0, x0, y1, z0, x0, y0, z1, x1, y0, z1, x1, y1, z1, x0, y1, z1];
    let indices = vec![0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7, 6, 3, 0, 4, 3, 4, 7];
    (positions, indices)
}

/// `isClosed(I)`: every undirected edge used exactly twice.
pub fn is_closed(ix: &[usize]) -> bool {
    let mut mm: HashMap<(usize, usize), u32> = HashMap::new();
    for t in ix.as_chunks::<3>().0 {
        for k in 0..3 {
            let (a, b) = (t.get(k).copied().unwrap_or(0), t.get(m::nx(k, 3)).copied().unwrap_or(0));
            let key = if a < b { (a, b) } else { (b, a) };
            let e = mm.entry(key).or_insert(0);
            *e = e.saturating_add(1);
        }
    }
    !mm.is_empty() && mm.values().all(|n| *n == 2)
}

/// A body: a box or a mesh (positions in mm, triangle indices).
#[derive(Clone, Debug, PartialEq)]
pub enum Body {
    Box(BoxB),
    Mesh { positions: Vec<f64>, indices: Vec<usize> },
}

fn box_of(v: &J) -> BoxB {
    let n = |k: &str| num(get(v, k));
    // `b.z || 0`
    let z = n("z");
    BoxB { x: n("x"), y: n("y"), z: if z != 0.0 && !z.is_nan() { z } else { 0.0 }, w: n("w"), d: n("d"), h: n("h") }
}

/// item.body → a body (None: none, or a mesh whose indices do not fit its vertices).
pub fn body_of(v: Option<&J>) -> R<Option<Body>> {
    let Some(b) = v.filter(|x| truthy(Some(x))) else { return Ok(None) };
    if get(b, "type").and_then(J::as_str) == Some("box") {
        return Ok(Some(Body::Box(box_of(b))));
    }
    let positions = nums(get(b, "positions"));
    let raw = nums(get(b, "indices"));
    let nv = positions.len() / 3;
    if nv > MAX_VERTS || raw.len() > MAX_VERTS.saturating_mul(6) {
        return Err(JsErr("The library item's mesh is too big to place.".into()));
    }
    let mut indices = Vec::with_capacity(raw.len());
    for x in raw {
        if !(x >= 0.0) || x.fract() != 0.0 || x >= nv as f64 {
            return Err(JsErr("The library item's mesh is damaged (an index past its vertices).".into()));
        }
        indices.push(to_usize(x));
    }
    Ok(Some(Body::Mesh { positions, indices }))
}

// ------------------------------------------------------------------------------------------------------- the placer

/// IfcPlacer on the editor's model (one per placement, as `editor.placer()` makes one per call).
pub struct Placer<'a> {
    pub w: W<'a>,
    origin: Option<Id>,
    op: Option<Id>,
    clear_ctx: Option<Id>,
}

impl<'a> Placer<'a> {
    pub fn new(api: &'a mut Api) -> R<Placer<'a>> {
        Ok(Placer { w: W::new(api)?, origin: None, op: None, clear_ctx: None })
    }

    fn origin(&mut self) -> R<Id> {
        if let Some(o) = self.origin {
            return Ok(o);
        }
        let p = self.w.point3(0.0, 0.0, 0.0)?;
        let o = self.w.make("IFCAXIS2PLACEMENT3D", vec![JV::H(p), JV::Null, JV::Null])?;
        self.origin = Some(o);
        Ok(o)
    }
    fn operator(&mut self) -> R<Id> {
        if let Some(o) = self.op {
            return Ok(o);
        }
        let p = self.w.point3(0.0, 0.0, 0.0)?;
        let o = self.w.make("IFCCARTESIANTRANSFORMATIONOPERATOR3D", vec![JV::Null, JV::Null, JV::H(p), JV::Null, JV::Null])?;
        self.op = Some(o);
        Ok(o)
    }

    /// `bodyItems(body)` → (items, representation type)
    fn body_items(&mut self, body: &Body) -> R<(Vec<Id>, &'static str)> {
        let (pos, ix) = match body {
            Body::Box(b) => box_mesh(b),
            Body::Mesh { positions, indices } => (positions.clone(), indices.clone()),
        };
        let pp: Vec<f64> = pos.iter().map(|v| self.w.u(*v)).collect();
        let tri = ix.as_chunks::<3>().0;
        if self.w.v4 {
            let coords: Vec<JV> = pp.as_chunks::<3>().0.iter().map(|c| JV::L(vec![JV::Num(c[0]), JV::Num(c[1]), JV::Num(c[2])])).collect();
            let list = if self.w.x3 {
                self.w.make("IFCCARTESIANPOINTLIST3D", vec![JV::L(coords), JV::Null])?
            } else {
                self.w.make("IFCCARTESIANPOINTLIST3D", vec![JV::L(coords)])?
            };
            let pi = |w: &W<'_>, n: usize| w.t("IFCPOSITIVEINTEGER", P::Num(n.saturating_add(1) as f64));
            let idx: Vec<JV> = tri.iter().map(|t| JV::L(vec![pi(&self.w, t[0]), pi(&self.w, t[1]), pi(&self.w, t[2])])).collect();
            let closed = is_closed(&ix);
            let cl = self.w.t("IFCBOOLEAN", P::Bool(closed));
            let fs = self.w.make("IFCTRIANGULATEDFACESET", vec![JV::H(list), JV::Null, cl, JV::L(idx), JV::Null])?;
            return Ok((vec![fs], "Tessellation"));
        }
        let mut pts = Vec::with_capacity(pp.len() / 3);
        for c in pp.as_chunks::<3>().0 {
            pts.push(self.w.point3(c[0], c[1], c[2])?);
        }
        let mut faces = Vec::new();
        for t in tri {
            if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
                continue;
            }
            let h = |i: usize| pts.get(i).copied().map_or(JV::Null, JV::H);
            let lp = self.w.make("IFCPOLYLOOP", vec![JV::L(vec![h(t[0]), h(t[1]), h(t[2])])])?;
            let tb = self.w.t("IFCBOOLEAN", P::Bool(true));
            let bound = self.w.make("IFCFACEOUTERBOUND", vec![JV::H(lp), tb])?;
            faces.push(JV::H(self.w.make("IFCFACE", vec![JV::L(vec![JV::H(bound)])])?));
        }
        if is_closed(&ix) {
            let sh = self.w.make("IFCCLOSEDSHELL", vec![JV::L(faces)])?;
            return Ok((vec![self.w.make("IFCFACETEDBREP", vec![JV::H(sh)])?], "Brep"));
        }
        let sh = self.w.make("IFCOPENSHELL", vec![JV::L(faces)])?;
        Ok((vec![self.w.make("IFCSHELLBASEDSURFACEMODEL", vec![JV::L(vec![JV::H(sh)])])?], "SurfaceModel"))
    }

    /// `partBodyItems(item, body)`: CC-RENDER-PARTS ('from-to #rrggbb[/alpha];…') → each part its own styled body item.
    fn part_body_items(&mut self, item: &J, body: &Body) -> R<Option<(Vec<Id>, &'static str)>> {
        let Some(text) = get(item, "source_attrs").and_then(|s| get(s, "CC-RENDER-PARTS")).and_then(J::as_str) else { return Ok(None) };
        let Body::Mesh { positions, indices } = body else { return Ok(None) };
        if text.len() > 2000 {
            return Ok(None);
        }
        let n_tri = indices.len() / 3;
        let mut parts: Vec<(usize, usize, [f64; 3], f64)> = Vec::new();
        for s_part in text.split(';') {
            let Some(p) = parse_part(s_part) else { return Ok(None) };
            let (from, to, rgb, tr) = p;
            if to < from || to >= n_tri || parts.last().is_some_and(|l| from <= l.1) || parts.len() >= 64 {
                return Ok(None);
            }
            parts.push((from, to, rgb, tr));
        }
        let mut out = Vec::new();
        let mut ty: Option<&'static str> = None;
        for (from, to, rgb, tr) in parts {
            let mut map: HashMap<usize, usize> = HashMap::new();
            let (mut ps, mut ix) = (Vec::new(), Vec::new());
            for t in from..=to {
                for k in 0..3 {
                    let v = indices.get(t.saturating_mul(3).saturating_add(k)).copied().unwrap_or(0);
                    let next = ps.len() / 3;
                    let e = *map.entry(v).or_insert_with(|| {
                        for d in 0..3 {
                            ps.push(positions.get(v.saturating_mul(3).saturating_add(d)).copied().unwrap_or(f64::NAN));
                        }
                        next
                    });
                    ix.push(e);
                }
            }
            let (items, t2) = self.body_items(&Body::Mesh { positions: ps, indices: ix })?;
            if ty.is_some_and(|x| x != t2) {
                return Ok(None);
            }
            ty = Some(t2);
            let col = self.w.make("IFCCOLOURRGB", vec![JV::Null, JV::Num(rgb[0]), JV::Num(rgb[1]), JV::Num(rgb[2])])?;
            let shade = if self.w.v4 {
                self.w.make("IFCSURFACESTYLESHADING", vec![JV::H(col), JV::Num(tr)])?
            } else {
                self.w.make("IFCSURFACESTYLESHADING", vec![JV::H(col)])?
            };
            let style = self.w.make("IFCSURFACESTYLE", vec![JV::Null, JV::s("BOTH"), JV::L(vec![JV::H(shade)])])?;
            for it in &items {
                let styles = if self.w.v4 {
                    JV::L(vec![JV::H(style)])
                } else {
                    JV::L(vec![JV::H(self.w.make("IFCPRESENTATIONSTYLEASSIGNMENT", vec![JV::L(vec![JV::H(style)])])?)])
                };
                self.w.make("IFCSTYLEDITEM", vec![JV::H(*it), styles, JV::Null])?;
            }
            out.extend(items);
        }
        Ok(if out.is_empty() { None } else { Some((out, ty.unwrap_or("Tessellation"))) })
    }

    fn footprint_items(&mut self, plan: Option<&J>) -> R<Vec<Id>> {
        let mut curves = Vec::new();
        for pts in plan_polylines(plan) {
            if pts.len() < 2 {
                continue;
            }
            let mut hs = Vec::with_capacity(pts.len());
            for [x, y] in pts {
                let (ux, uy) = (self.w.u(x), self.w.u(y));
                hs.push(JV::H(self.w.point2(ux, uy)?));
            }
            curves.push(JV::H(self.w.make("IFCPOLYLINE", vec![JV::L(hs)])?));
        }
        if curves.is_empty() { Ok(Vec::new()) } else { Ok(vec![self.w.make("IFCGEOMETRICCURVESET", vec![JV::L(curves)])?]) }
    }

    /// `typePsets(typeId)[name]` props: name → value
    fn type_pset(&self, type_id: Id, name: &str) -> R<Option<HashMap<String, P>>> {
        let t = self.w.must(type_id)?;
        // `out[psetName] = …`: a later set of the same name wins
        for h in t.a("HasPropertySets").hrefs().into_iter().rev() {
            let Some(ps) = self.w.line(h)? else { continue };
            if ps.a("Name").text_or_empty() != name {
                continue;
            }
            let mut out = HashMap::new();
            for ph in ps.a("HasProperties").hrefs() {
                if let Some(p) = self.w.line(ph)?
                    && p.get("Name").is_some_and(JV::truthy)
                {
                    out.insert(p.a("Name").val().string(), p.a("NominalValue").val());
                }
            }
            return Ok(Some(out));
        }
        Ok(None)
    }

    /// `libraryTypes()`: types this tool made, key ("uuid@version" or "sha:<sha256>") → type id.
    pub fn library_types(&self) -> R<HashMap<String, Id>> {
        let mut names: Vec<String> = Vec::new();
        for (_, r) in CLASSES {
            for n in [r[1], r[3]] {
                if !names.iter().any(|x| x == n) {
                    names.push(n.to_string());
                }
            }
        }
        // the file's other type classes (GetAllTypesOfModel): every *TYPE entity of the schema that has lines
        let more: Vec<String> = self
            .w
            .api
            .schema()
            .names()
            .filter(|n| n.ends_with("TYPE") && !n.starts_with("IFCREL") && !names.iter().any(|x| x == n))
            .map(str::to_string)
            .collect();
        names.extend(more);
        let mut out = HashMap::new();
        for n in names {
            for id in self.w.ids(&n) {
                let Some(line) = self.w.line(id)? else { continue };
                if !line.get("HasPropertySets").is_some_and(JV::truthy) {
                    continue;
                }
                let Some(p) = self.type_pset(id, PSET_LIBRARY)? else { continue };
                let uuid = p.get("ItemUuid").cloned().unwrap_or(P::Undef);
                let key = if uuid.truthy() {
                    format!("{}@{}", uuid.string(), p.get("ItemVersion").cloned().unwrap_or(P::Undef).string())
                } else {
                    let sha = p.get("ItemSha256").cloned().unwrap_or(P::Undef);
                    format!("sha:{}", if sha.truthy() { sha.string() } else { String::new() })
                };
                out.entry(key).or_insert(id);
            }
        }
        Ok(out)
    }

    fn library_props(&self, item: &J, o: &PlaceOpts) -> Vec<Row> {
        let lic = get(item, "license");
        let prov = get(item, "provenance");
        let pv = |k: &str| prov.and_then(|p| get(p, k));
        let library = if !o.library.is_empty() {
            o.library.clone()
        } else if truthy(pv("library")) {
            or_empty(pv("library"))
        } else if truthy(get(item, "uuid")) {
            "librarycraft".into()
        } else {
            "file".into()
        };
        let sha = if !o.sha256.is_empty() { o.sha256.clone() } else { or_empty(pv("sha256")) };
        let src = if !o.source_file.is_empty() { o.source_file.clone() } else { or_empty(pv("source_file")) };
        let version = match get(item, "version") {
            None => P::Num(0.0),
            v => pval(v),
        };
        vec![
            row("Library", "IFCLABEL", P::s(&library)),
            row("ItemUuid", "IFCIDENTIFIER", P::s(&or_empty(get(item, "uuid")))),
            row("ItemVersion", "IFCINTEGER", version),
            row("ItemSha256", "IFCIDENTIFIER", P::s(&sha)),
            row("License", "IFCLABEL", P::s(&or_empty(lic.and_then(|l| get(l, "id"))))),
            row("Attribution", "IFCTEXT", P::s(&or_empty(lic.and_then(|l| get(l, "attribution"))))),
            row("SourceFile", "IFCLABEL", P::s(&src)),
        ]
    }
    fn equipment_props(&self, item: &J) -> Vec<Row> {
        let p = get(item, "params");
        let pg = |k: &str| p.and_then(|x| get(x, k));
        let name = match item.as_object().and_then(|o| o.get("name")) {
            None => P::Undef,
            v => pval(v),
        };
        let mut out = vec![
            row("Name", "IFCLABEL", name),
            row("Maker", "IFCLABEL", P::s(&or_empty(get(item, "maker")))),
            row("Model", "IFCLABEL", P::s(&or_empty(get(item, "model")))),
            row("Category", "IFCLABEL", P::s(&or_empty(get(item, "category")))),
        ];
        for (k, n) in [("width", "Width"), ("depth", "Depth"), ("height", "Height")] {
            let v = num(pg(k));
            if v > 0.0 && pg(k).is_some() {
                out.push(row(n, "IFCPOSITIVELENGTHMEASURE", P::Num(self.w.u(v))));
            }
        }
        let wk = num(pg("weight_kg"));
        if wk > 0.0 && pg("weight_kg").is_some() {
            out.push(row("WeightKg", "IFCMASSMEASURE", pval(pg("weight_kg"))));
        }
        for (key, pre) in [("elec", "Elec"), ("plumb", "Plumb")] {
            if let Some(o) = pg(key).and_then(J::as_object) {
                for (k, v) in o {
                    if truthy(Some(v)) {
                        out.push((format!("{pre}{}", cap(k)), "IFCLABEL".into(), pval(Some(v))));
                    }
                }
            }
        }
        if truthy(pg("remarks")) {
            out.push(row("Remarks", "IFCTEXT", pval(pg("remarks"))));
        }
        out
    }

    /// `typeFor(item, opts)` → (type id, created)
    fn type_for(&mut self, item: &J, o: &PlaceOpts) -> R<(Id, bool)> {
        let prov_sha = get(item, "provenance").and_then(|p| get(p, "sha256"));
        let key = if truthy(get(item, "uuid")) {
            format!(
                "{}@{}",
                or_empty(get(item, "uuid")),
                item.as_object().and_then(|x| x.get("version")).map_or_else(|| "undefined".into(), js_string)
            )
        } else {
            format!("sha:{}", if !o.sha256.is_empty() { o.sha256.clone() } else { or_empty(prov_sha) })
        };
        if let Some(&known) = self.library_types()?.get(&key) {
            return Ok((known, false));
        }
        let body_ctx = self.w.ctx_of("body")?;
        let foot_ctx = self.w.ctx_of("foot")?;
        let mut maps = Vec::new();
        let mut body = body_of(get(item, "body"))?;
        if body.is_none() && o.stand_in_height > 0.0 {
            body = box_from_plan(get(item, "plan"), o.stand_in_height, 0.0).map(Body::Box);
        }
        // `!body` after the stand-in box: the words only when no box could be made either
        let has_body = body.is_some();
        if let Some(b) = &body {
            let (items, ty) = match self.part_body_items(item, b)? {
                Some(x) => x,
                None => self.body_items(b)?,
            };
            let (l1, l2) = (self.w.l("Body"), self.w.l(ty));
            let rep =
                self.w.make("IFCSHAPEREPRESENTATION", vec![JV::H(body_ctx), l1, l2, JV::L(items.iter().map(|i| JV::H(*i)).collect())])?;
            let org = self.origin()?;
            maps.push(self.w.make("IFCREPRESENTATIONMAP", vec![JV::H(org), JV::H(rep)])?);
        }
        let fp = self.footprint_items(get(item, "plan"))?;
        if !fp.is_empty() {
            let (l1, l2) = (self.w.l("FootPrint"), self.w.l("GeometricCurveSet"));
            let rep =
                self.w.make("IFCSHAPEREPRESENTATION", vec![JV::H(foot_ctx), l1, l2, JV::L(fp.iter().map(|i| JV::H(*i)).collect())])?;
            let org = self.origin()?;
            maps.push(self.w.make("IFCREPRESENTATIONMAP", vec![JV::H(org), JV::H(rep)])?);
        }
        if let Some(cl) = get(item, "clearances").and_then(J::as_array).filter(|a| !a.is_empty()) {
            let ctx = match self.clear_ctx {
                Some(c) => c,
                None => {
                    let c = self.w.ctx_of("Clearance")?;
                    self.clear_ctx = Some(c);
                    c
                }
            };
            let mut its = Vec::new();
            let mut ty = "Tessellation";
            for c in cl {
                let (items, t2) = self.body_items(&Body::Box(box_of(c)))?;
                its.extend(items);
                ty = t2;
            }
            let (l1, l2) = (self.w.l("Clearance"), self.w.l(ty));
            let rep = self.w.make("IFCSHAPEREPRESENTATION", vec![JV::H(ctx), l1, l2, JV::L(its.iter().map(|i| JV::H(*i)).collect())])?;
            let org = self.origin()?;
            maps.push(self.w.make("IFCREPRESENTATIONMAP", vec![JV::H(org), JV::H(rep)])?);
        }
        let rows = self.library_props(item, o);
        let lib_pset = self.w.pset(PSET_LIBRARY, &rows)?;
        let (_, _, cls_type) = classes_for(item, self.w.v4);
        let g = self.w.guid();
        let oh = self.w.owner_history();
        let name = label(&self.w, item.as_object().and_then(|x| x.get("name")));
        let desc = if !has_body && o.stand_in_height > 0.0 {
            self.w.x(&format!("Stand-in box {} mm tall: the library item has no 3D body.", num_str(o.stand_in_height)))
        } else {
            text(&self.w, or_null(get(item, "description")))
        };
        let et = if truthy(get(item, "maker")) && truthy(get(item, "model")) {
            self.w.l(&format!("{} {}", or_empty(get(item, "maker")), or_empty(get(item, "model"))))
        } else {
            JV::Null
        };
        let mut args =
            vec![g, oh, name, desc, JV::Null, JV::L(vec![JV::H(lib_pset)]), JV::L(maps.iter().map(|m| JV::H(*m)).collect()), JV::Null, et];
        if cls_type == "IFCFURNITURETYPE" {
            args.push(JV::e("NOTDEFINED"));
            if self.w.v4 {
                args.push(JV::e("NOTDEFINED"));
            }
        } else if cls_type != "IFCFURNISHINGELEMENTTYPE" {
            args.push(JV::e("NOTDEFINED"));
        }
        let id = self.w.make(cls_type, args)?;
        Ok((id, true))
    }

    /// `shapeFor(typeId)`: mapped items of each of the type's maps.
    fn shape_for(&mut self, type_id: Id) -> R<Option<Id>> {
        let t = self.w.must(type_id)?;
        let mut reps = Vec::new();
        for h in t.a("RepresentationMaps").hrefs() {
            let map = self.w.must(h)?;
            let rid = map
                .a("MappedRepresentation")
                .href()
                .ok_or_else(|| JsErr("TypeError: Cannot read properties of undefined (reading 'value')".into()))?;
            let rep = self.w.must(rid)?;
            let op = self.operator()?;
            let mi = self.w.make("IFCMAPPEDITEM", vec![JV::H(h), JV::H(op)])?;
            let mr = self.w.l("MappedRepresentation");
            reps.push(JV::H(self.w.make(
                "IFCSHAPEREPRESENTATION",
                vec![rep.a("ContextOfItems").clone(), rep.a("RepresentationIdentifier").clone(), mr, JV::L(vec![JV::H(mi)])],
            )?));
        }
        if reps.is_empty() { Ok(None) } else { Ok(Some(self.w.make("IFCPRODUCTDEFINITIONSHAPE", vec![JV::Null, JV::Null, JV::L(reps)])?)) }
    }

    /// `localPlacement(st, opts)`: model mm → file units → the storey's own axes.
    fn local_placement(&mut self, st_placement: &JV, o: &PlaceOpts) -> R<Id> {
        let pl = st_placement.href().ok_or_else(|| JsErr("TypeError: Cannot read properties of null (reading 'value')".into()))?;
        let ws = self.w.api.world(pl)?;
        let wi = inv4(&ws).ok_or_else(|| JsErr("TypeError: Cannot read properties of null (reading '0')".into()))?;
        let [lx, ly, _] = apply4(&wi, self.w.u(o.x), self.w.u(o.y), ws[14]);
        let r = (if o.rot != 0.0 && !o.rot.is_nan() { o.rot } else { 0.0 }) * D2R;
        let dl = [wi[0] * m::cos(r) + wi[4] * m::sin(r), wi[1] * m::cos(r) + wi[5] * m::sin(r)];
        let loc = self.w.point3(round_p(lx, 6), round_p(ly, 6), 0.0)?;
        let z = self.w.dir3(0.0, 0.0, 1.0)?;
        let xd = self.w.dir3(round_p(dl[0], 9), round_p(dl[1], 9), 0.0)?;
        let ax = self.w.make("IFCAXIS2PLACEMENT3D", vec![JV::H(loc), JV::H(z), JV::H(xd)])?;
        self.w.make("IFCLOCALPLACEMENT", vec![st_placement.clone(), JV::H(ax)])
    }

    fn occ_extra(&self, occ: &str) -> Vec<JV> {
        if self.w.v4 && occ != "IFCFURNISHINGELEMENT" && occ != "IFCFLOWTERMINAL" {
            return vec![JV::e("NOTDEFINED")];
        }
        if !self.w.v4 && occ == "IFCBUILDINGELEMENTPROXY" {
            return vec![JV::Null];
        }
        Vec::new()
    }

    /// `place(item, opts)`
    pub fn place(&mut self, item: &J, o: &PlaceOpts) -> R<Placed> {
        self.w.begin();
        match self.place_in(item, o) {
            Ok((id, type_id, created)) => {
                let journal = self.w.journal.take().unwrap_or_default();
                Ok(Placed { id, type_id, type_created: created, journal })
            }
            Err(e) => {
                // the JS leaves what it wrote before the error; the editor never records it, so nothing half-made stays
                if let Some(j) = self.w.journal.take() {
                    crate::draw::writer::undo_journal(self.w.api, &j)?;
                }
                Err(e)
            }
        }
    }

    fn place_in(&mut self, item: &J, o: &PlaceOpts) -> R<(Id, Id, bool)> {
        let st = match self.w.line(o.storey)? {
            Some(st) if st.class.to_ascii_lowercase().contains("buildingstorey") => st,
            _ => return Err(JsErr(crate::draw::tr::t0("place.err.pick_storey"))),
        };
        let (type_id, created) = self.type_for(item, o)?;
        let lp = self.local_placement(st.a("ObjectPlacement"), o)?;
        let shape = self.shape_for(type_id)?;
        let (_, occ, _) = classes_for(item, self.w.v4);
        let g = self.w.guid();
        let oh = self.w.owner_history();
        let name = item.as_object().and_then(|x| x.get("name"));
        let mut args = vec![
            g,
            oh,
            label(&self.w, name),
            text(&self.w, or_null(get(item, "description"))),
            label(&self.w, name),
            JV::H(lp),
            shape.map_or(JV::Null, JV::H),
            JV::Null,
        ];
        args.extend(self.occ_extra(occ));
        let id = self.w.make(occ, args)?;
        self.w.relate_type(id, type_id)?;
        self.w.contain(id, o.storey)?;
        let q = if o.quantity != 0.0 && !o.quantity.is_nan() { o.quantity } else { 1.0 };
        let q = m::round(q);
        let q = if q != 0.0 && !q.is_nan() { q } else { 1.0 };
        let prow = vec![
            row("ItemNumber", "IFCLABEL", P::s(&o.item_number)),
            row("Quantity", "IFCINTEGER", P::Num(m::max(1.0, q))),
            row("Notes", "IFCTEXT", P::s(&o.notes)),
        ];
        let pl = self.w.pset(PSET_PLACEMENT, &prow)?;
        self.w.relate(id, pl)?;
        let lib = self.library_props(item, o);
        let lp2 = self.w.pset(PSET_LIBRARY, &lib)?;
        self.w.relate(id, lp2)?;
        let eq = self.equipment_props(item);
        let ep = self.w.pset(PSET_EQUIPMENT, &eq)?;
        self.w.relate(id, ep)?;
        if let Some(src) = get(item, "source_attrs").and_then(J::as_object).filter(|s| !s.is_empty()) {
            let rows: Vec<Row> = src.iter().map(|(k, v)| (k.clone(), "IFCTEXT".to_string(), pval(Some(v)))).collect();
            let sp = self.w.pset(PSET_SOURCE, &rows)?;
            self.w.relate(id, sp)?;
        }
        Ok((id, type_id, created))
    }
}

/// libitem.js `canonicalJson(v)`: keys sorted at every level (UTF-16 order), no whitespace, numbers and strings as
/// `JSON.stringify` writes them. Nesting deeper than 64 levels is written as null (the JS would recurse on).
pub fn canonical_json(v: &J) -> String {
    let mut out = String::new();
    canon_in(v, &mut out, 0);
    out
}
fn canon_in(v: &J, out: &mut String, depth: u32) {
    if depth > 64 {
        out.push_str("null");
        return;
    }
    match v {
        J::Null => out.push_str("null"),
        J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        J::Number(n) => {
            let x = n.as_f64().unwrap_or(f64::NAN);
            out.push_str(&if x.is_finite() { num_str(x) } else { "null".to_string() });
        }
        J::String(s) => json_str(s, out),
        J::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canon_in(x, out, depth.saturating_add(1));
            }
            out.push(']');
        }
        J::Object(o) => {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                json_str(k, out);
                out.push(':');
                if let Some(x) = o.get(*k) {
                    canon_in(x, out, depth.saturating_add(1));
                }
            }
            out.push('}');
        }
    }
}
/// `JSON.stringify(s)`
fn json_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}
/// The bytes `itemHash` hashes: the canonical JSON of the item without `provenance`.
pub fn item_hash_input(item: &J) -> String {
    match item {
        J::Object(o) => {
            let mut rest = o.clone();
            rest.shift_remove("provenance");
            canonical_json(&J::Object(rest))
        }
        // `const { provenance, ...rest } = obj || {}`: a non-object spreads to {} (a string to its characters)
        J::String(s) => {
            let mut m = serde_json::Map::new();
            for (i, c) in s.encode_utf16().enumerate() {
                m.insert(i.to_string(), J::String(String::from_utf16_lossy(&[c])));
            }
            canonical_json(&J::Object(m))
        }
        _ => "{}".into(),
    }
}

/// `parseInt(s, 10)` → None for NaN.
fn parse_int(s: &str) -> Option<f64> {
    let t = s.trim_start();
    let (neg, rest) = match t.as_bytes().first() {
        Some(b'-') => (true, t.get(1..).unwrap_or("")),
        Some(b'+') => (false, t.get(1..).unwrap_or("")),
        _ => (false, t),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let v = crate::js::str_num(&digits);
    Some(if neg { -v } else { v })
}

/// `nextItemNumber()`: the next free whole number over every placement's ItemNumber.
pub fn next_item_number(api: &mut Api) -> R<String> {
    let mut n = 0.0;
    for rid in api.ids("IFCRELDEFINESBYPROPERTIES", false) {
        let r = api.must(rid)?;
        let Some(ps) = r.a("RelatingPropertyDefinition").href() else { continue };
        let Some(pl) = api.line(ps)? else { continue };
        if pl.a("Name").text_or_empty() != PSET_PLACEMENT || r.a("RelatedObjects").hrefs().is_empty() {
            continue;
        }
        for h in pl.a("HasProperties").hrefs() {
            let Some(p) = api.line(h)? else { continue };
            if p.a("Name").val().string() == "ItemNumber"
                && let Some(k) = parse_int(&p.a("NominalValue").val().string())
                && k > n
            {
                n = k;
            }
        }
    }
    Ok(num_str(n + 1.0))
}

/// `snapToWall`'s back of the item: planBounds' maxY, else a box body's y + d, else 0.
pub fn back_of(item: &J) -> f64 {
    if let Some(b) = plan_bounds(get(item, "plan")) {
        return if b[3] != 0.0 && !b[3].is_nan() { b[3] } else { 0.0 };
    }
    match get(item, "body") {
        Some(b) if get(b, "type").and_then(J::as_str) == Some("box") => {
            let v = num(get(b, "y")) + num(get(b, "d"));
            if v != 0.0 && !v.is_nan() { v } else { 0.0 }
        }
        _ => 0.0,
    }
}

/// `snapToWall(p, item, faces)`: the back of the item against the nearest wall face → (x, y, rot, distance).
pub fn snap_to_wall(p: [f64; 2], item: &J, faces: &[Face]) -> Option<(f64, f64, f64, f64)> {
    let mut best: Option<(f64, [f64; 2], [f64; 2])> = None;
    for (a, b, n) in faces {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l2 = dx * dx + dy * dy;
        let t = m::max(0.0, m::min(1.0, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2));
        let q = [a[0] + t * dx, a[1] + t * dy];
        let d = m::hypot(p[0] - q[0], p[1] - q[1]);
        if (p[0] - q[0]) * n[0] + (p[1] - q[1]) * n[1] < -1.0 {
            continue;
        }
        if best.is_none_or(|bb| d < bb.0) {
            best = Some((d, q, *n));
        }
    }
    let (d, q, n) = best?;
    let rot = m::atan2(-n[1], -n[0]) / D2R - 90.0;
    let max_y = back_of(item);
    Some((q[0] + n[0] * max_y, q[1] + n[1] * max_y, m::round(norm360(rot) * 1000.0) / 1000.0, d))
}

/// A wall face in plan, model mm: (a, b, n) — n points away from the wall.
pub type Face = ([f64; 2], [f64; 2], [f64; 2]);

/// `minAreaRect(points)`: the minimum-area bounding rectangle of plan points (rotating calipers over the hull edges) →
/// 4 corners.
pub fn min_area_rect(points: &[[f64; 2]]) -> Option<[[f64; 2]; 4]> {
    // `new Map(points.map(([x, y]) => [Math.round(x) + ',' + Math.round(y), [x, y]]))`: one point per rounded key, the
    // last one seen, in the order the keys first appeared
    let mut order: Vec<(i64, i64)> = Vec::new();
    let mut by: HashMap<(i64, i64), [f64; 2]> = HashMap::new();
    for &[x, y] in points {
        let k = (key_i(m::round(x)), key_i(m::round(y)));
        if by.insert(k, [x, y]).is_none() {
            order.push(k);
        }
    }
    let mut p: Vec<[f64; 2]> = order.iter().filter_map(|k| by.get(k).copied()).collect();
    p.sort_by(|a, b| {
        let d = a[0] - b[0];
        let d = if d != 0.0 && !d.is_nan() { d } else { a[1] - b[1] };
        d.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
    });
    if p.len() < 3 {
        return None;
    }
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut lo: Vec<[f64; 2]> = Vec::new();
    for &q in &p {
        while let [.., a, b] = lo[..] {
            if cross(a, b, q) <= 0.0 {
                lo.pop();
            } else {
                break;
            }
        }
        lo.push(q);
    }
    let mut hi: Vec<[f64; 2]> = Vec::new();
    for &q in p.iter().rev() {
        while let [.., a, b] = hi[..] {
            if cross(a, b, q) <= 0.0 {
                hi.pop();
            } else {
                break;
            }
        }
        hi.push(q);
    }
    lo.pop();
    hi.pop();
    let h: Vec<[f64; 2]> = lo.into_iter().chain(hi).collect();
    if h.len() < 3 {
        return None;
    }
    let mut best: Option<(f64, f64, f64, f64, f64, f64, f64)> = None;
    for (i, &a) in h.iter().enumerate() {
        let b = h.get(m::nx(i, h.len())).copied().unwrap_or(a);
        let l = m::hypot(a[0] - b[0], a[1] - b[1]);
        if l < 1e-9 {
            continue;
        }
        let (ux, uy) = ((b[0] - a[0]) / l, (b[1] - a[1]) / l);
        let (mut min_u, mut max_u, mut min_v, mut max_v) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
        for q in &h {
            let u = q[0] * ux + q[1] * uy;
            let v = -q[0] * uy + q[1] * ux;
            min_u = m::min(min_u, u);
            max_u = m::max(max_u, u);
            min_v = m::min(min_v, v);
            max_v = m::max(max_v, v);
        }
        let area = (max_u - min_u) * (max_v - min_v);
        if best.is_none_or(|bb| area < bb.0) {
            best = Some((area, ux, uy, min_u, max_u, min_v, max_v));
        }
    }
    let (_, ux, uy, min_u, max_u, min_v, max_v) = best?;
    let at = |u: f64, v: f64| [u * ux - v * uy, u * uy + v * ux];
    Some([at(min_u, min_v), at(max_u, min_v), at(max_u, max_v), at(min_u, max_v)])
}
#[allow(clippy::cast_possible_truncation)]
fn key_i(v: f64) -> i64 {
    if v.is_finite() { v.clamp(-9e15, 9e15) as i64 } else { i64::MIN }
}

/// `wallFaces`' faces of one wall from its mesh's plan points (model mm): the two long sides of its minimum
/// rectangle, normals away from its centre.
pub fn wall_faces(points: &[[f64; 2]]) -> Vec<Face> {
    let Some([p0, p1, p2, p3]) = min_area_rect(points) else { return Vec::new() };
    let d = |a: [f64; 2], b: [f64; 2]| m::hypot(a[0] - b[0], a[1] - b[1]);
    let sides = if d(p0, p1) >= d(p1, p2) { [(p0, p1), (p2, p3)] } else { [(p1, p2), (p3, p0)] };
    let (cx, cy) = ((p0[0] + p2[0]) / 2.0, (p0[1] + p2[1]) / 2.0);
    let mut out = Vec::new();
    for (a, b) in sides {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l = m::hypot(dx, dy);
        if l < 1.0 {
            continue;
        }
        let mut n = [-dy / l, dx / l];
        if (a[0] - cx) * n[0] + (a[1] - cy) * n[1] < 0.0 {
            n = [-n[0], -n[1]];
        }
        out.push((a, b, n));
    }
    out
}

/// One part of CC-RENDER-PARTS: `^\s*(\d{1,7})-(\d{1,7}) (#[0-9a-fA-F]{6})(?:\/(0(?:\.\d+)?|1(?:\.0+)?))?\s*$`
fn parse_part(s: &str) -> Option<(usize, usize, [f64; 3], f64)> {
    let t = s.trim_matches(|c: char| c.is_whitespace());
    let digits = |x: &str| !x.is_empty() && x.len() <= 7 && x.bytes().all(|b| b.is_ascii_digit());
    let (range, rest) = t.split_once(' ')?;
    let (a, b) = range.split_once('-')?;
    if !digits(a) || !digits(b) {
        return None;
    }
    let (hex, alpha) = match rest.split_once('/') {
        Some((h, al)) => (h, Some(al)),
        None => (rest, None),
    };
    if hex.len() != 7 || !hex.starts_with('#') || !hex.bytes().skip(1).all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let tr = match alpha {
        None => 0.0,
        Some(al) => {
            let ok0 = al == "0" || (al.starts_with("0.") && al.len() > 2 && al.bytes().skip(2).all(|c| c.is_ascii_digit()));
            let ok1 = al == "1" || (al.starts_with("1.") && al.len() > 2 && al.bytes().skip(2).all(|c| c == b'0'));
            if !ok0 && !ok1 {
                return None;
            }
            m::round((1.0 - crate::js::str_num(al)) * 100.0) / 100.0
        }
    };
    let ch = |o: usize| -> f64 {
        let v = hex.get(o..o.saturating_add(2)).and_then(|x| u8::from_str_radix(x, 16).ok()).map_or(0.0, f64::from);
        m::round(v / 255.0 * 1e4) / 1e4
    };
    Some((to_usize(crate::js::str_num(a)), to_usize(crate::js::str_num(b)), [ch(1), ch(3), ch(5)], tr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_parse_as_the_regex_does() {
        assert_eq!(parse_part(" 0-11 #ff8000/0.5 "), Some((0, 11, [1.0, 0.502, 0.0], 0.5)));
        assert_eq!(parse_part("3-4 #000000"), Some((3, 4, [0.0, 0.0, 0.0], 0.0)));
        assert_eq!(parse_part("3-4 #00000"), None);
        assert_eq!(parse_part("3-4 #000000/2"), None);
        assert_eq!(parse_part("12345678-4 #000000"), None);
    }

    #[test]
    fn plan_bounds_and_box_and_closed_meshes() {
        let plan: J = serde_json::json!({ "entities": [
            { "type": "line", "x1": -100, "y1": -50, "x2": 100, "y2": 0 },
            { "type": "circle", "x": 0, "y": -25, "r": 10 },
            { "type": "text", "x": 500, "y": 500, "value": "far" },
        ] });
        assert_eq!(plan_bounds(Some(&plan)), Some([-100.0, -50.0, 500.0, 500.0]));
        let b = box_from_plan(Some(&plan), 900.0, 0.0).unwrap();
        assert_eq!((b.w, b.d, b.h), (600.0, 550.0, 900.0));
        let (_, ix) = box_mesh(&b);
        assert!(is_closed(&ix));
        assert!(!is_closed(&ix[..ix.len() - 3]));
        assert_eq!(parse_int(" 12a"), Some(12.0));
        assert_eq!(parse_int("A1"), None);
    }

    #[test]
    fn placing_shares_types_undoes_and_redoes() {
        use crate::draw::editor::Editor;
        use crate::draw::newmodel::{NewBuilding, new_building, stack_levels};
        use crate::wapi::Rand;
        let nb = NewBuilding {
            name: "Place".into(),
            site_name: "Site".into(),
            levels: stack_levels(2, 3000.0),
            author: String::new(),
            file_name: "p.ifc".into(),
            now_ms: 1_791_979_200_000.0,
            version: String::new(),
        };
        let text = new_building(&nb, &mut Rand::new(7)).unwrap();
        let mut ed = Editor::open(std::sync::Arc::from(text.into_bytes().into_boxed_slice()), 7).unwrap();
        let storey = ed.api.ids("IFCBUILDINGSTOREY", false)[0];
        let item = std::sync::Arc::new(serde_json::json!({
            "format": "cadcraft-library-item", "format_version": 1, "name": "Table", "uuid": "u-1", "version": 2,
            "ifc": { "class": "IfcFurniture" },
            "body": { "type": "box", "x": -500, "y": -800, "z": 0, "w": 1000, "d": 800, "h": 750 },
            "plan": { "entities": [{ "type": "circle", "x": 0, "y": -400, "r": 300 }] },
            "params": { "width": 1000, "elec": { "volts": "120" } }
        }));
        let o =
            |x: f64, no: &str| PlaceOpts { storey, x, y: 0.0, rot: 90.0, item_number: no.into(), quantity: 1.0, ..PlaceOpts::default() };
        assert_eq!(next_item_number(&mut ed.api).unwrap(), "1");
        let a = ed.place_item(item.clone(), o(0.0, "1")).unwrap();
        assert!(a.type_created);
        let b = ed.place_item(item.clone(), o(2000.0, "4")).unwrap();
        assert!(!b.type_created, "one type per uuid@version");
        assert_eq!(b.type_id, a.type_id);
        assert_eq!(next_item_number(&mut ed.api).unwrap(), "5");
        assert_eq!(ed.api.ids("IFCFURNITURE", false).len(), 2);
        assert_eq!(ed.changes.last().map(|c| c.label().to_string()).unwrap(), "Table (item 4)");
        ed.undo().unwrap();
        assert_eq!(ed.api.ids("IFCFURNITURE", false).len(), 1);
        ed.redo().unwrap();
        assert_eq!(ed.api.ids("IFCFURNITURE", false).len(), 2);
        // a storey that is not one: refused, nothing written
        let n = ed.api.f.max_id;
        assert!(ed.place_item(item, PlaceOpts { storey: a.id, ..o(0.0, "9") }).is_err());
        assert_eq!(ed.api.f.max_id, n);
    }

    #[test]
    fn hostile_meshes_are_refused_not_panicked_on() {
        let bad = serde_json::json!({ "type": "mesh", "positions": [0, 0, 0, 1, 0, 0, 0, 1, 0], "indices": [0, 1, 7] });
        assert!(body_of(Some(&bad)).is_err());
        let frac = serde_json::json!({ "type": "mesh", "positions": [0, 0, 0], "indices": [0.5, 0, 0] });
        assert!(body_of(Some(&frac)).is_err());
        let odd = serde_json::json!({ "entities": [{ "type": "hatch", "loops": [[0, 0, 10, 0, 10]] }, { "type": "poly", "pts": [[0, 0, 1e300]], "closed": true }] });
        let _ = plan_polylines(Some(&odd));
        let _ = plan_bounds(Some(&odd));
    }
}
