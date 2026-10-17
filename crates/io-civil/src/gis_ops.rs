//! The script ops of the GIS file modules (the JS twin is forks/tools/cvdiff/ops-gis.mjs). Bytes travel as base64.

use crate::gis_geom::{self, read_feature, read_fields, read_geom};
use crate::gjs::{b64, unb64};
use cc_civil::gis_state::{Feature, Geom};
use cc_civil::js::{J, arr, as_str, numf};
use cc_civil::script::{Call, Registry};
use serde_json::{Value, json};

type R = Option<Result<Value, String>>;
fn ok<T: J + ?Sized>(v: &T) -> R {
    Some(Ok(v.j()))
}
fn err(e: String) -> R {
    Some(Err(e))
}
/// The fixed local date the harness pins for writers that stamp the time (2024-05-17 13:45:30).
pub const PINNED: crate::zipfile::DosDate = crate::zipfile::DosDate { year: 2024, month: 5, day: 17, hour: 13, minute: 45, second: 30 };
const DBF_DATE: (i32, u32, u32) = (2024, 5, 17);

fn bytes(v: Option<&Value>) -> Vec<u8> {
    unb64(as_str(v).unwrap_or(""))
}
fn opt_bytes(v: Option<&Value>) -> Option<Vec<u8>> {
    as_str(v).map(unb64)
}
fn usize_arg(v: Option<&Value>, d: usize) -> usize {
    v.and_then(Value::as_f64).and_then(cc_civil::js::usize_of).unwrap_or(d)
}
fn ring_of(v: Option<&Value>) -> Vec<Vec<f64>> {
    arr(v).iter().map(|p| arr(Some(p)).iter().map(|x| numf(Some(x))).collect()).collect()
}
fn features(v: Option<&Value>) -> Vec<Feature> {
    arr(v).iter().map(read_feature).collect()
}
/// `toLonLat` from the harness: lon = (x − x0)·k, lat = (y − y0)·k (or identity when absent).
pub fn lonlat(o: Option<&Value>) -> impl Fn(f64, f64) -> (f64, f64) {
    let l = o.and_then(|o| o.get("lonlat"));
    let k = l.and_then(|l| l.get("k")).map_or(1.0, |v| numf(Some(v)));
    let x0 = l.and_then(|l| l.get("x0")).map_or(0.0, |v| numf(Some(v)));
    let y0 = l.and_then(|l| l.get("y0")).map_or(0.0, |v| numf(Some(v)));
    move |x, y| ((x - x0) * k, (y - y0) * k)
}

pub fn handle(_reg: &mut Registry, c: &Call) -> R {
    match c.op {
        "gis.ringArea" => ok(&gis_geom::ring_area(&ring_of(c.arg(0)))),
        "gis.pointInRing" => ok(&gis_geom::point_in_ring(c.num(0), c.num(1), &ring_of(c.arg(2)))),
        "gis.polygonsFromRings" => {
            let rings: Vec<Vec<Vec<f64>>> = arr(c.arg(0)).iter().map(|r| ring_of(Some(r))).collect();
            ok(&gis_geom::polygons_from_rings(rings)
                .into_iter()
                .map(|p| p.into_iter().map(|r| r.into_iter().collect::<Vec<_>>()).collect::<Vec<_>>())
                .collect::<Vec<_>>())
        }
        "gis.orient" => ok(&c.arg(0).and_then(read_geom).map(|g| gis_geom::orient(&g, c.arg(1) == Some(&Value::Bool(true))))),
        "gis.bounds" => ok(&gis_geom::bounds(c.arg(0).and_then(read_geom).as_ref())),
        "gis.geomKind" => ok(&gis_geom::geom_kind(&cc_civil::js::string_or_empty(c.arg(0))).map(str::to_string)),
        "gis.vertexCount" => ok(&gis_geom::vertex_count(c.arg(0).and_then(read_geom).as_ref())),
        "gis.readWkb" => match gis_geom::read_wkb(&bytes(c.arg(0)), usize_arg(c.arg(1), 0)) {
            Ok((g, end, mp)) => Some(Ok(json!({ "geoms": g.j(), "end": end, "multipoint": mp }))),
            Err(e) => err(e.0),
        },
        "gis.writeWkb" => {
            let g: Option<Geom> = c.arg(0).and_then(read_geom);
            match gis_geom::write_wkb(g.as_ref(), c.arg(1) == Some(&Value::Bool(true))) {
                Ok(b) => ok(&b64(&b)),
                Err(e) => err(e.0),
            }
        }
        "dbf.read" => match crate::dbf::read_dbf(&bytes(c.arg(0)), as_str(c.arg(1)), usize_arg(c.arg(2), 2_000_000)) {
            Ok(d) => ok(&d),
            Err(e) => err(e.0),
        },
        "dbf.write" => {
            let recs: Vec<serde_json::Map<String, Value>> =
                arr(c.arg(1)).iter().map(|r| r.as_object().cloned().unwrap_or_default()).collect();
            let refs: Vec<&serde_json::Map<String, Value>> = recs.iter().collect();
            let (b, names) = crate::dbf::write_dbf(&read_fields(c.arg(0)), &refs, DBF_DATE);
            Some(Ok(json!({ "bytes": b64(&b), "names": names })))
        }
        "dbf.names" => ok(&crate::dbf::dbf_names(&arr(c.arg(0)).iter().map(cc_civil::js::to_js_string).collect::<Vec<_>>())),
        "shp.sets" => {
            let files: Vec<(String, Vec<u8>)> =
                arr(c.arg(0)).iter().map(|f| (cc_civil::js::string_or_empty(f.get("name")), bytes(f.get("data")))).collect();
            let sets = crate::shapefile::shapefile_sets(&files);
            Some(Ok(Value::Array(
                sets.iter()
                    .map(|s| {
                        let mut o = serde_json::Map::new();
                        o.insert("name".into(), Value::String(s.name.clone()));
                        for (k, v) in [("shp", &s.shp), ("shx", &s.shx), ("dbf", &s.dbf), ("prj", &s.prj), ("cpg", &s.cpg)] {
                            if let Some(v) = v {
                                o.insert(k.into(), Value::String(b64(v)));
                            }
                        }
                        Value::Object(o)
                    })
                    .collect(),
            )))
        }
        "shp.read" => {
            let s = c.arg(0).cloned().unwrap_or(Value::Null);
            let set = crate::shapefile::ShpSet {
                name: cc_civil::js::string_or_empty(s.get("name")),
                shp: opt_bytes(s.get("shp")),
                shx: opt_bytes(s.get("shx")),
                dbf: opt_bytes(s.get("dbf")),
                prj: opt_bytes(s.get("prj")),
                cpg: opt_bytes(s.get("cpg")),
            };
            match crate::shapefile::read_shapefile(&set, usize_arg(c.arg(1).and_then(|o| o.get("maxFeatures")), 500_000)) {
                Ok(l) => ok(&l),
                Err(e) => err(e.0),
            }
        }
        "shp.write" => {
            let o = c.arg(0).cloned().unwrap_or(Value::Null);
            let out = crate::shapefile::write_shapefile(
                as_str(o.get("geom")).unwrap_or(""),
                o.get("hasZ") == Some(&Value::Bool(true)),
                &read_fields(o.get("fields")),
                &features(o.get("features")),
                DBF_DATE,
            );
            Some(Ok(json!({ "shp": b64(&out.shp), "shx": b64(&out.shx), "dbf": b64(&out.dbf), "names": out.names })))
        }
        "geojson.read" => match crate::geojson::read_geojson(
            c.str(0),
            as_str(c.arg(1)).unwrap_or("GeoJSON"),
            usize_arg(c.arg(2).and_then(|o| o.get("maxFeatures")), 500_000),
        ) {
            Ok(r) => Some(Ok(json!({ "layers": r.layers.j(), "crsCode": r.crs_code.j(), "notes": r.notes }))),
            Err(e) => err(e.0),
        },
        "geojson.write" => {
            let groups: Vec<crate::geojson::WGroup> = arr(c.arg(0))
                .iter()
                .map(|g| crate::geojson::WGroup {
                    kind: cc_civil::js::string_or_empty(g.get("kind")),
                    geom: cc_civil::js::string_or_empty(g.get("geom")),
                    has_z: cc_civil::js::truthy(g.get("hasZ")),
                    fields: read_fields(g.get("fields")),
                    features: features(g.get("features")),
                })
                .collect();
            let o = c.arg(1);
            let zk = o.and_then(|o| o.get("zk")).map_or(1.0, |v| numf(Some(v)));
            let name = o.and_then(|o| o.get("name")).and_then(Value::as_str);
            ok(&crate::geojson::write_geojson(&groups, &lonlat(o), &|z| z * zk, name))
        }
        "kml.read" => match crate::kml::read_kml(
            c.str(0),
            as_str(c.arg(1)).unwrap_or("KML"),
            usize_arg(c.arg(2).and_then(|o| o.get("maxFeatures")), 500_000),
        ) {
            Ok((layers, notes)) => Some(Ok(json!({ "layers": layers.j(), "notes": notes }))),
            Err(e) => err(e.0),
        },
        "kml.write" => {
            let gs: Vec<(String, bool, Vec<Feature>)> = arr(c.arg(0))
                .iter()
                .map(|g| (cc_civil::js::string_or_empty(g.get("kind")), cc_civil::js::truthy(g.get("hasZ")), features(g.get("features"))))
                .collect();
            let groups: Vec<crate::kml::KGroup> =
                gs.iter().map(|(k, z, f)| crate::kml::KGroup { kind: k, has_z: *z, features: f }).collect();
            let o = c.arg(1);
            let zk = o.and_then(|o| o.get("zk")).map_or(1.0, |v| numf(Some(v)));
            let name = o.and_then(|o| o.get("name")).and_then(Value::as_str).unwrap_or("CivilCraft");
            ok(&crate::kml::write_kml(&groups, &lonlat(o), &|z| z * zk, name))
        }
        "zip.crc32" => ok(&crate::zipfile::crc32(&bytes(c.arg(0)))),
        "zip.list" => match crate::zipfile::list_zip(&bytes(c.arg(0))) {
            Ok(l) => ok(&l),
            Err(e) => err(e.0),
        },
        "zip.unzip" => {
            let b = bytes(c.arg(0));
            match crate::zipfile::list_zip(&b) {
                Err(e) => err(e.0),
                Ok(list) => {
                    let mut out = Vec::new();
                    for e in &list {
                        out.push(
                            match crate::zipfile::unzip_entry(
                                &b,
                                e,
                                c.arg(1)
                                    .and_then(Value::as_f64)
                                    .and_then(cc_civil::js::usize_of)
                                    .map_or(crate::zipfile::MAX_ENTRY, |m| m as u64),
                            ) {
                                Ok(d) => json!({ "name": e.name, "data": b64(&d) }),
                                Err(x) => json!({ "name": e.name, "err": x.0 }),
                            },
                        );
                    }
                    Some(Ok(Value::Array(out)))
                }
            }
        }
        "zip.write" => {
            let files: Vec<(String, Vec<u8>)> = arr(c.arg(0))
                .iter()
                .map(|f| {
                    let d = match f.get("text").and_then(Value::as_str) {
                        Some(t) => t.as_bytes().to_vec(),
                        None => bytes(f.get("data")),
                    };
                    (cc_civil::js::string_or_empty(f.get("name")), d)
                })
                .collect();
            let deflate = c.arg(1).and_then(|o| o.get("deflate")) != Some(&Value::Bool(false));
            let z = crate::zipfile::zip(&files, deflate, PINNED);
            Some(Ok(zip_view(&z, deflate)))
        }
        _ => None,
    }
}

/// What the harness compares of a written zip: the bytes when nothing is deflated, else what reading it gives (names,
/// sizes, CRCs, flags, contents), since deflated bytes differ by design (see zipfile.rs).
pub fn zip_view(z: &[u8], deflate: bool) -> Value {
    if !deflate {
        return json!({ "bytes": b64(z) });
    }
    match crate::zipfile::list_zip(z) {
        Err(e) => json!({ "err": e.0 }),
        Ok(list) => Value::Array(
            list.iter()
                .map(|e| {
                    let d = crate::zipfile::unzip_entry(z, e, crate::zipfile::MAX_ENTRY).map(|d| b64(&d)).unwrap_or_else(|x| x.0);
                    json!({ "name": e.name, "size": e.size, "crc": e.crc, "flags": e.flags, "dir": e.dir, "data": d })
                })
                .collect(),
        ),
    }
}

/// JSON → the DWG model's value (for headers and hand-made models in the harness).
pub fn to_v(v: &Value) -> cc_io_dwg::V {
    use cc_io_dwg::V;
    match v {
        Value::Null => V::Null,
        Value::Bool(b) => V::Bool(*b),
        Value::Number(n) => V::Num(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => V::Str(s.clone()),
        Value::Array(a) => V::Arr(a.iter().map(to_v).collect()),
        Value::Object(m) => {
            let mut o = cc_io_dwg::Obj::new();
            for (k, x) in m {
                o.set(k.clone(), to_v(x));
            }
            V::Obj(o)
        }
    }
}

/// The drawing of a `cad.*` op: `{dwg: b64}` (readDwg), `{dxf: b64}` (readDxfModel on its UTF-8 text) or `{model}`.
fn cad_model(v: Option<&Value>) -> Result<cc_io_dwg::Obj, String> {
    let v = v.ok_or("no drawing")?;
    if let Some(m) = v.get("model") {
        return match to_v(m) {
            cc_io_dwg::V::Obj(o) => Ok(o),
            _ => Err("model".into()),
        };
    }
    let mut m = if let Some(d) = v.get("dwg") {
        cc_io_dwg::read_dwg(&bytes(Some(d)), &cc_io_dwg::ReadOpts::default())
    } else {
        let text = crate::gjs::utf8_lossy(&bytes(v.get("dxf")));
        cc_io_dwg::dxf_model::try_read_dxf_model(&text)?
    };
    if let Some(u) = v.get("insunits") {
        // { ...m, header: { ...m.header, INSUNITS: u } }
        let mut h = m.get("header").and_then(cc_io_dwg::V::as_obj).cloned().unwrap_or_default();
        h.set("INSUNITS", to_v(u));
        m.set("header", cc_io_dwg::V::Obj(h));
    }
    Ok(m)
}

pub fn cad_handle(_reg: &mut Registry, c: &Call) -> R {
    match c.op {
        "cad.unitScale" => {
            let h = c.arg(0).map(to_v);
            ok(&crate::cad_linework::unit_scale(h.as_ref().and_then(cc_io_dwg::V::as_obj), c.str(1)))
        }
        "cad.cadUnits" => {
            let h = c.arg(0).map(to_v);
            ok(&crate::cad_linework::cad_units(h.as_ref().and_then(cc_io_dwg::V::as_obj)))
        }
        "cad.ocs" => {
            let e = c.arg(0).map(to_v);
            ok(&crate::cad_linework::ocs(e.as_ref()).map(|m| m.to_vec()))
        }
        "cad.splinePts" => match c.arg(0).map(to_v) {
            Some(cc_io_dwg::V::Obj(o)) => ok(&crate::cad_linework::spline_pts(&o)),
            _ => ok(&Vec::<Vec<f64>>::new()),
        },
        "cad.attrNumber" => ok(&crate::block_points::attr_number(&cc_civil::js::string_or_empty(c.arg(0)))),
        "cad.cleanBlk" => ok(&c.arg(0).and_then(crate::block_points::clean_blk)),
        "cad.linework" => {
            let m = match cad_model(c.arg(0)) {
                Ok(m) => m,
                Err(e) => return err(e),
            };
            let o = c.arg(1);
            let get = |k: &str| o.and_then(|o| o.get(k)).filter(|v| !v.is_null());
            let only: Option<Vec<String>> = get("layers").map(|l| arr(Some(l)).iter().map(cc_civil::js::to_js_string).collect());
            let scale = get("scale").map(|v| numf(Some(v)));
            ok(&crate::cad_linework::cad_linework(
                &m,
                as_str(get("file")).unwrap_or(""),
                as_str(get("projectUnit")).unwrap_or("ft"),
                scale,
                only.as_deref(),
            ))
        }
        "cad.blockRefs" | "cad.pointsFromBlocks" => {
            let m = match cad_model(c.arg(0)) {
                Ok(m) => m,
                Err(e) => return err(e),
            };
            let o = c.arg(1);
            let get = |k: &str| o.and_then(|o| o.get(k)).filter(|v| !v.is_null());
            let r = crate::block_points::block_refs(&m, as_str(get("projectUnit")).unwrap_or("ft"), get("scale").map(|v| numf(Some(v))));
            if c.op == "cad.blockRefs" {
                return ok(&r);
            }
            let p = c.arg(2);
            let pg = |k: &str| p.and_then(|o| o.get(k)).filter(|v| !v.is_null());
            let strs = |k: &str| -> Vec<String> { arr(pg(k)).iter().map(cc_civil::js::to_js_string).collect() };
            let (blocks, layers) = (strs("blocks"), strs("layers"));
            let existing: Vec<String> = arr(pg("existing")).iter().map(|e| cc_civil::js::string_of(e.get("n"))).collect();
            let fb = crate::block_points::FromBlocks {
                blocks: &blocks,
                layers: &layers,
                desc: as_str(pg("desc")).unwrap_or("block"),
                z: as_str(pg("z")).unwrap_or("insert"),
                number: as_str(pg("number")).unwrap_or("next"),
                existing: &existing,
                start: pg("start").map(|v| numf(Some(v))),
            };
            let (points, no_z, no_number, renumbered) = crate::block_points::points_from_blocks(&r.refs, &fb);
            Some(Ok(json!({ "points": points.j(), "skipped": { "noZ": no_z, "noNumber": no_number }, "renumbered": renumbered })))
        }
        _ => None,
    }
}

/// A mapping the harness names (the same on both sides): affine, through web Mercator, cut off past a column…
pub fn mapping(spec: Option<&Value>) -> impl Fn(f64, f64) -> Option<(f64, f64)> {
    let kind = as_str(spec.and_then(|s| s.get("kind"))).unwrap_or("id").to_string();
    let m: Vec<f64> = arr(spec.and_then(|s| s.get("m"))).iter().map(|x| numf(Some(x))).collect();
    let at = spec.and_then(|s| s.get("at")).map_or(f64::INFINITY, |v| numf(Some(v)));
    let g = move |i: usize| m.get(i).copied().unwrap_or(f64::NAN);
    move |u: f64, v: f64| -> Option<(f64, f64)> {
        match kind.as_str() {
            "affine" => Some((g(0) * u + g(2) * v + g(4), g(1) * u + g(3) * v + g(5))),
            // pixel → web Mercator (through the affine m) → lon/lat
            "merc" => {
                let (x, y) = (g(0) * u + g(2) * v + g(4), g(1) * u + g(3) * v + g(5));
                let (lon, lat) = crate::basemap::merc_to_lon_lat(x, y);
                Some((lon * 1000.0, lat * 1000.0))
            }
            "ll2merc" => Some(crate::basemap::lon_lat_to_merc(u, v)),
            "merc2ll" => Some(crate::basemap::merc_to_lon_lat(u, v)),
            "cut" => {
                if u > at {
                    None
                } else {
                    Some((u, v))
                }
            }
            _ => Some((u, v)),
        }
    }
}
fn bbox4(v: Option<&Value>) -> [f64; 4] {
    let a = arr(v);
    [numf(a.first()), numf(a.get(1)), numf(a.get(2)), numf(a.get(3))]
}
fn grid_of(v: Option<&Value>) -> Result<crate::basemap::Grid, String> {
    match v {
        Some(Value::String(s)) if s == "XYZ" => Ok(crate::basemap::xyz_grid()),
        Some(g) => {
            // a grid as JSON (open-ended sizes as "$Inf")
            let code = g.get("code").and_then(Value::as_f64);
            let o = arr(g.get("origin"));
            let matrices = arr(g.get("matrices"))
                .iter()
                .map(|m| crate::basemap::Matrix {
                    id: cc_civil::js::string_or_empty(m.get("id")),
                    res: numf(m.get("res")),
                    tw: numf(m.get("tw")),
                    th: numf(m.get("th")),
                    mw: numf(m.get("mw")),
                    mh: numf(m.get("mh")),
                    origin: m.get("origin").map(|o| (numf(arr(Some(o)).first()), numf(arr(Some(o)).get(1)))),
                })
                .collect();
            Ok(crate::basemap::Grid { code, origin: (numf(o.first()), numf(o.get(1))), matrices })
        }
        None => Err("no grid".into()),
    }
}
fn map_res<T: J>(r: Result<T, crate::basemap::MapError>) -> R {
    match r {
        Ok(v) => ok(&v),
        Err(e) => err(e.0),
    }
}

pub fn map_handle(_reg: &mut Registry, c: &Call) -> R {
    use crate::basemap as B;
    let s = |i: usize| cc_civil::js::string_or_empty(c.arg(i));
    let o = |k: &str| c.arg(0).and_then(|o| o.get(k)).filter(|v| !v.is_null());
    let os = |k: &str, d: &str| o(k).map_or_else(|| d.to_string(), cc_civil::js::to_js_string);
    match c.op {
        "map.lonLatToMerc" => {
            let (x, y) = B::lon_lat_to_merc(c.num(0), c.num(1));
            ok(&vec![x, y])
        }
        "map.mercToLonLat" => {
            let (x, y) = B::merc_to_lon_lat(c.num(0), c.num(1));
            ok(&vec![x, y])
        }
        "map.tileAt" => {
            let (a, b) = B::tile_at(c.num(0), c.num(1), c.arg(2).and_then(Value::as_f64).and_then(cc_civil::js::usize_of).unwrap_or(0));
            ok(&vec![a, b])
        }
        "map.xyzGrid" => ok(&B::xyz_grid()),
        "map.pickMatrix" => match grid_of(c.arg(0)) {
            Ok(g) => ok(&B::pick_matrix(&g, c.num(1))),
            Err(e) => err(e),
        },
        "map.gridTiles" | "map.gridTilesAny" => match grid_of(c.arg(0)) {
            Ok(g) => {
                let cap = c.arg(3).and_then(|o| o.get("cap")).map_or(64.0, |v| numf(Some(v)));
                let (m, t) = if c.op == "map.gridTiles" {
                    B::grid_tiles(&g, bbox4(c.arg(1)), c.num(2), cap)
                } else {
                    B::grid_tiles_any(&g, bbox4(c.arg(1)), c.num(2), cap)
                };
                Some(Ok(json!({ "matrix": m.j(), "tiles": t.j() })))
            }
            Err(e) => err(e),
        },
        "map.codeOfCrs" => ok(&B::code_of_crs(&s(0)).map(|(code, lon, c84)| json!({ "code": code.j(), "lonFirst": lon, "crs84": c84 }))),
        "map.clean" => ok(&B::clean(&s(0), c.arg(1).and_then(Value::as_f64).and_then(cc_civil::js::usize_of).unwrap_or(500))),
        "map.parseWms" => map_res(B::parse_wms_capabilities(c.str(0))),
        "map.parseWmts" => map_res(B::parse_wmts_capabilities(c.str(0))),
        "map.isLatFirst" => ok(&B::is_lat_first(
            c.arg(0).and_then(|k| k.get("code")).and_then(Value::as_f64),
            c.arg(0).and_then(|k| k.get("crs84")) == Some(&Value::Bool(true)),
        )),
        "map.wmsPickCode" => {
            ok(&B::wms_pick_code(&arr(c.arg(0)).iter().map(|x| numf(Some(x))).collect::<Vec<_>>(), c.arg(1).and_then(Value::as_f64)))
        }
        "map.wmsGetMapUrl" => map_res(B::wms_get_map_url(
            &os("url", ""),
            &os("version", "1.3.0"),
            &os("layer", "undefined"),
            &os("style", ""),
            &os("format", "image/png"),
            numf(o("code")),
            bbox4(o("bbox")),
            numf(o("width")),
            numf(o("height")),
            o("transparent") != Some(&Value::Bool(false)),
        )),
        "map.wmtsGrid" => {
            let set = c.arg(0).cloned().unwrap_or(Value::Null);
            let opts = c.arg(2);
            let to_set = opts.and_then(|o| o.get("toSet")).map(|sp| mapping(Some(sp)));
            let bb = opts.and_then(|o| o.get("bbox84")).filter(|v| v.is_array()).map(|v| bbox4(Some(v)));
            let f = to_set.as_ref().map(|f| f as &dyn Fn(f64, f64) -> Option<(f64, f64)>);
            ok(&B::wmts_grid(&set, &s(1), f, bb))
        }
        "map.wmtsTileUrl" => map_res(B::wmts_tile_url(
            &os("template", ""),
            &os("kvp", ""),
            &os("layer", "undefined"),
            &os("style", "default"),
            &os("format", "image/png"),
            &os("set", "undefined"),
            &os("matrix", "undefined"),
            &os("row", "undefined"),
            &os("col", "undefined"),
        )),
        "map.arcgisBase" => map_res(B::arcgis_base(&s(0)).map(|(b, k)| json!({ "base": b, "kind": k }))),
        "map.parseArcgis" => map_res(B::parse_arcgis_service(c.arg(0).unwrap_or(&Value::Null), &s(1))),
        "map.arcgisExportUrl" => map_res(B::arcgis_export_url(
            &os("base", ""),
            &os("kind", "MapServer"),
            bbox4(o("bbox")),
            numf(o("code")),
            numf(o("width")),
            numf(o("height")),
        )),
        "map.arcgisTileUrl" => map_res(B::arcgis_tile_url(&s(0), &s(1), &s(2), &s(3))),
        "map.checkTemplate" => map_res(B::check_template(&s(0))),
        "map.xyzUrl" => ok(&B::xyz_url(
            &s(0),
            c.num(1),
            c.num(2),
            c.num(3),
            &c.arg(4).map_or_else(
                || "abc".to_string(),
                |v| if cc_civil::js::truthy(Some(v)) { cc_civil::js::to_js_string(v) } else { String::new() },
            ),
        )),
        "map.checkUrl" => map_res(B::check_url(&s(0)).map(|u| u.as_str().to_string())),
        "map.capabilitiesUrl" => map_res(B::capabilities_url(&s(0), &s(1))),
        "map.parseWorldFile" => map_res(B::parse_world_file(&s(0)).map(|g| g.to_vec())),
        "map.worldFileFor" => ok(&B::world_file_for(&s(0))),
        "map.affineFrom3" => {
            let p = |i: usize| -> [[f64; 2]; 3] {
                let a = arr(c.arg(i));
                let q = |k: usize| {
                    let x = arr(a.get(k));
                    [numf(x.first()), numf(x.get(1))]
                };
                [q(0), q(1), q(2)]
            };
            ok(&B::affine_from3(p(0), p(1)).map(|m| m.to_vec()))
        }
        "map.planWarp" => {
            let f = mapping(c.arg(0));
            let o = c.arg(3);
            let g = |k: &str, d: f64| o.and_then(|o| o.get(k)).map_or(d, |v| numf(Some(v)));
            let md = cc_civil::js::usize_of(g("maxDepth", 6.0)).and_then(|x| u32::try_from(x).ok()).unwrap_or(6);
            let mc = cc_civil::js::usize_of(g("minCells", 0.0)).and_then(|x| u32::try_from(x).ok()).unwrap_or(0);
            ok(&B::plan_warp(&f, c.num(1), c.num(2), g("tol", 0.25), md, mc))
        }
        "map.viewSourceBox" => {
            let f = mapping(c.arg(1));
            let vb =
                c.arg(0).map_or([f64::NAN; 4], |v| [numf(v.get("minX")), numf(v.get("minY")), numf(v.get("maxX")), numf(v.get("maxY"))]);
            ok(&B::view_source_box(
                vb,
                &f,
                c.arg(2).and_then(Value::as_f64).and_then(cc_civil::js::usize_of).and_then(|x| u32::try_from(x).ok()).unwrap_or(8),
            )
            .map(|b| b.to_vec()))
        }
        "map.readMaps" => {
            let images: Option<Vec<String>> = c.arg(1).map(|l| arr(Some(l)).iter().map(cc_civil::js::to_js_string).collect());
            let mut notes = Vec::new();
            let r = B::read_maps(c.arg(0).unwrap_or(&Value::Null), &mut notes, images.as_deref());
            Some(Ok(json!({ "maps": r, "notes": notes })))
        }
        "map.readGrid" => map_res(B::read_grid(c.arg(0).unwrap_or(&Value::Null))),
        "map.gridJson" => match grid_of(c.arg(0)) {
            Ok(g) => ok(&B::grid_json(&g)),
            Err(e) => err(e),
        },
        "map.checkImageData" => ok(&B::check_image_data(c.arg(0).unwrap_or(&Value::Null))),
        _ => None,
    }
}

/// What the harness compares of a GeoPackage: its tables and every table's layers as the reader gives them.
pub fn gpkg_view(b: &[u8], max: usize) -> Result<Value, String> {
    let gp = crate::gpkg::open_gpkg(b).map_err(|e| e.0)?;
    let mut layers = serde_json::Map::new();
    for t in &gp.tables {
        let v = match gp.read(&t.name, max) {
            Ok(ls) => ls.j(),
            Err(e) => json!({ "err": e.0 }),
        };
        layers.insert(t.name.clone(), v);
    }
    Ok(json!({ "tables": gp.tables.j(), "layers": Value::Object(layers) }))
}
#[allow(clippy::type_complexity)]
fn ggroups(v: Option<&Value>) -> Vec<(String, String, String, bool, Vec<cc_civil::gis_state::Field>, Vec<Feature>)> {
    arr(v)
        .iter()
        .map(|g| {
            (
                cc_civil::js::string_or_empty(g.get("table")),
                cc_civil::js::string_or_empty(g.get("label")),
                cc_civil::js::string_or_empty(g.get("geom")),
                cc_civil::js::truthy(g.get("hasZ")),
                read_fields(g.get("fields")),
                features(g.get("features")),
            )
        })
        .collect()
}
fn gsrs(v: Option<&Value>) -> Option<crate::gpkg::Srs> {
    let s = v.filter(|s| s.is_object())?;
    Some(crate::gpkg::Srs {
        id: numf(s.get("id")),
        org: if cc_civil::js::truthy(s.get("org")) {
            cc_civil::js::to_js_string(s.get("org").unwrap_or(&Value::Null))
        } else {
            String::new()
        },
        code: s.get("code").filter(|c| !c.is_null()).map(|c| numf(Some(c))),
        name: if cc_civil::js::truthy(s.get("name")) {
            cc_civil::js::to_js_string(s.get("name").unwrap_or(&Value::Null))
        } else {
            String::new()
        },
        definition: if cc_civil::js::truthy(s.get("definition")) {
            cc_civil::js::to_js_string(s.get("definition").unwrap_or(&Value::Null))
        } else {
            String::new()
        },
    })
}
fn write_gpkg_bytes(c: &Call) -> Result<Vec<u8>, String> {
    let gs = ggroups(c.arg(0));
    let groups: Vec<crate::gpkg::GGroup> = gs
        .iter()
        .map(|(t, l, g, z, f, fe)| crate::gpkg::GGroup { table: t, label: l, geom: g, has_z: *z, fields: f, features: fe })
        .collect();
    let srs = gsrs(c.arg(1));
    crate::gpkg::write_gpkg(&groups, srs.as_ref()).map_err(|e| e.0)
}

pub fn gpkg_handle(_reg: &mut Registry, c: &Call) -> R {
    match c.op {
        "gpkg.read" => match gpkg_view(&bytes(c.arg(0)), usize_arg(c.arg(1).and_then(|o| o.get("maxFeatures")), 500_000)) {
            Ok(v) => Some(Ok(v)),
            Err(e) => err(e),
        },
        "gpkg.parseGpb" => match crate::gpkg::parse_gpb(&bytes(c.arg(0))) {
            Ok(None) => ok(&"ext".to_string()),
            Ok(Some((g, mp))) => Some(Ok(json!({ "geoms": g.j(), "multipoint": mp }))),
            Err(e) => err(e.0),
        },
        "gpkg.gpb" => match c.arg(0).and_then(read_geom) {
            Some(g) => ok(&b64(&crate::gpkg::gpb(
                &g,
                c.arg(1) == Some(&Value::Bool(true)),
                c.arg(2).and_then(Value::as_f64).map_or(0, |x| cc_civil::js::i64_of(x).and_then(|n| i32::try_from(n).ok()).unwrap_or(0)),
            ))),
            None => err("no geometry".into()),
        },
        "gpkg.write" => match write_gpkg_bytes(c).and_then(|b| gpkg_view(&b, 500_000)) {
            Ok(v) => Some(Ok(v)),
            Err(e) => err(e),
        },
        "gpkg.writeBytes" => match write_gpkg_bytes(c) {
            Ok(b) => ok(&b64(&b)),
            Err(e) => err(e),
        },
        _ => None,
    }
}

fn files_arg(v: Option<&Value>) -> Vec<(String, Vec<u8>)> {
    arr(v)
        .iter()
        .map(|f| {
            (
                cc_civil::js::string_or_empty(f.get("name")),
                match f.get("text").and_then(Value::as_str) {
                    Some(t) => t.as_bytes().to_vec(),
                    None => bytes(f.get("data")),
                },
            )
        })
        .collect()
}
fn egroups(v: Option<&Value>) -> Vec<crate::gis_export::EGroup> {
    arr(v)
        .iter()
        .map(|g| crate::gis_export::EGroup {
            key: cc_civil::js::string_or_empty(g.get("key")),
            label: cc_civil::js::string_or_empty(g.get("label")),
            kind: cc_civil::js::string_or_empty(g.get("kind")),
            file: cc_civil::js::string_or_empty(g.get("file")),
            geom: cc_civil::js::string_or_empty(g.get("geom")),
            has_z: cc_civil::js::truthy(g.get("hasZ")),
            fields: read_fields(g.get("fields")),
            features: features(g.get("features")),
        })
        .collect()
}

/// The harness's project system: an EPSG code from the catalog (null → no system).
fn code_crs(v: Option<&Value>) -> Result<Value, String> {
    match v.and_then(Value::as_f64) {
        None => Ok(Value::Null),
        Some(c) => Ok(crate::crs::by_code(c)?.map_or(Value::Null, crate::crs::crs_from_entry)),
    }
}
/// buildLayers override specs: `{ kind: 'same' | 'none' | 'wgs84' }` or `{ code }` (through resolveSrc).
fn src_spec(v: &Value) -> crate::gis_read::Src {
    match v.get("code") {
        Some(c) => crate::gis_read::resolve_src(None, Some(numf(Some(c))), None),
        None => crate::gis_read::read_src(v),
    }
}

pub fn gisx_handle(_reg: &mut Registry, c: &Call) -> R {
    match c.op {
        "gread.isGisFile" => ok(&crate::gis_read::is_gis_file(c.str(0))),
        "gread.gisFileKind" => ok(&crate::gis_read::gis_file_kind(c.str(0), c.str(1)).map(str::to_string)),
        "gread.resolveSrc" => {
            let o = c.arg(0);
            let g = |k: &str| o.and_then(|o| o.get(k)).filter(|v| !v.is_null());
            let srs = g("srs").map(|s| {
                (cc_civil::js::string_or_empty(s.get("org")), numf(s.get("code")), cc_civil::js::string_or_empty(s.get("definition")))
            });
            ok(&crate::gis_read::resolve_src(
                g("prj").and_then(Value::as_str),
                g("code").map(|v| cc_civil::js::number(Some(v))),
                srs.as_ref().map(|(a, b, d)| (a.as_str(), *b, d.as_str())),
            ))
        }
        "gread.readGisFiles" => {
            let (s, n) =
                crate::gis_read::read_gis_files(&files_arg(c.arg(0)), usize_arg(c.arg(1).and_then(|o| o.get("maxFeatures")), 500_000));
            Some(Ok(json!({ "sources": s.j(), "notes": n })))
        }
        "gread.planImport" => {
            let (srcs, _) = crate::gis_read::read_gis_files(&files_arg(c.arg(0)), 500_000);
            let st = c.arg(1);
            let crs = match code_crs(st.and_then(|o| o.get("code"))) {
                Ok(v) => v,
                Err(e) => return err(e),
            };
            Some(Ok(crate::gis_read::plan_import(&crs, as_str(st.and_then(|o| o.get("units"))).unwrap_or("ft"), &srcs)))
        }
        "gread.buildLayers" => {
            let (srcs, _) = crate::gis_read::read_gis_files(&files_arg(c.arg(0)), 500_000);
            let o = c.arg(1);
            let project = match code_crs(o.and_then(|o| o.get("code"))) {
                Ok(v) => v,
                Err(e) => return err(e),
            };
            let units = o.and_then(|o| o.get("units")).and_then(Value::as_str).unwrap_or("ft");
            let ov: Vec<(usize, crate::gis_read::Src)> = arr(o.and_then(|o| o.get("overrides")))
                .iter()
                .filter_map(|p| {
                    Some((arr(Some(p)).first().and_then(Value::as_f64).and_then(cc_civil::js::usize_of)?, src_spec(arr(Some(p)).get(1)?)))
                })
                .collect();
            let (l, n) = crate::gis_read::build_layers(&srcs, &project, units, &ov);
            Some(Ok(json!({ "layers": l.j(), "notes": n })))
        }
        "gread.middleLonLat" => {
            let (srcs, _) = crate::gis_read::read_gis_files(&files_arg(c.arg(0)), 500_000);
            ok(&crate::gis_read::middle_lon_lat(&srcs).map(|a| a.to_vec()))
        }
        "gread.latLonPoints" => {
            let pts: Vec<cc_civil::points::Pt> = arr(c.arg(0)).iter().map(cc_civil::points::read_pt).collect();
            let crs = match code_crs(c.arg(1)) {
                Ok(v) => v,
                Err(e) => return err(e),
            };
            match crate::gis_read::lat_lon_points(&pts, &crs) {
                Ok((p, bad)) => Some(Ok(json!({ "points": p.j(), "bad": bad }))),
                Err(e) => err(e),
            }
        }
        "gread.crsFromLandXml" => {
            let (crs, why) = crate::gis_read::crs_from_landxml(c.arg(0));
            Some(Ok(json!({ "crs": crs, "why": why })))
        }
        "gexp.safeName" => {
            let mut used: Vec<String> = Vec::new();
            for u in arr(c.arg(1)).iter().map(cc_civil::js::to_js_string) {
                if !used.contains(&u) {
                    used.push(u);
                }
            }
            let n = crate::gis_export::safe_name(&cc_civil::js::string_or_empty(c.arg(0)), &mut used);
            Some(Ok(json!({ "name": n, "used": used })))
        }
        "gexp.srsFor" => ok(&crate::gis_export::srs_for(c.arg(0).unwrap_or(&Value::Null), as_str(c.arg(1)))
            .map(|s| json!({ "id": s.id.j(), "org": s.org, "code": s.code.j(), "name": s.name, "definition": s.definition }))),
        "gexp.writeExport" => {
            let o = c.arg(1);
            let g = |k: &str| o.and_then(|o| o.get(k)).filter(|v| !v.is_null());
            let ll = lonlat(o);
            let has_ll = g("lonlat").is_some();
            let zk = g("zk").map_or(1.0, |v| numf(Some(v)));
            let srs = gsrs(g("srs"));
            let format = as_str(g("format")).unwrap_or("");
            let r = crate::gis_export::write_export(
                &egroups(c.arg(0)),
                format,
                as_str(g("prj")),
                if has_ll { Some(&ll) } else { None },
                &|z| z * zk,
                as_str(g("stem")).unwrap_or("site"),
                srs.as_ref(),
                PINNED,
            );
            match r {
                Err(e) => Some(Ok(json!({ "err": e }))),
                Ok(x) => {
                    let view = match format {
                        "shp" | "kmz" => zip_view(&x.bytes, true),
                        "gpkg" => gpkg_view(&x.bytes, 500_000).unwrap_or_else(|e| json!({ "err": e })),
                        _ => Value::String(b64(&x.bytes)),
                    };
                    Some(Ok(json!({ "name": x.name, "type": x.ty, "notes": x.notes, "view": view })))
                }
            }
        }
        "glayers.tokenLabel" => ok(&crate::gis_layers::token_label(c.str(0))),
        "glayers.labelText" => ok(&crate::gis_layers::label_text(c.raw(0))),
        "glayers.labelItems" | "glayers.useChoices" | "glayers.layerBounds" => {
            let (g, fs) = cc_civil::gis_state::read_gis_state(c.arg(0), c.arg(1), &mut Vec::new());
            let Some(l) = g.layers.iter().find(|l| l.id == c.str(2)) else { return Some(Ok(Value::Null)) };
            let f = crate::gis_layers::features_of(&fs, &l.id);
            match c.op {
                "glayers.labelItems" => ok(&crate::gis_layers::label_items(l, f, c.num(3))),
                "glayers.useChoices" => ok(&crate::gis_layers::use_choices(l, f)),
                _ => ok(&crate::gis_layers::layer_bounds(f)),
            }
        }
        "glayers.gisExtents" | "glayers.pickFeature" | "glayers.gisScene" => {
            let (g, fs) = cc_civil::gis_state::read_gis_state(c.arg(0), c.arg(1), &mut Vec::new());
            match c.op {
                "glayers.gisExtents" => ok(&crate::gis_layers::gis_extents(&g.layers, &fs)),
                "glayers.pickFeature" => ok(&crate::gis_layers::pick_feature(&g.layers, &fs, c.num(2), c.num(3), c.num(4))
                    .map(|(l, i)| json!({ "kind": "gis", "layer": l, "i": i }))),
                _ => Some(Ok(crate::gis_layers::gis_scene(&g.layers, &fs, c.num(2)))),
            }
        }
        _ => None,
    }
}
