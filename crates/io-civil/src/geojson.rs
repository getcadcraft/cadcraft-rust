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

/// `readGeoJSON(text, name, { maxFeatures })`.
pub fn read_geojson(text: &str, name: &str, max_features: usize) -> Result<GeoJsonRead, GeoJsonError> {
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
    for f in features {
        for k in js_keys(&f.p) {
            let i = *at.entry(k.as_str()).or_insert_with(|| {
                order.push((k.as_str(), None));
                order.len().saturating_sub(1)
            });
            let v = f.p.get(k).unwrap_or(&Value::Null);
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

/// `writeGeoJSON(groups, { toLonLat, zToMetres, name })`.
pub fn write_geojson(
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
