//! shapefile.js — ESRI shapefiles (.shp + .shx + .dbf + .prj + .cpg) read and written. Every count and offset is
//! checked against the file; a bad record stops the read with a message naming it.

use crate::dbf::read_dbf;
use crate::gis_geom::{ReadLayer, close_ring, orient, polygons_from_rings};
use crate::gjs::{f64le, i32be, i32le, put, utf8_lossy};
use cc_civil::gis_state::{Feature, Field, Geom, Pos};
use cc_civil::js::{G, max, min};
use cc_civil::tr;
use serde_json::{Map, Value};

/// The JS ShpError message.
#[derive(Clone, Debug)]
pub struct ShpError(pub String);

fn kind_of(t: i32) -> Option<&'static str> {
    Some(match t {
        0 => "null",
        1 | 8 | 11 | 18 | 21 | 28 => "pt",
        3 | 13 | 23 => "ln",
        5 | 15 | 25 => "pg",
        31 => "patch",
        _ => return None,
    })
}
fn z_type(t: i32) -> bool {
    matches!(t, 11 | 13 | 15 | 18)
}
const NODATA: f64 = -1e38;

/// A file of a set.
#[derive(Clone, Debug, Default)]
pub struct ShpSet {
    pub name: String,
    pub shp: Option<Vec<u8>>,
    pub shx: Option<Vec<u8>>,
    pub dbf: Option<Vec<u8>>,
    pub prj: Option<Vec<u8>>,
    pub cpg: Option<Vec<u8>>,
}

/// `shapefileSets(files)`: group files by name (case-blind).
pub fn shapefile_sets(files: &[(String, Vec<u8>)]) -> Vec<ShpSet> {
    let mut keys: Vec<String> = Vec::new();
    let mut sets: Vec<ShpSet> = Vec::new();
    for (name, data) in files {
        let Some(dot) = name.rfind('.') else { continue };
        let ext = name.get(dot.saturating_add(1)..).unwrap_or("").to_ascii_lowercase();
        if !matches!(ext.as_str(), "shp" | "shx" | "dbf" | "prj" | "cpg") {
            continue;
        }
        let stem = name.get(..dot).unwrap_or("");
        let base = stem.rsplit('/').next().unwrap_or(stem).to_string();
        let key = stem.to_lowercase();
        let i = match keys.iter().position(|k| *k == key) {
            Some(i) => i,
            None => {
                keys.push(key);
                sets.push(ShpSet { name: base, ..ShpSet::default() });
                sets.len().saturating_sub(1)
            }
        };
        if let Some(s) = sets.get_mut(i) {
            let d = Some(data.clone());
            match ext.as_str() {
                "shp" => s.shp = d,
                "shx" => s.shx = d,
                "dbf" => s.dbf = d,
                "prj" => s.prj = d,
                _ => s.cpg = d,
            }
        }
    }
    sets
}

enum Shape {
    Null,
    One(Geom),
    Many(Vec<Geom>),
}

/// `readShapefile(set, { maxFeatures })`.
pub fn read_shapefile(set: &ShpSet, max_features: usize) -> Result<ReadLayer, ShpError> {
    let name = if set.name.is_empty() { "shapefile".to_string() } else { set.name.clone() };
    let nm = name.as_str();
    let Some(b) = set.shp.as_deref() else { return Err(ShpError(tr!("shp.noshp", name = nm))) };
    let mut notes = Vec::new();
    if b.len() < 100 {
        return Err(ShpError(tr!("shp.short", name = nm)));
    }
    if i32be(b, 0) != 9994 || i32le(b, 28) != 1000 {
        return Err(ShpError(tr!("shp.header", name = nm)));
    }
    let declared = i64::from(i32be(b, 24)).saturating_mul(2);
    let blen = i64::try_from(b.len()).unwrap_or(i64::MAX);
    if declared > blen {
        notes.push(tr!("shp.declared", name = nm));
    }
    let end = blen.min(if declared > 100 { declared } else { blen });
    let ty = i32le(b, 32);
    if ty == 31 || kind_of(ty).is_none() {
        return Err(ShpError(if ty == 31 {
            tr!("shp.patch", name = nm)
        } else {
            cc_civil::i18n::t("shp.type", &[("name", nm.into()), ("type", cc_civil::i18n::F::from(i64::from(ty)))])
        }));
    }
    let mut shapes: Vec<Shape> = Vec::new();
    let mut p: i64 = 100;
    let (mut multi, mut m_dropped) = (false, false);
    let mut rec: i64 = 0;
    let mut guessed: Option<i32> = if kind_of(ty) == Some("null") { None } else { Some(ty) };
    while p.saturating_add(8) <= end {
        let pu = usize::try_from(p).unwrap_or(usize::MAX);
        let len = i64::from(i32be(b, pu.saturating_add(4))).saturating_mul(2);
        let s = p.saturating_add(8);
        let e = s.saturating_add(len);
        rec = rec.saturating_add(1);
        if len < 4 || e > end {
            if e > end && len >= 4 {
                notes.push(tr!("shp.cut", name = nm, rec = rec));
                break;
            }
            return Err(ShpError(tr!("shp.len", name = nm, rec = rec)));
        }
        let su = usize::try_from(s).unwrap_or(usize::MAX);
        let eu = usize::try_from(e).unwrap_or(usize::MAX);
        let st = i32le(b, su);
        if st != 0 && st != ty && ty != 0 && kind_of(st) != kind_of(ty) {
            return Err(ShpError(tr!("shp.other", name = nm, rec = rec)));
        }
        if st != 0 && guessed.is_none() {
            guessed = Some(st);
        }
        if matches!(st, 21 | 23 | 25 | 28) {
            m_dropped = true;
        }
        shapes.push(read_shape(b, su, eu, st, nm, rec)?);
        if matches!(st, 8 | 18 | 28) {
            multi = true;
        }
        p = e;
        if shapes.len() > max_features {
            break;
        }
    }
    let st0 = guessed.unwrap_or(ty);
    let geom = match kind_of(st0) {
        Some("null") => "pt",
        Some(k) => k,
        None => "",
    };
    let mut fields: Vec<Field> = Vec::new();
    let mut records: Vec<Map<String, Value>> = Vec::new();
    if let Some(dbf) = set.dbf.as_deref() {
        let cpg = set.cpg.as_deref().map(utf8_lossy);
        let d = read_dbf(dbf, cpg.as_deref(), max_features.saturating_add(1)).map_err(|e| ShpError(e.0))?;
        fields = d.fields.iter().map(|f| Field { name: f.name.clone(), ty: f.ty.to_string() }).collect();
        records = d.records;
        for x in d.notes {
            notes.push(tr!("shp.note", name = nm, note = x));
        }
        if records.len() != shapes.len() {
            notes.push(tr!("shp.dbf.count", name = nm, records = records.len(), shapes = shapes.len()));
        }
    } else {
        notes.push(tr!("shp.nodbf", name = nm));
    }
    let mut features: Vec<Feature> = Vec::new();
    let n_shapes = shapes.len();
    for (i, g) in shapes.into_iter().enumerate() {
        if features.len() >= max_features {
            break;
        }
        let p0 = records.get(i).cloned().unwrap_or_default();
        match g {
            Shape::Many(list) => {
                for q in list {
                    features.push(Feature { g: Some(q), p: p0.clone() });
                }
            }
            Shape::One(q) => features.push(Feature { g: Some(q), p: p0 }),
            Shape::Null => features.push(Feature { g: None, p: p0 }),
        }
    }
    if features.len() >= max_features && n_shapes > features.len() {
        notes.push(tr!("shp.max", name = nm, n = max_features));
    }
    if multi {
        notes.push(tr!("shp.multi", name = nm));
    }
    if m_dropped {
        notes.push(tr!("shp.m", name = nm));
    }
    let prj = set.prj.as_deref().map(|p| cc_civil::js::trim(&utf8_lossy(p)).to_string()).filter(|s| !s.is_empty());
    Ok(ReadLayer {
        name,
        geom: geom.to_string(),
        has_z: z_type(st0),
        fields,
        features,
        notes,
        extra: vec![("prj".into(), prj.map_or(Value::Null, Value::String)), ("shapeType".into(), Value::from(st0))],
    })
}

fn read_shape(b: &[u8], s: usize, e: usize, st: i32, name: &str, rec: i64) -> Result<Shape, ShpError> {
    if st == 0 {
        return Ok(Shape::Null);
    }
    let bad = || ShpError(tr!("shp.bad", name = name, rec = rec));
    let z = z_type(st);
    let f = |o: usize| f64le(b, o);
    let z_ok = |v: f64| v.is_finite() && v > NODATA;
    let at = |a: usize, k: usize| s.saturating_add(a).saturating_add(k);
    if matches!(st, 1 | 11 | 21) {
        if s.saturating_add(20) > e {
            return Err(bad());
        }
        let x = f(at(4, 0));
        let y = f(at(12, 0));
        if !x.is_finite() || !y.is_finite() {
            return Ok(Shape::Null);
        }
        if z && s.saturating_add(28) <= e && z_ok(f(at(20, 0))) {
            return Ok(Shape::One(Geom::Pt(vec![x, y, f(at(20, 0))])));
        }
        return Ok(Shape::One(Geom::Pt(vec![x, y])));
    }
    if matches!(st, 8 | 18 | 28) {
        if s.saturating_add(40) > e {
            return Err(bad());
        }
        let n = i32le(b, at(36, 0));
        if n < 0 {
            return Err(bad());
        }
        let n = usize::try_from(n).unwrap_or(0);
        if s.saturating_add(40).saturating_add(n.saturating_mul(16)) > e {
            return Err(bad());
        }
        let zs = s.saturating_add(40).saturating_add(n.saturating_mul(16)).saturating_add(16);
        let hz = z && zs.saturating_add(n.saturating_mul(8)) <= e;
        let mut out = Vec::new();
        for i in 0..n {
            let x = f(at(40, i.saturating_mul(16)));
            let y = f(at(48, i.saturating_mul(16)));
            if !x.is_finite() || !y.is_finite() {
                continue;
            }
            let zz = if hz { f(zs.saturating_add(i.saturating_mul(8))) } else { f64::NAN };
            out.push(Geom::Pt(if hz && z_ok(zz) { vec![x, y, zz] } else { vec![x, y] }));
        }
        return Ok(if out.is_empty() { Shape::Null } else { Shape::Many(out) });
    }
    // PolyLine / Polygon (+Z/M)
    if s.saturating_add(44) > e {
        return Err(bad());
    }
    let np = i32le(b, at(36, 0));
    let n = i32le(b, at(40, 0));
    if np < 0 || n < 0 || i64::from(np) > i64::from(n).saturating_add(1) {
        return Err(bad());
    }
    let npu = usize::try_from(np).unwrap_or(0);
    let nu = usize::try_from(n).unwrap_or(0);
    if s.saturating_add(44).saturating_add(npu.saturating_mul(4)).saturating_add(nu.saturating_mul(16)) > e {
        return Err(bad());
    }
    let ps = s.saturating_add(44);
    let pts = ps.saturating_add(npu.saturating_mul(4));
    let zs = pts.saturating_add(nu.saturating_mul(16)).saturating_add(16);
    let hz = z && zs.saturating_add(nu.saturating_mul(8)) <= e;
    let mut starts: Vec<usize> = Vec::with_capacity(npu);
    for i in 0..npu {
        let k = i32le(b, ps.saturating_add(i.saturating_mul(4)));
        if k < 0 || k > n || (i > 0 && usize::try_from(k).unwrap_or(0) < starts.g(i.saturating_sub(1))) {
            return Err(bad());
        }
        starts.push(usize::try_from(k).unwrap_or(0));
    }
    let mut parts: Vec<Vec<Pos>> = Vec::new();
    for i in 0..npu {
        let a = starts.g(i);
        let b_end = if i.saturating_add(1) < npu { starts.g(i.saturating_add(1)) } else { nu };
        let mut part = Vec::new();
        for k in a..b_end {
            let x = f(pts.saturating_add(k.saturating_mul(16)));
            let y = f(pts.saturating_add(k.saturating_mul(16)).saturating_add(8));
            if !x.is_finite() || !y.is_finite() {
                continue;
            }
            let zz = if hz { f(zs.saturating_add(k.saturating_mul(8))) } else { f64::NAN };
            part.push(if hz && z_ok(zz) { vec![x, y, zz] } else { vec![x, y] });
        }
        parts.push(part);
    }
    if matches!(st, 3 | 13 | 23) {
        let c: Vec<Vec<Pos>> = parts.into_iter().filter(|q| q.len() >= 2).collect();
        return Ok(if c.is_empty() { Shape::Null } else { Shape::One(Geom::Ln(c)) });
    }
    let rings: Vec<Vec<Pos>> = parts.into_iter().map(close_ring).filter(|r| r.len() >= 4).collect();
    let polys = polygons_from_rings(rings);
    Ok(if polys.is_empty() { Shape::Null } else { Shape::One(Geom::Pg(polys)) })
}

// ---------------------------------------------------------------- writing

/// writeShapefile's result.
pub struct ShpOut {
    pub shp: Vec<u8>,
    pub shx: Vec<u8>,
    pub dbf: Vec<u8>,
    pub names: Vec<String>,
}

/// `writeShapefile({ geom, hasZ, fields, features })`.
pub fn write_shapefile(geom: &str, has_z: bool, fields: &[Field], features: &[Feature], date: (i32, u32, u32)) -> ShpOut {
    let base: i32 = match geom {
        "pt" => 1,
        "ln" => 3,
        _ => 5,
    };
    let ty = base.saturating_add(if has_z { 10 } else { 0 });
    let mut recs: Vec<Vec<u8>> = Vec::new();
    let (mut x0, mut y0, mut x1, mut y1, mut z0, mut z1) =
        (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    let mut grow = |p: &Pos| {
        let (x, y) = (p.g(0), p.g(1));
        if x < x0 {
            x0 = x;
        }
        if x > x1 {
            x1 = x;
        }
        if y < y0 {
            y0 = y;
        }
        if y > y1 {
            y1 = y;
        }
        let z = if p.len() > 2 { p.g(2) } else { 0.0 };
        if z < z0 {
            z0 = z;
        }
        if z > z1 {
            z1 = z;
        }
    };
    for ft in features {
        let g = match &ft.g {
            Some(g) if g.kind() == geom => g.clone(),
            _ => {
                recs.push(vec![0u8; 4]);
                continue;
            }
        };
        let g = if matches!(g, Geom::Pg(_)) { orient(&g, false) } else { g };
        if let Geom::Pt(c) = &g {
            grow(c);
            let mut r = vec![0u8; if has_z { 36 } else { 20 }];
            put(&mut r, 0, &ty.to_le_bytes());
            put(&mut r, 4, &c.g(0).to_le_bytes());
            put(&mut r, 12, &c.g(1).to_le_bytes());
            if has_z {
                put(&mut r, 20, &(if c.len() > 2 { c.g(2) } else { 0.0 }).to_le_bytes());
                put(&mut r, 28, &(-1e39f64).to_le_bytes());
            }
            recs.push(r);
            continue;
        }
        let parts: Vec<&Vec<Pos>> = match &g {
            Geom::Ln(c) => c.iter().collect(),
            Geom::Pg(c) => c.iter().flatten().collect(),
            Geom::Pt(_) => Vec::new(),
        };
        let n: usize = parts.iter().map(|q| q.len()).sum();
        let size = 44usize.saturating_add(parts.len().saturating_mul(4)).saturating_add(n.saturating_mul(16)).saturating_add(if has_z {
            16usize.saturating_add(n.saturating_mul(8))
        } else {
            0
        });
        let mut r = vec![0u8; size];
        let (mut bx0, mut by0, mut bx1, mut by1, mut zz0, mut zz1) =
            (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
        put(&mut r, 0, &ty.to_le_bytes());
        put(&mut r, 36, &(i32::try_from(parts.len()).unwrap_or(i32::MAX)).to_le_bytes());
        put(&mut r, 40, &(i32::try_from(n).unwrap_or(i32::MAX)).to_le_bytes());
        let mut k = 0usize;
        let pts = 44usize.saturating_add(parts.len().saturating_mul(4));
        let zs = pts.saturating_add(n.saturating_mul(16)).saturating_add(16);
        for (i, q) in parts.iter().enumerate() {
            put(&mut r, 44usize.saturating_add(i.saturating_mul(4)), &(i32::try_from(k).unwrap_or(i32::MAX)).to_le_bytes());
            for pt in q.iter() {
                grow(pt);
                put(&mut r, pts.saturating_add(k.saturating_mul(16)), &pt.g(0).to_le_bytes());
                put(&mut r, pts.saturating_add(k.saturating_mul(16)).saturating_add(8), &pt.g(1).to_le_bytes());
                bx0 = min(bx0, pt.g(0));
                bx1 = max(bx1, pt.g(0));
                by0 = min(by0, pt.g(1));
                by1 = max(by1, pt.g(1));
                if has_z {
                    let z = if pt.len() > 2 { pt.g(2) } else { 0.0 };
                    put(&mut r, zs.saturating_add(k.saturating_mul(8)), &z.to_le_bytes());
                    zz0 = min(zz0, z);
                    zz1 = max(zz1, z);
                }
                k = k.saturating_add(1);
            }
        }
        put(&mut r, 4, &bx0.to_le_bytes());
        put(&mut r, 12, &by0.to_le_bytes());
        put(&mut r, 20, &bx1.to_le_bytes());
        put(&mut r, 28, &by1.to_le_bytes());
        if has_z {
            put(&mut r, pts.saturating_add(n.saturating_mul(16)), &zz0.to_le_bytes());
            put(&mut r, pts.saturating_add(n.saturating_mul(16)).saturating_add(8), &zz1.to_le_bytes());
        }
        recs.push(r);
    }
    if !x0.is_finite() {
        (x0, y0, x1, y1, z0, z1) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    }
    let header = |len: usize| -> Vec<u8> {
        let mut h = vec![0u8; 100];
        put(&mut h, 0, &9994i32.to_be_bytes());
        put(&mut h, 24, &(i32::try_from(len / 2).unwrap_or(i32::MAX)).to_be_bytes());
        put(&mut h, 28, &1000i32.to_le_bytes());
        put(&mut h, 32, &ty.to_le_bytes());
        put(&mut h, 36, &x0.to_le_bytes());
        put(&mut h, 44, &y0.to_le_bytes());
        put(&mut h, 52, &x1.to_le_bytes());
        put(&mut h, 60, &y1.to_le_bytes());
        put(&mut h, 68, &(if has_z { z0 } else { 0.0 }).to_le_bytes());
        put(&mut h, 76, &(if has_z { z1 } else { 0.0 }).to_le_bytes());
        h
    };
    let shp_len = recs.iter().fold(100usize, |s, r| s.saturating_add(8).saturating_add(r.len()));
    let mut shp = vec![0u8; shp_len];
    let shx_len = 100usize.saturating_add(recs.len().saturating_mul(8));
    let mut shx = vec![0u8; shx_len];
    put(&mut shp, 0, &header(shp_len));
    put(&mut shx, 0, &header(shx_len));
    let mut o = 100usize;
    for (i, r) in recs.iter().enumerate() {
        put(&mut shp, o, &(i32::try_from(i.saturating_add(1)).unwrap_or(i32::MAX)).to_be_bytes());
        put(&mut shp, o.saturating_add(4), &(i32::try_from(r.len() / 2).unwrap_or(i32::MAX)).to_be_bytes());
        put(&mut shp, o.saturating_add(8), r);
        let xi = 100usize.saturating_add(i.saturating_mul(8));
        put(&mut shx, xi, &(i32::try_from(o / 2).unwrap_or(i32::MAX)).to_be_bytes());
        put(&mut shx, xi.saturating_add(4), &(i32::try_from(r.len() / 2).unwrap_or(i32::MAX)).to_be_bytes());
        o = o.saturating_add(8).saturating_add(r.len());
    }
    let recs_p: Vec<&Map<String, Value>> = features.iter().map(|f| &f.p).collect();
    let (dbf, names) = crate::dbf::write_dbf(fields, &recs_p, date);
    ShpOut { shp, shx, dbf, names }
}
