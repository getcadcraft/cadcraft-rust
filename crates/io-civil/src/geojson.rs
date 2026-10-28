//! geojson.js — GeoJSON (RFC 7946) read and written: one layer per kind, the old `crs` member honoured, fields typed
//! from the values; writer = one FeatureCollection in longitude/latitude (1e-9°), right-hand-rule polygons.

use crate::gis_geom::{ReadLayer, has_z, map_xy, orient};
use crate::gjs::{js_keys, num, num_keep, soft_controls, stringify};
use cc_civil::gis_state::{Feature, Field, Geom, Pos};
use cc_civil::js::{G, round, slice16, to_js_string};
use cc_civil::tr;
use serde_json::{Map, Value};

/// The JS GeoJsonError message.
#[derive(Clone, Debug)]
pub struct GeoJsonError(pub String);

/// readGeoJSON's result.
#[derive(Clone, Debug, Default)]
pub struct GeoJsonRead {
    pub layers: Vec<ReadLayer>,
    pub crs_code: Option<f64>,
    pub notes: Vec<String>,
}

fn fin(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|x| x.is_finite())
}
fn is_pos(p: &Value) -> bool {
    let Some(a) = p.as_array() else { return false };
    (2..=4).contains(&a.len())
        && fin(a.first()).is_some()
        && fin(a.get(1)).is_some()
        && (a.len() < 3 || fin(a.get(2)).is_some() || a.get(2).is_some_and(Value::is_null))
}
fn pos(p: &Value) -> Pos {
    let a = p.as_array().map(Vec::as_slice).unwrap_or(&[]);
    let x = fin(a.first()).unwrap_or(f64::NAN);
    let y = fin(a.get(1)).unwrap_or(f64::NAN);
    match fin(a.get(2)) {
        Some(z) if a.len() >= 3 => vec![x, y, z],
        _ => vec![x, y],
    }
}

/// `/EPSG:{1,2}(?:[\d.]*:)?(\d{3,6})$/i` → the code digits.
fn epsg_code(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    let b = lower.as_bytes();
    let digits_to_end = |k: usize| -> Option<String> {
        let rest = lower.get(k..)?;
        if (3..=6).contains(&rest.len()) && rest.bytes().all(|c| c.is_ascii_digit()) { Some(rest.to_string()) } else { None }
    };
    let mut i = 0usize;
    while let Some(off) = lower.get(i..).and_then(|r| r.find("epsg")) {
        let j = i.saturating_add(off).saturating_add(4);
        for colons in 1..=2usize {
            if (0..colons).all(|c| b.get(j.saturating_add(c)) == Some(&b':')) {
                let k = j.saturating_add(colons);
                // the optional group first (greedy), then without it
                let mut m = k;
                let mut cands = Vec::new();
                while let Some(&c) = b.get(m) {
                    if c == b':' {
                        cands.push(m);
                    }
                    if !(c.is_ascii_digit() || c == b'.') {
                        break;
                    }
                    m = m.saturating_add(1);
                }
                for &cm in cands.iter().rev() {
                    if let Some(d) = digits_to_end(cm.saturating_add(1)) {
                        return Some(d);
                    }
                }
                if let Some(d) = digits_to_end(k) {
                    return Some(d);
                }
            }
        }
        i = i.saturating_add(off).saturating_add(1);
    }
    None
}

/// `crsFromMember(c, notes)`: Some(None) = WGS 84, Some(Some(code)), None = undefined.
fn crs_from_member(c: Option<&Value>, notes: &mut Vec<String>) -> Option<Option<f64>> {
    let c = c.filter(|c| c.is_object() || c.is_array())?;
    let props = c.get("properties");
    let pname = props.and_then(|p| p.get("name")).filter(|v| !v.is_null());
    let name = match pname {
        Some(v) => to_js_string(v),
        None => {
            if c.get("type").and_then(Value::as_str) == Some("EPSG") {
                format!("EPSG:{}", props.and_then(|p| p.get("code")).map_or_else(|| "undefined".to_string(), to_js_string))
            } else {
                String::new()
            }
        }
    };
    let up = name.to_ascii_uppercase();
    if up.ends_with("CRS84") || up.ends_with("CRS:84") {
        return Some(None);
    }
    if let Some(d) = epsg_code(&name) {
        let code = cc_civil::js::str_to_number(&d);
        return Some(if code == 4326.0 { None } else { Some(code) });
    }
    notes.push(tr!("geojson.crs.member", crs = slice16(&name, 60)));
    None
}

#[derive(Default)]
struct Counts {
    bad: usize,
    empty: usize,
    flattened: bool,
    split: bool,
}

fn rings(c: Option<&Value>) -> Option<Vec<Vec<Pos>>> {
    let a = c.and_then(Value::as_array)?;
    if a.is_empty() {
        return None;
    }
    let mut out: Vec<Vec<Pos>> = Vec::new();
    for r in a {
        let ok = r.as_array().is_some_and(|ra| ra.len() >= 4 && ra.iter().all(is_pos));
        if !ok {
            if out.is_empty() {
                return None;
            }
            continue;
        }
        let mut q: Vec<Pos> = r.as_array().map(|ra| ra.iter().map(pos).collect()).unwrap_or_default();
        let f = q.first().cloned().unwrap_or_default();
        let l = q.last().cloned().unwrap_or_default();
        if f.g(0) != l.g(0) || f.g(1) != l.g(1) {
            q.push(f);
        }
        out.push(q);
    }
    if out.is_empty() { None } else { Some(out) }
}

fn geoms(g: &Value, depth: u32, k: &mut Counts) -> Vec<Option<Geom>> {
    if !(g.is_object() || g.is_array()) {
        return vec![None];
    }
    let c = g.get("coordinates");
    let arr = c.and_then(Value::as_array);
    match g.get("type").and_then(Value::as_str).unwrap_or("") {
        "Point" => {
            if c.is_some_and(is_pos) {
                vec![c.map(|c| Geom::Pt(pos(c)))]
            } else {
                k.bad = k.bad.saturating_add(1);
                vec![None]
            }
        }
        "MultiPoint" => match arr {
            None => {
                k.bad = k.bad.saturating_add(1);
                vec![None]
            }
            Some(a) => {
                k.split = true;
                a.iter().filter(|p| is_pos(p)).map(|p| Some(Geom::Pt(pos(p)))).collect()
            }
        },
        "LineString" => match arr {
            Some(a) if a.len() >= 2 && a.iter().all(is_pos) => vec![Some(Geom::Ln(vec![a.iter().map(pos).collect()]))],
            _ => {
                k.bad = k.bad.saturating_add(1);
                vec![None]
            }
        },
        "MultiLineString" => match arr {
            None => {
                k.bad = k.bad.saturating_add(1);
                vec![None]
            }
            Some(a) => {
                let parts: Vec<Vec<Pos>> = a
                    .iter()
                    .filter_map(Value::as_array)
                    .filter(|l| l.len() >= 2 && l.iter().all(is_pos))
                    .map(|l| l.iter().map(pos).collect())
                    .collect();
                if parts.is_empty() {
                    if a.is_empty() {
                        k.empty = k.empty.saturating_add(1);
                    } else {
                        k.bad = k.bad.saturating_add(1);
                    }
                    vec![None]
                } else {
                    vec![Some(Geom::Ln(parts))]
                }
            }
        },
        "Polygon" => match rings(c) {
            Some(r) => vec![Some(Geom::Pg(vec![r]))],
            None => {
                k.bad = k.bad.saturating_add(1);
                vec![None]
            }
        },
        "MultiPolygon" => match arr {
            None => {
                k.bad = k.bad.saturating_add(1);
                vec![None]
            }
            Some(a) => {
                let polys: Vec<Vec<Vec<Pos>>> = a.iter().filter_map(|p| rings(Some(p))).collect();
                if polys.is_empty() {
                    if a.is_empty() {
                        k.empty = k.empty.saturating_add(1);
                    } else {
                        k.bad = k.bad.saturating_add(1);
                    }
                    vec![None]
                } else {
                    vec![Some(Geom::Pg(polys))]
                }
            }
        },
        "GeometryCollection" => {
            let list = g.get("geometries").and_then(Value::as_array);
            match list {
                Some(l) if depth <= 4 => {
                    k.flattened = true;
                    l.iter().flat_map(|x| geoms(x, depth.saturating_add(1), k)).collect()
                }
                _ => {
                    k.bad = k.bad.saturating_add(1);
                    vec![None]
                }
            }
        }
        _ => {
            k.bad = k.bad.saturating_add(1);
            vec![None]
        }
    }
}

/// A property value as readGeoJSON keeps it.
pub fn prop_value(v: &Value) -> Value {
    match v {
        Value::Null | Value::Bool(_) => v.clone(),
        Value::Number(n) => {
            let x = n.as_f64().unwrap_or(f64::NAN);
            if x.is_finite() { num_keep(x) } else { Value::Null }
        }
        Value::String(s) => Value::String(crate::gjs::cut16(&soft_controls(s), 10000)),
        _ => Value::String(slice16(&stringify(v), 10000)),
    }
}

/// `readGeoJSON(text, name, { maxFeatures })`, walking the text once ([`crate::gjparse`]): the same result as
/// [`read_geojson_tree`], which reads it as a JSON tree first.
pub fn read_geojson(text: &str, name: &str, max_features: usize) -> Result<GeoJsonRead, GeoJsonError> {
    use crate::gjparse::{FeatJ, Feats, G, GeomJ, Top};
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let top = crate::gjparse::parse(text).map_err(|_| GeoJsonError(tr!("geojson.json", name = name)))?;
    let t = match top {
        Top::Obj(t) => *t,
        Top::Arr => return Err(GeoJsonError(tr!("geojson.none", name = name))),
        Top::Scalar => return Err(GeoJsonError(tr!("geojson.not", name = name))),
    };
    let mut notes = Vec::new();
    let crs_code = crs_from_member(t.crs.as_ref(), &mut notes).unwrap_or(None);
    let feats: Vec<FeatJ> = match t.geom.ty.as_deref() {
        Some("FeatureCollection") => match t.features {
            Some(Feats::List(l)) => l,
            _ => return Err(GeoJsonError(tr!("geojson.nolist", name = name))),
        },
        Some("Feature") => vec![FeatJ::Obj { properties: t.properties, geometry: t.geometry }],
        Some(_)
            if t.geom.coords.as_ref().is_some_and(crate::gjparse::C::truthy)
                || t.geom.geometries.as_ref().is_some_and(crate::gjparse::Geoms::truthy) =>
        {
            vec![FeatJ::Obj {
                properties: None,
                geometry: Some(G::Obj(Box::new(GeomJ { ty: t.geom.ty, coords: t.geom.coords, geometries: t.geom.geometries }))),
            }]
        }
        _ => return Err(GeoJsonError(tr!("geojson.none", name = name))),
    };
    let mut by: [Vec<Feature>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    let mut k = Counts::default();
    let mut n = 0usize;
    for f in feats {
        if n >= max_features {
            notes.push(tr!("geojson.max", name = name, n = max_features));
            break;
        }
        let (properties, geometry) = match f {
            FeatJ::Scalar => {
                k.bad = k.bad.saturating_add(1);
                continue;
            }
            FeatJ::Arr => (None, None),
            FeatJ::Obj { properties, geometry } => (properties, geometry),
        };
        let props = match properties {
            Some(Value::Object(pm)) => props_of(pm),
            _ => Map::new(),
        };
        let gs = match geometry {
            None | Some(G::Null) => {
                k.empty = k.empty.saturating_add(1);
                Vec::new()
            }
            Some(g) => geoms_c(g, 0, &mut k),
        };
        let mut gs: Vec<Geom> = gs.into_iter().flatten().collect();
        let last = gs.pop();
        for g in gs {
            push_feature(&mut by, Feature { g: Some(g), p: props.clone() });
            n = n.saturating_add(1);
        }
        if let Some(g) = last {
            push_feature(&mut by, Feature { g: Some(g), p: props });
            n = n.saturating_add(1);
        }
    }
    Ok(finish(by, &k, notes, crs_code, name))
}

fn push_feature(by: &mut [Vec<Feature>; 3], f: Feature) {
    let slot = match f.g.as_ref().map(Geom::kind) {
        Some("pt") => 0,
        Some("ln") => 1,
        _ => 2,
    };
    if let Some(list) = by.get_mut(slot) {
        list.push(f);
    }
}

/// The notes and the layers (one per kind present) from the features by kind.
fn finish(mut by: [Vec<Feature>; 3], k: &Counts, mut notes: Vec<String>, crs_code: Option<f64>, name: &str) -> GeoJsonRead {
    if k.bad > 0 {
        notes.push(tr!("geojson.bad", name = name, n = k.bad));
    }
    if k.empty > 0 {
        notes.push(tr!("geojson.empty", name = name, n = k.empty));
    }
    if k.flattened {
        notes.push(tr!("geojson.flat", name = name));
    }
    if k.split {
        notes.push(tr!("geojson.multi", name = name));
    }
    let kinds: Vec<usize> = (0..3).filter(|&i| by.get(i).is_some_and(|l| !l.is_empty())).collect();
    let many = kinds.len() > 1;
    let mut layers = Vec::new();
    for i in kinds {
        let features = std::mem::take(by.get_mut(i).unwrap_or(&mut Vec::new()));
        let (kind, key) = match i {
            0 => ("pt", "geojson.layer.pt"),
            1 => ("ln", "geojson.layer.ln"),
            _ => ("pg", "geojson.layer.pg"),
        };
        let has = features.iter().any(|f| f.g.as_ref().is_some_and(has_z));
        layers.push(ReadLayer {
            name: if many { tr!(key, name = name) } else { name.to_string() },
            geom: kind.into(),
            has_z: has,
            fields: fields_of(&features),
            features,
            notes: Vec::new(),
            extra: Vec::new(),
        });
    }
    GeoJsonRead { layers, crs_code, notes }
}

/// A feature's properties as readGeoJSON keeps them (`Object.keys` order, the first 500, keys without controls and
/// cut to 64, values by [`prop_value`]), taking the parsed map's keys and texts where they need no change.
fn props_of(mut pm: Map<String, Value>) -> Map<String, Value> {
    let clean_key = |k: &str| !k.is_empty() && k.len() <= 64 && !k.bytes().any(|b| b < 0x20 || b == 0x7f);
    let soft = |s: &str| s.len() <= 10000 && !s.bytes().any(|b| (b < 0x20 && b != b'\t' && b != b'\n' && b != b'\r') || b == 0x7f);
    // the usual properties: every key kept as it is, in insertion order: the map itself, values made as kept
    if pm.len() <= 500 && pm.keys().all(|k| clean_key(k) && !crate::gjs::is_index(k)) {
        for v in pm.values_mut() {
            match v {
                Value::Null | Value::Bool(_) => {}
                Value::String(s) if soft(s) => {}
                other => *other = prop_value(other),
            }
        }
        return pm;
    }
    let mut props = Map::new();
    if pm.keys().any(|k| crate::gjs::is_index(k)) {
        for key in js_keys(&pm).into_iter().take(500) {
            let clean = crate::gjs::cut16(&crate::gjs::drop_controls(key), 64);
            if clean.is_empty() {
                continue;
            }
            let v = pm.get(key).map_or(Value::Null, prop_value);
            props.insert(clean, v);
        }
        return props;
    }
    for (key, v) in pm.into_iter().take(500) {
        let clean = if key.len() <= 64 && !key.bytes().any(|b| b < 0x20 || b == 0x7f) {
            key
        } else {
            crate::gjs::cut16(&crate::gjs::drop_controls(&key), 64)
        };
        if clean.is_empty() {
            continue;
        }
        let v = match v {
            Value::String(s)
                if s.len() <= 10000 && !s.bytes().any(|b| (b < 0x20 && b != b'\t' && b != b'\n' && b != b'\r') || b == 0x7f) =>
            {
                Value::String(s)
            }
            other => prop_value(&other),
        };
        props.insert(clean, v);
    }
    props
}

fn cfin(c: Option<&crate::gjparse::C>) -> Option<f64> {
    match c {
        Some(crate::gjparse::C::Num(x)) if x.is_finite() => Some(*x),
        _ => None,
    }
}
/// `isPos(p)` of a parsed coordinates value.
fn c_is_pos(c: &crate::gjparse::C) -> bool {
    use crate::gjparse::C;
    match c {
        // numbers only, every one finite (the parser refuses ±∞)
        C::Nums(v) => (2..=4).contains(&v.len()),
        C::Arr(a) => {
            (2..=4).contains(&a.len())
                && cfin(a.first()).is_some()
                && cfin(a.get(1)).is_some()
                && (a.len() < 3 || cfin(a.get(2)).is_some() || matches!(a.get(2), Some(C::Null)))
        }
        _ => false,
    }
}
/// `pos(p)` of a value `c_is_pos` accepted.
fn c_pos(c: crate::gjparse::C) -> Pos {
    use crate::gjparse::C;
    match c {
        C::Nums(mut v) => {
            v.truncate(3);
            v
        }
        C::Arr(a) => {
            let x = cfin(a.first()).unwrap_or(f64::NAN);
            let y = cfin(a.get(1)).unwrap_or(f64::NAN);
            match cfin(a.get(2)) {
                Some(z) if a.len() >= 3 => vec![x, y, z],
                _ => vec![x, y],
            }
        }
        _ => vec![f64::NAN, f64::NAN],
    }
}
/// A list of positions when every item is one (and there are at least `min`).
fn c_line(c: crate::gjparse::C, min: usize) -> Option<Vec<Pos>> {
    match c {
        crate::gjparse::C::Arr(a) if a.len() >= min && a.iter().all(c_is_pos) => Some(a.into_iter().map(c_pos).collect()),
        _ => None,
    }
}
/// `rings(c)` of a parsed value.
fn c_rings(c: Option<crate::gjparse::C>) -> Option<Vec<Vec<Pos>>> {
    use crate::gjparse::C;
    let a = match c? {
        C::Arr(a) => a,
        // numbers: the first ring is no list of positions
        C::Nums(_) => return None,
        _ => return None,
    };
    if a.is_empty() {
        return None;
    }
    let mut out: Vec<Vec<Pos>> = Vec::new();
    for r in a {
        let Some(mut q) = c_line(r, 4) else {
            if out.is_empty() {
                return None;
            }
            continue;
        };
        let f = q.first().cloned().unwrap_or_default();
        let l = q.last().cloned().unwrap_or_default();
        if f.g(0) != l.g(0) || f.g(1) != l.g(1) {
            q.push(f);
        }
        out.push(q);
    }
    if out.is_empty() { None } else { Some(out) }
}

/// `geoms(g)` of a parsed geometry value.
fn geoms_c(g: crate::gjparse::G, depth: u32, k: &mut Counts) -> Vec<Option<Geom>> {
    use crate::gjparse::{C, G, Geoms};
    let bad = |k: &mut Counts| {
        k.bad = k.bad.saturating_add(1);
        vec![None]
    };
    let g = match g {
        G::Null | G::Scalar => return vec![None],
        G::Arr => return bad(k),
        G::Obj(g) => *g,
    };
    // `as_array`: Some(items) for a list; a list of numbers has items that are no position and no list
    enum Arr {
        No,
        Numbers,
        Items(Vec<C>),
    }
    let arr = |c: Option<C>| match c {
        Some(C::Arr(a)) => Arr::Items(a),
        Some(C::Nums(_)) => Arr::Numbers,
        _ => Arr::No,
    };
    match g.ty.as_deref().unwrap_or("") {
        "Point" => match g.coords {
            Some(c) if c_is_pos(&c) => vec![Some(Geom::Pt(c_pos(c)))],
            _ => bad(k),
        },
        "MultiPoint" => match arr(g.coords) {
            Arr::No => bad(k),
            Arr::Numbers => {
                k.split = true;
                Vec::new()
            }
            Arr::Items(a) => {
                k.split = true;
                a.into_iter().filter(c_is_pos).map(|p| Some(Geom::Pt(c_pos(p)))).collect()
            }
        },
        "LineString" => match g.coords.and_then(|c| c_line(c, 2)) {
            Some(l) => vec![Some(Geom::Ln(vec![l]))],
            None => bad(k),
        },
        "MultiLineString" => match arr(g.coords) {
            Arr::No | Arr::Numbers => bad(k),
            Arr::Items(a) => {
                let was_empty = a.is_empty();
                let parts: Vec<Vec<Pos>> = a.into_iter().filter_map(|l| c_line(l, 2)).collect();
                if parts.is_empty() {
                    if was_empty {
                        k.empty = k.empty.saturating_add(1);
                        vec![None]
                    } else {
                        bad(k)
                    }
                } else {
                    vec![Some(Geom::Ln(parts))]
                }
            }
        },
        "Polygon" => match c_rings(g.coords) {
            Some(r) => vec![Some(Geom::Pg(vec![r]))],
            None => bad(k),
        },
        "MultiPolygon" => match arr(g.coords) {
            Arr::No | Arr::Numbers => bad(k),
            Arr::Items(a) => {
                let was_empty = a.is_empty();
                let polys: Vec<Vec<Vec<Pos>>> = a.into_iter().filter_map(|p| c_rings(Some(p))).collect();
                if polys.is_empty() {
                    if was_empty {
                        k.empty = k.empty.saturating_add(1);
                        vec![None]
                    } else {
                        bad(k)
                    }
                } else {
                    vec![Some(Geom::Pg(polys))]
                }
            }
        },
        "GeometryCollection" => match g.geometries {
            Some(Geoms::List(l)) if depth <= 4 => {
                k.flattened = true;
                l.into_iter().flat_map(|x| geoms_c(x, depth.saturating_add(1), k)).collect()
            }
            _ => bad(k),
        },
        _ => bad(k),
    }
}

/// readGeoJSON over a JSON tree (serde_json): the first Rust form, kept as the reference [`read_geojson`] is tested
/// and fuzzed against.
pub fn read_geojson_tree(text: &str, name: &str, max_features: usize) -> Result<GeoJsonRead, GeoJsonError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let obj: Value = serde_json::from_str(text).map_err(|_| GeoJsonError(tr!("geojson.json", name = name)))?;
    if !(obj.is_object() || obj.is_array()) {
        return Err(GeoJsonError(tr!("geojson.not", name = name)));
    }
    let mut notes = Vec::new();
    let crs_code = crs_from_member(obj.get("crs"), &mut notes).unwrap_or(None);
    let single;
    let feats: &[Value] = match obj.get("type").and_then(Value::as_str) {
        Some("FeatureCollection") => match obj.get("features").and_then(Value::as_array) {
            Some(a) => a,
            None => return Err(GeoJsonError(tr!("geojson.nolist", name = name))),
        },
        Some("Feature") => std::slice::from_ref(&obj),
        Some(_) if cc_civil::js::truthy(obj.get("coordinates")) || cc_civil::js::truthy(obj.get("geometries")) => {
            let mut m = Map::new();
            m.insert("type".into(), Value::String("Feature".into()));
            m.insert("geometry".into(), obj.clone());
            m.insert("properties".into(), Value::Null);
            single = Value::Object(m);
            std::slice::from_ref(&single)
        }
        _ => return Err(GeoJsonError(tr!("geojson.none", name = name))),
    };
    let mut by: [Vec<Feature>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    let mut k = Counts::default();
    let mut n = 0usize;
    for f in feats {
        if n >= max_features {
            notes.push(tr!("geojson.max", name = name, n = max_features));
            break;
        }
        if !(f.is_object() || f.is_array()) {
            k.bad = k.bad.saturating_add(1);
            continue;
        }
        let mut props = Map::new();
        if let Some(pm) = f.get("properties").and_then(Value::as_object) {
            for key in js_keys(pm).into_iter().take(500) {
                let clean = crate::gjs::cut16(&crate::gjs::drop_controls(key), 64);
                if clean.is_empty() {
                    continue;
                }
                let v = pm.get(key).map_or(Value::Null, prop_value);
                props.insert(clean, v);
            }
        }
        let gs = match f.get("geometry") {
            None | Some(Value::Null) => {
                k.empty = k.empty.saturating_add(1);
                Vec::new()
            }
            Some(g) => geoms(g, 0, &mut k),
        };
        for g in gs.into_iter().flatten() {
            let slot = match g.kind() {
                "pt" => 0,
                "ln" => 1,
                _ => 2,
            };
            if let Some(list) = by.get_mut(slot) {
                list.push(Feature { g: Some(g), p: props.clone() });
            }
            n = n.saturating_add(1);
        }
    }
    if k.bad > 0 {
        notes.push(tr!("geojson.bad", name = name, n = k.bad));
    }
    if k.empty > 0 {
        notes.push(tr!("geojson.empty", name = name, n = k.empty));
    }
    if k.flattened {
        notes.push(tr!("geojson.flat", name = name));
    }
    if k.split {
        notes.push(tr!("geojson.multi", name = name));
    }
    let kinds: Vec<usize> = (0..3).filter(|&i| by.get(i).is_some_and(|l| !l.is_empty())).collect();
    let many = kinds.len() > 1;
    let mut layers = Vec::new();
    for i in kinds {
        let features = std::mem::take(by.get_mut(i).unwrap_or(&mut Vec::new()));
        let (kind, key) = match i {
            0 => ("pt", "geojson.layer.pt"),
            1 => ("ln", "geojson.layer.ln"),
            _ => ("pg", "geojson.layer.pg"),
        };
        let has = features.iter().any(|f| f.g.as_ref().is_some_and(has_z));
        layers.push(ReadLayer {
            name: if many { tr!(key, name = name) } else { name.to_string() },
            geom: kind.into(),
            has_z: has,
            fields: fields_of(&features),
            features,
            notes: Vec::new(),
            extra: Vec::new(),
        });
    }
    Ok(GeoJsonRead { layers, crs_code, notes })
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10 && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() })
}

/// `fieldsOf(features)`: number, bool, date (YYYY-MM-DD) or text; in first-seen order.
pub fn fields_of(features: &[Feature]) -> Vec<Field> {
    let mut order: Vec<(&str, Option<&'static str>)> = Vec::new();
    let mut at: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    // the slot the key at each place had in the feature before (features mostly share their keys' order)
    let mut hint: Vec<usize> = Vec::new();
    for f in features {
        let sorted: Vec<(&String, &Value)>;
        let entries: &mut dyn Iterator<Item = (&String, &Value)> = if f.p.keys().any(|k| crate::gjs::is_index(k)) {
            sorted = crate::gjs::js_entries(&f.p);
            &mut sorted.iter().copied()
        } else {
            &mut f.p.iter()
        };
        for (j, (k, v)) in entries.enumerate() {
            let i = match hint.get(j).copied().filter(|&i| order.get(i).is_some_and(|o| o.0 == k.as_str())) {
                Some(i) => i,
                None => {
                    let i = *at.entry(k.as_str()).or_insert_with(|| {
                        order.push((k.as_str(), None));
                        order.len().saturating_sub(1)
                    });
                    if let Some(h) = hint.get_mut(j) {
                        *h = i;
                    } else {
                        hint.push(i);
                    }
                    i
                }
            };
            let t = match v {
                Value::Null => continue,
                Value::String(s) if s.is_empty() => continue,
                Value::Number(_) => "number",
                Value::Bool(_) => "bool",
                Value::String(s) if is_date(s) => "date",
                _ => "text",
            };
            if let Some(slot) = order.get_mut(i) {
                slot.1 = Some(if slot.1.is_none() || slot.1 == Some(t) { t } else { "text" });
            }
        }
    }
    order.into_iter().map(|(k, t)| Field { ty: t.unwrap_or("text").to_string(), name: k.to_string() }).collect()
}

/// A group to write.
#[derive(Clone, Debug)]
pub struct WGroup {
    pub kind: String,
    pub geom: String,
    pub has_z: bool,
    pub fields: Vec<Field>,
    pub features: Vec<Feature>,
}

fn r9(v: f64) -> f64 {
    round(v * 1e9) / 1e9
}
fn r4(v: f64) -> f64 {
    round(v * 1e4) / 1e4
}

/// A group to write, borrowed: (kind, has z, fields, features).
pub type WGroupRef<'a> = (&'a str, bool, &'a [Field], &'a [Feature]);

/// `writeGeoJSON(groups, { toLonLat, zToMetres, name })`.
pub fn write_geojson(
    groups: &[WGroup],
    to_lon_lat: &dyn Fn(f64, f64) -> (f64, f64),
    z_to_metres: &dyn Fn(f64) -> f64,
    name: Option<&str>,
) -> String {
    let refs: Vec<WGroupRef> = groups.iter().map(|g| (g.kind.as_str(), g.has_z, g.fields.as_slice(), g.features.as_slice())).collect();
    write_geojson_parts(&refs, to_lon_lat, z_to_metres, name)
}

/// A number as `JSON.stringify` writes the writer's `num(x)` (−0 as 0; NaN / ±∞ as null).
fn put_num(out: &mut String, x: f64) {
    if x.is_finite() {
        cc_cad::js::push_num(out, if x == 0.0 { 0.0 } else { x });
    } else {
        out.push_str("null");
    }
}

/// [`write_geojson`] written straight into the text (the same text as [`write_geojson_tree`], which builds a JSON
/// tree first): points and lines converted as they are written, polygons converted and wound first.
pub fn write_geojson_parts(
    groups: &[WGroupRef],
    to_lon_lat: &dyn Fn(f64, f64) -> (f64, f64),
    z_to_metres: &dyn Fn(f64) -> f64,
    name: Option<&str>,
) -> String {
    let mut out = String::new();
    out.push_str("{\"type\":\"FeatureCollection\",");
    if let Some(n) = name.filter(|n| !n.is_empty()) {
        out.push_str("\"name\":");
        crate::gjs::push_str_json(&mut out, n);
        out.push(',');
    }
    out.push_str("\"features\":[");
    let mut first = true;
    for &(kind, has_z, fields, features) in groups {
        // a position: [x, y] or [x, y, z] (converted already, or converted here)
        let put = |out: &mut String, p: &Pos, convert: bool| {
            let (x, y) = if convert { to_lon_lat(p.g(0), p.g(1)) } else { (p.g(0), p.g(1)) };
            out.push('[');
            put_num(out, r9(x));
            out.push(',');
            put_num(out, r9(y));
            if has_z && p.len() > 2 {
                out.push(',');
                put_num(out, r4(z_to_metres(p.g(2))));
            }
            out.push(']');
        };
        let line = |out: &mut String, l: &[Pos], convert: bool| {
            out.push('[');
            for (i, p) in l.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                put(out, p, convert);
            }
            out.push(']');
        };
        let poly = |out: &mut String, rings: &[Vec<Pos>]| {
            out.push('[');
            for (i, r) in rings.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                line(out, r, false);
            }
            out.push(']');
        };
        for f in features {
            if !first {
                out.push(',');
            }
            first = false;
            let mut properties = Map::new();
            properties.insert("kind".into(), Value::String(kind.to_string()));
            for fd in fields {
                let v = f.p.get(&fd.name).cloned().unwrap_or(Value::Null);
                properties.insert(if fd.name == "kind" { "kind_".into() } else { fd.name.clone() }, v);
            }
            out.push_str("{\"type\":\"Feature\",\"properties\":");
            crate::gjs::push_json(&mut out, &Value::Object(properties));
            out.push_str(",\"geometry\":");
            match &f.g {
                None => out.push_str("null"),
                Some(Geom::Pt(c)) => {
                    out.push_str("{\"type\":\"Point\",\"coordinates\":");
                    put(&mut out, c, true);
                    out.push('}');
                }
                Some(Geom::Ln(c)) => {
                    if let [one] = c.as_slice() {
                        out.push_str("{\"type\":\"LineString\",\"coordinates\":");
                        line(&mut out, one, true);
                    } else {
                        out.push_str("{\"type\":\"MultiLineString\",\"coordinates\":[");
                        for (i, l) in c.iter().enumerate() {
                            if i > 0 {
                                out.push(',');
                            }
                            line(&mut out, l, true);
                        }
                        out.push(']');
                    }
                    out.push('}');
                }
                Some(g0 @ Geom::Pg(_)) => {
                    let g = orient(&map_xy(g0, &mut |x, y| to_lon_lat(x, y)), true);
                    let Geom::Pg(c) = &g else {
                        out.push_str("null}");
                        continue;
                    };
                    if let [one] = c.as_slice() {
                        out.push_str("{\"type\":\"Polygon\",\"coordinates\":");
                        poly(&mut out, one);
                    } else {
                        out.push_str("{\"type\":\"MultiPolygon\",\"coordinates\":[");
                        for (i, p) in c.iter().enumerate() {
                            if i > 0 {
                                out.push(',');
                            }
                            poly(&mut out, p);
                        }
                        out.push(']');
                    }
                    out.push('}');
                }
            }
            out.push('}');
        }
    }
    out.push_str("]}");
    out
}

/// writeGeoJSON through a JSON tree: the first Rust form, kept as the reference [`write_geojson`] is tested against.
pub fn write_geojson_tree(
    groups: &[WGroup],
    to_lon_lat: &dyn Fn(f64, f64) -> (f64, f64),
    z_to_metres: &dyn Fn(f64) -> f64,
    name: Option<&str>,
) -> String {
    let mut features: Vec<Value> = Vec::new();
    for gr in groups {
        for f in &gr.features {
            let mut properties = Map::new();
            properties.insert("kind".into(), Value::String(gr.kind.clone()));
            for fd in &gr.fields {
                let v = f.p.get(&fd.name).cloned().unwrap_or(Value::Null);
                properties.insert(if fd.name == "kind" { "kind_".into() } else { fd.name.clone() }, v);
            }
            let Some(g0) = &f.g else {
                let mut o = Map::new();
                o.insert("type".into(), Value::String("Feature".into()));
                o.insert("properties".into(), Value::Object(properties));
                o.insert("geometry".into(), Value::Null);
                features.push(Value::Object(o));
                continue;
            };
            let mut g = map_xy(g0, &mut |x, y| to_lon_lat(x, y));
            if matches!(g, Geom::Pg(_)) {
                g = orient(&g, true);
            }
            let pp = |p: &Pos| -> Value {
                if gr.has_z && p.len() > 2 {
                    Value::Array(vec![num(r9(p.g(0))), num(r9(p.g(1))), num(r4(z_to_metres(p.g(2))))])
                } else {
                    Value::Array(vec![num(r9(p.g(0))), num(r9(p.g(1)))])
                }
            };
            let line = |l: &Vec<Pos>| Value::Array(l.iter().map(pp).collect());
            let (ty, coords) = match &g {
                Geom::Pt(c) => ("Point", pp(c)),
                Geom::Ln(c) if c.len() == 1 => ("LineString", line(c.first().unwrap_or(&Vec::new()))),
                Geom::Ln(c) => ("MultiLineString", Value::Array(c.iter().map(line).collect())),
                Geom::Pg(c) if c.len() == 1 => {
                    ("Polygon", Value::Array(c.first().map(|p| p.iter().map(line).collect()).unwrap_or_default()))
                }
                Geom::Pg(c) => ("MultiPolygon", Value::Array(c.iter().map(|p| Value::Array(p.iter().map(line).collect())).collect())),
            };
            let mut geometry = Map::new();
            geometry.insert("type".into(), Value::String(ty.into()));
            geometry.insert("coordinates".into(), coords);
            let mut o = Map::new();
            o.insert("type".into(), Value::String("Feature".into()));
            o.insert("properties".into(), Value::Object(properties));
            o.insert("geometry".into(), Value::Object(geometry));
            features.push(Value::Object(o));
        }
    }
    let mut out = Map::new();
    out.insert("type".into(), Value::String("FeatureCollection".into()));
    if let Some(n) = name.filter(|n| !n.is_empty()) {
        out.insert("name".into(), Value::String(n.into()));
    }
    out.insert("features".into(), Value::Array(features));
    stringify(&Value::Object(out))
}
