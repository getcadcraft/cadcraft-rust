//! gis-read.js — GIS vector files to project layers: reading (shapefile sets, zips, GeoJSON, KML/KMZ, GeoPackage), the
//! source coordinate system, what an import will do (planImport) and the layers in project coordinates (buildLayers).
//! `importInto` (one undo step on the project) sits on the project model.

use crate::gis_geom::{BBox, ReadLayer, bounds, bounds_into, map_xy, map_z};
use crate::gjs::utf8_lossy;
use cc_civil::gis_state::Feature;
use cc_civil::js::{J, slice16};
use cc_civil::tr;
use serde_json::{Value, json};

const MAX_FILE: usize = 200 * 1024 * 1024;

fn ext_of(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}
/// `isGisFile(name)`.
pub fn is_gis_file(name: &str) -> bool {
    name.contains('.')
        && matches!(ext_of(name).as_str(), "shp" | "shx" | "dbf" | "prj" | "cpg" | "zip" | "geojson" | "gpkg" | "kml" | "kmz")
}
/// `gisFileKind(name, head)`.
pub fn gis_file_kind(name: &str, head: &str) -> Option<&'static str> {
    if !name.contains('.') {
        return None;
    }
    match ext_of(name).as_str() {
        "shp" | "shx" | "dbf" | "cpg" => Some("shp"),
        "prj" => Some("prj"),
        "zip" => Some("zip"),
        "gpkg" => Some("gpkg"),
        "kml" => Some("kml"),
        "geojson" => Some("geojson"),
        "json" if json_type_head(head) => Some("geojson"),
        _ => None,
    }
}
/// `/"type"\s*:\s*"(FeatureCollection|Feature)"/.test(head)`.
fn json_type_head(head: &str) -> bool {
    let mut from = 0usize;
    while let Some(i) = head.get(from..).and_then(|r| r.find("\"type\"")).map(|k| k.saturating_add(from)) {
        let rest = head.get(i.saturating_add(6)..).unwrap_or("");
        let r = rest.trim_start_matches(cc_civil::js::is_ws);
        if let Some(r) = r.strip_prefix(':') {
            let r = r.trim_start_matches(cc_civil::js::is_ws);
            if r.starts_with("\"FeatureCollection\"") || r.starts_with("\"Feature\"") {
                return true;
            }
        }
        from = i.saturating_add(1);
    }
    false
}

/// A source system: `{ kind: 'wgs84' | 'crs' | 'none' | 'bad' | 'same', … }`.
#[derive(Clone, Debug)]
pub enum Src {
    Wgs84,
    Crs { crs: Value, name: String, matched: Option<Value>, geographic: bool, notes: Option<Vec<String>> },
    None,
    Bad(String),
    Same,
}
impl J for Src {
    fn j(&self) -> Value {
        match self {
            Src::Wgs84 => json!({ "kind": "wgs84" }),
            Src::None => json!({ "kind": "none" }),
            Src::Same => json!({ "kind": "same" }),
            Src::Bad(w) => json!({ "kind": "bad", "why": w }),
            Src::Crs { crs, name, matched, geographic, notes } => {
                let mut o = cc_civil::js::Obj::new()
                    .set("kind", "crs")
                    .set("crs", crs.clone())
                    .set("name", name)
                    .set("matched", matched.clone())
                    .set("geographic", *geographic);
                if let Some(n) = notes {
                    o = o.set("notes", n);
                }
                o.done()
            }
        }
    }
}
/// A source from JSON (for buildLayers overrides).
pub fn read_src(v: &Value) -> Src {
    match v.get("kind").and_then(Value::as_str) {
        Some("wgs84") => Src::Wgs84,
        Some("same") => Src::Same,
        Some("bad") => Src::Bad(cc_civil::js::string_or_empty(v.get("why"))),
        Some("crs") => Src::Crs {
            crs: v.get("crs").cloned().unwrap_or(Value::Null),
            name: cc_civil::js::string_or_empty(v.get("name")),
            matched: v.get("matched").filter(|m| !m.is_null()).cloned(),
            geographic: cc_civil::js::truthy(v.get("geographic")),
            notes: None,
        },
        _ => Src::None,
    }
}

fn entry_src(e: &crate::crs::Entry) -> Src {
    Src::Crs { crs: crate::crs::crs_from_entry(e), name: e.name.clone(), matched: Some(e.value()), geographic: e.geographic, notes: None }
}

/// `resolveSrc({ prj | code | srs })`. srs = (org, code, definition).
pub fn resolve_src(prj: Option<&str>, code: Option<f64>, srs: Option<(&str, f64, &str)>) -> Src {
    if let Some((org, code, def)) = srs {
        let epsg = org.to_uppercase() == "EPSG";
        if epsg && code == 4326.0 {
            return Src::Wgs84;
        }
        if epsg {
            match crate::crs::by_code(code) {
                Ok(Some(e)) => return entry_src(e),
                Err(e) => return Src::Bad(e),
                Ok(None) => {}
            }
        }
        if !def.is_empty() && def != "undefined" {
            return resolve_src(Some(def), None, None);
        }
        if epsg {
            return Src::Bad(tr!("gread.epsg.unknown.grid", code = code));
        }
        return Src::None;
    }
    if let Some(c) = code {
        if c == 4326.0 {
            return Src::Wgs84;
        }
        return match crate::crs::by_code(c) {
            Ok(Some(e)) => entry_src(e),
            Ok(None) => Src::Bad(tr!("gread.epsg.unknown", code = c)),
            Err(e) => Src::Bad(e),
        };
    }
    if let Some(p) = prj.filter(|p| !p.is_empty()) {
        return match crate::crs::crs_from_prj(p) {
            Ok(r) => {
                if r.matched.is_some_and(|m| m.code == 4326.0) {
                    return Src::Wgs84;
                }
                let geographic = r.crs.get("unit").and_then(Value::as_str) == Some("deg");
                let name = cc_civil::js::string_or_empty(r.crs.get("name"));
                Src::Crs { crs: r.crs.clone(), name, matched: r.matched.map(crate::crs::Entry::value), geographic, notes: Some(r.notes) }
            }
            Err(e) => Src::Bad(e),
        };
    }
    Src::None
}

/// A source read from files.
#[derive(Clone, Debug)]
pub struct Source {
    pub file: String,
    pub format: &'static str,
    pub layers: Vec<ReadLayer>,
    pub src: Src,
    pub notes: Vec<String>,
    /// format-specific keys: encoding (Shapefile), legacyCrs (GeoJSON), table (GeoPackage)
    pub extra: Vec<(String, Value)>,
}
impl J for Source {
    fn j(&self) -> Value {
        let mut o = cc_civil::js::Obj::new()
            .set("file", &self.file)
            .set("format", self.format)
            .set("layers", &self.layers)
            .set("src", &self.src)
            .set("notes", &self.notes);
        for (k, v) in &self.extra {
            o.put(k, v.clone());
        }
        o.done()
    }
}
fn legacy_crs(s: &Source) -> bool {
    s.extra.iter().any(|(k, v)| k == "legacyCrs" && v == &Value::Bool(true))
}

/// `readGisFiles(files, { maxFeatures })`.
pub fn read_gis_files(files: &[(String, Vec<u8>)], max_features: usize) -> (Vec<Source>, Vec<String>) {
    let mut sources = Vec::new();
    let mut notes = Vec::new();
    struct Flat {
        name: String,
        data: Vec<u8>,
        zip: Option<String>,
    }
    let mut flat: Vec<Flat> = Vec::new();
    for (name, data) in files {
        if data.len() > MAX_FILE {
            notes.push(tr!("gread.big", file = name.as_str()));
            continue;
        }
        let e = ext_of(name);
        if name.contains('.') && (e == "zip" || e == "kmz") {
            let entries = match crate::zipfile::list_zip(data) {
                Ok(l) => l,
                Err(err) => {
                    notes.push(tr!("gread.file.note", file = name.as_str(), note = err.0));
                    continue;
                }
            };
            let want: Vec<&crate::zipfile::Entry> = entries
                .iter()
                .filter(|en| {
                    !en.dir
                        && !hidden_path(&en.name)
                        && en.name.contains('.')
                        && matches!(ext_of(&en.name).as_str(), "shp" | "shx" | "dbf" | "prj" | "cpg" | "geojson" | "json" | "gpkg" | "kml")
                })
                .collect();
            if want.is_empty() {
                notes.push(tr!("kml.zip.empty", file = name.as_str()));
                continue;
            }
            let mut total: u64 = 0;
            for en in want {
                total = total.saturating_add(u64::from(en.size));
                if total > (MAX_FILE as u64).saturating_mul(2) {
                    notes.push(tr!("gread.zip.big", file = name.as_str()));
                    break;
                }
                match crate::zipfile::unzip_entry(data, en, crate::zipfile::MAX_ENTRY) {
                    Ok(d) => flat.push(Flat { name: en.name.clone(), data: d, zip: Some(name.clone()) }),
                    Err(err) => notes.push(tr!("gread.file.note", file = name.as_str(), note = err.0)),
                }
            }
        } else {
            flat.push(Flat { name: name.clone(), data: data.clone(), zip: None });
        }
    }
    // shapefile sets
    let is_shp_part = |n: &str| n.contains('.') && matches!(ext_of(n).as_str(), "shp" | "shx" | "dbf" | "prj" | "cpg");
    let shp_files: Vec<&Flat> = flat.iter().filter(|f| is_shp_part(&f.name)).collect();
    let named: Vec<(String, Vec<u8>)> = shp_files
        .iter()
        .map(|f| (format!("{}{}", f.zip.as_ref().map_or(String::new(), |z| format!("{z}/")), f.name), f.data.clone()))
        .collect();
    for set in crate::shapefile::shapefile_sets(&named) {
        let file = format!("{}.shp", set.name);
        if set.shp.is_none() {
            if set.dbf.is_some() {
                notes.push(tr!("gread.dbf.alone", name = set.name.as_str()));
            } else if set.prj.is_some() && !shp_files.iter().any(|f| ext_of(&f.name) == "shp") {
                notes.push(tr!("gread.prj.alone", name = set.name.as_str()));
            }
            continue;
        }
        match crate::shapefile::read_shapefile(&set, max_features) {
            Ok(mut layer) => {
                let prj = layer.extra.iter().find(|(k, _)| k == "prj").and_then(|(_, v)| v.as_str()).map(str::to_string);
                let src = match &prj {
                    Some(p) => resolve_src(Some(p), None, None),
                    None => Src::None,
                };
                let lnotes = std::mem::take(&mut layer.notes);
                sources.push(Source {
                    file,
                    format: "Shapefile",
                    layers: vec![layer],
                    src,
                    notes: lnotes,
                    extra: vec![("encoding".into(), Value::Null)],
                });
            }
            Err(e) => notes.push(e.0),
        }
    }
    for f in &flat {
        let head = if f.name.contains('.') && ext_of(&f.name) == "json" {
            utf8_lossy(f.data.get(..4096.min(f.data.len())).unwrap_or(&[]))
        } else {
            String::new()
        };
        match gis_file_kind(&f.name, &head) {
            Some("geojson") => {
                let text = utf8_lossy(&f.data);
                let base = f.name.rsplit('/').next().unwrap_or(&f.name);
                let stem = strip_ext_ci(base, &[".geojson", ".json"]);
                match crate::geojson::read_geojson(&text, &stem, max_features) {
                    Ok(r) => {
                        if r.layers.is_empty() {
                            notes.push(tr!("gread.geojson.empty", file = f.name.as_str()));
                            continue;
                        }
                        let legacy = r.crs_code.is_some_and(|c| c != 0.0 && !c.is_nan());
                        let src = if legacy { resolve_src(None, r.crs_code, None) } else { Src::Wgs84 };
                        sources.push(Source {
                            file: f.name.clone(),
                            format: "GeoJSON",
                            layers: r.layers,
                            src,
                            notes: r.notes,
                            extra: vec![("legacyCrs".into(), Value::Bool(legacy))],
                        });
                    }
                    Err(e) => notes.push(e.0),
                }
            }
            Some("kml") => {
                let base_name = f.zip.clone().unwrap_or_else(|| f.name.clone());
                let base = base_name.rsplit('/').next().unwrap_or(&base_name);
                let stem = strip_ext_ci(base, &[".kml", ".kmz"]);
                match crate::kml::read_kml(&utf8_lossy(&f.data), &stem, max_features) {
                    Ok((layers, knotes)) => {
                        if layers.is_empty() {
                            notes.push(tr!("kml.empty", file = f.name.as_str()));
                            continue;
                        }
                        let file = match &f.zip {
                            Some(z) => format!("{z} \u{203a} {}", f.name),
                            None => f.name.clone(),
                        };
                        sources.push(Source { file, format: "KML", layers, src: Src::Wgs84, notes: knotes, extra: Vec::new() });
                    }
                    Err(e) => notes.push(e.0),
                }
            }
            Some("gpkg") => match crate::gpkg::open_gpkg(&f.data) {
                Err(e) => notes.push(tr!("gread.file.note", file = f.name.as_str(), note = e.0)),
                Ok(gp) => {
                    if gp.tables.is_empty() {
                        notes.push(tr!("gread.gpkg.notables", file = f.name.as_str()));
                        continue;
                    }
                    for tb in &gp.tables {
                        let ls = match gp.read(&tb.name, max_features) {
                            Ok(l) => l,
                            Err(e) => {
                                // the JS's try covers the whole file: the first failing table ends it
                                notes.push(tr!("gread.file.note", file = f.name.as_str(), note = e.0));
                                break;
                            }
                        };
                        if ls.is_empty() {
                            notes.push(tr!("gread.gpkg.empty", file = f.name.as_str(), table = tb.identifier.as_str()));
                            continue;
                        }
                        let src = if tb.srs_id == -1.0 || tb.srs_id == 0.0 || tb.srs.is_none() {
                            if tb.srs_id == 0.0 { Src::Bad(tr!("gread.gpkg.undefgeo")) } else { Src::None }
                        } else {
                            let (o, c, d) = tb.srs.clone().unwrap_or_default();
                            resolve_src(None, None, Some((&o, c, &d)))
                        };
                        let lnotes = ls.first().map(|l| l.notes.clone()).unwrap_or_default();
                        sources.push(Source {
                            file: format!("{} \u{203a} {}", f.name, tb.identifier),
                            format: "GeoPackage",
                            layers: ls,
                            src,
                            notes: lnotes,
                            extra: vec![("table".into(), Value::String(tb.name.clone()))],
                        });
                    }
                }
            },
            _ => {}
        }
    }
    (sources, notes)
}
/// `(^|/)(__MACOSX|\.)` in a path.
fn hidden_path(n: &str) -> bool {
    n.starts_with("__MACOSX") || n.starts_with('.') || n.contains("/__MACOSX") || n.contains("/.")
}
fn strip_ext_ci(name: &str, exts: &[&str]) -> String {
    let lower = name.to_ascii_lowercase();
    for e in exts {
        if lower.ends_with(e) {
            return name.get(..name.len().saturating_sub(e.len())).unwrap_or("").to_string();
        }
    }
    name.to_string()
}

/// `srcName(src)`.
pub fn src_name(src: &Src) -> String {
    match src {
        Src::Wgs84 => tr!("gread.src.wgs84"),
        Src::Crs { name, .. } => name.clone(),
        Src::Bad(_) => tr!("gread.src.bad"),
        _ => tr!("gread.src.none"),
    }
}
fn unit_word(u: &str) -> Option<String> {
    match u {
        "m" => Some(tr!("gread.unit.m")),
        "ft" => Some(tr!("gread.unit.ft")),
        "ftUS" => Some(tr!("gread.unit.ftus")),
        _ => None,
    }
}
fn crs_unit(c: &Value) -> String {
    cc_civil::js::string_or_empty(c.get("unit"))
}

/// `middleLonLat(sources)`.
pub fn middle_lon_lat(sources: &[Source]) -> Option<[f64; 2]> {
    for s in sources {
        if !matches!(s.src, Src::Wgs84 | Src::Crs { .. }) {
            continue;
        }
        let mut b = BBox::default();
        for l in &s.layers {
            for f in &l.features {
                bounds_into(f.g.as_ref(), &mut b);
            }
        }
        if !b.min_x.is_finite() {
            continue;
        }
        let cx = (b.min_x + b.max_x) / 2.0;
        let cy = (b.min_y + b.max_y) / 2.0;
        let ll = match &s.src {
            Src::Wgs84 => Some([cx, cy]),
            Src::Crs { crs, .. } => crate::crs::transform(crs, &Value::Null, cx, cy).ok().map(crate::crs::nn),
            _ => None,
        };
        if let Some(ll) = ll
            && ll.iter().all(|v| v.is_finite())
        {
            return Some(ll);
        }
    }
    None
}

/// `planImport(state, sources)` (state = its crs and units).
pub fn plan_import(state_crs: &Value, units: &str, sources: &[Source]) -> Value {
    let unknown = sources.iter().any(|s| matches!(s.src, Src::None));
    let near = middle_lon_lat(sources);
    if cc_civil::js::truthy(Some(state_crs)) {
        return json!({ "case": "project", "project": state_crs, "offer": null, "near": near.map(|n| n.j()), "unknown": unknown, "needPick": false });
    }
    let fam: &[&str] = if units == "m" { &["m"] } else { &["ft", "ftUS"] };
    let known: Vec<&Source> = sources.iter().filter(|s| matches!(s.src, Src::Crs { .. } | Src::Wgs84)).collect();
    if known.is_empty() {
        return json!({ "case": "none", "project": null, "offer": null, "near": null, "unknown": unknown, "needPick": false });
    }
    if let Some(p) =
        known.iter().find(|s| matches!(&s.src, Src::Crs { crs, geographic, .. } if !geographic && fam.contains(&crs_unit(crs).as_str())))
        && let Src::Crs { crs, .. } = &p.src
    {
        return json!({ "case": "offer", "project": null, "offer": crs, "near": near.map(|n| n.j()), "unknown": unknown, "needPick": false });
    }
    let k = known.first().map(|s| s.src.clone()).unwrap_or(Src::None);
    let why = match &k {
        Src::Wgs84 => tr!("gread.why.latlon"),
        Src::Crs { geographic: true, .. } => tr!("gread.why.latlon"),
        Src::Crs { crs, name, .. } => {
            let u = crs_unit(crs);
            tr!(
                "gread.why.units",
                name = name.as_str(),
                unit = unit_word(&u).unwrap_or(u),
                project = unit_word(if units == "m" { "m" } else { "ft" }).unwrap_or_default()
            )
        }
        _ => String::new(),
    };
    json!({ "case": "pick", "project": null, "offer": null, "near": near.map(|n| n.j()), "unknown": unknown, "needPick": true, "why": why })
}

fn unit_m(u: &str) -> Option<f64> {
    match u {
        "m" => Some(1.0),
        "ft" => Some(0.3048),
        "ftUS" => Some(1200.0 / 3937.0),
        _ => None,
    }
}

/// A layer ready for addLayers.
#[derive(Clone, Debug)]
pub struct BuiltLayer {
    pub name: String,
    pub geom: String,
    pub has_z: bool,
    pub fields: Vec<cc_civil::gis_state::Field>,
    pub features: Vec<Feature>,
    pub source: (String, String, Option<String>),
}
impl J for BuiltLayer {
    fn j(&self) -> Value {
        json!({
            "name": self.name, "geom": self.geom, "hasZ": self.has_z, "fields": self.fields.j(), "features": self.features.j(),
            "source": { "file": self.source.0, "format": self.source.1, "crs": self.source.2 },
        })
    }
}

/// `buildLayers(sources, { project, units, overrides })`.
pub fn build_layers(sources: &[Source], project: &Value, units: &str, overrides: &[(usize, Src)]) -> (Vec<BuiltLayer>, Vec<String>) {
    let mut notes = Vec::new();
    let mut layers = Vec::new();
    let has_project = cc_civil::js::truthy(Some(project));
    let proj_m = if has_project {
        unit_m(&crs_unit(project)).unwrap_or(f64::NAN)
    } else if units == "m" {
        1.0
    } else {
        1200.0 / 3937.0
    };
    for (si, s) in sources.iter().enumerate() {
        let src = overrides.iter().find(|(i, _)| *i == si).map_or_else(|| s.src.clone(), |(_, o)| o.clone());
        let mut fwd: Option<crate::crs::Frame> = None;
        let mut zk = 1.0;
        let mut geo = false;
        let file = s.file.as_str();
        let mut src_def = Value::Null;
        match &src {
            Src::Same | Src::None => {
                if matches!(src, Src::None) && has_project {
                    notes.push(tr!("gread.nocrs.same", file = file));
                }
            }
            Src::Bad(why) => {
                notes.push(tr!("gread.bad.crs", file = file, why = why.as_str()));
                continue;
            }
            _ => {
                if !has_project {
                    notes.push(tr!("gread.noproject", file = file, crs = src_name(&src)));
                    continue;
                }
                geo = matches!(src, Src::Wgs84 | Src::Crs { geographic: true, .. });
                src_def = match &src {
                    Src::Crs { crs, .. } => crs.clone(),
                    _ => Value::Null,
                };
                match crate::crs::frame(project) {
                    Ok(Some(f)) => {
                        // f.from(srcDef) throws for a definition proj4 cannot read: the JS says so here
                        if let Err(e) = f.from(&src_def, 0.0, 0.0) {
                            notes.push(tr!("gread.noconvert", file = file, why = e));
                            continue;
                        }
                        fwd = Some(f);
                    }
                    Ok(None) => {}
                    Err(e) => {
                        notes.push(tr!("gread.noconvert", file = file, why = e));
                        continue;
                    }
                }
                let src_m = if geo {
                    1.0
                } else {
                    let u = match &src {
                        Src::Crs { crs, .. } => crs_unit(crs),
                        _ => String::new(),
                    };
                    let v = unit_m(&u).or_else(|| crate::crs::metres_per(&u)).unwrap_or(1.0);
                    if v != 0.0 && !v.is_nan() { v } else { 1.0 }
                };
                zk = src_m / proj_m;
            }
        }
        if ((s.format == "GeoJSON" && !legacy_crs(s)) || s.format == "KML") && !matches!(src, Src::Same | Src::None) {
            zk = 1.0 / proj_m;
        }
        for l in &s.layers {
            let mut bad = 0usize;
            let feats: Vec<Option<Feature>> = l
                .features
                .iter()
                .map(|f| {
                    let Some(g0) = &f.g else { return Some(f.clone()) };
                    if geo {
                        let b = bounds(Some(g0));
                        if b.min_x < -180.000001 || b.max_x > 180.000001 || b.min_y < -90.000001 || b.max_y > 90.000001 {
                            bad = bad.saturating_add(1);
                            return None;
                        }
                    }
                    let mut g = match &fwd {
                        Some(fr) => map_xy(g0, &mut |x, y| match fr.from(&src_def, x, y) {
                            Ok(p) => (p[0], p[1]),
                            Err(_) => (f64::NAN, f64::NAN),
                        }),
                        None => g0.clone(),
                    };
                    if zk != 1.0 {
                        g = map_z(&g, &|z| z * zk);
                    }
                    Some(Feature { g: Some(g), p: f.p.clone() })
                })
                .collect();
            if bad > 0 {
                notes.push(tr!("gread.outside", layer = l.name.as_str(), n = bad));
            }
            let finite = |f: &Feature| f.g.as_ref().is_none_or(|g| bounds(Some(g)).min_x.is_finite());
            if fwd.is_some() && feats.iter().flatten().any(|f| f.g.is_some() && !finite(f)) {
                notes.push(tr!("gread.someconvert", layer = l.name.as_str()));
            }
            let crs_text = match &src {
                Src::Same => {
                    if has_project {
                        Some(tr!("gread.src.asproject", name = cc_civil::js::string_or_empty(project.get("name"))))
                    } else {
                        None
                    }
                }
                Src::None => None,
                other => Some(src_name(other)),
            };
            layers.push(BuiltLayer {
                name: l.name.clone(),
                geom: l.geom.clone(),
                has_z: l.has_z,
                fields: l.fields.clone(),
                features: feats.into_iter().flatten().filter(|f| finite(f)).collect(),
                source: (s.file.clone(), s.format.to_string(), crs_text),
            });
        }
        if zk != 1.0 && s.layers.iter().any(|l| l.has_z) {
            let from = if s.format == "GeoJSON" || s.format == "KML" || geo {
                unit_word("m").unwrap_or_default()
            } else {
                match &src {
                    Src::Crs { crs, .. } => {
                        let u = crs_unit(crs);
                        unit_word(&u).unwrap_or(u)
                    }
                    _ => "undefined".into(),
                }
            };
            notes.push(tr!(
                "gread.z",
                file = file,
                from = from,
                to = unit_word(if proj_m == 1.0 { "m" } else { "ft" }).unwrap_or_default()
            ));
        }
        if fwd.is_some() {
            let differs = match &src {
                Src::Wgs84 => true,
                Src::Crs { crs, .. } => crs.get("proj4") != project.get("proj4"),
                _ => false,
            };
            let local = cc_civil::js::truthy(project.get("local"));
            if differs || local {
                notes.push(tr!(
                    if local { "gread.converted.local" } else { "gread.converted" },
                    file = file,
                    from = src_name(&src),
                    to = cc_civil::js::string_or_empty(project.get("name"))
                ));
            }
        }
    }
    (layers, notes)
}

/// `latLonPoints(points, crs)` → (points, bad).
pub fn lat_lon_points(points: &[cc_civil::points::Pt], crs: &Value) -> Result<(Vec<cc_civil::points::Pt>, usize), String> {
    let f = crate::crs::frame(crs)?.ok_or_else(|| "Cannot read properties of null (reading 'fromLonLat')".to_string())?;
    let mut out = Vec::new();
    let mut bad = 0usize;
    for p in points {
        let [x, y] = f.from_lon_lat(p.x, p.y)?;
        if !x.is_finite() || !y.is_finite() {
            bad = bad.saturating_add(1);
            continue;
        }
        let mut q = p.clone();
        q.x = x;
        q.y = y;
        out.push(q);
    }
    Ok((out, bad))
}

/// `crsFromLandXml(cs)` → (crs | null, why).
pub fn crs_from_landxml(cs: Option<&Value>) -> (Value, String) {
    let Some(cs) = cs.filter(|c| cc_civil::js::truthy(Some(c))) else { return (Value::Null, tr!("gread.lx.none")) };
    let code = cs.get("epsgCode").filter(|c| cc_civil::js::truthy(Some(c)));
    if let Some(c) = code
        && let Ok(Some(e)) = crate::crs::by_code_val(Some(c))
    {
        return (crate::crs::crs_from_entry(e), String::new());
    }
    if let Some(w) = cs.get("ogcWktCode").and_then(Value::as_str).filter(|w| !w.is_empty())
        && let Ok(r) = crate::crs::crs_from_prj(w)
    {
        return (r.crs, String::new());
    }
    if let Some(n) = cs.get("name").and_then(Value::as_str).filter(|n| !n.is_empty())
        && let Ok(list) = crate::crs::search(n, None, 5.0)
        && let Some(e) = list.into_iter().find(|x| x.name.to_lowercase() == n.to_lowercase())
    {
        return (crate::crs::crs_from_entry(e), String::new());
    }
    let why = match code {
        Some(c) => tr!("gread.lx.epsg", code = cc_civil::js::to_js_string(c)),
        None => {
            let n =
                cs.get("name").filter(|v| cc_civil::js::truthy(Some(v))).map_or_else(|| tr!("gread.lx.noname"), cc_civil::js::to_js_string);
            tr!("gread.lx.name", name = n)
        }
    };
    (Value::Null, why)
}

/// `slice16` kept in scope (file names in notes are not cut).
pub fn cut(s: &str, n: usize) -> String {
    slice16(s, n)
}
