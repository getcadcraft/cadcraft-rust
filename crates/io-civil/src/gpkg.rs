//! gpkg.js — OGC GeoPackage feature tables read and written. The JS goes through sql.js (SQLite in WebAssembly); this
//! port reads and writes the SQLite file itself (sqlite.rs): the same tables, columns, values and geometry blobs.
//!
//! Spec gap: the JS runs real SQL, so it would also read views and WITHOUT ROWID tables and names SQLite's own error
//! text when a table is damaged; here such a table is skipped or reported with the GeoPackage message. Written files are
//! not byte-identical to sql.js's (page layout, `last_change` time): parity is what reading them gives (forks/tools/
//! cvdiff/gpkg-check.mjs reads this writer's files with the frozen JS reader).

use crate::gis_geom::{BBox, ReadLayer, bounds_into, geom_kind, read_wkb, write_wkb};
use crate::gjs::{f64le, put};
use crate::sqlite::{Db, SVal, Writer, numeric_affinity};
use cc_civil::gis_state::{Feature, Field, Geom};
use cc_civil::js::{J, num_to_string, slice16, str_to_number};
use cc_civil::tr;
use serde_json::{Map, Value, json};

/// The JS GpkgError message.
#[derive(Clone, Debug)]
pub struct GpkgError(pub String);

/// A feature table of the file.
#[derive(Clone, Debug)]
pub struct Table {
    pub name: String,
    pub identifier: String,
    pub column: String,
    pub geom_type: String,
    pub geom: Option<&'static str>,
    pub srs_id: f64,
    pub z: f64,
    pub count: usize,
    pub srs: Option<(String, f64, String)>,
}
impl J for Table {
    fn j(&self) -> Value {
        json!({
            "name": self.name, "identifier": self.identifier, "column": self.column, "geomType": self.geom_type,
            "geom": self.geom, "srsId": self.srs_id.j(), "z": self.z.j(), "count": self.count,
            "srs": self.srs.as_ref().map(|(o, c, d)| json!({ "org": o, "code": c.j(), "definition": d })),
        })
    }
}

/// `String(v)` of a stored value.
fn sval_string(v: &SVal) -> String {
    match v {
        SVal::Null => "null".into(),
        SVal::Int(n) => num_to_string(*n as f64),
        SVal::Real(f) => num_to_string(*f),
        SVal::Text(s) => s.clone(),
        SVal::Blob(b) => b.iter().map(u8::to_string).collect::<Vec<_>>().join(","),
    }
}
/// `Number(v)`.
fn sval_number(v: &SVal) -> f64 {
    match v {
        SVal::Null => 0.0,
        SVal::Int(n) => *n as f64,
        SVal::Real(f) => *f,
        SVal::Text(s) => str_to_number(s),
        SVal::Blob(b) => match b.as_slice() {
            [] => 0.0,
            [x] => f64::from(*x),
            _ => f64::NAN,
        },
    }
}
fn truthy(v: &SVal) -> bool {
    match v {
        SVal::Null => false,
        SVal::Int(n) => *n != 0,
        SVal::Real(f) => *f != 0.0 && !f.is_nan(),
        SVal::Text(s) => !s.is_empty(),
        SVal::Blob(_) => true,
    }
}

/// Rows of a table by column name (case-blind, as SQLite resolves names).
struct Rows {
    cols: Vec<String>,
    rows: Vec<Vec<SVal>>,
}
impl Rows {
    fn get<'a>(&self, r: &'a [SVal], name: &str) -> Option<&'a SVal> {
        self.cols.iter().position(|c| c.eq_ignore_ascii_case(name)).and_then(|i| r.get(i))
    }
}
fn rows_of(db: &Db, table: &str) -> Result<Rows, String> {
    let cols = db.columns(table).map_err(|e| e.0)?.into_iter().map(|c| c.name).collect();
    let rows = db.rows(table).map_err(|e| e.0)?;
    Ok(Rows { cols, rows })
}

/// An open GeoPackage.
pub struct Gpkg<'a> {
    db: Db<'a>,
    pub tables: Vec<Table>,
}

/// `openGpkg(bytes)`.
pub fn open_gpkg(b: &[u8]) -> Result<Gpkg<'_>, GpkgError> {
    if b.len() < 100 || crate::gjs::utf8_lossy(b.get(..15).unwrap_or(&[])) != "SQLite format 3" {
        return Err(GpkgError(tr!("gpkg.notsqlite")));
    }
    let db = Db::open(b).map_err(|_| GpkgError(tr!("gpkg.damaged")))?;
    let unreadable = |why: String| GpkgError(tr!("gpkg.unreadable", why = why));
    if !db.has("gpkg_contents") || !db.has("gpkg_geometry_columns") {
        return Err(GpkgError(tr!("gpkg.notgpkg")));
    }
    let mut srs: Vec<(f64, (String, f64, String))> = Vec::new();
    if db.has("gpkg_spatial_ref_sys") {
        let r = rows_of(&db, "gpkg_spatial_ref_sys").map_err(unreadable)?;
        for row in r.rows.iter().take(1000) {
            let id = r.get(row, "srs_id").map_or(f64::NAN, sval_number);
            let org = r.get(row, "organization").filter(|v| **v != SVal::Null).map_or(String::new(), sval_string);
            let code = r.get(row, "organization_coordsys_id").map_or(f64::NAN, sval_number);
            let def = slice16(&r.get(row, "definition").filter(|v| **v != SVal::Null).map_or(String::new(), sval_string), 20000);
            // Map.set: a later row with the same id replaces the earlier
            match srs.iter_mut().find(|(k, _)| *k == id || (k.is_nan() && id.is_nan())) {
                Some(slot) => slot.1 = (org, code, def),
                None => srs.push((id, (org, code, def))),
            }
        }
    }
    let gc = rows_of(&db, "gpkg_geometry_columns").map_err(unreadable)?;
    let mut cols: Vec<(String, Vec<SVal>)> = Vec::new();
    for row in gc.rows.iter().take(1000) {
        let name = gc.get(row, "table_name").map_or_else(|| "undefined".into(), sval_string);
        let picked: Vec<SVal> = ["table_name", "column_name", "geometry_type_name", "srs_id", "z"]
            .iter()
            .map(|k| gc.get(row, k).cloned().unwrap_or(SVal::Null))
            .collect();
        match cols.iter_mut().find(|(k, _)| *k == name) {
            Some(slot) => slot.1 = picked,
            None => cols.push((name, picked)),
        }
    }
    let ct = rows_of(&db, "gpkg_contents").map_err(unreadable)?;
    let mut tables = Vec::new();
    let mut seen = 0usize;
    for row in &ct.rows {
        if ct.get(row, "data_type") != Some(&SVal::Text("features".into())) {
            continue;
        }
        seen = seen.saturating_add(1);
        if seen > 1000 {
            break;
        }
        let name = ct.get(row, "table_name").map_or_else(|| "undefined".into(), sval_string);
        let Some((_, g)) = cols.iter().find(|(k, _)| *k == name) else { continue };
        if !db.has(&name) {
            continue;
        }
        let Ok(count) = db.count(&name) else { continue };
        let g_srs = g.get(3).cloned().unwrap_or(SVal::Null);
        let srs_id = if g_srs == SVal::Null { ct.get(row, "srs_id").map_or(0.0, sval_number) } else { sval_number(&g_srs) };
        let ident_v = ct.get(row, "identifier").filter(|v| truthy(v)).map_or_else(|| name.clone(), sval_string);
        let gtn = g.get(2).cloned().unwrap_or(SVal::Null);
        let geom_type = if truthy(&gtn) { sval_string(&gtn) } else { "GEOMETRY".into() }.to_uppercase();
        let gk = geom_kind(&if truthy(&gtn) { sval_string(&gtn) } else { String::new() });
        let z = g.get(4).map_or(f64::NAN, sval_number);
        let srs_entry = if srs_id > 0.0 { srs.iter().find(|(k, _)| *k == srs_id).map(|(_, v)| v.clone()) } else { None };
        tables.push(Table {
            name,
            identifier: slice16(&ident_v, 120),
            column: g.get(1).map_or_else(|| "undefined".into(), sval_string),
            geom_type,
            geom: gk,
            srs_id,
            z: if z != 0.0 && !z.is_nan() { z } else { 0.0 },
            count,
            srs: srs_entry,
        });
    }
    Ok(Gpkg { db, tables })
}

fn upper_has(t: &str, w: &str) -> bool {
    t.contains(w)
}

impl Gpkg<'_> {
    /// `gp.read(tableName, { maxFeatures })`.
    pub fn read(&self, name: &str, max_features: usize) -> Result<Vec<ReadLayer>, GpkgError> {
        let Some(tb) = self.tables.iter().find(|t| t.name == name) else { return Err(GpkgError(tr!("gpkg.notable"))) };
        let ident = tb.identifier.as_str();
        let mut notes: Vec<String> = Vec::new();
        let info = self.db.columns(&tb.name).map_err(|e| GpkgError(tr!("gpkg.unreadable", why = e.0)))?;
        struct Fd {
            name: String,
            col: String,
            ty: &'static str,
        }
        let mut fields: Vec<Fd> = Vec::new();
        for c in &info {
            let ty = c.ty.to_uppercase();
            if c.name == tb.column || (c.pk > 0 && upper_has(&ty, "INT")) {
                continue;
            }
            if upper_has(&ty, "BLOB") {
                continue;
            }
            let fname = {
                let n = slice16(&crate::gjs::drop_controls(&c.name), 64);
                if n.is_empty() { "field".to_string() } else { n }
            };
            let t = if upper_has(&ty, "BOOL") {
                "bool"
            } else if ty == "DATE" {
                "date"
            } else if ["INT", "REAL", "FLOAT", "DOUBLE", "NUMERIC", "DECIMAL"].iter().any(|w| upper_has(&ty, w)) {
                "number"
            } else {
                "text"
            };
            fields.push(Fd { name: fname, col: c.name.clone(), ty: t });
        }
        let rows = self.db.rows(&tb.name).map_err(|e| GpkgError(tr!("gpkg.unreadable", why = e.0)))?;
        let pos = |n: &str| info.iter().position(|c| c.name.eq_ignore_ascii_case(n));
        let gpos = pos(&tb.column);
        let fpos: Vec<Option<usize>> = fields.iter().map(|f| pos(&f.col)).collect();
        let mut by: [Vec<Feature>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        let slot = |k: &str| match k {
            "pt" => 0usize,
            "ln" => 1,
            _ => 2,
        };
        let (mut n, mut bad, mut ext, mut multi) = (0usize, 0usize, 0usize, false);
        for r in &rows {
            if n >= max_features {
                notes.push(tr!("gpkg.max", table = ident, n = max_features));
                break;
            }
            let mut p = Map::new();
            for (f, fp) in fields.iter().zip(&fpos) {
                let v = fp.and_then(|i| r.get(i)).cloned().unwrap_or(SVal::Null);
                let v = if matches!(v, SVal::Blob(_)) { SVal::Null } else { v };
                let jv = if v == SVal::Null {
                    Value::Null
                } else if f.ty == "bool" {
                    Value::Bool(truthy(&v))
                } else if f.ty == "number" {
                    let x = sval_number(&v);
                    if x.is_finite() { crate::gjs::num_keep(x) } else { Value::Null }
                } else {
                    Value::String(slice16(&crate::gjs::soft_controls(&sval_string(&v)), 10000))
                };
                p.insert(f.name.clone(), jv);
            }
            n = n.saturating_add(1);
            let blob = match gpos.and_then(|i| r.get(i)) {
                Some(SVal::Blob(b)) => b,
                _ => {
                    if let Some(k) = tb.geom
                        && let Some(l) = by.get_mut(slot(k))
                    {
                        l.push(Feature { g: None, p });
                    }
                    continue;
                }
            };
            match parse_gpb(blob) {
                Err(_) => bad = bad.saturating_add(1),
                Ok(None) => ext = ext.saturating_add(1),
                Ok(Some((gs, mp))) => {
                    if mp {
                        multi = true;
                    }
                    for g in gs {
                        match g {
                            Some(g) => {
                                if let Some(l) = by.get_mut(slot(g.kind())) {
                                    l.push(Feature { g: Some(g), p: p.clone() });
                                }
                            }
                            None => {
                                if let Some(k) = tb.geom
                                    && let Some(l) = by.get_mut(slot(k))
                                {
                                    l.push(Feature { g: None, p: p.clone() });
                                }
                            }
                        }
                    }
                }
            }
        }
        if bad > 0 {
            notes.push(tr!("gpkg.bad", table = ident, n = bad));
        }
        if ext > 0 {
            notes.push(tr!("gpkg.ext", table = ident, n = ext));
        }
        if multi {
            notes.push(tr!("gpkg.multi", table = ident));
        }
        let kinds: Vec<usize> = (0..3).filter(|&i| by.get(i).is_some_and(|l| !l.is_empty())).collect();
        let many = kinds.len() > 1;
        let out_fields: Vec<Field> = fields.iter().map(|f| Field { name: f.name.clone(), ty: f.ty.to_string() }).collect();
        let mut out = Vec::new();
        for i in kinds {
            let features = std::mem::take(by.get_mut(i).unwrap_or(&mut Vec::new()));
            let (k, key) = match i {
                0 => ("pt", "gpkg.layer.pt"),
                1 => ("ln", "gpkg.layer.ln"),
                _ => ("pg", "gpkg.layer.pg"),
            };
            // `tb.z > 0 || by[k].some((f) => hasZ(f.g))`: the JS reads `g.t` of an empty geometry and throws
            let hz = if tb.z > 0.0 {
                true
            } else {
                let mut any = false;
                for f in &features {
                    match &f.g {
                        None => return Err(GpkgError("Cannot read properties of null (reading 't')".into())),
                        Some(g) if crate::gis_geom::has_z(g) => {
                            any = true;
                            break;
                        }
                        Some(_) => {}
                    }
                }
                any
            };
            out.push(ReadLayer {
                name: if many { tr!(key, table = ident) } else { ident.to_string() },
                geom: k.into(),
                has_z: hz,
                fields: out_fields.clone(),
                features,
                notes: notes.clone(),
                extra: Vec::new(),
            });
        }
        Ok(out)
    }
}

/// `parseGpb(bytes)` → Some((geoms, multipoint)) | None ('ext').
#[allow(clippy::type_complexity)]
pub fn parse_gpb(b: &[u8]) -> Result<Option<(Vec<Option<Geom>>, bool)>, GpkgError> {
    if b.len() < 8 || b.first() != Some(&0x47) || b.get(1) != Some(&0x50) {
        return Err(GpkgError(tr!("gpkg.geom.not")));
    }
    let flags = b.get(3).copied().unwrap_or(0);
    if flags & 0x20 != 0 {
        return Ok(None);
    }
    let env = (flags >> 1) & 7;
    if env > 4 {
        return Err(GpkgError(tr!("gpkg.geom.env")));
    }
    let size = [0usize, 32, 48, 48, 64].get(usize::from(env)).copied().unwrap_or(0);
    let empty = (flags >> 4) & 1 != 0;
    let start = 8usize.saturating_add(size);
    if start > b.len() {
        return Err(GpkgError(tr!("gpkg.geom.short")));
    }
    if empty && start >= b.len() {
        return Ok(Some((vec![None], false)));
    }
    let (geoms, _, mp) = read_wkb(b, start).map_err(|e| GpkgError(e.0))?;
    Ok(Some((if empty { vec![None] } else { geoms }, mp)))
}

/// `gpb(g, hasZ, srsId)`.
pub fn gpb(g: &Geom, has_z: bool, srs_id: i32) -> Vec<u8> {
    let w = write_wkb(Some(g), has_z).unwrap_or_default();
    let bx = crate::gis_geom::bounds(Some(g));
    let mut out = vec![0u8; 40usize.saturating_add(w.len())];
    put(&mut out, 0, &[0x47, 0x50, 0, 0x01 | (1 << 1)]);
    put(&mut out, 4, &srs_id.to_le_bytes());
    put(&mut out, 8, &bx.min_x.to_le_bytes());
    put(&mut out, 16, &bx.max_x.to_le_bytes());
    put(&mut out, 24, &bx.min_y.to_le_bytes());
    put(&mut out, 32, &bx.max_y.to_le_bytes());
    put(&mut out, 40, &w);
    let _ = f64le;
    out
}

const WGS84_WKT: &str = "GEOGCS[\"WGS 84\",DATUM[\"WGS_1984\",SPHEROID[\"WGS 84\",6378137,298.257223563,AUTHORITY[\"EPSG\",\"7030\"]],AUTHORITY[\"EPSG\",\"6326\"]],PRIMEM[\"Greenwich\",0,AUTHORITY[\"EPSG\",\"8901\"]],UNIT[\"degree\",0.0174532925199433,AUTHORITY[\"EPSG\",\"9122\"]],AXIS[\"Latitude\",NORTH],AXIS[\"Longitude\",EAST],AUTHORITY[\"EPSG\",\"4326\"]]";

/// A group to write: `{ table, label, geom, hasZ, fields, features }`.
pub struct GGroup<'a> {
    pub table: &'a str,
    pub label: &'a str,
    pub geom: &'a str,
    pub has_z: bool,
    pub fields: &'a [Field],
    pub features: &'a [Feature],
}
/// The project system's row `{ id, org, code, name, definition }`.
pub struct Srs {
    pub id: f64,
    pub org: String,
    pub code: Option<f64>,
    pub name: String,
    pub definition: String,
}

fn q(id: &str) -> String {
    format!("\"{}\"", id.replace('"', "\"\""))
}
/// A JS number as a stored value (INTEGER when whole, else REAL), as SQLite binds a JS number.
fn num_val(x: f64) -> SVal {
    match cc_civil::js::i64_of(x) {
        Some(n) if x.fract() == 0.0 => SVal::Int(n),
        _ => SVal::Real(x),
    }
}

/// `writeGpkg(groups, { srs })` → the file. Fails where SQLite would (a repeated table name, a non-integer id).
pub fn write_gpkg(groups: &[GGroup], srs: Option<&Srs>) -> Result<Vec<u8>, GpkgError> {
    let mut w = Writer::new();
    let mut srs_rows: Vec<(i64, Vec<SVal>)> = vec![
        (
            -1,
            vec![
                SVal::Text("Undefined cartesian SRS".into()),
                SVal::Null,
                SVal::Text("NONE".into()),
                SVal::Int(-1),
                SVal::Text("undefined".into()),
                SVal::Text("undefined cartesian coordinate reference system".into()),
            ],
        ),
        (
            0,
            vec![
                SVal::Text("Undefined geographic SRS".into()),
                SVal::Null,
                SVal::Text("NONE".into()),
                SVal::Int(0),
                SVal::Text("undefined".into()),
                SVal::Text("undefined geographic coordinate reference system".into()),
            ],
        ),
        (
            4326,
            vec![
                SVal::Text("WGS 84 geodetic".into()),
                SVal::Null,
                SVal::Text("EPSG".into()),
                SVal::Int(4326),
                SVal::Text(WGS84_WKT.into()),
                SVal::Text("longitude/latitude coordinates in decimal degrees on the WGS 84 spheroid".into()),
            ],
        ),
    ];
    let mut srs_id: f64 = -1.0;
    if let Some(s) = srs {
        srs_id = s.id;
        if s.id != 4326.0 {
            if !cc_civil::js::is_integer(s.id) {
                return Err(GpkgError("datatype mismatch".into()));
            }
            let id = cc_civil::js::i64_of(s.id).unwrap_or(0);
            if srs_rows.iter().any(|(r, _)| *r == id) {
                return Err(GpkgError("UNIQUE constraint failed: gpkg_spatial_ref_sys.srs_id".into()));
            }
            let name = if s.name.is_empty() { "Project system".to_string() } else { s.name.clone() };
            srs_rows.push((
                id,
                vec![
                    SVal::Text(slice16(&name, 200)),
                    SVal::Null,
                    SVal::Text(if s.org.is_empty() { "NONE".into() } else { s.org.clone() }),
                    num_val(s.code.unwrap_or(s.id)),
                    SVal::Text(if s.definition.is_empty() { "undefined".into() } else { s.definition.clone() }),
                    SVal::Text("the CivilCraft project\u{2019}s coordinate system".into()),
                ],
            ));
        }
    }
    srs_rows.sort_by_key(|(r, _)| *r);
    w.table(
        "gpkg_spatial_ref_sys",
        "CREATE TABLE gpkg_spatial_ref_sys (srs_name TEXT NOT NULL, srs_id INTEGER PRIMARY KEY, organization TEXT NOT NULL, organization_coordsys_id INTEGER NOT NULL, definition TEXT NOT NULL, description TEXT)",
        &srs_rows,
    );
    let mut contents: Vec<(i64, Vec<SVal>)> = Vec::new();
    let mut geomcols: Vec<(i64, Vec<SVal>)> = Vec::new();
    #[allow(clippy::type_complexity)]
    let mut feature_tables: Vec<(String, String, Vec<(i64, Vec<SVal>)>)> = Vec::new();
    let mut used: Vec<String> = Vec::new();
    let mut taken_tables: Vec<String> = Vec::new();
    let srs_val = num_val(srs_id);
    for g in groups {
        let t = g.table;
        if taken_tables.iter().any(|x| x.eq_ignore_ascii_case(t))
            || ["gpkg_spatial_ref_sys", "gpkg_contents", "gpkg_geometry_columns", "sqlite_sequence"]
                .iter()
                .any(|x| x.eq_ignore_ascii_case(t))
        {
            return Err(GpkgError(format!("table {} already exists", q(t))));
        }
        taken_tables.push(t.to_string());
        let ty = match g.geom {
            "pt" => "POINT",
            "ln" => "MULTILINESTRING",
            _ => "MULTIPOLYGON",
        };
        let mut col_names: Vec<String> = vec!["fid".into(), "geom".into()];
        struct Fc<'b> {
            f: &'b Field,
            col: String,
        }
        let mut fcols: Vec<Fc> = Vec::new();
        for f in g.fields {
            let c = {
                let c = crate::gjs::drop_controls(&f.name);
                if c.is_empty() { "field".to_string() } else { c }
            };
            let mut k = c.clone();
            let mut j = 2u64;
            while col_names.contains(&k.to_lowercase()) {
                k = format!("{c}_{j}");
                j = j.saturating_add(1);
            }
            col_names.push(k.to_lowercase());
            fcols.push(Fc { f, col: k });
        }
        let defs: Vec<String> = fcols
            .iter()
            .map(|fc| {
                let t = match fc.f.ty.as_str() {
                    "number" => "REAL",
                    "bool" => "BOOLEAN",
                    "date" => "DATE",
                    _ => "TEXT",
                };
                format!("{} {t}", q(&fc.col))
            })
            .collect();
        let sql = format!(
            "CREATE TABLE {} (fid INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, geom {ty}{})",
            q(t),
            if defs.is_empty() { String::new() } else { format!(", {}", defs.join(", ")) }
        );
        let mut b = BBox::default();
        let mut rows: Vec<(i64, Vec<SVal>)> = Vec::new();
        for (i, f) in g.features.iter().enumerate() {
            let geo = f.g.as_ref().filter(|x| x.kind() == g.geom);
            if let Some(x) = geo {
                bounds_into(Some(x), &mut b);
            }
            let mut vals = vec![
                SVal::Null,
                geo.map_or(SVal::Null, |x| {
                    SVal::Blob(gpb(x, g.has_z, cc_civil::js::i64_of(srs_id).and_then(|n| i32::try_from(n).ok()).unwrap_or(0)))
                }),
            ];
            for fc in &fcols {
                let v = f.p.get(&fc.f.name);
                let sv = match v {
                    None | Some(Value::Null) => SVal::Null,
                    Some(v) => match fc.f.ty.as_str() {
                        "bool" => SVal::Int(i64::from(cc_civil::js::truthy(Some(v)))),
                        "number" => match v.as_f64().filter(|x| x.is_finite() && v.is_number()) {
                            Some(x) => SVal::Real(x),
                            None => SVal::Null,
                        },
                        "date" => numeric_affinity(&cc_civil::js::to_js_string(v)),
                        _ => SVal::Text(cc_civil::js::to_js_string(v)),
                    },
                };
                vals.push(sv);
            }
            rows.push((i64::try_from(i).unwrap_or(0).saturating_add(1), vals));
        }
        let base = slice16(if g.label.is_empty() { t } else { g.label }, 120);
        let mut ident = base.clone();
        let mut j = 2u64;
        while used.contains(&ident) {
            ident = format!("{} {j}", slice16(if g.label.is_empty() { t } else { g.label }, 110));
            j = j.saturating_add(1);
        }
        used.push(ident.clone());
        let ok = b.min_x.is_finite();
        let ext = |v: f64| if ok { num_val(v) } else { SVal::Null };
        let k = i64::try_from(contents.len()).unwrap_or(0).saturating_add(1);
        contents.push((
            k,
            vec![
                SVal::Text(t.into()),
                SVal::Text("features".into()),
                SVal::Text(ident),
                SVal::Text("Exported from CivilCraft".into()),
                SVal::Text("2024-05-17T13:45:30.000Z".into()),
                ext(b.min_x),
                ext(b.min_y),
                ext(b.max_x),
                ext(b.max_y),
                srs_val.clone(),
            ],
        ));
        geomcols.push((
            k,
            vec![
                SVal::Text(t.into()),
                SVal::Text("geom".into()),
                SVal::Text(ty.into()),
                srs_val.clone(),
                SVal::Int(i64::from(g.has_z)),
                SVal::Int(0),
            ],
        ));
        feature_tables.push((t.to_string(), sql, rows));
    }
    w.table(
        "gpkg_contents",
        "CREATE TABLE gpkg_contents (table_name TEXT NOT NULL PRIMARY KEY, data_type TEXT NOT NULL, identifier TEXT UNIQUE, description TEXT DEFAULT '', last_change DATETIME NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')), min_x DOUBLE, min_y DOUBLE, max_x DOUBLE, max_y DOUBLE, srs_id INTEGER, CONSTRAINT fk_gc_r_srs_id FOREIGN KEY (srs_id) REFERENCES gpkg_spatial_ref_sys(srs_id))",
        &contents,
    );
    w.auto_index("gpkg_contents", 1, &contents, &[0]);
    w.auto_index("gpkg_contents", 2, &contents, &[2]);
    w.table(
        "gpkg_geometry_columns",
        "CREATE TABLE gpkg_geometry_columns (table_name TEXT NOT NULL, column_name TEXT NOT NULL, geometry_type_name TEXT NOT NULL, srs_id INTEGER NOT NULL, z TINYINT NOT NULL, m TINYINT NOT NULL, CONSTRAINT pk_geom_cols PRIMARY KEY (table_name, column_name), CONSTRAINT uk_gc_table_name UNIQUE (table_name), CONSTRAINT fk_gc_tn FOREIGN KEY (table_name) REFERENCES gpkg_contents(table_name), CONSTRAINT fk_gc_srs FOREIGN KEY (srs_id) REFERENCES gpkg_spatial_ref_sys (srs_id))",
        &geomcols,
    );
    w.auto_index("gpkg_geometry_columns", 1, &geomcols, &[0, 1]);
    w.auto_index("gpkg_geometry_columns", 2, &geomcols, &[0]);
    let mut seq: Vec<(i64, Vec<SVal>)> = Vec::new();
    let mut first = true;
    for (name, sql, rows) in &feature_tables {
        w.table(name, sql, rows);
        if first {
            first = false;
            // sqlite_sequence comes with the first AUTOINCREMENT table; its rows are written last (below)
        }
        if !rows.is_empty() {
            let k = i64::try_from(seq.len()).unwrap_or(0).saturating_add(1);
            seq.push((k, vec![SVal::Text(name.clone()), SVal::Int(i64::try_from(rows.len()).unwrap_or(0))]));
        }
    }
    if !feature_tables.is_empty() {
        w.table("sqlite_sequence", "CREATE TABLE sqlite_sequence(name,seq)", &seq);
    }
    Ok(w.finish(10400, 1_196_444_487))
}
