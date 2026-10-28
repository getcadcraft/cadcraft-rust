//! gis-export.js — the writers behind GIS › Export (shapefiles in a zip, GeoJSON, KML / KMZ, GeoPackage), layer file
//! names, nesting loops into polygons, and the GeoPackage system row. Building the groups from a project (`exportKinds`,
//! `exportGroups`, `exportProject`) sits on the project model.

use crate::gis_geom::{point_in_ring, ring_area};
use cc_civil::gis_state::{Feature, Field, Pos};
use cc_civil::js::G;
use cc_civil::tr;
use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

/// `safeName(s, used)` (used holds lower-case names).
pub fn safe_name(s: &str, used: &mut Vec<String>) -> String {
    let mut b = String::new();
    let mut run = false;
    for c in s.nfkd() {
        if c.is_ascii_alphanumeric() || c == '_' {
            b.push(c);
            run = false;
        } else if !run {
            b.push('_');
            run = true;
        }
    }
    // [^A-Za-z0-9_]+ → '_' (a run of other characters is one '_', but an '_' itself is kept as is)
    let t = b.trim_matches('_');
    let mut base: String = t.chars().take(40).collect();
    if base.is_empty() {
        base = "layer".into();
    }
    if base.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        base = format!("l_{base}");
    }
    let mut k = base.clone();
    let mut j = 2u64;
    while used.contains(&k.to_lowercase()) {
        k = format!("{base}_{j}");
        j = j.saturating_add(1);
    }
    used.push(k.to_lowercase());
    k
}

fn close(r: Vec<Pos>) -> Vec<Pos> {
    match (r.first(), r.last()) {
        (Some(f), Some(l)) if f.g(0) != l.g(0) || f.g(1) != l.g(1) => {
            let f = f.clone();
            let mut r = r;
            r.push(f);
            r
        }
        _ => r,
    }
}

/// `nest(loops)`: loops nested by containment (even depth = outer, odd = hole of the loop around it).
pub fn nest(loops: &[Vec<(f64, f64)>]) -> Vec<Vec<Vec<Pos>>> {
    struct Rg {
        r: Vec<Pos>,
        a: f64,
        depth: usize,
        poly: Option<usize>,
    }
    let mut rings: Vec<Rg> = loops
        .iter()
        .map(|l| close(l.iter().map(|&(x, y)| vec![x, y]).collect()))
        .filter(|r| r.len() >= 4)
        .map(|r| Rg { a: ring_area(&r).abs(), r, depth: 0, poly: None })
        .collect();
    cc_civil::js::sort_by(&mut rings, |a, b| b.a - a.a);
    let mut polys: Vec<Vec<Vec<Pos>>> = Vec::new();
    for i in 0..rings.len() {
        let first = rings.get(i).and_then(|r| r.r.first()).cloned().unwrap_or_default();
        let mut parent: Option<usize> = None;
        let mut j = i;
        while j > 0 {
            j = j.saturating_sub(1);
            if rings.get(j).is_some_and(|rj| point_in_ring(first.g(0), first.g(1), &rj.r)) {
                parent = Some(j);
                break;
            }
        }
        let depth = parent.and_then(|p| rings.get(p)).map_or(0, |p| p.depth.saturating_add(1));
        let r = rings.get(i).map(|x| x.r.clone()).unwrap_or_default();
        if depth.is_multiple_of(2) {
            polys.push(vec![r]);
            if let Some(x) = rings.get_mut(i) {
                x.depth = depth;
                x.poly = Some(polys.len().saturating_sub(1));
            }
        } else {
            if let Some(x) = rings.get_mut(i) {
                x.depth = depth;
            }
            if let Some(pi) = parent.and_then(|p| rings.get(p)).and_then(|p| p.poly)
                && let Some(poly) = polys.get_mut(pi)
            {
                poly.push(r);
            }
        }
    }
    polys
}

/// A group as exportGroups makes it.
#[derive(Clone, Debug)]
pub struct EGroup {
    pub key: String,
    pub label: String,
    pub kind: String,
    pub file: String,
    pub geom: String,
    pub has_z: bool,
    pub fields: Vec<Field>,
    pub features: Vec<Feature>,
}

/// writeExport's result.
pub struct Exported {
    pub name: String,
    pub bytes: Vec<u8>,
    pub ty: &'static str,
    pub notes: Vec<String>,
}

/// `writeExport(groups, { format, prj, toLonLat, zToMetres, stem, srs })`.
#[allow(clippy::too_many_arguments)]
pub fn write_export(
    groups: &[EGroup],
    format: &str,
    prj: Option<&str>,
    to_lon_lat: Option<&dyn Fn(f64, f64) -> (f64, f64)>,
    z_to_metres: &dyn Fn(f64) -> f64,
    stem: &str,
    srs: Option<&crate::gpkg::Srs>,
    date: crate::zipfile::DosDate,
) -> Result<Exported, String> {
    let mut notes = Vec::new();
    if groups.is_empty() {
        return Err(tr!("gexp.nothing"));
    }
    match format {
        "shp" => {
            let mut files: Vec<(String, Vec<u8>)> = Vec::new();
            for g in groups {
                let out = crate::shapefile::write_shapefile(&g.geom, g.has_z, &g.fields, &g.features, (date.year, date.month, date.day));
                files.push((format!("{}.shp", g.file), out.shp));
                files.push((format!("{}.shx", g.file), out.shx));
                files.push((format!("{}.dbf", g.file), out.dbf));
                files.push((format!("{}.cpg", g.file), b"UTF-8".to_vec()));
                if let Some(p) = prj.filter(|p| !p.is_empty()) {
                    files.push((format!("{}.prj", g.file), p.as_bytes().to_vec()));
                }
                let renamed: Vec<(String, String)> =
                    g.fields.iter().enumerate().map(|(i, f)| (f.name.clone(), out.names.g(i))).filter(|(a, b)| a != b).collect();
                if !renamed.is_empty() {
                    let list: Vec<String> = renamed.iter().take(8).map(|(a, b)| format!("{a} \u{2192} {b}")).collect();
                    notes.push(tr!(
                        if renamed.len() > 8 { "gexp.shortened.more" } else { "gexp.shortened" },
                        kind = g.label.as_str(),
                        list = list.join(", ")
                    ));
                }
            }
            Ok(Exported { name: format!("{stem}-gis.zip"), bytes: crate::zipfile::zip(&files, true, date), ty: "application/zip", notes })
        }
        "geojson" => {
            let Some(t) = to_lon_lat else { return Err(tr!("gexp.geojson.crs")) };
            let wg: Vec<crate::geojson::WGroupRef> =
                groups.iter().map(|g| (g.kind.as_str(), g.has_z, g.fields.as_slice(), g.features.as_slice())).collect();
            let text = crate::geojson::write_geojson_parts(&wg, t, z_to_metres, Some(stem));
            Ok(Exported { name: format!("{stem}.geojson"), bytes: text.into_bytes(), ty: "application/geo+json", notes })
        }
        "kml" | "kmz" => {
            let Some(t) = to_lon_lat else { return Err(tr!("kml.need.crs")) };
            let kg: Vec<crate::kml::KGroup> =
                groups.iter().map(|g| crate::kml::KGroup { kind: &g.kind, has_z: g.has_z, features: &g.features }).collect();
            let text = crate::kml::write_kml(&kg, t, z_to_metres, stem);
            if format == "kml" {
                return Ok(Exported {
                    name: format!("{stem}.kml"),
                    bytes: text.into_bytes(),
                    ty: "application/vnd.google-earth.kml+xml",
                    notes,
                });
            }
            Ok(Exported {
                name: format!("{stem}.kmz"),
                bytes: crate::zipfile::zip(&[("doc.kml".into(), text.into_bytes())], true, date),
                ty: "application/vnd.google-earth.kmz",
                notes,
            })
        }
        "gpkg" => {
            let gg: Vec<crate::gpkg::GGroup> = groups
                .iter()
                .map(|g| crate::gpkg::GGroup {
                    table: &g.file,
                    label: &g.label,
                    geom: &g.geom,
                    has_z: g.has_z,
                    fields: &g.fields,
                    features: &g.features,
                })
                .collect();
            let bytes = crate::gpkg::write_gpkg(&gg, srs).map_err(|e| e.0)?;
            Ok(Exported { name: format!("{stem}.gpkg"), bytes, ty: "application/geopackage+sqlite3", notes })
        }
        _ => Err("Unknown format.".into()),
    }
}

/// `srsFor(crs, prj)`.
pub fn srs_for(crs: &Value, prj: Option<&str>) -> Option<crate::gpkg::Srs> {
    if !cc_civil::js::truthy(Some(crs)) {
        return None;
    }
    let def = prj.filter(|p| !p.is_empty()).map_or_else(|| "undefined".to_string(), str::to_string);
    let name = cc_civil::js::string_or_empty(crs.get("name"));
    match crs.get("code").and_then(Value::as_f64).filter(|c| *c != 0.0 && !c.is_nan()) {
        Some(c) => Some(crate::gpkg::Srs { id: c, org: "EPSG".into(), code: Some(c), name, definition: def }),
        None => Some(crate::gpkg::Srs { id: 100_000.0, org: "NONE".into(), code: Some(100_000.0), name, definition: def }),
    }
}
