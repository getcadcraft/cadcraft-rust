//! gis-state.js — the GIS layers' part of the project file: layers (`state.gis.layers`), map layers (`state.gis.maps`,
//! shape only) and the features held outside the undo snapshots (`gisFeatures`: layer id → [{g, p}]).
//!
//! The project file is untrusted: layer ids `/^g\d{1,9}$/`, kinds, field names and types, colours, geometry shapes and
//! finite numbers are checked; sizes capped — 200 layers, 2 million features, 20 million vertices.

use crate::js::{J, Obj, arr, slice16, strip_controls, trim};
use crate::tr;
use serde_json::{Map, Value};

pub const LAYER_TOKENS: [&str; 7] = ["blue", "orange", "magenta", "cyan", "ochre", "green", "red"];
pub const MAX_LAYERS: usize = 200;
const MAX_FILE_FEATURES: usize = 2_000_000;
const MAX_VERTS: i64 = 20_000_000;

/// `clean(v, n)`: control characters → spaces, trimmed, at most n UTF-16 units.
pub fn clean(v: Option<&Value>, n: usize) -> String {
    slice16(trim(&strip_controls(&crate::js::string_or_empty(v), " ")), n)
}
/// `clean` for text.
pub fn clean_str(v: &str, n: usize) -> String {
    slice16(trim(&strip_controls(v, " ")), n)
}

/// A position `[x, y]` or `[x, y, z]`.
pub type Pos = Vec<f64>;

/// A feature geometry `{t: 'pt'|'ln'|'pg', c}`.
#[derive(Clone, Debug, PartialEq)]
pub enum Geom {
    Pt(Pos),
    /// parts
    Ln(Vec<Vec<Pos>>),
    /// polygons of rings
    Pg(Vec<Vec<Vec<Pos>>>),
}
impl Geom {
    pub fn kind(&self) -> &'static str {
        match self {
            Geom::Pt(_) => "pt",
            Geom::Ln(_) => "ln",
            Geom::Pg(_) => "pg",
        }
    }
}
impl J for Geom {
    fn j(&self) -> Value {
        match self {
            Geom::Pt(c) => crate::jo! { "t" => "pt", "c" => c },
            Geom::Ln(c) => crate::jo! { "t" => "ln", "c" => c },
            Geom::Pg(c) => crate::jo! { "t" => "pg", "c" => c },
        }
    }
}

/// A feature `{g, p}` (g may be null).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Feature {
    pub g: Option<Geom>,
    pub p: Map<String, Value>,
}
impl J for Feature {
    fn j(&self) -> Value {
        crate::jo! { "g" => self.g, "p" => Value::Object(self.p.clone()) }
    }
}

/// A layer field.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: String,
    pub ty: String,
}
impl J for Field {
    fn j(&self) -> Value {
        crate::jo! { "name" => self.name, "type" => self.ty }
    }
}
/// `layer.source`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Source {
    pub file: String,
    pub format: String,
    pub crs: Option<String>,
}
impl J for Source {
    fn j(&self) -> Value {
        crate::jo! { "file" => self.file, "format" => self.format, "crs" => self.crs }
    }
}
/// A GIS layer.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub id: String,
    pub name: String,
    pub geom: String,
    pub has_z: bool,
    pub fields: Vec<Field>,
    pub visible: bool,
    pub color: String,
    pub source: Source,
    /// `label: {field}` (0.4 V7)
    pub label: Option<String>,
}
impl J for Layer {
    fn j(&self) -> Value {
        Obj::new()
            .set("id", &self.id)
            .set("name", &self.name)
            .set("geom", &self.geom)
            .set("hasZ", self.has_z)
            .set("fields", &self.fields)
            .set("visible", self.visible)
            .set("color", &self.color)
            .set("source", &self.source)
            .opt("label", self.label.as_ref().map(|f| crate::jo! { "field" => f }))
            .done()
    }
}

/// `state.gis`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Gis {
    pub layers: Vec<Layer>,
    /// map layers: plain objects of strings / finite numbers / booleans / null
    pub maps: Vec<Map<String, Value>>,
}
impl J for Gis {
    fn j(&self) -> Value {
        crate::jo! { "layers" => self.layers, "maps" => self.maps.iter().map(|m| Value::Object(m.clone())).collect::<Vec<_>>() }
    }
}

/// The features of every layer, in layer insertion order (a JS Map). A layer's list is shared (`Arc`): the undo step,
/// the edit's copy and the project hold one list until one of them changes it (`Arc::make_mut`).
pub type Features = Vec<(String, std::sync::Arc<Vec<Feature>>)>;

/// `gisFeaturesJSON(map)` → `{ id: [[geometry, props], …] }`.
pub fn gis_features_json(map: &Features) -> Value {
    let mut out = Map::new();
    for (id, fs) in map {
        out.insert(id.clone(), Value::Array(fs.iter().map(|f| Value::Array(vec![f.g.j(), Value::Object(f.p.clone())])).collect()));
    }
    Value::Object(out)
}

fn fin(v: &Value) -> Option<f64> {
    v.as_f64().filter(|x| x.is_finite())
}

/// `readGisState(gis, gisFeatures, notes)` → (state.gis, features).
pub fn read_gis_state(gis: Option<&Value>, gis_features: Option<&Value>, notes: &mut Vec<String>) -> (Gis, Features) {
    let mut out = Gis::default();
    let mut features: Features = Vec::new();
    let gis = match gis {
        None | Some(Value::Null) => return (out, features),
        Some(g) => g,
    };
    if !matches!(gis, Value::Object(_) | Value::Array(_)) {
        notes.push(tr!("gstate.unreadable"));
        return (out, features);
    }
    let mut budget: i64 = MAX_VERTS;
    let mut total = 0usize;
    let mut dropped = 0usize;
    let pos_ok = |p: &Value| -> bool {
        let Some(a) = p.as_array() else { return false };
        (a.len() == 2 || a.len() == 3)
            && a.iter().all(|x| fin(x).is_some())
            && a.first().and_then(fin).is_some_and(|x| x.abs() < 1e10)
            && a.get(1).and_then(fin).is_some_and(|x| x.abs() < 1e10)
    };
    let take_pos = |p: &Value, budget: &mut i64| -> Pos {
        *budget = budget.saturating_sub(1);
        arr(Some(p)).iter().filter_map(fin).collect()
    };
    // geometry: Ok(None) = null, Err = undefined (dropped)
    let geom = |g: &Value, kind: &str, budget: &mut i64| -> Result<Option<Geom>, ()> {
        if g.is_null() {
            return Ok(None);
        }
        let Some(o) = g.as_object() else { return Err(()) };
        if o.get("t").and_then(Value::as_str) != Some(kind) || !o.get("c").is_some_and(Value::is_array) {
            return Err(());
        }
        let c = o.get("c").unwrap_or(&Value::Null);
        match kind {
            "pt" => {
                if pos_ok(c) {
                    Ok(Some(Geom::Pt(take_pos(c, budget))))
                } else {
                    Err(())
                }
            }
            "ln" => {
                let mut parts = Vec::new();
                for q in arr(Some(c)) {
                    let Some(qa) = q.as_array() else { continue };
                    let len = i64::try_from(qa.len()).unwrap_or(i64::MAX);
                    if qa.len() >= 2 && len <= *budget && qa.iter().all(pos_ok) {
                        parts.push(qa.iter().map(|p| take_pos(p, budget)).collect::<Vec<_>>());
                    }
                }
                if parts.is_empty() { Err(()) } else { Ok(Some(Geom::Ln(parts))) }
            }
            _ => {
                let mut polys = Vec::new();
                for poly in arr(Some(c)) {
                    let Some(pa) = poly.as_array() else { continue };
                    if pa.is_empty() {
                        continue;
                    }
                    let mut rings = Vec::new();
                    for r in pa {
                        let Some(ra) = r.as_array() else { continue };
                        let len = i64::try_from(ra.len()).unwrap_or(i64::MAX);
                        if ra.len() >= 4 && len <= *budget && ra.iter().all(pos_ok) {
                            rings.push(ra.iter().map(|p| take_pos(p, budget)).collect::<Vec<_>>());
                        }
                    }
                    if !rings.is_empty() {
                        polys.push(rings);
                    }
                }
                if polys.is_empty() { Err(()) } else { Ok(Some(Geom::Pg(polys))) }
            }
        }
    };
    let empty = Map::new();
    let src = gis_features.and_then(Value::as_object).unwrap_or(&empty);
    let mut seen: Vec<String> = Vec::new();
    for l in arr(gis.get("layers")).iter().take(MAX_LAYERS) {
        let id_text = l.get("id").map(crate::js::to_js_string).unwrap_or_else(|| "undefined".into());
        let id_ok = id_text.len() >= 2
            && id_text.len() <= 10
            && id_text.starts_with('g')
            && id_text.get(1..).is_some_and(|d| d.bytes().all(|b| b.is_ascii_digit()));
        let geom_kind = l.get("geom").and_then(Value::as_str).unwrap_or("");
        // seen.has(l.id) compares the raw value: ids are strings here (a number id never passes the test above as
        // a string… it does: String(5) = "5" fails /^g/); keep the text
        if !l.is_object() || !id_ok || seen.contains(&id_text) || !["pt", "ln", "pg"].contains(&geom_kind) {
            dropped = dropped.saturating_add(1);
            continue;
        }
        seen.push(id_text.clone());
        let mut field_names: Vec<String> = Vec::new();
        let mut fields = Vec::new();
        for f in arr(l.get("fields")).iter().take(500) {
            let name = clean(f.get("name"), 64);
            let ty = f.get("type").and_then(Value::as_str).filter(|t| ["text", "number", "date", "bool"].contains(t)).unwrap_or("text");
            if !name.is_empty() && !field_names.contains(&name) {
                field_names.push(name.clone());
                fields.push(Field { name, ty: ty.to_string() });
            }
        }
        let name = {
            let n = clean(l.get("name"), 120);
            if n.is_empty() { tr!("layers.default") } else { n }
        };
        let color = l.get("color").and_then(Value::as_str).filter(|c| LAYER_TOKENS.contains(c)).unwrap_or("blue").to_string();
        let s = l.get("source");
        let scrs = s.and_then(|s| s.get("crs"));
        let source = Source {
            file: clean(s.and_then(|s| s.get("file")), 200),
            format: clean(s.and_then(|s| s.get("format")), 40),
            crs: if crate::js::truthy(scrs) { Some(clean(scrs, 200)) } else { None },
        };
        let mut layer = Layer {
            id: id_text.clone(),
            name,
            geom: geom_kind.to_string(),
            has_z: l.get("hasZ") == Some(&Value::Bool(true)),
            fields,
            visible: l.get("visible") != Some(&Value::Bool(false)),
            color,
            source,
            label: None,
        };
        if let Some(lb) = l.get("label").filter(|v| v.is_object() || v.is_array()) {
            let f = clean(lb.get("field"), 64);
            if field_names.contains(&f) {
                layer.label = Some(f);
            }
        }
        let rows = src.get(&id_text).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
        let mut fs = Vec::new();
        let mut bad = 0usize;
        for r in rows {
            if total >= MAX_FILE_FEATURES || budget <= 0 {
                bad = bad.saturating_add(1);
                continue;
            }
            let Some(ra) = r.as_array().filter(|a| a.len() == 2) else {
                bad = bad.saturating_add(1);
                continue;
            };
            let g = match geom(ra.first().unwrap_or(&Value::Null), geom_kind, &mut budget) {
                Ok(g) => g,
                Err(()) => {
                    bad = bad.saturating_add(1);
                    continue;
                }
            };
            let mut p = Map::new();
            if let Some(props) = ra.get(1).and_then(Value::as_object) {
                for f in &layer.fields {
                    let Some(v) = props.get(&f.name) else { continue };
                    let nv = match v {
                        Value::Null | Value::Bool(_) => v.clone(),
                        Value::Number(n) if n.as_f64().is_some_and(f64::is_finite) => v.clone(),
                        Value::String(s) => {
                            let cleaned: String = s
                                .chars()
                                .map(|c| {
                                    let u = u32::from(c);
                                    if u <= 8 || u == 0x0b || u == 0x0c || (0x0e..=0x1f).contains(&u) || u == 0x7f { ' ' } else { c }
                                })
                                .collect();
                            Value::String(slice16(&cleaned, 10000))
                        }
                        _ => Value::Null,
                    };
                    p.insert(f.name.clone(), nv);
                }
            }
            fs.push(Feature { g, p });
            total = total.saturating_add(1);
        }
        if bad > 0 {
            notes.push(tr!("gstate.bad.features", n = bad, layer = layer.name.as_str()));
        }
        out.layers.push(layer);
        features.push((id_text, std::sync::Arc::new(fs)));
    }
    if dropped > 0 {
        notes.push(tr!("gstate.bad.layers", n = dropped));
    }
    // map layers: plain objects of strings, finite numbers and true/false, at most 100
    for m in arr(gis.get("maps")).iter().take(100) {
        let Some(mo) = m.as_object() else { continue };
        let mut o = Map::new();
        for (k, v) in mo.iter().take(50) {
            if k == "__proto__" || k.encode_utf16().count() > 40 {
                continue;
            }
            match v {
                Value::String(s) => {
                    o.insert(k.clone(), Value::String(if s.encode_utf16().count() > 40_000_000 { String::new() } else { s.clone() }));
                }
                Value::Bool(_) | Value::Null => {
                    o.insert(k.clone(), v.clone());
                }
                Value::Number(n) if n.as_f64().is_some_and(f64::is_finite) => {
                    o.insert(k.clone(), v.clone());
                }
                _ => {}
            }
        }
        out.maps.push(o);
    }
    (out, features)
}
