//! gis-geom.js — GIS feature geometry: rings, polygons from shapefile rings, winding, bounds, WKB read and write.
//!
//! geometry = null | `{t: 'pt', c: [x, y, z?]}` | `{t: 'ln', c: [part…]}` | `{t: 'pg', c: [[outer, hole…]…]}` —
//! [`cc_civil::gis_state::Geom`].

use crate::gjs::{f64_at, put, u8_at, u32_at};
use cc_civil::gis_state::{Feature, Field, Geom, Pos};
use cc_civil::i18n::{F, t};
use cc_civil::js::{G, J, Obj};
use cc_civil::tr;
use serde_json::Value;

pub type Ring = Vec<Pos>;

fn px(p: &Pos) -> f64 {
    p.g(0)
}
fn py(p: &Pos) -> f64 {
    p.g(1)
}

/// `ringArea(r)` (counter-clockwise +).
pub fn ring_area(r: &[Pos]) -> f64 {
    let mut a = 0.0;
    let n = r.len();
    for i in 0..n {
        let p = r.get(i).map_or(&[][..], Vec::as_slice);
        let q = r.get(i.wrapping_add(1).checked_rem(n).unwrap_or(0)).map_or(&[][..], Vec::as_slice);
        a += p.g(0) * q.g(1) - q.g(0) * p.g(1);
    }
    a / 2.0
}
/// `pointInRing(x, y, r)`.
pub fn point_in_ring(x: f64, y: f64, r: &[Pos]) -> bool {
    let mut inside = false;
    let n = r.len();
    if n == 0 {
        return false;
    }
    let mut j = n.saturating_sub(1);
    for i in 0..n {
        let (Some(a), Some(b)) = (r.get(i), r.get(j)) else { break };
        if (py(a) > y) != (py(b) > y) && x < ((px(b) - px(a)) * (y - py(a))) / (py(b) - py(a)) + px(a) {
            inside = !inside;
        }
        j = i;
    }
    inside
}
/// `closeRing(r)` (in place).
pub fn close_ring(mut r: Ring) -> Ring {
    if let (Some(f), Some(l)) = (r.first(), r.last())
        && (f.g(0) != l.g(0) || f.g(1) != l.g(1))
    {
        let c = f.clone();
        r.push(c);
    }
    r
}

/// `polygonsFromRings(rings)`: shapefile rule (clockwise = outer, counter-clockwise = hole).
pub fn polygons_from_rings(rings: Vec<Ring>) -> Vec<Vec<Ring>> {
    let mut outers: Vec<Ring> = Vec::new();
    let mut holes: Vec<Ring> = Vec::new();
    for r in rings {
        if r.len() < 4 {
            continue;
        }
        if ring_area(&r) < 0.0 {
            outers.push(r);
        } else {
            holes.push(r);
        }
    }
    if outers.is_empty() {
        return holes
            .into_iter()
            .map(|mut h| {
                h.reverse();
                vec![h]
            })
            .collect();
    }
    struct P {
        o: Ring,
        a: f64,
        holes: Vec<Ring>,
    }
    let mut polys: Vec<P> = outers.into_iter().map(|o| P { a: ring_area(&o).abs(), o, holes: Vec::new() }).collect();
    for h in holes {
        let ha = ring_area(&h).abs();
        let mut best: Option<usize> = None;
        let h0 = h.first().cloned().unwrap_or_default();
        for (k, p) in polys.iter().enumerate() {
            if p.a >= ha && point_in_ring(h0.g(0), h0.g(1), &p.o) && best.is_none_or(|b| polys.get(b).is_some_and(|bp| p.a < bp.a)) {
                best = Some(k);
            }
        }
        match best.and_then(|b| polys.get_mut(b)) {
            Some(p) => p.holes.push(h),
            None => {
                let mut o = h;
                o.reverse();
                polys.push(P { a: ha, o, holes: Vec::new() });
            }
        }
    }
    polys
        .into_iter()
        .map(|p| {
            let mut v = vec![p.o];
            v.extend(p.holes);
            v
        })
        .collect()
}

/// `orient(g, outerCCW)`.
pub fn orient(g: &Geom, outer_ccw: bool) -> Geom {
    match g {
        Geom::Pg(c) => Geom::Pg(
            c.iter()
                .map(|poly| {
                    poly.iter()
                        .enumerate()
                        .map(|(i, r)| {
                            let ccw = ring_area(r) > 0.0;
                            let want = if i == 0 { outer_ccw } else { !outer_ccw };
                            if ccw == want {
                                r.clone()
                            } else {
                                let mut q = r.clone();
                                q.reverse();
                                q
                            }
                        })
                        .collect()
                })
                .collect(),
        ),
        _ => g.clone(),
    }
}

/// A box `{minX, minY, maxX, maxY}`.
#[derive(Clone, Copy, Debug)]
pub struct BBox {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}
impl Default for BBox {
    fn default() -> Self {
        BBox { min_x: f64::INFINITY, min_y: f64::INFINITY, max_x: f64::NEG_INFINITY, max_y: f64::NEG_INFINITY }
    }
}
impl J for BBox {
    fn j(&self) -> Value {
        cc_civil::jo! { "minX" => self.min_x, "minY" => self.min_y, "maxX" => self.max_x, "maxY" => self.max_y }
    }
}
impl BBox {
    fn add(&mut self, p: &Pos) {
        let (x, y) = (p.g(0), p.g(1));
        if x < self.min_x {
            self.min_x = x;
        }
        if x > self.max_x {
            self.max_x = x;
        }
        if y < self.min_y {
            self.min_y = y;
        }
        if y > self.max_y {
            self.max_y = y;
        }
    }
}
/// `bounds(g, b)`.
pub fn bounds_into(g: Option<&Geom>, b: &mut BBox) {
    match g {
        None => {}
        Some(Geom::Pt(c)) => b.add(c),
        Some(Geom::Ln(c)) => c.iter().flatten().for_each(|p| b.add(p)),
        Some(Geom::Pg(c)) => {
            for poly in c {
                if let Some(o) = poly.first() {
                    o.iter().for_each(|p| b.add(p));
                }
            }
        }
    }
}
pub fn bounds(g: Option<&Geom>) -> BBox {
    let mut b = BBox::default();
    bounds_into(g, &mut b);
    b
}

/// `mapXY(g, fn)`: z kept.
pub fn map_xy(g: &Geom, f: &mut dyn FnMut(f64, f64) -> (f64, f64)) -> Geom {
    let mut m = |p: &Pos| {
        let (x, y) = f(p.g(0), p.g(1));
        if p.len() > 2 { vec![x, y, p.g(2)] } else { vec![x, y] }
    };
    match g {
        Geom::Pt(c) => Geom::Pt(m(c)),
        Geom::Ln(c) => Geom::Ln(c.iter().map(|part| part.iter().map(&mut m).collect()).collect()),
        Geom::Pg(c) => Geom::Pg(c.iter().map(|poly| poly.iter().map(|r| r.iter().map(&mut m).collect()).collect()).collect()),
    }
}
/// `mapZ(g, fn)`.
pub fn map_z(g: &Geom, f: &dyn Fn(f64) -> f64) -> Geom {
    let m = |p: &Pos| if p.len() > 2 { vec![p.g(0), p.g(1), f(p.g(2))] } else { p.clone() };
    match g {
        Geom::Pt(c) => Geom::Pt(m(c)),
        Geom::Ln(c) => Geom::Ln(c.iter().map(|part| part.iter().map(m).collect()).collect()),
        Geom::Pg(c) => Geom::Pg(c.iter().map(|poly| poly.iter().map(|r| r.iter().map(m).collect()).collect()).collect()),
    }
}
/// `vertexCount(g)`.
pub fn vertex_count(g: Option<&Geom>) -> usize {
    match g {
        None => 0,
        Some(Geom::Pt(_)) => 1,
        Some(Geom::Ln(c)) => c.iter().map(Vec::len).sum(),
        Some(Geom::Pg(c)) => c.iter().map(|p| p.iter().map(Vec::len).sum::<usize>()).sum(),
    }
}

/// `geomKind(type)` → 'pt' | 'ln' | 'pg'.
pub fn geom_kind(ty: &str) -> Option<&'static str> {
    let mut t: String = ty.to_lowercase().chars().filter(|&c| !(cc_civil::js::is_ws(c) || c == '_')).collect();
    for suf in ["zm", "z", "m"] {
        if t.ends_with(suf) {
            t.truncate(t.len().saturating_sub(suf.len()));
            break;
        }
    }
    match t.as_str() {
        "point" | "multipoint" => Some("pt"),
        "linestring" | "multilinestring" | "curve" | "multicurve" | "compoundcurve" => Some("ln"),
        "polygon" | "multipolygon" | "surface" | "multisurface" | "curvepolygon" => Some("pg"),
        _ => None,
    }
}

/// Does a geometry have a z anywhere (the readers' `hasZ`).
pub fn has_z(g: &Geom) -> bool {
    match g {
        Geom::Pt(c) => c.len() > 2,
        Geom::Ln(c) => c.iter().any(|p| p.iter().any(|q| q.len() > 2)),
        Geom::Pg(c) => c.iter().any(|poly| poly.iter().any(|r| r.iter().any(|q| q.len() > 2))),
    }
}

// ---------------------------------------------------------------- WKB

/// A geometry error (the JS GeomError's message).
#[derive(Debug, Clone)]
pub struct GeomError(pub String);

const MAX_N: u64 = 50_000_000;

/// `readWkb(bytes, offset)` → (geoms, end, multipoint).
pub fn read_wkb(b: &[u8], offset: usize) -> Result<(Vec<Option<Geom>>, usize, bool), GeomError> {
    let mut p = offset;
    let mut multipoint = false;
    let geoms = one(b, &mut p, 0, &mut multipoint)?;
    Ok((geoms, p, multipoint))
}
fn need(b: &[u8], p: usize, n: usize) -> Result<(), GeomError> {
    if p.saturating_add(n) > b.len() { Err(GeomError(tr!("geom.short"))) } else { Ok(()) }
}
fn one(b: &[u8], p: &mut usize, depth: u32, multipoint: &mut bool) -> Result<Vec<Option<Geom>>, GeomError> {
    if depth > 8 {
        return Err(GeomError(tr!("geom.deep")));
    }
    need(b, *p, 5)?;
    let order = u8_at(b, *p);
    let le = order == 1;
    if order > 1 {
        return Err(GeomError(tr!("geom.order")));
    }
    let mut ty = u32_at(b, p.saturating_add(1), le);
    *p = p.saturating_add(5);
    let (mut z, mut m) = (false, false);
    if ty & 0x8000_0000 != 0 {
        z = true;
        ty &= 0x0fff_ffff;
    }
    if ty & 0x4000_0000 != 0 {
        m = true;
        ty &= 0x0fff_ffff;
    }
    if ty & 0x2000_0000 != 0 {
        need(b, *p, 4)?;
        *p = p.saturating_add(4);
        ty &= 0x0fff_ffff;
    }
    if ty >= 3000 {
        z = true;
        m = true;
        ty = ty.saturating_sub(3000);
    } else if ty >= 2000 {
        m = true;
        ty = ty.saturating_sub(2000);
    } else if ty >= 1000 {
        z = true;
        ty = ty.saturating_sub(1000);
    }
    let dim: usize = 2usize.saturating_add(usize::from(z)).saturating_add(usize::from(m));
    let pt = |p: &mut usize| -> Result<Pos, GeomError> {
        need(b, *p, dim.saturating_mul(8))?;
        let x = f64_at(b, *p, le);
        let y = f64_at(b, p.saturating_add(8), le);
        let zz = if z { f64_at(b, p.saturating_add(16), le) } else { f64::NAN };
        *p = p.saturating_add(dim.saturating_mul(8));
        Ok(if z && zz.is_finite() { vec![x, y, zz] } else { vec![x, y] })
    };
    let count = |p: &mut usize, per: usize| -> Result<usize, GeomError> {
        need(b, *p, 4)?;
        let n = u32_at(b, *p, le);
        *p = p.saturating_add(4);
        let room = b.len().saturating_sub(*p) as u64;
        if u64::from(n) > MAX_N || u64::from(n).saturating_mul(per as u64) > room {
            return Err(GeomError(tr!("geom.count")));
        }
        Ok(usize::try_from(n).unwrap_or(0))
    };
    let seq = |p: &mut usize| -> Result<Vec<Pos>, GeomError> {
        let n = count(p, dim.saturating_mul(8))?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(pt(p)?);
        }
        Ok(out)
    };
    let poly = |p: &mut usize| -> Result<Vec<Ring>, GeomError> {
        let n = count(p, 4)?;
        let mut out = Vec::new();
        for _ in 0..n {
            out.push(close_ring(seq(p)?));
        }
        Ok(out.into_iter().filter(|r| r.len() >= 4).collect())
    };
    match ty {
        1 => {
            let q = pt(p)?;
            Ok(vec![if q.g(0).is_finite() && q.g(1).is_finite() { Some(Geom::Pt(q)) } else { None }])
        }
        2 => {
            let s = seq(p)?;
            Ok(vec![if s.len() >= 2 { Some(Geom::Ln(vec![s])) } else { None }])
        }
        3 => {
            let r = poly(p)?;
            Ok(vec![if r.is_empty() { None } else { Some(Geom::Pg(vec![r])) }])
        }
        4..=7 => {
            let n = count(p, 5)?;
            let mut kids: Vec<Option<Geom>> = Vec::new();
            for _ in 0..n {
                kids.extend(one(b, p, depth.saturating_add(1), multipoint)?);
            }
            match ty {
                4 => {
                    *multipoint = true;
                    Ok(if kids.is_empty() { vec![None] } else { kids })
                }
                5 => {
                    // Spec gap: the JS flattens any child's coordinates one level (a point inside a MultiLineString
                    // makes a broken part); only line children are kept here.
                    let parts: Vec<Vec<Pos>> =
                        kids.into_iter().flatten().flat_map(|k| if let Geom::Ln(c) = k { c } else { Vec::new() }).collect();
                    Ok(vec![if parts.is_empty() { None } else { Some(Geom::Ln(parts)) }])
                }
                6 => {
                    let polys: Vec<Vec<Ring>> =
                        kids.into_iter().flatten().flat_map(|k| if let Geom::Pg(c) = k { c } else { Vec::new() }).collect();
                    Ok(vec![if polys.is_empty() { None } else { Some(Geom::Pg(polys)) }])
                }
                _ => Ok(if kids.is_empty() { vec![None] } else { kids }),
            }
        }
        _ => Err(GeomError(t("geom.type", &[("type", F::from(f64::from(ty)))]))),
    }
}

/// `writeWkb(g, hasZ)`: ISO WKB little endian (Point, MultiLineString, MultiPolygon, + Z).
pub fn write_wkb(g: Option<&Geom>, has_z: bool) -> Result<Vec<u8>, GeomError> {
    let dim = if has_z { 3 } else { 2 };
    let mut out: Vec<u8> = Vec::new();
    let head = |out: &mut Vec<u8>, ty: u32, n: Option<usize>| {
        out.push(1);
        out.extend_from_slice(&(ty.saturating_add(if has_z { 1000 } else { 0 })).to_le_bytes());
        if let Some(n) = n {
            out.extend_from_slice(&(u32::try_from(n).unwrap_or(u32::MAX)).to_le_bytes());
        }
    };
    let cnt = |out: &mut Vec<u8>, n: usize| out.extend_from_slice(&(u32::try_from(n).unwrap_or(u32::MAX)).to_le_bytes());
    let coords = |out: &mut Vec<u8>, list: &[Pos]| {
        for p in list {
            out.extend_from_slice(&p.g(0).to_le_bytes());
            out.extend_from_slice(&p.g(1).to_le_bytes());
            if dim == 3 {
                let z = if p.len() > 2 { p.g(2) } else { 0.0 };
                out.extend_from_slice(&z.to_le_bytes());
            }
        }
    };
    match g {
        None => return Err(GeomError(tr!("geom.none"))),
        Some(Geom::Pt(c)) => {
            head(&mut out, 1, None);
            coords(&mut out, std::slice::from_ref(c));
        }
        Some(Geom::Ln(c)) => {
            head(&mut out, 5, Some(c.len()));
            for part in c {
                head(&mut out, 2, None);
                cnt(&mut out, part.len());
                coords(&mut out, part);
            }
        }
        Some(Geom::Pg(c)) => {
            head(&mut out, 6, Some(c.len()));
            for poly in c {
                head(&mut out, 3, None);
                cnt(&mut out, poly.len());
                for r in poly {
                    cnt(&mut out, r.len());
                    coords(&mut out, r);
                }
            }
        }
    }
    Ok(out)
}

/// Write a JS DataView-style u32 / f64 into a sized buffer.
pub fn put_u32le(b: &mut [u8], i: usize, v: u32) {
    put(b, i, &v.to_le_bytes());
}

// ---------------------------------------------------------------- layers as the readers return them

/// A layer read from a file: `{ name, geom, hasZ, fields, features, notes, … }` (+ format-specific keys in `extra`).
#[derive(Clone, Debug, Default)]
pub struct ReadLayer {
    pub name: String,
    pub geom: String,
    pub has_z: bool,
    pub fields: Vec<Field>,
    pub features: Vec<Feature>,
    pub notes: Vec<String>,
    pub extra: Vec<(String, Value)>,
}
impl J for ReadLayer {
    fn j(&self) -> Value {
        let mut o = Obj::new()
            .set("name", &self.name)
            .set("geom", &self.geom)
            .set("hasZ", self.has_z)
            .set("fields", &self.fields)
            .set("features", &self.features)
            .set("notes", &self.notes);
        for (k, v) in &self.extra {
            o.put(k, v.clone());
        }
        o.done()
    }
}

/// A geometry from JSON `{t, c}` (or null).
pub fn read_geom(v: &Value) -> Option<Geom> {
    let o = v.as_object()?;
    let c = o.get("c")?;
    let pos = |p: &Value| -> Pos { cc_civil::js::arr(Some(p)).iter().map(|x| cc_civil::js::numf(Some(x))).collect() };
    let list = |v: &Value| -> Vec<Pos> { cc_civil::js::arr(Some(v)).iter().map(pos).collect() };
    match o.get("t").and_then(Value::as_str)? {
        "pt" => Some(Geom::Pt(pos(c))),
        "ln" => Some(Geom::Ln(cc_civil::js::arr(Some(c)).iter().map(list).collect())),
        "pg" => {
            Some(Geom::Pg(cc_civil::js::arr(Some(c)).iter().map(|poly| cc_civil::js::arr(Some(poly)).iter().map(list).collect()).collect()))
        }
        _ => None,
    }
}
/// A feature from JSON `{g, p}`.
pub fn read_feature(v: &Value) -> Feature {
    Feature { g: v.get("g").and_then(read_geom), p: v.get("p").and_then(Value::as_object).cloned().unwrap_or_default() }
}
/// Fields from JSON `[{name, type}]`.
pub fn read_fields(v: Option<&Value>) -> Vec<Field> {
    cc_civil::js::arr(v)
        .iter()
        .map(|f| Field { name: cc_civil::js::string_or_empty(f.get("name")), ty: cc_civil::js::string_or_empty(f.get("type")) })
        .collect()
}
