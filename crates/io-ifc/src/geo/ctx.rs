//! geo-ctx.js — what every geometry module needs from an open file: attributes by name, numbers and references out of
//! raw values, the project's units, and placements as matrices (IFC4 ADD2 TC1 §8.18 Geometry resource:
//! IfcAxis2Placement2D/3D, IfcAxis1Placement, IfcCartesianTransformationOperator2D/3D (+ NonUniform); §8.17
//! IfcLocalPlacement, IfcGridPlacement). Matrices are column-major 4 × 4, in the file's length unit.
//!
//! Where the JS would throw (a parse error of a line it reads, `null[0]`, a call stack running out), these functions
//! return `Err(GeoErr)`: the caller catches it exactly where the JS has its `try` (geometry.rs `item`).

use std::borrow::Cow;
use std::collections::HashMap;
use std::rc::Rc;

use super::align::{SegCurve, linear_placement};
use super::math::{I4, M4, V3, cross, dot, frame, is_zero, mul, norm, perp, scale, sub};
use crate::model::{Id, IfcFile, id_key};
use crate::schema::Schema;
use crate::step::StepError;
use crate::value::Value;

/// What the JS throws (its message; only whether something threw decides the output).
#[derive(Clone, Debug, PartialEq)]
pub struct GeoErr(pub String);

impl From<StepError> for GeoErr {
    fn from(e: StepError) -> GeoErr {
        GeoErr(e.0)
    }
}
impl std::fmt::Display for GeoErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub type GR<T> = Result<T, GeoErr>;

/// `null[k]` / `undefined[k]`
pub fn type_error_read(what: &str, k: &str) -> GeoErr {
    GeoErr(format!("Cannot read properties of {what} (reading '{k}')"))
}
/// `x.map(…)` on something that is not an array
pub fn not_a_function(f: &str) -> GeoErr {
    GeoErr(format!("x.{f} is not a function"))
}
pub fn stack_overflow() -> GeoErr {
    GeoErr("Maximum call stack size exceeded".into())
}

/// `Array.isArray(v) ? v : null`
#[inline]
pub fn arr(v: Option<&Value>) -> Option<&[Value]> {
    match v {
        Some(Value::List(l)) => Some(l),
        _ => None,
    }
}

/// JS truthiness of a raw value (undefined → false).
#[inline]
pub fn truthy(v: Option<&Value>) -> bool {
    v.is_some_and(Value::truthy)
}

/// `(v || []).map(…)`: falsy → []; an array → its items; anything else throws (not a function).
pub fn list_or_empty(v: Option<&Value>) -> GR<&[Value]> {
    match v {
        Some(Value::List(l)) => Ok(l),
        x if !truthy(x) => Ok(&[]),
        _ => Err(not_a_function("map")),
    }
}

/// `v[k]` for an element read out of a list: null / undefined throw; an array gives its item; anything else
/// (numbers, objects, strings) gives undefined here (callers only take numbers or references out of it).
pub fn idx(v: Option<&Value>, k: usize) -> GR<Option<&Value>> {
    match v {
        None => Err(type_error_read("undefined", &k.to_string())),
        Some(Value::Null) => Err(type_error_read("null", &k.to_string())),
        Some(Value::List(l)) => Ok(l.get(k)),
        Some(_) => Ok(None),
    }
}

/// `g.num(v, d)`: a number, or a typed value's number; else d.
pub fn num(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Num(n)) => Some(*n),
        Some(Value::Typed(t)) => num(Some(&t.1)),
        _ => None,
    }
}
#[inline]
pub fn numd(v: Option<&Value>, d: f64) -> f64 {
    num(v).unwrap_or(d)
}
/// `g.ref(v)`
#[inline]
pub fn ref_(v: Option<&Value>) -> Option<Id> {
    match v {
        Some(Value::Ref(r)) => Some(*r),
        _ => None,
    }
}
/// `g.refs(v)`: the references of an array (others dropped); [] for anything else.
pub fn refs(v: Option<&Value>) -> Vec<Id> {
    match v {
        Some(Value::List(l)) => l.iter().filter_map(Value::as_ref_id).collect(),
        _ => Vec::new(),
    }
}
/// `g.str(v)`: a string, a typed value's string, an enumeration's name.
pub fn str_(v: Option<&Value>) -> Option<Cow<'_, str>> {
    match v {
        Some(Value::Str(s)) => Some(s.lossy()),
        Some(Value::Typed(t)) => str_(Some(&t.1)),
        Some(Value::Enum(e)) => Some(Cow::Borrowed(e)),
        _ => None,
    }
}
/// `g.enm(v)`
#[inline]
pub fn enm(v: Option<&Value>) -> Option<&str> {
    match v {
        Some(Value::Enum(e)) => Some(e),
        _ => None,
    }
}
/// `g.bool(v, d)`: .T. / .F. (also inside a typed value); else d.
pub fn bool_(v: Option<&Value>, d: Option<bool>) -> Option<bool> {
    let inner = match v {
        Some(Value::Typed(t)) => Some(&t.1),
        x => x,
    };
    match enm(inner) {
        Some("T") => Some(true),
        Some("F") => Some(false),
        _ => d,
    }
}

/// The call depth past which the JS engine's stack runs out on a placement loop (grid placements restart the depth
/// counter the JS keeps itself).
const MAX_CALL_DEPTH: u32 = 2_000;

pub struct GeoCtx<'f> {
    pub f: &'f IfcFile<'f>,
    pub s: &'static Schema,
    pub places: HashMap<u64, M4>,
    pub notes: HashMap<String, u32>,
    pub length_unit: f64,
    pub angle_unit: f64,
    calls: u32,
    pub(crate) seg_cache: HashMap<u64, Rc<SegCurve>>,
    pub(crate) seg_depth: u32,
}

impl<'f> GeoCtx<'f> {
    pub fn new(f: &'f IfcFile<'f>) -> GR<GeoCtx<'f>> {
        let mut g = GeoCtx {
            f,
            s: f.schema,
            places: HashMap::new(),
            notes: HashMap::new(),
            length_unit: 1.0,
            angle_unit: 1.0,
            calls: 0,
            seg_cache: HashMap::new(),
            seg_depth: 0,
        };
        g.units()?;
        Ok(g)
    }

    /// `g.t(id)`: the line's type ("" for a complex instance), None for no line.
    #[inline]
    pub fn t(&self, id: Option<Id>) -> Option<&'f str> {
        self.f.ty(id?)
    }
    /// The type when it is a non-empty name (JS truthiness of `g.t(id)`).
    #[inline]
    pub fn tt(&self, id: Option<Id>) -> Option<&'f str> {
        self.t(id).filter(|t| !t.is_empty())
    }
    pub fn is_a(&self, id: Option<Id>, up: &str) -> bool {
        self.tt(id).is_some_and(|t| self.s.is_a(t, up))
    }
    /// `g.a(id, name)`: Ok(None) = undefined; Err = the line does not parse.
    #[inline]
    pub fn a(&self, id: Option<Id>, name: &str) -> GR<Option<&'f Value>> {
        match id {
            None => Ok(None),
            Some(id) => Ok(self.f.attr(id, name)?),
        }
    }
    /// `g.ref(g.a(id, name))`
    #[inline]
    pub fn ra(&self, id: Option<Id>, name: &str) -> GR<Option<Id>> {
        Ok(ref_(self.a(id, name)?))
    }
    /// `g.num(g.a(id, name), d)`
    #[inline]
    pub fn na(&self, id: Option<Id>, name: &str, d: f64) -> GR<f64> {
        Ok(numd(self.a(id, name)?, d))
    }
    pub fn pretty(&self, t: &str) -> String {
        self.s.pretty(t).unwrap_or("null").to_string()
    }

    pub fn note(&mut self, text: impl Into<String>) {
        let n = self.notes.entry(text.into()).or_insert(0);
        *n = n.saturating_add(1);
    }

    // ---- units ----

    fn units(&mut self) -> GR<()> {
        let f = self.f;
        let mut ua: Option<Id> = None;
        let proj = f.of_type("IFCPROJECT", true).first().copied().or_else(|| f.of_type("IFCCONTEXT", true).first().copied());
        if let Some(p) = proj {
            ua = self.ra(Some(p), "UnitsInContext")?;
        }
        if ua.is_none() {
            ua = f.of_type("IFCUNITASSIGNMENT", false).first().copied();
        }
        if ua.is_none() {
            return Ok(());
        }
        for u in refs(self.a(ua, "Units")?) {
            let kind = enm(self.a(Some(u), "UnitType")?);
            if kind == Some("LENGTHUNIT") {
                self.length_unit = self.unit_factor(Some(u), "METRE", 0)?.unwrap_or(1.0);
            }
            if kind == Some("PLANEANGLEUNIT") {
                self.angle_unit = self.unit_factor(Some(u), "RADIAN", 0)?.unwrap_or(1.0);
            }
        }
        Ok(())
    }

    /// The SI value of one of `u` (SI base unit with prefix, or a conversion-based unit through its factor).
    fn unit_factor(&self, u: Option<Id>, base: &str, depth: u32) -> GR<Option<f64>> {
        if depth > 8 || u.is_none() {
            return Ok(None);
        }
        let t = self.t(u);
        if t == Some("IFCSIUNIT") {
            let p = enm(self.a(u, "Prefix")?);
            return Ok(Some(match p {
                Some(p) if !p.is_empty() => prefix(p).unwrap_or(1.0),
                _ => 1.0,
            }));
        }
        if t == Some("IFCCONVERSIONBASEDUNIT") || t == Some("IFCCONVERSIONBASEDUNITWITHOFFSET") {
            let m = self.ra(u, "ConversionFactor")?;
            if m.is_some() {
                let v = num(self.a(m, "ValueComponent")?);
                let inner = self.unit_factor(self.ra(m, "UnitComponent")?, base, depth.saturating_add(1))?.unwrap_or(1.0);
                if let Some(v) = v
                    && v != 0.0
                    && v.is_finite()
                {
                    return Ok(Some(v * inner));
                }
            }
            let n = str_(self.a(u, "Name")?).map(|s| s.to_uppercase()).unwrap_or_default();
            if base == "RADIAN" && n.contains("DEGREE") {
                return Ok(Some(std::f64::consts::PI / 180.0));
            }
            if n.contains("INCH") {
                return Ok(Some(0.0254));
            }
            if n.contains("FOOT") || n.contains("FEET") {
                return Ok(Some(0.3048));
            }
        }
        Ok(None)
    }

    // ---- points and directions ----

    pub fn point(&self, id: Option<Id>) -> GR<V3> {
        if id.is_none() {
            return Ok([0.0; 3]);
        }
        let Some(c) = arr(self.a(id, "Coordinates")?) else { return Ok([0.0; 3]) };
        Ok([numd(c.first(), 0.0), numd(c.get(1), 0.0), numd(c.get(2), 0.0)])
    }

    pub fn dir(&self, id: Option<Id>, d: Option<V3>) -> GR<Option<V3>> {
        if id.is_none() {
            return Ok(d);
        }
        let Some(r) = arr(self.a(id, "DirectionRatios")?) else { return Ok(d) };
        let v = [numd(r.first(), 0.0), numd(r.get(1), 0.0), numd(r.get(2), 0.0)];
        let n = norm(&v);
        Ok(if !is_zero(&n) { Some(n) } else { d })
    }
    /// `g.dir(id, d)` with a vector default.
    pub fn dirv(&self, id: Option<Id>, d: V3) -> GR<V3> {
        Ok(self.dir(id, Some(d))?.unwrap_or(d))
    }

    /// IfcVector → [x, y, z] with its magnitude.
    pub fn vector(&self, id: Option<Id>) -> GR<V3> {
        let d = self.dirv(self.ra(id, "Orientation")?, [1.0, 0.0, 0.0])?;
        Ok(scale(&d, self.na(id, "Magnitude", 1.0)?))
    }

    // ---- placements ----

    /// IfcAxis2Placement3D / 2D / IfcAxis1Placement → matrix (2D placements lie in the XY plane).
    pub fn axis2(&mut self, id: Option<Id>) -> GR<M4> {
        if id.is_none() {
            return Ok(I4);
        }
        let t = self.t(id);
        let o = self.point(self.ra(id, "Location")?)?;
        match t {
            Some("IFCAXIS2PLACEMENT2D") => {
                let x = self.dirv(self.ra(id, "RefDirection")?, [1.0, 0.0, 0.0])?;
                let mut x = norm(&[x[0], x[1], 0.0]);
                if super::math::falsy(x[0]) && super::math::falsy(x[1]) {
                    x = [1.0, 0.0, 0.0];
                }
                Ok(frame(&[o[0], o[1], 0.0], &x, &[-x[1], x[0], 0.0], &[0.0, 0.0, 1.0]))
            }
            Some("IFCAXIS1PLACEMENT") => {
                let z = self.dirv(self.ra(id, "Axis")?, [0.0, 0.0, 1.0])?;
                let x = perp(&z);
                Ok(frame(&o, &x, &cross(&z, &x), &z))
            }
            Some("IFCAXIS2PLACEMENT3D") => {
                let z = self.dirv(self.ra(id, "Axis")?, [0.0, 0.0, 1.0])?;
                let x = self.dir(self.ra(id, "RefDirection")?, None)?;
                let x = first_proj_axis(&z, x);
                Ok(frame(&o, &x, &cross(&z, &x), &z))
            }
            Some("IFCAXIS2PLACEMENTLINEAR") => {
                self.note("Linear placements are drawn at their start point");
                Ok(I4)
            }
            _ => Ok(I4),
        }
    }

    /// IfcCartesianTransformationOperator2D/3D (+ NonUniform): IFC4 IfcBaseAxis / IfcFirstProjAxis / IfcSecondProjAxis.
    pub fn operator(&self, id: Option<Id>) -> GR<M4> {
        if id.is_none() {
            return Ok(I4);
        }
        let t = self.t(id);
        let two = t == Some("IFCCARTESIANTRANSFORMATIONOPERATOR2D") || t == Some("IFCCARTESIANTRANSFORMATIONOPERATOR2DNONUNIFORM");
        let o = self.point(self.ra(id, "LocalOrigin")?)?;
        let s1 = self.na(id, "Scale", 1.0)?;
        let s2 = self.na(id, "Scale2", s1)?;
        let s3 = self.na(id, "Scale3", s1)?;
        let a1 = self.dir(self.ra(id, "Axis1")?, None)?;
        let a2 = self.dir(self.ra(id, "Axis2")?, None)?;
        if two {
            let x = match a1 {
                Some(a) => norm(&[a[0], a[1], 0.0]),
                None => [1.0, 0.0, 0.0],
            };
            let y = match a2 {
                Some(a) => norm(&[a[0], a[1], 0.0]),
                None => [-x[1], x[0], 0.0],
            };
            return Ok([x[0] * s1, x[1] * s1, 0.0, 0.0, y[0] * s2, y[1] * s2, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, o[0], o[1], 0.0, 1.0]);
        }
        let z = self.dirv(self.ra(id, "Axis3")?, [0.0, 0.0, 1.0])?;
        let x = first_proj_axis(&z, a1);
        let y = match a2 {
            Some(a2) => {
                let v = sub(&a2, &scale(&z, dot(&a2, &z)));
                let y = norm(&sub(&v, &scale(&x, dot(&v, &x))));
                if is_zero(&y) { cross(&z, &x) } else { y }
            }
            None => cross(&z, &x),
        };
        Ok([
            x[0] * s1,
            x[1] * s1,
            x[2] * s1,
            0.0,
            y[0] * s2,
            y[1] * s2,
            y[2] * s2,
            0.0,
            z[0] * s3,
            z[1] * s3,
            z[2] * s3,
            0.0,
            o[0],
            o[1],
            o[2],
            1.0,
        ])
    }

    /// World matrix of an IfcObjectPlacement (IfcLocalPlacement chain, IfcGridPlacement). Cached; cycles stop.
    pub fn placement(&mut self, id: Option<Id>, depth: u32) -> GR<M4> {
        let Some(pid) = id else { return Ok(I4) };
        if let Some(hit) = self.places.get(&id_key(pid)) {
            return Ok(*hit);
        }
        if depth > 64 {
            self.note("A placement chain loops");
            return Ok(I4);
        }
        if self.calls >= MAX_CALL_DEPTH {
            return Err(stack_overflow());
        }
        self.calls = self.calls.saturating_add(1);
        let r = self.placement_inner(id, depth);
        self.calls = self.calls.saturating_sub(1);
        let m = r?;
        self.places.insert(id_key(pid), m);
        Ok(m)
    }

    fn placement_inner(&mut self, id: Option<Id>, depth: u32) -> GR<M4> {
        let t = self.t(id);
        let d1 = depth.saturating_add(1);
        if t == Some("IFCLOCALPLACEMENT") {
            let rel = self.ra(id, "PlacementRelTo")?;
            let local = self.axis2(self.ra(id, "RelativePlacement")?)?;
            return Ok(match rel {
                Some(_) => mul(&self.placement(rel, d1)?, &local),
                None => local,
            });
        }
        if t == Some("IFCGRIDPLACEMENT") {
            return self.grid_placement(id);
        }
        if t == Some("IFCLINEARPLACEMENT") {
            let rp = self.ra(id, "RelativePlacement")?;
            if self.t(rp) == Some("IFCAXIS2PLACEMENTLINEAR") && linear_placement(self, rp)?.is_some() {
                // a station along an alignment, with its offsets (the alignment curve is in the frame PlacementRelTo gives)
                let rel = self.ra(id, "PlacementRelTo")?;
                let local = linear_placement(self, self.ra(id, "RelativePlacement")?)?.unwrap_or(I4);
                return Ok(match rel {
                    Some(_) => mul(&self.placement(rel, d1)?, &local),
                    None => local,
                });
            }
        }
        if let Some(t) = t.filter(|t| !t.is_empty()) {
            if self.s.entity(t).is_some_and(|e| e.attr_index("CartesianPosition").is_some()) {
                // IFC4X3 linear placements carry their position as a plain placement too
                let cp = self.ra(id, "CartesianPosition")?;
                let rel = self.ra(id, "PlacementRelTo")?;
                let local = if cp.is_some() { self.axis2(cp)? } else { I4 };
                let m = match rel {
                    Some(_) => mul(&self.placement(rel, d1)?, &local),
                    None => local,
                };
                if cp.is_none() {
                    self.note("A linear placement without a Cartesian position is drawn at the origin");
                }
                return Ok(m);
            }
            let p = self.pretty(t);
            self.note(format!("{p} placements are not read yet; drawn at the origin"));
        }
        Ok(I4)
    }

    /// IfcGridPlacement: the intersection of two grid axes (straight axis curves) offset by IfcVirtualGridIntersection,
    /// in the coordinate system of the IfcGrid the axes belong to (its ObjectPlacement; else PlacementRelTo).
    fn grid_placement(&mut self, id: Option<Id>) -> GR<M4> {
        let vgi = self.ra(id, "PlacementLocation")?;
        if vgi.is_none() {
            return Ok(I4);
        }
        let axes = refs(self.a(vgi, "IntersectingAxes")?);
        let offs: Vec<f64> = list_or_empty(self.a(vgi, "OffsetDistances")?)?.iter().map(|v| numd(Some(v), 0.0)).collect();
        if axes.len() < 2 {
            return Ok(I4);
        }
        let line = |g: &Self, ax: Id| -> GR<Option<(V3, V3)>> {
            let c = g.ra(Some(ax), "AxisCurve")?;
            let ct = g.t(c);
            let pts: Option<Vec<V3>> = if ct == Some("IFCPOLYLINE") {
                let mut v = Vec::new();
                for p in refs(g.a(c, "Points")?) {
                    v.push(g.point(Some(p))?);
                }
                Some(v)
            } else if ct == Some("IFCLINE") {
                let o = g.point(g.ra(c, "Pnt")?)?;
                let v = g.vector(g.ra(c, "Dir")?)?;
                Some(vec![o, [o[0] + v[0], o[1] + v[1], o[2] + v[2]]])
            } else {
                None
            };
            let Some(pts) = pts.filter(|p| p.len() >= 2) else { return Ok(None) };
            let (Some(first), Some(last)) = (pts.first(), pts.last()) else { return Ok(None) };
            let mut d = norm(&sub(last, first));
            if bool_(g.a(Some(ax), "SameSense")?, Some(true)) == Some(false) {
                d = scale(&d, -1.0);
            }
            Ok(Some((*first, d)))
        };
        let (Some(&ax0), Some(&ax1)) = (axes.first(), axes.get(1)) else { return Ok(I4) };
        let a = line(self, ax0)?;
        let b = line(self, ax1)?;
        let (Some((ap, ad)), Some((bp, bd))) = (a, b) else {
            self.note("Grid placements on curved axes are drawn at the origin");
            return Ok(I4);
        };
        // offsets move each axis to its left (seen along the axis), the third one up
        let off = |k: usize| super::math::or(offs.get(k).copied().unwrap_or(0.0), 0.0);
        let n_a = [-ad[1], ad[0], 0.0];
        let n_b = [-bd[1], bd[0], 0.0];
        let pa = [ap[0] + n_a[0] * off(0), ap[1] + n_a[1] * off(0)];
        let pb = [bp[0] + n_b[0] * off(1), bp[1] + n_b[1] * off(1)];
        let den = ad[0] * bd[1] - ad[1] * bd[0];
        if den.abs() < 1e-12 {
            return Ok(I4);
        }
        let ta = ((pb[0] - pa[0]) * bd[1] - (pb[1] - pa[1]) * bd[0]) / den;
        let o = [pa[0] + ad[0] * ta, pa[1] + ad[1] * ta, off(2)];
        let ref_dir = self.ra(id, "PlacementRefDirection")?;
        let mut x = ad;
        if ref_dir.is_some() && self.t(ref_dir) == Some("IFCDIRECTION") {
            x = self.dirv(ref_dir, ad)?;
        } else if ref_dir.is_some() && self.t(ref_dir) == Some("IFCVIRTUALGRIDINTERSECTION") {
            self.note("Grid placements aimed at another grid point face the first axis");
        }
        let x = norm(&[x[0], x[1], 0.0]);
        let local = frame(&o, &x, &[-x[1], x[0], 0.0], &[0.0, 0.0, 1.0]);
        // the grid's own placement
        let mut base: Option<Id> = None;
        for attr in ["UAxes", "VAxes", "WAxes"] {
            let inv = self.f.inverse("IFCGRID", attr)?;
            if let Some(grids) = inv.get(&id_key(ax0))
                && let Some(&g0) = grids.first()
            {
                base = self.ra(Some(g0), "ObjectPlacement")?;
                break;
            }
        }
        if base.is_none() {
            base = self.ra(id, "PlacementRelTo")?;
        }
        Ok(match base {
            Some(b) if Some(b) != id => mul(&self.placement(base, 1)?, &local),
            _ => local,
        })
    }
}

/// The project's map coordinate reference system (IfcProjectedCRS).
#[derive(Clone, Debug, PartialEq)]
pub struct Crs {
    pub name: Option<String>,
    pub description: Option<String>,
    pub geodetic_datum: Option<String>,
    pub vertical_datum: Option<String>,
    pub projection: Option<String>,
    pub zone: Option<String>,
}

/// GeoCtx.georef: IfcMapConversion (+ Scaled) / IfcRigidOperation.
#[derive(Clone, Debug, PartialEq)]
pub struct GeoRef {
    pub kind: &'static str,
    pub crs: Option<Crs>,
    pub eastings: f64,
    pub northings: f64,
    pub height: f64,
    pub x_axis: [f64; 2],
    /// radians from map east to the model's x axis
    pub rotation: f64,
    pub scale: f64,
    /// model units → map
    pub matrix: M4,
}

impl GeoCtx<'_> {
    /// The project's georeference, or None.
    pub fn georef(&self) -> GR<Option<GeoRef>> {
        let f = self.f;
        let Some(op) =
            f.of_type("IFCMAPCONVERSION", true).first().copied().or_else(|| f.of_type("IFCRIGIDOPERATION", true).first().copied())
        else {
            return Ok(None);
        };
        let op = Some(op);
        let t = self.t(op);
        let crs_id = self.ra(op, "TargetCRS")?;
        let s = |id: Option<Id>, n: &str| -> GR<Option<String>> { Ok(str_(self.a(id, n)?).map(|x| x.into_owned())) };
        let crs = match crs_id {
            None => None,
            Some(_) => Some(Crs {
                name: s(crs_id, "Name")?,
                description: s(crs_id, "Description")?,
                geodetic_datum: s(crs_id, "GeodeticDatum")?,
                vertical_datum: s(crs_id, "VerticalDatum")?,
                projection: s(crs_id, "MapProjection")?,
                zone: s(crs_id, "MapZone")?,
            }),
        };
        if t == Some("IFCRIGIDOPERATION") {
            let (e, n, h) = (self.na(op, "FirstCoordinate", 0.0)?, self.na(op, "SecondCoordinate", 0.0)?, self.na(op, "Height", 0.0)?);
            return Ok(Some(GeoRef {
                kind: "rigid",
                crs,
                eastings: e,
                northings: n,
                height: h,
                x_axis: [1.0, 0.0],
                rotation: 0.0,
                scale: 1.0,
                matrix: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, e, n, h, 1.0],
            }));
        }
        let (e, n, h) = (self.na(op, "Eastings", 0.0)?, self.na(op, "Northings", 0.0)?, self.na(op, "OrthogonalHeight", 0.0)?);
        let mut ax = self.na(op, "XAxisAbscissa", 1.0)?;
        let mut ay = self.na(op, "XAxisOrdinate", 0.0)?;
        let l = super::math::or(crate::jsmath::hypot2(ax, ay), 1.0);
        ax /= l;
        ay /= l;
        let sc = self.na(op, "Scale", 1.0)?;
        // Scale is the unit difference between the model and the map (IFC4), so heights take it too; only
        // IfcMapConversionScaled (IFC4X3) names its own FactorY / FactorZ
        let sy = self.na(op, "FactorY", sc)?;
        let sz = self.na(op, "FactorZ", sc)?;
        let m = [ax * sc, ay * sc, 0.0, 0.0, -ay * sy, ax * sy, 0.0, 0.0, 0.0, 0.0, sz, 0.0, e, n, h, 1.0];
        use crate::jsmath::JsMath;
        Ok(Some(GeoRef {
            kind: "map-conversion",
            crs,
            eastings: e,
            northings: n,
            height: h,
            x_axis: [ax, ay],
            rotation: ay.jatan2(ax),
            scale: sc,
            matrix: m,
        }))
    }
}

/// IFC IfcFirstProjAxis: X projected into the plane normal to Z (default (1,0,0), or (0,1,0) when Z is X).
pub fn first_proj_axis(z: &V3, x: Option<V3>) -> V3 {
    let v = x.unwrap_or(if z[0].abs() > 0.999999 && z[1].abs() < 1e-6 && z[2].abs() < 1e-6 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] });
    let p = sub(&v, &scale(z, dot(&v, z)));
    let n = norm(&p);
    if is_zero(&n) { perp(z) } else { n }
}

fn prefix(p: &str) -> Option<f64> {
    Some(match p {
        "EXA" => 1e18,
        "PETA" => 1e15,
        "TERA" => 1e12,
        "GIGA" => 1e9,
        "MEGA" => 1e6,
        "KILO" => 1e3,
        "HECTO" => 1e2,
        "DECA" => 1e1,
        "DECI" => 1e-1,
        "CENTI" => 1e-2,
        "MILLI" => 1e-3,
        "MICRO" => 1e-6,
        "NANO" => 1e-9,
        "PICO" => 1e-12,
        "FEMTO" => 1e-15,
        "ATTO" => 1e-18,
        _ => return None,
    })
}
